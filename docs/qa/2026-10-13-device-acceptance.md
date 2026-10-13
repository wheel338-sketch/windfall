# Windfall real-driver acceptance — 2026-10-13

Work started at `469be35c274f4e3d3dd47b6163f9da9609ed1c4e` on
`feat/parity-100`, with a clean tree. One agent, one build/test at a time;
`CARGO_BUILD_JOBS=8`. No worktrees, new branches, pushes, PRs, tags, parity
registries, system software installations, default-device changes or volume
changes. Results below distinguish software driver acceptance from physical
interface/listening acceptance.

During execution another process advanced this branch to `9244a26c` with
parity documentation only. Those edits were not made or staged by this agent.
Each JSON records the actual parent commit and dirty flag of its run.

## Machine and endpoint boundary

Windows 11 Home x64, `10.0.26300.9550`, Intel Core i9-14900F, 34,163,732,480
bytes installed RAM, America/New_York. Tests ran overnight. Output stimulus
was confined to `Realtek Digital Output (Realtek(R) Audio)` (unconnected
optical/S-PDIF) or `Speakers (Steam Streaming Speakers)` (virtual), with
only the matching WASAPI `Loopback:` input. No microphone was opened.

**Execution error:** the first inventory command omitted the output selector
and opened the default `Speakers (Stealth 700X Gen 3)` for 0.21 seconds. Inventory
created no stimulus and opened no capture stream, so no deliberate sound was
played. This violated the endpoint selection rule. It was immediately followed
by explicitly selected Realtek inventory; every subsequent device command used
an explicit allowed output. The original JSON is retained for audit, not
accepted as allowed-route evidence.

The inventory lists WASAPI only without the ASIO feature. The allowed outputs
offer two channels, a reported 480-frame period at 48 kHz, and 13 rates from
8 kHz through 384 kHz. Their loopbacks are two-channel 48 kHz shared mix format.
Enumeration also lists the headset, HDMI, BRIO and ASUS noise-cancelling
endpoints; these were never selected for subsequent output or capture.

## Reproduction commands

All Cargo commands run from the checkout in Git Bash after:

```bash
source scripts/msvc-env.sh
export CARGO_BUILD_JOBS=8
export TS_RS_EXPORT_DIR="$(cygpath -w "$(mktemp -d)")"
export WINDFALL_DEVICE_EVIDENCE="$(cygpath -w "$PWD/target/device-evidence")"
export WINDFALL_DEVICE_HOST=WASAPI
export WINDFALL_DEVICE_OUTPUT="Realtek Digital Output (Realtek(R) Audio)"
export WINDFALL_DEVICE_INPUT="Loopback: $WINDFALL_DEVICE_OUTPUT"
export WINDFALL_DEVICE_LABEL=realtek
```

Each scenario uses `cargo test -p windfall-desktop --lib SCENARIO -- --ignored
--test-threads=1 --nocapture`, with stdout/stderr in
`target/device-SCENARIO-LABEL.log`. Steam probes change both output and matching
loopback input and use a distinct label. `WINDFALL_DEVICE_THREADS=1` selects
serial processing for diagnosis only; the final acceptance uses Automatic.

## Failures and repairs

1. Both allowed endpoints originally refused the take within 100 ms with
   `The input device reported a dropout: samples were lost.` Instrumentation
   showed `Xrun` before the first input packet. CPAL 0.18.2 suppresses the
   initial discontinuity only when WASAPI's device position is zero; a
   loopback's first packet can have a nonzero position. Each stream now treats
   only an xrun notice before its first packet as its startup boundary.
   Subsequent xruns and every other fatal error still reject the take, with
   timestamp discontinuity, sequence and tail checks unchanged. Coverage
   includes ordinary, timed and shared multitrack capture. Regression verifies
   startup scope, subsequent dropout and other-host/device-loss behavior.
2. Loopback callback timestamp was zero beside a valid absolute QPC capture
   stamp. Subtracting callback/capture timestamps discarded that stamp and
   substituted delivery time. Input and output now map driver timestamps
   through `Stream::now()` into the recording clock's monotonic epoch,
   initialized off the audio callback before playback. Zero startup output
   timestamps do not seed the DAC fit. Regression covers capture with a zero
   callback timestamp and output startup anchors. No correlation or timing
   threshold was loosened.
