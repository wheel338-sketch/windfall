# Plugin hosting implementation hand-off — 2026-10-11

Branch: `gpt/w3-plugin-hosting`. Worktree: `.claude/worktrees/gpt-plugin-hosting`.
Implementation checkpoint: `03cb45b2`; the final documentation commit follows it.
**Source-only: no build, check, test, Clippy, device check, nested agent or CI ran.**
Rustfmt was used on touched Rust files. Every completed file group is committed.
Tracker/generated artifacts remain owned by the overseer/integrator.

## Per-row accounting

| Row | Claim | Implemented in this stream | Remaining gates |
| --- | --- | --- | --- |
| `fmt-host-vst3` | partial | Bridge note ownership/expression negotiation/replay; numeric SDK event-list correction and bounded translation storage; native scaling/owner settings; independent expression fixture and PCM regressions. | Compile/run new regressions, dynamic restart/capability re-preparation, external editors/plugins, Mac/Linux acceptance; coordinate multi-output routing with mixer owner. |
| `fmt-host-clap` | partial | Bridge owner/expression replay; identity-aware sine fixture with measurable pan/tuning; helper editors; saved wrapper DSP/lifecycle. | Compile/run regressions, dynamic restart/capability re-preparation, external corpus/editor acceptance, Mac/Linux desktop owners and multi-output routing. |
| `fmt-host-bridging` | partial | ABI 7 full held-key/instance/control snapshots, actual capabilities, guaranteed release/recovery reserves, native helper GUI/dirty capture and retry latch, wrapper policy in Load/reactivation. | New native crash/hang/state/editor/replay suites, capture/ack/restart races, packaged helper discovery, external corpus, non-Windows IPC/provider. Existing ready-session composition gates in process-bridge.md also remain. |
| `fmt-host-wrapper-options` | partial | Validated saved smart disable/fixed buffers/owner/editor policy, undo/clone/reopen command, engine/runtime identity, real in-process/bridged choice, purpose-built controls and mounted tests. | Threaded owner provider is unimplemented and explicitly refused/disabled; validate DSP/PDC/native editor behavior, state preservation during active policy edits, history/replacement races and live/offline equivalence. |
| `fmt-host-au` | partial | macOS-only AUv2 adapter behind existing host traits: registered identity, parameters, stereo Float32/MIDI, plist state, latency/tail, owner lifecycle; retained failed callback context and refusal of unsafe RT reset. | **Uncompiled, unexecuted, unverified: needs a Mac.** Installed-component scanner/catalog and desktop provider, Cocoa editor, transport callbacks, multi-bus/sidechain, reset recovery and third-party acceptance remain software/platform gates. |
| `fmt-host-vst2` | excluded by scope | Recorded owner's explicit decision: no VST2 implementation; no new Steinberg VST2 licences and project scope is CLAP/VST3/AU. Historical GPL-header research remains context only. | Overseer must record scope exclusion; no adapter/fixture/SDK/header was added. Never mark this as implemented functionality. |
| `fmt-host-fl-as-plugin` | partial | Separate Windfall CLAP target embedding real engine, host sample rate/clock/MIDI, 16 stereo outputs, checked embedded PCM/project state, partial-stream state IO and callback allocator harness. | VST3 export ABI, editor/export command, strict DSP reset flushing, active state publication/capture fence, clock/meter arbitration, packaging and actual other-host acceptance. Default program is empty until a prepared packet is loaded. |
| `core-vst2-vst3-audio-unit-and-clap-support` | partial | Shared host traits and the above CLAP/VST3/AU source paths. | Depends on `fmt-host-clap`, `fmt-host-vst3`, `fmt-host-au` and platform/containment acceptance. Overseer must narrow literal VST2 wording to match scope exclusion. Wrapper/bridge are hosting prerequisites; export is separately tracked. |

No full-row candidate is claimed: outstanding software gates are listed alongside
unexecuted/native external gates. Another worker owns database/picker UI and the
mixer stream owns routing; neither is reworked here.

## Implementation details that matter when integrating

