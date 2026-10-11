# Analyzer/display joins — second pass, 2026-10-12

Branch: `gpt/p2-analyzers`. All changes and tests in this handoff are **written, not executed**. The worker is forbidden to build/test. No native capture or real-window result is claimed.

## Current work

- Added `session/tests/analyzer_join.rs`, registered as a child of the existing analyzer tests. The fixture captures raw analyzer JSON strings from `Session::analyzer_subscribe` (the same path as the Tauri channel command), and native realtime JSON. Pure 440 Hz / 1875 Hz clips are loaded through the real clip preparation path, processed by the real processor, and advanced through source selection, seek/region loop, limiter release, calibrated tuner/moved source, PDC, withheld acknowledgement/recovery, stopped audio/recovery, rate, invalid sample-pool audio, deletion and project replacement.
- Native test `native_window_join_capture_and_checked_fixture_agree` writes to a validated target-relative `WINDFALL_ANALYZER_CAPTURE_DIR`. It compares checked capture payloads after removing process-issued ticket identities, CPU/xrun figures and temporary sample paths. The captured strings remain untouched. Determinism and exact fixture comparison require integrator review.
- Limiter history exposes the last actual input/output/reduction/gate/frame values for window inspection. Tuner forgets readings when a frame carries none, and exposes the displayed calibrated result. EQ clears held spectrum when no valid sounding spectrum arrives.

## Explicit unfinished gate

Checked-in native capture bytes cannot be produced under the worker's edit-only rule. The integrator must run the capture once, inspect it, and check in the resulting fixture. The normal native and frontend suites deliberately refuse missing fixtures; they do not substitute authored PCM/event payloads. All eight rows remain partial until that capture and joined/window execution are accepted.

## Capture bootstrap (integrator only)

From the repository root in Git Bash after `source scripts/msvc-env.sh` and with `TS_RS_EXPORT_DIR` pointing to a task-specific temporary directory:

```bash
WINDFALL_ANALYZER_CAPTURE_DIR=target/analyzer-join cargo test -p windfall-desktop --lib native_window_join_capture_and_checked_fixture_agree -- --test-threads=1
mkdir -p apps/desktop/src/features/analyzers/fixtures
cp target/analyzer-join/native-join.json apps/desktop/src/features/analyzers/fixtures/native-join.json
cargo test -p windfall-desktop --lib native_window_join_capture_and_checked_fixture_agree -- --test-threads=1
```

Do not promote a failed/incomplete capture. Commit reviewed native fixture bytes. Subsequent runs compare them and refuse drift. Other joins/window script and exact frontend commands will be recorded in the next commit.

## Mounted replay (written, not executed)

`apps/desktop/src/features/analyzers/native-join.test.tsx` test `replays real native session channel bytes through the mounted displays and every source lifecycle` parses raw captured strings, uses the real feed subscription callback, mounts real canvas painters against a recording context, and mounts the stage clock/meter, both waveform meters, limiter history, EQ spectrum and the extracted actual tuner readout. It asserts independent tone bins, stereo sides/vector geometry, native levels and waveform updates, clock text/wrap, calibrated A4/cents and changed tuner source, limiter input/reduction/release, PDC-aligned frame labels, rate, invalid levels and lifecycle clears. A missing capture fails explicitly. The optional target capture path and checked-in path exercise the same consumers.

The native service uses the same select/tick bodies with an explicit clock for deterministic capture; ordinary shell calls still supply `Instant::now`. The frontend rejects duplicate/out-of-order sequences and stale live tickets/terminal source callbacks. A 500 ms local freshness check clears evidence when the UI resumes after a freeze. Mixer waveform canvases clear immediately on project replacement.

```bash
WINDFALL_ANALYZER_CAPTURE_DIR=target/analyzer-join pnpm --dir apps/desktop test src/features/analyzers/native-join.test.tsx --maxWorkers=1
pnpm --dir apps/desktop test src/features/analyzers src/features/effects/analyzer-editors.test.tsx src/features/effects/tuner src/features/mixer/waveform-meter.qa.test.tsx --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```

After native fixture promotion, the first command also runs without the environment variable. Least-sure areas: native pool replacement invalid-PCM propagation and exact repeat-capture determinism; limiter release envelope under Stop; effect movement ownership; mocked canvas overload typing and animation scheduling. These are strict unexecuted assertions to repair against actual behavior, not accepted evidence.
