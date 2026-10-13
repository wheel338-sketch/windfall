//! The polyphonic engine the additive instruments share.
//!
//! An additive instrument is a spectrum and a way of playing it. The
//! spectrum is the instrument's own business; everything else -- taking
//! notes, handing them voices, shaping each one with an envelope and
//! putting the result out in stereo -- is the same for all of them and
//! happens here.
//!
//! The engine runs on a fixed grid of [`CONTROL_PERIOD`] samples. A
//! parameter change sets a target, and the settings every voice reads glide
//! to it over [`GLIDE_MS`], one step per sample, so that a control can be
//! moved while notes sound without a click. Because the grid is counted in
//! samples rather than in `process` calls, the output does not depend on
//! how the host divides the audio into blocks.

use windfall_core::pan_gains;

use crate::blocks::CONTROL_PERIOD;
use crate::blocks::adsr::Adsr;
use crate::blocks::math::{clean, flush, key_to_hz, ms_to_samples};
use crate::blocks::smooth::LinearRamp;
use crate::synth::EnvelopeParams;

use super::voicing::VoicingParams;

/// Most partials any additive instrument sounds at once.
pub const MAX_PARTIALS: usize = 64;

/// Most notes that can sound at full level at once.
pub const MAX_POLYPHONY: usize = 16;

/// Extra voices for notes fading out after being stolen, so that taking a
/// voice never has to cut one off.
const SPARE_VOICES: usize = 4;
const VOICES: usize = MAX_POLYPHONY + SPARE_VOICES;

/// Time a changed control takes to reach the sound.
pub const GLIDE_MS: f32 = 5.0;

/// Length of the fade that ends a stolen or stopped voice.
const CHOKE_MS: f32 = 4.0;

/// Shortest attack and release. Anything faster is a click.
const MIN_SEGMENT_MS: f32 = 0.5;

/// Highest phase step a voice's fundamental may take per sample. Half a
/// cycle per sample is the Nyquist frequency, and a partial there carries
/// no pitch worth hearing.
const MAX_INCREMENT: f32 = 0.49;

/// The rate an engine assumes before it is prepared.
const DEFAULT_RATE: f32 = 48_000.0;

/// Levels beyond this are not audio, and are treated as damage.
const AUDIO_LIMIT: f32 = 1.0e3;

/// Coefficients of a seventh-order odd polynomial for `sin` on the first
/// quarter cycle, fitted for the smallest worst-case error.
pub(super) const SINE_C3: f32 = -0.166_656_8;
pub(super) const SINE_C5: f32 = 0.008_312_37;
pub(super) const SINE_C7: f32 = -0.000_184_92;

/// A sine of `turns` cycles, where `turns` runs from 0 to 1.
///
/// [`sine`](crate::blocks::oscillator::sine) is the same curve from the
/// standard library's `sin`, and is what this is measured against. A full
/// bank of partials evaluates up to a thousand sines per output sample,
/// which is more than `sin` can keep up with, so the engine folds the cycle
/// onto its first quarter and evaluates a polynomial there instead. The
/// worst-case error is 1.1e-6, or about -119 dB.
#[inline]
pub(super) fn sine_turns(turns: f32) -> f32 {
    let (sign, quarter) = if turns < 0.5 {
        (1.0, turns)
    } else {
        (-1.0, turns - 0.5)
    };
    // The second eighth of the cycle mirrors the first.
    let quarter = if quarter > 0.25 {
        0.5 - quarter
    } else {
        quarter
    };
    let y = std::f32::consts::TAU * quarter;
    let y2 = y * y;
    sign * y * (1.0 + y2 * (SINE_C3 + y2 * (SINE_C5 + y2 * SINE_C7)))
}

/// Forces a sample to be audio: finite, and not so large that it could only
/// come from damage.
#[inline]
pub(super) fn audio(value: f32) -> f32 {
    flush(clean(value, -AUDIO_LIMIT, AUDIO_LIMIT, 0.0))
}

/// What every voice does with a note once it has one: the shared controls,
/// read straight off [`VoicingParams`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Common {
    pub envelope: EnvelopeParams,
    pub polyphony: usize,
    pub velocity: f32,
    pub gain: f32,
    pub pan: f32,
}

impl Common {
    pub(super) fn new(params: &VoicingParams) -> Self {
        Self {
            envelope: params.envelope,
            polyphony: usize::from(params.polyphony).clamp(1, MAX_POLYPHONY),
            velocity: params.velocity,
            gain: params.gain,
            pan: params.pan,
        }
    }
}

impl Default for Common {
    fn default() -> Self {
        Self::new(&VoicingParams::default())
    }
}

