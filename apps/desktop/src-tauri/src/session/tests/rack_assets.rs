//! Written, not executed: portable rack assets and owned native node fixtures.
use super::{Rig, SAMPLE_RATE};
use windfall_project::*;
use windfall_project::rack::*;

fn channel(rig: &Rig) -> ChannelId {
    ChannelId(rig.session.dispatch(Command::AddChannel { name: Some("Owned rack".into()), sample: None, instrument: Some(InstrumentKind::SubtractiveSynth), index: None, mixer_track: None }, None).unwrap().created[0])
}
fn node(id: u32, processor: RackProcessor) -> RackNode {
    RackNode { id, name: format!("Module {id}"), position: [0.0; 2], muted: false, solo: false, processor, asset: None, hosted: None }
}
fn wire(from: u32, port: u16, to: u32, input: u16) -> RackEdge {
    RackEdge { from: RackEndpoint { node: from, port }, to: RackEndpoint { node: to, port: input }, gain: 1.0, parameter: None }
}
fn effect_graph() -> RackGraph {
    RackGraph { next_id: 4, nodes: vec![node(1, RackProcessor::AudioInput), node(2, RackProcessor::Effect { params: EffectKind::Polarity.default_params(), mix: 1.0 }), node(3, RackProcessor::AudioOutput)], edges: vec![wire(1, AUDIO_OUT, 2, AUDIO_IN), wire(2, AUDIO_OUT, 3, AUDIO_IN)], ..Default::default() }
}
fn set(rig: &Rig, id: ChannelId, graph: RackGraph) {
    rig.session.dispatch(Command::SetChannelRack { id, graph: Some(graph), expected: None }, None).unwrap();
}
fn capture(rig: &Rig, channel: ChannelId) -> windfall_ipc::PresetCapture {
    rig.session.preset_capture_channel(channel, rig.session.state().generation, rig.session.document_snapshot().revision).unwrap()
}

#[test]
fn rack_native_container_carries_owned_ir_to_another_project_directory_without_source_files() {
    let source = Rig::new();
    let id = channel(&source);
    let mut graph = effect_graph();
    graph.nodes[1].processor = RackProcessor::Effect { params: EffectKind::Convolver.default_params(), mix: 1.0 };
    graph.nodes[1].asset = Some(RackNodeAsset::Impulse { impulse: ConvolverImpulse { name: "Original portable impulse".into(), sample_rate: SAMPLE_RATE, group_delay: 0, eq_gains: None, frames: vec![[0.5, 0.25], [0.125, -0.0625]] } });
    set(&source, id, graph.clone());
    let capture = capture(&source, id);
    assert!(capture.audio.is_none());
    assert!(capture.preset.assets.is_empty());
    let path = source.session.preset_write(&source.file("portable.wfpreset"), capture).unwrap();
    let destination = Rig::new();
    let target = channel(&destination);
    let copied = destination.file("from another machine.wfpreset");
    std::fs::copy(path, &copied).unwrap();
    let review = destination.session.preset_read(&copied).unwrap();
    assert!(review.missing_audio.is_empty());
    assert_eq!(review.preset.channel.as_ref().unwrap().rack, Some(graph.clone()));
    let expected = destination.project().channel(target).unwrap().clone();
    let before = destination.project();
    destination.session.preset_apply_channel(&copied, target, expected, false, false).unwrap();
    assert_eq!(destination.project().settings.modular_racks[0].graph, graph);
    destination.session.undo().unwrap();
    assert_eq!(destination.project(), before);
    destination.session.redo().unwrap();
    assert_eq!(destination.project().settings.modular_racks[0].graph, graph);
}

