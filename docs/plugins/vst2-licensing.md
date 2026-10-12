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

Scope decision (2026-10-11): **VST2 will not be implemented.** The project owner
has limited third-party hosting to CLAP, VST3 and Audio Unit because Steinberg
no longer offers new VST2 licences. No VST2 scan/load/process/state/editor
adapter or fixture is included. The independent-header investigation above is
historical context; its source licence does not change this product decision
and is not legal clearance.

The overseer should record `fmt-host-vst2` as excluded by scope, rather than
completed functionality. The umbrella row's VST2 wording also needs a scope
decision: it cannot be claimed literally complete while VST2 is unsupported.
This worker does not edit the parity tracker.
