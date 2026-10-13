# Round-five SoundFont hand-off — 2026-10-13

Worktree: `C:/Users/ewhee/.t3/projects/windfall/.claude/worktrees/r5-soundfont`.
Branch: `gpt/r5-soundfont`. Base: `469be35c274f4e3d3dd47b6163f9da9609ed1c4e`.
Rows: `inst-soundfont-player`, `fx-fruity-lsd`.

This is an **edit-only, unverified implementation hand-off**. Nothing was built or run: no builds, checks through Cargo, tests, package-manager commands, binding/WASM generation, installations or downloads. No sub-agents, push, PR or tag. No dependency was added. No generated bindings, simulator artifacts or parity trackers were edited. Permitted inspection consisted of reads, searches, git diff/whitespace inspection, UTF-8 audits and single-file `rustfmt --check --edition 2024 --config skip_children=true` calls. Those calls found formatting differences, not executed type/runtime evidence; the invalid UTF-8 bytes they initially found in two new text fragments were repaired and the read audit repeated. The integrator must format and compile the complete branch before accepting either row.

Implementation commits:

- `f162c2dd` — SF2 synthesis, controller graphs, linked stereo/24-bit PCM, registered synthetic reference/allocator tests.
- `40c5a2e2` — project-owned raw bank sources, portable archive/relink UI, native MIDI bridge and history/recovery tests.
- `fc2503f5` — legacy ownership test also omits the new raw-source marker.
- `5d4955e1` — main-input MIDI routing and latency-aware PCM assertion.
- `cf22e0da` — stale PCM-job/source-type fencing and exact parsed-owner readiness retry.
- `49469da0` — preset pan survives stereo layout coalescing.

