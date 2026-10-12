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
    #[error("Invalid analysis input: {0}")]
    Invalid(&'static str),
    #[error("Analysis budget exceeded: {0}")]
    Budget(&'static str),
    #[error("Analysis cancelled")]
    Cancelled,
    #[error("Analysis deadline exceeded")]
    Deadline,
    #[error("Analysis identifiers exhausted")]
    Exhausted,
    #[error("Analysis service is shut down")]
    Shutdown,
    #[error("Unknown analysis job or ticket")]
    Unknown,
    #[error("Analysis job is not ready for this operation")]
    NotReady,
    #[error("Analysis apply is already claimed")]
    Claimed,
    #[error("The document, source, selection or model changed; reopen analysis")]
    Stale,
    #[error(
        "Model is absent. Explicitly import the pinned local model; network access is unavailable"
    )]
    ModelAbsent,
    #[error("Model or artifact checksum mismatch")]
    Checksum,
    #[error("Model size does not match its manifest")]
    ModelSize,
    #[error("A competitor owns the output destination")]
    Collision,
    #[error("Analysis adapter failed: {0}")]
    Adapter(String),
    #[error("Analysis IO failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Analysis codec failed: {0}")]
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
