# Round-five rack assets and hosted providers — edit-only hand-off

Worktree: `C:/Users/ewhee/.t3/projects/windfall/.claude/worktrees/r5-rack-assets`.
Branch: `gpt/r5-rack-assets`. Base: `469be35c274f4e3d3dd47b6163f9da9609ed1c4e`.

Implementation commits:

- `6b18322a265217df12ac17080c895ac36c18f877`: node ownership, portable assets, native providers, compensation, capture, retirement and deterministic acceptance sources.
- `4c0e6e2f4c2a9ed621cb3e90b8c07951732e63ce`: mounted existing asset/native editors, file review/relinking and updated seam.
- `5279f254dd42ee80857bab266eee95094243ee61`: default original piano bank preparation and legacy Scratch missing-source/relink UI.
- `ed2da13012583bf6d83e5a9fe263f59f3ffe313e`: owned native fixture state through actual session save/open.

Nothing was built or run: no builds, tests, application, benchmarks, generators,
package commands, installs or downloads. Allowed source inspection, git and
single-file `rustfmt --check --edition 2024` were used. The rustfmt checks parsed
the inspected source and exited 1 for formatting differences; they are not
compilation or passing-test evidence. `git diff --check` was clean. Commit hooks
were disabled for these commits to honor the prohibition on running build/test
hooks. No dependencies, generated bindings/WASM, simulator artifacts, parity
files, other worktrees, sub-agents, push, PR or tag were changed/created.

## Per-row delivery

| Row | Changed mechanism/files | Written acceptance | Completion and remaining work |
| --- | --- | --- | --- |
| `core-fl-studio-mobile-rack-fx` | Project `rack_assets.rs`, rack/document/edit/preset ownership; engine `modular_assets.rs`, modular/plan/rack/state/message lifecycle; frontend modular asset/audio editors plus existing instrument/effect ownership adapters; native preset review and portable container tests. | P1/P2, E1–E4, D1, N1/N2, U1/U2 below. | Assigned asset ownership/preparation/loading/capture/packaging mechanisms are implemented candidates. Inline PCM/IR/spectra/banks travel with the rack; missing payloads and legacy Scratch references are reported and editable/relinkable. SOFTWARE: No additional missing behavior identified for the assigned Still-open cell. Integration compilation, generation and all tests remain mandatory and unexecuted; this is not a row promotion. EXTERNAL-ONLY: physical audio/MIDI, musical listening and other-OS runtime acceptance. |
| `fx-patcher` | All asset files above, appended rack plugin target, graph-owner canonicalization and native edit routing; `modular_provider.rs` over the existing factory/bridge; desktop native role/capture/editor retirement; existing provider insertion facade and PluginControls; prepared latency and outgoing generations. | P1–P3, E1–E4, D1, N1–N7, U1–U3 below. | Assigned hosted provider/preparation/state/editor/removal/retirement and asset mechanisms are implemented candidates. Owned CLAP/VST3 fixture cases cover audio, latency, state, history, render isolation and helper failure. Same-format/id provider selection at a new path preserves saved state and parameters for relinking. SOFTWARE: No additional missing behavior identified for the assigned Still-open cell. Integration compilation, generation and all tests remain mandatory and unexecuted; this is not a row promotion. EXTERNAL-ONLY: physical devices, listening and other-OS runtime/editor acceptance. |

## Ownership and realtime details

`RackNode.asset` and `RackNode.hosted` are serde-defaulted optional fields.
Assets own instrument PCM/spectra/drum programs, IRs, Scratch audio, mapped
sample zones with inline audio, or SF2 bytes plus bank/program. Zone mappings
cannot retain a source project's SampleId. Existing curves stay serialized in
params and are pinned by document/history/plan/runtime owners. The rack payload
and opaque-state bound is 64 MiB; text preset reads use the existing 256 MiB
preset envelope ceiling. Native containers carry the same graph payloads.

Control preparation uses existing standalone instrument/bank/SF2/IR/source
paths. The default rack PianoKeys node receives the same original version-one
bank as a standalone channel. Missing bank/audio payloads are silent with saved
names and relink controls. Native file review also includes nested plugin paths.
The library loads missing-payload graphs and displays their review warnings so
users can repair them in the owning module.

