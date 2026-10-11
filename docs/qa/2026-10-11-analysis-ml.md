# Audio analysis / ML implementation hand-off — 2026-10-11

Branch: `gpt/w3-analysis-ml`, base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
Worker follows the common brief: edits only, no builds/tests, no agents, no
downloads, no external dependencies, no generated bindings/WASM edits.

## Committed implementation

Restart recovery: worktree was clean at `747d98f3`. Kernels/schema are in
`0ffd0fd5`; native/UI integration and model-output workflows are in
`747d98f3`. No build, test or binding generator was run.

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
Added `corrected_note_duration_pitch_and_stereo_seams_match_waveform_reference`
and `stationary_hum_profile_preserves_tone_stereo_and_linear_wet_mix`.
Written, not executed: pitch checks use interpolated waveform crossings,
independent of the pitch detector, with a <10-cent target; hum SNR target is
>8 dB, and a transformed boundary has a <0.1 adjacent-sample-step target.
Render intervals now use 120 ms source context and up to 8 ms linear
crossfades, shrinking for short neighbouring intervals. Neutral render is an
exact buffer bypass. Strongest-channel and output scans checkpoint Work.

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
reviewed insertion. Follow-up extends editor views to the exact playback tempo
map: playlist edges and fades follow the map, while source offsets use stored
tempo and audio retains its source speed. `AudioViewTiming` freezes the
detection-time view in the saved recipe so later tempo changes cannot change
the source under retained estimates. Legacy recipes without timing remain
readable. Clip duration and trim placement use the same inverse map.

Model adapters opt into `InferenceTask`: exact four stem roles or separate
general/vocal dereverb roles with preserved channels. `NoteInferenceAdapter`
is the future typed score seam. No production adapter is installed.
`analysis_preview` claims a ready ticket while verifying/decoding an output;
the lease restores Ready after audition. Selected role import can create
separate aligned playlist rows and mixer inserts, optionally muting the original
mix, as one batch. UI exposes role preview/selection and a remix layout step.
Model-job capture and partial-range placement now use the same tempo-aware
view/clock as classical editors. Complete outputs retain the original clip
edges; partial outputs retain aligned source-frame origins. Missing-adapter UX
explicitly names licensed model/runtime requirements.
Canonical actions also validate task/model roles and empty stem selections
before submission/apply; native validation remains final.

Additional tests, all **written, not executed**:

- `session/tests/classical_analysis.rs`: `classical_pitch_edit_one_step_undo_save_reopen_keeps_estimates_and_original`,
  `audio_score_review_inserts_calibrated_notes_as_one_persisted_undo_step`,
  `classical_cancel_at_prepared_publication_preserves_project_and_review`,
  `immutable_estimates_bad_warp_and_stale_source_cannot_apply`,
  `noise_profile_dry_wet_and_apply_preserve_source_and_saved_profile`.
  Added `audio_score_under_tempo_ramp_uses_absolute_onsets_and_offsets`;
  pitch persistence also reopens/auditions after a tempo change. Written, not executed.
- `session/tests/analysis_jobs.rs`: `analysis_stem_review_preview_selected_remix_layout_is_aligned_and_one_undo`,
  `analysis_dereverb_modes_reach_adapter_and_refuse_mismatched_model_roles`,
  `analysis_unknown_duplicate_or_empty_stem_selection_leaves_ready_review_unchanged`.
  These intentionally copy authored PCM: lifecycle/layout evidence only.
- `windfall-project/tests/audio_analysis.rs`: `processing_metadata_legacy_defaults_history_reopen_and_retained_source`,
  `shorter_warp_duration_and_crossing_note_anchors_are_checked_atomically`.
- `windfall-engine/tests/analysis_clock.rs`: `audio_seconds_follow_tempo_ramp_and_meter_labels_do_not_change_clock`.
  Expanded with independently integrated clip durations/fades and stored-tempo
  offsets. Existing native audio-editor tempo-refusal case now checks supported
  opening without mutation. Written, not executed.
