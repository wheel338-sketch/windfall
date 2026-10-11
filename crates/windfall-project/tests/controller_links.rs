//! Controller links through the real document: commands, history, the file
//! format, and what happens to a link when its target leaves the project.
use windfall_project::controller_links::{
    MAX_CONTROLLER_LINKS, command_targets, links_problem, target_command,
};
use windfall_project::{file, *};

fn cc(number: u8) -> ControlSource {
    ControlSource::Midi {
        device: None,
        channel: Some(1),
        control: MidiControl::Cc { number },
    }
}

fn link(number: u8, target: AutomationTarget) -> ControllerLink {
    ControllerLink {
        source: cc(number),
        target: LinkTarget::Param { target },
        map: ControlMap::default(),
    }
}

struct Fixture {
    doc: Document,
    channel: ChannelId,
    synth: ChannelId,
    track: TrackId,
    other: TrackId,
    effect: EffectId,
}

fn fixture() -> Fixture {
    let mut doc = Document::new(Project::new("Links"));
    let mut create = |command| doc.dispatch(command, None).unwrap().created[0];
    let track = TrackId(create(Command::AddMixerTrack { name: None }));
    let other = TrackId(create(Command::AddMixerTrack { name: None }));
    let channel = ChannelId(create(Command::AddChannel {
        name: Some("Kick".into()),
        sample: None,
        instrument: None,
        index: None,
        mixer_track: Some(track),
    }));
    let synth = ChannelId(create(Command::AddChannel {
        name: Some("Lead".into()),
        sample: None,
        instrument: Some(InstrumentKind::SubtractiveSynth),
        index: None,
        mixer_track: Some(track),
    }));
    let effect = EffectId(create(Command::AddEffect {
        track,
        kind: EffectKind::ALL[0],
        index: None,
    }));
    Fixture {
        doc,
        channel,
        synth,
        track,
        other,
        effect,
    }
}

fn set(doc: &mut Document, links: Vec<ControllerLink>) -> Result<Applied, CommandError> {
    doc.dispatch(Command::SetControllerLinks { links }, None)
}

fn links(doc: &Document) -> &[ControllerLink] {
    &doc.project().settings.controller_links
}

#[test]
fn links_are_one_undo_step_and_survive_save_and_reopen() {
    let mut f = fixture();
    let before = f.doc.project().clone();
    let entries = f.doc.history().entries.len();
    let wanted = vec![
        link(7, AutomationTarget::ChannelVolume { channel: f.channel }),
        ControllerLink {
            source: ControlSource::Midi {
                device: Some("Keystation".into()),
                channel: None,
                control: MidiControl::PitchBend,
            },
            target: LinkTarget::Param {
                target: AutomationTarget::InstrumentParam {
                    channel: f.synth,
                    param: 0,
                },
            },
            map: ControlMap {
                min: 0.9,
                max: 0.1,
                formula: Some("x ^ 2".into()),
                smoothing_ms: 35.0,
                pickup: PickupMode::On,
                mode: SourceMode::Absolute,
            },
        },
        ControllerLink {
            source: ControlSource::Midi {
                device: None,
                channel: Some(10),
                control: MidiControl::Note { key: 36 },
            },
            target: LinkTarget::Action {
                action: ControlAction::TogglePlay,
            },
            map: ControlMap::default(),
        },
        ControllerLink {
            source: cc(20),
            target: LinkTarget::Slot {
                slot: SlotTarget::MixerPan { index: 1 },
            },
            map: ControlMap {
                mode: SourceMode::RelativeBinaryOffset,
                ..ControlMap::default()
            },
        },
        ControllerLink {
            source: ControlSource::Internal {
                controller: f.effect.0,
                output: 0,
            },
            target: LinkTarget::Param {
                target: AutomationTarget::TrackPan { track: f.track },
            },
            map: ControlMap::default(),
        },
    ];
    let applied = set(&mut f.doc, wanted.clone()).unwrap();
    assert!(applied.touched.settings);
    assert_eq!(applied.label, "Change controller links");
    assert_eq!(f.doc.history().entries.len(), entries + 1);
    assert_eq!(links(&f.doc), wanted);

    // Nothing but the links changed.
    let mut rest = f.doc.project().clone();
    rest.settings.controller_links.clear();
    assert_eq!(rest, before);

    assert!(f.doc.undo().unwrap().settings);
    assert_eq!(f.doc.project(), &before);
    assert!(f.doc.redo().unwrap().settings);
    assert_eq!(links(&f.doc), wanted);

    let json = file::to_json(f.doc.project()).unwrap();
    assert_eq!(file::from_json(&json).unwrap(), *f.doc.project());
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("links.windfall");
    file::save(f.doc.project(), &path).unwrap();
    let reopened = file::load(&path).unwrap();
    assert_eq!(reopened.settings.controller_links, wanted);
    reopened.check().unwrap();

    // The same list again is no edit.
    let entries = f.doc.history().entries.len();
    assert!(set(&mut f.doc, wanted).unwrap().touched.is_empty());
    assert_eq!(f.doc.history().entries.len(), entries);
}

