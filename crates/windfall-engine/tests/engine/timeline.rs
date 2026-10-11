//! Real processor navigation and linear region rendering, without an audio device.
use windfall_engine::{RenderOptions, StemOptions, render, render_stems, render_streaming_checked};
use windfall_ipc::{PlayMode, StemMode, TransportPatch};
use windfall_project::{
    MarkerKind, MeterChangeId, TickRange, TimelineMarker, TimelineMarkerId, TrackId,
};

use crate::realtime::allocator_calls;
use crate::support::{Rig, counted, counting, left, run};

const RATE: u32 = 48_000;

#[test]
fn timeline_hosted_transport_uses_song_meters_only_in_song_mode() {
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    use windfall_engine::plugins::{
        HostedEffect, HostedInstrument, PluginFactory, PluginTransport,
    };
    use windfall_project::{EffectParams, MeterChange, PluginBinding, PluginTarget, TimeSignature};
    #[derive(Debug, Default)]
    struct Probe([AtomicU64; 9]);
    struct Unit(Arc<Probe>);
    impl HostedEffect for Unit {
        fn transport(&mut self, t: PluginTransport) {
            for (cell, value) in self.0.0.iter().zip([
                u64::from(t.playing),
                t.tempo_bpm.to_bits(),
                t.position_beats.to_bits(),
                t.position_seconds.to_bits(),
                u64::from(t.numerator),
                u64::from(t.denominator),
                u64::from(t.meter_anchor.is_some()),
                t.meter_anchor
                    .map_or(0, |anchor| anchor.bar_origin_beats.to_bits()),
                t.meter_anchor
                    .map_or(0, |anchor| u64::from(anchor.bar_origin_index)),
            ]) {
                cell.store(value, Ordering::Relaxed);
            }
        }
        fn process(&mut self, _: &mut [f32], _: &mut [f32]) {}
        fn set_param(&mut self, _: u32, _: f32) {}
        fn set_tempo(&mut self, _: f32) {}
        fn latency(&self) -> usize {
            0
        }
        fn tail(&self) -> usize {
            0
        }
    }
    impl HostedInstrument for Unit {
        fn note_on(&mut self, _: u8, _: f32) {}
        fn note_off(&mut self, _: u8) {}
        fn all_notes_off(&mut self) {}
        fn voices(&self) -> usize {
            0
        }
    }
    #[derive(Debug)]
    struct Factory(Vec<Arc<Probe>>);
    impl PluginFactory for Factory {
        fn effect(
            &self,
            binding: &PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn HostedEffect>, String> {
            Ok(Box::new(Unit(
                self.0[binding.id.parse::<usize>().unwrap()].clone(),
            )))
        }
        fn instrument(
            &self,
            binding: &PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn HostedInstrument>, String> {
            Ok(Box::new(Unit(
                self.0[binding.id.parse::<usize>().unwrap()].clone(),
            )))
        }
    }
    let mut rig = tape();
    let probes: Vec<_> = (0..4).map(|_| Arc::new(Probe::default())).collect();
    for (i, format) in ["clap", "vst3"].into_iter().enumerate() {
        let channel = rig.synth_on(Default::default(), TrackId::MASTER);
        let effect = rig.effect(TrackId::MASTER, EffectParams::Limiter(Default::default()));
        for (j, target) in [
            PluginTarget::Instrument { channel },
            PluginTarget::Effect { effect },
        ]
        .into_iter()
        .enumerate()
        {
            rig.project.plugins.push(PluginBinding {
                sidechain_input: None,
                auxiliary_inputs: Vec::new(),
                wrapper: Default::default(),
                target,
                format: format.into(),
                path: "transport-probe".into(),
                id: (i * 2 + j).to_string(),
                name: "Transport probe".into(),
                state: Vec::new(),
                parameters: Vec::new(),
            });
        }
    }
    rig.pool
        .set_plugin_factory(Arc::new(Factory(probes.clone())));
    for (tick, numerator, denominator) in [(0, 7, 8), (1001, 3, 4)] {
        let id = rig.project.next_id;
        rig.project.next_id += 1;
        rig.project.playlist.timeline.meters.push(MeterChange {
            id: MeterChangeId(id),
            tick,
            signature: TimeSignature {
                numerator,
                denominator,
            },
        });
    }
    let pattern = rig.first_pattern();
    let other_pattern = rig.pattern(16);
    let (mut processor, controller) = rig.song_processor(RATE);
    // One frame observes exactly the requested position, through actual hosted
    // instruments and effect facades for both saved format identifiers.
    for (mode, source, tick, signature) in [
        (PlayMode::Song, pattern, 0., (7, 8)),
        (PlayMode::Pattern, pattern, 0., (4, 4)),
        (PlayMode::Pattern, other_pattern, 2001., (4, 4)),
        (PlayMode::Song, other_pattern, 2001., (3, 4)),
        (PlayMode::Song, pattern, 0., (7, 8)),
    ] {
        controller.set_transport(TransportPatch {
            mode: Some(mode),
            pattern: Some(source),
            ..Default::default()
        });
        controller.seek(tick);
        controller.play();
        let mut out = [0.; 2];
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        for probe in &probes {
            let values = probe.0.each_ref().map(|v| v.load(Ordering::Relaxed));
            assert_eq!((values[4], values[5]), signature, "{mode:?} tick {tick}");
            assert_eq!(values[0], 1);
            assert_eq!(f64::from_bits(values[1]), 120.);
            assert_eq!(f64::from_bits(values[2]), tick / 960.);
            assert_eq!(f64::from_bits(values[3]), tick / 1920.);
            if mode == PlayMode::Song {
                assert_eq!(values[6], 1);
                let (origin, index) = if tick >= 1001. { (1001., 1) } else { (0., 0) };
                assert_eq!(f64::from_bits(values[7]), origin / 960.);
                assert_eq!(values[8], index);
            } else {
                // Pattern-local meters have their own authoritative origin,
                // including the default signature at tick zero.
                assert_eq!(&values[6..], &[1, 0, 0]);
            }
        }
    }

    // Independent division of the 485-tick anchor and the 3845-tick downbeat
    // must reach both hosted roles intact; their beat subtraction rounds below
    // one 7/8 bar. Observe actual engine seeks, source switches and selection.
    rig.project.playlist.timeline.meters.clear();
    let id = rig.project.next_id;
    rig.project.next_id += 1;
    rig.project.playlist.timeline.meters.push(MeterChange {
        id: MeterChangeId(id),
        tick: 485,
        signature: TimeSignature {
            numerator: 7,
            denominator: 8,
        },
    });
    rig.project.playlist.clips[0].length = 12_000;
    let (mut processor, controller) = rig.song_processor(RATE);
    for (mode, source, tick) in [
        (PlayMode::Song, pattern, 3844.),
        (PlayMode::Song, pattern, 3845.),
        (PlayMode::Song, other_pattern, 7205.),
        (PlayMode::Pattern, other_pattern, 2001.),
        (PlayMode::Song, pattern, 3845.),
    ] {
        controller.set_transport(TransportPatch {
            mode: Some(mode),
            pattern: Some(source),
            ..Default::default()
        });
        controller.seek(tick);
        controller.play();
        let mut out = [0.; 2];
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        for probe in &probes {
            let values = probe.0.each_ref().map(|v| v.load(Ordering::Relaxed));
            assert_eq!(values[0], 1);
            assert_eq!(f64::from_bits(values[1]), 120.);
            assert_eq!(f64::from_bits(values[2]), tick / 960.);
            assert_eq!(f64::from_bits(values[3]), tick / 1920.);
            if mode == PlayMode::Song {
                assert_eq!(&values[4..7], &[7, 8, 1]);
                assert_eq!(f64::from_bits(values[7]), 485. / 960.);
                assert_eq!(values[8], 1);
            } else {
                assert_eq!(&values[4..], &[4, 4, 1, 0, 0]);
            }
        }
    }
    controller
        .set_timeline_region(Some(TickRange {
            start: 3845,
            end: 10_000,
        }))
        .unwrap();
    controller.seek(0.);
    controller.play();
    let mut out = [0.; 2];
    assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
    for probe in &probes {
        let values = probe.0.each_ref().map(|v| v.load(Ordering::Relaxed));
        assert_eq!(values[0], 1);
        assert_eq!(f64::from_bits(values[1]), 120.);
        assert_eq!(f64::from_bits(values[2]), 3845. / 960.);
        assert_eq!(f64::from_bits(values[3]), 3845. / 1920.);
        assert_eq!(&values[4..7], &[7, 8, 1]);
        assert_eq!(f64::from_bits(values[7]), 485. / 960.);
        assert_eq!(values[8], 1);
    }

    // The original scalar 4/4 lasts through the shortened second bar.
    // Both prepared hosted roles receive the actual cumulative bar origin,
    // independently of absolute position/seconds and the tempo ramp.
    rig.project.playlist.timeline.meters.clear();
    for (tick, numerator, denominator) in [(4001, 7, 8), (7361, 3, 4), (7400, 7, 16)] {
        let id = rig.project.next_id;
        rig.project.next_id += 1;
        rig.project.playlist.timeline.meters.push(MeterChange {
            id: MeterChangeId(id),
            tick,
            signature: TimeSignature {
                numerator,
                denominator,
            },
        });
    }
    rig.project.playlist.clips[0].length = 12_000;
    let lane = rig.playlist_track();
    let tempo = rig.automation(
        windfall_project::AutomationTarget::Tempo,
        &[(0, (60. - 10.) / 512.), (10_000, (180. - 10.) / 512.)],
    );
    rig.automation_clip(lane, tempo, 0, 10_000);
    let baseline_probes: Vec<_> = (0..4).map(|_| Arc::new(Probe::default())).collect();
    let mut baseline = Rig {
        project: rig.project.clone(),
        pool: rig.pool.clone(),
    };
    baseline.project.playlist.timeline.meters.clear();
    baseline
        .pool
        .set_plugin_factory(Arc::new(Factory(baseline_probes.clone())));
    let (mut baseline_processor, baseline_controller) = baseline.song_processor(RATE);
    let (mut processor, controller) = rig.song_processor(RATE);
    for controller in [&controller, &baseline_controller] {
        controller
            .set_timeline_region(Some(TickRange {
                start: 4001,
                end: 10_000,
            }))
            .unwrap();
    }
    for (mode, source, requested_tick, tick, signature, anchor) in [
        (PlayMode::Song, pattern, 0., 4001., (7, 8), Some((4001., 2))),
        (
            PlayMode::Song,
            other_pattern,
            7360.5,
            7360.5,
            (7, 8),
            Some((4001., 2)),
        ),
        (
            PlayMode::Song,
            pattern,
            7361.,
            7361.,
            (3, 4),
            Some((7361., 3)),
        ),
        (
            PlayMode::Song,
            pattern,
            7400.,
            7400.,
            (7, 16),
            Some((7400., 4)),
        ),
        (
            PlayMode::Pattern,
            pattern,
            2001.25,
            2001.25,
            (4, 4),
            Some((0., 0)),
        ),
        (
            PlayMode::Pattern,
            other_pattern,
            2001.,
            2001.,
            (4, 4),
            Some((0., 0)),
        ),
        (
            PlayMode::Song,
            pattern,
            4001.,
            4001.,
            (7, 8),
            Some((4001., 2)),
        ),
    ] {
        for controller in [&controller, &baseline_controller] {
            controller.set_transport(TransportPatch {
                mode: Some(mode),
                pattern: Some(source),
                ..Default::default()
            });
            controller.seek(requested_tick);
            controller.play();
        }
        let mut out = [0.; 2];
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        assert_eq!(allocator_calls(|| baseline_processor.process(&mut out)), 0);
        for (probe, baseline_probe) in probes.iter().zip(&baseline_probes) {
            let values = probe.0.each_ref().map(|v| v.load(Ordering::Relaxed));
            let baseline_values = baseline_probe
                .0
                .each_ref()
                .map(|v| v.load(Ordering::Relaxed));
            assert_eq!(
                &values[..4],
                &baseline_values[..4],
                "native clock/tempo must retain the existing authority"
            );
            assert_eq!(
                (values[4], values[5]),
                signature,
                "{mode:?} source {source:?}, requested tick {tick}, actual tick {}",
                f64::from_bits(values[2]) * 960.
            );
            assert_eq!(values[0], 1);
            assert!(f64::from_bits(values[1]).is_finite());
            let actual_tick = f64::from_bits(values[2]) * 960.;
            // The native clock reports the ceil-aligned first sample, at most
            // one 180-bpm sample after the requested musical position.
            assert!(actual_tick >= tick - 1e-9 && actual_tick <= tick + 0.06 + 1e-9);
            let seconds = if mode == PlayMode::Song {
                60. / 960. * 10_000. / 120. * (1.0_f64 + 2. * actual_tick / 10_000.).ln()
            } else {
                actual_tick * 60. / (120. * 960.)
            };
            assert!(
                (f64::from_bits(values[3]) - seconds).abs() < 1e-8,
                "{mode:?} expected seconds {seconds}, got {}",
                f64::from_bits(values[3])
            );
            match anchor {
                Some((origin, index)) => {
                    assert_eq!(values[6], 1);
                    assert_eq!(f64::from_bits(values[7]), origin / 960.);
                    assert_eq!(values[8], index);
                }
                None => assert_eq!(&values[6..], &[0, 0, 0]),
            }
        }
    }

    // Saved pattern meter maps have their own signatures and anchors, and
    // switching back to Song restores the song map for both hosted roles.
    rig.project
        .patterns
        .iter_mut()
        .find(|item| item.id == pattern)
        .unwrap()
        .time_signature = Some(TimeSignature {
        numerator: 3,
        denominator: 4,
    });
    let local_id = rig.project.next_id;
    rig.project.next_id += 1;
    let local = rig
        .project
        .patterns
        .iter_mut()
        .find(|item| item.id == other_pattern)
        .unwrap();
    local.time_signature = Some(TimeSignature {
        numerator: 5,
        denominator: 4,
    });
    local.timeline.meters.push(MeterChange {
        id: MeterChangeId(local_id),
        tick: 1200,
        signature: TimeSignature {
            numerator: 7,
            denominator: 8,
        },
    });
    rig.project.check().unwrap();
    let (mut processor, controller) = rig.song_processor(RATE);
    // Retain the selected region's absolute sample-grid alignment used above;
    // tempo-map inversion alone can round an exact boundary infinitesimally
    // into the preceding segment when a new processor has no selected region.
    controller
        .set_timeline_region(Some(TickRange {
            start: 4001,
            end: 10_000,
        }))
        .unwrap();
    for (mode, source, tick, signature, origin, index) in [
        (PlayMode::Pattern, pattern, 2001., (3, 4), 0., 0),
        (PlayMode::Pattern, other_pattern, 0., (5, 4), 0., 0),
        (PlayMode::Pattern, other_pattern, 2001., (7, 8), 1200., 1),
        (PlayMode::Song, other_pattern, 7400., (7, 16), 7400., 4),
        (PlayMode::Pattern, pattern, 2001., (3, 4), 0., 0),
    ] {
        controller.set_transport(TransportPatch {
            mode: Some(mode),
            pattern: Some(source),
            ..Default::default()
        });
        controller.seek(tick);
        controller.play();
        let mut out = [0.; 2];
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        for probe in &probes {
            let values = probe
                .0
                .each_ref()
                .map(|value| value.load(Ordering::Relaxed));
            assert_eq!(
                (values[4], values[5]),
                signature,
                "{mode:?} source {source:?}, requested tick {tick}, actual tick {}",
                f64::from_bits(values[2]) * 960.
            );
            assert_eq!(values[6], 1);
            assert_eq!(f64::from_bits(values[7]), origin / 960.);
            assert_eq!(values[8], index);
        }
    }
}

fn tape() -> Rig {
    let mut rig = Rig::new();
    rig.project.settings.tempo_bpm = 120.0; // exactly 25 frames per tick
    let lane = rig.playlist_track();
    let clip = rig.audio_clip(lane, counting(RATE, 100_000), TrackId::MASTER, 0, 3_840);
    rig.clip_mut(clip).offset = 20;
    rig
}

fn marker(rig: &mut Rig, tick: u32, kind: MarkerKind) {
    let id = rig.project.next_id;
    rig.project.next_id += 1;
    rig.project.playlist.timeline.markers.push(TimelineMarker {
        id: TimelineMarkerId(id),
        tick,
        name: "Navigation".into(),
        kind,
    });
}

#[test]
fn timeline_skip_clamped_to_selected_end_holds_automation_through_native_effect_tails() {
    use crate::support::{peak, sine};
    use windfall_dsp::{LimiterParams, ReverbParams};
    use windfall_project::{AutomationTarget, EffectParams};
    let build = |automated: bool| {
        let mut rig = Rig::new();
        let track = rig.track();
        rig.track_mut(track).volume = if automated { 1.0 } else { 0.125 };
        rig.effect(
            track,
            EffectParams::Reverb(ReverbParams {
                decay_s: 1.0,
                mix: 0.5,
                ..Default::default()
            }),
        );
        rig.effect(
            TrackId::MASTER,
            EffectParams::Limiter(LimiterParams {
                lookahead_ms: 10.0,
                ..Default::default()
            }),
        );
        let lane = rig.playlist_track();
        rig.audio_clip(lane, sine(RATE, 440.0, 1.0), track, 0, 960);
        let automation = rig.automation(AutomationTarget::TrackVolume { track }, &[(0, 0.25)]);
        let clip = rig.automation_clip(lane, automation, 0, 960);
        rig.clip_mut(clip).muted = !automated;
        marker(&mut rig, 90, MarkerKind::Skip { end: 200 });
        rig
    };
    let rig = build(true);
    let reference = build(false);
    let range = TickRange {
        start: 10,
        end: 100,
    };
    let start = |rig: &Rig| {
        let (processor, controller) = rig.song_processor(RATE);
        controller.set_timeline_region(Some(range)).unwrap();
        controller.seek(10.0);
        controller.play();
        (processor, controller)
    };
    let (mut processor, controller) = start(&rig);
    let (mut other, _) = start(&reference);
    let latency = controller.latency_frames() as usize;
    assert_eq!(latency, 480);
    let mut actual = vec![0.0; (2000 + latency + 28_800) * 2];
    assert_eq!(allocator_calls(|| processor.process(&mut actual)), 0);
    let expected = run(&mut other, actual.len() / 2, 79);
    assert!(!controller.transport().playing);
    assert_eq!(controller.frame().tick, 100.0);
    assert!(
        actual == expected,
        "stopped skip released its fader; first mismatching sample {:?}",
        actual.iter().zip(&expected).position(|(a, b)| a != b)
    );
    assert!(peak(&actual[(2000 + latency + 4800) * 2..]) > 1e-4);
    assert_eq!(controller.frame().automated[0].value, 0.25);
    // Offline export is deliberately linear, so this equivalent interval
    // ends where the live skip left its source, before the destination jump.
    for auto_tail in [false, true] {
        let options = RenderOptions {
            mode: PlayMode::Song,
            region: Some(TickRange { start: 10, end: 90 }),
            tail_secs: 0.6,
            auto_tail,
            ..Default::default()
        };
        let rendered = render(&rig.project, &rig.pool, &options, &mut |_| true);
        assert_eq!(
            rendered.samples(),
            render(&reference.project, &reference.pool, &options, &mut |_| true).samples()
        );
        assert_eq!(
            rendered.samples(),
            &actual[latency * 2..latency * 2 + rendered.samples().len()]
        );
    }
    controller.seek(20.0);
    run(&mut processor, 64, 64);
    assert!(
        controller.frame().automated.is_empty(),
        "an explicit stopped seek releases the hold"
    );
    controller.play();
    run(&mut processor, 64, 64);
    assert!(controller.transport().playing);
    assert_eq!(controller.frame().automated[0].value, 0.25);
}

#[test]
fn timeline_skip_pause_resume_and_seek_drive_the_processor_without_callback_allocations() {
    let mut rig = tape();
    marker(&mut rig, 13, MarkerKind::Skip { end: 31 });
    marker(&mut rig, 53, MarkerKind::Pause);
    rig.project.check().unwrap();
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    let mut audio = vec![0.0; (13 + 22) * 25 * 2];
    assert_eq!(allocator_calls(|| processor.process(&mut audio)), 0);
    assert_eq!(left(&audio)[200], counted(20 * 25 + 200));
    // The skipped source resumes at its own offset, on the first boundary frame.
    assert_eq!(left(&audio)[(13 + 8) * 25], counted((20 + 31 + 8) * 25));
    assert!(!controller.transport().playing);
    assert_eq!(controller.frame().tick, 53.0);
    controller.play();
    let resumed = run(&mut processor, 300, 37);
    assert!(controller.transport().playing);
    assert_eq!(left(&resumed)[200], counted((20 + 53) * 25 + 200));
    controller.seek(53.0); // an explicit seek re-arms the pause
    let mut out = [0.0; 2];
    assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
    assert!(!controller.transport().playing);
    assert_eq!(controller.navigation_overflows(), 0);
}

#[test]
fn timeline_loop_marker_and_selected_region_precedence_stop_and_seek_are_real() {
    let mut rig = tape();
    marker(&mut rig, 11, MarkerKind::Loop { end: 23 });
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.set_transport(TransportPatch {
        loop_song: Some(true),
        ..Default::default()
    });
    controller.play();
    let audio = left(&run(&mut processor, 34 * 25, 61));
    assert_eq!(audio[31 * 25], counted((20 + 19) * 25));
    controller
        .set_timeline_region(Some(TickRange { start: 31, end: 47 }))
        .unwrap();
    controller.seek(0.0); // clamped to selection start
    let audio = left(&run(&mut processor, 29 * 25, 79));
    assert_eq!(audio[27 * 25], counted((20 + 42) * 25));
    controller.set_transport(TransportPatch {
        loop_song: Some(false),
        ..Default::default()
    });
    run(&mut processor, 20 * 25, 53);
    assert!(!controller.transport().playing);
    assert_eq!(controller.frame().tick, 47.0);
    assert!(
        controller
            .set_timeline_region(Some(TickRange { start: 2, end: 2 }))
            .is_err()
    );
    assert_eq!(
        controller.timeline_region(),
        Some(TickRange { start: 31, end: 47 })
    );
}

#[test]
fn timeline_tiny_region_loop_has_an_observable_bounded_callback_limit() {
    let rig = tape();
    let (mut processor, controller) = rig.song_processor(RATE);
    controller
        .set_timeline_region(Some(TickRange { start: 1, end: 2 }))
        .unwrap();
    controller.set_transport(TransportPatch {
        loop_song: Some(true),
        ..Default::default()
    });
    controller.play();
    let mut audio = vec![0.0; 4_096 * 2];
    assert_eq!(allocator_calls(|| processor.process(&mut audio)), 0);
    assert!(!controller.transport().playing);
    assert_eq!(controller.navigation_overflows(), 1);
    assert_eq!(controller.frame().tick, 2.0);
}

#[test]
fn timeline_region_is_linear_sample_aligned_and_identical_in_buffer_streams_and_stems() {
    let mut rig = tape();
    rig.project.settings.tempo_bpm = 137.0; // fractional frame endpoints
    marker(&mut rig, 19, MarkerKind::Skip { end: 37 });
    marker(&mut rig, 61, MarkerKind::Pause);
    let options = RenderOptions {
        mode: PlayMode::Song,
        region: Some(TickRange { start: 7, end: 83 }),
        ..Default::default()
    };
    let baseline = render(&rig.project, &rig.pool, &options, &mut |_| true);
    let fpt = 60.0 * f64::from(RATE) / (137.0 * 960.0);
    assert_eq!(
        baseline.frames(),
        (83.0 * fpt).ceil() as usize - (7.0 * fpt).ceil() as usize
    );
    assert!(baseline.samples().iter().any(|s| *s > 0.0));
    rig.project.playlist.timeline.markers.clear();
    assert_eq!(
        baseline.samples(),
        render(&rig.project, &rig.pool, &options, &mut |_| true).samples()
    );
    for block_frames in [1, 37, 1_024] {
        let options = RenderOptions {
            block_frames,
            ..options.clone()
        };
        let mut streamed = Vec::new();
        let outcome = render_streaming_checked(
            &rig.project,
            &rig.pool,
            &options,
            &mut |block| {
                streamed.extend_from_slice(block);
                true
            },
            &mut |_| true,
        )
        .unwrap();
        assert!(outcome.completed);
        assert_eq!(streamed, baseline.samples());
        let mut mix = Vec::new();
        render_stems(
            &rig.project,
            &rig.pool,
            &options,
            &StemOptions {
                mode: StemMode::TrackOutputs,
                tracks: Some(Vec::new()),
                include_mix: true,
                numbered: false,
            },
            &mut |_, block| {
                mix.extend_from_slice(block);
                true
            },
            &mut |_| true,
        )
        .unwrap();
        assert_eq!(mix, baseline.samples());
    }
}

#[test]
fn timeline_regions_with_tempo_ramps_all_clip_sources_offsets_pdc_and_tails_match_live_and_streaming()
 {
    let mut rig = crate::rendering::tape();
    let lane = rig.playlist_track();
    let tempo = rig.automation(
        windfall_project::AutomationTarget::Tempo,
        &[(0, (60.0 - 10.0) / 512.0), (4000, (180.0 - 10.0) / 512.0)],
    );
    rig.automation_clip(lane, tempo, 0, 4000);
    let range = TickRange {
        start: 131,
        end: 2039,
    };
    let options = RenderOptions {
        mode: PlayMode::Song,
        region: Some(range),
        tail_secs: 0.2,
        ..Default::default()
    };
    let rendered = render(&rig.project, &rig.pool, &options, &mut |_| true);
    assert!(crate::support::peak(rendered.samples()) > 0.01);
    let seconds = |tick: u32| {
        60.0 / 960.0 * 4000.0 / 120.0 * (1.0 + 120.0 / 60.0 * f64::from(tick) / 4000.0).ln()
    };
    let body = (seconds(range.end) * f64::from(RATE)).ceil() as usize
        - (seconds(range.start) * f64::from(RATE)).ceil() as usize;
    assert_eq!(rendered.frames(), body + 9600);
    let (mut processor, controller) = rig.song_processor(RATE);
    let latency = controller.latency_frames() as usize;
    assert_eq!(latency, 480);
    controller.set_timeline_region(Some(range)).unwrap();
    controller.seek(f64::from(range.start));
    controller.play();
    let played = run(&mut processor, rendered.frames() + latency, 37);
    assert_eq!(rendered.samples(), &played[latency * 2..]);
    assert!(!controller.transport().playing);
    assert_eq!(controller.frame().tick, f64::from(range.end));
    for auto_tail in [false, true] {
        let options = RenderOptions {
            auto_tail,
            block_frames: 79,
            ..options.clone()
        };
        let buffer = render(&rig.project, &rig.pool, &options, &mut |_| true);
        let mut streamed = Vec::new();
        render_streaming_checked(
            &rig.project,
            &rig.pool,
            &options,
            &mut |block| {
                streamed.extend_from_slice(block);
                true
            },
            &mut |_| true,
        )
        .unwrap();
        assert_eq!(buffer.samples(), streamed);
    }
}

#[test]
fn timeline_invalid_regions_and_cancelled_streams_never_call_sink_as_a_success() {
    let rig = tape();
    let invalid = RenderOptions {
        mode: PlayMode::Song,
        region: Some(TickRange { start: 8, end: 8 }),
        ..Default::default()
    };
    let mut called = false;
    assert!(
        render_streaming_checked(
            &rig.project,
            &rig.pool,
            &invalid,
            &mut |_| {
                called = true;
                true
            },
            &mut |_| true
        )
        .is_err()
    );
    assert!(!called);
    let options = RenderOptions {
        region: Some(TickRange { start: 8, end: 900 }),
        ..invalid
    };
    let cancelled = render_streaming_checked(
        &rig.project,
        &rig.pool,
        &options,
        &mut |_| false,
        &mut |_| true,
    )
    .unwrap();
    assert!(!cancelled.completed);
    assert!(cancelled.frames <= options.block_frames as u64);
}

#[test]
fn timeline_fractional_selected_start_keeps_marker_and_note_onsets_on_the_first_sample() {
    let mut rig = tape();
    rig.project.settings.tempo_bpm = 137.0;
    marker(&mut rig, 7, MarkerKind::Skip { end: 13 });
    marker(&mut rig, 23, MarkerKind::Pause);
    let per_tick = f64::from(RATE) * 60.0 / (137.0 * 960.0);
    let frames = (23.0 * per_tick).ceil() as usize - (13.0 * per_tick).ceil() as usize;
    let (mut processor, controller) = rig.song_processor(RATE);
    controller
        .set_timeline_region(Some(TickRange { start: 7, end: 83 }))
        .unwrap();
    controller.play();
    let mut audio = vec![0.0; frames * 2];
    assert_eq!(allocator_calls(|| processor.process(&mut audio)), 0);
    assert!(
        !controller.transport().playing,
        "the skip at the fractional start was lost"
    );
    assert_eq!(controller.frame().tick, 23.0);

    let mut notes = Rig::new();
    notes.project.settings.tempo_bpm = 137.0;
    let sampler = notes.channel(crate::support::level(RATE, 0.5, 1.0));
    notes.note(sampler, 7, 100);
    let lane = notes.playlist_track();
    let pattern = notes.first_pattern();
    notes.clip(lane, pattern, 0, 960);
    let options = RenderOptions {
        mode: PlayMode::Song,
        region: Some(TickRange { start: 7, end: 83 }),
        ..Default::default()
    };
    let rendered = render(&notes.project, &notes.pool, &options, &mut |_| true);
    assert!(
        crate::support::peak(rendered.samples()) > 0.4,
        "the note at the fractional start was lost"
    );
    let mut streamed = Vec::new();
    render_streaming_checked(
        &notes.project,
        &notes.pool,
        &options,
        &mut |block| {
            streamed.extend_from_slice(block);
            true
        },
        &mut |_| true,
    )
    .unwrap();
    assert_eq!(rendered.samples(), streamed);
}

#[test]
fn timeline_pause_at_natural_end_and_selected_end_loop_tie_are_not_retriggered() {
    let mut rig = tape();
    marker(&mut rig, 3840, MarkerKind::Pause);
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    run(&mut processor, 3840 * 25 + 500, 53);
    assert!(!controller.transport().playing);
    assert_eq!(controller.frame().tick, 3840.0);
    controller
        .set_timeline_region(Some(TickRange {
            start: 3830,
            end: 3840,
        }))
        .unwrap();
    controller.set_transport(TransportPatch {
        loop_song: Some(true),
        ..Default::default()
    });
    controller.seek(0.0);
    controller.play();
    run(&mut processor, 24 * 25, 79);
    assert!(controller.transport().playing);
    assert_eq!(controller.frame().tick, 3834.0);
    assert_eq!(controller.navigation_overflows(), 0);
}
