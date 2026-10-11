# Modulation effects

The baseline Chorus/Flanger/Phaser contract below remains unchanged. The
distinct Bucket Chorus, Swarm Chorus, Stage Phaser and Stacked Flanger circuits
and their 2026-10-11 model/count/tempo additions are specified in
[E3-CIRCUITS.md](E3-CIRCUITS.md). Their authored acceptance tests have not been
executed by the edit-only worker.

Windfall's **Chorus**, **Flanger** and **Phaser** are original stereo processors.
Add them from a mixer track's Add effect menu. Every control uses the ordinary
parameter editor: drag or type a value, double-click to reset, and use its context
menu to create automation. Edits, replacement, copy, bypass and removal use the
shared document history. Settings and automation are stored in version-one
`.windfall` files. The master track supports the same processors.

## Behavior and controls

Chorus averages **three** delayed copies per input channel. Each tap has its own
free-running sine oscillator, starting at 0, 1/3 and 2/3 of a cycle. Delay is the
minimum, and Depth is the additional sweep: `delay + depth * (1 + sine) / 2`.
Voice 1/2/3 rate edit the three actual oscillators. Stereo phase offsets the right
oscillators without feeding left audio into right. Zero Depth gives a static
delay; zero rate freezes that voice's phase. Cubic interpolation preserves the
tone while continuously moving reads create the intended detuning. There is no
feedback. Default 12 ms delay, 4 ms depth, 0.6/0.8/1.1 Hz rates, quarter-cycle
stereo phase and 50% Mix give an audible stereo chorus.

Flanger mixes dry with one short swept delay per channel. Delay is 0.1–10 ms,
Depth adds 0–10 ms, and Rate is 0–5 Hz. Sine to triangle continuously changes the
oscillator shape. Stereo phase offsets right independently. Feedback is signed
and bounded to ±0.9; Invert wet changes the output polarity independently of
that feedback. Damping is a one-pole lowpass on both the wet and feedback signal,
with pole `0.9 * damping`; its cutoff in Hz therefore follows the sample rate.
At zero damping the filter is transparent. The default uses 1 ms delay, 3 ms
depth, 0.25 Hz, quarter-cycle stereo phase, +0.5 feedback, 20% damping, sine and
50% Mix. Convex linear interpolation deliberately trades some top-octave
brightness for a strictly bounded feedback loop.

Phaser runs **six first-order allpasses** per channel. Mixing their output with
dry produces three unevenly spaced notches. Sweep minimum/maximum are 20–10000
Hz, interpolated in log frequency by a sine LFO; crossed endpoints are sorted
each frame. At low sample rates the actual corner is capped at 45% of the rate.
Rate is 0–5 Hz, Stereo phase offsets the right sweep, and signed Feedback
(±0.85) recirculates the previous output through an explicit one-frame delay.
The default sweeps 200–2000 Hz at 0.3 Hz, with quarter-cycle stereo phase,
+0.35 feedback and 50% Mix. Six fixed stages are the complete supported stage
configuration; variable or vintage model stages are outside this delivery.

Stereo phase is measured in **cycles**: 0% synchronizes both channels, 25% is
90 degrees, 50% is 180 degrees, and 100% is a full cycle. Each processor's Mix
is linear: 0% is exact dry for ordinary finite input, 100% is wet only. Neutral
Phaser means Mix 0; at Mix 100 its allpass phase still changes the signal even
with zero feedback. Neutral Flanger means Mix 0; zero depth leaves a static comb.
The slot has its own ordinary mix/bypass crossfade. Moving the processor's Mix
to zero keeps its history running; bypass eventually stops and clears history.

## Stable contracts

The original first 15 effect kinds and all their control orders remain intact.
This branch appends `chorus`, `flanger`, `phaser` in that order. Parent composition
must append any other concurrently delivered kinds without changing previous
indices. These JSON field names and zero-based automation indices are permanent:

- Chorus: 0 `delayMs`, 1 `depthMs`, 2 `rate1Hz`, 3 `rate2Hz`, 4 `rate3Hz`,
  5 `stereoPhase`, 6 `mix`.
