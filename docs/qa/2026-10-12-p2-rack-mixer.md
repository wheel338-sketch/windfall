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

This document is updated at each coherent commit. Rack preview/GC, mounted
gesture/tempo-fit, echo telemetry/runtime cases, joined native mixer preset,
logger and transport cases, sidechain/region stems and the expanded editor
are still being implemented. Physical devices and listening remain external.

## Regeneration and risks

No generated files edited. This first piece changes no generated type shape.
Check the new MIDI test's pattern length/repetition assumptions first.
