//! Property tests through a real document: any song imports into a valid
//! project as one undo step, and exporting that project gives the song's
//! notes and tempo back.

mod common;

use common::*;
use proptest::prelude::*;
use windfall_midi::*;
use windfall_project::{
    Document, MAX_SONG_TICKS, MAX_TEMPO_BPM, MAX_TIMELINE_ITEMS, MIN_TEMPO_BPM, Project,
};

/// Equal apart from `next_id`, which undo leaves alone.
fn same_content(a: &Project, b: &Project) -> bool {
    let mut b = b.clone();
    b.next_id = a.next_id;
    *a == b
}

/// Imports a song into `document` and checks what every import promises.
fn import_into(
    document: &mut Document,
    song: &MidiSong,
    options: &ImportOptions,
) -> Result<ImportPlan, TestCaseError> {
    let before = document.project().clone();
    let steps = document.history().cursor;
    let plan = import(song, options);
    let applied = document.dispatch(plan.command(document.project()), None);
    prop_assert!(applied.is_ok(), "the import failed: {:?}", applied);
    prop_assert_eq!(document.project().check(), Ok(()));

    let project = document.project();
    prop_assert_eq!(
        project.channels.len(),
        before.channels.len() + plan.channels.len()
    );
    prop_assert_eq!(
        project.patterns.len(),
        before.patterns.len() + plan.patterns.len()
    );
    let added = project.playlist.clips.len() - before.playlist.clips.len();
    let tempo_clip = usize::from(!plan.tempo_points.is_empty());
    prop_assert_eq!(added, plan.clips.len() + tempo_clip);
    let notes = |project: &Project| -> usize {
        let lanes = project.patterns.iter().flat_map(|pattern| &pattern.lanes);
        lanes.map(|lane| lane.notes.len()).sum()
    };
    prop_assert_eq!(notes(project) - notes(&before), plan.note_count());

    // One undo takes all of it back, and one redo brings it again.
    if document.history().cursor > steps {
        prop_assert_eq!(document.history().cursor, steps + 1);
        let after = document.project().clone();
        document.undo();
        prop_assert!(same_content(document.project(), &before));
        document.redo();
        prop_assert_eq!(document.project(), &after);
    }
    Ok(plan)
}

fn options() -> impl Strategy<Value = ImportOptions> {
    let patterns = prop_oneof![
        Just(PatternStrategy::PerTrack),
        (0_u32..6, any::<bool>()).prop_map(|(bars, share)| PatternStrategy::Bars { bars, share }),
    ];
    let kit = prop_oneof![Just(None), Just(Some(DrumKit::factory()))];
    (
        patterns,
        kit,
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(patterns, drum_kit, tempo, time_signature, channel_mix, note_expression)| {
                ImportOptions {
                    patterns,
                    drum_kit,
                    tempo,
                    time_signature,
                    channel_mix,
                    note_expression,
                }
            },
        )
}

/// A song whose notes reach well past the 64 bars one pattern holds, with
/// some of them on the drum channel.
fn long_song() -> impl Strategy<Value = MidiSong> {
    let wild = Wildness {
        out_of_range: true,
        ticks: 700_000,
    };
    (midi_song(wild), any::<u64>()).prop_map(|(mut song, seed)| {
        let notes = song.tracks.iter_mut().flat_map(|track| &mut track.notes);
        for (index, note) in notes.enumerate() {
            // A third of the notes go to the drum channel, on the keys
            // drums are on, and now and then a note is very long.
            let dice = seed.rotate_left(index as u32 % 64);
            if dice % 3 == 0 {
                note.channel = DRUM_CHANNEL;
                note.key = 35 + (dice % 50) as u8;
            }
            if dice % 11 == 0 {
                note.length = note.length.saturating_mul(400);
            }
        }
        song
    })
}

