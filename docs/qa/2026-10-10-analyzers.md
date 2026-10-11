# Analyzers (roadmap T8) — 2026-10-10

Branch `worktree-agent-a2bd1b4040219f843`. Rows: `vis-fruity-big-clock`,
`vis-fruity-db-meter`, `vis-fruity-spectroman`, `vis-wave-candy`,
`win-mixer-waveform-view`, `fx-fruity-parametric-eq2`, `fx-fruity-limiter`,
`fx-tuner`.

**No row is claimed complete.** Part-way through this work the rule changed
to "edit only, the parent builds and tests". Everything written after that
point has never been compiled, type-checked or run. The two things that were
run before the rule changed are listed under "Executed", with their real
results. Nothing else in this document is evidence that anything works.

## Per row

| Row | Claim | Implemented | Tests | Remaining gates |
| --- | --- | --- | --- | --- |
| `fx-fruity-limiter` | Partial. DSP and engine part executed and passing; editor and native service written, not executed | Compress mode (threshold, ratio, knee, attack, shared release), gate (threshold, release, held-peak detector), per-block observation that does not consume the meter; engine carries the observation per block to the control side; editor has a control for every setting and a scrolling history of input, output, reduction and gate from the limiter's own readings | `limiter_dynamics.rs` (12, executed, passed); `analyzer_capture.rs` limiter and compression/gate cases (executed, passed); session `an_effect_slot_is_sent_the_limiters_history_and_the_tuners_calibrated_reading`; `analyzer-editors.test.tsx` limiter cases (written, not executed) | Parent build and run of the session and frontend tests; regenerated `descriptors.json`; a look at the history in the running app. Save/reopen of the new settings is covered only by the serde round-trip in `limiter_dynamics.rs` and the existing generic effect save path, not by a dedicated project-file test |
| `fx-tuner` | Partial. Calibrated reading executed at DSP and engine level; editor and service written, not executed | Tuner counts its blocks so a stale readout is not resent; engine forwards the readout; purpose-built editor shows note, cents, frequency, needle, reference control, and no note without a pitch | DSP and engine tuner cases (executed, passed: A4 at 440 Hz against 432 Hz reads +31.8 cents within 1.5); session effect-slot case; `tuner-editor.test.ts`, `analyzer-editors.test.tsx` tuner case (written, not executed) | Parent run; check in the app against a real instrument or tone. The tuner reads the track it is inserted on; there is no microphone input path |
| `fx-fruity-parametric-eq2` | Partial. Written, not executed on the UI side | The EQ editor asks the analyzer for its track's post-fader spectrum and draws the real FFT behind the curve on the same frequency axis, labelled as the track's output; the response curve is unchanged | Engine capture tests (executed, passed) prove the track spectrum source; `analyzer-editors.test.tsx` EQ case (written, not executed) | Parent run. The spectrum is the track after its fader, so it shows the EQ together with everything else on the track; a tap directly before/after the EQ slot does not exist |
| `vis-wave-candy` | Partial. Written, not executed on the UI side | Signal scope (`view.signalScope`): peak meter with RMS, hold and clip light, oscilloscope with width and trigger options, spectrum, spectrogram, vectorscope; each view can be shown or hidden; source is the mix output or any mixer track; preferences persist | Engine capture tests (executed, passed); session analyzer tests; `scope-panel.test.tsx` (5), `math.test.ts` (11) (written, not executed) | Parent run of everything unexecuted; the real-app smoke below |
| `vis-fruity-spectroman` | Partial | Existing mix-output Spectrum and Spectrogram panels untouched. Source-selectable spectrum and spectrogram are in the signal scope | As for the scope | The existing two panels still show the mix only. Real-app smoke |
| `vis-fruity-db-meter` | Partial | Existing Large master meter untouched. A source-selectable large peak meter is the signal scope's meter view | Session `the_feed_a_window_is_sent_tells_tracks_apart_for_the_large_meter_and_the_waveform_meters` (written, not executed) | The stage view itself has no source selector. Real-app smoke |
| `vis-fruity-big-clock` | Partial | No change to the view | Session `the_playhead_a_window_is_sent_follows_play_a_seek_and_the_loop_for_the_large_clock` (written, not executed) | Real-app smoke with the view open |
| `win-mixer-waveform-view` | Partial | Fixed: waveform interest and history are now cleared when the project is replaced (they were not, contrary to `docs/MIXER-WAVEFORM-METERS.md`) | The same session feed test: per-track extrema told apart, stale generation refused, deleted track, replaced project (written, not executed) | Parent run; real-app smoke |

