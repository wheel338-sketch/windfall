# Windfall Pack Synth content v1

The persisted `PackPatch` ID selects one of twelve original Windfall patches in
the Ember, Prism and Meadow packs. Each pack has an analog, wavetable, multisample
and FM patch. The performance macros edit the actual selected engine; no external
plugin, proprietary content, downloaded pack or patch loader is involved.

`polysynth/pack_assets.rs` defines immutable content prepared off the audio thread:
eight mip levels for each harmonic table, and six independently generated root-key
PCM zones with four harmonic-limited levels per sample. Key zones are 36/48/60/72/84/96.
PCM is 48 kHz, 0.5 seconds per zone, with a short attack and tapered decay/end.
These are original synthetic key/mallet/pluck multisamples, not recordings of a
commercial instrument. Wavetable scans interpolate between original soft and bright
spectra. The runtime owns the prepared asset bank independently of Copy parameters;
reset and patch changes retain it. Only prepare/construction/destruction own memory.

The formulas and resulting content in `pack_assets.rs` are dedicated to CC0-1.0.
The DSP code otherwise retains the repository's licence. There are no dependencies.
Version 1 IDs are stable and must never be reordered. Any future external pack
format must add an immutable control-thread loader, missing-asset diagnostics and
explicit content provenance before it can replace these self-contained own assets.

Public history/save/load/live/offline and mounted browser acceptance is written in
the stream's QA handoff. Nothing was executed by this edit-only worker.
