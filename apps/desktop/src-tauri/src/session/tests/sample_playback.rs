use super::Rig;
use windfall_codec::{WavSampleFormat, write_wav};
use windfall_core::AudioBuffer;
use windfall_project::{
    ChannelId, Command, InstrumentKind, SamplePlayback, SettingsPatch, slicer::SliceOptions,
};
fn source(rig: &Rig) -> ChannelId {
    let path = rig.folder.path().join("authored-loop.wav");
    let mut pcm = vec![0.0; 96000];
    for frame in [24000, 48000, 72000] {
        pcm[frame..frame + 1200].fill(0.6);
    }
    write_wav(
        &path,
        &AudioBuffer::from_interleaved(48000, 1, pcm),
        WavSampleFormat::Int24,
    )
    .unwrap();
    rig.session
        .add_channel_from_file(path.to_str().unwrap(), None)
        .unwrap();
    let sample = rig
        .project()
        .channels
        .iter()
        .find_map(|c| c.source.sample())
        .unwrap();
    let c = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::SliceDeck),
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
                channel: c,
                asset: Some(SamplePlayback {
                    sample: Some(sample),
                    ..Default::default()
                }),
            },
            None,
        )
        .unwrap();
    c
}
#[test]
fn detected_playable_slice_map_is_one_history_step_and_reopens_original_source() {
    let rig = Rig::new();
    let c = source(&rig);
    let path = rig.folder.path().join("authored-loop.wav");
    let bytes = std::fs::read(&path).unwrap();
    let before = rig.project();
    let guard = rig.session.timeline_state();
    let result = rig
        .session
        .sample_playback_detect(
            c,
            SliceOptions::Grid { grid_ticks: 960 },
            guard.generation,
            guard.revision,
        )
        .unwrap();
    assert!(result.patch.channels.is_some());
    let detected = rig.project();
    assert_eq!(
        detected
            .channel(c)
            .unwrap()
            .sample_playback
            .as_ref()
            .unwrap()
            .slices
            .len(),
        4
    );
    rig.session.undo().unwrap();
    assert_eq!(rig.project(), before);
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), detected);
    let save = rig.file("playable-slices.windfall");
    rig.session.project_save(Some(&save)).unwrap();
    rig.session.project_open(&save).unwrap();
    assert_eq!(
        rig.project().channel(c).unwrap().sample_playback,
        detected.channel(c).unwrap().sample_playback
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}
#[test]
fn detection_rechecks_edits_source_and_document_replacement_after_off_lock_work() {
    for point in ["sample-playback:detected", "sample-playback:prepared"] {
        for mutation in 0..3 {
            let rig = Rig::new();
            let c = source(&rig);
            let guard = rig.session.timeline_state();
            let held = rig.session.hold(point);
            let work = rig.session.background(move |s| {
                s.sample_playback_detect(
                    c,
                    SliceOptions::Transients { sensitivity: 0.6 },
                    guard.generation,
                    guard.revision,
                )
            });
            held.wait();
            match mutation {
                0 => {
                    rig.session
                        .dispatch(
                            Command::UpdateSettings {
                                patch: SettingsPatch {
                                    name: Some("Changed".into()),
                                    ..Default::default()
                                },
                            },
                            None,
                        )
                        .unwrap();
                }
                1 => {
                    let sample = rig
                        .project()
                        .channel(c)
                        .unwrap()
                        .sample_playback
                        .as_ref()
                        .unwrap()
                        .sample
                        .unwrap();
                    rig.session.state().pool.insert(
                        sample,
                        AudioBuffer::from_interleaved(48000, 1, vec![0.0; 96000]),
                    );
                }
                _ => {
                    rig.session.project_new().unwrap();
                }
            }
            let before = rig.session.document_snapshot();
            held.release();
            assert!(work.join().unwrap().unwrap_err().contains("changed"));
            assert_eq!(rig.session.document_snapshot(), before);
        }
    }
}
#[test]
fn obsolete_detection_request_and_invalid_options_leave_document_unchanged() {
    let rig = Rig::new();
    let c = source(&rig);
    let guard = rig.session.timeline_state();
    let before = rig.session.document_snapshot();
    assert!(
        rig.session
            .sample_playback_detect(
                c,
                SliceOptions::Grid { grid_ticks: 17 },
                guard.generation,
                guard.revision
            )
            .is_err()
    );
    assert!(
        rig.session
            .sample_playback_detect(
                c,
                SliceOptions::Grid { grid_ticks: 960 },
                guard.generation + 1,
                guard.revision
            )
            .is_err()
    );
    assert_eq!(rig.session.document_snapshot(), before);
}
