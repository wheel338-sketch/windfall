//! The browser panel, and bringing audio files into the project.

use std::path::{Path, PathBuf};

use windfall_core::{AudioBuffer, PPQ};
use windfall_ipc::{BrowserEntry, BrowserRoot, SampleInfo};
use windfall_ipc::{LibraryFileToken, LibraryMetadata, LibraryResults, LibrarySearch};
use windfall_project::file::sample_path_for;
use windfall_project::{
    ChannelId, ClipContent, ClipInit, Command, DispatchResult, MAX_MIXER_SIGNAL_TRACKS,
    MAX_SONG_TICKS, PlaylistTrackId, Project, SampleId, SamplePath, TrackId,
};

use super::samples::locate;
use super::{Session, State};
use crate::browser;
use crate::paths;

/// What a call about a sample says when the project it was made for has
/// been replaced by another.
pub const PROJECT_REPLACED: &str = "Another project was opened in the meantime.";

/// Where on the playlist an audio clip is to go.
#[derive(Debug, Clone, Copy)]
pub struct ClipPlace {
    /// The playlist track. `None` adds a track for the clip, at the end.
    pub track: Option<PlaylistTrackId>,
    /// The tick the clip starts on.
    pub start: u32,
    /// The mixer track the clip plays into. `None` leaves it to the
    /// project: the clip joins the audio clip of the same sample that was
    /// made last, on whatever mixer track that one plays into now, so that
    /// a file dropped three times is on one track and not on three. Only
    /// when no clip plays the sample is a mixer track added, named after
    /// the audio and routed to the master; and when the mixer is full the
    /// clip plays into the master instead, as a new channel does.
    pub mixer_track: Option<TrackId>,
}

/// An audio file that is decoded and about to be put to use in the project.
pub(super) struct Import {
    browser: Option<LibraryFileToken>,
    pub(super) file: PathBuf,
    pub(super) buffer: AudioBuffer,
    /// The document that was open when decoding started.
    generation: u64,
}

impl Session {
    pub fn library_search(&self, search: &LibrarySearch) -> Result<LibraryResults, String> {
        self.inner.library.search(search)
    }
    pub fn library_refresh(&self) {
        self.inner.library.refresh();
    }
    pub fn library_cancel(&self, generation: u32) {
        self.inner.library.cancel(generation);
    }
    pub fn library_file(&self, path: &str) -> Result<LibraryFileToken, String> {
        self.inner.library.file_token(path)
    }
    pub fn library_metadata(&self, path: &str) -> Result<LibraryMetadata, String> {
        self.inner.library.metadata(path)
    }
    pub fn library_set_metadata(
        &self,
        path: &str,
        metadata: LibraryMetadata,
    ) -> Result<LibraryMetadata, String> {
        self.inner.library.set_metadata(path, metadata)
    }
    /// The browser's top-level folders: the factory content first, then the
    /// folders the user added, in the order they were added.
    pub fn browser_roots(&self) -> Vec<BrowserRoot> {
        let settings = self.store();
        self.roots(&settings.settings().browser_roots)
    }

    /// Adds a folder to the browser and remembers it.
    pub fn browser_add_root(&self, path: &str) -> Result<Vec<BrowserRoot>, String> {
        if path.trim().is_empty() {
            return Err("Choose a folder to add.".to_owned());
        }
        let folder = paths::absolute(path)?;
        let shown = paths::display(&folder);
        if shown.len() > crate::library::MAX_PATH_BYTES {
            return Err("This folder path is too long for the library.".into());
        }
        if !folder.is_dir() {
            return Err(format!("\"{shown}\" is not a folder."));
        }
        let mut settings = self.store();
        if settings.settings().browser_roots.len() >= crate::library::MAX_ROOTS - 1 {
            return Err("The browser supports at most 128 folders, including Factory. Remove a folder first.".into());
        }
        let known = paths::same(&folder, &self.inner.factory_dir)
            || settings
                .settings()
                .browser_roots
                .iter()
                .any(|root| paths::same(Path::new(root), &folder));
        if known {
            return Err(format!("\"{shown}\" is already in the browser."));
        }
        settings.update(|stored| stored.browser_roots.push(shown));
        let roots = self.roots(&settings.settings().browser_roots);
        self.inner.library.set_roots(roots.clone());
        Ok(roots)
    }

