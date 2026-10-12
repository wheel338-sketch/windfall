//! Exporting the project to audio files: the mix as one file, or the mixer
//! tracks apart as stems.

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use windfall_codec::{
    AudioFormat, AudioTags, DEFAULT_FLAC_LEVEL, DEFAULT_VORBIS_QUALITY, Encoder, EncoderSettings,
    FlacBitDepth, MAX_FLAC_LEVEL, Mp3Channels, Mp3Rate, Mp3Settings, WavSampleFormat,
};
use windfall_core::samples_per_tick;
use windfall_engine::{
    PlaylistStemOptions, RenderOptions, SamplePool, StemOptions, Streamed, render_playlist_stems,
    render_stems, render_streaming_checked, stems,
};
use windfall_ipc::{BitDepth, ExportFormat, ExportOptions, ExportProgress, ExportedFile, PlayMode};
use windfall_project::{AutomationRange, AutomationTarget, PatternId, Project, TrackId};

use super::Session;
#[path = "export_markers.rs"]
mod markers;
use crate::events::Event;
use crate::paths;

mod mastering;

/// Sample rates an export may ask for.
const SAMPLE_RATES: std::ops::RangeInclusive<u32> = 8_000..=384_000;

/// Channels of an export: the engine renders stereo.
const CHANNELS: u16 = 2;

/// Longest export of a format that has no limit of its own, in seconds: a
/// day. Nothing is held in memory, so this only keeps a slip of the hand,
/// a hundred thousand loops of a pattern, from filling the disk.
const MAX_SECONDS: f64 = 24.0 * 60.0 * 60.0;

/// The most audio a WAV file holds, in bytes. Past 4 GB its header cannot
/// say how long it is; this leaves room for the header itself.
const MAX_WAV_BYTES: f64 = 4_294_967_000.0;

/// Shortest time between two progress events.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);

static NEXT_TRANSACTION: AtomicU64 = AtomicU64::new(0);

/// Holds finished files beside their destinations until every encoder succeeds.
struct FileTransaction {
    files: Vec<TransactionFile>,
    committed: bool,
}

struct TransactionFile {
    destination: PathBuf,
    stage: PathBuf,
    backup: PathBuf,
    backed_up: bool,
    placed: bool,
}

impl FileTransaction {
    fn new(targets: &[Target]) -> Self {
        let serial = NEXT_TRANSACTION.fetch_add(1, Ordering::Relaxed);
        let files = targets
            .iter()
            .enumerate()
            .map(|(index, target)| {
                let name = target
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                let prefix = format!(".{name}.{}-{serial}-{index}", std::process::id());
                TransactionFile {
                    destination: target.path.clone(),
                    stage: target.path.with_file_name(format!("{prefix}.stage")),
                    backup: target.path.with_file_name(format!("{prefix}.backup")),
                    backed_up: false,
                    placed: false,
                }
            })
            .collect();
        Self {
            files,
            committed: false,
        }
    }

    fn commit(&mut self) -> Result<(), String> {
        for file in &mut self.files {
            if file.destination.exists() {
                if !file.destination.is_file() {
                    return Err(format!(
                        "\"{}\": the destination is not a file",
                        paths::name(&file.destination)
                    ));
                }
                fs::rename(&file.destination, &file.backup).map_err(|error| error.to_string())?;
                file.backed_up = true;
            }
            fs::rename(&file.stage, &file.destination).map_err(|error| error.to_string())?;
            file.placed = true;
        }
        self.committed = true;
        for file in &self.files {
            if file.backed_up {
                let _ = fs::remove_file(&file.backup);
            }
        }
        Ok(())
    }
}

impl Drop for FileTransaction {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for file in self.files.iter().rev() {
            let _ = fs::remove_file(&file.stage);
            if file.placed {
                let _ = fs::remove_file(&file.destination);
            }
            if file.backed_up {
                let _ = fs::rename(&file.backup, &file.destination);
            }
        }
    }
}

/// One file an export writes.
struct Target {
    path: PathBuf,
    /// The mixer track the file is the stem of. `None` for the mix.
    track: Option<TrackId>,
    /// The playlist track the file is the stem of, for playlist stems.
    playlist_track: Option<windfall_project::PlaylistTrackId>,
}

/// Everything an export needs, copied out of the session so the project can
/// be edited while the export runs.
struct Job {
    request: Option<String>,
    /// The path the export was asked for, as a place on the disk. Stems
    /// are named after it.
    path: PathBuf,
    /// The files to write: one, or one for each stream of a stem render in
    /// the order the engine numbers them.
    targets: Vec<Target>,
    /// The folder the stems go in, when they get one of their own.
    folder: Option<PathBuf>,
    settings: EncoderSettings,
    stems: Option<StemOptions>,
    /// Set for an export of the playlist tracks; `stems` is then `None`.
    playlist_stems: Option<PlaylistStemOptions>,
    project: Project,
    pool: SamplePool,
    pattern: PatternId,
    options: ExportOptions,
}

