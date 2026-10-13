//! AUv2 adapter for installed macOS components. Native acceptance requires a Mac.
//! The OS registry identifies components by type/subtype/manufacturer, not a DLL.
mod cocoa;
mod sys;
use crate::au_model::{
    Bus, Catalog, CatalogEntry, ComponentId, ParameterAddress, ProviderPhase, StateContainer,
};
use crate::events::{PluginEvent, Transport};
use crate::instance::InstanceBackend;
use crate::processor::{
    AudioIo, AudioShape, BlockResult, ProcessFailed, ProcessStatus, ProcessorBackend,
    ProcessorShared,
};
use crate::{
    AudioPort, EditorError, EditorInfo, EditorOptions, HostEvent, PluginDescriptor, PluginError,
    PluginLayout, PluginNotification, PluginParam,
};
use std::{
    any::Any,
    ffi::c_void,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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
    installed_catalog()
        .entries
        .iter()
        .map(CatalogEntry::descriptor)
        .collect()
}
pub fn installed_catalog() -> Catalog {
    let mut result = Vec::new();
    for kind in [
        fourcc(b"aufx"),
        fourcc(b"aumu"),
        fourcc(b"aumf"),
        fourcc(b"augn"),
    ] {
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
            result.push(CatalogEntry {
                component: ComponentId {
                    kind: description.kind,
                    subtype: description.subtype,
                    manufacturer: description.manufacturer,
                },
                name: name_text,
                version,
            });
        }
    }
    let mut catalog = Catalog::default();
    // Duplicate registrations of an identical component resolve to the first
    // system registry entry; catalog replacement remains checked/atomic.
    result.sort_by_key(|entry| entry.component);
    result.dedup_by_key(|entry| entry.component);
    let _ = catalog.replace(result);
    catalog
}
struct Native {
    unit: Unit,
    reset_requested: AtomicBool,
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
    let parsed = ComponentId::parse(wanted).map_err(PluginError::Create)?;
    let description = Description {
        kind: parsed.kind,
        subtype: parsed.subtype,
        manufacturer: parsed.manufacturer,
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
    let native = Arc::new(Native {
        unit,
        reset_requested: AtomicBool::new(false),
    });
    let inputs = buses(unit, INPUT_SCOPE)?;
    let outputs = buses(unit, OUTPUT_SCOPE)?;
    let (params, addresses) = read_parameters(unit, &inputs, &outputs)?;
    crate::au_model::validate_buses(&inputs, &outputs).map_err(PluginError::Layout)?;
    let has_input = !inputs.is_empty();
    let layout = PluginLayout {
        audio_inputs: inputs
            .iter()
            .map(|bus| AudioPort {
                name: if bus.index == 0 {
                    "Main".into()
                } else {
                    format!("Input {}", bus.index + 1)
                },
                channels: bus.channels,
                main: bus.index == 0,
            })
            .collect(),
        audio_outputs: outputs
            .iter()
            .map(|bus| AudioPort {
                name: format!("Output {}", bus.index + 1),
                channels: bus.channels,
                main: bus.index == 0,
            })
            .collect(),
        note_inputs: u32::from(!has_input || description.kind == fourcc(b"aumf")),
        note_outputs: 0,
        has_editor: cocoa::available(unit),
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
            component: parsed,
            addresses,
            reset_notified: false,
            editor: None,
            phase: ProviderPhase::Inactive,
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
    component: ComponentId,
    addresses: Arc<[(u32, ParameterAddress)]>,
    reset_notified: bool,
    editor: Option<cocoa::Editor>,
    phase: ProviderPhase,
}
impl Drop for Instance {
    fn drop(&mut self) {
        self.editor = None;
    } // Cocoa view releases before AU disposal.
}
fn read_parameters(
    unit: Unit,
    inputs: &[Bus],
    outputs: &[Bus],
) -> Result<(Vec<PluginParam>, Arc<[(u32, ParameterAddress)]>), PluginError> {
    let mut addresses = Vec::new();
    for (scope, elements) in [
        (GLOBAL, 1),
        (INPUT_SCOPE, inputs.len()),
        (OUTPUT_SCOPE, outputs.len()),
    ] {
        for element in 0..elements {
            let (mut size, mut writable) = (0u32, 0u8);
            if unsafe {
                AudioUnitGetPropertyInfo(
                    unit,
                    PARAMETER_LIST,
                    scope,
                    element as u32,
                    &mut size,
                    &mut writable,
                )
            } != 0
            {
                continue;
            }
            if size % 4 != 0 || size / 4 > crate::MAX_PARAMETERS {
                return Err(PluginError::Layout("AU parameter bound".into()));
            }
            let mut ids = vec![0u32; size as usize / 4];
            if size > 0 {
                checked(
                    unsafe {
                        AudioUnitGetProperty(
                            unit,
                            PARAMETER_LIST,
                            scope,
                            element as u32,
                            ids.as_mut_ptr().cast(),
                            &mut size,
                        )
                    },
                    "scoped parameter list",
                )?;
                if size as usize != ids.len() * 4 {
                    return Err(PluginError::Layout(
                        "AU parameter list changed during discovery".into(),
                    ));
                }
            }
            addresses.extend(ids.into_iter().map(|id| ParameterAddress {
                id,
                scope,
                element: element as u32,
            }));
        }
    }
    let mapping = crate::au_model::parameter_map(&addresses).map_err(PluginError::Layout)?;
    let mut params = Vec::with_capacity(mapping.len());
    for &(published, address) in &mapping {
        let id = address.id;
        let mut info: ParameterInfo = unsafe { std::mem::zeroed() };
        let mut bytes = size_of::<ParameterInfo>() as u32;
        checked(
            unsafe {
                AudioUnitGetProperty(
                    unit,
                    PARAMETER_INFO,
                    address.scope,
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
            id: published,
            name,
            module: String::new(),
            min: f64::from(info.min),
            max: f64::from(info.max),
            default: f64::from(info.default.clamp(info.min, info.max)),
            stepped: info.unit == 1 || info.unit == 2, // Indexed / Boolean
            choice: info.unit == 1 || info.unit == 2,
            automatable: info.flags & (1 << 31) != 0,
            read_only: info.flags & (1 << 31) == 0,
            hidden: false,
            bypass: false,
            periodic: false,
        });
    }
    Ok((params, mapping.into()))
}
fn buses(unit: Unit, scope: u32) -> Result<Vec<Bus>, PluginError> {
    let mut count = 0u32;
    let mut bytes = size_of::<u32>() as u32;
    checked(
        unsafe {
            AudioUnitGetProperty(
                unit,
                11,
                scope,
                0,
                (&mut count as *mut u32).cast(),
                &mut bytes,
            )
        },
        "bus count",
    )?;
    if count > crate::MAX_PORTS {
        return Err(PluginError::Layout("AU bus bound".into()));
    }
    (0..count)
        .map(|index| {
            let mut format = StreamFormat::default();
            let mut size = size_of::<StreamFormat>() as u32;
            checked(
                unsafe {
                    AudioUnitGetProperty(
                        unit,
                        STREAM_FORMAT,
                        scope,
                        index,
                        (&mut format as *mut StreamFormat).cast(),
                        &mut size,
                    )
                },
                "bus format",
            )?;
            Ok(Bus {
                index,
                channels: format.channels,
            })
        })
        .collect()
}
fn property_at<T>(
    unit: Unit,
    property: u32,
    scope: u32,
    element: u32,
    value: &T,
) -> Result<(), PluginError> {
    checked(
        unsafe {
            AudioUnitSetProperty(
                unit,
                property,
                scope,
                element,
                (value as *const T).cast(),
                size_of::<T>() as u32,
            )
        },
        "set bus property",
    )
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
        let address = self.addresses.iter().find(|p| p.0 == id)?.1;
        let mut value = 0.0;
        (unsafe {
            AudioUnitGetParameter(
                self.native.unit,
                address.id,
                address.scope,
                address.element,
                &mut value,
            )
        } == 0
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
                    let Some((_, address)) = self.addresses.iter().find(|p| p.0 == id) else {
                        continue;
                    };
                    unsafe {
                        AudioUnitSetParameter(
                            self.native.unit,
                            address.id,
                            address.scope,
                            address.element,
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
        let result = if length < 0 || length as usize > limit.saturating_sub(20) {
            Err(PluginError::State("saved within size bound"))
        } else {
            let container = StateContainer {
                component: self.component,
                property_list: unsafe {
                    std::slice::from_raw_parts(CFDataGetBytePtr(data), length as usize)
                }
                .to_vec(),
            };
            container
                .encode()
                .map(Some)
                .map_err(|_| PluginError::State("saved within size bound"))
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
        let container;
        let bytes = if bytes.starts_with(b"WAU1") {
            container = StateContainer::decode(bytes, self.component)
                .map_err(|_| PluginError::State("restored matching AU identity"))?;
            container.property_list.as_slice()
        } else {
            bytes
        }; // legacy round-three raw property lists
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
        if !matches!(
            self.phase,
            ProviderPhase::Inactive | ProviderPhase::Returned
        ) {
            return Err(PluginError::Activate(
                "AU owner has not proved quiescence".into(),
            ));
        }
        if self.failed_input.is_some() {
            checked(
                unsafe { AudioUnitUninitialize(self.native.unit) },
                "failed activation quiesce",
            )?;
            remove_callbacks(self.native.unit, self.layout.audio_inputs.len())?;
            drop(self.failed_input.take());
        }
        let mut format = StreamFormat {
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
        let mut input = Box::new(Input {
            buses: self
                .layout
                .audio_inputs
                .iter()
                .map(|bus| OwnedBus::new(bus.channels, block))
                .collect(),
            clock: Transport::default(),
            changed: false,
            previous_frames: 0,
            rate,
        });
        let outputs = self
            .layout
            .audio_outputs
            .iter()
            .map(|bus| OwnedBus::new(bus.channels, block))
            .collect::<Vec<_>>();
        let install = (|| {
            for (index, bus) in self.layout.audio_outputs.iter().enumerate() {
                format.channels = bus.channels;
                property_at(
                    self.native.unit,
                    STREAM_FORMAT,
                    OUTPUT_SCOPE,
                    index as u32,
                    &format,
                )?;
            }
            for (index, bus) in self.layout.audio_inputs.iter().enumerate() {
                format.channels = bus.channels;
                property_at(
                    self.native.unit,
                    STREAM_FORMAT,
                    INPUT_SCOPE,
                    index as u32,
                    &format,
                )?;
                property_at(
                    self.native.unit,
                    RENDER_CALLBACK,
                    INPUT_SCOPE,
                    index as u32,
                    &Callback {
                        render: Some(render_input),
                        context: (&mut *input as *mut Input).cast(),
                    },
                )?;
            }
            // Some simple effects do not implement host callbacks. Installation
            // refusal is harmless only when the property itself is absent.
            let (mut bytes, mut writable) = (0, 0);
            if unsafe {
                AudioUnitGetPropertyInfo(self.native.unit, 27, GLOBAL, 0, &mut bytes, &mut writable)
            } == 0
            {
                property(
                    self.native.unit,
                    27,
                    GLOBAL,
                    &HostCallbacks {
                        context: (&mut *input as *mut Input).cast(),
                        beat_tempo: Some(beat_tempo),
                        musical_location: Some(musical_location),
                        transport: Some(transport_state),
                    },
                )?;
            }
            Ok::<(), PluginError>(())
        })();
        if let Err(error) = install {
            self.failed_input = Some(input);
            return Err(error);
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
        self.phase.activate().map_err(PluginError::Activate)?;
        self.native.reset_requested.store(false, Ordering::Release);
        self.reset_notified = false;
        Ok(Box::new(Processor {
            native: Arc::clone(&self.native),
            input,
            outputs,
            max_block: block,
            tail: tail.ceil() as u32,
            sidechain: None,
            addresses: Arc::clone(&self.addresses),
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
        // AU Reset is owner-only AFTER audio return, while the unit is still
        // initialized. Uninitializing first makes many units refuse Reset.
        if self.native.reset_requested.load(Ordering::Acquire) {
            self.phase.request_reset();
            checked(
                unsafe { AudioUnitReset(self.native.unit, GLOBAL, 0) },
                "reset recovery",
            )?;
        }
        checked(
            unsafe { AudioUnitUninitialize(self.native.unit) },
            "uninitialize",
        )?;
        remove_callbacks(self.native.unit, self.layout.audio_inputs.len())?;
        self.phase.returned().map_err(PluginError::Deactivate)?;
        self.active = false;
        Ok(())
    }
    fn finish_deactivation(&mut self, processor: Option<Box<dyn ProcessorBackend>>) {
        if let Some(processor) = processor {
            drop(processor);
            self.active = false;
            self.phase.recover(true);
        } else if self.active {
            std::mem::forget(Arc::clone(&self.native));
        }
    }
    fn idle(
        &mut self,
        _: Instant,
        _: Option<&ProcessorShared>,
        notify: &mut dyn FnMut(PluginNotification),
    ) {
        if self.native.reset_requested.load(Ordering::Acquire) && !self.reset_notified {
            self.reset_notified = true;
            notify(PluginNotification::RestartRequested);
        }
        if let Some(editor) = &mut self.editor
            && let Some(info) = editor.poll()
        {
            notify(PluginNotification::EditorResized {
                width: info.width,
                height: info.height,
            });
        }
        if self.editor.as_ref().is_some_and(cocoa::Editor::closed) {
            self.editor = None;
            notify(PluginNotification::EditorClosed);
        }
    }
    fn next_deadline(&self) -> Option<Instant> {
        None
    }
    fn open_editor(&mut self, options: &EditorOptions) -> Result<EditorInfo, EditorError> {
        if self.editor.is_some() {
            return Err(EditorError::AlreadyOpen);
        }
        let editor = cocoa::Editor::open(self.native.unit, options)?;
        let info = editor.info();
        self.editor = Some(editor);
        Ok(info)
    }
    fn close_editor(&mut self) {
        self.editor = None;
    }
    fn editor(&self) -> Option<EditorInfo> {
        self.editor.as_ref().map(cocoa::Editor::info)
    }
    fn set_editor_scale(&mut self, application: f64) -> Result<EditorInfo, EditorError> {
        self.editor
            .as_mut()
            .ok_or(EditorError::NoEditor)?
            .set_scale(application)
    }
}
fn remove_callbacks(unit: Unit, inputs: usize) -> Result<(), PluginError> {
    for index in 0..inputs {
        property_at(
            unit,
            RENDER_CALLBACK,
            INPUT_SCOPE,
            index as u32,
            &Callback {
                render: None,
                context: ptr::null_mut(),
            },
        )?;
    }
    let (mut bytes, mut writable) = (0, 0);
    if unsafe { AudioUnitGetPropertyInfo(unit, 27, GLOBAL, 0, &mut bytes, &mut writable) } == 0 {
        property(
            unit,
            27,
            GLOBAL,
            &HostCallbacks {
                context: ptr::null_mut(),
                beat_tempo: None,
                musical_location: None,
                transport: None,
            },
        )?;
    }
    Ok(())
}
struct OwnedBus {
    channels: Vec<Box<[f32]>>,
    list: Box<OwnedBufferList>,
}
// SAFETY: pointers refer to this bus's stable boxed samples. Only the exclusive
// audio owner accesses/restores the list; destruction returns to the main owner.
unsafe impl Send for OwnedBus {}
impl OwnedBus {
    fn new(channels: u32, frames: u32) -> Self {
        let mut bus = Self {
            channels: (0..channels)
                .map(|_| vec![0.0; frames as usize].into_boxed_slice())
                .collect(),
            list: Box::new(OwnedBufferList {
                count: channels,
                buffers: [Buffer {
                    channels: 1,
                    bytes: 0,
                    data: ptr::null_mut(),
                }; 64],
            }),
        };
        bus.prepare(frames);
        bus
    }
    fn prepare(&mut self, frames: u32) {
        self.list.count = self.channels.len() as u32;
        for (buffer, samples) in self.list.buffers.iter_mut().zip(&mut self.channels) {
            *buffer = Buffer {
                channels: 1,
                bytes: frames * 4,
                data: samples.as_mut_ptr().cast(),
            };
        }
    }
}
struct Input {
    buses: Vec<OwnedBus>,
    clock: Transport,
    changed: bool,
    previous_frames: u32,
    rate: f64,
}
unsafe extern "C" fn render_input(
    context: *mut c_void,
    _: *mut u32,
    _: *const Timestamp,
    bus: u32,
    frames: u32,
    list: *mut BufferList,
) -> i32 {
    let Some(input) = (unsafe { context.cast::<Input>().as_mut() }) else {
        return -50;
    };
    let Some(bus) = input.buses.get_mut(bus as usize) else {
        return -50;
    };
    if list.is_null()
        || unsafe { (*list).count } as usize != bus.channels.len()
        || bus.channels.iter().any(|c| c.len() < frames as usize)
    {
        return -50;
    }
    let buffers = unsafe {
        std::slice::from_raw_parts_mut(
            ptr::addr_of_mut!((*list).buffers).cast::<Buffer>(),
            bus.channels.len(),
        )
    };
    for (buffer, samples) in buffers.iter_mut().zip(&mut bus.channels) {
        if buffer.data.is_null() {
            buffer.data = samples.as_mut_ptr().cast();
        } else {
            if buffer.bytes < frames * 4 {
                return -50;
            }
            // Some units return our exact source pointer; avoid overlapping copy.
            if buffer.data != samples.as_mut_ptr().cast() {
                unsafe {
                    ptr::copy_nonoverlapping(
                        samples.as_ptr(),
                        buffer.data.cast::<f32>(),
                        frames as usize,
                    );
                }
            }
        }
        buffer.channels = 1;
        buffer.bytes = frames * 4;
    }
    0
}
unsafe extern "C" fn beat_tempo(context: *mut c_void, beat: *mut f64, tempo: *mut f64) -> i32 {
    let Some(input) = (unsafe { context.cast::<Input>().as_ref() }) else {
        return -50;
    };
    if !beat.is_null() {
        unsafe {
            beat.write(input.clock.position_beats);
        }
    }
    if !tempo.is_null() {
        unsafe {
            tempo.write(input.clock.tempo_bpm);
        }
    }
    0
}
unsafe extern "C" fn musical_location(
    context: *mut c_void,
    delta: *mut u32,
    numerator: *mut f32,
    denominator: *mut u32,
    downbeat: *mut f64,
) -> i32 {
    let Some(input) = (unsafe { context.cast::<Input>().as_ref() }) else {
        return -50;
    };
    let Some(clock) = crate::au_model::host_clock(&input.clock, input.rate) else {
        return -50;
    };
    if !delta.is_null() {
        unsafe {
            delta.write(clock.next_beat_frames);
        }
    }
    if !numerator.is_null() {
        unsafe {
            numerator.write(clock.numerator);
        }
    }
    if !denominator.is_null() {
        unsafe {
            denominator.write(clock.denominator);
        }
    }
    if !downbeat.is_null() {
        unsafe {
            downbeat.write(clock.downbeat);
        }
    }
    0
}
unsafe extern "C" fn transport_state(
    context: *mut c_void,
    playing: *mut u8,
    changed: *mut u8,
    sample: *mut f64,
    cycling: *mut u8,
    start: *mut f64,
    end: *mut f64,
) -> i32 {
    let Some(input) = (unsafe { context.cast::<Input>().as_ref() }) else {
        return -50;
    };
    unsafe {
        if !playing.is_null() {
            playing.write(u8::from(input.clock.playing));
        }
        if !changed.is_null() {
            changed.write(u8::from(input.changed));
        }
        if !sample.is_null() {
            sample.write(input.clock.position_seconds * input.rate);
        }
        if !cycling.is_null() {
            cycling.write(0);
        }
        if !start.is_null() {
            start.write(0.0);
        }
        if !end.is_null() {
            end.write(0.0);
        }
    }
    0
}
struct Processor {
    native: Arc<Native>,
    input: Box<Input>,
    outputs: Vec<OwnedBus>,
    max_block: u32,
    tail: u32,
    sidechain: Option<u32>,
    addresses: Arc<[(u32, ParameterAddress)]>,
}
impl Processor {
    fn render(
        &mut self,
        audio: AudioIo<'_>,
        key: Option<&[[f32; 2]]>,
        events: &[HostEvent],
        transport: &Transport,
        steady_time: u64,
    ) -> Result<BlockResult, ProcessFailed> {
        let (il, ir, ol, or) = match audio {
            AudioIo::Separate {
                in_left,
                in_right,
                out_left,
                out_right,
            } => (in_left, in_right, out_left, out_right),
            AudioIo::InPlace { .. } => return Err(ProcessFailed),
        };
        let frames = ol.len();
        if frames > self.max_block as usize
            || ir.len() != frames
            || il.len() != frames
            || or.len() != frames
        {
            return Err(ProcessFailed);
        }
        ol.fill(0.0);
        or.fill(0.0);
        if self.native.reset_requested.load(Ordering::Acquire) {
            // Silence while owner recovery is queued, without marking the native
            // processor permanently failed or calling non-RT-safe AudioUnitReset.
            return Ok(BlockResult {
                status: ProcessStatus::Continue,
                tail: Some(0),
                dropped_events: events.len() as u32,
            });
        }
        self.input.changed = crate::au_model::transport_changed(
            &self.input.clock,
            transport,
            self.input.previous_frames,
            self.input.rate,
        );
        self.input.clock = *transport;
        self.input.previous_frames = frames as u32;
        for bus in &mut self.input.buses {
            for channel in &mut bus.channels {
                channel.fill(0.0);
            }
        }
        if let Some(main) = self.input.buses.first_mut() {
            if !crate::au_model::copy_stereo(&mut main.channels, il, ir) {
                return Err(ProcessFailed);
            }
        }
        if let (Some(index), Some(key)) = (self.sidechain, key) {
            if key.len() != frames {
                return Err(ProcessFailed);
            }
            let Some(bus) = self.input.buses.get_mut(index as usize) else {
                return Err(ProcessFailed);
            };
            for (frame, pair) in key.iter().enumerate() {
                if bus.channels.len() == 1 {
                    bus.channels[0][frame] = (pair[0] + pair[1]) * 0.5;
                } else {
                    bus.channels[0][frame] = pair[0];
                    bus.channels[1][frame] = pair[1];
                }
            }
        }
        let mut drops = 0;
        for event in events {
            let status = match *event {
                HostEvent::Param { time, id, value } => {
                    let Some((_, address)) = self.addresses.iter().find(|p| p.0 == id) else {
                        drops += 1;
                        continue;
                    };
                    unsafe {
                        AudioUnitSetParameter(
                            self.native.unit,
                            address.id,
                            address.scope,
                            address.element,
                            value as f32,
                            time,
                        )
                    }
                }
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
        let timestamp = Timestamp {
            sample: steady_time as f64,
            rate: 1.0,
            flags: 1,
            ..Timestamp::default()
        };
        for (index, bus) in self.outputs.iter_mut().enumerate() {
            bus.prepare(frames as u32);
            let mut flags = 0;
            if unsafe {
                AudioUnitRender(
                    self.native.unit,
                    &mut flags,
                    &timestamp,
                    index as u32,
                    frames as u32,
                    (&mut *bus.list as *mut OwnedBufferList).cast(),
                )
            } != 0
            {
                return Err(ProcessFailed);
            }
            if bus.list.count as usize != bus.channels.len() {
                return Err(ProcessFailed);
            }
            // Audio Units may return their own buffers; copy synchronously before
            // rendering another bus, whose storage must remain independently owned.
            for (buffer, samples) in bus.list.buffers.iter().zip(&mut bus.channels) {
                if buffer.channels != 1 || buffer.bytes < frames as u32 * 4 || buffer.data.is_null()
                {
                    return Err(ProcessFailed);
                }
                if buffer.data != samples.as_mut_ptr().cast() {
                    unsafe {
                        ptr::copy_nonoverlapping(
                            buffer.data.cast::<f32>(),
                            samples.as_mut_ptr(),
                            frames,
                        );
                    }
                }
            }
        }
        let Some(main) = self.outputs.first() else {
            return Err(ProcessFailed);
        };
        ol.copy_from_slice(&main.channels[0][..frames]);
        or.copy_from_slice(&main.channels[if main.channels.len() == 1 { 0 } else { 1 }][..frames]);
        Ok(BlockResult {
            status: ProcessStatus::Continue,
            tail: Some(self.tail),
            dropped_events: drops,
        })
    }
}
impl ProcessorBackend for Processor {
    fn set_sidechain_input(&mut self, input: Option<u32>) {
        self.sidechain = input;
    }
    fn process(
        &mut self,
        audio: AudioIo<'_>,
        events: &[HostEvent],
        transport: &Transport,
        steady_time: u64,
        _: &mut dyn FnMut(PluginEvent),
    ) -> Result<BlockResult, ProcessFailed> {
        self.render(audio, None, events, transport, steady_time)
    }
    fn process_sidechain(
        &mut self,
        audio: AudioIo<'_>,
        key: Option<&[[f32; 2]]>,
        events: &[HostEvent],
        transport: &Transport,
        steady_time: u64,
        _: &mut dyn FnMut(PluginEvent),
    ) -> Result<BlockResult, ProcessFailed> {
        self.render(audio, key, events, transport, steady_time)
    }
    fn reset(&mut self) {
        self.native.reset_requested.store(true, Ordering::Release);
    }
    fn stop(&mut self) {
        self.reset();
    }
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
    use crate::PluginKind;
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
        assert!(
            !processor.health().failed,
            "reset queues owner recovery without poisoning audio health"
        );
        assert!(left.iter().chain(&right).all(|v| *v == 0.0));
        instance.deactivate(processor).unwrap();
        assert!(!instance.is_active());
        assert!(!instance.save_state().unwrap().as_bytes().is_empty());
        let mut processor = instance.activate(rate, 64).unwrap();
        left.fill(0.25);
        right.fill(-0.5);
        processor.process(&mut left, &mut right);
        assert!(left.iter().all(|v| (*v - 0.25).abs() < 1e-6));
        assert!(right.iter().all(|v| (*v + 0.5).abs() < 1e-6));
        assert!(!processor.health().failed);
        instance.deactivate(processor).unwrap();
    }
}
