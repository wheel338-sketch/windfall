//! Importing a song into a real document: what it becomes, what is fitted,
//! and that the project is valid afterwards and one undo away from what it
//! was.

mod common;

use common::*;
use windfall_midi::*;
use windfall_project::{
    AutomationRange, AutomationTarget, ChannelSource, ClipContent, Command, DEFAULT_CHANNEL_VOLUME,
    DEFAULT_KEY, Document, MAX_AUTOMATION_POINTS, MAX_MIXER_SIGNAL_TRACKS, MAX_PATTERN_STEPS,
    MAX_SONG_TICKS, Pattern, Project, SamplePath, TimeSignature, TrackId,
};

/// A track with a name and notes on channel 0.
fn named(name: &str, notes: Vec<MidiNote>) -> MidiTrack {
    MidiTrack {
        name: Some(name.to_owned()),
        notes,
        ..MidiTrack::default()
    }
}

fn on_channel(channel: u8, mut note: MidiNote) -> MidiNote {
    note.channel = channel;
    note
}

fn song_of(tracks: Vec<MidiTrack>) -> MidiSong {
    MidiSong {
        tracks,
        ..MidiSong::default()
    }
}

fn tempo_at(tick: u32, bpm: f64) -> TempoChange {
    TempoChange::from_bpm(tick, bpm)
}

fn curve(channel: u8, controller: u8, values: &[(u32, u16)]) -> ControlCurve {
    ControlCurve {
        channel,
        kind: ControlKind::Controller(controller),
        points: values
            .iter()
            .map(|&(tick, value)| ControlPoint { tick, value })
            .collect(),
    }
}

/// Equal apart from `next_id`, which undo leaves alone.
fn same_content(a: &Project, b: &Project) -> bool {
    let mut b = b.clone();
    b.next_id = a.next_id;
    *a == b
}

/// Imports a plan into a document as one command, and checks that the
/// project is valid and that one undo takes all of it back.
#[track_caller]
fn dispatch(document: &mut Document, plan: &ImportPlan) {
    let before = document.project().clone();
    let steps = document.history().cursor;
    let command = plan.command(document.project());
    let applied = match document.dispatch(command, None) {
        Ok(applied) => applied,
        Err(error) => panic!("the import failed: {error}"),
    };
    assert_eq!(applied.label, "Import MIDI");
    document.project().check().expect("the project is valid");
    if document.history().cursor == steps {
        assert!(same_content(&before, document.project()));
        return;
    }
    assert_eq!(document.history().cursor, steps + 1, "one undo step");
    let after = document.project().clone();
    document.undo().expect("there is an import to undo");
    assert!(same_content(document.project(), &before));
    document.redo().expect("there is an import to redo");
    assert_eq!(document.project(), &after);
}

/// Imports a song into a new project.
#[track_caller]
fn imported(song: &MidiSong, options: &ImportOptions) -> (Project, ImportPlan) {
    let mut document = Document::new(Project::new("Test"));
    let plan = import(song, options);
    dispatch(&mut document, &plan);
    (document.project().clone(), plan)
}

fn pattern<'a>(project: &'a Project, name: &str) -> &'a Pattern {
    let found = project.patterns.iter().find(|pattern| pattern.name == name);
    found.unwrap_or_else(|| panic!("there is no pattern called {name}"))
}

/// The notes of a pattern's only lane as (start, length, key).
fn notes_of(pattern: &Pattern) -> Vec<(u32, u32, u8)> {
    assert_eq!(pattern.lanes.len(), 1, "{pattern:?}");
    let notes = pattern.lanes[0].notes.iter();
    notes
        .map(|note| (note.start, note.length, note.key))
        .collect()
}

fn has(plan: &ImportPlan, adjustment: &Adjustment) -> bool {
    plan.adjustments.contains(adjustment)
}

#[test]
fn every_track_becomes_a_channel_a_pattern_and_a_clip() {
    let piano = named(
        "Piano",
        vec![
            note(0, 960, 60),
            MidiNote {
                velocity: 50,
                ..note(960, 480, 64)
            },
        ],
    );
    let bass = named("Bass", vec![on_channel(1, note(0, 3_000, 36))]);
    let song = MidiSong {
        name: Some("Demo".to_owned()),
        ..song_of(vec![piano, bass])
    };
    let (project, plan) = imported(&song, &ImportOptions::default());

    assert_eq!(plan.name.as_deref(), Some("Demo"));
    assert_eq!(plan.length, 3_840);
    assert_eq!(plan.note_count(), 3);
    assert!(plan.adjustments.is_empty(), "{:?}", plan.adjustments);
    assert_eq!(project.settings.name, "Test");

    let names: Vec<_> = project.channels.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Piano", "Bass"]);
    for channel in &project.channels {
        assert!(matches!(channel.source, ChannelSource::Instrument { .. }));
        assert_eq!(channel.volume, DEFAULT_CHANNEL_VOLUME);
        // Each channel has the mixer track a new channel gets.
        let track = project.mixer.track(channel.mixer_track).expect("a track");
        assert_ne!(track.id, TrackId::MASTER);
        assert_eq!(track.name, channel.name);
    }

    let piano = pattern(&project, "Piano");
    assert_eq!(piano.length_steps, 16);
    assert_eq!(piano.lanes[0].channel, project.channels[0].id);
    assert_eq!(notes_of(piano), [(0, 960, 60), (960, 480, 64)]);
    assert_eq!(piano.lanes[0].notes[0].velocity, 100.0 / 127.0);
    assert_eq!(piano.lanes[0].notes[1].velocity, 50.0 / 127.0);
    let bass = pattern(&project, "Bass");
    assert_eq!(bass.lanes[0].channel, project.channels[1].id);
    assert_eq!(notes_of(bass), [(0, 3_000, 36)]);
    // The pattern the project began with is still there.
    assert_eq!(project.patterns.len(), 3);

    let tracks: Vec<_> = project
        .playlist
        .tracks
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(tracks, ["Piano", "Bass"]);
    assert_eq!(project.playlist.clips.len(), 2);
    for (clip, (track, pattern)) in project
        .playlist
        .clips
        .iter()
        .zip(project.playlist.tracks.iter().zip([piano, bass]))
    {
        assert_eq!((clip.start, clip.length, clip.offset), (0, 3_840, 0));
        assert_eq!(clip.track, track.id);
        assert_eq!(
            clip.content,
            ClipContent::Pattern {
                pattern: pattern.id
            }
        );
    }
    assert!(project.automations.is_empty());
}

