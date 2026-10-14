# Windfall 1.0.0

Windfall is a free, open-source digital audio workstation for making music
with patterns, a piano roll and a song playlist, in the style of FL Studio.
It is a native desktop app: the audio engine is written in Rust, the window
is Tauri 2 and the interface is React. Windfall is not affiliated with
Image-Line and contains no FL Studio code, samples, presets or artwork.

This is the first stable release. The earlier `v0.1.0-alpha.1` release is
unchanged.

## What it does

- **Channel rack and step sequencer.** Sampler and instrument channels, step
  buttons, pattern length and swing, channel groups, a note preview per row
  and a step graph for velocity, pan and other per-step values.
- **Piano roll.** Draw, paint, select, slice, mute and erase tools; quantize,
  legato, chop, glue, arpeggiate, strum, flam, flip and velocity tools;
  scale guidance and chord and scale stamps; per-note expression curves and
  ghost notes from other channels.
- **Playlist.** Pattern, audio and automation clips on tracks; meter changes
  and markers; time selection playback and export; track groups; alternative
  arrangements; audio clip time-stretch and pitch.
- **Mixer.** Insert tracks with effect chains, sends, sidechains, routing
  between tracks, track presets, latency compensation and disk recording.
- **Built-in instruments.** Subtractive, FM and phase-modulation synths;
  additive, spectral and resynthesis voices; physical models (plucked and
  bowed strings, bass, membranes); drum machines and kick designers;
  samplers, a multisampler, SoundFont playback, slicers and a granular
  player; several vintage-style polysynths; a speech voice and MIDI output.
- **Built-in effects.** EQs and filters; compressors, limiters, multiband
  dynamics and transient shapers; reverbs, rooms and a convolver; delays
  and echo banks; chorus, flanger, phaser and other modulation; distortion,
  saturation, waveshaping and lo-fi; pitch shifting, pitch correction,
  frequency shifting and a vocoder; stereo and utility tools; analyzers, a
  tuner and a spectrogram.
- **Plugin hosting.** CLAP and VST3 instruments and effects on Windows, with
  a plugin manager, parameter automation, saved state, and an isolated
  helper process for plugin audio.
- **Recording.** Audio recording from a microphone or line input to clips,
  multitrack and loop recording, take groups and comping, and live MIDI
  input.
- **Export.** WAV, FLAC, OGG, MP3, WavPack and M4A (AAC), of the song, a pattern or a time
  selection, as a mix, as mixer stems or one file per playlist track. MIDI
  file import and export. Portable project archives (ZIP with audio).
- **Stem separation.** Split an audio clip into four stems with a model and
  ONNX Runtime library that you supply; Windfall ships neither and makes no
  network request.
- **Scripting.** Windfall Script, a small built-in language for editing a
  project, and an offline assistant that proposes edits you approve. Each
  applied batch is one undo step.
- **FL Studio project import.** `.flp` files are converted with notes,
  patterns, clips, automation, mixer routing and the device settings that
  have a Windfall counterpart, after a review of what was and was not
  brought over.
- **Languages.** English and Spanish, chosen in Settings.

## System requirements

- Windows 10 or Windows 11, 64-bit (x64), with the Microsoft Edge WebView2
  runtime. Windows 11 includes it; on Windows 10 the installer downloads it
  if it is missing, which needs an internet connection.
- macOS and Linux builds are not published for this release.

## Installing

Download `Windfall_1.0.0_x64-setup.exe` from the release and run it.
Uninstalling leaves your project files where you saved them.

## Known limitations

- **The installer is not code-signed.** Windows SmartScreen will warn that
  the publisher is unknown; choose "More info" and then "Run anyway" if you
  trust the download. Compare the file with the published SHA-256 checksum.
- There is no automatic update in this release.
- Some text is still shown in English when the interface is set to Spanish:
  several analysis and stem-separation messages, some piano-roll and
  playlist hints and status lines, and messages that come from the operating
  system or from libraries. The Spanish has not been reviewed by a native
  speaker.
- Testing with real audio interfaces, MIDI hardware and third-party plugins
  has been limited. Windfall's own tests use software devices and plugins
  written for testing; please report problems with your hardware or plugins.
- FL Studio import covers the documented parts of the format. Device
  settings without a Windfall counterpart are listed in the import review
  and left out.

## License

Windfall is licensed under the GNU General Public License, version 3 or
later (GPL-3.0-or-later). The audio control kit in
`apps/desktop/src/components/audio` is MIT licensed and the factory sounds in
`content/factory` are CC0. The source code for this release is the tagged
commit in the repository.
