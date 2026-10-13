# Round-five plugin export hand-off — 2026-10-13

Worktree: `.claude/worktrees/r5-plugin-export`. Branch: `gpt/r5-plugin-export`.
Base: `469be35c`. Source commits: `ae086552`, `c8f6a4bd`, `43020c98`,
`ba2a280e`.

**EDIT-ONLY: nothing was built or run.** No application, Cargo build/check/test/
Clippy/run, frontend tool/test, bindings/WASM generation, installation, download,
sub-agent, push, PR or tag ran. Single-changed-file `rustfmt --check --edition
2024 --config skip_children=true` inspections found formatting differences but
no parse errors in the inspected files. Those inspections are not passing
format or type gates. Git whitespace checks were clean. The available shadcn
skill file was not inside the allowed worktree; the small UI change follows
the existing Field/Input/Button components without reading outside the contract.

## Row `fmt-host-fl-as-plugin`

### Implemented source paths

- `crates/windfall-plugin/src/runtime.rs`, `package.rs`, and host
  `src/export_runtime.rs`/`lib.rs`/`host.rs`: a C ABI/version/target stamp, native
  source/copied-binary probe, strict bundle-local bounded manifest admission,
  exact runtime lookup order, and clear own-host refusal diagnostics. The
  format binaries contain the complete engine runtime. No separate engine DLL
  is needed. Windows/Linux/Mac layouts include the runtime stamp and helper;
  universal Mach-O records require both valid nonoverlapping x64/arm64 slices.
  Native Mac fat detection preserves a universal manifest from either slice.
- Desktop `session/plugin_export.rs`, `commands.rs`, export controls and Tauri
  config: blank override resolves the distributed runtime from resources first,
  then executable-adjacent distribution locations. The real Backend/caller
  envelope and immutable project/sample export snapshot remain intact.
- `scripts/build-plugin-runtime.mjs` and release `source.mjs`, `candidate.mjs`,
  `release.test.mjs`: serial runtime/helper preparation, Unix execute bits,
  universal assembly, Tauri resource mapping, exact distribution version/ABI,
  target and checksum inventory, and candidate helper/installer-target admission.
  Pre-dev also prepares resources. Direct Cargo desktop checks must prepare
  these resources first; the Tauri command does so automatically.
- `publication.rs`, `clap.rs`, `vst3.rs`, Windows load call in `panel.rs`:
  editor loads distinguish user edits from host state recall. VST3 dirty,
  parameter-values, latency and reload restart flags use the host handler.
  CLAP dirty/rescan/restart callbacks use host extensions; latency changed is
  delivered while inactive. Different-latency loads stay pending until host
  restart, preserving old audio/PDC while a host delays restart. Capture retains
  the old committed program until actual publication/deactivation. Invalid or
  already-pending loads retain the old state and emit no edit notifications.
- Host `clap/instance.rs`, `vst3/instance.rs`, `vst3/mod.rs`: observe a latency
  change across CLAP reactivation and retain the export's VST3 refusal reason.
- `nested.rs`, `state.rs`, `lib.rs`, engine `plugin_export.rs`, `plugins.rs`,
  `rack.rs`, host bridge `auth.rs`/`adapter.rs`: independent export-owned native
  providers. Native helpers have fresh random namespaces even when two export
  DLLs are loaded by one process. Each binding has its own helper, immutable
  fingerprint, negotiated parameter table, callback intent fence and bounded
  note-owner table. Preparation reports the original failure rather than an
  attachment panic. Only control captures native state/parameters and reaps
  helpers. Callback facades own no supervisor/control handles. Reset invalidates
  old completion proof and flushes notes, PCM, dry buffers and correction ramps.
  Publication retirement owns helper teardown; save/reopen retains native state.

### Explicit provider policies

CLAP/VST3 Automatic or Bridged bindings work through isolated helpers. AU,
in-process/threaded requests, recursive Windfall exports, missing native files,
missing helper, and rejected native startup report their concrete reason. Native
third-party binaries are not copied into exports; their saved installed paths
must exist on the receiving machine. Hardware device recording is unavailable
inside a device-free plugin.

