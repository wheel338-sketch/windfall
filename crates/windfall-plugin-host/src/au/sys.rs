//! Minimal public AudioToolbox/CoreFoundation declarations. No SDK vendoring.
use std::ffi::{c_char, c_void};
pub type Unit = *mut c_void;
pub type Component = *mut c_void;
pub type Cf = *const c_void;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Description {
    pub kind: u32,
    pub subtype: u32,
    pub manufacturer: u32,
    pub flags: u32,
    pub mask: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct StreamFormat {
    pub rate: f64,
    pub format: u32,
    pub flags: u32,
    pub bytes_packet: u32,
    pub frames_packet: u32,
    pub bytes_frame: u32,
    pub channels: u32,
    pub bits: u32,
    pub reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SmpteTime {
    pub subframes: i16,
    pub divisor: i16,
    pub counter: u32,
    pub kind: u32,
    pub flags: u32,
    pub hours: i16,
    pub minutes: i16,
    pub seconds: i16,
    pub frames: i16,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Timestamp {
    pub sample: f64,
    pub host: u64,
    pub rate: f64,
    pub word: u64,
    pub smpte: SmpteTime,
    pub flags: u32,
    pub reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Buffer {
    pub channels: u32,
    pub bytes: u32,
    pub data: *mut c_void,
}
#[repr(C)]
pub struct BufferList {
    pub count: u32,
    pub buffers: [Buffer; 1],
}
/// Owned backing storage with the same AudioBufferList prefix. Native callers
/// may pass shorter lists; only count buffers are accessed through raw pointers.
#[repr(C)]
pub struct OwnedBufferList { pub count: u32, pub buffers: [Buffer; 64] }
#[repr(C)]
pub struct HostCallbacks {
    pub context: *mut c_void,
    pub beat_tempo: Option<unsafe extern "C" fn(*mut c_void, *mut f64, *mut f64) -> i32>,
    pub musical_location: Option<unsafe extern "C" fn(*mut c_void, *mut u32, *mut f32, *mut u32, *mut f64) -> i32>,
    pub transport: Option<unsafe extern "C" fn(*mut c_void, *mut u8, *mut u8, *mut f64, *mut u8, *mut f64, *mut f64) -> i32>,
}
pub type Render =
    unsafe extern "C" fn(*mut c_void, *mut u32, *const Timestamp, u32, u32, *mut BufferList) -> i32;
#[repr(C)]
pub struct Callback {
    pub render: Option<Render>,
    pub context: *mut c_void,
}
#[repr(C)]
pub struct ParameterInfo {
    pub name: [c_char; 52],
    pub unit_name: Cf,
    pub clump: u32,
    pub cf_name: Cf,
    pub unit: u32,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub flags: u32,
}
#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    pub fn AudioComponentFindNext(
        previous: Component,
        description: *const Description,
    ) -> Component;
    pub fn AudioComponentGetDescription(component: Component, description: *mut Description)
    -> i32;
    pub fn AudioComponentCopyName(component: Component, name: *mut Cf) -> i32;
    pub fn AudioComponentGetVersion(component: Component, version: *mut u32) -> i32;
    pub fn AudioComponentInstanceNew(component: Component, unit: *mut Unit) -> i32;
    pub fn AudioComponentInstanceDispose(unit: Unit) -> i32;
    pub fn AudioUnitInitialize(unit: Unit) -> i32;
    pub fn AudioUnitUninitialize(unit: Unit) -> i32;
    pub fn AudioUnitReset(unit: Unit, scope: u32, element: u32) -> i32;
    pub fn AudioUnitGetPropertyInfo(
        unit: Unit,
        property: u32,
        scope: u32,
        element: u32,
        size: *mut u32,
        writable: *mut u8,
    ) -> i32;
    pub fn AudioUnitGetProperty(
        unit: Unit,
        property: u32,
        scope: u32,
        element: u32,
        data: *mut c_void,
        size: *mut u32,
    ) -> i32;
    pub fn AudioUnitSetProperty(
        unit: Unit,
        property: u32,
        scope: u32,
        element: u32,
        data: *const c_void,
        size: u32,
    ) -> i32;
    pub fn AudioUnitGetParameter(
        unit: Unit,
        id: u32,
        scope: u32,
        element: u32,
        value: *mut f32,
    ) -> i32;
    pub fn AudioUnitSetParameter(
        unit: Unit,
        id: u32,
        scope: u32,
        element: u32,
        value: f32,
        offset: u32,
    ) -> i32;
    pub fn AudioUnitRender(
        unit: Unit,
        flags: *mut u32,
        timestamp: *const Timestamp,
        bus: u32,
        frames: u32,
        audio: *mut BufferList,
    ) -> i32;
    pub fn MusicDeviceMIDIEvent(
        unit: Unit,
        status: u32,
        data1: u32,
        data2: u32,
        offset: u32,
    ) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFRelease(value: Cf);
    pub fn CFStringGetCString(value: Cf, bytes: *mut c_char, capacity: isize, encoding: u32) -> u8;
    pub fn CFDataCreate(allocator: Cf, bytes: *const u8, length: isize) -> Cf;
    pub fn CFDataGetLength(data: Cf) -> isize;
    pub fn CFDataGetBytePtr(data: Cf) -> *const u8;
    pub fn CFPropertyListCreateData(
        allocator: Cf,
        property_list: Cf,
        format: isize,
        options: usize,
        error: *mut Cf,
    ) -> Cf;
    pub fn CFPropertyListCreateWithData(
        allocator: Cf,
        data: Cf,
        options: usize,
        format: *mut isize,
        error: *mut Cf,
    ) -> Cf;
    pub fn CFBundleCreate(allocator: Cf, url: Cf) -> Cf;
    pub fn CFBundleLoadExecutable(bundle: Cf) -> u8;
}