- Flanger: 0 `delayMs`, 1 `depthMs`, 2 `rateHz`, 3 `stereoPhase`, 4 `feedback`,
  5 `damping`, 6 `shape`, 7 `invertWet`, 8 `mix`.
- Phaser: 0 `minHz`, 1 `maxHz`, 2 `rateHz`, 3 `stereoPhase`, 4 `feedback`, 5 `mix`.

Every struct has serde defaults for omitted fields and uses `type` with the tag
above in `EffectParams`. There is no file version change, vendor code, asset or
preset reuse, proprietary reference binary, or new dependency. Original example
settings in [examples/modulation](examples/modulation) are exact parameter JSON,
usable with `SetEffectParams` or by entering the corresponding controls. They
are authored here and dedicated to CC0; this code remains GPL-3.0-or-later.

## Phase, timing and preparation

These are musical time-varying effects with an undelayed dry component; **PDC
latency is zero**. Wet delay and allpass group delay are intentional sound
design. Compensation must not try to cancel their swept phase. No latency-tap
edit is reported. Delay/depth and stereo-phase edits intentionally move read
positions and can bend pitch; they use a 10 ms frame-clock ramp rather than
crossfading static taps. Rate/feedback/damping/mix/wet polarity also ramp in
10 ms. Phaser endpoints ramp in log space. Oscillators retain phase through
edits. Equal target writes do not restart ramps, and empty blocks neither
consume the initial settings nor advance clocks. Reset clears history, resets
oscillator phases and immediately reinstates current settings. Existing engine
seeks preserve effect history and the free-running modulation clock.

Prepare clamps finite rates to 1–384000 Hz and uses 48000 for nonfinite values.
It reserves the largest legal delay, independent of later edits or block size.
At very low rates Chorus has a two-frame interpolation floor and Flanger a
one-frame causal floor. Unprepared direct processing gives sanitized dry audio.
Inputs are bounded to ±1000 and nonfinite values become zero. Phaser's lattice
state has a ±1000000 exceptional numeric guard. Construction, prepare and
destruction occur off the callback. Processing, indexed parameters, tempo noop,
reset, mix/bypass and queries contain no allocation/free, lock, IO or wait.

Chorus readiness, warmup, tail and maximum possible quiet gap are
`ceil(40 ms * rate).max(2) + 2` frames, including interpolation support. Two
power-of-two delay lines use 16384 bytes at 48 kHz, 131072 at 384 kHz. These are
buffer payloads, separate from the fixed struct and host slot scratch storage.

Flanger readiness/warmup/gap are `H + 7`, where
`H = ceil(20 ms * rate).max(1) + 1`. A linear tap cannot exceed the maximum cell
value. With zero input and state bound M, all line cells are at most 0.9 M after
H frames. Seven further filter frames reduce the total bound by
`q = 0.9 + 0.1 * 0.9^7`. Input ±1000 gives maximum state 10000. Tail is therefore
`(H + 7) * ceil(ln(1e-6 / 10000) / ln(q))`, a conservative absolute 1e-6 bound
under any allowed sweep/feedback/damping, including edits from earlier targets.
Tail bounds refer to zero input after any control ramp has valid bounded
coefficients; all intermediate ramp values satisfy those bounds. Two delay-line
payloads are 8192 bytes at 48 kHz and 65536 at 384 kHz.

Phaser uses normalized lattice scattering:
`y = a*x + b*s; s' = b*x - a*s`, where
`a = (tan(pi*f/rate)-1)/(tan(pi*f/rate)+1)` is capped to ±0.9995 and
`b = sqrt(1-a*a)`. The negative coefficient cap also sets an actual corner
floor of `rate/pi * atan(0.0005/1.9995)` (about 30.56 Hz at 384 kHz); it
only exceeds the 20 Hz parameter minimum at the highest prepared rates.
This preserves energy as the coefficient moves instead of
using an unstable time-varying direct-form recurrence. Six stages per side hold
12 floats and feedback holds two. Two fixed gain ramps add 32 bytes of state
with the current `LinearRamp` layout. It has no preparation heap allocation.
Warmup/readiness are zero. Its explicit finite **idle-tail policy** allows four
seconds of zero input, fades wet output over the next 10 ms, then clears the
filter/feedback state independently for each channel. This is a deliberate tail
cap, not a claimed natural pole-decay bound at every setting. Tail and gap both
include the four seconds and fade; modulation phase continues through silence.
Input at or above 1e-20 resets the idle counter. If it returns during the fade,
wet gain recovers from its current value to one over the next 10 ms (480 frames
at 48 kHz). The old filter/feedback history continues processing throughout;
reentry neither freezes nor clears it. Recovery also applies from zero after
the terminal tail clear. Repeated nonzero input or equal parameter writes do
not restart recovery; empty blocks do not advance it. Recovery continues even
if the resumed input becomes silent again, until the next four-second idle
period starts another fade. Reset/prepare clear history and restore gain one.
Each channel has its own gain clock. Continuous input that has not entered an
idle fade remains at full gain, and Mix zero remains exact dry.

