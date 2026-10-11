//! The audio logger, with the engine run by hand and no audio device: what
//! a dump writes, where the clip goes, when a dump is refused, how the
//! logged input behaves, and what is remembered between runs.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use windfall_ipc::{
    AudioLoggerDumpRequest, AudioLoggerInput, AudioLoggerSettings, AudioLoggerSource,
    AudioLoggerSourceStatus, AudioLoggerState, AudioSettings, RecordingSource,
};
use windfall_project::{ClipContent, PlaylistTrackId, SamplePath, TrackId};

use super::{rms, Rig, SAMPLE_RATE};
use crate::session::audio_logger::InputBackend;
use crate::session::recording::{CaptureHandle, Sink};
use crate::settings::SETTINGS_FILE;
use crate::sync::lock;

const MASTER: AudioLoggerSource = AudioLoggerSource::Master;
const INPUT: AudioLoggerSource = AudioLoggerSource::Input;

#[test]
fn enable_song_play_dump_preserves_sample_alignment_through_playlist_and_reopen() {
    use windfall_ipc::{PlayMode, TransportPatch};
    use windfall_project::{Command, MixerTrackPatch};
    let mut rig = Rig::new();
    // Nonperiodic signed bursts provide an independent frame alignment oracle.
    let mut pcm = vec![0.0_f32; 48_000 * 2];
    for (frame, value) in [(1001, 0.2), (7237, -0.3), (17003, 0.4), (23119, -0.25)] {
        pcm[frame * 2] = value;
        pcm[frame * 2 + 1] = -value * 0.5;
    }
    let stimulus = rig.file("logger-stimulus.wav");
    windfall_codec::write_wav(
        Path::new(&stimulus),
        &windfall_core::AudioBuffer::from_interleaved(SAMPLE_RATE, 2, pcm),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
    rig.session
        .add_audio_clip_from_file(
            &stimulus,
            crate::session::ClipPlace {
                track: None,
                start: 0,
                mixer_track: None,
            },
        )
        .unwrap();
    rig.session
        .dispatch(
            Command::UpdateMixerTrack {
                id: TrackId::MASTER,
                patch: MixerTrackPatch {
                    volume: Some(1.0),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    rig.session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Song),
            loop_song: Some(false),
            ..Default::default()
        })
        .unwrap();
    rig.session.audio_logger_configure(settings()).unwrap();
    rig.session.transport_play().unwrap();
    let mut played = Vec::new();
    for _ in 0..50 {
        played.extend(rig.run(480));
        rig.session.audio_logger_state();
    }
    assert!(rms(&played) > 0.0001);
    let before = rig.session.document_snapshot();
    let result = rig
        .session
        .audio_logger_dump(AudioLoggerDumpRequest {
            source: MASTER,
            seconds: None,
            start: Some(3840),
            track: None,
        })
        .unwrap();
    let clip_id = windfall_project::ClipId(*result.created.last().unwrap());
    let project = rig.project();
    let clip = project
        .playlist
        .clips
        .iter()
        .find(|clip| clip.id == clip_id)
        .unwrap();
    assert_eq!(clip.start, 3840);
    let ClipContent::Audio { sample, .. } = clip.content else {
        panic!("dump is audio")
    };
    let recovered = rig.session.state().pool.get(sample).unwrap().clone();
    assert_eq!(recovered.samples(), played);
    let bursts = |audio: &[f32]| {
        audio
            .chunks_exact(2)
            .enumerate()
            .filter(|(_, frame)| frame[0].abs() > 1e-4)
            .map(|(frame, _)| frame)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        bursts(recovered.samples()),
        bursts(&played),
        "zero sample lag, including native PDC"
    );
    assert_eq!(
        rig.session.document_snapshot().history.cursor,
        before.history.cursor + 1
    );
    rig.session.undo().unwrap();
    assert!(rig
        .project()
        .playlist
        .clips
        .iter()
        .all(|clip| clip.id != clip_id));
    rig.session.redo().unwrap();
    rig.session.transport_stop();
    let path = rig.file("aligned-logger.windfall");
    rig.session.project_save(Some(&path)).unwrap();
    let rig = rig.restart();
    rig.session.project_open(&path).unwrap();
    let reopened = rig.project();
    assert_eq!(
        reopened
            .playlist
            .clips
            .iter()
            .find(|clip| clip.id == clip_id),
        Some(clip)
    );
    assert_eq!(
        rig.session.state().pool.get(sample).unwrap().samples(),
        played
    );
}

fn settings() -> AudioLoggerSettings {
    AudioLoggerSettings {
        seconds: 30,
        ..AudioLoggerSettings::default()
    }
}

/// A rig whose logger runs with `settings`, as after the app has started.
fn logging(settings: AudioLoggerSettings) -> Rig {
    let rig = Rig::new();
    rig.session.audio_logger_configure(settings).unwrap();
    rig
}

/// Runs the audio thread for `frames` frames with a kit sound started every
/// quarter second, and lets the logger take in what was played often enough
/// that its queue cannot overflow, however fast the test machine is.
fn play(rig: &mut Rig, frames: usize, channels: &[usize]) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * 2);
    let mut turn = 0;
    while out.len() < frames * 2 {
        let step = (frames - out.len() / 2).min(12_000);
        let channel = rig.channel(channels[turn % channels.len()]);
        rig.session.audition_note_on(channel, 60, 1.0);
        out.extend(rig.run(step));
        rig.session.audio_logger_state();
        turn += 1;
    }
    out
}

