//! The live analyzer service: which window looks at which source, and the
//! frames each of them is sent.
//!
//! A *slot* is one consumer of analyzer data in one window: the signal
//! scope, the open effect editor. A slot subscribes with a sender, then
//! selects a source. Slots that select the same source share one of the
//! engine's two capture seats; a third distinct source is refused and
//! nobody else's capture is taken away for it.
//!
//! # Who does what
//!
//! - Commands (`analyzer_subscribe`, `analyzer_select`, `analyzer_ack`,
//!   `analyzer_unsubscribe`) only change the service's own bookkeeping.
//!   A selection is checked against the open document under the document
//!   lock, which is released before the service's lock is taken.
//! - [`Session::analyzer_tick`] does everything else, on the realtime
//!   thread, which never takes the document lock: it asks the engine for
//!   captures, runs their analysis, builds the frames that are due, and
//!   sends them after every lock has been released.
//! - Replacing the project only marks the service
//!   ([`Session::analyzer_project_replaced`]); the next tick lets go of the
//!   old project's captures and tells every slot.
//!
//! # Bounds
//!
//! Four slots, two captures. A slot is sent at most thirty frames a second
//! and only after it acknowledged the frame before, so a window that stops
//! reading holds one frame and nothing piles up behind it. A frame larger
//! than 512 KiB is not sent. Frames that were not sent are counted and the
//! next one that is sent says it does not follow on. A slot that leaves an
//! event unacknowledged for two seconds is stalled and is sent nothing
//! until it acknowledges; a source with no new audio for half a second is
//! stalled, what it showed stops counting as current, and it resumes under
//! a new ticket.
//!
//! Lock order: document, then this service, then the engine's controller.
//! The tick takes the last two only.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windfall_dsp::EffectObservation;
use windfall_engine::analyzers::{ChannelStats, Dbfs, Snapshot, Source};
use windfall_engine::{
    CAPTURE_SEATS, CaptureRefusal, CaptureRequest, CaptureState, CaptureTicket, CaptureView,
    CapturedObservation,
};
use windfall_ipc::{
    ANALYZER_BANDS, ANALYZER_EVENT_BYTES, ANALYZER_EVENTS_PER_SECOND, ANALYZER_OBSERVATIONS,
    ANALYZER_SLOT_BYTES, ANALYZER_SUBSCRIPTIONS, ANALYZER_TEXT_BYTES, AnalyzerAck,
    AnalyzerCapability, AnalyzerClock, AnalyzerCounters, AnalyzerEnvelope, AnalyzerEvent,
    AnalyzerFrame, AnalyzerLevel, AnalyzerObservation, AnalyzerPhase, AnalyzerReason, AnalyzerRow,
    AnalyzerScope, AnalyzerSelect, AnalyzerSource, AnalyzerSpectrum, AnalyzerStatus,
    AnalyzerStereo, AnalyzerTicket, AnalyzerWant,
};
use windfall_project::EffectId;

use super::Session;
use crate::sync::lock;

/// Delivers one event, already encoded as the JSON text the window
/// receives. Returns false once the window is gone.
pub type AnalyzerSender = Box<dyn FnMut(&str) -> bool + Send>;

/// Time between two frames to one slot.
const FRAME_INTERVAL: Duration =
    Duration::from_nanos(1_000_000_000 / ANALYZER_EVENTS_PER_SECOND as u64);
/// How long a slot may leave a frame unacknowledged.
pub const ACK_TIMEOUT: Duration = Duration::from_secs(2);
/// How long a source may go without new audio.
pub const AUDIO_TIMEOUT: Duration = Duration::from_millis(500);
/// Time between two tries to get a capture the engine could not give.
const RETRY_INTERVAL: Duration = Duration::from_millis(100);

// Never reused, so two services of one process are told apart.
static NEXT_SERVICE: AtomicU64 = AtomicU64::new(1);

fn decimal(value: u64) -> String {
    value.to_string()
}

fn engine_source(source: AnalyzerSource) -> Source {
    match source {
        AnalyzerSource::MixOutput => Source::MixOutput,
        AnalyzerSource::TrackPostFader { track } => Source::TrackPostFader(track),
    }
}

