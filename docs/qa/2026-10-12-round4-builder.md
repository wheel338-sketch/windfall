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

- **A3 fixed:** portamento allocated a second overlapping voice; continue the
  most recent compatible held occurrence and transfer logical ownership instead.
  Slide used its note length as the ramp length; use `glide_ticks` in the
  musical clock. `cargo test -p windfall-engine --test piano_phrase_pcm --
  --test-threads=1 --nocapture`: **1/0/0**;
  `target/round4-builder-a3-engine-fixed.log`. Native joined expression gate:
  `cargo test -p windfall-desktop --lib
  session::tests::plugins::piano_expression_native_helper_sequences_slide_portamento_and_midi_only_bends
  -- --test-threads=1 --nocapture`: **1/0/0**, all four CLAP/MIDI-only and
  Slide/Portamento cases; `target/round4-builder-a3-native-current-fixed.log`.
  Two fixture expectation corrections, with unchanged numerical assertions:
  the ramp fixture now enables a zero-time gate envelope, because the explicit
  existing `without_an_envelope_the_whole_sample_plays_whatever_the_note_length`
  contract makes the old gap expectation false; the native stop fixture drains
  its advertised 32-frame delay before checking sustained silence. A causal
  key-up at frame 0 leaves the previous 32 samples in that delay, so including
  them in a 4096-frame RMS can exceed 0.01 without any stuck note.
  Initial native probes accidentally selected the old 2026-10-10 DLL; rebuilt
  current fixture offline in `target/plugin-session-fixtures` and corrected
  runner. `target/round4-builder-fixture-current.log`. Earlier native failures
  remain logged as diagnostic evidence, not current-fixture product verdicts.
  Neighboring `cargo test -p windfall-engine --test engine expression --
  --test-threads=1`: **2/0/0**, `target/round4-builder-a3-expression-regression.log`.

- **A4 correctness fixed:** pitch-synchronous
  window period-count rounding jumped by whole periods as the tracker jittered,
  adding crossings; use a continuous complete two-period span. The ~6 kHz
  detector's high-frequency lag interpolation lost narrow harmony peaks;
  raise resolution to ~8 kHz while retaining the existing public/PDC delay.
  LPC order/window/update cadence and stability loading were fixed in sample
  units; scale their physical support with rate (bounded at order 128/window
  4096), preserving baseline behavior through 48 kHz. All test tolerances and
  assertions unchanged. Pitch units **3/0/0**:
  `target/round4-builder-a4-span-probe.log`; harmony and latency/heap checks
  **2 passed**, formants still failed before support fix:
  `target/round4-builder-a4-quality-rate-fixed.log`. Full nonaligned pitch and
  independent formants check across all six rates and both modes **1/0/0**,
  121.64 s: `target/round4-builder-a4-formant-resolution.log`.
  Full spectral units **23/0/0**, `target/round4-builder-a4-spectral-fixed.log`.
  High-rate model CPU budget remains to be measured in B.

- **A5 fixed:** queued sequenced echoes survived pattern loops, while Seek
  reset all echo metadata and released generated live owners. Emit a bounded
  pass-boundary control trigger before the next pass's note-ons and cancel
  only sequenced echoes there. Seek retains live echo queues/owners; arpeggio
  scheduling is rebuilt as before. `cargo test -p windfall-engine --test engine
  rack_voice -- --test-threads=1 --nocapture`: **27/0/0**, including original
  unchanged seek/loop PCM comparison and callback allocator/partition gates;
  `target/round4-builder-a5-fixed.log`. Baseline **0/1/0**:
  `target/round4-builder-a5-baseline.log`. Internal sequencer note-collection
  test helpers now distinguish the new control trigger, verify the two exact
  pass-boundary frames, and retain every note-list assertion; they previously
  assumed all trigger variants were note-ons in this fixture.
  `cargo test -p windfall-engine --lib sequencer -- --test-threads=1`:
  **10/0/0**, `target/round4-builder-a5-sequencer-fixed.log` (first run 8/2/0
  identified those helper assumptions; no note/frame expectations relaxed).

