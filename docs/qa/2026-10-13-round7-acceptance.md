# Round-seven acceptance — 2026-10-13

Rows: `wf-languages`, `wf-project-settings`, `fmt-flp-import`. Main checkout,
branch `feat/parity-100`, started at `eca4de0d`. One build or test process at
a time through `target/round7f/run.sh` (Rust: MSVC environment,
`CARGO_BUILD_JOBS=8`, an isolated `TS_RS_EXPORT_DIR`, the native fixture
variables) and `target/round7f/front.sh` (Node and pnpm on PATH). Logs are
`target/round7-final-<step>.log`. No sub-agents, no audio device, no ignored
hardware test, no new dependency, no branch, push, pull request or tag.
`docs/parity/parity.json` and `PARITY.md` are untouched.

## Running record

### A second session was still building when this one started

This session was started at 19:12 on the premise that the round-seven builder
had finished and the tree was clean at `eca4de0d`. It had not: its closing
gate run (`target/round7/gates4.sh`, started 19:12:57) was in flight and it
had uncommitted edits in `crates/windfall-flp`, `docs/flp/coverage.md`, its
record and the simulator. What happened, in order:

- 19:13: `git merge --no-ff opus/r7-languages` committed as `579c40a9`
  (531 files, frontend only, no conflicts). The merge commit contains none of
  the builder's uncommitted files.
- 19:12:30 to 19:13:20: `pnpm typecheck` (exit 0) and `vitest src/lib/i18n`
  ran here while the builder's `cargo build` gate step was running. These are
  the only commands of this session that overlapped another build.
- 19:15: the overlap was noticed from `git status` and the process list. From
  then until the builder reported its commit, this session ran no build or
  test and edited no source in the checkout; its own uncommitted step-one
  edits were set aside as a patch (`target/round7f/step1-parked.patch`) so the
  builder's frontend gates saw the committed merge only. The two sessions
  agreed this by message.
- 19:47: the builder's gates finished and it committed `24117f6a` (its own
  paths only: omitted import controls filed as dropped in the review, with a
  test and the rebuilt simulator). It reported that it starts no further
  build. Its numbers for that source: `cargo test --workspace` 4838 / 0 / 40;
  Vitest 6495 / 2 over 534 files on the merged tree (the two translation
  guards). From here on this session is the only builder.

Consequence for the record: the builder's `gates4` frontend numbers describe
the merged tree at `579c40a9` plus its own files, not the tree it started
from. Nothing of this session was verified by the builder's run.

### Step 1: translation branch merged, guards green

Merged `opus/r7-languages` (`579c40a9`): no textual conflicts. On the merged
tree the two guards reported exactly what main had added since the branch's
base (`target/round7-final-i18n1.log`): three strings in
`features/channel-rack/voice/voice-panel.tsx` and 813 descriptor addresses per
language (473 distinct labels).

What was added:

- **Voice panel.** `Delay` and the delay knob's accessible name go through the
  catalog (`{title} delay`), and the restart switch is one parameterised
  message, `Restart {target} LFO on each note`, with the target translated as
  the neighbouring `Enable {target} LFO` does. The section title was built as
  `Pitch` + ` LFO` from the target's id and stayed English in Spanish; it is
  now one whole catalog entry per target (`Pitch LFO` → `LFO de afinación`),
  which also fixes the accessible names made from it. English text is
  unchanged.
- **Three-oscillator synth.** Fifteen entries in `descriptors-instruments.ts`
  (pulse width, octave layer and two-octave layer per oscillator, the filter's
  band-pass and high-pass levels and the four short forms).
- **Multiband delay.** The descriptor display path looks a name up by its
  exact English text (`translatedText`, and the guard checks
  `Object.hasOwn(catalog, name)`), so a parameterised entry is not supported
  and each of the 384 names is its own key. They are generated, not written
  out: `messages/descriptors-band-delay.ts` builds the 384 names, the 48 short
  forms the generic editor derives (`1 delay`), its 24 group headings
  (`Bank 1 time ms`) and the two layout choices from one sentence per property
  and language, with the key type kept exact through template-literal types
  (so `MessageKey` stays a closed union). Spanish: `Retardo de la banda 3 del
  banco 2`, `Nivel …`, `Panorama …`; headings `Tiempos del banco 1 (ms)`,
  `Niveles del banco 1`, `Panoramas del banco 1`; `16 bandas`.
- **`inspector-messages.ts`.** Two Spanish entries held U+FFFD where accented
  letters had been lost (`El par�metro {id} no est� disponible…`, `…no est�
  disponible. La ruta se conserva.`); both are restored. A scan of every
  catalog module for U+FFFD, UTF-8 read as Latin-1 and a question mark between
  letters finds nothing else (`target/round7f/mojibake.py`). The file had one
  stray CRLF (its last line); it is LF throughout now.

One test-harness change, written down because the rule is "fix the product,
not the test": `whole-app-coverage.test.ts`'s whole-application walk parses
about a thousand files with the TypeScript compiler and takes 5.8 s here on
its own, against Vitest's default limit of 5 s. Beside other workers it timed
out instead of reporting (`target/round7-final-i18n3.log`). It now has an
explicit 60 s limit. No assertion, pattern or invariant entry changed.

