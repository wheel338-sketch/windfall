# Plugin hosting implementation hand-off � 2026-10-11

Branch: `gpt/w3-plugin-hosting`. Worktree: `.claude/worktrees/gpt-plugin-hosting`.
All changes are source-only. **No builds, tests, device checks or nested agents were run.**
The overseer owns tracker status. No full row is claimed verified.

## Bridge instance transport checkpoint

ABI 7 replaces ABI 6: 16�128 legacy channel/key velocities and a checked,
bounded 1,024-owner snapshot precede timed events. The snapshot contains native
identity, channel/key, velocity, pan, fractional pitch and both modulation
controls. Ready negotiates actual native instance/expression capabilities.
The helper reconstructs owners and supported expressions after reset, gaps,
inactive parameter reconciliation and uncertain native admission; snapshots
remain desired intent and never manufacture completion proof.
Desktop bridge facade maps 64-bit engine identities to distinct native IDs,
forwards instance/channel release, expression and pitch, and retains release
velocity. Native recovery events have a separate preallocated budget so replay
does not consume ordinary admission. The CLAP sine fixture now respects PCKN
release identity and implements measurable pan/tuning.

Tests written, not executed:
- `bridge::adapter::tests::instance_snapshot_retains_overlap_expression_and_all_channels_after_reset`
- `bridge::adapter::tests::saturated_instance_expression_recovers_desired_controls_and_guarantees_release`
- `tests/bridge.rs::native_clap_instance_expression_audio_matches_reference_after_bridge_latency`
- Existing exact-layout/version assertions updated for ABI 7.

Integrator: first build the standalone fixture once; then set
`WINDFALL_BRIDGE_FIXTURE` to its DLL and `WINDFALL_DESKTOP_BRIDGE_HELPER` to the
fresh helper executable. Use Git Bash with `source scripts/msvc-env.sh`, one
Cargo job and temporary `TS_RS_EXPORT_DIR`. Run:

```sh
cargo test -p windfall-plugin-host --lib -- --include-ignored --test-threads=1
cargo test -p windfall-plugin-host --test bridge -- --test-threads=1
cargo test -p windfall-plugin-host --test realtime -- --test-threads=1
cargo test -p windfall-desktop --lib plugins:: -- --test-threads=1
cargo test -p windfall-desktop --lib session::tests::plugin -- --test-threads=1
cargo clippy -p windfall-plugin-host -p windfall-desktop --all-targets --all-features -- -D warnings
```

Check first: new CLAP C constant imports; helper Ready metadata; snapshot ABI
exact-size assertion; recovery budget versus native translation budgets;
healthy PCM comparison and startup/reset crossfades. Parent and helper must
be rebuilt together; ABI 6 helpers are deliberately incompatible.

## Row accounting (current checkpoint)

| Row | Claim | Remaining gates |
| --- | --- | --- |
| `fmt-host-vst3` | partial | Fresh fixture/editor/external-plugin and Mac/Linux acceptance; multi-output desktop routing. |
| `fmt-host-clap` | partial | Fresh fixture/editor/external-plugin and Mac/Linux acceptance; multi-output desktop routing. |
| `fmt-host-bridging` | partial | New expression replay validation; packaged discovery, fresh helper editor/lifecycle validation, external corpus, non-Windows IPC. |
| `fmt-host-wrapper-options` | partial | DSP fixed-buffer/smart-disable and native editor policy implemented; persistence/history/UI and threaded owner provider remain. |
| `fmt-host-au` | partial | macOS AUv2 source adapter written; needs a Mac to compile/execute plus scanner/provider/editor/multi-bus integration. |
| `fmt-host-vst2` | partial | GPL header evidence documented in docs/plugins/vst2-licensing.md; provenance/distribution decision required before an adapter. |
| `fmt-host-fl-as-plugin` | partial | Separate CLAP engine export target written; VST3 export, editor/export UI, reset/state arbitration, packaging and actual other-host acceptance remain. |
| `core-vst2-vst3-audio-unit-and-clap-support` | partial | Depends on VST2, VST3, AU, CLAP and platform/containment acceptance; wrapper/export rows tracked separately. |

Shared engine seams: crates/windfall-engine/Cargo.toml, src/processor.rs and src/sequencer.rs (feature-gated export hooks only). Generated IPC/TS types: none so far. Plugin
scanner metadata changes, when added, require the integrator to refresh the
existing generated plugin catalog bindings; never hand-edit them.

