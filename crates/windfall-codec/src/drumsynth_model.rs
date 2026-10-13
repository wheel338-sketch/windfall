//! Paul Kellett's block-ordered DrumSynth model. Copyright (c) 1998-2000
//! Paul Kellett; GPL-2.0-or-later. Bounds, local state and explicit PRNG are
//! Windfall additions. No code from a downloaded tree is executed.
use super::*;
use std::f64::consts::{FRAC_PI_4, TAU};

const BLOCK: usize = 1200;

struct Envelope {
    points: Vec<(f64, f64)>,
    point: usize,
    next: f64,
    value: f64,
    delta: f64,
    stretch: f64,
}
impl Envelope {
    fn new(points: Vec<(f64, f64)>, stretch: f64) -> Self {
        Self {
            points,
            point: 0,
            next: 0.0,
            value: 0.0,
            delta: 0.0,
            stretch,
        }
    }
    fn end(&self) -> f64 {
        self.points.last().unwrap().0
    }
    // Reference UpdateEnv: a coincident point consumes one sample, not a
    // division by zero. The implicit sentinel is bounded and local.
    fn at(&mut self, frame: usize, stop: bool) -> f64 {
        let t = frame as f64;
        if t < self.next {
            self.value += self.delta;
        } else if stop && t >= self.end() {
            self.value = 0.0;
            self.delta = 0.0;
            self.next = f64::INFINITY;
        } else {
            let current = self.points.get(self.point).copied().unwrap_or((t, 0.0));
            let next = self
                .points
                .get(self.point + 1)
                .copied()
                .unwrap_or((442000.0 * self.stretch, 0.0));
            self.next = next.0;
            self.value = current.1;
            self.delta = (next.1 - current.1) / (self.next - t).max(1.0);
            self.point += 1;
        }
        self.value
    }
}

struct Random(u64);
impl Random {
    // Explicit SplitMix64 replaces platform-dependent libc rand(). Same seed
    // and block order on native and wasm; FixedSeq covers both noise bands too.
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut x = self.0;
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
        x ^= x >> 31;
        (x >> 40) as f64 / 16777215.0
    }
}

fn wave(phase: f64, form: usize) -> f64 {
    match form {
        0 => phase.sin(),
        1 => (2.0 * (phase * 0.5).sin()).abs() - 1.0,
        2 => {
            let x = 4.0 * phase.rem_euclid(TAU) / TAU - 1.0;
            if x > 1.0 { 2.0 - x } else { x }
        }
        3 => 2.0 * (phase % TAU) / TAU - 1.0,
        _ => {
            if phase.sin() > 0.0 {
                1.0
            } else {
                -1.0
            }
        }
    }
}

struct Band {
    on: bool,
    level: f64,
    frequency: f64,
    width: f64,
    step: usize,
    phase: f64,
    deviation: f64,
}
impl Band {
    fn read(p: &DrumSynthPreset, section: &str, tune: f64, rate: u32) -> Result<Self, CodecError> {
        let on = integer(p, section, "on", 0, 1)? == 1;
        let frequency = p.number(section, "f", 1000.0, 0.0, 20000.0)? * tune * TAU / rate as f64;
        let df = integer(p, section, "df", 50, 100)? as f64;
        let denominator = 10000.0 - 6600.0 * (frequency.sqrt() - 0.19);
        if on && denominator <= 0.0 {
            return Err(invalid(
                "noise-band frequency/width is unstable at this rate",
            ));
        }
        let width = df * df / denominator.max(1.0);
        let step = 1 + ((40.0 - df / 2.5) / (width + 1.0 + frequency)) as usize;
        Ok(Self {
            on,
            level: (integer(p, section, "level", 128, 256)? as f64).powi(2),
            frequency,
            width,
            step,
            phase: FRAC_PI_4,
            deviation: 0.0,
        })
    }
}

fn integer(
    p: &DrumSynthPreset,
    section: &str,
    key: &str,
    default: usize,
    max: usize,
) -> Result<usize, CodecError> {
    let n = p.number(section, key, default as f64, 0.0, max as f64)?;
    if n.fract() != 0.0 {
        return Err(invalid("integer selector required"));
    }
    Ok(n as usize)
}

