# Modular rack and note modules — implementation hand-off

Branch: `gpt/w3-modular-rack`, based on `feat/parity-100`.
Implementation tip: `ac0e346f`; the final hand-off update is a separate commit.

This is an edit-only source delivery. No builds, tests, runners, benchmarks,
device checks or generated-artifact regeneration were executed. No parity
tracker or generated files were edited. All rows below remain **partial**;
written acceptance cases are not passing evidence.

## Delivered behavior

A saved channel side-table holds nested graphs with stable node/port identities,
typed audio/note/control wires, monotonic node allocation, checked endpoints,
cycle refusal, size/depth bounds and atomic undoable graph replacement. Stale
editors supply the expected graph. Channel removal/duplication removes/copies
its rack. Project save/reopen and rack preset text preserve nested state.
ChannelPreset captures and validates rack graphs through the existing native
model/session path; applying a plain sound clears the old graph and parameter
owners in the same transaction.

Control-side preparation flattens immutable topology, prepares built-in DSP and
compensation rings, and admits bounded event/child/generator tables before plan
publication. Audio wires drive actual instrument/effect processing; parallel
paths align at merges and outputs. Parent source latency propagates through
AudioInput; embedded instruments have independent origins. Playback and offline
render use the same runtime. Held parent occurrences survive plan replacement,
with old processor destruction deferred through the existing retirement path.

Six compiled note modules reuse the existing registered note-map foundations:
color routing, per-note ADSR, velocity/property power curves, zone splitting,
128-key remapping with bounded chords, and step sequencing/up/down/as-played
arpeggios. Child notes retain parent occurrence identity and mapped keys for
release. Live expression updates replay accepted property transforms; expression
map gain and fine pitch compose with child/envelope values. Existing layer
fanout remains upstream; no duplicate layer or expression-map model was added.

Root ControlInput macro indices reuse InstrumentParam automation and controller
links. Control wires address stable built-in descriptor IDs; latency-changing
controls are refused. Removed macro owners prune lanes/links. The inspector
hides original source parameter automation controls while rack macros own those
indices. Control sources and scripting remain owned by other streams.

The inspector mounts a working graph editor: add, connect, disconnect, nested
navigation, mute/solo, remove, undo/redo, built-in parameter controls, six distinct
note editors, and checked preset text load/save. Parameter gestures serialize
against the latest graph and cancel queued edits on history, unmount or document
generation changes. A ref-backed realtime output shows cumulative bounded
runtime refusals, cleared when the document changes.

## Per-row claims and remaining gates

Every row requires generated bindings, compilation and executed acceptance.

| Row | Claim | Delivered | Remaining software gates |
| --- | --- | --- | --- |
| fx-patcher | partial | Checked saved nested graph, audible built-in wires, compensation, macro automation, ownership/history, editor | Hosted node lifecycle/capture/editor ownership; auxiliary processor ports/assets; note-module parameter automation |
| core-fl-studio-mobile-rack-fx | partial | Rack of Windfall built-ins with saved state and checked preset text; ChannelPreset capture/apply | Native preset file/library UI through Backend; auxiliary built-in ports/assets; note-module automation |
| fx-vfx-color-mapper | partial | Color-to-channel routing with source color/expression retained; routed instrument audio and mounted cases | Automatable module settings; executed routing and layer/channel integration |
| fx-vfx-envelope | partial | Per-note ADSR destinations, release ownership and gain/pitch composition; purpose-built editor | Automatable module settings; multiple envelope shapes on a path (currently last wins) |
| fx-vfx-level-scaler | partial | Velocity/pan/release/pitch/X/Y scaling with normalized power curves; live property replay and editor | Automatable module settings; executed expression and audio coverage |
| fx-vfx-keyboard-splitter | partial | Checked low/high zones, optional pitch preservation and routed outputs; zone editor | Automatable module settings; executed routing and layer/channel integration |
| fx-vfx-key-mapper | partial | Saved 128-key map, bounded chord expansion, original-parent child release and key-only provider ownership | Automatable module settings; executed ownership coverage |
| fx-vfx-sequencer | partial | 16-step generator, gate/octave controls, up/down/as-played held chords, musical gates across tempo changes | Automatable module settings; executed clock/leader-removal and render coverage |

Hosted graph processors are not modeled; native-bound and sampler channels
refuse attachment. Do not relax those checks without real provider/binding,
state capture, editor and retirement ownership. Software gates above are not
hardware-only gates. No row should be promoted from this document alone.

