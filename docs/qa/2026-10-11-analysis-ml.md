# Audio analysis / ML implementation hand-off — 2026-10-11

Branch: `gpt/w3-analysis-ml`, base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
Worker follows the common brief: edits only, no builds/tests, no agents, no
downloads, no external dependencies, no generated bindings/WASM edits.

## Committed implementation

Restart recovery: worktree was clean at `747d98f3`. Kernels/schema are in
`0ffd0fd5`; native/UI integration and model-output workflows are in
`747d98f3`. No build, test or binding generator was run.
Restart follow-ups are committed in small groups through `d10a21f5`:
merged frontend anchors, frozen tempo-aware source timing, contextual seam
rendering, preserved-source selection, tempo-aware model placement, task
validation, remix loading and calibrated score boundary cases.

## Per-row claims

These are implementation claims for integrator review, not accepted parity
statuses or executed evidence. The overseer owns both parity files.

| Row | Claim | Implemented | Remaining gate |
| --- | --- | --- | --- |
| `core-denoising` | Full-row candidate, classical scope | Learned per-channel stationary profile, Wiener-style gains, linked source retention, wet/dry preview, saved recipe and one undo step | Execute independent hiss/hum SNR and signal-preservation fixtures; listening and memory validation |
| `editor-newtone` | Full-row candidate, classical scope | Immutable detected notes, separate pitch/onset/offset overrides, pointer/keyboard editor, draft history, contextual spectral render, save/reopen | Execute calibrated correction/history/mounted cases and listen to short/voiced boundaries |
| `editor-newtime` | Full-row candidate | Detected transient slices, editable ordered markers and duration, ratio/combined-anchor limits, preview/render/history/reopen | Execute marker/duration/tempo-map cases and boundary listening |
| `wf-audio-to-midi` | Full-row candidate within declared classical limits | Mono YIN and sparse spectral polyphony, selected-note review, RMS dynamics, exact tempo-map endpoint conversion, one-step piano-roll/channel/playlist insertion | Execute onset/offset/pitch/velocity and tempo/meter cases; no dense-mix or neural accuracy claim |
| `core-stem-separation` | Partial; open | Explicit checksum/licence pack import, four-role adapter contract, retained review, per-stem preview/select, aligned playlist/mixer batch | Choose licensed runtime/weights, implement production adapter and evaluate real separation |
| `win-playlist-deverb` | Partial; open | Distinct general/vocal modes, exact role/channel checks, preview/review/one-step retained import | Choose mode-aware licensed weights/runtime and evaluate actual dereverb |
| `wf-remix-a-song` | Partial; open | Song picker/import entry point, stem-task wizard, preview/select, aligned separate rows/inserts and optional original mute | Production separation dependency; long-song tiling/rate conversion for chosen runtime |