#[test]
fn the_channels_of_one_track_become_a_channel_each() {
    let track = MidiTrack {
        name: Some("Whole Band".to_owned()),
        notes: vec![
            on_channel(0, note(0, 100, 60)),
            on_channel(1, note(0, 100, 40)),
            on_channel(DRUM_CHANNEL, note(0, 100, 36)),
            on_channel(4, note(0, 100, 70)),
        ],
        programs: vec![
            ProgramChange {
                tick: 0,
                channel: 0,
                program: 0,
            },
            ProgramChange {
                tick: 0,
                channel: 1,
                program: 33,
            },
        ],
        ..MidiTrack::default()
    };
    let (project, plan) = imported(&song_of(vec![track]), &ImportOptions::default());
    let names: Vec<_> = project.channels.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Acoustic Grand Piano",
            "Electric Bass (finger)",
            "Whole Band ch 5",
            "Drums"
        ]
    );
    let planned: Vec<_> = plan
        .channels
        .iter()
        .map(|c| (c.midi_track, c.midi_channel, c.drums, c.program, c.notes))
        .collect();
    assert_eq!(
        planned,
        [
            (0, 0, false, Some(0), 1),
            (0, 1, false, Some(33), 1),
            (0, 4, false, None, 1),
            (0, 9, true, None, 1),
        ]
    );
    assert!(has(
        &plan,
        &Adjustment::DrumsAsSynth {
            channel: "Drums".to_owned()
        }
    ));
    assert!(has(
        &plan,
        &Adjustment::NotImported {
            what: Unsupported::ProgramChanges,
            count: 2
        }
    ));
    assert_eq!(project.patterns.len(), 5);
    assert_eq!(project.playlist.tracks.len(), 4);
}

#[test]
fn a_part_is_named_after_the_best_thing_its_track_has() {
    let program = |channel| ProgramChange {
        tick: 0,
        channel,
        program: 73,
    };
    let tracks = vec![
        MidiTrack {
            name: Some("Lead".to_owned()),
            instrument: Some("Moog".to_owned()),
            notes: vec![note(0, 10, 60)],
            programs: vec![program(0)],
            ..MidiTrack::default()
        },
        MidiTrack {
            instrument: Some("Moog".to_owned()),
            notes: vec![note(0, 10, 60)],
            programs: vec![program(0)],
            ..MidiTrack::default()
        },
        MidiTrack {
            notes: vec![note(0, 10, 60)],
            programs: vec![program(0)],
            ..MidiTrack::default()
        },
        // Tracks with no notes get no channel, but still count.
        MidiTrack::default(),
        named("Conductor's notes", Vec::new()),
        MidiTrack {
            notes: vec![note(0, 10, 60)],
            ..MidiTrack::default()
        },
        named("Kit", vec![on_channel(DRUM_CHANNEL, note(0, 10, 36))]),
        MidiTrack {
            notes: vec![on_channel(DRUM_CHANNEL, note(0, 10, 36))],
            ..MidiTrack::default()
        },
        named(
            "  Tab\there\u{7}  and a name that goes on and on and on and on and on, far past sixty-four characters ",
            vec![note(0, 10, 60)],
        ),
        named("\u{1}\n", vec![note(0, 10, 60)]),
    ];
    let (project, _) = imported(&song_of(tracks), &ImportOptions::default());
    let names: Vec<_> = project.channels.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Lead",
            "Moog",
            "Flute",
            "Track 6",
            "Kit",
            "Drums",
            "Tab here   and a name that goes on and on and on and on and on,",
            "Track 10",
        ]
    );
    assert!(names.iter().all(|name| name.chars().count() <= 64));
}

#[test]
fn the_first_tempo_becomes_the_projects_tempo() {
    let mut song = song_of(vec![named("Lead", vec![note(0, 960, 60)])]);
    let (project, plan) = imported(&song, &ImportOptions::default());
    assert_eq!(plan.tempo_bpm, None);
    assert_eq!(project.settings.tempo_bpm, 120.0);

    song.tempos = vec![tempo_at(0, 96.0)];
    let (project, plan) = imported(&song, &ImportOptions::default());
    assert_eq!(plan.tempo_bpm, Some(96.0));
    assert_eq!(project.settings.tempo_bpm, 96.0);
    // A tempo that never changes needs no automation.
    assert!(plan.tempo_points.is_empty());
    assert!(project.automations.is_empty());
    assert_eq!(project.playlist.tracks.len(), 1);

    // Said twice, it still never changes.
    song.tempos = vec![tempo_at(0, 96.0), tempo_at(960, 96.0)];
    let (_, plan) = imported(&song, &ImportOptions::default());
    assert!(plan.tempo_points.is_empty());

    let ignored = ImportOptions {
        tempo: false,
        ..ImportOptions::default()
    };
    let (project, plan) = imported(&song, &ignored);
    assert_eq!(plan.tempo_bpm, None);
    assert_eq!(project.settings.tempo_bpm, 120.0);
}

