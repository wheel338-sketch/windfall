//! Fixed storage shared by the family; each patch supplies its own synthesis graph.
use super::core::{self, Controls, Drift, Ladder, MAX_VOICES, Notes, SatSvf};
use super::fx::{Chorus, Phaser, Reverb, StereoDelay};
use super::pack_assets::PackAssets;
use crate::blocks::{
    adsr::Adsr,
    noise::{PinkNoise, Rng},
    oscillator::{Oscillator, Waveform},
};
use crate::{EnvelopeParams, Instrument, ParamSet};
use std::f32::consts::TAU;
use std::sync::Arc;

pub(super) const UNISON: usize = 7;

#[derive(Clone, Copy)]
pub(super) struct Frame {
    pub amp: EnvelopeParams,
    pub filter_env: EnvelopeParams,
    pub cutoff: f32,
    pub env_amount: f32,
    pub key_track: f32,
    pub lfo_rate: f32,
    pub lfo_filter: f32,
    pub gain: f32,
    pub glide: f32,
    pub polyphony: usize,
    pub chorus: f32,
    pub delay: f32,
    pub phaser: f32,
    pub reverb: f32,
}
impl Default for Frame {
    fn default() -> Self {
        Self {
            amp: EnvelopeParams::default(),
            filter_env: EnvelopeParams::default(),
            cutoff: 4000.0,
            env_amount: 0.0,
            key_track: 0.0,
            lfo_rate: 1.0,
            lfo_filter: 0.0,
            gain: 0.5,
            glide: 0.0,
            polyphony: MAX_VOICES,
            chorus: 0.0,
            delay: 0.0,
            phaser: 0.0,
            reverb: 0.0,
        }
    }
}

pub(super) struct Tone {
    increment_cache: [[(f32, f32, f32); 7]; UNISON],
    pan_cache: [Option<(f32, (f32, f32))>; UNISON],
    filter_cache: Option<([f32; 3], (f32, f32))>,
    cutoff_cache: Option<([f32; 3], f32)>,
    drive_cache: Option<(f32, f32, f32)>,
    pub matrix_cache: [super::operators::MatrixCache; UNISON],
    pub matrix_graph: super::operators::MatrixGraph,
    pub matrix_unison: UnisonCache,
    pub oscillators: [[Oscillator; 6]; UNISON],
    pub previous: [[f32; 6]; UNISON],
    pub operators: [Adsr; 6],
    pub levels: [f32; 6],
    pub ladder: [Ladder; 2],
    pub svf: [SatSvf; 2],
    pub highpass: [SatSvf; 2],
    pub drift: Drift,
    pub noise: Rng,
    pub pink: [[PinkNoise; 6]; UNISON],
    pub amp: Adsr,
    pub filter_env: Adsr,
    pub elapsed: u64,
    pub sample_phase: [f32; UNISON],
    pub sample_key: u8,
    pub sequence_step: u64,
    pub sequence_open: bool,
    pub sequence_env: Adsr,
}
impl Default for Tone {
    fn default() -> Self {
        Self {
            matrix_cache: [super::operators::MatrixCache::default(); UNISON],
            matrix_graph: super::operators::MatrixGraph::default(),
            matrix_unison: UnisonCache::default(),
            increment_cache: [[(f32::NAN, f32::NAN, 0.0); 7]; UNISON],
            pan_cache: [None; UNISON],
            filter_cache: None,
            cutoff_cache: None,
            drive_cache: None,
            oscillators: [[Oscillator::default(); 6]; UNISON],
            previous: [[0.0; 6]; UNISON],
            operators: [Adsr::default(); 6],
            levels: [0.0; 6],
            ladder: [Ladder::default(); 2],
            svf: [SatSvf::default(); 2],
            highpass: [SatSvf::default(); 2],
            drift: Drift::default(),
            noise: Rng::new(0x5746),
            pink: [[PinkNoise::new(); 6]; UNISON],
            amp: Adsr::default(),
            filter_env: Adsr::default(),
            elapsed: 0,
            sample_phase: [0.0; UNISON],
            sample_key: 60,
            sequence_step: u64::MAX,
            sequence_open: false,
            sequence_env: Adsr::default(),
        }
    }
}
impl Tone {
    #[inline]
    fn cutoff(&mut self, cutoff: f32, octaves: f32, rate: f32) -> f32 {
        let key = [cutoff, octaves, rate];
        match self.cutoff_cache {
            Some((before, value)) if before == key => value,
            _ => {
                let value = core::swept(cutoff, octaves, rate);
                self.cutoff_cache = Some((key, value));
                value
            }
        }
    }
    #[inline]
    pub fn drive(&mut self, input: f32, amount: f32) -> f32 {
        let amount = amount.clamp(0.0, 1.0);
        if amount <= 0.0 {
            return input;
        }
        let (gain, normalization) = match self.drive_cache {
            Some((before, gain, normalization)) if before == amount => (gain, normalization),
            _ => {
                let gain = 1.0 + 9.0 * amount;
                let normalization = gain.tanh();
                self.drive_cache = Some((amount, gain, normalization));
                (gain, normalization)
            }
        };
        input + amount * ((input * gain).tanh() / normalization - input)
    }
    #[inline]
    pub fn phase_increment(
        &mut self,
        lane: usize,
        oscillator: usize,
        pitch: f32,
        rate: f32,
    ) -> f32 {
        let (before, before_rate, increment) = self.increment_cache[lane][oscillator];
        if before == pitch && before_rate == rate {
            increment
        } else {
            let increment = core::increment(pitch, rate);
            self.increment_cache[lane][oscillator] = (pitch, rate, increment);
            increment
        }
    }
    #[inline]
    pub fn pan(&mut self, lane: usize, pan: f32) -> (f32, f32) {
        match self.pan_cache[lane] {
            Some((before, gains)) if before == pan => gains,
            _ => {
                let gains = core::pan_gains(pan);
                self.pan_cache[lane] = Some((pan, gains));
                gains
            }
        }
    }
    #[inline]
    pub fn filter_coefficients(&mut self, cutoff: f32, resonance: f32, rate: f32) -> (f32, f32) {
        let key = [cutoff, resonance, rate];
        match self.filter_cache {
            Some((before, coeffs)) if before == key => coeffs,
            _ => {
                let coeffs = (
                    core::swept_gain(cutoff, 0.0, rate),
                    core::svf_damping(resonance),
                );
                self.filter_cache = Some((key, coeffs));
                coeffs
            }
        }
    }
}
pub(super) fn envelope(env: &mut Adsr, p: EnvelopeParams, rate: f32) {
    env.configure(p.attack_ms, p.decay_ms, p.sustain, p.release_ms, rate);
}

