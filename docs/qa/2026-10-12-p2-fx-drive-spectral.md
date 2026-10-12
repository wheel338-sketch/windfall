# P2 drive, spectral and performance effects — implementation hand-off

Worktree: `C:/Users/ewhee/.t3/projects/windfall/.claude/worktrees/gpt-p2-fx-drive-spectral`.
Branch: `gpt/p2-fx-drive-spectral`; verified base supplied by the integrator:
`9ef9599a`. Source checkpoint before this document: `a838266f`.

**Nothing was built or run.** No compiler, test, benchmark, formatter, package
runner, install, bindings/WASM generator, or hardware check was invoked. All
tests below are authored, non-ignored acceptance gates, not passing evidence.
Only source reading/editing and Git inspection/commits were performed.

The eleven assigned SOFTWARE gaps have implementation or measurement gates in
this branch. No row is claimed verified or promoted to done. In particular,
declaring a CPU budget and writing its timer does not mean the budget was met.
The integrator must execute the gates and resolve failures before considering
the completion rule in `docs/qa/README.md` satisfied.

## Commits

- `02bdb786`: control-side curve/source retirement, document/history/plan roots,
  cached processor leases and callback reclamation acceptance.
- `b8c3d736`: whole-period pitch windows, host delay accounting, metadata-only
  replay reset, broader musical-transient/pitch measurements and CPU budgets.
- `e521e261`: native Session IR replacement/refusal/reopen and additional asset
  history/independent-engine acceptance.
- `03885963`: leases through slow save/backup/archive/export/preset snapshots;
  visible serialization refusal for expired unowned curve references.
- `a838266f`: mounted WAV publication refusal preserves the admitted IR/history.

No new effect kinds, dependencies, parameter fields, descriptor indices or
Command variants were added. No registries were reordered. The tracker,
generated bindings and simulator artifacts were not edited. None of these
eleven notes requested additional user-loadable presets; no vendor content or
presets were introduced. The synthesized fixtures and their round-number
recipes are original and may be used as CC0-1.0 test material.

## Shared ownership and realtime changes

`crates/windfall-dsp/src/asset.rs` now keeps immutable Arc-backed allocations in
a bounded store. Atomic slots publish pointers; a short reader counter protects
the pointer-to-Arc acquisition gap. The store retains the final owner, including
retired entries with outstanding readers. Only control-side collection drops
that owner. Lookup, lease replacement and reset do not allocate, free, lock,
wait or perform IO on audio. Curves are still persisted as points; scalar
parameters still contain Copy references. Source generations never reuse a
retired identity, even when the physical slot is reused.

`crates/windfall-project/src/{document,edit}.rs` pin the current project and both
sides of the actual retained mixer edits. Gesture merging rebuilds its roots,
so discarded intermediate curves are collectible. Cloned Documents retain
independent roots. Undo/redo truncation and document replacement release roots.
`crates/windfall-engine/src/plan.rs` independently pins compiled parameters,
including prepared scratch sources and departing effects. Waveshaper, Volume
Gate and Scratch cache reader leases in
`crates/windfall-dsp/src/{drive/waveshaper,performance/mod}.rs`.

`apps/desktop/src-tauri/src/session/preparation.rs` collects after the existing
engine retirement carrier is cleared outside recording/document/controller
guards. `session/{files,archive,export,presets,mixer_presets}.rs` retain assets
beside slow captured projects; export moves its lease into the worker closure.
`crates/windfall-engine/src/project_preparation.rs` no longer tells users that
an application restart is the only way to release retained scratch assets.

Important API boundary: a bare Copy CurveRef/ShapeRef/SourceRef is a lookup key,
not an owner. Fresh standalone publications remain unmanaged until adopted by
`AssetPins::for_effects`; Document and Plan adoption do this automatically.
Control-side callers retaining a plain cloned Project outside its Document
must retain `Document::effect_asset_pins()` beside it. An expired nonempty
reference now refuses serialization instead of silently saving an empty curve.
The native save race test covers this boundary for production project saving.

