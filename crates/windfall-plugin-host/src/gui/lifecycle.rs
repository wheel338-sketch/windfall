//! Portable editor ownership/resize/DPI policy. Native backends only observe and
//! apply window geometry; plugin calls always stay on the creating GUI thread.
use super::EditorError;
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

/// A destructor-free TLS slot, explicitly emptied by its native owners. A
/// Vec/RefCell stored directly in TLS would register a thread-exit destructor
/// that could call unmapped code after an exported host DLL is unloaded.
pub(crate) struct LocalRegistry<T> {
    slot: Cell<*mut RefCell<Vec<Weak<T>>>>,
}
impl<T> LocalRegistry<T> {
    pub const fn new() -> Self {
        Self {
            slot: Cell::new(std::ptr::null_mut()),
        }
    }
    pub fn add(&self, owner: &Rc<T>) {
        if self.slot.get().is_null() {
            self.slot
                .set(Box::into_raw(Box::new(RefCell::new(Vec::new()))));
        }
        // The slot is !Send/!Sync. Only this GUI owner accesses its stable box,
        // and retirement never runs with this borrow held.
        unsafe { &*self.slot.get() }
            .borrow_mut()
            .push(Rc::downgrade(owner));
    }
    pub fn live(&self) -> Vec<Rc<T>> {
        let pointer = self.slot.get();
        if pointer.is_null() {
            return Vec::new();
        }
        let mut owners = unsafe { &*pointer }.borrow_mut();
        owners.retain(|owner| owner.strong_count() != 0);
        owners.iter().filter_map(Weak::upgrade).collect()
    }
    pub fn retire(&self) {
        let pointer = self.slot.get();
        if pointer.is_null() {
            return;
        }
        let empty = {
            let mut owners = unsafe { &*pointer }.borrow_mut();
            owners.retain(|owner| owner.strong_count() != 0);
            owners.is_empty()
        };
        if empty {
            self.slot.set(std::ptr::null_mut());
            unsafe {
                drop(Box::from_raw(pointer));
            }
        }
    }
}

pub(crate) fn dimensions(width: u32, height: u32) -> Option<(u32, u32)> {
    ((1..=32768).contains(&width) && (1..=32768).contains(&height)).then_some((width, height))
}

#[derive(Clone, Copy)]
pub(crate) struct Observation {
    pub size: (u32, u32),
    pub monitor: f64,
    pub closed: bool,
}
pub(crate) trait Backend {
    fn observe(&self) -> Observation;
    fn resize(&self, size: (u32, u32));
}
pub(crate) struct Lifecycle {
    owner: usize,
    size: (u32, u32),
    monitor: f64,
    pending_monitor: Option<f64>,
    closing: bool,
}
impl Lifecycle {
    pub fn new(owner: usize, size: (u32, u32), monitor: f64) -> Result<Self, EditorError> {
        dimensions(size.0, size.1)
            .ok_or_else(|| EditorError::Failed("invalid editor dimensions".into()))?;
        super::scaling::content_scale(1.0, monitor)?;
        Ok(Self {
            owner,
            size,
            monitor,
            pending_monitor: None,
            closing: false,
        })
    }
    pub fn is_owner(&self, token: usize) -> bool {
        token == self.owner && !self.closing
    }
    pub fn poll(
        &mut self,
        token: usize,
        backend: &impl Backend,
        adjust: impl FnOnce(u32, u32) -> (u32, u32),
    ) -> Option<(u32, u32)> {
        if !self.is_owner(token) {
            return None;
        }
        let observation = backend.observe();
        self.closing |= observation.closed;
        if observation.monitor.is_finite()
            && observation.monitor > 0.0
            && observation.monitor != self.monitor
        {
            self.monitor = observation.monitor;
            self.pending_monitor = Some(self.monitor);
        }
        if self.closing || observation.size == self.size {
            return None;
        }
        let candidate = adjust(observation.size.0, observation.size.1);
        let accepted = dimensions(candidate.0, candidate.1).unwrap_or(self.size);
        if accepted != observation.size {
            backend.resize(accepted);
        }
        if accepted == self.size {
            return None;
        }
        self.size = accepted;
        Some(accepted)
    }
    pub fn resize(&mut self, token: usize, backend: &impl Backend, size: (u32, u32)) {
        if self.is_owner(token) && dimensions(size.0, size.1).is_some() {
            self.size = size;
            backend.resize(size);
        }
    }
    pub fn monitor(&self) -> f64 {
        self.monitor
    }
    pub fn take_monitor(&mut self) -> Option<f64> {
        self.pending_monitor.take()
    }
    pub fn closing(&self) -> bool {
        self.closing
    }
}

