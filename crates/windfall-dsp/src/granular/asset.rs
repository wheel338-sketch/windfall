//! Immutable source/marker/curve data prepared on the control thread.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_core::AudioBuffer;

pub const MAX_ASSET_SLICES: usize = 128;
pub const MAX_CURVE_POINTS: usize = 256;
pub const MAX_SOURCE_FRAMES: usize = 32_000_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct SliceRegion {
    /// Normalized source boundaries, exclusive end. Regions may overlap.
    pub start: f32,
    pub end: f32,
    pub gain: f32,
    pub pitch: f32,
    pub reverse: bool,
    pub attack_ms: f32,
    pub release_ms: f32,
    /// Zero selects A; one selects B. Keys follow vector order.
    pub deck: u8,
}
impl Default for SliceRegion {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: 1.0,
            gain: 1.0,
            pitch: 0.0,
            reverse: false,
            attack_ms: 1.0,
            release_ms: 4.0,
            deck: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlaybackPoint {
    pub time: f32,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PlaybackCurveMode {
    #[default]
    Position,
    Speed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PlaybackCurve {
    pub mode: PlaybackCurveMode,
    /// Speed curves integrate from this normalized position.
    pub origin: f32,
    pub points: Vec<PlaybackPoint>,
}
impl Default for PlaybackCurve {
    fn default() -> Self {
        Self {
            mode: PlaybackCurveMode::Position,
            origin: 0.0,
            points: vec![
                PlaybackPoint {
                    time: 0.0,
                    value: 0.0,
                },
                PlaybackPoint {
                    time: 1.0,
                    value: 1.0,
                },
            ],
        }
    }
}
impl PlaybackCurve {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.origin.is_finite()
            || !(0.0..=1.0).contains(&self.origin)
            || !(2..=MAX_CURVE_POINTS).contains(&self.points.len())
            || self.points[0].time != 0.0
            || self.points.last().unwrap().time != 1.0
        {
            return Err("A curve needs 2–256 ordered points spanning 0 to 1.");
        }
        let range = match self.mode {
            PlaybackCurveMode::Position => 0.0..=1.0,
            PlaybackCurveMode::Speed => -8.0..=8.0,
        };
        if self
            .points
            .iter()
            .any(|p| !p.time.is_finite() || !p.value.is_finite() || !range.contains(&p.value))
            || self.points.windows(2).any(|p| p[0].time >= p[1].time)
        {
            return Err(
                "Curve points must be finite and strictly ordered, with valid position/speed values.",
            );
        }
        Ok(())
    }
    /// Independent of block size: speed is analytically integrated over each
    /// linear segment, rather than accumulating callback-sized increments.
    pub fn position(&self, time: f64) -> f64 {
        let time = time.clamp(0.0, 1.0);
        let mut integral = f64::from(self.origin);
        for pair in self.points.windows(2) {
            let a = pair[0];
            let b = pair[1];
            let width = f64::from(b.time - a.time);
            let elapsed = (time - f64::from(a.time)).clamp(0.0, width);
            let slope = f64::from(b.value - a.value) / width;
            if self.mode == PlaybackCurveMode::Speed {
                integral += f64::from(a.value) * elapsed + slope * elapsed * elapsed * 0.5;
                if time <= f64::from(b.time) {
                    return integral.clamp(0.0, 1.0);
                }
            } else if time <= f64::from(b.time) {
                return (f64::from(a.value) + slope * elapsed).clamp(0.0, 1.0);
            }
        }
        integral.clamp(0.0, 1.0)
    }
}

pub fn validate_slices(slices: &[SliceRegion]) -> Result<(), &'static str> {
    if slices.is_empty() || slices.len() > MAX_ASSET_SLICES {
        return Err("Choose 1–128 slices.");
    }
    for s in slices {
        if !s.start.is_finite()
            || !s.end.is_finite()
            || s.start < 0.0
            || s.end > 1.0
            || s.start >= s.end
            || !s.gain.is_finite()
            || !(0.0..=2.0).contains(&s.gain)
            || !s.pitch.is_finite()
            || !(-48.0..=48.0).contains(&s.pitch)
            || !s.attack_ms.is_finite()
            || !(0.0..=1000.0).contains(&s.attack_ms)
            || !s.release_ms.is_finite()
            || !(1.0..=1000.0).contains(&s.release_ms)
            || s.deck > 1
        {
            return Err("Invalid slice boundaries, deck, gain, pitch or envelope.");
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct PlaybackAsset {
    pub(crate) sources: [Option<AudioBuffer>; 2],
    pub(crate) slices: Box<[SliceRegion]>,
    pub(crate) curve: PlaybackCurve,
    /// Optional per-key curves, sparse and immutable. Missing keys use `curve`.
    pub(crate) key_curves: Box<[(u8, PlaybackCurve)]>,
}
impl PlaybackAsset {
    /// Validates without truncation or sanitizing authored data. Missing decoded
    /// sources are retained as silent decks; the UI reports loading failures.
    pub fn new(
        sources: [Option<AudioBuffer>; 2],
        slices: Vec<SliceRegion>,
        curve: PlaybackCurve,
        key_curves: Vec<(u8, PlaybackCurve)>,
    ) -> Result<Self, &'static str> {
        validate_slices(&slices)?;
        curve.validate()?;
        if key_curves.len() > 128 {
            return Err("Too many key curves.");
        }
        let mut keys = [false; 128];
        for (key, curve) in &key_curves {
            if *key > 127 || keys[*key as usize] {
                return Err("Repeated or invalid curve key.");
            }
            keys[*key as usize] = true;
            curve.validate()?;
        }
        for audio in sources.iter().flatten() {
            if audio.frames() == 0
                || audio.frames() > MAX_SOURCE_FRAMES
                || audio.channels() == 0
                || audio.samples().iter().any(|value| !value.is_finite())
            {
                return Err("Source must contain 1–32 million finite frames.");
            }
        }
        Ok(Self {
            sources,
            slices: slices.into_boxed_slice(),
            curve,
            key_curves: key_curves.into_boxed_slice(),
        })
    }
    pub(crate) fn curve(&self, key: u8) -> &PlaybackCurve {
        self.key_curves
            .iter()
            .find(|(k, _)| *k == key)
            .map_or(&self.curve, |(_, c)| c)
    }
}
