# Round 4 acceptance — 2026-10-12

## Running record

- Started at `3186193f4ac64553758f7d538ff991591da918fd`, clean `feat/parity-100`, main checkout only. Read round-three acceptance, round-four builder record, QA completion decisions, roadmap sections 3/9 and worker contract. No worktree edits/builds, delegation, new branch, push, PR or tag. All build/test commands run serially in Git Bash with MSVC environment, `CARGO_BUILD_JOBS=8`, isolated `TS_RS_EXPORT_DIR`, and Node/pnpm PATH.
- Branch 1 `gpt/r4-evidence` merged at `783269a9`; no conflicts or compile fixes. `cargo check --workspace --all-targets`: exit 0, zero errors/warnings, 30.39 s (`target/round4-branch01-check.log`). Media fixture capture/promotion remains required; no test result credited yet.
- Branch 2 `gpt/r4-codecs` merged at `8daceb43`; no conflicts or compile fixes. `cargo check --workspace --all-targets`: exit 0, zero errors/warnings, 34.71 s (`target/round4-branch02-check.log`). Windows 0.62.2 reused with Media Foundation/COM features (MIT OR Apache-2.0); no new package version.
- Branch 3 `gpt/r4-interchange` merged at `20438c80`; no conflicts or compile fixes. `cargo check --workspace --all-targets`: exit 0, zero errors/warnings, 23.88 s (`target/round4-branch03-check.log`). Its explicitly partial delivery leaves FLP translation, complete legacy preset synthesis/readers and editor WAV annotation propagation unfinished; these remain software items rather than acceptance claims.
- Branch 4 `gpt/r4-files` merged at `86dd4909`. Conflict in `session/files.rs` preserves owned-preset carrying followed by extraction reference journaling before file publication. `cargo check --workspace --all-targets`: exit 0, zero errors/warnings, 21.61 s (`target/round4-branch04-check.log`). No dependency additions. Remaining pan-law families/native capability settings are explicitly partial in its hand-off.
- Branch 5 `gpt/r4-analysis` merged at `ae19c0a7`; no conflicts/compile fixes, preserving builder detection fixes. `cargo check --workspace --all-targets`: exit 0, zero errors/warnings, 13.84 s (`target/round4-branch05-check.log`). Aggregate accounting/file-backed analysis still require focused execution.
- Branch 6 stems conflicts unify selected-only anonymous capture with a bounded random-access renderer and a reader lock held across cloned-handle use. Preserve both streaming editor APIs, codec APIs, aggregate retained preview accounting, cancellation errors and independent artifact verification. Shared manager uses stem capacity, while dereverb capture retains its per-job bounds. Initial offline check refused missing ort (`target/round4-branch06-check.log`); authorized online check downloaded only its six-package tree, then failed on ambiguous streamed PCM element type (`target/round4-branch06-check-online.log`), repaired to explicit f32. No existing package versions/checksums upgraded.

## Final gates

Pending. Inherited builder results are context only, not acceptance evidence for this integrated tree.

## Benchmark integrity

Pending audit of all maximum-load instrument/effect workloads against unchanged admission limits.

## macOS cross-check

Pending.

## Dependency and manifest changes

- Codec reuses `windows` 0.62.2 (MIT OR Apache-2.0) with Foundation, COM and Media Foundation features.
- `ort` =2.0.0-rc.10, `ort-sys` =2.0.0-rc.10, `smallvec` 2.0.0-alpha.10, `ndarray` 0.16.1, `matrixmultiply` 0.3.11 and `rawpointer` 0.2.1 added exclusively for authorized ORT tree. All six downloaded Cargo manifests declare MIT/Apache-2.0 alternatives; GPL-3.0 compatible. `default-features=false`, only `load-dynamic`; default download-binaries/copy-dylibs disabled. No runtime library or weights downloaded. `std` pulls ndarray in rc.10 despite ndarray feature omission; this accounts for its three transitive packages. Existing package identities/checksums unchanged; smallvec references qualified to disambiguate two versions.

## Row acceptance

Pending integration and actual evidence. Tracker files remain unchanged.
