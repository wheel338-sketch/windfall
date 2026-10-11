# Subtractive, macro-hybrid and FM instruments: hand-off, 2026-10-10

Stream: roadmap families I1 and I2. Branch `worktree-agent-abec4265b5432892e`.

**Nothing in this document has been compiled or run.** The stream was
edit-only. Every test named here is "written, not executed". Treat the code as
a first draft that the parent must build, and expect compile errors.

No parity status is changed here. `docs/parity/parity.json` and
`docs/parity/PARITY.md` are untouched.

## Row map

Each row gets its own `InstrumentKind`, its own settings struct and its own
voice structure. Nothing is an alias or a preset of another kind. The existing
`MatrixFm`, `RingHybrid`, `TripleOsc`, `WaveLane` and `MacroVoice` kinds are
left exactly as they were, so their accepted tests are unaffected.

| Row | Kind (serde tag) | Windfall name | State |
| --- | --- | --- | --- |
| `inst-kepler` | `VintagePoly` (`vintagePoly`) | Vintage poly | not started |
| `inst-kepler-exo` | `DualPoly` (`dualPoly`) | Dual vintage poly | not started |
| `inst-poizone` | `CompactSynth` (`compactSynth`) | Compact synth | not started |
| `inst-sawer` | `VintageSaw` (`vintageSaw`) | Vintage saw | not started |
| `inst-minisynth` | `PocketSynth` (`pocketSynth`) | Pocket synth | not started |
| `inst-gms` | `GrooveSynth` (`grooveSynth`) | Groove synth | not started |
| `inst-toxic-biohazard` | `HybridSeq` (`hybridSeq`) | Hybrid sequencer | not started |
| `inst-sytrus` | `MatrixSynth` (`matrixSynth`) | Matrix synth | not started |
| `inst-flex` | `PackSynth` (`packSynth`) | Pack synth | not started |

The per-row sections below are filled in as each instrument lands.

## Shared code (landed)

`crates/windfall-dsp/src/polysynth/`:

- `core.rs`: `Controls` (5 ms smoothed settings, with a "changed this sample"
  flag so envelopes are reconfigured only when a setting moves), `Notes` /
  `Note` (fixed 16-slot allocator: polyphony limit, oldest-released-first
  stealing with a 5 ms de-click ramp, mono legato at a limit of one, constant
  time glide, 5 ms stop fade), `Ladder` (zero-delay-feedback four-pole ladder
  with saturated feedback and pole-mix outputs), `SatSvf` (saturating
  state-variable filter), `Drift`, `drive`, phase and pan helpers.
- `fx.rs`: `Chorus` (also the flanger), `StereoDelay`, `Phaser`, `Reverb`.
  Lines are allocated in `prepare`. With mix at zero each returns its input
  bit-for-bit, which the instruments rely on for exact silence after release.

## What the parent must regenerate

- `scripts/gen-bindings.sh` (new `InstrumentKind` variants, new parameter
  structs and enums, `descriptors.json`).
- `scripts/build-sim.sh` (the WASM the mock backend runs).

## Least-sure spots, shared code

- `core.rs` uses `if a && let Some(x) = ...` chains (edition 2024, Rust 1.88+).
- `Note::finish` frees a slot by assigning `Note::IDLE`; instruments must reset
  their own per-slot state when they see `!note.active` afterwards.
- Strict Clippy: helpers in `core.rs` that end up unused by any instrument will
  warn as dead code and need deleting.

## 2026-10-11 continuation â€” Vintage poly / inst-kepler

Full-row software candidate, awaiting integrator execution. Registered `VintagePoly`
with an independent DCO saw/pulse clock, divided sub, seeded noise, deterministic
component drift, PWM, bass-cut stage, nonlinear four-pole resonant ladder,
independent amp/filter contours, key tracking, LFO, seven-way stereo unison,
mono legato/glide and chorus. Shared runtime is typed by the patch graph; it
never delegates to an existing synth or selects a renamed preset. Fixed 16-slot
voice storage; two synthesis/filter substeps per host sample. Effects allocate
only in prepare and have bounded tails. No copied third-party implementation.

