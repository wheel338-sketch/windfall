//! AU views use logical AppKit points. Only application zoom transforms the
//! host parent; AppKit independently chooses backing pixels on each monitor.
//! Saved wrapper zoom is the base; live application zoom multiplies it once.
use super::{
    EditorError,
    lifecycle::{Observation, dimensions},
};

pub(crate) struct Geometry {
    logical: (u32, u32),
    wrapper: f64,
    application: f64,
    monitor: f64,
    closed: bool,
}
impl Geometry {
    pub fn new(logical: (u32, u32), wrapper: f64, monitor: f64) -> Result<Self, EditorError> {
        dimensions(logical.0, logical.1)
            .ok_or_else(|| EditorError::Failed("invalid AU editor dimensions".into()))?;
        super::EditorOptions {
            scale: wrapper,
            ..Default::default()
        }
        .validate()?;
        if !monitor.is_finite() || monitor <= 0.0 {
            return Err(EditorError::InvalidScale);
        }
        Ok(Self {
            logical,
            wrapper,
            application: 1.0,
            monitor,
            closed: false,
        })
    }
    pub fn logical(&self) -> (u32, u32) {
        self.logical
    }
    pub fn application(&self) -> f64 {
        self.wrapper * self.application
    }
    pub fn closed(&self) -> bool {
        self.closed
    }
    pub fn set_scale(&mut self, application: f64) -> Result<(), EditorError> {
        super::scaling::content_scale(application, 1.0)?;
        if self.closed {
            return Err(EditorError::NoEditor);
        }
        self.application = application;
        Ok(())
    }
    /// A view changing its own frame requests a new logical size. Otherwise a
    /// user resize of the host parent wins. Neither is multiplied by density.
    pub fn observe(
        &mut self,
        window: Observation,
        plugin: (u32, u32),
    ) -> Result<bool, EditorError> {
        if self.closed {
            return Ok(false);
        }
        self.closed = window.closed;
        if self.closed {
            return Ok(false);
        }
        let size = if plugin != self.logical {
            plugin
        } else {
            window.size
        };
        dimensions(size.0, size.1)
            .ok_or_else(|| EditorError::Failed("invalid AU editor resize".into()))?;
        let monitor = if window.monitor.is_finite() && window.monitor > 0.0 {
            window.monitor
        } else {
            self.monitor
        };
        let changed = size != self.logical || monitor != self.monitor;
        self.logical = size;
        self.monitor = monitor;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn observation(size: (u32, u32), monitor: f64) -> Observation {
        Observation {
            size,
            monitor,
            closed: false,
        }
    }
    fn points(state: &Geometry) -> (f64, f64) {
        (
            f64::from(state.logical().0) * state.application(),
            f64::from(state.logical().1) * state.application(),
        )
    }
    #[test]
    fn audio_unit_nondefault_wrapper_open_uses_application_points_once() {
        for scale in [0.5, 1.25, 1.5, 2.0, 4.0] {
            let mut state = Geometry::new((640, 480), scale, 2.0).unwrap();
            // Runtime sends its default global preference immediately on open.
            state.set_scale(1.0).unwrap();
            assert_eq!(state.logical(), (640, 480));
            assert_eq!(points(&state), (640.0 * scale, 480.0 * scale));
            assert_eq!(state.monitor, 2.0);
        }
    }
    #[test]
    fn audio_unit_live_zoom_preserves_logical_size_and_rejects_invalid_scale_atomically() {
        let mut state = Geometry::new((640, 480), 1.25, 2.0).unwrap();
        for scale in [1.5, 2.0, 1.0, 1.25] {
            state.set_scale(scale).unwrap();
            assert_eq!(points(&state), (640.0 * 1.25 * scale, 480.0 * 1.25 * scale));
            assert_eq!(state.logical(), (640, 480));
        }
        for scale in [f64::NAN, f64::INFINITY, 0.49, 4.01] {
            assert!(state.set_scale(scale).is_err());
            assert_eq!(points(&state), (1000.0, 750.0));
        }
    }
    #[test]
    fn audio_unit_monitor_transitions_change_backing_density_without_double_zoom() {
        let mut state = Geometry::new((640, 480), 1.5, 1.0).unwrap();
        for monitor in [2.0, 1.0, 1.5] {
            assert!(
                state
                    .observe(observation((640, 480), monitor), (640, 480))
                    .unwrap()
            );
            assert_eq!(points(&state), (960.0, 720.0));
            let backing = (
                points(&state).0 * state.monitor,
                points(&state).1 * state.monitor,
            );
            assert_eq!(backing, (960.0 * monitor, 720.0 * monitor));
            assert!(
                !state
                    .observe(observation((640, 480), monitor), (640, 480))
                    .unwrap()
            );
        }
        assert!(
            !state
                .observe(observation((640, 480), f64::NAN), (640, 480))
                .unwrap()
        );
        assert_eq!(state.monitor, 1.5);
        assert_eq!(points(&state), (960.0, 720.0));
    }
    #[test]
    fn audio_unit_plugin_and_host_resize_preserve_zoom_and_close_fences_geometry() {
        let mut state = Geometry::new((640, 480), 1.5, 2.0).unwrap();
        assert!(
            state
                .observe(observation((640, 480), 2.0), (800, 600))
                .unwrap()
        );
        assert_eq!(points(&state), (1200.0, 900.0));
        assert!(
            state
                .observe(observation((900, 700), 2.0), (800, 600))
                .unwrap()
        );
        assert_eq!(points(&state), (1350.0, 1050.0));
        assert!(
            state
                .observe(observation((900, 700), 2.0), (0, u32::MAX))
                .is_err()
        );
        assert_eq!(state.logical(), (900, 700));
        state
            .observe(
                Observation {
                    closed: true,
                    ..observation((1, 1), 1.0)
                },
                (1, 1),
            )
            .unwrap();
        assert!(state.closed());
        assert_eq!(state.set_scale(2.0), Err(EditorError::NoEditor));
        assert!(!state.observe(observation((1, 1), 1.0), (1, 1)).unwrap());
        assert_eq!(state.logical(), (900, 700));
    }
}
