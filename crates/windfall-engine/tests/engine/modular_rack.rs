//! Acceptance cases written, not executed by the edit-only worker.
use super::realtime::allocator_calls;
use super::support::plain_synth;
use windfall_dsp::rack_note::{
    NoteEnvelope, NoteProperty, NoteSequence, PropertyScale, RackNote, RackNoteModule, SequenceMode,
};
use windfall_dsp::{
    AnyEffect, AnyInstrument, EffectParams, InstrumentParams, NoteInstanceId, PolarityParams,
};
use windfall_engine::modular::ModularRack;
use windfall_project::rack::*;
const RATE: u32 = 48_000;
fn node(id: u32, processor: RackProcessor) -> RackNode {
    RackNode {
        id,
        name: format!("Node {id}"),
        position: [0.0; 2],
        muted: false,
        solo: false,
        processor,
    }
}
fn wire(from: u32, port: u16, to: u32, input: u16) -> RackEdge {
    RackEdge {
        from: RackEndpoint { node: from, port },
        to: RackEndpoint {
            node: to,
            port: input,
        },
        gain: 1.0,
        parameter: None,
    }
}
fn params() -> InstrumentParams {
    InstrumentParams::SubtractiveSynth(plain_synth())
}
fn synth() -> AnyInstrument {
    let mut dsp = AnyInstrument::new(&params());
    dsp.prepare(RATE as f32, 1);
    dsp.set_params(&params());
    dsp.set_tempo(120.0);
    dsp
}
fn graph(module: Option<RackNoteModule>) -> RackGraph {
    let mut graph = RackGraph {
        next_id: 5,
        nodes: vec![
            node(1, RackProcessor::NoteInput),
            node(2, RackProcessor::Instrument { params: params() }),
            node(3, RackProcessor::AudioOutput),
        ],
        edges: vec![wire(2, AUDIO_OUT, 3, AUDIO_IN)],
    };
    if let Some(module) = module {
        graph.nodes.push(node(4, RackProcessor::Note { module }));
        graph
            .edges
            .extend([wire(1, NOTE_OUT, 4, NOTE_IN), wire(4, NOTE_OUT, 2, NOTE_IN)]);
    } else {
        graph.edges.push(wire(1, NOTE_OUT, 2, NOTE_IN));
    }
    graph
}
fn audio(rack: &mut ModularRack, frames: usize, size: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * 2);
    let mut left = vec![0.0; size];
    let mut right = vec![0.0; size];
    for start in (0..frames).step_by(size) {
        let count = size.min(frames - start);
        left[..count].fill(0.0);
        right[..count].fill(0.0);
        rack.process(&mut left[..count], &mut right[..count]);
        for index in 0..count {
            out.extend([left[index], right[index]]);
        }
    }
    out
}
fn reference(dsp: &mut AnyInstrument, frames: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * 2);
    for _ in 0..frames {
        let mut left = [0.0];
        let mut right = [0.0];
        dsp.process(&mut left, &mut right);
        out.extend([left[0], right[0]]);
    }
    out
}
fn close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (frame, (a, b)) in actual.iter().zip(expected).enumerate() {
        assert!((a - b).abs() < 2e-6, "sample {frame}: {a} != {b}");
    }
}
fn start(dsp: &mut AnyInstrument, id: u64, note: RackNote) {
    dsp.note_on_instance(
        NoteInstanceId(id),
        note.key,
        note.velocity,
        note.pan,
        note.expression,
    );
    dsp.set_note_gain(NoteInstanceId(id), note.gain);
}
#[test]
fn synth_effect_wires_match_independent_audio_and_disconnect_silences_output() {
    let effect_params = EffectParams::Polarity(PolarityParams {
        left: true,
        right: true,
        ..Default::default()
    });
    let mut graph = graph(None);
    graph.nodes.push(node(
        4,
        RackProcessor::Effect {
            params: effect_params,
            mix: 1.0,
        },
    ));
    graph.edges[0] = wire(2, AUDIO_OUT, 4, AUDIO_IN);
    graph.edges.push(wire(4, AUDIO_OUT, 3, AUDIO_IN));
    let mut rack = ModularRack::prepare(&graph, RATE, 120.0).unwrap();
    rack.note_on(NoteInstanceId(88), RackNote::default());
    let mut instrument = synth();
    start(&mut instrument, 1, RackNote::default());
    let mut effect = AnyEffect::new(&effect_params);
    effect.prepare(RATE as f32, 1);
    effect.set_params(&effect_params);
    let mut expected = reference(&mut instrument, 4096);
    for sample in expected.chunks_exact_mut(2) {
        let mut left = [sample[0]];
        let mut right = [sample[1]];
        effect.process(&mut left, &mut right);
        sample.copy_from_slice(&[left[0], right[0]]);
    }
    let actual = audio(&mut rack, 4096, 127);
    assert!(actual.iter().any(|s| s.abs() > 0.01));
    close(&actual, &expected);
    graph.edges.pop();
    let mut rack = ModularRack::prepare(&graph, RATE, 120.0).unwrap();
    rack.note_on(NoteInstanceId(88), RackNote::default());
    assert!(audio(&mut rack, 4096, 251).iter().all(|s| *s == 0.0));
}
#[test]
fn parallel_paths_match_an_independently_delayed_dry_and_wet_reference() {
    let effect_params = windfall_dsp::EffectKind::Limiter.default_params();
    let graph = RackGraph {
        next_id: 4,
        nodes: vec![
            node(1, RackProcessor::AudioInput),
            node(
                2,
                RackProcessor::Effect {
                    params: effect_params,
                    mix: 1.0,
                },
            ),
            node(3, RackProcessor::AudioOutput),
        ],
        edges: vec![
            wire(1, AUDIO_OUT, 2, AUDIO_IN),
            RackEdge {
                gain: 0.5,
                ..wire(1, AUDIO_OUT, 3, AUDIO_IN)
            },
            RackEdge {
                gain: 0.5,
                ..wire(2, AUDIO_OUT, 3, AUDIO_IN)
            },
        ],
    };
    let mut rack = ModularRack::prepare(&graph, RATE, 120.0).unwrap();
    let mut effect = AnyEffect::new(&effect_params);
    effect.prepare(RATE as f32, 1);
    effect.set_params(&effect_params);
    let latency = effect.latency_samples();
    assert!(latency > 0);
    assert_eq!(rack.latency_samples(), latency);
    let input: Vec<_> = (0..4096)
        .map(|frame| {
            if frame == 200 {
                0.3
            } else {
                (frame as f32 * 0.071).sin() * 0.02
            }
        })
        .collect();
    for (frame, &value) in input.iter().enumerate() {
        let mut left = [value];
        let mut right = [value * 0.25];
        rack.process(&mut left, &mut right);
        let mut wet_l = [value];
        let mut wet_r = [value * 0.25];
        effect.process(&mut wet_l, &mut wet_r);
        let dry = if frame >= latency {
            input[frame - latency]
        } else {
            0.0
        };
        close(
            &[left[0], right[0]],
            &[(dry + wet_l[0]) * 0.5, (dry * 0.25 + wet_r[0]) * 0.5],
        );
    }
}
#[test]
fn nested_mixed_sources_and_parent_latency_match_compiled_metadata() {
    let nested = RackGraph {
        next_id: 5,
        nodes: vec![
            node(1, RackProcessor::AudioInput),
            node(
                2,
                RackProcessor::Effect {
                    params: windfall_dsp::EffectKind::Limiter.default_params(),
                    mix: 1.0,
                },
            ),
            node(3, RackProcessor::Instrument { params: params() }),
            node(4, RackProcessor::AudioOutput),
        ],
        edges: vec![
            wire(1, AUDIO_OUT, 2, AUDIO_IN),
            wire(2, AUDIO_OUT, 4, AUDIO_IN),
            wire(3, AUDIO_OUT, 4, AUDIO_IN),
        ],
    };
    let graph = RackGraph {
        next_id: 4,
        nodes: vec![
            node(1, RackProcessor::AudioInput),
            node(
                2,
                RackProcessor::Rack {
                    graph: Box::new(nested),
                },
            ),
            node(3, RackProcessor::AudioOutput),
        ],
        edges: vec![
            wire(1, AUDIO_OUT, 2, AUDIO_IN),
            wire(2, AUDIO_OUT, 3, AUDIO_IN),
        ],
    };
    for latency in [0, 12, 1000] {
        let rack = ModularRack::prepare_with_input_latency(&graph, RATE, 120.0, latency).unwrap();
        assert_eq!(
            rack.latency_samples(),
            graph.latency_with_input(RATE as f32, latency)
        );
    }
}
#[test]
fn gain_envelope_audio_matches_independent_per_frame_voice_gain() {
    let envelope = NoteEnvelope {
        attack_ms: 10.0,
        decay_ms: 0.0,
        sustain: 1.0,
        release_ms: 0.0,
        ..Default::default()
    };
    let mut rack = ModularRack::prepare(
        &graph(Some(RackNoteModule::Envelope { params: envelope })),
        RATE,
        120.0,
    )
    .unwrap();
    let mut dsp = synth();
    rack.note_on(
        NoteInstanceId(10),
        RackNote {
            gain: 0.3,
            ..Default::default()
        },
    );
    start(
        &mut dsp,
        1,
        RackNote {
            gain: 0.0,
            ..Default::default()
        },
    );
    let mut expected = Vec::new();
    for frame in 0..1024 {
        dsp.set_note_gain(NoteInstanceId(1), 0.3 * (frame as f32 / 480.0).min(1.0));
        expected.extend(reference(&mut dsp, 1));
    }
    close(&audio(&mut rack, 1024, 127), &expected);
}
#[test]
fn live_expression_updates_reapply_property_scaling_and_preserve_gain() {
    let module = RackNoteModule::PropertyScaler {
        params: PropertyScale {
            property: NoteProperty::Pan,
            curve: 2.0,
            ..Default::default()
        },
    };
    let mut rack = ModularRack::prepare(&graph(Some(module)), RATE, 120.0).unwrap();
    let mut dsp = synth();
    rack.note_on(NoteInstanceId(10), RackNote::default());
    start(
        &mut dsp,
        1,
        RackNote {
            pan: -0.5,
            ..Default::default()
        },
    );
    close(&audio(&mut rack, 256, 17), &reference(&mut dsp, 256));
    rack.set_note_expression(NoteInstanceId(10), 0.5, Default::default());
    dsp.set_note_expression(NoteInstanceId(1), 0.125, Default::default());
    rack.set_note_gain(NoteInstanceId(10), 0.4);
    dsp.set_note_gain(NoteInstanceId(1), 0.4);
    close(&audio(&mut rack, 512, 17), &reference(&mut dsp, 512));
}
#[test]
fn sequence_and_arpeggio_audio_match_independent_schedules_across_block_sizes() {
    for mode in [
        SequenceMode::Sequence,
        SequenceMode::Up,
        SequenceMode::Down,
        SequenceMode::AsPlayed,
    ] {
        let sequence = NoteSequence {
            mode,
            length: 2,
            octaves: 2,
            ..Default::default()
        };
        for size in [1, 17, 127, 1000] {
            let mut rack = ModularRack::prepare(
                &graph(Some(RackNoteModule::Sequencer { params: sequence })),
                RATE,
                120.0,
            )
            .unwrap();
            let mut dsp = synth();
            let keys: &[u8] = if mode == SequenceMode::Sequence {
                &[60]
            } else {
                &[67, 60, 64]
            };
            for (index, &key) in keys.iter().enumerate() {
                rack.note_on(
                    NoteInstanceId(10 + index as u64),
                    RackNote {
                        key,
                        ..Default::default()
                    },
                );
            }
            let mut expected = Vec::new();
            let mut child = 0;
            let mut current_key = 0;
            for frame in 0..24000 {
                if frame % 6000 == 0 {
                    let step = frame / 6000;
                    let key = match mode {
                        SequenceMode::Sequence => 60 + (step / 2 % 2) * 12,
                        SequenceMode::Up => [60, 64, 67][step % 3] + (step / 3 % 2) * 12,
                        SequenceMode::Down => [67, 64, 60][step % 3] + (step / 3 % 2) * 12,
                        SequenceMode::AsPlayed => [67, 60, 64][step % 3] + (step / 3 % 2) * 12,
                    } as u8;
                    child += 1;
                    current_key = key;
                    start(
                        &mut dsp,
                        child,
                        RackNote {
                            key,
                            ..Default::default()
                        },
                    );
                }
                if frame % 6000 == 3000 {
                    dsp.note_off_instance(NoteInstanceId(child), current_key);
                }
                expected.extend(reference(&mut dsp, 1));
            }
            close(&audio(&mut rack, 24000, size), &expected);
            for index in 0..keys.len() {
                rack.note_off(NoteInstanceId(10 + index as u64));
            }
            audio(&mut rack, 2048, size);
            assert_eq!(rack.active_voices(), 0);
        }
    }
}
#[test]
fn note_overflow_controls_reset_and_processing_do_not_touch_the_allocator() {
    let mut rack = ModularRack::prepare(
        &graph(Some(RackNoteModule::Sequencer {
            params: Default::default(),
        })),
        RATE,
        120.0,
    )
    .unwrap();
    let mut left = [0.0; 127];
    let mut right = [0.0; 127];
    assert_eq!(
        allocator_calls(|| {
            for id in 1..=100 {
                rack.note_on(NoteInstanceId(id), RackNote::default());
            }
            rack.process(&mut left, &mut right);
            rack.set_tempo(143.0);
            rack.set_control(0, 0.5);
            rack.set_note_expression(NoteInstanceId(1), 0.3, Default::default());
            rack.set_note_gain(NoteInstanceId(1), 0.4);
            rack.set_note_pitch_offset(NoteInstanceId(1), 1.5);
            for id in 1..=100 {
                rack.note_off(NoteInstanceId(id));
            }
            rack.process(&mut left, &mut right);
            rack.reset();
        }),
        0
    );
    assert!(rack.rejected_events() >= 36);
}
