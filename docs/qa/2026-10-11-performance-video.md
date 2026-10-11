# Performance, visualizer and video hand-off â€” 2026-10-11

Branch: `gpt/w3-performance-video`, base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
Edit-only worker: no build, test, typecheck, simulator generation or device check
has been executed. All acceptance cases listed here are **written, not executed**.
No dependencies or generated files changed. Parity statuses remain overseer-owned.

## Current implementation

`wf-performance-mode`: bounded 32-track/16-scene saved pattern pads; separate
sample-driven musical scheduler, last queued request wins, beat/bar/immediate
launch and stop, owned note releases, counted note overload, selective plan-exchange release (unchanged pattern/channel owners survive tempo/meter edits),
tempo-map progression and meter-local quantization. Hardware action targets reuse
controller links and rising-edge dispatch. Public `performance::render_capture`
replays a frame-stamped script through the real processor, without song seek or
song playback; bounded to 60 seconds/8192 events and refuses native plugin capture.
Partial: audio/automation clip pads, native plugin capture, live recording of the
accepted callback launch log, channel layer/arpeggiator/echo and expression-curve parity
remain. CPU/deadline and realtime allocator acceptance are not yet measured.
Note-off tests cover the scheduler and actual processor, but have not been run.

Performance grid now exposes 32 tracks × 16 scenes in banks, queued/active state,
per-track stops and atomic scene launch. MIDI Learn uses the controller-link action
contract. Browser/mock audio is explicitly unavailable rather than simulated.

`vis-video-visualizer-zgameeditor`: webview canvas consumes the native visualizer
analyzer slot, with mix/track source selection and three authored scenes (orbit,
bars, ribbon), persisted hue/intensity. A deterministic software rasterizer and
1024-point Hann FFT consume native offline stereo render for synchronized AVI 1.0
(BGR24 raw video + 16-bit PCM) and frame-hash metadata. Export is bounded by duration,
resolution and 128 MiB file budget; temporary working copies require extra memory.
No new native dependency. H.264/AAC compression remains a user licensing decision.

`vis-fruity-video-player`: saved local-file placements appear on the playlist,
with mute, tick position/length and second offset. Authorized native byte reads
(maximum 64 MiB) create webview Blob URLs; HTML video owns decoding. The follower
uses audio sample clocks/epochs and tempo-map tick position, with measured drift,
hard seeks, bounded rate correction and stale-clock pause; no wall-clock timers.
Unmuted video sound belongs to the webview, not the Windfall mixer/offline mix.
Device presentation latency remains unknown where the driver cannot report it.
Paths are local references, not portable embedded project assets.

`vis-fruity-dance`: newly authored CC0 procedural vector character Moss exposes
rest/step/wave/jump/lean poses, driven by real held-note owners of a saved chosen
channel and the audio frame phase. No third-party art or assets.

Saved stage settings already include checked video placements/mute/offset,
scene parameters and chosen note channel, with deletion-following/history.
Native realtime additions carry PDC-aligned media clock (driver latency explicitly
unknown) and one held-note pose per sounding channel, bounded to 128 channels.

## Written tests

- `crates/windfall-project/tests/stage_media.rs`:
  `stage_settings_legacy_omission_history_and_roundtrip`,
  `invalid_media_is_atomic_and_deletion_restores_references_on_undo`,
  `performance_midi_action_uses_checked_controller_contract`.
- `crates/windfall-engine/tests/performance.rs`:
  `offline_capture_is_bit_identical_across_partitions_and_does_not_play_song`,
  `retrigger_stop_and_tempo_plan_exchange_release_every_performance_note`,
  `capture_rejects_unsorted_events_missing_targets_and_unbounded_duration`,
  `delayed_launch_and_same_frame_stop_retrigger_capture_are_partition_independent`.
- Engine unit module `performance::tests`:
  `beat_bar_meter_boundaries_are_inclusive`,
  `latest_queued_launch_or_stop_wins_without_song_seek`,
  `queued_boundary_retimes_at_new_meter_and_preserves_tempo_owned_note_end`,
  `launch_stop_launch_race_releases_old_owner_before_new_owner`,
  `overload_drops_only_ons_and_all_accepted_owners_release_without_allocator_calls`,
  `processor_launch_retrigger_stop_and_tempo_adoption_do_not_call_allocator`.
- `apps/desktop/src-tauri/src/session/tests/performance_media.rs` (registered):
  `midi_note_pad_rising_edges_launch_and_stop_via_controller_links_without_song_seek`,
  `scene_is_one_admission_and_stale_pad_requests_are_refused`,
  `saved_media_reopens_and_only_saved_paths_in_current_generation_can_be_read`.
- `apps/desktop/src/features/video/follower.test.ts`:
  "maps clip offsets against musical tempo and stops outside the half-open placement",
  "seeks on epoch changes and loops without reading a wall clock",
  "trims modest drift, hard-corrects large drift and pauses a stalled sample cursor",
  "holds exact paused seeks and refuses absent, undecoded or out-of-range media".
- `apps/desktop/src/features/visualizer/render.test.ts`:
  "measures a bin-centred audio fixture against analytic RMS and Parseval energy",
  "matches independently specified bar pixels and checksums",
  "renders every authored scene reproducibly from deterministic audio",
  "maps real note keys and velocities to newly authored character poses".
