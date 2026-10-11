# E3 circuit behaviour and acceptance candidates

Source delivery on 2026-10-11. Tests are **written, not executed**. The handoff
is [2026-10-10-fx-delay-mod-reverb.md](qa/2026-10-10-fx-delay-mod-reverb.md).
No hardware measurements or sound-identical modelling is claimed.

## Public behaviour used

Read 2026-10-11: the [Vintage Chorus manual](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/Vintage%20Chorus.htm)
identifies a two-line bucket-brigade chorus with I, II and combined modes,
and adjustable modulation. Windfall's original clocked implementation uses
128 samples in transit (a 256-stage convention), a two-pole 9 kHz input guard,
and an output tone filter. Its authored triangle sweeps are 0.513/0.863 Hz over
1.66–5.35 ms with opposite sides, or 9.75 Hz over 3.3–3.7 ms in mono.
These numbers define Windfall's behavioural model, not measured hardware.
The pre-existing four-voice circuit remains Free mode and the legacy default.

The [Vintage Phaser manual](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/Vintage%20Phaser.htm)
identifies a pedal-inspired sweep with feedback and stereo modulation;
the [pedal manufacturer's description](https://www.ehx.com/products/small-stone/)
describes variable rate and feedback colour. Windfall exposes a four-stage,
equal-corner circuit producing two moving notches, and coloured signed feedback.
Six, eight and twelve stages and continuous stagger are also available. The
earlier twelve-stage, fully staggered default stays unchanged. It is an original
lossless lattice model of that behaviour, not a transistor/OTA circuit replica.

The [Hyper Chorus manual](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/Hyper%20Chorus.htm)
describes dense independently detuned delay voices. Windfall exposes 2–16 voices
per side with distinct delay/rate ratios and alternating stereo crossfeed.
The [Flangus manual](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/Fruity%20Flangus.htm)
describes controllable stacked flangers and order. Windfall exposes 1–4 serial
feedback combs, independent phases/rate ratios and stereo sweep spread.
Each count changes the processing topology; none is a renamed preset.

The [legacy Reeverb manual](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/Fruity%20Reeverb.htm)
describes room reflections, pre-delay, size, decay and damping. Windfall Room
provides four signed early taps, four damped parallel combs and two diffusers per
side. Its 0.1–1.5 second nominal RT60 and 5.01 second safety lifetime are explicit
limits. It covers the row's legacy room summary, not every successor control.

The [LuxeVerb manual](https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/LuxeVerb.htm)
describes modulated algorithmic spaces and a feed-forward pitch option. Windfall
Lush Space adds independently controlled space size and a wet pitch voice
(-12–12 semitones, 0–100% amount) to its cross-coupled modulated tanks. The pitch
voice uses two complementary 40 ms grains and convex interpolation. Pitch is
feed-forward, so it does not create an additional feedback stability problem.
No freeze, external ducking or pitch-feedback feature is claimed by this row's
summary mapping. Changing size intentionally glides and bends wet pitch.

## Stable controls and realtime contract

All original descriptor prefixes and enum values are retained. Appended controls:

- Bucket Chorus: 6 mode, 7 sync, 8 cycleBeats.
- Swarm Chorus: 6 voices, 7 sync, 8 cycleBeats.
- Stage Phaser: 7 stages (12, 4, 6, 8), 8 stagger, 9 sync, 10 cycleBeats.
- Stacked Flanger: 8 order, 9 sync, 10 cycleBeats.
- Lush Space: 8 pitchSemitones, 9 pitchAmount, 10 size.

Missing fields use serde defaults; old projects keep Free/four voices, eight
swarm voices, twelve staggered stages, two combs and unpitched unit-size tanks.
The four modulation circuits optionally use `BPM/(60*cycleBeats)` for the base
rate (0.125–16 quarter-note beats); per-voice rate ratios still apply. Fixed bucket
modes ignore free controls and tempo. The stack's second synced rate is 1.382
times the first. Tempo edits retain phase and glide over the normal 10 ms ramp.
Lush Space modulation stays in Hz; Room has no modulation clock. Host tempo is
bounded to 10–1000 BPM, nonfinite tempo uses 120. All clocks advance per frame.

Zero dry-path PDC for all six. All storage is reserved during preparation; reset,
parameter/tempo writes, processing, bypass and safety expiry allocate/free nothing.
Stack warm-up is now 64 ms, covering all four serial combs. Other spatial tails
retain their documented finite safety lifetimes. Lush lifetime includes its grain
history and the largest requested size/decay since reset:
`ceil((1.5*max(decayS,0.18)+0.77*(0.4+1.2*size))*rate)+grainHistory`.
Zero pitch amount preserves the original hall's samples at the original size.
The slot retains its shared bypass/mix ramps and clears on dormant wake.

Each purpose-built editor uses existing parameter bindings, automation context
menus and gestures. Model modes dim irrelevant controls. Size/reflection timing,
voice count, notch count and actual comb chain readouts expose the chosen circuit.
The unchanged project commands supply persistence, automation and undo/redo.

## Authored verification

`spatial/model_tests.rs` measures bucket timing, opposed/mono modes, real mode
differences, voice averaging against a continuous-delay reference, two phaser
notches using independently bilinear-warped cancellation frequencies, and every
comb order against an independent difference equation. It also checks extrema,
edit/empty-block partitions, tails, descriptor prefixes, migration and tempo/depth.
Existing spatial tests retain Room impulse/decay, rate sanitation and response
coverage. Lush tests add pitch spectral projection, size/early timing and pitch
edit partition checks to the earlier impulse/decay/modulation tests.

`dsp/e3_circuits.rs` covers all six mounted slots, realtime allocator guards,
topology changes, tempo, reset, bypass, tail expiry and cleared dormant wake.
`project/tests/e3_circuits.rs` covers every descriptor through checked commands,
gesture history, files, automation, removal/replacement history and rejected edits.
`engine/tests/e3_circuits_qa.rs` has six named native PCM/history/file/live-offline
cases. `features/effects/e3-circuits.test.tsx` mounts all six actual editors against
the shared Rust WASM document, tests distinct controls/history/files and automation,
plus bucket disabling and free-rate/tempo-cycle switching.

All require the integrator's runs. See the handoff for exact commands and risks.
