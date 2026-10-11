# AUv2 hosting source candidate

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
reset safety and real third-party AU instruments/effects remain acceptance work.
The Mac regression requires a stock Apple delay Audio Unit and tests real bypass
PCM, state restore, activation and returned ownership. It is written, not run.