/// How an export that did not fail ended.
enum Outcome {
    Written {
        files: Vec<ExportedFile>,
        /// Audio clips the render had no room to play.
        dropped_clips: u32,
        /// What mastering measured, when the export asked for it.
        mastering: Option<windfall_ipc::MasteringReview>,
    },
    Cancelled,
}

/// Sends progress events, no more often than [`PROGRESS_INTERVAL`].
struct Progress<'a> {
    session: &'a Session,
    path: String,
    request: Option<String>,
    sent_at: Option<Instant>,
    fraction: f32,
}

impl Progress<'_> {
    fn report(&mut self, fraction: f32) {
        self.fraction = fraction.clamp(self.fraction, 1.0);
        if self
            .sent_at
            .is_some_and(|at| at.elapsed() < PROGRESS_INTERVAL)
        {
            return;
        }
        self.sent_at = Some(Instant::now());
        self.send(false, None, None);
    }

    /// `outcome` is for the event that ends an export that did not fail.
    fn send(&self, done: bool, error: Option<String>, outcome: Option<Outcome>) {
        let (files, dropped_clips, cancelled, mastering) = match outcome {
            Some(Outcome::Written {
                files,
                dropped_clips,
                mastering,
            }) => (Some(files), dropped_clips, None, mastering),
            Some(Outcome::Cancelled) => (None, 0, Some(true), None),
            None => (None, 0, None, None),
        };
        let progress = ExportProgress {
            path: self.path.clone(),
            fraction: self.fraction,
            done,
            error,
            dropped_clips,
            files,
            cancelled,
            mastering,
        };
        self.session.emit(match &self.request {
            Some(request) => Event::ExportRequestProgress { request: request.clone(), progress },
            None => Event::ExportProgress(progress),
        });
    }
}

impl Session {
    /// Starts exporting the project to audio and returns at once. The
    /// export works from a copy of the project as it is now. Progress
    /// arrives as events, and the last one has `done` set and says how it
    /// ended: with the files that were written and how many audio clips
    /// are not in them because more overlapped than the engine plays at
    /// once, with the reason it failed, or cancelled.
    ///
    /// In pattern mode the pattern rendered is the one the transport has
    /// selected. Only one export runs at a time.
    ///
    /// A file starts where the song starts: whatever time the project's
    /// effects and instruments take to put their output out is left off
    /// its front. It ends `tail_secs` after the end of the song, or, with
    /// `auto_tail`, as soon as everything has rung out, if that is sooner.
    ///
    /// The audio is encoded as it is rendered, so a long song is never
    /// held in memory. Every file is written under another name beside
    /// where it belongs and takes its own name only when the whole export
    /// has gone through: an export that fails or is cancelled leaves no
    /// file, and leaves the files it would have replaced as they were.
    ///
    /// With `stems` the export writes one file for each stem, and the mix
    /// if it is asked for, all of the same length. For the path
    /// `Song.flac` they are `Song - Mix.flac`, `Song - Bass.flac` and so
    /// on, in the folder `Song` or beside it; `Song.flac` itself is not
    /// written. [`windfall_ipc::StemMode`] says what a stem holds.
    ///
    /// A file name with no extension gets the one of the format, and a
    /// name that asks for another format is refused; see [`export_path`].
    /// Progress events carry the path as it was given here, which is what
    /// the UI matches them by.
    pub fn export_audio(&self, options: ExportOptions) -> Result<(), String> {
        self.start_export(options, None)
    }

    /// [`Session::export_audio`], or with `playlist` the export of the
    /// playlist tracks that [`Session::export_playlist_tracks`] describes.
    pub(super) fn start_export(
        &self,
        options: ExportOptions,
        playlist: Option<windfall_ipc::ExportPlaylistStems>,
    ) -> Result<(), String> {
        self.start_export_correlated(options, playlist, None)
    }

