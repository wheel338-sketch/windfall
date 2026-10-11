# Slice, grain and curve instruments — implementation hand-off

Branch: `gpt/w3-slices-grains`, base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
Edit-only worker: no build, test, generator, browser or hardware check executed.

## Current implementation

All five rows are in implementation, not accepted. Existing Slice Map, Slice Deck,
Grain Cloud and Wave Ride registrations are reused; no registry indices reordered.
SamplePlayback authoring data is stored on Channel, omitted when absent. Source IDs,
up to 128 regions (including overlapping/reordered regions), deck B, envelopes and
shared/per-key position or speed curves are separate from scalar Copy parameters.
Control-side PlaybackAsset retains original decoded stereo AudioBuffer allocations.
Grain scheduling stays bounded to 16 voices × 32 grain slots. Speed curves use
analytic piecewise-linear integration independent of callback partitions.
SetSamplePlayback and GenerateSlicePattern use existing channel/pattern edit history.
Native plan preparation owns asset validation, unit construction and retirement.
Saved legacy v1 primitive tables/maps/curves migrate into immutable channel assets.
Scalar descriptor prefixes stay fixed; former map/curve descriptor suffixes retire.
Old automation targeting those suffixes must be refused with a migration diagnostic;
this migration gate needs dedicated tests and integration inspection.

## Remaining work in progress

Purpose-built source/slice/grain/curve editors, native/browser detection endpoint,
and registered acceptance tests have now been written. Test files are all **written,
not executed**: granular/tests.rs; dsp/sample_playback.rs; project/tests/sample_playback.rs;
engine/tests/sample_playback.rs; session/tests/sample_playback.rs; frontend
sample-playback/{editor,detection}.test.{tsx,ts}; params/access.test.ts and EVERY_COMMAND.
Detection applies one history step after source/generation/revision checks, with
independent shared Rust execution in a browser worker. Backend mocks label sources
simulated. Full-row candidacy is awaiting the integrator's compilation and evidence.

## Integrator commands (not run by this worker)

Regenerate bindings/descriptors and simulator using the combined integration branch:
`scripts/gen-bindings.sh <temporary-output>`; `scripts/build-sim.sh`.
Then `cargo test -p windfall-dsp --lib granular`,
`cargo test -p windfall-project --test sample_playback`,
`cargo test -p windfall-engine --test sample_playback`,
`cargo test -p windfall-desktop --lib session::tests::sample_playback`,
`cargo test -p windfall-dsp --test dsp params`, and existing project/engine suites.
Frontend: `pnpm --dir apps/desktop test src/features/sample-playback src/features/params/access.test.ts src/lib/ipc/sim/document.test.ts --maxWorkers=4`,
then typecheck/lint and package-scoped strict Clippy.

## Risks to inspect first

New hand-maintained Rust/TS asset types and old Channel literals; instrument unit reuse
must account for saved asset and decoded source identity; legacy map automation suffix
migration. Nothing is claimed to compile or pass before the integrator runs it.
