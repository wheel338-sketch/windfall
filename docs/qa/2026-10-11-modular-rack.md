# Modular rack and note modules — implementation hand-off

Branch: `gpt/w3-modular-rack`. Edit-only: no builds, tests, runners, benchmarks,
device checks or generated-artifact regeneration have been executed.

## Implemented

Saved channel graphs with stable node/port IDs, typed edges, cycle refusal,
node/depth/edge/event limits, nested preset JSON, project persistence, atomic
replacement, history, duplicate/remove ownership and stale-editor checks.
Control-side compilation flattens built-in graphs, prepares DSP and latency
buffers, and processes preallocated audio/note/control wires. Channel macro
inlets reuse instrument automation; removed inlet ownership prunes lanes.
The inspector mounts a graph editor with add/connect/disconnect, nested editing,
mute/solo, built-in parameter controls, six purpose-built note editors and
preset text load/save. Existing note-map, layer and expression ownership
foundations are reused. Control sources and scripting are outside this stream.

## Per-row status

| Row | Claim | Remaining gates |
| --- | --- | --- |
| fx-patcher | partial | Hosted lifecycle/state, acceptance coverage, executed validation |
| core-fl-studio-mobile-rack-fx | partial | Rack file/library workflow, acceptance coverage, executed validation |
| fx-vfx-color-mapper | partial | Routed-note runtime and mounted acceptance coverage |
| fx-vfx-envelope | partial | Executed runtime/mounted coverage |
| fx-vfx-level-scaler | partial | Executed runtime/mounted coverage |
| fx-vfx-keyboard-splitter | partial | Routed-note runtime and mounted coverage |
| fx-vfx-key-mapper | partial | Child ownership runtime and mounted coverage |
| fx-vfx-sequencer | partial | Executed runtime/mounted coverage |

## Tests written, not executed

`crates/windfall-project/tests/modular_rack.rs`:
- nested_rack_preset_and_project_round_trip_preserve_ids_wires_and_history
- cycles_type_mismatch_missing_ports_and_stale_edits_are_atomic
- macro_automation_follows_its_owner_and_removed_owner_is_undoable
- bounded_graphs_refuse_excess_nodes_depth_duplicate_ids_and_note_explosion
- legacy_projects_omit_racks_and_duplicate_channels_copy_an_independent_graph
- latency_changing_control_wire_is_refused_before_publication

DSP `rack_note::tests` now contains six tests for color routing, split
boundaries, bounded chord expansion, each property curve, envelope segments
and all module state round-trips (written, not executed).

Live property updates replay the immutable path's scaler modules in topological
order. Channel expression-map gain is preserved through generated notes and
multiplied by per-note gain envelopes. Sequencer now supports up/down/as-played
held-chord selection with shared bounded clocks; gate durations follow musical
beats across tempo changes. Parent fine pitch and envelope pitch offsets compose.
Latency metadata now passes absolute source arrival into nested audio inputs;
independent instruments keep their own source origin, matching flattened DSP.

Engine `tests/engine/modular_rack.rs` is registered and now contains seven
written, not executed cases: synth/effect and disconnected-wire reference,
parallel compensated dry/wet reference, nested/source latency agreement,
per-frame gain-envelope reference, live property-update reference, independent
sequence/arpeggio schedules across four block sizes, and allocator/overflow
admission coverage. Mounted `features/modular/editor.test.tsx` now has written, not executed cases:
add/connect/disconnect/undo, atomic cycle refusal, nested preset round-trip,
mute/solo/removal undo, each of the six distinct module controls, and persisted
held-chord mode. It uses the existing real-WASM document harness; regenerate
bindings and simulator before running. The engine group also has six written, not executed cases for real color/split
routes, same-key chord ownership, audible mute/solo/macros, maximum graph size,
live/offline/reopened project audio, and parent release through graph replacement
with allocator guarding plan adoption. Existing EVERY_COMMAND gains setChannelRack.

## Integrator commands (not executed here)

In Git Bash, `source scripts/msvc-env.sh`; put node/pnpm on PATH; set
`TS_RS_EXPORT_DIR` to a task-specific temporary directory.

- `cargo test -p windfall-dsp --lib notemap`
- `cargo test -p windfall-dsp --lib rack_note`
- `cargo test -p windfall-project --test modular_rack`
- `cargo test -p windfall-engine --test engine modular_rack`
- `cargo test -p windfall-engine --test engine`
- `cargo clippy -p windfall-dsp -p windfall-project -p windfall-engine --all-targets -- -D warnings`
- `bash scripts/gen-bindings.sh`
- `bash scripts/build-sim.sh`
- `pnpm --dir apps/desktop test src/features/modular src/lib/ipc/sim/document.test.ts --maxWorkers=4`
- Desktop typecheck/lint and native plugin state/capture regressions.

## Generation and seams

Regenerate rack graph/node/edge/endpoint/processor/preset/channel-rack types,
note-module configs (including SequenceMode), array-view TS note-map/chord/grid
tables while Rust still checks fixed lengths, ProjectSettings.modularRacks and Command.setChannelRack
(including optional expected graph), then simulator WASM. No generated edits.
Shared seams: DSP lib.rs; project lib/model/check/command/lower/automation;
engine lib/plan/state/project_preparation/rack/automation/layout; inspector,
automation targets and EVERY_COMMAND. Exact final inventory is maintained in git.

## Risks, in check order

1. Uncompiled Rust and generated TS integration; inspect new literals/selects.
2. Nested mixed-source/source-input latency changes require reference coverage.
3. Graph replacement retriggers held voices and resets delay/tail state.
4. Multiple envelope modules currently retain only the last shape per path.
5. Sequencer clock transfer when the held leader is removed needs coverage.
6. Queue budget is fixed but validation counts terminal fanout, not every
   transit event. Runtime overflow rejects deterministically; UI counter absent.
7. Hosted processors and sampler channels deliberately reject attachment;
   native state/editor ownership and rack preset library capture remain open.

Do not promote tracker rows based on this hand-off. Listening and executed
acceptance remain integrator gates.

Generation compatibility: note-map table, step-grid and chord fields explicitly
export as TS arrays so immutable editor copies type-check without tuple casts.
Number-field integer commits round before Rust deserialization.
