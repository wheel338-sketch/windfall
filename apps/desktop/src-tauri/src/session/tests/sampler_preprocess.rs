//! Sample pre-processing through the session: a real file in, a new real
//! file out, one undo step, save and reopen, and refusals that change
//! nothing.
use super::Rig;
use windfall_core::AudioBuffer;
use windfall_engine::sample_preprocess::{LoopSeam, SamplePreprocess, preprocess};
use windfall_ipc::SamplerPreprocessRequest;
use windfall_project::{
    ChannelId, ChannelSource, Command, InstrumentKind, SampleId, SamplePath, SamplerLoopMode,
    SamplerPatch, SamplerSettings, TrackId,
};

fn request(channel: ChannelId) -> SamplerPreprocessRequest {
    SamplerPreprocessRequest {
        channel,
        trim: None,
        reverse: None,
        remove_dc: None,
        invert_polarity: None,
        swap_stereo: None,
        fade_in_ms: None,
        fade_out_ms: None,
        loop_crossfade: None,
        normalize: None,
    }
}

/// A quiet 480 Hz sine riding on a DC offset: 2000 mono frames.
fn tone() -> AudioBuffer {
    let data = (0..2000)
        .map(|frame| {
            let phase = frame as f64 / 100.0 * std::f64::consts::TAU;
            (phase.sin() * 0.25 + 0.1) as f32
        })
        .collect();
    AudioBuffer::from_interleaved(48_000, 1, data)
}

/// Adds a sampler channel that plays a real WAV file in the test's folder.
fn add(rig: &Rig) -> (ChannelId, SampleId, String, Vec<u8>) {
    let path = rig.file("tone.wav");
    windfall_codec::write_wav(&path, &tone(), windfall_codec::WavSampleFormat::Float32).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    rig.session.add_channel_from_file(&path, None).unwrap();
    let project = rig.project();
    let channel = project.channels.last().unwrap();
    let sample = sampler_of(channel).sample.unwrap();
    rig.wait_until_loaded_or_failed(sample);
    assert!(rig.has_audio(sample));
    (channel.id, sample, path, bytes)
}

fn sampler_of(channel: &windfall_project::Channel) -> &SamplerSettings {
    match &channel.source {
        ChannelSource::Sampler(settings) => settings,
        ChannelSource::Instrument { .. } => panic!("the channel is not a sampler"),
    }
}

fn sampler(rig: &Rig, channel: ChannelId) -> SamplerSettings {
    sampler_of(rig.project().channel(channel).unwrap()).clone()
}

fn audio(rig: &Rig, sample: SampleId) -> AudioBuffer {
    rig.session.state().pool.get(sample).unwrap().clone()
}

fn processed_files(rig: &Rig) -> usize {
    let folder = rig.folder.path().join("recordings").join("Sample edits");
    std::fs::read_dir(folder).map_or(0, |entries| entries.count())
}

fn patch(rig: &Rig, channel: ChannelId, patch: SamplerPatch) {
    rig.session
        .dispatch(Command::UpdateSampler { id: channel, patch }, None)
        .unwrap();
}

