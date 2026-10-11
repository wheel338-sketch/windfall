# Note expression source implementation

Each project note now has a `NoteExpression` value, omitted from JSON at its
neutral default. Old projects load the neutral value. Add/update commands,
undo/redo edits and captured selections include expression. Provisional
TypeScript types expose the field optionally so legacy snapshots remain
representable. Existing Rust note/init constructors were adapted to the new
field without running their tests.

| Property | Stored range | Neutral | Built-in voice behavior |
| --- | --- | --- | --- |
| Release | 0–1 | 0.5 | Multiplies envelope release duration by 1/8 through 8. One-shot samplers without an envelope still play to their sample end. |
| Fine pitch | −1200–1200 cents | 0 | Adds fractional tuning independently of MIDI key, including spectral sampler bank playback. |
| Modulation X | 0–1 | 0.5 | Offsets voice filter cutoff by −4 through +4 octaves. |
| Modulation Y | 0–1 | 0.5 | Multiplies voice filter resonance by 1/4 through 4. |

Sampler voices enable their per-voice low-pass filter when either modulation
value differs from neutral. Neutral sampler expression preserves the bypass
path. Synth expression offsets its existing filter and scales its amplitude
and filter envelope releases. Synth voices also receive note pan now.
Compiled pattern events and song triggers carry expression into the voice
start, covering realtime sequencing and existing offline/stem render paths.

The rack graph exposes all expression properties. Piano-roll event lanes now
include release, fine pitch and modulation X/Y, with the existing selected-note
painting and scaling behavior. The note menu/palette and Alt+Enter open a
properties dialog for the captured selection: start, length, key, velocity,
pan and all expression fields. Mixed values show blank; only changed nonblank
fields apply. Expression reset restores all expression fields, including
articulation and glide duration, to their neutral values. The separate
[note-color routing](NOTE-COLORS.md) is retained on sound-expression reset.

`UpdateCapturedNotes` compares captured note identities and complete values
before editing. Changed/missing captured notes, duplicate captured identities
or updates naming uncaptured notes refuse the entire command. A graph stroke
or properties apply is one undo entry. The dialog closes on project/history
replacement and editor disposal; a changed revision disables its apply.

Piano copy/paste, move-copy and stamp submission preserve expression. Native
rhythm transformations inherit their source note expression. Glue and duplicate
identity comparison include expression, preserving independently voiced notes.
MIDI import carries release velocity (64 is neutral); MIDI export writes stored
release. Ordinary MIDI export does not encode per-note fractional pitch,
modulation or pan, which require a later explicit MIDI-expression/output
policy. FL import carries fine pitch and maps release/modulation values into
Windfall controls; its report retains approximation notices for differing
voice behavior and the still-unimplemented source edit-group property.
Slide and portamento implementation is described in [NOTE-ARTICULATION.md](NOTE-ARTICULATION.md).

The subsequent [runtime note-instance pass](NOTE-INSTANCES.md) replaces the
built-in instrument rack's key-based end ownership and preserves complete
mono/legato note controls. [Per-note curves](NOTE-CURVES.md) now have saved
authoring, source-instance lookup, sampler/instrument musical-clock sampling and
clipboard/generated-note ownership with cropped chopping contours.
Remaining T2 work includes slide-control curve policies, live MIDI/MPE expression,
bridge-provider expression integration and configurable instrument modulation
mapping. The subsequent [native plugin expression pass](NOTE-PLUGIN-EXPRESSION.md)
opts the desktop CLAP/VST3 provider into note-instance and standard expression
transport. MIDI-only inputs retain channel/key fallback. Live expression and
configurable mapping remain broader source work.

No builds, tests, reviews, interaction checks or artifact generators ran for
this pass, as directed by the user. The checked-in WASM simulator is still the
earlier advanced-fill artifact; new source commands need artifact regeneration
in the deferred build/QA pass.

## 2026-10-10: MIDI files and the expression map

Two pieces were written and have not been built or run in the integrated
tree; see [the QA hand-off](qa/2026-10-10-piano.md).

- MIDI export has a note-expression option: fine pitch as pitch bend,
  modulation X as controller 74, modulation Y as the modulation wheel (or
  channel pressure in MPE), a note's pan as controller 10 where no other
  pan sounds on its channel, and saved curves sampled into the same
  messages. Import reads the bend and controllers a channel has when each
  note starts. `crates/windfall-midi/src/expression.rs`.
- A channel has an expression map: what modulation X, modulation Y and
  release drive in its built-in voices (cutoff, resonance, volume, pan,
  pitch or nothing). The default is the fixed mapping above.
  `crates/windfall-dsp/src/expression_map.rs`, `SetChannelExpressionMap`.
  There is no UI for it, and hosted providers are handed the map without
  applying it.

Still missing: live MIDI/MPE input expression, bridge-provider expression
transport, the hosted side of the expression map and its UI.
