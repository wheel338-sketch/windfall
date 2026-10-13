# Modular rack host seam

This source pass is uncompiled and untested under the edit-only worker brief.
It integrates the existing note-map foundations rather than registering note
processors as audio EffectKind entries. It does not replace layer channels or
the per-channel expression map.

## Saved graph and preparation

ProjectSettings.modularRacks is a channel-ID side table, absent from legacy
JSON. RackGraph uses nonzero monotonic graph-local u32 node IDs and u16 ports:
audio input/output 0/1, note input/output 2/3, control input/output 4/5, and
note-channel outputs 16..31. RackEdge.parameter is a stable descriptor ID.
SetChannelRack checks and replaces the complete graph in one undoable command;
optional expected graph guards stale editors. Removing/duplicating a channel
removes/copies its graph. ChannelPreset capture includes nested graph state;
loading a plain sound clears the previous rack and parameter owners.

Limits: 16 racks per project, 64 total saved nodes, 256 total saved wires,
4 nested levels, 1024 flattened wires, 256 transit events per dispatch,
1024 instrument-child owners and 64 generator seats per rack. Preparation
refuses more than one second of latency or more than 16 seconds / 1,048,576
stereo frames of compensation storage. Round four adds aggregate nested fanout
admission; concurrent live-note exhaustion is still bounded and reported.

ModularRack::prepare_with_input_latency flattens and orders the immutable
built-in topology, constructs actual DSP and fixed rings outside the callback.
Audio paths align at merges and root outputs. Absolute latency metadata passes
parent source latency only into audio inputs; embedded instruments retain an
independent origin. The same frame processor runs playback and export.

## Note and control ownership

Six RackNoteModule variants map colors to route channels, attach per-note ADSR,
scale velocity/pan/release/fine-pitch/X/Y properties, split zones, remap keys and
expand bounded chords, or generate step sequences/held-chord arpeggios.
Generators use musical-beat gates and shared bounded group clocks. Each child
keeps the original parent occurrence, mapped key/channel and its own DSP ID;
note-off never remaps through current settings. Key-only DSP providers defer
release until the final logical owner of the key ends. Live property updates
replay scaler nodes from the accepted path, preserving mapped properties.
Expression-map gain multiplies gain envelopes; mapped pitch composes with the
parent glide offset. Layer fanout and expression mapping remain upstream.

ControlInput is an external normalized macro inlet, not a controller source.
Root indices 0..31 reuse InstrumentParam automation; deleting an inlet removes
its lanes/controller links. Stable descriptor IDs address built-in destinations.
Latency-changing control edges are refused; saved edits trigger preparation.
The round-four note-module parameter mechanism is documented below. Internal
controller sources use the existing project link mechanism; they are not
represented as working audio effects.

## Remaining gates

Round five models native and asset ownership on individual graph nodes, as
described below. Native-bound and sampler parent channels still reject rack
attachment; add sample-bank or hosted instrument nodes inside a built-in parent.
The round-three editor only saved/loaded checked preset text; round four also
connects the integrated native Backend file/library facade. Graph replacement
retriggers held parents and releases outgoing generations through bounded tail
storage. Asset/native outgoing audio keeps running until retirement.
The original round-three pass retained only the last envelope shape. Round four composes shapes and adds
auxiliary outputs; round five adds asset-bearing node preparation.

RealtimeFrame.rejectedRackEvents is optional and omitted at zero. A cumulative
engine counter exposes bounded owner/generator/event refusal. The editor uses a
ref-backed realtime output, with generation clearing in the realtime store.
No callback heap allocation/free, locks or I/O are introduced intentionally;
the written allocator suite must establish this after integration.

## Round-four addressing and admission (written, not executed)

`RackGraph.parameters` is a serde-defaulted root registry of
`RackParameterAddress { path, kind, parameter }`. `path` contains every nested
node ID including the owner. `kind` is the note-module tag; `parameter` is the
module's append-only descriptor index. `InstrumentParam { channel, param }`
retains macros at 0..31 and uses 32+registry-index for module controls.
`SetChannelRack` preserves the previous registry prefix, appends newly found
controls, retains nested nextId high-water marks and prunes removed owners'
clips, pattern events and links atomically. Missing owners leave tombstones.
Undo restores graph, owners and lanes together. Old instrument indices cannot
be reassigned to a newly attached rack. Legacy graphs omit the empty registry.

All six modules expose every saved scalar, table, chord and grid control.
DSP descriptor order and choice lists are new append-only contracts. The
descriptor generator adds `noteModules`, with paths relative to the entire
module JSON, so the UI reads the same tables as the DSP. The editor's
"Automation and controller links" controls use ordinary parameter gestures,
automation-clip menus, event lanes and internal/hardware link menus.

Preparation resolves each address to a flattened node/parameter pair. The
existing 64-frame automation grid applies Copy-only updates without rebuilding
the graph. Color/key/velocity/envelope controls are sampled on note admission;
already accepted mapped keys retain their release ownership. Live property
expression replay reads the current scaler controls. Active sequencers update
their controls without resetting phase, current gate ends or held owners.
Live and offline use the same processor. No script-node variant exists in this
base; the scripting worker's engine/runtime is not implemented here.