#[test]
fn preprocess_renders_a_new_sample_as_one_undo_step_and_survives_save_and_reopen() {
    let rig = Rig::new();
    let (channel, sample, path, original) = add(&rig);
    patch(
        &rig,
        channel,
        SamplerPatch {
            start: Some(0.1),
            end: Some(0.9),
            loop_mode: Some(SamplerLoopMode::Forward),
            loop_start: Some(0.25),
            loop_end: Some(0.55),
            loop_crossfade: Some(0.3),
            ..Default::default()
        },
    );
    let before = rig.session.document_snapshot();
    let settings = sampler(&rig, channel);
    let source = audio(&rig, sample);

    let result = rig
        .session
        .sampler_preprocess(SamplerPreprocessRequest {
            trim: Some(true),
            remove_dc: Some(true),
            fade_out_ms: Some(2.0),
            loop_crossfade: Some(0.2),
            normalize: Some(true),
            ..request(channel)
        })
        .unwrap();

    // What the kernel makes of the same source and settings.
    let expected = preprocess(
        &source,
        &SamplePreprocess {
            start: settings.start,
            end: settings.end,
            trim: true,
            reversed: settings.reverse,
            remove_dc: true,
            fade_out_ms: 2.0,
            loop_crossfade: Some(LoopSeam {
                start: settings.loop_start,
                end: settings.loop_end,
                amount: 0.2,
            }),
            normalize: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(expected.audio.frames(), 1600);
    assert!(expected.crossfade_frames > 0);

    let new_sample = SampleId(result.created[0]);
    assert_ne!(new_sample, sample);
    let processed = audio(&rig, new_sample);
    assert_eq!(processed.samples(), expected.audio.samples());
    assert_eq!(processed.sample_rate(), 48_000);
    let peak = processed
        .samples()
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    assert!(
        (peak - 10.0_f32.powf(-1.0 / 20.0)).abs() < 1e-6,
        "normalized to −1 dBFS"
    );

    // One history entry: the channel now plays the new sample, the trim is
    // baked, the loop points follow the audio and the playback crossfade
    // is replaced by the blended seam. Everything else is untouched.
    let edited = rig.project();
    assert_eq!(
        rig.session.document_snapshot().history.cursor,
        before.history.cursor + 1
    );
    let (loop_start, loop_end) = expected.loop_points.unwrap();
    assert_eq!(
        sampler(&rig, channel),
        SamplerSettings {
            sample: Some(new_sample),
            start: 0.0,
            end: 1.0,
            loop_start,
            loop_end,
            loop_crossfade: 0.0,
            ..settings.clone()
        }
    );
    // The original sample stays in the project and its file is untouched.
    assert!(edited.sample(sample).is_some());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let asset = edited.sample(new_sample).unwrap();
    let SamplePath::External(written) = &asset.path else {
        panic!("a processed sample is an external file");
    };
    assert!(written.contains("Sample edits"));
    let decoded = windfall_codec::decode_file(written).unwrap();
    assert_eq!(decoded.samples(), expected.audio.samples());
    assert_eq!(processed_files(&rig), 1);

    rig.session.undo().unwrap();
    let mut restored = before.project.clone();
    restored.next_id = rig.project().next_id; // ids are never reused
    assert_eq!(rig.project(), restored);
    assert_eq!(processed_files(&rig), 1, "history keeps the file");
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), edited);
    assert_eq!(audio(&rig, new_sample).samples(), expected.audio.samples());

    let saved = rig.file("song.windfall");
    rig.session.project_save(Some(&saved)).unwrap();
    let rig = rig.restart();
    rig.session.project_open(&saved).unwrap();
    assert_eq!(sampler(&rig, channel).sample, Some(new_sample));
    rig.wait_until_loaded_or_failed(new_sample);
    assert_eq!(audio(&rig, new_sample).samples(), expected.audio.samples());
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn reversing_writes_reversed_audio_and_keeps_the_region_and_loop_on_their_frames() {
    let rig = Rig::new();
    let (channel, sample, _, _) = add(&rig);
    patch(
        &rig,
        channel,
        SamplerPatch {
            start: Some(0.25),
            end: Some(0.5),
            loop_mode: Some(SamplerLoopMode::PingPong),
            loop_start: Some(0.0),
            loop_end: Some(0.4),
            ..Default::default()
        },
    );
    let settings = sampler(&rig, channel);
    let source = audio(&rig, sample);
    let result = rig
        .session
        .sampler_preprocess(SamplerPreprocessRequest {
            reverse: Some(true),
            invert_polarity: Some(true),
            ..request(channel)
        })
        .unwrap();
    let new_sample = SampleId(result.created[0]);
    let expected: Vec<f32> = source
        .samples()
        .iter()
        .rev()
        .map(|sample| -sample)
        .collect();
    assert_eq!(audio(&rig, new_sample).samples(), expected);
    let now = sampler(&rig, channel);
    // The region [500, 1000) of 2000 frames is now [1000, 1500), and the
    // loop [0, 200) of its 500 frames is now [300, 500).
    assert_eq!((now.start, now.end), (0.5, 0.75));
    assert_eq!((now.loop_start, now.loop_end), (0.6, 1.0));
    assert_eq!(
        now.reverse, settings.reverse,
        "the switch is not a treatment"
    );
    assert_eq!(now.loop_mode, SamplerLoopMode::PingPong);
    assert_eq!(now.loop_crossfade, settings.loop_crossfade);
}

#[test]
fn refused_requests_change_nothing_and_leave_no_file() {
    let rig = Rig::new();
    let (channel, _, _, _) = add(&rig);
    let synth = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::SubtractiveSynth),
                    index: None,
                    mixer_track: Some(TrackId::MASTER),
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let empty = ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: None,
                    index: None,
                    mixer_track: Some(TrackId::MASTER),
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let before = rig.session.document_snapshot();
    let normalize = |channel| SamplerPreprocessRequest {
        normalize: Some(true),
        ..request(channel)
    };
    let refused = [
        ("no treatment", request(channel)),
        ("instrument channel", normalize(synth)),
        ("no sample", normalize(empty)),
        ("missing channel", normalize(ChannelId(999_999))),
        (
            "seam without a loop",
            SamplerPreprocessRequest {
                loop_crossfade: Some(0.2),
                ..request(channel)
            },
        ),
        (
            "mono swap",
            SamplerPreprocessRequest {
                swap_stereo: Some(true),
                ..request(channel)
            },
        ),
        (
            "negative fade",
            SamplerPreprocessRequest {
                fade_in_ms: Some(-1.0),
                ..request(channel)
            },
        ),
        (
            "endless fade",
            SamplerPreprocessRequest {
                fade_out_ms: Some(f32::INFINITY),
                ..request(channel)
            },
        ),
    ];
    for (what, request) in refused {
        assert!(rig.session.sampler_preprocess(request).is_err(), "{what}");
        assert_eq!(rig.session.document_snapshot(), before, "{what}");
        assert_eq!(processed_files(&rig), 0, "{what}");
    }

    // A ping-pong loop turns around at its ends and has no seam.
    patch(
        &rig,
        channel,
        SamplerPatch {
            loop_mode: Some(SamplerLoopMode::PingPong),
            ..Default::default()
        },
    );
    let before = rig.session.document_snapshot();
    let error = rig
        .session
        .sampler_preprocess(SamplerPreprocessRequest {
            loop_crossfade: Some(0.2),
            ..request(channel)
        })
        .unwrap_err();
    assert!(error.contains("ping-pong"), "{error}");
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(processed_files(&rig), 0);
}