3. Completed Automatic takes still suffered hundreds of output xruns.
   A 120 s release song soak failed with **3,762 xruns**, peak CPU
   **1,428.09%**, actual buffer **480**, 4,213,152 frames played. Serial
   processing eliminated xruns (peak CPU about 1%); its second take had
   0.000 ms jitter and 0.001 ms largest step. Windows workers' ordinary 25 us
   timed parks can wake at an OS timer quantum, stranding the realtime callback
   at the completion barrier. Workers now use owned high-resolution waitable
   timers and MMCSS Pro Audio registration on Windows; no global timer
   resolution changes or callback OS
   wakes/waits were introduced. Timer failure yields rather than taking a
   long timed park. Timers close on their worker thread.
4. The monitoring harness previously required a non-feedback route and could
   not safely exercise the only permitted inputs. For same-device loopback it
   now plays the stimulus only on port 2, captures port 2 in mono, and monitors
   exclusively on port 1. The captured port cannot contain its monitored
   signal. Queue starvation/drop checks remain unchanged.
5. The output period fit previously compared a raw timestamp with a smoothed
   earlier anchor. Phase errors fed back into its rate estimate and produced
   a false startup drift curve ending in a step. DAC phase now follows the raw
   driver anchor; period is fitted over at least a second of raw frame/time
   pairs, and a phase step starts a new fit. Regression proves a phase step
   does not become sample-clock drift.
6. Monitor backlog draining compared driver-sized packets with two *mixer
   subdivision* lengths. Ordinary priming/delivery bursts exceeded that
   bound and dropped 517 frames despite no starvation. The producer now
   publishes its converted packet quantum; excess draining accounts for both
   producer and reader quanta. Queue capacity and bounded excess draining
   still constrain a genuinely stalled monitor. Regression verifies both
   normal packet/subdivision delivery and real backlog drops. Final standalone
   monitoring had zero dropped and zero starved frames, zero output xruns,
   and 19.688 ms reported queue depth at the last sample.
7. The settings dialog called callback-buffer duration "output latency" and
   asserted the advertised 480-frame inventory buffer was fixed, although
   actual callbacks at 44.1/96 kHz were 441/960 frames. The dialog now labels
   callback duration truthfully, calls inventory sizes advertised, and states
   that the running buffer can differ by rate. Mounted regressions cover the
   measured 44.1/48/96 kHz cases while preserving a requested 128-frame value.
   Their first run failed because the test replaced its backend *after* app
   startup without refreshing the engine-status store. The fixture now invokes
   the existing `refreshEngineStatus()` command before rendering. This repairs
   test setup without changing the display assertions or product event flow.

Initial failures are retained as separate JSON reports, including the startup,
timestamp-mapping, buffer and serial diagnostic runs. The original serial raw
take detected 24/24 bursts but had -1,439.391 ppm fitted residual drift,
3.738 ms jitter and 2.681 ms largest step; this exposed the invalid output
startup anchor. Its calibrated second take was -0.002 ppm / 0.000 ms /
0.001 ms, but -3.729 ms mean offset, so the scenario correctly remained red.

### Earlier virtual cable

The 2026-10-10 route used **Speakers (Steam Streaming Microphone)** as output,
which this brief forbids. That exact route was not reopened. Its historic
failure after 1.7 s / 74,946 frames and 3,830 ppm cannot be attributed
conclusively to the new startup fault: it had already received audio. Today's
first-fault and timestamp findings explain reproducible loopback failures,
not proof of the historical cable's cause. A permitted Steam Streaming
Microphone second input remains eligible for a separate input probe.

## Real-driver scenario results

All final rows below ran on the explicitly selected Realtek output/loopback
with Automatic processing and requested buffer 480. Inventory ran first;
settings also exercises requests 1, 128 and 1,024. The label is `realtek`.
Full measurements and check wording are in
[`device-evidence/2026-10-13/`](device-evidence/2026-10-13/); JSON only, no audio.