`PluginTarget::Rack { channel, path: [u32; 4], instrument }` is appended. Zeroes
pad the nested path. SetChannelRack, channel duplication and preset application
canonicalize native targets. Native state, parameter and wrapper commands edit
the checked graph owner. Project native inventories/capture include nested
bindings. Provider selection uses existing `plugins_insert` and scanner/runtime
discovery; DSP uses existing PluginFactory, HostedInstrument, HostedEffect and
ExternalEffect. No separate native loader or process bridge was introduced.

Actual prepared latency feeds rack merge/output rings and the containing
channel's existing compensation key. Child native identity/latency records remain
separate from that aggregate. Native nodes/retirees stay on the audio lane and
receive existing transport/control boundaries. Native per-note gain uses the
existing expression bridge. Failure keeps independent graph processing active
and exposes the existing target reason through PluginControls.

Families admit at most 64 pending/active/outgoing generations. Shared 256 MiB
payload/runtime reservations cover IR workspace, fixed seats, native dry
history and compensation; another shared 256 MiB budget covers decoded banks.
Outgoing racks move into preallocated seats, receive releases and no new input,
and finish DSP voices/tails before moving through the existing garbage queue.
DSP/assets/native facades/reservations drop on control. Native retirees release
capture/editor selection immediately; owner maintenance closes their editors
off audio. Unowned topology disconnections retain their prior audible behavior.

Native nodes advertise their main stereo/note bus and existing native parameter
controls. Built-in descriptor control wires/auxiliary aliases do not masquerade
as native parameters/buses. Parent-channel attachment restrictions remain the
existing ones; hosted and sample-bank processors are nodes inside an instrument
parent. Script/tempo work belongs to the sibling and was not implemented here.

## Exact new tests — written, not executed

Project: `crates/windfall-project/tests/modular_rack.rs`:

- P1 `rack_node_assets_are_owned_portable_checked_and_undoable_with_legacy_defaults`
- P2 `rack_sample_zones_preserve_missing_sources_and_relink_as_one_history_step`
- P3 `nested_hosted_node_owners_capture_state_parameters_presets_and_removal_history_without_aliasing` (also asserts same-identity relinking retains opaque state and native values).

Engine: `crates/windfall-engine/tests/engine/modular_rack.rs`, already registered
by `tests/engine/main.rs`:

- E1 `owned_rack_impulse_matches_standalone_convolver_pcm_and_latency_without_callback_allocation`
- E2 `owned_rack_sample_bank_matches_standalone_and_missing_bank_relinks_to_measured_audio`
- E3 `replaced_owned_rack_ir_finishes_its_delayed_audio_and_retires_without_callback_free`
- E4 `rack_default_piano_prepares_the_standalone_original_bank_before_first_note`

Engine unit module: `crates/windfall-engine/src/modular_assets.rs`, registered as
`modular::assets`:

- D1 `rack_generation_admission_counts_pending_and_retired_owners_until_control_drop`

Native: `apps/desktop/src-tauri/src/session/tests/rack_assets.rs`, newly
registered by `session/tests/mod.rs`:

- N1 `rack_native_container_carries_owned_ir_to_another_project_directory_without_source_files`
- N2 `rack_preset_review_reports_missing_embedded_bank_and_relink_keeps_history`
- N3 `clap_owned_fixture_as_rack_node_matches_standalone_and_preserves_state_history_and_editor_owner`
- N4 `vst3_owned_fixture_as_rack_node_matches_standalone_and_preserves_state_history_and_editor_owner`
- N5 `crashed_owned_rack_helper_keeps_parallel_builtin_audio_and_a_visible_target_reason`
- N6 `clap_owned_instrument_node_matches_standalone_note_and_release_pcm`
- N7 `vst3_owned_instrument_node_matches_standalone_note_and_release_pcm`

N3/N4 compare full offline PCM with the plugin alone, actual rack latency,
full live/offline PCM after removing each reported pipeline delay, captured
state through project JSON/native session save/open/rack presets, add/remove
history, render-versus-editor ownership and the fixture's real no-GUI refusal.
N3–N7 are Windows-gated, not ignored. They reuse the existing owned production
helper fixture via `plugins::n4_manager`; supply `WINDFALL_BRIDGE_FIXTURE` and
`WINDFALL_DESKTOP_BRIDGE_HELPER` exactly as for the existing N4 host suites.

Mounted UI: `apps/desktop/src/features/modular/editor.test.tsx`:

