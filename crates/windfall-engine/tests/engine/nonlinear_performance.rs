//! Real mixer hosting and capture of nonlinear and buffered processors.
use crate::support::Rig;
use windfall_core::AudioBuffer;
use windfall_dsp::{EffectKind, EffectParams};
use windfall_engine::{RenderOptions, render};
use windfall_ipc::PlayMode;

#[test]
fn all_eight_processors_capture_the_same_pcm_live_offline_and_under_block_partitions() {
    for kind in [
        EffectKind::Overdrive,
        EffectKind::Waveshaper,
        EffectKind::GuitarRack,
        EffectKind::DriveChain,
        EffectKind::VolumeGate,
        EffectKind::TimeTransport,
        EffectKind::PerformanceRack,
        EffectKind::Scratch,
    ] {
        let mut rig = Rig::new();
        let track = rig.track();
        let source = AudioBuffer::from_interleaved(
            48_000,
            2,
            (0..48_000)
                .flat_map(|n| {
                    let t = n as f64 / 48_000.0;
                    [
                        (0.25 * (std::f64::consts::TAU * 431.0 * t).sin()) as f32,
                        (0.15 * (std::f64::consts::TAU * 719.0 * t).sin()) as f32,
                    ]
                })
                .collect(),
        );
        let scratch_sample = rig.sample(source.clone());
        let channel = rig.channel_on(source, track);
        rig.steps(channel, &[0]);
        let curve = windfall_dsp::asset::CurveRef::new(&[
            windfall_dsp::asset::CurvePoint::new(0.0, 1.0),
            windfall_dsp::asset::CurvePoint::new(0.5, 0.25),
            windfall_dsp::asset::CurvePoint::new(1.0, 0.0),
        ])
        .unwrap();
        let mut params = kind.default_params();
        match &mut params {
            EffectParams::VolumeGate(p) => {
                p.time_curve = curve;
                p.volume_curve = curve;
                p.loop_beats = 0.25;
            }
            EffectParams::TimeTransport(p) => {
                p.auto_trigger = true;
                p.threshold_db = -30.0;
                p.loop_beats = 0.25;
            }
            EffectParams::Scratch(p) => {
                p.source_id = scratch_sample.0;
                p.position_curve = curve;
                p.curve_enabled = true;
                p.loop_beats = 0.25;
            }
            EffectParams::PerformanceRack(p) => {
                p.xy_enabled = true;
                p.x = 0.7;
                p.y = 0.2;
                p.loop_beats = 0.25;
                p.model = windfall_dsp::performance::PerformanceModel::Reverse;
            }
            EffectParams::GuitarRack(p) => {
                for i in [1, 5, 10] {
                    p.pedals[i].on = true;
                }
            }
            EffectParams::DriveChain(p) => {
                p.stages[0].model = windfall_dsp::drive::DriveStage::Fuzz;
                p.stages[1].model = windfall_dsp::drive::DriveStage::Chorus;
                p.stages[2].model = windfall_dsp::drive::DriveStage::Cabinet;
            }
            _ => {}
        }
        rig.effect(track, params);
        let lane = rig.playlist_track();
        rig.clip(lane, rig.first_pattern(), 0, 960);
        let options = RenderOptions {
            sample_rate: 48_000,
            mode: PlayMode::Song,
            block_frames: 101,
            tail_secs: 0.0,
            ..Default::default()
        };
        let captured = render(&rig.project, &rig.pool, &options, &mut |_| true);
        assert!(captured.samples().iter().all(|x| x.is_finite()), "{kind:?}");
        assert_eq!(
            captured.samples(),
            rig.play_song(48_000, captured.frames(), 137),
            "{kind:?}"
        );
        for block in [1, 7, 64, 512] {
            assert_eq!(
                captured.samples(),
                rig.play_song(48_000, captured.frames(), block),
                "{kind:?} block {block}"
            );
        }
        let (mut processor, controller) = rig.song_processor(48_000);
        controller.play();
        let mut block = [0.0; 254];
        for _ in 0..512 {
            assert_eq!(
                crate::realtime::allocator_calls(|| processor.process(&mut block)),
                0,
                "{kind:?}"
            );
        }
    }
}
