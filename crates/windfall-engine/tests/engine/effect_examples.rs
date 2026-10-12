//! Every shipped original preset renders through the real saved project host.
use crate::support::Rig;
use windfall_core::AudioBuffer;
use windfall_engine::{RenderOptions, render};
use windfall_project::effect_examples::EFFECT_EXAMPLES;
use windfall_project::*;

fn programme() -> AudioBuffer {
    let rate = 48_000;
    let samples = (0..rate * 2)
        .flat_map(|frame| {
            let t = f64::from(frame) / f64::from(rate);
            let hit = if frame % 12_000 < 1_200 { 1.0 } else { 0.3 };
            let left = hit * (0.35 * (std::f64::consts::TAU * 60.0 * t).sin()
                + 0.2 * (std::f64::consts::TAU * 1_000.0 * t).sin()
                + 0.1 * (std::f64::consts::TAU * 8_000.0 * t).sin());
            let right = 0.5 * left + 0.05 * (std::f64::consts::TAU * 630.0 * t).sin();
            [left as f32, right as f32]
        })
        .collect();
    AudioBuffer::from_interleaved(rate, 2, samples)
}

#[test]
fn every_original_effect_preset_reopens_and_renders_finite_nondefault_audio() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "windfall-effect-examples-{}-{nonce}", std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let options = RenderOptions {
        sample_rate: 48_000,
        tail_secs: 0.25,
        block_frames: 137,
        ..Default::default()
    };
    for example in EFFECT_EXAMPLES {
        // Read a user-loadable file, not a hand-constructed parameter fixture.
        let preset_path = directory.join(example.file_name);
        std::fs::write(&preset_path, example.bytes).unwrap();
        let (preset, _) = Preset::from_json(&std::fs::read(&preset_path).unwrap()).unwrap();
        let mut rig = Rig::new();
        let channel = rig.channel(programme());
        rig.note(channel, 0, 960);
        let effect = rig.effect(TrackId::MASTER, example.kind.default_params());
        let default_audio = render(&rig.project, &rig.pool, &options, &mut |_| true);
        let mut doc = Document::new(rig.project);
        let target = PluginTarget::Effect { effect };
        let expected = PluginPreset::capture(doc.project(), target).unwrap().state;
        let plugin = preset.plugin.unwrap();
        let PluginPresetState::Effect { params, .. } = &plugin.state else {
            panic!("not an effect");
        };
        assert_eq!(*params, params.sanitized(), "{}", example.name);
        doc.dispatch(Command::ApplyPluginPreset { target, expected, preset: plugin }, None)
            .unwrap();
        let original_audio = render(doc.project(), &rig.pool, &options, &mut |_| true);
        let path = directory.join(example.file_name).with_extension("windfall");
        file::save(doc.project(), &path).unwrap();
        let reopened = file::load(&path).unwrap();
        let reopened_audio = render(&reopened, &rig.pool, &options, &mut |_| true);
        assert_eq!(original_audio.samples(), reopened_audio.samples(), "{}", example.name);
        assert_eq!(default_audio.frames(), reopened_audio.frames());
        assert!(reopened_audio.samples().iter().all(|value| value.is_finite()));
        let difference = reopened_audio.samples().iter().zip(default_audio.samples())
            .map(|(a, b)| f64::from(a - b).powi(2)).sum::<f64>()
            / reopened_audio.samples().len() as f64;
        assert!(difference > 1e-8, "inaudible/default example {}: {difference}", example.name);
        std::fs::remove_file(preset_path).unwrap();
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir(directory).unwrap();
}