- ABI 7 rejects old mappings. Layout is 16×128 legacy velocities plus 1,024
  eight-word owner/control records before parameters/timed events; SLOT_WORDS×4
  is 266,560 and REGION_BYTES is 1,066,496. Ready carries actual native instance
  support and 16 expression masks. Desktop maps engine u64 owners to native IDs.
  Parent/helper must be rebuilt together; Load also gained defaulted wrapper data.
- Recovery covers overlap, channels, release velocity, pan, fractional pitch and
  both modulation controls. Desired snapshots survive reset/overflow; admission,
  clean native DSP and complete controls must prove a processed generation.
  Recovery has its own preallocated budget; ordinary overflow remains counted.
- VST3 Events now retains pointer-free numeric expression type 4 with checked
  identity/normalized finite value; pointer-bearing text/SysEx remain refused.
  Prepared native input capacity is 12,289; VST3 note storage 27,650 and input
  parameter points 16,641. The native output flood fixture emits 30,000 events.
  New expression fixture class 23 is appended: old indices stay stable; count 24.
- Helpers open/close real native editors and pump Win32 messages. Native dirty
  serials feed guarded pending Capture updates; acknowledgement follows accepted
  document/engine state. Failed/cancelled capture releases delivery latch to retry.
  Native restart currently shares dirty serial handling; full restart integration
  and updated expression capability adoption still need acceptance.
- Fixed native B collects audio/sidechain/events, adds B samples to native latency,
  and retires pending parameters off audio. Finite-tail smart disable requires a
  second of observed quiet; controls/input wake it; endless tails remain active.
  Saved policy reaches both local adapters and helper reactivation. Core activation
  rejects recursively selecting an owner provider. Threaded is explicitly absent.
- Nondefault wrapper policy participates in engine and desktop binding identity.
  `SetPluginWrapper` is one Plugins edit, undoable and cloned/reopened normally.
  Default policy/variable fixedBuffer are omitted from JSON; legacy explicit null
  remains readable. Native HWND is transient. Custom scaling refusal is an error.
  Attached means an owned native top-level window, not a pane embedded in React.
- AU FFI is wholly macOS-gated and links system AudioToolbox/CoreFoundation only.
  Failed initialization retains installed callback storage until owner removal or
  disposal. Active reset refuses subsequent render rather than calling non-RT-safe
  AudioUnitReset; engine-level reset/repreparation is therefore still incomplete.
- Export uses existing GPL engine plus already-used MIT/Apache clap-sys 0.5; no
  SDK/content is vendored. Preparation/state IO/disposal stay off audio. Host clock
  bends held notes; startup messages are consumed before host song transport.
  Nested native bindings, hardware recording, internal tempo automation/navigation
  markers, nonzero tempo_inc and active state load are explicitly refused. State
  is bounded to 64 MiB encoded project+PCM; callback renders borrowed outputs.

## Authored acceptance tests — written, not executed

