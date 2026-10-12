//! Export: what is refused, what runs, and what it writes.

use std::path::Path;
use std::sync::mpsc;

use windfall_ipc::{BitDepth, ExportFormat, ExportOptions, PlayMode, TransportPatch};
use windfall_project::{ClipContent, ClipInit, Command, PatternId, PlaylistTrackId};

use super::{Rig, SAMPLE_RATE, rms};
use crate::events::Event;
use crate::session::ClipPlace;
use crate::sync::lock;

fn options(rig: &Rig, name: &str) -> ExportOptions {
    ExportOptions {
        path: rig.file(name),
        format: ExportFormat::Wav,
        bit_depth: BitDepth::Float32,
        sample_rate: SAMPLE_RATE,
        mode: PlayMode::Pattern,
        pattern_loops: 1,
        tail_secs: 0.0,
        auto_tail: false,
        ..ExportOptions::default()
    }
}

fn kick_on_the_beat(rig: &Rig) {
    for step in [0, 4, 8, 12] {
        rig.session
            .dispatch(
                Command::ToggleStep {
                    pattern: rig.pattern(),
                    channel: rig.channel(0),
                    step,
                },
                None,
            )
            .unwrap();
    }
}

#[test]
fn an_export_that_cannot_work_is_refused_before_it_starts() {
    let rig = Rig::new();
    let session = &rig.session;
    let refused = |change: &dyn Fn(&mut ExportOptions)| {
        let mut options = options(&rig, "refused.wav");
        change(&mut options);
        session.export_audio(options).unwrap_err()
    };

    assert_eq!(
        refused(&|o| o.path = String::new()),
        "Choose where to save the file."
    );
    assert_eq!(
        refused(&|o| o.path = "beat.wav".to_owned()),
        "\"beat.wav\" is not a full path"
    );
    assert_eq!(
        refused(&|o| o.pattern_loops = 0),
        "Render the pattern at least once."
    );
    assert_eq!(
        refused(&|o| o.sample_rate = 12),
        "12 Hz is not a sample rate Windfall can export."
    );
    assert_eq!(
        refused(&|o| o.tail_secs = f32::NAN),
        "The tail must be zero seconds or longer."
    );
    assert_eq!(
        refused(&|o| o.mode = PlayMode::Song),
        "The playlist is empty, so there is no song to export."
    );
    // A hundred thousand bars would need tens of gigabytes of memory.
    let error = refused(&|o| o.pattern_loops = 100_000);
    assert!(error.contains("minutes long"), "{error}");

    assert!(rig.events.take().is_empty());
    assert!(!Path::new(&rig.file("refused.wav")).exists());
    // None of the refusals left the session thinking an export is running.
    session.export_audio(options(&rig, "fine.wav")).unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
}

#[test]
fn a_second_export_is_refused_while_one_runs_and_edits_do_not_reach_it() {
    let rig = Rig::new();
    let session = &rig.session;
    kick_on_the_beat(&rig);

    // Holds the export at its first progress event until the test lets go.
    let (reached, wait_reached) = mpsc::channel();
    let (release, wait_release) = mpsc::channel::<()>();
    let mut held = false;
    *lock(&rig.events.hook) = Some(Box::new(move |event| {
        if matches!(event, Event::ExportProgress(_)) && !held {
            held = true;
            reached.send(()).unwrap();
            wait_release.recv().unwrap();
        }
    }));

    let first = options(&rig, "first.wav");
    session.export_audio(first.clone()).unwrap();
    wait_reached.recv().unwrap();

    assert_eq!(
        session
            .export_audio(options(&rig, "second.wav"))
            .unwrap_err(),
        "An export is already running."
    );

    // Editing while the export runs is safe: it has its own copy.
    session
        .dispatch(
            Command::ClearLane {
                pattern: rig.pattern(),
                channel: rig.channel(0),
            },
            None,
        )
        .unwrap();
    session.project_new().unwrap();

    release.send(()).unwrap();
    let done = rig.events.wait_for_export();
    assert_eq!(done.error, None);
    assert_eq!(done.path, first.path);
    assert!(!Path::new(&rig.file("second.wav")).exists());
    let audio = windfall_codec::decode_file(&first.path).unwrap();
    assert!(rms(audio.samples()) > 0.02, "the edit reached the export");

    // And now another may run.
    rig.events.take();
    session.export_audio(options(&rig, "second.wav")).unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
    assert!(Path::new(&rig.file("second.wav")).is_file());
}