#[test]
fn tempo_changes_become_a_stepped_automation_over_the_whole_song() {
    let mut song = song_of(vec![named("Lead", vec![note(0, 7_680, 60)])]);
    song.tempos = vec![
        tempo_at(0, 100.0),
        tempo_at(1_920, 150.0),
        // Two on one tick: the last counts.
        tempo_at(3_840, 90.0),
        tempo_at(3_840, 60.0),
        tempo_at(5_000, 60.0),
        // On the last tick of the song there is nothing left to play.
        tempo_at(7_680, 200.0),
    ];
    let (project, plan) = imported(&song, &ImportOptions::default());
    assert_eq!(project.settings.tempo_bpm, 100.0);

    assert_eq!(project.automations.len(), 1);
    let automation = &project.automations[0];
    assert_eq!(automation.name, "Tempo");
    assert_eq!(automation.target, AutomationTarget::Tempo);
    assert_eq!(automation.points, plan.tempo_points);
    let steps: Vec<_> = automation
        .points
        .iter()
        .map(|point| {
            assert!(point.hold);
            (point.tick, AutomationRange::TEMPO.value(point.value))
        })
        .collect();
    assert_eq!(steps, [(0, 100.0), (1_920, 150.0), (3_840, 60.0)]);

    let track = project.playlist.tracks.last().expect("a tempo track");
    assert_eq!(track.name, "Tempo");
    let clip = project
        .playlist
        .clips
        .iter()
        .find(|clip| clip.track == track.id)
        .expect("a tempo clip");
    assert_eq!((clip.start, clip.length), (0, 7_680));
    let automation = automation.id;
    assert_eq!(clip.content, ClipContent::Automation { automation });
}

#[test]
fn a_song_whose_first_tempo_comes_late_starts_at_120() {
    let mut song = song_of(vec![named("Lead", vec![note(0, 3_840, 60)])]);
    song.tempos = vec![tempo_at(960, 150.0)];
    let (project, plan) = imported(&song, &ImportOptions::default());
    assert_eq!(project.settings.tempo_bpm, 150.0);
    let steps: Vec<_> = plan
        .tempo_points
        .iter()
        .map(|point| (point.tick, AutomationRange::TEMPO.value(point.value)))
        .collect();
    assert_eq!(steps, [(0, 120.0), (960, 150.0)]);
}

#[test]
fn tempos_outside_the_projects_range_are_brought_into_it() {
    let mut song = song_of(vec![named("Lead", vec![note(0, 3_840, 60)])]);
    song.tempos = vec![
        TempoChange {
            tick: 0,
            micros_per_quarter: MAX_MICROS_PER_QUARTER,
        },
        TempoChange {
            tick: 960,
            micros_per_quarter: 1,
        },
        tempo_at(1_920, 300.0),
    ];
    let (project, plan) = imported(&song, &ImportOptions::default());
    assert_eq!(project.settings.tempo_bpm, 10.0);
    let values: Vec<_> = plan.tempo_points.iter().map(|point| point.value).collect();
    assert_eq!(values[..2], [0.0, 1.0]);
    assert!(has(&plan, &Adjustment::TempoOutOfRange { count: 2 }));
}

#[test]
fn more_tempo_changes_than_an_automation_holds_are_thinned_evenly() {
    let mut song = song_of(vec![named("Lead", vec![note(0, 100_000, 60)])]);
    song.tempos = (0..10_000)
        .map(|index| tempo_at(index * 10, 60.0 + f64::from(index % 200)))
        .collect();
    let (project, plan) = imported(&song, &ImportOptions::default());
    let points = &project.automations[0].points;
    assert_eq!(points.len(), MAX_AUTOMATION_POINTS);
    assert_eq!(points[0].tick, 0);
    assert_eq!(points[MAX_AUTOMATION_POINTS - 1].tick, 99_990);
    assert!(points.windows(2).all(|pair| pair[0].tick < pair[1].tick));
    assert!(has(
        &plan,
        &Adjustment::TempoChangesThinned {
            changes: 10_000,
            kept: MAX_AUTOMATION_POINTS
        }
    ));
}

#[test]
fn a_song_of_nothing_but_tempo_changes_brings_only_those() {
    let song = MidiSong {
        tempos: vec![tempo_at(0, 100.0), tempo_at(3_840, 130.0)],
        length: 7_680,
        ..MidiSong::default()
    };
    let (project, plan) = imported(&song, &ImportOptions::default());
    assert!(!plan.is_empty());
    assert!(project.channels.is_empty());
    assert_eq!(project.automations.len(), 1);
    assert_eq!(project.playlist.clips[0].length, 7_680);
}

