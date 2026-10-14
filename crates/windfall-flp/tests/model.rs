//! From bytes to the typed model: one test for each kind of event, on
//! files the fixture writer makes.

mod common;

use common::{Events, MODERN, file, project, write};
use windfall_flp::{
    Arrangement, AutomationCurve, AutomationPoint, Channel, ChannelKind, ChannelParam,
    ChannelParams, ControlEvent, ControlTarget, ControlValue, CutGroups, EnvelopeLfo, FileFormat,
    FlVersion, FlpProject, Header, Insert, InsertEqBand, InsertParam, LegacyPlaylistItem,
    LegacyStep, MainParam, Note, Pattern, PlaylistItem, PlaylistItemExtra, PlaylistSource, Plugin,
    Polyphony, RawEvent, RawValue, RemoteController, Route, Slot, TimeMarker, Track, parse,
};

const HEADER: Header = Header {
    format: FileFormat::Project,
    channel_count: 1,
    ppq: 96,
};

/// Parses a stream of events written the modern way.
fn read(build: impl FnOnce(&mut Events)) -> FlpProject {
    let mut events = Events::new(true);
    events.data(199, b"20.8.4.2576\0");
    build(&mut events);
    parse(&file(HEADER, &events.bytes)).expect("a readable file")
}

/// Writes a project and reads it back.
fn round_trip(project: &FlpProject) -> FlpProject {
    parse(&write(project)).expect("a readable file")
}

fn note(position: u32, channel: u16, key: u16, length: u32) -> Note {
    Note {
        position,
        channel,
        key,
        length,
        flags: 0x4000,
        fine_pitch: 120,
        release: 64,
        pan: 64,
        velocity: 100,
        mod_x: 128,
        mod_y: 128,
        ..Note::default()
    }
}

fn insert(name: &str) -> Insert {
    Insert {
        name: Some(name.to_owned()),
        flags: Some(0x0C),
        output: Some(-1),
        ..Insert::default()
    }
}

#[test]
fn the_version_is_surfaced_as_text_and_as_numbers() {
    let project = read(|_| {});
    assert_eq!(project.version_text.as_deref(), Some("20.8.4.2576"));
    assert_eq!(
        project.version(),
        Some(FlVersion {
            major: 20,
            minor: 8,
            patch: 4,
            build: 2576
        })
    );
    assert_eq!(project.header, Some(HEADER));
    assert_eq!(project.ppq(), 96);
}

#[test]
fn project_settings_come_from_their_events() {
    let project = read(|events| {
        events.dword(156, 69_420);
        events.byte(17, 3);
        events.byte(18, 8);
        events.byte(11, 64);
        events.word(80, (-1200_i16) as u16);
        events.byte(9, 1);
        events.byte(23, 2);
        events.byte(30, 0);
        events.word(67, 5);
        events.text(194, "Night Drive");
        events.text(207, "Ada");
        events.text(206, "House");
        events.text(195, "First line\rSecond line");
        events.text(197, "https://example.org");
        events.text(202, "");
    });
    let settings = &project.settings;
    assert_eq!(settings.tempo_millibpm, Some(69_420));
    assert_eq!(settings.tempo_bpm(), Some(69.42));
    assert_eq!(
        (settings.numerator, settings.denominator),
        (Some(3), Some(8))
    );
    assert_eq!(settings.swing, Some(64));
    assert_eq!(settings.main_pitch, Some(-1200));
    assert_eq!(settings.loop_active, Some(true));
    assert_eq!(settings.pan_law, Some(2));
    assert_eq!(settings.play_truncated_notes, Some(false));
    assert_eq!(settings.current_pattern, Some(5));
    assert_eq!(settings.title.as_deref(), Some("Night Drive"));
    assert_eq!(settings.author.as_deref(), Some("Ada"));
    assert_eq!(settings.genre.as_deref(), Some("House"));
    assert_eq!(
        settings.comments.as_deref(),
        Some("First line\rSecond line")
    );
    assert!(!settings.comments_are_rtf);
    assert_eq!(settings.url.as_deref(), Some("https://example.org"));
    assert_eq!(settings.data_path.as_deref(), Some(""));
}

