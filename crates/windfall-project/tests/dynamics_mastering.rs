//! Dynamics, transient and mastering effect settings through the real
//! document, history and version-one file format.

use windfall_dsp::multiband::{TransientListen, TransientShaperParams};
use windfall_dsp::{EffectKind, EffectParams, ParamInfo, ParamKind, ParamScale};
use windfall_project::*;

const KINDS: [EffectKind; 7] = [
    EffectKind::MultibandCompressor,
    EffectKind::MultibandMaximizer,
    EffectKind::OneKnob,
    EffectKind::TransientShaper,
    EffectKind::BassHarmonics,
    EffectKind::StageStack,
    EffectKind::MasterLimiter,
];

/// The values a gesture moves a control through: somewhere on the way, then
/// an end of the range that is not where it started. A switch has only the
/// other position.
fn travel(info: &ParamInfo) -> Vec<f32> {
    let last = if info.default == info.max {
        info.min
    } else {
        info.max
    };
    match info.kind {
        ParamKind::Toggle => vec![1.0 - info.default],
        ParamKind::Choice | ParamKind::Integer => {
            let middle = ((info.min + info.max) * 0.5).round();
            if middle == last {
                vec![last]
            } else {
                vec![middle, last]
            }
        }
        ParamKind::Float => vec![(info.min + info.max) * 0.5, last],
    }
}

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

fn target(effect: EffectId, param: usize) -> AutomationTarget {
    AutomationTarget::EffectParam {
        track: TrackId::MASTER,
        effect,
        param: param as u32,
    }
}

fn same_content(a: &Project, b: &Project) -> bool {
    let mut b = b.clone();
    b.next_id = a.next_id;
    *a == b
}

#[test]
fn settings_gestures_noops_and_disk_save_reopen_preserve_every_control() {
    let directory = tempfile::tempdir().unwrap();
    for kind in KINDS {
        let mut doc = Document::new(Project::new("Dynamics family"));
        let effect = add(&mut doc, kind);
        assert_eq!(params(&doc, effect), kind.default_params());
        for (index, info) in kind.descriptors().iter().enumerate() {
            let before = params(&doc, effect);
            let cursor = doc.history().cursor;
            let gesture = Some(100 + index as u64);
            let values = travel(info);
            let final_value = *values.last().unwrap();
            for value in values {
                doc.dispatch(set(effect, index, value), gesture).unwrap();
                assert_eq!(params(&doc, effect).get(index), Some(value));
                doc.project().check().unwrap();
            }
            assert_eq!(doc.history().cursor, cursor + 1);
            let edited = params(&doc, effect);
            doc.undo().unwrap();
            assert_eq!(params(&doc, effect), before);
            doc.redo().unwrap();
            assert_eq!(params(&doc, effect), edited);
            doc.mark_saved();
            let snapshot = doc.project().clone();
            let cursor = doc.history().cursor;
            let noop = doc.dispatch(set(effect, index, final_value), None).unwrap();
            assert!(noop.touched.is_empty());
            assert_eq!(doc.history().cursor, cursor);
            assert_eq!(doc.project(), &snapshot);
            assert!(!doc.is_dirty());
        }
        let path = directory.path().join(format!("{kind:?}.windfall"));
        file::save(doc.project(), &path).unwrap();
        assert_eq!(file::load(&path).unwrap(), *doc.project());
        assert_eq!(doc.project().format_version, 1);
    }
}

#[test]
fn v1_defaults_and_settings_saved_before_split_outputs_load_without_a_format_bump() {
    for kind in KINDS {
        let mut doc = Document::new(Project::new("Defaults"));
        let effect = add(&mut doc, kind);
        let mut json = serde_json::to_value(doc.project()).unwrap();
        json["mixer"]["tracks"][0]["effects"][0]["params"] = serde_json::json!({"type": kind});
        let loaded = file::from_json(&json.to_string()).unwrap();
        assert_eq!(loaded, *doc.project());
        for malformed in [
            serde_json::json!({"type": "unknownFilter"}),
            serde_json::Value::Null,
        ] {
            json["mixer"]["tracks"][0]["effects"][0]["params"] = malformed;
            assert!(file::from_json(&json.to_string()).is_err());
        }
        if kind == EffectKind::TransientShaper {
            // A shaper saved before it had split outputs has no `listen`.
            json["mixer"]["tracks"][0]["effects"][0]["params"] = serde_json::json!({
                "type": "transientShaper",
                "attack": 0.5,
                "sustain": -0.25,
                "sensitivity": 2.0
            });
            let loaded = file::from_json(&json.to_string()).unwrap();
            assert_eq!(
                loaded.mixer.tracks[0].effects[0].params,
                EffectParams::TransientShaper(TransientShaperParams {
                    attack: 0.5,
                    sustain: -0.25,
                    sensitivity: 2.0,
                    listen: TransientListen::Both,
                })
            );
            for (index, listen) in [
                TransientListen::Both,
                TransientListen::Attack,
                TransientListen::Sustain,
            ]
            .into_iter()
            .enumerate()
            {
                // The output choice is the fourth control, after the three
                // that automation already addressed by index.
                doc.dispatch(set(effect, 3, index as f32), None).unwrap();
                let EffectParams::TransientShaper(settings) = params(&doc, effect) else {
                    unreachable!()
                };
                assert_eq!(settings.listen, listen);
                let loaded = file::from_json(&file::to_json(doc.project()).unwrap()).unwrap();
                assert_eq!(loaded, *doc.project());
            }
            let mut json = serde_json::to_value(doc.project()).unwrap();
            json["mixer"]["tracks"][0]["effects"][0]["params"]["listen"] =
                serde_json::json!("neither");
            assert!(file::from_json(&json.to_string()).is_err());
        }
    }
}

