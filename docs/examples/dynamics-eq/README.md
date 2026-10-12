# Original dynamics, EQ, filter, stereo and delay examples

These 17 patches, their example phrases and plain descriptive names were authored for Windfall
on 2026-10-12. They are dedicated to the public domain under
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).
They contain only original parameter choices and notes: no vendor presets, audio,
samples, impulse responses or copied names. Source code keeps its repository
licence.

Open any `.windfall` file here with **File -> Open**, then press Play. Each
has an original four-note phrase, a built-in synth and the named effect on the
master. Toggle that effect to compare. Save As and reopen to keep your edits.
No samples, plugins or downloads are needed. Installed desktop bundles ship
this folder under their resource directory's `examples/effects/`.

Each companion `.wfpreset` is also a real Windfall plugin preset for the
existing preset session API and checked `ApplyPluginPreset` command. That
command loads settings as one undo step; it can replace the destination's
effect kind. The current desktop editor does not expose a plugin-preset file
picker, so File -> Open of the companion project is the user-facing workflow.
Unspecified settings use the existing reader's defaults and notices.

The control-side catalogue in
`crates/windfall-project/src/effect_examples.rs` embeds exactly these files.
Its tests load the real envelope, compare raw typed settings with the reader,
check sanitization and undo/redo, write and reopen projects, and render all
patches and the self-contained projects against their default counterparts.
Tests are written, not executed.

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