#[test]
fn an_old_file_gives_its_tempo_in_two_events_and_its_text_in_bytes() {
    let mut events = Events::new(false);
    events.data(199, b"9.0.3\0");
    events.word(93, 500);
    events.word(66, 140);
    events.text(194, "Old Song");
    events.text(198, "{\\rtf1 notes}");
    events.byte(12, 100);
    let project = parse(&file(HEADER, &events.bytes)).expect("a readable file");
    assert_eq!(project.settings.tempo_millibpm, Some(140_500));
    assert_eq!(project.settings.title.as_deref(), Some("Old Song"));
    assert_eq!(project.settings.comments.as_deref(), Some("{\\rtf1 notes}"));
    assert!(project.settings.comments_are_rtf);
    assert_eq!(project.settings.main_volume, Some(100));
}

#[test]
fn the_single_tempo_event_wins_over_the_old_pair() {
    let project = read(|events| {
        events.word(66, 140);
        events.dword(156, 128_000);
    });
    assert_eq!(project.settings.tempo_millibpm, Some(128_000));
}

#[test]
fn a_sampler_channel_reads_its_name_colour_sample_and_levels() {
    let mut expected = project(MODERN, 96);
    expected.channels.push(Channel {
        iid: 3,
        kind: ChannelKind::Sampler,
        name: Some("Kick".to_owned()),
        color: Some(0x10_20_FF),
        enabled: Some(false),
        sample_path: Some(
            "%FLStudioFactoryData%\\Data\\Patches\\Packs\\Drums\\Kick.wav".to_owned(),
        ),
        volume: Some(10_000),
        pan: Some(3_200),
        pitch: Some(-1_200),
        insert: Some(4),
        cut: Some(CutGroups { cuts: 2, cut_by: 2 }),
        root_note: Some(57),
        fx_flags: Some(0x2),
        sampler_flags: Some(0xA),
        preamp: Some(128),
        filter_mod_x: Some(0),
        filter_mod_y: Some(0),
        filter_type: Some(0),
        params: Some(ChannelParams {
            main_pitch: Some(false),
            add_root: Some(false),
            key_region: Some((0, 0)),
            remove_dc: Some(false),
            normalize: Some(false),
            inverted: Some(false),
            declick_mode: Some(0),
            crossfade: Some(0),
            trim: Some(0),
            midi_channel_through: Some(0),
            sample_offset: Some(0.0),
            stretch_formant: Some(-12.0),
            fix_trim: Some(false),
            drum_stretch: [Some(0.0); 5],
            stretch_time: Some(0),
            stretch_pitch: Some(0),
            stretch_multiplier: Some(0),
            stretch_mode: Some(0),
            sample_start: Some(0.25),
            sample_length: Some(0.5),
            arp_direction: Some(0),
            arp_range: Some(0),
            arp_chord: Some(0),
            arp_time: Some(0),
            arp_gate: Some(0),
            arp_slide: Some(false),
            arp_repeat: Some(0),
            full_porta: Some(false),
            gate: Some(0),
            delay_flags: Some(0),
        }),
        envelopes: vec![
            EnvelopeLfo::default(),
            EnvelopeLfo {
                flags: 4,
                enabled: true,
                predelay: 100,
                attack: 20_000,
                hold: 20_000,
                decay: 30_000,
                sustain: 50,
                release: 20_000,
                amount: 0,
                ..Default::default()
            },
        ],
        polyphony: Some(Polyphony {
            max: 4,
            slide: 500,
            flags: 1,
        }),
        group: Some(1),
        ..Channel::default()
    });
    let read = round_trip(&expected);
    assert_eq!(read.channels, expected.channels);
    let channel = &read.channels[0];
    assert!(channel.reversed());
    assert_eq!(channel.display_name(), Some("Kick"));
    assert_eq!(
        channel.volume_envelope().map(|envelope| envelope.sustain),
        Some(50)
    );
}

#[test]
fn a_generator_channel_reads_its_plugin_and_state() {
    let mut expected = project(MODERN, 96);
    expected.channels.push(Channel {
        iid: 0,
        kind: ChannelKind::Generator,
        plugin: Some(Plugin {
            internal_name: "Example Synth".to_owned(),
            generator: Some(true),
            state: vec![1, 2, 3, 4, 5, 6, 7, 8],
        }),
        ..Channel::default()
    });
    let read = round_trip(&expected);
    assert_eq!(read.channels, expected.channels);
    assert_eq!(read.channels[0].display_name(), Some("Example Synth"));
}