Classical scope: finite mono/stereo at 8–192 kHz, up to 120 seconds and 64 MiB
per audio buffer, <=4096 notes, <=512 base warp markers, ratios 0.25–4,
correction <=24 semitones. Detection targets 50–1500 Hz. Short sources below
the calibrated YIN support return no guessed notes but still support time/noise
controls. Score velocities are RMS-derived dynamics, not measured MIDI
performance velocity; polyphonic dynamics use a shared strongest-channel cell.
Detection covers the current complete clip view, and insertion reviews selected
notes. Spectral polyphony targets sparse tonal mixtures with <=8 simultaneous
roots: harmonics/octave unisons, percussion and dense music can cause errors.
Glide/vibrato estimates are retained as note means; there is no pitch-contour
automation or formant-preservation quality claim. Noise reduction targets
stationary profiles, not arbitrary nonstationary voices or reverb.

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
Polyphonic note gating/dynamics now use the current 10 ms cell rather than
the centered FFT lookahead, preventing future attacks from creating early
notes and past audio from holding offsets through silence. Added
`repeated_score_notes_have_calibrated_onsets_offsets_pitch_and_dynamics`:
two repeated A4 notes, silence, differing amplitudes, mono/poly modes,
50/10 ms boundary targets and bounded RMS-derived velocity. Written, not executed.
Held spectral notes average their pitch/salience estimates instead of pinning
an onset-window estimate. Denoise output scans checkpoint and reject nonfinite
float output before even a neutral recipe can return it.
Inputs shorter than the calibrated pitch support return no monophonic notes
rather than guessed pitch, while remaining usable by time/noise controls.
The short-source neutral case is written, not executed.

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
The `analysis.remix` canonical action starts a wizard by explicitly choosing
and importing a song through the existing audio-import path, then opens the
stem task and guides model/range, preview/select and aligned layout review.
Source load and final stem import are separate undo steps. Cancellation or
project replacement during the file picker cannot attach a stale dialog.
Closing/invalidating the analysis panel stops its audition.

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
- `features/analysis/panel.test.tsx`: selected stem preview and aligned remix
  payload; distinct general/vocal mode submission; mismatched-role refusal;
  real song loading into the wizard; replacement during file picking.
  All written, not executed. Existing checksum/model-absence/cancel/stale
  lifecycle fixtures in M1 are retained.
  `actions.test.tsx` also pins startup registration of the new remix action;
  written, not executed.

## Remaining work

Integrator compilation, acceptance execution, interval-boundary listening,
memory/latency validation and chosen-model quality evaluation remain. No test,
listening, hardware, cross-platform, benchmark or measured quality result is
claimed. The three trained-model rows remain open even if software lifecycle
tests pass. Four classical rows are candidates within the bounds above.

## Accepted product and runtime decisions (overseer follow-up)

- Denoising, pitch/time editors and audio-to-MIDI use the accepted classical
  product scope above. Neural transcription/voice suppression are optional
  future directions, not acceptance gates for these four rows.
- Dereverb uses built-in classical DSP: vocal mode is multichannel STFT WPE,
  general mode is conservative decay-model late-power suppression. No trained
  weights or neural claim are required. Implementation is in progress in this
  follow-up; independent measured-behaviour acceptance is still an integration
  gate. Prior missing-model dereverb copy/claim is superseded by this decision.
- Four-stem separation and remix remain open. ONNX Runtime is the selected
  runtime direction; users import licensed, pinned packs and no weights ship
  in the repository. No `ort` dependency/runtime implementation is added here.
  The ONNX manifest now records opset, named input/output tensor shapes,
  symbolic dimensions, sample rate, segment/overlap frames and ordered stem
  roles. Pack metadata is atomically persisted beside the cache and restored
  after restart with byte checksums reverified. A corrupt/absent pack reports
  an offline error; it is never silently trusted or downloaded.
