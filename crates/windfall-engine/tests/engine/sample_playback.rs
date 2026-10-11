//! Prepared immutable assets must survive callback adoption without allocator calls.
use super::{realtime::allocator_calls, support::Rig};
use windfall_core::AudioBuffer;
use windfall_dsp::granular::{PlaybackCurve, PlaybackPoint};
use windfall_project::{ChannelSource, InstrumentKind, SamplePlayback};

#[test]
fn prepared_assets_adopt_active_notes_and_retire_without_callback_allocation_or_free() {
    for kind in [
        InstrumentKind::SliceMap,
        InstrumentKind::SliceDeck,
        InstrumentKind::GrainCloud,
        InstrumentKind::WaveRide,
    ] {
        let mut rig = Rig::new();
        let channel = rig.channel(AudioBuffer::from_interleaved(
            48000,
            2,
            (0..192000)
                .map(|i| (i as f32 * 0.013).sin() * 0.2)
                .collect(),
        ));
        let sample = rig.channel_mut(channel).source.sample().unwrap();
        let c = rig.channel_mut(channel);
        c.source = ChannelSource::Instrument {
            params: kind.default_params(),
            asset: None,
        };
        c.sample_playback = Some(SamplePlayback {
            sample: Some(sample),
            ..Default::default()
        });
        let (mut processor, controller) = rig.processor(48000);
        let mut out = [0.0; 1024];
        controller.note_on(channel, 60, 0.8);
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        assert!(out.iter().any(|v| v.abs() > 1e-5), "{kind:?}");
        for round in 0..6 {
            let asset = rig.channel_mut(channel).sample_playback.as_mut().unwrap();
            asset.slices[0].reverse = round % 2 == 0;
            asset.slices[0].gain = 0.7;
            asset.curve = PlaybackCurve {
                points: vec![
                    PlaybackPoint {
                        time: 0.0,
                        value: 0.8,
                    },
                    PlaybackPoint {
                        time: 1.0,
                        value: 0.2,
                    },
                ],
                ..Default::default()
            };
            if round == 3 {
                rig.pool.insert(
                    sample,
                    AudioBuffer::from_interleaved(
                        32000,
                        1,
                        (0..64000).map(|i| (i as f32 * 0.019).sin() * 0.1).collect(),
                    ),
                );
            }
            // Preparation and control-side retirement are deliberately outside the watch.
            controller.set_project(&rig.project, &rig.pool);
            assert_eq!(
                allocator_calls(|| processor.process(&mut out)),
                0,
                "{kind:?} round {round}"
            );
            controller.frame();
        }
        controller.note_off(channel, 60);
        for _ in 0..110 {
            assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        }
        assert!(out.iter().all(|v| *v == 0.0), "{kind:?}");
        controller.stop();
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        controller.frame();
    }
}
