# Slice, grain and curve instrument seam

This supersedes the initial 4096-frame mono/Copy-table seam. Existing registrations
remain SliceMap, SliceDeck, GrainCloud, WaveRide; enum/ALL order is unchanged.
Their common descriptor prefixes keep indices. Immutable maps and curves are
musical assets, excluded from ParamSet and scalar automation.

## Saved and prepared assets

windfall_project::SamplePlayback lives in optional Channel.sample_playback,
omitted from old/default JSON. It holds source SampleIds A/B, 1–128 SliceRegions,
a shared PlaybackCurve, sparse per-key curves, source BPM/beat count and an
optional <=4096-frame inline source solely for legacy migration. Checked commands
SetSamplePlayback and GenerateSlicePattern use channel/pattern history. Removal
of referenced A/B sources is refused. Duplication/save/reopen retain assets.
Incompatible kind replacement clears assets in the same undo step.

windfall_dsp::granular::PlaybackAsset owns cheap clones of original decoded
AudioBuffer storage, boxed arbitrary overlapping/reordered regions and curves.
It validates finite source data, 1–32 million frames, 1–128 regions and 2–256
strictly ordered points spanning [0,1]. Construction/Arc replacement/drop happen
control-side. AnyInstrument::set_playback_asset only installs assets on unpublished
prepared units. Missing decoded sources produce silence; authoring/load failures
surface to users. Native preparation rejects invalid assets before publication.

Prepared engine identity includes saved authoring data and decoded source pointer,
frame count and rate. Reuse requires the same identity. A newly built compatible
unit copies fixed active note/voice/grain/control-clock state from the previous unit
without replacing or releasing either asset on the callback. Plans retain original
assets and retire them through control-side queues. Held slice notes retain captured
boundaries/gain/pitch/reverse/envelope; edits change subsequent notes. Source reload
and curve changes retain progress but use the new prepared source/curve.

## Distinct playback behavior

- **Slice Map** serves the basic playable slicer rows. Keys baseKey + index select
  regions in vector order; out-of-range keys are silent, with no modulo wrap.
  Starts and exclusive ends floor normalized boundaries to original frames.
  Stereo is preserved; mono duplicates to both sides. Keys choose regions rather
  than transpose them. Source/output-rate conversion uses existing linear granular
  interpolation, clamped to each region so adjacent slices never bleed.
- **Slice Deck** adds per-region pitch/gain/reverse, attack/release, arbitrary
  overlaps, ordering and deck A/B selection. Reverse starts on the final frame.
  Detection reuses windfall_project::slicer::analyze with a natural-length
  source-aligned clip. Playlist restrictions stay separate. Generated note lengths
  follow region fractions of source beats; project tempo controls scheduling.
  Playback is tape-style, without time stretching.
- **Grain Cloud** launches note-local Hann-windowed grains immediately, then at
  density 1–128 events/second, spacing 1000/density ms. Size 1–250 ms has a minimum
  of three output frames. Position/spray choose wrapped original-source starts;
  pitch is relative to rootKey. A wrapping LCG uses seed XOR (key * 0x9e3779b9);
  identical note/seed/reset reproduces schedules independently of callback blocks.
  Seed is a 24-bit scalar. Note-off stops scheduling and fades existing grains.
  Grain counters expose accepted/dropped events.
- **Wave Ride** plays original source position along 2–256 drawn points, optionally
  per key. Position values [0,1] interpolate linearly. Speed values [-8,8] integrate
  analytically from origin [0,1] over normalized time, then clamp source position.
  Duration 1–10,000 ms and pitch scale scan speed. Holds/reversals/per-note gestures
  work; normalized time 1 retires the voice. No coupling to the scratcher effect.

## Scalar automation and migration

| Kind | Scalar controls in original prefix order |
| --- | --- |
| Slice Map / Deck | baseKey, level, releaseMs |
| Grain Cloud | rootKey, grainSizeMs, density, position, spray, pitch, seed, level, releaseMs |
| Wave Ride | rootKey, durationMs, pitch, level, releaseMs |

Continuous controls glide on a persistent 16-frame clock with 10 ms smoothing;
integers jump. Assets are discrete undoable data. Removed legacy marker/per-slice/
eight-position suffixes are not reassigned. Projects automating those suffixes are
refused with a specific diagnostic so users retain the original file and resolve
those lanes before importing. Custom Channel deserialization migrates old tables,
maps and curves into saved assets; old ordinary channels keep exact default JSON.
Migrated maps retain the old modulo-key mapping and global release through an
optional legacyKeyWrap asset flag and 30 ms region release. Authored editor/detection
edits clear that flag and use bounded mappings. New/default assets omit the flag.
Standalone SampleTable/SliceSettings remain legacy helpers outside Copy params.

Current channel-preset export explicitly refuses playback assets because its
portable schema models one sampler source. Save a project or duplicate a channel
to retain complete data. Plugin presets carry scalar controls only.

## Realtime, limits and evidence

Fixed storage: 16 voices and 32 grains per voice. Overflow steals the oldest voice;
slot overflow drops/counts grain events deterministically. No callback allocation,
free, lock, IO or wait is introduced. Release expression applies; all-notes-off uses
a maximum 4 ms fade. Playback has zero algorithmic latency and no tail after voices
retire. Linear interpolation is not bandlimited; extreme transposition may alias.
Native performance/listening remain integration evidence. Existing sequencer seeks
do not chase notes started earlier.

Editors: features/sample-playback. Native guarded detection:
Session::sample_playback_detect; browser workers use the same Rust export.
Regenerate bindings/descriptors and simulator together. Acceptance inventory:
docs/qa/2026-10-11-slices-grains.md (all new tests written, not executed).
Own CC0 content example: content/examples/Slice and Grain Study/README.md.
