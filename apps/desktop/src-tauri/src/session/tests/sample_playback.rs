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
        .find(|c| c.name == "authored-loop")
        .and_then(|c| c.source.sample())
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

fn wav_slices(rig: &Rig) -> ChannelId {
    let c = source(rig);
    let path = rig.folder.path().join("regions.wav");
    let metadata = windfall_codec::WavMetadata {
        cues: vec![
            windfall_codec::WavCue { id: 9, frame: 48000, label: "Later first key".into() },
            windfall_codec::WavCue { id: 4, frame: 0, label: "Earlier second key".into() },
        ],
        slices: vec![
            windfall_codec::WavSlice { cue_id: 9, length: 48000 },
            windfall_codec::WavSlice { cue_id: 4, length: 24000 },
        ],
        tempo: Some(windfall_codec::WavTempo {
            bpm: 120.0, beats: 4, numerator: 4, denominator: 4,
            root_key: None, one_shot: false,
        }),
        ..Default::default()
    };
    let mut writer = windfall_codec::WavWriter::create_with_metadata(
        &path, 48000, 1, WavSampleFormat::Float32,
        &windfall_codec::AudioTags::default(), &metadata,
    ).unwrap();
    let pcm = vec![0.25; 96000];
    writer.write(&pcm).unwrap();
    writer.finalize().unwrap();
    let added = rig.session.add_channel_from_file(path.to_str().unwrap(), None).unwrap();
    let sample = windfall_project::SampleId(added.created[0]);
    rig.session.dispatch(Command::SetSamplePlayback {
        channel: c, asset: Some(SamplePlayback { sample: Some(sample), ..Default::default() }),
    }, None).unwrap();
    c
}

#[test]
fn authored_wav_slices_import_export_undo_and_reopen_keep_key_order_and_source_pcm() {
    let rig = Rig::new();
    let c = wav_slices(&rig);
    let before = rig.session.document_snapshot();
    let guard = rig.session.timeline_state();
    rig.session.sample_playback_detect(c, SliceOptions::WavMarkers, guard.generation, guard.revision).unwrap();
    let imported = rig.project();
    let asset = imported.channel(c).unwrap().sample_playback.as_ref().unwrap();
    assert_eq!(asset.slices.iter().map(|s| (s.start, s.end)).collect::<Vec<_>>(),
        vec![(0.5, 1.0), (0.0, 0.25)]);
    assert_eq!(rig.session.document_snapshot().history.cursor, before.history.cursor + 1);
    rig.session.undo().unwrap();
    assert_eq!(rig.project(), before.project);
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), imported);
    let guard = rig.session.timeline_state();
    let output = rig.file("source-slices.wav");
    rig.session.sample_playback_export(c, &output, guard.generation, guard.revision).unwrap();
    let decoded = windfall_codec::decode_file(&output).unwrap();
    assert_eq!(decoded.frames(), 96000);
    assert!(decoded.samples().iter().all(|v| *v == 0.25));
    // Independent RIFF walker: inspect disk positions and region lengths.
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let mut at = 12;
    let mut cues = Vec::new();
    let mut lengths = Vec::new();
    while at < bytes.len() {
        let size = word(at + 4) as usize;
        let end = at + 8 + size;
        assert!(end <= bytes.len());
        if &bytes[at..at + 4] == b"cue " {
            assert_eq!(word(at + 8), 2);
            for index in 0..2 { cues.push(word(at + 12 + index * 24 + 20)); }
        }
        if &bytes[at..at + 4] == b"LIST" && &bytes[at + 8..at + 12] == b"adtl" {
            let mut sub = at + 12;
            while sub < end {
                let len = word(sub + 4) as usize;
                if &bytes[sub..sub + 4] == b"ltxt" { lengths.push(word(sub + 12)); }
                sub += 8 + len + len % 2;
            }
        }
        at = end + size % 2;
    }
    assert_eq!(cues, vec![48000, 0]);
    assert_eq!(lengths, vec![48000, 24000]);
    let save = rig.file("wav-slices.windfall");
    rig.session.project_save(Some(&save)).unwrap();
    rig.session.project_open(&save).unwrap();
    assert_eq!(rig.project().channel(c).unwrap().sample_playback, imported.channel(c).unwrap().sample_playback);
}

#[test]
fn wav_slice_import_rechecks_source_document_and_edits_after_preparation() {
    for point in ["sample-playback:detected", "sample-playback:prepared"] {
        for mutation in 0..3 {
            let rig = Rig::new();
            let c = wav_slices(&rig);
            let guard = rig.session.timeline_state();
            let held = rig.session.hold(point);
            let work = rig.session.background(move |s| s.sample_playback_detect(
                c, SliceOptions::WavMarkers, guard.generation, guard.revision,
            ));
            held.wait();
            match mutation {
                0 => { rig.session.project_new().unwrap(); }
                1 => { rig.session.dispatch(Command::UpdateSettings { patch: SettingsPatch {
                    name: Some("Changed during marker read".into()), ..Default::default()
                } }, None).unwrap(); }
                _ => {
                    let path = rig.folder.path().join("regions.wav");
                    std::fs::write(path, b"RIFF hostile truncated metadata").unwrap();
                }
            }
            let before = rig.session.document_snapshot();
            held.release();
            assert!(work.join().unwrap().is_err());
            assert_eq!(rig.session.document_snapshot(), before);
        }
    }
}

#[test]
fn wav_slice_import_without_regions_and_stale_export_leave_document_and_destination_unchanged() {
    let rig = Rig::new();
    let c = source(&rig);
    let guard = rig.session.timeline_state();
    let before = rig.session.document_snapshot();
    assert!(rig.session.sample_playback_detect(c, SliceOptions::WavMarkers, guard.generation, guard.revision).is_err());
    let output = rig.file("untouched.wav");
    std::fs::write(&output, b"existing destination").unwrap();
    assert!(rig.session.sample_playback_export(c, &output, guard.generation + 1, guard.revision).is_err());
    assert_eq!(std::fs::read(output).unwrap(), b"existing destination");
    assert_eq!(rig.session.document_snapshot(), before);
}