#[test]
fn the_first_time_signature_becomes_the_projects() {
    let signature = |tick, numerator, denominator| TimeSignatureChange {
        tick,
        numerator,
        denominator,
    };
    let mut song = song_of(vec![named("Lead", vec![note(0, 3_000, 60)])]);
    song.time_signatures = vec![signature(0, 3, 4)];
    let (project, plan) = imported(&song, &ImportOptions::default());
    let waltz = TimeSignature {
        numerator: 3,
        denominator: 4,
    };
    assert_eq!(project.settings.time_signature, waltz);
    // 3,000 ticks are a little more than one bar of 2,880.
    assert_eq!(plan.length, 5_760);
    assert_eq!(pattern(&project, "Lead").length_steps, 24);
    assert!(plan.adjustments.is_empty());

    let kept = ImportOptions {
        time_signature: false,
        ..ImportOptions::default()
    };
    let (project, plan) = imported(&song, &kept);
    assert_eq!(project.settings.time_signature.numerator, 4);
    assert_eq!(plan.time_signature, None);
    // The song's own bars still decide how long its patterns are.
    assert_eq!(plan.length, 5_760);

    song.time_signatures = vec![
        signature(0, 7, 8),
        signature(960, 7, 8),
        signature(1_920, 4, 4),
        signature(2_880, 5, 4),
    ];
    let (project, plan) = imported(&song, &ImportOptions::default());
    assert_eq!(project.playlist.timeline.meters.len(), 4);
    assert!(!has(&plan, &Adjustment::TimeSignatureChanges { count: 2 }));
}

#[test]
fn a_time_signature_a_project_cannot_have_gets_the_nearest_it_can() {
    let cases = [
        ((17, 8), (16, 8)),
        ((255, 4), (16, 4)),
        ((4, 1), (4, 2)),
        ((3, 32), (3, 16)),
        ((9, 128), (9, 16)),
    ];
    for ((numerator, denominator), (fitted_numerator, fitted_denominator)) in cases {
        let mut song = song_of(vec![named("Lead", vec![note(0, 100, 60)])]);
        song.time_signatures = vec![TimeSignatureChange {
            tick: 0,
            numerator,
            denominator,
        }];
        let (project, plan) = imported(&song, &ImportOptions::default());
        let used = TimeSignature {
            numerator: fitted_numerator,
            denominator: fitted_denominator,
        };
        assert_eq!(project.settings.time_signature, used);
        assert_eq!(plan.length, used.ticks_per_bar());
        assert!(has(
            &plan,
            &Adjustment::TimeSignatureFitted {
                numerator,
                denominator,
                used
            }
        ));
    }
}

#[test]
fn a_song_longer_than_a_pattern_gets_one_pattern_for_each_64_bars() {
    let bar = 3_840;
    let longest = MAX_PATTERN_STEPS * 240;
    let notes = vec![
        note(0, 960, 60),
        // Held across the end of the first pattern.
        note(longest - 480, 1_440, 62),
        // In the third stretch of 64 bars. The second holds nothing else.
        note(longest * 2 + bar, 960, 64),
    ];
    let (project, plan) = imported(
        &song_of(vec![named("Lead", notes)]),
        &ImportOptions::default(),
    );
    assert_eq!(plan.length, longest * 2 + bar * 2);
    assert!(has(&plan, &Adjustment::NotesSplit { count: 1 }));

    let first = pattern(&project, "Lead 1");
    assert_eq!(first.length_steps, MAX_PATTERN_STEPS);
    assert_eq!(notes_of(first), [(0, 960, 60), (longest - 480, 480, 62)]);
    let second = pattern(&project, "Lead 2");
    assert_eq!(second.length_steps, MAX_PATTERN_STEPS);
    assert_eq!(notes_of(second), [(0, 960, 62)]);
    let third = pattern(&project, "Lead 3");
    assert_eq!(third.length_steps, 32);
    assert_eq!(notes_of(third), [(bar, 960, 64)]);

    let clips: Vec<_> = project
        .playlist
        .clips
        .iter()
        .map(|clip| (clip.start, clip.length))
        .collect();
    assert_eq!(
        clips,
        [(0, longest), (longest, longest), (longest * 2, bar * 2)]
    );
    assert_eq!(plan.note_count(), 4);
}

#[test]
fn bars_of_three_four_cut_a_long_song_on_a_bar_line() {
    let bar = 2_880;
    let mut song = song_of(vec![named("Lead", vec![note(250_000, 100, 60)])]);
    song.time_signatures = vec![TimeSignatureChange {
        tick: 0,
        numerator: 3,
        denominator: 4,
    }];
    let (project, _) = imported(&song, &ImportOptions::default());
    // 85 bars of 3/4 are 1,020 steps, the most whole bars a pattern holds.
    let stretch = 85 * bar;
    let clip = &project.playlist.clips[0];
    assert_eq!(clip.start, stretch);
    assert_eq!(clip.length, 2 * bar);
    assert_eq!(
        notes_of(pattern(&project, "Lead")),
        [(250_000 - stretch, 100, 60)]
    );
}

