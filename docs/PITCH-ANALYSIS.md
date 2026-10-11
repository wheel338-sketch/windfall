# Monophonic pitch-analysis foundation

API/resource contract, 2026-10-08; base `7027569f8b466242a75faf327381e6ac44c88d28`.
R1 numerical repair is incremental from immutable
`60327573326a27ebdf000588e0ff0a7239ec30c0`; integration remains CLOSED pending
parent-owned fresh R2 review. Only pitch.rs, pitch_analysis.rs and this doc may change.
This is the independent M3 signal foundation. The whole phases 0–7 / 342-row
goal remains active. No job, inference, UI, IPC or project command is registered.
M1 is accepted; Session/native factory attachment remains gated by P1 preparation
under guards and Windows source namespace R2.

## Domain and worker API

`pitch::PitchAnalyzer::prepare(PitchConfig, PitchLimits, &mut Work)` checks the
configuration and prepares all FFT plans and reusable scratch off State guards
and off the audio thread. `analyze(PitchSource, &mut Work)` borrows immutable
interleaved f32 PCM, an `AudioShape`, a source-relative half-open `FrameRange`,
and a u64 frame origin. Shape/sample count, rates, ranges, origin arithmetic and
every borrowed PCM sample are checked; nonfinite samples are rejected before
any centering/normalization, including unselected channels and frames. Finite
clipped/over-unity samples are allowed. No file IO, model or UI state is involved.

Rates 8,000–192,000 Hz, 1–8 channels, F0 bounds 40–2,000 Hz (upper bound at most
rate/8), hop 1–100 ms are supported. An empty selection returns an empty complete
result after validation. A nonempty selection shorter than the required support
window is explicitly refused. The API is batch-only; it has no hidden tracking
history and repeated calls reset scratch. Partitioning restarts the hop grid and
constrains edge context to each selection; equal interior windows agree, but
arbitrary partitions are not promised bit-identical boundary estimates.

`ChannelPolicy::Channel(index)` analyzes that channel. `Strongest` selects the
channel with greatest centered energy independently in each support window,
breaking ties toward the lowest index. There is no channel averaging: anti-phase
stereo survives. A stronger unwanted source can win; this is monophonic analysis,
not source separation. The chosen channel is reported for every estimate.

Complete immutable results contain ordered hop cells covering exactly the
selection, integer absolute cell bounds and midpoint timestamps, actual support
bounds, optional estimated Hz, periodicity confidence and centered RMS. A voiced
cell has Hz; an unvoiced cell has none. Segments cover the same half-open range,
splitting on voicing, selected channel or adjacent pitch jumps (default 100 cents).
Segment Hz is a geometric mean, confidence an arithmetic mean. These are detected
regions, not MIDI notes or user edits; gradual glides can remain one region.
Confidence is `1 - interpolated_CMND_minimum_depth`, bounded 0–1;
it is a periodicity score, not a calibrated probability of correct pitch.
Unvoiced confidence is zero; a strong periodic interferer or a dominant harmonic
can have high confidence while being the wrong intended voice. There is no
probabilistic calibration or temporal Viterbi/median smoothing in this version.

All cancellation/deadline/work, capacity, arithmetic, allocation and input failures
are visible errors. Failed work returns no partial result. Cancellation/deadlines
are cooperative through public M1 `Work` checkpoints, including preparation,
validation and between bounded FFT calls. No realtime, live tuner or latency claim.

## Algorithm and budgets

Own implementation of fixed-comparison-window YIN steps: squared differences,
cumulative mean normalization, first interpolated trough below a fixed threshold (default
0.15), and raw-difference parabolic lag interpolation (paper II.E). No forced
best-lag fallback for noise.
Subtract a constant window mean; no PCM sanitation. A centered RMS floor (default
0.003) gates silence. No copied upstream detector code or external audio/model.

Let L = ceil(rate/min_hz), W = 2L, T = W+L+1, and N the next power of two >= 2T.
The first W centered samples form A; all T form B. Zero-padded FFT correlation
`IFFT(conj(FFT(A))*FFT(B))/N` gives `sum A[j]*B[j+lag]`. Prefix squared energies
give each exact fixed-W squared difference without a quadratic lag loop. Include
one neighbor above L for interpolation. Independent direct f64 sums check
this math at all lags, beyond checks of sinusoidal frequency labels.