/// What one voice of an additive instrument sounds.
///
/// A `Tone` owns the waveform and nothing else. Its `Target` is what the
/// instrument asks for, worked out from the parameters; its `Setup` is what
/// a voice reads while it renders, which glides toward the target. The
/// engine keeps one of each and hands every voice the same `Setup`, so the
/// per-partial arithmetic that follows from the parameters is done once per
/// control period however many notes are sounding.
pub(super) trait Tone: Copy {
    /// What the instrument asks for.
    type Target: Copy + Default;

    /// What a voice reads while it renders one control period.
    type Setup: Copy;

    /// A voice with nothing sounding.
    fn silent() -> Self;

    /// The settings that play `target` at once, with nothing gliding.
    fn settled(target: &Self::Target) -> Self::Setup;

    /// Puts the voice at the start of a note.
    fn restart(&mut self, setup: &Self::Setup);

    /// Moves `setup` on by one control period.
    ///
    /// It must arrive at `target` after `remaining` periods, and must stand
    /// exactly on it once `remaining` is zero. `setup` holds the values the
    /// period starts from together with their change per sample, so this is
    /// called once per period however many voices read it.
    fn glide(setup: &mut Self::Setup, target: &Self::Target, remaining: u32);

    /// Adds `out.len()` samples to `out`.
    ///
    /// `increment` is the cycles the note's fundamental advances per
    /// sample. `offset` is how many samples of the control period have
    /// already been rendered, which is what lets a host split a period
    /// across two blocks and still get the same output.
    fn render(&mut self, setup: &Self::Setup, increment: f32, offset: usize, out: &mut [f32]);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    /// The key is down.
    Held,
    /// The key was let go and the release is playing.
    Released,
    /// Being faded out quickly to make room or to stop.
    Choked,
}

#[derive(Clone, Copy)]
struct Voice<T: Tone> {
    stage: Stage,
    key: u8,
    /// Cycles the fundamental advances per sample.
    increment: f32,
    /// Level the note is played at, after the velocity control.
    level: f32,
    /// Counts up with every note, so the oldest voice can be found.
    order: u64,
    tone: T,
    amp: Adsr,
    choke: LinearRamp,
}

impl<T: Tone> Voice<T> {
    fn silent() -> Self {
        Self {
            stage: Stage::Idle,
            key: 69,
            increment: 0.0,
            level: 0.0,
            order: 0,
            tone: T::silent(),
            amp: Adsr::default(),
            choke: LinearRamp::new(1.0),
        }
    }

    fn is_sounding(&self) -> bool {
        matches!(self.stage, Stage::Held | Stage::Released)
    }

    /// How loud the voice is right now, for choosing which fading voice to
    /// take over.
    fn loudness(&self) -> f32 {
        self.amp.value() * self.choke.value()
    }

    fn configure(&mut self, common: &Common, sample_rate: f32) {
        let envelope = &common.envelope;
        self.amp.configure(
            envelope.attack_ms.max(MIN_SEGMENT_MS),
            envelope.decay_ms,
            envelope.sustain,
            envelope.release_ms.max(MIN_SEGMENT_MS),
            sample_rate,
        );
    }

    fn render(&mut self, setup: &T::Setup, offset: usize, out: &mut [f32]) {
        let mut tone = [0.0_f32; CONTROL_PERIOD];
        let frames = out.len();
        self.tone
            .render(setup, self.increment, offset, &mut tone[..frames]);
        for (sample, voice) in out.iter_mut().zip(&tone[..frames]) {
            let gain = self.amp.tick() * self.level * self.choke.tick();
            *sample += voice * gain;
        }
        let faded = self.stage == Stage::Choked && self.choke.is_settled();
        if faded || self.amp.is_idle() {
            self.stage = Stage::Idle;
        }
    }
}

/// A polyphonic additive instrument: a bank of preallocated voices, all
/// playing the same gliding settings at their own pitches.
///
/// The instruments on top of this own their parameters and turn them into a
/// [`Tone::Target`]. Everything in here is fixed in size and allocated when
/// the engine is built.
pub(super) struct AdditiveEngine<T: Tone> {
    sample_rate: f32,
    common: Common,
    target: T::Target,
    setup: T::Setup,
    /// Control periods left before `setup` stands on `target`.
    remaining: u32,
    /// Control periods a change takes, which `prepare` works out.
    glide_periods: u32,
    /// Samples the output gain takes to reach a change, which is the same
    /// time in samples rather than periods.
    glide_samples: u32,
    voices: [Voice<T>; VOICES],
    gain_left: LinearRamp,
    gain_right: LinearRamp,
    notes_started: u64,
    /// Samples left of the control period being rendered.
    until_control: usize,
    /// True until the first block after `prepare` or `reset`, during which
    /// settings apply at once instead of gliding.
    fresh: bool,
}

