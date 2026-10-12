# Plugin host evaluation

Written 2026-10-07, at the start of phase 4. It answers one question from `WINDFALL_PLAN.md`: which Rust libraries can Windfall build its CLAP and VST3 hosting on, and where does it have to bind the official SDKs itself.

Everything below comes from reading the published source of each crate (the crates.io tarballs named by version), from the crates.io and GitHub APIs on the day of writing, and from running the result against real plugins. Where something was not checked, it says so.

## What Windfall needs from a host library

- A license compatible with GPL-3.0-or-later.
- Hosting, not plugin authoring: load a file, list what is in it, create an instance, activate it, process audio with events, read and write parameters, save and restore state, open the editor.
- An audio path that Windfall can keep free of allocation, locks and blocking in its own code. The plugin may still do those things. The engine rule is about what Windfall adds.
- A threading model that matches the engine. Processors are built and prepared away from the audio thread, handed over ready, and dropped away from it again.
- Windows, macOS and Linux.
- A way to put the plugin's editor in a native window.

## CLAP

### clack (clack-host, clack-extensions, clack-common, clack-plugin) 0.2.0

| | |
|---|---|
| License | MIT OR Apache-2.0 |
| Source | github.com/prokopyl/clack, 241 stars, 4 open issues, last push 2026-09-12 |
| Releases | 0.1.0 on 2026-05-03, 0.1.1 on 2026-07-29, 0.2.0 on 2026-09-12 |
| Sits on | `clap-sys` 0.5.0 (CLAP 1.2.2), `libloading` 0.8, `bitflags` |
| Size | clack-host 5,752 lines, clack-extensions 14,251 lines |

Safe wrappers over the CLAP C API, split by the three thread classes CLAP defines. `PluginInstance` holds the main-thread half and is not `Send`. `activate` returns a `StoppedPluginAudioProcessor`, which is `Send` and not `Sync`, so it can travel to the audio thread and nothing can call it from two threads at once. `deactivate` takes the processor back. That is the engine's own rule, expressed in types.

What it covers for a host:

- Loading a `.clap` file and running its entry (`PluginEntry::load`), including macOS bundles through `NSBundle`. Entries are cached per path and kept alive by every instance made from them.
- The plugin factory and descriptors: id, name, vendor, version, features.
- `init`, `activate`, `start_processing`, `process`, `stop_processing`, `reset`, `deactivate`, `destroy`, `on_main_thread`.
- Every event type of CLAP 1.2: notes, note expressions, parameter values and modulation, gesture begin and end, transport, MIDI, MIDI 2.
- The host side of 27 extensions. Windfall uses audio-ports, note-ports, params, state, latency, tail, gui, timer, log and thread-check.
- Host callbacks run inside `catch_unwind`, so a panic in Windfall's handlers cannot unwind into the plugin.

What the process path costs. `StartedPluginAudioProcessor::process` (`clack-host/src/process.rs:462`) builds one `clap_process` struct on the stack and calls the plugin. It does not allocate or lock. The two places where a careless host would allocate are its own choices:

- Events. `EventBuffer` is a growing `Vec`. A host can instead implement `InputEventBuffer` and `OutputEventBuffer` on a fixed array (`clack-common/src/events/io/implementation.rs:15,31`), and Windfall does.
- Audio buffers. `AudioPorts::with_input_buffers` reuses a `Vec` sized at activation. Windfall skips it and builds the `clap_audio_buffer` array itself through `InputAudioBuffers::from_raw_buffers`, because it needs things the safe builder cannot express: the same memory for input and output when a plugin allows in-place processing, and silent or scratch buffers for the extra ports of a plugin with a sidechain.

How editors are embedded. `PluginGui` wraps `is_api_supported`, `create`, `set_scale`, `get_size`, `can_resize`, `get_resize_hints`, `adjust_size`, `set_size`, `set_parent`, `set_transient`, `suggest_title`, `show`, `hide` and `destroy`. `Window::from_win32_hwnd`, `from_cocoa_nsview` and `from_x11_handle` carry the parent. Clack creates no window and runs no event loop. The host owns both.