Purpose-built `polysynth-editor.tsx` follows the oscillator â†’ filter â†’ contours â†’
LFO â†’ voices/chorus signal flow. Existing checked parameter commands provide
history, automation, persistence and host publication; no new project fields.

Tests **written, not executed**:
- DSP `polysynth/tests.rs`: `vintage_poly_tuning_filter_pwm_chorus_unison_and_partition`,
  `vintage_poly_glide_and_envelope_release_have_sample_duration`,
  `vintage_poly_high_saw_suppresses_folded_inharmonic_components`.
- `polysynth_acceptance.rs`: `vintage_poly_filter_reaches_history_disk_and_live_offline_pcm`
  (registered as a child module of `builtin_instrument_acceptance_qa.rs`).
- Mounted `polysynth-editor.test.tsx`: signal-flow composition, command edit and undo.
- Existing parameter suite extended with complete descriptor/default/serde/range checks.

Integrator commands (Git Bash, after source scripts/msvc-env.sh and temporary
TS_RS_EXPORT_DIR):
`cargo test -p windfall-dsp --lib polysynth`; `cargo test -p windfall-dsp --test dsp params`;
`cargo test -p windfall-engine --test builtin_instrument_acceptance_qa polysynth`;
then regenerate bindings/descriptors and WASM, and
`pnpm --dir apps/desktop test src/features/channel-rack/synth/polysynth-editor.test.tsx src/features/params/access.test.ts --maxWorkers=4`.
Check compile/strict Clippy first: shared runtime/core helpers intended for later
instruments may still be unused until the remaining graphs land. The high-note
alias threshold, filter transfer and PCM fixture are authored assertions, not
measured evidence. Target-machine listening/performance remains unmeasured.
New generated kind/tag: `VintagePoly` / `vintagePoly`; `VintagePolyParams`.

## 2026-10-11 continuation — DualPoly / inst-kepler-exo

Full-row software candidate, awaiting execution. Two independent oscillator banks with cross-frequency modulation, sub/noise, deterministic drift and PWM feed switchable 12/24 dB lowpass, bandpass or highpass ladder outputs with drive. Independent contours, key tracking, mono glide, stereo unison, chorus, tempo echo and reverb extend the compact vintage model. DSP test: dual_poly_banks_cross_modulation_and_each_filter_response_sound.
Purpose-built editor group: Dual oscillator banks. Uses the existing checked instrument
commands, tagged project persistence, engine hosting and live/offline processor.
New generated types: `DualPoly`, `DualPolyParams`, `dualPoly` tag and descriptors.
Tests **written, not executed**: `dualPoly_musical_edit_reaches_history_disk_and_live_offline_pcm`
in registered `polysynth_acceptance.rs`; the `dualPoly` mounted editor case
(edit and undo); complete descriptor/serde/default/range tests in DSP params;
per-graph DSP tests in `polysynth/tests.rs` (names below).
Use the integrator commands in the Vintage Poly section, after regenerating
bindings/descriptors and WASM. Check strict Clippy and numerical thresholds first;
no measured sound/performance claim is made. No external proprietary content.

## 2026-10-11 continuation — CompactSynth / inst-poizone

Full-row software candidate, awaiting execution. A quick-programming two-oscillator graph with pitch/fine mix, PWM and ring blend into a saturating multimode state-variable filter. Contours, unison, glide and chorus have direct controls. DSP test: compact_synth_quick_oscillators_ring_and_filter_modes_are_distinct.
Purpose-built editor group: Quick oscillators. Uses the existing checked instrument
commands, tagged project persistence, engine hosting and live/offline processor.
New generated types: `CompactSynth`, `CompactSynthParams`, `compactSynth` tag and descriptors.
Tests **written, not executed**: `compactSynth_musical_edit_reaches_history_disk_and_live_offline_pcm`
in registered `polysynth_acceptance.rs`; the `compactSynth` mounted editor case
(edit and undo); complete descriptor/serde/default/range tests in DSP params;
per-graph DSP tests in `polysynth/tests.rs` (names below).
Use the integrator commands in the Vintage Poly section, after regenerating
bindings/descriptors and WASM. Check strict Clippy and numerical thresholds first;
no measured sound/performance claim is made. No external proprietary content.

