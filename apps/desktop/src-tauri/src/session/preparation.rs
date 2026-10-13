//! Document/source admission paired with engine readiness. These owners stay
//! outside recording/State guards through both refusal and successful install.
use super::{Session, State};
use std::collections::HashSet;
use windfall_engine::{
    PreparationSnapshot, PreparedPublication, ProjectPublicationIntent, ProjectPublicationLease,
    ProjectRetirement, PublicationRefusal, SamplePool,
};
use windfall_project::{Project, SampleId};

pub(super) struct ProjectPreparation {
    engine: PreparationSnapshot,
    pool: SamplePool,
    sample_dir: Option<std::path::PathBuf>,
    factory_dir: std::path::PathBuf,
    loading: HashSet<SampleId>,
    generation: u64,
    edits: u64,
    replacements: u64,
    factory: Option<(u64, u64)>,
}

pub(super) struct ReadyProject {
    baseline: ProjectPreparation,
    ready: PreparedPublication,
}

impl Session {
    pub(super) fn project_preparation(&self, state: &State) -> ProjectPreparation {
        ProjectPreparation {
            engine: self.controller().preparation_snapshot(),
            pool: state.pool.clone(),
            sample_dir: state.sample_dir.clone(),
            factory_dir: self.inner.factory_dir.clone(),
            loading: state.loading.clone(),
            generation: state.generation,
            edits: state.edits,
            replacements: state.replacements,
            factory: factory_stamp(&state.pool),
        }
    }

    /// Caller has released every recording/document/controller guard.
    pub(super) fn retire_project(&self, retirement: &mut ProjectRetirement) {
        self.controller().take_retired(retirement);
        retirement.clear();
        windfall_dsp::asset::collect_retired();
    }
}

impl ProjectPreparation {
    /// Replacement references belong to the incoming project, not the old document.
    pub(super) fn with_sample_dir(mut self, dir: Option<std::path::PathBuf>) -> Self {
        self.sample_dir = dir;
        self
    }

    /// SF2 IO/parser work belongs to this unlocked worker phase.
    pub(super) fn load_sample_instruments(
        &self,
        project: &Project,
        pool: &SamplePool,
    ) -> Result<SamplePool, String> {
        let mut pool = pool.clone();
        pool.retain_video_audio(project);
        for video in project
            .settings
            .stage_media
            .videos
            .iter()
            .filter(|video| !video.muted)
        {
            if pool.video_audio(&video.path).is_none() {
                // Join picture bytes and PCM from one immutable read.
                use std::io::Read;
                let file = std::fs::File::open(&video.path)
                    .map_err(|error| format!("Cannot decode video audio '{}': cannot open video: {error}. Mute it or restore the file.", video.path))?;
                let mut bytes = Vec::new();
                file.take(windfall_codec::MAX_VIDEO_FILE_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| error.to_string())?;
                if bytes.len() as u64 > windfall_codec::MAX_VIDEO_FILE_BYTES {
                    return Err("Video exceeds the 64 MiB file budget".into());
                }
                let audio = windfall_codec::decode_video_audio(&bytes, &windfall_codec::DecodeOptions { max_decoded_bytes: windfall_codec::MAX_VIDEO_FILE_BYTES })
                    .map_err(|error| format!("Cannot decode video audio '{}': {error}. Mute it or choose AVI PCM / Ogg Vorbis.", video.path))?;
                if pool
                    .video_prepared_bytes()
                    .saturating_add(bytes.len())
                    .saturating_add(std::mem::size_of_val(audio.samples()))
                    > 128 * 1024 * 1024
                {
                    return Err(
                        "Prepared video picture bytes and audio exceed the 128 MiB project budget"
                            .into(),
                    );
                }
                pool.insert_video_source(video.path.clone(), audio, bytes);
            }
        }
        for channel in &project.channels {
            let Some(asset @ windfall_project::SampleInstrumentAsset::SoundFont { .. }) =
                &channel.sample_instrument
            else {
                continue;
            };
            let asset = asset.canonical(project);
            let path = asset.soundfont_path(project).expect("SoundFont path");
            if pool.has_sample_instrument_asset(&asset) {
                continue;
            }
            let file = windfall_project::file::resolve_sample_path(
                path,
                self.sample_dir.as_deref(),
                &self.factory_dir,
            )
            .ok_or("SoundFont reference cannot be resolved")?;
            if !file.exists() {
                pool.insert_unavailable_soundfont(
                    asset.clone(),
                    format!(
                        "Missing SoundFont: {}; choose its replacement in the editor",
                        file.display()
                    ),
                );
                continue;
            }
            let (bytes, _workspace) = super::sample_instruments::read_soundfont(
                &file,
                &pool.sample_bank_budget_handle(),
            )?;
            pool.insert_soundfont(asset.clone(), &bytes)?;
        }
        Ok(pool)
    }