- U1 `loads an owned IR in the real rack convolver editor and carries it through library history`
- U2 `edits the real waveshaper curve on its rack node with undo`
- U3 `edits a saved native node parameter and restores its owner on undo while reporting browser capability`

Existing assertions/tolerances/registration remain intact. Existing RackNode
literals in project/engine modular tests, engine script_notes and native
files_qa receive default-absent asset/hosted fields only.

## Shared-file edits for the integrator

Primary new files: project `src/rack_assets.rs`; engine `src/modular_assets.rs`
and `src/modular_provider.rs`; frontend modular `asset-editor.tsx`,
`audio-input.tsx`, `hosted-editor.tsx`; instrument `editor-owner.tsx`; effects
`asset-owner.tsx`; native `session/tests/rack_assets.rs`.

Shared project files: `src/lib.rs`, `rack.rs`, `plugin.rs`, `check.rs`,
`document.rs`, `edit.rs`, `lower.rs`, `lower/preset_apply.rs`, `preset.rs`,
`controller_links/mod.rs` and `tests/modular_rack.rs`.

Shared engine files: `src/modular.rs`, `rack.rs`, `state.rs`, `plan.rs`,
`plugins.rs`, `message.rs`, `tests/engine/modular_rack.rs` and the default fields
in `tests/engine/script_notes.rs`. Preserve the script/tempo sibling's additions
when merging the modular preparation signature, flat-node fields, RackNode
literals, graph lowering and compensation records.

Shared desktop Rust files: `files_commands.rs`, `plugins/runtime.rs`,
`plugins/bridge.rs`, `session/presets.rs`, `session/tests/mod.rs`,
`session/tests/plugins.rs` (fixture helper visibility and exhaustive target arm)
and `session/tests/files_qa.rs` (default fields). Preserve the hosting sibling's
dynamic capability/latency/restart work around the appended target role helper,
capture inventory, native expression gain and retired editor selection.

Shared frontend files: modular `editor.tsx`, `library.tsx`, `editor.test.tsx`;
plugins `controls.tsx`, `store.ts`, `wrapper-controls.tsx`; channel-rack
`inspector/generic-instrument-editor.tsx` and instruments `use-instrument-binding.ts`,
`key-drum-editor.tsx`, `layer-kick-editor.tsx`, `number-editor.tsx`,
`resynthesis-editor.tsx`, `snapshot-editor.tsx`; effects
`nonlinear/editors.tsx`, `nonlinear/performance-editors.tsx`,
`spectral/convolver-editor.tsx`, `spectral/impulse.ts`.

Also changed `crates/windfall-sim/src/preset_files.rs` for exhaustive target
capture/naming and `docs/integration/seams/modular-rack.md` for this contract.

## Least certain spots and required integration

1. No compiler/typechecker ran. Regenerate bindings and the real document WASM
   before frontend checks. New exported types: RackAudioAsset, RackSampleZone,
   RackNodeAsset; changed RackNode and PluginTarget. Check context adapter types,
   computed parameter updates, native gain fields, trait-object forwarding and
   borrow splitting during capture/graph compilation. Run normal formatting.
2. Merge native target/factory/compensation edits with the dynamic-host sibling.
   Verify aggregate channel records, child ownership, actual latency changes,
   stopped/bypassed control boundaries and old editor retirement. Run all
   existing ownership/capture/bridge and rack regression suites, not only new
   tests. New native PCM assertions use unchanged 1e-6 precision; do not loosen
   them if buffer/event scheduling exposes a defect.
3. Run the allocator and delayed-tail cases after integration. Check retained
   generations under dense edits, queue backpressure, stop/reset and sample-rate
   replacement. Verify budget accounting and the 64-seat/family admission seam.
4. Native fixture tests require the currently built desktop helper and owned
   fixture DLL, using the existing explicit environment injection. No fixture
   was built here. UI file/curve/native tests require freshly regenerated Rust
   artifacts; old WASM cannot establish this delivery's behavior.

Focused gates for the integrator: project `--test modular_rack` and preset/plugin
regressions; engine `--lib modular::assets` and `--test engine modular_rack`;
desktop `session::tests::rack_assets` plus existing files_qa/plugins/ownership
gates; frontend modular, effects spectral/nonlinear, instrument asset editors
and plugin controls/wrappers; then normal workspace tests, strict Clippy,
typecheck/lint, binding audit and macOS/Linux cross-checks. All are unexecuted.

HAND-OFF COMPLETE
