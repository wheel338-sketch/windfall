//! One native processing worker per plugin. Two prepared packets circulate
//! through bounded SPSC queues. Audio never waits, wakes an OS primitive,
//! allocates or disposes native ownership. One quantum of worker delay plus
//! one quantum of collection is reported to PDC. A deadline miss fails the
//! processor; explicit control-side replacement is required to recover.
use crate::{
    HostEvent, PluginEvent, Transport,
    processor::{
        AudioIo, BLOCK_EVENT_CAPACITY, BlockResult, ProcessFailed, ProcessStatus, ProcessorBackend,
    },
};
use std::{
    any::Any,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread::JoinHandle,
};

/// Only the plugin worker waits. Windows' ordinary sleep can round a 50-us
/// poll to a full system tick; use an existing-binding high-resolution timer
/// so that short prepared audio quanta do not fail solely because of that tick.
#[cfg(windows)]
struct WorkerWait(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl WorkerWait {
    fn new() -> Self {
        // SAFETY: worker-local unnamed auto-reset timer; no callback handle.
        Self(unsafe {
            windows_sys::Win32::System::Threading::CreateWaitableTimerExW(
                std::ptr::null(),
                std::ptr::null(),
                0x2,
                0x001F0003,
            )
        })
    }
    fn wait(&self) {
        use windows_sys::Win32::System::Threading::{SetWaitableTimer, WaitForSingleObject};
        let due: i64 = -500; // Relative 50 microseconds, in 100-ns units.
        if !self.0.is_null()
            && unsafe { SetWaitableTimer(self.0, &due, 0, None, std::ptr::null(), 0) } != 0
        {
            unsafe {
                WaitForSingleObject(self.0, 1);
            }
        } else {
            std::thread::yield_now();
        }
    }
}
#[cfg(windows)]
impl Drop for WorkerWait {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }
}
#[cfg(not(windows))]
struct WorkerWait;
#[cfg(not(windows))]
impl WorkerWait {
    fn new() -> Self {
        Self
    }
    fn wait(&self) {
        std::thread::sleep(std::time::Duration::from_micros(50));
    }
}

