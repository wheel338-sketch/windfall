//! Edits to the document, and telling everyone about them.

use windfall_project::{
    AutomationId, AutomationTarget, ClipContent, ClipInit, Command, DispatchResult,
    DocumentSnapshot, MAX_SONG_TICKS, PlaylistTrackId, ProjectPatch, Touched,
};

use super::{Session, State};
use crate::events::Event;

impl Session {
    /// Native notifications schedule here; neither State nor owner idle waits
    /// for a processor. Recording exclusion covers the joined owner lifetime.
    pub(crate) fn capture_plugin_update(
        &self,
        runtime: &crate::plugins::Runtime,
        request: crate::plugins::PendingUpdate,
    ) -> Result<crate::plugins::CaptureOutcome, String> {
        let crate::plugins::Update::Capture { target, .. } = request.update else {
            return Err("Not a native state request".into());
        };
        let _recording = self.recording_idle()?;
        let current = |state: &State| {
            runtime.is_current(request.revision, request.token)
                && state
                    .document
                    .project()
                    .plugin(target)
                    .is_some_and(|binding| {
                        crate::plugins::binding_identity(binding) == request.binding
                    })
        };
        let desired = {
            let state = self.state();
            if !current(&state) {
                return Ok(crate::plugins::CaptureOutcome::Obsolete);
            }
            state
                .document
                .project()
                .plugin(target)
                .expect("checked binding")
                .parameters
                .clone()
        };
        let Some(captured) = runtime.capture_pending(request.clone(), desired.clone())? else {
            return Ok(crate::plugins::CaptureOutcome::Obsolete);
        };
        let state = self.state();
        if !current(&state) {
            return Ok(crate::plugins::CaptureOutcome::Obsolete);
        }
        let binding = state
            .document
            .project()
            .plugin(target)
            .expect("checked binding");
        if binding.parameters != desired {
            return Err("Plugin parameters changed during state capture; retrying".into());
        }
        let mut commands: Vec<_> = captured
            .parameters
            .iter()
            .filter(|(id, _)| {
                binding
                    .parameters
                    .iter()
                    .any(|param| param.id == *id && !param.read_only)
            })
            .map(|&(id, value)| Command::SetPluginParam { target, id, value })
            .collect();
        commands.push(Command::SetPluginState {
            target,
            state: captured.bytes.clone(),
        });
        let ticket = self.sample_edit_ticket(
            &state,
            Command::Batch {
                label: Some("Native plugin state".into()),
                commands,
            },
            None,
            Vec::new(),
        )?;
        drop(state);
        drop(_recording);
        let mut prepared = ticket.prepare()?;
        #[cfg(test)]
        self.pause("plugin-capture:prepared");
        let mut retirement = None;
        let result = {
            let _recording = self.recording_idle()?;
            let mut state = self.state();
            if !current(&state) {
                return Ok(crate::plugins::CaptureOutcome::Obsolete);
            }
            prepared.commit(&mut state, &mut retirement)
        };
        if let Some(retirement) = &mut retirement {
            self.retire_project(retirement);
        }
        result?;
        Ok(crate::plugins::CaptureOutcome::Accepted {
            restart: captured.restart,
            acknowledgement: runtime.acknowledge_committed_capture(&request, &captured),
        })
    }
    pub fn document_snapshot(&self) -> DocumentSnapshot {
        let state = self.state();
        state.document.snapshot(state.path_text())
    }

    /// Applies a command. The patch in the result is the one every window
    /// is sent.
    pub fn dispatch(
        &self,
        command: Command,
        gesture: Option<u64>,
    ) -> Result<DispatchResult, String> {
        self.check_native_note_settings(&command)?;
        // What the command is about to set, for linking the control last
        // moved and for recording the movement as automation.
        let tweaks = self.controller_tweak_probe(&command);
        let gesture = self.recording_edit_gesture(!tweaks.is_empty()).or(gesture);
        let result = self.dispatch_quiet(command, gesture);
        if result.is_ok() {
            self.controller_tweaked(&tweaks);
        }
        result
    }