#[test]
fn splitting_by_bars_lets_bars_that_are_alike_share_a_pattern() {
    let bar = 3_840;
    let riff = |at: u32| [note(at, 480, 60), note(at + 960, 480, 67)];
    let mut notes = Vec::new();
    notes.extend(riff(0));
    notes.extend(riff(bar));
    notes.push(note(bar * 2, 240, 72));
    // The fourth bar is empty.
    notes.extend(riff(bar * 4));
    let song = song_of(vec![named("Riff", notes)]);

    let shared = ImportOptions {
        patterns: PatternStrategy::Bars {
            bars: 1,
            share: true,
        },
        ..ImportOptions::default()
    };
    let (project, plan) = imported(&song, &shared);
    assert_eq!(plan.patterns.len(), 2);
    assert_eq!(
        notes_of(pattern(&project, "Riff 1")),
        [(0, 480, 60), (960, 480, 67)]
    );
    assert_eq!(notes_of(pattern(&project, "Riff 2")), [(0, 240, 72)]);
    assert_eq!(pattern(&project, "Riff 1").length_steps, 16);
    let riff_id = pattern(&project, "Riff 1").id;
    let placed: Vec<_> = project
        .playlist
        .clips
        .iter()
        .map(|clip| {
            let ClipContent::Pattern { pattern } = clip.content else {
                panic!("not a pattern clip");
            };
            (clip.start / bar, clip.length, pattern == riff_id)
        })
        .collect();
    assert_eq!(
        placed,
        [
            (0, bar, true),
            (1, bar, true),
            (2, bar, false),
            (4, bar, true)
        ]
    );

    let separate = ImportOptions {
        patterns: PatternStrategy::Bars {
            bars: 1,
            share: false,
        },
        ..ImportOptions::default()
    };
    let (project, plan) = imported(&song, &separate);
    assert_eq!(plan.patterns.len(), 4);
    assert_eq!(project.playlist.clips.len(), 4);
    assert_eq!(
        notes_of(pattern(&project, "Riff 4")),
        [(0, 480, 60), (960, 480, 67)]
    );

    let two_bars = ImportOptions {
        patterns: PatternStrategy::Bars {
            bars: 2,
            share: true,
        },
        ..ImportOptions::default()
    };
    let (project, plan) = imported(&song, &two_bars);
    assert_eq!(plan.patterns.len(), 3);
    // The song is five bars long, so its last pattern is one bar.
    assert_eq!(pattern(&project, "Riff 1").length_steps, 32);
    assert_eq!(pattern(&project, "Riff 3").length_steps, 16);

    // More bars than a pattern holds means as many as it holds, and none
    // means one.
    for (bars, patterns) in [(u32::MAX, 1), (1_000, 1), (0, 2)] {
        let options = ImportOptions {
            patterns: PatternStrategy::Bars { bars, share: true },
            ..ImportOptions::default()
        };
        let plan = import(&song, &options);
        assert_eq!(plan.patterns.len(), patterns, "{bars} bars");
    }
}

#[test]
fn bars_with_the_same_notes_at_another_velocity_are_not_alike() {
    let bar = 3_840;
    let soft = MidiNote {
        velocity: 40,
        ..note(bar, 480, 60)
    };
    let song = song_of(vec![named("Riff", vec![note(0, 480, 60), soft])]);
    let options = ImportOptions {
        patterns: PatternStrategy::Bars {
            bars: 1,
            share: true,
        },
        ..ImportOptions::default()
    };
    assert_eq!(import(&song, &options).patterns.len(), 2);
}

#[test]
fn notes_past_the_end_of_the_longest_song_are_left_out_or_cut() {
    let notes = vec![
        note(0, 100, 60),
        note(MAX_SONG_TICKS - 100, 500, 62),
        note(MAX_SONG_TICKS, 100, 64),
        note(u32::MAX - 10, 10, 65),
    ];
    let (project, plan) = imported(
        &song_of(vec![named("Lead", notes)]),
        &ImportOptions::default(),
    );
    assert_eq!(plan.length, MAX_SONG_TICKS);
    assert_eq!(plan.note_count(), 2);
    assert!(has(&plan, &Adjustment::NotesPastEnd { count: 2 }));
    assert!(has(&plan, &Adjustment::NotesCutAtEnd { count: 1 }));
    let last = project.playlist.clips.last().expect("a clip");
    assert_eq!(last.start + last.length, MAX_SONG_TICKS);
    let last = project.patterns.last().expect("a pattern");
    let cut = last.lanes[0].notes[0];
    assert_eq!(cut.length, 100);
    assert_eq!(last.length_ticks(), last.lanes[0].notes[0].start + 100);
}

#[test]
fn an_import_stops_at_a_million_notes() {
    // Five notes as long as the longest song, cut into one-bar patterns,
    // are 250,000 notes each.
    let notes = (0..5)
        .map(|key| note(0, MAX_SONG_TICKS, 60 + key))
        .collect();
    let options = ImportOptions {
        patterns: PatternStrategy::Bars {
            bars: 1,
            share: true,
        },
        ..ImportOptions::default()
    };
    let plan = import(&song_of(vec![named("Drone", notes)]), &options);
    // Every bar is the same four notes, so one pattern holds them all.
    assert_eq!(plan.patterns.len(), 1);
    assert_eq!(plan.patterns[0].lanes[0].notes.len(), 4);
    assert_eq!(plan.note_count(), 4);
    assert_eq!(plan.clips.len(), 250_000);
    assert_eq!(plan.clips.len() * 4, MAX_IMPORTED_NOTES);
    assert!(has(&plan, &Adjustment::NotesOverLimit { count: 1 }));
    assert!(has(&plan, &Adjustment::NotesSplit { count: 4 }));
}

