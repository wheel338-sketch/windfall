# Round-seven languages hand-off — 2026-10-13

Worktree `.claude/worktrees/r7-languages`, branch `opus/r7-languages`, base
`91c7d1ed`. Row: `wf-languages`. Frontend only: no Rust was built or changed,
no push, PR or tag, no sub-agents.

## Result

Both strict guards pass, with the other i18n tests, `typecheck` and `lint`.
The full frontend suite (`pnpm --dir apps/desktop exec vitest run --maxWorkers=2`)
passed at `b069ff1c`: **534 files, 6,497 tests, 6,497 passed, 0 failed, 0
skipped**, in 944 s. Two earlier full runs are part of the record: after the
mass rewrite 6,487 of 6,490 passed (the two guards, and one test whose
source pattern had to follow the new title form); after the hand edits 6,495
of 6,497 passed, the two failures being hand edits of mine that had changed
an English accessible name, which I restored. The i18n folder alone is 18
files, 29 tests, all passing.

| Guard | Before | After |
| --- | --- | --- |
| `whole-app-coverage.test.ts` | 6,086 occurrences of 4,756 distinct strings in 495 files | 0 in 998 scanned files |
| `descriptor-coverage.test.ts` | 2,309 distinct labels missing (5,431 addresses) | 0 |

The catalogs went from 953 keys to 8,058 in each language. The new entries
live in 50 area modules under `apps/desktop/src/lib/i18n/messages/`
(`piano-roll.ts`, `channel-rack.ts`, `effects.ts`, …, three
`descriptors-*.ts`, `backend.ts`, and `shared.ts` for keys several areas use),
composed into `en`/`es` through `messages/index.ts`.

## Approach

A codemod rewrote each guard finding by its syntax context, using the
TypeScript compiler API and the repository's own guard as the finder. It was
run area by area with a typecheck after each batch. The script was a one-off
and is not committed. About 5,400 findings were rewritten mechanically. The
rest were resolved by about 150 hand edits, by the guard repairs below, or
by a justified invariant.

The patterns, all of which the guard accepts:

- **Shown while rendering or when something happens**: `t("…")`
  (`translatedText`) and `message("… {name}", { name })`. JSX text, display
  props, hints, refusals, toasts, `whyDisabled` and raised errors use these.
- **Tables built at module load**: the property becomes a getter
  (`get label() { return t("…") }`), so it is translated each time it is
  read. Where a list item is also a key or an index, the list keeps its
  English and the place that shows the item calls `t(item)`.
- **Action and context-menu titles**: `...dynamicTitle("…", {})`. The
  registry keeps the English title, so the palette still finds a command by
  its English name, and the kept message is translated when shown. A kept
  message can now mark values that are catalog keys themselves
  (`dynamicTitle("Add {value0}", { value0: descriptor.name }, { value0: "lower" })`),
  so a built-in instrument or effect name inside a title follows the language.
- **Templates**: one parameterised message with named placeholders. Templates
  that chose words inline became whole-sentence variants or plural entries
  (`{ one, other }`, chosen with `Intl.PluralRules`). Numbers inside a
  sentence go through the new `plainNumber`, which uses the locale's decimal
  mark without grouping or rounding, so English output is unchanged.
- **Reacting to a language change**: `LanguageBoundary`
  (`lib/i18n/boundary.tsx`) remounts what it wraps when the language changes.
  The shell wraps the chrome, panels and most dialogs in it. The command
  palette, the settings dialog, an open MIDI review and the toasts are left
  outside, because they already follow the language through hooks and should
  keep what they hold. Project, engine and open-dialog state live in stores
  and survive. What is lost on a change is local state inside the boundary: an
  open context menu closes and a half-typed field is cleared.
- **Raised errors** are translated where they are raised. So that such text
  is not then labelled as an opaque native diagnostic, `localizeBackendText`
  now recognises text that is already the current language's rendering of a
  catalog entry.

