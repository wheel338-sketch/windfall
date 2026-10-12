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
    let mut before = rig.project();
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
    before.next_id = rig.project().next_id;
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

#[test]
fn portable_bank_copies_own_pcm_loads_into_a_fresh_player_and_preserves_one_step_history() {
    let rig = Rig::new();
    let channel = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::MultiSampler),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let path = rig.file("source.wav");
    let pcm = windfall_core::AudioBuffer::from_interleaved(48000, 1, vec![0.25; 4800]);
    windfall_codec::write_wav(&path, &pcm, windfall_codec::WavSampleFormat::Float32).unwrap();
    rig.session
        .sample_instrument_import_zone(channel, None, path.clone(), None)
        .unwrap();
    let bank_path = rig.file("own.wfbank");
    rig.session
        .sample_instrument_save_bank(
            channel,
            rig.project()
                .channel(channel)
                .unwrap()
                .sample_instrument
                .clone(),
            bank_path.clone(),
        )
        .unwrap();
    std::fs::remove_file(path).unwrap(); // Loading must use the exported copy.
    rig.session.project_new().unwrap();
    let player = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::SamplePlayer),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let mut before = rig.project();
    rig.session
        .sample_instrument_load_bank(player, None, bank_path)
        .unwrap();
    let loaded = rig.project();
    let Some(SampleInstrumentAsset::Zones { zones }) =
        &loaded.channel(player).unwrap().sample_instrument
    else {
        panic!("player bank missing")
    };
    let audio = rig
        .session
        .state()
        .pool
        .get(windfall_project::SampleId(zones[0].sample))
        .unwrap()
        .clone();
    assert_eq!(audio.samples(), pcm.samples());
    rig.session.undo().unwrap();
    before.next_id = rig.project().next_id;
    assert_eq!(rig.project(), before);
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), loaded);
}

#[test]
fn speech_cancel_before_worker_start_and_after_render_preserves_document_and_cleans_pcm() {
    let rig = Rig::new();
    let channel = ChannelId(rig.session.dispatch(Command::AddChannel { name: None, sample: None,
        instrument: Some(InstrumentKind::TextVoice), index: None, mixer_track: None }, None).unwrap().created[0]);
    let request = rig.session.sample_instrument_speech_begin(channel).unwrap();
    let before = rig.session.document_snapshot();
    rig.session.sample_instrument_speech_cancel(channel, request);
    assert!(rig.session.sample_instrument_speech_request(channel, None, "Hello".into(), 48.0, 1.0, request).unwrap_err().contains("cancelled"));
    assert_eq!(rig.session.document_snapshot(), before);
    let request = rig.session.sample_instrument_speech_begin(channel).unwrap();
    let hold = rig.session.hold("sample-speech:rendered");
    let job = rig.session.background(move |session| session.sample_instrument_speech_request(channel, None, "Hello".into(), 48.0, 1.0, request));
    hold.wait(); rig.session.sample_instrument_speech_cancel(channel, request); hold.release();
    assert!(job.join().unwrap().unwrap_err().contains("cancelled"));
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(std::fs::read_dir(rig.session.store().recordings_dir().join("Text voice")).unwrap().count(), 0);
}

#[test]
fn stale_speech_cancellation_does_not_cancel_the_new_request_and_preparation_cancel_is_atomic() {
    let rig = Rig::new();
    let channel = ChannelId(rig.session.dispatch(Command::AddChannel { name: None, sample: None,
        instrument: Some(InstrumentKind::TextVoice), index: None, mixer_track: None }, None).unwrap().created[0]);
    let old = rig.session.sample_instrument_speech_begin(channel).unwrap();
    let request = rig.session.sample_instrument_speech_begin(channel).unwrap();
    rig.session.sample_instrument_speech_cancel(channel, old);
    let before = rig.session.document_snapshot();
    let hold = rig.session.hold("sampler:prepared");
    let job = rig.session.background(move |session| session.sample_instrument_speech_request(channel, None, "Hello".into(), 48.0, 1.0, request));
    hold.wait(); rig.session.sample_instrument_speech_cancel(channel, request); hold.release();
    assert!(job.join().unwrap().unwrap_err().contains("cancelled"));
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(std::fs::read_dir(rig.session.store().recordings_dir().join("Text voice")).unwrap().count(), 0);
}

