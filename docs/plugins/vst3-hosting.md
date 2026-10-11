# VST3 hosting, verified 2026-10-07

`windfall-plugin-host` now hosts VST3 effects and instruments through the same public APIs as CLAP. The `vst3-hosting` Cargo feature remains a compatibility alias; the default build hosts VST3. Only Windows x64/MSVC was exercised here. Linux module/audio source paths are implemented but were not compiled or run in this environment. macOS bundle entry is explicitly unsupported. Embedded VST3 editors are implemented only for Windows HWND parents.

## Public seam and lifecycle

Use `PluginHost::load`, `PluginModule::create`, `PluginInstance::activate`, and `PluginProcessor`, or the existing `prepare_effect` / `prepare_instrument` adapters. Notes, sample-offset parameter changes, transport, latency/tail, health, state and editor methods keep their existing signatures. Continuous VST3 parameter values use SDK normalized units, 0–1; stepped values use 0–`stepCount`, so the common integer/choice adapter remains correct. Parameter text is obtained through the controller. Stable SDK IDs identify parameters; discovery order determines adapter indices. Optional `IUnitInfo` supplies bounded group paths, and automation/read-only/hidden/bypass/list/wrap flags are retained.

The creating main thread owns modules, instances, controller calls and editors. It initializes the component once, obtains the processor, initializes a separate controller only when necessary, connects connection points, assigns a component handler, and synchronizes the initial component state to the controller. A combined component/controller is not initialized or terminated twice. Module entry/exit are cached per canonical binary path; each instance retains its DLL ownership. Factory/state failures release initialized objects and run module exit before unloading.

Activation negotiates the existing speaker arrangements, verifies that bus counts/channels did not change, activates declared audio/event buses, calls `setupProcessing` while inactive, then `setActive(true)`. Only 32-bit sample processing is supported; a 64-bit-only processor is rejected. The exclusive audio owner starts `setProcessing(true)` on its first callback and handles process, reset and stop. Return the processor/adapter to its creating main thread before deactivation, controller/component termination and module release. Losing an instance while its processor is still out deliberately keeps the COM/DLL graph alive; it never terminates a plugin beneath a running audio owner.

VST3 state capture and restoration are **inactive-only**. Calling either while a processor is out returns `PluginError::State`; this is a real integration constraint. The shell must pause/retire and return its adapter, capture/restore state on the owning main thread, then prepare it again. There is no active snapshot API in this checkpoint, and no audio-thread lock or wait is introduced to obtain one. The desktop integration currently treats VST3 as scanned but unavailable to add until it can perform that lifecycle.

State consists of bounded component bytes, optional controller bytes, and normalized parameter edits deferred while inactive. Restoration validates host lengths, IDs and finite values before touching the plugin; it restores component state first, calls controller `setComponentState`, restores controller state, and queues deferred edits for processing. This prevents an inactive controller edit from silently disappearing when component state has not received an audio block yet. Writes exceeding the configured state limit remain errors even if a plugin ignores the stream's failure. A plugin's opaque state content remains its responsibility; state belongs to its matching plugin identity.

## Audio behavior and bounds

All SDK parameter/event COM objects, audio channels, bus structures and pointer arrays are allocated before audio ownership transfers. Parameter queues share a fixed arena of 1,024 points per direction rather than allocating a matrix per parameter. Input/output event storage is fixed at 3,072 entries, enough for 1,024 scheduled events plus note-offs for all 16×128 remembered channel/key pairs. Overflow is counted and reported through existing health counters. Queue drains are bounded. No host mutex, growing allocation, logging sink, controller call or UI operation is used inside process/reset/stop. The module cache mutex is used only during main-thread loading.

The common wrapper sorts/splits input events, preserves sample offsets across split blocks, validates host events, scrubs NaN/infinity/denormals and tracks overruns. The VST3 backend supplies note-on/off events, channel/key note IDs, normalized parameter queues and a `ProcessContext` carrying sample rate, sample/beat position, continuous sample time, tempo, time signature, bar position and playing state. Parameter feedback updates atomic values and crosses to main-thread notifications; controller changes are applied from idle. Main-thread gestures cross a bounded queue, wrong-thread gesture/dirty callbacks are rejected, and restart/latency/dirty requests become existing notifications.