Commands for this step, on the merged tree with the additions above:

| Command | Result | Log under `target/` |
|---|---|---|
| `pnpm --dir apps/desktop test src/lib/i18n src/features/channel-rack/voice --maxWorkers=4` | 10 files, 37 passed, 0 failed (both guards) | `round7-final-i18n4.log` |
| `pnpm --dir apps/desktop typecheck` | pass, 23 s | `round7-final-s1-typecheck.log` |
| `pnpm --dir apps/desktop lint` | pass, 36 s | `round7-final-s1-lint.log` |
| `pnpm --dir apps/desktop test --maxWorkers=4 --reporter=default --reporter=json --outputFile=../../target/round7-final-s1-frontend-results.json` | 534 files, 6,497 passed, 0 failed, 0 skipped, 562 s | `round7-final-s1-frontend.log` |

### Step 3: `wf-project-settings` (commit `08c25553`, more below)

**Bridge protocol version eleven.** `crates/windfall-plugin-host/src/bridge/protocol.rs`
is now version 11 and the helper handshake is Hello5 (`auth.rs`). A
version-ten helper is refused at authentication with "requires Hello5 /
mapping ABI11"; `previous_authenticated_helper_hellos_are_closed_before_any_load_metadata`
now also sends Hello4, and `earlier_mapping_versions_never_downgrade_the_v11_ramp_handover_contract`
refuses headers 1 to 10. Each slot gained one area used in both directions
(a channel stage of 6 words and one carried note of 68 words per note
instance) and five header words: ramps handed in with a block, the departure
request and its frame, the ramps reported with a reply, and "faded out". The
pinned layout sizes moved from 602,432 to 880,960 bytes per slot and from
2,409,984 to 3,524,096 per region, with the reason in the test.

**3.1 Ramp carried across a replaced helper.** `NoteRamp`, `ChannelStage` and
`Carried` have an explicit wire form with validation (`project_expression.rs`,
`NotePan::state` in `windfall-dsp`). When the owner asks a bridged unit to
leave (`bridge::Departure`, used by `Runtime::bridge_audio` when the helper
it replaces reported a capability change), the unit tells its helper from
that frame on; the bank in the helper fades the channel stage out over 5 ms,
holds every note ramp where it is and reports ramps and stage with each
reply. The unit keeps the report of the last output that was *played* and
writes it to the per-target carry under the engine identity of the note. The
replacement unit hands the carried ramp in with the block that starts the
replayed note; its helper continues the ramp, moves the routes to its own
capabilities over the fade, takes the stage over and fades in. "Departed" is
only said once the block that ended faded has been played to its last frame.
A departure nobody follows up expires after two seconds, and a withdrawn one
clears the carry.

**3.2 Bridged plugin without note instances.** The unit now owns such notes
by id; the helper plays them by key and channel and its bank owns them for
the channel stage, so their pan and volume are applied after the plugin as in
process. `instance snapshot for key-only native plugin` is no longer an
error.

**Capture leaves the audio alone (first addition from the orchestrator).**
What triggered the 5 ms fade: any hand-back of a sounding instrument to its
owner. For CLAP an ordinary capture never handed anything back (state was
read while active). For VST3 every capture deactivated the plugin, in process
and in the helper, so a save while music played was an audible gap before
round seven and a faded gap after it. The new test showed it:
`a_state_capture_during_a_sounding_note_leaves_the_audio_untouched` differed
from frame 576 for VST3 (`target/round7-final-s3-capture2.log`). Fix: VST3
state is read from the running plugin (`vst3/instance.rs::save_state` no
longer refuses while active; loading while active is still refused), the
in-process capture and the capture in the helper take the active path for
VST3 as for CLAP, and only a plugin that asked to be restarted is taken back,
where its unit is replaced anyway. A failed VST3 plugin is still refused by a
capture; the audio side now publishes the failure for that. Audio Unit
capture is unchanged (macOS only, cannot be run here).

