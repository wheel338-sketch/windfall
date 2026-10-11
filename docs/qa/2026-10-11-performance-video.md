# Performance, visualizer and video hand-off — 2026-10-11

Branch: `gpt/w3-performance-video`, base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
Edit-only worker: no build, test, typecheck, simulator generation or device check
has been executed. All acceptance cases listed here are **written, not executed**.
No dependencies or generated files changed. Parity statuses remain overseer-owned.

## Current implementation

`wf-performance-mode`: bounded 32-track/16-scene saved pattern pads; separate
sample-driven musical scheduler, last queued request wins, beat/bar/immediate
launch and stop, owned note releases, counted note overload, plan-exchange release,
tempo-map progression and meter-local quantization. Hardware action targets reuse
controller links and rising-edge dispatch. Public `performance::render_capture`
replays a frame-stamped script through the real processor, without song seek or
song playback; bounded to 60 seconds/8192 events and refuses native plugin capture.
Partial: audio/automation clip pads, native plugin capture, live recording of the
accepted callback launch log, channel arpeggiator/echo and expression-curve parity
remain. CPU/deadline and realtime allocator acceptance are not yet measured.

Video/visualizer/character UI and export work is in progress in the next checkpoint.
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
  `capture_rejects_unsorted_events_missing_targets_and_unbounded_duration`.
- Engine unit module `performance::tests`:
  `beat_bar_meter_boundaries_are_inclusive`,
  `latest_queued_launch_or_stop_wins_without_song_seek`.

## Integrator commands (not run here)

Run serially in Git Bash after `source scripts/msvc-env.sh`; use a private
`TS_RS_EXPORT_DIR`, `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`.

```sh
cargo test -p windfall-project --test stage_media --test controller_links
cargo test -p windfall-engine --test performance
cargo test -p windfall-engine --lib performance::tests
cargo test -p windfall-desktop --lib session::tests::controllers
cargo clippy -p windfall-project -p windfall-engine -p windfall-ipc -p windfall-desktop --all-targets -- -D warnings
```

Regenerate bindings and simulator only after merged source reconciliation:
`StageMedia`, `PerformancePad`, `VideoPlacement`, `VisualSettings`, `VisualScene`,
`LaunchQuantization`, `ControlAction`, `Command`, `ProjectSettings`,
`PerformanceState`, `PerformanceTrackState`, `MediaClock`, `MediaRenderAudio`,
`ActiveNotePose`, `RealtimeFrame`.

Check first: hand-maintained Rust/TS shape consistency; scheduler note ownership
across plan take-over; captured PCM partition equality; generated controller
action union with struct payloads; bounded media-export snapshot/plugin state.
Real controller, WebView/GPU/decoder playback, measured audiovisual drift against
physical DAC timestamps, other OSes and compressed-codec licensing stay external.
