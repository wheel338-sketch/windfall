# Spatial DSP integration seam

2026-10-11 continuation: the registered E3 processors now expose actual bucket
modes, selectable chorus voices, phaser stages/stagger and flanger order, with
tempo controls and dedicated editors. [E3-CIRCUITS.md](../../E3-CIRCUITS.md)
supersedes the earlier model/count coverage and tempo/warmup limits below.
Inverted Delay and delay-bank host/editor gates are also implemented in source.
The new tests are written, not executed; the old verification section records
only the earlier circuit checkpoint, not these changes.

Implemented in `crates/windfall-dsp/src/spatial/`, exported through the existing
`pub mod spatial`. Only that directory and this document were edited for this
assignment. These processors compile and implement the real in-place `Effect`
interface. Registry, effect union, engine, persistence commands, editors,
generated bindings and parity status are integration-owner work.

## Finished types and proposed registry names

Every processor has a corresponding `*Params` type, `Default`, stable
`ParamSet` descriptors, sanitized Copy parameters, serde camelCase with legacy
missing-field defaults, and `ts_rs::TS`. Suggested `EffectKind` variant names
match the processor names below; suggested serialized tags are camelCase.

| Processor / proposed EffectKind | Product name | Parity reference | Circuit and limits |
| --- | --- | --- | --- |
| `VintageChorus` | Vintage Chorus | `fx-vintage-chorus` | Four independently detuned voices per channel (existing Chorus has three), staggered delays, stereo phase spread, wet-path one-pole tone filter. Delay 1–25 ms, depth 0–12 ms, base rate 0–5 Hz; voice rate ratios 0.67, 0.843, 1.016, 1.189. |
| `HyperChorus` | Hyper Chorus | `fx-hyper-chorus` | Eight independently detuned voices per channel; ratios 0.67 + 0.173 × voice index, phase offsets, alternating source-channel crossfeed, wet tone filtering. Same delay/depth bounds; brighter, deeper defaults. This is a new voice engine, not a wrapper around existing Chorus. |
| `VintagePhaser` | Vintage Phaser | `fx-vintage-phaser` | Twelve staggered lossless lattice allpasses per channel (existing Phaser has six), logarithmic sweep, stereo phase offset, one-pole feedback color filter. Signed feedback bounded to ±0.8. |
| `StackedFlanger` | Stacked Flanger | `fx-fruity-flangus` | Two swept feedback combs in series per channel, with independent rates and starting phases; second comb is slightly shorter. Each stage blends dry/delayed audio and has tone-filtered feedback bounded to ±0.75. Existing Flanger is one comb. |
| `BandDelay` | Band Delay | `fx-multiband-delay` (partial compact alternative) | Two cumulative two-pole lowpasses form three complementary bands: low, upper-low, input-upper. Independent 1–1000 ms stereo delays, levels, balance pans and feedback per band. Wet bands recombine; equal delays/levels with no pan/feedback reconstruct the delayed input. Feedback bounded to ±0.85 and lowpassed at 7 kHz. |
| `Room` | Room | `fx-fruity-reeverb` | Small Schroeder room, four signed early reflections, four independent damped combs per channel, two short output allpasses. Existing Reverb is a sixteen-line Hadamard FDN. Nominal RT60 0.1–1.5 s (default 0.45), pre-delay 0–30 ms; different early/loop/diffuser constants. Loop gain capped at 0.94. |
| `Spreader` | Spreader | `fx-spreader` | Mid/side width plus 0–30 ms Haas decorrelation injected as equal-and-opposite side signals. At ordinary levels, mono fold-down preserves the original mid, even through control edits. Zero Haas delay removes its decorrelation contribution. |
| `StereoEnhancer` | Stereo Enhancer | `fx-fruity-stereo-enhancer` (partial) | Width, channel balance, and variable mono-bass lock: two cascaded side-channel highpasses suppress bass in S while preserving M. At full lock DC is mono and low sine content is much more mono than high sine content. Existing StereoMatrix has constant coefficients/channel delays and cannot perform this frequency-dependent lock. No phase-offset control is implemented here. |

