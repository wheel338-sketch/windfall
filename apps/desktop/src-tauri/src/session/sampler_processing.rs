//! Checked sampler preparation, outside document and recording locks.
use super::{Session, State};
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use windfall_core::AudioBuffer;
use windfall_engine::{ProjectPublicationIntent, ProjectRetirement, SamplePool};
use windfall_project::{Applied, Command, DispatchResult, Document, SampleId};

/// Candidate import/edit snapshot. The caller owns recording/library exclusion
/// when taking this ticket and committing its result; preparation takes no locks.
pub(super) struct SampleEditTicket {
    session: Session,
    command: Command,
    applied: Applied,
    gesture: Option<u64>,
    document: Document,
    original_pool: SamplePool,
    candidate_pool: SamplePool,
    recovered: HashSet<SampleId>,
    loading: HashSet<SampleId>,
    generation: u64,
    edits: u64,
    replacements: u64,
    request: Option<u64>,
    preparation: Option<super::preparation::ProjectPreparation>,
    preserve_noop: bool,
    retry_sources: bool,
    installed: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

pub(super) struct PreparedSampleEdit {
    ticket: SampleEditTicket,
    prepared: super::preparation::ReadyProject,
}

impl SampleEditTicket {
    /// Ordinary dispatch still advances its patch revision for a no-op. The
    /// explicit sampler recovery API instead preserves its document/gesture.
    pub(super) fn ordinary(mut self) -> Self {
        self.preserve_noop = false;
        self
    }

    pub(super) fn on_install(
        mut self,
        installed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        self.installed = Some(installed);
        self
    }
    fn current(&self) -> bool {
        self.request
            .is_none_or(|id| self.session.inner.sampler_ticket.load(Ordering::Acquire) == id)
    }

    /// Slow DSP/compilation; call only after dropping State/library guards.
    /// Does not acquire recording exclusion, including for recording-finish imports.
    pub(super) fn prepare(mut self) -> Result<PreparedSampleEdit, String> {
        self.candidate_pool = self.preparation.as_ref().expect("unprepared edit")
            .load_sample_instruments(self.document.project(),&self.candidate_pool)?;
        let pool = self
            .candidate_pool
            .prepare_samplers(
                self.document.project(),
                &mut || self.current(),
                &mut |_, done, total| {
                    if self.request.is_some() && self.current() {
                        self.session
                            .inner
                            .sampler_done
                            .store(u32::from(done), Ordering::Release);
                        self.session
                            .inner
                            .sampler_total
                            .store(u32::from(total), Ordering::Release);
                    }
                },
            )
            .map_err(|error| error.to_string())?;
        if !self.current() {
            return Err("Sampler preparation was cancelled.".into());
        }
        // The ticket remains the source/cancellation owner through preparation.
        let preparation = self.preparation.take().expect("unprepared edit");
        let prepared = preparation
            .prepare(
                self.document.project(),
                &pool,
                ProjectPublicationIntent::Edit,
            )
            .map_err(|error| error.to_string())?;
        Ok(PreparedSampleEdit {
            ticket: self,
            prepared,
        })
    }
}

impl PreparedSampleEdit {
    /// Final caller-held recording -> library -> State exclusion is required.
    /// Caller must additionally recheck file/root/version/project-directory guards.
    pub(super) fn commit(
        &mut self,
        state: &mut State,
        retirement: &mut Option<ProjectRetirement>,
    ) -> Result<DispatchResult, String> {
        let ticket = &self.ticket;
        if retirement.is_some() {
            return Err(
                "Release the preceding project retirement outside guards before reusing its slot."
                    .into(),
            );
        }
        if !ticket.current() {
            return Err("Sampler preparation was cancelled.".into());
        }
        if state.generation != ticket.generation
            || state.edits != ticket.edits
            || state.replacements != ticket.replacements
        {
            return Err(
                "The project changed while sampler audio was being prepared. Try again.".into(),
            );
        }
        if !state.pool.same_sources(&ticket.original_pool) || state.loading != ticket.loading {
            return Err(
                "The source changed while sampler audio was being prepared. Try again.".into(),
            );
        }
        let pool = self.prepared.pool().clone();
        let lease = self.prepared.publication(&ticket.session, state)?;
        let applied = if ticket.preserve_noop && ticket.applied.touched.is_empty() {
            // Preserve even an open gesture: this publication only repairs runtime.
            ticket.applied.clone()
        } else {
            state
                .document
                .dispatch(ticket.command.clone(), ticket.gesture)
                .map_err(|error| error.to_string())?
        };
        // These no-op assignments were verified absent and settled in the
        // original snapshot. Attach only their prepared handles; generic no-op
        // publication must not fetch cache audio or replace a held source.
        for &id in &ticket.recovered {
            let audio = pool.get(id).expect("verified recovery source");
            state.pool.insert(id, audio.clone());
            state.loaded.insert(id);
            state.failed.remove(&id);
        }
        ticket
            .session
            .commit_prepared_parameters(state, &applied.touched);
        *retirement = Some(lease.install());
        if let Some(installed) = &ticket.installed {
            installed.store(true, Ordering::Release);
        }
        let patch = if !ticket.preserve_noop && applied.touched.is_empty() {
            state.pool.install_sampler_preparation(&pool);
            ticket.session.sync_transport();
            ticket.session.publish(state, &applied.touched)
        } else {
            ticket
                .session
                .publish_prepared(state, &applied.touched, &pool)
        };
        Ok(DispatchResult {
            created: applied.created,
            patch,
        })
    }

