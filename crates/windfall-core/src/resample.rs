//! Shared 32-tap windowed-sinc kernel used by recording and offline workers.
//! Callers supply their own bounded history and absolute sample positions.
pub fn weight(distance: f64, step: f64) -> f64 {
    let cutoff = (1.0 / step).min(1.0) * 0.94;
    let x = std::f64::consts::PI * distance * cutoff;
    let sinc = if x.abs() < 1e-9 { 1.0 } else { x.sin() / x };
    sinc * cutoff * (0.5 + 0.5 * (std::f64::consts::PI * distance / 16.0).cos()).max(0.0)
}
