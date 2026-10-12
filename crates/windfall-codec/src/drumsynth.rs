//! Bounded DrumSynth text presets, from LMMS's public DrumSynth.cpp (GPL-2+).
//! Retains unknown fields. Rendering is explicitly a subset: tone and flat
//! white noise. Active unsupported synthesis is refused rather than discarded.
use crate::CodecError;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use windfall_core::AudioBuffer;

pub const MAX_DRUMSYNTH_PRESET_BYTES: usize = 64 * 1024;
pub const MAX_DRUMSYNTH_FRAMES: usize = 3_840_000;
/// Preset sources always reopen at the same authored rate and noise seed.
pub const DRUMSYNTH_SOURCE_RATE: u32 = 44_100;
pub const DRUMSYNTH_SOURCE_SEED: u64 = 7;

pub fn render_drumsynth_source(
    bytes: &[u8],
    options: &crate::DecodeOptions,
) -> Result<DrumSynthRendered, CodecError> {
    if bytes.is_empty() {
        return Err(CodecError::NoAudio);
    }
    let frames = (options.max_decoded_bytes / 4).min(MAX_DRUMSYNTH_FRAMES as u64) as usize;
    if frames == 0 {
        return Err(CodecError::TooLarge {
            limit_bytes: options.max_decoded_bytes,
        });
    }
    DrumSynthPreset::parse(bytes)?.render(DRUMSYNTH_SOURCE_RATE, frames, DRUMSYNTH_SOURCE_SEED)
}