#[test]
fn channel_kinds_read_by_their_numbers() {
    let project = read(|events| {
        for (iid, kind) in [(0, 0), (1, 2), (2, 3), (3, 4), (4, 5), (5, 1)] {
            events.word(64, iid);
            events.byte(21, kind);
        }
    });
    let kinds: Vec<ChannelKind> = project
        .channels
        .iter()
        .map(|channel| channel.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            ChannelKind::Sampler,
            ChannelKind::Generator,
            ChannelKind::Layer,
            ChannelKind::AudioClip,
            ChannelKind::AutomationClip,
            ChannelKind::Other(1),
        ]
    );
}

#[test]
fn a_layer_lists_its_children() {
    let project = read(|events| {
        events.word(64, 2);
        events.byte(21, 3);
        events.word(94, 0);
        events.word(94, 1);
    });
    assert_eq!(project.channels[0].layer_children, [0, 1]);
}

#[test]
fn the_older_channel_name_event_is_read_and_the_newer_one_wins() {
    let project = read(|events| {
        events.word(64, 0);
        events.text(192, "Old name");
        events.word(64, 1);
        events.text(192, "Old name");
        events.text(203, "New name");
        events.word(64, 2);
        events.text(203, "New name");
        events.text(192, "Old name");
    });
    let names: Vec<_> = project
        .channels
        .iter()
        .map(|channel| channel.name.as_deref())
        .collect();
    assert_eq!(
        names,
        [Some("Old name"), Some("New name"), Some("New name")]
    );
}

#[test]
fn an_automation_clip_reads_its_points() {
    let curve = AutomationCurve {
        points: vec![
            AutomationPoint {
                offset: 0.0,
                value: 1.0,
                tension: 0.0,
                mode: 0,
            },
            AutomationPoint {
                offset: 8.0,
                value: 0.25,
                tension: -0.5,
                mode: 2,
            },
            AutomationPoint {
                offset: 0.0,
                value: 0.75,
                tension: 0.5,
                mode: 0,
            },
        ],
    };
    let mut expected = project(MODERN, 96);
    expected.channels.push(Channel {
        iid: 5,
        kind: ChannelKind::AutomationClip,
        name: Some("Volume ride".to_owned()),
        automation: Some(curve.clone()),
        ..Channel::default()
    });
    assert_eq!(round_trip(&expected).channels[0].automation, Some(curve));
}

#[test]
fn a_curve_that_claims_more_points_than_it_has_gives_the_ones_it_has() {
    let project = read(|events| {
        events.word(64, 0);
        let mut data = vec![0_u8; 21];
        data[17..21].copy_from_slice(&u32::MAX.to_le_bytes());
        data.extend(4.0_f64.to_le_bytes());
        data.extend(0.5_f64.to_le_bytes());
        data.extend([0_u8; 8]);
        data.extend([1, 2, 3]);
        events.data(234, &data);
    });
    let curve = project.channels[0].automation.as_ref().expect("a curve");
    assert_eq!(curve.points.len(), 1);
    assert_eq!((curve.points[0].offset, curve.points[0].value), (4.0, 0.5));
}

#[test]
fn a_pattern_reads_its_name_colour_length_and_notes() {
    let mut expected = project(MODERN, 96);
    expected.patterns.push(Pattern {
        iid: 2,
        name: Some("Verse".to_owned()),
        color: Some(0xFF_80_00),
        length: Some(768),
        notes: vec![
            note(0, 0, 60, 0),
            Note {
                flags: 0x4008,
                group: 3,
                fine_pitch: 240,
                release: 0,
                midi_channel: 9,
                pan: 128,
                velocity: 128,
                mod_x: 0,
                mod_y: 255,
                ..note(96, 1, 131, 4_000_000)
            },
        ],
        ..Pattern::default()
    });
    let read = round_trip(&expected);
    assert_eq!(read.patterns, expected.patterns);
    assert!(read.patterns[0].notes[0].is_step());
    assert!(read.patterns[0].notes[1].is_slide());
}

#[test]
fn patterns_come_out_in_the_order_of_their_numbers() {
    let project = read(|events| {
        for iid in [7, 2, 5] {
            events.word(65, iid);
            events.text(193, &format!("Pattern {iid}"));
        }
        events.word(65, 2);
        events.dword(150, 0x00_00_00_FF);
    });
    let iids: Vec<u16> = project.patterns.iter().map(|pattern| pattern.iid).collect();
    assert_eq!(iids, [2, 5, 7]);
    assert_eq!(project.patterns[0].color, Some(0xFF_00_00));
    assert_eq!(
        project.pattern(5).and_then(|p| p.name.as_deref()),
        Some("Pattern 5")
    );
}

