//! Native control/worker APIs. None of these APIs belong on an audio callback
//! or underneath a document/engine lock. This crate never edits a document.
mod artifacts;
mod inference;
mod input;
pub use inference::{InferenceTask, NoteInferenceAdapter};
pub mod classical;
pub mod classical_memory;
pub mod dereverb;
mod dereverb_adapter;
mod jobs;
mod model;
mod ort_adapter;
pub mod pitch;
pub mod separation;
pub use dereverb_adapter::{BUILTIN_DEREVERB_ID, ClassicalDereverbAdapter, DEREVERB_SCRATCH_BYTES};
pub use ort_adapter::OrtBackend;
mod work;

pub use artifacts::{ArtifactMetadata, ArtifactWriter, RelativeName};
pub use input::{
    AudioShape, CaptureStamp, CapturedInput, ContentFingerprint, FingerprintKind, FrameRange,
    InputLimits,
};
pub use jobs::{
    AnalysisAdapter, AppliedAssets, ApplyLease, JobBudget, JobId, JobManager, JobRequest,
    JobSnapshot, JobStatus, ManagerConfig, PreparedApply, Review, TicketId, Usage, WorkerContext,
};
pub use model::{
    ModelCache, ModelCacheLimits, ModelManifest, ModelProvenance, OnnxContract, OutputRole,
    TensorDimension, TensorSpec, VerifiedModel, WaveformLayout,
};
pub use work::{CancelToken, Work};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, AnalysisError>;

#[derive(Debug, Error)]
pub enum AnalysisError {
    #[error("{}", windfall_core::notice!("analysis.error.invalid", detail = windfall_core::notice::term(_0)))]
    Invalid(&'static str),
    #[error("{}", windfall_core::notice!("analysis.error.budget", detail = windfall_core::notice::term(_0)))]
    Budget(&'static str),
    #[error("{}", windfall_core::notice!("analysis.error.cancelled"))]
    Cancelled,
    #[error("{}", windfall_core::notice!("analysis.error.deadline"))]
    Deadline,
    #[error("{}", windfall_core::notice!("analysis.error.exhausted"))]
    Exhausted,
    #[error("{}", windfall_core::notice!("analysis.error.shutdown"))]
    Shutdown,
    #[error("{}", windfall_core::notice!("analysis.error.unknown"))]
    Unknown,
    #[error("{}", windfall_core::notice!("analysis.error.notReady"))]
    NotReady,
    #[error("{}", windfall_core::notice!("analysis.error.claimed"))]
    Claimed,
    #[error("{}", windfall_core::notice!("analysis.error.stale"))]
    Stale,
    #[error("{}", windfall_core::notice!("analysis.error.modelAbsent"))]
    ModelAbsent,
    #[error("{}", windfall_core::notice!("analysis.error.checksum"))]
    Checksum,
    #[error("{}", windfall_core::notice!("analysis.error.modelSize"))]
    ModelSize,
    #[error("{}", windfall_core::notice!("analysis.error.collision"))]
    Collision,
    #[error("{}", windfall_core::notice!("analysis.error.adapter", detail = windfall_core::notice::term(_0)))]
    Adapter(String),
    #[error("{}", windfall_core::notice!("analysis.error.io", detail = windfall_core::notice::term(_0.to_string())))]
    Io(#[from] std::io::Error),
    #[error("{}", windfall_core::notice!("analysis.error.codec", detail = windfall_core::notice::term(_0.to_string())))]
    Codec(#[from] windfall_codec::CodecError),
}

pub(crate) fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).ok_or(AnalysisError::Exhausted)
}

pub(crate) const CHUNK_BYTES: usize = 16 * 1024;
pub(crate) const CHUNK_SAMPLES: usize = CHUNK_BYTES / 4;
