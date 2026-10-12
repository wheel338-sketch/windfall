# Native analysis foundation (M1 first stage)

`windfall-analysis` is a native control/worker crate on the accepted A-EDITOR
source contract at `1e24389eb58f4d59f3c0a121d946e1372ef7e585`. It does not edit a
project, connect to the engine, expose IPC, or ship an inference adapter. Its
authored test adapter copies selected PCM on real CPU workers and writes real
files. It proves lifecycle only. M1 Session/IPC/UI integration and M2–M4 actual
separation, denoising, pitch detection/editing and audio-to-MIDI remain required.
No parity row is closed by this delivery.

## Input and exact source checks

`CapturedInput::capture` runs away from recording/State/audio locks. It validates
shape, nonempty inclusive-start/exclusive-end range, checked frame/sample/byte
arithmetic and every finite sample. It retains a compact owned copy of the
whole immutable input; the selection is a checked slice of that copy. Copying
avoids retaining an external `Arc<Vec<f32>>` with hidden excess capacity. The
observed allocated capacity, rather than just selected frames, is charged.
No buffer is cloned into a status response. Ready review retains the original
captured PCM until cancellation or completed control-side retirement.

The Session supplies a canonical `CaptureStamp`:

- Document generation and edit revision, exact logical source key and binding
  digest, loaded original `AudioIdentity`, content fingerprint, prepared input
  frame/channel/rate shape, and exact selected range.
- `binding` must cover the accepted exact logical source and clip/render
  settings, including offset/reverse/stretch/pitch/gain/pan/fades/routing and
  timeline placement as appropriate. A digest of a pathname alone is inadequate.
  Canonical binding rules belong to the later Session adapter, which must
  preserve existing exact source/history guards.
- `ContentFingerprint::reader` hashes streamed actual file bytes with SHA-256
  and a byte cap. `ContentFingerprint::audio` hashes a versioned prefix, little
  endian sample rate/channel/frame fields and exact Float32 bits. File bytes
  and decoded PCM use different fingerprint kinds. Sizes/mtimes are never
  treated as content hashes. Paths do not enter the content fingerprint; path
  aliases to the same bytes compare equal, same-size changed bytes do not.
- A supplied verified fingerprint is a caller contract. For file input, the
  later loader must fingerprint the **exact file version decoded** into its
  loaded source and immutable view; independently hashing a mutable path after
  decoding does not establish that identity. Capture/recheck IO occurs off
  State. The final Session check must still compare the current loaded exact
  source allocation and canonical binding under its guards.

`Review::check_eligibility` compares all stamp fields and the entire model
manifest, including revision, checksum, provenance and shape, and requires a
live current source identity. It performs no IO and proves no atomic document
installation. Stale/refused work leaves the immutable review available until
explicit cancellation, consumption or shutdown.

## Execution and resource accounting

`JobManager` owns fixed real worker threads, a condition-variable queue and
bounded retained records. `submit`, `snapshot`, `wait`, `wait_for_change`,
`review`, `cancel`, `forget`, `claim`, `usage`, `retry_cleanup`, `shutdown` are
control-side operations. They must never run on audio or under Session State
or controller locks. The adapter has no mutable document or engine pointer.

Default limits: two workers, four queued jobs, sixteen total retained records,
256 MiB admitted memory, 1 GiB staging/publication/retained disk, 256 retained
output/temporary files, 64 MiB adapter scratch per job, 4,000,000,000 work units per job,
ten-minute execution/apply timeout. Input defaults cap whole captures at
64 MiB, 23,040,000 frames, mono/stereo and 8,000–192,000 Hz. Model cache defaults
cap a model at 256 MiB, cache at 1 GiB, 32 directory entries and one import.
Callers may configure tighter bounds. No CUDA/Python installation is required.

Each admission reserves:

