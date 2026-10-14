# Round eight: languages — 2026-10-14

Row: `wf-languages` ("Shows the interface in a chosen language", English and
Spanish). Main checkout, branch `feat/parity-100`, started at `0f8a682c`
(Rust 4,855 / 0 / 40, frontend 6,509 / 0). One build or test process at a
time through `target/round8/run.sh` (MSVC environment, `CARGO_BUILD_JOBS=8`,
an isolated `TS_RS_EXPORT_DIR`, the native fixture variables) and
`target/round8/front.sh` (Node and pnpm on PATH). Logs are
`target/round8-<step>.log`. No sub-agents, no audio device, no ignored
hardware test, no new dependency, no branch, push, pull request or tag.
`docs/parity/parity.json` and `PARITY.md` are untouched.

## What was open

Round seven left text that Rust builds in English and a Spanish window then
shows in English: 116 desktop sentences pinned in
`apps/desktop/src-tauri/src/uncoded_messages.txt`, about 420 sentences and 24
hand-written `Display` impls in the core crates (not scanned at all), history
labels built from setting names in `windfall-project`, and playlist history
labels that appended an English "s" for plurals.

## Counts

Measured with the corrected scanner (below) on a clean export of `0f8a682c`
and on the tip. A "sentence" is a `format!`, `write!`, `writeln!` or
`#[error]` literal that is prose with values in it.

| | Desktop | Core crates | Total |
|---|---|---|---|
| Before (`0f8a682c`), corrected scan | 165 | 525 | 690 |
| Before, as the old guard counted | 116 listed | not scanned | 116 |
| After, built in English for the user (`[backlog]`) | 0 | 0 | 0 |
| After, recorded `[internal]` with a reason | 14 | 53 | 67 |
| After, recorded `[data]` with a reason | 3 | 3 | 6 |

Core before, by crate: project 195, flp 117, engine 42, midi 39, plugin-host
39, codec 26, plugin 22, dsp 12, analysis 11, ipc 9, archive 7, sim 4,
factory 2.

Fixed sentences (no values) that become errors were a second, larger hole:
2,121 literals in error positions (`Err(`, `ok_or(`, an error variant, a
refusal, a warning pushed for the user, a `reason:` field). At `0f8a682c`,
568 of them had a catalog entry (540 desktop, 28 core). All 2,121 have one
now, with Spanish.

Registry: 82 codes at `0f8a682c`, 860 now (append-only; the pins test holds
the first and last). 203 `ui_message` calls in the desktop and 658
`notice!` calls in the core crates name registered codes and give their
declared parameters. New plain catalog modules: `messages/backend-statics.ts`
(1,411 fixed sentences) and `messages/backend-terms.ts` (280 words the
backend sends as terms: setting and field names, device and stage names, FL
event names, units).

## Decisions

**The core crates carry a code, not English, in their `String` errors.**
`windfall_core::notice` holds a `Notice` (a code and named values) and the
`notice!` macro. A core error's `Display` is the envelope
`windfall-message:{"code":…,"params":{…}}` that the desktop already sent,
params sorted by name. The alternative, structured error types converted at
the IPC boundary, was measured: about 265 `.to_string()` conversions in the
desktop would each have had to be found and changed, and any one missed
leaks English. With the envelope as the carrier the existing conversions are
already right, and nothing in the core crates knows about the interface: the
registry and the declared parameters live in `windfall-ipc`, the words in the
interface's catalogs.

**Terms.** A value that is one of Windfall's own English words (a setting's
name, an envelope stage, a device, an FL Studio event name) is sent as
`notice::term(word)`, code `text.term`; the interface shows the catalog's
word when it has one and the word as it is otherwise. A term that is already
a coded message (a reason passed on from deeper down) stays that message.
Names somebody typed, file names and OS or library causes stay verbatim
values.

**English where English is right.** `windfall_core::english` (feature
`english`) renders an envelope in English from the interface's own catalog
modules, compiled in with `include_str!` (a test fails if a native catalog
module is not in its list). Tests use it to keep asserting readable English;
the exported plugin (`windfall-plugin`), whose panel has no interface
catalog, uses it to show English.

**History labels** for settings and scripts are `history.changeSetting` and
`history.changeScript` with the setting's name as a term, stored as code and
values; old projects' plain labels still show as they are.