impl<T: Tone> AdditiveEngine<T> {
    pub(super) fn new() -> Self {
        let target = T::Target::default();
        let common = Common::default();
        let (left, right) = output_gains(&common);
        // An engine that is played without being prepared still glides,
        // rather than stepping, at the rate it assumes.
        let glide_samples = ms_to_samples(GLIDE_MS, DEFAULT_RATE);
        Self {
            sample_rate: DEFAULT_RATE,
            common,
            target,
            setup: T::settled(&target),
            remaining: 0,
            glide_periods: (glide_samples / CONTROL_PERIOD as u32).max(1),
            glide_samples,
            voices: [Voice::silent(); VOICES],
            gain_left: LinearRamp::new(left),
            gain_right: LinearRamp::new(right),
            notes_started: 0,
            until_control: 0,
            fresh: true,
        }
    }

    /// See [`Instrument::prepare`](crate::Instrument::prepare).
    pub(super) fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = clean(sample_rate, 1.0, 768_000.0, DEFAULT_RATE);
        self.glide_samples = ms_to_samples(GLIDE_MS, self.sample_rate);
        self.glide_periods = (self.glide_samples / CONTROL_PERIOD as u32).max(1);
        self.reset();
    }

    /// See [`Instrument::reset`](crate::Instrument::reset).
    pub(super) fn reset(&mut self) {
        self.voices = [Voice::silent(); VOICES];
        self.setup = T::settled(&self.target);
        self.remaining = 0;
        let (left, right) = output_gains(&self.common);
        self.gain_left.snap(left);
        self.gain_right.snap(right);
        self.notes_started = 0;
        self.until_control = 0;
        self.fresh = true;
    }

    /// Takes the spectrum and the shared controls a note should now use.
    /// Sounding notes glide to them, unless no block has been rendered
    /// since `prepare` or `reset`, in which case they apply at once.
    pub(super) fn set_target(&mut self, target: &T::Target, common: &Common) {
        let previous = std::mem::replace(&mut self.common, *common);
        self.target = *target;
        if self.fresh {
            self.setup = T::settled(&self.target);
            self.remaining = 0;
            let (left, right) = output_gains(&self.common);
            self.gain_left.snap(left);
            self.gain_right.snap(right);
        } else {
            self.remaining = self.glide_periods;
            let (left, right) = output_gains(&self.common);
            self.gain_left.set_target(left, self.glide_samples);
            self.gain_right.set_target(right, self.glide_samples);
        }
        if common.envelope != previous.envelope {
            for voice in &mut self.voices {
                if voice.stage != Stage::Idle {
                    voice.configure(&self.common, self.sample_rate);
                }
            }
        }
    }

    /// The rate the engine was prepared at, for an instrument that has a
    /// control measured in time.
    pub(super) fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// What the instrument last asked for, for a test to measure against.
    #[cfg(test)]
    pub(super) fn target(&self) -> T::Target {
        self.target
    }

    /// See [`Instrument::note_on`](crate::Instrument::note_on).
    pub(super) fn note_on(&mut self, key: u8, velocity: f32) {
        let key = key.min(127);
        if !velocity.is_finite() || velocity <= 0.0 {
            self.note_off(key);
            return;
        }
        let velocity = velocity.min(1.0);
        while self.sounding_voices() >= self.common.polyphony {
            match self.oldest_sounding() {
                Some(index) => self.choke(index),
                None => break,
            }
        }
        let index = self.free_voice();
        let order = self.notes_started;
        self.notes_started = self.notes_started.wrapping_add(1);
        let increment = (key_to_hz(f32::from(key)) / self.sample_rate).clamp(0.0, MAX_INCREMENT);
        let level = 1.0 - self.common.velocity * (1.0 - velocity);
        let voice = &mut self.voices[index];
        if voice.stage == Stage::Idle {
            *voice = Voice::silent();
        } else {
            // Taking over a voice that is still fading: carry on from the
            // level it has reached, so there is no jump.
            voice.amp.scale_level(voice.choke.value());
        }
        voice.choke.snap(1.0);
        voice.stage = Stage::Held;
        voice.key = key;
        voice.increment = increment;
        voice.level = level;
        voice.order = order;
        voice.tone.restart(&self.setup);
        voice.configure(&self.common, self.sample_rate);
        voice.amp.gate_on();
    }

    /// See [`Instrument::note_off`](crate::Instrument::note_off).
    pub(super) fn note_off(&mut self, key: u8) {
        let key = key.min(127);
        for voice in &mut self.voices {
            if voice.stage == Stage::Held && voice.key == key {
                voice.stage = Stage::Released;
                voice.amp.gate_off();
            }
        }
    }

    /// See [`Instrument::all_notes_off`](crate::Instrument::all_notes_off).
    pub(super) fn all_notes_off(&mut self) {
        for index in 0..VOICES {
            if self.voices[index].stage != Stage::Idle {
                self.choke(index);
            }
        }
    }

    /// See [`Instrument::active_voices`](crate::Instrument::active_voices).
    pub(super) fn active_voices(&self) -> usize {
        self.voices
            .iter()
            .filter(|voice| voice.stage != Stage::Idle)
            .count()
    }

    /// See [`Instrument::process`](crate::Instrument::process).
    pub(super) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.fresh = false;
        let frames = left.len().min(right.len());
        let mut start = 0;
        while start < frames {
            if self.until_control == 0 {
                self.control();
                self.until_control = CONTROL_PERIOD;
            }
            let end = start + (frames - start).min(self.until_control);
            self.render(&mut left[start..end], &mut right[start..end]);
            self.until_control -= end - start;
            start = end;
        }
        // A host may pass buffers of different lengths. Whatever is past
        // the shorter of the two is still replaced, with silence.
        for sample in left
            .iter_mut()
            .skip(frames)
            .chain(right.iter_mut().skip(frames))
        {
            *sample = 0.0;
        }
    }

    fn control(&mut self) {
        T::glide(&mut self.setup, &self.target, self.remaining);
        self.remaining = self.remaining.saturating_sub(1);
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len();
        let offset = CONTROL_PERIOD - self.until_control;
        let mut bus = [0.0_f32; CONTROL_PERIOD];
        for voice in &mut self.voices {
            if voice.stage != Stage::Idle {
                voice.render(&self.setup, offset, &mut bus[..frames]);
            }
        }
        for index in 0..frames {
            let voice = audio(bus[index]);
            left[index] = audio(voice * self.gain_left.tick());
            right[index] = audio(voice * self.gain_right.tick());
        }
    }

    /// Starts a quick fade that ends with the voice free.
    fn choke(&mut self, index: usize) {
        let samples = ms_to_samples(CHOKE_MS, self.sample_rate);
        let voice = &mut self.voices[index];
        voice.stage = Stage::Choked;
        voice.choke.set_target(0.0, samples);
    }

    fn sounding_voices(&self) -> usize {
        self.voices
            .iter()
            .filter(|voice| voice.is_sounding())
            .count()
    }

    /// The voice a new note should take: a free one, or failing that the
    /// quietest of the ones fading out.
    fn free_voice(&self) -> usize {
        if let Some(index) = self
            .voices
            .iter()
            .position(|voice| voice.stage == Stage::Idle)
        {
            return index;
        }
        let mut quietest = 0;
        let mut level = f32::INFINITY;
        for (index, voice) in self.voices.iter().enumerate() {
            if voice.stage == Stage::Choked && voice.loudness() < level {
                quietest = index;
                level = voice.loudness();
            }
        }
        quietest
    }

    /// The sounding voice that started first, preferring ones whose key has
    /// been let go.
    fn oldest_sounding(&self) -> Option<usize> {
        let oldest = |stage: Stage| {
            self.voices
                .iter()
                .enumerate()
                .filter(|(_, voice)| voice.stage == stage)
                .min_by_key(|(_, voice)| voice.order)
                .map(|(index, _)| index)
        };
        oldest(Stage::Released).or_else(|| oldest(Stage::Held))
    }
}

