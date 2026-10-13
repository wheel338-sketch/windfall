//! Device-free engine acceptance, written but not executed during the edit-only pass.
use crate::support::{Rig, left, level, right, run};
use windfall_dsp::{control::*, surface::ControlSurfaceParams};
use windfall_engine::{RenderOptions, render};
use windfall_ipc::PlayMode;
use windfall_project::*;
const RATE: u32 = 48_000;
fn connect(
    rig: &mut Rig,
    source: EffectId,
    output: u8,
    target: AutomationTarget,
    smoothing_ms: f32,
) {
    rig.project.settings.controller_links.push(ControllerLink {
        source: ControlSource::Internal {
            controller: source.0,
            output,
        },
        target: LinkTarget::Param { target },
        map: ControlMap {
            smoothing_ms,
            pickup: PickupMode::Off,
            ..Default::default()
        },
    });
    rig.project.check().unwrap();
}
fn carrier() -> (Rig, TrackId) {
    let mut rig = Rig::new();
    let track = rig.track();
    let lane = rig.playlist_track();
    rig.audio_clip(lane, level(RATE, 0.5, 2.0), track, 0, 1920);
    (rig, track)
}
#[test]
fn internal_surface_link_uses_automation_range_and_live_offline_are_identical() {
    let (mut rig, target) = carrier();
    let controls = rig.track();
    let source = rig.effect(
        controls,
        EffectParams::ControlSurface(ControlSurfaceParams {
            knob1: 0.4,
            ..Default::default()
        }),
    );
    connect(
        &mut rig,
        source,
        0,
        AutomationTarget::TrackVolume { track: target },
        0.0,
    );
    let a = rig.play_song(RATE, 48000, 37);
    let b = rig.play_song(RATE, 48000, 256);
    assert_eq!(a, b);
    let options = RenderOptions {
        mode: PlayMode::Song,
        block_frames: 127,
        ..Default::default()
    };
    let offline = render(&rig.project, &rig.pool, &options, &mut |_| true);
    let offline: Vec<f32> = (0..offline.frames())
        .flat_map(|i| {
            let (l, r) = offline.stereo_frame(i);
            [l, r]
        })
        .collect();
    assert_eq!(a, offline);
    for value in left(&a)[1024..47000].iter() {
        assert!((*value - 0.16).abs() < 1e-6);
    }
}
#[test]
fn bounded_formula_really_modulates_a_host_destination() {
    let (mut rig, target) = carrier();
    let controls = rig.track();
    let source = rig.effect(
        controls,
        EffectParams::FormulaSource(FormulaSourceParams {
            a: 0.4,
            b: 0.2,
            c: 0.3,
            ..Default::default()
        }),
    );
    rig.project
        .settings
        .internal_controllers
        .push(InternalControllerSettings {
            controller: source.0,
            expression: Some("a * b + c".into()),
            ..Default::default()
        });
    connect(
        &mut rig,
        source,
        0,
        AutomationTarget::TrackPan { track: target },
        0.0,
    );
    let out = rig.play_song(RATE, 4800, 37);
    assert!((left(&out)[2000] - 0.5).abs() < 1e-6);
    assert!((right(&out)[2000] - 0.38).abs() < 1e-6);
}
#[test]
fn peak_source_reads_real_isolated_track_audio_and_is_block_invariant() {
    let (mut rig, target) = carrier();
    let detector = rig.track();
    rig.track_mut(detector).output = None;
    let lane = rig.playlist_track();
    rig.audio_clip(lane, level(RATE, 0.25, 2.0), detector, 0, 1920);
    let source = rig.effect(
        detector,
        EffectParams::EnvelopeFollower(EnvelopeFollowerParams {
            attack_ms: 0.0,
            release_ms: 0.0,
            ..Default::default()
        }),
    );
    connect(
        &mut rig,
        source,
        0,
        AutomationTarget::TrackVolume { track: target },
        0.0,
    );
    let a = rig.play_song(RATE, 4800, 1);
    let b = rig.play_song(RATE, 4800, 127);
    assert_eq!(a, b);
    assert!((left(&a)[2000] - 0.0625).abs() < 1e-6);
}
#[test]
fn linked_pad_axes_and_smoothing_follow_the_normalized_reference() {
    let (mut rig, target) = carrier();
    let controls = rig.track();
    let source = rig.effect(
        controls,
        EffectParams::XyzPad(XyzPadParams {
            x: 0.25,
            y: 0.5,
            z: 0.75,
        }),
    );
    connect(
        &mut rig,
        source,
        0,
        AutomationTarget::TrackPan { track: target },
        20.0,
    );
    let out = rig.play_song(RATE, 4800, 37);
    assert!((right(&out)[4000] - 0.25).abs() < 0.006);
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    let mut buffer = [0.0; 128];
    processor.process(&mut buffer);
    assert_eq!(
        crate::realtime::allocator_calls(|| {
            for _ in 0..50 {
                processor.process(&mut buffer);
            }
        }),
        0
    );
}
#[test]
fn dc_routes_generate_calibrated_samples_only_on_chosen_spare_channels() {
    let mut rig = Rig::new();
    let source = rig.effect(
        TrackId::MASTER,
        EffectParams::VoltageSource(VoltageSourceParams {
            value1: 0.5,
            armed: true,
            ..Default::default()
        }),
    );
    rig.project
        .settings
        .internal_controllers
        .push(InternalControllerSettings {
            controller: source.0,
            voltage_routes: vec![VoltageRoute {
                output: 0,
                channel: 2,
                zero: -0.2,
                scale: 0.8,
            }],
            ..Default::default()
        });
    rig.project.check().unwrap();
    let (mut processor, _) = rig.processor(RATE);
    processor.set_output_channels(4);
    let mut out = [0.0; 128];
    processor.process(&mut out);
    assert_eq!(processor.unavailable_dc_routes(), 0);
    assert!(out.iter().all(|v| *v == 0.0));
    for frame in processor.hardware_samples().as_chunks::<4>().0 {
        assert_eq!(frame[0], 0.0);
        assert_eq!(frame[1], 0.0);
        assert!((frame[2] - 0.2).abs() < 1e-6);
        assert_eq!(frame[3], 0.0);
    }
    rig.effect_mut(source).params = EffectParams::VoltageSource(VoltageSourceParams {
        value1: 0.5,
        ..Default::default()
    });
    let (mut disabled, _) = rig.processor(RATE);
    disabled.set_output_channels(4);
    disabled.process(&mut out);
    assert!(disabled.hardware_samples().iter().all(|v| *v == 0.0));
    let (mut absent, _) = rig.processor(RATE);
    absent.set_output_channels(2);
    absent.process(&mut out);
    assert_eq!(absent.unavailable_dc_routes(), 1);
}
#[test]
fn keyboard_notes_drive_velocity_without_an_audible_controller_signal() {
    let (mut rig, target) = carrier();
    let controls = rig.track();
    let channel = rig.channel_on(level(RATE, 0.0, 2.0), controls);
    let source = rig.effect(
        controls,
        EffectParams::KeyboardSource(KeyboardSourceParams::default()),
    );
    let note = rig.note(channel, 0, 1920);
    note.velocity = 0.5;
    let lane = rig.playlist_track();
    let pattern = rig.first_pattern();
    rig.clip(lane, pattern, 0, 1920);
    connect(
        &mut rig,
        source,
        1,
        AutomationTarget::TrackVolume { track: target },
        0.0,
    );
    let out = rig.play_song(RATE, 4800, 37);
    assert!((left(&out)[2000] - 0.25).abs() < 1e-6);
}
#[test]
fn removing_link_returns_target_to_its_stored_value_without_callback_allocation() {
    let (mut rig, target) = carrier();
    let controls = rig.track();
    let source = rig.effect(
        controls,
        EffectParams::ControlSurface(ControlSurfaceParams {
            knob1: 0.25,
            ..Default::default()
        }),
    );
    connect(
        &mut rig,
        source,
        0,
        AutomationTarget::TrackVolume { track: target },
        0.0,
    );
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    run(&mut processor, 4800, 64);
    rig.project.settings.controller_links.clear();
    controller.set_project(&rig.project, &rig.pool);
    let mut block = [0.0; 128];
    assert_eq!(
        crate::realtime::allocator_calls(|| processor.process(&mut block)),
        0
    );
    let out = run(&mut processor, 4800, 64);
    assert!((left(&out)[4000] - 0.5).abs() < 1e-6);
}

