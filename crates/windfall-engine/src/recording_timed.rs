//! Timestamped ADC packets and worker-side conversion to the output time grid.
use super::*;
use crate::recording_clock::{CaptureGate, RecordingClock, nanos};
use std::sync::atomic::AtomicU32;
use windfall_ipc::{RecordingAlignment, RecordingAlignmentStatus};

#[path = "recording_group.rs"]
mod group;
pub use group::CaptureRoute;

const PACKET_FRAMES: usize = 256;
const TAPS: usize = 32;
const HISTORY: usize = 128;
struct Packet {
    audio: [[f32; 2]; PACKET_FRAMES],
    len: usize,
    frame: u64,
    capture_ns: u64,
    latency_ns: u64,
    measured: bool,
}
#[derive(Default)]
pub struct AlignmentMetrics {
    rate: AtomicU32,
    input_latency: AtomicU64,
    output_latency: AtomicU64,
    drift: AtomicU64,
    measured: AtomicBool,
    trimmed: AtomicU64,
}
impl AlignmentMetrics {
    pub fn snapshot(&self) -> RecordingAlignmentStatus {
        RecordingAlignmentStatus {
            input_sample_rate: self.rate.load(Ordering::Relaxed),
            input_latency_ms: self.input_latency.load(Ordering::Relaxed) as f64 / 1e6,
            output_latency_ms: self.output_latency.load(Ordering::Relaxed) as f64 / 1e6,
            drift_ppm: f64::from_bits(self.drift.load(Ordering::Relaxed)),
            measured_timestamps: self.measured.load(Ordering::Relaxed),
            trimmed_frames: self.trimmed.load(Ordering::Relaxed),
        }
    }
}
impl Capture {
    /// Input opens before the caller starts transport. Only this worker performs
    /// resampling, gate trimming, drift fitting or WAV writes.
    pub fn start_timed<F>(source: RecordingSource, rate: u32, gate: CaptureGate, metrics: Arc<AlignmentMetrics>, mut monitor: Option<crate::recording_monitor::MonitorWriter>, mut sink: F) -> Result<Self, String>
    where F: FnMut(&[f32]) -> Result<(), String> + Send + 'static {
        let stop = Arc::new(AtomicBool::new(false));
        let fault = Arc::new(InputFault::new());
        let frames = Arc::new(AtomicU64::new(0));
        let (ready, waiting) = std::sync::mpsc::sync_channel(1);
        let s = stop.clone(); let f = fault.clone(); let count = frames.clone();
        let worker = std::thread::Builder::new().name("windfall-timed-input".into()).spawn(move || {
            let options = source.alignment.clone().unwrap_or_default();
            let (stream, mut queue, input_rate) = match open_timed(&source, rate, gate.clock(), f.clone()) {
                Ok(value) => value,
                Err(error) => { let _ = ready.send(Err(error.clone())); return Err(error); }
            };
            metrics.rate.store(input_rate, Ordering::Relaxed);
            let mut aligner = Aligner::new(input_rate, rate, gate, options, metrics);
            let _ = ready.send(Ok(()));
            let mut total = 0_u64;
            let mut stopping = None;
            loop {
                let mut received = false;
                while let Ok(packet) = queue.pop() {
                    received = true;
                    if let Err(error) = aligner.push(&packet, &mut sink) { f.store(true, Ordering::Release); return Err(error); }
                    if let Some(monitor) = &mut monitor {
                        let output_period = aligner.gate.clock().output().map_or(1e9 / f64::from(rate), |clock| clock.nanos_per_frame);
                        monitor.push(&packet.audio[..packet.len], output_period / aligner.period);
                    }
                    total += packet.len as u64; count.store(total, Ordering::Relaxed);
                }
                if f.load(Ordering::Acquire) { return Err(f.describe("The take was discarded so nothing is kept out of time.")); }
                if s.load(Ordering::Acquire) {
                    if stopping.is_none() { aligner.gate.close(); stopping = Some(std::time::Instant::now()); }
                    let end = aligner.gate.window()?.end as f64;
                    let required = end + aligner.options.offset_ms.max(0.0) * 1e6 + TAPS as f64 * aligner.period;
                    if aligner.anchor.is_some() && aligner.input_time(aligner.received as f64) >= required { break; }
                    if stopping.is_some_and(|at| at.elapsed() >= Duration::from_secs(2)) {
                        return Err("Input did not deliver the recording tail before closing; take discarded.".into());
                    }
                }
                if !received { std::thread::sleep(Duration::from_millis(2)); }
            }
            drop(stream);
            while let Ok(packet) = queue.pop() { aligner.push(&packet, &mut sink)?; total += packet.len as u64; }
            aligner.finish(&mut sink)?;
            if f.load(Ordering::Acquire) { return Err(f.describe("The take was discarded.")); }
            count.store(total, Ordering::Relaxed);
            Ok(total)
        }).map_err(|e| e.to_string())?;
        match waiting.recv() {
            Ok(Ok(())) => Ok(Self { stop, fault, frames, worker: Some(worker) }),
            Ok(Err(error)) => { let _ = worker.join(); Err(error) },
            Err(_) => { let _ = worker.join(); Err("Input worker stopped while opening the device.".into()) },
        }
    }
}
struct TimedFeed {
    queue: Producer<Packet>, channels: usize, left: usize, right: usize,
    clock: RecordingClock, rate: u32, frame: u64, fault: Arc<InputFault>,
}
impl TimedFeed {
    fn push<T: SizedSample>(&mut self, data: &[T], info: &cpal::InputCallbackInfo) where f32: FromSample<T> {
        if self.fault.load(Ordering::Relaxed) { return; }
        if !data.len().is_multiple_of(self.channels) { self.fault.raise(FaultReason::InvalidAudio); return; }
        let stamp = info.timestamp();
        let latency = stamp.callback.checked_duration_since(stamp.capture);
        let latency_ns = latency.map_or(0, nanos);
        let captured = self.clock.now_nanos().saturating_sub(latency_ns);
        for (index, chunk) in data.chunks(PACKET_FRAMES * self.channels).enumerate() {
            let len = chunk.len() / self.channels;
            let mut packet = Packet {
                audio: [[0.0; 2]; PACKET_FRAMES], len, frame: self.frame,
                capture_ns: captured.saturating_add((index as u64 * PACKET_FRAMES as u64 * 1_000_000_000) / u64::from(self.rate)),
                latency_ns, measured: latency.is_some(),
            };
            for (out, frame) in packet.audio.iter_mut().zip(chunk.chunks_exact(self.channels)) {
                *out = [f32::from_sample(frame[self.left]), f32::from_sample(frame[self.right])];
                if !out[0].is_finite() || !out[1].is_finite() { self.fault.raise(FaultReason::InvalidAudio); return; }
            }
            self.frame = self.frame.saturating_add(len as u64);
            if self.queue.push(packet).is_err() { self.fault.raise(FaultReason::Overflow); return; }
        }
    }
}
fn open_timed(source: &RecordingSource, output_rate: u32, clock: RecordingClock, fault: Arc<InputFault>) -> Result<(cpal::Stream, rtrb::Consumer<Packet>, u32), String> {
    let id = cpal::available_hosts().into_iter().find(|id| id.name() == source.host).ok_or("Input host is unavailable.")?;
    let host = cpal::host_from_id(id).map_err(|e| e.to_string())?;
    let endpoint = crate::recording_endpoint::find_input(&host, &source.device)?.ok_or("Input device is unavailable.")?;
    let configs = crate::recording_endpoint::input_configs(&endpoint)?;
    let device = endpoint.device;
    let right = source.right.unwrap_or(source.left);
    let preferred = source.alignment.as_ref().and_then(|a| a.input_sample_rate).unwrap_or(output_rate);
    let config = configs.into_iter()
        .filter(|c| c.channels() > source.left.max(right) && matches!(c.sample_format(), SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16 | SampleFormat::I32))
        .map(|c| { let rate = preferred.clamp(c.min_sample_rate(), c.max_sample_rate()); (rate.abs_diff(preferred), c.with_sample_rate(rate)) })
        .min_by_key(|(distance, _)| *distance).map(|(_, config)| config).ok_or("Input has no supported format for the selected channels.")?;
    let rate = config.sample_rate();
    let (queue, consumer) = RingBuffer::new((rate as usize * 2).div_ceil(PACKET_FRAMES).max(16));
    let feed = TimedFeed { queue, channels: usize::from(config.channels()), left: usize::from(source.left), right: usize::from(right), clock, rate, frame: 0, fault: fault.clone() };
    let stream = match config.sample_format() {
        SampleFormat::F32 => build_timed::<f32>(&device, &config.config(), feed, fault),
        SampleFormat::I16 => build_timed::<i16>(&device, &config.config(), feed, fault),
        SampleFormat::U16 => build_timed::<u16>(&device, &config.config(), feed, fault),
        SampleFormat::I32 => build_timed::<i32>(&device, &config.config(), feed, fault),
        _ => unreachable!(),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, consumer, rate))
}
fn build_timed<T: SizedSample>(device: &cpal::Device, config: &cpal::StreamConfig, mut feed: TimedFeed, fault: Arc<InputFault>) -> Result<cpal::Stream, String> where f32: FromSample<T> {
    device.build_input_stream(*config, move |data: &[T], info| feed.push(data, info), move |error: cpal::Error| fault.report(error), None).map_err(|e| e.to_string())
}

