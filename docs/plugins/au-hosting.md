# AUv2 hosting source candidate

## Round-four source checkpoint — 2026-10-12

`au_model` is available on all platforms and contains checked component IDs,
installed catalog data, stable global/scoped parameter mapping, bus validation
and stereo/mono copying, host clock conversion, identity-bound state containers,
and the provider reset state machine. Its deterministic tests can run on Windows.
Existing global parameter IDs retain their identity; scoped parameters append
to the descriptor list and receive deterministic, non-aliasing IDs.

The macOS adapter enumerates installed effect, instrument, music-effect and
generator components. The desktop manager presents their `au://TYPE:SUBTYPE:VENDOR`
registry paths in the ordinary plugin picker. The main-thread Cocoa dispatcher
creates/activates/captures/edits/disposes instances; only prepared processors
cross to audio. A command already on Cocoa main runs directly instead of waiting
for its own queued dispatch. A bounded proxy pump services owner notifications.

The adapter negotiates each input/output bus and allocates independent planar
storage. Selected auxiliary inputs receive the engine sidechain; unused channels
are silent, mono buses sum stereo, and every native output bus is rendered with
the same timestamp. The existing stereo facade consumes the first output;
additional buses are owned and rendered, not separately routed into mixer tracks.
Host callbacks supply beat/tempo, meter/downbeat and timeline/play/seek changes.
Reset publishes a request and returns silence until the owner receives audio,
calls native Reset while initialized, uninitializes, removes callbacks and
prepares again. Failed native reset remains an explicit recovery error.

AU Cocoa factory views are embedded in an owned NSWindow, with retained view,
factory and bundle lifetimes. Attached windows use the Tauri NSWindow owner;
unsupported custom scaling is refused. Handwritten Objective-C declarations use
typed selector signatures, arm64 structure returns and Intel `objc_msgSend_stret`
for NSRect. No new external dependency is introduced: AudioToolbox,
CoreFoundation, AppKit and the system Objective-C runtime provide the ABI.

**None of the macOS code has been compiled, linked or executed.** It remains
unverified platform software. The hand-off lists exact portable tests and native
Mac checks, including SDK layout, callback ownership, Cocoa CF reference and
view teardown checks. Earlier limitations below are historical where superseded.

The `cfg(target_os = "macos")` adapter in `windfall-plugin-host/src/au` uses
installed component identities, not a copied SDK or an inferred DLL ABI.
`PluginHost::create_audio_unit` returns the same `PluginInstance` and
`PluginProcessor` facade used by CLAP/VST3. See the
[implementation hand-off](../qa/2026-10-11-plugin-hosting.md) for its exact scope.

The public API references are Apple's [AudioUnit SDK](https://github.com/apple/AudioUnitSDK)
and [AUBase interface](https://github.com/apple/AudioUnitSDK/blob/main/include/AudioUnitSDK/AUBase.h).
The adapter links system AudioToolbox/CoreFoundation; no library or SDK was
vendored and no dependency was added. All handwritten declarations require
layout and behavior checks against the target Mac SDK.

This Windows worker has not compiled or executed the adapter. It is not desktop
AU readiness: Cocoa views, installed-component scanner/catalog integration,
main-thread desktop runtime, transport callbacks, multi-bus routing, native
reset lifecycle recovery and real third-party AU instruments/effects remain acceptance work.
Active reset explicitly refuses subsequent rendering and requires owner-side
deactivation/repreparation: Apple's AUBase documents Reset as not realtime-safe.
Failed activation retains installed callback context until owner-side callback
removal or unit disposal; a failed Initialize cannot leave a dangling pointer.
The Mac regression requires a stock Apple delay Audio Unit and tests real bypass
PCM, state restore, activation and returned ownership. It is written, not run.
