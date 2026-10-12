# Native VST3 portability: diagnosis and proposed implementation boundary

Prepared 2026-10-08 on isolated branch `gpt/t3-vst3-portability-ci`, fixed base
`6d0773804199faba65f866feda21b8c4ba9a6b9a`. This is a diagnosis and design artifact.
No production change, source commit, native macOS success, or full-goal/parity
closure is claimed. The parent's full Windfall goal remains active (342 rows).

## Authority and current ownership

The authorized first stage permits read-only source/primary-SDK research, this
new document, and private `target/diagnostics/vst3-portability` artifacts. The
parent must grant exact production paths before implementation. N4 owns the
bridge/runtime/shared fixtures, T1 owns native meter processor/controller
gates, P1 owns preparation, and T8 owns new analyzers. Their edits must not be
reverted or imported into this branch. No nested agents were used.

`src/vst3/processor.rs`, shared fixture `test-plugins/src/vst3.rs` and `lib.rs`,
Cargo manifests/features/locks, and CI YAML remain closed. This stage ran no
Cargo command in the shared project root, installed no tools, dispatched no
remote CI, and created no push, release, or PR. There were no discovered
`AGENTS.md`, `CONTEXT.md`, or ADR files in the scoped checkout/guidance search.

## Actual CI failures and provenance

Run `37748566795`, head `43e42983`, supplies the original failures. Parent-provided
completed logs were read without alteration:

- `C:/Temp/windfall-43-macos-ci.log`, SHA-256
  `b7f9ce9d08332e8f98bfe4f01474def73685164ee53a3b9d3954e365f9517585`.
  Lines 3118–3195: 20 realtime tests, 12 passed and eight failed; every failing
  VST case reports `Unsupported("VST3 bundle entry on macOS")`.
- `C:/Temp/windfall-43-ubuntu-ci.log`, SHA-256
  `70e2e9a02033d45fa68a0524509708fd83198fd15e4f3f170bc9f51d32b5e959`.
  Lines 3521–3535: `realtime-7eed3941ad3392a3` reports four completed CLAP tests,
  then signal 11/SIGSEGV without a Rust assertion. Runner: Ubuntu 24.04.5,
  Rust 1.99.0, commit `b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`.

The logs/brief report strict workspace Clippy and binding/parity/fresh checks
passing on macOS and Ubuntu. That is compilation/check evidence, not native
VST3 execution success. Windows realtime 20-pass evidence is parent-reported;
this stage did not independently run Windows host tests.

The host crate, its fixture, core crate, and root Cargo manifests/lock are
unchanged between `43e42983` and this fixed base. DSP adds accepted E3 delay
code at the fixed base. The local diagnostic therefore uses identical host
and fixture source to the failed run, but a newer DSP source closure and a
different Linux distribution/kernel. It is not the original Ubuntu runner.

## macOS: proven refusal, plus two missing prerequisites

`crates/windfall-plugin-host/src/vst3/mod.rs:74` explicitly returns Unsupported
on macOS before resolving a binary or loading a factory. That proves why these
eight tests fail; none reaches native VST processing there:

- `r4_inactive_and_timed_vst3_points_reach_native_at_their_frames`
- `r4_disjoint_vst3_pending_editor_and_timed_controls_process_truthfully`
- `r4_full_editor_queue_and_other_native_point_sources_fit_without_allocating`
- `r4_failed_native_process_never_publishes_control_readback_and_retains_final_intent`
- `runtime_repair_vst3_ordinary_and_reserved_panics_release_all_channels`
- `vst3_effect_and_instrument_callbacks_allocate_and_free_nothing`
- `vst3_ownership_boundaries_move_adapters_without_allocator_calls`
- `vst3_reset_and_saturated_release_keep_every_admitted_note_off`