- `apps/desktop/src/features/visualizer/avi.test.ts`:
  "retains song-time animation phase for a range export instead of restarting the scene",
  "declares exact stream rates, ceil frame count, PCM length and index offsets",
  "rejects malformed, over-budget or cancelled inputs before publishing a file".
- `apps/desktop/src/features/performance-media/panels.test.tsx`:
  "launches a saved pad and a scene through current document tokens, including keyboard activation",
  "reports unavailable launch audio instead of presenting mock playback as live",
  "selects the native analyzer spectrum and persists scene parameters through checked commands",
  "edits mute and source offset and removes saved timeline video with undo",
  "drops a media read that completes after unmount without allocating a Blob URL",
  "animates the selected channel from the real note-pose feed and clears on replacement".

All of the above are written, not executed. The mounted analyzer case hands in
native-protocol fixtures; it is not evidence of a real webview/audio stream.
Independent references include analytic sine/Hann power, explicitly specified
bar pixels with a separate BigInt FNV implementation, and a separate RIFF parser
checking stream rates, chunk PCM samples and every index pointer.


## Integrator commands (not run here)

Run serially in Git Bash after `source scripts/msvc-env.sh`; use a private
`TS_RS_EXPORT_DIR`, `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`.

```sh
cargo test -p windfall-project --test stage_media --test controller_links
cargo test -p windfall-engine --test performance
cargo test -p windfall-engine --lib performance::tests
cargo test -p windfall-desktop --lib session::tests::performance_media
cargo test -p windfall-desktop --lib session::tests::controllers
cargo clippy -p windfall-project -p windfall-engine -p windfall-ipc -p windfall-desktop --all-targets -- -D warnings
```

After merged source reconciliation, regenerate the bindings and simulator before
frontend checks (the current checked-in artifacts cannot accept SetStageMedia):

```sh
scripts/gen-bindings.sh
scripts/build-sim.sh
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop test src/features/video/follower.test.ts src/features/visualizer/render.test.ts src/features/visualizer/avi.test.ts src/features/performance-media/panels.test.tsx src/lib/ipc/sim/document.test.ts
pnpm --dir apps/desktop lint
```

New/changed generated types:
`StageMedia`, `PerformancePad`, `VideoPlacement`, `VisualSettings`, `VisualScene`,
`LaunchQuantization`, `ControlAction`, `Command`, `ProjectSettings`,
`PerformanceState`, `PerformanceTrackState`, `MediaClock`, `MediaRenderAudio`,
`ActiveNotePose`, `RealtimeFrame`.

## Claims and integration risks

| Row | Claim | Remaining |
| --- | --- | --- |
| wf-performance-mode | Partial | Accepted live launch-log recording/export workflow, native plugin render-owner capture, audio/automation pads, channel layers/arp/echo and note-curve parity; deadline measurements. |
| vis-video-visualizer-zgameeditor | Full-row candidate | Execute deterministic/reference/mounted cases; native feed and AVI interoperability/memory gates. |
| vis-fruity-video-player | Full-row candidate | Real HTML decoder and drift/seek/loop gate; physical device latency unknown, local 64 MiB files, unmuted audio outside mixer. |
| vis-fruity-dance | Full-row candidate | Real note-owner/vector rendering gate; bounded to one newest held-note pose per channel, first 128 sounding channels. |

Check first: hand-maintained Rust/TS shape consistency; scheduler note ownership
across plan take-over; captured PCM partition equality; generated controller
action union with struct payloads; bounded media-export snapshot/plugin state.
Real controller, WebView/GPU/decoder playback, measured audiovisual drift against
physical DAC timestamps, other OSes and compressed-codec licensing stay external.


Risk order: generated enum/payload shapes and Rust ownership/typecheck first;
then processor allocator and note-off/tempo take-over cases; partition equality;
then simulator persistence/mounted cases and analytic FFT/pixel/RIFF checks;
then exported-file memory and webview interoperability. Native analyzer bands and
levels are f32 on the wire; offline analysis mirrors that quantization. Live seeks/loops and range exports now derive animation phase from the same
song tempo-map seconds, retaining the range start in firstVisualFrame metadata.
Analyzer window alignment across PDC, seeks and tempo changes remains a native
comparison gate (offline range analysis has a fresh 1024-frame startup window).
Native audio export conservatively caps the range at 4800 ticks (30 seconds at
10 BPM), so the available duration can be much shorter at ordinary tempos.
HTML play rejection and unsupported files are visible errors; real autoplay
policy, variable frame-rate media and AVI player behavior need external checks.

## Shared seams touched

- Project: `crates/windfall-project/src/{lib,model,command,check,lower}.rs` and
  `src/controller_links/mod.rs` (append-only action variants).
- Engine: `crates/windfall-engine/src/{lib,message,voice,shared,processor,controller}.rs`.
- IPC: `crates/windfall-ipc/src/lib.rs` (realtime additions and module export).
- Native: `apps/desktop/src-tauri/src/commands.rs`,
  `session/{mod,controllers}.rs`, `session/tests/mod.rs`.
- Frontend: `apps/desktop/src/lib/ipc/{backend,tauri,mock}.ts`,
  `lib/ipc/sim/document.test.ts`, `lib/store/realtime.ts`,
  `features/layout/{app-shell.tsx,register-actions.ts}`, `features/playlist/index.tsx`.

New implementation logic and tests live in owned stage_media, performance and
performance_media modules plus the performance/video/visualizer feature folders.
No tracker, dependency, generated artifact, main checkout or other worktree edits.
