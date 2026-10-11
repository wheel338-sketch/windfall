# Audio input recording

Native recording selects a host/device and zero-based hardware channels, shown
as 1-based labels. Mono duplicates the selected channel into stereo. F32,
signed 16/32-bit and unsigned 16-bit streams are supported. The source feature
pass now includes synchronized ADC/DAC timing, native input-rate conversion,
count-in, routed monitoring and loop takes.

- [Timestamp and latency alignment](RECORDING-ALIGNMENT.md): captured packet
  times, DAC-frame gates, clock fitting/resampling, signed offset, stop tails
  and driver latency/drift readouts.
- [Metronome/count-in](METRONOME-COUNT-IN.md): native beat clicks, gain/accents,
  song/pattern meters, pre-opened input and playback deadlines.
- [Input monitoring](INPUT-MONITORING.md): explicit mixer route, worker-side
  resampling, bounded monitor queue, input gain/buffering and dropout telemetry.
- [Loop takes](LOOP-RECORDING.md): region looping, complete/final-partial passes,
  keep selection, separate kept sources and one atomic attachment/undo step.
- [Multitrack inputs](MULTITRACK-RECORDING.md): saved mixer inputs/arms,
  simultaneous shared-device capture, independent-device alignment, per-track
  monitoring/offsets and atomic group attachment.
- [Mixer disk taps](MIXER-DISK-RECORDING.md): post-effects/post-fader choices,
  native sample/processing-latency gates and mixer-only or mixed source groups.
- [Printed clip playback](PRINTED-CLIP-PLAYBACK.md): persisted Direct output,
  delay alignment and selection routing past Master processing.
- [Audio comping](AUDIO-COMPING.md): retained-source range choices, crossfades,
  editable composite clips and one atomic native history command.
- [Saved take groups](TAKE-GROUPS.md): retained pass/input associations,
  synchronized multitrack comping, pass audition and composite replacement.
- [External output ports](EXTERNAL-OUTPUT-ROUTING.md): saved mixer destinations,
  multichannel device layouts and shared output delay alignment.
- [Real-device acceptance](DEVICE-ACCEPTANCE.md): the opt-in harness that
  drives these paths against the machine's own endpoints, loopback inputs
  (`Loopback: <output device>`), and capture errors that name their cause.
- [Audio logger](AUDIO-LOGGER.md): a bounded background history of the master,
  chosen mixer tracks and one input, addable to the playlist afterwards.

Ordinary project edits, history, seeks, mode changes, replacement, save/export
and output configuration remain blocked while recording owns the take.
Stop/discard/failure closes capture and monitoring and restores owned playback
preferences. Failures remove only owned temporary sources and leave the document
unchanged. Finished sources remain for undo/redo; Save uses the existing project
sample-copy pipeline.

Earlier synthetic tests covered the original untimed callback/file/ownership
workflow before this feature pass. They do not verify the new behavior. No
builds, tests, typechecks, artifact generation, hardware/listening checks,
browser QA or reviews were run for these source additions. External output
logger workflows and remaining platform/driver support remain required implementation
work. No GitHub CI or Actions are used.
