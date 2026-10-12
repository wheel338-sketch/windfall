//! Real native worker/file lifecycle tests. The only adapter is explicitly
//! test-only authored CPU copying, and makes no inference/ML quality claim.
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::Path;
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tempfile::TempDir;
use windfall_analysis::*;
use windfall_core::AudioBuffer;

// Measures this native executable's Rust allocations (not OS RSS, foreign
// runtime allocations or a general inference memory guarantee).
struct TrackingAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static FINAL_GUARD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FINAL_COUNTS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}
fn final_event(allocate: bool) {
    let _ = FINAL_GUARD.try_with(|guard| {
        if guard.get() {
            FINAL_COUNTS.with(|counts| {
                let (allocs, frees) = counts.get();
                counts.set((
                    allocs + usize::from(allocate),
                    frees + usize::from(!allocate),
                ));
            });
        }
    });
}
#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;
fn allocated(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::SeqCst) + bytes;
    PEAK.fetch_max(live, Ordering::SeqCst);
}
unsafe impl std::alloc::GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        let pointer = unsafe { std::alloc::System.alloc(layout) };
        if !pointer.is_null() {
            final_event(true);
            allocated(layout.size());
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: std::alloc::Layout) {
        final_event(false);
        unsafe {
            std::alloc::System.dealloc(pointer, layout);
        }
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
    }
    unsafe fn realloc(
        &self,
        pointer: *mut u8,
        layout: std::alloc::Layout,
        bytes: usize,
    ) -> *mut u8 {
        let resized = unsafe { std::alloc::System.realloc(pointer, layout, bytes) };
        if !resized.is_null() {
            final_event(true);
            final_event(false);
            LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
            allocated(bytes);
        }
        resized
    }
}

const MODEL: &[u8] = include_bytes!("fixtures/lifecycle-cpu-v1.model");
fn work() -> Work {
    Work::new(CancelToken::default(), 10_000_000, Duration::from_secs(30)).unwrap()
}
fn manifest() -> ModelManifest {
    ModelManifest {
        onnx: None,
        id: "test-only-lifecycle-copy".into(),
        version: "1".into(),
        revision: 1,
        sha256: Sha256::digest(MODEL).into(),
        bytes: MODEL.len() as u64,
        max_bytes: 4096,
        provenance: ModelProvenance {
            origin: "authored:windfall/lifecycle-cpu-v1".into(),
            source_revision: "fixture-v1".into(),
            author: "Windfall contributors".into(),
            license_spdx: "CC0-1.0".into(),
            license_reference: "tests/fixtures/README.md".into(),
            adapter_id: "test-only-copy".into(),
            adapter_version: "1".into(),
            device: "cpu".into(),
        },
        sample_rate: 48_000,
        input_channels: 2,
        max_input_frames: 48_000,
        outputs: vec![OutputRole {
            role: "lifecycle-copy".into(),
            channels: 2,
        }],
    }
}

#[test]
fn onnx_contract_rejects_ambiguous_shapes_segment_policy_and_stem_mapping() {
    use windfall_analysis::{OnnxContract, TensorDimension, TensorSpec};
    let mut pack = manifest();
    pack.onnx = Some(OnnxContract {
        opset: 17,
        inputs: vec![TensorSpec {
            name: "audio".into(),
            shape: vec![
                TensorDimension::Fixed(1),
                TensorDimension::Fixed(2),
                TensorDimension::Symbol("frames".into()),
            ],
        }],
        outputs: vec![TensorSpec {
            name: "stems".into(),
            shape: vec![
                TensorDimension::Fixed(1),
                TensorDimension::Fixed(2),
                TensorDimension::Symbol("frames".into()),
            ],
        }],
        sample_rate: 48_000,
        segment_frames: 24_000,
        overlap_frames: 6_000,
        stem_roles: vec!["lifecycle-copy".into()],
    });
    pack.validate().unwrap();
    for change in 0..6 {
        let mut bad = pack.clone();
        let onnx = bad.onnx.as_mut().unwrap();
        match change {
            0 => onnx.opset = 0,
            1 => onnx.overlap_frames = onnx.segment_frames,
            2 => onnx.inputs[0].shape[2] = TensorDimension::Fixed(0),
            3 => onnx.stem_roles = vec!["wrong-stem".into()],
            4 => onnx.sample_rate = 44_100,
            _ => onnx.inputs.push(onnx.inputs[0].clone()),
        }
        assert!(
            bad.validate().is_err(),
            "invalid ONNX contract variant {change}"
        );
    }
}
fn source() -> AudioBuffer {
    AudioBuffer::from_interleaved(
        48_000,
        2,
        vec![0.25, -0.25, 0.5, -0.5, 0.75, -0.75, 1.0, -1.0],
    )
}
fn stamp(audio: &AudioBuffer) -> CaptureStamp {
    CaptureStamp {
        generation: 7,
        edit_revision: 11,
        source_key: 5,
        binding: [42; 32],
        source_identity: audio.identity(),
        source_fingerprint: ContentFingerprint::audio(audio, &mut work()).unwrap(),
        input_shape: AudioShape::of(audio),
        selection: FrameRange { start: 1, end: 3 },
    }
}
fn request(audio: &AudioBuffer) -> JobRequest {
    JobRequest {
        amount: 1.0,
        task: Default::default(),
        input: CapturedInput::capture(audio, stamp(audio), InputLimits::default(), &mut work())
            .unwrap(),
        model: manifest(),
        budget: JobBudget {
            scratch_bytes: 4096,
            output_bytes: 16 * 1024,
            work_units: 1_000_000,
            timeout: Duration::from_secs(30),
        },
    }
}

#[derive(Default)]
struct CopyAdapter {
    gate: Option<Arc<Gate>>,
    ready_gate: Option<Arc<Gate>>,
    mid_output_gate: Option<Arc<Gate>>,
    mode: Mode,
    active: AtomicUsize,
    maximum_active: AtomicUsize,
    starts: AtomicUsize,
}
#[derive(Default, Clone, Copy)]
enum Mode {
    #[default]
    Copy,
    Nonfinite,
    TooLong,
    TooShort,
    DecodeError,
    Panic,
    Missing,
    WrongRole,
    HugeChunk,
    TooMany,
    EncodedTooLarge,
    BadPath,
}
struct Gate {
    entered: Barrier,
    release: Barrier,
}
impl Gate {
    fn new(parties: usize) -> Arc<Self> {
        Arc::new(Self {
            entered: Barrier::new(parties),
            release: Barrier::new(parties),
        })
    }
    fn hold(self: &Arc<Self>) -> Release {
        self.entered.wait();
        Release(Some(self.clone()))
    }
}
// Unwind releases the worker before the manager's joining destructor.
struct Release(Option<Arc<Gate>>);
impl Release {
    fn release(mut self) {
        self.0.take().unwrap().release.wait();
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(gate) = self.0.take() {
            gate.release.wait();
        }
    }
}
struct Active<'a>(&'a AtomicUsize);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl AnalysisAdapter for CopyAdapter {
    fn id(&self) -> &str {
        "test-only-copy"
    }
    fn version(&self) -> &str {
        "1"
    }
    fn run(
        &self,
        input: &CapturedInput,
        model: &VerifiedModel,
        context: &mut WorkerContext,
        writer: &mut ArtifactWriter,
    ) -> Result<()> {
        assert_eq!(model.bytes(), MODEL);
        assert_eq!(context.scratch().len(), 1024);
        self.starts.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum_active.fetch_max(active, Ordering::SeqCst);
        let _active = Active(&self.active);
        if let Some(gate) = &self.gate {
            gate.entered.wait();
            gate.release.wait();
        }
        context.checkpoint(1)?;
        match self.mode {
            Mode::DecodeError => {
                return writer.import_audio(
                    RelativeName::new("copy.wav")?,
                    "lifecycle-copy",
                    Cursor::new(b"not an audio file"),
                    context.work(),
                );
            }
            Mode::Panic => panic!("authored adapter panic seam"),
            Mode::Missing => return Ok(()),
            Mode::EncodedTooLarge => {
                return writer.import_audio(
                    RelativeName::new("copy.wav")?,
                    "lifecycle-copy",
                    io::repeat(0).take(32 * 1024),
                    context.work(),
                );
            }
            Mode::BadPath => {
                RelativeName::new("../competitor.wav")?;
            }
            _ => {}
        }
        let role = if matches!(self.mode, Mode::WrongRole) {
            "not-declared"
        } else {
            "lifecycle-copy"
        };
        writer.start_audio(RelativeName::new("copy.wav")?, role, context.work())?;
        match self.mode {
            Mode::Nonfinite => writer.write_audio(&[f32::NAN, 0.0], context.work())?,
            Mode::TooLong => writer.write_audio(&[0.0; 6], context.work())?,
            Mode::TooShort => writer.write_audio(&[0.0; 2], context.work())?,
            Mode::HugeChunk => writer.write_audio(&[0.0; 4098], context.work())?,
            _ => {
                for chunk in input.selected_samples().chunks(4096) {
                    writer.write_audio(chunk, context.work())?;
                }
            }
        }
        if let Some(gate) = &self.mid_output_gate {
            gate.entered.wait();
            gate.release.wait();
        }
        writer.finish_audio(context.work())?;
        if matches!(self.mode, Mode::TooMany) {
            writer.start_audio(
                RelativeName::new("second.wav")?,
                "lifecycle-copy",
                context.work(),
            )?;
        }
        if let Some(gate) = &self.ready_gate {
            gate.entered.wait();
            gate.release.wait();
        }
        Ok(())
    }
}

