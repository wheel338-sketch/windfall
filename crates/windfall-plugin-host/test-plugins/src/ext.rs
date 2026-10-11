//! The extensions the test plugins offer: ports, parameters, state,
//! latency, tail and timers.

use std::ffi::{CStr, c_char, c_void};
use std::ptr;

use clap_sys::events::{clap_input_events, clap_output_events};
use clap_sys::ext::audio_ports::{
    CLAP_AUDIO_PORT_IS_MAIN, CLAP_EXT_AUDIO_PORTS, CLAP_PORT_MONO, CLAP_PORT_STEREO,
    clap_audio_port_info, clap_plugin_audio_ports,
};
use clap_sys::ext::latency::{CLAP_EXT_LATENCY, clap_plugin_latency};
use clap_sys::ext::note_ports::{
    CLAP_EXT_NOTE_PORTS, CLAP_NOTE_DIALECT_CLAP, CLAP_NOTE_DIALECT_MIDI, clap_note_port_info,
    clap_plugin_note_ports,
};
use clap_sys::ext::params::{
    CLAP_EXT_PARAMS, CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_ENUM, CLAP_PARAM_IS_READONLY,
    CLAP_PARAM_IS_STEPPED, clap_param_info, clap_plugin_params,
};
use clap_sys::ext::state::{CLAP_EXT_STATE, clap_plugin_state};
use clap_sys::ext::tail::{CLAP_EXT_TAIL, clap_plugin_tail};
use clap_sys::ext::timer_support::{CLAP_EXT_TIMER_SUPPORT, clap_plugin_timer_support};
use clap_sys::id::CLAP_INVALID_ID;
use clap_sys::plugin::clap_plugin;
use clap_sys::stream::{clap_istream, clap_ostream};

use crate::Kind;
use crate::plugin::{Plugin, SINE_LATENCY, handle_param_event, slot};

pub(crate) const GAIN_ID: u32 = 7;
const MODE_ID: u32 = 9;

/// The tail the sidechain plugin reports, in samples.
const SIDECHAIN_TAIL: u32 = 4_800;

pub(crate) struct ParamDef {
    pub id: u32,
    pub slot: usize,
    pub name: &'static str,
    pub module: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub flags: u32,
}

const READING: u32 = CLAP_PARAM_IS_READONLY;

const fn reading(id: u32, slot: usize, name: &'static str, max: f64) -> ParamDef {
    ParamDef {
        id,
        slot,
        name,
        module: "Readings",
        min: 0.0,
        max,
        default: 0.0,
        flags: READING,
    }
}

/// The gain plugin's controls, followed by readings a test can check to
/// learn what the host did.
const GAIN_PARAMS: &[ParamDef] = &[
    ParamDef {
        id: GAIN_ID,
        slot: slot::GAIN,
        name: "Gain",
        module: "Main",
        min: 0.0,
        max: 2.0,
        default: 1.0,
        flags: CLAP_PARAM_IS_AUTOMATABLE,
    },
    ParamDef {
        id: MODE_ID,
        slot: slot::MODE,
        name: "Mode",
        module: "Main",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        flags: CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM,
    },
    reading(20, slot::TIMER_TICKS, "Timer ticks", 1.0e9),
    reading(21, slot::MAIN_THREAD_CALLS, "Main thread calls", 1.0e9),
    reading(22, slot::TEMPO, "Tempo seen", 1.0e4),
    reading(23, slot::PLAYING, "Playing seen", 1.0),
    reading(24, slot::BEATS, "Beats seen", 1.0e9),
    reading(25, slot::DENORMALS_FLUSHED, "Denormals flushed", 1.0),
    reading(26, slot::IN_PLACE, "Processed in place", 1.0),
];

const SINE_PARAMS: &[ParamDef] = &[ParamDef {
    id: 1,
    slot: slot::LEVEL,
    name: "Level",
    module: "",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    flags: CLAP_PARAM_IS_AUTOMATABLE,
}];