    /// Takes a folder the user added out of the browser. Nothing on disk is
    /// touched.
    pub fn browser_remove_root(&self, path: &str) -> Result<Vec<BrowserRoot>, String> {
        let folder = paths::clean(Path::new(path));
        if paths::same(&folder, &self.inner.factory_dir) {
            return Err("The factory library cannot be removed.".to_owned());
        }
        let mut settings = self.store();
        let is_it = |root: &String| paths::same(Path::new(root), &folder);
        if !settings.settings().browser_roots.iter().any(is_it) {
            return Err(format!("\"{path}\" is not in the browser."));
        }
        settings.update(|stored| stored.browser_roots.retain(|root| !is_it(root)));
        let roots = self.roots(&settings.settings().browser_roots);
        self.inner.library.set_roots(roots.clone());
        Ok(roots)
    }

    /// Lists a folder for the browser. Reads the disk.
    pub fn browser_list(&self, path: &str) -> Result<Vec<BrowserEntry>, String> {
        browser::list_folder(&paths::absolute(path)?)
    }

    /// Facts about an audio file and its waveform overview. Slow the first
    /// time a file is asked for.
    pub fn sample_info(&self, path: &str) -> Result<SampleInfo, String> {
        self.inner.cache.info(&paths::absolute(path)?)
    }

    pub fn browser_sample_info(
        &self,
        path: &str,
        token: &LibraryFileToken,
    ) -> Result<SampleInfo, String> {
        let token = self.inner.library.pin_file(token, path)?;
        let info = self.sample_info(path)?;
        self.inner.library.check_file(&token, path)?;
        Ok(info)
    }

    /// The same as [`sample_info`](Self::sample_info) for a sample of the
    /// project, whose stored path the UI cannot resolve by itself.
    ///
    /// Refused if another project was opened while the file was read: the
    /// id then means another sample, or none, and the answer would be shown
    /// as that sample's.
    pub fn sample_info_by_id(&self, sample: SampleId) -> Result<SampleInfo, String> {
        let (file, generation) = {
            let state = self.state();
            let asset = state
                .document
                .project()
                .sample(sample)
                .ok_or_else(|| format!("sample {} does not exist", sample.0))?;
            let file = locate(asset, state.project_dir(), &self.inner.factory_dir)?;
            (file, state.generation)
        };
        let info = self.inner.cache.info(&file)?;
        #[cfg(test)]
        self.pause("sample-info:read");
        if self.state().generation != generation {
            return Err(PROJECT_REPLACED.to_owned());
        }
        Ok(info)
    }

    /// Adds a channel that plays the audio file at `path`, as one undo
    /// step. The channel is named after the file and goes at `index` in the
    /// rack, or at the end. Creates the sample, the channel and the
    /// channel's mixer track, in that order; a file the project already
    /// uses is not added again, and its sample is the one reported.
    ///
    /// Slow the first time a file is asked for. A file that cannot be
    /// decoded is refused, and the project is not touched. Neither is a
    /// project that was opened while the file was being decoded: the
    /// channel was asked for in the one before it.
    pub fn add_channel_from_file(
        &self,
        path: &str,
        index: Option<u32>,
    ) -> Result<DispatchResult, String> {
        let import = self.decode_for_project(path)?;
        self.add_channel_import(import, index)
    }

    pub fn browser_add_channel(
        &self,
        path: &str,
        index: Option<u32>,
        token: LibraryFileToken,
    ) -> Result<DispatchResult, String> {
        let import = self.decode_for_browser(path, token)?;
        self.add_channel_import(import, index)
    }