    pub(crate) fn start_export_correlated(
        &self,
        options: ExportOptions,
        playlist: Option<windfall_ipc::ExportPlaylistStems>,
        request: Option<String>,
    ) -> Result<(), String> {
        if request.as_ref().is_some_and(|id| id.is_empty() || id.len() > 128 || !id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')) {
            return Err("Invalid export request identity.".into());
        }
        let _recording = self.recording_idle()?;
        super::playlist_export::check(&options, playlist.as_ref())?;
        if options.region_generation.is_some() != options.region_revision.is_some() {
            return Err(
                "A timeline export source requires both generation and revision.".to_owned(),
            );
        }
        if let Some(range) = options.region {
            if options.mode != PlayMode::Song {
                return Err("A timeline export region requires song mode.".to_owned());
            }
            range.check()?;
        }
        if options.path.trim().is_empty() {
            return Err("Choose where to save the file.".to_owned());
        }
        let format = audio_format(options.format);
        let path = export_path(paths::absolute(&options.path)?, format)?;
        if !SAMPLE_RATES.contains(&options.sample_rate) {
            return Err(format!(
                "{} Hz is not a sample rate Windfall can export.",
                options.sample_rate
            ));
        }
        let settings = encoder_settings(&options)?;
        if !options.tail_secs.is_finite() || options.tail_secs < 0.0 {
            return Err("The tail must be zero seconds or longer.".to_owned());
        }
        if options.mode == PlayMode::Pattern && options.pattern_loops < 1 {
            return Err("Render the pattern at least once.".to_owned());
        }
        if let Some(asked) = &options.mastering {
            mastering::check(asked, options.stems.is_some() || playlist.is_some())?;
        }

        // All three under one lock, so the pattern is one of this project's
        // and not of a project opened a moment later.
        let (project, asset_pins, pool, pattern, plugin_revision) = {
            let state = self.state();
            if let (Some(generation), Some(revision)) =
                (options.region_generation, options.region_revision)
                && (generation != state.generation || revision != state.document.revision())
            {
                return Err("The project changed before the selected region could be exported. Select the region again.".to_owned());
            }
            if state
                .pool
                .needs_sampler_preparation(state.document.project())
            {
                return Err("Sampler audio is not prepared for the current sources yet. Wait for preparation or Apply its settings again before exporting.".into());
            }
            let mut pool = state.pool.clone();
            // Retain the published banks even if a later edit prunes the live
            // cache before this export worker starts rendering.
            pool.share_sampler_budget(&state.pool);
            (
                state.document.project().clone(),
                state.document.effect_asset_pins(),
                pool,
                self.controller().transport().pattern,
                self.plugin_revision(),
            )
        };
        // Native state capture runs on its owner after releasing the document lock.
        let project = self.capture_plugins_idle(project, plugin_revision)?;
        if options.mode == PlayMode::Song
            && options.region.is_none()
            && project.playlist.clips.is_empty()
            && project.settings.stage_media.videos.is_empty()
        {
            return Err("The playlist is empty, so there is no song to export.".to_owned());
        }
        let frames = expected_frames(&project, pattern, &options);
        let longest = longest_frames(&settings, options.sample_rate);
        if frames > longest {
            let minutes = |frames: f64| frames / f64::from(options.sample_rate) / 60.0;
            return Err(format!(
                "The export would be {:.0} minutes long. The longest {} file Windfall can write at {} Hz is {:.0} minutes.",
                minutes(frames),
                format.name(),
                options.sample_rate,
                minutes(longest).floor(),
            ));
        }

        let stem_options = options.stems.as_ref().map(|stems| StemOptions {
            mode: stems.mode,
            tracks: stems.tracks.clone(),
            include_mix: stems.include_mix,
            numbered: stems.numbered,
        });
        let playlist_stems = playlist.as_ref().map(super::playlist_export::stem_options);
        let (targets, folder) = match (&playlist, &options.stems, &stem_options) {
            (Some(asked), _, _) => {
                let list = super::playlist_export::list(&project, asked)?;
                let folder = asked.folder.then(|| path.with_extension(""));
                let targets = list
                    .into_iter()
                    .map(|stem| Target {
                        path: stem_path(&path, folder.as_deref(), &stem.name),
                        track: None,
                        playlist_track: stem.track,
                    })
                    .collect();
                (targets, folder)
            }
            (None, Some(asked), Some(stem_options)) => {
                let list = stems(&project, stem_options).map_err(|error| error.to_string())?;
                let folder = asked.folder.then(|| path.with_extension(""));
                let targets = list
                    .into_iter()
                    .map(|stem| Target {
                        path: stem_path(&path, folder.as_deref(), &stem.name),
                        track: stem.track,
                        playlist_track: None,
                    })
                    .collect();
                (targets, folder)
            }
            _ => {
                let mix = Target {
                    path: path.clone(),
                    track: None,
                    playlist_track: None,
                };
                (vec![mix], None)
            }
        };

        if self.inner.exporting.swap(true, Ordering::SeqCst) {
            return Err("An export is already running.".to_owned());
        }
        self.inner.export_cancelled.store(false, Ordering::SeqCst);
        let job = Job {
            request,
            path,
            targets,
            folder,
            settings,
            stems: stem_options,
            playlist_stems,
            project,
            pool,
            pattern,
            options,
        };
        let session = self.clone();
        let spawned = std::thread::Builder::new()
            .name("windfall-export".to_owned())
            .spawn(move || {
                let _asset_pins = asset_pins;
                session.run_export(&job);
            });
        spawned.map(drop).map_err(|error| {
            self.inner.exporting.store(false, Ordering::SeqCst);
            format!("Could not start the export: {error}")
        })
    }