- **A6 fixed:** capture trimmed 12 frames of warm-up/group delay without
  offsetting launch admission. Source attack at input frame 1532 therefore
  appeared at output frame 1520 (peak 0.000154 before requested frame 1531).
  Offset recorded control frames by the trimmed warm-up and preserve stable
  same-frame request order. `cargo test -p windfall-engine --test performance
  -- --test-threads=1 --nocapture`: **4/0/0**, unchanged assertions;
  `target/round4-builder-a6-fixed.log`. Baseline **0/1/0**, diagnostic
  `target/round4-builder-a6-baseline.log`, `target/round4-builder-a6-probe.log`;
  temporary instrumentation removed.

## Resume / final gates

B in progress; A7 and final gates still pending. No UI-visible types or
TypeScript changed.

## B profiling and intermediate measurements

- Windows CPU sampling unavailable: `wpr -start CPU -filemode` returned
  **0xc5585011**, "Failed to enable the policy to profile system performance";
  `target/round4-builder-profile-start.log`. No elevation or downloads.
  Temporary release kernel timing harness (`--lib round4_profile`, 2/0/0)
  measures actual six-operator and 8x FIR kernels, with constituent-operation
  timing: matrix 200,000 lane substeps **51.3812 ms**, frequency-only work
  **29.7587 ms**; 10M unit phases remainder **48.1899 ms**, floor **14.3461 ms**;
  8x oversampler 200,000 host frames **82.7891 ms**, shaping-only **4.3153 ms**.
  `target/round4-builder-hot-kernel-profile.log`. Remove this temporary harness
  before final checks; these two tests are profiling, not accuracy acceptance.
- Unchanged release instrument baseline **11/9/0**, 194.88 s;
  `target/round4-builder-instruments-baseline.log`. Matrix **4.7294x**, Hybrid
  **5.4889x**; scheduling variability also failed Vintage Saw this run.
- Matrix coefficient caches invalidate on parameter configuration and actual
  lane pitch/rate/key change; unit-cycle phase normalization uses floor.
  Partial physics, stereo pan and filter coefficients are cached until their
  exact inputs change. First optimization: **12/8/0**, 134.56 s,
  `target/round4-builder-instruments-optimized1.log`: Matrix **1.4475x**,
  Hybrid **2.2043x**. Second: **15/5/0**, 119.95 s,
  `target/round4-builder-instruments-optimized2.log`: Color Scan **0.4531x**,
  Harmonic Filter **0.6487x**, Spectral Resynth passes; Snapshot Morph
  **0.7563x**, Metal Voice **1.1108x**, Matrix **1.5084x**, Hybrid **1.2778x**;
  GMS Analog/Phase/Ring pass **0.5526/0.7205/0.4804x**, Table fails **1.9767x**.
  No polyphony, partial counts, operator routes or workloads changed.
- Drive optimization uses mirrored FIR rings with unchanged coefficients and
  per-tap accumulation order, cached SVF coefficients and four independent
  cabinet dot-product accumulators (all taps retained; smaller roundoff bound).
  **All drive budget workloads pass** in
  `target/round4-builder-effects-optimized2.log`; grouped run **2/1/0**.
  The A4 high-rate Voice LPC expansion introduces a spectral CPU failure:
  Voice mode **1.630809x** vs **0.5**, Mix **0.480075x** at block 64. Mirrored
  LPC history removes per-lag modular addressing while retaining order/window.
  Accuracy/allocator regression checks and further spectral optimization pending.
- Current third instrument experiment: keep all 16 GMS Table harmonics using
  the sine recurrence in double precision; share one precise shimmer rotation
  per voice; dense FM uses the existing additive minimax polynomial with a new
  independent 1M-phase accuracy check. Standard-library reference remains
  unchanged. `target/round4-builder-instruments-optimized3.log` still running;
  not yet accepted or committed as a product fix.
