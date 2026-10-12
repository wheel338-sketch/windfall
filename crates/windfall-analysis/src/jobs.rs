use crate::artifacts::{Artifact, publish};
use crate::model::inventory;
use crate::{
    AnalysisError, ArtifactMetadata, ArtifactWriter, CancelToken, CaptureStamp, CapturedInput,
    InputLimits, ModelCache, ModelCacheLimits, ModelManifest, RelativeName, Result, VerifiedModel,
    Work, add, lock,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tempfile::TempDir;

// Includes worst-case native paths for sixteen staged/published/prebuilt
// artifact records, bounded destinations, transitions and scheduler nodes.
const RECORD_BYTES: u64 = 128 * 1024;
const CODEC_ALLOWANCE: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct JobId(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketId(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Queued,
    Running,
    Cancelling,
    Cancelled,
    Failed,
    Ready,
    Consumed,
}
impl JobStatus {
    fn settled(self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::Failed | Self::Ready | Self::Consumed
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSnapshot {
    pub id: JobId,
    pub request: u64,
    pub ticket: TicketId,
    pub status: JobStatus,
    pub sequence: u64,
    pub completed_work: u64,
    pub maximum_work: u64,
    pub failure: Option<String>,
    /// Only transitions, at most eight; progress is a coalesced latest value.
    pub transitions: Vec<JobStatus>,
}
#[derive(Debug, Clone, Copy)]
pub struct JobBudget {
    pub scratch_bytes: u64,
    pub output_bytes: u64,
    pub work_units: u64,
    pub timeout: Duration,
}

pub struct JobRequest {
    pub amount: f32,
    pub task: crate::InferenceTask,
    pub input: CapturedInput,
    pub model: ModelManifest,
    pub budget: JobBudget,
}

#[derive(Debug, Clone)]
pub struct ManagerConfig {
    pub staging_root: PathBuf,
    pub published_root: PathBuf,
    pub workers: usize,
    pub max_queued: usize,
    pub max_retained_jobs: usize,
    pub memory_bytes: u64,
    /// Includes staging + in-progress publication + every retained final file.
    pub disk_bytes: u64,
    pub max_published_files: usize,
    pub max_scratch_bytes: u64,
    pub max_work_units: u64,
    pub max_timeout: Duration,
    pub input_limits: InputLimits,
}
impl ManagerConfig {
    pub fn bounded(staging_root: PathBuf, published_root: PathBuf) -> Self {
        Self {
            staging_root,
            published_root,
            workers: 2,
            max_queued: 4,
            max_retained_jobs: 16,
            memory_bytes: 256 * 1024 * 1024,
            disk_bytes: 1024 * 1024 * 1024,
            max_published_files: 256,
            max_scratch_bytes: 64 * 1024 * 1024,
            max_work_units: 4_000_000_000,
            max_timeout: Duration::from_secs(600),
            input_limits: InputLimits::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub queued: usize,
    pub active: usize,
    pub retained_jobs: usize,
    pub memory_bytes: u64,
    pub reserved_disk_bytes: u64,
    pub published_disk_bytes: u64,
    pub reserved_output_files: usize,
    pub published_files: usize,
}

/// Trusted native adapter, installed once per manager. An adapter's own fixed
/// model/runtime memory must be charged in scratch_bytes; it must use the
/// provided bounded scratch and checkpoint between bounded CPU chunks. This
/// cooperative contract does not sandbox arbitrary native allocations/code.
/// Built-in classical dereverb ships; imported ONNX inference remains unavailable.
pub trait AnalysisAdapter: Send + Sync + 'static {
    fn id(&self) -> &str;
    fn version(&self) -> &str;
    fn accepts_model(&self, model: &ModelManifest) -> bool {
        model.provenance.adapter_id == self.id() && model.provenance.adapter_version == self.version()
    }
    fn required_scratch_bytes(&self, _task: crate::InferenceTask) -> u64 {
        0
    }
    /// Reservation includes adapter-owned allocations; do not allocate it twice.
    fn scratch_buffer_bytes(&self, reserved: u64) -> u64 {
        reserved
    }
    /// Only trusted native code can supply a weight-free algorithm descriptor.
    fn builtin_model(&self, _manifest: &ModelManifest) -> Result<Option<VerifiedModel>> {
        Ok(None)
    }
    fn supports(&self, task: crate::InferenceTask) -> bool {
        task == crate::InferenceTask::Audio
    }
    fn run_task(
        &self,
        task: crate::InferenceTask,
        input: &CapturedInput,
        model: &VerifiedModel,
        context: &mut WorkerContext,
        artifacts: &mut ArtifactWriter,
    ) -> Result<()> {
        if !self.supports(task) {
            return Err(AnalysisError::Adapter(
                "This runtime does not support the requested operation/mode.".into(),
            ));
        }
        self.run(input, model, context, artifacts)
    }
    fn run(
        &self,
        input: &CapturedInput,
        model: &VerifiedModel,
        context: &mut WorkerContext,
        artifacts: &mut ArtifactWriter,
    ) -> Result<()>;
}
pub struct WorkerContext {
    amount: f32,
    work: Work,
    scratch: Vec<f32>,
}
impl WorkerContext {
    pub fn amount(&self) -> f32 {
        self.amount
    }
    pub fn work(&mut self) -> &mut Work {
        &mut self.work
    }
    pub fn scratch(&mut self) -> &mut [f32] {
        &mut self.scratch
    }
    pub fn checkpoint(&mut self, units: u64) -> Result<()> {
        self.work.checkpoint(units)
    }
}

#[derive(Debug, Clone)]
pub struct Review {
    pub amount: f32,
    pub task: crate::InferenceTask,
    pub job: JobId,
    pub ticket: TicketId,
    pub capture: CaptureStamp,
    pub model: ModelManifest,
    pub artifacts: Vec<ArtifactMetadata>,
}
impl Review {
    /// Pure eligibility only. The caller must rehash/recover the canonical
    /// current source off State, then atomically guard recording-before-State
    /// and dispatch one checked document batch. This proves no installation.
    pub fn check_eligibility(&self, current: &CaptureStamp, model: &ModelManifest) -> Result<()> {
        if self.capture != *current || self.model != *model || !current.source_identity.is_live() {
            return Err(AnalysisError::Stale);
        }
        Ok(())
    }
}

struct Proposal {
    amount: f32,
    task: crate::InferenceTask,
    input: CapturedInput,
    model: ModelManifest,
    _directory: TempDir,
    artifacts: Vec<Artifact>,
    destinations: Option<Vec<RelativeName>>,
}
impl Proposal {
    fn check_eligibility(&self, current: &CaptureStamp, model: &ModelManifest) -> Result<()> {
        if self.input.stamp() != current
            || self.model != *model
            || !current.source_identity.is_live()
        {
            return Err(AnalysisError::Stale);
        }
        Ok(())
    }
    fn review(&self, id: JobId, ticket: TicketId) -> Review {
        Review {
            amount: self.amount,
            task: self.task,
            job: id,
            ticket,
            capture: self.input.stamp().clone(),
            model: self.model.clone(),
            artifacts: self.artifacts.iter().map(|a| a.metadata.clone()).collect(),
        }
    }
}
struct Task {
    request: JobRequest,
    deadline: Instant,
}
struct Record {
    snapshot: JobSnapshot,
    task: Option<Task>,
    proposal: Option<Proposal>,
    cancel: CancelToken,
    claimed: bool,
    retiring: bool,
    memory: u64,
    disk: u64,
    files: usize,
    budget: JobBudget,
    cleanup_path: Option<PathBuf>,
    publication_temp: Option<PathBuf>,
}
impl Record {
    fn bump(&mut self) -> Result<()> {
        if self.snapshot.sequence >= u64::MAX - 16 {
            return Err(AnalysisError::Exhausted);
        }
        self.snapshot.sequence += 1;
        Ok(())
    }
    fn transition(&mut self, status: JobStatus) {
        // All fallible operations reserve terminal-transition headroom.
        self.snapshot.sequence += 1;
        self.snapshot.status = status;
        self.snapshot.transitions.push(status);
        debug_assert!(self.snapshot.transitions.len() <= 8);
    }
}
#[derive(Clone)]
struct Counters {
    job: u64,
    ticket: u64,
    request: u64,
}
// A replaced Session/manager cannot make an old in-process IPC ticket name a
// new job. These volatile IDs are process-wide; tickets are never persisted.
static IDENTIFIERS: Mutex<Counters> = Mutex::new(Counters {
    job: 1,
    ticket: 1,
    request: 1,
});
impl Counters {
    fn submission(&mut self) -> Result<(JobId, TicketId, u64)> {
        let mut prepared = self.clone();
        let ids = (
            JobId(next(&mut prepared.job)?),
            TicketId(next(&mut prepared.ticket)?),
            next(&mut prepared.request)?,
        );
        *self = prepared;
        Ok(ids)
    }
}
fn next(counter: &mut u64) -> Result<u64> {
    if *counter == u64::MAX {
        return Err(AnalysisError::Exhausted);
    }
    let value = *counter;
    *counter += 1;
    Ok(value)
}
struct State {
    records: BTreeMap<JobId, Record>,
    queue: VecDeque<JobId>,
    usage: Usage,
    #[cfg(test)]
    counters: Option<Counters>,
    shutdown: bool,
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    config: ManagerConfig,
    cache: ModelCache,
    adapter: Arc<dyn AnalysisAdapter>,
}

/// Control-side owner. Drop/shutdown joins real worker threads and can do IO;
/// it must never run under State, controller or audio locks.
pub struct JobManager {
    shared: Arc<Shared>,
    workers: Mutex<Vec<JoinHandle<()>>>,
    lifecycle: Mutex<()>,
}
impl JobManager {
    /// Recheck caller-owned auditions before any capture/preview/apply working
    /// allocation. Included reservations remain held during a claimed review.
    pub fn check_retained_memory(&self, retained: u64) -> Result<()> {
        let state = lock(&self.shared.state);
        if add(state.usage.memory_bytes, retained)? > self.shared.config.memory_bytes {
            return Err(AnalysisError::Budget(
                "retained previews and analysis memory",
            ));
        }
        Ok(())
    }
    /// Control-side preflight for the Session's serialized, bounded anonymous
    /// capture files. Existing jobs cannot grow beyond their held reservations.
    /// Submit separately charges the retained input file for its whole lifetime.
    pub fn check_capture_disk(&self, bytes: u64) -> Result<()> {
        let state = lock(&self.shared.state);
        if state.shutdown {
            return Err(AnalysisError::Shutdown);
        }
        if add(
            add(
                state.usage.reserved_disk_bytes,
                state.usage.published_disk_bytes,
            )?,
            bytes,
        )? > self.shared.config.disk_bytes
        {
            return Err(AnalysisError::Budget("capture/staging/publication disk"));
        }
        Ok(())
    }
    pub fn new(
        mut config: ManagerConfig,
        cache: ModelCache,
        adapter: Arc<dyn AnalysisAdapter>,
    ) -> Result<Self> {
        if config.workers == 0
            || config.workers > 16
            || config.max_queued == 0
            || config.max_retained_jobs == 0
            || config.max_retained_jobs > 256
            || config.memory_bytes < RECORD_BYTES + CODEC_ALLOWANCE
            || config.disk_bytes == 0
            || config.max_published_files == 0
            || config.max_work_units == 0
            || config.max_work_units > u64::MAX - 32
            || config.max_timeout.is_zero()
        {
            return Err(AnalysisError::Invalid("manager bounds"));
        }
        fs::create_dir_all(&config.staging_root)?;
        fs::create_dir_all(&config.published_root)?;
        config.staging_root = fs::canonicalize(&config.staging_root)?;
        config.published_root = fs::canonicalize(&config.published_root)?;
        if config.staging_root.as_os_str().len() > 1024
            || config.published_root.as_os_str().len() > 1024
        {
            return Err(AnalysisError::Invalid("job root path length"));
        }
        let roots = [
            &config.staging_root as &Path,
            &config.published_root,
            cache.root(),
        ];
        for (i, root) in roots.iter().enumerate() {
            for other in &roots[..i] {
                if root.starts_with(other) || other.starts_with(root) {
                    return Err(AnalysisError::Invalid("overlapping ownership directories"));
                }
            }
        }
        let (published_disk_bytes, published_files) = inventory(
            &config.published_root,
            ModelCacheLimits {
                max_model_bytes: config.disk_bytes,
                max_cache_bytes: config.disk_bytes,
                max_entries: config.max_published_files,
                max_imports: 1,
            },
        )?;
        // Stale directories from earlier processes are never deleted here.
        // Refuse a nonempty staging root: a session must give us fresh storage.
        if fs::read_dir(&config.staging_root)?.next().is_some() {
            return Err(AnalysisError::Invalid(
                "staging root must be empty and exclusively owned",
            ));
        }
        let shared = Arc::new(Shared {
            config,
            cache,
            adapter,
            changed: Condvar::new(),
            state: Mutex::new(State {
                records: BTreeMap::new(),
                queue: VecDeque::new(),
                usage: Usage {
                    published_disk_bytes,
                    published_files,
                    ..Usage::default()
                },
                #[cfg(test)]
                counters: None,
                shutdown: false,
            }),
        });
        let manager = Self {
            shared,
            workers: Mutex::new(Vec::new()),
            lifecycle: Mutex::new(()),
        };
        for number in 0..manager.shared.config.workers {
            let shared = manager.shared.clone();
            let handle = thread::Builder::new()
                .name(format!("windfall-analysis-{number}"))
                .spawn(move || worker(shared))?;
            lock(&manager.workers).push(handle);
        }
        Ok(manager)
    }
    pub fn submit(&self, request: JobRequest) -> Result<JobId> {
        self.submit_with_retained_memory(request, 0)
    }
    /// Session-owned audition capacity remains reserved with this job, even
    /// if a previous job was consumed while the callback still owns its audio.
    pub fn submit_with_retained_memory(&self, request: JobRequest, retained: u64) -> Result<JobId> {
        if !request.amount.is_finite() || !(0.0..=1.0).contains(&request.amount) {
            return Err(AnalysisError::Invalid("analysis wet/dry amount"));
        }
        let config = &self.shared.config;
        let shape = request.input.stamp().input_shape;
        let frames = request.input.stamp().selection.frames()?;
        request.model.validate_input(shape, frames)?;
        request.task.validate(&request.model)?;
        if !self.shared.adapter.supports(request.task) {
            return Err(AnalysisError::Adapter(
                "The imported pack has no installed runtime for this operation/mode.".into(),
            ));
        }
        if !self.shared.adapter.accepts_model(&request.model) {
            return Err(AnalysisError::Invalid("adapter/model revision mismatch"));
        }
        let b = request.budget;
        if b.scratch_bytes < self.shared.adapter.required_scratch_bytes(request.task)
            || b.scratch_bytes > config.max_scratch_bytes
            || !b.scratch_bytes.is_multiple_of(4)
            || b.scratch_bytes > usize::MAX as u64
            || b.output_bytes == 0
            || b.work_units == 0
            || b.work_units > config.max_work_units
            || b.timeout.is_zero()
            || b.timeout > config.max_timeout
        {
            return Err(AnalysisError::Invalid("job budget bounds"));
        }
        let limits = config.input_limits;
        if request.input.is_file_backed() && !request.model.is_builtin_dereverb() {
            return Err(AnalysisError::Invalid(
                "file-backed input requires built-in dereverb",
            ));
        }
        if request.input.retained_bytes() > limits.max_bytes
            || request.input.disk_bytes()? > limits.max_bytes
            || shape.frames > limits.max_frames
            || shape.channels > limits.max_channels
            || shape.sample_rate < limits.min_sample_rate
            || shape.sample_rate > limits.max_sample_rate
        {
            return Err(AnalysisError::Budget("manager input bounds"));
        }
        let mut decode_bytes = 0;
        let mut minimum_output = 0;
        for output in &request.model.outputs {
            let bytes = frames
                .checked_mul(u64::from(output.channels))
                .and_then(|n| n.checked_mul(4))
                .ok_or(AnalysisError::Exhausted)?;
            decode_bytes = decode_bytes.max(bytes);
            minimum_output = add(minimum_output, add(bytes, 256)?)?;
        }
        if minimum_output > b.output_bytes {
            return Err(AnalysisError::Budget("aligned output reservation"));
        }
        // A built-in audition may retain the previous result while preparing
        // the next wet/dry preview or the committed pool source. Reserve a
        // third result for the codec sink's grow/reallocation coexistence.
        if request.model.is_builtin_dereverb() {
            decode_bytes = decode_bytes
                .checked_mul(3)
                .ok_or(AnalysisError::Exhausted)?;
        }
        let memory = add(
            retained,
            add(
                add(
                    add(
                        add(request.input.retained_bytes(), request.model.bytes)?,
                        b.scratch_bytes,
                    )?,
                    decode_bytes,
                )?,
                CODEC_ALLOWANCE,
            )?,
        )?;
        let memory = add(
            memory,
            add(
                request.model.retained_bytes()?,
                request
                    .model
                    .copy_bytes()?
                    .checked_mul(2)
                    .ok_or(AnalysisError::Exhausted)?,
            )?,
        )?;
        let disk = add(
            request.input.disk_bytes()?,
            b.output_bytes
                .checked_mul(3)
                .ok_or(AnalysisError::Exhausted)?,
        )?;
        let deadline = Instant::now()
            .checked_add(b.timeout)
            .ok_or(AnalysisError::Exhausted)?;
        // One sequential publication temporary plus the bounded final set.
        let files = request.model.outputs.len() + 1 + usize::from(request.input.is_file_backed());
        let mut state = lock(&self.shared.state);
        if state.shutdown {
            return Err(AnalysisError::Shutdown);
        }
        if state.queue.len() >= config.max_queued || state.records.len() >= config.max_retained_jobs
        {
            return Err(AnalysisError::Budget("queue/retained job count"));
        }
        if add(state.usage.memory_bytes, add(memory, RECORD_BYTES)?)? > config.memory_bytes {
            return Err(AnalysisError::Budget("retained job memory"));
        }
        if add(
            add(
                state.usage.reserved_disk_bytes,
                state.usage.published_disk_bytes,
            )?,
            disk,
        )? > config.disk_bytes
        {
            return Err(AnalysisError::Budget("job/publication disk"));
        }
        if state
            .usage
            .published_files
            .checked_add(state.usage.reserved_output_files)
            .and_then(|n| n.checked_add(files))
            .is_none_or(|n| n > config.max_published_files)
        {
            return Err(AnalysisError::Budget("persistent output count"));
        }
        let (id, ticket, request_id) = allocate_submission(&mut state)?;
        state.usage.memory_bytes += memory + RECORD_BYTES;
        state.usage.reserved_disk_bytes += disk;
        state.usage.reserved_output_files += files;
        state.usage.queued += 1;
        state.usage.retained_jobs += 1;
        state.queue.push_back(id);
        state.records.insert(
            id,
            Record {
                snapshot: JobSnapshot {
                    id,
                    request: request_id,
                    ticket,
                    status: JobStatus::Queued,
                    sequence: 1,
                    completed_work: 0,
                    maximum_work: b.work_units,
                    failure: None,
                    transitions: {
                        let mut transitions = Vec::with_capacity(8);
                        transitions.push(JobStatus::Queued);
                        transitions
                    },
                },
                task: Some(Task { request, deadline }),
                proposal: None,
                cancel: CancelToken::default(),
                claimed: false,
                retiring: false,
                memory,
                disk,
                files,
                budget: b,
                cleanup_path: None,
                publication_temp: None,
            },
        );
        self.shared.changed.notify_all();
        Ok(id)
    }
    pub fn usage(&self) -> Usage {
        lock(&self.shared.state).usage
    }
    pub fn snapshot(&self, id: JobId) -> Result<JobSnapshot> {
        Ok(lock(&self.shared.state)
            .records
            .get(&id)
            .ok_or(AnalysisError::Unknown)?
            .snapshot
            .clone())
    }
    /// Wait off locks for ready/failed/cancelled/consumed. Condition-variable
    /// predicates include record removal and shutdown; there is no polling.
    pub fn wait(&self, id: JobId) -> Result<JobSnapshot> {
        let mut state = lock(&self.shared.state);
        loop {
            let record = state.records.get(&id).ok_or(AnalysisError::Unknown)?;
            if record.snapshot.status.settled() && !record.retiring {
                return Ok(record.snapshot.clone());
            }
            state = self
                .shared
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
    pub fn wait_for_change(&self, id: JobId, after_sequence: u64) -> Result<JobSnapshot> {
        let mut state = lock(&self.shared.state);
        loop {
            let record = state.records.get(&id).ok_or(AnalysisError::Unknown)?;
            if after_sequence > record.snapshot.sequence {
                return Err(AnalysisError::Invalid("future progress sequence"));
            }
            if record.snapshot.sequence > after_sequence || record.snapshot.status.settled() {
                return Ok(record.snapshot.clone());
            }
            state = self
                .shared
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
    pub fn review(&self, ticket: TicketId) -> Result<Review> {
        let state = lock(&self.shared.state);
        let (id, record) = find_ticket(&state, ticket)?;
        if record.claimed {
            return Err(AnalysisError::Claimed);
        }
        let proposal = record.proposal.as_ref().ok_or(AnalysisError::NotReady)?;
        Ok(proposal.review(id, ticket))
    }
    pub fn cancel(&self, id: JobId) -> Result<JobStatus> {
        let cleanup = {
            let mut state = lock(&self.shared.state);
            let record = state.records.get_mut(&id).ok_or(AnalysisError::Unknown)?;
            if record.claimed {
                return Err(AnalysisError::Claimed);
            }
            match record.snapshot.status {
                JobStatus::Consumed
                | JobStatus::Cancelled
                | JobStatus::Failed
                | JobStatus::Cancelling => return Ok(record.snapshot.status),
                JobStatus::Running => {
                    record.cancel.cancel();
                    record.transition(JobStatus::Cancelling);
                    self.shared.changed.notify_all();
                    return Ok(JobStatus::Cancelling);
                }
                JobStatus::Queued | JobStatus::Ready => {
                    record.cancel.cancel();
                    record.transition(JobStatus::Cancelling);
                    record.retiring = true;
                    let task = record.task.take();
                    let proposal = record.proposal.take();
                    if task.is_some() {
                        state.queue.retain(|queued| *queued != id);
                        state.usage.queued -= 1;
                    }
                    Cleanup {
                        shared: self.shared.clone(),
                        id,
                        task,
                        proposal,
                        terminal: JobStatus::Cancelled,
                        failure: None,
                        work: None,
                    }
                }
            }
        };
        drop(cleanup);
        Ok(JobStatus::Cancelled)
    }
    /// Explicit retention policy: only settled, unclaimed, fully retired jobs
    /// can be forgotten. Ready work must first be cancelled or consumed.
    pub fn forget(&self, id: JobId) -> Result<()> {
        let mut state = lock(&self.shared.state);
        let record = state.records.get(&id).ok_or(AnalysisError::Unknown)?;
        if record.claimed || record.retiring {
            return Err(AnalysisError::Claimed);
        }
        if !matches!(
            record.snapshot.status,
            JobStatus::Consumed | JobStatus::Cancelled | JobStatus::Failed
        ) {
            return Err(AnalysisError::NotReady);
        }
        if record.cleanup_path.is_some() || record.publication_temp.is_some() {
            return Err(AnalysisError::Budget("staging cleanup remains reserved"));
        }
        debug_assert_eq!(record.memory, 0);
        state.records.remove(&id);
        state.usage.memory_bytes -= RECORD_BYTES;
        state.usage.retained_jobs -= 1;
        self.shared.changed.notify_all();
        Ok(())
    }
    pub fn claim(
        &self,
        ticket: TicketId,
        current: &CaptureStamp,
        model: &ModelManifest,
    ) -> Result<ApplyLease> {
        let mut state = lock(&self.shared.state);
        if state.shutdown {
            return Err(AnalysisError::Shutdown);
        }
        let (id, record) = find_ticket(&state, ticket)?;
        if record.claimed {
            return Err(AnalysisError::Claimed);
        }
        let proposal = record.proposal.as_ref().ok_or(AnalysisError::NotReady)?;
        proposal.check_eligibility(current, model)?;
        if record.snapshot.completed_work >= record.budget.work_units {
            return Err(AnalysisError::Budget("apply work units"));
        }
        let timeout = record.budget.timeout;
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(AnalysisError::Exhausted)?;
        let request = allocate_apply(&mut state)?;
        let record = state.records.get_mut(&id).expect("ticket found");
        record.bump()?;
        record.claimed = true;
        let mut work = make_work(
            &self.shared,
            id,
            CancelToken::default(),
            record.budget.work_units,
            deadline,
        );
        work.set_completed(record.snapshot.completed_work);
        let proposal = record.proposal.take();
        Ok(ApplyLease {
            shared: self.shared.clone(),
            id,
            ticket,
            request,
            proposal,
            work: Some(work),
        })
    }
    pub fn shutdown(&self) {
        let _lifecycle = lock(&self.lifecycle);
        let ids = {
            let mut state = lock(&self.shared.state);
            state.shutdown = true;
            self.shared.changed.notify_all();
            state.records.keys().copied().collect::<Vec<_>>()
        };
        for id in ids {
            let _ = self.cancel(id);
        }
        let handles = std::mem::take(&mut *lock(&self.workers));
        for handle in handles {
            let _ = handle.join();
        }
    }
    /// Retry only this job's private refused staging, never a published path.
    /// A cleanup failure retains its disk/count reservation and blocks forget.
    pub fn retry_cleanup(&self, id: JobId) -> Result<()> {
        let paths = {
            let mut state = lock(&self.shared.state);
            let record = state.records.get_mut(&id).ok_or(AnalysisError::Unknown)?;
            if record.claimed || record.retiring {
                return Err(AnalysisError::Claimed);
            }
            if !matches!(
                record.snapshot.status,
                JobStatus::Cancelled | JobStatus::Failed | JobStatus::Consumed
            ) {
                return Err(AnalysisError::NotReady);
            }
            if record.cleanup_path.is_none() && record.publication_temp.is_none() {
                return Ok(());
            }
            record.retiring = true;
            (record.cleanup_path.clone(), record.publication_temp.clone())
        };
        let result = remove_job_paths(&paths, &self.shared.config);
        let mut state = lock(&self.shared.state);
        let record = state.records.get_mut(&id).expect("cleanup retry retained");
        record.retiring = false;
        if result.is_ok() {
            record.cleanup_path = None;
            record.publication_temp = None;
            let (disk, files) = (record.disk, record.files);
            record.disk = 0;
            record.files = 0;
            state.usage.reserved_disk_bytes -= disk;
            state.usage.reserved_output_files -= files;
        }
        self.shared.changed.notify_all();
        result
    }
}
impl Drop for JobManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn find_ticket(state: &State, ticket: TicketId) -> Result<(JobId, &Record)> {
    state
        .records
        .iter()
        .find(|(_, record)| record.snapshot.ticket == ticket)
        .map(|(id, record)| (*id, record))
        .ok_or(AnalysisError::Unknown)
}
fn allocate_submission(_state: &mut State) -> Result<(JobId, TicketId, u64)> {
    #[cfg(test)]
    if let Some(counters) = _state.counters.as_mut() {
        return counters.submission();
    }
    lock(&IDENTIFIERS).submission()
}
fn allocate_apply(_state: &mut State) -> Result<u64> {
    #[cfg(test)]
    if let Some(counters) = _state.counters.as_mut() {
        return next(&mut counters.request);
    }
    next(&mut lock(&IDENTIFIERS).request)
}
fn make_work(
    shared: &Arc<Shared>,
    id: JobId,
    cancel: CancelToken,
    maximum: u64,
    deadline: Instant,
) -> Work {
    let mut work = Work::with_deadline(cancel, maximum, deadline);
    let progress_shared = shared.clone();
    work.progress = Some(Arc::new(move |completed| {
        let mut state = lock(&progress_shared.state);
        let record = state.records.get_mut(&id).ok_or(AnalysisError::Unknown)?;
        record.bump()?;
        if completed < record.snapshot.completed_work {
            return Err(AnalysisError::Invalid("progress regressed"));
        }
        record.snapshot.completed_work = completed;
        progress_shared.changed.notify_all();
        Ok(())
    }));
    work
}

fn worker(shared: Arc<Shared>) {
    loop {
        let (id, task, cancel) = {
            let mut state = lock(&shared.state);
            loop {
                if state.shutdown {
                    return;
                }
                if let Some(id) = state.queue.pop_front() {
                    state.usage.queued -= 1;
                    state.usage.active += 1;
                    let record = state.records.get_mut(&id).expect("queued record");
                    record.transition(JobStatus::Running);
                    let task = record.task.take().expect("queued task");
                    let cancel = record.cancel.clone();
                    shared.changed.notify_all();
                    break (id, task, cancel);
                }
                state = shared
                    .changed
                    .wait(state)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        let deadline = task.deadline;
        let token = cancel.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            execute(&shared, id, task, token)
        }))
        .unwrap_or_else(|_| Err(AnalysisError::Adapter("native adapter panicked".into())));
        let cleanup = {
            let mut state = lock(&shared.state);
            state.usage.active -= 1;
            let shutting_down = state.shutdown;
            let record = state.records.get_mut(&id).expect("active record retained");
            match result {
                Ok(proposal)
                    if !cancel.is_cancelled() && !shutting_down && Instant::now() < deadline =>
                {
                    record.proposal = Some(proposal);
                    record.transition(JobStatus::Ready);
                    shared.changed.notify_all();
                    None
                }
                result => {
                    let (proposal, failure, terminal) = match result {
                        Ok(proposal) if cancel.is_cancelled() || shutting_down => {
                            (Some(proposal), None, JobStatus::Cancelled)
                        }
                        Ok(proposal) => (
                            Some(proposal),
                            Some(AnalysisError::Deadline.to_string()),
                            JobStatus::Failed,
                        ),
                        Err(_) if cancel.is_cancelled() || shutting_down => {
                            (None, None, JobStatus::Cancelled)
                        }
                        Err(error) => (
                            None,
                            Some(error.to_string().chars().take(1024).collect()),
                            JobStatus::Failed,
                        ),
                    };
                    record.retiring = true;
                    Some(Cleanup {
                        shared: shared.clone(),
                        id,
                        task: None,
                        proposal,
                        terminal,
                        failure,
                        work: None,
                    })
                }
            }
        };
        drop(cleanup);
    }
}

fn execute(shared: &Arc<Shared>, id: JobId, task: Task, cancel: CancelToken) -> Result<Proposal> {
    let request = task.request;
    let work = make_work(shared, id, cancel, request.budget.work_units, task.deadline);
    let mut context = WorkerContext {
        amount: request.amount,
        work,
        scratch: Vec::new(),
    };
    context.work.check()?;
    let scratch_bytes = shared
        .adapter
        .scratch_buffer_bytes(request.budget.scratch_bytes);
    if scratch_bytes > request.budget.scratch_bytes || !scratch_bytes.is_multiple_of(4) {
        return Err(AnalysisError::Invalid("adapter scratch reservation"));
    }
    context
        .scratch
        .try_reserve_exact((scratch_bytes / 4) as usize)
        .map_err(|_| AnalysisError::Budget("worker scratch allocation"))?;
    if context.scratch.capacity() as u64 * 4 > request.budget.scratch_bytes {
        return Err(AnalysisError::Budget("worker scratch capacity"));
    }
    context.scratch.resize((scratch_bytes / 4) as usize, 0.0);
    let model = match shared.adapter.builtin_model(&request.model)? {
        Some(model) => model,
        None => shared.cache.load(&request.model, &mut context.work)?,
    };
    let mut writer = if request.model.provenance.adapter_id == crate::separation::ADAPTER_ID {
        ArtifactWriter::new_at_rate(
        &shared.config.staging_root,
        &request.model,
        request.input.stamp().selection.frames()?,
        request.input.stamp().selection.start,
        request.budget.output_bytes,
        request.input.stamp().input_shape.sample_rate,
        )?
    } else {
        ArtifactWriter::new(&shared.config.staging_root, &request.model,
            request.input.stamp().selection.frames()?, request.input.stamp().selection.start,
            request.budget.output_bytes)?
    };
    lock(&shared.state)
        .records
        .get_mut(&id)
        .expect("running record")
        .cleanup_path = Some(writer.path().to_path_buf());
    shared.adapter.run_task(
        request.task,
        &request.input,
        &model,
        &mut context,
        &mut writer,
    )?;
    let (directory, artifacts) = writer.complete(&mut context.work)?;
    Ok(Proposal {
        amount: request.amount,
        task: request.task,
        input: request.input,
        model: request.model,
        _directory: directory,
        artifacts,
        destinations: None,
    })
}

/// Drops all uncommitted staging and buffers before releasing reservations.
/// Published outputs are never owned by this deletion path.
struct Cleanup {
    shared: Arc<Shared>,
    id: JobId,
    task: Option<Task>,
    proposal: Option<Proposal>,
    terminal: JobStatus,
    failure: Option<String>,
    // The progress closure and its allocation are retired after State guards.
    work: Option<Work>,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        drop(self.task.take());
        drop(self.proposal.take());
        drop(self.work.take());
        let paths = {
            let state = lock(&self.shared.state);
            let record = state
                .records
                .get(&self.id)
                .expect("cleanup record retained");
            (record.cleanup_path.clone(), record.publication_temp.clone())
        };
        let removal = remove_job_paths(&paths, &self.shared.config);
        let mut state = lock(&self.shared.state);
        let record = state
            .records
            .get_mut(&self.id)
            .expect("cleanup record retained");
        let (memory, disk, files) = (record.memory, record.disk, record.files);
        record.memory = 0;
        record.retiring = false;
        record.snapshot.failure = self.failure.take();
        if let Err(error) = &removal {
            record.snapshot.failure = Some(format!(
                "Owned staging/publication cleanup failed and remains reserved: {error}"
            ));
        } else {
            record.disk = 0;
            record.files = 0;
            record.cleanup_path = None;
            record.publication_temp = None;
        }
        if record.snapshot.status != self.terminal {
            record.transition(self.terminal);
        }
        state.usage.memory_bytes -= memory;
        if removal.is_ok() {
            state.usage.reserved_disk_bytes -= disk;
            state.usage.reserved_output_files -= files;
        }
        self.shared.changed.notify_all();
    }
}

fn remove_job_paths(
    paths: &(Option<PathBuf>, Option<PathBuf>),
    config: &ManagerConfig,
) -> Result<()> {
    let staging = paths
        .0
        .as_ref()
        .map(|path| remove_owned_stage(path, &config.staging_root))
        .transpose();
    let publication = paths
        .1
        .as_ref()
        .map(|path| remove_owned_publication(path, &config.published_root))
        .transpose();
    staging.and(publication).map(|_| ())
}
fn remove_owned_publication(path: &Path, root: &Path) -> Result<()> {
    if path.parent() != Some(root) {
        return Err(AnalysisError::Invalid(
            "publication temporary ownership path",
        ));
    }
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            fs::remove_file(path)?;
            Ok(())
        }
        Ok(_) => Err(AnalysisError::Invalid(
            "publication temporary was replaced; manual recovery required",
        )),
    }
}
fn cleanup_publication_temp(shared: &Shared, id: JobId) -> Result<()> {
    let path = lock(&shared.state)
        .records
        .get(&id)
        .expect("claimed record retained")
        .publication_temp
        .clone();
    if let Some(path) = path {
        remove_owned_publication(&path, &shared.config.published_root)?;
        lock(&shared.state)
            .records
            .get_mut(&id)
            .expect("claimed record retained")
            .publication_temp = None;
    }
    Ok(())
}

fn remove_owned_stage(path: &Path, root: &Path) -> Result<()> {
    if path.parent() != Some(root) {
        return Err(AnalysisError::Invalid("staging ownership path"));
    }
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path)?;
            Ok(())
        }
        Ok(_) => Err(AnalysisError::Invalid(
            "staging directory was replaced; manual recovery required",
        )),
    }
}

/// Exclusive ready-ticket ownership; cancellation/forget refuse while claimed.
/// Dropping a failed/refused apply restores Ready (or cancels during shutdown).
/// One bounded set of destinations is pinned on the first publication attempt;
/// retries reuse already published files and cannot grow orphan output sets.
pub struct ApplyLease {
    shared: Arc<Shared>,
    id: JobId,
    ticket: TicketId,
    request: u64,
    proposal: Option<Proposal>,
    work: Option<Work>,
}
impl ApplyLease {
    /// Audition the exact captured selection under the same review lease. This
    /// uses immutable prepared input, never the source pathname or a new view.
    pub fn preview_input(&self, work: &mut Work) -> Result<windfall_core::AudioBuffer> {
        let input = self.input();
        let count = input
            .stamp()
            .selection
            .frames()?
            .checked_mul(u64::from(input.stamp().input_shape.channels))
            .ok_or(AnalysisError::Exhausted)?;
        let count = usize::try_from(count).map_err(|_| AnalysisError::Exhausted)?;
        let mut copy = Vec::new();
        copy.try_reserve_exact(count)
            .map_err(|_| AnalysisError::Budget("dry preview allocation"))?;
        input.for_each_selected(work, |block, work| {
            work.checkpoint(block.len() as u64)?;
            copy.extend_from_slice(block);
            Ok(())
        })?;
        work.check()?;
        Ok(windfall_core::AudioBuffer::from_interleaved(
            input.stamp().input_shape.sample_rate,
            input.stamp().input_shape.channels,
            copy,
        ))
    }
    /// Claim ownership while auditioning; cancellation cannot remove the
    /// staging asset being decoded. Dropping the lease restores Ready.
    pub fn preview_audio(&self, role: &str, work: &mut Work) -> Result<windfall_core::AudioBuffer> {
        let proposal = self.proposal.as_ref().expect("live lease");
        let artifact = proposal
            .artifacts
            .iter()
            .find(|a| a.metadata.role == role)
            .ok_or(AnalysisError::Invalid("unknown preview role"))?;
        let audio = windfall_codec::decode_file_strict_with(
            &artifact.staged,
            &windfall_codec::DecodeOptions {
                max_decoded_bytes: artifact.metadata.shape.frames
                    * u64::from(artifact.metadata.shape.channels)
                    * 4,
            },
        )?;
        if crate::AudioShape::of(&audio) != artifact.metadata.shape {
            return Err(AnalysisError::Checksum);
        }
        let mut file = std::fs::File::open(&artifact.staged)?;
        let fingerprint =
            crate::ContentFingerprint::reader(&mut file, artifact.metadata.bytes, work)?;
        if fingerprint.sha256 != artifact.metadata.sha256
            || fingerprint.bytes != artifact.metadata.bytes
        {
            return Err(AnalysisError::Checksum);
        }
        work.check()?;
        Ok(audio)
    }
    pub fn request(&self) -> u64 {
        self.request
    }
    pub fn input(&self) -> &CapturedInput {
        &self.proposal.as_ref().expect("live apply lease").input
    }
    pub fn review(&self) -> Review {
        self.proposal
            .as_ref()
            .expect("live apply lease")
            .review(self.id, self.ticket)
    }
    /// File copy, checksum, no-clobber publication and sync all happen here,
    /// off State/audio. A collision leaves its final bytes intact. Even partial
    /// successful final publications are retained and charged, never unlinked.
    pub fn prepare(mut self, names: Vec<RelativeName>) -> Result<PreparedApply> {
        let proposal = self.proposal.as_mut().expect("live lease");
        if names.len() != proposal.artifacts.len()
            || names.iter().enumerate().any(|(i, n)| {
                names[..i]
                    .iter()
                    .any(|old| old.as_str().eq_ignore_ascii_case(n.as_str()))
            })
        {
            return Err(AnalysisError::Invalid("publication name count/duplicates"));
        }
        if proposal
            .destinations
            .as_ref()
            .is_some_and(|old| *old != names)
        {
            return Err(AnalysisError::Invalid("publication destinations changed"));
        }
        proposal.destinations = Some(names.clone());
        // One tracked publication temporary at a time. A refused deletion
        // blocks a retry before it can allocate/write another temporary.
        cleanup_publication_temp(&self.shared, self.id)?;
        let work = self.work.as_mut().expect("live apply work");
        for (artifact, name) in proposal.artifacts.iter_mut().zip(names) {
            let target = self.shared.config.published_root.join(name.as_str());
            if let Some(path) = &artifact.metadata.published_path {
                if *path != target {
                    return Err(AnalysisError::Invalid("published target changed"));
                }
                let fingerprint = crate::ContentFingerprint::reader(
                    fs::File::open(path)?,
                    artifact.metadata.bytes,
                    work,
                )?;
                if fingerprint.bytes != artifact.metadata.bytes
                    || fingerprint.sha256 != artifact.metadata.sha256
                {
                    return Err(AnalysisError::Checksum);
                }
                continue;
            }
            let publication = publish(artifact, &target, work, |path| {
                let path = path.to_path_buf();
                lock(&self.shared.state)
                    .records
                    .get_mut(&self.id)
                    .expect("claimed record retained")
                    .publication_temp = Some(path);
            });
            if let Err(error) = publication {
                // Keep the original refusal while preserving the path/charge
                // if the explicit deletion attempt also fails.
                let _ = cleanup_publication_temp(&self.shared, self.id);
                return Err(error);
            }
            artifact.metadata.published_path = Some(target);
            let mut state = lock(&self.shared.state);
            let record = state
                .records
                .get_mut(&self.id)
                .expect("claimed record retained");
            record.disk -= artifact.metadata.bytes;
            record.files -= 1;
            state.usage.reserved_disk_bytes -= artifact.metadata.bytes;
            state.usage.published_disk_bytes += artifact.metadata.bytes;
            state.usage.reserved_output_files -= 1;
            state.usage.published_files += 1;
            drop(state);
            cleanup_publication_temp(&self.shared, self.id)?;
        }
        let artifacts = proposal
            .artifacts
            .iter()
            .map(|a| a.metadata.clone())
            .collect();
        Ok(PreparedApply {
            lease: self,
            artifacts,
        })
    }
}
impl Drop for ApplyLease {
    fn drop(&mut self) {
        let Some(proposal) = self.proposal.take() else {
            return;
        };
        let cleanup = {
            let mut state = lock(&self.shared.state);
            let shutdown = state.shutdown;
            let record = state
                .records
                .get_mut(&self.id)
                .expect("claimed record retained");
            record.claimed = false;
            if shutdown {
                record.retiring = true;
                Some(Cleanup {
                    shared: self.shared.clone(),
                    id: self.id,
                    task: None,
                    proposal: Some(proposal),
                    terminal: JobStatus::Cancelled,
                    failure: None,
                    work: self.work.take(),
                })
            } else {
                record.proposal = Some(proposal);
                self.shared.changed.notify_all();
                None
            }
        };
        drop(cleanup);
    }
}

pub struct PreparedApply {
    lease: ApplyLease,
    artifacts: Vec<ArtifactMetadata>,
}
impl PreparedApply {
    /// Prebuilt output metadata for off-State pool/command preparation.
    pub fn artifacts(&self) -> &[ArtifactMetadata] {
        &self.artifacts
    }
    pub fn input(&self) -> &CapturedInput {
        self.lease.input()
    }
    pub fn request(&self) -> u64 {
        self.lease.request()
    }
    pub fn review(&self) -> Review {
        self.lease.review()
    }
    pub fn check_eligibility(&self, current: &CaptureStamp, model: &ModelManifest) -> Result<()> {
        self.lease
            .proposal
            .as_ref()
            .expect("prepared proposal")
            .check_eligibility(current, model)
    }
    /// Call ONLY after the caller's real atomic document commit. Infallible,
    /// with no disk/codec/worker operations; returns deferred retirement, which
    /// must be dropped/retired after every Session/recording guard is released.
    /// If the caller commit failed, drop PreparedApply instead to restore Ready.
    pub fn acknowledge_commit(mut self) -> AppliedAssets {
        let proposal = self.lease.proposal.take().expect("prepared proposal");
        {
            let mut state = lock(&self.lease.shared.state);
            let record = state
                .records
                .get_mut(&self.lease.id)
                .expect("claimed record retained");
            record.claimed = false;
            record.retiring = true;
            record.transition(JobStatus::Consumed);
            self.lease.shared.changed.notify_all();
        }
        // Move metadata, retain input/staging reservations until retirement.
        let artifacts = std::mem::take(&mut self.artifacts);
        AppliedAssets {
            artifacts,
            cleanup: Some(Cleanup {
                shared: self.lease.shared.clone(),
                id: self.lease.id,
                task: None,
                proposal: Some(proposal),
                terminal: JobStatus::Consumed,
                failure: None,
                work: self.lease.work.take(),
            }),
        }
    }
}

/// Persistent source metadata plus control-side retirement. The persistent
/// files outlive this value, job cancellation, history changes and shutdown.
pub struct AppliedAssets {
    artifacts: Vec<ArtifactMetadata>,
    cleanup: Option<Cleanup>,
}
impl AppliedAssets {
    pub fn artifacts(&self) -> &[ArtifactMetadata] {
        &self.artifacts
    }
    pub fn retire(mut self) -> Vec<ArtifactMetadata> {
        drop(self.cleanup.take());
        std::mem::take(&mut self.artifacts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioShape, ContentFingerprint, FrameRange, ModelProvenance, OutputRole};
    use std::sync::Barrier;
    use windfall_core::AudioBuffer;
    struct UnitAdapter {
        entered: Barrier,
        release: Barrier,
        expire: std::sync::atomic::AtomicBool,
    }
    impl AnalysisAdapter for UnitAdapter {
        fn id(&self) -> &str {
            "test-only-unit"
        }
        fn version(&self) -> &str {
            "1"
        }
        fn run(
            &self,
            input: &CapturedInput,
            _: &VerifiedModel,
            context: &mut WorkerContext,
            writer: &mut ArtifactWriter,
        ) -> Result<()> {
            self.entered.wait();
            self.release.wait();
            if self.expire.load(std::sync::atomic::Ordering::SeqCst) {
                context.work.expire_at_checkpoint();
            }
            context.checkpoint(1)?;
            writer.start_audio(RelativeName::new("unit.wav")?, "unit", context.work())?;
            writer.write_audio(input.selected_samples(), context.work())?;
            writer.finish_audio(context.work())
        }
    }
    struct UnitRelease(Option<Arc<UnitAdapter>>);
    impl Drop for UnitRelease {
        fn drop(&mut self) {
            if let Some(adapter) = self.0.take() {
                adapter.release.wait();
            }
        }
    }
    fn setup() -> (TempDir, JobManager, Arc<UnitAdapter>, AudioBuffer) {
        setup_with_limits(None)
    }
    fn setup_with_limits(
        limits: Option<(u64, usize)>,
    ) -> (TempDir, JobManager, Arc<UnitAdapter>, AudioBuffer) {
        use sha2::{Digest, Sha256};
        let root = tempfile::tempdir().unwrap();
        let bytes = b"authored unit fixture only";
        let model = ModelManifest {
            onnx: None,
            id: "test-only-unit".into(),
            version: "1".into(),
            revision: 1,
            sha256: Sha256::digest(bytes).into(),
            bytes: bytes.len() as u64,
            max_bytes: 128,
            provenance: ModelProvenance {
                origin: "authored:unit".into(),
                source_revision: "1".into(),
                author: "Windfall contributors".into(),
                license_spdx: "CC0-1.0".into(),
                license_reference: "authored tests".into(),
                adapter_id: "test-only-unit".into(),
                adapter_version: "1".into(),
                device: "cpu".into(),
            },
            sample_rate: 48_000,
            input_channels: 1,
            max_input_frames: 4,
            outputs: vec![OutputRole {
                role: "unit".into(),
                channels: 1,
            }],
        };
        let cache =
            ModelCache::new(root.path().join("models"), ModelCacheLimits::default()).unwrap();
        cache
            .import_reader(
                &model,
                std::io::Cursor::new(bytes),
                &mut Work::new(CancelToken::default(), 10_000, Duration::from_secs(30)).unwrap(),
            )
            .unwrap();
        let adapter = Arc::new(UnitAdapter {
            entered: Barrier::new(2),
            release: Barrier::new(2),
            expire: std::sync::atomic::AtomicBool::new(false),
        });
        let mut config =
            ManagerConfig::bounded(root.path().join("staging"), root.path().join("published"));
        config.workers = 1;
        if let Some((disk, files)) = limits {
            config.disk_bytes = disk;
            config.max_published_files = files;
            fs::create_dir_all(&config.published_root).unwrap();
            fs::write(config.published_root.join("competitor.wav"), b"competitor").unwrap();
        }
        let manager = JobManager::new(config, cache, adapter.clone()).unwrap();
        let audio = AudioBuffer::from_interleaved(48_000, 1, vec![0.0, 0.25]);
        // Keep the manifest in a helper request; reconstructing it is bounded.
        (root, manager, adapter, audio)
    }
    fn unit_request(manager: &JobManager, audio: &AudioBuffer) -> JobRequest {
        use sha2::{Digest, Sha256};
        let bytes = b"authored unit fixture only";
        let mut work = Work::new(CancelToken::default(), 10_000, Duration::from_secs(30)).unwrap();
        let capture = CaptureStamp {
            generation: 1,
            edit_revision: 1,
            source_key: 1,
            binding: [0; 32],
            source_identity: audio.identity(),
            source_fingerprint: ContentFingerprint::audio(audio, &mut work).unwrap(),
            input_shape: AudioShape::of(audio),
            selection: FrameRange { start: 0, end: 2 },
        };
        let model = ModelManifest {
            onnx: None,
            id: "test-only-unit".into(),
            version: "1".into(),
            revision: 1,
            sha256: Sha256::digest(bytes).into(),
            bytes: bytes.len() as u64,
            max_bytes: 128,
            provenance: ModelProvenance {
                origin: "authored:unit".into(),
                source_revision: "1".into(),
                author: "Windfall contributors".into(),
                license_spdx: "CC0-1.0".into(),
                license_reference: "authored tests".into(),
                adapter_id: manager.shared.adapter.id().into(),
                adapter_version: "1".into(),
                device: "cpu".into(),
            },
            sample_rate: 48_000,
            input_channels: 1,
            max_input_frames: 4,
            outputs: vec![OutputRole {
                role: "unit".into(),
                channels: 1,
            }],
        };
        JobRequest {
            amount: 1.0,
            task: Default::default(),
            input: CapturedInput::capture(audio, capture, InputLimits::default(), &mut work)
                .unwrap(),
            model,
            budget: JobBudget {
                scratch_bytes: 0,
                output_bytes: 4096,
                work_units: 10_000,
                timeout: Duration::from_secs(30),
            },
        }
    }
    #[test]
    fn counters_fail_before_rollover_without_reusing_ids() {
        let mut counter = u64::MAX - 1;
        assert_eq!(next(&mut counter).unwrap(), u64::MAX - 1);
        assert!(matches!(next(&mut counter), Err(AnalysisError::Exhausted)));
        assert_eq!(counter, u64::MAX);
    }
    #[test]
    #[cfg(windows)]
    fn r1_locked_publication_temporary_retains_quota_until_explicit_cleanup() {
        use std::cell::RefCell;
        use std::os::windows::fs::OpenOptionsExt;
        use std::rc::Rc;
        let (_root, manager, adapter, audio) = setup_with_limits(Some((12298, 3)));
        let id = manager.submit(unit_request(&manager, &audio)).unwrap();
        adapter.entered.wait();
        drop(UnitRelease(Some(adapter)));
        let ready = manager.wait(id).unwrap();
        let current = unit_request(&manager, &audio);
        let target = manager.shared.config.published_root.join("competitor.wav");
        fs::write(&target, b"competitor").unwrap();
        let held = Rc::new(RefCell::new(None));
        let copy = held.clone();
        crate::artifacts::PUBLICATION_TEST_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |path| {
                *copy.borrow_mut() = Some(
                    fs::OpenOptions::new()
                        .read(true)
                        .share_mode(3)
                        .open(path)
                        .unwrap(),
                );
            }));
        });
        let result = manager
            .claim(ready.ticket, current.input.stamp(), &current.model)
            .unwrap()
            .prepare(vec![RelativeName::new("competitor.wav").unwrap()]);
        crate::artifacts::PUBLICATION_TEST_HOOK.with(|hook| {
            hook.borrow_mut().take();
        });
        assert!(matches!(result, Err(AnalysisError::Collision)));
        assert_eq!(
            fs::read_dir(&manager.shared.config.published_root)
                .unwrap()
                .count(),
            2
        );
        for _ in 0..2 {
            let refused = manager
                .claim(ready.ticket, current.input.stamp(), &current.model)
                .unwrap()
                .prepare(vec![RelativeName::new("competitor.wav").unwrap()]);
            assert!(matches!(refused, Err(AnalysisError::Io(_))));
            assert_eq!(manager.snapshot(id).unwrap().status, JobStatus::Ready);
            assert_eq!(
                fs::read_dir(&manager.shared.config.published_root)
                    .unwrap()
                    .count(),
                2
            );
            assert_eq!(manager.usage().reserved_disk_bytes, 12288);
        }
        manager.cancel(id).unwrap();
        assert_eq!(manager.usage().reserved_disk_bytes, 12288);
        assert_eq!(manager.usage().reserved_output_files, 2);
        for _ in 0..4 {
            assert_eq!(manager.cancel(id).unwrap(), JobStatus::Cancelled);
            assert!(matches!(manager.forget(id), Err(AnalysisError::Budget(_))));
            assert!(matches!(
                manager.submit(unit_request(&manager, &audio)),
                Err(AnalysisError::Budget(_))
            ));
            assert!(manager.retry_cleanup(id).is_err());
            assert_eq!(
                manager.usage().reserved_disk_bytes + manager.usage().published_disk_bytes,
                12298
            );
            assert_eq!(
                manager.usage().reserved_output_files + manager.usage().published_files,
                3
            );
            assert_eq!(
                fs::read_dir(&manager.shared.config.published_root)
                    .unwrap()
                    .count(),
                2
            );
        }
        held.borrow_mut().take();
        manager.retry_cleanup(id).unwrap();
        assert_eq!(manager.usage().reserved_disk_bytes, 0);
        assert_eq!(manager.usage().reserved_output_files, 0);
        manager.forget(id).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"competitor");
        assert_eq!(
            fs::read_dir(&manager.shared.config.published_root)
                .unwrap()
                .count(),
            1
        );
        eprintln!(
            "R1_CLEANUP ready_retries_blocked=2 cancelled_refusals=4 reserved_bytes=12288 reserved_files=2 competitor_bytes=10 competitor_files=1 after_retry_reserved_bytes=0 after_retry_reserved_files=0"
        );
    }
    #[test]
    fn r1_artifact_case_alias_preserves_actual_first_pcm_and_checksum() {
        use sha2::{Digest, Sha256};
        let (_root, manager, _adapter, audio) = setup();
        let mut model = unit_request(&manager, &audio).model;
        model.outputs.push(OutputRole {
            role: "second".into(),
            channels: 1,
        });
        let mut writer =
            ArtifactWriter::new(&manager.shared.config.staging_root, &model, 2, 0, 4096).unwrap();
        let mut work = Work::new(CancelToken::default(), 10000, Duration::from_secs(30)).unwrap();
        writer
            .start_audio(RelativeName::new("stem.wav").unwrap(), "unit", &mut work)
            .unwrap();
        writer.write_audio(&[0.25, -0.5], &mut work).unwrap();
        writer.finish_audio(&mut work).unwrap();
        let first = writer.path().join("stem.wav");
        let before = fs::read(&first).unwrap();
        assert!(matches!(
            writer.start_audio(RelativeName::new("STEM.wav").unwrap(), "second", &mut work),
            Err(AnalysisError::Invalid(_))
        ));
        assert_eq!(fs::read(&first).unwrap(), before);
        assert_eq!(
            Sha256::digest(&before),
            Sha256::digest(fs::read(&first).unwrap())
        );
        assert_eq!(
            windfall_codec::decode_file_strict_with(
                &first,
                &windfall_codec::DecodeOptions {
                    max_decoded_bytes: 8
                }
            )
            .unwrap()
            .samples(),
            &[0.25, -0.5]
        );
        assert_eq!(fs::read_dir(writer.path()).unwrap().count(), 1);
        writer
            .start_audio(RelativeName::new("other.wav").unwrap(), "second", &mut work)
            .unwrap();
        writer.write_audio(&[0.125, 0.125], &mut work).unwrap();
        writer.finish_audio(&mut work).unwrap();
        let (directory, artifacts) = writer.complete(&mut work).unwrap();
        assert_eq!(
            artifacts[0].metadata.sha256,
            <[u8; 32]>::from(Sha256::digest(&before))
        );
        assert_eq!(fs::read(&artifacts[0].staged).unwrap(), before);
        drop(directory);
        assert_eq!(
            fs::read_dir(&manager.shared.config.staging_root)
                .unwrap()
                .count(),
            0
        );
    }
    #[test]
    fn expired_queued_deadline_is_checked_before_adapter_and_releases_owned_resources() {
        let (_root, manager, adapter, audio) = setup();
        let running = manager.submit(unit_request(&manager, &audio)).unwrap();
        adapter.entered.wait();
        let release = UnitRelease(Some(adapter.clone()));
        let queued = manager.submit(unit_request(&manager, &audio)).unwrap();
        lock(&manager.shared.state)
            .records
            .get_mut(&queued)
            .unwrap()
            .task
            .as_mut()
            .unwrap()
            .deadline = Instant::now();
        drop(release);
        assert_eq!(manager.wait(running).unwrap().status, JobStatus::Ready);
        let failure = manager.wait(queued).unwrap();
        assert_eq!(failure.status, JobStatus::Failed);
        assert!(failure.failure.unwrap().contains("deadline"));
        assert_eq!(failure.completed_work, 0);
        manager.cancel(running).unwrap();
        assert_eq!(manager.usage().reserved_disk_bytes, 0);
        assert_eq!(manager.usage().active, 0);
    }
    #[test]
    fn native_progress_exhaustion_terminates_without_wrapping_or_leaking_staging() {
        let (_root, manager, adapter, audio) = setup();
        let running = manager.submit(unit_request(&manager, &audio)).unwrap();
        adapter.entered.wait();
        let release = UnitRelease(Some(adapter.clone()));
        lock(&manager.shared.state)
            .records
            .get_mut(&running)
            .unwrap()
            .snapshot
            .sequence = u64::MAX - 16;
        drop(release);
        let failure = manager.wait(running).unwrap();
        assert_eq!(failure.status, JobStatus::Failed);
        assert!(failure.failure.unwrap().contains("exhausted"));
        assert_eq!(failure.sequence, u64::MAX - 15);
        assert_eq!(manager.usage().reserved_disk_bytes, 0);
        assert_eq!(
            fs::read_dir(&manager.shared.config.staging_root)
                .unwrap()
                .count(),
            0
        );
    }
    #[test]
    fn job_ticket_request_exhaustion_refuses_actual_admission_atomically() {
        let (_root, manager, _adapter, audio) = setup();
        for counters in [
            Counters {
                job: u64::MAX,
                ticket: 1,
                request: 1,
            },
            Counters {
                job: 1,
                ticket: u64::MAX,
                request: 1,
            },
            Counters {
                job: 1,
                ticket: 1,
                request: u64::MAX,
            },
        ] {
            lock(&manager.shared.state).counters = Some(counters);
            assert!(matches!(
                manager.submit(unit_request(&manager, &audio)),
                Err(AnalysisError::Exhausted)
            ));
            assert_eq!(manager.usage().memory_bytes, 0);
            assert_eq!(manager.usage().reserved_disk_bytes, 0);
            assert_eq!(manager.usage().retained_jobs, 0);
        }
    }
    #[test]
    fn running_deadline_fails_at_bounded_checkpoint_without_sleep_or_stale_ready() {
        let (_root, manager, adapter, audio) = setup();
        let id = manager.submit(unit_request(&manager, &audio)).unwrap();
        adapter.entered.wait();
        let release = UnitRelease(Some(adapter.clone()));
        adapter
            .expire
            .store(true, std::sync::atomic::Ordering::SeqCst);
        drop(release);
        let failed = manager.wait(id).unwrap();
        assert_eq!(failed.status, JobStatus::Failed);
        assert!(failed.failure.unwrap().contains("deadline"));
        assert!(!failed.transitions.contains(&JobStatus::Ready));
        assert_eq!(manager.usage().reserved_disk_bytes, 0);
        assert_eq!(
            fs::read_dir(&manager.shared.config.staging_root)
                .unwrap()
                .count(),
            0
        );
    }
}