    /// Stops the export that is running, if one is. It ends with a last
    /// event that has `cancelled` set, and leaves no file: what it had
    /// written so far is removed, and the files it would have replaced are
    /// as they were. An export that is already putting its finished files
    /// in place goes through.
    pub fn export_cancel(&self) {
        if self.inner.exporting.load(Ordering::SeqCst) {
            self.inner.export_cancelled.store(true, Ordering::SeqCst);
        }
    }

    fn run_export(&self, job: &Job) {
        #[cfg(test)]
        self.pause("sampler:export-snapshot");
        let mut progress = Progress {
            session: self,
            path: job.options.path.clone(),
            request: job.request.clone(),
            sent_at: None,
            fraction: 0.0,
        };
        let cancelled = || self.inner.export_cancelled.load(Ordering::SeqCst);
        // A panic must not leave the session refusing every later export.
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            write_export(job, &mut progress, &cancelled)
        }));
        let (error, outcome) = match outcome {
            Ok(Ok(outcome)) => {
                if matches!(outcome, Outcome::Written { .. }) {
                    progress.fraction = 1.0;
                }
                (None, Some(outcome))
            }
            Ok(Err(reason)) => (
                Some(format!(
                    "Could not export to \"{}\": {reason}",
                    paths::display(&job.path)
                )),
                None,
            ),
            Err(_) => (Some("The export stopped unexpectedly.".to_owned()), None),
        };
        if let Some(error) = &error {
            log::warn!("{error}");
        }
        // Cleared first, so a UI that starts the next export on hearing
        // `done` is not refused.
        self.inner.exporting.store(false, Ordering::SeqCst);
        progress.send(true, error, outcome);
    }
}

fn audio_format(format: ExportFormat) -> AudioFormat {
    match format {
        ExportFormat::Wav => AudioFormat::Wav,
        ExportFormat::Flac => AudioFormat::Flac,
        ExportFormat::Ogg => AudioFormat::Ogg,
        ExportFormat::Mp3 => AudioFormat::Mp3,
        ExportFormat::Wv => AudioFormat::WavPack,
        ExportFormat::M4a => AudioFormat::M4a,
    }
}

/// How the files of an export are encoded, or why they cannot be the way
/// the options ask. Settings of a format that is not the one chosen are
/// not looked at.
fn encoder_settings(options: &ExportOptions) -> Result<EncoderSettings, String> {
    let settings = match options.format {
        ExportFormat::M4a => EncoderSettings::M4a { bitrate: options.aac_bitrate.unwrap_or(192) },
        ExportFormat::Wv => EncoderSettings::WavPack { depth: match options.bit_depth {
            BitDepth::Int16 => windfall_codec::WavPackDepth::Int16,
            BitDepth::Int24 => windfall_codec::WavPackDepth::Int24,
            BitDepth::Float32 => return Err("WavPack export supports 16/24-bit integer PCM. Choose an integer depth or WAV for float audio.".into()),
        } },
        ExportFormat::Ogg => EncoderSettings::Vorbis {
            quality: options.ogg_quality.unwrap_or(DEFAULT_VORBIS_QUALITY),
        },
        ExportFormat::Mp3 => EncoderSettings::Mp3 {
            settings: options
                .mp3
                .map_or_else(Mp3Settings::default, |settings| Mp3Settings {
                    rate: match settings.rate {
                        windfall_ipc::Mp3Rate::Cbr { bitrate } => Mp3Rate::Cbr(bitrate),
                        windfall_ipc::Mp3Rate::Vbr { quality } => Mp3Rate::Vbr(quality),
                    },
                    channels: match settings.channels {
                        windfall_ipc::Mp3Channels::Mono => Mp3Channels::Mono,
                        windfall_ipc::Mp3Channels::Stereo => Mp3Channels::Stereo,
                        windfall_ipc::Mp3Channels::JointStereo => Mp3Channels::JointStereo,
                    },
                }),
        },
        ExportFormat::Wav => EncoderSettings::Wav {
            format: match options.bit_depth {
                BitDepth::Int16 => WavSampleFormat::Int16,
                BitDepth::Int24 => WavSampleFormat::Int24,
                BitDepth::Float32 => WavSampleFormat::Float32,
            },
        },
        ExportFormat::Flac => {
            let depth = match options.bit_depth {
                BitDepth::Int16 => FlacBitDepth::Int16,
                BitDepth::Int24 => FlacBitDepth::Int24,
                BitDepth::Float32 => {
                    return Err(
                        "A FLAC file holds 16-bit or 24-bit audio. Choose one of the two, or export a WAV file to keep 32-bit float."
                            .to_owned(),
                    );
                }
            };
            let level = options.flac_level.unwrap_or(DEFAULT_FLAC_LEVEL);
            if level > MAX_FLAC_LEVEL {
                return Err(format!(
                    "FLAC compression levels go from 0 to {MAX_FLAC_LEVEL}, and {level} was asked for."
                ));
            }
            EncoderSettings::Flac { depth, level }
        }
    };
    // Whatever else the format cannot hold, in the encoder's own words.
    settings
        .check(options.sample_rate, encoder_channels(&settings))
        .map_err(|error| sentence(&error.to_string()))?;
    Ok(settings)
}