    fn check_native_note_settings(&self, command: &Command) -> Result<(), String> {
        // Copy only target IDs under State. Native ownership queries may wait,
        // and therefore run after releasing every document/recording guard.
        let (targets, runtime) = {
            let state=self.state();
            let mut targets=Vec::new();
            fn collect(command:&Command, project:&windfall_project::Project, targets:&mut Vec<windfall_project::PluginTarget>) {
                match command {
                    Command::UpdateSettings {patch} if patch.pan_law.is_some_and(|law|law!=project.settings.pan_law) => {
                        targets.extend(project.plugins.iter().filter_map(|binding|matches!(binding.target,windfall_project::PluginTarget::Instrument{..}).then_some(binding.target)));
                    }
                    Command::SetChannelExpressionMap{id,map,..} => {
                        let native=map.modulation_x.plugin==windfall_dsp::PluginExpressionDestination::Pan
                            || map.modulation_y.plugin==windfall_dsp::PluginExpressionDestination::Pan
                            || map.release.plugin==windfall_dsp::PluginExpressionDestination::Pan
                            || map.modulation_x.plugin==windfall_dsp::PluginExpressionDestination::Volume
                            || map.modulation_y.plugin==windfall_dsp::PluginExpressionDestination::Volume
                            || map.release.plugin==windfall_dsp::PluginExpressionDestination::Volume;
                        let target=windfall_project::PluginTarget::Instrument{channel:*id};
                        if native && project.plugin(target).is_some(){targets.push(target);}
                    }
                    Command::Batch{commands,..} => for command in commands {collect(command,project,targets);},
                    _=>{}
                }
            }
            collect(command,state.document.project(),&mut targets);
            (targets,crate::sync::lock(&self.inner.plugins).as_ref().map(|manager|manager.runtime.clone()))
        };
        if !targets.is_empty() {
            let runtime=runtime.ok_or("Native note capabilities require a running hosted instrument")?;
            for target in targets { runtime.note_capabilities(target)?.require_pan_law(0)?; }
        }
        Ok(())
    }

