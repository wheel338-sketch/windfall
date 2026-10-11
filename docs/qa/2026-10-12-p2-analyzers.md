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
