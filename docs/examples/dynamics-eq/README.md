# Original dynamics, EQ, filter, stereo and delay examples

These 17 patches and their plain descriptive names were authored for Windfall
on 2026-10-12. They are dedicated to the public domain under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).
They contain only original parameter choices: no vendor presets, audio,
samples, impulse responses or copied names. Source code keeps its repository
licence.

Each `.wfpreset` is a real Windfall plugin preset. Add an effect, open its
existing **Load preset** action and choose the file. The checked
`ApplyPluginPreset` command loads the effect settings as one undo step.
Save your project normally; reopening retains the patch. Loading can replace
the destination's effect kind, so use a new slot when trying an example.
Unspecified settings use the existing preset reader's defaults; its notices
explain those defaults. No assets or installation are needed.

The control-side catalogue in
`crates/windfall-project/src/effect_examples.rs` embeds exactly these files.
Its tests load the real envelope, compare raw typed settings with the reader,
check sanitization and undo/redo, write and reopen projects, and render all
patches against their default counterparts. Tests are written, not executed.

| Effect | Example |
| --- | --- |
| Transient Shaper | Attack lift |
| Mastering Limiter | Gentle level and color |
| Bass Harmonics | Bass octave support |
| Multiband Maximizer | Balanced band lift |
| Frequency delay | Alternating short band echoes |
| Spreader | Short stereo space |
| Morph EQ | Low to high curve blend |
| Band Split | Three band tilt |
| Echo bank | Two linked filtered echoes |
| Selectable filter | Soft resonant lowpass |
| Filter Bank | Alternating drawn gate |
| Multiband Compressor | Gentle three band control |
| Pan Motion | Slow pan and pulse |
| Stereo Enhancer | Right offset and narrow sides |
| One Knob | Moderate band enhancement |
| Stage Stack | Warm safety stages |
| Seven Band | Low mid cleanup |

These are starting points, not claims of mastering suitability, pitch-neutral
delay automation or sound-identical emulation. Adjust levels for your material.
