//! Native analysis control service with built-in classical dereverberation.
//! Gate -> recording -> State is the capture/apply order. IO, hashes, decoding,
//! model/worker waits and retirement always run outside recording/State guards.
use super::{Session, State};
mod source_namespace;
mod pack_registry;
use crate::sync::lock;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windfall_analysis as native;
use windfall_engine::{ProjectPublicationIntent, SamplePool};
use windfall_ipc::{
    AnalysisApply, AnalysisArtifact, AnalysisCapability, AnalysisJob, AnalysisModel,
    AnalysisOutputRole, AnalysisProvenance, AnalysisReview, AnalysisStatus, AnalysisSubmit,
};
use windfall_project::{
    Clip, ClipContent, ClipInit, Command, DispatchResult, MAX_SONG_TICKS,
    SampleAsset, SampleId, SamplePath,
};

pub(super) const UNAVAILABLE: &str = "Stem separation/remix require an imported licensed ONNX pack and an installed ONNX Runtime adapter. Pack registration does not install executable code. Classical dereverb is built in.";
const PCM_LIMIT: u64 = 16 * 1024 * 1024;
const OUTPUT_PCM_LIMIT: u64 = 32 * 1024 * 1024;
const MODEL_LIMIT: u64 = 64 * 1024 * 1024;
const JOBS: usize = 8;

fn error(kind: &str, message: impl std::fmt::Display) -> String {
    format!("analysis:{kind}: {message}")
}
fn native_error(error: native::AnalysisError) -> String {
    let kind = match error {
        native::AnalysisError::Cancelled => "cancelled",
        native::AnalysisError::Stale => "stale",
        native::AnalysisError::ModelAbsent => "modelAbsent",
        native::AnalysisError::Budget(_) => "budget",
        native::AnalysisError::Claimed => "claimed",
        native::AnalysisError::Shutdown => "shutdown",
        native::AnalysisError::Io(_) | native::AnalysisError::Codec(_) => "io",
        native::AnalysisError::Deadline => "deadline",
        native::AnalysisError::Exhausted => "exhausted",
        native::AnalysisError::Collision => "collision",
        _ => "invalid",
    };
    self::error(kind, error)
}
fn number(text: &str) -> Result<u64, String> {
    let value = text
        .parse::<u64>()
        .map_err(|_| error("invalid", "Expected a decimal identifier or count."))?;
    if value.to_string() != text {
        return Err(error("invalid", "Decimal strings must be canonical."));
    }
    Ok(value)
}
fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn digest(text: &str) -> Result<[u8; 32], String> {
    if text.len() != 64
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(error(
            "invalid",
            "SHA256 must be 64 lowercase hexadecimal digits.",
        ));
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|e| error("invalid", e))?;
    }
    Ok(bytes)
}
fn work(cancel: native::CancelToken) -> Result<native::Work, String> {
    native::Work::new(cancel, 500_000_000, Duration::from_secs(120)).map_err(native_error)
}
fn input_limits() -> native::InputLimits {
    native::InputLimits {
        max_bytes: PCM_LIMIT,
        ..Default::default()
    }
}
fn manifest(spec: AnalysisModel) -> Result<native::ModelManifest, String> {
    let p = spec.provenance;
    if spec.id == native::BUILTIN_DEREVERB_ID { return Err(error("invalid", "Built-in dereverb is code, not an imported model pack.")); }
    let model = native::ModelManifest {
        onnx: spec.onnx.map(onnx_native),
        id: spec.id,
        version: spec.version,
        revision: number(&spec.revision)?,
        sha256: digest(&spec.sha256)?,
        bytes: number(&spec.bytes)?,
        max_bytes: number(&spec.max_bytes)?,
        provenance: native::ModelProvenance {
            origin: p.origin,
            source_revision: p.source_revision,
            author: p.author,
            license_spdx: p.license_spdx,
            license_reference: p.license_reference,
            adapter_id: p.adapter_id,
            adapter_version: p.adapter_version,
            device: p.device,
        },
        sample_rate: spec.sample_rate,
        input_channels: spec.input_channels,
        max_input_frames: number(&spec.max_input_frames)?,
        outputs: spec
            .outputs
            .into_iter()
            .map(|o| native::OutputRole {
                role: o.role,
                channels: o.channels,
            })
            .collect(),
    };
    model.validate().map_err(native_error)?;
    if model.max_bytes > MODEL_LIMIT {
        return Err(error("budget", "Local model limit is 64 MiB."));
    }
    // Compact incoming IPC storage before retaining it in the bounded registry.
    Ok(model.clone())
}
fn model_wire(model: &native::ModelManifest) -> AnalysisModel {
    let p = &model.provenance;
    AnalysisModel {
        onnx: model.onnx.as_ref().map(onnx_wire),
        id: model.id.clone(),
        version: model.version.clone(),
        revision: model.revision.to_string(),
        sha256: hex(&model.sha256),
        bytes: model.bytes.to_string(),
        max_bytes: model.max_bytes.to_string(),
        provenance: AnalysisProvenance {
            origin: p.origin.clone(),
            source_revision: p.source_revision.clone(),
            author: p.author.clone(),
            license_spdx: p.license_spdx.clone(),
            license_reference: p.license_reference.clone(),
            adapter_id: p.adapter_id.clone(),
            adapter_version: p.adapter_version.clone(),
            device: p.device.clone(),
        },
        sample_rate: model.sample_rate,
        input_channels: model.input_channels,
        max_input_frames: model.max_input_frames.to_string(),
        outputs: model
            .outputs
            .iter()
            .map(|o| AnalysisOutputRole {
                role: o.role.clone(),
                channels: o.channels,
            })
            .collect(),
    }
}
fn onnx_native(o: windfall_ipc::AnalysisOnnxContract) -> native::OnnxContract {
    fn tensors(items: Vec<windfall_ipc::AnalysisTensorSpec>) -> Vec<native::TensorSpec> {
        items.into_iter().map(|t| native::TensorSpec { name: t.name, shape: t.shape.into_iter().map(|d| match d {
            windfall_ipc::AnalysisTensorDimension::Fixed(n) => native::TensorDimension::Fixed(n),
            windfall_ipc::AnalysisTensorDimension::Symbol(s) => native::TensorDimension::Symbol(s),
        }).collect() }).collect()
    }
    native::OnnxContract { opset: o.opset, inputs: tensors(o.inputs), outputs: tensors(o.outputs), sample_rate: o.sample_rate, segment_frames: o.segment_frames, overlap_frames: o.overlap_frames, stem_roles: o.stem_roles }
}
fn onnx_wire(o: &native::OnnxContract) -> windfall_ipc::AnalysisOnnxContract {
    fn tensors(items: &[native::TensorSpec]) -> Vec<windfall_ipc::AnalysisTensorSpec> {
        items.iter().map(|t| windfall_ipc::AnalysisTensorSpec { name: t.name.clone(), shape: t.shape.iter().map(|d| match d {
            native::TensorDimension::Fixed(n) => windfall_ipc::AnalysisTensorDimension::Fixed(*n),
            native::TensorDimension::Symbol(s) => windfall_ipc::AnalysisTensorDimension::Symbol(s.clone()),
        }).collect() }).collect()
    }
    windfall_ipc::AnalysisOnnxContract { opset: o.opset, inputs: tensors(&o.inputs), outputs: tensors(&o.outputs), sample_rate: o.sample_rate, segment_frames: o.segment_frames, overlap_frames: o.overlap_frames, stem_roles: o.stem_roles.clone() }
}
fn job_wire(job: native::JobSnapshot) -> AnalysisJob {
    AnalysisJob {
        job: job.id.0.to_string(),
        ticket: job.ticket.0.to_string(),
        request: job.request.to_string(),
        sequence: job.sequence.to_string(),
        status: match job.status {
            native::JobStatus::Queued => AnalysisStatus::Queued,
            native::JobStatus::Running => AnalysisStatus::Running,
            native::JobStatus::Cancelling => AnalysisStatus::Cancelling,
            native::JobStatus::Cancelled => AnalysisStatus::Cancelled,
            native::JobStatus::Failed => AnalysisStatus::Failed,
            native::JobStatus::Ready => AnalysisStatus::Ready,
            native::JobStatus::Consumed => AnalysisStatus::Consumed,
        },
        completed_work: job.completed_work.to_string(),
        maximum_work: job.maximum_work.to_string(),
        failure: job.failure,
    }
}
fn task_native(task: windfall_ipc::AnalysisTask) -> native::InferenceTask {
    match task {
        windfall_ipc::AnalysisTask::Audio => native::InferenceTask::Audio,
        windfall_ipc::AnalysisTask::StemSeparation => native::InferenceTask::StemSeparation,
        windfall_ipc::AnalysisTask::DereverbGeneral => native::InferenceTask::DereverbGeneral,
        windfall_ipc::AnalysisTask::DereverbVocal => native::InferenceTask::DereverbVocal,
    }
}
fn task_wire(task: native::InferenceTask) -> windfall_ipc::AnalysisTask {
    match task {
        native::InferenceTask::Audio => windfall_ipc::AnalysisTask::Audio,
        native::InferenceTask::StemSeparation => windfall_ipc::AnalysisTask::StemSeparation,
        native::InferenceTask::DereverbGeneral => windfall_ipc::AnalysisTask::DereverbGeneral,
        native::InferenceTask::DereverbVocal => windfall_ipc::AnalysisTask::DereverbVocal,
    }
}