#[test]
fn a_project_saved_before_links_existed_opens_with_none_and_saves_none() {
    let project = Project::new("Old");
    let json = file::to_json(&project).unwrap();
    // A project with no links writes no field at all, as old files have it.
    assert!(!json.contains("controllerLinks"));
    let opened = file::from_json(&json).unwrap();
    assert!(opened.settings.controller_links.is_empty());
    assert_eq!(opened, project);
}

#[test]
fn invalid_links_are_refused_and_change_nothing() {
    let mut f = fixture();
    let good = link(7, AutomationTarget::ChannelVolume { channel: f.channel });
    set(&mut f.doc, vec![good.clone()]).unwrap();
    let before = f.doc.project().clone();
    let history = f.doc.history();
    let mut bad_channel = good.clone();
    bad_channel.source = ControlSource::Midi {
        device: None,
        channel: Some(0),
        control: MidiControl::Cc { number: 7 },
    };
    let mut bad_number = good.clone();
    bad_number.source = cc(128);
    let mut bad_range = good.clone();
    bad_range.map.max = 1.2;
    let mut bad_formula = good.clone();
    bad_formula.map.formula = Some("1 +".into());
    let mut bad_smoothing = good.clone();
    bad_smoothing.map.smoothing_ms = -4.0;
    let mut bad_device = good.clone();
    bad_device.source = ControlSource::Midi {
        device: Some(String::new()),
        channel: None,
        control: MidiControl::Cc { number: 7 },
    };
    let missing = link(
        8,
        AutomationTarget::ChannelVolume {
            channel: ChannelId(9_999),
        },
    );
    let no_param = link(
        9,
        AutomationTarget::InstrumentParam {
            channel: f.synth,
            param: 9_999,
        },
    );
    let sampler_param = link(
        9,
        AutomationTarget::InstrumentParam {
            channel: f.channel,
            param: 0,
        },
    );
    let far = ControllerLink {
        source: cc(10),
        target: LinkTarget::Slot {
            slot: SlotTarget::ChannelPan { index: 128 },
        },
        map: ControlMap::default(),
    };
    for bad in [
        vec![bad_channel],
        vec![bad_number],
        vec![bad_range],
        vec![bad_formula],
        vec![bad_smoothing],
        vec![bad_device],
        vec![missing],
        vec![no_param],
        vec![sampler_param],
        vec![far],
        vec![good.clone(), good.clone()],
    ] {
        let error = set(&mut f.doc, bad).unwrap_err();
        assert!(!error.to_string().is_empty());
        assert_eq!(f.doc.project(), &before);
        assert_eq!(f.doc.history(), history);
    }
    let many: Vec<ControllerLink> = (0..=MAX_CONTROLLER_LINKS)
        .map(|index| ControllerLink {
            source: ControlSource::Midi {
                device: None,
                channel: Some(1 + (index / 128) as u8),
                control: MidiControl::Cc {
                    number: (index % 128) as u8,
                },
            },
            target: LinkTarget::Action {
                action: ControlAction::Play,
            },
            map: ControlMap::default(),
        })
        .collect();
    assert!(set(&mut f.doc, many.clone()).is_err());
    assert!(set(&mut f.doc, many[..MAX_CONTROLLER_LINKS].to_vec()).is_ok());
    // One source may move several targets, and one target may have several sources.
    let shared = vec![
        link(7, AutomationTarget::ChannelVolume { channel: f.channel }),
        link(7, AutomationTarget::ChannelPan { channel: f.channel }),
        link(8, AutomationTarget::ChannelPan { channel: f.channel }),
    ];
    assert!(set(&mut f.doc, shared).is_ok());
}