#[test]
fn an_export_that_fails_says_why_in_its_last_event() {
    let rig = Rig::new();
    let session = &rig.session;
    let nowhere = options(&rig, "no such folder/beat.wav");

    session.export_audio(nowhere.clone()).unwrap();
    let done = rig.events.wait_for_export();
    assert_eq!(done.path, nowhere.path);
    let error = done.error.unwrap();
    assert!(error.starts_with("Could not export to \""), "{error}");
    assert!(error.contains("beat.wav"), "{error}");

    rig.events.take();
    session.export_audio(options(&rig, "after.wav")).unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
}

#[test]
fn pattern_mode_exports_the_pattern_the_transport_has_selected() {
    let rig = Rig::new();
    let session = &rig.session;
    kick_on_the_beat(&rig);
    let empty = session
        .dispatch(Command::AddPattern { name: None }, None)
        .unwrap()
        .created[0];
    let export = |name: &str| {
        rig.events.take();
        let options = ExportOptions {
            sample_rate: 44_100,
            ..options(&rig, name)
        };
        session.export_audio(options.clone()).unwrap();
        assert_eq!(rig.events.wait_for_export().error, None);
        windfall_codec::decode_file(&options.path).unwrap()
    };

    let beat = export("beat.wav");
    // One bar at 120 bpm, at the rate asked for and not the engine's.
    assert_eq!((beat.sample_rate(), beat.frames()), (44_100, 88_200));
    assert!(rms(beat.samples()) > 0.02);

    session
        .transport_set(TransportPatch {
            pattern: Some(PatternId(empty)),
            ..TransportPatch::default()
        })
        .unwrap();
    let silence = export("silence.wav");
    assert_eq!(silence.frames(), 88_200);
    assert_eq!(rms(silence.samples()), 0.0);
}

#[test]
fn song_mode_exports_the_playlist_to_the_end_of_its_last_clip() {
    let rig = Rig::new();
    let session = &rig.session;
    kick_on_the_beat(&rig);
    let track = session
        .dispatch(
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0];
    let bar = rig.project().patterns[0].length_ticks();
    session
        .dispatch(
            Command::AddClips {
                clips: vec![ClipInit {
                    track: PlaylistTrackId(track),
                    // One silent bar, then three bars of the pattern.
                    start: bar,
                    length: Some(bar * 3),
                    offset: None,
                    muted: None,
                    content: ClipContent::Pattern {
                        pattern: rig.pattern(),
                    },
                }],
            },
            None,
        )
        .unwrap();

    let options = ExportOptions {
        mode: PlayMode::Song,
        // Loops are a pattern-mode setting and are ignored for a song.
        pattern_loops: 0,
        tail_secs: 0.25,
        bit_depth: BitDepth::Int16,
        ..options(&rig, "song.wav")
    };
    session.export_audio(options.clone()).unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);

    let audio = windfall_codec::decode_file(&options.path).unwrap();
    let bar_frames = SAMPLE_RATE as usize * 2;
    assert_eq!(audio.frames(), bar_frames * 4 + SAMPLE_RATE as usize / 4);
    let bars = audio.samples().chunks(bar_frames * 2).collect::<Vec<_>>();
    // 16-bit silence still carries dither.
    assert!(rms(bars[0]) < 1e-4, "the first bar should be silent");
    for bar in &bars[1..4] {
        assert!(rms(bar) > 0.02);
    }
    // Exporting does not move the transport.
    assert!(!session.transport_state().playing);
    assert_eq!(session.transport_state().mode, PlayMode::Pattern);
}

