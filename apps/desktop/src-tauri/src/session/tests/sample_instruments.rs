use super::Rig;
use windfall_project::{ChannelId, Command, InstrumentKind, SampleInstrumentAsset, SamplePath};

fn soundfont_channel(rig:&Rig)->ChannelId {
    ChannelId(rig.session.dispatch(Command::AddChannel {name:None,sample:None,
        instrument:Some(InstrumentKind::SoundFont),index:None,mixer_track:None},None).unwrap().created[0])
}

#[test]
fn soundfont_import_owns_source_and_missing_relink_reopens_with_checked_history() {
    let rig=Rig::new();let channel=soundfont_channel(&rig);
    let before=rig.project();let original=rig.file("sf-owned.sf2");
    std::fs::write(&original,general_midi_fixture()).unwrap();
    rig.session.soundfont_load(channel,None,original.clone(),0,0).unwrap();
    let imported=rig.project();let asset=imported.channel(channel).unwrap().sample_instrument.clone().unwrap();
    let source=asset.soundfont_source().unwrap();assert!(imported.sample(source).is_some());
    assert!(rig.session.state().pool.has_sample_instrument_asset(&asset.canonical(&imported)));
    rig.session.undo().unwrap();assert_eq!(rig.project(),before);
    rig.session.redo().unwrap();assert_eq!(rig.project(),imported);
    let saved=rig.file("sf-owned.windfall");rig.session.project_save(Some(&saved)).unwrap();
    let replacement=rig.file("sf-replacement.sf2");std::fs::rename(&original,&replacement).unwrap();
    let reopened=rig.restart();reopened.session.project_open(&saved).unwrap();
    assert_eq!(reopened.session.sample_instrument_missing_sources(channel).unwrap(),[source]);
    let before=reopened.project();
    reopened.session.soundfont_relink(channel,asset.clone(),before.sample(source).unwrap().path.clone(),replacement.clone()).unwrap();
    let after=reopened.project();assert_eq!(after.channel(channel).unwrap().sample_instrument,Some(asset.clone()));
    assert_eq!(after.sample(source).unwrap().path,SamplePath::External(replacement));
    assert!(reopened.session.sample_instrument_missing_sources(channel).unwrap().is_empty());
    reopened.session.undo().unwrap();assert_eq!(reopened.project(),before);
    assert_eq!(reopened.session.sample_instrument_missing_sources(channel).unwrap(),[source]);
    reopened.session.redo().unwrap();assert_eq!(reopened.project(),after);
    reopened.session.project_save(Some(&saved)).unwrap();
    let final_session=reopened.restart();final_session.session.project_open(&saved).unwrap();
    assert_eq!(final_session.project().sample(source),after.sample(source));
    assert!(final_session.session.sample_instrument_missing_sources(channel).unwrap().is_empty());
}

#[test]
fn soundfont_relink_rejects_malformed_missing_presets_and_stale_publication_atomically() {
    let rig=Rig::new();let channel=soundfont_channel(&rig);let file=rig.file("sf-original.sf2");
    std::fs::write(&file,general_midi_fixture()).unwrap();rig.session.soundfont_load(channel,None,file,0,0).unwrap();
    let project=rig.project();let asset=project.channel(channel).unwrap().sample_instrument.clone().unwrap();
    let source=asset.soundfont_source().unwrap();let expected=project.sample(source).unwrap().path.clone();
    let before=rig.session.document_snapshot();let bad=rig.file("sf-bad.sf2");std::fs::write(&bad,b"hostile").unwrap();
    assert!(rig.session.soundfont_relink(channel,asset.clone(),expected.clone(),bad).is_err());
    assert_eq!(rig.session.document_snapshot(),before);
    let wrong=rig.file("sf-wrong-preset.sf2");let mut bytes=general_midi_fixture();
    let at=bytes.windows(4).position(|w|w==b"phdr").unwrap()+8+20;bytes[at..at+2].copy_from_slice(&1u16.to_le_bytes());
    std::fs::write(&wrong,bytes).unwrap();
    assert!(rig.session.soundfont_relink(channel,asset.clone(),expected.clone(),wrong).is_err());
    assert_eq!(rig.session.document_snapshot(),before);
    let valid=rig.file("sf-valid.sf2");std::fs::write(&valid,general_midi_fixture()).unwrap();
    let hold=rig.session.hold("soundfont-relink:prepared");
    let job=rig.session.background(move|session|session.soundfont_relink(channel,asset,expected,valid));
    hold.wait();rig.session.project_new().unwrap();let before=rig.session.document_snapshot();hold.release();
    assert!(job.join().unwrap().is_err());assert_eq!(rig.session.document_snapshot(),before);
}

