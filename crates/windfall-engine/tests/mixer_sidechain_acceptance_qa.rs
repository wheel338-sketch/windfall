#[allow(dead_code)]
#[path = "engine/support.rs"]
mod support;

use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed},
};
use support::{Rig, level, run};
use windfall_core::AudioBuffer;
use windfall_dsp::{CompressorParams, DetectorMode, EffectKind, StereoMatrixParams};
use windfall_engine::plugins::{HostedEffect, HostedInstrument, PluginFactory};
use windfall_project::{
    Command, Document, EffectParams, PluginAuxInput, PluginBinding, PluginTarget, Send, TrackId,
    file,
};

const RATE: u32 = 8_000;
const FRAMES: usize = 3_000;

fn detector_rig(main_level: f32, key_level: f32) -> (Rig, TrackId, TrackId) {
    let mut rig = Rig::new();
    let main = rig.track();
    let key = rig.track();
    rig.track_mut(key).output = None;
    let main_channel = rig.channel_on(level(RATE, main_level, 2.0), main);
    let key_channel = rig.channel_on(level(RATE, key_level, 2.0), key);
    rig.steps(main_channel, &[0]);
    rig.steps(key_channel, &[0]);
    rig.track_mut(key).sidechains.push(Send {
        target: main,
        gain: 1.0,
    });
    (rig, main, key)
}

fn compressor(sidechain: bool, detector: DetectorMode) -> EffectParams {
    EffectParams::Compressor(CompressorParams {
        sidechain,
        detector,
        threshold_db: -18.0,
        ratio: 100.0,
        attack_ms: 0.05,
        release_ms: 5.0,
        knee_db: 0.0,
        ..Default::default()
    })
}

fn settled_peak(audio: &[f32]) -> f32 {
    audio[2_000 * 2..]
        .iter()
        .map(|v| v.abs())
        .fold(0.0, f32::max)
}

#[test]
fn qa_detector_route_is_inaudible_but_an_independent_send_is_audible() {
    let (mut rig, main, key) = detector_rig(0.0, 0.75);
    // A non-detector slot cannot make the key bus audible.
    rig.effect(main, EffectKind::Balance.default_params());
    let key_only = rig.play(RATE, FRAMES, 127);
    assert!(key_only.iter().all(|v| *v == 0.0));
    assert_eq!(key_only, rig.play(RATE, FRAMES, 1));
    rig.track_mut(key).sends.push(Send {
        target: main,
        gain: 0.5,
    });
    let audible = rig.play(RATE, FRAMES, 127);
    assert!((settled_peak(&audible) - 0.375).abs() < 1e-5);
    assert_eq!(audible, rig.play(RATE, FRAMES, 64));
}

#[test]
fn qa_external_peak_and_rms_compressors_duck_only_selected_detector_slots() {
    for detector in [DetectorMode::Peak, DetectorMode::Rms] {
        let (mut rig, main, key) = detector_rig(0.05, 1.0);
        rig.effect(main, compressor(true, detector));
        let ducked = rig.play(RATE, FRAMES, 127);
        assert!(
            settled_peak(&ducked) < 0.01,
            "{detector:?} must detect the key"
        );
        assert_eq!(ducked, rig.play(RATE, FRAMES, 1));
        rig.track_mut(main).effects[0].params = compressor(false, detector);
        let internal = rig.play(RATE, FRAMES, 127);
        assert!((settled_peak(&internal) - 0.05).abs() < 1e-5);
        rig.track_mut(main).effects[0].params = compressor(true, detector);
        rig.track_mut(key).muted = true;
        let muted_key = rig.play(RATE, FRAMES, 127);
        assert!((settled_peak(&muted_key) - 0.05).abs() < 1e-5);
        rig.track_mut(key).muted = false;
        rig.track_mut(main).solo = true;
        assert!(
            settled_peak(&rig.play(RATE, FRAMES, 127)) < 0.01,
            "soloed destination must retain its detector source"
        );
    }
}

#[test]
fn qa_saved_detector_routes_history_and_mixed_cycles_reach_processor_audio() {
    let (mut rig, main, key) = detector_rig(0.05, 1.0);
    rig.effect(main, compressor(true, DetectorMode::Peak));
    rig.track_mut(key).sidechains.clear();
    let mut document = Document::new(rig.project.clone());
    document
        .dispatch(
            Command::SetSidechain {
                from: key,
                to: main,
                gain: Some(1.0),
            },
            None,
        )
        .unwrap();
    assert!(
        document
            .dispatch(
                Command::SetSidechain {
                    from: main,
                    to: key,
                    gain: Some(1.0)
                },
                None
            )
            .is_err()
    );
    let saved = file::to_json(document.project()).unwrap();
    rig.project = file::from_json(&saved).unwrap();
    assert!(settled_peak(&rig.play(RATE, FRAMES, 127)) < 0.01);
    document.undo().expect("one routing undo");
    rig.project = document.project().clone();
    assert!((settled_peak(&rig.play(RATE, FRAMES, 127)) - 0.05).abs() < 1e-5);
    document.redo().expect("routing redo");
    rig.project = document.project().clone();
    assert!(settled_peak(&rig.play(RATE, FRAMES, 127)) < 0.01);
}

// This fixture exercises the public engine/provider seam. It is deliberately
// not evidence of CLAP/VST3 bus negotiation or desktop bridge owner exchange.
#[derive(Debug, Default)]
struct KeyFactory {
    revision: AtomicU64,
    creates: AtomicUsize,
}

struct KeyProbe {
    selected: Option<u32>,
    difference: bool,
}

