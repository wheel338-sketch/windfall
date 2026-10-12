# Round four scripting hand-off — edit-only

Worktree: `.claude/worktrees/r4-scripting`; branch: `gpt/r4-scripting`;
base: `cfd2d37e`. Source tip before this hand-off: `f8a57c31`.

Nothing was built or run: no executable, build, check, test, lint, formatter,
generator, dependency install/download, browser or device execution. Only source
reading/editing and git operations/whitespace inspections were performed. Commit
hooks were disabled to respect the machine's no-build/no-test rule. No sub-agents,
push, PR, tag, dependency, generated binding/WASM, simulator or parity edits.
Every fixture below is **written, not executed**. No row is promoted to done.

## wf-midi-scripting

The acceptance cell's source/target editor and gesture/pickup/smoothing/capture
work is implemented as a candidate for integrator validation.

Changed files:

- `crates/windfall-project/src/scripting/{library,controller,tests}.rs`: persisted
  checked `ControllerScript.map` appended with a legacy immediate-takeover default;
  existing source/target/action capability validation; normalized mapped output
  retained for session delivery. No descriptor, choice, kind or Command reorder.
- `apps/desktop/src-tauri/src/session/{controllers,scripting,edit}.rs`: interpreter
  remains outside document/hub locks, with 5000 steps per event and a cooperative
  100-ms pump deadline. Script map/tempo outputs enter the existing link states;
  smoothing advances during idle pumps, pickup indicators persist, actual stored
  values are recorded through the existing song/pattern capture/touch path, and
  600-ms movements/recording passes use ordinary gesture IDs. Batch admission binds
  generation/revision; feedback still preflights the native output and rechecks
  the committed stamp. Event/output overflow refuses the batch. Rejected script
  glides are discarded, including recording/publication refusals. Unrelated
  ordinary transport/clock actions retain their separate semantics.
- `apps/desktop/src/features/assistant/{controller-binding,script-editor}.tsx`,
  `features/controllers/links-panel.tsx`: device/channel wildcards or exact device
  name/channel; CC, note/pad, full-resolution wheel and pressure; checked project
  parameters, positional slots, existing action targets, pickup, smoothing,
  absolute/toggle/relative modes and feedback channel. The existing target registry
  includes hosted parameters and rack control inlets. Install/removal remain
  reviewed Rust commands with undo; Edit controller assignment restores the saved
  source, parameters and mapping. Live errors, pickup holds and drops are visible.
- `session/tests/scripting.rs`, `features/assistant/script-editor.test.tsx` and
  `docs/SCRIPTING.md`: recorded-byte-stream and mounted persistence fixtures/docs.

Exact new tests:

- `crates/windfall-project/src/scripting/tests.rs`:
  `legacy_controller_bindings_default_to_immediate_takeover_and_new_maps_roundtrip`.
- `apps/desktop/src-tauri/src/session/tests/scripting.rs`:
  `recorded_script_stream_pickup_smoothing_gestures_and_reopen_match_link_runtime`;
  `recorded_script_wheel_stream_records_pattern_automation_in_the_same_gesture`;
  `scripted_pad_actions_use_rising_edges_and_over_budget_streams_refuse_the_whole_batch`;
  `recording_refusal_clears_script_glides_without_a_deferred_document_edit`.
  CC, wheel and pad fixtures decode explicit complete MIDI byte packets through
  production `MidiMessage::parse`, then drive the actual Session pump. They assert
  measured pickup/idle hold, exponential smoothing, edges, history, captured curve
  endpoints, refusal atomicity and file reopen. Existing
  `controller_script_pump_maps_actions_and_refuses_partial_output` keeps all its
  assertions and additionally pins unrelated transport-action behavior on failure.
- `apps/desktop/src/features/assistant/script-editor.test.tsx`:
  `reviews a named pressure binding with soft takeover, smoothing and a checked channel target and reopens it`.
  Uses the real WASM document for reviewed installation and file reopen; native
  capability is an explicit fixture, not actual hardware execution.

No missing implementation identified for this row's assigned Still-open cell.
Validation is pending, so this is not software passing evidence.
EXTERNAL-ONLY: physical controller pickup/encoder behavior, port naming/hotplug,
device feedback delivery/firmware and other-OS driver behavior.

## editor-edison

The remaining audio-selection scripting item is implemented as a candidate for
integrator validation.

Changed files:

- `crates/windfall-project/src/scripting/{vm,audio}.rs`: same bounded postfix
  compiler/arithmetic evaluator, with selection-only `gain`, `reverse` and
  `normalize` passes. No host handles/IO, notes, controller actions or loops in the
  audio adapter. Linked stereo gain/peak normalization, frame-pair reverse, finite
  validation, clipping refusal, parameter checks and source-line diagnostics.
- `session/audio_editor.rs` and new `session/audio_editor/script_editor.rs`: one
  admitted script worker shared with piano; five-second cooperative execution
  deadline; 100-million-instruction/sample-visit budget; two-minute/64-MiB caps per
  input/scratch/output buffer; one retained result. Preview writes no file/history.
  Audition uses the cached selection. Apply uses that exact reviewed buffer through
  the existing guarded `audio_editor_commit_with` / `replace_selection` publication
  path, not a rerun. Cancellation is rechecked immediately before mutation; stale
  source/generation/edit and recording exclusion remain inherited. One undoable
  derived-WAV edit, source preservation, redo and fresh save/reopen.
- `crates/windfall-ipc/src/audio_edit.rs`, native `commands.rs`, frontend
  `lib/ipc/{backend,tauri,mock}.ts`, `features/audio-editor/{types,index,script-tools}.tsx`
  (types is `.ts`): `audio_script_preview`, `audio_script_audition`,
  `audio_script_apply`, shared `script_begin`/`script_cancel`; real editor Run
  script, parameter controls, result waveform, audition, Cancel and Apply. Browser
  backend explicitly refuses processing. Original gain-ramp/reverse/normalize/
  linear-fade-in/squared-fade-out examples are shipped in `AUDIO_SCRIPT_EXAMPLES`.
- `docs/{AUDIO-EDITOR,SCRIPTING}.md`, project/native/frontend fixtures below.

Exact new tests:

- `crates/windfall-project/src/scripting/audio.rs` (registered as the VM's child
  module via `#[path = "audio.rs"] mod audio`):
  `audio_scripts_original_examples_have_exact_frames_and_stereo_pairing`;
  `audio_scripts_budget_cancel_invalid_surface_and_clipping_return_no_result`.
- `apps/desktop/src-tauri/src/session/tests/audio_editor.rs`:
  `audio_script_preview_apply_is_one_exact_selection_edit_with_undo_and_fresh_reopen`;
  `audio_script_cancel_during_publication_and_invalid_source_leave_no_history_or_file`.
  Real Float32 source, exact selected PCM and unchanged outside/source bytes,
  no-history/no-file preview, history/redo/fresh session reopen, and deterministic
  cancellation barrier before native publication.
- `apps/desktop/src/features/audio-editor/script-tools.test.tsx`:
  `runs the captured selection, reviews waveform, auditions and applies only the reviewed ticket`;
  `cancels the native ticket and ignores late preview output`;
  `reports browser capability without creating a scripted audio edit`.
  Native replies are declared UI fixtures; DSP/file evidence is the Rust suite.

No missing implementation identified for this row's assigned Still-open cell.
Validation is pending. The execution deadline covers interpreter work, not OS IO,
snapshot cloning or engine preparation. Editor source text is a transient draft;
the applied derived audio, its clip and undo history use normal project storage.
EXTERNAL-ONLY: listening review and other-OS native editor/audio behavior.

## fx-vfx-script — partial, stays in progress

Changed files:

- Project `scripting/{vm,library,tests}.rs`: `copies COUNT KEY_STEP TICK_STEP`,
  maximum 16 outputs, still exactly one final emit and no loops; fixed
  `TransformedNotes` array; atomic key/end bounds checking before output; checked
  scripted clip/tail expansion beyond the song bound refuses the document edit.
  Existing first-output `evaluate` API remains; multi-output hosts use
  `evaluate_notes`. No callback allocation/free/lock/IO added.
- Engine `src/{script_notes,plan,sequencer}.rs`: compile all fixed outputs into
  the channel note event table with original expression/note-source identity.
  Effective patterns grow to transformed ends; repeated clip playback extent
  grows accordingly. Already-admitted scripted notes retain their release tick
  past a short clip window and song/render extent includes tails. Both live
  sequencing and offline rendering consume that same immutable table before
  ordinary instrument/rack/MIDI-out delivery. Authored project placements remain
  unchanged and reopen derives the same effective playback.