Nested providers require realtime scheduling because their callback never waits
for a process helper. CLAP declares the hard realtime requirement and refuses
offline render mode; VST3 refuses offline setup for such a program. Loading a
nested program while already in offline mode is refused atomically with a
realtime-bounce reason. Built-in-only programs retain offline mode. This is an
intentional provider refusal covered by authored tests and permitted by the
task's work-or-refuse rule.

### Exact authored tests for the integrator

All are non-ignored. New modules are registered in the respective crate roots;
Cargo discovers the two native integration targets. The Windows notification
module is explicitly registered by `tests/native_export.rs`.

| File | Test |
| --- | --- |
| `crates/windfall-plugin-host/src/export_runtime.rs` | `explicit_stamp_refuses_abi_version_target_and_unknown_fields` |
| same | `abi_entry_refuses_null_malformed_and_missing_stamp_fields` |
| `crates/windfall-plugin-host/src/bridge/adapter.rs` | `hard_reset_clears_held_notes_pcm_ramps_and_old_completion_proof` |
| `crates/windfall-plugin/src/package.rs` | `bundle_first_state_and_stamp_refuse_missing_malformed_abi_version_and_target` |
| same | `automatic_lookup_reports_all_locations_and_never_uses_invalid_preferred_runtime` |
| same | `universal_layout_requires_both_checked_nonoverlapping_architecture_slices` |
| same, strengthened existing | `every_package_layout_contains_loadable_state_and_factory_metadata` |
| same, retained existing | `missing_runtime_never_publishes_or_overwrites_a_destination` |
| `crates/windfall-plugin/src/publication.rs` | `editor_load_announces_dirty_parameters_latency_restart_and_refusal_is_atomic` |
| same | `nested_offline_mode_is_a_visible_refusal_and_retains_the_previous_program` |
| `crates/windfall-plugin/src/nested.rs` | `nested_provider_refusals_identify_format_policy_recursion_and_missing_helper` |
| same | `nested_note_ownership_controls_process_and_hard_reset_have_zero_callback_memory_calls` |
| `crates/windfall-plugin/tests/native_export.rs` | `runtime_probe_and_real_host_refuse_missing_unstamped_mismatched_and_corrupt_bundles` |
| `crates/windfall-plugin/tests/native_export/notifications.rs` | `native_editor_loads_emit_own_host_dirty_parameters_pdc_and_restart_notifications` |
| `crates/windfall-plugin/tests/nested_export.rs` | `owned_unstamped_fixture_and_missing_nested_helper_are_visible_refusals` |
| same | `nested_clap_and_vst3_in_both_exports_measure_audio_state_reset_and_helper_retirement` |
| same | `nested_gain_effects_preserve_stereo_and_measure_saved_parameter_attenuation` |
| `apps/desktop/src/features/export/plugin-export-controls.test.tsx` | `automatically resolves the distributed runtime when the override is empty` |
| `scripts/release/release.test.mjs` | `export-owned runtime resource inventory refuses missing helper, ABI/version and changed bytes` |

Retain the existing publication fence, CLAP lifecycle/reset/stream/allocation,
native export audio/state/tempo/reset, bridge authentication/ownership and all
release inventory suites. No assertion was weakened, test removed or tolerance
loosened. The layout-only unit builder uses inert bytes behind a private test
seam; those bytes never pass the public native runtime probe/export path.

Native tests measure 440 Hz output, positive RMS, exact negotiated helper latency,
stereo polarity, quarter-gain energy relative to an independently rendered
built-in reference, state recall, reset silence and confirmed control-side helper
retirement. Simultaneously active CLAP/VST3 export DLLs retain independent owners.
The VST3 owned gain fixture maps normalized gain to twice its value, so the test
requests 0.125 versus CLAP's linear 0.25 to measure the same quarter attenuation.

### Serial integration commands (not executed)

Use the repository's MSVC environment, jobs 1, temporary TS export directory,
and the integrator's serial machine policy. Format touched files before strict
gates. `node scripts/build-plugin-runtime.mjs` builds release resources; do not
invoke it concurrently with Cargo or other workers.