- Actual compact input capacity, manifest's exact model bytes, **every retained
  manifest String/Vec capacity** (`ModelManifest::retained_bytes`), and two
  compact manifest copies for peak worker model/writer preparation. This includes
  unused provenance/device/role string capacity and unused output-vector slots;
  validating short lengths does not excuse retained large allocations.
- Declared adapter scratch, one largest decoded output PCM buffer, a 2 MiB
  codec/IO allowance, and 128 KiB for retained records, worst-case native
  staged/published paths, prebuilt commit metadata and scheduler storage.
- Three times the output byte budget for private staging, persistent publication
  and an owned temporary whose deletion may be refused, plus the declared
  aligned role count **and one sequential publication temporary slot**.
  Already published bytes/files
  stay charged even after job removal. Existing files in the dedicated output
  directory are included when the manager starts.
- Every queued/running/ready/apply-claimed/retiring input, scratch or output
  reservation. Consumption cannot release reservations before the deferred
  input/staging retirement actually finishes. Completed status records keep
  their fixed memory charge until explicit `forget`.

Adapters are trusted native implementations, configured once per manager;
requests cannot enqueue arbitrary adapter objects holding unaccounted audio.
They must use bounded provided scratch and checkpoints, declare any runtime
resident memory in scratch and add adapter-specific measurements. This is
cooperative admission accounting, not an OS memory sandbox. Existing codec
internals have their own guarded metadata/channel/rate limits and bounded PCM
sink; the allowance is not proof of an arbitrary codec/model runtime's maximum
RSS. A future adapter needs its own allocation/peak-memory audit and CPU deadline
measurements. Blocking filesystem calls and a codec decode cannot be preempted
mid-call; cancellation/deadline is checked before/after them and between bounded
sample/byte chunks. No hard realtime latency is claimed.

Statuses are `Queued → Running → Ready → Consumed`, with cancellation through
`Cancelling → Cancelled` and failures to `Failed`. Queued and ready work can be
cancelled without running an adapter. Running cancellation is cooperative and
cannot publish Ready after its cancellation wins the control lock. Consumed
cancellation is a no-op. A claimed apply refuses cancellation/removal, preventing
file retirement while a caller commits. Failure/cancellation releases buffers
and staging off the manager lock before releasing their reservations.

Job, review ticket and request IDs increase without reuse, including after
forgetting or replacing a manager/Session in this process. The process-wide
allocator is not reset by shutdown. Tickets are volatile and must never be
saved/reused across application process launches. The later IPC must encode
u64 IDs as decimal strings to avoid JavaScript integer precision loss.
ID allocation refuses before integer rollover; checked arithmetic
refuses oversized shape/budget/deadline calculations. Status/progress sequence
numbers increase, work never regresses/exceeds its budget, progress is coalesced
instead of appended to an unbounded event list, and transition history has at
most eight entries. Each apply gets a new request ID. Work exhaustion can refuse
an apply retry; it cannot create an unlimited stream of retained outputs.

Shutdown serializes joining, stops dequeuing once shutdown starts, cancels
queued/ready jobs and asks active workers to stop, then joins off all Session
locks. Outstanding apply/retirement owners keep the shared accounting alive.
An apply dropped during shutdown retires staging; an acknowledged real commit
retains its sources. `forget` accepts only failed/cancelled/consumed records with
finished retirement. Ready work must be explicitly cancelled or consumed.

## Explicit local models

`ModelManifest` fixes ID, version, revision, exact SHA-256 and byte length/limit,
source revision/origin/author, weight SPDX/license reference, adapter ID/version,
CPU device, exact input channels/rate/frame cap and one to sixteen unique output
roles with channel counts. Provenance strings and roles are bounded. Weight
terms are separate from runtime/code terms. Nothing searches for guessed
weights, downloads or uploads. Missing models return an explicit import/offline
error. There is no production adapter disguised as successful inference.

