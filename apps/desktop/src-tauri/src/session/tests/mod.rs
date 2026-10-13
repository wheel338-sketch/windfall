//! Headless tests of the session: no window, no audio device. The engine is
//! a [`Processor`] the tests run by hand.

mod analysis_jobs;
mod analyzers;
mod archive;
mod audio_editor;
mod audio_logger;
mod beat;
mod classical_analysis;
mod clip_recording;
mod consolidate;
mod controllers;
mod detached_windows;
mod device_acceptance;
mod document;
mod effects;
mod export;
mod export_formats;
mod export_tempo;
mod files;
mod files_qa;
mod flp;
mod import_formats;
mod preset_render;
mod import_recording;
mod library;
mod mastering_qa;
mod midi;
mod midi_hardware;
#[cfg(windows)]
mod mixer_preset_native;
#[path = "../mixer_presets_qa.rs"]
mod mixer_presets_qa;
#[path = "../mixer_render_qa.rs"]
mod mixer_render_qa;
mod nonlinear_performance;
mod p2_convolver;
mod pattern_movement;
mod performance_media;
mod performance_media_evidence;
mod piano_waveform;
mod playback;
mod playlist_bounce;
mod playlist_export;
mod plugin_recording;
#[cfg(windows)]
mod plugin_update;
#[cfg(windows)]
mod plugins;
mod project_preparation;
mod rack_assets;
mod rack_second_pass;
mod recording;
#[path = "../recording_completion_qa.rs"]
mod recording_completion_qa;
mod remote_cloud_content;
mod sample_playback;
mod sampler_preprocess;
mod sampler_processing;
mod scripting;
#[cfg(windows)]
mod sidechain_buses;
mod slicer;
mod song;
mod stem_separation;
mod timeline;
mod transport_acceptance;
mod update_shutdown;
mod versions;
mod video_playback;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use windfall_engine::Processor;
use windfall_ipc::{AudioHost, AudioSettings, EngineStatus, ExportProgress, TransportState};
use windfall_project::{ChannelId, PatternId, Project, ProjectPatch, SampleId};

use super::{AudioDevice, Session, SessionConfig};
use crate::events::{Event, EventSink};
use crate::paths;
use crate::settings::{SETTINGS_FILE, SettingsStore};
use crate::shell::DEV_FACTORY_DIR;
use crate::sync::lock;

/// Sample rate of the engine the tests run.
const SAMPLE_RATE: u32 = 48_000;

/// How long a test waits for another thread before it fails.
const PATIENCE: Duration = Duration::from_secs(30);

type Hook = Box<dyn FnMut(&Event) + Send>;

/// An event sink that keeps what it is sent.
#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<Event>>,
    arrived: Condvar,
    /// Called with each event before it is recorded, on the emitting thread.
    hook: Mutex<Option<Hook>>,
}

impl EventSink for Recorder {
    fn emit(&self, event: Event) {
        // Taken out while it runs: a hook may block, and the events other
        // threads emit meanwhile must still get through.
        let hook = lock(&self.hook).take();
        if let Some(mut hook) = hook {
            hook(&event);
            lock(&self.hook).get_or_insert(hook);
        }
        lock(&self.events).push(event);
        self.arrived.notify_all();
    }
}

impl Recorder {
    /// Removes and returns everything recorded so far.
    fn take(&self) -> Vec<Event> {
        std::mem::take(&mut lock(&self.events))
    }

