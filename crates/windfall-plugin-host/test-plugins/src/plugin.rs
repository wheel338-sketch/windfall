//! The plugin object every kind shares, and its audio processing.

use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::events::{
    CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_MIDI, CLAP_EVENT_NOTE_CHOKE, CLAP_EVENT_NOTE_END,
    CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_ON, CLAP_EVENT_PARAM_GESTURE_BEGIN,
    CLAP_EVENT_PARAM_GESTURE_END, CLAP_EVENT_PARAM_VALUE, CLAP_TRANSPORT_HAS_BEATS_TIMELINE,
    CLAP_TRANSPORT_HAS_TEMPO, CLAP_TRANSPORT_IS_PLAYING, clap_event_header, clap_event_midi,
    clap_event_note, clap_event_param_gesture, clap_event_param_value, clap_input_events,
    clap_output_events,
};
use clap_sys::fixedpoint::CLAP_BEATTIME_FACTOR;
use clap_sys::host::clap_host;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::{
    CLAP_PROCESS_CONTINUE, CLAP_PROCESS_ERROR, CLAP_PROCESS_SLEEP, clap_process,
    clap_process_status,
};

use crate::Kind;
use crate::ext;

/// Samples by which the sine instrument's output lags its notes. It holds
/// its output back by this much for real, so a host that compensates lines
/// it up exactly.
pub(crate) const SINE_LATENCY: usize = 32;

/// The key that makes the gain plugin act as if its knob had been turned in
/// its editor: it sets its gain to twice the note's velocity and tells the
/// host with a gesture.
pub(crate) const GESTURE_KEY: i16 = 127;

const VOICES: usize = 16;

/// Where each value lives in [`Plugin::values`].
pub(crate) mod slot {
    pub const GAIN: usize = 0;
    pub const MODE: usize = 1;
    pub const TIMER_TICKS: usize = 2;
    pub const MAIN_THREAD_CALLS: usize = 3;
    pub const TEMPO: usize = 4;
    pub const PLAYING: usize = 5;
    pub const BEATS: usize = 6;
    pub const DENORMALS_FLUSHED: usize = 7;
    pub const IN_PLACE: usize = 8;
    pub const LEVEL: usize = 0;
    pub const COUNT: usize = 9;
}

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    key: i16,
    channel: i16,
    note_id: i32,
    phase: f64,
    step: f64,
    amplitude: f64,
}

const SILENT_VOICE: Voice = Voice {
    active: false,
    key: 0,
    channel: 0,
    note_id: -1,
    phase: 0.0,
    step: 0.0,
    amplitude: 0.0,
};

/// What only the audio thread touches.
struct Audio {
    sample_rate: f64,
    voices: [Voice; VOICES],
    delay: [[f32; SINE_LATENCY]; 2],
    delay_at: usize,
    /// Frames of the delay that still hold sound after the last voice ended.
    ringing: usize,
    blocks: u32,
    bridge_delay: crate::bridge_behaviors::DelayedEffect,
    bridge_notes: [bool; 128],
}

/// What only the main thread touches.
pub(crate) struct Main {
    pub timer: Option<u32>,
    pub timer_ticks: u64,
    pub width: u32,
    pub height: u32,
    pub gui_created: bool,
    #[cfg(windows)]
    pub window: windows_sys::Win32::Foundation::HWND,
}

#[repr(C)]
pub(crate) struct Plugin {
    clap: clap_plugin,
    pub host: *const clap_host,
    pub kind: Kind,
    /// Parameter values and readings, as the bits of an `f64`, so both
    /// threads can use them.
    values: [AtomicU64; slot::COUNT],
    audio: UnsafeCell<Audio>,
    main: UnsafeCell<Main>,
}

impl Plugin {
    /// # Safety
    /// `plugin` must be a pointer this library handed out from `create` and
    /// has not destroyed.
    pub unsafe fn from_raw<'a>(plugin: *const clap_plugin) -> &'a Plugin {
        // SAFETY: `plugin_data` was set to the `Plugin` in `create`.
        unsafe { &*((*plugin).plugin_data as *const Plugin) }
    }

    pub fn value(&self, slot: usize) -> f64 {
        f64::from_bits(self.values[slot].load(Ordering::Relaxed))
    }

    pub fn set_value(&self, slot: usize, value: f64) {
        self.values[slot].store(value.to_bits(), Ordering::Relaxed);
    }

