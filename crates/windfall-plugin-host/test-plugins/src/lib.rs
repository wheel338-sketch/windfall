//! CLAP plugins for the plugin host's tests, all in one library.
//!
//! The well-behaved ones are a gain effect, a sine instrument and a few
//! effects with unusual port layouts. The others fail in one chosen way
//! each, so the host's containment can be tested.
//!
//! Two failures happen before any plugin exists, when the host loads the
//! file. They are switched on by the file's name, so a test copies this one
//! library under another name to get them:
//!
//! - a name containing `crash-on-load` crashes in the entry's `init`,
//! - a name containing `hang-on-load` never returns from it,
//! - a name containing `nasty` adds the plugins that hang or crash in their
//!   own `init`, which would otherwise slow down every scan of the file.

mod bridge_behaviors;
mod ext;
#[cfg(windows)]
mod gui;
mod plugin;
mod vst3;

use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::OnceLock;

use clap_sys::entry::clap_plugin_entry;
use clap_sys::factory::plugin_factory::{CLAP_PLUGIN_FACTORY_ID, clap_plugin_factory};
use clap_sys::host::clap_host;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::version::CLAP_VERSION;

/// What a plugin of this library does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Stereo gain with parameters, state and an editor. Works in place.
    Gain,
    /// Truthful stereo37-frame delay; original gain fixture stays undelayed.
    BridgeDelayed,
    BridgeProcessHang,
    BridgeIdleHang,
    BridgeNoteProbe,
    BridgeCaptureExit,
    BridgeCaptureHang,
    BridgeInvalidStream,
    BridgeBadLatency,
    BridgeEventFlood,
    BridgeIgnoredStreamError,
    BridgeStateBoundary,
    BridgeControlledProcessError,
    /// Sine instrument that takes CLAP notes and reports latency.
    Sine,
    /// The same instrument with a note port that only speaks MIDI.
    MidiSine,
    /// Swaps left and right. Wrong if its input and output share memory.
    Swap,
    /// Stereo effect with a mono sidechain input and a mono extra output.
    Sidechain,
    /// Stereo effect with three sidechain inputs: one mono, one stereo and
    /// one with four channels. Its output says which of them was fed.
    MultiSidechain,
    /// One channel in, one channel out, at half the level.
    Mono,
    /// Writes values that are not numbers.
    Nan,
    /// Reports an error from its third block on.
    ProcessError,
    /// Panics in `process`, which ends the process it is loaded in.
    ProcessPanic,
    /// Claims four billion ports and parameters.
    AbsurdPorts,
    /// Takes 30 ms for every block.
    Slow,
    /// Its `init` returns false.
    InitFail,
    /// Its `init` never returns.
    InitHang,
    /// Its `init` ends the process.
    InitCrash,
    Dynamic,
}

struct Spec {
    kind: Kind,
    id: &'static CStr,
    name: &'static CStr,
    features: &'static [&'static CStr],
}

const EFFECT: &[&CStr] = &[c"audio-effect", c"utility", c"stereo"];
const INSTRUMENT: &[&CStr] = &[c"instrument", c"synthesizer", c"stereo"];

