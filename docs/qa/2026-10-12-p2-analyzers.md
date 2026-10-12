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

## Opt-in real-window smoke (written, not executed)

Two debug-only shell overrides were added:

- `WINDFALL_CONFIG_DIR`: absolute configuration directory, separating settings, plugin database, autosave and project artifacts from the user's configuration.
- `WINDFALL_OUTPUT_GAIN`: finite linear output gain in `[0,1]`; the smoke uses `0.01`. `WINDFALL_SILENT=1` still takes precedence. Nonzero gain is necessary because the mix-output analyzer is post-output-gain.

`analyzer_smoke_enabled` and `analyzer_smoke_control` are off unless this is a debug native build, `WINDFALL_ANALYZER_SMOKE=1`, and an absolute isolated configuration override is present. Release builds refuse controls and do not contain the fixture implementation. The frontend installs `window.__windfallAnalyzerSmoke` and mounts the display host only after the native gate allows it. Ordinary builds/startups mount no smoke views and expose no window bridge.

The native debug fixture creates independently authored stereo WAV tones (440 Hz at 0.8 hard left; 1875 Hz at 0.2 hard right), inserts a real limiter, tuner calibrated to 432 Hz, and EQ, saves a `.windfall` project and reopens it through the normal native project path. The host mounts the real views and the actual effect editors sequentially (respecting two capture seats). It reads the DOM values those consumers wrote. It does not synthesize healthy readings or report native payloads as displayed evidence.

`scripts/analyzer-window-smoke.mjs` launches the prebuilt app with a separate WebView2 profile and loopback DevTools port. It uses Node's built-in fetch/WebSocket, no new dependency or automation framework. It refuses an occupied port and closes its own child app. It collects clock position/time, master readouts, per-source meter values, spectrum peaks, per-track waveform extrema/serials, limiter input/output/reduction/history updates and release, tuner note/cents, EQ live spectrum, source refusal/clears and native subscription replacement after a real 2.3-second webview-thread stall. Both startup and live waits have deadlines; failures produce a JSON report and nonzero exit.

Integrator commands, from repository root:

```bash
# Existing generation only if needed after integration; this branch adds no generated DTOs.
# Build a debug app with bundled frontend, not a cargo-only dev executable expecting Vite:
pnpm --dir apps/desktop tauri build --debug --no-bundle
# Node 22 or newer; no install step:
node scripts/analyzer-window-smoke.mjs target/debug/windfall-desktop.exe target/analyzer-window/report.json 9237
```

A pass is exit 0 plus `passed: true`, every `checks[].pass: true`, and actual display values present in `samples[]`. A missing device, frontend bundle, WebView2 debugging endpoint, native gate or valid frame is a failure, never a passing unavailable/silent fixture. Inspect the report's native project path, source levels, tone bins (within one actual FFT bin), A4/+31.8 cents, limiter reduction above 3 dB with release, loop wrap and post-stall new subscription before recording acceptance. Rate/PDC/invalid-source propagation is covered by the joined suite; the window script does not change hardware sample rate or inject NaNs into a device stream. Physical instrument/microphone listening and other-OS/device timing remain external.

## EQ source-selection decision

No per-slot FFT tap is required by the accepted design. `ANALYZER-INTEGRATION.md` explicitly allows an EQ overlay consuming selected actual PCM separately from the parameter response, with its location disclosed. The actual EQ editor selects its own track post fader and labels that location. Consequently the spectrum includes subsequent effects and fader gain; it is not represented as the EQ slot's input/output or as a response calculated from settings. The joined replay checks native track audio and lifecycle, and the real-window host mounts the real response/FFT overlay together.

## Per-row handoff

All test files/tests above are **written, not executed**. No row is promoted.

| Row | Claim | SOFTWARE still not done and why |
| --- | --- | --- |
| `vis-fruity-big-clock` | Partial | Native channel replay and real-window play/seek/loop harness written; native captured fixture bytes and executed joined/window proof remain, forbidden in worker scope. |
| `vis-fruity-db-meter` | Partial | Captured per-source peaks, deletion/replacement, rate/PDC/invalid/stall assertions and window readouts written; checked capture and executed proof remain. |
| `vis-fruity-spectroman` | Partial | Spectrum/spectrogram painters receive native channel captures and known-tone/window checks are written; checked capture and executed proof remain. |
| `vis-wave-candy` | Partial | Real scope/meter/spectrum/spectrogram/vector consumers are mounted with native replay and lifecycle assertions; checked capture and executed proof remain. |
| `win-mixer-waveform-view` | Partial | Real native interest, track-separated waveform consumer/serial/extrema checks and immediate replacement clearing are written; checked capture and executed proof remain. |
| `fx-fruity-limiter` | Partial | Actual native history observations reach the scrolling consumer/editor; input/output/reduction/decay and lifecycle checks are written; checked capture and executed proof remain. |
| `fx-fruity-parametric-eq2` | Partial | Native track post-fader FFT, explicit source policy, held-spectrum clearing and real response-overlay smoke are written; checked capture and executed proof remain. |
| `fx-tuner` | Partial | Native calibrated tone, effect moved between sources, actual note/cents readout and no-reading clear are written; checked capture and executed proof remain. Instrument listening remains external. |

## Shared seams / generated types

Shared files changed: `apps/desktop/src-tauri/src/{commands.rs,lib.rs}` and `apps/desktop/src/features/layout/app-shell.tsx`. Narrow additional seams: native `session/{analyzers.rs,realtime.rs,tests/analyzers.rs}`, shell configuration, existing analyzer feed/canvas/status, stage clock/meter, mixer waveform and effect consumers. No project model/command, DSP registry, backend interface, generated binding or WASM edits. No new generated types to regenerate for this branch; the two debug commands use a bool and unexported JSON.

## Check in this order

1. Compile the native test and debug command modules. Check the exact native import/clip and region signatures, capture-seat availability after reopen, and injected invalid-pool publication through `Session::dispatch`.
2. Capture twice and compare the immutable payloads. The comparator intentionally removes only process-issued lease identities, CPU/xruns and temporary sample paths. Do not mask real numerical/order/clock drift to get a pass. Fixture event bytes themselves are untouched.
3. Run mounted replay/typecheck/lint. Particular risks: overloaded canvas spy return typing, Node file/env imports under the app TS config, effect-observation cadence after Stop, strict calibrated tuner acceptance, and async source replacement/rebind scheduling.
4. Build the debug bundled app and run the real-window script. Inspect whether WebView2 honors the loopback debugging environment in this build. Source selection waits for the native source identity, and UI-stall recovery requires a changed native subscription plus counted skipped events.
5. Review fixture size, provenance and JSON report; commit the genuine checked-in capture. No checked-in bytes were fabricated in this worker pass.

## Source-review follow-up (written, not executed)

- A replacement track gets a distinct native 880 Hz clip; replay asserts its 44.1 kHz peak bin, rather than accepting a silent/invalid mix as replacement evidence.
- Captured `project:loaded` JSON enters the actual frontend snapshot/reset boundary before old analyzer callbacks. The native selection reply for the renewed missing source is captured too. The realtime store now clears play/tick authority on project replacement, preventing the large clock from keeping the previous project's position until the next feed frame.
- Replay delivers an intact real predecessor frame late after source deletion and asserts it cannot revive the canvas/EQ. It checks vector leaning on both hard-panned sources.
- The window websocket handshake now has a five-second deadline. No build/test/lint/script was executed. The additional narrow seam is `apps/desktop/src/lib/store/realtime.ts`; generated types remain unchanged.
