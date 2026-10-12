//! Native session acceptance sources: no window, codec or hardware required.
use super::Rig;
use std::time::Instant;
use windfall_engine::midi_hardware::{InputMessage, MidiMessage};
use windfall_project::{
    Command, ControlAction, ControlMap, ControlSource, ControllerLink, LaunchQuantization,
    LinkTarget, MidiControl, PerformancePad, StageMedia, VideoPlacement, TrackId,
};

fn seed(rig: &Rig) -> StageMedia {
    let media = StageMedia {
        quantization: LaunchQuantization::Immediate,
        pads: vec![
            PerformancePad {
                track: 0,
                scene: 0,
                pattern: rig.pattern(),
            },
            PerformancePad {
                track: 1,
                scene: 0,
                pattern: rig.pattern(),
            },
        ],
        ..Default::default()
    };
    rig.session
        .dispatch(
            Command::SetStageMedia {
                media: media.clone(),
            },
            None,
        )
        .unwrap();
    media
}

fn send(rig: &Rig, message: MidiMessage) -> bool {
    rig.session.controller_message(InputMessage {
        device: 0,
        message,
        at: Instant::now(),
        stamp_micros: 0,
    })
}

#[test]
fn midi_note_pad_rising_edges_launch_and_stop_via_controller_links_without_song_seek() {
    let mut rig = Rig::new();
    seed(&rig);
    let link = |key, action| ControllerLink {
        source: ControlSource::Midi {
            device: None,
            channel: Some(1),
            control: MidiControl::Note { key },
        },
        target: LinkTarget::Action { action },
        map: ControlMap::default(),
    };
    rig.session
        .dispatch(
            Command::SetControllerLinks {
                links: vec![
                    link(36, ControlAction::PerformancePad { track: 0, scene: 0 }),
                    link(37, ControlAction::PerformanceStopAll),
                ],
            },
            None,
        )
        .unwrap();
    rig.session.controllers_pump(Instant::now());
    rig.run(64);
    let playhead = rig.session.controller().frame().tick;
    assert!(send(
        &rig,
        MidiMessage::NoteOn {
            channel: 0,
            key: 36,
            velocity: 127
        }
    ));
    rig.session.controllers_pump(Instant::now());
    rig.run(64);
    assert_eq!(
        rig.session.performance_state().tracks[0].active,
        Some(rig.pattern())
    );
    assert!(!rig.session.controller().transport().playing);
    assert_eq!(rig.session.controller().frame().tick, playhead);
    // Stop via another mapped pad. Repeated held NoteOn must not relaunch.
    assert!(send(
        &rig,
        MidiMessage::NoteOn {
            channel: 0,
            key: 37,
            velocity: 127
        }
    ));
    rig.session.controllers_pump(Instant::now());
    rig.run(64);
    assert!(send(
        &rig,
        MidiMessage::NoteOn {
            channel: 0,
            key: 36,
            velocity: 100
        }
    ));
    rig.session.controllers_pump(Instant::now());
    rig.run(64);
    assert_eq!(rig.session.performance_state().tracks[0].active, None);
    assert!(send(
        &rig,
        MidiMessage::NoteOff {
            channel: 0,
            key: 36
        }
    ));
    rig.session.controllers_pump(Instant::now());
    assert!(send(
        &rig,
        MidiMessage::NoteOn {
            channel: 0,
            key: 36,
            velocity: 80
        }
    ));
    rig.session.controllers_pump(Instant::now());
    rig.run(64);
    assert_eq!(
        rig.session.performance_state().tracks[0].active,
        Some(rig.pattern())
    );
    rig.session.performance_stop_all();
    rig.run(48000);
    assert!(
        rig.session
            .performance_state()
            .tracks
            .iter()
            .all(|track| track.active.is_none())
    );
    assert_eq!(rig.session.controller().frame().voices, 0);
}

