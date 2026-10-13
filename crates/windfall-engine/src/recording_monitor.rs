//! Bounded live ADC monitor path. The worker resamples; output only dequeues.
use crate::mixer::Frame;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use windfall_ipc::{RecordingMonitorSettings, RecordingMonitorStatus};

#[derive(Clone)]
pub struct MonitorTelemetry {
    inner: Arc<Counters>,
    rate: u32,
}
#[derive(Default)]
struct Counters {
    queued: AtomicU64,
    producer_quantum: AtomicU64,
    dropped: AtomicU64,
    starved: AtomicU64,
}
impl MonitorTelemetry {
    pub fn snapshot(&self) -> RecordingMonitorStatus {
        RecordingMonitorStatus {
            buffered_ms: self.inner.queued.load(Ordering::Relaxed) as f64 * 1000.0
                / f64::from(self.rate),
            dropped_frames: self.inner.dropped.load(Ordering::Relaxed),
            starved_frames: self.inner.starved.load(Ordering::Relaxed),
        }
    }
}
pub struct MonitorWriter {
    queue: Producer<Frame>,
    pub telemetry: MonitorTelemetry,
    history: [Frame; 128],
    seen: u64,
    position: f64,
}
pub(crate) struct MonitorReader {
    queue: Consumer<Frame>,
    telemetry: MonitorTelemetry,
    pub settings: RecordingMonitorSettings,
    target: usize,
    primed: bool,
    gain: f32,
    last: Frame,
}
pub(crate) fn channel(
    settings: RecordingMonitorSettings,
    rate: u32,
) -> (MonitorWriter, Box<MonitorReader>) {
    let target = (u64::from(rate) * u64::from(settings.buffer_ms) / 1000).max(32) as usize;
    let (producer, consumer) = RingBuffer::new((target * 4).max(2048));
    let telemetry = MonitorTelemetry {
        inner: Arc::new(Counters::default()),
        rate,
    };
    (
        MonitorWriter {
            queue: producer,
            telemetry: telemetry.clone(),
            history: [[0.0; 2]; 128],
            seen: 0,
            position: 0.0,
        },
        Box::new(MonitorReader {
            queue: consumer,
            telemetry,
            settings,
            target,
            primed: false,
            gain: 0.0,
            last: [0.0; 2],
        }),
    )
}
impl MonitorWriter {
    /// Called only by the input worker. Period ratio follows independent ADC
    /// and DAC fits, keeping the live queue bounded over long sessions.
    pub(crate) fn push(&mut self, audio: &[Frame], step: f64) {
        let step = step.clamp(0.01, 100.0);
        // Driver packets arrive in bursts larger than a mixer subdivision.
        // Publish their converted quantum so normal packet delivery cannot
        // be mistaken for an accumulated monitor backlog on the callback.
        self.telemetry
            .inner
            .producer_quantum
            .fetch_max((audio.len() as f64 / step).ceil() as u64, Ordering::Relaxed);
        for sample in audio {
            self.history[self.seen as usize % 128] = *sample;
            self.seen += 1;
            while self.position + 16.0 < self.seen as f64 {
                let center = self.position.floor() as i64;
                let mut sum = [0.0_f64; 2];
                let mut weights = 0.0;
                for tap in -15..=16 {
                    let index = center + tap;
                    let distance = self.position - index as f64;
                    let weight = windfall_core::resample::weight(distance, step);
                    weights += weight;
                    if index >= 0
                        && (index as u64) >= self.seen.saturating_sub(128)
                        && (index as u64) < self.seen
                    {
                        let value = self.history[index as usize % 128];
                        sum[0] += f64::from(value[0]) * weight;
                        sum[1] += f64::from(value[1]) * weight;
                    }
                }
                let frame = [
                    (sum[0] / weights.max(1e-9)) as f32,
                    (sum[1] / weights.max(1e-9)) as f32,
                ];
                if self.queue.push(frame).is_err() {
                    self.telemetry.inner.dropped.fetch_add(1, Ordering::Relaxed);
                }
                self.position += step;
            }
        }
    }
}
impl MonitorReader {
    pub fn render(&mut self, out: &mut [Frame]) {
        let available = self.queue.slots();
        self.telemetry
            .inner
            .queued
            .store(available as u64, Ordering::Relaxed);
        if !self.primed {
            if available < self.target {
                return;
            }
            self.primed = true;
        }
        // Drop excess with a fixed callback work bound, then ramp back in.
        let excess = available
            .saturating_sub(
                self.target
                    + out.len().max(
                        self.telemetry
                            .inner
                            .producer_quantum
                            .load(Ordering::Relaxed) as usize,
                    ) * 2,
            )
            .min(1024);
        for _ in 0..excess {
            if self.queue.pop().is_ok() {
                self.telemetry.inner.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        if excess > 0 {
            self.gain = 0.0;
        }
        for output in out {
            match self.queue.pop() {
                Ok(frame) => {
                    self.last = frame;
                    self.gain = (self.gain + self.settings.gain / 128.0).min(self.settings.gain);
                }
                Err(_) => {
                    self.telemetry.inner.starved.fetch_add(1, Ordering::Relaxed);
                    self.gain *= 0.9;
                    if self.gain < 1e-6 {
                        self.gain = 0.0;
                    }
                }
            }
            output[0] += self.last[0] * self.gain;
            output[1] += self.last[1] * self.gain;
        }
        if self.queue.slots() == 0 {
            self.primed = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_packet_bursts_are_not_dropped_as_mixer_subdivision_backlog() {
        let (mut writer, mut reader) = channel(
            RecordingMonitorSettings {
                track: windfall_project::TrackId(0),
                gain: 0.1,
                buffer_ms: 20,
            },
            48_000,
        );
        // Initial priming plus the packet arriving just before the callback.
        for _ in 0..5 {
            writer.push(&[[0.25; 2]; 256], 1.0);
        }
        reader.render(&mut [[0.0; 2]; 64]);
        assert_eq!(writer.telemetry.snapshot().dropped_frames, 0);
        assert_eq!(writer.telemetry.snapshot().starved_frames, 0);
        // A genuinely stalled reader still bounds backlog and reports drops.
        for _ in 0..30 {
            writer.push(&[[0.25; 2]; 256], 1.0);
        }
        reader.render(&mut [[0.0; 2]; 64]);
        assert!(writer.telemetry.snapshot().dropped_frames > 0);
    }
}