#[test]
fn soundfont_portable_archive_contains_exact_bank_bytes_and_opens_without_original_file() {
    let rig=Rig::new();let channel=soundfont_channel(&rig);let file=rig.file("sf-archive.sf2");
    let bytes=general_midi_fixture();std::fs::write(&file,&bytes).unwrap();
    let earlier=rig.file("sf-earlier.bank");std::fs::write(&earlier,&bytes).unwrap();
    rig.session.soundfont_load(channel,None,earlier.clone(),0,0).unwrap();
    let previous=rig.project().channel(channel).unwrap().sample_instrument.clone();
    let retained=previous.as_ref().unwrap().soundfont_source().unwrap();
    rig.session.soundfont_load(channel,previous,file.clone(),0,0).unwrap();
    let before=rig.project();let source=before.channel(channel).unwrap().sample_instrument.as_ref().unwrap().soundfont_source().unwrap();
    assert!(before.is_soundfont_source(retained));
    let archive=rig.file("sf-portable.zip");rig.session.project_archive_save(&archive).unwrap();
    assert_eq!(rig.project(),before);std::fs::remove_file(&file).unwrap();std::fs::remove_file(&earlier).unwrap();
    rig.session.project_archive_open(&archive).unwrap();let reopened=rig.project();
    let state=rig.session.state();
    let path=windfall_project::file::resolve_sample_path(&reopened.sample(source).unwrap().path,
        state.sample_dir.as_deref(),&rig.session.inner.factory_dir).unwrap();
    assert_eq!(std::fs::read(path).unwrap(),bytes);
    let asset=reopened.channel(channel).unwrap().sample_instrument.as_ref().unwrap();
    assert_eq!(asset.soundfont_source(),Some(source));assert!(state.pool.has_sample_instrument_asset(&asset.canonical(&reopened)));
    drop(state);
    let SamplePath::Project(relative)=&reopened.sample(source).unwrap().path else {panic!("archive bank was not portable")};
    assert_eq!(rig.session.soundfont_presets(relative).unwrap()[0].program,0);
    rig.session.soundfont_load(channel,Some(asset.clone()),relative.clone(),0,0).unwrap();
    assert_eq!(rig.project().channel(channel).unwrap().sample_instrument.as_ref().unwrap().soundfont_source(),Some(source));
    assert_eq!(rig.project().samples.len(),reopened.samples.len());
}