#[test]
fn an_export_named_without_an_extension_is_written_as_a_wav_file() {
    let rig = Rig::new();
    let session = &rig.session;
    kick_on_the_beat(&rig);

    for (typed, written) in [
        ("typed name", "typed name.wav"),
        ("take.2", "take.2.wav"),
        ("01. Intro", "01. Intro.wav"),
        ("Loud.WAV", "Loud.WAV"),
    ] {
        rig.events.take();
        let options = options(&rig, typed);
        session.export_audio(options.clone()).unwrap();
        let done = rig.events.wait_for_export();
        assert_eq!(done.error, None, "{typed}");
        // Progress is reported under the path as it was given.
        assert_eq!(done.path, options.path);
        let audio = windfall_codec::decode_file(rig.file(written)).unwrap();
        assert!(rms(audio.samples()) > 0.02, "{written}");
        if typed != written {
            assert!(!Path::new(&options.path).exists(), "{typed}");
        }
    }

    rig.events.take();
    assert_eq!(
        session.export_audio(options(&rig, "beat.mp3")).unwrap_err(),
        "The export is a WAV file, so its name cannot end in .mp3. End it in .wav, leave the ending off, or choose another format."
    );
    assert!(rig.events.take().is_empty());
    assert!(!Path::new(&rig.file("beat.mp3")).exists());
    assert!(!Path::new(&rig.file("beat.mp3.wav")).exists());
}

#[test]
fn an_export_says_how_many_audio_clips_it_had_no_room_for() {
    let rig = Rig::new();
    let session = &rig.session;
    let file = super::factory_file("Bass/Bass Sub.wav");
    let place = ClipPlace {
        track: None,
        start: 0,
        mixer_track: None,
    };
    let created = session.add_audio_clip_from_file(&file, place).unwrap();
    let first = rig
        .project()
        .playlist
        .clips
        .iter()
        .find(|clip| Some(&clip.id.0) == created.created.last())
        .cloned()
        .unwrap();
    let song = ExportOptions {
        mode: PlayMode::Song,
        ..options(&rig, "crowd.wav")
    };

    // One clip, and then 128 of it: everything is in the file.
    let copies = |count: usize| Command::AddClips {
        clips: vec![
            ClipInit {
                track: first.track,
                start: first.start,
                length: Some(first.length),
                offset: None,
                muted: None,
                content: first.content.clone(),
            };
            count
        ],
    };
    session.dispatch(copies(127), None).unwrap();
    session.export_audio(song.clone()).unwrap();
    let done = rig.events.wait_for_export();
    assert_eq!((done.error, done.dropped_clips), (None, 0));

    // Five more start on the same tick, and the engine plays 128 at once.
    // The file is written all the same, and the last event counts the
    // clips that are not in it. No event before it does.
    session.dispatch(copies(5), None).unwrap();
    rig.events.take();
    session.export_audio(song.clone()).unwrap();
    let done = rig.events.wait_for_export();
    assert_eq!((done.error, done.dropped_clips), (None, 5));
    for event in rig.events.take() {
        if let Event::ExportProgress(progress) = event {
            assert_eq!(progress.dropped_clips, if progress.done { 5 } else { 0 });
        }
    }
    assert!(Path::new(&song.path).is_file());
}

#[test]
fn song_wav_export_writes_named_cues_and_rebased_sampler_loops() {
    use windfall_project::{MarkerKind, TickRange};
    let rig = Rig::new();
    kick_on_the_beat(&rig);
    let lane = rig
        .session
        .dispatch(
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0];
    rig.session
        .dispatch(
            Command::AddClips {
                clips: vec![ClipInit {
                    track: PlaylistTrackId(lane),
                    start: 0,
                    length: Some(3840),
                    offset: None,
                    muted: None,
                    content: ClipContent::Pattern {
                        pattern: rig.pattern(),
                    },
                }],
            },
            None,
        )
        .unwrap();
    rig.session
        .dispatch(
            Command::AddTimelineMarker {
                tick: 960,
                name: "Original loop".into(),
                kind: MarkerKind::Loop { end: 2880 },
            },
            None,
        )
        .unwrap();
    rig.session
        .dispatch(
            Command::AddTimelineMarker {
                tick: 1920,
                name: "Original cue".into(),
                kind: MarkerKind::Named,
            },
            None,
        )
        .unwrap();
    let export = ExportOptions {
        mode: PlayMode::Song,
        region: Some(TickRange {
            start: 960,
            end: 3840,
        }),
        ..options(&rig, "own-markers.wav")
    };
    rig.session.export_audio(export.clone()).unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
    let metadata = windfall_codec::read_wav_metadata_file(&export.path).unwrap();
    assert_eq!(
        metadata
            .cues
            .iter()
            .map(|cue| (cue.frame, cue.label.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "Original loop"), (24000, "Original cue")]
    );
    assert_eq!((metadata.loops[0].start, metadata.loops[0].end), (0, 48000));
    assert_eq!(metadata.tempo.unwrap().beats, 3);
    assert_eq!(
        windfall_codec::decode_file(export.path).unwrap().frames(),
        72000
    );
}

