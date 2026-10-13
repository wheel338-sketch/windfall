//! Portable registration ledger. Tickets prevent dispatch after unregister or
//! same-address re-registration during a reentrant plugin callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Fd(i32),
    Timer { interval: u64, next: u64 },
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Registration {
    pub handler: usize,
    pub ticket: u64,
    pub source: Source,
}
#[derive(Default)]
pub(crate) struct Registrations {
    entries: Vec<Registration>,
    next: u64,
}
impl Registrations {
    pub fn register(&mut self, handler: usize, source: Source) -> Option<u64> {
        if handler == 0 || self.entries.len() == 256 || self.next == u64::MAX {
            return None;
        }
        match source {
            Source::Fd(fd) if fd < 0 => return None,
            Source::Timer { interval, .. } if interval == 0 || interval > 86_400_000 => {
                return None;
            }
            _ => {}
        }
        if self.entries.iter().any(|entry| {
            entry.handler == handler
                && match (entry.source, source) {
                    (Source::Fd(a), Source::Fd(b)) => a == b,
                    (Source::Timer { .. }, Source::Timer { .. }) => true,
                    _ => false,
                }
        }) {
            return None;
        }
        self.next += 1;
        self.entries.push(Registration {
            handler,
            ticket: self.next,
            source,
        });
        Some(self.next)
    }
    pub fn unregister(&mut self, handler: usize, timer: bool) -> Vec<u64> {
        let removed = self
            .entries
            .iter()
            .filter(|entry| {
                entry.handler == handler && matches!(entry.source, Source::Timer { .. }) == timer
            })
            .map(|entry| entry.ticket)
            .collect();
        self.entries.retain(|entry| {
            entry.handler != handler || matches!(entry.source, Source::Timer { .. }) != timer
        });
        removed
    }
    pub fn snapshot(&self) -> Vec<Registration> {
        self.entries.clone()
    }
    pub fn contains(&self, ticket: u64) -> bool {
        self.entries.iter().any(|entry| entry.ticket == ticket)
    }
    pub fn due(&mut self, now: u64) -> Vec<Registration> {
        let mut due = Vec::new();
        for entry in &mut self.entries {
            if let Source::Timer { interval, next } = &mut entry.source
                && now >= *next
            {
                *next = now.saturating_add(*interval);
                due.push(*entry);
            }
        }
        due
    }
    pub fn ready(&mut self, now: u64, mut readable: impl FnMut(i32) -> bool) -> Vec<Registration> {
        let mut ready = self.due(now);
        ready.extend(
            self.entries
                .iter()
                .copied()
                .filter(|entry| match entry.source {
                    Source::Fd(fd) => readable(fd),
                    Source::Timer { .. } => false,
                }),
        );
        ready
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unregister_and_reregister_invalidates_pending_dispatch_ticket() {
        let mut book = Registrations::default();
        let old = book.register(1, Source::Fd(9)).unwrap();
        let snapshot = book.snapshot();
        assert_eq!(book.unregister(1, false), [old]);
        let new = book.register(1, Source::Fd(9)).unwrap();
        assert!(!book.contains(snapshot[0].ticket));
        assert!(book.contains(new));
        assert_ne!(old, new);
        book.clear();
        assert!(!book.contains(new));
    }
    #[test]
    fn timers_coalesce_missed_ticks_and_unregister_during_dispatch() {
        let mut book = Registrations::default();
        let ticket = book
            .register(
                3,
                Source::Timer {
                    interval: 10,
                    next: 10,
                },
            )
            .unwrap();
        assert!(book.due(9).is_empty());
        assert_eq!(book.due(1000).len(), 1);
        assert!(book.due(1000).is_empty());
        assert!(book.due(1009).is_empty());
        assert_eq!(book.due(1010)[0].ticket, ticket);
        book.unregister(3, true);
        assert!(!book.contains(ticket));
        assert!(book.due(2000).is_empty());
    }
    #[test]
    fn registrations_bound_duplicates_bad_fds_and_timer_intervals() {
        let mut book = Registrations::default();
        assert!(book.register(0, Source::Fd(0)).is_none());
        assert!(book.register(1, Source::Fd(-1)).is_none());
        assert!(
            book.register(
                1,
                Source::Timer {
                    interval: 0,
                    next: 0
                }
            )
            .is_none()
        );
        for fd in 0..256 {
            assert!(book.register(1, Source::Fd(fd)).is_some());
        }
        assert!(book.register(2, Source::Fd(257)).is_none());
        assert_eq!(book.unregister(1, false).len(), 256);
        assert!(book.snapshot().is_empty());
    }
    #[test]
    fn fake_fd_backend_dispatches_only_ready_current_registrations() {
        let mut book = Registrations::default();
        let first = book.register(1, Source::Fd(9)).unwrap();
        assert!(book.register(1, Source::Fd(9)).is_none());
        let stale = book.register(2, Source::Fd(9)).unwrap();
        book.register(3, Source::Fd(10)).unwrap();
        book.register(
            4,
            Source::Timer {
                interval: 20,
                next: 20,
            },
        )
        .unwrap();
        let ready = book.ready(20, |fd| fd == 9);
        assert_eq!(ready.len(), 3);
        // A first callback unregisters a later callback and re-registers the
        // same handler address. Its old ready ticket must never be dispatched.
        let mut dispatched = Vec::new();
        for registration in ready {
            if !book.contains(registration.ticket) {
                continue;
            }
            dispatched.push(registration.handler);
            if registration.ticket == first {
                book.unregister(2, false);
                book.register(2, Source::Fd(9)).unwrap();
            }
        }
        assert_eq!(dispatched, [4, 1]);
        assert!(!book.contains(stale));
        assert_eq!(book.ready(21, |_| false).len(), 0);
    }
}
