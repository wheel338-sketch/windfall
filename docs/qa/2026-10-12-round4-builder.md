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

## Resume / final gates

A2 is next. A3–A7, release budgets and final gates not yet run.