#[test]
fn notes_of_files_older_than_9_are_20_bytes_each() {
    let mut expected = project("8.0.2", 96);
    expected.patterns.push(Pattern {
        iid: 1,
        notes: vec![
            Note {
                group: 0,
                ..note(0, 2, 60, 24)
            },
            Note {
                group: 0,
                velocity: 90,
                pan: 20,
                ..note(48, 2, 67, 24)
            },
        ],
        ..Pattern::default()
    });
    let bytes = write(&expected);
    let note_bytes = windfall_flp::open(&bytes)
        .expect("a readable file")
        .events
        .filter_map(Result::ok)
        .find(|event| event.id == 224)
        .and_then(|event| event.value.data().map(<[u8]>::len));
    assert_eq!(note_bytes, Some(40));
    assert_eq!(round_trip(&expected).patterns, expected.patterns);
}

#[test]
fn notes_that_end_mid_note_keep_the_whole_ones_and_say_so() {
    let project = read(|events| {
        events.word(65, 1);
        let mut data = vec![0_u8; 24 * 2 + 5];
        data[12] = 60;
        data[24 + 12] = 62;
        events.data(224, &data);
    });
    assert_eq!(project.patterns[0].notes.len(), 2);
    assert_eq!(project.diagnostics.len(), 1);
    assert!(
        windfall_core::english::english(&project.diagnostics[0].message)
            .contains("middle of a note")
    );
    assert!(project.diagnostics[0].offset.is_some());
}

#[test]
fn notes_without_a_pattern_are_left_out_with_a_diagnostic() {
    let project = read(|events| events.data(224, &[0_u8; 24]));
    assert!(project.patterns.is_empty());
    assert!(
        windfall_core::english::english(&project.diagnostics[0].message)
            .contains("before it names a pattern")
    );
}

#[test]
fn steps_of_the_oldest_files_are_kept_with_their_channel() {
    let mut expected = project("9.0.3", 96);
    expected.channels.push(Channel {
        iid: 4,
        ..Channel::default()
    });
    expected.patterns.push(Pattern {
        iid: 1,
        legacy_steps: vec![
            LegacyStep { channel: 4, raw: 0 },
            LegacyStep {
                channel: 4,
                raw: 0x0104,
            },
        ],
        ..Pattern::default()
    });
    let read = round_trip(&expected);
    assert_eq!(read.patterns, expected.patterns);
    assert_eq!(read.patterns[0].legacy_steps[1].step(), 4);
}

#[test]
fn recorded_control_changes_are_kept_and_name_what_they_change() {
    let location = ControlTarget::Channel {
        channel: 2,
        param: ChannelParam::Pan,
    }
    .encode();
    let mut expected = project(MODERN, 96);
    expected.patterns.push(Pattern {
        iid: 1,
        control_events: vec![
            ControlEvent {
                position: 0,
                location,
                value: 6_400,
            },
            ControlEvent {
                position: 96,
                location,
                value: -5,
            },
        ],
        ..Pattern::default()
    });
    let read = round_trip(&expected);
    assert_eq!(read.patterns, expected.patterns);
    assert_eq!(
        ControlTarget::decode(read.patterns[0].control_events[0].location),
        ControlTarget::Channel {
            channel: 2,
            param: ChannelParam::Pan
        }
    );
}

#[test]
fn markers_belong_to_the_pattern_or_arrangement_they_follow() {
    let mut expected = project(MODERN, 96);
    expected.patterns.push(Pattern {
        iid: 4,
        markers: vec![TimeMarker {
            position: 0,
            kind: 8,
            name: Some("3/2".to_owned()),
            numerator: Some(3),
            denominator: Some(2),
        }],
        ..Pattern::default()
    });
    expected.arrangements.push(Arrangement {
        index: 0,
        name: Some("Song".to_owned()),
        markers: vec![
            TimeMarker {
                position: 384,
                kind: 0,
                name: Some("Drop".to_owned()),
                numerator: Some(4),
                denominator: Some(4),
            },
            TimeMarker {
                position: 0x00FF_FFFF,
                kind: 5,
                name: None,
                numerator: None,
                denominator: None,
            },
        ],
        ..Arrangement::default()
    });
    let read = round_trip(&expected);
    assert_eq!(read.patterns, expected.patterns);
    assert_eq!(read.arrangements, expected.arrangements);
    assert!(read.patterns[0].markers[0].is_time_signature());
}