struct Fixture {
    root: TempDir,
    cache: ModelCache,
    config: ManagerConfig,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let cache = ModelCache::new(
            root.path().join("models"),
            ModelCacheLimits {
                max_model_bytes: 4096,
                max_cache_bytes: 64 * 1024,
                max_entries: 16,
                max_imports: 2,
            },
        )
        .unwrap();
        cache
            .import_reader(&manifest(), Cursor::new(MODEL), &mut work())
            .unwrap();
        let mut config =
            ManagerConfig::bounded(root.path().join("staging"), root.path().join("published"));
        config.workers = 1;
        config.max_timeout = Duration::from_secs(30);
        Self {
            root,
            cache,
            config,
        }
    }
    fn manager(&self, adapter: Arc<CopyAdapter>) -> JobManager {
        JobManager::new(self.config.clone(), self.cache.clone(), adapter).unwrap()
    }
    fn staging_empty(&self) {
        assert_eq!(fs::read_dir(&self.config.staging_root).unwrap().count(), 0);
    }
}
fn name(text: &str) -> Vec<RelativeName> {
    vec![RelativeName::new(text).unwrap()]
}
fn ready(manager: &JobManager, audio: &AudioBuffer) -> Review {
    let id = manager.submit(request(audio)).unwrap();
    let snapshot = manager.wait(id).unwrap();
    assert_eq!(snapshot.status, JobStatus::Ready, "{:?}", snapshot.failure);
    manager.review(snapshot.ticket).unwrap()
}

#[test]
fn retained_auditions_are_reserved_refused_and_released_with_job_retirement() {
    let mut fixture = Fixture::new();
    fixture.config.memory_bytes = 8 * 1024 * 1024;
    let manager = fixture.manager(Arc::new(CopyAdapter::default()));
    let audio = source();
    assert!(matches!(
        manager.submit_with_retained_memory(request(&audio), 8 * 1024 * 1024),
        Err(AnalysisError::Budget("retained job memory"))
    ));
    assert_eq!(manager.usage().memory_bytes, 0);
    let id = manager
        .submit_with_retained_memory(request(&audio), 4 * 1024 * 1024)
        .unwrap();
    let settled = manager.wait(id).unwrap();
    assert_eq!(settled.status, JobStatus::Ready);
    assert!(manager.usage().memory_bytes > 6 * 1024 * 1024);
    assert!(manager.usage().memory_bytes <= 8 * 1024 * 1024);
    assert!(matches!(
        manager.check_retained_memory(2 * 1024 * 1024),
        Err(AnalysisError::Budget(
            "retained previews and analysis memory"
        ))
    ));
    manager.cancel(id).unwrap();
    manager.wait(id).unwrap();
    manager.forget(id).unwrap();
    assert_eq!(manager.usage().memory_bytes, 0);
}

#[test]
fn actual_worker_review_commit_retirement_and_history_source_lifetime() {
    assert_eq!(
        format!("{:x}", Sha256::digest(MODEL)),
        "07f6812eb7ac451adc4b010bb18b2fe098ce235498880c80ead3fad519e47d33"
    );
    let fixture = Fixture::new();
    let manager = fixture.manager(Arc::new(CopyAdapter::default()));
    let audio = source();
    let review = ready(&manager, &audio);
    let before = manager.snapshot(review.job).unwrap();
    assert_eq!(
        before.transitions,
        [JobStatus::Queued, JobStatus::Running, JobStatus::Ready]
    );
    assert!(before.sequence > 3 && before.completed_work > MODEL.len() as u64);
    assert_eq!(review.capture.source_identity, audio.identity());
    assert_eq!(review.artifacts[0].frame_origin, 1);
    assert_eq!(
        review.artifacts[0].shape,
        AudioShape {
            frames: 2,
            channels: 2,
            sample_rate: 48_000
        }
    );
    let lease = manager
        .claim(review.ticket, &stamp(&audio), &manifest())
        .unwrap();
    assert!(matches!(
        manager.cancel(review.job),
        Err(AnalysisError::Claimed)
    ));
    assert!(matches!(
        manager.claim(review.ticket, &stamp(&audio), &manifest()),
        Err(AnalysisError::Claimed)
    ));
    let prepared = lease.prepare(name("history.wav")).unwrap();
    prepared
        .check_eligibility(&stamp(&audio), &manifest())
        .unwrap();
    let path = prepared.review().artifacts[0]
        .published_path
        .clone()
        .unwrap();
    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "4dbb9f64ee9794914f7a9bf01ed95c3775858a4bb9413877a99218b88077528b"
    );
    assert_eq!(bytes.len(), 74);
    assert_eq!(
        windfall_codec::decode_file(&path).unwrap().samples(),
        &[0.5, -0.5, 0.75, -0.75]
    );
    // This test simulates caller acceptance; it does NOT prove a Session batch.
    let applied = prepared.acknowledge_commit();
    assert_eq!(
        manager.snapshot(review.job).unwrap().status,
        JobStatus::Consumed
    );
    assert!(manager.usage().memory_bytes > 64 * 1024);
    assert!(matches!(
        manager.forget(review.job),
        Err(AnalysisError::Claimed)
    ));
    let assets = applied.retire();
    fixture.staging_empty();
    assert_eq!(manager.usage().memory_bytes, 128 * 1024);
    assert_eq!(manager.cancel(review.job).unwrap(), JobStatus::Consumed);
    assert!(matches!(
        manager.claim(review.ticket, &stamp(&audio), &manifest()),
        Err(AnalysisError::NotReady)
    ));
    manager.forget(review.job).unwrap();
    assert_eq!(manager.usage().memory_bytes, 0);
    manager.shutdown();
    drop(manager);
    // Undo removes a reference; redo/save/archive still need identical bytes.
    assert_eq!(
        fs::read(assets[0].published_path.as_ref().unwrap()).unwrap(),
        bytes
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn r1_final_borrowed_check_and_commit_have_zero_allocations_and_frees() {
    r1_final_guard(1);
}
#[test]
fn r1_final_seam_sixteen_outputs_moves_prebuilt_metadata_without_allocating_or_freeing() {
    r1_final_guard(16);
}
fn r1_final_guard(count: usize) {
    let fixture = Fixture::new();
    let adapter: Arc<dyn AnalysisAdapter> = if count == 1 {
        Arc::new(CopyAdapter::default())
    } else {
        Arc::new(R1MultiAdapter { alias: false })
    };
    let manager = JobManager::new(fixture.config.clone(), fixture.cache.clone(), adapter).unwrap();
    let audio = source();
    let current = stamp(&audio);
    let mut model = manifest();
    for index in 1..count {
        model.outputs.push(OutputRole {
            role: format!("role-{index}"),
            channels: 2,
        });
    }
    let mut job = request(&audio);
    job.model = model.clone();
    let id = manager.submit(job).unwrap();
    let snapshot = manager.wait(id).unwrap();
    assert_eq!(snapshot.status, JobStatus::Ready);
    let prepared = manager
        .claim(snapshot.ticket, &current, &model)
        .unwrap()
        .prepare(
            (0..count)
                .map(|index| RelativeName::new(format!("guarded-{index}.wav")).unwrap())
                .collect(),
        )
        .unwrap();
    let mut stale = current.clone();
    stale.edit_revision += 1;
    FINAL_COUNTS.with(|c| c.set((0, 0)));
    FINAL_GUARD.with(|c| c.set(true));
    let eligible = prepared.check_eligibility(&current, &model);
    let refused = prepared.check_eligibility(&stale, &model);
    let applied = prepared.acknowledge_commit();
    FINAL_GUARD.with(|c| c.set(false));
    eligible.unwrap();
    assert!(matches!(refused, Err(AnalysisError::Stale)));
    let counts = FINAL_COUNTS.with(|c| c.get());
    assert_eq!(counts, (0, 0), "final seam allocation/free counts");
    eprintln!(
        "R1_FINAL_SEAM allocations={} frees={} prepared_metadata={} status=Consumed retirement=deferred",
        counts.0,
        counts.1,
        applied.artifacts().len()
    );
    assert_eq!(applied.artifacts().len(), count);
    applied.retire();
}

// LIVE/PEAK describe the whole executable. Isolate only their asserting cases
// from unrelated libtest allocations; each case still uses real worker threads.
fn isolated_heap_case(name: &str) -> bool {
    const CHILD: &str = "WINDFALL_ANALYSIS_HEAP_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(name) {
        return false;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--test-threads=1", "--nocapture"])
        .env(CHILD, name)
        .output()
        .expect("start isolated native heap case");
    eprint!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "isolated heap case {name}: {}",
        output.status
    );
    true
}

