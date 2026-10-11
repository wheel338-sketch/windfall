# Real-device recording acceptance

The recording, monitoring, count-in, loop-take and port-routing code is covered
by tests that use fake captures and a hand-run processor. Those tests cannot
say what a real driver does. This harness drives the same session calls the
app makes against the audio endpoints of the machine it runs on, and writes
what it measured as JSON.

It is opt-in. Every device test is `#[ignore]` and needs an evidence folder, so
an ordinary `cargo test` never opens a device or makes a sound.

Source: `apps/desktop/src-tauri/src/session/tests/device_acceptance.rs`.

## What it does

Each test starts a real `Engine` on the chosen output, builds a `Session` on
it, and puts a stimulus on the playlist at tick 0: a WAV at the device's rate
holding a 4 ms burst of fixed pseudo-random noise every 0.5 s, the first at
0.25 s. The default amplitude is 0.05 (about −26 dBFS). Takes are 12 s unless
told otherwise.

The test then calls `Session::recording_start` with playback synchronized,
waits, calls `recording_stop`, and decodes the WAV of the clip the take became.
Each burst is located by cross-correlation within ±200 ms of where it was
played, with a parabola through the peak for a position between frames. From
those positions it reports:

- **offset**: mean, minimum and maximum of how late the bursts are in the
  take, in frames and milliseconds. This is the alignment error of the route.
- **drift**: the slope of the offsets over the take, in parts per million.
- **jitter**: the standard deviation of the offsets around that slope.
- **largest step** between two bursts in a row. A dropped or repeated block
  shows up here.
- how many bursts were found and how many were not.

Next to the measurements every result records: the commit and whether the
tree was dirty, UTC time, OS version and architecture, build profile, what was
asked of the output (host, device, rate, buffer, channels) and what it gave,
the input device, the alignment status the session reported (input rate, input
and output latency, drift, whether timestamps came from the driver, trimmed
pre-roll), engine xruns during the take, and progress samples while recording.

The result file is written before any check is asserted, so a failed run
leaves its evidence. `checks` lists each pass/fail with its wording.

`the_burst_locator_finds_a_shifted_and_drifting_train_exactly` is the one test
here that is not ignored: it checks the measurement itself on audio made by
hand (a train 37 frames late running 100 ppm slow, and silence).

## Scenarios

| Test | What it establishes |
| --- | --- |
| `device_inventory` | Hosts, outputs with rates/buffer range/channel counts, inputs. Every WASAPI output is also listed as a `Loopback:` input. |
| `device_round_trip_alignment` | One synchronized take: all bursts found, jitter ≤ 1 ms, no step > 1 ms, drift within 50 ppm, transport/region restored, undo/redo of the take. Then a second take with the measured mean offset applied as the manual recording offset must land within 1 ms. |
| `device_count_in_gate` | A take with no count-in and one with a one-bar count-in. Audio starts about one bar later, the bursts land within 1 ms of the take without a count-in, nothing of the pre-roll is in the take, pre-roll was trimmed, transport restored. |
| `device_loop_takes` | Loops one bar for 3.6 passes and keeps all. Three complete passes and a final partial one, three passes together exactly three regions long, every pass on the region's first tick, earlier passes muted, bursts at the same place in every pass within 1 ms, one saved take group, one undo removes all. |
| `device_multitrack_arms` | Arms a track on the hardware input (and a second input if given) and the stimulus's own track after its fader. Each becomes its own clip of equal length, the mixer tap holds the stimulus within one frame, the hardware inputs hold every burst, one take group with a lane per track, one undo removes all. |
| `device_input_monitoring` | Standalone Listen on a track for a few seconds: stays active with no error, input rate reported, no underrun once settled, no dropped monitor frames, stop ends it. |
| `device_external_output_ports` | Sends the stimulus track to hardware ports of its own with the ordinary output replaced, and records the device's loopback of those ports and of the others. With four or more ports it uses ports 3/4; on a stereo device it folds to mono on port 2. |
| `device_loss_recovery` | Records, then waits while something outside takes the output device away and gives it back. Reports the status timeline, outage length and the take's error; the take must be refused with no clip and no file left, and a new take on the reopened device must work. |

## Running it

Windows, Git Bash, from the repository root:

```bash
source scripts/msvc-env.sh
export TS_RS_EXPORT_DIR="$(cygpath -w "$(mktemp -d)")"
export WINDFALL_DEVICE_EVIDENCE="$(cygpath -w docs/evidence/recording-YYYY-MM-DD)"
export WINDFALL_DEVICE_LABEL=loopback
export WINDFALL_DEVICE_INPUT="Loopback: Speakers (Your Device)"
cargo test -p windfall-desktop --lib device_round_trip_alignment -- --ignored --test-threads=1
```

Run one test at a time with `--test-threads=1`: two engines on one device
disturb each other. Results are named `<scenario>-<label>.json`.

| Variable | Meaning |
| --- | --- |
| `WINDFALL_DEVICE_EVIDENCE` | Folder for the results. Required. |
| `WINDFALL_DEVICE_LABEL` | Name of the route in file names. Default `default`. |
| `WINDFALL_DEVICE_HOST` | Audio host. Default: the system's. |
| `WINDFALL_DEVICE_OUTPUT` | Output device. Default: the system's. |
| `WINDFALL_DEVICE_INPUT` | Input device, or `Loopback: <output device>`. Required by all but the inventory. |
| `WINDFALL_DEVICE_INPUT2` | A second input for the multitrack check. |
| `WINDFALL_DEVICE_RATE`, `_BUFFER`, `_CHANNELS` | What to ask of the output. |
| `WINDFALL_DEVICE_SECONDS` | Take length. Default 12. |
| `WINDFALL_DEVICE_GAIN` | Burst amplitude. Default 0.05. |
| `WINDFALL_DEVICE_MIN_SCORE` | Lowest normalised correlation that counts as a burst. Default 0.5; an acoustic path may need less. |
| `WINDFALL_DEVICE_LOSS_SECS` | How long the device-loss check waits. Default 40. |

### Routes and what each one proves

- **Loopback of the output** (`Loopback: <device>`): the system mixer's copy
  of what the engine plays, on the output's own clock. It is audible. It
  proves the engine's clock, gate, take and file handling on a real WASAPI
  stream. It does not include a converter, so it says nothing about analogue
  latency. Other applications playing on that device end up in the take; the
  correlation tolerates them, but quiet the machine first.
- **A virtual cable** (an output and an input of one virtual driver): silent,
  and a real duplex pair of endpoints. Same limits as loopback.
- **A microphone hearing the speakers**: a real independent input clock, so
  drift correction is exercised. The offset includes the air path and the
  result depends on the room; lower `WINDFALL_DEVICE_MIN_SCORE` if needed and
  say so in the record.
- **A cable from an interface output to its input**: the only route that
  measures converter round-trip latency and closes the analogue gate.

For monitoring, choose a route that cannot feed back: the input must not hear
the output. Never monitor a device's own loopback.

### Device loss

`device_loss_recovery` writes `device-loss-ready-<label>` into the evidence
folder once it is recording. Take the output device away then (unplug it, or
disable the endpoint in the system's sound settings) and give it back a few
seconds later. The test polls the engine status every 50 ms and records each
change. It ends two seconds after the stream is running again, or after
`WINDFALL_DEVICE_LOSS_SECS`.

## Reading a result

A systematic offset on a loopback or cable route is the error of the driver's
reported latency, not a fault of the take: the calibrated second take shows
whether the manual recording offset removes it. Jitter, steps and drift are
the engine's and the driver's own. Record skipped and failed checks as such.
A requested buffer the driver ignored is visible as `requested.bufferFrames`
next to `actual.bufferFrames`.

## Loopback inputs

`crates/windfall-engine/src/recording_endpoint.rs` lists every WASAPI output
as an input named `Loopback: <device>` and opens it through the ordinary
timestamped capture path. The name is the whole contract: recording sources
and saved mixer input routes keep their host and device strings, so the
recording dialog, armed tracks, monitoring and the audio logger can all use a
loopback with no model change. A loopback has one format, the device's shared
mix format. It delivers nothing while nothing plays on the device, so a
loopback of an idle endpoint ends a take with the missing-audio error rather
than inventing silence at the wrong time. Other hosts offer no loopback.

## Capture failures say why

A capture's driver callbacks and its worker share an `InputFault`. It keeps
the first reason raised: a dropout the driver reported, a lost device, a full
capture queue, unusable audio data, or another driver error. The take's error
is that reason as a sentence followed by what became of the take. Two driver
notices lose no audio and no longer end a capture: realtime priority denied,
and a change of the system's default device.