fn encoder_channels(settings: &EncoderSettings) -> u16 {
    if matches!(
        settings,
        EncoderSettings::Mp3 {
            settings: Mp3Settings {
                channels: Mp3Channels::Mono,
                ..
            }
        }
    ) {
        1
    } else {
        CHANNELS
    }
}

/// A message as a sentence: with a capital and a full stop.
fn sentence(message: &str) -> String {
    let mut letters = message.chars();
    let first = letters
        .next()
        .map(|letter| letter.to_uppercase().to_string());
    let mut sentence = first.unwrap_or_default();
    sentence.push_str(letters.as_str());
    if !sentence.ends_with('.') {
        sentence.push('.');
    }
    sentence
}

/// The most frames one file of an export can hold.
fn longest_frames(settings: &EncoderSettings, sample_rate: u32) -> f64 {
    let day = MAX_SECONDS * f64::from(sample_rate);
    match settings {
        EncoderSettings::Wav { format } => {
            let bytes = match format {
                WavSampleFormat::Int16 => 2.0,
                WavSampleFormat::Int24 => 3.0,
                WavSampleFormat::Float32 => 4.0,
            };
            day.min((MAX_WAV_BYTES / (bytes * f64::from(CHANNELS))).floor())
        }
        EncoderSettings::Flac { .. }
        | EncoderSettings::Vorbis { .. }
        | EncoderSettings::Mp3 { .. } => day,
        EncoderSettings::WavPack { depth } => {
            let bytes = match depth { windfall_codec::WavPackDepth::Int16 => 2.0, windfall_codec::WavPackDepth::Int24 => 3.0 };
            day.min((MAX_WAV_BYTES / (bytes * f64::from(CHANNELS))).floor())
        }
        EncoderSettings::M4a { .. } => day.min((windfall_codec::MAX_M4A_PACKETS as f64 * 1024.0) - 8192.0),
    }
}

/// The file an export writes, from the path the user gave.
///
/// A name with no extension gets the format's, as saving a project adds
/// `.windfall`. A name that ends in another extension is refused, because
/// writing a WAV file called `beat.mp3` helps nobody. Only something that
/// looks like an extension counts as one: up to five letters and digits
/// with a letter among them. The `2` of `take.2` and the ` Intro` of
/// `01. Intro` are part of the name, and the extension is added after them.
fn export_path(path: PathBuf, format: AudioFormat) -> Result<PathBuf, String> {
    let wanted = format.extension();
    let given = path
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| {
            extension.len() <= 5
                && extension.chars().all(|c| c.is_ascii_alphanumeric())
                && extension.chars().any(|c| c.is_ascii_alphabetic())
        });
    match given {
        Some(extension) if extension.eq_ignore_ascii_case(wanted) => Ok(path),
        Some(extension) => Err(format!(
            "The export is a {} file, so its name cannot end in .{extension}. End it in .{wanted}, leave the ending off, or choose another format.",
            format.name()
        )),
        None => {
            let mut name = path.into_os_string();
            name.push(".");
            name.push(wanted);
            Ok(PathBuf::from(name))
        }
    }
}

/// The file of the stem called `stem` of an export to `path`: named after
/// the export and the stem, with the export's ending, in `folder` or else
/// where `path` is.
fn stem_path(path: &Path, folder: Option<&Path>, stem: &str) -> PathBuf {
    let mut name = path.file_stem().unwrap_or_default().to_os_string();
    name.push(" - ");
    name.push(stem);
    if let Some(extension) = path.extension() {
        name.push(".");
        name.push(extension);
    }
    match folder {
        Some(folder) => folder.join(name),
        None => path.with_file_name(name),
    }
}

/// Writes the files of an export. Whatever way it ends without all of them
/// written, none is left, and a folder made for them is removed again.
fn write_export(
    job: &Job,
    progress: &mut Progress,
    cancelled: &dyn Fn() -> bool,
) -> Result<Outcome, String> {
    let made_folder = match &job.folder {
        Some(folder) if !folder.is_dir() => {
            fs::create_dir_all(folder).map_err(|error| {
                format!(
                    "the folder \"{}\" could not be made ({error})",
                    paths::display(folder)
                )
            })?;
            true
        }
        _ => false,
    };
    let outcome = write_files(job, progress, cancelled);
    if let (true, Some(folder), false) = (
        made_folder,
        &job.folder,
        matches!(outcome, Ok(Outcome::Written { .. })),
    ) {
        // Only an empty folder goes: nothing but this export put it there.
        let _ = fs::remove_dir(folder);
    }
    outcome
}

