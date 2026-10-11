# Shell, notebooks, multithreading and release — 2026-10-11

Branch `gpt/w3-shell-release`, base `87b87bc2439ec076d0f75c5b4d997c9db1faba95`.
Implementation worker: edit only. **Every new test is written, not executed.**
No builds, tests, generators, workflows, keys, hosts, installers, releases,
pushes or subordinate agents were run/created. Parity status is overseer-owned.

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
Existing frontend generation guards cannot authenticate an old-window command
against a server document identity; delayed dispatch crossing asynchronous
project replacement remains a separate unresolved gate. Native window APIs and
new command argument/token ordering are first compile/runtime risks to check.

## Remaining streams

Detached windows, scaling, tutorials, languages, updater and scheduler work
continues below. None is presently claimed complete.

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

Partial: manual link/manual content service is still absent; guides are explicit
state-gated assistants, not anchored highlights of every control. Mounted full
native record→kept clip and completed file export walkthroughs, error/device
loss and keyboard focus checks remain. Export completion currently observes a
successful event in the current project/window, rather than a dedicated tutorial
export request token. Nonmodal focus and opening child dialogs require native
webview verification. Pattern placement currently creates a lane and clip as
two ordinary history operations when the playlist starts empty.