## Evidence and scope

The acoustic suite uses independent f64 cubic impulse weights and delayed-sine
references for Chorus, analytic comb/allpass transfer functions, separately
expanded recurrences, and notch-frequency checks for Flanger/Phaser. Lifecycle
and allocator checks are separate from those references. Project tests use the
real checked Document, command/history/automation ranges and disk save/load.
Engine tests route real stereo audio, drive every automation index, compare
live/offline and both stem modes, and guard callback allocation/free. UI tests
use locally regenerated descriptors/bindings and the actual shared Rust WASM
document for Add effect menu, every control, history, copy/replace, mix/bypass,
automation, removal undo and save/reopen.

The source delivery implements the basic `fx-fruity-chorus`, `fx-fruity-flanger`
and `fx-fruity-phaser` rows under roadmap E3. The final checkpoint records exact
commands, counts and CPU/memory observations below. Throughput measurements are
host CPU observations; they are separate
from audio-device deadline proof or listening approval. Windows device/listening
and macOS/Linux runs remain external gates. Parent owns combined generated
artifacts and parity review. This delivery does not claim Vintage Chorus,
many-voice Chorus, stacked Flanger, Vintage Phaser, wider E3 families or full
feature-row closure.

### Original feature-checkpoint checks (`b2fef7d1`)

Verified thread attachment, clean checkout and branch
`gpt/t3-modulation-effects-e3b` at base
`125d709ccbea137b4d5abf762ee459a2bad15771` before edits. Native Cargo work was serial,
with `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1` and private target
`C:/Users/ewhee/.t3/worktrees/windfall/gpt-t3-modulation-effects-e3b/target/modulation`.
Commands below ran after sourcing `scripts/msvc-env.sh` in Git Bash.
The WASM build script used this same worktree's private `target/sim` directory,
also with one Cargo job and no concurrent Cargo process.

| Command | Result |
| --- | --- |
| `cargo clippy -p windfall-dsp -p windfall-project -p windfall-engine --all-targets -- -D warnings` | Passed |
| `cargo test -p windfall-dsp --test dsp -- --test-threads=1` | 163 passed; 5 opt-in tests ignored |
| `cargo test -p windfall-dsp --test filter_family_registry -- --test-threads=1` | 6 passed |
| `cargo test -p windfall-project --test commands --test file --test modulation -- --test-threads=1` | 136 + 33 + 4 passed |
| `cargo test -p windfall-engine --test engine modulation -- --test-threads=1` | 5 passed |
| `cargo test -p windfall-engine --test engine realtime -- --test-threads=1` | 8 passed |
| `cargo test -p windfall-dsp --release --test dsp modulation_observations -- --ignored --test-threads=1 --nocapture` | 1 passed; observations below |
| `WINDFALL_DSP_RENDER_DIR="$PWD/target/modulation-listening" cargo test -p windfall-dsp --release --test dsp modulation_render_examples -- --ignored --test-threads=1 --nocapture` | 1 passed; four authored dry/wet WAVs |
| `bash scripts/gen-bindings.sh` then `bash scripts/build-sim.sh` | Passed; actual shared-WASM build used below |

Desktop checks used existing project-root `node_modules` read-only, without an
install. A temporary `.modulation-vitest.config.ts` extended the repository's
Vitest config with only `cacheDir` pointing at private
`target/modulation-ui-cache`; it is excluded from the checkpoint.