struct Owned {
    registry_loaded: bool,
    cache: Option<native::ModelCache>,
    manager: Option<Arc<native::JobManager>>,
    models: BTreeMap<(String, String), native::ModelManifest>,
    bindings: BTreeMap<native::JobId, Binding>,
    adapter: Option<Arc<dyn native::AnalysisAdapter>>,
    stage: Option<PathBuf>,
    closed: bool,
}
fn current_model(owned: &Owned, review: &native::Review) -> Result<native::ModelManifest, String> {
    if review.model.is_builtin_dereverb() && owned.adapter.as_ref().is_some_and(|a| a.id() == native::BUILTIN_DEREVERB_ID) {
        return native::ClassicalDereverbAdapter::descriptor(review.capture.input_shape, review.task).map_err(native_error);
    }
    owned.models.get(&(review.model.id.clone(), review.model.version.clone())).cloned()
        .ok_or_else(|| error("modelAbsent", "Pinned model is missing."))
}
pub(super) struct Service {
    root: PathBuf,
    gate: Mutex<()>,
    owned: Mutex<Owned>,
    preparation: Mutex<Option<native::CancelToken>>,
    #[cfg(test)]
    final_calls: Mutex<Option<(usize, usize)>>,
}
impl Service {
    pub(super) fn new(root: PathBuf) -> Self {
        Self {
            root,
            gate: Mutex::new(()),
            owned: Mutex::new(Owned {
                registry_loaded: false,
                cache: None,
                manager: None,
                models: BTreeMap::new(),
                bindings: BTreeMap::new(),
                adapter: Some(Arc::new(native::ClassicalDereverbAdapter)),
                stage: None,
                closed: false,
            }),
            preparation: Mutex::new(None),
            #[cfg(test)]
            final_calls: Mutex::new(None),
        }
    }
    fn cache(&self) -> Result<native::ModelCache, String> {
        if let Some(cache) = &lock(&self.owned).cache {
            return Ok(cache.clone());
        }
        let cache = native::ModelCache::new(
            self.root.join("Models"),
            native::ModelCacheLimits {
                max_model_bytes: MODEL_LIMIT,
                max_cache_bytes: 256 * 1024 * 1024,
                max_entries: 8,
                max_imports: 1,
            },
        )
        .map_err(native_error)?;
        lock(&self.owned).cache = Some(cache.clone());
        Ok(cache)
    }
    fn manager(&self) -> Result<Arc<native::JobManager>, String> {
        let owned = lock(&self.owned);
        owned
            .manager
            .clone()
            .ok_or_else(|| error("unavailable", UNAVAILABLE))
    }
    fn ensure_manager(&self) -> Result<Arc<native::JobManager>, String> {
        if lock(&self.owned).closed {
            return Err(error("shutdown", "Analysis service is shut down."));
        }
        if let Some(manager) = &lock(&self.owned).manager {
            return Ok(manager.clone());
        }
        let adapter = lock(&self.owned)
            .adapter
            .clone()
            .ok_or_else(|| error("unavailable", UNAVAILABLE))?;
        let cache = self.cache()?;
        std::fs::create_dir_all(&self.root).map_err(|e| error("io", e))?;
        let stage = tempfile::Builder::new()
            .prefix("Session-")
            .tempdir_in(&self.root)
            .map_err(|e| error("io", e))?;
        let mut config =
            native::ManagerConfig::bounded(stage.path().join("Jobs"), self.root.join("Outputs"));
        config.workers = 1;
        config.max_queued = 2;
        config.max_retained_jobs = JOBS;
        config.memory_bytes = 192 * 1024 * 1024;
        config.max_work_units = 100_000_000_000;
        config.input_limits = input_limits();
        let manager =
            Arc::new(native::JobManager::new(config, cache, adapter).map_err(native_error)?);
        let mut owned = lock(&self.owned);
        owned.manager = Some(manager.clone());
        owned.stage = Some(stage.keep());
        Ok(manager)
    }
    fn begin_preparation(&self) -> Preparation<'_> {
        let cancel = native::CancelToken::default();
        *lock(&self.preparation) = Some(cancel.clone());
        Preparation {
            service: self,
            cancel,
        }
    }
    pub(super) fn shutdown(&self) {
        self.cancel_preparation();
        let _gate = lock(&self.gate);
        let manager = {
            let mut owned = lock(&self.owned);
            owned.closed = true;
            owned.manager.clone()
        };
        if let Some(manager) = manager {
            manager.shutdown();
        }
        // Never recursively remove a parent around refused job cleanup. Only
        // empty containers can retire, preserving evidence and competitors.
        if let Some(stage) = &lock(&self.owned).stage {
            let _ = std::fs::remove_dir(stage.join("Jobs"));
            let _ = std::fs::remove_dir(stage);
        }
    }
    fn cancel_preparation(&self) {
        if let Some(cancel) = &*lock(&self.preparation) {
            cancel.cancel();
        }
    }
    #[cfg(test)]
    pub(super) fn install_test_adapter(&self, adapter: Arc<dyn native::AnalysisAdapter>) {
        let _gate = lock(&self.gate);
        let mut owned = lock(&self.owned);
        assert!(owned.manager.is_none());
        owned.adapter = Some(adapter);
    }
    #[cfg(test)]
    pub(super) fn test_manager(&self) -> Arc<native::JobManager> {
        self.manager().expect("test installed a real manager")
    }
    #[cfg(test)]
    pub(super) fn final_calls(&self) -> (usize, usize) {
        lock(&self.final_calls).expect("an actual guarded commit occurred")
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.shutdown();
    }
}
struct Preparation<'a> {
    service: &'a Service,
    cancel: native::CancelToken,
}
impl Drop for Preparation<'_> {
    fn drop(&mut self) {
        lock(&self.service.preparation).take();
    }
}