    pub fn add_value(&self, slot: usize, amount: f64) {
        self.set_value(slot, self.value(slot) + amount);
    }

    /// # Safety
    /// Only for functions CLAP marks `[main-thread]`, and the reference must
    /// not outlive the call.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn main(&self) -> &mut Main {
        // SAFETY: the host calls main-thread functions from one thread, one
        // at a time.
        unsafe { &mut *self.main.get() }
    }

    /// # Safety
    /// Only for functions CLAP marks `[audio-thread]`, or while the plugin
    /// is not active.
    #[allow(clippy::mut_from_ref)]
    unsafe fn audio(&self) -> &mut Audio {
        // SAFETY: the host never runs two audio-thread functions at once.
        unsafe { &mut *self.audio.get() }
    }
}

pub(crate) fn create(
    kind: Kind,
    descriptor: &'static clap_plugin_descriptor,
    host: *const clap_host,
) -> *const clap_plugin {
    let values = std::array::from_fn(|_| AtomicU64::new(0));
    let plugin = Box::new(Plugin {
        clap: clap_plugin {
            desc: descriptor,
            plugin_data: ptr::null_mut(),
            init: Some(init),
            destroy: Some(destroy),
            activate: Some(activate),
            deactivate: Some(deactivate),
            start_processing: Some(start_processing),
            stop_processing: Some(stop_processing),
            reset: Some(reset),
            process: Some(process),
            get_extension: Some(get_extension),
            on_main_thread: Some(on_main_thread),
        },
        host,
        kind,
        values,
        audio: UnsafeCell::new(Audio {
            sample_rate: 48_000.0,
            voices: [SILENT_VOICE; VOICES],
            delay: [[0.0; SINE_LATENCY]; 2],
            delay_at: 0,
            ringing: 0,
            blocks: 0,
            bridge_delay: crate::bridge_behaviors::DelayedEffect::default(),
            bridge_notes: [false; 128],
        }),
        main: UnsafeCell::new(Main {
            timer: None,
            timer_ticks: 0,
            width: 320,
            height: 200,
            gui_created: false,
            #[cfg(windows)]
            window: ptr::null_mut(),
        }),
    });
    for param in ext::params_of(kind) {
        plugin.set_value(param.slot, param.default);
    }
    let raw = Box::into_raw(plugin);
    // SAFETY: `raw` was just made from a box and nothing else holds it.
    unsafe {
        (*raw).clap.plugin_data = raw as *mut c_void;
        &raw const (*raw).clap
    }
}

unsafe extern "C" fn init(plugin: *const clap_plugin) -> bool {
    // SAFETY: the host passes the plugin it was given.
    let plugin = unsafe { Plugin::from_raw(plugin) };
    match plugin.kind {
        Kind::BridgeIdleHang => {
            if let Some(callback) = unsafe { (*plugin.host).request_callback } {
                unsafe { callback(plugin.host) };
            }
            true
        }
        Kind::InitFail => false,
        Kind::InitHang => loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        },
        Kind::InitCrash => std::process::abort(),
        _ => true,
    }
}

unsafe extern "C" fn destroy(plugin: *const clap_plugin) {
    // SAFETY: the host passes the plugin it was given and never uses it
    // again, so the box made in `create` can be taken back.
    unsafe {
        #[cfg(windows)]
        crate::gui::destroy_window(Plugin::from_raw(plugin));
        drop(Box::from_raw((*plugin).plugin_data as *mut Plugin));
    }
}

unsafe extern "C" fn activate(
    plugin: *const clap_plugin,
    sample_rate: f64,
    _min_frames: u32,
    _max_frames: u32,
) -> bool {
    // SAFETY: the host passes the plugin it was given, which is not
    // processing while it is activated.
    let audio = unsafe { Plugin::from_raw(plugin).audio() };
    audio.sample_rate = sample_rate;
    audio.blocks = 0;
    audio.voices = [SILENT_VOICE; VOICES];
    audio.delay = [[0.0; SINE_LATENCY]; 2];
    audio.delay_at = 0;
    audio.ringing = 0;
    audio.bridge_delay.reset();
    audio.bridge_notes.fill(false);
    true
}

unsafe extern "C" fn deactivate(_plugin: *const clap_plugin) {}

