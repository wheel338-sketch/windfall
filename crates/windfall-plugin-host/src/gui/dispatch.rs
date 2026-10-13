//! Portable cancellation fence for GUI-owner initialization. Native creation and
//! retirement are dispatched by the shell; the waiting proxy only changes intent.
use std::sync::atomic::{AtomicU8, Ordering};
#[derive(Default)]
pub struct Startup {
    phase: AtomicU8,
}
impl Startup {
    pub fn begin(&self) -> bool {
        self.phase
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    pub fn publish(&self) -> bool {
        self.phase
            .compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    pub fn cancel(&self) {
        self.phase.store(3, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.phase.load(Ordering::Acquire) == 3
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Native<'a>(&'a Cell<usize>);
    impl Drop for Native<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    #[test]
    fn cancelled_queued_gui_initialization_never_constructs_a_native_owner() {
        let startup = Startup::default();
        startup.cancel();
        let constructed = Cell::new(0);
        if startup.begin() {
            constructed.set(constructed.get() + 1);
        }
        assert_eq!(constructed.get(), 0);
        assert!(!startup.publish());
        assert!(startup.is_cancelled());
    }
    #[test]
    fn cancellation_during_gui_initialization_retires_the_unpublished_owner_once() {
        let startup = Startup::default();
        let retired = Cell::new(0);
        assert!(startup.begin());
        let native = Native(&retired);
        startup.cancel();
        assert!(!startup.publish());
        drop(native);
        assert_eq!(retired.get(), 1);
        assert!(!startup.begin());
    }
    #[test]
    fn failed_proxy_start_cancels_an_already_published_gui_owner() {
        let startup = Startup::default();
        assert!(startup.begin());
        assert!(startup.publish());
        assert!(!startup.publish());
        startup.cancel();
        assert!(startup.is_cancelled());
        assert!(!startup.begin());
    }
}