fn valid_slot(slot: &str) -> bool {
    !slot.is_empty()
        && slot.len() <= ANALYZER_SLOT_BYTES
        && slot
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

struct Request {
    number: u32,
    source: AnalyzerSource,
    effect: Option<EffectId>,
    want: AnalyzerWant,
    /// The project it was made for.
    project: u64,
    name: Option<String>,
    /// The capture seat it reads, while it has one.
    seat: Option<usize>,
    /// Why it has none, when it was turned away.
    refused: Option<AnalyzerReason>,
}

struct Subscription {
    window: String,
    slot: String,
    /// Renewed when a stalled view comes back, so that what it held before
    /// carries another ticket than what it is sent after.
    id: u64,
    sender: Arc<Mutex<AnalyzerSender>>,
    request: Option<Request>,
    sequence: u64,
    /// The frame it has not acknowledged, and when that was sent.
    unacked: Option<(u64, Instant)>,
    stalled: bool,
    next_due: Option<Instant>,
    /// Frames it was not sent since the selection.
    skipped: u64,
    /// The next frame does not follow on from the last one sent.
    gap: bool,
    /// The window last sent: selection, clock epoch, end frame.
    last: Option<(u64, u64, u64)>,
    /// What the effect measured since the last frame sent.
    observations: Vec<AnalyzerObservation>,
    dropped_observations: u64,
    /// The standing it was last told, so it is told only of changes.
    told: Option<(u32, AnalyzerPhase, Option<AnalyzerReason>, Option<u64>)>,
}

struct Seat {
    source: AnalyzerSource,
    effect: Option<EffectId>,
    project: u64,
    ticket: Option<CaptureTicket>,
    phase: AnalyzerPhase,
    reason: Option<AnalyzerReason>,
    /// The newest window seen: clock epoch and end frame.
    newest: Option<(u64, u64)>,
    /// When a window newer than the one before was last seen.
    fresh_at: Instant,
    /// A frame of this ticket has been seen, so a stall leaves stale
    /// evidence behind and recovery needs a new ticket.
    shown: bool,
    stalled: bool,
    retry_at: Instant,
    /// The project it belongs to is gone. The next tick lets go of it.
    doomed: bool,
}

struct State {
    subscriptions: Vec<Subscription>,
    seats: [Option<Seat>; CAPTURE_SEATS],
    next_subscription: u64,
}

pub(super) struct Service {
    id: u64,
    state: Mutex<State>,
}

impl Service {
    pub(super) fn new() -> Self {
        Self {
            id: NEXT_SERVICE.fetch_add(1, Ordering::Relaxed),
            state: Mutex::new(State {
                subscriptions: Vec::new(),
                seats: [const { None }; CAPTURE_SEATS],
                next_subscription: 1,
            }),
        }
    }
}

/// One event on its way to one slot, encoded and sent with no lock held.
struct Outgoing {
    subscription: u64,
    sender: Arc<Mutex<AnalyzerSender>>,
    event: AnalyzerEvent,
    /// The sequence of a frame, which must be acknowledged.
    frame: Option<u64>,
}

fn ticket_of(service: u64, subscription: &Subscription, seat: &Seat) -> Option<AnalyzerTicket> {
    seat.ticket.map(|ticket| AnalyzerTicket {
        service: decimal(service),
        subscription: decimal(subscription.id),
        stream: decimal(ticket.stream),
        selection: decimal(ticket.selection),
        project: decimal(seat.project),
    })
}

fn idle_status(slot: &str, phase: AnalyzerPhase, reason: Option<AnalyzerReason>) -> AnalyzerStatus {
    AnalyzerStatus {
        slot: slot.to_owned(),
        request: 0,
        phase,
        reason,
        ticket: None,
        source: None,
        effect: None,
        source_name: None,
        sample_rate: None,
    }
}

/// What a slot should be told right now.
fn status_of(service: u64, subscription: &Subscription, seats: &[Option<Seat>]) -> AnalyzerStatus {
    let Some(request) = &subscription.request else {
        return idle_status(&subscription.slot, AnalyzerPhase::Idle, None);
    };
    let seat = request
        .seat
        .and_then(|seat| seats.get(seat))
        .and_then(Option::as_ref);
    let (phase, reason) = match (request.refused, seat) {
        (Some(reason), _) => (AnalyzerPhase::Refused, Some(reason)),
        (None, Some(seat)) => (seat.phase, seat.reason),
        (None, None) => (AnalyzerPhase::Preparing, None),
    };
    AnalyzerStatus {
        slot: subscription.slot.clone(),
        request: request.number,
        phase,
        reason,
        ticket: seat.and_then(|seat| ticket_of(service, subscription, seat)),
        source: Some(request.source),
        effect: request.effect,
        source_name: request.name.clone(),
        sample_rate: seat
            .and_then(|seat| seat.ticket)
            .map(|ticket| ticket.sample_rate),
    }
}

fn level(stats: &ChannelStats) -> AnalyzerLevel {
    let levels = stats.levels;
    let db = |value: Dbfs| match value {
        Dbfs::Silence => None,
        Dbfs::Finite(db) => Some(db as f32),
    };
    AnalyzerLevel {
        valid: levels.is_some(),
        peak: levels.map(|levels| levels.sample_peak as f32),
        rms: levels.map(|levels| levels.rms as f32),
        peak_db: levels.and_then(|levels| db(levels.peak_dbfs)),
        rms_db: levels.and_then(|levels| db(levels.rms_dbfs)),
        silent: levels.is_some_and(|levels| levels.sample_peak == 0.0),
        clipped: stats.clip_samples,
        invalid: stats.invalid_samples,
    }
}

/// Sums bins into at most [`ANALYZER_BANDS`] equal bands, DC and Nyquist
/// included once each, and averages over the valid sides: the contract of
/// `RealtimeFrame::spectrum`. Empty if neither side is valid or a value is
/// not a finite, non-negative power.
pub(super) fn bands(valid: [bool; 2], bins: &[[f64; 2]]) -> Vec<f32> {
    let sides = valid.iter().filter(|&&valid| valid).count();
    let count = bins.len().min(ANALYZER_BANDS);
    if sides == 0 || count == 0 {
        return Vec::new();
    }
    let mut result = Vec::with_capacity(count);
    for band in 0..count {
        let from = band * bins.len() / count;
        let end = (band + 1) * bins.len() / count;
        let mut power = 0.0_f64;
        for bin in &bins[from..end] {
            for (value, counted) in bin.iter().zip(valid) {
                if counted {
                    if !value.is_finite() || *value < 0.0 {
                        return Vec::new();
                    }
                    power += value;
                }
            }
        }
        let power = (power / sides as f64) as f32;
        if !power.is_finite() {
            return Vec::new();
        }
        result.push(power);
    }
    result
}

fn finite(value: f32) -> Option<f32> {
    value.is_finite().then_some(value)
}

fn observation(packet: &CapturedObservation) -> AnalyzerObservation {
    let first_frame = decimal(packet.first_frame);
    match packet.observation {
        EffectObservation::Dynamics(dynamics) => AnalyzerObservation::Dynamics {
            first_frame,
            frames: packet.frames,
            input_peak: finite(dynamics.input_peak).unwrap_or(0.0),
            output_peak: finite(dynamics.output_peak).unwrap_or(0.0),
            reduction_db: finite(dynamics.reduction_db).unwrap_or(0.0),
            gate_db: finite(dynamics.gate_db).unwrap_or(0.0),
        },
        EffectObservation::Tuner { readout, .. } => {
            let pitched = readout.midi_note.is_some() && readout.frequency_hz > 0.0;
            AnalyzerObservation::Tuner {
                first_frame,
                frames: packet.frames,
                frequency_hz: pitched.then_some(readout.frequency_hz).and_then(finite),
                midi_note: if pitched { readout.midi_note } else { None },
                cents: pitched.then_some(readout.cents_error).and_then(finite),
                confidence: finite(readout.confidence).unwrap_or(0.0),
            }
        }
    }
}

/// The parts of a frame that come from the analysis.
struct Measured {
    clock: AnalyzerClock,
    levels: [AnalyzerLevel; 2],
    stereo: AnalyzerStereo,
    spectrum: Option<AnalyzerSpectrum>,
    history: Vec<AnalyzerRow>,
    scope: Option<AnalyzerScope>,
    vector: Option<Vec<[f32; 2]>>,
    envelope: Option<AnalyzerEnvelope>,
}

/// Copies out of a snapshot what one slot wants. `after` is the end frame
/// of the window that slot was sent last in this same clock epoch, so that
/// only windows it has not seen go in the history.
fn measured(snapshot: &Snapshot, want: AnalyzerWant, after: Option<u64>) -> Measured {
    let range = snapshot.range;
    let clock = AnalyzerClock {
        epoch: decimal(range.clock.0),
        first_frame: decimal(range.first_frame),
        end_frame: decimal(range.end_frame),
        sample_rate: range.sample_rate,
        pdc_frames: range.pdc_frames,
        aligned_first_frame: range.pdc_aligned_first_frame().to_string(),
        time_label: range.pdc_time_label(),
        device_latency_frames: range.device_latency_frames,
    };
    let levels = [level(&snapshot.channels[0]), level(&snapshot.channels[1])];
    let stereo = AnalyzerStereo {
        correlation: snapshot
            .stereo
            .correlation
            .map(|value| value as f32)
            .and_then(finite),
        mid_rms: snapshot
            .stereo
            .mid_rms
            .map(|value| value as f32)
            .and_then(finite),
        side_rms: snapshot
            .stereo
            .side_rms
            .map(|value| value as f32)
            .and_then(finite),
    };
    let valid = snapshot.spectrum_valid;
    let mut spectrum = None;
    let mut history = Vec::new();
    if want.spectrum {
        let side = |side: usize| -> Vec<f32> {
            if !valid[side] {
                return Vec::new();
            }
            snapshot
                .spectrum
                .iter()
                .map(|bin| bin.mean_square[side] as f32)
                .collect()
        };
        let powers: Vec<[f64; 2]> = snapshot
            .spectrum
            .iter()
            .map(|bin| bin.mean_square)
            .collect();
        spectrum = Some(AnalyzerSpectrum {
            bin_hz: range.sample_rate as f32 / snapshot.config.fft_size as f32,
            enbw_bins: snapshot.calibration.enbw_bins as f32,
            left: side(0),
            right: side(1),
            bands: bands(valid, &powers),
        });
        for slice in snapshot.spectrogram_slices() {
            if slice.range.end_frame >= range.end_frame
                || slice.range.clock != range.clock
                || after.is_some_and(|after| slice.range.end_frame <= after)
            {
                continue;
            }
            let row = bands(slice.valid, &slice.mean_square);
            if !row.is_empty() {
                history.push(AnalyzerRow {
                    first_frame: decimal(slice.range.first_frame),
                    end_frame: decimal(slice.range.end_frame),
                    bands: row,
                });
            }
        }
    }
    let scope = want.scope.then(|| {
        let frames = snapshot.config.fft_size.min(snapshot.history.len());
        let window = &snapshot.history[snapshot.history.len() - frames..];
        AnalyzerScope {
            first_frame: decimal(range.end_frame - frames as u64),
            left: window.iter().map(|frame| finite(frame[0])).collect(),
            right: window.iter().map(|frame| finite(frame[1])).collect(),
        }
    });
    let vector = want.vector.then(|| {
        snapshot
            .vectorscope
            .iter()
            .map(|point| [point[0] as f32, point[1] as f32])
            .collect()
    });
    let envelope = want.envelope.then(|| AnalyzerEnvelope {
        first_frame: decimal(snapshot.history_first_frame),
        end_frame: decimal(range.end_frame),
        bins: snapshot
            .envelope
            .iter()
            .map(|bin| [bin.min[0], bin.max[0], bin.min[1], bin.max[1]])
            .collect(),
    });
    Measured {
        clock,
        levels,
        stereo,
        spectrum,
        history,
        scope,
        vector,
        envelope,
    }
}

/// What one look at a seat's capture gave, copied out of the engine's view.
struct Looked {
    state: CaptureState,
    effect_present: bool,
    /// Clock epoch and end frame of the newest window.
    window: Option<(u64, u64)>,
    observations: Vec<AnalyzerObservation>,
    dropped_observations: u64,
    latency_refused: bool,
    /// A frame for each slot that is due, by its place in the list.
    frames: Vec<(usize, AnalyzerFrame)>,
}

impl Session {
    /// What the analyzer can do, and whether a stream is there for it.
    pub fn analyzer_capability(&self) -> AnalyzerCapability {
        let capability = self.controller().capture_capability();
        let bins = (capability.config.fft_size / 2 + 1) as u32;
        AnalyzerCapability {
            native: true,
            reason: None,
            captures: CAPTURE_SEATS as u32,
            subscriptions: ANALYZER_SUBSCRIPTIONS as u32,
            fft_size: capability.config.fft_size as u32,
            hop_size: capability.config.hop_size as u32,
            bins,
            bands: bins.min(ANALYZER_BANDS as u32),
            window: "periodicHann".to_owned(),
            scope_frames: capability.config.fft_size as u32,
            vector_points: windfall_engine::analyzers::VECTOR_POINTS as u32,
            envelope_bins: windfall_engine::analyzers::ENVELOPE_BINS as u32,
            history_rows: windfall_engine::analyzers::SPECTROGRAM_SLICES as u32 - 1,
            observations: ANALYZER_OBSERVATIONS as u32,
            max_event_bytes: ANALYZER_EVENT_BYTES as u32,
            max_events_per_second: ANALYZER_EVENTS_PER_SECOND,
            sample_rate: capability.stream.map(|stream| stream.1),
            min_sample_rate: 8_000,
            max_sample_rate: 384_000,
        }
    }

    /// Starts sending a window's slot its analyzer events. A slot that
    /// subscribes again, as after a reload, replaces its old sender and
    /// starts with nothing selected.
    pub fn analyzer_subscribe(
        &self,
        window: &str,
        slot: &str,
        sender: AnalyzerSender,
    ) -> AnalyzerStatus {
        if !valid_slot(slot) {
            return idle_status(
                slot.get(..ANALYZER_SLOT_BYTES).unwrap_or(""),
                AnalyzerPhase::Refused,
                Some(AnalyzerReason::InvalidSlot),
            );
        }
        let service = &self.inner.analyzers;
        // The old sender is dropped after the lock.
        let displaced;
        {
            let mut state = lock(&service.state);
            let at = state
                .subscriptions
                .iter()
                .position(|sub| sub.window == window && sub.slot == slot);
            if at.is_none() && state.subscriptions.len() >= ANALYZER_SUBSCRIPTIONS {
                return idle_status(
                    slot,
                    AnalyzerPhase::Refused,
                    Some(AnalyzerReason::SubscriptionCapacity),
                );
            }
            displaced = at.map(|at| state.subscriptions.remove(at));
            let id = state.next_subscription;
            state.next_subscription += 1;
            state.subscriptions.push(Subscription {
                window: window.to_owned(),
                slot: slot.to_owned(),
                id,
                sender: Arc::new(Mutex::new(sender)),
                request: None,
                sequence: 0,
                unacked: None,
                stalled: false,
                next_due: None,
                skipped: 0,
                gap: true,
                last: None,
                observations: Vec::with_capacity(ANALYZER_OBSERVATIONS),
                dropped_observations: 0,
                told: Some((0, AnalyzerPhase::Idle, None, None)),
            });
        }
        drop(displaced);
        idle_status(slot, AnalyzerPhase::Idle, None)
    }

    /// Stops a slot. Its capture is let go at the next tick if no other
    /// slot reads it.
    pub fn analyzer_unsubscribe(&self, window: &str, slot: &str) {
        let removed: Vec<Subscription> = {
            let mut state = lock(&self.inner.analyzers.state);
            let (gone, kept) = std::mem::take(&mut state.subscriptions)
                .into_iter()
                .partition(|sub| sub.window == window && sub.slot == slot);
            state.subscriptions = kept;
            gone
        };
        drop(removed);
    }

    /// Stops every slot of a window that has closed.
    pub fn analyzer_window_closed(&self, window: &str) {
        let removed: Vec<Subscription> = {
            let mut state = lock(&self.inner.analyzers.state);
            let (gone, kept) = std::mem::take(&mut state.subscriptions)
                .into_iter()
                .partition(|sub| sub.window == window);
            state.subscriptions = kept;
            gone
        };
        drop(removed);
    }

    /// Points a slot at a source. The answer says where the selection
    /// stands; frames follow as events once real audio has been analyzed.
    /// A refusal leaves the slot with what it had selected before.
    pub fn analyzer_select(&self, window: &str, select: AnalyzerSelect) -> AnalyzerStatus {
        self.analyzer_select_at(window, select, Instant::now())
    }

    pub(super) fn analyzer_select_at(
        &self,
        window: &str,
        select: AnalyzerSelect,
        now: Instant,
    ) -> AnalyzerStatus {
        let refuse = |reason| AnalyzerStatus {
            slot: select
                .slot
                .get(..ANALYZER_SLOT_BYTES.min(select.slot.len()))
                .unwrap_or("")
                .to_owned(),
            request: select.request,
            phase: AnalyzerPhase::Refused,
            reason: Some(reason),
            ticket: None,
            source: Some(select.source),
            effect: select.effect,
            source_name: None,
            sample_rate: None,
        };
        if !valid_slot(&select.slot) {
            return refuse(AnalyzerReason::InvalidSlot);
        }
        // The document says whether the source is there, and its name. Its
        // lock is released before the service's is taken.
        let name;
        {
            let state = self.state();
            if state.generation != select.generation {
                return refuse(AnalyzerReason::StaleProject);
            }
            let mixer = &state.document.project().mixer;
            match select.source {
                AnalyzerSource::MixOutput => {
                    name = None;
                    if select.effect.is_some() {
                        return refuse(AnalyzerReason::EffectNotOnSource);
                    }
                }
                AnalyzerSource::TrackPostFader { track } => {
                    let Some(found) = mixer.track(track) else {
                        return refuse(AnalyzerReason::SourceMissing);
                    };
                    if let Some(effect) = select.effect
                        && !found.effects.iter().any(|slot| slot.id == effect)
                    {
                        let elsewhere = mixer
                            .tracks
                            .iter()
                            .any(|track| track.effects.iter().any(|slot| slot.id == effect));
                        return refuse(if elsewhere {
                            AnalyzerReason::EffectNotOnSource
                        } else {
                            AnalyzerReason::EffectMissing
                        });
                    }
                    name = (found.name.len() <= ANALYZER_TEXT_BYTES).then(|| found.name.clone());
                }
            }
        }

        let service = &self.inner.analyzers;
        let mut guard = lock(&service.state);
        let State {
            subscriptions,
            seats,
            ..
        } = &mut *guard;
        let Some(at) = subscriptions
            .iter()
            .position(|sub| sub.window == window && sub.slot == select.slot)
        else {
            return refuse(AnalyzerReason::NotSubscribed);
        };
        // A seat already on this source and project is shared. Otherwise a
        // seat nobody else reads is taken; the slot's own one counts as free.
        let same = |seat: &Seat| {
            !seat.doomed
                && seat.source == select.source
                && seat.effect == select.effect
                && seat.project == select.generation
        };
        let used_by_others = |index: usize| {
            subscriptions.iter().enumerate().any(|(other, sub)| {
                other != at
                    && sub
                        .request
                        .as_ref()
                        .is_some_and(|request| request.seat == Some(index))
            })
        };
        let shared = (0..CAPTURE_SEATS).find(|&index| seats[index].as_ref().is_some_and(same));
        let free = (0..CAPTURE_SEATS).find(|&index| !used_by_others(index));
        let Some(index) = shared.or(free) else {
            return refuse(AnalyzerReason::SourceCapacity);
        };
        if shared.is_none() {
            // The engine is asked at the next tick, off this thread. An old
            // capture in the seat is replaced by that selection.
            seats[index] = Some(Seat {
                source: select.source,
                effect: select.effect,
                project: select.generation,
                ticket: None,
                phase: AnalyzerPhase::Preparing,
                reason: None,
                newest: None,
                fresh_at: now,
                shown: false,
                stalled: false,
                retry_at: now,
                doomed: false,
            });
        }
        let subscription = &mut subscriptions[at];
        subscription.request = Some(Request {
            number: select.request,
            source: select.source,
            effect: select.effect,
            want: select.want,
            project: select.generation,
            name,
            seat: Some(index),
            refused: None,
        });
        subscription.gap = true;
        subscription.last = None;
        subscription.skipped = 0;
        subscription.observations.clear();
        subscription.dropped_observations = 0;
        let status = status_of(service.id, subscription, seats);
        subscription.told = Some((
            status.request,
            status.phase,
            status.reason,
            seats[index]
                .as_ref()
                .and_then(|seat| seat.ticket)
                .map(|ticket| ticket.selection),
        ));
        status
    }

    /// Records that a slot received a frame, which lets the next be sent.
    /// An acknowledgement of anything but the frame it is waiting on does
    /// nothing. A stalled slot that acknowledges is told it was stalled and
    /// then resumes under a new ticket.
    pub fn analyzer_ack(&self, window: &str, ack: AnalyzerAck) {
        let Ok(sequence) = ack.sequence.parse::<u64>() else {
            return;
        };
        let mut state = lock(&self.inner.analyzers.state);
        let State {
            subscriptions,
            next_subscription,
            ..
        } = &mut *state;
        let Some(subscription) = subscriptions
            .iter_mut()
            .find(|sub| sub.window == window && sub.slot == ack.slot)
        else {
            return;
        };
        if subscription.unacked.map(|unacked| unacked.0) != Some(sequence) {
            return;
        }
        subscription.unacked = None;
        if subscription.stalled {
            subscription.stalled = false;
            subscription.gap = true;
            subscription.last = None;
            subscription.id = *next_subscription;
            *next_subscription += 1;
            // Told at the next tick, ahead of any frame.
            subscription.told = None;
        }
    }

    /// The document was replaced by project `generation`. Called with the
    /// document lock held, so it only marks: the next tick lets go of the
    /// old project's captures, outside that lock, and tells every slot.
    pub(super) fn analyzer_project_replaced(&self, generation: u64) {
        // The waveform meters' interest is by track id, and the new
        // project's ids start over: without this the old project's choice
        // and its last third of a second of history would show under the
        // new project's tracks. Only atomics are written.
        self.controller().clear_waveform_history();
        let mut state = lock(&self.inner.analyzers.state);
        for seat in state.seats.iter_mut().flatten() {
            if seat.project != generation {
                seat.doomed = true;
            }
        }
        for subscription in &mut state.subscriptions {
            if let Some(request) = &mut subscription.request
                && request.project != generation
            {
                request.seat = None;
                request.refused = Some(AnalyzerReason::ProjectReplaced);
                subscription.gap = true;
                subscription.last = None;
                subscription.observations.clear();
            }
        }
    }

    /// One step of the service, at `now`: gets and lets go of captures,
    /// runs their analysis, and sends what is due. The realtime thread
    /// calls it sixty times a second; it never takes the document lock.
    /// Returns how many events it sent.
    pub fn analyzer_tick(&self, now: Instant) -> usize {
        let service = &self.inner.analyzers;
        let controller = self.controller();
        let mut outgoing: Vec<Outgoing> = Vec::new();
        {
            let mut guard = lock(&service.state);
            let State {
                subscriptions,
                seats,
                ..
            } = &mut *guard;

            // A view that has sat on a frame too long is sent nothing more.
            for subscription in subscriptions.iter_mut() {
                if let Some((_, sent)) = subscription.unacked
                    && !subscription.stalled
                    && now.saturating_duration_since(sent) > ACK_TIMEOUT
                {
                    subscription.stalled = true;
                }
            }

            for (index, slot) in seats.iter_mut().enumerate() {
                let used = subscriptions.iter().any(|sub| {
                    sub.request
                        .as_ref()
                        .is_some_and(|request| request.seat == Some(index))
                });
                let Some(seat) = slot else {
                    continue;
                };
                if seat.doomed || !used {
                    // Let go of the engine's capture. If the audio thread
                    // has not yet taken the last hand-over, try again.
                    if seat.ticket.is_none() || controller.capture_clear(index) {
                        *slot = None;
                    }
                    continue;
                }
                if seat.ticket.is_none() && now >= seat.retry_at {
                    let request = CaptureRequest {
                        project: seat.project,
                        source: engine_source(seat.source),
                        effect: seat.effect,
                    };
                    match controller.capture_select(index, request) {
                        Ok(ticket) => {
                            seat.ticket = Some(ticket);
                            seat.phase = AnalyzerPhase::AwaitingAudio;
                            seat.reason = None;
                            seat.newest = None;
                            seat.fresh_at = now;
                            seat.shown = false;
                            seat.stalled = false;
                        }
                        Err(refusal) => {
                            let (phase, reason) = match refusal {
                                CaptureRefusal::NoStream => (AnalyzerPhase::WaitingForStream, None),
                                CaptureRefusal::Busy => (AnalyzerPhase::Preparing, None),
                                CaptureRefusal::SourceMissing => {
                                    (AnalyzerPhase::Refused, Some(AnalyzerReason::SourceMissing))
                                }
                                CaptureRefusal::EffectMissing => {
                                    (AnalyzerPhase::Refused, Some(AnalyzerReason::EffectMissing))
                                }
                                CaptureRefusal::EffectNotOnSource => (
                                    AnalyzerPhase::Refused,
                                    Some(AnalyzerReason::EffectNotOnSource),
                                ),
                                CaptureRefusal::RateUnsupported => (
                                    AnalyzerPhase::Refused,
                                    Some(AnalyzerReason::RateUnsupported),
                                ),
                                CaptureRefusal::Preparation(_) | CaptureRefusal::Seat => {
                                    (AnalyzerPhase::Refused, Some(AnalyzerReason::Preparation))
                                }
                                CaptureRefusal::Exhausted => {
                                    (AnalyzerPhase::Refused, Some(AnalyzerReason::Exhausted))
                                }
                            };
                            seat.phase = phase;
                            seat.reason = reason;
                            seat.retry_at = now + RETRY_INTERVAL;
                        }
                    }
                }
                let Some(ticket) = seat.ticket else {
                    continue;
                };

                // Which slots on this seat could take a frame now.
                let due: Vec<usize> = subscriptions
                    .iter()
                    .enumerate()
                    .filter(|(_, sub)| {
                        sub.request
                            .as_ref()
                            .is_some_and(|request| request.seat == Some(index))
                            && !sub.stalled
                            && sub.unacked.is_none()
                            && sub.next_due.is_none_or(|due| now >= due)
                    })
                    .map(|(at, _)| at)
                    .collect();
                let (tick, playing) = controller.heard_position();
                let service_id = service.id;
                let seat_view: &Seat = seat;
                let readers: &[Subscription] = subscriptions;
                let looked = controller.capture_poll(index, ticket, |view: CaptureView<'_>| {
                    let window = view
                        .snapshot
                        .map(|snapshot| (snapshot.range.clock.0, snapshot.range.end_frame));
                    let mut frames = Vec::new();
                    if view.state == CaptureState::Ready
                        && view.effect_present
                        && !seat_view.stalled
                        && let Some(snapshot) = view.snapshot
                        && let Some(window) = window
                    {
                        for &at in &due {
                            let subscription = &readers[at];
                            let Some(request) = &subscription.request else {
                                continue;
                            };
                            // The same window is not sent to a slot twice.
                            if subscription.last == Some((ticket.selection, window.0, window.1)) {
                                continue;
                            }
                            let Some(ticket_text) = ticket_of(service_id, subscription, seat_view)
                            else {
                                continue;
                            };
                            let after = subscription
                                .last
                                .filter(|last| last.0 == ticket.selection && last.1 == window.0)
                                .map(|last| last.2);
                            let parts = measured(snapshot, request.want, after);
                            let counters = AnalyzerCounters {
                                dropped_frames: decimal(view.counters.dropped_frames),
                                invalid_samples: decimal(view.counters.invalid_samples),
                                resets: decimal(view.counters.resets),
                                analyzed_windows: decimal(view.counters.analyzed_windows),
                                dropped_windows: decimal(view.counters.dropped_snapshots),
                                dropped_observations: String::new(),
                                missing_source_blocks: decimal(view.seat.missing_source_blocks),
                                skipped_events: String::new(),
                            };
                            frames.push((
                                at,
                                AnalyzerFrame {
                                    slot: subscription.slot.clone(),
                                    request: request.number,
                                    ticket: ticket_text,
                                    sequence: String::new(),
                                    clock: parts.clock,
                                    gap: false,
                                    levels: parts.levels,
                                    stereo: parts.stereo,
                                    spectrum: parts.spectrum,
                                    history: parts.history,
                                    scope: parts.scope,
                                    vector: parts.vector,
                                    envelope: parts.envelope,
                                    reduction_db: snapshot
                                        .gain_reduction
                                        .filter(|_| request.effect.is_some())
                                        .map(|reduction| reduction.db)
                                        .and_then(finite),
                                    observations: Vec::new(),
                                    counters,
                                    playing,
                                    tick,
                                },
                            ));
                        }
                    }
                    Looked {
                        state: view.state,
                        effect_present: view.effect_present,
                        window,
                        observations: view.observations.iter().map(observation).collect(),
                        dropped_observations: view.seat.dropped_observations,
                        latency_refused: view.seat.latency_refusals > 0,
                        frames,
                    }
                });

                // The seat's standing.
                let mut renew = false;
                match looked.state {
                    CaptureState::NoStream | CaptureState::Stale => {
                        // The stream went, or came back as another one.
                        seat.ticket = None;
                        seat.phase = AnalyzerPhase::WaitingForStream;
                        seat.reason = None;
                        seat.newest = None;
                        seat.retry_at = now;
                    }
                    CaptureState::SourceMissing => {
                        seat.phase = AnalyzerPhase::Refused;
                        seat.reason = Some(AnalyzerReason::SourceMissing);
                    }
                    CaptureState::AwaitingAudio | CaptureState::Ready => {
                        let fresh = looked.window.is_some() && looked.window != seat.newest;
                        if fresh {
                            seat.newest = looked.window;
                            seat.fresh_at = now;
                        }
                        if !looked.effect_present {
                            seat.phase = AnalyzerPhase::Refused;
                            seat.reason = Some(AnalyzerReason::EffectMissing);
                        } else if seat.stalled {
                            if fresh && seat.shown {
                                // Audio is back. What was shown before the
                                // stall is not continued: a new ticket.
                                renew = true;
                            } else if fresh {
                                seat.stalled = false;
                                seat.phase = AnalyzerPhase::Live;
                                seat.reason = None;
                            }
                        } else if now.saturating_duration_since(seat.fresh_at) > AUDIO_TIMEOUT {
                            seat.stalled = true;
                            seat.phase = AnalyzerPhase::Stalled;
                            seat.reason = Some(if looked.latency_refused {
                                AnalyzerReason::LatencyUnsupported
                            } else {
                                AnalyzerReason::AudioStalled
                            });
                        } else if looked.state == CaptureState::Ready {
                            seat.phase = AnalyzerPhase::Live;
                            seat.reason = None;
                        } else {
                            seat.phase = AnalyzerPhase::AwaitingAudio;
                            seat.reason = None;
                        }
                    }
                }
                if renew {
                    seat.ticket = None;
                    seat.newest = None;
                    seat.retry_at = now;
                    seat.phase = AnalyzerPhase::Preparing;
                    seat.reason = None;
                }

                // What the effect measured goes to every slot on the seat,
                // whether or not it gets a frame this time.
                let readable = seat.ticket.is_some() && seat.phase == AnalyzerPhase::Live;
                for subscription in subscriptions.iter_mut() {
                    let on_seat = subscription
                        .request
                        .as_ref()
                        .is_some_and(|request| request.seat == Some(index));
                    if !on_seat {
                        continue;
                    }
                    if !readable {
                        subscription.observations.clear();
                        subscription.gap = true;
                        continue;
                    }
                    for observation in &looked.observations {
                        if subscription.observations.len() >= ANALYZER_OBSERVATIONS {
                            subscription.observations.remove(0);
                            subscription.dropped_observations += 1;
                            subscription.gap = true;
                        }
                        subscription.observations.push(observation.clone());
                    }
                }
                if !readable {
                    continue;
                }
                seat.shown |= !looked.frames.is_empty();
                for (at, mut frame) in looked.frames {
                    let subscription = &mut subscriptions[at];
                    subscription.sequence += 1;
                    frame.sequence = decimal(subscription.sequence);
                    frame.gap = subscription.gap;
                    frame.observations = std::mem::take(&mut subscription.observations);
                    frame.counters.dropped_observations = decimal(
                        looked
                            .dropped_observations
                            .saturating_add(subscription.dropped_observations),
                    );
                    frame.counters.skipped_events = decimal(subscription.skipped);
                    subscription.gap = false;
                    subscription.last = looked
                        .window
                        .map(|window| (ticket.selection, window.0, window.1));
                    subscription.unacked = Some((subscription.sequence, now));
                    // A running deadline keeps thirty a second without a
                    // burst after a late tick.
                    subscription.next_due = Some(match subscription.next_due {
                        Some(due) if now.saturating_duration_since(due) < FRAME_INTERVAL => {
                            due + FRAME_INTERVAL
                        }
                        _ => now + FRAME_INTERVAL,
                    });
                    outgoing.push(Outgoing {
                        subscription: subscription.id,
                        sender: subscription.sender.clone(),
                        event: AnalyzerEvent::Frame(Box::new(frame)),
                        frame: Some(subscription.sequence),
                    });
                }
            }

            // Slots that had to wait while unacknowledged missed a frame.
            for subscription in subscriptions.iter_mut() {
                if subscription.unacked.is_some()
                    && !outgoing
                        .iter()
                        .any(|out| out.subscription == subscription.id)
                    && subscription.next_due.is_some_and(|due| now >= due)
                    && subscription
                        .request
                        .as_ref()
                        .is_some_and(|r| r.seat.is_some())
                {
                    subscription.skipped += 1;
                    subscription.gap = true;
                    subscription.next_due = Some(now + FRAME_INTERVAL);
                }
            }

            // Tell each slot its standing when that changed. A stalled view
            // is told nothing until it acknowledges. A status goes out
            // ahead of that tick's frame.
            let mut statuses = Vec::new();
            for subscription in subscriptions.iter_mut() {
                if subscription.stalled {
                    continue;
                }
                let status = status_of(service.id, subscription, seats);
                let selection = status
                    .ticket
                    .as_ref()
                    .and_then(|ticket| ticket.selection.parse::<u64>().ok());
                let telling = (status.request, status.phase, status.reason, selection);
                if subscription.told == Some(telling) {
                    continue;
                }
                // A view back from a stall hears that it was stalled first.
                let event = if subscription.told.is_none() && subscription.request.is_some() {
                    AnalyzerStatus {
                        phase: AnalyzerPhase::Stalled,
                        reason: Some(AnalyzerReason::ViewStalled),
                        ..status.clone()
                    }
                } else {
                    status
                };
                subscription.told = if event.reason == Some(AnalyzerReason::ViewStalled) {
                    // The real standing follows at the next tick.
                    Some((event.request, event.phase, event.reason, selection))
                } else {
                    Some(telling)
                };
                statuses.push(Outgoing {
                    subscription: subscription.id,
                    sender: subscription.sender.clone(),
                    event: AnalyzerEvent::Status(event),
                    frame: None,
                });
            }
            statuses.append(&mut outgoing);
            outgoing = statuses;
        }

        // Encoding and sending, with every lock released.
        let mut sent = 0;
        let mut gone: Vec<u64> = Vec::new();
        let mut oversized: Vec<(u64, u64)> = Vec::new();
        for out in outgoing {
            let Ok(text) = serde_json::to_string(&out.event) else {
                continue;
            };
            if text.len() > ANALYZER_EVENT_BYTES {
                if let Some(sequence) = out.frame {
                    oversized.push((out.subscription, sequence));
                }
                continue;
            }
            let delivered = {
                let mut send = lock(&out.sender);
                (*send)(&text)
            };
            if delivered {
                sent += 1;
            } else {
                gone.push(out.subscription);
            }
        }
        if !gone.is_empty() || !oversized.is_empty() {
            let removed: Vec<Subscription>;
            {
                let mut state = lock(&service.state);
                for subscription in &mut state.subscriptions {
                    if oversized.iter().any(|&(id, sequence)| {
                        id == subscription.id
                            && subscription.unacked.map(|unacked| unacked.0) == Some(sequence)
                    }) {
                        // It was never sent, so nothing is waited for.
                        subscription.unacked = None;
                        subscription.skipped += 1;
                        subscription.gap = true;
                    }
                }
                let (lost, kept) = std::mem::take(&mut state.subscriptions)
                    .into_iter()
                    .partition(|sub| gone.contains(&sub.id));
                state.subscriptions = kept;
                removed = lost;
            }
            drop(removed);
        }
        sent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_keep_all_power_and_never_turn_bad_bins_into_numbers() {
        // 513 bins of one unit each, both sides valid: 64 bands that sum to
        // 513, since the two sides are averaged.
        let bins = vec![[1.0, 1.0]; 513];
        let summed = bands([true, true], &bins);
        assert_eq!(summed.len(), 64);
        assert!((summed.iter().sum::<f32>() - 513.0).abs() < 1.0e-3);
        // One valid side counts alone.
        let left = vec![[2.0, f64::NAN]; 513];
        let summed = bands([true, false], &left);
        assert!((summed.iter().sum::<f32>() - 1026.0).abs() < 1.0e-2);
        // No valid side, a negative power, or a NaN on a valid side: empty.
        assert!(bands([false, false], &bins).is_empty());
        assert!(bands([true, true], &left).is_empty());
        assert!(bands([true, true], &[[1.0, -1.0]]).is_empty());
        // Fewer bins than bands keeps one band per bin.
        assert_eq!(bands([true, true], &[[0.5, 0.5]; 3]), vec![0.5; 3]);
    }

    #[test]
    fn slot_names_are_short_plain_ascii() {
        assert!(valid_slot("scope"));
        assert!(valid_slot("effect-1_a"));
        assert!(!valid_slot(""));
        assert!(!valid_slot("has space"));
        assert!(!valid_slot("é"));
        assert!(!valid_slot(&"x".repeat(ANALYZER_SLOT_BYTES + 1)));
    }
}