`ModelCache::import_reader` / `import_file` reserve bounded import count/cache
disk, stream into an owned `NamedTempFile`, validate exact size and SHA-256,
sync and publish with `persist_noclobber`. Cancellation before publication
removes only staging. Publication is the import commit point; cancellation
after it cannot remove cache data. Existing and concurrently won entries are
read and rehashed rather than trusted by name/metadata. Invalid existing entries
are refused and preserved; users must explicitly repair their own cache.
`load` rehashes a bounded compact immutable model-byte copy on the worker before
the adapter runs, avoiding inference against a changing cache pathname.

Cache and output roots are dedicated single-control-authority directories;
cache clones share reservations. Symlinks/non-files in cache/output inventory
are refused. Independent authorities/external arbitrary writers are not an
interprocess quota system or a hostile filesystem security boundary. Collisions
preserve competitor bytes. The three ownership roots must be disjoint, with
fresh exclusively owned staging. Crashed staging is refused on next startup,
never speculatively garbage-collected.

## Artifacts and publication/commit protocol

`ArtifactWriter` creates a unique private job directory. Portable flat relative
names reject separators/drives/ADS/traversal/dots/spaces/device names. Artifact
names and publication destinations reject ASCII case aliases before any writes,
including on a case-sensitive host. Relative names compact unused String capacity.
Outputs must be Float32 WAV, one per declared role, exactly aligned selected frame count
and original frame origin, at the manifest rate and role channel count. Chunk
writes cap at 4096 whole-frame finite samples. Disk/count/byte/frame overflows,
nonfinite data, missing/extra/duplicate roles and bad paths are refused. Encoded
helper output first streams into bounded owned staging, then is probed for
exact aligned Float32 WAV before creating a decoder and normalizing to this
contract. Compressed helper codecs need a separate memory audit and are refused
by this first foundation. Completed WAVs are independently decoded for shape
and finiteness and hashed before review. Both decodes use the additive codec
`decode_file_strict_with` entry: nonfinite samples return `CodecError::Corrupt`
before ordinary sanitation. `decode_bytes_strict_with` uses the same supported
parser/layout/byte limits. Every ordinary decode entry keeps its previous
signature, defaults, damaged-file handling and NaN/Inf-to-zero behavior. No
format, model runtime or dependency was added. No output PCM is retained between files.
Codec replacing-writer APIs see only private staging, never persistent targets.

The Session integration must follow this protocol:

1. Capture a canonical source/view/stamp with recording exclusion before State;
   perform view preparation, fingerprinting/copying/admission off all those
   guards, then recheck exact source/document before treating the job as current.
2. Poll/await status; get a bounded review with capture/model/output metadata.
   Review itself cannot edit anything.
3. `claim(ticket, current_stamp, manifest)` checks pure eligibility and gives one
   `ApplyLease`. `prepare(relative_names)` copies verified staging into owned
   temporary output files, syncs and atomically publishes each with no replacement.
   Caller output names must be uniquely chosen. There is no multi-file atomic
   publication claim; a later collision can leave earlier final files retained.
4. Decode published outputs, prepare the ordinary pool/checked command batch and
   `Controller::prepare_project` off State. Recheck actual file content and
   loaded source state as required by the Session source contract.
5. Acquire recording exclusion **before State**, recheck capture/source/request,
   exact clip and every prepared pool source, then dispatch **one checked batch**
   using the existing prepared-plan publication/retirement path. A recording,
   project replacement, stale edit/model/source or rejected batch changes no
   document/history/pool/engine state. Pure crate tests do not prove this step.
   `PreparedApply::check_eligibility` compares borrowed canonical fields directly;
   do not call the allocating `review()` here. `prepare()` has already built
   bounded output metadata, exposed by `artifacts()`, before entering these guards.
6. Only after the actual caller commit, call
   `PreparedApply::acknowledge_commit`. This infallible transition does no IO,
   codec work, allocation, deallocation or worker joining. It moves prebuilt
   metadata and defers even the progress closure's destruction. It returns
   `AppliedAssets` with deferred
   retirement. Release all Session/recording guards, then `retire`/drop the assets.
   Their final files remain external sources for undo/redo/save/archive.