#[test]
fn a_result_for_a_changed_project_is_dropped_with_its_file() {
    let rig = Rig::new();
    let (channel, _, _, _) = add(&rig);
    let hold = rig.session.hold("sampler-preprocess:rendered");
    let worker = rig.session.background(move |session| {
        session.sampler_preprocess(SamplerPreprocessRequest {
            normalize: Some(true),
            ..request(channel)
        })
    });
    hold.wait();
    // The file exists while the job is paused, before it may be attached.
    assert_eq!(processed_files(&rig), 1);
    patch(
        &rig,
        channel,
        SamplerPatch {
            gain: Some(0.5),
            ..Default::default()
        },
    );
    let edited = rig.session.document_snapshot();
    hold.release();
    let error = worker.join().unwrap().unwrap_err();
    assert!(error.contains("changed while pre-processing"), "{error}");
    assert_eq!(rig.session.document_snapshot(), edited);
    assert_eq!(processed_files(&rig), 0);

    // The same request then succeeds against the current project.
    rig.session
        .sampler_preprocess(SamplerPreprocessRequest {
            normalize: Some(true),
            ..request(channel)
        })
        .unwrap();
    assert_eq!(processed_files(&rig), 1);
    assert_eq!(sampler(&rig, channel).gain, 0.5);
}

