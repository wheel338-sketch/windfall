# Importing an FL Studio project

Choose **File > Import FL Studio project...**, select an `.flp`, and review the conversion. Category counts distinguish exact musical data, approximations, silent/bypassed placeholders and omitted data. Expand a category or Diagnostics to inspect details. The current document and playback remain unchanged until **Open converted project** succeeds. Existing unsaved changes use Windfall's Save / Don't save / Cancel prompt.

Missing audio does not prevent import. In Sample folders, select a search folder, the installed FL Studio folder, or the FL Studio user data folder. Every change prepares a fresh review. Search folders scan at most 20,000 entries, eight subdirectory levels, without following symlinks; only a unique filename match is selected. Remove broad folders when multiple files share a name. Unreadable files retain their sample references and warnings so the existing sample reload/relink workflow can recover them. Windfall supplies no Image-Line content.

The converted document starts unsaved. Save it as `.windfall`; the source `.flp` is never written. `Project.retainedPlugins` is a serde-defaulted additive field, so existing project files load without a migration. Each retained entry holds source plugin identifiers, descriptive source channel/track/slot, hosted metadata when recognized, and original opaque state bytes. Descriptive ids survive deletion and are not executable runtime plugin bindings. Save/reopen keeps these states. The Imported sounds status button opens their names, locations, hosted metadata and retained-byte counts at any time, including after reopening. They do not emulate proprietary instruments or establish identical sound.

The shell API is `flp_preview(path, options)` returning a review token/report, `flp_open(token)` returning a document snapshot, and `flp_cancel(token)`. Conversion, project validation, sample resolution and decoding finish before installation. A shared project replacement ticket refuses installation after another project request or an intervening edit. The UI cancels abandoned review tokens; neither failure nor cancellation replaces the active document.

The browser/mock uses the same Rust reader/converter compiled into `windfall-sim` (`flp_convert`) and the same unsaved document semantics (`doc_new_unsaved`). Its file picker reads real selected FL bytes. Browser sample folders cannot grant arbitrary filesystem access: missing external audio is reported with guidance to use the desktop app. Mock tests use its existing simulated sample filesystem, not decoded browser audio.

## Verification

Native synthetic-container integration tests exercise successful checked/unsaved import, diagnostics, missing samples, unique/ambiguous sample searches, cancellation and parse failures, intervening edits, source-byte preservation, and opaque-state save/reopen. Frontend tests run generated FL bytes through the wasm importer and cover review, replacement, save/reopen, picker cancellation, malformed files, discard cancellation and stale previews. Historical, property and hostile-input coverage remains in `coverage.md`. No additional third-party package is needed; existing dependencies retain their prior licenses.

## Fidelity extension awaiting integration (2026-10-11)

Import conversion now builds independent arrangement clip/lane layouts and
retains original native states for mapped sounds, as well as placeholders.
The Imported sounds report can therefore include an archival original state
beside a playable approximate Windfall instrument/effect. These bytes are not
restored to the approximate processor. Selected-arrangement meters/labels remain
shared timeline data. Desktop review now resolves requested WAV smpl loop
metadata after decoding; only the first forward/alternating loop is represented,
with approximate seam/release behavior and warnings. Browser conversion does not
resolve external loop files. Absolute stretch-time resolution and broader
real-version acceptance remain open. See
[coverage](coverage.md) and [worker hand-off](../qa/2026-10-11-formats.md).
No desktop acceptance was run by the edit-only worker.