Dropping a lease/prepared apply after failed commit returns Ready, retaining
input/staging and any already published metadata. The first attempted destination
set is pinned; retries reuse the same validated final files rather than generating
more orphans. Cancellation after Ready removes only staging, even after partial
publication. Published files are never deleted on failure/stale/refusal/cancel,
job forgetting, controller retirement, shutdown or undo. Final bytes/files are
permanently charged for the manager lifetime and re-inventoried on restart; this
finite budget can require another directory/explicit user-owned recovery. There
is no speculative source GC or claim that a failed publication/commit is free.

Every publication temporary is registered on its job immediately after exclusive
creation, before fallible copying/checksum/sync/rename. At most one exists per
job. A failed destructor deletion is followed by explicit owned-path cleanup;
a refusal keeps that path and its disk/file reservations charged. Apply retries
must remove it before writing another temporary. Cancellation/consumption retain
the record and reservations until both private staging and publication-temporary
cleanup succeed; repeated cancel/forget/admit cannot bypass the quotas.

`retry_cleanup` is explicit and bounded to that job's private child directory
and exact registered publication-temporary path. Replacement files/symlinks at
the private directory, and directories/symlinks at the temporary path, are
refused for manual recovery. It never takes a final path, cache path or another
job's path. No cleanup unlinks committed or competitor destinations. Ordinary
Drop is only the first attempt; retirement checks both tracked paths. A crash
leftover in the publication root is counted by the next startup inventory and
is never guessed to be garbage.

## Round-four classical aggregate quotas and file-backed dereverb

Classical editor preparation now has a 256-MiB aggregate worker quota.
`classical_memory` preflights detection arrays, transient/note/count/candidate
growth, FFT construction, denoise f64 overlap-add and frame-weight arrays,
returned f32 PCM, contextual interleaved/planar stretch buffers, full render
output and seam/runtime storage. `Work::with_memory_limit` includes caller-held
audio and recipe capacities; `peak_memory_bytes` exposes the accounted high
water mark. The Session counts distinct prepared audio identities and retained
recipe capacities, incoming edits, and every still-live audition identity.
Quota failure is an ordinary observable resource error before DSP allocation;
previous proposals/document/history remain available. Shared input clones and
neutral recipes do not allocate another complete PCM buffer. This is an audited
cooperative bound, rather than an OS memory sandbox or a process-wide RSS cap.

Controller audition accounting uses a fixed 256-slot weak-identity table on the
control side. Queued, playing and control-retirement owners remain charged
until their audio allocation actually dies; repeated aliases count once. The
table never pins PCM and requires no callback changes. Exhausting this table
fails subsequent analysis admission closed. Other still-live owners can cause
conservative overcharging, which cannot bypass the quota.

Built-in dereverb uses an anonymous file-backed Float32 selection, retaining at
most 64 KiB of input/IO allowance rather than the full source/view. The full
rendered producer is checked in whole-frame chunks, but only selection bytes
occupy the capture file. Source identity, actual file fingerprint, frozen clip
timing/binding, native file authority and final stale checks remain required.
Loaded project audio is borrowed during capture; this path does not add another
whole decoded/source buffer. Strict source verification and staged-output
verification decode bounded blocks. A short DrumSynth recipe keeps its previous
bounded synthesis path; it is not treated as a packetized audio file.

Tape/reverse/pitch/offset/normalize/pan/fades stream with the existing view law.
Spectral preparation uses the same exact offline clock/latency with bounded
planar input and an anonymous intermediate file; no full planar or output copy
is retained. Workers consume selection blocks through `for_each_selected`.
The existing review lease, no-clobber file publication and one-step guarded
document apply/undo/save/reopen protocol are unchanged.

