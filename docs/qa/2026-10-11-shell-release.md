# Shell, notebooks, multithreading and release — 2026-10-11

Branch `gpt/w3-shell-release`, base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
Implementation worker: edit only. **Every new test is written, not executed.**
No builds, tests, generators, workflows, keys, hosts, installers, releases,
pushes or subordinate agents were run/created. Parity status is overseer-owned.

## Final implementation claims

All assigned rows have committed implementation or acceptance-test source.
There is no executed acceptance evidence from this worker. The work is ready
for integration, not a claim that all eight rows are done.

| Row | Claim | Remaining before full-row acceptance |
| --- | --- | --- |
| `wf-detached-windows` | Partial | Caller generation on custom mutation IPC; actual two-WebView focus/menu/edit/close/reopen/replacement races; open-window monitor removal/work-area checks. |
| `wf-ui-scaling` | Partial | Release bridge editor hosting; macOS/Linux native editors; independent VST3 fixture; live monitor DPI notifications; physical input, WebGPU loss and each-scale large-project performance gates. |
| `wf-help-tutorials` | Partial | Full mounted/native recording and export walkthroughs, dedicated export request correlation, device-loss/error/focus cases. |
| `wf-languages` | Partial | Complete translation of remaining main-panel interiors, dynamic/native/dialog text and palette search; translator review and expansion checks. English/Spanish catalogs cover every currently defined key. |
| `core-lifetime-free-updates` | Partial | Session-wide installation shutdown lease, bounded streaming/redirect policy, actual signed/corrupt fixtures, operator values and real installers/signing/trust checks. |
| `wf-multithreading` | Partial: foundation only | Production processor/mixer/plan integration, complete per-track ownership, deterministic edge reduction, plan-swap retirement, deadline-output policy, live thread setting and measured realtime/offline acceptance. Production remains serial. |
| `fx-fruity-notebook` | Full-row candidate for song-position playback cues | Regeneration and written acceptance execution; verify native save/reopen/history/follow cleanup. No audio-parameter page automation is introduced. |
| `fx-fruity-html-notebook` | Partial | Execute hostile-content fixtures and packaged three-engine CSP/resource/IPC isolation checks. Active remote HTML/navigation is unsupported. |

The following sections record the exact scope, written tests and commands for
each row. The integration checklist at the end orders the highest-risk gates.

## Notebook changes

`NotebookPage` gains optional, default-omitted `html` and `startTick` fields.
Existing checked ReplaceNotebook, patch, history and project save paths carry
both. Legacy JSON remains identical for plain uncued pages. Cues are checked
against MAX_SONG_TICKS. The editor saves HTML source, provides an opt-in safe
preview, and follows the latest page cue during song playback, including seeks
and loops. Manual/pattern/stopped mode does not advance pages or create edits.
Equal cues choose the earlier page. Feed subscriptions stop on unmount.
No new command or audio-side automation target is added.

HTML is parsed inside an inert template and reconstructed from an HTML-only,
attribute-free text/table allowlist, with resource and executable subtrees
removed, node/depth bounds, no external sanitizer dependency. Links display
their HTTP(S) address as text. The preview is a sandbox iframe with no allowed
capabilities, no same-origin access, no script/IPC and a deny-all resource CSP.
Active hyperlinks/remote pages are intentionally unsupported.

Application null CSP is replaced by a proposed local-only policy. WASM needs
`wasm-unsafe-eval`; Base UI, canvas controls and CSS zoom need inline styles;
local asset protocols/data/blob are limited to their consumers. Tauri IPC is
allowed only in the application, never the notebook. Dev websocket exception
is restricted to local Vite port 1420. **Policy is authored, not runtime-tested**;
native WebView2/WKWebView/WebKitGTK startup, IPC, WASM, fonts, assets, dialogs,
canvases and sandbox tests remain required before acceptance.

