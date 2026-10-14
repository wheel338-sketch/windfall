# Round eight acceptance: the last three rows — 2026-10-14

Rows: `wf-project-settings`, `fmt-flp-import`, `wf-languages`. Main
checkout, branch `feat/parity-100`, started at `8ccae116` (Rust 4,866 / 0 /
40, frontend 6,518 / 0). One build or test process at a time through
`target/round8i/run.sh` (MSVC environment, `CARGO_BUILD_JOBS=8`, an isolated
`TS_RS_EXPORT_DIR`, the native fixture variables) and
`target/round8i/front.sh` (Node and pnpm on PATH); the final gates through
`target/round8i/gates.sh`. Logs are `target/round8i-<step>.log` and
`target/round8i-gate-<step>.log`. No sub-agents, no real audio or MIDI
device, no ignored hardware test, no new dependency, no branch, push, pull
request or tag. `docs/parity/parity.json` and `PARITY.md` are untouched.

## The merge

`git merge --no-ff opus/r8-rows` (worker branch based on `1ddb997a`, hand-off
`docs/qa/2026-10-14-r8-rows.md`, built by nobody before this round), commit
`073ba694`. Two files conflicted, both because main had meanwhile turned the
FL import review's English lines into codes (round eight, languages):

- `crates/windfall-flp/src/convert/native.rs`, one hunk: the worker's
  SimSynth flag lines and its new omitted lines for fields outside the native
  table (SimSynth switches; 3x Osc offset, stereo detune, invert, phase
  randomness, osc-3 AM; Reeverb 2, Reeverb, Delay 2, Parametric EQ 2 fields)
  were English `format!` lines ending in the old English `OMITTED` suffix.
  They are now `flp.native.fieldOmitted` notices (device, field, value), the
  same code the native table's omitted fields use, so `omitted_field()` and
  `outcome()` (which read the code) file them as dropped and list them under
  "Omitted controls". The worker's tests that called the old two-argument
  `omitted_field(name, line)` or matched the English suffix
  (`devices_outside_the_table_report_exactly_their_recorded_omissions`,
  `synth.rs` `shapes_without_a_twin_play_as_a_saw_and_say_so`) read the
  field from the code now.
- `crates/windfall-flp/src/convert/voice.rs`, seven hunks: main's coded
  channel lines (`flp.voice.stretchFormant`, `arpUnsupported`, `echoFlag`,
  `echoPan`, `envelopeField`, `lfoField`) were kept, with the worker's
  bookkeeping added (`report.omit("Channel …", …)` per device and the
  de-duplicated envelope and LFO field lists). The worker had reworded those
  lines to say "omitted"; that wording went into the catalog instead
  (`native/flp.ts`, English and Spanish), and `flp.native.simsynthFlag` no
  longer claims "warmth, trackamp and chorus are reported per channel" (they
  are omitted lines of their own now).

Everything else merged cleanly. The terms guard
(`apps/desktop/src/lib/i18n/backend-terms.test.ts`) read the channel field
tuples as `(name, value)`; the worker made them `(name, field, value)`, so
the two patterns now also accept the extra field. That keeps the guard
reading the same words, it does not exclude any.

## What was fixed after the merge (commit `f844cf81`, `dac47698`)

Found by running the hand-off's tests and the guards. Product changes first.