#[test]
fn playlist_items_read_position_length_track_and_what_they_play() {
    let mut expected = project(MODERN, 96);
    expected.arrangements.push(Arrangement {
        index: 0,
        items: vec![
            PlaylistItem {
                position: 0,
                length: 384,
                track: 9,
                group: 0,
                flags: 0x40,
                source: PlaylistSource::Pattern {
                    pattern: 3,
                    start: None,
                    end: None,
                },
                extra: None,
            },
            PlaylistItem {
                position: 384,
                length: 192,
                track: 499,
                group: 2,
                flags: 0x1040,
                source: PlaylistSource::Pattern {
                    pattern: 3,
                    start: Some(96),
                    end: Some(288),
                },
                extra: None,
            },
            PlaylistItem {
                position: 768,
                length: 1536,
                track: 0,
                group: 0,
                flags: 0x40,
                source: PlaylistSource::Channel {
                    channel: 5,
                    start: Some(1.5),
                    end: None,
                },
                extra: None,
            },
        ],
        ..Arrangement::default()
    });
    let read = round_trip(&expected);
    assert_eq!(read.arrangements, expected.arrangements);
    let items = &read.arrangements[0].items;
    assert!(!items[0].muted());
    assert!(items[1].muted());
}

#[test]
fn tracks_are_counted_from_198_before_version_12_9_1() {
    for (version, top) in [("11.1.0", 198_u16), ("12.9.0", 198), ("12.9.1", 499)] {
        let mut events = Events::new(version != "11.1.0");
        events.data(199, format!("{version}\0").as_bytes());
        let mut item = vec![0_u8; 32];
        item[4..6].copy_from_slice(&0x5000_u16.to_le_bytes());
        item[6..8].copy_from_slice(&0x5001_u16.to_le_bytes());
        item[8..12].copy_from_slice(&96_u32.to_le_bytes());
        item[12..14].copy_from_slice(&(top - 2).to_le_bytes());
        item[24..32].copy_from_slice(&[0xFF; 8]);
        events.data(233, &item);
        let project = parse(&file(HEADER, &events.bytes)).expect("a readable file");
        assert_eq!(project.arrangements[0].items[0].track, 2, "{version}");
    }
}

#[test]
fn items_of_fl_studio_21_carry_fades_and_a_gain() {
    let extra = PlaylistItemExtra {
        id: 7,
        fade_in: 12.5,
        fade_in_tension: 0.0,
        fade_out: 250.0,
        fade_out_tension: 0.5,
        gain: 0.8,
        fade_flags: 3,
    };
    let mut expected = project("21.0.3.3517", 96);
    expected.arrangements.push(Arrangement {
        index: 0,
        items: vec![PlaylistItem {
            position: 96,
            length: 960,
            track: 1,
            group: 0,
            flags: 0x40,
            source: PlaylistSource::Channel {
                channel: 2,
                start: None,
                end: None,
            },
            extra: Some(extra),
        }],
        ..Arrangement::default()
    });
    assert_eq!(round_trip(&expected).arrangements, expected.arrangements);
}

#[test]
fn fifteen_old_items_are_not_taken_for_eight_new_ones() {
    // 480 bytes divide by both item sizes, so only the version can tell.
    let mut expected = project(MODERN, 96);
    expected.arrangements.push(Arrangement {
        index: 0,
        items: (0..15)
            .map(|index| PlaylistItem {
                position: index * 96,
                length: 96,
                track: 0,
                group: 0,
                flags: 0x40,
                source: PlaylistSource::Pattern {
                    pattern: 1,
                    start: None,
                    end: None,
                },
                extra: None,
            })
            .collect(),
        ..Arrangement::default()
    });
    assert_eq!(round_trip(&expected).arrangements[0].items.len(), 15);
}