#[test]
fn rack_preset_review_reports_missing_embedded_bank_and_relink_keeps_history() {
    let rig = Rig::new();
    let id = channel(&rig);
    let mut graph = RackGraph { next_id: 4, nodes: vec![node(1, RackProcessor::NoteInput), node(2, RackProcessor::Instrument { params: InstrumentKind::MultiSampler.default_params() }), node(3, RackProcessor::AudioOutput)], edges: vec![wire(1, NOTE_OUT, 2, NOTE_IN), wire(2, AUDIO_OUT, 3, AUDIO_IN)], ..Default::default() };
    graph.nodes[1].asset = Some(RackNodeAsset::Zones { zones: vec![RackSampleZone { mapping: windfall_dsp::sample_bank::SampleZone::default(), audio: RackAudioAsset { name: "lost.wav".into(), sample_rate: SAMPLE_RATE, frames: None } }] });
    set(&rig, id, graph.clone());
    let path = rig.session.preset_write(&rig.file("missing.wfpreset"), capture(&rig, id)).unwrap();
    assert_eq!(rig.session.preset_read(&path).unwrap().missing_audio, ["Module 2: lost.wav"]);
    let Some(RackNodeAsset::Zones { zones }) = &mut graph.nodes[1].asset else { unreachable!() };
    zones[0].audio.frames = Some(vec![[0.25; 2]; 512]);
    set(&rig, id, graph.clone());
    rig.session.undo().unwrap();
    assert_eq!(rig.project().settings.modular_racks[0].graph.missing_assets(), ["Module 2: lost.wav"]);
    rig.session.redo().unwrap();
    assert_eq!(rig.project().settings.modular_racks[0].graph, graph);
}

