# Second pass: rack, mixer, playlist and recording

Branch: `gpt/p2-rack-mixer`; base `038a7de0`.

All new tests below are **written, not executed**. No builds, checks, tests,
generators, lint, benchmarks, browser or device runs were performed by this
worker. Touched Rust files were formatted; git whitespace inspection was clean.
No dependencies, generated files or parity statuses were changed.

## Per-row hand-off

“Candidate” means the assigned software changes and harness are written;
compilation and acceptance still belong to the integrator. It is not passing
execution evidence or a promotion of the parity tracker.

| Row | Claim | Implemented / software still outstanding |
| --- | --- | --- |
| `win-rack-sampler-precomputed` | Full-row candidate | −1 dBFS normalization default/reference/UI; prepared-result audition and overview; conservative derived-file collection with source/history/save retention. No assigned software item left unwritten. |
| `win-rack-channel-envelopes` | Full-row candidate | Real mounted envelope/LFO knob gestures and native audible publication, one history step, undo/redo and reopen. No assigned software item left unwritten. |
| `win-rack-echo-delay` | Full-row candidate | Real refused-echo telemetry and mounted counter/reset; instrument repeat frequency, seek/live vs sequenced loop discard, partition equality, allocation and one-frame cost smoke. No assigned software item left unwritten. |
| `win-rack-sampler-stretch` | Full-row candidate | Real inspector Fit/apply against WASM, exact saved ratio/history; actual native preparation/duration/PCM/history/reopen. No assigned software item left unwritten. |
| `inst-channel-sampler` | Full-row candidate | Inherits the completed preprocessing, gesture, preview/GC and tempo-fit joins above. No assigned software item left unwritten. |
| `core-mixer` | Full-row candidate | Real CLAP/VST3 helper capture while running, disk track preset, another track, restored PCM, history and reopen. No assigned software item left unwritten; physical IO/arming remains external. |
| `win-mixer-track-presets` | Full-row candidate | Same joined native preset flow, independent gain reference and allocation assertions. No assigned software item left unwritten. |
| `win-playlist-instrument-audio-tracks` | Full-row candidate | MIDI song placements filter to linked channel; encoded MIDI, repeated/shared patterns, history/reopen. No assigned software item left unwritten; physical linked recording remains external. |
| `fmt-export-playlist-tracks` | Full-row candidate | Native decoded sidechain-driven stems/selected region alignment plus mounted guarded export request. No assigned software item left unwritten. |
| `wf-transport` | Full-row candidate | Real Record/start/keep panel, WASM attachment/history/reopen and native take reopen; late reply generation guard. No assigned software item left unwritten; hardware latency/drift remains external. |
| `core-audio-logger` | Full-row candidate | Native enable/song-play/dump/playlist, exact recovered PCM/frame alignment, one undo/redo and fresh reopen. No assigned software item left unwritten; listening remains external. |
| `editor-edison` | Partial pending script integration | Chain-position slot recorder, continuous editor, spectrum, persistent regions/markers, EQ/blur/convolution/repair/loop tuning/silence splitting, selection stretch quality/formants and sampler/playlist drag are written. Guarded script-selection entry point is written/tested; sandbox interpreter and Run script IPC/UI action remain on the unmerged `gpt/w3-scripting-assistant` branch. This worker intentionally did not create a second interpreter. |

## Implemented behavior

Preprocessing Preview and Apply share the native options/kernel. Preview
returns the actual rendered waveform/duration and auditions that buffer without
writing a file or document history. Stale source/settings/edit/generation refuses
before audition. Normalization targets −1 dBFS, matching the editor; the UI says so.

`session/derived_assets.rs` owns only successfully attached outputs created in
this session. Collection retains document, undo and redo source roots, and pins
imported/saved/backup sources. New/Open can release abandoned unsaved outputs.
No arbitrary directory scanning/deleting occurs. Files from older sessions are
conservatively retained because other saved documents may still reference them.
The cache checks file existence before serving a decoded entry.

MIDI song export filters each linked instrument-track placement to its channel;
pattern export retains all pattern channels. The original shared pattern is
unchanged. Refused-echo count travels from native engine frame through
`RealtimeFrame.rejectedEchoes?` to the inspector's accessible output.

The new recorder uses the existing transparent Send tap effect slot. Its real
effect editor mounts Record/Stop and edit/Discard and a duration/drop counter.
The bounded logger tap copies the stereo signal immediately after that exact
slot, before following effects and fader. It allocates no callback scratch and
uses the existing queue/worker/history retirement. New/Open disarms effect ids.
Logger generation snapshots are taken before acquiring the logger mutex; no
document lock is acquired under that mutex. Changing logger sources starts a
new history; simultaneous slot recorders are refused in the UI to avoid losing
the first recording silently. Unavailable output errors are shown.