## Changes to the guard

I changed `source-guard.ts`, in both directions, each with a fixture test in
`whole-app-coverage.test.ts`. The scanned file set was not narrowed.

Narrower (false positives it no longer reports):

- A mapped list's item is bound to where the callback shows it. A list of ids
  that is only compared or used as a key (`(["previous", "next"] as const).map`)
  is no longer reported; an item rendered raw still is. On the base tree this
  alone removed 369 of the 6,086 occurrences.
- Text inside `<code>` is literal syntax. A string literal whose declared
  type is a closed union of string literals is a code. Arithmetic with `+` is
  not a concatenated phrase. The key of a lookup is not shown.

Wider (things it could not see before):

- Every value of a table read by key for display (`LABELS[kind]`,
  `Object.values(TABLE)`, `Object.entries(TABLE)`) is a caption. This found
  more than a hundred strings the base guard missed (analyzer status
  reasons, synth parameter descriptions, lane and tool hints, stage help),
  which are now translated.

## Invariant list

`invariants.ts` now has 81 literals (was 32), 8 keycap names (unchanged) and
11 patterns (was 6). The added literals are units and symbols (`dBFS`,
`LUFS`, `kbps`, `MiB`, `px`, `min`, `ppm`), acronyms (`DC`, `CC`, `FX`, `RAM`,
`CPU`, `SHA-256`, `M4A`, `AAC`), single letters (`L`, `R`, `M`, `S`, `W`,
`x`, `y`, `a`, `b`, `c`), abbreviations (`Op`, `Osc`), names (`FL Studio`,
`Nyquist`, `Moss`, the two language endonyms, six coined effect names, three
coined pack names) and three size tokens of the developer kit (`sm`, `md`,
`lg`). The added patterns are triplet note values, Roman-numeral degrees, CSS
custom-property references, URLs and BCP 47 locale tags. One rule was added:
text made only of invariant terms joined by notation (`· kHz`, `M4A (AAC)`,
`C♯ / D♭`) is invariant.

Of the 273 entries the translators proposed as invariant, about a quarter
were accepted. The rest were technical ids the repaired guard no longer reports,
were restructured, or were translated (`Shift` in the note tools, the
keyboard key `Shift` → `Mayús`, field ids shown as labels).

`sameAsEnglish` has 143 entries (was 14), each with a reason: 19
punctuation-only templates, 23 templates or fragments around `ticks`,
`clips`, `cents`, 28 words Spanish spells the same, 52 terms Spanish-speaking
producers use unchanged, 6 keyboard key names and one command-syntax example.

## Translations

All eight translator chunks arrived complete. Cross-chunk conflicts on the
same English text (16) were settled against the shipped catalog: `Dry`/`Wet`
→ `Original`/`Procesado`, `Rate` → `Frecuencia`, `Width` → `Anchura`,
`Glide` → `Deslizamiento`, `Pitch wheel` → `Rueda de pitch bend`, and so on.
A term scan across all the entries showed the core vocabulary is uniform
(`pista`, `muestra`, `preajuste`, `panorama`, `corte`, `bucle`, `compás`,
`lista de reproducción`, `mezclador`); the one drift fixed was `delay` →
`retardo` in four descriptor labels.

I translated these myself, because the translators never saw them:
about 470 parameterised messages the codemod and the hand edits created,
about 130 strings from tables the base guard could not see, chord and
interval names, and 415 backend messages. They have had no second reader.

## Tests

- `lib/i18n/runtime-switch.test.tsx` (new, 3 tests) switches to Spanish at
  run time with a context menu, the settings dialog, a descriptor-made
  instrument editor, a kept failure, the command palette and a disabled
  reason mounted, and searches the palette by Spanish and by English name.
- `whole-app-coverage.test.ts` gained 4 fixture tests for the guard changes.
- `shell-coverage.test.ts`: its pattern for built-in action titles now
  matches `...dynamicTitle("…", {})`; its assertions are unchanged.
