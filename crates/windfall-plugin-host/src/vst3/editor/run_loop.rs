//! VST3 Linux IRunLoop queried through IPlugFrame. poll(2) is nonblocking;
//! callbacks and all registration/refcount retirement run on the editor owner.
use super::Frame;
use crate::gui::lifecycle::LocalRegistry;
use crate::gui::run_loop::{Registrations, Source};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    time::Instant,
};
use vst3::{
    ComPtr, ComRef,
    Steinberg::{Linux::*, *},
};
#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}
unsafe extern "C" {
    fn poll(fds: *mut PollFd, count: usize, timeout: i32) -> i32;
}
enum Handler {
    Fd(ComPtr<IEventHandler>),
    Timer(ComPtr<ITimerHandler>),
}
impl Clone for Handler {
    fn clone(&self) -> Self {
        match self {
            Self::Fd(v) => Self::Fd(Clone::clone(v)),
            Self::Timer(v) => Self::Timer(Clone::clone(v)),
        }
    }
}
pub(in crate::vst3) struct State {
    owner: usize,
    start: Instant,
    alive: Cell<bool>,
    book: RefCell<Registrations>,
    handlers: RefCell<BTreeMap<u64, Handler>>,
}
// No TLS destructor may outlive an exported host library's code mapping.
thread_local! { static LOOPS: LocalRegistry<State> = const { LocalRegistry::new() }; }
#[derive(Clone)]
pub(in crate::vst3) struct RunLoop(Rc<State>);
impl std::ops::Deref for RunLoop {
    type Target = State;
    fn deref(&self) -> &State {
        &self.0
    }
}
impl RunLoop {
    pub fn new() -> Self {
        let state = Rc::new(State {
            owner: super::super::handlers::token(),
            start: Instant::now(),
            alive: Cell::new(true),
            book: RefCell::new(Registrations::default()),
            handlers: RefCell::new(BTreeMap::new()),
        });
        LOOPS.with(|registry| registry.add(&state));
        Self(state)
    }
}
pub(in crate::vst3) fn pump() {
    let loops = LOOPS.with(LocalRegistry::live);
    for run_loop in loops {
        run_loop.poll();
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.alive.set(false);
        LOOPS.with(LocalRegistry::retire);
    }
}
impl State {
    fn owner(&self) -> bool {
        self.alive.get() && self.owner == super::super::handlers::token()
    }
    fn now(&self) -> u64 {
        self.start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }
    fn register(&self, address: usize, source: Source, handler: Handler) -> tresult {
        if !self.owner() {
            return kResultFalse;
        }
        let Some(ticket) = self.book.borrow_mut().register(address, source) else {
            return kInvalidArgument;
        };
        self.handlers.borrow_mut().insert(ticket, handler);
        kResultOk
    }
    fn unregister(&self, address: usize, timer: bool) -> tresult {
        if !self.owner() {
            return kResultFalse;
        }
        let tickets = self.book.borrow_mut().unregister(address, timer);
        if tickets.is_empty() {
            return kResultFalse;
        }
        // Drop native references outside map borrows, allowing native destructors
        // to reenter unregister without RefCell panics.
        let removed = tickets
            .into_iter()
            .filter_map(|ticket| self.handlers.borrow_mut().remove(&ticket))
            .collect::<Vec<_>>();
        drop(removed);
        kResultOk
    }
    pub fn poll(&self) {
        if !self.owner() {
            return;
        }
        let registrations = self.book.borrow().snapshot();
        let events = registrations
            .iter()
            .filter_map(|entry| match entry.source {
                Source::Fd(fd) => Some((entry.ticket, fd)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut descriptors = events
            .iter()
            .map(|(_, fd)| PollFd {
                fd: *fd,
                events: 1 | 2,
                revents: 0,
            })
            .collect::<Vec<_>>();
        if !descriptors.is_empty() {
            unsafe {
                poll(descriptors.as_mut_ptr(), descriptors.len(), 0);
            }
        }
        let ready = self.book.borrow_mut().ready(self.now(), |fd| {
            descriptors
                .iter()
                .any(|descriptor| descriptor.fd == fd && descriptor.revents & (1 | 2 | 8 | 16) != 0)
        });
        for registration in ready {
            let ticket = registration.ticket;
            let fd = match registration.source {
                Source::Fd(fd) => Some(fd),
                Source::Timer { .. } => None,
            };
            if !self.alive.get() || !self.book.borrow().contains(ticket) {
                continue;
            }
            let handler = self.handlers.borrow().get(&ticket).cloned();
            // No ledger/map borrow survives a plugin call. A prior callback may
            // unregister later ready callbacks or register at the same address.
            match (handler, fd) {
                (Some(Handler::Fd(handler)), Some(fd)) => unsafe {
                    handler.onFDIsSet(fd);
                },
                (Some(Handler::Timer(handler)), None) => unsafe {
                    handler.onTimer();
                },
                _ => {}
            }
        }
    }
    pub fn clear(&self) {
        self.alive.set(false);
        self.book.borrow_mut().clear();
        let handlers = std::mem::take(&mut *self.handlers.borrow_mut());
        drop(handlers);
    }
}
#[allow(non_snake_case)]
impl IRunLoopTrait for Frame {
    unsafe fn registerEventHandler(
        &self,
        handler: *mut IEventHandler,
        fd: FileDescriptor,
    ) -> tresult {
        if !self.run_loop.owner() {
            return kResultFalse;
        }
        let Some(reference) = (unsafe { ComRef::from_raw(handler) }) else {
            return kInvalidArgument;
        };
        self.run_loop.register(
            handler as usize,
            Source::Fd(fd),
            Handler::Fd(reference.to_com_ptr()),
        )
    }
    unsafe fn unregisterEventHandler(&self, handler: *mut IEventHandler) -> tresult {
        self.run_loop.unregister(handler as usize, false)
    }
    unsafe fn registerTimer(
        &self,
        handler: *mut ITimerHandler,
        milliseconds: TimerInterval,
    ) -> tresult {
        if !self.run_loop.owner() {
            return kResultFalse;
        }
        let Some(reference) = (unsafe { ComRef::from_raw(handler) }) else {
            return kInvalidArgument;
        };
        self.run_loop.register(
            handler as usize,
            Source::Timer {
                interval: milliseconds,
                next: self.run_loop.now().saturating_add(milliseconds),
            },
            Handler::Timer(reference.to_com_ptr()),
        )
    }
    unsafe fn unregisterTimer(&self, handler: *mut ITimerHandler) -> tresult {
        self.run_loop.unregister(handler as usize, true)
    }
}
#[allow(non_snake_case)]
impl IRunLoopTrait for super::super::HostApplication {
    unsafe fn registerEventHandler(
        &self,
        handler: *mut IEventHandler,
        fd: FileDescriptor,
    ) -> tresult {
        if !self.run_loop.owner() {
            return kResultFalse;
        }
        let Some(reference) = (unsafe { ComRef::from_raw(handler) }) else {
            return kInvalidArgument;
        };
        self.run_loop.register(
            handler as usize,
            Source::Fd(fd),
            Handler::Fd(reference.to_com_ptr()),
        )
    }
    unsafe fn unregisterEventHandler(&self, handler: *mut IEventHandler) -> tresult {
        self.run_loop.unregister(handler as usize, false)
    }
    unsafe fn registerTimer(
        &self,
        handler: *mut ITimerHandler,
        milliseconds: TimerInterval,
    ) -> tresult {
        if !self.run_loop.owner() {
            return kResultFalse;
        }
        let Some(reference) = (unsafe { ComRef::from_raw(handler) }) else {
            return kInvalidArgument;
        };
        self.run_loop.register(
            handler as usize,
            Source::Timer {
                interval: milliseconds,
                next: self.run_loop.now().saturating_add(milliseconds),
            },
            Handler::Timer(reference.to_com_ptr()),
        )
    }
    unsafe fn unregisterTimer(&self, handler: *mut ITimerHandler) -> tresult {
        self.run_loop.unregister(handler as usize, true)
    }
}
