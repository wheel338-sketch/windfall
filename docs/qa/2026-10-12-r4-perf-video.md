# Round four performance/video hand-off — 2026-10-12

Branch: `gpt/r4-perf-video`. Base: `cfd2d37e`.
Worktree: `C:/Users/ewhee/.t3/projects/windfall/.claude/worktrees/r4-perf-video`.
Last implementation commit before this hand-off: `ce95db51`.

This was an EDIT-ONLY round. No builds, tests, typechecks, binding/simulator
generation, dependency installation, fixture builds, device checks or browser
checks were executed. Every test below is **written, not executed**. No parity
status was edited; both rows remain `in-progress` until integrator acceptance.
No new dependencies, generated artifacts, main-checkout edits or other-worktree
edits were made. The contract was read first from its requested main-checkout
path; implementation work stayed in the assigned worktree.

## `wf-performance-mode`

Implemented software:

- Preallocated, bounded callback recording of final stereo output, including
  native instruments/effects. Callback adoption, PCM/log writes and completion
  use prepared storage and atomics; buffers and boxed replay state retire
  through the existing control-side garbage path. One queued capture can own
  storage at a time. Limits: 60 seconds, 8–192 kHz, 8192 log events, 16 saved takes.
- Logs record winning launches/stops at the actual accepted quantized boundary,
  rather than recording all enqueue attempts. Initial active pads and stop-all
  are logged. Same-frame log order is retained. The existing launch selection,
  held-note order and `render_capture` partition loop were preserved.
- Stop/save/cancel, saved-take replay, stop replay, Float32 WAV export and checked
  removal are exposed in the grid. Capture saves through the ordinary checked
  sample/settings transaction, with undo/reopen and stale-generation/revision
  refusal. Stream replacement, clock gaps, capacity overflow, missing sample
  files and stopped callback acknowledgements produce errors.
- Replay uses the captured final PCM as the authoritative sound, so native
  plugin state/randomness cannot alter a saved take. It consumes the saved
  sample-stamped log to restore pad status and to support recording a replay.
  Same-rate PCM is copied exactly; other rates use absolute rational source
  positions. Replay bypasses a second mix gain/effect pass. Export writes the
  same authoritative Float32 PCM. This is a printed performance; editing the
  original pads after recording does not re-render a saved take.
- The independent scripted `performance::render_capture` path now obtains a
  native render-role factory and checks render errors instead of categorically
  refusing projects with native plugins. Active native ownership is not stolen.
- Pads can select saved audio or automation clips as well as patterns. Prepared
  audio uses existing clip offset/gain/pan/fade/pitch/stretch and mixer routing;
  looping/retrigger/stop have bounded tails. Automation uses existing target and
  curve evaluation, restores stored targets on stop, and supports tempo pads.
- Performance note starts now use the existing layer/arpeggiator/echo route.
  Natural releases preserve echo/one-shot behavior; explicit stops cut their
  owners. Note curves read the independent performance musical clock while the
  song is stopped. Layer children inherit the same curve source.

Files changed for this row:

- Project: `crates/windfall-project/src/stage_media.rs`, `src/lib.rs`,
  `tests/stage_media.rs` (`PerformancePad.clip`, saved `PerformanceTake` and
  `PerformanceLogEvent`, validation/defaults/deletion following).
- Engine: `crates/windfall-engine/src/performance_recording.rs`,
  `performance_clips.rs`, `performance.rs`, `processor.rs`, `controller.rs`,
  `message.rs`, `shared.rs`, `plan.rs`, `automation.rs`, `note_curves.rs`,
  `channel_voice_runtime.rs`, `sequencer.rs`, `lib.rs`, and
  `tests/performance_extended.rs`. `sequencer.rs` only adds the curve-source
  discriminator for existing notes plus the separate video clock described below.