/// Expected complete import metadata, derived from the normalized source:
/// supported signature bounds, last event on a tick, and 4/4 before a late
/// first event. The oracle does not read the import plan or exported map.
fn expected_time_signatures(song: &MidiSong) -> Vec<TimeSignatureChange> {
    let checked: std::collections::BTreeMap<_, _> = song
        .time_signatures
        .iter()
        .filter(|event| event.tick < MAX_SONG_TICKS)
        .map(|event| {
            (
                event.tick,
                TimeSignatureChange {
                    tick: event.tick,
                    numerator: event.numerator.clamp(1, 16),
                    denominator: event.denominator.clamp(2, 16).next_power_of_two(),
                },
            )
        })
        .collect();
    let mut expected: Vec<_> = checked.into_values().take(MAX_TIMELINE_ITEMS).collect();
    if expected.first().is_none_or(|event| event.tick != 0) {
        expected.insert(
            0,
            TimeSignatureChange {
                tick: 0,
                numerator: 4,
                denominator: 4,
            },
        );
    }
    expected
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn any_song_imports_into_a_valid_project_as_one_undo_step(
        song in long_song(),
        options in options(),
        again in options(),
    ) {
        let mut document = Document::new(Project::new("Test"));
        let plan = import_into(&mut document, &song, &options)?;
        for adjustment in &plan.adjustments {
            prop_assert!(!adjustment.to_string().is_empty());
        }
        // A second import, into a project that now has things in it.
        import_into(&mut document, &song, &again)?;
    }

    #[test]
    fn however_the_patterns_are_cut_the_song_plays_the_same(
        song in long_song(),
        options in options(),
    ) {
        let mut document = Document::new(Project::new("Test"));
        let plan = import_into(&mut document, &song, &options)?;
        let exported = export_song(document.project(), &ExportOptions::default());
        let exported = exported.expect("the song exports");

        // Every tick a key is held in the file, it is held in the project:
        // a note may be split where a pattern ends, but it is all there.
        let held = |notes: &mut dyn Iterator<Item = &MidiNote>| -> u64 {
            notes.map(|note| u64::from(note.length)).sum()
        };
        let normal = song.clone().normalized();
        let in_file = held(&mut normal.tracks.iter().flat_map(|track| &track.notes));
        let in_project = held(&mut exported.tracks.iter().flat_map(|track| &track.notes));
        prop_assert_eq!(in_project, in_file);
        // Each clip plays the notes of its pattern once.
        let placed: usize = plan
            .clips
            .iter()
            .flat_map(|clip| &plan.patterns[clip.pattern].lanes)
            .map(|lane| lane.notes.len())
            .sum();
        prop_assert_eq!(exported.note_count(), placed);
        // The song ends with its last pattern, which may be before the
        // file does.
        prop_assert!(exported.length <= plan.length);
    }

    #[test]
    fn exporting_an_imported_song_gives_its_notes_and_tempo_back(
        song in midi_song(Wildness::TAME),
    ) {
        let normal = song.clone().normalized();
        let mut document = Document::new(Project::new("Test"));
        let plan = import_into(&mut document, &song, &ImportOptions::default())?;
        let exported = export_song(document.project(), &ExportOptions::default());
        let exported = exported.expect("the song exports");

        // A track for each part, with the part's name and its notes. The
        // MIDI channel is a new one and the release velocity is forgotten.
        prop_assert_eq!(exported.tracks.len(), plan.channels.len());
        for (track, part) in exported.tracks.iter().zip(&plan.channels) {
            prop_assert_eq!(track.name.as_deref(), Some(part.name.as_str()));
            let source = &normal.tracks[part.midi_track];
            let wanted: Vec<_> = source
                .notes
                .iter()
                .filter(|note| note.channel == part.midi_channel)
                .map(|note| (note.start, note.length, note.key, note.velocity))
                .collect();
            let got: Vec<_> = track
                .notes
                .iter()
                .map(|note| (note.start, note.length, note.key, note.velocity))
                .collect();
            prop_assert_eq!(got, wanted);

            // The first volume and pan come back, unless they are what a
            // MIDI channel has anyway.
            let channel = track.notes[0].channel;
            for (controller, default) in [(CC_VOLUME, 100), (CC_PAN, 64)] {
                let kind = ControlKind::Controller(controller);
                let first = |track: &MidiTrack, channel| {
                    let curve = track.control(channel, kind);
                    curve.map(|curve| curve.points[0].value)
                };
                let wanted = first(source, part.midi_channel).filter(|value| *value != default);
                prop_assert_eq!(first(track, channel), wanted);
            }
        }

        // The tempo on every tick, to within two microseconds per quarter
        // note, of which one is the rounding to what a file stores.
        if !exported.tracks.is_empty() || !plan.tempo_points.is_empty() {
            prop_assert_eq!(exported.length, plan.length);
        }
        let ticks = normal.tempos.iter().chain(&exported.tempos).map(|tempo| tempo.tick);
        let ticks = ticks.flat_map(|tick| [tick.saturating_sub(1), tick, tick + 1]);
        for tick in ticks.chain([0, plan.length - 1]).filter(|tick| *tick < plan.length) {
            let wanted = bpm(normal.micros_per_quarter_at(tick));
            let wanted = micros_per_quarter(wanted.clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM));
            let got = exported.micros_per_quarter_at(tick);
            prop_assert!(got.abs_diff(wanted) <= 2, "tick {}: {} for {}", tick, got, wanted);
        }

        prop_assert_eq!(exported.time_signatures, expected_time_signatures(&normal));
    }

    #[test]
    fn a_song_goes_from_bytes_to_a_project_and_back_to_the_same_bytes(
        mut song in midi_song(Wildness::TAME),
    ) {
        // Whole numbers of beats a minute, which an automation stores
        // exactly, so that the tempo is not rounded on every trip.
        const METRONOME: [u32; 8] =
            [1_000_000, 750_000, 600_000, 500_000, 480_000, 400_000, 300_000, 250_000];
        for tempo in &mut song.tempos {
            tempo.micros_per_quarter = METRONOME[tempo.micros_per_quarter as usize % 8];
        }
        // File, song, project, song, file, and the same again from there:
        // once a song has been through a project, another trip changes
        // nothing.
        let first = read(&write(&song, &WriteOptions::default())).expect("the file is valid");
        let mut document = Document::new(Project::new("Test"));
        import_into(&mut document, &first, &ImportOptions::default())?;
        let exported = export_song(document.project(), &ExportOptions::default());
        let bytes = write(&exported.expect("the song exports"), &WriteOptions::default());

        let second = read(&bytes).expect("the file is valid");
        let mut document = Document::new(Project::new("Test"));
        import_into(&mut document, &second, &ImportOptions::default())?;
        let exported = export_song(document.project(), &ExportOptions::default());
        let again = write(&exported.expect("the song exports"), &WriteOptions::default());
        prop_assert_eq!(again, bytes);
    }
}