#[test]
fn a_link_leaves_with_its_target_in_the_same_step_and_returns_on_undo() {
    let mut f = fixture();
    let kept = vec![
        link(1, AutomationTarget::Tempo),
        link(2, AutomationTarget::TrackVolume { track: f.other }),
        ControllerLink {
            source: cc(3),
            target: LinkTarget::Action {
                action: ControlAction::Stop,
            },
            map: ControlMap::default(),
        },
    ];
    let mut all = kept.clone();
    all.extend([
        link(10, AutomationTarget::ChannelVolume { channel: f.synth }),
        link(11, AutomationTarget::ChannelPan { channel: f.synth }),
        link(
            12,
            AutomationTarget::InstrumentParam {
                channel: f.synth,
                param: 1,
            },
        ),
    ]);
    set(&mut f.doc, all.clone()).unwrap();
    let with_links = f.doc.project().clone();
    let entries = f.doc.history().entries.len();

    f.doc
        .dispatch(Command::RemoveChannel { id: f.synth }, None)
        .unwrap();
    assert_eq!(links(&f.doc), kept);
    assert_eq!(f.doc.history().entries.len(), entries + 1, "one step");
    f.doc.project().check().unwrap();

    f.doc.undo().unwrap();
    assert_eq!(f.doc.project(), &with_links);
    assert_eq!(links(&f.doc), all);
    f.doc.redo().unwrap();
    assert_eq!(links(&f.doc), kept);
}

#[test]
fn removing_a_track_or_an_effect_unlinks_everything_on_it() {
    let mut f = fixture();
    let effect_links = vec![
        link(
            20,
            AutomationTarget::EffectParam {
                track: f.track,
                effect: f.effect,
                param: 0,
            },
        ),
        link(
            21,
            AutomationTarget::EffectMix {
                track: f.track,
                effect: f.effect,
            },
        ),
    ];
    let track_links = vec![
        link(22, AutomationTarget::TrackVolume { track: f.track }),
        link(23, AutomationTarget::TrackPan { track: f.track }),
        link(
            24,
            AutomationTarget::TrackParam {
                track: f.track,
                param: 0,
            },
        ),
    ];
    let stays = link(25, AutomationTarget::TrackVolume { track: f.other });
    let mut all = vec![stays.clone()];
    all.extend(effect_links.clone());
    all.extend(track_links.clone());
    set(&mut f.doc, all.clone()).unwrap();

    f.doc
        .dispatch(
            Command::RemoveEffect {
                track: f.track,
                effect: f.effect,
            },
            None,
        )
        .unwrap();
    let mut after_effect = vec![stays.clone()];
    after_effect.extend(track_links.clone());
    assert_eq!(links(&f.doc), after_effect);
    f.doc.undo().unwrap();
    assert_eq!(links(&f.doc), all);

    f.doc
        .dispatch(Command::RemoveMixerTrack { id: f.track }, None)
        .unwrap();
    assert_eq!(links(&f.doc), vec![stays]);
    f.doc.project().check().unwrap();
    f.doc.undo().unwrap();
    assert_eq!(links(&f.doc), all);
    f.doc.project().check().unwrap();
}

