# Controller links

A controller link binds a control source to something it moves. This page
is the contract for anything that adds a source or reads the links:
hardware through the session, internal controllers through the audio engine.

Status: the original hardware project-model tests were built and run during
the MIDI stream. Internal source hosting, validation, editors and acceptance
tests were written during the edit-only freeze and have not been compiled
or run. See `docs/qa/2026-10-10-midi.md` and
`docs/qa/2026-10-11-control-sources.md` for their separate validation gates.

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
| `Internal { controller, output }` | The engine, at an owning effect slot or Dashboard channel | 0 to 1 |

`device` is a port's display name; `None` takes any input. `channel` is 1
to 16; `None` takes any. `control` is a controller number, a key (1 while
down), the pitch wheel (fourteen bits) or channel pressure.

`Internal.controller` is the id of the effect or channel that produces the
signal. Ids come from one allocator, so the number is unambiguous.
The checked project rejects missing owners and out-of-range ports. The
hardware runtime skips these links. Deleting a source removes its outgoing
links and configuration in the same undo step; undo restores all three.
Deleting a destination removes incoming links in the same edit. Moving an
effect preserves its id and links. Replacing a source kind prunes links to
ports that no longer exist and clears incompatible source configuration.

`crates/windfall-project/src/internal_controllers.rs` describes stable ports
that a future rack can reuse; this stream does not implement a rack:

| Source kind | Outputs (zero-based in the link) |
| --- | --- |
| Note envelope | 0: envelope |
| Keyboard source | 0: pitch, 1: velocity |
| Audio peak follower | 0: peak/RMS plus LFO, 1: LFO |
| Formula source | 0: result |
| XY / XYZ pad | 0: X, 1: Y, 2: Z for XYZ |
| Control surface / Dashboard | 0..7: panel controls |
| Voltage source | 0..7: normalized controls, separately routable as DC |

`ProjectSettings::internal_controllers` stores optional owner configuration:
note channel, drawn envelope and duration, compiled formula text, RMS and
LFO tempo sync, named panel widgets, and calibrated DC routes. An empty
list is omitted. `Command::SetInternalControllerSettings` replaces this
configuration in one checked, undoable edit. Ordinary source parameters
remain ordinary automatable parameters.

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
Sources do not need to know target units. Internal sources require project
`Param` targets and absolute maps without pickup. Tempo is explicitly refused:
the engine currently compiles it into the song clock map, so supporting
internal tempo modulation requires a clock feedback contract. Internal sources
cannot use `Slot` or `Action` targets or application-global links.

- `min` above `max` runs the link backwards.
- `formula` is optional text such as `1 - x^2`. Grammar and functions are
  documented in `formula.rs`. It compiles to at most 48 steps on a
  sixteen-deep stack.
- `smoothing_ms` is the time to cover about two thirds of the way.
  The checked range is 0..5000 ms.
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

## How internal sources use it

`crates/windfall-engine/src/internal_controllers.rs`

Plans compile source configuration and mapping programs away from the
callback. Source values are evaluated per sample at the real slot input;
Dashboard reads its MIDI-out panel values. Hosted control sources preserve
carrier audio and add no audio tail. The follower measures real track audio,
with selectable stereo peak or RMS ballistics and a shared free/tempo-synced
LFO phase. The bounded source formula accepts a/b/c, compiles at most 256
bytes and 64 operations with 16 nesting levels, and contains arithmetic errors.
Its grammar is separate from the link's expression of x.

Links apply on the common 64-frame automation grid after ordinary automation.
The preceding source endpoint shapes the following grid; exponential link
smoothing uses elapsed samples, followed by the existing target ramps/glides.
Multiple links to a target apply in stored order. Compatible plan adoption
carries source and link state, including existing fader ramps, while respecting
mute/solo changes. Removing a link returns the target to its stored setting.

Each note source has a fixed table of 128 held notes, with starts at the actual
note frame and latest-held-key fallback. Overflow is counted through
`Controller::rejected_control_notes`. Stop, input disconnect and sequencer clock
changes release or move the appropriate held notes. Variable-size storage is
allocated when preparing the plan, never while processing source samples.

Project checks reject scalar dependency cycles, including a source driving its
own input. They also include detector feedback through upstream audio routes,
sends, effect output taps and sidechain gain. Current-track detectors are
conservative because their input can change with track selection. These checks
can refuse a graph that would happen to be silent under today's gain settings;
they do not infer safety from silence. Invalid externally supplied source
configuration fails closed during engine plan compilation.

The voltage source starts unarmed. Its eight controls have 5 ms smoothing;
calibration emits `clamp(zero + scale * control, -1, 1)` as a software audio
sample on each chosen physical output channel. Checked routes reserve channels
against audible outputs and other DC routes. DC uses the multichannel buffer
separately from audible stereo; ordinary stereo export does not export DC.
`Processor::set_output_channels`, `hardware_samples`, and
`unavailable_dc_routes` provide a device-free harness and unavailable-route
count. Real DC coupling, volts, hardware calibration and voltage safety remain
external gates; this software contract establishes none of them.

The normal parameter context menu lists internal sources and ports. Source
editors show existing links and permit range, x mapping, smoothing and removal.
XY/XYZ values use ordinary parameter gestures and automation recording; drawn
envelope points and custom panel layouts persist as owner configuration.

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
