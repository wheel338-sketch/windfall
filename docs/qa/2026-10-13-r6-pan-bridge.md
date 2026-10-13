# Round-six pan bridge hand-off - 2026-10-13

Worktree: `.claude/worktrees/r6-pan-bridge`. Branch: `gpt/r6-pan-bridge`.
Base: `c0144d31`. Row: `wf-project-settings`.

This is an **edit-only, partial source hand-off**. It does not complete all four
assigned items and is not acceptance evidence. Keep the row in progress.

Implementation commits:

- `1cf6ff28`: versioned helper project-expression timeline, shared ramps,
  bounded parameter recovery, native fixture and helper tests.
- `943eb068`: desktop integration, actual-channel validation, independent
  native destination admission, capability panel and measured/native/UI tests.

Nothing was built or run as acceptance evidence. No build, compiler, Cargo,
test, application, package runner, generator, install or download was run.
Only reads/edits, Git and permitted single-file `rustfmt --check --edition 2024
--config skip_children=true` inspections ran. The checks reported formatting
differences in the larger edited files; no passing formatter gate is claimed.
`git diff --check` passed. Hooks were disabled for commits to honor the worker
contract. No agents, pushes, PRs or tags were created. No dependencies,
manifests, lockfiles, generated bindings, simulator artifacts or parity trackers
were changed. New tests are registered and non-ignored.

## Changes and current completion

### 1. Isolated native project-law/expression ramps

Source implemented for processors advertising native instances, Pan and Volume
on the actual note channel. `crates/windfall-plugin-host/src/project_expression.rs`
provides the common frame-clock ramp used by the desktop in-process host and
the helper. Pan, volume, gain and mapped expression changes use five
milliseconds; removed mapped destinations ramp toward their neutral values.
Pan-law gains are computed at control changes and the frame path uses prepared
state. The first ramp frame retains the preceding gain.

Changed host files: `src/bridge/{protocol,slots,adapter,auth,helper}.rs`,
`src/{adapter,processor,lib}.rs`, the new `project_expression.rs` and registered
`bridge/helper_ramp_tests.rs`. `windfall-dsp/src/{lib,note_pan}.rs` exposes the
same project-law type and adds position retargeting/settled queries.
`windfall-plugin/src/nested.rs` supplies the new snapshot field for its legacy
controls path. Wire and fixture compatibility tests were updated without
weakening any tolerance.

ABI10 carries explicit typed expression masks and all seven f64 controls in
25-word note snapshots, plus the starting project law and separately reserved
timed law changes. Hello4 rejects older helper Hellos before Load disclosure;
startup diagnostics name the required versions. Mapping slots are 602,432 bytes;
the mapping is 2,409,984 bytes. Recovery and timed events retain explicit note
channels and IDs. Helper replay/ramp events use a prepared native reserve.

The helper's parameter-uncertainty set is now bounded inline storage. Snapshot
parameter recovery uses prepared native parameter events rather than allocating
processor buffers through deactivation/re-activation in `Native::process`.
Control/state capture still uses its existing off-audio ownership operations.

Desktop `plugins/{runtime,bridge}.rs` sends raw mapped intent to the bridge,
applies the law in the helper once, shares NoteRamp on the direct path, and
removes the unconditional bridge `projectPanLaw=false` override. The bridge now
implements `set_expression_map` and `set_note_pan_law`, including sounding notes.
The bridge document has an ABI10 source-contract section.

The owned VST3 fixture appends class index26 / ID
`0000000000000000000000000000001B`, Project Expression Probe, with native Volume
in addition to the existing expression probe kinds. Existing class IDs and the
Pan-without-Volume class23 retain their behavior. Scanner/macOS bundle fixture
counts now assert 27 and the scanner asserts the new class is an instrument.

### 2. Actual note and live MIDI channels

Source implemented: capability validation no longer clamps an invalid channel
or assumes channel zero. `plugins/note_capabilities.rs` publishes observed
channels through fixed atomics and a visible invalid-channel refusal. Both
desktop providers refuse invalid color groups before sanitization. Expression
updates retain their original sounding owner's channel.

`session/edit.rs` checks each authored note color channel for the relevant
instrument, plus observed live MIDI channels, after dropping the document
guard. It uses zero only when no authored/observed channel exists. The panel and
expression-map inspector use the same authored/observed channel selection;
capability polling keeps revision/project-generation guards. Existing checked
document commands, persistence and undo/redo remain the edit path.

The new measured native test asserts a color-group owner on channel3, live MIDI
on channel9, no voice for invalid channel16, audible PCM and the published
refusal. Mounted UI tests cover authored channel5 and observed live channel3,
rather than substituting channel zero.

### 3. Independent native Pan/Volume admission

Source implemented only for the supported native destination: Rust and UI now
check Pan-only and Volume-only map requests independently. The actual channel's
other capability is not required merely to admit that destination. The panel
shows separate Pan and Volume native/unavailable routes.

SOFTWARE: The complementary per-note host route is not implemented. Law
compensation still requires the complete native instance/Pan/Volume trio.
A Pan-only or Volume-only processor therefore does not yet get the missing
operation after its individual voice, and its project-law setting can still
be refused. Implement prepared, independently addressable voice audio before
claiming host fallback. The current adapter exposes already-summed plugin
outputs; multiplying those outputs would change every overlapping note and
would not satisfy the requested behavior. All four mixed-route PCM and
no-double-application fixture combinations remain required.

### 4. Dynamic capability loss and return

