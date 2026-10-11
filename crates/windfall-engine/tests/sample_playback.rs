//! Written public live/offline and asset/history acceptance, no device needed.
use windfall_core::AudioBuffer;
use windfall_dsp::granular::{PlaybackCurve, PlaybackPoint, SliceRegion};
use windfall_engine::{Processor, RenderOptions, SamplePool, render};
use windfall_project::*;
fn fixture(kind: InstrumentKind) -> (Document, ChannelId, SamplePool) {
    let mut d = Document::new(Project::new("slice grain curve acceptance"));
    let s = SampleId(
        d.dispatch(
            Command::AddSample {
                name: "Authored source".into(),
                path: SamplePath::Factory("User/acceptance.wav".into()),
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let c = ChannelId(
        d.dispatch(
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
    let mut pool = SamplePool::new();
    pool.insert(
        s,
        AudioBuffer::from_interleaved(
            48000,
            1,
            (0..96000)
                .map(|i| ((i as f32 * 0.021).sin() * 0.4) + (i as f32 / 96000.0 * 0.1))
                .collect(),
        ),
    );
    d.dispatch(
        Command::SetSamplePlayback {
            channel: c,
            asset: Some(SamplePlayback {
                sample: Some(s),
                slices: vec![SliceRegion::default()],
                ..Default::default()
            }),
        },
        None,
    )
    .unwrap();
    d.dispatch(
        Command::AddNotes {
            pattern: d.project().patterns[0].id,
            channel: c,
            notes: vec![NoteInit {
                start: 0,
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
    (d, c, pool)
}
fn pcm(d: &Project, pool: &SamplePool, block: usize) -> Vec<f32> {
    let audio = render(
        d,
        pool,
        &RenderOptions {
            block_frames: block,
            ..Default::default()
        },
        &mut |_| true,
    );
    assert!(audio.samples().iter().all(|v| v.is_finite()));
    audio.samples().to_vec()
}
#[test]
fn every_sample_instrument_live_offline_history_and_reopen_pcm_match() {
    for kind in [
        InstrumentKind::SliceMap,
        InstrumentKind::SliceDeck,
        InstrumentKind::GrainCloud,
        InstrumentKind::WaveRide,
    ] {
        let (mut d, c, pool) = fixture(kind);
        let baseline = pcm(d.project(), &pool, 64);
        assert!(baseline.iter().any(|v| v.abs() > 1e-4));
        let mut changed = d
            .project()
            .channel(c)
            .unwrap()
            .sample_playback
            .clone()
            .unwrap();
        if kind == InstrumentKind::WaveRide {
            changed.curve = PlaybackCurve {
                points: vec![
                    PlaybackPoint {
                        time: 0.0,
                        value: 1.0,
                    },
                    PlaybackPoint {
                        time: 1.0,
                        value: 0.0,
                    },
                ],
                ..Default::default()
            };
        } else if kind == InstrumentKind::GrainCloud {
            let mut params = kind.default_params();
            params.set(4, 0.0);
            d.dispatch(Command::SetInstrumentParams { channel: c, params }, None)
                .unwrap();
        } else {
            changed.slices[0].start = 0.25;
            changed.slices[0].reverse = true;
        }
        if kind != InstrumentKind::GrainCloud {
            d.dispatch(
                Command::SetSamplePlayback {
                    channel: c,
                    asset: Some(changed),
                },
                None,
            )
            .unwrap();
        }
        let edited = pcm(d.project(), &pool, 7);
        assert_ne!(baseline, edited);
        d.undo().unwrap();
        assert_eq!(baseline, pcm(d.project(), &pool, 127));
        d.redo().unwrap();
        let reopened = file::from_json(&file::to_json(d.project()).unwrap()).unwrap();
        assert_eq!(edited, pcm(&reopened, &pool, 256));
        let (mut processor, controller) = Processor::new(48000);
        controller.set_project(&reopened, &pool);
        controller.play();
        let latency = controller.latency_frames() as usize;
        let mut live = vec![0.0; (36000 + latency) * 2];
        for block in live.chunks_mut(254) {
            processor.process(block);
        }
        assert_eq!(
            &live[latency * 2..(latency + 36000) * 2],
            &edited[..36000 * 2],
            "{kind:?}"
        );
    }
}
#[test]
fn source_reload_and_asset_edits_rebuild_same_kind_without_stale_asset_reuse() {
    let (mut d, c, mut pool) = fixture(InstrumentKind::SliceDeck);
    let (mut processor, controller) = Processor::new(48000);
    controller.set_project(d.project(), &pool);
    controller.play();
    processor.process(&mut [0.0; 1024]);
    let mut changed = d
        .project()
        .channel(c)
        .unwrap()
        .sample_playback
        .clone()
        .unwrap();
    changed.slices[0].gain = 0.0;
    d.dispatch(
        Command::SetSamplePlayback {
            channel: c,
            asset: Some(changed),
        },
        None,
    )
    .unwrap();
    controller.set_project(d.project(), &pool);
    processor.process(&mut [0.0; 1024]);
    controller.seek(0.0);
    processor.process(&mut [0.0; 1024]); // drain the documented 4 ms outgoing-note fade
    let mut silence = [1.0; 1024];
    processor.process(&mut silence);
    assert!(silence.iter().all(|v| v.abs() < 1e-5));
    d.undo().unwrap();
    let s = d
        .project()
        .channel(c)
        .unwrap()
        .sample_playback
        .as_ref()
        .unwrap()
        .sample
        .unwrap();
    pool.insert(s, AudioBuffer::from_interleaved(48000, 1, vec![0.0; 96000]));
    controller.set_project(d.project(), &pool);
    processor.process(&mut [0.0; 1024]);
    controller.seek(0.0);
    processor.process(&mut silence);
    assert!(silence.iter().all(|v| v.abs() < 1e-5));
}
#[test]
fn release_is_silent_at_export_tail_and_missing_sources_are_silent() {
    let (d, _, pool) = fixture(InstrumentKind::GrainCloud);
    let pcm = pcm(d.project(), &pool, 73);
    assert!(pcm[pcm.len() - 1024..].iter().all(|v| *v == 0.0));
    assert!(
        self::pcm(d.project(), &SamplePool::new(), 17)
            .iter()
            .all(|v| *v == 0.0)
    );
}