```sh
node scripts/build-plugin-runtime.mjs
cargo test -p windfall-plugin-host --lib export_runtime::tests:: -- --test-threads=1
cargo test -p windfall-plugin-host --lib bridge::adapter::tests:: -- --test-threads=1
cargo test -p windfall-plugin --lib -- --test-threads=1
# Set WINDFALL_EXPORT_FIXTURE to the actual freshly built Windfall DLL.
# Set WINDFALL_BRIDGE_HELPER to the freshly built windfall-plugin-audio executable.
# Set WINDFALL_BRIDGE_FIXTURE to the existing owned windfall_test_plugins DLL.
# These prevent fixture targets from launching nested Cargo builds.
cargo test -p windfall-plugin --test native_export -- --test-threads=1
cargo test -p windfall-plugin --test nested_export -- --test-threads=1
cargo test -p windfall-engine --features plugin-export --lib -- --test-threads=1
cargo test -p windfall-engine --features plugin-export --test engine -- --test-threads=1
node --test scripts/release/release.test.mjs scripts/release/workflow.test.mjs
pnpm --dir apps/desktop test src/features/export/plugin-export-controls.test.tsx src/features/export/export-dialog.test.tsx --maxWorkers=4
```

Also run desktop typecheck/lint, package/workspace strict format/Clippy and the
full required suites. Build actual installers and inspect installed
`plugin-runtime` resources, then export with a blank runtime override. On Mac,
check/run both native architecture slices, bundle/helper execution, Cocoa-main
ownership and the sibling editor; run corresponding Linux software gates.

### Completion and remaining work

The three assigned non-editor omissions have complete implementation candidates
and authored evidence. This is not a software acceptance or a row promotion.

- SOFTWARE: Integrator compile/link/test/format/Clippy and actual Windows
  installed-resource/export gates remain unexecuted. No additional knowingly
  missing behavior in this worker's three assigned items is claimed.
- SOFTWARE: Cocoa/X11 panels belong to `gpt/r5-nonwindows-editors`. Merge their
  work and ensure every editor program-load action calls `load_editor`, not
  host-recall `load`. This branch changes only the existing Windows call in
  `panel.rs`, not their platform editor implementations.
- EXTERNAL-ONLY: After software gates pass, real third-party DAW acceptance,
  physical deadline/listening checks, receiving-machine native plugin installs,
  and distributor signing/notarization. Another OS's execution remains external
  to this Windows edit-only worker; native software gates are still mandatory.

### Integration seams and dependencies

Reconcile `panel.rs` and any sibling `clap/gui.rs` changes around the new
`Publication::load_editor` seam. Reconcile additive `HostedEffect::reset_hosted`
with hosting workers. Desktop command, Tauri config, and release resource
contracts are narrow shared seams. Runtime construction/capture never borrows
the desktop native owner or a speculative offline provider.

No new external package/version was added or downloaded. Existing workspace
GPL-3.0-or-later plugin-host and DSP edges move from dev to normal dependencies
of `windfall-plugin`; host's already locked ISC libloading performs probes.
No generated bindings, simulator, parity files or other worktrees were edited.

### Least certain spots

1. Uncompiled FFI notification/render-extension signatures, shared VST3 handler
   lifetime and host restart sequencing. Verify actual Windows editor messages
   and latency notifications across CLAP deactivate/reactivate.
2. Native helper startup/parameter negotiation, inactive VST3 capture and reset
   proof, plus scheduler-sensitive nested sine/gain evidence. Do not weaken the
   measured assertions if these fail; diagnose ownership/timing instead.
3. Tauri pre-dev/pre-build target environment, resource copying for direct Cargo
   desktop checks, installer target inventory, and universal lipo/execute bits.
4. Mac CFBundle/dlopen handling of runtime probes and universal slices, and
   Linux/Mac helper GUI/lifecycle behavior. Only portable layout logic has been
   inspected here; nothing has been typechecked or executed.
5. Formatting and strict Clippy for the authored compact Rust/JS additions.
   The allowed rustfmt checks reported differences and are not passing gates.

HAND-OFF COMPLETE
