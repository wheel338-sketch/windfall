# E1 CPU acceptance protocol — measurements pending

No measurement was run by the edit-only implementer. This document declares
the budget before measuring it. It is not evidence of a pass.

The registered, non-ignored
`e1_active_and_extreme_cpu_measurements_meet_the_declared_target_budget`
in `crates/windfall-dsp/tests/dsp/e1_cpu.rs` covers Seven Band, Spreader,
Stereo Enhancer, Pan Motion, Morph EQ, Filter Bank, Selectable filter and
Band Split. Active settings keep all bands/filters working. Extreme settings
exercise range ends, all eight drawn filter patterns, and all graphic bands.
Both static processing and parameter edits once per block are measured at
48/96 kHz with 64/256-frame blocks. Allocation, finite-output, audible-workload
and 1/257-frame invariance assertions are in the separate deterministic test
`e1_cpu_workloads_are_active_finite_partition_invariant_and_allocate_nothing`.

Release budgets, per processor and per logical core:

- Seven Band: at most 10% of the processed audio duration.
- Each other E1 effect: at most 35%.

Each measurement is the median of five quarter-second audio trials after
warm-up. Input synthesis, preparation and printing are outside the timer;
buffer copies, processing and (in edit cases) full parameter writes are inside.
The test prints machine OS/architecture, profile, kind, setting group, edit
mode, rate, block size, measured CPU percentage, budget and nanoseconds/frame.
The wider debug budget (40 times release) is a development regression check;
debug output cannot satisfy the row's target-machine CPU acceptance gate.
These are individual-processor budgets, not a claim about an arbitrarily large
full mixer or the worst operating-system scheduling stall.

Integrator: use the roadmap's Git Bash/MSVC setup and a temporary
`TS_RS_EXPORT_DIR`, stop other builds, and execute serially:

```bash
cargo test -p windfall-dsp --release --test dsp e1_cpu -- --test-threads=1 --nocapture
```

Record the target CPU model, OS build, power mode, compiler version, commit,
background-load conditions and every `E1_CPU` line alongside the exit status.
Retain failures as evidence; investigate the workload and competing CPU load
before changing budgets. The older ignored Seven Band comparison is retained
unchanged and is supplementary, not the only timing gate.

SOFTWARE: target-machine release measurements and their recorded results are
still required. The implementer was explicitly prohibited from running them.
