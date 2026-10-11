use super::{
    core,
    runtime::{Context, Frame, Patch, Tone, instrument, oscillator},
};
use crate::param::param_set;
use crate::{EnvelopeParams, Waveform};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PolyResponse {
    Low4,
    Low2,
    Band,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct DualPolyParams {
    pub wave_a: Waveform,
    pub wave_b: Waveform,
    pub balance: f32,
    pub coarse_b: f32,
    pub fine_b: f32,
    pub pulse_width: f32,
    pub pwm: f32,
    pub cross_mod: f32,
    pub sub: f32,
    pub noise: f32,
    pub drift: f32,
    pub response: PolyResponse,
    pub drive: f32,
    pub chorus: f32,
    pub delay: f32,
    pub reverb: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub filter_amount: f32,
    pub key_track: f32,
    pub envelope: EnvelopeParams,
    pub filter_envelope: EnvelopeParams,
    pub lfo_rate_hz: f32,
    pub lfo_filter: f32,
    pub unison: u8,
    pub detune_cents: f32,
    pub spread: f32,
    pub glide_ms: f32,
    pub polyphony: u8,
    pub gain: f32,
}
impl Default for DualPolyParams {
    fn default() -> Self {
        Self {
            wave_a: Waveform::Saw,
            wave_b: Waveform::Pulse,
            balance: 0.5,
            coarse_b: 0.0,
            fine_b: 7.0,
            pulse_width: 0.5,
            pwm: 0.0,
            cross_mod: 0.0,
            sub: 0.2,
            noise: 0.0,
            drift: 0.2,
            response: PolyResponse::Low4,
            drive: 0.2,
            chorus: 0.0,
            delay: 0.0,
            reverb: 0.0,
            cutoff_hz: 4000.0,
            resonance: 0.2,
            filter_amount: 2.0,
            key_track: 0.5,
            envelope: EnvelopeParams::default(),
            filter_envelope: EnvelopeParams::default(),
            lfo_rate_hz: 0.8,
            lfo_filter: 0.0,
            unison: 1,
            detune_cents: 12.0,
            spread: 0.6,
            glide_ms: 0.0,
            polyphony: 8,
            gain: 0.5,
        }
    }
}
param_set!(DualPolyParams, "Dual vintage poly", {
choice [wave_a] "waveA" "Bank A waveform" { Waveform, Saw, [Saw "saw" "Saw", Pulse "pulse" "Pulse", Triangle "triangle" "Triangle", Sine "sine" "Sine", Square "square" "Square", WhiteNoise "whiteNoise" "White noise", PinkNoise "pinkNoise" "Pink noise"] }
choice [wave_b] "waveB" "Bank B waveform" { Waveform, Pulse, [Saw "saw" "Saw", Pulse "pulse" "Pulse", Triangle "triangle" "Triangle", Sine "sine" "Sine", Square "square" "Square", WhiteNoise "whiteNoise" "White noise", PinkNoise "pinkNoise" "Pink noise"] }
float [balance] "balance" "Bank balance" { Fraction, Linear, 0.0, 1.0, 0.5 }
float [coarse_b] "coarseB" "Bank B pitch" { Semitones, Linear, -24.0, 24.0, 0.0 }
float [fine_b] "fineB" "Bank B detune" { Cents, Linear, -100.0, 100.0, 7.0 }
float [pulse_width] "pulseWidth" "Pulse width" { Fraction, Linear, 0.05, 0.95, 0.5 }
float [pwm] "pwm" "Pulse modulation" { Fraction, Linear, 0.0, 0.4, 0.0 }
float [cross_mod] "crossMod" "Cross modulation" { Ratio, Linear, 0.0, 4.0, 0.0 }
float [sub] "sub" "Sub level" { Fraction, Linear, 0.0, 1.0, 0.2 }
float [noise] "noise" "Noise level" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [drift] "drift" "Component drift" { Fraction, Linear, 0.0, 1.0, 0.2 }
choice [response] "response" "Filter response" { PolyResponse, Low4, [Low4 "low4" "24 dB lowpass", Low2 "low2" "12 dB lowpass", Band "band" "Bandpass", High "high" "Highpass"] }
float [drive] "drive" "Filter drive" { Fraction, Linear, 0.0, 1.0, 0.2 }
float [chorus] "chorus" "Stereo chorus" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [delay] "delay" "Echo mix" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [reverb] "reverb" "Room mix" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [cutoff_hz] "cutoffHz" "Filter cutoff" { Hertz, Logarithmic, 20.0, 20000.0, 4000.0 }
float [resonance] "resonance" "Filter resonance" { Fraction, Linear, 0.0, 1.0, 0.2 }
float [filter_amount] "filterAmount" "Filter envelope depth" { Octaves, Linear, -6.0, 6.0, 2.0 }
float [key_track] "keyTrack" "Filter key tracking" { Fraction, Linear, 0.0, 1.0, 0.5 }
float [envelope.attack_ms] "envelope.attackMs" "Amp attack" { Milliseconds, Linear, 0.0, 4000.0, 2.0 }
float [envelope.decay_ms] "envelope.decayMs" "Amp decay" { Milliseconds, Linear, 1.0, 4000.0, 200.0 }
float [envelope.sustain] "envelope.sustain" "Amp sustain" { Fraction, Linear, 0.0, 1.0, 0.8 }
float [envelope.release_ms] "envelope.releaseMs" "Amp release" { Milliseconds, Linear, 1.0, 4000.0, 150.0 }
float [filter_envelope.attack_ms] "filterEnvelope.attackMs" "Filter attack" { Milliseconds, Linear, 0.0, 4000.0, 2.0 }
float [filter_envelope.decay_ms] "filterEnvelope.decayMs" "Filter decay" { Milliseconds, Linear, 1.0, 4000.0, 200.0 }
float [filter_envelope.sustain] "filterEnvelope.sustain" "Filter sustain" { Fraction, Linear, 0.0, 1.0, 0.8 }
float [filter_envelope.release_ms] "filterEnvelope.releaseMs" "Filter release" { Milliseconds, Linear, 1.0, 4000.0, 150.0 }
float [lfo_rate_hz] "lfoRateHz" "LFO rate" { Hertz, Logarithmic, 0.01, 20.0, 0.8 }
float [lfo_filter] "lfoFilter" "LFO filter depth" { Octaves, Linear, 0.0, 4.0, 0.0 }
int [unison] "unison" "Unison voices" { None, 1.0, 7.0, 1.0 }
float [detune_cents] "detuneCents" "Unison detune" { Cents, Linear, 0.0, 60.0, 12.0 }
float [spread] "spread" "Stereo spread" { Fraction, Linear, 0.0, 1.0, 0.6 }
float [glide_ms] "glideMs" "Glide" { Milliseconds, Linear, 0.0, 2000.0, 0.0 }
int [polyphony] "polyphony" "Polyphony" { None, 1.0, 16.0, 8.0 }
float [gain] "gain" "Output level" { Gain, Linear, 0.0, 1.0, 0.5 }
});

impl Patch for DualPolyParams {
    fn frame(&self) -> Frame {
        Frame {
            amp: self.envelope,
            filter_env: self.filter_envelope,
            cutoff: self.cutoff_hz,
            env_amount: self.filter_amount,
            key_track: self.key_track,
            lfo_rate: self.lfo_rate_hz,
            lfo_filter: self.lfo_filter,
            gain: self.gain,
            glide: self.glide_ms,
            polyphony: self.polyphony as usize,
            chorus: self.chorus,
            delay: self.delay,
            reverb: self.reverb,
            ..Frame::default()
        }
    }
    fn sample(&self, t: &mut Tone, c: &Context) -> (f32, f32) {
        let drift = t.drift.tick(c.slot, c.rate) * self.drift;
        let mut sum = (0.0, 0.0);
        let noise = t.noise.bipolar() * self.noise;
        for u in 0..self.unison as usize {
            let pos = core::unison_position(u, self.unison as usize);
            let pitch = c.pitch + (pos * self.detune_cents + drift) / 100.0;
            let a = oscillator(
                t,
                u,
                0,
                pitch,
                c.rate,
                self.wave_a,
                self.pulse_width + c.lfo * self.pwm,
            );
            // Linear frequency modulation from bank A into bank B. The positive
            // frequency bound prevents runaway at extreme cross-mod settings.
            let inc = core::increment(pitch + self.coarse_b + self.fine_b / 100.0, c.rate);
            let b = t.oscillators[u][1].tick(
                self.wave_b,
                (inc * (1.0 + self.cross_mod * a)).clamp(0.0, 0.249),
                self.pulse_width,
            );
            let sub = oscillator(t, u, 2, pitch - 12.0, c.rate, Waveform::Square, 0.5);
            let raw = a * (1.0 - self.balance) + b * self.balance + sub * self.sub + noise;
            let pan = core::pan_gains(pos * self.spread);
            sum.0 += raw * pan.0;
            sum.1 += raw * pan.1;
        }
        let k = core::ladder_feedback(self.resonance);
        let g = core::swept_gain(c.cutoff, 0.0, c.rate);
        let filter = |t: &mut Tone, side: usize, input: f32| {
            let stages =
                t.ladder[side].tick(core::drive(input / self.unison as f32, self.drive), g, k);
            let response = match self.response {
                PolyResponse::Low4 => core::ladder_low4(&stages),
                PolyResponse::Low2 => core::ladder_low2(&stages),
                PolyResponse::Band => core::ladder_band2(&stages),
                PolyResponse::High => core::ladder_high2(&stages),
            };
            response * core::ladder_makeup(k)
        };
        (filter(t, 0, sum.0), filter(t, 1, sum.1))
    }
}
instrument!(DualPoly, DualPolyParams);