The vintage processors are original Windfall circuits inspired by the requested
behavior. They do **not** claim a measured model of a specific analog synth or
pedal. Public model specifications, frequency-response/reference measurements,
listening checks and integrated acceptance remain before closing those parity
rows. BandDelay does not fulfill the sixteen-independent-band parity summary;
that implementation already belongs to `FrequencyDelay`. StereoEnhancer also
does not close all behaviors in its width/phase/balance parity summary.

## Explicit overlap skips

**DelayBank / `fx-fruity-delay-bank`: skipped; no proven DSP gap.** Read
`echo_bank.rs`, `echo_bank/params.rs` and the processing/routing implementation.
`EchoBank` already has eight independently timed stereo units with input/output
gain and pan, per-unit filters, feedback filtering, tempo sync and `next_send`
to the next unit. Its process loop injects the dry source into every unit
(parallel), and passes each unit's delayed next-send into the following unit
(serial). Four requested taps are a subset: enable units 0–3 and disable the
remaining units; set input gains of downstream units to zero for a pure serial
chain, or next-send to zero for pure parallel echoes. A copied four-unit type
would add no circuit behavior. Suggested integration: reuse existing
`EchoBank` / `EchoBankParams`, with a four-unit preset/editor view if desired.
The spatial tests exercise four enabled units with simultaneous injection and
serial sends, verifying exact partition invariance including time/pan edits.
EchoBank's own feedback-tail policy may report `usize::MAX`; spatial does not
change that existing module or claim its parity acceptance.

**TempoEcho / `fx-fruity-delay-2`, `fx-fruity-delay`: skipped under the brief's
existing-coverage rule.** `DelayParams` already exposes `sync`, `NoteDivision`,
stereo offset, feedback, ping-pong mode and low/high-cut filters. `Delay::set_tempo`
updates synced times; its processing loop swaps echo feedback and uses mono
injection for ping-pong. All requested TempoEcho circuit behavior is present.
Suggested integration: reuse `Delay` / `DelayParams`, or expose a simpler
control view. The `fx-fruity-delay` summary also mentions inverted stereo mode;
existing `DelayMode` only has Stereo and PingPong, so that parity-specific gap
remains explicitly open, outside this skip. No TempoEcho type or alias was added.

## Realtime, timing and tail contract

- All eight new effects report **zero PDC latency**, for every parameter set.
  Their echoes, Haas offset, reflections and phase response are intentional
  audible effects, not common transport latency. Dry paths stay immediate.
- Construction and `prepare` may allocate. `process`, `set_params`, `set_tempo`
  and `reset` never allocate/reallocate/free, lock, wait or perform IO.
  Reset and silence expiry clear existing fixed buffers in place.
- Prepared sample rates are sanitized to 1–384000 Hz (invalid → 48000).
  Unprepared effects pass through finite guarded input. Input NaN/infinity
  becomes zero; pathological input is limited to ±1000 and internal/output
  values to ±1e6. Subnormal states are flushed. Ordinary audio is linear except
  for the intentional feedback/color/mixing circuits.
- Continuous controls use 10 ms per-sample linear ramps. Writes before the
  first processed frame after prepare/reset snap immediately. Identical target
  writes do not restart ramps. Log-frequency controls ramp in log space.
  Milliseconds-to-samples conversion uses f64 arithmetic before the f32 tap.
- Modulation phases and all control/tail clocks advance per frame, including
  zero-input frames. Delay lines are allocated to fixed maxima and reads clamp
  to their capacities. Cubic interpolation is used only on feed-forward chorus
  voices; feedback delays use convex linear interpolation. Live time edits
  glide and intentionally bend pitch; no pitch-preserving time crossfade is
  claimed. Nothing depends on max_block or the host's block boundaries.
- No new processor offers tempo-sync controls; their modulation rates are Hz
  and times are milliseconds. Their inherited `set_tempo` is a no-op. Tempo
  sync is supplied by the explicitly reused existing Delay/EchoBank processors.

