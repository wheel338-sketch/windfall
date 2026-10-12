//! Preallocated VST3 audio-thread data. Controller calls never happen here.
use super::{
    buffers::{Changes, Events, PENDING_PARAMETER_CAPACITY},
    handlers::AudioCall,
    instance::{Objects, Value},
};
use crate::{
    descriptor::{AudioPort, PluginLayout},
    error::PluginError,
    events::{HostEvent, MAX_NOTE_INSTANCES, NoteExpressionKind, PluginEvent, Transport},
    processor::{AudioIo, BlockResult, ProcessFailed, ProcessStatus, ProcessorBackend},
};
use std::{any::Any, sync::Arc};
use vst3::{
    ComPtr, ComWrapper,
    Steinberg::{Vst::*, *},
};

#[derive(Clone, Copy)]
struct NativeHeld {
    id: u32,
    key: u8,
    channel: u8,
}
impl NativeHeld {
    const EMPTY: Self = Self {
        id: 0,
        key: 0,
        channel: 0,
    };
}

struct Ports {
    buffers: Vec<Vec<Vec<f32>>>,
    _pointers: Vec<Vec<*mut f32>>,
    buses: Vec<AudioBusBuffers>,
    main: Option<usize>,
    key: Option<usize>,
    auxiliary: Vec<usize>,
}

fn process_context(
    transport: &Transport,
    sample_rate: f64,
    steady_time: u64,
) -> Result<ProcessContext, ProcessFailed> {
    let (bar_start, _) = transport.bar_position().ok_or(ProcessFailed)?;
    // SAFETY: ProcessContext is POD, only flagged fields are consumed.
    let mut context: ProcessContext = unsafe { std::mem::zeroed() };
    context.state = 512 | 1024 | 2048 | 8192 | 131072 | if transport.playing { 2 } else { 0 };
    context.sampleRate = sample_rate;
    context.projectTimeSamples = (transport.position_seconds * sample_rate) as i64;
    context.continousTimeSamples = steady_time.min(i64::MAX as u64) as i64;
    context.projectTimeMusic = transport.position_beats;
    context.tempo = transport.tempo_bpm;
    context.timeSigNumerator = transport.numerator.into();
    context.timeSigDenominator = transport.denominator.into();
    context.barPositionMusic = bar_start;
    Ok(context)
}

