# Subtractive, macro-hybrid and FM instruments: hand-off, 2026-10-10

Stream: roadmap families I1 and I2. Branch `worktree-agent-abec4265b5432892e`.

**Nothing in this document has been compiled or run.** The stream was
edit-only. Every test named here is "written, not executed". Treat the code as
an unverified software candidate; the integrator must establish acceptance.

No parity status is changed here. `docs/parity/parity.json` and
`docs/parity/PARITY.md` are untouched.

## Row map

Each row gets its own `InstrumentKind`, its own settings struct and its own
typed patch graph over shared fixed-capacity voice storage. Nothing is an alias or a preset of another kind. The existing
`MatrixFm`, `RingHybrid`, `TripleOsc`, `WaveLane` and `MacroVoice` kinds are
left exactly as they were, so their accepted tests are unaffected.

| Row | Kind (serde tag) | Windfall name | State |
| --- | --- | --- | --- |
| `inst-kepler` | `VintagePoly` (`vintagePoly`) | Vintage poly | full-row software candidate; execution pending |
| `inst-kepler-exo` | `DualPoly` (`dualPoly`) | Dual vintage poly | full-row software candidate; execution pending |
| `inst-poizone` | `CompactSynth` (`compactSynth`) | Compact synth | full-row software candidate; execution pending |
| `inst-sawer` | `VintageSaw` (`vintageSaw`) | Vintage saw | full-row software candidate; execution pending |
| `inst-minisynth` | `PocketSynth` (`pocketSynth`) | Pocket synth | full-row software candidate; execution pending |
| `inst-gms` | `GrooveSynth` (`grooveSynth`) | Groove synth | full-row software candidate; execution pending |
| `inst-toxic-biohazard` | `HybridSeq` (`hybridSeq`) | Hybrid sequencer | full-row software candidate; execution pending |
| `inst-sytrus` | `MatrixSynth` (`matrixSynth`) | Matrix synth | full-row software candidate; execution pending |
| `inst-flex` | `PackSynth` (`packSynth`) | Pack synth | full-row software candidate; execution pending |

All nine instruments have separate implementation commits. Acceptance remains with the integrator.

## Shared code (landed)

`crates/windfall-dsp/src/polysynth/`:

