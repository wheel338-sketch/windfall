use crate::blocks::oscillator::Waveform;
use crate::param::param_set;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct AnalogEnvelopeParams {
    pub attack_ms: f32,
    pub decay_ms: f32,
    pub sustain: f32,
    pub release_ms: f32,
}

impl Default for AnalogEnvelopeParams {
    fn default() -> Self {
        Self {
            attack_ms: 5.0,
            decay_ms: 180.0,
            sustain: 0.7,
            release_ms: 180.0,
        }
    }
}

// Indices follow this table. Append new rows; never reorder existing rows.
param_set!(AnalogEnvelopeParams, "Voice envelope", {
    float [attack_ms] "attackMs" "Attack" { Milliseconds, Linear, 0.0, 2000.0, 5.0 }
    float [decay_ms] "decayMs" "Decay" { Milliseconds, Linear, 0.0, 4000.0, 180.0 }
    float [sustain] "sustain" "Sustain" { Fraction, Linear, 0.0, 1.0, 0.7 }
    float [release_ms] "releaseMs" "Release" { Milliseconds, Linear, 0.0, 4000.0, 180.0 }
});

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct AnalogOscParams {
    pub waveform: Waveform,
    pub semitones: f32,
    pub detune_cents: f32,
    pub level: f32,
    /// The share of each cycle the pulse waveform spends high.
    pub pulse_width: f32,
    /// Mixes in the same wave an octave up at equal weight.
    pub octave_up: bool,
    /// Mixes in the same wave two octaves up at equal weight with what is
    /// already there.
    pub two_octaves_up: bool,
}

impl Default for AnalogOscParams {
    fn default() -> Self {
        Self {
            waveform: Waveform::Saw,
            semitones: 0.0,
            detune_cents: 0.0,
            level: 0.5,
            pulse_width: 0.5,
            octave_up: false,
            two_octaves_up: false,
        }
    }
}