Written tests: `crates/windfall-project/tests/notebook_playback_html.rs`
(`notebook_cues_html_history_and_legacy_json_roundtrip`,
`notebook_out_of_range_cue_is_refused_atomically_and_on_load`);
`features/notebook/playback.test.tsx` (cue boundaries/order, mounted song-only
follow/feed cleanup, saved cue/HTML transaction);
`features/notebook/safe-html.test.tsx` (hostile markup, namespace/entity tricks,
mounted permissionless frame/CSP, sanitizer bounds).

Integrator (after regeneration, not run here):
```powershell
cargo test -p windfall-project --test notebook_playback_html --test notebook_acceptance_qa
pnpm --dir apps/desktop test src/features/notebook --maxWorkers=2
```

Generated type change: regenerate NotebookPage and dependent bindings/WASM.
Compile risk: ts-rs optional boolean/nullable cue shape and frontend optional
properties; sanitizer fixture expectations; CSP behavior in native packaged
webviews. `fx-fruity-notebook` is a summary-level candidate awaiting execution;
audio-parameter page automation remains absent. `fx-fruity-html-notebook` is a
safe local HTML subset, with active links/remote HTML unsupported and native
CSP acceptance pending.

## Detached windows

Adds atomic saved `panel-layouts.json` outside the document. Geometry is
monitor-relative in logical pixels, restored against current monitor identity
and DPI; absent monitors fall back to an available monitor, offscreen/oversize
frames clamp within it, corrupt files recover empty. OS chrome is subtracted
when restoring inner size to avoid growing on reopen. Native move/resize records
in memory and close saves; no IO is introduced in the audio callback.

Realtime subscribe now owns a random token. Last UI unsubscribe releases the
native sender, old channels stop delivering, and delayed old cleanup cannot
remove the replacement sender. Window destruction still releases all feeds
and analyzer slots using the existing shell hook.

Written, not executed: `panel_layout.rs` tests
`layout_recovers_removed_monitor_and_negative_desktop_at_new_dpi`,
`layout_clamps_offscreen_and_corrupt_data`, `layout_file_roundtrip_and_corrupt_recovery`;
registered `session/tests/detached_windows.rs` tests
`two_windows_racing_edits_share_one_document_and_ordered_revision_stream`,
`close_reopen_stale_cleanup_and_replacement_release_realtime_owners`;
`lib/ipc/tauri-realtime.test.ts` shared-feed/cleanup/renewal/stale-delivery case.

```powershell
cargo test -p windfall-desktop panel_layout
cargo test -p windfall-desktop detached_windows
pnpm --dir apps/desktop test src/lib/ipc/tauri-realtime.test.ts src/features/layout/detach.test.tsx src/features/layout/shortcuts.integration.test.tsx src/features/layout/project-replaced.test.tsx --maxWorkers=2
```

Partial: native two-WebView edit/close/reopen/project replacement and device-loss
races, monitor removal while an already-open window is running, maximized
geometry/taskbar work-area correctness, focus/menus on real Windows/macOS/Linux
remain. Session tests prove shared owner/revision order, not native event delivery.
Native windows now receive a transient server generation envelope at startup
and project-loaded events. Dispatch/undo/redo/history-jump carry that generation.
Session validates it while creating the edit/history candidate under State;
the existing prepared admission rechecks generation/replacement after slow work.
Controller tweak recording also refuses a departed generation. A late startup
snapshot cannot reset the replacement identity. This changes only native window
IPC envelopes, not saved projects or generated musical types; the raw Session
Event and document_snapshot APIs remain compatible for other callers/tests.
Concurrent optimistic preparation may explicitly refuse an edit; the two-window
fixture retries the refused AddPattern against the same generation and verifies
ordered revisions. This is not a claim that arbitrary conflicting commands are
automatically rebased.