## Test file key

The per-row lists below use these exact paths:

- **A**: `crates/windfall-dsp/src/asset.rs` (existing lib test module).
- **D**: `crates/windfall-dsp/tests/dsp/nonlinear_performance.rs` (already
  registered in `crates/windfall-dsp/tests/dsp/main.rs`).
- **P**: `crates/windfall-project/tests/nonlinear_performance.rs` (standalone
  integration target).
- **E**: `crates/windfall-engine/tests/engine/nonlinear_performance.rs` (already
  registered in `crates/windfall-engine/tests/engine/main.rs`).
- **Q**: `crates/windfall-dsp/tests/dsp/p2_pitch_quality.rs` (new, appended to
  `tests/dsp/main.rs`).
- **B**: `crates/windfall-dsp/tests/dsp/p2_effect_budgets.rs` (new, appended to
  `tests/dsp/main.rs`).
- **T**: `crates/windfall-dsp/src/performance/extended_tests.rs` (existing lib
  test module).
- **N**: `apps/desktop/src-tauri/src/session/tests/p2_convolver.rs` (new,
  appended to `session/tests/mod.rs`; existing `tests/effects.rs::add_synth`
  gained sibling visibility only).
- **U**: `apps/desktop/src/features/effects/spectral/editors.test.tsx` (mounted
  effect editor, existing test discovery).

## Per row

### fx-pitch-shifter

Changed `crates/windfall-dsp/src/spectral/{pitch,mod}.rs` and `src/effect.rs`:
Pitch Shift observes the existing antiphase-safe YIN detector and selects
whole-period grain spans for voiced input. Its prepared span can hold two
periods at 60 Hz at the actual rate. The host compensation table and processor
agree on the expanded centre delay: 1026 frames at 44.1/48 kHz, 1602 at 96 kHz,
3202 at 192 kHz and 6402 at 384 kHz. No control index changed.

New tests/files:

- Q: `nonaligned_pitch_and_independent_formants_are_measured_in_both_modes_at_all_rates`.
- Q: `expanded_pitch_windows_preserve_public_latency_and_callback_heap_contracts`.
- B: `representative_vocal_mix_and_four_harmony_workloads_have_declared_spectral_cpu_budgets`.

Believed complete in source: broader non-period-aligned cents/formant acceptance,
both modes, high-rate grain storage and matching host latency. Tests span
8/44.1/48/96/192/384 kHz; pitch probes declare <25 cents and independent formant
centroids must rise by >10%. Existing narrower 375 Hz assertions remain intact.
SOFTWARE: execute these measurements and release CPU gate, especially high-rate
LPC behavior; resolve any failing bounds. EXTERNAL-ONLY: vocal/full-mix listening.

### fx-pitcher

Changed `crates/windfall-dsp/src/spectral/pitch.rs`: correction and all four
harmony grains prepare larger high-rate storage while keeping their existing
reported centre latency. The selected even-period span is no longer truncated
to 2048 samples at low fundamentals. Detector interpolation tolerates its
declared 60/1000 Hz endpoints without dropping a valid endpoint to unvoiced.

New tests/files:

- Q: `correction_and_all_four_harmonies_cover_sixty_to_one_thousand_hz_at_all_rates`.
- Q: `expanded_pitch_windows_preserve_public_latency_and_callback_heap_contracts`.
- B: `representative_vocal_mix_and_four_harmony_workloads_have_declared_spectral_cpu_budgets`.

Believed complete in source: correction and four simultaneous distinct MIDI
harmonies over endpoint and nonaligned 60–1000 Hz probes at all six test rates.
Correction declares <25 cents; isolated harmony projections must peak within
22.5 cents and exceed a minimum energy, so missing voices cannot pass.
SOFTWARE: execute quality/latency/allocator and release callback-cost gates.
EXTERNAL-ONLY: human vocal and physical MIDI/controller review.

