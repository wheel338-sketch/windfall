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