    pub(super) fn prepare(
        self,
        project: &Project,
        pool: &SamplePool,
        intent: ProjectPublicationIntent,
    ) -> Result<ReadyProject, windfall_engine::ProjectPreparationError> {
        // Keep the original source handles through commit and off-guard drop.
        let pool = self
            .load_sample_instruments(project, pool)
            .map_err(|reason| {
                windfall_engine::ProjectPreparationError::Sampler(
                    windfall_engine::sampler_processing::SamplerPreparationError::Asset(reason),
                )
            })?;
        let ready = self.engine.clone().prepare(project, &pool, intent)?;
        Ok(ReadyProject {
            baseline: self,
            ready,
        })
    }
}

impl ReadyProject {
    pub(super) fn pool(&self) -> &SamplePool {
        self.ready.sampler_pool()
    }

    pub(super) fn publication<'a>(
        &'a mut self,
        session: &'a Session,
        state: &State,
    ) -> Result<ProjectPublicationLease<'a>, AdmissionRefusal> {
        if session
            .inner
            .update_installing
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(AdmissionRefusal::Document);
        }
        let before = &self.baseline;
        if state.generation != before.generation
            || state.edits != before.edits
            || state.replacements != before.replacements
        {
            return Err(AdmissionRefusal::Document);
        }
        if factory_stamp(&state.pool) != before.factory {
            return Err(AdmissionRefusal::Provider);
        }
        // Stream/provider refusal takes precedence over a retryable source or
        // plan refresh. The lease is borrowed: rejecting it owns no units.
        let lease = session
            .controller()
            .publication(&mut self.ready)
            .map_err(AdmissionRefusal::Engine)?;
        if !state.pool.same_sources(&before.pool) || state.loading != before.loading {
            return Err(AdmissionRefusal::Source);
        }
        Ok(lease)
    }

    /// Cheap recapture only after a truthful stale refusal, while the document
    /// guard still excludes musical edits. Retrying never adopts the old token.
    pub(super) fn refresh(
        &self,
        session: &Session,
        state: &State,
        refusal: &AdmissionRefusal,
    ) -> Option<ProjectPreparation> {
        let before = &self.baseline;
        if !matches!(
            refusal,
            AdmissionRefusal::Source | AdmissionRefusal::Engine(PublicationRefusal::StalePlan)
        ) || state.generation != before.generation
            || state.edits != before.edits
            || state.replacements != before.replacements
            || factory_stamp(&state.pool) != before.factory
        {
            return None;
        }
        let next = session.project_preparation(state);
        before.engine.same_environment(&next.engine).then_some(next)
    }
}

fn factory_stamp(pool: &SamplePool) -> Option<(u64, u64)> {
    pool.plugin_factory()
        .map(|factory| (factory.provider_identity(), factory.revision()))
}

#[derive(Debug)]
pub(super) enum AdmissionRefusal {
    Document,
    Source,
    Provider,
    Engine(PublicationRefusal),
}
impl std::fmt::Display for AdmissionRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Document => f.write_str("The project changed while audio was being prepared. Try again."),
            Self::Source => f.write_str("The source changed while audio was being prepared. Try again."),
            Self::Provider => f.write_str("The plugin provider or revision changed while audio was being prepared. Try again."),
            Self::Engine(error) => std::fmt::Display::fmt(error, f),
        }
    }
}
impl From<AdmissionRefusal> for String {
    fn from(error: AdmissionRefusal) -> Self {
        error.to_string()
    }
}