This reverses an earlier rule that tests had pinned ("active capture must
remain forbidden"). Expectations rewritten, each with a comment saying what
it asserted before and why it changed:

| Test | Was | Is |
|---|---|---|
| `vst3_host::refusing_native_deactivation_never_proves_inactive_state` | saving while active refused | saving reads active state; loading while active refused |
| `vst3_host::refusing_native_processing_stop_returns_the_running_processor`, `lifecycle_gain_offsets_transport_state_and_restart`, `wrapper_options::threaded_native_vst3_deactivation_refusal_returns_same_worker_pipeline` | `save_state().is_err()` while active | `is_ok()` |
| `bridge::unprocessed_capture_intent_is_separate_from_native_dsp_acknowledgement`, `plugins::bridge::tests::native_vst3_capture_keeps_settled_gain_without_acknowledging_pending_note` | VST3 capture reconciled pending intent into the plugin by deactivating it | pending intent stays a companion of the state for VST3 as for CLAP |
| `ownership_tests::runtime_repair_capture_reconciles_absent_controls_and_keeps_timeout_intent`, `vst3_capture_without_a_callback_reads_state_and_never_deactivates_the_processor` (renamed from `vst3_no_callback_refuses_capture_…`) | a capture without an audio callback was refused "block boundary" | it succeeds, carries the pending control, and the processor stays active |
| `ownership_tests::capture_refusal_returns_the_exact_active_adapter_…`, `runtime_repair_capture_refusal_retains_pending_controls_…` | an ordinary capture was refused by a plugin that refuses deactivation | the ordinary capture succeeds; the refusal is exercised through a restart capture (`restart_capture`), the path that still takes the plugin back |
| `ownership_tests::recovery_failure_stays_dry_and_can_be_retired_on_its_owner` | reactivation failed after a capture | the same failure through a hand-over and return (`take_back_and_restore`) |
| `session::tests::plugins::n4_ordinary_capture_never_deactivates_so_the_document_intent_stands` (renamed from `n4_native_deactivation_edit_supersedes_…`) | the edit a fixture makes on deactivation won over the document value | no deactivation happens, so the document value stands (0.625, output 0.3125); the deactivation edit is still honoured on a restart (`native_vst3_dirty_restart_and_deactivation_edits_…`, passing) |

Measured so far (helper processes are real, `WINDFALL_DESKTOP_BRIDGE_HELPER`):

| Test | What it measures | Result |
|---|---|---|
| `project_expression::tests::a_ramp_mid_flight_crosses_the_bridge_exactly_and_bad_words_are_refused` | ramp, carried note and stage through their wire form, four laws, a route change in flight; 300 further frames of copy against original; eleven kinds of bad words | bit-identical; all refused |
| `bridge::slots::tests::handed_ramps_the_departure_request_and_the_report_roundtrip_and_are_validated` | the new words through a slot in both directions, stale words not read, refusals | pass |
| `plugins::bridge::route_tests::bridged_vst3_capability_loss_and_return_mid_note_keeps_the_ramp_and_never_clicks` | six directions (volume, pan, both; lost and returned), four laws, live and offline, two helpers each plus two neutral ones: every frame of ramp, fade-out, silence and fade-in against the model; largest step against the click bound; gain before and after; one voice, none stuck; no allocation in `process`, `set_note_expression` or the replayed `note_on_instance` | 24 migrations: worst 4.8e-7; step at most 82% of the bound (0.00150 against 0.00501); gain before equals the frozen value to 5 decimals; live and offline identical |
| `…::bridged_clap_note_instance_loss_and_return_mid_note_keeps_the_ramp_and_never_clicks` | the same for CLAP losing and regaining note instances | 8 migrations: worst 1.9e-7; step at most 70% of the bound (0.01156 against 0.01657); identical |
| `…::a_bridged_plugin_without_note_instances_gets_the_channel_stage_like_in_process` | the MIDI-only CLAP twin through a helper against the in-process owner: four laws, live and offline, law change, per-note move, note gain, a newer note and its end | identical samples (worst 0.0) |
| `…::leaving_waits_for_the_fade_while_audio_runs_and_gives_up_when_it_does_not` | the wait of the owner: returns false within 500 ms without audio and the unit plays on; returns true once the faded output was played | pass |
| `plugins::runtime::route_tests::a_state_capture_during_a_sounding_note_leaves_the_audio_untouched` | in process, VST3 and CLAP: 4,096 frames with a pan ramp in flight, state captured from another thread from frame 512, against the same without a capture | identical samples |
| `plugins::bridge::route_tests::a_bridged_state_capture_during_a_sounding_note_leaves_the_audio_untouched` | the same through a helper, live and offline | identical samples |

Commands: `cargo test --offline -p windfall-plugin-host --no-fail-fast -- --test-threads=1`
296 / 0 / 7 over 20 result blocks (`s3-host5`); `cargo test --offline -p windfall-desktop --lib plugins:: -- --test-threads=1 --include-ignored`
106 of 107 passed, the one failure being the deactivation-edit test rewritten
above (`s3-plugins3`); `… --lib session::tests::plugin` 41 / 0 (`s3-session1`);
`cargo clippy --offline --workspace --all-targets -- -D warnings` clean
(`s3-clippy1`).

**3.3 A capability change without replacing the unit.** A VST3 plugin can
announce that its note expressions changed (`kNoteExpressionChanged`) without
asking to be restarted. The host treated that flag like any other restart
request, so in practice every capability change replaced the unit and the
same-unit path had no native coverage. Now:

- `PluginInstance::follow_note_expression_changes(true)` makes the VST3
  instance ask its controller again on the owner thread when that flag
  arrives and write the result to cells the running processor reads
  (`vst3/processor.rs::note_expression_masks`, shared with the instance and
  read through the threaded and wrapper layers). No restart is requested for
  that flag alone. Off, the default, nothing changes: the helper process
  keeps the default, so a bridged plugin still gets a replacement helper,
  now with the ramp carried (3.1).
- The desktop owner turns it on for in-process plugins. The unit already
  published the adapter capabilities at every control boundary and re-read
  its routes every block; with live cells that path is now reachable.
- Fixture class 28, "VST3 Live Expression Switch" (appended; class counts
  moved from 28 to 29): it offers pan and volume per note and can withhold
  either while it runs. Its controller announces the change through a
  gesture (`getParamStringByValue` marks, the idiom the fixture already uses
  for editor gestures) with `kNoteExpressionChanged` alone; its processor
  stops applying the property at the frame of a parameter point (8 for pan,
  9 for volume) and starts from neutral when it offers it again.

Not changed, and why it matters to an auditor: `midi_fallback` (whether a
VST3 plugin without a tuning expression is bent through its MIDI mapping) is
still decided at activation, and the capability panel of a *bridged* plugin
shows what the helper negotiated at launch (a bridged live change still
replaces the helper, which renegotiates).

**Release budget for the multiband delay (second addition).** No release
budget case existed for this effect in any suite, three-band layout
included, so there was no "existing case" to share a limit with. The new
case, `p2_effect_budgets::multiband_delay_three_and_sixteen_band_layouts_have_one_declared_cpu_budget`,
measures both layouts with the P2 harness (384 kHz, blocks of 1, 64, 256
and 2,048, the minimum of three trials on a performance core, reset
deadline included) against one declared share of 20% of a core, the tightest
share that suite declares. The sixteen-band workload has every band of the
playing bank sounding with its own delay, level and pan (asserted distinct),
feedback 0.75 to 0.85 on all three groups, the output fully wet, and the
other seven banks holding different settings.

Its first run failed, on reset and not on CPU: `reset()` took 3,343 µs
against the 167 µs a reset may take (`target/round7-final-s3-budget1.log`).
The delay zeroed every line in full: 32 lines of a second each, 67 MB at
384 kHz. The three-band layout before round seven had the same defect at a
fifth of the size, never measured; and a layout change did the same clear
inside a block. Fixed in the DSP, not in the test: `band_delay.rs::EchoLine`
counts what was pushed since the last clear and reads anything older as
silence, which is what a zeroed line returns.
`echo_line_tests::a_line_cleared_in_constant_time_reads_exactly_like_a_zeroed_one`
compares it bit for bit with a fully zeroed `DelayLine` over more than
10,000 reads across clears, wraps and partial fills; the existing band-delay
references (`cargo test -p windfall-dsp --lib spatial`, 28 / 0) are
unchanged.

| Layout | Block | CPU share of one performance core | Reset |
|---|---|---|---|
| three bands | 64 / 256 / 2,048 | 2.9% / 2.8% / 2.7% | 0.1 µs |
| sixteen bands | 64 / 256 / 2,048 | 11.0% / 10.9% / 10.9% | 0.1 µs |

(limit 20%; one-frame blocks are reported and not judged, as for every P2
case: 16.2% and 25.0%.) Log: `target/round7-final-s3-budget2.log`.

More measurements for this step:

| Test | What it measures | Result |
|---|---|---|
| `vst3_host::a_live_note_expression_change_is_followed_by_the_running_processor_without_a_restart` | class 28 through the host: pan, volume, both withheld and returned; followed (masks change, no restart) and not followed (restart requested, masks kept) | pass |
| `plugins::runtime::route_tests::vst3_live_capability_change_keeps_the_unit_and_hands_the_property_to_the_channel_and_back` | the production in-process unit with class 28: a note with gain 0.6 and a pan ramp in flight; pan, volume or both withheld and offered again, four laws: routes read again at the boundary, same unit and plugin, never deactivated, no restart, no allocation at the boundary; every frame against the fixture neutral output times note gain and law gain | 12 runs: worst 4.5e-8; largest step 13% of the bound; one voice, none stuck |

Commands after these changes: `cargo test --offline -p windfall-plugin-host --no-fail-fast -- --test-threads=1`
297 / 0 / 7 (`s3-host6`); `cargo test --offline -p windfall-desktop --lib plugin -- --test-threads=1 --include-ignored`
126 / 0 / 0 (`s3-plugins4`, includes the nine software capture opt-ins);
`cargo clippy --offline --workspace --all-targets -- -D warnings` clean
(`s3-clippy3`).

After step 3 the whole workspace was run once as an early regression check,
on `c928a1be`: `cargo test --offline --workspace --no-fail-fast -- --test-threads=1`
4850 / 0 / 40 over 187 result blocks, 1,068 s
(`target/round7-final-g-workspace1.log`). The final gate run is further down.

### Step 2, frontend part (commits `d9ef6f4a`, `a61f7dcd`)

Done on the frontend, without any Rust change:

- **History labels (item 3, frontend half).** `historyLabel(key, values)`
  is what a label is kept as: its catalog key, which is its English text,
  or `windfall-label:{key, values}` when it has values. Nothing is
  translated when the edit is made. `historyLabelText(label)` makes the words
  where the history is shown (`features/history/history-list.tsx::groupRuns`),
  so the panel follows the language whichever language an edit was made in.
  It also covers what the backend writes: the 183 static labels of
  `crates/windfall-project/src/lower.rs` now have English and Spanish
  entries (`messages/history-labels.ts`), a coded `windfall-message:` label
  is decoded, and the two labels the backend makes from a setting name
  (`Change {name}`, `Change script {name}`) are recognised, with the setting
  name translated when it is a catalog entry. A label that is none of these
  (an older version, a typed name, a broken envelope) is shown as it is.
  Sixteen files of label writers moved from `t("…")` to `historyLabel("…")`,
  the simulator's included.
- **Text kept in stores (item 4).** `errorSource(error)` is what a rejection
  is kept as (the backend text: a code with parameters, or English that is a
  catalog key); `errorMessage(error)` is its words in the interface language
  for text shown now and discarded with the language boundary. Fourteen files
  that keep an error in a store, or translate it when shown, keep the source,
  and their display sites call `localizeBackendText` (audio logger, backup
  job, library search, current source, input monitor, waveform meter,
  recording). Before this, `errorMessage` returned the untranslated source
  and about 120 places showed it raw, so a backend error in an inline alert
  was English in a Spanish interface even where a catalog entry existed, and
  a coded message would have been shown as JSON. That was not in the
  hand-off.
- **Titles fixed at registration (item 5).** A kept title can hold the way
  to make a value instead of the value (`() => item.label.toLowerCase()`).
  `dynamicMessage` makes it in the language shown, through `inLanguage`,
  which also gives the registry its English title when the app starts in
  Spanish. The eight piano-roll title families (colours, articulations,
  portamento presets, lengths, tools, snaps, lanes) and the playlist marker
  titles use it.
- **Guard.** `source-guard.ts` takes `historyLabel` for a catalog-key call
  (its key is checked) and still reports a label written without one;
  fixture test added. No invariant was added.

Tests added: `lib/i18n/history-labels.test.ts` (5: every backend label
literal has an entry in both languages, by reading `lower.rs`; keeping;
following a switch; unknown text; grouping by the words shown),
`lib/i18n/kept-values.test.ts` (3: English registry title when registered
in Spanish; `inLanguage` nests and restores; more than 40 piano-roll titles
right in Spanish and in English after switching both ways).

Expectations changed, because they asserted the old storage: the history
label of a step fill is kept as key and value (`steps.test.ts`, and the
`labels()` helpers of the rack and playlist tests read the history as it is
shown); `mock.test.ts` asserts the retained envelope through `errorSource`
and its words through `errorMessage`.

Commands: full frontend suite on these changes, first run 6,502 / 4 over 536
files, the four being the expectations above
(`target/round7-final-s2-frontend1.log`); after updating them the affected
folders 93 files, 987 / 0 (`s2-focused1`), `typecheck` and `lint` clean
(`s2-typecheck5`, `s2-lint2`). `d9ef6f4a` went in with a type error in one
test because a pipeline hid the typecheck result; `a61f7dcd` fixes it.

### Step 4: `fmt-flp-import`

Checked against the README entry of 2026-10-13:

- Exact, approximate and omitted counts: the review shows them per category
  (`import-dialog.tsx`), and since the builder's `24117f6a` an omitted
  control is counted as omitted and not as approximate.
- The pinned list: `open_list::decoded_fields_without_a_stand_in_control_are_exactly_the_recorded_list`
  passes (233 families, 33 device names).
- `docs/flp/coverage.md` says the same as the README.
- **Mismatch found and fixed:** the review named omitted fields only as
  lines of a category, mixed with the approximated ones and cut at 200 lines
  per category ("N additional details omitted"), so in a project with many
  devices an omitted field could go unnamed. `ImportReport` now carries
  `omitted_controls` (device, how many times it is used, field names, in
  layout order; at most 256 devices, with a count beyond that), filled where
  the native lines are filed, and the review shows it as a section "Omitted
  controls", one line per device, with the sentence that the original state
  is kept. Bindings regenerated (627 files, one new: `OmittedControls.ts`),
  simulator rebuilt (7,448,818 bytes, current).

Tests: `report::tests::omitted_controls_are_listed_by_device_whatever_the_line_limit`
(300 uses of one device overflow the category, the list stays whole; merge by
device; the 256-device bound); the builder's fixture test now also asserts
the list for Fruity Soft Clipper (`postgain`) and that it equals the dropped
lines; `import-dialog.test.tsx` "names the omitted controls of each device
in the review, and says nothing when none are". FLP suite 209 / 0
(`s4-flp2`); review tests 7 / 0 (`s4-ui3`).

### Step 2, Rust part (commit `4b8ff021` and after)

**The mechanism (item 6, first two guards).** `crates/windfall-ipc/src/ui_messages.rs`
now declares, beside the append-only code list, the parameters each code is
shown with (`UI_MESSAGE_PARAMS`, `ui_message_params`). `ui_message` refuses,
in debug and test builds, a message that lacks one of its declared
parameters. Tests:

- Rust, `every_code_declares_its_parameters_and_the_registry_only_grows`:
  one declaration per code in the same order, parameter names sorted, unique
  and alphanumeric, and the first and last code of each release of the
  registry pinned by position (0, 56, 57 and 81 of 82), which is what keeps
  it append-only. `a_message_without_one_of_its_declared_parameters_is_refused_in_tests`.
- UI, `native-messages.test.ts`: every Rust code has an entry in every
  language (already there), and now every entry, in every plural form, uses
  exactly the parameters Rust declares. It reads the Rust table as the
  formatter leaves it.

**First coded families (item 1, partly).** 25 codes, 28 sites: templates
(9), browser folders (4), plugin database (4), scores (3), mixer presets
(5). Each has an English entry, which is the text the backend used to build,
and a Spanish one. A nested cause is a parameter: what the operating system
or a library said is shown as it came, and a cause that is itself a coded
message (the plugin database's "not being saved" wraps its own read error)
is put into words too (`nativeMessage` decodes a parameter that is an
envelope). Six tests that asserted the English words of these messages now
assert the code (`browser.rs`, `plugin_database.rs`, `mixer_presets_qa.rs`,
`files_qa.rs`).

**The third guard: no bare prose (item 6).** The mechanism chosen is a test
over the desktop crate's own source, `apps/desktop/src-tauri/src/message_guard.rs`,
not a type at the command boundary: the crate has 238 commands and about 700
functions returning `Result<_, String>`, and a new error type there would
need `From<String>` to compile, which lets prose through again. The test
does two things. It checks that every `ui_message("code", json!({…}))` in
the crate names a registered code and gives each declared parameter. And it
finds every sentence with values in it that is still built with `format!`
and requires that set to equal `uncoded_messages.txt`: a new one fails until
it is coded or listed as internal with a reason; a coded one fails until its
line is removed. So the list can only shrink, and what is left is exact.

**What is left on item 1, exactly.** The hand-off listed 136 sites by a
pattern. The guard's own scan of the tree as it is now finds 150 distinct
parameterised sentences; triaged: 6 are not shown to anybody (the
assistant's model instruction, three file names, two HTTP lines) and are
listed as internal with their reasons; 28 sites are coded; **116 are still
built in English** and are listed, file by file, in the backlog section of
`uncoded_messages.txt`. Until each is coded a Spanish user sees it as
`Diagnóstico nativo: <English>`.

**Item 2, the core crates: inventoried, not converted.** The same scan over
`crates/*` (non-test code): windfall-project 190 sentences in 21 files
(almost all `check.rs` and the command lowering: what is wrong with a
project file, and why an edit was refused), windfall-flp 110 (the lines of
the import review), windfall-plugin-host 26, windfall-engine 22,
windfall-plugin 22, windfall-codec 21, windfall-midi 17 (already mapped to
codes at the boundary by `midi_error_message` and friends), windfall-ipc 10,
windfall-sim 4, and one each in analysis, dsp and factory. Besides these,
24 error types implement `Display` by hand. The approach I would take, and
took for the one family that is done (MIDI, before this round): where a
crate's error is an enum, map its variants to codes in `windfall-ipc`, so
the core crates stay free of interface concerns; where a crate builds a
sentence (project validation, the import review), give the IPC type a code
and parameters beside the text. None of that is done in this round beyond
what already existed.

**Item 3, history labels, backend half.** Not changed in Rust. The static
labels are translated at display by key and the two made from a setting
name by pattern (see the frontend part); nothing in the backend stores a
code. A label the backend builds some other way in future would be shown in
English; `history-labels.test.ts` fails if `lower.rs` gains a literal label
without a catalog entry.

**Item 7, the real window.** `scripts/language-switch-window-smoke.mjs` (new)
drives the debug desktop window with the software audio sink: it adds a
channel in English, switches to Spanish through the settings dialog, checks
the menu bar, a menu, the status bar, a dialog, a backend refusal (a
template named like a factory one, which arrives as `templates.factoryName`
with its name), a descriptor-made instrument editor and the history, checks
the project and its history are unchanged, switches back and checks again.
The Spanish it expects is read from the catalogs. Its first runs found two
things:

- The status bar said "Not saved to a file yet" (and would have said
  "Edited") in English in a Spanish window: the two were assigned to a
  variable as plain literals, which the static guard cannot see. They go
  through the catalog now, and the smoke checks the status bar.
- The profile directory of a smoke keeps the language choice; the smoke now
  starts from a new one each run.

The remount on a language change held in the real window: the project, its
generation and its history were identical before, during and after.

## Final gates

All ran one at a time through `target/round7f/gates.sh` on the source of
`eaa66579`, without affinity or priority settings (the deadline harnesses use
their recorded internal measuring-thread policy). Rust counts are passed /
failed / ignored. Logs are `target/round7-final-gate-<step>.log` unless said
otherwise.

| Gate | Command | Result |
|---|---|---|
| Fixture and native binaries | `cargo build --offline --manifest-path crates/windfall-plugin-host/test-plugins/Cargo.toml --target-dir target`; `cargo build --offline -p windfall-desktop -p windfall-plugin -p windfall-plugin-host --bins --lib` | built |
| Format | `cargo fmt --all --check` | pass |
| Strict lint | `cargo clippy --workspace --all-targets -- -D warnings` | pass, no diagnostics |
| Bindings | `bash scripts/gen-bindings.sh target/round7f/final-bindings`; `node scripts/check-bindings.mjs target/round7f/final-bindings`; `node --test scripts/check-bindings.test.mjs` | 627 files; all current; tests pass |
| Simulator | `bash scripts/build-sim.sh`; `node scripts/check-sim.mjs` | 7,448,810 bytes; current. The committed simulator was stale after `4b8ff021` (new codes in `windfall-ipc`); this rebuild is committed with the record. |
| Rust workspace, gate run | `cargo test --workspace --no-fail-fast -- --test-threads=1` | 4854 / 1 / 40 over 187 blocks, 963 s. The failure was `session::tests::library::the_factory_content_is_the_first_root_and_lists_as_folders_of_sounds`, which asserted the English words of a browser refusal that is now `browser.folderMissing`; it asserts the code now. |
| Rust workspace, confirmation | same, after that one change | 4855 / 0 / 40 over 187 blocks, exit 0, 856 s (`round7-final-gate2-workspace.log`); format and lint again clean (`gate2-fmt`, `gate2-clippy`) |
| Software capture opt-ins | `cargo test --offline -p windfall-desktop --lib plugins::runtime::capture_ack_tests:: -- --test-threads=1 --include-ignored` | 9 / 0 / 0 |
| Release E1 | `cargo test --offline --release -p windfall-dsp --test dsp e1_cpu -- --test-threads=1 --nocapture` | 3 / 0 / 0 |
| Release P2 | `… p2_effect_budgets …` | 4 / 0 / 0 (the new multiband-delay case: three bands 2.9 / 2.7 / 2.7%, sixteen bands 11.0 / 11.0 / 10.9% of a core at blocks 64 / 256 / 2,048, limit 20%; reset 0.1 to 0.2 µs) |
| Release instruments | `… instrument_deadlines …` | 22 / 0 / 0 |
| Release threading | `cargo test --offline --release -p windfall-engine --test engine release_many_heavy_tracks -- --test-threads=1 --nocapture` | 1 / 0 / 0 |
| Frontend typecheck, lint | `pnpm --dir apps/desktop typecheck`; `… lint` | pass; pass |
| Frontend suite | `pnpm --dir apps/desktop test --maxWorkers=4 --reporter=default --reporter=json --outputFile=../../target/round7-final-gate-frontend-results.json` | 536 files, 6,509 passed, 0 failed, 0 skipped, 534 s |
| Production frontend | `pnpm --dir apps/desktop build` | pass |
| Parity consistency | `node scripts/parity.mjs --check` | 342 rows: 334 done, 3 in progress, 5 won't do; tracker unchanged |
| Release and resource scripts | `node --test scripts/release/release.test.mjs scripts/release/workflow.test.mjs` | pass, 123 s |
| Apple host | `cargo check --offline --target aarch64-apple-darwin -p windfall-plugin-host --all-targets` | pass, no diagnostics |
| Apple desktop and export | `bash target/round5/macos-check.sh` (environment as in the round-six record) | pass, no diagnostics |
| Linux host and common | `bash target/round5/linux-check.sh -p windfall-plugin-host -p windfall-core -p windfall-dsp -p windfall-project -p windfall-ipc -p windfall-stretch` | pass, no diagnostics |
| Linux broad integration | `bash target/round5/linux-desktop-check-only.sh` | pass, no diagnostics |
| Tauri debug build | `pnpm --dir apps/desktop tauri build --debug --no-bundle` | built |
| Analyzer window | `node scripts/analyzer-window-smoke.mjs target/debug/windfall-desktop.exe target/round7-analyzer-window/report.json 9271` | 18 checks, all pass, software-memory-sink |
| Sample window | `node scripts/sample-workflow-window-smoke.mjs … target/round7-sample-window/report.json 9272` | 65 checks, all pass |
| Notebook window | `node scripts/notebook-isolation-window-smoke.mjs … target/round7-notebook-window/report.json 9273` | 35 checks, all pass |
| Video window | `node scripts/video-player-window-smoke.mjs … target/round7-video-window/report.json 9274` | 35 checks, all pass |
| Language window (new) | `node scripts/language-switch-window-smoke.mjs … target/round7-language-window/report.json 9275` | 14 checks, all pass |
| Factory content | `cargo run --offline -p windfall-factory -- verify content/factory` | content matches the generator |

Not run this round: the real ONNX stem harness (not in this round's list and
nothing it covers changed). No real audio device, MIDI device or ignored
hardware test was used; every window report names the software memory sink.

### Cross-target results

The four checks above compile the Rust changed this round for those systems:
the bridge protocol, the ramp hand-over, the helper and its capture, the
live note-expression cells and the threaded wrapper, the VST3 instance (macOS
and Linux builds of the plugin host), the desktop's departure and carry, the
message codes and the source guard (desktop and plugin crates), the
constant-time echo line (DSP). They are type checks only; nothing was linked
or run on another system. Audio Unit capture was not changed and still reads
state only when inactive.

## Where this stops

Commits on `feat/parity-100`, in order: `579c40a9` (merge of the translation
branch), `0fbb7510` (step 1), `08c25553`, `11ffcc5c`, `c928a1be` (step 3),
`d9ef6f4a`, `a61f7dcd` (step 2, frontend), `1ddf1874` (step 4), `4b8ff021`,
`eaa66579` (step 2, codes, guards, window smoke) and the closing commit that
holds this table, the corrected library test and the rebuilt simulator. The
builder's `24117f6a` sits between `579c40a9` and `0fbb7510`. Nothing is
pushed. No build or test process is left running.

| Row | Evidence run here | Still open | Recommendation |
|---|---|---|---|
| `wf-project-settings` — the project pan law and per-note pan and volume for hosted plugins | In process (round seven, builder): four routes times four laws within 1.5e-8; 32 capability loss and return migrations, CLAP and VST3, live and offline, every frame within 4.8e-7 of the model. Through real helper processes (this round): the same 32 migrations, every frame within 4.8e-7, largest step at most 82% of the click bound, live equals offline exactly, no allocation in `process`, `set_note_expression` or the replayed `note_on_instance`, one voice and none stuck; the owner's wait for the fade (returns without audio, returns once the faded output was played); a bridged plugin without note instances against the in-process owner, four laws, live and offline, identical samples; the wire form of ramps, stages and carried notes bit for bit, with eleven kinds of malformed words refused; version-ten helpers refused at authentication. Same unit: fixture class 28 withholds and returns pan, volume or both while running; the production unit re-reads its routes at the boundary, is never replaced or deactivated and asks no restart, every frame within 4.5e-8 of the law, largest step 13% of the bound; the host-level test of the same switch. Capture: a state capture during a sounding note leaves the output identical sample for sample, VST3 and CLAP, in process and through the helper, live and offline (VST3 capture used to deactivate the plugin). Release budget for the multiband delay at both layouts. Full suites and cross-target checks in the gate table. | EXTERNAL-ONLY: third-party CLAP and VST3 plugins (including ones that change note expressions while running, and real VST3 state read while processing), physical audio and MIDI devices, listening; running the bridge, the helper and the threaded wrapper on macOS and Linux (type-checked only); Audio Unit hosting on macOS (unchanged, still captures inactive). SCOPE: README 2026-10-13, `wf-project-settings`: one route per property per plugin output; mixed per-note routes are not attempted. | `done` |
| `fmt-flp-import` — reads notes, clips, automation, routing and sample references out of an FL project file | The README end state checked against the product: the review counts exact, approximate and omitted per category, an omitted control is filed as omitted (`24117f6a`), and since this round the review also lists every device with its omitted field names, uncut by the 200-line category limit (`ImportReport::omitted_controls`, "Omitted controls" section). Tests: `report::tests::omitted_controls_are_listed_by_device_whatever_the_line_limit`; the round-seven fixture test asserts the list for Fruity Soft Clipper and that it matches the dropped lines; `import-dialog.test.tsx` shows the section and its absence; the pinned open list (233 families on 33 device names) passes; `docs/flp/coverage.md` agrees with the README and now names the list. FLP suite 209 / 0; full suites in the gate table. | EXTERNAL-ONLY: a corpus of real FL project files, listening. SCOPE: README 2026-10-13, `fmt-flp-import`: the stand-in controls for the 233 recorded families are work on the stand-ins, not on the import row; device state with no public layout stays opaque with a per-device warning. (The review's per-field lines are still English in a Spanish interface; that is open under `wf-languages`.) | `done` |
| `wf-languages` — shows the interface in a chosen language | Translation branch merged; both guards pass on the merged tree (0 untranslated occurrences, 0 missing descriptor addresses); full frontend suite 6,509 / 0. Code-based history labels (frontend), store-kept errors as source translated at display, titles that follow a switch, all with tests. Rust: every message code declares its parameters (append-only registry test, debug refusal of a missing parameter); the UI test checks every code in every language has exactly those parameters; a source guard in the desktop crate checks every coded call and pins the English sentences that remain. 28 backend sites converted (templates, browser folders, plugin database, scores, mixer presets), nested causes decoded. The real window: switched to Spanish through settings and back, 14 checks (menu bar, a menu, the status bar, a dialog, a backend refusal sent as a code, an instrument editor, the history, the project unchanged); it found and fixed an English status bar. | SOFTWARE: (1) 116 parameterised sentences in the desktop crate are still built in English, each listed in `apps/desktop/src-tauri/src/uncoded_messages.txt` [backlog] (the guard fails if one is added, and when one is coded until its line goes). (2) The core crates: about 420 sentences built with `format!` (windfall-project 190, mostly project validation and edit refusals; windfall-flp 110, the import review lines; plugin-host 26, engine 22, plugin 22, codec 21, ipc 10, sim 4, analysis, dsp and factory 1 each) and 24 hand-written `Display` impls reach the interface in English; MIDI is the only family mapped to codes at the IPC boundary. (3) History labels the backend builds from a setting name are stored as English and recognised by pattern at display; they are not yet stored as a code with parameters. (4) Labels the playlist writes in English through `plural()` ("Delete clips" and the like) may lack catalog entries and would show in English; not enumerated. EXTERNAL: review of the Spanish by a native-speaking producer, text expansion in tight controls. | `in-progress` |
