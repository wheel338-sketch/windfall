//! Sampler voices: one playing copy of a sample each, in a pool allocated
//! once.
//!
//! The pool is the one hard limit on how much can sound at once. Up to
//! [`MAX_VOICES`] voices play at full level. A note that would be one more
//! steals a voice: the quietest one that is already releasing, or failing
//! that the one that started first, with the lowest slot winning a tie. The
//! stolen voice is not cut off but faded out over a few milliseconds in one
//! of [`FADING_SLOTS`] extra slots. Only when those are all taken as well,
//! which takes hundreds of notes within those few milliseconds, does the
//! fading voice nearest silence end at once. Every one of these choices
//! depends only on the voices themselves, so the same notes always steal
//! the same voices.

use std::ops::Range;

use rtrb::Producer;
use windfall_core::{AudioBuffer, db_to_gain, pan_gains};
use windfall_dsp::{
    NoteArticulation, NoteExpression,
    blocks::svf::{Svf, SvfCoeffs},
};
use windfall_project::{ChannelId, Envelope, SamplerLoopMode, TrackId};

use crate::channel_voice::{
    ModulationBlock, PreparedChannelEnvelopes, VoiceFilter, VoiceFilterMode,
};
use crate::message::{Garbage, retire};
use crate::mixer::{Frame, Mixer};
use crate::note_glide::NoteGlide;
use crate::plan::Plan;
use crate::sequencer::Clock;
use crate::state::{Heard, PlanState, Strip};
use windfall_dsp::NoteInstanceId;

/// Most voices that sound at full level at once. One more steals a voice.
pub(crate) const MAX_VOICES: usize = 256;

/// Extra slots for voices that are fading out after being stolen or cut, so
/// that taking a voice never has to end one abruptly.
const FADING_SLOTS: usize = 64;

/// Length of the fade that ends a voice early.
const FADE_SECONDS: f64 = 0.004;

/// Shortest envelope release. A release of zero would be a click.
const MIN_RELEASE_SECONDS: f32 = 0.001;

/// Level of a browser preview in decibels. A preview plays on top of
/// whatever the project is playing, and a sample at full level added to a
/// full mix would push the master into clipping.
pub const PREVIEW_GAIN_DB: f32 = -6.0;

/// How far short of its target a plain exponential would still be when a
/// decay or release runs out of time: a thousandth of the way, which is
/// 60 dB. The curve used is that exponential lowered by this much and
/// scaled back to full height. It falls at a steady rate in decibels, as
/// an exponential does, and still lands exactly on its target on its last
/// frame instead of creeping up on it forever.
const CURVE_FLOOR: f64 = 0.001;

/// Who started a voice. It decides what stops it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    /// A note from a pattern.
    Sequenced,
    /// A note played by hand from the UI.
    Live,
    /// Native MIDI input, invalidated independently of UI-held notes.
    Hardware,
    /// The browser's sample preview.
    Preview,
}

/// Where a voice's signal goes and whose gain and pan shape it.
#[derive(Debug, Clone, Copy)]
enum Route {
    /// Through the channel at this index of the current plan.
    Channel(usize),
    /// Straight into the master track.
    Master,
    /// Into the mixer track with this id, at the gain each side of the
    /// voice had on its channel when a change of plan took the channel
    /// from under it. The voice was fading out for good, lost its channel,
    /// or its channel moved to another track, and something on the way
    /// from the track to the output has a memory: an effect or a
    /// compensation delay. Leaving that path would be heard as a jump, so
    /// the voice finishes inside it. `track` is the track's index in the
    /// current plan.
    Track {
        track: usize,
        id: TrackId,
        left: f32,
        right: f32,
    },
    /// Past the mixer, straight to the output. The voice was fading out
    /// when the plan changed, and nothing but faders lay between it and
    /// the output, or its track is gone. It finishes at the gain each side
    /// of it was heard at in that moment, so nothing the new plan does to
    /// the mixer can make it click, and no fader has to glide on its
    /// account.
    Apart { left: f32, right: f32 },
}

/// The gain a voice is given on each side while it renders.
#[derive(Debug, Clone, Copy)]
enum Mix {
    /// A channel's gain and pan, which may be gliding.
    Strip(Strip),
    /// The same gain on every frame: left, then right.
    Sides(f32, f32),
}

/// A stretch of the block being processed: the frame offsets `frames` of
/// the block that starts on frame `base`.
pub(crate) struct Stretch {
    pub base: u64,
    pub frames: Range<usize>,
}

/// A note to start.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Note {
    pub source: Option<crate::note_curves::CurveSource>,
    /// Index into [`Plan::channels`].
    pub channel: usize,
    pub key: u8,
    pub velocity: f32,
    pub pan: f32,
    pub expression: NoteExpression,
    /// Tick on the sequencer's clock on which the note ends, for samplers
    /// with an envelope or loop. Infinity holds the note until it is released by
    /// hand.
    pub end: f64,
    pub origin: Origin,
}

/// The part of a sample a voice or an audio clip plays, and in which
/// direction.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Region {
    pub first: usize,
    pub frames: usize,
    pub reverse: bool,
}

impl Region {
    /// The frame `index` steps into the region in playing order. Outside
    /// the region is silence.
    #[inline]
    fn frame(&self, sample: &AudioBuffer, index: isize) -> (f32, f32) {
        if index < 0 || index as usize >= self.frames {
            return (0.0, 0.0);
        }
        let index = index as usize;
        sample.stereo_frame(if self.reverse {
            self.first + self.frames - 1 - index
        } else {
            self.first + index
        })
    }

    /// The sample at a fractional position, by four-point Hermite
    /// interpolation. A whole-numbered position returns the stored frame
    /// untouched.
    #[inline]
    pub fn read(&self, sample: &AudioBuffer, position: f64) -> (f32, f32) {
        interpolate(position, |index| self.frame(sample, index))
    }
}

/// Prepared loop bounds in the trimmed region's playing order. The voice's
/// position advances through a virtual periodic sequence; reflected taps at
/// ping-pong turns and wrapped taps at forward seams follow that sequence.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LoopRegion {
    pub first: usize,
    pub frames: usize,
    pub mode: SamplerLoopMode,
    pub crossfade: f32,
}

impl LoopRegion {
    fn period(self) -> usize {
        match self.mode {
            SamplerLoopMode::PingPong => 2 * self.frames.saturating_sub(1).max(1),
            SamplerLoopMode::Off | SamplerLoopMode::Forward => self.frames,
        }
    }

    fn advance(self, position: &mut f64, cycled: &mut bool, step: f64) {
        *position += step;
        let first = self.first as f64;
        let period = self.period() as f64;
        if *position >= first + period {
            *position = first + (*position - first).rem_euclid(period);
            *cycled = true;
        }
    }

    fn read(self, region: Region, sample: &AudioBuffer, position: f64, cycled: bool) -> (f32, f32) {
        // A one-frame loop holds that frame at every rate, without interpolating
        // into silence or borrowing a frame outside the loop.
        if self.frames == 1 && position >= self.first as f64 {
            return region.frame(sample, self.first as isize);
        }
        interpolate(position, |index| {
            let first = self.first as isize;
            if index < first && !cycled {
                return region.frame(sample, index.max(0));
            }
            loop_crossfade_mix(
                self.frames,
                self.mode,
                self.crossfade,
                index - first,
                |mapped| region.frame(sample, first + mapped as isize),
            )
        })
    }
}