#[test]
fn all_hosted_sources_leave_real_carrier_audio_unchanged_without_links() {
    let (plain, _) = carrier();
    let reference = plain.play_song(RATE, 4800, 37);
    for kind in [
        EffectKind::NoteEnvelope,
        EffectKind::KeyboardSource,
        EffectKind::EnvelopeFollower,
        EffectKind::FormulaSource,
        EffectKind::XyPad,
        EffectKind::XyzPad,
        EffectKind::ControlSurface,
        EffectKind::VoltageSource,
    ] {
        let (mut rig, track) = carrier();
        rig.effect(track, kind.default_params());
        assert_eq!(rig.play_song(RATE, 4800, 37), reference, "{kind:?}");
    }
}

#[test]
fn reversed_formula_mapping_and_grid_smoothing_match_independent_exponential_reference() {
    let (mut rig, target) = carrier();
    let controls = rig.track();
    let source = rig.effect(controls, EffectKind::ControlSurface.default_params());
    connect(
        &mut rig,
        source,
        0,
        AutomationTarget::TrackPan { track: target },
        10.0,
    );
    let map = &mut rig.project.settings.controller_links[0].map;
    map.min = 0.8;
    map.max = 0.2;
    map.formula = Some("x^2".into());
    rig.project.check().unwrap();
    let out = rig.play_song(RATE, 4096, 37);
    // x=.5 -> .8 + (.2-.8)*x^2 = .65. At grid boundary n the
    // preceding grid's ramp has arrived at the nth exponential endpoint.
    for n in 4..60 {
        let reference = 0.65 + (0.5 - 0.65) * (-(n as f64) * 64.0 / 480.0).exp();
        assert!(
            (f64::from(left(&out)[n * 64]) - (1.0 - reference)).abs() < 2e-6,
            "grid {n}"
        );
        assert!((right(&out)[n * 64] - 0.5).abs() < 1e-6);
    }
}
#[test]
fn note_triggered_envelope_modulates_real_audio_identically_for_callback_partitions() {
    let (mut rig, target) = carrier();
    let controls = rig.track();
    let channel = rig.channel_on(level(RATE, 0.0, 2.0), controls);
    let source = rig.effect(
        controls,
        EffectParams::NoteEnvelope(NoteEnvelopeParams {
            attack_ms: 0.0,
            decay_ms: 0.0,
            sustain: 0.5,
            release_ms: 0.0,
            ..Default::default()
        }),
    );
    rig.note(channel, 8, 192);
    let lane = rig.playlist_track();
    let pattern = rig.first_pattern();
    rig.clip(lane, pattern, 0, 1920);
    connect(
        &mut rig,
        source,
        0,
        AutomationTarget::TrackVolume { track: target },
        0.0,
    );
    let a = rig.play_song(RATE, 9600, 1);
    let b = rig.play_song(RATE, 9600, 127);
    assert_eq!(a, b);
    assert!((left(&a)[2000] - 0.25).abs() < 1e-6);
    assert!(left(&a)[6000].abs() < 1e-6);
}