fn status(state: &AudioLoggerState, source: AudioLoggerSource) -> &AudioLoggerSourceStatus {
    state
        .sources
        .iter()
        .find(|status| status.source == source)
        .unwrap_or_else(|| panic!("{source:?} is not among {:#?}", state.sources))
}

fn held(rig: &Rig, source: AudioLoggerSource) -> u64 {
    status(&rig.session.audio_logger_state(), source).available_frames
}

fn dump(source: AudioLoggerSource, seconds: Option<f64>) -> AudioLoggerDumpRequest {
    AudioLoggerDumpRequest {
        source,
        seconds,
        start: Some(0),
        track: None,
    }
}

/// The file of the sample the project got last.
fn last_file(rig: &Rig) -> PathBuf {
    match &rig.project().samples.last().unwrap().path {
        SamplePath::External(path) => PathBuf::from(path),
        other => panic!("the logged audio is stored as {other:?}"),
    }
}

fn recordings(rig: &Rig) -> PathBuf {
    rig.folder.path().join("recordings")
}

fn logged_files(rig: &Rig) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(recordings(rig)) else {
        return Vec::new();
    };
    entries
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("Logged-"))
        .collect()
}

fn decode(file: &Path) -> (u32, Vec<f32>) {
    let audio = windfall_codec::decode_file(file).unwrap();
    assert_eq!(audio.channels(), 2);
    (audio.sample_rate(), audio.samples().to_vec())
}

/// An input that opens no device. The test plays the driver: it hands audio
/// to the capture's sink and decides when the capture fails.
struct FakeInput {
    rate: u32,
    refuse: AtomicBool,
    opens: AtomicU32,
    closes: Arc<AtomicU32>,
    failed: Arc<AtomicBool>,
    sink: Arc<Mutex<Option<Sink>>>,
}

impl FakeInput {
    fn at(rate: u32) -> Arc<Self> {
        Arc::new(Self {
            rate,
            refuse: AtomicBool::new(false),
            opens: AtomicU32::new(0),
            closes: Arc::new(AtomicU32::new(0)),
            failed: Arc::new(AtomicBool::new(false)),
            sink: Arc::new(Mutex::new(None)),
        })
    }

    fn open(&self) -> bool {
        lock(&self.sink).is_some()
    }

    /// What the capture's worker does with a block from the driver.
    fn feed(&self, samples: &[f32]) {
        let mut sink = lock(&self.sink);
        (sink.as_mut().expect("the input is open"))(samples).unwrap();
    }
}

struct FakeCapture {
    closes: Arc<AtomicU32>,
    failed: Arc<AtomicBool>,
    sink: Arc<Mutex<Option<Sink>>>,
}

impl CaptureHandle for FakeCapture {
    fn frames(&self) -> u64 {
        0
    }
    fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
    fn finish(self: Box<Self>) -> Result<u64, String> {
        if self.failed.load(Ordering::Acquire) {
            Err("Synthetic input loss".into())
        } else {
            Ok(0)
        }
    }
}

impl Drop for FakeCapture {
    fn drop(&mut self) {
        lock(&self.sink).take();
        self.closes.fetch_add(1, Ordering::AcqRel);
    }
}

impl InputBackend for FakeInput {
    fn rate(&self, _: &AudioLoggerInput, preferred: u32) -> Result<u32, String> {
        assert_eq!(preferred, SAMPLE_RATE, "the output's rate is asked for");
        Ok(self.rate)
    }

    fn open(
        &self,
        input: &AudioLoggerInput,
        rate: u32,
        sink: Sink,
    ) -> Result<Box<dyn CaptureHandle>, String> {
        assert_eq!((input.device.as_str(), rate), ("Line in", self.rate));
        assert!(!self.open(), "the device is opened once at a time");
        if self.refuse.load(Ordering::Acquire) {
            return Err("Synthetic refusal to open".into());
        }
        self.opens.fetch_add(1, Ordering::AcqRel);
        self.failed.store(false, Ordering::Release);
        *lock(&self.sink) = Some(sink);
        Ok(Box::new(FakeCapture {
            closes: self.closes.clone(),
            failed: self.failed.clone(),
            sink: self.sink.clone(),
        }))
    }
}