    fn add_channel_import(
        &self,
        import: Import,
        index: Option<u32>,
    ) -> Result<DispatchResult, String> {
        let name = paths::stem(&import.file);
        self.dispatch_with_sample(import, "Add channel", |sample| Command::AddChannel {
            name: Some(name.clone()),
            sample: Some(sample),
            instrument: None,
            index,
            mixer_track: None,
        })
    }

    /// Makes a channel play the audio file at `path`, as one undo step.
    /// Creates the sample, or reports the one the project already has for
    /// that file.
    ///
    /// Refused if another project was opened while the file was being
    /// decoded, in which the channel's id may name some other channel.
    pub fn set_channel_sample_from_file(
        &self,
        channel: ChannelId,
        path: &str,
    ) -> Result<DispatchResult, String> {
        let import = self.decode_for_project(path)?;
        self.replace_sample_import(channel, import)
    }

    pub fn browser_replace_sample(
        &self,
        channel: ChannelId,
        path: &str,
        token: LibraryFileToken,
    ) -> Result<DispatchResult, String> {
        let import = self.decode_for_browser(path, token)?;
        self.replace_sample_import(channel, import)
    }

    fn replace_sample_import(
        &self,
        channel: ChannelId,
        import: Import,
    ) -> Result<DispatchResult, String> {
        self.dispatch_with_sample(import, "Change channel sample", |sample| {
            Command::SetChannelSample {
                id: channel,
                sample: Some(sample),
            }
        })
    }

