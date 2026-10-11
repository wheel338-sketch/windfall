# Slice, grain and curve instruments — implementation hand-off

Branch: `gpt/w3-slices-grains`; base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
Implementation commits: `b1ca47d4` (immutable assets/DSP/hosting), `e1170f09`
(editors, guarded detection, acceptance cases). Restart checkpoint: worktree was clean.

**Edit-only:** no build, test, generator, browser, audio or hardware check executed.
All tests below are **written, not executed**. No parity status has been changed.

## Row status at restart

| Row | Claim | Implemented | Remaining before acceptance |
| --- | --- | --- | --- |
| inst-fruity-slicer | Full-row candidate | Slice Map: shared transient/grid detector, original sources, key mapping, pattern generation, slice editor | Integrator compilation, generated artifacts, test and mounted UI evidence |
| inst-fruity-slicer-2 | Full-row candidate | Slice Deck: per-slice gain/pitch/reverse/envelopes, overlaps, ordering, dedicated editor | Same integration gates |
| inst-slicex | Full-row candidate | Slice Deck with two independent source decks, per-region deck selection and editing | Same integration gates; confirm row scope against richer workflow |
| inst-fruity-granulizer | Full-row candidate | Grain Cloud: size/density/spacing/position/spray/pitch/seed controls, bounded deterministic scheduling | Same integration gates |
| inst-wave-traveller | Full-row candidate | Wave Ride: shared/per-key drawn position or integrated speed curves, dedicated editor | Same integration gates |

## Implemented contract

Existing Slice Map, Slice Deck, Grain Cloud and Wave Ride registrations are reused;
no instrument registry indices reordered. Channel.samplePlayback is optional and
omitted when absent. Original sample IDs, up to 128 arbitrary regions, deck B,
envelopes and shared/per-key curves live outside scalar Copy parameters. Immutable
PlaybackAsset retains decoded AudioBuffer allocations. Construction and retirement
remain control-side; callbacks use fixed 16-voice/32-grain-slot storage.

SetSamplePlayback and GenerateSlicePattern use normal channel/pattern history.
Native and simulator detection prepare off-thread and reject stale generation,
revision or source identity before publishing one edit. Asset/source identity joins
instrument reuse keys; asset replacement copies active fixed playback state into
an already prepared unit. Ordinary save/reopen and undo/redo retain authoring data.

Legacy v1 mono tables/maps/curves migrate into immutable saved assets. Existing
scalar descriptor prefixes retain their indices. Former marker/per-slice/curve
suffix automation is refused with a migration diagnostic; it is not silently
mapped to a different parameter. Original files must be retained for those lanes.

## Acceptance files (written, not executed)

- DSP: `crates/windfall-dsp/src/granular/tests.rs` and
  `crates/windfall-dsp/tests/dsp/sample_playback.rs`: measured boundaries, reverse,
  overlaps/deck B, deterministic density, analytic curves, partition/release and
  allocation/free guards. `tests/dsp/params.rs` pins scalar descriptor prefixes.
- Project: `crates/windfall-project/tests/sample_playback.rs`: asset/history/pattern
  generation, save/reopen, atomic invalid input, synthetic stereo transient/tempo
  detection and legacy migration.
- Engine: `crates/windfall-engine/tests/sample_playback.rs`: all four kinds live vs
  offline PCM, history/reopen, source replacement and silent release/export tail.
- Native: `apps/desktop/src-tauri/src/session/tests/sample_playback.rs`: one-history
  detection, source preservation/reopen and stale edits/source/document barriers.
- Frontend: `apps/desktop/src/features/sample-playback/{editor,detection}.test.*`:
  mounted slice, grain, curve controls and real shared Rust detector fixtures.
  `features/params/access.test.ts` and `lib/ipc/sim/document.test.ts` extend the
  existing descriptor and EVERY_COMMAND acceptance surfaces.

## Remaining static review

Check native fixture source selection, mounted editor replacement scope, legacy
suffix automation rejection coverage, callback plan-adoption allocator coverage,
and update the obsolete granular integration seam. Factory content is generated
and must not be hand-added; use existing CC0 factory samples or authored test PCM.

## Integrator commands (not run by this worker)

On the combined integration branch regenerate using the repository workflows:
`scripts/gen-bindings.sh <temporary-output>` and `scripts/build-sim.sh`.
Run `cargo test -p windfall-dsp --lib granular`,
`cargo test -p windfall-dsp --test dsp sample_playback`,
`cargo test -p windfall-dsp --test dsp params`,
`cargo test -p windfall-project --test sample_playback`,
`cargo test -p windfall-engine --test sample_playback`,
`cargo test -p windfall-desktop --lib session::tests::sample_playback`.
Then run existing project/engine suites, package-scoped strict Clippy and frontend
`pnpm --dir apps/desktop test src/features/sample-playback src/features/params/access.test.ts src/lib/ipc/sim/document.test.ts --maxWorkers=4`,
followed by repository typecheck/lint commands. Confirm package/script names before
running; this worker has not executed any of these commands.

## First integration risks

1. Hand-maintained Rust/TS types and Channel literal additions; regenerate all
   asset types/descriptors and simulator WASM together before frontend checks.
2. Prepared unit reuse/adoption and off-thread retirement under active notes.
3. Legacy custom Channel deserialization and retired automation suffix diagnostics.
4. Exact live/offline equality and authored transient tolerances are unexecuted.
5. Existing sequencer seek policy skips notes starting before seek (no note chase).
   Asset edits capture slice settings for subsequent notes; held notes retain their
   captured region/envelope while preserving playback progress.
## Restart continuation — static corrections

Native fixture selection now names the imported authored-loop channel. The mounted
instrument section keys playback editors by generation/channel/kind; replacement
cannot retain an obsolete busy state. Curve fields guard endpoints, finite values
and no-op blur edits. New tests cover per-key curves, one pointer gesture/cancel,
replacement remount, legacy automation suffix diagnostics, no-chase/boundary seek,
and callback asset adoption/source replacement/retirement allocation/free guards.
All remain **written, not executed**.

An incompatible instrument/preset replacement clears the obsolete playback asset
within the same undo step. Channel preset export now explicitly refuses these
assets rather than silently losing sample/map/curve data: the existing preset
asset format only carries one sampler source. Project save and channel duplication
retain the complete asset. Portable sample-instrument channel presets remain a
separate format-extension limitation; plugin presets carry scalar controls only.
The integrator must also run `cargo test -p windfall-engine --test engine sample_playback`.