- IPC/native: `crates/windfall-ipc/src/performance_media.rs`;
  `apps/desktop/src-tauri/src/commands.rs`,
  `session/{mod,performance_recording,performance_media,recording,transport}.rs`,
  `session/tests/performance_media.rs`.
- UI: `apps/desktop/src/features/performance/{grid,recording-controls}.tsx`,
  `features/performance-media/{controls.tsx,store.ts,panels.test.tsx}`,
  `lib/ipc/{backend,tauri,mock}.ts`, `lib/ipc/sim/document.test.ts` (source fixture
  shape only). The mock explicitly refuses native audio recording/replay/export.

Exact written tests:

- Engine unit `performance::tests`:
  `accepted_log_contains_only_the_winning_launch_at_its_quantized_boundary`.
- Engine unit `performance_recording::tests`:
  `callback_capture_preserves_exact_pcm_and_same_frame_launch_transcript_without_allocating`,
  `capture_refuses_clock_discontinuity_and_event_overflow_without_partial_success`,
  `processor_performance_record_adoption_copy_completion_and_replay_have_no_allocator_calls`,
  `recording_admission_refuses_a_second_buffer_until_callback_completion`,
  `replay_consumes_sample_stamped_log_in_order_at_native_and_resampled_rates`.
- Engine unit `note_curves::performance_tests`:
  `every_performance_curve_parameter_reads_musical_phase_instead_of_stopped_song_clock`.
- `crates/windfall-engine/tests/performance_extended.rs`:
  `performance_layer_fans_out_and_arp_echo_change_measured_audio`,
  `performance_note_pan_curve_moves_sound_while_song_transport_is_stopped`,
  `audio_and_automation_pads_loop_route_and_restore_without_song_playback`.
- `crates/windfall-project/tests/stage_media.rs`:
  `recorded_performance_log_defaults_roundtrips_and_rejects_unsorted_events_atomically`.
