//! E3 controls reach native PCM, live/offline, history and saved files.
//! Written, not executed.
#[allow(dead_code)]
#[path = "engine/support.rs"]
mod support;
use support::{Rig, sine};
use windfall_engine::{Processor, RenderOptions, render};
use windfall_project::*;

fn fixture(kind: EffectKind) -> (Rig, Document, EffectId) {
    let mut rig = Rig::new();
    rig.project.settings.tempo_bpm = 120.0;
    let channel = rig.channel(sine(48_000, 440.0, 2.0));
    rig.note(channel, 240, 1920);
    let mut doc = Document::new(rig.project.clone());
    let id = EffectId(
        doc.dispatch(
            Command::AddEffect {
                track: TrackId::MASTER,
                kind,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let mut params = kind.default_params();
    params.set(
        kind.descriptors()
            .iter()
            .position(|p| p.id == "mix")
            .unwrap(),
        1.0,
    );
    doc.dispatch(
        Command::SetEffectParams {
            track: TrackId::MASTER,
            effect: id,
            params,
        },
        None,
    )
    .unwrap();
    (rig, doc, id)
}

fn audio(doc: &Document, rig: &Rig, block: usize) -> Vec<f32> {
    render(
        doc.project(),
        &rig.pool,
        &RenderOptions {
            block_frames: block,
            ..Default::default()
        },
        &mut |_| true,
    )
    .samples()
    .to_vec()
}

fn acceptance(kind: EffectKind, edits: &[(&str, f32)]) {
    let (rig, mut doc, id) = fixture(kind);
    let baseline = audio(&doc, &rig, 127);
    let before = doc.project().mixer.tracks[0].effects.clone();
    for (name, value) in edits {
        let param = kind
            .descriptors()
            .iter()
            .position(|p| p.id == *name)
            .unwrap() as u32;
        doc.dispatch(
            Command::SetEffectParam {
                track: TrackId::MASTER,
                effect: id,
                param,
                value: *value,
            },
            Some(901),
        )
        .unwrap();
    }
    let changed = audio(&doc, &rig, 7);
    let energy: f64 = baseline.iter().map(|x| f64::from(*x).powi(2)).sum();
    let difference: f64 = baseline
        .iter()
        .zip(&changed)
        .map(|(a, b)| f64::from(a - b).powi(2))
        .sum();
    assert!(
        energy > 1.0 && difference / energy > 1e-5,
        "{kind:?} must change actual native audio"
    );
    assert!(changed.iter().all(|x| x.is_finite()));
    doc.undo().unwrap();
    assert_eq!(doc.project().mixer.tracks[0].effects, before);
    assert_eq!(audio(&doc, &rig, 64), baseline);
    doc.redo().unwrap();
    assert_eq!(audio(&doc, &rig, 127), changed);
    let path = std::env::temp_dir().join(format!(
        "windfall-e3-{}-{kind:?}-{}.windfall",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    file::save(doc.project(), &path).unwrap();
    let loaded = file::load(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        loaded.mixer.tracks[0].effects,
        doc.project().mixer.tracks[0].effects
    );
    let reopened = Document::new(loaded);
    assert_eq!(audio(&reopened, &rig, 256), changed);
    for block in [7, 64, 127] {
        let (mut processor, controller) = Processor::new(48_000);
        controller.set_project(reopened.project(), &rig.pool);
        assert_eq!(controller.project_preparation_error(), None);
        controller.play();
        let latency = controller.latency_frames() as usize;
        let mut live = vec![0.0; (36_000 + latency) * 2];
        for chunk in live.chunks_mut(block * 2) {
            processor.process(chunk);
        }
        assert_eq!(
            &live[latency * 2..(latency + 36_000) * 2],
            &changed[..72_000],
            "{kind:?}, block {block}"
        );
    }
}

#[test]
fn room_size_changes_native_reflections_and_roundtrips() {
    acceptance(EffectKind::Room, &[("size", 1.0)]);
}
#[test]
fn lush_pitched_space_changes_native_audio_and_roundtrips() {
    acceptance(
        EffectKind::LushSpace,
        &[("pitchAmount", 1.0), ("size", 0.0)],
    );
}
#[test]
fn bucket_mode_changes_native_character_and_roundtrips() {
    acceptance(EffectKind::VintageChorus, &[("mode", 1.0)]);
}
#[test]
fn pedal_stage_configuration_changes_native_notches_and_roundtrips() {
    acceptance(
        EffectKind::VintagePhaser,
        &[("stages", 1.0), ("stagger", 0.0)],
    );
}
#[test]
fn sixteen_voice_chorus_changes_native_detuning_and_roundtrips() {
    acceptance(EffectKind::HyperChorus, &[("voices", 16.0)]);
}
#[test]
fn four_stacked_combs_change_native_audio_and_roundtrip() {
    acceptance(EffectKind::StackedFlanger, &[("order", 4.0)]);
}