#[test]
fn r1_high_capacity_valid_manifest_is_charged_queued_running_and_ready() {
    if isolated_heap_case("r1_high_capacity_valid_manifest_is_charged_queued_running_and_ready") {
        return;
    }
    let mut fixture = Fixture::new();
    fixture.config.memory_bytes = 24 * 1024 * 1024;
    let gate = Gate::new(2);
    let manager = fixture.manager(Arc::new(CopyAdapter {
        gate: Some(gate.clone()),
        ..Default::default()
    }));
    let audio = source();
    let baseline = LIVE.load(Ordering::SeqCst);
    let mut job = request(&audio);
    let mut origin = String::with_capacity(8 * 1024 * 1024);
    origin.push_str(&job.model.provenance.origin);
    job.model.provenance.origin = origin;
    let mut roles = Vec::with_capacity(128 * 1024);
    roles.append(&mut job.model.outputs);
    job.model.outputs = roles;
    job.model.validate().unwrap();
    let retained = job.model.provenance.origin.capacity()
        + job.model.outputs.capacity() * size_of::<OutputRole>();
    let actual_manifest = job.model.retained_bytes().unwrap();
    let id = manager.submit(job).unwrap();
    let release = gate.hold();
    assert!(
        manager.usage().memory_bytes >= retained as u64,
        "retained manifest allocation must be charged"
    );
    let running_charge = manager.usage().memory_bytes;
    let running_heap = LIVE.load(Ordering::SeqCst).saturating_sub(baseline);
    assert!(running_heap >= retained);
    let mut queued_small = request(&audio);
    queued_small.model.provenance.device = String::with_capacity(4 * 1024 * 1024);
    queued_small.model.provenance.device.push_str("cpu");
    let queued_capacity = queued_small.model.retained_bytes().unwrap();
    let queued_id = manager.submit(queued_small).unwrap();
    assert_eq!(manager.usage().queued, 1);
    assert!(manager.usage().memory_bytes >= running_charge + queued_capacity);
    let queued_charge = manager.usage().memory_bytes;
    assert!(LIVE.load(Ordering::SeqCst).saturating_sub(baseline) >= running_heap + 4 * 1024 * 1024);
    manager.cancel(queued_id).unwrap();
    manager.forget(queued_id).unwrap();
    assert_eq!(manager.usage().memory_bytes, running_charge);
    let mut queued = request(&audio);
    queued.model.provenance.author = String::with_capacity(128 * 1024 * 1024);
    queued
        .model
        .provenance
        .author
        .push_str("Windfall contributors");
    assert!(matches!(
        manager.submit(queued),
        Err(AnalysisError::Budget(_))
    ));
    release.release();
    assert_eq!(manager.wait(id).unwrap().status, JobStatus::Ready);
    assert!(manager.usage().memory_bytes >= retained as u64);
    let ready_heap = LIVE.load(Ordering::SeqCst).saturating_sub(baseline);
    assert!(ready_heap >= retained);
    eprintln!(
        "R1_MANIFEST string_capacity={} vector_capacity_bytes={} retained_manifest={} queued_extra_manifest={} running_charge={} queued_total_charge={} ready_charge={}",
        8 * 1024 * 1024,
        128 * 1024 * size_of::<OutputRole>(),
        actual_manifest,
        queued_capacity,
        running_charge,
        queued_charge,
        manager.usage().memory_bytes
    );
    eprintln!(
        "R1_MANIFEST actual_Rust_heap_running_above_baseline={running_heap} actual_Rust_heap_ready_above_baseline={ready_heap}"
    );
    manager.cancel(id).unwrap();
    assert_eq!(manager.usage().memory_bytes, 128 * 1024);
}

#[test]
fn manager_replacement_never_reuses_process_job_ticket_or_apply_request_ids() {
    let fixture = Fixture::new();
    let audio = source();
    let mut previous = (0, 0, 0);
    for _ in 0..2 {
        let manager = fixture.manager(Arc::new(CopyAdapter::default()));
        let review = ready(&manager, &audio);
        let snapshot = manager.snapshot(review.job).unwrap();
        assert!(
            review.job.0 > previous.0
                && review.ticket.0 > previous.1
                && snapshot.request > previous.2
        );
        let lease = manager
            .claim(review.ticket, &stamp(&audio), &manifest())
            .unwrap();
        assert!(lease.request() > snapshot.request);
        previous = (review.job.0, review.ticket.0, lease.request());
        drop(lease);
        manager.cancel(review.job).unwrap();
        manager.forget(review.job).unwrap();
        manager.shutdown();
    }
}

struct R1MultiAdapter {
    alias: bool,
}
impl AnalysisAdapter for R1MultiAdapter {
    fn id(&self) -> &str {
        "test-only-copy"
    }
    fn version(&self) -> &str {
        "1"
    }
    fn run(
        &self,
        input: &CapturedInput,
        model: &VerifiedModel,
        context: &mut WorkerContext,
        artifacts: &mut ArtifactWriter,
    ) -> Result<()> {
        artifacts.start_audio(
            RelativeName::new("stem.wav")?,
            &model.manifest().outputs[0].role,
            context.work(),
        )?;
        artifacts.write_audio(input.selected_samples(), context.work())?;
        artifacts.finish_audio(context.work())?;
        for (index, role) in model.manifest().outputs.iter().enumerate().skip(1) {
            artifacts.start_audio(
                RelativeName::new(if self.alias {
                    "STEM.wav".to_owned()
                } else {
                    format!("other-{index}.wav")
                })?,
                &role.role,
                context.work(),
            )?;
            artifacts.write_audio(&[0.125; 4], context.work())?;
            artifacts.finish_audio(context.work())?;
        }
        Ok(())
    }
}
fn r1_two_roles() -> ModelManifest {
    let mut model = manifest();
    model.outputs.push(OutputRole {
        role: "second".into(),
        channels: 2,
    });
    model
}
#[test]
fn r1_artifact_and_destination_portable_case_aliases_refused_before_writes() {
    r1_alias_case(true);
}
#[test]
fn r1_destination_case_aliases_refuse_without_partial_publication() {
    r1_alias_case(false);
}
fn r1_alias_case(alias: bool) {
    let fixture = Fixture::new();
    let manager = JobManager::new(
        fixture.config.clone(),
        fixture.cache.clone(),
        Arc::new(R1MultiAdapter { alias }),
    )
    .unwrap();
    let audio = source();
    let mut job = request(&audio);
    job.model = r1_two_roles();
    let id = manager.submit(job).unwrap();
    let status = manager.wait(id).unwrap();
    if alias {
        assert_eq!(
            status.status,
            JobStatus::Failed,
            "staged aliases must refuse before replacing first PCM"
        );
    } else {
        assert_eq!(status.status, JobStatus::Ready);
        let before = manager.review(status.ticket).unwrap();
        let result = manager
            .claim(status.ticket, &stamp(&audio), &r1_two_roles())
            .unwrap()
            .prepare(vec![
                RelativeName::new("final.wav").unwrap(),
                RelativeName::new("FINAL.wav").unwrap(),
            ]);
        assert!(matches!(result, Err(AnalysisError::Invalid(_))));
        assert_eq!(
            fs::read_dir(&fixture.config.published_root)
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            manager.review(status.ticket).unwrap().artifacts,
            before.artifacts
        );
        manager.cancel(id).unwrap();
    }
    fixture.staging_empty();
}