What is missing or weak:

- No scanning, no cache, no process isolation. Those are the host's job in any case.
- `HostWrapper::handle_main_thread` (`clack-host/src/extensions/wrapper.rs:126`) trusts the plugin to call main-thread callbacks from the main thread. Its safety comment is a TODO. A plugin that calls `latency.changed()` from its audio thread would reach the main-thread handler from the wrong thread. Windfall's handlers therefore check the calling thread and otherwise only set atomic flags.
- The audio buffer module is undocumented (`#[allow(missing_docs)] // TODO: doc this`).
- The crates are five months old on crates.io. The repository is older and is the base of at least one other published host (`plughost-formats`, by its lockfile).
- A non-UTF-8 path is converted lossily before it is handed to the plugin's entry.

Platforms. Loading is `libloading` on all three, with bundle resolution on macOS. The GUI types name win32, cocoa, x11 and wayland.

### clap-sys 0.5.0

| | |
|---|---|
| License | MIT OR Apache-2.0 |
| Source | github.com/micahrj/clap-sys, 225 stars, last push 2026-07-20 |
| Releases | 0.1.0 in June 2022, 0.5.0 on 2025-01-03 |

A hand-maintained, line-for-line translation of the CLAP headers at version 1.2.2. No build script, no dependencies. Complete for the stable API. It is what clack is built on, so choosing clack does not rule it out: Windfall names `clap-sys` types where it builds raw buffers.

Using it alone means writing what clack already has: the host struct and its extension lookup, the lifetime rules between entry, instance and processor, panic guards on every callback, and the stream and event wrappers. That is about 6,000 lines of unsafe code to own for no gain in control, since clack lets the host replace the two parts that matter for realtime work.

Windfall's test plugins are written against `clap-sys` directly, on purpose. A host built on clack and tested only against plugins built on clack would share one reading of the ABI. The test plugins are a second, independent one.

### Newer crates

Read by source on 2026-10-07. None can carry a realtime host.

| Crate | License | Built on | Finding |
|---|---|---|---|
| truce-rack-clap 1.1.5 | MIT OR Apache-2.0 | `clap-sys` 0.5 | Passes a null `clap_host` to the plugin (`src/lib.rs:503`, marked TODO), which breaks the CLAP contract. Stereo hard-coded. Three `Vec` allocations per block. |
| maolan-plugin-host 0.0.8 | BSD-2-Clause | its own `#[repr(C)]` structs | A helper binary for one DAW's protocol. One heap allocation per event. Its buffer lock hands out `&mut T` from `&self` without locking (`src/util.rs:4-22`). No macOS. |
| plughost 0.1.0 | MIT OR Apache-2.0 | clack, in a sub-crate not read | Out of process only, and offline by its own README: each block is a blocking socket round trip. One week old. Worth reading for how it supervises scans. |
| plugin_host 0.1.0 | MIT | nothing | Every method is a stub. `process` returns `true` without calling a plugin. |

## VST3

### vst3 0.3.0 (coupler-rs)

| | |
|---|---|
| License | MIT OR Apache-2.0 |
| Source | github.com/coupler-rs/vst3-rs, 78 stars, 1 open issue, last push 2026-03-28 |
| Releases | 0.1.0 in August 2023, 0.2.0 on 2025-11-01, 0.3.0 on 2025-12-07 |
| Sits on | `com-scrape-types` 0.1.1, nothing else |

Bindings generated from the VST3 SDK headers. Since 0.3.0 the generated file (15,436 lines) ships in the crate, identical on all three platforms, so building needs no SDK checkout, no libclang and no C++ compiler. The SDK the bindings come from is MIT since version 3.8 (October 2025), which is what lets a crate publish them.

