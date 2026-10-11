# Controller links

A controller link binds a control source to something it moves. This page
is the contract for anything that adds a source or reads the links:
hardware today, internal controllers (peak, envelope, formula) next.

Status: the project model and its tests were built and run. The session,
engine and UI parts were written during a build freeze and have not been
compiled or run. See `docs/qa/2026-10-10-midi.md`.

## The model

`crates/windfall-project/src/controller_links/`

```text
ControllerLink { source: ControlSource, target: LinkTarget, map: ControlMap }
```

Links saved with a project are `ProjectSettings::controller_links`, at most
256. `Command::SetControllerLinks { links }` replaces the list in one undo
step. A project with no links writes no field, so older files and newer
ones without links are byte-compatible.

Links for every project are `ControllerSettings::links` in the app's
`settings.json`. They may not use `LinkTarget::Param`.

### Sources

| Variant | Fed by | Value |
| --- | --- | --- |
| `Midi { device, channel, control }` | The MIDI worker, through the session | 0 to 1 |
| `Internal { controller, output }` | Reserved. Nothing feeds it yet | 0 to 1 |

`device` is a port's display name; `None` takes any input. `channel` is 1
to 16; `None` takes any. `control` is a controller number, a key (1 while
down), the pitch wheel (fourteen bits) or channel pressure.

`Internal.controller` is the id of the effect or channel that produces the
signal. Ids come from one allocator, so the number is unambiguous.
`Internal` links are stored, checked, undone and removed with their target
like any other. The hardware runtime skips them. **Not done for
`Internal`:** removing the link when its *source* is removed, and checking
that the source exists. Whoever adds the first internal source owns both.

### Targets

| Variant | Meaning |
| --- | --- |
| `Param { target: AutomationTarget }` | One thing of this project, by id |
| `Slot { slot }` | Something found by position: mixer track or channel `index` plus the session's bank offset, the master fader, the tempo |
| `Action { action }` | A command, run when the source goes from off to on |

A `Param` link is removed in the same edit that removes its target, on
every path that removes that target's automations, and comes back on undo.
A link to an effect follows the effect to another track.

### The map

```text
target value = min + (max - min) * formula(x)
```

`x` is the source value. The target value is 0 to 1 and means what an
automation point's value means for the same `AutomationTarget`:
`Project::automation_range(target).value(n)` gives the target's own unit.
Anything automatable is linkable, and a source never needs to know units.

- `min` above `max` runs the link backwards.
- `formula` is optional text such as `1 - x^2`. Grammar and functions are
  documented in `formula.rs`. It compiles to at most 48 steps on a
  sixteen-deep stack.
- `smoothing_ms` is the time to cover about two thirds of the way.
- `pickup` is `inherit`, `on` or `off`.
- `mode` is `absolute`, three relative encodings for endless knobs, or
  `toggle`.

## What a new source reuses

- `ControlMap::compile() -> CompiledMap`. Parse once, off the audio thread.
- `CompiledMap::shape(x) -> f32`. `Copy`, no allocation, no locks, bounded
  work. Safe to call per block or per sample on the audio thread.
- `LinkState::input` and `LinkState::advance` for pickup, relative steps
  and control-rate smoothing, with time passed in.
- `target_command(project, target, value)` gives the checked command that
  sets a target. `command_targets(project, command)` is its reverse.

An audio-rate source should compile its links into the engine plan and
apply `shape` where it applies automation. It should not go through
`target_command`: that path makes an undo step per movement, which is
right for a hand on a knob and wrong for a signal.

## How hardware uses it

`apps/desktop/src-tauri/src/session/controllers.rs`

1. The MIDI input callback pushes a fixed-size packet. Nothing else.
2. The MIDI worker hands each message to the session. The session matches
   it against the sources of every link in force and queues it (512
   entries, oldest dropped and counted).
3. The `windfall-controllers` thread empties the queue. Under the document
   lock it resolves targets and runs each link's state; with the lock
   released it dispatches one batch of ordinary commands. Movements less
   than 600 ms apart share a gesture id and are one undo step.

Smoothing therefore runs at control rate, about every 10 ms, not per
sample. Edits are refused while an audio recording take is open, as every
edit is.

Profiles (`ControllerProfile`) are link sets with `Slot` and `Action`
targets. Three are built in; a user profile is the same structure as a
JSON file in the `controllers` folder beside `settings.json`.

## Second-pass implementation, 2026-10-12 (written, not executed)

Settings list project/global links and expose range reversal, formula, smoothing, source mode, pickup, checked edit/removal and global positional/action learn. Multi-link shows linked count/order/cancel; named pickup indicators persist across idle pumps. Project links use checked commands/history; global links remain application settings. Stale drafts and old generations are refused.

Automation records pattern events or song clips by transport mode. Mouse/linked movements and final curve share a pass gesture. A fixed 64-slot engine touch override makes the held control audible above its previous lane. Stop/disarm releases touch and commits the contour, preserving outside points. Same-generation publication refusal retains a bounded recovery take.

Profiles optionally declare feedback: ProfileFeedback[] beside controls. Sources are banked parameter slots, transport/arm states and mixer names. Messages encode CC/note LEDs, 14-bit motor positions or bounded ASCII SysEx displays with fixed prefix/suffix/width. Validation bounds channels, bytes, envelopes and rule/message lengths. Mackie-compatible and nanoKONTROL profiles include feedback. The control pump encodes changed values (including published audible automation) and sends through the MIDI worker to the explicitly configured main output, never from the audio callback.

Internal-source runtime/source-removal validation belongs to `gpt/w3-control-sources` and is not duplicated here. Merge that branch before closing the linking row. The handoff `docs/qa/2026-10-12-p2-midi.md` names written tests and risks; none ran here. Named-device firmware/LED/motor/display behavior remains external.
