//! One monotonic time domain for ADC packets, DAC frames and recording gates.
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU32, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

const PENDING: u64 = u64::MAX;

/// Maps the driver's stream epoch to our monotonic epoch. Callback timestamps
/// are device-clock snapshots, not necessarily the instant our callback runs.
/// In particular WASAPI loopback can report a zero callback clock even while
/// its capture packets carry valid absolute QPC timestamps.
#[derive(Clone, Default)]
pub(crate) struct DriverTimebase(Arc<OnceLock<(cpal::StreamInstant, u64)>>);

impl DriverTimebase {
    pub fn initialize(&self, stream: &cpal::Stream, clock: &RecordingClock) {
        use cpal::traits::StreamTrait;
        let before = clock.now_nanos();
        let instant = stream.now();
        let after = clock.now_nanos();
        let _ = self.0.set((instant, before + (after - before) / 2));
    }

    pub fn nanos_at(&self, instant: cpal::StreamInstant) -> Option<u64> {
        let &(base, at) = self.0.get()?;
        if let Some(delta) = instant.checked_duration_since(base) {
            at.checked_add(nanos(delta))
        } else {
            at.checked_sub(nanos(base.checked_duration_since(instant)?))
        }
    }

    pub fn capture(
        &self,
        stamp: cpal::InputStreamTimestamp,
        clock: &RecordingClock,
    ) -> (u64, u64, bool) {
        let now = clock.now_nanos();
        if let Some(captured) = self
            .nanos_at(stamp.capture)
            .filter(|captured| captured.abs_diff(now) < 5_000_000_000)
        {
            return (captured, now.saturating_sub(captured), true);
        }
        let latency = stamp.callback.checked_duration_since(stamp.capture);
        let latency_ns = latency.map_or(0, nanos);
        (
            now.saturating_sub(latency_ns),
            latency_ns,
            latency.is_some(),
        )
    }
}
#[derive(Clone)]
pub struct RecordingClock {
    inner: Arc<ClockState>,
}
struct ClockState {
    epoch: Instant,
    generation: AtomicU64,
    version: AtomicU64,
    frame: AtomicU64,
    nanos: AtomicU64,
    rate: AtomicU32,
    period: AtomicU64,
    latency: AtomicU64,
    measured: AtomicU32,
    fit_frame: AtomicU64,
    fit_nanos: AtomicU64,
    serial: AtomicU64,
    ticket: AtomicU64,
    start: AtomicU64,
    end: AtomicU64,
    start_frame: AtomicU64,
}
#[derive(Debug, Clone, Copy)]
pub struct OutputClock {
    pub generation: u64,
    pub frame: u64,
    pub nanos: u64,
    pub sample_rate: u32,
    pub nanos_per_frame: f64,
    pub latency_nanos: u64,
    pub measured: bool,
}
impl OutputClock {
    pub fn nanos_at(self, frame: u64) -> u64 {
        let delta = (frame.abs_diff(self.frame) as f64 * self.nanos_per_frame).round() as u64;
        if frame >= self.frame {
            self.nanos.saturating_add(delta)
        } else {
            self.nanos.saturating_sub(delta)
        }
    }
}
#[derive(Clone)]
pub struct CaptureGate {
    clock: RecordingClock,
    ticket: u64,
    generation: u64,
}
#[derive(Debug, Clone, Copy)]
pub struct CaptureWindow {
    pub start: Option<u64>,
    pub end: u64,
    pub start_frame: Option<u64>,
}
impl Default for RecordingClock {
    fn default() -> Self {
        Self::new()
    }
}
impl RecordingClock {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ClockState {
                epoch: Instant::now(),
                generation: AtomicU64::new(0),
                version: AtomicU64::new(0),
                frame: AtomicU64::new(0),
                nanos: AtomicU64::new(0),
                rate: AtomicU32::new(0),
                period: AtomicU64::new(0),
                latency: AtomicU64::new(0),
                measured: AtomicU32::new(0),
                fit_frame: AtomicU64::new(0),
                fit_nanos: AtomicU64::new(0),
                serial: AtomicU64::new(0),
                ticket: AtomicU64::new(0),
                start: AtomicU64::new(PENDING),
                end: AtomicU64::new(PENDING),
                start_frame: AtomicU64::new(PENDING),
            }),
        }
    }
    pub fn now_nanos(&self) -> u64 {
        nanos(self.inner.epoch.elapsed())
    }
    pub fn output(&self) -> Option<OutputClock> {
        for _ in 0..4 {
            let version = self.inner.version.load(Ordering::Acquire);
            if !version.is_multiple_of(2) {
                continue;
            }
            let value = OutputClock {
                generation: self.inner.generation.load(Ordering::Acquire),
                frame: self.inner.frame.load(Ordering::Relaxed),
                nanos: self.inner.nanos.load(Ordering::Relaxed),
                sample_rate: self.inner.rate.load(Ordering::Relaxed),
                nanos_per_frame: f64::from_bits(self.inner.period.load(Ordering::Relaxed)),
                latency_nanos: self.inner.latency.load(Ordering::Relaxed),
                measured: self.inner.measured.load(Ordering::Relaxed) != 0,
            };
            if version == self.inner.version.load(Ordering::Acquire) && value.sample_rate > 0 {
                return Some(value);
            }
        }
        None
    }
    /// Output callback only; stamped before the processor handles its requests.
    pub(crate) fn publish_output(
        &self,
        frame: u64,
        rate: u32,
        timestamp: cpal::OutputStreamTimestamp,
    ) {
        let latency = timestamp
            .playback
            .checked_duration_since(timestamp.callback);
        let latency_ns = latency.map_or(0, nanos);
        let measured_playback = self.now_nanos().saturating_add(latency_ns);
        self.publish_position(
            frame,
            rate,
            measured_playback,
            latency_ns,
            latency.is_some(),
        );
    }

    pub(crate) fn publish_driver_output(
        &self,
        frame: u64,
        rate: u32,
        timestamp: cpal::OutputStreamTimestamp,
        timebase: &DriverTimebase,
    ) {
        // WASAPI can ask for its initial buffer before its audio clock has
        // started. Zero is not a measured DAC timestamp; seeding the fit with
        // it makes later valid anchors converge through a fictitious drift.
        if timestamp.callback == cpal::StreamInstant::ZERO {
            return;
        }
        let now = self.now_nanos();
        if let Some(playback) = timebase
            .nanos_at(timestamp.playback)
            .filter(|playback| playback.abs_diff(now) < 5_000_000_000)
        {
            self.publish_position(frame, rate, playback, playback.saturating_sub(now), true);
        } else {
            self.publish_output(frame, rate, timestamp);
        }
    }

    fn publish_position(
        &self,
        frame: u64,
        rate: u32,
        measured_playback: u64,
        latency_ns: u64,
        measured: bool,
    ) {
        let nominal = 1e9 / f64::from(rate.max(1));
        let mut period = nominal;
        if let Some(previous) = self
            .output()
            .filter(|c| c.sample_rate == rate && frame > c.frame)
        {
            let predicted = previous.nanos_at(frame);
            let residual = measured_playback as f64 - predicted as f64;
            if residual.abs() <= 1_000_000.0 {
                let origin_frame = self.inner.fit_frame.load(Ordering::Relaxed);
                let origin_nanos = self.inner.fit_nanos.load(Ordering::Relaxed);
                let elapsed = frame.saturating_sub(origin_frame);
                period = previous.nanos_per_frame;
                if elapsed >= u64::from(rate) {
                    let observed = (measured_playback.saturating_sub(origin_nanos) as f64
                        / elapsed as f64)
                        .clamp(nominal * 0.995, nominal * 1.005);
                    period += (observed - period) * 0.02;
                }
            } else {
                self.inner.fit_frame.store(frame, Ordering::Relaxed);
                self.inner
                    .fit_nanos
                    .store(measured_playback, Ordering::Relaxed);
            }
        } else {
            self.inner.fit_frame.store(frame, Ordering::Relaxed);
            self.inner
                .fit_nanos
                .store(measured_playback, Ordering::Relaxed);
        }
        self.inner.version.fetch_add(1, Ordering::AcqRel);
        self.inner.frame.store(frame, Ordering::Relaxed);
        // The raw driver DAC anchor is already a sample timestamp. Fitting
        // period from an earlier *smoothed* anchor fed phase error back into
        // the clock rate, creating drift and a later discontinuous reset.
        self.inner.nanos.store(measured_playback, Ordering::Relaxed);
        self.inner.rate.store(rate, Ordering::Relaxed);
        self.inner.period.store(period.to_bits(), Ordering::Relaxed);
        self.inner.latency.store(latency_ns, Ordering::Relaxed);
        self.inner
            .measured
            .store(u32::from(measured), Ordering::Relaxed);
        self.inner.version.fetch_add(1, Ordering::Release);
    }
    /// Stream replacement invalidates every recording clock ticket.
    pub(crate) fn reset_output(&self) {
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
        self.inner.rate.store(0, Ordering::Release);
        self.close_active();
    }
    pub fn arm(&self, scheduled: bool) -> CaptureGate {
        let ticket = self
            .inner
            .serial
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        self.inner
            .start
            .store(if scheduled { PENDING } else { 0 }, Ordering::Relaxed);
        self.inner.end.store(PENDING, Ordering::Relaxed);
        self.inner.start_frame.store(PENDING, Ordering::Relaxed);
        self.inner.ticket.store(ticket, Ordering::Release);
        CaptureGate {
            clock: self.clone(),
            ticket,
            generation: self.inner.generation.load(Ordering::Acquire),
        }
    }
    pub(crate) fn active_ticket(&self) -> Option<u64> {
        let ticket = self.inner.ticket.load(Ordering::Acquire);
        (ticket != 0).then_some(ticket)
    }
    pub(crate) fn schedule(&self, ticket: u64, frame: u64) {
        if self.active_ticket() != Some(ticket) {
            return;
        }
        if let Some(output) = self.output() {
            self.inner.start_frame.store(frame, Ordering::Relaxed);
            self.inner
                .start
                .store(output.nanos_at(frame), Ordering::Release);
        }
    }
    pub(crate) fn close_active(&self) {
        self.inner.end.fetch_min(self.now_nanos(), Ordering::AcqRel);
    }
}
impl CaptureGate {
    pub(crate) fn current(&self) -> bool {
        self.clock.inner.ticket.load(Ordering::Acquire) == self.ticket
            && self.clock.inner.generation.load(Ordering::Acquire) == self.generation
    }
    pub(crate) fn scheduled_frame(&self) -> Option<u64> {
        let frame = self.clock.inner.start_frame.load(Ordering::Acquire);
        (frame != PENDING).then_some(frame)
    }
    pub fn clock(&self) -> RecordingClock {
        self.clock.clone()
    }
    pub fn window(&self) -> Result<CaptureWindow, String> {
        if self.clock.inner.ticket.load(Ordering::Acquire) != self.ticket
            || self.clock.inner.generation.load(Ordering::Acquire) != self.generation
        {
            return Err("Recording output clock changed; take discarded.".into());
        }
        let start = self.clock.inner.start.load(Ordering::Acquire);
        let frame = self.clock.inner.start_frame.load(Ordering::Acquire);
        let start_frame = (frame != PENDING).then_some(frame);
        let start = if start != PENDING {
            Some(
                start_frame
                    .and_then(|frame| self.clock.output().map(|clock| clock.nanos_at(frame)))
                    .unwrap_or(start),
            )
        } else {
            None
        };
        Ok(CaptureWindow {
            start,
            end: self.clock.inner.end.load(Ordering::Acquire),
            start_frame,
        })
    }
    pub fn close(&self) {
        if self.clock.active_ticket() == Some(self.ticket) {
            self.clock.close_active();
        }
    }
    pub fn release(&self) {
        let _ = self.clock.inner.ticket.compare_exchange(
            self.ticket,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}
pub(crate) fn nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX - 1)) as u64
}