It gives every interface a vtable struct and a trait, plus `ComPtr`, `ComRef` and `ComWrapper` for holding COM objects and for implementing host-side interfaces in Rust. Everything a host needs is present as a binding: `IPluginFactory` 1 to 3, `IComponent`, `IAudioProcessor`, `IEditController`, `IConnectionPoint`, `IComponentHandler`, `IHostApplication`, `IBStream`, `IParameterChanges`, `IParamValueQueue`, `IEventList`, `IPlugView`, `IPlugFrame`, `IRunLoop`.

It is bindings and nothing more, and says so. No module loading, no bundle layout, no host objects, no safe process call. A host writes all of that. The thread rules are the SDK's: the controller and the view belong to the UI thread, `process` to the audio thread, and the host implements the parameter and event queues that cross between them. `ComWrapper::new` allocates, so a host must build its queues once at setup and reuse them. Nothing in the crate prevents that.

### vst3-sys (RustAudio)

Not on crates.io. The repository's `Cargo.toml` says GPL-3.0, written by hand from the SDK when the SDK was GPL or proprietary. Last push 2023-06-19. GPL-3.0 is compatible with Windfall, but the crate is unmaintained, unpublished and older than SDK 3.8. Not considered further.

### Newer crates

| Crate | License | Built on | Finding |
|---|---|---|---|
| vst3-host 0.9.0 | MIT | `vst3` 0.3 | The widest feature set of any crate here: scanning with a crash-isolated probe, parameters, gestures, state, editor on three platforms. Its audio path locks mutexes that the plugin's editor thread also takes (`src/internal/plugin_impl.rs:2120`, `com_implementations.rs:1770`), copies every buffer in and out, and logs. Its own docs say the path is not realtime-safe. The host owns the transport, with no way to set the song position per block. Good reference for COM thread affinity. |
| truce-rack-vst3 1.1.5 | MIT OR Apache-2.0 | `vst3` 0.3 | `initialize` with a null host context. No component handler. Parameter changes never reach the processor. Two COM allocations per block. |
| rack 0.4.8 | MIT OR Apache-2.0 | a C++ shim | The build script clones the Steinberg SDK from the network, unpinned, and compiles it with CMake. No component handler, no transport, no editor for VST3. |
| maolan-plugin-host 0.0.8 | BSD-2-Clause | `vst3` 0.3 | VST3 on Linux and FreeBSD only. Up to three COM allocations and five `Vec`s per block. |

### Binding the official SDK directly

The SDK is MIT, so Windfall could vendor `pluginterfaces` and compile Steinberg's C++ hosting helpers (`public.sdk/source/vst/hosting`) behind a C shim. That brings a C++ toolchain into every build of the workspace on three platforms, plus a shim to write and keep in step. The hosting helpers are convenience classes (module loading, a parameter-change container, an event list). They are a few hundred lines each, and the interfaces they implement are in the `vst3` crate already. Writing them in Rust is less work than binding them, and keeps the build pure Rust.

## Decision

CLAP is hosted on clack-host and clack-extensions 0.2.0, with `clap-sys` 0.5.0 named directly where raw buffers are built.

VST3 scanning and audio hosting use the exactly pinned `vst3` 0.3.0 bindings, with the host objects written in Windfall. No other VST3 crate is used, and the C++ SDK is not built. The [VST3 hosting report](vst3-hosting.md) describes the implemented API, verified audio/editor behavior, and remaining boundaries.

The reasons:

1. Clack's types state the engine's threading rule. The main-thread half cannot leave its thread, the audio half can be sent but not shared, and deactivation needs the audio half back. Windfall would have to write the same split over `clap-sys`.
2. Clack does not force an allocation on the audio path. The two growing containers it offers are optional, and Windfall replaces both with fixed ones. A test counts allocator calls around `process` and requires zero.
3. Every crate that wraps more than clack does has a blocking or allocating audio path, or leaves out part of the plugin contract. Each would have to be rewritten where it matters.
4. For VST3 there is no safe host crate that keeps the audio path clean, so the choice is between raw bindings in Rust and raw bindings in C++. The Rust ones need no C++ toolchain and are generated from the same headers.
5. All of it is MIT or Apache-2.0, which GPL-3.0-or-later can include.

