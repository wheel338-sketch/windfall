//! Public document acceptance for portamento phrasing and the note LFO, as
//! one value per note and as a curve along each note.
use windfall_project::file::{from_json, to_json};
use windfall_project::piano_tools::NoteLfoProperty;
use windfall_project::*;

const PATTERN: PatternId = PatternId(1);

fn fixture(notes: Vec<NoteInit>) -> (Document, ChannelId) {
    let mut doc = Document::new(Project::new("Articulation QA"));
    let channel = ChannelId(
        doc.dispatch(
            Command::AddChannel {
                name: Some("Lead".into()),
                sample: None,
                instrument: Some(InstrumentKind::SubtractiveSynth),
                mixer_track: None,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    doc.dispatch(
        Command::AddNotes {
            pattern: PATTERN,
            channel,
            notes,
        },
        None,
    )
    .unwrap();
    (doc, channel)
}

fn init(start: u32, length: u32, key: u8) -> NoteInit {
    NoteInit {
        start,
        length,
        key,
        velocity: Some(0.6),
        pan: Some(0.25),
        expression: None,
    }
}

fn notes(doc: &Document, channel: ChannelId) -> Vec<Note> {
    doc.project()
        .pattern(PATTERN)
        .unwrap()
        .lane(channel)
        .map(|lane| lane.notes.clone())
        .unwrap_or_default()
}

/// Through the JSON boundary the UI and the native bridge use.
fn json(command: &Command) -> Command {
    serde_json::from_str(&serde_json::to_string(command).unwrap()).unwrap()
}

fn transform(
    doc: &mut Document,
    channel: ChannelId,
    selected: Vec<Note>,
    transform: NoteTransform,
) -> Result<Applied, CommandError> {
    doc.dispatch(
        json(&Command::TransformNotes {
            pattern: PATTERN,
            channel,
            notes: selected,
            transform,
        }),
        None,
    )
}

fn entries(doc: &Document) -> usize {
    doc.history().entries.len()
}

#[test]
fn portamento_ties_each_onset_into_the_next_and_glides_every_note_after_the_first() {
    // A chord, a single note, a slide and a last note.
    let slide = NoteExpression {
        articulation: NoteArticulation::Slide,
        ..NoteExpression::default()
    };
    let (mut doc, channel) = fixture(vec![
        init(0, 100, 60),
        init(0, 300, 64),
        init(480, 120, 67),
        NoteInit {
            expression: Some(slide),
            ..init(960, 120, 72)
        },
        init(1_440, 200, 65),
        // Not selected: left exactly as it is.
        init(240, 50, 40),
    ]);
    let all = notes(&doc, channel);
    let untouched = *all.iter().find(|note| note.key == 40).unwrap();
    let selected: Vec<Note> = all.iter().copied().filter(|note| note.key != 40).collect();
    let before = entries(&doc);

    let applied = transform(
        &mut doc,
        channel,
        selected.clone(),
        NoteTransform::Portamento {
            glide: 90,
            overlap: 20,
        },
    )
    .unwrap();
    assert_eq!(applied.label, "Portamento notes");
    assert_eq!(entries(&doc), before + 1, "one undo step");
    let after = notes(&doc, channel);
    let by_key = |key: u8| *after.iter().find(|note| note.key == key).unwrap();

    // Both notes of the first chord reach the next selected onset plus the
    // overlap, and keep their articulation: there is nothing to glide from.
    for key in [60, 64] {
        let note = by_key(key);
        assert_eq!((note.start, note.length), (0, 480 + 20));
        assert_eq!(note.expression.articulation, NoteArticulation::Normal);
        assert_eq!(note.expression.glide_ticks, 240);
    }
    // Every later note glides in from the one before.
    let second = by_key(67);
    assert_eq!((second.start, second.length), (480, 480 + 20));
    assert_eq!(second.expression.articulation, NoteArticulation::Portamento);
    assert_eq!(second.expression.glide_ticks, 90);
    // A slide stays a slide, and still reaches the next onset.
    let slid = by_key(72);
    assert_eq!(slid.length, 480 + 20);
    assert_eq!(slid.expression.articulation, NoteArticulation::Slide);
    // The last onset keeps its length and glides too.
    let last = by_key(65);
    assert_eq!((last.start, last.length), (1_440, 200));
    assert_eq!(last.expression.articulation, NoteArticulation::Portamento);
    assert_eq!(last.expression.glide_ticks, 90);

    // Ids, keys, velocities and pans are kept; the unselected note is untouched.
    assert_eq!(by_key(40), untouched);
    for old in &selected {
        let new = after.iter().find(|note| note.id == old.id).unwrap();
        assert_eq!((new.key, new.velocity, new.pan), (old.key, old.velocity, old.pan));
    }

    // It survives the file, and one undo puts every note back.
    let loaded = from_json(&to_json(doc.project()).unwrap()).unwrap();
    assert_eq!(&loaded, doc.project());
    doc.undo().unwrap();
    assert_eq!(notes(&doc, channel), all);
    doc.redo().unwrap();
    assert_eq!(notes(&doc, channel), after);
}

#[test]
fn portamento_refuses_bad_settings_and_stale_selections_without_changing_anything() {
    let (mut doc, channel) = fixture(vec![init(0, 100, 60), init(480, 100, 62)]);
    let selected = notes(&doc, channel);
    let project = doc.project().clone();
    let history = entries(&doc);
    for (glide, overlap) in [(0, 0), (MAX_PATTERN_TICKS + 1, 0), (240, MAX_PATTERN_TICKS + 1)] {
        let tool = NoteTransform::Portamento { glide, overlap };
        assert!(transform(&mut doc, channel, selected.clone(), tool).is_err());
        assert_eq!(doc.project(), &project);
        assert_eq!(entries(&doc), history);
    }
    let mut stale = selected.clone();
    stale[0].length += 1;
    let tool = NoteTransform::Portamento {
        glide: 240,
        overlap: 0,
    };
    assert!(transform(&mut doc, channel, stale, tool.clone()).is_err());
    assert!(transform(&mut doc, channel, Vec::new(), tool).is_err());
    assert_eq!(doc.project(), &project);

    // An overlap never carries a note past the longest pattern.
    let far = MAX_PATTERN_TICKS - 200;
    let (mut doc, channel) = fixture(vec![init(far - 480, 100, 60), init(far, 100, 62)]);
    let selected = notes(&doc, channel);
    transform(
        &mut doc,
        channel,
        selected,
        NoteTransform::Portamento {
            glide: 240,
            overlap: MAX_PATTERN_TICKS,
        },
    )
    .unwrap();
    let first = notes(&doc, channel)[0];
    assert_eq!(first.start + first.length, MAX_PATTERN_TICKS);
    doc.project().check().unwrap();
}

fn sine(period: u32) -> CurveLfo {
    CurveLfo {
        wave: CurveLfoWave::Sine,
        period,
        phase: 0.0,
        center: 0.5,
        depth: 0.5,
        width: 0.5,
        seed: 7,
    }
}

#[test]
fn the_note_lfo_writes_one_value_per_note_read_at_its_start() {
    // A note every quarter of a 960-tick cycle.
    let (mut doc, channel) = fixture(vec![
        init(0, 100, 60),
        init(240, 100, 62),
        init(480, 100, 64),
        init(720, 100, 65),
    ]);
    let selected = notes(&doc, channel);
    let before = entries(&doc);
    let lfo = |property, strength| NoteTransform::Lfo {
        property,
        origin: 0,
        strength,
        lfo: sine(960),
    };

    transform(&mut doc, channel, selected.clone(), lfo(NoteLfoProperty::Velocity, 1.0)).unwrap();
    assert_eq!(entries(&doc), before + 1);
    let written = notes(&doc, channel);
    let velocities: Vec<f32> = written.iter().map(|note| note.velocity).collect();
    for (got, wanted) in velocities.iter().zip([0.5, 1.0, 0.5, 0.0]) {
        assert!((got - wanted).abs() < 1e-6, "{velocities:?}");
    }
    // Only the property asked for moves.
    for (new, old) in written.iter().zip(&selected) {
        assert_eq!((new.id, new.start, new.length, new.key), (old.id, old.start, old.length, old.key));
        assert_eq!(new.pan, old.pan);
        assert_eq!(new.expression, old.expression);
    }
    doc.undo().unwrap();
    assert_eq!(notes(&doc, channel), selected);

    // Each property covers its own whole range: pan -1 to 1, fine pitch
    // -1200 to 1200 cents, the others 0 to 1.
    transform(&mut doc, channel, selected.clone(), lfo(NoteLfoProperty::Pan, 1.0)).unwrap();
    let pans: Vec<f32> = notes(&doc, channel).iter().map(|note| note.pan).collect();
    for (got, wanted) in pans.iter().zip([0.0, 1.0, 0.0, -1.0]) {
        assert!((got - wanted).abs() < 1e-6, "{pans:?}");
    }
    doc.undo().unwrap();
    transform(&mut doc, channel, selected.clone(), lfo(NoteLfoProperty::FinePitchCents, 1.0))
        .unwrap();
    let cents: Vec<f32> = notes(&doc, channel)
        .iter()
        .map(|note| note.expression.fine_pitch_cents)
        .collect();
    for (got, wanted) in cents.iter().zip([0.0, 1200.0, 0.0, -1200.0]) {
        assert!((got - wanted).abs() < 1e-3, "{cents:?}");
    }
    doc.undo().unwrap();

    // Half strength goes half way from what the note had.
    transform(&mut doc, channel, selected.clone(), lfo(NoteLfoProperty::Velocity, 0.5)).unwrap();
    let second = notes(&doc, channel)[1];
    assert!((second.velocity - 0.8).abs() < 1e-6, "{}", second.velocity);
    doc.undo().unwrap();

    // The origin sets where the cycle starts.
    transform(
        &mut doc,
        channel,
        selected.clone(),
        NoteTransform::Lfo {
            property: NoteLfoProperty::ModulationX,
            origin: 240,
            strength: 1.0,
            lfo: sine(960),
        },
    )
    .unwrap();
    let x: Vec<f32> = notes(&doc, channel)
        .iter()
        .map(|note| note.expression.modulation_x)
        .collect();
    for (got, wanted) in x.iter().zip([0.0, 0.5, 1.0, 0.5]) {
        assert!((got - wanted).abs() < 1e-6, "{x:?}");
    }
    doc.undo().unwrap();

    // A seeded sample-and-hold is the same every time and differs by seed.
    let held = |seed| NoteTransform::Lfo {
        property: NoteLfoProperty::Release,
        origin: 0,
        strength: 1.0,
        lfo: CurveLfo {
            wave: CurveLfoWave::SampleHold,
            period: 240,
            seed,
            ..sine(240)
        },
    };
    let run = |doc: &mut Document, seed| {
        transform(doc, channel, selected.clone(), held(seed)).unwrap();
        let values: Vec<f32> = notes(doc, channel)
            .iter()
            .map(|note| note.expression.release)
            .collect();
        doc.undo().unwrap();
        values
    };
    let first = run(&mut doc, 1);
    assert_eq!(run(&mut doc, 1), first);
    assert_ne!(run(&mut doc, 2), first);
    assert!(first.iter().all(|value| (0.0..=1.0).contains(value)));

    // Bad settings are refused and leave everything alone.
    let project = doc.project().clone();
    for bad in [
        NoteTransform::Lfo {
            property: NoteLfoProperty::Velocity,
            origin: 0,
            strength: 1.5,
            lfo: sine(960),
        },
        NoteTransform::Lfo {
            property: NoteLfoProperty::Velocity,
            origin: MAX_PATTERN_TICKS + 1,
            strength: 1.0,
            lfo: sine(960),
        },
        NoteTransform::Lfo {
            property: NoteLfoProperty::Velocity,
            origin: 0,
            strength: 1.0,
            lfo: sine(0),
        },
    ] {
        assert!(transform(&mut doc, channel, selected.clone(), bad).is_err());
        assert_eq!(doc.project(), &project);
    }
}

fn curve_lfo(
    channel: ChannelId,
    expected: Vec<Note>,
    expected_curves: Vec<NoteExpressionCurve>,
    parameter: NoteCurveParameter,
    strength: f64,
) -> Command {
    Command::GenerateNoteCurveLfo {
        pattern: PATTERN,
        channel,
        expected,
        expected_curves,
        parameter,
        origin: 0,
        resolution: 120,
        strength,
        lfo: sine(960),
    }
}

#[test]
fn the_curve_lfo_gives_each_note_a_curve_along_its_length_in_one_edit() {
    let (mut doc, channel) = fixture(vec![init(0, 960, 60), init(960, 480, 64), init(1_920, 240, 67)]);
    let all = notes(&doc, channel);
    let selected = all[..2].to_vec();
    let before = entries(&doc);

    let applied = doc
        .dispatch(
            json(&curve_lfo(
                channel,
                selected.clone(),
                Vec::new(),
                NoteCurveParameter::FinePitchCents,
                1.0,
            )),
            None,
        )
        .unwrap();
    assert_eq!(applied.label, "Write note curve LFO");
    assert_eq!(entries(&doc), before + 1);
    let curves = doc.project().pattern(PATTERN).unwrap().note_curves.clone();
    assert_eq!(curves.len(), 2, "one curve for each captured note");
    let first = curves.iter().find(|curve| curve.note == selected[0].id).unwrap();
    let second = curves.iter().find(|curve| curve.note == selected[1].id).unwrap();
    assert_eq!(first.parameter, NoteCurveParameter::FinePitchCents);
    // A whole cycle along the first note: up a quarter in, down three quarters in.
    assert!((first.value_at(0.25).unwrap() - 1200.0).abs() < 1e-2);
    assert!((first.value_at(0.75).unwrap() + 1200.0).abs() < 1e-2);
    assert!(first.value_at(0.0).unwrap().abs() < 1e-2);
    // The second note starts a cycle later and is half a cycle long.
    assert!((second.value_at(0.5).unwrap() - 1200.0).abs() < 1e-2);
    assert!(second.value_at(1.0).unwrap().abs() < 1e-2);
    // The notes themselves do not change, and the third has no curve.
    assert_eq!(notes(&doc, channel), all);
    assert!(curves.iter().all(|curve| curve.note != all[2].id));

    // It survives the file.
    let loaded = from_json(&to_json(doc.project()).unwrap()).unwrap();
    assert_eq!(&loaded, doc.project());

    // A second property keeps the first one's curves; the capture must name them.
    let stale = curve_lfo(channel, selected.clone(), Vec::new(), NoteCurveParameter::Pan, 1.0);
    let project = doc.project().clone();
    assert!(doc.dispatch(stale, None).is_err(), "the captured curves changed");
    assert_eq!(doc.project(), &project);
    doc.dispatch(
        curve_lfo(channel, selected.clone(), curves.clone(), NoteCurveParameter::Pan, 1.0),
        None,
    )
    .unwrap();
    let both = doc.project().pattern(PATTERN).unwrap().note_curves.clone();
    assert_eq!(both.len(), 4);
    for curve in &curves {
        assert!(both.contains(curve), "the fine-pitch curve is kept");
    }
    let pan = both
        .iter()
        .find(|curve| curve.note == selected[0].id && curve.parameter == NoteCurveParameter::Pan)
        .unwrap();
    assert!((pan.value_at(0.25).unwrap() - 1.0).abs() < 1e-4);
    assert!(pan.points.iter().all(|point| (-1.0..=1.0).contains(&point.value)));

    // Each edit is one undo step.
    doc.undo().unwrap();
    assert_eq!(doc.project().pattern(PATTERN).unwrap().note_curves, curves);
    doc.undo().unwrap();
    assert!(doc.project().pattern(PATTERN).unwrap().note_curves.is_empty());

    // Changed notes, no notes and bad settings are refused.
    let project = doc.project().clone();
    let mut moved = selected.clone();
    moved[0].start += 1;
    for bad in [
        curve_lfo(channel, moved, Vec::new(), NoteCurveParameter::Pan, 1.0),
        curve_lfo(channel, Vec::new(), Vec::new(), NoteCurveParameter::Pan, 1.0),
        curve_lfo(channel, selected.clone(), Vec::new(), NoteCurveParameter::Pan, 2.0),
        Command::GenerateNoteCurveLfo {
            pattern: PATTERN,
            channel,
            expected: selected.clone(),
            expected_curves: Vec::new(),
            parameter: NoteCurveParameter::Pan,
            origin: 0,
            resolution: 0,
            strength: 1.0,
            lfo: sine(960),
        },
    ] {
        assert!(doc.dispatch(bad, None).is_err());
        assert_eq!(doc.project(), &project);
    }
}