    /// Waits until `wanted` picks something out of the recorded events.
    fn wait_for<T>(&self, mut wanted: impl FnMut(&[Event]) -> Option<T>) -> T {
        let deadline = Instant::now() + PATIENCE;
        let mut events = lock(&self.events);
        loop {
            if let Some(found) = wanted(&events) {
                return found;
            }
            let left = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_else(|| panic!("timed out; the events so far are {events:#?}"));
            events = self
                .arrived
                .wait_timeout(events, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }

    /// Waits for the event that ends an export.
    fn wait_for_export(&self) -> ExportProgress {
        self.wait_for(|events| {
            events.iter().find_map(|event| match event {
                Event::ExportProgress(progress) if progress.done => Some(progress.clone()),
                _ => None,
            })
        })
    }
}

/// Points in the session's slow work where a test can stop a thread, to
/// run something else at exactly that moment.
///
/// The session reaches a point by calling [`Session::pause`] with its
/// name, which returns at once unless a test holds the point.
#[derive(Default)]
pub(super) struct Pauses {
    points: Mutex<HashMap<&'static str, Stage>>,
    moved: Condvar,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// The next thread to reach the point stops there.
    Armed,
    /// A thread is stopped at the point. Others pass it.
    Reached,
}

impl Pauses {
    /// Waits, with the points locked, until `done` says so.
    fn wait_until(&self, what: &str, done: impl Fn(&HashMap<&'static str, Stage>) -> bool) {
        let deadline = Instant::now() + PATIENCE;
        let mut points = lock(&self.points);
        while !done(&points) {
            let left = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_else(|| panic!("timed out waiting {what}"));
            points = self
                .moved
                .wait_timeout(points, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }
}

impl Session {
    /// A point a test may hold a thread at.
    pub(super) fn pause(&self, point: &'static str) {
        let pauses = &self.inner.pauses;
        {
            let mut points = lock(&pauses.points);
            if points.get(point) != Some(&Stage::Armed) {
                return;
            }
            points.insert(point, Stage::Reached);
            pauses.moved.notify_all();
        }
        pauses.wait_until(&format!("to be released from {point}"), |points| {
            !points.contains_key(point)
        });
    }

    /// Makes the next thread that reaches `point` stop there until the
    /// hold is released.
    fn hold(&self, point: &'static str) -> Hold {
        lock(&self.inner.pauses.points).insert(point, Stage::Armed);
        Hold {
            session: self.clone(),
            point,
        }
    }

    /// Runs `work` on another thread, as the app runs a slow command.
    fn background<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Session) -> T + Send + 'static,
    ) -> JoinHandle<T> {
        let session = self.clone();
        std::thread::spawn(move || work(&session))
    }
}

/// A thread stopped, or about to be, at a point of the session's work.
struct Hold {
    session: Session,
    point: &'static str,
}

impl Hold {
    /// Waits until a thread is stopped at the point.
    fn wait(&self) {
        let point = self.point;
        self.session
            .inner
            .pauses
            .wait_until(&format!("for a thread to reach {point}"), |points| {
                points.get(point) == Some(&Stage::Reached)
            });
    }

    /// Lets the stopped thread go on.
    fn release(self) {}
}

impl Drop for Hold {
    fn drop(&mut self) {
        let pauses = &self.session.inner.pauses;
        lock(&pauses.points).remove(self.point);
        pauses.moved.notify_all();
    }
}

/// How long a test gives a thread to finish something it must not be able
/// to finish. The test is right whether or not the time runs out; a wait
/// only makes sure that code which lets the thread through is caught.
const GRACE: Duration = Duration::from_millis(150);

/// Whether a thread that must be waiting for another is in fact still
/// running once [`GRACE`] is up.
fn still_running<T>(thread: &JoinHandle<T>) -> bool {
    let deadline = Instant::now() + GRACE;
    while Instant::now() < deadline && !thread.is_finished() {
        std::thread::sleep(Duration::from_millis(1));
    }
    !thread.is_finished()
}

/// An audio device that opens whatever it is asked for, except a device
/// named "Unplugged".
struct FakeDevice {
    status: Mutex<EngineStatus>,
    /// What [`AudioDevice::devices`] lists.
    hosts: Mutex<Vec<AudioHost>>,
}

impl FakeDevice {
    fn describe(settings: &AudioSettings) -> EngineStatus {
        let sample_rate = settings.sample_rate.unwrap_or(SAMPLE_RATE);
        let buffer_frames = settings.buffer_frames.unwrap_or(480);
        let unplugged = settings.device.as_deref() == Some("Unplugged");
        EngineStatus {
            output_channels: 0,
            running: !unplugged,
            host: settings.host.clone().unwrap_or_else(|| "Test".to_owned()),
            device: settings.device.clone().or(Some("Speakers".to_owned())),
            sample_rate,
            buffer_frames,
            latency_ms: buffer_frames as f32 * 1000.0 / sample_rate as f32,
            latency_frames: 0,
            error: unplugged.then(|| "audio output device \"Unplugged\" was not found".to_owned()),
        }
    }
}

impl AudioDevice for FakeDevice {
    fn suspend(&self) -> Result<(), String> {
        lock(&self.status).running = false;
        Ok(())
    }
    fn status(&self) -> EngineStatus {
        lock(&self.status).clone()
    }

    fn reconfigure(&self, settings: &AudioSettings) {
        *lock(&self.status) = Self::describe(settings);
    }

    fn devices(&self) -> Vec<AudioHost> {
        lock(&self.hosts).clone()
    }
}

/// A session with everything around it that a test wants to look at.
struct Rig {
    session: Session,
    /// The audio thread, run by calling [`Rig::run`].
    processor: Processor,
    events: Arc<Recorder>,
    device: Arc<FakeDevice>,
    /// Holds the settings file and whatever the test saves.
    folder: TempDir,
}

impl Rig {
    fn new() -> Self {
        Self::in_folder(tempfile::tempdir().expect("a temporary folder"))
    }

    /// Starts a session whose settings file is in `folder`, as a second run
    /// of the app on the same machine would.
    fn in_folder(folder: TempDir) -> Self {
        let settings = SettingsStore::load(folder.path().join(SETTINGS_FILE));
        let (processor, controller) = Processor::new(SAMPLE_RATE);
        let events = Arc::new(Recorder::default());
        let device = Arc::new(FakeDevice {
            status: Mutex::new(FakeDevice::describe(&settings.settings().audio)),
            hosts: Mutex::new(Vec::new()),
        });
        let session = Session::new(SessionConfig {
            controller,
            audio: device.clone(),
            events: events.clone(),
            factory_dir: PathBuf::from(DEV_FACTORY_DIR),
            settings,
        });
        Self {
            session,
            processor,
            events,
            device,
            folder,
        }
    }

    /// Ends this run of the app and starts another with the same settings.
    fn restart(self) -> Self {
        let Self {
            session, folder, ..
        } = self;
        drop(session);
        Self::in_folder(folder)
    }

    /// Runs the audio thread for `frames` frames and returns what it played,
    /// interleaved stereo.
    fn run(&mut self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        // In blocks, as a device would ask for it.
        for block in out.chunks_mut(960) {
            self.processor.process(block);
        }
        out
    }

    fn project(&self) -> Project {
        self.session.document_snapshot().project
    }

    fn channel(&self, index: usize) -> ChannelId {
        self.project().channels[index].id
    }

    fn pattern(&self) -> PatternId {
        self.project().patterns[0].id
    }

    /// A path inside the test's own folder.
    fn file(&self, name: &str) -> String {
        paths::display(&self.folder.path().join(name))
    }

    /// The patches recorded so far, removed from the record along with
    /// every other event.
    fn take_patches(&self) -> Vec<ProjectPatch> {
        self.events
            .take()
            .into_iter()
            .filter_map(|event| match event {
                Event::ProjectPatch(patch) => Some(patch),
                _ => None,
            })
            .collect()
    }

    fn take_transport_states(&self) -> Vec<TransportState> {
        self.events
            .take()
            .into_iter()
            .filter_map(|event| match event {
                Event::TransportState(state) => Some(state),
                _ => None,
            })
            .collect()
    }

    /// Whether the engine has been handed audio for a sample.
    fn has_audio(&self, sample: SampleId) -> bool {
        let state = self.session.state();
        state.pool.contains(sample) && state.loaded.contains(&sample)
    }

    /// Waits for the background loader to finish with a sample.
    fn wait_until_loaded_or_failed(&self, sample: SampleId) {
        let deadline = Instant::now() + PATIENCE;
        loop {
            {
                let state = self.session.state();
                if state.loaded.contains(&sample) || state.failed.contains(&sample) {
                    return;
                }
            }
            assert!(Instant::now() < deadline, "sample {sample:?} never loaded");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// The full path of a file of the factory content.
fn factory_file(relative: &str) -> String {
    paths::display(&paths::clean(&Path::new(DEV_FACTORY_DIR).join(relative)))
}

/// Root mean square of interleaved samples.
fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|s| f64::from(*s).powi(2)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

#[test]
fn a_session_starts_on_a_clean_default_project_the_engine_can_play() {
    let mut rig = Rig::new();
    let snapshot = rig.session.document_snapshot();
    assert_eq!(snapshot.revision, 0);
    assert!(!snapshot.dirty);
    assert_eq!(snapshot.path, None);
    assert!(snapshot.history.entries.is_empty());
    assert_eq!(snapshot.project.settings.name, "Untitled");
    let names: Vec<&str> = snapshot
        .project
        .channels
        .iter()
        .map(|channel| channel.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["Kick Punch", "Clap Wide", "Hat Closed 1", "Snare Tight"]
    );
    for sample in &snapshot.project.samples {
        assert!(rig.has_audio(sample.id), "{sample:?}");
    }
    assert!(rig.events.take().is_empty());

    // The transport already points at the project's pattern.
    let transport = rig.session.transport_state();
    assert!(!transport.playing);
    assert_eq!(transport.pattern, rig.pattern());

    // The kit is in the engine: a note played by hand makes sound.
    rig.session.audition_note_on(rig.channel(0), 60, 1.0);
    assert!(rms(&rig.run(4_800)) > 0.01);
}

mod sample_instruments;
mod sample_playback_workflow;
