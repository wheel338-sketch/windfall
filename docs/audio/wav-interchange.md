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
positions follow the actual offline render clock. Native song/region/pattern
exports capture sequencer clock anchors after processing each linked-tempo
control grid, including audio-dependent sources. Region exports use their own
seek epoch and output origin. Mixer output stems share the full-mix clock;
isolated mixer and playlist stems replay its effective tempo, even when the
detector's input is removed from a stem. Metadata uses that captured trajectory,
never a static tempo reconstruction for a linked clock. Pattern note `ltxt`
regions map both musical endpoints to output frames, including swing, gate,
shift and repeated prepared-score extents.

ACID/beat notes are emitted only for a constant tempo over the rendered musical
body and a representable meter. A changing clock omits both `acid` and
`WindfallBeats/1`; cues, loops and note regions remain. The final native export
result's optional `wavMetadata` message reports whether the static tempo/beat
claim was written or omitted. Tail samples do not add musical beats. Metadata
discovered during rendering is appended after RIFF `data`, with its size included
in the final RIFF header; PCM stays in place and the staging transaction still
protects existing destination files.

These versioned notes are a Windfall extension, not an ACID/RIFF standard or a
claim of third-party support. Other applications can read standard regions and
ignore the notes. The audio editor carries cue notes, slice attributes, exact
beats, root key, ACID meter/tempo and smpl loop kind/count on document-owned
`AudioRegion.notes`, default-empty for old projects. New native WAV imports read
these off-lock before the ordinary source-version/preparation publication guard.
Deleting annotations does not modify the source file. Checked region edits,
history and project files preserve their notes. The editor exposes names,
endpoints, note text, slice parameters, beats, tempo/meter/root and navigation;
users can add, rename and remove labelled regions and markers.

The editor saves its rendered selection with annotations to a Float32 WAV,
without changing history. Extract, processing and sampler/playlist selection
transfers write the same metadata into their app-owned WAV as well as the
document. Frames follow the existing offset, reverse, pitch/stretch and edit
coordinate transforms. Cropping scales exact beat counts by the retained
fraction; stretching preserves musical beat counts. Empty regions become named
markers with a `Collapsed annotation:` note preserving the former attribute.
Unknown note text is preserved verbatim. Region limits are 256 annotations,
16 notes per annotation and 1024 UTF-8 bytes per note; exceeding these refuses
the import/edit explicitly. Invalid understood attributes are also refused.

The editor's **Import annotations to timeline** action uses the current song
clock, including tempo automation, and the clip's placement. Named regions
produce named start/end markers. Skip/pause and forward infinite-loop annotations
become executable native markers in one checked undo step. Finite, alternating
and backward sample loops remain named boundaries: the native song loop has no
sample-direction or finite-repeat equivalent. Existing timeline collision and
range checks refuse invalid action imports atomically. ACID metadata remains
source metadata and does not overwrite the song's tempo/meter.

`WindfallLoop/1 <forward|alternating|backward> <play_count>`,
`WindfallTempo/1 <bpm> <integer_beats> <numerator> <denominator> <root_or_-1> <one_shot>`
and `WindfallRoot/1 <key>` are document/editor representations of standard WAV
fields. They are written back into smpl/ACID, rather than duplicated as extension
notes. A source-metadata annotation anchors these fields; exact beat notes remain
RIFF notes. Synthetic independent RIFF walkers inspect cue, label, region, note,
smpl and ACID bytes. Round-five code and fixtures have not been built or run;
no external application or listening check was performed.