- `engine/src/performance.rs`: **one test-literal field only**, `scripted: false`,
  to match extended `plan::NoteEvent`. `engine/tests/engine/script_notes.rs` adds
  direct instrument and existing modular-rack score fixtures. No `rack.rs`,
  `modular.rs`, rack graph/processor module or rack automation mechanism edits.
  Preserve the new `PlanPattern.source_length` and `NoteEvent.scripted` fields
  while integrating the rack worker's changes.

Exact new tests:

- `crates/windfall-project/src/scripting/tests.rs`:
  `bounded_note_copies_preserve_lengths_offsets_keys_and_refuse_partial_results`;
  `scripted_clip_expansion_past_song_bound_is_an_atomic_checked_refusal`.
- `crates/windfall-engine/src/script_notes.rs`:
  `fixed_note_copy_success_and_bounds_failure_allocate_and_free_nothing`.
- `crates/windfall-engine/tests/engine/script_notes.rs`:
  `scripted_short_long_and_duplicated_durations_match_manual_score_live_and_offline`.
  Independent manually authored scores, actual instrument and modular-rack PCM,
  short-window release tails, 7/251/1024-frame partitions, saved reopen and actual
  song callback allocation/free counters. Existing engine integration module is
  already registered in `tests/engine/main.rs`.

SOFTWARE: hardware audition still sends raw `Message::NoteOn` / `HardwareNote`
through `Processor::start`; it needs allocation-free script application and
original-input-key/instance release pairing, including delayed copies and gate
length semantics for held input. This is not a physical-device-only gate.
SOFTWARE: script parameters remain static assignment values. Automatable,
append-only parameter addressing, checked value edits, live sampling and
live/offline parameter automation are not implemented. Do not close this row
based on the other worker's modular-rack parameter automation.
EXTERNAL-ONLY after those software gaps: real MIDI output/controller timing and
third-party hosted-destination behavior. A dedicated transformed MIDI-out byte/
release trace should accompany that remaining join; this round's new duration
fixtures measure instrument/rack PCM, not native output bytes.

## Integrator checks — run serially, not executed here

Regenerate combined bindings/WASM before frontend checks. New exports:
`ControllerScript.map` (serde default preserving legacy takeover),
`AudioScriptRequest`, `AudioScriptPreview`; both request tickets use the existing
`#[ts(type = "number")]` convention. Feature-local audio IPC types mirror those
Rust exports. No new dependencies or Command/descriptor indices.

Use the repository MSVC setup and an isolated `TS_RS_EXPORT_DIR`. Run the new
tests through their registered suites, plus existing controller/capture and
audio-editor regressions:

```sh
cargo test -p windfall-project --lib scripting
cargo test -p windfall-engine --lib script_notes
cargo test -p windfall-engine --test engine script_notes
cargo test -p windfall-engine --test engine modular_rack
cargo test -p windfall-desktop --lib session::tests::scripting
cargo test -p windfall-desktop --lib session::tests::controllers
cargo test -p windfall-desktop --lib session::tests::audio_editor
cargo test -p windfall-desktop --lib session::controller_capture
pnpm --dir apps/desktop test src/features/assistant/script-editor.test.tsx src/features/audio-editor src/features/controllers --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```

Then the integrator's full fmt/strict-Clippy/workspace/native-window gates. The
new code was not formatter-run; apply the integrator's normal formatting pass.
No assertion, tolerance, timeout, skip or ignore was weakened.

## Least certain spots

1. All compilation is pending. Check native child-module privacy/NLL around the
   cached editor result and publication lease, Tauri registrations and ts-rs
   exports first. Regenerate before TypeScript/WASM tests.
2. Controller pass gestures/touch recording and exact clock/pickup/smoothing
   expectations need the real native suite. New tests use synthetic recorded
   packets and a synthetic recording-admission flag, not a device/take.
3. Effective pattern/clip repetition and short-window tail math needs the score
   fixture plus existing sequencing/performance/tempo/custom-PPQ regressions.
   No dedicated native MIDI-out scripted trace was added in this round.
4. Mounted Base UI choice intent, window/token renewal, cancellation ordering
   and React/strict-Clippy/formatting diagnostics remain unexecuted. Browser/UI
   fixtures do not establish native file or PCM execution.
5. Remaining VFX hardware/automation work is real implementation work. This
   hand-off intentionally makes no full-row claim for `fx-vfx-script`.

HAND-OFF COMPLETE
