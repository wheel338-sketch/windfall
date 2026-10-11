# Slice, grain and curve instruments — final implementation hand-off

Branch: `gpt/w3-slices-grains`; base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
Last implementation commit: `392d2de9` (this final hand-off is a subsequent docs commit).
Earlier pieces: `b1ca47d4` assets/DSP/hosting; `e1170f09` editors/detection/tests;
`8caf138e` lifecycle/replacement guards; `78398483` visible key bounds;
`65d16ad8` seam/content docs; `b9840430` legacy key/release preservation.

**Edit-only:** no build, check, test, generator, browser, audio device or hardware
run. Rustfmt was used on owned Rust files. Every test listed below is **written,
not executed**. No generated artifacts, parity status, dependencies, PRs or tags
were changed. No sub-agents were used.

## Per-row claims

| Row | Claim | Distinct behavior implemented | Remaining acceptance |
| --- | --- | --- | --- |
| inst-fruity-slicer-2 | Full-row candidate | Slice Map/Deck independent key triggering, shared transient/beat-grid detection and generated note patterns | Combined compilation, fresh bindings/WASM, tests and native walkthrough |
| inst-fruity-slicer | Full-row candidate | Slice Map original-source regions, keyboard mapping, manual split/boundaries/reordering and pattern generation | Same integration gates |
| inst-slicex | Full-row candidate | Slice Deck per-slice gain/pitch/reverse/attack/release, overlapping/reordered regions, two independent source decks and dedicated editor | Same gates; listening on musical loops |
| inst-fruity-granulizer | Full-row candidate | Grain Cloud size/density/spacing/position/spray/pitch/seed controls, bounded seeded note-local grain scheduling | Same gates; native deadline/listening evidence |
| inst-wave-traveller | Full-row candidate | Wave Ride drawn shared/per-key position or analytically integrated speed curves, duration/pitch controls and dedicated editor | Same gates; native drawing/playback walkthrough |

These are implementation candidates against the row summaries, not accepted parity
closures. Shared limitations: portable sample-bearing channel preset export is
explicitly refused; plugin presets contain scalar controls only. Legacy automation
of retired asset descriptor suffixes is explicitly refused rather than reassigned.
Project persistence/history and channel duplication retain complete authored assets.

## End-to-end implementation

Existing SliceMap, SliceDeck, GrainCloud and WaveRide variants/ALL lists are reused;
no instrument kind or registry index was inserted or reordered. Their scalar
parameter prefixes retain original indices. PCM, markers, envelopes and curves are
removed from Copy params. Optional Channel.samplePlayback is omitted when absent.

Saved SamplePlayback references original sources A/B, 1–128 arbitrary SliceRegions,
a shared curve and sparse per-key curves, source BPM/beat count and optional legacy
inline PCM. PlaybackAsset retains original decoded AudioBuffers, preserving stereo
and original length (up to 32 million finite frames), with control-side validation.
Curves have 2–256 ordered finite points. Grain state is fixed at 16 voices × 32
slots; overflow steals/counts voices or drops/counts grains deterministically.

SetSamplePlayback and GenerateSlicePattern are checked history commands. Source
removal is refused while either deck references it. Duplication/save/reopen retain
assets; incompatible instrument replacement clears assets in the same undo step.
Native detection snapshots under recording → State ordering, detects/compiles
outside those locks, then rechecks generation, revision, edits and all source
identities before publishing one edit. Browser detection invokes the same Rust
export in a worker; simulated browser audio is labeled explicitly.

Prepared asset/source identity joins unit reuse admission. Replacement prepares a
new unit off-thread, then copies fixed active notes/voice/grain/control-clock state
without asset ownership changes in the callback. Retirement uses existing queues.
Held slice notes retain captured settings; asset edits affect subsequent notes.
Source reload/curve edits preserve progress while using the new prepared source.

Legacy Channel deserialization migrates old mono tables/maps/eight-point curves.
Old modulo-key mapping and global release survive through optional legacyKeyWrap
and migrated 30 ms region release. Editor/detection authoring selects the new
bounded mapping. Deck B assignment retains a migrated inline deck A. Ordinary old
channels retain exact default JSON. Retired suffix automation is refused with a
specific diagnostic, leaving the original file intact.

