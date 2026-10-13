//! Acceptance cases written during the edit-only pass; not executed by worker.
use windfall_project::*;

fn create(doc: &mut Document, command: Command) -> u32 {
    doc.dispatch(command, None).unwrap().created[0]
}
fn effect(doc: &mut Document, track: TrackId, kind: EffectKind) -> EffectId {
    EffectId(create(
        doc,
        Command::AddEffect {
            track,
            kind,
            index: None,
        },
    ))
}
fn link(source: EffectId, output: u8, target: AutomationTarget) -> ControllerLink {
    ControllerLink {
        source: ControlSource::Internal {
            controller: source.0,
            output,
        },
        target: LinkTarget::Param { target },
        map: ControlMap {
            pickup: PickupMode::Off,
            ..Default::default()
        },
    }
}
fn set(doc: &mut Document, links: Vec<ControllerLink>) -> Result<Applied, CommandError> {
    doc.dispatch(Command::SetControllerLinks { links }, None)
}
fn config(
    doc: &mut Document,
    configs: Vec<InternalControllerSettings>,
) -> Result<Applied, CommandError> {
    doc.dispatch(
        Command::SetInternalControllerSettings { settings: configs },
        None,
    )
}

#[test]
fn every_source_has_checked_stable_normalized_ports_and_saved_settings() {
    let mut doc = Document::new(Project::new("Sources"));
    for (kind, ports) in [
        (EffectKind::NoteEnvelope, 1),
        (EffectKind::KeyboardSource, 2),
        (EffectKind::FormulaSource, 1),
        (EffectKind::EnvelopeFollower, 2),
        (EffectKind::XyPad, 2),
        (EffectKind::XyzPad, 3),
        (EffectKind::ControlSurface, 8),
        (EffectKind::VoltageSource, 8),
    ] {
        let id = effect(&mut doc, TrackId::MASTER, kind);
        assert_eq!(internal_controllers::outputs(kind).len(), ports);
        config(
            &mut doc,
            vec![InternalControllerSettings {
                controller: id.0,
                expression: (kind == EffectKind::FormulaSource).then(|| "a * b + c".into()),
                ..Default::default()
            }],
        )
        .unwrap();
        let before = doc.project().clone();
        let reopened = file::from_json(&file::to_json(&before).unwrap()).unwrap();
        assert_eq!(reopened, before);
        reopened.check().unwrap();
        let bad = link(
            id,
            ports as u8,
            AutomationTarget::TrackPan {
                track: TrackId::MASTER,
            },
        );
        assert!(set(&mut doc, vec![bad]).is_err());
        assert_eq!(doc.project(), &before);
    }
}
#[test]
fn source_deletion_unlinks_and_removes_configuration_in_the_same_undo_step() {
    let mut doc = Document::new(Project::new("Removal"));
    let id = effect(&mut doc, TrackId::MASTER, EffectKind::FormulaSource);
    config(
        &mut doc,
        vec![InternalControllerSettings {
            controller: id.0,
            expression: Some("a + c".into()),
            ..Default::default()
        }],
    )
    .unwrap();
    set(
        &mut doc,
        vec![link(
            id,
            0,
            AutomationTarget::TrackPan {
                track: TrackId::MASTER,
            },
        )],
    )
    .unwrap();
    let before = doc.project().clone();
    let count = doc.history().entries.len();
    doc.dispatch(
        Command::RemoveEffect {
            track: TrackId::MASTER,
            effect: id,
        },
        None,
    )
    .unwrap();
    assert!(doc.project().settings.controller_links.is_empty());
    assert!(doc.project().settings.internal_controllers.is_empty());
    assert_eq!(doc.history().entries.len(), count + 1);
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
    doc.redo().unwrap();
    assert!(doc.project().settings.controller_links.is_empty());
}
#[test]
fn direct_and_indirect_formula_input_cycles_are_refused_atomically() {
    let mut doc = Document::new(Project::new("Cycles"));
    let a = effect(&mut doc, TrackId::MASTER, EffectKind::FormulaSource);
    let b = effect(&mut doc, TrackId::MASTER, EffectKind::FormulaSource);
    let target = |effect| AutomationTarget::EffectParam {
        track: TrackId::MASTER,
        effect,
        param: 0,
    };
    let before = doc.project().clone();
    assert!(set(&mut doc, vec![link(a, 0, target(a))]).is_err());
    assert_eq!(doc.project(), &before);
    set(&mut doc, vec![link(a, 0, target(b))]).unwrap();
    let before = doc.project().clone();
    let history = doc.history();
    assert!(set(&mut doc, vec![link(a, 0, target(b)), link(b, 0, target(a))]).is_err());
    assert_eq!(doc.project(), &before);
    assert_eq!(doc.history(), history);
}
#[test]
fn peak_audio_feedback_through_formula_and_upstream_gain_is_refused() {
    let mut doc = Document::new(Project::new("Audio cycle"));
    let input = TrackId(create(&mut doc, Command::AddMixerTrack { name: None }));
    let peak = effect(&mut doc, TrackId::MASTER, EffectKind::EnvelopeFollower);
    let formula = effect(&mut doc, TrackId::MASTER, EffectKind::FormulaSource);
    let links = vec![
        link(
            peak,
            0,
            AutomationTarget::EffectParam {
                track: TrackId::MASTER,
                effect: formula,
                param: 0,
            },
        ),
        link(formula, 0, AutomationTarget::TrackVolume { track: input }),
    ];
    let before = doc.project().clone();
    assert!(set(&mut doc, links).is_err());
    assert_eq!(doc.project(), &before);
    // Post-detector fader has no route back to the detector and is allowed.
    set(
        &mut doc,
        vec![link(
            peak,
            0,
            AutomationTarget::TrackVolume {
                track: TrackId::MASTER,
            },
        )],
    )
    .unwrap();
}
#[test]
fn malformed_configuration_preserves_project_history_and_installed_formula() {
    let mut doc = Document::new(Project::new("Formula"));
    let id = effect(&mut doc, TrackId::MASTER, EffectKind::FormulaSource);
    config(
        &mut doc,
        vec![InternalControllerSettings {
            controller: id.0,
            expression: Some("a * b + c".into()),
            ..Default::default()
        }],
    )
    .unwrap();
    let before = doc.project().clone();
    let history = doc.history();
    for expression in ["a +", "eval(a)", "1e100", &"a".repeat(257)] {
        assert!(
            config(
                &mut doc,
                vec![InternalControllerSettings {
                    controller: id.0,
                    expression: Some(expression.into()),
                    ..Default::default()
                }]
            )
            .is_err()
        );
        assert_eq!(doc.project(), &before);
        assert_eq!(doc.history(), history);
    }
}
#[test]
fn legacy_default_omits_configuration_and_missing_sources_are_refused() {
    let mut doc = Document::new(Project::new("Legacy"));
    let json = file::to_json(doc.project()).unwrap();
    assert!(!json.contains("internalControllers"));
    assert_eq!(file::from_json(&json).unwrap(), *doc.project());
    assert!(
        set(
            &mut doc,
            vec![link(
                EffectId(999),
                0,
                AutomationTarget::TrackPan {
                    track: TrackId::MASTER
                }
            )]
        )
        .is_err()
    );
}
#[test]
fn dashboard_layout_and_internal_link_save_reopen_and_unlink_on_channel_deletion() {
    let mut doc = Document::new(Project::new("Dashboard"));
    let channel = ChannelId(create(
        &mut doc,
        Command::AddChannel {
            name: Some("Hardware panel".into()),
            sample: None,
            instrument: Some(InstrumentKind::Dashboard),
            index: None,
            mixer_track: None,
        },
    ));
    config(
        &mut doc,
        vec![InternalControllerSettings {
            controller: channel.0,
            controls: vec![SurfaceControl {
                output: 0,
                name: "Cutoff".into(),
                kind: SurfaceControlKind::Slider,
                column: 2,
                row: 1,
            }],
            ..Default::default()
        }],
    )
    .unwrap();
    set(
        &mut doc,
        vec![link(
            EffectId(channel.0),
            0,
            AutomationTarget::TrackPan {
                track: TrackId::MASTER,
            },
        )],
    )
    .unwrap();
    let before = doc.project().clone();
    assert_eq!(
        file::from_json(&file::to_json(&before).unwrap()).unwrap(),
        before
    );
    doc.dispatch(Command::RemoveChannel { id: channel }, None)
        .unwrap();
    assert!(doc.project().settings.controller_links.is_empty());
    assert!(doc.project().settings.internal_controllers.is_empty());
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
}
#[test]
fn drawn_envelopes_layout_and_dc_calibration_reject_unbounded_or_conflicting_data() {
    let mut doc = Document::new(Project::new("Assets"));
    let env = effect(&mut doc, TrackId::MASTER, EffectKind::NoteEnvelope);
    let points = vec![
        ControlPoint {
            position: 0.0,
            value: 0.0,
        },
        ControlPoint {
            position: 0.5,
            value: 1.0,
        },
        ControlPoint {
            position: 1.0,
            value: 0.4,
        },
    ];
    let cfg = InternalControllerSettings {
        controller: env.0,
        envelope: points,
        duration_ms: 1000.0,
        ..Default::default()
    };
    config(&mut doc, vec![cfg.clone()]).unwrap();
    let before = doc.project().clone();
    let mut bad = cfg;
    bad.envelope[1].position = 0.0;
    assert!(config(&mut doc, vec![bad]).is_err());
    assert_eq!(doc.project(), &before);
    let dc = effect(&mut doc, TrackId::MASTER, EffectKind::VoltageSource);
    let route = VoltageRoute {
        output: 0,
        channel: 2,
        zero: -0.2,
        scale: 0.8,
    };
    config(
        &mut doc,
        vec![InternalControllerSettings {
            controller: dc.0,
            voltage_routes: vec![route],
            ..Default::default()
        }],
    )
    .unwrap();
    let mut bad = route;
    bad.channel = 0;
    assert!(
        config(
            &mut doc,
            vec![InternalControllerSettings {
                controller: dc.0,
                voltage_routes: vec![bad],
                ..Default::default()
            }]
        )
        .is_err()
    );
}

