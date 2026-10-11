# Analyzers (roadmap T8) — 2026-10-10

Branch `worktree-agent-a2bd1b4040219f843`. Rows: `vis-fruity-big-clock`,
`vis-fruity-db-meter`, `vis-fruity-spectroman`, `vis-wave-candy`,
`win-mixer-waveform-view`, `fx-fruity-parametric-eq2`, `fx-fruity-limiter`,
`fx-tuner`.

**No row is claimed complete.** Part-way through this work the rule changed
to "edit only, the parent builds and tests". Everything written after that
point has never been compiled or run. The two things that were run before
the rule changed are listed under "Executed", with their real results.
Nothing else in this document is evidence that anything works.

## What exists now

| Layer | Files | State |
| --- | --- | --- |
| DSP: limiter compress mode, gate, per-block observation; tuner block count; `AnyEffect::observation` | `crates/windfall-dsp/src/{limiter.rs,observe.rs,tuner/mod.rs,effect.rs,lib.rs}` | Compiled and tested before the rule change |
| Engine: two capture seats per stream, mix output or track post-fader, effect observations, tickets | `crates/windfall-engine/src/{capture.rs,capture_control.rs}`, hooks in `processor.rs`, `controller.rs`, `project_preparation.rs`, `mixer.rs`, `rack.rs`, `lib.rs` | Compiled and its own test file passed before the rule change. `Controller::heard_position` was added afterwards and is not compiled |
| IPC types | `crates/windfall-ipc/src/analyzer.rs` | Written, not compiled |
| Native service and commands | `apps/desktop/src-tauri/src/session/analyzers.rs`, `commands.rs`, hooks in `session/{mod,realtime,files}.rs`, `lib.rs` | Written, not compiled |
| Frontend seam, feed, maths | `apps/desktop/src/features/analyzers/{types,feed,math}.ts`, `lib/ipc/{backend,tauri,mock}.ts` | Written, not type-checked |

Later sections of this file are updated as each further piece lands.

## Executed (before the edit-only rule)

Windows 11, Git Bash, `scripts/msvc-env.sh`, `CARGO_BUILD_JOBS=4`.

| Command | Result |
| --- | --- |
| `cargo test -p windfall-dsp` | exit 0. `dsp` target 179 passed, 5 ignored; lib 454 passed; the other targets 26, 13, 6, 21 passed. This run included `limiter_dynamics` (12 tests) after its one wrong expectation was corrected |
| `cargo test -p windfall-engine --test analyzer_capture` | 9 passed, 0 failed |
| `cargo test -p windfall-engine` (whole crate) | **Did not run.** The linker failed with exit code `0xc0000142` on four test binaries while the machine was out of resources. No engine regression result exists |

`rustfmt --check` was run on the new Rust files (allowed under the rule) and
parses all of them; that is a syntax check, not a type check.

## Tests written

| File | Tests | Executed |
| --- | --- | --- |
| `crates/windfall-dsp/tests/dsp/limiter_dynamics.rs` (registered in `tests/dsp/main.rs`) | 12: appended descriptor order and legacy JSON; limit mode ignores the new settings bit for bit; ratio line; top ratio and knee; input gain against threshold; attack and release times; gate silences and passes; gate through low-note zero crossings; gate release time; ceiling holds in every mode; per-block observation without consuming the meter; gate and compression reported apart; tuner reports the calibrated note | Yes, passed |
| `crates/windfall-engine/tests/analyzer_capture.rs` | 9: distinguishable track/mix/master sources and stale tickets; refusals leave the seat alone; seek/play/stop/mode/new plan break the analysis, an ordinary loop does not; deleted source and reopened stream at another rate; late poll overflow and recovery; bitwise device-output equality with NaN/Inf/overflow input and gain ramps; limiter observations, PDC label and its change; compression and gate through the engine; tuner calibration | Yes, passed |
| `crates/windfall-ipc/src/analyzer.rs` `mod tests` | 2: wire shape of tagged sources, nulls, frame positions beyond 2^53; defaults | No |
| `apps/desktop/src-tauri/src/session/analyzers.rs` `mod tests` | 2: band summing; slot names | No |
| `apps/desktop/src-tauri/src/session/tests/analyzers.rs` (registered in `session/tests/mod.rs`) | 4: each slot gets its own source and no frame of an earlier choice, capacity refusals, shared seat; seek, deleted track, new project, new stream and rate; stalled view and stalled audio; limiter history and tuner reading through an effect slot | No |
| `apps/desktop/src/features/analyzers/math.test.ts` | 11 | No |

## Least-sure spots (go here first)

1. `session/analyzers.rs::analyzer_tick` — long function with split borrows
   of `State { subscriptions, seats }` and a closure passed to
   `Controller::capture_poll` that reads both. Most likely place for a borrow
   error.
2. `commands.rs::analyzer_subscribe` takes
   `Channel<tauri::ipc::InvokeResponseBody>` and sends
   `InvokeResponseBody::Json(String)`. Checked against the vendored Tauri
   2.12.1 source by reading, not by compiling.
3. `session/tests/analyzers.rs` assumes the default subtractive synth
   sustains a held note, that a hard-panned track has an exactly silent
   opposite side, and that the tuner locks to an A from the synth's default
   patch. The spectral assertions (`low_share`) are the softest.
4. That same test file waits for a stream that got capture seats, because
   the analyzer's quotas (8 taps, 8 slots) are per process and the desktop
   lib test binary runs many sessions at once. If it is flaky, run it with
   `--test-threads=1` first to tell quota contention from a real failure.
5. `capture_control.rs::heard_position` calls the private
   `Controller::playing`; it relies on being a child module of `controller`.
6. Frontend types in `features/analyzers/types.ts` are written by hand to
   match the Rust serde shapes; compare with the regenerated bindings.

## Parent must regenerate

- `apps/desktop/src/bindings/*` (`scripts/gen-bindings.sh`): new
  `LimiterMode`, wider `LimiterParams`, all `Analyzer*` types, and
  `descriptors.json` with the limiter's seven new settings. The limiter
  editor binds those ids and throws until `descriptors.json` has them.
- `apps/desktop/src/lib/ipc/sim/windfall_sim.wasm` and its metadata
  (`scripts/build-sim.sh`): `windfall-dsp` changed.

## Shared seam files touched

`crates/windfall-ipc/src/lib.rs` (two lines), `crates/windfall-dsp/src/effect.rs`
(one method appended to `AnyEffect`), `crates/windfall-dsp/src/lib.rs`,
`apps/desktop/src-tauri/src/commands.rs` (five commands appended, five names
in the handler list), `apps/desktop/src-tauri/src/lib.rs` (one line),
`apps/desktop/src-tauri/src/session/mod.rs` (module, re-export, one field,
one initializer), `apps/desktop/src/lib/ipc/{backend,tauri,mock}.ts`.

Outside the seam list, narrow edits: `session/realtime.rs` (one line: the
tick), `session/files.rs` (one line: project replaced), engine
`processor.rs`/`controller.rs`/`project_preparation.rs`/`mixer.rs`/`rack.rs`,
`crates/windfall-dsp/src/multiband/dynamics.rs` and several test files (a
`..LimiterParams::default()` on struct literals that no longer name every
field).
