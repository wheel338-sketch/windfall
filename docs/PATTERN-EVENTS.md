# Event automation in patterns

An automation clip moves a control along the song. An event lane moves it
along a pattern: the pattern stores the curve itself, and the control
follows it wherever the pattern plays, with no clip on the playlist.

State of this document: the model, commands and engine were built and their
tests run on 2026-10-10. The editor window and everything in TypeScript were
written afterwards without being built or run. See
[the QA hand-off](qa/2026-10-10-piano.md).

## Saved data

`Pattern.events` is a list of `PatternEventLane { id, target, points }`.

- `target` is an `AutomationTarget`: a channel's volume or pan, a mixer
  track's volume, pan or integrated setting, a send or sidechain level, an
  effect's mix or setting, an instrument setting. The tempo is refused: it
  belongs to the song.
- `points` are `AutomationPoint`s in pattern ticks, read with the same
  `curve_value` an automation uses: bends, holds, two points on one tick
  for a jump. At least one point, at most 4,096, none past the longest
  pattern.
- A pattern has at most one lane per target and at most 64 lanes.
- `id` is an `AutomationId` from the project's id counter, unique among
  automations and lanes. The engine reports a lane's value under it.

The field is omitted from the file when a pattern has no lanes, and old
files load with none. Loading refuses a shared or unissued id, the tempo, a
target the project does not have, two lanes for one control, and points
that are missing, out of order or out of range.

## Commands

`SetPatternEvents { pattern, target, expected, points }` replaces one
lane. `expected` is the curve the editor captured, empty when the pattern
had none; the command fails when the pattern holds something else. No
points removes the lane. Values are brought into 0 to 1 and bends into −1
to 1. A new lane hands out an id. With a gesture id, the commands of one
drag are one undo step.

`GeneratePatternEventLfo { pattern, target, expected, start, end,
resolution, lfo }` writes the shared [LFO](CURVE-LFO.md) into a range of
the lane in one edit. A pattern with no lane starts from one point at the
value the control has now.

Lanes follow what they move. Removing a channel, track, effect, send or
sidechain removes the lanes that moved it, and undo brings them back with
it. A lane follows its effect to another track. A duplicated pattern, and a
pattern copied by Make unique, gets lanes with ids of their own.

## Playback

Lanes compile into the same per-target automation lanes clips do
(`crates/windfall-engine/src/automation.rs`), so they are read on the same
64-frame grid, ramp the same way and never allocate on the callback.

- **Pattern mode.** The selected pattern's lanes are followed from the
  start of the pattern and around again at every pass. A pattern with no
  lane for a control leaves it at its stored value. Stopping lets go at
  once: the control returns to its stored value over 100 ms. The song, by
  contrast, holds its automation while a tail rings.
- **Song mode.** Every clip of the pattern plays the lane, read at the
  place in the pattern: the clip's offset plus the time since its start,
  around the pattern's length, exactly as the clip plays the notes. Before
  the first such clip the control is untouched. After a clip ends it stays
  where the clip left it, as after an automation clip. A muted clip, or one
  on a muted playlist track, counts for nothing.
- **Both at once.** Where an automation clip and a pattern clip move the
  same control at the same time, the one on the playlist track nearer the
  top wins, then the later start, then the lower clip id — the rule
  automation clips already follow among themselves.

Nothing a lane does is written to the project: the stored value stays what
a drag of the control sets.

## Editor

**View → Show event editor**, or **Edit events in pattern** in the menu of
any automatable knob or fader, opens the event editor on the selected
pattern. It shows one lane at a time: choose the pattern and the control.
The control list holds every control that has a lane in the pattern, then
the level and pan of each channel and mixer track, each effect's mix and
each send; a setting of an instrument or effect is reached from the
control's own menu and is listed once it has a lane.

Double-click the curve to add a point. Drag a point, or focus it and use
the arrow keys, to move it; a point cannot pass its neighbours. Delete
removes it. Tick, value, bend and hold of the selected point can be typed.
**Write LFO…** opens the LFO tool on the lane and **Clear lane** removes it.
Which pattern and control the window shows is view state: it is not saved
and not part of undo.

## Not there

- No playhead in the editor and no snap setting.
- No recording of a moving control into a lane.
- FL Studio event data is not imported into lanes.
- The mirror of the engine's lanes in `apps/desktop/src/lib/automation/lanes.ts`
  does not know lanes, so the playlist's automation overview does not show
  what a pattern clip does to a control.
- A knob or fader does not show the value a lane is giving it. The engine
  reports it under the lane's id, but the frontend lookup in
  `features/automation/now.ts` only knows automations.
