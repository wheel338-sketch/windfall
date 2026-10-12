# Round-four plugin export / Audio Unit hand-off — 2026-10-12

Worktree: `.claude/worktrees/r4-plugin-export`. Branch: `gpt/r4-plugin-export`.
Base: `cfd2d37e`. Source commits: `05ccfdb9` (export), `3c617d95` (AU),
`bac93a77` (design checkpoint), `c041ee75` (package portability).

**EDIT-ONLY: nothing was built or run.** No application, test, build/check,
Clippy, frontend tool, bindings/WASM generator, install, download, nested agent,
push, PR, tag or CI ran. Permitted source inspection included single-file
`rustfmt --check --edition 2024 --config skip_children=true` and Git. The
rustfmt inspections reported formatting differences; the selected files had no
syntax parse errors after the Cocoa cast comparison was corrected. These are
not type checks or passing format gates. Final `git diff --check` was clean.

Neither row is promoted to done. The implementations and tests below are source
candidates for the integrator; the remaining omissions are stated explicitly.

## `fmt-host-fl-as-plugin`

### Changes and completed source paths

- `crates/windfall-plugin/src/vst3.rs`: real `GetPluginFactory`, factory 2,
  combined component/audio processor/edit controller, VST3 module entry/exit,
  sixteen stereo output buses, rack MIDI events, host process context,
  bounded partial-stream state IO, and `IPlugView` lifecycle. Canonical FUID
  `57494E4446414C4C50524F4752414D01` has SDK COM ordering on Windows.
- `src/publication.rs`: off-audio preparation, one atomic pending engine,
  block-boundary swap, audio acknowledgement, control-side retirement and
  capture fence. Capture returns the fully committed old document until a
  replacement is acknowledged, then the complete new one. A second publication
  is refused while pending. Quiescent deactivation commits unconsumed loads.
  Invalid/preparation-failed loads preserve the previous program.
- `src/clap.rs`, `src/clap/gui.rs`, `src/panel.rs`: CLAP shares that publication
  path, active state load, cached latency publication, host main-callback
  collection, meter and tempo-ramp support. The Windows native control panel
  embeds under the actual host parent and loads an exported state packet.
  Each view unregisters its native window class before the DLL can unload.
- `src/package.rs`: staged/new-destination distribution directory containing
  CLAP and VST3 binaries plus program state, VST3 module metadata, macOS
  Info.plist/PkgInfo and installation instructions. Windows, Linux and Mac
  architecture layouts are explicit. Linux `renameat2(RENAME_NOREPLACE)` and
  macOS `renamex_np(RENAME_EXCL)` prevent replacing an existing directory;
  Windows directory rename also refuses existing targets. Corrupt bundled
  program state refuses plugin creation instead of loading an empty song.
- Desktop `session/plugin_export.rs`, `commands.rs`, and frontend
  `features/export/plugin-export-controls.tsx` / `export-dialog.tsx`: reachable
  export control, immutable checked project/decoded sample snapshot, actual
  native command, errors and retry. This export does not mutate history or
  project paths, so no musical undo command is needed.
- Engine `plugin_export.rs`, `processor.rs`, `rack.rs`, `voice.rs`, `clips.rs`
  and DSP `track.rs`: hard reset clears voices/clips, instruments/modular racks,
  effect slots, track EQ, metronome and compensation histories in place. Sample
  handles remain retained for the existing control-side retirement path.
- Clock arbitration: the host controls transport/tempo/meter. Internal tempo
  automation and navigation markers are preserved in state but suspended in
  the detached playback plan. Missing meter data retains the previous meter;
  initial meter comes from the project. CLAP tempo ramps render one-frame spans.

### Exact tests to run

| File | New or strengthened test |
| --- | --- |
| `crates/windfall-plugin/src/publication.rs` | `capture_fence_publishes_only_complete_applied_programs` |
| same | `quiescent_deactivation_commits_unconsumed_load_and_invalid_load_is_atomic` |
| `crates/windfall-plugin/src/package.rs` | `every_package_layout_contains_loadable_state_and_factory_metadata` |
| same | `missing_runtime_never_publishes_or_overwrites_a_destination` |
| `crates/windfall-plugin/src/clap/tests.rs` | `hard_reset_flushes_sampler_effect_and_compensation_audio_without_memory_calls` |
| same | `host_meter_override_is_transient_checked_and_preserves_saved_document` |
| same | `clap_boundary_handles_partial_state_streams_sample_offsets_and_lifecycle` now requires a successful active publication, refuses a second pending load, and retains measured PCM/allocation checks |
| `crates/windfall-plugin/tests/native_export.rs` | `exported_clap_and_vst3_load_through_own_host_audio_state_tempo_and_reset` |
| `apps/desktop/src/features/export/plugin-export-controls.test.tsx` | `reports desktop capability refusal without simulating a bundle` |
| same | `submits trimmed destinations, reports native failures and permits retry` |