The instrument section remounts by generation/channel/kind; obsolete detection
cannot retain busy/selection/peak state when IDs repeat after New/Open. Dedicated
slice, grain and curve controls use existing shadcn/Base UI components and scalar
parameter automation bindings. Pointer drawing commits once per gesture; cancel
publishes no asset. Numeric curve editing guards ordering/endpoints and no-op blur.
MIDI-key overflow is visible and blocks invalid pattern generation.

## Test inventory — all written, not executed

`crates/windfall-dsp/src/granular/tests.rs`:

- immutable_sources_exceed_4096_frames_and_slice_boundaries_never_bleed
- reverse_pitch_gain_overlaps_reorder_and_second_deck_are_independent
- seeded_grain_schedules_repeat_and_respect_density_capacity
- drawn_position_and_integrated_speed_curves_match_independent_reference
- per_note_curves_release_and_partition_invariance
- scalar_params_exclude_assets_and_keep_prefix_indices
- invalid_assets_fail_before_publication
- migrated_maps_preserve_modulo_keys_and_global_release_without_changing_new_maps

`crates/windfall-dsp/tests/dsp/sample_playback.rs` (registered in dsp/main.rs):
`populated_sample_instrument_callbacks_never_allocate_or_free`.
`tests/dsp/params.rs`: `slice_grain_curve_parameter_prefixes_are_stable_and_sources_are_assets`.

`crates/windfall-project/tests/sample_playback.rs`:

- asset_edits_pattern_generation_undo_redo_and_file_roundtrip
- source_and_curve_validation_rejection_is_atomic
- transient_detection_on_authored_stereo_and_beat_grid_follow_source_tempo
- legacy_v1_source_map_and_curve_migrate_without_pcm_in_copy_controls
- old_v1_channels_without_assets_keep_exact_json_and_defaults
- scalar_automation_survives_but_legacy_asset_suffix_is_explicitly_refused
- replacing_instrument_kind_clears_incompatible_assets_and_undo_restores_them

`crates/windfall-engine/tests/sample_playback.rs`:

- every_sample_instrument_live_offline_history_and_reopen_pcm_match
- source_reload_and_asset_edits_rebuild_same_kind_without_stale_asset_reuse
- release_is_silent_at_export_tail_and_missing_sources_are_silent
- seeking_to_a_note_boundary_restarts_deterministically_and_mid_note_seek_does_not_chase

`tests/engine/sample_playback.rs` (registered in engine/main.rs):
`prepared_assets_adopt_active_notes_and_retire_without_callback_allocation_or_free`.

`apps/desktop/src-tauri/src/session/tests/sample_playback.rs` (registered):

- detected_playable_slice_map_is_one_history_step_and_reopens_original_source
- detection_rechecks_edits_source_and_document_replacement_after_off_lock_work
- obsolete_detection_request_and_invalid_options_leave_document_unchanged

`apps/desktop/src/features/sample-playback/editor.test.tsx`: mounted cases for
migrated deck A retention, slice splitting/gain/reverse/reordering/undo, pattern
history, density gesture, curve numeric edits, per-key overrides and replacement
remount during pending detection. `curve-editor.test.tsx`: multi-move gesture commits
once; pointer cancel publishes nothing. `detection.test.ts`: authored transients
through actual shared Rust/WASM, history and stale guards. `params/access.test.ts`
pins scalar prefixes/assets; sim/document.test.ts adds small valid EVERY_COMMAND
examples for both appended command variants.

## Shared seams touched

- DSP: src/instrument.rs; tests/dsp/{main,params}.rs. Granular logic stays in its
  owned directory. Existing exhaustive registrations already contained all four.
- Project: src/{model,command,lower,check,lib,preset,layer}.rs; new asset/lowering/
  migration modules. ChannelWire must be reconciled with any concurrent Channel
  fields. preset.rs also contains the explicit export refusal/incompatible clear.
