# Round-five non-Windows editors — implementation hand-off

Worktree: `C:/Users/ewhee/.t3/projects/windfall/.claude/worktrees/r5-nonwindows-editors`.
Branch: `gpt/r5-nonwindows-editors`; base: `469be35c274f4e3d3dd47b6163f9da9609ed1c4e`.

EDIT-ONLY. Nothing was built or run as an application or test. No Cargo command,
frontend command, binding/WASM generator, install, nested agent, push, PR or tag
ran. No native-platform or compiler success is claimed. Source inspection used
the permitted single-file `rustfmt --check --edition 2024 --config
skip_children=true`; all 35 changed Rust files passed after applying its source
formatting suggestions. `git diff --check` passed. Primary online API/manifest
text was read; no reference code, SDK, package or asset was downloaded to disk
or vendored. The parity inventory and generated artifacts are untouched.

Source commits, in order:

- `636d8726`: Cocoa/XEmbed hosting, portable policy/tests, Linux run loops and
  helper GUI control messages.
- `529fde2d`: exported Cocoa/X11 panels and their format entry wiring.
- `50ee7c18`: desktop native parents, bridge scale/report integration, AU review
  repairs and the check-only macOS script.
- `1b9df78c`: explicit registry retirement and per-window Cocoa class leases.
- `54e2006e`: native parent/dimension unit documentation.
- `d5d824c8`: reject a foreign run-loop caller before reading owner-local cells.
- `005ce027`: helper CFRunLoop source service and XEmbed keyboard/focus forwarding.
- `e7771838`: scoped native-adapter lint annotations and checked AU alignment.

## Per-row source result

### `fmt-host-vst3`

`crates/windfall-plugin-host/src/vst3/editor.rs` now uses the existing frame/view
path on Cocoa and X11 as well as HWND. Cocoa receives the **NSView**, not its
NSWindow. Linux receives the XID value encoded in VST3's pointer-sized parent
argument with the exact `X11EmbedWindowID` platform string. Both retain the view,
frame, parent and resize state on their creating GUI owner. Failed attachment
clears the frame; closing removes the view, clears its frame/run-loop references,
retires the last window-client COM reference and then destroys the native parent.

`src/gui/{native,cocoa,cocoa_ffi,x11,x11_ffi,lifecycle}.rs` implements owned
windows, actual application-window ownership, bounded resize negotiation,
plugin resize rejection rollback and close handling. Cocoa defers its close
button until plugin teardown. X11 distinguishes a child DestroyNotify from the
container's destruction, sends XEmbed notification/activation/focus messages,
forwards keyboard-proxy events with checked XCB requests, preserves activation
during child focus/grabs, and uses checked XCB child queries/sends so a
disappearing child cannot trigger
Xlib's process-fatal error handler through those operations.

`src/vst3/editor/run_loop.rs` exposes SDK `Linux::IRunLoop` through the frame
and `IHostApplication`. `src/vst3/gui_context_linux.rs` supplies the latter to
`IPluginFactory3::setHostContext` **before** instance construction and to native
initialize calls. It works before an editor exists. Nonblocking poll(2) and
owner-local timers feed native handlers; callbacks run with no registration/map
borrow held. Registrations have distinct tickets, a 256-entry bound, positive
timer intervals up to 24 hours, duplicate rejection, coalesced missed ticks and
unregister/teardown invalidation. Native refs release while their module remains
mapped. Foreign-thread calls return failure before touching owner-local cells.

Window/run-loop registries use portable `LocalRegistry` rather than a Vec with
a TLS destructor. Last-owner retirement frees their storage explicitly, so a
thread-exit callback cannot point into an unloaded exported host library.
Cocoa windows own and dispose their delegate class after releasing its instance.

Exact new tests: the five `gui::lifecycle::tests::*` and four
`gui::run_loop::tests::*` listed below, plus the scaling test. Existing Windows
`tests/editor.rs` and `tests/vst3_host.rs` remain required regression gates.

The owned editor implementation is complete as a source candidate. Whole-row
completion also depends on the sibling's dynamic capability and routing work.

- SOFTWARE: dynamic ports/latency/parameters/note capability re-admission,
  stable automation diagnostics/PDC and saved extra-output routing are assigned
  to `gpt/r5-hosting-dynamic`; this worker does not implement or certify them.
- EXTERNAL-ONLY: native Cocoa/X11 execution, real plugin corpus, focus/window
  manager interaction, external hosts and monitor transitions after checks pass.

### `fmt-host-clap`

`src/clap/editor.rs` now creates host-owned Cocoa/X11 parents and passes pinned
raw CLAP `clap_window` handles: `specific.cocoa` is an NSView pointer;
`specific.x11` is an unsigned-long XID. The plugin's GUI is destroyed on every
failed native parent/show path. Native resize rejection preserves the prior
geometry. `src/clap/instance.rs` enables the existing monitor notification path
on both targets. CLAP hide/destroy precedes native parent retirement.