Written, not executed: additional detached-window native tests
`stale_window_dispatch_and_history_cannot_touch_a_replacement_document`,
`prepared_window_edit_loses_admission_when_the_project_is_replaced`;
`lib/ipc/tauri-document-owner.test.ts` generation arguments, replacement envelope,
late startup snapshot and listener release. Run alongside the commands above:
```powershell
pnpm --dir apps/desktop test src/lib/ipc/tauri-document-owner.test.ts src/lib/store/project.test.ts src/features/layout/project-replaced.test.tsx --maxWorkers=2
```
Remaining: custom mutation IPC beyond ordinary dispatch/history (automation,
prepare-clip, plugin-add, file imports that begin after replacement) needs the
same caller identity to close every stale-window entry point. Native shell event
wrapping/State generation mirror, changed command arguments and native geometry
are the first compile/runtime risks to check. Two actual WebViews remain external.

Detached composition also initializes its transient panel shortcut scope (without
changing the saved center tab) and mounts command palette, Settings, Export,
recording, MIDI and import dialogs that its real actions can open. This prevents
an action from opening invisible state in a window missing that dialog. Mounted
`features/layout/panel-window.test.tsx` scope/cleanup and actual Settings/Export/
recording surface cases are written, not executed. Its mixer body is stubbed;
native window focus/menu isolation still requires two separate WebViews.
```powershell
pnpm --dir apps/desktop test src/features/layout/panel-window.test.tsx src/features/layout/detach.test.tsx src/features/help/guided-tutorial.test.tsx --maxWorkers=2
```

## Free updates / release

Native Tauri updater plugin is registered, with separate check, download/verify
and explicit install/restart commands restricted to the main window. Stable
and preview use distinct operator-baked HTTPS endpoints. A serialized operation
gate and checked selection token prevent concurrent/stale selections. App
rejects same-version/downgrade, stable prerelease, wrong/missing channel,
unsigned/non-HTTPS download and different/missing project-format metadata.
Tauri's `Update::download` verifies the cryptographic signature before bytes
enter the installable cache. Timeouts/offline and verification/install failures
surface in Settings → Updates with retry. Installation refuses dirty project,
playing transport and active recording; the module never rewrites a project.

Configuration: updater artifacts enabled; plugin has empty operator placeholders
and no frontend updater permission (custom commands enforce the policy).
Build-time values required: `WINDFALL_UPDATE_PUBLIC_KEY` (public verification key),
`WINDFALL_UPDATE_STABLE_URL`, `WINDFALL_UPDATE_PREVIEW_URL`. Operator alone
provides the matching private signing key/password, signed HTTPS release host
and installer artifacts, Windows signing certificate, Apple developer identity/
notarization credentials, and Linux package/signing distribution choices.
Each manifest must add `windfall: { channel: "stable" | "preview",
projectFormatVersion: 1 }` alongside standard Tauri version/platform URLs/
signatures/notes. Never publish over `v0.1.0-alpha.1`. No workflow added/run.

Dependencies: `tauri-plugin-updater = 2.13.3` (MIT OR Apache-2.0), explicit existing
transitive `semver = 1` / `url = 2` (MIT OR Apache-2.0). GPL-compatible. Integrator
must resolve/update Cargo.lock; dependency installation was not run here.

Written, not executed: `updates.rs` policy fixture
`update_fixture_rejects_downgrade_channel_format_and_unsigned_packages`, operation
gate `update_operations_are_serialized_and_recover_after_failure`;
`features/updates/section.test.tsx` mounted verification/save/playback gating,
offline/corrupt retry, missing verification, operator-unconfigured cases.

```powershell
cargo test -p windfall-desktop updates
pnpm --dir apps/desktop test src/features/updates/section.test.tsx src/features/settings/settings.test.tsx --maxWorkers=2
```

Partial: actual plugin cryptographic fixture using a pre-existing signed artifact,
redirect/download-size enforcement before allocating the plugin's complete
buffer, installer rollback/upgrade/uninstall, interrupted download/installation,
signing trust and actual three-OS packages remain. Do not count mocked corrupt
errors as signature proof. The plugin handles signatures; our fixtures exercise
policy/UI. Detached/import edits racing the final dirty check are not frozen by
a session-wide shutdown lease; inspect this before enabling release installation.
Updater APIs/async command Send bounds, plugin initialization with empty config,
packaging flag and dependency resolution are the first compile checks.

