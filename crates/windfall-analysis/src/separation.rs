//! Offline four-stem waveform separation. Every allocation is chunk-sized;
//! whole-song input/output lives in private files, never in the inference loop.
use crate::{
    AnalysisAdapter, AnalysisError, ArtifactWriter, AudioShape, CapturedInput, InferenceTask,
    ModelManifest, RelativeName, Result, VerifiedModel, Work, WorkerContext,
};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub const ADAPTER_ID: &str = "windfall-onnx-separation";
pub const SCRATCH_BYTES: u64 = 512 * 1024 * 1024;

/// Prepare once per job, then run fixed-size chunks at the model rate/channels.
/// Chunks and each ordered stem use interleaved f32; the runtime handles layout.
pub trait SeparationBackend: Send + Sync {
    fn prepare(
        &self,
        manifest: &ModelManifest,
        weights: &[u8],
        work: &mut Work,
    ) -> Result<Box<dyn PreparedSeparation>>;
}
pub trait PreparedSeparation {
    fn run_chunk(&mut self, input: &[f32], work: &mut Work) -> Result<Vec<Vec<f32>>>;
}

/// Deterministic test/reference decomposition; never installed as production ML.
/// Linear FIR bands telescope exactly to the input, including stereo polarity.
pub struct ReferenceBackend;
struct ReferencePrepared {
    channels: usize,
    roles: Vec<String>,
}
impl SeparationBackend for ReferenceBackend {
    fn prepare(
        &self,
        model: &ModelManifest,
        _: &[u8],
        work: &mut Work,
    ) -> Result<Box<dyn PreparedSeparation>> {
        work.check()?;
        contract(model)?;
        Ok(Box::new(ReferencePrepared {
            channels: model.input_channels as usize,
            roles: model.onnx.as_ref().unwrap().stem_roles.clone(),
        }))
    }
}
impl PreparedSeparation for ReferencePrepared {
    fn run_chunk(&mut self, input: &[f32], work: &mut Work) -> Result<Vec<Vec<f32>>> {
        let mut out = vec![vec![0.0; input.len()]; 4];
        for (frame, samples) in input.chunks_exact(self.channels).enumerate() {
            if frame % 2048 == 0 {
                work.checkpoint(2048)?;
            }
            for (channel, &x) in samples.iter().enumerate() {
                let previous = |lag: usize| {
                    if frame >= lag {
                        input[(frame - lag) * self.channels + channel]
                    } else {
                        0.0
                    }
                };
                let low = (x + previous(1) + previous(2) + previous(3)) * 0.25;
                let mid = (x + previous(1)) * 0.5;
                for (stem, role) in self.roles.iter().enumerate() {
                    out[stem][frame * self.channels + channel] = match role.as_str() {
                        "bass" => low,
                        "vocals" => mid - low,
                        "drums" => x - mid,
                        "other" => 0.0,
                        _ => return Err(AnalysisError::Invalid("reference stem role")),
                    };
                }
            }
        }
        Ok(out)
    }
}
pub fn contract(model: &ModelManifest) -> Result<&crate::OnnxContract> {
    model.validate()?;
    let contract = model.onnx.as_ref().ok_or(AnalysisError::Invalid(
        "separation pack needs waveform ONNX contract",
    ))?;
    contract.validate_separation(model)?;
    Ok(contract)
}

