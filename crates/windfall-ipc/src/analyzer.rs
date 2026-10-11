//! The live analyzer protocol: choosing what to look at, and what the native
//! analyzer reports about it.
//!
//! Nothing here is saved in a project. Tickets, sequences and frame
//! positions are native 64-bit counts, sent as decimal strings so that
//! JavaScript never rounds one. A value that could not be measured is
//! absent (`null`), never a zero that would read as a healthy measurement:
//! silence, a side with a bad sample, an unknown latency.
//!
//! One consumer of analyzer data in one window is a *slot*: the signal
//! scope, the open effect editor. A slot subscribes once, then selects a
//! source as often as it likes. Every selection gets a ticket, and every
//! event carries the ticket and the request number it answers, so a view
//! can tell an event for what it shows now from one for what it showed a
//! moment ago.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_project::{EffectId, TrackId};

/// Most sources analyzed at once.
pub const ANALYZER_CAPTURES: usize = 2;
/// Most slots subscribed at once, over all windows.
pub const ANALYZER_SUBSCRIPTIONS: usize = 4;
/// Most bytes one event may take on the wire. A larger one is not sent.
pub const ANALYZER_EVENT_BYTES: usize = 512 * 1024;
/// Most events a slot is sent each second.
pub const ANALYZER_EVENTS_PER_SECOND: u32 = 30;
/// Most effect observations one frame carries. More than that since the
/// last frame are merged, deepest reading first, and counted.
pub const ANALYZER_OBSERVATIONS: usize = 64;
/// Longest slot name, in bytes of ASCII.
pub const ANALYZER_SLOT_BYTES: usize = 32;
/// Longest name or message copied into an event, in bytes of UTF-8.
pub const ANALYZER_TEXT_BYTES: usize = 256;
/// Frequency bands in [`AnalyzerSpectrum::bands`] and each history row.
pub const ANALYZER_BANDS: usize = 64;

/// Where the analyzed audio is taken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum AnalyzerSource {
    /// What leaves the engine: every track, what goes past the mixer, the
    /// metronome, and the output gain.
    MixOutput,
    /// One mixer track after its effects, fader and pan. The master's is
    /// the master without the output gain.
    TrackPostFader { track: TrackId },
}

/// The parts of a frame a slot wants. Parts not asked for are left out,
/// which keeps a meter's events small.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct AnalyzerWant {
    /// The newest analysis window as samples, for an oscilloscope.
    pub scope: bool,
    /// The spectrum of that window per bin and in bands, and band history.
    pub spectrum: bool,
    /// Mid and side points of that window, for a vectorscope.
    pub vector: bool,
    /// Minima and maxima of the last two windows, for a waveform.
    pub envelope: bool,
}

/// A slot's choice of source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerSelect {
    pub slot: String,
    /// The slot's own count of its selections. Echoed in every event.
    pub request: u32,
    /// The project the caller believes is open, as `TimelinePlaybackState`
    /// reports it. A selection for another project is refused.
    #[ts(type = "number")]
    pub generation: u64,
    pub source: AnalyzerSource,
    /// An effect on the selected track whose own measurements come with
    /// the audio: a limiter's levels and reduction, a tuner's reading.
    #[serde(default)]
    #[ts(optional)]
    pub effect: Option<EffectId>,
    #[serde(default)]
    pub want: AnalyzerWant,
}

/// Where a slot's selection stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AnalyzerPhase {
    /// Subscribed, with nothing selected.
    Idle,
    /// Selected; the audio thread has not taken the analyzer over yet.
    Preparing,
    /// The analyzer is in place and no full window of audio has arrived.
    AwaitingAudio,
    /// Frames are arriving for this exact selection.
    Live,
    /// No audio stream is open. The selection is kept and resumes.
    WaitingForStream,
    /// Audio or the view stopped keeping up. What was shown is no longer
    /// current; frames resume under a new ticket.
    Stalled,
    /// This backend cannot analyze audio at all.
    Unavailable,
    /// The selection was not accepted. `reason` says why.
    Refused,
}

