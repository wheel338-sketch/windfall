use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::blocks::adsr::Adsr;
use crate::blocks::envelope::Ballistics;
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};

use super::{Controls, audio, peak, rate};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct EnvelopeFollowerParams {
    pub attack_ms: f32,
    pub release_ms: f32,
    pub input_gain: f32,
    pub base: f32,
    pub amount: f32,
    pub duck: f32,
    pub lfo_depth: f32,
    pub rate_hz: f32,
}

impl Default for EnvelopeFollowerParams {
    fn default() -> Self {
        Self {
            attack_ms: 10.0,
            release_ms: 100.0,
            input_gain: 1.0,
            base: 0.0,
            amount: 1.0,
            duck: 0.0,
            lfo_depth: 0.0,
            rate_hz: 1.0,
        }
    }
}

param_set!(EnvelopeFollowerParams, "Envelope Follower", {
    float [attack_ms] "attackMs" "Attack" { Milliseconds, Linear, 0.0, 2000.0, 10.0 }
    float [release_ms] "releaseMs" "Release" { Milliseconds, Linear, 0.0, 10000.0, 100.0 }
    float [input_gain] "inputGain" "Detector Gain" { Gain, Linear, 0.0, 16.0, 1.0 }
    float [base] "base" "Base" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [amount] "amount" "Amount" { Gain, Linear, -4.0, 4.0, 1.0 }
    float [duck] "duck" "Ducking" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [lfo_depth] "lfoDepth" "LFO Depth" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [rate_hz] "rateHz" "LFO Rate" { Hertz, Logarithmic, 0.01, 20.0, 1.0 }
});

pub struct EnvelopeFollower {
    params: EnvelopeFollowerParams,
    controls: Controls<6>,
    follower: Ballistics,
    sample_rate: f32,
    phase: f64,
    last_lfo: f32,
    sync_rate: Option<f32>,
}