The native Session manager now admits at most 512 MiB of job memory and 1 GiB
of capture/staging/publication/retained disk. A built-in job reserves 64 MiB of
DSP scratch, 64 KiB of input storage, three selected-result PCM allocations
(decoding/reallocation and audition coexistence), model/record/codec metadata,
and caller-held auditions. Capture scratch and temporary disk are preflighted
alongside retained jobs. Captured selection disk bytes and its open file slot
stay reserved through cancellation/consumption retirement. Every preview/apply
also rechecks still-live controller auditions. Final project PCM belongs to the
ordinary prepared project pool; it is not claimed to stay file-backed at playback.

Explicit bounds: mono/stereo at 8–192 kHz, one-hour rendered frame-count cap,
128 MiB of selected PCM/result and optional spectral intermediate, 1 GiB of
encoded source, existing 500,000,000-unit/120-second capture/apply work bounds,
and 100,000,000,000-unit/600-second dereverb execution bounds. Longer clips can
use a bounded selection without capturing the whole clip to disk. Bounds remain
observable refusals, never truncation or successful dry substitution. Imported
model input/output limits stay on their previous path; no separation runtime,
rate converter, weight pack or dependency is introduced here.

New allocator/lifecycle regressions are listed in the round-four analysis
hand-off. They are written, not executed by the edit-only worker.

## First-stage verification and next exclusive window (historical)

Tests live in auto-discovered `tests/analysis_jobs.rs` and library unit tests.
The authored CPU fixture contract is in `tests/fixtures/README.md`; model SHA-256
is `07f6812eb7ac451adc4b010bb18b2fe098ce235498880c80ead3fad519e47d33`.
The exact 74-byte Float32 WAV SHA-256 is
`4dbb9f64ee9794914f7a9bf01ed95c3775858a4bb9413877a99218b88077528b`.
Barrier seams exercise native queue/active cancellation and shutdown without
timing sleeps. Tests inspect actual decoded files, retained bytes and directories.
They cover whole-input capacity/bounds/NaN/Inf/ranges, queue/count/concurrency,
memory/disk/work/integer overflow, monotonic transitions, absent/corrupt models,
stream read/checksum/size/cancellation errors, output codecs/IO and alignment,
ready/claimed/consumed cancellation, stale stamp/model revisions, no-clobber
collisions, refusal/retry/double consume and staged versus persistent lifetime.
No hardware, packaged app, Session undo/save/archive, browser UI or real inference
verification is claimed by these crate tests.

R1 at immutable `89d5ffbd3cb426d62d36786636a1e0a6e032b8b8` identified five
foundation defects. Compiled RED regressions reproduced a locked publication
temporary releasing its reservation, undercharged high-capacity manifests,
20 allocations/18 frees in the final seam, sanitized encoded nonfinite PCM
reaching Ready, and Windows case aliases reaching Ready/partial publication.
All five repairs have compiled GREEN native regressions. Extended checks cover:

- An actual Windows no-delete file handle blocks two Ready apply retries and
  four cancel/forget/admit/cleanup cycles. A 12,288-byte/two-file reservation
  stays charged alongside the pre-inventoried 10-byte competitor; closing the
  handle and explicitly retrying removes only the owned temporary, releases
  reservations and leaves competitor bytes unchanged.
- An 8 MiB String plus 4 MiB output-vector capacity retains 12,583,031 manifest
  bytes, charged within 14,815,904 admitted bytes while Running and Ready.
  A valid queued manifest adds 4,194,486 retained bytes (21,243,263 total admitted);
  a separate 128 MiB-capacity valid String is refused by the 24 MiB manager.
  Actual Rust heap above the test baseline was 12,603,238 bytes at the worker
  barrier and 12,606,051 after Ready, confirming the charged capacity remained
  retained rather than merely predicting admission from source.
- Thread-local allocator/free guards measure **zero allocations and zero frees**
  across borrowed valid/stale eligibility plus acknowledgement, for both one
  and sixteen prebuilt output metadata records. Retirement happens afterwards.