## Wrapper/editor and AU checkpoint

Native helper editors now open/close on the native owner and pump Win32 events.
Native state/parameter notifications publish a monotonic bounded change serial;
the desktop queues the existing guarded capture request and acknowledges only
after document/engine acceptance. Editor options call actual CLAP/VST3 scaling
interfaces; a custom scale refusal is an error. Attached means a native DAW-owned
top-level editor, detached means no owner; unsupported floating-only attachment
is refused. Existing bridge editor test now requires open/close success.

`wrapper::WrapperOptions` and `activate_with_options` implement fixed native
buffers with B-sample latency, event-offset preservation and off-audio pending
parameter retirement, and smart disable after a finite tail plus a second of
observed silence. Events/input wake processing; endless-tail plugins stay awake.
Threaded/bridged policy choices are explicitly refused by the in-process API;
actual default desktop bridging continues through the existing supervisor.
**Wrapper options are not yet wired into project commands/history or a desktop
options editor. This row remains partial.**

AUv2 source is entirely macOS-gated. Installed AudioComponent registration,
parameter discovery, stereo Float32 render/input callbacks, MIDI, native plist
state, negotiated latency/tail, fallible processor return and owner-side disposal
use the existing host traits. `PluginHost::create_audio_unit` uses stable
component registry IDs. This is **uncompiled, unexecuted and unverified; a Mac is
required**. Cocoa editor, registry scanner/desktop provider, host callbacks,
AU multi-bus/sidechain and native reset-thread acceptance remain open.
Windows cannot call or link these framework declarations.

Additional tests written, not executed:
- `wrapper::tests::smart_disable_stops_only_after_observed_quiet_and_wakes_on_input_or_control`
- `tests/wrapper_options.rs::fixed_native_buffers_delay_pcm_exactly_and_preserve_pending_state_on_return`
- `tests/wrapper_options.rs::wrapper_legacy_defaults_and_unsupported_owner_modes_are_explicit`
- macOS `au::tests::apple_effect_bypass_audio_parameter_state_and_processor_return`

Additional commands: `cargo test -p windfall-plugin-host --test wrapper_options`
and on a Mac `cargo test -p windfall-plugin-host --lib au::tests:: -- --test-threads=1`.
Check VST3 content-scale interface imports, native owner/window semantics,
bridge dirty capture/ack races, and AU C struct layout/CF ownership first.
No new dependencies or generated IPC/TS types in this checkpoint.

## Restart checkpoint

The assigned worktree was clean at restart, at commit 6fec307b. No uncommitted
implementation was lost. Remaining software work begins with the standalone
Windfall plugin export target; full-row gates above remain explicit.


## Windfall export checkpoint

`crates/windfall-plugin` is a separate CLAP cdylib/rlib target. It embeds the
engine with host sample rate, sample-offset MIDI/transport, clock bending,
16 actual stereo mixer outputs, cached engine latency and checked bounded
project-plus-PCM state. Preparation, state IO and disposal are main-thread only.
Startup messages are consumed before applying the host song clock. No device
is opened. Active state load, nested native bindings, hardware recording,
internal tempo arbitration, navigation markers and tempo ramps are refused.
Reset currently stops/fades and seeks; strict DSP tail/delay flushing is open.
There is no editor, desktop export command, VST3 export ABI or installer yet.
See docs/plugins/windfall-as-plugin.md. This is a partial source candidate.

Tests in crates/windfall-plugin/src/clap/tests.rs, written, not executed:
- host_clock_places_note_on_exact_frame_and_bends_tempo_without_restarting
- embedded_state_restores_identical_multibus_pcm_and_host_sample_rate
- clap_boundary_handles_partial_state_streams_sample_offsets_and_lifecycle
  (includes actual CLAP callback allocation/free accounting and a PCM ramp reference)