/// Why a phase is what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AnalyzerReason {
    /// Analysis needs the desktop app's audio engine.
    DesktopRequired,
    /// The slot is not subscribed.
    NotSubscribed,
    /// The slot name is empty, too long or not plain ASCII.
    InvalidSlot,
    /// Four slots are subscribed already.
    SubscriptionCapacity,
    /// Two other sources are being analyzed already.
    SourceCapacity,
    /// The selection names another project than the one that is open.
    StaleProject,
    /// The track is not in the project, or has left it.
    SourceMissing,
    /// The effect is not in the project, or has left it.
    EffectMissing,
    /// The effect is not on the selected track.
    EffectNotOnSource,
    /// The stream's sample rate is outside 8 to 384 kHz.
    RateUnsupported,
    /// The source's latency is more than the analyzer's one second.
    LatencyUnsupported,
    /// The analyzer could not be prepared: a quota or an allocation.
    Preparation,
    /// A ticket or sequence counter is used up.
    Exhausted,
    /// No new audio arrived for half a second.
    AudioStalled,
    /// The view did not acknowledge an event for two seconds.
    ViewStalled,
    /// The project was replaced. Select again for the new one.
    ProjectReplaced,
}

/// Names one accepted selection. Every field is a decimal count that is
/// never reused while the app runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerTicket {
    /// The native analyzer service of this run of the app.
    pub service: String,
    pub subscription: String,
    /// The audio stream. A reopened device is another stream.
    pub stream: String,
    /// The engine's selection, shared by every slot looking at one source.
    pub selection: String,
    /// The project it was made for.
    pub project: String,
}

/// What a slot is told when its standing changes, and in answer to every
/// subscribe and select.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerStatus {
    pub slot: String,
    /// The selection this answers, or 0 before the first.
    pub request: u32,
    pub phase: AnalyzerPhase,
    pub reason: Option<AnalyzerReason>,
    pub ticket: Option<AnalyzerTicket>,
    pub source: Option<AnalyzerSource>,
    pub effect: Option<EffectId>,
    /// The selected track's name as the project has it now. Absent for the
    /// mix output, and for a name longer than 256 bytes.
    pub source_name: Option<String>,
    pub sample_rate: Option<u32>,
}

/// What the native analyzer can do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerCapability {
    /// False in a browser, where there is no audio engine.
    pub native: bool,
    pub reason: Option<AnalyzerReason>,
    pub captures: u32,
    pub subscriptions: u32,
    /// Samples per analysis window, and samples between two windows.
    pub fft_size: u32,
    pub hop_size: u32,
    /// Spectrum bins, DC through Nyquist: `fft_size / 2 + 1`.
    pub bins: u32,
    pub bands: u32,
    /// "periodicHann".
    pub window: String,
    pub scope_frames: u32,
    pub vector_points: u32,
    pub envelope_bins: u32,
    pub history_rows: u32,
    pub observations: u32,
    pub max_event_bytes: u32,
    pub max_events_per_second: u32,
    /// The open stream's rate, or absent with no stream.
    pub sample_rate: Option<u32>,
    pub min_sample_rate: u32,
    pub max_sample_rate: u32,
}

/// When the analyzed audio was, on the engine's own clock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerClock {
    /// Counts the breaks in the stream: a seek, a transport change, a new
    /// plan. Windows with different epochs are not neighbours.
    pub epoch: String,
    /// The window is frames `first_frame` up to, not including,
    /// `end_frame`, counted since the stream opened.
    pub first_frame: String,
    pub end_frame: String,
    pub sample_rate: u32,
    /// Frames by which this source lags the notes that made it, from the
    /// engine's delay compensation.
    pub pdc_frames: u32,
    /// `first_frame` less `pdc_frames`. Can be negative.
    pub aligned_first_frame: String,
    /// That position as hours, minutes, seconds and milliseconds of audio
    /// since the stream opened.
    pub time_label: String,
    /// The device's own delay in frames. Absent: not known. A buffer size
    /// is not a measurement of it.
    pub device_latency_frames: Option<u32>,
}

