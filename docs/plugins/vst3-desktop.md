# VST3 desktop ownership and state

Scanned, usable VST3 instruments and effects can now be added in the Windows desktop app. Loading independently verifies the scanned binary identity and plugin ID, declared format/path extension, and instrument/effect role. Bundle scanning, cache freshness, approval and loading resolve the same nested binary; approval retains its scanned canonical path, size and modification time. A missing or changed inner binary is rejected until rescanned, even when bundle-directory metadata is unchanged. Cache schema 2 discards older entries for rescan. Failed or blocked scan entries remain unavailable. Offline rendering creates independent native instances; it cannot select the playback editor or state owner.

## Native lifecycle and ownership

`windfall-plugin-host::ownership::exchange` allocates two single-item queues and an atomic request flag before installation. Every installed plugin receives a control boundary, including stopped, bypassed and leaving units. Empty callbacks also service the exchange before plan messages are applied.

At the boundary, audio moves its adapter into the return queue. This operation performs no native lifecycle call, allocation, free, lock, wait, file operation or job submission. Queue fullness preserves local ownership. During absence, instruments are silent and effects pass dry input; installed latency and tail metadata remain available. Bypass and silence continue to follow engine policy.

The creating native owner waits up to 500 ms for the returned adapter, then quiesces it and captures inactive state. VST3 `setProcessing(false)` and `setActive(false)` refusals propagate through fallible `deactivate`, `release_effect` and `release_instrument` APIs. The error owns the exact returned processor/adapter, including pending parameters and note metadata. The instance remains active until successful deactivation is proven. Inactive-state checks in the VST3 backend remain enforced.

On refusal, the exact active adapter is returned to audio. After successful capture the owner prepares a replacement adapter on the same native instance, checks latency/tail, and queues it for the next boundary. Save failures still attempt safe recovery. Failed processors, failed reactivation or changed latency/tail report an error and leave the old slot silent/dry. Save/backup/export propagate recovery errors. For a requested native restart, the session preserves proven inactive state even when the old slot cannot resume, reports the recovery error, and installs a replacement plan with correct compensation. Failed processors remain failed until explicit retry. Native teardown runs on the creating owner, including refusal and recovery failure retirement. An outstanding processor after API misuse or a failed owner channel is deliberately leaked to avoid wrong-thread teardown.

A missing callback times out without deactivating an outstanding processor. The owner services a return racing cancellation by resuming the exact adapter; notification maintenance never quiesces it. A queued control job cancelled at the generic 15-second deadline cannot later execute. Once a job has started, cancellation is cooperative and the caller joins native work and recovery before returning. Thus its recording exclusion remains held throughout a late native operation. An in-process native call that hangs cannot safely be interrupted; this join can exceed the nominal deadline. Plugin crash/hang isolation is outside this ownership contract.

## Deferred dirty state

Owner maintenance turns native dirty/state, restart, parameter-rescan and latency notifications into coalesced capture tickets. Tickets carry playback token, installed revision and binding fingerprint. Native parameter gestures continue through the existing bounded document-update path. Non-gesture writable parameter changes schedule capture; read-only meters do not.

The session worker retains tickets during recording and transient failures, retrying after recording exclusion becomes available. It checks the document identity briefly under `State`, releases that lock before native capture, and checks the identity again before applying the result. Native state and current editable parameter values form one undoable document edit. Restart waits until these edits have been reconciled. Obsolete token/revision/binding tickets cannot change a replacement document. Temporary waiting status clears after recovery.

Deactivation-generated dirty, rescan, restart and leftover controller updates are serviced while inactive before serialization. Their final native parameter values and latency changes are included in the capture, so reopening cannot overwrite a captured preset with stale document values. Notifications already covered by capture do not schedule an endless dirty loop. Save/backup/export use the same ownership path and preserve their recording and stale-snapshot guards. CLAP still saves active state without pausing its processor.

## Controls during an ownership pause

The desktop wrapper preallocates one pending value and atomic delivery metadata per writable binding parameter and fixed 128-key held/replayed tables. Parameter changes arriving without an adapter retain their latest values even after engine caches mark them applied. Rejected admission also retains the latest value for bounded retry. Document commitment is published before plan delivery; empty callbacks, admission and absent or failed adapters do not acknowledge native application. Tempo and transport follow their latest state.

State capture preserves outstanding committed parameter intent, including an edit not yet observed by a callback. Active CLAP capture keeps its opaque native state and overlays outstanding values in the companion project parameters; reopening and offline creation apply those parameters after loading opaque state. VST3 capture reconciles outstanding controls while proven inactive, before serialization. A parameter edit generated by native deactivation supersedes older accepted or rejected controls. Successfully processed newer automation remains eligible for native readback. Failed capture does not acknowledge controls, and stale parameter snapshots are rejected. A dirty capture ticket is acknowledged only after the session accepts its document edit. See [runtime repairs](runtime-repairs.md) for ordering, bounds and regression evidence.