pub(super) struct Context<'a> {
    pub operator_graph: Option<&'a OperatorGraph>,
    pub pitch: f32,
    pub key: u8,
    pub held: bool,
    pub slot: usize,
    pub lfo: f32,
    pub cutoff: f32,
    pub rate: f32,
    pub bpm: f32,
    pub assets: Option<&'a PackAssets>,
}

#[derive(Clone, Copy)]
pub(super) struct UnisonFrame {
    count: usize,
    detune: f32,
    spread: f32,
    pub offsets: [f32; UNISON],
    pub pans: [(f32, f32); UNISON],
}
#[derive(Default)]
pub(super) struct UnisonCache(Option<UnisonFrame>);
impl UnisonCache {
    #[inline]
    pub fn prepare(&mut self, count: usize, detune: f32, spread: f32) -> UnisonFrame {
        if let Some(frame) = self.0
            && frame.count == count
            && frame.detune == detune
            && frame.spread == spread
        {
            return frame;
        }
        let positions = std::array::from_fn::<_, UNISON, _>(|u| core::unison_position(u, count));
        let frame = UnisonFrame {
            count,
            detune,
            spread,
            offsets: positions.map(|position| position * detune / 100.0),
            pans: positions.map(|position| core::pan_gains(position * spread)),
        };
        self.0 = Some(frame);
        frame
    }
}

pub(super) struct OperatorGraph {
    pub fm: [[f32; 6]; 6],
    pub outputs: [f32; 6],
}

pub(super) trait Patch: ParamSet {
    const NEEDS_ASSETS: bool = false;
    fn frame(&self) -> Frame;
    fn configure(&self, _tone: &mut Tone, _rate: f32) {}
    /// Whether graph/operator setup depends on this smoothed parameter change.
    fn configuration_changed(&self, _previous: &Self) -> bool {
        true
    }
    fn operator_graph(&self) -> Option<OperatorGraph> {
        None
    }
    fn sample(&self, tone: &mut Tone, context: &Context) -> (f32, f32);
}

