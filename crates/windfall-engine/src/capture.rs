//! Selectable analyzer captures: the mix output or one mixer track after its
//! fader, copied on the audio thread into a prepared analyzer tap.
//!
//! A stream has [`CAPTURE_SEATS`] seats. Each seat is one of the analyzer
//! foundation's tap slots plus a fixed queue for what a chosen effect
//! measured. The control side prepares a tap for a source and stages it; the
//! audio thread takes it over between two device buffers and from then on
//! copies that source's audio into it, 256 frames at most per call. Nothing
//! here allocates, frees, locks, waits or does any analysis on the audio
//! thread: a tap that is replaced goes back to the control side through the
//! slot's own retirement queue.
//!
//! What is copied:
//!
//! - [`Source::MixOutput`]: the finished output after the output gain and
//!   before non-finite samples are replaced by silence for the device, so a
//!   broken signal is reported as broken and not as healthy silence.
//! - [`Source::TrackPostFader`]: the track's buffer once its effects, its
//!   integrated processing, its fader and its pan have run, before it is
//!   added to anything else. The master track's is the master without the
//!   output gain and without what goes past the mixer.
//!
//! The control side is in `capture_control.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};
use windfall_dsp::EffectObservation;
use windfall_project::EffectId;

use crate::analyzers::{
    self, AudioTapSlot, ClockEpoch, GainReduction, PrepareError, Source, Stamp, TapInstaller,
};
use crate::mixer::{Frame, MAX_BLOCK, Mixer};
use crate::plan::Plan;
use crate::ramp::Ramp;
use crate::state::PlanState;

/// Sources that can be captured at once on one stream.
pub const CAPTURE_SEATS: usize = 2;

/// Effect observations one seat holds for the control side to collect. At
/// one per 256-frame block that is over a second at 48 kHz.
pub const CAPTURE_OBSERVATIONS: usize = 256;

/// No effect is bound.
const UNBOUND: u64 = 0;

/// One samples-per-bit mask over a whole block, both sides.
const MASK_WORDS: usize = MAX_BLOCK * 2 / 64;

/// What the audio thread counts for a seat. Each has one writer.
#[derive(Default)]
pub(crate) struct SeatShared {
    /// The selection the effect binding belongs to, or [`UNBOUND`]. Written
    /// last when binding and first when unbinding, so a reader that sees the
    /// same ticket before and after reading the effect has a matching pair.
    bound_ticket: AtomicU64,
    bound_effect: AtomicU32,
    /// Observations that found the queue full.
    pub dropped_observations: AtomicU64,
    /// Blocks in which the selected track was not in the plan.
    pub missing_source_blocks: AtomicU64,
    /// Blocks not published because the source's latency is more than the
    /// analyzer's one second.
    pub latency_refusals: AtomicU64,
    /// Blocks the tap refused: a full queue, a cancelled tap.
    pub refused_blocks: AtomicU64,
}

impl SeatShared {
    pub(crate) fn bind(&self, ticket: u64, effect: Option<EffectId>) {
        self.bound_ticket.store(UNBOUND, Ordering::Release);
        if let Some(effect) = effect {
            self.bound_effect.store(effect.0, Ordering::Release);
            self.bound_ticket.store(ticket, Ordering::Release);
        }
    }

    fn bound(&self, ticket: u64) -> Option<EffectId> {
        if self.bound_ticket.load(Ordering::Acquire) != ticket {
            return None;
        }
        let effect = self.bound_effect.load(Ordering::Acquire);
        (self.bound_ticket.load(Ordering::Acquire) == ticket).then_some(EffectId(effect))
    }
}

fn count(counter: &AtomicU64) {
    counter.store(
        counter.load(Ordering::Relaxed).saturating_add(1),
        Ordering::Relaxed,
    );
}

/// What a bound effect measured over one block, with the block's place on
/// the engine's clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapturedObservation {
    /// The selection it was captured for.
    pub selection: u64,
    pub effect: EffectId,
    pub clock: u64,
    pub first_frame: u64,
    pub frames: u32,
    pub sample_rate: u32,
    pub observation: EffectObservation,
}

