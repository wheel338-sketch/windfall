# Physical instruments integration seam

Public types live in `windfall_dsp::physical`: `Pluck` / `PluckParams`,
`FingerBass` / `FingerBassParams`, and `AcousticString` / `AcousticStringParams`.
Each instrument implements `Instrument`, has `Default`, and exposes Copy,
sanitized `ParamSet` controls with serde camelCase, legacy-field defaults and
ts-rs exports. Registered `InstrumentKind` variants are `Pluck`, `FingerBass`,
and `AcousticString`; matching parameter/runtime union variants can use these
names. Registry/model/command hosting is wired; parity status remains with the integration owner.

- **Pluck**, parity `inst-plucked`: Karplus–Strong delay with deterministic
  noise plus a plucked triangular displacement, decay, brightness and pick
  position. Parameters in descriptor order: `decaySeconds`, `brightness`,
  `pickPosition`, `level`.
- **Finger Bass**, parity `inst-boobass`: damped plucked string plus two
  normalized bandpass body peaks, at 95–140 Hz (Q 2.5) and 310–500 Hz (Q 3).
  Tone moves both the loop damping and body frequencies; `body = 0` exposes
  the bare string. Parameters: `decaySeconds`, `tone`, `damping`, `body`,
  `pickPosition`, `level`.
- **Acoustic String**, parity `inst-sakura`: two independently excited strings,
  the second detuned upward by three cents and driven through one-way bridge
  coupling; a first-order scattering allpass introduces stiffness dispersion.
  Fundamental phase compensation preserves tuning while upper partials move.
  Parameters: `decaySeconds`, `brightness`, `stiffness`, `sympathetic`,
  `pickPosition`, `level`.

## Realtime and stability limits

`prepare` allocates two fixed delay lines per voice; construction and destruction
also belong off the audio thread. All other calls reuse storage without
allocation, freeing, IO, synchronization or waits. Pluck and Acoustic String
retain eight voices including releases; overflow steals the oldest. Finger Bass
now uses a single monophonic string: every positive note replaces it with a
4 ms endpoint correction. An old key release cannot stop the new key.
Excitation uses a note sequence/key seed, which resets reproducibly. Every
sample is processed independently of callback boundaries, and the smoothed
controls update on a persistent 16-sample clock (10 ms smoothing).

The loop gain is `min(exp(-ln(1000)/(frequency * effectiveDecay)), 0.9998)`.
Finger Bass divides nominal decay by `1 + 5 * damping`. Delay interpolation and
the damping FIR are convex combinations with magnitude at most one. Acoustic
allpass coefficients are in [-0.7, 0] and use orthogonal scattering, preserving
energy even during coefficient changes. One-way secondary drive is scaled by
loop loss and cannot create an additional feedback cycle. Feedback writes are
bounded to ±2. Body filters are outside the string feedback and have fixed
moderate Q; band outputs are normalized by inverse Q. Nonfinite per-voice output
resets that voice to silence as a final guard.

Sample rates are sanitized to 8–192 kHz (nonfinite becomes 48 kHz). MIDI keys
are clamped to 0–127. The fundamental is capped at sampleRate/8 to retain room
for phase compensation; e.g. at 48 kHz the ceiling is 6 kHz, so the highest MIDI
keys share that ceiling. Linear interpolation adds treble loss and can shorten
decay at high pitches. Decay controls are nominal low-frequency RT60: Pluck
0.1–12 s, Finger Bass 0.1–8 s, Acoustic String 0.2–24 s.

Output is centered mono copied to stereo, with fixed 0.25 headroom per summed
instrument. Note attack fades in over 2 ms; key release fades over 80 ms;
`all_notes_off` fades over at most 4 ms. Quiet voices retire after output stays
below 1e-6 for at least 2048 samples and two periods. Inactive output is exact
zero; latency and post-voice tail are zero. Pick position is an excitation
control affecting subsequent notes, rather than injecting energy into ringing
strings. The base trait's legacy key note handling is supported; instance IDs,
per-note expression, pitch bends and tempo are not specialized here.

## Focused verification

Physical-module tests measure fundamental pitch after the attack (20-cent
tolerance), natural decay and exact silence, every parameter-range corner,
rapid automation with bounded polyphony, Finger Bass harmonic balance with and
without its body, Acoustic String stiffness spectrum, the second string's
contribution, deterministic reset, and exact partition invariance with note and
parameter events. Parameter tests cover defaults, descriptor order, camelCase
serialization and invalid-value sanitization. Run only the focused package
filter, with TypeScript exports redirected to temporary storage:

```sh
source scripts/msvc-env.sh
physical_bindings=$(mktemp -d)
export TS_RS_EXPORT_DIR=$(cygpath -w "$physical_bindings")
cargo test -p windfall-dsp --lib physical
```

Verification on 2026-10-08: the package command was blocked by concurrent,
out-of-scope compile errors in `drive`, `multiband` and `spatial`. A temporary
rustc test harness imported the actual `instrument.rs`, `param.rs`, `synth.rs`,
`note_expression.rs` and `blocks/mod.rs`, plus this physical module, and linked
the package's existing dependency artifacts. All 13 physical tests passed
(including three TypeScript export tests). The harness and its executable were
removed afterward. Only physical Rust files were formatted; full package and
workspace validation remain the integration owner's responsibility.

## Finger Bass application checkpoint � 2026-10-11

The six controls retain their indices and automation/history semantics. Public
engine acceptance covers audible tone edits, undo/redo, actual disk reopen and
live/offline PCM equality; mounted channel-rack acceptance covers tone editing.
New and existing physical measurements are **written, not executed** in this
edit-only stream. See [hand-off](../../qa/2026-10-11-inst-additive-physical-drum.md).
