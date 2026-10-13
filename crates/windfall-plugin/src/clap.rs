//! Raw CLAP boundary. Host lifecycle/thread rules protect disjoint main/audio
//! cells. All allocation and state decoding happens on the main thread.

use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::{cell::UnsafeCell, rc::Rc};

use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::{
    CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_MIDI, CLAP_EVENT_TRANSPORT,
    CLAP_TRANSPORT_HAS_BEATS_TIMELINE, CLAP_TRANSPORT_HAS_TEMPO, CLAP_TRANSPORT_IS_PLAYING,
    clap_event_midi, clap_event_transport,
};
use clap_sys::ext::audio_ports::{
    CLAP_AUDIO_PORT_IS_MAIN, CLAP_EXT_AUDIO_PORTS, CLAP_PORT_STEREO, clap_audio_port_info,
    clap_plugin_audio_ports,
};
use clap_sys::ext::latency::{CLAP_EXT_LATENCY, clap_plugin_latency};
use clap_sys::ext::note_ports::{
    CLAP_EXT_NOTE_PORTS, CLAP_NOTE_DIALECT_MIDI, clap_note_port_info, clap_plugin_note_ports,
};
use clap_sys::ext::state::{CLAP_EXT_STATE, clap_plugin_state};
use clap_sys::factory::plugin_factory::{CLAP_PLUGIN_FACTORY_ID, clap_plugin_factory};
use clap_sys::fixedpoint::CLAP_BEATTIME_FACTOR;
use clap_sys::host::clap_host;
use clap_sys::id::CLAP_INVALID_ID;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::{
    CLAP_PROCESS_CONTINUE, CLAP_PROCESS_ERROR, clap_process, clap_process_status,
};
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::CLAP_VERSION;

#[cfg(test)]
use crate::GuestEngine;
use crate::{MAX_HOST_FRAMES, MAX_STATE_BYTES, OUTPUT_CHANNELS, OUTPUT_PORTS, Program};

const ID: &CStr = c"org.windfall.program";
const MAX_EVENTS: u32 = 4096;

struct Descriptor(clap_plugin_descriptor);
// SAFETY: all C pointers refer to immutable static strings or a permanently
// retained feature list. CLAP hosts never mutate descriptors.
unsafe impl Send for Descriptor {}
unsafe impl Sync for Descriptor {}
static DESCRIPTOR: OnceLock<Descriptor> = OnceLock::new();
fn descriptor() -> &'static clap_plugin_descriptor {
    &DESCRIPTOR
        .get_or_init(|| {
            Descriptor(clap_plugin_descriptor {
                clap_version: CLAP_VERSION,
                id: ID.as_ptr(),
                name: c"Windfall".as_ptr(),
                vendor: c"Windfall".as_ptr(),
                url: c"https://github.com/erivgout/windfall".as_ptr(),
                manual_url: c"".as_ptr(),
                support_url: c"".as_ptr(),
                version: c"0.1.0-alpha.1".as_ptr(),
                description: c"Windfall project engine with host transport and 16 stereo outputs"
                    .as_ptr(),
                features: Box::leak(
                    vec![c"instrument".as_ptr(), c"stereo".as_ptr(), ptr::null()]
                        .into_boxed_slice(),
                )
                .as_ptr(),
            })
        })
        .0
}

struct Instance {
    clap: clap_plugin,
    state: Rc<crate::publication::Publication>,
    gui: UnsafeCell<gui::Editor>,
    host: *const clap_host,
    active: AtomicBool,
    processing: AtomicBool,
    max_frames: AtomicU32,
    latency: AtomicU32,
}

impl Instance {
    // SAFETY: only for a live pointer returned by factory_create.
    unsafe fn get<'a>(plugin: *const clap_plugin) -> Option<&'a Self> {
        if plugin.is_null() {
            return None;
        }
        unsafe { ((*plugin).plugin_data as *const Self).as_ref() }
    }
}