The native integration target is automatically registered by Cargo. Its
`mod common` registers the checked PCM/song fixture. It follows the existing
native fixture pattern: build the actual export once into the target scratch
directory, or use `WINDFALL_EXPORT_FIXTURE` pointing at a prebuilt export DLL.
It packages and loads both formats through Windfall's own host, asserts sixteen
outputs/canonical VST3 identity, measures onset at sample 240 versus 120 for
120 versus 240 BPM, checks stereo polarity and reset silence, and round-trips
native state. Windows also opens/closes the actual embedded control panel.
Existing engine/CLAP onset, tempo bending, multibus PCM and stream tests remain.

### Remaining

- SOFTWARE: Export panels for Cocoa/X11 are not implemented; non-Windows
  panel creation explicitly refuses the platform. Bundle layouts and module
  entry points exist, but that does not provide those editors.
- SOFTWARE: The release/installer must ship the built `windfall-plugin` runtime
  and resolve it automatically. This command currently asks the caller for the
  runtime file; it verifies file presence and constructs layouts, not the ABI
  of an arbitrary caller-selected file. The native integration test supplies
  the real runtime, while layout unit tests deliberately use inert bytes.
- SOFTWARE: Native editor program loads do not yet notify host dirty state.
  VST3 active replacement also lacks the host restart/PDC notification when
  the newly prepared program changes latency. CLAP main-callback collection
  reports latency changes, but these broader editor/restart cases need completion.
- SOFTWARE: The earlier export restriction on nested native plugin bindings
  remains; an export-owned nested provider is still absent. Hardware recording
  routes remain deliberately refused because a plugin does not open devices.
- EXTERNAL-ONLY: After software gates pass, accept real distributed bundles
  in another DAW, including multibus routing, project recall, MIDI, preroll,
  meter/tempo changes, editor focus, and distributor signing/notarization.

## `fmt-host-au`

### Changes and completed source paths

- `crates/windfall-plugin-host/src/au_model.rs` is portable and registered in
  `src/lib.rs`. It supplies strict type/subtype/manufacturer parsing, catalog
  replacement/identity validation, canonical scoped parameter mapping without
  moving existing global IDs, bus/storage mapping, host meter/tempo/timeline
  conversion, transport-change detection, identity-bound `WAU1` state containers,
  and the provider state machine. The native adapter uses these models.
- macOS `src/au/mod.rs` enumerates installed AUv2 components; discovers and
  negotiates every input/output bus; owns each bus's planar buffer list and PCM;
  maps the selected sidechain; preserves mono/stereo conversion; installs
  beat/tempo, musical-location and transport callbacks; renders all native
  outputs at one timestamp; and retains callback contexts through failed setup.
  Existing stereo host facade consumes the first output; extra outputs are
  independently owned/rendered and discarded, not routed into extra mixer tracks.
- `src/au/cocoa.rs` embeds the installed Cocoa factory view in an owned NSWindow.
  It retains/releases factory/view/bundle objects, releases property-returned
  CF references, handles attached-window ownership, checks Cocoa main and
  refuses unsupported scale. Typed Objective-C calls distinguish object/void/
  BOOL returns and arm64/Intel NSRect returns. `sys.rs` declares system ABI only.
- Reset now requests owner recovery atomically and returns silence while pending.
  Owner return triggers native Reset while still initialized, followed by
  uninitialize/callback removal and later preparation. Reset failure remains
  an explicit recovery error. Native resources are never reset/disposed on audio.
- Desktop `plugins/au_owner.rs`, `plugins/runtime.rs`, `plugins/mod.rs`, `lib.rs`
  and the NSWindow parent seam in `commands.rs`: main-thread AU construction,
  controls, state and Cocoa lifetime; bounded one-tick dispatch proxy; direct
  execution for calls already on main; installed components in the normal
  picker; registered-identity creation and saved-state restoration using the
  existing ownership/fingerprint/render-provider facade.
- Project `plugin.rs` now accepts saved `au` bindings. No persisted enum,
  parameter registry or descriptor index was reordered. Missing AU state remains
  portable project data on Windows.

### Exact tests to run

All seven following tests are in `crates/windfall-plugin-host/src/au_model.rs`
and are available on Windows:

- `registry_identity_and_catalog_replacement_are_checked_and_deterministic`
- `scoped_parameters_keep_global_ids_and_do_not_alias_buses`
- `mono_stereo_and_sidechain_channels_have_independent_owned_storage`
- `state_container_rejects_wrong_component_truncation_and_trailing_bytes`
- `reset_requires_return_and_successful_control_side_recovery_before_render`
- `host_clock_maps_meter_beats_tempo_timeline_and_segment_downbeats`
- `transport_change_detects_stop_and_seek_but_not_normal_progress`

Project `crates/windfall-project/tests/plugins.rs` adds
`audio_unit_registry_bindings_survive_history_and_portable_project_reopen`.
The existing registered native Mac test
`au::tests::apple_effect_bypass_audio_parameter_state_and_processor_return`
is strengthened to require reset-pending silence without poisoned health,
actual owner return, successful reactivation and measured bypass PCM again.