struct AudioSeat {
    slot: AudioTapSlot,
    shared: Arc<SeatShared>,
    observations: Producer<CapturedObservation>,
    /// Counts the discontinuities of this stream, so no analysis window
    /// spans a seek, a transport change or a new plan.
    clock: u64,
    /// The observation serial last sent on, so one that the effect did not
    /// renew, because it is switched off, is not sent again.
    last_serial: u64,
    /// The seat's tap listens to the mix output. Kept up to date where the
    /// tap is taken over, so asking costs nothing.
    mix: bool,
    exhausted: bool,
}

/// The audio thread's half of a stream's captures.
pub(crate) struct AudioCaptures {
    seats: [AudioSeat; CAPTURE_SEATS],
    /// Which samples of the output block were not finite before the output
    /// gain. Prepared with the processor, so marking allocates nothing.
    invalid: [u64; MASK_WORDS],
}

/// The control side's half of one seat, as it comes out of preparation.
pub(crate) struct SeatEndpoints {
    pub installer: TapInstaller,
    pub shared: Arc<SeatShared>,
    pub observations: Consumer<CapturedObservation>,
}

/// Prepares the seats of a new stream. Allocates, so it is for the control
/// side, outside every lock. Refused when the process is out of tap slots.
pub(crate) fn prepare() -> Result<(AudioCaptures, [SeatEndpoints; CAPTURE_SEATS]), PrepareError> {
    let mut audio = Vec::with_capacity(CAPTURE_SEATS);
    let mut control = Vec::with_capacity(CAPTURE_SEATS);
    for _ in 0..CAPTURE_SEATS {
        let (slot, installer) = analyzers::tap_slot()?;
        let shared = Arc::new(SeatShared::default());
        let (tx, rx) = RingBuffer::new(CAPTURE_OBSERVATIONS);
        audio.push(AudioSeat {
            slot,
            shared: shared.clone(),
            observations: tx,
            clock: 0,
            last_serial: 0,
            mix: false,
            exhausted: false,
        });
        control.push(SeatEndpoints {
            installer,
            shared,
            observations: rx,
        });
    }
    let (Ok(seats), Ok(control)) = (audio.try_into(), control.try_into()) else {
        unreachable!("exactly CAPTURE_SEATS seats were prepared");
    };
    Ok((
        AudioCaptures {
            seats,
            invalid: [0; MASK_WORDS],
        },
        control,
    ))
}

impl AudioCaptures {
    /// Takes over whatever the control side staged. Once per device buffer,
    /// before any of it is processed.
    pub fn boundary(&mut self) {
        for seat in &mut self.seats {
            if seat.slot.boundary() {
                seat.last_serial = 0;
                seat.mix = seat
                    .slot
                    .active()
                    .is_some_and(|tap| tap.selection().source == Source::MixOutput);
            }
        }
    }

    /// Marks a break in the stream: what was captured before it and what
    /// comes after it are never analyzed as one piece.
    pub fn reset(&mut self) {
        for seat in &mut self.seats {
            let Some(clock) = seat.clock.checked_add(1) else {
                seat.exhausted = true;
                continue;
            };
            seat.clock = clock;
            if let Some(tap) = seat.slot.active()
                && tap.reset().is_err()
            {
                seat.exhausted = true;
            }
        }
    }

    /// Whether any seat listens to the mix output.
    pub fn wants_mix(&self) -> bool {
        self.seats.iter().any(|seat| seat.mix)
    }

