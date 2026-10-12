# E3 delay-family source contract

## 2026-10-12 shared host admission

The host now keeps a controller-wide echo-history pool across publications and
stream attachments. Its retained limit is 1 GiB and peak limit is 1 GiB plus
64 KiB of fixed staging headroom. Before constructing replacement effects, it
atomically reserves the entire candidate's new echo payload against all
installed, fading, queued, speculative and retiring owners. Reused owners are
charged once. Leaving plan definitions also participate in the existing
per-plan size check; omitting them from a later plan does not release their
actual owner charge.

A reservation moves with each prepared effect unit through callback adoption
and retirement. It is released only after the concrete unit's history storage
is destroyed on the control side. Candidate cancellation/failure returns unused
payload and staging. Concurrent preparation includes other workers' staging
conservatively in retained admission; zero-allocation owner reuse still proceeds.
Old-rate owners stay charged during preparation at another rate. Standalone
offline renderers have their own pool; this is a bound on echo payload for one
controller, not a cap on all engine/plugin/audio-asset memory or allocator
bookkeeping.

New deterministic small-budget tests cover retained/peak refusal with unchanged
installed PCM, full-budget parameter reuse, queued/cancelled candidates, fading
and retirement capture/drop, changed-rate admission, and returning failed-build
charges. Existing saturating-tail and processor preparation tests are retained.
All additions are authored, not executed by the edit-only worker. See
`docs/qa/2026-10-12-p2-fx-dyn-eq.md` for the integration hand-off.

2026-10-11 continuation: both registered processors now have purpose-built
editors (linked eight-unit routing and drawn sixteen-band delay/level/pan),
fallible host admission and saturating aggregate tails. The original DSP
contract below is retained. The four named integration gates are full-row
source candidates; new tests are written, not executed. See
[the current handoff](qa/2026-10-10-fx-delay-mod-reverb.md) for commands and limits.

Source checkpoint; no effect registry, project, engine, IPC, generated bindings,
UI, parity or release changes. Ownership is confined to the two new public DSP
modules, their private submodules, this document, the auto-discovered integration
test, and two `pub mod` declarations. Existing effects retain their indices.

## Public references and scope

Read 2026-10-08: [Image-Line Delay Bank manual](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/Fruity%20Delay%20Bank.htm)
describes eight units, signed sends to the next unit, input/output balance,
pre/post filtering, eight filter responses, up to three sections, stereo offset,
stereo feedback modes, and echo granulation. Its filter gain range is 0.1–2;
it does not specify a numerical delay-time range. Windfall supplies its own
transfers, explicit time range and headroom policy below. Oversampling and
granulation are not in this first source checkpoint.

[Image-Line Multiband Delay manual](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/Multiband%20Delay.htm)
describes sixteen nominal 20 Hz–20 kHz bands, independent delay (0–1000 ms),
level (0–1) and pan, a 100 ms scale range, and several splitter responses.
Windfall implements two explicitly complementary splitter responses. A linear
phase splitter, eight curve snapshots/morph, harmonic semitone delay/weighting,
pitch-glide mode, and wet compression/saturation/HP options remain subsequent
work. These sources describe controls, not proprietary algorithms. No assets,
presets or sound-identical claims are made.

## EchoBank

Exactly eight units, numbered 0–7. Each receives
`input_gain * global_input * balanced(dry) + next_send[i-1] * node[i-1]`.
Here `node` is the processed, enabled, balanced unit signal before its main
output gain. Main output sums `output_gain[i] * node[i]`; next sends are
independent of main-output level, so intermediate serial units can be unheard.
There is no cyclic cross-unit graph; feedback cycles are local stereo delay
loops. Unit 7 has no next destination; its next control is reserved and has no
audible destination. Setting all next sends to zero is parallel routing;
zeroing inputs after the first and enabling next sends is serial routing.
Any mixture gives a feed-forward chain with parallel taps. Signed sends and
output levels permit phase inversion. Unit enable ramps its dry injection and
output contribution; linked input, history and filters continue running.

Input and feedback each have separate filter controls: off, lowpass, bandpass,
notch, highpass, low shelf, peak, high shelf; frequency 20–20000 Hz; Q 0.5–10;
shelf/peak gain −18–18 dB; gain 0.1–2; one, two or three sections. Reuse the
existing public SVF filter-family transfer. The off path is unity before gain.
All sections run continuously and their selection is ramped. Input filtering
can move before/after the delay using ramps at both the write and read stages.
For post weight `w`, the transient is `(1-w)*D((1-w)*Fpre(x)+w*x)` plus
`w*Fpost(D((1-w)*Fpre(x)+w*x))`; it includes dry and cascaded filter terms
midway through the edit. It is exactly pre-filtered at zero and post-filtered
at one, without allocating a second full history. Feedback filtering applies only to recirculation;
the first echo is consequently clean of the feedback filter.