Simply deleting that return is insufficient. `tests/common/mod.rs::plugin_file`
currently copies the Rust cdylib to a raw file such as `fixture.vst3` on every
platform. `test-plugins/src/vst3.rs:898–916` exports the factory and Windows/Linux
entry pairs, but no `bundleEntry` or `bundleExit`. A proper macOS fixture needs
both a bundle and those exports. Existing `vst3_host.rs` and `vst3_scan.rs` also
have whole-file Windows/Linux cfg gates; removing those gates is a separate
expansion requiring platform-aware fixture changes and a grant, not a way to
fix or conceal the eight existing failures.

### Primary platform contracts

Steinberg SDK research is pinned to
`586dc5e6c8012c3e4b01c79389375cbe96bdb1da` (SDK 3.8.1). The official
[macOS host implementation](https://github.com/steinbergmedia/vst3_public_sdk/blob/586dc5e6c8012c3e4b01c79389375cbe96bdb1da/source/vst/hosting/module_mac.mm)
creates/loads a CFBundle, resolves all three required exports, calls
`bool bundleEntry(CFBundleRef)`, obtains an owned factory, releases that factory
before `bool bundleExit()`, and finally releases its CFBundle reference. Its
destructor does not explicitly call `CFBundleUnloadExecutable`.

The official [macOS plugin entry implementation](https://github.com/steinbergmedia/vst3_public_sdk/blob/586dc5e6c8012c3e4b01c79389375cbe96bdb1da/source/main/macmain.cpp)
retains the passed bundle and increments an entry counter before InitModule
returns. Consequently, an attempted entry that returns false can still need
the matching exit to balance SDK bookkeeping. A success-only exit flag would
miss that case.

Apple documents that [CFBundleCopyExecutableURL](https://developer.apple.com/documentation/corefoundation/cfbundlecopyexecutableurl(_:))
returns the actual executable URL with owned Copy-rule lifetime. The
[CFBundleExecutable key](https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/CoreFoundationKeys.html)
lets the OS locate the executable even when a bundle directory is renamed.
Thus the current stem-based `Contents/MacOS/<bundle-stem>` resolver is not a
complete macOS identity rule. Apple also documents
[CFBundleLoadExecutableAndReturnError](https://developer.apple.com/documentation/corefoundation/cfbundleloadexecutableandreturnerror(_:_:))
and owned CFError results, and describes
[unload hazards](https://developer.apple.com/library/archive/documentation/CoreFoundation/Conceptual/CFBundles/AccessingaBundlesContents/AccessingaBundlesContents.html).
The initial proposal follows SDK bundle ownership and release rather than
adding forced CFBundle executable unload. CFRelease must not be described as
proof of immediate code unmapping.

## Linux: native reproduction and precise unload mechanism

An existing Docker image made a native local feedback loop possible without
installing or downloading anything:

`rust@sha256:24e632c09342c20abf8312cf4f61430a911c01ed3a5e4c02b87292b1c39c5273`

It has Rust 1.99.0 (the CI compiler commit), GCC and binutils, and Debian 13.7
on x86_64 under WSL2 kernel `6.6.87.2-microsoft-standard-WSL2`. Containers use
`--pull=never --network=none`. The existing Cargo registry is mounted read-only;
all source snapshots, manifests and build outputs live under private target
diagnostics. No shared/root source or registry files are written.

`prepare_snapshot.py` copies 135 tracked inputs. The only input edit narrows
the *private* root workspace member list to host/DSP/core. An initial locked
attempt correctly refused that private workspace lock update. The subsequent
offline snapshot lock removes 499 unrelated packages and introduces no new
package version/source. `provenance-check.json` verifies zero changed copied
source, package manifests, original tests, or fixture inputs. The build uses
the original profile and dependency versions.

Observed, sequential final evidence:

1. Unchanged original 20-test parallel run: Cargo exit 101, underlying SIGSEGV.
2. `python target/diagnostics/vst3-portability/run_native.py single-direct`
   runs the built original test executable with one exact VST test and one
   test thread. Two isolated repeats exited 139 in 1.359 and 1.469 seconds.
3. `run_native.py serial-direct`: exit 139 in 2.109 seconds, after four CLAP
   successes and at the first VST test. This matches the original symptom
   pattern while proving parallel test execution is unnecessary locally.
4. `trace_fault.c` traces the unchanged executable and its test thread via
   ptrace. A second probe adds observation-only `observe_dlclose.so`.
   `trace-2.log` records the fixture's base at dlclose, then a SIGSEGV at an
   unmapped PC exactly `fixture_base + 0x555d0`.
5. `addr2line` on the exact fixture identifies that offset as
   `std::sys::thread_local::guard::key::enable::run`, Rust
   `library/std/src/sys/thread_local/guard/key.rs:20`. The return address is
   libc `+0x8ff91`, immediately after an indirect `call *%rdx` during pthread
   cleanup. `trace-1.log` independently records the unmapped fault without
   the dlclose observer.
6. A private differential that suppresses only fixture dlclose preserves its
   code mapping. One original test passes; a complete original parallel run
   reports **20 passed, zero failed/ignored/filtered**, exit 0 in 1.656 seconds.
   The retention probe is deliberately not a repair or normal validation.
7. A minimal independent Rust cdylib + C pthread host calls thread identity,
   dlcloses the cdylib, then exits the worker. `std::thread::current().id()`
   produces exit 139 after dlclose; replacing only the identity operation with
   native `pthread_self`/`pthread_equal` produces exit 0 and a joined pthread.
   See `minimal-verdict.json` and the two minimal logs.

The fixture registers a Rust pthread/TLS cleanup callback through
`std::thread::current().id()` at component construction (`vst3.rs:123`) and
owner checks in terminate/setActive (`:253–254`, `:321–322`). Normal module
destruction unmapped the code before the native test thread exits; the later
pthread callback jumps into unmapped code. Rust's exact-version official
[thread-current source](https://github.com/rust-lang/rust/blob/b940084d7eb6a299eb4bfeb8e34901bc051e7ac4/library/std/src/thread/current.rs)
enables the cleanup guard when initializing its current-thread handle. The
[guard source](https://github.com/rust-lang/rust/blob/b940084d7eb6a299eb4bfeb8e34901bc051e7ac4/library/std/src/sys/thread_local/guard/key.rs)
registers a destructor-bearing TLS key and schedules its callback at thread
exit. This source contract, exact fault symbol, observed unmapping and minimal
differential establish the local mechanism.

This is strong evidence for the reported Ubuntu failure because the compiler,
host and fixture source match and the serial failure pattern matches. The
original remote job has no core/PC trace, so its exact fault instruction is
not independently proven. A native Ubuntu rerun without the retention shim
remains required before declaring that job repaired.

### Hypotheses resolved and unresolved

The initial ranked probes considered an invalid ABI pointer, premature code
unload, and stack exhaustion. The mapped/unmapped PC and symbolized callback
select the fixture TLS-after-unload mechanism. Parallel module entry/cache
racing is not required for the minimized crash. No UUID/vtable reinterpretation
or processor fix is justified by this evidence.

The Weak module cache remains a separate lifecycle audit: a new load can see a
dead Weak before the previous ModuleData destructor finishes. Holding a mutex
only while loading, or taking that mutex only in Drop, does not by itself prove
that retirement and a new entry cannot overlap. This stage did not reproduce
that possible race and proposes no race repair as the explanation for CI.

## Exact proposed implementation grants

### A. Narrow Linux fixture identity window, coordinated with N4

Request an exact N4-approved window in `test-plugins/src/vst3.rs` for the creator
field, its constructor assignment, the two owner comparisons, and a private
helper declaration; optionally put the helper in a new
`test-plugins/src/vst3/native_thread.rs`. Preserve both original assertion
messages and their rejection of a different live owner thread. Preserve every
class/parameter/event/state/bridge behavior and all original R4/readback/drop/
allocator assertions.

Proposed private API:

```rust
struct CreatorThread(/* platform-native identity on Unix */);
impl CreatorThread {
    fn current() -> Self;
    fn is_current(&self) -> bool;
}
```

On Linux x86_64/glibc, bind the actual `pthread_t` representation (`c_ulong`)
and use `pthread_equal` for comparison. The private minimal probe validates
that target only. Audit platform libc headers before declaring another Linux
ABI supported. On macOS, use its native pthread representation/equality or
`pthread_threadid_np` with checked return status, not a guessed Linux typedef.
Retain existing Windows identity behavior in this narrow Linux stage. Add a
wrong-live-thread negative check and load/unload-before-worker-exit regression
so a constant identity or removal of the assertions cannot pass. Never use
permanent host library retention, cfg skipping, or larger stacks as the fix.

No production host/processor or Cargo change is needed for this specific Linux
fixture mechanism. Applying even these tiny shared-fixture edits still needs
the parent's exact grant and N4 coordination.

### B. macOS native module, executable identity, and bundle fixture

Request loader-only portions of `src/vst3/mod.rs`, `src/paths.rs`, new private
`src/vst3/module_macos.rs`, new `src/paths/macos.rs`, and narrowly scoped
`tests/common/mod.rs` fixture publication. Request new module-entry tests in
`tests/vst3_module.rs`. Shared fixture Mac entry exports require a separate N4
window; a new `test-plugins/src/vst3/entry.rs` can own entry-only code with an
approved declaration in the existing fixture file. `test-plugins/src/lib.rs`
and `src/vst3/processor.rs` remain outside this grant.

Proposed host internals, with no public host API change:

```rust
enum NativeModule { Dynamic(libloading::Library), MacBundle(MacBundle) }
struct MacBundle { /* owned, non-null CFBundleRef; attempted-entry state */ }
// Private operations: open bundle, resolve typed factory/entry/exit,
// attempt entry, retain executable owner, pair exit, release owner.
// paths::vst3_binary(path: &Path) -> Option<PathBuf> keeps its signature.
```

Use scoped `#[link(name = "CoreFoundation", kind = "framework")]` C FFI in
the new macOS helpers; no new Cargo dependency is currently proposed. Opaque
CF references and CF Boolean representations need explicit ABI declarations
checked against Apple headers/native compilation. Resolve a file-system URL
from path bytes; manage each Create/Copy reference and CFError on every return.
Resolve metadata through CFBundleCopyExecutableURL without loading executable
code for identity. The loader must use that same executable and a real bundle
reference, verify the mandatory export set before entry, and only publish the
module after receiving a non-null owned factory. Keep class ID conversion and
the pinned VST3 interface bindings untouched.

Windows and Linux raw-file and existing bundle resolution must retain current
compatibility. Linux continues to receive the actual owned dlopen handle;
conversion into/from libloading's raw handle must occur once. Their loader
policy must not silently become stricter just because macOS needs mandatory
bundle exports.

For macOS only, `plugin_file(folder, "*.vst3")` should publish a real bundle:

```text
fixture.vst3/
  Contents/Info.plist
  Contents/MacOS/windfall-test
```

Info.plist declares `CFBundleExecutable=windfall-test`, `CFBundlePackageType=BNDL`,
a test bundle identifier unique per scratch bundle, and a version. Publish
all pieces under the existing COPY mutex before another test can load them.
Copy the existing cdylib; prove natively that CFBundle loads that Mach-O output
and reject this approach if it cannot. Keep CLAP copies and Windows/Linux raw
VST copies unchanged. Add `extern "C"` Mac entry/exit exports that verify a
real, non-null CFBundle argument and balance their own entry ledger; do not
pretend the Linux handle or a null pointer is a CFBundle.

No changes to `realtime.rs` assertions are proposed. Enabling `vst3_host.rs` /
`vst3_scan.rs` on macOS is a later exact grant, after their bundle-catalog and
platform GUI setup is adapted. Windows HWND editor tests remain about Windows.

## Failure, exit, and unload review before production expansion

- Opening/resolving metadata fails: release all created/copied CF values and
  errors; do not call entry, exit, or factory.
- A mandatory Mac export is missing: fail before entry; release executable
  ownership. Resolve entry and exit as a pair, so cleanup is callable if entry
  is attempted.
- Entry returns false: SDK counters/retains are already possible. Pair an
  attempted SDK entry with exactly one exit before releasing ownership, even
  though load is refused. Do not call the factory or cache the failed module.
- Entry succeeds, factory is missing/null or later capture/create fails:
  unwind initialized objects; release any owned COM references first; call
  the matching exit once; then release executable/bundle ownership. No cache
  publication before a complete owned factory exists.
- Normal unload: instance/controller/processor/connection references remain
  within existing module ownership; factory release precedes module exit;
  exit runs while its code is callable. Rust fixture thread-exit callbacks
  must not survive their code mapping. CFRelease is not forced code unload.
- Exit returns false: observe it in negative diagnostics; never retry exit or
  use the module afterwards. Define caller-visible reporting where an error
  can be returned; destructor cleanup cannot invent a successful recovery.
- Existing compatibility permits absent Windows/Linux entries. Current Linux
  code records ModuleExit even when ModuleEntry is absent; current Windows/
  Linux refusal returns before arming cleanup. SDK Linux and Windows counters
  advance before returning false. These are separate negative-path audits,
  not proven causes of the fixture crash. Any policy/behavior repair needs a
  distinct narrow grant and explicit legacy-entry compatibility tests.
- Preserve the weak cache's non-retention property. If retirement ordering is
  changed, use an explicit per-binary lifecycle state (loading/live/retiring/
  empty) whose waiters cannot enter until factory/exit/executable cleanup
  completes. In particular, notify only after dropping executable ownership,
  not before Rust's implicit field drop. Never involve the audio path in that
  mutex/wait. Review poison, refusal and reentrancy behavior separately.

Independent entry-negative fixtures should expose actual platform entry
signatures and configurable refusal/missing-export/null-factory cases, with
an external event ledger readable after unload. A minimal C fixture can record
events without registering Rust TLS. Do not keep an extra dlopen handle merely
to inspect counters: that would hide the exact unload being tested. Positive
factory lifetime uses the existing SDK fixture with the owner-identity repair;
the negative fixtures must not duplicate or weaken its processing assertions.

Primary [Linux SDK host](https://github.com/steinbergmedia/vst3_public_sdk/blob/586dc5e6c8012c3e4b01c79389375cbe96bdb1da/source/vst/hosting/module_linux.cpp)
passes its dlopen handle, treats Linux entry/exit as required, and releases the
factory before exit/dlclose. The [Linux entry source](https://github.com/steinbergmedia/vst3_public_sdk/blob/586dc5e6c8012c3e4b01c79389375cbe96bdb1da/source/main/linuxmain.cpp)
increments its counter before InitModule returns. The
[Windows entry source](https://github.com/steinbergmedia/vst3_public_sdk/blob/586dc5e6c8012c3e4b01c79389375cbe96bdb1da/source/main/dllmain.cpp)
has the analogous counter behavior; the
[Windows host](https://github.com/steinbergmedia/vst3_public_sdk/blob/586dc5e6c8012c3e4b01c79389375cbe96bdb1da/source/vst/hosting/module_win32.cpp)
allows its entry exports to be optional. These SDK policies are evidence for
the audit, not authorization to change Windfall's legacy compatibility.

## Available native tools and bounded validation plan

Windows has Rust 1.99.0, installed Windows-MSVC/Linux-GNU/Wasm Rust targets,
Python 3.12, CMake and Docker Desktop. VS 2022 BuildTools with the VC x86/x64
component is installed; `cl`, GCC, Clang, GDB and LLDB were not on the shell
PATH. No standalone cross-GCC was found. WSL lists only `docker-desktop`, whose
shell has no inventoried development tools. The already-installed Docker Rust
image provides Linux GCC, Rust, addr2line/objdump/readelf/nm and timeout; no GDB
or LLDB was found there. No native macOS/Xcode/CFBundle environment is available.
An installed Rust cross target alone supplies no native execution evidence.

After exact grants, validate in separate stages:

1. **Linux fixture window:** reproduce the original single red test, implement
   the owner-identity change, then run the same executable without LD_PRELOAD
   retention. Require the unchanged full realtime suite serial and default
   parallel to pass, followed by at most 10 parallel repeats (30 seconds each).
   Run the wrong-owner and unload-before-thread-exit regressions. Run host
   `vst3_host`/`vst3_scan` and scoped Clippy. Stop on first failure and capture
   a new trace; do not repeat broad workspace runs as a substitute for diagnosis.
2. **Windows regression:** in the bound worktree, run unchanged realtime 20,
   existing VST host/scan tests, and the new entry-negative tests with MSVC,
   then scoped Clippy. This catches any accidental change to raw loading,
   UUID order, ownership refusal, or allocator assertions. No fresh Windows
   test success is claimed by this document.
3. **macOS native gate:** parent-arranged native machine/runner, exact repaired
   commit, SDK-compatible bundle. First validate bundle resolution including
   renamed/different-executable names, actual bundle argument and entry/exit
   ledger. Run missing-entry/missing-exit/missing-factory/refusal/null-factory
   cases. Then require all original 20 realtime cases, including the eight
   formerly failing VST cases, without skips or assertion changes; repeat
   default parallel at most 10 times with per-run timeout. Run the cfg-expanded
   VST host/scan suites only after their separately granted adaptation. Record
   OS, CPU, compiler, commit, executable hashes, results and cleanup evidence.
4. **Actual Ubuntu confirmation:** parent reruns the native Ubuntu test command
   at the repaired commit without diagnostic retention. Require all 20 tests
   plus the targeted unload regression to pass. If it still faults, obtain
   its PC/maps/core; the local Debian mechanism must not be used to declare a
   different remote fault fixed.
5. **Parent integration:** only after native evidence and review, accept a
   source-only incremental commit for the exact granted change; do not amend,
   reset, push/dispatch CI or import another owner's branch from this worker.
   Wider workspace/release/parity closure belongs to the full parent goal.

## Review handoff artifacts

The only tracked change from this stage is this document. Private artifacts
are deliberately ignored under `target/diagnostics/vst3-portability`:

- `snapshot-provenance.json`, `provenance-check.json`, source snapshot and
  per-run JSON command/exit records.
- Original red logs: `parallel-1.log`, `single-direct-1.log`,
  `single-direct-2.log`, `serial-direct-1.log`.
- `trace-1.log`, `trace-2.log`, `trace_fault.c`, `observe_dlclose.c`,
  `symbolize_trace.py`; these are tagged private diagnostic tools.
- `minimal_identity.rs`, `minimal_identity_host.c`, `minimal-verdict.json`,
  `minimal-rust-1.log`, `minimal-native-1.log`.
- `retain-single-1.log`, `retain-all-1.log`, `retain-all-1.json`: diagnostic
  differentials, explicitly not repair validation.
- SDK source snapshots, pinned commit and hashes under `sdk/`.

An early Cargo `single` run and `serial` run were launched while the first was
still finishing; they share scratch paths. They are retained as exploratory
artifacts but excluded from the final isolated evidence above. Subsequent
direct runs and probes were sequential. The initial `parallel.log` is only
the expected private `--locked` refusal, not a native crash reproduction.

Exact diagnostic executable SHA-256:
`a6c74ef8f45956f9e7dfc2405f718049ad783a799f998cc5ef73340e03278843`.
Fixture SHA-256:
`2fb2c064868839df9450313aecba3450be2c5ba4a25d51dbc48cc75b1a2f0970`.

The diagnosis is reviewable now. Implementing A requires the narrow N4 fixture
identity grant; implementing B requires the listed loader/path/helper/entry
windows and actual native macOS validation access. Nothing in this evidence
authorizes processor, bridge, Cargo, CI, or unrelated preparation/analyzer edits.

## Hosting stream source follow-up (2026-10-11)

The hosting worker preserves the existing native creator-thread identity and
module/bundle lifecycle code. It appends a pointer-free numeric expression
fixture class (23), updates fixture count assertions to 24, and adds native/helper
PCM tests. See [the hand-off](qa/2026-10-11-plugin-hosting.md). No Linux or Mac
build/test ran in this stream; earlier diagnostic evidence is not new acceptance
of the appended fixture or the macOS AU adapter.
