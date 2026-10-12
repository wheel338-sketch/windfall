# WAV slice and navigation interchange

The native slice export writes a normal Float32 RIFF/WAVE file. Standard
`cue ` frame offsets, `LIST/adtl/labl` names, `ltxt` regions, `smpl` unity note
and loop records, and `acid` tempo/meter remain readable by other WAV tools.
`smpl` loop ends on disk are inclusive; region lengths and all API end positions
are exclusive. Positions count audio frames, regardless of channel count.

RIFF does not standardize slice gain, pitch, envelopes, source deck, fractional
beat counts, or playlist navigation actions. Windfall preserves these in UTF-8,
NUL-terminated `LIST/adtl/note` records. Each begins with its cue ID as a little
endian u32, followed by one of these versioned texts:

- `WindfallSlice/1 <gain> <pitch> <attack_ms> <release_ms> <reverse> <deck>`.
  Gain is linear (0..2), pitch is semitones (-48..48), attack is 0..1000 ms,
  release is 1..1000 ms, and reverse/deck are each 0 or 1. It describes the
  region with the same cue ID. Region vector order is MIDI slice order.
- `WindfallBeats/1 <bpm> <beats>` records exact finite decimal source tempo and
  beat length. A fractional length writes zero in ACID's integer beat field
  (unspecified) and preserves its actual value here. The slice reader validates
  the existing 10..522 BPM and 0.001..256 beat limits. Song/pattern exports may
  have longer musical bodies; their notes are codec metadata, not slice assets.
- `WindfallNavigation/1 loop <end_frame>`, `WindfallNavigation/1 skip <end_frame>`,
  or `WindfallNavigation/1 pause`. End frames are exclusive, relative to the
  exported file. A range ending outside the export is not given a guessed end.

Multiple notes can refer to one cue. The native slice reader refuses duplicate
slice/beat descriptions, unknown Windfall slice/beat versions, nonfinite or
out-of-range values, and invalid frame regions before changing the document.
Codec limits are 4096 cues/notes/regions/loops, 1024 UTF-8 bytes per text and
4 MiB aggregate metadata; native slice labels are limited to 256 UTF-8 bytes.

Two decks export as one self-contained WAV: A followed by B, with B regions
rebased into that audio. Different source rates use deterministic linear
resampling onto A's sample grid; mono is replicated when stereo is needed.
Native import assigns both decks to that one sample and retains each region's
deck and cue-frame boundaries. Re-export recognizes a shared source and writes
it once. Imported boundaries are frame-quantized, not byte-identical original
normalized floats. The combined source is bounded to 32 million frames and the
ordinary decoded-byte budget. Attributes are retained as settings, not baked
into the exported source PCM.

Offline rendering uses finite `play_passes`, which disables interactive timeline
navigation in the existing engine. Navigation cues therefore describe the
linear rendered audio; their notes retain the musical actions. They do not
claim that the renderer performed jumps or paused waiting for a person. Cue
positions still follow the compiled tempo map. ACID/beat notes are omitted for
varying tempo or meter, which cannot be represented by one constant value.

These versioned notes are a Windfall extension, not an ACID/RIFF standard or a
claim of third-party support. Other applications can read standard regions and
ignore the notes. Audio-editor preservation of the extra slice attributes and
an action-aware navigation import workflow remain software work. Synthetic
independent RIFF walkers inspect the extension bytes; no external application
or listening check was run in the edit-only round.
