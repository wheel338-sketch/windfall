# Windfall CLAP export target

`crates/windfall-plugin` is a separate `cdylib`/`rlib` target exporting
`clap_entry` and plugin identity `org.windfall.program`. It embeds the real
Windfall engine without opening an audio or MIDI device. It is source-only at
this checkpoint: **not built, tested or accepted by an external host**.

The host supplies integral sample rates from 8–384 kHz and blocks up to 8192
frames. Each process call is split into at most 256-frame engine quanta and at
sample offsets of MIDI/transport events. MIDI channel 1 selects rack row 1,
channel 16 selects row 16; absent rows stay silent. The advertised note dialect
is raw MIDI, with note on/off and channel all-notes-off. There is no advertised
polyphonic expression or outgoing MIDI support yet.

Sixteen stereo output ports expose actual engine external mixer outputs.
The master uses output 1 by default. Saved `externalOutput` routes select the
other ports, retain the existing mixer routing semantics and compensation, and
are refused if they exceed 32 channels. Output buffers are Float32 only.
Activation caches engine latency; the CLAP latency extension returns it without
calling the controller from the callback.

Host beat position, tempo and play/stop drive the engine song sequencer directly.
Tempo changes bend the clock without restarting held notes; discontinuities
seek using the engine's existing voice-release behaviour. Negative preroll stays
stopped until beat zero. Incoming transport events take effect at their sample
offset; constant-tempo spans advance from the host anchor. A nonzero CLAP
`tempo_inc`, internal tempo automation and navigation markers are explicitly
refused pending a host-clock arbitration design. Meter is currently the saved
project's meter, not an override from the external host.

The state extension saves schema 1 JSON containing the normal checked/migrated
project file as a string and decoded user-owned PCM assets. Every project asset
must be present exactly once. State streams handle partial IO and propagate
errors; malformed or oversized state leaves the previous program untouched.
The bound is 64 MiB of encoded state, and decoded asset validation rejects
nonfinite PCM and invalid layouts. State is decoded and compiled off audio.
`Program::encode` lets a future desktop export command produce the same packet.
The default program is empty; the host must load a prepared packet to hear a song.
An external project-file export command and editor are not implemented yet.

Native plugin bindings and hardware recording routes are refused rather than
silently substituted. Active state loads are refused until an export-owned
publication/capture fence is added. Reset currently uses the engine's stop/fade
and seek; strict CLAP reset flushing of every effect tail/delay remains an open
acceptance gate. No VST3 export entry point or package installer is present.
These are remaining software gates, in addition to actual acceptance in another
DAW. This target cannot close `fmt-host-fl-as-plugin` yet.

Tests in `src/clap/tests.rs` are **written, not executed**: host tempo onset at
independently calculated exact frames; mid-note-clock tempo bending; sample-rate
changes; state-restored identical multibus PCM; actual CLAP factory/lifecycle,
partial state IO, sample-offset MIDI and callback allocation/free accounting.

The boundary follows the public CLAP [process](https://github.com/free-audio/clap/blob/main/include/clap/process.h),
[events](https://github.com/free-audio/clap/blob/main/include/clap/events.h) and
[state](https://github.com/free-audio/clap/blob/main/include/clap/ext/state.h)
headers. `clap-sys` 0.5 is MIT OR Apache-2.0, already used by the native fixtures;
no proprietary ABI header or content is included.