unsafe extern "C" fn count(_: *const clap_plugin_factory) -> u32 {
    1
}
unsafe extern "C" fn get_descriptor(
    _: *const clap_plugin_factory,
    index: u32,
) -> *const clap_plugin_descriptor {
    if index == 0 {
        descriptor()
    } else {
        ptr::null()
    }
}
unsafe extern "C" fn create(
    _: *const clap_plugin_factory,
    host: *const clap_host,
    id: *const c_char,
) -> *const clap_plugin {
    if host.is_null() || id.is_null() {
        return ptr::null();
    }
    if unsafe { CStr::from_ptr(id) } != ID
        || unsafe { (*host).clap_version.major } != CLAP_VERSION.major
    {
        return ptr::null();
    }
    crate::runtime::error("");
    let program = match crate::package::initial_program() {
        Ok(program) => program,
        Err(error) => {
            report(host, &error);
            return ptr::null();
        }
    };
    let plugin = Box::new(Instance {
        clap: clap_plugin {
            desc: descriptor(),
            plugin_data: ptr::null_mut(),
            init: Some(init),
            destroy: Some(destroy),
            activate: Some(activate),
            deactivate: Some(deactivate),
            start_processing: Some(start),
            stop_processing: Some(stop),
            reset: Some(reset),
            process: Some(process),
            get_extension: Some(extension),
            on_main_thread: Some(on_main_thread),
        },
        state: Rc::new(crate::publication::Publication::new(program)),
        gui: UnsafeCell::new(Default::default()),
        host,
        active: AtomicBool::new(false),
        processing: AtomicBool::new(false),
        max_frames: AtomicU32::new(0),
        latency: AtomicU32::new(0),
    });
    let raw = Box::into_raw(plugin);
    unsafe {
        (*raw).state.set_notifier(Some(Box::new(move |flags| {
            // Main-thread editor/state preparation only. The host outlives its plugin.
            if let Some(get) = (*host).get_extension {
                if flags & crate::publication::DIRTY != 0
                    && let Some(state) = get(host, CLAP_EXT_STATE.as_ptr())
                        .cast::<clap_sys::ext::state::clap_host_state>()
                        .as_ref()
                    && let Some(mark) = state.mark_dirty
                {
                    mark(host);
                }
                if flags & crate::publication::PARAMETERS != 0
                    && let Some(params) = get(host, clap_sys::ext::params::CLAP_EXT_PARAMS.as_ptr())
                        .cast::<clap_sys::ext::params::clap_host_params>()
                        .as_ref()
                    && let Some(rescan) = params.rescan
                {
                    rescan(host, clap_sys::ext::params::CLAP_PARAM_RESCAN_VALUES);
                }
            }
            if flags & crate::publication::RESTART != 0
                && let Some(restart) = (*host).request_restart
            {
                restart(host);
            }
        })));
    }
    unsafe {
        (*raw).clap.plugin_data = raw.cast();
        &raw const (*raw).clap
    }
}
static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(count),
    get_plugin_descriptor: Some(get_descriptor),
    create_plugin: Some(create),
};
unsafe extern "C" fn entry_init(_: *const c_char) -> bool {
    true
}
unsafe extern "C" fn entry_deinit() {}
unsafe extern "C" fn get_factory(id: *const c_char) -> *const c_void {
    if !id.is_null() && unsafe { CStr::from_ptr(id) } == CLAP_PLUGIN_FACTORY_ID {
        (&raw const FACTORY).cast()
    } else {
        ptr::null()
    }
}
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION,
    init: Some(entry_init),
    deinit: Some(entry_deinit),
    get_factory: Some(get_factory),
};