Cocoa follows CLAP's logical-point API: **do not call gui.set_scale for cocoa**.
Application zoom changes the parent NSView's bounds transform, while AppKit
handles backing density. X11 calls `gui.set_scale` with application factor times
native monitor density and re-reads the plugin's actual size. Floating-editor
capability/refusal behavior remains explicit.

Exact new tests: the lifecycle and scaling tests below. Existing Windows
`tests/editor.rs` and CLAP hosting tests remain regression gates.

- SOFTWARE: the sibling's dynamic capability and saved output-routing items
  above remain outside this worker's scope.
- EXTERNAL-ONLY: native CLAP Cocoa/X11 plugins, application zoom, resize, close,
  focus and physical monitor transitions after integration checks.

### `fmt-host-bridging`

The existing initial-thread helper GUI pump now reaches both embedded editors
and all Linux frame/context run loops. The Cocoa helper also services zero-wait
CFRunLoop timer/source turns. `src/bridge/helper.rs` handles Unix scale updates
and GUI reports; `supervisor/editor.rs` provides their control methods.
The desktop remembers application zoom for helper open/reopen, delivers later
changes, and reports monitor-scale refusals through its existing plugin errors.
Reports are limited to the current playback token/revision and polled at most
once per second per owner/token, exclusively on control.

Before serializing a Cocoa editor request, the supervisor removes the desktop
NSWindow pointer. The helper repeats this check and embeds inside its **own**
NSView/NSWindow on its actual main thread. AppKit child-window ownership cannot
cross a process boundary; helper lifetime follows the bridge owner rather than
using a foreign Objective-C object pointer. X11 server XIDs remain valid remote
transient owners. No native plugin/view/run-loop pointer is sent in GUI reports.
Mapped audio ABI/offsets/version and capture/ACK logic are unchanged.

Exact new tests: `gui::bridge_editor::tests::helper_cocoa_parent_is_local_and_x11_server_id_survives`,
`gui::bridge_editor::tests::helper_monitor_refusals_are_reported_once_and_close_clears_stale_errors`,
plus the lifecycle/run-loop tests below. Existing `tests/bridge.rs` is required.

- SOFTWARE: capture/ACK/restart/document-replacement composition and capability
  re-admission into prepared helpers/plans belong to the hosting sibling.
- EXTERNAL-ONLY: installed helper GUI/main-thread execution on macOS/Linux,
  process shutdown, real plugin GUI load and physical device deadlines.

### `wf-ui-scaling`

The same native parent adapters and desktop owner dispatch now cover direct and
bridged macOS/Linux editors. VST3 receives `IPlugViewContentScaleSupport` factors;
CLAP X11 receives `gui.set_scale`; CLAP Cocoa uses its application-only point
transform. Scale refusal remains a visible error, and monitor changes are
coalesced without closing a healthy editor. Cocoa observes its actual NSWindow
screen/backingScaleFactor. Linux samples the output containing the editor centre
through RandR, accounts for rotation/physical dimensions, and honors Xft.dpi.
Native API dimensions are documented as Cocoa points versus Windows/X11 pixels.

Exact new tests: `monitor_changes_coalesce_once_without_application_double_scaling`
in `src/gui/lifecycle.rs`, `x11_physical_density_and_user_override_have_checked_independent_units`
in `src/gui/scaling.rs`, and the helper refusal tests above. The existing
`application_and_monitor_scaling_are_combined_once` test is retained.

No known remaining SOFTWARE implementation in this worker's scaling scope.

- EXTERNAL-ONLY: actual Retina/mixed-DPI monitors, RandR/Xft/compositor settings,
  fractional density, vendor scale support and native content/focus behavior.

### `fmt-host-fl-as-plugin` — panels only

`crates/windfall-plugin/src/panel/{cocoa,x11}.rs` supplies genuine child panels
under the host's NSView/XID. Both show an exported-program path field, load
control and result text and use the same bounded program-load transaction.
Cocoa uses native text fields/button actions on main, retains its action target,
detaches/releases controls and disposes its per-panel runtime class before
unload. X11 supplies XEmbed metadata, input/focus, UTF-8 XIM handling with a
Latin-1 fallback, and mouse/keyboard load control.

CLAP uses the host timer extension to poll the X11 connection; GUI destroy
unregisters it, including final plugin destruction. VST3 retains the supplied
frame and registers an `ITimerHandler` through its `IRunLoop`; removing/resetting
the frame unregisters before closing the connection. Neither GUI starts an
audio device or GUI worker thread. Native APIs unavailable in an external host
are refused instead of displaying an inert panel.