- Engine: src/{lib,plan,state,rack,project_preparation}.rs and new sample_playback.rs;
  tests/engine/main.rs. Remaining engine controller/sequencer/stems/soak/test edits
  only supply the new optional Channel field in existing literals.
- Desktop: commands.rs, session/mod.rs, session/tests/mod.rs; new session module.
  IPC {backend,tauri,mock}.ts; sim/{slicer.ts,document.test.ts}; channel-rack inspector
  instrument-section.tsx; params/access.test.ts.
- Simulator: src/{lib,slicer}.rs exports sample_playback_detect. No new IPC crate
  type or generated source file was hand-edited.

## Regenerate on the combined branch

New exported types: SamplePlayback (including optional legacyKeyWrap),
InlinePlaybackSource, KeyPlaybackCurve, SliceRegion, PlaybackPoint, PlaybackCurve,
PlaybackCurveMode. Changed: Channel, Command, SliceMapParams, SliceDeckParams,
GrainCloudParams, WaveRideParams and their dependent exports/barrel/descriptors.
Remove stale generated table/map/position fields; preserve scalar prefix order.
Rebuild windfall_sim.wasm and its metadata with the appended detection ABI.

## Exact integrator gates — not run here

Use Git Bash on Windows and bounded UI workers. Regenerate real combined artifacts
before frontend checks, then keep ordinary TS exports in a temporary directory:

```bash
source scripts/msvc-env.sh
scripts/gen-bindings.sh
scripts/build-sim.sh
node scripts/check-bindings.mjs
node scripts/check-sim.mjs
task_bindings="$(mktemp -d)"
export TS_RS_EXPORT_DIR="$(cygpath -w "$task_bindings")"
cargo test -p windfall-dsp --lib granular
cargo test -p windfall-dsp --test dsp sample_playback
cargo test -p windfall-dsp --test dsp params
cargo test -p windfall-project --test sample_playback
cargo test -p windfall-engine --test sample_playback
cargo test -p windfall-engine --test engine sample_playback
cargo test -p windfall-desktop --lib session::tests::sample_playback
cargo test -p windfall-project --lib --tests
cargo test -p windfall-engine --lib --tests
cargo fmt --all --check
cargo clippy -p windfall-dsp -p windfall-project -p windfall-engine -p windfall-sim -p windfall-desktop --all-targets -- -D warnings
pnpm --dir apps/desktop test src/features/sample-playback src/features/params/access.test.ts src/lib/ipc/sim/document.test.ts --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```

Then perform native import/select/detect/edit/keyboard/pattern/curve/save/reopen/
undo/redo/export walkthroughs for each kind and musical listening/deadline checks.
Fresh WASM is mandatory: the old artifact cannot execute these commands/types.

## Risks in check order

1. Uncompiled Rust/TS integration and all Channel literals/ChannelWire fields after
   parallel merges; generated optional fields and descriptors must match callers.
2. Asset identity/replacement/adoption/retirement with active notes; allocator guards
   must run in both DSP and whole engine. Native validation currently prepares
   assets twice off-thread; direct low-level plan compilation treats invalid assets
   as absent. Normal checked session publication rejects them before mutation.
3. Legacy deserialization/key wrapping/release and explicit suffix automation
   refusal; imported files with removed automation lanes require user review.
4. Exact live/offline PCM, seek-boundary phase/latency, source reload fade draining,
   transient tolerances, pointer/Select mounted cases: none has executed.
5. Portable sample-bearing channel presets remain unsupported and explicitly
   refused. Plugin presets retain only scalar controls. Existing seek policy does
   not chase notes begun before seek; no new note-chase behavior is claimed.
6. Linear interpolation may alias at extreme pitch; 16 voices/32 slots and source/
   curve/region limits are deliberate. Native performance/listening remains open.

Own CC0 example settings use existing factory audio:
`content/examples/Slice and Grain Study/README.md`. No generated factory files were
edited. An earlier combined shell edit proposing new example-WAV creation was
rejected by automatic approval review; its only stated reason was blocked by policy.
Existing-content/text examples were completed instead. No WAV generator was run.