struct Packet {
    auxiliary: crate::outputs::OutputBank,
    input: [Box<[f32]>; 2],
    output: [Box<[f32]>; 2],
    key: Box<[[f32; 2]]>,
    events: Vec<HostEvent>,
    notifications: Vec<PluginEvent>,
    transport: Transport,
    steady: u64,
    epoch: u64,
    result: BlockResult,
    failed: bool,
    processed: bool,
    controls: u64,
    sidechain_input: Option<u32>,
}
impl Packet {
    fn new(block: usize) -> Self {
        Self {
            auxiliary: crate::outputs::OutputBank::new(block),
            input: std::array::from_fn(|_| vec![0.0; block].into_boxed_slice()),
            output: std::array::from_fn(|_| vec![0.0; block].into_boxed_slice()),
            key: vec![[0.0; 2]; block].into_boxed_slice(),
            events: Vec::with_capacity(BLOCK_EVENT_CAPACITY),
            notifications: Vec::with_capacity(BLOCK_EVENT_CAPACITY),
            transport: Transport::default(),
            steady: 0,
            epoch: 1,
            result: BlockResult {
                status: ProcessStatus::Continue,
                tail: None,
                dropped_events: 0,
            },
            failed: false,
            processed: false,
            controls: 0,
            sidechain_input: None,
        }
    }
    fn silence(&mut self) {
        self.auxiliary.clear();
        for output in &mut self.output {
            output.fill(0.0);
        }
        self.notifications.clear();
        self.result = BlockResult {
            status: ProcessStatus::Continue,
            tail: None,
            dropped_events: 0,
        };
        self.failed = false;
    }
}
struct Signals {
    exit: AtomicBool,
    reset: AtomicU64,
    stop: AtomicBool,
    input: AtomicU32,
    ready: AtomicBool,
    retiring: AtomicBool,
    completed_controls: AtomicU64,
}
struct Threaded {
    rendered: crate::outputs::OutputBank,
    send: rtrb::Producer<Packet>,
    receive: rtrb::Consumer<Packet>,
    local: Packet,
    spare: Option<Packet>,
    pending: bool,
    cursor: usize,
    epoch: u64,
    rate: f64,
    latency: u32,
    midi: bool,
    midi_masks: [u32; 16],
    instances: bool,
    masks: [u32; 16],
    signals: Arc<Signals>,
    worker: Option<JoinHandle<Box<dyn ProcessorBackend>>>,
    native: Option<Box<dyn ProcessorBackend>>,
    realtime: bool,
    failed: bool,
    controls: u64,
    failure_drops: u32,
}
pub(crate) fn wrap(
    native: Box<dyn ProcessorBackend>,
    block: usize,
    rate: f64,
) -> Box<dyn ProcessorBackend> {
    let midi = native.supports_midi_expression();
    let midi_masks = std::array::from_fn(|channel| native.midi_expression_mask(channel as u8));
    let instances = native.supports_note_instances();
    let masks = std::array::from_fn(|channel| native.note_expression_mask(channel as u8));
    let latency = native
        .additional_latency()
        .saturating_add((block as u32).saturating_mul(2));
    let signals = Arc::new(Signals {
        exit: AtomicBool::new(false),
        reset: AtomicU64::new(1),
        stop: AtomicBool::new(false),
        input: AtomicU32::new(u32::MAX),
        ready: AtomicBool::new(false),
        retiring: AtomicBool::new(false),
        completed_controls: AtomicU64::new(0),
    });
    let (send, receive, worker) = start(native, &signals);
    Box::new(Threaded {
        rendered: crate::outputs::OutputBank::new(block),
        send,
        receive,
        local: Packet::new(block),
        spare: Some(Packet::new(block)),
        pending: false,
        cursor: 0,
        epoch: 1,
        rate,
        latency,
        midi,
        midi_masks,
        instances,
        masks,
        signals,
        worker: Some(worker),
        native: None,
        realtime: true,
        failed: false,
        controls: 0,
        failure_drops: 0,
    })
}
fn start(
    mut native: Box<dyn ProcessorBackend>,
    signals: &Arc<Signals>,
) -> (
    rtrb::Producer<Packet>,
    rtrb::Consumer<Packet>,
    JoinHandle<Box<dyn ProcessorBackend>>,
) {
    let (send, mut requests) = rtrb::RingBuffer::<Packet>::new(1);
    let (mut replies, receive) = rtrb::RingBuffer::<Packet>::new(1);
    signals.exit.store(false, Ordering::Release);
    signals.retiring.store(false, Ordering::Release);
    signals.ready.store(false, Ordering::Release);
    let control = signals.clone();
    let worker = std::thread::spawn(move || {
        let wait = WorkerWait::new();
        let mut epoch = control.reset.load(Ordering::Acquire);
        control.ready.store(true, Ordering::Release);
        loop {
            let Ok(mut packet) = requests.pop() else {
                if control.exit.load(Ordering::Acquire) {
                    break;
                }
                wait.wait();
                continue;
            };
            let requested = control.reset.load(Ordering::Acquire);
            if requested != epoch {
                native.reset();
                epoch = requested;
            }
            if control.stop.swap(false, Ordering::AcqRel) {
                native.stop();
            }
            native.set_sidechain_input(packet.sidechain_input);
            packet.silence();
            if packet.epoch == epoch {
                let [in_left, in_right] = &mut packet.input;
                let [out_left, out_right] = &mut packet.output;
                let notifications = &mut packet.notifications;
                let mut drops = 0_u32;
                let _flush = crate::containment::NoDenormals::enter();
                let result = native.process_sidechain(
                    AudioIo::Separate {
                        in_left,
                        in_right,
                        out_left,
                        out_right,
                    },
                    Some(&packet.key),
                    &packet.events,
                    &packet.transport,
                    packet.steady,
                    &mut |event| {
                        if notifications.len() < notifications.capacity() {
                            notifications.push(event);
                        } else {
                            drops = drops.saturating_add(1);
                        }
                    },
                );
                for (bus, output) in packet.auxiliary.buses.iter_mut().enumerate() { native.copy_output(bus as u32, output); }
                match result {
                    Ok(mut result) => {
                        result.dropped_events = result.dropped_events.saturating_add(drops);
                        if result.dropped_events == 0 && native.controls_processed() {
                            control
                                .completed_controls
                                .store(packet.controls, Ordering::Release);
                        }
                        packet.result = result;
                        packet.processed = true;
                    }
                    Err(_) => {
                        packet.failed = true;
                        packet.result.dropped_events =
                            native.failure_dropped_events().saturating_add(drops);
                    }
                }
            }
            // Exactly one outstanding job; the reply slot was emptied before
            // it was submitted. Retain ownership even if the invariant fails.
            while let Err(rtrb::PushError::Full(returned)) = replies.push(packet) {
                packet = returned;
                if control.exit.load(Ordering::Acquire) {
                    break;
                }
                std::thread::yield_now();
            }
        }
        // Misuse retirement has no native owner waiting to receive FFI. Keep
        // it mapped rather than destroy it on this processing thread.
        if !control.retiring.load(Ordering::Acquire) {
            std::mem::forget(native);
            panic!("threaded processor dropped without owner retirement");
        }
        native
    });
    // Activation/owner only. First callback cannot race worker startup.
    while !signals.ready.load(Ordering::Acquire) {
        std::thread::yield_now();
    }
    (send, receive, worker)
}
impl Threaded {
    fn take_reply(&mut self) -> Result<Packet, ProcessFailed> {
        loop {
            if let Ok(packet) = self.receive.pop() {
                return Ok(packet);
            }
            if self.realtime || self.worker.as_ref().is_none_or(JoinHandle::is_finished) {
                self.failed = true;
                return Err(ProcessFailed);
            }
            // Offline rendering is explicitly opted in through set_realtime;
            // this wait is unreachable in the device callback's default mode.
            std::thread::yield_now();
        }
    }
    fn join(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.signals.retiring.store(true, Ordering::Release);
            self.signals.exit.store(true, Ordering::Release);
            self.native = Some(worker.join().expect("native processing worker panicked"));
            if let Ok(packet) = self.receive.pop() {
                self.spare = Some(packet);
            }
            self.pending = false;
        }
    }
}
impl ProcessorBackend for Threaded {
    fn copy_output(&self, bus: u32, output: &mut [[f32; 2]]) -> bool {
        if self.failed { output.fill([0.0; 2]); false } else { self.rendered.copy(bus, output) }
    }
    fn controls_processed(&self) -> bool {
        !self.failed && self.signals.completed_controls.load(Ordering::Acquire) == self.controls
    }
    fn additional_latency(&self) -> u32 {
        self.latency
    }
    fn supports_midi_expression(&self) -> bool {
        self.midi
    }
    fn midi_expression_mask(&self, channel: u8) -> u32 {
        self.midi_masks[usize::from(channel.min(15))]
    }
    fn supports_note_instances(&self) -> bool {
        self.instances
    }
    fn note_expression_mask(&self, channel: u8) -> u32 {
        self.masks[usize::from(channel.min(15))]
    }
    fn set_realtime(&mut self, realtime: bool) {
        self.realtime = realtime;
    }
    fn set_sidechain_input(&mut self, input: Option<u32>) {
        self.signals
            .input
            .store(input.unwrap_or(u32::MAX), Ordering::Release);
    }
    fn return_to_owner(&mut self) {
        self.join();
        self.native.as_mut().unwrap().return_to_owner();
    }
    fn resume_after_refusal(&mut self) {
        if let Some(mut native) = self.native.take() {
            native.resume_after_refusal();
            let (send, receive, worker) = start(native, &self.signals);
            self.send = send;
            self.receive = receive;
            self.worker = Some(worker);
        }
    }
    fn process(
        &mut self,
        audio: AudioIo<'_>,
        events: &[HostEvent],
        transport: &Transport,
        steady: u64,
        out: &mut dyn FnMut(PluginEvent),
    ) -> Result<BlockResult, ProcessFailed> {
        self.process_sidechain(audio, None, events, transport, steady, out)
    }
    fn process_sidechain(
        &mut self,
        audio: AudioIo<'_>,
        key: Option<&[[f32; 2]]>,
        events: &[HostEvent],
        transport: &Transport,
        steady: u64,
        out: &mut dyn FnMut(PluginEvent),
    ) -> Result<BlockResult, ProcessFailed> {
        if self.native.is_some() {
            return Err(ProcessFailed);
        }
        if self.failed {
            return Err(ProcessFailed);
        }
        let AudioIo::Separate {
            in_left,
            in_right,
            out_left,
            out_right,
        } = audio
        else {
            return Err(ProcessFailed);
        };
        let block = self.local.input[0].len();
        let mut event_at = 0;
        let mut drops = 0_u32;
        for frame in 0..out_left.len() {
            let cursor = self.cursor;
            if cursor == 0 {
                self.local.transport = *transport;
                self.local.transport.advance(frame, self.rate);
                self.local.steady = steady.saturating_add(frame as u64);
                self.local.epoch = self.epoch;
                let input = self.signals.input.load(Ordering::Acquire);
                self.local.sidechain_input = (input != u32::MAX).then_some(input);
                for notification in self.local.notifications.drain(..) {
                    out(notification);
                }
            }
            out_left[frame] = self.local.output[0][cursor];
            out_right[frame] = self.local.output[1][cursor];
            for (rendered, bus) in self.rendered.buses.iter_mut().zip(&self.local.auxiliary.buses) { rendered[frame] = bus[cursor]; }
            self.local.input[0][cursor] = in_left[frame];
            self.local.input[1][cursor] = in_right[frame];
            self.local.key[cursor] = key
                .and_then(|key| key.get(frame))
                .copied()
                .unwrap_or([0.0; 2]);
            while event_at < events.len() && events[event_at].time() as usize == frame {
                let mut event = events[event_at];
                event.set_time(cursor as u32);
                if self.local.events.len() < self.local.events.capacity() {
                    if matches!(event, HostEvent::Param { .. }) {
                        let Some(serial) = self.controls.checked_add(1) else {
                            self.failed = true;
                            return Err(ProcessFailed);
                        };
                        self.controls = serial;
                    }
                    self.local.events.push(event);
                } else {
                    // Overflow cannot later acknowledge a control that was
                    // admitted by the outer queue but never reached native DSP.
                    self.failure_drops = self.failure_drops.saturating_add(1);
                    self.failed = true;
                    return Err(ProcessFailed);
                }
                event_at += 1;
            }
            self.cursor += 1;
            if self.cursor == block {
                let mut next = if self.pending {
                    self.take_reply()?
                } else {
                    self.spare.take().expect("prepared spare")
                };
                if next.epoch != self.epoch {
                    next.silence();
                }
                drops = drops.saturating_add(next.result.dropped_events);
                if next.failed {
                    self.failure_drops = next.result.dropped_events;
                    self.spare = Some(next);
                    self.failed = true;
                    return Err(ProcessFailed);
                }
                // An epoch-skipped request may contain controls that never
                // reached native state. Keep their latest values in the next
                // job, before newer controls already collected in local.
                if !next.processed {
                    for index in (0..next.events.len()).rev() {
                        let event = next.events[index];
                        if !matches!(event, HostEvent::Param { .. }) {
                            continue;
                        }
                        if self.local.events.len() < self.local.events.capacity() {
                            let mut event = event;
                            event.set_time(0);
                            self.local.events.insert(0, event);
                        } else {
                            self.failure_drops = self.failure_drops.saturating_add(1);
                            self.spare = Some(next);
                            self.failed = true;
                            return Err(ProcessFailed);
                        }
                    }
                }
                next.events.clear();
                self.local.processed = false;
                self.local.controls = self.controls;
                std::mem::swap(&mut next, &mut self.local);
                if let Err(rtrb::PushError::Full(returned)) = self.send.push(next) {
                    self.spare = Some(returned);
                    self.failed = true;
                    return Err(ProcessFailed);
                }
                self.pending = true;
                self.cursor = 0;
            }
        }
        Ok(BlockResult {
            status: ProcessStatus::Continue,
            tail: self.local.result.tail,
            dropped_events: drops,
        })
    }
    fn retire_params(&mut self, params: &mut Vec<HostEvent>) {
        self.join();
        // join drained the outstanding request. Its events already reached
        // native state and must never be applied after newer local controls.
        self.native.as_mut().unwrap().retire_params(params);
        if let Some(packet) = &self.spare
            && !packet.processed
        {
            params.extend(
                packet
                    .events
                    .iter()
                    .copied()
                    .filter(|event| matches!(event, HostEvent::Param { .. })),
            );
        }
        params.extend(
            self.local
                .events
                .iter()
                .copied()
                .filter(|event| matches!(event, HostEvent::Param { .. })),
        );
    }
    fn failure_dropped_events(&self) -> u32 {
        self.failure_drops
    }
    fn reset(&mut self) {
        if let Some(native) = &mut self.native {
            native.reset();
            return;
        }
        let Some(epoch) = self.epoch.checked_add(1) else {
            self.failed = true;
            return;
        };
        self.epoch = epoch;
        self.signals.reset.store(self.epoch, Ordering::Release);
        self.local.silence();
        self.local.events.clear();
        self.cursor = 0;
    }
    fn stop(&mut self) {
        if let Some(native) = &mut self.native {
            native.stop();
        } else {
            self.signals.stop.store(true, Ordering::Release);
        }
    }
    fn into_any(mut self: Box<Self>) -> Box<dyn Any> {
        self.join();
        self.native.take().unwrap().into_any()
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self.join();
        self.native.as_mut().unwrap().as_any_mut()
    }
}
impl Drop for Threaded {
    fn drop(&mut self) {
        // Normal retirement calls join on control. API misuse must never join
        // or destroy FFI from an audio-thread last drop.
        if self.worker.is_some() {
            self.signals.exit.store(true, Ordering::Release);
            if let Some(worker) = self.worker.take() {
                std::mem::forget(worker);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    struct Probe {
        owner: std::thread::ThreadId,
        gate: Arc<AtomicBool>,
        entered: Arc<AtomicBool>,
        resets: Arc<AtomicUsize>,
        gain: f32,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            assert_eq!(
                self.owner,
                std::thread::current().id(),
                "native retirement must return to its creator"
            );
        }
    }
    impl ProcessorBackend for Probe {
        fn supports_note_instances(&self) -> bool {
            true
        }
        fn note_expression_mask(&self, _: u8) -> u32 {
            0x36
        }
        fn process(
            &mut self,
            audio: AudioIo<'_>,
            events: &[HostEvent],
            transport: &Transport,
            steady: u64,
            out: &mut dyn FnMut(PluginEvent),
        ) -> Result<BlockResult, ProcessFailed> {
            self.process_sidechain(audio, None, events, transport, steady, out)
        }
        fn process_sidechain(
            &mut self,
            audio: AudioIo<'_>,
            key: Option<&[[f32; 2]]>,
            events: &[HostEvent],
            _: &Transport,
            _: u64,
            _: &mut dyn FnMut(PluginEvent),
        ) -> Result<BlockResult, ProcessFailed> {
            self.entered.store(true, Ordering::Release);
            while self.gate.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            let AudioIo::Separate {
                in_left,
                in_right,
                out_left,
                out_right,
            } = audio
            else {
                return Err(ProcessFailed);
            };
            for frame in 0..out_left.len() {
                for event in events.iter().filter(|event| event.time() as usize == frame) {
                    if let HostEvent::Param { value, .. } = event {
                        self.gain = *value as f32;
                    }
                }
                let detector = key.map_or(0.0, |key| (key[frame][0] + key[frame][1]) * 0.125);
                out_left[frame] = in_left[frame] * self.gain + detector;
                out_right[frame] = in_right[frame] * self.gain - detector;
            }
            Ok(BlockResult {
                status: ProcessStatus::Continue,
                tail: Some(0),
                dropped_events: 0,
            })
        }
        fn reset(&mut self) {
            self.resets.fetch_add(1, Ordering::Relaxed);
        }
        fn stop(&mut self) {}
        fn into_any(self: Box<Self>) -> Box<dyn Any> {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }
    fn probe(
        gate: Arc<AtomicBool>,
        entered: Arc<AtomicBool>,
        resets: Arc<AtomicUsize>,
    ) -> Box<dyn ProcessorBackend> {
        Box::new(Probe {
            owner: std::thread::current().id(),
            gate,
            entered,
            resets,
            gain: 1.0,
        })
    }
    fn run(
        unit: &mut dyn ProcessorBackend,
        input: &mut [f32],
        events: &[HostEvent],
        with_key: bool,
    ) -> Result<Vec<[f32; 2]>, ProcessFailed> {
        let mut right: Vec<f32> = input.iter().map(|value| -*value).collect();
        let mut left_out = vec![0.0; input.len()];
        let mut right_out = left_out.clone();
        let key = vec![[0.25, 0.75]; input.len()];
        unit.process_sidechain(
            AudioIo::Separate {
                in_left: input,
                in_right: &mut right,
                out_left: &mut left_out,
                out_right: &mut right_out,
            },
            with_key.then_some(&key[..]),
            events,
            &Transport::default(),
            0,
            &mut |_| {},
        )?;
        Ok(left_out
            .into_iter()
            .zip(right_out)
            .map(|(left, right)| [left, right])
            .collect())
    }
    #[test]
    fn threaded_partitioned_pcm_sidechain_and_parameter_offsets_match_two_quantum_delay() {
        let mut unit = wrap(
            probe(
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicUsize::new(0)),
            ),
            8,
            48_000.0,
        );
        unit.set_realtime(false);
        assert_eq!(unit.additional_latency(), 16);
        assert!(unit.supports_note_instances());
        assert_eq!(unit.note_expression_mask(2), 0x36);
        let mut actual = Vec::new();
        let mut position = 0;
        for frames in [3, 2, 7, 1, 8, 11, 16] {
            let mut input: Vec<_> = (position..position + frames)
                .map(|frame| frame as f32 / 64.0)
                .collect();
            let events = if (position..position + frames).contains(&5) {
                vec![HostEvent::Param {
                    time: (5 - position) as u32,
                    id: 7,
                    value: 0.5,
                }]
            } else {
                vec![]
            };
            actual.extend(
                run(&mut *unit, &mut input, &events, true)
                    .ok()
                    .expect("worker PCM"),
            );
            position += frames;
        }
        for (frame, actual) in actual.iter().enumerate() {
            let expected = if frame < 16 {
                0.0
            } else {
                let source = frame - 16;
                source as f32 / 64.0 * if source < 5 { 1.0 } else { 0.5 } + 0.125
            };
            assert_eq!(*actual, [expected, -expected], "frame {frame}");
        }
        unit.return_to_owner();
        drop(
            unit.into_any()
                .downcast::<Probe>()
                .expect("original processor"),
        );
    }
    #[test]
    fn threaded_deadline_never_waits_and_retirement_keeps_newer_unprocessed_controls() {
        let gate = Arc::new(AtomicBool::new(true));
        let entered = Arc::new(AtomicBool::new(false));
        let mut unit = wrap(
            probe(gate.clone(), entered.clone(), Arc::new(AtomicUsize::new(0))),
            8,
            48_000.0,
        );
        assert_eq!(
            run(
                &mut *unit,
                &mut [1.0; 8],
                &[HostEvent::Param {
                    time: 0,
                    id: 7,
                    value: 0.25
                }],
                false
            )
            .ok()
            .unwrap(),
            vec![[0.0; 2]; 8]
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        while !entered.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(entered.load(Ordering::Acquire));
        assert!(!unit.controls_processed());
        assert!(
            run(
                &mut *unit,
                &mut [1.0; 8],
                &[HostEvent::Param {
                    time: 0,
                    id: 7,
                    value: 0.75
                }],
                false
            )
            .is_err()
        );
        assert!(
            gate.load(Ordering::Acquire),
            "callback returned while native processing remained gated"
        );
        gate.store(false, Ordering::Release);
        unit.return_to_owner();
        let mut pending = Vec::new();
        unit.retire_params(&mut pending);
        assert_eq!(
            pending,
            vec![HostEvent::Param {
                time: 0,
                id: 7,
                value: 0.75
            }]
        );
        let native = unit.into_any().downcast::<Probe>().ok().unwrap();
        assert_eq!(
            native.gain, 0.25,
            "submitted control applied once on worker"
        );
        drop(native);
    }
    #[test]
    fn threaded_reset_rejects_old_output_without_callback_native_reset() {
        let resets = Arc::new(AtomicUsize::new(0));
        let mut unit = wrap(
            probe(
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                resets.clone(),
            ),
            8,
            48_000.0,
        );
        unit.set_realtime(false);
        run(&mut *unit, &mut [1.0; 8], &[], false).ok().unwrap();
        unit.reset();
        let output = run(&mut *unit, &mut [0.0; 32], &[], false).ok().unwrap();
        assert_eq!(output, vec![[0.0; 2]; 32]);
        unit.return_to_owner();
        assert_eq!(resets.load(Ordering::Relaxed), 1);
        drop(unit.into_any().downcast::<Probe>().ok().unwrap());
    }
    #[test]
    fn threaded_event_capacity_exhaustion_is_reported_without_control_acknowledgement() {
        let mut unit = wrap(
            probe(
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicUsize::new(0)),
            ),
            8,
            48_000.0,
        );
        let mut events = vec![
            HostEvent::Param {
                time: 0,
                id: 7,
                value: 0.25
            };
            BLOCK_EVENT_CAPACITY
        ];
        events.push(HostEvent::Param {
            time: 0,
            id: 7,
            value: 0.75,
        });
        assert!(run(&mut *unit, &mut [1.0], &events, false).is_err());
        assert_eq!(unit.failure_dropped_events(), 1);
        assert!(!unit.controls_processed());
        unit.return_to_owner();
        let mut pending = Vec::new();
        unit.retire_params(&mut pending);
        assert_eq!(pending.len(), BLOCK_EVENT_CAPACITY);
        let native = unit.into_any().downcast::<Probe>().ok().unwrap();
        assert_eq!(
            native.gain, 1.0,
            "an incomplete packet cannot claim native processing"
        );
        drop(native);
    }
}