/// Read one frame of a virtual loop, blending its tail into its start.
/// `frame` reads in loop-relative playing order. Ping-pong reflects both taps.
/// The blend spans `round(amount * frames)`, capped at half the source loop.
/// Zero (including non-finite amounts) returns the dry frame unchanged.
#[inline]
pub fn loop_crossfade_mix(
    frames: usize,
    mode: SamplerLoopMode,
    amount: f32,
    offset: isize,
    frame: impl Fn(usize) -> (f32, f32),
) -> (f32, f32) {
    if frames == 0 {
        return (0.0, 0.0);
    }
    let period = match mode {
        SamplerLoopMode::PingPong => 2 * frames.saturating_sub(1).max(1),
        SamplerLoopMode::Off | SamplerLoopMode::Forward => frames,
    };
    let phase = offset.rem_euclid(period as isize) as usize;
    let mapped = |phase: usize| {
        if frames == 1 {
            0
        } else if mode == SamplerLoopMode::PingPong {
            phase.min(period - phase)
        } else {
            phase
        }
    };
    let dry = frame(mapped(phase));
    if mode == SamplerLoopMode::Off || !amount.is_finite() || amount <= 0.0 {
        return dry;
    }
    let count =
        ((f64::from(amount.clamp(0.0, 1.0)) * frames as f64).round() as usize).min(frames / 2);
    if count == 0 || phase < period - count {
        return dry;
    }
    let tail = phase - (period - count);
    let start = frame(mapped(tail));
    if tail + 1 == count {
        return start;
    }
    let angle = (tail + 1) as f32 / count as f32 * std::f32::consts::FRAC_PI_2;
    let (wet_gain, dry_gain) = angle.sin_cos();
    (
        dry.0 * dry_gain + start.0 * wet_gain,
        dry.1 * dry_gain + start.1 * wet_gain,
    )
}

#[inline]
fn interpolate(position: f64, frame: impl Fn(isize) -> (f32, f32)) -> (f32, f32) {
    let index = position.floor() as isize;
    let fraction = (position - index as f64) as f32;
    let current = frame(index);
    if fraction == 0.0 {
        return current;
    }
    let before = frame(index - 1);
    let next = frame(index + 1);
    let after = frame(index + 2);
    (
        hermite(before.0, current.0, next.0, after.0, fraction),
        hermite(before.1, current.1, next.1, after.1, fraction),
    )
}

/// Catmull-Rom spline through four evenly spaced points, evaluated
/// `fraction` of the way from `y1` to `y2`.
#[inline]
fn hermite(y0: f32, y1: f32, y2: f32, y3: f32, fraction: f32) -> f32 {
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * fraction + c2) * fraction + c1) * fraction + y1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

/// An attack, decay, sustain, release envelope, advanced one frame at a
/// time.
///
/// The attack is a straight line from silence to full level. The decay and
/// the release are exponential curves, the decay from full level down to
/// the sustain level and the release from wherever the level is down to
/// silence.
///
/// Each stage counts its frames and ends on the frame its time says. The
/// level is worked out from that count, never the other way round: a level
/// that is nudged along a little every frame stops moving once the nudge is
/// smaller than the level can resolve, and then the stage never ends.
#[derive(Debug, Clone, Copy)]
struct EnvelopeState {
    stage: Stage,
    /// Frames spent in the current stage.
    elapsed: u64,
    attack_frames: u64,
    decay_frames: u64,
    release_frames: u64,
    sustain: f32,
    /// Level the release started from.
    release_from: f32,
    /// The plain exponential of the decay or release under way: 1 as the
    /// stage starts, multiplied by the stage's ratio every frame. Sixty
    /// seconds at 384 kHz are 23 million multiplications, which double
    /// precision carries without visible drift.
    curve: f64,
    decay_ratio: f64,
    release_ratio: f64,
}

/// What the plain exponential is multiplied by every frame to come down to
/// its floor in `frames` frames.
fn curve_ratio(frames: u64) -> f64 {
    ((CURVE_FLOOR / (1.0 + CURVE_FLOOR)).ln() / frames.max(1) as f64).exp()
}

impl EnvelopeState {
    fn set_release_ms(&mut self, ms: f32, sample_rate: f64) {
        // Release duration is selected at key-up; later edits preserve the
        // already-running release trajectory.
        if matches!(self.stage, Stage::Release | Stage::Done) {
            return;
        }
        let frames = (f64::from(ms.max(MIN_RELEASE_SECONDS * 1000.0)) * sample_rate / 1000.0)
            .round()
            .max(1.0) as u64;
        if frames != self.release_frames {
            self.release_frames = frames;
            self.release_ratio = curve_ratio(frames);
        }
    }
    fn new(envelope: &Envelope, sample_rate: f64) -> Self {
        let frames = |ms: f32| (f64::from(ms) * sample_rate / 1000.0).round() as u64;
        let decay_frames = frames(envelope.decay_ms);
        let release_frames = frames(envelope.release_ms.max(MIN_RELEASE_SECONDS * 1000.0)).max(1);
        let mut state = Self {
            stage: Stage::Attack,
            elapsed: 0,
            attack_frames: frames(envelope.attack_ms),
            decay_frames,
            release_frames,
            sustain: envelope.sustain,
            release_from: 0.0,
            curve: 1.0,
            decay_ratio: curve_ratio(decay_frames),
            release_ratio: curve_ratio(release_frames),
        };
        state.settle();
        state
    }

    /// Moves on from every stage whose time is up. A stage of no length is
    /// passed without a single frame.
    fn settle(&mut self) {
        loop {
            let (frames, next) = match self.stage {
                Stage::Attack => (self.attack_frames, Stage::Decay),
                Stage::Decay => (self.decay_frames, Stage::Sustain),
                Stage::Release => (self.release_frames, Stage::Done),
                Stage::Sustain | Stage::Done => return,
            };
            if self.elapsed < frames {
                return;
            }
            self.stage = next;
            self.elapsed = 0;
            self.curve = 1.0;
        }
    }

    /// How much of the decay or release under way is still to come: 1 on
    /// its first frame, and 0 on the frame after its last.
    fn fall(&self) -> f64 {
        ((1.0 + CURVE_FLOOR) * self.curve - CURVE_FLOOR).max(0.0)
    }

    /// The level of the frame about to play.
    fn level(&self) -> f32 {
        match self.stage {
            Stage::Attack => (self.elapsed as f64 / self.attack_frames as f64) as f32,
            Stage::Decay => {
                let sustain = f64::from(self.sustain);
                (sustain + (1.0 - sustain) * self.fall()) as f32
            }
            Stage::Sustain => self.sustain,
            Stage::Release => (f64::from(self.release_from) * self.fall()) as f32,
            Stage::Done => 0.0,
        }
    }

    /// The level for this frame, or `None` once the envelope has run out.
    #[inline]
    fn next(&mut self) -> Option<f32> {
        match self.stage {
            Stage::Done => return None,
            // Holding a level of zero would only burn a voice.
            Stage::Sustain if self.sustain <= 0.0 => return None,
            Stage::Sustain => return Some(self.sustain),
            Stage::Attack | Stage::Decay | Stage::Release => {}
        }
        let level = self.level();
        self.elapsed += 1;
        match self.stage {
            Stage::Decay => self.curve *= self.decay_ratio,
            Stage::Release => self.curve *= self.release_ratio,
            Stage::Attack | Stage::Sustain | Stage::Done => {}
        }
        self.settle();
        Some(level)
    }

    fn release(&mut self) {
        if matches!(self.stage, Stage::Release | Stage::Done) {
            return;
        }
        self.release_from = self.level();
        self.stage = if self.release_from > 0.0 {
            Stage::Release
        } else {
            Stage::Done
        };
        self.elapsed = 0;
        self.curve = 1.0;
    }