```powershell
# From apps/desktop, after the local generation above:
node node_modules/vitest/vitest.mjs run src/features/effects src/features/params src/features/mixer/effects.test.tsx --config .modulation-vitest.config.ts --configLoader native --maxWorkers=1
node node_modules/typescript/bin/tsc -p tsconfig.app.json --incremental --tsBuildInfoFile ../../target/modulation-ui-app.tsbuildinfo
node node_modules/typescript/bin/tsc -p tsconfig.node.json --incremental --tsBuildInfoFile ../../target/modulation-ui-node.tsbuildinfo
node node_modules/eslint/bin/eslint.js src/features/effects/modulation.test.tsx src/features/params/access.test.ts src/features/params/format.ts src/features/params/format.test.ts --max-warnings 0
```

**327 UI tests in 13 files passed**, including 13 modulation tests through the
real Rust WASM document. Both TypeScript checks and scoped ESLint passed.
The actual desktop app was also exercised in T3's browser against this WASM:
all three menu entries added real master-track slots with all 22 controls;
Flanger feedback reached +90%, Undo restored +50%, and Invert wet toggled on.
There were no displayed error alerts. A short-delay readout rounding fix keeps
whole milliseconds stable when retyped. Signed feedback uses percentages,
including its finite extrema. Static tone-reference checks observed maximum
absolute gain error `0.000015637` across the tested feedback/polarity cases.

### Original host work and memory observations (`b2fef7d1`)

Windows, Intel Core i9-14900F (32 logical processors), Rust
`1.99.0 (b940084d7 2026-09-28)`, optimized release test executable, one test thread.
Each processor ran 480000 stereo frames at 48000 Hz in 1875 blocks of 256,
using its audible defaults and constant +0.1/-0.2 channel inputs. An allocator
counter measured newly allocated buffer payload during prepare, excluding
construction, fixed structs, host scratch and existing allocations.

| Effect | Prepare bytes at 48 / 384 kHz | Sum of elapsed DSP calls | Throughput | Maximum observed call |
| --- | ---: | ---: | ---: | ---: |
| Chorus | 16384 / 131072 | 0.0410 s | 243.8× realtime | 0.197 ms |
| Flanger | 8192 / 65536 | 0.0154 s | 648.1× realtime | 0.154 ms |
| Phaser | 0 / 0 | 0.0524 s | 190.7× realtime | 0.197 ms |

Elapsed calls use a wall-clock timer as a CPU-work observation, not a process
CPU-time counter, device deadline guarantee or scheduling stress test. These
are one-run host observations, not enforced timing thresholds. Separate
allocator guards observed zero callback allocation/reallocation/free during
processing, live edits, reset, slot transitions, plan replacement and automation.
Code inspection found no locks, IO or waits in the new callback paths.
The four six-second WAVs in ignored `target/modulation-listening` are generated
from an original harmonic chord and silence; rendering is not listening approval.
The negative-feedback Flanger example uses ordinary positive wet delays; it
does not claim through-zero flanging.

### Parent composition

The checkpoint diff provides the exact mechanical registry hunks in
`crates/windfall-dsp/src/effect.rs`: imports; the three appended variants and
`ALL` entries; names, descriptors and defaults; parameter/effect dispatch macros;
kind/sanitization matches; construction/set-params arms; and zero-latency arms.
`src/lib.rs` adds the module and six public reexports. When composing another
owner's appended kinds, increase `ALL` by three from that branch's current
length and retain every existing variant's order. The first 15 discriminants
and serialized tags are tested unchanged; the modulation order is tested among
the new suffix so other deliveries may append too.

The remaining mechanical consumers are test-module registrations, random-params
and engine realtime match arms, the DSP expected-name list, the appendable
filter-family registry assertion, and the TypeScript exhaustive kind map.
No Plan/Rack/State/controller/Processor/Session/runtime/analyzer implementation,
dependency, version or lockfile changed. The existing descriptor-driven mixer
menu and parameter editor expose these distinct controls without another UI
implementation. Parent must regenerate combined bindings/descriptors, automation
fixtures and WASM after registry composition; locally generated artifacts are
excluded from this source checkpoint. Parent also owns parity decisions,
private push and independent Standards/Spec reviews.