/// A bounded circular history with a symmetric windowed-sinc interpolator.
/// The history retains pre-roll too, so the first audible frame has both sides
/// of its interpolation kernel. Input callback jitter is fitted on the worker.
struct Aligner {
    gate: CaptureGate, options: RecordingAlignment, metrics: Arc<AlignmentMetrics>,
    input_rate: u32, output_rate: u32, period: f64,
    anchor: Option<(u64, f64)>, fit_origin: Option<(u64, f64)>,
    history: [[f32; 2]; HISTORY], seen: u64, received: u64,
    origin: Option<f64>, output_index: u64, output: [f32; 2048], used: usize,
}
impl Aligner {
    fn new(input_rate: u32, output_rate: u32, gate: CaptureGate, options: RecordingAlignment, metrics: Arc<AlignmentMetrics>) -> Self {
        Self { gate, options, metrics, input_rate, output_rate, period: 1e9 / f64::from(input_rate), anchor: None, fit_origin: None,
            history: [[0.0; 2]; HISTORY], seen: 0, received: 0, origin: None, output_index: 0, output: [0.0; 2048], used: 0 }
    }
    fn push<F>(&mut self, packet: &Packet, sink: &mut F) -> Result<(), String> where F: FnMut(&[f32]) -> Result<(), String> {
        self.gate.window()?;
        if packet.frame != self.received { return Err("Recording input frame sequence was interrupted; take discarded.".into()); }
        let measured = packet.capture_ns as f64;
        let mut fitted = measured;
        if let Some((frame, time)) = self.anchor {
            let predicted = time + packet.frame.saturating_sub(frame) as f64 * self.period;
            let residual = measured - predicted;
            // A device timestamp discontinuity cannot become a silent edit.
            if residual.abs() > 250_000_000.0 { return Err("Recording input clock jumped; take discarded.".into()); }
            if self.options.drift_correction && packet.measured {
                if let Some((first_frame, first_time)) = self.fit_origin {
                    let elapsed = packet.frame.saturating_sub(first_frame);
                    if elapsed >= u64::from(self.input_rate) {
                        let nominal = 1e9 / f64::from(self.input_rate);
                        let observed = ((measured - first_time) / elapsed as f64).clamp(nominal * 0.995, nominal * 1.005);
                        self.period += (observed - self.period) * 0.02;
                    }
                }
                fitted = predicted + residual.clamp(-100_000.0, 100_000.0) * 0.1;
            } else { fitted = predicted; }
        } else { self.fit_origin = Some((packet.frame, measured)); }
        self.anchor = Some((packet.frame, fitted));
        self.metrics.input_latency.store(packet.latency_ns, Ordering::Relaxed);
        let output_clock = self.gate.clock().output();
        self.metrics.output_latency.store(output_clock.map_or(0, |c| c.latency_nanos), Ordering::Relaxed);
        self.metrics.measured.store(packet.measured && output_clock.is_some_and(|c| c.measured), Ordering::Relaxed);
        self.metrics.drift.store(((self.period * f64::from(self.input_rate) / 1e9 - 1.0) * -1e6).to_bits(), Ordering::Relaxed);
        for audio in &packet.audio[..packet.len] {
            self.history[self.seen as usize % HISTORY] = *audio;
            self.seen += 1; self.received += 1;
            self.render(false, sink)?;
        }
        self.flush(sink)
    }
    fn input_time(&self, frame: f64) -> f64 { let (at, time) = self.anchor.expect("input timestamp present"); time + (frame - at as f64) * self.period }
    fn render<F>(&mut self, finishing: bool, sink: &mut F) -> Result<(), String> where F: FnMut(&[f32]) -> Result<(), String> {
        let window = self.gate.window()?;
        let Some(start) = window.start else { return Ok(()); };
        let offset = self.options.offset_ms * 1e6;
        let origin = if start == 0 {
            if self.origin.is_none() { self.origin = Some(self.input_time(0.0)); }
            self.origin.expect("manual capture origin present")
        } else { start as f64 };
        let earliest = self.seen.saturating_sub(HISTORY as u64) as f64 + (TAPS / 2) as f64;
        if self.origin.is_none() && start > 0 {
            let first_time = self.input_time(0.0) - offset;
            // Opening the input before playback normally supplies this range.
            // Any actual missing prefix is retained as silence, never shortened.
            if first_time > origin {
                let missing = ((first_time - origin) * f64::from(self.output_rate) / 1e9).ceil() as u64;
                for _ in 0..missing { self.emit([0.0; 2], sink)?; self.output_index += 1; }
            }
            self.origin = Some(origin);
        }
        let (anchor_frame, anchor_time) = self.anchor.expect("input timestamp present");
        loop {
            let time = window.start_frame.and_then(|frame| self.gate.clock().output().map(|clock| clock.nanos_at(frame.saturating_add(self.output_index)) as f64))
                .unwrap_or(origin + self.output_index as f64 * 1e9 / f64::from(self.output_rate));
            if time >= window.end as f64 { break; }
            let position = anchor_frame as f64 + (time + offset - anchor_time) / self.period;
            if position < earliest && self.seen > HISTORY as u64 {
                // Count-in packets may have advanced beyond an unscheduled gate.
                // Fill a genuine unavailable range explicitly instead of moving
                // the remaining audio earlier in the playlist.
                self.emit([0.0; 2], sink)?; self.output_index += 1; continue;
            }
            if position < -(TAPS as f64) {
                self.emit([0.0; 2], sink)?; self.output_index += 1; continue;
            }
            if position + (TAPS / 2) as f64 >= self.seen as f64 && !finishing { break; }
            if position >= self.received as f64 { break; }
            let center = position.floor() as i64;
            let cutoff = (f64::from(self.output_rate) * self.period / 1e9).min(1.0) * 0.94;
            let mut audio = [0.0_f64; 2]; let mut weight_sum = 0.0;
            for tap in -(TAPS as i64 / 2) + 1..=TAPS as i64 / 2 {
                let at = center + tap;
                let distance = position - at as f64;
                let x = std::f64::consts::PI * distance * cutoff;
                let sinc = if x.abs() < 1e-9 { 1.0 } else { x.sin() / x };
                let window = 0.5 + 0.5 * (std::f64::consts::PI * distance / (TAPS / 2) as f64).cos();
                let weight = sinc * cutoff * window.max(0.0);
                weight_sum += weight;
                if at >= 0 && (at as u64) < self.received && (at as u64) >= self.seen.saturating_sub(HISTORY as u64) {
                    let sample = self.history[at as usize % HISTORY];
                    audio[0] += f64::from(sample[0]) * weight; audio[1] += f64::from(sample[1]) * weight;
                }
            }
            self.emit([(audio[0] / weight_sum.max(1e-9)) as f32, (audio[1] / weight_sum.max(1e-9)) as f32], sink)?;
            self.output_index += 1;
        }
        let trimmed = ((start as f64 + offset - self.input_time(0.0)).max(0.0) / self.period) as u64;
        self.metrics.trimmed.store(trimmed.min(self.received), Ordering::Relaxed);
        Ok(())
    }
    fn emit<F>(&mut self, audio: [f32; 2], sink: &mut F) -> Result<(), String> where F: FnMut(&[f32]) -> Result<(), String> {
        self.output[self.used..self.used + 2].copy_from_slice(&audio); self.used += 2;
        if self.used == self.output.len() { self.flush(sink)?; }
        Ok(())
    }
    fn flush<F>(&mut self, sink: &mut F) -> Result<(), String> where F: FnMut(&[f32]) -> Result<(), String> {
        if self.used > 0 { sink(&self.output[..self.used])?; self.used = 0; } Ok(())
    }
    fn finish<F>(&mut self, sink: &mut F) -> Result<(), String> where F: FnMut(&[f32]) -> Result<(), String> {
        if self.anchor.is_some() { self.render(true, sink)?; } self.flush(sink)
    }
}