    fn releasing(&self) -> bool {
        self.stage == Stage::Release
    }
}

struct Voice {
    instance: NoteInstanceId,
    channel_envelopes: Option<PreparedChannelEnvelopes>,
    modulation_released: bool,
    channel_pitch: f64,
    glide_coefficient: f64,
    source: Option<crate::note_curves::CurveSource>,
    expression: NoteExpression,
    release_base_ms: f32,
    active: bool,
    /// Held for as long as the voice may read it. A stopped voice can still
    /// hold its sample for a moment when there was no room to hand it back.
    sample: Option<AudioBuffer>,
    bank: Option<std::sync::Arc<crate::sampler_processing::SamplerBank>>,
    origin: Origin,
    /// The channel that started the voice. Meaningless for a preview.
    channel: ChannelId,
    route: Route,
    key: u8,
    color_group: u8,
    cut_group: u8,
    /// Frame the voice started on.
    started: u64,
    /// Tick on the sequencer's clock on which the note ends. It is kept as
    /// a tick, not as a frame, so the end follows the tempo when that
    /// changes under a sounding note. Voices without an envelope ignore it.
    end: f64,
    region: Region,
    loop_region: Option<LoopRegion>,
    loop_cycled: bool,
    /// Frames into the region, in playing order.
    position: f64,
    /// Sample frames to advance per output frame: pitch times the ratio of
    /// the sample's rate to the output rate.
    step: f64,
    pitch: f64,
    pitch_base_step: f64,
    glide: Option<NoteGlide>,
    /// Velocity times the sampler's own gain.
    gain: f32,
    /// The note's pan, added to the channel's.
    pan: f32,
    /// `None` plays the sample to its end at full level.
    envelope: Option<EnvelopeState>,
    expression_filter: Option<SvfCoeffs>,
    /// The channel's own filter, when it is switched on.
    channel_filter: Option<VoiceFilter>,
    /// The last modulated coefficients with the cutoff offset, resonance
    /// offset and note expression they were made from, so a filter that is
    /// not moving is not designed again on every frame.
    filter_cache: Option<([f32; 4], SvfCoeffs)>,
    filters: [Svf; 2],
    /// Frames left of an early fade-out. Zero means the voice is not fading.
    fade: u32,
    /// Frames left of a fade-in. A voice whose channel moved to another
    /// mixer track comes up on the new track while a copy of it fades out
    /// on the old one.
    fade_in: u32,
}

impl Voice {
    fn idle() -> Self {
        Self {
            instance: NoteInstanceId(0),
            channel_envelopes: None,
            modulation_released: false,
            channel_pitch: 0.0,
            glide_coefficient: 1.0,
            source: None,
            expression: NoteExpression::NEUTRAL,
            release_base_ms: 0.0,
            active: false,
            sample: None,
            bank: None,
            origin: Origin::Sequenced,
            channel: ChannelId(0),
            route: Route::Master,
            key: 0,
            color_group: 0,
            cut_group: 0,
            started: 0,
            end: f64::INFINITY,
            region: Region {
                first: 0,
                frames: 0,
                reverse: false,
            },
            loop_region: None,
            loop_cycled: false,
            position: 0.0,
            step: 1.0,
            pitch: 0.0,
            pitch_base_step: 1.0,
            glide: None,
            gain: 0.0,
            pan: 0.0,
            envelope: None,
            expression_filter: None,
            channel_filter: None,
            filter_cache: None,
            filters: [Svf::default(); 2],
            fade: 0,
            fade_in: 0,
        }
    }

    /// The voice as it carries on in another slot. Shares the sample.
    fn copy(&self) -> Self {
        Self {
            sample: self.sample.clone(),
            bank: self.bank.clone(),
            ..*self
        }
    }

