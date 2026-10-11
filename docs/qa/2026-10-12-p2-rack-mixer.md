# Second pass: rack, mixer, playlist and recording

Branch: `gpt/p2-rack-mixer`, base `038a7de0`.

All changes and tests in this hand-off are **written, not executed**.
The worker has run no build, test, generator, lint or device command.

## Completed pieces

- Pre-processing normalization now targets −1 dBFS, matching the audio editor.
  The native DSP/session reference assertions and checkbox label use that target.
- MIDI song export filters every pattern placement on an instrument lane to
  its linked channel. Pattern export retains all channels. The new native
  `linked_tracks_qa::song_export_filters_each_placement_without_filtering_shared_patterns`
  covers repeated placements, real encoded MIDI, undo/redo and JSON reopen.

## Integrator commands

Run sequentially in the integrator environment, with an isolated
`TS_RS_EXPORT_DIR` and after regenerating the combined bindings/WASM:

```bash
cargo test -p windfall-midi --test linked_tracks_qa
cargo test -p windfall-engine --lib sample_preprocess
cargo test -p windfall-desktop --lib session::tests::sampler_preprocess
pnpm --dir apps/desktop test src/features/channel-rack/rack-voice.qa.test.tsx --maxWorkers=4
```

## Remaining work

Second coherent piece (written, not executed):

- `session/tests/mixer_preset_native.rs`: CLAP and VST3
  `*_running_helper_preset_restores_pcm_history_and_reopen` join real scanner,
  running helper, native state, library file, new-track restoration, independent
  steady-PCM reference, zero callback allocation, history and reopen. Requires
  `WINDFALL_BRIDGE_FIXTURE` and `WINDFALL_DESKTOP_BRIDGE_HELPER` pointing at the
  integrator's explicitly built fixture and current desktop executable.
- `session/tests/audio_logger.rs`:
  `enable_song_play_dump_preserves_sample_alignment_through_playlist_and_reopen`
  compares nonperiodic signed bursts and recovered PCM frame for frame after
  enable/play/dump, then checks one undo, redo and fresh reopen.
- `sampler-second-pass.qa.test.tsx`: real inspector envelope/LFO knob drags,
  one gesture/history step, undo/redo and WASM JSON reopen; mounted tempo-fit
  draft/apply/history/reopen. Device preparation is a fixture adapter; native
  spectral DSP remains covered by the existing sampler preparation suite.
- `transport/recording.qa.test.tsx`: real transport panel Record/start/keep
  through synthetic device capture and real WASM attachment/history/reopen.
  The native transport acceptance now also saves/reopens its actual take.
  A late stop reply is refused by frontend project-generation guard.
- `RealtimeFrame.rejectedEchoes?` carries the engine's actual refused-repeat
  counter into an imperatively updated accessible rack output. Regenerate
  this binding; no generated file was edited.

Additional sequential integrator commands:

```bash
cargo test -p windfall-desktop --lib session::tests::mixer_preset_native
cargo test -p windfall-desktop --lib enable_song_play_dump_preserves_sample_alignment
cargo test -p windfall-desktop --lib session::tests::transport_acceptance
pnpm --dir apps/desktop test src/features/channel-rack/sampler-second-pass.qa.test.tsx src/features/transport/recording.qa.test.tsx --maxWorkers=4
```

Check fixture control ID 7, gain interpretation and helper scheduling first;
then mounted knob roles, tempo-fit sample metadata and action accessible names.

This document is updated at each coherent commit. Rack preview/GC, mounted
gesture/tempo-fit, echo telemetry/runtime cases, joined native mixer preset,
logger and transport cases, sidechain/region stems and the expanded editor
are still being implemented. Physical devices and listening remain external.

## Regeneration and risks

No generated files edited. This first piece changes no generated type shape.
Check the new MIDI test's pattern length/repetition assumptions first.

Third piece (written, not executed): instrument-channel echo frequency,
partition equality, allocator and one-frame cost smoke; seek/live versus
sequenced loop discard; decoded native sidechain stems and selected-region
alignment; mounted real export panel with guarded region request.

Run sequentially:
```bash
cargo test -p windfall-engine --test engine rack_voice
cargo test -p windfall-desktop --lib sidechain_stems_measure_lane_isolation
pnpm --dir apps/desktop test src/features/export/playlist-tracks.qa.test.tsx --maxWorkers=4
```
Check exact sidechain detector attenuation and selected-region endpoint lengths
first. One-frame timing is a bounded-work smoke test, not a driver deadline.

Fourth piece (written, not executed): prepared-result Preview renders through
the very same options/kernel as Apply, auditions the rendered audio, returns
its waveform/duration, and makes no file or history entry. Stale previews
refuse before publication. Cleanup tracks only successful outputs created by
this session; document/undo/redo roots, imported files and saved/backup assets
are retained. Unknown files and older-session assets are never deleted.
Tests in session/tests/sampler_preprocess.rs cover preview equivalence, stale
preview refusal, undo/redo retention, saved reopen, abandoned output cleanup
and preservation of imported/unowned files. The real inspector has Preview,
Stop and cleanup controls plus mounted WASM no-history assertions.

Run sequentially:
```bash
cargo test -p windfall-desktop --lib session::tests::sampler_preprocess
pnpm --dir apps/desktop test src/features/channel-rack/sampler-second-pass.qa.test.tsx --maxWorkers=4
```
New native commands: sampler_preprocess_preview, collect_derived_assets.
No dependency or persisted type changed. Cleanup is intentionally conservative
across app restarts: it cannot prove another saved document stopped using an
older asset. No arbitrary-directory scan is offered.