Source implemented only for UI reporting: the real polling panel announces
capability changes and independently updates each displayed native route.
The mounted test exercises loss and return on the actual observed channel.
Existing round-five native re-admission is retained.

SOFTWARE: Seamless audio migration to/from host fallback after losing Pan,
Volume or instance capabilities is not implemented. Sounding-owner preservation,
block-boundary route crossfades and measured CLAP/VST3 mid-note click bounds
remain required. The helper recovery snapshot retains desired targets, not the
full in-flight ramp phase; preserve current/target/remaining ramp state when
implementing this migration. UI notification is not audio fallback evidence.

## Exact new and extended tests for the integrator

All names below exist in source but were not executed.

- `crates/windfall-plugin-host/src/project_expression.rs`:
  `project_expression::tests::project_law_and_expression_ramps_follow_the_same_240_frame_clock`.
  Independent gain reconstruction and ramp endpoints for all four laws.
- `crates/windfall-plugin-host/src/bridge/slots.rs`:
  `bridge::slots::tests::v10_law_timeline_and_full_expression_snapshot_roundtrip_and_validate_channels`.
  Full f64 controls, timed laws, channel14 preservation and invalid channel/law
  refusal. Existing exact byte-layout assertion is updated for ABI10.
- `crates/windfall-plugin-host/src/bridge/protocol.rs`:
  `bridge::protocol::tests::earlier_mapping_versions_never_downgrade_the_v10_project_expression_contract`.
  Refuses mappings1 through9. Existing authenticated-helper process test now
  refuses Hello1/2/3; its metadata nondisclosure assertions remain.
- `crates/windfall-plugin-host/src/bridge/helper_ramp_tests.rs`, registered under
  `helper.rs` on Windows:
  `bridge::helper::ramp_tests::helper_project_laws_expression_ramps_and_parameter_events_measure_pcm_without_allocator_calls`.
  Actual Native helper owner processing, CLAP and VST3, all laws, within-block
  frame13 law changes, frame7 volume changes, analytical PCM at unchanged
  2e-6 tolerance, zero host alloc/realloc/free calls, timed parameter uncertainty
  insertion/retirement and changed snapshot recovery without re-activation.
- `apps/desktop/src-tauri/src/plugins/bridge.rs`:
  `plugins::bridge::tests::bridged_and_in_process_project_laws_and_live_expression_ramps_match_pcm`.
  Real helpers versus the desktop direct adapter, CLAP/VST3, streaming/offline,
  all laws, overlapping owners on channels3/5, live law/pan/volume/gain changes
  and map destination removal; PCM difference below the existing 2e-6 bound
  after declared bridge latency. Direct/streaming callback allocator guards.
- `apps/desktop/src-tauri/src/plugins/ownership_tests.rs`:
  `plugins::runtime::ownership_tests::native_colour_group_and_live_midi_channels_are_observed_and_invalid_notes_are_refused`.
- Extended `plugins::note_capabilities::tests::native_pan_law_requires_all_three_capabilities_on_the_actual_channel`:
  independent Pan-only/Volume-only admission and invalid-channel refusal.
- `apps/desktop/src/features/plugins/note-capabilities.test.tsx`:
  `follows the actual live MIDI channel and announces capability loss and return`;
  `uses the saved note colour channel when the plugin supports a different channel from zero`.
  Existing capability truth-table test also asserts independent destination
  admission and rejects negative/fractional/out-of-range channels.

Supply the newly rebuilt fixture as `WINDFALL_BRIDGE_FIXTURE` and the newly
rebuilt current desktop/helper as `WINDFALL_DESKTOP_BRIDGE_HELPER` before native
tests. The new helper-owner test requires its prebuilt fixture and never starts
nested Cargo. Format the source, compile and run these focused modules, then
the existing plugin-host bridge/realtime/dynamic capabilities/capture suites,
desktop ownership/state/retirement tests, the existing native pan-law test,
project persistence/undo/settings suites, project-info panel and expression-map
inspector suites, desktop typecheck/lint and the complete prescribed gates.
Refresh local generated/mock artifacts through the integrator workflow.

## Least certain spots

- No compiler or test evidence exists. Rust borrow/type errors, test allocator
  registration and the new fixture metadata need the first integration build.
- Native parameter snapshot recovery now uses audio events; run the full state,
  dirty preset, capture-proof and reset recovery oracles to check that accepted
  control intent and native state retain their previous distinctions.
- Real streaming helper scheduling, fixed/threaded wrappers, native event
  reserves, callback partitions and cost with many simultaneously ramping owners
  need measured evidence. The streaming test uses its test thread as the device
  clock; the callback itself never waits. Its tolerance was not loosened.
- Destination removal, initial fractional tuning, native law normalization and
  latent fixture output alignment have only been inspected. Run both the new
  equality tests and the unchanged round-five native PCM reference.
- Mixed native/host routing and click-bounded capability loss/return are actual
  missing implementations, not merely uncertain test outcomes.

SOFTWARE: This row remains in progress until the two missing audio behaviors
above and their required measured route/capability fixtures are implemented,
then all current and new software gates are built and passing. Declared MPE
per-note channel behavior should also receive focused native fixture coverage;
the new live-channel test covers actual routed note-on channels, not a complete
MPE controller gesture sequence.

EXTERNAL-ONLY: Physical audio/MIDI devices, independent listening, vendor
native expression transfer/declaration checks and actual other-OS execution,
after the software work and development-machine gates pass.

All implementation changes were committed before writing this hand-off. This
record commit adds only the hand-off document. Its terminator records committed
delivery of this partial source packet, not completion or acceptance of the row.

HAND-OFF COMPLETE