    /// Adds the voice's next frames to `out`, a stretch of its mixer track's
    /// buffer that begins on frame `start`. The note ends on frame
    /// `release_at`. Returns true when the voice has ended.
    ///
    /// Every piece of state moves one frame at a time, so cutting the output
    /// at different places gives the same samples.
    // Per-voice rendering borrows the output and immutable mix/plan alongside
    // frame, fade, and clock context; keep this realtime boundary explicit.
    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        out: &mut [Frame],
        mix: Mix,
        start: u64,
        release_at: u64,
        fade_frames: u32,
        clock: Clock,
        plan: &Plan,
        sample_rate: f64,
    ) -> bool {
        let Some(sample) = &self.sample else {
            return true;
        };
        // A strip that is still gliding is read again on every frame.
        let (gliding, mut left, mut right) = match mix {
            Mix::Sides(left, right) => (None, left, right),
            Mix::Strip(strip) => {
                let (left, right) =
                    stereo_gains(strip.gain.at(start), strip.pan.at(start) + self.pan);
                let steady = strip.gain.settled(start)
                    && strip.pan.settled(start)
                    && !self.source.is_some_and(|source| source.active);
                ((!steady).then_some(strip), left, right)
            }
        };

        let output_frames = out.len();
        let mut modulation = ModulationBlock::default();
        let mut modulation_at = 0;
        let mut modulation_end = 0;
        for (offset, out) in out.iter_mut().enumerate() {
            let frame = start + offset as u64;
            if let Some(envelopes) = &mut self.channel_envelopes {
                if frame >= release_at && !self.modulation_released {
                    envelopes.gate_off();
                    self.modulation_released = true;
                    modulation_end = offset;
                }
                if offset >= modulation_end {
                    let until_release = if self.modulation_released {
                        usize::MAX
                    } else {
                        release_at.saturating_sub(frame) as usize
                    };
                    envelopes.process_block(
                        (output_frames - offset).min(until_release.max(1)),
                        &mut modulation,
                    );
                    modulation_at = offset;
                    modulation_end = offset + modulation.frames;
                }
            }
            let mod_index = if self.channel_envelopes.is_some() {
                offset - modulation_at
            } else {
                0
            };
            let cutoff_offset = modulation.filter_cutoff_offset[mod_index];
            let pitch_cents = modulation.pitch_cents[mod_index];
            let pan_offset = modulation.pan[mod_index];
            let resonance_offset = modulation.resonance[mod_index];
            let tremolo = modulation.volume[mod_index];
            if let Some(source) = self.source.filter(|source| source.active) {
                let (pan, expression) = source.controls(plan, clock.tick_at(frame));
                let moved =
                    f64::from(expression.fine_pitch_cents - self.expression.fine_pitch_cents)
                        / 100.0;
                if moved != 0.0 {
                    self.pitch += moved;
                    if let Some(glide) = &mut self.glide {
                        glide.from += moved;
                        glide.to += moved;
                    }
                    if self.glide.is_none() {
                        self.step = self.pitch_base_step * (self.pitch / 12.0).exp2();
                    }
                }
                if expression.modulation_x != self.expression.modulation_x
                    || expression.modulation_y != self.expression.modulation_y
                {
                    self.expression_filter = (expression.modulation_x != 0.5
                        || expression.modulation_y != 0.5)
                        .then(|| {
                            SvfCoeffs::new(
                                (20_000.0 * expression.cutoff_octaves().exp2())
                                    .clamp(20.0, sample_rate as f32 * 0.45),
                                std::f32::consts::FRAC_1_SQRT_2 * expression.resonance_multiplier(),
                                sample_rate as f32,
                            )
                        });
                }
                if expression.release != self.expression.release
                    && let Some(envelope) = &mut self.envelope
                {
                    envelope.set_release_ms(
                        self.release_base_ms * expression.release_multiplier(),
                        sample_rate,
                    );
                }
                self.expression = expression;
                self.pan = pan;
            }
            if let Some(glide) = self.glide {
                let tick = clock.tick_at(frame);
                self.pitch = glide.at(tick);
                self.step = self.pitch_base_step * (self.pitch / 12.0).exp2();
                if tick >= glide.end {
                    self.glide = None;
                }
            }
            if self.origin != Origin::Preview {
                self.channel_pitch += self.glide_coefficient * (self.pitch - self.channel_pitch);
                self.step = self.pitch_base_step
                    * (self.channel_pitch / 12.0 + f64::from(pitch_cents) / 1200.0).exp2();
            }
            if let Mix::Strip(strip) = mix
                && self.channel_envelopes.is_some()
            {
                (left, right) = stereo_gains(
                    strip.gain.at(frame),
                    (strip.pan.at(frame) + self.pan + pan_offset).clamp(-1.0, 1.0),
                );
            }
            if let Some(strip) = gliding {
                (left, right) = stereo_gains(
                    strip.gain.at(frame),
                    (strip.pan.at(frame) + self.pan + pan_offset).clamp(-1.0, 1.0),
                );
            }
            let mut level = self.gain * tremolo;
            if let Some(envelope) = &mut self.envelope {
                if frame >= release_at {
                    envelope.release();
                }
                match envelope.next() {
                    Some(envelope_level) => level *= envelope_level,
                    None => return true,
                }
            }
            if self.fade > 0 {
                level *= self.fade as f32 / fade_frames as f32;
            }
            if self.fade_in > 0 {
                level *= 1.0 - self.fade_in as f32 / fade_frames as f32;
                self.fade_in -= 1;
            }
            let (mut sample_left, mut sample_right) = match self.loop_region {
                Some(loop_region) => {
                    loop_region.read(self.region, sample, self.position, self.loop_cycled)
                }
                None => self.region.read(sample, self.position),
            };
            // The channel filter, with cutoff and resonance moved by their
            // envelopes and LFOs and by the note's own expression. With the
            // filter off, a modulation still sweeps the 20 kHz low-pass that
            // note expression uses.
            let modulated_filter = if self.channel_filter.is_some()
                || cutoff_offset != 0.0
                || resonance_offset != 0.0
            {
                let key = [
                    cutoff_offset,
                    resonance_offset,
                    self.expression.modulation_x,
                    self.expression.modulation_y,
                ];
                let coeffs = match self.filter_cache {
                    Some((made_from, coeffs)) if made_from == key => coeffs,
                    _ => {
                        let (base, amount) = self
                            .channel_filter
                            .map_or((20_000.0, 0.0), |filter| (filter.cutoff_hz, filter.resonance));
                        let coeffs = SvfCoeffs::new(
                            (base * (self.expression.cutoff_octaves() + cutoff_offset).exp2())
                                .clamp(20.0, sample_rate as f32 * 0.45),
                            VoiceFilter::q((amount + resonance_offset).clamp(0.0, 1.0))
                                * self.expression.resonance_multiplier(),
                            sample_rate as f32,
                        );
                        self.filter_cache = Some((key, coeffs));
                        coeffs
                    }
                };
                Some(coeffs)
            } else {
                None
            };
            if let Some(coeffs) = modulated_filter
                .as_ref()
                .or(self.expression_filter.as_ref())
            {
                let mode = self
                    .channel_filter
                    .map_or(VoiceFilterMode::LowPass, |filter| filter.mode);
                let left = self.filters[0].tick(coeffs, sample_left);
                let right = self.filters[1].tick(coeffs, sample_right);
                (sample_left, sample_right) = match mode {
                    VoiceFilterMode::LowPass => (left.low, right.low),
                    VoiceFilterMode::HighPass => (left.high, right.high),
                    // Scaled by the damping, so the peak passes at unity.
                    VoiceFilterMode::BandPass => (left.band * coeffs.k, right.band * coeffs.k),
                    VoiceFilterMode::Notch => (left.low + left.high, right.low + right.high),
                };
            }
            out[0] += sample_left * level * left;
            out[1] += sample_right * level * right;

            if let Some(loop_region) = self.loop_region {
                loop_region.advance(&mut self.position, &mut self.loop_cycled, self.step);
            } else {
                self.position += self.step;
            }
            if self.fade > 0 {
                self.fade -= 1;
                if self.fade == 0 {
                    return true;
                }
            }
            if self.loop_region.is_none() && self.position >= self.region.frames as f64 {
                return true;
            }
        }
        false
    }
}

#[inline]
fn stereo_gains(gain: f32, pan: f32) -> (f32, f32) {
    let (left, right) = pan_gains(pan);
    (gain * left, gain * right)
}

pub(crate) struct VoicePool {
    voices: Box<[Voice]>,
    sample_rate: f64,
    fade_frames: u32,
    preview_gain: f32,
    next_instance: u64,
}

impl VoicePool {
    pub fn new(sample_rate: u32) -> Self {
        let sample_rate = f64::from(sample_rate);
        Self {
            voices: (0..MAX_VOICES + FADING_SLOTS)
                .map(|_| Voice::idle())
                .collect(),
            sample_rate,
            fade_frames: ((FADE_SECONDS * sample_rate).round() as u32).max(1),
            preview_gain: db_to_gain(PREVIEW_GAIN_DB),
            next_instance: 1,
        }
    }

    /// Voices sounding right now, fading ones included.
    pub fn active(&self) -> u32 {
        self.voices.iter().filter(|voice| voice.active).count() as u32
    }

    /// Where each voice that is heard through the mixer plays into, in the
    /// plan the voices are bound to.
    pub fn destinations(&self) -> impl Iterator<Item = Heard> {
        self.voices
            .iter()
            .filter(|voice| voice.active)
            .filter_map(|voice| match voice.route {
                Route::Channel(channel) => Some(Heard::Channel(channel)),
                Route::Track { track, .. } => Some(Heard::Track(track)),
                Route::Master => Some(Heard::Track(0)),
                Route::Apart { .. } => None,
            })
    }

    /// Hands back samples that stopped voices could not return earlier.
    pub fn sweep(&mut self, plan: &Plan, garbage: &mut Producer<Garbage>) {
        for index in 0..self.voices.len() {
            if !self.voices[index].active && self.voices[index].sample.is_some() {
                self.end(index, plan, garbage);
            }
        }
    }

