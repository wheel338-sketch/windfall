//! Checked session-only playback regions. Musical timeline edits use Document.
use super::{Session, State};
use std::sync::atomic::Ordering;
use windfall_ipc::{TimelinePlaybackState, TransportPatch, TransportState};
use windfall_project::TickRange;

pub(super) const MAX_TIMELINE_REQUEST: u64 = (1_u64 << 53) - 1;

impl Session {
    pub(super) fn clear_timeline_play_owner(&self, _state: &State) {
        self.inner.timeline_play_request.store(0, Ordering::Relaxed);
    }

    fn check_timeline_guard(
        &self,
        state: &State,
        guard: TimelinePlaybackState,
        region: Option<TickRange>,
    ) -> Result<(), String> {
        if state.generation != guard.generation
            || state.document.revision() != guard.revision
            || guard.request == 0
            || guard.request > MAX_TIMELINE_REQUEST
            || self.inner.timeline_request.load(Ordering::Relaxed) != guard.request
            || guard.region.is_none()
            || region != guard.region
        {
            return Err(
                "The project or timeline selection changed before playback could continue."
                    .to_owned(),
            );
        }
        Ok(())
    }
}

impl Session {
    pub fn timeline_state(&self) -> TimelinePlaybackState {
        let state = self.state();
        TimelinePlaybackState {
            request: self.inner.timeline_request.load(Ordering::Relaxed),
            generation: state.generation,
            revision: state.document.revision(),
            region: self.controller().timeline_region(),
            navigation_overflows: self.controller().navigation_overflows(),
        }
    }

    pub fn timeline_region(
        &self,
        region: Option<TickRange>,
        generation: u64,
        revision: u64,
    ) -> Result<TimelinePlaybackState, String> {
        self.timeline_region_request(region, generation, revision, None, None)
    }

    pub fn timeline_region_request(
        &self,
        region: Option<TickRange>,
        generation: u64,
        revision: u64,
        request: Option<u64>,
        cancel: Option<TimelinePlaybackState>,
    ) -> Result<TimelinePlaybackState, String> {
        if let Some(range) = region {
            range.check()?;
        }
        let _recording = self.recording_idle()?;
        let state = self.state();
        if state.generation != generation || state.document.revision() != revision {
            return Err("The project changed before the timeline region could be applied. Select the region again.".to_owned());
        }
        let previous = self.inner.timeline_request.load(Ordering::Relaxed);
        let request = request.unwrap_or_else(|| previous.saturating_add(1));
        if request <= previous || request > MAX_TIMELINE_REQUEST {
            return Err(
                "The timeline request is stale or exceeds the exact request limit.".to_owned(),
            );
        }
        let mut stopped = false;
        if let Some(cancel) = cancel {
            if cancel.request == 0 || cancel.request > MAX_TIMELINE_REQUEST {
                return Err(
                    "The cancelled timeline request exceeds the exact request limit.".to_owned(),
                );
            }
            // A delayed selected Play can commit while its cancellation query
            // is awaiting IO. Reconcile only that exact still-owned action,
            // atomically with the newer region and watermark. Ordinary
            // transport explicitly relinquishes this ownership under State.
            if self
                .check_timeline_guard(&state, cancel, self.controller().timeline_region())
                .is_ok()
                && self.inner.timeline_play_request.load(Ordering::Relaxed) == cancel.request
            {
                if self.controller().transport().playing {
                    self.controller().stop();
                    stopped = true;
                }
                self.clear_timeline_play_owner(&state);
            }
        }
        self.controller().set_timeline_region(region)?;
        self.inner
            .timeline_request
            .store(request, Ordering::Relaxed);
        if stopped {
            self.sync_transport();
        }
        Ok(TimelinePlaybackState {
            request,
            generation,
            revision,
            region,
            navigation_overflows: self.controller().navigation_overflows(),
        })
    }

    pub fn timeline_transport_set(
        &self,
        patch: TransportPatch,
        guard: TimelinePlaybackState,
    ) -> Result<TransportState, String> {
        if patch.count_in_bars.is_some_and(|bars| bars > 8) {
            return Err("Count-in must be 0 to 8 bars.".into());
        }
        if patch.metronome.is_some_and(|settings| {
            !settings.gain.is_finite() || !(0.0..=1.0).contains(&settings.gain)
        }) {
            return Err("Metronome gain must be between 0 and 1.".into());
        }
        let _recording = self.recording_idle()?;
        let state = self.state();
        self.check_timeline_guard(&state, guard, self.controller().timeline_region())?;
        if let Some(pattern) = patch.pattern
            && state.document.project().pattern(pattern).is_none()
        {
            return Err(format!("pattern {} does not exist", pattern.0));
        }
        self.controller().set_transport(patch);
        Ok(self.sync_transport())
    }

    pub fn timeline_transport_seek(
        &self,
        tick: f64,
        guard: TimelinePlaybackState,
    ) -> Result<(), String> {
        let _recording = self.recording_idle()?;
        let state = self.state();
        self.check_timeline_guard(&state, guard, self.controller().timeline_region())?;
        self.controller().seek(tick);
        Ok(())
    }

    pub fn timeline_transport_play(
        &self,
        guard: TimelinePlaybackState,
    ) -> Result<TransportState, String> {
        let _recording = self.recording_idle()?;
        let state = self.state();
        self.check_timeline_guard(&state, guard, self.controller().timeline_region())?;
        if let Some(reason) = super::transport::nothing_to_play(state.document.project()) {
            return Err(reason.to_owned());
        }
        self.controller().play();
        self.inner
            .timeline_play_request
            .store(guard.request, Ordering::Relaxed);
        Ok(self.sync_transport())
    }
}
