# Slide and portamento source implementation

Notes carry normal, slide or portamento articulation in their optional
expression. Legacy notes remain normal. Portamento stores a duration of
1–245760 musical ticks, defaulting to one step (240 ticks). Slide duration is
the slide note's length. These controls survive native note editing,
clipboard operations, transformations, history and project persistence.

The piano roll has an articulation picker for new draw/paint notes and a
portamento-duration field. Paint captures its expression at stroke start.
Selected notes can change articulation from the menu, palette or properties
dialog; properties can also change glide duration. Labels distinguish slides
and portamento notes. Slide drawing does not audition an extra note-on.

Native sampler and built-in instrument sequencing applies a slide to the
held voices of that channel and origin without starting another voice. The
latest held pitch anchors the slide, and chord intervals are retained. Source
envelopes, velocities and note endings continue. A slide with no held voice
does nothing. A portamento starts a new destination voice and glides from the
latest held pitch; with no predecessor it starts at its destination. Samplers
retain the destination's selected sample/bank while changing playback pitch.

Glides follow musical ticks, including the plan's swing warp, tempo changes
and transport clock retiming. Voice pitch interpolation runs with the audio
sample clock using preallocated voice/owner records. The synth's mono fallback
retains the effective fractional pitch and current expression of held notes.

FL import maps its slide flag and retains an approximation notice for
differences in source voice/chord behavior. Ordinary MIDI export omits slide
control notes and exports portamento destination notes as ordinary notes;
per-note MIDI pitch/glide output remains separate required implementation.
The visual meter simulator skips slide note-on pulses.

Hosted instruments now have explicit pitch-update and capability methods.
The [native plugin expression pass](NOTE-PLUGIN-EXPRESSION.md) connects the
desktop provider's native CLAP/VST3 note IDs and supported fractional-pitch
events to these calls. MIDI-only inputs retain fallback. Saved editable
expression curves are described in [NOTE-CURVES.md](NOTE-CURVES.md). Live
MIDI/MPE, bridge-provider integration and slide-control curve ownership remain.

No builds, tests, QA, reviews or artifact generators ran for this pass. Native
and UI source additions are awaiting the later artifact and QA pass.

## 2026-10-10: MIDI files and phrasing

MIDI export can now write slides and portamento: with note expression on, a
slide is a pitch-bend ramp on the held notes' channel with no note-on of
its own, and a portamento note starts bent to the pitch before it and
returns to centre over its glide. See `crates/windfall-midi/src/expression.rs`
for the mapping and the export report. The Portamento note tool is described
in [PIANO-TOOLS.md](PIANO-TOOLS.md). Both are written and not executed.

Still missing: a pitch-bend fallback for MIDI-only plugin inputs, live MIDI
output of sequenced glides, and bridge-provider pitch transport.
