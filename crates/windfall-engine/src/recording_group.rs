//! One timestamped input stream per device, many aligned route writers.
use super::*;
use std::collections::BTreeMap;

/// Stereo sample blocks delivered to a route's off-callback writer.
pub type RecordingSink = Box<dyn FnMut(&[f32]) -> Result<(), String> + Send>;
/// A route index, its packet consumer and the device's negotiated input rate.
type InputRouteQueue = (usize, rtrb::Consumer<Packet>, u32);
/// Shared device streams and their independently queued recording routes.
type OpenedInputGroup = (Vec<cpal::Stream>, Vec<InputRouteQueue>);

pub struct CaptureRoute {
    pub source: RecordingSource,
    pub metrics: Arc<AlignmentMetrics>,
    pub monitor: Option<crate::recording_monitor::MonitorWriter>,
    pub sink: RecordingSink,
    pub disk: Option<crate::recording_disk::DiskReader>,
}
struct DiskChannel {
    reader: crate::recording_disk::DiskReader,
    metrics: Arc<AlignmentMetrics>,
    sink: RecordingSink,
}
struct Channel {
    queue: rtrb::Consumer<Packet>,
    aligner: Aligner,
    monitor: Option<crate::recording_monitor::MonitorWriter>,
    sink: RecordingSink,
}
impl Channel {
    fn consume(&mut self, packet: &Packet) -> Result<(), String> {
        self.aligner.push(packet, &mut self.sink)?;
        if let Some(monitor) = &mut self.monitor {
            let period = self
                .aligner
                .gate
                .clock()
                .output()
                .map_or(1e9 / f64::from(self.aligner.output_rate), |clock| {
                    clock.nanos_per_frame
                });
            monitor.push(&packet.audio[..packet.len], period / self.aligner.period);
        }
        Ok(())
    }
    fn tail_ready(&self) -> Result<bool, String> {
        let end = self.aligner.gate.window()?.end as f64;
        Ok(self.aligner.anchor.is_some()
            && self.aligner.input_time(self.aligner.received as f64)
                >= end
                    + self.aligner.options.offset_ms.max(0.0) * 1e6
                    + TAPS as f64 * self.aligner.period)
    }
}
impl Capture {
    /// Live monitoring opens shared input streams without a take, file or gate.
    /// The signature changes on project/input-route replacement; output clock
    /// replacement also ends the owned group rather than playing at an old rate.
    pub fn start_monitor_group(
        routes: Vec<CaptureRoute>,
        rate: u32,
        clock: RecordingClock,
        signature: Arc<AtomicU64>,
        expected: u64,
        controller: crate::Controller,
    ) -> Result<Self, String> {
        if routes.is_empty() || routes.len() > windfall_project::MAX_MIXER_TRACKS {
            return Err("Enable monitoring on at least one mixer input.".into());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let fault = Arc::new(InputFault::new());
        let frames = Arc::new(AtomicU64::new(0));
        let (ready, waiting) = std::sync::mpsc::sync_channel(1);
        let s = stop.clone();
        let f = fault.clone();
        let count = frames.clone();
        let worker = std::thread::Builder::new()
            .name("windfall-live-inputs".into())
            .spawn(move || {
                struct OutputOwner(crate::Controller);
                impl Drop for OutputOwner {
                    fn drop(&mut self) {
                        self.0.stop_input_monitor();
                    }
                }
                let _owner = OutputOwner(controller);
                let opened = open_group(&routes, rate, clock.clone(), f.clone());
                let (streams, opened) = match opened {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = ready.send(Err(error.clone()));
                        return Err(error);
                    }
                };
                let mut routes: Vec<_> = routes.into_iter().map(Some).collect();
                let mut channels = opened
                    .into_iter()
                    .map(|(index, queue, input_rate)| {
                        let route = routes[index].take().expect("one queue per monitor route");
                        route.metrics.rate.store(input_rate, Ordering::Relaxed);
                        (
                            queue,
                            route,
                            input_rate,
                            1e9 / f64::from(input_rate),
                            None::<(u64, f64)>,
                            0_u64,
                        )
                    })
                    .collect::<Vec<_>>();
                let mut generation = clock.output().map(|output| output.generation);
                let _ = ready.send(Ok(()));
                loop {
                    if s.load(Ordering::Acquire) {
                        break;
                    }
                    if signature.load(Ordering::Acquire) != expected {
                        return Err(
                            "Input routes or project changed; start monitoring again.".into()
                        );
                    }
                    let output = clock.output();
                    if generation.is_none() {
                        generation = output.map(|output| output.generation);
                    }
                    if output.is_some_and(|output| {
                        output.sample_rate != rate
                            || generation.is_some_and(|before| before != output.generation)
                    }) {
                        return Err("Audio output changed; start monitoring again.".into());
                    }
                    if f.load(Ordering::Acquire) {
                        return Err(f.describe("Input monitoring stopped."));
                    }
                    let mut received = false;
                    for (queue, route, input_rate, period, origin, next) in &mut channels {
                        for _ in 0..32 {
                            let Ok(packet) = queue.pop() else {
                                break;
                            };
                            received = true;
                            if packet.frame != *next {
                                return Err(
                                    "Input monitoring frame sequence was interrupted.".into()
                                );
                            }
                            let measured = packet.capture_ns as f64;
                            if let Some((frame, time)) = *origin {
                                let elapsed = packet.frame.saturating_sub(frame);
                                if elapsed >= u64::from(*input_rate) && packet.measured {
                                    let nominal = 1e9 / f64::from(*input_rate);
                                    let observed = (measured - time) / elapsed as f64;
                                    if !observed.is_finite()
                                        || !(nominal * 0.98..=nominal * 1.02).contains(&observed)
                                    {
                                        return Err(
                                            "Input monitoring clock was interrupted.".into()
                                        );
                                    }
                                    *period += (observed.clamp(nominal * 0.995, nominal * 1.005)
                                        - *period)
                                        * 0.02;
                                }
                            } else {
                                *origin = Some((packet.frame, measured));
                            }
                            *next += packet.len as u64;
                            route
                                .metrics
                                .input_latency
                                .store(packet.latency_ns, Ordering::Relaxed);
                            route.metrics.output_latency.store(
                                output.map_or(0, |output| output.latency_nanos),
                                Ordering::Relaxed,
                            );
                            route.metrics.measured.store(
                                packet.measured && output.is_some_and(|output| output.measured),
                                Ordering::Relaxed,
                            );
                            route.metrics.drift.store(
                                ((*period * f64::from(*input_rate) / 1e9 - 1.0) * -1e6).to_bits(),
                                Ordering::Relaxed,
                            );
                            if let Some(monitor) = &mut route.monitor {
                                let output_period = output
                                    .map_or(1e9 / f64::from(rate), |output| output.nanos_per_frame);
                                monitor.push(&packet.audio[..packet.len], output_period / *period);
                            }
                        }
                    }
                    count.store(
                        channels.iter().map(|channel| channel.5).min().unwrap_or(0),
                        Ordering::Relaxed,
                    );
                    if !received {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                drop(streams);
                Ok(count.load(Ordering::Relaxed))
            })
            .map_err(|error| error.to_string())?;
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
                Err("Input monitor worker stopped while opening devices.".into())
            }
        }
    }