1. **The Pitcher's scale lines were English sentences** (the worker's
   `key_on` mapping). `message_guard::sentences_still_built_in_english_are_exactly_the_recorded_list`
   listed both. They are `flp.native.pitcherScale` (device, keys, root,
   rootName, scale, scaleName with the scale's name as a term) and
   `flp.native.pitcherNoScale` (device, keys), registered at the end of the
   registry, English and Spanish in `native/flp.ts`, and the scale names
   `chromatic`, `major`, `minor`, `pentatonic` in the terms catalog.
2. **The FL notes the merge changed had no Spanish.** The terms guard listed
   22 missing entries: nine new effect notes and stand-in rules (Reeverb 2,
   Reeverb, Delay 2, Delay cutoff and own tempo, Stereo Shaper side,
   Panomatic target, BassDrum drop, Pitcher keys) and two device keys
   (`channel note echo`, `channel time stretch`). All have English and
   Spanish in `messages/backend-terms.ts`. Seven entries whose sentence the
   merge removed from the converter (checked with `git grep` over the whole
   tree) were deleted rather than left behind.
3. **Windfall's own channel devices under "Omitted controls" showed in
   English in a Spanish window.** "Channel envelope", "Channel LFO",
   "Channel arpeggiator", "Channel note echo" and "Channel time stretch" are
   now catalog words (`messages/flp-import.ts`, both languages) and the
   import review puts the device name through the catalog
   (`import-dialog.tsx`); a plugin's product name is not a catalog key and
   stays as it is. The mocked omitted-controls test now includes a channel
   device and reads the section again in Spanish
   ("Envolvente del canal: hold").
4. Clippy (`type_complexity`) on the worker's app-path restart test: the case
   tuple is a named type alias (`dac47698`).

Test expectations that were wrong, and why:

- Five `round8_controls.rs` tests (`bassdrum_peak_and_base_give_the_pitch_drop`,
  `pitch_shifter_wet_and_formant_correction_import_play_and_reopen`,
  `stereo_shaper_delay_holds_one_side_back_by_the_stated_rule`,
  `vintage_chorus_tempo_sync_keeps_the_fitted_rate_at_the_import_tempo`,
  `omitted_fields_outside_the_native_table_are_counted_and_listed_by_device`)
  searched the report's raw text for English words; the lines are codes on
  main. They read the review through `windfall_core::english` now (a
  `review()` helper), which is the English the interface shows, so they check
  the same words.
- `vocoder_band_count_is_carried_within_the_stand_in_range` asserted that the
  vocoder sounds with the synth carrier and the test tone on the main input,
  and it was silent. The worker's hand-off flagged this as an unverified
  assumption. The stand-in's main input is its *carrier* and its modulator
  is the sidechain key (`windfall-dsp/src/spectral/vocoder.rs`, "the main
  input is always the carrier"); with no key the modulator is silence. The
  test now also sets `keyIsCarrier`, which makes the main input the modulator
  while the synth supplies the carrier. The band-count assertions are as
  they were.
- The import review test on a real conversion matched `"omitted:"` in a raw
  line; it reads the line in English through `nativeMessage(text, "en")`.
- My own Spanish rendering test first expected the count at the start of
  "No se han importado 12.345 marcadores"; the catalog puts it in the
  middle, and the assertion now compares the whole sentence.

## Row `wf-project-settings`: what was checked against the audit

The audit's gap (`target/round8/audit-done-rows.json`): on the path the app
takes when a bridged plugin raises a restart, `capture_pending` →
`Record::readmit` → the helper's `Native::readmit` deactivated the plugin
before any fade, so no ramp was reported, the carry stayed empty and the
replacement started each note at its target.

What the merged code does (`apps/desktop/src-tauri/src/plugins/bridge.rs`):
`Record::readmit` sets `Departure::held`, asks the unit to leave (150 ms
limit, audio running) so the helper fades its output out over 5 ms and the
unit writes each sounding note's ramp into the carry, and only then sends
`Message::Readmit`; a failed re-admission withdraws the departure (`stay`).
`held` stops the two-second "replacement never came" expiry from withdrawing
it while the document prepares the replacement. `Departure::leave` returns
at once when the request already stands and the unit has faded, so the
departure in `Runtime::bridge_audio` (`runtime.rs`, the lines the audit
cited) reuses that carry. The bridge protocol is unchanged (version 11).

Evidence run here (`target/round8i-bridge2.log`,
`cargo test -p windfall-desktop --lib plugins:: -- --test-threads=1 --nocapture`,
106 passed / 0 failed / 4 ignored, the 4 being the long-standing
`capture_ack_tests` ignored in the baseline):

- `a_vst3_plugin_that_restarts_itself_mid_note_hands_the_ramp_over_on_the_apps_own_path`:
  fixture class 29 (appended), whose controller itself announces
  `kNoteExpressionChanged` while active; the helper does not follow it live
  and raises `RestartRequested`. Pan, volume and both, lost and returned,
  under the four pan laws: 24 runs. Each goes through the production owner's
  `Update::Capture`, `capture_pending` (which re-admits, departure first),
  `Command::ReadmitPlugin` on a real `Document`,
  `acknowledge_committed_capture`, `prepare_document().instrument()` (that
  is `bridge_audio`, with the old unit playing on), and the note replayed on
  the replacement. The test stands in only for the session worker and the
  audio thread. Each run asserts: the unit faded before the plugin was given
  back; every frame against the ramp model (worst error 2.98e-7); the click
  bound across the hand-over (largest step 0.00255 against a bound of
  0.00851); gains just before and after continue the ramp; one voice, no
  stuck note, silence after release; no allocation in `process`,
  `set_note_expression` or the replayed `note_on_instance`; and the live run
  equals the same hand-over run with offline helpers
  (`live_vs_offline=0.0e0` in all 24).
- `a_clap_plugin_that_restarts_itself_mid_note_hands_the_ramp_over_on_the_apps_own_path`:
  the new "Test Switching Sine", which calls `host->request_restart` from
  `process` when its note dialect changes while active; note instances lost
  and returned, four laws, 8 runs, same assertions (largest step 0.01966
  against 0.02817, worst error 2.09e-7, `live_vs_offline=0.0e0`).
- The earlier bridged migration tests, the in-process route tests (class 28
  same-unit switch) and the capture tests pass unchanged.
- `crates/windfall-plugin-host/tests/dynamic_capabilities.rs`
  `a_note_offer_the_plugin_changes_itself_asks_for_a_restart_and_waits_for_it`
  and `vst3_scan` with 30 classes: pass (`target/round8i-host1.log`).
- UI: `note-capabilities.test.tsx` checks the panel's sentence, which now
  says the channel-applied property "follows the most recent note that
  sounds; notes that overlap share it" (Spanish: "…; las notas que se
  solapan la comparten"), as the revised README entry puts it.
  `time-signature.test.tsx` edits the transport bar's popover (the audit's
  minor item). Both pass (`target/round8i-fe3.log`).

## Row `fmt-flp-import`: what was checked against the audit

- The seven fields the audit named are mapped, each with a fixture test in
  `crates/windfall-flp/tests/round8_controls.rs` (12 / 0,
  `target/round8i-flp3.log` and the full suite in `target/round8i-s2d.log`):
  Pitch Shifter `wet` → `mix`; Vintage Phaser `color_freq` → `colorHz` and
  `mod_tempo` → `sync`/`cycleBeats`; Vintage Chorus `mod_tempo_sync` →
  `sync`/`cycleBeats`; Fruity Chorus `wetonly` → `mix` 1; Stereo Shaper
  `delay` → left or right delay; Panomatic `lfo_target` → `depth`,
  `volumeDepth` or both. Each checks the imported value, the audible effect
  against a reference worked out in the test, and save and reopen with the
  original state. The check also exposed BassDrum `peak`, Vocoder
  `num_bands`, Pitcher `key_on`, Pitch Shifter `formant_correction` and five
  fields read by number in Reeverb 2, Reeverb, Delay 2 and Delay; all are
  mapped and tested.
- The pinned list (`native.rs`, module `open_list`, 3 / 0 in the 102 library
  tests): `decoded_fields_without_a_stand_in_control_are_exactly_the_recorded_list`
  pins every omitted field with a reason, native and outside the table
  (SimSynth switches, 3x Osc fields, channel envelope, LFO, arpeggiator, note
  echo and time stretch); `every_omitted_field_was_checked_against_the_controls_of_its_stand_in`
  works out, from each table device's stand-in descriptors, the controls no
  decoded field sets and requires them to equal a reviewed line; outside the
  table each reviewed control must exist on the stand-in, and the channel
  stand-ins are destructured field by field.
- The review: omitted fields are dropped lines (counted as omitted) and are
  listed per device under "Omitted controls";
  `reviews a project the importer really converted: its counts and its omitted controls`
  renders the dialog from a project the simulator's build of the real
  converter produced (SimSynth's five switches, the soft clipper's post
  gain). Passes, with the simulator rebuilt (`target/round8i-sim2.log`).
- `docs/flp/coverage.md` against the pinned list: the test's printed list
  and the coverage table were compared by script, 43 devices and 269
  families, no difference (`target/round8i-flp-open.log`). The section
  heading now says "built and tested".

## Step 2: the exported plugin's panel follows the chosen language

The panel (`crates/windfall-plugin/src/panel.rs`, with `panel/cocoa.rs` and
`panel/x11.rs`) is native Rust that draws Win32, AppKit or X11 controls, and
it had English literals plus `windfall_core::english` for coded messages. It
can reach the catalogs, because `windfall_core::english` already compiles the
interface's catalog modules in. What it could not reach was the language: the
choice lived only in the web view's `localStorage`. The change, kept small:

- **The app keeps the language where the plugin can read it.** The desktop's
  settings file gains `language` (`settings.rs`, `Option<String>`, serde
  default through the struct's `#[serde(default)]`; a value a newer version
  adds does not void the file). A new command `remember_language` takes `en`
  or `es` (`InterfaceLanguage`); the interface calls it at window start and
  on every change (`rememberLanguage` in `features/layout/use-app-boot.ts`,
  `Backend.rememberLanguage`; the mock does nothing).
- **The plugin reads it** (`crates/windfall-plugin/src/language.rs`): the
  desktop's config folder as Tauri names it (`%APPDATA%`,
  `~/Library/Application Support`, `$XDG_CONFIG_HOME` or `~/.config`, then
  `org.windfall.Windfall`; a debug build honours `WINDFALL_CONFIG_DIR` as the
  desktop does), `settings.json`, top-level `language`. Without it, the
  system language decides (Windows `GetUserDefaultUILanguage`, macOS
  `NSLocale preferredLanguages`, POSIX `LC_ALL`/`LC_MESSAGES`/`LANG`), and
  English when that is not one Windfall has. Neither CLAP nor VST3 tells a
  plugin its host's language, so the system's stands for it.