### fx-transporter

Changed `crates/windfall-dsp/src/performance/buffer.rs`: replay reset invalidates
metadata and masks unwritten physical slots instead of clearing megabytes of
PCM. Onset capture keeps the existing constant-time ring swap.

New tests/files:

- T: `metadata_reset_masks_old_replay_pcm_before_and_after_partial_refill`.
- T: `representative_original_kick_snare_and_pick_transients_detect_without_sustain_retriggers`.
- B: `all_twelve_xy_models_and_transient_capture_reset_have_declared_callback_budgets`.

Believed complete in source: constant-time maximum-rate reset and representative
original kick, seeded snare and plucked excitation probes. Detector acceptance
requires exactly one detection per attack, within 3 ms, with no sustain retrigger
at 44.1/48/96/384 kHz. The timing workload also toggles manual capture edges.
SOFTWARE: run detector and release reset/capture budget measurements.
EXTERNAL-ONLY: physical MIDI timing/hotplug and listening on real music.

### fx-gross-beat

Changed the shared asset store, cached Volume Gate leases, Document/Edit roots,
Plan roots and Session retirement/slow snapshot files listed above.

New tests/files:

- A: `curve_leases_remain_valid_during_control_side_collection`.
- P: `replay_and_scratch_history_release_reclaims_only_discarded_gesture_curves`.
- N: `save_snapshot_retains_drawn_assets_across_project_replacement_and_collection`
  exercises the same shared store/snapshot contract using a drawn shape.

Believed complete in source: time/volume curves remain alive for history, clones
and plans, while merged/discarded history and project replacement permit
control-side reclamation. Existing sample-exact trigger/wrap/release tests are
unchanged. SOFTWARE: execute lifecycle and existing host/trigger regressions.
EXTERNAL-ONLY: real MIDI pad timing/hotplug and musical listening.

### fx-hardcore-11-guitar-fx

Added B's maximum-quality admission workload using every pedal enabled together,
all four original cabinets, 8x oversampling, 384 kHz and 1/64/256/2048-frame calls.
No pedal/cabinet behavior was replaced or reduced to satisfy a budget.

New test/file: B:
`maximum_quality_drive_guitar_and_modular_graphs_have_declared_e4_cpu_budgets`.

Believed complete in source: repeatable maximum eleven-pedal/cabinet workload
and declared 75% of one core budget. SOFTWARE: execute the release measurement;
if it exceeds the budget, optimize and rerun without reducing the workload.
EXTERNAL-ONLY: musical guitar/cabinet listening.

### fx-distructor

Added B's maximum 8x/384 kHz graph workloads: fuzz/chorus/cabinet,
lowpass/highpass/drive and three simultaneous cabinets. Parameters drive actual
separate modules, with maximum resonance/depth/rate/nonlinear stage gain.

New test/file: B:
`maximum_quality_drive_guitar_and_modular_graphs_have_declared_e4_cpu_budgets`.

Believed complete in source: reproducible modular CPU workloads and declared
75% of one core ceiling. SOFTWARE: record release cost and optimize any failure;
the three cabinet graph may be the limiting case. EXTERNAL-ONLY: listening.

### fx-effector-12-fx

Changed the shared replay reset and added B's twelve-model maximum-rate workload.
Each model warms a full captured ring and uses enabled XY, maximum feedback and
a maximum-length loop; one-frame and real device-size blocks are covered.

New tests/files:

- T: `metadata_reset_masks_old_replay_pcm_before_and_after_partial_refill`.
- B: `all_twelve_xy_models_and_transient_capture_reset_have_declared_callback_budgets`.

Believed complete in source: actual buffered treatment, reset and callback-cost
coverage for all twelve models with a declared 25% single-core ceiling.
SOFTWARE: run release budgets and existing exact host/XY/partition regressions.
EXTERNAL-ONLY: musical listening. Buffered speed/pitch models still make no
duration-preserving formant promise.

