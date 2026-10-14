use windfall_dsp::EffectKind;
use windfall_project::{
    Command, Document, MixerTrackPatch, MixerTrackPreset, Project, TrackId, file,
};

fn add_track(doc: &mut Document, name: &str) -> TrackId {
    let applied = doc
        .dispatch(
            Command::AddMixerTrack {
                name: Some(name.into()),
            },
            None,
        )
        .unwrap();
    TrackId(applied.created[0])
}

#[test]
fn qa_preset_replaces_processing_with_fresh_ids_preserves_routing_and_single_history_step() {
    let mut doc = Document::new(Project::new("Preset acceptance"));
    let source = add_track(&mut doc, "Source");
    let destination = add_track(&mut doc, "Destination");
    doc.dispatch(
        Command::UpdateMixerTrack {
            id: source,
            patch: MixerTrackPatch {
                volume: Some(0.75),
                pan: Some(-0.3),
                muted: Some(true),
                latency_offset_ms: Some(-12.5),
                ..Default::default()
            },
        },
        None,
    )
    .unwrap();
    doc.dispatch(
        Command::AddEffect {
            track: source,
            kind: EffectKind::Limiter,
            index: None,
        },
        None,
    )
    .unwrap();
    doc.dispatch(
        Command::AddEffect {
            track: destination,
            kind: EffectKind::Limiter,
            index: None,
        },
        None,
    )
    .unwrap();
    doc.dispatch(
        Command::SetTrackOutput {
            id: destination,
            output: Some(source),
        },
        None,
    )
    .unwrap();
    let preset = MixerTrackPreset::capture(doc.project(), source).unwrap();
    let encoded = serde_json::to_string(&preset).unwrap();
    let preset: MixerTrackPreset = serde_json::from_str(&encoded).unwrap();
    preset.validate().unwrap();
    let expected = doc.project().mixer.track(destination).unwrap().clone();
    let old_effect = expected.effects[0].id;
    let mut before = doc.project().clone();
    let applied = doc
        .dispatch(
            Command::ApplyMixerTrackPreset {
                id: destination,
                expected: expected.clone(),
                preset: preset.clone(),
                name_color: false,
            },
            None,
        )
        .unwrap();
    assert_eq!(
        windfall_core::english::english(&applied.label),
        "Load mixer track preset"
    );
    let actual = doc.project().mixer.track(destination).unwrap();
    assert_eq!(actual.name, expected.name);
    assert_eq!(actual.output, expected.output);
    assert_eq!(actual.sends, expected.sends);
    assert_eq!(actual.sidechains, expected.sidechains);
    assert_eq!(actual.recording, expected.recording);
    assert_eq!(actual.dock, expected.dock);
    assert_eq!(actual.volume, preset.volume);
    assert_eq!(actual.pan, preset.pan);
    assert_eq!(actual.latency_offset_ms, preset.latency_offset_ms);
    assert!(actual.muted);
    assert_ne!(actual.effects[0].id, preset.effects[0].id);
    assert_ne!(actual.effects[0].id, old_effect);
    assert_eq!(actual.effects[0].params, preset.effects[0].params);
    doc.project().check().unwrap();
    let after = doc.project().clone();
    before.next_id = after.next_id;
    let saved = file::to_json(&after).unwrap();
    assert_eq!(file::from_json(&saved).unwrap(), after);
    doc.undo().unwrap();
    assert_eq!(*doc.project(), before);
    doc.redo().unwrap();
    assert_eq!(*doc.project(), after);
    let before_rejection = doc.project().clone();
    assert!(
        doc.dispatch(
            Command::ApplyMixerTrackPreset {
                id: destination,
                expected,
                preset,
                name_color: true
            },
            None
        )
        .is_err()
    );
    assert_eq!(*doc.project(), before_rejection);
}

#[test]
fn qa_preset_invalid_settings_and_effect_ids_reject_atomically() {
    let mut doc = Document::new(Project::new("Invalid preset"));
    let id = add_track(&mut doc, "Insert");
    doc.dispatch(
        Command::AddEffect {
            track: id,
            kind: EffectKind::Limiter,
            index: None,
        },
        None,
    )
    .unwrap();
    let baseline = MixerTrackPreset::capture(doc.project(), id).unwrap();
    let mut candidates = Vec::new();
    let mut preset = baseline.clone();
    preset.version = 2;
    candidates.push(preset);
    let mut preset = baseline.clone();
    preset.pan = f32::NAN;
    candidates.push(preset);
    let mut preset = baseline.clone();
    preset.latency_offset_ms = 1000.1;
    candidates.push(preset);
    let mut preset = baseline.clone();
    preset.name = "bad\nname".into();
    candidates.push(preset);
    let mut preset = baseline.clone();
    preset.effects.push(preset.effects[0].clone());
    candidates.push(preset);
    let mut preset = baseline.clone();
    preset.effects[0].mix = f32::INFINITY;
    candidates.push(preset);
    for preset in candidates {
        let before = doc.project().clone();
        let expected = doc.project().mixer.track(id).unwrap().clone();
        assert!(preset.validate().is_err());
        assert!(
            doc.dispatch(
                Command::ApplyMixerTrackPreset {
                    id,
                    expected,
                    preset,
                    name_color: true
                },
                None
            )
            .is_err()
        );
        assert_eq!(*doc.project(), before);
    }
}

#[test]
fn qa_signed_manual_latency_validation_persistence_and_history() {
    let mut doc = Document::new(Project::new("Latency correction"));
    let id = add_track(&mut doc, "Insert");
    for offset in [-1000.0, -12.5, 0.0, 0.5, 1000.0] {
        let before = doc.project().clone();
        doc.dispatch(
            Command::UpdateMixerTrack {
                id,
                patch: MixerTrackPatch {
                    latency_offset_ms: Some(offset),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
        assert_eq!(
            doc.project().mixer.track(id).unwrap().latency_offset_ms,
            offset
        );
        let after = doc.project().clone();
        assert_eq!(
            file::from_json(&file::to_json(&after).unwrap()).unwrap(),
            after
        );
        doc.undo().unwrap();
        assert_eq!(*doc.project(), before);
        doc.redo().unwrap();
        assert_eq!(*doc.project(), after);
    }
    for offset in [-1000.01, 1000.01, f64::NAN, f64::INFINITY] {
        let before = doc.project().clone();
        assert!(
            doc.dispatch(
                Command::UpdateMixerTrack {
                    id,
                    patch: MixerTrackPatch {
                        latency_offset_ms: Some(offset),
                        ..Default::default()
                    }
                },
                None
            )
            .is_err()
        );
        assert_eq!(*doc.project(), before);
    }
}
