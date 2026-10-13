//! Real VST3 combined component/controller/processor. SDK IDs are permanent.
#![allow(non_snake_case)]
use crate::publication::Publication;
use crate::{MAX_HOST_FRAMES, MAX_STATE_BYTES, OUTPUT_CHANNELS, OUTPUT_PORTS, Program};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::{
    cell::UnsafeCell,
    ffi::{CStr, c_void},
    ptr,
    rc::Rc,
};
use vst3::Steinberg::Vst::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper, Interface, Steinberg::*};

pub const CLASS_ID: &str = "57494E4446414C4C50524F4752414D01";
fn cid() -> TUID {
    let mut id = (*b"WINDFALLPROGRAM\x01").map(|v| v as char8);
    // SDK FUID canonical text uses COM GUID byte order on Windows.
    if cfg!(windows) {
        id[..4].reverse();
        id[4..6].reverse();
        id[6..8].reverse();
    }
    id
}
fn chars<const N: usize>(value: &str) -> [char8; N] {
    let mut text = [0; N];
    for (s, b) in text.iter_mut().take(N - 1).zip(value.bytes()) {
        *s = b as char8;
    }
    text
}
struct Factory;
impl Class for Factory {
    type Interfaces = (IPluginFactory2,);
}
impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        unsafe {
            info.write(PFactoryInfo {
                vendor: chars("Windfall"),
                url: chars("https://github.com/erivgout/windfall"),
                email: [0; 128],
                flags: 0,
            });
        }
        kResultOk
    }
    unsafe fn countClasses(&self) -> i32 {
        1
    }
    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        if index != 0 || info.is_null() {
            return kInvalidArgument;
        }
        unsafe {
            info.write(PClassInfo {
                cid: cid(),
                cardinality: i32::MAX,
                category: chars("Audio Module Class"),
                name: chars("Windfall"),
            });
        }
        kResultOk
    }
    unsafe fn createInstance(
        &self,
        class: FIDString,
        iid: FIDString,
        obj: *mut *mut c_void,
    ) -> tresult {
        if obj.is_null() || class.is_null() || iid.is_null() {
            return kInvalidArgument;
        }
        unsafe {
            obj.write(ptr::null_mut());
        }
        if unsafe { ptr::read_unaligned(class.cast::<TUID>()) } != cid() {
            return kNoInterface;
        }
        let requested = unsafe { ptr::read_unaligned(iid.cast::<[u8; 16]>()) };
        crate::runtime::error("");
        let program = match crate::package::initial_program() {
            Ok(program) => program,
            Err(error) => { crate::runtime::error(error); return kResultFalse; }
        };
        let component = ComWrapper::new(Component {
            state: Rc::new(Publication::new(program)),
            initialized: AtomicBool::new(false),
            active: AtomicBool::new(false),
            processing: AtomicBool::new(false),
            setup: UnsafeCell::new((48_000.0, 0)),
            latency: AtomicU32::new(0),
            handler: Rc::new(UnsafeCell::new(None)),
        });
        if requested == IComponent::IID {
            unsafe {
                obj.write(
                    component
                        .to_com_ptr::<IComponent>()
                        .unwrap()
                        .into_raw()
                        .cast(),
                );
            }
        } else if requested == IAudioProcessor::IID {
            unsafe {
                obj.write(
                    component
                        .to_com_ptr::<IAudioProcessor>()
                        .unwrap()
                        .into_raw()
                        .cast(),
                );
            }
        } else if requested == IEditController::IID {
            unsafe {
                obj.write(
                    component
                        .to_com_ptr::<IEditController>()
                        .unwrap()
                        .into_raw()
                        .cast(),
                );
            }
        } else {
            return kNoInterface;
        }
        kResultOk
    }
}
impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        if index != 0 || info.is_null() {
            return kInvalidArgument;
        }
        unsafe {
            info.write(PClassInfo2 {
                cid: cid(),
                cardinality: i32::MAX,
                category: chars("Audio Module Class"),
                name: chars("Windfall"),
                classFlags: 0,
                subCategories: chars("Instrument|Synth"),
                vendor: chars("Windfall"),
                version: chars(env!("CARGO_PKG_VERSION")),
                sdkVersion: chars("VST 3"),
            });
        }
        kResultOk
    }
}
type HandlerSlot = Rc<UnsafeCell<Option<ComPtr<IComponentHandler>>>>;
struct Component {
    state: Rc<Publication>,
    initialized: AtomicBool,
    active: AtomicBool,
    processing: AtomicBool,
    setup: UnsafeCell<(f64, u32)>,
    latency: AtomicU32,
    handler: HandlerSlot,
}
impl Class for Component {
    type Interfaces = (IComponent, IAudioProcessor, IEditController);
}
impl IPluginBaseTrait for Component {
    unsafe fn initialize(&self, context: *mut FUnknown) -> tresult {
        if context.is_null() || self.initialized.swap(true, Ordering::AcqRel) {
            kResultFalse
        } else {
            kResultOk
        }
    }
    unsafe fn terminate(&self) -> tresult {
        if self.active.load(Ordering::Acquire) {
            return kResultFalse;
        }
        unsafe {
            *self.handler.get() = None;
        }
        self.initialized.store(false, Ordering::Release);
        kResultOk
    }
}
impl IComponentTrait for Component {
    unsafe fn getControllerClassId(&self, _: *mut TUID) -> tresult {
        kNotImplemented
    }
    unsafe fn setIoMode(&self, _: IoMode) -> tresult {
        kResultOk
    }
    unsafe fn getBusCount(&self, media: MediaType, direction: BusDirection) -> i32 {
        match (media, direction) {
            (0, 1) => OUTPUT_PORTS as i32,
            (1, 0) => 1,
            _ => 0,
        }
    }
    unsafe fn getBusInfo(
        &self,
        media: MediaType,
        direction: BusDirection,
        index: i32,
        info: *mut BusInfo,
    ) -> tresult {
        if info.is_null() || index < 0 || index >= unsafe { self.getBusCount(media, direction) } {
            return kInvalidArgument;
        }
        let mut name = [0; 128];
        let label = if media == 1 {
            "Rack MIDI".into()
        } else {
            format!("Output {}", index + 1)
        };
        for (s, c) in name.iter_mut().zip(label.encode_utf16()) {
            *s = c;
        }
        unsafe {
            info.write(BusInfo {
                mediaType: media,
                direction,
                channelCount: if media == 1 { 16 } else { 2 },
                name,
                busType: if index == 0 { 0 } else { 1 },
                flags: 1,
            });
        }
        kResultOk
    }
    unsafe fn getRoutingInfo(&self, _: *mut RoutingInfo, _: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }
    unsafe fn activateBus(
        &self,
        media: MediaType,
        direction: BusDirection,
        index: i32,
        _: TBool,
    ) -> tresult {
        if index >= 0 && index < unsafe { self.getBusCount(media, direction) } {
            kResultOk
        } else {
            kInvalidArgument
        }
    }
    unsafe fn setActive(&self, active: TBool) -> tresult {
        if self.processing.load(Ordering::Acquire) {
            return kResultFalse;
        }
        if active == 0 {
            self.state.deactivate();
            self.active.store(false, Ordering::Release);
            return kResultOk;
        }
        if self.active.load(Ordering::Acquire) || !self.initialized.load(Ordering::Acquire) {
            return kResultFalse;
        }
        let (rate, frames) = unsafe { *self.setup.get() };
        if frames == 0 {
            return kResultFalse;
        }
        let prepared =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.state.activate(rate)));
        let latency = match prepared {
            Ok(Ok(latency)) => latency,
            Ok(Err(error)) => { crate::runtime::error(error); return kResultFalse; }
            Err(_) => { crate::runtime::error("Windfall program preparation panicked"); return kResultFalse; }
        };
        self.latency.store(latency, Ordering::Release);
        self.active.store(true, Ordering::Release);
        kResultOk
    }
    unsafe fn setState(&self, stream: *mut IBStream) -> tresult {
        match unsafe { read_program(stream) }.and_then(|program| self.state.load(program)) {
            Ok(()) => kResultOk,
            Err(error) => { crate::runtime::error(error); kResultFalse },
        }
    }
    unsafe fn getState(&self, stream: *mut IBStream) -> tresult {
        unsafe { write_program(&self.state, stream) }
    }
}
unsafe fn read_program(stream: *mut IBStream) -> Result<Program, String> {
    let stream = unsafe { ComRef::from_raw(stream) }.ok_or("null stream")?;
    let mut length = [0u8; 4];
    unsafe {
        read_exact(&stream, &mut length)?;
    }
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_STATE_BYTES {
        return Err("state bound".into());
    }
    let mut bytes = vec![0; length];
    unsafe {
        read_exact(&stream, &mut bytes)?;
    }
    Program::decode(&bytes)
}
unsafe fn read_exact(stream: &ComRef<'_, IBStream>, bytes: &mut [u8]) -> Result<(), String> {
    let mut offset = 0;
    while offset < bytes.len() {
        let count = (bytes.len() - offset).min(8192) as i32;
        let mut actual = 0;
        let status =
            unsafe { stream.read(bytes[offset..].as_mut_ptr().cast(), count, &mut actual) };
        if status != kResultOk || actual <= 0 || actual > count {
            return Err("short state stream".into());
        }
        offset += actual as usize;
    }
    Ok(())
}
unsafe fn write_program(state: &Publication, stream: *mut IBStream) -> tresult {
    let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else {
        return kInvalidArgument;
    };
    let mut bytes = match state.capture() { Ok(bytes) => bytes,
        Err(error) => { crate::runtime::error(error); return kResultFalse; } };
    let mut length = (bytes.len() as u32).to_le_bytes();
    for data in [&mut length[..], &mut bytes[..]] {
        let mut offset = 0;
        while offset < data.len() {
            let count = (data.len() - offset).min(8192) as i32;
            let mut actual = 0;
            if unsafe { stream.write(data[offset..].as_mut_ptr().cast(), count, &mut actual) }
                != kResultOk
                || actual <= 0
                || actual > count
            {
                return kResultFalse;
            }
            offset += actual as usize;
        }
    }
    kResultOk
}
impl IAudioProcessorTrait for Component {
    unsafe fn setBusArrangements(
        &self,
        _: *mut SpeakerArrangement,
        inputs: i32,
        outputs: *mut SpeakerArrangement,
        count: i32,
    ) -> tresult {
        if inputs != 0 || count != OUTPUT_PORTS as i32 || outputs.is_null() {
            return kResultFalse;
        }
        if unsafe { std::slice::from_raw_parts(outputs, OUTPUT_PORTS) }
            .iter()
            .any(|a| *a != 3)
        {
            kResultFalse
        } else {
            kResultOk
        }
    }
    unsafe fn getBusArrangement(
        &self,
        direction: BusDirection,
        index: i32,
        arrangement: *mut SpeakerArrangement,
    ) -> tresult {
        if direction != 1 || !(0..OUTPUT_PORTS as i32).contains(&index) || arrangement.is_null() {
            return kInvalidArgument;
        }
        unsafe {
            arrangement.write(3);
        }
        kResultOk
    }
    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size == 0 { kResultOk } else { kResultFalse }
    }
    unsafe fn getLatencySamples(&self) -> u32 {
        self.state.requested_latency()
    }
    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        let Some(setup) = (unsafe { setup.as_ref() }) else {
            return kInvalidArgument;
        };
        if self.active.load(Ordering::Acquire)
            || setup.symbolicSampleSize != 0
            || setup.maxSamplesPerBlock <= 0
            || setup.maxSamplesPerBlock > MAX_HOST_FRAMES as i32
            || !setup.sampleRate.is_finite()
            || !(8_000.0..=384_000.0).contains(&setup.sampleRate)
            || setup.sampleRate.fract() != 0.0
        {
            return kInvalidArgument;
        }
        if let Err(error) = self.state.set_offline(setup.processMode == 2) {
            crate::runtime::error(error); return kResultFalse;
        }
        unsafe {
            *self.setup.get() = (setup.sampleRate, setup.maxSamplesPerBlock as u32);
        }
        kResultOk
    }
    unsafe fn setProcessing(&self, state: TBool) -> tresult {
        if !self.active.load(Ordering::Acquire) {
            return kResultFalse;
        }
        if state == 0
            && let Some(engine) = unsafe { self.state.audio() }
        {
            engine.reset();
        }
        self.processing.store(state != 0, Ordering::Release);
        kResultOk
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let Some(data) = (unsafe { data.as_mut() }) else {
            return kInvalidArgument;
        };
        if !self.processing.load(Ordering::Acquire)
            || data.symbolicSampleSize != 0
            || data.numSamples < 0
            || data.numSamples > unsafe { (*self.setup.get()).1 } as i32
            || data.numInputs != 0
            || data.numOutputs != OUTPUT_PORTS as i32
            || data.outputs.is_null()
        {
            return kInvalidArgument;
        }
        let outputs = unsafe { std::slice::from_raw_parts_mut(data.outputs, OUTPUT_PORTS) };
        for bus in outputs.iter() {
            let channels = unsafe { bus.__field0.channelBuffers32 };
            if bus.numChannels != 2
                || channels.is_null()
                || unsafe { (*channels).is_null() || (*channels.add(1)).is_null() }
            {
                return kInvalidArgument;
            }
        }
        for bus in outputs.iter_mut() {
            bus.silenceFlags = 0;
            for channel in 0..2 {
                unsafe {
                    std::slice::from_raw_parts_mut(
                        *bus.__field0.channelBuffers32.add(channel),
                        data.numSamples as usize,
                    )
                }
                .fill(0.0);
            }
        }
        let Some(engine) = (unsafe { self.state.audio() }) else {
            return kResultFalse;
        };
        let context = unsafe { data.processContext.as_ref() };
        // No context means stopped MIDI instrument mode; internal transport
        // cannot take control when the host has not supplied a clock.
        let (beats, tempo, playing) = context.map_or(
            (
                engine.position_ticks() / windfall_core::PPQ as f64,
                120.0,
                false,
            ),
            |c| (c.projectTimeMusic, c.tempo, c.state & 2 != 0),
        );
        if let Some(c) = context {
            if c.state & (1024 | 512) != (1024 | 512)
                || c.sampleRate != f64::from(engine.sample_rate())
                || c.timeSigNumerator < 0
                || c.timeSigDenominator < 0
                || c.timeSigNumerator > 32
                || c.timeSigDenominator > 32
            {
                return kInvalidArgument;
            }
            if c.state & 8192 != 0
                && !engine.sync_meter(c.timeSigNumerator as u16, c.timeSigDenominator as u16)
            {
                return kInvalidArgument;
            }
        }
        let events = unsafe { ComRef::from_raw(data.inputEvents) };
        let count = events.as_ref().map_or(0, |e| unsafe { e.getEventCount() });
        if !(0..=4096).contains(&count) {
            return kInvalidArgument;
        }
        let mut previous = 0;
        for index in 0..count {
            let mut event: Event = unsafe { std::mem::zeroed() };
            if unsafe { events.as_ref().unwrap().getEvent(index, &mut event) } != kResultOk
                || event.sampleOffset < previous
                || event.sampleOffset >= data.numSamples
                || event.busIndex != 0
            {
                return kInvalidArgument;
            }
            previous = event.sampleOffset;
            let (channel, pitch, velocity) = if event.r#type == 0 {
                let n = unsafe { event.__field0.noteOn };
                (n.channel, n.pitch, n.velocity)
            } else if event.r#type == 1 {
                let n = unsafe { event.__field0.noteOff };
                (n.channel, n.pitch, n.velocity)
            } else {
                continue;
            };
            if !(0..16).contains(&channel)
                || !(0..128).contains(&pitch)
                || !velocity.is_finite()
                || !(0.0..=1.0).contains(&velocity)
            {
                return kInvalidArgument;
            }
        }
        let mut frame = 0usize;
        let mut index = 0;
        while frame < data.numSamples as usize {
            if !engine.sync_transport(
                beats
                    + if playing {
                        frame as f64 * tempo / (60.0 * f64::from(engine.sample_rate()))
                    } else {
                        0.0
                    },
                tempo,
                playing,
            ) {
                return kInvalidArgument;
            }
            let mut next = data.numSamples as usize;
            while index < count {
                let mut event: Event = unsafe { std::mem::zeroed() };
                unsafe {
                    events.as_ref().unwrap().getEvent(index, &mut event);
                }
                if event.sampleOffset as usize > frame {
                    next = event.sampleOffset as usize;
                    break;
                }
                if event.r#type == 0 {
                    let n = unsafe { event.__field0.noteOn };
                    engine.midi(
                        n.channel as u8,
                        n.pitch as u8,
                        (n.velocity * 127.0).round().max(1.0) as u8,
                    );
                } else if event.r#type == 1 {
                    let n = unsafe { event.__field0.noteOff };
                    engine.midi(n.channel as u8, n.pitch as u8, 0);
                }
                index += 1;
            }
            let frames = (next - frame).min(windfall_engine::Processor::plugin_block_frames());
            let Some(pcm) = engine.render(frames) else {
                return kResultFalse;
            };
            for (port, bus) in outputs.iter_mut().enumerate() {
                for channel in 0..2 {
                    let output = unsafe {
                        std::slice::from_raw_parts_mut(
                            *bus.__field0.channelBuffers32.add(channel),
                            data.numSamples as usize,
                        )
                    };
                    for offset in 0..frames {
                        output[frame + offset] = pcm[offset * OUTPUT_CHANNELS + port * 2 + channel];
                    }
                }
            }
            frame += frames;
        }
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> u32 {
        u32::MAX
    }
}
impl IEditControllerTrait for Component {
    unsafe fn setComponentState(&self, stream: *mut IBStream) -> tresult {
        if unsafe { read_program(stream) }.is_ok() {
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn setState(&self, _: *mut IBStream) -> tresult {
        kResultOk
    } // combined component owns musical state
    unsafe fn getState(&self, _: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getParameterCount(&self) -> i32 {
        0
    }
    unsafe fn getParameterInfo(&self, _: i32, _: *mut ParameterInfo) -> tresult {
        kInvalidArgument
    }
    unsafe fn getParamStringByValue(&self, _: u32, _: f64, _: *mut String128) -> tresult {
        kInvalidArgument
    }
    unsafe fn getParamValueByString(&self, _: u32, _: *mut TChar, _: *mut f64) -> tresult {
        kInvalidArgument
    }
    unsafe fn normalizedParamToPlain(&self, _: u32, value: f64) -> f64 {
        value
    }
    unsafe fn plainParamToNormalized(&self, _: u32, value: f64) -> f64 {
        value
    }
    unsafe fn getParamNormalized(&self, _: u32) -> f64 {
        0.0
    }
    unsafe fn setParamNormalized(&self, _: u32, _: f64) -> tresult {
        kInvalidArgument
    }
    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        unsafe {
            *self.handler.get() = ComRef::from_raw(handler).map(|h| h.to_com_ptr());
        }
        let handler = Rc::clone(&self.handler);
        self.state.set_notifier(Some(Box::new(move |flags| {
            if let Some(handler) = unsafe { &*handler.get() } {
                if flags & crate::publication::DIRTY != 0
                    && let Some(dirty) = handler.cast::<IComponentHandler2>() {
                    unsafe { dirty.setDirty(1); }
                }
                let mut restart = 0;
                // VST3 SDK RestartFlags: parameter values=4, latency=8, reload=1.
                if flags & crate::publication::PARAMETERS != 0 { restart |= 4; }
                if flags & crate::publication::LATENCY != 0 { restart |= 8; }
                if flags & crate::publication::RESTART != 0 { restart |= 1; }
                if restart != 0 { unsafe { handler.restartComponent(restart); } }
            }
        })));
        kResultOk
    }
    unsafe fn createView(&self, name: FIDString) -> *mut IPlugView {
        if name.is_null() || unsafe { CStr::from_ptr(name) } != c"editor" {
            return ptr::null_mut();
        }
        ComWrapper::new(View {
            state: Rc::clone(&self.state),
            panel: UnsafeCell::new(None),
        })
        .to_com_ptr::<IPlugView>()
        .unwrap()
        .into_raw()
    }
}
struct View {
    state: Rc<Publication>,
    panel: UnsafeCell<Option<crate::panel::Panel>>,
}
impl Class for View {
    type Interfaces = (IPlugView,);
}
impl IPlugViewTrait for View {
    unsafe fn isPlatformTypeSupported(&self, platform: FIDString) -> tresult {
        if crate::panel::supports(platform) {
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn attached(&self, parent: *mut c_void, platform: FIDString) -> tresult {
        if !crate::panel::supports(platform) || unsafe { (&*self.panel.get()).is_some() } {
            return kResultFalse;
        }
        match crate::panel::Panel::open(parent, Rc::clone(&self.state)) {
            Ok(panel) => {
                unsafe {
                    *self.panel.get() = Some(panel);
                }
                kResultOk
            }
            Err(_) => kResultFalse,
        }
    }
    unsafe fn removed(&self) -> tresult {
        unsafe {
            *self.panel.get() = None;
        }
        kResultOk
    }
    unsafe fn onWheel(&self, _: f32) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyDown(&self, _: char16, _: int16, _: int16) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyUp(&self, _: char16, _: int16, _: int16) -> tresult {
        kResultFalse
    }
    unsafe fn getSize(&self, rect: *mut ViewRect) -> tresult {
        if rect.is_null() {
            return kInvalidArgument;
        }
        unsafe {
            rect.write(ViewRect {
                left: 0,
                top: 0,
                right: 560,
                bottom: 180,
            });
        }
        kResultOk
    }
    unsafe fn onSize(&self, _: *mut ViewRect) -> tresult {
        kResultOk
    }
    unsafe fn onFocus(&self, _: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setFrame(&self, _: *mut IPlugFrame) -> tresult {
        kResultOk
    }
    unsafe fn canResize(&self) -> tresult {
        kResultFalse
    }
    unsafe fn checkSizeConstraint(&self, rect: *mut ViewRect) -> tresult {
        unsafe { self.getSize(rect) }
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory)
        .to_com_ptr::<IPluginFactory>()
        .unwrap()
        .into_raw()
}
#[unsafe(no_mangle)]
pub extern "system" fn InitDll() -> bool {
    true
}
#[unsafe(no_mangle)]
pub extern "system" fn ExitDll() -> bool {
    true
}
#[unsafe(no_mangle)]
pub extern "system" fn ModuleEntry(_: *mut c_void) -> bool {
    true
}
#[unsafe(no_mangle)]
pub extern "system" fn ModuleExit() -> bool {
    true
}
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub extern "system" fn bundleEntry(_: *mut c_void) -> bool {
    true
}
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub extern "system" fn bundleExit() -> bool {
    true
}