- **The words come from the shared catalogs.** `windfall_core::english`
  gains `Language` and `in_language(text, language)`: the Spanish object of
  each compiled-in catalog module, plus the terms and fixed-sentence modules
  (`messages/backend-terms.ts`, `messages/backend-statics.ts`), with the
  interface's rules for terms, causes, identifiers and Spanish numbers
  (decimal comma, at most three decimals, groups from five digits). The
  panel's own five texts are codes `plugin.panel.title`, `.stateFile`,
  `.load`, `.queued`, `.hostFirst` in `native/plugins.ts`, English and
  Spanish. Every panel text, load refusal and preparation error goes
  through `in_language`. The X11 panel draws Latin-1, which has every letter
  of both languages; a long dash is drawn as "-", as before.
- Tests: `windfall-plugin` `panel::windows::tests::the_native_panel_renders_in_spanish_and_in_english`
  opens the real Win32 panel in a hidden host window and reads every
  control's text (Spanish, then English); `panel::tests::the_panel_words_follow_the_language`;
  `language::tests` (settings file read, folder name checked against
  `tauri.conf.json`'s identifier); `windfall-core`
  `coded_messages_read_in_spanish_with_their_words_and_numbers` and
  `a_language_is_read_from_a_setting_or_a_locale`; desktop
  `settings::tests::the_interface_language_is_kept_where_the_exported_plugin_reads_it`;
  frontend `features/layout/remember-language.test.ts`. All pass
  (`target/round8i-s2a.log`, `-s2b`, `-s2c`, `-fe3`). The Cocoa and X11
  panels type-check in the cross-target gates; drawing them was not run.

## Gates

Run one at a time by `target/round8i/gates.sh` on `dac47698`, no affinity or
priority settings. Rust counts are passed / failed / ignored.

| Gate | Command | Result |
|---|---|---|
| Fixture and native binaries | `cargo build --offline --manifest-path crates/windfall-plugin-host/test-plugins/Cargo.toml --target-dir target`; `cargo build --offline -p windfall-desktop -p windfall-plugin -p windfall-plugin-host --bins --lib` | built (`round8i-gate-fixtures.log`, `-native.log`) |
| Format | `cargo fmt --all --check` | pass |
| Strict lint | `cargo clippy --workspace --all-targets -- -D warnings` | pass, no diagnostics (`round8i-clippy1.log` built it after the alias fix; the gate step confirmed from cache) |
| Rust workspace | `cargo test --workspace --no-fail-fast -- --test-threads=1` | 172 unit and integration blocks: **4,890 / 0 / 40**, 1,035 s (`round8i-gate-workspace.log`). The step's exit 101 came from its doc-test phase, where every crate's `rustdoc` stopped with `0xc0000142 STATUS_DLL_INIT_FAILED` at the moment the session environment restarted (04:10); see the next row |
| Doc tests, again | `cargo test --workspace --doc --no-fail-fast -- --test-threads=1` | 16 blocks, all ok (the workspace has no doc tests), exit 0 (`round8i-gate-doctests.log`) |
| Release E1 | `cargo test --offline --release -p windfall-dsp --test dsp e1_cpu -- --test-threads=1 --nocapture` | 3 / 0 / 0 |
| Release P2 | `… p2_effect_budgets …` | 4 / 0 / 0 |
| Release instruments | `… instrument_deadlines …` | 22 / 0 / 0 |
| Release threading | `cargo test --offline --release -p windfall-engine --test engine release_many_heavy_tracks -- --test-threads=1 --nocapture` | 1 / 0 / 0 |
| Bindings | `bash scripts/gen-bindings.sh target/round8i/final-bindings`; `node scripts/check-bindings.mjs …`; `node --test scripts/check-bindings.test.mjs` | 627 files; all current; tests pass |
| Simulator | `bash scripts/build-sim.sh`; `node scripts/check-sim.mjs` | 7,559,821 bytes; current |
| Frontend typecheck, lint | `pnpm --dir apps/desktop typecheck`; `… lint` | pass; pass |
| Frontend suite | `pnpm --dir apps/desktop test --maxWorkers=4 …` | 539 files, **6,521 passed, 0 failed**, 677 s (`round8i-gate-frontend.log`, `round8i-gate-frontend-results.json`) |
| Production frontend | `pnpm --dir apps/desktop build` | pass |
| Parity consistency | `node scripts/parity.mjs --check` | 339 / 342 accounted for; `PARITY.md` up to date; tracker unchanged |
| Release and resource scripts | `node --test scripts/release/release.test.mjs scripts/release/workflow.test.mjs` | 69 / 0 |
| Apple host | `cargo check --offline --target aarch64-apple-darwin -p windfall-plugin-host --all-targets` | pass, no diagnostics |
| Apple desktop and export | `bash target/round5/macos-check.sh` (`-p windfall-desktop -p windfall-plugin`) | pass, no diagnostics; `windfall-plugin` rechecked with the Cocoa panel change |
| Linux host and common | `bash target/round5/linux-check.sh -p windfall-plugin-host -p windfall-core -p windfall-dsp -p windfall-project -p windfall-ipc -p windfall-stretch` | pass, no diagnostics |
| Linux broad integration | `bash target/round5/linux-desktop-check-only.sh` (includes `windfall-plugin`) | pass, no diagnostics; the X11 panel change rechecked |
| Tauri debug build | `pnpm --dir apps/desktop tauri build --debug --no-bundle` | built, 690 s |
| Analyzer window | `node scripts/analyzer-window-smoke.mjs target/debug/windfall-desktop.exe … 9271` | **gate run failed** on its 13th check ("limiter reports real input/output/reduction"); four re-runs on the same binary passed, 18 checks each (`round8i-analyzer2.log` to `-analyzer5.log`). See "Not resolved" |
| Sample window | `node scripts/sample-workflow-window-smoke.mjs … 9272` | 65 checks, all pass |
| Notebook window | `node scripts/notebook-isolation-window-smoke.mjs … 9273` | 35 checks, all pass |
| Video window | `node scripts/video-player-window-smoke.mjs … 9274` | 35 checks, all pass |
| Language window | `node scripts/language-switch-window-smoke.mjs … 9275` | 16 checks, all pass |
| Factory content | `cargo run --offline -p windfall-factory -- verify content/factory` | content matches the generator |

Every window report names the software memory sink. Against the start of
the round: Rust +24 tests (4,866 → 4,890), frontend +3 (6,518 → 6,521).

Cross-target: the macOS (aarch64-apple-darwin) and Linux (x86_64 GNU)
checks type-check the changed crates, including the Cocoa and X11 panels and
the plugin's system-language code for each system. Nothing was run on those
systems.

## Not resolved

- **Analyzer window smoke, intermittent.** In the gate run the smoke waited
  until the limiter's history had more than 10 blocks, then read the view
  200 ms later and found 0 blocks (`target/round8i-analyzer-window/report.json`).
  The history emptied for a moment between two reads, so this is not a
  slow-start timeout. Four runs afterwards on the same binary passed, reading
  12 to 29 blocks. The cause is not known. Nothing in this round touches the
  analyzer, the limiter's history or its editor. The interface's new
  start-up call (`remember_language`) writes the settings file once per
  window, and I have not ruled it out. This round's rows do not depend on
  it, but the auditor should know the gate was not clean on the first run.
- `wf-languages`' remaining scope items (internal error kinds named inside a
  diagnostic `detail`; OS and library causes verbatim) are recorded in the
  languages record's decisions, not as a decision in `docs/qa/README.md`.

