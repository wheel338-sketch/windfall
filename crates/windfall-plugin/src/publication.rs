//! Single control publisher / single callback consumer. A prepared engine is
//! moved at a block boundary; the replaced box is reclaimed only by control.
use crate::{GuestEngine, Program};
use std::cell::UnsafeCell;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
type Notifier = Box<dyn Fn(u32)>;

pub(crate) struct Publication {
    notify: UnsafeCell<Option<Notifier>>,
    main: UnsafeCell<Main>,
    audio: UnsafeCell<Option<GuestEngine>>,
    pending: AtomicPtr<GuestEngine>,
    retired: AtomicPtr<GuestEngine>,
    applied: AtomicBool,
    rate: AtomicU32,
    latency: AtomicU32,
    requested_latency: AtomicU32,
    restart_required: AtomicBool,
}
pub(crate) const DIRTY: u32 = 1;
pub(crate) const PARAMETERS: u32 = 2;
pub(crate) const LATENCY: u32 = 4;
pub(crate) const RESTART: u32 = 8;
struct Main {
    committed: Program,
    pending: Option<Program>,
    provider: Option<std::sync::Arc<crate::nested::Provider>>,
    pending_provider: Option<std::sync::Arc<crate::nested::Provider>>,
    offline: bool,
}
impl Default for Publication {
    fn default() -> Self {
        Self::new(Program::default())
    }
}
impl Publication {
    pub fn new(program: Program) -> Self {
        Self {
            notify: UnsafeCell::new(None),
            main: UnsafeCell::new(Main {
                committed: program,
                pending: None,
                provider: None,
                pending_provider: None,
                offline: false,
            }),
            audio: UnsafeCell::new(None),
            pending: AtomicPtr::new(ptr::null_mut()),
            retired: AtomicPtr::new(ptr::null_mut()),
            applied: AtomicBool::new(false),
            rate: AtomicU32::new(0),
            latency: AtomicU32::new(0),
            requested_latency: AtomicU32::new(0),
            restart_required: AtomicBool::new(false),
        }
    }
    // Main-thread only. The acquire pairs with callback acknowledgement AFTER
    // the complete processor swap. Capture observes either full old or full new.
    pub fn capture(&self) -> Result<Vec<u8>, String> {
        self.collect();
        let main = unsafe { &*self.main.get() };
        if let Some(provider) = &main.provider {
            provider.capture(&main.committed)?.encode()
        } else {
            main.committed.encode()
        }
    }
    pub fn collect(&self) {
        if self.applied.swap(false, Ordering::Acquire) {
            let main = unsafe { &mut *self.main.get() };
            if let Some(program) = main.pending.take() {
                main.committed = program;
                main.provider = main.pending_provider.take();
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
        self.load_program(program, false)
    }
    pub fn hard_realtime(&self) -> bool {
        let main = unsafe { &*self.main.get() };
        !main.committed.project.plugins.is_empty()
            || main
                .pending
                .as_ref()
                .is_some_and(|p| !p.project.plugins.is_empty())
    }
    pub fn set_offline(&self, offline: bool) -> Result<(), String> {
        if offline && self.hard_realtime() {
            return Err(
                "Nested CLAP/VST3 helpers require realtime host scheduling; use a realtime bounce"
                    .into(),
            );
        }
        unsafe {
            (&mut *self.main.get()).offline = offline;
        }
        Ok(())
    }
    /// Shared by native editors on every platform. Host recall is not a user edit.
    pub fn load_editor(&self, program: Program) -> Result<(), String> {
        self.load_program(program, true)
    }
    pub fn set_notifier(&self, notify: Option<Notifier>) {
        unsafe {
            *self.notify.get() = notify;
        }
    }
    fn announce(&self, flags: u32) {
        if let Some(notify) = unsafe { &*self.notify.get() } {
            notify(flags);
        }
    }
    fn load_program(&self, program: Program, editor: bool) -> Result<(), String> {
        program.validate()?;
        self.collect();
        let main = unsafe { &mut *self.main.get() };
        if main.offline && !program.project.plugins.is_empty() {
            return Err("Nested CLAP/VST3 helpers require realtime host scheduling; leave offline rendering before loading this program".into());
        }
        if main.pending.is_some() {
            return Err("Previous program awaits an audio boundary".into());
        }
        let rate = self.rate.load(Ordering::Acquire);
        if rate == 0 {
            main.committed = program;
            self.announce(PARAMETERS | if editor { DIRTY } else { 0 });
            return Ok(());
        }
        let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            GuestEngine::activate(&program, f64::from(rate))
        }))
        .map_err(|_| "Program preparation panicked; previous program retained".to_owned())?;
        let engine = Box::new(prepared?);
        let changed_latency = self.latency.load(Ordering::Acquire) != engine.latency();
        self.requested_latency
            .store(engine.latency(), Ordering::Release);
        self.restart_required
            .store(changed_latency, Ordering::Release);
        main.pending_provider = engine.provider.clone();
        main.pending = Some(program);
        self.pending.store(Box::into_raw(engine), Ordering::Release);
        // Latency-changing programs cannot cross a live boundary. They settle
        // on host deactivation/restart; old audio and CLAP latency stay valid
        // while the host delays its restart. VST3 can query the requested PDC.
        self.announce(
            PARAMETERS
                | if editor { DIRTY } else { 0 }
                | if changed_latency {
                    LATENCY | RESTART
                } else {
                    0
                },
        );
        Ok(())
    }
    pub fn activate(&self, rate: f64) -> Result<u32, String> {
        self.collect();
        let engine = GuestEngine::activate(unsafe { &(&*self.main.get()).committed }, rate)?;
        let latency = engine.latency();
        unsafe {
            (&mut *self.main.get()).provider = engine.provider.clone();
        }
        self.latency.store(latency, Ordering::Release);
        self.requested_latency.store(latency, Ordering::Release);
        self.restart_required.store(false, Ordering::Release);
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
        // Read the restart bit AFTER acquiring the published pointer, so a
        // callback racing preparation cannot consume a latency-changing engine.
        if !pending.is_null() && self.restart_required.load(Ordering::Acquire) {
            self.pending.store(pending, Ordering::Release);
            return audio.as_mut();
        }
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
    pub fn requested_latency(&self) -> u32 {
        self.requested_latency.load(Ordering::Acquire)
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
                // Unconsumed helper has no new native state to retain.
                main.pending_provider = None;
                main.provider = None;
            }
        }
        self.collect();
        let main = unsafe { &mut *self.main.get() };
        if let Some(provider) = &main.provider {
            match provider.capture(&main.committed) {
                Ok(program) => main.committed = program,
                Err(error) => crate::runtime::error(error),
            }
        }
        unsafe {
            drop((*self.audio.get()).take());
        }
        self.rate.store(0, Ordering::Release);
        self.restart_required.store(false, Ordering::Release);
        main.provider = None;
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
    fn nested_offline_mode_is_a_visible_refusal_and_retains_the_previous_program() {
        let mut program = crate::clap::tests::program();
        program.project.channels[0].source = windfall_project::ChannelSource::Instrument {
            params: windfall_dsp::InstrumentParams::SubtractiveSynth(Default::default()),
            asset: None,
        };
        program
            .project
            .plugins
            .push(windfall_project::PluginBinding {
                capabilities: Default::default(),
                target: windfall_project::PluginTarget::Instrument {
                    channel: program.project.channels[0].id,
                },
                format: "clap".into(),
                path: "owned.clap".into(),
                id: "owned".into(),
                name: "Owned".into(),
                state: vec![],
                parameters: vec![],
                wrapper: Default::default(),
                sidechain_input: None,
                auxiliary_inputs: vec![],
            });
        let state = Publication::default();
        let previous = state.capture().unwrap();
        state.set_offline(true).unwrap();
        assert!(
            state
                .load_editor(program.clone())
                .unwrap_err()
                .contains("realtime host scheduling")
        );
        assert_eq!(state.capture().unwrap(), previous);
        state.set_offline(false).unwrap();
        state.load(program).unwrap();
        assert!(state.hard_realtime());
        assert!(
            state
                .set_offline(true)
                .unwrap_err()
                .contains("realtime bounce")
        );
    }
    #[test]
    fn editor_load_announces_dirty_parameters_latency_restart_and_refusal_is_atomic() {
        let flags = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let state = Publication::default();
        let out = flags.clone();
        state.set_notifier(Some(Box::new(move |flag| out.borrow_mut().push(flag))));
        state.load(Program::default()).unwrap();
        assert_eq!(&*flags.borrow(), &[PARAMETERS]);
        flags.borrow_mut().clear();
        state.load_editor(Program::default()).unwrap();
        assert_eq!(&*flags.borrow(), &[DIRTY | PARAMETERS]);
        flags.borrow_mut().clear();
        state.activate(48_000.0).unwrap();
        let mut latency = Program::default();
        latency.project.next_id = 50;
        latency.project.mixer.tracks[0]
            .effects
            .push(windfall_project::EffectSlot {
                id: windfall_project::EffectId(40),
                impulse: None,
                enabled: true,
                mix: 1.0,
                params: windfall_dsp::EffectParams::Limiter(Default::default()),
            });
        let expected = GuestEngine::activate(&latency, 48_000.0).unwrap().latency();
        assert!(expected > 0);
        state.load_editor(latency).unwrap();
        assert_eq!(&*flags.borrow(), &[DIRTY | PARAMETERS | LATENCY | RESTART]);
        assert_eq!(state.latency(), 0);
        assert_eq!(state.requested_latency(), expected);
        assert!(state.load_editor(Program::default()).is_err());
        assert_eq!(flags.borrow().len(), 1);
        unsafe {
            assert_eq!(state.audio().unwrap().latency(), 0);
        }
        assert!(
            state.load(Program::default()).is_err(),
            "restart still owns the pending slot"
        );
        state.deactivate();
        state.activate(48_000.0).unwrap();
        assert_eq!(state.latency(), expected);
        state.collect();
        flags.borrow_mut().clear();
        let old = state.capture().unwrap();
        let mut invalid = Program::default();
        invalid.project.settings.tempo_bpm = f64::NAN;
        assert!(state.load_editor(invalid).is_err());
        assert!(flags.borrow().is_empty());
        assert_eq!(state.capture().unwrap(), old);
        state.deactivate();
    }
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