## Acceptance tests

All cases below are **written, not executed**. They include independent DSP/audio
references, fixed block-size variations, allocation counters, real project
history/persistence and the existing real-WASM mounted document harness.

### DSP

`crates/windfall-dsp/src/rack_note.rs` — written, not executed:

- `color_mapping_routes_channels_and_keeps_source_color_and_expression_gain`
- `splitter_boundaries_drop_outside_zones_and_optionally_preserve_pitch`
- `key_mapping_expands_a_bounded_chord_and_clamps_at_keyboard_limits`
- `scaler_applies_the_power_curve_to_each_property_in_its_normalized_space`
- `envelope_attaches_an_independent_per_note_shape_with_exact_linear_segments`
- `sequencer_modes_and_all_note_modules_round_trip_saved_state`

### Project

`crates/windfall-project/tests/modular_rack.rs` — written, not executed:

- `nested_rack_preset_and_project_round_trip_preserve_ids_wires_and_history`
- `cycles_type_mismatch_missing_ports_and_stale_edits_are_atomic`
- `macro_automation_follows_its_owner_and_removed_owner_is_undoable`
- `bounded_graphs_refuse_excess_nodes_depth_duplicate_ids_and_note_explosion`
- `legacy_projects_omit_racks_and_duplicate_channels_copy_an_independent_graph`
- `latency_changing_control_wire_is_refused_before_publication`
- `channel_preset_capture_keeps_nested_racks_and_loading_plain_sound_removes_them`

### Engine (registered in tests/engine/main.rs)

`crates/windfall-engine/tests/engine/modular_rack.rs` — written, not executed:

- `synth_effect_wires_match_independent_audio_and_disconnect_silences_output`
- `parallel_paths_match_an_independently_delayed_dry_and_wet_reference`
- `nested_mixed_sources_and_parent_latency_match_compiled_metadata`
- `gain_envelope_audio_matches_independent_per_frame_voice_gain`
- `live_expression_updates_reapply_property_scaling_and_preserve_gain`
- `sequence_and_arpeggio_audio_match_independent_schedules_across_block_sizes`
- `note_overflow_controls_reset_and_processing_do_not_touch_the_allocator`
- `color_and_keyboard_outputs_deliver_only_matching_routes_to_real_instruments`
- `chord_children_release_by_parent_without_stopping_same_key_siblings`
- `mute_solo_and_macro_wires_move_the_audible_processor`
- `maximum_graph_processes_without_allocation_and_refuses_one_more_node`
- `channel_graph_matches_live_offline_and_reopened_project_audio`
- `graph_replacement_preserves_live_parent_release_and_retires_off_callback`
- `musical_gate_retimes_mid_note_and_arpeggio_clock_survives_leader_release`
- `bounded_refusals_are_visible_in_realtime_frames_and_do_not_double_count`

### Mounted editor

`apps/desktop/src/features/modular/editor.test.tsx` — written, not executed:

- `adds a node, connects a signal wire, disconnects and undoes through the document`
- `refuses a cycle atomically and reports it in the mounted editor`
- `round-trips a nested rack preset with stable identities and wires`
- `saves mute and solo and makes removal undoable`
- `mounts and saves the distinct %s controls` (six cases: colorRouter, envelope,
  propertyScaler, keySplitter, keyMapper, sequencer)
- `retains the final parameter drag value as one undoable gesture`
- `exposes held-chord sequence modes and persists the selection`

`apps/desktop/src/lib/ipc/sim/document.test.ts` adds a small valid
`setChannelRack` example to EVERY_COMMAND. Existing note-map, preset and native
capture suites must also run; they have not been executed here.

## Integrator commands (not executed here)

Run from the integrated checkout in Git Bash with node/pnpm on PATH. The binding
script supplies the MSVC environment itself. For Rust tests, first source
`scripts/msvc-env.sh` and set TS_RS_EXPORT_DIR to an isolated directory so ts-rs
exports do not write incidental generated files into the checkout:

```bash
source scripts/msvc-env.sh
rack_exports="$(mktemp -d)"
export TS_RS_EXPORT_DIR="$(cygpath -w "$rack_exports")"
cargo test -p windfall-dsp --lib notemap
cargo test -p windfall-dsp --lib rack_note
cargo test -p windfall-project --test modular_rack
cargo test -p windfall-project --test presets
cargo test -p windfall-engine --test engine modular_rack
cargo test -p windfall-engine --test engine
cargo clippy -p windfall-dsp -p windfall-project -p windfall-engine -p windfall-ipc --all-targets -- -D warnings
cargo fmt --all --check
bash scripts/gen-bindings.sh
bash scripts/build-sim.sh
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
pnpm --dir apps/desktop test src/features/modular src/lib/ipc/sim/document.test.ts src/features/channel-rack/inspector.test.tsx --maxWorkers=4
cargo test -p windfall-desktop --lib session::tests
```

Complete normal integration checks and live/offline listening after these cases.
The broad native session suite covers capture and lifecycle regressions even
though hosted graph nodes remain unavailable. Commands are instructions for the
single integrator, not evidence that any check passed.

## Generated artifacts

Regenerate the bindings barrel and descriptor/automation fixture artifacts via
`scripts/gen-bindings.sh`, then simulator WASM and metadata via
`scripts/build-sim.sh`. No generated artifacts were hand-edited.

New or changed exported shapes:

- RackGraph, RackNode, RackProcessor, RackEndpoint, RackEdge, RackPreset,
  ChannelRack.
- RackNoteModule, NoteProperty, PropertyScale, EnvelopeDestination,
  NoteEnvelope, SequenceMode, NoteSequence.
- Referenced ColorMapParams, KeyMapParams, KeySplitParams, LevelScaleParams,
  GridStep and StepGridParams. Tables/chords/grid use TS array views while Rust
  still validates fixed lengths; key-split preserveKey is explicitly camelCase.
- ProjectSettings.modularRacks; Command.setChannelRack including optional
  expected graph; ChannelPreset.rack; RealtimeFrame.rejectedRackEvents.

## Shared seams touched

New dedicated logic lives in DSP rack_note.rs, project rack.rs, engine modular.rs
and frontend features/modular. Integration edits are limited to these seams:

- DSP: src/lib.rs; src/notemap/{color_map,key_map,step_grid}.rs.
- Project: src/{lib,model,check,command,lower,automation,preset}.rs;
  src/lower/preset_apply.rs.
- Engine: src/{lib,plan,state,project_preparation,rack,automation,shared,
  processor,controller}.rs; tests/engine/main.rs. No layout.rs edit.
- IPC: crates/windfall-ipc/src/lib.rs.
- Frontend: features/channel-rack/inspector/index.tsx;
  lib/automation/targets.ts; lib/store/realtime.ts;
  lib/ipc/sim/document.test.ts (EVERY_COMMAND).
- Seam documentation: docs/integration/seams/notemap.md and new
  docs/integration/seams/modular-rack.md.

## Risks, in order to check

1. Nothing was compiled. Check Rust borrowing/traits, all widened literals,
   generated TS export shapes, generic selects and React/compiler lint first.
   New frontend files have not been formatted by a runner.
2. Execute independent latency and audio references, live/offline/reopen,
   muted/solo/control paths and callback allocator guards. Graph replacement
   retriggers held voices and resets tails/delays, which can click. The original
   source DSP is still processed when AudioInput is disconnected (extra CPU).
3. Saved validation counts terminal fanout rather than all transit events or
   combined nested fanout. Runtime bounds are enforced and visible, but stronger
   prepublication aggregate admission remains necessary. Limits: 16 racks per
   project; 64 saved nodes and 256 wires per graph including descendants; depth
   four; 1024 flattened wires; 256 transit events; 1024 child owners; 64 generator
   seats per rack. Verify adversarial/nested fanout and refusal accounting.
4. Compensation admission permits at most one second of latency and at most
   16 seconds / 1,048,576 stereo frames of ring storage per rack (about 8 MiB).
   This is not charged to an aggregate project resource budget; CPU/memory soak
   has not been performed.
5. Run musical gate/tempo/leader-removal and same-key owner tests. Envelope paths
   retain only their final shape. Note-module parameter automation is absent.
6. Finish hosted lifecycle and auxiliary built-in asset/port coverage before
   declaring the rack row complete. Finish native channel/rack preset file and
   library UI exposure; model/session capture alone does not complete that UX.

Implementation commits begin at 64efebf0. The source pass was committed in small
file groups throughout; the final hand-off commit follows ac0e346f. The overseer
owns parity status and the single integrator owns regeneration and execution.