#[test]
fn soundfont_native_hardware_cc_changes_real_pcm_without_document_edits() {
    use windfall_engine::midi_hardware::{InputMessage,MidiMessage};
    let mut rig=Rig::new();let channel=soundfont_channel(&rig);let file=rig.file("sf-midi.sf2");
    std::fs::write(&file,general_midi_fixture()).unwrap();rig.session.soundfont_load(channel,None,file,0,0).unwrap();
    let snapshot=rig.session.document_snapshot();let generation=rig.session.midi_hardware_state().generation;
    rig.session.midi_hardware_target(Some(channel),generation,snapshot.revision).unwrap();
    let epoch=rig.session.controller().hardware_epoch();assert!(rig.session.hardware_note(epoch,60,127));
    let before=rig.session.document_snapshot();
    let baseline=rig.run(2048).iter().map(|x|x*x).sum::<f32>();assert!(baseline>0.001);
    rig.session.soundfont_hardware_message(InputMessage {device:1,at:std::time::Instant::now(),stamp_micros:0,
        message:MidiMessage::Controller{channel:0,number:7,value:0}});
    assert!(rig.run(2048).iter().map(|x|x*x).sum::<f32>()>0.001);
    rig.session.soundfont_hardware_message(InputMessage {device:0,at:std::time::Instant::now(),stamp_micros:0,
        message:MidiMessage::Controller{channel:0,number:7,value:0}});
    let latency=rig.session.controller().latency_frames() as usize;
    rig.run(latency+512);
    let quiet=rig.run(2048).iter().map(|x|x*x).sum::<f32>();
    assert!(quiet<baseline*0.0001);
    assert_eq!(rig.session.document_snapshot(),before);
}

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
    let request = rig.session.sample_instrument_speech_begin(channel).unwrap();
    let before = rig.session.document_snapshot();
    rig.session
        .sample_instrument_speech_cancel(channel, request);
    assert!(
        rig.session
            .sample_instrument_speech_request(channel, None, "Hello".into(), 48.0, 1.0, request)
            .unwrap_err()
            .contains("cancelled")
    );
    assert_eq!(rig.session.document_snapshot(), before);
    let request = rig.session.sample_instrument_speech_begin(channel).unwrap();
    let hold = rig.session.hold("sample-speech:rendered");
    let job = rig.session.background(move |session| {
        session.sample_instrument_speech_request(channel, None, "Hello".into(), 48.0, 1.0, request)
    });
    hold.wait();
    rig.session
        .sample_instrument_speech_cancel(channel, request);
    hold.release();
    assert!(job.join().unwrap().unwrap_err().contains("cancelled"));
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(
        std::fs::read_dir(rig.session.store().recordings_dir().join("Text voice"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn stale_speech_cancellation_does_not_cancel_the_new_request_and_preparation_cancel_is_atomic() {
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
    let old = rig.session.sample_instrument_speech_begin(channel).unwrap();
    let request = rig.session.sample_instrument_speech_begin(channel).unwrap();
    rig.session.sample_instrument_speech_cancel(channel, old);
    let before = rig.session.document_snapshot();
    let hold = rig.session.hold("sampler:prepared");
    let job = rig.session.background(move |session| {
        session.sample_instrument_speech_request(channel, None, "Hello".into(), 48.0, 1.0, request)
    });
    hold.wait();
    rig.session
        .sample_instrument_speech_cancel(channel, request);
    hold.release();
    assert!(job.join().unwrap().unwrap_err().contains("cancelled"));
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(
        std::fs::read_dir(rig.session.store().recordings_dir().join("Text voice"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn missing_player_reopens_relinks_same_ids_and_restores_source_readiness_through_history() {
    let rig = Rig::new();
    let missing = rig.file("missing.wav");
    let source = windfall_project::SampleId(
        rig.session
            .dispatch(
                Command::AddSample {
                    name: "Missing layer".into(),
                    path: SamplePath::External(missing.clone()),
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    rig.wait_until_loaded_or_failed(source);
    let channel = ChannelId(
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
    let bank = SampleInstrumentAsset::Zones {
        zones: vec![windfall_dsp::sample_bank::SampleZone {
            sample: source.0,
            ..Default::default()
        }],
    };
    rig.session
        .dispatch(
            Command::SetSampleInstrumentAsset {
                channel,
                expected: None,
                asset: Some(bank.clone()),
            },
            None,
        )
        .unwrap();
    let file = rig.file("missing-player.windfall");
    rig.session.project_save(Some(&file)).unwrap();
    let reopened = rig.restart();
    reopened.session.project_open(&file).unwrap();
    reopened.wait_until_loaded_or_failed(source);
    assert_eq!(
        reopened
            .session
            .sample_instrument_missing_sources(channel)
            .unwrap(),
        vec![source]
    );
    let replacement = reopened.file("replacement.wav");
    let pcm = windfall_core::AudioBuffer::from_interleaved(48000, 1, vec![0.3; 4800]);
    windfall_codec::write_wav(&replacement, &pcm, windfall_codec::WavSampleFormat::Float32)
        .unwrap();
    let expected = reopened.project().sample(source).unwrap().path.clone();
    reopened
        .session
        .sample_instrument_relink_source(channel, Some(bank.clone()), source, expected, replacement)
        .unwrap();
    let linked = reopened.project();
    assert_eq!(
        linked.channel(channel).unwrap().sample_instrument,
        Some(bank.clone())
    );
    assert_eq!(
        reopened.session.state().pool.get(source).unwrap().samples(),
        pcm.samples()
    );
    assert!(
        reopened
            .session
            .sample_instrument_missing_sources(channel)
            .unwrap()
            .is_empty()
    );
    assert!(reopened.session.undo().is_some());
    reopened.wait_until_loaded_or_failed(source);
    assert_eq!(
        reopened
            .session
            .sample_instrument_missing_sources(channel)
            .unwrap(),
        vec![source]
    );
    assert_eq!(
        reopened
            .project()
            .channel(channel)
            .unwrap()
            .sample_instrument,
        Some(bank)
    );
    assert!(reopened.session.redo().is_some());
    assert_eq!(reopened.project(), linked);
    assert_eq!(
        reopened.session.state().pool.get(source).unwrap().samples(),
        pcm.samples()
    );
}

#[test]
fn stale_source_relink_cannot_change_a_replaced_project() {
    let rig = Rig::new();
    let source = windfall_project::SampleId(
        rig.session
            .dispatch(
                Command::AddSample {
                    name: "Layer".into(),
                    path: SamplePath::External(rig.file("missing.wav")),
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    rig.wait_until_loaded_or_failed(source);
    let channel = ChannelId(
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
    let bank = SampleInstrumentAsset::Zones {
        zones: vec![windfall_dsp::sample_bank::SampleZone {
            sample: source.0,
            ..Default::default()
        }],
    };
    rig.session
        .dispatch(
            Command::SetSampleInstrumentAsset {
                channel,
                expected: None,
                asset: Some(bank.clone()),
            },
            None,
        )
        .unwrap();
    let path = rig.file("relink.wav");
    windfall_codec::write_wav(
        &path,
        &windfall_core::AudioBuffer::from_interleaved(48000, 1, vec![0.2; 480]),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
    let expected = rig.project().sample(source).unwrap().path.clone();
    let hold = rig.session.hold("sample-relink:decoded");
    let job = rig.session.background(move |session| {
        session.sample_instrument_relink_source(channel, Some(bank), source, expected, path)
    });
    hold.wait();
    rig.session.project_new().unwrap();
    let before = rig.session.document_snapshot();
    hold.release();
    assert!(job.join().unwrap().is_err());
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn project_controller_destination_reopens_without_changing_application_midi_settings() {
    let rig = Rig::new();
    let id = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::MidiOut),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let application = rig.session.midi_hardware_state().settings;
    let before = rig.project();
    let mut params = InstrumentKind::MidiOut.default_params();
    if let windfall_dsp::InstrumentParams::MidiOut(params) = &mut params {
        params.feedback_enabled = true;
        params.feedback_controller = 1;
        params.feedback_motor = 2;
        params.feedback_strength = 0.7;
        params.feedback_curve = 0.5;
    }
    rig.session
        .dispatch(
            Command::SetInstrumentParams {
                channel: id,
                params,
            },
            None,
        )
        .unwrap();
    let saved = rig.project();
    rig.session.undo().unwrap();
    assert_eq!(rig.project(), before);
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), saved);
    let path = rig.file("project-feedback.windfall");
    rig.session.project_save(Some(&path)).unwrap();
    assert_eq!(rig.session.midi_hardware_state().settings, application);
    let reopened = rig.restart();
    reopened.session.project_open(&path).unwrap();
    assert_eq!(
        reopened.project().channel(id).unwrap().source,
        saved.channel(id).unwrap().source
    );
    assert_eq!(reopened.session.midi_hardware_state().settings, application);
}

fn general_midi_fixture() -> Vec<u8> {
    fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        if !data.len().is_multiple_of(2) {
            out.push(0);
        }
        out
    }
    fn list(id: &[u8; 4], leaves: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
        let mut data = id.to_vec();
        for (tag, bytes) in leaves {
            data.extend(chunk(&tag, &bytes));
        }
        chunk(b"LIST", &data)
    }
    fn put16(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut phdr = vec![0; 76];
    phdr[..6].copy_from_slice(b"Own GM");
    phdr[38..41].copy_from_slice(b"EOP");
    put16(&mut phdr, 38 + 24, 1);
    let mut inst = vec![0; 44];
    inst[..4].copy_from_slice(b"Tone");
    inst[22..25].copy_from_slice(b"EOI");
    put16(&mut inst, 22 + 20, 1);
    let mut shdr = vec![0; 92];
    shdr[..4].copy_from_slice(b"Wave");
    shdr[46..49].copy_from_slice(b"EOS");
    put32(&mut shdr, 24, 8192);
    put32(&mut shdr, 28, 128);
    put32(&mut shdr, 32, 4096);
    put32(&mut shdr, 36, 48000);
    shdr[40] = 60;
    put16(&mut shdr, 44, 1);
    let records = |values: &[(u16, u16)]| {
        values
            .iter()
            .flat_map(|(a, b)| a.to_le_bytes().into_iter().chain(b.to_le_bytes()))
            .collect::<Vec<_>>()
    };
    let mut data = b"sfbk".to_vec();
    data.extend(list(b"INFO", vec![(*b"ifil", vec![2, 0, 4, 0])]));
    data.extend(list(
        b"sdta",
        vec![(
            *b"smpl",
            (0..8192 + 46)
                .flat_map(|frame| {
                    if frame < 8192 {
                        6000i16.to_le_bytes()
                    } else {
                        0i16.to_le_bytes()
                    }
                })
                .collect(),
        )],
    ));
    data.extend(list(
        b"pdta",
        vec![
            (*b"phdr", phdr),
            (*b"pbag", records(&[(0, 0), (1, 0)])),
            (*b"pmod", vec![0; 10]),
            (*b"pgen", records(&[(41, 0), (0, 0)])),
            (*b"inst", inst),
            (*b"ibag", records(&[(0, 0), (2, 0)])),
            (*b"imod", vec![0; 10]),
            (*b"igen", records(&[(54, 1), (53, 0), (0, 0)])),
            (*b"shdr", shdr),
        ],
    ));
    chunk(b"RIFF", &data)
}

#[test]
fn general_midi_soundfont_destination_preserves_rack_notes_history_disk_and_live_export_pcm() {
    use windfall_project::{ChannelPatch, NoteInit, SampleInstrumentAsset};
    let mut rig = Rig::new();
    for channel in rig.project().channels {
        rig.session
            .dispatch(
                Command::UpdateChannel {
                    id: channel.id,
                    patch: ChannelPatch {
                        muted: Some(true),
                        ..Default::default()
                    },
                },
                None,
            )
            .unwrap();
    }
    let channel = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::MidiOut),
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
            Command::AddNotes {
                pattern: rig.project().patterns[0].id,
                channel,
                notes: vec![NoteInit {
                    start: 0,
                    length: 960,
                    key: 60,
                    velocity: Some(1.0),
                    pan: None,
                    expression: None,
                }],
            },
            None,
        )
        .unwrap();
    let before = rig.project();
    let settings = rig.session.midi_hardware_state().settings;
    let path = rig.file("own-gm.sf2");
    std::fs::write(&path, general_midi_fixture()).unwrap();
    rig.session
        .sample_instrument_general_midi(
            channel,
            before.channel(channel).unwrap().source.clone(),
            path.clone(),
            0,
            0,
        )
        .unwrap();
    let saved = rig.project();
    assert_eq!(
        saved.channel(channel).unwrap().mixer_track,
        before.channel(channel).unwrap().mixer_track
    );
    assert_eq!(saved.patterns, before.patterns);
    assert!(matches!(
        saved.channel(channel).unwrap().sample_instrument,
        Some(SampleInstrumentAsset::SoundFont {
            source: Some(_),
            bank: 0,
            program: 0,
            ..
        })
    ));
    let pool = rig.session.state().pool.clone();
    let render = |block| {
        windfall_engine::render(
            &saved,
            &pool,
            &windfall_engine::RenderOptions {
                block_frames: block,
                ..Default::default()
            },
            &mut |_| true,
        )
    };
    let offline = render(64);
    assert!(offline.samples().iter().any(|x| x.abs() > 0.01));
    assert_eq!(offline.samples(), render(7).samples());
    rig.session.controller().play();
    let latency = rig.session.controller().latency_frames() as usize;
    let live = rig.run(36000 + latency);
    assert_eq!(
        &live[latency * 2..(latency + 36000) * 2],
        &offline.samples()[..72000]
    );
    rig.session.undo().unwrap();
    assert_eq!(rig.project(), before);
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), saved);
    assert_eq!(rig.session.midi_hardware_state().settings, settings);
    let file = rig.file("gm.windfall");
    rig.session.project_save(Some(&file)).unwrap();
    let reopened = rig.restart();
    reopened.session.project_open(&file).unwrap();
    assert_eq!(
        reopened
            .project()
            .channel(channel)
            .unwrap()
            .sample_instrument,
        saved.channel(channel).unwrap().sample_instrument
    );
}

#[test]
fn malformed_and_stale_general_midi_banks_leave_the_original_destination_unchanged() {
    let rig = Rig::new();
    let channel = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::MidiOut),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let expected = rig.project().channel(channel).unwrap().source.clone();
    let before = rig.session.document_snapshot();
    let path = rig.file("bad.sf2");
    std::fs::write(&path, b"not sf2").unwrap();
    assert!(
        rig.session
            .sample_instrument_general_midi(channel, expected.clone(), path, 0, 0)
            .is_err()
    );
    assert_eq!(rig.session.document_snapshot(), before);
    let path = rig.file("gm.sf2");
    std::fs::write(&path, general_midi_fixture()).unwrap();
    let hold = rig.session.hold("general-midi:prepared");
    let job = rig.session.background(move |session| {
        session.sample_instrument_general_midi(channel, expected, path, 0, 0)
    });
    hold.wait();
    rig.session.project_new().unwrap();
    let before = rig.session.document_snapshot();
    hold.release();
    assert!(job.join().unwrap().is_err());
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn encoded_soundfont_read_is_admitted_before_allocation_and_releases_its_lease() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("workspace.sf2");
    std::fs::write(&path, [7u8; 32]).unwrap();
    let budget = std::sync::Arc::new(windfall_dsp::sample_bank::BankBudget::new(32));
    let occupied = budget.reserve(1).unwrap();
    assert!(
        super::super::sample_instruments::read_soundfont(&path, &budget)
            .unwrap_err()
            .contains("memory refused")
    );
    assert_eq!(budget.used(), 1);
    drop(occupied);
    let (bytes, lease) = super::super::sample_instruments::read_soundfont(&path, &budget).unwrap();
    assert_eq!(bytes, vec![7; 32]);
    assert_eq!(budget.used(), 32);
    assert!(budget.reserve(1).is_err());
    drop(bytes);
    drop(lease);
    assert_eq!(budget.used(), 0);
}
