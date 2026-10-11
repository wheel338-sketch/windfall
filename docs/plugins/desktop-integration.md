# Desktop CLAP integration

The Windows desktop app scans default CLAP/VST3 directories and configured folders through its own executable in scanner-helper mode. Options → Plugin manager, the browser, and the mixer's Add effect menu open the manager. Search identifies plugins by their discovered name/vendor/format. Adding an instrument selects its new rack channel and opens channel settings; adding an effect selects and opens its mixer inspector. The inspector shows actual discovered native parameters, their ranges, read-only/stepped flags, automation actions, and native editor controls.

Only CLAP entries can be added in this checkpoint. VST3 entries are discoverable but disabled with an explicit explanation: that backend requires releasing its active audio adapter before state capture. Active state capture must not be bypassed to enable VST3. macOS/Linux compile the plain model and engine seam, but native desktop hosting returns unavailable immediately; their application-main-thread/editor ownership is not implemented. Browser/mock mode preserves and edits plugin data through the same WebAssembly document and reports native hosting unavailable.

## Discovery and persistence

`plugins.json` in the application config directory is the host's scanner catalog/cache/blocklist. `plugin-folders.json` stores configured directories. The manager limits folders to 64 and results/files to 4096. Scanner crashes and deadlines affect the child process, not the desktop process. The host runner retains its existing bounded protocol and per-probe deadlines; Retry unblocks a file before rescanning. Status reports completed/total/current and errors. Successfully scanned files are approved by path, plugin ID, size and modification time; changed or unapproved files remain unavailable until a successful scan. This is discovery isolation, **not audio crash containment**: playback loads native code into Windfall's process.

`Project.plugins?: PluginBinding[]` is plain serde/TypeScript data without a native host dependency. A binding contains `target` (`{type:"instrument",channel}` or `{type:"effect",effect}`), `format`, `path`, `id`, `name`, opaque `state` bytes, and discovered `parameters`. Each parameter stores stable native `id`, name, min/max/value and stepped/readOnly/automatable flags. CLAP state uses the host's checked WFPS container. Missing plugins retain their binding/state/parameters; instruments are silent and effects pass input through, with an inspector status message. Deletion, duplication, undo and redo preserve binding ownership. Builtin placeholders provide existing channel/slot identity but their builtin parameter editors/reset actions are replaced by native controls. Replacing an effect with a builtin removes the binding.

Automation retains the existing `instrumentParam`/`effectParam` target shape: `param` indexes the binding's persisted discovery table, whose entry supplies the native ID and range. No builtin parameter count/range applies to a hosted binding. Read-only and nonautomatable native parameters reject automation.

## Audio and instance ownership

`windfall_engine::plugins::PluginFactory` prepares `HostedInstrument`/`HostedEffect` audio halves outside processing; `SamplePool` carries an optional factory. State preparation accounts for reported instrument/effect latency in the existing graph PDC, compensates effect dry/wet paths and fades wet/bypass changes. Native latency above one second is refused before allocating compensation. Notes are emitted at existing exact sample boundaries; automation uses native IDs; musical transport and tempo accompany processing. Host adapters provide latency, tails, sleep/voice handling and bounded event processing.

A dedicated Windows owner thread creates, restores, activates, idles, captures state, edits and destroys native instances, and pumps Win32 editor events. Audio halves cross to the engine. Their processing, parameter/note/transport delivery and current-instance publication do not use the owner queue, locks or allocation. The engine's existing control-side garbage retirement returns audio halves to the owner, where editors close, adapters release/deactivate and instances/modules are destroyed. Control requests have a 15-second reply deadline; an unresponsive in-process plugin can still stall its owner thread or crash the process.

Playback preparation can be speculative. Creation therefore does not select a current editor/state instance. Each installed non-retiring audio half publishes its token with one atomic store when it receives an audio block, including stopped transport blocks. Offline render/stem factories have a separate role and never publish playback selection. The owner accepts editor/state/idle updates only for the selected token and document/retry revision. State/editor capture also checks exact binding path/ID/format/state, preventing equal numeric targets in another project from capturing an old instance. Installing a missing-plugin placeholder clears obsolete selection. Headless sessions without audio blocks preserve the saved state instead of guessing which prepared instance is live.