/// Renders the job straight into its encoders, one for each file.
fn write_files(
    job: &Job,
    progress: &mut Progress,
    cancelled: &dyn Fn() -> bool,
) -> Result<Outcome, String> {
    let prepared_pool = job
        .pool
        .prepare_plugin_render()
        .prepare_samplers(&job.project, &mut || !cancelled(), &mut |_, _, _| {})
        .map_err(|error| error.to_string())?;
    let options = &job.options;
    let render = RenderOptions {
        region: options.region,
        sample_rate: options.sample_rate,
        mode: options.mode,
        pattern: Some(job.pattern),
        pattern_loops: options.pattern_loops,
        tail_secs: options.tail_secs,
        auto_tail: options.auto_tail,
        ..RenderOptions::default()
    };

    // Dropping an encoder removes what it has written, so every way out of
    // here before the files are in place leaves nothing behind.
    let mut encoders = Vec::with_capacity(job.targets.len());
    let mut transaction = FileTransaction::new(&job.targets);
    let project_settings = &job.project.settings;
    let tags = AudioTags {
        title: &project_settings.name,
        author: &project_settings.author,
        genre: &project_settings.genre,
        comments: &project_settings.comments,
    };
    let wav_metadata = if matches!(job.settings, EncoderSettings::Wav { .. }) {
        match options.mode {
            PlayMode::Song => markers::song_metadata(
                &job.project,
                &prepared_pool,
                options.sample_rate,
                options.region,
            )?,
            PlayMode::Pattern => markers::pattern_metadata(
                &job.project,
                job.pattern,
                options.sample_rate,
                options.pattern_loops,
            )?,
        }
    } else {
        windfall_codec::WavMetadata::default()
    };
    for (target, file) in job.targets.iter().zip(&transaction.files) {
        let encoder = Encoder::open_with_wav_metadata(
            &file.stage,
            &job.settings,
            options.sample_rate,
            encoder_channels(&job.settings),
            &tags,
            &wav_metadata,
        );
        encoders.push(encoder.map_err(|error| named(&target.path, job, &error.to_string()))?);
    }

    // With mastering the render goes to a file beside the export first, and
    // the encoder is given the master once the whole mix has been measured.
    let mut master = match &options.mastering {
        Some(asked) => Some(mastering::MasterJob::start(
            &transaction.files[0].stage,
            options.sample_rate,
            asked,
        )?),
        None => None,
    };
    let mastered = master.is_some();

    let mut failure = None;
    let mut mono = Vec::with_capacity(render.block_frames);
    let mut write = |stream: usize, block: &[f32]| {
        if let Some(master) = master.as_mut() {
            return match master.take(block) {
                Ok(()) => true,
                Err(error) => {
                    failure = Some(error);
                    false
                }
            };
        }
        let samples = if encoder_channels(&job.settings) == 1 {
            mono.clear();
            mono.extend(
                block
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| (pair[0] + pair[1]) * 0.5),
            );
            mono.as_slice()
        } else {
            block
        };
        match encoders[stream].write(samples) {
            Ok(()) => true,
            Err(error) => {
                failure = Some(named(&job.targets[stream].path, job, &error.to_string()));
                false
            }
        }
    };
    let mut report = |fraction: f32| {
        // Rendering is the first half of an export that is mastered.
        progress.report(if mastered { fraction * 0.5 } else { fraction });
        !cancelled() && prepared_pool.plugin_render_error().is_none()
    };
    let streamed: Streamed = match (&job.playlist_stems, &job.stems) {
        (Some(lanes), _) => render_playlist_stems(
            &job.project,
            &prepared_pool,
            &render,
            lanes,
            &mut write,
            &mut report,
        )
        .map_err(|error| error.to_string())?,
        (None, Some(stems)) => render_stems(
            &job.project,
            &prepared_pool,
            &render,
            stems,
            &mut write,
            &mut report,
        )
        .map_err(|error| error.to_string())?,
        (None, None) => render_streaming_checked(
            &job.project,
            &prepared_pool,
            &render,
            &mut |block| write(0, block),
            &mut report,
        )
        .map_err(|error| error.to_string())?,
    };
    if let Some(reason) = failure {
        return Err(reason);
    }
    if let Some(error) = streamed
        .plugin_error
        .or_else(|| prepared_pool.plugin_render_error())
    {
        return Err(error);
    }
    if !streamed.completed || cancelled() {
        return Ok(Outcome::Cancelled);
    }
    if streamed.frames == 0 {
        return Err("there is nothing to export".to_owned());
    }

    let review = match master.as_mut() {
        Some(master) => {
            let mut mono = Vec::new();
            let channels = encoder_channels(&job.settings);
            let encoder = &mut encoders[0];
            let mut sink = |block: &[f32]| {
                let samples = if channels == 1 {
                    mono.clear();
                    mono.extend(
                        block
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|pair| (pair[0] + pair[1]) * 0.5),
                    );
                    mono.as_slice()
                } else {
                    block
                };
                encoder.write(samples).map_err(|error| error.to_string())
            };
            let mut advance = |fraction: f32| {
                progress.report(0.5 + 0.5 * fraction);
                !cancelled()
            };
            match master.finish(&mut sink, &mut advance)? {
                Some(review) => Some(review),
                None => return Ok(Outcome::Cancelled),
            }
        }
        None => None,
    };
    // The mix that was set aside goes before the files are put in place.
    drop(master);

    // Finalize under staging names so a later encoder failure cannot
    // replace an earlier destination. The transaction restores backups
    // if moving a completed set fails part way through.
    if let Some(error) = prepared_pool.plugin_render_error() {
        return Err(error);
    }
    for (encoder, target) in encoders.into_iter().zip(&job.targets) {
        if let Err(error) = encoder.finalize() {
            return Err(named(&target.path, job, &error.to_string()));
        }
    }
    if cancelled() {
        return Ok(Outcome::Cancelled);
    }
    if let Some(error) = prepared_pool.plugin_render_error() {
        return Err(error);
    }
    transaction.commit()?;
    let files = job.targets.iter().map(|target| ExportedFile {
        path: paths::display(&target.path),
        track: target.track,
        playlist_track: target.playlist_track,
        frames: streamed.frames,
        bytes: fs::metadata(&target.path).map_or(0, |file| file.len()),
    });
    Ok(Outcome::Written {
        files: files.collect(),
        dropped_clips: streamed.dropped_clips,
        mastering: review,
    })
}

