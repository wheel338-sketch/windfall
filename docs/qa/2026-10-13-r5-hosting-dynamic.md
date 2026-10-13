# Round five hosting dynamic worker hand-off - 2026-10-13

Worktree: `C:/Users/ewhee/.t3/projects/windfall/.claude/worktrees/r5-hosting-dynamic`.
Branch: `gpt/r5-hosting-dynamic`. Base: `469be35c274f4e3d3dd47b6163f9da9609ed1c4e`.

EDIT-ONLY. No Cargo build/check/test, Clippy, frontend tool, generation, install,
download, push, PR, tag or sub-agent ran. The permitted single-file
`rustfmt --check --edition 2024 --config skip_children=true` source checks ran:
there are formatting differences for the integrator to fix. A source encoding
error found by that check was corrected. No parser error remains in the
subsequent checked files. `git diff --check` passed. No passing compiler,
runtime, fixture, UI, allocator or platform result is claimed.

The contract and exact SOFTWARE cells in round-four acceptance, earlier hosting
hand-offs, hosting/bridge design notes, roadmap section 3 and QA completion rule
were read. Parity inventories, generated bindings and simulator artifacts were
not edited. No dependency or Cargo.lock change was made. Cocoa/X11 editor code
and macOS/Linux-specific implementation blocks were untouched. The Mac bundle
fixture tests have count-only updates for the appended portable VST3 class.

Source commits preceding this document:

- `15785223`: saved capability snapshots, routing commands and permanent parameter addresses.
- `8dd965e2`: native refresh, bridge ABI/audio buses, policy providers and dynamic fixtures.
- `98d41b21`: restored topology refresh and pending native controller points.
- `a0a77880`: retain dirty notifications during restored CLAP refresh.
- `51765d2a`: engine routing/PDC, desktop capture/recovery, UI and integration tests.
- `5f989b4f`: fixed/Threaded auxiliary bus PCM delay regression.
- `4bdb1ac2`: dynamic fixture emits only its declared parameter IDs.

## Per-row accounting

### fmt-host-clap

Implemented inactive owner-side audio/note-port, parameter and dialect refresh
in host `src/{instance,clap/instance,clap/handlers}.rs`. Audio-port rescans are
supported; requested owner callbacks complete during inactive refresh without
consuming the dirty/restart flags that the installed owner must observe.
An existing processor's latency remains fixed until re-admission. Fresh native
instances refresh again after loading state and applying companion parameters.

Desktop `plugins/{runtime,capabilities,bridge}.rs` and `session/edit.rs` capture
state and current metadata, then commit `ReadmitPlugin` through the ordinary
checked document/prepared-engine transaction. Every accepted snapshot increments
its saved capability revision, so unchanged opaque bytes cannot cause reuse of
an old latency/note/port owner. New helpers use new sessions/mappings. All native
queries, allocations, deactivation, joins and retirement occur off audio.

Project `src/{plugin,command,lower,document,check}.rs` preserves append-only
parameter positions. Removed IDs remain tombstones with their original ranges,
values, automation and controller links; new IDs append, returning IDs recover
the same position. Document history also retains consumed addresses when undo
or discarding redo would otherwise permit reuse. Rejected transactions do not
consume addresses. Legacy bindings omit the new default capability field.

`features/plugins/controls.tsx` shows stale parameter/route diagnostics, disables
unavailable native controls, and allows a stale output route to be disconnected.
It exposes checked native-output-to-mixer-track selection with undo/redo and
persistence. Native auxiliary buses travel through direct, fixed-buffer,
Threaded and process providers. Mono buses duplicate into stereo; the stereo
mixer reads the first pair of a wider native bus. Native processing retains its
full declared buffers. Changed/missing buses retain their saved routes and show
an unavailable diagnostic instead of deleting musical routing.

No known remaining owned implementation item is intentionally deferred. This is
source completion awaiting the integrator's compilation and measured suites.

- SOFTWARE: separate owner - merge the sibling's Cocoa/X11 embedded-editor implementation.
- EXTERNAL-ONLY: other-platform execution, external plugins and physical device acceptance.

### fmt-host-vst3

The same saved metadata/address/routing and prepared-plan path applies. Host
`src/vst3/instance.rs` now shares one checked SDK metadata reader between creation
and inactive re-admission. It rebuilds native parameter storage after ownership
return, preserves pending controller points and newer deactivation edits, and
keeps old prepared latency fixed. Fresh activation renegotiates audio/event
buses and expression masks. `src/vst3/processor.rs` exposes prepared auxiliary
output buffers; controller calls remain off the host audio callback.

