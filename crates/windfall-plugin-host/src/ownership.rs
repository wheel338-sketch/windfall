//! Bounded ownership exchange between a native owner and an audio adapter.
//!
//! Construct and retire both halves on control threads. The audio half only
//! moves an already allocated value at a block boundary. It never waits,
//! destroys the value, or invokes native lifecycle operations. The owner must
//! return the value before cancelling a successful request. Cancelling a
//! request that raced a boundary still requires draining `take_returned`.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Owner-side endpoint. Requests are level-triggered and may be cancelled.
pub struct ControlOwnership<T> {
    requested: Arc<AtomicBool>,
    returned: rtrb::Consumer<T>,
    resume: rtrb::Producer<T>,
}

/// Audio-side endpoint. Its destruction belongs on the native owner thread.
pub struct AudioOwnership<T> {
    requested: Arc<AtomicBool>,
    current: Option<T>,
    returned: rtrb::Producer<T>,
    resume: rtrb::Consumer<T>,
}

/// Allocates two single-value queues before audio ownership begins.
pub fn exchange<T>(value: T) -> (ControlOwnership<T>, AudioOwnership<T>) {
    let requested = Arc::new(AtomicBool::new(false));
    let (returned, receive) = rtrb::RingBuffer::new(1);
    let (resume, incoming) = rtrb::RingBuffer::new(1);
    (
        ControlOwnership {
            requested: requested.clone(),
            returned: receive,
            resume,
        },
        AudioOwnership {
            requested,
            current: Some(value),
            returned,
            resume: incoming,
        },
    )
}

impl<T> ControlOwnership<T> {
    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }

    /// A timeout cancels the request, not a return already in flight. The
    /// owner must continue servicing returned values after a timeout.
    pub fn cancel(&self) {
        self.requested.store(false, Ordering::Release);
    }

    pub fn take_returned(&mut self) -> Option<T> {
        self.returned.pop().ok()
    }

    /// Transfers a restored adapter back without destroying it on failure.
    pub fn resume(&mut self, value: T) -> Result<(), T> {
        match self.resume.push(value) {
            Ok(()) => {
                self.cancel();
                Ok(())
            }
            Err(rtrb::PushError::Full(value)) => Err(value),
        }
    }
}

impl<T> AudioOwnership<T> {
    /// Call once at a block boundary, even if this slot is bypassed, silent,
    /// leaving a plan, or the transport is stopped. Returns true on reacquire.
    pub fn boundary(&mut self) -> bool {
        let mut acquired = false;
        if self.current.is_none()
            && let Ok(value) = self.resume.pop()
        {
            self.current = Some(value);
            acquired = true;
        }
        if self.requested.load(Ordering::Acquire)
            && let Some(value) = self.current.take()
        {
            // A failed push retains ownership locally. No value is freed in
            // the callback, including an unexpected full return queue.
            if let Err(rtrb::PushError::Full(value)) = self.returned.push(value) {
                self.current = Some(value);
            }
            acquired = false;
        }
        acquired
    }

    /// True while the owner is asking for the value back.
    pub fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    pub fn current(&self) -> Option<&T> {
        self.current.as_ref()
    }

    pub fn current_mut(&mut self) -> Option<&mut T> {
        self.current.as_mut()
    }

    /// Owner-thread retirement after the engine has stopped accessing this
    /// endpoint. Drain the control endpoint's return queue as well.
    pub fn retire(&mut self) -> Option<T> {
        self.current.take().or_else(|| self.resume.pop().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_request_before_reacquire_returns_the_same_value() {
        let (mut owner, mut audio) = exchange(Box::new(7));
        owner.request();
        audio.boundary();
        assert!(audio.current().is_none());
        let value = owner.take_returned().unwrap();
        owner.resume(value).unwrap();
        owner.request();
        assert!(!audio.boundary());
        assert_eq!(**owner.take_returned().as_ref().unwrap(), 7);
    }

    #[test]
    fn cancel_before_and_after_boundary_preserves_exact_ownership() {
        let (mut owner, mut audio) = exchange(Box::new(9));
        owner.request();
        owner.cancel();
        audio.boundary();
        assert_eq!(**audio.current().unwrap(), 9);
        assert!(owner.take_returned().is_none());
        owner.request();
        audio.boundary();
        owner.cancel();
        let value = owner.take_returned().unwrap();
        owner.resume(value).unwrap();
        assert!(audio.boundary());
        assert_eq!(**audio.retire().as_ref().unwrap(), 9);
    }
}