#[cfg(test)]
mod timebase_tests {
    use super::*;

    #[test]
    fn an_output_phase_step_is_not_fitted_as_sample_clock_drift() {
        let clock = RecordingClock::new();
        clock.publish_position(0, 48000, 1_000_000_000, 0, true);
        for block in 1..=300 {
            let at = 1_010_000_000 + block * 10_000_000;
            clock.publish_position(block * 480, 48000, at, 0, true);
            let output = clock.output().unwrap();
            assert_eq!(output.nanos, at);
            assert!((output.nanos_per_frame - 1e9 / 48000.0).abs() < 0.001);
        }
    }

    #[test]
    fn output_does_not_seed_a_measured_fit_from_the_unstarted_driver_clock() {
        let clock = RecordingClock::new();
        let map = DriverTimebase::default();
        map.0
            .set((cpal::StreamInstant::from_millis(1000), 1_000_000))
            .unwrap();
        clock.publish_driver_output(
            0,
            48000,
            cpal::OutputStreamTimestamp {
                callback: cpal::StreamInstant::ZERO,
                playback: cpal::StreamInstant::ZERO,
            },
            &map,
        );
        assert!(clock.output().is_none());
        clock.publish_driver_output(
            480,
            48000,
            cpal::OutputStreamTimestamp {
                callback: cpal::StreamInstant::from_millis(1000),
                playback: cpal::StreamInstant::from_millis(1010),
            },
            &map,
        );
        assert_eq!(clock.output().unwrap().nanos, 11_000_000);
    }

    #[test]
    fn capture_uses_the_driver_epoch_even_when_callback_clock_is_zero() {
        let clock = RecordingClock::new();
        let base = cpal::StreamInstant::from_secs_f64(100_000.0);
        let map = DriverTimebase::default();
        map.0.set((base, 10_000_000)).unwrap();
        let stamp = cpal::InputStreamTimestamp {
            capture: base.checked_sub(Duration::from_millis(3)).unwrap(),
            callback: cpal::StreamInstant::ZERO,
        };
        let (captured, _, measured) = map.capture(stamp, &clock);
        assert!(measured);
        assert_eq!(captured, 7_000_000);
        assert_eq!(
            map.nanos_at(base.checked_add(Duration::from_millis(4)).unwrap()),
            Some(14_000_000)
        );
        assert_eq!(map.nanos_at(cpal::StreamInstant::ZERO), None);
    }
}