Editor Apply renews the preview/token on the derived clip while keeping the
panel open. The nonmodal dialog allows drops onto the actual sampler and playlist.
Metadata writes and transfers use native guards and one document transaction.
Regions are optional source-frame metadata, omitted when empty; markers have
equal endpoints. Source/view mapping follows clip timing/reverse. A region
crossing a partial reverse remains a contiguous bounding interval.

Worker DSP includes bounded Hann STFT EQ and magnitude blur, linear FFT
convolution with a captured in-view impulse (up to 32768 frames, cropped tail),
short clipping-plateau Hermite repair, duration-preserving loop seam blend,
linked-stereo silence-run discovery, and a bounded anti-phase-safe spectrogram.
Strip silence makes clips at the original positions, with whole-tick offsets
and an overlap refusal. New controls are purpose-specific; original WAVs remain
untouched. Selection transfers create a Float32 WAV and attach it atomically;
sampler trim/reverse/loop/stretch/tune reset, while other controls remain.

### Script integration seam

`Session::audio_editor_run_script_selection(token, AudioEditRange, run)` takes
an immutable selected AudioBuffer into a runner callback and accepts a replacement
AudioBuffer/error. It shares built-in source/generation/edit/publication guards,
finite/rate/channel/duration validation, source preservation and one-step history.
Call it on a blocking worker. The scripting worker owns interpreter quotas,
parsing and execution. Its IPC/UI adapter should pass the active editor token and
range plus script into its sandbox, call this seam and deliver the resulting
DispatchResult through the existing editor commit/token-renewal flow. The built-in
`"script"` Apply operation refuses direct execution. See `docs/AUDIO-EDITOR.md`.

## Tests — all written, not executed

- `crates/windfall-midi/tests/linked_tracks_qa.rs`:
  `song_export_filters_each_placement_without_filtering_shared_patterns` measures
  real encoded MIDI note/channel/tick placements and undo/redo/JSON reopen.
- `session/tests/mixer_preset_native.rs`:
  `clap_running_helper_preset_restores_pcm_history_and_reopen` and VST3 counterpart
  join scanner/helper, running state capture, library file, another track, gain
  reference, callback allocation, undo/redo and fresh reopen. Requires the fixture
  DLL/current helper env variables below; neither case is skipped without them.
- `session/tests/audio_logger.rs`:
  `enable_song_play_dump_preserves_sample_alignment_through_playlist_and_reopen`
  compares nonperiodic signed bursts/recovered PCM exactly;
  `slot_recorder_captures_between_effects_then_edits_and_reopens_as_one_take`
  measures .4 → .5 gain → tap → .25 gain as .2 tap/.05 output;
  `slot_recording_disarms_on_document_replacement_and_refuses_the_old_history`.
- `session/tests/rack_second_pass.rs`:
  `envelope_and_target_lfo_gestures_publish_audible_native_changes_as_one_history_step`
  and `tempo_fit_prepares_the_requested_native_duration_and_restores_it_after_history_and_reopen`.
- `session/tests/sampler_preprocess.rs`: revised normalization reference;
  `prepared_preview_has_the_apply_waveform_and_duration_without_files_or_history`,
  `a_stale_prepared_preview_is_refused_before_audition_or_history`,
  `derived_collection_retains_history_saved_and_original_sources_and_removes_only_abandoned_owned_audio`.
- `engine/tests/engine/rack_voice.rs`: instrument frequency, partition/allocator,
  seek/live/loop-discard and one-frame smoke additions.
- `session/tests/playlist_export.rs`:
  `sidechain_stems_measure_lane_isolation_and_selected_region_sample_alignment`.
- `session/tests/transport_acceptance.rs`: actual captured take save/fresh reopen.
- `engine/src/audio_edit/advanced.rs`: `advanced_convolution_matches_direct_linear_reference_without_wrap_or_channel_mix`,
  `advanced_flat_eq_has_independent_gain_reference_and_spectrum_keeps_antiphase`,
  `advanced_repair_loop_and_silence_regions_are_bounded_and_linked`,
  `advanced_blur_changes_tone_energy_and_is_deterministic_and_finite`.
- `project/tests/audio_regions.rs`:
  `audio_regions_have_guarded_atomic_history_and_default_compatible_persistence`.
- `session/tests/audio_editor.rs`:
  `regions_markers_follow_views_transfers_history_and_fresh_reopen`,
  `editor_advanced_tools_and_sandbox_entry_share_one_step_history_and_saved_sources`,
  `strip_silence_makes_position_preserving_clips_in_one_saved_undo_step`,
  `invalid_or_stale_script_results_leave_no_asset_or_history`.
