# Spectral DSP integration seam — updated 2026-10-11

This supersedes the initial bounded-payload foundation contract. Source and
written acceptance are on `gpt/w3-spectral-effects`; nothing in this pass was
built or tested. See `docs/qa/2026-10-11-spectral-effects.md` for integration gates.
No parity status was edited.

## Ownership / assets

`ConvolverParams` now contains only the original two scalar descriptor controls,
`mix` and `gain`; it remains Copy. A project `EffectSlot.impulse` optionally owns
`ConvolverImpulse { name, sampleRate, groupDelay?, eqGains?, frames }` with independent stereo taps.
Absence means a unit impulse. The optional field is omitted at default. The slot
loader migrates the old inline mono payload (up to 2048 taps, implicitly 48 kHz)
into the new asset without dropping its samples.

`SetConvolverImpulse` validates the whole asset and commits through the existing
mixer-track edit/history/patch path. Duplication and individual effect/mixer-track
presets carry assets; preset migration preserves legacy inline IRs before scalar
cleanup. Loading another kind clears the prior IR. Slow WAV reads are canceled
when the document revision changes. PCM is portable
in project JSON: save/reopen does not depend on the original WAV still existing.
The editor reads user PCM16/24/32 or float32 WAVs, with a 2 MiB preflight file
budget, 65536-frame admission and explicit malformed/unsupported/long-IR errors.
No proprietary IR content is supplied. A symmetric 1025-tap three-region FIR EQ
is generated from original sinc/Blackman equations on the control side.

Plan effects retain immutable asset snapshots. IR changes force new prepared
processor generations; scalar automation preserves an existing owner. The engine
resamples and constructs `PreparedImpulse` kernels while preparing `EffectUnit`,
before audio publication. Rate-expanded IRs over 65536 frames refuse preparation
with a visible error instead of switching to identity. A 128 MiB admission bound
accounts for the new plan's stereo kernels/history, dry-delay storage and transient PCM. Historical
plans still retire through the engine's existing control-side retirement path.
The bound is per preparation, not a universal process RSS claim.

Partitioned overlap-save uses 128-frame partitions / 256-point FFTs, independently
for stereo channels. `set_params`, processing and reset never allocate, replace
assets, perform IO or construct kernels. Convolution reports 128 transport frames;
a generated symmetric EQ adds intentional FIR group delay (512 at 48 kHz).
Persisted, symmetry-validated metadata carries this into mixer PDC and aligns
the dry mix: the generated EQ reports 640 frames at 48 kHz. Imported WAVs have
zero declared group delay; their intrinsic phase remains part of their response.
Resampling uses linear interpolation, with no anti-alias quality claim.

## Frequency and pitch

Frequency Shifter still uses a prepared 255-tap Hilbert FIR and quadrature
modulation, additive Hz (not a ratio), 127-frame latency, independent stereo and
sample-clock oscillator state. Image rejection near DC/Nyquist and out-of-band
aliasing remain finite-FIR limitations.

Pitch Shift retains the linked cubic-grain ratio path, fixed centre latency 1026,
wet ages 2-2050. Descriptor indices 0/1 remain shift/mix; appended controls are
`formantSemitones` and `mode`. Voice mode estimates a bounded order-16 LPC source
filter, whitens the source, shifts excitation, then reconstructs an independently
warped envelope. Mix mode uses order eight for nonneutral formants and keeps the
legacy grain signal at neutral formants. This is approximate source/filter
processing, with explicit finite-state safeguards, not transparent neural vocal
resynthesis. Independent spectral-centroid / pitch-cent tests are authored.

Pitch Correct uses fixed direct YIN differences on a 256-frame decimated tracker
and the same `windfall_core::yin::{first_trough,parabolic_minimum}` as the existing
FFT/batch `windfall-analysis` detector. Selection uses strongest centered stereo
energy, preserving antiphase signals. Batch Work/FFT allocation never enters the
callback. 60-1000 Hz; at 48 kHz the window is 2048 input frames / 256-frame hop,
with audio centre latency 3074. Scale/root/speed/amount/tuning/mix retain indices;
`harmonyMix` appends. `harmonyChannel` is a nonautomated stable source-channel ID,
changed via full checked effect settings and retained on disk/history.

Four lowest distinct admitted MIDI keys from that real channel tune four extra
linked grain voices (two-octave shift bound), with 5 ms release/gain smoothing.
Missing/deleted sources supply no held keys. Engine capture records controls at
every audio frame, including short notes, instead of taking a final-block MIDI
snapshot. The same processor path serves live and offline rendering.

## Vocoders

The same filter-bank engine exposes classic 16-band and detailed 32-band modes.
Main carrier / key modulator is default; `keyIsCarrier` swaps those roles. Missing
external key is zero (never self modulation); short supplied keys are zero after
their end. Rectified linked envelopes preserve antiphase keys and independent
stereo carrier balance. Causal banks have zero transport latency and intentional
attack/release/filter tails. Band-count edits crossfade two actual banks.

An explicitly selected `synthCarrier` supplies an additive harmonic saw, omitting
harmonics above 0.45 sample rate and bounded to 64 partials. `carrierHz` controls
its actual frequency. With key-as-carrier selected, the main signal is the
modulator and the explicit synth replaces the external carrier; otherwise the
key remains the modulator. No implicit oscillator replaces a missing modulator.
All three new descriptors append after the existing six.

Purpose-built editors show routing, actual band layout, synth controls, separate
pitch/formants/modes, scale and named harmony source selection, and IR load/clear/
EQ generation. The vocoder drawing is labeled a filter layout, not a live
energy meter. Bindings/descriptors/WASM must be regenerated by the integrator.
