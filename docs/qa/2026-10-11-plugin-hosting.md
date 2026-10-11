# Plugin hosting implementation hand-off — 2026-10-11

Branch: `gpt/w3-plugin-hosting`. Worktree: `.claude/worktrees/gpt-plugin-hosting`.
All changes are source-only. **No builds, tests, device checks or nested agents were run.**
The overseer owns tracker status. No full row is claimed verified.

## Bridge instance transport checkpoint

ABI 7 replaces ABI 6: 16×128 legacy channel/key velocities and a checked,
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
| `fmt-host-bridging` | partial | New expression replay validation; packaged discovery, actual helper editor/lifecycle, external corpus, non-Windows IPC. |
| `fmt-host-wrapper-options` | partial | Implementation in progress; persistence/history/UI and all stated lifecycle semantics required. |
| `fmt-host-au` | partial | macOS adapter in progress; cannot compile or execute on this Windows worker. Needs a Mac and native acceptance. |
| `fmt-host-vst2` | partial | Public GPL-header provenance/licence investigation and scoped implementation in progress. |
| `fmt-host-fl-as-plugin` | partial | Separate export target in progress; actual other-host acceptance external. |
| `core-vst2-vst3-audio-unit-and-clap-support` | partial | Depends on VST2, VST3, AU, CLAP and platform/containment acceptance; wrapper/export rows tracked separately. |

Shared seam files: none so far. Generated IPC/TS types: none so far. Plugin
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