| Scenario / command test filter | Result and measured evidence |
| --- | --- |
| `device_inventory` | PASS: selected Realtek is listed; all WASAPI outputs have loopback inputs. |
| `device_round_trip_alignment` | PASS: raw 24/24 bursts, mean 0.873 ms (41.914 frames), drift -0.432 ppm, jitter 0.010 ms, largest step 0.035 ms. Calibrated 24/24, mean 0.001 ms (0.041 frames), drift 0.945 ppm, jitter 0.011 ms, largest step 0.024 ms. Zero output xruns in both takes; transport/region, undo/redo pass. |
| `device_count_in_gate` | PASS: delay 2,011 ms / 2,000 ms expected; 12/12 bursts both takes. Without count-in mean 0.958 ms, drift 1.190 ppm, jitter 0.012 ms, step 0.040 ms. With count-in mean 0.953 ms, drift -2.008 ppm, jitter 0.007 ms, step 0.021 ms. Lead peak 8.91e-10 versus burst peak 0.02747; pre-roll trim and transport restoration pass. |
| `device_loop_takes` | PASS: three 96,000-frame complete passes and a 58,010-frame partial; first three total 288,000 frames. Complete-pass mean offsets 0.850/0.835/0.843 ms (spread 0.015 ms); each finds 4/4 bursts, partial 2/2. Complete-pass drift -28.525/0.457/8.038 ppm; jitter 0.004/0.001/0.009 ms; steps 0.021/0.003/0.020 ms. First tick, muting, group and single undo pass. |
| `device_multitrack_arms` | PASS: two 384,411-frame clips. Post-fader tap: 16/16, zero mean/drift/jitter/step, score 1. Hardware loopback: 16/16, mean 0.895 ms, drift 1.143 ppm, jitter 0.010 ms, largest step 0.029 ms. One group/two lanes and one undo. |
| `device_input_monitoring` | PASS: 48 kHz, remains active for 8 s, no error; zero drops/starvation/output xruns; stop closes the group. Port isolation recorded explicitly. |
| `device_external_output_ports` | PASS: stereo endpoint folds to mono on port 2; assigned port 10/10 bursts, mean 0.851 ms, drift -3.504 ppm, jitter 0.011 ms, step 0.024 ms; port 1 detects zero bursts. |
| `device_audio_settings` | PASS: 13 advertised rates × 4 requested buffers = 52 trials; all run at their offered rate, preserve request, deliver callbacks and invalidate the prior clock ticket. Invalid zero-channel configuration reports failure; selected driver reopens through session configuration. |

An additional round trip used `WINDFALL_DEVICE_RATE=44100`,
`WINDFALL_DEVICE_BUFFER=128`, `WINDFALL_DEVICE_LABEL=realtek-44100` with the
same output and loopback. The driver delivered **44.1 kHz / 441 frames** while
capture remained **48 kHz**, exercising conversion onto a different output
sample grid. **PASS:** both takes detect 24/24 bursts and have zero xruns.
Raw mean 1.646 ms (72.607 frames), drift 0.048 ppm, jitter 0.010 ms, largest
step 0.024 ms. Calibrated mean -0.002 ms (-0.069 frames), drift 0.121 ppm,
jitter 0.007 ms, step 0.027 ms. This demonstrates real shared-mode conversion,
not independent physical ADC clock drift. The extra mean latency is calibrated
by the same manual offset mechanism; no route-specific threshold adjustment
was needed.

These offered rates are WASAPI **client stream** rates, which are also the
engine's processing rate. The loopback/shared device mix format remains 48 kHz;
Windows converts other client rates. This is not proof the optical converter
changed its physical clock, and no system mix-format setting was changed.

At 44.1/48/96 kHz, requested buffers 1/128/480/1,024 all delivered respectively
441/480/960 frames. These are 10 ms shared-mode callback periods. The session's
`engine_settings()` retains the request and `engine_status()` reports actual
delivered callback frames, which the existing settings UI formats separately.
All those trials had zero xruns. This is the smallest callback period granted
through the current CPAL WASAPI path, not a claim of 1- or 128-frame hardware
operation. CPAL 0.18.2 initializes `AUDCLNT_SHAREMODE_SHARED` through
`IAudioClient::Initialize`; its public stream config exposes no exclusive mode
or `IAudioClient3` low-latency shared option. Adding a functioning setting would
require a new/modified backend, so no inert mode selector was added. Physical
ASIO and multichannel buffers remain external acceptance.

## Release soak

Before and after the DSP scheduling repair, with explicitly selected Realtek
and no input stream:

```bash
cargo run --release -p windfall-engine --bin windfall-soak -- --seconds 120 \
  --host WASAPI --device "Realtek Digital Output (Realtek(R) Audio)" \
  --sample-rate 48000 --buffer 480 --song --silent
```

The song includes effects, a synth, audio clip and automation. `--silent`
ramps the engine's own output gain to zero before playback; it does not touch
system volume. Processing was Automatic. Final result: **PASS, 0 xruns,
44.92% peak callback CPU, actual/requested buffer 480/480, 48 kHz,
5,770,656 frames played** over 120 seconds. Before the worker timer/priority
repair: 3,762 xruns and 1,428.09% peak CPU. Logs are `target/device-soak.log`
and `target/device-soak-final.log`; both results are archived as JSON.