## 2026-10-11 continuation — VintageSaw / inst-sawer

Full-row software candidate, awaiting execution. A saw-focused charge/discharge model with DC-compensated asymmetry, pitch decay contour, pulse blend, sub, deterministic component wander and a saturating two-pole integrator filter. Unison, glide, amp/filter envelopes and chorus/phaser/tempo echo complete the graph. DSP test: vintage_saw_asymmetric_charge_pitch_contour_and_saturating_filter.
Purpose-built editor group: Charging saw oscillator. Uses the existing checked instrument
commands, tagged project persistence, engine hosting and live/offline processor.
New generated types: `VintageSaw`, `VintageSawParams`, `vintageSaw` tag and descriptors.
Tests **written, not executed**: `vintageSaw_musical_edit_reaches_history_disk_and_live_offline_pcm`
in registered `polysynth_acceptance.rs`; the `vintageSaw` mounted editor case
(edit and undo); complete descriptor/serde/default/range tests in DSP params;
per-graph DSP tests in `polysynth/tests.rs` (names below).
Use the integrator commands in the Vintage Poly section, after regenerating
bindings/descriptors and WASM. Check strict Clippy and numerical thresholds first;
no measured sound/performance claim is made. No external proprietary content.

## 2026-10-11 continuation — PocketSynth / inst-minisynth

Full-row software candidate, awaiting execution. A bounded one-oscillator mobile-style engine with five shapes, sub divider, warmth, two-pole filter, envelope, vibrato, mono glide, chorus and tempo echo. Purpose-built Oscillator/Filter/Envelope/Motion/Effects tabs keep programming compact. DSP test: pocket_synth_each_shape_tunes_and_mobile_controls_reach_audio.
Purpose-built editor group: Oscillator. Uses the existing checked instrument
commands, tagged project persistence, engine hosting and live/offline processor.
New generated types: `PocketSynth`, `PocketSynthParams`, `pocketSynth` tag and descriptors.
Tests **written, not executed**: `pocketSynth_musical_edit_reaches_history_disk_and_live_offline_pcm`
in registered `polysynth_acceptance.rs`; the `pocketSynth` mounted editor case
(edit and undo); complete descriptor/serde/default/range tests in DSP params;
per-graph DSP tests in `polysynth/tests.rs` (names below).
Use the integrator commands in the Vintage Poly section, after regenerating
bindings/descriptors and WASM. Check strict Clippy and numerical thresholds first;
no measured sound/performance claim is made. No external proprietary content.

## 2026-10-11 continuation — GrooveSynth / inst-gms

Full-row software candidate, awaiting execution. A groove-box hybrid with distinct analog, phase modulation, ring and harmonic-scan paths. Tempo/free motion and rhythmic gating feed drive/filter, amp and filter contours, unison and chorus/phaser/echo/room. DSP test: groove_synth_real_analog_phase_ring_table_paths_and_tempo_motion.
Purpose-built editor group: Groove engine. Uses the existing checked instrument
commands, tagged project persistence, engine hosting and live/offline processor.
New generated types: `GrooveSynth`, `GrooveSynthParams`, `grooveSynth` tag and descriptors.
Tests **written, not executed**: `grooveSynth_musical_edit_reaches_history_disk_and_live_offline_pcm`
in registered `polysynth_acceptance.rs`; the `grooveSynth` mounted editor case
(edit and undo); complete descriptor/serde/default/range tests in DSP params;
per-graph DSP tests in `polysynth/tests.rs` (names below).
Use the integrator commands in the Vintage Poly section, after regenerating
bindings/descriptors and WASM. Check strict Clippy and numerical thresholds first;
no measured sound/performance claim is made. No external proprietary content.