#[test]
fn a_drum_kit_gives_every_drum_a_sampler_channel() {
    let hits = [
        (0, 36),
        (240, 42),
        (480, 38),
        (720, 42),
        (960, 35),
        (960, 81),
        (1_200, 43),
        (1_440, 45),
    ];
    let notes = hits
        .iter()
        .map(|&(start, key)| on_channel(DRUM_CHANNEL, note(start, 120, key)))
        .collect();
    let drums = named("Beat", notes);
    let lead = named("Lead", vec![note(0, 960, 36)]);
    let song = song_of(vec![drums, lead]);
    let options = ImportOptions {
        drum_kit: Some(DrumKit::factory()),
        ..ImportOptions::default()
    };
    let mut document = Document::new(Project::new("Test"));
    let plan = import(&song, &options);
    dispatch(&mut document, &plan);
    let project = document.project().clone();

    let names: Vec<_> = project.channels.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Kick Deep",
            "Kick Punch",
            "Snare Tight",
            "Hat Closed 1",
            "Tom Low",
            "Beat",
            "Lead"
        ]
    );
    assert_eq!(project.samples.len(), 5);
    let kick = &project.samples[1];
    assert_eq!(kick.name, "Kick Punch");
    assert_eq!(
        kick.path,
        SamplePath::Factory("Drums/Kicks/Kick Punch.wav".to_owned())
    );
    for (channel, sample) in project.channels.iter().zip(&project.samples) {
        let ChannelSource::Sampler(sampler) = &channel.source else {
            panic!("{} is not a sampler", channel.name);
        };
        assert_eq!(sampler.sample, Some(sample.id));
    }
    assert!(matches!(
        project.channels[5].source,
        ChannelSource::Instrument { .. }
    ));

    // One pattern holds the whole kit, on one playlist track.
    let beat = pattern(&project, "Beat");
    assert_eq!(beat.lanes.len(), 6);
    let lane = |index: usize| {
        let lane = beat.lane(project.channels[index].id).expect("a lane");
        let notes = lane.notes.iter();
        notes.map(|note| (note.start, note.key)).collect::<Vec<_>>()
    };
    assert_eq!(lane(1), [(0, DEFAULT_KEY)]);
    assert_eq!(lane(3), [(240, DEFAULT_KEY), (720, DEFAULT_KEY)]);
    // Two toms of the file share the one tom the kit has for them.
    assert_eq!(lane(4), [(1_200, DEFAULT_KEY), (1_440, DEFAULT_KEY)]);
    // The key with no pad stays what it was, on the synth.
    assert_eq!(lane(5), [(960, 81)]);
    assert_eq!(project.playlist.tracks.len(), 2);
    assert!(has(&plan, &Adjustment::DrumKeysWithoutPad { notes: 1 }));
    assert!(
        !plan
            .adjustments
            .iter()
            .any(|a| matches!(a, Adjustment::DrumsAsSynth { .. }))
    );
    // The melodic track keeps its key 36 as a pitch.
    assert_eq!(notes_of(pattern(&project, "Lead")), [(0, 960, 36)]);

    // A second import finds the samples already there and shares them.
    dispatch(&mut document, &plan);
    assert_eq!(document.project().samples.len(), 5);
    assert_eq!(document.project().channels.len(), 14);
    let again = &document.project().channels[8];
    assert_eq!(again.name, "Kick Punch");
    assert_eq!(again.source.sample(), Some(kick.id));
}

#[test]
fn a_kit_that_covers_every_drum_leaves_no_synth_channel() {
    let kit = DrumKit {
        pads: vec![
            DrumPad {
                key: 36,
                name: "Boom".to_owned(),
                sample: SamplePath::External("C:/kit/boom.wav".to_owned()),
            },
            // A path a project cannot store: the pad is passed over.
            DrumPad {
                key: 38,
                name: "Broken".to_owned(),
                sample: SamplePath::Project("../outside.wav".to_owned()),
            },
        ],
    };
    let options = ImportOptions {
        drum_kit: Some(kit),
        ..ImportOptions::default()
    };
    let kick_only = song_of(vec![MidiTrack {
        notes: vec![on_channel(DRUM_CHANNEL, note(0, 10, 36))],
        ..MidiTrack::default()
    }]);
    let (project, plan) = imported(&kick_only, &options);
    assert_eq!(project.channels.len(), 1);
    assert_eq!(project.channels[0].name, "Boom");
    assert_eq!(project.playlist.tracks[0].name, "Drums");
    assert_eq!(pattern(&project, "Drums").lanes.len(), 1);
    assert!(plan.adjustments.is_empty(), "{:?}", plan.adjustments);

    let with_snare = song_of(vec![MidiTrack {
        notes: vec![
            on_channel(DRUM_CHANNEL, note(0, 10, 36)),
            on_channel(DRUM_CHANNEL, note(0, 10, 38)),
        ],
        ..MidiTrack::default()
    }]);
    let (project, plan) = imported(&with_snare, &options);
    let names: Vec<_> = project.channels.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Boom", "Drums"]);
    assert_eq!(project.samples.len(), 1);
    assert!(has(&plan, &Adjustment::DrumKeysWithoutPad { notes: 1 }));
}

