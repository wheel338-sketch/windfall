//! Device-free boundary/identity evidence. Registered by processor.rs.
use super::*;
use windfall_project::scripting::*;
use windfall_project::*;

fn document() -> (Document, ChannelId) {
    let mut document = Document::new(Project::new("script clock"));
    let channel = ChannelId(
        document
            .dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::SubtractiveSynth),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    (document, channel)
}
fn install(document: &mut Document, channel: ChannelId, source: &str) {
    let mut library = document.project().settings.scripts.clone();
    let reference = ScriptReference {
        id: "live".into(),
        revision: library.assets.len() as u32 + 1,
    };
    library.assets.push(ScriptAsset {
        reference: reference.clone(),
        language_version: 1,
        kind: ScriptKind::NoteTransform,
        name: "Live".into(),
        source: source.into(),
    });
    library.channels.retain(|b| b.channel != channel);
    library.channels.push(ChannelScript {
        channel,
        script: reference,
        parameters: vec![],
    });
    document
        .dispatch(Command::SetScriptLibrary { library }, None)
        .unwrap();
}
fn process(processor: &mut Processor, frames: usize) {
    let mut out = [0.0; 128];
    let mut remaining = frames;
    while remaining > 0 {
        let frames = remaining.min(64);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out[..frames * 2])),
            0
        );
        remaining -= frames;
    }
}
fn keys(processor: &Processor) -> Vec<u8> {
    processor
        .state
        .channel_voice
        .iter()
        .flat_map(|c| &c.owners)
        .flatten()
        .map(|o| o.note.key)
        .collect()
}
#[test]
fn raw_ui_and_hardware_script_releases_keep_original_press_identity_across_edits() {
    let (mut document, channel) = document();
    install(
        &mut document,
        channel,
        "set key key 12 +\ncopies 3 7 60\nemit",
    );
    let (mut processor, controller) = Processor::new(48000);
    controller.set_project(document.project(), &crate::SamplePool::new());
    controller.note_on(channel, 60, 0.8);
    process(&mut processor, 1501);
    assert_eq!(keys(&processor), vec![72, 79]);
    install(&mut document, channel, "set key key 12 -\nemit");
    controller.set_project(document.project(), &crate::SamplePool::new());
    controller.note_on(channel, 60, 0.8);
    process(&mut processor, 1);
    assert_eq!(keys(&processor), vec![72, 79, 48]);
    controller.note_off(channel, 60);
    process(&mut processor, 2000);
    assert_eq!(keys(&processor), vec![48]); // delayed 86 of the first press never starts
    controller.note_off(channel, 60);
    process(&mut processor, 1);
    assert!(keys(&processor).is_empty());
    let epoch = controller.hardware_epoch();
    assert!(controller.hardware_note(epoch, channel, 64, 100));
    controller.note_on(channel, 60, 0.8);
    process(&mut processor, 1);
    assert_eq!(keys(&processor), vec![52, 48]);
    assert!(controller.hardware_note(epoch, channel, 64, 0));
    process(&mut processor, 1);
    assert_eq!(keys(&processor), vec![48]);
}
#[test]
fn raw_script_gates_delayed_copies_and_queue_independent_panic_allocate_nothing() {
    let (mut document, channel) = document();
    install(
        &mut document,
        channel,
        "set key key 12 +\nset length 60\ncopies 3 7 120\nemit",
    );
    let (mut processor, controller) = Processor::new(48000);
    controller.set_project(document.project(), &crate::SamplePool::new());
    let epoch = controller.hardware_epoch();
    assert!(controller.hardware_note(epoch, channel, 60, 100));
    process(&mut processor, 1);
    assert_eq!(keys(&processor), vec![72]);
    process(&mut processor, 1500);
    assert!(keys(&processor).is_empty()); // explicit 60-tick gate, original key still held
    controller.panic_hardware();
    process(&mut processor, 6000);
    assert!(keys(&processor).is_empty());
    assert!(!processor.script_notes.active());
    controller.note_on(channel, 60, 0.8);
    process(&mut processor, 1);
    controller.stop();
    process(&mut processor, 6000);
    assert!(keys(&processor).is_empty());
    assert!(!processor.script_notes.active());
}