#[test]
fn unchanged_link_survives_unrelated_plan_edit_between_grid_boundaries_without_a_gain_dip() {
    let (mut rig, target) = carrier();
    let controls = rig.track();
    let source = rig.effect(
        controls,
        EffectParams::ControlSurface(ControlSurfaceParams {
            knob1: 0.4,
            ..Default::default()
        }),
    );
    connect(
        &mut rig,
        source,
        0,
        AutomationTarget::TrackVolume { track: target },
        5.0,
    );
    let (mut processor, controller) = rig.song_processor(RATE);
    controller.play();
    run(&mut processor, 4801, 37);
    rig.project.settings.name = "Unrelated edit".into();
    controller.set_project(&rig.project, &rig.pool);
    let out = run(&mut processor, 256, 37);
    assert!(left(&out).iter().all(|value| (*value - 0.16).abs() < 1e-6));
}

#[test]
fn feedback_tempo_matches_live_offline_audio_notes_automation_and_loop_partitions() {
    let (mut rig, target) = carrier();
    let source = rig.effect(
        TrackId::MASTER,
        EffectParams::EnvelopeFollower(EnvelopeFollowerParams::default()),
    );
    rig.project
        .settings
        .internal_controllers
        .push(InternalControllerSettings {
            controller: source.0,
            sync_beats: Some(0.25),
            ..Default::default()
        });
    connect(&mut rig, source, 1, AutomationTarget::Tempo, 0.0);
    let link = rig.project.settings.controller_links.last_mut().unwrap();
    link.map.min = (90.0 - 10.0) / 512.0;
    link.map.max = (150.0 - 10.0) / 512.0;
    let channel = rig.channel(crate::support::sine(RATE, 220.0, 0.1));
    rig.note(channel, 240, 240);
    rig.note(channel, 720, 360);
    let lane = rig.project.playlist.tracks[0].id;
    rig.clip(lane, rig.first_pattern(), 0, 1920);
    let automation = rig.automation(
        AutomationTarget::TrackPan { track: target },
        &[(0, 0.5), (480, 0.75), (960, 0.25)],
    );
    rig.automation_clip(lane, automation, 0, 1920);
    let a = render(
        &rig.project,
        &rig.pool,
        &RenderOptions {
            mode: PlayMode::Song,
            block_frames: 37,
            ..Default::default()
        },
        &mut |_| true,
    );
    let b = render(
        &rig.project,
        &rig.pool,
        &RenderOptions {
            mode: PlayMode::Song,
            block_frames: 251,
            ..Default::default()
        },
        &mut |_| true,
    );
    assert_eq!(a.samples(), b.samples());
    assert!(a.samples().iter().any(|s| s.abs() > 0.01));
    assert_eq!(a.samples(), rig.play_song(RATE, a.frames(), 7));
    assert!((38400..=64000).contains(&a.frames()));
    let looping = |block| {
        let (mut processor, controller) = rig.song_processor(RATE);
        controller.set_transport(windfall_ipc::TransportPatch {
            loop_song: Some(true),
            ..Default::default()
        });
        controller.play();
        run(&mut processor, 150000, block)
    };
    assert_eq!(looping(7), looping(1024));
}

