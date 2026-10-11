# Hosted note-instance and expression transport

This feature-first source packet connects the existing engine instance contract
to the desktop native CLAP/VST3 provider. No builds, tests, reviews, generators
or runtime checks ran. Native binaries and packaged bridge helpers remain
unregenerated until the deferred build/QA pass.

Host events now include independent note-on/off ownership and typed expression
updates. Runtime 64-bit identities map to fresh positive native 32-bit IDs in a
1,024-entry preallocated owner cache. IDs below 2,048 remain reserved for the
legacy channel/key API. Overlapping notes on one key retain separate endings,
current expression and fractional pitch. Note color continues to choose the
zero-based event channel. Expression changes retain the original note channel
for that owner; they do not move a sounding voice to another MIDI channel.

Initial note-on and supported controls enter the processor queue together.
The adapter refuses the whole group under event pressure instead of starting a
partially initialized voice. Ordinary updates retain desired controls for later
admission. Note-offs use a separate preallocated reserve, with redundant-release
suppression by native identity. Panic, reset and adapter voice accounting include
both instance owners and legacy channel/key notes.

CLAP ports using the native CLAP dialect receive exact PCKN note IDs, expression
events and matching releases. MIDI-only ports retain the engine's channel/key
fallback and last-owner release behavior. CLAP defines pan in 0..1 and tuning
as a relative semitone offset; Windfall converts its signed pan and absolute
fractional pitch into those units. [CLAP event definitions](https://github.com/free-audio/clap/blob/main/include/clap/events.h).

VST3 event-input instruments receive independent note IDs. Preparation reads
bounded note-expression metadata on the native owner thread for each channel;
the callback filters updates through those cached support masks. Tuning is
converted to the SDK's normalized value, and expression events follow their
note-on at the same frame. No controller expression-discovery calls run on the
audio thread. [VST3 note-expression interface](https://github.com/steinbergmedia/vst3_pluginterfaces/blob/master/vst/ivstnoteexpression.h).

The current hosted mapping sends note pan, fine pitch/slide/portamento tuning,
modulation X as brightness and modulation Y as expression. Release becomes
note-off velocity. These standard plugin controls do not impose Windfall's
built-in filter/resonance or envelope multiplier on a plugin. Unsupported VST3
types are skipped; a plugin's own handling determines the resulting sound.
Configurable per-instrument/per-plugin modulation mapping remains feature work.
Expression metadata is captured at activation; dynamic expression-list refresh
and one-shot expression policies remain additional platform work.

The desktop provider retains complete desired owners while the native adapter
is away for state capture. On return it resets stale notes and replays starts
with their current controls in bounded groups. Partially replayed owners retain
started/dirty state across blocks. Releases during replay remove their individual
owners, while panic clears the whole desired set. A prepared successor receives
the engine's existing owner inheritance path and current fractional pitch.
Realtime and offline rendering use the same native adapter operations.

Bridge mapping ABI 6 uses eight words per event, adding explicit instance IDs,
expression kinds and 64-bit values. The release reserve covers channel/key and
instance owners. Codec validation rejects invalid identities, kinds, nonfinite
or out-of-range values and nonzero reserved fields. The helper translates
instance releases through the guaranteed native release path. ABI 5 mappings
are incompatible. Desktop bridge-provider integration, full instance snapshot
replay through bridge recovery and bridge capability negotiation remain source
work; this packet does not claim those integrations.

Live MIDI/MPE expression, expression recording, configurable mapping and slide
control-note curve ownership remain in the full project inventory. Later QA must
cover same-key overlap, source-origin release, unsupported plugin controls,
color channels, curve/glide ordering, saturated queues, capture/return/restart,
reset/panic, realtime/offline event partitions and mismatched helper versions.

## 2026-10-11 bridge source update

The bridge provider now negotiates native instance/expression support, maps
engine owners, and replays complete bounded channel/instance/control snapshots
through ABI 7 recovery. This supersedes the source gap above, subject to
integrator execution of the authored regressions in
[the hosting hand-off](qa/2026-10-11-plugin-hosting.md). No new native success
or cross-platform readiness is claimed.

The hosting follow-up also fixes VST3's SDK event list to retain numeric
expression, reserves recovery/fixed-block translation storage, and adds native
CLAP/VST3 PCM regressions for same-key ownership/release and helper replay.
Capability changes after a native restart still require provider re-preparation
acceptance. Tests are written, not executed; live MIDI/MPE and configurable
expression routing are not closed by these hosting changes.