Each declared audio bus has its own owned channels. The first nonempty main bus is routed to the stereo adapter. Mono input receives `(left + right)/2`; mono output feeds both sides. Extra input buses are silent, extra output buses are processed and discarded. Every callback restores pointer arrays and bus counts before calling the plugin. Native multichannel input on the main bus receives the stereo signal on channels 0/1 and silence on the remaining channels. Explicit sidechain and multichannel routing remain outside this stereo seam.

Initial latency and tail are queried after activation. Latency restart requests update the shared latency and notify the shell; compensation-plan rebuilding belongs to the engine/shell. Tail is the activation-time SDK value; arbitrary dynamic tail changes are not polled on every block. Process failure disables further calls, restoring dry audio for these separate-buffer effects and silencing instruments through the common policy. A reported overrun cannot preempt a hung callback.

Reset stops/restarts processing and delivers note-offs for remembered held notes on the next block. VST3 all-notes-off uses note-off events, so a plugin may play its release/tail; the SDK offers no universal immediate voice-choke call through this interface. Same channel/key overlaps share a note ID and are not independent polyphonic note instances. MIDI CC/pitch mapping, note expressions, SysEx, program-list APIs, context menus, SDK host-message/attribute allocation, host-requested editor opening and grouped gestures are not implemented. Pointer-bearing output events are rejected; native note-output events are not exposed as an instrument voice-end signal. The processing mode supplied to VST3 is realtime; the existing `set_realtime(false)` suppresses the host's realtime watchdog for verification/export, but does not renegotiate SDK offline mode.

Scanning remains isolated in supervised helper processes. Audio hosting is **in-process**. A plugin crash, memory corruption or hang can still kill or stall the DAW. This is crash isolation for scanning and diagnostics, not a security sandbox or audio crash containment.

## Windows editor verification

`IPlugView` is created on the owning main thread, checked for HWND support, assigned an `IPlugFrame`, attached to the native parent and removed/reset before that parent is destroyed. Sizes are bounded, resize callbacks reject the wrong thread and recursive calls, and idle applies requested parent dimensions. User resizing uses the plugin's size constraint callback. The shell must pump its normal window/event loop and call instance idle; the `editor` example supplies that loop for verification.

The actual Surge XT VST3 editor opened at 1,141×711 and OB-Xf at 1,150×576, both resizable. Each was pumped for two seconds, produced a resize notification, and closed. The example asserted a visible native top-level HWND and that the HWND was destroyed afterward. This establishes attach/frame/resize/teardown behavior for those binaries, not every editor feature or every DPI/monitor arrangement. No audio device was opened.

## Actual audio evidence