#[test]
fn pattern_blocks_of_old_playlists_get_a_row_for_each_pattern() {
    let mut expected = project("9.0.3", 96);
    expected.arrangements.push(Arrangement {
        index: 0,
        items: vec![
            PlaylistItem {
                position: 0,
                length: 384,
                // Row 990 of the file: pattern 9, below the 199 clip tracks.
                track: 198 + 9,
                group: 0,
                flags: 0x40,
                source: PlaylistSource::Pattern {
                    pattern: 9,
                    start: None,
                    end: None,
                },
                extra: None,
            },
            PlaylistItem {
                position: 192,
                length: 111,
                track: 101,
                group: 0,
                flags: 0x40,
                source: PlaylistSource::Channel {
                    channel: 4,
                    start: None,
                    end: None,
                },
                extra: None,
            },
        ],
        ..Arrangement::default()
    });
    assert_eq!(round_trip(&expected).arrangements, expected.arrangements);
}

#[test]
fn an_item_on_a_track_that_does_not_exist_is_left_out_with_a_diagnostic() {
    let project = read(|events| {
        let mut item = vec![0_u8; 32];
        item[6] = 1;
        item[12..14].copy_from_slice(&700_u16.to_le_bytes());
        events.data(233, &item);
    });
    assert!(project.arrangements[0].items.is_empty());
    assert!(
        windfall_core::english::english(&project.diagnostics[0].message)
            .contains("1 clip of the playlist sits")
    );
}

#[test]
fn the_playlist_of_the_oldest_files_is_one_event_for_each_bar() {
    let mut expected = project("3.5.6", 96);
    expected.arrangements.push(Arrangement {
        index: 0,
        legacy_items: vec![
            LegacyPlaylistItem { bar: 0, pattern: 1 },
            LegacyPlaylistItem { bar: 7, pattern: 3 },
        ],
        ..Arrangement::default()
    });
    assert_eq!(round_trip(&expected).arrangements, expected.arrangements);
}

#[test]
fn playlist_tracks_read_their_name_colour_state_and_height() {
    let mut expected = project(MODERN, 96);
    expected.arrangements.push(Arrangement {
        index: 0,
        tracks: vec![
            Track {
                iid: 1,
                name: None,
                color: Some(0x48_51_56),
                enabled: true,
                height: 1.0,
            },
            Track {
                iid: 2,
                name: Some("Drums".to_owned()),
                color: Some(0xFF_00_00),
                enabled: false,
                height: 0.5,
            },
        ],
        ..Arrangement::default()
    });
    assert_eq!(round_trip(&expected).arrangements, expected.arrangements);
}

#[test]
fn several_arrangements_are_kept_apart_and_the_current_one_is_named() {
    let mut expected = project(MODERN, 96);
    for (index, name) in [(0, "Intro idea"), (1, "Full song")] {
        expected.arrangements.push(Arrangement {
            index,
            name: Some(name.to_owned()),
            ..Arrangement::default()
        });
    }
    expected.current_arrangement = Some(1);
    let read = round_trip(&expected);
    assert_eq!(read.arrangements, expected.arrangements);
    assert_eq!(
        read.main_arrangement().and_then(|a| a.name.as_deref()),
        Some("Full song")
    );
}

#[test]
fn mixer_inserts_read_in_order_with_the_master_first() {
    let mut expected = project(MODERN, 96);
    expected.mixer.delay_compensation = Some(true);
    expected.mixer.inserts = vec![
        Insert {
            volume: Some(12_800),
            pan: Some(0),
            ..insert("Master")
        },
        Insert {
            color: Some(0xFF_14_14),
            flags: Some(0x104C),
            volume: Some(16_000),
            pan: Some(-6_400),
            stereo_separation: Some(64),
            eq: [
                InsertEqBand {
                    gain: Some(1_800),
                    frequency: Some(0),
                    width: Some(0),
                },
                InsertEqBand {
                    gain: Some(0),
                    frequency: Some(33_145),
                    width: Some(17_500),
                },
                InsertEqBand {
                    gain: Some(-1_800),
                    frequency: Some(65_536),
                    width: Some(65_536),
                },
            ],
            routes: vec![
                Route {
                    target: 0,
                    level: Some(12_800),
                },
                Route {
                    target: 3,
                    level: Some(6_400),
                },
                Route {
                    target: 126,
                    level: None,
                },
            ],
            input: Some(-1),
            ..insert("Drums")
        },
        Insert {
            name: None,
            flags: Some(0x44),
            ..insert("")
        },
    ];
    let read = round_trip(&expected);
    assert_eq!(read.mixer, expected.mixer);
    let drums = &read.mixer.inserts[1];
    assert!(drums.enabled() && drums.solo() && drums.effects_enabled());
    assert!(!read.mixer.inserts[2].enabled());
}