This is a brief stability check, not a long unattended soak or proof against
arbitrary system load. It directly reproduces and verifies the worker
scheduling regression on this machine.

## ASIO build preparation

No LLVM installation was needed. Commands:

```powershell
& 'C:/Users/ewhee/AppData/Local/Programs/Python/Python312/python.exe' -m venv target/device-venv
& target/device-venv/Scripts/python.exe -m pip install libclang
```

`libclang` 18.1.1 installed into that venv only; `LIBCLANG_PATH` points to
`target/device-venv/Lib/site-packages/clang/native` (`libclang.dll`). The SDK
was downloaded from the official redirect
<https://download.steinberg.net/sdk_downloads/ASIO-SDK_2.3.4_2025-10-15.zip>,
extracted under `target/device-asio-sdk/ASIOSDK`, and selected with
`CPAL_ASIO_DIR`. ZIP SHA256:
`D5EBF0C20DD2C5F43771FD0C1418F4B361BF52434EE670097CFA6B3A335E2ECA`.
Neither DLL nor SDK is committed, and no Rust dependency or lockfile changed.

```bash
export LIBCLANG_PATH="$(cygpath -w "$PWD/target/device-venv/Lib/site-packages/clang/native")"
export CPAL_ASIO_DIR="$(cygpath -w "$PWD/target/device-asio-sdk/ASIOSDK")"
cargo test -p windfall-engine --features asio --lib -- --test-threads=1
cargo test -p windfall-engine --features asio --lib device_stream_error_recovery -- --ignored --test-threads=1
cargo run -p windfall-engine --features asio --bin windfall-soak -- --list-devices
```

ASIO compilation and non-device tests: **305 passed, 0 failed, 2 ignored**
(the opt-in real output recovery test and existing realtime benchmark).
Logs: `target/device-asio-tests.log`, `target/device-stream-recovery.log`,
`target/device-asio-drivers.log`. Driver enumeration exposes the **ASIO host
with zero drivers**. The real-backend injected recovery test passes: the old
stream delivered 14,976 frames, the recoverable fault produced an observed
stopped/reconnecting status, the actual CPAL stream closed and reopened, the
new stream delivered 14,976 frames, and the previous capture ticket became
invalid. The selected rate remained 48 kHz. Physical removal was not induced.
A feature build is not a physical ASIO-device latency/compatibility test.

## Final gate setup and execution

`cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D
warnings` pass, logs `target/device-fmt.log` and `target/device-clippy.log`.
The first full Rust invocation lacked the repository's explicit native fixture
variables: native bridge tests failed on missing environment values and a
later test attempted a nested desktop Cargo build. That run was stopped;
`target/device-workspace-tests.log` retains its partial failure evidence. This
was a harness setup error, not a product repair or reason to skip assertions.
The existing `target/round4-run.sh` documented the necessary prebuilt paths.

The final run builds each fixture/helper serially, then supplies the same
variables without a nested build:

```bash
cargo build --manifest-path crates/windfall-plugin-host/test-plugins/Cargo.toml --target-dir target
cargo build -p windfall-desktop --bin windfall-desktop
cargo build -p windfall-plugin
export WINDFALL_BRIDGE_FIXTURE="$(cygpath -w "$PWD/target/debug/windfall_test_plugins.dll")"
export WINDFALL_DESKTOP_BRIDGE_HELPER="$(cygpath -w "$PWD/target/debug/windfall-desktop.exe")"
export WINDFALL_EXPORT_FIXTURE="$(cygpath -w "$PWD/target/debug/windfall_plugin.dll")"
cargo test --workspace --no-fail-fast -- --test-threads=1
```

Logs: `target/device-native-fixture-build.log`,
`target/device-native-helper-build.log`, `target/device-native-export-build.log`,
`target/device-workspace-tests-final.log`. TS exports go to a fresh temporary
directory for tests. The new device checks are opt-in like the existing
harness; no existing test was ignored, skipped or weakened.

Because the settings wording changed TypeScript, the final frontend gate also
regenerates bindings and the simulator before typecheck/lint/tests. Results
and the six recommendations follow once these gates complete.

### Hybrid CPU benchmark conditions

