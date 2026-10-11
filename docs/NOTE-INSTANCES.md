# Runtime note-instance ownership

This source pass adds `NoteInstanceId` to the DSP instrument contract. A
runtime instance belongs to one instrument owner and one trigger occurrence;
it is separate from a persisted project `NoteId`. Repeating a note through
another loop pass or overlapping playlist clip starts a fresh runtime owner.
The id is not written into project files or exported as a UI binding.

The instrument rack now keeps 1,024 preallocated held-owner records per
instrument instead of one ending per MIDI key. Each record contains its key,
velocity, pan, expression, origin and musical ending. Starts assign fresh
nonzero ids from the engine namespace; the legacy DSP key API uses a separate
namespace. Endings follow tempo-map changes and transport clock shifts as
before. Simultaneous note-off boundaries use creation identity for ordering.
Reaching the logical ownership ceiling releases the oldest owner before
reusing its slot. The DSP's existing voice stealing/polyphony limits still
govern audible voices independently.

Built-in instruments expose instance-aware start, release and expression
update calls. The subtractive synth stores identity on each voice. Releasing
one polyphonic instance preserves other notes on its key. Its mono/legato
stack stores complete held-note controls, including multiple occurrences of
the same key; falling back restores the prior instance's velocity, pan,
release, fine pitch and modulation. Expression updates also change cached
mono notes so a later fallback uses the current controls. The legacy key API
remains available and releases all its held occurrences on that key.

Sequenced, UI-live and hardware origins are tracked separately in the rack.
UI key-up releases UI-live owners on that key; hardware key-up releases the
hardware owners. Transport jumps release sequenced owners while retaining
live owners. Hardware panic preserves owners from the other origins. Sampler
voices already had independent voice/end records and continue using them.

Plugin owner replacement copies the held records and id counter into the
prepared successor, then replays the current owners. New hosted-instrument
trait methods carry identity and expression, with an explicit capability
method. Existing providers remain key-only until their platform event
transport opts in. For them, a shorter overlapping owner does not send a
premature key-up: the last owner of that key sends the release. This fallback
cannot express independent per-voice plugin endings, and does not claim
CLAP/VST3 expression or note-instance parity.

The subsequent [native plugin expression pass](NOTE-PLUGIN-EXPRESSION.md) opts
the desktop provider into instance transport for native CLAP note ports and
VST3 event-input instruments, including held-owner replay and fractional pitch.
MIDI-only CLAP inputs retain the key/channel fallback described above.

Remaining implementation work includes bridge-provider instance replay and
capability integration, live MIDI/MPE expression and configurable modulation
routing. These remain in the whole-project feature
inventory. No builds, tests, review rounds or runtime checks ran for this
source pass, under the user's feature-first instruction. The subsequent
[articulation pass](NOTE-ARTICULATION.md) adds per-instance fractional pitch
updates and musical-tick slide/portamento playback.
The [curve pass](NOTE-CURVES.md) now carries stable saved source IDs and
independent occurrence clocks into held records and sampler voices, with
per-instance musical-clock control sampling and owner/tempo preservation.

## 2026-10-11 bridge source update

The bridge provider now negotiates native instance/expression support, maps
engine owners, and replays complete bounded channel/instance/control snapshots
through ABI 7 recovery. This supersedes the source gap above, subject to
integrator execution of the authored regressions in
[the hosting hand-off](qa/2026-10-11-plugin-hosting.md). No new native success
or cross-platform readiness is claimed.
