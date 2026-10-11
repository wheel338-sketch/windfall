//! Routing the separately routable outputs of effects: Send Tap's copy and
//! Transient Split's two parts. Commands, validation, history, pruning and
//! the file format.

use windfall_dsp::EffectKind;
use windfall_project::{
    Command, Document, EffectId, EffectOutputRoute, MixerTrackPreset, Project, TrackId, file,
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

fn add_effect(doc: &mut Document, track: TrackId, kind: EffectKind) -> EffectId {
    let applied = doc
        .dispatch(
            Command::AddEffect {
                track,
                kind,
                index: None,
            },
            None,
        )
        .unwrap();
    EffectId(applied.created[0])
}

fn route(effect: EffectId, output: u8, target: TrackId) -> Command {
    Command::SetEffectOutput {
        effect,
        output,
        target: Some(target),
    }
}

fn routes(doc: &Document) -> &[EffectOutputRoute] {
    &doc.project().mixer.effect_outputs
}

/// The project as it was, except that undo keeps the ids a command
/// allocated reserved: `next_id` never goes back.
#[track_caller]
fn assert_restored(doc: &Document, expected: &Project) {
    let mut restored = doc.project().clone();
    assert!(restored.next_id >= expected.next_id);
    restored.next_id = expected.next_id;
    assert_eq!(&restored, expected);
}

struct Fixture {
    doc: Document,
    source: TrackId,
    first: TrackId,
    second: TrackId,
    tap: EffectId,
    split: EffectId,
}

fn fixture() -> Fixture {
    let mut doc = Document::new(Project::new("Effect outputs"));
    let source = add_track(&mut doc, "Source");
    let first = add_track(&mut doc, "First");
    let second = add_track(&mut doc, "Second");
    let tap = add_effect(&mut doc, source, EffectKind::SendTap);
    let split = add_effect(&mut doc, source, EffectKind::TransientSplit);
    Fixture {
        doc,
        source,
        first,
        second,
        tap,
        split,
    }
}

#[test]
fn send_tap_transient_split_and_band_split_have_routable_outputs() {
    for kind in EffectKind::ALL {
        let outputs = kind.aux_outputs();
        match kind {
            EffectKind::SendTap => {
                assert_eq!(outputs.len(), 1);
                assert_eq!((outputs[0].id, outputs[0].label), ("send", "Send"));
                assert!(!outputs[0].splits, "a send is a copy");
            }
            EffectKind::TransientSplit => {
                let names: Vec<_> = outputs.iter().map(|output| output.label).collect();
                assert_eq!(names, ["Transient", "Sustain"]);
                assert!(outputs.iter().all(|output| output.splits));
            }
            EffectKind::BandSplit => {
                let names: Vec<_> = outputs.iter().map(|output| output.label).collect();
                assert_eq!(names, ["Low", "Mid", "High"]);
                assert!(outputs.iter().all(|output| output.splits));
            }
            _ => assert!(outputs.is_empty(), "{kind:?} has no routable output"),
        }
        assert!(outputs.len() <= windfall_dsp::MAX_AUX_OUTPUTS);
        assert_eq!(
            windfall_project::effect_outputs::output_count(kind),
            outputs.len()
        );
    }
}

#[test]
fn routes_are_added_retargeted_and_disconnected_one_undo_step_each() {
    let Fixture {
        mut doc,
        first,
        second,
        tap,
        split,
        ..
    } = fixture();
    let unrouted = doc.project().clone();

    let applied = doc.dispatch(route(tap, 0, first), None).unwrap();
    assert!(applied.created.is_empty());
    assert!(
        applied.touched.mixer,
        "the mixer section carries the routes"
    );
    assert_eq!(
        routes(&doc),
        [EffectOutputRoute {
            effect: tap,
            output: 0,
            target: first
        }]
    );
    let one = doc.project().clone();

    doc.dispatch(route(split, 1, second), None).unwrap();
    doc.dispatch(route(split, 0, first), None).unwrap();
    assert_eq!(routes(&doc).len(), 3);

    // Retargeting replaces the route of that output and leaves the others.
    doc.dispatch(route(tap, 0, second), None).unwrap();
    assert_eq!(routes(&doc).len(), 3);
    assert_eq!(routes(&doc)[0].target, second);
    let retargeted = doc.project().clone();

    // Disconnecting removes just that output's route.
    doc.dispatch(
        Command::SetEffectOutput {
            effect: split,
            output: 1,
            target: None,
        },
        None,
    )
    .unwrap();
    assert_eq!(routes(&doc).len(), 2);
    assert!(
        routes(&doc)
            .iter()
            .all(|route| (route.effect, route.output) != (split, 1))
    );

    // Disconnecting what is not connected, or routing to where it already
    // goes, changes nothing and leaves no history entry.
    let history = doc.history().entries.len();
    let same = doc
        .dispatch(
            Command::SetEffectOutput {
                effect: split,
                output: 1,
                target: None,
            },
            None,
        )
        .unwrap();
    assert!(same.touched.is_empty());
    assert!(
        doc.dispatch(route(tap, 0, second), None)
            .unwrap()
            .touched
            .is_empty()
    );
    assert_eq!(doc.history().entries.len(), history);

    let labels: Vec<_> = doc
        .history()
        .entries
        .iter()
        .map(|entry| entry.label.clone())
        .collect();
    assert_eq!(labels[labels.len() - 1], "Disconnect effect output");
    assert_eq!(labels[labels.len() - 2], "Route effect output");

    doc.undo().expect("undo disconnect");
    assert_eq!(doc.project(), &retargeted);
    for _ in 0..3 {
        doc.undo().expect("undo a route");
    }
    assert_eq!(doc.project(), &one);
    doc.undo().expect("undo the first route");
    assert_eq!(doc.project(), &unrouted);
    for _ in 0..4 {
        doc.redo().expect("redo");
    }
    assert_eq!(doc.project(), &retargeted);
    doc.project().check().unwrap();
}

#[test]
fn invalid_routes_are_refused_and_change_nothing() {
    let Fixture {
        mut doc,
        source,
        first,
        tap,
        split,
        ..
    } = fixture();
    let plain = add_effect(&mut doc, source, EffectKind::Eq);
    let on_master = add_effect(&mut doc, TrackId::MASTER, EffectKind::SendTap);
    let before = doc.project().clone();
    let history = doc.history().entries.len();

    let refused = [
        ("an output the effect lacks", route(tap, 1, first)),
        ("a third split output", route(split, 2, first)),
        ("an effect with no outputs", route(plain, 0, first)),
        ("an effect on the master", route(on_master, 0, first)),
        ("its own track", route(tap, 0, source)),
        ("a missing track", route(tap, 0, TrackId(9_999))),
        ("a missing effect", route(EffectId(9_999), 0, first)),
    ];
    for (what, command) in refused {
        assert!(
            doc.dispatch(command, None).is_err(),
            "{what} must be refused"
        );
        assert_eq!(doc.project(), &before, "{what} must change nothing");
    }
    // Disconnecting an output that does not exist is refused the same way.
    assert!(
        doc.dispatch(
            Command::SetEffectOutput {
                effect: plain,
                output: 0,
                target: None,
            },
            None,
        )
        .is_err()
    );
    assert_eq!(doc.history().entries.len(), history);
}

#[test]
fn routing_loops_through_an_effect_output_are_refused_in_both_directions() {
    let Fixture {
        mut doc,
        source,
        first,
        second,
        tap,
        split,
    } = fixture();

    // First already feeds Source, so Source's tap cannot feed First.
    doc.dispatch(
        Command::SetTrackOutput {
            id: first,
            output: Some(source),
        },
        None,
    )
    .unwrap();
    let before = doc.project().clone();
    assert!(doc.dispatch(route(tap, 0, first), None).is_err());
    // Nor through a longer way round: Second -> First -> Source.
    doc.dispatch(
        Command::SetSend {
            from: second,
            to: first,
            gain: Some(1.0),
        },
        None,
    )
    .unwrap();
    assert!(doc.dispatch(route(split, 0, second), None).is_err());
    doc.undo().expect("undo the send");
    assert_eq!(doc.project(), &before);

    // The other way round: once the tap feeds Second, nothing may route
    // Second back into Source, by output, send or sidechain.
    doc.dispatch(route(tap, 0, second), None).unwrap();
    let routed = doc.project().clone();
    let back = [
        Command::SetTrackOutput {
            id: second,
            output: Some(source),
        },
        Command::SetSend {
            from: second,
            to: source,
            gain: Some(1.0),
        },
        Command::SetSidechain {
            from: second,
            to: source,
            gain: Some(1.0),
        },
    ];
    for command in back {
        assert!(doc.dispatch(command, None).is_err());
        assert_eq!(doc.project(), &routed);
    }

    // Retargeting a route is tested without the route it replaces: moving
    // the tap from Second to the master is allowed.
    doc.dispatch(route(tap, 0, TrackId::MASTER), None).unwrap();
    assert_eq!(routes(&doc)[0].target, TrackId::MASTER);
    doc.project().check().unwrap();
}

#[test]
fn routes_follow_their_effect_and_retire_with_it_in_the_same_undo_step() {
    let Fixture {
        mut doc,
        source,
        first,
        second,
        tap,
        split,
    } = fixture();
    doc.dispatch(route(tap, 0, first), None).unwrap();
    doc.dispatch(route(split, 0, first), None).unwrap();
    doc.dispatch(route(split, 1, second), None).unwrap();
    let routed = doc.project().clone();

    // Reordering inside the chain keeps every route.
    doc.dispatch(
        Command::MoveEffect {
            track: source,
            effect: tap,
            to_track: None,
            index: 1,
        },
        None,
    )
    .unwrap();
    assert_eq!(routes(&doc).len(), 3);
    doc.undo().expect("undo reorder");

    // Deleting an effect retires its routes; one undo restores both.
    doc.dispatch(
        Command::RemoveEffect {
            track: source,
            effect: split,
        },
        None,
    )
    .unwrap();
    assert_eq!(routes(&doc).len(), 1);
    assert_eq!(routes(&doc)[0].effect, tap);
    doc.undo().expect("undo delete effect");
    assert_restored(&doc, &routed);

    // Replacing an effect makes a new effect, so the old routes go.
    doc.dispatch(
        Command::ReplaceEffect {
            track: source,
            effect: split,
            kind: EffectKind::TransientSplit,
        },
        None,
    )
    .unwrap();
    assert_eq!(routes(&doc).len(), 1);
    doc.undo().expect("undo replace");
    assert_restored(&doc, &routed);

    // A copy of an effect starts unrouted.
    let copy = doc
        .dispatch(
            Command::DuplicateEffect {
                track: source,
                effect: tap,
            },
            None,
        )
        .unwrap();
    let copy = EffectId(copy.created[0]);
    assert!(routes(&doc).iter().all(|route| route.effect != copy));
    assert_eq!(routes(&doc).len(), 3);
    doc.undo().expect("undo duplicate");

    // Deleting a destination retires the routes into it only.
    doc.dispatch(Command::RemoveMixerTrack { id: first }, None)
        .unwrap();
    assert_eq!(
        routes(&doc),
        [EffectOutputRoute {
            effect: split,
            output: 1,
            target: second
        }]
    );
    doc.project().check().unwrap();
    doc.undo().expect("undo delete destination");
    assert_restored(&doc, &routed);

    // Deleting the source takes its effects and all their routes.
    doc.dispatch(Command::RemoveMixerTrack { id: source }, None)
        .unwrap();
    assert!(routes(&doc).is_empty());
    doc.project().check().unwrap();
    doc.undo().expect("undo delete source");
    assert_restored(&doc, &routed);
    doc.redo().expect("redo delete source");
    assert!(routes(&doc).is_empty());
    doc.undo().expect("undo again");

    // Moving an effect onto the track it sends to drops that route, which
    // would otherwise feed the track into itself, and keeps the others.
    doc.dispatch(
        Command::MoveEffect {
            track: source,
            effect: split,
            to_track: Some(first),
            index: 0,
        },
        None,
    )
    .unwrap();
    let kept: Vec<_> = routes(&doc)
        .iter()
        .filter(|route| route.effect == split)
        .collect();
    assert_eq!(kept.len(), 1);
    assert_eq!((kept[0].output, kept[0].target), (1, second));
    doc.project().check().unwrap();
    doc.undo().expect("undo move");
    assert_restored(&doc, &routed);

    // Moving it onto the master, which feeds nothing, drops all of them.
    doc.dispatch(
        Command::MoveEffect {
            track: source,
            effect: split,
            to_track: Some(TrackId::MASTER),
            index: 0,
        },
        None,
    )
    .unwrap();
    assert!(routes(&doc).iter().all(|route| route.effect != split));
    doc.project().check().unwrap();
    doc.undo().expect("undo move to master");
    assert_restored(&doc, &routed);
}

#[test]
fn a_moved_effect_loses_a_route_that_would_loop_where_it_now_sits() {
    let Fixture {
        mut doc,
        source,
        first,
        second,
        tap,
        ..
    } = fixture();
    // Source's tap feeds First, and First outputs to Second.
    doc.dispatch(route(tap, 0, first), None).unwrap();
    doc.dispatch(
        Command::SetTrackOutput {
            id: first,
            output: Some(second),
        },
        None,
    )
    .unwrap();
    // On Second the tap would send into First, which feeds Second.
    doc.dispatch(
        Command::MoveEffect {
            track: source,
            effect: tap,
            to_track: Some(second),
            index: 0,
        },
        None,
    )
    .unwrap();
    assert!(
        routes(&doc).is_empty(),
        "a looping route is dropped, not kept broken"
    );
    doc.project().check().unwrap();
    doc.undo().expect("undo move");
    assert_eq!(routes(&doc).len(), 1);
    doc.project().check().unwrap();
}

#[test]
fn a_track_preset_carries_no_routes_and_loading_one_retires_the_replaced_ones() {
    let Fixture {
        mut doc,
        source,
        first,
        second,
        tap,
        ..
    } = fixture();
    doc.dispatch(route(tap, 0, first), None).unwrap();
    let preset = MixerTrackPreset::capture(doc.project(), source).unwrap();
    let encoded = serde_json::to_string(&preset).unwrap();
    assert!(
        !encoded.contains("effectOutputs"),
        "routing stays with the project"
    );

    // Loading the preset onto the routed track replaces its effects with
    // fresh ones, so the old tap's route goes in the same step.
    let expected = doc.project().mixer.track(source).unwrap().clone();
    let before = doc.project().clone();
    doc.dispatch(
        Command::ApplyMixerTrackPreset {
            id: source,
            expected,
            preset: preset.clone(),
            name_color: false,
        },
        None,
    )
    .unwrap();
    assert!(routes(&doc).is_empty());
    doc.project().check().unwrap();
    doc.undo().expect("one undo step");
    assert_restored(&doc, &before);

    // Loading it onto another track leaves routes elsewhere alone.
    let expected = doc.project().mixer.track(second).unwrap().clone();
    doc.dispatch(
        Command::ApplyMixerTrackPreset {
            id: second,
            expected,
            preset,
            name_color: false,
        },
        None,
    )
    .unwrap();
    assert_eq!(routes(&doc).len(), 1);
    assert_eq!(routes(&doc)[0].effect, tap);
}

#[test]
fn routes_save_reopen_and_stay_out_of_files_that_have_none() {
    let Fixture {
        mut doc,
        first,
        second,
        tap,
        split,
        ..
    } = fixture();
    // A project without routes writes no field, so older builds and older
    // files are unaffected.
    let plain = file::to_json(doc.project()).unwrap();
    assert!(!plain.contains("effectOutputs"));
    assert!(
        file::from_json(&plain)
            .unwrap()
            .mixer
            .effect_outputs
            .is_empty()
    );

    doc.dispatch(route(tap, 0, first), None).unwrap();
    doc.dispatch(route(split, 1, second), None).unwrap();
    let saved = file::to_json(doc.project()).unwrap();
    assert!(saved.contains("effectOutputs"));
    let reopened = file::from_json(&saved).unwrap();
    assert_eq!(&reopened, doc.project());
    assert_eq!(reopened.mixer.effect_outputs.len(), 2);
}

#[test]
fn a_file_with_a_damaged_route_is_refused_on_load() {
    let Fixture {
        mut doc,
        source,
        first,
        tap,
        ..
    } = fixture();
    doc.dispatch(route(tap, 0, first), None).unwrap();
    let good = doc.project().clone();

    let damage: [(&str, Box<dyn Fn(&mut Project)>); 6] = [
        (
            "an output the effect lacks",
            Box::new(|p| p.mixer.effect_outputs[0].output = 1),
        ),
        (
            "a missing effect",
            Box::new(|p| p.mixer.effect_outputs[0].effect = EffectId(9_999)),
        ),
        (
            "a missing target",
            Box::new(|p| p.mixer.effect_outputs[0].target = TrackId(9_999)),
        ),
        (
            "its own track",
            Box::new(move |p| p.mixer.effect_outputs[0].target = source),
        ),
        (
            "a duplicated route",
            Box::new(|p| {
                let copy = p.mixer.effect_outputs[0];
                p.mixer.effect_outputs.push(copy);
            }),
        ),
        (
            "a routing loop",
            Box::new(move |p| {
                let track = p
                    .mixer
                    .tracks
                    .iter_mut()
                    .find(|track| track.id == first)
                    .unwrap();
                track.output = Some(source);
            }),
        ),
    ];
    for (what, change) in damage {
        let mut project = good.clone();
        change(&mut project);
        assert!(project.check().is_err(), "{what} must fail validation");
        // Encoding does not validate; loading the text does.
        let text = file::to_json(&project).unwrap();
        assert!(file::from_json(&text).is_err(), "{what} must not load");
    }
    good.check().unwrap();
}