The official archives already downloaded for the initial host evaluation were extracted only under `%TEMP%/windfall-plugin-verification-a0371897`; no installer or system plugin installation was used. Release provenance, archive hashes and upstream GPL licenses are in [host evaluation](host-evaluation.md#initial-clap-first-verification-2026-10-07).

The [supervised report](verification-hosting-2026-10-07.json) passes scan, creation, activation, 64 processing blocks, parameter text/readability, inactive state restore, reactivation and destruction for Surge XT, Surge XT Effects and OB-Xf in both CLAP and VST3 formats. Effects receive a sine signal; instruments receive a note. Each stage has a 20-second deadline. The host check explicitly returns the processor before state operations.

The [VST3 measurement report](vst3-audio-verification-2026-10-07.json) additionally runs 65,536 frames at 48 kHz in 512-frame blocks. Effects receive a 0.1-amplitude 440 Hz input; instruments receive note-on at sample 32 and note-off during block 96. Output goes only into memory. Every sample is checked finite and health must show zero failures, scrubs or dropped events.

| VST3 binary | Readable parameters | Peak | RMS | Nonzero stereo samples | Saved host state | Latency / tail |
|---|---:|---:|---:|---:|---:|---|
| Surge XT 1.4.0, source 348cfb3 | 2,855 / 2,855 | 0.26794 | 0.08771 | 130,884 | 60,317 bytes | 0 / 96,000 samples |
| Surge XT Effects 1.4.0, source 348cfb3 | 62 / 62 | 0.10642 | 0.06068 | 131,070 | 1,165 bytes | 0 / 96,000 samples |
| OB-Xf 1.0.3, source 1223e6f | 2,183 / 2,183 | 0.13569 | 0.05333 | 99,064 | 2,389 bytes | 0 / 0 samples |

These are measurements from one Windows run with the default patches, not compatibility guarantees for other builds or patches. Synth measurements vary with their random/voice behavior. Both pre/post-processing state restored successfully. All three outputs were finite, nonzero and free of host scrubbing/drop/failure reports.

## Fixture and verification results

The independent GPL-3.0-or-later fixture exports an actual SDK factory and combined component/controller/processor. It includes stereo gain, mono gain, instrument, rejected absurd buses, process error and NaN output cases. It requires a valid host context, rejects duplicate initialize, checks setup-before-active and processing-before-process, validates transport, returns tempo/playing/beat feedback, honors parameter and note offsets and writes/reads state. Editor absence is tested. Real editor availability/attachment is verified separately with Surge XT and OB-Xf.

Current Windows/MSVC validation:

- `cargo test -p windfall-plugin-host --all-features --all-targets`: **93 passed**, zero failed/ignored.
- Host all-features/all-target clippy with `-D warnings`, and independent fixture all-target clippy with `-D warnings`: pass.
- Host and fixture format checks, host doc tests, binaries/all examples and `git diff --check`: pass.
- Calibrated counting allocator: **zero host allocations, reallocations or frees** while VST3 effect/instrument/mono fixtures process events, split blocks, reset and stop. The counter measures the host executable's allocator; it does not establish that arbitrary plugin DLLs never allocate.
- Source audit: parameter/event COM buffers use fixed storage and cells under exclusive audio ownership; callback communication uses atomics and `rtrb`. No mutex is reachable from the host's process/reset/stop code. Main-thread state/editor/loading operations may allocate and perform OS calls.

No Linux/macOS, audio-device playback, whole-workspace or desktop build was performed in this host-only checkout. Every Cargo shell sourced `scripts/msvc-env.sh`.

## Bindings and licenses

`vst3 = "=0.3.0"` is generated Rust binding code licensed MIT OR Apache-2.0. Its `com-scrape-types` dependency has the same license. The referenced current SDK interface license is MIT. `libloading` is ISC, `rtrb` and `windows-sys` are MIT OR Apache-2.0. These dependencies are compatible with the repository's GPL-3.0-or-later license. No proprietary SDK content, C++ SDK implementation, real plugin binaries or release archives were copied into the repository. No new dependency was added beyond pinning the existing binding version exactly.

Lifecycle decisions follow primary Steinberg documentation: [API and threading model](https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical%2BDocumentation/API%2BDocumentation/Index.html), [processor call sequence](https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical%2BDocumentation/Workflow%2BDiagrams/Audio%2BProcessor%2BCall%2BSequence.html), [processing FAQ](https://steinbergmedia.github.io/vst3_dev_portal/pages/FAQ/Processing.html), and [communication FAQ](https://steinbergmedia.github.io/vst3_dev_portal/pages/FAQ/Communication.html). SDK/binding license sources are linked in [host evaluation](host-evaluation.md#primary-evaluation-sources).

## 2026-10-11 hosting source checkpoint

See [the worker hand-off](../qa/2026-10-11-plugin-hosting.md) for ABI 7 channel/instance
replay, negotiated expression capabilities, real helper editor lifecycle and
guarded native dirty capture, DSP wrapper policies, and the unverified macOS
AUv2 adapter. Earlier ABI/editor-limit notes are historical where superseded.
All new regressions are written, not executed; platform/corpus/packaging gates
remain explicit and no whole hosting row is newly verified.
