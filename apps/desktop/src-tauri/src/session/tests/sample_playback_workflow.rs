//! Joined native Session workflow. These tests drive the same operations as the
//! window, but deliberately make no claim about an actual WebView or device.
use super::{Rig, SAMPLE_RATE, rms};
use windfall_codec::{WavSampleFormat, decode_file, write_wav};
use windfall_core::AudioBuffer;
use windfall_dsp::granular::{PlaybackCurve, PlaybackPoint};
use windfall_ipc::{BitDepth, ExportFormat, ExportOptions, PlayMode, TransportPatch};
use windfall_project::{
    ChannelId, Command, InstrumentKind, KeyPlaybackCurve, NoteInit, SampleId, SamplePlayback,
    SettingsPatch, slicer::SliceOptions,
};

fn import(rig: &Rig, name: &str, phase: f32) -> SampleId {
    let path = rig.folder.path().join(format!("{name}.wav"));
    let pcm = (0..96_000)
        .flat_map(|frame| {
            let pulse = if frame % 24_000 < 1200 { 0.35 } else { 0.0 };
            let signal = (frame as f32 * 0.019 + phase).sin() * 0.15 + pulse;
            [signal, -signal * 0.6]
        })
        .collect();
    write_wav(
        &path,
        &AudioBuffer::from_interleaved(SAMPLE_RATE, 2, pcm),
        WavSampleFormat::Float32,
    )
    .unwrap();
    rig.session
        .add_channel_from_file(path.to_str().unwrap(), None)
        .unwrap();
    let sample = rig
        .project()
        .channels
        .iter()
        .find(|c| c.name == name)
        .and_then(|c| c.source.sample())
        .unwrap();
    assert!(rig.has_audio(sample));
    let info = rig.session.sample_info_by_id(sample).unwrap();
    assert_eq!(info.sample_rate, SAMPLE_RATE);
    sample
}

fn export(rig: &Rig, name: &str) -> AudioBuffer {
    rig.events.take();
    let options = ExportOptions {
        path: rig.file(name),
        format: ExportFormat::Wav,
        bit_depth: BitDepth::Float32,
        sample_rate: SAMPLE_RATE,
        mode: PlayMode::Pattern,
        pattern_loops: 1,
        tail_secs: 0.0,
        auto_tail: false,
        ..ExportOptions::default()
    };
    rig.session.export_audio(options.clone()).unwrap();
    let done = rig.events.wait_for_export();
    assert_eq!(done.error, None);
    assert_eq!(done.path, options.path);
    let audio = decode_file(&options.path).unwrap();
    assert_eq!(audio.sample_rate(), SAMPLE_RATE);
    assert_eq!(audio.frames(), 96_000);
    assert!(audio.samples().iter().all(|v| v.is_finite() && v.abs() <= 4.0));
    assert!(rms(audio.samples()) > 1e-4);
    audio
}