fn tempo_project() -> Project {
    let (mut document, _) = document();
    let id = EffectId(
        document
            .dispatch(
                Command::AddEffect {
                    track: TrackId::MASTER,
                    kind: EffectKind::EnvelopeFollower,
                    index: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    document
        .dispatch(
            Command::SetInternalControllerSettings {
                settings: vec![InternalControllerSettings {
                    controller: id.0,
                    sync_beats: Some(0.25),
                    ..Default::default()
                }],
            },
            None,
        )
        .unwrap();
    document
        .dispatch(
            Command::SetControllerLinks {
                links: vec![ControllerLink {
                    source: ControlSource::Internal {
                        controller: id.0,
                        output: 1,
                    },
                    target: LinkTarget::Param {
                        target: AutomationTarget::Tempo,
                    },
                    map: ControlMap {
                        min: (90.0 - 10.0) / 512.0,
                        max: (150.0 - 10.0) / 512.0,
                        pickup: PickupMode::Off,
                        ..Default::default()
                    },
                }],
            },
            None,
        )
        .unwrap();
    document.project().clone()
}
#[test]
fn synced_tempo_feedback_is_bounded_integrates_exactly_and_export_has_the_measured_length() {
    let project = tempo_project();
    let (mut processor, controller) = Processor::new(48000);
    controller.set_project(&project, &crate::SamplePool::new());
    controller.set_transport(windfall_ipc::TransportPatch {
        metronome: Some(windfall_ipc::MetronomeSettings {
            enabled: true,
            ..Default::default()
        }),
        ..Default::default()
    });
    controller.play_passes(1);
    let mut clicks = Vec::new();
    let mut expected_clicks = Vec::new();
    let mut previous_tempo = 120.0;
    let mut integrated = 0.0;
    let mut measured_end = None;
    let length = f64::from(project.patterns[0].length_steps * TICKS_PER_STEP);
    let mut out = [0.0; 128];
    for _ in 0..4000 {
        let start = processor.frame;
        let before = processor.sequencer.clock().tick_at(start);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
        let tempo = processor.state.tempo();
        assert_eq!(processor.state.controls.source_tempo, previous_tempo);
        previous_tempo = tempo;
        assert!((90.0..=150.0).contains(&tempo));
        for index in 0..processor.triggers.len() {
            let trigger = processor.triggers.get(index);
            if matches!(trigger.what, Fire::Click { .. }) {
                clicks.push(trigger.frame);
            }
        }
        for beat in 0..project.patterns[0].length_steps.div_ceil(4) {
            let tick = f64::from(beat * PPQ);
            if tick < before {
                continue;
            }
            let frame = start
                + (((tick - before) * 60.0 * 48000.0 / (tempo * f64::from(PPQ)) - 1e-6)
                    .ceil()
                    .max(0.0) as u64);
            if frame < processor.frame {
                expected_clicks.push(frame);
            }
        }
        // Clock reanchoring must preserve the cumulative sum of block tempos.
        integrated += 64.0 * tempo * f64::from(PPQ) / (60.0 * 48000.0);
        assert!((processor.sequencer.clock().tick_at(processor.frame) - integrated).abs() < 1e-8);
        if let Some(end) = processor.rendered_end() {
            let independent = start
                + (((length - before) * 60.0 * 48000.0 / (tempo * f64::from(PPQ)) - 1e-6)
                    .ceil()
                    .max(0.0) as u64);
            assert_eq!(end, independent);
            measured_end = Some(end as usize);
            break;
        }
    }
    assert_eq!(clicks, expected_clicks);
    assert_eq!(clicks.len(), 4);
    let end = measured_end.expect("bounded positive tempo reaches the pattern end");
    for block in [1, 37, 251, 1024] {
        let audio = crate::render(
            &project,
            &crate::SamplePool::new(),
            &crate::RenderOptions {
                block_frames: block,
                ..Default::default()
            },
            &mut |_| true,
        );
        assert_eq!(audio.frames(), end);
    }
}
#[test]
fn tempo_seek_is_a_deterministic_new_epoch_and_unlink_restores_the_static_clock() {
    let project = tempo_project();
    let (mut processor, controller) = Processor::new(48000);
    controller.set_project(&project, &crate::SamplePool::new());
    controller.play();
    process(&mut processor, 333);
    controller.seek(480.0);
    process(&mut processor, 64);
    let tempo = processor.state.tempo();
    let first = processor.sequencer.tick(&processor.plan, processor.frame);
    process(&mut processor, 777);
    controller.seek(480.0);
    process(&mut processor, 64);
    assert_eq!(processor.state.tempo(), tempo);
    assert_eq!(
        processor.sequencer.tick(&processor.plan, processor.frame),
        first
    );
    let mut unlinked = project;
    unlinked.settings.controller_links.clear();
    controller.set_project(&unlinked, &crate::SamplePool::new());
    process(&mut processor, 64);
    assert_eq!(processor.state.tempo(), 120.0);
    assert_eq!(processor.sequencer.clock().frames_between(0.0, 1.0), 25.0);
}

#[test]
fn scripted_midi_output_trace_uses_transformed_keys_and_original_release_frames() {
    let (mut document, channel) = document();
    install(
        &mut document,
        channel,
        "set key key 12 +\ncopies 3 7 60\nemit",
    );
    let mut project = document.project().clone();
    project.channels[0].source = ChannelSource::Instrument {
        asset: None,
        params: InstrumentParams::MidiOut(Default::default()),
    };
    let (mut processor, controller) = Processor::new(48000);
    controller.set_project(&project, &crate::SamplePool::new());
    controller.note_on(channel, 60, 1.0);
    process(&mut processor, 2000);
    controller.note_off(channel, 60);
    process(&mut processor, 3000);
    let mut trace = Vec::new();
    controller.drain_midi_out(|frame, bytes| {
        if bytes.len() == 3 && matches!(bytes[0] & 0xf0, 0x80 | 0x90) {
            trace.push((frame, bytes.to_vec()));
        }
    });
    assert_eq!(
        trace,
        vec![
            (0, vec![0x90, 72, 127]),
            (1500, vec![0x90, 79, 127]),
            (2000, vec![0x80, 72, 0]),
            (2000, vec![0x80, 79, 0]),
        ]
    );
    assert_eq!(controller.midi_out_dropped(), 0);
}

#[test]
fn tempo_smoothing_stays_inside_declared_bounds_with_an_outside_static_seed() {
    let mut project = tempo_project();
    project.settings.tempo_bpm = 500.0;
    project.settings.controller_links[0].map.smoothing_ms = 5000.0;
    let (mut processor, controller) = Processor::new(48000);
    controller.set_project(&project, &crate::SamplePool::new());
    controller.play();
    for _ in 0..100 {
        process(&mut processor, 64);
        assert!((90.0..=150.0).contains(&processor.state.tempo()));
    }
}

#[test]
fn a_plain_key_held_before_script_assignment_releases_before_its_new_scripted_press() {
    let (mut document, channel) = document();
    let (mut processor, controller) = Processor::new(48000);
    controller.set_project(document.project(), &crate::SamplePool::new());
    controller.note_on(channel, 60, 0.8);
    process(&mut processor, 1);
    assert_eq!(keys(&processor), vec![60]);
    install(&mut document, channel, "set key key 12 +\nemit");
    controller.set_project(document.project(), &crate::SamplePool::new());
    controller.note_on(channel, 60, 0.8);
    process(&mut processor, 1);
    assert_eq!(keys(&processor), vec![60, 72]);
    controller.note_off(channel, 60);
    process(&mut processor, 1);
    assert_eq!(keys(&processor), vec![72]);
    controller.note_off(channel, 60);
    process(&mut processor, 1);
    assert!(keys(&processor).is_empty());
}