    /// Puts the audio file at `path` on the playlist as an audio clip, as
    /// one undo step. The clip is as long as the file lasts at the tempo
    /// the project has now, at unity gain, with no fades.
    ///
    /// Creates, in this order: the sample, or the one the project already
    /// has for that file, which is reported in its place; the playlist
    /// track, if `place` named none; the mixer track, if one was made; and
    /// the clip, which is always last. A mixer track is made only when
    /// `place` named none, no audio clip plays the sample yet and the
    /// mixer has room, so its id is often left out: see
    /// [`ClipPlace::mixer_track`].
    ///
    /// Slow the first time a file is asked for. A file that cannot be
    /// decoded is refused, and the project is not touched. Neither is a
    /// project that was opened while the file was being decoded.
    pub fn add_audio_clip_from_file(
        &self,
        path: &str,
        place: ClipPlace,
    ) -> Result<DispatchResult, String> {
        let import = self.decode_for_project(path)?;
        self.attach_audio_clip_import(import, place, None)
    }
    /// The same for work that began in the document numbered `generation`:
    /// refused if another document has taken its place, however long ago.
    pub(super) fn add_audio_clip_from_file_in(
        &self,
        path: &str,
        place: ClipPlace,
        generation: u64,
    ) -> Result<DispatchResult, String> {
        let import = self.decode_for_generation(path, generation)?;
        self.attach_audio_clip_import(import, place, None)
    }
    pub(super) fn attach_audio_clip_from_file(
        &self,
        path: &str,
        place: ClipPlace,
        installed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<DispatchResult, String> {
        let import = self.decode_for_project(path)?;
        // recording_stop owns the finishing flag; its mutex is released during preparation.
        self.attach_audio_clip_import(import, place, Some(installed))
    }

    pub fn browser_add_clip(
        &self,
        path: &str,
        place: ClipPlace,
        token: LibraryFileToken,
    ) -> Result<DispatchResult, String> {
        let import = self.decode_for_browser(path, token)?;
        self.attach_audio_clip_import(import, place, None)
    }
    fn attach_audio_clip_import(
        &self,
        import: Import,
        place: ClipPlace,
        installed: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<DispatchResult, String> {
        let name = paths::stem(&import.file);
        self.dispatch_import(import, installed, |state, path, sample, buffer| {
            let project = state.document.project();
            let next_id = if project.sample(sample).is_some() {
                project.next_id
            } else {
                project.next_id.saturating_add(1)
            };
            let mut commands = vec![Command::AddSample {
                name: name.clone(),
                path,
            }];
            commands.extend(audio_clip_commands(
                project, sample, buffer, &name, place, next_id,
            ));
            Command::Batch {
                label: Some("Add audio clip".to_owned()),
                commands,
            }
        })
    }

    /// Puts a sample the project already has on the playlist as an audio
    /// clip, as one undo step, the way
    /// [`add_audio_clip_from_file`](Self::add_audio_clip_from_file) does
    /// for a file.
    ///
    /// Creates, in this order: the playlist track, if `place` named none;
    /// the mixer track, if one was made, which is under the same
    /// conditions; and the clip, which is always last.
    ///
    /// Fails for a sample whose file is missing or still being read: how
    /// long the clip is to be is not known without its audio.
    pub fn add_audio_clip_from_sample(
        &self,
        sample: SampleId,
        place: ClipPlace,
    ) -> Result<DispatchResult, String> {
        let _recording = self.recording_idle()?;
        let state = self.state();
        let project = state.document.project();
        let asset = project
            .sample(sample)
            .ok_or_else(|| format!("sample {} does not exist", sample.0))?;
        let buffer = state.pool.get(sample).ok_or_else(|| {
            format!(
                "The audio of \"{}\" is not loaded, so a clip of it cannot be made. Check that its file is there, then reload the samples.",
                asset.name
            )
        })?;
        let commands =
            audio_clip_commands(project, sample, buffer, &asset.name, place, project.next_id);
        let batch = Command::Batch {
            label: Some("Add audio clip".to_owned()),
            commands,
        };
        let ticket = self.sample_edit_ticket(&state, batch, None, Vec::new())?;
        drop(state);
        drop(_recording);
        self.finish_sample_edit(ticket)
    }

    fn roots(&self, user: &[String]) -> Vec<BrowserRoot> {
        std::iter::once(browser::factory_root(&self.inner.factory_dir))
            .chain(user.iter().map(|root| browser::user_root(Path::new(root))))
            .collect()
    }

    /// Decodes a file before the project is changed to use it, and notes
    /// which project that is.
    fn decode_for_project(&self, path: &str) -> Result<Import, String> {
        let generation = self.state().generation;
        self.decode_for_generation(path, generation)
    }

    fn decode_for_generation(&self, path: &str, generation: u64) -> Result<Import, String> {
        let file = paths::absolute(path)?;
        self.pin_derived(&file);
        let buffer = self.inner.cache.decode(&file)?;
        #[cfg(test)]
        self.pause("import:decoded");
        Ok(Import {
            browser: None,
            file,
            buffer,
            generation,
        })
    }

    fn decode_for_browser(&self, path: &str, token: LibraryFileToken) -> Result<Import, String> {
        // Membership/identity checks can block on a drive; pin the document first.
        let generation = self.state().generation;
        let token = self.inner.library.pin_file(&token, path)?;
        let mut import = self.decode_for_generation(path, generation)?;
        self.inner.library.check_file(&token, path)?;
        import.browser = Some(token);
        Ok(import)
    }

    /// Dispatches one undo step that adds the sample of an import and then
    /// runs the command `then` builds from the sample's id. Refuses if the
    /// project is no longer the one the import was decoded for.
    fn dispatch_with_sample(
        &self,
        import: Import,
        label: &str,
        then: impl FnOnce(SampleId) -> Command,
    ) -> Result<DispatchResult, String> {
        let name = paths::stem(&import.file);
        self.dispatch_import(import, None, |_, path, sample, _| Command::Batch {
            label: Some(label.to_owned()),
            commands: vec![Command::AddSample { name, path }, then(sample)],
        })
    }

    /// File routes share one candidate-source preparation/commit contract.
    /// No source, history or audible plan changes until preparation succeeds.
    fn dispatch_import(
        &self,
        mut import: Import,
        installed: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
        build: impl FnOnce(&State, SamplePath, SampleId, &AudioBuffer) -> Command,
    ) -> Result<DispatchResult, String> {
        let original_file = import.file.clone();
        let mut owned_preset = self.own_preset(&mut import)?;
        let (ticket, sample, directory) = {
            let _recording = self.import_recording_guard(installed.is_some())?;
            let _library = import
                .browser
                .as_ref()
                .map(|token| self.inner.library.guard(token))
                .transpose()?;
            let state = self.state();
            self.check_import_generation(&state, &import)?;
            let project = state.document.project();
            // Saving can make an External path eligible for Project storage.
            // Keep the original asset/path when it resolves to this same file;
            // otherwise AddSample would create a duplicate after save/reopen.
            // locate/paths::same are lexical and do not touch the filesystem.
            let held = project.samples.iter().find(|held| {
                locate(held, state.project_dir(), &self.inner.factory_dir)
                    .is_ok_and(|file| paths::same(&file, &import.file))
            });
            let (path, sample) = match held {
                Some(held) => (held.path.clone(), held.id),
                None => (
                    sample_path_for(&import.file, state.project_dir(), &self.inner.factory_dir),
                    SampleId(project.next_id),
                ),
            };
            self.check_loaded_import(&state, sample, &import.buffer, &import.file)?;
            // Reuse the exact held source, even after a separate same-version
            // decode. Clip geometry and prepared banks use what will play.
            let source = state.pool.get(sample).unwrap_or(&import.buffer);
            let command = build(&state, path, sample, source);
            let mut ticket =
                self.sample_edit_ticket(&state, command, None, vec![(sample, source.clone())])?;
            if let Some(installed) = &installed {
                ticket = ticket.on_install(installed.clone());
            }
            (ticket, sample, state.project_dir().map(Path::to_path_buf))
        };
        let mut prepared = ticket.prepare()?;
        let mut retirement = None;
        #[cfg(test)]
        self.pause("import:prepared");
        // Preparation may be slow. Repeat disk/root checks off State/audio
        // locks, then recheck the in-memory guards at the atomic commit.
        if let Some(token) = &import.browser {
            self.inner
                .library
                .check_file(token, &paths::display(&original_file))?;
        } else {
            let current = self.inner.cache.decode(&import.file)?;
            if !self.inner.cache.same_file_version(&import.buffer, &current) {
                return Err(
                    "The source file changed while audio was being prepared. Try the import again."
                        .into(),
                );
            }
        }
        if let Some(owned) = &owned_preset {
            let current = self.inner.cache.decode(&owned.original)?;
            if !self
                .inner
                .cache
                .same_file_version(&owned.original_audio, &current)
                || super::preset_assets::read_original(&owned.original)? != owned.bytes
            {
                return Err("The preset source changed during preparation. Try again.".into());
            }
        }
        let result = {
            let _recording = self.import_recording_guard(installed.is_some())?;
            let _library = import
                .browser
                .as_ref()
                .map(|token| self.inner.library.guard(token))
                .transpose()?;
            let mut state = self.state();
            self.check_import_generation(&state, &import)?;
            if state.project_dir() != directory.as_deref() {
                return Err(
                "The project folder changed while audio was being prepared. Try the import again."
                    .into(),
            );
            }
            self.check_loaded_import(&state, sample, &import.buffer, &import.file)?;
            prepared.commit(&mut state, &mut retirement)
        };
        if let Some(retirement) = &mut retirement {
            self.retire_project(retirement);
        }
        if result.is_ok()
            && let Some(owned) = &mut owned_preset
        {
            owned.installed = true;
            self.own_derived(&owned.path);
        }
        result
    }

    fn import_recording_guard(
        &self,
        finishing: bool,
    ) -> Result<std::sync::MutexGuard<'_, Option<super::recording::Take>>, String> {
        if !finishing {
            return self.recording_idle();
        }
        let guard = crate::sync::lock(&self.inner.recording);
        if guard.is_some()
            || self
                .inner
                .recording_finish_cancelled
                .load(std::sync::atomic::Ordering::Acquire)
            || !self
                .inner
                .recording_finishing
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err("Recording completion ownership changed.".into());
        }
        Ok(guard)
    }

