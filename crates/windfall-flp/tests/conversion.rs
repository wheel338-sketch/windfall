//! Generated fixtures are original test data dedicated to CC0-1.0.
mod common;
use common::{MODERN, project, write};
use proptest::prelude::*;
use windfall_flp::*;
use windfall_project::{ChannelSource, ClipContent, SamplePath};

fn note(position: u32, length: u32) -> Note {
    Note {
        position,
        length,
        channel: 0,
        key: 60,
        fine_pitch: 120,
        velocity: 100,
        pan: 64,
        mod_x: 128,
        mod_y: 128,
        ..Default::default()
    }
}
fn fixture(ppq: u16) -> FlpProject {
    let mut p = project(MODERN, ppq);
    p.settings.title = Some("Original fixture".into());
    p.settings.tempo_millibpm = Some(128_125);
    p.channels.push(Channel {
        iid: 0,
        name: Some("Original sample".into()),
        sample_path: Some("samples/own.wav".into()),
        volume: Some(12800),
        pan: Some(6400),
        insert: Some(1),
        ..Default::default()
    });
    p.patterns.push(Pattern {
        iid: 1,
        length: Some(u32::from(ppq) * 4),
        notes: vec![note(u32::from(ppq), u32::from(ppq))],
        ..Default::default()
    });
    p.mixer.inserts = vec![
        Insert {
            output: Some(-1),
            ..Default::default()
        },
        Insert {
            output: Some(-1),
            name: Some("Bus".into()),
            volume: Some(12800),
            routes: vec![Route {
                target: 0,
                level: None,
            }],
            ..Default::default()
        },
    ];
    p.arrangements.push(Arrangement {
        items: vec![PlaylistItem {
            position: u32::from(ppq) * 4,
            length: u32::from(ppq) * 4,
            track: 3,
            group: 0,
            flags: 0x1000,
            source: PlaylistSource::Pattern {
                pattern: 1,
                start: Some(u32::from(ppq)),
                end: None,
            },
            extra: None,
        }],
        ..Default::default()
    });
    p
}
fn converted(p: &FlpProject) -> Conversion {
    let c = import(
        &write(p),
        &ConvertOptions {
            project_dir: Some("C:/own".into()),
            ..Default::default()
        },
    )
    .unwrap();
    c.project.check().unwrap();
    assert!(
        !c.report
            .read_problems
            .iter()
            .any(|p| p.contains("empty project"))
    );
    c
}

#[test]
fn recorded_pattern_controls_import_from_real_bytes_as_held_lanes() {
    use windfall_flp::target::{ChannelParam, ControlTarget};
    let mut source = fixture(480);
    source.patterns[0].control_events = vec![
        ControlEvent { position: 0, location: ControlTarget::Channel { channel: 0, param: ChannelParam::Pan }.encode(), value: 0 },
        ControlEvent { position: 480, location: ControlTarget::Channel { channel: 0, param: ChannelParam::Pan }.encode(), value: 6400 },
        ControlEvent { position: 480, location: ControlTarget::Channel { channel: 0, param: ChannelParam::Pan }.encode(), value: 12800 },
    ];
    let output = converted(&source);
    let events = &output.project.patterns[0].events;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].points.len(), 2);
    assert_eq!(events[0].points[0].tick, 0);
    assert_eq!(events[0].points[0].value, 0.0);
    assert_eq!(events[0].points[1].tick, 960);
    assert_eq!(events[0].points[1].value, 1.0);
    assert!(events[0].points.iter().all(|point| point.hold));
    let saved = windfall_project::file::to_json(&output.project);
    assert!(saved.is_ok());
}