    /// [`dispatch`](Self::dispatch) without telling the controller hub:
    /// for the edits the hub makes itself.
    pub(super) fn dispatch_quiet(
        &self,
        command: Command,
        gesture: Option<u64>,
    ) -> Result<DispatchResult, String> {
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            self.sample_edit_ticket(&state, command, gesture, Vec::new())?
                .ordinary()
        };
        self.finish_sample_edit(ticket)
    }

    /// Controller captures and dumps must bind the ticket under the document
    /// lock to the generation they observed, before off-thread preparation.
    pub(super) fn dispatch_controller_capture(
        &self,
        command: Command,
        gesture: Option<u64>,
        generation: u64,
    ) -> Result<DispatchResult, String> {
        self.dispatch_controller_batch(command, gesture, generation, None)
    }

    pub(super) fn dispatch_controller_batch(
        &self,
        command: Command,
        gesture: Option<u64>,
        generation: u64,
        revision: Option<u64>,
    ) -> Result<DispatchResult, String> {
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if state.generation != generation
                || revision.is_some_and(|r| r != state.document.revision())
            {
                return Err("Project changed. Try again.".into());
            }
            self.sample_edit_ticket(&state, command, gesture, Vec::new())?
                .ordinary()
        };
        self.finish_sample_edit(ticket)
    }

    /// Validate native ownership and apply its edit under the same document lock.
    /// An obsolete notification succeeds without changing the document; a
    /// temporary refusal remains an error so the manager can retain the edit.
    pub(crate) fn dispatch_plugin_update(
        &self,
        runtime: &crate::plugins::Runtime,
        (revision, token, binding): (u64, u64, u64),
        command: Command,
        gesture: Option<u64>,
    ) -> Result<bool, String> {
        #[cfg(test)]
        self.pause("plugin-update:apply");
        let _recording = self.recording_idle()?;
        let state = self.state();
        let target = match &command {
            Command::SetPluginParam { target, .. } | Command::SetPluginState { target, .. } => {
                *target
            }
            _ => return Err("This is not a native plugin update".into()),
        };
        if !runtime.is_current(revision, token)
            || !state
                .document
                .project()
                .plugin(target)
                .is_some_and(|current| crate::plugins::binding_identity(current) == binding)
        {
            return Ok(false);
        }
        let ticket = self.sample_edit_ticket(&state, command, gesture, Vec::new())?;
        drop(state);
        drop(_recording);
        let mut prepared = ticket.prepare()?;
        let mut retirement = None;
        {
            let _recording = self.recording_idle()?;
            let mut state = self.state();
            if !runtime.is_current(revision, token)
                || !state
                    .document
                    .project()
                    .plugin(target)
                    .is_some_and(|current| crate::plugins::binding_identity(current) == binding)
            {
                return Ok(false);
            }
            prepared.commit(&mut state, &mut retirement)?;
        }
        if let Some(retirement) = &mut retirement {
            self.retire_project(retirement);
        }
        Ok(true)
    }

    /// Makes an automation of `target` and puts it on the playlist, as one
    /// undo step: what "Create automation clip" on a knob or fader does.
    ///
    /// The automation starts as one point at the value the target has now,
    /// so nothing changes until the curve is drawn. Its clip goes on a new
    /// playlist track, at the end, from the start of the song for as long
    /// as the song is, and at least four bars.
    ///
    /// Creates, in this order: the automation, the playlist track and the
    /// clip. Fails when the project has nothing of that kind to automate.
    pub fn automate(&self, target: AutomationTarget) -> Result<DispatchResult, String> {
        let _recording = self.recording_idle()?;
        let state = self.state();
        let project = state.document.project();
        let clips = project.playlist.clips.iter();
        let song = clips
            .map(|clip| clip.start.saturating_add(clip.length))
            .max();
        let bars = project.settings.time_signature.ticks_per_bar() * 4;
        let length = song.unwrap_or(0).max(bars).min(MAX_SONG_TICKS);
        // A batch cannot pass an id from one command to the next, so the
        // ids the automation and the track will get are worked out first.
        let automation = AutomationId(project.next_id);
        let track = PlaylistTrackId(project.next_id.saturating_add(1));
        let batch = Command::Batch {
            label: Some("Create automation clip".to_owned()),
            commands: vec![
                Command::AddAutomation {
                    name: None,
                    target,
                    points: None,
                },
                Command::AddPlaylistTrack {
                    name: None,
                    index: None,
                },
                Command::AddClips {
                    clips: vec![ClipInit {
                        track,
                        start: 0,
                        length: Some(length),
                        offset: None,
                        muted: None,
                        content: ClipContent::Automation { automation },
                    }],
                },
            ],
        };
        let ticket = self.sample_edit_ticket(&state, batch, None, Vec::new())?;
        drop(state);
        drop(_recording);
        self.finish_sample_edit(ticket)
    }

    /// Undoes the last edit. `None` when there is nothing to undo.
    pub fn undo(&self) -> Option<ProjectPatch> {
        self.prepare_history(HistoryMove::Undo, None)
    }

    /// Review UI's Undo applies only to the exact latest applied batch. Never
    /// retry against a newer edit while native history preparation is pending.
    pub fn proposal_undo(
        &self,
        stamp: windfall_project::proposal::ProposalStamp,
    ) -> Result<ProjectPatch, String> {
        self.prepare_history_guarded(HistoryMove::Undo, Some(stamp), None)
            .ok_or_else(|| "batch undo was refused: project changed, recording is active, or preparation failed".into())
    }

    /// Applies the last undone edit again. `None` when there is none.
    pub fn redo(&self) -> Option<ProjectPatch> {
        self.prepare_history(HistoryMove::Redo, None)
    }

    /// Undoes or redoes until `cursor` history entries are applied.
    pub fn history_jump(&self, cursor: u32) -> ProjectPatch {
        self.prepare_history(HistoryMove::Jump(cursor), None)
            .unwrap_or_else(|| self.state().document.unchanged_patch())
    }

    /// Metadata-only path relinking after Save as; the exact audio handles and
    /// musical plan are unchanged. Musical edits always use `publish_prepared`.
    pub(super) fn publish(&self, state: &mut State, touched: &Touched) -> ProjectPatch {
        let patch = state.document.patch(touched);
        if !touched.is_empty() {
            state.edits += 1;
        }
        self.emit(Event::ProjectPatch(patch.clone()));
        patch
    }

    /// Publish bounded native desired-value metadata after Document dispatch,
    /// before the guaranteed serial engine install can be consumed. This does
    /// not service/join/capture the owner or prepare any native unit.
    pub(super) fn commit_prepared_parameters(&self, state: &State, touched: &Touched) {
        if !touched.is_empty()
            && let Some(manager) = &*crate::sync::lock(&self.inner.plugins)
        {
            manager.runtime.commit_parameters(state.document.project());
        }
    }

    /// Patch/sample bookkeeping AFTER the borrowed ready lease was installed.
    /// This helper neither constructs units nor falls back to legacy publication.
    pub(super) fn publish_prepared(
        &self,
        state: &mut State,
        touched: &Touched,
        prepared: &windfall_engine::SamplePool,
    ) -> ProjectPatch {
        self.publish_with_prepared(state, touched, prepared)
    }

    fn publish_with_prepared(
        &self,
        state: &mut State,
        touched: &Touched,
        prepared: &windfall_engine::SamplePool,
    ) -> ProjectPatch {
        self.refresh_input_monitor_signature(state);
        let patch = if touched.is_empty() {
            state.document.unchanged_patch()
        } else {
            state.document.patch(touched)
        };
        if !touched.is_empty() {
            self.sync_prepared_samples(state, prepared);
            state.edits += 1;
            if touched.channels {
                self.controller().panic_hardware();
                if state
                    .midi_target
                    .is_some_and(|id| state.document.project().channel(id).is_none())
                {
                    state.midi_target = None;
                }
            }
        }
        state.pool.install_sampler_preparation(prepared);
        self.sync_transport();
        self.emit(Event::ProjectPatch(patch.clone()));
        patch
    }

    /// A loader/cache notification only queues preparation. No native/DSP work
    /// or implicit retirement is allowed while its caller holds State.
    pub(super) fn push_project(&self, state: &State) {
        self.refresh_input_monitor_signature(state);
        if state
            .pool
            .needs_sampler_preparation(state.document.project())
        {
            self.queue_sampler_preparation();
        } else {
            self.queue_clip_preparation();
        }
    }
}