Time is 1–24000 ms or the existing fourteen `NoteDivision` values at 10–1000
BPM; longest whole note at 10 BPM is 24000 ms, without truncation. Stereo offset
is −1000–1000 ms, extending one channel, so preparation reserves 25 seconds.
Time is rounded to whole samples, minimum one, and a left impulse at frame 0
appears at frame `round(time_seconds * actual_rate)` on the left. Normal
feedback preserves channels. Windfall inverted swaps the injected channels,
and repeats stay on that swapped side; ping-pong preserves injection and swaps
on every recirculation. This is Windfall's explicitly defined channel-inversion
behavior; the manual does not specify a sample equation. Signed output and send
controls provide polarity inversion. Stereo separation scales the side signal
from zero (mono) to one (original stereo). Balance preserves unity at center
and attenuates the opposite side at either endpoint, without crossfeed.
Its explicit law in both processors is
`[L * (1-max(pan,0)), R * (1+min(pan,0))]`.

Gain/pan/mode/enable/filter edits use frame-count ramps; time edits use two
fixed taps and a latest pending request. An ongoing fade finishes before the
pending fade starts. Its first frame retains the old tap exactly. No tap arena
grows under rapid automation; at most two reads per channel per unit. New taps
wait for valid history after a live edit. Reset discards history and applies the
latest requested values on the first nonempty frame. Empty process calls and
duplicate writes do not advance or restart anything.

The feedback range is 0–1, multiplied by the global feedback amount. Filters
can boost, and unity feedback may sustain. The stored loop signal is bounded
to ±64 after filtering/gain and summation; processor inputs are sanitized to
±1000 and nonfinite values to zero. This explicit hard headroom boundary may
distort extreme settings. No guarantee of decay is made for a boosting loop.
`tail_samples()` is `usize::MAX` while feedback is nonzero or fading; zero
feedback reports a conservative filter/chain/history horizon. `gap_samples()`
also covers filter history and every possible unit hop. No host should treat
an unlimited tail as a duration to allocate. User-selected render horizons
remain required for sustained feedback.

Echo times are musical processing, not algorithmic PDC: latency is zero.
Warm-up/readiness reports the longest enabled input-to-output tap path,
including signed links and current/target ramp values, separate from PDC.
Unused parallel units do not add a serial warm-up penalty.
Host bypass accounting must retain wet tails until its fade reaches zero and
clear the processor on dormant wake (the existing utility repair contract).
Direct processors expose no slot bypass. Actual EffectSlot/engine integration
and its bypass regressions wait for the parent's serialized registry window.

## FrequencyDelay

Exactly sixteen independent stereo histories, delays 0–1000 ms, levels 0–1,
pan −1–1 and enabled flags. Deselecting a band crossfades to its unprocessed
split signal while continuing its delay history. Global dry/wet 0–1, feedback
0–1, scale −1–1, short range toggle, bandwidth 0.5–2 and splitter choice are
stable descriptor-addressed controls. Positive scale gives `scale * delay`;
negative scale gives `abs(scale) * (1000 - delay)`; zero gives zero time. Short
range multiplies by 0.1. Delay zero passes the current band signal; feedback
uses the preceding output sample to avoid an algebraic loop. Delayed feedback
uses the same selected tap as the audible echo. All feedback writes have the
same ±64 boundary. Gains and bandwidth are smoothed in signal/coefficient
space, independently of block size. Tempo has no effect on this processor.

Nominal crossover `i` is `20 * 1000^((i+1)/16)` Hz, `i=0..14`. Bandwidth scales
log distance from the geometric spectrum center; cutoffs are capped at 0.49
of the actual rate with the shared SVF math's positive normalized floor. At
low rates coincident cutoffs yield zero-width bands; indices/count never change.
First and last bands extend to DC and Nyquist to retain all input energy.

