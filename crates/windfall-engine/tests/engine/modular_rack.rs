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
        parameters: Vec::new(),
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
fn rack_automation_clip_and_internal_link_match_independent_notes_live_offline_and_reopen() {
    use super::support::{Rig, run};
    use windfall_engine::{RenderOptions, render};
    use windfall_project::*;
    for linked in [false, true] {
        let mut rig = Rig::new();
        let channel = rig.synth_on(plain_synth(), TrackId::MASTER);
        let pattern = rig.first_pattern();
        rig.note_in(pattern, channel, 20, 400).key = 60;
        rig.note_in(pattern, channel, 500, 400).key = 60;
        let mut rack = graph(Some(RackNoteModule::KeyMapper {
            params: Default::default(),
            chord: [0; 4],
            voices: 1,
        }));
        rack.retain_parameter_addresses(None).unwrap();
        let parameter = 32
            + rack
                .parameters
                .iter()
                .position(|p| p.path == [4] && p.parameter == 60)
                .unwrap() as u32;
        rig.project.settings.modular_racks.push(ChannelRack {
            channel,
            graph: rack,
        });
        let target = AutomationTarget::InstrumentParam {
            channel,
            param: parameter,
        };
        let lane = rig.playlist_track();
        rig.clip(lane, pattern, 0, 960);
        if linked {
            let source = rig.effect(
                TrackId::MASTER,
                EffectParams::ControlSurface(windfall_dsp::surface::ControlSurfaceParams {
                    knob1: 72.0 / 127.0,
                    ..Default::default()
                }),
            );
            rig.project.settings.controller_links.push(ControllerLink {
                source: ControlSource::Internal {
                    controller: source.0,
                    output: 0,
                },
                target: LinkTarget::Param { target },
                map: ControlMap {
                    pickup: PickupMode::Off,
                    ..Default::default()
                },
            });
        } else {
            let automation = rig.automation(target, &[(0, 72.0 / 127.0)]);
            rig.automation_clip(lane, automation, 0, 960);
        }
        rig.project.check().unwrap();
        let options = RenderOptions {
            mode: windfall_ipc::PlayMode::Song,
            block_frames: 17,
            tail_secs: 0.1,
            ..Default::default()
        };
        let expected_project = rig.project.clone();
        let actual = render(&rig.project, &rig.pool, &options, &mut |_| true);
        assert!(actual.samples().iter().any(|sample| sample.abs() > 0.01));
        let (mut processor, controller) = rig.song_processor(RATE);
        let latency = controller.latency_frames() as usize;
        controller.play();
        close(
            &run(&mut processor, actual.frames() + latency, 127)[latency * 2..],
            actual.samples(),
        );
        rig.project = file::from_json(&file::to_json(&rig.project).unwrap()).unwrap();
        close(
            render(&rig.project, &rig.pool, &options, &mut |_| true).samples(),
            actual.samples(),
        );
        rig.project.settings.modular_racks.clear();
        rig.project.settings.controller_links.clear();
        rig.project.automations.clear();
        rig.project
            .playlist
            .clips
            .retain(|clip| !matches!(clip.content, ClipContent::Automation { .. }));
        for note in &mut rig.pattern_mut(pattern).lanes[0].notes {
            note.key = 72;
        }
        close(
            actual.samples(),
            render(&rig.project, &rig.pool, &options, &mut |_| true).samples(),
        );
        rig.project = expected_project;
        let (mut processor, controller) = rig.song_processor(RATE);
        controller.play();
        let mut out = [0.0; 254];
        assert_eq!(
            allocator_calls(|| {
                for _ in 0..16 {
                    processor.process(&mut out);
                }
            }),
            0
        );
    }
}