- `audio-editor/analysis-tools.test.tsx`: independent estimate/override editing,
  draft undo/audition payload; crossing warp rejection/profile wet preview;
  selected score apply; late detection after replacement.
  `analysis-edit.test.ts`: mapping/order/ratio constraints.
  Follow-up: frontend mapping now merges override edges exactly like the native
  renderer; conflicting shared boundaries and combined ratios fail before
  audition. New markers inherit the current mapping. A selected note can reset
  to its estimate. The added mapping/shared-boundary case is written, not executed.
  Native previews now expose an optional preserved-source envelope via
  `AudioAnalysisPreview.sourcePeaks`, so the frontend can select source
  coordinates independently of a reopened derived clip.
  The mounted editor now owns that source waveform/range. Added preserved-source
  selection and real browser-backend refusal cases, written, not executed.

## Remaining work

Integration verification, interval-boundary listening/continuity, selected-range
and richer MIDI onset evaluation, model/runtime licence evaluation. All seven
rows remain partial until final scope and integrator evidence are reconciled.

## Model/runtime decision required

No production inference adapter or weights are installed. Importing a manifest
does not install executable code. Existing explicit import checks checksum,
licence metadata, roles and bounded sizes; missing adapter/model returns an
offline failure, not copied audio. Deterministic copies exist only in tests.
Model registrations currently need explicit manifest reimport after restart,
although cache bytes are retained.

- **Demucs v4**: upstream [repository](https://github.com/facebookresearch/demucs)
  and [MIT licence](https://github.com/facebookresearch/demucs/blob/main/LICENSE).
  The archived upstream uses Python/PyTorch; `htdemucs` targets four stereo
  stems at 44.1 kHz. A packaged CPU implementation must pin preprocessing,
  segment overlap, gain handling and output alignment. Per-stem automatic
  rescaling must not silently change the reviewed mix balance. No ONNX export
  compatibility is assumed. Decide exact fork/version, model bytes/checksum,
  separate weight provenance/licence, runtime distribution and memory budget.
  The current 64 MiB pack cap may need an audited increase for chosen weights.
- **RNNoise**: [repository](https://github.com/xiph/rnnoise) and
  [BSD-style three-clause licence](https://github.com/xiph/rnnoise/blob/main/COPYING).
  Its C runtime targets 48 kHz mono speech noise suppression. It is a possible
  optional voice denoiser, not a music separator or either dereverb mode.
  Pin model provenance and frame contract, channel/resampling/gain policy,
  distribution notices and music/stereo damage evaluation. Upstream setup
  downloads model data; none of those scripts were run here.
- **Basic Pitch**: [repository](https://github.com/spotify/basic-pitch) and
  [Apache-2.0 licence](https://github.com/spotify/basic-pitch/blob/main/LICENSE).
  TensorFlow reference models have ONNX/CoreML/TFLite conversions; upstream
  Windows uses ONNX. Input is downmixed and resampled to 22.05 kHz. A selected
  runtime must match the reference windowing, stitching, onset/note/contour
  postprocessing and tested conversion. [ONNX Runtime's MIT licence](https://github.com/microsoft/onnxruntime/blob/main/LICENSE)
  alone does not approve a particular binary, wrapper or model. Decide pinned
  model/runtime versions, weight licence and redistribution notices. The
  classical spectral heuristic makes no Basic Pitch quality claim.
- **Dereverb**: no trained dereverb candidate is selected in the roadmap.
  Supply licensed general and vocal weights (or a demonstrably mode-aware
  model), preprocessing/output contracts and bounded CPU adapter. Evaluate
  tail reduction, dry-signal damage and stereo coherence on authored dry/reverb
  fixtures. Denoising cannot stand in for dereverberation.

The user must select and approve exact redistributable model/runtime artefacts
or explicitly import a supported licensed pack. An implementer must then add
the corresponding production adapter and independent quality evaluation.
`core-stem-separation`, `win-playlist-deverb` and `wf-remix-a-song`
remain open until that happens. No dependency or weights were added.

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