/// Random access to an immutable, selected source. Reads may be bounded chunks.
pub trait FrameSource {
    fn shape(&self) -> AudioShape;
    fn read_frames(&mut self, start: u64, output: &mut [f32], work: &mut Work) -> Result<()>;
}
pub struct SliceSource<'a> {
    pub samples: &'a [f32],
    pub shape: AudioShape,
}
impl FrameSource for SliceSource<'_> {
    fn shape(&self) -> AudioShape {
        self.shape
    }
    fn read_frames(&mut self, start: u64, output: &mut [f32], work: &mut Work) -> Result<()> {
        work.checkpoint(output.len() as u64)?;
        let start = usize::try_from(start)
            .map_err(|_| AnalysisError::Exhausted)?
            .checked_mul(self.shape.channels as usize)
            .ok_or(AnalysisError::Exhausted)?;
        let slice = self
            .samples
            .get(
                start
                    ..start
                        .checked_add(output.len())
                        .ok_or(AnalysisError::Exhausted)?,
            )
            .ok_or(AnalysisError::Invalid("source read outside selection"))?;
        if slice.iter().any(|x| !x.is_finite()) {
            return Err(AnalysisError::Invalid("nonfinite source"));
        }
        output.copy_from_slice(slice);
        Ok(())
    }
}
pub struct PcmSource {
    pub file: std::fs::File,
    pub shape: AudioShape,
    pub origin: u64,
}
impl FrameSource for PcmSource {
    fn shape(&self) -> AudioShape {
        self.shape
    }
    fn read_frames(&mut self, start: u64, output: &mut [f32], work: &mut Work) -> Result<()> {
        let channels = u64::from(self.shape.channels);
        if !output.len().is_multiple_of(channels as usize)
            || start
                .checked_add(output.len() as u64 / channels)
                .is_none_or(|end| end > self.shape.frames)
        {
            return Err(AnalysisError::Invalid("PCM source read bounds"));
        }
        let offset = start
            .checked_add(self.origin)
            .and_then(|n| n.checked_mul(channels))
            .and_then(|n| n.checked_mul(4))
            .ok_or(AnalysisError::Exhausted)?;
        self.file.seek(SeekFrom::Start(offset))?;
        let mut bytes = [0u8; 16384];
        for chunk in output.chunks_mut(4096) {
            work.checkpoint(chunk.len() as u64)?;
            self.file.read_exact(&mut bytes[..chunk.len() * 4])?;
            for (x, b) in chunk.iter_mut().zip(bytes.chunks_exact(4)) {
                *x = f32::from_le_bytes(b.try_into().unwrap());
                if !x.is_finite() {
                    return Err(AnalysisError::Invalid("nonfinite PCM source"));
                }
            }
        }
        Ok(())
    }
}
pub fn rate_frames(frames: u64, from: u32, to: u32) -> Result<u64> {
    if from == 0 || to == 0 {
        return Err(AnalysisError::Invalid("resampler rate"));
    }
    frames
        .checked_mul(u64::from(to))
        .and_then(|n| n.checked_add(u64::from(from) / 2))
        .map(|n| (n / u64::from(from)).max(1))
        .ok_or(AnalysisError::Exhausted)
}

/// Absolute phase, reused recording kernel, symmetric 16-frame context.
/// Negative/model padding is zero; finite source boundaries are extended.
pub fn read_resampled(
    source: &mut dyn FrameSource,
    rate: u32,
    start: i64,
    output: &mut [f32],
    work: &mut Work,
) -> Result<()> {
    let shape = source.shape();
    let length = rate_frames(shape.frames, shape.sample_rate, rate)?;
    read_resampled_to_length(source, rate, start, output, length, work)
}
/// Restoration uses the captured frame count, avoiding double-rounded length
/// loss at odd sample-rate ratios. Any final fractional frame extends the edge.
pub fn read_resampled_to_length(
    source: &mut dyn FrameSource,
    rate: u32,
    start: i64,
    output: &mut [f32],
    length: u64,
    work: &mut Work,
) -> Result<()> {
    let shape = source.shape();
    if shape.frames == 0
        || !(8_000..=192_000).contains(&rate)
        || !(1..=2).contains(&shape.channels)
        || !(8_000..=192_000).contains(&shape.sample_rate)
        || !output.len().is_multiple_of(shape.channels as usize)
    {
        return Err(AnalysisError::Invalid("resample source shape"));
    }
    let channels = shape.channels as usize;
    let step = f64::from(shape.sample_rate) / f64::from(rate);
    let total = i64::try_from(length).map_err(|_| AnalysisError::Exhausted)?;
    // Work in 2048 output frames so even extreme rate ratios have fixed scratch.
    for (block_index, block) in output.chunks_mut(2048 * channels).enumerate() {
        let first = start + (block_index * 2048) as i64;
        let last = first + (block.len() / channels) as i64;
        block.fill(0.0);
        let valid_first = first.max(0);
        let valid_last = last.min(total);
        if valid_first >= valid_last {
            continue;
        }
        let low =
            (((valid_first as f64 * step).floor() as i64 - 16).max(0) as u64).min(shape.frames - 1);
        let high = (((valid_last - 1) as f64 * step).floor() as u64 + 18).min(shape.frames);
        let mut input = vec![0.0; (high - low) as usize * channels];
        source.read_frames(low, &mut input, work)?;
        for frame in valid_first..valid_last {
            let position = frame as f64 * step;
            let center = position.floor() as i64;
            for channel in 0..channels {
                let value = if shape.sample_rate == rate {
                    input[(frame as u64 - low) as usize * channels + channel]
                } else {
                    let mut sum = 0.0;
                    let mut weights = 0.0;
                    for tap in -15..=16 {
                        let index = (center + tap).clamp(0, shape.frames as i64 - 1) as u64;
                        let w =
                            windfall_core::resample::weight(position - (center + tap) as f64, step);
                        sum += f64::from(input[(index - low) as usize * channels + channel]) * w;
                        weights += w;
                    }
                    (sum / weights.max(1e-9)) as f32
                };
                block[(frame - first) as usize * channels + channel] = value;
            }
        }
        work.checkpoint(block.len() as u64)?;
    }
    Ok(())
}