Primary API sources read (implementation references, not runtime evidence):
[Tauri updater](https://v2.tauri.app/plugin/updater/),
[Update download/install](https://docs.rs/tauri-plugin-updater/latest/tauri_plugin_updater/struct.Update.html),
[UpdaterBuilder](https://docs.rs/tauri-plugin-updater/latest/tauri_plugin_updater/struct.UpdaterBuilder.html).

## Languages

Adds typed English/Spanish catalogs, plural forms, named interpolation, Intl
number/date formatting, a Settings language selector, persisted `windfall.language`
outside the document, cross-window storage rehydration without write echoes,
and document language accessibility metadata. Every catalog contains all defined
keys with matching placeholders. Static action IDs map to catalog messages;
user-named recent projects are not passed through action-title translation.
Menus, all main panel headings/tabs/dock controls, builtin transport/action
labels, Browser navigation, notebook primary controls and Settings core navigation
consume the catalog. English remains the compatibility fallback for unmigrated
messages. The second catalog is complete **for the migrated message set**.

Written, not executed: `lib/i18n/catalogs.test.tsx` all-key/plural/placeholder
coverage, Spanish plural/decimal/date formatting, mounted language/panel
translation/persistence, other-window rehydration/invalid recovery/cleanup.

```powershell
pnpm --dir apps/desktop test src/lib/i18n/catalogs.test.tsx src/features/layout/menus.test.ts src/features/layout/detach.test.tsx src/features/settings/settings.test.tsx src/features/notebook/panel.test.tsx --maxWorkers=2
```

Partial: remaining panel-local inspectors, dynamic status/error/help text, MIDI/
plugin dialogs, full command palette search and native dialog/menu localization
are not fully migrated. Existing technical number entry retains its canonical
parse contract; all number/date consumers have not yet adopted Intl helpers.
Do not call Spanish a complete application/main-panel translation. Translator
review, text expansion at 75–200% and accessibility/native OS runs remain.
No IPC/generated type or project-format change for language preferences.
Notebook labels, buttons, hints, validation and inert-preview accessibility
titles are now fully migrated too; mounted Spanish notebook controls/preview
and unchanged user-content case is written, not executed in catalogs.test.tsx.

## Help and tutorials

Getting started is now nonmodal so the actual editors remain interactive.
Retains existing quick actions and adds selectable build-beat, record, import,
export guides. Each Next gate inspects the real document/transport/recording/
export state; cancelled/failed imports and exports cannot count as completion.
Beat placement uses ordinary checked AddPlaylistTrack/AddClips commands, song
play uses the real transport, and recording/import/export use existing actions.
Project replacement closes/resets the guide and releases its export listener.
Panel-header Help opens contextual instructions for that panel, including in
detached windows. All new guide/context instructions have Spanish messages.

Restart reconciliation found the help dialog was initially mounted only in the
main shell. It now belongs to shared window overlays (one instance per window).
The additional mounted detached-header → actual dialog → project-replacement
cleanup test is written, not executed.

Written, not executed: `features/help/guided-tutorial.test.tsx` mounted real-WASM
note gate and undoable clip placement, real picker cancellation, recording/export
evidence predicates, active/detached context; existing getting-started tests must
run for nonmodal/menu/action/replacement regressions.

```powershell
pnpm --dir apps/desktop test src/features/help/getting-started.test.tsx src/features/help/guided-tutorial.test.tsx src/lib/i18n/catalogs.test.tsx --maxWorkers=2
```

Bundled English/Spanish manual topics now open from Read manual inside Help,
without network/IPC/navigation. The guide opts out of the shared backdrop and
uses Base UI disablePointerDismissal to keep focus/outside edits from closing
it. The dialog primitive gains an opt-in showOverlay=false prop; every other
dialog retains its default behavior. A mounted actual help-dialog/outside-focus/
manual-expansion case is written, not executed. Check the pinned Base UI prop
and native nonmodal behavior before acceptance; reference:
[Base UI Dialog](https://base-ui.com/react/components/dialog).

Partial: guides are explicit state-gated assistants, not anchored highlights of
every control. Mounted full
native record→kept clip and completed file export walkthroughs, error/device
loss and keyboard focus checks remain. Export completion currently observes a
successful event in the current project/window, rather than a dedicated tutorial
export request token. Nonmodal focus and opening child dialogs require native
webview verification. Pattern placement currently creates a lane and clip as
two ordinary history operations when the playlist starts empty.

## Multithreading: contained scheduler foundation, not live integration

Determinism/allocation acceptance source was written before the implementation.
`windfall-engine::track_workers` adds a safe ownership-transfer boundary for the
existing `TrackProcessor` (post-slot EQ/stereo). Each value contains prepared DSP
and two fixed 256-frame buffers. Control-side construction precreates 0–7 workers
(1–8 total threads) and capacity-one `rtrb` queues using the existing dependency.
The callback-facing submit/harvest/budget APIs move owners without closures,
spawn, allocation, locks, OS wakes or blocking waits. Single-thread/refused work
returns the entire owner for the exact same serial process method.

At most 256 rounds × 7 queue probes are allowed. This is a known operation budget,
**not a measured microsecond guarantee**. Idle workers park for 50 microseconds;
OS dispatch latency remains external. A missed budget latches fallback for NEW
work and leaves the in-flight owner with its worker. It must be harvested before
replacement/retirement; the callback cannot race a second copy of DSP state.
Control-side destruction stops and joins workers, never in the callback.

`GraphWaves` compiles consecutive independent ranges of a supplied topological
order with all audio/detector/aux dependencies. It rejects bad orders/cycles;
Current/hosted-unsafe tracks can be singleton barriers. Results occupy indexed
slots so the caller can preserve the original floating-point accumulation order.
It does not substitute completion order for plan order.

Written, not executed: registered `tests/engine/multithreading.rs`:
`track_workers_match_serial_bits_across_blocks_and_reversed_completion_order`,
`bounded_handoff_exhaustion_and_deadline_miss_return_or_retain_the_owner_without_allocator_calls`,
`one_thread_and_invalid_lane_use_serial_without_spawning_or_losing_track_state`,
`graph_waves_preserve_serial_sum_order_and_include_detector_dependencies`.
Unit tests `worker_dsp_allocator_is_guarded_on_the_actual_worker_thread` and
`malformed_blocks_do_not_change_the_owned_buffers` cover actual worker-thread
DSP allocator calls and atomic refusal of malformed blocks. The integration
allocator guard covers submit/poll/refusal/serial process/recovery on the caller.

```powershell
cargo test -p windfall-engine --lib track_workers
cargo test -p windfall-engine --test engine multithreading
cargo test -p windfall-engine --test engine realtime
```

**Partial. Production processor/mixer/plan are still single-threaded.** No thread
setting is exposed as if it changed live playback. The serial mixer combines
mutable effect slots, aux/sidechain/taps, PDC and Current routing inside one track
loop. Shipping parallel execution needs a prepared per-track owner that covers
that WHOLE loop, deterministic edge reduction, deadline-output policy, delayed
owner retirement during plan swaps/project replacement and offline/live parity.
The present boundary covers only post-slot EQ/stereo and is a reusable source
foundation, not a multicore DAW claim. Full-render bit identity across routing,
automation, plugin failure and plan swaps, callback allocator checks of the live
path, thread-count persistence/mounted Settings, priority/affinity, and measured
worst-case budgets on Windows/macOS/Linux remain. First compile checks: Send on
TrackProcessor, ring queue ownership and large by-value error/clippy policy.
The large-error lint is explicitly allowed only on submit: boxing a refused
TrackJob would add a callback allocation. Caller retains the by-value owner.

## Interface/native editor scaling

The existing shell scaling gates in `docs/UI-SCALING.md` were reviewed. New
native plumbing carries the existing persisted UI factor through one serialized,
coalescing listener per window and a dedicated custom IPC command. Plugin runtime
stores the preference for reopen and calls open native editors on their owning
GUI thread. It does not dispatch a document command or alter audio settings.
CLAP embedded Windows editors use gui.set_scale and their reported new size;
VST3 Windows editors query IPlugViewContentScaleSupport, call setContentScaleFactor
and honor their reported dimensions/normal frame callbacks. Monitor DPI comes
from the editor HWND and is combined exactly once with the application factor.
Unsupported/refused scaling reports an error and keeps the editor alive.

Written, not executed: `gui/scaling.rs` test
`application_and_monitor_scaling_are_combined_once`;
`tests/editor.rs` test
`native_content_scale_resizes_restores_reopens_and_keeps_refused_editor_alive`;
`features/plugins/editor-scale.test.tsx` real mounted Settings preference/no
document-or-audio change/persist/cleanup, slow native call coalescing and refusal
recovery. The existing gain test plugin now supports scale negotiation; Invert
mode also injects a scale refusal solely for the fixture. No proprietary plugin
is used. Source/API review corrected get_size to its Option contract and repaired
the two message catalog files from Windows-1252 to UTF-8.

```powershell
cargo test -p windfall-plugin-host --lib gui::scaling
cargo test -p windfall-plugin-host --test editor
pnpm --dir apps/desktop test src/features/plugins/editor-scale.test.tsx src/lib/ui-scale.test.tsx src/features/piano-roll/layer-scaling.test.tsx src/features/browser/layout-scaling.test.tsx src/lib/i18n/catalogs.test.tsx --maxWorkers=2
```

**Partial:** the existing release audio/state bridge explicitly refuses native
editors. This work does not invent a bridge GUI protocol or bypass containment.
Its owner must deliver editor hosting/scaling there before release parity. macOS
and Linux native editor ownership remain missing in the baseline host. Dynamic
editor-monitor DPI transitions are read on scale change/reopen, not yet wired
to WM_DPICHANGED. Required physical Windows/WebView2 pointer/touch/trackpad and
detached monitor transitions, macOS WKWebView, Linux WebKitGTK, real WebGPU
device-loss, and scale-specific 10k/50k performance checks remain external.
First compile checks: pinned CLAP gui signatures, VST3 generated COM
scale trait, Win32 GetDpiForWindow availability, runtime clone/preferences and
backend interface additions. No new generated IPC types or dependencies.

Primary native API reference:
[VST3 content-scale contract](https://steinbergmedia.github.io/vst3_doc/base/classSteinberg_1_1IPlugViewContentScaleSupport.html).
CLAP source API reference:
[Clack GUI host wrappers](https://raw.githubusercontent.com/prokopyl/clack/main/extensions/src/gui/host.rs).
VST3 has no independent native editor fixture yet (its existing fixture returns
no view); write/execute a real COM-view resize/refusal case before accepting it.

## Shared seams and regeneration

Brief-listed shared seams touched, with new behavior delegated to adjacent modules:

- `apps/desktop/src-tauri/src/commands.rs`, `lib.rs`, `session/mod.rs` and
  `session/tests/mod.rs`: custom native commands, state/plugin registration,
  owner generation/realtime lifetime and test registration.
- `apps/desktop/src/lib/ipc/backend.ts`, `tauri.ts`, `mock.ts`: editor scaling,
  native generation envelopes/arguments and subscription token release.
- `apps/desktop/src/features/layout/app-shell.tsx`: detached composition and
  shared help overlays; `use-app-boot.ts` adds editor-scale listener lifetime.
- `crates/windfall-engine/src/lib.rs` and `tests/engine/main.rs`: isolated worker
  module export and acceptance registration. Production `processor.rs`,
  `mixer.rs`, and `plan.rs` are untouched.

Other cross-owner surfaces to reconcile during merge: session
`controllers.rs`, `edit.rs`, `files.rs`, `realtime.rs`, native `shell.rs`,
`plugins/runtime.rs`, plugin-host instance traits and CLAP/VST3 editor/instance
modules, `gui/{mod,win32}.rs`, the test-plugin GUI/editor fixture, action button/
menu item translation, `components/ui/dialog.tsx`, layout menu/panel/title/
workspace and Settings composition. Dialog's optional `showOverlay` preserves
the default for existing callers. `NotebookPage` owns the persisted additions
in `crates/windfall-project/src/notebook.rs`; existing notebook test literals
gain defaults. No musical Command variant, descriptor order, EVERY_COMMAND
table or parity tracker is changed.

Integrator must resolve the new updater dependencies into `Cargo.lock`, then
regenerate **all** project/IPC TypeScript bindings with the normal generator
(not hand edits), including `NotebookPage`'s optional `html`/`startTick` and
referencing Notebook/Project/Command/patch types. Rebuild and commit
`apps/desktop/src/lib/ipc/sim/windfall_sim.wasm` and its metadata. Existing
generator outputs such as descriptors/automation fixtures may be rewritten;
no new DSP descriptor is intended. The native WindowDocumentSnapshot envelope
is a private custom IPC contract maintained in the Tauri backend, not a saved
project field or exported musical binding.

Integrator-only commands, not run here (Git Bash for shell scripts on Windows;
scripts load the existing MSVC environment):
```sh
cargo check -p windfall-project -p windfall-engine -p windfall-plugin-host -p windfall-desktop
bash scripts/gen-bindings.sh
bash scripts/build-sim.sh
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```
Run the per-row test commands above only after regeneration. Include existing
native session history/edit/recording/realtime and frontend notebook/help/
settings/detach/replacement regressions. Use the repository's normal final
format/lint/clippy checks after resolving source errors; none ran in this worker.

## Ordered integration risks and external gates

1. **Compile and generated contracts first:** updater plugin version/API and
   asynchronous Send bounds; TrackProcessor Send and queue ownership; ts-rs
   optional NotebookPage fields; native generation event envelope/arguments;
   Base UI nonmodal prop; CLAP/VST3/Win32 scale interfaces. Generated artifacts
   and Cargo.lock are intentionally stale until the integrator regenerates.
2. **Project/window lifetime:** exercise late snapshots, optimistic refusal,
   two live WebViews racing edits/history/replacement and feed close/reopen.
   Extend caller identity to the remaining custom mutation IPC before claiming
   complete stale-window isolation.
3. **Release installation:** add a session shutdown lease that admits no edits,
   imports or recording between the final dirty check and installation. Verify
   an existing correctly signed artifact and corrupted signature/payload;
   bounded transfer/redirects, offline retry and interruption. Policy mocks
   cannot establish cryptographic or installer safety.
4. **Realtime scheduler:** do not enable this foundation in the production
   callback until the complete track loop/plan-swap lifetime and deterministic
   accumulation are integrated and allocator/bit-identity tests pass. The
   256-round probe bound has no measured scheduling-time guarantee.
5. **Packaged rendering/security and usability:** run the authored application
   and deny-all notebook CSP on Windows WebView2, macOS WKWebView and Linux
   WebKitGTK. Verify startup/IPC/WASM/fonts/local audio/canvases/dialogs plus
   hostile HTML with no execution, remote resource request or IPC. Complete
   physical scaling/focus/device-loss and language expansion review.

External operator values are precisely the three WINDFALL_UPDATE_* build-time
values listed above, their matching private signing key/password and signed
HTTPS manifests/artifacts. Operator also owns platform signing identities,
notarization and installers/packages. No key, host, workflow or release was
created. Actual Windows installer upgrade/uninstall/recovery and macOS/Linux
native builds, signing/trust/install/upgrade/uninstall checks remain outside
this edit-only worker. Keep alpha version immutable; select and record the
release version/channel/project-format manifest before publishing.