pub(crate) fn params_of(kind: Kind) -> &'static [ParamDef] {
    match kind {
        Kind::Gain => GAIN_PARAMS,
        Kind::BridgeDelayed
        | Kind::BridgeProcessHang
        | Kind::BridgeIdleHang
        | Kind::BridgeCaptureExit
        | Kind::BridgeCaptureHang
        | Kind::BridgeInvalidStream
        | Kind::BridgeBadLatency
        | Kind::BridgeEventFlood
        | Kind::BridgeIgnoredStreamError
        | Kind::BridgeStateBoundary
        | Kind::BridgeControlledProcessError => &GAIN_PARAMS[..1],
        Kind::Sine | Kind::MidiSine => SINE_PARAMS,
        _ => &[],
    }
}

fn write_text(target: &mut [c_char], text: &str) {
    let room = target.len().saturating_sub(1);
    let bytes = &text.as_bytes()[..text.len().min(room)];
    for (slot, byte) in target.iter_mut().zip(bytes) {
        *slot = *byte as c_char;
    }
    if let Some(end) = target.get_mut(bytes.len()) {
        *end = 0;
    }
}

unsafe extern "C" fn params_count(plugin: *const clap_plugin) -> u32 {
    // SAFETY: the host passes the plugin it was given.
    let kind = unsafe { Plugin::from_raw(plugin) }.kind;
    if kind == Kind::AbsurdPorts {
        return u32::MAX;
    }
    params_of(kind).len() as u32
}

unsafe extern "C" fn params_get_info(
    plugin: *const clap_plugin,
    index: u32,
    info: *mut clap_param_info,
) -> bool {
    // SAFETY: the host passes the plugin it was given and room for the info.
    let (kind, info) = unsafe { (Plugin::from_raw(plugin).kind, &mut *info) };
    if kind == Kind::AbsurdPorts {
        // Every index answers, each with the same id.
        info.id = 1;
        info.flags = 0;
        info.cookie = ptr::null_mut();
        write_text(&mut info.name, "Endless");
        write_text(&mut info.module, "");
        info.min_value = 0.0;
        info.max_value = 1.0;
        info.default_value = 0.0;
        return true;
    }
    let Some(param) = params_of(kind).get(index as usize) else {
        return false;
    };
    info.id = param.id;
    info.flags = param.flags;
    info.cookie = ptr::null_mut();
    write_text(&mut info.name, param.name);
    write_text(&mut info.module, param.module);
    info.min_value = param.min;
    info.max_value = param.max;
    info.default_value = param.default;
    true
}

unsafe extern "C" fn params_get_value(
    plugin: *const clap_plugin,
    id: u32,
    value: *mut f64,
) -> bool {
    // SAFETY: the host passes the plugin it was given.
    let plugin = unsafe { Plugin::from_raw(plugin) };
    let Some(param) = params_of(plugin.kind).iter().find(|param| param.id == id) else {
        return false;
    };
    // SAFETY: the host passes room for one value.
    unsafe { *value = plugin.value(param.slot) };
    true
}

unsafe extern "C" fn params_value_to_text(
    plugin: *const clap_plugin,
    id: u32,
    value: f64,
    buffer: *mut c_char,
    capacity: u32,
) -> bool {
    // SAFETY: the host passes the plugin it was given.
    let kind = unsafe { Plugin::from_raw(plugin) }.kind;
    if buffer.is_null() || !params_of(kind).iter().any(|param| param.id == id) {
        return false;
    }
    let text = match (kind, id) {
        (Kind::Gain, GAIN_ID) if value <= 0.0 => "-inf dB".to_owned(),
        (Kind::Gain, GAIN_ID) => format!("{:.1} dB", 20.0 * value.log10()),
        (Kind::Gain, MODE_ID) if value >= 0.5 => "Invert".to_owned(),
        (Kind::Gain, MODE_ID) => "Normal".to_owned(),
        (Kind::Sine | Kind::MidiSine, _) => format!("{:.0} %", value * 100.0),
        _ => format!("{value}"),
    };
    // SAFETY: the host passes a buffer of `capacity` bytes.
    write_text(
        unsafe { std::slice::from_raw_parts_mut(buffer, capacity as usize) },
        &text,
    );
    true
}