#[test]
fn invalid_edits_are_atomic_and_finite_extrema_use_descriptor_sanitization() {
    for kind in KINDS {
        let mut doc = Document::new(Project::new("Validation"));
        let effect = add(&mut doc, kind);
        doc.mark_saved();
        for command in [
            set(effect, kind.descriptors().len(), 1.0),
            set(effect, 0, f32::NAN),
            Command::SetEffectParams {
                track: TrackId::MASTER,
                effect,
                params: EffectKind::Eq.default_params(),
            },
            Command::AddAutomation {
                name: None,
                target: target(effect, kind.descriptors().len()),
                points: None,
            },
        ] {
            let before = doc.project().clone();
            let history = doc.history();
            assert!(doc.dispatch(command, None).is_err());
            assert_eq!(doc.project(), &before);
            assert_eq!(doc.history(), history);
            assert!(!doc.is_dirty());
        }
        for (index, info) in kind.descriptors().iter().enumerate() {
            // Project commands treat infinities as endpoints; direct DSP
            // parameter writes instead use that processor's nonfinite policy.
            for (value, expected) in [
                (-f32::MAX, info.min),
                (f32::MAX, info.max),
                (f32::NEG_INFINITY, info.min),
                (f32::INFINITY, info.max),
            ] {
                doc.dispatch(set(effect, index, value), None).unwrap();
                assert_eq!(params(&doc, effect).get(index), Some(expected));
                doc.project().check().unwrap();
            }
        }
    }
}

#[test]
fn every_control_has_the_correct_automation_range_and_removal_history_restores_curves() {
    for kind in KINDS {
        let mut doc = Document::new(Project::new("Automation"));
        let effect = add(&mut doc, kind);
        for (index, info) in kind.descriptors().iter().enumerate() {
            let target = target(effect, index);
            let range = doc.project().automation_range(&target).unwrap();
            assert_eq!((range.min, range.max), (info.min, info.max));
            let middle = if info.kind == ParamKind::Toggle {
                info.max
            } else if info.kind == ParamKind::Choice {
                ((info.min + info.max) * 0.5).round()
            } else if info.scale == ParamScale::Logarithmic {
                (info.min * info.max).sqrt()
            } else {
                (info.min + info.max) * 0.5
            };
            assert!((range.value(0.5) - middle).abs() < middle.abs().max(1.0) * 1e-5);
            assert_eq!(range.value(0.0), info.min);
            assert!((range.value(1.0) - info.max).abs() < info.max.abs().max(1.0) * 1e-5);
            for normalized in [0.0, 0.2, 0.5, 0.8, 1.0] {
                let value = range.value(normalized);
                assert!(
                    (range.value(range.normalized(value)) - value).abs()
                        < value.abs().max(1.0) * 1e-5
                );
            }
            let id = AutomationId(
                doc.dispatch(
                    Command::AddAutomation {
                        name: None,
                        target,
                        points: None,
                    },
                    None,
                )
                .unwrap()
                .created[0],
            );
            let default = &doc.project().automations.last().unwrap().points[0];
            assert!(
                (range.value(default.value) - params(&doc, effect).get(index).unwrap()).abs()
                    < info.default.abs().max(1.0) * 1e-4
            );
            doc.dispatch(
                Command::SetAutomationPoints {
                    id,
                    points: vec![
                        AutomationPoint {
                            tick: 0,
                            value: 0.0,
                            curve: 0.0,
                            hold: false,
                        },
                        AutomationPoint {
                            tick: 960,
                            value: 1.0,
                            curve: 0.0,
                            hold: false,
                        },
                    ],
                },
                None,
            )
            .unwrap();
        }
        doc.project().check().unwrap();
        let before = doc.project().clone();
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
        assert!(same_content(doc.project(), &before));
        doc.redo().unwrap();
        assert!(doc.project().automations.is_empty());
        doc.undo().unwrap();
        let replacement = EffectId(
            doc.dispatch(
                Command::ReplaceEffect {
                    track: TrackId::MASTER,
                    effect,
                    kind,
                },
                None,
            )
            .unwrap()
            .created[0],
        );
        assert_ne!(replacement, effect);
        assert_eq!(params(&doc, replacement), kind.default_params());
        assert!(doc.project().automations.is_empty());
        doc.undo().unwrap();
        assert!(same_content(doc.project(), &before));
        let reopened = file::from_json(&file::to_json(doc.project()).unwrap()).unwrap();
        assert_eq!(reopened, *doc.project());
    }
}
