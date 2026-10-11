# Mixer sidechains — source implementation in progress

This packet adds detector routing, route automation, built-in compressor support,
native hosted auxiliary inputs and bridge key transport for `win-mixer-sidechain`.
This records source coverage, not verified parity. Builds, tests, QA, reviews and
artifact generators remain deferred under the feature-first instruction.

## Saved routes and ownership

`MixerTrack.sidechains` is an optional saved list of post-fader detector-only
copies using the existing target/gain record. Ordinary sends and sidechains may
share a destination independently. `SetSidechain` adds/updates/removes a route in
one document transaction, clamps finite gains to the mixer range, and rejects
missing/Current sources or destinations, self-routing, Master sources and
dependency cycles. Deleting a track removes incoming sidechains. Project
validation checks unique destinations, finite levels and the combined routing
graph. Existing files default to no sidechains.

The inspector lists detector inputs and outgoing destinations, with add/remove
and gain gestures. Routing choices account for detector dependencies. Track
deletion warnings include incoming detector sources. Preset loading preserves
destination routing and stores the compressor's detector mode with its settings.

## Engine signal flow

Compiled edges distinguish outputs, audible sends and sidechains. All influence
topological ordering and arrival-delay calculations. Sidechains are accumulated
in a separate preallocated stereo buffer for each track; they never enter its
audible input or normal meter. Existing edge gain ramps and compensation delays
apply to those copies. Their delay identities include route kind, so an audible
send and a key route to the same destination do not share compensation history.
Normal send automation resolves only audible send edges. `SidechainGain`
automation resolves only key edges, using the existing bounded edge gain ramps.
Its frontend lane identity differs from an audible send to the same destination.
Level controls expose the automation menu and live marker/readback; route removal
uses the existing automation-deletion interaction and removes its owned lanes.
Deleting either endpoint also removes matching automation.

Each effect also receives a copy of the track key bus, delayed by the cumulative
latency of the preceding slots. These key delay seats are prepared off audio,
bounded by the existing one-second compensation policy, and retain matched
history through plan adoption. Each slot's key transfer comes from the same
prepared chain description as its main signal, excluding the track input prefix.
When a preceding Stereo Matrix is present, bounded serial compensation stages
mirror its tap transition, readiness and exact rack-generation insertion/departure
waits, keeping upstream transitions ahead of downstream stages. Zero-delay matrix
stages are retained to prime future edits. Fixed-only chains and paths beyond
the one-second host bound keep scalar compensation. No key buffer allocation
occurs during processing.

Solo downstream traversal follows audible routes; upstream traversal also keeps
detector sources required by soloed tracks. Sidechain audio continues to follow
source mute, fader and pan. A key-only source can disconnect its ordinary output
while retaining detector routes.

## Compressor and provider seam

`CompressorParams.sidechain` is an append-only toggle at parameter index 9,
defaulting off. When enabled, peak/RMS level detection uses the external stereo
key instead of main audio; absent keys mean silence and release. Gain reduction,
makeup, wet mixing and the main stereo signal remain on the original path. The
control is available in the compressor editor with a provisional descriptor
overlay pending combined descriptor generation.

Built-in effects, DSP slots, engine effect units and hosted-effect interfaces
now accept an optional key slice. Effects that do not consume detectors retain
their ordinary processing behavior. Desktop native effects forward the key into
`PluginProcessor`; its bounded sub-blocks receive matching key slices.

CLAP allocates separate first-two-channel buffers for every auxiliary input at
activation. VST3 uses its existing owned bus buffers. Both default to the first
non-main audio input, fold stereo keys for mono ports, fill the first two channels
for wider ports, and keep other auxiliary inputs/channels silent. Key copies
replace nonfinite samples with zero and bound finite values to the host input
range. No per-block allocation is introduced.

Discovery stores bounded auxiliary-input names, channel counts and native input
indices in `PluginBinding`. `SetPluginSidechainInput` saves an effect's selection
in one undo transaction; `None` means the first auxiliary input. The plugin panel
lists reported ports and retains a visible saved unavailable choice. Selection
does not change the plugin's opaque-state identity. Plan adoption applies it to
the prepared effect; the desktop audio half keeps the desired index and reapplies
it when an owner returns a replacement processor. A selected input absent from
the active layout receives no key, without redirecting it to the main signal.

Bridge ABI version five adds a separate stereo key plane and a bounded input
selection word to each slot. Submission scrubs key samples; decoding rejects
nonfinite wire samples or invalid port indices. The helper applies the selection
and forwards matching key slices at its event boundaries. The realtime adapter
collects key frames alongside main frames, and the offline API accepts a matching
key slice through the same collection path. Port changes commit at complete
bridge block boundaries. Existing identity/epoch/ownership checks and deadline
fallback paths remain in use; a version-four peer cannot attach to this layout.
This extends the bridge subsystem's source implementation; selecting it as the
desktop's normal hosted provider remains separate integration work.

The subsequent [note-expression pass](NOTE-PLUGIN-EXPRESSION.md) advances the
mapping to ABI 6 for wider instance/expression events and release reserves.
Its key-plane and auxiliary-selection behavior retains the source paths above;
ABI 5 peers cannot attach to the new mapping.

## Remaining implementation and deferred validation

Desktop bridge-provider integration remains source work in the wider project.
Combined bindings/descriptors/WASM
artifacts also remain deferred. During the later QA pass, cover audible isolation,
detector timing/levels, peak/RMS modes, mute/solo, mixed route cycles, simultaneous
audible/key destinations, deletion/history, per-slot alignment and hosted plugin
ownership exchange, multiple auxiliary inputs, mono/wider port mapping, unavailable
saved selections, and version-six key transport. Bindings are provisionally synchronized; the simulator still
requires the combined native artifact build. No GitHub CI or Actions are used.

## Follow-up, 2026-10-10

Tests for the three gates the completion audit named are written: a real
CLAP/VST3 fixture with mono, stereo and four-channel auxiliary inputs and the
host, helper, desktop-provider and session cases that use it; owner exchange
with the saved choice reapplied; and engine cases that edit latency, routes
and history while the key is sounding. A key shorter than the audio block is
now silent only past its own end. None of it has been executed yet. See
`docs/qa/2026-10-10-mixer.md`.