- LPC profile **1/0/0** (`--release --lib round4_profile_formants`): 50k frames
  whitening/analysis **80.0726 ms**, synthesis **32.7846 ms**, 100 stereo
  estimates **154.1876 ms**; `target/round4-builder-lpc-kernel-profile.log`.
  Four-way full-window correlation and exact autocorrelation sharing for equal
  or inverted histories retain all order/window support and independent filter
  memories. Full effect budgets now **3/0/0**:
  `target/round4-builder-effects-optimized3.log`; Voice block 64 **0.368053x**
  versus 0.5, Mix **0.192187x**. Temporary profiling tests removed; source
  snapshots retained under `target/round4_profile_*.rs` for this machine.
- Third complete instrument run **18/2/0**, 74.49 s;
  `target/round4-builder-instruments-optimized3.log`: Snapshot **0.4473x**,
  Metal **0.4823x**, Matrix **0.6532x**, GMS Table **0.8340x** and Hybrid
  Serial **0.9589x** still fail. Cached oscillator increments and phase-only
  advancement for inaudible GMS branches preserve their phases across mode
  changes. Fourth run **18/2/0**, 69.50 s:
  `target/round4-builder-instruments-optimized4.log`; GMS passes every mode,
  but a naive per-sample sparse-route filter regressed Matrix/Hybrid. Replace
  it with prepared active-route masks and a branch-free dense path, borrowing
  rather than copying the coefficient cache. Focused Matrix **1/0/0**,
  **0.6819x**: `target/round4-builder-instrument-matrix-final-probe.log`.
  Hybrid focused and all accuracy/allocator checks still pending.
- Hybrid still unresolved: prepared graph construction, phase-only inactive
  oscillators, cached drive normalization and a SIMD-friendly independent
  six-carrier block bring Serial to **0.6758x** (tail passes), but the next
  Pairs variant measures **1.1045x** versus 0.75. All six operators, routes,
  unison lanes, and synthesis substeps remain. No budget/expectation changes.
  `target/round4-builder-instrument-hybrid-drive-probe.log` (**0/1/0**, 12.01 s).
  Stack/Parallel have not been reached by that unchanged fail-fast test yet.
  This is a remaining performance failure, not a claim that one-core admission
  is impossible. Seven original failing instrument rows have passing release
  measurements; complete final aggregate run and accuracy checks pending.
  DSP source formatted; full release unit accuracy check now running.
- Release DSP units **668/3/0**, 76.95 s;
  `target/round4-builder-dsp-unit-accuracy.log`. All signal/reference tests,
  including independent sine error, passed. Three existing standalone allocator
  probes invoke plain `rustc -Copt-level=1` against release ThinLTO archives and
  fail to link (`LNK1107` on the core archive plus unresolved core symbols).
  They are being rerun in the normal test profile with unchanged assertions;
  this release probe-harness issue is not a passing allocator result.
- Normal-profile complete DSP units **671/0/0**, 79.07 s,
  `cargo test -p windfall-dsp --lib -- --test-threads=1`;
  `target/round4-builder-dsp-unit-accuracy-debug.log`. All existing signal,
  reference, automation, partition and allocator probes pass, plus the new
  independent minimax error gate. Full release integration suite now running.
- Complete release DSP integration **224/0/5**, 123.20 s,
  `cargo test --release -p windfall-dsp --test dsp -- --test-threads=1`;
  `target/round4-builder-dsp-integration-accuracy.log`. All 20 instrument
  deadlines and all effect budgets passed in this aggregate run, alongside
  the unchanged expanded pitch, independent processor/reference, and callback
  heap checks. Existing five opt-ins remain ignored. Earlier focused Hybrid
  Pairs timing failure remains evidence of variability; required final explicit
  `--nocapture` budget runs will determine final reported admission numbers.
  DSP fixes now validated for numerical accuracy and allocation behavior and
  committed as a checkpoint. A7 next, then final serial gates.