## Where this stops

Commits on `feat/parity-100` after `8ccae116`: the worker's commits from
`opus/r8-rows` (`333e665b` … `366e70e2`), the merge `073ba694`, `f844cf81`,
`dac47698` and the commit that holds this record. Nothing is pushed. No
build or test process is left running.

| Row id and summary | Evidence you actually ran | Still open | Recommendation |
|---|---|---|---|
| `wf-project-settings`: project meter, timebase and pan law, edited, saved, undoable, heard; hosted plugins take each property in exactly one route | The audit's gap is closed on the app's own path. The two new native tests use fixtures that raise the restart themselves (VST3 class 29 through `kNoteExpressionChanged`, CLAP through `request_restart`) and go through `Update::Capture` → `capture_pending` → `Record::readmit` (fade and carry first) → `ReadmitPlugin` → `prepare_document`/`bridge_audio` → replay. That is 32 runs (24 VST3, 8 CLAP) under all four laws. Each asserts ramp continuity frame by frame, the click bound, one voice, no stuck note, no allocation, and live equal to offline (difference 0 in every run). The 106 desktop plugin tests and the host-level restart and scan tests pass. The panel wording matches the revised README entry, with an English and Spanish UI test; the time-signature popover has a UI test. Full gates above. | EXTERNAL-ONLY: third-party CLAP and VST3 plugins that change their note capabilities while running (only owned fixtures here), and how they apply their own per-note pan. SCOPE: README decision `wf-project-settings` (revised after the round-eight audit): one summed output; per-note host processing inside it not attempted; the channel-applied property follows the most recent sounding note. | `done` |
| `fmt-flp-import`: FL Studio projects import with notes, clips, automation, routing, controllers, samples, and device state where a stand-in control exists | The seven audit fields, plus nine more the descriptor check exposed, are mapped, each with an import, audio and reopen fixture test (12 / 0). The pinned list covers native, channel and synth-switch omissions with reasons, and a test checks each device's unset stand-in controls against its actual descriptors (3 / 0). Omitted fields are dropped lines listed per device. The import review renders from a real conversion through the rebuilt simulator, and Windfall's channel device names follow the language. `coverage.md` equals the pinned list (43 devices, 269 families, compared by script). Full FLP suite and gates above. | EXTERNAL-ONLY: a corpus of real FL Studio project files, and listening against FL Studio itself. Several value scalings are Windfall's disclosed reading of unstated public ranges (listed in `coverage.md`). SCOPE: README decision `fmt-flp-import` (revised after the round-eight audit): adding controls to stand-ins is not part of the row; device state without a public layout stays opaque with a warning. | `done` |
| `wf-languages`: shows the interface in a chosen language (English, Spanish) | Everything in the languages record, plus this round. The exported plugin's panel now follows the chosen language: the app keeps it in `settings.json`, the plugin reads it (falling back to the system language), and its texts and coded messages come from the shared catalogs through `in_language`. A real Win32 panel is rendered and read in Spanish and English by a test. The merge's new FL lines, notes and channel device names have Spanish, and the guards pass (message guard 3 / 0, terms and catalog tests). The language window smoke passes 16 checks. Frontend 6,521 / 0, Rust 4,890 / 0. | EXTERNAL-ONLY: a native speaker's review of the Spanish (including this round's new entries), and the Cocoa and X11 panels drawn on macOS and Linux (type-checked only). SCOPE: no README decision covers this row's scope. The languages record (`2026-10-14-round8-languages.md`, "Decisions" and "Still open") keeps OS and library causes verbatim by design and leaves internal error kinds named in a diagnostic `detail`. If the auditor requires a README decision for those, it has to be added first. | `done` |