    /// Starts a note on frame `now`, applying the channel's cut rules and
    /// stealing a voice if the pool is full.
    // Starting a voice transfers prepared envelopes while borrowing the plan
    // and deferred-drop queue, with timing kept explicit for cut/steal ordering.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        plan: &Plan,
        garbage: &mut Producer<Garbage>,
        note: Note,
        now: u64,
        clock: Clock,
        glide_end: f64,
        mut channel_envelopes: PreparedChannelEnvelopes,
        glide_coefficient: f64,
    ) -> Option<NoteInstanceId> {
        let expression = note.expression.clamped();
        let channel = &plan.channels[note.channel];
        let sampler = &channel.sampler;
        let at = clock.tick_at(now);
        let from = self
            .voices
            .iter()
            .filter(|voice| {
                voice.active
                    && voice.channel == channel.id
                    && voice.origin == note.origin
                    && voice.color_group == expression.color_group.unwrap_or(0)
                    && voice.fade == 0
                    && voice.end > at
            })
            .max_by_key(|voice| voice.started)
            .map(|voice| voice.glide.map_or(voice.pitch, |glide| glide.at(at)));
        let selected = if sampler.spectral {
            sampler.bank.as_ref().and_then(|bank| bank.at(note.key))
        } else {
            sampler.sample.as_ref()
        };
        let sample = selected?;

        // Notes that start on the same frame are a chord, not a retrigger,
        // so only voices from before this frame are cut.
        let fade_frames = self.fade_frames;
        for voice in self
            .voices
            .iter_mut()
            .filter(|voice| voice.active && voice.origin != Origin::Preview && voice.started < now)
        {
            let same_channel = voice.channel == channel.id;
            let cut_by_self = sampler.cut_self && same_channel;
            let cut_by_group =
                sampler.cut_group != 0 && voice.cut_group == sampler.cut_group && !same_channel;
            if (cut_by_self || cut_by_group) && voice.fade == 0 {
                voice.fade = fade_frames;
            }
        }

        // A note with no velocity cuts like any other note-on, but it can
        // never be heard, so it does not get a voice.
        if note.velocity <= 0.0 {
            return None;
        }

        self.make_room();
        let slot = self.free_slot(plan, garbage)?;
        let expression = note.expression.clamped();
        let pitch = if sampler.spectral {
            2.0_f64.powf(f64::from(expression.fine_pitch_cents) / 1200.0)
        } else {
            2.0_f64.powf(
                f64::from(
                    f32::from(note.key) + sampler.key_offset + expression.fine_pitch_cents / 100.0,
                ) / 12.0,
            )
        };
        let target_pitch = f64::from(note.key) + f64::from(expression.fine_pitch_cents) / 100.0;
        let glide = if expression.articulation == NoteArticulation::Portamento {
            from.map(|from| NoteGlide::new(at, glide_end, from, target_pitch))
        } else {
            None
        };
        let pitch_base_step = pitch * f64::from(sample.sample_rate())
            / self.sample_rate
            / (target_pitch / 12.0).exp2();
        let current_pitch = glide.map_or(target_pitch, |glide| glide.from);
        let instance = NoteInstanceId(self.next_instance);
        self.next_instance = self.next_instance.wrapping_add(1);
        channel_envelopes.gate_on();
        let channel_filter = Some(channel_envelopes.filter()).filter(|filter| filter.enabled);
        self.voices[slot] = Voice {
            instance,
            channel_filter,
            filter_cache: None,
            channel_envelopes: channel_envelopes.enabled().then_some(channel_envelopes),
            modulation_released: false,
            channel_pitch: if glide_coefficient < 1.0 {
                from.unwrap_or(current_pitch)
            } else {
                current_pitch
            },
            glide_coefficient,
            active: true,
            sample: Some(sample.clone()),
            source: note.source,
            expression,
            release_base_ms: sampler.envelope.map_or(0.0, |envelope| envelope.release_ms),
            bank: sampler.bank.clone(),
            origin: note.origin,
            channel: channel.id,
            route: Route::Channel(note.channel),
            key: note.key,
            color_group: expression.color_group.unwrap_or(0),
            cut_group: sampler.cut_group,
            started: now,
            end: note.end,
            region: Region {
                first: if sampler.spectral { 0 } else { sampler.start },
                frames: if sampler.spectral {
                    sample.frames()
                } else {
                    sampler.end - sampler.start
                },
                reverse: !sampler.spectral && sampler.reverse,
            },
            position: 0.0,
            loop_region: sampler.loop_region,
            loop_cycled: sampler.loop_region.is_some_and(|region| region.first == 0),
            step: if glide.is_some() {
                pitch_base_step * (current_pitch / 12.0).exp2()
            } else {
                pitch * f64::from(sample.sample_rate()) / self.sample_rate
            },
            pitch: current_pitch,
            pitch_base_step,
            glide,
            gain: note.velocity * sampler.gain,
            pan: note.pan,
            envelope: sampler.envelope.as_ref().map(|envelope| {
                let mut envelope = *envelope;
                envelope.release_ms *= expression.release_multiplier();
                EnvelopeState::new(&envelope, self.sample_rate)
            }),
            expression_filter: (expression.modulation_x != 0.5 || expression.modulation_y != 0.5)
                .then(|| {
                    SvfCoeffs::new(
                        (20_000.0 * expression.cutoff_octaves().exp2())
                            .clamp(20.0, self.sample_rate as f32 * 0.45),
                        std::f32::consts::FRAC_1_SQRT_2 * expression.resonance_multiplier(),
                        self.sample_rate as f32,
                    )
                }),
            filters: [Svf::default(); 2],
            fade: 0,
            fade_in: 0,
        };
        Some(instance)
    }

    /// Plays a whole buffer straight into the master at
    /// [`PREVIEW_GAIN_DB`], replacing any preview already playing. Gives
    /// the buffer back when it cannot be played.
    pub fn preview(
        &mut self,
        plan: &Plan,
        garbage: &mut Producer<Garbage>,
        sample: AudioBuffer,
        now: u64,
    ) -> Option<AudioBuffer> {
        self.fade_origin(Origin::Preview);
        if sample.frames() == 0 {
            return Some(sample);
        }
        let Some(slot) = self.free_slot(plan, garbage) else {
            return Some(sample);
        };
        self.voices[slot] = Voice {
            active: true,
            origin: Origin::Preview,
            route: Route::Master,
            started: now,
            region: Region {
                first: 0,
                frames: sample.frames(),
                reverse: false,
            },
            step: f64::from(sample.sample_rate()) / self.sample_rate,
            gain: self.preview_gain,
            sample: Some(sample),
            bank: None,
            ..Voice::idle()
        };
        None
    }

    pub fn reset_channel_modulation(&mut self, channel: ChannelId) {
        for voice in self
            .voices
            .iter_mut()
            .filter(|voice| voice.active && voice.channel == channel)
        {
            if let Some(envelopes) = &mut voice.channel_envelopes {
                envelopes.reset();
            }
        }
    }

    pub fn release_instance(&mut self, channel: ChannelId, id: NoteInstanceId) {
        self.end_instance(channel, id, true);
    }

    /// Ordinary key-up releases envelopes; a one-shot still plays to its end.
    pub fn key_up_instance(&mut self, channel: ChannelId, id: NoteInstanceId) {
        self.end_instance(channel, id, false);
    }

    fn end_instance(&mut self, channel: ChannelId, id: NoteInstanceId, force: bool) {
        for voice in self
            .voices
            .iter_mut()
            .filter(|voice| voice.active && voice.channel == channel && voice.instance == id)
        {
            voice.end = f64::NEG_INFINITY;
            if let Some(envelopes) = &mut voice.channel_envelopes {
                envelopes.gate_off();
                voice.modulation_released = true;
            }
            if force && voice.envelope.is_none() {
                voice.fade = self.fade_frames;
            }
        }
    }

    pub fn retarget_instance(
        &mut self,
        plan: &Plan,
        id: NoteInstanceId,
        note: Note,
        at: f64,
        glide_end: f64,
    ) -> bool {
        let channel = &plan.channels[note.channel];
        let Some(voice) = self.voices.iter_mut().find(|voice| {
            voice.active && voice.channel == channel.id && voice.instance == id && voice.fade == 0
        }) else {
            return false;
        };
        if channel.sampler.spectral {
            // Existing prepared bank selection, preserving envelope and playback position.
            if let Some(sample) = channel
                .sampler
                .bank
                .as_ref()
                .and_then(|bank| bank.at(note.key))
            {
                voice.sample = Some(sample.clone());
                voice.region.frames = sample.frames();
                voice.position = voice.position.min(sample.frames().saturating_sub(1) as f64);
                let pitch =
                    f64::from(note.key) + f64::from(note.expression.fine_pitch_cents) / 100.0;
                voice.pitch_base_step = f64::from(sample.sample_rate()) / self.sample_rate
                    * (-f64::from(note.key) / 12.0).exp2();
                voice.pitch = pitch;
            } else {
                return false;
            }
        } else {
            voice.pitch = f64::from(note.key) + f64::from(note.expression.fine_pitch_cents) / 100.0;
        }
        let target = voice.pitch;
        voice.glide = if note.expression.articulation == NoteArticulation::Portamento {
            Some(NoteGlide::new(at, glide_end, voice.channel_pitch, target))
        } else {
            None
        };
        voice.pitch = voice.glide.map_or(target, |glide| glide.from);
        voice.key = note.key;
        voice.source = note.source;
        voice.expression = note.expression.clamped();
        voice.pan = note.pan;
        voice.end = note.end;
        voice.origin = note.origin;
        true
    }

    /// Moves the end of every note by `ticks`. The sequencer's clock reads
    /// that much more from now on because playback jumped, and the notes
    /// still sounding are to last as long as they would have.
    pub fn shift_ends(&mut self, ticks: f64) {
        for voice in &mut self.voices {
            voice.end += ticks;
            if let Some(source) = &mut voice.source {
                source.shift(ticks);
            }
            if let Some(glide) = &mut voice.glide {
                glide.shift(ticks);
            }
        }
    }

    /// Gives every note the end `moved` makes of the one it has: the tempo
    /// map changed, and each clock tick now means another place in the
    /// song.
    pub fn move_ends(&mut self, moved: impl Fn(f64) -> f64) {
        for voice in self.voices.iter_mut().filter(|voice| voice.active) {
            voice.end = moved(voice.end);
            if let Some(source) = &mut voice.source {
                source.move_clock(&moved);
            }
            if let Some(glide) = &mut voice.glide {
                glide.move_clock(&moved);
            }
        }
    }

    /// Fades out every voice with this origin.
    pub fn fade_origin(&mut self, origin: Origin) {
        let fade_frames = self.fade_frames;
        for voice in &mut self.voices {
            if voice.active && voice.origin == origin && voice.fade == 0 {
                voice.fade = fade_frames;
            }
        }
    }

    /// Gets the voices ready for a new plan that takes over on frame `now`.
    ///
    /// A voice that carries on is pointed at its channel's place in the new
    /// plan. If the channel now plays into another mixer track, the voice
    /// fades in there while a copy of it fades out where it was, so the
    /// move is a crossfade and not a jump.
    ///
    /// A voice that is fading out for good, a voice whose channel the new
    /// plan no longer has, which starts fading here, and the copy a moved
    /// voice leaves behind all finish at the gain their channel gave each
    /// side of them on this frame. Where they finish depends on what lay
    /// between them and the output under the old plan:
    ///
    /// - Nothing but faders and pans: they leave the mix and go straight
    ///   to the output at the level they were heard at. That is the same
    ///   sound, and it leaves the new plan's faders free to change at
    ///   once, since nothing is heard through them.
    /// - An effect or a compensation delay: they stay in the track they
    ///   were in for as long as the new plan has it, and are heard through
    ///   its effects and everything after it like any other sound there.
    ///   Taking them out would be heard as a jump. If the track is gone
    ///   too, they have nowhere to finish but the output.
    ///
    /// What is left in the mix is exactly what the new plan's gains have to
    /// be gentle with.
    pub fn rebind(&mut self, plan: &Plan, old_plan: &Plan, old_state: &PlanState, now: u64) {
        let fade_frames = self.fade_frames;
        let through = old_state.gains_to_output(old_plan, now);
        // Where a voice that was in the old plan's track `old_track`, at
        // these gains, finishes.
        let finish = |old_track: usize, left: f32, right: f32| {
            let id = old_plan.tracks[old_track].id;
            let stays = plan
                .track_ids
                .get(id.0)
                .filter(|_| old_state.shaped[old_track]);
            match stays {
                Some(track) => Route::Track {
                    track,
                    id,
                    left,
                    right,
                },
                None => {
                    let [track_left, track_right] = through[old_track];
                    Route::Apart {
                        left: left * track_left,
                        right: right * track_right,
                    }
                }
            }
        };

        for index in 0..self.voices.len() {
            let voice = &mut self.voices[index];
            if !voice.active {
                continue;
            }
            if let Some(source) = &mut voice.source {
                source.refresh(plan);
            }
            let leaving = voice.fade > 0;
            match voice.route {
                Route::Channel(old_index) => {
                    let strip = &old_state.channels[old_index];
                    let (left, right) =
                        stereo_gains(strip.gain.at(now), strip.pan.at(now) + voice.pan);
                    let old_track = old_plan.channels[old_index].track;
                    let kept = plan.channel(voice.channel).filter(|_| !leaving);
                    let Some(new_index) = kept else {
                        voice.source = None;
                        voice.route = finish(old_track, left, right);
                        if !leaving {
                            voice.fade = fade_frames;
                        }
                        continue;
                    };
                    voice.route = Route::Channel(new_index);
                    let moved = plan.tracks[plan.channels[new_index].track].id
                        != old_plan.tracks[old_track].id;
                    if !moved {
                        continue;
                    }
                    let free = |voice: &Voice| !voice.active && voice.sample.is_none();
                    // With no slot to spare the voice changes track at once.
                    let Some(spare) = self.voices.iter().position(free) else {
                        continue;
                    };
                    let mut behind = self.voices[index].copy();
                    behind.source = None;
                    behind.route = finish(old_track, left, right);
                    behind.fade = fade_frames;
                    self.voices[spare] = behind;
                    self.voices[index].fade_in = fade_frames;
                }
                Route::Track {
                    track,
                    id,
                    left,
                    right,
                } => {
                    voice.route = match plan.track_ids.get(id.0) {
                        Some(track) => Route::Track {
                            track,
                            id,
                            left,
                            right,
                        },
                        None => {
                            let [track_left, track_right] = through[track];
                            Route::Apart {
                                left: left * track_left,
                                right: right * track_right,
                            }
                        }
                    };
                }
                Route::Master if leaving && !old_state.shaped[0] => {
                    let (left, right) = pan_gains(voice.pan);
                    let [master_left, master_right] = through[0];
                    voice.route = Route::Apart {
                        left: left * master_left,
                        right: right * master_right,
                    };
                }
                Route::Master | Route::Apart { .. } => {}
            }
        }
    }

    /// Adds every voice's output to its mixer track over `stretch`. `clock`
    /// says on which frame each note ends.
    pub fn render(
        &mut self,
        plan: &Plan,
        state: &PlanState,
        clock: Clock,
        garbage: &mut Producer<Garbage>,
        mixer: &mut Mixer,
        stretch: Stretch,
    ) {
        let Stretch { base, frames } = stretch;
        let start = base + frames.start as u64;
        for index in 0..self.voices.len() {
            let voice = &mut self.voices[index];
            if !voice.active {
                continue;
            }
            let (mix, out) = match voice.route {
                Route::Channel(channel) => (
                    Mix::Strip(state.channels[channel]),
                    mixer.track_mut(plan.channels[channel].track),
                ),
                Route::Master => (Mix::Strip(Strip::at_rest(1.0, 0.0)), mixer.track_mut(0)),
                Route::Track {
                    track, left, right, ..
                } => (Mix::Sides(left, right), mixer.track_mut(track)),
                Route::Apart { left, right } => (Mix::Sides(left, right), mixer.apart_mut()),
            };
            let out = &mut out[frames.clone()];
            let release_at = clock.frame_of(voice.end);
            if voice.render(
                out,
                mix,
                start,
                release_at,
                self.fade_frames,
                clock,
                plan,
                self.sample_rate,
            ) {
                self.end(index, plan, garbage);
            }
        }
    }

    pub fn slide_notes(
        &mut self,
        channel: ChannelId,
        key: u8,
        expression: NoteExpression,
        at: f64,
        end: f64,
        origin: Origin,
    ) {
        let group = expression.color_group.unwrap_or(0);
        let Some(anchor) = self
            .voices
            .iter()
            .filter(|voice| {
                voice.active
                    && voice.channel == channel
                    && voice.origin == origin
                    && voice.color_group == group
                    && voice.fade == 0
                    && voice.end > at
            })
            .max_by_key(|voice| voice.started)
        else {
            return;
        };
        let delta = f64::from(key.min(127))
            + f64::from(expression.clamped().fine_pitch_cents) / 100.0
            - anchor.glide.map_or(anchor.pitch, |glide| glide.at(at));
        for voice in self.voices.iter_mut().filter(|voice| {
            voice.active
                && voice.channel == channel
                && voice.origin == origin
                && voice.color_group == group
                && voice.fade == 0
                && voice.end > at
        }) {
            let from = voice.glide.map_or(voice.pitch, |glide| glide.at(at));
            voice.pitch = from;
            voice.glide = Some(NoteGlide::new(
                at,
                end,
                from,
                (from + delta).clamp(-12.0, 139.0),
            ));
        }
    }

    /// Keeps the number of voices at full level within [`MAX_VOICES`] by
    /// fading one out when a new voice is about to join: the quietest voice
    /// already in its release if there is one, otherwise the oldest.
    fn make_room(&mut self) {
        let sounding =
            |voice: &&mut Voice| voice.active && voice.fade == 0 && voice.origin != Origin::Preview;
        if self.voices.iter_mut().filter(sounding).count() < MAX_VOICES {
            return;
        }
        let release_level = |voice: &Voice| {
            voice
                .envelope
                .filter(EnvelopeState::releasing)
                .map(|envelope| envelope.level())
        };
        let mut victim: Option<&mut Voice> = None;
        for voice in self.voices.iter_mut().filter(sounding) {
            let better = match &victim {
                None => true,
                Some(best) => match (release_level(voice), release_level(best)) {
                    (Some(level), Some(best_level)) => level < best_level,
                    (Some(_), None) => true,
                    (None, Some(_)) => false,
                    (None, None) => voice.started < best.started,
                },
            };
            if better {
                victim = Some(voice);
            }
        }
        if let Some(voice) = victim {
            voice.fade = self.fade_frames;
        }
    }

    /// A slot a new voice can take. When every slot is busy the pool is full
    /// of fading voices, and the one nearest silence is ended to make room.
    fn free_slot(&mut self, plan: &Plan, garbage: &mut Producer<Garbage>) -> Option<usize> {
        let free = |voice: &Voice| !voice.active && voice.sample.is_none();
        if let Some(index) = self.voices.iter().position(free) {
            return Some(index);
        }
        let index = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| voice.active && voice.fade > 0)
            .min_by_key(|(_, voice)| voice.fade)
            .map(|(index, _)| index)?;
        self.end(index, plan, garbage);
        free(&self.voices[index]).then_some(index)
    }

    /// Stops a voice and lets go of its sample without freeing memory here.
    fn end(&mut self, index: usize, plan: &Plan, garbage: &mut Producer<Garbage>) {
        let voice = &mut self.voices[index];
        voice.active = false;
        let Some(sample) = voice.sample.take() else {
            return;
        };
        if let Some(bank) = voice.bank.take() {
            if plan.holds_sampler_bank(&bank) {
                drop(sample);
                drop(bank);
            } else if garbage.is_full() {
                voice.sample = Some(sample);
                voice.bank = Some(bank);
            } else {
                drop(sample); // the bank still retains this buffer
                retire(garbage, Garbage::SamplerBank(bank));
            }
            return;
        }
        if plan.holds(&sample) {
            // The plan keeps the audio alive, so this drop frees nothing.
            drop(sample);
        } else if garbage.is_full() {
            voice.sample = Some(sample);
        } else {
            retire(garbage, Garbage::Sample(sample));
        }
    }
}

