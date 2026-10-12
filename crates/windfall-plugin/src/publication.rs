//! Single control publisher / single callback consumer. A prepared engine is
//! moved at a block boundary; the replaced box is reclaimed only by control.
use crate::{GuestEngine, Program};
use std::cell::UnsafeCell;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

pub(crate) struct Publication {
    main: UnsafeCell<Main>,
    audio: UnsafeCell<Option<GuestEngine>>,
    pending: AtomicPtr<GuestEngine>,
    retired: AtomicPtr<GuestEngine>,
    applied: AtomicBool,
    rate: AtomicU32,
    latency: AtomicU32,
}
struct Main {
    committed: Program,
    pending: Option<Program>,
}
impl Default for Publication {
    fn default() -> Self {
        Self::new(Program::default())
    }
}
impl Publication {
    pub fn new(program: Program) -> Self {
        Self {
            main: UnsafeCell::new(Main {
                committed: program,
                pending: None,
            }),
            audio: UnsafeCell::new(None),
            pending: AtomicPtr::new(ptr::null_mut()),
            retired: AtomicPtr::new(ptr::null_mut()),
            applied: AtomicBool::new(false),
            rate: AtomicU32::new(0),
            latency: AtomicU32::new(0),
        }
    }
    // Main-thread only. The acquire pairs with callback acknowledgement AFTER
    // the complete processor swap. Capture observes either full old or full new.
    pub fn capture(&self) -> Result<Vec<u8>, String> {
        self.collect();
        unsafe { (&*self.main.get()).committed.encode() }
    }
    pub fn collect(&self) {
        if self.applied.swap(false, Ordering::Acquire) {
            let main = unsafe { &mut *self.main.get() };
            if let Some(program) = main.pending.take() {
                main.committed = program;
            }
            let retired = self.retired.swap(ptr::null_mut(), Ordering::AcqRel);
            if !retired.is_null() {
                unsafe {
                    drop(Box::from_raw(retired));
                }
            }
        }
    }
    pub fn load(&self, program: Program) -> Result<(), String> {
        program.validate()?;
        self.collect();
        let main = unsafe { &mut *self.main.get() };
        if main.pending.is_some() {
            return Err("Previous program awaits an audio boundary".into());
        }
        let rate = self.rate.load(Ordering::Acquire);
        if rate == 0 {
            main.committed = program;
            return Ok(());
        }
        let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            GuestEngine::activate(&program, f64::from(rate))
        }))
        .map_err(|_| "Program preparation panicked; previous program retained".to_owned())?;
        let engine = Box::new(prepared?);
        main.pending = Some(program);
        self.pending.store(Box::into_raw(engine), Ordering::Release);
        Ok(())
    }
    pub fn activate(&self, rate: f64) -> Result<u32, String> {
        self.collect();
        let engine = GuestEngine::activate(unsafe { &(&*self.main.get()).committed }, rate)?;
        let latency = engine.latency();
        self.latency.store(latency, Ordering::Release);
        unsafe {
            *self.audio.get() = Some(engine);
        }
        self.rate.store(rate as u32, Ordering::Release);
        Ok(latency)
    }
    /// Exclusive callback only. No Box construction/destruction or controller
    /// operation; the one occupied retirement slot blocks the next publication.
    #[allow(clippy::mut_from_ref)] // UnsafeCell, exclusive host audio lifecycle.
    pub unsafe fn audio(&self) -> Option<&mut GuestEngine> {
        let audio = unsafe { &mut *self.audio.get() };
        let pending = self.pending.swap(ptr::null_mut(), Ordering::Acquire);
        if !pending.is_null()
            && let Some(engine) = audio
        {
            unsafe {
                core::ptr::swap(engine, pending);
            }
            self.latency.store(engine.latency(), Ordering::Release);
            self.retired.store(pending, Ordering::Release);
            self.applied.store(true, Ordering::Release);
        }
        audio.as_mut()
    }
    pub fn latency(&self) -> u32 {
        self.latency.load(Ordering::Acquire)
    }
    pub fn needs_collection(&self) -> bool {
        self.applied.load(Ordering::Acquire)
    }
    /// Main-thread after host callback quiescence, including pending loads that
    /// never got a process callback. Those loads become the next activation.
    pub fn deactivate(&self) {
        let pending = self.pending.swap(ptr::null_mut(), Ordering::AcqRel);
        if !pending.is_null() {
            unsafe {
                drop(Box::from_raw(pending));
            }
            let main = unsafe { &mut *self.main.get() };
            if let Some(program) = main.pending.take() {
                main.committed = program;
            }
        }
        self.collect();
        unsafe {
            drop((*self.audio.get()).take());
        }
        self.rate.store(0, Ordering::Release);
    }
}
impl Drop for Publication {
    fn drop(&mut self) {
        self.deactivate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_fence_publishes_only_complete_applied_programs() {
        let state = Publication::default();
        state.activate(48_000.0).unwrap();
        let mut next = Program::default();
        next.project.settings.name = "new complete program".into();
        let encoded = next.encode().unwrap();
        let old = state.capture().unwrap();
        state.load(next).unwrap();
        assert_eq!(state.capture().unwrap(), old);
        assert!(state.load(Program::default()).is_err());
        unsafe {
            state.audio().unwrap();
        }
        assert_eq!(state.capture().unwrap(), encoded);
        assert!(state.retired.load(Ordering::Acquire).is_null());
        state.deactivate();
    }
    #[test]
    fn quiescent_deactivation_commits_unconsumed_load_and_invalid_load_is_atomic() {
        let state = Publication::default();
        state.activate(44_100.0).unwrap();
        let mut next = Program::default();
        next.project.settings.name = "next".into();
        let bytes = next.encode().unwrap();
        state.load(next).unwrap();
        state.deactivate();
        assert_eq!(state.capture().unwrap(), bytes);
        state.activate(96_000.0).unwrap();
        let mut invalid = Program::default();
        invalid.project.settings.tempo_bpm = f64::NAN;
        assert!(state.load(invalid).is_err());
        assert_eq!(state.capture().unwrap(), bytes);
    }
}
