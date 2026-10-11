# Internal control sources — implementation hand-off

Branch: `gpt/w3-control-sources`. Base: `87b87bc2439ec076d0f75c5b4d997c9db1faba95` (`feat/parity-100`). Implementation and contract through `50120317`; this hand-off is committed separately afterward. Use branch HEAD for the complete delivery.

**Edit-only delivery:** no builds, tests, package tools, generators, benchmarks or audio/MIDI device checks were run. All acceptance additions listed below are **written, not executed**. Generated files and parity tracker files were not edited. No new dependency, push, PR or sub-agent.

## Per-row claims

A full-row candidate means the software path and acceptance cases are implemented; the integrator must compile, regenerate and run them before the overseer can close the row. These claims are not passing-test evidence.

| Row | Claim | Implemented behavior | Remaining |
| --- | --- | --- | --- |
| `core-fruity-envelope-controller` | Full-row candidate | Note-channel input, sample-frame note starts, ADSR or saved drawn envelope/duration, normalized output, destination links and dedicated curve editor | Integration validation; references in engine unit/integration tests below |
| `core-fruity-keyboard-controller` | Full-row candidate | Piano-roll/input pitch and velocity ports, latest-held-key fallback, fixed note table and 5 ms glide, note-input editor | Integration validation; real device input remains external |
| `core-fruity-voltage-controller` | Full-row candidate (software scope) | Eight smoothed controls, default-off arming, affine sample calibration, explicit spare physical output routes and device-free multichannel harness/editor | Integration validation; DC coupling, volts, physical calibration and voltage safety are external and never claimed |
| `fx-fruity-formula-controller` | Full-row candidate | Bounded a/b/c evaluator, checked expression installation, arithmetic containment, actual destination modulation and dedicated expression/error editor | Integration validation; intentionally no arbitrary code |
| `fx-fruity-peak-controller` | Full-row candidate | Actual slot-input stereo peak/RMS follower, independent LFO output and combined port, free Hz or beat sync, detector/sync editor | Integration validation; conservative detector graph may refuse silent-but-structurally-cyclic graphs |
| `fx-fruity-x-y-controller` | Full-row candidate | Real pointer/keyboard XY pad, normalized ports, one gesture undo, automatable/recordable axis parameters | Integration validation, especially native simultaneous pointer-axis recording (mounted cases record axes separately) |
| `fx-fruity-x-y-z-controller` | Full-row candidate | XY pad plus Z control, three normalized ports, gestures/automation, dedicated XYZ editor | Integration validation, including Z recording and simultaneous axes in native app |
| `fx-control-surface` | Full-row candidate | Blank user-built panel, up to eight named knobs/sliders/buttons, configurable layout, persistent widget/parameter values and outgoing links | Integration validation; fixed eight-port capacity is explicit |
| `inst-dashboard` | Full-row candidate | Distinct instrument with configurable panel, eight internal ports, actual assignable MIDI CC output using existing MIDI-out implementation | Integration validation; real MIDI device acceptance external |
| `wf-internal-controllers` | Partial | Existing mixer/channel creation, source choices in parameter context menus, normalized destination maps/ranges/smoothing, source link editor, source/target deletion with atomic undo, cycle refusal and save/reopen | Internal Tempo target is explicitly refused; the clock-map architecture needs a realtime clock feedback contract before “any control” is complete |

Source rows use the shared link contract; its current Tempo exclusion also applies to their links. Internal sources support project Param targets with absolute maps and no pickup. Hardware Slot/Action/relative contracts are preserved. No modular rack was built; stable port descriptions are reusable by its owner.

## Implementation and semantics