/// Emits finalized interleaved stem blocks; normalized positive windows cover
/// both ends and odd final chunks. Live overlap never grows with song length.
pub fn separate_stream(
    source: &mut dyn FrameSource,
    model: &ModelManifest,
    prepared: &mut dyn PreparedSeparation,
    work: &mut Work,
    emit: &mut dyn FnMut(usize, &[f32], &mut Work) -> Result<()>,
) -> Result<()> {
    let spec = contract(model)?;
    model.validate_input(source.shape(), source.shape().frames)?;
    if source.shape().channels != model.input_channels {
        return Err(AnalysisError::Invalid("source/model channels"));
    }
    let c = model.input_channels as usize;
    let n = spec.segment_frames as usize;
    let context = spec.context_frames as usize;
    let hop = n - spec.overlap_frames as usize;
    let frames = rate_frames(
        source.shape().frames,
        source.shape().sample_rate,
        model.sample_rate,
    )?;
    let mut sums = vec![vec![0.0_f64; n * c]; 4];
    let mut weights = vec![0.0_f64; n];
    let mut input = vec![0.0; (n + context * 2) * c];
    let mut output = vec![0.0; hop * c];
    let mut start = 0u64;
    while start < frames {
        work.check()?;
        read_resampled(
            source,
            model.sample_rate,
            start as i64 - context as i64,
            &mut input,
            work,
        )?;
        let stems = prepared.run_chunk(&input, work)?;
        work.check()?; // cancelled native run can never publish output
        if stems.len() != 4
            || stems
                .iter()
                .any(|s| s.len() != input.len() || s.iter().any(|x| !x.is_finite()))
        {
            return Err(AnalysisError::Adapter(
                "Model returned invalid four-stem waveform shape or nonfinite values.".into(),
            ));
        }
        for frame in 0..n {
            let window = (std::f64::consts::PI * (frame as f64 + 0.5) / n as f64)
                .sin()
                .powi(2);
            weights[frame] += window;
            for stem in 0..4 {
                for channel in 0..c {
                    sums[stem][frame * c + channel] +=
                        f64::from(stems[stem][(frame + context) * c + channel]) * window;
                }
            }
        }
        let flush = hop.min((frames - start) as usize);
        for stem in 0..4 {
            for frame in 0..flush {
                for channel in 0..c {
                    output[frame * c + channel] =
                        (sums[stem][frame * c + channel] / weights[frame]) as f32;
                }
            }
            emit(stem, &output[..flush * c], work)?;
            sums[stem].copy_within(hop * c.., 0);
            sums[stem][(n - hop) * c..].fill(0.0);
        }
        weights.copy_within(hop.., 0);
        weights[n - hop..].fill(0.0);
        start += flush as u64;
        work.checkpoint((n * c * 4) as u64)?;
    }
    Ok(())
}

