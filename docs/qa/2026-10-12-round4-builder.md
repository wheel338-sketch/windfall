# Round-four builder — 2026-10-12

Base: `cfd2d37ee5af6ce8dc3f55acd539ba4929b0945b`, branch `feat/parity-100`.
Serial builds/tests only; Git Bash with `scripts/msvc-env.sh`, Cargo jobs 8,
temporary native `TS_RS_EXPORT_DIR`. No worktree changes or hardware tests.

- **A1 fixed:** adjacent 5 ms RMS cells mistook beating between held chord
  tones for attacks (27 false onsets), splitting otherwise accurate pitches.
  Compare novelty against the preceding 40 ms envelope peak. Existing tests
  unchanged. `cargo test -p windfall-analysis --test classical_tools --
  --test-threads=1`: **8 passed / 0 failed / 0 ignored**;
  `target/round4-builder-a1-fixed.log`. Baseline 0/1/0 and diagnostic probe:
  `target/round4-builder-a1-baseline.log`, `target/round4-builder-a1-probe.log`.
  Temporary instrumentation removed.

- **A2 fixed:** desktop's 256-frame helper block could be submitted and expire
  inside one 480-frame callback, forcing latency-aligned dry fallback even for
  healthy native PCM. Desktop bridge now uses 512-frame helper blocks (existing
  protocol capacity); two-block pipeline/PDC reports the resulting latency,
  with no callback waits/allocations. Existing assertions unchanged.
  `cargo test -p windfall-desktop --lib session::tests::mixer_preset_native --
  --test-threads=1 --nocapture`: **2 passed / 0 failed / 0 ignored** including
  capture, undo/redo, restore and reopen, all-frame PCM and allocator checks;
  `target/round4-builder-a2-block-probe.log`. Baseline **0/2/0**:
  `target/round4-builder-a2-baseline.log`.

## Resume / final gates

A3 is next. A3–A7, release budgets and final gates not yet run.