#[derive(Clone)]
struct Binding {
    clip: Clip,
    sample: SampleAsset,
    path: PathBuf,
    project_path: Option<PathBuf>,
    sample_dir: Option<PathBuf>,
    replacements: u64,
    stamp: native::CaptureStamp,
    file_identity: u64,
    tempo: f64,
    clock: windfall_engine::SongClock,
}
impl Binding {
    fn current(&self, state: &State) -> bool {
        state.generation == self.stamp.generation
            && state.edits == self.stamp.edit_revision
            && state.replacements == self.replacements
            && state.path == self.project_path
            && state.sample_dir == self.sample_dir
            && state.document.project().settings.tempo_bpm == self.tempo
            && state
                .document
                .project()
                .playlist
                .clips
                .iter()
                .any(|clip| clip == &self.clip)
            && state.document.project().sample(self.sample.id) == Some(&self.sample)
            && state
                .pool
                .get(self.sample.id)
                .is_some_and(|audio| audio.identity() == self.stamp.source_identity)
    }
}
// Windows retains the effective direct-volume DOS object, root-to-leaf ordinary
// namespace and file authority throughout the final guarded check. Reparse and
// substituted/chained drive paths fail closed before publication.
// Other platforms retain exact immutable loaded-source guards;
// external filesystem writes after the off-State recheck are not interprocess
// transactions there, and are explicitly outside the saved document mutation.
fn source_file(path: &Path) -> Result<source_namespace::SourceFile, String> {
    source_namespace::open(path)
}
fn file_bytes(file: &mut impl Read, work: &mut native::Work) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 16384];
    loop {
        work.check().map_err(native_error)?;
        let n = file.read(&mut chunk).map_err(|e| error("io", e))?;
        if n == 0 {
            break;
        }
        if bytes
            .len()
            .checked_add(n)
            .is_none_or(|len| len as u64 > PCM_LIMIT)
        {
            return Err(error("budget", "Encoded source limit is 16 MiB."));
        }
        work.checkpoint(n as u64).map_err(native_error)?;
        bytes.extend_from_slice(&chunk[..n]);
    }
    Ok(bytes)
}