fn line_in() -> AudioLoggerInput {
    AudioLoggerInput {
        host: "Fake".into(),
        device: "Line in".into(),
        left: 0,
        right: Some(1),
    }
}

/// A rig that logs the master and a fake input captured at `rate`.
fn logging_input(rate: u32) -> (Rig, Arc<FakeInput>) {
    let rig = Rig::new();
    let input = FakeInput::at(rate);
    rig.session.audio_logger_set_backend(input.clone());
    rig.session
        .audio_logger_configure(AudioLoggerSettings {
            input: Some(line_in()),
            ..settings()
        })
        .unwrap();
    (rig, input)
}

/// Interleaved stereo in which every sample has a value of its own.
fn ramp(from: usize, frames: usize) -> Vec<f32> {
    (from * 2..(from + frames) * 2)
        .map(|index| index as f32 / 65_536.0)
        .collect()
}

fn recording_source() -> RecordingSource {
    RecordingSource {
        host: "Fake".into(),
        device: "Line in".into(),
        left: 0,
        right: None,
        armed_tracks: None,
        mixer_tap: None,
        alignment: None,
        loop_recording: None,
        monitor: None,
    }
}

struct TakeCapture;

impl CaptureHandle for TakeCapture {
    fn frames(&self) -> u64 {
        480
    }
    fn failed(&self) -> bool {
        false
    }
    fn finish(self: Box<Self>) -> Result<u64, String> {
        Ok(480)
    }
}

#[test]
fn the_last_seconds_of_the_master_become_a_clip_of_exactly_what_was_played() {
    let mut rig = logging(settings());
    let state = rig.session.audio_logger_state();
    assert!(state.running && state.error.is_none());
    assert_eq!(status(&state, MASTER).sample_rate, SAMPLE_RATE);
    assert_eq!(status(&state, MASTER).capacity_seconds, 30.0);
    assert_eq!(status(&state, MASTER).available_frames, 0);

    let mut played = play(&mut rig, 60_000, &[0, 1, 2, 3]);
    assert!(rms(&played) > 0.005, "the kit sounds");
    let before = rig.project();
    let result = rig
        .session
        .audio_logger_dump(AudioLoggerDumpRequest {
            source: MASTER,
            seconds: Some(0.5),
            start: Some(1_920),
            track: None,
        })
        .unwrap();
    assert!(!result.created.is_empty());

    // The file is the last half second, sample for sample.
    let file = last_file(&rig);
    assert!(file.starts_with(recordings(&rig)), "{}", file.display());
    assert_eq!(logged_files(&rig).len(), 1);
    let (rate, written) = decode(&file);
    assert_eq!(rate, SAMPLE_RATE);
    assert_eq!(written.len(), 48_000);
    assert!(written == played[played.len() - 48_000..]);

    // An ordinary audio clip of that sample, where it was asked for, on a
    // playlist track made for it.
    let project = rig.project();
    let clip = project.playlist.clips.last().unwrap();
    assert_eq!(clip.start, 1_920);
    assert_eq!(
        project.playlist.clips.len(),
        before.playlist.clips.len() + 1
    );
    assert_eq!(
        project.playlist.tracks.len(),
        before.playlist.tracks.len() + 1
    );
    assert_eq!(clip.track, project.playlist.tracks.last().unwrap().id);
    let ClipContent::Audio { sample, .. } = clip.content else {
        panic!("the dump made {:?}", clip.content);
    };
    assert_eq!(sample, project.samples.last().unwrap().id);
    assert!(rig.has_audio(sample));
    assert_eq!(
        rig.session.sample_info_by_id(sample).unwrap().frames,
        24_000
    );

    // The logger was not stopped: it goes on from where it was, and a
    // second dump, of everything, lands on the track that was named.
    assert_eq!(held(&rig, MASTER), 60_000);
    played.extend(play(&mut rig, 30_000, &[2, 0]));
    assert_eq!(held(&rig, MASTER), 90_000);
    assert_eq!(
        status(&rig.session.audio_logger_state(), MASTER).dropped_frames,
        0
    );
    let track = project.playlist.tracks[0].id;
    rig.session
        .audio_logger_dump(AudioLoggerDumpRequest {
            source: MASTER,
            seconds: None,
            start: Some(0),
            track: Some(track),
        })
        .unwrap();
    let all = decode(&last_file(&rig)).1;
    assert!(all == played);
    // Clips are kept in song order, so the new one is found by where it is.
    let clips = project.playlist.clips.len();
    let project = rig.project();
    assert_eq!(project.playlist.clips.len(), clips + 1);
    assert!(project
        .playlist
        .clips
        .iter()
        .any(|clip| (clip.start, clip.track) == (0, track)));
    assert_eq!(logged_files(&rig).len(), 2);

    // More seconds than there are gives all there is.
    rig.session
        .audio_logger_dump(dump(MASTER, Some(600.0)))
        .unwrap();
    assert!(decode(&last_file(&rig)).1 == played);
}

