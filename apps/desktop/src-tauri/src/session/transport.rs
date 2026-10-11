//! The transport: play, stop, and where the playhead is.

use windfall_ipc::{PlayMode, TransportPatch, TransportState};
use windfall_project::{ClipContent, Project};

use super::Session;
use crate::events::Event;
use crate::sync::lock;

/// What playing a song says when the playlist has no clips.
pub const EMPTY_PLAYLIST: &str =
    "The playlist is empty. Add a clip to the playlist, or switch to pattern mode.";

/// What playing a song says when the playlist has clips and all are muted.
pub const MUTED_PLAYLIST: &str =
    "Every clip on the playlist is muted. Unmute a clip, or switch to pattern mode.";

/// What playing a song says when the only clips that are not muted are
/// automation clips.
pub const SILENT_PLAYLIST: &str = "Only automation clips would play, and they make no sound by themselves. Add or unmute a pattern or audio clip, or switch to pattern mode.";

/// Why playing the song would make no sound, if it would not: there is no
/// clip, every clip is muted or sits on a muted track, or the clips that
/// are left are all automation, which moves controls and plays nothing.
pub(super) fn nothing_to_play(project: &Project) -> Option<&'static str> {
    let playlist = &project.playlist;
    if playlist.clips.is_empty() {
        return Some(EMPTY_PLAYLIST);
    }
    let mut playing = playlist.clips.iter().filter(|clip| {
        let track_muted = playlist
            .tracks
            .iter()
            .any(|track| track.id == clip.track && track.muted);
        !clip.muted && !track_muted
    });
    let Some(first) = playing.next() else {
        return Some(MUTED_PLAYLIST);
    };
    let sounds = |content: &ClipContent| match content {
        ClipContent::Pattern { .. } | ClipContent::Audio { .. } => true,
        ClipContent::Automation { .. } => false,
    };
    let heard = sounds(&first.content) || playing.any(|clip| sounds(&clip.content));
    (!heard).then_some(SILENT_PLAYLIST)
}

impl Session {
    pub fn mixer_waveform_tracks(
        &self,
        tracks: Vec<windfall_project::TrackId>,
        generation: u64,
        revision: u64,
    ) -> Result<(), String> {
        let state = self.state();
        if state.generation != generation || state.document.revision() != revision {
            return Err("The project changed before waveform selection could update".into());
        }
        if tracks.len() > 128 {
            return Err("Waveform meters support at most 128 visible tracks".into());
        }
        let mut ids = std::collections::HashSet::new();
        if tracks
            .iter()
            .any(|id| !ids.insert(*id) || state.document.project().mixer.track(*id).is_none())
        {
            return Err("Waveform selection contains missing or repeated tracks".into());
        }
        self.controller().set_waveform_tracks(&tracks);
        Ok(())
    }

    pub fn current_mixer_target(
        &self,
        track: Option<windfall_project::TrackId>,
        generation: u64,
        revision: u64,
    ) -> Result<(), String> {
        let state = self.state();
        if state.generation != generation || state.document.revision() != revision {
            return Err("Project changed before the Current source could follow selection".into());
        }
        if let Some(id) = track {
            let source = state
                .document
                .project()
                .mixer
                .track(id)
                .ok_or("Current source no longer exists")?;
            if source.current {
                return Err("Current cannot follow itself".into());
            }
        }
        self.controller().set_current_track(track);
        Ok(())
    }
    pub fn transport_state(&self) -> TransportState {
        self.controller().transport()
    }

    /// Starts playback. With no audio device open nothing plays, and the
    /// state returned says so.
    ///
    /// In song mode with nothing on the playlist to hear, playback is not
    /// started and the error says what to do: the engine would stop again
    /// at once and leave the user with a dead Play button.
    pub fn transport_play(&self) -> Result<TransportState, String> {
        // Held across the check and the start, so the last clip cannot be
        // removed in between.
        let state = self.state();
        let transport = self.controller().transport();
        if transport.mode == PlayMode::Song
            && !transport.playing
            && let Some(reason) = nothing_to_play(state.document.project())
        {
            return Err(reason.to_owned());
        }
        self.clear_timeline_play_owner(&state);
        let armed = self.controller_state().note_record.armed;
        let bars = transport.count_in_bars.unwrap_or(0);
        if armed && bars > 0 {
            self.controller().play_count_in(bars);
        } else {
            self.controller().play();
        }
        Ok(self.sync_transport())
    }

    /// Stops playback. The playhead returns to where playback started.
    pub fn transport_stop(&self) -> TransportState {
        let state = self.state();
        self.clear_timeline_play_owner(&state);
        self.controller().stop();
        self.sync_transport()
    }

    /// Stops playback if it is running, and starts it if it is not, with
    /// the refusal of [`transport_play`](Self::transport_play).
    pub fn transport_toggle(&self) -> Result<TransportState, String> {
        if self.controller().transport().playing {
            Ok(self.transport_stop())
        } else {
            self.transport_play()
        }
    }

    pub fn transport_seek(&self, tick: f64) {
        let Ok(_recording) = self.recording_idle() else {
            return;
        };
        let state = self.state();
        self.clear_timeline_play_owner(&state);
        self.controller().seek(tick);
    }

    /// Changes the play mode, the pattern that plays in pattern mode, or
    /// song looping. Fails for a pattern the project does not have.
    pub fn transport_set(&self, patch: TransportPatch) -> Result<TransportState, String> {
        if patch.count_in_bars.is_some_and(|bars| bars > 8) {
            return Err("Count-in must be 0 to 8 bars.".into());
        }
        if patch.metronome.is_some_and(|settings| {
            !settings.gain.is_finite() || !(0.0..=1.0).contains(&settings.gain)
        }) {
            return Err("Metronome gain must be between 0 and 1.".into());
        }
        let _recording = self.recording_idle()?;
        // Held across the change, so the pattern cannot be deleted between
        // the check and the engine hearing of it.
        let state = self.state();
        if let Some(pattern) = patch.pattern
            && state.document.project().pattern(pattern).is_none()
        {
            return Err(format!("pattern {} does not exist", pattern.0));
        }
        self.clear_timeline_play_owner(&state);
        self.controller().set_transport(patch);
        Ok(self.sync_transport())
    }

    /// Tells the UI the transport state if it is not what it was last told.
    /// Everything that can change the transport ends by calling this, and
    /// so does the realtime loop, which is how playback that ends by itself
    /// gets announced.
    pub(super) fn sync_transport(&self) -> TransportState {
        let mut told = lock(&self.inner.transport);
        let now = self.controller().transport();
        if *told != now {
            *told = now;
            self.emit(Event::TransportState(now));
        }
        now
    }

    /// Tells the UI the transport state whether or not it changed, as after
    /// loading a project, when the UI starts over.
    pub(super) fn announce_transport(&self) -> TransportState {
        // This authority is called after document replacement. Never carry a
        // region with reused IDs/ticks into the replacement project.
        self.controller()
            .set_timeline_region(None)
            .expect("clearing a region is valid");
        let mut told = lock(&self.inner.transport);
        let now = self.controller().transport();
        *told = now;
        self.emit(Event::TransportState(now));
        now
    }
}