**Playlist plurals** come from catalog plural entries (`{ one, other }`
through `Intl.PluralRules`) for Clone, Cut, Delete, Duplicate, Move, Paint,
Paste, Resize, Slice, Trim, Mute and Unmute clips; the appended-"s" helper is
gone.

## Guards

`apps/desktop/src-tauri/src/message_guard.rs` now scans the desktop and every
crate under `crates/`, test code cut out:

- `every_coded_message_names_a_registered_code_and_gives_its_parameters`:
  every `ui_message("code", json!({…}))` and every `notice!("code", …)`.
- `sentences_still_built_in_english_are_exactly_the_recorded_list`: the
  `format!` / `write!` / `writeln!` / `#[error]` prose is exactly the list in
  `uncoded_messages.txt`, each entry under a reason. Sections: `[internal]`
  (nobody is shown it), `[data]` (a default name the project keeps and the
  user can rename), `[backlog]` (empty).
- `fixed_sentences_made_into_errors_are_in_the_catalog` (new): every fixed
  prose literal in an error position has an English catalog key, so the
  interface can translate it. It found 42 the Python prototype of the scan had
  missed (`io::Error::other(…)` and a few others); all are catalogued.

The old guard undercounted: it cut a file at its first `#[cfg(test)] mod x;`
declaration, so everything after that line went unscanned, and it did not
look at `write!` or `#[error]`. That is why it listed 116 desktop sentences
where there were 165.

Exclusions, all with their reason in the file: file and folder names written
to disk, HTTP heads, the language-model prompt, the WAV region note, the
preview sample's name, macOS plist text, the soak CLI's report, the factory
loops JSON and its verify report, `windfall-midi`'s `Display` and report
English (the interface uses `windfall_ipc::midi_*` codes instead), the
plugin-check tool's report, JSON-reader `expecting` texts, the lock file, an
OS shared-object name, the backpressure ready file. `[data]`: the default
sample names "{} (preset source)", "{} ({} BPM, key {} minor)", "Recording
take {}", the automation names "{} track volume" / "{} track pan", and
"{name} sample effects".

## Tests added

- Rust, `session/tests/message_families.rs`: one error of each family, made
  through the call that makes it, decodes to its code and values — a project
  file breaking a rule (`project.load.damaged` around `project.check.tempo`
  600 / 10 / 522), a refused edit (`project.edit.fieldTooLong`, field as a
  term, max 128), an FL import review line (`flp.project.tempo`) and an
  unreadable FL file (`flp.error.tooShort`), an audio file that will not
  decode (`codec.noAudio`, `codec.corrupt` with a term), a desktop refusal
  (`templates.factoryName`).
- Rust, `notice::tests`: a term is a word unless it is a coded message
  already.
- UI, `native-messages.test.ts`: the same families rendered in Spanish
  (project validation, edit, not found, FLP lines, plurals, omitted field,
  became-effect with notes, desktop refusal), embedded lists, capitalization.
- UI, `backend-terms.test.ts`: every term the Rust sources send (check kinds,
  not-found kinds, clamped subjects, fields, envelope stages, preset labels,
  audio edit labels, FL notes and event names, voice targets) has Spanish.
- UI, `history-labels.test.ts`: the coded setting and script labels and the
  plural playlist labels in Spanish; `lower.rs` builds no `Change …` label in
  English.
- Window, `scripts/language-switch-window-smoke.mjs`: two more checks in the
  real window while it is Spanish — opening a project file whose tempo breaks
  the format's rule shows the refusal in Spanish, and the FL Studio import
  review of a file with a 600 bpm tempo shows its line in Spanish. Both files
  are chosen in the operating system's own file dialog: the smoke finds the
  dialog by its Spanish title (the native dialog class only), puts the path
  into its file name box and sends it OK, with the dialog's own window
  messages. The refusal is checked word for word: said once, from the code.
- UI, `native-messages.test.ts` also: an id is written as it is and a count
  grouped; a message sent as a sentence ends with one full stop.
  `mock.test.ts`: a shell refusal is put into words once, from what the
  shell sent.

## Running record

