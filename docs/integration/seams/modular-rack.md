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
stereo frames of compensation storage. Nested fanout needs stronger aggregate
validation; runtime refuses excess transit events and reports the count.

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
Note-module parameter automation and controller-source attachment are still
integration gates; they are not represented as working audio effects.

## Remaining gates

Hosted graph nodes are not modeled: native-bound and sampler channels reject
rack attachment. Implement real provider/binding/capture/editor ownership before
relaxing that check. Channel preset file/library functions exist natively but
are not exposed through the current frontend Backend interface; the rack editor
currently saves/loads checked preset text. Graph replacement retriggers held
parents and resets DSP tails/delays. A path retains only its last envelope
shape. Multiple output ports/assets of built-ins are not represented.

RealtimeFrame.rejectedRackEvents is optional and omitted at zero. A cumulative
engine counter exposes bounded owner/generator/event refusal. The editor uses a
ref-backed realtime output, with generation clearing in the realtime store.
No callback heap allocation/free, locks or I/O are introduced intentionally;
the written allocator suite must establish this after integration.