Notes reconstruct from the current held state immediately before processing, after engine commands and sequencer expiry. Reconstruction first releases old native notes and then replays only currently held keys. Note-offs, UI releases, hardware epoch panic, sustain-translated releases, transport stop and removal therefore cannot replay an obsolete note. Partial admission retries remaining keys on later blocks. A native state/revision replacement on the same channel inherits engine live/hardware/sequence ownership and velocities; a removed channel or different plugin does not.

The shared host retains the CLAP saturation fix: ordinary admission stays at 1024 events, immediate adapter releases use preallocated reserved capacity, and accepted main-thread parameter edits remain queued until admission has room. Inactive CLAP parameter flushes use ordered 1024-event chunks. The VST3 native event list has 4225 preallocated slots for all 2048 channel/key pairs, ordinary event expansion and reserved releases. Processing and reconciliation do not allocate or free host memory.

Document adoption is observed before engine parameter deduplication through a callback-local metadata hook. Matching adoption alone does not acknowledge delivery; later successful native processing with no outstanding rejected control or new drops does. This preserves newer automation after a committed value equals the engine cache. VST3 input parameter points have a separate 5376-slot bound covering inactive, editor and ordinary inputs together. Readback advances after actual native completion, and failed/refused points retain final intent for owner recovery with truthful drop counts. See [R4 ordering and capacity evidence](runtime-repairs.md#r4-deduplicated-adoption-and-native-parameter-points).

## Verification and limits

Windows/MSVC tests use the repository's independent VST3 and CLAP DLL fixtures. They exercise native processing, inactive capture, refusal/recovery, dirty notifications deferred through recording, deactivation-generated edits, song automation and note expiry during absence, UI/hardware ownership, replacement, stop/removal, queued cancellation, running-job join, late return, stale plans, independent offline instances, session save/backup/reopen, WAV export, scanner role/failure gating, and committed CLAP parameter edits during hardware saturation with unchanged playback tokens. Calibrated thread-local allocator counters measure zero host callback allocations, reallocations and frees through exchange and event reconciliation. They do not measure memory allocated inside an arbitrary external plugin DLL.

Fixture editor routing and the explicit no-editor error are tested. A native editor window, installed external plugins and physical audio/MIDI devices remain unverified. These checks use headless buffers on Windows; no Linux or macOS execution is claimed. Test-only fixture helpers retain Windows gates.

Final checks use `scripts/msvc-env.sh`, `CARGO_BUILD_JOBS=1`, worktree-local `CARGO_TARGET_DIR=target/vst3-enable-verification`, and a task-specific temporary `TS_RS_EXPORT_DIR`. UI checks use locally regenerated types and the actual WASM simulator with at most four workers. Generated bindings/WASM, plan/parity files and publication changes belong to the integrating parent and are excluded from this feature's commits.

The original production-enablement commit `7476b074ffaff593e9a9f392665d1a1e8067a1c9` was verified on Windows on 2026-10-07:

- `cargo test -p windfall-plugin-host --all-features --all-targets`: 106 passed, including native processing-stop and deactivation refusals, reserved releases, and ordered inactive parameter flushing.
- `cargo test -p windfall-desktop --lib`: 202 passed. The final run uses `RUST_TEST_THREADS=1` so fixture build helpers run sequentially.
- `cargo test -p windfall-engine --all-targets`: 314 passed.
- Host all-feature/all-target and engine/desktop all-target Clippy checks with `-D warnings`, workspace and standalone fixture formatting checks, UI typecheck/lint, and six plugin UI tests with `--maxWorkers=4` passed.

The committed CLAP parameter saturation regression was reproduced before its repair: 1024 hardware note-ons plus empty callbacks left the native value at `0.5` after the document committed `0.75`. Retaining rejected final values makes this same real-fixture regression pass with unchanged playback token and zero callback allocator calls. The native VST3 release regression also covers reset expansion plus saturated notes and a final reserved release without callback allocation. Deferred dirty-state tests capture the deactivation-generated `0.375` preset and its latency change from 17 to 64 samples before installing the replacement plan.

Follow-up persistence, nested-binary identity and multi-channel release repairs have separate scoped verification recorded in [runtime repairs](runtime-repairs.md). The full workspace and UI checks above are historical evidence for the enablement commit, not repeated full-suite claims for these repairs. No generated artifacts are included in the repair commit. Binary identity uses metadata rather than a content hash; it does not provide tamper detection or eliminate replacement between validation and native loading.

## 2026-10-11 hosting source checkpoint

See [the worker hand-off](../qa/2026-10-11-plugin-hosting.md) for ABI 7 channel/instance
replay, negotiated expression capabilities, real helper editor lifecycle and
guarded native dirty capture, DSP wrapper policies, and the unverified macOS
AUv2 adapter. Earlier ABI/editor-limit notes are historical where superseded.
All new regressions are written, not executed; platform/corpus/packaging gates
remain explicit and no whole hosting row is newly verified.
