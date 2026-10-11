# Manual mixer latency and insert capacity — source implementation

The feature-first pass adds saved `MixerTrack.latencyOffsetMs`, defaulting to
zero for older projects. A signed correction from -1000 to 1000 milliseconds
adjusts the track's declared output latency. Positive values account for
unreported processor or external signal delay; negative values reduce an
overstated report. The declaration is rounded at the engine sample rate and
clamped at zero. This setting does not insert an audible delay on the track.

`UpdateMixerTrack` validates the value and captures it with the ordinary mixer
history transaction. Track cloning and project save/load preserve it. The
inspector provides a typed field, Apply, Enter, Reset, cancellation of the draft
and command errors. Changes are disabled during an active recording.

The native routing layout uses the corrected declaration for output/send
arrival times, competing direct and instrument sources, downstream strips,
Master, printed clip compensation, external ports and disk alignment metadata.
Existing delay seats retarget with the engine's compensation crossfade and
history transfer. A manual declaration uses scalar compensation because it
has no actual DSP transfer to mirror; matrix-aware automatic compensation
continues on unaffected paths. Existing graph delay bounds still apply.
Realtime and offline rendering compile the same source model.

The audible mixer limit is 501 tracks: 500 inserts and Master. The subsequent
[Current utility](CURRENT-MIXER-UTILITY.md) has one additional allocation seat
without consuming an insert. Native
document limits, plan transfer scratch, mixer buffers and atomic meter storage
use the shared project constant. Frontend add-track eligibility and mock shell
flows use the corresponding constant; strip virtualization remains in place.
FLP conversion uses the larger document capacity through its existing shared
limit, and its parser already permits 512 insert records.

Bindings are provisionally synchronized in source. The browser latency
simulator reflects declared built-in routing delays, sends, exclusive hardware
routes and manual correction; it does not provide DSP or native plugin latency
measurements. Its Rust WASM document still awaits the combined artifact pass.

No build, test, generator, benchmark, runtime check or QA review was performed
for these additions. Capacity performance, manual timing and persistence are
deferred acceptance work. Current source coverage is recorded separately; other mixer
workflows and the remaining whole-project parity scope remain implementation
work. No GitHub CI or Actions were used.

## Follow-up, 2026-10-10

Measured alignment tests for automatic compensation and the manual correction
are written (`crates/windfall-engine/tests/mixer_pdc_alignment_qa.rs`,
`crates/windfall-engine/src/qa_mixer_pdc_ports.rs`) and not yet executed. A
routed effect output (Send Tap, Transient Split) is declared at the measured
latency of its place in the chain; the track's manual correction does not
apply to it. See `docs/qa/2026-10-10-mixer.md`.