unsafe extern "C" fn params_text_to_value(
    plugin: *const clap_plugin,
    id: u32,
    text: *const c_char,
    value: *mut f64,
) -> bool {
    // SAFETY: the host passes the plugin it was given.
    let kind = unsafe { Plugin::from_raw(plugin) }.kind;
    if text.is_null() || !params_of(kind).iter().any(|param| param.id == id) {
        return false;
    }
    // SAFETY: the host passes a valid C string.
    let text = unsafe { CStr::from_ptr(text) }.to_string_lossy();
    let text = text.trim();
    let number = |unit: &str| text.trim_end_matches(unit).trim().parse::<f64>().ok();
    let parsed = match (kind, id) {
        (Kind::Gain, GAIN_ID) if text.starts_with("-inf") => Some(0.0),
        (Kind::Gain, GAIN_ID) => number("dB").map(|db| 10.0_f64.powf(db / 20.0)),
        (Kind::Gain, MODE_ID) => match text {
            "Normal" => Some(0.0),
            "Invert" => Some(1.0),
            _ => None,
        },
        (Kind::Sine | Kind::MidiSine, _) => number("%").map(|percent| percent / 100.0),
        _ => number(""),
    };
    match parsed {
        Some(parsed) => {
            // SAFETY: the host passes room for one value.
            unsafe { *value = parsed };
            true
        }
        None => false,
    }
}

unsafe extern "C" fn params_flush(
    plugin: *const clap_plugin,
    input: *const clap_input_events,
    output: *const clap_output_events,
) {
    if input.is_null() {
        return;
    }
    // SAFETY: the host passes the plugin it was given and valid lists.
    unsafe {
        let plugin = Plugin::from_raw(plugin);
        let (Some(size), Some(get)) = ((*input).size, (*input).get) else {
            return;
        };
        for index in 0..size(input) {
            let event = get(input, index);
            if !event.is_null() {
                handle_param_event(plugin, event, output);
            }
        }
    }
}

static PARAMS: clap_plugin_params = clap_plugin_params {
    count: Some(params_count),
    get_info: Some(params_get_info),
    get_value: Some(params_get_value),
    value_to_text: Some(params_value_to_text),
    text_to_value: Some(params_text_to_value),
    flush: Some(params_flush),
};

/// The four bytes a plugin's state starts with.
fn state_magic(kind: Kind) -> &'static [u8; 4] {
    match kind {
        Kind::Gain => b"WFTG",
        _ => b"WFTS",
    }
}

/// The slots a plugin saves, in order.
fn state_slots(kind: Kind) -> &'static [usize] {
    match kind {
        Kind::Gain => &[slot::GAIN, slot::MODE],
        _ => &[slot::LEVEL],
    }
}

