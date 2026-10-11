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
    for frame in processor.hardware_samples().chunks_exact(4) {
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