/// One side's level over the window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerLevel {
    /// False if any sample of the window was not a finite number. The
    /// levels are then absent, not zero.
    pub valid: bool,
    /// Absolute sample peak as linear gain. Not an oversampled true peak.
    pub peak: Option<f32>,
    pub rms: Option<f32>,
    /// The peak in dB relative to full scale. Absent for silence.
    pub peak_db: Option<f32>,
    /// The RMS in dB relative to a full-scale sample, so a full-scale sine
    /// reads -3.01. Absent for silence.
    pub rms_db: Option<f32>,
    /// A valid window with no signal in it.
    pub silent: bool,
    /// Samples at or over full scale.
    pub clipped: u32,
    /// Samples that were not finite numbers.
    pub invalid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerStereo {
    /// Normalized zero-lag cross product of the two sides, -1 to 1, not
    /// DC-removed. Absent for silence and for a window with a bad sample.
    pub correlation: Option<f32>,
    pub mid_rms: Option<f32>,
    pub side_rms: Option<f32>,
}

/// The spectrum of the window: real FFT output of the captured audio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerSpectrum {
    /// Hz between two bins: the sample rate over the window length.
    pub bin_hz: f32,
    /// Bins the window spreads a pure tone's power over: 1.5 for Hann.
    pub enbw_bins: f32,
    /// Mean-square power per bin, DC through Nyquist, left then right. A
    /// side with a bad sample is empty.
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    /// The same power summed into at most 64 equal bands and averaged over
    /// the valid sides. Empty if neither side is valid.
    pub bands: Vec<f32>,
}

/// One earlier window's spectrum in bands, for a spectrogram.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerRow {
    pub first_frame: String,
    pub end_frame: String,
    pub bands: Vec<f32>,
}

/// The newest window as samples. A sample that was not a finite number is
/// `null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerScope {
    pub first_frame: String,
    pub left: Vec<Option<f32>>,
    pub right: Vec<Option<f32>>,
}

/// Minima and maxima over the last two windows, oldest first: left
/// minimum, left maximum, right minimum, right maximum. A bin whose side
/// had only bad samples has `null` there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerEnvelope {
    pub first_frame: String,
    pub end_frame: String,
    pub bins: Vec<[Option<f32>; 4]>,
}

/// What the selected effect measured over one stretch of audio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum AnalyzerObservation {
    /// A limiter's block: its input after the input gain, its output, what
    /// it turned down, and how far its gate shut.
    #[serde(rename_all = "camelCase")]
    Dynamics {
        first_frame: String,
        frames: u32,
        input_peak: f32,
        output_peak: f32,
        reduction_db: f32,
        gate_db: f32,
    },
    /// A tuner's reading. Without a steady pitch the frequency, the note
    /// and the cents are absent.
    #[serde(rename_all = "camelCase")]
    Tuner {
        first_frame: String,
        frames: u32,
        frequency_hz: Option<f32>,
        midi_note: Option<u8>,
        /// Cents from the nearest note against the tuner's reference.
        cents: Option<f32>,
        /// How periodic the sound is, 0 to 1. Not a probability.
        confidence: f32,
    },
}

/// What was lost on the way, each as a decimal count since the selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerCounters {
    /// Audio that found the analyzer's queue full.
    pub dropped_frames: String,
    /// Samples that were not finite numbers.
    pub invalid_samples: String,
    /// Breaks in the analysis: seeks, edits, overflows.
    pub resets: String,
    pub analyzed_windows: String,
    /// Results the reader had no room for.
    pub dropped_windows: String,
    /// Effect observations that found their queue full, or were merged.
    pub dropped_observations: String,
    /// Audio blocks in which the track was not in the plan.
    pub missing_source_blocks: String,
    /// Events not sent to this slot because it had not acknowledged the
    /// one before, or because they were over the size limit.
    pub skipped_events: String,
}