unsafe extern "C" fn state_save(plugin: *const clap_plugin, stream: *const clap_ostream) -> bool {
    // SAFETY: the host passes the plugin it was given.
    let plugin = unsafe { Plugin::from_raw(plugin) };
    use crate::bridge_behaviors::{CaptureFault, before_capture};
    let fault = match plugin.kind {
        Kind::BridgeCaptureExit => CaptureFault::Exit,
        Kind::BridgeCaptureHang => CaptureFault::Hang,
        Kind::BridgeInvalidStream => CaptureFault::PartialStream,
        _ => CaptureFault::None,
    };
    if !before_capture(fault, plugin.value(slot::GAIN)) {
        let bytes = [0u8; 3];
        if let Some(write) = unsafe { (*stream).write } {
            unsafe {
                write(stream, bytes.as_ptr().cast(), bytes.len() as u64);
            }
        }
        return false; // Refuse the truncated native state, do not claim success.
    }
    let mut bytes = state_magic(plugin.kind).to_vec();
    for &slot in state_slots(plugin.kind) {
        bytes.extend_from_slice(&plugin.value(slot).to_le_bytes());
    }
    // SAFETY: the host passes a valid stream.
    let Some(write) = (unsafe { (*stream).write }) else {
        return false;
    };
    if plugin.kind == Kind::BridgeIgnoredStreamError && plugin.value(slot::GAIN) >= 0.75 {
        // Valid bounded memory, deliberately broken native save semantics:
        // ignore the first failed host write and claim success. A small-limit
        // native unit hits this immediately; the helper hits its real256MiB cap.
        let padding = [0x42u8; 8192];
        for _ in 0..=((256 << 20) / padding.len()) {
            if unsafe { write(stream, padding.as_ptr().cast(), padding.len() as u64) } <= 0 {
                return true;
            }
        }
        return true;
    }
    let mut written = 0;
    while written < bytes.len() {
        let rest = &bytes[written..];
        // SAFETY: `rest` is valid for its length.
        let count = unsafe { write(stream, rest.as_ptr() as *const c_void, rest.len() as u64) };
        if count <= 0 {
            return false;
        }
        written += count as usize;
    }
    if plugin.kind == Kind::BridgeStateBoundary && plugin.value(slot::GAIN) >= 0.75 {
        let padding = [0u8; 8192];
        while written < crate::bridge_behaviors::STATE_LIMIT {
            let count = (crate::bridge_behaviors::STATE_LIMIT - written).min(padding.len());
            if unsafe { write(stream, padding.as_ptr().cast(), count as u64) } != count as i64 {
                return false;
            }
            written += count;
        }
    }
    true
}

unsafe extern "C" fn state_load(plugin: *const clap_plugin, stream: *const clap_istream) -> bool {
    // SAFETY: the host passes the plugin it was given.
    let plugin = unsafe { Plugin::from_raw(plugin) };
    // SAFETY: the host passes a valid stream.
    let Some(read) = (unsafe { (*stream).read }) else {
        return false;
    };
    let slots = state_slots(plugin.kind);
    let mut bytes = vec![0_u8; 4 + slots.len() * 8];
    let mut filled = 0;
    while filled < bytes.len() {
        let rest = &mut bytes[filled..];
        // SAFETY: `rest` is valid for its length.
        let count = unsafe { read(stream, rest.as_mut_ptr() as *mut c_void, rest.len() as u64) };
        if count <= 0 {
            return false;
        }
        filled += count as usize;
    }
    if &bytes[..4] != state_magic(plugin.kind) {
        return false;
    }
    for (&slot, value) in slots.iter().zip(bytes[4..].as_chunks::<8>().0.iter()) {
        let value = f64::from_le_bytes(*value);
        if !value.is_finite() {
            return false;
        }
        plugin.set_value(slot, value);
    }
    true
}

static STATE: clap_plugin_state = clap_plugin_state {
    save: Some(state_save),
    load: Some(state_load),
};

struct Port {
    id: u32,
    name: &'static str,
    channels: u32,
    main: bool,
    in_place: bool,
}

const fn stereo(in_place: bool) -> Port {
    Port {
        id: 0,
        name: "Main",
        channels: 2,
        main: true,
        in_place,
    }
}

const NONE: &[Port] = &[];
const STEREO_IN_PLACE: &[Port] = &[stereo(true)];
const STEREO_SEPARATE: &[Port] = &[stereo(false)];
const MONO_IN_PLACE: &[Port] = &[Port {
    id: 0,
    name: "Main",
    channels: 1,
    main: true,
    in_place: true,
}];
const WITH_EXTRA: &[Port] = &[
    stereo(false),
    Port {
        id: 1,
        name: "Sidechain",
        channels: 1,
        main: false,
        in_place: false,
    },
];

/// A main stereo input followed by a mono, a stereo and a four-channel
/// sidechain input, in that order. See `process_multi_sidechain`.
const WITH_THREE_KEYS: &[Port] = &[
    stereo(false),
    Port {
        id: 1,
        name: "Key Mono",
        channels: 1,
        main: false,
        in_place: false,
    },
    Port {
        id: 2,
        name: "Key Stereo",
        channels: 2,
        main: false,
        in_place: false,
    },
    Port {
        id: 3,
        name: "Key Quad",
        channels: 4,
        main: false,
        in_place: false,
    },
];