Let `L_i(z) = c_i(1+z^-1)/(1-(1-2c_i)z^-1)`, with
`c_i = tan(pi*f_i/rate)/(1+tan(pi*f_i/rate))`. Gentle uses `C_i=L_i`; steep uses
`C_i=L_i^2`. Both have frequency-dependent phase. The bands are `B_0=C_0`,
`B_i=C_i-C_(i-1)` for 1–14, `B_15=1-C_14`. Thus `sum(B_i)=1` by telescoping,
including during cutoff and splitter interpolation. This is not sixteen
unrelated bandpasses and not an assertion of linear phase. The neutral setting
(all delays zero, feedback zero, levels one, pans center) reconstructs the
original normalized audio samples within floating point summation error in
the linear headroom region. The explicit ±64 internal bounds take priority
for extreme signals. Equal delays produce
a correspondingly delayed unity signal. Independent tests use analytic complex
transfer functions and impulse sums rather than the production splitter.

PDC is zero; per-band delay is intentional. Warm-up/readiness is the longest
active/pending tap. Tail and gap include crossover decay, tap transitions and
old history. Nonzero feedback has an unlimited conservative tail. Reset clears
all crossover, last-output and valid-history state without touching heap owners.

## Storage and API

Sample rates: finite 1–384000 Hz are supported; finite out-of-range rates clamp;
NaN/Inf fall back to 48000 Hz. `actual_sample_rate()` exposes this policy.
Constructors hold empty rings; only preparation allocates histories, replacing
and releasing all old histories on the control side. Exact-length ring storage
is `ceil(seconds * actual_rate)+1` f32 values per channel; no power-of-two padding
or uncharged tap vectors. Reset invalidates by counters in constant time.
`prepared_bytes()` includes the entire inline processor and heap histories;
allocator bookkeeping is excluded. `preparation_bytes(rate)` remains a final
payload estimate for compatibility; an estimate alone does not admit memory or
guarantee allocation. `preparation_requirements(rate)` and `try_prepare` supply
checked sizing, peak accounting and observable refusal. Preparing at a lower
rate replaces every ring instead of retaining high-rate capacity.
No process/reset/set_params/set_tempo allocation, reallocation, free, lock, wait
or IO; no heap clones. One caller-owned block at a time. Dropping is control-side.

Public modules: `echo_bank::{EchoBank, EchoBankParams, EchoUnitParams,
EchoFilterParams, EchoFilterMode, EchoFeedbackMode}` and
`frequency_delay::{FrequencyDelay, FrequencyDelayParams, FrequencyBandParams,
FrequencySplit}`. Both implement `Effect`, all complete settings implement
`ParamSet`, all persisted structs/enums derive serde and TS without autoexport.
Descriptor order is explicit in source and tested against nested JSON paths.

Both processor module paths also expose the same owned preparation types:
`PreparationBudget`, `PreparationRequirements`, `PreparationError`,
`PreparationRefusal`, `PreparationStatus`. The processor methods are:

```rust
fn preparation_requirements(&self, sample_rate: f32)
    -> Result<PreparationRequirements, PreparationError>;
fn try_prepare(&mut self, sample_rate: f32, max_block: usize,
    budget: PreparationBudget) -> Result<(), PreparationError>;
fn preparation_status(&self) -> PreparationStatus;
```

`PreparationRequirements` holds the sanitized `sample_rate`, `retained_bytes`
and `peak_bytes`. `PreparationBudget` has independent `retained_bytes` and
`peak_bytes` limits. Retained payload includes the inline processor and new
history capacities. Peak adds all existing history capacities and the fixed
staging headers (640 bytes for sixteen histories, 1280 for thirty-two on this
64-bit target). Ordinary call-stack temporaries and allocator bookkeeping are
excluded; hosts must reserve their own overhead/headroom. Every size addition,
multiplication and each Vec's `isize` pointer-offset limit is checked before
reservation. `try_reserve_exact` allocates empty vectors fallibly, then resize
initializes within reserved capacity; there is no `vec![0; capacity]` abort path.
Actual capacities are checked against admission limits before publishing.

All sixteen/thirty-two histories stage before any live sample rate, parameter,
filter, ramp, tap, history or clock mutation. On success, histories swap and
fixed inline preparation/reset applies the existing sanitized parameter target
and tempo. Old histories retire on the calling control thread. On failure,
partial staged histories retire there and the previous prepared processor
remains usable with exactly the same audio state. The only live change on
refusal is its copyable status latch; no time, rate, band or unit is shortened.

