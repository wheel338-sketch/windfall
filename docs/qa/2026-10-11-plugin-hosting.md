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
