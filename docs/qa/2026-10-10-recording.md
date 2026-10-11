# Recording, device I/O and audio editor — 2026-10-10

Branch `worktree-agent-a06e4facd1d47868d`, based on `fd60cdde`.

Rows: `core-audio-recording`, `win-mixer-audio-io`, `win-mixer-record-arm`,
`wf-count-in`, `wf-loop-recording`, `core-audio-logger`, `editor-edison`,
`wf-audio-settings`.

## Read this first: what was and was not executed

Part-way through this work the instruction changed to **edit only: no build,
test, lint, install, generation, soak or device run**. So this branch has two
kinds of statement, and they must not be mixed up.

**Executed, before that instruction** (Windows 11 10.0.26300, x86_64, commit
`fd60cdde` with a dirty tree, other agents' builds running on the machine):

| What | Command | Result |
| --- | --- | --- |
| Soak, song with effects, silent | `target/release/windfall-soak.exe --seconds 600 --buffer 128 --sample-rate 48000 --song --silent` | **pass**: 600 s, 0 xruns, max CPU 10.53 %, 28 810 176 frames. Requested 128 frames, **actual 480** (10 ms), WASAPI, Speakers (Stealth 700X Gen 3), 48 kHz, 2 channels. Log: `docs/perf/soak-2026-10-10-wasapi-480-song.txt`. |
| Engine endpoint unit tests, first version | `cargo test -p windfall-engine --lib recording_endpoint` | 3 passed. The module has since gained `InputFault` and two more tests that were **not** run. |
| Burst locator self-test | `cargo test -p windfall-desktop --lib the_burst_locator` | 1 passed. This also means the acceptance harness compiled at that point. |
| Device inventory | `cargo test -p windfall-desktop --lib device_inventory -- --ignored --test-threads=1` | 1 passed. `docs/evidence/recording-2026-10-10/inventory-default.json`. |
| Round trip through a virtual cable | `device_round_trip_alignment`, output "Speakers (Steam Streaming Microphone)", input "Microphone (Steam Streaming Microphone)" | **failed**: the take was discarded after 1.7 s. `docs/evidence/recording-2026-10-10/round-trip-cable.json`. See "A real failure" below. |

**Written, not executed: everything else.** That is all of the audio logger,
all of the editor tools, the capture fault reasons, the later edits to the
acceptance harness, every TypeScript file, and every test named below unless
the table above lists it. None of it has been compiled. No pass count is
claimed for it.

The audio logger and the editor DSP/session code were written by two helper
agents whose worktrees were cancelled before they committed or reported. Their
uncommitted files were copied into this branch and finished by hand: the
frontends, the editor's session tests and the documents are new here. Whether
the helpers had compiled their Rust is not known, so treat it as never built.

## What this machine has

From the inventory run:

- Outputs (all WASAPI shared mode, 2 channels, buffer range 480–480 frames):
  Speakers (Stealth 700X Gen 3) [default], Realtek Digital Output, SAMSUNG
  (NVIDIA High Definition Audio), AI Noise-Canceling Speaker (ASUS Utility),
  Speakers (Steam Streaming Speakers), Speakers (Steam Streaming Microphone).
- Inputs: Microphone (Stealth 700X Gen 3) 2 ch 48 kHz, Microphone (Logitech
  BRIO) 2 ch 48 kHz, AI Noise-Canceling Microphone (ASUS Utility) 2 ch 48 kHz,
  Microphone (Steam Streaming Microphone) 1 ch 44.1 kHz, and a `Loopback:`
  input for each of the six outputs (2 ch 48 kHz).
- Every endpoint grants exactly 480 frames at 48 kHz. cpal 0.18 opens WASAPI
  in shared mode at the device period, so **480 frames is the smallest buffer
  this machine grants** through the default build. A "Realtek ASIO" driver is
  registered, but the `asio` feature is off and cannot be built here (no
  libclang for its bindings), so no ASIO route was tried.
- No output offers more than two channels and there is no audio interface, so
  a four-port routing check and an analogue loopback are not possible here.

## A real failure the parent should look at first

The one real duplex take that ran (virtual cable, input at 44.1 kHz mono into a
48 kHz session) was discarded after about 1.7 s with the old catch-all error
"input dropout, device loss, or timestamp queue overflow". The session had
written 74 946 frames, reported an input drift of 3 830 ppm, and the output
counted one xrun during the take. The machine was loaded by other builds.

The capture code treated **any** driver error callback as fatal, and cpal
reports a WASAPI capture discontinuity as `ErrorKind::Xrun` through that
callback. So the likeliest cause is a single reported capture glitch, but that
was not confirmed. The code now keeps the first fault reason and says it in
the take's error (`recording_endpoint::InputFault`), so the next run of the
same test names the cause. Until that is run, do not read this as either a
driver quirk or an engine bug.

Two things to decide once the cause is known:

- Discarding a whole take on one reported input dropout is what
  `docs/RECORDING-ALIGNMENT.md` specifies. It keeps alignment honest and loses
  the performance. If real devices report dropouts under ordinary load, the
  better behaviour is to keep the take and fill the gap with silence from the
  packet timestamps. That is not implemented.
- A drift reading of 3 830 ppm on a virtual device is far outside any real
  clock. Check whether the period fit is being driven by timestamp jitter on
  short packets before trusting the drift readout on other devices.

## Rows

### `core-audio-recording` — still partial

Implemented here:

- Loopback inputs: every WASAPI output is listed as `Loopback: <device>` and
  opens through the untimed, timed and group capture paths
  (`crates/windfall-engine/src/recording_endpoint.rs`, call sites in
  `recording.rs`, `recording_timed.rs`, `recording_group.rs`). This is the
  "internal audio" source of the row summary on real endpoints, and it lets a
  round trip be measured without a cable.
- Fault reasons: dropout, lost device, queue overflow, unusable data, other
  driver error. Realtime-priority-denied and default-device-changed notices no
  longer end a capture.
- The acceptance harness (`session/tests/device_acceptance.rs`,
  [DEVICE-ACCEPTANCE.md](../DEVICE-ACCEPTANCE.md)).

Tests written: `recording_endpoint::tests` (five: loopback names, host rule,
mix-format range, first fault reason kept, driver errors that lose no audio);
reason assertions added to `recording::tests::converts_selected_channels_and_refuses_overflow`
and `unsigned_and_signed_32_bit_conversion_and_incomplete_frame_failure`;
`device_round_trip_alignment`, `device_loss_recovery`.

Remaining gates: a passing real round trip on any route (none exists yet); the
cause of the failure above; microphone or line capture heard and measured;
analogue loopback through an interface for converter latency; independent
clock drift on a real second device; device loss with a device actually
removed.

### `wf-count-in` — still partial

Nothing changed in the count-in code. `device_count_in_gate` is written to
establish on a real device what the synthetic tests cannot: that audio starts
a bar later, that the bursts land where they do with no count-in, that none of
the pre-roll is in the take, and that the transport and region are restored.
Not run. The row stays open until it passes on a real route.

### `wf-loop-recording` — still partial

Nothing changed in the loop code. `device_loop_takes` is written: three
complete passes and a partial one, three passes exactly three regions long,
same burst positions in every pass, one take group, one undo. Not run.

### `win-mixer-record-arm` — still partial

Nothing changed in the arm code. `device_multitrack_arms` is written: a
hardware-input arm and a post-fader arm of the stimulus track recorded
together, equal lengths, the mixer tap within one frame of the stimulus, one
take group, one undo. Not run. Post-effects mode and a second physical input
are not covered by it unless `WINDFALL_DEVICE_INPUT2` is given.

### `win-mixer-audio-io` — still partial

Loopback inputs can be saved as a track's input like any device.
`device_input_monitoring` (standalone Listen telemetry) and
`device_external_output_ports` are written. On this machine the port check can
only show a mono fold reaching port 2 and not port 1; a stereo pair on ports
3/4 needs a device with four outputs. Not run. Remaining: both of those on
real routes, monitoring heard for latency and feedback, a multichannel device.

### `core-audio-logger` — implemented, not executed

See [AUDIO-LOGGER.md](../AUDIO-LOGGER.md) for behaviour, bounds, threads and the
full test list. Engine tap and history, session, IPC, settings, three Tauri
commands, backend methods, dialog, transport button and palette action are all
present. Sources: master, up to four mixer tracks after their effects, one
input.

It is a candidate for full-row completion once its tests pass, because the
row's summary ("keeps recording audio in the background so an unplanned take
can be recovered afterwards") is what it does and it needs no hardware beyond
an output. Until the engine and session tests have actually passed it is not
done. A real-device smoke run (enable, play, add to playlist, listen) is still
owed.

### `editor-edison` — still partial

See the "Cleaning and processing tools" section of
[AUDIO-EDITOR.md](../AUDIO-EDITOR.md). Added: remove DC, de-click, noise
reduction with a captured or automatic profile, trim silence, gain, normalize
to target (peak/RMS), fade shapes, swap/mono/invert, selection stretch and
pitch, insert silence; dialog controls for all of them.

Remaining, exactly: the dialog still closes after each edit; there is no
recorder inside the editor and no tap between two effects (the logger's
post-effects track history is the nearest thing); no spectral view, regions,
scripting, convolution/blur/EQ, drag to sampler, clipping repair or loop
tuning; stretch quality and formant controls are not in the dialog.

### `wf-audio-settings` — still partial

Nothing changed in the settings code or UI, which already choose host, device,
rate, buffer and channel count and show what was asked next to what runs.
`windfall-soak` now prints a `requested:` and an `actual:` line and adds
`requested_buffer_frames` and `frames_played` to its result line.

Evidence added: the 600 s song soak above at the smallest buffer this machine
grants (480 frames; 128 was asked for). Remaining: a soak at a buffer below
480 needs a driver that grants one (ASIO with the feature built, or exclusive
or low-latency shared WASAPI, which cpal 0.18 does not offer); other devices
and operating systems; the row's "chooses the driver" has one driver to choose
in the default Windows build.

## How to run what was not run

Rust, package-scoped, after `source scripts/msvc-env.sh` and a temporary
`TS_RS_EXPORT_DIR`:

```bash
cargo test -p windfall-ipc audio_logger
cargo test -p windfall-ipc audio_edit
cargo test -p windfall-engine --lib recording
cargo test -p windfall-engine --lib audio_logger
cargo test -p windfall-engine --lib audio_clean
cargo test -p windfall-engine --lib audio_process
cargo test -p windfall-engine --lib audio_edit
cargo test -p windfall-engine --test engine audio_logger
cargo test -p windfall-desktop --lib audio_logger
cargo test -p windfall-desktop --lib audio_editor
cargo test -p windfall-desktop --lib the_burst_locator
```

Frontend, after regenerating bindings:

```bash
pnpm --dir apps/desktop test src/features/audio-logger src/features/audio-editor src/features/transport --maxWorkers=4
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
```

Real devices: [DEVICE-ACCEPTANCE.md](../DEVICE-ACCEPTANCE.md). On this machine
the useful routes are, in this order:

1. `WINDFALL_DEVICE_INPUT="Loopback: Speakers (Stealth 700X Gen 3)"` with the
   default output. Audible, quiet.
2. Output "Speakers (Steam Streaming Microphone)", input "Microphone (Steam
   Streaming Microphone)". Silent; the route that failed.