pub(super) fn render(
    p: &DrumSynthPreset,
    rate: u32,
    max_frames: usize,
    seed: u64,
) -> Result<DrumSynthRendered, CodecError> {
    if !(8000..=192000).contains(&rate) || max_frames == 0 || max_frames > MAX_DRUMSYNTH_FRAMES {
        return Err(invalid("invalid render rate or frame budget"));
    }
    let stretch =
        p.number("general", "stretch", 100.0, 20.0, 1000.0)? * 0.01 * rate as f64 / 44100.0;
    let gain = 10.0_f64.powf(p.number("general", "level", 0.0, -96.0, 24.0)? * 0.05);
    let tune = 2.0_f64.powf(p.number("general", "tuning", 0.0, -96.0, 96.0)? / 12.0);
    let mut envelopes = Vec::new();
    for (section, key) in [
        ("tone", "envelope"),
        ("noise", "envelope"),
        ("overtones", "envelope1"),
        ("overtones", "envelope2"),
        ("noiseband", "envelope"),
        ("noiseband2", "envelope"),
        ("general", "filterenv"),
    ] {
        envelopes.push(Envelope::new(p.envelope(section, key, stretch)?, stretch));
    }
    let mut active = [false; 6];
    let mut levels = [0.0_f64; 5];
    for (i, section) in [
        "tone",
        "noise",
        "overtones",
        "noiseband",
        "noiseband2",
        "distortion",
    ]
    .iter()
    .enumerate()
    {
        active[i] = integer(p, section, "on", 0, 1)? == 1;
        if i < 5 {
            levels[i] =
                (integer(p, section, "level", if i == 1 { 0 } else { 128 }, 256)? as f64).powi(2);
        }
    }
    if !active[..5].iter().any(|x| *x) {
        return Err(CodecError::NoAudio);
    }
    let end = envelopes[..6]
        .iter()
        .enumerate()
        .filter(|(i, _)| active[if *i > 2 { i - 1 } else { *i }])
        .map(|(_, e)| e.end())
        .fold(0.0_f64, f64::max);
    let length = 2400 + BLOCK * (end / BLOCK as f64) as usize;
    if length > max_frames {
        return Err(CodecError::TooLarge {
            limit_bytes: max_frames as u64 * 4,
        });
    }
    let f1 = (p.number("tone", "f1", 200.0, 0.0, 20000.0)? * tune * TAU / rate as f64).max(0.001);
    let mut f2 = p.number("tone", "f2", 120.0, 0.0, 20000.0)? * tune * TAU / rate as f64;
    let droop = p.number("tone", "droop", 0.0, 0.0, 100.0)?;
    // Kellett computes droop before LongestEnv scales the envelope times.
    // Preserve that authored-sample trajectory, including non-unity Stretch;
    // the later linear sweep and amplitude envelope use stretched duration.
    let authored_end = envelopes[0].end() / stretch;
    let droop_rate = -4.0 * 10.0_f64.powf((droop - 20.0) / 30.0) / authored_end.max(1.0);
    let delta = if droop > 0.0 {
        f2 = f1 + (f2 - f1) / (1.0 - (droop_rate * authored_end).exp());
        f1 - f2
    } else {
        f2 - f1
    };
    let mut tone_phase = p.number("tone", "phase", 90.0, -360.0, 360.0)?.to_radians();
    let mut tracking_phase = tone_phase;
    let mut overtone_phase = [tone_phase; 2];
    let mut overtone_freq = [
        p.number("overtones", "f1", 200.0, 0.0, 20000.0)? * tune * TAU / rate as f64,
        p.number("overtones", "f2", 120.0, 0.0, 20000.0)? * tune * TAU / rate as f64,
    ];
    let tracks = [
        integer(p, "overtones", "track1", 0, 1)? == 1 && active[0],
        integer(p, "overtones", "track2", 0, 1)? == 1 && active[0],
    ];
    for i in 0..2 {
        if tracks[i] {
            overtone_freq[i] /= f1;
        }
    }
    let forms = [
        integer(p, "overtones", "wave1", 0, 4)?,
        integer(p, "overtones", "wave2", 0, 4)?,
    ];
    let method = integer(p, "overtones", "method", 2, 3)?;
    if active[2] && method == 3 && overtone_freq.iter().any(|f| *f <= 0.0) {
        return Err(invalid("cymbal oscillators require positive frequencies"));
    }
    let balance = integer(p, "overtones", "param", 50, 100)? as f64 * 0.01;
    let drive = 8.0 * balance.powi(3);
    let cymbal_q = (0.28 + (1.0 - balance).powi(2)).powi(2);
    let cymbal_f = (1.8 - 0.7 * cymbal_q) * 0.92;
    let mut periods = [0.0; 6];
    for (i, period) in periods.iter_mut().enumerate() {
        *period = TAU / overtone_freq[0].max(1e-12)
            + (TAU / overtone_freq[1].max(1e-12) - TAU / overtone_freq[0].max(1e-12))
                * 0.2
                * i as f64;
    }
    let mut counters = periods;
    let mut cymbal = [0.0; 3];
    let main_filter = integer(p, "general", "filter", 0, 1)? * 2;
    let filter = if main_filter == 0 {
        integer(p, "overtones", "filter", 0, 1)?
    } else {
        main_filter
    };
    let resonance = (0.0101 * p.number("general", "resonance", 0.0, 0.0, 100.0)?).sqrt();
    let highpass = integer(p, "general", "highpass", 0, 1)? as f64;
    let mut filter_in = 0.0;
    let mut filter_out = 0.0;
    let slope = p.number("noise", "slope", 0.0, -100.0, 100.0)?;
    if slope.fract() != 0.0 {
        return Err(invalid("noise slope must be an integer"));
    }
    let (a, b, c, d, g) = if slope < 0.0 {
        (
            1.0 + slope / 105.0,
            0.0,
            0.0,
            -slope / 105.0,
            (1.0 + 0.0005 * slope * slope) * levels[1],
        )
    } else {
        (1.0, -slope / 50.0, slope.abs() / 100.0, 0.0, levels[1])
    };
    let mut random = Random(if integer(p, "noise", "fixedseq", 0, 1)? == 1 {
        1
    } else {
        seed
    });
    let mut noise = [0.0; 3];
    let mut noise_out = 0.0;
    let mut bands = [
        Band::read(p, "noiseband", tune, rate)?,
        Band::read(p, "noiseband2", tune, rate)?,
    ];
    let bits = integer(p, "distortion", "bits", 0, 8)?;
    let clipping = integer(p, "distortion", "clipping", 0, 96)? as f64;
    let step = [1, 2, 3, 4, 8, 10, 20][integer(p, "distortion", "rate", 0, 6)?];
    let attenuation = if active[5] {
        2.0_f64.powi(2 * bits as i32)
    } else {
        1.0
    };
    let clip = if active[5] {
        (gain
            * levels
                .iter()
                .enumerate()
                .filter(|(i, _)| active[*i])
                .map(|(_, v)| *v)
                .fold(0.0_f64, f64::max))
        .min(32700.0)
    } else {
        32700.0
    };
    let distortion_gain = gain * attenuation * 10.0_f64.powf(clipping * 0.05);
    let mut out = Vec::with_capacity(length);
    for start in (0..length).step_by(BLOCK) {
        let mut data = [0.0_f64; BLOCK];
        let mut increments = [f2; BLOCK];
        if active[1] {
            for (i, sample) in data.iter_mut().enumerate() {
                let env = envelopes[1].at(start + i, false);
                noise[2] = noise[1];
                noise[1] = noise[0];
                noise[0] = 2.0 * random.unit() - 1.0;
                noise_out = a * noise[0] + b * noise[1] + c * noise[2] + d * noise_out;
                *sample = noise_out * g * env;
            }
            if (start + BLOCK) as f64 >= envelopes[1].end() {
                active[1] = false;
            }
        }
        if active[0] {
            tracking_phase = tone_phase;
            for i in 0..BLOCK {
                let t = (start + i) as f64;
                increments[i] = if droop > 0.0 {
                    f2 + delta * (t * droop_rate).exp()
                } else {
                    f1 + t / envelopes[0].end().max(1.0) * delta
                };
                tone_phase += increments[i];
                data[i] += levels[0] * envelopes[0].at(start + i, false) * tone_phase.sin();
            }
            if (start + BLOCK) as f64 >= envelopes[0].end() {
                active[0] = false;
            }
        }
        for (band_index, band) in bands.iter_mut().enumerate() {
            if !band.on {
                continue;
            }
            for (i, sample) in data.iter_mut().enumerate() {
                let t = start + i;
                if t % band.step == 0 {
                    band.deviation = random.unit() - 0.5;
                }
                band.phase += band.frequency + band.width * band.deviation;
                *sample += band.phase.cos() * envelopes[4 + band_index].at(t, false) * band.level;
            }
            if (start + BLOCK) as f64 >= envelopes[4 + band_index].end() {
                band.on = false;
            }
        }
        for i in 0..BLOCK {
            let t = start + i;
            let mut ot = 0.0;
            if active[2] {
                let e1 = envelopes[2].at(t, true);
                let e2 = envelopes[3].at(t, true);
                tracking_phase += increments[i];
                for j in 0..2 {
                    overtone_phase[j] = if tracks[j] {
                        tracking_phase * overtone_freq[j]
                    } else {
                        overtone_phase[j] + overtone_freq[j]
                    };
                }
                let w2 = wave(overtone_phase[1], forms[1]);
                ot = match method {
                    0 => {
                        levels[2]
                            * ((1.0 - balance) * e1 * wave(overtone_phase[0], forms[0])
                                + balance * e2 * w2)
                    }
                    1 => levels[2] * e1 * wave(overtone_phase[0] + drive * e2 * w2, forms[0]),
                    2 => {
                        levels[2]
                            * e1
                            * wave(overtone_phase[0], forms[0])
                            * (1.0 - drive / 8.0 + drive / 8.0 * e2 * w2)
                    }
                    _ => {
                        for j in 0..6 {
                            counters[j] += 1.0;
                            if counters[j] > periods[j] {
                                counters[j] -= periods[j];
                                ot = levels[2] * e1;
                            }
                        }
                        let cf = e2 * cymbal_f;
                        cymbal[0] += cf * cymbal[1];
                        cymbal[1] += cf * (ot + cymbal[2] - cymbal_q * cymbal[1] - cymbal[0]);
                        cymbal[2] = ot;
                        cymbal[1]
                    }
                };
            }
            if filter == 0 {
                data[i] += ot;
            } else {
                let e = envelopes[6].at(t, false);
                let feedback = if e > 0.2 {
                    1.001 - 10.0_f64.powf(e - 1.0)
                } else {
                    0.999 - 0.7824 * e
                };
                let input = ot + if filter == 2 { data[i] } else { 0.0 };
                let temp = input + resonance * (1.0 + 1.0 / feedback) * (filter_in - filter_out);
                filter_in = feedback * (filter_in - temp) + temp;
                filter_out = feedback * (filter_out - filter_in) + filter_in;
                data[i] = filter_out - highpass * input + if filter == 1 { data[i] } else { 0.0 };
            }
        }
        if active[5] {
            for sample in &mut data {
                *sample = distortion_gain * (*sample / attenuation).trunc();
            }
            for group in data.chunks_mut(step) {
                let average = group.iter().sum::<f64>() / group.len() as f64;
                group.fill(average);
            }
        } else {
            for sample in &mut data {
                *sample *= gain;
            }
        }
        for sample in data {
            if !sample.is_finite() {
                return Err(invalid("unstable synthesis parameters"));
            }
            out.push((sample.clamp(-clip, clip).trunc() / 32768.0) as f32);
        }
    }
    let mut warnings=vec!["DrumSynth reference model rendered with deterministic portable noise; libc random sequences and floating-point rounding can differ from other renderers.".to_owned()];
    for (section, key) in p.fields.keys() {
        let known = match section.as_str() {
            "general" => [
                "version",
                "comment",
                "stretch",
                "level",
                "tuning",
                "filter",
                "highpass",
                "resonance",
                "filterenv",
            ]
            .contains(&key.as_str()),
            "tone" => {
                ["on", "level", "f1", "f2", "droop", "phase", "envelope"].contains(&key.as_str())
            }
            "noise" => ["on", "level", "slope", "fixedseq", "envelope"].contains(&key.as_str()),
            "overtones" => [
                "on",
                "level",
                "f1",
                "f2",
                "wave1",
                "wave2",
                "track1",
                "track2",
                "method",
                "param",
                "envelope1",
                "envelope2",
                "filter",
            ]
            .contains(&key.as_str()),
            "noiseband" | "noiseband2" => {
                ["on", "level", "f", "df", "envelope"].contains(&key.as_str())
            }
            "distortion" => ["on", "clipping", "bits", "rate"].contains(&key.as_str()),
            _ => false,
        };
        if !known {
            warnings.push(format!("Retained but not interpreted: {section}.{key}"));
        }
    }
    Ok(DrumSynthRendered {
        audio: AudioBuffer::from_interleaved(rate, 1, out),
        warnings,
    })
}