Windfall keeps the binding out of its public API. `windfall-plugin-host` exposes its own types (`PluginInstance`, `PluginProcessor`, `PluginEffect`, `PluginInstrument`), so replacing clack, or moving a plugin into another process, changes one module.

### Risks of the decision

- Clack is young and has one maintainer. Windfall uses a narrow part of it and pins the version. If it stalls, the fallback is the same code over `clap-sys`, which is the layer below and changes only when CLAP does.
- `vst3` 0.3.0 has had no commit since March 2026. It is generated code from a stable ABI, so it ages slowly, but new SDK interfaces arrive only when someone regenerates it. `vst3-bindgen` 0.3.0 is published for that.

## Dependencies added

Every crate `windfall-plugin-host` adds to the workspace lock file, with the license from its manifest:

| Crate | Version | License | Used for |
|---|---|---|---|
| clack-host | 0.2.0 | MIT OR Apache-2.0 | CLAP hosting |
| clack-extensions | 0.2.0 | MIT OR Apache-2.0 | CLAP extensions |
| clack-common | 0.2.0 | MIT OR Apache-2.0 | shared by the two above |
| clap-sys | 0.5.0 | MIT OR Apache-2.0 | CLAP C types |
| libloading | 0.8.x | ISC | loading plugin files |
| bitflags | 2.x | MIT OR Apache-2.0 | used by clack |
| vst3 | 0.3.0 | MIT OR Apache-2.0 | VST3 interface bindings |
| com-scrape-types | 0.1.1 | MIT OR Apache-2.0 | COM pointers for `vst3` |
| rtrb | 0.4.x | MIT OR Apache-2.0 | lock-free queues between threads (already used by the engine) |
| windows-sys, windows-link | 0.61.x, 0.2.x | MIT OR Apache-2.0 | Win32 windows, Windows only |
| serde, serde_json | 1.x | MIT OR Apache-2.0 | scanner output and cache (already in the workspace) |

The test plugins add nothing beyond `clap-sys`, `vst3` and `windows-sys`.

The sections on real plugins and on what VST3 does today are at the end of this file. They are filled in from the runs made after the code was written.

## Implemented host and public API

The CLAP path loads modules, creates main-thread instances, activates prepared audio-thread processors, processes stereo effects and instruments, and deactivates/destroys them off the audio thread. Mono inputs receive the stereo average; mono outputs feed both sides. Main ports support in-place processing only when both ports declare the pairing. Sidechain inputs are silent and additional outputs have separate scratch buffers; routing those ports is future engine work. Blocks longer than the activation maximum are split without losing event offsets or transport position.

Notes use sample offsets and CLAP notes or MIDI according to the first note input's dialect. Parameters support metadata, module paths, stepped/enum flags, values, text conversions, block-boundary changes from the main thread, sample-offset changes from the processor, and plugin gestures/value notifications back to `idle`. Native state uses bounded streams and an opaque versioned, base64-serialized `PluginState`; plugins without a state extension fall back to a parameter snapshot. Latency and tails are read from extensions. The adapters expose dynamically owned parameter descriptions shaped like `windfall_dsp::ParamInfo`; they cannot supply its static string references directly.

The principal signatures are:

```rust
PluginHost::windfall() -> PluginHost
PluginHost::load(&self, path: &Path) -> Result<PluginModule, PluginError>
PluginModule::descriptors(&self) -> Vec<PluginDescriptor>
PluginModule::create(&self, id: &str) -> Result<PluginInstance, PluginError>
PluginModule::probe(&self, id: &str) -> Result<PluginLayout, PluginError>
PluginInstance::activate(&mut self, sample_rate: f64, max_block: usize)
    -> Result<PluginProcessor, PluginError>
PluginInstance::deactivate(&mut self, processor: PluginProcessor)
PluginInstance::set_param(&mut self, id: u32, value: f64) -> bool
PluginInstance::save_state(&mut self) -> Result<PluginState, PluginError>
PluginInstance::load_state(&mut self, state: &PluginState) -> Result<(), PluginError>
PluginInstance::idle(&mut self, notify: &mut dyn FnMut(PluginNotification))
PluginInstance::open_editor(&mut self, options: &EditorOptions)
    -> Result<EditorInfo, EditorError>
PluginInstance::close_editor(&mut self)
PluginProcessor::note_on(&mut self, time: u32, key: u8, velocity: f32) -> bool
PluginProcessor::note_off(&mut self, time: u32, key: u8) -> bool
PluginProcessor::set_param(&mut self, time: u32, id: u32, value: f64) -> bool
PluginProcessor::set_transport(&mut self, transport: Transport)
PluginProcessor::process(&mut self, left: &mut [f32], right: &mut [f32]) -> ProcessStatus
PluginInstance::prepare_effect(&mut self, sample_rate: f32, max_block: usize)
    -> Result<PluginEffect, PluginError>
PluginInstance::prepare_instrument(&mut self, sample_rate: f32, max_block: usize)
    -> Result<PluginInstrument, PluginError>
scan_file(runner: &dyn ScanRunner, path: &Path) -> Result<FileScan, ScannerUnavailable>
check_plugin(program: &Path, path: &Path, id: &str, timeout: Duration)
    -> Result<CheckReport, ScannerUnavailable>
```

`PluginEffect` and `PluginInstrument` offer `reset`, `set_param(index, value)`, `set_tempo`, `process(left, right)`, latency and tail. Instruments also offer note on/off, all-notes-off and a held-note count. Preparation belongs to the main-thread instance, and release returns the adapter to that instance. A held-note count is not the plugin's actual release-voice count; the engine must listen for silence and honor the reported tail. These adapters are separate types and do not yet change `AnyEffect`, `AnyInstrument` or the engine.

Discovery has platform-specific CLAP/VST3 paths plus caller-supplied folders. The scanner uses one helper process per file, emits progress before each instance, and reruns while skipping instances that crash or hang. The supervising process enforces deadlines on recognized helper progress, not arbitrary plugin logging; it bounds buffered stdout and the reader queue. The JSON catalog caches path, binary mtime and size, writes through a temporary rename, records crash/timeout blocklisting, and offers `retry` and `retry_blocked`. Timestamp/size caching is not a content integrity check. A plugin can spoof stdout protocol messages; this scanner is crash isolation, not a security sandbox.

### Realtime evidence and containment

`tests/realtime.rs` uses a calibrated thread-local counting allocator around actual fixture-plugin processing, events, block splitting, reset/stop, overflow, mono/sidechain layouts and both adapters. Every tested audio path makes **zero allocations, reallocations or frees**. The calibration uses `black_box` so optimization cannot remove the allocation. A source audit of `processor.rs`, `clap/processor.rs`, `clap/events.rs`, the CLAP callbacks and clack's `StartedPluginAudioProcessor::process` found no host mutex, blocking channel or growing allocation in those paths. Setup preallocates port buffers and fixed-capacity event vectors; crossing threads uses `rtrb` and atomics. Draining the main-thread queue is bounded even if the producer keeps writing concurrently. Reset and stop also mark the audio callback context so plugin logging cannot call a potentially allocating user log sink there.

These are host guarantees, not guarantees about arbitrary plugin code. CLAP host callbacks use clack's panic guards; the native window procedure catches Rust panics. A Rust plugin panic at a non-unwinding C ABI aborts its process. The fixture proves that the supervised check process dies and the parent survives. In-process plugin crashes, memory corruption and hangs can still kill or stall the DAW. NaN/infinity output is replaced with silence, denormals are flushed with the processor setting restored afterward, finite output is bounded, process failures disable further calls, and overruns increment atomic health counters. The watchdog observes a late return; it cannot preempt a hung plugin. No audible playback was used in any verification.

### Platform and feature status