Exact new test: `panel::tests::native_panel_load_is_complete_and_read_decode_refusals_preserve_state`
in `crates/windfall-plugin/src/panel.rs`. Existing publication and native-export
tests remain required. The common load helper is the integration hook for the
export sibling's dirty/PDC/restart notifications; retain those changes when
merging its `panel.rs`/format work.

- SOFTWARE: release runtime bundling/autoresolution/ABI validation, editor dirty
  and PDC/restart notifications and nested providers belong to
  `gpt/r5-plugin-export`, which owns the rest of this row.
- EXTERNAL-ONLY: actual external DAWs, XIM/input/focus, Cocoa class retirement,
  program loading and multibus recall on native operating systems.

### `fmt-host-au`

Reviewed `au/{mod,cocoa,sys}.rs`, desktop `plugins/{runtime,au_owner}.rs`, the
Tauri startup dispatch and command native-parent path. Host callback layouts,
AudioToolbox Boolean/u32 scopes/elements, CF ownership and selectors retain
their existing system ABI. Cocoa NSRect return handling remains direct on
arm64 and `objc_msgSend_stret` on Intel.

Changed `au/cocoa.rs` to validate the returned Cocoa property size/alignment
against its allocated storage, bound metadata releases and verify the factory
responds to `uiViewForAudioUnit:withSize:` before dispatch. Rejected factories
release their owned reference before trying another class. Desktop AU owner
startup now has a portable cancellation/publication fence: timed-out queued
startup cannot construct a late owner; cancellation during construction and
failed proxy spawn retire installed objects on main. Owner IDs cannot wrap.
The command now uses the calling window's NSWindow instead of hard-coding main.

`scripts/check-macos-desktop.sh` removes the C compiler requirement for this
**non-linking Rust check** using the upstream helper's supported `DOCS_RS=1`
branch. It scopes that environment to cargo check and uses the dedicated
`target/macos-desktop-typecheck` directory. There is no Cargo-config override,
replacement exception shim, removed framework or disabled desktop code.
Wry 0.57.0 explicitly enables objc2's exception feature and uses exception
catching for WebKit scheme registration; its locked dependency cannot safely
be replaced by Rust panic catching. Normal native builds still compile/link
that Objective-C helper with the native Apple toolchain.

Exact new tests: the three `gui::dispatch::tests::*` below. Run the existing
portable `au_model` tests and native AU suites as well. The check script itself
was **not executed**. Desktop Rust type-check evidence still belongs to the
integrator; this document does not promote the row to done.

No known remaining SOFTWARE implementation in the owned AU integration.

- EXTERNAL-ONLY: native AU factory/view behavior, actual CF/Objective-C ownership,
  main-thread callback/window interaction and native link/execution. Windows
  non-linking Apple-target checking is an outstanding integrator gate below.

## Exact portable tests to run on Windows

All are non-ignored and registered through `src/lib.rs -> gui/mod.rs` or the
existing export `panel` module; no unregistered standalone test files were added.

`crates/windfall-plugin-host/src/gui/lifecycle.rs`:

- `wrong_owner_and_closed_editor_never_call_native_resize`
- `constrained_resize_and_invalid_plugin_size_preserve_last_geometry`
- `monitor_changes_coalesce_once_without_application_double_scaling`
- `plugin_teardown_precedes_parent_and_last_view_reference_retirement`
- `owner_registry_reclaims_its_tls_storage_before_library_unload_and_can_reopen`

`crates/windfall-plugin-host/src/gui/run_loop.rs`:

- `unregister_and_reregister_invalidates_pending_dispatch_ticket`
- `timers_coalesce_missed_ticks_and_unregister_during_dispatch`
- `registrations_bound_duplicates_bad_fds_and_timer_intervals`
- `fake_fd_backend_dispatches_only_ready_current_registrations`

`crates/windfall-plugin-host/src/gui/dispatch.rs`:

- `cancelled_queued_gui_initialization_never_constructs_a_native_owner`
- `cancellation_during_gui_initialization_retires_the_unpublished_owner_once`
- `failed_proxy_start_cancels_an_already_published_gui_owner`

`crates/windfall-plugin-host/src/gui/bridge_editor.rs`:

- `helper_cocoa_parent_is_local_and_x11_server_id_survives`
- `helper_monitor_refusals_are_reported_once_and_close_clears_stale_errors`

`crates/windfall-plugin-host/src/gui/scaling.rs`:

- `x11_physical_density_and_user_override_have_checked_independent_units`

`crates/windfall-plugin/src/panel.rs`:

- `native_panel_load_is_complete_and_read_decode_refusals_preserve_state`

Suggested integration commands (not run by this worker):

