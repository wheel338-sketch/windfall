//! Independent source references; written, not executed by the edit-only worker.
use super::*;
use windfall_dsp::control::*;
use windfall_project::{ControlPoint, InternalControllerSettings};

fn source(params: EffectParams) -> Source {
    Source {
        id: EffectId(1),
        track: 0,
        channel: None,
        note_channel: None,
        config: InternalControllerSettings {
            controller: 1,
            duration_ms: 8.0,
            ..Default::default()
        },
        params,
    }
}
fn note(key: u8, velocity: f32) -> Note {
    Note {
        channel: 0,
        key,
        velocity,
        end: f64::INFINITY,
        origin: Origin::Live,
        pan: 0.0,
        expression: Default::default(),
        source: None,
    }
}
fn tick(node: &mut Node, source: &Source, now: u64, tempo: f64) -> [f32; 8] {
    node.process(
        source,
        source.params,
        (&[0.0], &[0.0]),
        now,
        Clock::control_default(1000, 120.0),
        tempo,
    );
    node.values
}
#[test]
fn drawn_envelope_starts_at_exact_note_frame_and_interpolates_the_saved_points() {
    let mut source = source(EffectParams::NoteEnvelope(NoteEnvelopeParams {
        release_ms: 0.0,
        ..Default::default()
    }));
    source.config.envelope = vec![
        ControlPoint {
            position: 0.0,
            value: 0.0,
        },
        ControlPoint {
            position: 0.5,
            value: 1.0,
        },
        ControlPoint {
            position: 1.0,
            value: 0.5,
        },
    ];
    let mut node = Node::new(&source, 1000.0);
    node.note_on(note(60, 1.0), 3);
    for (frame, expected) in [
        0.0, 0.0, 0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 0.875, 0.75, 0.625, 0.5, 0.5,
    ]
    .into_iter()
    .enumerate()
    {
        assert!(
            (tick(&mut node, &source, frame as u64, 120.0)[0] - expected).abs() < 1e-6,
            "frame {frame}"
        );
    }
    node.note_off(0, 60, Origin::Live);
    assert_eq!(tick(&mut node, &source, 13, 120.0)[0], 0.0);
}
#[test]
fn hosted_adsr_attack_matches_analytic_exponential_and_note_off_releases() {
    let source = source(EffectParams::NoteEnvelope(NoteEnvelopeParams {
        attack_ms: 4.0,
        decay_ms: 0.0,
        sustain: 0.5,
        release_ms: 0.0,
        ..Default::default()
    }));
    let mut node = Node::new(&source, 1000.0);
    node.note_on(note(60, 1.0), 2);
    assert_eq!(tick(&mut node, &source, 0, 120.0)[0], 0.0);
    assert_eq!(tick(&mut node, &source, 1, 120.0)[0], 0.0);
    for elapsed in 1..=3 {
        let reference = 1.3 * (1.0 - (0.3_f32 / 1.3).powf(elapsed as f32 / 4.0));
        assert!((tick(&mut node, &source, elapsed + 1, 120.0)[0] - reference).abs() < 1e-6);
    }
    assert!((tick(&mut node, &source, 5, 120.0)[0] - 1.0).abs() < 1e-6);
    // A float coefficient can put the stage boundary one sample either side.
    tick(&mut node, &source, 6, 120.0);
    assert_eq!(tick(&mut node, &source, 7, 120.0)[0], 0.5);
    node.note_off(0, 60, Origin::Live);
    assert_eq!(tick(&mut node, &source, 8, 120.0)[0], 0.0);
}
#[test]
fn tempo_synced_and_free_lfo_ports_share_the_same_smoothed_phase_and_reference() {
    for (sync, tempo, hz) in [
        (None, 120.0, 3.0_f32),
        (Some(0.5), 120.0, 4.0),
        (Some(0.0625), 600.0, 160.0),
    ] {
        let mut source = source(EffectParams::EnvelopeFollower(EnvelopeFollowerParams {
            rate_hz: 3.0,
            lfo_depth: 1.0,
            ..Default::default()
        }));
        source.config.sync_beats = sync;
        let mut node = Node::new(&source, 1000.0);
        let mut phase = 0.0_f64;
        for frame in 0..1000 {
            let values = tick(&mut node, &source, frame, tempo);
            let reference = (0.5 + 0.5 * (std::f64::consts::TAU * phase).sin()) as f32;
            assert!((values[1] - reference).abs() < 1e-6);
            assert_eq!(values[0], values[1]);
            phase = (phase + f64::from(hz / 1000.0)).fract();
        }
    }
}
#[test]
fn keyboard_overlap_falls_back_and_glides_pitch_and_velocity_in_five_samples() {
    let source = source(EffectParams::KeyboardSource(KeyboardSourceParams::default()));
    let mut node = Node::new(&source, 1000.0);
    node.note_on(note(60, 0.25), 0);
    assert_eq!(tick(&mut node, &source, 0, 120.0)[1], 0.25);
    node.note_on(note(72, 0.75), 1);
    for step in 1..=5 {
        let values = tick(&mut node, &source, step, 120.0);
        assert!((values[0] - (60.0 + 12.0 * step as f32 / 5.0) / 127.0).abs() < 1e-6);
        assert!((values[1] - (0.25 + 0.5 * step as f32 / 5.0)).abs() < 1e-6);
    }
    node.note_off(0, 72, Origin::Live);
    for frame in 6..=10 {
        tick(&mut node, &source, frame, 120.0);
    }
    assert_eq!(node.values[1], 0.25);
    assert_eq!(node.values[0], 60.0 / 127.0);
}
#[test]
fn held_note_table_is_bounded_and_origin_release_and_clock_moves_are_precise() {
    let source = source(EffectParams::KeyboardSource(KeyboardSourceParams::default()));
    let mut node = Node::new(&source, 1000.0);
    for _ in 0..129 {
        node.note_on(note(60, 0.5), 0);
    }
    assert_eq!(node.rejected_notes, 1);
    node.note_off(0, 60, Origin::Hardware);
    assert_eq!(node.held.iter().flatten().count(), 128);
    node.note_off(0, 60, Origin::Live);
    assert_eq!(node.held.iter().flatten().count(), 0);
    let mut runtime = Runtime {
        nodes: vec![node],
        values: vec![],
        rate: 1000.0,
        clock: Clock::control_default(1000, 120.0),
    };
    let mut finite = note(60, 0.5);
    finite.end = 100.0;
    finite.origin = Origin::Sequenced;
    runtime.nodes[0].note_on(finite, 0);
    runtime.nodes[0].note_on(note(72, 0.75), 0);
    runtime.move_ends(&|end| end + 20.0);
    assert_eq!(runtime.nodes[0].held[0].unwrap().end_tick, 120.0);
    assert!(runtime.nodes[0].held[1].unwrap().end_tick.is_infinite());
    runtime.release_origin(Origin::Live);
    assert!(runtime.nodes[0].held[1].is_none());
    runtime.end_sequenced();
    assert!(runtime.nodes[0].held.iter().all(Option::is_none));
}
#[test]
fn dashboard_values_normalize_real_midi_cc_and_xy_xyz_ports_match_parameters() {
    let mut midi = windfall_dsp::MidiOutParams::default();
    midi.controllers[0].value = 64;
    assert!((dashboard_params(midi).get(0).unwrap() - 64.0 / 127.0).abs() < 1e-6);
    for params in [
        EffectParams::XyPad(XyPadParams { x: 0.25, y: 0.75 }),
        EffectParams::XyzPad(XyzPadParams {
            x: 0.25,
            y: 0.75,
            z: 0.5,
        }),
    ] {
        let source = source(params);
        let mut node = Node::new(&source, 1000.0);
        let values = tick(&mut node, &source, 0, 120.0);
        assert_eq!(&values[..2], &[0.25, 0.75]);
        if matches!(params, EffectParams::XyzPad(_)) {
            assert_eq!(values[2], 0.5);
        }
    }
}
