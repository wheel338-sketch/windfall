# DrumSynth model attribution

The renderer in `src/drumsynth_model.rs` is a Rust port of the public
DrumSynth 2.0 model, with bounded input/output, local state, explicit portable
randomness and sample-rate scaling. It is distributed under this workspace's
GPL-3.0-or-later licence, using the upstream GPL-2.0-or-later grant.

- Paul Kellett, Copyright (c) 1998-2000, `drumsynth.c` and `help/drumsyn.rtf`:
  [ntwk/DrumSynth d0ac0c72d3fb3e688a35c21c749d5f606e19ccb7](https://github.com/ntwk/DrumSynth/tree/d0ac0c72d3fb3e688a35c21c749d5f606e19ccb7).
  `readme.license` explicitly offers MIT or GPL version 2 or any later version;
  `gpl-2.0.txt` contains the GPL text. The MIT alternative is not used here.
- Paul Kellett and Paul Giblock, Copyright (c) 1998-2000 and 2007,
  [LMMS a2f57e70ce9c3468b4b6d21955bbe65a0989048a/src/core/DrumSynth.cpp](https://github.com/LMMS/lmms/blob/a2f57e70ce9c3468b4b6d21955bbe65a0989048a/src/core/DrumSynth.cpp):
  header grants GPL version 2 or any later version. Envelope times are scaled
  from authored 44,100 Hz samples to the requested render rate.

The original code is provided without warranty, including merchantability or
fitness for a particular purpose. See the repository's LICENSE for the GPL.
Downloaded trees remain outside the repository and were read only. Tests
construct their own presets and numerical references; no third-party preset
or audio file is distributed.

The original libc `rand()` has no portable sequence. Windfall uses SplitMix64
in the same block/component ordering. `Noise.FixedSeq=1` selects seed 1 across
noise and both bands; otherwise the caller's seed applies. Integer output
quantization, block decimation and the 2,400-plus-rounded-envelope tail are
preserved. Floating-point rounding and noise realizations can differ from the
original executable; this is a complete model, not a bit-identical binary port.
