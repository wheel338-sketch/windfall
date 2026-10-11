# Audio analysis / ML implementation hand-off — 2026-10-11

Branch: `gpt/w3-analysis-ml`, base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
Worker follows the common brief: edits only, no builds/tests, no agents, no
downloads, no external dependencies, no generated bindings/WASM edits.

## Implementation in progress

Classical kernels: `crates/windfall-analysis/src/classical.rs`.
Persistent estimates, overrides, warp anchors and noise-profile controls:
`crates/windfall-project/src/audio_analysis.rs`, optional default-omitted
`SampleAsset.processing`, checked `SetSampleProcessing` command. Original
samples referenced by recipes cannot be removed. Existing project and stretch
workspace crates are reused by analysis; Cargo.lock dependency edges reflect
those additions. No new external crate or model weights.

Written, not executed: `crates/windfall-analysis/tests/classical_tools.rs`:
`calibrated_note_pitch_duration_and_neutral_render`,
`repeated_and_held_notes_silence_and_polyphonic_chord`,
`glide_vibrato_and_noise_keep_bounded_estimates`,
`warp_order_bounds_overrides_and_cancel_are_explicit`,
`learned_profile_improves_independent_snr_and_dry_is_exact`.

These assertions are acceptance targets, not measurements. The SNR fixture
requires >5 dB improvement and <1.5 dB tone RMS error; no result is claimed.
The existing calibrated pitch suite is retained unchanged.

## Native/UI integration (written, not executed)

`audio_editor/classical_editor.rs` connects detection, saved proposal reopening,
cached dry/wet audition, cancellable spectral render and existing prepared
publication. `audio_analysis_send_midi` converts selected note endpoints through
`SongClock`, sharing the engine's compiled tempo lane, validates meter metadata,
rejects overflow/resolution/pattern bounds and batches new pattern/channel/notes
and playlist clip in one history step. Source/estimate provenance is immutable.
`features/audio-editor/analysis-tools.tsx` mounts pitch/score, time/warp and noise
views with pointer/keyboard edits, draft history, preview, cancellation and
reviewed insertion. The existing editor's tempo-automation refusal remains:
the clock itself is tested against an analytic ramp, but opening automated-tempo
audio clips still needs an editor view contract extension.

Model adapters opt into `InferenceTask`: exact four stem roles or separate
general/vocal dereverb roles with preserved channels. `NoteInferenceAdapter`
is the future typed score seam. No production adapter is installed.
`analysis_preview` claims a ready ticket while verifying/decoding an output;
the lease restores Ready after audition. Selected role import can create
separate aligned playlist rows and mixer inserts, optionally muting the original
mix, as one batch. UI exposes role preview/selection and a remix layout step.

Additional tests, all **written, not executed**:

- `session/tests/classical_analysis.rs`: `classical_pitch_edit_one_step_undo_save_reopen_keeps_estimates_and_original`,
  `audio_score_review_inserts_calibrated_notes_as_one_persisted_undo_step`,
  `classical_cancel_at_prepared_publication_preserves_project_and_review`,
  `immutable_estimates_bad_warp_and_stale_source_cannot_apply`,
  `noise_profile_dry_wet_and_apply_preserve_source_and_saved_profile`.
- `session/tests/analysis_jobs.rs`: `analysis_stem_review_preview_selected_remix_layout_is_aligned_and_one_undo`,
  `analysis_dereverb_modes_reach_adapter_and_refuse_mismatched_model_roles`,
  `analysis_unknown_duplicate_or_empty_stem_selection_leaves_ready_review_unchanged`.
  These intentionally copy authored PCM: lifecycle/layout evidence only.
- `windfall-project/tests/audio_analysis.rs`: `processing_metadata_legacy_defaults_history_reopen_and_retained_source`,
  `shorter_warp_duration_and_crossing_note_anchors_are_checked_atomically`.
- `windfall-engine/tests/analysis_clock.rs`: `audio_seconds_follow_tempo_ramp_and_meter_labels_do_not_change_clock`.
- `audio-editor/analysis-tools.test.tsx`: independent estimate/override editing,
  draft undo/audition payload; crossing warp rejection/profile wet preview;
  selected score apply; late detection after replacement.
  `analysis-edit.test.ts`: mapping/order/ratio constraints.

## Remaining work

Integration verification, interval-boundary listening/continuity, selected-range
and richer MIDI onset evaluation, model/runtime licence evaluation. All seven
rows remain partial until final scope and integrator evidence are reconciled.

## Integrator commands (worker has not run any of them)

Use Git Bash with `source scripts/msvc-env.sh`, one Cargo owner, and a temporary
native `TS_RS_EXPORT_DIR`.

```sh
cargo test -p windfall-analysis --locked --offline --test classical_tools
cargo test -p windfall-analysis --locked --offline --test pitch_analysis
cargo test -p windfall-project --locked --offline
cargo clippy -p windfall-analysis -p windfall-project --all-targets --locked --offline -- -D warnings
cargo test -p windfall-engine --locked --offline --test analysis_clock
cargo test -p windfall-desktop --locked --offline classical_analysis
cargo test -p windfall-desktop --locked --offline analysis_jobs
pnpm --dir apps/desktop test src/features/audio-editor src/features/analysis --maxWorkers=1
```

Check first: type export of the recipe/Clip references, new sample literals,
neutral render identity and independent SNR/frequency assertions. No listening,
hardware, cross-platform or model-quality evidence exists for this change.
