//! The equalizer, filter and stereo effects through the real document:
//! every control, history, the file format and projects saved before a
//! processor gained a control.

use windfall_dsp::{EffectKind, EffectParams, ParamKind};
use windfall_project::*;

/// The registered kinds this stream owns. A kind added later is appended.
const KINDS: [EffectKind; 7] = [
    EffectKind::SevenBand,
    EffectKind::Spreader,
    EffectKind::StereoEnhancer,
    EffectKind::PanLfo,
    EffectKind::MorphEq,
    EffectKind::FilterBank,
    EffectKind::BandSplit,
];

fn add(doc: &mut Document, kind: EffectKind) -> EffectId {
    EffectId(
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
    )
}

fn params(doc: &Document, effect: EffectId) -> EffectParams {
    doc.project().mixer.tracks[0].effect(effect).unwrap().params
}

fn set(effect: EffectId, param: usize, value: f32) -> Command {
    Command::SetEffectParam {
        track: TrackId::MASTER,
        effect,
        param: param as u32,
        value,
    }
}

/// A value of the control that is not its default.
fn other(info: &windfall_dsp::ParamInfo) -> f32 {
    if info.default == info.max {
        info.min
    } else {
        info.max
    }
}

#[test]
fn every_control_is_one_undo_step_and_survives_the_file() {
    let directory = tempfile::tempdir().unwrap();
    for kind in KINDS {
        let mut doc = Document::new(Project::new("EQ, filter and stereo"));
        let effect = add(&mut doc, kind);
        assert_eq!(params(&doc, effect), kind.default_params());
        for (index, info) in kind.descriptors().iter().enumerate() {
            let before = params(&doc, effect);
            let cursor = doc.history().cursor;
            let value = other(info);
            let middle = match info.kind {
                ParamKind::Float => (info.min + info.max) * 0.5,
                _ => value,
            };
            // Two changes of one gesture, as a drag makes them.
            let gesture = Some(500 + index as u64);
            doc.dispatch(set(effect, index, middle), gesture).unwrap();
            doc.dispatch(set(effect, index, value), gesture).unwrap();
            assert_eq!(params(&doc, effect).get(index), Some(value), "{}", info.id);
            assert_eq!(doc.history().cursor, cursor + 1, "{}", info.id);
            doc.project().check().unwrap();
            doc.undo().unwrap();
            assert_eq!(params(&doc, effect), before, "{}", info.id);
            doc.redo().unwrap();
            assert_eq!(params(&doc, effect).get(index), Some(value), "{}", info.id);
        }
        let path = directory.path().join(format!("{kind:?}.windfall"));
        file::save(doc.project(), &path).unwrap();
        let loaded = file::load(&path).unwrap();
        assert_eq!(loaded.mixer, doc.project().mixer, "{kind:?}");
        loaded.check().unwrap();
    }
}

#[test]
fn a_control_that_does_not_exist_changes_nothing() {
    for kind in KINDS {
        let mut doc = Document::new(Project::new("EQ, filter and stereo"));
        let effect = add(&mut doc, kind);
        let before = doc.project().clone();
        let cursor = doc.history().cursor;
        let past = kind.descriptors().len();
        assert!(doc.dispatch(set(effect, past, 0.0), None).is_err());
        assert!(doc.dispatch(set(effect, 0, f32::NAN), None).is_err());
        assert_eq!(doc.project(), &before);
        assert_eq!(doc.history().cursor, cursor);
    }
}

#[test]
fn every_control_can_be_automated_and_the_curve_leaves_with_the_effect() {
    for kind in KINDS {
        let mut doc = Document::new(Project::new("EQ, filter and stereo"));
        let effect = add(&mut doc, kind);
        for index in 0..kind.descriptors().len() {
            doc.dispatch(
                Command::AddAutomation {
                    name: None,
                    target: AutomationTarget::EffectParam {
                        track: TrackId::MASTER,
                        effect,
                        param: index as u32,
                    },
                    points: None,
                },
                None,
            )
            .unwrap();
        }
        assert_eq!(doc.project().automations.len(), kind.descriptors().len());
        doc.project().check().unwrap();
        doc.dispatch(
            Command::RemoveEffect {
                track: TrackId::MASTER,
                effect,
            },
            None,
        )
        .unwrap();
        assert!(doc.project().automations.is_empty());
        doc.undo().unwrap();
        assert_eq!(doc.project().automations.len(), kind.descriptors().len());
    }
}

/// The project as JSON with the settings of its only effect replaced.
fn with_settings(doc: &Document, settings: serde_json::Value) -> Project {
    let mut json = serde_json::to_value(doc.project()).unwrap();
    json["mixer"]["tracks"][0]["effects"][0]["params"] = settings;
    serde_json::from_value(json).unwrap()
}

#[test]
fn settings_saved_with_only_their_type_load_as_the_defaults() {
    for kind in KINDS {
        let mut doc = Document::new(Project::new("EQ, filter and stereo"));
        add(&mut doc, kind);
        let tag = serde_json::to_value(kind).unwrap();
        let loaded = with_settings(&doc, serde_json::json!({ "type": tag }));
        assert_eq!(
            loaded.mixer.tracks[0].effects[0].params,
            kind.default_params(),
            "{kind:?}"
        );
        loaded.check().unwrap();
    }
}