### fx-fruity-blood-overdrive

Added B's hot-mode 8x oversampling/384 kHz timing workload and a declared 20%
single-core ceiling. Existing measured transfer/headroom/alias tests are intact.

New test/file: B:
`maximum_quality_drive_guitar_and_modular_graphs_have_declared_e4_cpu_budgets`.

Believed complete in source: explicit maximum-quality E4 CPU measurement gate.
SOFTWARE: measure and admit the release CPU budget. EXTERNAL-ONLY: listening.

### fx-fruity-convolver

Added native Session acceptance in N and mounted import acceptance in U.
`session/tests/effects.rs` exposes its existing synth fixture to the new sibling
module, and `session/tests/mod.rs` registers N. Native acceptance publishes
twelve changing portable stereo IRs while an audition voice sounds, measures
their increasing live levels, checks exact undo/redo/offline captures, saves and
reopens, and refuses an 8000 Hz/65536-frame IR at the actual 48 kHz host before
document/history commit. Clear/undo restores the admitted asset.

New tests/files:

- N: `session_ir_replacement_undo_reopen_and_expanded_rate_refusal_preserve_document_and_audio`.
- U: `repeated WAV imports retain exact undo redo and saved reopen payloads for native Session publication`.
- U: `keeps the sounding IR and history when native publication refuses an imported WAV`.

Believed complete in source: the missing native Session publication/history/
reopen/refusal join and the mounted WAV payload/state workflow. The mounted
refusal uses a scripted backend rejection; the actual host expansion refusal is
exercised separately through production Session, not inferred from that mock.
SOFTWARE: run N and U with fresh simulator artifacts and the existing FIR/PDC/
portable preset suites. EXTERNAL-ONLY: imported/lower-rate IR listening; linear
IR resampling remains the documented quality limit.

### fx-fruity-scratcher

Changed the shared source/curve store, cached Scratch leases, independent Plan
roots, history roots and Session retirement/slow snapshot ownership.

New tests/files:

- A: `managed_sources_reuse_capacity_without_redirecting_reclaimed_handles`.
- A: `curve_leases_remain_valid_during_control_side_collection`.
- P: `replay_and_scratch_history_release_reclaims_only_discarded_gesture_curves`.
- E: `independent_scratch_plans_keep_sources_until_control_side_engine_retirement`.
- N: `save_snapshot_retains_drawn_assets_across_project_replacement_and_collection`
  covers shared curve snapshot ownership.

Believed complete in source: released source entries can reuse capacity beyond
the old 1024-entry lifetime limit without redirecting old handles; a second
engine keeps its own source until its independent owner retires. SOFTWARE: run
lifecycle/allocator and existing loaded-source/missing-source regressions.
EXTERNAL-ONLY: loaded-source listening. Sample-bearing portable plugin presets
remain explicitly refused, as recorded in the existing acceptance decision.

### fx-fruity-waveshaper

Changed the shared curve store, two-sided crossfade leases, document/history/
plan ownership and slow snapshot retention; added the maximum 8x CPU workload.

New tests/files:

- A: `curve_leases_remain_valid_during_control_side_collection`.
- D: `releasing_asset_roots_and_callback_leases_never_frees_on_audio`.
- P: `released_redo_and_replaced_documents_reclaim_curves_but_clones_and_undo_keep_them`.
- N: `save_snapshot_retains_drawn_assets_across_project_replacement_and_collection`.
- B: `maximum_quality_drive_guitar_and_modular_graphs_have_declared_e4_cpu_budgets`.

Believed complete in source: history/clone/crossfade-safe reclamation and a
declared 20% single-core CPU gate. SOFTWARE: run retirement/allocator/native
save-race and release CPU gates. EXTERNAL-ONLY: transfer-curve listening.

## CPU interpretation and sequential integration gates