- Authored native Float32/Float64 WAVs containing NaN/+Inf/-Inf are refused by
  both strict file and byte decoding. Ordinary decoding still sanitizes all six
  cases to zero. Finite PCM and the same decoded-byte limits remain compatible;
  a real analysis helper worker refuses encoded Float32 nonfinite outputs.
- Native artifact case aliases preserve the first WAV's PCM/bytes/stored SHA;
  destination case aliases refuse before publishing any file. Existing partial
  publication, competitor, failed caller commit, stale and history-owner tests
  continue to preserve finals without speculative deletion.

The one-second authored 48 kHz stereo lifecycle emits exactly 384,058 bytes,
SHA-256 `1704336f534fe5e5105dc93e12b068a27ce34bc432c54955794ab16134fe886f`.
One measured run took 13,669 microseconds, admitted 3,001,010 memory bytes and
measured 939,174 Rust heap bytes above its baseline (which excludes the original
caller source). It reserved 3,145,728 disk bytes; after publication 2,761,670
remained reserved and 384,058 were persistent; retirement released the remainder.
This is a lifecycle sample, not a performance guarantee, RSS measurement,
real-time result or inference/ML-quality evidence.

Run one Cargo at a time in Git Bash after `source scripts/msvc-env.sh`, with
`CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, `CARGO_TARGET_DIR=target/m1-native` and
task-local `TS_RS_EXPORT_DIR`. Cargo generated only the new windfall-analysis
lock entry, referencing already locked packages; subsequent checks use
`--locked --offline`. Verify only owned Rust formatting and strict crate
`--all-targets` Clippy. No full desktop/workspace/UI build or binding generation.

Proposed next child ownership, after acceptance and explicit parent grant:
- New `apps/desktop/src-tauri/src/session/analysis_jobs.rs` and
  `apps/desktop/src-tauri/src/session/tests/analysis_jobs.rs`,
  `crates/windfall-ipc/src/analysis.rs`, and
  `apps/desktop/src/features/analysis/*`, including source/history/save/archive
  and UI review tests.
- Serialized registration only: root `Cargo.toml` analysis dependency entry,
  `apps/desktop/src-tauri/Cargo.toml`, native `session/mod.rs` module/Inner
  construction/drop, `session/tests/mod.rs` test registration,
  `crates/windfall-ipc/src/lib.rs`, native `commands.rs` and `src/lib.rs` invoke
  registrations, `apps/desktop/src/lib/ipc/{backend,tauri,mock}.ts`, and
  `apps/desktop/src/features/playlist/audio/clip-inspector.tsx` action entry.

Final native seam: off-State `claim`/`prepare`/borrowed `artifacts` plus loaded
pool/command/controller preparation; recording-before-State borrowed
`PreparedApply::check_eligibility` and one real checked document batch;
`acknowledge_commit` only after that commit; `AppliedAssets::retire` after every
guard. Decimal-string IDs must remain process-volatile on IPC. Shared files
remain closed until parent acceptance and explicit exclusive window grant.
Existing editor/edit/library/
files/clip_processing, sampler/cache and Document exact-source/history internals
remain reserved. Parent owns generated TS/WASM, shared root documentation/parity
and final integration/publication. Mock must honestly report native inference
unavailable in browser mode. Native production now installs classical
dereverb; ONNX separation remains unavailable until its actual runtime/model
adapter is integrated. See the current hand-off below.

## Dependency license evidence

The only dependency graph addition is the native analysis package; sha2 0.10.9
and tempfile 3.27.0 already existed in Cargo.lock. Default features only (std;
tempfile randomness via existing getrandom). No runtime, FFI model binary,
network library or weights were added. Existing windfall-core/codec/serde/
thiserror dependency versions are reused without upgrades; preserve codec's existing Symphonia MPL-2.0,
Vorbis BSD-3-Clause and LAME LGPL obligations described in its manifest.

Local primary license files from the exact locked registry source were inspected
on 2026-10-08. The added references use the MIT option, compatible with this
GPL-3.0-or-later native app; MIT notices must accompany distributions. A table
of exact primary-file hashes and attribution follows. Platform-transitive
licenses are recorded even where this Windows build does not select them.

| Locked source | License option / primary file | Primary SHA-256 | Attribution |
| --- | --- | --- | --- |
| [sha2-0.10.9](https://github.com/RustCrypto/hashes) | MIT / LICENSE-MIT | b4eb00df6e2a4d22518fcaa6a2b4646f249b3a3c9814509b22bd2091f1392ff1 | Copyright (c) 2006-2009 Graydon Hoare; Copyright (c) 2009-2013 Mozilla Foundation; Copyright (c) 2016 Artyom Pavlov |
| [tempfile-3.27.0](https://github.com/Stebalien/tempfile) | MIT / LICENSE-MIT | 8b427f5bc501764575e52ba4f9d95673cf8f6d80a86d0d06599852e1a9a20a36 | Copyright (c) 2015 Steven Allen |
| [cfg-if-1.0.5](https://github.com/rust-lang/cfg-if) | MIT / LICENSE-MIT | 378f5840b258e2779c39418f3f2d7b2ba96f1c7917dd6be0713f88305dbda397 | Copyright (c) 2014 Alex Crichton |
| [cpufeatures-0.2.17](https://github.com/RustCrypto/utils) | MIT / LICENSE-MIT | ae9baa7beea910273c2f384c2a6b721fb7bd02bda3436074a1072e4ee689f985 | Copyright (c) 2020-2025 The RustCrypto Project Developers |
| [libc-0.2.190](https://github.com/rust-lang/libc) | MIT / LICENSE-MIT | 123a331b5dbf04c30097fa43b8f858bc85df671fe776de498d01f3d6b7c1f69e | Copyright (c) The Rust Project Developers |
| [digest-0.10.7](https://github.com/RustCrypto/traits) | MIT / LICENSE-MIT | 9e0dfd2dd4173a530e238cb6adb37aa78c34c6bc7444e0e10c1ab5d8881f63ba | Copyright (c) 2017 Artyom Pavlov |
| [block-buffer-0.10.4](https://github.com/RustCrypto/utils) | MIT / LICENSE-MIT | d5c22aa3118d240e877ad41c5d9fa232f9c77d757d4aac0c2f943afc0a95e0ef | Copyright (c) 2018-2019 The RustCrypto Project Developers |
| [crypto-common-0.1.7](https://github.com/RustCrypto/traits) | MIT / LICENSE-MIT | 3521672491a3479422d5fe1aca6645dd2984090f85da6e5205abfb18fb7a6897 | Copyright (c) 2021 RustCrypto Developers |
| [generic-array-0.14.7](https://github.com/fizyk20/generic-array.git) | MIT / LICENSE | c09aae9d3c77b531f56351a9947bc7446511d6b025b3255312d3e3442a9a7583 | Copyright (c) 2015 Bartłomiej Kamiński |
| [typenum-1.20.1](https://github.com/paholg/typenum) | MIT / LICENSE-MIT | a825bd853ab71619a4923d7b4311221427848070ff44d990da39b0b274c1683f | Copyright (c) 2014 Paho Lurie-Gregg |
| [version_check-0.9.5](https://github.com/SergioBenitez/version_check) | MIT / LICENSE-MIT | b7e650f3fce5c53249d1cdc608b54df156a97edd636cf9d23498d0cfe7aec63e | Copyright (c) 2017-2018 Sergio Benitez |
| [fastrand-2.5.0](https://github.com/smol-rs/fastrand) | MIT / LICENSE-MIT | 23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3 | No separate copyright line in this primary text; retain its notice. |
| [getrandom-0.4.3](https://github.com/rust-random/getrandom) | MIT / LICENSE-MIT | 523a42c25d245dde9c015f882cec7f4555aad883382a6cf19b4b7d9b2cd5419b | Copyright (c) 2018-2026 The rust-random Project Developers; Copyright (c) 2014 The Rust Project Developers |
| [r-efi-6.0.0](https://github.com/r-efi/r-efi) | MIT / AUTHORS | d027e91dbc9cdbb2f1190068e498bd6b61cff022b6a032b191021ba658d96111 | Copyright (C) 2017-2023 Red Hat, Inc.; Copyright (C) 2019-2023 Microsoft Corporation; Copyright (C) 2022-2023 David Rheinsberg |
| [once_cell-1.21.4](https://github.com/matklad/once_cell) | MIT / LICENSE-MIT | 23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3 | No separate copyright line in this primary text; retain its notice. |
| [rustix-1.1.5](https://github.com/bytecodealliance/rustix) | MIT / LICENSE-MIT | 23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3 | No separate copyright line in this primary text; retain its notice. |
| [bitflags-2.13.2](https://github.com/bitflags/bitflags) | MIT / LICENSE-MIT | 6485b8ed310d3f0340bf1ad1f47645069ce4069dcc6bb46c7d5c6faf41de1fdb | Copyright (c) 2014 The Rust Project Developers |
| [errno-0.3.14](https://github.com/lambda-fairy/rust-errno) | MIT / LICENSE-MIT | 8764a597675778ddfd4e25f81b08a05dbcf089ac05662df7613fe67f150e3aa2 | Copyright (c) 2014 Chris Wong |
| [linux-raw-sys-0.12.1](https://github.com/sunfishcode/linux-raw-sys) | MIT / LICENSE-MIT | 23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3 | No separate copyright line in this primary text; retain its notice. |
| [windows-sys-0.61.2](https://github.com/microsoft/windows-rs) | MIT / LICENSE-MIT | c2cfccb812fe482101a8f04597dfc5a9991a6b2748266c47ac91b6a5aae15383 | Copyright (c) Microsoft Corporation. |
| [windows-link-0.2.1](https://github.com/microsoft/windows-rs) | MIT / LICENSE-MIT | c2cfccb812fe482101a8f04597dfc5a9991a6b2748266c47ac91b6a5aae15383 | Copyright (c) Microsoft Corporation. |

Retain the attribution above and each exact package license in distribution notices.
rustix and linux-raw-sys also carry COPYRIGHT files retaining contributor copyrights.
r-efi uses the MIT section of AUTHORS; its UEFI backend is not selected on Windows.
The common MIT permission/warranty text is reproduced here from sha2/LICENSE-MIT:

```text
Copyright (c) 2006-2009 Graydon Hoare
Copyright (c) 2009-2013 Mozilla Foundation
Copyright (c) 2016 Artyom Pavlov

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```


## Classical dereverb and persistent ONNX contracts (2026-10-11)

The native default adapter runs real classical general/vocal dereverb without
weights. Built-in descriptors use an explicit reserved ID and zero bytes;
they cannot be imported as model packs. JobManager resolves them through
trusted native code; every imported model still follows checksum/cache loading.
Scratch reservation includes adapter-owned vectors without duplicating unused
generic scratch. Mode and wet/dry amount are pinned in the retained review.
Dry/wet A/B uses that same lease and immutable captured input. Native source/
view admission remains 16 MiB; bounded DSP streaming is not file-backed capture.

Imported manifests may declare an ONNX opset, named tensor shapes with explicit
fixed/symbolic dimensions, rate, segment/overlap frames and ordered stem roles.
Registration persists atomically in the sibling packs.json, with bounded reads
and rehashed cache content on restart. Jobs/tickets remain process-volatile.
Import never installs executable code or downloads weights. ONNX Runtime is
the chosen separation direction but no ort dependency/adapter ships here.
See [the hand-off](qa/2026-10-11-analysis-ml.md) for exact acceptance targets,
commands, licensing decisions and remaining gates. All new cases are written,
not executed by this worker.
