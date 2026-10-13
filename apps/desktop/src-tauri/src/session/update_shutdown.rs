//! Owned update shutdown: native state is frozen before the unsaved decision.
use super::Session;
use crate::sync::lock;
use std::{sync::{Mutex, MutexGuard, TryLockError, atomic::Ordering}, time::{Duration, Instant}};
use windfall_project::{Command, PluginTarget, Project};

const CAPTURE_WAIT: Duration = Duration::from_secs(5);
fn bounded_lock<T>(mutex: &Mutex<T>, deadline: Instant) -> Result<MutexGuard<'_, T>, String> {
    loop {
        match mutex.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(error)) => return Ok(error.into_inner()),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(1)),
            Err(_) => return Err("Update preparation timed out waiting for native state or a save. Nothing was installed; finish or retry the plugin operation.".into()),
        }
    }
}

pub(crate) struct UpdateShutdown {
    session: Session,
    suspended: bool,
    monitoring: bool,
    audio_settings: windfall_ipc::AudioSettings,
    editors: Vec<(PluginTarget, Option<usize>)>,
    editors_closed: bool,
    captured: Option<Project>,
    restored: bool,
}
impl Session {
    pub(crate) fn begin_update_shutdown(&self) -> Result<UpdateShutdown, String> {
        self.begin_update_shutdown_at(|_| Ok(()))
    }
    // The native fixture table injects cancellation only between real stages.
    pub(super) fn begin_update_shutdown_at(&self, mut stage: impl FnMut(&'static str) -> Result<(), String>) -> Result<UpdateShutdown, String> {
        let deadline = Instant::now() + CAPTURE_WAIT;
        let saving = bounded_lock(&self.inner.save, deadline)?;
        // Joins an in-flight native capture's entire guarded owner lifetime.
        // A prepared result whose guard has ended is rejected by the freeze;
        // the fresh full capture below includes its unpublished native state.
        let recording = bounded_lock(&self.inner.recording, deadline)?;
        if recording.is_some() || self.inner.exporting.load(Ordering::Acquire)
            || self.inner.recording_finishing.load(Ordering::Acquire)
            || self.inner.performance_capturing.load(Ordering::Acquire)
        {
            return Err("Wait for export or recording completion before installing".into());
        }
        let state = self.state();
        self.inner.update_installing.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "Update installation already owns the session")?;
        let project = state.document.project().clone();
        let revision = self.plugin_revision();
        drop(state);
        drop(recording);
        drop(saving);
        let mut shutdown = UpdateShutdown {
            session: self.clone(), suspended: false, monitoring: false,
            audio_settings: self.engine_settings(), editors: Vec::new(),
            editors_closed: false, captured: None, restored: false,
        };
        let result: Result<(), String> = (|| {
            stage("frozen")?;
            if let Some(manager) = lock(&self.inner.plugins).clone() {
                shutdown.editors = manager.runtime.update_editors(CAPTURE_WAIT)?;
                stage("editors-retained")?;
                // Prevent new native editor changes while the decision is open.
                shutdown.editors_closed = true;
                manager.runtime.update_close_editors(CAPTURE_WAIT)?;
                stage("editors-closed")?;
                let captured = manager.runtime.capture_for_update(project.clone(), revision.ok_or("Missing plugin revision")?, CAPTURE_WAIT)
                    .map_err(|error| format!("Update preparation could not capture current native state; nothing was installed. {error}"))?;
                shutdown.captured = Some(captured);
            } else {
                shutdown.captured = Some(project.clone());
            }
            // This metadata-only checked batch preserves undo/redo. The engine
            // still owns the exact sounding instance until suspension; rollback
            // installs the captured plan while detached, before reopening audio.
            let captured = shutdown.captured.as_ref().expect("captured project");
            let mut commands = Vec::new();
            for binding in captured.plugin_bindings() {
                let before = project.plugin(binding.target).ok_or("Plugin disappeared during update capture")?;
                if before != binding {
                    let mut metadata = binding.parameters.clone();
                    for parameter in &mut metadata {
                        if let Some(old) = before.parameters.iter().find(|old| old.id == parameter.id) { parameter.value = old.value; }
                    }
                    if metadata != before.parameters || binding.auxiliary_inputs != before.auxiliary_inputs
                        || binding.capabilities.outputs != before.capabilities.outputs
                    {
                        commands.push(Command::ReadmitPlugin {
                            target: binding.target, state: binding.state.clone(),
                            parameters: binding.parameters.clone(),
                            auxiliary_inputs: binding.auxiliary_inputs.clone(),
                            outputs: binding.capabilities.outputs.clone(),
                        });
                    } else {
                        for parameter in &binding.parameters {
                            if !parameter.read_only && before.parameters.iter().any(|old| old.id == parameter.id && old.value != parameter.value) {
                                commands.push(Command::SetPluginParam { target: binding.target, id: parameter.id, value: parameter.value });
                            }
                        }
                        if before.state != binding.state {
                            commands.push(Command::SetPluginState { target: binding.target, state: binding.state.clone() });
                        }
                    }
                }
            }
            if !commands.is_empty() {
                let mut state = self.state();
                let applied = state.document.dispatch(Command::Batch { label: Some("Native plugin state before update".into()), commands }, None).map_err(|error| error.to_string())?;
                self.publish(&mut state, &applied.touched);
            }
            stage("captured")?;
            stage("published")?;
            shutdown.quiesce_at(&mut stage)?;
            Ok(())
        })();
        if let Err(error) = result {
            let recovery = shutdown.restore().err().map(|error| format!("; {error}")).unwrap_or_default();
            return Err(format!("{error}{recovery}"));
        }
        Ok(shutdown)
    }
}
impl UpdateShutdown {
    pub(super) fn session(&self) -> &Session { &self.session }
    pub(crate) fn save(&self, path: Option<&str>) -> Result<(), String> {
        self.session.project_save_owned(path, false, true).map(|_| ())
    }
    pub(crate) fn quiesce(&mut self) -> Result<(), String> {
        self.quiesce_at(&mut |_| Ok(()))
    }
    fn quiesce_at(&mut self, stage: &mut impl FnMut(&'static str) -> Result<(), String>) -> Result<(), String> {
        if self.suspended { return Ok(()); }
        let _configuring = lock(&self.session.inner.configuring);
        self.audio_settings = self.session.engine_settings();
        self.monitoring |= self.session.input_monitor_state().active;
        if self.monitoring { self.session.input_monitor_stop(); }
        stage("monitoring-stopped")?;
        self.suspended = true;
        self.session.inner.audio.suspend()?;
        stage("audio-suspended")?;
        if let Some(manager) = lock(&self.session.inner.plugins).clone() {
            manager.runtime.update_shutdown_barrier()?;
        }
        stage("owners-retired")?;
        Ok(())
    }
    pub(crate) fn rollback(mut self) -> Result<(), String> { self.restore() }
    fn restore(&mut self) -> Result<(), String> {
        if self.restored { return Ok(()); }
        // Never launch edits while rollback still owns the audio/editor boundary.
        let result = self.restore_owned();
        self.restored = true;
        self.session.inner.update_installing.store(false, Ordering::Release);
        if self.monitoring {
            self.monitoring = false;
            let monitor = self.session.input_monitor_start();
            if result.is_ok() { monitor?; }
        }
        result
    }
    fn restore_owned(&mut self) -> Result<(), String> {
        if self.captured.is_some() && !self.suspended {
            // A cancellation at capture/publication still drops held notes and
            // uses the same detached restore path as a failed installer.
            self.quiesce()?;
        }
        if self.suspended {
            let _configuring = lock(&self.session.inner.configuring);
            if self.captured.is_some() {
                let (mut project, pool, preparation) = {
                    let state = self.session.state();
                    (state.document.project().clone(), state.pool.clone(), self.session.project_preparation(&state))
                };
                // Reapply the retained opaque memento byte-for-byte. Save may
                // have relinked assets/paths; those accepted document changes
                // remain current, while native state comes from this owner.
                if let Some(captured) = &self.captured {
                    for binding in project.plugin_bindings_mut() {
                        if let Some(before) = captured.plugin(binding.target) {
                            binding.state = before.state.clone();
                        }
                    }
                }
                let mut prepared = preparation.prepare(&project, &pool, windfall_engine::ProjectPublicationIntent::Edit).map_err(|error| error.to_string())?;
                let mut retirement = {
                    let mut state = self.session.state();
                    let pool = prepared.pool().clone();
                    let lease = prepared.publication_for_update(self, &state)?;
                    let retirement = lease.install();
                    state.pool.install_sampler_preparation(&pool);
                    retirement
                };
                self.session.retire_project(&mut retirement);
            }
            self.session.inner.audio.reconfigure(&self.audio_settings);
            self.suspended = false;
            if !self.session.inner.audio.status().running {
                return Err("Audio could not resume after update cancellation; reopen Audio settings".into());
            }
            self.session.sync_transport();
        }
        if self.editors_closed {
            if let Some(manager) = lock(&self.session.inner.plugins).clone() {
                let mut errors = Vec::new();
                for (target, parent) in &self.editors {
                    if let Err(error) = manager.runtime.update_reopen_editor(*target, *parent) { errors.push(error); }
                }
                if !errors.is_empty() { return Err(format!("Plugin editors could not resume after update cancellation: {}", errors.join("; "))); }
            }
            self.editors_closed = false;
        }
        Ok(())
    }
}
impl Drop for UpdateShutdown {
    fn drop(&mut self) { let _ = self.restore(); }
}