#[test]
fn composed_gain_pan_and_pitch_envelopes_match_per_sample_reference_through_independent_releases() {
    use windfall_dsp::rack_note::EnvelopeDestination;
    let shapes = [
        NoteEnvelope {
            attack_ms: 10.0,
            decay_ms: 0.0,
            sustain: 1.0,
            release_ms: 20.0,
            ..Default::default()
        },
        NoteEnvelope {
            attack_ms: 20.0,
            decay_ms: 0.0,
            sustain: 1.0,
            release_ms: 40.0,
            ..Default::default()
        },
        NoteEnvelope {
            destination: EnvelopeDestination::Pan,
            amount: 0.2,
            attack_ms: 10.0,
            release_ms: 30.0,
            ..Default::default()
        },
        NoteEnvelope {
            destination: EnvelopeDestination::Pitch,
            amount: 0.1,
            attack_ms: 10.0,
            release_ms: 10.0,
            ..Default::default()
        },
    ];
    let mut graph = graph(None);
    graph.next_id = 8;
    graph.edges.retain(|edge| edge.from.node != 1);
    for (i, shape) in shapes.iter().enumerate() {
        graph.nodes.push(node(
            4 + i as u32,
            RackProcessor::Note {
                module: RackNoteModule::Envelope { params: *shape },
            },
        ));
        graph.edges.push(wire(
            if i == 0 { 1 } else { 3 + i as u32 },
            NOTE_OUT,
            4 + i as u32,
            NOTE_IN,
        ));
    }
    graph.edges.push(wire(7, NOTE_OUT, 2, NOTE_IN));
    for size in [1, 17, 127] {
        let mut rack = ModularRack::prepare(&graph, RATE, 120.0).unwrap();
        rack.note_on(NoteInstanceId(9), RackNote::default());
        let mut dsp = synth();
        start(
            &mut dsp,
            1,
            RackNote {
                gain: 0.0,
                ..Default::default()
            },
        );
        let mut expected = Vec::new();
        for frame in 0..4000 {
            let seconds = frame as f64 / f64::from(RATE);
            let mut levels = [0.0; 4];
            for (i, shape) in shapes.iter().enumerate() {
                levels[i] = if frame < 1024 {
                    shape.level(seconds)
                } else {
                    shape.level(1024.0 / f64::from(RATE))
                        * (1.0 - (frame - 1024) as f32 / (shape.release_ms * RATE as f32 / 1000.0))
                            .max(0.0)
                };
            }
            if frame == 1024 + 1920 {
                dsp.note_off_instance(NoteInstanceId(1), 60);
            }
            if frame < 1024 + 1920 {
                dsp.set_note_gain(NoteInstanceId(1), levels[0] * levels[1]);
                dsp.set_note_expression(NoteInstanceId(1), levels[2] * 0.2, Default::default());
                dsp.set_note_pitch(NoteInstanceId(1), 60.0 + levels[3] * 1.2);
            }
            expected.extend(reference(&mut dsp, 1));
        }
        let mut actual = audio(&mut rack, 1024, size);
        rack.note_off(NoteInstanceId(9));
        actual.extend(audio(&mut rack, 4000 - 1024, size));
        close(&actual, &expected);
    }
}

#[test]
fn realtime_sequencer_parameter_change_keeps_phase_and_matches_changed_pitch_schedule() {
    let mut graph = graph(Some(RackNoteModule::Sequencer {
        params: Default::default(),
    }));
    graph.retain_parameter_addresses(None).unwrap();
    // The sequence header occupies 0..3, followed by stepsPerBeat and
    // gate/pitch/velocity triplets. Step 2 pitch is module parameter 9.
    let index = 32
        + graph
            .parameters
            .iter()
            .position(|p| p.parameter == 9)
            .unwrap();
    let mut rack = ModularRack::prepare(&graph, RATE, 120.0).unwrap();
    let mut dsp = synth();
    rack.note_on(NoteInstanceId(9), RackNote::default());
    start(&mut dsp, 1, RackNote::default());
    close(&audio(&mut rack, 2048, 17), &reference(&mut dsp, 2048));
    assert_eq!(allocator_calls(|| rack.set_control(index, 7.0)), 0);
    let mut expected = Vec::new();
    for frame in 2048..9000 {
        if frame == 3000 {
            dsp.note_off_instance(NoteInstanceId(1), 60);
        }
        if frame == 6000 {
            start(
                &mut dsp,
                2,
                RackNote {
                    key: 67,
                    ..Default::default()
                },
            );
        }
        expected.extend(reference(&mut dsp, 1));
    }
    close(&audio(&mut rack, 9000 - 2048, 127), &expected);
}

