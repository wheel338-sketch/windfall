# Wave 2 row acceptance — 2026-10-11

## Running progress

- Starting checkpoint: `feat/parity-100` at `98b0bf2f`; clean, no merge in progress. Read the full integrator brief and dynamics/mastering hand-off. Process inventory found no stray Windfall build/test process.
- Integration order: dynamics/mastering; EQ/filter/stereo; delay/modulation/reverb; nonlinear/performance; spectral; subtractive/FM; additive/physical/drum; slices/grains; formats.
- Branch 1 dynamics/mastering merged at `b0f06187`. `chk.sh` now reports exit 0, zero errors, zero warnings. Resolved registrations without duplicating existing audio logger modules; retained existing exhaustive cases and appended MasterLimiter. Fixed test block-size type inference and removed an unused helper. Targeted audio/UI tests and row reconciliation remain pending.
- Branch 2 EQ/filter/stereo merged at `649c3f1f`; `chk.sh` exit 0, zero errors/warnings. Kept both routing/editor additions. Expanded the transient test's scratch storage to three outputs and cleared Transient Shaper's newly unused third output. Targeted audio/UI tests remain pending.
- Branch 3 delay/modulation/reverb merged at `a8e14789`; `chk.sh` exit 0, zero errors/warnings on its first run. Preserved editor registrations and unconditionally registered effect history admission while retaining both existing QA modules.
- Branch 4 nonlinear/performance merged at `4da6d6c7`; `chk.sh` exit 0, zero errors/warnings on its first run. Preserved all editor/test registrations, with existing audio logger registrations kept once.
- Branch 5 spectral merged at `0e7a9f93`; `chk.sh` now exit 0, zero errors/warnings. Combined prepared IR creation with existing delay admission/error mapping; retained transport plus harmony methods and both preset validations. Fixed two optional TS scalar annotations and the native preset-name pattern.
- Branch 6 subtractive/FM merged at `08a4a941`; `chk.sh` now exit 0, zero errors/warnings. Nine kinds appended after the existing 27; corrected integer descriptor literals and removed an unused macro import.
- Branch 7 additive/physical/drum merged at `bebb7b01`; `chk.sh` now exit 0, zero errors/warnings. Registry is 43 kinds: original 27, nine subtractive/FM, seven additive/drum. All command, preset, editor and allocator-test additions retained; corrected four dispatch arms whose common bodies/braces were shared by conflict hunks.
- Branch 8 slices/grains merged at `1bc20964`; `chk.sh` now exit 0, zero errors/warnings. Both asset identities participate in reuse and persist in every ledger. Fixed source patterns and fixtures for the separate instrument asset, and a worker's misplaced stems helper field. All command examples/editor/test additions retained.
- Branch 9 formats merged at `6855ca84`; `chk.sh` now exit 0, zero errors/warnings. Kept worker glide-duration/omission reporting and both engine modules; fixed SendTapParams module path. All nine are merged. Next: generated bindings/WASM, focused tests and full gates; row acceptance and wave 1 classification remain pending.
- Build/test commands will run sequentially with eight Cargo jobs and four Vitest workers. No devices/ignored tests, parity inventory edits, pushes or delegation.

## Verified execution record

| Gate | Verified result | Evidence |
| --- | --- | --- |
| Branch 1 `chk.sh` | Cargo exit 0; zero errors/warnings | `target/round2-branch1-check.log` |
| Branch 2 `chk.sh` | Cargo exit 0; zero errors/warnings | `target/round2-branch2-check.log` |
| Branch 3 `chk.sh` | Cargo exit 0; zero errors/warnings | `target/round2-branch3-check.log` |
| Branch 4 `chk.sh` | Cargo exit 0; zero errors/warnings | `target/round2-branch4-check.log` |
| Branch 5 `chk.sh` | Cargo exit 0; zero errors/warnings | `target/round2-branch5-check.log` |
| Branch 6 `chk.sh` | Cargo exit 0; zero errors/warnings | `target/round2-branch6-check.log` |
| Branch 7 `chk.sh` | Cargo exit 0; zero errors/warnings | `target/round2-branch7-check.log` |
| Branch 8 `chk.sh` | Cargo exit 0; zero errors/warnings | `target/round2-branch8-check.log` |
| Branch 9 `chk.sh` | Cargo exit 0; zero errors/warnings | `target/round2-branch9-check.log` |

## Row acceptance

| Row and required summary | Passing test evidence (see exact-file index) | Still open / hand-off reconciliation | Recommendation |
| --- | --- | --- | --- |

## Exact-file index

Pending integration and execution.

## Per-branch repairs

- Dynamics/mastering: registration/format conflicts preserved both intents; test-only closure needed explicit `usize`; unused test helper removed. No assertion or tolerance changed.
- EQ/filter/stereo: shared routing tests and editors retain both additions. AuxBlock grew from two to three buffers; transient test supplies all three. Product integration fix clears the unused third Transient Shaper output; test now asserts silence there.
- Delay/modulation/reverb: editor and module-registration conflicts only; no post-merge compiler fixes needed. Fallible preparation retains the integrated slot scratch layout.
- Nonlinear/performance: editor/test-registration conflicts only; no post-merge compiler fixes needed.
- Spectral: merged IR preparation with delay history admission into one fallible build; preserved transport/harmony and preset validations. ts-rs optional scalar fields need `as = "Option<u32>"`; native preset naming ignores the new embedded impulse field. No audio assertions changed.
- Subtractive/FM: duplicate import conflict resolved; 33 integer descriptor rows used float literals incompatible with integer clamp. Corrected literal types without changing values/ranges/order; removed redundant trait import.
- Additive/physical/drum: joined registries in merge order, exhaustive metadata/dispatch, editor routing, allocator tests and both preset asset patterns. Restored shared dispatch bodies/braces after initial union resolution; no worker implementation or tolerance change needed.
- Slices/grains: retained both immutable asset identities in reuse/ledger publication; joined source guards, command examples, editors and tests. Widened four source patterns and two test constructors. Removed a misplaced field preceding the stems helper's actual Channel literal. No assertions weakened.
- Formats: retained source-length slide duration and unknown-marker loss accounting; joined engine module exports. Corrected SendTapParams to its public module path.
