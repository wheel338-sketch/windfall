# VST2 compatibility licence investigation — 2026-10-11

The Steinberg VST2 SDK was not downloaded, inspected, used or vendored.
Steinberg's [developer licensing page](https://steinbergmedia.github.io/vst3_dev_portal/pages/VST%2B3%2BLicensing/Which%2Bfiles%2Bfall%2Bunder%2Bwhich%2Blicense.html)
says new VST2 plugin/host agreements stopped in October 2018.

There is a plausible GPL-compatible independent interface:
[LMMS's aeffectx.h](https://github.com/LMMS/lmms/blob/master/include/aeffectx.h)
carries Javier Serrano Polo's 2006 copyright and GPL-2.0-or-later terms. Those
source-licence terms are compatible with Windfall's GPL-3.0-or-later distribution.
[Carla's VeSTige header](https://github.com/falkTX/Carla/blob/main/source/includes/vestige/vestige.h)
records a good-faith belief that the interface was independently derived without
reference to the SDK or acceptance of its agreement. That statement does not
establish an audited clean-room provenance or dispose of separate contractual
restrictions on a particular distributor. It must not be described as legal
clearance. A compatible source licence alone does not settle every relevant right.

Recommended scope decision: keep `fmt-host-vst2` open, approve a narrowly scoped
GPL-interface compatibility adapter only after pinning/auditing the selected
header's exact source history and reviewing applicable distribution obligations.
Do not label ordinary `.dll` files as verified plugins before an isolated scan;
refuse mismatched bitness, unknown entry points and shell subplugins explicitly.
Implement replacing audio, normalized parameters, MIDI, chunks/parameter state,
editor ownership and native-time callbacks with an original fixture when that
provenance decision is resolved. Do not acquire the discontinued SDK as a shortcut.

This packet has **not** implemented VST2 scan/load/process/state/editor support
and cannot close the row. This is an unresolved distribution/provenance decision,
not evidence that independent VST2 hosting is necessarily unlawful.