Integrator commands (worker has not run these):
```sh
cargo test -p windfall-plugin --lib -- --test-threads=1
cargo test -p windfall-engine --features plugin-export --lib
cargo test -p windfall-engine --features plugin-export --test engine
cargo clippy -p windfall-plugin -p windfall-engine --all-targets --all-features -- -D warnings
cargo build -p windfall-plugin --release --jobs 1
```
On Windows, package target/release/windfall_plugin.dll as Windfall.clap for an
external host only after source tests pass. Mac/Linux packaging remains external.
Check raw CLAP callback field types, engine attachment/publication signatures,
new fixture field literals, independent onset/PCM assumptions and clock-bend
floors first. Regenerate Cargo.lock for the new workspace package; no IPC/TS
schema additions in this group. Engine hooks compile only with plugin-export.
No build/check/test was executed, including by the fixture harness.

## AU lifecycle follow-up

The AU adapter no longer invokes AudioUnitReset on the callback: Apple's
public AUBase contract marks reset non-realtime-safe. Active reset now refuses
subsequent rendering through normal failed-health/silence handling and requires
owner deactivation/repreparation. The Mac bypass test also asserts this failure
and returned ownership (written, not executed). Failed Initialize/timing setup
retains callback input context; retries first quiesce/remove it, and successful
return removes the native callback before releasing buffers. Mac review must
check callback-property scope, idempotent Uninitialize and error-return disposal.

## Wrapper persistence checkpoint

PluginBinding now stores an omitted-at-default, validated wrapper policy:
smart disable, fixed native buffer, processing owner choice, editor scale and
detachment. SetPluginWrapper is one undoable Edit::Plugins transaction. Native
owner/engine binding hashes include nondefault policy so preparation does not
reuse an owner with another DSP policy. Existing binding literals gain only a
Default::default() field, including mixer-stream fixtures; mixer routing is not
changed. The EVERY_COMMAND simulation table has the new command example.

Test written, not executed: windfall-project/tests/plugins.rs::
wrapper_options_preserve_legacy_json_and_roundtrip_history_clone_and_reopen.
Integrator: cargo test -p windfall-project --test plugins; regenerate bindings
and simulation artifacts, then run the document command table suite. Regenerate
PluginBinding, Command, PluginWrapperOptions and PluginProcessingMode plus their
barrel exports. Generated files are not edited here. Runtime/UI wiring follows
in the next group; persisted data alone is not claimed working DSP integration.
Shared seams newly touched: project command.rs/lower.rs/plugin.rs, engine
plugins.rs and binding literals in controller/device/render/preparation and
session/mixer tests; only required field defaults were added to those literals.

## Wrapper adapter/helper checkpoint

Option-aware effect/instrument preparation now reaches activate_with_options.
Helper Load carries validated DSP wrapper policy, and every inactive reconcile,
describe/capture reactivation keeps that policy. Fixed native blocks may differ
from IPC block size; native preparation reserves the larger bound. Ready reports
combined native/wrapper latency. Threaded/bridged are owner choices and cannot
be recursively selected inside an already launched helper. Existing Launch
literals gain explicit defaults only.

Test written, not executed: tests/bridge.rs::
helper_fixed_buffer_policy_reports_and_measures_its_added_latency (independent
stereo PCM ramp, 128 bridge + 32 wrapper samples). Run the bridge test command
above. Desktop saved-policy conversion is the next group, not yet claimed wired.
Parent/helper must both use this new Load shape; old controls reject new metadata.

## Saved wrapper runtime checkpoint

Desktop preparation now selects automatic/in-process/bridged owners from the
saved policy, passes smart-disable/fixed buffers to both native adapters and
helpers, and preserves policy on owner reactivation after capture. Threaded
selection returns an explicit unsupported-provider error; it is not simulated.
Native editor open applies saved scale/detachment and the current main HWND
through a narrow plugin_editor command addition. The HWND is transient, never
persisted. Attach means an owned top-level window; floating-only native editors
can still refuse attachment. Failed/cancelled bridge dirty capture now releases
its delivery latch so a later capture can retry.

Test written, not executed: plugins::wrapper::tests::
saved_policy_selects_real_owners_and_preserves_native_buffer_and_editor_settings.
Integrator: cargo test -p windfall-desktop --lib plugins::wrapper::;
run existing ownership/capture_ack and native editor tests too. Least certain:
Tauri HWND return type, attached editor across helper-process ownership, dirty
capture retry races and preserved wrapper delay after inactive reconciliation.
Shared seam commands.rs has only the injected AppHandle/parent lookup; IPC
pluginEditor arguments are unchanged. UI policy controls are the next group.