pub(super) struct Runtime<P: Patch> {
    configuration: Option<P>,
    envelope_configuration: Option<(EnvelopeParams, EnvelopeParams)>,
    operator_graph: Option<OperatorGraph>,
    rate: f32,
    pan_law: windfall_core::PanLaw,
    note_pans: [crate::note_pan::NotePan; MAX_VOICES],
    bpm: f32,
    controls: Controls<P>,
    notes: Notes,
    tones: [Tone; MAX_VOICES],
    lfo_phase: f32,
    chorus: Chorus,
    delay: StereoDelay,
    phaser: Phaser,
    reverb: Reverb,
    tail: usize,
    assets: Option<Arc<PackAssets>>,
    effects_clean: bool,
}
impl<P: Patch> Default for Runtime<P> {
    fn default() -> Self {
        Self {
            configuration: None,
            envelope_configuration: None,
            operator_graph: None,
            rate: 48000.0,
            pan_law: windfall_core::PanLaw::Balance,
            note_pans: [crate::note_pan::NotePan::default(); MAX_VOICES],
            bpm: 120.0,
            controls: Controls::default(),
            notes: Notes::default(),
            tones: std::array::from_fn(|_| Tone::default()),
            lfo_phase: 0.0,
            chorus: Chorus::default(),
            delay: StereoDelay::default(),
            phaser: Phaser::default(),
            reverb: Reverb::default(),
            tail: 0,
            assets: None,
            effects_clean: true,
        }
    }
}
impl<P: Patch> Runtime<P> {
    fn pan_latest(&mut self, pan: f32) {
        if let Some(index) = self.notes.latest() {
            self.note_pans[index] = crate::note_pan::NotePan::new(pan, self.pan_law);
        }
    }
}
impl<P: Patch> Instrument for Runtime<P> {
    type Params = P;
    fn prepare(&mut self, rate: f32, _max_block: usize) {
        let _ = super::operators::prepare_vector_features();
        self.rate = core::rate(rate);
        if P::NEEDS_ASSETS && self.assets.is_none() {
            self.assets = Some(PackAssets::shared());
        }
        self.controls.prepare(self.rate);
        self.chorus.prepare(self.rate);
        self.delay.prepare(self.rate);
        self.phaser.prepare(self.rate);
        self.reverb.prepare(self.rate);
        self.reset();
    }
    fn reset(&mut self) {
        self.configuration = None;
        self.envelope_configuration = None;
        self.operator_graph = None;
        self.controls.reset();
        self.notes.reset();
        self.note_pans
            .fill(crate::note_pan::NotePan::new(0.0, self.pan_law));
        self.lfo_phase = 0.0;
        self.tail = 0;
        self.effects_clean = true;
        for tone in &mut self.tones {
            *tone = Tone::default();
        }
        self.chorus.reset();
        self.delay.reset();
        self.phaser.reset();
        self.reverb.reset();
    }
    fn set_params(&mut self, params: &P) {
        self.controls.set(params);
    }
    fn set_tempo(&mut self, bpm: f32) {
        self.bpm = core::tempo(bpm);
    }
    fn note_on(&mut self, key: u8, velocity: f32) {
        let frame = self.controls.target().frame();
        self.notes
            .note_on(key, velocity, frame.polyphony, frame.glide, self.rate);
        if velocity.is_finite() && velocity > 0.0 {
            self.pan_latest(0.0);
        }
    }
    fn set_note_pan_law(&mut self, law: windfall_core::PanLaw) {
        self.pan_law = law;
        for pan in &mut self.note_pans {
            pan.retarget(law, self.rate);
        }
    }
    fn note_on_expression(
        &mut self,
        key: u8,
        velocity: f32,
        pan: f32,
        _expression: crate::NoteExpression,
    ) {
        self.note_on(key, velocity);
        if velocity.is_finite() && velocity > 0.0 {
            self.pan_latest(pan);
        }
    }
    fn note_off(&mut self, key: u8) {
        self.notes.note_off(key);
    }
    fn all_notes_off(&mut self) {
        self.notes.stop(self.rate);
    }
    fn active_voices(&self) -> usize {
        self.notes.active()
    }
    fn tail_samples(&self) -> usize {
        let f = self.controls.target().frame();
        let mut tail = 0;
        if f.chorus > 0.0 {
            tail += self.chorus.tail_samples();
        }
        if f.delay > 0.0 {
            tail += self.delay.tail_samples(0.35);
        }
        if f.reverb > 0.0 {
            tail += self.reverb.tail_samples(0.5);
        }
        // Phaser has recursive allpass state; clear it at this bounded tail.
        if f.phaser > 0.0 {
            tail += self.rate as usize * 2;
        }
        tail
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        // If the current configured graph already equals the target graph,
        // smoothing unrelated controls cannot change it anywhere in this block.
        // Graph/envelope edits keep the per-sample comparison and preparation.
        let configuration_may_change = self
            .configuration
            .as_ref()
            .is_none_or(|previous| self.controls.target().configuration_changed(previous));
        let target_frame = self.controls.target().frame();
        let envelopes_may_change = self.envelope_configuration.is_none_or(|(amp, filter)| {
            amp != target_frame.amp || filter != target_frame.filter_env
        });
        // Tail bounds depend only on the target patch and prepared line lengths.
        // Avoid repeating decay logarithms for every rendered sample.
        let held_tail = self.tail_samples();
        for (left, right) in left.iter_mut().zip(right) {
            let (p, changed) = self.controls.tick();
            let configure = changed
                && configuration_may_change
                && self
                    .configuration
                    .as_ref()
                    .is_none_or(|previous| p.configuration_changed(previous));
            if configure {
                self.operator_graph = p.operator_graph();
                self.configuration = Some(*p);
            }
            let f = p.frame();
            let amp_changed = envelopes_may_change
                && self
                    .envelope_configuration
                    .is_none_or(|(amp, _)| amp != f.amp);
            let filter_changed = envelopes_may_change
                && self
                    .envelope_configuration
                    .is_none_or(|(_, filter)| filter != f.filter_env);
            if amp_changed || filter_changed {
                self.envelope_configuration = Some((f.amp, f.filter_env));
            }
            let lfo = (TAU * self.lfo_phase).sin();
            core::advance(&mut self.lfo_phase, f.lfo_rate / self.rate);
            let mut sum = (0.0, 0.0);
            let sounding = self.notes.active() > 0;
            for (slot, (note, tone)) in self.notes.slots.iter_mut().zip(&mut self.tones).enumerate()
            {
                if !note.active {
                    continue;
                }
                if note.start {
                    *tone = Tone::default();
                    tone.sample_key = note.key;
                    tone.noise = Rng::new(0x5746 + slot as u32);
                    for (u, oscillators) in tone.oscillators.iter_mut().enumerate() {
                        for (op, oscillator) in oscillators.iter_mut().enumerate() {
                            *oscillator =
                                Oscillator::new((u as f32 * 0.137 + op as f32 * 0.071) % 1.0);
                        }
                    }
                }
                if amp_changed || note.start {
                    envelope(&mut tone.amp, f.amp, self.rate);
                }
                if filter_changed || note.start {
                    envelope(&mut tone.filter_env, f.filter_env, self.rate);
                }
                if configure || note.start {
                    p.configure(tone, self.rate);
                }
                if note.start {
                    tone.amp.gate_on();
                    tone.filter_env.gate_on();
                    for op in &mut tone.operators {
                        op.gate_on();
                    }
                    note.start = false;
                }
                if note.release {
                    tone.amp.gate_off();
                    tone.filter_env.gate_off();
                    for op in &mut tone.operators {
                        op.gate_off();
                    }
                    note.release = false;
                }
                let amp = tone.amp.tick();
                let filter = tone.filter_env.tick();
                for (level, op) in tone.levels.iter_mut().zip(&mut tone.operators) {
                    *level = op.tick();
                }
                let pitch = note.glide();
                let cutoff = tone.cutoff(
                    f.cutoff,
                    f.env_amount * filter
                        + f.key_track * (pitch - 60.0) / 12.0
                        + lfo * f.lfo_filter,
                    self.rate * 2.0,
                );
                let ctx = Context {
                    operator_graph: self.operator_graph.as_ref(),
                    pitch,
                    key: note.key,
                    held: note.held,
                    slot,
                    lfo,
                    cutoff,
                    rate: self.rate * 2.0,
                    bpm: self.bpm,
                    assets: self.assets.as_deref(),
                };
                // Two substeps reduce folding from the nonlinear synthesis/filter stages.
                let a = p.sample(tone, &ctx);
                let b = p.sample(tone, &ctx);
                tone.elapsed = tone.elapsed.wrapping_add(1);
                let gain = amp * note.velocity * f.gain * 0.25;
                let pan = self.note_pans[slot].tick();
                let out = note.finish(
                    (a.0 + b.0) * 0.5 * gain * pan.0,
                    (a.1 + b.1) * 0.5 * gain * pan.1,
                    !tone.amp.is_idle(),
                );
                sum.0 += out.0;
                sum.1 += out.1;
            }
            if sounding {
                self.tail = held_tail;
                self.effects_clean = false;
            }
            if !sounding && self.tail == 0 {
                if !self.effects_clean {
                    self.chorus.reset();
                    self.delay.reset();
                    self.phaser.reset();
                    self.reverb.reset();
                    self.effects_clean = true;
                }
                *left = 0.0;
                *right = 0.0;
                continue;
            }
            self.tail = self.tail.saturating_sub(usize::from(!sounding));
            let out = self.chorus.tick(sum.0, sum.1, 0.37, 0.8, f.chorus);
            let out = self.phaser.tick(out.0, out.1, 0.2, 0.6, f.phaser);
            let out = self.delay.tick(
                out.0,
                out.1,
                core::division_seconds(0.5, self.bpm) * self.rate,
                0.35,
                f.delay,
                true,
            );
            let out = self.reverb.tick(out.0, out.1, 0.5, f.reverb);
            *left = core::guard(out.0);
            *right = core::guard(out.1);
            if !sounding && self.tail == 0 {
                self.chorus.reset();
                self.delay.reset();
                self.phaser.reset();
                self.reverb.reset();
                self.effects_clean = true;
            }
        }
    }
}