#[derive(Clone, Copy)]
enum HistoryMove {
    Undo,
    Redo,
    Jump(u32),
}
impl HistoryMove {
    fn apply(self, document: &mut windfall_project::Document) -> Option<Touched> {
        match self {
            Self::Undo => document.undo(),
            Self::Redo => document.redo(),
            Self::Jump(cursor) => Some(document.jump(cursor)),
        }
    }
}
impl Session {
    pub fn undo_from_window(&self, generation: u64) -> Result<Option<ProjectPatch>, String> {
        self.history_from_window(generation, HistoryMove::Undo)
    }
    pub fn redo_from_window(&self, generation: u64) -> Result<Option<ProjectPatch>, String> {
        self.history_from_window(generation, HistoryMove::Redo)
    }
    pub fn history_jump_from_window(
        &self,
        generation: u64,
        cursor: u32,
    ) -> Result<ProjectPatch, String> {
        self.history_from_window(generation, HistoryMove::Jump(cursor))?
            .ok_or_else(|| "The history operation was superseded".into())
    }
    fn history_from_window(
        &self,
        generation: u64,
        action: HistoryMove,
    ) -> Result<Option<ProjectPatch>, String> {
        if self.window_document_generation() != generation {
            return Err(super::PROJECT_REPLACED.into());
        }
        let result = self.prepare_history(action, Some(generation));
        if self.window_document_generation() != generation {
            return Err(super::PROJECT_REPLACED.into());
        }
        Ok(result)
    }