#[test]
fn effect_slots_read_their_plugin_name_state_and_controls() {
    let mut expected = project(MODERN, 96);
    expected.mixer.inserts = vec![
        insert("Master"),
        Insert {
            slots: vec![
                Slot {
                    index: 0,
                    plugin: Plugin {
                        internal_name: "Example EQ".to_owned(),
                        generator: Some(false),
                        state: vec![9, 8, 7],
                    },
                    name: Some("Tone".to_owned()),
                    color: Some(0x00_00_FF),
                    enabled: Some(false),
                    mix: Some(6_400),
                },
                Slot {
                    index: 7,
                    plugin: Plugin {
                        internal_name: "Example Delay".to_owned(),
                        generator: Some(false),
                        state: Vec::new(),
                    },
                    name: None,
                    color: None,
                    enabled: Some(true),
                    mix: Some(12_800),
                },
            ],
            ..insert("Bus")
        },
    ];
    assert_eq!(round_trip(&expected).mixer, expected.mixer);
}

#[test]
fn the_mixer_of_fl_studio_11_numbers_slots_in_their_wrappers() {
    let mut expected = project("11.1.0", 96);
    expected.channels.push(Channel {
        iid: 0,
        name: Some("Lead".to_owned()),
        ..Channel::default()
    });
    expected.mixer.inserts = vec![
        insert("Master"),
        Insert {
            color: Some(0x4A_4B_79),
            slots: vec![
                Slot {
                    index: 0,
                    plugin: Plugin {
                        internal_name: "Example EQ".to_owned(),
                        generator: Some(false),
                        state: vec![2, 0, 0, 0],
                    },
                    ..Slot::default()
                },
                Slot {
                    index: 2,
                    plugin: Plugin {
                        internal_name: "Example Reverb".to_owned(),
                        generator: Some(false),
                        state: vec![1],
                    },
                    name: Some("Hall".to_owned()),
                    ..Slot::default()
                },
            ],
            routes: vec![Route {
                target: 6,
                level: None,
            }],
            ..insert("Lead")
        },
    ];
    let read = round_trip(&expected);
    assert_eq!(read.mixer, expected.mixer);
    // The channel before the mixer keeps its own name.
    assert_eq!(read.channels, expected.channels);
}

#[test]
fn slot_numbers_carry_a_flag_in_their_high_byte() {
    let project = read(|events| {
        events.text(204, "Bus");
        events.data(236, &[0, 0, 0, 0, 0x4C, 0, 0, 0, 0, 0, 0, 0]);
        events.text(201, "Example EQ");
        events.data(213, &[1]);
        events.word(98, 0x0103);
        events.dword(147, u32::MAX);
    });
    assert_eq!(project.mixer.inserts[0].slots[0].index, 3);
}

#[test]
fn control_values_that_are_not_about_an_insert_stay_with_the_mixer() {
    let main_volume = ControlValue {
        location: ControlTarget::Main(MainParam::Volume).encode(),
        value: 10_000,
    };
    let mut expected = project(MODERN, 96);
    expected.mixer.inserts = vec![insert("Master")];
    expected.mixer.other_controls = vec![main_volume];
    expected.initial_controls = vec![ControlValue {
        location: ControlTarget::Channel {
            channel: 1,
            param: ChannelParam::Volume,
        }
        .encode(),
        value: 12_800,
    }];
    let read = round_trip(&expected);
    assert_eq!(read.mixer, expected.mixer);
    assert_eq!(read.mixer.main_volume(), Some(10_000));
    assert_eq!(read.initial_controls, expected.initial_controls);
    assert_eq!(
        read.initial_controls[0].target(),
        ControlTarget::Channel {
            channel: 1,
            param: ChannelParam::Volume
        }
    );
}

#[test]
fn a_link_from_an_automation_clip_names_its_clip_and_its_target() {
    let mut expected = project(MODERN, 96);
    expected.remote_controllers = vec![
        RemoteController {
            source: 5,
            location: ControlTarget::Insert {
                insert: 2,
                param: InsertParam::Volume,
            }
            .encode(),
            flags: 8,
            smoothing: 469,
        },
        RemoteController {
            source: 0x22C3,
            location: ControlTarget::Main(MainParam::Tempo).encode(),
            flags: 8,
            smoothing: 0,
        },
    ];
    let read = round_trip(&expected);
    assert_eq!(read.remote_controllers, expected.remote_controllers);
    assert_eq!(read.remote_controllers[0].source_channel(), Some(5));
    assert_eq!(
        read.remote_controllers[0].target(),
        ControlTarget::Insert {
            insert: 2,
            param: InsertParam::Volume
        }
    );
    // A controller plugin in the mixer is not an automation clip.
    assert_eq!(read.remote_controllers[1].source_channel(), None);
}