#[test]
fn a_dump_is_one_undo_step_and_is_found_again_after_save_and_reopen() {
    let mut rig = logging(settings());
    let played = play(&mut rig, 24_000, &[0, 3]);
    let mut before = rig.project();
    rig.session.audio_logger_dump(dump(MASTER, None)).unwrap();
    let logged = rig.project();
    let sample = logged.samples.last().unwrap().id;
    assert_eq!(logged.samples.len(), before.samples.len() + 1);
    let history = rig.session.document_snapshot().history;
    assert_eq!(history.entries.len(), 1, "{:?}", history.entries);

    // One undo takes the clip, its tracks and the sample away again.
    rig.session.undo().unwrap();
    before.next_id = logged.next_id;
    assert_eq!(rig.project(), before);
    assert!(last_file_exists(&logged), "undo keeps the file for redo");
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), logged);
    assert!(rig.has_audio(sample));

    // Saved, and opened by another run of the app: the sample is loaded by
    // the ordinary sample pipeline, and it is the logged audio.
    let path = rig.file("logged.windfall");
    rig.session.project_save(Some(&path)).unwrap();
    let rig = rig.restart();
    rig.session.project_open(&path).unwrap();
    assert_eq!(rig.project().playlist, logged.playlist);
    let reopened = rig.project();
    let sample = reopened.samples.last().unwrap();
    rig.wait_until_loaded_or_failed(sample.id);
    assert!(rig.has_audio(sample.id));
    let info = rig.session.sample_info_by_id(sample.id).unwrap();
    assert_eq!((info.frames, info.sample_rate), (24_000, SAMPLE_RATE));
    let state = rig.session.state();
    assert!(state.pool.get(sample.id).unwrap().samples() == played);
}

fn last_file_exists(project: &windfall_project::Project) -> bool {
    match &project.samples.last().unwrap().path {
        SamplePath::External(path) => Path::new(path).is_file(),
        _ => false,
    }
}

#[test]
fn a_logged_mixer_track_is_dumped_as_it_left_its_effects() {
    let mut rig = Rig::new();
    let project = rig.project();
    let kick = project.channels[0].mixer_track;
    let hat = project.channels[2].mixer_track;
    assert_ne!(kick, TrackId::MASTER);
    let state = rig
        .session
        .audio_logger_configure(AudioLoggerSettings {
            tracks: vec![kick, hat, TrackId(9_999)],
            ..settings()
        })
        .unwrap();
    assert_eq!(state.sources.len(), 4);
    let missing = status(
        &state,
        AudioLoggerSource::Track {
            track: TrackId(9_999),
        },
    );
    assert!(!missing.active && missing.label.contains("not in this project"));
    let named = status(&state, AudioLoggerSource::Track { track: kick });
    assert!(
        named.active && named.label.contains("Kick Punch"),
        "{named:?}"
    );

    // Only the kick plays, into a track at unity, so the master is what
    // left that track; the hat's track is logged too and stays silent.
    let played = play(&mut rig, 36_000, &[0]);
    assert!(rms(&played) > 0.005);
    rig.session
        .audio_logger_dump(dump(AudioLoggerSource::Track { track: kick }, Some(0.5)))
        .unwrap();
    let (rate, written) = decode(&last_file(&rig));
    assert_eq!(rate, SAMPLE_RATE);
    assert!(written == played[played.len() - 48_000..]);
    assert!(logged_files(&rig)[0].starts_with(&format!("Logged-track-{}-", kick.0)));

    rig.session
        .audio_logger_dump(dump(AudioLoggerSource::Track { track: hat }, None))
        .unwrap();
    let silent = decode(&last_file(&rig)).1;
    assert_eq!(silent.len(), 72_000);
    assert_eq!(rms(&silent), 0.0);

    // A track the project does not have was never heard, and one that is
    // not logged cannot be dumped.
    let before = rig.project();
    let missing = dump(
        AudioLoggerSource::Track {
            track: TrackId(9_999),
        },
        None,
    );
    let error = rig.session.audio_logger_dump(missing).unwrap_err();
    assert!(
        error.contains("not heard anything from that mixer track"),
        "{error}"
    );
    let snare = dump(
        AudioLoggerSource::Track {
            track: project.channels[3].mixer_track,
        },
        None,
    );
    let error = rig.session.audio_logger_dump(snare).unwrap_err();
    assert!(error.contains("not logging that source"), "{error}");
    assert_eq!(rig.project(), before);
    assert_eq!(logged_files(&rig).len(), 2);
}