Use the pinned scalar `rustfft::algorithm::Radix4` explicitly, rather than opaque
planner caches: three O(N log N) transforms per hop, two complex f64 signal
buffers, one complex f64 scratch buffer, f64 prefix energy and lag scores.
The exact-version constructor reserves 2N twiddles then shrinks below N; the
plan-memory bound includes both forward/inverse plans and constructor transients.
All vector allocation uses checked capacities; rustfft's constructor internally
uses infallible allocation, so process-wide allocator exhaustion remains the
usual Rust abort limitation. Limits bound requested allocation, not OS RSS or
allocator metadata, and cannot preempt one native FFT call.

Defaults: 16 MiB analyzer allocation bound, 64 MiB per-result bound,
200,000 cells / segments, and 86,400,000 selected frames (30 minutes at 48 kHz,
7.5 minutes at 192 kHz; callers can explicitly raise this checked frame cap).
Borrowed input bytes are separately limited to 1 GiB; no source copy. Work units conservatively
charge scalar arithmetic/visits and FFT N log2 N terms, not pretend to measure
CPU instructions. A preflight requirement reports validation, worst-case per-hop
work and peak output before executing. Actual dimensions, charges, allocation
measurements and CPU/quality results are recorded below. Hop lengths are integer
frames; at 22.05 kHz the reference's 220-frame hop is about 9.977 ms. Source
timestamps are never converted through floating-point seconds by the detector.

## Provenance and later attachment