#[test]
fn timeline_selected_arrangement_meters_and_named_markers_survive_actual_flp_bytes() {
    let mut source = fixture(960);
    source.arrangements[0].markers = vec![
        TimeMarker {
            position: 4001,
            kind: 8,
            numerator: Some(7),
            denominator: Some(8),
            ..Default::default()
        },
        TimeMarker {
            position: 7400,
            kind: 8,
            numerator: Some(3),
            denominator: Some(4),
            ..Default::default()
        },
        TimeMarker {
            position: 7400,
            kind: 0,
            name: Some("Chorus".into()),
            ..Default::default()
        },
        TimeMarker {
            position: 7500,
            kind: 3,
            name: Some("Unsupported FL navigation".into()),
            ..Default::default()
        },
    ];
    source.patterns[0].markers.push(TimeMarker {
        position: 0,
        kind: 8,
        numerator: Some(5),
        denominator: Some(4),
        ..Default::default()
    });
    let converted = converted(&source);
    let timeline = &converted.project.playlist.timeline;
    assert_eq!(
        timeline
            .meters
            .iter()
            .map(|m| (m.tick, m.signature.numerator, m.signature.denominator))
            .collect::<Vec<_>>(),
        [(4001, 7, 8), (7400, 3, 4)]
    );
    assert_eq!(timeline.markers[0].name, "Chorus");
    assert_eq!(timeline.markers[0].tick, 7400);
    let report = format!("{:?}", converted.report);
    assert_eq!(
        converted.project.patterns[0]
            .timeline
            .meters
            .iter()
            .map(|m| (m.tick, m.signature.numerator, m.signature.denominator))
            .collect::<Vec<_>>(),
        [(0, 5, 4)]
    );
    assert!(!report.contains("per-pattern meter maps"));
    assert!(report.contains("marker kind 3"));
}
#[test]
fn generated_notes_arrangement_and_paths_survive_every_requested_time_base() {
    for ppq in [96, 192, 384, 480, 960, 7] {
        let c = converted(&fixture(ppq));
        assert_eq!(c.project.settings.tempo_bpm, 128.125);
        assert_eq!(c.project.settings.name, "Original fixture");
        let n = c.project.patterns[0].lanes[0].notes[0];
        assert_eq!((n.start, n.length), (960, 960));
        let clip = &c.project.playlist.clips[0];
        assert_eq!(
            (clip.start, clip.length, clip.offset, clip.muted),
            (3840, 3840, 960, true)
        );
        assert!(
            matches!(&c.project.samples[0].path, SamplePath::External(path) if path.ends_with("own.wav"))
        );
        assert_eq!(
            c.project.channels[0].mixer_track,
            c.project.mixer.tracks[1].id
        );
    }
}
#[test]
fn unsupported_instrument_keeps_notes_and_raw_state() {
    let mut p = fixture(96);
    p.channels[0].kind = ChannelKind::Generator;
    p.channels[0].plugin = Some(Plugin {
        internal_name: "Original fictional generator".into(),
        state: vec![1, 2, 3],
        ..Default::default()
    });
    let c = converted(&p);
    assert!(
        matches!(&c.project.channels[0].source, ChannelSource::Sampler(s) if s.sample.is_none())
    );
    assert_eq!(c.plugins[0].state, [1, 2, 3]);
    assert_eq!(c.project.patterns[0].lanes[0].notes.len(), 1);
    assert!(
        c.report
            .categories
            .iter()
            .any(|c| c.section == ReportSection::Channels && c.placeholders == 1)
    );
}
#[test]
fn sampler_controls_and_envelope_are_imported_and_losses_reported() {
    let mut p = fixture(96);
    p.channels[0].fx_flags = Some(2);
    p.channels[0].pitch = Some(150);
    p.channels[0].params = Some(ChannelParams {
        sample_start: Some(0.25),
        sample_length: Some(0.5),
        ..Default::default()
    });
    p.channels[0].cut = Some(CutGroups { cuts: 2, cut_by: 2 });
    p.channels[0].envelopes = vec![
        EnvelopeLfo::default(),
        EnvelopeLfo {
            enabled: true,
            attack: 1000,
            decay: 5000,
            sustain: 64,
            release: 6000,
            ..Default::default()
        },
    ];
    let c = converted(&p);
    let ChannelSource::Sampler(ref s) = c.project.channels[0].source else {
        panic!("sampler")
    };
    assert_eq!(
        (s.start, s.end, s.reverse, s.tune, s.cut_group, s.cut_self),
        (0.25, 0.75, true, 1.5, 2, true)
    );
    assert_eq!(s.envelope.unwrap().sustain, 0.5);
    assert!(!c.report.is_clean());
}
#[test]
fn cyclic_routes_are_refused_without_invalidating_the_project() {
    let mut p = fixture(96);
    p.mixer.inserts.push(Insert {
        output: Some(-1),
        name: Some("Second bus".into()),
        routes: vec![Route {
            target: 1,
            level: Some(12800),
        }],
        ..Default::default()
    });
    p.mixer.inserts[1].routes = vec![Route {
        target: 2,
        level: Some(12800),
    }];
    let c = converted(&p);
    assert!(
        c.report
            .categories
            .iter()
            .flat_map(|c| &c.lines)
            .any(|l| l.text.contains("feed a mixer track back into itself"))
    );
}
#[test]
fn layers_copy_notes_to_children_and_cycles_terminate() {
    let mut p = fixture(96);
    p.channels.push(Channel {
        iid: 1,
        kind: ChannelKind::Layer,
        layer_children: vec![0, 2],
        ..Default::default()
    });
    p.channels.push(Channel {
        iid: 2,
        kind: ChannelKind::Layer,
        layer_children: vec![1],
        ..Default::default()
    });
    p.patterns[0].notes[0].channel = 1;
    let c = converted(&p);
    assert_eq!(c.project.channels.len(), 1);
    assert_eq!(c.project.patterns[0].lanes[0].notes.len(), 1);
}
#[test]
fn audio_and_automation_clips_are_placed_with_target_and_hold() {
    let mut p = fixture(96);
    p.channels.push(Channel {
        iid: 1,
        kind: ChannelKind::AudioClip,
        sample_path: Some("own.wav".into()),
        ..Default::default()
    });
    p.channels.push(Channel {
        iid: 2,
        kind: ChannelKind::AutomationClip,
        automation: Some(AutomationCurve {
            points: vec![
                AutomationPoint {
                    offset: 0.0,
                    value: 0.25,
                    ..Default::default()
                },
                AutomationPoint {
                    offset: 1.0,
                    value: 0.75,
                    mode: 2,
                    ..Default::default()
                },
            ],
        }),
        ..Default::default()
    });
    p.remote_controllers.push(RemoteController {
        source: 2,
        location: ControlTarget::Channel {
            channel: 0,
            param: ChannelParam::Pan,
        }
        .encode(),
        flags: 0,
        smoothing: 0,
    });
    for channel in [1, 2] {
        p.arrangements[0].items.push(PlaylistItem {
            position: 0,
            length: 96,
            track: 1,
            group: 0,
            flags: 0,
            source: PlaylistSource::Channel {
                channel,
                start: Some(0.0),
                end: None,
            },
            extra: None,
        });
    }
    let c = converted(&p);
    assert_eq!(c.project.automations.len(), 1);
    assert_eq!(c.project.automations[0].points[1].tick, 960);
    assert!(c.project.automations[0].points[0].hold);
    assert!(
        c.project
            .playlist
            .clips
            .iter()
            .any(|c| matches!(c.content, ClipContent::Audio { .. }))
    );
    assert!(
        c.project
            .playlist
            .clips
            .iter()
            .any(|c| matches!(c.content, ClipContent::Automation { .. }))
    );
}
#[test]
fn effect_defaults_and_unsupported_states_are_reported() {
    let mut p = fixture(96);
    p.mixer.inserts[1].slots = vec![
        Slot {
            index: 0,
            plugin: Plugin {
                internal_name: "Fruity Limiter".into(),
                ..Default::default()
            },
            enabled: Some(false),
            mix: Some(6400),
            ..Default::default()
        },
        Slot {
            index: 1,
            plugin: Plugin {
                internal_name: "Original fictional effect".into(),
                state: vec![5, 6],
                ..Default::default()
            },
            ..Default::default()
        },
    ];
    let c = converted(&p);
    assert_eq!(c.project.mixer.tracks[1].effects.len(), 1);
    assert!(!c.project.mixer.tracks[1].effects[0].enabled);
    assert_eq!(c.project.mixer.tracks[1].effects[0].mix, 0.5);
    assert_eq!(c.plugins[0].state, [5, 6]);
    assert!(!c.report.is_clean());
}
#[test]
fn direct_hostile_model_always_gives_a_checked_project() {
    let mut p = fixture(0);
    p.channels[0].params = Some(ChannelParams {
        sample_start: Some(f64::NAN),
        sample_length: Some(f64::NEG_INFINITY),
        ..Default::default()
    });
    p.patterns[0].notes.extend([
        note(u32::MAX, u32::MAX),
        Note {
            key: u16::MAX,
            ..note(0, 0)
        },
    ]);
    let c = convert(&p, &ConvertOptions::default());
    c.project.check().unwrap();
    assert!(!c.report.is_clean());
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn generated_projects_round_trip_and_convert(ppq in 1u16..=960, positions in prop::collection::vec((0u32..1000, 1u32..1000), 0..40)) {
        let mut p = fixture(ppq);
        p.patterns[0].notes = positions.iter().map(|&(start,len)| note(start,len)).collect();
        let parsed = parse(&write(&p)).unwrap();
        prop_assert_eq!(&parsed.patterns[0].notes, &p.patterns[0].notes);
        let a = convert(&parsed, &ConvertOptions::default());
        let b = convert(&parsed, &ConvertOptions::default());
        prop_assert!(a.project.check().is_ok());
        prop_assert!(!a.report.read_problems.iter().any(|p| p.contains("empty project")));
        prop_assert_eq!(a.project, b.project);
    }
    #[test]
    fn hostile_event_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let header = Header { format: FileFormat::Project, channel_count: 1, ppq: 96 };
        let bytes = common::file(header, &bytes);
        let result = std::panic::catch_unwind(|| {
            if let Ok(p) = parse(&bytes) { let c = convert(&p, &ConvertOptions::default()); assert!(c.project.check().is_ok()); }
        });
        prop_assert!(result.is_ok());
    }
}