#[test]
fn a_dump_is_refused_while_recording_for_an_empty_history_and_for_bad_requests() {
    let mut rig = logging(settings());
    let before = rig.project();

    // Nothing was played yet: there is nothing to put on the playlist.
    let error = rig
        .session
        .audio_logger_dump(dump(MASTER, None))
        .unwrap_err();
    assert!(
        error.contains("not heard anything from the master output"),
        "{error}"
    );
    let error = rig
        .session
        .audio_logger_dump(dump(INPUT, None))
        .unwrap_err();
    assert!(error.contains("not logging an audio input"), "{error}");

    play(&mut rig, 12_000, &[0]);
    for seconds in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let error = rig
            .session
            .audio_logger_dump(dump(MASTER, Some(seconds)))
            .unwrap_err();
        assert!(error.contains("how many seconds"), "{error}");
    }
    let gone = AudioLoggerDumpRequest {
        track: Some(PlaylistTrackId(9_999)),
        ..dump(MASTER, None)
    };
    let error = rig.session.audio_logger_dump(gone).unwrap_err();
    assert_eq!(error, "Playlist track no longer exists.");
    let outside = AudioLoggerDumpRequest {
        start: Some(u32::MAX),
        ..dump(MASTER, None)
    };
    let error = rig.session.audio_logger_dump(outside).unwrap_err();
    assert!(error.contains("outside the song"), "{error}");

    // While a take is being recorded the playlist is the take's.
    rig.session
        .recording_start_with(recording_source(), 0, None, |_, _, mut sink| {
            sink(&[0.25; 960])?;
            Ok(Box::new(TakeCapture))
        })
        .unwrap();
    let error = rig
        .session
        .audio_logger_dump(dump(MASTER, None))
        .unwrap_err();
    assert_eq!(error, "Stop or cancel recording first.");
    // The logger itself goes on through the take.
    play(&mut rig, 12_000, &[1]);
    assert_eq!(held(&rig, MASTER), 24_000);
    rig.session.recording_cancel();

    assert_eq!(rig.project(), before);
    assert!(logged_files(&rig).is_empty(), "{:?}", logged_files(&rig));
    assert!(rig.session.document_snapshot().history.entries.is_empty());
    rig.session.audio_logger_dump(dump(MASTER, None)).unwrap();
    assert_eq!(logged_files(&rig).len(), 1);
}

#[test]
fn a_project_opened_during_a_dump_is_left_alone_and_the_file_is_removed() {
    let mut rig = logging(settings());
    play(&mut rig, 12_000, &[0]);
    let hold = rig.session.hold("audio-logger:written");
    let dumping = rig
        .session
        .background(|session| session.audio_logger_dump(dump(MASTER, None)));
    hold.wait();
    assert_eq!(logged_files(&rig).len(), 1, "the audio is written");
    rig.session.project_new().unwrap();
    let fresh = rig.session.document_snapshot();
    hold.release();

    let error = dumping.join().unwrap().unwrap_err();
    assert!(error.contains("another project was opened"), "{error}");
    let now = rig.session.document_snapshot();
    assert_eq!(now.project, fresh.project);
    assert_eq!(now.revision, fresh.revision);
    assert!(now.history.entries.is_empty() && !now.dirty);
    assert!(logged_files(&rig).is_empty(), "{:?}", logged_files(&rig));

    // The history was not used up by the refused dump.
    rig.session.audio_logger_dump(dump(MASTER, None)).unwrap();
    assert_eq!(decode(&last_file(&rig)).1.len(), 24_000);
}

