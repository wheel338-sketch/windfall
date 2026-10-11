# Audio logger

The audio logger keeps the last minutes of audio in memory while you work, so
something played without recording can still be put on the playlist. It is on
by default and logs the master output. Nothing is written to disk until a
history is added to the playlist.

Open it from **Logger** in the transport bar or **Audio logger…** in the
command palette.

## Sources

- **Master output**: the final stereo mix as it left the engine, metronome
  and output gain included.
- **Mixer tracks, after their effects** and before their fader: up to four
  tracks. This is the tap the editor workflow uses: log the track that
  carries an instrument or an input, add the part you want to the playlist,
  and open that clip in the audio editor.
- **An audio input**: one hardware input (or a `Loopback:` input, see
  [DEVICE-ACCEPTANCE.md](DEVICE-ACCEPTANCE.md)) captured continuously at its
  own rate, whether or not anything is recording.

Each source has its own history. The dialog shows, per source, whether audio
is arriving, how much is held of how much it can hold, its sample rate, its
memory, frames dropped, and any error.

## Adding a history to the playlist

**Add to playlist** takes the last 10, 30, 60 or 120 seconds of a source, or
everything it holds, writes a Float32 WAV into the app's recordings folder on
a worker thread, and adds it as an ordinary audio clip at the playhead on a
new playlist track. It is one undo step; undo and redo behave as for any
imported clip, and Save copies the sample through the normal sample pipeline.
The native call can also place the clip at a chosen tick and on an existing
playlist track. The logger keeps running while a history is added.

A request is refused, with the document unchanged and no file left behind,
when a recording take is active or finishing, when the history is empty, when
the seconds asked for are not a positive number, when the playlist track no
longer exists, or when another project was opened while the file was being
written.

## Bounds

- History length: 30 to 600 seconds, 120 by default. Changing the length or
  the sources starts every history over.
- One history never holds more than 256 MiB, whatever length was asked for: a
  long history at a high sample rate is shortened to fit, and the dialog shows
  the capacity it came to. Stereo Float32 at 48 kHz is about 23 MiB a minute.
- Each source has a transfer queue of two seconds between the audio thread
  and the worker.
- At most one master history, four track histories and one input history.

The oldest audio is overwritten.

## Threads

- The audio callback owns the logger's tap. For each logged source it copies
  the block it just made into a preallocated single-producer queue. A full
  queue drops what does not fit and counts it. Nothing else happens there: no
  allocation, free, lock, wait or IO, whether the logger is being turned on,
  turned off or is overflowing. The tap reaches the processor as a message and
  comes back as garbage for the control side to drop, like every other heap
  object the audio thread is handed.
- A worker owned by the logger drains the queues into the histories every few
  milliseconds.
- Reading the state and copying out a history share one mutex with that
  worker. The audio callback never takes it.
- The input is captured by the ordinary untimed capture: its driver callback
  only pushes frames into a bounded queue and its own worker writes them into
  the input's history.
- A watch thread looks after the logger twice a second: it takes up a stream
  that was replaced and reopens an input that made way for a take.

Offline renders and exports carry no logger and are not changed by one.

## Device changes, recording and failures

- Reopening the audio output clears every history and starts again at the new
  stream's rate. The old tap is retired, not leaked.
- A recording take closes the logged input before opening its own capture, so
  the device is free. The input is reopened onto the same history when the
  take is over, kept or cancelled.
- An input that cannot be opened, or that fails while running, becomes an
  error on that source. Its history stays readable, playback and recording are
  not disturbed, and it is not retried until the logger is configured again.
- A mixer track id that names no track of the open project logs nothing and
  shows as not receiving audio.

## Settings

Enabled, length, master, tracks and input are app settings, stored in the
settings file with defaults for every field, so a file written before the
logger existed loads with the logger on, 120 seconds, master only. They are
not part of a project and not in undo history.

## Browser preview

The browser backend has no audio history. It reports the logger as off with a
message saying it needs the desktop app, and refuses configuration and adding
to the playlist. It never fabricates audio.

## Source

- `crates/windfall-engine/src/audio_logger.rs`: history ring, tap, worker,
  `Controller::start_audio_logger` / `reattach_audio_logger` /
  `stop_audio_logger`, `input_rate`.
- `crates/windfall-engine/src/{processor,mixer,message,controller}.rs`: the
  tap's message, its retirement and the two places it is fed.
- `crates/windfall-ipc/src/audio_logger.rs`: settings, state, sources, request.
- `apps/desktop/src-tauri/src/session/audio_logger.rs`: settings, input
  capture, watch thread, `audio_logger_state` / `_configure` / `_dump`.
- `apps/desktop/src/features/audio-logger/`: store, dialog, IPC contract.

## Verification status

Written, not executed. Nothing in this feature has been compiled or run: the
branch was finished under an instruction not to build or test. The tests exist
and are registered:

- `crates/windfall-engine/src/audio_logger.rs` unit tests and
  `crates/windfall-engine/tests/engine/audio_logger.rs`: ring order across
  wraps, memory bound, counted overflow when nobody drains, a track logged
  after its effects and before its fader, a track followed across plans and
  stopped when removed, resize and retirement of old taps, no allocation in
  `Processor::process` with the logger on, offline render unaffected.
- `apps/desktop/src-tauri/src/session/tests/audio_logger.rs`: exact master and
  track dumps against what the hand-run processor returned, one undo step and
  save/reopen, refusals, a project opened during a dump, a fake input at its
  own rate, input failure as an error state, the input making way for a take
  and coming back, reconfiguration, settings round trip and an older settings
  file.
- `crates/windfall-ipc/src/audio_logger.rs`: settings defaults and tagged
  source JSON.
- `apps/desktop/src/features/audio-logger/logger.test.tsx`: formatting, the
  browser message, opening from the command registry, settings requests, dump
  requests, native errors, an empty history.

No real-device run exists. To exercise the logger on a real output, build a
session on a real `Engine` as `device_acceptance.rs` does, call
`Session::audio_logger_configure`, play, and call `Session::audio_logger_dump`.

Not implemented: a tap at an arbitrary position between two effects of a
track (the tap is after the whole chain), more than one logged input, logging
to disk, and placing a dumped clip so that it lines up with where it was heard
on the timeline.
