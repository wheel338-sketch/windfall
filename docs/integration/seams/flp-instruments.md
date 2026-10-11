# FL Studio generator import seam

`crates/windfall-flp/src/convert/synth.rs` matches the plugin's internal name
after trimming whitespace, using ASCII case-insensitive comparison. Channel
display names do not select an instrument. Only existing `InstrumentKind`
variants are used; no DSP registry or module wiring is added here.

## Decoded instrument

`3x Osc` continues to select `SubtractiveSynth` through its existing numeric
state decoder. Its 92-byte layout, oscillator fields, mix calculations,
channel volume-envelope and polyphony conversion, sanitization and loss notes
are unchanged. Its existing tests remain.

## Default stand-ins

Each name below selects `kind.default_params().sanitized()`, independently of
the source state. The import report receives this note:

> its patch settings were not decoded, so it starts from Windfall's defaults

No patch byte offsets, preset compatibility or identical sound are claimed.
The mapping follows the registered kinds and the documented DSP stand-ins in
[FM](fm.md), [additive](additive.md), [physical](physical.md),
[drums](drums.md) and [granular](granular.md).

| FL internal name | Registered `InstrumentKind` |
| --- | --- |
| Fruity DX10 | `FourOp` |
| Sytrus | `MatrixFm` |
| Toxic Biohazard | `RingHybrid` |
| Harmless | `HarmonicFilter` |
| Morphine | `SnapshotMorph` |
| Ogun | `MetalVoice` |
| Harmor | `SpectralResynth` |
| Autogun | `NumberVoice` |
| BeepMap | `ColorScan` |
| Plucked! | `Pluck` |
| BooBass | `FingerBass` |
| Sakura | `AcousticString` |
| Fruity Kick | `Kick` |
| BassDrum | `LayerKick` |
| Drumaxx | `DrumRack` |
| Drumpad | `Membrane` |
| Fruity DrumSynth Live | `DrumVoice` |
| Fruity Slicer | `SliceMap` |
| Slicex | `SliceDeck` |
| Fruity Granulizer | `GrainCloud` |
| Wave Traveller | `WaveRide` |

The stored name for Plucked is `Plucked!`, including the exclamation mark,
as used by [DawVert's FL converter](https://github.com/SatyrDiamond/DawVert/blob/main/plugins/plugconv/lmms__n_flstudio.py).
Drumaxx selects the documented sixteen-pad workflow rather than a single
`Membrane` voice; Drumpad selects the single modal `Membrane` workflow.

Harmor and BeepMap now select the prepared-source workflows. The original
source recording/image and vendor patch data are not recovered from opaque FL
state; users supply sources in the Windfall editor. The four granular stand-ins have empty sample
tables at their defaults and therefore remain silent until supplied with
source audio. Their mapping recognizes the instrument workflow but does not
recover samples, slice markers or playback settings.

## Silent placeholders

BassDrum selects LayerKick defaults; its source sample and patch settings are
not recovered. DirectWave,
SoundFont / Fruity Soundfont Player, FL Keys, FPC, Kepler, Poizone, Sawer,
Transistor Bass, FLEX, GMS, MiniSynth, SimSynth and Speech Synthesizer also
remain silent placeholders. No unregistered analog, zones, speech or surface
module is required. Other unknown names return `None`, as do channels without
a plugin. The granular seam's `inst-fruity-slicer-2` parity identifier does not
establish an internal plugin name `Fruity Slicer 2`, so that spelling is not
added as an alias.

## Focused verification

Tests cover every new name with empty and opaque state, its expected kind,
sanitized defaults and loss note, plus case/whitespace matching. Existing
3x Osc decoding tests remain, with an additional case/whitespace decoding
check. Unsupported and unknown names still return `None`.

Run from Git Bash:

```bash
source scripts/msvc-env.sh
rustfmt --edition 2024 crates/windfall-flp/src/convert/synth.rs
cargo test -p windfall-flp --lib convert::synth
```

Verified on 2026-10-08: the command above passed all 11 focused tests
(72 filtered out). Only the owned Rust file was rustfmt formatted.

## Newly registered default mappings

These mappings supersede their entries in the silent-placeholder list above:

| FL internal name | InstrumentKind |
| --- | --- |
| Transistor Bass | `AcidLine` |
| SimSynth, Poizone | `TripleOsc` |
| Sawer | `WaveLane` |
| FLEX, GMS | `MacroVoice` |
| Speech Synthesizer | `SpeechVoice` |

Each uses `kind.default_params().sanitized()`. Patch settings are still not
decoded; the report stays "its patch settings were not decoded, so it starts
from Windfall's defaults". The 3x Osc numeric decoder is unchanged. DirectWave, SoundFont, Fruity Soundfont Player, FL Keys, FPC, Kepler, MiniSynth
and Fruity Slicer 2 remain placeholders.

## I3/I4 implementation update — 2026-10-11

The mappings above now use the seven prepared/full-row instrument kinds and
single membrane introduced by this stream. Existing project kind tags remain
loadable. Focused default/opaque-state/case tests were updated, **written, not
executed**; the 2026-10-08 execution note describes historical code only.
Run `cargo test -p windfall-flp --lib convert::synth` after integration.
