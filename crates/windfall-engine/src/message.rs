//! What crosses between the control side and the audio thread.
//!
//! Both directions are lock-free queues. Anything that owns heap memory is
//! built on the control side, moved to the audio thread whole, and moved
//! back as [`Garbage`] when the audio thread is done with it, so the audio
//! thread itself never allocates or frees.

use std::sync::Arc;

use rtrb::{Producer, PushError};
use windfall_core::AudioBuffer;
use windfall_ipc::PlayMode;
use windfall_project::{ChannelId, PatternId};

use crate::plan::Plan;
use crate::state::PlanState;

/// Messages the control side can have waiting for the audio thread.
pub(crate) const MESSAGE_CAPACITY: usize = 1024;

/// Room for retired values. Handling one message retires at most two. The
/// control side explicitly captures garbage into an outer retirement carrier;
/// a full queue delays serial message adoption until that collection happens.
pub(crate) const GARBAGE_CAPACITY: usize = 4 * MESSAGE_CAPACITY;

/// Free garbage slots the audio thread wants before it takes a message.
pub(crate) const GARBAGE_HEADROOM: usize = 4;

// Scene requests remain inline so adopting a scene cannot allocate or free.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Message {
    SetWorkers(Box<crate::parallel::Workers>),
    SetPlan {
        plan: Arc<Plan>,
        state: Box<PlanState>,
    },
    /// `passes` makes playback stop by itself after that many times through
    /// the pattern or song. `from` starts it somewhere other than the
    /// stopped position, which stays where stopping returns to.
    Play {
        sequence: u32,
        passes: Option<u32>,
        from: Option<f64>,
    },
    Stop {
        sequence: u32,
    },
    CountIn {
        sequence: u32,
        bars: u8,
        capture: Option<u64>,
    },
    Seek(f64),
    SetRegion(Option<windfall_project::TickRange>),
    SetTransport {
        mode: PlayMode,
        pattern: PatternId,
        loop_song: bool,
        metronome: windfall_ipc::MetronomeSettings,
    },
    NoteOn {
        channel: ChannelId,
        key: u8,
        velocity: f32,
    },
    NoteOff {
        channel: ChannelId,
        key: u8,
    },
    HardwareNote {
        epoch: u64,
        channel: ChannelId,
        key: u8,
        /// Zero ends the note; 1..=127 starts it.
        velocity: u8,
    },
    Preview(AudioBuffer),
    StopPreview,
    Performance(crate::performance::LaunchRequest),
    PerformanceRecordBegin(Arc<crate::performance_recording::PerformanceRecording>),
    PerformanceRecordEnd,
    PerformanceReplay(Option<Box<crate::performance_recording::Replay>>),
    PerformanceStopAll,
    PerformanceScene([Option<crate::performance::LaunchRequest>; crate::performance::TRACKS]),
    SetOutputGain(f32),
    SetInputMonitors(Box<[Box<crate::recording_monitor::MonitorReader>]>),
    SetDiskTaps(Box<[crate::recording_disk::DiskWriter]>),
    /// Installs the audio logger's tap, or with `None` takes it away.
    SetAudioLogger(Option<Box<crate::audio_logger::LoggerTap>>),
    /// Recording touch override; real parameter units, cleared at stop/disarm.
    AutomationTouch {
        target: windfall_project::AutomationTarget,
        value: Option<f32>,
    },
}

/// Values the audio thread is done with. Dropping them is the control
/// side's job.
#[expect(
    dead_code,
    reason = "the values are never read, only carried to the thread that may free them"
)]
#[allow(clippy::large_enum_variant)] // rack owners move inline; boxing would allocate on audio
pub(crate) enum Garbage {
    PerformanceReplay(Box<crate::performance_recording::Replay>),
    PerformanceRecording(Arc<crate::performance_recording::PerformanceRecording>),
    Workers(Box<crate::parallel::Workers>),
    Plan(Arc<Plan>),
    State(Box<PlanState>),
    Sample(AudioBuffer),
    SamplerBank(Arc<crate::sampler_processing::SamplerBank>),
    SampleBank(Arc<windfall_dsp::sample_bank::SampleBank>),
    ModularRack(crate::modular::ModularRack),
    InputMonitors(Box<[Box<crate::recording_monitor::MonitorReader>]>),
    DiskTaps(Box<[crate::recording_disk::DiskWriter]>),
    AudioLogger(Box<crate::audio_logger::LoggerTap>),
}

/// Sends a value back to the control side to be dropped there.
pub(crate) fn retire(garbage: &mut Producer<Garbage>, item: Garbage) {
    if let Err(PushError::Full(item)) = garbage.push(item) {
        // Callers make sure there is room, so this should not happen. If it
        // ever does, leaking is the one way left to avoid freeing memory on
        // the audio thread.
        std::mem::forget(item);
    }
}
