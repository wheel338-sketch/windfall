//! Real mixer hosting and capture of nonlinear and buffered processors.
use crate::support::Rig;
use windfall_core::AudioBuffer;
use windfall_dsp::{EffectKind, EffectParams};
use windfall_engine::{RenderOptions, render};
use windfall_ipc::PlayMode;

/// Export removes PDC latency; align the live capture by the same reported
/// latency, retaining exact PCM equality and callback allocation checks.
fn aligned_song(rig: &Rig, frames: usize, block: usize) -> Vec<f32> {
    let (mut processor, controller) = rig.song_processor(48_000);
    let latency = controller.latency_frames() as usize;
    controller.play();
    let mut output = vec![0.0; (frames + latency) * 2];
    for chunk in output.chunks_mut(block * 2) {
        assert_eq!(
            crate::realtime::allocator_calls(|| processor.process(chunk)),
            0
        );
    }
    output[latency * 2..].to_vec()
}

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
            aligned_song(&rig, captured.frames(), 137),
            "{kind:?}"
        );
        for block in [1, 7, 64, 512] {
            assert_eq!(
                captured.samples(),
                aligned_song(&rig, captured.frames(), block),
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

#[test]
fn scratch_source_reload_reuses_project_ids_without_redirecting_another_engine() {
    use windfall_dsp::performance::ScratchParams;
    let fixture = |value| {
        let mut rig = Rig::new();
        let track = rig.track();
        let sample = rig.sample(AudioBuffer::from_interleaved(48_000, 1, vec![value; 2048]));
        let channel = rig.channel_on(
            AudioBuffer::from_interleaved(48_000, 1, vec![0.0; 8000]),
            track,
        );
        rig.steps(channel, &[0]);
        rig.effect(
            track,
            EffectParams::Scratch(ScratchParams {
                source_id: sample.0,
                position: 0.5,
                ..Default::default()
            }),
        );
        rig
    };
    let original = fixture(0.2);
    let replacement = fixture(-0.4);
    let (mut processor, controller) = original.processor(48_000);
    controller.play();
    let mut block = [0.0; 512];
    for _ in 0..16 {
        processor.process(&mut block);
    }
    assert!(block.iter().all(|x| (*x - 0.2).abs() < 1e-6));
    controller.set_project(&replacement.project, &replacement.pool);
    for _ in 0..16 {
        assert_eq!(
            crate::realtime::allocator_calls(|| processor.process(&mut block)),
            0
        );
    }
    assert!(block.iter().all(|x| (*x + 0.4).abs() < 1e-6));
    let again = original.play(48_000, 4096, 137);
    assert!(again[2048..].iter().all(|x| (*x - 0.2).abs() < 1e-6));
}

#[test]
fn recorded_trigger_edges_replay_the_exact_history_in_live_and_offline_capture() {
    use windfall_project::AutomationTarget;
    for kind in [EffectKind::VolumeGate, EffectKind::TimeTransport] {
        let mut rig = Rig::new();
        let track = rig.track();
        let source: Vec<f32> = (0..48_000).map(|n| (n % 997) as f32 * 0.0005).collect();
        let lane = rig.playlist_track();
        rig.audio_clip(
            lane,
            AudioBuffer::from_interleaved(48_000, 1, source.clone()),
            track,
            0,
            1920,
        );
        let mut params = kind.default_params();
        let loop_index = kind
            .descriptors()
            .iter()
            .position(|d| d.id == "loopBeats")
            .unwrap();
        params.set(loop_index, 0.125);
        let effect = rig.effect(track, params);
        let trigger_index = kind
            .descriptors()
            .iter()
            .position(|d| d.id == "trigger")
            .unwrap();
        let target = AutomationTarget::EffectParam {
            track,
            effect,
            param: trigger_index as u32,
        };
        // These edges are on the existing engine's 64-frame automation grid.
        // Effect parameters are aimed at the NEXT grid value, so boolean
        // controls take effect one grid step before the curve's edge.
        // A MIDI-recorded Trigger lane uses this same saved automation path.
        let automation = rig.automation(
            target,
            &[(0, 0.0), (256, 0.0), (256, 1.0), (768, 1.0), (768, 0.0)],
        );
        let control_lane = rig.playlist_track();
        rig.automation_clip(control_lane, automation, 0, 1920);
        let options = RenderOptions {
            sample_rate: 48_000,
            mode: PlayMode::Song,
            block_frames: 101,
            tail_secs: 0.0,
            ..Default::default()
        };
        let captured = render(&rig.project, &rig.pool, &options, &mut |_| true);
        let output = captured.samples();
        let start = 6400 - 64;
        let end = 19200 - 64;
        let mut previous = source[4999];
        let mut transition_from = previous;
        let mut transition_left = 0;
        // Independent captured-slice + two-ms endpoint blend reference,
        // including every wrap and the release rather than omitting seams.
        for frame in 5000..40000 {
            let held = (start..end).contains(&frame);
            if frame == end || (held && (frame - start) % 3000 == 0) {
                transition_from = previous;
                transition_left = 96;
            }
            let next = if held {
                source[start - 3000 + (frame - start) % 3000]
            } else {
                source[frame]
            };
            let expected = if transition_left > 0 {
                transition_left -= 1;
                let mix = 1.0 - transition_left as f32 / 96.0;
                transition_from * (1.0 - mix) + next * mix
            } else {
                next
            };
            assert!(
                (output[frame * 2] - expected).abs() < 1e-6,
                "{kind:?} frame {frame}: {} != {expected}",
                output[frame * 2]
            );
            assert!((output[frame * 2 + 1] - expected).abs() < 1e-6);
            previous = expected;
        }
        for block in [1, 7, 64, 137, 512] {
            assert_eq!(
                output,
                aligned_song(&rig, captured.frames(), block),
                "{kind:?} block {block}"
            );
        }
    }
}
