use super::Rig;
use windfall_project::{ChannelId, Command, InstrumentKind, SampleInstrumentAsset, SamplePath};

#[test]
fn speech_render_saves_a_source_as_one_history_step_and_reopens() {
    let rig = Rig::new();
    let channel = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::TextVoice),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let result = rig
        .session
        .sample_instrument_speech(channel, None, "Hello windfall".into(), 48.0, 1.0)
        .unwrap();
    assert_eq!(result.created.len(), 1);
    let edited = rig.project();
    let Some(SampleInstrumentAsset::Speech {
        source: Some(source),
        ..
    }) = &edited.channel(channel).unwrap().sample_instrument
    else {
        panic!("saved speech source missing")
    };
    let SamplePath::External(path) = &edited.sample(*source).unwrap().path else {
        panic!("speech source path missing")
    };
    let audio = windfall_codec::decode_file(path).unwrap();
    assert_eq!(audio.frames(), 24000);
    assert!(rig.has_audio(*source));
    rig.session.undo().unwrap();
    assert!(
        rig.project()
            .channel(channel)
            .unwrap()
            .sample_instrument
            .is_none()
    );
    assert!(
        std::path::Path::new(path).exists(),
        "history retains source"
    );
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), edited);
    let saved = rig.file("speech.windfall");
    rig.session.project_save(Some(&saved)).unwrap();
    let restarted = rig.restart();
    restarted.session.project_open(&saved).unwrap();
    restarted.wait_until_loaded_or_failed(*source);
    assert!(restarted.has_audio(*source));
    assert!(matches!(
        restarted
            .project()
            .channel(channel)
            .unwrap()
            .sample_instrument,
        Some(SampleInstrumentAsset::Speech {
            source: Some(_),
            ..
        })
    ));
}
#[test]
fn unsupported_speech_and_stale_recipe_do_not_change_document_or_history() {
    let rig = Rig::new();
    let channel = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::TextVoice),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let before = rig.session.document_snapshot();
    assert!(
        rig.session
            .sample_instrument_speech(channel, None, "123".into(), 48.0, 1.0)
            .is_err()
    );
    assert!(
        rig.session
            .sample_instrument_speech(
                channel,
                Some(SampleInstrumentAsset::Speech {
                    text: "old".into(),
                    pitch: 48.0,
                    duration: 1.0,
                    version: 1,
                    source: None
                }),
                "Hello".into(),
                48.0,
                1.0
            )
            .is_err()
    );
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn import_zone_reuses_sources_and_undo_removes_the_layer_without_extra_channels() {
    let rig = Rig::new();
    let channel = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::PadBank),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let before = rig.project();
    let path = rig.file("pad.wav");
    windfall_codec::write_wav(
        &path,
        &windfall_core::AudioBuffer::from_interleaved(48000, 1, vec![0.25; 4800]),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
    rig.session
        .sample_instrument_import_zone(channel, None, path.clone(), Some(37))
        .unwrap();
    let first = rig.project();
    let expected = first.channel(channel).unwrap().sample_instrument.clone();
    rig.session
        .sample_instrument_import_zone(channel, expected, path, Some(37))
        .unwrap();
    let second = rig.project();
    assert_eq!(second.channels.len(), before.channels.len());
    assert_eq!(second.samples.len(), before.samples.len() + 1);
    let Some(SampleInstrumentAsset::Zones { zones }) =
        &second.channel(channel).unwrap().sample_instrument
    else {
        panic!("missing mappings")
    };
    assert_eq!(zones.len(), 2);
    assert_eq!(zones[0].sample, zones[1].sample);
    assert_eq!((zones[0].key_low, zones[0].key_high), (37, 37));
    assert!(rig.has_audio(windfall_project::SampleId(zones[0].sample)));
    rig.session.undo().unwrap();
    assert_eq!(rig.project(), first);
    rig.session.undo().unwrap();
    assert_eq!(rig.project(), before);
}

#[test]
fn replacement_during_speech_render_refuses_commit_and_cleans_the_saved_source() {
    let rig = Rig::new();
    let channel = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::TextVoice),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let hold = rig.session.hold("sample-speech:rendered");
    let job = rig.session.background(move |session| {
        session.sample_instrument_speech(channel, None, "Hello".into(), 48.0, 1.0)
    });
    hold.wait();
    rig.session.project_new().unwrap();
    let replacement = rig.session.document_snapshot();
    hold.release();
    assert!(job.join().unwrap().is_err());
    assert_eq!(rig.session.document_snapshot(), replacement);
    let folder = rig.session.store().recordings_dir().join("Text voice");
    assert_eq!(std::fs::read_dir(folder).unwrap().count(), 0);
}
