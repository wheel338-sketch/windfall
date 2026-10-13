//! Application zoom is distinct from native monitor DPI. Combine exactly once
//! for physical native APIs (never use webview devicePixelRatio here). CLAP's
//! Cocoa API uses logical points and an application-only NSView transform.
use super::EditorError;

pub(crate) fn content_scale(application: f64, monitor: f64) -> Result<f64, EditorError> {
    if !application.is_finite()
        || !(0.75..=2.0).contains(&application)
        || !monitor.is_finite()
        || monitor <= 0.0
    {
        return Err(EditorError::InvalidScale);
    }
    Ok(application * monitor)
}

#[cfg(any(test, target_os = "linux"))]
pub(crate) fn physical_monitor_scale(pixels: u32, millimeters: u64) -> Option<f64> {
    if pixels == 0 || millimeters == 0 {
        return None;
    }
    let scale = f64::from(pixels) * 25.4 / millimeters as f64 / 96.0;
    scale.is_finite().then_some(scale.clamp(0.5, 8.0))
}
#[cfg(any(test, target_os = "linux"))]
pub(crate) fn xft_scale(resources: &str) -> Option<f64> {
    resources.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim() != "Xft.dpi" {
            return None;
        }
        let dpi = value.trim().parse::<f64>().ok()?;
        (dpi.is_finite() && dpi > 0.0).then_some((dpi / 96.0).clamp(0.5, 8.0))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn application_and_monitor_scaling_are_combined_once() {
        assert_eq!(content_scale(1.25, 1.5).unwrap(), 1.875);
        assert_eq!(content_scale(1.0, 2.0).unwrap(), 2.0);
        for factor in [f64::NAN, f64::INFINITY, 0.0, 0.74, 2.01] {
            assert_eq!(content_scale(factor, 1.0), Err(EditorError::InvalidScale));
        }
        assert_eq!(content_scale(1.0, 0.0), Err(EditorError::InvalidScale));
    }
    #[test]
    fn x11_physical_density_and_user_override_have_checked_independent_units() {
        assert_eq!(physical_monitor_scale(1920, 508), Some(1.0));
        assert_eq!(physical_monitor_scale(3840, 508), Some(2.0));
        assert_eq!(physical_monitor_scale(1920, 0), None);
        assert_eq!(physical_monitor_scale(0, 508), None);
        assert_eq!(xft_scale("Other: 42\nXft.dpi: 144\n"), Some(1.5));
        assert_eq!(xft_scale("Xft.dpi: NaN"), None);
        assert_eq!(xft_scale("Xft.dpi: -96"), None);
    }
}