- No other existing test was edited.

## Backend messages

Inventory of `apps/desktop/src-tauri/src` (read-only, by pattern): 459
distinct static messages at 515 sites, and 136 parameterised sites in 42
files.

The 459 static ones are covered now: `messages/backend.ts` has an entry
keyed by each English text (first letter capitalised, as `errorMessage`
delivers it), and `localizeBackendText` already translates text that is a
catalog key. 44 were in the catalogs already; 415 are new.

The parameterised ones need a structured code and parameters from Rust,
through the existing `windfall-message:` envelope in
`crates/windfall-ipc/src/ui_messages.rs`. Until then they reach a Spanish UI
as `Diagnóstico nativo: <English>`. The list is a pattern match, so treat it
as a superset to triage: a few entries are not user-facing (the assistant
prompt at `assistant/provider.rs:48`). Paths are under
`apps/desktop/src-tauri/src/`.

- `assistant/provider.rs:48` — `Return only one JSON object conforming to this contract. Do not execute actions or invent IDs. Commands will be validated and reviewed by the user. Never echo credentials. {contract}`
- `browser.rs:47` — `The folder "{shown}" does not exist.`
- `browser.rs:49` — `Windfall is not allowed to read the folder "{shown}".`
- `browser.rs:53` — `Could not read the folder "{shown}": {error}`
- `commands.rs:663` — `The task stopped unexpectedly: {error}`
- `commands.rs:1861` — `Could not save sheet music: {error}`
- `commands.rs:1988` — `Unknown panel: {id}`
- `files_commands.rs:29` — `The task stopped unexpectedly: {error}`
- `library.rs:104` — `Could not read library metadata ({e}). Close Windfall and repair or rename {} before saving favorites or tags. The original file has been preserved.`
- `library.rs:137` — `Could not save favorites and tags to {}: {e}. Check free space and write access, then try again.`
- `library.rs:215` — `Could not start the library worker: {e}. Restart Windfall to try again.`
- `library.rs:520` — `Could not read {}: {e}. Check that the file and folder are still available, then refresh the library.`
- `library.rs:533` — `Could not read {}: {e}. Check file access, then refresh the library.`
- `library.rs:583` — `Could not index {}: {error}. Check folder access or reconnect the drive, then refresh; you can remove an unavailable folder from the browser.`
- `paths.rs:32` — `"{path}" is not a full path`
- `plugins/bridge.rs:508` — `Isolated plugin render failed: {error:?}`
- `plugins/note_capabilities.rs:90` — `The requested note expression is unavailable on MIDI channel {} (Pan: {}, Volume: {}, note instances: {}).`
- `plugins/platform_editors.rs:45` — `The helper's plugin editor refused application scale {application} at monitor scale {monitor}.`
- `plugins/runtime.rs:699` — `Process bridge recovery is pending; retry manually: {}`
- `plugins/runtime.rs:797` — `The plugin refused content scaling at application scale {application} and monitor scale {monitor}.`
- `plugins/runtime.rs:1856` — `The editor opened, but its content could not be scaled: {error}`
- `plugins/runtime.rs:2131` — `Native helper update shutdown failed: {}`
- `plugin_database.rs:58` — `{error}. Favorites and categories are not being saved. Repair or rename "{}" and restart to save them again.`
- `plugin_database.rs:130` — `Could not save the plugin database: {error}`
- `plugin_database.rs:139` — `The plugin database could not be opened: {error}`
- `plugin_database.rs:145` — `The plugin database could not be read: {error}`
- `samples.rs:72` — `"{}" is a folder, not an audio file`
- `samples.rs:87` — `Could not load "{}": {error}`
- `session/analysis_jobs/source_namespace/drive.rs:81` — `Cannot retain the effective drive object ({status:#x}).`
- `session/audio_logger.rs:181` — `The audio logger keeps {AUDIO_LOGGER_MIN_SECONDS} to {AUDIO_LOGGER_MAX_SECONDS} seconds.`
- `session/audio_logger.rs:186` — `The audio logger follows at most {AUDIO_LOGGER_MAX_TRACKS} mixer tracks, each once.`
- `session/audio_logger.rs:701` — `Could not create the recordings folder: {error}`
- `session/audio_logger.rs:721` — `Could not create the logged audio file: {error}`
- `session/consolidate.rs:70` — `"{}" has no unmuted pattern or audio clip to consolidate.`
- `session/controllers.rs:771` — `Controller movement applied; MIDI feedback failed: {error}`
- `session/controllers.rs:810` — `The note log could not be saved: {error}`
- `session/controllers.rs:1583` — `At most {MAX_LEARN_TARGETS} controls can be linked in one pass.`
- `session/controllers.rs:1602` — `There can be at most {MAX_CONTROLLER_LINKS} controller links.`
- `session/derived_assets.rs:69` — `Could not collect unused derived audio {}: {error}`
- `session/export/mastering.rs:35` — `the mix could not be set aside for mastering ({error})`
- `session/export/mastering.rs:61` — `the mix could not be set aside for mastering ({error})`
- `session/export/mastering.rs:71` — `the mix set aside for mastering could not be read ({error})`
- `session/export.rs:96` — `"{}": the destination is not a file`
- `session/export.rs:314` — `{} Hz is not a sample rate Windfall can export.`
- `session/export.rs:371` — `The export would be {:.0} minutes long. The longest {} file Windfall can write at {} Hz is {:.0} minutes.`
- `session/export.rs:452` — `Could not start the export: {error}`
- `session/extractions.rs:50` — `Could not record extracted audio ownership: {e}`
- `session/extractions.rs:157` — `Kept extracted audio {} because a saved reference could not be checked: {error}`
- `session/extractions.rs:195` — `Recovered an unsaved template/archive project at {}. Open it from Recent projects to recover its audio.`
- `session/extractions.rs:197` — `Could not recover extracted project {}: {error}`
- `session/extractions.rs:205` — `Could not reclaim abandoned extraction {}: {error}`
- `session/extractions.rs:229` — `Could not checkpoint the unsaved extracted project: {error}. Its initial project and audio are retained.`
- `session/extractions.rs:314` — `Could not preserve preset capture audio: {e}`
- `session/extractions.rs:448` — `Could not remove unused extracted audio {}: {error}`
- `session/files.rs:80` — `{what}: sampler preparation failed; see the project warning.`
- `session/files.rs:83` — `{what}: native plugin preparation failed; see the project warning.`
- `session/files.rs:86` — `{what}: project preparation failed; see the project warning.`
- `session/files.rs:88` — `{what}: engine readiness changed before publication. Try again; see the project warning.`
- `session/flp.rs:37` — `Could not read the FL project: {e}`
- `session/flp.rs:40` — `Could not read the FL project: {e}`
- `session/flp.rs:57` — `The converted project is invalid: {e}`
- `session/flp.rs:103` — `Sample {} has {} cues; only the MIDI-key range 60..127 fits the stand-in.`
- `session/flp.rs:128` — `Slicex sample {} cue metadata: {error}; embedded cue fallback retained.`
- `session/flp.rs:281` — `Could not search {}: {e}`
- `session/flp.rs:329` — `More than one file matches {basename}; choose a more specific search folder.`
- `session/flp.rs:363` — `Sample {} requests loop points, but only RIFF/WAVE loop metadata is supported.`
- `session/flp.rs:372` — `Sample {} loop metadata was not applied: {error}`
- `session/flp.rs:380` — `Sample {} requests loop points but its WAV has none.`
- `session/flp.rs:390` — `Sample {} loop bounds or backward mode cannot be applied.`
- `session/flp.rs:414` — `Sample {} loop lies outside its trimmed playback region.`
- `session/flp.rs:428` — `Sample {} loop points came from its WAV smpl chunk; loop release/seam behavior can differ.`
- `session/flp.rs:429` — `Sample {} loop metadata was not applied: {error}`
- `session/flp.rs:432` — `Sample {}: only the first loop and indefinite repetition are represented.`
- `session/input_monitor.rs:76` — `Choose an input for monitored track {}.`
- `session/library.rs:114` — `"{shown}" is already in the browser.`
- `session/library.rs:132` — `"{path}" is not in the browser.`
- `session/library.rs:175` — `sample {} does not exist`
- `session/mixer_presets.rs:97` — `Could not open the mixer preset: {error}`
- `session/mixer_presets.rs:101` — `Could not read the mixer preset: {error}`
- `session/mixer_presets.rs:106` — `Invalid mixer preset: {error}`
- `session/mixer_presets.rs:169` — `Could not open the mixer preset: {error}`
- `session/mixer_presets.rs:195` — `Could not open the mixer preset: {error}`
- `session/mixer_presets.rs:197` — `Invalid mixer preset: {error}`
- `session/music_lighting.rs:79` — `Could not initialise Chroma HTTP client: {e}`
- `session/music_lighting.rs:101` — `Chroma REST service unavailable: {e}. Enable the local Chroma SDK service.`
- `session/music_lighting.rs:106` — `Chroma REST service returned HTTP {}`
- `session/music_lighting.rs:115` — `Chroma response failed: {e}`
- `session/music_lighting.rs:128` — `Chroma rejected the request: {}`
- `session/music_lighting.rs:379` — `Could not start Chroma worker: {error}`
- `session/plugin_export.rs:64` — `Original preset source '{}' cannot be resolved`
- `session/plugin_export.rs:86` — `Raw SF2 source '{}' cannot be resolved`
- `session/plugin_export.rs:88` — `Raw SF2 source '{}': {e}`
- `session/plugin_export.rs:112` — `Sample '{}' is not decoded; plugin export requires every PCM source`
- `session/preparation.rs:77` — `Cannot decode video audio '{}': cannot open video: {error}. Mute it or restore the file.`
- `session/preparation.rs:86` — `Cannot decode video audio '{}': {error}. Mute it or choose AVI PCM / Ogg Vorbis.`
- `session/presets.rs:176` — `Could not save the preset: {error}`
- `session/presets.rs:265` — `"{}" is a {} preset, not a channel preset.`
- `session/presets.rs:289` — `The preset's audio "{}" is not beside it. Put it back, or load the preset without its audio.`
- `session/presets.rs:328` — `Could not read the preset library: {error}`
- `session/presets.rs:387` — `Could not delete the preset: {error}`
- `session/presets.rs:516` — `"{name}" cannot be used as a file name.`
- `session/presets.rs:523` — `Could not open the preset: {error}`
- `session/presets.rs:528` — `Could not read the preset: {error}`
- `session/presets.rs:540` — `The preset's audio "{}" could not be copied: {error}`
- `session/presets.rs:547` — `The preset's audio "{}" is not a file.`
- `session/presets.rs:553` — `The preset's audio "{}" is larger than a preset can bring along.`
- `session/preset_assets.rs:29` — `Preset source exceeds {limit} bytes.`
- `session/recording_multitrack.rs:50` — `Choose a hardware input for armed track {}.`
- `session/recording_multitrack.rs:60` — `Combined recording offset for {} exceeds 1000 ms.`
- `session/sampler_preprocess.rs:49` — `{what} must be zero or more.`
- `session/sampler_preprocess.rs:155` — `{name} (prepared preview)`
- `session/samples.rs:41` — `{} imported plugin states are retained; unsupported instruments stay silent and effects bypassed. Use the source FL project with a compatible host to recover those sounds.`
- `session/samples.rs:99` — `Missing sample: {}`
- `session/sample_bank_files.rs:110` — `invalid sample-bank manifest: {e}`
- `session/sample_instruments.rs:54` — `Could not open SoundFont: {e}`
- `session/sample_instruments.rs:60` — `SoundFont file exceeds the {limit}-byte read limit`
- `session/sample_instruments.rs:70` — `Could not read SoundFont: {e}`
- `session/scores.rs:59` — `Could not save the score: {error}`
- `session/scores.rs:72` — `Could not open the score: {error}`
- `session/scores.rs:77` — `Could not read the score: {error}`
- `session/templates.rs:153` — `Could not read the templates folder: {error}`
- `session/templates.rs:195` — `"{stem}" is a template that comes with Windfall. Pick another name.`
- `session/templates.rs:201` — `Could not make the templates folder: {error}`
- `session/templates.rs:208` — `A template called "{stem}" exists already. Delete it first, or pick another name.`
- `session/templates.rs:231` — `There is no factory template "{factory}".`
- `session/templates.rs:257` — `Could not delete the template: {error}`
- `session/templates.rs:269` — `"{name}" is not the name of a template.`
- `session/templates.rs:275` — `The template "{name}" no longer exists.`
- `session/timeline.rs:139` — `pattern {} does not exist`
- `session/transport.rs:217` — `pattern {} does not exist`
- `session/update_shutdown.rs:84` — `Update preparation could not capture current native state; nothing was installed. {error}`
- `session/update_shutdown.rs:292` — `Plugin editors could not resume after update cancellation: {}`
- `session/versions.rs:82` — `Could not save the new version: {}`
- `updates.rs:295` — `Update check failed; retry when online. {error}`
- `updates.rs:354` — `Download or signature verification failed. Nothing was installed. {error}`
- `updates.rs:518` — `Installation failed. Keep the current installation and retry. {error}{}`

