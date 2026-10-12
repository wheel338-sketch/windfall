//! Trusted built-in production adapter. Zero model bytes are an explicit
//! algorithm descriptor, never a fake imported weight file/cache entry.
use crate::dereverb::{DereverbMode, DereverbSettings, DereverbStream};
use crate::{
    AnalysisAdapter, AnalysisError, ArtifactWriter, AudioShape, CapturedInput, InferenceTask,
    ModelManifest, ModelProvenance, OutputRole, RelativeName, Result, VerifiedModel, WorkerContext,
};
pub const BUILTIN_DEREVERB_ID: &str = "windfall-classical-dereverb";
pub const DEREVERB_SCRATCH_BYTES: u64 = 64 * 1024 * 1024;
pub struct ClassicalDereverbAdapter;
impl ClassicalDereverbAdapter {
    pub fn descriptor(shape: AudioShape, task: InferenceTask) -> Result<ModelManifest> {
        let role = match task {
            InferenceTask::DereverbGeneral => "dereverb-general",
            InferenceTask::DereverbVocal => "dereverb-vocal",
            _ => return Err(AnalysisError::Invalid("built-in dereverb task")),
        };
        let manifest = ModelManifest {
            id: BUILTIN_DEREVERB_ID.into(),
            version: "1".into(),
            revision: 1,
            sha256: [0; 32],
            bytes: 0,
            max_bytes: 0,
            onnx: None,
            provenance: ModelProvenance {
                origin: "builtin:windfall/classical-dereverb".into(),
                source_revision: "1".into(),
                author: "Windfall contributors".into(),
                license_spdx: "GPL-3.0-only".into(),
                license_reference: "Windfall repository LICENSE (algorithm code; no weights)"
                    .into(),
                adapter_id: BUILTIN_DEREVERB_ID.into(),
                adapter_version: "1".into(),
                device: "cpu".into(),
            },
            sample_rate: shape.sample_rate,
            input_channels: shape.channels,
            max_input_frames: shape.frames,
            outputs: vec![OutputRole {
                role: role.into(),
                channels: shape.channels,
            }],
        };
        manifest.validate()?;
        task.validate(&manifest)?;
        Ok(manifest)
    }
}
impl AnalysisAdapter for ClassicalDereverbAdapter {
    fn id(&self) -> &str {
        BUILTIN_DEREVERB_ID
    }
    fn version(&self) -> &str {
        "1"
    }
    fn supports(&self, task: InferenceTask) -> bool {
        matches!(
            task,
            InferenceTask::DereverbGeneral | InferenceTask::DereverbVocal
        )
    }
    fn required_scratch_bytes(&self, _: InferenceTask) -> u64 {
        DEREVERB_SCRATCH_BYTES
    }
    fn scratch_buffer_bytes(&self, _: u64) -> u64 {
        0
    } // budget reserved for bounded DSP-owned vectors
    fn builtin_model(&self, manifest: &ModelManifest) -> Result<Option<VerifiedModel>> {
        if !manifest.is_builtin_dereverb() {
            return Err(AnalysisError::Adapter("ONNX pack registered, but the ONNX Runtime adapter is not installed. Built-in dereverb uses no model pack.".into()));
        }
        manifest.validate()?;
        Ok(Some(VerifiedModel::builtin(manifest.clone())))
    }
    fn run_task(
        &self,
        task: InferenceTask,
        input: &CapturedInput,
        model: &VerifiedModel,
        context: &mut WorkerContext,
        artifacts: &mut ArtifactWriter,
    ) -> Result<()> {
        if !self.supports(task) || !model.manifest().is_builtin_dereverb() {
            return Err(AnalysisError::Invalid("built-in dereverb binding"));
        }
        task.validate(model.manifest())?;
        let mode = if task == InferenceTask::DereverbVocal {
            DereverbMode::Vocal
        } else {
            DereverbMode::General
        };
        let mut processor = DereverbStream::new(
            input.stamp().input_shape.sample_rate,
            input.stamp().input_shape.channels,
            DereverbSettings {
                mode,
                amount: context.amount(),
            },
        )?;
        let role = &model.manifest().outputs[0].role;
        artifacts.start_audio(
            RelativeName::new(format!("{role}.wav"))?,
            role,
            context.work(),
        )?;
        let mut emit =
            |samples: &[f32], work: &mut crate::Work| artifacts.write_audio(samples, work);
        input.for_each_selected(context.work(), |block, work| {
            processor.push(block, work, &mut emit)
        })?;
        processor.finish(context.work(), &mut emit)?;
        artifacts.finish_audio(context.work())
    }
    fn run(
        &self,
        _: &CapturedInput,
        _: &VerifiedModel,
        _: &mut WorkerContext,
        _: &mut ArtifactWriter,
    ) -> Result<()> {
        Err(AnalysisError::Invalid(
            "Select general or vocal dereverb mode",
        ))
    }
}