/// Shared runtime choice is read once when a job starts; changes cannot alter a
/// running session. ORT itself admits just one dynamic library per app process.
pub type RuntimeChoice = Arc<RwLock<Option<PathBuf>>>;
pub struct SeparationAdapter {
    pub backend: Arc<dyn SeparationBackend>,
}
impl AnalysisAdapter for SeparationAdapter {
    fn id(&self) -> &str {
        ADAPTER_ID
    }
    fn version(&self) -> &str {
        "1"
    }
    fn supports(&self, task: InferenceTask) -> bool {
        task == InferenceTask::StemSeparation
    }
    fn required_scratch_bytes(&self, _: InferenceTask) -> u64 {
        SCRATCH_BYTES
    }
    fn scratch_buffer_bytes(&self, _: u64) -> u64 {
        0
    }
    fn run_task(
        &self,
        task: InferenceTask,
        input: &CapturedInput,
        model: &VerifiedModel,
        context: &mut WorkerContext,
        artifacts: &mut ArtifactWriter,
    ) -> Result<()> {
        if !self.supports(task) {
            return Err(AnalysisError::Invalid("separation task"));
        }
        self.run(input, model, context, artifacts)
    }
    fn run(
        &self,
        input: &CapturedInput,
        model: &VerifiedModel,
        context: &mut WorkerContext,
        artifacts: &mut ArtifactWriter,
    ) -> Result<()> {
        let manifest = model.manifest();
        let spec = contract(manifest)?;
        let mut prepared = self
            .backend
            .prepare(manifest, model.bytes(), context.work())?;
        // Spools are in the writer's owned job directory and disk reservation.
        // On any failure the ordinary job retirement removes the entire directory.
        let mut files = (0..4)
            .map(|_| tempfile::tempfile_in(artifacts.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        let mut source = input.frame_source()?;
        separate_stream(
            source.as_mut(),
            manifest,
            prepared.as_mut(),
            context.work(),
            &mut |stem, samples, work| {
                for chunk in samples.chunks(4096) {
                    work.checkpoint(chunk.len() as u64)?;
                    let mut bytes = [0u8; 16384];
                    for (sample, slot) in chunk.iter().zip(bytes.chunks_exact_mut(4)) {
                        slot.copy_from_slice(&sample.to_le_bytes());
                    }
                    files[stem].write_all(&bytes[..chunk.len() * 4])?;
                }
                Ok(())
            },
        )?;
        drop(prepared);
        let source_shape = source.shape();
        let model_frames = rate_frames(
            source_shape.frames,
            source_shape.sample_rate,
            manifest.sample_rate,
        )?;
        let mut block =
            vec![0.0; 4096 / manifest.input_channels as usize * manifest.input_channels as usize];
        for (index, file) in files.into_iter().enumerate() {
            let role = &spec.stem_roles[index];
            artifacts.start_audio(
                RelativeName::new(format!("{role}.wav"))?,
                role,
                context.work(),
            )?;
            let mut source = PcmSource {
                file,
                origin: 0,
                shape: AudioShape {
                    frames: model_frames,
                    channels: manifest.input_channels,
                    sample_rate: manifest.sample_rate,
                },
            };
            let mut start = 0;
            while start < source_shape.frames {
                let count = (source_shape.frames - start)
                    .min((block.len() / manifest.input_channels as usize) as u64)
                    as usize;
                let block = &mut block[..count * manifest.input_channels as usize];
                read_resampled_to_length(
                    &mut source,
                    source_shape.sample_rate,
                    start as i64,
                    block,
                    source_shape.frames,
                    context.work(),
                )?;
                artifacts.write_audio(block, context.work())?;
                start += count as u64;
            }
            artifacts.finish_audio(context.work())?;
        }
        Ok(())
    }
}

/// Keep the established classical runtime available alongside ONNX jobs.
pub struct ProductionAdapter {
    pub separation: SeparationAdapter,
}
impl AnalysisAdapter for ProductionAdapter {
    fn id(&self) -> &str {
        ADAPTER_ID
    }
    fn version(&self) -> &str {
        "1"
    }
    fn accepts_model(&self, model: &ModelManifest) -> bool {
        model.is_builtin_dereverb()
            || (model.provenance.adapter_id == self.id()
                && model.provenance.adapter_version == self.version())
    }
    fn supports(&self, task: InferenceTask) -> bool {
        self.separation.supports(task) || crate::ClassicalDereverbAdapter.supports(task)
    }
    fn required_scratch_bytes(&self, task: InferenceTask) -> u64 {
        if task == InferenceTask::StemSeparation {
            SCRATCH_BYTES
        } else {
            crate::DEREVERB_SCRATCH_BYTES
        }
    }
    fn scratch_buffer_bytes(&self, _: u64) -> u64 {
        0
    }
    fn builtin_model(&self, model: &ModelManifest) -> Result<Option<VerifiedModel>> {
        if model.is_builtin_dereverb() {
            crate::ClassicalDereverbAdapter.builtin_model(model)
        } else {
            Ok(None)
        }
    }
    fn run_task(
        &self,
        task: InferenceTask,
        input: &CapturedInput,
        model: &VerifiedModel,
        context: &mut WorkerContext,
        artifacts: &mut ArtifactWriter,
    ) -> Result<()> {
        if task == InferenceTask::StemSeparation {
            self.separation
                .run_task(task, input, model, context, artifacts)
        } else {
            crate::ClassicalDereverbAdapter.run_task(task, input, model, context, artifacts)
        }
    }
    fn run(
        &self,
        _: &CapturedInput,
        _: &VerifiedModel,
        _: &mut WorkerContext,
        _: &mut ArtifactWriter,
    ) -> Result<()> {
        Err(AnalysisError::Invalid(
            "Select stem separation or a dereverb mode",
        ))
    }
}