| Capability | Windows x64 | macOS | Linux |
|---|---|---|---|
| CLAP discovery/scanner/core hosting | Tested | Implemented through clack; not run here | Implemented; not run here |
| Host-owned embedded CLAP native editor | Tested Win32 | Not implemented; returns unsupported | Not implemented; returns unsupported |
| Plugin-owned floating CLAP editor | Implemented, not separately exercised | API structure present, not exercised | API structure present, not exercised; no X11 fd integration |
| VST3 factory/component/controller/layout scan | Tested | Bundle entry not implemented; explicit unsupported | ModuleEntry/ModuleExit path implemented; not run here |
| VST3 audio, parameter edits, inactive state round trips | Tested effects/instruments, mono/stereo | Bundle loading unsupported | Source implementation present; not built or run here |
| VST3 embedded native editor | Tested Win32 with Surge XT and OB-Xf | Not implemented | Not implemented |

The native editor example is `cargo run -p windfall-plugin-host --example editor -- <file> <id> [seconds]`. It defaults to 30 seconds, opens without taking keyboard focus, pumps timers/window messages and closes. On Windows it checks that the returned handle is a visible native top-level window and that it is destroyed on close. The fixture editor test also exercises resize negotiation, timer callbacks, user close, duplicate-open rejection and reopen.

VST3 scans enumerate `IPluginFactory`/`IPluginFactory2`, filter audio component classes, initialize `IComponent` with a non-null `IHostApplication`, read bounded audio/event buses, initialize a separate controller when needed, connect component/controller `IConnectionPoint`s, read the parameter count, create/release an unattached view to detect editor support, disconnect, terminate and release before module exit. CLIDs use canonical FUID text rather than Windows COM byte order, with a round-trip test. `hasState` for VST3 means the component has the standard state API; successful state saving is not established by scanning.

The `vst3-hosting` feature is retained for source compatibility; audio hosting is available in the default build. `PluginModule::create` returns a real VST3 instance with the existing instance, processor, effect/instrument adapter and Windows editor APIs. State save/restore requires returning the processor first; active state operations return an explicit error. Native 32-bit sample processing, reusable bounded COM queues, sample-offset parameters/notes, transport and inactive component/controller state round trips are implemented. Remaining limits, including optional host messaging, are recorded in [VST3 hosting](vst3-hosting.md). No SDK C++ sources were copied or vendored.

## Initial CLAP-first verification, 2026-10-07

This subsection and `verification-2026-10-07.json` preserve the initial checkpoint before VST3 audio hosting was implemented. The current [supervised lifecycle report](verification-hosting-2026-10-07.json) passes all six CLAP/VST3 binaries; the [VST3 audio measurements](vst3-audio-verification-2026-10-07.json) establish finite nonzero effect/instrument processing. See [the current hosting report](vst3-hosting.md).

No CLAP or VST3 installations were found in this machine's standard common-files or per-user common folders. The official release archives were extracted only under `%TEMP%/windfall-plugin-verification-a0371897`, never installed or committed. The supervised `verify` example ran the real scanner and the lifecycle check helper with a 20-second deadline for each step. The checked-in [machine-readable report](verification-2026-10-07.json) records every result and failure. This is a narrow Windows smoke test at 48 kHz, 512 frames, 64 blocks; it does not establish broad version/platform compatibility.