### Precisely unverified / remaining

- SOFTWARE: All macOS adapter, Cocoa editor and desktop dispatcher code remains
  uncompiled/unlinked/unexecuted. The row cannot be called external-only under
  the completion rule until native Mac software checks pass. No additional
  omission is knowingly claimed in the assigned catalog/editor/transport/reset/
  bus-ownership source paths, but source inspection cannot prove them complete.
- Unverified: AudioToolbox property sizes/scopes and setup order; zero-input
  instruments and AU generators; scoped parameter metadata; host callback ABI,
  sample offsets and same-timestamp multi-output renders; callback removal
  after failed initialization; native reset refusal/recovery and held MIDI replay.
- Unverified: actual Cocoa factory discovery and CF ownership, BOOL/NSSize/NSRect
  selector ABIs on arm64 and Intel, Tauri `ns_window`/`run_on_main_thread` APIs,
  dispatch timeout/reentrancy/shutdown, attached focus/resize/close, view teardown
  before AU disposal, and installed instrument/effect project recall in the app.
- EXTERNAL-ONLY: After native software checks pass, a third-party AU corpus,
  device deadlines and operator interaction/listening acceptance on macOS.

## Integrator checks (not run by this worker)

Run serially with the repository's normal MSVC setup, Cargo jobs 1 and temporary
TS export directory. Refresh the lockfile for direct workspace dependency edges;
no new external package version is needed. Format the touched files before the
strict formatting/lint gates.

```sh
cargo build -p windfall-plugin --lib --jobs 1
cargo test -p windfall-plugin --lib -- --test-threads=1
# Set WINDFALL_EXPORT_FIXTURE to the built windfall_plugin.dll to avoid a nested build.
cargo test -p windfall-plugin --test native_export -- --test-threads=1
cargo test -p windfall-plugin-host --lib au_model::tests:: -- --test-threads=1
cargo test -p windfall-project --test plugins -- --test-threads=1
cargo test -p windfall-engine --features plugin-export --lib -- --test-threads=1
cargo test -p windfall-engine --features plugin-export --test engine -- --test-threads=1
# Also run existing desktop plugin ownership/capture/sidechain suites and export tests.
```

Frontend: run both `plugin-export-controls.test.tsx` and the existing
`export-dialog.test.tsx`, then desktop typecheck/lint. On a configured Mac SDK:
`cargo check --target aarch64-apple-darwin -p windfall-plugin-host -p windfall-plugin -p windfall-desktop`,
then the native AU test and desktop installed-AU/Cocoa/sidechain/recall workflows.
Cross-check Intel structure-return ABI separately; Windows parsing cannot do it.

## Shared-file edits / dependencies

Integrator must reconcile desktop `src-tauri/src/plugins/runtime.rs` with
`gpt/r4-hosting`: owner maintenance was extracted without changing its Windows
logic, request submission now supports direct Cocoa-main execution, and AU
creation is a format-specific branch ahead of the file loader. CLAP/VST3 host
backend modules, bridge internals and fixture classes were not restructured.

Other shared seams: engine `processor.rs`, `rack.rs`, `voice.rs`, `clips.rs`,
`plugin_export.rs`; DSP `track.rs`; project `plugin.rs` / plugin tests; host
`src/lib.rs`; desktop `Cargo.toml`, `commands.rs`, `lib.rs`, `plugins/mod.rs`,
`session/mod.rs`; frontend `export-dialog.tsx`. Dedicated files contain the
remaining additions. Generated bindings, simulator artifacts and parity files
were not edited.

Existing locked external packages reused: `vst3 = 0.3.0` and `windows-sys = 0.61`
(both MIT OR Apache-2.0). Export test edges reuse workspace DSP and plugin host;
desktop adds the workspace GPL engine export library. Cargo.lock already has
`objc2`, `objc2-app-kit`, AudioToolbox/CoreAudio wrappers and core-foundation
families; none was added to a manifest. Native AU uses in-repository framework
declarations and introduces no external dependency.

## Least certain spots

1. Uncompiled VST3 binding trait signatures/COM view types, Win32 control styles,
   host UI attachment, and export DLL symbol/lifetime behavior. Native regression
   onset expectations rely on the existing engine's checked sampler fixture.
2. Engine hard-reset coverage across all DSP kinds; the authored allocation
   guard measures the sampler/delay path, not every built-in processor family.
3. All handwritten macOS FFI and Cocoa ownership/structure-return ABI, AU
   scope-specific parameter discovery, and reset behavior in arbitrary units.
4. Desktop owner extraction/main-thread pump, especially capture cancellation,
   callbacks during shutdown and interaction with the integrator's hosting edits.
5. Packaging runtime validation/distribution, Linux no-replace symbol availability,
   Mac binary architectures and the explicitly missing editor dirty/PDC notifications.

HAND-OFF COMPLETE
