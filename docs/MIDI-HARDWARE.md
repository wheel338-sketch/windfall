# MIDI hardware

Windfall's native desktop app can audition a channel from one MIDI input and
optionally forward those live notes to one MIDI output. Standard MIDI File
import/export remains separate in `windfall-midi` and keeps its existing behavior.

## Using it

1. Open **Options → Settings → MIDI hardware**.
2. Select a MIDI input and an input channel, or **All channels**.
3. Select an explicit **Audition destination** from the channel rack. This plays
   the existing sampler, built-in synth, or supported hosted CLAP instrument
   through its usual mixer routing. The audio output must be running.
4. Optionally select **Live MIDI output** and its channel (1–16). Only accepted
   live input notes are forwarded; patterns and the playlist are not sent.

Device and MIDI-channel settings are remembered in `settings.json`. Ports are
identified by the backend's opaque IDs, so duplicate display names do not select
one another. A missing saved device remains unavailable; Windfall never substitutes
another port. **Refresh ports** updates the list; **Reconnect devices** reapplies
the settings. Disconnected ports do not automatically reopen.

The audition destination is runtime-only. It neither changes the project nor
adds an undo step, and is cleared on project replacement or destination removal.
Undoing removal does not silently rearm it. Changing destinations releases the old
notes. Device and destination changes are refused during an owned audio recording
take; playing notes and using **Panic MIDI** remain available.

**Stop** and **Panic MIDI** invalidate queued hardware notes and release the
hardware voices. Device changes, detected removal, audio-stream replacement,
channel edits, and queue overflow also invalidate the held-note set. New notes
received afterward may audition normally. An output still available receives
exact note-offs plus sustain-off, all-notes-off and all-sound-off messages before
its old connection is closed. A physically disconnected or failed output cannot
acknowledge cleanup messages.

## Supported events and limits

- Note-on, note-off and note-on with velocity zero are accepted. Velocity is
  normalized to the existing engine's 0–1 scale; release velocity is ignored.
- Sustain (CC64) holds released keys until the pedal comes up. Reset controllers
  (CC121) releases sustain. CC120/CC123 on an accepted input channel panic the
  entire live route. Other controllers are ignored.
- Accepted input MIDI channels feed one project channel. A key remains held until
  all contributing input channels release it. Repeated keys retrigger; output
  retriggers first send a balancing note-off. The existing instrument model still
  holds one note per key, with the latest sequenced/UI/hardware note owning it.
- Samplers retain their normal one-shot/envelope behavior: a one-shot with no
  envelope finishes its sample on note-off. Panic fades it immediately.
- Hardware input events enter at the next audio buffer boundary. There is a
  2 ms worker service wait plus OS scheduling, device, and audio-buffer latency;
  input timestamps are not mapped to the sample clock. Native enumeration/opening
  and output-driver calls may delay the control worker. Removal is checked every
  500 ms of worker operation and on explicit refresh, rather than through a
  guaranteed instantaneous OS notification.
- Output is live-note forwarding with sustain already translated into note-offs.
  It requires a valid audition destination and engine connection. No sequenced
  output, clock, transport sync, SysEx, pitch bend, pressure, program changes,
  MIDI 2.0, controller mapping/scripting, note recording, or MIDI learn is provided.
- Browser simulation exposes the desktop-only limit and no invented native ports.
  No Web MIDI permission or hardware access is requested.
- Existing VST3 hosting restrictions are unchanged. This feature does not capture
  active VST3 state or enable a previously unsupported plugin role.

## Ownership and realtime safety

`windfall-engine::midi_hardware` owns native MIDI control independently of the
SMF crate. `Runtime` creates one named worker; all native input/output handles,
enumeration, opening, closing, and output sends belong to that worker. A `Ports`
seam supplies deterministic fake devices in tests without constructing real ports.
The shell keeps only a control handle and a weak-session audition destination.

The input callback parses only fixed-size supported messages, reads an atomic
epoch, and pushes a `Copy` packet into a preallocated 1,024-entry `rtrb` SPSC queue.
It never accesses the project, allocates, acquires a Windfall lock, or does file I/O.
Overflow increments an atomic dropped count and advances the panic epoch. The
native MIDI library and OS driver have their own internals; Windfall's callback
guarantee does not assert that those third-party internals are lock-free.

The worker tracks keys/sustain in fixed arrays and services at most 1,024 packets
per turn. Native control requests use a bounded 16-entry queue. Session delivery
checks the epoch and destination under `state`, then calls the controller. A
destination request must match both the session's document generation and the
document revision; a replacement can reuse channel IDs and revision zero safely.
Device configuration retains recording exclusion before taking any document lock.

Hardware messages use a bounded controller enqueue that refuses a full queue or
pending general-control backlog. Hardware traffic never grows that backlog. A
refusal invalidates the whole hardware held-note set, so a lost note-off cannot
leave its preceding note held. Transport/stream invalidation is serialized with
controller message enqueue. Every hardware message carries its capture epoch and
is checked again by the audio processor.

The audio callback reads the panic epoch even when normal-message processing is
blocked by garbage-queue pressure. It fades hardware sampler voices and releases
hardware instrument keys separately from UI notes. Heap-owning plans, samples,
and plugin instances retain the existing controller-side retirement path. MIDI
does not prepare DSP, enumerate devices, send output, lock, allocate, or perform
file I/O on that callback. Instrument preparation and existing native plugin
role/token/revision ownership remain unchanged. Sustain, held notes, device state,
and routing are never written into the project/history per input callback.

## IPC and integration

`windfall-ipc` exports `MidiPort`, `MidiHardwareSettings`, and `MidiHardwareState`.
The Tauri commands and `Backend` methods are:

- `midi_hardware_state` / `midiHardwareState`: settings, enumerated ports,
  connection/error/drop status, runtime target, and document generation.
- `midi_hardware_refresh` / `midiHardwareRefresh`: refresh ports off the UI/audio
  threads and return the state.
- `midi_hardware_configure` / `midiHardwareConfigure`: apply and persist complete
  device settings. A port-open failure is returned in state while preserving the
  requested settings for a later reconnect.
- `midi_hardware_target` / `midiHardwareTarget`: explicit channel or null, plus
  `generation` and `revision`; refuses stale requests.
- `midi_hardware_panic` / `midiHardwarePanic`: queue-independent invalidation.

The settings panel polls status while open, serializes its changes, ignores old
project/read responses, and leaves panic available while configuration is pending.
The shell starts the worker once. Standard MIDI import/export IPC is unchanged.

The integration owner must regenerate TypeScript bindings using
`scripts/gen-bindings.sh` after combining branches. This feature intentionally
does not commit generated bindings or a branch-only wasm artifact; no project or
simulation command/model change requires a wasm rebuild for this feature itself.

## Verification

Windows checks on 2026-10-07 use Git Bash with `source scripts/msvc-env.sh` and a
task-specific `TS_RS_EXPORT_DIR` outside the repository. Tests use fake MIDI
ports/events and manually driven audio processors, plus Windfall's native CLAP
test instrument. This is **not physical MIDI hardware verification**. macOS,
Linux, USB/Bluetooth/virtual drivers, unplug/replug on real equipment, long-run
latency, and exclusive device access still require testing on those systems.

Checks exercised:

- `cargo test -p windfall-engine`: existing engine behavior and hardware parsing,
  channel filtering, sustain, repeated/multichannel keys, panic, full input/audio
  queues, disconnect/configure failure, output-send failure, one-worker ownership,
  stream changes, and allocator counts for input and audio callback processing.
- `cargo test -p windfall-desktop midi_hardware --lib`: sampler/synth/native CLAP
  audition and note-off/panic, unchanged history/project state, generation/removed
  targets, recording exclusion, and settings persistence.
- `cargo test -p windfall-midi` and the desktop `session::tests::midi` tests:
  SMF parser/writer/import/export regression coverage.
- `pnpm test` with the settings and existing MIDI UI/mock tests: device IDs with
  duplicate names, desktop/browser limits, guarded destination requests, live
  input/output configuration, and pending/refused configuration with panic usable.
- `cargo fmt --all -- --check`, targeted `cargo clippy --all-targets -- -D warnings`,
  desktop TypeScript checking and ESLint.
- T3 collaborative preview: real browser settings flow at 1280×800 and 960×600,
  disabled native controls in simulation, usable panic, scrollable settings, and
  no horizontal dialog overflow. Native Tauri device UI with physical ports was
  not exercised.

## Primary references and dependency license

The native dependency is `midir` 0.11.0 (MIT, compatible with GPL-3.0-or-later).
The queue uses the existing `rtrb` dependency (MIT OR Apache-2.0).

- [midir upstream repository, backends and license](https://github.com/Boddlnagg/midir)
- [Input callback and connection lifetime](https://docs.rs/midir/latest/midir/struct.MidiInput.html)
- [Opaque port identity](https://docs.rs/midir/latest/midir/struct.MidiInputPort.html)
- [Output connection and send API](https://docs.rs/midir/latest/midir/struct.MidiOutputConnection.html)
- [MIDI Association MIDI 1.0 message summary](https://midi.org/summary-of-midi-1-0-messages)

## Additions of 2026-10-10 (written, not built or run)

The sections above describe the first delivery, which was built and tested.
What follows was written during a build freeze: none of it has been
compiled, run, or tried on a MIDI port. Where it contradicts the limits
listed above, the limits above describe what is verified.

- **Several inputs.** `extraInputs` opens up to seven more ports beside the
  main one. Every channel message and MIDI clock reaches the session with
  the input it came from and its capture time. Sysex, time code and active
  sensing are not read.
- **Controller links.** See `docs/CONTROLLER-LINKS.md`. Right-click a knob
  or fader, or use the command palette: Multi-link controllers, Link last
  tweaked control, Toggle controller pickup mode.
- **Profiles.** Mackie Control compatible, Korg nanoKONTROL2 and a standard
  volume/pan surface. None was checked against the physical device.
- **Note recording.** With "Record played notes into the pattern" armed,
  notes played while the transport runs are written when it stops, as one
  undo step. The typing keyboard and touch controllers are recorded too.
  The time of a note is the playhead at the moment the input callback took
  it, carried on from the last published playhead at the tempo, moved by
  the recording offset. The output device's latency is not subtracted.
- **Note log.** Everything played is kept for thirty minutes or 16,384
  notes. Dumping it to a pattern does not empty it.
- **Automation recording.** In song mode, with Record automation armed,
  control movements by mouse or controller are written when the transport
  stops, as one undo step. A control already driven by an automation clip
  keeps following its old curve until then.
- **MIDI clock.** Send: start, or song position and continue, 24 pulses a
  quarter note, stop. It follows the published playhead, so it lags the
  audio by about one buffer and is not latency compensated. Follow: start,
  stop, continue and tempo of a device's clock; a followed tempo change is
  a project edit.
- **MIDI output channel.** A rack instrument, "MIDI output", sends its
  notes, program and bank, pitch wheel and eight controllers. The audio
  thread writes fixed-size messages to a preallocated queue and the MIDI
  worker sends them at the time their audio is heard. It sends nothing
  during an offline render. It cannot play another plugin.

The opt-in real-port check is
`cargo test -p windfall-engine real_ports -- --ignored --nocapture`. It
lists the machine's ports and sends one second of clock to the first
output. It has not been run.