fn ports_of(kind: Kind, is_input: bool) -> &'static [Port] {
    match kind {
        Kind::MultiSidechain if is_input => WITH_THREE_KEYS,
        Kind::MultiSidechain => STEREO_SEPARATE,
        Kind::Sine | Kind::MidiSine | Kind::BridgeNoteProbe if is_input => NONE,
        Kind::Sine | Kind::MidiSine | Kind::BridgeNoteProbe | Kind::Swap => STEREO_SEPARATE,
        Kind::Sidechain => WITH_EXTRA,
        Kind::Mono => MONO_IN_PLACE,
        _ => STEREO_IN_PLACE,
    }
}

unsafe extern "C" fn audio_ports_count(plugin: *const clap_plugin, is_input: bool) -> u32 {
    // SAFETY: the host passes the plugin it was given.
    let kind = unsafe { Plugin::from_raw(plugin) }.kind;
    if kind == Kind::AbsurdPorts {
        return u32::MAX;
    }
    ports_of(kind, is_input).len() as u32
}

unsafe extern "C" fn audio_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    // SAFETY: the host passes the plugin it was given and room for the info.
    let (kind, info) = unsafe { (Plugin::from_raw(plugin).kind, &mut *info) };
    if kind == Kind::AbsurdPorts {
        info.id = index;
        write_text(&mut info.name, "Too many");
        info.flags = 0;
        info.channel_count = 1_000_000;
        info.port_type = ptr::null();
        info.in_place_pair = CLAP_INVALID_ID;
        return true;
    }
    let Some(port) = ports_of(kind, is_input).get(index as usize) else {
        return false;
    };
    info.id = port.id;
    write_text(&mut info.name, port.name);
    info.flags = if port.main {
        CLAP_AUDIO_PORT_IS_MAIN
    } else {
        0
    };
    info.channel_count = port.channels;
    info.port_type = match port.channels {
        1 => CLAP_PORT_MONO.as_ptr(),
        2 => CLAP_PORT_STEREO.as_ptr(),
        // CLAP names no type for four plain channels.
        _ => ptr::null(),
    };
    info.in_place_pair = if port.in_place {
        port.id
    } else {
        CLAP_INVALID_ID
    };
    true
}

static AUDIO_PORTS: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(audio_ports_count),
    get: Some(audio_ports_get),
};

unsafe extern "C" fn note_ports_count(plugin: *const clap_plugin, is_input: bool) -> u32 {
    // SAFETY: the host passes the plugin it was given.
    match unsafe { Plugin::from_raw(plugin) }.kind {
        Kind::AbsurdPorts => u32::MAX,
        Kind::Gain | Kind::Sine | Kind::MidiSine | Kind::BridgeNoteProbe if is_input => 1,
        _ => 0,
    }
}

unsafe extern "C" fn note_ports_get(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_note_port_info,
) -> bool {
    // SAFETY: the host passes the plugin it was given and room for the info.
    let (kind, info) = unsafe { (Plugin::from_raw(plugin).kind, &mut *info) };
    let dialects = match kind {
        Kind::AbsurdPorts => CLAP_NOTE_DIALECT_CLAP,
        Kind::Gain | Kind::Sine | Kind::BridgeNoteProbe if is_input && index == 0 => {
            CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI
        }
        Kind::MidiSine if is_input && index == 0 => CLAP_NOTE_DIALECT_MIDI,
        _ => return false,
    };
    info.id = index;
    info.supported_dialects = dialects;
    info.preferred_dialect = if dialects & CLAP_NOTE_DIALECT_CLAP != 0 {
        CLAP_NOTE_DIALECT_CLAP
    } else {
        CLAP_NOTE_DIALECT_MIDI
    };
    write_text(&mut info.name, "Notes");
    true
}

static NOTE_PORTS: clap_plugin_note_ports = clap_plugin_note_ports {
    count: Some(note_ports_count),
    get: Some(note_ports_get),
};