    /// The exact commit guard remains unchanged. Only completion of decoders
    /// that were already pending permits a new, fully prepared ticket. An
    /// unrelated source replacement/addition never enters this retry route.
    fn settled_load_retry(&mut self, state: &State) -> Result<Option<SampleEditTicket>, String> {
        let ticket = &self.ticket;
        if !ticket.current()
            || !ticket.retry_sources
            || ticket.loading.is_empty()
            || !state.loading.is_subset(&ticket.loading)
            || state.loading == ticket.loading
        {
            return Ok(None);
        }
        let settled = |id: &SampleId| ticket.loading.contains(id) && !state.loading.contains(id);
        let same = |pool: &SamplePool, other: &SamplePool| {
            pool.iter().all(|(id, audio)| {
                settled(&id)
                    || other
                        .get(id)
                        .is_some_and(|now| audio.identity() == now.identity())
            })
        };
        if !same(&ticket.original_pool, &state.pool) || !same(&state.pool, &ticket.original_pool) {
            return Ok(None);
        }
        let refusal = match self.prepared.publication(&ticket.session, state) {
            Ok(_) => return Ok(None),
            Err(error) => error,
        };
        let Some(preparation) = self.prepared.refresh(&ticket.session, state, &refusal) else {
            return Ok(None);
        };
        let mut next = ticket.session.sample_edit_ticket(
            state,
            ticket.command.clone(),
            ticket.gesture,
            Vec::new(),
        )?;
        next.preparation = Some(preparation);
        next.request = ticket.request;
        next.preserve_noop = ticket.preserve_noop;
        next.installed = ticket.installed.clone();
        Ok(Some(next))
    }
}

impl Session {
    /// Snapshot a candidate under the caller's existing lock order. Overlays are
    /// private until successful publication. A no-op can recover only a settled
    /// absent source explicitly assigned by the command to its candidate channel.
    pub(super) fn sample_edit_ticket(
        &self,
        state: &State,
        command: Command,
        gesture: Option<u64>,
        sources: Vec<(SampleId, AudioBuffer)>,
    ) -> Result<SampleEditTicket, String> {
        let retry_sources = sources.is_empty();
        let mut document = state.document.clone();
        let applied = document
            .dispatch(command.clone(), gesture)
            .map_err(|error| error.to_string())?;
        let mut candidate_pool = state.pool.clone();
        let mut recovered = HashSet::new();
        fn assigns(command: &Command, document: &Document, sample: SampleId) -> bool {
            match command {
                Command::SetChannelSample {
                    id,
                    sample: Some(assigned),
                } => {
                    *assigned == sample
                        && document
                            .project()
                            .channel(*id)
                            .is_some_and(|channel| channel.source.sample() == Some(sample))
                }
                Command::Batch { commands, .. } => commands
                    .iter()
                    .any(|command| assigns(command, document, sample)),
                _ => false,
            }
        }
        for (id, audio) in sources {
            if applied.touched.is_empty() {
                if state.pool.contains(id)
                    || state.loading.contains(&id)
                    || !assigns(&command, &document, id)
                {
                    continue;
                }
                recovered.insert(id);
            }
            if state.loading.contains(&id) {
                return Err("The candidate source is already loading. Wait and try again.".into());
            }
            if document.project().sample(id).is_none() {
                return Err("The candidate source is not registered in the project.".into());
            }
            candidate_pool.insert(id, audio);
        }
        if applied.touched.samples {
            for asset in &document.project().samples {
                if !candidate_pool.contains(asset.id)
                    && !state.loading.contains(&asset.id)
                    && let Ok(path) = super::samples::locate(
                        asset,
                        state.sample_dir.as_deref(),
                        &self.inner.factory_dir,
                    )
                    && let Some(audio) = self.inner.cache.peek(&path)
                {
                    candidate_pool.insert(asset.id, audio);
                }
            }
        }
        Ok(SampleEditTicket {
            session: self.clone(),
            command,
            applied,
            gesture,
            document,
            original_pool: state.pool.clone(),
            candidate_pool,
            recovered,
            loading: state.loading.clone(),
            generation: state.generation,
            edits: state.edits,
            replacements: state.replacements,
            request: None,
            preparation: Some(self.project_preparation(state)),
            preserve_noop: true,
            retry_sources,
            installed: None,
        })
    }
    pub fn sampler_preparation_begin(&self) -> Result<u64, String> {
        drop(self.recording_idle()?);
        let _state = self.state();
        let request = self.inner.sampler_ticket.fetch_add(1, Ordering::AcqRel) + 1;
        self.inner.sampler_done.store(0, Ordering::Release);
        self.inner.sampler_total.store(0, Ordering::Release);
        Ok(request)
    }