/// Reads at most the text budget plus one byte, even if the file grows after
/// its initial metadata check. Original text remains the persisted source.
pub fn render_drumsynth_file(
    path: impl AsRef<Path>,
    options: &crate::DecodeOptions,
) -> Result<DrumSynthRendered, CodecError> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_DRUMSYNTH_PRESET_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    render_drumsynth_source(&bytes, options)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrumSynthPreset {
    pub fields: BTreeMap<(String, String), String>,
}
#[derive(Debug)]
pub struct DrumSynthRendered {
    pub audio: AudioBuffer,
    pub warnings: Vec<String>,
}
fn invalid(message: &str) -> CodecError {
    CodecError::Corrupt(format!("DrumSynth preset: {message}"))
}
impl DrumSynthPreset {
    pub fn parse(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() > MAX_DRUMSYNTH_PRESET_BYTES {
            return Err(invalid("64 KiB input budget exceeded"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| invalid("expected ASCII/UTF-8 text"))?;
        if text.contains('\0') {
            return Err(invalid("NUL in text"));
        }
        let mut fields = BTreeMap::new();
        let mut section = String::new();
        let mut sections = 0;
        for line in text.lines() {
            if line.len() > 2048 {
                return Err(invalid("line exceeds 2048 bytes"));
            }
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                let name = line
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .ok_or_else(|| invalid("malformed section"))?;
                sections += 1;
                if sections > 64 || name.is_empty() || name.len() > 64 {
                    return Err(invalid("section budget exceeded"));
                }
                section = name.to_ascii_lowercase();
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| invalid("expected key=value"))?;
            let key = key.trim().to_ascii_lowercase();
            if section.is_empty() || key.is_empty() || key.len() > 64 || fields.len() >= 512 {
                return Err(invalid("invalid key or field budget exceeded"));
            }
            if fields
                .insert((section.clone(), key), value.trim().to_owned())
                .is_some()
            {
                return Err(invalid("duplicate key"));
            }
        }
        let preset = Self { fields };
        let version = preset
            .get("general", "version")
            .ok_or_else(|| invalid("missing version"))?
            .to_ascii_lowercase();
        if !version.starts_with("drumsynth v1.") && !version.starts_with("drumsynth v2.") {
            return Err(CodecError::UnsupportedFormat(
                "DrumSynth version (only v1/v2 text presets)".into(),
            ));
        }
        Ok(preset)
    }
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.fields
            .get(&(section.to_ascii_lowercase(), key.to_ascii_lowercase()))
            .map(String::as_str)
    }
    fn number(
        &self,
        section: &str,
        key: &str,
        default: f64,
        min: f64,
        max: f64,
    ) -> Result<f64, CodecError> {
        let Some(text) = self.get(section, key) else {
            return Ok(default);
        };
        let n = text.parse::<f64>().map_err(|_| invalid("invalid number"))?;
        if !n.is_finite() || !(min..=max).contains(&n) {
            return Err(invalid("nonfinite or out-of-range number"));
        }
        Ok(n)
    }
    /// Offline worker rendering only. Seeded white noise is deterministic but
    /// does not reproduce the source product's random sequence. No exact sound
    /// equivalence claim, including its integer rounding and tail padding.
    pub fn render(
        &self,
        rate: u32,
        max_frames: usize,
        seed: u64,
    ) -> Result<DrumSynthRendered, CodecError> {
        if !(8000..=192000).contains(&rate) || max_frames == 0 || max_frames > MAX_DRUMSYNTH_FRAMES
        {
            return Err(invalid("invalid render rate or frame budget"));
        }
        for (section, key) in self.fields.keys() {
            if key == "on"
                && !["tone", "noise"].contains(&section.as_str())
                && self.number(section, "on", 0.0, 0.0, 1.0)? != 0.0
            {
                return Err(CodecError::UnsupportedFormat(format!(
                    "active DrumSynth {section} is not rendered"
                )));
            }
        }
        for (section, key) in [
            ("general", "filter"),
            ("general", "highpass"),
            ("noise", "slope"),
            ("tone", "droop"),
        ] {
            if self.number(section, key, 0.0, -10000.0, 10000.0)? != 0.0 {
                return Err(CodecError::UnsupportedFormat(format!(
                    "DrumSynth {section}.{key} is not rendered"
                )));
            }
        }
        let on = |section| -> Result<bool, CodecError> {
            let flag = self.number(section, "on", 0.0, 0.0, 1.0)?;
            if flag != 0.0 && flag != 1.0 {
                return Err(invalid("On must be zero or one"));
            }
            Ok(flag == 1.0)
        };
        let tone = on("tone")?;
        let noise = on("noise")?;
        if !tone && !noise {
            return Err(CodecError::NoAudio);
        }
        let stretch = self.number("general", "stretch", 100.0, 20.0, 1000.0)? / 100.0
            * f64::from(rate)
            / 44100.0;
        let gain = 10.0_f64.powf(self.number("general", "level", 0.0, -96.0, 24.0)? / 20.0);
        let tune = 2.0_f64.powf(self.number("general", "tuning", 0.0, -96.0, 96.0)? / 12.0);
        let tenv = self.envelope("tone", stretch)?;
        let nenv = self.envelope("noise", stretch)?;
        let length = if tone {
            tenv.last().expect("validated envelope").0
        } else {
            0.0
        }
        .max(if noise {
            nenv.last().expect("validated envelope").0
        } else {
            0.0
        })
        .ceil() as usize;
        if length == 0 {
            return Err(CodecError::NoAudio);
        }
        if length > max_frames {
            return Err(CodecError::TooLarge {
                limit_bytes: max_frames as u64 * 4,
            });
        }
        let f1 = self.number("tone", "f1", 200.0, 0.0, 20000.0)? * tune;
        let f2 = self.number("tone", "f2", 120.0, 0.0, 20000.0)? * tune;
        if tone && f1.max(f2) >= f64::from(rate) / 2.0 {
            return Err(invalid("tone exceeds Nyquist at requested sample rate"));
        }
        let tl = self.number("tone", "level", 128.0, 0.0, 256.0)?.powi(2) / 32768.0;
        let nl = self.number("noise", "level", 0.0, 0.0, 256.0)?.powi(2) / 32768.0;
        let mut phase = self
            .number("tone", "phase", 90.0, -360.0, 360.0)?
            .to_radians();
        let mut state = seed;
        let mut out = Vec::with_capacity(length);
        let tone_end = tenv.last().expect("validated envelope").0;
        for frame in 0..length {
            let t = frame as f64;
            let mut sample = 0.0;
            if tone && t < tone_end {
                let frequency = f1 + (f2 - f1) * t / tone_end;
                phase += std::f64::consts::TAU * frequency / f64::from(rate);
                sample += tl * env(&tenv, t) * phase.sin();
            }
            if noise {
                state = state.wrapping_add(0x9e3779b97f4a7c15);
                let mut bits = state;
                bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
                bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d049bb133111eb);
                bits ^= bits >> 31;
                sample += nl * env(&nenv, t) * ((bits >> 40) as f64 / 8388608.0 - 1.0);
            }
            out.push((sample * gain).clamp(-1.0, 1.0) as f32);
        }
        let mut warnings=vec!["Tone/flat-noise subset rendered approximately; source integer rounding, random noise and tail padding differ.".to_owned()];
        for (section, key) in self.fields.keys() {
            let mapped = match section.as_str() {
                "general" => [
                    "version", "comment", "stretch", "level", "tuning", "filter", "highpass",
                ]
                .contains(&key.as_str()),
                "tone" => ["on", "level", "f1", "f2", "droop", "phase", "envelope"]
                    .contains(&key.as_str()),
                "noise" => ["on", "level", "slope", "envelope"].contains(&key.as_str()),
                _ => key == "on",
            };
            if !mapped {
                warnings.push(format!("Retained but not interpreted: {section}.{key}"));
            }
        }
        Ok(DrumSynthRendered {
            audio: AudioBuffer::from_interleaved(rate, 1, out),
            warnings,
        })
    }
    fn envelope(&self, section: &str, stretch: f64) -> Result<Vec<(f64, f64)>, CodecError> {
        let text = self.get(section, "envelope").unwrap_or("0,0 100,0");
        let mut points = Vec::new();
        for token in text.split_whitespace() {
            if points.len() >= 32 {
                return Err(invalid("envelope exceeds 32 points"));
            }
            let (time, level) = token
                .split_once(',')
                .ok_or_else(|| invalid("envelope needs time,level pairs"))?;
            let time = time
                .parse::<f64>()
                .map_err(|_| invalid("invalid envelope time"))?;
            let level = level
                .parse::<f64>()
                .map_err(|_| invalid("invalid envelope level"))?;
            if !time.is_finite()
                || !level.is_finite()
                || !(0.0..=441000.0).contains(&time)
                || !(0.0..=100.0).contains(&level)
                || points
                    .last()
                    .is_some_and(|&(prior, _)| time * stretch <= prior)
            {
                return Err(invalid(
                    "envelope times must increase; levels must be 0..100",
                ));
            }
            points.push((time * stretch, level / 100.0));
        }
        if points.len() < 2 || points[0].0 != 0.0 {
            return Err(invalid("envelope needs two points starting at zero"));
        }
        Ok(points)
    }
}
fn env(points: &[(f64, f64)], frame: f64) -> f64 {
    let Some(pair) = points
        .windows(2)
        .find(|p| frame >= p[0].0 && frame < p[1].0)
    else {
        return 0.0;
    };
    pair[0].1 + (pair[1].1 - pair[0].1) * (frame - pair[0].0) / (pair[1].0 - pair[0].0)
}