| File | New/changed cases |
| --- | --- |
| host `src/bridge/adapter.rs` | `instance_snapshot_retains_overlap_expression_and_all_channels_after_reset`; `saturated_instance_expression_recovers_desired_controls_and_guarantees_release` |
| host `src/bridge/protocol.rs`, `slots.rs` | ABI 7 exact-layout/version/snapshot validation assertions |
| host `tests/bridge.rs` | `native_clap_instance_expression_audio_matches_reference_after_bridge_latency`; `helper_fixed_buffer_policy_reports_and_measures_its_added_latency`; `vst3_expression_identity_and_controls_replay_after_helper_timeline_reset`; existing helper editor now requires open/close success |
| host `src/vst3/buffers.rs` | `numeric_expressions_survive_the_sdk_list_and_refusal_counts_as_a_drop`; enlarged fixed input capacity assertions |
| host `tests/vst3_host.rs` | `native_numeric_expression_controls_distinct_overlapping_note_identities` |
| standalone fixture `src/vst3_expression.rs` | `reused_identity_updates_existing_owner_even_with_an_earlier_empty_slot` |
| host `src/wrapper.rs` | `smart_disable_stops_only_after_observed_quiet_and_wakes_on_input_or_control` |
| host `tests/wrapper_options.rs` | `fixed_native_buffers_delay_pcm_exactly_and_preserve_pending_state_on_return`; `wrapper_legacy_defaults_and_unsupported_owner_modes_are_explicit` |
| project `tests/plugins.rs` | `wrapper_options_preserve_legacy_json_and_roundtrip_history_clone_and_reopen` |
| project `src/plugin_wrapper.rs` | `variable_buffer_is_absent_matching_the_optional_generated_type` |
| desktop `plugins/wrapper.rs` | `saved_policy_selects_real_owners_and_preserves_native_buffer_and_editor_settings` |
| frontend `wrapper-controls.test.tsx` | mounted history/default undo, owner/editor file reopen, uncommon saved values/threaded recovery, failed-command retry |
| frontend `sim/document.test.ts` | one valid `setPluginWrapper` EVERY_COMMAND case |
| host macOS `src/au/mod.rs` | `apple_effect_bypass_audio_parameter_state_and_processor_return`, including reset refusal/failed silence and returned ownership |
| export `src/clap/tests.rs` | `host_clock_places_note_on_exact_frame_and_bends_tempo_without_restarting`; `embedded_state_restores_identical_multibus_pcm_and_host_sample_rate`; `clap_boundary_handles_partial_state_streams_sample_offsets_and_lifecycle` (actual factory/partial streams/MIDI/allocator accounting) |

Fixtures compute actual native PCM; wrapper uses an independent delayed ramp,
expression uses algebraic stereo marks and export uses independently calculated
onsets and restored multibus samples. Existing crash/hang/state/capture/allocator
assertions are retained. Mac bundle/scan counts change only from 23 to 24.

## Integrator commands — worker has not run them

Run sequentially, from the integrated checkout in Git Bash. Source MSVC setup;
keep Cargo/test jobs serial and use temporary TS exports for Rust suites. Existing
host fixture helpers can invoke nested Cargo, so do not overlap test programs.

```sh
source scripts/msvc-env.sh
export CARGO_BUILD_JOBS=1
export TS_RS_EXPORT_DIR="$(cygpath -w "$(mktemp -d)")"
cargo generate-lockfile
cargo build --manifest-path crates/windfall-plugin-host/test-plugins/Cargo.toml --target-dir target/plugin-fixtures --jobs 1
cargo test --manifest-path crates/windfall-plugin-host/test-plugins/Cargo.toml --lib --jobs 1
cargo build -p windfall-plugin-host --bin windfall-plugin-audio --jobs 1
export WINDFALL_BRIDGE_FIXTURE="$(cygpath -w "$PWD/target/plugin-fixtures/debug/windfall_test_plugins.dll")"
export WINDFALL_DESKTOP_BRIDGE_HELPER="$(cygpath -w "$PWD/target/debug/windfall-plugin-audio.exe")"
cargo test -p windfall-project --test plugins -- --test-threads=1
cargo test -p windfall-project --lib variable_buffer_is_absent -- --test-threads=1
cargo test -p windfall-plugin-host --lib -- --include-ignored --test-threads=1
cargo test -p windfall-plugin-host --test wrapper_options -- --test-threads=1
cargo test -p windfall-plugin-host --test vst3_host -- --test-threads=1
cargo test -p windfall-plugin-host --test vst3_scan -- --test-threads=1
cargo test -p windfall-plugin-host --test bridge -- --test-threads=1
cargo test -p windfall-plugin-host --test realtime -- --test-threads=1
cargo test -p windfall-desktop --lib plugins:: -- --include-ignored --test-threads=1
cargo test -p windfall-desktop --lib session::tests::plugin -- --include-ignored --test-threads=1
cargo test -p windfall-plugin --lib -- --test-threads=1
cargo test -p windfall-engine --features plugin-export --lib -- --test-threads=1
cargo test -p windfall-engine --features plugin-export --test engine -- --test-threads=1
cargo clippy -p windfall-plugin-host -p windfall-project -p windfall-engine -p windfall-plugin -p windfall-desktop --all-targets --all-features -- -D warnings
```