| Plugin / vendor | Format | Scan | Instantiate/activate | Process silence + note | Parameters | Save/restore state | Failure |
|---|---|---|---|---|---|---|---|
| Surge XT 1.4.0 / Surge Synth Team | CLAP | Pass | Pass | Pass; peak about 0.27 | 775, all readable/text | 58,609 bytes, identical after restore | None |
| Surge XT Effects 1.4.0 / Surge Synth Team | CLAP | Pass | Pass | Pass; silence peak 0 | 61, all readable/text | 1,089 bytes, identical after restore | None |
| OB-Xf 1.0.3 / Surge Synth Team | CLAP | Pass | Pass | Pass; peak about 0.13 | 102, all readable/text | 2,314 bytes, identical after restore | None |
| Surge XT 1.4.0 / Surge Synth Team | VST3 | Pass; buses, editor detected | Scan creates/initializes component + controller; audio creation refused | Not reached | 2,855 reported by controller | Not tested | Audio hosting explicitly unsupported |
| Surge XT Effects 1.4.0 / Surge Synth Team | VST3 | Pass; buses, editor detected | Scan creates/initializes component + controller; audio creation refused | Not reached | 62 reported by controller | Not tested | Audio hosting explicitly unsupported |
| OB-Xf 1.0.3 / Surge Synth Team | VST3 | Pass; buses, editor detected | Scan creates/initializes component + controller; audio creation refused | Not reached | 2,183 reported by controller | Not tested | Audio hosting explicitly unsupported |

The Surge VST3 parameter counts include its MIDI-controller parameters. They differ legitimately from CLAP. CLAP state restore was also verified between separate instances using the independent gain fixture. Both Surge XT and OB-Xf real CLAP editors opened briefly and closed cleanly; the dedicated fixture provides automated native handle/resize/timer assertions. No VST3 editor was attached and no audio device was opened.

Release provenance and licenses:

- [Surge official nightly release](https://github.com/surge-synthesizer/surge/releases/tag/Nightly): `surge-xt-win64-juce7-NIGHTLY-2026-10-01-348cfb3-pluginsonly.zip`, source revision `348cfb3`, SHA-256 `28c6c140f4714e5bc03fced74e2f7de3f793f24cd9a5bc965ba5521bb8e51225`. Surge XT and its effects are GPL-3.0, checked against the [upstream license](https://github.com/surge-synthesizer/surge/blob/main/LICENSE).
- [OB-Xf official v1.0.3 release](https://github.com/surge-synthesizer/OB-Xf/releases/tag/v1.0.3): `ob-xf-Windows-v1.0.3.zip`, SHA-256 `149fda0649daf8a3aa0851801f92515f0dbf6203acf33c0edc7d3c445e80b1e7`. GPL-3.0, checked against its [upstream license](https://github.com/surge-synthesizer/OB-Xf/blob/main/LICENSE).

The independent dev-only fixture library is GPL-3.0-or-later. Its raw CLAP gain/state/editor and sine/latency plugins include mono, separate-buffer swap, silent sidechain, MIDI-only notes, NaN, process failure/panic, absurd ports, slow process, init rejection/crash/hang, crash/hang on entry load, and continuously logging hang cases. Its VST3 factory now exposes a hosted stereo effect, instrument, mono effect, process-error/NaN cases and absurd buses. The VST3 processor checks lifecycle and transport and returns parameter feedback. Integration tests build it into `target/tmp` and copy it into per-test scratch folders; nothing installed on the machine is a test dependency.

## Validation

The current Windows/MSVC all-features/all-targets run passes **93 tests**, with no failures or ignored tests: library 19, containment 9, native CLAP editor 1, parameters/state 10, CLAP processing 18, realtime 8, scanner 19, VST3 hosting 8, VST3 scanner 1. Host and independent fixture all-target clippy pass with warnings denied. Both format checks, doc tests, binary/example builds and `git diff --check` pass. The VST3 allocator test covers effect/instrument/mono processing, offsets, split blocks, reset and stop, with zero host allocation/reallocation/free calls. See [VST3 hosting](vst3-hosting.md) for the precise lifecycle and platform limits.

Initial CLAP-first checkpoint: **79 tests passed**, none failed or ignored. Historical `cargo test -p windfall-plugin-host` summaries, in order (library, scanner/check binaries, containment, native editor, parameter/state, processing, realtime, scanner, VST3 scan, doctests):

```text
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.19s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.24s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

`cargo clippy -p windfall-plugin-host --all-features --all-targets -- -D warnings`, the fixture crate's own all-target clippy with warnings denied, both crate-format checks, and `git diff --check` pass. Binaries and all three examples build. Every cargo shell sourced `scripts/msvc-env.sh`. No whole-workspace or desktop build was performed in this host-only checkout.

## Integration proposal

No engine, model or shell code changes belong to this spike. Add a model-owned `PluginReference { format, id, state_blob, parameter_snapshot }`, where snapshots are sorted `(plugin_parameter_id, value)` pairs. Resolve its id through the catalog; keep the actual binary path in the catalog, not as the project's portable identity. Add `EffectParams::Plugin(PluginReference)` and `ChannelSource::Plugin(PluginReference)` variants. Store opaque state and snapshots with the project; resolve missing/blocked plugins to a visible bypass/silent placeholder so projects can still open.

The shell main thread owns `PluginHost`, modules, instances and GUI handles in a registry keyed by effect/channel id and project generation. It loads/restores inactive instances, activates/prepares adapters, then passes only `Send` adapters into prepared `PlanState` seats. Extend the rack processor enum to contain built-in or plugin adapters while retaining the existing id-based take-over and retired-state return. Match by both slot id and plugin identity; never reuse an old instance after a plugin identity change. GUI handles remain entirely in the shell registry. Return retired adapters from the audio thread, stop/release/deactivate them on the appropriate lifecycle threads and destroy them off the audio thread; drain/remove the GUI registry when the project changes.

Automation needs stable plugin parameter ids, with a prepared id-to-index mapping for the adapter. The shell's `idle` notifications turn begin/value/end into one undoable gesture and automation events. Latency changes trigger an off-thread compensation-plan rebuild; restart requests return the processor for reactivation. Save/load state should happen with processing paused and the instance returned when a plugin requires it. Sidechains and extra outputs need an explicit prepared port-routing extension beyond this stereo adapter.

`InstanceBackend` and `ProcessorBackend` are the seam for later process-hosting. A helper can share preallocated audio blocks and fixed event/transport arrays and exchange state/editor commands through a main-thread pipe. A pipelined audio protocol must consume the last completed block without waiting, add/report one block of latency and yield silence/bypass if the helper misses its deadline. A synchronous pipe read, futex wait or blocking event on the audio thread would break the engine contract.

## Primary evaluation sources

The package versions above were read from downloaded registry source, including each `Cargo.toml` license; `cargo search clack-host` and `cargo search vst3` were rerun on 2026-10-07. Repository maintenance figures for clack and vst3 were rechecked through the GitHub API. Primary sources: [clack source](https://github.com/prokopyl/clack), [clack-host docs](https://docs.rs/clack-host/0.2.0/clack_host/), [clap-sys source](https://github.com/micahrj/clap-sys), [vst3-rs source](https://github.com/coupler-rs/vst3-rs), [vst3 docs](https://docs.rs/vst3/0.3.0/vst3/), [vst3-sys source/license](https://github.com/RustAudio/vst3-sys), [official VST3 SDK](https://github.com/steinbergmedia/vst3sdk), [SDK interface license](https://github.com/steinbergmedia/vst3_pluginterfaces/blob/master/LICENSE.txt), and [CLAP specification](https://github.com/free-audio/clap).

## 2026-10-11 hosting source checkpoint

See [the worker hand-off](../qa/2026-10-11-plugin-hosting.md) for ABI 7 channel/instance
replay, negotiated expression capabilities, real helper editor lifecycle and
guarded native dirty capture, DSP wrapper policies, and the unverified macOS
AUv2 adapter. Earlier ABI/editor-limit notes are historical where superseded.
All new regressions are written, not executed; platform/corpus/packaging gates
remain explicit and no whole hosting row is newly verified.

Current source additions also include saved wrapper DSP/owner/editor controls
with undo/reopen tests, the numeric VST3 expression SDK-list correction and
native fixture/helper replay cases, and a separate CLAP engine export target.
[Wrapper settings](wrapper-options.md) documents supported behavior and explicit
threaded refusal. VST2 is excluded by the owner decision; AU and export remain
partial. These additions are written, not executed and do not extend historical
verification claims above.