#[test]
fn r1_encoded_nonfinite_float32_helper_fails_in_real_worker() {
    struct Encoded {
        bytes: Vec<u8>,
    }
    impl AnalysisAdapter for Encoded {
        fn id(&self) -> &str {
            "test-only-copy"
        }
        fn version(&self) -> &str {
            "1"
        }
        fn run(
            &self,
            _: &CapturedInput,
            _: &VerifiedModel,
            context: &mut WorkerContext,
            writer: &mut ArtifactWriter,
        ) -> Result<()> {
            writer.import_audio(
                RelativeName::new("bad.wav")?,
                "lifecycle-copy",
                Cursor::new(&self.bytes),
                context.work(),
            )
        }
    }
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let fixture = Fixture::new();
        let path = fixture.root.path().join("bad.wav");
        windfall_codec::write_wav(
            &path,
            &AudioBuffer::from_interleaved(48_000, 2, vec![0.25; 4]),
            windfall_codec::WavSampleFormat::Float32,
        )
        .unwrap();
        let mut bytes = fs::read(path).unwrap();
        let position = bytes.windows(4).position(|w| w == b"data").unwrap() + 8;
        bytes[position..position + 4].copy_from_slice(&bad.to_le_bytes());
        assert_eq!(
            windfall_codec::decode_bytes(&bytes, Some("wav"))
                .unwrap()
                .samples()[0],
            0.0
        );
        let manager = JobManager::new(
            fixture.config.clone(),
            fixture.cache.clone(),
            Arc::new(Encoded { bytes }),
        )
        .unwrap();
        let id = manager.submit(request(&source())).unwrap();
        assert_eq!(manager.wait(id).unwrap().status, JobStatus::Failed);
        fixture.staging_empty();
    }
}

#[test]
fn barrier_queue_concurrency_cancel_and_count_retention() {
    let mut fixture = Fixture::new();
    fixture.config.max_queued = 1;
    fixture.config.max_retained_jobs = 2;
    let gate = Gate::new(2);
    let adapter = Arc::new(CopyAdapter {
        gate: Some(gate.clone()),
        ..CopyAdapter::default()
    });
    let manager = fixture.manager(adapter.clone());
    let audio = source();
    let running = manager.submit(request(&audio)).unwrap();
    let release = gate.hold();
    assert_eq!(
        manager.snapshot(running).unwrap().status,
        JobStatus::Running
    );
    let queued = manager.submit(request(&audio)).unwrap();
    assert_eq!(manager.snapshot(queued).unwrap().status, JobStatus::Queued);
    assert!(matches!(
        manager.submit(request(&audio)),
        Err(AnalysisError::Budget(_))
    ));
    assert!(matches!(
        manager.forget(running),
        Err(AnalysisError::NotReady)
    ));
    assert_eq!(manager.cancel(queued).unwrap(), JobStatus::Cancelled);
    assert_eq!(
        manager.wait(queued).unwrap().transitions,
        [
            JobStatus::Queued,
            JobStatus::Cancelling,
            JobStatus::Cancelled
        ]
    );
    assert_eq!(manager.cancel(running).unwrap(), JobStatus::Cancelling);
    assert_eq!(
        manager.snapshot(running).unwrap().status,
        JobStatus::Cancelling
    );
    release.release();
    assert_eq!(manager.wait(running).unwrap().status, JobStatus::Cancelled);
    assert_eq!(adapter.starts.load(Ordering::SeqCst), 1);
    assert_eq!(adapter.maximum_active.load(Ordering::SeqCst), 1);
    fixture.staging_empty();
    assert_eq!(manager.usage().active, 0);
    assert_eq!(manager.usage().queued, 0);
    assert_eq!(manager.usage().reserved_disk_bytes, 0);
    assert!(matches!(
        manager.submit(request(&audio)),
        Err(AnalysisError::Budget(_))
    ));
    manager.forget(queued).unwrap();
    manager.forget(running).unwrap();
    let later = manager.submit(request(&audio)).unwrap();
    assert!(later.0 > queued.0);
    gate.hold().release();
    let result = manager.wait(later).unwrap();
    assert!(result.request > manager.usage().retained_jobs as u64);
    manager.cancel(later).unwrap();
}

#[test]
fn two_real_workers_stop_at_barrier_and_outputs_do_not_share_staging() {
    let mut fixture = Fixture::new();
    fixture.config.workers = 2;
    let gate = Gate::new(3);
    let adapter = Arc::new(CopyAdapter {
        gate: Some(gate.clone()),
        ..CopyAdapter::default()
    });
    let manager = fixture.manager(adapter.clone());
    let audio = source();
    let first = manager.submit(request(&audio)).unwrap();
    let second = manager.submit(request(&audio)).unwrap();
    let release = gate.hold();
    assert_eq!(manager.usage().active, 2);
    assert_eq!(adapter.maximum_active.load(Ordering::SeqCst), 2);
    release.release();
    let one = manager.wait(first).unwrap();
    let two = manager.wait(second).unwrap();
    assert_eq!(one.status, JobStatus::Ready);
    assert_eq!(two.status, JobStatus::Ready);
    assert_eq!(
        fs::read_dir(&fixture.config.staging_root).unwrap().count(),
        2
    );
    manager.cancel(first).unwrap();
    assert_eq!(
        fs::read_dir(&fixture.config.staging_root).unwrap().count(),
        1
    );
    assert_eq!(
        manager.review(two.ticket).unwrap().artifacts[0]
            .shape
            .frames,
        2
    );
    manager.cancel(second).unwrap();
    fixture.staging_empty();
}

#[test]
fn compact_inputs_shape_finite_range_and_memory_admission() {
    let audio = source();
    let mut vector = Vec::with_capacity(1_000_000);
    vector.extend_from_slice(audio.samples());
    let oversized_capacity = AudioBuffer::from_interleaved(48_000, 2, vector);
    let input = CapturedInput::capture(
        &oversized_capacity,
        stamp(&oversized_capacity),
        InputLimits::default(),
        &mut work(),
    )
    .unwrap();
    assert_eq!(input.retained_bytes(), 32);
    assert_ne!(input.audio().identity(), oversized_capacity.identity());
    for range in [
        FrameRange { start: 3, end: 1 },
        FrameRange { start: 1, end: 1 },
        FrameRange { start: 0, end: 5 },
    ] {
        let mut capture = stamp(&audio);
        capture.selection = range;
        assert!(
            CapturedInput::capture(&audio, capture, InputLimits::default(), &mut work()).is_err()
        );
    }
    let limits = InputLimits {
        max_bytes: 31,
        ..InputLimits::default()
    };
    assert!(matches!(
        CapturedInput::capture(&audio, stamp(&audio), limits, &mut work()),
        Err(AnalysisError::Budget(_))
    ));
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let invalid = AudioBuffer::from_interleaved(48_000, 2, vec![value, 0.0, 0.0, 0.0]);
        let mut capture = stamp(&audio);
        capture.input_shape = AudioShape::of(&invalid);
        capture.selection = FrameRange { start: 0, end: 2 };
        assert!(
            CapturedInput::capture(&invalid, capture, InputLimits::default(), &mut work()).is_err()
        );
    }
    let mut fixture = Fixture::new();
    fixture.config.memory_bytes = 2 * 1024 * 1024 + 128 * 1024;
    let manager = fixture.manager(Arc::new(CopyAdapter::default()));
    assert!(matches!(
        manager.submit(request(&audio)),
        Err(AnalysisError::Budget("retained job memory"))
    ));
    assert_eq!(manager.usage().retained_jobs, 0);
}

#[test]
fn worker_output_failures_release_only_owned_staging() {
    for mode in [
        Mode::Nonfinite,
        Mode::TooLong,
        Mode::TooShort,
        Mode::DecodeError,
        Mode::Panic,
        Mode::Missing,
        Mode::WrongRole,
        Mode::HugeChunk,
        Mode::TooMany,
        Mode::EncodedTooLarge,
        Mode::BadPath,
    ] {
        let fixture = Fixture::new();
        let manager = fixture.manager(Arc::new(CopyAdapter {
            mode,
            ..CopyAdapter::default()
        }));
        let competitor = fixture.config.published_root.join("competitor.wav");
        fs::write(&competitor, b"competitor bytes").unwrap();
        let id = manager.submit(request(&source())).unwrap();
        let result = manager.wait(id).unwrap();
        assert_eq!(result.status, JobStatus::Failed);
        assert!(result.failure.is_some());
        fixture.staging_empty();
        assert_eq!(fs::read(&competitor).unwrap(), b"competitor bytes");
        assert_eq!(manager.usage().reserved_disk_bytes, 0);
        assert_eq!(manager.usage().memory_bytes, 128 * 1024);
    }
}

