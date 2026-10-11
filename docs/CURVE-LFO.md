# Event and automation LFO source implementation

`curve_lfo` is a project-thread generator shared by note-event tools and
automation commands. It supports sine, triangle, rising/falling saw, pulse
with width control and deterministic seeded sample-and-hold. Period is in
musical ticks; phase, center, depth and pulse width are normalized. Values
clamp to the target's normalized range. The generator owns no callback work.

The piano roll's note menu, lane-header menu and palette open Note-event LFO.
It captures selected notes, or the complete current lane when none are
selected. The user chooses any existing event property: velocity, pan,
release, fine pitch or modulation X/Y. The native `NoteTransform::Lfo` samples
the common generator at note onsets relative to an explicit origin and blends
the result with the current property according to apply strength. Normalized
values map onto the property's full native range. One captured transaction
updates the notes, retaining unrelated properties and identities. The event
lane switches to the chosen property after apply.

An automation's source menu offers Write LFO. It captures the complete
automation, including target and points, and accepts source Start/End and
point spacing. `GenerateAutomationLfo` refuses changed captures, computes the
new points off audio, splices the range and commits one undo step. Curves shared
by several playlist clips change together. The curve before Start and from End
onward is restored: split exponential bends scale by their retained span and
duplicate boundary values localize jumps. Pulse/sample-and-hold write held
segments; saw/pulse/sample-hold boundaries receive explicit left/right values
on the first representable whole tick at or after the transition.

Point generation and the combined curve enforce the existing 4,096-point
limit before mutation. The note tool enforces the existing 16,384-note input
bound. Project/history replacement closes captures, editor disposal closes
note requests and a changed project/selection disables apply. A UI-only shape
preview displays up to eight cycles; native source owns all emitted values.
Period presets use the project's current beat/bar units.

These event-lane values are note-on properties. Continuous per-note expression
curves, recorded pattern parameter events and source-to-runtime-instance curve
lookup remain required work. Continuous automation points use the existing
scalar target and playlist playback infrastructure. No builds, tests, QA,
reviews or artifact generators ran for this source pass. Provisional bindings
await the later combined artifact pass.

## Opened from a clip — 2026-10-10

The clip menu of one selected automation clip has Write LFO. Opened there,
the dialog offers the part of the curve that clip shows: Start is the
clip's offset and End its offset plus its length. The generator and the
command are unchanged. The engine test
`an_lfo_written_into_a_curve_moves_the_linked_fader_live_and_offline`
measures the linked fader offline and in the callback and was run on
2026-10-10; `features/automation/lfo-automation.qa.test.tsx` covers the
dialog and is written, not yet run.

The note tool's values above are note-on properties: one value per note,
read at its start. Two further writers were added on 2026-10-10.

**Curve along each note.** "Write as: Curve along each note" in the note
LFO dialog sends `GenerateNoteCurveLfo`. Each captured note gets one
[expression curve](NOTE-CURVES.md) for pan, release, fine pitch or
modulation X/Y: the LFO read every `resolution` ticks along the note from
the origin, blended with the value the note had there by the apply
strength. Pulse and sample-and-hold are written as held steps; where a saw,
pulse or sample-and-hold jumps, the tick before the jump and the tick of it
both get a point while the curve has room. A note too long for 256 points
at the chosen spacing is read on a coarser grid. Velocity is struck once and
is always written per note. The captured notes and their existing curves
are compared as for `SetNoteExpressionCurves`; curves for other properties
are kept.

**Event lane.** "Write LFO…" in the event editor sends
`GeneratePatternEventLfo`, which splices the LFO into a range of a
pattern's [event lane](PATTERN-EVENTS.md) with the same range writer
automation uses.

Recording a moving control into a pattern does not exist. The generator
itself is unchanged. The native writers and their unit tests were built and
run; the dialog changes and the document-level acceptance in
`crates/windfall-project/tests/piano_articulation_lfo_qa.rs` are written and
not executed. See [the QA hand-off](qa/2026-10-10-piano.md).