#[test]
fn missing_player_reopens_relinks_same_ids_and_restores_source_readiness_through_history() {
    let rig = Rig::new();
    let missing = rig.file("missing.wav");
    let source = windfall_project::SampleId(rig.session.dispatch(Command::AddSample { name: "Missing layer".into(), path: SamplePath::External(missing.clone()) }, None).unwrap().created[0]);
    rig.wait_until_loaded_or_failed(source);
    let channel = ChannelId(rig.session.dispatch(Command::AddChannel { name: None, sample: None,
        instrument: Some(InstrumentKind::SamplePlayer), index: None, mixer_track: None }, None).unwrap().created[0]);
    let bank = SampleInstrumentAsset::Zones { zones: vec![windfall_dsp::sample_bank::SampleZone { sample: source.0, ..Default::default() }] };
    rig.session.dispatch(Command::SetSampleInstrumentAsset { channel, expected: None, asset: Some(bank.clone()) }, None).unwrap();
    let file = rig.file("missing-player.windfall"); rig.session.project_save(Some(&file)).unwrap();
    let reopened = rig.restart(); reopened.session.project_open(&file).unwrap(); reopened.wait_until_loaded_or_failed(source);
    assert_eq!(reopened.session.sample_instrument_missing_sources(channel).unwrap(), vec![source]);
    let replacement = reopened.file("replacement.wav");
    let pcm = windfall_core::AudioBuffer::from_interleaved(48000, 1, vec![0.3; 4800]);
    windfall_codec::write_wav(&replacement, &pcm, windfall_codec::WavSampleFormat::Float32).unwrap();
    let expected = reopened.project().sample(source).unwrap().path.clone();
    reopened.session.sample_instrument_relink_source(channel, Some(bank.clone()), source, expected, replacement).unwrap();
    let linked = reopened.project();
    assert_eq!(linked.channel(channel).unwrap().sample_instrument, Some(bank.clone()));
    assert_eq!(reopened.session.state().pool.get(source).unwrap().samples(), pcm.samples());
    assert!(reopened.session.sample_instrument_missing_sources(channel).unwrap().is_empty());
    assert!(reopened.session.undo().is_some()); reopened.wait_until_loaded_or_failed(source);
    assert_eq!(reopened.session.sample_instrument_missing_sources(channel).unwrap(), vec![source]);
    assert_eq!(reopened.project().channel(channel).unwrap().sample_instrument, Some(bank));
    assert!(reopened.session.redo().is_some());
    assert_eq!(reopened.project(), linked);
    assert_eq!(reopened.session.state().pool.get(source).unwrap().samples(), pcm.samples());
}

#[test]
fn stale_source_relink_cannot_change_a_replaced_project() {
    let rig = Rig::new();
    let source = windfall_project::SampleId(rig.session.dispatch(Command::AddSample { name: "Layer".into(), path: SamplePath::External(rig.file("missing.wav")) }, None).unwrap().created[0]);
    rig.wait_until_loaded_or_failed(source);
    let channel = ChannelId(rig.session.dispatch(Command::AddChannel { name: None, sample: None,
        instrument: Some(InstrumentKind::SamplePlayer), index: None, mixer_track: None }, None).unwrap().created[0]);
    let bank = SampleInstrumentAsset::Zones { zones: vec![windfall_dsp::sample_bank::SampleZone { sample: source.0, ..Default::default() }] };
    rig.session.dispatch(Command::SetSampleInstrumentAsset { channel, expected: None, asset: Some(bank.clone()) }, None).unwrap();
    let path = rig.file("relink.wav"); windfall_codec::write_wav(&path,
        &windfall_core::AudioBuffer::from_interleaved(48000, 1, vec![0.2; 480]), windfall_codec::WavSampleFormat::Float32).unwrap();
    let expected = rig.project().sample(source).unwrap().path.clone();
    let hold = rig.session.hold("sample-relink:decoded");
    let job = rig.session.background(move |session| session.sample_instrument_relink_source(channel, Some(bank), source, expected, path));
    hold.wait(); rig.session.project_new().unwrap(); let before = rig.session.document_snapshot(); hold.release();
    assert!(job.join().unwrap().is_err()); assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn project_controller_destination_reopens_without_changing_application_midi_settings() {
    let rig=Rig::new();let id=ChannelId(rig.session.dispatch(Command::AddChannel {name:None,sample:None,instrument:Some(InstrumentKind::MidiOut),index:None,mixer_track:None},None).unwrap().created[0]);
    let application=rig.session.midi_hardware_state().settings;
    let before=rig.project();let mut params=InstrumentKind::MidiOut.default_params();if let windfall_dsp::InstrumentParams::MidiOut(params)=&mut params {params.feedback_enabled=true;params.feedback_controller=1;params.feedback_motor=2;params.feedback_strength=0.7;params.feedback_curve=0.5;}
    rig.session.dispatch(Command::SetInstrumentParams {channel:id,params},None).unwrap();let saved=rig.project();
    rig.session.undo().unwrap();assert_eq!(rig.project(),before);rig.session.redo().unwrap();assert_eq!(rig.project(),saved);
    let path=rig.file("project-feedback.windfall");rig.session.project_save(Some(&path)).unwrap();assert_eq!(rig.session.midi_hardware_state().settings,application);let reopened=rig.restart();reopened.session.project_open(&path).unwrap();
    assert_eq!(reopened.project().channel(id).unwrap().source,saved.channel(id).unwrap().source);
    assert_eq!(reopened.session.midi_hardware_state().settings,application);
}