#[test]
fn stale_canonical_checks_leave_review_and_publication_unconsumed() {
    let fixture = Fixture::new();
    let manager = fixture.manager(Arc::new(CopyAdapter::default()));
    let audio = source();
    let review = ready(&manager, &audio);
    let mut changes = Vec::new();
    let original = stamp(&audio);
    let mut x = original.clone();
    x.generation += 1;
    changes.push(x);
    let mut x = original.clone();
    x.edit_revision += 1;
    changes.push(x);
    let mut x = original.clone();
    x.source_key += 1;
    changes.push(x);
    let mut x = original.clone();
    x.binding[0] ^= 1;
    changes.push(x);
    let mut x = original.clone();
    x.source_fingerprint.sha256[0] ^= 1;
    changes.push(x);
    let other = source();
    let mut x = original.clone();
    x.source_identity = other.identity();
    changes.push(x);
    let mut x = original.clone();
    x.selection = FrameRange { start: 0, end: 2 };
    changes.push(x);
    let mut x = original.clone();
    x.input_shape.sample_rate = 44_100;
    changes.push(x);
    for current in changes {
        assert!(matches!(
            manager.claim(review.ticket, &current, &manifest()),
            Err(AnalysisError::Stale)
        ));
    }
    let mut changed_model = manifest();
    changed_model.revision += 1;
    assert!(matches!(
        manager.claim(review.ticket, &original, &changed_model),
        Err(AnalysisError::Stale)
    ));
    assert_eq!(
        manager.snapshot(review.job).unwrap().status,
        JobStatus::Ready
    );
    let prepared = manager
        .claim(review.ticket, &original, &manifest())
        .unwrap()
        .prepare(name("stale.wav"))
        .unwrap();
    let bytes = fs::read(fixture.config.published_root.join("stale.wav")).unwrap();
    let mut changed = original;
    changed.edit_revision += 1;
    assert!(matches!(
        prepared.check_eligibility(&changed, &manifest()),
        Err(AnalysisError::Stale)
    ));
    drop(prepared); // actual caller commit refused
    assert_eq!(
        manager.snapshot(review.job).unwrap().status,
        JobStatus::Ready
    );
    manager.cancel(review.job).unwrap();
    fixture.staging_empty();
    assert_eq!(
        fs::read(fixture.config.published_root.join("stale.wav")).unwrap(),
        bytes
    );
    assert_eq!(manager.usage().published_disk_bytes, bytes.len() as u64);
}

#[test]
fn final_collision_competitor_bytes_and_failed_commit_retry_are_bounded() {
    let fixture = Fixture::new();
    let manager = fixture.manager(Arc::new(CopyAdapter::default()));
    let audio = source();
    let review = ready(&manager, &audio);
    let target = fixture.config.published_root.join("collision.wav");
    fs::write(&target, b"competitor").unwrap();
    assert!(matches!(
        manager
            .claim(review.ticket, &stamp(&audio), &manifest())
            .unwrap()
            .prepare(name("collision.wav")),
        Err(AnalysisError::Collision)
    ));
    assert_eq!(fs::read(&target).unwrap(), b"competitor");
    assert!(matches!(
        manager
            .claim(review.ticket, &stamp(&audio), &manifest())
            .unwrap()
            .prepare(name("different.wav")),
        Err(AnalysisError::Invalid(_))
    ));
    assert!(!fixture.config.published_root.join("different.wav").exists());
    manager.cancel(review.job).unwrap();
    fixture.staging_empty();
    let review = ready(&manager, &audio);
    let prepared = manager
        .claim(review.ticket, &stamp(&audio), &manifest())
        .unwrap()
        .prepare(name("retry.wav"))
        .unwrap();
    let before = manager.usage();
    let path = prepared.review().artifacts[0]
        .published_path
        .clone()
        .unwrap();
    let bytes = fs::read(&path).unwrap();
    drop(prepared);
    let retry = manager
        .claim(review.ticket, &stamp(&audio), &manifest())
        .unwrap()
        .prepare(name("retry.wav"))
        .unwrap();
    assert_eq!(
        manager.usage().published_disk_bytes,
        before.published_disk_bytes
    );
    assert_eq!(manager.usage().published_files, before.published_files);
    retry.acknowledge_commit().retire();
    assert_eq!(fs::read(path).unwrap(), bytes);
    fixture.staging_empty();
}

#[test]
fn explicit_model_import_checks_cache_content_size_cancel_io_and_absent() {
    let fixture = Fixture::new();
    let model = manifest();
    let path = fixture
        .cache
        .import_reader(&model, Cursor::new(MODEL), &mut work())
        .unwrap();
    let original = fs::read(&path).unwrap();
    fs::write(&path, vec![b'x'; MODEL.len()]).unwrap();
    assert!(matches!(
        fixture.cache.load(&model, &mut work()),
        Err(AnalysisError::Checksum)
    ));
    assert!(matches!(
        fixture
            .cache
            .import_reader(&model, Cursor::new(MODEL), &mut work()),
        Err(AnalysisError::Checksum)
    ));
    assert_eq!(fs::read(&path).unwrap(), vec![b'x'; MODEL.len()]);
    fs::write(&path, original).unwrap();
    let absent = ModelManifest {
        onnx: None,
        sha256: [1; 32],
        ..model.clone()
    };
    assert!(matches!(
        fixture.cache.load(&absent, &mut work()),
        Err(AnalysisError::ModelAbsent)
    ));
    for data in [&MODEL[..MODEL.len() - 1], &[0; 512][..]] {
        assert!(matches!(
            fixture
                .cache
                .import_reader(&absent, Cursor::new(data), &mut work()),
            Err(AnalysisError::ModelSize)
        ));
    }
    assert!(matches!(
        fixture
            .cache
            .import_reader(&absent, Cursor::new(MODEL), &mut work()),
        Err(AnalysisError::Checksum)
    ));
    let token = CancelToken::default();
    token.cancel();
    let mut cancelled = Work::new(token, 10_000, Duration::from_secs(30)).unwrap();
    assert!(matches!(
        fixture
            .cache
            .import_reader(&absent, Cursor::new(MODEL), &mut cancelled),
        Err(AnalysisError::Cancelled)
    ));
    struct BadReader;
    impl Read for BadReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("authored read failure"))
        }
    }
    assert!(matches!(
        fixture.cache.import_reader(&absent, BadReader, &mut work()),
        Err(AnalysisError::Io(_))
    ));
    assert_eq!(fs::read_dir(fixture.cache.root()).unwrap().count(), 1);
    assert_eq!(
        fixture.cache.load(&model, &mut work()).unwrap().bytes(),
        MODEL
    );
}

#[test]
fn file_fingerprint_tracks_actual_content_and_ignores_path_aliases() {
    let fixture = Fixture::new();
    let a = fixture.root.path().join("source.bin");
    let alias = fixture.root.path().join("alias.bin");
    fs::write(&a, b"same-size-A").unwrap();
    fs::hard_link(&a, &alias).unwrap();
    let fingerprint = |path: &Path| {
        ContentFingerprint::reader(fs::File::open(path).unwrap(), 1024, &mut work()).unwrap()
    };
    let before = fingerprint(&a);
    assert_eq!(fingerprint(&alias), before);
    fs::write(&a, b"same-size-B").unwrap();
    let after = fingerprint(&alias);
    assert_eq!(before.bytes, after.bytes);
    assert_ne!(before.sha256, after.sha256);
}

#[test]
fn disk_file_work_and_integer_bounds_fail_observably() {
    let audio = source();
    let mut fixture = Fixture::new();
    fixture.config.disk_bytes = 16 * 1024;
    let manager = fixture.manager(Arc::new(CopyAdapter::default()));
    assert!(matches!(
        manager.submit(request(&audio)),
        Err(AnalysisError::Budget("job/publication disk"))
    ));
    let fixture = Fixture::new();
    let manager = fixture.manager(Arc::new(CopyAdapter::default()));
    let mut oversized = request(&audio);
    oversized.budget.output_bytes = u64::MAX;
    assert!(matches!(
        manager.submit(oversized),
        Err(AnalysisError::Exhausted)
    ));
    let mut too_little_output = request(&audio);
    too_little_output.budget.output_bytes = 1;
    assert!(matches!(
        manager.submit(too_little_output),
        Err(AnalysisError::Budget("aligned output reservation"))
    ));
    let mut little_work = request(&audio);
    little_work.budget.work_units = 1;
    let id = manager.submit(little_work).unwrap();
    let failed = manager.wait(id).unwrap();
    assert_eq!(failed.status, JobStatus::Failed);
    assert!(failed.failure.unwrap().contains("work units"));
    assert!(matches!(
        AudioShape {
            frames: u64::MAX,
            channels: 2,
            sample_rate: 48_000
        }
        .pcm_bytes(),
        Err(AnalysisError::Exhausted)
    ));
    assert!(Work::new(CancelToken::default(), u64::MAX, Duration::from_secs(1)).is_err());
    for bad in [
        "../a.wav", "a/b.wav", "C:a.wav", "a\\b.wav", "CON.wav", "LPT1.wav", ".a.wav", "a..wav",
        "a.wav.", "a.wav ",
    ] {
        assert!(RelativeName::new(bad).is_err(), "{bad}");
    }
}