impl Default for EnvelopeFollower {
    fn default() -> Self {
        let mut follower = Ballistics::new(0.0);
        follower.set_times(10.0, 100.0, 48_000.0);
        Self {
            params: EnvelopeFollowerParams::default(),
            controls: Controls::new([1.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
            follower,
            sample_rate: 48_000.0,
            phase: 0.0,
            last_lfo: 0.5,
            sync_rate: None,
        }
    }
}

impl EnvelopeFollower {
    fn configure(&mut self) {
        self.follower.set_times(
            self.params.attack_ms,
            self.params.release_ms,
            self.sample_rate,
        );
    }

    fn set_controls(&mut self) {
        let p = self.params;
        self.controls.set([
            p.input_gain,
            p.base,
            p.amount,
            p.duck,
            p.lfo_depth,
            self.sync_rate.unwrap_or(p.rate_hz),
        ]);
    }

    /// Musical rates can exceed the free-running knob's 20 Hz maximum.
    /// They use the same five-millisecond glide and phase as the combined output.
    pub fn set_sync_rate(&mut self, hz: Option<f32>) {
        self.sync_rate = hz.map(|hz| crate::blocks::math::clean(hz, 0.001, 160.0, 1.0));
        self.set_controls();
    }

    pub fn lfo_value(&self) -> f32 {
        self.last_lfo
    }

    fn tick(&mut self, left: f32, right: f32) -> [f32; 2] {
        self.tick_detector(left, right, false)
    }

    fn tick_detector(&mut self, left: f32, right: f32, rms: bool) -> [f32; 2] {
        let [input_gain, base, amount, duck, lfo_depth, hz] = self.controls.tick();
        let detector = if rms {
            let l = audio(left) * input_gain;
            let r = audio(right) * input_gain;
            ((l * l + r * r) * 0.5).min(1.0)
        } else {
            (peak(left, right) * input_gain).min(1.0)
        };
        let level = self.follower.tick(detector);
        let level = if rms { level.max(0.0).sqrt() } else { level };
        let wave = (0.5 + 0.5 * (std::f64::consts::TAU * self.phase).sin()) as f32;
        self.last_lfo = wave;
        self.phase = (self.phase + f64::from(hz / self.sample_rate)).fract();
        let control = (base + amount * (level + lfo_depth * wave)).clamp(0.0, 1.0);
        [control, 1.0 - duck * control]
    }

    pub fn process_control<const N: usize>(
        &mut self,
        left: &[f32; N],
        right: &[f32; N],
        output: &mut [f32; N],
    ) {
        for ((l, r), value) in left.iter().zip(right).zip(output) {
            *value = self.tick(*l, *r)[0];
        }
    }

    /// Ballistics on mean stereo power, followed by a square root.
    pub fn process_control_rms<const N: usize>(
        &mut self,
        left: &[f32; N],
        right: &[f32; N],
        output: &mut [f32; N],
    ) {
        for ((l, r), value) in left.iter().zip(right).zip(output) {
            *value = self.tick_detector(*l, *r, true)[0];
        }
    }

    pub fn process_with_control<const N: usize>(
        &mut self,
        left: &mut [f32; N],
        right: &mut [f32; N],
        output: &mut [f32; N],
    ) {
        for ((l, r), value) in left.iter_mut().zip(right).zip(output) {
            let [control, gain] = self.tick(*l, *r);
            *value = control;
            *l = audio(audio(*l) * gain);
            *r = audio(audio(*r) * gain);
        }
    }
}

impl Effect for EnvelopeFollower {
    type Params = EnvelopeFollowerParams;

    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        self.controls.prepare(self.sample_rate);
        self.configure();
        self.reset();
    }

    fn reset(&mut self) {
        self.controls.reset();
        self.follower.snap(0.0);
        self.phase = 0.0;
        self.last_lfo = 0.5;
    }

    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        self.params = p;
        self.configure();
        self.set_controls();
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let gain = self.tick(*l, *r)[1];
            *l = audio(audio(*l) * gain);
            *r = audio(audio(*r) * gain);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct NoteEnvelopeParams {
    /// Rising edge attacks, falling edge releases; hold true to sustain.
    pub trigger: bool,
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
    pub depth: f32,
    pub gain: f32,
}

impl Default for NoteEnvelopeParams {
    fn default() -> Self {
        Self {
            trigger: false,
            attack_ms: 10.0,
            decay_ms: 100.0,
            sustain: 0.7,
            release_ms: 200.0,
            depth: 1.0,
            gain: 1.0,
        }
    }
}

param_set!(NoteEnvelopeParams, "Note Envelope", {
    toggle [trigger] "trigger" "Trigger / Gate" { false }
    float [attack_ms] "attackMs" "Attack" { Milliseconds, Linear, 0.0, 10000.0, 10.0 }
    float [decay_ms] "decayMs" "Decay" { Milliseconds, Linear, 0.0, 10000.0, 100.0 }
    float [sustain] "sustain" "Sustain" { Fraction, Linear, 0.0, 1.0, 0.7 }
    float [release_ms] "releaseMs" "Release" { Milliseconds, Linear, 0.0, 10000.0, 200.0 }
    float [depth] "depth" "Depth" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [gain] "gain" "Gain" { Gain, Linear, 0.0, 2.0, 1.0 }
});

pub struct NoteEnvelope {
    params: NoteEnvelopeParams,
    controls: Controls<2>,
    envelope: Adsr,
    sample_rate: f32,
}

impl Default for NoteEnvelope {
    fn default() -> Self {
        let mut result = Self {
            params: NoteEnvelopeParams::default(),
            controls: Controls::new([1.0, 1.0]),
            envelope: Adsr::default(),
            sample_rate: 48_000.0,
        };
        result.configure();
        result
    }
}

impl NoteEnvelope {
    fn configure(&mut self) {
        let p = self.params;
        self.envelope.configure(
            p.attack_ms,
            p.decay_ms,
            p.sustain,
            p.release_ms,
            self.sample_rate,
        );
    }

    fn tick(&mut self) -> [f32; 2] {
        let [depth, gain] = self.controls.tick();
        let value = self.envelope.tick();
        [value, gain * (1.0 - depth + depth * value)]
    }

    /// Explicit note-on retrigger while a gate is already held. The new
    /// attack starts from the current envelope level.
    pub fn trigger(&mut self) {
        self.params.trigger = true;
        self.envelope.gate_on();
    }

    pub fn process_control<const N: usize>(&mut self, output: &mut [f32; N]) {
        for value in output {
            *value = self.tick()[0];
        }
    }

    pub fn process_with_control<const N: usize>(
        &mut self,
        left: &mut [f32; N],
        right: &mut [f32; N],
        output: &mut [f32; N],
    ) {
        for ((l, r), value) in left.iter_mut().zip(right).zip(output) {
            let [control, gain] = self.tick();
            *value = control;
            *l = audio(audio(*l) * gain);
            *r = audio(audio(*r) * gain);
        }
    }
}

impl Effect for NoteEnvelope {
    type Params = NoteEnvelopeParams;

    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        self.controls.prepare(self.sample_rate);
        self.configure();
        self.reset();
    }

    fn reset(&mut self) {
        self.controls.reset();
        self.envelope.reset();
        if self.params.trigger {
            self.envelope.gate_on();
        }
    }

    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        let old_gate = self.params.trigger;
        self.params = p;
        self.configure();
        self.controls.set([p.depth, p.gain]);
        if p.trigger && !old_gate {
            self.envelope.gate_on();
        } else if !p.trigger && old_gate {
            self.envelope.gate_off();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let gain = self.tick()[1];
            *l = audio(audio(*l) * gain);
            *r = audio(audio(*r) * gain);
        }
    }
}