impl Session {
    pub fn analysis_capability(&self) -> AnalysisCapability {
        let service = &self.inner.analysis;
        // Capability must remain nonblocking while shutdown joins a worker.
        let registry_error = if let Ok(_gate) = service.gate.try_lock() {
            if lock(&service.owned).closed { None } else { service.restore_packs().err() }
        } else { None };
        let owned = lock(&service.owned);
        let available = owned.adapter.is_some() && !owned.closed;
        AnalysisCapability {
            native: true,
            classical_dereverb: owned.adapter.as_ref().is_some_and(|a| a.id() == native::BUILTIN_DEREVERB_ID) && !owned.closed,
            available,
            reason: registry_error.or_else(|| (!available).then(|| UNAVAILABLE.into())),
            models: owned.models.values().map(model_wire).collect(),
        }
    }
    pub fn analysis_cancel_preparation(&self) {
        self.inner.analysis.cancel_preparation();
    }
    pub fn analysis_shutdown(&self) {
        self.inner.analysis.shutdown();
    }
    pub fn analysis_model_import(
        &self,
        path: &str,
        spec: AnalysisModel,
    ) -> Result<AnalysisModel, String> {
        let service = &self.inner.analysis;
        let _gate = lock(&service.gate);
        if lock(&service.owned).closed {
            return Err(error("shutdown", "Analysis service is shut down."));
        }
        service.restore_packs()?;
        let model = manifest(spec)?;
        let key = (model.id.clone(), model.version.clone());
        {
            let owned = lock(&service.owned);
            if owned.models.len() >= 8 && !owned.models.contains_key(&key) {
                return Err(error("budget", "At most eight pinned models."));
            }
            if let Some(old) = owned.models.get(&key)
                && (model.revision < old.revision
                    || (model.revision == old.revision && *old != model))
            {
                return Err(error(
                    "stale",
                    "A changed model requires a newer explicit revision.",
                ));
            }
        }
        if path.len() > 2048 {
            return Err(error("invalid", "Model path is too long."));
        }
        let preparation = service.begin_preparation();
        service
            .cache()?
            .import_file(
                &model,
                Path::new(path),
                &mut work(preparation.cancel.clone())?,
            )
            .map_err(native_error)?;
        let response = model_wire(&model);
        let mut models = lock(&service.owned).models.clone();
        models.insert(key, model);
        service.persist_packs(&models)?;
        lock(&service.owned).models = models;
        Ok(response)
    }
    pub fn analysis_submit(&self, request: AnalysisSubmit) -> Result<AnalysisJob, String> {
        let service = &self.inner.analysis;
        let _gate = lock(&service.gate);
        let builtin = request.model_id == native::BUILTIN_DEREVERB_ID;
        if !builtin { service.restore_packs()?; }
        let manager = service.ensure_manager()?;
        let amount = request.amount.unwrap_or(1.0);
        if !amount.is_finite() || !(0.0..=1.0).contains(&amount) { return Err(error("invalid", "Wet/dry amount must be 0 to 1.")); }
        let imported = if builtin {
            if request.model_version != "1" || number(&request.model_revision)? != 1 { return Err(error("stale", "Built-in algorithm revision changed.")); }
            None
        } else { Some(lock(&service.owned).models.get(&(request.model_id, request.model_version)).cloned()
            .ok_or_else(|| error("modelAbsent", "Explicitly import this pinned local model."))?) };
        if let Some(model) = &imported {
            let owned = lock(&service.owned);
            if !owned.adapter.as_ref().is_some_and(|a| a.id() == model.provenance.adapter_id && a.version() == model.provenance.adapter_version && a.supports(task_native(request.task))) {
                return Err(error("unavailable", UNAVAILABLE));
            }
        }
        let selection = native::FrameRange {
            start: number(&request.start_frame)?,
            end: number(&request.end_frame)?,
        };
        let preparation = service.begin_preparation();
        let mut work = work(preparation.cancel.clone())?;
        let (clip, sample, source, generation, edits, replacements, project_path, directory, project) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let project = state.document.project();
            let clip = project
                .playlist
                .clips
                .iter()
                .find(|c| c.id == request.clip)
                .cloned()
                .ok_or_else(|| error("stale", "The audio clip no longer exists."))?;
            let id = clip
                .content
                .sample()
                .ok_or_else(|| error("invalid", "Select an audio clip."))?;
            let sample = project
                .sample(id)
                .cloned()
                .ok_or_else(|| error("stale", "The source is missing."))?;
            let source = state
                .pool
                .get(id)
                .cloned()
                .ok_or_else(|| error("invalid", "Reload the source first."))?;
            (
                clip,
                sample,
                source,
                state.generation,
                state.edits,
                state.replacements,
                state.path.clone(),
                state.sample_dir.clone(),
                project.clone(),
            )
        };
        if native::AudioShape::of(&source)
            .pcm_bytes()
            .map_err(native_error)?
            > PCM_LIMIT
        {
            return Err(error("budget", "Loaded source limit is 16 MiB."));
        }
        let path = super::samples::locate(&sample, directory.as_deref(), &self.inner.factory_dir)?;
        let (SamplePath::External(stored)
        | SamplePath::Project(stored)
        | SamplePath::Factory(stored)) = &sample.path;
        if path.as_os_str().len() > 1024 || sample.name.len() > 1024 || stored.len() > 1024 {
            return Err(error("budget", "Source metadata is too large."));
        }
        let mut file = source_file(&path)?;
        let bytes = file_bytes(&mut file, &mut work)?;
        let fingerprint =
            native::ContentFingerprint::reader(std::io::Cursor::new(&bytes), PCM_LIMIT, &mut work)
                .map_err(native_error)?;
        let decoded = windfall_codec::decode_bytes_with(
            &bytes,
            path.extension().and_then(|e| e.to_str()),
            &windfall_codec::DecodeOptions {
                max_decoded_bytes: PCM_LIMIT,
            },
        )
        .map_err(|e| error("io", e))?;
        if native::ContentFingerprint::audio(&decoded, &mut work).map_err(native_error)?
            != native::ContentFingerprint::audio(&source, &mut work).map_err(native_error)?
        {
            return Err(error(
                "stale",
                "The source file changed; reload its audio first.",
            ));
        }
        drop(decoded);
        drop(bytes);
        let mut render_pool = SamplePool::new();
        render_pool.insert(sample.id, source.clone());
        let tempo = project.settings.tempo_bpm;
        let clock = windfall_engine::SongClock::new(&project)?;
        let timing = windfall_engine::audio_edit::view_timing(&render_pool, &clip, tempo, &clock)?;
        // Preflight the existing render helper before any derived PCM allocation.
        // Its broader editor bound is intentionally left unchanged.
        let rendered_frames = (timing.duration_frames - 1e-6).ceil();
        let bounded_frames = |frames: f64, channels: u16| {
            frames.is_finite()
                && frames >= 1.0
                && frames * f64::from(channels) * 4.0 <= PCM_LIMIT as f64
        };
        if !bounded_frames(rendered_frames, 2) {
            return Err(error("budget", "Rendered clip limit is 16 MiB."));
        }
        if let ClipContent::Audio {
            stretch: windfall_project::ClipStretch::Spectral { ratio, .. },
            ..
        } = clip.content
            && !bounded_frames((source.frames() as f64 * ratio).round(), source.channels())
        {
            return Err(error("budget", "Stretched source limit is 16 MiB."));
        }
        selection
            .validate(rendered_frames as u64)
            .map_err(native_error)?;
        let shape = native::AudioShape { frames: rendered_frames as u64, channels: 2, sample_rate: source.sample_rate() };
        let model = match imported {
            Some(model) => model,
            None => native::ClassicalDereverbAdapter::descriptor(shape, task_native(request.task)).map_err(native_error)?,
        };
        if number(&request.model_revision)? != model.revision { return Err(error("stale", "Model revision changed.")); }
        model
            .validate_input(
                native::AudioShape {
                    frames: rendered_frames as u64,
                    channels: 2,
                    sample_rate: source.sample_rate(),
                },
                selection.frames().map_err(native_error)?,
            )
            .map_err(native_error)?;
        let view = windfall_engine::audio_edit::render_view_timed(&render_pool, &clip, &timing)?;
        work.check().map_err(native_error)?;
        let encoded_binding =
            serde_json::to_vec(&(&clip, &sample, tempo, timing, &directory, &project_path))
                .map_err(|e| error("invalid", e))?;
        let binding_digest = native::ContentFingerprint::reader(
            std::io::Cursor::new(encoded_binding),
            16 * 1024,
            &mut work,
        )
        .map_err(native_error)?
        .sha256;
        let stamp = native::CaptureStamp {
            generation,
            edit_revision: edits,
            source_key: u64::from(sample.id.0),
            binding: binding_digest,
            source_identity: source.identity(),
            source_fingerprint: fingerprint,
            input_shape: native::AudioShape::of(&view),
            selection,
        };
        let input = native::CapturedInput::capture(&view, stamp.clone(), input_limits(), &mut work)
            .map_err(native_error)?;
        drop(view);
        drop(render_pool);
        drop(source);
        let binding = Binding {
            clip,
            sample,
            path: path.clone(),
            project_path,
            sample_dir: directory,
            replacements,
            stamp,
            file_identity: crate::library::file_identity(&path).map_err(|e| error("io", e))?,
            tempo,
            clock,
        };
        let output_pcm = model
            .outputs
            .iter()
            .try_fold(0_u64, |total, role| {
                selection.frames().and_then(|n| {
                    n.checked_mul(u64::from(role.channels))
                        .and_then(|n| n.checked_mul(4))
                        .and_then(|n| n.checked_add(total))
                        .ok_or(native::AnalysisError::Exhausted)
                })
            })
            .map_err(native_error)?;
        if output_pcm > OUTPUT_PCM_LIMIT {
            return Err(error("budget", "Combined output PCM limit is 32 MiB."));
        }
        #[cfg(test)]
        self.pause("analysis:captured");
        {
            let _recording = self.recording_idle()?;
            if !binding.current(&self.state()) {
                return Err(error(
                    "stale",
                    "The source or project changed during capture.",
                ));
            }
        }
        work.check().map_err(native_error)?;
        let id = manager
            .submit(native::JobRequest {
                amount,
                task: task_native(request.task),
                input,
                model,
                budget: native::JobBudget {
                    scratch_bytes: if builtin { native::DEREVERB_SCRATCH_BYTES } else { OUTPUT_PCM_LIMIT },
                    output_bytes: output_pcm
                        .checked_mul(3)
                        .and_then(|n| n.checked_add(16 * 4096))
                        .ok_or_else(|| error("budget", "Output bytes overflow."))?,
                    work_units: if builtin { 100_000_000_000 } else { 500_000_000 },
                    timeout: Duration::from_secs(if builtin { 600 } else { 120 }),
                },
            })
            .map_err(native_error)?;
        lock(&service.owned).bindings.insert(id, binding);
        Ok(job_wire(manager.snapshot(id).map_err(native_error)?))
    }
    pub fn analysis_status(&self, job: &str) -> Result<AnalysisJob, String> {
        Ok(job_wire(
            self.inner
                .analysis
                .manager()?
                .snapshot(native::JobId(number(job)?))
                .map_err(native_error)?,
        ))
    }
    pub fn analysis_cancel(&self, job: &str) -> Result<AnalysisJob, String> {
        let manager = self.inner.analysis.manager()?;
        let id = native::JobId(number(job)?);
        manager.cancel(id).map_err(native_error)?;
        Ok(job_wire(manager.snapshot(id).map_err(native_error)?))
    }
    pub fn analysis_forget(&self, job: &str) -> Result<(), String> {
        let service = &self.inner.analysis;
        let _gate = lock(&service.gate);
        let id = native::JobId(number(job)?);
        service.manager()?.forget(id).map_err(native_error)?;
        lock(&service.owned).bindings.remove(&id);
        Ok(())
    }
    pub fn analysis_retry_cleanup(&self, job: &str) -> Result<(), String> {
        self.inner
            .analysis
            .manager()?
            .retry_cleanup(native::JobId(number(job)?))
            .map_err(native_error)
    }
    pub fn analysis_review(&self, ticket: &str) -> Result<AnalysisReview, String> {
        let service = &self.inner.analysis;
        let _gate = lock(&service.gate);
        let manager = service.manager()?;
        let review = manager
            .review(native::TicketId(number(ticket)?))
            .map_err(native_error)?;
        let clip = lock(&service.owned)
            .bindings
            .get(&review.job)
            .ok_or_else(|| error("stale", "This job belongs to an expired Session."))?
            .clip
            .id;
        Ok(AnalysisReview {
            amount: review.amount,
            task: task_wire(review.task),
            job: job_wire(manager.snapshot(review.job).map_err(native_error)?),
            clip,
            generation: review.capture.generation.to_string(),
            edit_revision: review.capture.edit_revision.to_string(),
            source_sha256: hex(&review.capture.source_fingerprint.sha256),
            binding_sha256: hex(&review.capture.binding),
            start_frame: review.capture.selection.start.to_string(),
            end_frame: review.capture.selection.end.to_string(),
            input_frames: review.capture.input_shape.frames.to_string(),
            model: model_wire(&review.model),
            artifacts: review
                .artifacts
                .iter()
                .map(|a| AnalysisArtifact {
                    name: a.name.clone(),
                    role: a.role.clone(),
                    frames: a.shape.frames.to_string(),
                    channels: a.shape.channels,
                    sample_rate: a.shape.sample_rate,
                    frame_origin: a.frame_origin.to_string(),
                    bytes: a.bytes.to_string(),
                    sha256: hex(&a.sha256),
                })
                .collect(),
        })
    }
    pub fn analysis_preview(&self, ticket: &str, role: &str) -> Result<(), String> {
        let preview_ticket = {
            let mut latest = lock(&self.inner.preview);
            *latest = latest.checked_add(1).ok_or_else(|| error("exhausted", "Preview tickets exhausted."))?;
            *latest
        };
        let service = &self.inner.analysis;
        let _gate = lock(&service.gate);
        let manager = service.manager()?;
        let ticket = native::TicketId(number(ticket)?);
        let review = manager.review(ticket).map_err(native_error)?;
        let (binding, model) = {
            let owned = lock(&service.owned);
            let binding = owned.bindings.get(&review.job).cloned()
                .ok_or_else(|| error("stale", "This review expired."))?;
            let model = current_model(&owned, &review)?;
            (binding,model)
        };
        { let _recording = self.recording_idle()?; if !binding.current(&self.state()) {
            return Err(error("stale","The source or project changed."));
        }}
        let preparation = service.begin_preparation();
        let mut work = work(preparation.cancel.clone())?;
        let lease = manager.claim(ticket,&binding.stamp,&model).map_err(native_error)?;
        let audio = if role == "dry-input" && review.model.is_builtin_dereverb() {
            lease.preview_input(&mut work).map_err(native_error)?
        } else { lease.preview_audio(role,&mut work).map_err(native_error)? };
        { let _recording = self.recording_idle()?; if !binding.current(&self.state()) {
            return Err(error("stale","The source or project changed."));
        }}
        work.check().map_err(native_error)?;
        let latest = lock(&self.inner.preview);
        if *latest == preview_ticket { self.controller().preview(audio); }
        Ok(())
    }
    pub fn analysis_apply(&self, request: AnalysisApply) -> Result<DispatchResult, String> {
        let service = &self.inner.analysis;
        let _gate = lock(&service.gate);
        let manager = service.manager()?;
        let ticket = native::TicketId(number(&request.ticket)?);
        let review = manager.review(ticket).map_err(native_error)?;
        if let Some(roles) = &request.selected_roles {
            if roles.is_empty() || roles.len() > 16 || roles.iter().enumerate().any(|(i, role)|
                !review.artifacts.iter().any(|a| &a.role == role) || roles[..i].contains(role)) {
                return Err(error("invalid", "Select unique roles from this exact review."));
            }
        }
        if request.separate_tracks && review.task != native::InferenceTask::StemSeparation {
            return Err(error("invalid", "Remix track layout requires a stem separation review."));
        }
        if manager.snapshot(review.job).map_err(native_error)?.request != number(&request.request)?
        {
            return Err(error("stale", "This review request is obsolete."));
        }
        let (binding, model) = {
            let owned = lock(&service.owned);
            let binding = owned
                .bindings
                .get(&review.job)
                .cloned()
                .ok_or_else(|| error("stale", "Unknown Session source."))?;
            let model = current_model(&owned, &review)?;
            (binding, model)
        };
        if request.replace_original
            && (binding.stamp.selection.start != 0
                || binding.stamp.selection.end != binding.stamp.input_shape.frames)
        {
            return Err(error(
                "invalid",
                "Replacing a clip requires its complete rendered range.",
            ));
        }
        let preparation = service.begin_preparation();
        let mut work = work(preparation.cancel.clone())?;
        let mut file = source_file(&binding.path)?;
        let current_file = native::ContentFingerprint::reader(&mut file, PCM_LIMIT, &mut work)
            .map_err(native_error)?;
        if current_file != binding.stamp.source_fingerprint
            || crate::library::file_identity(&binding.path).map_err(|e| error("io", e))?
                != binding.file_identity
        {
            return Err(error("stale", "The actual source file changed."));
        }
        let (
            mut document,
            original_pool,
            loading,
            loaded,
            failed,
            sampler_request,
            project_preparation,
        ) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if !binding.current(&state) {
                return Err(error(
                    "stale",
                    "The captured clip, source or project changed.",
                ));
            }
            (
                state.document.clone(),
                state.pool.clone(),
                state.loading.clone(),
                state.loaded.clone(),
                state.failed.clone(),
                self.inner
                    .sampler_ticket
                    .load(std::sync::atomic::Ordering::Acquire),
                self.project_preparation(&state),
            )
        };
        let lease = manager
            .claim(ticket, &binding.stamp, &model)
            .map_err(native_error)?;
        let apply_request = lease.request();
        let names = review
            .artifacts
            .iter()
            .enumerate()
            .map(|(index, _)| {
                native::RelativeName::new(format!(
                    "Analysis-{}-{}-{index}.wav",
                    std::process::id(),
                    review.job.0
                ))
            })
            .collect::<native::Result<Vec<_>>>()
            .map_err(native_error)?;
        let prepared = lease.prepare(names).map_err(native_error)?;
        let mut pool = original_pool.clone();
        let mut sources = Vec::new();
        let mut commands = Vec::new();
        let mut output_files = Vec::with_capacity(prepared.artifacts().len());
        if request.replace_original {
            commands.push(Command::RemoveClips {
                clips: vec![binding.clip.id],
            });
        } else if request.mute_original {
            commands.push(Command::UpdateClips { updates: vec![windfall_project::ClipUpdate {
                id: binding.clip.id, patch: windfall_project::ClipPatch {
                    muted: Some(true), ..Default::default()
                }
            }] });
        }
        let rate = f64::from(model.sample_rate);
        let start = binding.clock.tick_after(binding.clip.start,
            binding.stamp.selection.start as f64 / rate)?;
        let length = if binding.stamp.selection.start == 0
            && binding.stamp.selection.end == binding.stamp.input_shape.frames {
            binding.clip.length
        } else {
            let end = binding.clock.tick_after_precise(start,
                binding.stamp.selection.frames().map_err(native_error)? as f64 / rate)?.ceil();
            (end as u32).checked_sub(start).filter(|length| *length > 0)
                .ok_or_else(|| error("invalid", "Invalid output duration."))?
        };
        if start
            .checked_add(length)
            .is_none_or(|end| end > MAX_SONG_TICKS)
        {
            return Err(error("invalid", "Output falls outside song."));
        }
        let ClipContent::Audio {
            mixer_track,
            output,
            ..
        } = binding.clip.content
        else {
            return Err(error("invalid", "Expected audio clip."));
        };
        let mut next_id = document.project().next_id;
        for artifact in prepared.artifacts() {
            if request.selected_roles.as_ref().is_some_and(|roles| !roles.contains(&artifact.role)) {
                continue;
            }
            work.check().map_err(native_error)?;
            let path = artifact
                .published_path
                .as_ref()
                .ok_or_else(|| error("invalid", "Output is not published."))?;
            // Acquire the no-write/no-delete handle before either decoding or
            // hashing: both must describe the same final file version.
            let mut output_file = source_file(path)?;
            let strict = windfall_codec::decode_file_strict_with(
                path,
                &windfall_codec::DecodeOptions {
                    max_decoded_bytes: OUTPUT_PCM_LIMIT,
                },
            )
            .map_err(|e| error("io", e))?;
            if native::AudioShape::of(&strict) != artifact.shape {
                return Err(error("invalid", "Published output shape changed."));
            }
            let fingerprint =
                native::ContentFingerprint::reader(&mut output_file, artifact.bytes, &mut work)
                    .map_err(native_error)?;
            if fingerprint.bytes != artifact.bytes || fingerprint.sha256 != artifact.sha256 {
                return Err(error("invalid", "Published output checksum changed."));
            }
            let decoded = self.inner.cache.decode(path)?;
            if native::ContentFingerprint::audio(&decoded, &mut work).map_err(native_error)?
                != native::ContentFingerprint::audio(&strict, &mut work).map_err(native_error)?
            {
                return Err(error("stale", "The cached output version changed."));
            }
            drop(strict);
            let (track, mixer_track, output) = if request.separate_tracks {
                let mixer = windfall_project::TrackId(next_id);
                let playlist = windfall_project::PlaylistTrackId(next_id.checked_add(1)
                    .ok_or_else(|| error("invalid", "Document identifiers exhausted."))?);
                next_id = next_id.checked_add(2).ok_or_else(|| error("invalid", "Document identifiers exhausted."))?;
                commands.push(Command::AddMixerTrack { name: Some(format!("{} — {}", binding.sample.name, artifact.role)) });
                commands.push(Command::AddPlaylistTrack { name: Some(artifact.role.clone()), index: None });
                (playlist, mixer, windfall_project::ClipAudioOutput::Mixer)
            } else { (binding.clip.track, mixer_track, output) };
            let sample = SampleId(next_id);
            next_id = next_id
                .checked_add(2)
                .ok_or_else(|| error("invalid", "Document identifiers exhausted."))?;
            commands.push(Command::AddSample {
                name: format!("{} ({})", binding.sample.name, artifact.role),
                path: SamplePath::External(crate::paths::display(path)),
            });
            commands.push(Command::AddClips {
                clips: vec![ClipInit {
                    track,
                    start,
                    length: Some(length),
                    offset: Some(0),
                    muted: Some(binding.clip.muted),
                    content: ClipContent::Audio {
                        sample,
                        mixer_track,
                        output,
                        normalize: false,
                        gain: 1.0,
                        pan: 0.0,
                        fade_in: 0,
                        fade_out: 0,
                        reverse: false,
                        pitch: 0.0,
                        stretch: Default::default(),
                    },
                }],
            });
            pool.insert(sample, decoded.clone());
            sources.push((sample, decoded));
            output_files.push(output_file);
        }
        let command = Command::Batch {
            label: Some(if request.separate_tracks { "Remix: import aligned stems" } else { "Apply analysis outputs" }.into()),
            commands,
        };
        document
            .dispatch(command.clone(), None)
            .map_err(|e| error("invalid", e))?;
        let mut plan = project_preparation
            .prepare(document.project(), &pool, ProjectPublicationIntent::Edit)
            .map_err(|e| match e {
                windfall_engine::ProjectPreparationError::Sampler(
                    windfall_engine::sampler_processing::SamplerPreparationError::Cancelled,
                ) => error("cancelled", e),
                _ => error("samplerPreparation", e),
            })?;
        work.check().map_err(native_error)?;
        #[cfg(test)]
        self.pause("analysis:prepared");
        work.check().map_err(native_error)?;
        // Keep the vector allocation until after all final guards, while moving
        // the already prepared shared handles into the live pool.
        let mut sources = sources.into_iter();
        let retirement;
        let final_result = {
            let _recording = self.recording_idle()?;
            let mut state = self.state();
            if !binding.current(&state)
                || !state.pool.same_sources(&original_pool)
                || state.loading != loading
                || state.loaded != loaded
                || state.failed != failed
                || self
                    .inner
                    .sampler_ticket
                    .load(std::sync::atomic::Ordering::Acquire)
                    != sampler_request
                || prepared.request() != apply_request
            {
                return Err(error(
                    "stale",
                    "The project, full source pool or preparation request changed.",
                ));
            }
            if preparation.cancel.is_cancelled() {
                return Err(error(
                    "cancelled",
                    "Analysis apply preparation was cancelled.",
                ));
            }
            #[cfg(test)]
            let mut checked = Ok(());
            #[cfg(test)]
            let eligibility_calls = crate::test_alloc::allocator_calls(|| {
                checked = prepared.check_eligibility(&binding.stamp, &model);
            });
            #[cfg(not(test))]
            let checked = prepared.check_eligibility(&binding.stamp, &model);
            checked.map_err(native_error)?;
            // Admission borrows the ready engine owner before document mutation;
            // stale stream/provider/plan tickets cannot become successful edits.
            let prepared_pool = plan.pool().clone();
            let publication = plan
                .publication(self, &state)
                .map_err(|e| error("stale", e))?;
            let applied = state
                .document
                .dispatch(command, None)
                .map_err(|e| error("invalid", e))?;
            self.commit_prepared_parameters(&state, &applied.touched);
            retirement = publication.install();
            for (sample, decoded) in sources.by_ref() {
                state.pool.insert(sample, decoded);
                state.loaded.insert(sample);
                state.failed.remove(&sample);
            }
            let result = DispatchResult {
                created: applied.created,
                patch: self.publish_prepared(&mut state, &applied.touched, &prepared_pool),
            };
            #[cfg(test)]
            let mut assets = None;
            #[cfg(test)]
            let acknowledge_calls = crate::test_alloc::allocator_calls(|| {
                assets = Some(prepared.acknowledge_commit());
            });
            #[cfg(test)]
            let assets = assets.expect("acknowledgement moved its prebuilt assets");
            #[cfg(not(test))]
            let assets = prepared.acknowledge_commit();
            #[cfg(test)]
            {
                assert_eq!((eligibility_calls, acknowledge_calls), (0, 0));
                // The small test evidence is written after the State guard below.
                drop(state);
                drop(_recording);
                *lock(&service.final_calls) = Some((eligibility_calls, acknowledge_calls));
            }
            (result, assets)
        };
        // No large candidate/input/progress/file retirement under final guards.
        let mut retirement = retirement;
        self.retire_project(&mut retirement);
        drop(file);
        drop(output_files);
        drop(document);
        drop(pool);
        drop(original_pool);
        final_result.1.retire();
        Ok(final_result.0)
    }
}