impl HostedEffect for KeyProbe {
    fn set_sidechain_input(&mut self, input: Option<u32>) {
        self.selected = input;
    }
    fn process(&mut self, _left: &mut [f32], _right: &mut [f32]) {}
    fn process_sidechain(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        let available = self.selected.is_none_or(|input| input == 2);
        for (at, (left, right)) in left.iter_mut().zip(right).enumerate() {
            let key = key.filter(|_| available).map_or([0.0; 2], |key| key[at]);
            if self.difference {
                *left -= key[0];
                *right -= key[1];
            } else {
                *left = key[0];
                *right = key[1];
            }
        }
    }
    fn set_param(&mut self, _id: u32, _value: f32) {}
    fn set_tempo(&mut self, _bpm: f32) {}
    fn latency(&self) -> usize {
        0
    }
    fn tail(&self) -> usize {
        0
    }
}

impl PluginFactory for KeyFactory {
    fn revision(&self) -> u64 {
        self.revision.load(Relaxed)
    }
    fn effect(
        &self,
        binding: &PluginBinding,
        _rate: u32,
        _block: usize,
    ) -> Result<Box<dyn HostedEffect>, String> {
        self.creates.fetch_add(1, Relaxed);
        Ok(Box::new(KeyProbe {
            selected: None,
            difference: binding.id == "difference",
        }))
    }
    fn instrument(
        &self,
        _binding: &PluginBinding,
        _rate: u32,
        _block: usize,
    ) -> Result<Box<dyn HostedInstrument>, String> {
        Err("effect fixture only".into())
    }
}

fn bind(rig: &mut Rig, track: TrackId, id: &str, input: Option<u32>) {
    let effect = rig.effect(track, EffectKind::Balance.default_params());
    rig.project.plugins.push(PluginBinding {
        target: PluginTarget::Effect { effect },
        format: "clap".into(),
        path: "qa-key-fixture.clap".into(),
        id: id.into(),
        name: id.into(),
        state: Vec::new(),
        parameters: Vec::new(),
        sidechain_input: input,
        wrapper: Default::default(),
        auxiliary_inputs: vec![
            PluginAuxInput {
                index: 2,
                name: "Detector".into(),
                channels: 2,
            },
            PluginAuxInput {
                index: 7,
                name: "Saved inactive input".into(),
                channels: 1,
            },
        ],
    });
}

#[test]
fn qa_per_slot_key_alignment_follows_preceding_matrix_delays_across_blocks() {
    let mut rig = Rig::new();
    let main = rig.track();
    let key = rig.track();
    rig.track_mut(key).output = None;
    let signal: Vec<f32> = (0..16_000)
        .map(|i| if i % 37 == 0 { 0.5 } else { 0.0 })
        .collect();
    for track in [main, key] {
        let channel = rig.channel_on(
            AudioBuffer::from_interleaved(RATE, 1, signal.clone()),
            track,
        );
        rig.steps(channel, &[0]);
    }
    rig.track_mut(key).sidechains.push(Send {
        target: main,
        gain: 1.0,
    });
    // First slot observes the undelayed key, second slot the matrix-delayed key.
    bind(&mut rig, main, "key-copy", None);
    rig.effect(
        main,
        EffectParams::StereoMatrix(StereoMatrixParams {
            left_delay_ms: 4.0,
            right_delay_ms: 4.0,
            ..Default::default()
        }),
    );
    bind(&mut rig, main, "difference", None);
    rig.pool.set_plugin_factory(Arc::new(KeyFactory::default()));
    let output = rig.play(RATE, FRAMES, 127);
    assert!(
        output.iter().all(|v| v.abs() < 1e-6),
        "slot main/key arrival mismatch"
    );
    assert_eq!(output, rig.play(RATE, FRAMES, 1));
    assert_eq!(output, rig.play(RATE, FRAMES, 64));
    // Positive control: without subtraction the delayed key reaches the output.
    rig.track_mut(main).effects.pop();
    rig.project.plugins.pop();
    assert!(rig.play(RATE, FRAMES, 127).iter().any(|v| *v > 0.1));
}

#[test]
fn qa_saved_aux_selection_reapplies_on_provider_revision_and_unavailable_input_is_silent() {
    let (mut rig, main, _) = detector_rig(0.0, 0.5);
    bind(&mut rig, main, "key-copy", Some(2));
    rig.project = file::from_json(&file::to_json(&rig.project).unwrap()).unwrap();
    let factory = Arc::new(KeyFactory::default());
    rig.pool.set_plugin_factory(factory.clone());
    let (mut processor, controller) = rig.processor(RATE);
    controller.play();
    assert!((settled_peak(&run(&mut processor, FRAMES, 127)) - 0.5).abs() < 1e-5);
    assert_eq!(factory.creates.load(Relaxed), 1);
    factory.revision.store(1, Relaxed);
    controller.set_project(&rig.project, &rig.pool);
    assert!((settled_peak(&run(&mut processor, FRAMES, 127)) - 0.5).abs() < 1e-5);
    assert_eq!(factory.creates.load(Relaxed), 2);
    let target = rig.project.plugins[0].target;
    let mut document = Document::new(rig.project.clone());
    document
        .dispatch(
            Command::SetPluginSidechainInput {
                target,
                input: Some(7),
            },
            None,
        )
        .unwrap();
    rig.project = document.project().clone();
    controller.set_project(&rig.project, &rig.pool);
    assert_eq!(settled_peak(&run(&mut processor, FRAMES, 127)), 0.0);
    assert_eq!(
        factory.creates.load(Relaxed),
        2,
        "selection must preserve owner identity"
    );
    document.undo().unwrap();
    rig.project = document.project().clone();
    controller.set_project(&rig.project, &rig.pool);
    assert!((settled_peak(&run(&mut processor, FRAMES, 127)) - 0.5).abs() < 1e-5);
}