- Reused registered DSP foundations for envelope, keyboard, formula, pads and surface. Added KeyboardSource, FormulaSource and VoltageSource effect registry entries, and Dashboard instrument entry. New enum/list entries are appended; c is appended after Formula's old a/b/base/amount descriptors. Existing descriptor indices are retained.
- Optional `ProjectSettings.internal_controllers` stores drawn points, note channel, expression, RMS/sync choice, surface widgets and DC routes. Empty side tables are omitted. `Command::SetInternalControllerSettings` is appended and has an EVERY_COMMAND fixture.
- Project checks bound configurations/sources to 256, drawn points to 64 and widgets/DC routes to eight. Formula programs are compiled off the callback (256 bytes, 64 operations, 16 nesting levels). Malformed edits leave installed data/history intact. Source-specific settings, layout overlap, route conflicts and finite numeric ranges are checked.
- Stable ports: envelope; keyboard pitch/velocity; peak+LFO and LFO; formula result; X/Y[/Z]; eight surface/Dashboard/DC outputs. Owners use the shared allocator's effect/channel ids. See `docs/CONTROLLER-LINKS.md` and `crates/windfall-project/src/internal_controllers.rs`.
- Source evaluation uses real slot input per sample and preallocated node storage. Hosted sources preserve carrier audio and declare no audio tail; standalone DSP audio-processing APIs remain available. Links shape the preceding endpoint on the common 64-frame grid, after ordinary automation, with exponential smoothing and existing target ramps/glides. Multiple links to one destination apply in stored order.
- Note tables are fixed at 128 per source; overflow is counted through `Controller::rejected_control_notes`. Note starts honor their frame, latest held key wins, stop/disconnect release appropriate origins, and sequencer/tempo-map clock shifts move ends. Compatible plans carry source/link state and fader ramps without taking an extra smoothing step. Adoption respects mute/solo audibility changes. Changing source configuration resets its node state.
- Cycles include scalar owner dependencies and detector feedback through upstream tracks/sends, output taps and sidechain gains. Current-track detector dependencies are conservative because selection can change their audio. No gain/silence assumption is used to permit a feedback loop.
- Source deletion prunes outgoing links and configuration inside the existing transaction; target deletion prunes incoming links. Undo restores them together. Moving an effect retains its source id. Kind replacement removes missing ports and incompatible settings. Removing a surface widget leaves its stable output port/link intact. Removing a link restores its target's stored setting on plan adoption.
- DC calibration is in normalized audio samples: `clamp(zero + scale * value, -1, 1)`. Routes reserve physical channels against audible routes and other DC sources. DC is collected separately from audible stereo. Ordinary stereo offline export does not contain DC. `Processor::set_output_channels`, `hardware_samples` and `unavailable_dc_routes` expose software checks; real hardware behavior is unverified.
- Editors cover each source's actual purpose, including drawn envelope, installed/draft formula and refusal, peak/RMS/sync, pointer pads, custom widgets, Dashboard MIDI assignments and DC route calibration. Pad/curve right-click does not edit values. Sliders end gestures on release/capture loss/blur/commit. Link edits/removal match source and destination in the latest document, and advertise the checked 5000 ms smoothing maximum.

## Acceptance inventory — written, not executed

### DSP: `crates/windfall-dsp/src/control/tests.rs`

New cases:

- `formula_three_inputs_division_power_and_errors_match_reference`
- `rms_follower_attack_and_release_match_exponential_power_reference`
- `dc_calibration_and_five_ms_smoothing_are_bounded_and_exact`
- `dashboard_registry_sends_real_assignable_midi_cc_without_audio`

Existing foundation cases also cover envelope attack/sustain/release/retrigger, keyboard pitch/velocity, formula bounds, XY/XYZ processing, parameter persistence and block invariance. Exhaustive kind fixtures were extended in `crates/windfall-dsp/tests/dsp/realtime.rs`; the pinned kind count was updated in `tests/filter_family_registry.rs`.

### Engine unit: `crates/windfall-engine/src/internal_controllers/tests.rs`

- `drawn_envelope_starts_at_exact_note_frame_and_interpolates_the_saved_points`
- `hosted_adsr_attack_matches_analytic_exponential_and_note_off_releases`
- `tempo_synced_and_free_lfo_ports_share_the_same_smoothed_phase_and_reference`
- `keyboard_overlap_falls_back_and_glides_pitch_and_velocity_in_five_samples`
- `held_note_table_is_bounded_and_origin_release_and_clock_moves_are_precise`
- `dashboard_values_normalize_real_midi_cc_and_xy_xyz_ports_match_parameters`

### Engine integration: `crates/windfall-engine/tests/engine/internal_controllers.rs`

Registered in `tests/engine/main.rs`:

- `internal_surface_link_uses_automation_range_and_live_offline_are_identical`
- `bounded_formula_really_modulates_a_host_destination`
- `peak_source_reads_real_isolated_track_audio_and_is_block_invariant`
- `linked_pad_axes_and_smoothing_follow_the_normalized_reference`
- `dc_routes_generate_calibrated_samples_only_on_chosen_spare_channels`
- `keyboard_notes_drive_velocity_without_an_audible_controller_signal`
- `removing_link_returns_target_to_its_stored_value_without_callback_allocation`
- `all_hosted_sources_leave_real_carrier_audio_unchanged_without_links`
- `reversed_formula_mapping_and_grid_smoothing_match_independent_exponential_reference`
- `note_triggered_envelope_modulates_real_audio_identically_for_callback_partitions`
- `unchanged_link_survives_unrelated_plan_edit_between_grid_boundaries_without_a_gain_dip`

Realtime kind fixtures in `tests/engine/realtime.rs` include all appended sources. Integration cases use real oscillator audio, destination modulation and independent normalized/range or exponential references. Allocation guards cover prepared callback processing. Live/offline equality and callback partitions are written assertions, not measured results yet.

### Project: `crates/windfall-project/tests/internal_controllers.rs`

- `every_source_has_checked_stable_normalized_ports_and_saved_settings`
- `source_deletion_unlinks_and_removes_configuration_in_the_same_undo_step`
- `direct_and_indirect_formula_input_cycles_are_refused_atomically`
- `peak_audio_feedback_through_formula_and_upstream_gain_is_refused`
- `malformed_configuration_preserves_project_history_and_installed_formula`
- `legacy_default_omits_configuration_and_missing_sources_are_refused`
- `dashboard_layout_and_internal_link_save_reopen_and_unlink_on_channel_deletion`
- `drawn_envelopes_layout_and_dc_calibration_reject_unbounded_or_conflicting_data`
- `source_specific_settings_and_nonfinite_duration_are_refused_without_history_changes`

Existing `tests/controller_links.rs` fixture now uses a real XY source instead of an EQ pretending to be one.

### Mounted UI: `apps/desktop/src/features/internal-controllers/editors.test.tsx`

- `mounts purpose-built %s editor` (noteEnvelope, keyboardSource, formulaSource, envelopeFollower, voltageSource)
- `pad %s moves both axes in one undo gesture` (xyPad, xyzPad)
- `formula editor persists three-input expression and shows a refusal without overwriting it`
- `envelope editor stores the actual drawn curve in one edit`
- `peak editor switches real detector mode and musical LFO sync`
- `control surface starts blank and persists a configurable named widget`
- `voltage editor adds an explicit spare output route and reports the hardware gate`
- `internal linking is offered in browser menus and ranges are editable`
- `dashboard mounts configurable panel with real assignable MIDI controls`
- `pad %s axis edits are recordable into real saved automation curves` (xyPad, xyzPad)
- `pad right-click leaves values and undo history untouched`

Recording cases use the real document/recorder with simulated transport realtime delivery, and record X and Y separately through keyboard edits. They do not prove simultaneous pointer recording or native transport/device behavior. Exhaustive parameter-kind access fixtures and EVERY_COMMAND were updated separately.

## Shared seams touched

Full changed-file inventory is available with `git diff --name-only 87b87bc2439ec076d0f75c5b4d997c9db1faba95 HEAD`. New logic lives in dedicated internal controller modules/editors/tests.

- DSP: `src/effect.rs`, `src/instrument.rs`, existing `src/control/{mod,envelope,formula,keyboard}.rs`; registry/realtime test fixtures. No DSP `lib.rs` edit.
- Project: `src/{model,command,lower,check,lib}.rs`, `src/controller_links/mod.rs`, existing controller-links test fixture.
- Engine: `src/{automation,controller,lib,mixer,plan,processor,rack,sequencer,shared,state}.rs`, `tests/engine/{main,realtime}.rs`. `rack.rs` only hosts control slots; no modular rack feature was added.
- Frontend: `features/effects/effect-editor.tsx`, `features/channel-rack/inspector/instrument-section.tsx`, `features/controllers/menu.ts`, `features/params/access.test.ts`, `lib/ipc/sim/document.test.ts` EVERY_COMMAND.
- Contract documentation: `docs/CONTROLLER-LINKS.md`.