### Spec P2 repair: tail-fade reentry

Independent Standards reported zero actionable violations. Independent Spec
identified one P2: resumed input reset Phaser's idle counter and restored wet
gain immediately while the old allpass history remained audible. The original
feature checkpoint `b2fef7d1d16d25920ff8945726bc5c1b9f79ec9e` is preserved.

Before changing production source, the actual committed processor was executed
at 48 kHz, endpoints 20 Hz, rate zero, feedback +0.85, Mix one: DC 1 for 48000
frames, silence for 192240 frames, then DC 1e-6. The added regression failed:

| Executed source | Last quiet output | First resumed output | One-frame jump |
| --- | ---: | ---: | ---: |
| Original `b2fef7d1` | 0.0032145930 | 0.0064299246 | 0.0032153316 |
| Repaired recovery | 0.0032145930 | 0.0032216602 | 0.0000070671 |

The repair retains the existing idle fade and live allpass/feedback processing.
Two per-channel frame-clock ramps recover from current wet gain over 10 ms;
the behavior and reset/noop policy are specified above. No parameter, kind,
JSON, automation-index, latency or tail-length contract changes. Related slot
code already recovers wet gain with `LinearRamp`; Chorus and Flanger have no
corresponding idle-tail gain reset. No shared-tail implementation changed.

Three new regressions cover the reported half-fade reentry, delayed right-channel
reentry, single-frame versus irregular partitions, equal parameter/tempo writes,
empty calls, simultaneous Mix edits, reset during recovery, exact dry Mix and
reentry after terminal state clearing. Allocator guards cover the actual
recovery callbacks and reset, observing zero allocation/reallocation/free.

An independent f64 direct-form recurrence predicts the retained tail's evolution
and a separate fresh-input response. The residual history is normalized at the
last quiet frame to separate accumulated five-second f32 history rounding from
the gain-envelope discontinuity; the new-input response remains unscaled. The
history normalization is bounded within 1% of the nominal reference, and the
known old tail amplitude is asserted. Maximum recovery prediction error was
`1.418e-7`, below the new `5e-7` bound. The original jump limit stays `1e-4`,
and every pre-existing assertion is unchanged. Freezing or prematurely clearing
the old history cannot satisfy the evolving-tail prediction.

Repair checks were local, with the same serial Cargo/private-target policy:

| Command | Result |
| --- | --- |
| `cargo test -p windfall-dsp --test dsp phaser_tail -- --test-threads=1 --nocapture` | 3 passed |
| `cargo test -p windfall-dsp --test dsp -- --test-threads=1` | 166 passed; 5 opt-in tests ignored |
| `cargo clippy -p windfall-dsp --all-targets -- -D warnings` | Passed |
| `cargo test -p windfall-engine --test engine modulation -- --test-threads=1` | 5 passed; all three effects' routing/automation/live/offline/stems/RT assertions retained |
| `CARGO_TARGET_DIR="$PWD/target/sim" cargo check -p windfall-dsp --release --target wasm32-unknown-unknown` | Passed |

Only `src/modulation/phaser.rs`, `tests/dsp/modulation.rs` and this document change
in the incremental repair. UI/project/contracts and generated artifacts are
unchanged; their original checkpoint evidence remains above. No GitHub Actions
were dispatched or rerun, and no push/import/nested agent was used. Parent owns
the bounded fresh Spec followup and combined generation. External platform runs
await future user authorization and available minutes.

## Public behavior sources

Only behavioral descriptions were consulted, on 2026-10-08:

- [Chorus manual](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/Fruity%20Chorus.htm):
  detuned delayed copies, separate rates, delay/depth and stereo phase.
- [Flanger manual](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/Fruity%20Flanger.htm):
  swept delay, feedback, damping, phase and polarity.
- [Phaser manual](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/Fruity%20Phaser.htm):
  swept frequency cancellation and allpass-stage/feedback/dry-wet behavior.

These guide the three basic row summaries in `WINDFALL_PLAN.md`'s parity matrix
and roadmap E3. They do not specify a proprietary implementation or imply
sample-identical compatibility.