    fn check_import_generation(&self, state: &State, import: &Import) -> Result<(), String> {
        if state.generation != import.generation {
            return Err(format!(
                "\"{}\" was not added, because another project was opened while it was loading.",
                paths::name(&import.file)
            ));
        }
        Ok(())
    }

    /// File path deduplication may reuse only a settled source version.
    /// Runs before dispatch, after recording -> library -> State guards; this
    /// cache comparison reads in-memory provenance and never touches the disk.
    fn check_loaded_import(
        &self,
        state: &State,
        sample: SampleId,
        buffer: &AudioBuffer,
        file: &Path,
    ) -> Result<(), String> {
        // A reload/redo can already hold decoded audio off-lock even when the
        // pool is empty. Do not install a newer import which that outstanding
        // load could overwrite. Check and dispatch share the same State lock.
        if state.loading.contains(&sample) {
            return Err(format!(
                "The project sample \"{}\" is still loading. Wait for loading to finish and try the import again.",
                paths::name(file)
            ));
        }
        if let Some(held) = state.pool.get(sample)
            && !self.inner.cache.same_file_version(held, buffer)
        {
            return Err(format!(
                "The project is already using an older or unverified version of \"{}\". Import the changed file under a new path, or reopen the project to reload its sources.",
                paths::name(file)
            ));
        }
        Ok(())
    }
}