Errors are `CapacityOverflow { maximum_delay_samples, history_count }`,
`RetainedBudgetExceeded { required_bytes, budget_bytes }`,
`PeakBudgetExceeded { required_bytes, budget_bytes }`, or
`ReservationFailed { history_index, requested_bytes }`. The latter identifies
`2 * unit_or_band + channel`, left then right. Retained-budget refusal takes
precedence if both limits fail. Errors implement Display/Error and own no heap
data. `PreparationStatus` exposes `is_prepared` and `last_refusal`, an optional
`PreparationRefusal { sample_rate, error }` recording the sanitized requested
clock that was refused. `actual_sample_rate()` continues to report the old
clock. A first refusal leaves `is_prepared=false` and the existing sanitized
passthrough behavior, rather than installing a different clock. Reset, no-ops,
setters and processing preserve the latch; only successful preparation clears
it. `preparation_requirements` is a read-only query and does not latch errors.

The existing `Effect::prepare` signature is unchanged and delegates to
`try_prepare` with `PreparationBudget::UNLIMITED`. It too retains prior state
and latches any refusal. Legacy callers must check `preparation_status()` off
thread before treating a preparation as successful. Concrete host integration
must use `try_prepare` with actual admitted limits, not the unlimited wrapper.
`max_block` retains the Effect argument shape; these frame-based processors do
not allocate block-sized scratch. None of these preparation methods belongs in
an audio callback. Static `preparation_bytes` saturates to `usize::MAX` if its
checked calculation cannot be represented; the fallible API reports the error.
Preparation requires exclusive ownership of a detached/stopped processor.
Hosts prepare a candidate off thread while the distinct installed plan remains
active; this API does not permit racing preparation against a callback.

EchoBank has 220 descriptors: four global controls followed by 27 per unit.
Unit offset is `4 + 27*i`: enabled, inputGain, inputPan, sync, timeMs, division,
stereoOffsetMs, feedback, feedbackMode, feedbackPan, separation, six input-filter
fields (mode/frequencyHz/q/gainDb/gain/sections), the same six feedback-filter
fields, filterPost, outputGain, outputPan, nextSend. The last nextSend is a
zero-span reserved field, forcibly sanitized to zero, with no ninth destination.
All outputGain and nextSend descriptors use `None` for their signed coefficients;
their indices, ranges, defaults, smoothing and JSON are unchanged. Input and
filter gain descriptors remain `Gain`. Read-only source tracing of
`apps/desktop/src/features/params/format.ts` and
`apps/desktop/src/components/audio/units.ts` establishes why this matters:
`gain` formats through `20*log10(value)` (nonpositive becomes `−∞ dB`) and parses
through `10^(dB/20)` (`"-1"` becomes positive 0.891251, displayed infinity becomes
zero). `none` instead uses the signed plain number and parseNumber paths:
−1 displays as `−1.00` and parses as −1. This is source behavior, not an executed
editor acceptance claim. Native regressions check all signed metadata fields,
stable indices/JSON and independently timed negative output/send impulses.
FrequencyDelay has 71 descriptors: dry, wet, feedback, scale, shortRange,
bandwidth, split, then delayMs/level/pan/enabled at `7 + 4*i` for bands 0–15.
Missing objects/fields use serde defaults. Parameter setters also sanitize
directly constructed malformed values; empty calls and duplicate targets keep
the same clocks. `params()` returns the sanitized target, not the current ramp.

Next requested window: append two kinds/variants and dispatch arms in `effect.rs`
(no existing reorder), parent-generated TS/descriptors/WASM, project old-v1
default/command/automation/undo tests, engine same realtime/offline/slot/PDC and
bounded-plan preparation tests, then purpose-built eight-unit links editor and
sixteen-band drawn delay/level/pan editor. A large generic parameter list does
not close either row. All other E3 delays/modulation/reverbs remain in scope.

The current host sums tails with ordinary `usize::sum` in
`windfall-engine/src/state.rs` around line 1235. Before admitting either kind,
the serialized engine window must use saturating aggregation for unlimited
tails and test multiple sustaining effects plus finite render horizons. This
file is closed to E3; no host change is included here. Consider control-side
boxed variants for the two new `AnyEffect` arms so the 35 KiB EchoBank state
does not enlarge every existing effect slot. All owner retirement remains
off callback under the utility repair contract.

