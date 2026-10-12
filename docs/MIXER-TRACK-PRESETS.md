# Mixer track presets — source implementation

Source coverage for `win-mixer-track-presets`. Checks, builds, tests, reviews,
binding generation and simulator regeneration remain deferred. No acceptance
or plugin compatibility verification is claimed.

## Saved state

Version 1 `.wfmixer` JSON contains name/color, fader/pan/mute, the integrated
three-band EQ and stereo matrix, manual latency correction, the ordered effect
chain with enable/wet settings, and hosted effect plugin bindings with parameters
and opaque state. Routing, sends, recording, hardware assignments, solo and docks
belong to the destination and are preserved. Name/color restoration is optional.
Plugin binaries and external plugin assets are referenced rather than packaged.

Native capture snapshots the project and plugin owner revision, releases the
document lock while capturing live plugin state, then rejects a changed document.
Recording exclusion covers the capture. The frontend also captures its document
generation across async work so an old panel cannot apply a file to a replacement
project with reused track IDs.

## Loading and history

`ApplyMixerTrackPreset` validates the version, finite settings/ranges, slot count,
slot IDs and mixes, built-in parameter ranges, and unique in-chain plugin targets.
Plugin metadata and state use the existing native validation; aggregate state is
bounded at 256 MiB. Loading requires the destination track to equal its captured
snapshot. Every imported slot receives a newly allocated project ID, and hosted
plugin targets are remapped to those IDs. Automation of replaced effects is
removed, the chain/settings are replaced and obsolete bindings are pruned within
one undo transaction. Track-owned automation and routing are retained.

The inspector exposes Save to shelf, Save file, Load file, local preset loading
and deletion. The app-wide shelf holds up to 128 presets in browser storage;
storage errors are surfaced without publishing a successful shelf change. Large
presets can use files. Native writes use the existing atomic file helper; native
reads cap bytes before parsing. Browser file input and download implement the
file controls for the mock without claiming native audio-state capture.

## Deferred validation

Exercise save/load/undo/redo of built-in and hosted chains, live plugin state,
fresh target ownership, automation removal/restoration, stale destination and
document guards, file bounds/version errors, unavailable plugins/assets, shelf
quota errors and file cancellation. Bindings are provisionally synchronized;
the simulator artifact remains the earlier advanced-fill build until the later
combined artifact pass. No GitHub CI or Actions are used.

## Portable controller and script configuration — round four

Optional `tables` stores controller-panel configuration, incoming/outgoing
controller links and the exact script revisions assigned to the preset's track
or effects. The source IDs form a local namespace: the destination track and
newly allocated effect IDs replace them in the same checked undo transaction.
Slot links are captured against their concrete source objects. External targets
require a unique match by role, name, built-in kind and hosted plugin identity
and parameter-ID order. Missing or ambiguous targets, conflicting immutable
script revisions and invalid controller configurations refuse the entire edit
with the existing visible command error. The file retains all configuration.
Older presets without this optional table still load. Round-four tests are
written for integration verification; no build or passing evidence is claimed.

## Follow-up, 2026-10-10

Presets can now be kept in a per-user library folder (`mixer-presets` beside
the settings file) and browsed from the Track presets panel: list with
per-file errors, collision-safe save, read, delete. The panel names hosted
plugins a preset refers to that are not installed here, before and after
loading. A preset never carries effect-output routes; loading one retires the
routes of the effects it replaces. Session and mounted tests are written and
not yet executed, and a preset captured from a running native plugin is still
not covered end to end. See `docs/qa/2026-10-10-mixer.md`.