    pub fn sampler_preparation_cancel(&self, request: u64) {
        // Linearize cancellation with the final check/publication under state.
        let _state = self.state();
        let _ = self.inner.sampler_ticket.compare_exchange(
            request,
            request + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    pub fn sampler_preparation_progress(
        &self,
        request: u64,
    ) -> windfall_ipc::SamplerPreparationProgress {
        windfall_ipc::SamplerPreparationProgress {
            request,
            completed: self.inner.sampler_done.load(Ordering::Acquire),
            total: self.inner.sampler_total.load(Ordering::Acquire),
            current: self.inner.sampler_ticket.load(Ordering::Acquire) == request,
        }
    }

    pub fn prepare_sampler_command(
        &self,
        command: Command,
        request: u64,
    ) -> Result<DispatchResult, String> {
        fn checked(command: &Command) -> bool {
            match command {
                Command::UpdateSampler { .. } => true,
                Command::Batch { commands, .. } => {
                    !commands.is_empty() && commands.iter().all(checked)
                }
                _ => false,
            }
        }
        if !checked(&command) {
            return Err("Sampler preparation accepts only sampler settings edits.".into());
        }
        self.prepare_sampler_edit(command, None, Some(request))
    }

    pub(super) fn prepare_sampler_edit(
        &self,
        command: Command,
        gesture: Option<u64>,
        request: Option<u64>,
    ) -> Result<DispatchResult, String> {
        drop(self.recording_idle()?);
        let mut ticket = {
            let state = self.state();
            self.sample_edit_ticket(&state, command, gesture, Vec::new())?
        };
        ticket.request = request;
        self.finish_sample_edit(ticket)
    }

    pub(super) fn finish_sample_edit(
        &self,
        ticket: SampleEditTicket,
    ) -> Result<DispatchResult, String> {
        let mut ticket = ticket;
        for attempt in 0..8 {
            let mut prepared = ticket.prepare()?;
            let mut retirement = None;
            #[cfg(test)]
            self.pause("sampler:prepared");
            let (result, next) = {
                let _recording = self.recording_idle()?;
                let mut state = self.state();
                let result = prepared.commit(&mut state, &mut retirement);
                let next = if result.is_err() && attempt < 7 {
                    prepared.settled_load_retry(&state)?
                } else {
                    None
                };
                (result, next)
            };
            if let Some(retirement) = &mut retirement {
                self.retire_project(retirement);
            }
            let Some(next) = next else {
                return if attempt == 7 {
                    result.map_err(|error| format!("{error} Audio readiness retry limit (8 attempts) reached; edit refused."))
                } else {
                    result
                };
            };
            // The refused native/DSP candidate dies before the new constructors,
            // with all recording/document/controller guards released.
            drop(prepared);
            self.emit(crate::events::Event::ProjectWarnings(vec![
                "A pending sample load completed; retrying audio preparation from the current source snapshot.".into(),
            ]));
            ticket = next;
        }
        unreachable!("bounded edit retry returns on its eighth attempt")
    }

    /// Source reload/attachment can need a bank without a musical settings edit.
    pub(super) fn queue_sampler_preparation(&self) {
        if self.inner.preparing_samplers.swap(true, Ordering::AcqRel) {
            return;
        }
        let session = self.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("windfall-sampler-preparation".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    session.refresh_sampler_plan()
                }));
                let state = session.state();
                session
                    .inner
                    .preparing_samplers
                    .store(false, Ordering::Release);
                if matches!(result, Ok(true))
                    && state
                        .pool
                        .needs_sampler_preparation(state.document.project())
                {
                    session.queue_sampler_preparation();
                }
                if result.is_err() {
                    log::error!("sampler preparation worker stopped unexpectedly");
                }
            })
        {
            self.inner
                .preparing_samplers
                .store(false, Ordering::Release);
            log::error!("could not start sampler preparation: {error}");
        }
    }

    fn refresh_sampler_plan(&self) -> bool {
        // Continuous editing cannot turn a background worker into an unbounded loop.
        for _ in 0..8 {
            let (project, pool, generation, edits, replacements, preparation) = {
                let state = self.state();
                (
                    state.document.project().clone(),
                    state.pool.clone(),
                    state.generation,
                    state.edits,
                    state.replacements,
                    self.project_preparation(&state),
                )
            };
            let mut prepared =
                match preparation.prepare(&project, &pool, ProjectPublicationIntent::Edit) {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        self.emit(crate::events::Event::ProjectWarnings(vec![
                            error.to_string(),
                        ]));
                        return false;
                    }
                };
            #[cfg(test)]
            self.pause("sampler:background-prepared");
            let retirement;
            {
                let _recording = match self.recording_idle() {
                    Ok(recording) => recording,
                    Err(error) => {
                        self.emit(crate::events::Event::ProjectWarnings(vec![
                            error.to_string(),
                        ]));
                        return false;
                    }
                };
                let mut state = self.state();
                if state.generation != generation || state.replacements != replacements {
                    return false;
                }
                if state.edits != edits || !pool.same_sources(&state.pool) {
                    continue;
                }
                let pool = prepared.pool().clone();
                let lease = match prepared.publication(self, &state) {
                    Ok(lease) => lease,
                    Err(error) => {
                        self.emit(crate::events::Event::ProjectWarnings(vec![
                            error.to_string(),
                        ]));
                        return false;
                    }
                };
                retirement = lease.install();
                state.pool.install_sampler_preparation(&pool);
                self.sync_transport();
            }
            let mut retirement = retirement;
            self.retire_project(&mut retirement);
            return true;
        }
        self.emit(crate::events::Event::ProjectWarnings(vec!["Sampler preparation was superseded by repeated edits. Apply the sampler settings again.".into()]));
        false
    }
}
