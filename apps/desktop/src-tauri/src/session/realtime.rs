//! The realtime feed: the playhead and meters, sixty times a second.

use std::io;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windfall_ipc::RealtimeFrame;

use super::Session;
use crate::sync::lock;

/// Time between two frames of the realtime feed.
pub const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);

/// Frames between two looks at the audio device's status: about a second.
const STATUS_EVERY: u32 = 60;

fn next_wait(deadline: &mut Instant, now: Instant) -> Option<Duration> {
    *deadline += FRAME_INTERVAL;
    let wait = deadline.checked_duration_since(now);
    if wait.is_none() && now.duration_since(*deadline) > FRAME_INTERVAL * 4 {
        // Resume from now after a long suspension; skip the stale backlog.
        *deadline = now;
    }
    wait
}

/// Delivers a frame to one window. Returns false once the window is gone,
/// and is then dropped.
pub type FrameSender = Box<dyn FnMut(&RealtimeFrame) -> bool + Send>;

pub(super) struct Subscriber {
    /// Names the window, so a window that subscribes again, as after a
    /// reload, replaces its old sender instead of being sent everything
    /// twice.
    key: String,
    send: FrameSender,
}

impl Session {
    /// Starts sending realtime frames to a window.
    pub fn subscribe_realtime(&self, key: impl Into<String>, send: FrameSender) {
        let key = key.into();
        let mut subscribers = lock(&self.inner.subscribers);
        subscribers.retain(|subscriber| subscriber.key != key);
        subscribers.push(Subscriber { key, send });
    }

    /// Stops sending realtime frames to a window, as when it closes.
    pub fn unsubscribe_realtime(&self, key: &str) {
        lock(&self.inner.subscribers).retain(|subscriber| subscriber.key != key);
    }

    /// Reads one frame from the engine and sends it to every subscriber.
    ///
    /// The outer carrier explicitly frees returned audio owners before reading
    /// the frame/resetting peaks, with no Session/controller guard held. Exactly
    /// one caller must do this, and keep
    /// doing it whether or not anyone is listening. That caller is the
    /// thread [`spawn_realtime`](Self::spawn_realtime) starts.
    pub fn realtime_tick(&self) -> RealtimeFrame {
        let mut retirement = windfall_engine::ProjectRetirement::default();
        self.realtime_tick_with_retirement(&mut retirement)
    }

    fn realtime_tick_with_retirement(
        &self,
        retirement: &mut windfall_engine::ProjectRetirement,
    ) -> RealtimeFrame {
        self.realtime_tick_at_with_retirement(retirement, Instant::now())
    }

    #[cfg(test)]
    pub(super) fn realtime_tick_at(&self, now: Instant) -> RealtimeFrame {
        let mut retirement = windfall_engine::ProjectRetirement::default();
        self.realtime_tick_at_with_retirement(&mut retirement, now)
    }

    fn realtime_tick_at_with_retirement(
        &self,
        retirement: &mut windfall_engine::ProjectRetirement,
        now: Instant,
    ) -> RealtimeFrame {
        self.retire_project(retirement);
        let frame = self.controller().frame();
        lock(&self.inner.subscribers).retain_mut(|subscriber| (subscriber.send)(&frame));
        self.analyzer_tick(now);
        // Playback that ended by itself: the end of a song, or a device
        // that went away.
        let told = lock(&self.inner.transport).playing;
        if told != frame.playing {
            self.sync_transport();
        }
        frame
    }

    /// Starts the thread that drives the realtime feed. It stops when the
    /// last handle to the session is dropped.
    pub fn spawn_realtime(&self) -> io::Result<JoinHandle<()>> {
        let session = self.downgrade();
        thread::Builder::new()
            .name("windfall-realtime".to_owned())
            .spawn(move || {
                let mut deadline = Instant::now();
                let mut frames = 0_u32;
                let mut retirement = windfall_engine::ProjectRetirement::default();
                loop {
                    // Frames are timed from a running deadline and not from
                    // the end of the last one, so the rate does not drift
                    // with how long a frame takes to send.
                    if let Some(wait) = next_wait(&mut deadline, Instant::now()) {
                        thread::sleep(wait);
                    }
                    let Some(session) = session.upgrade() else {
                        break;
                    };
                    session.realtime_tick_with_retirement(&mut retirement);
                    frames = frames.wrapping_add(1);
                    if frames.is_multiple_of(STATUS_EVERY) {
                        session.poll_engine_status();
                    }
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_deadlines_keep_sixty_hz_without_drifting_with_frame_work() {
        let start = Instant::now();
        let mut deadline = start;
        for frame in 1..=600 {
            let work = Duration::from_millis(frame % 9);
            let now = deadline + work;
            let wait = next_wait(&mut deadline, now).unwrap();
            assert_eq!(wait, FRAME_INTERVAL - work);
            assert_eq!(now + wait, start + FRAME_INTERVAL * frame as u32);
        }
        let nominal_second = FRAME_INTERVAL * 60;
        assert!(Duration::from_secs(1).abs_diff(nominal_second) < Duration::from_nanos(60));
    }

    #[test]
    fn suspended_feed_skips_backlog_and_resumes_with_one_interval() {
        let start = Instant::now();
        let mut deadline = start;
        let resumed = start + FRAME_INTERVAL * 100;
        assert_eq!(next_wait(&mut deadline, resumed), None);
        assert_eq!(deadline, resumed);
        assert_eq!(next_wait(&mut deadline, resumed), Some(FRAME_INTERVAL));

        // Short delays retain the running schedule rather than causing drift.
        let late = deadline + FRAME_INTERVAL * 2;
        assert_eq!(next_wait(&mut deadline, late), None);
        assert_eq!(deadline, resumed + FRAME_INTERVAL * 2);
    }
}