fn workflow(kind: InstrumentKind) {
    let mut rig = Rig::new();
    rig.session
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    tempo_bpm: Some(120.0),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    let sample = import(&rig, "workflow-loop-a", 0.0);
    let sample_b = import(&rig, "workflow-loop-b", 1.0);
    let original_path = rig.folder.path().join("workflow-loop-a.wav");
    let original_bytes = std::fs::read(&original_path).unwrap();
    let channel = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(kind),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    rig.session
        .dispatch(
            Command::SetSamplePlayback {
                channel,
                asset: Some(SamplePlayback {
                    sample: Some(sample),
                    sample_b: (kind == InstrumentKind::SliceDeck).then_some(sample_b),
                    ..Default::default()
                }),
            },
            None,
        )
        .unwrap();
    let slicing = matches!(kind, InstrumentKind::SliceMap | InstrumentKind::SliceDeck);
    if slicing {
        // Exercise both native detector entry paths against imported PCM.
        for options in [
            SliceOptions::Transients { sensitivity: 1.0 },
            SliceOptions::Grid { grid_ticks: 960 },
        ] {
            let before = rig.project();
            let cursor = rig.session.document_snapshot().history.cursor;
            let guard = rig.session.timeline_state();
            rig.session
                .sample_playback_detect(channel, options, guard.generation, guard.revision)
                .unwrap();
            let detected = rig.project();
            assert!(
                detected
                    .channel(channel)
                    .unwrap()
                    .sample_playback
                    .as_ref()
                    .unwrap()
                    .slices
                    .len()
                    >= 2
            );
            assert_eq!(rig.session.document_snapshot().history.cursor, cursor + 1);
            rig.session.undo().unwrap();
            assert_eq!(rig.project(), before);
            rig.session.redo().unwrap();
            assert_eq!(rig.project(), detected);
        }
        let before = rig.project().patterns;
        let cursor = rig.session.document_snapshot().history.cursor;
        rig.session
            .dispatch(
                Command::GenerateSlicePattern {
                    channel,
                    pattern: rig.pattern(),
                },
                None,
            )
            .unwrap();
        let project = rig.project();
        let notes = &project.patterns[0]
            .lanes
            .iter()
            .find(|l| l.channel == channel)
            .unwrap()
            .notes;
        assert_eq!(
            notes.iter().map(|n| (n.start, n.key)).collect::<Vec<_>>(),
            [(0, 60), (960, 61), (1920, 62), (2880, 63)]
        );
        assert_eq!(rig.session.document_snapshot().history.cursor, cursor + 1);
        rig.session.undo().unwrap();
        assert_eq!(rig.project().patterns, before);
        rig.session.redo().unwrap();
        assert_eq!(rig.project().patterns, project.patterns);
    } else {
        rig.session
            .dispatch(
                Command::AddNotes {
                    pattern: rig.pattern(),
                    channel,
                    notes: vec![NoteInit {
                        start: 0,
                        length: 1920,
                        key: 60,
                        velocity: Some(0.8),
                        pan: None,
                        expression: None,
                    }],
                },
                None,
            )
            .unwrap();
    }
    let baseline = export(&rig, "before-edit.wav");
    let before = rig.project();
    let cursor = rig.session.document_snapshot().history.cursor;
    if kind == InstrumentKind::GrainCloud {
        let param = kind
            .descriptors()
            .iter()
            .position(|p| p.id == "density")
            .unwrap() as u32;
        rig.session
            .dispatch(
                Command::SetInstrumentParam {
                    channel,
                    param,
                    value: 128.0,
                },
                None,
            )
            .unwrap();
    } else {
        let mut asset = before
            .channel(channel)
            .unwrap()
            .sample_playback
            .clone()
            .unwrap();
        if kind == InstrumentKind::WaveRide {
            asset.key_curves.push(KeyPlaybackCurve {
                key: 60,
                curve: PlaybackCurve {
                    points: vec![
                        PlaybackPoint {
                            time: 0.0,
                            value: 0.9,
                        },
                        PlaybackPoint {
                            time: 0.5,
                            value: 0.1,
                        },
                        PlaybackPoint {
                            time: 1.0,
                            value: 0.7,
                        },
                    ],
                    ..Default::default()
                },
            });
        } else {
            asset.slices.swap(0, 3);
            asset.slices[0].start += 0.025;
            if kind == InstrumentKind::SliceDeck {
                asset.slices[0].deck = 1;
                asset.slices[0].gain = 0.4;
                asset.slices[0].pitch = 7.0;
                asset.slices[0].reverse = true;
                asset.slices[0].attack_ms = 12.0;
                asset.slices[0].release_ms = 80.0;
            }
        }
        rig.session
            .dispatch(
                Command::SetSamplePlayback {
                    channel,
                    asset: Some(asset),
                },
                None,
            )
            .unwrap();
    }
    let authored = rig.project();
    assert_eq!(rig.session.document_snapshot().history.cursor, cursor + 1);
    let edited = export(&rig, "after-edit.wav");
    assert_ne!(
        baseline.samples(),
        edited.samples(),
        "{kind:?} edit must change native PCM"
    );
    rig.session.undo().unwrap();
    assert_eq!(rig.project(), before);
    assert_eq!(export(&rig, "undo.wav").samples(), baseline.samples());
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), authored);
    assert_eq!(export(&rig, "redo.wav").samples(), edited.samples());

    // The stopped transport's keyboard uses the same prepared source and edits.
    rig.session.audition_note_on(channel, 60, 0.8);
    assert!(rms(&rig.run(4096)) > 1e-4, "{kind:?} keyboard");
    rig.session.audition_note_off(channel, 60);
    rig.run(96_000);
    assert_eq!(rms(&rig.run(128)), 0.0);

    let saved = rig.file("sample-workflow.windfall");
    rig.session.project_save(Some(&saved)).unwrap();
    rig.session.project_new().unwrap();
    rig.session.project_open(&saved).unwrap();
    rig.wait_until_loaded_or_failed(sample);
    rig.wait_until_loaded_or_failed(sample_b);
    assert!(rig.has_audio(sample) && rig.has_audio(sample_b));
    assert_eq!(rig.project().channel(channel), authored.channel(channel));
    assert_eq!(export(&rig, "reopened.wav").samples(), edited.samples());
    assert_eq!(std::fs::read(&original_path).unwrap(), original_bytes);

    rig.session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Pattern),
            pattern: Some(rig.pattern()),
            ..Default::default()
        })
        .unwrap();
    rig.session.transport_stop();
    rig.run(512); // Drain project replacement and the outgoing-note fade.
    rig.session.transport_play().unwrap();
    let latency = rig.session.controller().latency_frames() as usize;
    let live = rig.run(24_000 + latency);
    assert_eq!(
        &live[latency * 2..],
        &edited.samples()[..48_000],
        "{kind:?} live/export"
    );
}

#[test]
fn slice_map_import_detection_authoring_keyboard_history_reopen_and_export_are_joined() {
    workflow(InstrumentKind::SliceMap);
}

#[test]
fn slice_deck_two_sources_authoring_keyboard_history_reopen_and_export_are_joined() {
    workflow(InstrumentKind::SliceDeck);
}

#[test]
fn grain_cloud_import_density_keyboard_history_reopen_and_export_are_joined() {
    workflow(InstrumentKind::GrainCloud);
}

#[test]
fn wave_ride_import_per_key_curve_keyboard_history_reopen_and_export_are_joined() {
    workflow(InstrumentKind::WaveRide);
}
