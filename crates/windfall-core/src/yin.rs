//! Shared YIN trough selection, used by offline analysis and bounded live tracking.
/// Three-point interpolation. CMND selects on modeled depth; raw differences
/// refine the period, avoiding cumulative-normalization pitch bias.
pub fn parabolic_minimum(left: f64, middle: f64, right: f64) -> (f64, f64) {
    let curvature = left - 2.0 * middle + right;
    let offset = if curvature > 0.0 {
        (0.5 * (left - right) / curvature).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let depth = middle + 0.5 * offset * (right - left + offset * curvature);
    (offset, depth.max(0.0))
}
/// Search from lag two, including above-range fundamentals, so an out-of-range
/// first trough cannot be mistaken for an in-range octave/subharmonic.
pub fn first_trough(cmnd: &[f64], max_lag: usize, threshold: f64) -> Option<(usize, f64)> {
    (2..=max_lag.min(cmnd.len().saturating_sub(2))).find_map(|lag| {
        let (left, middle, right) = (cmnd[lag - 1], cmnd[lag], cmnd[lag + 1]);
        if middle < left && middle <= right {
            let depth = parabolic_minimum(left, middle, right).1;
            if depth < threshold {
                return Some((lag, depth));
            }
        }
        None
    })
}