pub(super) fn oscillator(
    tone: &mut Tone,
    u: usize,
    op: usize,
    pitch: f32,
    rate: f32,
    wave: Waveform,
    width: f32,
) -> f32 {
    if wave == Waveform::WhiteNoise {
        return tone.noise.bipolar();
    }
    if wave == Waveform::PinkNoise {
        return tone.pink[u][op].tick(&mut tone.noise);
    }
    let increment = tone.phase_increment(u, op, pitch, rate);
    tone.oscillators[u][op].tick(wave, increment, width.clamp(0.05, 0.95))
}

// Public types have distinct parameter contracts and monomorphized synthesis graphs.
macro_rules! instrument {
    ($name:ident, $params:ident) => {
        #[derive(Default)]
        pub struct $name(super::runtime::Runtime<$params>);
        impl crate::Instrument for $name {
            type Params = $params;
            fn prepare(&mut self, rate: f32, block: usize) {
                self.0.prepare(rate, block);
            }
            fn reset(&mut self) {
                self.0.reset();
            }
            fn set_params(&mut self, p: &$params) {
                self.0.set_params(p);
            }
            fn set_tempo(&mut self, bpm: f32) {
                self.0.set_tempo(bpm);
            }
            fn note_on(&mut self, key: u8, velocity: f32) {
                self.0.note_on(key, velocity);
            }
            fn set_note_pan_law(&mut self, law: windfall_core::PanLaw) {
                self.0.set_note_pan_law(law);
            }
            fn note_on_expression(
                &mut self,
                key: u8,
                velocity: f32,
                pan: f32,
                expression: crate::NoteExpression,
            ) {
                self.0.note_on_expression(key, velocity, pan, expression);
            }
            fn note_off(&mut self, key: u8) {
                self.0.note_off(key);
            }
            fn all_notes_off(&mut self) {
                self.0.all_notes_off();
            }
            fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
                self.0.process(l, r);
            }
            fn active_voices(&self) -> usize {
                self.0.active_voices()
            }
            fn tail_samples(&self) -> usize {
                self.0.tail_samples()
            }
        }
    };
}
pub(super) use instrument;

#[cfg(test)]
mod unison_tests {
    use super::*;
    #[test]
    fn prepared_matrix_unison_matches_original_positions_and_pan_bits() {
        let mut cache = UnisonCache::default();
        for count in 1..=UNISON {
            for detune in [-50.0, 0.0, 23.7] {
                for spread in [0.0, 0.3, 1.0] {
                    for _ in 0..2 {
                        let frame = cache.prepare(count, detune, spread);
                        for u in 0..count {
                            let position = core::unison_position(u, count);
                            assert_eq!(
                                frame.offsets[u].to_bits(),
                                (position * detune / 100.0).to_bits()
                            );
                            let expected = core::pan_gains(position * spread);
                            assert_eq!(
                                (frame.pans[u].0.to_bits(), frame.pans[u].1.to_bits()),
                                (expected.0.to_bits(), expected.1.to_bits())
                            );
                        }
                    }
                }
            }
        }
    }
}