#[test]
fn shutdown_cancels_queue_and_running_worker_then_joins_without_sleeps() {
    let fixture = Fixture::new();
    let gate = Gate::new(2);
    let adapter = Arc::new(CopyAdapter {
        gate: Some(gate.clone()),
        ..CopyAdapter::default()
    });
    let manager = Arc::new(fixture.manager(adapter));
    let audio = source();
    let running = manager.submit(request(&audio)).unwrap();
    let release = gate.hold();
    let queued = manager.submit(request(&audio)).unwrap();
    let sequence = manager.snapshot(running).unwrap().sequence;
    let shutdown_manager = manager.clone();
    let shutdown = std::thread::spawn(move || shutdown_manager.shutdown());
    assert_eq!(
        manager.wait_for_change(running, sequence).unwrap().status,
        JobStatus::Cancelling
    );
    assert_eq!(manager.wait(queued).unwrap().status, JobStatus::Cancelled);
    release.release();
    shutdown.join().unwrap();
    assert_eq!(manager.wait(running).unwrap().status, JobStatus::Cancelled);
    assert_eq!(manager.wait(queued).unwrap().status, JobStatus::Cancelled);
    assert!(matches!(
        manager.submit(request(&audio)),
        Err(AnalysisError::Shutdown)
    ));
    assert_eq!(manager.usage().active, 0);
    fixture.staging_empty();
}

#[test]
fn actual_output_io_refusal_preserves_replacement_and_reserves_failed_cleanup() {
    let fixture = Fixture::new();
    let gate = Gate::new(2);
    let manager = fixture.manager(Arc::new(CopyAdapter {
        gate: Some(gate.clone()),
        ..CopyAdapter::default()
    }));
    let audio = source();
    let id = manager.submit(request(&audio)).unwrap();
    let release = gate.hold();
    let staging = fs::read_dir(&fixture.config.staging_root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        staging.parent(),
        Some(fixture.config.staging_root.as_path())
    );
    fs::remove_dir(&staging).unwrap();
    fs::write(&staging, b"replacement file must survive refusal").unwrap();
    release.release();
    let result = manager.wait(id).unwrap();
    assert_eq!(result.status, JobStatus::Failed);
    assert!(result.failure.unwrap().contains("cleanup failed"));
    assert_eq!(
        fs::read(&staging).unwrap(),
        b"replacement file must survive refusal"
    );
    assert!(manager.usage().reserved_disk_bytes > 0);
    assert!(manager.forget(id).is_err());
    assert!(manager.retry_cleanup(id).is_err());
    // External owner removes its replacement, then only our reservation retires.
    fs::remove_file(&staging).unwrap();
    manager.retry_cleanup(id).unwrap();
    assert_eq!(manager.usage().reserved_disk_bytes, 0);
    manager.forget(id).unwrap();
    fixture.staging_empty();
}

#[test]
fn cancellation_at_ready_publication_boundary_cannot_resurrect_ready() {
    let fixture = Fixture::new();
    let gate = Gate::new(2);
    let manager = fixture.manager(Arc::new(CopyAdapter {
        ready_gate: Some(gate.clone()),
        ..CopyAdapter::default()
    }));
    let audio = source();
    let id = manager.submit(request(&audio)).unwrap();
    let release = gate.hold();
    let before = manager.snapshot(id).unwrap();
    assert_eq!(before.status, JobStatus::Running);
    assert!(before.completed_work > 100);
    manager.cancel(id).unwrap();
    release.release();
    let after = manager.wait(id).unwrap();
    assert_eq!(after.status, JobStatus::Cancelled);
    assert!(after.sequence > before.sequence);
    assert!(after.completed_work >= before.completed_work);
    assert!(!after.transitions.contains(&JobStatus::Ready));
    fixture.staging_empty();
}

#[test]
fn simultaneous_final_publications_have_one_owner_and_loser_cannot_remove_winner() {
    let mut fixture = Fixture::new();
    fixture.config.workers = 2;
    let manager = Arc::new(fixture.manager(Arc::new(CopyAdapter::default())));
    let audio = source();
    let one = ready(&manager, &audio);
    let two = ready(&manager, &audio);
    let lease_one = manager
        .claim(one.ticket, &stamp(&audio), &manifest())
        .unwrap();
    let lease_two = manager
        .claim(two.ticket, &stamp(&audio), &manifest())
        .unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let barrier_one = barrier.clone();
    let first = std::thread::spawn(move || {
        barrier_one.wait();
        lease_one.prepare(name("simultaneous.wav"))
    });
    let barrier_two = barrier.clone();
    let second = std::thread::spawn(move || {
        barrier_two.wait();
        lease_two.prepare(name("simultaneous.wav"))
    });
    barrier.wait();
    let results = [first.join().unwrap(), second.join().unwrap()];
    let mut winner = None;
    let mut collisions = 0;
    for result in results {
        match result {
            Ok(prepared) => {
                assert!(winner.is_none());
                winner = Some(prepared);
            }
            Err(AnalysisError::Collision) => collisions += 1,
            Err(error) => panic!("unexpected publication result: {error}"),
        }
    }
    assert_eq!(collisions, 1);
    let winner = winner.unwrap();
    let winner_id = winner.review().job;
    let loser = if winner_id == one.job {
        two.job
    } else {
        one.job
    };
    let path = fixture.config.published_root.join("simultaneous.wav");
    let bytes = fs::read(&path).unwrap();
    manager.cancel(loser).unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
    winner.acknowledge_commit().retire();
    fixture.staging_empty();
    assert_eq!(manager.usage().published_files, 1);
    assert_eq!(manager.usage().published_disk_bytes, bytes.len() as u64);
    manager.cancel(winner_id).unwrap();
    manager.shutdown();
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn cache_import_race_verifies_competitor_and_cancellation_removes_only_staging() {
    let fixture = Fixture::new();
    let data = b"second authored fixture, not weights";
    let mut model = manifest();
    model.sha256 = Sha256::digest(data).into();
    model.bytes = data.len() as u64;
    let file_name = format!("{:x}.model", Sha256::digest(data));
    let final_path = fixture.cache.root().join(file_name);
    struct CompetitorReader<'a> {
        reader: Cursor<&'a [u8]>,
        path: &'a Path,
        once: bool,
    }
    impl Read for CompetitorReader<'_> {
        fn read(&mut self, chunk: &mut [u8]) -> io::Result<usize> {
            if !self.once {
                self.once = true;
                fs::write(self.path, b"competitor owned wrong bytes")?;
            }
            self.reader.read(chunk)
        }
    }
    let result = fixture.cache.import_reader(
        &model,
        CompetitorReader {
            reader: Cursor::new(data),
            path: &final_path,
            once: false,
        },
        &mut work(),
    );
    assert!(matches!(
        result,
        Err(AnalysisError::ModelSize | AnalysisError::Checksum)
    ));
    assert_eq!(
        fs::read(&final_path).unwrap(),
        b"competitor owned wrong bytes"
    );
    assert_eq!(fs::read_dir(fixture.cache.root()).unwrap().count(), 2);
    let token = CancelToken::default();
    let mut cancelled_work = Work::new(token.clone(), 10_000, Duration::from_secs(30)).unwrap();
    let other = b"third authored fixture";
    let mut other_model = model;
    other_model.sha256 = Sha256::digest(other).into();
    other_model.bytes = other.len() as u64;
    struct CancelReader<'a> {
        reader: Cursor<&'a [u8]>,
        token: CancelToken,
    }
    impl Read for CancelReader<'_> {
        fn read(&mut self, chunk: &mut [u8]) -> io::Result<usize> {
            let n = self.reader.read(chunk)?;
            self.token.cancel();
            Ok(n)
        }
    }
    assert!(matches!(
        fixture.cache.import_reader(
            &other_model,
            CancelReader {
                reader: Cursor::new(other),
                token
            },
            &mut cancelled_work
        ),
        Err(AnalysisError::Cancelled)
    ));
    assert_eq!(fs::read_dir(fixture.cache.root()).unwrap().count(), 2);
    assert_eq!(
        fs::read(&final_path).unwrap(),
        b"competitor owned wrong bytes"
    );
}

#[test]
fn persistent_file_quota_and_restart_inventory_survive_forget_and_shutdown() {
    let mut fixture = Fixture::new();
    fixture.config.max_published_files = 2;
    let manager = fixture.manager(Arc::new(CopyAdapter::default()));
    let audio = source();
    let review = ready(&manager, &audio);
    let assets = manager
        .claim(review.ticket, &stamp(&audio), &manifest())
        .unwrap()
        .prepare(name("retained.wav"))
        .unwrap()
        .acknowledge_commit()
        .retire();
    manager.forget(review.job).unwrap();
    assert!(matches!(
        manager.submit(request(&audio)),
        Err(AnalysisError::Budget("persistent output count"))
    ));
    manager.shutdown();
    drop(manager);
    let restarted = fixture.manager(Arc::new(CopyAdapter::default()));
    assert_eq!(restarted.usage().published_files, 1);
    assert!(matches!(
        restarted.submit(request(&audio)),
        Err(AnalysisError::Budget("persistent output count"))
    ));
    assert!(assets[0].published_path.as_ref().unwrap().is_file());
}

