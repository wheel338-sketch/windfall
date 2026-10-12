//! Independent VST3 factory and component fixtures for the scanner.
#![allow(non_snake_case)]
mod native_thread;
#[path = "vst3_expression.rs"]
mod expression;
#[cfg(windows)]
#[path = "vst3_editor.rs"]
mod editor;
use native_thread::CreatorThread;
use std::ffi::c_void;
use std::ptr;
use vst3::Steinberg::Vst::*;
use vst3::{Class, ComPtr, ComWrapper, Interface, Steinberg::*};

fn chars<const N: usize>(text: &str) -> [char8; N] {
    let mut result = [0; N];
    for (slot, byte) in result.iter_mut().take(N - 1).zip(text.bytes()) {
        *slot = byte as char8;
    }
    result
}
fn cid(index: i32) -> TUID {
    let mut id = [0; 16];
    id[15] = (index + 1) as char8;
    id
}
/// How many classes the factory offers. New ones are appended, so the
/// positions and ids of the earlier ones do not move.
const CLASSES: i32 = 25;
/// The auxiliary input buses of the multi-detector class, after its main
/// stereo bus: their names and speaker arrangements (left; left and right;
/// left, right and both surrounds).
const MULTI_KEYS: [(&str, SpeakerArrangement); 3] =
    [("Key Mono", 1), ("Key Stereo", 3), ("Key Quad", 0x33)];