unsafe extern "C" fn latency_get(plugin: *const clap_plugin) -> u32 {
    if unsafe { Plugin::from_raw(plugin) }.kind == Kind::BridgeBadLatency {
        return u32::MAX;
    }
    if matches!(
        unsafe { Plugin::from_raw(plugin) }.kind,
        Kind::BridgeDelayed
            | Kind::BridgeProcessHang
            | Kind::BridgeIdleHang
            | Kind::BridgeCaptureExit
            | Kind::BridgeCaptureHang
            | Kind::BridgeInvalidStream
            | Kind::BridgeBadLatency
            | Kind::BridgeEventFlood
            | Kind::BridgeIgnoredStreamError
            | Kind::BridgeStateBoundary
            | Kind::BridgeControlledProcessError
    ) {
        crate::bridge_behaviors::DELAY as u32
    } else {
        SINE_LATENCY as u32
    }
}

static LATENCY: clap_plugin_latency = clap_plugin_latency {
    get: Some(latency_get),
};

unsafe extern "C" fn tail_get(_plugin: *const clap_plugin) -> u32 {
    SIDECHAIN_TAIL
}

static TAIL: clap_plugin_tail = clap_plugin_tail {
    get: Some(tail_get),
};

unsafe extern "C" fn on_timer(plugin: *const clap_plugin, _timer: u32) {
    // SAFETY: the host passes the plugin it was given, on the main thread.
    unsafe {
        let plugin = Plugin::from_raw(plugin);
        plugin.add_value(slot::TIMER_TICKS, 1.0);
        plugin.main().timer_ticks += 1;
        #[cfg(windows)]
        crate::gui::on_timer(plugin);
    }
}

static TIMER: clap_plugin_timer_support = clap_plugin_timer_support {
    on_timer: Some(on_timer),
};

pub(crate) fn get(kind: Kind, id: &CStr) -> *const c_void {
    let has_params = !params_of(kind).is_empty() || kind == Kind::AbsurdPorts;
    if id == CLAP_EXT_AUDIO_PORTS {
        &raw const AUDIO_PORTS as *const c_void
    } else if id == CLAP_EXT_NOTE_PORTS {
        &raw const NOTE_PORTS as *const c_void
    } else if id == CLAP_EXT_PARAMS && has_params {
        &raw const PARAMS as *const c_void
    } else if id == CLAP_EXT_STATE
        && matches!(
            kind,
            Kind::Gain
                | Kind::Sine
                | Kind::BridgeDelayed
                | Kind::BridgeProcessHang
                | Kind::BridgeIdleHang
                | Kind::BridgeCaptureExit
                | Kind::BridgeCaptureHang
                | Kind::BridgeInvalidStream
                | Kind::BridgeBadLatency
                | Kind::BridgeEventFlood
                | Kind::BridgeIgnoredStreamError
                | Kind::BridgeStateBoundary
                | Kind::BridgeControlledProcessError
        )
    {
        &raw const STATE as *const c_void
    } else if id == CLAP_EXT_LATENCY
        && matches!(
            kind,
            Kind::Sine
                | Kind::MidiSine
                | Kind::BridgeDelayed
                | Kind::BridgeProcessHang
                | Kind::BridgeIdleHang
                | Kind::BridgeCaptureExit
                | Kind::BridgeCaptureHang
                | Kind::BridgeInvalidStream
                | Kind::BridgeBadLatency
                | Kind::BridgeEventFlood
                | Kind::BridgeIgnoredStreamError
                | Kind::BridgeStateBoundary
                | Kind::BridgeControlledProcessError
        )
    {
        &raw const LATENCY as *const c_void
    } else if id == CLAP_EXT_TAIL && kind == Kind::Sidechain {
        &raw const TAIL as *const c_void
    } else if id == CLAP_EXT_TIMER_SUPPORT && kind == Kind::Gain {
        &raw const TIMER as *const c_void
    } else {
        #[cfg(windows)]
        if kind == Kind::Gain {
            return crate::gui::extension(id);
        }
        ptr::null()
    }
}