- `g1`, `g2`: the three guard tests. The first run of the new static-catalog
  guard listed 42 uncatalogued sentences (see Guards); after adding them,
  3 / 0 / 0.
- `w2`: full workspace after the conversions, 4,798 / 62 / 40, 1,148 s. Of
  the 62, 60 asserted English words of a message that is now a code; those
  assertions now read the message through `windfall_core::english`, which
  says it the way the English interface does, so each still checks the same
  words. The other two were product findings:
  - a reason that was already a coded message was wrapped as a term
    (`text.term` around an envelope) by `CodecError` and a few others;
    `notice::term` now passes a coded message through, with a test;
  - `engine.preparation.native` sent the plugin's target as Rust debug text
    (`Effect { effect: EffectId(14) }`). It now sends
    `project.plugin.instrument` / `.effect` / `.rack` with the id (three new
    codes).
  One English expectation changed: an FL import line about one clip now
  reads "1 clip of the playlist sits …" where it read "1 clips … sit"; the
  line is a plural entry now. Likewise a MIDI score note "1 note starts
  past …" / "1 note was cut …" (were "1 notes …").
- `r1` to `r3`: the failed tests again, with the later assertions of the
  same tests (a test stops at its first failure). Two more product
  findings: an export refusal had lost its full stop, because the desktop's
  `sentence()` now passes a coded message through; it sends it inside the
  new `text.sentence`, which the interface and `english` end with exactly
  one full stop. And a SoundFont import refused for memory put the Rust
  debug form of its reason into the message (`MemoryBudget("windfall-message:…")`),
  which nothing could put into words; the reason is now passed on as the
  coded message it is. `r3`: 1 / 0 / 0.
- Checkpoint commits `cf10190a`, `4f16d4a5`.
- `b1`, `b2`: bindings regenerated, 627 files, all current. `s1`, `s2`:
  simulator rebuilt, current.
- `f1` to `f3`: the i18n frontend tests. Findings, all fixed in the
  product: `localizeBackendText` read a backend list that starts with a
  coded message as one broken message ("No se ha podido interpretar el
  mensaje nativo"); it now puts such a list into words line by line. One
  Spanish entry named `{max}` twice
  (`project.timeline.tooManyItems`). The settings dialog showed the audio
  error unlocalized and built from fragments. The whole-app guard read the
  catalog key `title` (the FL event 194, sent as a term) as a display
  property; the event is now named "project title", which says more to the
  user, rather than the guard being given an exception. Terms Spanish
  shares with English (`clip`, `swing`, `tempo`, `color`, `audio`,
  `ping-pong`, `pogo`, `plugin`) and the parameter-only codes were added to
  `sameAsEnglish` with their reasons. `f3`: 12 files, 48 / 0.
- Checkpoint commit `3afc3b1f`.
- `f4`: the whole frontend suite, 537 files, 6,496 / 21. The failures read
  stored history labels and backend refusals as raw text, which are now
  coded (the playlist's labels carry their count; settings and scripts are
  `history.changeSetting` / `history.changeScript`; refusals reach a toast
  as an element that says them in the language shown). Those tests now read
  them as the interface shows them (`historyLabelText`, `nativeMessage`,
  `errorMessage`, and a new `toastWords` in the test harness). Two product
  findings: an id inside a coded message was grouped like a count
  ("channel 12,345", "12.345" in Spanish); the values that number a thing
  (`id`, `handle`, `channel`, `clip`, `effect`, `output`, `pattern`,
  `track`) are now written as they are, with a test. And the mock backend
  capitalised a coded message ("Windfall-message:…"), which then could
  not be read; it leaves one as it is.
- `f5`, `f6`: the failed files again, 21 files, 268 / 0; lint and
  typecheck clean. Checkpoint commit `533c84b7`; simulator rebuilt after
  the FL event rename (`s3`, `s4`).
- `f7`: whole frontend suite, 537 files, 6,518 / 0 / 0, 454 s.
- `c1` to `c4`: strict lint. Findings were `term(x.to_string())` where `x`
  already was text, needless borrows and a stray doc comment; clean at
  `c4`. Checkpoint `dff9c7f0`.
- `t1`, `w1`: Tauri debug build and the language window. The new checks
  took several tries to reach the real file dialog. Tauri's `invoke` and the
  object holding it cannot be replaced from the page, and the DevTools
  network domain does not see its calls. Keystrokes did not reach the dialog
  from a background process, and UI Automation shows its controls only as
  panes. The dialog's own window messages work (`WM_SETTEXT` on the file
  name box, `WM_COMMAND IDOK`). The window then showed a product fault no
  unit test had: a refusal from the shell read
  "Diagnóstico nativo: El archivo del proyecto está dañado: …". The
  desktop's IPC wrapper puts a refusal into words when the call fails and
  keeps what the shell sent as the error's cause; the toast put those words
  into words again, and a nested message is not recognised as already said.
  `errorSource` now takes a coded message from the cause, so it is said
  once, when shown (test in `mock.test.ts`). `t2`, `w1` again: 16 checks,
  all pass, the refusal word for word. Checkpoint `8eba2772`.