#[test]
fn a_link_follows_its_effect_to_another_track() {
    let mut f = fixture();
    let before = vec![
        link(
            20,
            AutomationTarget::EffectParam {
                track: f.track,
                effect: f.effect,
                param: 0,
            },
        ),
        link(
            21,
            AutomationTarget::EffectMix {
                track: f.track,
                effect: f.effect,
            },
        ),
    ];
    set(&mut f.doc, before.clone()).unwrap();
    f.doc
        .dispatch(
            Command::MoveEffect {
                track: f.track,
                effect: f.effect,
                to_track: Some(f.other),
                index: 0,
            },
            None,
        )
        .unwrap();
    let after = vec![
        link(
            20,
            AutomationTarget::EffectParam {
                track: f.other,
                effect: f.effect,
                param: 0,
            },
        ),
        link(
            21,
            AutomationTarget::EffectMix {
                track: f.other,
                effect: f.effect,
            },
        ),
    ];
    assert_eq!(links(&f.doc), after);
    f.doc.project().check().unwrap();
    f.doc.undo().unwrap();
    assert_eq!(links(&f.doc), before);
}

#[test]
fn a_send_link_leaves_when_the_send_is_removed() {
    let mut f = fixture();
    f.doc
        .dispatch(
            Command::SetSend {
                from: f.track,
                to: f.other,
                gain: Some(0.5),
            },
            None,
        )
        .unwrap();
    let send = link(
        30,
        AutomationTarget::SendGain {
            track: f.track,
            target: f.other,
        },
    );
    set(&mut f.doc, vec![send.clone()]).unwrap();
    f.doc
        .dispatch(
            Command::SetSend {
                from: f.track,
                to: f.other,
                gain: None,
            },
            None,
        )
        .unwrap();
    assert!(links(&f.doc).is_empty());
    f.doc.undo().unwrap();
    assert_eq!(links(&f.doc), vec![send]);
}

#[test]
fn every_target_has_a_command_that_sets_it_and_reads_back_as_that_target() {
    let mut f = fixture();
    f.doc
        .dispatch(
            Command::SetSend {
                from: f.track,
                to: f.other,
                gain: Some(0.5),
            },
            None,
        )
        .unwrap();
    let targets = [
        AutomationTarget::ChannelVolume { channel: f.channel },
        AutomationTarget::ChannelPan { channel: f.channel },
        AutomationTarget::TrackVolume { track: f.track },
        AutomationTarget::TrackPan { track: f.track },
        AutomationTarget::TrackVolume {
            track: TrackId::MASTER,
        },
        AutomationTarget::TrackParam {
            track: f.track,
            param: 0,
        },
        AutomationTarget::SendGain {
            track: f.track,
            target: f.other,
        },
        AutomationTarget::EffectParam {
            track: f.track,
            effect: f.effect,
            param: 0,
        },
        AutomationTarget::EffectMix {
            track: f.track,
            effect: f.effect,
        },
        AutomationTarget::InstrumentParam {
            channel: f.synth,
            param: 0,
        },
        AutomationTarget::Tempo,
    ];
    for target in targets {
        let range = f.doc.project().automation_range(&target).unwrap();
        for normalized in [0.0_f32, 0.3, 1.0] {
            let value = range.value(normalized);
            let command = target_command(f.doc.project(), &target, value).unwrap();
            assert_eq!(
                command_targets(f.doc.project(), &command),
                vec![target],
                "{target:?}"
            );
            f.doc.dispatch(command, Some(1)).unwrap();
            let stored = f.doc.project().automation_stored_value(&target).unwrap();
            assert!(
                (stored - value).abs() <= 1e-4 * value.abs().max(1.0),
                "{target:?}: wanted {value}, stored {stored}"
            );
        }
    }
    // Something that is not there has no command.
    let gone = AutomationTarget::ChannelPan {
        channel: ChannelId(77_777),
    };
    assert!(target_command(f.doc.project(), &gone, 0.0).is_none());
    // Commands that do not set a value name no target.
    let project = f.doc.project();
    assert!(command_targets(project, &Command::AddMixerTrack { name: None }).is_empty());
    assert!(
        command_targets(
            project,
            &Command::UpdateChannel {
                id: f.channel,
                patch: ChannelPatch {
                    name: Some("Renamed".into()),
                    ..ChannelPatch::default()
                },
            }
        )
        .is_empty()
    );
    // A batch names each target once, in order.
    let batch = Command::Batch {
        label: None,
        commands: vec![
            target_command(project, &targets[1], 0.0).unwrap(),
            target_command(project, &targets[0], 0.5).unwrap(),
            target_command(project, &targets[1], 0.2).unwrap(),
        ],
    };
    assert_eq!(
        command_targets(project, &batch),
        vec![targets[1], targets[0]]
    );
}