#[test]
fn isolated_stems_replay_the_full_mix_audio_dependent_tempo_clock() {
    use windfall_engine::{StemMode, StemOptions, render_stems, render_streaming};
    let (mut rig, target) = carrier();
    let other = rig.track();
    let lane = rig.project.playlist.tracks[0].id;
    rig.audio_clip(lane, level(RATE, 0.5, 2.0), other, 0, 1920);
    let source = rig.effect(
        TrackId::MASTER,
        EffectParams::EnvelopeFollower(EnvelopeFollowerParams {
            attack_ms: 0.0,
            release_ms: 0.0,
            ..Default::default()
        }),
    );
    connect(&mut rig, source, 0, AutomationTarget::Tempo, 0.0);
    let link = rig.project.settings.controller_links.last_mut().unwrap();
    link.map.min = (90.0 - 10.0) / 512.0;
    link.map.max = (150.0 - 10.0) / 512.0;
    let options = RenderOptions {
        mode: PlayMode::Song,
        block_frames: 37,
        ..Default::default()
    };
    let mix = render(&rig.project, &rig.pool, &options, &mut |_| true);
    let mut streamed = Vec::new();
    let outcome = render_streaming(
        &rig.project,
        &rig.pool,
        &options,
        &mut |block| {
            streamed.extend_from_slice(block);
            true
        },
        &mut |_| true,
    );
    assert!(outcome.completed);
    assert_eq!(outcome.frames as usize, mix.frames());
    assert_eq!(streamed, mix.samples());
    let mut isolated = Vec::new();
    let outcome = render_stems(
        &rig.project,
        &rig.pool,
        &options,
        &StemOptions {
            mode: StemMode::ToMaster,
            tracks: Some(vec![target]),
            include_mix: false,
            numbered: false,
        },
        &mut |stream, block| {
            assert_eq!(stream, 0);
            isolated.extend_from_slice(block);
            true
        },
        &mut |_| true,
    )
    .unwrap();
    assert!(outcome.completed);
    assert_eq!(outcome.frames as usize, mix.frames());
    let expected: Vec<f32> = mix.samples().iter().map(|sample| sample * 0.5).collect();
    assert_eq!(isolated, expected);
}