- [de Cheveigné and Kawahara, YIN (2002), paper proof](https://www.ee.columbia.edu/~dpwe/papers/deChevK02-yin.pdf),
  original paper; algorithm equations only. FFT cross-correlation is our derived
  acceleration, checked against direct sums, rather than a claim of all six YIN
  refinements or pYIN. [NRAO Fourier appendix](https://www.cv.nrao.edu/~sransom/web/A1.html)
  explains correlation, DFT normalization and zero padding.
- [rustfft 6.4.1](https://docs.rs/rustfft/6.4.1/rustfft/algorithm/struct.Radix4.html),
  source commit `4758ab0dd6f256c50ac8987c75c9cb96152dc2ca`; exact cached crate
  SHA256 `21db5f9893e91f41798c88680037dba611ca6674703c1a18601b01a72c8adb89`
  matches Cargo.lock. Inspected Cargo.toml, README, LICENSE-MIT, LICENSE-APACHE,
  src/lib.rs and src/algorithm/radix4.rs. MIT OR Apache-2.0 is compatible
  with GPL-3.0-or-later. MIT notice: Copyright (c) 2015 The RustFFT Developers;
  retain the full permission/copyright notice in redistribution. The archive has
  no separate NOTICE file. Existing default avx/sse/neon features are unchanged;
  this module intentionally uses the auditable scalar power-of-two algorithm.
  Existing locked num-complex 0.4.6 / num-integer 0.1.47 / num-traits 0.2.19 /
  primal-check 0.3.4 / strength_reduce 0.2.4 / transpose 0.2.3 dependencies remain
  unchanged (cached manifests declare MIT OR Apache-2.0); no new package or
  feature/version. Existing GPL app/codec redistribution obligations remain.

Later backend work must capture/fingerprint immutable unsanitized input, prepare
on its worker before acquiring guards, share cancellation/deadline accounting,
and retain source identity/revision/range/config/algorithm version alongside the
complete estimates. The existing model-requiring adapter must not be given a
fake manifest for this classical algorithm; parent approval of a precise adapter
seam is still needed. Applying future edits must recheck staleness and use one
checked undoable proposal, with user overrides separate from detected values.

Full M3 still requires persistent editable pitch/timing regions, audition and
non-destructive render, transient warp markers with existing stretch, duration/
pitch and marker-order validation, undo/save/reopen, audition/export equivalence,
stale-source tests and musical listening evidence. Realtime correction/harmony
requires separate prepared tracking/synthesis latency work; existing stretch
latency and approximate formants must remain explicit. A live selected source
and calibrated display are also required for a tuner. This foundation completes
none of those product/parity rows and makes no real-vocal quality claim.

## Declared reference and acceptance (before execution)

Authored deterministic f64 synthesis, cast once to raw f32: stationary calibrated
notes 55/82.406889/110/220/440/880 Hz at 8/22.05/44.1/48/96/192 kHz; harmonic
complexes with stronger second harmonic and missing fundamental (2nd+3rd+4th);
linear 180–360 Hz glide and ±35-cent 5 Hz vibrato about 220 Hz. Expected F0 comes
from the oscillator phase derivative, not from another copy of this detector.
Evaluate dynamics outside 80 ms edge margins. Fixed acceptance: stationary
p95 <=5 cents, max <=10 cents, voiced recall >=99%; dynamic p95 <=25 cents,
recall >=95%; octave errors (>=600 cents) zero on this declared set. Silence,
DC, deterministic broadband noise and isolated impulses: false voicing <=1%.
Tone/silence and 220→330 Hz step boundaries <=40 ms; integer range continuity
is exact. These are engineering thresholds, not inferred from product errors.
Clipping, edge rates, anti-phase stereo, short/refused inputs, invalid values,
large frame origins, empty/reset/partition behavior and resource limits are also
covered. Separately measure allocator-request peaks and warmed CPU throughput;
neither is an OS RSS or realtime deadline guarantee.

## Immutable 603 measured checkpoint (before R1 repair)

2026-10-08, Windows x86_64 MSVC; rustc 1.99.0
`b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`, LLVM 23.1.1; Visual Studio 2022
BuildTools environment from `scripts/msvc-env.sh`. Intel Core i9-14900F,
24 cores / 32 logical processors. One Cargo owner, jobs=1, tests serial;
dev/test profile opt-level=1, dependencies opt-level=3; scalar f64 Radix4.
No GUI/engine/root build, release build, source registration or listening trial.

- Stationary notes/harmonics: 1,286 / 1,286 cells voiced (100% recall), p95
  absolute error **0.063037 cents**, max **0.945309 cents**, **zero octave errors**.
  Each of 36 rate/note cases is 0.3 s; each of four 48 kHz harmonic cases is 0.5 s.
  All cells, including edges, count. Strong-second mix amplitudes f1/f2/f3/f4 =
  0.2/0.6/0.35/0.2; missing-fundamental mix = 0/0.6/0.35/0.2. Third partial
  has 0.3-radian phase offset. These do not cover arbitrarily dominant harmonics.
- Linear glide 180–360 Hz in 2 s: 184 evaluated cells, 100% voiced recall,
  p95 **5.972405 cents**, max **6.370509 cents**, zero octave errors.
- 5 Hz vibrato: 184 cells, 100% recall, p95/max **8.729498 cents**, zero octave
  errors. Frequency is 220 Hz plus a sine with depth
  `220*(2^(35/1200)-1)` Hz (positive excursion +35 cents; negative about -35.72).
  Phase is its analytic integral. Dynamic endpoints omit 80 ms per side.
- Silence, DC=0.9, fixed-seed broadband noise and four isolated unit impulses:
  800 cells, **0 false voiced (0%)**. This is not a noisy-vocal evaluation.
- Known 0.5 s silence / 220 Hz / 330 Hz / silence boundaries: recognized-state
  bracket errors **0, 20, 20 ms**, within the fixed 40 ms limit. The bracket is
  the last correct old state before and first correct new state after the known
  step, each within 25 cents when voiced. Mixed transition windows may be
  unvoiced/intermediate; no claim of sample-exact acoustic onset detection.
  Cells and segments cover the source range exactly with no gaps/overlaps.
- Anti-phase stereo survives; explicit channel and centered-energy selection
  choose the declared fixture sources. Clipped 220 Hz remains within 5 cents;
  DC/very small finite PCM stays unvoiced, and scaled tones up to f32's finite
  amplitude limit remain finite and correctly detected. Every NaN/±Inf, including
  unused channel/outside-selection samples, is refused before centering.
- Valid 192 kHz / 40–2000 Hz configuration tests 40.1 and 1990 Hz and an exact
  minimum-support selection. Its L/W/T/N = 4,800/9,600/14,401/32,768; reusable
  scratch **1,726,496 bytes**, conservative analyzer peak **3,827,744 bytes**.
  8-channel selection, empty/short/invalid ranges, overflow and origins above
  2^53, complete partition interior equivalence and reusable reset all pass.

Default 48 kHz / min=50 Hz: L/W/T/N = 960/1,920/2,881/8,192. Support is about
60.02 ms, which limits temporal precision; the cell midpoint is an exact frame
label, not a promise of instantaneous F0 there. Reusable array storage is
**423,968 bytes**; conservative analyzer peak bound is **952,352 bytes**.
On this ABI estimates occupy 88 bytes and segments 48 bytes; output vectors are
reserved once, with segment capacity `min(cell_count, segment_limit)`.
The worst-case output bound is `cells*88 + segment_capacity*48` bytes.
Segment-limit exhaustion during execution refuses the whole result, rather
than returning a truncated region list. Allocator metadata, borrowed PCM,
caller Work/token allocations, previously retained results and OS RSS are excluded.

For auditable work admission, preparation charges
`N*(16*log2(N)+16) + scratch_bytes`. Each hop charges
`N*(96*log2(N)+32) + 16*T*(channels+1) + 32*(L+2) + 128`, including conservative
FFT arithmetic, all-channel window visits and segmentation. Validation charges
one unit per borrowed interleaved sample. Silence is charged the same worst-case
hop budget. At default mono this is **2,258,976 preparation units** and
**10,608,864 units/hop**. These are reproducible conservative accounting units,
not measured instructions/FLOPs. Work.check tests cancellation/deadline after
each FFT, between channel scans and before/after validation chunks of 4,096
samples. A single FFT up to N=32,768 is not preemptible; neither are allocator
calls. Exact-charge admission, one-unit-short failure after some cells, async
cancellation, running deadline expiry and reuse after errors are tested.

Final 30 s mono run: 3,000 estimates, output capacity **408,000 bytes**;
**31,828,032,000 work units**, measured requested heap peak **1,093,504 bytes**,
**13 allocation/reallocation calls**, zero remaining tracked bytes after drop.
Preparation 170 µs, analysis **552 ms / 54.3× audio speed** in the final run;
prior warmed runs were 477–540 ms. Timing varies with scheduling/CPU load.

Explicit 30-minute valid run: borrowed input **345,600,000 bytes**, 180,000
estimates / one voiced segment; output capacity **24,480,000 bytes**, measured
requested heap peak **25,165,504 bytes**, still **13 allocations** and zero
remaining tracked bytes after drop. Work **1,909,681,920,000 units**, analysis
**32,611 ms / 55.2× audio speed**. It runs as an explicitly ignored-by-default
resource test because ordinary CI need not allocate that source. This demonstrates
long valid clip support; it does not demonstrate general vocal quality or live
processing. No shipped external corpus, model, proprietary code or audio asset.

Failure history retained for review: the first synthetic run had max error
10.527528 cents with normalized-difference interpolation. Using the paper's
raw-difference abscissa removed that bias without widening acceptance thresholds.
The first boundary assertion expected two directly adjacent exact notes across a
mixed-support step; it was replaced by the independently known transition bracket
above with the same 40 ms bound. The extended edge test exposed a 1100 Hz tone
being mislabeled 550 Hz when search began at the 1000 Hz bound. The search now
inspects shorter lags and refuses its first above-range trough, with a regression
test. A first-trough estimate outside either requested frequency bound is
unvoiced; no out-of-range Hz is clamped into range. Near an exact bound, small
estimation error may therefore reject a cell. Clippy's test-iterator findings
were fixed with `rfind`; no warnings were suppressed. Those owner tests were
green at 603; independent R1 subsequently found the missed octave case below.
Real-vocal/formant-heavy/breathy/creaky and noisy-speech quality,
listening, polyphony, universal octave robustness and platform CPU comparisons
remain unmeasured and must not be inferred from these synthetic results.

## Immutable 603 reproduction and source evidence

All Cargo commands below used Git Bash after `source scripts/msvc-env.sh`, with
`CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=target/m3-native
TS_RS_EXPORT_DIR=target/m3-native/bindings`; one command at a time. No exported
bindings appeared outside the private target. Native codec dependencies are
already part of this crate; this detector calls none of their IO/sanitation APIs.

```text
cargo test -p windfall-analysis --locked --offline -- --test-threads=1
# 8 unit + 30 existing lifecycle + 13 pitch integration tests passed;
# one explicit long resource test ignored; doc-tests empty.
cargo test -p windfall-analysis --test pitch_analysis --locked --offline -- --test-threads=1 --nocapture
# final metrics above; 13 passed / one ignored
cargo test -p windfall-analysis --test pitch_analysis thirty_minute_valid_clip_measurement --locked --offline -- --ignored --exact --test-threads=1 --nocapture
# one passed, actual 30-minute source analyzed
cargo clippy -p windfall-analysis --all-targets --locked --offline -- -D warnings
# passed without suppression
rustfmt --edition 2024 --check crates/windfall-analysis/src/pitch.rs crates/windfall-analysis/tests/pitch_analysis.rs
git diff --check
# both passed
```

Exact SHA256 at this source checkpoint (paths relative to this worktree):

```text
crates/windfall-analysis/src/pitch.rs
5fe34deb6f518d826c942f857aa1fdb3a920f03b5dab4800229ed7c08dacf730
crates/windfall-analysis/tests/pitch_analysis.rs
56261bce869d67d7830845e78db8576fde5ba3ba46685f2f378baf04a14ae68f
crates/windfall-analysis/src/lib.rs
338e5b739182152131adeb1824ca794a14460ecf517d993d2af71c147fcb75b0
crates/windfall-analysis/Cargo.toml
36ead4e56fcacd6d5823611e26556aca661b9f13cf493e34491a66ec8d327838
Cargo.lock
92882ebb59145d649c7d60000e7999ee7c8acd185d72efc51345126f89bc2455
```

Byte comparison against base lock SHA256
`ceb58c2965aecd9d505e4976b6be57521b1abe21c1974f9365119e422c05c255`
proves the only lock bytes added are ` "rustfft",` in windfall-analysis's list.
Source window is exactly this new doc, new pitch module/test, one lib module line,
one commented pinned dependency, and that lock reference. A local source-only
commit records these six files; no push, PR, release, import or root mutation.
Parent independent Standards/Spec review and precise later attachment grants
remain required; this is not an M3 completion/parity claim.

## R1 P2 numerical repair: contract and declared validation

Spec R1 found sampled-depth thresholding could miss the fractional-period trough
and choose an exactly repeated two-period lag. Standards R1 had no hard violation;
its optional shared raw-difference helper is addressed only to keep the numerical
paths coherent: interpolation now uses the same finite/negative-roundoff checks
as CMND construction. No dependencies, limits, API shape, source-coordinate
rules, PCM policy, preparation or registration seams change. The algorithm identity
is `windfall-fixed-window-yin-fft-v2` because selection/confidence semantics change;
v1 is preserved in immutable 603, with no amendment or parent import.

The existing public `prepare/analyze` seam, explicitly granted by this repair
brief, is used for RED/GREEN tests. No mocks, new test seams or child agents.
The single 8 kHz / 8.5-sample / second harmonic 1.2x regression is compiled first
against unchanged 603 pitch.rs. The bounded grid is declared before execution:
rates 8/12/16/22.05/44.1/48/96/192 kHz; first period band starts at
`ceil(rate/min(2000,rate/8))`, the other eight samples longer; fractions
0.125/0.25/0.375/0.5/0.625/0.75/0.875; second-harmonic amplitudes 0/0.6/1.2/2.0
times the fundamental; phases 0 and 0.7 radians. These 896 authored 0.1 s cases
use analytic `F0=rate/period` truth and the unchanged default 0.15 threshold.
Fixed acceptance: >=99% voiced recall, every voiced estimate within 25 cents,
zero octave errors (>=600 cents). All cells/edges count, with no rate omissions.
The original calibrated-note/dynamic/noise thresholds remain unchanged.

Separate direct f64 squared-difference / quadratic-fit reference checks probe
the supported threshold interval and both sides of interpolated acceptance and
Hz-range boundaries. This bounded slow reference uses raw sample subtraction,
no FFT, prefix energies or production helper. Frequency truth stays analytic;
reference math verifies selection/refusal semantics rather than defining pitch
quality from the product's own outputs. All original signal, lifecycle, reuse,
cancellation and resource checks, including the explicit 30-minute run and strict
scoped Clippy/owned fmt/diff, were rerun on frozen numerical source. Parent
alone starts R2. This repair does not close foundation integration or any M3/parity row.

## R1 RED/GREEN and equations

The targeted command below was compiled and run twice before editing pitch.rs,
while HEAD remained 603 and pitch.rs SHA256 was exactly
`5fe34deb6f518d826c942f857aa1fdb3a920f03b5dab4800229ed7c08dacf730`.
Only the new regression test was added then. Both runs failed on the reviewer's
actual symptom; no threshold tuning was used to obtain RED. The first RED test
executable SHA256 was
`f88d4d54f18e328963bdaac4e8ba45836d093831eed5acb79c4a79e67bfd8a34`
(a private build artifact, not shipped or committed).

```text
cargo test -p windfall-analysis --test pitch_analysis r1_fractional_period_harmonic_trough_does_not_skip_to_octave --locked --offline -- --exact --test-threads=1 --nocapture
RED against 603:
expected_hz=941.176470588, first_hz=470.5402486549174,
first_confidence=1.000000000, voiced=30, octave_errors=30
assertion failed: left=30, right=0
test result: FAILED. 0 passed; 1 failed

GREEN on repaired frozen numerical source:
sampled_depth=0.171944363, interpolated_depth=0.075541284,
lag17_depth=0.000000000
expected_hz=941.176470588, first_hz=941.1764705882352,
first_confidence=0.924458716, voiced=30, octave_errors=0
test result: ok. 1 passed; 0 failed
```

The RED direct-math diagnostic initially fit around lag 9 (sampled depth
0.188660976, unconstrained fitted depth 0.057602902). The final diagnostic
correctly identifies lag 8 as the local sampled trough and fits its neighbors
7/8/9, yielding the GREEN depths above. This diagnostic correction did not
change the authored PCM, F0 truth, octave assertion or acceptance threshold.

Cause: 603 required a **sampled** CMND value below 0.15 before interpolating,
so it skipped the admissible first trough and selected the repeated period at
17 samples. Independent direct f64 differences agree with the FFT difference
test; raw-period interpolation alone cannot rescue a skipped candidate.
[YIN §II.E](https://www.ee.columbia.edu/~dpwe/papers/deChevK02-yin.pdf) specifies
using each interpolated CMND minimum's ordinate for selection and the
corresponding raw-difference minimum's abscissa for period estimation.

For lag k and fixed comparison W, the unchanged equations are
`D[k]=sum(j=0..W-1)(x[j]-x[j+k])^2`, and
`C[k]=k*D[k]/sum(i=1..k)D[i]`, with C[0]=1 and all-zero sums producing C=1.
At a sampled local trough, put `a=C[k-1], b=C[k], c=C[k+1]` and
`Q=a-2b+c`. The parabola is
`p(u)=b + (c-a)*u/2 + Q*u^2/2`. Its vertex is
`u=(a-c)/(2Q)` and its depth is `p(u)`. The detector traverses local troughs
in lag order (strict descent, nonstrict ascent for the leftmost plateau),
accepting the first whose interpolated depth is **strictly** below q. A valid
local trough has Q>0 and its vertex lies within half a sample. A negative
modeled depth is projected to zero, which cannot change acceptance because
every supported q is positive; this does not sanitize any PCM.

For the selected k, repeat that abscissa fit using raw D[k-1..k+1], retaining
603's ±0.5-sample interpolation clamp and zero offset for nonpositive curvature.
Return `rate/(k+u_raw)` only if it is within the exact requested Hz interval;
otherwise return unvoiced and do not hunt for a later subharmonic. Confidence
is now `clamp(1-p(u_CMND),0,1)`, a periodicity score, still not calibrated F0
probability. No global-minimum/noise fallback was introduced. The shared raw
helper retains the exact finite check and negative tolerance
`-1e-10*max(total_window_energy,f64::MIN_POSITIVE)` from 603 and clamps only
accepted derived roundoff to zero. It is used consistently for both normalization
and period interpolation, addressing Standards' optional duplication concern.

## R1 measured coverage and limitations

The frozen-source scoped run passed **8 library + 30 existing lifecycle + 17
pitch integration tests = 55 ordinary tests**, with one explicit long resource
test ignored by default. The additional four pitch tests are the original
reviewer regression, the 896-case rate/period/harmonic grid, direct threshold/Hz
boundary probes, and the explicit wide-range stricter-threshold limitation.
No mock/private helper supplies the analytic F0 truth; existing independent
FFT/direct-difference math, stereo, PCM/range/refusal, reset/partition,
cancellation/deadline and exact-charge tests all pass again.

The expanded default-q=0.15 grid has **9,072/9,072 voiced cells (100%)**,
p95 **5.890542901 cents**, max **8.276147615 cents**, and **zero octave errors**.
It includes every declared rate, phase, fraction and harmonic mix, without
discarding edge cells or failing configurations. Original stationary p95/max
remain 0.063037/0.945309 cents; glide/vibrato p95 remain 5.972405/8.729498 cents;
original noise false voicing remains **0/800** and boundary errors remain
0/20/20 ms. Original thresholds, RMS floor and rate/channel limits are unchanged.

Boundary probes use 18 authored combinations: periods 8.25/8.5/8.75, second
harmonic ratios 0.6/1.2/2.0, phases 0/0.7. A one-cell 8 kHz / 900–1000 Hz
window has independently known W/L/T=18/9/28 and support [386,414), timestamp
400, also checked with absolute origin 2^54+17. Direct quadratic coefficients
and derivatives predict selection/refusal without FFT or production helpers.
There are **108 threshold probes**: 0.01, fundamental trough depth±1e-5, 0.15,
0.29999 and 0.3. **Six** derived probes below the allowed 0.01 bound are checked
configuration refusals (the modeled trough is zero), not omitted cases.
Of **102 valid probes, 72 voice and 30 refuse**, agreeing with independent math
and confidence, with no octave estimates. **72 Hz-boundary probes** bracket
the independently fitted raw Hz by ±0.1 Hz on both minimum and maximum bounds;
all agree with conservative admission/refusal and exact integer support.

The narrow range cannot admit a 17-sample lag, so a separate full 50–1000 Hz
grid records rather than hides the remaining threshold tradeoff. Its exact
one-support selection (W/L/T=320/160/481) uses the same 8.5-sample signal:

```text
q=0.010000000: chosen lag17, 470.540248655 Hz, confidence1, OCTAVE ERROR
q=0.075531284: chosen lag17, 470.540248655 Hz, confidence1, OCTAVE ERROR
q=0.075551284: chosen lag8, 941.176470588 Hz, confidence0.924458716
q=0.150000000: chosen lag8, 941.176470588 Hz, confidence0.924458716
q=0.300000000: chosen lag8, 941.176470588 Hz, confidence0.924458716
```

These **two octave errors in five wide-range threshold probes** are explicitly
expected by the primary selection equations: a stricter q rejects the true
interpolated trough while its perfect two-period repetition remains admissible.
They are not counted as correct pitch or folded into the default-q grid's zero
octave claim. The limitation test passes because the public detector agrees
with direct math and reports the errors; it is not evidence that stricter
thresholds eliminate octave mistakes. Confidence near 1 on that wrong octave
is a further reason not to interpret periodicity as F0 probability. Preventing
all such harmonic/threshold errors would require further quality research or
tracking policy, outside this numerical correction. The default-q reviewer
case and bounded declared default grid are corrected, not universal F0 quality.

No new allocations, FFT calls, plan caches or buffers are introduced. Memory
and work formulas are unchanged. The new local-trough fit adds only bounded
O(L) scalar work under existing conservative 32*(L+2)+128 and 32*N allowances;
3*N-log-N transforms still dominate. The final 30 s run used preparation
219 µs and analysis **503 ms / 59.6×** audio speed, with the same **1,093,504-byte
requested heap peak, 13 allocation calls, 31,828,032,000 work units** and zero
tracked bytes after drop. The final explicit 30-minute run passed on frozen
pitch.rs/test hashes below: **33,282 ms / 54.1×** audio speed, **180,000 estimates
/ one voiced segment**, borrowed source **345,600,000 bytes**, output capacity
**24,480,000 bytes**, requested heap peak **25,165,504 bytes**, **13 allocation
calls**, **1,909,681,920,000 work units**, zero tracked bytes after drop. An earlier
repaired-source run before adding the final limitation test took 30,096 ms with
the same heap/work figures; CPU timing varies with scheduling/load. These are
worker measurements, not OS RSS, realtime deadlines or listening evidence.
All real-vocal/listening, broader-platform, full M3 editor/render/warp and
realtime-correction requirements remain open. Parent R2 review and attachment
authorization remain separate; no parity row or integration gate is closed.

## R1 frozen-source reproduction and hashes

Same MSVC/Rust/CPU/build environment as 603, using msvc-env.sh, jobs=1 and the
private target/m3-native (test-local TS_RS_EXPORT_DIR unchanged). Commands were
serial, with no other Cargo owner in this worktree:

```text
cargo test -p windfall-analysis --test pitch_analysis r1_fractional_period_harmonic_trough_does_not_skip_to_octave --locked --offline -- --exact --test-threads=1 --nocapture
# compiled RED against unchanged 603, then GREEN after selection correction
cargo test -p windfall-analysis --locked --offline -- --test-threads=1 --nocapture
# frozen-source final: 8 + 30 + 17 passed, one long test ignored
cargo clippy -p windfall-analysis --all-targets --locked --offline -- -D warnings
# frozen source: passed without suppression
rustfmt --edition 2024 --check crates/windfall-analysis/src/pitch.rs crates/windfall-analysis/tests/pitch_analysis.rs
# passed
cargo test -p windfall-analysis --test pitch_analysis thirty_minute_valid_clip_measurement --locked --offline -- --ignored --exact --test-threads=1 --nocapture
# explicit long resource test on frozen final numerical/test source
git diff --check
# passed; final incremental window is exactly three paths
```

Final source SHA256:

```text
crates/windfall-analysis/src/pitch.rs
58a63912d2842f61e976867db1490b13cd0fb4b48373dd2f8b96d2e57bdd0f1f
crates/windfall-analysis/tests/pitch_analysis.rs
7ce54c56bd2bce5639d3d829ed232c173486d1dbe16b68dbdbf925743a1b40af
```

The incremental local source-only commit has direct parent
`60327573326a27ebdf000588e0ff0a7239ec30c0`, preserving its direct base
`7027569f8b466242a75faf327381e6ac44c88d28`. Only pitch.rs, pitch_analysis.rs and
this doc change; lib.rs/Cargo.toml/Cargo.lock bytes remain exactly those of 603.
No amendments, new dependencies, source/job/UI/IPC/registry edits, parent imports,
push, PR, release, new agents or review threads. The full phases 0–7 / 342-row
parent objective remains active; parent alone starts fresh R2.

## Shared realtime math, 2026-10-11 (written, not executed)

The spectral effects worker extracted the existing first interpolated CMND-trough
search and parabolic minimum into `windfall-core/src/yin.rs`. This batch detector
now calls that shared helper; its FFT differences, Work checkpoints, bounds,
raw-difference interpolation, error policy and algorithm version are unchanged.
Pitch Correct supplies fixed direct differences to the same math from prepared
streaming storage; it does not invoke this worker/batch API on audio. Existing
analysis reference tests must be rerun by the integrator to confirm numerical
identity. This change establishes neither a new analysis quality claim nor new
measurements of the realtime processor.