/// A reason with the name of the file it is about, when the export has
/// more than the one file its message names anyway.
fn named(path: &Path, job: &Job, reason: &str) -> String {
    if job.targets.len() == 1 && path == job.path {
        reason.to_owned()
    } else {
        format!("\"{}\": {reason}", paths::name(path))
    }
}

/// How many frames an export will be at most, near enough to refuse one
/// that is far too long before anything is rendered.
fn expected_frames(project: &Project, pattern: PatternId, options: &ExportOptions) -> f64 {
    let ticks = match options.mode {
        PlayMode::Pattern => {
            let length = project
                .pattern(pattern)
                .or(project.patterns.first())
                .map_or(0, |pattern| pattern.length_ticks());
            f64::from(length) * f64::from(options.pattern_loops)
        }
        PlayMode::Song if options.region.is_some() => {
            let range = options.region.expect("a selected range exists");
            f64::from(range.end - range.start)
        }
        PlayMode::Song => project
            .playlist
            .clips
            .iter()
            .map(|clip| f64::from(clip.start) + f64::from(clip.length))
            .chain(project.settings.stage_media.videos.iter().map(|video| f64::from(video.start_tick) + f64::from(video.length_ticks)))
            .fold(0.0, f64::max),
    };
    let tempo = match options.mode {
        PlayMode::Pattern => project.settings.tempo_bpm,
        PlayMode::Song => slowest_tempo(project),
    };
    let sample_rate = f64::from(options.sample_rate);
    ticks * samples_per_tick(tempo, sample_rate) + f64::from(options.tail_secs) * sample_rate
}