- Exact four-stem model/export, licensed weight bytes, CPU runtime version,
  preprocessing, stitching and memory/rate budgets still need implementation
  and evaluation. Demucs upstream code is MIT, but its PyTorch reference does
  not establish that a particular ONNX export or weight licence is suitable.
  [Demucs](https://github.com/facebookresearch/demucs),
  [ONNX Runtime MIT licence](https://github.com/microsoft/onnxruntime/blob/main/LICENSE).
  RNNoise (BSD-3-Clause speech denoising) and Basic Pitch (Apache-2.0 note
  transcription) are not four-stem or dereverb solutions and are not required
  for the accepted classical products.

Follow-up registry test: `pack_registry_restart_keeps_onnx_contract_and_rechecks_checksum`
in `session/analysis_jobs/pack_registry.rs`, **written, not executed**.
Registration is bounded to eight packs / 128 KiB metadata. A malformed or
corrupt registry fails closed with its error exposed in capability; explicit
repair of that registry is required before imports can rewrite it.

## Integrator commands (worker has not run any of them)

Use Git Bash with `source scripts/msvc-env.sh`, one Cargo owner, and a temporary
native `TS_RS_EXPORT_DIR`. Regenerate model/IPC bindings and simulation before
frontend verification; generated files were intentionally not edited here.

```sh
source scripts/msvc-env.sh
scripts/gen-bindings.sh
scripts/build-sim.sh
binding_scratch="$(mktemp -d)"
export TS_RS_EXPORT_DIR="$(cygpath -w "$binding_scratch")"
cargo test -p windfall-analysis --locked --offline --test classical_tools
cargo test -p windfall-analysis --locked --offline --test pitch_analysis
cargo test -p windfall-analysis --locked --offline --test analysis_jobs
cargo test -p windfall-project --locked --offline
cargo clippy -p windfall-analysis -p windfall-project --all-targets --locked --offline -- -D warnings
cargo test -p windfall-engine --locked --offline --test analysis_clock
cargo test -p windfall-desktop --lib --locked --offline classical_analysis
cargo test -p windfall-desktop --lib --locked --offline analysis_jobs
cargo test -p windfall-desktop --lib --locked --offline audio_editor
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
pnpm --dir apps/desktop test src/features/audio-editor src/features/analysis --maxWorkers=1
pnpm --dir apps/desktop test src/lib/ipc/sim/document.test.ts --maxWorkers=1
```

## Generated types and shared seams

Regenerate `DetectedAudioNote`, `AudioAnalysisData`, `AudioNoteOverride`,
`AudioWarpMarker`, `AudioDenoiseSettings`, `AudioViewTiming`,
`AudioEditRecipe`, `AudioAnalysisPreview`, `AudioAnalysisEdit`,
`AnalysisTask`, and changed `SampleAsset`, `Command`, `AnalysisSubmit`,
`AnalysisReview`, `AnalysisApply` and their barrel. Desktop-only calls are
`audio_analysis_detect/audition/apply/send_midi/cancel` and
`analysis_preview`. Refresh WASM plus its metadata for
`SetSampleProcessing` and persistence schemas. No model asset generator.

Shared seams touched:

- `windfall-project/src/{model,command,lower,edit,check,lib}.rs`;
  `arrangement.rs`/`preset.rs` and existing fixture constructors only add
  default processing fields.
- `windfall-engine/src/{lib,tempo,audio_edit}.rs`; existing sample fixtures
  including the soak constructor only add the new optional field. No soak run.
- `windfall-ipc/src/{analysis,audio_edit}.rs`.
- Desktop `commands.rs`, `session/{mod,analysis_jobs,audio_editor}.rs`,
  `session/tests/mod.rs`.
- Frontend `lib/ipc/{backend,tauri,mock}.ts`, simulation EVERY_COMMAND table;
  existing audio-editor mount and analysis action/panel modules.
- Analysis adapter/job exports and Cargo dependency/lock edges. Dependencies
  reuse existing workspace crates; no external dependency was introduced.

## Risks to check in order

1. Fresh TS exports/WASM first: optional non-Option IPC fields, recipe/Clip
   references, new sample constructors and reversible command lowering.
   Verify Rust/TS schema matches; generated artefacts are currently stale.
2. Compile native Session methods and all test fixtures; inspect any borrow,
   clippy size/warning or mounted React Compiler diagnostics before widening
   checks. None were run by this worker.
3. Execute independent frequency/SNR, neutral identity and 8 ms seam targets.
   These numerical assertions may require correcting algorithms, not weakening
   acceptance to fit observed output. Short interval spectral phases/formants,
   dense note grids and transient smearing need listening.
4. Tempo ramps, rounding at clip ends, trim offsets, frozen-source reopening
   after tempo changes, one-step history and retained original samples.
   Editor views bake per-clip gain/pan/fades; mixer processing and transport
   de-click ramps are deliberately outside the analysis input.
5. Peak memory and UI responsiveness at input bounds. Classical work is
   serialized by the editor worker and has Work/deadline checkpoints, but its
   f64 denoise accumulators and prepared buffers are not charged to the M1
   JobManager quota. New allocations can be several multiples of the 64 MiB
   buffer limit; RSS has not been measured.
6. Model-backed jobs retain stricter 16 MiB source/view, 32 MiB combined output,
   64 MiB model and exact sample-rate/channel contracts. A full long song or
   real Demucs pack needs audited tiling/resampling/cap adjustments. Registries
   are process-volatile; explicitly reimport manifests after restart.
7. Installed-model quality and redistribution remain external. Deterministic
   test outputs prove cancellation/checksum/staleness/review/layout only,
   never separation, dereverb or neural transcription.

## Follow-up dereverb kernel (written, not executed)

`crates/windfall-analysis/src/dereverb.rs` implements canonical overlapping
2-second windows / 1-second hops independent of incoming block partitions.
Vocal WPE solves jointly across channels with 35 ms prediction delay, eight
past taps, three desired-power reweighting iterations and regularized complex
Cholesky. General mode uses a fitted/clamped RT60 (0.15–2.5 s), exponentially
decaying delayed power, shared stereo gain, 0.38 gain floor and transient
protection. Vocal mode also bounds residual amplification and uses a 0.22
floor late-power gain. Low-confidence/no-decay material bypasses; natural
musical releases remain intrinsically ambiguous. No formant/room inversion
claim. Exact zero-amount bypass; output frame origin and length are preserved.
Streaming retains at most two input seconds plus one output overlap;
segment spectra/synthesis scratch remain bounded at 192 kHz stereo.
Algorithm written independently; no upstream implementation/package copied.
WPE origin: [NTT research](https://www.rd.ntt/cs/team_project/media/signal/wpe/),
Nakatani et al., DOI 10.1109/TASL.2010.2052251. No NTT p-code downloaded.

`crates/windfall-analysis/tests/dereverb.rs` acceptance cases, all **written,
not executed**: authored impulse/vibrato/music convolved with seeded noise
room response; late-tail energy reduction >10%, direct-region damage bounded,
already-dry relative error <1e-6, exact silence/zero wet, proportional stereo
image, exact output across input block partitions, modes differ, cancellation
and stream lifecycle. These are targets, not measured results.
Integrator additionally run: `cargo test -p windfall-analysis --locked --offline --test dereverb`.
Native adapter hookup is complete; UI is the next file group.
Built-in descriptors explicitly declare zero weight bytes and cannot be
imported through the model cache. JobManager asks trusted native adapters for
a weight-free descriptor; imported packs still hash/load their own bytes.
DSP scratch reserves 64 MiB without allocating an unused generic scratch
vector. Amount is pinned in job and review; preview/apply retains the existing
source/staleness and one-step history guards. Native session acceptance:
`builtin_dereverb_without_weights_reviews_amount_and_applies_one_saved_undo_step`
and `builtin_descriptor_cannot_be_imported_as_fake_weights`, written, not executed.
Production import case distinguishes built-in availability from unavailable
ONNX inference. Capability remains nonblocking while shutdown joins workers.UI now offers built-in dereverb without a model, distinct mode copy, boundedwet/dry input and reviewed amount. Canonical actions accept its native revieweddescriptor and refuse unsupported registered ONNX inference explicitly. Mountedcases `uses built-in %s without a pack and pins reviewed wet/dry` for bothmodes are **written, not executed** in `features/analysis/panel.test.tsx`.Regenerate `AnalysisCapability`, `AnalysisSubmit`, `AnalysisReview`,`AnalysisOnnxContract`, `AnalysisTensorSpec`, `AnalysisTensorDimension` andchanged `AnalysisModel` in addition to the earlier binding list.Dry/wet A/B audition uses the same retained review lease: `dry-input` auditionsthe exact captured selection without rereading/re-rendering a changed source.Only built-in dereverb accepts this reserved audition role; it cannot beselected for import. Mounted and native cases include dry audition, written,not executed. Apply amount remains the immutable reviewed render's setting.
