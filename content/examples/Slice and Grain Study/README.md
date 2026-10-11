# Slice and Grain Study

These authored settings demonstrate each instrument using Windfall's existing
factory audio. All factory sounds are synthesized by Windfall and dedicated under
CC0 1.0 Universal; see `content/factory/LICENSE.md` and its generation/provenance
notes. No third-party recordings or presets are used. These settings are also
dedicated under CC0 1.0 Universal:
https://creativecommons.org/publicdomain/zero/1.0/legalcode .

## Slice Map and Slice Deck

Import `Drums/Snares/Snare Noisy.wav` from the factory browser, add Slice Map and
select it as Source A. Split its full-source region into four quarters; set Source
beats to 4 and project tempo to 120. Generate a pattern: keys 60–63 trigger regions
in order at quarter-note intervals. Move the last region earlier and regenerate
after undoing the first pattern. To exercise detection on musical loops, import a
user-owned loop and set its actual source BPM before beat-grid/transient detection.

In Slice Deck use the same source and regions. Reverse slice 2; set slice 3 pitch
to +12 and gain to 0.5; set slice 4 attack to 15 ms/release to 50 ms. Import
`Drums/Percussion/Wood Block.wav`, select it as Source B and choose deck 1 for
slice 4. Regions refer to each deck's full original source independently. Overlap
two region boundaries to layer alternate attacks on different keys.

## Grain Cloud

Use Snare Noisy as Source A, grain size 80 ms, density 24, position 0.25,
spray 1, pitch 0, seed 12345, level 0.5 and release 30 ms. Hold key 60. Increase
density to 64 and size to 120 ms for more overlap. Reset/retrigger with the same
seed to repeat the schedule. Density 64 means a 15.625 ms launch interval.

## Wave Ride

Use Snare Noisy as Source A, duration 2000 ms, pitch 0, level 0.5 and position
points `(0,0), (0.35,0.8), (0.65,0.2), (1,1)` for forward/reverse motion. Choose
Curve note key 60 to store that gesture independently. For an integrated speed
example use origin 0.4 and points `(0,1), (1,-1)`: normalized source position is
`0.4 + t - t²`, peaking at 0.65 halfway through the gesture.

Save/reopen a project or duplicate the channel to retain the original-source map
and curves. Portable channel-preset export is explicitly refused until its asset
schema supports these instruments. Export and compare live playback at the same
project settings. These instructions are **not executed acceptance evidence**.