#[test]
fn auxiliary_audio_ports_match_independent_split_outputs_and_muting_clears_every_port() {
    for kind in [
        windfall_dsp::EffectKind::SendTap,
        windfall_dsp::EffectKind::BandSplit,
        windfall_dsp::EffectKind::TransientSplit,
    ] {
        for port in 0..kind.aux_outputs().len() {
            let params = kind.default_params();
            let graph = RackGraph {
                next_id: 4,
                nodes: vec![
                    node(1, RackProcessor::AudioInput),
                    node(2, RackProcessor::Effect { params, mix: 1.0 }),
                    node(3, RackProcessor::AudioOutput),
                ],
                edges: vec![
                    wire(1, AUDIO_OUT, 2, AUDIO_IN),
                    wire(2, AUX_AUDIO_BASE + port as u16, 3, AUDIO_IN),
                ],
                ..Default::default()
            };
            let mut rack = ModularRack::prepare(&graph, RATE, 120.0).unwrap();
            let mut dsp = AnyEffect::new(&params);
            dsp.prepare(RATE as f32, 1);
            dsp.set_params(&params);
            let mut left = [0.0];
            let mut right = [0.0];
            let mut measured = 0.0f32;
            for frame in 0..4096 {
                let value = (frame as f32 * 0.137).sin() * 0.2;
                left[0] = value;
                right[0] = value * 0.5;
                let mut expected_l = left;
                let mut expected_r = right;
                let mut first = [[0.0; 2]];
                let mut second = [[0.0; 2]];
                let mut third = [[0.0; 2]];
                dsp.process_aux(
                    &mut expected_l,
                    &mut expected_r,
                    None,
                    &mut windfall_dsp::AuxBlock {
                        routed: 7,
                        outputs: [&mut first, &mut second, &mut third],
                    },
                );
                let expected = [first[0], second[0], third[0]][port];
                assert_eq!(allocator_calls(|| rack.process(&mut left, &mut right)), 0);
                close(&[left[0], right[0]], &expected);
                measured = measured.max(left[0].abs());
            }
            assert!(measured > 0.0001);
            let mut muted = graph;
            muted.nodes[1].muted = true;
            let mut rack = ModularRack::prepare(&muted, RATE, 120.0).unwrap();
            for _ in 0..128 {
                left[0] = 0.2;
                right[0] = 0.1;
                rack.process(&mut left, &mut right);
                assert_eq!([left[0], right[0]], [0.0; 2]);
            }
        }
    }
}
#[test]
fn synth_effect_wires_match_independent_audio_and_disconnect_silences_output() {
    let effect_params = EffectParams::Polarity(PolarityParams {
        left: true,
        right: true,
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
    for sample in expected.as_chunks_mut::<2>().0 {
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
        parameters: Vec::new(),
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
        parameters: Vec::new(),
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
        parameters: Vec::new(),
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

#[test]
fn color_and_keyboard_outputs_deliver_only_matching_routes_to_real_instruments() {
    let mut colors = windfall_dsp::notemap::ColorMapParams::default();
    colors.table[3] = 7;
    for (module, route, input, mapped_key) in [
        (
            RackNoteModule::ColorRouter { params: colors },
            7,
            RackNote {
                key: 62,
                expression: windfall_dsp::NoteExpression {
                    color_group: Some(3),
                    ..Default::default()
                },
                ..Default::default()
            },
            62,
        ),
        (
            RackNoteModule::KeySplitter {
                params: Default::default(),
                preserve_key: false,
            },
            1,
            RackNote::default(),
            72,
        ),
    ] {
        let mut graph = graph(Some(module));
        graph.edges.last_mut().unwrap().from.port = NOTE_CHANNEL_BASE + route;
        let mut rack = ModularRack::prepare(&graph, RATE, 120.0).unwrap();
        rack.note_on(NoteInstanceId(42), input);
        let mut dsp = synth();
        start(
            &mut dsp,
            1,
            RackNote {
                key: mapped_key,
                ..input
            },
        );
        close(&audio(&mut rack, 2048, 17), &reference(&mut dsp, 2048));
        graph.edges.last_mut().unwrap().from.port = NOTE_CHANNEL_BASE + (route + 1) % 16;
        let mut silent = ModularRack::prepare(&graph, RATE, 120.0).unwrap();
        silent.note_on(NoteInstanceId(42), input);
        assert!(
            audio(&mut silent, 2048, 127)
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }
}
#[test]
fn chord_children_release_by_parent_without_stopping_same_key_siblings() {
    let graph = graph(Some(RackNoteModule::KeyMapper {
        params: Default::default(),
        chord: [0, 4, 7, 12],
        voices: 4,
    }));
    let mut rack = ModularRack::prepare(&graph, RATE, 120.0).unwrap();
    let mut dsp = synth();
    for parent in [11, 12] {
        rack.note_on(NoteInstanceId(parent), RackNote::default());
        for (index, key) in [60, 64, 67, 72].into_iter().enumerate() {
            start(
                &mut dsp,
                (parent - 11) * 4 + index as u64 + 1,
                RackNote {
                    key,
                    ..Default::default()
                },
            );
        }
    }
    close(&audio(&mut rack, 256, 17), &reference(&mut dsp, 256));
    rack.note_off(NoteInstanceId(11));
    for (id, key) in [(1, 60), (2, 64), (3, 67), (4, 72)] {
        dsp.note_off_instance(NoteInstanceId(id), key);
    }
    close(&audio(&mut rack, 2048, 127), &reference(&mut dsp, 2048));
    assert!(rack.active_voices() >= 4);
    rack.note_off(NoteInstanceId(12));
    audio(&mut rack, 2048, 127);
    assert_eq!(rack.active_voices(), 0);
}
#[test]
fn mute_solo_and_macro_wires_move_the_audible_processor() {
    let mut graph = RackGraph {
        parameters: Vec::new(),
        next_id: 5,
        nodes: vec![
            node(1, RackProcessor::AudioInput),
            node(
                2,
                RackProcessor::Effect {
                    params: EffectParams::Polarity(Default::default()),
                    mix: 1.0,
                },
            ),
            node(3, RackProcessor::AudioOutput),
            node(
                4,
                RackProcessor::ControlInput {
                    value: 0.0,
                    index: 0,
                },
            ),
        ],
        edges: vec![
            wire(1, AUDIO_OUT, 2, AUDIO_IN),
            wire(2, AUDIO_OUT, 3, AUDIO_IN),
            RackEdge {
                parameter: Some("left".into()),
                ..wire(4, CONTROL_OUT, 2, CONTROL_IN)
            },
        ],
    };
    graph.nodes[1].solo = true;
    let mut rack = ModularRack::prepare(&graph, RATE, 120.0).unwrap();
    for _ in 0..512 {
        rack.process(&mut [0.2], &mut [0.1]);
    }
    rack.set_control(0, 1.0);
    let mut left = [0.2];
    let mut right = [0.1];
    for _ in 0..512 {
        left[0] = 0.2;
        right[0] = 0.1;
        rack.process(&mut left, &mut right);
    }
    assert!((left[0] + 0.2).abs() < 1e-6);
    assert!((right[0] - 0.1).abs() < 1e-6);
    graph.nodes[1].muted = true;
    let mut muted = ModularRack::prepare(&graph, RATE, 120.0).unwrap();
    left[0] = 0.2;
    right[0] = 0.1;
    muted.process(&mut left, &mut right);
    assert_eq!((left, right), ([0.0], [0.0]));
}
#[test]
fn maximum_graph_processes_without_allocation_and_refuses_one_more_node() {
    let mut max = RackGraph {
        parameters: Vec::new(),
        next_id: 65,
        nodes: (1..=64)
            .map(|id| {
                node(
                    id,
                    if id == 1 {
                        RackProcessor::AudioInput
                    } else if id == 64 {
                        RackProcessor::AudioOutput
                    } else {
                        RackProcessor::Effect {
                            params: EffectParams::Polarity(Default::default()),
                            mix: 1.0,
                        }
                    },
                )
            })
            .collect(),
        edges: (1..64)
            .map(|id| wire(id, AUDIO_OUT, id + 1, AUDIO_IN))
            .collect(),
    };
    let mut rack = ModularRack::prepare(&max, RATE, 120.0).unwrap();
    let mut left = [0.01; 127];
    let mut right = [0.02; 127];
    assert_eq!(allocator_calls(|| rack.process(&mut left, &mut right)), 0);
    assert_eq!(left, [0.01; 127]);
    assert_eq!(right, [0.02; 127]);
    max.nodes.push(node(65, RackProcessor::AudioOutput));
    max.next_id = 66;
    assert!(ModularRack::prepare(&max, RATE, 120.0).is_err());
}
#[test]
fn channel_graph_matches_live_offline_and_reopened_project_audio() {
    use super::support::{Rig, run};
    use windfall_engine::{RenderOptions, render};
    let mut rig = Rig::new();
    rig.project.settings.tempo_bpm = 120.0;
    let channel = rig.synth_on(plain_synth(), windfall_project::TrackId::MASTER);
    rig.note(channel, 0, 240).key = 60;
    rig.project
        .settings
        .modular_racks
        .push(windfall_project::ChannelRack {
            channel,
            graph: graph(Some(RackNoteModule::Sequencer {
                params: Default::default(),
            })),
        });
    let lane = rig.playlist_track();
    rig.clip(lane, rig.first_pattern(), 0, 960);
    let options = RenderOptions {
        mode: windfall_ipc::PlayMode::Song,
        tail_secs: 0.1,
        block_frames: 17,
        ..Default::default()
    };
    let rendered = render(&rig.project, &rig.pool, &options, &mut |_| true);
    assert!(rendered.samples().iter().any(|sample| sample.abs() > 0.01));
    let (mut processor, controller) = rig.song_processor(RATE);
    let latency = controller.latency_frames() as usize;
    controller.play();
    let played = run(&mut processor, rendered.frames() + latency, 127);
    close(&played[latency * 2..], rendered.samples());
    let json = windfall_project::file::to_json(&rig.project).unwrap();
    rig.project = windfall_project::file::from_json(&json).unwrap();
    close(
        render(&rig.project, &rig.pool, &options, &mut |_| true).samples(),
        rendered.samples(),
    );
}
#[test]
fn graph_replacement_preserves_live_parent_release_and_retires_off_callback() {
    use super::support::{Rig, run};
    let mut rig = Rig::new();
    let channel = rig.synth_on(plain_synth(), windfall_project::TrackId::MASTER);
    rig.project
        .settings
        .modular_racks
        .push(windfall_project::ChannelRack {
            channel,
            graph: graph(None),
        });
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(channel, 60, 1.0);
    let mut output = [0.0; 254];
    processor.process(&mut output);
    rig.project.settings.modular_racks[0].graph.edges.clear();
    controller.set_project(&rig.project, &rig.pool);
    assert_eq!(allocator_calls(|| processor.process(&mut output)), 0);
    assert!(output.iter().all(|sample| sample.abs() < 1e-5));
    rig.project.settings.modular_racks.clear();
    controller.set_project(&rig.project, &rig.pool);
    assert_eq!(allocator_calls(|| processor.process(&mut output)), 0);
    assert!(
        run(&mut processor, 1024, 127)
            .iter()
            .any(|sample| sample.abs() > 0.01)
    );
    controller.note_off(channel, 60);
    assert_eq!(allocator_calls(|| processor.process(&mut output)), 0);
    assert!(
        run(&mut processor, 4096, 127)
            .iter()
            .rev()
            .take(256)
            .all(|sample| sample.abs() < 1e-5)
    );
    controller.frame();
}

#[test]
fn musical_gate_retimes_mid_note_and_arpeggio_clock_survives_leader_release() {
    let sequence = NoteSequence::default();
    let mut rack = ModularRack::prepare(
        &graph(Some(RackNoteModule::Sequencer { params: sequence })),
        RATE,
        120.0,
    )
    .unwrap();
    rack.note_on(NoteInstanceId(9), RackNote::default());
    let mut dsp = synth();
    start(&mut dsp, 1, RackNote::default());
    close(&audio(&mut rack, 1000, 17), &reference(&mut dsp, 1000));
    rack.set_tempo(240.0);
    let mut expected = Vec::new();
    for frame in 1000..5000 {
        if frame == 2000 {
            dsp.note_off_instance(NoteInstanceId(1), 60);
        }
        if frame == 3500 {
            start(&mut dsp, 2, RackNote::default());
        }
        expected.extend(reference(&mut dsp, 1));
    }
    close(&audio(&mut rack, 4000, 127), &expected);
    let sequence = NoteSequence {
        mode: SequenceMode::Up,
        ..Default::default()
    };
    let mut rack = ModularRack::prepare(
        &graph(Some(RackNoteModule::Sequencer { params: sequence })),
        RATE,
        120.0,
    )
    .unwrap();
    rack.note_on(NoteInstanceId(1), RackNote::default());
    rack.note_on(
        NoteInstanceId(2),
        RackNote {
            key: 67,
            ..Default::default()
        },
    );
    audio(&mut rack, 1000, 17);
    rack.note_off(NoteInstanceId(1));
    rack.set_tempo(240.0);
    let rest = audio(&mut rack, 2400, 17);
    assert!(
        rest.iter()
            .rev()
            .take(256)
            .all(|sample| sample.abs() < 1e-5)
    );
    assert!(
        audio(&mut rack, 1000, 17)
            .iter()
            .any(|sample| sample.abs() > 0.01)
    );
    rack.note_off(NoteInstanceId(2));
    audio(&mut rack, 2048, 17);
    assert_eq!(rack.active_voices(), 0);
}
#[test]
fn bounded_refusals_are_visible_in_realtime_frames_and_do_not_double_count() {
    use super::support::Rig;
    let mut rig = Rig::new();
    let channel = rig.synth_on(plain_synth(), windfall_project::TrackId::MASTER);
    rig.project
        .settings
        .modular_racks
        .push(windfall_project::ChannelRack {
            channel,
            graph: graph(Some(RackNoteModule::Sequencer {
                params: Default::default(),
            })),
        });
    let (mut processor, controller) = rig.processor(RATE);
    for index in 0..100 {
        controller.note_on(channel, (48 + index % 12) as u8, 0.1);
    }
    let mut output = [0.0; 254];
    assert_eq!(allocator_calls(|| processor.process(&mut output)), 0);
    let refused = controller.frame().rejected_rack_events.unwrap();
    assert!(refused >= 36);
    assert_eq!(controller.frame().rejected_rack_events, Some(refused));
    assert_eq!(allocator_calls(|| processor.process(&mut output)), 0);
    assert_eq!(controller.frame().rejected_rack_events, Some(refused));
}
