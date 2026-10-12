//! Independent polyphonic time-stretching and pitch-shifting in pure Rust.
//!
//! # Algorithm and provenance
//!
//! This is an adapted Rust port of the spectral phase prediction, peak-local
//! frequency mapping and multi-channel handling of Signalsmith Stretch 1.3.2.
//! Kaiser-window STFT normalization and half-bin-shifted real FFT packing follow
//! Signalsmith Linear. The complex FFT itself is `rustfft`; no C/C++ is linked.
//! Both Signalsmith libraries are MIT: their notices, and dependency notices,
//! are retained in `LICENSE-THIRD-PARTY`. Windfall additions are GPL-3.0-or-later.
//!
//! Input spectral energy sets output magnitude. Phase predictions across time
//! and neighbouring frequencies preserve partials and transient timing. The
//! strongest channel determines phase, with other channels retaining their
//! measured phase difference. Frequency maps translate each harmonic's main
//! lobe together. Windfall adds a deterministic sample clock, parameter glides,
//! history reprojection for moving pitch maps, bounded spectral-envelope gain,
//! a transformed-only 20 Hz spectral rolloff and an 8 ms silence-tail gate.
//!
//! References: [Signalsmith Stretch source](https://github.com/Signalsmith-Audio/signalsmith-stretch),
//! [Signalsmith Linear](https://github.com/Signalsmith-Audio/linear), and
//! [The Design of Signalsmith Stretch](https://signalsmith-audio.co.uk/writing/2023/stretch-design/).
//!
//! # Streaming contract
//!
//! Construct or [`Stretcher::prepare`] on a worker thread. All other methods
//! allocate nothing and take no locks. Drop on a worker thread as well.
//! [`Stretcher::process`] receives planar channels and returns consumed input
//! frames; it fills the shortest output channel length. At a steady ratio R,
//! N output frames consume N/R input frames, rounded with carried remainder.
//! Ask [`Stretcher::input_frames_needed`] for the exact requirement before a
//! call, then advance the source by the returned count. Short or absent input
//! is zero padded; thus the count can exceed the supplied input length.
//!
//! Time ratio is output duration / input duration, clamped to 0.25..=4. Pitch
//! is clamped to +/-24 semitones. Nonfinite controls restore neutral targets.
//! Ratio changes glide over 50 ms; pitch changes over 500 ms. [`Stretcher::reset`]
//! snaps both to their targets and clears history. [`Stretcher::flush`] is
//! processing silence; tails fall silent after the latency and window history.
//! [`Latency`] reports input lookahead and output delay separately. Total output
//! delay at steady R is `output + input*R`; it is fractional at some ratios.
//!
//! After a seek, feed the preceding [`Stretcher::seek_frames`] into
//! [`Stretcher::seek`]. To hear source frame S next, the pre-roll must end at
//! S + [`Latency::input_frames`], including the lookahead after S. Shorter
//! pre-roll implies silence before it. Unity seeks match continuous samples
//! within 3e-5. Nonunity seeks rebuild phase: tests bound RMS level difference
//! to 0.5 dB, individual partial level to 1 dB, and hit location to 1 ms.
//! Absolute waveform phase at nonunity is not promised. Crossfade seek changes
//! in the host, and do expensive priming off the callback thread.
//!
//! [`stretch`] allocates off-thread, returns exactly `round(frames*R)` frames,
//! trims fractional leading latency, and zero pads the end. Exact unity is a
//! bit-identical buffer copy. There is no engine/project-model dependency.
//!
//! # Quality and limits
//!
//! Fast uses 100 ms windows / 15 ms hops; Standard uses 120 / 15 ms;
//! High uses 120 / 10 ms and caps window bandwidth at eight overlaps. High
//! improves temporal update density; it does not resolve closer notes than
//! Standard. At 48 kHz, latency is 2400+2400 frames for Fast and 2880+2880
//! for Standard/High. See `VALIDATION.md` for measured CPU and signal figures.
//!
//! Tests cover stationary sines 110..3520 Hz and a five-note chord, time ratios
//! 0.5..2, +/-12 semitone shifts, stereo phase, noise, transients, automation,
//! hostile input, deterministic block partitioning and counting-allocation.
//! Frequencies stay within 0.25 cent and each stationary partial within 0.5 dB
//! for those fixtures; these are measured bounds, not guarantees for all music.
//! Unity streaming sample error is bounded by 3e-5 after latency compensation.
//! Click-train pre-echo before -2 ms is below -22 dB, with >99% energy between
//! -2 and +10 ms, at ratios 0.75..1.5. Repeated parameter sweeps have bounded
//! adjacent-sample steps; pitch automation can still cause audible level dips.
//!
//! Noise loses level and may sound phased. Dense low chords with partials
//! closer than the analysis resolution can interfere. Large stretches smear
//! and strengthen attacks; content above Nyquist is discarded when shifted up.
//! Transformed sub-bass below 20 Hz is reduced; unity bypasses that correction.
//! Formant preservation is optional and approximate, suited to voiced spectra:
//! it is not a vocal pitch tracker and may alter levels on unvoiced material.
//! An 8 ms gate truncates residual spectral ringing after input silence.
//! Arbitrary music has not been listening-tested, and realtime deadlines must
//! be verified on the host device: spectra are processed in bursts per hop.
//!
//! # Loop fitting
//!
//! [`tempo_ratio`] and [`beat_length_ratio`] compute unclamped fitting ratios.
//! [`estimate_tempo`] allocates offline and returns periodic-onset candidates
//! over 60..200 BPM. Confidence measures correlation, not probability. Beat
//! unit ambiguity remains; silence, steady signals, noise and short fixtures
//! are rejected rather than assigned a confident tempo.

mod fft;
mod offline;
mod stft;
mod stretcher;
mod tempo;

pub use offline::{
    stretch, stretch_blocks_cancellable, stretch_with_formants, stretch_with_formants_cancellable,
    stretched_frames,
};
pub use stretcher::{
    Latency, MAX_PITCH_SEMITONES, MAX_TIME_RATIO, MIN_TIME_RATIO, Quality, Stretcher,
};

pub use tempo::{TempoCandidate, beat_length_ratio, estimate_tempo, tempo_ratio};