Editor parameter gestures are assigned history gesture IDs. Latest pending parameter/state edits are retained when document dispatch temporarily rejects them, and filtered by revision plus exact current token before retry. Automation-origin parameter notifications without editor gestures are not written back as document edits. State/latency/restart notifications trigger control-side persistence/repreparation. Save, recovery backup and offline export capture native state after releasing the session document lock. Export prepares independent instances through the same engine factory path.

## IPC additions

Commands: `plugins_state`, `plugins_scan({retry?:string})`, `plugins_add_folder({folder:string})`, `plugins_add({path:string,id:string,track?:TrackId})`, and `plugin_editor({target:PluginTarget,open:boolean})`. `plugins_add` returns the ordinary `DispatchResult` with created channel/effect ID and patch. Document commands are `addPluginInstrument`, `addPluginEffect`, `setPluginParam`, and `setPluginState`; patches optionally carry `plugins`.

`PluginManagerState` contains folders, entries, blocked entries, scanning/completed/total/current/error, and per-target instance errors. `PluginEntry` contains path/format/id/name/vendor/instrument/usable/error. `PluginInstanceStatus` contains target/error. All nullable status values are explicit nulls; the project plugins field is omitted when empty.

When integrating recording, hold the session's recording-idle guard across manager add/config/editor mutation and project replacement, and let the pending native edits reconcile after recording stops. Async prepared plans must retain the factory and audio role and may be dropped before install: selection must continue to happen only from installed audio blocks. For speculative document replacement, `Runtime::prepare_document()` returns a factory with a reserved revision while leaving the live revision untouched. Set that factory on a temporary preparation pool. After final ticket/recording guards and `Controller::set_prepared_project`, call the staged factory's `install_document()`. Store the session pool with the base manager runtime factory for later edits and retries. Rejected preparation drops the staged halves without publishing their revision. Native retirement must stay in control-side garbage collection.

## Verification and limits

Synthetic engine tests establish nonzero hosted instrument/effect rendering, silent missing instruments, stable-ID edits with instance reuse, zero allocator calls during processing, native-range automation, exact note starts, and offline compensation of a 32-frame native effect delay. Windows desktop tests build a real CLAP fixture and the actual desktop scanner-helper executable; they verify isolated discovery, session instrument/effect playback, offline WAV audio, state/parameter save/reopen, missing-plugin retention, and speculative/export/revision/binding ownership. UI tests exercise manager search/retry/add/selection, native range clamping, undo and editor routing; exhaustive WebAssembly command tests cover all four new document commands.

No hardware output or audible/manual editor verification is required by these tests. External installed plugins and OS editor behavior remain optional manual checks. Plugin-specific nondeterminism, crashes, hangs, unsupported layouts and parameter metadata changes are outside the synthetic fixture guarantee. The project uses the existing GPL-3.0 workspace host dependency; its MIT/Apache-2.0 CLAP bindings, ISC loader and existing compatible dependencies introduce no new external licensing requirement here.

## 2026-10-11 hosting source checkpoint

See [the worker hand-off](../qa/2026-10-11-plugin-hosting.md) for ABI 7 channel/instance
replay, negotiated expression capabilities, real helper editor lifecycle and
guarded native dirty capture, DSP wrapper policies, and the unverified macOS
AUv2 adapter. Earlier ABI/editor-limit notes are historical where superseded.
All new regressions are written, not executed; platform/corpus/packaging gates
remain explicit and no whole hosting row is newly verified.

Current source additions also include saved wrapper DSP/owner/editor controls
with undo/reopen tests, the numeric VST3 expression SDK-list correction and
native fixture/helper replay cases, and a separate CLAP engine export target.
[Wrapper settings](wrapper-options.md) documents supported behavior and explicit
threaded refusal. VST2 is excluded by the owner decision; AU and export remain
partial. These additions are written, not executed and do not extend historical
verification claims above.