The proposed host contract reserves retained and peak requirements against the
project/rack budget, then invokes the concrete `try_prepare` seam off thread.
Only an `Ok(())` instance may be installed in audio. A typed refusal releases
the reservation and candidate off thread, reports required/budget or failing
history bytes to the user, and retains the current plan. Charge other live/
retiring plan owners separately: an empty candidate's peak includes its own
staging, not a different rack's old processor. Budget admission cannot replace
checking the actual allocation result. Hosts must neither shorten times nor
clamp a requested device rate to save memory. The new API is per-instance;
project-wide admission, installation and retirement remain integration work.
At 384 kHz ten banks still exceed 6 GB; replacing a live bank at that rate
temporarily needs approximately 1.229 GB of its own payload.

## Original checkpoint evidence (immutable 6326f883)

Windows/MSVC, 2026-10-08, isolated task target, one Cargo job and one Rust test
thread. No workspace, desktop, Tauri or UI build was run. All TS export tests
wrote to the task's `target/e3-bindings`, never app artifacts.

Payload bytes at rates 1/8000/48000/384000 Hz:

- EchoBank instance: 37168 / 12835568 / 76835568 / 614435568 bytes.
- One stereo unit's history: 208 / 1600008 / 9600008 / 76800008 bytes.
- FrequencyDelay instance: 4448 / 1028320 / 6148320 / 49156320 bytes.
- One stereo band's history: 16 / 64008 / 384008 / 3072008 bytes.

The instance figures include 35504 inline bytes for EchoBank and 4192 for
FrequencyDelay. Reprepare tests verify the pre-admission estimate equals all
retained payload and that a smaller rate releases the larger arrays.

The ignored release observation was explicitly executed. Each measurement
processed 16384 stereo frames in 64-frame blocks after priming 1/2-ms taps.
All eight bank units use three-section resonant input and feedback filters,
unity feedback and next sends; all sixteen frequency bands use the steep
splitter and unity feedback. Automation changes every unit/band time between
valid taps, bank cutoff/Q, bandwidth and tempo every 64 frames. Constant vs
automated elapsed microseconds on this run:

- 8000 Hz: EchoBank 37142 / 38972; FrequencyDelay 2505 / 3184.
- 48000 Hz: EchoBank 37700 / 41623; FrequencyDelay 2489 / 3375.
- 384000 Hz: EchoBank 39579 / 39585; FrequencyDelay 4091 / 3662.

This is a single shared-machine timing observation, not a device guarantee or
a deadline test. Cache/OS/competing jobs affect it. Bank reads are bounded at
16 settled / 32 fading history reads per frame, with 576 continuously running
SVFs. Frequency reads are bounded at 32 settled / 64 fading reads with 60
one-pole updates. Reset and setters traverse fixed inline control/filter state,
never history length. No callback sample coefficient tables or tap vectors exist.

Commands (Git Bash, sourced MSVC environment):

```bash
source scripts/msvc-env.sh
export CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1
export CARGO_TARGET_DIR=target/e3-native
export TS_RS_EXPORT_DIR="$(pwd)/target/e3-bindings"
cargo test -p windfall-dsp --lib
cargo clippy -p windfall-dsp --lib --test delay_family -- -D warnings
cargo test -p windfall-dsp --release --test delay_family -- --include-ignored --nocapture
rustfmt --edition 2024 --check crates/windfall-dsp/src/echo_bank.rs crates/windfall-dsp/src/frequency_delay.rs crates/windfall-dsp/tests/delay_family.rs
git diff --check
```

128 library tests and 25 release delay-family tests pass (including the one
normally ignored CPU observation); strict target Clippy and owned formatting
pass. Full-duration bank 24-second plus 1-second-offset impulses and frequency
1000-ms impulses execute at all four rates, not just scaled-down times. Tests
also cover all filter modes/orders and domain endpoints, independent analytic
bank lowpass/feedback equations, sixteen individual impulse and DFT transfers,
phase, level/pan, parallel and signed serial routing, unity during bandwidth
and splitter edits, feedback including zero-time causality, extreme nonfinite
signals, block partition, repeated targets/empty calls/reset, stale-history
invalidation, live band deselection, pending-tap readiness, old taps and
conservative tail/feedback crossings. All guarded process/setter/reset/tempo
paths make zero alloc/realloc/free calls. Finite-tail sample exhaustion is
directly tested at 1 Hz; the high-rate finite filter horizons use conservative
pole-domain bounds, not multi-minute exhaustion measurements at each rate.

Actual slot bypass/dormant restoration, native/live versus offline host paths,
project history/serde migration, generated inventory, drawn editors, device
deadlines and listening remain outside this first source window. No acoustic
equivalence or full parity completion is claimed.

