//! AUv2 adapter for installed macOS components. Native acceptance requires a Mac.
//! The OS registry identifies components by type/subtype/manufacturer, not a DLL.
mod sys;
use crate::events::{PluginEvent, Transport};
use crate::instance::InstanceBackend;
use crate::processor::{
    AudioIo, AudioShape, BlockResult, ProcessFailed, ProcessStatus, ProcessorBackend,
    ProcessorShared,
};
use crate::{
    AudioPort, EditorError, EditorInfo, EditorOptions, HostEvent, PluginDescriptor, PluginError,
    PluginFormat, PluginKind, PluginLayout, PluginNotification, PluginParam,
};
use std::{
    any::Any,
    ffi::c_void,
    ptr,
    sync::{Arc, atomic::Ordering},
    time::Instant,
};
use sys::*;
const GLOBAL: u32 = 0;
const INPUT_SCOPE: u32 = 1;
const OUTPUT_SCOPE: u32 = 2;
const CLASS_INFO: u32 = 0;
const PARAMETER_LIST: u32 = 3;
const PARAMETER_INFO: u32 = 4;
const STREAM_FORMAT: u32 = 8;
const LATENCY: u32 = 12;
const MAX_FRAMES: u32 = 14;
const TAIL: u32 = 20;
const RENDER_CALLBACK: u32 = 23;
fn checked(status: i32, operation: &str) -> Result<(), PluginError> {
    if status == 0 {
        Ok(())
    } else {
        Err(PluginError::Create(format!(
            "Audio Unit {operation}: OSStatus {status}"
        )))
    }
}
fn fourcc(value: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*value)
}
fn id(description: Description) -> String {
    format!(
        "{:08X}:{:08X}:{:08X}",
        description.kind, description.subtype, description.manufacturer
    )
}
fn text(value: Cf) -> String {
    if value.is_null() {
        return String::new();
    }
    let mut bytes = [0_i8; 1024];
    // SAFETY: registry and parameter APIs return CFString references.
    if unsafe { CFStringGetCString(value, bytes.as_mut_ptr(), bytes.len() as isize, 0x08000100) }
        == 0
    {
        return String::new();
    }
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(
        &bytes[..end]
            .iter()
            .map(|byte| *byte as u8)
            .collect::<Vec<_>>(),
    )
    .into_owned()
}
/// System registration discovery, called on the Cocoa main thread.
pub fn descriptors() -> Vec<PluginDescriptor> {
    let mut result = Vec::new();
    for kind in [fourcc(b"aufx"), fourcc(b"aumu"), fourcc(b"aumf")] {
        let query = Description {
            kind,
            ..Description::default()
        };
        let mut component = ptr::null_mut();
        for _ in 0..65_536 {
            // SAFETY: component iteration and output references follow AudioComponent API.
            component = unsafe { AudioComponentFindNext(component, &query) };
            if component.is_null() {
                break;
            }
            let mut description = Description::default();
            let mut name = ptr::null();
            let mut version = 0;
            if unsafe { AudioComponentGetDescription(component, &mut description) } != 0 {
                continue;
            }
            unsafe {
                AudioComponentCopyName(component, &mut name);
                AudioComponentGetVersion(component, &mut version);
            }
            let name_text = text(name);
            if !name.is_null() {
                unsafe {
                    CFRelease(name);
                }
            }
            result.push(PluginDescriptor {
                format: PluginFormat::Au,
                id: id(description),
                name: name_text,
                vendor: String::new(),
                version: version.to_string(),
                features: vec![],
                kind: if kind == fourcc(b"aumu") {
                    PluginKind::Instrument
                } else {
                    PluginKind::Effect
                },
            });
        }
    }
    result
}
struct Native {
    unit: Unit,
}
// SAFETY: AudioUnit DSP ownership transfers exclusively to one processor while
// control operations require its return. The unit is never shared for rendering.
unsafe impl Send for Native {}
unsafe impl Sync for Native {}
impl Drop for Native {
    fn drop(&mut self) {
        unsafe {
            AudioComponentInstanceDispose(self.unit);
        }
    }
}
/// Create an installed AUv2 component by registry identity; no path substitution.
pub(crate) fn create(
    wanted: &str,
) -> Result<(PluginDescriptor, Box<dyn InstanceBackend>), PluginError> {
    let descriptor = descriptors()
        .into_iter()
        .find(|d| d.id == wanted)
        .ok_or_else(|| PluginError::NotFound(wanted.into()))?;
    let words: Vec<_> = wanted
        .split(':')
        .map(|word| u32::from_str_radix(word, 16))
        .collect::<Result<_, _>>()
        .map_err(|_| PluginError::Create("invalid AU component id".into()))?;
    if words.len() != 3 {
        return Err(PluginError::Create("invalid AU component id".into()));
    }
    let description = Description {
        kind: words[0],
        subtype: words[1],
        manufacturer: words[2],
        ..Description::default()
    };
    let component = unsafe { AudioComponentFindNext(ptr::null_mut(), &description) };
    if component.is_null() {
        return Err(PluginError::NotFound(wanted.into()));
    }
    let mut unit = ptr::null_mut();
    checked(
        unsafe { AudioComponentInstanceNew(component, &mut unit) },
        "instantiate",
    )?;
    if unit.is_null() {
        return Err(PluginError::Create("AU returned a null instance".into()));
    }
    let native = Arc::new(Native { unit });
    let mut size = 0;
    let mut writable = 0;
    checked(
        unsafe {
            AudioUnitGetPropertyInfo(unit, PARAMETER_LIST, GLOBAL, 0, &mut size, &mut writable)
        },
        "parameter list",
    )?;
    if size % 4 != 0 || size / 4 > crate::descriptor::MAX_PARAMETERS {
        return Err(PluginError::Layout(
            "AU parameter count exceeds bound".into(),
        ));
    }
    let mut ids = vec![0_u32; (size / 4) as usize];
    if size > 0 {
        checked(
            unsafe {
                AudioUnitGetProperty(
                    unit,
                    PARAMETER_LIST,
                    GLOBAL,
                    0,
                    ids.as_mut_ptr().cast(),
                    &mut size,
                )
            },
            "parameters",
        )?;
    }
    let mut params = Vec::with_capacity(ids.len());
    for id in ids {
        let mut info: ParameterInfo = unsafe { std::mem::zeroed() };
        let mut bytes = size_of::<ParameterInfo>() as u32;
        checked(
            unsafe {
                AudioUnitGetProperty(
                    unit,
                    PARAMETER_INFO,
                    GLOBAL,
                    id,
                    (&mut info as *mut ParameterInfo).cast(),
                    &mut bytes,
                )
            },
            "parameter metadata",
        )?;
        let has_cf_name = info.flags & (1 << 27) != 0;
        let name = if has_cf_name {
            text(info.cf_name)
        } else {
            let end = info
                .name
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(info.name.len());
            String::from_utf8_lossy(
                &info.name[..end]
                    .iter()
                    .map(|byte| *byte as u8)
                    .collect::<Vec<_>>(),
            )
            .into_owned()
        };
        if has_cf_name && info.flags & (1 << 4) != 0 && !info.cf_name.is_null() {
            unsafe {
                CFRelease(info.cf_name);
            }
        }
        if !info.min.is_finite()
            || !info.max.is_finite()
            || info.min > info.max
            || !info.default.is_finite()
        {
            return Err(PluginError::Layout("invalid AU parameter range".into()));
        }
        params.push(PluginParam {
            id,
            name,
            module: String::new(),
            min: f64::from(info.min),
            max: f64::from(info.max),
            default: f64::from(info.default.clamp(info.min, info.max)),
            stepped: false,
            choice: false,
            automatable: info.flags & (1 << 31) != 0,
            read_only: info.flags & (1 << 31) == 0,
            hidden: false,
            bypass: false,
            periodic: false,
        });
    }
    let has_input = descriptor.kind != PluginKind::Instrument;
    let layout = PluginLayout {
        audio_inputs: if has_input {
            vec![AudioPort {
                name: "Main".into(),
                channels: 2,
                main: true,
            }]
        } else {
            vec![]
        },
        audio_outputs: vec![AudioPort {
            name: "Main".into(),
            channels: 2,
            main: true,
        }],
        note_inputs: u32::from(!has_input || description.kind == fourcc(b"aumf")),
        note_outputs: 0,
        has_editor: false,
        parameter_count: params.len() as u32,
        has_state: true,
    };
    Ok((
        descriptor,
        Box::new(Instance {
            native,
            params,
            layout,
            active: false,
            failed_input: None,
        }),
    ))
}
struct Instance {
    native: Arc<Native>,
    params: Vec<PluginParam>,
    layout: PluginLayout,
    active: bool,
    // Installed callbacks can retain their context even when Initialize fails.
    // Keep it alive until an owner-thread callback removal or unit disposal.
    failed_input: Option<Box<Input>>,
}
fn property<T>(unit: Unit, property: u32, scope: u32, value: &T) -> Result<(), PluginError> {
    checked(
        unsafe {
            AudioUnitSetProperty(
                unit,
                property,
                scope,
                0,
                (value as *const T).cast(),
                size_of::<T>() as u32,
            )
        },
        "set property",
    )
}
fn scalar(unit: Unit, property: u32) -> f64 {
    let mut value = 0.0_f64;
    let mut size = size_of::<f64>() as u32;
    if unsafe {
        AudioUnitGetProperty(
            unit,
            property,
            GLOBAL,
            0,
            (&mut value as *mut f64).cast(),
            &mut size,
        )
    } == 0
        && value.is_finite()
        && value >= 0.0
    {
        value
    } else {
        0.0
    }
}
impl InstanceBackend for Instance {
    fn layout(&self) -> &PluginLayout {
        &self.layout
    }
    fn shape(&self) -> AudioShape {
        AudioShape {
            has_input: !self.layout.audio_inputs.is_empty(),
            in_place: false,
        }
    }
    fn params(&self) -> &[PluginParam] {
        &self.params
    }
    fn param_value(&mut self, id: u32) -> Option<f64> {
        let mut value = 0.0;
        (unsafe { AudioUnitGetParameter(self.native.unit, id, GLOBAL, 0, &mut value) } == 0
            && value.is_finite())
        .then_some(f64::from(value))
    }
    fn param_text(&mut self, id: u32, value: f64) -> Option<String> {
        self.params
            .iter()
            .any(|p| p.id == id)
            .then(|| value.to_string())
    }
    fn param_from_text(&mut self, id: u32, text: &str) -> Option<f64> {
        let param = self.params.iter().find(|p| p.id == id)?;
        let value: f64 = text.parse().ok()?;
        value.is_finite().then(|| param.clamp(value))
    }
    fn flush_params(&mut self, changes: &[HostEvent], _: &mut dyn FnMut(PluginEvent)) {
        for event in changes {
            if let HostEvent::Param { id, value, .. } = *event {
                if let Some(param) = self.params.iter().find(|p| p.id == id) {
                    unsafe {
                        AudioUnitSetParameter(
                            self.native.unit,
                            id,
                            GLOBAL,
                            0,
                            param.clamp(value) as f32,
                            0,
                        );
                    }
                }
            }
        }
    }
    fn save_state(&mut self, limit: usize) -> Result<Option<Vec<u8>>, PluginError> {
        if self.active {
            return Err(PluginError::State("saved while active"));
        }
        let mut list: Cf = ptr::null();
        let mut size = size_of::<Cf>() as u32;
        checked(
            unsafe {
                AudioUnitGetProperty(
                    self.native.unit,
                    CLASS_INFO,
                    GLOBAL,
                    0,
                    (&mut list as *mut Cf).cast(),
                    &mut size,
                )
            },
            "class info",
        )?;
        if list.is_null() {
            return Err(PluginError::State("saved"));
        }
        let data = unsafe { CFPropertyListCreateData(ptr::null(), list, 200, 0, ptr::null_mut()) };
        unsafe {
            CFRelease(list);
        }
        if data.is_null() {
            return Err(PluginError::State("serialized"));
        }
        let length = unsafe { CFDataGetLength(data) };
        let result = if length < 0 || length as usize > limit {
            Err(PluginError::State("saved within size bound"))
        } else {
            Ok(Some(
                unsafe { std::slice::from_raw_parts(CFDataGetBytePtr(data), length as usize) }
                    .to_vec(),
            ))
        };
        unsafe {
            CFRelease(data);
        }
        result
    }
    fn load_state(&mut self, bytes: &[u8]) -> Result<(), PluginError> {
        if self.active || bytes.len() > crate::state::MAX_STATE_BYTES {
            return Err(PluginError::State("restored"));
        }
        let data = unsafe { CFDataCreate(ptr::null(), bytes.as_ptr(), bytes.len() as isize) };
        if data.is_null() {
            return Err(PluginError::State("decoded"));
        }
        let list = unsafe {
            CFPropertyListCreateWithData(ptr::null(), data, 0, ptr::null_mut(), ptr::null_mut())
        };
        unsafe {
            CFRelease(data);
        }
        if list.is_null() {
            return Err(PluginError::State("decoded"));
        }
        let result = property(self.native.unit, CLASS_INFO, GLOBAL, &list);
        unsafe {
            CFRelease(list);
        }
        result
    }
    fn activate(
        &mut self,
        rate: f64,
        block: u32,
        shared: &ProcessorShared,
    ) -> Result<Box<dyn ProcessorBackend>, PluginError> {
        if self.failed_input.is_some() {
            checked(
                unsafe { AudioUnitUninitialize(self.native.unit) },
                "failed activation quiesce",
            )?;
            property(
                self.native.unit,
                RENDER_CALLBACK,
                INPUT_SCOPE,
                &Callback {
                    render: None,
                    context: ptr::null_mut(),
                },
            )?;
            drop(self.failed_input.take());
        }
        let format = StreamFormat {
            rate,
            format: fourcc(b"lpcm"),
            flags: 1 | (1 << 3) | (1 << 5),
            bytes_packet: 4,
            frames_packet: 1,
            bytes_frame: 4,
            channels: 2,
            bits: 32,
            reserved: 0,
        };
        property(self.native.unit, MAX_FRAMES, GLOBAL, &block)?;
        property(self.native.unit, STREAM_FORMAT, OUTPUT_SCOPE, &format)?;
        let mut input = Box::new(Input {
            left: vec![0.0; block as usize].into_boxed_slice(),
            right: vec![0.0; block as usize].into_boxed_slice(),
        });
        if self.shape().has_input {
            property(self.native.unit, STREAM_FORMAT, INPUT_SCOPE, &format)?;
            let installed = property(
                self.native.unit,
                RENDER_CALLBACK,
                INPUT_SCOPE,
                &Callback {
                    render: Some(render_input),
                    context: (&mut *input as *mut Input).cast(),
                },
            );
            if let Err(error) = installed {
                self.failed_input = Some(input);
                return Err(error);
            }
        }
        let initialized = checked(
            unsafe { AudioUnitInitialize(self.native.unit) },
            "initialize",
        );
        if let Err(error) = initialized {
            self.failed_input = Some(input);
            return Err(error);
        }
        let latency = scalar(self.native.unit, LATENCY) * rate;
        let tail = scalar(self.native.unit, TAIL) * rate;
        if latency > f64::from(u32::MAX) || tail > f64::from(u32::MAX) {
            unsafe {
                AudioUnitUninitialize(self.native.unit);
            }
            self.failed_input = Some(input);
            return Err(PluginError::Activate("AU timing exceeds bound".into()));
        }
        shared
            .latency
            .store(latency.ceil() as u32, Ordering::Relaxed);
        shared.tail.store(tail.ceil() as u32, Ordering::Relaxed);
        self.active = true;
        Ok(Box::new(Processor {
            native: Arc::clone(&self.native),
            input,
            max_block: block,
            tail: tail.ceil() as u32,
            reset_refused: false,
        }))
    }
    fn quiesce(&mut self, processor: &mut dyn ProcessorBackend) -> Result<(), PluginError> {
        if !processor
            .as_any_mut()
            .downcast_mut::<Processor>()
            .is_some_and(|p| Arc::ptr_eq(&p.native, &self.native))
        {
            return Err(PluginError::Deactivate(
                "AU processor owner mismatch".into(),
            ));
        }
        checked(
            unsafe { AudioUnitUninitialize(self.native.unit) },
            "uninitialize",
        )?;
        if self.shape().has_input {
            property(
                self.native.unit,
                RENDER_CALLBACK,
                INPUT_SCOPE,
                &Callback {
                    render: None,
                    context: ptr::null_mut(),
                },
            )?;
        }
        self.active = false;
        Ok(())
    }
    fn finish_deactivation(&mut self, processor: Option<Box<dyn ProcessorBackend>>) {
        if let Some(processor) = processor {
            drop(processor);
            self.active = false;
        } else if self.active {
            std::mem::forget(Arc::clone(&self.native));
        }
    }
    fn idle(
        &mut self,
        _: Instant,
        _: Option<&ProcessorShared>,
        _: &mut dyn FnMut(PluginNotification),
    ) {
    }
    fn next_deadline(&self) -> Option<Instant> {
        None
    }
    fn open_editor(&mut self, _: &EditorOptions) -> Result<EditorInfo, EditorError> {
        Err(EditorError::UnsupportedPlatform)
    }
    fn close_editor(&mut self) {}
    fn editor(&self) -> Option<EditorInfo> {
        None
    }
}
struct Input {
    left: Box<[f32]>,
    right: Box<[f32]>,
}
unsafe extern "C" fn render_input(
    context: *mut c_void,
    _: *mut u32,
    _: *const Timestamp,
    _: u32,
    frames: u32,
    list: *mut BufferList,
) -> i32 {
    if context.is_null() || list.is_null() {
        return -50;
    }
    // SAFETY: installed context remains boxed through uninitialize; callback is
    // synchronous within the exclusively owned AudioUnitRender invocation.
    let input = unsafe { &mut *context.cast::<Input>() };
    let list = unsafe { &mut *list };
    if frames as usize > input.left.len() || list.count != 2 {
        return -50;
    }
    for (buffer, source) in list
        .buffers
        .iter_mut()
        .zip([&mut input.left, &mut input.right])
    {
        if buffer.data.is_null() {
            buffer.data = source.as_mut_ptr().cast();
        } else {
            if buffer.bytes < frames * 4 {
                return -50;
            }
            unsafe {
                ptr::copy_nonoverlapping(
                    source.as_ptr(),
                    buffer.data.cast::<f32>(),
                    frames as usize,
                );
            }
        }
        buffer.channels = 1;
        buffer.bytes = frames * 4;
    }
    0
}
struct Processor {
    native: Arc<Native>,
    input: Box<Input>,
    max_block: u32,
    tail: u32,
    reset_refused: bool,
}
impl ProcessorBackend for Processor {
    fn process(
        &mut self,
        audio: AudioIo<'_>,
        events: &[HostEvent],
        _: &Transport,
        steady_time: u64,
        _: &mut dyn FnMut(PluginEvent),
    ) -> Result<BlockResult, ProcessFailed> {
        if self.reset_refused {
            return Err(ProcessFailed);
        }
        let (il, ir, ol, or) = match audio {
            AudioIo::Separate {
                in_left,
                in_right,
                out_left,
                out_right,
            } => (in_left, in_right, out_left, out_right),
            AudioIo::InPlace { .. } => return Err(ProcessFailed),
        };
        if ol.len() > self.max_block as usize {
            return Err(ProcessFailed);
        }
        self.input.left[..il.len()].copy_from_slice(il);
        self.input.right[..ir.len()].copy_from_slice(ir);
        let mut drops = 0;
        for event in events {
            let status = match *event {
                HostEvent::Param { time, id, value } => unsafe {
                    AudioUnitSetParameter(self.native.unit, id, GLOBAL, 0, value as f32, time)
                },
                HostEvent::NoteOn {
                    time,
                    key,
                    channel,
                    velocity,
                } => unsafe {
                    MusicDeviceMIDIEvent(
                        self.native.unit,
                        u32::from(0x90 | channel),
                        u32::from(key),
                        (velocity * 127.0).round() as u32,
                        time,
                    )
                },
                HostEvent::NoteOff {
                    time,
                    key,
                    channel,
                    velocity,
                } => unsafe {
                    MusicDeviceMIDIEvent(
                        self.native.unit,
                        u32::from(0x80 | channel),
                        u32::from(key),
                        (velocity * 127.0).round() as u32,
                        time,
                    )
                },
                HostEvent::AllNotesOff { time } => {
                    let mut status = 0;
                    for channel in 0..16 {
                        status |= unsafe {
                            MusicDeviceMIDIEvent(self.native.unit, 0xb0 | channel, 123, 0, time)
                        };
                    }
                    status
                }
                _ => {
                    drops += 1;
                    continue;
                }
            };
            if status != 0 {
                drops += 1;
            }
        }
        let frames = ol.len() as u32;
        let mut list = BufferList {
            count: 2,
            buffers: [
                Buffer {
                    channels: 1,
                    bytes: frames * 4,
                    data: ol.as_mut_ptr().cast(),
                },
                Buffer {
                    channels: 1,
                    bytes: frames * 4,
                    data: or.as_mut_ptr().cast(),
                },
            ],
        };
        let timestamp = Timestamp {
            sample: steady_time as f64,
            rate: 1.0,
            flags: 1,
            ..Timestamp::default()
        };
        let mut flags = 0;
        if unsafe {
            AudioUnitRender(
                self.native.unit,
                &mut flags,
                &timestamp,
                0,
                frames,
                &mut list,
            )
        } != 0
        {
            return Err(ProcessFailed);
        }
        Ok(BlockResult {
            status: ProcessStatus::Continue,
            tail: Some(self.tail),
            dropped_events: drops,
        })
    }
    fn reset(&mut self) {
        // Apple's AUBase declares Reset not realtime-safe. Refuse this active
        // processor so normal health handling requires owner deactivation and
        // preparation; never call AudioUnitReset from the callback.
        self.reset_refused = true;
    }
    fn stop(&mut self) {}
    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apple_effect_bypass_audio_parameter_state_and_processor_return() {
        let descriptor = descriptors()
            .into_iter()
            .find(|d| {
                d.kind == PluginKind::Effect
                    && d.id.ends_with(":6170706C")
                    && d.name.contains("Delay")
            })
            .expect("Mac must provide an Apple delay AUv2 fixture");
        let (descriptor, backend) = create(&descriptor.id).unwrap();
        let mut instance = crate::PluginInstance::new(descriptor, backend);
        let state = instance.save_state().unwrap();
        instance.load_state(&state).unwrap();
        let rate = 48_000.0;
        let mut processor = instance.activate(rate, 64).unwrap();
        // Stock Audio Unit bypass is native behavior, not a host stub.
        let native = processor
            .backend_mut()
            .as_any_mut()
            .downcast_mut::<Processor>()
            .unwrap();
        property(native.native.unit, 21, GLOBAL, &1_u32).unwrap();
        let mut left = [0.25; 64];
        let mut right = [-0.5; 64];
        processor.process(&mut left, &mut right);
        assert!(left.iter().all(|v| (*v - 0.25).abs() < 1e-6));
        assert!(right.iter().all(|v| (*v + 0.5).abs() < 1e-6));
        processor.reset();
        processor.process(&mut left, &mut right);
        assert!(processor.health().failed);
        assert!(left.iter().chain(&right).all(|v| *v == 0.0));
        instance.deactivate(processor).unwrap();
        assert!(!instance.is_active());
        assert!(!instance.save_state().unwrap().as_bytes().is_empty());
    }
}