#[test]
fn scene_is_one_admission_and_stale_pad_requests_are_refused() {
    let mut rig = Rig::new();
    seed(&rig);
    let generation = rig.session.controller_state().generation;
    let revision = rig.session.document_snapshot().revision;
    assert!(
        rig.session
            .performance_pad(0, Some(0), generation + 1, revision)
            .is_err()
    );
    assert!(
        rig.session
            .performance_pad(0, Some(0), generation, revision + 1)
            .is_err()
    );
    assert!(
        rig.session
            .performance_scene(1, generation, revision)
            .is_err()
    );
    rig.session
        .performance_scene(0, generation, revision)
        .unwrap();
    rig.run(1);
    let state = rig.session.performance_state();
    assert_eq!(state.tracks[0].active, Some(rig.pattern()));
    assert_eq!(state.tracks[1].active, Some(rig.pattern()));
    assert_eq!(state.dropped_notes, "0");
}

#[test]
fn saved_media_reopens_and_only_saved_paths_in_current_generation_can_be_read() {
    let rig = Rig::new();
    let path = rig.file("authored-fixture.webm");
    std::fs::write(&path, b"fixture bytes, no codec execution").unwrap();
    let mut media = seed(&rig);
    media.videos = vec![VideoPlacement {
        path: path.clone(),
        start_tick: 960,
        length_ticks: 3840,
        offset_seconds: 1.25,
        muted: true,
        mixer_track: TrackId::MASTER,
    }];
    media.dance_channel = Some(rig.channel(0));
    media.visual.hue = 240.0;
    rig.session
        .dispatch(
            Command::SetStageMedia {
                media: media.clone(),
            },
            None,
        )
        .unwrap();
    let generation = rig.session.controller_state().generation;
    assert_eq!(
        rig.session.media_video_read(&path, generation).unwrap(),
        b"fixture bytes, no codec execution"
    );
    assert!(rig.session.media_video_read(&path, generation + 1).is_err());
    assert!(
        rig.session
            .media_video_read(&rig.file("not-saved.webm"), generation)
            .is_err()
    );
    let saved = rig
        .session
        .project_save(Some(&rig.file("media.windfall")))
        .unwrap();
    rig.session.project_new().unwrap();
    assert!(rig.session.media_video_read(&path, generation).is_err());
    rig.session.project_open(&saved).unwrap();
    assert_eq!(rig.project().settings.stage_media, media);
}

#[test]
fn unmuted_video_audio_prepares_without_document_lock_and_reopens_exact_pcm() {
    let rig = Rig::new();
    let path = rig.file("video-audio.ogg");
    // The existing decoder probes bytes, not extensions. A PCM fixture makes
    // mixer/persistence assertions independent of a third-party lossy encoder.
    let source = windfall_core::AudioBuffer::from_interleaved(48000, 2, vec![0.25; 48000]);
    windfall_codec::write_wav(&path, &source, windfall_codec::WavSampleFormat::Float32).unwrap();
    let mut media = StageMedia::default();
    media.videos.push(VideoPlacement { path: path.clone(), start_tick: 0, length_ticks: 960, offset_seconds: 0.0, muted: false, mixer_track: TrackId::MASTER });
    rig.session.dispatch(Command::SetStageMedia { media: media.clone() }, None).unwrap();
    assert_eq!(rig.session.state().pool.video_audio(&path).unwrap().samples(), source.samples());
    let saved = rig.session.project_save(Some(&rig.file("unmuted-video.windfall"))).unwrap();
    rig.session.project_new().unwrap();
    rig.session.project_open(&saved).unwrap();
    assert_eq!(rig.project().settings.stage_media, media);
    assert_eq!(rig.session.state().pool.video_audio(&path).unwrap().samples(), source.samples());
}

#[test]
fn missing_video_decoder_refuses_unmute_atomically() {
    let rig = Rig::new();
    let path = rig.file("missing-codec.webm");
    std::fs::write(&path, b"no supported decoder").unwrap();
    let before = rig.project();
    let mut media = StageMedia::default();
    media.videos.push(VideoPlacement { path, start_tick: 0, length_ticks: 960, offset_seconds: 0.0, muted: false, mixer_track: TrackId::MASTER });
    let error = rig.session.dispatch(Command::SetStageMedia { media }, None).unwrap_err();
    assert!(error.to_string().contains("Cannot decode video audio"));
    assert_eq!(rig.project(), before);
}