    // History can refer to a variant evicted from the bounded cache. Compile
    // its target snapshot before reacquiring the session lock as well.
    fn prepare_history(
        &self,
        action: HistoryMove,
        generation: Option<u64>,
    ) -> Option<ProjectPatch> {
        self.prepare_history_guarded(action, None, generation)
    }
    fn prepare_history_guarded(
        &self,
        action: HistoryMove,
        expected: Option<windfall_project::proposal::ProposalStamp>,
        generation: Option<u64>,
    ) -> Option<ProjectPatch> {
        drop(self.recording_idle().ok()?);
        for _ in 0..8 {
            let (
                mut document,
                mut pool,
                directory,
                generation,
                edits,
                replacements,
                loading,
                preparation,
            ) = {
                let state = self.state();
                if expected.is_some_and(|stamp| {
                    stamp.generation != state.generation
                        || stamp.revision != state.document.revision()
                }) {
                    return None;
                }
                if generation.is_some_and(|generation| state.generation != generation) {
                    return None;
                }
                (
                    state.document.clone(),
                    state.pool.clone(),
                    state.sample_dir.clone(),
                    state.generation,
                    state.edits,
                    state.replacements,
                    state.loading.clone(),
                    self.project_preparation(&state),
                )
            };
            let original_pool = pool.clone();
            let original_project = document.project().clone();
            action.apply(&mut document)?;
            for asset in &document.project().samples {
                if original_project
                    .sample(asset.id)
                    .is_some_and(|old| old.path != asset.path)
                {
                    pool.remove(asset.id);
                }
            }
            for asset in &document.project().samples {
                if !pool.contains(asset.id)
                    && !loading.contains(&asset.id)
                    && let Ok(path) =
                        super::samples::locate(asset, directory.as_deref(), &self.inner.factory_dir)
                    && let Some(audio) = self.inner.cache.peek(&path)
                {
                    pool.insert(asset.id, audio);
                }
            }
            let mut prepared = match preparation.prepare(
                document.project(),
                &pool,
                windfall_engine::ProjectPublicationIntent::Edit,
            ) {
                Ok(prepared) => prepared,
                Err(error) => {
                    self.emit(Event::ProjectWarnings(vec![error.to_string()]));
                    return None;
                }
            };
            #[cfg(test)]
            self.pause("sampler:history-prepared");
            let retirement;
            let result = {
                let _recording = self.recording_idle().ok()?;
                let mut state = self.state();
                if expected.is_some_and(|stamp| {
                    stamp.generation != state.generation
                        || stamp.revision != state.document.revision()
                }) {
                    return None;
                }
                if state.generation != generation || state.replacements != replacements {
                    return None;
                }
                if state.edits != edits {
                    continue;
                }
                // Resolved history sources are candidate additions, not the live baseline.
                if !state.pool.same_sources(&original_pool) || state.loading != loading {
                    continue;
                }
                let pool = prepared.pool().clone();
                let lease = match prepared.publication(self, &state) {
                    Ok(lease) => lease,
                    Err(error) => {
                        self.emit(Event::ProjectWarnings(vec![error.to_string()]));
                        return None;
                    }
                };
                let touched = action.apply(&mut state.document)?;
                self.commit_prepared_parameters(&state, &touched);
                retirement = lease.install();
                Some(self.publish_prepared(&mut state, &touched, &pool))
            };
            let mut retirement = retirement;
            self.retire_project(&mut retirement);
            return result;
        }
        self.emit(Event::ProjectWarnings(vec![
            "Sampler history preparation was superseded by repeated edits. Try again.".into(),
        ]));
        None
    }
}
