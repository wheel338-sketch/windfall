# Round five script/Tempo hand-off - edit-only

Worktree: `.claude/worktrees/r5-script-tempo`; branch: `gpt/r5-script-tempo`.
Base: `469be35c`. Implementation tip before this hand-off: `8e20ab59`.

Implementation commits:
- `ecf535df`: project script addresses, checked edits/bounds and Tempo link admission.
- `41379c76`: raw-note identity, dynamic script scheduling, integrated Tempo and export clocks.
- `d00a693f`: installed parameter controls, menus, target lookup and contracts.
- `26aeceaa`, `8e20ab59`: deleted-owner refusal, input retirement and suspended-target UI cleanup.

Nothing was built or run as software evidence: no Cargo, package command,
compiler, test suite, app, generator, install, download or device execution.
Single-file `rustfmt --check` was used only for permitted source inspection;
formatting differences remain for the integrator's formatting pass. Early bare
rustfmt calls used its default edition and reported existing 2024 let chains;
subsequent checks used `--edition 2024 --config skip_children=true` and reported
formatting differences without parse diagnostics. Git whitespace and UTF-8
source inspections were read-only. All tests below are written, NOT EXECUTED.
Hooks were disabled for commits. No dependencies, sub-agents, push, PR, tag,
parity edits, generated bindings, WASM or simulator changes.

## fx-vfx-script

Implemented candidates for every assigned round-four Still-open item:

- `crates/windfall-engine/src/script_notes.rs`, `processor.rs`, `voice.rs`,
  `channel_voice_runtime.rs`, `internal_controllers.rs`: UI NoteOn and actual
  HardwareNote messages use prepared fixed bytecode. A 256-input/4096-output
  ledger records original channel/key/origin and press ID, including plain keys
  held before assignment. Scripted outputs retain that ID through layer/chord,
  echo and instrument-owner paths. FIFO original releases release surviving
  instances and cancel pending copies; edits do not reevaluate old copies.
  Gates end at musical ticks or the original release, whichever comes first.
  Hardware epoch panic and Stop cancel pending origins without heap work.
  Removed channels retire their input/pending records. Capacity/evaluation
  failures reject the whole input and publish the existing note-path counter.
- `project/src/scripting/{library,vm}.rs`, `automation.rs`, `lower.rs`,
  `controller_links/mod.rs`, `file.rs`: append-only address registry keyed by
  channel/script ID/declaration name. Reserved InstrumentParam namespace is
  `0x80000000 + index`; native/rack prefixes are untouched. Reordering a new
  revision's declarations does not renumber a saved address. Removal leaves
  tombstones and prunes affected automation/links in the same undo transaction.
  Checked SetInstrumentParam validates declared bounds, materializes defaults,
  and uses the normal history/gesture/save path. Missing channel owners refuse
  edits/targets. Legacy files derive missing addresses on load.
- `engine/src/{automation,plan,state,performance}.rs`: runtime parameter arrays
  serve normal automation, pattern events and internal links. Linked/automated
  scores sample at each original onset using the same fixed VM as raw notes.
  Delayed copies retain sampled values. Performance launching also samples
  once and schedules bounded copies/gates. Interval proofs reserve maximum
  declared ends, including short-clip tails, for a stable loop/render extent.
  Causal/time-bounded automation admission refuses earlier-than-input onsets,
  zero-crossing divisors and unbounded time. Unautomated legacy assignments
  retain their original baked score behavior and stored values.
- Desktop `features/assistant/{script-editor,script-parameters,controller-binding}.tsx`,
  `lib/script-parameters.ts`, `lib/automation/targets.ts`: installed ParamControls
  use checked edits/gestures and offer automation/internal/hardware link menus;
  controller script target choices include script parameters. Suspended owners
  are hidden. A ref-backed refusal readout uses existing realtime telemetry.
  `docs/SCRIPTING.md` describes gates, identity, limits and parameter timing.

Exact new tests:

`crates/windfall-engine/src/script_tempo_tests.rs`, registered by the appended
child module in `src/processor.rs`:
- `raw_ui_and_hardware_script_releases_keep_original_press_identity_across_edits`
- `raw_script_gates_delayed_copies_and_queue_independent_panic_allocate_nothing`
- `scripted_midi_output_trace_uses_transformed_keys_and_original_release_frames`
- `a_plain_key_held_before_script_assignment_releases_before_its_new_scripted_press`

`crates/windfall-engine/src/script_notes.rs`, existing registered unit module:
- `script_input_capacity_failure_cancel_and_parameter_updates_allocate_and_free_nothing`