- The same window showed the status bar's "Memory sink (no audio device)"
  in English: Windfall's own device names are now put through the catalog
  in the status bar, the master strip, the settings dialog and the external
  output field (names of real devices are not catalog keys and stay as
  they are). `f9`: 72 files, 625 / 0; lint and typecheck clean. Commit
  `1d69b88b`.

## Gates

All ran one at a time through `target/round8/gates.sh` on the source of
`1d69b88b`, without affinity or priority settings. Rust counts are passed /
failed / ignored. Logs are `target/round8-gate-<step>.log`.

| Gate | Command | Result |
|---|---|---|
| Fixture and native binaries | `cargo build --offline --manifest-path crates/windfall-plugin-host/test-plugins/Cargo.toml --target-dir target`; `cargo build --offline -p windfall-desktop -p windfall-plugin -p windfall-plugin-host --bins --lib` | built |
| Format | `cargo fmt --all --check` | pass |
| Strict lint | `cargo clippy --workspace --all-targets -- -D warnings` | pass, no diagnostics |
| Rust workspace, gate run | `cargo test --workspace --no-fail-fast -- --test-threads=1` | 4,860 / 6 / 40 over 187 blocks, 988 s. The six were the native-helper editor tests (`n4_production_*` ×4, `*_owned_fixture_as_rack_node_*` ×2): the native step had just rebuilt the desktop helper, whose editor refusal is now `pluginHost.editor.noEditor`; they assert "no editor" through `english` now (`r4`: 6 / 0 / 0) |
| Rust workspace, confirmation | same, after that change, with format and lint again (`gate2-fmt`, `gate2-clippy`) | 4,866 / 0 / 40 over 187 blocks, exit 0, 858 s (`round8-gate2-workspace.log`); format and lint clean |
| Bindings | `bash scripts/gen-bindings.sh target/round8/final-bindings`; `node scripts/check-bindings.mjs target/round8/final-bindings`; `node --test scripts/check-bindings.test.mjs` | 627 files; all current; tests pass |
| Simulator | `bash scripts/build-sim.sh`; `node scripts/check-sim.mjs` | 7,545,501 bytes; current (rebuilt after the lint fixes in `windfall-project` / `windfall-flp`, committed with `90cc7e58`) |
| Frontend typecheck, lint | `pnpm --dir apps/desktop typecheck`; `… lint` | pass; pass |
| Frontend suite | `pnpm --dir apps/desktop test --maxWorkers=4 --reporter=default --reporter=json --outputFile=../../target/round8-gate-frontend-results.json` | 537 files, 6,518 passed, 0 failed, 0 skipped, 511 s |
| Production frontend | `pnpm --dir apps/desktop build` | pass |
| Parity consistency | `node scripts/parity.mjs --check` | 342 rows: 334 done, 3 in progress, 5 won't do; tracker unchanged |
| Release and resource scripts | `node --test scripts/release/release.test.mjs scripts/release/workflow.test.mjs` | 69 / 0, 121 s |
| Apple host | `cargo check --offline --target aarch64-apple-darwin -p windfall-plugin-host --all-targets` | pass, no diagnostics |
| Apple desktop and export | `bash target/round5/macos-check.sh` | pass, no diagnostics |
| Linux host and common | `bash target/round5/linux-check.sh -p windfall-plugin-host -p windfall-core -p windfall-dsp -p windfall-project -p windfall-ipc -p windfall-stretch` | pass, no diagnostics |
| Linux broad integration | `bash target/round5/linux-desktop-check-only.sh` | pass, no diagnostics |
| Tauri debug build | `pnpm --dir apps/desktop tauri build --debug --no-bundle` | built |
| Analyzer window | `node scripts/analyzer-window-smoke.mjs target/debug/windfall-desktop.exe target/round8-analyzer-window/report.json 9271` | 18 checks, all pass, software-memory-sink |
| Sample window | `node scripts/sample-workflow-window-smoke.mjs … target/round8-sample-window/report.json 9272` | 65 checks, all pass |
| Notebook window | `node scripts/notebook-isolation-window-smoke.mjs … target/round8-notebook-window/report.json 9273` | 35 checks, all pass |
| Video window | `node scripts/video-player-window-smoke.mjs … target/round8-video-window/report.json 9274` | 35 checks, all pass |
| Language window | `node scripts/language-switch-window-smoke.mjs … target/round8-language-window/report.json 9275` | 16 checks (two new), all pass |
| Factory content | `cargo run --offline -p windfall-factory -- verify content/factory` | content matches the generator |