impl Ports {
    fn new(ports: &[AudioPort], max_block: usize) -> Self {
        let mut buffers: Vec<Vec<Vec<f32>>> = ports
            .iter()
            .map(|p| (0..p.channels).map(|_| vec![0.0; max_block]).collect())
            .collect();
        let mut pointers: Vec<Vec<*mut f32>> = buffers
            .iter_mut()
            .map(|p| p.iter_mut().map(|b| b.as_mut_ptr()).collect())
            .collect();
        let buses = pointers
            .iter_mut()
            .map(|p| AudioBusBuffers {
                numChannels: p.len() as i32,
                silenceFlags: 0,
                __field0: AudioBusBuffers__type0 {
                    channelBuffers32: p.as_mut_ptr(),
                },
            })
            .collect();
        Self {
            buffers,
            _pointers: pointers,
            buses,
            main: ports.iter().position(|p| p.main && p.channels > 0),
            key: ports.iter().position(|p| !p.main && p.channels > 0),
            auxiliary: ports
                .iter()
                .enumerate()
                .filter_map(|(index, p)| (!p.main && p.channels > 0).then_some(index))
                .collect(),
        }
    }
    fn clear(&mut self, frames: usize) {
        for (index, bus) in self.buffers.iter_mut().enumerate() {
            for (channel_index, channel) in bus.iter_mut().enumerate() {
                channel[..frames].fill(0.0);
                self._pointers[index][channel_index] = channel.as_mut_ptr();
            }
            self.buses[index].numChannels = bus.len() as i32;
            self.buses[index].__field0.channelBuffers32 = self._pointers[index].as_mut_ptr();
            self.buses[index].silenceFlags = 0;
        }
    }
    fn input(&mut self, left: &[f32], right: &[f32]) {
        if let Some(main) = self.main {
            let bus = &mut self.buffers[main];
            if bus.len() == 1 {
                for ((out, l), r) in bus[0].iter_mut().zip(left).zip(right) {
                    *out = (*l + *r) * 0.5;
                }
            } else {
                bus[0][..left.len()].copy_from_slice(left);
                bus[1][..right.len()].copy_from_slice(right);
            }
        }
    }
    fn key_input(&mut self, key: Option<&[[f32; 2]]>, frames: usize) {
        if let Some(bus) = self.key {
            let channels = &mut self.buffers[bus];
            let mono = channels.len() == 1;
            for (side, channel) in channels.iter_mut().take(2).enumerate() {
                for (index, output) in channel[..frames].iter_mut().enumerate() {
                    let pair = key
                        .and_then(|key| key.get(index))
                        .copied()
                        .unwrap_or([0.0; 2]);
                    let value = if mono {
                        (pair[0] + pair[1]) * 0.5
                    } else {
                        pair[side]
                    };
                    *output = if value.is_finite() {
                        value.clamp(-1.0e6, 1.0e6)
                    } else {
                        0.0
                    };
                }
            }
            self.buses[bus].silenceFlags = if key.is_none() { u64::MAX } else { 0 };
        }
    }
    fn output(&self, left: &mut [f32], right: &mut [f32]) {
        if let Some(main) = self.main {
            let bus = &self.buffers[main];
            left.copy_from_slice(&bus[0][..left.len()]);
            right.copy_from_slice(&bus[usize::from(bus.len() > 1)][..right.len()]);
        } else {
            left.fill(0.0);
            right.fill(0.0);
        }
    }
}
pub(super) struct VstProcessor {
    objects: Arc<Objects>,
    values: Arc<[Value]>,
    // Final input intent from a failed native call/translation, retained until
    // owner return. Readback remains the last successfully processed value.
    unprocessed: Box<[Option<f64>]>,
    inputs: Ports,
    outputs: Ports,
    parameters: ComWrapper<Changes>,
    output_parameters: ComWrapper<Changes>,
    parameter_ptr: ComPtr<IParameterChanges>,
    output_parameter_ptr: ComPtr<IParameterChanges>,
    events: ComWrapper<Events>,
    output_events: ComWrapper<Events>,
    event_ptr: ComPtr<IEventList>,
    output_event_ptr: ComPtr<IEventList>,
    edits: rtrb::Consumer<(u32, f64)>,
    sample_rate: f64,
    started: bool,
    reset_notes: bool,
    held: [[bool; 128]; 16],
    event_input: bool,
    instances: Box<[NativeHeld]>,
    expression_masks: [u32; 16],
    midi_mapping: Box<[[u32; 131]; 16]>,
    midi_expression: [[Option<u32>; 7]; 16],
    midi_fallback: bool,
    native_drops: u32,
}
// SAFETY: all mutable COM buffers are exclusively owned by this processor,
// moved to one audio thread, and used synchronously in process only. Rc in
// Changes shares arenas only between queues of this same exclusive owner.
unsafe impl Send for VstProcessor {}
impl VstProcessor {
    /// Owner-only lifecycle; a refusal retains both native and Rust ownership.
    pub(super) fn quiesce(&mut self) -> Result<(), PluginError> {
        if self.started {
            if unsafe { self.objects.processor.setProcessing(0) } != kResultOk {
                return Err(PluginError::Deactivate(
                    "setProcessing(false) refused".into(),
                ));
            }
            self.started = false;
        }
        Ok(())
    }
    /// The native owner calls this only after exclusive audio ownership returns.
    /// Preserve editor points still queued for process as deferred state overrides.
    pub(super) fn retain_pending_edits(&mut self) {
        for (value, pending) in self.values.iter().zip(&mut self.unprocessed) {
            if let Some(normalized) = pending.take() {
                value.set(normalized);
                value
                    .pending
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        for _ in 0..crate::instance::QUEUE_CAPACITY {
            let Ok((id, value)) = self.edits.pop() else {
                break;
            };
            if let Some(v) = self.value(id)
                && v.writable
            {
                v.set(value);
                v.pending.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
    pub fn new(
        objects: Arc<Objects>,
        layout: &PluginLayout,
        values: Arc<[Value]>,
        sample_rate: f64,
        max_block: u32,
        edits: rtrb::Consumer<(u32, f64)>,
    ) -> Result<Self, PluginError> {
        // SAFETY: inactive component on its main thread, bounded bus indices.
        unsafe {
            if objects.processor.canProcessSampleSize(0) != kResultOk {
                return Err(PluginError::Activate(
                    "32-bit audio is not supported".into(),
                ));
            }
            let mut ins = Vec::with_capacity(layout.audio_inputs.len());
            let mut outs = Vec::with_capacity(layout.audio_outputs.len());
            for (direction, count, arrangements) in [
                (0, layout.audio_inputs.len(), &mut ins),
                (1, layout.audio_outputs.len(), &mut outs),
            ] {
                for index in 0..count {
                    let mut arrangement = 0;
                    if objects.processor.getBusArrangement(
                        direction,
                        index as i32,
                        &mut arrangement,
                    ) != kResultOk
                    {
                        return Err(PluginError::Activate("bus arrangement refused".into()));
                    }
                    arrangements.push(arrangement);
                }
            }
            if objects.processor.setBusArrangements(
                ins.as_mut_ptr(),
                ins.len() as i32,
                outs.as_mut_ptr(),
                outs.len() as i32,
            ) != kResultOk
            {
                return Err(PluginError::Activate("bus arrangements refused".into()));
            }
            for (direction, ports) in [(0, &layout.audio_inputs), (1, &layout.audio_outputs)] {
                if objects.component.getBusCount(0, direction) != ports.len() as i32 {
                    return Err(PluginError::Layout(
                        "bus count changed during arrangement".into(),
                    ));
                }
                for (index, port) in ports.iter().enumerate() {
                    let mut bus: BusInfo = std::mem::zeroed();
                    if objects
                        .component
                        .getBusInfo(0, direction, index as i32, &mut bus)
                        != kResultOk
                        || bus.channelCount != port.channels as i32
                    {
                        return Err(PluginError::Layout(
                            "channel count changed during arrangement".into(),
                        ));
                    }
                }
            }
            for (media, direction, count) in [
                (0, 0, ins.len()),
                (0, 1, outs.len()),
                (1, 0, layout.note_inputs as usize),
                (1, 1, layout.note_outputs as usize),
            ] {
                for index in 0..count {
                    if objects
                        .component
                        .activateBus(media, direction, index as i32, 1)
                        != kResultOk
                    {
                        return Err(PluginError::Activate("bus activation refused".into()));
                    }
                }
            }
            let mut setup = ProcessSetup {
                processMode: 0,
                symbolicSampleSize: 0,
                maxSamplesPerBlock: max_block as i32,
                sampleRate: sample_rate,
            };
            if objects.processor.setupProcessing(&mut setup) != kResultOk
                || objects.component.setActive(1) != kResultOk
            {
                return Err(PluginError::Activate("processing setup refused".into()));
            }
        }
        let parameters = Changes::input(values.len().saturating_add(16 * 7));
        let parameter_ptr = parameters
            .to_com_ptr::<IParameterChanges>()
            .expect("changes");
        let output_parameters = Changes::new();
        let output_parameter_ptr = output_parameters
            .to_com_ptr::<IParameterChanges>()
            .expect("changes");
        let events = Events::new();
        let event_ptr = events.to_com_ptr::<IEventList>().expect("events");
        let output_events = Events::new();
        let output_event_ptr = output_events.to_com_ptr::<IEventList>().expect("events");
        let mut expression_masks = [0; 16];
        if let Some(controller) = objects
            .controller
            .as_ref()
            .and_then(|controller| controller.cast::<INoteExpressionController>())
        {
            for (channel, expression_mask) in expression_masks.iter_mut().enumerate() {
                let count =
                    unsafe { controller.getNoteExpressionCount(0, channel as i16) }.clamp(0, 256);
                for index in 0..count {
                    let mut info: NoteExpressionTypeInfo = unsafe { std::mem::zeroed() };
                    if unsafe {
                        controller.getNoteExpressionInfo(0, channel as i16, index, &mut info)
                    } == kResultOk
                        && info.typeId <= 5
                    {
                        *expression_mask |= 1 << info.typeId;
                    }
                }
            }
        }
        // Query the controller while preparing, never from process(). VST3
        // represents CC, pressure, bend and program input as parameter points.
        let mut midi_mapping = Box::new([[u32::MAX; 131]; 16]);
        if let Some(mapping) = objects
            .controller
            .as_ref()
            .and_then(|controller| controller.cast::<IMidiMapping>())
        {
            for (channel, row) in midi_mapping.iter_mut().enumerate() {
                for (controller, id) in row.iter_mut().enumerate() {
                    let mut assigned = u32::MAX;
                    if unsafe {
                        mapping.getMidiControllerAssignment(
                            0,
                            channel as i16,
                            controller as i16,
                            &mut assigned,
                        )
                    } == kResultOk
                    {
                        *id = assigned;
                    }
                }
            }
        }
        let mut midi_expression = [[None; 7]; 16];
        for (channel, assignments) in midi_expression.iter_mut().enumerate() {
            for (index, number) in [7_usize, 10, 129, 1, 11, 74, 128].into_iter().enumerate() {
                let id = midi_mapping[channel][number];
                if id != u32::MAX && values.iter().any(|value| value.id == id && value.writable) {
                    assignments[index] = Some(id);
                }
            }
        }
        let midi_fallback = expression_masks.iter().all(|mask| mask & NoteExpressionKind::Tuning.mask() == 0) &&
            midi_expression.iter().any(|assignments| assignments[2].is_some());
        Ok(Self {
            objects,
            midi_mapping,
            unprocessed: vec![None; values.len()].into_boxed_slice(),
            values,
            inputs: Ports::new(&layout.audio_inputs, max_block as usize),
            outputs: Ports::new(&layout.audio_outputs, max_block as usize),
            parameters,
            output_parameters,
            parameter_ptr,
            output_parameter_ptr,
            events,
            output_events,
            event_ptr,
            output_event_ptr,
            edits,
            sample_rate,
            started: false,
            reset_notes: false,
            held: [[false; 128]; 16],
            event_input: layout.note_inputs > 0,
            instances: vec![NativeHeld::EMPTY; MAX_NOTE_INSTANCES].into_boxed_slice(),
            expression_masks,
            midi_expression,
            midi_fallback,
            native_drops: 0,
        })
    }
    fn value(&self, id: u32) -> Option<&Value> {
        self.values
            .binary_search_by_key(&id, |v| v.id)
            .ok()
            .map(|i| &self.values[i])
    }
    fn input_point(&mut self, id: u32, time: u32, normalized: f64) {
        if !self.parameters.push(id, time, normalized)
            && let Ok(index) = self.values.binary_search_by_key(&id, |v| v.id)
        {
            self.unprocessed[index] = Some(normalized);
        }
    }
    fn retain_failed_block(&mut self) {
        let values = &self.values;
        let unprocessed = &mut self.unprocessed;
        self.parameters.visit_final(&mut |id, normalized| {
            if let Ok(index) = values.binary_search_by_key(&id, |v| v.id)
                && unprocessed[index].is_none()
            {
                // Rejected final points already take precedence over each
                // queue's final admitted value.
                unprocessed[index] = Some(normalized);
            }
        });
    }
    fn dropped_events(&self) -> u32 {
        self.parameters
            .dropped()
            .saturating_add(self.output_parameters.dropped())
            .saturating_add(self.events.dropped())
            .saturating_add(self.output_events.dropped())
            .saturating_add(self.native_drops)
    }
    fn note(&self, time: u32, key: u8, channel: u8, velocity: f32, on: bool) {
        self.note_with_id(
            time,
            key,
            channel,
            velocity,
            on,
            i32::from(channel) * 128 + i32::from(key),
        );
    }
    fn note_with_id(&self, time: u32, key: u8, channel: u8, velocity: f32, on: bool, id: i32) {
        if !self.event_input {
            return;
        }
        // SAFETY: Event is an SDK POD. Only the active pointer-free union
        // variant below is exposed to the plugin for this process call.
        let mut event: Event = unsafe { std::mem::zeroed() };
        event.sampleOffset = time as i32;
        event.flags = 1;
        if on {
            event.r#type = 0;
            event.__field0.noteOn = NoteOnEvent {
                channel: channel.into(),
                pitch: key.into(),
                tuning: 0.0,
                velocity,
                length: 0,
                noteId: id,
            };
        } else {
            event.r#type = 1;
            event.__field0.noteOff = NoteOffEvent {
                channel: channel.into(),
                pitch: key.into(),
                velocity,
                noteId: id,
                tuning: 0.0,
            };
        }
        self.events.push(event);
    }
    fn instance(&mut self, time: u32, id: u32, key: u8, channel: u8, velocity: f32, on: bool) {
        if on {
            let slot = self
                .instances
                .iter()
                .position(|note| note.id == id)
                .or_else(|| self.instances.iter().position(|note| note.id == 0));
            let Some(slot) = slot else {
                self.native_drops = self.native_drops.saturating_add(1);
                return;
            };
            self.instances[slot] = NativeHeld { id, key, channel };
        } else if let Some(note) = self.instances.iter_mut().find(|note| note.id == id) {
            *note = NativeHeld::EMPTY;
        }
        self.note_with_id(time, key, channel, velocity, on, id as i32);
    }
    fn expression(&self, time: u32, id: u32, channel: u8, kind: NoteExpressionKind, value: f64) {
        if self.midi_fallback {
            if let Some(parameter) = self.midi_expression[channel as usize][kind as usize] {
                let normalized = if kind == NoteExpressionKind::Tuning { (value + 1.0) / 2.0 } else { value };
                self.parameters.push(parameter, time, normalized.clamp(0.0, 1.0));
            }
            return;
        }
        if !self.event_input || self.expression_masks[channel as usize] & kind.mask() == 0 {
            return;
        }
        let value = match kind {
            NoteExpressionKind::Tuning => value / 240.0 + 0.5,
            NoteExpressionKind::Volume => value / 4.0,
            _ => value,
        };
        let mut event: Event = unsafe { std::mem::zeroed() };
        event.sampleOffset = time as i32;
        event.flags = 1;
        event.r#type = 4;
        event.__field0.noteExpressionValue = NoteExpressionValueEvent {
            typeId: kind as u32,
            noteId: id as i32,
            value,
        };
        self.events.push(event);
    }
    fn release_instances(&mut self, time: u32) {
        for index in 0..self.instances.len() {
            let note = self.instances[index];
            if note.id != 0 {
                self.note_with_id(time, note.key, note.channel, 0.0, false, note.id as i32);
            }
        }
        self.instances.fill(NativeHeld::EMPTY);
    }
}
impl ProcessorBackend for VstProcessor {
    fn supports_midi_expression(&self) -> bool { self.event_input && self.midi_fallback }
    fn midi_expression_mask(&self, channel: u8) -> u32 {
        if !self.midi_fallback { return 0; }
        self.midi_expression[channel.min(15) as usize].iter().enumerate().fold(0, |mask, (index, id)| mask | if id.is_some() { 1 << index } else { 0 })
    }
    fn supports_note_instances(&self) -> bool {
        self.event_input
    }
    fn note_expression_mask(&self, channel: u8) -> u32 {
        self.expression_masks[channel.min(15) as usize]
    }
    fn set_sidechain_input(&mut self, input: Option<u32>) {
        self.inputs.key = match input {
            Some(index) => self
                .inputs
                .auxiliary
                .iter()
                .copied()
                .find(|port| *port == index as usize),
            None => self.inputs.auxiliary.first().copied(),
        };
    }
    fn process(
        &mut self,
        audio: AudioIo<'_>,
        events: &[HostEvent],
        transport: &Transport,
        steady_time: u64,
        out: &mut dyn FnMut(PluginEvent),
    ) -> Result<BlockResult, ProcessFailed> {
        self.process_sidechain(audio, None, events, transport, steady_time, out)
    }
    fn process_sidechain(
        &mut self,
        audio: AudioIo<'_>,
        key: Option<&[[f32; 2]]>,
        events: &[HostEvent],
        transport: &Transport,
        steady_time: u64,
        out: &mut dyn FnMut(PluginEvent),
    ) -> Result<BlockResult, ProcessFailed> {
        let mut context = process_context(transport, self.sample_rate, steady_time)?;
        let _guard = AudioCall::enter();
        // Clear before starting too, so a start failure cannot repeat a prior
        // block's drop count.
        self.parameters.clear();
        self.output_parameters.clear();
        self.events.clear();
        self.output_events.clear();
        self.native_drops = 0;
        // SAFETY: processor has one exclusive audio owner, lifecycle calls
        // occur here rather than on the concurrent controller/main thread.
        if !self.started {
            if unsafe { self.objects.processor.setProcessing(1) } != kResultOk {
                return Err(ProcessFailed);
            }
            self.started = true;
        }
        let frames = audio.frames();
        self.inputs.clear(frames);
        self.outputs.clear(frames);
        let mut pending_count = 0;
        for value in self.values.iter() {
            if pending_count == PENDING_PARAMETER_CAPACITY {
                break;
            }
            if value
                .pending
                .swap(false, std::sync::atomic::Ordering::Relaxed)
            {
                // The capacity includes every deferred point. Preserve a
                // refused flag rather than consuming an undeliverable value.
                if !self.parameters.push(value.id, 0, value.normalized()) {
                    value
                        .pending
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                }
                pending_count += 1;
            }
        }
        if self.reset_notes {
            self.release_instances(0);
            for channel in 0..16 {
                for key in 0..128 {
                    if self.held[channel][key] {
                        self.note(0, key as u8, channel as u8, 0.0, false);
                    }
                }
            }
            self.held = [[false; 128]; 16];
            self.reset_notes = false;
        }
        // Bounded queue drain, followed by sorted scheduled points.
        for _ in 0..crate::instance::QUEUE_CAPACITY {
            let Ok((id, value)) = self.edits.pop() else {
                break;
            };
            if let Some(v) = self.value(id) {
                if !v.writable {
                    continue;
                }
                self.input_point(id, 0, value);
            }
        }
        for event in events {
            match *event {
                HostEvent::Midi { time, bytes, .. } => {
                    let channel = usize::from(bytes[0] & 15);
                    let (controller, value) = match bytes[0] & 0xf0 {
                        0xb0 => (usize::from(bytes[1]), f64::from(bytes[2]) / 127.0),
                        0xd0 => (128, f64::from(bytes[1]) / 127.0),
                        0xe0 => (
                            129,
                            f64::from(u16::from(bytes[1]) | u16::from(bytes[2]) << 7) / 16383.0,
                        ),
                        0xc0 => (130, f64::from(bytes[1]) / 127.0),
                        _ => {
                            self.native_drops = self.native_drops.saturating_add(1);
                            continue;
                        }
                    };
                    let id = self.midi_mapping[channel][controller];
                    if id == u32::MAX || !self.parameters.push(id, time, value) {
                        self.native_drops = self.native_drops.saturating_add(1);
                    }
                }
                HostEvent::NoteOnInstance {
                    time,
                    id,
                    key,
                    channel,
                    velocity,
                } => self.instance(time, id, key, channel, velocity, true),
                HostEvent::NoteOffInstance {
                    time,
                    id,
                    key,
                    channel,
                    velocity,
                } => self.instance(time, id, key, channel, velocity, false),
                HostEvent::NoteExpression {
                    time,
                    id,
                    channel,
                    kind,
                    value,
                    ..
                } => self.expression(time, id, channel, kind, value),
                HostEvent::Param { time, id, value } => {
                    if let Some(v) = self.value(id) {
                        if !v.writable {
                            continue;
                        }
                        let normalized = (value / v.scale).clamp(0.0, 1.0);
                        self.input_point(id, time, normalized);
                    }
                }
                HostEvent::NoteOn {
                    time,
                    key,
                    channel,
                    velocity,
                } => {
                    self.note(time, key, channel, velocity, true);
                    self.held[channel as usize][key as usize] = true;
                }
                HostEvent::NoteOff {
                    time,
                    key,
                    channel,
                    velocity,
                } => {
                    self.note(time, key, channel, velocity, false);
                    self.held[channel as usize][key as usize] = false;
                }
                HostEvent::AllNotesOff { time } => {
                    self.release_instances(time);
                    for channel in 0..16 {
                        if self.midi_fallback {
                            for (index, value) in [1.0, 0.5, 0.0, 0.5, 0.5, 0.5, 0.5].into_iter().enumerate() {
                                self.expression(time, crate::events::FIRST_NOTE_INSTANCE_ID, channel as u8, NoteExpressionKind::from_raw(index as u32).unwrap(), value);
                            }
                        }
                        for key in 0..128 {
                            if self.held[channel][key] {
                                self.note(time, key as u8, channel as u8, 0.0, false);
                                self.held[channel][key] = false;
                            }
                        }
                    }
                }
            }
        }
        // A valid block fits by construction. An unexpected translation
        // refusal is a failed block, never a successful control acknowledgement.
        if self.parameters.dropped() != 0 || self.events.dropped() != 0 || self.native_drops != 0 {
            self.retain_failed_block();
            return Err(ProcessFailed);
        }
        let (left, right) = match audio {
            AudioIo::Separate {
                in_left,
                in_right,
                out_left,
                out_right,
            } => {
                self.inputs.input(in_left, in_right);
                (out_left, out_right)
            }
            AudioIo::InPlace { left, right } => {
                self.inputs.input(left, right);
                (left, right)
            }
        };
        self.inputs.key_input(key, frames);
        let mut data = ProcessData {
            processMode: 0,
            symbolicSampleSize: 0,
            numSamples: frames as i32,
            numInputs: self.inputs.buses.len() as i32,
            numOutputs: self.outputs.buses.len() as i32,
            inputs: self.inputs.buses.as_mut_ptr(),
            outputs: self.outputs.buses.as_mut_ptr(),
            inputParameterChanges: self.parameter_ptr.as_ptr(),
            outputParameterChanges: self.output_parameter_ptr.as_ptr(),
            inputEvents: self.event_ptr.as_ptr(),
            outputEvents: self.output_event_ptr.as_ptr(),
            processContext: &mut context,
        };
        // SAFETY: all buffers and interfaces live for this synchronous call;
        // plugin must not retain buffer/context pointers beyond process.
        if unsafe { self.objects.processor.process(&mut data) } != kResultOk {
            self.retain_failed_block();
            return Err(ProcessFailed);
        }
        // Admission into an SDK queue is not processing. Publish readback only
        // after the actual native call succeeded; newer output edits win next.
        self.parameters.visit_final(&mut |id, normalized| {
            if let Some(v) = self.value(id) {
                v.set(normalized);
            }
        });
        self.outputs.output(left, right);
        self.output_parameters.visit(&mut |id, _time, normalized| {
            if let Some(v) = self.value(id) {
                v.set(normalized);
                out(PluginEvent::ParamValue {
                    id,
                    value: normalized * v.scale,
                });
            }
        });
        Ok(BlockResult {
            status: ProcessStatus::Continue,
            tail: None,
            dropped_events: self.dropped_events(),
        })
    }
    fn failure_dropped_events(&self) -> u32 {
        self.dropped_events()
    }
    fn reset(&mut self) {
        self.stop();
        self.reset_notes = true;
    }
    fn stop(&mut self) {
        let _guard = AudioCall::enter();
        let _ = self.quiesce();
    }
    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(all(test, windows))]
#[test]
fn r4_unexpected_native_point_refusal_counts_drops_without_false_readback_and_recovers() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let target = manifest.join("../../target/vst3-ownership-fixture");
    let build = std::process::Command::new(env!("CARGO"))
        .args(["build", "--manifest-path"])
        .arg(manifest.join("test-plugins/Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .env("CARGO_BUILD_JOBS", "1")
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let path = target.join("point-refusal.vst3");
    std::fs::copy(target.join("debug/windfall_test_plugins.dll"), &path).unwrap();
    let module = crate::PluginHost::windfall().load(&path).unwrap();
    let mut instance = module.create(&module.descriptors()[0].id).unwrap();
    let mut processor = instance.activate(48_000.0, 64).unwrap();
    // Fault injection on the creating owner, before the measured audio work:
    // valid production input storage has room for all three sources.
    let vst = processor
        .backend_mut()
        .as_any_mut()
        .downcast_mut::<VstProcessor>()
        .unwrap();
    vst.parameters = Changes::input(0);
    vst.parameter_ptr = vst.parameters.to_com_ptr().unwrap();
    assert!(processor.set_param(1, 7, 0.25));
    assert!(processor.set_param(63, 7, 0.75));
    let mut left = [1.0; 64];
    let mut right = left;
    assert_eq!(
        processor.process(&mut left, &mut right),
        ProcessStatus::Failed
    );
    assert_eq!(processor.health().dropped_events, 2);
    assert_eq!(instance.param_value(7), Some(0.5));
    assert_eq!(left, [1.0; 64]);
    instance.deactivate(processor).unwrap();
    assert_eq!(instance.param_value(7), Some(0.75));
    let mut recovered = instance.activate(48_000.0, 64).unwrap();
    assert_eq!(
        recovered.process(&mut left, &mut right),
        ProcessStatus::Continue
    );
    assert_eq!(
        left, [1.5; 64],
        "retained final intent must actually process"
    );
    assert_eq!(recovered.health().dropped_events, 0);
    instance.deactivate(recovered).unwrap();
}

#[cfg(test)]
mod meter_tests {
    use super::*;
    use crate::MeterAnchor;

    #[test]
    fn independently_converted_ticks_report_the_exact_native_downbeat() {
        let context = process_context(
            &Transport {
                playing: true,
                tempo_bpm: 137.0,
                position_beats: 3845.0 / 960.0,
                position_seconds: 9.25,
                numerator: 7,
                denominator: 8,
                meter_anchor: Some(MeterAnchor {
                    bar_origin_beats: 485.0 / 960.0,
                    bar_origin_index: 1,
                }),
            },
            48_000.0,
            123,
        )
        .unwrap_or_else(|_| panic!("valid transport"));
        assert_eq!(context.barPositionMusic, 3845.0 / 960.0);
        assert_eq!(context.projectTimeMusic, 3845.0 / 960.0);
        assert_eq!(context.projectTimeSamples, 444_000);
        assert_eq!(context.continousTimeSamples, 123);
        assert_eq!(context.sampleRate, 48_000.0);
        assert_eq!(context.tempo, 137.0);
        assert_eq!(context.timeSigNumerator, 7);
        assert_eq!(context.timeSigDenominator, 8);
        assert_eq!(context.state, 512 | 1024 | 2048 | 8192 | 131072 | 2);
    }

    #[test]
    fn native_bars_keep_positions_immediately_before_the_downbeat_in_the_prior_bar() {
        let downbeat = 3845.0_f64 / 960.0;
        for (beats, start) in [
            (3844.0 / 960.0, 485.0 / 960.0),
            (3844.5 / 960.0, 485.0 / 960.0),
            (downbeat.next_down(), 485.0 / 960.0),
            (downbeat, 3845.0 / 960.0),
            (downbeat.next_up(), 3845.0 / 960.0),
            (3845.5 / 960.0, 3845.0 / 960.0),
            (7205.0 / 960.0, 7205.0 / 960.0),
        ] {
            let context = process_context(
                &Transport {
                    position_beats: beats,
                    numerator: 7,
                    denominator: 8,
                    meter_anchor: Some(MeterAnchor {
                        bar_origin_beats: 485.0 / 960.0,
                        bar_origin_index: 1,
                    }),
                    ..Transport::default()
                },
                48_000.0,
                0,
            )
            .unwrap_or_else(|_| panic!("valid transport"));
            assert_eq!(context.barPositionMusic, start, "{beats}");
            assert_eq!(context.projectTimeMusic, beats);
            assert_eq!(context.state & 2, 0);
        }
    }

    #[test]
    fn native_bar_fields_follow_shortened_song_bars() {
        let origin = 4001.0 / 960.0;
        for (beats, bar_start) in [
            (origin, origin),
            (origin + 3.499, origin),
            (origin + 3.5, origin + 3.5),
            (origin + 7.125, origin + 7.0),
        ] {
            let context = process_context(
                &Transport {
                    playing: true,
                    tempo_bpm: 137.0,
                    position_beats: beats,
                    position_seconds: 9.25,
                    numerator: 7,
                    denominator: 8,
                    meter_anchor: Some(MeterAnchor {
                        bar_origin_beats: origin,
                        bar_origin_index: 2,
                    }),
                },
                48_000.0,
                123,
            )
            .unwrap_or_else(|_| panic!("valid transport"));
            assert_eq!(context.barPositionMusic, bar_start);
            assert_eq!(context.projectTimeMusic, beats);
            assert_eq!(context.projectTimeSamples, 444_000);
            assert_eq!(context.continousTimeSamples, 123);
            assert_eq!(context.sampleRate, 48_000.0);
            assert_eq!(context.tempo, 137.0);
            assert_eq!(context.timeSigNumerator, 7);
            assert_eq!(context.timeSigDenominator, 8);
            assert_eq!(context.state, 512 | 1024 | 2048 | 8192 | 131072 | 2);
        }
    }

    #[test]
    fn native_scalar_pattern_transport_retains_beat_zero_origin() {
        let context = process_context(
            &Transport {
                position_beats: 8.125,
                ..Transport::default()
            },
            48_000.0,
            u64::MAX,
        )
        .unwrap_or_else(|_| panic!("valid scalar transport"));
        assert_eq!(context.barPositionMusic, 8.0);
        assert_eq!(context.continousTimeSamples, i64::MAX);
        assert_eq!(context.state & 2, 0);
    }

    #[test]
    fn invalid_anchor_refuses_native_transport_before_processing() {
        for anchor in [
            MeterAnchor {
                bar_origin_beats: f64::INFINITY,
                bar_origin_index: 0,
            },
            MeterAnchor {
                bar_origin_beats: -1.0,
                bar_origin_index: 0,
            },
            MeterAnchor {
                bar_origin_beats: 1.0,
                bar_origin_index: 0,
            },
            MeterAnchor {
                bar_origin_beats: 0.0,
                bar_origin_index: u32::MAX,
            },
        ] {
            assert!(
                process_context(
                    &Transport {
                        meter_anchor: Some(anchor),
                        ..Transport::default()
                    },
                    48_000.0,
                    0
                )
                .is_err()
            );
        }
    }
}