struct Factory;
impl Class for Factory {
    type Interfaces = (IPluginFactory2,);
}
impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: caller supplies a writable SDK output struct.
        unsafe {
            info.write(PFactoryInfo {
                vendor: chars("Windfall Tests"),
                url: [0; 256],
                email: [0; 128],
                flags: 0,
            });
        }
        kResultOk
    }
    unsafe fn countClasses(&self) -> i32 {
        CLASSES
    }
    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        if info.is_null() || !(0..CLASSES).contains(&index) {
            return kInvalidArgument;
        }
        // SAFETY: caller supplies a writable SDK output struct.
        unsafe {
            info.write(PClassInfo {
                cid: cid(index),
                cardinality: i32::MAX,
                category: chars("Audio Module Class"),
                name: chars(
                    [
                        "VST3 Test Effect",
                        "VST3 Absurd Ports",
                        "VST3 Test Instrument",
                        "VST3 Mono",
                        "VST3 Process Error",
                        "VST3 NaN",
                        "VST3 Deactivation Refusal",
                        "VST3 Native Dirty",
                        "VST3 Recovery Failure",
                        "VST3 Parameter Sources",
                        "VST3 Bridge Delayed Effect",
                        "VST3 Bridge Permanent Process Hang",
                        "VST3 Bridge Held Key Probe",
                        "VST3 Bridge Capture Exit",
                        "VST3 Bridge Capture Hang",
                        "VST3 Bridge Partial State Failure",
                        "VST3 Bridge Unsupported Latency",
                        "VST3 Bridge Native Event Flood",
                        "VST3 Bridge Successful Native State Limit",
                        "VST3 Bridge Controlled Process Error",
                        "VST3 Bridge Deactivation Edit",
                        "VST3 Auxiliary Detector",
                        "VST3 Multi Auxiliary Detector",
                        "VST3 Note Expression Probe",
                        "VST3 MIDI Mapping Instrument",
                    ][index as usize],
                ),
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
        if class.is_null() || iid.is_null() || obj.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: SDK passes sixteen-byte ids and a writable output pointer.
        unsafe {
            obj.write(ptr::null_mut());
            let class = ptr::read_unaligned(class.cast::<TUID>());
            let iid = ptr::read_unaligned(iid.cast::<[u8; 16]>());
            if iid != IComponent::IID || !(0..CLASSES).any(|i| class == cid(i)) {
                return kNoInterface;
            }
            let component = ComWrapper::new(Component {
                editor: class == cid(0),
                absurd: class == cid(1),
                instrument: class == cid(2) || class == cid(12) || class == cid(23) || class == cid(24),
                expression_probe: class == cid(23),
                midi_mapping: class == cid(24),
                midi_bend: std::array::from_fn(|_| std::sync::atomic::AtomicU64::new(0.5_f64.to_bits())),
                sidechain: class == cid(21),
                multi_sidechain: class == cid(22),
                bridge_note_probe: class == cid(12),
                mono: class == cid(3),
                refuse: class == cid(6),
                notify: class == cid(7),
                recovery_failure: class == cid(8),
                multi_params: class == cid(9),
                bridge_delayed: class == cid(10)
                    || class == cid(11)
                    || (13..21).any(|i| class == cid(i)),
                bridge_bad_latency: class == cid(16),
                bridge_state_boundary: class == cid(18),
                bridge_controlled_error: class == cid(19),
                bridge_deactivation_edit: class == cid(20),
                bridge_event_flood: class == cid(17),
                bridge_capture_fault: if class == cid(13) {
                    crate::bridge_behaviors::CaptureFault::Exit
                } else if class == cid(14) {
                    crate::bridge_behaviors::CaptureFault::Hang
                } else if class == cid(15) {
                    crate::bridge_behaviors::CaptureFault::PartialStream
                } else {
                    crate::bridge_behaviors::CaptureFault::None
                },
                bridge_hang: class == cid(11),
                activations: std::sync::atomic::AtomicU32::new(0),
                creator: CreatorThread::current(),
                notified: std::sync::atomic::AtomicBool::new(false),
                handler: std::cell::UnsafeCell::new(None),
                fault: if class == cid(4) {
                    1
                } else if class == cid(5) {
                    2
                } else {
                    0
                },
                initialized: std::sync::atomic::AtomicBool::new(false),
                active: std::sync::atomic::AtomicBool::new(false),
                processing: std::sync::atomic::AtomicBool::new(false),
                gain: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0.5_f64.to_bits())),
                extra: std::array::from_fn(|_| {
                    std::sync::atomic::AtomicU64::new(0.5_f64.to_bits())
                }),
                meters: std::array::from_fn(|_| std::sync::atomic::AtomicU64::new(0)),
                audio: std::cell::UnsafeCell::new(Audio::default()),
            })
            .to_com_ptr::<IComponent>()
            .unwrap();
            obj.write(component.into_raw().cast());
        }
        kResultOk
    }
}
impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        if info.is_null() || !(0..CLASSES).contains(&index) {
            return kInvalidArgument;
        }
        // SAFETY: caller supplies a writable SDK output struct.
        unsafe {
            info.write(PClassInfo2 {
                cid: cid(index),
                cardinality: i32::MAX,
                category: chars("Audio Module Class"),
                name: chars(
                    [
                        "VST3 Test Effect",
                        "VST3 Absurd Ports",
                        "VST3 Test Instrument",
                        "VST3 Mono",
                        "VST3 Process Error",
                        "VST3 NaN",
                        "VST3 Deactivation Refusal",
                        "VST3 Native Dirty",
                        "VST3 Recovery Failure",
                        "VST3 Parameter Sources",
                        "VST3 Bridge Delayed Effect",
                        "VST3 Bridge Permanent Process Hang",
                        "VST3 Bridge Held Key Probe",
                        "VST3 Bridge Capture Exit",
                        "VST3 Bridge Capture Hang",
                        "VST3 Bridge Partial State Failure",
                        "VST3 Bridge Unsupported Latency",
                        "VST3 Bridge Native Event Flood",
                        "VST3 Bridge Successful Native State Limit",
                        "VST3 Bridge Controlled Process Error",
                        "VST3 Bridge Deactivation Edit",
                        "VST3 Auxiliary Detector",
                        "VST3 Multi Auxiliary Detector",
                        "VST3 Note Expression Probe",
                        "VST3 MIDI Mapping Instrument",
                    ][index as usize],
                ),
                classFlags: 0,
                subCategories: chars(if index == 2 || index == 12 || index == 23 || index == 24 {
                    "Instrument|Synth"
                } else {
                    "Fx|Tools"
                }),
                vendor: chars("Windfall Tests"),
                version: chars("1.0"),
                sdkVersion: chars("3.8"),
            });
        }
        kResultOk
    }
}
struct Component {
    editor: bool,
    sidechain: bool,
    /// Three auxiliary inputs of one, two and four channels. The output
    /// carries each of their channels with the marks of the CLAP fixture.
    multi_sidechain: bool,
    absurd: bool,
    instrument: bool,
    expression_probe: bool,
    midi_mapping: bool,
    midi_bend: [std::sync::atomic::AtomicU64; 16],
    bridge_note_probe: bool,
    mono: bool,
    refuse: bool,
    notify: bool,
    recovery_failure: bool,
    multi_params: bool,
    bridge_delayed: bool,
    bridge_hang: bool,
    bridge_bad_latency: bool,
    bridge_state_boundary: bool,
    bridge_controlled_error: bool,
    bridge_deactivation_edit: bool,
    bridge_event_flood: bool,
    bridge_capture_fault: crate::bridge_behaviors::CaptureFault,
    activations: std::sync::atomic::AtomicU32,
    creator: CreatorThread,
    notified: std::sync::atomic::AtomicBool,
    handler: std::cell::UnsafeCell<Option<ComPtr<IComponentHandler>>>,
    fault: u8,
    initialized: std::sync::atomic::AtomicBool,
    active: std::sync::atomic::AtomicBool,
    processing: std::sync::atomic::AtomicBool,
    gain: std::sync::Arc<std::sync::atomic::AtomicU64>,
    extra: [std::sync::atomic::AtomicU64; 2],
    meters: [std::sync::atomic::AtomicU64; 3],
    audio: std::cell::UnsafeCell<Audio>,
}
impl Class for Component {
    type Interfaces = (IComponent, IAudioProcessor, IEditController, INoteExpressionController, IMidiMapping);
}
impl IPluginBaseTrait for Component {
    unsafe fn initialize(&self, context: *mut FUnknown) -> tresult {
        // SAFETY: borrowed SDK host context, used only inside this call.
        let Some(context) = (unsafe { vst3::ComRef::from_raw(context) }) else {
            return kInvalidArgument;
        };
        let Some(host) = context.cast::<IHostApplication>() else {
            return kNoInterface;
        };
        let mut name = [0; 128];
        // SAFETY: live host interface and sized name output.
        if unsafe { host.getName(&mut name) } != kResultOk || name[0] != u16::from(b'W') {
            return kInvalidArgument;
        }
        if self
            .initialized
            .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            return kResultFalse;
        }
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        assert_eq!(
            self.creator,
            CreatorThread::current(),
            "native destruction left the creating owner"
        );
        kResultOk
    }
}
impl IComponentTrait for Component {
    unsafe fn getControllerClassId(&self, _id: *mut TUID) -> tresult {
        kNotImplemented
    }
    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }
    unsafe fn getBusCount(&self, media: MediaType, dir: BusDirection) -> i32 {
        if self.absurd {
            1000
        } else if media == 0 {
            if self.instrument && dir == 0 {
                0
            } else if self.sidechain && dir == 0 {
                2
            } else if self.multi_sidechain && dir == 0 {
                1 + MULTI_KEYS.len() as i32
            } else {
                1
            }
        } else {
            i32::from(self.instrument && dir == 0)
        }
    }
    unsafe fn getBusInfo(
        &self,
        media: MediaType,
        dir: BusDirection,
        index: i32,
        bus: *mut BusInfo,
    ) -> tresult {
        if index < 0 || index >= unsafe { self.getBusCount(media, dir) } || bus.is_null() {
            return kInvalidArgument;
        }
        let multi_key = (self.multi_sidechain && media == 0 && dir == 0 && index >= 1)
            .then(|| MULTI_KEYS[index as usize - 1]);
        let mut name = [0; 128];
        for (slot, unit) in name.iter_mut().zip(
            match multi_key {
                Some((name, _)) => name,
                None if index == 1 => "Detector",
                None => "Stereo",
            }
            .encode_utf16(),
        ) {
            *slot = unit;
        }
        // SAFETY: caller supplies a writable SDK bus structure.
        unsafe {
            bus.write(BusInfo {
                mediaType: media,
                direction: dir,
                channelCount: if let Some((_, arrangement)) = multi_key {
                    arrangement.count_ones() as i32
                } else if media == 1 || self.mono || index == 1 {
                    1
                } else {
                    2
                },
                name,
                busType: if index == 1 || multi_key.is_some() { 1 } else { 0 },
                flags: u32::from(index == 0),
            });
        }
        kResultOk
    }
    unsafe fn getRoutingInfo(
        &self,
        _input: *mut RoutingInfo,
        _output: *mut RoutingInfo,
    ) -> tresult {
        kNotImplemented
    }
    unsafe fn activateBus(
        &self,
        _media: MediaType,
        _dir: BusDirection,
        _index: i32,
        _state: TBool,
    ) -> tresult {
        kResultOk
    }
    unsafe fn setActive(&self, state: TBool) -> tresult {
        assert_eq!(
            self.creator,
            CreatorThread::current(),
            "native lifecycle left the creating owner"
        );
        if state != 0
            && self.recovery_failure
            && self
                .activations
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                > 0
        {
            return kResultFalse;
        }
        if state == 0
            && self.refuse
            && f64::from_bits(self.gain.load(std::sync::atomic::Ordering::Relaxed)) >= 0.5
        {
            return kResultFalse;
        }
        if self.processing.load(std::sync::atomic::Ordering::Relaxed) {
            return kResultFalse;
        }
        if state == 0 && (self.notify || self.bridge_deactivation_edit) {
            if f64::from_bits(self.gain.load(std::sync::atomic::Ordering::Relaxed)) == 0.625 {
                self.gain
                    .store(0.375_f64.to_bits(), std::sync::atomic::Ordering::Relaxed);
            }
            unsafe {
                self.notify_owner(4);
            }
        }
        self.active
            .store(state != 0, std::sync::atomic::Ordering::Relaxed);
        kResultOk
    }
    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        // SAFETY: forwarded borrowed stream from the host.
        unsafe { self.read_state(state) }
    }
    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        if self.bridge_state_boundary
            && f64::from_bits(self.gain.load(std::sync::atomic::Ordering::Relaxed)) >= 0.75
        {
            return unsafe { self.write_boundary_state(state) };
        }
        unsafe { self.write_state(state) }
    }
}
struct Audio {
    rate: f64,
    phase: f64,
    midi_phases: [f64; 16],
    velocity: f32,
    configured: bool,
    held: [[bool; 128]; 16],
    held_count: usize,
    extra: [f64; 2],
    expressions: expression::Voices,
    bridge_delay: crate::bridge_behaviors::DelayedEffect,
}
impl Default for Audio {
    fn default() -> Self {
        Self {
            rate: 0.0,
            phase: 0.0,
            midi_phases: [0.0; 16],
            velocity: 0.0,
            configured: false,
            held: [[false; 128]; 16],
            held_count: 0,
            extra: [0.5; 2],
            expressions: expression::Voices::default(),
            bridge_delay: crate::bridge_behaviors::DelayedEffect::default(),
        }
    }
}
impl Component {
    unsafe fn notify_owner(&self, flags: i32) {
        if let Some(handler) = unsafe { &*self.handler.get() } {
            unsafe {
                handler.restartComponent(flags);
            }
            if let Some(dirty) = handler.cast::<IComponentHandler2>() {
                unsafe {
                    dirty.setDirty(1);
                }
            }
        }
    }
    unsafe fn read_state(&self, state: *mut IBStream) -> tresult {
        let Some(stream) = (unsafe { vst3::ComRef::from_raw(state) }) else {
            return kInvalidArgument;
        };
        let mut bytes = [0; 8];
        let mut count = 0;
        if unsafe { stream.read(bytes.as_mut_ptr().cast(), 8, &mut count) } != kResultOk
            || count != 8
        {
            return kResultFalse;
        }
        let value = f64::from_le_bytes(bytes);
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return kInvalidArgument;
        }
        self.gain
            .store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
        kResultOk
    }
    unsafe fn write_boundary_state(&self, state: *mut IBStream) -> tresult {
        let Some(stream) = (unsafe { vst3::ComRef::from_raw(state) }) else {
            return kInvalidArgument;
        };
        // Native VST2 container adds16B, ordinary controller adds8B. This
        // component supplies exactly the remaining native payload budget.
        let target = crate::bridge_behaviors::STATE_LIMIT - 24;
        let mut gain = self
            .gain
            .load(std::sync::atomic::Ordering::Relaxed)
            .to_le_bytes();
        let mut actual = 0;
        if unsafe { stream.write(gain.as_mut_ptr().cast(), gain.len() as i32, &mut actual) }
            != kResultOk
            || actual != gain.len() as i32
        {
            return kResultFalse;
        }
        let mut written = gain.len();
        let mut padding = [0u8; 8192];
        while written < target {
            let count = (target - written).min(padding.len());
            if unsafe { stream.write(padding.as_mut_ptr().cast(), count as i32, &mut actual) }
                != kResultOk
                || actual != count as i32
            {
                return kResultFalse;
            }
            written += count;
        }
        kResultOk
    }
    unsafe fn write_state(&self, state: *mut IBStream) -> tresult {
        let Some(stream) = (unsafe { vst3::ComRef::from_raw(state) }) else {
            return kInvalidArgument;
        };
        if !crate::bridge_behaviors::before_capture(
            self.bridge_capture_fault,
            f64::from_bits(self.gain.load(std::sync::atomic::Ordering::Relaxed)),
        ) {
            let mut bytes = [0u8; 3];
            let mut count = 0;
            unsafe {
                stream.write(bytes.as_mut_ptr().cast(), 3, &mut count);
            }
            return kInternalError;
        }
        let mut bytes = self
            .gain
            .load(std::sync::atomic::Ordering::Relaxed)
            .to_le_bytes();
        let mut count = 0;
        if unsafe { stream.write(bytes.as_mut_ptr().cast(), 8, &mut count) } == kResultOk
            && count == 8
        {
            kResultOk
        } else {
            kResultFalse
        }
    }
}
impl IAudioProcessorTrait for Component {
    unsafe fn setBusArrangements(
        &self,
        _inputs: *mut SpeakerArrangement,
        numIns: i32,
        _outputs: *mut SpeakerArrangement,
        numOuts: i32,
    ) -> tresult {
        if numIns
            == if self.sidechain {
                2
            } else if self.multi_sidechain {
                1 + MULTI_KEYS.len() as i32
            } else {
                i32::from(!self.instrument)
            }
            && numOuts == 1
        {
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn getBusArrangement(
        &self,
        dir: BusDirection,
        index: i32,
        arr: *mut SpeakerArrangement,
    ) -> tresult {
        if self.multi_sidechain && dir == 0 && (1..=MULTI_KEYS.len() as i32).contains(&index) {
            if arr.is_null() {
                return kInvalidArgument;
            }
            unsafe {
                arr.write(MULTI_KEYS[index as usize - 1].1);
            }
            return kResultOk;
        }
        if (index != 0 && !(self.sidechain && dir == 0 && index == 1)) || arr.is_null() {
            return kInvalidArgument;
        }
        unsafe {
            arr.write(if self.mono || index == 1 { 1 } else { 3 });
        }
        kResultOk
    }
    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size == 0 { kResultOk } else { kResultFalse }
    }
    unsafe fn getLatencySamples(&self) -> u32 {
        if self.expression_probe {
            return 0;
        }
        if self.bridge_bad_latency {
            return u32::MAX;
        }
        if self.bridge_delayed {
            return crate::bridge_behaviors::DELAY as u32;
        }
        if self.notify && f64::from_bits(self.gain.load(std::sync::atomic::Ordering::Relaxed)) < 0.5
        {
            64
        } else {
            17
        }
    }
    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        if setup.is_null() || self.active.load(std::sync::atomic::Ordering::Relaxed) {
            return kInvalidArgument;
        }
        let setup = unsafe { &*setup };
        if setup.symbolicSampleSize != 0 || setup.maxSamplesPerBlock <= 0 || setup.sampleRate <= 0.0
        {
            return kInvalidArgument;
        }
        let audio = unsafe { &mut *self.audio.get() };
        audio.rate = setup.sampleRate;
        audio.configured = true;
        kResultOk
    }
    unsafe fn setProcessing(&self, state: TBool) -> tresult {
        if !self.active.load(std::sync::atomic::Ordering::Relaxed) {
            return kResultFalse;
        }
        if state == 0
            && self.refuse
            && f64::from_bits(self.gain.load(std::sync::atomic::Ordering::Relaxed)) == 0.75
        {
            return kResultFalse;
        }
        self.processing
            .store(state != 0, std::sync::atomic::Ordering::Relaxed);
        if state == 0 {
            let audio = unsafe { &mut *self.audio.get() };
            audio.phase = 0.0;
            audio.velocity = 0.0;
            audio.held = [[false; 128]; 16];
            audio.held_count = 0;
            audio.expressions.clear();
            audio.bridge_delay.reset();
        }
        kResultOk
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        if self.bridge_hang {
            crate::bridge_behaviors::hang();
        }
        if self.fault == 1 {
            return kInternalError;
        }
        if data.is_null() || !self.processing.load(std::sync::atomic::Ordering::Relaxed) {
            return kResultFalse;
        }
        let data = unsafe { &mut *data };
        let audio = unsafe { &mut *self.audio.get() };
        if !audio.configured
            || data.numOutputs != 1
            || (self.sidechain && data.numInputs != 2)
            || (self.multi_sidechain && data.numInputs != 1 + MULTI_KEYS.len() as i32)
            || data.numSamples <= 0
            || data.processContext.is_null()
        {
            return kInvalidArgument;
        }
        let context = unsafe { &*data.processContext };
        if context.sampleRate != audio.rate
            || context.state & 1024 == 0
            || !context.tempo.is_finite()
        {
            return kResultFalse;
        }
        for (index, value) in [
            context.tempo / 300.0,
            f64::from(context.state & 2 != 0),
            context.projectTimeMusic / 1000.0,
        ]
        .into_iter()
        .enumerate()
        {
            self.meters[index].store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
            if let Some(changes) = unsafe { vst3::ComRef::from_raw(data.outputParameterChanges) } {
                let mut at = 0;
                let id = 22 + index as u32;
                if let Some(queue) =
                    unsafe { vst3::ComRef::from_raw(changes.addParameterData(&id, &mut at)) }
                {
                    unsafe {
                        queue.addPoint(0, value, &mut at);
                    }
                }
            }
        }
        let output = unsafe { &mut *data.outputs };
        if output.numChannels != if self.mono { 1 } else { 2 } {
            return kInvalidArgument;
        }
        if self.multi_sidechain {
            // A host that hands over a narrower bus is refused, not read past.
            for (bus, (_, arrangement)) in MULTI_KEYS.iter().enumerate() {
                let auxiliary = unsafe { &*data.inputs.add(bus + 1) };
                if auxiliary.numChannels != arrangement.count_ones() as i32
                    || unsafe { auxiliary.__field0.channelBuffers32 }.is_null()
                {
                    return kInvalidArgument;
                }
            }
        }
        let mut gain = f64::from_bits(self.gain.load(std::sync::atomic::Ordering::Relaxed));
        let changes = unsafe { vst3::ComRef::from_raw(data.inputParameterChanges) };
        let events = unsafe { vst3::ComRef::from_raw(data.inputEvents) };
        for frame in 0..data.numSamples {
            if let Some(changes) = &changes {
                for index in 0..unsafe { changes.getParameterCount() } {
                    let Some(queue) =
                        (unsafe { vst3::ComRef::from_raw(changes.getParameterData(index)) })
                    else {
                        continue;
                    };
                    let id = unsafe { queue.getParameterId() };
                    if id != 7 && !(self.multi_params && (8..=9).contains(&id)) && !(self.midi_mapping && (100..116).contains(&id)) {
                        continue;
                    }
                    for point in 0..unsafe { queue.getPointCount() } {
                        let (mut offset, mut value) = (0, 0.0);
                        if unsafe { queue.getPoint(point, &mut offset, &mut value) } == kResultOk
                            && offset == frame
                        {
                            if id == 7 {
                                gain = value;
                                self.gain
                                    .store(gain.to_bits(), std::sync::atomic::Ordering::Relaxed);
                            } else if self.midi_mapping && (100..116).contains(&id) {
                                self.midi_bend[(id - 100) as usize].store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
                            } else {
                                audio.extra[(id - 8) as usize] = value;
                                self.extra[(id - 8) as usize]
                                    .store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
            if self.bridge_controlled_error && !crate::bridge_behaviors::controlled_process_ok(gain)
            {
                return kResultFalse;
            }
            if let Some(events) = &events {
                for index in 0..unsafe { events.getEventCount() } {
                    let mut event: Event = unsafe { std::mem::zeroed() };
                    if unsafe { events.getEvent(index, &mut event) } == kResultOk
                        && event.sampleOffset == frame
                    {
                        if self.expression_probe {
                            audio.expressions.event(&event);
                            continue;
                        }
                        if event.r#type == 0 {
                            let note = unsafe { event.__field0.noteOn };
                            let held = &mut audio.held[note.channel as usize][note.pitch as usize];
                            if !*held {
                                *held = true;
                                audio.held_count += 1;
                            }
                            audio.velocity = note.velocity;
                        } else if event.r#type == 1 {
                            let note = unsafe { event.__field0.noteOff };
                            let held = &mut audio.held[note.channel as usize][note.pitch as usize];
                            if *held {
                                *held = false;
                                audio.held_count -= 1;
                            }
                            if audio.held_count == 0 {
                                audio.velocity = 0.0;
                            }
                        }
                    }
                }
            }
            let synth = if self.instrument {
                (audio.phase * std::f64::consts::TAU).sin() as f32 * audio.velocity
            } else {
                0.0
            };
            audio.phase = (audio.phase + 440.0 / audio.rate).fract();
            let mut midi_synth = [0.0; 2];
            if self.midi_mapping {
                for midi_channel in 0..16 {
                    let Some(key) = audio.held[midi_channel].iter().position(|held| *held) else { continue; };
                    let bend = f64::from_bits(self.midi_bend[midi_channel].load(std::sync::atomic::Ordering::Relaxed));
                    let frequency = 440.0 * 2.0_f64.powf((key as f64 + (bend - 0.5) * 4.0 - 69.0) / 12.0);
                    let value = (audio.midi_phases[midi_channel] * std::f64::consts::TAU).sin() as f32;
                    // Independent color/channel probes in the two output sides.
                    if midi_channel == 2 { midi_synth[0] += value; }
                    else if midi_channel == 5 { midi_synth[1] += value; }
                    else { midi_synth[0] += value; midi_synth[1] += value; }
                    audio.midi_phases[midi_channel] = (audio.midi_phases[midi_channel] + frequency / audio.rate).fract();
                }
            }
            if self.bridge_delayed {
                let input = unsafe { &*data.inputs };
                let pair = unsafe {
                    [
                        *(*input.__field0.channelBuffers32).add(frame as usize),
                        *(*input.__field0.channelBuffers32.add(1)).add(frame as usize),
                    ]
                };
                let delayed = audio.bridge_delay.tick(pair, gain as f32);
                for (channel, value) in delayed.into_iter().enumerate() {
                    unsafe {
                        (*output.__field0.channelBuffers32.add(channel))
                            .add(frame as usize)
                            .write(value);
                    }
                }
                continue;
            }
            let expression_pcm = if self.expression_probe { audio.expressions.pcm() } else { [0.0; 2] };
            for channel in 0..output.numChannels {
                let synth = if self.midi_mapping { midi_synth[channel as usize] } else { synth };
                let out = unsafe { *output.__field0.channelBuffers32.add(channel as usize) };
                let input = if self.instrument {
                    0.0
                } else {
                    let input = unsafe { &*data.inputs };
                    unsafe {
                        *(*input.__field0.channelBuffers32.add(channel as usize))
                            .add(frame as usize)
                    }
                };
                unsafe {
                    out.add(frame as usize).write(if self.fault == 2 {
                        f32::NAN
                    } else if self.bridge_note_probe {
                        f32::from(audio.held[0][69])
                    } else if self.expression_probe {
                        expression_pcm[channel as usize] * (gain as f32 * 2.0)
                    } else if self.multi_params && channel == 1 {
                        audio.extra.iter().sum::<f64>() as f32
                    } else {
                        let detector = if self.sidechain {
                            let auxiliary = &*data.inputs.add(1);
                            *(*auxiliary.__field0.channelBuffers32).add(frame as usize)
                        } else if self.multi_sidechain {
                            let mut marked = 0.0;
                            for (bus, marks) in crate::plugin::MULTI_KEY_MARKS.iter().enumerate() {
                                let auxiliary = &*data.inputs.add(bus + 1);
                                for (index, mark) in marks.iter().enumerate() {
                                    marked += *(*auxiliary.__field0.channelBuffers32.add(index))
                                        .add(frame as usize)
                                        * mark[channel as usize];
                                }
                            }
                            marked
                        } else {
                            0.0
                        };
                        (input + synth) * (gain as f32 * 2.0) + detector
                    });
                }
            }
        }
        if self.bridge_event_flood && gain >= 0.75 {
            if let Some(events) = unsafe { vst3::ComRef::from_raw(data.outputEvents) } {
                let mut event: Event = unsafe { std::mem::zeroed() };
                event.r#type = 0;
                event.__field0.noteOn.channel = 0;
                event.__field0.noteOn.pitch = 69;
                event.__field0.noteOn.velocity = 0.75;
                event.__field0.noteOn.noteId = -1;
                // Exceed the host's recovery/fixed-block note reserve (27,650),
                // so this fixture still exercises rejected native output.
                for _ in 0..30_000 {
                    unsafe {
                        events.addEvent(&mut event);
                    }
                }
            }
        }
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> u32 {
        64
    }
}
impl IEditControllerTrait for Component {
    unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
        unsafe { self.read_state(state) }
    }
    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        unsafe { self.read_state(state) }
    }
    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        unsafe { self.write_state(state) }
    }
    unsafe fn getParameterCount(&self) -> i32 {
        if self.midi_mapping { 20 } else if self.multi_params { 6 } else { 4 }
    }
    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        if !(0..unsafe { self.getParameterCount() }).contains(&index) || info.is_null() {
            return kInvalidArgument;
        }
        if self.midi_mapping && index >= 4 {
            let mut title = [0; 128];
            for (slot, unit) in title.iter_mut().zip(format!("Bend channel {}", index - 3).encode_utf16()) { *slot = unit; }
            unsafe { info.write(ParameterInfo { id: 100 + (index - 4) as u32, title, shortTitle: title, units: [0; 128], stepCount: 0, defaultNormalizedValue: 0.5, unitId: 0, flags: 1 }); }
            return kResultOk;
        }
        let mut title = [0; 128];
        for (s, c) in title.iter_mut().zip(
            ["Gain", "Tempo", "Playing", "Beats", "Pending", "Editor"][index as usize]
                .encode_utf16(),
        ) {
            *s = c;
        }
        unsafe {
            info.write(ParameterInfo {
                id: if index == 0 {
                    7
                } else if index >= 4 {
                    4 + index as u32
                } else {
                    21 + index as u32
                },
                title,
                shortTitle: title,
                units: [0; 128],
                stepCount: 0,
                defaultNormalizedValue: if index == 0 || index >= 4 { 0.5 } else { 0.0 },
                unitId: 0,
                flags: if index == 0 || index >= 4 { 1 } else { 2 },
            });
        }
        kResultOk
    }
    unsafe fn getParamStringByValue(&self, id: u32, value: f64, text: *mut String128) -> tresult {
        if id != 7 || text.is_null() {
            return kInvalidArgument;
        }
        // Deterministic editor gesture through the actual registered host
        // handler. The audio probe changes only when its queued point processes.
        if self.multi_params
            && value == 0.9375
            && let Some(handler) = unsafe { &*self.handler.get() }
        {
            unsafe {
                handler.performEdit(9, 0.75);
            }
        }
        let mut result = [0; 128];
        for (s, c) in result
            .iter_mut()
            .zip(format!("{:.2}", value * 2.0).encode_utf16())
        {
            *s = c;
        }
        unsafe {
            text.write(result);
        }
        kResultOk
    }
    unsafe fn getParamValueByString(&self, id: u32, text: *mut TChar, value: *mut f64) -> tresult {
        if id != 7 || text.is_null() || value.is_null() {
            return kInvalidArgument;
        }
        let mut bytes = Vec::new();
        for index in 0..128 {
            let unit = unsafe { *text.add(index) };
            if unit == 0 {
                break;
            }
            bytes.push(unit);
        }
        let Ok(parsed) = String::from_utf16_lossy(&bytes).parse::<f64>() else {
            return kResultFalse;
        };
        unsafe {
            value.write(parsed * 0.5);
        }
        kResultOk
    }
    unsafe fn normalizedParamToPlain(&self, _id: u32, value: f64) -> f64 {
        value * 2.0
    }
    unsafe fn plainParamToNormalized(&self, _id: u32, value: f64) -> f64 {
        value * 0.5
    }
    unsafe fn getParamNormalized(&self, id: u32) -> f64 {
        if self.midi_mapping && (100..116).contains(&id) { return f64::from_bits(self.midi_bend[(id - 100) as usize].load(std::sync::atomic::Ordering::Relaxed)); }
        if self.multi_params && (8..=9).contains(&id) {
            return f64::from_bits(
                self.extra[(id - 8) as usize].load(std::sync::atomic::Ordering::Relaxed),
            );
        }
        if (22..=24).contains(&id) {
            return f64::from_bits(
                self.meters[(id - 22) as usize].load(std::sync::atomic::Ordering::Relaxed),
            );
        }
        f64::from_bits(self.gain.load(std::sync::atomic::Ordering::Relaxed))
    }
    unsafe fn setParamNormalized(&self, id: u32, mut value: f64) -> tresult {
        if self.midi_mapping && (100..116).contains(&id) && value.is_finite() && (0.0..=1.0).contains(&value) {
            self.midi_bend[(id - 100) as usize].store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
            return kResultOk;
        }
        if self.multi_params
            && (8..=9).contains(&id)
            && value.is_finite()
            && (0.0..=1.0).contains(&value)
        {
            self.extra[(id - 8) as usize]
                .store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
            return kResultOk;
        }
        if id != 7 || !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return kInvalidArgument;
        }
        if self.notify
            && value == 0.875
            && !self
                .notified
                .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            value = 0.625; // a native preset edit, independent of the host value
            unsafe {
                self.notify_owner(5);
            }
        }
        self.gain
            .store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
        kResultOk
    }
    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        unsafe {
            *self.handler.get() =
                vst3::ComRef::from_raw(handler).map(|handler| handler.to_com_ptr());
        }
        kResultOk
    }
    unsafe fn createView(&self, name: FIDString) -> *mut IPlugView {
        #[cfg(windows)]
        {
            if !self.editor || name.is_null() || unsafe { std::ffi::CStr::from_ptr(name) } != c"editor" {
                return ptr::null_mut();
            }
            // The view keeps its own references, never a borrowed Component pointer.
            editor::create(self.gain.clone(), unsafe { (&*self.handler.get()).clone() })
        }
        #[cfg(not(windows))]
        {
            let _ = (name, self.editor);
            ptr::null_mut()
        }
    }
}
/// SDK factory export, independently implemented from the host.
impl IMidiMappingTrait for Component {
    unsafe fn getMidiControllerAssignment(&self, bus: i32, channel: i16, number: CtrlNumber, id: *mut ParamID) -> tresult {
        if !self.midi_mapping || bus != 0 || !(0..16).contains(&channel) || number != 129 || id.is_null() { return kResultFalse; }
        unsafe { id.write(100 + channel as u32); }
        kResultOk
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    let factory: ComPtr<IPluginFactory> = ComWrapper::new(Factory).to_com_ptr().unwrap();
    factory.into_raw()
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
pub extern "system" fn ModuleEntry(_module: *mut c_void) -> bool {
    true
}
#[unsafe(no_mangle)]
pub extern "system" fn ModuleExit() -> bool {
    true
}