#[cfg(test)]
mod tests {
    use windfall_project::MAX_ENVELOPE_MS;

    use super::*;

    /// What a decay or release has left `elapsed` frames into `frames`.
    fn fall(elapsed: u64, frames: u64) -> f64 {
        let floor = CURVE_FLOOR / (1.0 + CURVE_FLOOR);
        (1.0 + CURVE_FLOOR) * floor.powf(elapsed as f64 / frames as f64) - CURVE_FLOOR
    }

    #[test]
    fn interpolation_passes_through_the_stored_points() {
        assert_eq!(hermite(0.3, 0.7, -0.2, 0.9, 0.0), 0.7);
        assert!((hermite(0.3, 0.7, -0.2, 0.9, 1.0) + 0.2).abs() < 1e-6);
        // A straight line stays a straight line.
        assert!((hermite(0.0, 1.0, 2.0, 3.0, 0.25) - 1.25).abs() < 1e-6);
    }

    #[test]
    fn a_reversed_region_reads_backwards() {
        let sample = AudioBuffer::from_interleaved(48_000, 1, vec![1.0, 2.0, 3.0, 4.0]);
        let region = Region {
            first: 1,
            frames: 2,
            reverse: true,
        };
        assert_eq!(region.read(&sample, 0.0), (3.0, 3.0));
        assert_eq!(region.read(&sample, 1.0), (2.0, 2.0));
        assert_eq!(region.frame(&sample, 2), (0.0, 0.0));
    }