// Indices follow this table. Append new rows; never reorder existing rows.
param_set!(AnalogOscParams, "Oscillator controls", {
    choice [waveform] "waveform" "Oscillator waveform" { Waveform, Saw, [Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw", Square "square" "Square", Pulse "pulse" "Pulse", WhiteNoise "whiteNoise" "White noise", PinkNoise "pinkNoise" "Pink noise"] }
    float [semitones] "semitones" "Oscillator pitch" { Semitones, Linear, -24.0, 24.0, 0.0 }
    float [detune_cents] "detuneCents" "Oscillator detune" { Cents, Linear, -100.0, 100.0, 0.0 }
    float [level] "level" "Oscillator level" { Gain, Linear, 0.0, 1.0, 0.5 }
    float [pulse_width] "pulseWidth" "Oscillator pulse width" { Fraction, Linear, 0.01, 0.99, 0.5 }
    toggle [octave_up] "octaveUp" "Oscillator octave layer" { false }
    toggle [two_octaves_up] "twoOctavesUp" "Oscillator two-octave layer" { false }
});

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct TripleOscParams {
    pub oscillators: [AnalogOscParams; 3],
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub envelope_amount: f32,
    pub envelope: AnalogEnvelopeParams,
    pub gain: f32,
    /// How much of the band-pass output of the filter joins the low-pass one.
    pub band_mix: f32,
    /// How much of the high-pass output of the filter joins the low-pass one.
    pub high_mix: f32,
}

impl Default for TripleOscParams {
    fn default() -> Self {
        Self {
            oscillators: [AnalogOscParams::default(); 3],
            cutoff_hz: 4000.0,
            resonance: 0.15,
            envelope_amount: 0.3,
            envelope: AnalogEnvelopeParams::default(),
            gain: 0.5,
            band_mix: 0.0,
            high_mix: 0.0,
        }
    }
}

// Indices follow this table. Append new rows; never reorder existing rows.
param_set!(TripleOscParams, "Three oscillator synth", {
    choice [oscillators[0].waveform] "oscillators.0.waveform" "Oscillator 1 waveform" { Waveform, Saw, [Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw", Square "square" "Square", Pulse "pulse" "Pulse", WhiteNoise "whiteNoise" "White noise", PinkNoise "pinkNoise" "Pink noise"] }
    float [oscillators[0].semitones] "oscillators.0.semitones" "Oscillator 1 pitch" { Semitones, Linear, -24.0, 24.0, 0.0 }
    float [oscillators[0].detune_cents] "oscillators.0.detuneCents" "Oscillator 1 detune" { Cents, Linear, -100.0, 100.0, 0.0 }
    float [oscillators[0].level] "oscillators.0.level" "Oscillator 1 level" { Gain, Linear, 0.0, 1.0, 0.5 }
    choice [oscillators[1].waveform] "oscillators.1.waveform" "Oscillator 2 waveform" { Waveform, Saw, [Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw", Square "square" "Square", Pulse "pulse" "Pulse", WhiteNoise "whiteNoise" "White noise", PinkNoise "pinkNoise" "Pink noise"] }
    float [oscillators[1].semitones] "oscillators.1.semitones" "Oscillator 2 pitch" { Semitones, Linear, -24.0, 24.0, 0.0 }
    float [oscillators[1].detune_cents] "oscillators.1.detuneCents" "Oscillator 2 detune" { Cents, Linear, -100.0, 100.0, 0.0 }
    float [oscillators[1].level] "oscillators.1.level" "Oscillator 2 level" { Gain, Linear, 0.0, 1.0, 0.5 }
    choice [oscillators[2].waveform] "oscillators.2.waveform" "Oscillator 3 waveform" { Waveform, Saw, [Sine "sine" "Sine", Triangle "triangle" "Triangle", Saw "saw" "Saw", Square "square" "Square", Pulse "pulse" "Pulse", WhiteNoise "whiteNoise" "White noise", PinkNoise "pinkNoise" "Pink noise"] }
    float [oscillators[2].semitones] "oscillators.2.semitones" "Oscillator 3 pitch" { Semitones, Linear, -24.0, 24.0, 0.0 }
    float [oscillators[2].detune_cents] "oscillators.2.detuneCents" "Oscillator 3 detune" { Cents, Linear, -100.0, 100.0, 0.0 }
    float [oscillators[2].level] "oscillators.2.level" "Oscillator 3 level" { Gain, Linear, 0.0, 1.0, 0.5 }
    float [cutoff_hz] "cutoffHz" "Filter cutoff" { Hertz, Logarithmic, 20.0, 18000.0, 4000.0 }
    float [resonance] "resonance" "Filter resonance" { Fraction, Linear, 0.0, 1.0, 0.15 }
    float [envelope_amount] "envelopeAmount" "Filter envelope amount" { Fraction, Linear, 0.0, 1.0, 0.3 }
    float [envelope.attack_ms] "envelope.attackMs" "Attack" { Milliseconds, Linear, 0.0, 2000.0, 5.0 }
    float [envelope.decay_ms] "envelope.decayMs" "Decay" { Milliseconds, Linear, 0.0, 4000.0, 180.0 }
    float [envelope.sustain] "envelope.sustain" "Sustain" { Fraction, Linear, 0.0, 1.0, 0.7 }
    float [envelope.release_ms] "envelope.releaseMs" "Release" { Milliseconds, Linear, 0.0, 4000.0, 180.0 }
    float [gain] "gain" "Output level" { Gain, Linear, 0.0, 1.0, 0.5 }
    // Appended in round seven. Rows above keep their indices.
    float [oscillators[0].pulse_width] "oscillators.0.pulseWidth" "Oscillator 1 pulse width" { Fraction, Linear, 0.01, 0.99, 0.5 }
    toggle [oscillators[0].octave_up] "oscillators.0.octaveUp" "Oscillator 1 octave layer" { false }
    toggle [oscillators[0].two_octaves_up] "oscillators.0.twoOctavesUp" "Oscillator 1 two-octave layer" { false }
    float [oscillators[1].pulse_width] "oscillators.1.pulseWidth" "Oscillator 2 pulse width" { Fraction, Linear, 0.01, 0.99, 0.5 }
    toggle [oscillators[1].octave_up] "oscillators.1.octaveUp" "Oscillator 2 octave layer" { false }
    toggle [oscillators[1].two_octaves_up] "oscillators.1.twoOctavesUp" "Oscillator 2 two-octave layer" { false }
    float [oscillators[2].pulse_width] "oscillators.2.pulseWidth" "Oscillator 3 pulse width" { Fraction, Linear, 0.01, 0.99, 0.5 }
    toggle [oscillators[2].octave_up] "oscillators.2.octaveUp" "Oscillator 3 octave layer" { false }
    toggle [oscillators[2].two_octaves_up] "oscillators.2.twoOctavesUp" "Oscillator 3 two-octave layer" { false }
    float [band_mix] "bandMix" "Filter band-pass level" { Gain, Linear, 0.0, 1.0, 0.0 }
    float [high_mix] "highMix" "Filter high-pass level" { Gain, Linear, 0.0, 1.0, 0.0 }
});

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct WaveLaneParams {
    pub scan: f32,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub envelope_amount: f32,
    pub envelope: AnalogEnvelopeParams,
    pub gain: f32,
}

impl Default for WaveLaneParams {
    fn default() -> Self {
        Self {
            scan: 0.55,
            cutoff_hz: 4000.0,
            resonance: 0.15,
            envelope_amount: 0.3,
            envelope: AnalogEnvelopeParams::default(),
            gain: 0.5,
        }
    }
}

// Indices follow this table. Append new rows; never reorder existing rows.
param_set!(WaveLaneParams, "Wavetable synth", {
    float [scan] "scan" "Table position" { Fraction, Linear, 0.0, 1.0, 0.55 }
    float [cutoff_hz] "cutoffHz" "Filter cutoff" { Hertz, Logarithmic, 20.0, 18000.0, 4000.0 }
    float [resonance] "resonance" "Filter resonance" { Fraction, Linear, 0.0, 1.0, 0.15 }
    float [envelope_amount] "envelopeAmount" "Filter envelope amount" { Fraction, Linear, 0.0, 1.0, 0.3 }
    float [envelope.attack_ms] "envelope.attackMs" "Attack" { Milliseconds, Linear, 0.0, 2000.0, 5.0 }
    float [envelope.decay_ms] "envelope.decayMs" "Decay" { Milliseconds, Linear, 0.0, 4000.0, 180.0 }
    float [envelope.sustain] "envelope.sustain" "Sustain" { Fraction, Linear, 0.0, 1.0, 0.7 }
    float [envelope.release_ms] "envelope.releaseMs" "Release" { Milliseconds, Linear, 0.0, 4000.0, 180.0 }
    float [gain] "gain" "Output level" { Gain, Linear, 0.0, 1.0, 0.5 }
});

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct MacroVoiceParams {
    pub engine_mix: f32,
    pub tone: f32,
    pub motion: f32,
    pub shape: f32,
}

impl Default for MacroVoiceParams {
    fn default() -> Self {
        Self {
            engine_mix: 0.0,
            tone: 0.65,
            motion: 0.25,
            shape: 0.4,
        }
    }
}

// Indices follow this table. Append new rows; never reorder existing rows.
param_set!(MacroVoiceParams, "Macro synth", {
    float [engine_mix] "engineMix" "Engine blend" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [tone] "tone" "Brightness" { Fraction, Linear, 0.0, 1.0, 0.65 }
    float [motion] "motion" "Modulation depth" { Fraction, Linear, 0.0, 1.0, 0.25 }
    float [shape] "shape" "Harmonic shape" { Fraction, Linear, 0.0, 1.0, 0.4 }
});

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct AcidStep {
    pub enabled: bool,
    pub pitch_offset: i8,
    pub accent: bool,
    pub slide: bool,
}

impl Default for AcidStep {
    fn default() -> Self {
        Self {
            enabled: true,
            pitch_offset: 0,
            accent: false,
            slide: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct AcidLineParams {
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub envelope_amount: f32,
    pub decay_ms: f32,
    pub accent_amount: f32,
    pub slide_ms: f32,
    pub pulse_mix: f32,
    pub gain: f32,
    pub sequencer: bool,
    pub gate: f32,
    pub manual_slide: bool,
    pub steps: [AcidStep; 16],
}

impl Default for AcidLineParams {
    fn default() -> Self {
        Self {
            cutoff_hz: 180.0,
            resonance: 0.65,
            envelope_amount: 0.65,
            decay_ms: 220.0,
            accent_amount: 0.5,
            slide_ms: 80.0,
            pulse_mix: 0.0,
            gain: 0.45,
            sequencer: false,
            gate: 0.65,
            manual_slide: false,
            steps: [AcidStep::default(); 16],
        }
    }
}

// Indices follow this table. Append new rows; never reorder existing rows.
param_set!(AcidLineParams, "Sequenced resonant bass", {
    float [cutoff_hz] "cutoffHz" "Filter cutoff" { Hertz, Logarithmic, 20.0, 18000.0, 180.0 }
    float [resonance] "resonance" "Filter resonance" { Fraction, Linear, 0.0, 1.0, 0.65 }
    float [envelope_amount] "envelopeAmount" "Filter envelope amount" { Fraction, Linear, 0.0, 1.0, 0.65 }
    float [decay_ms] "decayMs" "Envelope decay" { Milliseconds, Linear, 10.0, 2000.0, 220.0 }
    float [accent_amount] "accentAmount" "Accent amount" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [slide_ms] "slideMs" "Pitch slide time" { Milliseconds, Linear, 0.0, 1000.0, 80.0 }
    float [pulse_mix] "pulseMix" "Pulse blend" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [gain] "gain" "Output level" { Gain, Linear, 0.0, 1.0, 0.45 }
    toggle [sequencer] "sequencer" "Internal sequencer" { false }
    float [gate] "gate" "Step gate length" { Fraction, Linear, 0.05, 1.0, 0.65 }
    toggle [manual_slide] "manualSlide" "Slide held notes" { false }
    toggle [steps[0].enabled] "steps.0.enabled" "Step 1 enabled" { true }
    int [steps[0].pitch_offset] "steps.0.pitchOffset" "Step 1 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[0].accent] "steps.0.accent" "Step 1 accent" { false }
    toggle [steps[0].slide] "steps.0.slide" "Step 1 slide" { false }
    toggle [steps[1].enabled] "steps.1.enabled" "Step 2 enabled" { true }
    int [steps[1].pitch_offset] "steps.1.pitchOffset" "Step 2 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[1].accent] "steps.1.accent" "Step 2 accent" { false }
    toggle [steps[1].slide] "steps.1.slide" "Step 2 slide" { false }
    toggle [steps[2].enabled] "steps.2.enabled" "Step 3 enabled" { true }
    int [steps[2].pitch_offset] "steps.2.pitchOffset" "Step 3 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[2].accent] "steps.2.accent" "Step 3 accent" { false }
    toggle [steps[2].slide] "steps.2.slide" "Step 3 slide" { false }
    toggle [steps[3].enabled] "steps.3.enabled" "Step 4 enabled" { true }
    int [steps[3].pitch_offset] "steps.3.pitchOffset" "Step 4 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[3].accent] "steps.3.accent" "Step 4 accent" { false }
    toggle [steps[3].slide] "steps.3.slide" "Step 4 slide" { false }
    toggle [steps[4].enabled] "steps.4.enabled" "Step 5 enabled" { true }
    int [steps[4].pitch_offset] "steps.4.pitchOffset" "Step 5 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[4].accent] "steps.4.accent" "Step 5 accent" { false }
    toggle [steps[4].slide] "steps.4.slide" "Step 5 slide" { false }
    toggle [steps[5].enabled] "steps.5.enabled" "Step 6 enabled" { true }
    int [steps[5].pitch_offset] "steps.5.pitchOffset" "Step 6 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[5].accent] "steps.5.accent" "Step 6 accent" { false }
    toggle [steps[5].slide] "steps.5.slide" "Step 6 slide" { false }
    toggle [steps[6].enabled] "steps.6.enabled" "Step 7 enabled" { true }
    int [steps[6].pitch_offset] "steps.6.pitchOffset" "Step 7 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[6].accent] "steps.6.accent" "Step 7 accent" { false }
    toggle [steps[6].slide] "steps.6.slide" "Step 7 slide" { false }
    toggle [steps[7].enabled] "steps.7.enabled" "Step 8 enabled" { true }
    int [steps[7].pitch_offset] "steps.7.pitchOffset" "Step 8 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[7].accent] "steps.7.accent" "Step 8 accent" { false }
    toggle [steps[7].slide] "steps.7.slide" "Step 8 slide" { false }
    toggle [steps[8].enabled] "steps.8.enabled" "Step 9 enabled" { true }
    int [steps[8].pitch_offset] "steps.8.pitchOffset" "Step 9 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[8].accent] "steps.8.accent" "Step 9 accent" { false }
    toggle [steps[8].slide] "steps.8.slide" "Step 9 slide" { false }
    toggle [steps[9].enabled] "steps.9.enabled" "Step 10 enabled" { true }
    int [steps[9].pitch_offset] "steps.9.pitchOffset" "Step 10 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[9].accent] "steps.9.accent" "Step 10 accent" { false }
    toggle [steps[9].slide] "steps.9.slide" "Step 10 slide" { false }
    toggle [steps[10].enabled] "steps.10.enabled" "Step 11 enabled" { true }
    int [steps[10].pitch_offset] "steps.10.pitchOffset" "Step 11 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[10].accent] "steps.10.accent" "Step 11 accent" { false }
    toggle [steps[10].slide] "steps.10.slide" "Step 11 slide" { false }
    toggle [steps[11].enabled] "steps.11.enabled" "Step 12 enabled" { true }
    int [steps[11].pitch_offset] "steps.11.pitchOffset" "Step 12 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[11].accent] "steps.11.accent" "Step 12 accent" { false }
    toggle [steps[11].slide] "steps.11.slide" "Step 12 slide" { false }
    toggle [steps[12].enabled] "steps.12.enabled" "Step 13 enabled" { true }
    int [steps[12].pitch_offset] "steps.12.pitchOffset" "Step 13 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[12].accent] "steps.12.accent" "Step 13 accent" { false }
    toggle [steps[12].slide] "steps.12.slide" "Step 13 slide" { false }
    toggle [steps[13].enabled] "steps.13.enabled" "Step 14 enabled" { true }
    int [steps[13].pitch_offset] "steps.13.pitchOffset" "Step 14 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[13].accent] "steps.13.accent" "Step 14 accent" { false }
    toggle [steps[13].slide] "steps.13.slide" "Step 14 slide" { false }
    toggle [steps[14].enabled] "steps.14.enabled" "Step 15 enabled" { true }
    int [steps[14].pitch_offset] "steps.14.pitchOffset" "Step 15 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[14].accent] "steps.14.accent" "Step 15 accent" { false }
    toggle [steps[14].slide] "steps.14.slide" "Step 15 slide" { false }
    toggle [steps[15].enabled] "steps.15.enabled" "Step 16 enabled" { true }
    int [steps[15].pitch_offset] "steps.15.pitchOffset" "Step 16 pitch" { Semitones, -24, 24, 0 }
    toggle [steps[15].accent] "steps.15.accent" "Step 16 accent" { false }
    toggle [steps[15].slide] "steps.15.slide" "Step 16 slide" { false }
});