B prints raw CPU share, maximum callback time, best and maximum reset time for
each actual model/graph/block. Release admission is: overdrive/waveshaper <=20%,
guitar/modular <=75%, transient/XY <=25%, spectral <=50% of a single core.
Reset must fit a 64-frame deadline at 384 kHz. Maximum callback/reset outliers
also have an explicit 5 ms scheduling allowance; this is not an xrun-free device
claim. One-frame calls are exercised/reported, but mean CPU admission uses blocks
of at least 64 frames. Debug builds use an explicit 64x ceiling for pathological
stall detection; **a debug pass cannot establish the release CPU budget**.

The integrator should format touched Rust source and run these sequentially,
in its integration checkout with the normal MSVC setup and a temporary
`TS_RS_EXPORT_DIR`:

```text
cargo test -p windfall-dsp --lib asset
cargo test -p windfall-dsp --lib drive
cargo test -p windfall-dsp --lib performance
cargo test -p windfall-dsp --lib spectral
cargo test -p windfall-dsp --test dsp
cargo test -p windfall-dsp --release --test dsp p2_effect_budgets -- --test-threads=1 --nocapture
cargo test -p windfall-project --test nonlinear_performance
cargo test -p windfall-project --test spectral_effects
cargo test -p windfall-project --test presets
cargo test -p windfall-engine --test engine nonlinear_performance
cargo test -p windfall-engine --test engine spectral
cargo test -p windfall-desktop --lib session::tests::p2_convolver
```

Also run existing native file/backup/archive/export/preset regressions because
their snapshot ownership changed, full relevant package suites, format checking
and scoped strict Clippy. Regenerate bindings/descriptors and simulator once
after merging, then run the existing nonlinear/performance and spectral mounted
UI suites, frontend typecheck/lint and artifact freshness checks. No generated
file edits from this worker need to be preserved.

## Least certain spots

1. `asset.rs::Slot::read` and `collect_retired`: the Arc/pointer acquisition
   protocol and final-store-owner rule were source-reviewed, never compiled or
   stress-run. Review sequential-consistency ordering, reuse/tombstones and
   callbacks concurrent with withdrawal. Rooted operations must never free on
   audio. Bare Copy handles do not grant ownership; audit any additional slow
   Project/preset snapshots if a new expired-curve refusal appears.
2. `spectral/pitch.rs`: the reused correction object supplies Pitch Shift's
   detector without running its resynthesis, but still owns prepared unused
   correction/harmony grain storage. Check preparation memory accounting and
   reset CPU. Whole-period spans, rate-expanded delay, detector endpoint
   tolerance and PDC need compiled live/offline acceptance at every rate.
3. Q's cents, harmony peak-energy and LPC centroid bounds are reasoned, not
   measured. High-rate LPC uses the inherited fixed 512-frame estimator window;
   the new 192/384 kHz formant probes may expose a quality shortfall. Fix DSP or
   a demonstrated measurement bug, rather than weakening existing assertions.
4. B's maximum cabinet/8x graph CPU and timer overhead are unmeasured. The
   declared release thresholds may expose a real performance deficit. Record
   raw numbers on a quiet machine, particularly three cabinets and all eleven
   pedals; optimize if required. Debug timing is not production evidence.
5. N's live gain monotonicity, audition preservation over undo/redo, file capture
   pause and short post-refusal sounding interval need native execution. U's
   dispatch spy assumes the existing backend proxy and await behavior; verify
   with regenerated WASM. The test module registrations are included.
6. Hand-edited Rust signatures/tuple destructuring, source lease dereferences,
   array/iterator type inference, TS test typing and source formatting have not
   met their compilers/linters. No formatter was run under the edit-only rule.

All implementation changes were committed before this hand-off was finalized.
The marker below means the edit-only delivery is complete; it is not a claim
that software acceptance ran or that parity rows can already be marked done.
HAND-OFF COMPLETE