#[test]
fn clip_channels_keep_their_pattern_notes_on_silent_placeholders() {
    let mut p = fixture(96);
    p.channels[0].kind = ChannelKind::AudioClip;
    let c = converted(&p);
    assert_eq!(c.project.patterns[0].lanes[0].notes.len(), 1);
    assert!(
        matches!(&c.project.channels[0].source, ChannelSource::Sampler(s) if s.sample.is_none())
    );
    assert_eq!(c.report.category(ReportSection::Notes).dropped, 0);
}
#[test]
fn a_native_generator_uses_our_instrument_and_reports_sound_approximation() {
    let mut p = fixture(96);
    p.channels[0].kind = ChannelKind::Generator;
    p.channels[0].plugin = Some(Plugin {
        internal_name: "3x Osc".into(),
        state: vec![0; 92],
        ..Default::default()
    });
    let c = converted(&p);
    assert!(matches!(
        c.project.channels[0].source,
        ChannelSource::Instrument { .. }
    ));
    assert_eq!(c.report.category(ReportSection::Channels).approximated, 1);
}
#[test]
fn source_unknowns_and_metadata_are_losses_while_named_markers_survive() {
    let mut p = fixture(96);
    p.settings.author = Some("Original author".into());
    p.patterns[0].markers.push(TimeMarker {
        name: Some("Original marker".into()),
        ..Default::default()
    });
    p.uninterpreted.push(RawEvent {
        id: 63,
        value: RawValue::Byte(2),
    });
    let c = converted(&p);
    assert!(c.report.unknown_event_ids.contains(&63));
    assert_eq!(c.report.category(ReportSection::Other).dropped, 1);
    let marker = &c.project.patterns[0].timeline.markers[0];
    assert_eq!((marker.tick, marker.name.as_str()), (0, "Original marker"));
    assert_eq!(c.project.settings.author, "Original author");
}
#[test]
fn master_volume_is_converted_even_without_insert_events() {
    let mut p = fixture(96);
    p.mixer.inserts.clear();
    p.settings.main_volume = Some(0);
    let c = converted(&p);
    assert_eq!(c.project.mixer.tracks[0].volume, 0.0);
}
#[test]
fn mapped_effect_parameters_can_receive_automation() {
    let mut p = fixture(96);
    p.mixer.inserts[1].slots.push(Slot {
        index: 0,
        plugin: Plugin {
            internal_name: "Fruity Compressor".into(),
            state: vec![0; 28],
            ..Default::default()
        },
        ..Default::default()
    });
    p.channels.push(Channel {
        iid: 1,
        kind: ChannelKind::AutomationClip,
        automation: Some(AutomationCurve {
            points: vec![AutomationPoint {
                value: 0.5,
                ..Default::default()
            }],
        }),
        ..Default::default()
    });
    p.remote_controllers.push(RemoteController {
        source: 1,
        location: ControlTarget::SlotPlugin {
            insert: 1,
            slot: 0,
            param: 0,
        }
        .encode(),
        flags: 0,
        smoothing: 0,
    });
    p.arrangements[0].items.push(PlaylistItem {
        position: 0,
        length: 96,
        track: 0,
        group: 0,
        flags: 0,
        source: PlaylistSource::Channel {
            channel: 1,
            start: None,
            end: None,
        },
        extra: None,
    });
    let c = converted(&p);
    assert_eq!(c.project.automations.len(), 1);
    assert!(matches!(
        c.project.automations[0].target,
        windfall_project::AutomationTarget::EffectParam { .. }
    ));
    assert_eq!(c.report.category(ReportSection::Automation).approximated, 1);
}