The host audio helper supports the desktop fixture protocol; packaged acceptance
must additionally build/use the real desktop helper-mode executable, exercise its
installed lookup and confirm matching ABI. On a native Mac (omit MSVC/cygpath):
`cargo test -p windfall-plugin-host --lib au::tests:: -- --test-threads=1` and
`cargo test -p windfall-plugin-host --test vst3_bundle_macos -- --test-threads=1`.
Run Linux host/realtime regression suites without retention/skip workarounds.

After Rust schema/engine checks, regenerate with `scripts/gen-bindings.sh` and
`scripts/build-sim.sh`. From `apps/desktop`:

```sh
npm run test -- src/features/plugins/wrapper-controls.test.tsx src/features/plugins/plugins.test.tsx src/lib/ipc/sim/document.test.ts
npm run typecheck
npm run lint
```

Only after these pass, build/package the export with
`cargo build -p windfall-plugin --release --jobs 1`; copy the Windows DLL to
Windfall.clap and accept it in an external host. Mac/Linux export bundles and
VST3 export are separate remaining work. Do not infer real-host success from
unit tests. No generated output is included in this branch.

## Shared seams and regeneration

Behavioral seams: project `src/{command.rs,lower.rs,plugin.rs}`; engine
`Cargo.toml`, `src/{plugins.rs,processor.rs,sequencer.rs}` plus new
`plugin_export.rs`; desktop `src-tauri/src/commands.rs` (AppHandle/HWND only),
`plugins/{mod.rs,runtime.rs,bridge.rs}`; frontend `features/plugins/controls.tsx`
(two-line composition) and `lib/ipc/sim/document.test.ts` (one command example).
New dedicated modules contain wrapper model/runtime/UI, AU and export logic.

Only required default wrapper fields were added to binding literals in engine
`src/{controller.rs,device.rs,project_preparation.rs,render.rs}`; engine
`tests/engine/{timeline.rs,utility_effects.rs}` and three mixer QA files;
desktop session `mixer_presets_qa.rs`, `tests/{archive.rs,files_qa.rs,plugins.rs,
project_preparation.rs}`; desktop plugin capture/ownership tests; project
`tests/{imported_plugin_persistence.rs,presets.rs}` and host sidechain tests.
No mixer routing, catalog/picker redesign or descriptor-index reordering.

Regenerate `PluginBinding`, `PluginWrapperOptions`, `PluginProcessingMode`,
`Command` and TS barrel; WebAssembly simulation binary/metadata; Cargo.lock for
new workspace export package; fixtures/scanner/helper/desktop/export binaries.
Bridge control/Ready schema is internal, but incompatible helpers must be replaced.

## Check risks in this order

1. Compile/type consistency: new SDK expression interface tuple/pointer constness,
   VST3 scale interface imports, AU C layouts/CF lifetime (Mac), CLAP raw callback
   types/engine export publication signatures, Tauri HWND representation and Base
   UI numeric Select/ToggleGroup types. Warnings are denied; nothing was compiled.
2. Rebuild matching parent/helper ABI 7 and 24-class fixture. Verify capacity/layout,
   exact native PCM/reset crossfade, per-instance memory increase, no callback
   allocations, no translation refusal silently becoming completion proof.
3. Capture/ack cancellation/retry, native restart/capability adoption, parameter
   retention after fixed-buffer return, dirty native state during policy replacement,
   undo/reopen races, sidechain/PDC/live/offline equivalence and attached helper GUI.
4. AU callback-property scopes, failed Initialize/Uninitialize/disposal ownership,
   plist ownership, real instrument/effect acceptance and owner reset recovery.
   AU cannot yet be selected through the desktop plugin model/provider.
5. Export active-state/strict-reset/tempo-meter constraints and VST3 ABI/editor/
   packaging gaps; run actual other-host acceptance after adding those features.
6. External plugin corpus, packaged helper discovery, Mac/Linux native owners and
   multi-output mixer integration. VST2 remains excluded and umbrella wording must
   be reconciled by the overseer. Historical docs' earlier successes do not verify
   this source batch.