    #[test]
    fn loop_interpolation_follows_periodic_taps_at_fractional_seams_and_turns() {
        // Outside values must never leak into a cycled loop's interpolation.
        let sample = AudioBuffer::from_interleaved(
            48_000,
            2,
            vec![9.0, -9.0, 0.2, -0.2, 0.8, -0.8, 9.0, -9.0],
        );
        let region = Region {
            first: 0,
            frames: 4,
            reverse: false,
        };
        for mode in [SamplerLoopMode::Forward, SamplerLoopMode::PingPong] {
            let loop_region = LoopRegion {
                first: 1,
                frames: 2,
                mode,
                crossfade: 0.0,
            };
            // Both two-frame modes alternate, and the half-way points average.
            for position in [1.5, 2.5] {
                let read = loop_region.read(region, &sample, position, true);
                assert!((read.0 - 0.5).abs() < 1e-6, "{mode:?}: {read:?}");
                assert_eq!(read.0, -read.1);
            }
            // An exact period plus a fraction must preserve the fraction.
            let mut position = 1.0;
            let mut cycled = false;
            loop_region.advance(&mut position, &mut cycled, 2000.5);
            assert_eq!(position, 1.5);
            assert!(cycled);
        }
    }

    #[test]
    fn ping_pong_turns_reflect_the_curve_and_do_not_duplicate_endpoints() {
        let sample = AudioBuffer::from_interleaved(48_000, 1, vec![0.1, 0.2, 0.4]);
        let region = Region {
            first: 0,
            frames: 3,
            reverse: false,
        };
        let loop_region = LoopRegion {
            first: 0,
            frames: 3,
            mode: SamplerLoopMode::PingPong,
            crossfade: 0.0,
        };
        let mut position = 0.0;
        let mut cycled = true;
        let got: Vec<f32> = (0..9)
            .map(|_| {
                let value = loop_region.read(region, &sample, position, cycled).0;
                loop_region.advance(&mut position, &mut cycled, 1.0);
                value
            })
            .collect();
        assert_eq!(got, [0.1, 0.2, 0.4, 0.2, 0.1, 0.2, 0.4, 0.2, 0.1]);
        for (a, b) in [(1.75, 2.25), (0.25, 3.75)] {
            assert!(
                (loop_region.read(region, &sample, a, true).0
                    - loop_region.read(region, &sample, b, true).0)
                    .abs()
                    < 1e-7
            );
        }
    }

