# Note-color groups source implementation

Each note can store one of sixteen color groups, indexed 0–15, in its
expression. A group is also its explicit MIDI channel. Untagged legacy notes
retain their channel's display color and the exporter's automatic channel
assignment. JSON omits an unset group; the native expression validator limits
explicit groups to 0–15. No project-version change is required.

The piano roll's new-note picker offers channel color and all sixteen groups.
Draw, paint and chord/scale stamps capture that choice into new notes. The properties dialog and
note-menu/palette actions recolor captured selections with one undo entry.
Separate selection actions select every note in a chosen group, including
untagged notes. Mixed properties keep individual colors until explicitly
changed; resetting sound expression leaves color routing intact.

Canvas note batches shade group colors by velocity and preserve selection
flags. Copy/paste, native transformations, duplicates, history and project
persistence retain the group through the existing expression contract. Glue
and duplicate identity keep otherwise identical notes in distinct groups.
Native sampler/instrument slide and portamento targeting is confined to the
same group; untagged voices belong to the default group for native glide
targeting.

MIDI import stores the original zero-based note channel. Export writes an
explicit group directly to MIDI channels 1–16, including channel 10. Untagged
notes retain the existing per-track automatic assignment. Track volume/pan
controllers are emitted to each MIDI channel its notes actually use. Tracks
using an explicit shared MIDI channel also share MIDI controller semantics;
this is deliberate routing, not independent per-note mixing. FL import maps
the note's MIDI/color channel and retains separate source edit-group loss
notices.

The desktop plugin provider now maps groups into its existing CLAP/VST3
event-channel fields. Engine releases wait for the last logical owner on the
same channel/key; a matching key on another group does not delay release.
The host adapter and owner-exchange replay keep 2048 preallocated channel/key
slots. Immediate-release admission reserves every channel/key pair plus a
panic, and native translation buffers derive their capacities from that
reserve. Legacy key-only calls remain on channel zero. Platform per-instance
identity/pitch expression and live hardware/MPE output remain separate required
feature work. No builds, tests, QA, reviews or artifact generators ran for this
source pass; provisional bindings await the later artifact pass.

## 2026-10-10

With note expression on, MIDI export writes each colour group's pitch bend
and controllers on that group's channel, so groups keep separate bends; in
MPE mode a colour group cannot choose the channel and the export report
counts those notes. Written, not executed. Live output of sequenced notes
to a MIDI device, which is what would send colour groups to hardware, is
still not implemented.