3. Output "Speakers (Steam Streaming Microphone)", input "Microphone (Logitech
   BRIO)" for `device_input_monitoring` only: silent and cannot feed back.

Run each scenario at least three times and report the spread; other builds
disturb timing.

## Where it is most likely not to compile or pass

Go to these first.

1. **`InputFault` replacing the capture fault flag.** The type of `fault`
   changed from `Arc<AtomicBool>` to `Arc<InputFault>` in `recording.rs`,
   `recording_timed.rs` and `recording_group.rs` by search and replace. It
   keeps `load(Ordering)` and `store(bool, Ordering)` so untouched call sites
   still read the same. `recording_timed.rs` and `recording_group.rs` get
   `InputFault` and `FaultReason` through `use super::*`. The error callbacks
   are now `move |error: cpal::Error| fault.report(error)`.
2. **The audio logger's Rust**, whole: `windfall-engine/src/audio_logger.rs`,
   its edits to `processor.rs`, `mixer.rs` (`Mixer::mix` has one more
   argument), `message.rs`, `controller.rs`, and
   `session/audio_logger.rs`. One edit was made after copying: `input_rate`
   now resolves the device through `recording_endpoint::find_input`, and the
   `cpal::traits` import was removed with it.
3. **The editor's Rust**, whole: `audio_edit.rs`, `audio_edit/clean.rs`,
   `audio_edit/process.rs`, the length logic in `session/audio_editor.rs`.
