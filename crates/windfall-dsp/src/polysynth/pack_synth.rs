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
pub enum PackPatch {
    EmberBass,
    EmberGlass,
    EmberKeys,
    EmberBell,
    PrismLead,
    PrismSweep,
    PrismMallet,
    PrismChime,
    MeadowPad,
    MeadowAir,
    MeadowPluck,
    MeadowMetal,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct PackSynthParams {
    pub patch: PackPatch,
    pub tone: f32,
    pub shape: f32,
    pub motion: f32,
    pub attack: f32,
    pub decay: f32,
    pub width: f32,
    pub air: f32,
    pub crunch: f32,
    pub echo: f32,
    pub space: f32,
    pub glide_ms: f32,
    pub polyphony: u8,
    pub gain: f32,
}
impl Default for PackSynthParams {
    fn default() -> Self {
        Self {
            patch: PackPatch::EmberBass,
            tone: 0.65,
            shape: 0.3,
            motion: 0.2,
            attack: 0.0,
            decay: 0.4,
            width: 0.5,
            air: 0.0,
            crunch: 0.0,
            echo: 0.0,
            space: 0.0,
            glide_ms: 0.0,
            polyphony: 8,
            gain: 0.5,
        }
    }
}
param_set!(PackSynthParams, "Pack synth", {
choice [patch] "patch" "Windfall patch" { PackPatch, EmberBass, [EmberBass "emberBass" "Ember / Copper bass", EmberGlass "emberGlass" "Ember / Glass sweep", EmberKeys "emberKeys" "Ember / Soft keys", EmberBell "emberBell" "Ember / Bronze bell", PrismLead "prismLead" "Prism / Pulse lead", PrismSweep "prismSweep" "Prism / Hollow sweep", PrismMallet "prismMallet" "Prism / Wood mallet", PrismChime "prismChime" "Prism / High chime", MeadowPad "meadowPad" "Meadow / Warm pad", MeadowAir "meadowAir" "Meadow / Air motion", MeadowPluck "meadowPluck" "Meadow / Seed pluck", MeadowMetal "meadowMetal" "Meadow / Silver tine"] }
float [tone] "tone" "Brightness" { Fraction, Linear, 0.0, 1.0, 0.65 }
float [shape] "shape" "Harmonic shape" { Fraction, Linear, 0.0, 1.0, 0.3 }
float [motion] "motion" "Motion" { Fraction, Linear, 0.0, 1.0, 0.2 }
float [attack] "attack" "Attack" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [decay] "decay" "Decay and release" { Fraction, Linear, 0.0, 1.0, 0.4 }
float [width] "width" "Width" { Fraction, Linear, 0.0, 1.0, 0.5 }
float [air] "air" "Air" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [crunch] "crunch" "Crunch" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [echo] "echo" "Echo" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [space] "space" "Space" { Fraction, Linear, 0.0, 1.0, 0.0 }
float [glide_ms] "glideMs" "Glide" { Milliseconds, Linear, 0.0, 2000.0, 0.0 }
int [polyphony] "polyphony" "Polyphony" { None, 1.0, 16.0, 8.0 }
float [gain] "gain" "Output level" { Gain, Linear, 0.0, 1.0, 0.5 }
});
impl PackPatch {
    pub(super) fn address(self) -> (usize, usize) {
        let index = match self {
            Self::EmberBass => 0,
            Self::EmberGlass => 1,
            Self::EmberKeys => 2,
            Self::EmberBell => 3,
            Self::PrismLead => 4,
            Self::PrismSweep => 5,
            Self::PrismMallet => 6,
            Self::PrismChime => 7,
            Self::MeadowPad => 8,
            Self::MeadowAir => 9,
            Self::MeadowPluck => 10,
            Self::MeadowMetal => 11,
        };
        (index / 4, index % 4)
    }
}
impl Patch for PackSynthParams {
    const NEEDS_ASSETS: bool = true;
    fn frame(&self) -> Frame {
        let bank = self.patch.address().0;
        let envelope = EnvelopeParams {
            attack_ms: 2.0 + self.attack * 1500.0,
            decay_ms: 80.0 + self.decay * 1200.0,
            sustain: if bank == 2 { 0.9 } else { 0.7 },
            release_ms: 40.0 + self.decay * 1600.0,
        };
        Frame {
            amp: envelope,
            filter_env: envelope,
            cutoff: 100.0 * 180.0_f32.powf(self.tone),
            env_amount: self.motion * 2.0,
            lfo_rate: 0.3 + self.motion * 6.0,
            lfo_filter: self.motion,
            gain: self.gain,
            glide: self.glide_ms,
            polyphony: self.polyphony as usize,
            chorus: self.width * 0.35,
            delay: self.echo,
            reverb: self.space,
            ..Frame::default()
        }
    }
    fn sample(&self, t: &mut Tone, c: &Context) -> (f32, f32) {
        let (bank, mode) = self.patch.address();
        let mut sum = (0.0, 0.0);
        let count = if self.width > 0.01 { 3 } else { 1 };
        for u in 0..count {
            let pos = core::unison_position(u, count);
            let pitch = c.pitch + pos * self.width * 0.16 + c.lfo * self.motion * 0.08;
            let raw = match mode {
                0 => {
                    let a = oscillator(t, u, 0, pitch, c.rate, Waveform::Saw, 0.5);
                    let b = oscillator(
                        t,
                        u,
                        1,
                        pitch - 12.0,
                        c.rate,
                        Waveform::Pulse,
                        0.1 + 0.8 * self.shape,
                    );
                    (a * (1.0 - self.shape) + b * self.shape) * (0.8 + 0.1 * bank as f32)
                }
                1 => {
                    let phase = t.oscillators[u][0].phase();
                    let inc = core::increment(pitch, c.rate);
                    oscillator(t, u, 0, pitch, c.rate, Waveform::Sine, 0.5);
                    c.assets.map_or(0.0, |a| {
                        a.table(
                            bank,
                            phase,
                            inc,
                            (self.shape + c.lfo * self.motion * 0.2).clamp(0.0, 1.0),
                        )
                    })
                }
                2 => {
                    c.assets.map_or(0.0, |a| {
                        a.sample(bank, c.key, &mut t.sample_phase[u], pitch, c.rate)
                    }) * (0.6 + 0.8 * self.shape)
                }
                _ => {
                    let carrier = t.oscillators[u][0].phase();
                    oscillator(t, u, 0, pitch, c.rate, Waveform::Sine, 0.5);
                    let ratio = 2.0 + bank as f32;
                    let modulator = oscillator(
                        t,
                        u,
                        1,
                        pitch + 12.0 * ratio.log2(),
                        c.rate,
                        Waveform::Sine,
                        0.5,
                    );
                    let index = 0.1 + 4.0 * self.shape + c.lfo * self.motion;
                    (std::f32::consts::TAU * carrier + modulator * index).sin()
                }
            };
            let air = t.noise.bipolar() * self.air * 0.12;
            let pan = core::pan_gains(pos * self.width);
            sum.0 += (raw + air) * pan.0;
            sum.1 += (raw + air) * pan.1;
        }
        let filter = |t: &mut Tone, side: usize, input: f32| {
            t.svf[side]
                .tick(
                    core::drive(input / count as f32, self.crunch),
                    core::swept_gain(c.cutoff, 0.0, c.rate),
                    0.8,
                )
                .0
        };
        (filter(t, 0, sum.0), filter(t, 1, sum.1))
    }
}
instrument!(PackSynth, PackSynthParams);
