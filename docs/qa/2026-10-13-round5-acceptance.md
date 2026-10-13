# Round-five acceptance — 2026-10-13

Integration and acceptance are IN PROGRESS. No unexecuted gate is credited.

## Running record

- Started clean on `feat/parity-100` at `2957d3ccb2d87d338817c8130944466563ae478a`, main checkout only. Read round-four acceptance (including gates, benchmark integrity, cross-check and row decisions), device acceptance hybrid-core scheduling evidence, QA completion rule, roadmap sections 3/9, worker contract and incoming hand-offs. No subagents, worktree edits/builds, branch creation, push, PR or tag. All builds/tests run serially through `target/round5/run.sh` in Git Bash with MSVC setup, `CARGO_BUILD_JOBS=8`, isolated `TS_RS_EXPORT_DIR` and Node/pnpm PATH. No real device harness, playback, capture or ignored hardware tests authorized.
- Script-tempo merge staged; workspace check pending. Hand-off adds raw/script identity, append-only script addresses, live/offline parameter automation and feedback-safe Tempo integration; no dependencies.

## Gate table

Pending. Counts will be taken from actual execution logs.

## Deadline harness

Pending implementation. Budget definition: share of one performance core; limits and workloads unchanged. Prior device record (inherited evidence, not rerun here) unrestricted workspace: 4546 passed / 9 failed / 40 ignored; performance-class affinity instrument run: 21 passed / 0 failed.

## Cross-target checks

Pending. No platform-specific module is credited yet.

## Dependency/manifest changes

Pending final diff audit. No dependency downloaded at this checkpoint.

## Row acceptance

Pending full integration and actual gate execution. Tracker files remain untouched.
- Script-tempo check PASS: `cargo check --offline --workspace --all-targets`, zero errors/warnings, exit 0, 2 s (`target/round5-branch01-check-repair1.log`). Initial check failed on a fixture binding that retained a mutable Clip rather than ClipId; use its id, preserving all expectations. No assertions changed.
- Interchange check PASS: `cargo check --offline --workspace --all-targets`, zero errors/warnings, exit 0, 18 s (`target/round5-branch02-check.log`). No conflicts or compile repairs. WAV annotation authoring/native transfer/timeline candidate integrated. FLP and legacy synth/readers explicitly remain SOFTWARE per hand-off.
- Rack-assets check PASS: `cargo check --offline --workspace --all-targets`, zero errors/warnings, exit 0, 15 s (`target/round5-branch03-check-repair1.log`). Initial check found bridge note controls named gain rather than the actual volume field, and deprecated atomic fetch_update; repaired to volume and try_update. No conflicts; asset/native rack tests unexecuted at this checkpoint.
- Shell check PASS: `cargo check --offline --workspace --all-targets`, zero errors/warnings, exit 0, 33 s (`target/round5-branch04-check.log`); subsequent deadline-harness all-target check PASS, exit 0, 1 s (`target/round5-branch04-harness-check.log`). Conflict resolution retains rack editor ownership/retirement, localization, native capability publication and rack note gain combined with capability-aware pan mapping. Shell hand-off explicitly retains updater runtime memento/capture/endpoint expansion, transitive localization, isolated-bridge pan-law/actual-channel expression selection SOFTWARE items.
- Hosting-dynamic check PASS: `cargo check --offline --workspace --all-targets`, zero errors/warnings, exit 0, 14 s (`target/round5-branch05-check-repair4.log`). Conflicts retain rack retirement, native capabilities, localization and auxiliary-output copying; pan-block endpoint retains its new auxiliary offset. Compile repairs add default OutputBlock auxiliary fields, default rack PluginBinding capabilities, correct project PanLaw type and capture acknowledgement import. Initial/repair1–3 failures retained. ABI 9 fixture/helpers must be rebuilt together before tests.
- Nonwindows-editors check PASS: `cargo check --offline --workspace --all-targets`, zero errors/warnings, exit 0, 2 s (`target/round5-branch06-check-repair1.log`). Preserve Linux GUI host context in the extracted VST3 constructor, shared metadata reader, helper restart flags plus editor refusal reporting, bridge editor-open bookkeeping and application scale. Initial Windows-only unused mut warning repaired with cfg shadowing. Existing raw-window-handle 0.6.2 direct desktop Linux edge refreshed offline; no package downloads.
- `cargo fmt --all`: PASS, exit 0, 5 s (`target/round5-fmt-apply.log`). Formatted the integrated edit-only branches. Preliminary release instrument run is active; no result credited yet. Its live inventory selects class 1, CPU-set IDs 256–271 / logical 0–15 without external process affinity. CPAL 0.18.2 realtime feature promotes WASAPI callback via audio_thread_priority MMCSS Audio; active parallel Workers use Pro Audio/HIGH with owned lifetime and no affinity. The alternate public TrackWorkers utility is not the active DSP lane path.