Recursive processors have an explicit maximum silence lifetime followed by a
10 ms fade and exact state clear. New input during that fade recovers smoothly.
This deliberately conservative policy makes `tail_samples` a true finite bound
even through parameter automation, rather than pretending an IIR stops exactly
at RT60. For rate `r`, the reported tail is `ceil(hold_seconds * r) +
ceil(0.010 * r)` (minimum fade one frame). Remaining delay history is included.

| Processor | Silence hold / exact reported tail | Warm-up and delay readiness | Reported silent gap |
| --- | --- | --- | --- |
| VintageChorus, HyperChorus | 0.3 s + 10 ms fade | ceil(64 ms × r) + 2 frames | Same as readiness |
| VintagePhaser | 3 s + 10 ms fade | 0 | Entire tail (conservative) |
| StackedFlanger | 2 s + 10 ms fade | ceil(32 ms × r) | Entire tail (conservative) |
| BandDelay | 90 s + 10 ms fade | ceil(1000 ms × r) | Entire tail (conservative) |
| Room | 5 s + 10 ms fade | ceil(100 ms × r) | Entire tail (conservative) |
| Spreader | No recursive state: ceil(30 ms × r) + 1 frame | Same as tail | Same as tail |
| StereoEnhancer | 0.5 s + 10 ms fade | 0 | Entire tail (conservative) |

BandDelay's 90-second safety lifetime covers the longest permitted 1-second
echo with ±0.85 feedback far beyond its nominal 60 dB decay; usual settings are
much shorter. Room's audible RT60 is at most 1.5 seconds, substantially smaller
than existing Reverb's 20-second limit, with a 5.01-second hard safety lifetime.
The silence timers run from sanitized stereo input, not wet-output thresholds.

## Verification completed

`rustfmt --edition 2024` was run only on spatial files. Library compilation
passed with `cargo build -p windfall-dsp --lib`. After a transient sibling
`multiband/tests.rs` absence was resolved by its owner, the scoped command
`cargo test -p windfall-dsp --lib spatial` passed **13/13** tests. These cover:

- Delayed band impulse and exact complementary recombination; toned chorus
  impulse arrival; Room early reflections and measured tail-energy decay.
- Positive/negative maximum feedback, finite output, hostile inputs/sample
  rates, default/serde/descriptor consistency, and exact tail expiry.
- Both choruses, phaser and stacked flanger modulating a steady sine.
- Stronger mono lock for 40 Hz than 2 kHz; Spreader mono fold-down preservation
  including live edits.
- Exact partition invariance for Room, existing EchoBank as the DelayBank
  substitute, and all seven other new circuits, including edits during tails.

`spatial/verify.rs` is an optional standalone test entry point that links the
actual built `windfall-dsp` library. It adds an isolated thread-local allocator
guard, avoiding a global allocator in the shared crate's unit-test target.
Its **15/15** tests passed, including zero allocation/reallocation/free calls
through parameter/tempo writes, reset, processing and silence-expiry paths for
all eight effects; allocator positive controls also passed.

To reproduce, use Git Bash with `source scripts/msvc-env.sh`, set
`TS_RS_EXPORT_DIR` to a temporary native path, build the library, and compile
the standalone entry with the matching Cargo artifact paths:

```bash
rustc --edition=2024 --test crates/windfall-dsp/src/spatial/verify.rs \
  -L dependency=target/debug/deps \
  --extern windfall_dsp=target/debug/libwindfall_dsp.rlib \
  --extern serde=target/debug/deps/libserde-<matching-cargo-hash>.rlib \
  --extern serde_json=target/debug/deps/libserde_json-<matching-cargo-hash>.rlib \
  -o "$task_bindings/spatial-tests.exe"
"$task_bindings/spatial-tests.exe" spatial --test-threads=2
```

Resolve matching serde artifacts from Cargo's `--message-format=json` output;
choosing the newest cached rlib by modification time can mix feature/profile
builds and is not reliable. No workspace suite, pnpm, CI, commits, pushes or PRs
were used. No host registration or parity status was changed.