Not run: the release CPU budgets (E1, P2, instruments, threading). No audio
processing changed: the DSP and engine edits are to how errors are worded
(`Display` impls and refusals), and the audio callback paths are untouched.
No real audio device, MIDI device or ignored hardware test was used; every
window report names the software memory sink. The cross-target checks type
check the changed crates for macOS and Linux; nothing was run there.

## Still open

- Some internal error kinds still reach a message as their Rust name inside
  a `detail` value, around words that are translated: the SoundFont and
  zone-table import errors (`InvalidRiff`, `MissingChunk(…)`), speech
  rendering, the analyzer and capture refusals, project preparation's
  "refused" fallback, the bridge protocol (`Configuration`). These are
  diagnostics for a bug report rather than sentences; giving each variant a
  code is a follow-up. The memory refusal and the plugin target, which a
  user meets, were fixed this round.
- What the operating system or a library says (I/O errors, decoder and
  serde reasons, a plugin's own text) stays verbatim inside a translated
  sentence, by design.
- The exported plugin (`windfall-plugin`) has no catalog of its own; its
  panel shows English, rendered from the same catalog through
  `windfall_core::english`.
- History labels saved by older versions as plain English show through the
  old path (`Change {name}` with the name looked up); new ones are coded.
- The Spanish was written here and checked against the catalog's existing
  terms; it has had no review by a native speaker.

## Where this stops

Commits on `feat/parity-100` after `1ddb997a` (a docs-only commit on top of `0f8a682c`, already there when this round began): `cf10190a`, `4f16d4a5`,
`3afc3b1f`, `533c84b7`, `fcbc0254`, `dff9c7f0`, `8eba2772`,
`1d69b88b`, `90cc7e58` and the closing commit that holds this record.
Nothing is pushed. No build or test process is left running. The branch
`opus/r8-rows` (hand-off `docs/qa/2026-10-14-r8-rows.md`) was not touched
and is not merged.

| Row | Evidence run here | Still open | Recommendation |
|---|---|---|---|
| `wf-languages` — shows the interface in a chosen language (English, Spanish) | No text the user is shown is built in English in Rust any more: 0 of the 690 sentences counted at `0f8a682c` (165 desktop, 525 core) are left in the backlog; 73 remain, each recorded with its reason (67 nobody is shown, 6 default names the user renames). All 2,121 fixed sentences made into errors have Spanish, held by a new guard test. 860 registered codes; every `ui_message` and `notice!` names a registered code and gives its parameters; every language has every code. Rust tests decode one error of each family to its code and values; UI tests render each family in Spanish; history labels from setting names are coded; playlist plurals come from plural entries. In the real window while Spanish: a menu, a dialog, an instrument editor, the history, a desktop refusal, a project file refused for its tempo (word for word, said once) and an FL import review line, both files chosen in the OS file dialog. Full suites, cross-target checks and five window smokes in the gate table. | EXTERNAL-ONLY: review of the Spanish by a native speaker. SCOPE: internal error kinds named in a diagnostic `detail` (list above); OS and library causes verbatim by design; the exported plugin's panel in English. | `done` |