#[test]
fn the_first_volume_and_pan_of_a_part_set_its_channel() {
    let track = MidiTrack {
        name: Some("Pad".to_owned()),
        notes: vec![note(0, 100, 60), on_channel(1, note(0, 100, 60))],
        controls: vec![
            curve(0, CC_VOLUME, &[(0, 127), (500, 20)]),
            curve(0, CC_PAN, &[(10, 0)]),
            curve(1, CC_PAN, &[(0, 127)]),
            curve(0, 1, &[(0, 64)]),
            // A channel with no notes sets nothing.
            curve(5, CC_VOLUME, &[(0, 1)]),
        ],
        ..MidiTrack::default()
    };
    let song = song_of(vec![track]);
    // Without note expression the modulation wheel is left behind too; with
    // it, `tests/expression.rs` covers what the wheel becomes.
    let plain = ImportOptions {
        note_expression: false,
        ..ImportOptions::default()
    };
    let (project, plan) = imported(&song, &plain);
    let first = &project.channels[0];
    assert!((first.volume - 0.8 * 1.27 * 1.27).abs() < 1e-6);
    assert_eq!(first.pan, -1.0);
    let second = &project.channels[1];
    assert_eq!(second.volume, DEFAULT_CHANNEL_VOLUME);
    assert_eq!(second.pan, 1.0);
    assert_eq!(plan.channels[1].volume, None);
    // Three of the six messages were put to use.
    assert!(has(
        &plan,
        &Adjustment::NotImported {
            what: Unsupported::Controllers,
            count: 3
        }
    ));

    let untouched = ImportOptions {
        channel_mix: false,
        ..plain
    };
    let (project, plan) = imported(&song, &untouched);
    assert_eq!(project.channels[0].volume, DEFAULT_CHANNEL_VOLUME);
    assert_eq!(project.channels[0].pan, 0.0);
    assert!(has(
        &plan,
        &Adjustment::NotImported {
            what: Unsupported::Controllers,
            count: 6
        }
    ));
}