/// The slowest the song's tempo gets: the stored tempo, or the lowest
/// point of an automation of the tempo if that is slower. A song is never
/// longer than it would be at this tempo throughout.
fn slowest_tempo(project: &Project) -> f64 {
    let automations = project.automations.iter();
    let points = automations
        .filter(|automation| automation.target == AutomationTarget::Tempo)
        .flat_map(|automation| &automation.points);
    points
        .map(|point| f64::from(AutomationRange::TEMPO.value(point.value)))
        .fold(project.settings.tempo_bpm, f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_of_the_format_is_added_when_the_name_has_none() {
        let named = |name: &str| export_path(PathBuf::from(name), AudioFormat::Wav);
        assert_eq!(named("beat"), Ok(PathBuf::from("beat.wav")));
        assert_eq!(named("beat.wav"), Ok(PathBuf::from("beat.wav")));
        assert_eq!(named("Beat.WAV"), Ok(PathBuf::from("Beat.WAV")));
        // A dot in the name does not make an extension of what follows.
        assert_eq!(named("take.2"), Ok(PathBuf::from("take.2.wav")));
        assert_eq!(named("01. Intro"), Ok(PathBuf::from("01. Intro.wav")));
        assert_eq!(named("v1.final-mix"), Ok(PathBuf::from("v1.final-mix.wav")));
        assert_eq!(named(".hidden"), Ok(PathBuf::from(".hidden.wav")));

        let flac = |name: &str| export_path(PathBuf::from(name), AudioFormat::Flac);
        assert_eq!(flac("beat"), Ok(PathBuf::from("beat.flac")));
        assert_eq!(flac("beat.FLAC"), Ok(PathBuf::from("beat.FLAC")));
    }

    #[test]
    fn a_song_is_sized_by_the_slowest_its_tempo_gets() {
        use windfall_project::{
            Automation, AutomationId, AutomationPoint, Clip, ClipContent, ClipId, PlaylistTrackId,
        };

        // Four bars of a pattern at a stored 120 bpm: eight seconds.
        let mut project = Project::new("Song");
        let pattern = project.patterns[0].id;
        project.playlist.clips.push(Clip {
            id: ClipId(50),
            track: PlaylistTrackId(40),
            start: 0,
            length: 15_360,
            offset: 0,
            muted: false,
            content: ClipContent::Pattern { pattern },
        });
        let options = ExportOptions {
            bit_depth: BitDepth::Float32,
            tail_secs: 1.0,
            ..ExportOptions::default()
        };
        assert_eq!(expected_frames(&project, pattern, &options), 9.0 * 48_000.0);

        // A tempo curve that dips to 60 bpm somewhere can make the song
        // twice as long, and that is what room is checked for.
        let point = |tick: u32, bpm: f32| AutomationPoint {
            tick,
            value: (bpm - 10.0) / 512.0,
            curve: 0.0,
            hold: false,
        };
        project.automations.push(Automation {
            id: AutomationId(60),
            name: String::new(),
            color: 0,
            target: AutomationTarget::Tempo,
            points: vec![point(0, 200.0), point(960, 60.0), point(1_920, 300.0)],
        });
        assert_eq!(slowest_tempo(&project), 60.0);
        assert_eq!(
            expected_frames(&project, pattern, &options),
            17.0 * 48_000.0
        );
        // A pattern is exported at the stored tempo, which no curve moves.
        let looped = ExportOptions {
            mode: PlayMode::Pattern,
            ..options
        };
        assert_eq!(expected_frames(&project, pattern, &looped), 3.0 * 48_000.0);
    }

    #[test]
    fn a_name_that_asks_for_another_format_is_refused() {
        for (name, extension) in [
            ("beat.mp3", "mp3"),
            ("beat.FLAC", "FLAC"),
            ("beat.wav.ogg", "ogg"),
            ("beat.txt", "txt"),
        ] {
            let error = export_path(PathBuf::from(name), AudioFormat::Wav).unwrap_err();
            assert_eq!(
                error,
                format!(
                    "The export is a WAV file, so its name cannot end in .{extension}. End it in .wav, leave the ending off, or choose another format."
                )
            );
        }
        assert_eq!(
            export_path(PathBuf::from("beat.wav"), AudioFormat::Flac).unwrap_err(),
            "The export is a FLAC file, so its name cannot end in .wav. End it in .flac, leave the ending off, or choose another format."
        );
    }

    #[test]
    fn stems_are_named_after_the_export_and_keep_its_ending() {
        let path = Path::new("out").join("My Song.FLAC");
        assert_eq!(
            stem_path(&path, None, "03 Bass"),
            Path::new("out").join("My Song - 03 Bass.FLAC")
        );
        let folder = path.with_extension("");
        assert_eq!(folder, Path::new("out").join("My Song"));
        assert_eq!(
            stem_path(&path, Some(&folder), "Mix"),
            Path::new("out").join("My Song").join("My Song - Mix.FLAC")
        );
        // A dot in the name is part of the name.
        let dotted = Path::new("take.2.wav");
        assert_eq!(
            stem_path(dotted, None, "Mix"),
            Path::new("take.2 - Mix.wav")
        );
    }

    #[test]
    fn a_wav_file_is_limited_by_its_header_and_the_others_by_the_day() {
        let wav = |format| longest_frames(&EncoderSettings::Wav { format }, 48_000);
        // 4 GB of 32-bit float stereo is a little over three hours.
        assert_eq!(
            (wav(WavSampleFormat::Float32) / 48_000.0 / 60.0).floor(),
            186.0
        );
        assert_eq!(
            (wav(WavSampleFormat::Int16) / 48_000.0 / 60.0).floor(),
            372.0
        );
        let flac = EncoderSettings::Flac {
            depth: FlacBitDepth::Int24,
            level: 5,
        };
        assert_eq!(longest_frames(&flac, 48_000), 24.0 * 3_600.0 * 48_000.0);
        // At a low rate a day is less than a WAV file holds.
        assert_eq!(
            longest_frames(
                &EncoderSettings::Wav {
                    format: WavSampleFormat::Int16
                },
                8_000
            ),
            24.0 * 3_600.0 * 8_000.0
        );
    }

    #[test]
    fn a_message_is_made_a_sentence() {
        assert_eq!(sentence("cannot write"), "Cannot write.");
        assert_eq!(sentence("Already one."), "Already one.");
    }
}