#[cfg(windows)]
fn hosted_fixture(vst3: bool) {
    use windfall_engine::{modular::ModularRack, plugins::{PluginFactory, PluginTransport}};
    let mut rig = Rig::new();
    let (manager, path) = super::plugins::n4_manager(&rig, vst3);
    let channel = channel(&rig);
    let target = PluginTarget::Rack { channel, path: [2, 0, 0, 0], instrument: false };
    let id = if vst3 { manager.state().entries.into_iter().find(|e| e.name == "VST3 Bridge Delayed Effect").unwrap().id } else { "org.windfall.test.bridge-delayed".into() };
    let binding = manager.binding(&path, &id, target).unwrap();
    let mut graph = effect_graph(); graph.nodes[1].hosted = Some(binding.clone());
    set(&rig, channel, graph.clone());
    rig.run(480);
    let token = manager.runtime.selected_token(target).expect("rack selects an actual native owner");
    assert!(manager.runtime.editor_binding(target, Some(rig.project().plugin(target).unwrap().clone()), true).unwrap_err().contains("no editor"), "the owned delayed fixture has no GUI; ownership must reach that explicit capability error");
    assert!(!manager.runtime.bridge_status().iter().find(|s| s.1 == token).unwrap().2.failed);
    let rendered = manager.runtime.render_factory().unwrap();
    let mut rack = ModularRack::prepare_with_factory(&graph, SAMPLE_RATE, 120.0, rendered.as_ref()).unwrap();
    let mut standalone = rendered.effect(&binding, SAMPLE_RATE, 512).unwrap();
    assert_eq!(rack.latency_samples(), standalone.latency());
    assert_eq!(rack.latency_samples(), 549);
    let transport = PluginTransport { playing: false, tempo_bpm: 120.0, position_beats: 0.0, position_seconds: 0.0, numerator: 4, denominator: 4, meter_anchor: None };
    rack.transport(transport); standalone.transport(transport);
    let mut left = [0.5; 2048]; let mut right = left;
    let mut reference = left; let mut reference_right = left;
    rack.process(&mut left, &mut right);
    for (left, right) in reference.chunks_mut(512).zip(reference_right.chunks_mut(512)) { standalone.process(left, right); }
    for (actual, expected) in left.iter().chain(&right).zip(reference.iter().chain(&reference_right)) { assert!((actual - expected).abs() < 1e-6, "rack node differs from the standalone owned fixture"); }
    assert!(left.iter().any(|v| v.abs() > 0.1));
    assert_eq!(rendered.render_error(), None);
    assert_eq!(manager.runtime.selected_token(target), Some(token), "render owners cannot become native editor owners");
    // A distinct live target uses the same provider/bridge, without changing
    // the project's selected editor owner. Compare steady PCM after each
    // provider's negotiated pipeline delay, with helper time between blocks.
    let live_target = PluginTarget::Rack { channel, path: [4, 0, 0, 0], instrument: false };
    let mut live_binding = binding.clone(); live_binding.target = live_target;
    let mut live_graph = graph.clone(); live_graph.nodes[1].id = 4; live_graph.nodes[1].hosted = Some(live_binding); live_graph.next_id = 5;
    for edge in &mut live_graph.edges { if edge.from.node == 2 { edge.from.node = 4; } if edge.to.node == 2 { edge.to.node = 4; } }
    let mut live = ModularRack::prepare_with_factory(&live_graph, SAMPLE_RATE, 120.0, manager.runtime.as_ref()).unwrap();
    live.transport(transport);
    assert_eq!(live.latency_samples(), 1061);
    let mut live_pcm = Vec::new();
    for _ in 0..16 {
        let mut left = [0.5; 480]; let mut right = left;
        assert_eq!(crate::test_alloc::allocator_calls(|| live.process(&mut left, &mut right)), 0);
        live_pcm.extend_from_slice(&left);
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    for sample in live_pcm.iter().rev().take(480) { assert!((sample - left[2047]).abs() < 1e-6, "live and offline node PCM differ after negotiated latency"); }
    for (actual, expected) in live_pcm[1061..1061 + left.len() - 549].iter().zip(&left[549..]) {
        assert!((actual - expected).abs() < 1e-6, "live/offline PCM differs after compensating the provider's reported pipeline latency");
    }
    assert_eq!(manager.runtime.selected_token(target), Some(token));
    let captured = manager.runtime.capture(rig.project()).unwrap();
    assert!(captured.plugin(target).unwrap().state.starts_with(b"WFPS"));
    let reopened = windfall_project::file::from_json(&windfall_project::file::to_json(&captured).unwrap()).unwrap();
    assert_eq!(reopened.plugin(target), captured.plugin(target));
    let (body, _) = ChannelPreset::capture(&captured, channel).unwrap();
    let preset = Preset::of_channel("Native node", body, None);
    let loaded = Preset::from_json(preset.to_json().unwrap().as_bytes()).unwrap().0;
    assert_eq!(loaded.channel.unwrap().rack.unwrap().nodes[1].hosted, captured.settings.modular_racks[0].graph.nodes[1].hosted);
    let before = rig.project();
    rig.session.dispatch(Command::SetChannelRack { id: channel, graph: None, expected: None }, None).unwrap();
    let mut out = [0.0; 960];
    assert_eq!(crate::test_alloc::allocator_calls(|| rig.processor.process(&mut out)), 0);
    assert_eq!(manager.runtime.selected_token(target), None);
    rig.session.undo().unwrap();
    rig.run(480);
    assert_eq!(rig.project(), before);
    assert!(manager.runtime.selected_token(target).is_some());
    rig.session.redo().unwrap();
    rig.run(480);
    assert!(rig.project().plugin(target).is_none());
}
#[cfg(windows)]
#[test]
fn clap_owned_fixture_as_rack_node_matches_standalone_and_preserves_state_history_and_editor_owner() { hosted_fixture(false); }
#[cfg(windows)]
#[test]
fn vst3_owned_fixture_as_rack_node_matches_standalone_and_preserves_state_history_and_editor_owner() { hosted_fixture(true); }

#[cfg(windows)]
#[test]
fn crashed_owned_rack_helper_keeps_parallel_builtin_audio_and_a_visible_target_reason() {
    let rig = Rig::new();
    let (manager, path) = super::plugins::n4_manager(&rig, false);
    let channel = channel(&rig);
    let target = PluginTarget::Rack { channel, path: [4, 0, 0, 0], instrument: false };
    let mut binding = manager.binding(&path, "org.windfall.test.bridge-controlled-process-error", target).unwrap();
    binding.parameters.iter_mut().find(|p| p.id == 7).unwrap().value = 1.0;
    let mut graph = effect_graph();
    let mut failed = node(4, RackProcessor::Effect { params: EffectKind::Polarity.default_params(), mix: 1.0 }); failed.hosted = Some(binding);
    graph.next_id = 5; graph.nodes.push(failed);
    graph.edges.extend([wire(1, AUDIO_OUT, 4, AUDIO_IN), wire(4, AUDIO_OUT, 3, AUDIO_IN)]);
    let mut rack = windfall_engine::modular::ModularRack::prepare_with_factory(&graph, SAMPLE_RATE, 120.0, manager.runtime.as_ref()).unwrap();
    rack.transport(windfall_engine::plugins::PluginTransport { playing: true, tempo_bpm: 120.0, position_beats: 0.0, position_seconds: 0.0, numerator: 4, denominator: 4, meter_anchor: None });
    let mut last = [0.0; 480];
    for _ in 0..32 {
        last.fill(0.25); let mut right = last;
        assert_eq!(crate::test_alloc::allocator_calls(|| rack.process(&mut last, &mut right)), 0);
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    assert!(last.iter().all(|v| v.is_finite() && *v >= 0.24), "parallel builtin path must keep running after native helper failure");
    assert!(manager.runtime.errors().iter().any(|(owner, reason)| *owner == target && !reason.is_empty()));
}

#[cfg(windows)]
fn hosted_instrument(vst3: bool) {
    use windfall_engine::{modular::ModularRack, plugins::PluginFactory};
    let rig = Rig::new();
    let (manager, path) = super::plugins::n4_manager(&rig, vst3);
    let channel = channel(&rig);
    let target = PluginTarget::Rack { channel, path: [2, 0, 0, 0], instrument: true };
    let id = if vst3 { manager.state().entries.into_iter().find(|e| e.name == "VST3 Test Instrument").unwrap().id } else { "org.windfall.test.sine".into() };
    let binding = manager.binding(&path, &id, target).unwrap();
    let mut graph = RackGraph { next_id: 4, nodes: vec![node(1, RackProcessor::NoteInput), node(2, RackProcessor::Instrument { params: InstrumentKind::SubtractiveSynth.default_params() }), node(3, RackProcessor::AudioOutput)], edges: vec![wire(1, NOTE_OUT, 2, NOTE_IN), wire(2, AUDIO_OUT, 3, AUDIO_IN)], ..Default::default() };
    graph.nodes[1].hosted = Some(binding.clone());
    set(&rig, channel, graph.clone());
    let rendered = manager.runtime.render_factory().unwrap();
    let mut rack = ModularRack::prepare_with_factory(&graph, SAMPLE_RATE, 120.0, rendered.as_ref()).unwrap();
    let mut standalone = rendered.instrument(&binding, SAMPLE_RATE, 512).unwrap();
    assert_eq!(rack.latency_samples(), standalone.latency());
    rack.note_on(windfall_dsp::NoteInstanceId(42), windfall_dsp::rack_note::RackNote::default());
    standalone.note_on_instance(windfall_dsp::NoteInstanceId(1), 60, 1.0, 0.0, windfall_dsp::NoteExpression::NEUTRAL);
    let mut left = [0.0; 4096]; let mut right = left;
    let mut expected = left; let mut expected_right = left;
    rack.process(&mut left, &mut right);
    for (left, right) in expected.chunks_mut(512).zip(expected_right.chunks_mut(512)) { standalone.process(left, right); }
    for (a, b) in left.iter().chain(&right).zip(expected.iter().chain(&expected_right)) { assert!((a-b).abs() < 1e-6); }
    assert!(super::rms(&left) > 0.01);
    rack.note_off(windfall_dsp::NoteInstanceId(42));
    standalone.note_off_instance(windfall_dsp::NoteInstanceId(1), 60);
    left.fill(0.0); right.fill(0.0); expected.fill(0.0); expected_right.fill(0.0);
    rack.process(&mut left, &mut right);
    for (left, right) in expected.chunks_mut(512).zip(expected_right.chunks_mut(512)) { standalone.process(left, right); }
    for (a, b) in left.iter().chain(&right).zip(expected.iter().chain(&expected_right)) { assert!((a-b).abs() < 1e-6); }
    assert!(left.iter().rev().take(512).all(|v| v.abs() < 1e-6));
}
#[cfg(windows)]
#[test]
fn clap_owned_instrument_node_matches_standalone_note_and_release_pcm() { hosted_instrument(false); }
#[cfg(windows)]
#[test]
fn vst3_owned_instrument_node_matches_standalone_note_and_release_pcm() { hosted_instrument(true); }