The properly bootstrapped unrestricted workspace run completed **4,546 passed,
9 failed, 40 ignored**. The only failed target was the unchanged DSP instrument
deadline module: BeepMap, GMS (tail deadline), Harmless, Harmor, Kepler Exo,
Morphine, Ogun, Sytrus and Toxic Biohazard. Main-render ratios included
0.9349/1.1260/0.9075/0.7561/0.9301/0.9773/1.1016/1.0363; GMS's main render
was 0.3800 but its release/tail deadline failed. The threshold remains 0.75.
The workspace DSP executable was exactly the same named binary
`dsp-7c8bf98e3fa1116b.exe` used by the earlier passing baseline; neither DSP
source nor tests changed in this task.

Windows `GetSystemCpuSetInformation` reports 16 logical processors at
EfficiencyClass 1 (0–15) and 16 at class 0 (16–31). Higher classes identify
faster processors. [Microsoft CPU-set definition](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-system_cpu_set_information).
A controlled test-process affinity of `0xFFFF` selected the higher class.
Only this agent's gate process and descendants were affected; system settings
and other applications were untouched, and the parent's original affinity was
restored. The same instrument profile source passed **21/0**, with BeepMap
0.4878, Harmless 0.5905, Harmor 0.4847 and Ogun 0.5125. This supports a hybrid
CPU scheduling/resource explanation; it is not evidence that every failed
thread's earlier CPU placement was observed directly.

The full workspace rerun uses the same package/feature configuration as the
unrestricted gate, with this affinity selection. It is recorded separately in
`target/device-workspace-tests-pcores.log`; the unrestricted failures are
retained in `target/device-workspace-tests-final.log`. The CPU-set inventory
and selected mask are in `benchmark-cpu-profile.json` and
`benchmark-cpu-workspace.json`. No benchmark threshold, assertion, optimization
profile, or DSP implementation was changed to obtain the controlled run.

Reproduce in a dedicated PowerShell process, setting affinity before launching
Git Bash with the preparation/environment above:

```powershell
$gateProcess = [Diagnostics.Process]::GetCurrentProcess()
$priorAffinity = $gateProcess.ProcessorAffinity
try {
  $gateProcess.ProcessorAffinity = [IntPtr]0xFFFF # verified higher-class mask on this machine only
  & 'C:/Program Files/Git/bin/bash.exe' -lc 'source scripts/msvc-env.sh && export CARGO_BUILD_JOBS=8 && export TS_RS_EXPORT_DIR="$(cygpath -w "$(mktemp -d)")" && export WINDFALL_BRIDGE_FIXTURE="$(cygpath -w "$PWD/target/debug/windfall_test_plugins.dll")" && export WINDFALL_DESKTOP_BRIDGE_HELPER="$(cygpath -w "$PWD/target/debug/windfall-desktop.exe")" && export WINDFALL_EXPORT_FIXTURE="$(cygpath -w "$PWD/target/debug/windfall_plugin.dll")" && cargo test --workspace --no-fail-fast -- --test-threads=1'
} finally { $gateProcess.ProcessorAffinity = $priorAffinity }
```

The deadline gate remains sensitive to unrestricted hybrid-core scheduling on
this machine. All real-driver acceptance, settings, injected recovery and both
soaks ran **without this affinity selection**, so their measurements do not
depend on it.

The full workspace with the verified higher-class mask passed **4,555 tests,
0 failed, 40 ignored**. It used the full workspace DSP binary
`dsp-7c8bf98e3fa1116b.exe`, the same one that failed deadlines under unrestricted
scheduling. The named 0.75 gates pass without DSP edits or changed thresholds.
Both unrestricted and controlled counts are intentionally reported; the former
is not relabeled green. The 40 ignored tests include pre-existing manual/resource
opt-ins plus the new opt-in settings/recovery device tests.

## Six parity recommendations

These recommendations apply to the six requested rows, based on real silent
WASAPI driver acceptance plus the existing software gates. They do not claim
physical-interface/listening or other-OS acceptance. No parity registry was
edited by this agent.