No Rust IPC/Tauri/session seam changes, new IPC method or RealtimeFrame field. Existing commands/parameter gestures carry the new configuration and controls.

## Regenerate after integration

Do not hand-edit generated output. Run the repository binding/descriptors and simulator scripts once on the integration branch.

- New exported types: `ControlPoint`, `SurfaceControlKind`, `SurfaceControl`, `InternalControllerSettings`, `VoltageRoute`; newly registered `KeyboardSourceParams`, `VoltageSourceParams`.
- Changed exported types: `ProjectSettings` (`internalControllers`), `Command` (`setInternalControllerSettings`), `EffectKind`/`EffectParams` (formulaSource, keyboardSource, voltageSource), `InstrumentKind`/`InstrumentParams` (dashboard), `FormulaSourceParams` (appended c).
- Regenerate the binding barrel, descriptors/automation fixtures, WASM simulator and metadata. Existing bindings/WASM on this worker branch predate these additions, so frontend validation requires regeneration first.

## Integrator commands — not run here

Use Git Bash with MSVC environment. Set `TS_RS_EXPORT_DIR` to an absolute native path in your task-specific temporary directory so ts-rs tests do not unexpectedly edit the working tree.

```sh
source scripts/msvc-env.sh
export TS_RS_EXPORT_DIR='C:/Users/ewhee/AppData/Local/Temp/wf-tools/control-sources-ts'
cargo test -p windfall-dsp --lib control
cargo test -p windfall-dsp --test dsp realtime
cargo test -p windfall-dsp --test filter_family_registry
cargo test -p windfall-project --test controller_links
cargo test -p windfall-project --test internal_controllers
cargo test -p windfall-engine --lib internal_controllers
cargo test -p windfall-engine --test engine internal_controllers
cargo test -p windfall-engine --test engine realtime
scripts/gen-bindings.sh
scripts/build-sim.sh
pnpm --dir apps/desktop test src/features/internal-controllers src/features/params/access.test.ts src/lib/ipc/sim/document.test.ts --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```

Review generated descriptor ordering and old saved automation compatibility. Then check native mounted editors, save/reopen, undo/delete, stop/disconnect, and pointer X/Y plus Z automation recording with real transport. Device gates are separate from these software tests.

## Risks and remaining gates, in check order

1. Nothing was compiled. First check exhaustive registry/match arms, shared seam merges, generated TS optional fields and exact command shapes. Keep additions appended when reconciling other workers' kinds/commands.
2. Run engine reference/allocation/live-offline cases. In particular inspect source evaluation before slot passthrough, grid order/one-grid delay, cached linked native values and gain-ramp transfer during plan adoption (including mute/solo). Test fixtures now use ProjectSettings.name and f32-aware UI tolerances.
3. Run mounted/type/lint cases after fresh bindings/WASM. New editor files were written manually under the package-tool freeze. Inspect gesture cleanup and simultaneous pointer axis recording: the browser recorder's existing in-flight writing guard may drop a second concurrent axis edit; separate keyboard recording cases do not cover it. Native session recording also needs acceptance. Source scalar readouts were not added to RealtimeFrame.
4. Tempo is the known functional umbrella gap: `AutomationTarget::Tempo` compiles into a plan clock map and its runtime automation setter does nothing. This stream refuses the link rather than silently storing an ineffective destination. A future realtime clock node must preserve note/audio scheduling and address LFO/envelope clock feedback. Do not mark the umbrella row done.
5. Detector cycles deliberately use conservative audio reachability, including Current selection and sidechain gain. Source configuration changes reset node state. Surfaces/Dashboard have eight fixed ports; deleting a widget does not delete its port's links. Confirm these semantics in integration UI.
6. DC harness tests prove only normalized software samples and channel selection once run. Available channel count, callback block sizing through the existing hardware buffer, actual DC coupling, hardware voltage calibration and voltage safety need external verification. No device/safe-voltage claim is made. Dashboard hardware MIDI acceptance is also external.

All work is committed file-group by file-group. Final read-only status/whitespace inspection is recorded in the final report; no build or test substitutes were used.