#[test]
fn the_panning_law_a_file_names_becomes_the_projects_law() {
    use windfall_project::PanLaw;
    let mut plain = fixture(96);
    plain.settings.pan_law = None;
    let without = converted(&plain);
    assert_eq!(without.project.settings.pan_law, PanLaw::Balance);
    for (law, expected) in [
        // Circular: the power of the two sides stays constant.
        (0, PanLaw::ConstantPower),
        // Triangular is the balance law.
        (2, PanLaw::Balance),
        // A value no source documents leaves the project's default alone.
        (7, PanLaw::Balance),
    ] {
        let mut source = fixture(96);
        source.settings.pan_law = Some(law);
        let with = converted(&source);
        assert_eq!(with.project.settings.pan_law, expected, "law {law}");
        // The law is carried over, not counted as a difference.
        let (counted, plain) = (
            with.report.category(ReportSection::Project),
            without.report.category(ReportSection::Project),
        );
        assert_eq!(
            (counted.exact, counted.approximated, counted.dropped),
            (plain.exact, plain.approximated, plain.dropped),
            "law {law}"
        );
        // Nothing else about the project depends on the law.
        let mut same = with.project.clone();
        same.settings.pan_law = PanLaw::Balance;
        assert_eq!(same, without.project, "law {law}");
        // The file's own resolution is converted, not adopted.
        assert_eq!(with.project.settings.timebase_ppq, 960);
    }
}