/// Native plugin teardown precedes native parent retirement. Closures keep
/// format/API specifics in the platform adapters.
pub(crate) fn close_plugin_then_parent(
    hide: impl FnOnce(),
    destroy: impl FnOnce(),
    parent: impl FnOnce(),
) {
    hide();
    destroy();
    parent();
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    struct Fake {
        observation: Cell<Observation>,
        applied: RefCell<Vec<(u32, u32)>>,
    }
    impl Backend for Fake {
        fn observe(&self) -> Observation {
            self.observation.get()
        }
        fn resize(&self, size: (u32, u32)) {
            self.applied.borrow_mut().push(size);
        }
    }
    fn fake() -> Fake {
        Fake {
            observation: Cell::new(Observation {
                size: (640, 480),
                monitor: 1.0,
                closed: false,
            }),
            applied: RefCell::new(Vec::new()),
        }
    }
    #[test]
    fn wrong_owner_and_closed_editor_never_call_native_resize() {
        let backend = fake();
        let mut state = Lifecycle::new(7, (640, 480), 1.0).unwrap();
        state.resize(8, &backend, (800, 600));
        assert_eq!(state.poll(8, &backend, |_, _| panic!("wrong owner")), None);
        backend.observation.set(Observation {
            closed: true,
            ..backend.observe()
        });
        assert_eq!(state.poll(7, &backend, |_, _| panic!("closed")), None);
        state.resize(7, &backend, (800, 600));
        assert!(state.closing());
        assert!(backend.applied.borrow().is_empty());
    }
    #[test]
    fn constrained_resize_and_invalid_plugin_size_preserve_last_geometry() {
        let backend = fake();
        let mut state = Lifecycle::new(7, (640, 480), 1.0).unwrap();
        backend.observation.set(Observation {
            size: (803, 603),
            ..backend.observe()
        });
        assert_eq!(
            state.poll(7, &backend, |w, h| (w / 10 * 10, h / 10 * 10)),
            Some((800, 600))
        );
        assert_eq!(&*backend.applied.borrow(), &[(800, 600)]);
        backend.observation.set(Observation {
            size: (900, 700),
            ..backend.observe()
        });
        assert_eq!(state.poll(7, &backend, |_, _| (u32::MAX, 0)), None);
        assert_eq!(&*backend.applied.borrow(), &[(800, 600), (800, 600)]);
    }
    #[test]
    fn monitor_changes_coalesce_once_without_application_double_scaling() {
        let backend = fake();
        let mut state = Lifecycle::new(7, (640, 480), 1.0).unwrap();
        for monitor in [1.25, 2.0, f64::NAN] {
            backend.observation.set(Observation {
                monitor,
                ..backend.observe()
            });
            state.poll(7, &backend, |w, h| (w, h));
        }
        assert_eq!(state.take_monitor(), Some(2.0));
        assert_eq!(state.take_monitor(), None);
        assert_eq!(
            super::super::scaling::content_scale(1.25, state.monitor()).unwrap(),
            2.5
        );
    }
    #[test]
    fn plugin_teardown_precedes_parent_and_last_view_reference_retirement() {
        let trace = RefCell::new(Vec::new());
        close_plugin_then_parent(
            || trace.borrow_mut().push("hide"),
            || trace.borrow_mut().push("destroy"),
            || trace.borrow_mut().push("parent"),
        );
        assert_eq!(&*trace.borrow(), &["hide", "destroy", "parent"]);
        trace.borrow_mut().clear();
        close_plugin_then_parent(
            || trace.borrow_mut().push("removed"),
            || trace.borrow_mut().extend(["clear-frame", "clear-run-loop"]),
            || trace.borrow_mut().extend(["view-reference", "parent"]),
        );
        assert_eq!(
            &*trace.borrow(),
            &[
                "removed",
                "clear-frame",
                "clear-run-loop",
                "view-reference",
                "parent"
            ]
        );
    }
    #[test]
    fn owner_registry_reclaims_its_tls_storage_before_library_unload_and_can_reopen() {
        struct Native {
            registry: Rc<LocalRegistry<Native>>,
            retired: Rc<Cell<usize>>,
        }
        impl Drop for Native {
            fn drop(&mut self) {
                self.retired.set(self.retired.get() + 1);
                self.registry.retire();
            }
        }
        assert!(!std::mem::needs_drop::<LocalRegistry<Native>>());
        let registry = Rc::new(LocalRegistry::new());
        let retired = Rc::new(Cell::new(0));
        for expected in [1, 2] {
            let native = Rc::new(Native {
                registry: registry.clone(),
                retired: retired.clone(),
            });
            registry.add(&native);
            let held_by_pump = registry.live();
            drop(native);
            assert_eq!(retired.get(), expected - 1);
            drop(held_by_pump);
            assert_eq!(retired.get(), expected);
            assert!(registry.slot.get().is_null());
            assert!(registry.live().is_empty());
        }
    }
}