Acceptance source: [round-four acceptance](2026-10-12-round4-acceptance.md), Row acceptance cells for these two rows. Earlier hand-offs: [round-four samples](2026-10-12-r4-samples.md), [sample instruments](2026-10-11-sample-instruments.md). Implementation reference: the public [SoundFont 2.04 technical specification](https://www.synthfont.com/sfspec24.pdf), especially generator units/defaults/precedence, source curves/default modulators, synthesis and linked-modulator sections. No vendor bank, preset, sound or artwork is bundled.

## `inst-soundfont-player`

### Changes

DSP files: `crates/windfall-dsp/src/sample_bank/{soundfont,sf2_voice,asset,mod,params,tests}.rs`, new `sample_bank/tests/sf2.rs`, `src/instrument.rs`, and `tests/dsp/{main,params,sample_bank,sf2_fixture}.rs`.

The reader resolves every defined value generator in native SF2 units before publishing an immutable region. Instrument/global/local replacement and preset addition remain separate. Preset and instrument key/velocity ranges intersect. Sample/substitution generators remain instrument-only; reserved generators are ignored as prescribed. Fine/coarse start, end and loop offsets address the sample pool with checked bounds. Root override, fixed key/velocity, coarse/fine/scale tuning, pitch correction, attenuation, pan, exclusive class, no loop/continuous/loop-until-release modes are used by the actual voice. Linked headers are checked for reciprocal links, matching lengths/rates/pitch and meaningful loop alignment, then decoded into one stereo buffer with one oscillator position. Matching left/right zones coalesce into one voice. `sm24` reconstructs signed 24-bit values from the high word and unsigned low byte without discarding the low bits.

The dedicated SF2 voice uses timecents, absolute cents and centibels directly. It has delayed triangular modulation/vibrato LFOs, their specified pitch/filter/volume routes, a modulation envelope and a volume envelope with delay/attack/hold/decay/sustain/release and independent key-number hold/decay scaling. Volume decay/release uses the amplifier's dB transfer; modulation sustain uses tenths of a percent. Absolute-zero time sentinels survive offsets. The resonant two-pole low-pass converts cutoff from absolute cents and applies half-resonance passband attenuation. Chorus/reverb sends are two defined post-amplifier stereo buses into the existing Windfall chorus/reverb at full wet; their returns join the rack output and contribute tails. The project note pan law still applies independently.

Default modulators cover velocity attenuation/filter, pressure/mod wheel vibrato, CC7/10/11, CC91/93 and pitch wheel with sensitivity. Explicit instrument modulators replace matching defaults/global modulators; preset modulators add after their own global/local replacement. Linear/concave/convex/switch, polarity, direction, amount sources and linear/absolute transforms are evaluated. Linked modulators are prepared into topological graphs; cyclic routes are omitted while unrelated acyclic routes remain. Their callback evaluator uses a fixed 65,536-float stack scratch array rather than recursion or heap storage.

Prepared PCM, zone/region metadata, modulation lists and graph storage have aggregate admission. Graph owners take a lease before immutable construction; regions also conservatively charge their referenced graph bytes, so linked graphs can be charged more than once. Held/releasing voices remain Copy indexes/state inside the existing retained-bank/retirement scheme. The voice, controller, reset, choke and bank handover paths construct no heap owners on audio. The allocator test exercises a real parsed linked-stereo, 24-bit, generator/modulator bank, including a linked controller graph and send effects.

Persistence/native files: project `src/{model,sample_instrument,command,lower,check,file}.rs`; engine `src/{sample_instruments,controller,message,processor,rack}.rs` and `midi_hardware/message.rs`; native `src/commands.rs`, `session/{sample_instruments,sampler_processing,preparation,samples,archive,midi_hardware,controllers}.rs`; archive `src/lib.rs`. Existing `SampleAsset` literals in project arrangement/preset/slicer tests, native extraction recovery, engine fixtures/soak and plugin test helpers gained the default `soundfont: false` field only.

`SoundFont.source` is a defaulted project SampleId; its SampleAsset has a defaulted raw-bank marker, which also survives replacement of the referencing preset. `RegisterSoundFontSource` appends to Command and participates in ordinary sample history/patches. Native import parses the requested preset before committing source registration and selection as one checked batch. Legacy path-only projects acquire owned IDs when read. Effective paths come from the owned source, so source relink and save/archive path adoption do not change its identity. Raw banks are skipped by PCM decoding, including previously selected banks retained as project sources. Changing the source type invalidates cached PCM and clears its audio-load flags; already decoded audio jobs cannot attach PCM or mark the raw source failed. Readiness retries carry the exact parsed SF2 candidates forward instead of rereading changed bytes. Archive packaging copies their exact bytes through the existing bounded manifest/copy machinery and updates the captured snapshot's fallback paths. The live project is unchanged by archive export.

Native relink checks expected asset/path and generation/edit/replacement/source guards, parses every selected preset sharing the source into candidate storage, then commits one undoable path edit. Malformed or incompatible replacements and stale preparation leave history/document/audio unchanged. Reopened missing banks remain unavailable and report source IDs. Relative project/factory paths in the reopened editor resolve against the project root for browsing and preset selection; changing a preset in an archived bank preserves its source ID.

Frontend files: `features/sample-instruments/{editor,editor.test}.tsx`, `lib/ipc/{backend,tauri}.ts`, `lib/ipc/sim/document.test.ts`. The real editor uses native checked import, displays the source identity/missing status, and offers a dedicated “Relink SoundFont source” dialog with visible verification failures. Generic descriptor controls expose the appended MIDI controls after the original four parameters. The exhaustive command table includes registration. Main-input native MIDI CC/pressure/poly-pressure/pitch-wheel/RPN sensitivity reach the sounding SF2 unit through inline messages fenced by the existing hardware epoch; queue refusal is reported. Raw hardware state is transient; saved/automated descriptor controls use the existing checked parameter/history/export path.

### Exact new tests

`crates/windfall-dsp/src/sample_bank/tests/sf2.rs` — registered by `sample_bank/tests.rs`, which remains registered by `sample_bank/mod.rs`:

- `sf2_every_value_generator_defaults_units_ranges_and_preset_addition`
- `sf2_envelope_reference_timecents_centibels_and_key_scaled_hold_decay`
- `sf2_global_local_override_and_cross_level_range_intersection`
- `sf2_all_eight_sample_address_offsets_are_frame_units`
- `sf2_default_modulators_replace_at_instrument_and_add_at_preset`
- `sf2_source_curves_polarities_directions_amount_sources_and_transforms`
- `sf2_all_default_controller_destinations_match_reference_values`
- `sf2_voice_measures_fixed_key_velocity_scale_root_tune_and_lfo_pitch_depth`
- `sf2_stereo_linked_zones_make_one_phase_locked_voice_with_24_bit_pcm`
- `sf2_preset_pan_adds_after_stereo_channel_layout_and_keeps_one_voice`
- `sf2_negative_24_bit_reconstruction_and_hostile_lengths_links_modes`
- `sf2_nonlooping_headers_ignore_unused_loop_points_and_support_low_sample_rates`
- `sf2_delay_volume_envelope_loop_until_release_exclusive_class_and_partitions`
- `sf2_defined_chorus_reverb_returns_and_immutable_metadata_are_budgeted`
- `sf2_voice_filter_resonance_attenuation_and_pan_have_measured_reference_gain`
- `sf2_lfo_delay_frequency_and_pitch_filter_volume_routes_match_computed_steps`
- `sf2_raw_midi_rpn_pitch_pressure_arbitrary_cc_and_sustain_reach_sounding_voices`
- `sf2_linked_modulator_graph_sums_inputs_ignores_cycles_and_keeps_composition`
- `sf2_modulation_envelope_filter_and_lfo_volume_routes_match_pcm_reference`

`crates/windfall-dsp/tests/dsp/sample_bank.rs`, registered in `tests/dsp/main.rs`:

- `sf2_real_stereo_24_bit_modulated_voice_controls_chokes_reset_and_handover_are_allocator_free`

`crates/windfall-dsp/tests/dsp/params.rs`:

- `soundfont_controller_descriptors_append_after_saved_controls_and_legacy_defaults`

`crates/windfall-project/tests/sample_instruments.rs`:

- `soundfont_source_identity_relink_history_json_and_legacy_upgrade`
- `raw_soundfont_role_is_saved_after_preset_replacement_and_cannot_alias_audio`

`apps/desktop/src-tauri/src/session/tests/sample_instruments.rs`, existing registered module:

- `soundfont_import_owns_source_and_missing_relink_reopens_with_checked_history`
- `soundfont_relink_rejects_malformed_missing_presets_and_stale_publication_atomically`
- `soundfont_portable_archive_contains_exact_bank_bytes_and_opens_without_original_file`
- `soundfont_native_hardware_cc_changes_real_pcm_without_document_edits`
- `soundfont_registration_discards_cached_pcm_and_an_already_decoded_audio_job`
- `soundfont_readiness_retry_reuses_the_parsed_owner_without_rereading_changed_bytes`

`crates/windfall-engine/src/processor.rs`, inline registered tests:

- `stale_soundfont_midi_preserves_stopped_automation_hold`

`apps/desktop/src/features/sample-instruments/editor.test.tsx`:

- `validates SoundFont relink in its dedicated dialog and preserves source identity with undo`

The existing `browses bank/program presets and retains their file reference` test now asserts an owned source. Existing hardware message roundtrip tests include poly pressure. Existing SF2 truncation, budget/retirement, parameter, history and live/offline tests were retained.

SOFTWARE: No deliberately omitted behavior remains from this row's assigned Still-open cell. Build, test, formatting and regeneration gates are pending; this is not an accepted completion.

EXTERNAL-ONLY: musical listening, a physical MIDI controller/audio device and interoperability/listening with the operator's own licensed banks after the software gates pass. ROM-only sample content needs its external ROM; no ROM sound library is bundled or synthesized as a substitute.

## `fx-fruity-lsd`

The accepted round-four product decision remains the user-chosen SoundFont GM equivalent. Its bank uses the same reader, renderer, controllers, sends and source ownership described above. Native GM conversion now registers the raw file identity in its existing guarded one-step batch. Channel identity, notes and mixer destination stay with the channel; import/undo/save/reopen and live/export continue through the existing integration.

Files and new tests are the shared files/tests above. Also rerun the existing native `general_midi_soundfont_destination_preserves_rack_notes_history_disk_and_live_export_pcm` and `malformed_and_stale_general_midi_banks_leave_the_original_destination_unchanged`, plus the existing frontend GM capability/refusal test. The successful GM test now requires an owned SF2 source. These tests were not run here.

SOFTWARE: No deliberately omitted behavior remains from the assigned SF2-completeness dependency. Acceptance still requires the integrator's complete software gates.

EXTERNAL-ONLY: musical listening and physical MIDI/audio-device checks with a user-supplied GM bank. No OS-synth audio capture or bundled GM sounds are claimed.

## Integrator gates — not run

Regenerate bindings, descriptors and simulator artifacts after merging; the worker did not modify them. Changed persisted shapes: SoundFont asset source ID, optional SampleAsset raw-bank marker, appended SoundFont parameter fields and appended RegisterSoundFontSource command. Instrument/effect kind order and the first four SoundFont parameter indices are unchanged.

Run the registered SF2/lib tests and DSP allocator/parameter suites; project sample-instrument tests; native sample-instrument/archive tests; engine processor/hardware-message tests and the existing sample-instrument live/offline acceptance suite. Run frontend sample-instrument editor tests, exhaustive sim command tests, parameter access tests, typecheck/lint, and package-scoped strict Clippy after formatting. The old WASM cannot exercise the new checked command.

## Least certain spots

1. No Rust or TypeScript compilation has occurred. Native ticket/cache canonicalization, the new SampleAsset literal field and Command exhaustiveness need the integrator's compiler. The mechanical field changes intentionally cover every SampleAsset literal found in this tree.
2. Linked graph equality/composition and graph-owner plus region-budget accounting. Linked routes use 256 KiB of fixed stack scratch per evaluation; exercise the actual Windows callback stack and allocator test. Large graphs can be conservatively refused before publication.
3. SF2 controller endpoint conventions, absolute-zero times, convex envelope attack, resonance transfer and stereo zone coalescing. The written tests use independent units, first-impulse/prewarping, gain/frequency and phase-lock references, but their tolerances and resulting PCM have no executed evidence.
4. Native missing/relink/undo/redo and archive capture/extraction/source retention, including banks with a non-.sf2 extension and archived relative paths. Source guards, budget failure and stale publication need the headless session tests.
5. Appended descriptor controls and generated TS/WASM shapes, Base UI dialog typing, and strict Rust/TS lint. Single-file rustfmt checks report formatting differences which the integrator must reconcile.

All source and test changes were committed before this hand-off was written. Nothing was built or run; no tests or generators were executed.

HAND-OFF COMPLETE