#[test]
fn what_a_project_has_no_place_for_is_listed() {
    let track = MidiTrack {
        notes: vec![note(0, 100, 60)],
        controls: vec![
            ControlCurve {
                channel: 0,
                kind: ControlKind::PitchBend,
                points: vec![ControlPoint { tick: 0, value: 0 }; 3],
            },
            ControlCurve {
                channel: 0,
                kind: ControlKind::ChannelPressure,
                points: vec![ControlPoint { tick: 0, value: 0 }; 2],
            },
            ControlCurve {
                channel: 0,
                kind: ControlKind::KeyPressure(60),
                points: vec![ControlPoint { tick: 0, value: 0 }],
            },
        ],
        ..MidiTrack::default()
    };
    let song = MidiSong {
        key_signatures: vec![KeySignatureChange {
            tick: 0,
            sharps: 2,
            minor: false,
        }],
        markers: vec![
            Marker {
                tick: 0,
                text: "Intro".to_owned(),
            },
            Marker {
                tick: 50,
                text: "Verse".to_owned(),
            },
        ],
        ..song_of(vec![track])
    };
    // The bend is listed whole when note expression is not imported. See
    // `tests/expression.rs` for the bend a note takes when it is.
    let plain = ImportOptions {
        note_expression: false,
        ..ImportOptions::default()
    };
    let plan = import(&song, &plain);
    let left = |what, count| Adjustment::NotImported { what, count };
    assert_eq!(
        plan.adjustments,
        [
            left(Unsupported::PitchBend, 3),
            left(Unsupported::Aftertouch, 3),
            left(Unsupported::KeySignatures, 1),
        ]
    );
    let lines: Vec<String> = plan.adjustments.iter().map(ToString::to_string).collect();
    assert_eq!(
        lines,
        [
            "3 pitch bend messages not imported",
            "3 aftertouch messages not imported",
            "1 key signature not imported",
        ]
    );
    assert_eq!(
        plan.markers,
        [(0, "Intro".to_owned()), (50, "Verse".to_owned())]
    );
    let (project, _) = imported(&song, &ImportOptions::default());
    let markers = |timeline: &windfall_project::Timeline| {
        timeline
            .markers
            .iter()
            .map(|m| (m.tick, m.name.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(markers(&project.playlist.timeline), plan.markers);
    assert_eq!(
        markers(&project.patterns.last().unwrap().timeline),
        plan.markers
    );
}

#[test]
fn every_adjustment_reads_as_a_sentence() {
    let used = TimeSignature {
        numerator: 16,
        denominator: 8,
    };
    let lines = [
        (
            Adjustment::NotesPastEnd { count: 2 },
            "2 notes left out: they start past",
        ),
        (Adjustment::NotesCutAtEnd { count: 1 }, "1 note shortened"),
        (
            Adjustment::NotesSplit { count: 3 },
            "3 notes split where a pattern ends",
        ),
        (
            Adjustment::NotesOverLimit { count: 9 },
            "at most 1000000 notes",
        ),
        (
            Adjustment::TempoOutOfRange { count: 1 },
            "1 tempo brought into Windfall's range of 10 to 522 bpm",
        ),
        (
            Adjustment::TempoChangesThinned {
                changes: 9_000,
                kept: 4_096,
            },
            "9000 times and 4096 of the changes are kept",
        ),
        (
            Adjustment::TimeSignatureFitted {
                numerator: 17,
                denominator: 8,
                used,
            },
            "17/8 is not one Windfall has, so 16/8 is used",
        ),
        (
            Adjustment::TimeSignatureChanges { count: 1 },
            "1 time signature change not imported",
        ),
        (
            Adjustment::DrumsAsSynth {
                channel: "Kit".to_owned(),
            },
            "\"Kit\" is on the MIDI drum channel",
        ),
        (
            Adjustment::DrumKeysWithoutPad { notes: 4 },
            "4 drum notes on keys the drum kit has no sound for",
        ),
    ];
    for (adjustment, part) in lines {
        let line = adjustment.to_string();
        assert!(line.contains(part), "{line}");
    }
}

#[test]
fn notes_that_overlap_on_one_key_are_imported_as_they_are() {
    let notes = vec![note(0, 500, 60), note(100, 500, 60), note(100, 500, 60)];
    let (project, _) = imported(
        &song_of(vec![named("Lead", notes)]),
        &ImportOptions::default(),
    );
    assert_eq!(
        notes_of(pattern(&project, "Lead")),
        [(0, 500, 60), (100, 500, 60), (100, 500, 60)]
    );
}

#[test]
fn a_song_that_is_not_in_its_normal_form_is_imported_as_if_it_were() {
    let wild = MidiNote {
        start: 960,
        length: 0,
        key: 200,
        velocity: 0,
        release: 255,
        channel: 77,
    };
    let song = MidiSong {
        tracks: vec![named("Odd\0", vec![wild, note(0, 10, 60)])],
        tempos: vec![tempo_at(960, 75.0), tempo_at(0, 150.0)],
        ..MidiSong::default()
    };
    let (project, plan) = imported(&song, &ImportOptions::default());
    assert_eq!(project.settings.tempo_bpm, 150.0);
    assert_eq!(plan.tempo_points.len(), 2);
    let names: Vec<_> = project.channels.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Odd ch 1", "Odd ch 16"]);
    let odd = &project.patterns.last().expect("a pattern").lanes[0].notes[0];
    assert_eq!((odd.start, odd.length, odd.key), (960, 1, 127));
    assert_eq!(odd.velocity, 1.0 / 127.0);
}

#[test]
fn an_empty_song_imports_as_nothing() {
    let plan = import(&MidiSong::default(), &ImportOptions::default());
    assert!(plan.is_empty());
    assert_eq!(plan.length, 3_840);
    assert!(plan.commands(&Project::new("Test")).is_empty());
    let mut document = Document::new(Project::new("Test"));
    dispatch(&mut document, &plan);
    assert_eq!(document.history().cursor, 0);
}

#[test]
fn a_full_mixer_sends_the_channels_it_has_no_room_for_to_the_master() {
    assert_full_mixer_import(false);
    assert_full_mixer_import(true);
}

fn assert_full_mixer_import(with_current: bool) {
    let mut document = Document::new(Project::new("Test"));
    if with_current {
        document
            .dispatch(Command::EnsureCurrentMixerTrack, None)
            .unwrap();
    }
    // Room for exactly one more mixer track.
    for _ in 0..MAX_MIXER_SIGNAL_TRACKS - 2 {
        let command = Command::AddMixerTrack { name: None };
        document
            .dispatch(command, None)
            .expect("the mixer has room");
    }
    let tracks = (0..3)
        .map(|index| named(&format!("Part {index}"), vec![note(0, 100, 60 + index)]))
        .collect();
    let plan = import(&song_of(tracks), &ImportOptions::default());
    dispatch(&mut document, &plan);
    let project = document.project();
    assert_eq!(
        project.mixer.tracks.len(),
        MAX_MIXER_SIGNAL_TRACKS + usize::from(with_current)
    );
    assert_eq!(
        project
            .mixer
            .tracks
            .iter()
            .filter(|track| track.current)
            .count(),
        usize::from(with_current)
    );
    let routed: Vec<_> = project
        .channels
        .iter()
        .map(|channel| channel.mixer_track == TrackId::MASTER)
        .collect();
    assert_eq!(routed, [false, true, true]);
    // The ids worked out for the notes still found their channels.
    for (index, channel) in project.channels.iter().enumerate() {
        let pattern = pattern(project, &format!("Part {index}"));
        assert_eq!(pattern.lanes[0].channel, channel.id);
        assert_eq!(pattern.lanes[0].notes[0].key, 60 + index as u8);
    }
}

#[test]
fn a_file_read_from_bytes_imports_into_a_project_that_has_things_in_it() {
    let events = [
        meta(0, 0x03, b"Keys"),
        tempo(0, 400_000),
        meta(0, 0x58, &[6, 3, 24, 8]),
        event(0, &[0xB0, 7, 90]),
        event(0, &[0x90, 60, 100]),
        event(480, &[0x90, 64, 80]),
        event(480, &[0x80, 60, 0]),
        // Never ended: it lasts to the end of the track.
        event(0, &[0x90, 67, 1]),
        end(960),
    ];
    let song = read(&file(0, 480, &[track(&events)])).expect("the file is valid");

    let mut document = Document::new(Project::new("Test"));
    for name in ["Kick", "Snare"] {
        let command = Command::AddChannel {
            name: Some(name.to_owned()),
            sample: None,
            instrument: None,
            index: None,
            mixer_track: None,
        };
        document
            .dispatch(command, None)
            .expect("a channel is added");
    }
    let plan = import(&song, &ImportOptions::default());
    dispatch(&mut document, &plan);
    let project = document.project();
    assert_eq!(project.channels.len(), 3);
    assert_eq!(project.settings.tempo_bpm, 150.0);
    assert_eq!(project.settings.time_signature.numerator, 6);
    let keys = pattern(project, "Keys");
    // The file is two beats of 6/8 longer than one bar, so two bars.
    assert_eq!(keys.length_steps, 24);
    assert_eq!(
        notes_of(keys),
        [(0, 1_920, 60), (960, 2_880, 64), (1_920, 1_920, 67)]
    );
    assert!((project.channels[2].volume - 0.8 * 0.81).abs() < 1e-6);
    assert_eq!(document.history().cursor, 3);
}