unsafe extern "C" fn init(plugin: *const clap_plugin) -> bool {
    unsafe { Instance::get(plugin) }.is_some()
}
fn report(host: *const clap_host, message: &str) {
    crate::runtime::error(message);
    let Ok(message) = std::ffi::CString::new(message) else {
        return;
    };
    if let Some(get) = unsafe { (*host).get_extension }
        && let Some(log) = unsafe {
            get(host, clap_sys::ext::log::CLAP_EXT_LOG.as_ptr())
                .cast::<clap_sys::ext::log::clap_host_log>()
                .as_ref()
        }
        && let Some(log) = log.log
    {
        unsafe {
            log(host, clap_sys::ext::log::CLAP_LOG_ERROR, message.as_ptr());
        }
    }
}
unsafe extern "C" fn destroy(plugin: *const clap_plugin) {
    if let Some(instance) = unsafe { Instance::get(plugin) } {
        // CLAP requires deactivation before destruction. Refuse invalid lifecycle
        // rather than dropping live processors under another thread.
        if !instance.active.load(Ordering::Acquire) {
            #[cfg(target_os = "linux")]
            unsafe {
                gui::destroy(plugin);
            }
            unsafe {
                drop(Box::from_raw((*plugin).plugin_data as *mut Instance));
            }
        }
    }
}
unsafe extern "C" fn activate(plugin: *const clap_plugin, rate: f64, min: u32, max: u32) -> bool {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else {
        return false;
    };
    if instance.active.load(Ordering::Acquire) || min == 0 || max < min || max > MAX_HOST_FRAMES {
        return false;
    }
    // Main-thread only, not active; contain preparation panics at the FFI boundary.
    let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        instance.state.activate(rate)
    }));
    let latency = match prepared {
        Ok(Ok(latency)) => latency,
        Ok(Err(error)) => {
            report(instance.host, &error);
            return false;
        }
        Err(_) => {
            report(instance.host, "Windfall program preparation panicked");
            return false;
        }
    };
    instance.latency.store(latency, Ordering::Release);
    if let Some(message) = instance.state.preparation_error() {
        report(instance.host, message);
    }
    // CLAP latency.changed is permitted while inactive, not after an active
    // audio-boundary swap. Active loads requested restart during preparation.
    if let Some(get) = unsafe { (*instance.host).get_extension }
        && let Some(ext) = unsafe {
            get(instance.host, CLAP_EXT_LATENCY.as_ptr())
                .cast::<clap_sys::ext::latency::clap_host_latency>()
                .as_ref()
        }
        && let Some(changed) = ext.changed
    {
        unsafe {
            changed(instance.host);
        }
    }
    instance.max_frames.store(max, Ordering::Release);
    instance.active.store(true, Ordering::Release);
    true
}
unsafe extern "C" fn deactivate(plugin: *const clap_plugin) {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else {
        return;
    };
    if instance.processing.load(Ordering::Acquire) {
        return;
    }
    // Main-thread deactivation is after every audio callback has returned.
    instance.state.deactivate();
    instance.active.store(false, Ordering::Release);
    instance.latency.store(0, Ordering::Release);
}
unsafe extern "C" fn start(plugin: *const clap_plugin) -> bool {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else {
        return false;
    };
    instance.active.load(Ordering::Acquire) && !instance.processing.swap(true, Ordering::AcqRel)
}
unsafe extern "C" fn stop(plugin: *const clap_plugin) {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else {
        return;
    };
    instance.processing.store(false, Ordering::Release);
}
unsafe extern "C" fn reset(plugin: *const clap_plugin) {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else {
        return;
    };
    if let Some(engine) = unsafe { instance.state.audio() } {
        engine.reset();
    }
}
unsafe extern "C" fn on_main_thread(plugin: *const clap_plugin) {
    if let Some(instance) = unsafe { Instance::get(plugin) } {
        instance.state.collect();
    }
}

#[derive(Clone, Copy)]
struct Clock {
    beats: f64,
    tempo: f64,
    playing: bool,
    numerator: u16,
    denominator: u16,
    tempo_inc: f64,
}
fn clock(transport: &clap_event_transport) -> Option<Clock> {
    if transport.header.size < size_of::<clap_event_transport>() as u32
        || transport.flags & (CLAP_TRANSPORT_HAS_BEATS_TIMELINE | CLAP_TRANSPORT_HAS_TEMPO)
            != (CLAP_TRANSPORT_HAS_BEATS_TIMELINE | CLAP_TRANSPORT_HAS_TEMPO)
        || !transport.tempo.is_finite()
        || !(windfall_project::MIN_TEMPO_BPM..=windfall_project::MAX_TEMPO_BPM)
            .contains(&transport.tempo)
        || !transport.tempo_inc.is_finite()
    {
        return None;
    }
    Some(Clock {
        beats: transport.song_pos_beats as f64 / CLAP_BEATTIME_FACTOR as f64,
        tempo: transport.tempo,
        playing: transport.flags & CLAP_TRANSPORT_IS_PLAYING != 0,
        numerator: if transport.flags & clap_sys::events::CLAP_TRANSPORT_HAS_TIME_SIGNATURE != 0 {
            transport.tsig_num
        } else {
            0
        },
        denominator: if transport.flags & clap_sys::events::CLAP_TRANSPORT_HAS_TIME_SIGNATURE != 0 {
            transport.tsig_denom
        } else {
            0
        },
        tempo_inc: transport.tempo_inc,
    })
}

