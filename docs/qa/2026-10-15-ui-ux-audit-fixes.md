# UI/UX audit fixes — 2026-10-15

Response to `docs/qa/ui-ux-audit-2026-10-15.txt` (30 observations from operating the
installed 1.0.2 app by hand). The audit listed problems only; the solutions below were
chosen during implementation. Classification: **bug** = a cause was found in code;
**friction** = a design change to a reported difficulty; **unverified** = no cause
found without runtime evidence.

Verification is unit/integration level: `tsc -b`, ESLint on changed files, the full
desktop vitest suite, `cargo test -p windfall-engine --lib device`, and clippy on
windfall-engine. None of these fixes has been rechecked in the installed app, and
nothing was retested with the audit's window-level method.

| # | Class | Change |
|---|---|---|
| 01 | bug | Notebook drafts were kept only in the panel until a hidden "Save notebook" button was pressed. Now an edit is written into the project after 1 s idle and when the notebook closes, so the project is marked unsaved. Ctrl+S inside the notebook commits the draft, then saves. The save button and status stay pinned at the bottom. |
| 02 | bug | After an edit replaced the clip, the playlist dropped the old clip from the selection, and the editor treated that as the user leaving. The editor now follows the derived clip. The analysis and script tools follow their results too. |
| 03 | bug | Browser > Project > Edit history showed the stored label (`windfall-label:{…}`). It now renders through `historyLabelText`. |
| 04 | bug | "analysis:unavailable: windfall-message:…" nested payloads are now decoded into a localized sentence in Analysis, analysis commands' disabled hints, and Settings > Stem separation. |
| 05 | friction | The piano-roll toolbar wraps onto more rows instead of scrolling sideways. |
| 06 | friction | The piano roll starts on Step snap (the playlist keeps Bar). Choosing a snap in either editor still updates both. |
| 07 | bug + friction | A resize snapped the drag distance, not the edge, which gave 1¼-beat notes. The edge now lands on a grid line. A pattern the editor grew shrinks back when notes are pulled in again, but never below its length before growing, and never when the length was set by hand. While dragging, the readout shows the new length in beats. |
| 08, 30 (clips) | friction | The audio clip strip's rows of preset/Previous/Next/Half/Double buttons became one menu per setting. |
| 09 | bug | When Undo removes the strip's clips, the empty strip hides, and the timeline scrolls back to existing tracks. |
| 10 | bug | Double-clicking an audio clip reopens its settings. They are also in View > Playlist and the playlist context menu. |
| 11 | friction | Right-click delete stays the default (FL-style, also used by the piano roll). A saved "Right-click deletes clips" option turns it off, so right-click opens the clip menu instead. |
| 12 | friction | Mixer track metadata (sidechain, presets, EQ, I/O, latency) folds into a "Track settings" section, closed by default. A newly added effect scrolls into view. |
| 13 | friction | The +Effect and Add channel menus are grouped by family, with one-line descriptions, and Add channel gains "Browse plugins…". There is no text search inside these menus (see open items). |
| 14 | bug | Adaptive mixer strips were fixed at 80 px. Strips in the main area now share the available width (80–128 px). |
| 15 | bug | The plugin manager showed the internal track id. It now shows the strip number and name. The channel badge, routing menu and strip labels also counted the Current utility track; they now all use one shared `useMixerTrackNumber`. |
| 16, 17 | friction | One search covers two labelled lists in the plugin manager. Category editing is behind "Edit category", and built-in cards show a description instead. |
| 18 | bug + friction | The tag form starts folded, and the tree scrolls the selected row back into view. |
| 19 | friction | Files Windfall can't use (licences, readmes, manifests) are hidden in the Factory root and in search. User folders still show them greyed. |
| 20 | friction | A factory loop selected in Library shows its source BPM and an "Add at {tempo} BPM" button that inserts the same way the Loops tab does. |
| 21 | bug | The missing-folder check compared plain text, but the backend sends a coded message with a backslash path. A new song now gets a "no backups yet" empty state, and long paths wrap. |
| 22 | bug (inferred) | While audio devices load, the Audio dropdowns are disabled, so focus fell on a lower field and the dialog scrolled there. The dialog now opens at the top, with a fixed title and a jump list of sections. |
| 23 | bug | Tempo fitting shows 2 decimals for BPM and 4 for the multiplier; the exact values are kept. Apply stays pinned in the footer. |
| 24 | friction | Notebook cue and recording start/loop-end positions are entered as bar:beat(:tick). The audio editor shows its range as time, and adds a waveform time ruler and a spectrum frequency axis. |
| 25 | friction | The recording dialog's status and Start/Stop sit in a fixed footer; only the setup scrolls. |
| 26 | bug | "Audio clip settings" lacked editor-related keywords. There is a new "Open audio editor" action, which also works when the settings strip is hidden. A search with no full match shows the closest actions. |
| 27 | bug | Song mode showed "Playing from the playlist" while stopped. The status now follows the transport ("Playing the song" / "Song stopped") and is not truncated. |
| 28 | bug (partial) + friction | One slow buffer could be counted twice: once when it finished late, and again by the coverage check. The second count now skips a gap that a dropout counted just before already explains. The count opens a popover with the time, engine load, buffer size, advice and Reset. |
| 29 | friction | Browser tabs renamed: Files → "Presets & templates", Backups → "Auto backups", Cloud backup → "Folder backup". Tab ids are unchanged. |
| 30 (channel) | friction | Channel/sample settings use a shared `value-steps` menu in place of 38 button rows. They add a section jump bar and foldable sections, and LFOs and envelopes fold while off. Loop knob labels no longer truncate, and every knob's full label shows on hover. |

## Open items / needs runtime evidence

- **28:** no code path was found that counts dropouts during stopped editing at low CPU. The audio stream keeps running while stopped, and the CPU figure is smoothed, so a single late callback still counts. Finding the real cause needs a device-level trace.
- **19:** the audit's `assets/GeneralUser-GS-LICENSE.txt` could not be found in the source tree or the installed factory folder. Such a file is hidden, but a folder holding only it would show as "(empty)".
- **13:** there is no inline text filter in the add menus. The menus' built-in type-to-jump is advertised instead, and the plugin manager has a full search.
- **24:** the audio editor shows time, not bar:beat, since that needs the clip's song position, tempo map and stretch. Analysis note/warp fields still use frames.
- **22:** the cause was inferred from code and was not reproduced.
- The audit's own coverage limits still apply. Not certified: microphone capture, external MIDI/plugins, ONNX analysis and stems, every codec, and every instrument/effect.