#[test]
fn a_logged_input_keeps_its_own_rate_and_is_dumped_exactly() {
    let (mut rig, input) = logging_input(44_100);
    assert!(input.open());
    let state = rig.session.audio_logger_state();
    let logged = status(&state, INPUT);
    assert!(logged.active && logged.error.is_none());
    assert_eq!(logged.sample_rate, 44_100, "not relabelled as the output's");
    assert_eq!(logged.capacity_seconds, 30.0);
    assert_eq!(logged.label, "Input: Line in");
    assert_eq!(status(&state, MASTER).sample_rate, SAMPLE_RATE);

    // The input is logged with no recording running and nothing playing.
    input.feed(&ramp(0, 4_410));
    input.feed(&ramp(4_410, 6_615));
    assert_eq!(held(&rig, INPUT), 11_025);
    play(&mut rig, 12_000, &[0]);
    rig.session
        .audio_logger_dump(dump(INPUT, Some(0.1)))
        .unwrap();
    let (rate, written) = decode(&last_file(&rig));
    assert_eq!(rate, 44_100);
    assert!(written == ramp(6_615, 4_410));
    assert!(logged_files(&rig)[0].starts_with("Logged-input-"));
    rig.session.audio_logger_dump(dump(INPUT, None)).unwrap();
    assert!(decode(&last_file(&rig)).1 == ramp(0, 11_025));

    // Past its length the oldest input audio is overwritten.
    for block in 0..140 {
        input.feed(&ramp(11_025 + block * 9_600, 9_600));
    }
    let total = 11_025 + 140 * 9_600;
    assert_eq!(held(&rig, INPUT), 44_100 * 30);
    rig.session
        .audio_logger_dump(dump(INPUT, Some(1.0)))
        .unwrap();
    assert!(decode(&last_file(&rig)).1 == ramp(total - 44_100, 44_100));
    assert_eq!(held(&rig, MASTER), 12_000, "the master history is its own");
}

#[test]
fn an_input_that_fails_or_cannot_open_is_an_error_state_and_nothing_else_stops() {
    let (mut rig, input) = logging_input(48_000);
    input.feed(&ramp(0, 4_800));
    play(&mut rig, 12_000, &[0]);

    // The device is lost: the capture reports it and the logger shows it.
    input.failed.store(true, Ordering::Release);
    rig.session.audio_logger_maintain();
    assert!(!input.open(), "the failed capture is closed");
    let state = rig.session.audio_logger_state();
    let logged = status(&state, INPUT);
    assert_eq!(logged.error.as_deref(), Some("Synthetic input loss"));
    assert!(!logged.active);
    assert_eq!(logged.available_frames, 4_800, "what it heard is kept");
    // The output side neither stopped nor failed.
    assert!(state.running && state.error.is_none());
    assert!(status(&state, MASTER).active);
    let played = play(&mut rig, 12_000, &[1]);
    assert!(rms(&played) > 0.001);
    assert_eq!(held(&rig, MASTER), 24_000);
    // A failed input is not reopened behind the user's back.
    rig.session.audio_logger_maintain();
    assert_eq!(input.opens.load(Ordering::Acquire), 1);
    rig.session.audio_logger_dump(dump(INPUT, None)).unwrap();
    assert!(decode(&last_file(&rig)).1 == ramp(0, 4_800));

    // An input that cannot be opened is reported the same way.
    input.refuse.store(true, Ordering::Release);
    let other = AudioLoggerSettings {
        input: Some(AudioLoggerInput {
            left: 1,
            ..line_in()
        }),
        ..settings()
    };
    let state = rig.session.audio_logger_configure(other.clone()).unwrap();
    let logged = status(&state, INPUT);
    assert_eq!(logged.error.as_deref(), Some("Synthetic refusal to open"));
    assert!(!logged.active);
    assert_eq!(status(&state, MASTER).available_frames, 24_000);
    assert!(state.running);

    // Configuring again tries again.
    input.refuse.store(false, Ordering::Release);
    let state = rig
        .session
        .audio_logger_configure(AudioLoggerSettings {
            input: Some(line_in()),
            ..settings()
        })
        .unwrap();
    assert!(status(&state, INPUT).active && status(&state, INPUT).error.is_none());
    assert_eq!(input.opens.load(Ordering::Acquire), 2);

    // Without an input the capture is closed and its row is gone.
    let state = rig.session.audio_logger_configure(settings()).unwrap();
    assert!(!input.open());
    assert!(state.sources.iter().all(|status| status.source != INPUT));
}