#[test]
fn events_nothing_reads_are_kept_as_they_are_and_counted() {
    let mut expected = project(MODERN, 96);
    expected.uninterpreted = vec![
        RawEvent {
            id: 28,
            value: RawValue::Byte(1),
        },
        RawEvent {
            id: 101,
            value: RawValue::Word(0xBEEF),
        },
        RawEvent {
            id: 159,
            value: RawValue::DWord(2576),
        },
        RawEvent {
            id: 250,
            value: RawValue::Data(vec![1, 2, 3]),
        },
        RawEvent {
            id: 250,
            value: RawValue::Data(Vec::new()),
        },
    ];
    let read = round_trip(&expected);
    assert_eq!(read.uninterpreted, expected.uninterpreted);
    assert_eq!(read.uninterpreted_counts.get(&250), Some(&2));
    // 28 and 159 are ids the sources have names for; 101 and 250 are new.
    assert_eq!(read.unknown_ids(), [101, 250]);
}

#[test]
fn the_name_of_the_license_holder_is_counted_and_not_kept() {
    let project = read(|events| events.text(200, "Someone's Name"));
    assert_eq!(project.uninterpreted_counts.get(&200), Some(&1));
    assert!(project.uninterpreted.is_empty());
}

#[test]
fn events_of_a_channel_with_no_channel_before_them_are_kept_raw() {
    let project = read(|events| {
        events.byte(21, 2);
        events.dword(135, 60);
    });
    assert!(project.channels.is_empty());
    assert_eq!(project.uninterpreted.len(), 2);
}

#[test]
fn a_file_that_is_cut_off_gives_what_came_before_and_says_so() {
    let mut expected = project(MODERN, 96);
    expected.settings.title = Some("Cut".to_owned());
    expected.channels.push(Channel {
        iid: 0,
        name: Some("Kick".to_owned()),
        ..Channel::default()
    });
    let whole = write(&expected);
    let cut = &whole[..whole.len() - 3];
    let read = parse(cut).expect("the start is readable");
    assert_eq!(read.settings.title.as_deref(), Some("Cut"));
    assert_eq!(read.channels.len(), 1);
    assert_eq!(read.channels[0].name, None);
    let messages: Vec<String> = read
        .diagnostics
        .iter()
        .map(|d| windfall_core::english::english(&d.message))
        .collect();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("shorter than it says"))
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("ends in the middle"))
    );
}

#[test]
fn other_kinds_of_file_are_read_and_named() {
    let mut events = Events::new(true);
    events.data(199, b"20.8.4.2576\0");
    events.word(64, 0);
    events.text(203, "Preset");
    let header = Header {
        format: FileFormat::ChannelState,
        ..HEADER
    };
    let project = parse(&file(header, &events.bytes)).expect("a readable file");
    assert_eq!(
        project.header.map(|h| h.format),
        Some(FileFormat::ChannelState)
    );
    assert_eq!(FileFormat::ChannelState.describe(), "a channel preset");
    assert_eq!(project.channels.len(), 1);
}

#[test]
fn more_inserts_than_a_mixer_can_have_are_capped() {
    let project = read(|events| {
        for _ in 0..windfall_flp::parse::MAX_INSERTS + 50 {
            events.dword(147, 0);
        }
    });
    assert_eq!(
        project.mixer.inserts.len(),
        windfall_flp::parse::MAX_INSERTS
    );
    assert!(project.diagnostics.iter().any(|d| {
        windfall_core::english::english(&d.message)
            .contains("more mixer inserts than are read. 50 were left out")
    }));
}

#[test]
fn a_flood_of_unknown_events_is_counted_but_not_all_kept() {
    let flood = windfall_flp::parse::MAX_KEPT_EVENTS + 100;
    let project = read(|events| {
        for _ in 0..flood {
            events.byte(40, 1);
        }
    });
    assert_eq!(
        project.uninterpreted.len(),
        windfall_flp::parse::MAX_KEPT_EVENTS
    );
    assert_eq!(project.uninterpreted_counts.get(&40), Some(&(flood as u32)));
}