### The gate that is still open for all five view rows

`ANALYZER-TAPS.md` step 5 asks for a smoke through the real device callback
and the real window. What exists is one level short of that:

- Engine: real mixed audio through a hand-run processor (executed, passed).
- Session: the native `Session` with real slots, selections, tickets and the
  exact JSON text a window is sent, processor run by hand (written, not
  executed).
- Frontend: the real views fed events of the native shape through a watched
  backend (written, not executed).

Not built: a run of the actual Tauri app with WebView2 and a WASAPI stream.
A plan that needs no new dependency: build the app, start it with
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>` and
`WINDFALL_SILENT`-like low output gain, and drive the scope over the DevTools
protocol from a Node script, reading the `data-*` attributes the views set
(`data-phase`, `data-peak-db`, `data-blocks`, `data-peak-hz`, `data-live`).
Two small shell changes would be needed first: a config-directory override
so a test run does not touch the user's settings, and an output gain other
than zero, because the mix-output source is taken after the output gain and
`WINDFALL_SILENT=1` makes it silent. No physical-device or other-OS evidence
exists.

## Executed (before the edit-only rule)

Windows 11, Git Bash, `scripts/msvc-env.sh`, `CARGO_BUILD_JOBS=4`.

| Command | Result |
| --- | --- |
| `cargo test -p windfall-dsp` | exit 0. `dsp` target 179 passed, 5 ignored; lib 454 passed; other targets 26, 13, 6, 21 passed. Includes `limiter_dynamics` after one wrong expectation in the test was corrected |
| `cargo test -p windfall-engine --test analyzer_capture` | 9 passed, 0 failed |
| `cargo test -p windfall-engine` (whole crate) | **Did not run.** The linker failed with exit code `0xc0000142` on four test binaries while the machine was out of resources. There is no engine regression result |

`rustfmt` was run on the new Rust files (allowed under the rule). It parses
them; that is a syntax check, not a type check.

## What exists

| Layer | Files | State |
| --- | --- | --- |
| DSP | `crates/windfall-dsp/src/{limiter.rs,observe.rs,tuner/mod.rs,effect.rs,lib.rs}` | Compiled and tested before the rule change |
| Engine captures | `crates/windfall-engine/src/{capture.rs,capture_control.rs}`; hooks in `processor.rs`, `controller.rs`, `project_preparation.rs`, `mixer.rs`, `rack.rs`, `lib.rs`; `Processor::reopen` | Compiled and tested before the rule change, except `Controller::heard_position`, added afterwards |
| IPC types | `crates/windfall-ipc/src/analyzer.rs` | Not compiled |
| Native service, commands | `apps/desktop/src-tauri/src/session/analyzers.rs`, `commands.rs`; hooks in `session/{mod,realtime,files}.rs`, `lib.rs` | Not compiled |
| Frontend | `apps/desktop/src/features/analyzers/*`, `features/effects/{limiter/history.tsx,limiter/limiter-editor.tsx,tuner/tuner-editor.tsx,eq/live-spectrum.tsx}`, edits to `eq/eq-display.tsx`, `eq/eq-editor.tsx`, `effect-editor.tsx`, `editor-props.ts`, `layout/{register-actions.ts,app-shell.tsx}`, `lib/ipc/{backend,tauri,mock}.ts` | Not type-checked, linted or run |

How it fits together: a stream has two capture seats. The control side
prepares an analyzer for the mix output (after the output gain, before the
device scrub) or for one track after its fader and stages it; the audio
thread takes it over between buffers and copies at most 256 frames per call
into a prepared queue, with no allocation, lock or analysis. The realtime
thread runs the FFT, and the session's analyzer service sends each window
slot at most thirty frames a second, one unacknowledged frame at a time.
Every selection has a ticket that is never reused; frames of an earlier
selection are dropped at the service and again in the frontend feed.

## Tests written

| File | Tests | Executed |
| --- | --- | --- |
| `crates/windfall-dsp/tests/dsp/limiter_dynamics.rs` (registered in `tests/dsp/main.rs`) | 12 | Yes, passed |
| `crates/windfall-engine/tests/analyzer_capture.rs` | 9: `each_source_reports_its_own_audio_and_a_new_selection_never_shows_the_old_one`, `selections_are_checked_against_the_project_and_refusals_change_nothing`, `a_seek_a_transport_change_and_a_new_plan_never_share_a_window_with_what_came_before`, `a_deleted_source_shows_nothing_and_a_new_stream_or_rate_makes_old_tickets_stale`, `a_look_that_comes_too_late_loses_audio_openly_and_recovers_on_a_fresh_window`, `listening_to_the_mix_changes_no_bit_of_the_device_output_and_reports_bad_samples_as_bad`, `a_limiter_sends_its_own_levels_and_reduction_block_by_block_and_keeps_its_meter`, `compression_and_the_gate_are_heard_and_reported_through_the_engine`, `a_tuner_on_the_captured_track_reads_the_note_against_its_reference` | Yes, passed |
| `crates/windfall-ipc/src/analyzer.rs` `mod tests` | 2 | No |
| `apps/desktop/src-tauri/src/session/analyzers.rs` `mod tests` | 2 | No |
| `apps/desktop/src-tauri/src/session/tests/analyzers.rs` (registered in `session/tests/mod.rs`) | 6: `each_slot_is_sent_its_own_source_and_no_frame_of_an_earlier_choice`, `a_seek_a_deleted_track_a_new_project_and_a_new_stream_each_end_what_was_shown`, `a_view_or_a_stream_that_stops_keeping_up_is_stalled_openly_and_resumes_under_a_new_ticket`, `an_effect_slot_is_sent_the_limiters_history_and_the_tuners_calibrated_reading`, `the_feed_a_window_is_sent_tells_tracks_apart_for_the_large_meter_and_the_waveform_meters`, `the_playhead_a_window_is_sent_follows_play_a_seek_and_the_loop_for_the_large_clock` | No |
| `apps/desktop/src/features/analyzers/math.test.ts` | 11 | No |
| `apps/desktop/src/features/analyzers/scope-panel.test.tsx` | 5 | No |
| `apps/desktop/src/features/effects/tuner/tuner-editor.test.ts` | 3 | No |
| `apps/desktop/src/features/effects/analyzer-editors.test.tsx` | 5 | No |

Commands for the parent, in order:

```bash
scripts/gen-bindings.sh            # then scripts/build-sim.sh
cargo test -p windfall-dsp
cargo test -p windfall-ipc --lib analyzer
cargo test -p windfall-engine --test analyzer_capture
cargo test -p windfall-engine
cargo test -p windfall-desktop --lib session::analyzers
cargo test -p windfall-desktop --lib session::tests::analyzers -- --test-threads=1
pnpm --dir apps/desktop test src/features/analyzers src/features/effects --maxWorkers=4
pnpm --dir apps/desktop typecheck && pnpm --dir apps/desktop lint
```

## Least-sure spots (go here first)

1. `session/analyzers.rs::analyzer_tick` — a long function with split
   borrows of `State { subscriptions, seats }` and a closure passed to
   `Controller::capture_poll` that reads both through shared reborrows
   (`seat_view`, `readers`). The likeliest place for a borrow error.
2. `session/analyzers.rs::analyzer_select` — closures `same`, `used_by_others`
   and `refuse` borrow `select` and `subscriptions` before
   `subscriptions[at]` is borrowed mutably.
3. `commands.rs::analyzer_subscribe` takes
   `Channel<tauri::ipc::InvokeResponseBody>` and sends
   `InvokeResponseBody::Json(String)`. Checked by reading the vendored Tauri
   2.12.1 source, not by compiling.
4. `capture_control.rs::heard_position` calls the private
   `Controller::playing` and `inner.shared.tick()`; it relies on being a
   child module of `controller`.
5. Session tests assume: the default subtractive synth sustains a held note;
   a hard-panned track has an exactly silent other side; the default pattern
   is one bar (3840 ticks) at 120 BPM; the tuner locks to an A from the
   synth's default patch. `low_share` (spectral split at 400 Hz) is the
   softest assertion. The stall timings are counted in 17 ms ticks against
   `ACK_TIMEOUT` and `AUDIO_TIMEOUT`.
6. Analyzer quotas are per process (8 taps, 8 tap slots). The desktop lib
   test binary runs many sessions at once, each of which now takes two tap
   slots when it can; only the first four alive get capture seats. The
   session tests wait for one that did. If they are flaky, run them with
   `--test-threads=1` to separate quota contention from a real failure. The
   same quota means existing `spectrum` unit tests in the engine lib, which
   already assumed a free tap, are no worse off, but they were not re-run.
7. Frontend: `features/analyzers/types.ts` is written by hand to match the
   Rust serde shapes; compare with the regenerated bindings. The limiter
   editor binds seven ids that are not in the checked-in `descriptors.json`
   and throws until it is regenerated. `eslint-plugin-react-hooks` v7 may
   object to refs used inside the handlers passed to `useAnalyzerFrames`.
8. Tailwind class `group-data-in-tune/tuner:` and
   `group-data-clipped/meter:` are written without being rendered once.
9. `scope-panel.test.tsx` relies on jsdom's `requestAnimationFrame` for the
   meter's DOM updates and on Base UI's menu exposing `menuitemradio`.

## Behaviour decisions worth a second opinion

- The mix-output source is after the output gain, as the integration
  proposal specified and as the existing spectrum tap does. With
  `WINDFALL_SILENT=1` it is silent while track sources are not.
- An ordinary pattern or song loop is not a break in the analysis: the audio
  is continuous and so are the windows. Seek, play, stop, mode change,
  count-in, navigation jumps and a new plan are breaks.
- A frame flagged `gap` clears a view's history instead of leaving a hole.
- Status changes are sent without waiting for an acknowledgement; only
  frames are credit-gated.
- Each effect editor has its own slot (`fx-<id>`). With two sources and four
  slots for the app, a third editor on a third track is refused and says so.
- `Processor::reopen` is a new public engine function, added so a headless
  host and the tests can change stream and rate without a device.

## Parent must regenerate

- `apps/desktop/src/bindings/*` (`scripts/gen-bindings.sh`): new
  `LimiterMode`, wider `LimiterParams`, all `Analyzer*` types, and
  `descriptors.json` with the limiter's seven new settings.
- `apps/desktop/src/lib/ipc/sim/windfall_sim.wasm` and its metadata
  (`scripts/build-sim.sh`): `windfall-dsp` changed.

## Shared seam files touched

`crates/windfall-ipc/src/lib.rs` (two lines), `crates/windfall-dsp/src/effect.rs`
(one method appended to `AnyEffect`), `crates/windfall-dsp/src/lib.rs`,
`apps/desktop/src-tauri/src/commands.rs` (five commands appended, five names
in the handler list), `apps/desktop/src-tauri/src/lib.rs` (one line),
`apps/desktop/src-tauri/src/session/mod.rs` (module, re-export, one field,
one initializer), `apps/desktop/src/lib/ipc/{backend,tauri,mock}.ts`,
`apps/desktop/src/features/layout/register-actions.ts` (action
registration).

Outside the seam list, narrow edits: `session/realtime.rs` (one line: the
tick), `session/files.rs` (one line: project replaced), engine
`processor.rs`, `controller.rs`, `project_preparation.rs`, `mixer.rs`,
`rack.rs`, `crates/windfall-dsp/src/multiband/dynamics.rs` and test files in
dsp, engine and project (`..LimiterParams::default()` on struct literals
that no longer name every field), `features/layout/app-shell.tsx` (mounts
the scope), `features/effects/{editor-props.ts,effect-editor.tsx}` (the
track is passed to editors; the tuner gets its editor).

## Docs now out of date

`docs/ANALYZER-TAPS.md` and `docs/ANALYZER-INTEGRATION.md` still describe
the engine hook, the service and the views as future work.
`docs/integration/seams/tuner.md` says the tuner has no UI.
`docs/MIXER-WAVEFORM-METERS.md` was right about clearing interest on project
replacement only from this change on. None of them was edited here.