4. **New session tests for the editor** (`session/tests/audio_editor.rs`, the
   last four). `audio_editor_several_edits_in_a_row_undo_one_at_a_time`
   asserts the derived sample's name drops the earlier edits; that depends on
   a path-parent comparison in `audio_editor.rs` holding on Windows.
   `audio_editor_length_changing_tools_…` assumes trim-silence works on a
   four-frame clip.
5. **TypeScript against bindings that do not exist yet.** The logger and the
   editor use feature-local contracts (`features/audio-logger/types.ts`,
   `features/audio-editor/types.ts`) so they should typecheck with or without
   regenerated bindings; `audio-editor/types.ts` used to re-export from
   `@/bindings` and no longer does. Least sure: the `Select` wrappers'
   `onValueChange` typing in `logger-dialog.tsx` and `tools-panel.tsx` (copied
   from `recording-dialog.tsx`'s `Choice`), and `useAudioLoggerStore()` used
   without a selector.
6. **Existing frontend tests the new UI can disturb.** A "Logger" button was
   added to the transport bar, which is `h-12 overflow-hidden`; an action
   `audioLogger.open` was added to the registry. Any test that counts toolbar
   buttons, palette entries or actions will notice. The editor dialog gained
   two groups and one `role="status"` line; existing editor tests select by
   exact names and should not be affected.
7. **`logger.test.tsx`**: the dump test hands `receivePatch` a minimal patch
   cast with `as never`; store calls are made outside `act`.
8. **`device_external_output_ports`** was rewritten for two-port devices after
   the harness last compiled.
9. **Formatting.** Only the two new files of mine were run through rustfmt.
   The copied logger and editor files and the sed-edited capture files were
   not; `cargo fmt --check` may list them.

## Shared seam files touched

All appends, no reordering:

- `crates/windfall-ipc/src/lib.rs`: `mod audio_logger; pub use audio_logger::*;`
  at the end.
- `apps/desktop/src-tauri/src/commands.rs`: three commands
  (`audio_logger_state`, `audio_logger_configure`, `audio_logger_dump`) and
  their three handler entries.
- `apps/desktop/src-tauri/src/session/mod.rs`: `mod audio_logger;`, one
  `Inner` field, one initialiser.
- `apps/desktop/src-tauri/src/lib.rs`: not touched. `shell.rs` gained
  `session.spawn_audio_logger()?`.
- `apps/desktop/src/lib/ipc/{backend,tauri,mock}.ts`: three methods each, one
  import in `mock.ts`.
- `apps/desktop/src/lib/actions/builtin.ts`: one import, one action.
- `apps/desktop/src-tauri/src/session/tests/mod.rs`: `mod device_acceptance;`
  and `mod audio_logger;`.
- `crates/windfall-engine/tests/engine/main.rs`: `mod audio_logger;`.

Not seam files but shared with the mixer stream: `crates/windfall-engine/src/
{processor,mixer,message,controller}.rs` (logger tap), `session/{recording,
audio,library}.rs` (one line each for the logger, one helper in `library.rs`),
`settings.rs` (one field), `features/transport/transport-bar.tsx` (one button).
No file under `crates/windfall-project` or `crates/windfall-dsp` was touched.

## To regenerate

- TypeScript bindings (`scripts/gen-bindings.sh`): new exports
  `AudioLoggerInput`, `AudioLoggerSettings`, `AudioLoggerSource`,
  `AudioLoggerSourceStatus`, `AudioLoggerState`, `AudioLoggerDumpRequest`,
  `AudioEditRange`, `AudioNormalizeMode`, `AudioFadeDirection`,
  `AudioFadeShape`, `AudioEditChannels`; changed `AudioEditOperation`. The
  binding inventory check will need these added.
- The simulator WASM (`scripts/build-sim.sh`): `windfall-ipc` changed. The
  browser backend's logger and editor refusals are in `mock.ts`, not in WASM.

Nothing generated is in the commits.
