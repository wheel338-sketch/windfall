//! Process boundary policy: Cocoa object pointers are never transferable.
//! X11 ids name server resources and may be used as transient owners remotely.
use super::EditorOptions;
pub(crate) fn helper_options(mut options: EditorOptions, cocoa: bool) -> EditorOptions {
    if cocoa {
        options.owner_window = None;
        options.detached = true;
    }
    options
}
#[derive(Default)]
pub(crate) struct Refusals {
    latest: Option<[f64; 2]>,
}
impl Refusals {
    pub fn record(&mut self, application: f64, monitor: f64) {
        self.latest = Some([application, monitor]);
    }
    pub fn take(&mut self) -> Option<[f64; 2]> {
        self.latest.take()
    }
    pub fn clear(&mut self) {
        self.latest = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_cocoa_parent_is_local_and_x11_server_id_survives() {
        let options = EditorOptions {
            detached: false,
            owner_window: Some(123),
            scale: 1.25,
            ..Default::default()
        };
        let cocoa = helper_options(options.clone(), true);
        assert_eq!(cocoa.owner_window, None);
        assert!(cocoa.detached);
        assert_eq!(cocoa.scale, 1.25);
        assert!(cocoa.validate().is_ok());
        assert_eq!(helper_options(options.clone(), false), options);
    }
    #[test]
    fn helper_monitor_refusals_are_reported_once_and_close_clears_stale_errors() {
        let mut report = Refusals::default();
        report.record(1.25, 1.5);
        report.record(1.25, 2.0);
        assert_eq!(report.take(), Some([1.25, 2.0]));
        assert_eq!(report.take(), None);
        report.record(1.5, 2.0);
        report.clear();
        assert_eq!(report.take(), None);
    }
}