## Preliminary deadline measurements (before remaining merges)

`cargo test --offline --release -p windfall-dsp --test dsp instrument_deadlines -- --test-threads=1 --nocapture`: **21 passed / 0 failed / 0 ignored**, exit 0, test time 135.07 s / total 183 s (`target/round5-instruments-release-first.log`). No external affinity or priority. Each variant retains its full two-second maximum workload, plus full release/tail and allocator/signal/repeatability assertions. One unrestricted diagnostic trial precedes three highest-class trials; each trial resets the instrument and runs the identical workload and tail. Limits remain 0.75 share, original capacities and bounds.

[Windows CPU-set documentation](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-system_cpu_set_information) defines larger efficiency classes as faster processors. Dynamic symbol loading falls back to unrestricted measurement on unavailable APIs; size-bounded variable records skip unknown types/exclusively foreign allocations. The RAII guard retains the prior thread CPU-set IDs and clears/restores them on exit, without process-affinity changes. Active DSP lanes already use MMCSS Pro Audio/HIGH and CPAL realtime registers callbacks as Audio. Neither product path pins cores.

The paired unrestricted/selected numbers below were measured in this run. Multiple rows per family are its original distinct workload variants. They are not a physical callback deadline claim.

| Instrument variant | Unrestricted whole workload | Selected minimum | Median | Worst | Min CPU share |
| --- | ---: | ---: | ---: | ---: | ---: |
| Number Voice | 560.2715ms | 545.4391ms | 547.3649ms | 556.5612ms | 0.2727 |
| Layer Kick | 30.1671ms | 31.601ms | 31.6789ms | 31.7638ms | 0.0158 |
| Color Scan | 1.3249767s | 927.5548ms | 929.1539ms | 1.0504623s | 0.4638 |
| Finger Bass | 3.3905ms | 1.801ms | 1.8042ms | 1.8137ms | 0.0009 |
| Drum Rack | 465.7319ms | 226.7803ms | 227.2793ms | 228.6171ms | 0.1134 |
| Membrane | 15.4073ms | 14.6809ms | 15.247ms | 15.5906ms | 0.0073 |
| Drum Voice | 51.2166ms | 24.0295ms | 24.6243ms | 25.0989ms | 0.0120 |
| Drum Voice | 35.364ms | 24.9008ms | 25.2108ms | 25.2358ms | 0.0125 |
| Drum Voice | 26.7152ms | 23.3899ms | 23.641ms | 23.7389ms | 0.0117 |
| Drum Voice | 72.2751ms | 24.6042ms | 24.6052ms | 24.7108ms | 0.0123 |
| Pack synth | 710.6014ms | 411.9692ms | 414.3047ms | 417.3701ms | 0.2060 |
| Pack synth | 818.8565ms | 397.8881ms | 400.4017ms | 495.8414ms | 0.1989 |
| Pack synth | 783.8088ms | 420.8986ms | 433.0234ms | 457.9252ms | 0.2104 |
| Pack synth | 1.0355513s | 567.4746ms | 569.4043ms | 573.1798ms | 0.2837 |
| Pack synth | 817.36ms | 414.2133ms | 415.4886ms | 416.1671ms | 0.2071 |
| Pack synth | 749.8313ms | 386.3731ms | 387.4066ms | 388.4929ms | 0.1932 |
| Pack synth | 765.9847ms | 409.8717ms | 417.606ms | 467.1543ms | 0.2049 |
| Pack synth | 1.0026074s | 586.0261ms | 586.1929ms | 588.4314ms | 0.2930 |
| Pack synth | 676.6441ms | 415.975ms | 416.5983ms | 431.847ms | 0.2080 |
| Pack synth | 730.95ms | 386.8525ms | 391.6684ms | 393.0303ms | 0.1934 |
| Pack synth | 784.7922ms | 416.7123ms | 417.7265ms | 421.581ms | 0.2084 |
| Pack synth | 1.1346178s | 594.1668ms | 594.1845ms | 598.5468ms | 0.2971 |
| Groove synth | 665.285ms | 310.2093ms | 312.6142ms | 339.8893ms | 0.1551 |
| Groove synth | 1.6049514s | 807.3363ms | 808.7729ms | 844.1552ms | 0.4037 |
| Groove synth | 798.813ms | 385.1144ms | 385.9192ms | 437.9763ms | 0.1926 |
| Groove synth | 1.9823926s | 1.0523187s | 1.066453s | 1.1593871s | 0.5262 |
| Harmonic Filter | 2.2536229s | 1.1664351s | 1.2228048s | 1.2839321s | 0.5832 |
| Spectral Resynth | 1.6829591s | 931.6188ms | 931.7389ms | 934.3066ms | 0.4658 |
| Dual vintage poly | 1.4865041s | 788.1463ms | 789.8868ms | 904.0943ms | 0.3941 |
| Vintage poly | 1.2647589s | 608.9817ms | 614.0745ms | 614.3251ms | 0.3045 |
| Pocket synth | 246.6112ms | 128.6006ms | 128.9453ms | 129.0355ms | 0.0643 |
| Snapshot Morph | 1.687098s | 910.3053ms | 946.8347ms | 953.7638ms | 0.4552 |
| Metal Voice | 1.7388748s | 976.3958ms | 985.2649ms | 1.0437114s | 0.4882 |
| Compact synth | 1.1081336s | 543.2273ms | 543.4632ms | 590.7128ms | 0.2716 |
| Vintage saw | 1.0351132s | 578.9775ms | 585.6475ms | 587.0565ms | 0.2895 |
| Matrix synth | 1.9824147s | 1.0956364s | 1.1046662s | 1.1146482s | 0.5478 |
| Hybrid sequencer | 1.7276453s | 1.025451s | 1.0363626s | 1.0392422s | 0.5127 |
| Hybrid sequencer | 1.6898272s | 1.0075318s | 1.0650557s | 1.0772217s | 0.5038 |
| Hybrid sequencer | 1.7052122s | 1.0289805s | 1.033521s | 1.0819831s | 0.5145 |
| Hybrid sequencer | 1.6177088s | 1.010378s | 1.0250469s | 1.0257405s | 0.5052 |
- Software/native-window harness all-target check PASS, zero warnings/errors, exit 0, 26 s (`target/round5-software-check.log`). Added debug-only Engine::software memory sink using the actual Processor and explicit Software status, with owned suspend/reopen/close requests and no real audio/MIDI startup; smokes explicitly select it. Existing silent flag only turns down a real output and was insufficient for this task. New memory-sink callback/reopen test pending. Native smoke results must be labeled software/UI, never hardware deadlines.
- Engine library focused/regression run PASS: `cargo test --offline -p windfall-engine --lib -- --test-threads=1`, **319 passed / 0 failed / 2 ignored**, exit 0, 3.01 s tests / 133 s total (`target/round5-engine-focused-first.log`). Includes registered script_tempo_tests raw identity/gates/Tempo, dynamic plugin routing/PDC, modular generation, allocator and new software memory-sink callback/reopen. No ignored test executed. Current code still precedes soundfont/evidence/plugin-export merges.
- Prebuilt fixture/helper bootstrap PASS: `cargo build --offline --manifest-path crates/windfall-plugin-host/test-plugins/Cargo.toml --target-dir target`, exit 0, 2 s (`target/round5-fixture-build.log`); `cargo build --offline -p windfall-desktop -p windfall-plugin -p windfall-plugin-host --bins --lib`, exit 0, 234 s (`target/round5-native-helper-build.log`). Both precede evidence/soundfont/plugin-export and will be rebuilt if relevant source changes.
- Evidence branch check PASS after one unused fixture closure-variable repair: workspace all-target check zero errors/warnings, exit 0, 2 s (`target/round5-branch07-check-repair1.log`). No merge conflicts. New PianoKeys 24-scenario held/release/tail gate shares the self-contained CPU-set/minimum/median/worst/unrestricted harness; 0.05/0.20 release budgets and debug diagnostic distinction unchanged. New video window smoke uses explicit software-memory-sink metadata; no audio device will be opened.
- Frontend merge review repairs (execution pending): translated dynamic output-route/removed-parameter diagnostics in en/es, preserved application-locale subscriptions, and made rack native parameters honor tombstone unavailability just like standalone controls. Used resolving-merge-conflicts and shadcn skills; no component/package installed. Ordinary workspace-first command active; no suite result credited until completion.