const SPECS: &[Spec] = &[
    Spec {
        kind: Kind::Gain,
        id: c"org.windfall.test.gain",
        name: c"Test Gain",
        features: EFFECT,
    },
    Spec {
        kind: Kind::Sine,
        id: c"org.windfall.test.sine",
        name: c"Test Sine",
        features: INSTRUMENT,
    },
    Spec {
        kind: Kind::MidiSine,
        id: c"org.windfall.test.midi-sine",
        name: c"Test MIDI Sine",
        features: INSTRUMENT,
    },
    Spec {
        kind: Kind::Swap,
        id: c"org.windfall.test.swap",
        name: c"Test Swap",
        features: EFFECT,
    },
    Spec {
        kind: Kind::Sidechain,
        id: c"org.windfall.test.sidechain",
        name: c"Test Sidechain",
        features: EFFECT,
    },
    Spec {
        kind: Kind::Mono,
        id: c"org.windfall.test.mono",
        name: c"Test Mono",
        features: &[c"audio-effect", c"mono"],
    },
    Spec {
        kind: Kind::Nan,
        id: c"org.windfall.test.nan",
        name: c"Test NaN",
        features: EFFECT,
    },
    Spec {
        kind: Kind::ProcessError,
        id: c"org.windfall.test.process-error",
        name: c"Test Process Error",
        features: EFFECT,
    },
    Spec {
        kind: Kind::ProcessPanic,
        id: c"org.windfall.test.process-panic",
        name: c"Test Process Panic",
        features: EFFECT,
    },
    Spec {
        kind: Kind::AbsurdPorts,
        id: c"org.windfall.test.absurd-ports",
        name: c"Test Absurd Ports",
        features: EFFECT,
    },
    Spec {
        kind: Kind::Slow,
        id: c"org.windfall.test.slow",
        name: c"Test Slow",
        features: EFFECT,
    },
    Spec {
        kind: Kind::InitFail,
        id: c"org.windfall.test.init-fail",
        name: c"Test Init Fail",
        features: EFFECT,
    },
    Spec {
        kind: Kind::InitHang,
        id: c"org.windfall.test.init-hang",
        name: c"Test Init Hang",
        features: EFFECT,
    },
    Spec {
        kind: Kind::InitCrash,
        id: c"org.windfall.test.init-crash",
        name: c"Test Init Crash",
        features: EFFECT,
    },
    Spec {
        kind: Kind::BridgeDelayed,
        id: c"org.windfall.test.bridge-delayed",
        name: c"Test Bridge Delayed Effect",
        features: EFFECT,
    },
    Spec {
        kind: Kind::BridgeProcessHang,
        id: c"org.windfall.test.bridge-process-hang",
        name: c"Test Bridge Permanent Process Hang",
        features: EFFECT,
    },
    Spec {
        kind: Kind::BridgeIdleHang,
        id: c"org.windfall.test.bridge-idle-hang",
        name: c"Test Bridge Permanent Main Thread Hang",
        features: EFFECT,
    },
    Spec {
        kind: Kind::BridgeNoteProbe,
        id: c"org.windfall.test.bridge-note-probe",
        name: c"Test Bridge Held Key Probe",
        features: INSTRUMENT,
    },
    Spec {
        kind: Kind::BridgeCaptureExit,
        id: c"org.windfall.test.bridge-capture-exit",
        name: c"Test Bridge Capture Exit",
        features: EFFECT,
    },
    Spec {
        kind: Kind::BridgeCaptureHang,
        id: c"org.windfall.test.bridge-capture-hang",
        name: c"Test Bridge Capture Hang",
        features: EFFECT,
    },
    Spec {
        kind: Kind::BridgeInvalidStream,
        id: c"org.windfall.test.bridge-invalid-stream",
        name: c"Test Bridge Partial State Failure",
        features: EFFECT,
    },
    Spec {
        kind: Kind::BridgeBadLatency,
        id: c"org.windfall.test.bridge-bad-latency",
        name: c"Test Bridge Unsupported Latency",
        features: EFFECT,
    },
    Spec {
        kind: Kind::BridgeEventFlood,
        id: c"org.windfall.test.bridge-event-flood",
        name: c"Test Bridge Native Event Flood",
        features: EFFECT,
    },
    Spec {
        kind: Kind::BridgeIgnoredStreamError,
        id: c"org.windfall.test.bridge-ignored-stream-error",
        name: c"Test Bridge Ignored State Write Failure",
        features: EFFECT,
    },
    Spec {
        kind: Kind::BridgeStateBoundary,
        id: c"org.windfall.test.bridge-state-boundary",
        name: c"Test Bridge Successful Native State Limit",
        features: EFFECT,
    },
    Spec {
        kind: Kind::BridgeControlledProcessError,
        id: c"org.windfall.test.bridge-controlled-process-error",
        name: c"Test Bridge Controlled Process Error",
        features: EFFECT,
    },
    // Appended last, so the positions of the plugins above do not move.
    Spec {
        kind: Kind::MultiSidechain,
        id: c"org.windfall.test.multi-sidechain",
        name: c"Test Multi Sidechain",
        features: EFFECT,
    },
    Spec {
        kind: Kind::Dynamic,
        id: c"org.windfall.test.dynamic",
        name: c"Test Dynamic Capabilities",
        features: EFFECT,
    },
];

/// The file name the host loaded this library under, in lower case.
static FILE_NAME: OnceLock<String> = OnceLock::new();

fn file_name_contains(part: &str) -> bool {
    FILE_NAME.get().is_some_and(|name| name.contains(part))
}

