//! An owned session freeze precedes the unsaved-work prompt. All edits,
//! controller work and prepared admissions use recording exclusion or final
//! publication admission, both of which refuse while this owner exists.
use super::Session;
use crate::sync::lock;
use std::sync::atomic::Ordering;

pub(crate) struct UpdateShutdown {
    session: Session,
    suspended: bool,
    monitoring: bool,
    audio_settings: windfall_ipc::AudioSettings,
}
impl Session {
    pub(crate) fn begin_update_shutdown(&self) -> Result<UpdateShutdown, String> {
        let saving = lock(&self.inner.save);
        let recording = self.recording_idle()?;
        let state = self.state();
        if self.inner.exporting.load(Ordering::Acquire)
            || self.inner.recording_finishing.load(Ordering::Acquire)
        {
            return Err("Wait for export or recording completion before installing".into());
        }
        self.inner.update_installing.store(true, Ordering::Release);
        // A prepared project can no longer publish; replacement tickets from
        // before the freeze cannot cross a successful installation.
        drop(state);
        drop(recording);
        drop(saving);
        Ok(UpdateShutdown {
            session: self.clone(),
            suspended: false,
            monitoring: false,
            audio_settings: self.engine_settings(),
        })
    }
}
impl UpdateShutdown {
    pub(crate) fn save(&self, path: Option<&str>) -> Result<(), String> {
        self.session
            .project_save_owned(path, false, true)
            .map(|_| ())
    }
    pub(crate) fn quiesce(&mut self) -> Result<(), String> {
        // 1. Stop input monitoring, 2. close/join the output callback,
        // 3. drain hosted-owner retirement on its own control thread.
        let _configuring = lock(&self.session.inner.configuring);
        self.audio_settings = self.session.engine_settings();
        self.monitoring = self.session.input_monitor_state().active;
        if self.monitoring {
            self.session.input_monitor_stop();
        }
        self.suspended = true; // rollback even if device acknowledgement fails
        self.session.inner.audio.suspend()?;
        if let Some(manager) = lock(&self.session.inner.plugins).clone() {
            manager.runtime.update_shutdown_barrier()?;
        }
        Ok(())
    }
    pub(crate) fn rollback(mut self) -> Result<(), String> {
        self.restore()
    }
    fn restore(&mut self) -> Result<(), String> {
        if self.suspended {
            let _configuring = lock(&self.session.inner.configuring);
            self.session.inner.audio.reconfigure(&self.audio_settings);
            self.suspended = false;
            if !self.session.inner.audio.status().running {
                self.session
                    .inner
                    .update_installing
                    .store(false, Ordering::Release);
                return Err(
                    "Audio could not resume after update cancellation; reopen Audio settings"
                        .into(),
                );
            }
        }
        self.session
            .inner
            .update_installing
            .store(false, Ordering::Release);
        if self.monitoring {
            self.monitoring = false;
            self.session.input_monitor_start()?;
        }
        Ok(())
    }
}
impl Drop for UpdateShutdown {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