| Row | Evidence | Still open | Recommendation |
| --- | --- | --- | --- |
| `core-audio-recording` | 48 kHz and 44.1 kHz real-driver calibrated round trips; count-in; loop takes; multitrack; injected real-backend recovery; existing recording regressions. | EXTERNAL: physical microphone/line capture and analogue round trip, independent ADC clock drift, listening, actual device unplug. Exact historical cable failure unconfirmed because its output is forbidden here. No remaining identified SOFTWARE gap in this row. | `done` |
| `win-mixer-audio-io` | Isolated-port native input monitoring: zero drops/starvation; hardware output mono fold reaches port 2 only; real routing/stop checks. | EXTERNAL: a physical multichannel interface, actual headphone/input latency and feedback/listening checks, other platforms. No remaining identified SOFTWARE gap in this row. | `done` |
| `win-mixer-record-arm` | Hardware loopback plus post-fader tap: equal 384,411-frame clips, 16/16 bursts, exact tap placement, two lanes/one group/one undo. | EXTERNAL: independent second physical input and independent clocks, physical line/mic capture/listening. Steam Streaming Microphone was not substituted for an input carrying the stimulus; the passing scenario uses one loopback plus a mixer tap. No remaining identified SOFTWARE gap in this row. | `done` |
| `wf-audio-settings` | 52 real rate/buffer trials, requested versus delivered state/UI regression, output clock restart, invalid-config recovery, injected error reopen, 120 s zero-xrun release soak, ASIO compile/305 tests and enumeration. | EXTERNAL: physical ASIO and multichannel devices, lower native buffers on a supporting backend/interface, manual unplug and other OS. Current CPAL exposes no exclusive/low-latency shared request; no inert setting was added. No remaining identified SOFTWARE gap within the available backend's supported modes. | `done` |
| `wf-count-in` | Real one-bar count-in: 2,011 ms delay, equal burst alignment within 0.005 ms, no retained click, trimmed pre-roll and restored transport/region. | EXTERNAL: metronome/count-in listening and converter timing on a physical capture interface. No remaining identified SOFTWARE gap in this row. | `done` |
| `wf-loop-recording` | Three exact complete passes plus partial; 288,000-frame total; 0.015 ms pass-position spread; saved group, muting and one undo. | EXTERNAL: physical long-loop recording/listening and independent input clocks. No remaining identified SOFTWARE gap in this row. | `done` |

Separate unresolved validation limitation: the unrestricted hybrid-CPU DSP
deadline run had nine failures; the identical full-workspace binary passes
with the faster-class mask. That condition is recorded explicitly above and
is outside these six recording/settings rows. No DSP source/threshold was
changed. The earlier forbidden-route cable failure also remains unconfirmed,
rather than being assigned the startup diagnosis without evidence.

## Final verification and handoff

| Gate | Final result | Log |
| --- | --- | --- |
| `cargo fmt --all --check` | PASS | `target/device-fmt.log` |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS, zero warnings | `target/device-clippy.log` |
| `cargo test --workspace --no-fail-fast -- --test-threads=1` with documented CPU selection and native fixture paths | **4,555 passed / 0 failed / 40 ignored** | `target/device-workspace-tests-pcores.log` |
| Same full Rust command with unrestricted CPU scheduling | **4,546 passed / 9 deadline failures / 40 ignored**; limitation retained above | `target/device-workspace-tests-final.log` |
| ASIO feature non-device library tests | **305 passed / 0 failed / 2 opt-ins ignored** | `target/device-asio-tests.log` |
| `scripts/gen-bindings.sh` | PASS, 615 files regenerated; no generated diff | `target/device-bindings.log` |
| `scripts/build-sim.sh` and `node scripts/check-sim.mjs` | PASS, current 6,957,931-byte simulator; no generated diff | `target/device-build-sim.log`, `target/device-check-sim.log` |
| `pnpm --dir apps/desktop typecheck` | PASS | `target/device-typecheck-final.log` |
| `pnpm --dir apps/desktop lint` | PASS | `target/device-lint-final.log` |
| Focused settings UI regression | **18 passed / 0 failed** | `target/device-settings-tests.log` |
| `pnpm --dir apps/desktop test --maxWorkers=4` | **6,430 passed / 0 failed** | `target/device-frontend-tests-final.log` |

The first frontend run was 6,427 passed / 3 newly added fixture failures;
`target/device-frontend-tests.log` retains it. The fixture correction passed
both its focused file and the subsequent full frontend run. The final
generators produced no binding/simulator source changes. No new Rust
dependency, manifest, lockfile, parity registry or worktree edit is included.

JSON reports were first written under `target/device-evidence/` and copied to
`docs/qa/device-evidence/2026-10-13/`. Failed diagnostics remain named separately;
they are not counted as accepted runs. No audio files, SDK or local venv are
included. Logs stay under `target/device-*.log`. The handoff commit on
`feat/parity-100` contains the source/test repairs, this report and the JSON
evidence; its parent is `9244a26c` (the externally committed parity docs).