- Registered desktop `session::tests::performance_media`:
  `performance_callback_log_pcm_save_reopen_undo_and_wav_export_are_exact`,
  `native_vst3_performance_output_is_captured_and_replayed_bit_for_bit`
  (Windows only; not ignored, uses the repository's existing native fixture).
- `apps/desktop/src/features/performance-media/panels.test.tsx`:
  `records stops and saves only after callback completion through current document tokens`.

SOFTWARE remaining:

- Builder-owned existing failure:
  `delayed_launch_and_same_frame_stop_retrigger_capture_are_partition_independent`.
  Preventing PCM in the first 1531 delayed-launch frames and fixing same-frame
  stop/retrigger partition ordering are intentionally not implemented here.
  `crates/windfall-engine/tests/performance.rs` is unchanged relative to the base.
- All new source still needs compilation, generated-shape reconciliation, full
  suites, realtime allocator/deadline measurement, and actual native UI checks.
  These are software gates, not EXTERNAL-ONLY. No other known acceptance-cell
  implementation item has been deliberately left out of this worker's scope.

EXTERNAL-ONLY after those software gates: physical MIDI/interface response,
hardware presentation latency and listening evaluation, and other operating
systems requiring machines unavailable to the integrator.

Least-certain spots: prepared capture publication/acknowledgement and cancellation
under stream replacement; native render-factory integration; natural echo/layer
owner releases; tempo/automation pad processing deadlines; generated TS shapes
and mounted Base UI controls. Maximum-sized buffers and complete curve/tempo
combinations need the integrator's stress checks.

## `vis-fruity-video-player`

Implemented software:

- An unmuted placement has a persisted mixer track, defaulting legacy placements
  to Master. Invalid routes fail checked validation; deleted routes follow to
  Master. The webview remains muted so there is one audio owner.
- Video bytes and audio decode off the audio thread and outside the document
  guard. Prepared immutable PCM and picture bytes are installed together, so
  later disk changes cannot make picture and soundtrack read different file
  versions. The callback only reads prepared sample storage. Bounds: 64 MiB
  source file, 64 MiB decoded PCM, 128 MiB combined prepared video data/project.
- Audio reads derive from the same placement offset, tempo-map musical position
  and transport clock as the picture, with half-open ranges, resampling, seek
  and loop addressing. A separate read-only video clock handles multiple short
  loop wraps within a callback and stops at finite-pass exhaustion.
- Prepared sound enters the selected native mixer input before normal effects,
  sends, faders and PDC. Ordinary offline exports and mixer stems include it;
  playlist-clip stems mute it to avoid copying the same soundtrack into each
  unrelated clip stem. Video-only projects can play and export.
- Existing Symphonia probing/decoding is reused. A bounded RIFF AVI PCM demuxer
  feeds the existing WAV decoder and respects audio stream start. Missing files,
  unsupported codecs and malformed streams produce clear errors; export refuses
  absent prepared audio instead of succeeding silently.
- Raw RGB AVI picture playback is provided in a canvas using prepared native
  bytes and the existing sample-clock follower. This makes the repository's
  own raw BGR24 + PCM AVI format playable end to end without a new decoder
  dependency. Frame lookup handles offsets, seeks/loops and exact stream rate;
  the pixel surface is reused. HTML decoding remains for other picture formats.

Capability boundary: PCM AVI is the supported paired picture/audio format added
here; the existing audio decoder also probes its already-supported formats.
MP4/AAC, compressed AVI and WebM audio codecs absent from the repository fail
explicitly. No new dependency or codec-download path was added. Paths remain
local references; reopening prepares them again and reports missing sources.

Files changed for this row:

- Codec: `crates/windfall-codec/src/{video_audio,lib}.rs`.
- Engine: `crates/windfall-engine/src/{video_audio,pool,plan,processor,sequencer,render,stems,lib}.rs`,
  `src/stems/playlist.rs`, `tests/video_audio.rs`.
- Project: `crates/windfall-project/src/{stage_media,lib}.rs`,
  `tests/stage_media.rs`.
- Native: `apps/desktop/src-tauri/src/commands.rs`,
  `session/{preparation,performance_media,transport,export}.rs`,
  `session/tests/performance_media.rs`.
- UI: `apps/desktop/src/features/video/{avi.ts,avi.test.ts,player.tsx,panel.tsx,follower.test.ts}`,
  `features/performance-media/panels.test.tsx`, `lib/ipc/{backend,tauri,mock}.ts`.

Exact written tests:

- Codec unit `video_audio::tests`:
  `avi_pcm_demux_preserves_stream_start_and_interleaved_samples`,
  `missing_video_decoder_and_truncated_avi_are_errors`.
- Engine unit `video_audio::tests`:
  `video_source_time_is_half_open_offset_exact_and_seek_safe`.
- `crates/windfall-engine/tests/video_audio.rs`:
  `video_audio_uses_mixer_fader_and_matches_live_and_export_pcm`,
  `video_audio_mute_and_missing_decoder_cannot_silently_export_success`,
  `video_audio_short_loops_wrap_multiple_times_per_callback_without_partition_drift`,
  `video_mixer_stems_discover_sources_and_isolate_each_soundtrack`.
- `crates/windfall-project/tests/stage_media.rs`:
  `legacy_video_route_defaults_to_master_and_invalid_route_is_atomic`.
- Registered desktop `session::tests::performance_media`:
  `unmuted_video_audio_prepares_without_document_lock_and_reopens_exact_pcm`
  (also immutable picture bytes and stale revision),
  `missing_video_decoder_refuses_unmute_atomically` (unsupported and absent files).
- `apps/desktop/src/features/video/avi.test.ts`:
  `decodes the repository AVI picture pixels and seeks by its exact stream rate`,
  `leaves other containers to the webview and refuses truncated AVI`.
- Existing mounted test augmented:
  `edits mute and source offset and removes saved timeline video with undo`
  verifies the HTML element stays muted when the native soundtrack is enabled.

SOFTWARE remaining: execute decoder/PCM/stem/persistence and mounted tests,
compile native and generated bindings, then test actual Windows webview playback,
picture/audio seek/loop alignment and prepared-data memory/deadline bounds. No
known acceptance-cell implementation item is deliberately left out. This row is
a full-row implementation candidate, not a completion claim.

EXTERNAL-ONLY after software acceptance: physical display/DAC presentation
latency, human audiovisual/listening evaluation, independent AVI player
interoperability, and other operating systems without an available machine.

Least-certain spots: AVI stream header/start/padding edge cases and real WebView
canvas scheduling; PDC alignment against the displayed cursor; multiple-video
memory limits; mixer/playlist-stem source isolation and rapid seek/region-loop
changes. The unsupported-codec error is intentional, not silent fallback audio.

## Integrator overlaps and checks

Preserve the builder's launch/partition fix when integrating these commits.
Unavoidable overlap is in `crates/windfall-engine/src/performance.rs` (accepted
boundary notifications, On/Off payload metadata, read-only clip phase/tempo
accessors and native render-factory preparation) and `processor.rs` (performance
event routing, capture/replay, and a **new automation-pad-only** one-frame mixer
parameter path). Original pending/held iteration order and the legacy note-only
partition path were not changed. The new video helper in `sequencer.rs` neither
advances transport nor changes existing `song_tick_at` or launch ordering.

Shared integration seams also include project/engine module registration,
`Plan`, `SamplePool`, Controller/Message/Shared ownership, session construction,
command registration, native preparation/export/transport guards, IPC backends
and UI defaults. Regenerate bindings for `PerformancePad.clip`, `VideoPlacement.mixerTrack`,
`StageMedia.takes`, `PerformanceTake`, `PerformanceLogEvent` and
`PerformanceRecordingState`; regenerate the simulator after source integration.

Suggested integrator checks, **not run here**, serially under the repository's
Windows/MSVC setup, private `TS_RS_EXPORT_DIR`, `CARGO_BUILD_JOBS=1` and
`RUST_TEST_THREADS=1`:

```sh
cargo test -p windfall-codec --lib video_audio::tests
cargo test -p windfall-project --test stage_media --test controller_links
cargo test -p windfall-engine --lib performance
cargo test -p windfall-engine --lib note_curves::performance_tests
cargo test -p windfall-engine --lib video_audio::tests
cargo test -p windfall-engine --test performance --test performance_extended --test video_audio
cargo test -p windfall-desktop --lib session::tests::performance_media
cargo test -p windfall-desktop --lib session::tests::controllers
cargo clippy -p windfall-codec -p windfall-project -p windfall-engine -p windfall-ipc -p windfall-desktop --all-targets -- -D warnings
scripts/gen-bindings.sh
scripts/build-sim.sh
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop test src/features/video/avi.test.ts src/features/video/follower.test.ts src/features/performance-media/panels.test.tsx src/lib/ipc/sim/document.test.ts
pnpm --dir apps/desktop lint
```

Prebuild the existing native bridge fixture/helper serially and set
`WINDFALL_BRIDGE_FIXTURE` / `WINDFALL_DESKTOP_BRIDGE_HELPER` using the repository
fixture workflow before the native VST3 test; no nested Cargo build is intended.
Then run full suites and real native UI/allocator/deadline gates as required by
the QA completion rule. This command list does not replace those full gates.

Read-only checks performed: source/diff review, strict UTF-8 reads of all 49
changed source files, `git diff --check` (clean), and confirmation that
`tests/performance.rs` is untouched. Permitted single-file `rustfmt --check`
on `performance_recording.rs`, `performance_extended.rs` and `processor.rs`
reported formatting differences (exit 1); no syntax diagnostics were seen.
Formatting is not claimed clean. Integrator formatting must be reconciled with
the ordering fix. No build or test command was executed.