The portable fixture appends class index 25 (ID ending `1A`); prior class IDs and
positions remain unchanged. Its persisted switch changes latency, input/output
buses, parameter presence, note-port count and expression capabilities. Its
37-frame mode delays actual PCM; extra buses emit independent mono/stereo marks.
Windows scan and Mac bundle tests now expect 26 classes. No native editor code
was written for this fixture or host in this worker.

No known remaining owned implementation item is intentionally deferred; the
shared new tests below are mandatory before acceptance.

- SOFTWARE: separate owner - merge the sibling's macOS/Linux embedded-editor implementation.
- EXTERNAL-ONLY: native OS, external plugin corpus and device execution.

### fmt-host-bridging

Mapping ABI is now 9. Header word 30 is a latched readmission request, and slot
word 55 carries checked output count. Each slot has 64 bounded stereo auxiliary
bus payloads. Header/slot/region byte counts are 256 / 532800 / 2131456. Parent
and helper must be rebuilt together; earlier mappings remain rejected.

Host `src/bridge/{control,helper,supervisor,slots,adapter,protocol}.rs` supplies
an authenticated owner/epoch-fenced inactive readmission operation. It returns
state and full metadata, caches completed replies separately from ordinary
captures, and leaves the old native processor inactive for retirement. The
mapping is never resized or reused while an old helper can write it. Audio
copies prepared bus storage through the same two-block pipeline; offline waits
use that identical pipeline. Invalid/nonfinite output is refused.

Desktop runtime keeps completed captures until document acceptance, including
preparation refusal or late owner-job cancellation. New document parameter
intent overlays companion values without discarding completed bytes or repeating
native save for that pending serial. ACK bookkeeping clears only covered cached
serials. A retired token completes an old ACK without delivering it to a new
helper. Revision, selected-token and binding checks fence replacement documents.
A failed selected helper schedules recovery through the existing capture and
prepared-plan path. It restores the last completed checkpoint plus current
parameter intent into a new helper; it does not load native code into the parent.
Save/export during unfinished capability admission reports the pending admission
rather than serializing new state against an old descriptor list.

The native table-driven test enumerates both formats and both capture-exit/hang
faults at these boundaries, with and without document replacement:

| Boundary | Recovery/acceptance oracle | Replacement oracle |
| --- | --- | --- |
| Before reply | Last completed checkpoint and newer companion intent survive helper termination. | Revision changes while an old owner control job is in flight; its result cannot change the replacement. |
| After reply | The same completed bytes are returned on retry, then restored by a fresh owner. | A completed precommit capture becomes obsolete. |
| After commit, before ACK | One checked document edit survives helper failure and an unconfirmed receipt. | The accepted old-document edit cannot be applied to the replacement; old ACKs retire. |
| After ACK | Receipt retry is idempotent and cannot replay state into another token. | Replacement remains unchanged by repeated retired-token ACK delivery. |

This gives 32 native rows. Successful recovery rows assert actual restored PCM,
zero callback allocator calls, fresh selected tokens, state equality and
undo/redo; replacement rows assert document preservation and terminal ACKs.
Existing capture/ACK/replacement/allocator tests were retained.

Technical boundary: a plugin that crashes or hangs before producing a complete
state reply has not supplied recoverable new bytes. No host can reconstruct
arbitrary private native memory after that process is gone. This implementation
preserves completed captures and the durable checkpoint, retains current host
parameter intent, and reports the failure. It does not claim to recover
unreturned editor-only/native-private edits. The table explicitly distinguishes
that case from losing already completed state.

- SOFTWARE: separate owner - merge non-Windows embedded-editor ownership code.
- EXTERNAL-ONLY: installed packaging, other-platform IPC and external crash/hang corpus.

### core-vst2-vst3-audio-unit-and-clap-support

The three shared owned gaps have source implementations through CLAP/VST3,
project persistence, live/offline engine plans and the real desktop controls.
VST2 remains excluded by the recorded owner scope decision; no VST2 adapter or
licence was added. AU work belongs to its other owner and was not changed.

- SOFTWARE: other streams - AU integration/compilation and non-Windows embedded editors.
- EXTERNAL-ONLY: platform, licensed external plugin and hardware acceptance after integration gates.

## Exact authored test inventory - none executed

Project integration target `crates/windfall-project/tests/plugins.rs`:

- `readmission_keeps_tombstone_addresses_automation_links_routes_and_history`
- `legacy_binding_omits_capabilities_and_loads_with_empty_admission`
- `undo_and_discarded_redo_cannot_reuse_a_dynamic_parameter_address`

Autodiscovered host integration target `crates/windfall-plugin-host/tests/dynamic_capabilities.rs`:

- `clap_and_vst3_live_switches_readmit_ports_latency_parameters_and_note_capabilities`
- `native_extra_buses_follow_fixed_and_threaded_pipeline_delays`

Existing host integration target `crates/windfall-plugin-host/tests/bridge.rs`:

- `dynamic_clap_and_vst3_helpers_readmit_live_capabilities_into_fresh_mappings`

Registered engine library module `crates/windfall-engine/src/plugins.rs`:

- `saved_native_output_routes_match_live_pcm_offline_reopen_and_undo`
- `dynamic_readmission_rebuilds_native_latency_compensation_off_callback`

Existing registered desktop capture module
`apps/desktop/src-tauri/src/plugins/capture_ack_tests.rs`:

- `bridge_capture_ack_restart_replacement_native_interleaving_table`

Mounted UI `apps/desktop/src/features/plugins/controls.test.tsx`:

- `routes a native bus through checked history and actual file reopen`
- `shows retained stale automation addresses and lets a missing output be disconnected`

`apps/desktop/src/lib/ipc/sim/document.test.ts` retains EVERY_COMMAND exhaustiveness
and adds valid `readmitPlugin` / `setPluginOutputRoute` examples. Existing Rust
binding constructors received default capability snapshots; existing oracles
were not loosened, ignored, skipped or deleted. Protocol layout/version and
fixture counts were updated for the new actual ABI/class inventory.

## Integrator order

Run serially in the integration checkout, with MSVC setup, one Cargo job and a
private `TS_RS_EXPORT_DIR`. First apply formatting and compile the source and
fixture. Rebuild the fixture, audio helper and desktop helper before native
suites. Set `WINDFALL_BRIDGE_FIXTURE` and `WINDFALL_DESKTOP_BRIDGE_HELPER` to those
prebuilt files so tests do not initiate nested builds. Then run:

1. Project plugin integration tests, full project tests and checked history/preset/controller suites.
2. Host `dynamic_capabilities`, `bridge`, `wrapper_options`, `realtime`, `vst3_host`, scan and library suites, preserving earlier capture/reset/no-drop tests.
3. Engine library `plugins::tests::` plus mixer/PDC/render/realtime suites.
4. Desktop plugin/capture/ACK/session suites, particularly the new native table and existing ready-capture/replacement/policy tests.
5. Regenerate TypeScript bindings and WASM only in the integration checkout. New exports include `PluginCapabilities`, `PluginOutputRoute`, optional `PluginBinding.capabilities` and the appended command variants.
6. Mounted plugin controls/wrapper tests, EVERY_COMMAND simulation tests, then full frontend typecheck/lint/tests and strict workspace Clippy.
7. Reconcile sibling platform changes and run native Mac/Linux checks and suites. The count-only Mac bundle changes are required by the portable appended fixture class.

## Least certain spots

1. No compiler ran. SDK pointer/trait types, disjoint mutable borrows in output
   copying, new struct fields/exhaustive command consumers, and strict Clippy
   require integration checks. Rustfmt source checks found formatting differences.
2. Expanded bridge storage and 64-bus callback loops need the full allocator,
   latency, reset, corrupt-packet, no-drop and deadline suites. All packet/bank
   allocations are prepared; actual device deadlines remain measured gates.
3. Prepared native owner replacement, PDC decrease after departing-owner
   retirement, and auxiliary-bus compensation/history continuity need the engine
   and desktop session suites. The new tests assert exact PCM and declared delays.
4. The native capture table uses explicit prebuilt fixtures/helpers and 100 ms
   control fault deadlines. Validate confirmed reap, completed-reply caches,
   ACK receipt loss and revision replacement under the real desktop owner loop.
5. The stereo mixer reads the first pair of wider native output buses; this
   follows the host's existing stereo projection. Wider native speaker-layout
   routing is not a new surround mixer implementation in this source batch.
6. The unreturned-native-private-state boundary above is unavoidable after
   process loss; only completed state/checkpoints and host-owned intent can be
   preserved. No current acoustic/platform/packaging result is claimed.

All implementation and tests are committed. This document is the final source
hand-off; rows still require the integrator's passing Windows gates.
HAND-OFF COMPLETE