fn output_gains(common: &Common) -> (f32, f32) {
    let (left, right) = pan_gains(common.pan);
    (common.gain * left, common.gain * right)
}

/// Implements [`Instrument`](crate::Instrument) for an instrument that is a
/// parameter struct, an [`AdditiveEngine`] and a way of filling the engine
/// from the parameters.
///
/// All six instruments pass their notes straight to the engine; what makes
/// each one itself is its `apply`, which is where its spectrum is worked
/// out.
macro_rules! engine_instrument {
    ($instrument:ident, $params:ident) => {
        impl Default for $instrument {
            fn default() -> Self {
                let mut instrument = Self {
                    params: $params::default(),
                    engine: super::engine::AdditiveEngine::new(),
                };
                instrument.apply();
                instrument
            }
        }

        impl $crate::Instrument for $instrument {
            type Params = $params;

            fn prepare(&mut self, sample_rate: f32, max_block: usize) {
                self.engine.prepare(sample_rate, max_block);
                self.apply();
            }

            fn reset(&mut self) {
                self.engine.reset();
                self.apply();
            }

            fn set_params(&mut self, params: &$params) {
                self.params = $crate::ParamSet::sanitized(params);
                self.apply();
            }

            fn note_on(&mut self, key: u8, velocity: f32) {
                self.engine.note_on(key, velocity);
            }

            fn note_off(&mut self, key: u8) {
                self.engine.note_off(key);
            }

            fn all_notes_off(&mut self) {
                self.engine.all_notes_off();
            }

            fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
                self.engine.process(left, right);
            }

            fn active_voices(&self) -> usize {
                self.engine.active_voices()
            }
        }
    };
}

pub(super) use engine_instrument;