```sh
cargo test --offline -p windfall-plugin-host --lib gui::
cargo test --offline -p windfall-plugin --lib panel::tests
cargo test --offline -p windfall-plugin-host --lib au_model::
cargo test --offline -p windfall-plugin-host --test editor --test bridge --test vst3_host
bash scripts/check-macos-desktop.sh
cargo check --offline --target aarch64-apple-darwin -p windfall-plugin-host -p windfall-plugin --all-targets
cargo check --offline --target x86_64-unknown-linux-gnu -p windfall-plugin-host -p windfall-plugin --all-targets
```

Also run desktop Linux check, existing Windows desktop/plugin owner suites,
export/publication tests and scoped strict Clippy. On native Mac/Linux, exercise
open/resize/close/reopen, application scale, monitor changes, frame and context
timers/fds, helper editors, and unload before the GUI thread exits. The Linux
implementation links system X11, Xrandr, X11-xcb and xcb; these are system FFI
libraries, not newly added Rust packages. Native link/runtime availability must
be checked alongside the desktop's existing GTK/WebKit system prerequisites.

## Shared-file edits / merge instructions

The platform adapters and new portable modules are this worker's owned code.
These narrowly scoped shared files also changed:

- Host `clap/instance.rs`: widen only the monitor-scale polling cfg.
- Host `vst3/{mod,instance}.rs`: Linux host-context interface/guard, supplied
  before construction and retained until owner retirement. Preserve the sibling's
  capability/routing logic around these additions.
- Host `bridge/{control,helper,supervisor}.rs`: append Unix GUI control variants,
  hook platform control methods, strip foreign Cocoa pointers, report GUI
  refusals. No audio shared-memory layout, capture/ACK or reset edits.
- Export `panel.rs`: platform exports/support and the common bounded load helper.
  Export `clap.rs` and `clap/gui.rs`: native API/parent selection and Linux timer
  lifecycle. Export `vst3.rs`: Linux frame retention/timer attachment only.
  Preserve the export sibling's dirty/PDC notifications and publication logic;
  route editor loads through its final common notification-aware load path.
- Desktop `plugins/{mod,runtime}.rs`: module declarations and Unix-only editor
  scale/report hooks. `commands.rs`: calling Cocoa window and GUI-thread XID
  lookup. `plugins/au_owner.rs`: cancellation/identity/cleanup repairs.
- Desktop `Cargo.toml`: Linux direct edge to **already locked** raw-window-handle
  **0.6.2**, MIT OR Apache-2.0. No new package/version. Cargo.lock was not generated
  or edited; the integrator must refresh its direct dependency list offline.

No source outside this worktree was edited. No other row's DSP, persistence,
registries, generated bindings or tracker status changed.

## Least certain spots

1. Native Cocoa resize/bounds transforms, delegate class disposal, helper
   NSApplication event pumping and mixed Retina screens need real AppKit QA.
2. Native X11/XCB checked requests, XEmbed focus, RandR physical-density policy,
   XIM and external-host timer behavior need Linux link/runtime QA. The struct
   signatures are hand-declared; target checking is mandatory.
3. The locked Tauri/Wry stack has not been type-checked here. The upstream
   DOCS_RS helper skip should allow its Rust check without cc, but this worker
   did not execute that script or verify later build-script prerequisites.
   Never use its dedicated check artifacts for linking/distribution.
4. Context/frame `IRunLoop` ownership and native COM release callbacks require
   native tests, especially cancellation, unload-before-thread-exit and plugins
   retaining their factory host context. Portable tests measure policy/storage
   retirement; they do not prove a vendor's native lifecycle.
5. Shared hosting/export merge hunks above may overlap sibling work. Compile the
   merged result and preserve all existing callbacks, capture/ACK and notification
   assertions. No type-check, Clippy, runtime or native acceptance is claimed.

Primary references used for signature/policy review:

- [CLAP GUI specification](https://raw.githubusercontent.com/free-audio/clap/main/include/clap/ext/gui.h)
- [Steinberg Linux run-loop contract](https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical%2BDocumentation/Provide%2BA%2BRunloop%2BOn%2BLinux/Index.html)
- [Pinned vst3 0.3.0 interface docs](https://coupler.rs/vst3-rs/vst3/Steinberg/Linux/struct.IRunLoopVtbl.html)
- [XEmbed specification](https://specifications.freedesktop.org/xembed/latest-single/)
- [Tauri 2.12.1 native window/dispatch interfaces](https://raw.githubusercontent.com/tauri-apps/tauri/tauri-v2.12.1/crates/tauri/src/webview/webview_window.rs)
- [Wry 0.57.0 exception dependency](https://raw.githubusercontent.com/tauri-apps/wry/wry-v0.57.0/Cargo.toml)
- [objc2-exception-helper 0.1.1 check-only build-script branch](https://raw.githubusercontent.com/madsmtm/objc2/objc2-0.6.4/crates/objc2-exception-helper/build.rs)

HAND-OFF COMPLETE