fn visible(spec: &Spec) -> bool {
    match spec.kind {
        Kind::InitHang | Kind::InitCrash => file_name_contains("nasty"),
        _ => true,
    }
}

struct Descriptors(Vec<clap_plugin_descriptor>);

// SAFETY: the descriptors point at statics and at leaked, never-changed
// feature lists, so sharing them between threads is sound.
unsafe impl Send for Descriptors {}
// SAFETY: as above.
unsafe impl Sync for Descriptors {}

static DESCRIPTORS: OnceLock<Descriptors> = OnceLock::new();

fn descriptors() -> &'static [clap_plugin_descriptor] {
    &DESCRIPTORS
        .get_or_init(|| {
            let list = SPECS
                .iter()
                .filter(|spec| visible(spec))
                .map(|spec| {
                    let mut features: Vec<*const c_char> = spec
                        .features
                        .iter()
                        .map(|feature| feature.as_ptr())
                        .collect();
                    features.push(ptr::null());
                    clap_plugin_descriptor {
                        clap_version: CLAP_VERSION,
                        id: spec.id.as_ptr(),
                        name: spec.name.as_ptr(),
                        vendor: c"Windfall tests".as_ptr(),
                        url: c"".as_ptr(),
                        manual_url: c"".as_ptr(),
                        support_url: c"".as_ptr(),
                        version: c"1.2.3".as_ptr(),
                        description: c"A plugin for testing hosts".as_ptr(),
                        features: Box::leak(features.into_boxed_slice()).as_ptr(),
                    }
                })
                .collect();
            Descriptors(list)
        })
        .0
}

unsafe extern "C" fn factory_count(_factory: *const clap_plugin_factory) -> u32 {
    descriptors().len() as u32
}

unsafe extern "C" fn factory_descriptor(
    _factory: *const clap_plugin_factory,
    index: u32,
) -> *const clap_plugin_descriptor {
    descriptors()
        .get(index as usize)
        .map_or(ptr::null(), |descriptor| descriptor as *const _)
}

unsafe extern "C" fn factory_create(
    _factory: *const clap_plugin_factory,
    host: *const clap_host,
    plugin_id: *const c_char,
) -> *const clap_plugin {
    if host.is_null() || plugin_id.is_null() {
        return ptr::null();
    }
    // SAFETY: the host passes a valid C string.
    let wanted = unsafe { CStr::from_ptr(plugin_id) };
    let visible_specs = SPECS.iter().filter(|spec| visible(spec));
    for (spec, descriptor) in visible_specs.zip(descriptors()) {
        if spec.id == wanted {
            return plugin::create(spec.kind, descriptor, host);
        }
    }
    ptr::null()
}

static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(factory_count),
    get_plugin_descriptor: Some(factory_descriptor),
    create_plugin: Some(factory_create),
};

unsafe extern "C" fn entry_init(plugin_path: *const c_char) -> bool {
    let path = if plugin_path.is_null() {
        String::new()
    } else {
        // SAFETY: the host passes a valid C string.
        unsafe { CStr::from_ptr(plugin_path) }
            .to_string_lossy()
            .into_owned()
    };
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_lowercase();
    if name.contains("crash-on-load") {
        // An access violation, the commonest way a real plugin dies.
        let nowhere = std::hint::black_box(8_usize) as *mut u8;
        // SAFETY: none. The write is meant to fault.
        unsafe { nowhere.write_volatile(1) };
    }
    if name.contains("hang-on-load") {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    if name.contains("chatty-on-load") {
        use std::io::Write;
        loop {
            println!("plugin chatter is not scanner progress");
            let _ = std::io::stdout().flush();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    let _ = FILE_NAME.set(name);
    true
}

unsafe extern "C" fn entry_deinit() {}

unsafe extern "C" fn entry_get_factory(factory_id: *const c_char) -> *const c_void {
    if factory_id.is_null() {
        return ptr::null();
    }
    // SAFETY: the host passes a valid C string.
    if unsafe { CStr::from_ptr(factory_id) } == CLAP_PLUGIN_FACTORY_ID {
        &raw const FACTORY as *const c_void
    } else {
        ptr::null()
    }
}

/// The symbol a CLAP host looks for.
#[unsafe(no_mangle)]
#[allow(non_upper_case_globals)]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION,
    init: Some(entry_init),
    deinit: Some(entry_deinit),
    get_factory: Some(entry_get_factory),
};
