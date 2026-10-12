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
    pub(super) fn prepare(
        self,
        project: &Project,
        pool: &SamplePool,
        intent: ProjectPublicationIntent,
    ) -> Result<ReadyProject, windfall_engine::ProjectPreparationError> {
        // Keep the original source handles through commit and off-guard drop.
        let ready = self.engine.clone().prepare(project, pool, intent)?;
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