/// One analysis result for a slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerFrame {
    pub slot: String,
    pub request: u32,
    pub ticket: AnalyzerTicket,
    /// Counts this slot's events. Acknowledge it to get the next.
    pub sequence: String,
    pub clock: AnalyzerClock,
    /// True when this frame does not follow on from the one sent before:
    /// a break in the audio, or events that were skipped. A history must
    /// not join the two.
    pub gap: bool,
    /// Left, then right.
    pub levels: [AnalyzerLevel; 2],
    pub stereo: AnalyzerStereo,
    pub spectrum: Option<AnalyzerSpectrum>,
    /// Earlier windows than `spectrum`, oldest first, at most seven.
    pub history: Vec<AnalyzerRow>,
    pub scope: Option<AnalyzerScope>,
    /// Mid and side of at most 128 samples spread over the window.
    pub vector: Option<Vec<[f32; 2]>>,
    pub envelope: Option<AnalyzerEnvelope>,
    /// The deepest gain reduction of the selected effect over the window,
    /// in dB. Absent unless every frame of the window has that effect's
    /// own figure: it is never worked out from the audio.
    pub reduction_db: Option<f32>,
    /// What the selected effect measured since the frame before, oldest
    /// first.
    pub observations: Vec<AnalyzerObservation>,
    pub counters: AnalyzerCounters,
    /// The transport when this frame was sent, as the realtime feed has
    /// it: the heard playhead in ticks. It is the time of sending, not of
    /// the window.
    pub playing: bool,
    pub tick: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum AnalyzerEvent {
    Status(AnalyzerStatus),
    Frame(Box<AnalyzerFrame>),
}

/// Tells the service an event arrived, which lets the next one be sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AnalyzerAck {
    pub slot: String,
    pub sequence: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_and_events_are_tagged_and_absent_values_are_null() {
        let source = AnalyzerSource::TrackPostFader { track: TrackId(7) };
        assert_eq!(
            serde_json::to_string(&source).unwrap(),
            r#"{"type":"trackPostFader","track":7}"#
        );
        assert_eq!(
            serde_json::to_string(&AnalyzerSource::MixOutput).unwrap(),
            r#"{"type":"mixOutput"}"#
        );
        let level = AnalyzerLevel {
            valid: false,
            peak: None,
            rms: None,
            peak_db: None,
            rms_db: None,
            silent: false,
            clipped: 0,
            invalid: 3,
        };
        let text = serde_json::to_string(&level).unwrap();
        assert!(text.contains(r#""peak":null"#), "{text}");
        assert!(text.contains(r#""invalid":3"#), "{text}");
        let scope = AnalyzerScope {
            first_frame: "18014398509481985".into(),
            left: vec![Some(0.5), None],
            right: vec![None, Some(-0.25)],
        };
        let text = serde_json::to_string(&scope).unwrap();
        assert_eq!(
            text,
            r#"{"firstFrame":"18014398509481985","left":[0.5,null],"right":[null,-0.25]}"#
        );
        // Frame positions beyond 2^53 survive the wire exactly.
        let back: AnalyzerScope = serde_json::from_str(&text).unwrap();
        assert_eq!(back.first_frame.parse::<u64>().unwrap(), (1 << 54) + 1);
    }

    #[test]
    fn a_selection_defaults_its_optional_parts() {
        let select: AnalyzerSelect = serde_json::from_str(
            r#"{"slot":"scope","request":3,"generation":5,"source":{"type":"mixOutput"}}"#,
        )
        .unwrap();
        assert_eq!(select.effect, None);
        assert_eq!(select.want, AnalyzerWant::default());
        let observation = AnalyzerObservation::Tuner {
            first_frame: "0".into(),
            frames: 256,
            frequency_hz: None,
            midi_note: None,
            cents: None,
            confidence: 0.25,
        };
        let text = serde_json::to_string(&observation).unwrap();
        assert!(
            text.starts_with(r#"{"type":"tuner","firstFrame":"0""#),
            "{text}"
        );
        assert!(text.contains(r#""frequencyHz":null"#), "{text}");
    }
}