#[test]
fn positions_resolve_by_order_and_bank_and_miss_where_nothing_is() {
    let f = fixture();
    let project = f.doc.project();
    let volume = |index| SlotTarget::MixerVolume { index };
    assert_eq!(
        volume(0).resolve(project, 0),
        Some(AutomationTarget::TrackVolume { track: f.track })
    );
    assert_eq!(
        volume(1).resolve(project, 0),
        Some(AutomationTarget::TrackVolume { track: f.other })
    );
    // The bank offset moves every position.
    assert_eq!(
        volume(0).resolve(project, 1),
        Some(AutomationTarget::TrackVolume { track: f.other })
    );
    assert_eq!(volume(2).resolve(project, 0), None);
    assert_eq!(volume(0).resolve(project, u32::MAX), None);
    assert_eq!(
        SlotTarget::MixerPan { index: 1 }.resolve(project, 0),
        Some(AutomationTarget::TrackPan { track: f.other })
    );
    assert_eq!(
        SlotTarget::ChannelVolume { index: 1 }.resolve(project, 0),
        Some(AutomationTarget::ChannelVolume { channel: f.synth })
    );
    assert_eq!(
        SlotTarget::ChannelPan { index: 0 }.resolve(project, 0),
        Some(AutomationTarget::ChannelPan { channel: f.channel })
    );
    assert_eq!(
        SlotTarget::ChannelPan { index: 2 }.resolve(project, 0),
        None
    );
    // The master and the tempo are where they are whatever the bank.
    assert_eq!(
        SlotTarget::MasterVolume.resolve(project, 40),
        Some(AutomationTarget::TrackVolume {
            track: TrackId::MASTER
        })
    );
    assert_eq!(
        SlotTarget::Tempo.resolve(project, 40),
        Some(AutomationTarget::Tempo)
    );
}

#[test]
fn links_kept_outside_a_project_may_not_name_its_ids() {
    let f = fixture();
    let by_id = link(7, AutomationTarget::ChannelVolume { channel: f.channel });
    assert!(links_problem(std::slice::from_ref(&by_id), None).is_some());
    assert_eq!(links_problem(&[by_id], Some(f.doc.project())), None);
    let by_place = ControllerLink {
        source: cc(7),
        target: LinkTarget::Slot {
            slot: SlotTarget::ChannelVolume { index: 0 },
        },
        map: ControlMap::default(),
    };
    assert_eq!(links_problem(&[by_place], None), None);
}

#[test]
fn a_file_with_a_dangling_link_is_refused_by_the_check() {
    let mut f = fixture();
    set(
        &mut f.doc,
        vec![link(
            7,
            AutomationTarget::ChannelVolume { channel: f.channel },
        )],
    )
    .unwrap();
    let mut broken = f.doc.project().clone();
    broken.channels.retain(|channel| channel.id != f.channel);
    assert!(broken.check().is_err());
}
