//! The core delay's inverted stereo mode must swap the sides of the echoes
//! in native audio, live and offline, and survive history and files.
#[allow(dead_code)]
#[path = "engine/support.rs"]
mod support;

use support::{Rig, sine};
use windfall_dsp::{DelayMode, DelayParams, ParamSet};
use windfall_engine::{Processor, RenderOptions, render};
use windfall_project::*;

fn audio(project: &Project, rig: &Rig, block: usize) -> Vec<f32> {
    let result = render(
        project,
        &rig.pool,
        &RenderOptions {
            block_frames: block,
            ..Default::default()
        },
        &mut |_| true,
    );
    assert!(result.samples().iter().all(|sample| sample.is_finite()));
    result.samples().to_vec()
}

/// The energy of the left and of the right side of interleaved stereo.
fn sides(pcm: &[f32]) -> (f64, f64) {
    let side = |offset: usize| {
        pcm.iter()
            .skip(offset)
            .step_by(2)
            .map(|&sample| f64::from(sample).powi(2))
            .sum::<f64>()
    };
    (side(0), side(1))
}

#[test]
fn inverted_delay_swaps_the_echo_sides_persists_and_matches_live_offline() {
    let mut rig = Rig::new();
    rig.project.settings.tempo_bpm = 120.0;
    let channel = rig.channel(sine(48_000, 440.0, 0.5));
    // Everything this channel plays is on the left.
    rig.channel_mut(channel).pan = -1.0;
    rig.note(channel, 240, 480);
    let mut doc = Document::new(rig.project.clone());
    let track = TrackId::MASTER;
    let effect = EffectId(
        doc.dispatch(
            Command::AddEffect {
                track,
                kind: EffectKind::Delay,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    // Echoes only, so what is heard is where the echoes are.
    doc.dispatch(
        Command::SetEffectParams {
            track,
            effect,
            params: EffectParams::Delay(DelayParams {
                sync: false,
                time_ms: 60.0,
                feedback: 0.4,
                mix: 1.0,
                ..Default::default()
            }),
        },
        None,
    )
    .unwrap();
    let stereo = audio(doc.project(), &rig, 64);
    let (left, right) = sides(&stereo);
    assert!(left > 1.0, "the echoes of a left sound are on the left: {left}");
    assert!(right < left * 1.0e-3, "{right} against {left}");

    let mode = DelayParams::descriptors()
        .iter()
        .position(|info| info.id == "mode")
        .unwrap() as u32;
    doc.dispatch(
        Command::SetEffectParam {
            track,
            effect,
            param: mode,
            value: 2.0,
        },
        None,
    )
    .unwrap();
    let inverted = audio(doc.project(), &rig, 7);
    let (left, right) = sides(&inverted);
    assert!(right > 1.0, "inverted echoes are on the right: {right}");
    assert!(left < right * 1.0e-3, "{left} against {right}");
    // The same echoes, on the other side: the two sides change places.
    let (stereo_left, _) = sides(&stereo);
    assert!((right / stereo_left - 1.0).abs() < 1.0e-3);

    doc.undo().unwrap();
    assert_eq!(audio(doc.project(), &rig, 127), stereo);
    doc.redo().unwrap();
    assert_eq!(audio(doc.project(), &rig, 1000), inverted);

    let path = std::env::temp_dir().join(format!(
        "windfall-delay-inverted-qa-{}-{}.windfall",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    file::save(doc.project(), &path).unwrap();
    let loaded = file::load(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let params = &loaded
        .mixer
        .tracks
        .iter()
        .find(|item| item.id == track)
        .unwrap()
        .effects
        .iter()
        .find(|item| item.id == effect)
        .unwrap()
        .params;
    let EffectParams::Delay(params) = params else {
        panic!("saved effect retained its type")
    };
    assert_eq!(params.mode, DelayMode::Inverted);
    assert_eq!(audio(&loaded, &rig, 256), inverted);

    for block in [7, 128] {
        let (mut processor, controller) = Processor::new(48_000);
        controller.set_project(&loaded, &rig.pool);
        controller.play();
        let latency = controller.latency_frames() as usize;
        let mut live = vec![0.0; (36_000 + latency) * 2];
        for chunk in live.chunks_mut(block * 2) {
            processor.process(chunk);
        }
        assert_eq!(
            &live[latency * 2..(latency + 36_000) * 2],
            &inverted[..72_000]
        );
    }
}