#[test]
fn one_second_cpu_lifecycle_measures_owned_peak_and_retirement() {
    if isolated_heap_case("one_second_cpu_lifecycle_measures_owned_peak_and_retirement") {
        return;
    }
    let fixture = Fixture::new();
    let manager = fixture.manager(Arc::new(CopyAdapter::default()));
    let samples = (0..96_000)
        .map(|i| ((i % 32) as f32 - 16.0) / 16.0)
        .collect();
    let audio = AudioBuffer::from_interleaved(48_000, 2, samples);
    let mut captured = stamp(&audio);
    captured.selection = FrameRange {
        start: 0,
        end: 48_000,
    };
    let baseline = LIVE.load(Ordering::SeqCst);
    PEAK.store(baseline, Ordering::SeqCst);
    let started = std::time::Instant::now();
    let input = CapturedInput::capture(
        &audio,
        captured.clone(),
        InputLimits::default(),
        &mut work(),
    )
    .unwrap();
    let id = manager
        .submit(JobRequest {
            amount: 1.0,
            task: Default::default(),
            input,
            model: manifest(),
            budget: JobBudget {
                scratch_bytes: 4096,
                output_bytes: 1024 * 1024,
                work_units: 10_000_000,
                timeout: Duration::from_secs(30),
            },
        })
        .unwrap();
    let snapshot = manager.wait(id).unwrap();
    assert_eq!(snapshot.status, JobStatus::Ready, "{:?}", snapshot.failure);
    let admitted = manager.usage();
    assert_eq!(
        admitted.memory_bytes,
        128 * 1024
            + 2 * 1024 * 1024
            + 384_000
            + MODEL.len() as u64
            + 4096
            + 384_000
            + 3 * manifest().retained_bytes().unwrap()
    );
    assert_eq!(admitted.reserved_disk_bytes, 3 * 1024 * 1024);
    let prepared = manager
        .claim(snapshot.ticket, &captured, &manifest())
        .unwrap()
        .prepare(name("one-second.wav"))
        .unwrap();
    let metadata = prepared.review().artifacts[0].clone();
    assert_eq!(metadata.shape.frames, 48_000);
    assert_eq!(metadata.frame_origin, 0);
    assert_eq!(metadata.bytes, 384_058);
    assert_eq!(
        metadata
            .sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "1704336f534fe5e5105dc93e12b068a27ce34bc432c54955794ab16134fe886f"
    );
    let peak = PEAK.load(Ordering::SeqCst).saturating_sub(baseline);
    assert!(
        (peak as u64) <= admitted.memory_bytes,
        "measured Rust heap {peak} exceeds admitted {}",
        admitted.memory_bytes
    );
    let usage_during_commit = manager.usage();
    prepared.acknowledge_commit().retire();
    fixture.staging_empty();
    let final_usage = manager.usage();
    assert_eq!(final_usage.memory_bytes, 128 * 1024);
    assert_eq!(final_usage.reserved_disk_bytes, 0);
    assert_eq!(final_usage.published_disk_bytes, metadata.bytes);
    assert_eq!(
        windfall_codec::decode_file(metadata.published_path.as_ref().unwrap())
            .unwrap()
            .samples(),
        audio.samples()
    );
    eprintln!(
        "M1 one-second CPU lifecycle: elapsed_us={} admitted_memory={} Rust_heap_peak_above_baseline={} disk_reserved={} published_bytes={} completed_work={} output_sha={}",
        started.elapsed().as_micros(),
        admitted.memory_bytes,
        peak,
        usage_during_commit.reserved_disk_bytes,
        metadata.bytes,
        manager.snapshot(id).unwrap().completed_work,
        metadata
            .sha256
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
}

#[test]
fn helper_float32_wav_import_is_real_and_wrong_shape_or_integer_codec_is_refused() {
    struct EncodedAdapter {
        path: std::path::PathBuf,
    }
    impl AnalysisAdapter for EncodedAdapter {
        fn id(&self) -> &str {
            "test-only-copy"
        }
        fn version(&self) -> &str {
            "1"
        }
        fn run(
            &self,
            _: &CapturedInput,
            _: &VerifiedModel,
            context: &mut WorkerContext,
            artifacts: &mut ArtifactWriter,
        ) -> Result<()> {
            artifacts.import_audio(
                RelativeName::new("helper.wav")?,
                "lifecycle-copy",
                fs::File::open(&self.path)?,
                context.work(),
            )
        }
    }
    for (rate, channels, format, accepted) in [
        (48_000, 2, windfall_codec::WavSampleFormat::Float32, true),
        (44_100, 2, windfall_codec::WavSampleFormat::Float32, false),
        (48_000, 1, windfall_codec::WavSampleFormat::Float32, false),
        (48_000, 2, windfall_codec::WavSampleFormat::Int16, false),
    ] {
        let fixture = Fixture::new();
        let path = fixture.root.path().join("authored-helper.wav");
        let pcm =
            AudioBuffer::from_interleaved(rate, channels, vec![0.5; usize::from(channels) * 2]);
        windfall_codec::write_wav(&path, &pcm, format).unwrap();
        let manager = JobManager::new(
            fixture.config.clone(),
            fixture.cache.clone(),
            Arc::new(EncodedAdapter { path }),
        )
        .unwrap();
        let audio = source();
        let id = manager.submit(request(&audio)).unwrap();
        let status = manager.wait(id).unwrap();
        if accepted {
            assert_eq!(status.status, JobStatus::Ready, "{:?}", status.failure);
            let prepared = manager
                .claim(status.ticket, &stamp(&audio), &manifest())
                .unwrap()
                .prepare(name("helper-final.wav"))
                .unwrap();
            let assets = prepared.acknowledge_commit().retire();
            assert_eq!(
                windfall_codec::decode_file(assets[0].published_path.as_ref().unwrap())
                    .unwrap()
                    .samples(),
                &[0.5; 4]
            );
        } else {
            assert_eq!(status.status, JobStatus::Failed);
        }
        fixture.staging_empty();
    }
}

#[test]
fn multiple_aligned_roles_and_partial_publication_preserve_first_final_and_competitor() {
    struct MultiAdapter;
    impl AnalysisAdapter for MultiAdapter {
        fn id(&self) -> &str {
            "test-only-copy"
        }
        fn version(&self) -> &str {
            "1"
        }
        fn run(
            &self,
            input: &CapturedInput,
            model: &VerifiedModel,
            context: &mut WorkerContext,
            artifacts: &mut ArtifactWriter,
        ) -> Result<()> {
            for (index, role) in model.manifest().outputs.iter().enumerate() {
                artifacts.start_audio(
                    RelativeName::new(format!("role-{index}.wav"))?,
                    &role.role,
                    context.work(),
                )?;
                if role.channels == 1 {
                    let mono = [input.selected_samples()[0], input.selected_samples()[2]];
                    artifacts.write_audio(&mono, context.work())?;
                } else {
                    artifacts.write_audio(input.selected_samples(), context.work())?;
                }
                artifacts.finish_audio(context.work())?;
            }
            Ok(())
        }
    }
    let fixture = Fixture::new();
    let manager = JobManager::new(
        fixture.config.clone(),
        fixture.cache.clone(),
        Arc::new(MultiAdapter),
    )
    .unwrap();
    let audio = source();
    let mut model = manifest();
    model.outputs.push(OutputRole {
        role: "aligned-mono".into(),
        channels: 1,
    });
    let mut job = request(&audio);
    job.model = model.clone();
    let id = manager.submit(job).unwrap();
    let status = manager.wait(id).unwrap();
    assert_eq!(status.status, JobStatus::Ready);
    let review = manager.review(status.ticket).unwrap();
    assert_eq!(review.artifacts.len(), 2);
    assert!(
        review
            .artifacts
            .iter()
            .all(|a| a.shape.frames == 2 && a.shape.sample_rate == 48_000 && a.frame_origin == 1)
    );
    assert_eq!(review.artifacts[1].shape.channels, 1);
    let competitor = fixture.config.published_root.join("role-two.wav");
    fs::write(&competitor, b"competitor second role").unwrap();
    let result = manager
        .claim(status.ticket, &stamp(&audio), &model)
        .unwrap()
        .prepare(vec![
            RelativeName::new("role-one.wav").unwrap(),
            RelativeName::new("role-two.wav").unwrap(),
        ]);
    assert!(matches!(result, Err(AnalysisError::Collision)));
    let first = fixture.config.published_root.join("role-one.wav");
    let first_bytes = fs::read(&first).unwrap();
    assert_eq!(manager.usage().published_files, 1);
    assert_eq!(fs::read(&competitor).unwrap(), b"competitor second role");
    manager.cancel(id).unwrap();
    fixture.staging_empty();
    manager.forget(id).unwrap();
    manager.shutdown();
    assert_eq!(fs::read(&first).unwrap(), first_bytes);
    assert_eq!(fs::read(&competitor).unwrap(), b"competitor second role");
}

#[test]
fn absent_corrupt_model_and_input_rate_channel_empty_bounds_fail_before_adapter() {
    let fixture = Fixture::new();
    let adapter = Arc::new(CopyAdapter::default());
    let manager = fixture.manager(adapter.clone());
    let audio = source();
    let mut missing = request(&audio);
    missing.model.sha256 = [2; 32];
    let id = manager.submit(missing).unwrap();
    assert!(
        manager
            .wait(id)
            .unwrap()
            .failure
            .unwrap()
            .contains("absent")
    );
    assert_eq!(adapter.starts.load(Ordering::SeqCst), 0);
    let cache_path = fixture
        .cache
        .import_reader(&manifest(), Cursor::new(MODEL), &mut work())
        .unwrap();
    fs::write(&cache_path, b"wrong size").unwrap();
    let id = manager.submit(request(&audio)).unwrap();
    assert!(manager.wait(id).unwrap().failure.unwrap().contains("size"));
    assert_eq!(adapter.starts.load(Ordering::SeqCst), 0);
    fixture.staging_empty();
    for (rate, channels, samples) in [
        (7999, 2, vec![0.0; 8]),
        (192_001, 2, vec![0.0; 8]),
        (48_000, 3, vec![0.0; 12]),
        (48_000, 2, vec![]),
    ] {
        let buffer = AudioBuffer::from_interleaved(rate, channels, samples);
        let mut captured = stamp(&audio);
        captured.input_shape = AudioShape::of(&buffer);
        assert!(
            CapturedInput::capture(&buffer, captured, InputLimits::default(), &mut work()).is_err()
        );
    }
    let mut capture = stamp(&audio);
    capture.source_fingerprint.bytes = 0;
    assert!(CapturedInput::capture(&audio, capture, InputLimits::default(), &mut work()).is_err());
    let transient = source();
    let mut capture = stamp(&audio);
    capture.source_identity = transient.identity();
    drop(transient);
    assert!(CapturedInput::capture(&audio, capture, InputLimits::default(), &mut work()).is_err());
    for mutated in 0..5 {
        let mut model = manifest();
        match mutated {
            0 => model.provenance.license_reference.clear(),
            1 => model.provenance.device = "cuda".into(),
            2 => model.outputs.push(model.outputs[0].clone()),
            3 => model.outputs[0].channels = 3,
            _ => model.provenance.source_revision = "x".repeat(1025),
        }
        assert!(model.validate().is_err());
    }
}

#[test]
fn running_cancel_drops_open_partial_wav_before_private_directory() {
    let fixture = Fixture::new();
    let gate = Gate::new(2);
    let manager = fixture.manager(Arc::new(CopyAdapter {
        mid_output_gate: Some(gate.clone()),
        ..CopyAdapter::default()
    }));
    let audio = source();
    let id = manager.submit(request(&audio)).unwrap();
    let release = gate.hold();
    let directory = fs::read_dir(&fixture.config.staging_root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1); // open codec temp
    let before = manager.snapshot(id).unwrap();
    manager.cancel(id).unwrap();
    release.release();
    let cancelled = manager.wait(id).unwrap();
    assert_eq!(cancelled.status, JobStatus::Cancelled);
    assert!(cancelled.sequence > before.sequence);
    assert!(cancelled.completed_work >= before.completed_work);
    fixture.staging_empty();
    assert_eq!(
        fs::read_dir(&fixture.config.published_root)
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn model_import_concurrency_disk_entry_and_declared_byte_reservations_are_bounded() {
    struct PausedReader {
        cursor: Cursor<Vec<u8>>,
        gate: Arc<Gate>,
        paused: bool,
    }
    impl Read for PausedReader {
        fn read(&mut self, chunk: &mut [u8]) -> io::Result<usize> {
            if !self.paused {
                self.paused = true;
                self.gate.entered.wait();
                self.gate.release.wait();
            }
            self.cursor.read(chunk)
        }
    }
    let fixture = Fixture::new();
    let payload = b"authored parallel local cache fixture";
    let mut model = manifest();
    model.sha256 = Sha256::digest(payload).into();
    model.bytes = payload.len() as u64;
    let gate = Gate::new(3);
    let mut handles = Vec::new();
    for _ in 0..2 {
        let cache = fixture.cache.clone();
        let model = model.clone();
        let gate = gate.clone();
        let payload = payload.to_vec();
        handles.push(std::thread::spawn(move || {
            cache.import_reader(
                &model,
                PausedReader {
                    cursor: Cursor::new(payload),
                    gate,
                    paused: false,
                },
                &mut work(),
            )
        }));
    }
    let release = gate.hold();
    let mut third = model.clone();
    third.sha256 = [3; 32];
    assert!(matches!(
        fixture
            .cache
            .import_reader(&third, Cursor::new(payload), &mut work()),
        Err(AnalysisError::Budget("model import/entry count"))
    ));
    release.release();
    let one = handles.remove(0).join().unwrap().unwrap();
    let two = handles.remove(0).join().unwrap().unwrap();
    assert_eq!(one, two);
    assert_eq!(fs::read(&one).unwrap(), payload);
    assert_eq!(
        fixture.cache.load(&model, &mut work()).unwrap().bytes(),
        payload
    );
    assert_eq!(fs::read_dir(fixture.cache.root()).unwrap().count(), 2);
    let mut small = manifest();
    small.max_bytes = small.bytes;
    let disk_cache = ModelCache::new(
        fixture.root.path().join("limited-disk"),
        ModelCacheLimits {
            max_model_bytes: small.bytes,
            max_cache_bytes: small.bytes,
            max_entries: 3,
            max_imports: 1,
        },
    )
    .unwrap();
    disk_cache
        .import_reader(&small, Cursor::new(MODEL), &mut work())
        .unwrap();
    let mut smaller = model.clone();
    smaller.max_bytes = smaller.bytes;
    assert!(matches!(
        disk_cache.import_reader(&smaller, Cursor::new(payload), &mut work()),
        Err(AnalysisError::Budget("model cache disk"))
    ));
    let entry_cache = ModelCache::new(
        fixture.root.path().join("limited-entries"),
        ModelCacheLimits {
            max_model_bytes: 4096,
            max_cache_bytes: 8192,
            max_entries: 1,
            max_imports: 1,
        },
    )
    .unwrap();
    entry_cache
        .import_reader(&manifest(), Cursor::new(MODEL), &mut work())
        .unwrap();
    assert!(matches!(
        entry_cache.import_reader(&model, Cursor::new(payload), &mut work()),
        Err(AnalysisError::Budget("model import/entry count"))
    ));
    let mut excessive = manifest();
    excessive.max_bytes = 4097;
    assert!(matches!(
        fixture
            .cache
            .import_reader(&excessive, Cursor::new(MODEL), &mut work()),
        Err(AnalysisError::Budget("model bytes"))
    ));
    assert_eq!(fs::read_dir(disk_cache.root()).unwrap().count(), 1);
    assert_eq!(fs::read_dir(entry_cache.root()).unwrap().count(), 1);
}

#[test]
fn shutdown_with_claimed_apply_retains_final_then_uncommitted_staging_retires() {
    let fixture = Fixture::new();
    let manager = fixture.manager(Arc::new(CopyAdapter::default()));
    let audio = source();
    let review = ready(&manager, &audio);
    let prepared = manager
        .claim(review.ticket, &stamp(&audio), &manifest())
        .unwrap()
        .prepare(name("shutdown-claimed.wav"))
        .unwrap();
    let path = prepared.review().artifacts[0]
        .published_path
        .clone()
        .unwrap();
    let bytes = fs::read(&path).unwrap();
    manager.shutdown();
    assert_eq!(
        manager.snapshot(review.job).unwrap().status,
        JobStatus::Ready
    );
    assert!(matches!(
        manager.cancel(review.job),
        Err(AnalysisError::Claimed)
    ));
    drop(prepared);
    assert_eq!(
        manager.wait(review.job).unwrap().status,
        JobStatus::Cancelled
    );
    fixture.staging_empty();
    manager.forget(review.job).unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
}