#[test]
fn pattern_wav_note_cues_follow_swing_shift_repeats_and_saved_notes() {
    use windfall_project::{ChannelPatch, ChannelTiming, NoteInit, SettingsPatch};
    let rig = Rig::new();
    let c = rig.channel(0);
    rig.session.dispatch(Command::AddNotes {
        pattern: rig.pattern(), channel: c,
        notes: vec![NoteInit { start: 240, length: 240, key: 65, velocity: None, pan: None, expression: None }],
    }, None).unwrap();
    rig.session.dispatch(Command::UpdateSettings { patch: SettingsPatch {
        swing: Some(0.75), ..Default::default()
    } }, None).unwrap();
    rig.session.dispatch(Command::UpdateChannel { id: c, patch: ChannelPatch {
        timing: Some(ChannelTiming { swing_mix: 1.0, gate_ticks: 120, shift_ticks: 40 }),
        ..Default::default()
    } }, None).unwrap();
    let save = rig.file("note-markers.windfall");
    rig.session.project_save(Some(&save)).unwrap();
    rig.session.project_open(&save).unwrap();
    let before = rig.session.document_snapshot();
    let mut export = options(&rig, "pattern-cues.wav");
    export.pattern_loops = 2;
    rig.session.export_audio(export.clone()).unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
    let metadata = windfall_codec::read_wav_metadata_file(&export.path).unwrap();
    assert_eq!(metadata.cues.iter().map(|cue| cue.frame).collect::<Vec<_>>(), vec![8500, 104500]);
    assert_ne!(metadata.cues[0].id, metadata.cues[1].id);
    assert!(metadata.cues.iter().all(|cue| cue.label.ends_with("MIDI 65")));
    let decoded = windfall_codec::decode_file(&export.path).unwrap();
    assert_eq!(decoded.frames(), 192000);
    // Inspect the first cue without using Windfall's metadata reader.
    let bytes = std::fs::read(&export.path).unwrap();
    let mut at = 12;
    loop {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if &bytes[at..at + 4] == b"cue " {
            assert_eq!(u32::from_le_bytes(bytes[at + 32..at + 36].try_into().unwrap()), 8500);
            break;
        }
        at += 8 + size + size % 2;
        assert!(at < bytes.len(), "missing cue chunk");
    }
    assert_eq!(rig.session.document_snapshot(), before);
}
#[test]
fn song_wav_navigation_refusal_preserves_destination_and_a_region_before_navigation_exports() {
    use windfall_project::{MarkerKind, TickRange};
    let rig = Rig::new();
    let input = rig.file("navigation-source.wav");
    windfall_codec::write_wav(&input,
        &windfall_core::AudioBuffer::from_interleaved(48000, 1, vec![0.25; 48000]),
        windfall_codec::WavSampleFormat::Float32).unwrap();
    rig.session.add_audio_clip_from_file(&input, ClipPlace { start: 0, track: None, mixer_track: None }).unwrap();
    rig.session.dispatch(Command::AddTimelineMarker {
        tick: 960, name: "Skip source".into(), kind: MarkerKind::Skip { end: 1440 },
    }, None).unwrap();
    let before = rig.session.document_snapshot();
    let mut export = options(&rig, "navigation.wav");
    export.mode = PlayMode::Song;
    std::fs::write(&export.path, b"existing destination").unwrap();
    rig.session.export_audio(export.clone()).unwrap();
    assert!(rig.events.wait_for_export().error.unwrap().contains("skip or pause"));
    assert_eq!(std::fs::read(&export.path).unwrap(), b"existing destination");
    assert_eq!(rig.session.document_snapshot(), before);
    let guard = rig.session.timeline_state();
    export.path = rig.file("before-navigation.wav");
    export.region = Some(TickRange { start: 0, end: 480 });
    export.region_generation = Some(guard.generation);
    export.region_revision = Some(guard.revision);
    rig.session.export_audio(export.clone()).unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
    assert_eq!(windfall_codec::decode_file(&export.path).unwrap().frames(), 12000);
    assert!(windfall_codec::read_wav_metadata_file(&export.path).unwrap().cues.is_empty());
    assert_eq!(rig.session.document_snapshot(), before);
}