unsafe extern "C" fn process(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> clap_process_status {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else {
        return CLAP_PROCESS_ERROR;
    };
    let Some(block) = (unsafe { process.as_ref() }) else {
        return CLAP_PROCESS_ERROR;
    };
    if !instance.processing.load(Ordering::Acquire)
        || block.frames_count > instance.max_frames.load(Ordering::Acquire)
        || block.audio_inputs_count != 0
        || block.audio_outputs_count != OUTPUT_PORTS as u32
        || block.audio_outputs.is_null()
    {
        return CLAP_PROCESS_ERROR;
    }
    // Validate all output views before touching any PCM. F64 is not advertised.
    let outputs = unsafe { std::slice::from_raw_parts_mut(block.audio_outputs, OUTPUT_PORTS) };
    for output in outputs.iter() {
        if output.channel_count != 2
            || output.data32.is_null()
            || unsafe { (*output.data32).is_null() || (*output.data32.add(1)).is_null() }
        {
            return CLAP_PROCESS_ERROR;
        }
    }
    for output in outputs.iter_mut() {
        output.constant_mask = 0;
        for channel in 0..2 {
            unsafe {
                std::slice::from_raw_parts_mut(
                    *output.data32.add(channel),
                    block.frames_count as usize,
                )
            }
            .fill(0.0);
        }
    }
    let Some(engine) = (unsafe { instance.state.audio() }) else {
        return CLAP_PROCESS_ERROR;
    };
    if instance.state.needs_collection()
        && let Some(request) = unsafe { (*instance.host).request_callback }
    {
        unsafe {
            request(instance.host);
        }
    }
    let mut transport = if block.transport.is_null() {
        None
    } else {
        let Some(clock) = clock(unsafe { &*block.transport }) else {
            return CLAP_PROCESS_ERROR;
        };
        Some(clock)
    };
    let mut event_count = 0;
    let mut get = None;
    if let Some(events) = unsafe { block.in_events.as_ref() } {
        let (Some(size), Some(reader)) = (events.size, events.get) else {
            return CLAP_PROCESS_ERROR;
        };
        event_count = unsafe { size(block.in_events) };
        if event_count > MAX_EVENTS {
            return CLAP_PROCESS_ERROR;
        }
        get = Some(reader);
        let mut previous = 0;
        for index in 0..event_count {
            let event = unsafe { reader(block.in_events, index) };
            let Some(header) = (unsafe { event.as_ref() }) else {
                return CLAP_PROCESS_ERROR;
            };
            if header.size < size_of_val(header) as u32
                || header.time < previous
                || header.time >= block.frames_count
            {
                return CLAP_PROCESS_ERROR;
            }
            previous = header.time;
            if header.space_id == CLAP_CORE_EVENT_SPACE_ID {
                if header.type_ == CLAP_EVENT_MIDI {
                    if header.size < size_of::<clap_event_midi>() as u32 {
                        return CLAP_PROCESS_ERROR;
                    }
                    let midi = unsafe { &*event.cast::<clap_event_midi>() };
                    if midi.port_index != 0 || midi.data[1] >= 128 || midi.data[2] >= 128 {
                        return CLAP_PROCESS_ERROR;
                    }
                } else if header.type_ == CLAP_EVENT_TRANSPORT
                    && (header.size < size_of::<clap_event_transport>() as u32
                        || clock(unsafe { &*event.cast::<clap_event_transport>() }).is_none())
                {
                    return CLAP_PROCESS_ERROR;
                }
            }
        }
    }
    let mut frame = 0usize;
    let mut event_index = 0;
    while frame < block.frames_count as usize {
        if let Some(clock) = transport
            && (!engine.sync_transport(clock.beats, clock.tempo, clock.playing)
                || !engine.sync_meter(clock.numerator, clock.denominator))
        {
            return CLAP_PROCESS_ERROR;
        }
        let mut next = block.frames_count as usize;
        while event_index < event_count {
            let event =
                unsafe { get.expect("validated event reader")(block.in_events, event_index) };
            let header = unsafe { &*event };
            if header.time as usize > frame {
                next = header.time as usize;
                break;
            }
            if header.space_id == CLAP_CORE_EVENT_SPACE_ID {
                if header.type_ == CLAP_EVENT_MIDI {
                    let midi = unsafe { &*event.cast::<clap_event_midi>() };
                    let status = midi.data[0] & 0xf0;
                    if status == 0x90 || status == 0x80 {
                        engine.midi(
                            midi.data[0] & 15,
                            midi.data[1],
                            if status == 0x80 { 0 } else { midi.data[2] },
                        );
                    } else if status == 0xb0 && (midi.data[1] == 120 || midi.data[1] == 123) {
                        for key in 0..128 {
                            engine.midi(midi.data[0] & 15, key, 0);
                        }
                    }
                } else if header.type_ == CLAP_EVENT_TRANSPORT {
                    transport = clock(unsafe { &*event.cast::<clap_event_transport>() });
                    if let Some(clock) = transport
                        && (!engine.sync_transport(clock.beats, clock.tempo, clock.playing)
                            || !engine.sync_meter(clock.numerator, clock.denominator))
                    {
                        return CLAP_PROCESS_ERROR;
                    }
                }
            }
            event_index += 1;
        }
        let mut frames = (next - frame).min(windfall_engine::Processor::plugin_block_frames());
        if transport.is_some_and(|clock| clock.tempo_inc != 0.0) {
            frames = frames.min(1);
        }
        if let Some(clock) = transport.filter(|clock| clock.playing && clock.beats < 0.0) {
            let preroll =
                (-clock.beats * 60.0 * f64::from(engine.sample_rate()) / clock.tempo).ceil();
            frames = frames.min((preroll as usize).max(1));
        }
        let rate = engine.sample_rate();
        let Some(pcm) = engine.render(frames) else {
            return CLAP_PROCESS_ERROR;
        };
        if pcm.len() != frames * OUTPUT_CHANNELS {
            return CLAP_PROCESS_ERROR;
        }
        for (port, output) in outputs.iter_mut().enumerate() {
            for channel in 0..2 {
                let data = unsafe {
                    std::slice::from_raw_parts_mut(
                        *output.data32.add(channel),
                        block.frames_count as usize,
                    )
                };
                for offset in 0..frames {
                    data[frame + offset] = pcm[offset * OUTPUT_CHANNELS + port * 2 + channel];
                }
            }
        }
        if let Some(clock) = &mut transport
            && clock.playing
        {
            clock.beats += (frames as f64 * clock.tempo
                + clock.tempo_inc * frames as f64 * (frames as f64 - 1.0) * 0.5)
                / (60.0 * f64::from(rate));
            clock.tempo += frames as f64 * clock.tempo_inc;
        }
        frame += frames;
    }
    CLAP_PROCESS_CONTINUE
}

unsafe extern "C" fn audio_count(_: *const clap_plugin, input: bool) -> u32 {
    if input { 0 } else { OUTPUT_PORTS as u32 }
}
fn text(target: &mut [c_char], value: &str) {
    target.fill(0);
    let capacity = target.len().saturating_sub(1);
    for (byte, out) in value.bytes().zip(target.iter_mut().take(capacity)) {
        *out = byte as c_char;
    }
}
unsafe extern "C" fn audio_get(
    _: *const clap_plugin,
    index: u32,
    input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if input || index >= OUTPUT_PORTS as u32 || info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = index;
    info.flags = if index == 0 {
        CLAP_AUDIO_PORT_IS_MAIN
    } else {
        0
    };
    text(&mut info.name, &format!("Output {}", index + 1));
    info.channel_count = 2;
    info.port_type = CLAP_PORT_STEREO.as_ptr();
    info.in_place_pair = CLAP_INVALID_ID;
    true
}
static AUDIO: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(audio_count),
    get: Some(audio_get),
};
unsafe extern "C" fn note_count(_: *const clap_plugin, input: bool) -> u32 {
    u32::from(input)
}
unsafe extern "C" fn note_get(
    _: *const clap_plugin,
    index: u32,
    input: bool,
    info: *mut clap_note_port_info,
) -> bool {
    if !input || index != 0 || info.is_null() {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = 0;
    info.supported_dialects = CLAP_NOTE_DIALECT_MIDI;
    info.preferred_dialect = CLAP_NOTE_DIALECT_MIDI;
    text(&mut info.name, "Rack MIDI");
    true
}
static NOTES: clap_plugin_note_ports = clap_plugin_note_ports {
    count: Some(note_count),
    get: Some(note_get),
};
unsafe extern "C" fn latency(plugin: *const clap_plugin) -> u32 {
    unsafe { Instance::get(plugin) }.map_or(0, |p| p.state.latency())
}
static LATENCY: clap_plugin_latency = clap_plugin_latency { get: Some(latency) };

unsafe extern "C" fn save(plugin: *const clap_plugin, stream: *const clap_ostream) -> bool {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else {
        return false;
    };
    let Some(stream_ref) = (unsafe { stream.as_ref() }) else {
        return false;
    };
    let Some(write) = stream_ref.write else {
        return false;
    };
    let bytes = match instance.state.capture() {
        Ok(bytes) => bytes,
        Err(error) => {
            report(instance.host, &error);
            return false;
        }
    };
    let mut offset = 0;
    while offset < bytes.len() {
        let left = &bytes[offset..];
        let count = unsafe { write(stream, left.as_ptr().cast(), left.len() as u64) };
        if count <= 0 || count as usize > left.len() {
            return false;
        }
        offset += count as usize;
    }
    true
}
unsafe extern "C" fn load(plugin: *const clap_plugin, stream: *const clap_istream) -> bool {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else {
        return false;
    };
    let Some(stream_ref) = (unsafe { stream.as_ref() }) else {
        return false;
    };
    let Some(read) = stream_ref.read else {
        return false;
    };
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let count = unsafe { read(stream, chunk.as_mut_ptr().cast(), chunk.len() as u64) };
        if count < 0 || count as usize > chunk.len() {
            return false;
        }
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count as usize) > MAX_STATE_BYTES {
            return false;
        }
        bytes.extend_from_slice(&chunk[..count as usize]);
    }
    match Program::decode(&bytes).and_then(|program| instance.state.load(program)) {
        Ok(()) => {
            if let Some(message) = instance.state.preparation_error() {
                report(instance.host, message);
            }
            true
        }
        Err(error) => {
            report(instance.host, &error);
            false
        }
    }
}
static STATE: clap_plugin_state = clap_plugin_state {
    save: Some(save),
    load: Some(load),
};
unsafe extern "C" fn hard_realtime(plugin: *const clap_plugin) -> bool {
    unsafe { Instance::get(plugin) }.is_some_and(|p| p.state.hard_realtime())
}
unsafe extern "C" fn render_mode(plugin: *const clap_plugin, mode: i32) -> bool {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else {
        return false;
    };
    if !(0..=1).contains(&mode) || instance.active.load(Ordering::Acquire) {
        return false;
    }
    match instance.state.set_offline(mode == 1) {
        Ok(()) => true,
        Err(error) => {
            report(instance.host, &error);
            false
        }
    }
}
static RENDER: clap_sys::ext::render::clap_plugin_render =
    clap_sys::ext::render::clap_plugin_render {
        has_hard_realtime_requirement: Some(hard_realtime),
        set: Some(render_mode),
    };
unsafe extern "C" fn extension(_: *const clap_plugin, id: *const c_char) -> *const c_void {
    if id.is_null() {
        return ptr::null();
    }
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_AUDIO_PORTS {
        (&raw const AUDIO).cast()
    } else if id == CLAP_EXT_NOTE_PORTS {
        (&raw const NOTES).cast()
    } else if id == CLAP_EXT_LATENCY {
        (&raw const LATENCY).cast()
    } else if id == CLAP_EXT_STATE {
        (&raw const STATE).cast()
    } else if id == clap_sys::ext::render::CLAP_EXT_RENDER {
        (&raw const RENDER).cast()
    } else {
        gui::extension(id)
    }
}

mod gui;
#[cfg(test)]
pub(crate) mod tests;