- `core.rs`: `Controls` (5 ms smoothed settings, with a "changed this sample"
  flag so envelopes are reconfigured only when a setting moves), `Notes` /
  `Note` (fixed 16-slot allocator: polyphony limit, oldest-released-first
  stealing with a 5 ms de-click ramp, mono legato at a limit of one, constant
  time glide, 5 ms stop fade), `Ladder` (nonlinear-feedback four-pole ladder
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

## 2026-10-11 continuation — Vintage poly / inst-kepler

Full-row software candidate, awaiting integrator execution. Registered `VintagePoly`
with an independent DCO saw/pulse clock, divided sub, seeded noise, deterministic
component drift, PWM, bass-cut stage, nonlinear four-pole resonant ladder,
independent amp/filter contours, key tracking, LFO, seven-way stereo unison,
mono legato/glide and chorus. Shared runtime is typed by the patch graph; it
never delegates to an existing synth or selects a renamed preset. Fixed 16-slot
voice storage; two synthesis/filter substeps per host sample. Effects allocate
only in prepare and have bounded tails. No copied third-party implementation.

Purpose-built `polysynth-editor.tsx` follows the oscillator → filter → contours →
LFO → voices/chorus signal flow. Existing checked parameter commands provide
history, automation, persistence and host publication; no new project fields.

Tests **written, not executed**:
- DSP `polysynth/tests.rs`: `vintage_poly_tuning_filter_pwm_chorus_unison_and_partition`,
  `vintage_poly_glide_and_envelope_release_have_sample_duration`,
  `vintage_poly_high_saw_suppresses_folded_inharmonic_components`.
- `builtin_instrument_acceptance_qa/polysynth.rs`: `vintage_poly_filter_reaches_history_disk_and_live_offline_pcm`
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

## 2026-10-11 continuation - DualPoly / inst-kepler-exo

Full-row software candidate, awaiting execution. Two independent oscillator banks with cross-frequency modulation, sub/noise, deterministic drift and PWM feed switchable 12/24 dB lowpass, bandpass or highpass ladder outputs with drive. Independent contours, key tracking, mono glide, stereo unison, chorus, tempo echo and reverb extend the compact vintage model. DSP test: dual_poly_banks_cross_modulation_and_each_filter_response_sound.
Purpose-built editor group: Dual oscillator banks. Uses the existing checked instrument
commands, tagged project persistence, engine hosting and live/offline processor.
New generated types: `DualPoly`, `DualPolyParams`, `dualPoly` tag and descriptors.
Tests **written, not executed**: `dual_poly_musical_edit_reaches_history_disk_and_live_offline_pcm`
in registered `builtin_instrument_acceptance_qa/polysynth.rs`; the `dualPoly` mounted editor case
(edit and undo); complete descriptor/serde/default/range tests in DSP params;
per-graph DSP tests in `polysynth/tests.rs` (names below).
Use the integrator commands in the Vintage Poly section, after regenerating
bindings/descriptors and WASM. Check strict Clippy and numerical thresholds first;
no measured sound/performance claim is made. No external proprietary content.

## 2026-10-11 continuation - CompactSynth / inst-poizone

Full-row software candidate, awaiting execution. A quick-programming two-oscillator graph with pitch/fine mix, PWM and ring blend into a saturating multimode state-variable filter. Contours, unison, glide and chorus have direct controls. DSP test: compact_synth_quick_oscillators_ring_and_filter_modes_are_distinct.
Purpose-built editor group: Quick oscillators. Uses the existing checked instrument
commands, tagged project persistence, engine hosting and live/offline processor.
New generated types: `CompactSynth`, `CompactSynthParams`, `compactSynth` tag and descriptors.
Tests **written, not executed**: `compact_synth_musical_edit_reaches_history_disk_and_live_offline_pcm`
in registered `builtin_instrument_acceptance_qa/polysynth.rs`; the `compactSynth` mounted editor case
(edit and undo); complete descriptor/serde/default/range tests in DSP params;
per-graph DSP tests in `polysynth/tests.rs` (names below).
Use the integrator commands in the Vintage Poly section, after regenerating
bindings/descriptors and WASM. Check strict Clippy and numerical thresholds first;
no measured sound/performance claim is made. No external proprietary content.

## 2026-10-11 continuation - VintageSaw / inst-sawer

Full-row software candidate, awaiting execution. A saw-focused charge/discharge model with DC-compensated asymmetry, pitch decay contour, pulse blend, sub, deterministic component wander and a saturating two-pole integrator filter. Unison, glide, amp/filter envelopes and chorus/phaser/tempo echo complete the graph. DSP test: vintage_saw_asymmetric_charge_pitch_contour_and_saturating_filter.
Purpose-built editor group: Charging saw oscillator. Uses the existing checked instrument
commands, tagged project persistence, engine hosting and live/offline processor.
New generated types: `VintageSaw`, `VintageSawParams`, `vintageSaw` tag and descriptors.
Tests **written, not executed**: `vintage_saw_musical_edit_reaches_history_disk_and_live_offline_pcm`
in registered `builtin_instrument_acceptance_qa/polysynth.rs`; the `vintageSaw` mounted editor case
(edit and undo); complete descriptor/serde/default/range tests in DSP params;
per-graph DSP tests in `polysynth/tests.rs` (names below).
Use the integrator commands in the Vintage Poly section, after regenerating
bindings/descriptors and WASM. Check strict Clippy and numerical thresholds first;
no measured sound/performance claim is made. No external proprietary content.

## 2026-10-11 continuation - PocketSynth / inst-minisynth

Full-row software candidate, awaiting execution. A bounded one-oscillator mobile-style engine with five shapes, sub divider, warmth, two-pole filter, envelope, vibrato, mono glide, chorus and tempo echo. Purpose-built Oscillator/Filter/Envelope/Motion/Effects tabs keep programming compact. DSP test: pocket_synth_each_shape_tunes_and_mobile_controls_reach_audio.
Purpose-built editor group: Oscillator. Uses the existing checked instrument
commands, tagged project persistence, engine hosting and live/offline processor.
New generated types: `PocketSynth`, `PocketSynthParams`, `pocketSynth` tag and descriptors.
Tests **written, not executed**: `pocket_synth_musical_edit_reaches_history_disk_and_live_offline_pcm`
in registered `builtin_instrument_acceptance_qa/polysynth.rs`; the `pocketSynth` mounted editor case
(edit and undo); complete descriptor/serde/default/range tests in DSP params;
per-graph DSP tests in `polysynth/tests.rs` (names below).
Use the integrator commands in the Vintage Poly section, after regenerating
bindings/descriptors and WASM. Check strict Clippy and numerical thresholds first;
no measured sound/performance claim is made. No external proprietary content.

## 2026-10-11 continuation - GrooveSynth / inst-gms

Full-row software candidate, awaiting execution. A groove-box hybrid with distinct analog, phase modulation, ring and harmonic-scan paths. Tempo/free motion and rhythmic gating feed drive/filter, amp and filter contours, unison and chorus/phaser/echo/room. DSP test: groove_synth_real_analog_phase_ring_table_paths_and_tempo_motion.
Purpose-built editor group: Groove engine. Uses the existing checked instrument
commands, tagged project persistence, engine hosting and live/offline processor.
New generated types: `GrooveSynth`, `GrooveSynthParams`, `grooveSynth` tag and descriptors.
Tests **written, not executed**: `groove_synth_musical_edit_reaches_history_disk_and_live_offline_pcm`
in registered `builtin_instrument_acceptance_qa/polysynth.rs`; the `grooveSynth` mounted editor case
(edit and undo); complete descriptor/serde/default/range tests in DSP params;
per-graph DSP tests in `polysynth/tests.rs` (names below).
Use the integrator commands in the Vintage Poly section, after regenerating
bindings/descriptors and WASM. Check strict Clippy and numerical thresholds first;
no measured sound/performance claim is made. No external proprietary content.

## 2026-10-11 continuation - HybridSeq / inst-toxic-biohazard

Full-row software candidate, awaiting execution. Six individually enveloped operators, four real operator algorithms, feedback/key scaling and a separate BLEP saw feed the subtractive filter. A native 16-step pitch/velocity/enable grid supplies tempo divisions, length and envelope-gated steps. Chorus, echo and room are real effect stages. DSP tests: hybrid_sequence_algorithms_feedback_key_scaling_and_subtractive_path; hybrid_sequence_step_pitch_gate_velocity_and_tempo_are_real.
Purpose-built editor group: FM and subtractive graph. Uses the existing checked instrument
commands, tagged project persistence, engine hosting and live/offline processor.
New generated types: `HybridSeq`, `HybridSeqParams`, `hybridSeq` tag and descriptors.
Tests **written, not executed**: `hybrid_seq_musical_edit_reaches_history_disk_and_live_offline_pcm`
in registered `builtin_instrument_acceptance_qa/polysynth.rs`; the `hybridSeq` mounted editor case
(edit and undo); complete descriptor/serde/default/range tests in DSP params;
per-graph DSP tests in `polysynth/tests.rs` (names below).
Use the integrator commands in the Vintage Poly section, after regenerating
bindings/descriptors and WASM. Check strict Clippy and numerical thresholds first;
no measured sound/performance claim is made. No external proprietary content.

## 2026-10-11 continuation - MatrixSynth / inst-sytrus

Full-row software candidate, awaiting execution. Six individual operator shapes/ratios/envelopes/key scales/feedback values feed independently editable 6x6 FM and 6x6 ring matrices. Cycles use previous-substep signals, with explicit carrier outputs, filter mix, contours and LFO pitch/phase/filter modulation. The native UI edits both matrices and each operator, not only patches. DSP tests: matrix_synth_fm_sidebands_match_bessel_and_ring_matches_product_identity; matrix_synth_cycles_feedback_key_scaling_modulation_and_filter_are_audible.
Purpose-built editor group: FM routing matrix. Uses the existing checked instrument
commands, tagged project persistence, engine hosting and live/offline processor.
New generated types: `MatrixSynth`, `MatrixSynthParams`, `matrixSynth` tag and descriptors.
Tests **written, not executed**: `matrix_synth_musical_edit_reaches_history_disk_and_live_offline_pcm`
in registered `builtin_instrument_acceptance_qa/polysynth.rs`; the `matrixSynth` mounted editor case
(edit and undo); complete descriptor/serde/default/range tests in DSP params;
per-graph DSP tests in `polysynth/tests.rs` (names below).
Use the integrator commands in the Vintage Poly section, after regenerating
bindings/descriptors and WASM. Check strict Clippy and numerical thresholds first;
no measured sound/performance claim is made. No external proprietary content.

## 2026-10-11 continuation - PackSynth / inst-flex

Full-row software candidate, awaiting execution. Three original CC0 content packs, twelve named patches and real subtractive/wavetable/multisample/FM engines share performance macros and searchable native browsing. Immutable mip tables and six root-key PCM zones with mip levels are prepared off-thread and held outside Copy settings. DSP tests: pack_synth_own_banks_use_real_subtractive_wavetable_multisample_and_fm; own_pack_assets_are_mip_limited_and_multisamples_have_independent_roots. See integration/seams/polysynth-packs.md for content provenance and asset contract.
Purpose-built editor group: Windfall patch browser. Uses the existing checked instrument
commands, tagged project persistence, engine hosting and live/offline processor.
New generated types: `PackSynth`, `PackSynthParams`, `packSynth` tag and descriptors.
Tests **written, not executed**: `pack_synth_musical_edit_reaches_history_disk_and_live_offline_pcm`
in registered `builtin_instrument_acceptance_qa/polysynth.rs`; the `packSynth` mounted editor case
(edit and undo); complete descriptor/serde/default/range tests in DSP params;
per-graph DSP tests in `polysynth/tests.rs` (names below).
Use the integrator commands in the Vintage Poly section, after regenerating
bindings/descriptors and WASM. Check strict Clippy and numerical thresholds first;
no measured sound/performance claim is made. No external proprietary content.

## Final source and acceptance supplement, 2026-10-11

Implementation commits: `d909d1ac` Vintage Poly, `40bda262` Dual Poly,
`e6c0ad12` Compact Synth, `9d527ba6` Vintage Saw, `b2b32701` Pocket Synth,
`1fa77021` Groove Synth, `ac54e248` Hybrid Sequencer, `facbe0d1` Matrix Synth,
`77aa87ba` Pack Synth. Final source/acceptance supplement: `135d7cf0`.

Every test below is **written, not executed**. Rustfmt parsed/formatted owned
Rust files; it is not compilation or test evidence. No build, test, benchmark,
generation, device check, push or parity-status edit was performed.

The supplement places the engine child module under
`crates/windfall-engine/tests/builtin_instrument_acceptance_qa/polysynth.rs`,
with an explicit parent path so Cargo does not discover it as a standalone
integration test. It also removes unused helpers, restricts matrix operator
shapes to supported `OperatorWave` choices, and budgets high-note phase
modulation against the host Nyquist limit. Cycles use previous-substep signals.

Pack assets now use a control-thread-initialized `OnceLock<Arc<PackAssets>>`.
Callbacks only borrow the existing reference; parameter publication and reset
never initialize or release the assets. Effects buffers clear once when a tail
finishes, preventing old inactive-effect audio from leaking into the next note.

Additional tests, **written, not executed**:

- In `crates/windfall-dsp/src/polysynth/tests.rs`:
  `every_family_row_tunes_to_midi_and_limits_high_note_aliases`,
  `every_family_row_has_envelopes_glide_and_stereo_voice_behavior`,
  `every_family_row_filter_sweep_and_live_controls_are_partition_invariant`.
  These cover all nine rows with representative plain patches, release time,
  mono glide, stereo/unison behavior (Pocket uses chorus), cutoff sweeps and
  exact sample-timeline comparisons across block sizes. They do not establish
  that all extreme nonlinear/FM/ring settings are alias-free.
- In `crates/windfall-dsp/tests/dsp/realtime.rs`:
  `subtractive_hybrid_matrix_and_pack_callbacks_do_not_allocate_or_free`.
  Guard prepared maximum patches, note changes, tempo, oversized blocks, stop
  and reset for all nine kinds.
- In the engine child module:
  `hybrid_sequence_edit_reaches_saved_project_and_both_host_paths`,
  `matrix_ring_route_and_modulation_reach_saved_project_and_both_host_paths`,
  `own_pack_engine_selection_reaches_saved_project_and_both_host_paths`.
  The accepted engine harness supplies checked commands, undo/redo, disk
  save/reopen and exact live/offline PCM for these and the nine cases above.
- In `polysynth-editor.test.tsx`: `edits ring routing, operator feedback and live
  modulation in the matrix`, `edits the hybrid sequence rather than merely
  choosing an FM patch`, and `searches own packs, selects a multisample patch
  and restores it with undo`, in addition to all nine mounted edit/undo cases.

### Exact integrator commands

Run sequentially from the merged integration checkout. These are hand-off
instructions, not commands executed by this worker. Git Bash on Windows:

```bash
source scripts/msvc-env.sh
export TS_RS_EXPORT_DIR="$(cygpath -w "$(mktemp -d)")"
cargo test -p windfall-dsp --lib polysynth
cargo test -p windfall-dsp --test dsp params
cargo test -p windfall-dsp --test dsp realtime::subtractive_hybrid_matrix_and_pack_callbacks_do_not_allocate_or_free
cargo test -p windfall-engine --test builtin_instrument_acceptance_qa polysynth_acceptance
cargo clippy -p windfall-dsp -p windfall-engine --all-targets -- -D warnings
scripts/gen-bindings.sh
scripts/build-sim.sh
pnpm --dir apps/desktop test src/features/channel-rack/synth/polysynth-editor.test.tsx src/features/params/access.test.ts src/features/channel-rack/inspector/instrument-section.test.tsx --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
pnpm --dir apps/desktop exec prettier --check src/features/channel-rack/synth/polysynth-editor.tsx src/features/channel-rack/synth/pack-browser.tsx src/features/channel-rack/synth/polysynth-editor.test.tsx
```

Then run full repository integration gates. New TS callers require regenerated
types/descriptors and the refreshed simulator before the mounted cases can run.

### Shared seams and generated output

Shared files touched:
`crates/windfall-dsp/src/instrument.rs`, `crates/windfall-dsp/src/lib.rs`,
`crates/windfall-dsp/tests/dsp/params.rs`, `crates/windfall-dsp/tests/dsp/realtime.rs`,
`crates/windfall-engine/tests/builtin_instrument_acceptance_qa.rs`,
`apps/desktop/src/features/channel-rack/inspector/instrument-section.tsx`,
`apps/desktop/src/features/params/access.test.ts`.

`InstrumentKind::ALL` is 36 here: the merged 27 include `MidiOut`, followed by
these nine appended kinds. Reconcile the parallel instrument stream's appended
kinds and exhaustive matches during integration. Preserve descriptor order.
Existing tagged persistence, checking, history, lowering and `AnyInstrument`
hosting are reused without new project fields or commands.

Regenerate `InstrumentKind`, `InstrumentParams`, all nine `*Params` and their
descriptors/defaults, plus `PolyResponse`, `CompactResponse`, `GrooveMode`,
`HybridAlgorithm`, `HybridStep`, `MatrixOperator`, `OperatorWave`, `PackPatch`.
Regenerate simulator WASM/metadata too. No generated output was hand-edited.
The immutable asset contract and own-content provenance are in
`docs/integration/seams/polysynth-packs.md`; large tables never enter Copy params.

### Remaining gates and risks, in check order

1. Compile, strict Clippy, bindings and TypeScript: macro forwarding, child-test
   path, array descriptors, exports, exhaustive registry arms and concurrent
   additive/physical/drum additions are unverified.
2. Execute DSP spectral/PCM tests. Bessel sidebands and ring-product identities
   use independent references, but thresholds, filter behavior and extreme
   FM/ring aliasing are unmeasured. Two substeps and conservative modulation/mip
   limits are not proof of alias-free nonlinear graphs at every setting.
3. Execute allocator, history/disk, live/offline and mounted editor tests after
   regeneration. Legacy note-instance/expression defaults remain in use; this
   work does not establish MPE/per-note expression support.
4. Profile maximum 16 voices, seven unison, six operators/dense matrices and the
   roughly 7 MB shared original pack bank on the target machine. First-load
   asset generation occurs in prepare; its latency is unmeasured.
5. Listen through native editors and patches. Sample patches are original finite
   one-shot key/mallet/pluck sounds. Subjective vintage behavior, macro usefulness,
   sustained-note transitions and effects tails still need listening. No
   proprietary content, copied third-party engine or dependency was added.