#[test]
fn source_specific_settings_and_nonfinite_duration_are_refused_without_history_changes() {
    let mut doc = Document::new(Project::new("Settings bounds"));
    let id = effect(&mut doc, TrackId::MASTER, EffectKind::FormulaSource);
    let before = doc.project().clone();
    let history = doc.history();
    for config_value in [
        InternalControllerSettings {
            controller: id.0,
            duration_ms: f32::NAN,
            ..Default::default()
        },
        InternalControllerSettings {
            controller: id.0,
            rms: true,
            ..Default::default()
        },
        InternalControllerSettings {
            controller: id.0,
            sync_beats: Some(1.0),
            ..Default::default()
        },
        InternalControllerSettings {
            controller: id.0,
            envelope: vec![
                ControlPoint {
                    position: 0.0,
                    value: 0.0,
                },
                ControlPoint {
                    position: 1.0,
                    value: 1.0,
                },
            ],
            ..Default::default()
        },
    ] {
        assert!(config(&mut doc, vec![config_value]).is_err());
        assert_eq!(doc.project(), &before);
        assert_eq!(doc.history(), history);
    }
}

#[test]
fn synced_source_tempo_links_are_checked_bounded_undoable_and_saved() {
    let mut doc = Document::new(Project::new("Clock links"));
    let source = effect(&mut doc, TrackId::MASTER, EffectKind::EnvelopeFollower);
    config(
        &mut doc,
        vec![InternalControllerSettings {
            controller: source.0,
            sync_beats: Some(0.25),
            ..Default::default()
        }],
    )
    .unwrap();
    let mut connection = link(source, 1, AutomationTarget::Tempo);
    connection.map.min = (90.0 - 10.0) / 512.0;
    connection.map.max = (150.0 - 10.0) / 512.0;
    set(&mut doc, vec![connection.clone()]).unwrap();
    assert_eq!(doc.project().settings.controller_links[0], connection);
    let before = doc.snapshot(None);
    let mut invalid = connection.clone();
    invalid.map.max = 2.0;
    assert!(set(&mut doc, vec![invalid]).is_err());
    assert_eq!(doc.snapshot(None), before);
    let reopened = file::from_json(&file::to_json(doc.project()).unwrap()).unwrap();
    assert_eq!(reopened.settings.controller_links, vec![connection]);
    doc.undo().unwrap();
    assert!(doc.project().settings.controller_links.is_empty());
    doc.redo().unwrap();
    assert_eq!(doc.project().settings.controller_links.len(), 1);
}
