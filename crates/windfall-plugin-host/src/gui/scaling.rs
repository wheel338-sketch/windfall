//! Application zoom is distinct from native monitor DPI. Combine exactly once
//! for Windows native plugin APIs (never use webview devicePixelRatio here).
use super::EditorError;

pub(crate) fn content_scale(application: f64, monitor: f64) -> Result<f64, EditorError> {
    if !application.is_finite() || !(0.75..=2.0).contains(&application)
        || !monitor.is_finite() || monitor <= 0.0
    {
        return Err(EditorError::InvalidScale);
    }
    Ok(application * monitor)
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
}