`crates/windfall-engine/src/performance.rs`, existing registered unit module:
- `launched_script_parameters_sample_once_and_preserve_delayed_copy_gates`

`crates/windfall-engine/tests/engine/script_notes.rs`, already registered in
`tests/engine/main.rs`:
- `script_parameter_automation_and_internal_links_match_manual_notes_live_and_offline`
- `automated_script_delay_and_gate_keep_short_clip_tails_and_reserved_export_extent`

`crates/windfall-project/src/scripting/tests.rs`, existing registered unit module:
- `script_parameter_addresses_are_checked_append_only_undoable_and_reopen_with_tombstones`
- `script_time_extent_is_proven_over_parameter_bounds_and_invalid_division_is_refused`
- `legacy_parameterized_baked_scores_load_and_unsafe_time_automation_is_refused_atomically`

Frontend:
- `apps/desktop/src/features/assistant/script-editor.test.tsx`:
  `edits installed script parameters through checked history and exposes their automation menu`
- `apps/desktop/src/lib/script-parameters.test.ts`:
  `keeps saved indices across declaration order changes and excludes tombstones`

These assert actual owner keys, exact release/gate/queued-copy behavior, a MIDI
byte/frame trace, zero allocation/free counts, atomic refusal/history, registry
order, deletion/undo, save/reopen, independent manually authored PCM and export
extent. No identified missing implementation remains for the assigned cell;
compilation and acceptance are pending, so the row is not promoted to done.
EXTERNAL-ONLY after acceptance: physical UI/MIDI device delivery/hotplug,
listening and third-party hosted/rack destination behavior on actual devices.

## wf-internal-controllers

Implemented candidates for the assigned Tempo gap:

- `project/src/internal_controllers.rs`: Tempo is admitted as a project Param
  target. Existing map bounds, normalized source/formula containment and
  absolute-map rules still apply; other zero-delay graph cycle checks remain.
- `engine/src/{internal_controllers,sequencer,plan,processor}.rs`: a linked
  Tempo selects an integrated unwarped musical clock on 64-frame grid epochs.
  Effective tempo stays within each declared map interval, including smoothing
  from a static seed outside it. Synced sources use the preceding grid's tempo;
  previously computed source endpoints drive the next grid, avoiding algebraic
  feedback. Notes/gates/curves, clip boundaries/fades, automation, metronome and
  synced processors share the clock. Performance advances with effective Tempo.
  Compatible edits retain source/clock state. Play/seek or changed Tempo links
  begin a fresh epoch; loops retain continuous phase. Unlink remaps endpoints
  and restores the static map without moving musical position.
- `engine/src/{render,stems}.rs`: finite transport end is measured from the
  actual clock; waveform/streaming exports use that duration plus latency/tail.
  The positive minimum bounds progress. Isolated To-master stems capture the
  full-mix Tempo trajectory off audio and replay it, so detector input changes
  cannot change stem timing. Track-output stems share the full mix processor.
- `features/controllers/menu.ts`: actual Tempo parameter menus offer internal
  sources. Existing checked commands/editors save and undo the link. The full
  clock/native-tape/seek/loop contract is in `docs/CONTROLLER-LINKS.md`.

Exact new tests:

`crates/windfall-engine/src/script_tempo_tests.rs`:
- `synced_tempo_feedback_is_bounded_integrates_exactly_and_export_has_the_measured_length`
- `tempo_seek_is_a_deterministic_new_epoch_and_unlink_restores_the_static_clock`
- `tempo_smoothing_stays_inside_declared_bounds_with_an_outside_static_seed`

`crates/windfall-engine/tests/engine/internal_controllers.rs`, existing registered module:
- `feedback_tempo_matches_live_offline_audio_notes_automation_and_loop_partitions`
- `isolated_stems_replay_the_full_mix_audio_dependent_tempo_clock`

`crates/windfall-project/tests/internal_controllers.rs`, existing integration target:
- `synced_source_tempo_links_are_checked_bounded_undoable_and_saved`

Evidence written includes independent integrated-tick/end-frame and metronome
references; source previous-grid assertions; bounds with 5-second smoothing and
an outside seed; callback allocator guards; sample-exact note/audio/automation
PCM across 7/37/251/1024 partitions and loops; deterministic seek/unlink;
streaming/full-render equality; and an audio-dependent isolated-stem reference.
No identified missing implementation remains for the assigned cell. All build,
execution and acceptance gates remain pending; do not promote the row yet.
EXTERNAL-ONLY after acceptance: listening, physical audio/MIDI timing and
non-Windows/third-party hosted tempo-sync delivery.