- Mounted real panels against the real WASM document:
  `channel-rack/sampler-second-pass.qa.test.tsx` (filter/LFO gesture history,
  Fit/apply/reopen, no-history preview/GC controls, telemetry/reset),
  `transport/recording.qa.test.tsx` (start/keep/attachment/history/reopen),
  `export/playlist-tracks.qa.test.tsx` (guarded selected-region request),
  `audio-editor/second-pass.qa.test.tsx` (spectrum/regions/markers/history/reopen,
  advanced/stretch request controls, real effect editor recorder attachment,
  actual sampler drop/history/reopen and late transfer refusal),
  `audio-editor/editor.test.tsx` (stay-open token renewal; existing stale cases),
  `mixer/effects.test.tsx` (menu includes recorder entry).
  Native preview/device/file adapters in mounted tests are fixtures; these do
  not claim native PCM measurements. The paired Rust tests measure that side.

## Sequential integrator commands

Use the Windows/MSVC environment and isolated TS_RS_EXPORT_DIR. Regenerate the
combined bindings and WASM first (overseer-owned scripts); do not run concurrent
builds. New generated types are listed below. Build the fixture/helper explicitly
before the joined preset cases:

```bash
cargo build --manifest-path crates/windfall-plugin-host/test-plugins/Cargo.toml
cargo build -p windfall-desktop
# Set WINDFALL_BRIDGE_FIXTURE to the built windfall_test_plugins.dll.
# Set WINDFALL_DESKTOP_BRIDGE_HELPER to the current windfall-desktop.exe.
cargo test -p windfall-midi --test linked_tracks_qa
cargo test -p windfall-project --test audio_regions
cargo test -p windfall-engine --lib sample_preprocess
cargo test -p windfall-engine --lib audio_edit
cargo test -p windfall-engine --test engine rack_voice
cargo test -p windfall-engine --test engine audio_logger
cargo test -p windfall-desktop --lib session::tests::mixer_preset_native
cargo test -p windfall-desktop --lib session::tests::rack_second_pass
cargo test -p windfall-desktop --lib session::tests::sampler_preprocess
cargo test -p windfall-desktop --lib session::tests::audio_logger
cargo test -p windfall-desktop --lib session::tests::audio_editor
cargo test -p windfall-desktop --lib session::tests::transport_acceptance
cargo test -p windfall-desktop --lib session::tests::playlist_export
pnpm --dir apps/desktop test src/features/channel-rack/sampler-second-pass.qa.test.tsx src/features/channel-rack/rack-voice.qa.test.tsx src/features/transport/recording.qa.test.tsx src/features/export/playlist-tracks.qa.test.tsx src/features/audio-editor src/features/mixer/effects.test.tsx --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```

Then run the integrator's ordinary whole-suite gates. Every command above is a
handoff instruction, not a command executed by this worker.

## Shared seams and regeneration

Minimal additions to `commands.rs`; `session/{mod.rs,tests/mod.rs,files.rs,types.rs,
audio_editor.rs,audio_logger.rs}`; project `{model.rs,command.rs,lower.rs,check.rs}`;
IPC `lib.rs` RealtimeFrame plus `audio_edit.rs`/`audio_logger.rs`; engine mixer
chain tap and logger; Backend/Tauri/mock; `sim/document.test.ts` EVERY_COMMAND;
effect editor/menu; rack inspector/grid and playlist drop handlers; dialog optional
`showBackdrop` defaulting true. Source literals were widened for SampleAsset.regions.
All other workers' registrations/order must be preserved on merge.

Regenerate: `RealtimeFrame` (optional rejectedEchoes), `AudioEditOperation`,
`AudioEditPreview` (spectral/regions), `AudioEditDestination`,
`AudioEditTransferRequest`, `AudioRegion`, `SampleAsset`, `Command`
(SetSampleRegions), `AudioLoggerSettings` (optional effects), `AudioLoggerSource`
(effect). Browser WASM must include SetSampleRegions. No generated file was edited.

## Risks / remaining gates, in order

1. Compile/ts-rs/WASM shape consistency and merges in shared seams. All contracts
   were reconciled by reading only; formatter parsing is not compilation.
2. Native preset fixture control ID 7, .625 gain/state interpretation and real
   helper scheduling/reopen. Tests require the explicit current executable/DLL.
3. Native rack gesture RMS/deterministic restart and Fast-stretch ending tolerance;
   selected-region sidechain detector attenuation/file endpoint assertions.
4. Mounted accessible names and Base UI nonmodal drag interaction. jsdom tests
   cover real handlers/WASM history, not a native-window mouse walkthrough.
5. DSP STFT numerical tolerances, selection boundary behavior, silence split
   whole-tick quantization and metadata mapping after compound timing edits.
6. Merge and wire the other worker's sandbox interpreter/Run script action to the
   native selection seam; scripting is the outstanding software dependency.

Derived cleanup is intentionally conservative across restart/saved files. The
one-frame elapsed bound is only a generous bounded-work smoke assertion, not a
physical driver's timing result. Physical audio/MIDI capture, device loss,
converter alignment/drift, linked-track recording, listening, third-party plugin
compatibility and other-OS checks remain external and are not claimed.