/// The commands that put `sample`, whose audio is `buffer`, on the playlist
/// as an audio clip: a playlist track where `place` names none, a mixer
/// track where it names none and no clip of the sample has one to share,
/// then the clip. `next_id` is the id the project hands out to the first of
/// them.
pub(super) fn audio_clip_commands(
    project: &Project,
    sample: SampleId,
    buffer: &AudioBuffer,
    name: &str,
    place: ClipPlace,
    mut next_id: u32,
) -> Vec<Command> {
    // A batch cannot pass an id from one command to the next, so the ids
    // the new tracks will get are worked out here, in the order the
    // commands allocate them.
    let mut allocate = || {
        let id = next_id;
        next_id = next_id.saturating_add(1);
        id
    };
    let mut commands = Vec::new();
    let track = place.track.unwrap_or_else(|| {
        commands.push(Command::AddPlaylistTrack {
            name: None,
            index: None,
        });
        PlaylistTrackId(allocate())
    });
    // The clip of this sample that was made last has the highest id.
    let clips = project.playlist.clips.iter();
    let shared = clips
        .filter_map(|clip| match clip.content {
            ClipContent::Audio {
                sample: played,
                mixer_track,
                ..
            } if played == sample => Some((clip.id, mixer_track)),
            _ => None,
        })
        .max_by_key(|(id, _)| *id)
        .map(|(_, mixer_track)| mixer_track);
    let mixer_track = match place.mixer_track.or(shared) {
        Some(mixer_track) => mixer_track,
        None if project
            .mixer
            .tracks
            .iter()
            .filter(|track| !track.current)
            .count()
            < MAX_MIXER_SIGNAL_TRACKS =>
        {
            commands.push(Command::AddMixerTrack {
                name: Some(name.to_owned()),
            });
            TrackId(allocate())
        }
        // A full mixer must not stop the user adding audio.
        None => TrackId::MASTER,
    };

    // As long as the audio lasts at the tempo the project has now, cut
    // where the longest song ends.
    let ticks = buffer.duration_secs() * project.settings.tempo_bpm * f64::from(PPQ) / 60.0;
    let room = MAX_SONG_TICKS.saturating_sub(place.start).max(1);
    let length = (ticks.ceil().max(1.0) as u32).min(room);
    commands.push(Command::AddClips {
        clips: vec![ClipInit {
            track,
            start: place.start,
            length: Some(length),
            offset: None,
            muted: None,
            content: ClipContent::Audio {
                sample,
                mixer_track,
                output: Default::default(),
                normalize: false,
                gain: 1.0,
                pan: 0.0,
                fade_in: 0,
                fade_out: 0,
                reverse: false,
                pitch: 0.0,

                stretch: Default::default(),
            },
        }],
    });
    commands
}
