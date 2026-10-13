//! Input capture. The driver callback only converts and pushes whole stereo
//! frames into a bounded SPSC queue; the owning worker performs disk IO.
use crate::recording_endpoint::{FaultReason, InputFault};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use rtrb::{Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;
use windfall_ipc::{RecordingInput, RecordingSource};

#[path = "recording_timed.rs"]
mod timed;
pub use timed::{AlignmentMetrics, CaptureRoute};

pub struct Capture {
    stop: Arc<AtomicBool>,
    fault: Arc<InputFault>,
    frames: Arc<AtomicU64>,
    worker: Option<JoinHandle<Result<u64, String>>>,
}
impl Capture {
    /// Opens the input on its owning worker. The sink is invoked only there.
    /// Only a device configuration at exactly `rate` is accepted: captured
    /// samples are never relabeled with the output rate.
    pub fn start<F>(source: RecordingSource, rate: u32, mut sink: F) -> Result<Self, String>
    where
        F: FnMut(&[f32]) -> Result<(), String> + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let fault = Arc::new(InputFault::new());
        let frames = Arc::new(AtomicU64::new(0));
        let (ready, waiting) = std::sync::mpsc::sync_channel(1);
        let s = stop.clone();
        let f = fault.clone();
        let count = frames.clone();
        let worker = std::thread::Builder::new()
            .name("windfall-input-writer".into())
            .spawn(move || {
                let opened = open(&source, rate, f.clone());
                let (stream, mut queue) = match opened {
                    Ok(value) => {
                        let _ = ready.send(Ok(()));
                        value
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error.clone()));
                        return Err(error);
                    }
                };
                let mut block = [0.0_f32; 2048];
                let mut total = 0;
                loop {
                    let mut n = 0;
                    while n < block.len() {
                        match queue.pop() {
                            Ok(frame) => {
                                block[n] = frame[0];
                                block[n + 1] = frame[1];
                                n += 2;
                            }
                            Err(_) => break,
                        }
                    }
                    if n > 0 {
                        if let Err(error) = sink(&block[..n]) {
                            f.store(true, Ordering::Release);
                            return Err(error);
                        }
                        total += (n / 2) as u64;
                        count.store(total, Ordering::Relaxed);
                    }
                    if f.load(Ordering::Acquire) {
                        return Err(f.describe("The take was discarded."));
                    }
                    if s.load(Ordering::Acquire) {
                        break;
                    }
                    if n == 0 {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                // Dropping closes callbacks before draining the final queued frames.
                drop(stream);
                while let Ok(frame) = queue.pop() {
                    sink(&frame)?;
                    total += 1;
                }
                if f.load(Ordering::Acquire) {
                    return Err(f.describe("The take was discarded."));
                }
                Ok(total)
            })
            .map_err(|e| e.to_string())?;
        match waiting.recv() {
            Ok(Ok(())) => Ok(Self {
                stop,
                fault,
                frames,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err("Input worker stopped while opening the device.".into())
            }
        }
    }
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }
    pub fn failed(&self) -> bool {
        self.fault.load(Ordering::Acquire)
            || self.worker.as_ref().is_some_and(JoinHandle::is_finished)
    }
    pub fn finish(mut self) -> Result<u64, String> {
        self.stop.store(true, Ordering::Release);
        self.worker
            .take()
            .expect("worker present")
            .join()
            .map_err(|_| "Recording writer stopped unexpectedly.".to_owned())?
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Device enumeration never opens a microphone stream.
pub fn recording_inputs() -> Vec<RecordingInput> {
    let mut found = Vec::new();
    for id in cpal::available_hosts() {
        let Ok(host) = cpal::host_from_id(id) else {
            continue;
        };
        let Ok(devices) = host.input_devices() else {
            continue;
        };
        for device in devices {
            let Ok(name) = device.description().map(|d| d.name().to_owned()) else {
                continue;
            };
            let Ok(configs) = device.supported_input_configs() else {
                continue;
            };
            let mut channels = 0;
            let mut rates = Vec::new();
            for config in configs {
                if !matches!(
                    config.sample_format(),
                    SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16 | SampleFormat::I32
                ) {
                    continue;
                }
                channels = channels.max(config.channels());
                for rate in [44100, 48000, 88200, 96000, 192000] {
                    if rate >= config.min_sample_rate()
                        && rate <= config.max_sample_rate()
                        && !rates.contains(&rate)
                    {
                        rates.push(rate)
                    }
                }
            }
            if channels > 0 {
                found.push(RecordingInput {
                    host: id.name().into(),
                    device: name,
                    channels,
                    sample_rates: rates,
                });
            }
        }
        // What plays on an output device, captured through its loopback. It
        // has the one rate the device mixes at.
        for (name, format) in crate::recording_endpoint::loopback_inputs(&host) {
            if matches!(
                format.sample_format(),
                SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16 | SampleFormat::I32
            ) {
                found.push(RecordingInput {
                    host: id.name().into(),
                    device: name,
                    channels: format.channels(),
                    sample_rates: vec![format.min_sample_rate()],
                });
            }
        }
    }
    found
}
struct Feed {
    queue: Producer<[f32; 2]>,
    channels: usize,
    left: usize,
    right: usize,
    fault: Arc<InputFault>,
}
impl Feed {
    fn push<T: SizedSample>(&mut self, data: &[T])
    where
        f32: FromSample<T>,
    {
        if self.fault.load(Ordering::Relaxed) {
            return;
        }
        if !data.len().is_multiple_of(self.channels) {
            self.fault.raise(FaultReason::InvalidAudio);
            return;
        }
        for frame in data.chunks_exact(self.channels) {
            let a = f32::from_sample(frame[self.left]);
            let b = f32::from_sample(frame[self.right]);
            if !a.is_finite() || !b.is_finite() {
                self.fault.raise(FaultReason::InvalidAudio);
                break;
            }
            if self.queue.push([a, b]).is_err() {
                self.fault.raise(FaultReason::Overflow);
                break;
            }
        }
    }
}
fn open(
    source: &RecordingSource,
    rate: u32,
    fault: Arc<InputFault>,
) -> Result<(cpal::Stream, rtrb::Consumer<[f32; 2]>), String> {
    let id = cpal::available_hosts()
        .into_iter()
        .find(|id| id.name() == source.host)
        .ok_or("Input host is unavailable.")?;
    let host = cpal::host_from_id(id).map_err(|e| e.to_string())?;
    let endpoint = crate::recording_endpoint::find_input(&host, &source.device)?
        .ok_or("Input device is unavailable.")?;
    let configs = crate::recording_endpoint::input_configs(&endpoint)?;
    let device = endpoint.device;
    let right = source.right.unwrap_or(source.left);
    let config = configs.into_iter().find(|c| {
        c.channels() > source.left.max(right) && rate >= c.min_sample_rate() && rate <= c.max_sample_rate()
        && matches!(c.sample_format(), SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16 | SampleFormat::I32)
    }).ok_or("The input does not support the output sample rate and selected channels. Choose matching audio settings.")?.with_sample_rate(rate);
    let (queue, consumer) = RingBuffer::new((rate as usize).saturating_mul(2));
    let feed = Feed {
        queue,
        channels: usize::from(config.channels()),
        left: usize::from(source.left),
        right: usize::from(right),
        fault: fault.clone(),
    };
    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config.config(), feed, fault),
        SampleFormat::I16 => build::<i16>(&device, &config.config(), feed, fault),
        SampleFormat::U16 => build::<u16>(&device, &config.config(), feed, fault),
        SampleFormat::I32 => build::<i32>(&device, &config.config(), feed, fault),
        _ => unreachable!(),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, consumer))
}
fn build<T: SizedSample>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut feed: Feed,
    fault: Arc<InputFault>,
) -> Result<cpal::Stream, String>
where
    f32: FromSample<T>,
{
    let startup = Arc::new(crate::recording_endpoint::InputStartup::new(
        device.id().map(|id| id.host().name()).unwrap_or(""),
    ));
    let data_startup = startup.clone();
    device
        .build_input_stream(
            *config,
            move |data: &[T], _| {
                data_startup.received();
                feed.push(data);
            },
            move |error: cpal::Error| startup.report(&fault, error),
            None,
        )
        .map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converts_selected_channels_and_refuses_overflow() {
        let (queue, mut out) = RingBuffer::new(2);
        let fault = Arc::new(InputFault::new());
        let mut feed = Feed {
            queue,
            channels: 3,
            left: 1,
            right: 2,
            fault: fault.clone(),
        };
        feed.push(&[0_i16, i16::MAX, i16::MIN, 0, 0, 0]);
        assert_eq!(out.pop().unwrap(), [32767.0 / 32768.0, -1.0]);
        assert_eq!(out.pop().unwrap(), [0.0, 0.0]);
        feed.push(&[0_i16; 9]);
        assert!(fault.load(Ordering::Acquire));
        assert_eq!(fault.reason(), Some(FaultReason::Overflow));
        assert_eq!(out.slots(), 2);
    }
    #[test]
    fn unsigned_and_signed_32_bit_conversion_and_incomplete_frame_failure() {
        let (queue, mut out) = RingBuffer::new(4);
        let fault = Arc::new(InputFault::new());
        let mut feed = Feed {
            queue,
            channels: 2,
            left: 0,
            right: 1,
            fault: fault.clone(),
        };
        feed.push(&[0_u16, u16::MAX]);
        assert_eq!(out.pop().unwrap(), [-1.0, 32767.0 / 32768.0]);
        feed.push(&[i32::MIN, 0_i32]);
        assert_eq!(out.pop().unwrap(), [-1.0, 0.0]);
        feed.push(&[0.0_f32]);
        assert!(fault.load(Ordering::Acquire));
        assert_eq!(fault.reason(), Some(FaultReason::InvalidAudio));
    }
    #[test]
    fn mono_duplicates_without_allocating() {
        let (queue, mut out) = RingBuffer::new(4);
        let mut feed = Feed {
            queue,
            channels: 2,
            left: 1,
            right: 1,
            fault: Arc::new(InputFault::new()),
        };
        assert_eq!(
            crate::test_alloc::allocator_calls(|| feed.push(&[0.0_f32, 0.25, 0.0, -0.5])),
            0
        );
        assert_eq!(out.pop().unwrap(), [0.25, 0.25]);
        assert_eq!(out.pop().unwrap(), [-0.5, -0.5]);
    }
}