#[test]
fn the_logged_input_makes_way_for_a_recording_take_and_comes_back() {
    let (rig, input) = logging_input(48_000);
    input.feed(&ramp(0, 2_400));
    assert_eq!(input.opens.load(Ordering::Acquire), 1);

    // The take opens the device itself, and finds it closed.
    let seen = input.clone();
    rig.session
        .recording_start_with(recording_source(), 0, None, move |_, _, mut sink| {
            assert!(!seen.open(), "the logger still holds the device");
            assert_eq!(seen.closes.load(Ordering::Acquire), 1);
            sink(&[0.25; 960])?;
            Ok(Box::new(TakeCapture))
        })
        .unwrap();
    assert!(rig.session.recording_state().active);
    let state = rig.session.audio_logger_state();
    let logged = status(&state, INPUT);
    assert!(!logged.active);
    assert!(logged
        .error
        .as_deref()
        .unwrap()
        .contains("Paused while a recording take"));
    assert_eq!(logged.available_frames, 2_400);

    // It stays closed for as long as the take lasts, whatever is asked.
    rig.session.audio_logger_maintain();
    rig.session
        .audio_logger_configure(AudioLoggerSettings {
            seconds: 60,
            input: Some(line_in()),
            ..settings()
        })
        .unwrap();
    rig.session.audio_logger_maintain();
    assert!(!input.open());
    assert_eq!(input.opens.load(Ordering::Acquire), 1);
    rig.session
        .audio_logger_configure(AudioLoggerSettings {
            input: Some(line_in()),
            ..settings()
        })
        .unwrap();

    // The take is kept; the recording worked.
    let result = rig.session.recording_stop().unwrap();
    assert!(!result.created.is_empty());
    assert!(!rig.session.recording_state().active);

    // And the input is logged again.
    rig.session.audio_logger_maintain();
    assert!(input.open());
    assert_eq!(input.opens.load(Ordering::Acquire), 2);
    input.feed(&ramp(100, 1_200));
    let state = rig.session.audio_logger_state();
    let logged = status(&state, INPUT);
    assert!(logged.active && logged.error.is_none());
    assert_eq!(logged.available_frames, 1_200);
}

#[test]
fn an_input_paused_for_a_take_that_was_cancelled_continues_its_history() {
    let (rig, input) = logging_input(48_000);
    input.feed(&ramp(0, 2_400));
    rig.session
        .recording_start_with(recording_source(), 0, None, |_, _, _| {
            Ok(Box::new(TakeCapture))
        })
        .unwrap();
    rig.session.recording_cancel();
    assert!(!input.open());
    rig.session.audio_logger_maintain();
    assert!(input.open());
    input.feed(&ramp(2_400, 2_400));
    assert_eq!(held(&rig, INPUT), 4_800);
    rig.session.audio_logger_dump(dump(INPUT, None)).unwrap();
    assert!(decode(&last_file(&rig)).1 == ramp(0, 4_800));

    // A take that could not even start leaves the input to come back too.
    let refused = rig
        .session
        .recording_start_with(recording_source(), 0, None, |_, _, _| {
            Err("Failed input open".into())
        });
    assert!(refused.is_err());
    assert!(!input.open());
    rig.session.audio_logger_maintain();
    assert!(input.open());
    assert_eq!(held(&rig, INPUT), 4_800);
}

#[test]
fn changing_the_length_or_the_sources_starts_over_and_disabling_stops() {
    let mut rig = logging(settings());
    play(&mut rig, 12_000, &[0]);
    assert_eq!(held(&rig, MASTER), 12_000);
    let memory = rig.session.audio_logger_state().memory_bytes;
    assert_eq!(memory, (30 + 2) * u64::from(SAMPLE_RATE) * 8);

    // The same settings again change nothing and lose nothing.
    rig.session.audio_logger_configure(settings()).unwrap();
    assert_eq!(held(&rig, MASTER), 12_000);

    // A longer history is another allocation: it starts empty.
    let longer = AudioLoggerSettings {
        seconds: 60,
        ..settings()
    };
    let state = rig.session.audio_logger_configure(longer).unwrap();
    assert_eq!(status(&state, MASTER).available_frames, 0);
    assert_eq!(status(&state, MASTER).capacity_seconds, 60.0);
    assert_eq!(state.memory_bytes, (60 + 2) * u64::from(SAMPLE_RATE) * 8);
    play(&mut rig, 12_000, &[0]);
    assert_eq!(held(&rig, MASTER), 12_000);

    // Disabled: nothing is held and nothing more is logged.
    let off = AudioLoggerSettings {
        enabled: false,
        ..settings()
    };
    let state = rig.session.audio_logger_configure(off).unwrap();
    assert!(!state.running && state.sources.is_empty());
    assert_eq!(state.memory_bytes, 0);
    play(&mut rig, 12_000, &[0]);
    let error = rig
        .session
        .audio_logger_dump(dump(MASTER, None))
        .unwrap_err();
    assert_eq!(error, "The audio logger is not running.");
    rig.session.audio_logger_maintain();
    assert!(!rig.session.audio_logger_state().running);

    // Enabled again it runs, with nothing from before.
    let state = rig.session.audio_logger_configure(settings()).unwrap();
    assert!(state.running);
    assert_eq!(status(&state, MASTER).available_frames, 0);

    // Settings outside the bounds are refused and not remembered.
    for bad in [
        AudioLoggerSettings {
            seconds: 29,
            ..settings()
        },
        AudioLoggerSettings {
            seconds: 601,
            ..settings()
        },
        AudioLoggerSettings {
            tracks: (1..=5).map(TrackId).collect(),
            ..settings()
        },
        AudioLoggerSettings {
            tracks: vec![TrackId(3), TrackId(3)],
            ..settings()
        },
    ] {
        assert!(rig.session.audio_logger_configure(bad).is_err());
    }
    assert_eq!(rig.session.audio_logger_state().settings, settings());
}

