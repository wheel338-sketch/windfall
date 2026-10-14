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
  review of a file with a 600 bpm tempo shows its line in Spanish. The OS file
  picker is the one thing not used: the smoke answers the app's request to
  open it through the DevTools network domain with the file's path.

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

## Gates

(Filled in by the final gate run.)
