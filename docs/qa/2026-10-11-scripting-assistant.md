# Assistant and scripting implementation hand-off

Branch: `gpt/w3-scripting-assistant`, base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.

Implementation checkpoint after application restart. All tests in this stream are **written, not executed**.
The worker is edit-only: no build, test, dependency installation, generation,
device access or network request was performed. No provider was selected.

Implementation scope: checked, revision/generation guarded command proposals;
offline help/intent provider; bounded Rust scripting with piano/controller
workers and a fixed realtime note-transform subset. No Python source compatibility.

The integrator owns generated bindings/WASM and validation.

## Row claims at this checkpoint

| Row | Claim | Implemented and remaining |
| --- | --- | --- |
| core-gopher | Full-row candidate, pending validation | Provider trait, deterministic offline help/intent provider, native and WASM checked proposals, mounted review/apply/guarded undo. User must decide model/service, credentials, privacy and offline fallback policy before a model-backed adapter is added. No provider SDK, dependency or network calls. |
| win-piano-scripting | Full-row candidate for Windfall Script v1, pending validation | Fixed bounded Rust interpreter, selected-note/generator API, parameters, cancellable native worker, previews, atomic review/apply/undo, saved immutable revision references and editor. This language does not execute Python source. |
| wf-midi-scripting | Partial | Existing controller stream/profile links feed bounded script actions, normalized target mapping and opt-in CC feedback on the native MIDI worker. Editor installs CC channel-1/Master-volume mappings; persisted API supports broader checked sources/targets. Hardware/profile/device feedback acceptance remains external. |
| fx-vfx-script | Partial | Fixed allocation-free note evaluator, immutable channel references and sequenced plan compilation shared by live playback/export, editor assignment, independent render/evaluator fixtures. Modular rack worker must hook up this channel transform seam. Hardware audition and automation transforms remain unsupported. |

## Tests and validation

All the following are **written, not executed**. Never infer successful compilation or runtime evidence from this hand-off.

- `crates/windfall-project/src/scripting/tests.rs`: interpreter semantics, line diagnostics, source/code/stack/parameter/output/step limits, cancellation, selected and generated notes, fixed-transform equivalence, library/revision persistence, controller fixtures and checked proposal rollback/staleness/recording exclusions.
- `crates/windfall-project/src/assistant.rs`: local intents/help and invalid input.
- `apps/desktop/src-tauri/src/session/tests/scripting.rs`: `assistant_review_apply_guarded_undo_and_stale_preparation_are_atomic`, `invalid_proposal_rolls_back_ids_history_and_replacement_invalidates_approval`, `piano_worker_selected_fixture_preview_apply_undo_cancel_and_save`, `controller_script_pump_maps_actions_and_refuses_partial_output`, `recording_and_worker_busy_exclusions_are_explicit`.
- `crates/windfall-engine/src/script_notes.rs`: `fixed_transform_success_and_failure_allocate_and_free_nothing`.
- `crates/windfall-engine/tests/engine/script_notes.rs`: independent manually transformed audio score, irregular live blocks, reopen/export equality and callback allocator counters.
- `crates/windfall-engine/src/midi_hardware/tests.rs`: `installed_script_feedback_is_bounded_owned_by_worker_and_epoch_guarded`; synthetic native port harness, not hardware evidence.
- `apps/desktop/src/features/assistant/{panel,script-editor}.test.tsx`: mounted local help, review/apply/undo, stale/recording rejection, late-response dismissal, script parameters/errors, saved revision/assignment and selected-note preview.
- `apps/desktop/src/lib/ipc/sim/proposals.test.ts`: Rust WASM authority rejects malformed, oversized, stale and recording-excluded proposals without edits. `document.test.ts` registers `setScriptLibrary` in `EVERY_COMMAND`.

Integrator commands (from repository root; initialize the repository's documented Windows Rust/MSVC environment first):

```sh
scripts/gen-bindings.sh
scripts/build-sim.sh
cargo test -p windfall-project --lib scripting
cargo test -p windfall-project --lib assistant
cargo test -p windfall-engine --lib script_notes
cargo test -p windfall-engine --lib midi_hardware::tests::installed_script_feedback
cargo test -p windfall-engine --test engine script_notes
cargo test -p windfall-desktop --lib session::tests::scripting
pnpm --dir apps/desktop test src/features/assistant src/lib/ipc/sim/proposals.test.ts src/lib/ipc/sim/document.test.ts --maxWorkers=4
```

Before accepting rows, also run the repository's required project persistence/history, controller/recording, typecheck and lint checks. Verify package/script names against manifests when integrating; this worker cannot execute them.

## Shared seams and generation

Project: `src/{model,command,lower,check,lib}.rs`; engine: `src/{lib,plan}.rs`, MIDI hardware request worker, `tests/engine/main.rs`; desktop native: `commands.rs`, `session/{mod,edit,controllers}.rs`, `session/tests/mod.rs`; WASM simulator: `src/lib.rs`; frontend: IPC `backend/tauri/mock.ts`, simulator document and `EVERY_COMMAND`, layout `register-actions/app-shell`, action menus. New behavior otherwise lives in assistant/proposal/scripting modules.

Regenerate Rust TS exports and the WASM simulator, including `Command::SetScriptLibrary`, optional `ProjectSettings.scripts`, `ProposalStamp`, `CommandProposal`, `AssistantReply`, `ScriptParameter`, `ScriptDiagnostic`, `ScriptNote`, `ScriptKind`, `ScriptReference`, `ScriptAsset`, `ChannelScript`, `ControllerScript`, `ScriptLibrary`, `PianoScriptRequest`, `PianoScriptPreview`. Never manually edit generated files.

## Risks in integration order

1. Nothing compiled: check new Rust/TS shapes, Tauri argument registration and existing session ticket/history APIs first; run generation before frontend checks.
2. Check strict lint and mounted component conventions, then batch atomicity, revision races, worker admission/cancellation and recording exclusions.
3. Native feedback can fail after the internal command transaction committed. It emits a warning; physical MIDI cannot be rolled back. Disconnected feedback is preflighted and queued packets are hardware-epoch guarded.
4. Note transformation currently runs while preparing immutable sequenced event plans, not on hardware audition; rack integration remains another worker's responsibility. Invalid transforms are rejected on checked project edits; unchecked engine fixtures fail open.
5. Finish the script API documentation and source-only fixture review at the next checkpoint. Validate real MIDI feedback, actual recording races and provider decisions externally. No network, audio device or other-OS evidence exists.