#[test]
fn prepared_preview_has_the_apply_waveform_and_duration_without_files_or_history() {
    let rig = Rig::new();
    let (channel, _, _, _) = add(&rig);
    patch(
        &rig,
        channel,
        SamplerPatch {
            start: Some(0.1),
            end: Some(0.9),
            ..Default::default()
        },
    );
    let request = SamplerPreprocessRequest {
        trim: Some(true),
        normalize: Some(true),
        ..request(channel)
    };
    let before = rig.session.document_snapshot();
    let preview = rig.session.sampler_preprocess_preview(request).unwrap();
    assert_eq!(preview.frames, 1600);
    assert!((preview.duration_secs - 1600.0 / 48000.0).abs() < 1e-12);
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(processed_files(&rig), 0);
    let result = rig.session.sampler_preprocess(request).unwrap();
    let rendered = audio(&rig, SampleId(result.created[0]));
    assert_eq!(
        preview.peaks,
        windfall_engine::audio_edit::overview(&rendered, 512)
    );
    assert_eq!(preview.frames as usize, rendered.frames());
}

#[test]
fn a_stale_prepared_preview_is_refused_before_audition_or_history() {
    let rig = Rig::new();
    let (channel, _, _, _) = add(&rig);
    let hold = rig.session.hold("sampler-preprocess:preview");
    let worker = rig.session.background(move |session| {
        session.sampler_preprocess_preview(SamplerPreprocessRequest {
            normalize: Some(true),
            ..request(channel)
        })
    });
    hold.wait();
    rig.session.project_new().unwrap();
    let before = rig.session.document_snapshot();
    hold.release();
    assert!(worker.join().unwrap().unwrap_err().contains("changed"));
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(processed_files(&rig), 0);
}

#[test]
fn derived_collection_retains_history_saved_and_original_sources_and_removes_only_abandoned_owned_audio()
 {
    let rig = Rig::new();
    let (channel, _, original, bytes) = add(&rig);
    let make = || {
        rig.session
            .sampler_preprocess(SamplerPreprocessRequest {
                normalize: Some(true),
                ..request(channel)
            })
            .unwrap()
    };
    let first = make();
    let id = SampleId(first.created[0]);
    let SamplePath::External(derived) = rig.project().sample(id).unwrap().path.clone() else {
        panic!("external")
    };
    rig.session.undo().unwrap();
    assert_eq!(
        rig.session.collect_derived_assets().unwrap(),
        0,
        "redo root"
    );
    assert!(std::path::Path::new(&derived).exists());
    rig.session.redo().unwrap();
    assert_eq!(
        rig.session.collect_derived_assets().unwrap(),
        0,
        "live root"
    );
    let untouched = rig
        .folder
        .path()
        .join("recordings/Sample edits/unknown.wav");
    std::fs::write(&untouched, b"not owned by this session").unwrap();
    rig.session.project_new().unwrap();
    assert_eq!(rig.session.collect_derived_assets().unwrap(), 1);
    assert!(!std::path::Path::new(&derived).exists());
    assert!(untouched.exists());
    assert_eq!(std::fs::read(&original).unwrap(), bytes);

    let (channel, _, _, _) = add(&rig);
    let result = rig
        .session
        .sampler_preprocess(SamplerPreprocessRequest {
            normalize: Some(true),
            ..request(channel)
        })
        .unwrap();
    let id = SampleId(result.created[0]);
    let SamplePath::External(saved_asset) = rig.project().sample(id).unwrap().path.clone() else {
        panic!("external")
    };
    let path = rig.file("saved.windfall");
    rig.session.project_save(Some(&path)).unwrap();
    rig.session.project_new().unwrap();
    assert_eq!(
        rig.session.collect_derived_assets().unwrap(),
        0,
        "saved project pins source"
    );
    rig.session.project_open(&path).unwrap();
    rig.wait_until_loaded_or_failed(id);
    assert!(rig.has_audio(id));
    assert!(std::path::Path::new(&saved_asset).exists());
}
