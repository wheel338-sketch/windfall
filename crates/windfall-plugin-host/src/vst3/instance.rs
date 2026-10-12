//! Main-thread VST3 ownership and lifecycle.
use super::*;
use super::{handlers::Handler, processor::VstProcessor, stream::Stream};
use crate::{
    events::{HostEvent, PluginEvent},
    gui::{EditorError, EditorInfo, EditorOptions},
    instance::PluginNotification,
    params::PluginParam,
    processor::{AudioShape, ProcessorBackend, ProcessorShared},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

pub(super) struct Value {
    pub id: u32,
    pub scale: f64,
    pub bits: AtomicU64,
    pub pending: AtomicBool,
    pub writable: bool,
}
impl Value {
    pub fn normalized(&self) -> f64 {
        f64::from_bits(self.bits.load(Ordering::Relaxed))
    }
    pub fn set(&self, value: f64) {
        if value.is_finite() {
            self.bits
                .store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }
}
pub(super) struct Objects {
    pub component: ComPtr<IComponent>,
    pub processor: ComPtr<IAudioProcessor>,
    pub controller: Option<ComPtr<IEditController>>,
    separate: bool,
    connection: Option<Connection>,
    _context: ComPtr<IHostApplication>,
    _module: Vst3Module,
}
// SAFETY: only IAudioProcessor is accessed on the single audio owner. The
// controller and lifecycle remain on the creating thread; destruction is
// deferred until the processor is returned to that thread.
unsafe impl Send for Objects {}
unsafe impl Sync for Objects {}
impl Drop for Objects {
    fn drop(&mut self) {
        drop(self.connection.take());
        // SAFETY: main-thread teardown after audio ownership has returned.
        unsafe {
            if let Some(controller) = &self.controller {
                controller.setComponentHandler(ptr::null_mut());
                if self.separate {
                    controller.terminate();
                }
            }
            self.component.terminate();
        }
    }
}
pub(super) struct VstInstance {
    objects: Arc<Objects>,
    layout: PluginLayout,
    params: Vec<PluginParam>,
    values: Arc<[Value]>,
    handler: ComWrapper<Handler>,
    active: bool,
    editor: Option<super::editor::Editor>,
    controller_values: Vec<u64>,
    deactivation_values: Vec<(u32, f64, Option<f64>)>,
}
fn utf16(text: &[u16]) -> String {
    String::from_utf16_lossy(&text[..text.iter().position(|&v| v == 0).unwrap_or(text.len())])
}
impl VstInstance {
    pub fn new(module: Vst3Module, id: &str) -> Result<Self, PluginError> {
        let cid = parse_id(id).ok_or_else(|| PluginError::NotFound(id.into()))?;
        let component = module.object::<IComponent>(&cid)?;
        let context = ComWrapper::new(HostApplication)
            .to_com_ptr::<IHostApplication>()
            .expect("host");
        // SAFETY: live COM objects and bounded SDK outputs; all on main thread.
        unsafe {
            if component.initialize(context.as_ptr().cast()) != kResultOk {
                return Err(PluginError::Create("component initialize refused".into()));
            }
        }
        let initialized = Initialized(component.clone());
        let processor = component
            .cast::<IAudioProcessor>()
            .ok_or_else(|| PluginError::Create("no audio processor".into()))?;
        let mut separate_guard = None;
        let controller = if let Some(controller) = component.cast::<IEditController>() {
            Some(controller)
        } else {
            let mut cid = [0; 16];
            // SAFETY: live initialized component and sized class ID.
            if unsafe { component.getControllerClassId(&mut cid) } == kResultOk {
                let controller = module.object::<IEditController>(&cid)?;
                // SAFETY: live host and controller, retained until terminate.
                if unsafe { controller.initialize(context.as_ptr().cast()) } != kResultOk {
                    return Err(PluginError::Create("controller initialize refused".into()));
                }
                separate_guard = Some(Initialized(controller.clone()));
                Some(controller)
            } else {
                None
            }
        };
        let connection = controller
            .as_ref()
            .map(|controller| Connection::new(&component, controller));
        let handler = Handler::new();
        let mut layout = PluginLayout {
            has_state: true,
            ..PluginLayout::default()
        };
        let mut params = Vec::new();
        let mut values = Vec::new();
        // SAFETY: initialized interfaces and sized output structures.
        unsafe {
            for (direction, ports) in [
                (0, &mut layout.audio_inputs),
                (1, &mut layout.audio_outputs),
            ] {
                let count = component.getBusCount(0, direction);
                if !(0..=MAX_PORTS as i32).contains(&count) {
                    return Err(PluginError::Layout("absurd bus count".into()));
                }
                for index in 0..count {
                    let mut bus: BusInfo = std::mem::zeroed();
                    if component.getBusInfo(0, direction, index, &mut bus) != kResultOk
                        || !(0..=MAX_PORT_CHANNELS as i32).contains(&bus.channelCount)
                    {
                        return Err(PluginError::Layout("invalid bus".into()));
                    }
                    ports.push(AudioPort {
                        name: utf16(&bus.name),
                        channels: bus.channelCount as u32,
                        main: bus.busType == 0,
                    });
                }
            }
            for (direction, count) in [(0, &mut layout.note_inputs), (1, &mut layout.note_outputs)]
            {
                let buses = component.getBusCount(1, direction);
                if !(0..=MAX_PORTS as i32).contains(&buses) {
                    return Err(PluginError::Layout("absurd event bus count".into()));
                }
                *count = buses as u32;
            }
            if let Some(controller) = &controller {
                let handler_ptr = handler.to_com_ptr::<IComponentHandler>().expect("handler");
                controller.setComponentHandler(handler_ptr.as_ptr());
                // A separate controller must receive the component's initial state.
                let stream = Stream::new(Vec::new(), crate::state::MAX_STATE_BYTES, true);
                let iface = stream.to_com_ptr::<IBStream>().expect("stream");
                if component.getState(iface.as_ptr()) == kResultOk {
                    stream.rewind();
                    controller.setComponentState(iface.as_ptr());
                }
                let count = controller.getParameterCount();
                if !(0..=MAX_PARAMETERS as i32).contains(&count) {
                    return Err(PluginError::Layout("absurd parameter count".into()));
                }
                let mut units = std::collections::BTreeMap::new();
                if let Some(info) = controller.cast::<IUnitInfo>() {
                    let count = info.getUnitCount();
                    if !(0..=MAX_PARAMETERS as i32).contains(&count) {
                        return Err(PluginError::Layout("absurd unit count".into()));
                    }
                    for index in 0..count {
                        let mut unit: UnitInfo = std::mem::zeroed();
                        if info.getUnitInfo(index, &mut unit) == kResultOk {
                            units.insert(unit.id, (unit.parentUnitId, utf16(&unit.name)));
                        }
                    }
                }
                for index in 0..count {
                    let mut p: ParameterInfo = std::mem::zeroed();
                    if controller.getParameterInfo(index, &mut p) != kResultOk
                        || p.id == u32::MAX
                        || p.stepCount < 0
                        || !p.defaultNormalizedValue.is_finite()
                        || !(0.0..=1.0).contains(&p.defaultNormalizedValue)
                        || values.iter().any(|v: &Value| v.id == p.id)
                    {
                        return Err(PluginError::Layout("invalid parameter".into()));
                    }
                    let scale = f64::from(p.stepCount.max(1));
                    let mut unit = p.unitId;
                    let mut names = Vec::new();
                    let mut visited = Vec::new();
                    for _ in 0..32 {
                        if visited.contains(&unit) {
                            break;
                        }
                        visited.push(unit);
                        let Some((parent, name)) = units.get(&unit) else {
                            break;
                        };
                        if !name.is_empty() {
                            names.push(name.as_str());
                        }
                        unit = *parent;
                    }
                    names.reverse();
                    let current = controller.getParamNormalized(p.id);
                    values.push(Value {
                        id: p.id,
                        scale,
                        pending: AtomicBool::new(false),
                        writable: p.flags & 2 == 0,
                        bits: AtomicU64::new(
                            if current.is_finite() {
                                current.clamp(0.0, 1.0)
                            } else {
                                p.defaultNormalizedValue
                            }
                            .to_bits(),
                        ),
                    });
                    params.push(PluginParam {
                        id: p.id,
                        name: utf16(&p.title),
                        module: names.join("/"),
                        min: 0.0,
                        max: scale,
                        default: p.defaultNormalizedValue * scale,
                        stepped: p.stepCount > 0,
                        choice: p.flags & 8 != 0,
                        automatable: p.flags & 1 != 0,
                        read_only: p.flags & 2 != 0,
                        hidden: p.flags & 16 != 0,
                        bypass: p.flags & 65536 != 0,
                        periodic: p.flags & 4 != 0,
                    });
                }
                layout.has_editor =
                    ComPtr::from_raw(controller.createView(c"editor".as_ptr())).is_some();
            }
        }
        layout.parameter_count = params.len() as u32;
        values.sort_by_key(|v| v.id);
        let controller_values = values.iter().map(|v| v.normalized().to_bits()).collect();
        let deactivation_values = values
            .iter()
            .filter(|v| v.writable)
            .map(|v| (v.id, 0.0, None))
            .collect();
        let separate = separate_guard.is_some();
        // Ownership of termination moves to Objects. Drop COM refs without
        // calling terminate a second time (forget only the initialized wrappers).
        drop(initialized.into_inner());
        if let Some(controller_guard) = separate_guard {
            drop(controller_guard.into_inner());
        }
        Ok(Self {
            objects: Arc::new(Objects {
                component,
                processor,
                controller,
                separate,
                connection,
                _context: context,
                _module: module,
            }),
            layout,
            params,
            values: values.into(),
            handler,
            active: false,
            editor: None,
            controller_values,
            deactivation_values,
        })
    }
    fn value(&self, id: u32) -> Option<&Value> {
        self.values
            .binary_search_by_key(&id, |v| v.id)
            .ok()
            .map(|i| &self.values[i])
    }
    fn sync_values(&self) {
        if let Some(controller) = &self.objects.controller {
            for value in self.values.iter() {
                // SAFETY: main-thread controller access, after state or idle.
                value.set(unsafe { controller.getParamNormalized(value.id) });
            }
        }
    }
}
impl InstanceBackend for VstInstance {
    fn quiesce(&mut self, processor: &mut dyn ProcessorBackend) -> Result<(), PluginError> {
        let processor = processor
            .as_any_mut()
            .downcast_mut::<VstProcessor>()
            .ok_or_else(|| PluginError::Deactivate("incorrect processor backend".into()))?;
        if let Some(controller) = &self.objects.controller {
            for (id, before, after) in &mut self.deactivation_values {
                *before = unsafe { controller.getParamNormalized(*id) };
                *after = None;
            }
        }
        processor.quiesce()?;
        // SAFETY: exclusive returned processor, creating owner thread.
        if unsafe { self.objects.component.setActive(0) } != kResultOk {
            return Err(PluginError::Deactivate("setActive(false) refused".into()));
        }
        self.active = false;
        if let Some(controller) = &self.objects.controller {
            for (id, before, after) in &mut self.deactivation_values {
                let value = unsafe { controller.getParamNormalized(*id) };
                if value.is_finite() && value.to_bits() != before.to_bits() {
                    *after = Some(value.clamp(0.0, 1.0));
                }
            }
        }
        Ok(())
    }
    fn deactivation_param_value(&self, id: u32) -> Option<f64> {
        self.deactivation_values
            .iter()
            .find(|edit| edit.0 == id)?
            .2
            .map(|value| value * self.value(id).expect("known parameter").scale)
    }
    fn flush_retired_params(&mut self, changes: &[HostEvent], out: &mut dyn FnMut(PluginEvent)) {
        for change in changes {
            if let HostEvent::Param { id, .. } = change
                && self.deactivation_param_value(*id).is_none()
            {
                self.flush_params(std::slice::from_ref(change), out);
            }
        }
    }
    fn layout(&self) -> &PluginLayout {
        &self.layout
    }
    fn shape(&self) -> AudioShape {
        AudioShape {
            has_input: self
                .layout
                .audio_inputs
                .iter()
                .any(|p| p.main && p.channels > 0),
            in_place: false,
        }
    }
    fn params(&self) -> &[PluginParam] {
        &self.params
    }
    fn param_value(&mut self, id: u32) -> Option<f64> {
        if !self.active
            && let Some(controller) = &self.objects.controller
            && let Some(v) = self.value(id)
        {
            let value = unsafe { controller.getParamNormalized(id) };
            if value.is_finite() && value.to_bits() != v.normalized().to_bits() {
                v.set(value);
                if v.writable {
                    v.pending.store(true, Ordering::Relaxed);
                }
            }
        }
        self.value(id).map(|v| v.normalized() * v.scale)
    }
    fn param_text(&mut self, id: u32, value: f64) -> Option<String> {
        let v = self.value(id)?;
        let controller = self.objects.controller.as_ref()?;
        let mut text = [0; 128];
        // SAFETY: live main-thread controller, valid ID and String128 output.
        (unsafe {
            controller.getParamStringByValue(id, (value / v.scale).clamp(0.0, 1.0), &mut text)
        } == kResultOk)
            .then(|| utf16(&text))
    }
    fn param_from_text(&mut self, id: u32, text: &str) -> Option<f64> {
        let v = self.value(id)?;
        let controller = self.objects.controller.as_ref()?;
        let text: Vec<u16> = text.encode_utf16().take(127).chain(Some(0)).collect();
        let mut value = 0.0;
        // SAFETY: null-terminated input and live main-thread controller.
        (unsafe { controller.getParamValueByString(id, text.as_ptr().cast_mut(), &mut value) }
            == kResultOk
            && value.is_finite())
        .then_some(value.clamp(0.0, 1.0) * v.scale)
    }
    fn flush_params(&mut self, changes: &[HostEvent], _out: &mut dyn FnMut(PluginEvent)) {
        if let Some(controller) = &self.objects.controller {
            for change in changes {
                if let HostEvent::Param { id, value, .. } = *change
                    && let Some(v) = self.value(id)
                {
                    v.set(value / v.scale);
                    v.pending.store(true, Ordering::Relaxed);
                    // SAFETY: inactive main-thread controller access. Initial
                    // parameter points are also sent on the first audio block.
                    unsafe {
                        controller.setParamNormalized(id, v.normalized());
                    }
                }
            }
        }
    }
    fn save_state(&mut self, limit: usize) -> Result<Option<Vec<u8>>, PluginError> {
        if self.active {
            return Err(PluginError::State(
                "saved while active; return the processor first",
            ));
        }
        // Controller edits made while inactive are not delivered through
        // process until activation. Preserve those edits alongside component
        // state rather than silently saving the component's older values.
        if let Some(controller) = &self.objects.controller {
            for v in self.values.iter() {
                let value = unsafe { controller.getParamNormalized(v.id) };
                if value.is_finite() && value.to_bits() != v.normalized().to_bits() {
                    v.set(value);
                    v.pending.store(true, Ordering::Relaxed);
                }
            }
        }
        let pending: Vec<_> = self
            .values
            .iter()
            .filter(|v| v.writable && v.pending.load(Ordering::Relaxed))
            .map(|v| (v.id, v.normalized()))
            .collect();
        let overhead = 16 + pending.len() * 12;
        let stream = Stream::new(Vec::new(), limit.saturating_sub(overhead), true);
        let iface = stream.to_com_ptr::<IBStream>().expect("stream");
        // SAFETY: inactive component and bounded main-thread stream.
        if unsafe { self.objects.component.getState(iface.as_ptr()) } != kResultOk
            || stream.failed()
        {
            return Err(PluginError::State("saved"));
        }
        let component = stream.bytes();
        let controller_bytes = if let Some(controller) = &self.objects.controller {
            let stream = Stream::new(
                Vec::new(),
                limit.saturating_sub(overhead + component.len()),
                true,
            );
            let iface = stream.to_com_ptr::<IBStream>().expect("stream");
            // SAFETY: inactive main-thread controller and bounded stream.
            let result = unsafe { controller.getState(iface.as_ptr()) };
            if stream.failed() {
                return Err(PluginError::State("saved within the state limit"));
            }
            if result == kResultOk {
                stream.bytes()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        if component.len() + controller_bytes.len() + overhead > limit {
            return Err(PluginError::State("saved within the state limit"));
        }
        let mut bytes = b"VST2".to_vec();
        bytes.extend_from_slice(&(component.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(controller_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(pending.len() as u32).to_le_bytes());
        bytes.extend(component);
        bytes.extend(controller_bytes);
        for (id, value) in pending {
            bytes.extend_from_slice(&id.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        Ok(Some(bytes))
    }
    fn load_state(&mut self, bytes: &[u8]) -> Result<(), PluginError> {
        if self.active {
            return Err(PluginError::State(
                "loaded while active; return the processor first",
            ));
        }
        if bytes.len() < 16 || &bytes[..4] != b"VST2" {
            return Err(PluginError::State("loaded: invalid VST3 state"));
        }
        let a = u32::from_le_bytes(bytes[4..8].try_into().expect("length")) as usize;
        let b = u32::from_le_bytes(bytes[8..12].try_into().expect("length")) as usize;
        let count = u32::from_le_bytes(bytes[12..16].try_into().expect("length")) as usize;
        if count > self.values.len()
            || a.checked_add(b)
                .and_then(|n| n.checked_add(16))
                .and_then(|n| count.checked_mul(12).and_then(|extra| n.checked_add(extra)))
                != Some(bytes.len())
        {
            return Err(PluginError::State("loaded: invalid VST3 state length"));
        }
        let mut overrides = Vec::with_capacity(count);
        for entry in bytes[16 + a + b..].as_chunks::<12>().0 {
            let id = u32::from_le_bytes(entry[..4].try_into().expect("id"));
            let value = f64::from_le_bytes(entry[4..].try_into().expect("value"));
            if self.value(id).is_none_or(|v| !v.writable)
                || !value.is_finite()
                || !(0.0..=1.0).contains(&value)
                || overrides.iter().any(|(previous, _)| *previous == id)
            {
                return Err(PluginError::State("loaded: invalid pending parameter"));
            }
            overrides.push((id, value));
        }
        let stream = Stream::new(bytes[16..16 + a].to_vec(), a, false);
        let iface = stream.to_com_ptr::<IBStream>().expect("stream");
        // SAFETY: inactive interfaces and bounded streams on their owner thread.
        unsafe {
            if self.objects.component.setState(iface.as_ptr()) != kResultOk {
                return Err(PluginError::State("loaded"));
            }
            if let Some(controller) = &self.objects.controller {
                stream.rewind();
                controller.setComponentState(iface.as_ptr());
                if b > 0 {
                    let stream = Stream::new(bytes[16 + a..16 + a + b].to_vec(), b, false);
                    let iface = stream.to_com_ptr::<IBStream>().expect("stream");
                    if controller.setState(iface.as_ptr()) != kResultOk {
                        return Err(PluginError::State("loaded: controller"));
                    }
                }
            }
        }
        self.sync_values();
        for v in self.values.iter() {
            v.pending.store(false, Ordering::Relaxed);
        }
        for (id, value) in overrides {
            let v = self.value(id).expect("validated ID");
            v.set(value);
            v.pending.store(true, Ordering::Relaxed);
            if let Some(controller) = &self.objects.controller {
                unsafe {
                    controller.setParamNormalized(id, value);
                }
            }
        }
        Ok(())
    }
    fn activate(
        &mut self,
        sample_rate: f64,
        max_block: u32,
        shared: &ProcessorShared,
    ) -> Result<Box<dyn ProcessorBackend>, PluginError> {
        let (producer, consumer) = rtrb::RingBuffer::new(crate::instance::QUEUE_CAPACITY);
        self.handler.set_queue(Some(producer));
        let processor = VstProcessor::new(
            self.objects.clone(),
            &self.layout,
            self.values.clone(),
            sample_rate,
            max_block,
            consumer,
        )?;
        // SAFETY: initialized processor queried off the audio thread.
        unsafe {
            shared.latency.store(
                self.objects.processor.getLatencySamples(),
                Ordering::Relaxed,
            );
            shared
                .tail
                .store(self.objects.processor.getTailSamples(), Ordering::Relaxed);
        }
        self.active = true;
        for (_, _, after) in &mut self.deactivation_values {
            *after = None;
        }
        Ok(Box::new(processor))
    }
    fn finish_deactivation(&mut self, processor: Option<Box<dyn ProcessorBackend>>) {
        self.handler.set_queue(None);
        // The outer instance's Drop calls this with None while an audio
        // owner may still run. Leave active true: Drop deliberately keeps
        // the entire COM/module ownership graph alive in this misuse case.
        let Some(processor) = processor else {
            return;
        };
        let Ok(mut processor) = processor.into_any().downcast::<VstProcessor>() else {
            return;
        };
        processor.retain_pending_edits();
        processor.stop();
        drop(processor);
        // Native lifecycle edits are newer than every returned editor/audio
        // point. Restore them after retaining those older pending points.
        for (id, _, after) in &self.deactivation_values {
            if let Some(value) = after {
                let v = self.value(*id).expect("known parameter");
                v.set(*value);
                v.pending.store(true, Ordering::Relaxed);
            }
        }
        if let Some(controller) = &self.objects.controller {
            for value in self.values.iter() {
                if value.pending.load(Ordering::Relaxed) {
                    unsafe {
                        controller.setParamNormalized(value.id, value.normalized());
                    }
                }
            }
        }
    }
    fn idle(
        &mut self,
        _now: Instant,
        active: Option<&ProcessorShared>,
        notify: &mut dyn FnMut(PluginNotification),
    ) {
        let values = &self.values;
        let controller = &self.objects.controller;
        self.handler.drain(&mut |event| {
            let event = if let PluginEvent::ParamValue { id, value } = event {
                if let Ok(index) = values.binary_search_by_key(&id, |v| v.id) {
                    let v = &values[index];
                    v.set(value);
                    if active.is_none() {
                        v.pending.store(true, Ordering::Relaxed);
                    }
                    PluginEvent::ParamValue {
                        id,
                        value: value * v.scale,
                    }
                } else {
                    return;
                }
            } else {
                event
            };
            notify(event.into());
        });
        if let Some(controller) = controller
            && active.is_some()
        {
            for (v, previous) in values.iter().zip(&mut self.controller_values) {
                let value = v.normalized();
                if *previous == value.to_bits() {
                    continue;
                }
                *previous = value.to_bits();
                unsafe {
                    controller.setParamNormalized(v.id, value);
                }
            }
        }
        let flags = self.handler.flags.swap(0, Ordering::Relaxed);
        if self.handler.dirty.swap(false, Ordering::Relaxed) {
            notify(PluginNotification::StateChanged);
        }
        if flags & 8 != 0 {
            let latency = unsafe { self.objects.processor.getLatencySamples() };
            if let Some(shared) = active {
                shared.latency.store(latency, Ordering::Relaxed);
            }
            notify(PluginNotification::LatencyChanged { samples: latency });
        }
        if flags & 4 != 0 {
            if let Some(controller) = &self.objects.controller {
                for v in values.iter() {
                    let value = unsafe { controller.getParamNormalized(v.id) };
                    if value.is_finite() && value.to_bits() != v.normalized().to_bits() {
                        v.set(value);
                        if v.writable {
                            v.pending.store(true, Ordering::Relaxed);
                        }
                    }
                }
            }
            notify(PluginNotification::ParamsRescanned);
        }
        if flags & !12 != 0 {
            notify(PluginNotification::RestartRequested);
        }
        if let Some(editor) = &mut self.editor
            && editor.idle(notify)
        {
            self.close_editor();
            notify(PluginNotification::EditorClosed);
        }
    }
    fn next_deadline(&self) -> Option<Instant> {
        self.editor
            .as_ref()
            .map(|_| Instant::now() + std::time::Duration::from_millis(16))
    }
    fn open_editor(&mut self, options: &EditorOptions) -> Result<EditorInfo, EditorError> {
        if self.editor.is_some() {
            return Err(EditorError::AlreadyOpen);
        }
        let controller = self
            .objects
            .controller
            .as_ref()
            .ok_or(EditorError::NoEditor)?;
        let editor = super::editor::Editor::open(controller, options)?;
        let info = editor.info();
        self.editor = Some(editor);
        Ok(info)
    }
    fn close_editor(&mut self) {
        self.editor = None;
    }
    fn editor(&self) -> Option<EditorInfo> {
        self.editor.as_ref().map(super::editor::Editor::info)
    }

    fn set_editor_scale(&mut self, application: f64) -> Result<EditorInfo, EditorError> {
        self.editor
            .as_mut()
            .ok_or(EditorError::NoEditor)?
            .set_scale(application)
    }
}
impl Drop for VstInstance {
    fn drop(&mut self) {
        self.close_editor();
        // Losing an outstanding processor cannot safely terminate a live
        // plugin, nor unload its DLL from the audio thread. Match CLAP's
        // deliberate leak for this API misuse.
        if self.active && Arc::strong_count(&self.objects) > 1 {
            std::mem::forget(self.objects.clone());
        }
    }
}