## R1 repair evidence (incremental child of 6326f883)

2026-10-08, same Windows/MSVC task target and single job/test thread. Before
the production edits, the new signed-descriptor regression failed with
`Gain != None` at units.0.outputGain. The new fallible-API fixture then failed
to compile against 6326 (`E0422`/`E0599`, missing types/methods). This safe RED
step did not invoke the old infallible vector abort path or attempt OS OOM.

Four private native tests exercise actual staging and reservation refusal.
The test-only allocator returns null once for the selected valid, fallible
history reservation; std's real `try_reserve_exact` reports the allocation
error. The hook is scoped to that reservation, thread-local and absent from
production. Every history position (sixteen bank, thirty-two frequency) is
refused in turn. Tests account for each earlier staged allocation and its
retirement, verify retained live bytes/clock/params/readiness/tail unchanged,
and compare subsequent stereo samples bit-for-bit with an independently
prepared, untouched processor while filter, tempo, gain and tap edits run.
Legacy prepare is tested after partial staging, as are first refusal,
unprepared passthrough, latch survival through reset and successful recovery.
Private checked-size fixtures cover sample-count/byte-count/aggregate/header/
retained/peak overflow without large reservations. Public tests reject budgets
one byte below either limit, compare audio after refusal, then admit the exact
limits at full 1/8000/48000/384000 Hz and reprepare down to 1 Hz. Callback
allocator guards remain active after refusal and successful preparation.

The repair adds 32 inline bytes to each processor for its copyable refusal
latch; all history counts, lengths and per-unit/per-band figures above remain
unchanged. Current retained payload at 1/8000/48000/384000 Hz is:

- EchoBank: 37200 / 12835600 / 76835600 / 614435600 bytes (inline 35536).
- FrequencyDelay: 4480 / 1028352 / 6148352 / 49156352 bytes (inline 4224).

At 384000 Hz this is still 614.4 MB / 49.2 MB, without shortening any delay.
Initial preparation peak is 614436240 / 49157632 bytes. Replacing a live
instance at the same rate peaks at 1228836304 / 98309760 bytes. At all four
rates, same-rate replacement peaks reported by checked payload accounting are:

- EchoBank: 39504 / 25636304 / 153636304 / 1228836304 bytes.
- FrequencyDelay: 6016 / 2053760 / 12293760 / 98309760 bytes.

132 library tests and 26 release integration tests pass; the original CPU
observation is the sole ignored test in this repair run. All twenty-four
original signal/allocator cases run again, including full 25-second bank and
1000-ms frequency impulses at all four rates. Strict DSP Clippy, owned
formatting and diff checks pass. The earlier CPU observation and its counts
remain attributed to immutable 6326, and were not rerun solely to update the
count. DSP callback equations and frame clocks were not changed by this
repair; the P3 positional-control naming judgement remains a suggestion.

Repair validation uses the commands above except that the release invocation
omits `--include-ignored`. `cargo clippy -p windfall-dsp --lib --tests -- -D warnings`
also checks the private test-only allocator/rollback code. The new preparation
cases are part of the library run. Foundation R2 review, fallible host admission,
saturating engine tail aggregation, registry/project integration, drawn editors and all other
remaining E3 work remain pending; this repair does not close parity.

## Host integration and editors (2026-10-10, source only)

The four gates above are addressed in source; none of it has been compiled
or run. `docs/qa/2026-10-10-fx-delay-mod-reverb.md` has the files, the tests
and the commands.

- Tails: `windfall-engine/src/effect_host.rs` adds tails and horizons with
  saturating arithmetic, and `PlanState::settled` uses it. An endless tail
  is never counted out; the track is done when it has been quiet for the
  summed gap.
- Admission: a plan reserves the history of every echo bank and frequency
  delay it runs against 1 GiB before preparing any. Each new one is then
  prepared through `EffectSlot::try_prepare` within its own share, and a
  refusal fails the plan build on the control side with a typed error.
  The boxed `AnyEffect` variants already keep the bank's inline state out
  of every other slot.
- Editors: `features/effects/echo-bank` is the linked eight-unit editor and
  `features/effects/frequency-delay` the drawn sixteen-band one.

The bank's reported gap is still its whole conservative horizon, about 200
seconds, so an automatic render tail through a bank runs for as long as the
user allows. Oversampling, granulation, the linear-phase splitter, curve
snapshots and the other controls listed under "Public references and
scope" remain outside what is implemented.