Notes carry up to eight independent envelope shapes in fixed storage. Their
gain contributions multiply; pan/pitch/X/Y contributions add and clamp after
composition. Each release begins from its own current level and lasts its own
duration. The logical voice retires after the longest release. A ninth shape
on any flattened path is refused before publication with a checked reason.

Aggregate admission counts flattened transit events, including nested boundary
Cartesian products and all root NoteInput starts. Chords are charged four
voices even if fewer are stored, so automation cannot invalidate admission.
Bounds remain 256 transit events, 64 generator seats per trigger and 1024
flattened wires. Concurrent live-note exhaustion still uses the existing
visible bounded refusal counters. Conservative admission also charges mutually
exclusive route outputs together.

Effect auxiliary ports begin at 32 in `EffectKind::aux_outputs()` order. The
editor exposes their names; saved port checks reject unadvertised outputs.
Playback uses `EffectSlot::process_routed`, pre-sized frame scratch and existing
edge compensation rings. Muting clears main and auxiliary outputs.

The rack library reuses Backend's existing native `.wfpreset` capture/read/write
and library APIs. These calls now exist in the integrated base, superseding the
round-three statement that the facade is absent. Save captures the containing
channel and complete root graph. Rack-library load extracts only the rack and
applies a checked graph command. File/library/example loads allocate fresh root
node identities so an existing parameter lane cannot attach accidentally.
Generation/revision guards refuse stale asynchronous reads. Browser storage uses
the existing real-Rust preset harness with memory storage; desktop calls use
native files. Capability errors are displayed.

Three original parametric example racks live in
`apps/desktop/src/features/modular/examples.ts`: Soft double envelope, Two octave
chord and Quarter beat pulse. Their graph definitions are CC0-1.0 and contain
no bundled third-party presets, audio or artwork. They instantiate the user's
chosen Windfall instrument and can be saved as ordinary preset files.

## Round-five node assets and native providers (written, not executed)

`RackNode.asset` is optional and default-absent in old files. It owns validated
instrument PCM/spectra/drum patches, an impulse, Scratch audio, mapped sample
zones with inline audio, or SoundFont bytes and bank/program. Project SampleIds
cannot be stored in an owned zone. Curves remain serialized in scalar params;
document, history, plan and runtime pins retain their immutable store owners.
The aggregate rack payload/state limit is 64 MiB. Preparation also reserves a
shared 256 MiB family budget for payloads/IR workspace/fixed storage/native dry
histories/compensation and a separate shared 256 MiB decoded-bank budget. Both
cover pending, active and outgoing generations, with a 64-generation limit.

The control-side `modular_assets.rs` prepares the real standalone instrument,
bank parser, source publisher and convolver before admission. Missing payloads
retain their names and produce unavailable banks/silent sources. Native preset
review reports each missing owner; loading retains the graph so the editor can
relink it. Inline payloads are carried directly by both rack text and the native
`.wfpreset` container, requiring no destination-project sample namespace or
source directory. Existing convolver/curve/layer/spectral/snapshot/number/drum
editors receive rack ownership adapters; bank audio/zone/SF2 controls save via
the same checked graph command. Native file reads retain revision/generation
guards. Rack text permits the existing 256 MiB preset-file ceiling.

`RackNode.hosted` owns a normal PluginBinding. The appended
`PluginTarget::Rack { channel, path: [u32; 4], instrument }` uses zero padding
after the node path. Checked replacement, duplication and preset application
canonicalize the target. The node stays an instrument/effect port owner and
native state replaces its built-in provider. Native provider selection goes
through the existing `plugins_insert` facade and scanner/discovery. Selecting
the same format/id at another location preserves state, wrapper and saved
parameter values for relinking; another provider uses its own defaults.

`modular_provider.rs` wraps existing PluginFactory/HostedEffect/
HostedInstrument/ExternalEffect instances. It introduces no ABI loader or
second process host. The desktop bridge selects instrument/effect role through
the common target helper; capture inventories include nested node bindings.
Rack native controls/state/wrapper edits are ordinary checked commands routed
to their graph owner. Editor opening, parameter gestures, retry/failure reasons
and render isolation use the existing manager. Native graph control wires and
auxiliary built-in port aliases are checked refusals; the main stereo/note bus
and native parameter controls are the advertised provider interface.

Actual prepared latency is used by internal rack rings and published under the
containing channel's existing compensation key, with child native identities
recorded separately. Native nodes and their retirees keep the audio lane and
receive existing transport/control-boundary calls. Per-note gain forwards into
the existing native expression bridge. A failed helper uses that bridge's
visible failure/bypass policy while independent graph nodes continue processing.

Outgoing racks move into 64 preallocated seats, receive releases and zero new
input, finish voices and tails, then move through the existing garbage queue.
Their DSP, source leases, native facades and admission leases are destroyed on
control. Outgoing native owners relinquish editor/capture selection immediately;
owner maintenance closes their editors off audio. Plain unowned topology edits
retain the previous audible-disconnection behavior while still retiring safely.

Neither umbrella is promoted from unexecuted source. See
`docs/qa/2026-10-13-r5-rack-assets.md` for exact written tests and shared files.
Internal-source Tempo and scripted node paths are owned by the sibling worker.
