# Rack voice tools: held echo, channel filter, layers, sample pre-processing

Added 2026-10-10 on top of
[the channel voice seam](integration/seams/channel-voice.md). Where the two
disagree, this file is the newer one: in particular, echo of held notes is no
longer unsupported. Status and evidence are in
[the QA hand-off](qa/2026-10-10-rack-sampler.md); at the time of writing the
code had not been built or run in full.

## Note echo

Echo works for every note source: pattern notes, arpeggiator output, and
notes held by hand from the UI keyboard or a MIDI device.

A held note has no length when it starts. Its repeats start on time anyway,
and each one is released as long after key-up as it started after key-down,
so every echo lasts as long as the note did. A key let go before its first
echo still gets whole echoes.

Echoes sound alongside the note they repeat. They count toward the channel's
voice limit and give way first: the oldest echo is released when a new note
needs a voice, and a new echo is skipped when only played notes are
sounding. A channel limited to one voice, or in mono legato, stays one
voice: there an echo takes the note's place.

Stop and MIDI panic silence queued and sounding echoes. A seek discards
queued echoes.

Each channel has room for 128 pending echoes and 32 held notes with echoes.
A note whose echoes do not fit plays dry, and
`Controller::rejected_echoes()` counts it. The count is not shown in the
desktop UI yet.

## Channel filter, envelopes and LFOs

These are saved in `channel.voice.envelopes` and apply to sampler channels.

- **Filter** (`filterSettings`): on/off, low-pass, high-pass, band-pass or
  notch, cutoff 20 Hz to 20 kHz, resonance 0 to 1. Resonance 0 has no peak
  (Q 0.707); each fifth of the range doubles Q, up to about 22.6. Band-pass
  passes its centre at unity.
- **Modulation targets**: cutoff is target X and resonance is target Y.
- **Envelopes**: cutoff (`filter`, ±8 octaves), resonance (`resonance`, ±1 of
  the range), pitch (±2400 cents) and pan (±1). The volume envelope is the
  sampler's own envelope section.
- **LFOs** (`lfos`): one each for volume, pan, pitch, cutoff and resonance,
  with sine, triangle or square shape, 0.01 to 30 Hz, depth −1 to 1. Full
  depth is a tremolo down to silence, full pan travel, 2400 cents, 8 octaves
  or the whole resonance range. The volume LFO only ever lowers the level.
  The older assignable LFO (`lfo`) remains and can also target volume and
  resonance.

With the filter off and no cutoff or resonance modulation, a voice is not
filtered and sounds exactly as before. `resonance`, `lfos` and
`filterSettings` are left out of a project file while at their defaults, so
older files save back unchanged.

On an instrument channel the pan, pitch and cutoff targets reach the
instrument through note expression. The filter, the resonance target and the
volume LFO do not apply and the panel hides them.

In the panel, one knob drag is one undo step.

## Layer channels

A layer channel makes no sound. Every note it is given, from a pattern or
played by hand, plays on each of its child channels.

- **Add layer channel** is in the rack's add menus and the command palette.
  The layer has no mixer track of its own.
- Any channel can be made a layer in its settings, under **Layer**. Its own
  sample or instrument is then silent.
- Up to 16 children. A layer can contain layers; a note reaches each
  sounding channel once, and at most 64 channels.
- A layer cannot play itself, a missing channel or a loop of layers. Such an
  edit is refused and nothing changes. A project file that contains one does
  not load.
- Deleting a channel takes it out of every layer in the same undo step.
- Muting a layer stops it passing notes on. Muting a child silences that
  child. Soloing a layer keeps its children audible.
- The layer's own swing, gate and shift shape its notes. Its arpeggiator,
  echo and polyphony settings are not applied; each child applies its own.

Saved as `channel.voice.layer = { enabled, children }`, left out of the file
on channels that are not layers.

## Sample pre-processing

**Pre-process sample** in a sampler's settings renders a new sample from the
current one and gives it to the channel in one undo step. The original
sample stays in the project and its file is never written.

Treatments, applied in this order:

1. Trim to the played region. The trim is then reset to the whole sample.
2. Reverse the audio. The reverse switch is not changed, so the sample then
   plays the other way round. The region and the loop stay on their audio.
3. Remove DC offset, per channel.
4. Invert polarity.
5. Swap left and right. Refused for a mono sample.
6. Fade in and fade out over the played region, equal-power, up to 60 s.
7. Crossfade the loop seam, for a forward loop only, up to half the loop.
   With audio before the loop, the end of the loop fades into the audio that
   leads into it and the loop keeps its length. A loop that starts at the
   first frame is shortened by the blend, and the audio that followed it
   fades into its start. The playback loop crossfade is set to zero.
8. Normalize: one gain for every channel, peak to full scale.

The reverse switch and the playback loop crossfade remain what they were:
settings that change how an unchanged sample is read.

The new sample is a Float32 WAV under `recordings/Sample edits` in the app
settings folder, listed in the project as an external sample. Undo and redo
keep the file. Nothing collects these files yet. Samples longer than 64 MiB
of audio, or with more than two channels, are refused.

If the project, the sampler's settings or the source audio change while the
job runs, the result is dropped, its file is deleted and an error says so.
The browser preview cannot render audio and refuses with a reason.

## Fit to tempo

The stretch section has a beats field and **Fit to tempo**. Left empty, the
field uses the suggested beat count, the one nearest the region's own length
at the project tempo.

- In **Independent** mode it sets the duration ratio so the played region
  lasts that many beats, and pitch stays. Apply stretch then prepares it.
  The ratio must be within 0.25 to 4.
- In **Tape** mode it sets the tune so the region lasts that many beats at
  its root key. Pitch changes with speed. The tune must be within ±48
  semitones.

It uses the tempo stored in the project settings, not tempo automation.
