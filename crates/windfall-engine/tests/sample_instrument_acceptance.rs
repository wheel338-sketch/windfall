//! Written-only public document/history/disk/live/offline sample-bank cases.
use windfall_core::AudioBuffer;
use windfall_dsp::sample_bank::SampleZone;
use windfall_engine::{Processor, RenderOptions, SamplePool, render};
use windfall_project::*;

fn output(project: &Project, pool: &SamplePool, block: usize) -> Vec<f32> {
    let audio = render(
        project,
        pool,
        &RenderOptions {
            block_frames: block,
            ..Default::default()
        },
        &mut |_| true,
    );
    assert!(audio.frames() > 0);
    audio.samples().to_vec()
}
#[test]
fn mapped_layers_reach_history_disk_and_equal_live_offline_pcm() {
    let mut doc = Document::new(Project::new("Sample bank acceptance"));
    let source = SampleId(
        doc.dispatch(
            Command::AddSample {
                name: "Own sine".into(),
                path: SamplePath::External("C:/Own/sine.wav".into()),
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let channel = ChannelId(
        doc.dispatch(
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
    let zones = vec![SampleZone {
        sample: source.0,
        root_key: 60,
        ..Default::default()
    }];
    doc.dispatch(
        Command::EditSampleZones {
            channel,
            expected: vec![],
            zones: zones.clone(),
        },
        None,
    )
    .unwrap();
    doc.dispatch(
        Command::AddNotes {
            pattern: doc.project().patterns[0].id,
            channel,
            notes: vec![NoteInit {
                start: 240,
                length: 1920,
                key: 60,
                velocity: Some(1.0),
                pan: None,
                expression: None,
            }],
        },
        None,
    )
    .unwrap();
    let mut pool = SamplePool::new();
    let pcm = (0..96000)
        .map(|i| (std::f32::consts::TAU * 440.0 * i as f32 / 48000.0).sin() * 0.25)
        .collect();
    pool.insert(source, AudioBuffer::from_interleaved(48000, 1, pcm));
    let baseline = output(doc.project(), &pool, 64);
    assert!(baseline.iter().any(|x| x.abs() > 0.05));
    let changed = vec![SampleZone {
        gain: 0.25,
        ..zones[0]
    }];
    doc.dispatch(
        Command::EditSampleZones {
            channel,
            expected: zones,
            zones: changed,
        },
        None,
    )
    .unwrap();
    let edited = output(doc.project(), &pool, 7);
    assert_ne!(edited, baseline);
    doc.undo().unwrap();
    assert_eq!(output(doc.project(), &pool, 127), baseline);
    doc.redo().unwrap();
    assert_eq!(output(doc.project(), &pool, 1), edited);
    let path = std::env::temp_dir().join(format!(
        "windfall-sample-bank-{}-{}.windfall",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    file::save(doc.project(), &path).unwrap();
    let loaded = file::load(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(output(&loaded, &pool, 256), edited);
    let (mut processor, controller) = Processor::new(48000);
    controller.set_project(&loaded, &pool);
    controller.play();
    let latency = controller.latency_frames() as usize;
    let mut live = vec![0.0; (36000 + latency) * 2];
    for chunk in live.chunks_mut(256) {
        processor.process(chunk);
    }
    assert_eq!(&live[latency * 2..(latency + 36000) * 2], &edited[..72000]);
}
#[test]
fn own_piano_and_speech_recipes_are_public_instruments_with_deterministic_pcm() {
    for (kind, asset) in [
        (
            InstrumentKind::PianoKeys,
            SampleInstrumentAsset::Piano {
                electric: false,
                version: 1,
            },
        ),
        (
            InstrumentKind::TextVoice,
            SampleInstrumentAsset::Speech {
                text: "Hello windfall".into(),
                pitch: 48.0,
                duration: 1.0,
                version: 1,
                source: None,
            },
        ),
    ] {
        let mut doc = Document::new(Project::new("Own source"));
        let channel = ChannelId(
            doc.dispatch(
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
        doc.dispatch(
            Command::SetSampleInstrumentAsset {
                channel,
                expected: None,
                asset: Some(asset),
            },
            None,
        )
        .unwrap();
        doc.dispatch(
            Command::AddNotes {
                pattern: doc.project().patterns[0].id,
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
        let a = output(doc.project(), &SamplePool::new(), 64);
        let b = output(doc.project(), &SamplePool::new(), 7);
        assert_eq!(a, b);
        assert!(a.iter().any(|x| x.abs() > 0.01));
    }
}

#[test]
fn candidate_banks_share_one_budget_with_retained_and_concurrent_snapshots() {
    let mut doc = Document::new(Project::new("budget"));
    let sample = SampleId(doc.dispatch(Command::AddSample { name: "PCM".into(), path: SamplePath::External("C:/pcm.wav".into()) }, None).unwrap().created[0]);
    let channel = ChannelId(doc.dispatch(Command::AddChannel { name: None, sample: None, instrument: Some(InstrumentKind::MultiSampler), index: None, mixer_track: None }, None).unwrap().created[0]);
    doc.dispatch(Command::EditSampleZones { channel, expected: vec![], zones: vec![SampleZone { sample: sample.0, ..Default::default() }] }, None).unwrap();
    let mut pool = SamplePool::with_sample_bank_budget(128);
    pool.insert(sample, AudioBuffer::from_interleaved(48000, 1, vec![0.2; 16]));
    let first = pool.prepare_sample_instruments(doc.project(), &mut || true).unwrap();
    let mut edited = doc.project().clone();
    let Some(SampleInstrumentAsset::Zones { zones }) = &mut edited.channels.iter_mut().find(|c| c.id == channel).unwrap().sample_instrument else { panic!() };
    zones[0].gain = 0.5;
    let second = first.prepare_sample_instruments(&edited, &mut || true).unwrap();
    assert_eq!(pool.sample_bank_retained_bytes(), 128);
    let error = pool.prepare_sample_instruments(&edited, &mut || true).unwrap_err();
    assert!(error.contains("memory refused"));
    assert_eq!(pool.sample_bank_retained_bytes(), 128, "failed candidate rolled back");
    let same = second.prepare_sample_instruments(&edited, &mut || true).unwrap();
    assert_eq!(pool.sample_bank_retained_bytes(), 128, "cached owners aren't charged twice");
    drop(same); drop(second);
    assert_eq!(pool.sample_bank_retained_bytes(), 64);
    drop(first);
    assert_eq!(pool.sample_bank_retained_bytes(), 0);
    let mut other = SamplePool::new(); other.share_sampler_budget(&pool);
    assert_eq!(other.sample_bank_retained_bytes(), 0);
}
