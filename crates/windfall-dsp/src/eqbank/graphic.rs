use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Centers of the graphic bands of [`MorphEq`](super::MorphEq): two thirds
/// of an octave apart from 25 Hz to 16 kHz. High centers are clamped below
/// Nyquist at low rates.
pub const MORPH_CURVE_FREQUENCIES_HZ: [f32; MORPH_CURVE_BANDS] = [
    25.0, 40.0, 63.0, 100.0, 160.0, 250.0, 400.0, 630.0, 1_000.0, 1_600.0, 2_500.0, 4_000.0,
    6_300.0, 10_000.0, 16_000.0,
];

/// Bands in one stored curve.
pub const MORPH_CURVE_BANDS: usize = 15;

/// Stored curves a morph can pass through.
pub const MORPH_CURVES: usize = 4;

/// Q of every graphic band: a bell two thirds of an octave wide.
pub const MORPH_CURVE_Q: f32 = 2.1;

/// One stored curve shape: a gain for each fixed band.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct MorphCurveParams {
    /// Gains in dB, -18 to 18, ordered by ascending band frequency; default
    /// zero.
    pub gains_db: [f32; MORPH_CURVE_BANDS],
}

impl Default for MorphCurveParams {
    fn default() -> Self {
        Self {
            gains_db: [0.0; MORPH_CURVE_BANDS],
        }
    }
}

/// The band gains at `morph` (0 to 1) along the first `count` curves, which
/// sit evenly spaced on that travel. Gains move in straight lines of dB
/// between neighbors, and a position on a curve gives that curve exactly.
pub(super) fn interpolated(
    curves: &[MorphCurveParams; MORPH_CURVES],
    count: u8,
    morph: f32,
) -> [f32; MORPH_CURVE_BANDS] {
    let count = usize::from(count).clamp(2, MORPH_CURVES);
    let position = morph.clamp(0.0, 1.0) * (count - 1) as f32;
    let from = (position.floor() as usize).min(count - 2);
    let share = position - from as f32;
    let (a, b) = (&curves[from].gains_db, &curves[from + 1].gains_db);
    if share <= 0.0 {
        return *a;
    }
    if share >= 1.0 {
        return *b;
    }
    std::array::from_fn(|band| a[band] + share * (b[band] - a[band]))
}