## Shared-file edits / merge notes

No `rack.rs`, `modular.rs`, rack asset/host processor files, asset packaging or
native session APIs were changed. Preserve the sibling's rack/host additions.
The shared seams edited are:

- Project `src/{automation,file,internal_controllers,lower}.rs`,
  `src/controller_links/mod.rs`, `src/lower/preset_apply.rs` (only preserving the
  reserved script namespace during instrument/rack replacement), and script
  library/VM. SetChannelRack's pruning keeps high script addresses; lower
  SetInstrumentParam checks that namespace before rack/plugin/native lookup.
- Engine `src/{automation,channel_voice_runtime,internal_controllers,performance,
  plan,processor,render,sequencer,state,stems,voice}.rs`.
  NoteEvent adds `script_input` and `script_tail`; new literals need None for
  ordinary events, including the existing performance fixture. Runtime Note
  adds `input`; ordinary literals need None. Plan/PlanState add prepared scripts
  and parameter mapping; Plan also carries an optional stem Tempo replay.
  Do not overwrite incoming asset/host/rack preparation fields when merging.
- Existing engine/project acceptance modules and the assistant/controllers/
  automation target UI seams listed above. `internal_controllers/tests.rs`
  changes only existing Note/Runtime literals for the new fields.

No persisted enum, Command, effect/instrument kind, descriptor parameter or
choice list was reordered or removed. No dependency was added. New generated
exports are `ScriptParameterAddress` and the optional ScriptLibrary address
field; regenerate Rust bindings and WASM on the integration checkout before
frontend execution. This worker did not edit generated files.

## Integrator checks - NOT RUN

Use the repository's normal MSVC environment and isolated TS_RS_EXPORT_DIR.
First format merged source, then ordinary workspace all-target checking and
strict Clippy. Focused execution:

```sh
cargo test -p windfall-project --lib scripting
cargo test -p windfall-project --test internal_controllers
cargo test -p windfall-project --test controller_links
cargo test -p windfall-project --test presets
cargo test -p windfall-engine --lib script_notes
cargo test -p windfall-engine --lib script_tempo_tests
cargo test -p windfall-engine --lib performance
cargo test -p windfall-engine --test engine script_notes
cargo test -p windfall-engine --test engine internal_controllers
cargo test -p windfall-engine --test engine realtime
cargo test -p windfall-engine --test engine sequencing
cargo test -p windfall-engine --test engine automation
cargo test -p windfall-engine --test engine stems
cargo test -p windfall-engine --test engine timeline
scripts/gen-bindings.sh
scripts/build-sim.sh
pnpm --dir apps/desktop test src/features/assistant/script-editor.test.tsx src/lib/script-parameters.test.ts src/features/internal-controllers src/features/controllers --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```

Then the integrator's full Rust/frontend/native-window and registry-prefix
acceptance gates. Every new case is non-ignored; existing assertions, tolerances,
timeouts, tests and deadline limits are unchanged.

## Least certain spots

1. Nothing was compiled. Check shared NoteEvent/Note/Plan/Runtime literal fields,
   serde/ts-rs optional exports, type inference in script registry/default value
   edits, borrow boundaries in adoption/launcher scheduling, and strict Clippy.
   Single-file rustfmt checks do not establish type correctness.
2. Run the exact PCM/clock assertions first: rounding at grid/pass boundaries,
   short-window transformed tails, metronome floor handling, new seek epochs,
   static-map unlink and isolated-stem trajectory capture/latency are the most
   consequential joins. Source/detector epoch semantics are documented; seeks
   do not reconstruct historical external/detector audio input.
3. New queues are fixed-size and failure counters are visible. The ordinary
   unassigned raw-note path now records press order as well; stress the 256-input
   admission limit and old echo/mono/layer/hardware-panic regressions. No new
   CPU admission or deadline result is claimed.
4. Rebuild bindings/WASM before mounted UI tests. Check slider keyboard gestures,
   Base UI context menu accessible names, async refusal display, declaration
   parsing of checked source, and cleanup for deleted/suspended channels.
5. Script time automation is intentionally causal and requires a finite declared
   bound. Unautomated legacy assignments retain baked placement; automated loop
   extents reserve declared maxima, including possible silence after a shorter
   chosen gate. Review these documented contracts in acceptance.

HAND-OFF COMPLETE