#[test]
fn reopening_the_audio_device_clears_the_history_and_logging_goes_on() {
    let (mut rig, input) = logging_input(48_000);
    play(&mut rig, 24_000, &[0]);
    input.feed(&ramp(0, 2_400));
    assert_eq!(held(&rig, MASTER), 24_000);

    let status_now = rig.session.engine_configure(AudioSettings {
        buffer_frames: Some(256),
        ..AudioSettings::default()
    });
    assert!(status_now.running);
    let state = rig.session.audio_logger_state();
    assert!(state.running && state.error.is_none());
    assert_eq!(status(&state, MASTER).available_frames, 0);
    assert_eq!(status(&state, INPUT).available_frames, 0);
    assert_eq!(status(&state, MASTER).sample_rate, SAMPLE_RATE);
    // The input was closed and opened again, once.
    assert_eq!(input.opens.load(Ordering::Acquire), 2);
    assert_eq!(input.closes.load(Ordering::Acquire), 1);

    let played = play(&mut rig, 12_000, &[1]);
    assert_eq!(held(&rig, MASTER), 12_000);
    rig.session.audio_logger_dump(dump(MASTER, None)).unwrap();
    assert!(decode(&last_file(&rig)).1 == played);
    // Looking after a healthy logger changes nothing.
    rig.session.audio_logger_maintain();
    assert_eq!(held(&rig, MASTER), 12_000);
    assert_eq!(input.opens.load(Ordering::Acquire), 2);
}

#[test]
fn logger_settings_are_remembered_and_an_older_settings_file_gives_the_defaults() {
    // A settings file from before the logger existed.
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join(SETTINGS_FILE),
        r#"{ "audio": { "bufferFrames": 512 }, "browserRoots": [], "recentProjects": [] }"#,
    )
    .unwrap();
    let rig = Rig::in_folder(folder);
    assert_eq!(rig.session.engine_settings().buffer_frames, Some(512));
    let state = rig.session.audio_logger_state();
    assert_eq!(state.settings, AudioLoggerSettings::default());
    assert!(state.settings.enabled && state.settings.master);
    assert_eq!(state.settings.seconds, 120);
    assert!(state.settings.tracks.is_empty() && state.settings.input.is_none());
    assert_eq!(
        (state.min_seconds, state.max_seconds, state.max_tracks),
        (30, 600, 4)
    );
    // Nothing runs until the app starts the logger.
    assert!(!state.running);
    rig.session.audio_logger_restore();
    let state = rig.session.audio_logger_state();
    assert!(state.running);
    assert_eq!(status(&state, MASTER).capacity_seconds, 120.0);

    let chosen = AudioLoggerSettings {
        enabled: true,
        seconds: 300,
        master: false,
        tracks: vec![TrackId(2), TrackId(5)],
        input: None,
    };
    rig.session.audio_logger_configure(chosen.clone()).unwrap();
    let json = std::fs::read_to_string(rig.folder.path().join(SETTINGS_FILE)).unwrap();
    assert!(json.contains("\"audioLogger\""), "{json}");
    assert!(json.contains("\"bufferFrames\": 512"), "{json}");

    // The next run starts the logger as it was left.
    let rig = rig.restart();
    assert_eq!(rig.session.audio_logger_state().settings, chosen);
    rig.session.audio_logger_restore();
    let state = rig.session.audio_logger_state();
    assert!(state.running);
    assert_eq!(state.sources.len(), 2);
    assert!(state
        .sources
        .iter()
        .all(|status| status.capacity_seconds == 300.0));

    // A file edited by hand to hold more than the logger takes is cut to
    // what it can run, not trusted and not thrown away.
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join(SETTINGS_FILE),
        r#"{ "audioLogger": { "seconds": 86400, "tracks": [1, 1, 2, 3, 4, 5, 6] } }"#,
    )
    .unwrap();
    let rig = Rig::in_folder(folder);
    let settings = rig.session.audio_logger_state().settings;
    assert_eq!(settings.seconds, 600);
    assert_eq!(settings.tracks, [1, 2, 3, 4].map(TrackId));
    assert!(settings.enabled && settings.master);
}