    pub fn start_group(
        routes: Vec<CaptureRoute>,
        rate: u32,
        gate: CaptureGate,
    ) -> Result<Self, String> {
        if routes.is_empty() || routes.len() > windfall_project::MAX_MIXER_TRACKS {
            return Err("Arm between one track and the mixer's recording capacity.".into());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let fault = Arc::new(InputFault::new());
        let frames = Arc::new(AtomicU64::new(0));
        let (ready, waiting) = std::sync::mpsc::sync_channel(1);
        let s = stop.clone();
        let f = fault.clone();
        let count = frames.clone();
        let worker = std::thread::Builder::new().name("windfall-multitrack-input".into()).spawn(move || {
            let mut hardware = Vec::new(); let mut disks = Vec::new();
            for route in routes {
                let CaptureRoute { mut source, metrics, monitor, sink, disk } = route;
                if let Some(reader) = disk {
                    metrics.rate.store(rate, Ordering::Relaxed);
                    metrics.measured.store(true, Ordering::Relaxed);
                    disks.push(DiskChannel { reader, metrics, sink });
                    if monitor.is_some() {
                        source.mixer_tap = None;
                        hardware.push(CaptureRoute { source, metrics: Arc::new(AlignmentMetrics::default()), monitor, sink: Box::new(|_| Ok(())), disk: None });
                    }
                } else { hardware.push(CaptureRoute { source, metrics, monitor, sink, disk: None }); }
            }
            let routes = hardware;
            let opened = open_group(&routes, rate, gate.clock(), f.clone());
            let (streams, opened) = match opened {
                Ok(value) => value,
                Err(error) => { let _ = ready.send(Err(error.clone())); return Err(error); }
            };
            if f.load(Ordering::Acquire) {
                let error = "An input failed while the recording group was opening.".to_owned();
                let _ = ready.send(Err(error.clone())); return Err(error);
            }
            let mut routes: Vec<_> = routes.into_iter().map(Some).collect();
            let mut channels: Vec<_> = opened.into_iter().map(|(index, queue, input_rate)| {
                let route = routes[index].take().expect("one opened queue per route");
                route.metrics.rate.store(input_rate, Ordering::Relaxed);
                Channel { queue, aligner: Aligner::new(input_rate, rate, gate.clone(), route.source.alignment.unwrap_or_default(), route.metrics), monitor: route.monitor, sink: route.sink }
            }).collect();
            let _ = ready.send(Ok(()));
            let mut stopping = None;
            loop {
                let mut received = false;
                for channel in &mut channels {
                    // Bound each route's visit so a busy stream cannot starve
                    // another device or defer stop indefinitely.
                    for _ in 0..32 {
                        let Ok(packet) = channel.queue.pop() else { break; };
                        received = true;
                        if let Err(error) = channel.consume(&packet) { f.store(true, Ordering::Release); return Err(error); }
                    }
                }
                for channel in &mut disks {
                    if let Err(error) = channel.reader.pump(&mut channel.sink).map(|value| received |= value) { f.store(true, Ordering::Release); return Err(error); }
                    channel.metrics.output_latency.store(gate.clock().output().map_or(0, |clock| clock.latency_nanos), Ordering::Relaxed);
                }
                count.store(channels.iter().map(|channel| channel.aligner.received).chain(disks.iter().map(|channel| channel.reader.frames())).min().unwrap_or(0), Ordering::Relaxed);
                if f.load(Ordering::Acquire) { return Err(f.describe("All tracks in the take were discarded.")); }
                if s.load(Ordering::Acquire) {
                    if stopping.is_none() { gate.close(); stopping = Some(std::time::Instant::now()); }
                    let mut complete = true;
                    for channel in &channels { complete &= channel.tail_ready()?; }
                    for channel in &disks { complete &= channel.reader.tail_ready()?; }
                    if complete { break; }
                    if stopping.is_some_and(|time| time.elapsed() >= Duration::from_secs(2)) { return Err("A recording device did not deliver its final aligned samples; the take group was discarded.".into()); }
                }
                if !received { std::thread::sleep(Duration::from_millis(2)); }
            }
            drop(streams);
            for channel in &mut disks {
                channel.reader.close();
                while channel.reader.pump(&mut channel.sink)? {}
            }
            for channel in &mut channels {
                while let Ok(packet) = channel.queue.pop() { channel.consume(&packet)?; }
                channel.aligner.finish(&mut channel.sink)?;
            }
            if f.load(Ordering::Acquire) { return Err(f.describe("The take group was discarded.")); }
            Ok(channels.iter().map(|channel| channel.aligner.received).chain(disks.iter().map(|channel| channel.reader.frames())).min().unwrap_or(0))
        }).map_err(|error| error.to_string())?;
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
                Err("Multitrack writer stopped while opening inputs.".into())
            }
        }
    }
}
fn open_group(
    routes: &[CaptureRoute],
    rate: u32,
    clock: RecordingClock,
    fault: Arc<InputFault>,
) -> Result<OpenedInputGroup, String> {
    let mut groups = BTreeMap::<(String, String), Vec<usize>>::new();
    for (index, route) in routes.iter().enumerate() {
        groups
            .entry((route.source.host.clone(), route.source.device.clone()))
            .or_default()
            .push(index);
    }
    let mut streams = Vec::new();
    let mut queues = Vec::new();
    for ((host_name, device_name), indices) in groups {
        let id = cpal::available_hosts()
            .into_iter()
            .find(|id| id.name() == host_name)
            .ok_or_else(|| format!("Input host {host_name} is unavailable."))?;
        let host = cpal::host_from_id(id).map_err(|error| error.to_string())?;
        let endpoint = crate::recording_endpoint::find_input(&host, &device_name)?
            .ok_or_else(|| format!("Input device {device_name} is unavailable."))?;
        let configs = crate::recording_endpoint::input_configs(&endpoint)?;
        let device = endpoint.device;
        let highest = indices
            .iter()
            .map(|index| {
                routes[*index].source.left.max(
                    routes[*index]
                        .source
                        .right
                        .unwrap_or(routes[*index].source.left),
                )
            })
            .max()
            .unwrap_or(0);
        let preferred = routes[indices[0]]
            .source
            .alignment
            .as_ref()
            .and_then(|alignment| alignment.input_sample_rate)
            .unwrap_or(rate);
        let config = configs
            .into_iter()
            .filter(|config| {
                config.channels() > highest
                    && matches!(
                        config.sample_format(),
                        SampleFormat::F32
                            | SampleFormat::I16
                            | SampleFormat::U16
                            | SampleFormat::I32
                    )
            })
            .map(|config| {
                let chosen = preferred.clamp(config.min_sample_rate(), config.max_sample_rate());
                (chosen.abs_diff(preferred), config.with_sample_rate(chosen))
            })
            .min_by_key(|(distance, _)| *distance)
            .map(|(_, config)| config)
            .ok_or_else(|| format!("{device_name} has no format supporting all armed channels."))?;
        let rate = config.sample_rate();
        let mut feeds = Vec::with_capacity(indices.len());
        for index in indices {
            let source = &routes[index].source;
            let (producer, consumer) =
                RingBuffer::new((rate as usize * 2).div_ceil(PACKET_FRAMES).max(16));
            feeds.push((
                usize::from(source.left),
                usize::from(source.right.unwrap_or(source.left)),
                producer,
            ));
            queues.push((index, consumer, rate));
        }
        let feed = GroupFeed {
            feeds,
            channels: usize::from(config.channels()),
            clock: clock.clone(),
            rate,
            frame: 0,
            fault: fault.clone(),
            timebase: crate::recording_clock::DriverTimebase::default(),
        };
        let stream = match config.sample_format() {
            SampleFormat::F32 => build_group::<f32>(&device, &config.config(), feed, fault.clone()),
            SampleFormat::I16 => build_group::<i16>(&device, &config.config(), feed, fault.clone()),
            SampleFormat::U16 => build_group::<u16>(&device, &config.config(), feed, fault.clone()),
            SampleFormat::I32 => build_group::<i32>(&device, &config.config(), feed, fault.clone()),
            _ => unreachable!(),
        }?;
        stream.play().map_err(|error| error.to_string())?;
        streams.push(stream);
    }
    Ok((streams, queues))
}
struct GroupFeed {
    feeds: Vec<(usize, usize, Producer<Packet>)>,
    channels: usize,
    clock: RecordingClock,
    rate: u32,
    frame: u64,
    fault: Arc<InputFault>,
    timebase: crate::recording_clock::DriverTimebase,
}
impl GroupFeed {
    fn push<T: SizedSample>(&mut self, data: &[T], info: &cpal::InputCallbackInfo)
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
        let (captured, latency_ns, measured) = self.timebase.capture(info.timestamp(), &self.clock);
        for (chunk_index, chunk) in data.chunks(PACKET_FRAMES * self.channels).enumerate() {
            let len = chunk.len() / self.channels;
            let capture_ns = captured.saturating_add(
                chunk_index as u64 * PACKET_FRAMES as u64 * 1_000_000_000 / u64::from(self.rate),
            );
            for (left, right, queue) in &mut self.feeds {
                let mut packet = Packet {
                    audio: [[0.0; 2]; PACKET_FRAMES],
                    len,
                    frame: self.frame,
                    capture_ns,
                    latency_ns,
                    measured,
                };
                for (output, input) in packet
                    .audio
                    .iter_mut()
                    .zip(chunk.chunks_exact(self.channels))
                {
                    *output = [
                        f32::from_sample(input[*left]),
                        f32::from_sample(input[*right]),
                    ];
                    if !output[0].is_finite() || !output[1].is_finite() {
                        self.fault.raise(FaultReason::InvalidAudio);
                        return;
                    }
                }
                if queue.push(packet).is_err() {
                    self.fault.raise(FaultReason::Overflow);
                    return;
                }
            }
            self.frame += len as u64;
        }
    }
}
fn build_group<T: SizedSample>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut feed: GroupFeed,
    fault: Arc<InputFault>,
) -> Result<cpal::Stream, String>
where
    f32: FromSample<T>,
{
    let timebase = feed.timebase.clone();
    let clock = feed.clock.clone();
    let startup = Arc::new(crate::recording_endpoint::InputStartup::new(
        device.id().map(|id| id.host().name()).unwrap_or(""),
    ));
    let data_startup = startup.clone();
    let stream = device
        .build_input_stream(
            *config,
            move |data: &[T], info| {
                data_startup.received();
                feed.push(data, info);
            },
            move |error: cpal::Error| startup.report(&fault, error),
            None,
        )
        .map_err(|error| error.to_string())?;
    timebase.initialize(&stream, &clock);
    Ok(stream)
}
