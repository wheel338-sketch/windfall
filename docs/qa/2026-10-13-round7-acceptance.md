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