unsafe extern "C" fn start_processing(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C" fn stop_processing(_plugin: *const clap_plugin) {}

unsafe extern "C" fn reset(plugin: *const clap_plugin) {
    // SAFETY: the host passes the plugin it was given, on the audio thread.
    let audio = unsafe { Plugin::from_raw(plugin).audio() };
    audio.voices = [SILENT_VOICE; VOICES];
    audio.delay = [[0.0; SINE_LATENCY]; 2];
    audio.delay_at = 0;
    audio.ringing = 0;
    audio.bridge_delay.reset();
    audio.bridge_notes.fill(false);
}

unsafe extern "C" fn on_main_thread(plugin: *const clap_plugin) {
    // SAFETY: the host passes the plugin it was given.
    let plugin = unsafe { Plugin::from_raw(plugin) };
    if plugin.kind == Kind::BridgeIdleHang {
        crate::bridge_behaviors::hang();
    }
    plugin.add_value(slot::MAIN_THREAD_CALLS, 1.0);
}

unsafe extern "C" fn get_extension(plugin: *const clap_plugin, id: *const c_char) -> *const c_void {
    if id.is_null() {
        return ptr::null();
    }
    // SAFETY: the host passes the plugin it was given and a valid C string.
    unsafe { ext::get(Plugin::from_raw(plugin).kind, CStr::from_ptr(id)) }
}

/// The events of one block, read in order.
struct Incoming {
    list: *const clap_input_events,
    count: u32,
    next: u32,
}

impl Incoming {
    /// # Safety
    /// `list` must be null or the host's valid event list.
    unsafe fn new(list: *const clap_input_events) -> Self {
        let count = if list.is_null() {
            0
        } else {
            // SAFETY: the list is valid by the caller's promise.
            unsafe { (*list).size.map_or(0, |size| size(list)) }
        };
        Self {
            list,
            count,
            next: 0,
        }
    }

    /// The next event that is due at or before `frame`.
    ///
    /// # Safety
    /// The list given to `new` must still be valid.
    unsafe fn due(&mut self, frame: u32) -> Option<*const clap_event_header> {
        if self.next >= self.count {
            return None;
        }
        // SAFETY: the list is valid and `next` is below its size.
        let header = unsafe { (*self.list).get?(self.list, self.next) };
        // SAFETY: the host returns a valid event for a valid index.
        if header.is_null() || unsafe { (*header).time } > frame {
            return None;
        }
        self.next += 1;
        Some(header)
    }
}

/// # Safety
/// `out` must be null or the host's valid output list, and `event` a
/// complete CLAP event that starts with its header.
unsafe fn push<T>(out: *const clap_output_events, event: &T) {
    if out.is_null() {
        return;
    }
    // SAFETY: the list is valid by the caller's promise.
    if let Some(try_push) = unsafe { (*out).try_push } {
        // SAFETY: as above, and the event starts with a header.
        unsafe { try_push(out, event as *const T as *const clap_event_header) };
    }
}

fn header<T>(time: u32, kind: u16) -> clap_event_header {
    clap_event_header {
        size: size_of::<T>() as u32,
        time,
        space_id: CLAP_CORE_EVENT_SPACE_ID,
        type_: kind,
        flags: 0,
    }
}

/// The sample pointer of one channel of one port, or null if the host gave
/// fewer ports or channels than the plugin has.
///
/// # Safety
/// `buffers` must point at `ports` valid buffers.
unsafe fn channel(
    buffers: *const clap_audio_buffer,
    ports: u32,
    port: u32,
    index: u32,
) -> *mut f32 {
    if buffers.is_null() || port >= ports {
        return ptr::null_mut();
    }
    // SAFETY: `port` is in range by the check above.
    let buffer = unsafe { &*buffers.add(port as usize) };
    if buffer.data32.is_null() || index >= buffer.channel_count {
        return ptr::null_mut();
    }
    // SAFETY: `index` is below the buffer's channel count.
    unsafe { *buffer.data32.add(index as usize) }
}

unsafe extern "C" fn process(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> clap_process_status {
    // SAFETY: the host passes the plugin it was given and a valid block.
    let (plugin, block) = unsafe { (Plugin::from_raw(plugin), &*process) };
    // SAFETY: this is the audio thread.
    let audio = unsafe { plugin.audio() };
    audio.blocks += 1;

    // SAFETY: every pointer below comes from the host's block.
    unsafe {
        match plugin.kind {
            Kind::Gain => process_gain(plugin, block),
            Kind::BridgeDelayed
            | Kind::BridgeIdleHang
            | Kind::BridgeCaptureExit
            | Kind::BridgeCaptureHang
            | Kind::BridgeInvalidStream
            | Kind::BridgeBadLatency
            | Kind::BridgeEventFlood
            | Kind::BridgeIgnoredStreamError
            | Kind::BridgeStateBoundary
            | Kind::BridgeControlledProcessError => process_bridge_delayed(plugin, audio, block),
            Kind::BridgeNoteProbe => process_bridge_note_probe(audio, block),
            Kind::BridgeProcessHang => crate::bridge_behaviors::hang(),
            Kind::Sine | Kind::MidiSine => process_sine(plugin, audio, block),
            Kind::Swap => process_stereo(block, |left, right| (right, left)),
            Kind::Sidechain => process_sidechain(block),
            Kind::MultiSidechain => process_multi_sidechain(block),
            Kind::Mono => process_mono(block),
            Kind::Nan => process_stereo(block, |_, _| (f32::NAN, f32::INFINITY)),
            Kind::ProcessError => {
                if audio.blocks >= 3 {
                    CLAP_PROCESS_ERROR
                } else {
                    process_stereo(block, |left, right| (left, right))
                }
            }
            Kind::ProcessPanic => panic!("the test plugin panics in process"),
            Kind::Slow => {
                std::thread::sleep(std::time::Duration::from_millis(30));
                process_stereo(block, |left, right| (left, right))
            }
            Kind::AbsurdPorts | Kind::InitFail | Kind::InitHang | Kind::InitCrash => {
                process_stereo(block, |left, right| (left, right))
            }
        }
    }
}

/// # Safety
/// All channels and event lists belong to the host's valid current block.
unsafe fn process_bridge_delayed(
    plugin: &Plugin,
    audio: &mut Audio,
    block: &clap_process,
) -> clap_process_status {
    let channels = unsafe {
        [
            channel(block.audio_inputs, block.audio_inputs_count, 0, 0),
            channel(block.audio_inputs, block.audio_inputs_count, 0, 1),
            channel(block.audio_outputs, block.audio_outputs_count, 0, 0),
            channel(block.audio_outputs, block.audio_outputs_count, 0, 1),
        ]
    };
    if channels.iter().any(|channel| channel.is_null()) {
        return CLAP_PROCESS_ERROR;
    }
    let mut incoming = unsafe { Incoming::new(block.in_events) };
    for frame in 0..block.frames_count {
        while let Some(event) = unsafe { incoming.due(frame) } {
            unsafe {
                handle_param_event(plugin, event, block.out_events);
            }
        }
        let at = frame as usize;
        if plugin.kind == Kind::BridgeControlledProcessError
            && !crate::bridge_behaviors::controlled_process_ok(plugin.value(slot::GAIN))
        {
            return CLAP_PROCESS_ERROR;
        }
        let input = unsafe { [*channels[0].add(at), *channels[1].add(at)] };
        let output = audio
            .bridge_delay
            .tick(input, plugin.value(slot::GAIN) as f32);
        unsafe {
            *channels[2].add(at) = output[0];
            *channels[3].add(at) = output[1];
        }
    }
    if plugin.kind == Kind::BridgeEventFlood && plugin.value(slot::GAIN) >= 0.75 {
        let value = clap_event_param_value {
            header: header::<clap_event_param_value>(0, CLAP_EVENT_PARAM_VALUE),
            param_id: ext::GAIN_ID,
            cookie: ptr::null_mut(),
            note_id: -1,
            port_index: -1,
            channel: -1,
            key: -1,
            value: plugin.value(slot::GAIN),
        };
        for _ in 0..5000 {
            unsafe {
                push(block.out_events, &value);
            }
        }
    }
    CLAP_PROCESS_CONTINUE
}

/// # Safety
/// Event and output pointers belong to the host's current valid block.
unsafe fn process_bridge_note_probe(
    audio: &mut Audio,
    block: &clap_process,
) -> clap_process_status {
    let left = unsafe { channel(block.audio_outputs, block.audio_outputs_count, 0, 0) };
    let right = unsafe { channel(block.audio_outputs, block.audio_outputs_count, 0, 1) };
    if left.is_null() || right.is_null() {
        return CLAP_PROCESS_ERROR;
    }
    let mut incoming = unsafe { Incoming::new(block.in_events) };
    for frame in 0..block.frames_count {
        while let Some(event) = unsafe { incoming.due(frame) } {
            let head = unsafe { &*event };
            if head.space_id != CLAP_CORE_EVENT_SPACE_ID {
                continue;
            }
            if matches!(
                head.type_,
                CLAP_EVENT_NOTE_ON | CLAP_EVENT_NOTE_OFF | CLAP_EVENT_NOTE_CHOKE
            ) {
                let note = unsafe { &*(event as *const clap_event_note) };
                if note.channel == 0 && (0..128).contains(&note.key) {
                    audio.bridge_notes[note.key as usize] =
                        head.type_ == CLAP_EVENT_NOTE_ON && note.velocity > 0.0;
                } else if note.key < 0 && head.type_ != CLAP_EVENT_NOTE_ON {
                    audio.bridge_notes.fill(false);
                }
            }
        }
        let marker = f32::from(audio.bridge_notes[69]);
        unsafe {
            *left.add(frame as usize) = marker;
            *right.add(frame as usize) = marker;
        }
    }
    CLAP_PROCESS_CONTINUE
}

/// Runs `shape` over a stereo port pair, sample by sample.
///
/// # Safety
/// `block` must be the host's valid block.
unsafe fn process_stereo(
    block: &clap_process,
    shape: impl Fn(f32, f32) -> (f32, f32),
) -> clap_process_status {
    // SAFETY: the block's buffers are valid for its counts.
    let (in_left, in_right, out_left, out_right) = unsafe {
        (
            channel(block.audio_inputs, block.audio_inputs_count, 0, 0),
            channel(block.audio_inputs, block.audio_inputs_count, 0, 1),
            channel(block.audio_outputs, block.audio_outputs_count, 0, 0),
            channel(block.audio_outputs, block.audio_outputs_count, 0, 1),
        )
    };
    if in_left.is_null() || in_right.is_null() || out_left.is_null() || out_right.is_null() {
        return CLAP_PROCESS_ERROR;
    }
    for frame in 0..block.frames_count as usize {
        // SAFETY: every channel holds `frames_count` samples. Raw pointers
        // are used because input and output may be the same memory.
        unsafe {
            let (left, right) = shape(*in_left.add(frame), *in_right.add(frame));
            *out_left.add(frame) = left;
            *out_right.add(frame) = right;
        }
    }
    CLAP_PROCESS_CONTINUE
}

/// # Safety
/// `block` must be the host's valid block.
unsafe fn process_mono(block: &clap_process) -> clap_process_status {
    // SAFETY: the block's buffers are valid for its counts.
    let (input, output) = unsafe {
        (
            channel(block.audio_inputs, block.audio_inputs_count, 0, 0),
            channel(block.audio_outputs, block.audio_outputs_count, 0, 0),
        )
    };
    if input.is_null() || output.is_null() {
        return CLAP_PROCESS_ERROR;
    }
    for frame in 0..block.frames_count as usize {
        // SAFETY: both channels hold `frames_count` samples.
        unsafe { *output.add(frame) = *input.add(frame) * 0.5 };
    }
    CLAP_PROCESS_CONTINUE
}

/// Adds the sidechain to both main channels and copies it to the extra
/// output. A host that leaves out a port gets an error, not a crash.
///
/// # Safety
/// `block` must be the host's valid block.
unsafe fn process_sidechain(block: &clap_process) -> clap_process_status {
    // SAFETY: the block's buffers are valid for its counts.
    let (in_left, in_right, side, out_left, out_right, extra) = unsafe {
        (
            channel(block.audio_inputs, block.audio_inputs_count, 0, 0),
            channel(block.audio_inputs, block.audio_inputs_count, 0, 1),
            channel(block.audio_inputs, block.audio_inputs_count, 1, 0),
            channel(block.audio_outputs, block.audio_outputs_count, 0, 0),
            channel(block.audio_outputs, block.audio_outputs_count, 0, 1),
            channel(block.audio_outputs, block.audio_outputs_count, 1, 0),
        )
    };
    let channels = [in_left, in_right, side, out_left, out_right, extra];
    if channels.iter().any(|channel| channel.is_null()) {
        return CLAP_PROCESS_ERROR;
    }
    for frame in 0..block.frames_count as usize {
        // SAFETY: every channel holds `frames_count` samples.
        unsafe {
            let key = *side.add(frame);
            *out_left.add(frame) = *in_left.add(frame) + key;
            *out_right.add(frame) = *in_right.add(frame) + key;
            *extra.add(frame) = key;
        }
    }
    CLAP_PROCESS_CONTINUE
}

/// What each sidechain channel adds to the main output, as `[left, right]`
/// factors. Every channel of every sidechain input leaves a different mark,
/// so a test reads from the output alone which input the host fed and which
/// of its channels:
///
/// - the mono input goes to the left and, inverted, to the right,
/// - the stereo input goes straight to its own side,
/// - the four-channel input's first pair goes inverted to its own side, and
///   its second pair doubled to its own side.
pub(crate) const MULTI_KEY_MARKS: [&[[f32; 2]]; 3] = [
    &[[1.0, -1.0]],
    &[[1.0, 0.0], [0.0, 1.0]],
    &[[-1.0, 0.0], [0.0, -1.0], [2.0, 0.0], [0.0, 2.0]],
];

/// Passes the main signal and adds every sidechain channel with its mark
/// from [`MULTI_KEY_MARKS`]. A host that leaves out a port or a channel gets
/// an error, not a crash.
///
/// # Safety
/// `block` must be the host's valid block.
unsafe fn process_multi_sidechain(block: &clap_process) -> clap_process_status {
    // SAFETY: the block's buffers are valid for its counts.
    let (in_left, in_right, out_left, out_right) = unsafe {
        (
            channel(block.audio_inputs, block.audio_inputs_count, 0, 0),
            channel(block.audio_inputs, block.audio_inputs_count, 0, 1),
            channel(block.audio_outputs, block.audio_outputs_count, 0, 0),
            channel(block.audio_outputs, block.audio_outputs_count, 0, 1),
        )
    };
    if in_left.is_null() || in_right.is_null() || out_left.is_null() || out_right.is_null() {
        return CLAP_PROCESS_ERROR;
    }
    let mut keys = [ptr::null_mut::<f32>(); 7];
    let mut marks = [[0.0_f32; 2]; 7];
    let mut count = 0;
    for (port, port_marks) in MULTI_KEY_MARKS.iter().enumerate() {
        for (index, mark) in port_marks.iter().enumerate() {
            // SAFETY: as above. Sidechain inputs follow the main one.
            let key = unsafe {
                channel(
                    block.audio_inputs,
                    block.audio_inputs_count,
                    port as u32 + 1,
                    index as u32,
                )
            };
            if key.is_null() {
                return CLAP_PROCESS_ERROR;
            }
            keys[count] = key;
            marks[count] = *mark;
            count += 1;
        }
    }
    for frame in 0..block.frames_count as usize {
        // SAFETY: every channel holds `frames_count` samples. The main input
        // is read before the output is written, as they may share memory.
        unsafe {
            let mut left = *in_left.add(frame);
            let mut right = *in_right.add(frame);
            for (key, mark) in keys.iter().zip(&marks) {
                let sample = *key.add(frame);
                left += sample * mark[0];
                right += sample * mark[1];
            }
            *out_left.add(frame) = left;
            *out_right.add(frame) = right;
        }
    }
    CLAP_PROCESS_CONTINUE
}

/// Applies a parameter change, from `process` or from `flush`.
///
/// # Safety
/// `header` must be a valid event, and `out` null or a valid output list.
pub(crate) unsafe fn handle_param_event(
    plugin: &Plugin,
    header: *const clap_event_header,
    out: *const clap_output_events,
) {
    // SAFETY: the header is valid by the caller's promise.
    let head = unsafe { &*header };
    if head.space_id != CLAP_CORE_EVENT_SPACE_ID {
        return;
    }
    match head.type_ {
        CLAP_EVENT_PARAM_VALUE => {
            // SAFETY: the type says which struct the header begins.
            let event = unsafe { &*(header as *const clap_event_param_value) };
            if let Some(param) = ext::params_of(plugin.kind)
                .iter()
                .find(|param| param.id == event.param_id)
            {
                plugin.set_value(param.slot, event.value.clamp(param.min, param.max));
            }
        }
        CLAP_EVENT_NOTE_ON if plugin.kind == Kind::Gain => {
            // SAFETY: the type says which struct the header begins.
            let event = unsafe { &*(header as *const clap_event_note) };
            if event.key != GESTURE_KEY {
                return;
            }
            let gain = (event.velocity * 2.0).clamp(0.0, 2.0);
            plugin.set_value(slot::GAIN, gain);
            let begin = clap_event_param_gesture {
                header: self::header::<clap_event_param_gesture>(
                    head.time,
                    CLAP_EVENT_PARAM_GESTURE_BEGIN,
                ),
                param_id: ext::GAIN_ID,
            };
            let value = clap_event_param_value {
                header: self::header::<clap_event_param_value>(head.time, CLAP_EVENT_PARAM_VALUE),
                param_id: ext::GAIN_ID,
                cookie: ptr::null_mut(),
                note_id: -1,
                port_index: -1,
                channel: -1,
                key: -1,
                value: gain,
            };
            let end = clap_event_param_gesture {
                header: self::header::<clap_event_param_gesture>(
                    head.time,
                    CLAP_EVENT_PARAM_GESTURE_END,
                ),
                param_id: ext::GAIN_ID,
            };
            // SAFETY: the output list is valid by the caller's promise.
            unsafe {
                push(out, &begin);
                push(out, &value);
                push(out, &end);
            }
        }
        _ => {}
    }
}

/// # Safety
/// `block` must be the host's valid block.
unsafe fn process_gain(plugin: &Plugin, block: &clap_process) -> clap_process_status {
    // SAFETY: the block's buffers are valid for its counts.
    let (in_left, in_right, out_left, out_right) = unsafe {
        (
            channel(block.audio_inputs, block.audio_inputs_count, 0, 0),
            channel(block.audio_inputs, block.audio_inputs_count, 0, 1),
            channel(block.audio_outputs, block.audio_outputs_count, 0, 0),
            channel(block.audio_outputs, block.audio_outputs_count, 0, 1),
        )
    };
    if in_left.is_null() || in_right.is_null() || out_left.is_null() || out_right.is_null() {
        return CLAP_PROCESS_ERROR;
    }

    plugin.set_value(slot::IN_PLACE, f64::from(u8::from(in_left == out_left)));
    // A multiplication that leaves a denormal, unless the host has told the
    // processor to flush those to zero.
    let tiny = std::hint::black_box(f32::MIN_POSITIVE) * std::hint::black_box(0.5_f32);
    plugin.set_value(slot::DENORMALS_FLUSHED, f64::from(u8::from(tiny == 0.0)));
    if !block.transport.is_null() {
        // SAFETY: a transport the host passes is valid for the call.
        let transport = unsafe { &*block.transport };
        if transport.flags & CLAP_TRANSPORT_HAS_TEMPO != 0 {
            plugin.set_value(slot::TEMPO, transport.tempo);
        }
        if transport.flags & CLAP_TRANSPORT_HAS_BEATS_TIMELINE != 0 {
            let beats = transport.song_pos_beats as f64 / CLAP_BEATTIME_FACTOR as f64;
            plugin.set_value(slot::BEATS, beats);
        }
        let playing = transport.flags & CLAP_TRANSPORT_IS_PLAYING != 0;
        plugin.set_value(slot::PLAYING, f64::from(u8::from(playing)));
    }

    // SAFETY: the event lists are the host's.
    let mut incoming = unsafe { Incoming::new(block.in_events) };
    for frame in 0..block.frames_count {
        // SAFETY: as above.
        while let Some(event) = unsafe { incoming.due(frame) } {
            // SAFETY: the event came from the host's list.
            unsafe { handle_param_event(plugin, event, block.out_events) };
        }
        let mut gain = plugin.value(slot::GAIN) as f32;
        if plugin.value(slot::MODE) >= 0.5 {
            gain = -gain;
        }
        let at = frame as usize;
        // SAFETY: every channel holds `frames_count` samples. Raw pointers
        // are used because input and output may be the same memory.
        unsafe {
            *out_left.add(at) = *in_left.add(at) * gain;
            *out_right.add(at) = *in_right.add(at) * gain;
        }
    }
    CLAP_PROCESS_CONTINUE
}

fn start_voice(audio: &mut Audio, key: i16, channel: i16, note_id: i32, velocity: f64) {
    let free = audio
        .voices
        .iter()
        .position(|voice| !voice.active)
        .unwrap_or(0);
    let frequency = 440.0 * 2.0_f64.powf((f64::from(key) - 69.0) / 12.0);
    audio.voices[free] = Voice {
        active: true,
        key,
        channel,
        note_id,
        phase: 0.0,
        step: std::f64::consts::TAU * frequency / audio.sample_rate,
        amplitude: velocity,
    };
}

/// Ends the voices of `key`, or every voice if it is negative, and tells the
/// host each note has ended.
///
/// # Safety
/// `out` must be null or the host's valid output list.
unsafe fn end_voices(audio: &mut Audio, key: i16, time: u32, out: *const clap_output_events) {
    for voice in &mut audio.voices {
        if voice.active && (key < 0 || voice.key == key) {
            voice.active = false;
            let ended = clap_event_note {
                header: header::<clap_event_note>(time, CLAP_EVENT_NOTE_END),
                note_id: voice.note_id,
                port_index: 0,
                channel: voice.channel,
                key: voice.key,
                velocity: 0.0,
            };
            // SAFETY: the output list is valid by the caller's promise.
            unsafe { push(out, &ended) };
        }
    }
}

/// # Safety
/// `block` must be the host's valid block.
unsafe fn process_sine(
    plugin: &Plugin,
    audio: &mut Audio,
    block: &clap_process,
) -> clap_process_status {
    // SAFETY: the block's buffers are valid for its counts.
    let (out_left, out_right) = unsafe {
        (
            channel(block.audio_outputs, block.audio_outputs_count, 0, 0),
            channel(block.audio_outputs, block.audio_outputs_count, 0, 1),
        )
    };
    if out_left.is_null() || out_right.is_null() {
        return CLAP_PROCESS_ERROR;
    }
    let midi_only = plugin.kind == Kind::MidiSine;

    // SAFETY: the event lists are the host's.
    let mut incoming = unsafe { Incoming::new(block.in_events) };
    for frame in 0..block.frames_count {
        // SAFETY: as above.
        while let Some(event) = unsafe { incoming.due(frame) } {
            // SAFETY: the event came from the host's list.
            let head = unsafe { &*event };
            if head.space_id != CLAP_CORE_EVENT_SPACE_ID {
                continue;
            }
            match head.type_ {
                CLAP_EVENT_NOTE_ON if !midi_only => {
                    // SAFETY: the type says which struct the header begins.
                    let note = unsafe { &*(event as *const clap_event_note) };
                    start_voice(audio, note.key, note.channel, note.note_id, note.velocity);
                }
                CLAP_EVENT_NOTE_OFF | CLAP_EVENT_NOTE_CHOKE if !midi_only => {
                    // SAFETY: the type says which struct the header begins.
                    let note = unsafe { &*(event as *const clap_event_note) };
                    // SAFETY: the output list is the host's.
                    unsafe { end_voices(audio, note.key, head.time, block.out_events) };
                }
                CLAP_EVENT_MIDI if midi_only => {
                    // SAFETY: the type says which struct the header begins.
                    let midi = unsafe { &*(event as *const clap_event_midi) };
                    let [status, key, velocity] = midi.data;
                    let channel = i16::from(status & 0x0f);
                    match status & 0xf0 {
                        0x90 if velocity > 0 => {
                            let level = f64::from(velocity) / 127.0;
                            start_voice(audio, i16::from(key), channel, -1, level);
                        }
                        // SAFETY: the output list is the host's.
                        0x80 | 0x90 => unsafe {
                            end_voices(audio, i16::from(key), head.time, block.out_events);
                        },
                        // All sound off and all notes off.
                        // SAFETY: the output list is the host's.
                        0xb0 if key == 120 || key == 123 => unsafe {
                            end_voices(audio, -1, head.time, block.out_events);
                        },
                        _ => {}
                    }
                }
                // SAFETY: the event came from the host's list.
                _ => unsafe { handle_param_event(plugin, event, block.out_events) },
            }
        }

        let level = plugin.value(slot::LEVEL);
        let mut sample = 0.0_f64;
        let mut sounding = false;
        for voice in audio.voices.iter_mut().filter(|voice| voice.active) {
            sample += voice.phase.sin() * voice.amplitude * level;
            voice.phase += voice.step;
            sounding = true;
        }
        if sounding {
            audio.ringing = SINE_LATENCY;
        } else {
            audio.ringing = audio.ringing.saturating_sub(1);
        }
        let sample = sample as f32;
        let at = frame as usize;
        // SAFETY: both channels hold `frames_count` samples.
        unsafe {
            *out_left.add(at) = audio.delay[0][audio.delay_at];
            *out_right.add(at) = audio.delay[1][audio.delay_at];
        }
        audio.delay[0][audio.delay_at] = sample;
        audio.delay[1][audio.delay_at] = sample;
        audio.delay_at = (audio.delay_at + 1) % SINE_LATENCY;
    }

    if audio.ringing > 0 {
        CLAP_PROCESS_CONTINUE
    } else {
        CLAP_PROCESS_SLEEP
    }
}