    #[test]
    fn a_single_frame_loop_is_constant_at_fractional_and_large_rates() {
        let sample = AudioBuffer::from_interleaved(48_000, 1, vec![9.0, 0.25, 9.0]);
        let region = Region {
            first: 0,
            frames: 3,
            reverse: false,
        };
        for mode in [SamplerLoopMode::Forward, SamplerLoopMode::PingPong] {
            let loop_region = LoopRegion {
                first: 1,
                frames: 1,
                mode,
                crossfade: 0.0,
            };
            for step in [0.01, 0.5, 1.0, 13.7, 1000.25] {
                let mut position = 1.0;
                let mut cycled = true;
                for _ in 0..1000 {
                    assert_eq!(
                        loop_region.read(region, &sample, position, cycled),
                        (0.25, 0.25)
                    );
                    loop_region.advance(&mut position, &mut cycled, step);
                }
            }
        }
    }

    #[test]
    fn the_envelope_walks_through_its_stages() {
        let envelope = Envelope {
            attack_ms: 2.0,
            decay_ms: 2.0,
            sustain: 0.5,
            release_ms: 2.0,
        };
        // 1000 frames a second makes one frame one millisecond.
        let mut state = EnvelopeState::new(&envelope, 1000.0);
        let mut next = || state.next().expect("the envelope is still open");
        // Two frames up in a straight line.
        assert_eq!(next(), 0.0);
        assert_eq!(next(), 0.5);
        // Two frames down a curve that has all but arrived half way.
        let half_way = fall(1, 2) as f32;
        assert!((0.03..0.031).contains(&half_way));
        assert_eq!(next(), 1.0);
        assert!((next() - (0.5 + 0.5 * half_way)).abs() < 1e-6);
        assert_eq!(next(), 0.5);
        assert_eq!(next(), 0.5);
        state.release();
        assert_eq!(state.next(), Some(0.5));
        let level = state.next().expect("the release has a second frame");
        assert!((level - 0.5 * half_way).abs() < 1e-6);
        assert_eq!(state.next(), None);
    }

    #[test]
    fn stages_of_no_length_are_passed_over() {
        let envelope = Envelope {
            attack_ms: 0.0,
            decay_ms: 0.0,
            sustain: 0.25,
            release_ms: 0.0,
        };
        let mut state = EnvelopeState::new(&envelope, 48_000.0);
        assert_eq!(state.next(), Some(0.25));
        state.release();
        // The shortest release is a millisecond.
        let levels: Vec<f32> = std::iter::from_fn(|| state.next()).collect();
        assert_eq!(levels.len(), 48);
        assert_eq!(levels[0], 0.25);
    }

    #[test]
    fn a_decay_falls_at_a_steady_rate_in_decibels() {
        let envelope = Envelope {
            attack_ms: 0.0,
            decay_ms: 1_000.0,
            sustain: 0.0,
            release_ms: 1_000.0,
        };
        let mut state = EnvelopeState::new(&envelope, 48_000.0);
        let decay: Vec<f32> = (0..48_000).map(|_| state.next().unwrap()).collect();
        assert_eq!(state.next(), None, "a decay to silence ends the voice");

        // Every tenth of the time takes off the same 6 dB, a factor of two,
        // until the curve bends down to meet silence at the very end.
        for tenth in 0..5 {
            let ratio = decay[(tenth + 1) * 4_800] / decay[tenth * 4_800];
            assert!((0.49..0.51).contains(&ratio), "tenth {tenth}: {ratio}");
        }
        assert_eq!(decay[0], 1.0);
        assert!(decay.is_sorted_by(|a, b| a > b));
        assert!(decay[47_999] > 0.0 && decay[47_999] < 1e-6);
    }

    /// Runs an envelope with the same time for every stage and checks that
    /// each stage takes exactly its number of frames.
    fn assert_stages_end_on_time(ms: f32, sample_rate: u32, sustain: f32) {
        let what = format!("{ms} ms at {sample_rate} Hz, sustain {sustain}");
        let envelope = Envelope {
            attack_ms: ms,
            decay_ms: ms,
            sustain,
            release_ms: ms,
        };
        let frames = (f64::from(ms) * f64::from(sample_rate) / 1000.0).round() as u64;
        let mut state = EnvelopeState::new(&envelope, f64::from(sample_rate));
        // Where a curve of `frames` frames is when half its time is up.
        let half_way = fall(frames / 2, frames.max(1)) as f32;

        let mut last = 0.0;
        for frame in 0..frames {
            assert_eq!(state.stage, Stage::Attack, "{what}: frame {frame}");
            let level = state.next().unwrap();
            assert!(
                level >= last && level <= 1.0,
                "{what}: attack frame {frame}"
            );
            last = level;
        }
        last = 1.0;
        for frame in 0..frames {
            assert_eq!(state.stage, Stage::Decay, "{what}: frame {frame}");
            let level = state.next().unwrap();
            assert!(
                level <= last && level >= sustain,
                "{what}: decay frame {frame}"
            );
            if frame == 0 {
                assert_eq!(level, 1.0, "{what}: the decay starts from the top");
            }
            if frame == frames / 2 {
                let expected = sustain + (1.0 - sustain) * half_way;
                assert!((level - expected).abs() < 1e-5, "{what}: half way {level}");
            }
            last = level;
        }
        // The frame after the last of the decay is the first on the
        // sustain level, and the level stays there.
        assert_eq!(state.stage, Stage::Sustain, "{what}");
        for _ in 0..3 {
            assert_eq!(state.next(), Some(sustain), "{what}: sustain");
        }

        state.release();
        let shortest = (f64::from(MIN_RELEASE_SECONDS) * f64::from(sample_rate)).round() as u64;
        let release_frames = frames.max(shortest);
        last = sustain;
        for frame in 0..release_frames {
            assert_eq!(state.stage, Stage::Release, "{what}: frame {frame}");
            let level = state.next().unwrap();
            assert!(
                level <= last && level >= 0.0,
                "{what}: release frame {frame}"
            );
            if frame == release_frames / 2 {
                let expected = sustain * fall(frame, release_frames) as f32;
                assert!((level - expected).abs() < 1e-5, "{what}: half way {level}");
            }
            last = level;
        }
        assert_eq!(state.next(), None, "{what}: the release ran over");
        assert!(last < sustain * 0.01, "{what}: the release ended on {last}");
    }

    #[test]
    fn every_stage_ends_on_time_at_every_legal_length_and_sample_rate() {
        for sample_rate in [8_000, 44_100, 48_000, 96_000, 192_000, 384_000] {
            for ms in [0.0, 0.01, 1.0, 37.5, 1_000.0] {
                for sustain in [0.5, 0.95] {
                    assert_stages_end_on_time(ms, sample_rate, sustain);
                }
            }
        }
        // The longest stage the model allows. At 48 kHz a level stepped down
        // from 1 toward 0.95 over this long would not move at all: the step
        // is smaller than the gap between 1 and the next float below it.
        assert_stages_end_on_time(MAX_ENVELOPE_MS, 48_000, 0.95);
        assert_stages_end_on_time(MAX_ENVELOPE_MS, 384_000, 0.95);
    }
}