    /// Copies each selected track's finished block, and sends on what a
    /// bound effect measured over it. Call after the mixer has run and
    /// before anything reuses its buffers.
    pub fn publish_tracks(
        &mut self,
        plan: &Plan,
        state: &PlanState,
        mixer: &Mixer,
        base: u64,
        rate: u32,
        frames: usize,
    ) {
        for seat in &mut self.seats {
            if seat.exhausted {
                continue;
            }
            let Some(tap) = seat.slot.active() else {
                continue;
            };
            let selection = tap.selection();
            let ticket = selection.generation.0;

            // The effect first: a tuner on the mix is as good as one on a
            // track, and its reading labels the track's stamp below.
            let mut reduction = None;
            if let Some(effect) = seat.shared.bound(ticket)
                && let Some(place) = plan.effect_ids.get(effect.0)
                && let Some(&(track, place)) = plan.effect_places.get(place)
                && let Some(unit) = state
                    .chains
                    .get(track)
                    .and_then(|chain| chain.get(place))
                    .and_then(Option::as_ref)
                && let Some(observation) = unit.observation()
                && observation.serial() != seat.last_serial
            {
                seat.last_serial = observation.serial();
                if let EffectObservation::Dynamics(dynamics) = observation {
                    reduction = Some(GainReduction {
                        effect,
                        db: dynamics.reduction_db,
                    });
                }
                let packet = CapturedObservation {
                    selection: ticket,
                    effect,
                    clock: seat.clock,
                    first_frame: base,
                    frames: frames as u32,
                    sample_rate: rate,
                    observation,
                };
                if seat.observations.push(packet).is_err() {
                    count(&seat.shared.dropped_observations);
                }
            }

            let Source::TrackPostFader(id) = selection.source else {
                continue;
            };
            let Some(index) = plan.track_ids.get(id.0) else {
                count(&seat.shared.missing_source_blocks);
                continue;
            };
            let behind = state.behind.get(index).copied().unwrap_or(0);
            if behind > rate as usize {
                count(&seat.shared.latency_refusals);
                continue;
            }
            let stamp = Stamp {
                selection,
                clock: ClockEpoch(seat.clock),
                first_frame: base,
                sample_rate: rate,
                pdc_frames: behind as u32,
                device_latency_frames: None,
                gain_reduction: reduction,
            };
            if tap.publish(stamp, &mixer.track(index)[..frames]).is_err() {
                count(&seat.shared.refused_blocks);
            }
        }
    }

    /// Applies the output gain to `out` as the processor's own finishing
    /// does, and lets every seat on the mix output copy the result before
    /// the samples that were not finite are replaced by silence. A finite
    /// sample comes out as exactly the product the plain finishing gives,
    /// and one that was not finite comes out as zero, so the device hears
    /// the same either way.
    pub fn finish_output(
        &mut self,
        gain: &Ramp,
        base: u64,
        rate: u32,
        latency: u64,
        out: &mut [Frame],
    ) {
        debug_assert!(out.len() <= MAX_BLOCK);
        let frames = out.len().min(MAX_BLOCK);
        self.invalid = [0; MASK_WORDS];
        let mut any_invalid = false;
        let steady = gain.settled(base);
        let mut level = gain.at(base);
        for (offset, frame) in out[..frames].iter_mut().enumerate() {
            if !steady {
                level = gain.at(base + offset as u64);
            }
            for (side, sample) in frame.iter_mut().enumerate() {
                if !sample.is_finite() {
                    let bit = offset * 2 + side;
                    self.invalid[bit / 64] |= 1 << (bit % 64);
                    any_invalid = true;
                }
                *sample *= level;
            }
        }
        for seat in &mut self.seats {
            if seat.exhausted {
                continue;
            }
            let Some(tap) = seat.slot.active() else {
                continue;
            };
            let selection = tap.selection();
            if selection.source != Source::MixOutput {
                continue;
            }
            if latency > u64::from(rate) {
                count(&seat.shared.latency_refusals);
                continue;
            }
            let stamp = Stamp {
                selection,
                clock: ClockEpoch(seat.clock),
                first_frame: base,
                sample_rate: rate,
                pdc_frames: latency as u32,
                device_latency_frames: None,
                gain_reduction: None,
            };
            if tap.publish(stamp, &out[..frames]).is_err() {
                count(&seat.shared.refused_blocks);
            }
        }
        if any_invalid {
            for (offset, frame) in out[..frames].iter_mut().enumerate() {
                for (side, sample) in frame.iter_mut().enumerate() {
                    let bit = offset * 2 + side;
                    if self.invalid[bit / 64] & (1 << (bit % 64)) != 0 {
                        *sample = 0.0;
                    }
                }
            }
        }
    }
}
