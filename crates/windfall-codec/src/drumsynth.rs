//! Bounded DrumSynth text presets and the complete offline synthesis model.
//! Port of Paul Kellett's DrumSynth 2.0 (GPL-2.0-or-later), with sample-rate
//! scaling from LMMS. See NOTICE-drumsynth.md for pinned sources and attribution.
#[path = "drumsynth_model.rs"]
mod model;
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
        // Kellett's INI reader accepts byte-valued comments. Numeric keys and
        // values remain ASCII; lossily decoded metadata cannot change audio.
        // The import owner preserves the exact original bytes separately.
        let text = String::from_utf8_lossy(bytes);
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
    /// Offline deterministic rendering. FixedSeq selects seed 1 for every noise
    /// generator; otherwise the caller's seed is used. Never touches a device.
    pub fn render(
        &self,
        rate: u32,
        max_frames: usize,
        seed: u64,
    ) -> Result<DrumSynthRendered, CodecError> {
        model::render(self, rate, max_frames, seed)
    }
    fn envelope(
        &self,
        section: &str,
        key: &str,
        stretch: f64,
    ) -> Result<Vec<(f64, f64)>, CodecError> {
        let text = self.get(section, key).unwrap_or("0,0 100,0");
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
                || !(0.0..=442000.0).contains(&time)
                || !(0.0..=100.0).contains(&level)
                || points
                    .last()
                    .is_some_and(|&(prior, _)| time * stretch < prior)
            {
                return Err(invalid(
                    "envelope times must not decrease; levels must be 0..100",
                ));
            }
            points.push((time * stretch, level / 100.0));
        }
        if points.len() < 2 || points[0].0 != 0.0 || points.last().unwrap().0 == 0.0 {
            return Err(invalid("envelope needs two points starting at zero"));
        }
        Ok(points)
    }
}