Not covered by this inventory: messages whose text is built in the core
crates (`crates/*`) and reaches the UI through `Display`, multi-line
`format!` calls the pattern missed, and undo-history labels the backend
writes. The history panel shows backend labels as they arrive, in English.

## Not finished, and what I am unsure about

- **Rust codes** for the 136 parameterised sites above, and an inventory of
  the core crates. This is the remaining SOFTWARE item for the row.
- **Undo-history labels.** Batch labels the frontend writes are translated
  when the edit is made, so they stay in that language; labels the backend
  writes are English. A code-based history label would fix both.
- **Text kept in stores.** An error or status string a store keeps was
  translated when it was raised and does not change on a language switch.
  Text kept in component state inside the boundary is cleared by the remount.
- **Values fixed at registration.** Some action titles embed a translated
  word taken when the action was registered (piano-roll note colours, tools
  and lane kinds: `dynamicTitle("…{value0}", { value0: item.label.toLowerCase() })`).
  They are right at start-up and stale after a switch until restart. Built-in
  instrument and effect names in titles do not have this problem.
- **Sentences split around inline markup** (`Drag <b>this</b> here`) are
  translated as separate fragments. Word order may read oddly in Spanish.
- **Plurals.** Ten messages are real plural entries. Elsewhere the code keeps
  its two English branches (`"1 audio clip"` / `"{count} audio clips"`),
  which works for Spanish but would not for a language with more forms.
- **What the guard still cannot see**: text passed to a child component
  through a prop that is not a display name, and text built by pushing onto
  a list. I found and fixed the list-built cases by search (mixer delete
  confirmation, mastering summary, import warnings); there may be others.
- **The remount** was exercised in the mounted test and by the suite, not in
  the running desktop app. Panels are already mounted and unmounted by
  docking, so I expect it to hold, but nobody has watched it happen.
- **The invariant list** treats `a`, `s`, `x`, `min` and similar as
  invariant anywhere. A stray one-letter prose fragment would pass.
- EXTERNAL: review of the Spanish by a native-speaking producer, and text
  expansion in tight controls.

HAND-OFF COMPLETE