#[test]
fn channel_swing_docks_sidechains_send_destinations_and_opaque_state_survive() {
    let mut p = fixture(96);
    p.channels[0].swing = Some(32);
    p.channels[0].time_shift = Some(64);
    p.mixer.inserts.push(Insert {
        name: Some("Detector".into()),
        flags: Some(0x8c),
        ..Default::default()
    });
    p.mixer.inserts[1].flags = Some(0x4c);
    p.mixer.inserts[1].routes.push(Route {
        target: 2,
        level: Some(0),
    });
    let state = common::numbers(&[0, 256, 256, 2]);
    p.mixer.inserts[1].slots.push(Slot {
        plugin: Plugin {
            internal_name: "Fruity Send".into(),
            state: state.clone(),
            ..Default::default()
        },
        ..Default::default()
    });
    let c = converted(&p);
    assert_eq!(c.project.channels[0].timing.swing_mix, 0.25);
    assert_eq!(c.project.settings.timebase_ppq, 96);
    let bus = &c.project.mixer.tracks[1];
    let detector = &c.project.mixer.tracks[2];
    assert_eq!(bus.dock, windfall_project::MixerDock::Middle);
    assert_eq!(detector.dock, windfall_project::MixerDock::Right);
    assert_eq!(bus.sidechains[0].target, detector.id);
    assert_eq!(c.project.mixer.effect_outputs[0].target, detector.id);
    assert!(c.project.retained_plugins.iter().any(|p| p.state == state));
    assert!(format!("{:?}", c.report).contains("Nonlinear channel time shift"));
    let json = windfall_project::file::to_json(&c.project).unwrap();
    assert_eq!(windfall_project::file::from_json(&json).unwrap(), c.project);
}
#[test]
fn unknown_version_bad_event_length_and_unknown_marker_counts_remain_honest() {
    let mut p = fixture(96);
    p.version_text = Some("999.7.1".into());
    p.arrangements[0].markers.push(TimeMarker {
        kind: 127,
        position: 96,
        ..Default::default()
    });
    let c = converted(&p);
    assert!(c.report.category(ReportSection::Other).dropped >= 1);
    assert!(!c.project.patterns[0].lanes[0].notes.is_empty());
    let mut events = common::Events::new(true);
    events.bytes.extend([213, 255, 255, 255, 255, 15]);
    let bytes = common::file(p.header.unwrap(), &events.bytes);
    assert!(parse(&bytes).is_err());
}

#[test]
fn effects_beyond_chain_capacity_keep_their_opaque_state_and_count_the_loss() {
    let mut source = fixture(96);
    source.mixer.inserts[1].slots = (0..=windfall_project::MAX_EFFECT_SLOTS)
        .map(|index| Slot {
            index: index as u8,
            plugin: Plugin {
                internal_name: "Fruity Balance".into(),
                state: vec![index as u8, 90],
                ..Default::default()
            },
            ..Default::default()
        })
        .collect();
    // Direct hostile model: a public parser need not admit out-of-format slot counts.
    let imported = convert(&source, &ConvertOptions::default());
    imported.project.check().unwrap();
    assert_eq!(
        imported.project.mixer.tracks[1].effects.len(),
        windfall_project::MAX_EFFECT_SLOTS
    );
    assert_eq!(
        imported.plugins.len(),
        windfall_project::MAX_EFFECT_SLOTS + 1
    );
    assert!(imported.report.category(ReportSection::Effects).dropped >= 1);
    let last = windfall_project::MAX_EFFECT_SLOTS as u8;
    assert!(
        imported
            .project
            .retained_plugins
            .iter()
            .any(|entry| entry.state == [last, 90])
    );
}
