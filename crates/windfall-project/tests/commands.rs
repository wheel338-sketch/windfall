//! What each command does: its success path, each way it fails, and what it
//! cascades to. Every successful command is also undone and redone.

use windfall_dsp::{
    CompressorParams, DelayParams, DetectorMode, LimiterParams, ParamSet, ReverbParams,
    SynthParams, VoiceMode,
};
use windfall_project::*;

const PATTERN: PatternId = PatternId(1);

#[test]
fn utility_effects_add_edit_gesture_undo_and_v1_save_reopen() {
    for kind in [
        EffectKind::Balance,
        EffectKind::DcBlock,
        EffectKind::ChannelMute,
        EffectKind::Polarity,
        EffectKind::StereoMatrix,
        EffectKind::SoftClipper,
        EffectKind::Distortion,
    ] {
        let mut doc = document();
        let effect = add_effect(&mut doc, TrackId::MASTER, kind);
        let original = slot(&doc, TrackId::MASTER, effect).params;
        let cursor = doc.history().cursor;
        let info = &kind.descriptors()[0];
        for value in [
            info.min,
            if info.default == info.max {
                info.min + (info.max - info.min) * 0.25
            } else {
                info.max
            },
        ] {
            doc.dispatch(set_param(TrackId::MASTER, effect, 0, value), Some(37))
                .unwrap();
        }
        assert_eq!(doc.history().cursor, cursor + 1);
        let edited = slot(&doc, TrackId::MASTER, effect).params;
        assert_ne!(edited, original);
        doc.undo().unwrap();
        assert_eq!(slot(&doc, TrackId::MASTER, effect).params, original);
        doc.redo().unwrap();
        assert_eq!(slot(&doc, TrackId::MASTER, effect).params, edited);
        let json = windfall_project::file::to_json(doc.project()).unwrap();
        let reopened = windfall_project::file::from_json(&json).unwrap();
        assert_eq!(reopened, *doc.project());
        assert_eq!(reopened.format_version, 1);
        // Empty settings objects use serde defaults, including the new kinds.
        let mut value = serde_json::to_value(&reopened).unwrap();
        value["mixer"]["tracks"][0]["effects"][0]["params"] = serde_json::json!({"type": kind});
        let legacy = windfall_project::file::from_json(&value.to_string()).unwrap();
        assert_eq!(legacy.mixer.tracks[0].effects[0].params, original);
        run(
            &mut doc,
            Command::RemoveEffect {
                track: TrackId::MASTER,
                effect,
            },
        );
    }
    let old = windfall_project::file::to_json(&Project::new("legacy v1")).unwrap();
    assert_eq!(
        windfall_project::file::from_json(&old).unwrap(),
        Project::new("legacy v1")
    );
}

fn document() -> Document {
    Document::new(Project::new("Test"))
}

/// Equal apart from `next_id`, which undo leaves alone.
fn same_content(a: &Project, b: &Project) -> bool {
    let mut b = b.clone();
    b.next_id = a.next_id;
    *a == b
}

/// Dispatches a command that must succeed, then checks that the project is
/// valid and that undo and redo restore it exactly.
#[track_caller]
fn run(doc: &mut Document, command: Command) -> Applied {
    let before = doc.project().clone();
    let cursor = doc.history().cursor;
    let applied = match doc.dispatch(command.clone(), None) {
        Ok(applied) => applied,
        Err(error) => panic!("{command:?} failed: {error}"),
    };
    doc.project().check().expect("the project is valid");
    let after = doc.project().clone();
    if doc.history().cursor == cursor {
        assert!(applied.touched.is_empty(), "an edit left no history entry");
        assert!(same_content(&before, &after));
    } else {
        doc.undo().expect("there is an edit to undo");
        assert!(
            same_content(doc.project(), &before),
            "undo did not restore the project"
        );
        doc.project().check().expect("the undone project is valid");
        doc.redo().expect("there is an edit to redo");
        assert_eq!(doc.project(), &after, "redo did not restore the project");
    }
    applied
}

/// Dispatches a command that must fail, and checks that nothing changed.
#[track_caller]
fn fail(doc: &mut Document, command: Command) -> CommandError {
    let before = doc.project().clone();
    let history = doc.history();
    let dirty = doc.is_dirty();
    let error = match doc.dispatch(command.clone(), None) {
        Ok(_) => panic!("{command:?} should have failed"),
        Err(error) => error,
    };
    assert_eq!(
        doc.project(),
        &before,
        "a failed command changed the project"
    );
    assert_eq!(doc.history(), history);
    assert_eq!(doc.is_dirty(), dirty);
    error
}

#[track_caller]
fn assert_invalid(error: CommandError, words: &str) {
    match error {
        CommandError::Invalid(message) => {
            assert!(message.contains(words), "\"{message}\" lacks \"{words}\"");
        }
        other => panic!("expected an invalid-input error, got {other:?}"),
    }
}

fn not_found(kind: &'static str, id: u32) -> CommandError {
    CommandError::NotFound { kind, id }
}

fn add_channel(doc: &mut Document, name: &str) -> (ChannelId, TrackId) {
    let applied = run(
        doc,
        Command::AddChannel {
            name: Some(name.to_owned()),
            sample: None,
            instrument: None,
            index: None,
            mixer_track: None,
        },
    );
    (ChannelId(applied.created[0]), TrackId(applied.created[1]))
}

fn add_sample(doc: &mut Document, name: &str) -> SampleId {
    let applied = run(
        doc,
        Command::AddSample {
            name: name.to_owned(),
            path: SamplePath::Factory(format!("drums/{name}.wav")),
        },
    );
    SampleId(applied.created[0])
}

fn add_mixer_track(doc: &mut Document) -> TrackId {
    TrackId(run(doc, Command::AddMixerTrack { name: None }).created[0])
}

fn add_effect(doc: &mut Document, track: TrackId, kind: EffectKind) -> EffectId {
    let applied = run(
        doc,
        Command::AddEffect {
            track,
            kind,
            index: None,
        },
    );
    EffectId(applied.created[0])
}

/// Adds a channel that plays the synth, on a mixer track of its own.
fn add_instrument(doc: &mut Document) -> (ChannelId, TrackId) {
    let applied = run(
        doc,
        Command::AddChannel {
            name: None,
            sample: None,
            instrument: Some(InstrumentKind::SubtractiveSynth),
            index: None,
            mixer_track: None,
        },
    );
    (ChannelId(applied.created[0]), TrackId(applied.created[1]))
}

fn add_playlist_track(doc: &mut Document) -> PlaylistTrackId {
    PlaylistTrackId(
        run(
            doc,
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
        )
        .created[0],
    )
}

fn add_pattern(doc: &mut Document) -> PatternId {
    PatternId(run(doc, Command::AddPattern { name: None }).created[0])
}

fn note(start: u32, key: u8) -> NoteInit {
    NoteInit {
        start,
        length: 240,
        key,
        velocity: None,
        pan: None,
        expression: None,
    }
}

fn add_notes(doc: &mut Document, channel: ChannelId, notes: Vec<NoteInit>) -> Vec<NoteId> {
    let applied = run(
        doc,
        Command::AddNotes {
            pattern: PATTERN,
            channel,
            notes,
        },
    );
    applied.created.into_iter().map(NoteId).collect()
}

fn add_clip(doc: &mut Document, track: PlaylistTrackId, pattern: PatternId, start: u32) -> ClipId {
    let applied = run(
        doc,
        Command::AddClips {
            clips: vec![ClipInit {
                track,
                start,
                length: None,
                offset: None,
                muted: None,
                content: ClipContent::Pattern { pattern },
            }],
        },
    );
    ClipId(applied.created[0])
}

fn lane_notes(doc: &Document, pattern: PatternId, channel: ChannelId) -> Vec<Note> {
    let pattern = doc.project().pattern(pattern).expect("the pattern exists");
    pattern
        .lane(channel)
        .map(|lane| lane.notes.clone())
        .unwrap_or_default()
}

fn sampler(doc: &Document, channel: ChannelId) -> SamplerSettings {
    let channel = doc.project().channel(channel).expect("the channel exists");
    let ChannelSource::Sampler(sampler) = &channel.source else {
        panic!("the channel is not a sampler");
    };
    sampler.clone()
}

fn track(doc: &Document, id: TrackId) -> MixerTrack {
    let track = doc.project().mixer.track(id).expect("the track exists");
    track.clone()
}

fn touched(build: impl FnOnce(&mut Touched)) -> Touched {
    let mut touched = Touched::default();
    build(&mut touched);
    touched
}

fn label(doc: &Document) -> String {
    let history = doc.history();
    history.entries[history.cursor as usize - 1].label.clone()
}

// Project::new

#[test]
fn a_new_project_is_empty_and_valid() {
    let project = Project::new("Song");
    project.check().unwrap();
    assert_eq!(project.format_version, FORMAT_VERSION);
    assert_eq!(project.settings.name, "Song");
    assert_eq!(project.settings.tempo_bpm, 120.0);
    assert_eq!(project.settings.time_signature.numerator, 4);
    assert_eq!(project.settings.time_signature.denominator, 4);
    assert_eq!(project.settings.swing, 0.0);
    assert!(project.samples.is_empty());
    assert!(project.channels.is_empty());
    assert!(project.playlist.tracks.is_empty());
    assert!(project.playlist.clips.is_empty());

    let [pattern] = project.patterns.as_slice() else {
        panic!("expected one pattern");
    };
    assert_eq!(pattern.name, "Pattern 1");
    assert_eq!(pattern.length_steps, 16);
    assert!(pattern.lanes.is_empty());

    let [master] = project.mixer.tracks.as_slice() else {
        panic!("expected only the master track");
    };
    assert_eq!(master.id, TrackId::MASTER);
    assert_eq!(master.name, "Master");
    assert_eq!(master.volume, 1.0);
    assert_eq!(master.output, None);
    assert!(project.next_id > pattern.id.0);
}

// UpdateSettings

#[test]
fn update_settings_changes_each_field() {
    let mut doc = document();
    let applied = run(
        &mut doc,
        Command::UpdateSettings {
            patch: SettingsPatch {
                name: Some("Banger".to_owned()),
                tempo_bpm: Some(140.5),
                time_signature: Some(TimeSignature {
                    numerator: 7,
                    denominator: 8,
                }),
                swing: Some(0.25),
                ..SettingsPatch::default()
            },
        },
    );
    assert_eq!(applied.touched, touched(|t| t.settings = true));
    assert_eq!(applied.label, "Change project settings");
    assert!(applied.created.is_empty());
    let settings = &doc.project().settings;
    assert_eq!(settings.name, "Banger");
    assert_eq!(settings.tempo_bpm, 140.5);
    assert_eq!(settings.time_signature.numerator, 7);
    assert_eq!(settings.time_signature.denominator, 8);
    assert_eq!(settings.swing, 0.25);
}

fn tempo(bpm: f64) -> Command {
    Command::UpdateSettings {
        patch: SettingsPatch {
            tempo_bpm: Some(bpm),
            ..SettingsPatch::default()
        },
    }
}

#[test]
fn update_settings_labels_a_single_field() {
    let mut doc = document();
    assert_eq!(run(&mut doc, tempo(90.0)).label, "Change tempo");
    let rename = Command::UpdateSettings {
        patch: SettingsPatch {
            name: Some("New".to_owned()),
            ..SettingsPatch::default()
        },
    };
    assert_eq!(run(&mut doc, rename).label, "Rename project");
}

#[test]
fn tempo_and_swing_are_clamped() {
    let mut doc = document();
    run(&mut doc, tempo(1.0));
    assert_eq!(doc.project().settings.tempo_bpm, MIN_TEMPO_BPM);
    run(&mut doc, tempo(f64::INFINITY));
    assert_eq!(doc.project().settings.tempo_bpm, MAX_TEMPO_BPM);

    let swing = |swing| Command::UpdateSettings {
        patch: SettingsPatch {
            swing: Some(swing),
            ..SettingsPatch::default()
        },
    };
    run(&mut doc, swing(3.0));
    assert_eq!(doc.project().settings.swing, 1.0);
    run(&mut doc, swing(-3.0));
    assert_eq!(doc.project().settings.swing, 0.0);
    assert_invalid(fail(&mut doc, swing(f32::NAN)), "not a number");
    assert_invalid(fail(&mut doc, tempo(f64::NAN)), "not a number");
}

#[test]
fn a_bad_time_signature_is_rejected() {
    let mut doc = document();
    let signature = |numerator, denominator| Command::UpdateSettings {
        patch: SettingsPatch {
            // The name must not change either when the signature is bad.
            name: Some("Changed".to_owned()),
            time_signature: Some(TimeSignature {
                numerator,
                denominator,
            }),
            ..SettingsPatch::default()
        },
    };
    assert_invalid(fail(&mut doc, signature(0, 4)), "beats per bar");
    assert_invalid(fail(&mut doc, signature(17, 4)), "beats per bar");
    assert_invalid(fail(&mut doc, signature(4, 3)), "beat unit");
    assert_invalid(fail(&mut doc, signature(4, 0)), "beat unit");
    run(&mut doc, signature(16, 16));
}

#[test]
fn a_command_that_changes_nothing_leaves_no_history() {
    let mut doc = document();
    let applied = run(&mut doc, tempo(120.0));
    assert!(applied.touched.is_empty());
    assert_eq!(applied.label, "Change tempo");
    assert!(doc.history().entries.is_empty());
    assert!(!doc.is_dirty());
}

// Samples

#[test]
fn add_sample_registers_a_file() {
    let mut doc = document();
    let applied = run(
        &mut doc,
        Command::AddSample {
            name: "Kick".to_owned(),
            path: SamplePath::Project("samples/kick.wav".to_owned()),
        },
    );
    assert_eq!(applied.touched, touched(|t| t.samples = true));
    assert_eq!(applied.label, "Add sample");
    let [sample] = doc.project().samples.as_slice() else {
        panic!("expected one sample");
    };
    assert_eq!(applied.created, [sample.id.0]);
    assert_eq!(sample.name, "Kick");
    assert_eq!(
        sample.path,
        SamplePath::Project("samples/kick.wav".to_owned())
    );
}

#[test]
fn add_sample_reuses_a_sample_with_the_same_path() {
    let mut doc = document();
    let first = add_sample(&mut doc, "kick");
    let entries = doc.history().entries.len();
    let applied = run(
        &mut doc,
        Command::AddSample {
            name: "Another name".to_owned(),
            path: SamplePath::Factory("drums/kick.wav".to_owned()),
        },
    );
    assert_eq!(applied.created, [first.0]);
    assert!(applied.touched.is_empty());
    assert_eq!(doc.project().samples.len(), 1);
    assert_eq!(doc.history().entries.len(), entries);

    // The same text under another kind is another file.
    let other = run(
        &mut doc,
        Command::AddSample {
            name: "kick".to_owned(),
            path: SamplePath::Project("drums/kick.wav".to_owned()),
        },
    );
    assert_ne!(other.created, [first.0]);
    assert_eq!(doc.project().samples.len(), 2);
}

#[test]
fn add_sample_rejects_a_malformed_path() {
    let mut doc = document();
    let add = |path| Command::AddSample {
        name: "x".to_owned(),
        path,
    };
    assert_invalid(
        fail(&mut doc, add(SamplePath::Project(String::new()))),
        "empty",
    );
    assert_invalid(
        fail(&mut doc, add(SamplePath::External(String::new()))),
        "empty",
    );
    assert_invalid(
        fail(&mut doc, add(SamplePath::Factory("a\\b.wav".to_owned()))),
        "forward slashes",
    );
    assert_invalid(
        fail(&mut doc, add(SamplePath::Project("../b.wav".to_owned()))),
        "inside its folder",
    );
    assert_invalid(
        fail(&mut doc, add(SamplePath::Project("/b.wav".to_owned()))),
        "inside its folder",
    );
}

#[test]
fn remove_sample_fails_while_a_channel_uses_it() {
    let mut doc = document();
    let sample = add_sample(&mut doc, "kick");
    let applied = run(
        &mut doc,
        Command::AddChannel {
            name: None,
            sample: Some(sample),
            instrument: None,
            index: None,
            mixer_track: None,
        },
    );
    let channel = ChannelId(applied.created[0]);

    assert_invalid(
        fail(&mut doc, Command::RemoveSample { id: sample }),
        "still used by the channel \"kick\"",
    );
    assert_eq!(
        fail(&mut doc, Command::RemoveSample { id: SampleId(99) }),
        not_found("sample", 99)
    );

    run(
        &mut doc,
        Command::SetChannelSample {
            id: channel,
            sample: None,
        },
    );
    let applied = run(&mut doc, Command::RemoveSample { id: sample });
    assert_eq!(applied.touched, touched(|t| t.samples = true));
    assert_eq!(applied.label, "Delete sample");
    assert!(doc.project().samples.is_empty());
}

// Channels

#[test]
fn add_channel_makes_a_mixer_track_and_routes_to_it() {
    let mut doc = document();
    let applied = run(
        &mut doc,
        Command::AddChannel {
            name: None,
            sample: None,
            instrument: None,
            index: None,
            mixer_track: None,
        },
    );
    assert_eq!(applied.label, "Add channel");
    assert_eq!(
        applied.touched,
        touched(|t| {
            t.channels = true;
            t.mixer = true;
        })
    );
    let [channel_id, track_id] = applied.created[..] else {
        panic!("expected a channel id and a track id");
    };

    let [channel] = doc.project().channels.as_slice() else {
        panic!("expected one channel");
    };
    assert_eq!(channel.id, ChannelId(channel_id));
    assert_eq!(channel.name, "Sampler");
    assert_eq!(channel.color, PALETTE[0]);
    assert_eq!(channel.volume, DEFAULT_CHANNEL_VOLUME);
    assert_eq!(channel.pan, 0.0);
    assert!(!channel.muted && !channel.solo);
    assert_eq!(channel.mixer_track, TrackId(track_id));
    assert_eq!(
        channel.source,
        ChannelSource::Sampler(SamplerSettings::default())
    );

    let [_, insert] = doc.project().mixer.tracks.as_slice() else {
        panic!("expected the master and one insert");
    };
    assert_eq!(insert.id, TrackId(track_id));
    assert_eq!(insert.name, "Sampler");
    assert_eq!(insert.color, channel.color);
    assert_eq!(insert.volume, 1.0);
    assert_eq!(insert.output, Some(TrackId::MASTER));
    assert!(insert.sends.is_empty());
}

#[test]
fn add_channel_takes_its_name_from_the_sample_unless_given_one() {
    let mut doc = document();
    let sample = add_sample(&mut doc, "snare");
    let add = |name: Option<&str>| Command::AddChannel {
        name: name.map(str::to_owned),
        sample: Some(sample),
        instrument: None,
        index: None,
        mixer_track: None,
    };
    run(&mut doc, add(None));
    run(&mut doc, add(Some("Clap")));
    let channels = &doc.project().channels;
    assert_eq!(channels[0].name, "snare");
    assert_eq!(channels[1].name, "Clap");
    assert_eq!(sampler(&doc, channels[0].id).sample, Some(sample));
    assert_eq!(doc.project().mixer.tracks[2].name, "Clap");
}

#[test]
fn add_channel_on_a_given_track_makes_no_track() {
    let mut doc = document();
    let insert = add_mixer_track(&mut doc);
    let applied = run(
        &mut doc,
        Command::AddChannel {
            name: None,
            sample: None,
            instrument: None,
            index: None,
            mixer_track: Some(insert),
        },
    );
    assert_eq!(applied.created.len(), 1);
    assert_eq!(applied.touched, touched(|t| t.channels = true));
    assert_eq!(doc.project().channels[0].mixer_track, insert);
    assert_eq!(doc.project().mixer.tracks.len(), 2);
}

#[test]
fn add_channel_rejects_unknown_references() {
    let mut doc = document();
    let error = fail(
        &mut doc,
        Command::AddChannel {
            name: None,
            sample: Some(SampleId(50)),
            instrument: None,
            index: None,
            mixer_track: None,
        },
    );
    assert_eq!(error, not_found("sample", 50));
    assert_eq!(error.to_string(), "sample 50 does not exist");
    let error = fail(
        &mut doc,
        Command::AddChannel {
            name: None,
            sample: None,
            instrument: None,
            index: None,
            mixer_track: Some(TrackId(51)),
        },
    );
    assert_eq!(error, not_found("mixer track", 51));
}

#[test]
fn add_channel_places_the_channel_at_the_index() {
    let mut doc = document();
    let (a, _) = add_channel(&mut doc, "A");
    let (b, _) = add_channel(&mut doc, "B");
    let at = |index| Command::AddChannel {
        name: None,
        sample: None,
        instrument: None,
        index: Some(index),
        mixer_track: Some(TrackId::MASTER),
    };
    let first = ChannelId(run(&mut doc, at(0)).created[0]);
    let middle = ChannelId(run(&mut doc, at(2)).created[0]);
    let past_end = ChannelId(run(&mut doc, at(99)).created[0]);
    let order: Vec<ChannelId> = doc.project().channels.iter().map(|c| c.id).collect();
    assert_eq!(order, [first, a, middle, b, past_end]);
}

#[test]
fn new_channels_cycle_through_the_palette() {
    let mut doc = document();
    for _ in 0..PALETTE.len() + 1 {
        run(
            &mut doc,
            Command::AddChannel {
                name: None,
                sample: None,
                instrument: None,
                index: None,
                mixer_track: Some(TrackId::MASTER),
            },
        );
    }
    let colors: Vec<u32> = doc.project().channels.iter().map(|c| c.color).collect();
    assert_eq!(colors[..PALETTE.len()], PALETTE);
    assert_eq!(colors[PALETTE.len()], PALETTE[0]);
}

#[test]
fn add_channel_plays_into_the_master_when_the_mixer_is_full() {
    let mut doc = document();
    for _ in 1..MAX_MIXER_SIGNAL_TRACKS {
        doc.dispatch(Command::AddMixerTrack { name: None }, None)
            .unwrap();
    }
    let applied = run(
        &mut doc,
        Command::AddChannel {
            name: None,
            sample: None,
            instrument: None,
            index: None,
            mixer_track: None,
        },
    );
    assert_eq!(applied.created.len(), 1);
    assert_eq!(doc.project().channels[0].mixer_track, TrackId::MASTER);
    assert_eq!(doc.project().mixer.tracks.len(), MAX_MIXER_SIGNAL_TRACKS);
    assert!(
        doc.project()
            .mixer
            .tracks
            .iter()
            .all(|track| !track.current)
    );
    doc.project().check().unwrap();
}

#[test]
fn remove_channel_takes_its_notes_and_leaves_its_track() {
    let mut doc = document();
    let (kick, kick_track) = add_channel(&mut doc, "Kick");
    let (hat, _) = add_channel(&mut doc, "Hat");
    let second = add_pattern(&mut doc);
    let third = add_pattern(&mut doc);
    add_notes(&mut doc, kick, vec![note(0, 60), note(960, 60)]);
    add_notes(&mut doc, hat, vec![note(480, 60)]);
    run(
        &mut doc,
        Command::ToggleStep {
            pattern: second,
            channel: kick,
            step: 3,
        },
    );

    let applied = run(&mut doc, Command::RemoveChannel { id: kick });
    assert_eq!(applied.label, "Delete channel");
    assert_eq!(
        applied.touched,
        touched(|t| {
            t.channels = true;
            t.patterns = vec![PATTERN, second];
        })
    );
    assert!(doc.project().channel(kick).is_none());
    assert!(doc.project().mixer.track(kick_track).is_some());
    for pattern in [PATTERN, second, third] {
        assert!(lane_notes(&doc, pattern, kick).is_empty());
    }
    assert_eq!(lane_notes(&doc, PATTERN, hat).len(), 1);

    assert_eq!(
        fail(&mut doc, Command::RemoveChannel { id: kick }),
        not_found("channel", kick.0)
    );
}

#[test]
fn duplicate_channel_copies_the_channel_and_its_notes() {
    let mut doc = document();
    let (kick, kick_track) = add_channel(&mut doc, "Kick");
    let (hat, _) = add_channel(&mut doc, "Hat");
    let second = add_pattern(&mut doc);
    add_notes(
        &mut doc,
        kick,
        vec![note(0, 60), note(0, 60), note(960, 64)],
    );
    run(
        &mut doc,
        Command::ToggleStep {
            pattern: second,
            channel: kick,
            step: 2,
        },
    );
    run(
        &mut doc,
        Command::UpdateChannel {
            id: kick,
            patch: ChannelPatch {
                volume: Some(0.5),
                muted: Some(true),
                ..ChannelPatch::default()
            },
        },
    );
    let tracks_before = doc.project().mixer.tracks.len();

    let applied = run(&mut doc, Command::DuplicateChannel { id: kick });
    assert_eq!(applied.label, "Duplicate channel");
    assert_eq!(
        applied.touched,
        touched(|t| {
            t.channels = true;
            t.patterns = vec![PATTERN, second];
        })
    );
    let [copy_id] = applied.created[..] else {
        panic!("expected one created id");
    };
    let copy_id = ChannelId(copy_id);

    let order: Vec<ChannelId> = doc.project().channels.iter().map(|c| c.id).collect();
    assert_eq!(order, [kick, copy_id, hat]);
    let original = doc.project().channel(kick).unwrap();
    let copy = doc.project().channel(copy_id).unwrap();
    assert_eq!(copy.name, "Kick #2");
    assert_eq!(copy.mixer_track, kick_track);
    assert_eq!(copy.volume, 0.5);
    assert!(copy.muted);
    assert_eq!(copy.source, original.source);
    assert_eq!(doc.project().mixer.tracks.len(), tracks_before);

    for pattern in [PATTERN, second] {
        let originals = lane_notes(&doc, pattern, kick);
        let copies = lane_notes(&doc, pattern, copy_id);
        assert_eq!(originals.len(), copies.len());
        for (original, copy) in originals.iter().zip(&copies) {
            assert_ne!(original.id, copy.id);
            assert_eq!(
                Note {
                    id: original.id,
                    ..*copy
                },
                *original
            );
        }
    }

    let again = run(&mut doc, Command::DuplicateChannel { id: copy_id });
    let third = doc.project().channel(ChannelId(again.created[0])).unwrap();
    assert_eq!(third.name, "Kick #3");

    assert_eq!(
        fail(&mut doc, Command::DuplicateChannel { id: ChannelId(77) }),
        not_found("channel", 77)
    );
}

#[test]
fn move_channel_reorders_the_rack() {
    let mut doc = document();
    let (a, _) = add_channel(&mut doc, "A");
    let (b, _) = add_channel(&mut doc, "B");
    let (c, _) = add_channel(&mut doc, "C");
    let order = |doc: &Document| -> Vec<ChannelId> {
        doc.project().channels.iter().map(|c| c.id).collect()
    };

    let applied = run(&mut doc, Command::MoveChannel { id: a, index: 2 });
    assert_eq!(applied.touched, touched(|t| t.channels = true));
    assert_eq!(applied.label, "Move channel");
    assert_eq!(order(&doc), [b, c, a]);

    run(&mut doc, Command::MoveChannel { id: c, index: 0 });
    assert_eq!(order(&doc), [c, b, a]);

    // An index past the end means the end.
    run(&mut doc, Command::MoveChannel { id: c, index: 50 });
    assert_eq!(order(&doc), [b, a, c]);

    let entries = doc.history().entries.len();
    let applied = run(&mut doc, Command::MoveChannel { id: c, index: 2 });
    assert!(applied.touched.is_empty());
    assert_eq!(doc.history().entries.len(), entries);

    assert_eq!(
        fail(
            &mut doc,
            Command::MoveChannel {
                id: ChannelId(77),
                index: 0
            }
        ),
        not_found("channel", 77)
    );
}

fn update_channel(id: ChannelId, patch: ChannelPatch) -> Command {
    Command::UpdateChannel { id, patch }
}

#[test]
fn update_channel_changes_each_field() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let insert = add_mixer_track(&mut doc);
    let applied = run(
        &mut doc,
        update_channel(
            kick,
            ChannelPatch {
                name: Some("Boom".to_owned()),
                color: Some(0x123456),
                volume: Some(1.5),
                pan: Some(-0.5),
                muted: Some(true),
                solo: Some(true),
                mixer_track: Some(insert),
                group: None,
                timing: None,
            },
        ),
    );
    assert_eq!(applied.touched, touched(|t| t.channels = true));
    assert_eq!(applied.label, "Change channel");
    let channel = doc.project().channel(kick).unwrap();
    assert_eq!(channel.name, "Boom");
    assert_eq!(channel.color, 0x123456);
    assert_eq!(channel.volume, 1.5);
    assert_eq!(channel.pan, -0.5);
    assert!(channel.muted && channel.solo);
    assert_eq!(channel.mixer_track, insert);
}

fn rename_channel(id: ChannelId, name: &str) -> Command {
    update_channel(
        id,
        ChannelPatch {
            name: Some(name.to_owned()),
            ..ChannelPatch::default()
        },
    )
}

fn track_name(doc: &Document, id: TrackId) -> String {
    doc.project().mixer.track(id).unwrap().name.clone()
}

#[test]
fn renaming_a_channel_renames_the_mixer_track_made_for_it() {
    let mut doc = document();
    let (lead, lead_track) = add_instrument(&mut doc);
    assert_eq!(track_name(&doc, lead_track), "Subtractive synth");

    // One command, one undo step, both names. `run` undoes and redoes it.
    let steps = doc.history().entries.len();
    let applied = run(&mut doc, rename_channel(lead, "Lead"));
    assert_eq!(applied.label, "Rename channel");
    assert_eq!(
        applied.touched,
        touched(|t| {
            t.channels = true;
            t.mixer = true;
        })
    );
    assert_eq!(doc.history().entries.len(), steps + 1);
    assert_eq!(doc.project().channel(lead).unwrap().name, "Lead");
    assert_eq!(track_name(&doc, lead_track), "Lead");
    doc.undo().unwrap();
    assert_eq!(
        doc.project().channel(lead).unwrap().name,
        "Subtractive synth"
    );
    assert_eq!(track_name(&doc, lead_track), "Subtractive synth");
    doc.redo().unwrap();

    // The track follows again and again, and what is named after it next
    // has the new name.
    run(&mut doc, rename_channel(lead, "Pluck"));
    assert_eq!(track_name(&doc, lead_track), "Pluck");
    let volume = Command::AddAutomation {
        name: None,
        target: AutomationTarget::TrackVolume { track: lead_track },
        points: None,
    };
    let id = AutomationId(run(&mut doc, volume).created[0]);
    assert_eq!(automation(&doc, id).name, "Pluck track volume");

    // A change that is not to the name leaves the track alone.
    let applied = run(
        &mut doc,
        update_channel(
            lead,
            ChannelPatch {
                volume: Some(0.5),
                ..ChannelPatch::default()
            },
        ),
    );
    assert_eq!(applied.touched, touched(|t| t.channels = true));
}

#[test]
fn renaming_a_channel_leaves_a_track_that_is_not_its_own_alone() {
    // The user gave the track a name of its own.
    let mut doc = document();
    let (kick, kick_track) = add_channel(&mut doc, "Kick");
    let renamed = Command::UpdateMixerTrack {
        id: kick_track,
        patch: MixerTrackPatch {
            name: Some("Drums".to_owned()),
            ..MixerTrackPatch::default()
        },
    };
    run(&mut doc, renamed);
    let applied = run(&mut doc, rename_channel(kick, "Boom"));
    assert_eq!(applied.touched, touched(|t| t.channels = true));
    assert_eq!(track_name(&doc, kick_track), "Drums");
    // Named back after the channel by hand, it follows once more.
    let back = Command::UpdateMixerTrack {
        id: kick_track,
        patch: MixerTrackPatch {
            name: Some("Boom".to_owned()),
            ..MixerTrackPatch::default()
        },
    };
    run(&mut doc, back);
    run(&mut doc, rename_channel(kick, "Kick"));
    assert_eq!(track_name(&doc, kick_track), "Kick");

    // A second channel plays into the track.
    let (snare, snare_track) = add_channel(&mut doc, "Snare");
    let route = |channel: ChannelId, track: TrackId| {
        update_channel(
            channel,
            ChannelPatch {
                mixer_track: Some(track),
                ..ChannelPatch::default()
            },
        )
    };
    run(&mut doc, route(snare, kick_track));
    run(&mut doc, rename_channel(kick, "Kick 1"));
    assert_eq!(track_name(&doc, kick_track), "Kick");
    // The track the second channel left has its name still, and the
    // channel is not on it: it stays as it is.
    run(&mut doc, rename_channel(snare, "Snare 1"));
    assert_eq!(track_name(&doc, snare_track), "Snare");
    run(&mut doc, route(snare, snare_track));
    run(&mut doc, rename_channel(kick, "Kick"));

    // Another track plays into it, through its output or through a send.
    let bus = add_mixer_track(&mut doc);
    run(&mut doc, send(bus, kick_track, Some(0.5)));
    run(&mut doc, rename_channel(kick, "Kick 2"));
    assert_eq!(track_name(&doc, kick_track), "Kick");
    run(&mut doc, send(bus, kick_track, None));
    run(&mut doc, rename_channel(kick, "Kick"));
    run(&mut doc, output(bus, Some(kick_track)));
    run(&mut doc, rename_channel(kick, "Kick 3"));
    assert_eq!(track_name(&doc, kick_track), "Kick");
    run(&mut doc, output(bus, Some(TrackId::MASTER)));
    run(&mut doc, rename_channel(kick, "Kick"));

    // An audio clip plays into it.
    let sample = add_sample(&mut doc, "loop");
    let lane = add_playlist_track(&mut doc);
    let clip = ClipInit {
        track: lane,
        start: 0,
        length: Some(960),
        offset: None,
        muted: None,
        content: ClipContent::Audio {
            sample,
            mixer_track: kick_track,
            output: Default::default(),
            normalize: false,
            gain: 1.0,
            pan: 0.0,
            fade_in: 0,
            fade_out: 0,
            reverse: false,
            pitch: 0.0,

            stretch: Default::default(),
        },
    };
    let clip = ClipId(run(&mut doc, Command::AddClips { clips: vec![clip] }).created[0]);
    run(&mut doc, rename_channel(kick, "Kick 4"));
    assert_eq!(track_name(&doc, kick_track), "Kick");
    run(&mut doc, Command::RemoveClips { clips: vec![clip] });
    run(&mut doc, rename_channel(kick, "Kick"));

    // With all of that gone the track is the channel's own again.
    run(&mut doc, rename_channel(kick, "Kick 5"));
    assert_eq!(track_name(&doc, kick_track), "Kick 5");

    // The master is nobody's own, whatever a channel on it is called, and
    // a rename that also moves the channel away leaves the old track.
    let on_master = Command::AddChannel {
        name: Some("Master".to_owned()),
        sample: None,
        instrument: None,
        index: None,
        mixer_track: Some(TrackId::MASTER),
    };
    let direct = ChannelId(run(&mut doc, on_master).created[0]);
    run(&mut doc, rename_channel(direct, "Direct"));
    assert_eq!(track_name(&doc, TrackId::MASTER), "Master");
    let moved = update_channel(
        kick,
        ChannelPatch {
            name: Some("Kick 6".to_owned()),
            mixer_track: Some(bus),
            ..ChannelPatch::default()
        },
    );
    run(&mut doc, moved);
    assert_eq!(track_name(&doc, kick_track), "Kick 5");
}

#[test]
fn update_channel_labels_say_what_changed() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let cases = [
        (
            ChannelPatch {
                name: Some("K".to_owned()),
                ..ChannelPatch::default()
            },
            "Rename channel",
        ),
        (
            ChannelPatch {
                color: Some(1),
                ..ChannelPatch::default()
            },
            "Change channel color",
        ),
        (
            ChannelPatch {
                volume: Some(0.1),
                ..ChannelPatch::default()
            },
            "Change channel volume",
        ),
        (
            ChannelPatch {
                pan: Some(0.1),
                ..ChannelPatch::default()
            },
            "Change channel pan",
        ),
        (
            ChannelPatch {
                muted: Some(true),
                ..ChannelPatch::default()
            },
            "Mute channel",
        ),
        (
            ChannelPatch {
                muted: Some(false),
                ..ChannelPatch::default()
            },
            "Unmute channel",
        ),
        (
            ChannelPatch {
                solo: Some(true),
                ..ChannelPatch::default()
            },
            "Solo channel",
        ),
        (
            ChannelPatch {
                solo: Some(false),
                ..ChannelPatch::default()
            },
            "Unsolo channel",
        ),
        (
            ChannelPatch {
                mixer_track: Some(TrackId::MASTER),
                ..ChannelPatch::default()
            },
            "Route channel",
        ),
    ];
    for (patch, expected) in cases {
        assert_eq!(run(&mut doc, update_channel(kick, patch)).label, expected);
        assert_eq!(label(&doc), expected);
    }
}

#[test]
fn update_channel_validates_its_values() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let volume = |volume| {
        update_channel(
            kick,
            ChannelPatch {
                volume: Some(volume),
                ..ChannelPatch::default()
            },
        )
    };
    let pan = |pan| {
        update_channel(
            kick,
            ChannelPatch {
                pan: Some(pan),
                ..ChannelPatch::default()
            },
        )
    };
    run(&mut doc, volume(9.0));
    assert_eq!(doc.project().channel(kick).unwrap().volume, MAX_GAIN);
    run(&mut doc, volume(-1.0));
    assert_eq!(doc.project().channel(kick).unwrap().volume, 0.0);
    run(&mut doc, pan(-7.0));
    assert_eq!(doc.project().channel(kick).unwrap().pan, -1.0);
    run(&mut doc, pan(f32::INFINITY));
    assert_eq!(doc.project().channel(kick).unwrap().pan, 1.0);
    assert_invalid(fail(&mut doc, volume(f32::NAN)), "not a number");
    assert_invalid(fail(&mut doc, pan(f32::NAN)), "not a number");

    let color = update_channel(
        kick,
        ChannelPatch {
            color: Some(0x1_000_000),
            ..ChannelPatch::default()
        },
    );
    assert_invalid(fail(&mut doc, color), "0xRRGGBB");

    let route = update_channel(
        kick,
        ChannelPatch {
            name: Some("Renamed".to_owned()),
            mixer_track: Some(TrackId(88)),
            ..ChannelPatch::default()
        },
    );
    assert_eq!(fail(&mut doc, route), not_found("mixer track", 88));
    assert_eq!(
        fail(
            &mut doc,
            update_channel(ChannelId(77), ChannelPatch::default())
        ),
        not_found("channel", 77)
    );
}

#[test]
fn negative_zero_is_stored_as_zero() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    run(
        &mut doc,
        update_channel(
            kick,
            ChannelPatch {
                pan: Some(0.5),
                ..ChannelPatch::default()
            },
        ),
    );
    run(
        &mut doc,
        update_channel(
            kick,
            ChannelPatch {
                pan: Some(-0.0),
                ..ChannelPatch::default()
            },
        ),
    );
    assert!(doc.project().channel(kick).unwrap().pan.is_sign_positive());
}

#[test]
fn set_channel_sample_sets_and_clears() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let sample = add_sample(&mut doc, "kick");
    let set = |id, sample| Command::SetChannelSample { id, sample };

    let applied = run(&mut doc, set(kick, Some(sample)));
    assert_eq!(applied.touched, touched(|t| t.channels = true));
    assert_eq!(applied.label, "Change channel sample");
    assert_eq!(sampler(&doc, kick).sample, Some(sample));
    // The channel keeps the name it was given.
    assert_eq!(doc.project().channel(kick).unwrap().name, "Kick");

    run(&mut doc, set(kick, None));
    assert_eq!(sampler(&doc, kick).sample, None);

    assert_eq!(
        fail(&mut doc, set(kick, Some(SampleId(66)))),
        not_found("sample", 66)
    );
    assert_eq!(
        fail(&mut doc, set(ChannelId(77), Some(sample))),
        not_found("channel", 77)
    );
}

fn update_sampler(id: ChannelId, patch: SamplerPatch) -> Command {
    Command::UpdateSampler { id, patch }
}

#[test]
fn update_sampler_changes_each_field() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let applied = run(
        &mut doc,
        update_sampler(
            kick,
            SamplerPatch {
                root_key: Some(48),
                tune: Some(-12.5),
                gain: Some(1.5),
                start: Some(0.25),
                end: Some(0.75),
                reverse: Some(true),
                cut_self: Some(true),
                cut_group: Some(3),
                ..SamplerPatch::default()
            },
        ),
    );
    assert_eq!(applied.touched, touched(|t| t.channels = true));
    assert_eq!(applied.label, "Change sampler");
    assert_eq!(
        sampler(&doc, kick),
        SamplerSettings {
            sample: None,
            root_key: 48,
            tune: -12.5,
            gain: 1.5,
            start: 0.25,
            end: 0.75,
            reverse: true,
            envelope: None,
            cut_self: true,
            cut_group: 3,
            ..SamplerSettings::default()
        }
    );
}

#[test]
fn sampler_loops_validate_and_undo_as_sampler_edits() {
    let mut doc = document();
    let (id, _) = add_channel(&mut doc, "Loop");
    let change = |loop_mode, loop_start, loop_end| {
        update_sampler(
            id,
            SamplerPatch {
                loop_mode,
                loop_start,
                loop_end,
                ..SamplerPatch::default()
            },
        )
    };
    let applied = run(&mut doc, change(Some(SamplerLoopMode::Forward), None, None));
    assert_eq!(applied.label, "Change sample loop mode");
    assert_eq!(sampler(&doc, id).loop_mode, SamplerLoopMode::Forward);
    let applied = run(&mut doc, change(None, Some(0.25), Some(0.75)));
    assert_eq!(applied.label, "Change sample loop range");
    assert_eq!(sampler(&doc, id).loop_start, 0.25);
    assert_eq!(sampler(&doc, id).loop_end, 0.75);
    run(
        &mut doc,
        change(Some(SamplerLoopMode::PingPong), None, None),
    );
    run(&mut doc, change(Some(SamplerLoopMode::Off), None, None));
    assert_eq!(
        sampler(&doc, id).loop_start,
        0.25,
        "disabling keeps the points"
    );
    for (start, end) in [
        (0.8, 0.2),
        (0.5, 0.5),
        (f32::NAN, 1.0),
        (0.0, f32::INFINITY),
    ] {
        let message = if start.is_finite() && end.is_finite() {
            "before its end"
        } else {
            "not a number"
        };
        assert_invalid(
            fail(&mut doc, change(None, Some(start), Some(end))),
            message,
        );
    }
    run(&mut doc, change(None, Some(-2.0), Some(2.0)));
    assert_eq!(
        (sampler(&doc, id).loop_start, sampler(&doc, id).loop_end),
        (0.0, 1.0)
    );
    // Both points can be changed atomically even when either alone would cross.
    run(&mut doc, change(None, Some(0.8), Some(0.9)));
    run(&mut doc, change(None, Some(0.1), Some(0.2)));
}

#[test]
fn update_sampler_validates_its_values() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");

    let applied = run(
        &mut doc,
        update_sampler(
            kick,
            SamplerPatch {
                tune: Some(100.0),
                ..SamplerPatch::default()
            },
        ),
    );
    assert_eq!(applied.label, "Change tuning");
    assert_eq!(sampler(&doc, kick).tune, MAX_TUNE_SEMITONES);
    run(
        &mut doc,
        update_sampler(
            kick,
            SamplerPatch {
                tune: Some(-100.0),
                gain: Some(50.0),
                ..SamplerPatch::default()
            },
        ),
    );
    assert_eq!(sampler(&doc, kick).tune, -MAX_TUNE_SEMITONES);
    assert_eq!(sampler(&doc, kick).gain, MAX_GAIN);

    let root = update_sampler(
        kick,
        SamplerPatch {
            root_key: Some(128),
            ..SamplerPatch::default()
        },
    );
    assert_invalid(fail(&mut doc, root), "key 128");
    let nan = update_sampler(
        kick,
        SamplerPatch {
            gain: Some(f32::NAN),
            ..SamplerPatch::default()
        },
    );
    assert_invalid(fail(&mut doc, nan), "not a number");
    assert_eq!(
        fail(
            &mut doc,
            update_sampler(ChannelId(77), SamplerPatch::default())
        ),
        not_found("channel", 77)
    );
}

#[test]
fn the_sample_start_must_come_before_its_end() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let range = |start, end| {
        update_sampler(
            kick,
            SamplerPatch {
                start,
                end,
                ..SamplerPatch::default()
            },
        )
    };
    assert_invalid(
        fail(&mut doc, range(Some(0.5), Some(0.5))),
        "before its end",
    );
    assert_invalid(
        fail(&mut doc, range(Some(0.8), Some(0.2))),
        "before its end",
    );
    // Out-of-range values are clamped first: a start of 2 becomes 1, the end.
    assert_invalid(fail(&mut doc, range(Some(2.0), None)), "before its end");
    assert_invalid(fail(&mut doc, range(None, Some(-1.0))), "before its end");

    let applied = run(&mut doc, range(Some(-4.0), Some(4.0)));
    assert!(applied.touched.is_empty(), "0 to 1 is the default range");
    run(&mut doc, range(Some(0.4), None));
    assert_eq!(
        run(&mut doc, range(None, Some(0.6))).label,
        "Change sample range"
    );
    assert_eq!(sampler(&doc, kick).start, 0.4);
    assert_eq!(sampler(&doc, kick).end, 0.6);
    assert_invalid(fail(&mut doc, range(None, Some(0.3))), "before its end");
}

#[test]
fn set_sampler_envelope_turns_it_on_and_off() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let set = |id, envelope| Command::SetSamplerEnvelope { id, envelope };

    let applied = run(&mut doc, set(kick, Some(Envelope::default())));
    assert_eq!(applied.touched, touched(|t| t.channels = true));
    assert_eq!(applied.label, "Change envelope");
    assert_eq!(sampler(&doc, kick).envelope, Some(Envelope::default()));

    run(
        &mut doc,
        set(
            kick,
            Some(Envelope {
                attack_ms: -5.0,
                decay_ms: f32::INFINITY,
                sustain: 4.0,
                release_ms: 30.0,
            }),
        ),
    );
    assert_eq!(
        sampler(&doc, kick).envelope,
        Some(Envelope {
            attack_ms: 0.0,
            decay_ms: MAX_ENVELOPE_MS,
            sustain: 1.0,
            release_ms: 30.0,
        })
    );

    let nan = Envelope {
        release_ms: f32::NAN,
        ..Envelope::default()
    };
    assert_invalid(fail(&mut doc, set(kick, Some(nan))), "not a number");

    run(&mut doc, set(kick, None));
    assert_eq!(sampler(&doc, kick).envelope, None);
    assert_eq!(
        fail(&mut doc, set(ChannelId(77), None)),
        not_found("channel", 77)
    );
}

// Patterns

#[test]
fn add_pattern_appends_an_empty_pattern() {
    let mut doc = document();
    let applied = run(&mut doc, Command::AddPattern { name: None });
    let id = PatternId(applied.created[0]);
    assert_eq!(applied.label, "Add pattern");
    assert_eq!(
        applied.touched,
        touched(|t| {
            t.pattern_list = true;
            t.patterns = vec![id];
        })
    );
    let pattern = doc.project().patterns.last().unwrap();
    assert_eq!(pattern.id, id);
    assert_eq!(pattern.name, "Pattern 2");
    assert_eq!(pattern.color, PALETTE[1]);
    assert_eq!(pattern.length_steps, DEFAULT_PATTERN_STEPS);
    assert!(pattern.lanes.is_empty());

    run(
        &mut doc,
        Command::AddPattern {
            name: Some("Pattern 4".to_owned()),
        },
    );
    // "Pattern 4" is taken, so the fourth pattern is numbered past it.
    let applied = run(&mut doc, Command::AddPattern { name: None });
    let pattern = doc
        .project()
        .pattern(PatternId(applied.created[0]))
        .unwrap();
    assert_eq!(pattern.name, "Pattern 5");
}

#[test]
fn remove_pattern_takes_its_clips() {
    let mut doc = document();
    let second = add_pattern(&mut doc);
    let lane = add_playlist_track(&mut doc);
    let kept = add_clip(&mut doc, lane, PATTERN, 0);
    add_clip(&mut doc, lane, second, 3840);
    add_clip(&mut doc, lane, second, 7680);

    let applied = run(&mut doc, Command::RemovePattern { id: second });
    assert_eq!(applied.label, "Delete pattern");
    assert_eq!(
        applied.touched,
        touched(|t| {
            t.pattern_list = true;
            t.playlist = true;
        })
    );
    assert!(doc.project().pattern(second).is_none());
    let clips: Vec<ClipId> = doc.project().playlist.clips.iter().map(|c| c.id).collect();
    assert_eq!(clips, [kept]);
}

#[test]
fn remove_pattern_fails_on_the_last_pattern() {
    let mut doc = document();
    assert_invalid(
        fail(&mut doc, Command::RemovePattern { id: PATTERN }),
        "at least one pattern",
    );
    assert_eq!(
        fail(&mut doc, Command::RemovePattern { id: PatternId(90) }),
        not_found("pattern", 90)
    );
    let second = add_pattern(&mut doc);
    let applied = run(&mut doc, Command::RemovePattern { id: PATTERN });
    assert_eq!(applied.touched, touched(|t| t.pattern_list = true));
    assert_eq!(doc.project().patterns[0].id, second);
}

#[test]
fn duplicate_pattern_copies_notes_under_new_ids() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let (hat, _) = add_channel(&mut doc, "Hat");
    let last = add_pattern(&mut doc);
    add_notes(
        &mut doc,
        kick,
        vec![note(0, 60), note(0, 60), note(480, 62)],
    );
    add_notes(&mut doc, hat, vec![note(240, 60)]);
    run(
        &mut doc,
        Command::UpdatePattern {
            id: PATTERN,
            patch: PatternPatch {
                length_steps: Some(32),
                color: Some(0xABCDEF),
                ..PatternPatch::default()
            },
        },
    );

    let applied = run(&mut doc, Command::DuplicatePattern { id: PATTERN });
    let [copy_id] = applied.created[..] else {
        panic!("expected one created id");
    };
    let copy_id = PatternId(copy_id);
    assert_eq!(applied.label, "Duplicate pattern");
    assert_eq!(
        applied.touched,
        touched(|t| {
            t.pattern_list = true;
            t.patterns = vec![copy_id];
        })
    );
    let order: Vec<PatternId> = doc.project().patterns.iter().map(|p| p.id).collect();
    assert_eq!(order, [PATTERN, copy_id, last]);

    let original = doc.project().pattern(PATTERN).unwrap();
    let copy = doc.project().pattern(copy_id).unwrap();
    assert_eq!(copy.name, "Pattern 1 #2");
    assert_eq!(copy.length_steps, 32);
    assert_eq!(copy.color, 0xABCDEF);
    assert_eq!(copy.lanes.len(), original.lanes.len());
    for (original, copy) in original.lanes.iter().zip(&copy.lanes) {
        assert_eq!(original.channel, copy.channel);
        assert_eq!(original.notes.len(), copy.notes.len());
        for (original, copy) in original.notes.iter().zip(&copy.notes) {
            assert_ne!(original.id, copy.id);
            assert_eq!((original.start, original.key), (copy.start, copy.key));
        }
    }

    assert_eq!(
        fail(&mut doc, Command::DuplicatePattern { id: PatternId(90) }),
        not_found("pattern", 90)
    );
}

#[test]
fn move_pattern_reorders_the_list() {
    let mut doc = document();
    let second = add_pattern(&mut doc);
    let third = add_pattern(&mut doc);
    let order = |doc: &Document| -> Vec<PatternId> {
        doc.project().patterns.iter().map(|p| p.id).collect()
    };

    let applied = run(
        &mut doc,
        Command::MovePattern {
            id: third,
            index: 0,
        },
    );
    assert_eq!(applied.touched, touched(|t| t.pattern_list = true));
    assert_eq!(applied.label, "Move pattern");
    assert_eq!(order(&doc), [third, PATTERN, second]);

    run(
        &mut doc,
        Command::MovePattern {
            id: third,
            index: 99,
        },
    );
    assert_eq!(order(&doc), [PATTERN, second, third]);

    let same = run(
        &mut doc,
        Command::MovePattern {
            id: second,
            index: 1,
        },
    );
    assert!(same.touched.is_empty());
    assert_eq!(
        fail(
            &mut doc,
            Command::MovePattern {
                id: PatternId(90),
                index: 0
            }
        ),
        not_found("pattern", 90)
    );
}

#[test]
fn update_pattern_changes_each_field() {
    let mut doc = document();
    let update = |id, patch| Command::UpdatePattern { id, patch };
    let applied = run(
        &mut doc,
        update(
            PATTERN,
            PatternPatch {
                name: Some("Intro".to_owned()),
                color: Some(0x00FF00),
                length_steps: Some(64),
            },
        ),
    );
    assert_eq!(applied.touched, touched(|t| t.patterns = vec![PATTERN]));
    assert_eq!(applied.label, "Change pattern");
    let pattern = doc.project().pattern(PATTERN).unwrap();
    assert_eq!(pattern.name, "Intro");
    assert_eq!(pattern.color, 0x00FF00);
    assert_eq!(pattern.length_steps, 64);
    assert_eq!(pattern.length_ticks(), 64 * 240);

    let length = |length_steps| {
        update(
            PATTERN,
            PatternPatch {
                length_steps: Some(length_steps),
                ..PatternPatch::default()
            },
        )
    };
    assert_eq!(run(&mut doc, length(0)).label, "Change pattern length");
    assert_eq!(doc.project().pattern(PATTERN).unwrap().length_steps, 1);
    run(&mut doc, length(50_000));
    assert_eq!(
        doc.project().pattern(PATTERN).unwrap().length_steps,
        MAX_PATTERN_STEPS
    );

    let color = update(
        PATTERN,
        PatternPatch {
            color: Some(u32::MAX),
            ..PatternPatch::default()
        },
    );
    assert_invalid(fail(&mut doc, color), "0xRRGGBB");
    assert_eq!(
        fail(&mut doc, update(PatternId(90), PatternPatch::default())),
        not_found("pattern", 90)
    );
}

// Notes

fn toggle(channel: ChannelId, step: u32) -> Command {
    Command::ToggleStep {
        pattern: PATTERN,
        channel,
        step,
    }
}

#[test]
fn toggle_step_adds_then_removes_a_note() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");

    let applied = run(&mut doc, toggle(kick, 4));
    assert_eq!(applied.label, "Toggle step");
    assert_eq!(applied.touched, touched(|t| t.patterns = vec![PATTERN]));
    let [id] = applied.created[..] else {
        panic!("expected one created id");
    };
    assert_eq!(
        lane_notes(&doc, PATTERN, kick),
        [Note {
            id: NoteId(id),
            start: 4 * TICKS_PER_STEP,
            length: TICKS_PER_STEP,
            key: DEFAULT_KEY,
            velocity: DEFAULT_VELOCITY,
            pan: 0.0,
            expression: Default::default(),
        }]
    );

    let applied = run(&mut doc, toggle(kick, 4));
    assert!(applied.created.is_empty());
    assert_eq!(applied.touched, touched(|t| t.patterns = vec![PATTERN]));
    assert!(doc.project().pattern(PATTERN).unwrap().lanes.is_empty());
}

#[test]
fn toggle_step_removes_every_note_that_starts_on_the_step() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let ids = add_notes(
        &mut doc,
        kick,
        vec![note(480, 60), note(480, 72), note(481, 60), note(0, 60)],
    );
    run(&mut doc, toggle(kick, 2));
    let left: Vec<NoteId> = lane_notes(&doc, PATTERN, kick)
        .iter()
        .map(|n| n.id)
        .collect();
    assert_eq!(left, [ids[3], ids[2]]);
}

#[test]
fn toggle_step_rejects_unknown_ids_and_impossible_steps() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    assert_eq!(
        fail(&mut doc, toggle(ChannelId(77), 0)),
        not_found("channel", 77)
    );
    let unknown_pattern = Command::ToggleStep {
        pattern: PatternId(90),
        channel: kick,
        step: 0,
    };
    assert_eq!(fail(&mut doc, unknown_pattern), not_found("pattern", 90));
    assert_invalid(
        fail(&mut doc, toggle(kick, u32::MAX)),
        "step 4294967296 is past the end of the longest pattern, which has 1024 steps",
    );
    assert_invalid(
        fail(&mut doc, toggle(kick, MAX_PATTERN_STEPS)),
        "step 1025 is past the end of the longest pattern",
    );
    // The last step of the longest pattern is the last that can be lit.
    let applied = run(&mut doc, toggle(kick, MAX_PATTERN_STEPS - 1));
    let notes = lane_notes(&doc, PATTERN, kick);
    assert_eq!(notes[0].id, NoteId(applied.created[0]));
    assert_eq!(notes[0].start, MAX_PATTERN_TICKS - TICKS_PER_STEP);
}

#[test]
fn a_note_cannot_be_put_past_the_end_of_the_longest_pattern() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let at = |start: u32| NoteInit {
        start,
        ..note(0, 60)
    };
    let add = |notes: Vec<NoteInit>| Command::AddNotes {
        pattern: PATTERN,
        channel: kick,
        notes,
    };

    // The last tick of the longest pattern is fine, and a note may ring on
    // past the end. One tick further it could never play.
    let ids = add_notes(&mut doc, kick, vec![at(0), at(MAX_PATTERN_TICKS - 1)]);
    assert_invalid(
        fail(&mut doc, add(vec![at(0), at(MAX_PATTERN_TICKS)])),
        "the note would start at step 1025, past the end of the longest pattern, which has 1024 steps",
    );
    assert_invalid(
        fail(&mut doc, add(vec![at(20_000_000)])),
        "past the end of the longest pattern",
    );

    // Moving is held to the same end, in one update or as one of several.
    let to = |start: u32| NotePatch {
        start: Some(start),
        ..NotePatch::default()
    };
    assert_invalid(
        fail(
            &mut doc,
            update_notes(kick, vec![(ids[0], to(MAX_PATTERN_TICKS))]),
        ),
        "past the end of the longest pattern",
    );
    let both = update_notes(kick, vec![(ids[1], to(480)), (ids[0], to(u32::MAX))]);
    assert_invalid(fail(&mut doc, both), "past the end of the longest pattern");
    run(
        &mut doc,
        update_notes(kick, vec![(ids[0], to(MAX_PATTERN_TICKS - 1))]),
    );
}

#[test]
fn a_note_an_older_file_has_past_the_longest_pattern_can_be_changed_but_not_moved_further() {
    // Earlier versions let a paste put notes out here, and such a file
    // still loads.
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let ids = add_notes(&mut doc, kick, vec![note(0, 60), note(240, 60)]);
    let mut project = doc.project().clone();
    project.patterns[0].lanes[0].notes[1].start = MAX_PATTERN_TICKS + 4_800;
    project.check().expect("the file still loads");
    let mut doc = Document::new(project);
    let far = ids[1];

    let patch = |change: fn(&mut NotePatch)| {
        let mut patch = NotePatch::default();
        change(&mut patch);
        update_notes(kick, vec![(far, patch)])
    };
    run(&mut doc, patch(|p| p.velocity = Some(0.25)));
    run(&mut doc, patch(|p| p.length = Some(100)));
    run(&mut doc, patch(|p| p.key = Some(72)));
    assert_invalid(
        fail(
            &mut doc,
            patch(|p| p.start = Some(MAX_PATTERN_TICKS + 4_801)),
        ),
        "past the end of the longest pattern",
    );
    // It can be brought back inside, copied along with its channel, and
    // deleted.
    run(&mut doc, Command::DuplicateChannel { id: kick });
    run(&mut doc, patch(|p| p.start = Some(960)));
    let remove = Command::RemoveNotes {
        pattern: PATTERN,
        channel: kick,
        notes: vec![far],
    };
    run(&mut doc, remove);
}

#[test]
fn add_notes_creates_ids_in_order_and_keeps_the_lane_sorted() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let applied = run(
        &mut doc,
        Command::AddNotes {
            pattern: PATTERN,
            channel: kick,
            notes: vec![
                note(960, 60),
                note(0, 64),
                NoteInit {
                    start: 0,
                    length: 100,
                    key: 60,
                    velocity: Some(0.5),
                    pan: Some(-0.25),
                    expression: None,
                },
                note(0, 60),
            ],
        },
    );
    assert_eq!(applied.label, "Add notes");
    assert_eq!(applied.touched, touched(|t| t.patterns = vec![PATTERN]));
    let ids = &applied.created;
    assert_eq!(ids.len(), 4);
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));

    let notes = lane_notes(&doc, PATTERN, kick);
    let order: Vec<u32> = notes.iter().map(|n| n.id.0).collect();
    // By start, then key, then id.
    assert_eq!(order, [ids[2], ids[3], ids[1], ids[0]]);
    assert_eq!(notes[0].length, 100);
    assert_eq!(notes[0].velocity, 0.5);
    assert_eq!(notes[0].pan, -0.25);
    assert_eq!(notes[1].velocity, DEFAULT_VELOCITY);
    assert_eq!(notes[1].pan, 0.0);

    let one = run(
        &mut doc,
        Command::AddNotes {
            pattern: PATTERN,
            channel: kick,
            notes: vec![note(5, 5)],
        },
    );
    assert_eq!(one.label, "Add note");
}

#[test]
fn add_notes_validates_every_note_before_adding_any() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let add = |notes| Command::AddNotes {
        pattern: PATTERN,
        channel: kick,
        notes,
    };
    let with = |change: fn(&mut NoteInit)| {
        let mut bad = note(0, 60);
        change(&mut bad);
        vec![note(0, 60), bad]
    };
    assert_invalid(
        fail(&mut doc, add(with(|n| n.length = 0))),
        "at least one tick long",
    );
    assert_invalid(fail(&mut doc, add(with(|n| n.key = 128))), "key 128");
    assert_invalid(
        fail(&mut doc, add(with(|n| n.start = u32::MAX))),
        "past the end of the longest pattern",
    );
    assert_invalid(
        fail(
            &mut doc,
            add(with(|n| {
                n.start = 1;
                n.length = u32::MAX;
            })),
        ),
        "past the last tick",
    );
    assert_invalid(
        fail(&mut doc, add(with(|n| n.velocity = Some(f32::NAN)))),
        "not a number",
    );
    assert_invalid(
        fail(&mut doc, add(with(|n| n.pan = Some(f32::NAN)))),
        "not a number",
    );

    run(
        &mut doc,
        add(with(|n| {
            n.velocity = Some(7.0);
            n.pan = Some(-7.0);
            n.key = 127;
        })),
    );
    let notes = lane_notes(&doc, PATTERN, kick);
    assert_eq!(notes[1].velocity, 1.0);
    assert_eq!(notes[1].pan, -1.0);

    let empty = run(&mut doc, add(Vec::new()));
    assert!(empty.touched.is_empty() && empty.created.is_empty());

    let unknown_channel = Command::AddNotes {
        pattern: PATTERN,
        channel: ChannelId(77),
        notes: vec![note(0, 60)],
    };
    assert_eq!(fail(&mut doc, unknown_channel), not_found("channel", 77));
    let unknown_pattern = Command::AddNotes {
        pattern: PatternId(90),
        channel: kick,
        notes: vec![note(0, 60)],
    };
    assert_eq!(fail(&mut doc, unknown_pattern), not_found("pattern", 90));
}

#[test]
fn lanes_are_kept_in_channel_order() {
    let mut doc = document();
    let (a, _) = add_channel(&mut doc, "A");
    let (b, _) = add_channel(&mut doc, "B");
    let (c, _) = add_channel(&mut doc, "C");
    for channel in [c, a, b] {
        add_notes(&mut doc, channel, vec![note(0, 60)]);
    }
    let lanes = &doc.project().pattern(PATTERN).unwrap().lanes;
    let order: Vec<ChannelId> = lanes.iter().map(|lane| lane.channel).collect();
    assert_eq!(order, [a, b, c]);
}

#[test]
fn remove_notes_removes_them_and_drops_an_empty_lane() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let ids = add_notes(
        &mut doc,
        kick,
        vec![note(0, 60), note(240, 60), note(480, 60)],
    );
    let remove = |notes| Command::RemoveNotes {
        pattern: PATTERN,
        channel: kick,
        notes,
    };

    let applied = run(&mut doc, remove(vec![ids[1]]));
    assert_eq!(applied.label, "Delete note");
    assert_eq!(applied.touched, touched(|t| t.patterns = vec![PATTERN]));
    let left: Vec<NoteId> = lane_notes(&doc, PATTERN, kick)
        .iter()
        .map(|n| n.id)
        .collect();
    assert_eq!(left, [ids[0], ids[2]]);

    // One unknown id fails the whole command.
    assert_eq!(
        fail(&mut doc, remove(vec![ids[0], ids[1]])),
        not_found("note", ids[1].0)
    );

    // Naming a note twice removes it once.
    assert_eq!(
        run(&mut doc, remove(vec![ids[0], ids[2], ids[0]])).label,
        "Delete notes"
    );
    assert!(doc.project().pattern(PATTERN).unwrap().lanes.is_empty());

    assert_eq!(
        fail(&mut doc, remove(vec![ids[0]])),
        not_found("note", ids[0].0)
    );
    assert!(run(&mut doc, remove(Vec::new())).touched.is_empty());
}

#[test]
fn notes_of_another_lane_are_not_found() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let (hat, _) = add_channel(&mut doc, "Hat");
    let ids = add_notes(&mut doc, kick, vec![note(0, 60)]);
    add_notes(&mut doc, hat, vec![note(0, 60)]);
    let error = fail(
        &mut doc,
        Command::RemoveNotes {
            pattern: PATTERN,
            channel: hat,
            notes: ids.clone(),
        },
    );
    assert_eq!(error, not_found("note", ids[0].0));
}

fn update_notes(channel: ChannelId, updates: Vec<(NoteId, NotePatch)>) -> Command {
    Command::UpdateNotes {
        pattern: PATTERN,
        channel,
        updates: updates
            .into_iter()
            .map(|(id, patch)| NoteUpdate { id, patch })
            .collect(),
    }
}

#[test]
fn update_notes_changes_notes_and_keeps_the_lane_sorted() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let ids = add_notes(
        &mut doc,
        kick,
        vec![note(0, 60), note(240, 60), note(480, 60)],
    );

    let applied = run(
        &mut doc,
        update_notes(
            kick,
            vec![
                (
                    ids[0],
                    NotePatch {
                        start: Some(960),
                        key: Some(72),
                        ..NotePatch::default()
                    },
                ),
                (
                    ids[2],
                    NotePatch {
                        length: Some(10),
                        velocity: Some(2.0),
                        pan: Some(0.5),
                        ..NotePatch::default()
                    },
                ),
            ],
        ),
    );
    assert_eq!(applied.label, "Move notes");
    assert_eq!(applied.touched, touched(|t| t.patterns = vec![PATTERN]));
    assert!(applied.created.is_empty());
    let notes = lane_notes(&doc, PATTERN, kick);
    let order: Vec<NoteId> = notes.iter().map(|n| n.id).collect();
    assert_eq!(order, [ids[1], ids[2], ids[0]]);
    assert_eq!((notes[2].start, notes[2].key), (960, 72));
    assert_eq!(notes[1].length, 10);
    assert_eq!(notes[1].velocity, 1.0);
    assert_eq!(notes[1].pan, 0.5);
}

#[test]
fn update_notes_labels_say_what_changed() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let ids = add_notes(&mut doc, kick, vec![note(0, 60), note(240, 60)]);
    let cases = [
        (
            NotePatch {
                start: Some(7),
                ..NotePatch::default()
            },
            "Move note",
        ),
        (
            NotePatch {
                key: Some(7),
                ..NotePatch::default()
            },
            "Move note",
        ),
        (
            NotePatch {
                length: Some(7),
                ..NotePatch::default()
            },
            "Resize note",
        ),
        (
            NotePatch {
                velocity: Some(0.7),
                ..NotePatch::default()
            },
            "Change note velocity",
        ),
        (
            NotePatch {
                pan: Some(0.7),
                ..NotePatch::default()
            },
            "Change note pan",
        ),
    ];
    for (patch, expected) in cases {
        let applied = run(&mut doc, update_notes(kick, vec![(ids[0], patch)]));
        assert_eq!(applied.label, expected);
    }
    let resize = NotePatch {
        length: Some(99),
        ..NotePatch::default()
    };
    let applied = run(
        &mut doc,
        update_notes(kick, vec![(ids[0], resize), (ids[1], resize)]),
    );
    assert_eq!(applied.label, "Resize notes");
}

#[test]
fn a_note_dragged_by_its_start_is_resized_not_moved() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let ids = add_notes(&mut doc, kick, vec![note(480, 60), note(960, 64)]);
    let ends = |doc: &Document| -> Vec<u32> {
        let notes = lane_notes(doc, PATTERN, kick);
        notes.iter().map(|note| note.start + note.length).collect()
    };
    let before = ends(&doc);
    let front = |start: u32, length: u32| NotePatch {
        start: Some(start),
        length: Some(length),
        ..NotePatch::default()
    };

    // The start moves and the end stays where it was: 240 ticks at 480
    // become 360 ticks at 360.
    let applied = run(
        &mut doc,
        update_notes(kick, vec![(ids[0], front(360, 360))]),
    );
    assert_eq!(applied.label, "Resize note");
    assert_eq!(ends(&doc), before);
    let both = vec![(ids[0], front(420, 300)), (ids[1], front(900, 300))];
    let applied = run(&mut doc, update_notes(kick, both));
    assert_eq!(applied.label, "Resize notes");
    assert_eq!(ends(&doc), before);

    // With an end that moves too, or a key, it is a move.
    let applied = run(&mut doc, update_notes(kick, vec![(ids[0], front(0, 240))]));
    assert_eq!(applied.label, "Move note");
    let keyed = NotePatch {
        key: Some(61),
        ..front(120, 120)
    };
    let applied = run(&mut doc, update_notes(kick, vec![(ids[0], keyed)]));
    assert_eq!(applied.label, "Move note");
    // One note resized from its front and another moved is a move.
    let mixed = vec![
        (ids[0], front(60, 180)),
        (
            ids[1],
            NotePatch {
                start: Some(1_920),
                ..NotePatch::default()
            },
        ),
    ];
    let applied = run(&mut doc, update_notes(kick, mixed));
    assert_eq!(applied.label, "Move notes");
}

#[test]
fn update_notes_validates_before_changing_anything() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let ids = add_notes(&mut doc, kick, vec![note(0, 60), note(240, 60)]);
    let good = NotePatch {
        start: Some(100),
        ..NotePatch::default()
    };
    let with = |bad: NotePatch| update_notes(kick, vec![(ids[0], good), (ids[1], bad)]);

    let zero = NotePatch {
        length: Some(0),
        ..NotePatch::default()
    };
    assert_invalid(fail(&mut doc, with(zero)), "at least one tick long");
    let high = NotePatch {
        key: Some(200),
        ..NotePatch::default()
    };
    assert_invalid(fail(&mut doc, with(high)), "key 200");
    let far = NotePatch {
        start: Some(u32::MAX),
        ..NotePatch::default()
    };
    assert_invalid(
        fail(&mut doc, with(far)),
        "past the end of the longest pattern",
    );
    let long = NotePatch {
        length: Some(u32::MAX),
        ..NotePatch::default()
    };
    assert_invalid(fail(&mut doc, with(long)), "past the last tick");

    let missing = update_notes(kick, vec![(ids[0], good), (NoteId(4000), good)]);
    assert_eq!(fail(&mut doc, missing), not_found("note", 4000));

    // A patch that sets what is already there changes nothing.
    let same = NotePatch {
        start: Some(0),
        key: Some(60),
        ..NotePatch::default()
    };
    assert!(
        run(&mut doc, update_notes(kick, vec![(ids[0], same)]))
            .touched
            .is_empty()
    );

    // Two updates of one note build on each other.
    let longer = NotePatch {
        length: Some(500),
        ..NotePatch::default()
    };
    run(
        &mut doc,
        update_notes(kick, vec![(ids[0], good), (ids[0], longer)]),
    );
    let moved = lane_notes(&doc, PATTERN, kick)
        .into_iter()
        .find(|n| n.id == ids[0])
        .unwrap();
    assert_eq!((moved.start, moved.length), (100, 500));
}

#[test]
fn clear_lane_removes_every_note_of_the_channel() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let (hat, _) = add_channel(&mut doc, "Hat");
    add_notes(&mut doc, kick, vec![note(0, 60), note(240, 60)]);
    add_notes(&mut doc, hat, vec![note(0, 60)]);
    let clear = |pattern, channel| Command::ClearLane { pattern, channel };

    let applied = run(&mut doc, clear(PATTERN, kick));
    assert_eq!(applied.label, "Clear notes");
    assert_eq!(applied.touched, touched(|t| t.patterns = vec![PATTERN]));
    assert!(lane_notes(&doc, PATTERN, kick).is_empty());
    assert_eq!(lane_notes(&doc, PATTERN, hat).len(), 1);

    assert!(run(&mut doc, clear(PATTERN, kick)).touched.is_empty());
    assert_eq!(
        fail(&mut doc, clear(PATTERN, ChannelId(77))),
        not_found("channel", 77)
    );
    assert_eq!(
        fail(&mut doc, clear(PatternId(90), kick)),
        not_found("pattern", 90)
    );
}

// Mixer

#[test]
fn add_mixer_track_adds_a_track_routed_to_the_master() {
    let mut doc = document();
    let applied = run(&mut doc, Command::AddMixerTrack { name: None });
    assert_eq!(applied.label, "Add mixer track");
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
    let added = track(&doc, TrackId(applied.created[0]));
    assert_eq!(added.name, "Insert 1");
    assert_eq!(added.color, PALETTE[0]);
    assert_eq!(added.volume, 1.0);
    assert_eq!(added.pan, 0.0);
    assert_eq!(added.output, Some(TrackId::MASTER));

    let named = run(
        &mut doc,
        Command::AddMixerTrack {
            name: Some("Insert 3".to_owned()),
        },
    );
    assert_eq!(track(&doc, TrackId(named.created[0])).name, "Insert 3");
    let next = add_mixer_track(&mut doc);
    assert_eq!(track(&doc, next).name, "Insert 4");
}

#[test]
fn the_mixer_holds_a_limited_number_of_tracks() {
    let mut doc = document();
    for _ in 1..MAX_MIXER_SIGNAL_TRACKS {
        doc.dispatch(Command::AddMixerTrack { name: None }, None)
            .unwrap();
    }
    doc.project().check().unwrap();
    assert_eq!(doc.project().mixer.tracks.len(), MAX_MIXER_SIGNAL_TRACKS);
    assert_invalid(
        fail(&mut doc, Command::AddMixerTrack { name: None }),
        "the mixer is full",
    );
}

#[test]
fn remove_mixer_track_reroutes_to_the_master_and_drops_sends() {
    let mut doc = document();
    let (kick, bus) = add_channel(&mut doc, "Kick");
    let (hat, hat_track) = add_channel(&mut doc, "Hat");
    let feeder = add_mixer_track(&mut doc);
    let sender = add_mixer_track(&mut doc);
    run(
        &mut doc,
        Command::SetTrackOutput {
            id: feeder,
            output: Some(bus),
        },
    );
    for (to, gain) in [(bus, 0.5), (hat_track, 0.25)] {
        run(
            &mut doc,
            Command::SetSend {
                from: sender,
                to,
                gain: Some(gain),
            },
        );
    }

    let applied = run(&mut doc, Command::RemoveMixerTrack { id: bus });
    assert_eq!(applied.label, "Delete mixer track");
    assert_eq!(
        applied.touched,
        touched(|t| {
            t.channels = true;
            t.mixer = true;
        })
    );
    assert!(doc.project().mixer.track(bus).is_none());
    assert_eq!(
        doc.project().channel(kick).unwrap().mixer_track,
        TrackId::MASTER
    );
    assert_eq!(doc.project().channel(hat).unwrap().mixer_track, hat_track);
    assert_eq!(track(&doc, feeder).output, Some(TrackId::MASTER));
    assert_eq!(
        track(&doc, sender).sends,
        [Send {
            target: hat_track,
            gain: 0.25
        }]
    );

    // A track nothing is routed to touches the mixer only.
    let applied = run(&mut doc, Command::RemoveMixerTrack { id: feeder });
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
}

#[test]
fn the_master_track_cannot_be_removed() {
    let mut doc = document();
    assert_invalid(
        fail(
            &mut doc,
            Command::RemoveMixerTrack {
                id: TrackId::MASTER,
            },
        ),
        "master track cannot be deleted",
    );
    assert_eq!(
        fail(&mut doc, Command::RemoveMixerTrack { id: TrackId(88) }),
        not_found("mixer track", 88)
    );
}

#[test]
fn update_mixer_track_changes_each_field() {
    let mut doc = document();
    let insert = add_mixer_track(&mut doc);
    let update = |id, patch| Command::UpdateMixerTrack { id, patch };
    let applied = run(
        &mut doc,
        update(
            insert,
            MixerTrackPatch {
                name: Some("Drums".to_owned()),
                color: Some(0x0000FF),
                volume: Some(0.5),
                pan: Some(0.25),
                muted: Some(true),
                solo: Some(true),
                recording: None,
                ..MixerTrackPatch::default()
            },
        ),
    );
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
    assert_eq!(applied.label, "Change mixer track");
    let changed = track(&doc, insert);
    assert_eq!(changed.name, "Drums");
    assert_eq!(changed.color, 0x0000FF);
    assert_eq!(changed.volume, 0.5);
    assert_eq!(changed.pan, 0.25);
    assert!(changed.muted && changed.solo);

    let volume = |id, volume| {
        update(
            id,
            MixerTrackPatch {
                volume: Some(volume),
                ..MixerTrackPatch::default()
            },
        )
    };
    let applied = run(&mut doc, volume(TrackId::MASTER, 5.0));
    assert_eq!(applied.label, "Change mixer track volume");
    assert_eq!(track(&doc, TrackId::MASTER).volume, MAX_GAIN);
    assert_invalid(fail(&mut doc, volume(insert, f32::NAN)), "not a number");

    let color = update(
        insert,
        MixerTrackPatch {
            color: Some(0xFF00_0000),
            ..MixerTrackPatch::default()
        },
    );
    assert_invalid(fail(&mut doc, color), "0xRRGGBB");
    assert_eq!(
        fail(&mut doc, update(TrackId(88), MixerTrackPatch::default())),
        not_found("mixer track", 88)
    );
}

fn output(id: TrackId, output: Option<TrackId>) -> Command {
    Command::SetTrackOutput { id, output }
}

fn send(from: TrackId, to: TrackId, gain: Option<f32>) -> Command {
    Command::SetSend { from, to, gain }
}

#[test]
fn set_track_output_routes_a_track() {
    let mut doc = document();
    let a = add_mixer_track(&mut doc);
    let b = add_mixer_track(&mut doc);

    let applied = run(&mut doc, output(a, Some(b)));
    assert_eq!(applied.label, "Route mixer track");
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
    assert_eq!(track(&doc, a).output, Some(b));

    run(&mut doc, output(a, None));
    assert_eq!(track(&doc, a).output, None);

    assert_invalid(
        fail(&mut doc, output(TrackId::MASTER, Some(a))),
        "master track's output",
    );
    assert_invalid(
        fail(&mut doc, output(TrackId::MASTER, None)),
        "master track's output",
    );
    assert_eq!(
        fail(&mut doc, output(a, Some(TrackId(88)))),
        not_found("mixer track", 88)
    );
    assert_eq!(
        fail(&mut doc, output(TrackId(88), None)),
        not_found("mixer track", 88)
    );
}

#[test]
fn routing_cannot_loop() {
    let mut doc = document();
    let a = add_mixer_track(&mut doc);
    let b = add_mixer_track(&mut doc);
    let c = add_mixer_track(&mut doc);
    let looped = "loop back on itself";

    assert_invalid(fail(&mut doc, output(a, Some(a))), looped);
    assert_invalid(fail(&mut doc, send(a, a, Some(1.0))), looped);

    // a -> b -> c -> master.
    run(&mut doc, output(a, Some(b)));
    run(&mut doc, output(b, Some(c)));
    assert_invalid(fail(&mut doc, output(c, Some(a))), looped);
    assert_invalid(fail(&mut doc, output(b, Some(a))), looped);
    assert_invalid(fail(&mut doc, send(c, a, Some(1.0))), looped);
    assert_invalid(fail(&mut doc, send(c, b, Some(1.0))), looped);

    // A loop through a send counts too: c sends to nothing yet, so a send
    // from a to c is fine, and then c may not output to a.
    run(&mut doc, output(a, None));
    run(&mut doc, send(a, c, Some(1.0)));
    assert_invalid(fail(&mut doc, output(c, Some(a))), looped);

    // Routing that merely meets again is not a loop.
    run(&mut doc, send(a, b, Some(1.0)));
    run(&mut doc, send(a, TrackId::MASTER, Some(1.0)));
    run(&mut doc, output(a, Some(c)));
}

#[test]
fn set_send_adds_changes_and_removes_a_send() {
    let mut doc = document();
    let a = add_mixer_track(&mut doc);
    let b = add_mixer_track(&mut doc);

    let applied = run(&mut doc, send(a, b, Some(0.5)));
    assert_eq!(applied.label, "Add send");
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
    assert_eq!(
        track(&doc, a).sends,
        [Send {
            target: b,
            gain: 0.5
        }]
    );

    let applied = run(&mut doc, send(a, b, Some(9.0)));
    assert_eq!(applied.label, "Change send level");
    assert_eq!(
        track(&doc, a).sends,
        [Send {
            target: b,
            gain: MAX_GAIN
        }]
    );

    let applied = run(&mut doc, send(a, b, None));
    assert_eq!(applied.label, "Remove send");
    assert!(track(&doc, a).sends.is_empty());
    assert!(run(&mut doc, send(a, b, None)).touched.is_empty());

    assert_invalid(fail(&mut doc, send(a, b, Some(f32::NAN))), "not a number");
    assert_invalid(
        fail(&mut doc, send(TrackId::MASTER, a, Some(1.0))),
        "master track cannot send",
    );
    assert_eq!(
        fail(&mut doc, send(a, TrackId(88), Some(1.0))),
        not_found("mixer track", 88)
    );
    assert_eq!(
        fail(&mut doc, send(TrackId(88), a, None)),
        not_found("mixer track", 88)
    );
}

// Playlist

#[test]
fn playlist_tracks_are_added_renamed_and_muted() {
    let mut doc = document();
    let applied = run(
        &mut doc,
        Command::AddPlaylistTrack {
            name: None,
            index: None,
        },
    );
    assert_eq!(applied.label, "Add playlist track");
    assert_eq!(applied.touched, touched(|t| t.playlist = true));
    let id = PlaylistTrackId(applied.created[0]);
    assert_eq!(
        doc.project().playlist.tracks,
        [PlaylistTrack {
            id,
            name: "Track 1".to_owned(),
            muted: false,
            solo: false,
            color: 0,
            height: 0,
        }]
    );
    let second = add_playlist_track(&mut doc);
    assert_eq!(doc.project().playlist.tracks[1].name, "Track 2");

    let update = |id, patch| Command::UpdatePlaylistTrack { id, patch };
    let applied = run(
        &mut doc,
        update(
            second,
            PlaylistTrackPatch {
                name: Some("Drums".to_owned()),
                muted: None,
                solo: None,
                color: None,
                height: None,
            },
        ),
    );
    assert_eq!(applied.label, "Rename playlist track");
    assert_eq!(applied.touched, touched(|t| t.playlist = true));
    let applied = run(
        &mut doc,
        update(
            second,
            PlaylistTrackPatch {
                name: None,
                muted: Some(true),
                solo: None,
                color: None,
                height: None,
            },
        ),
    );
    assert_eq!(applied.label, "Mute playlist track");
    assert_eq!(
        doc.project().playlist.tracks[1],
        PlaylistTrack {
            id: second,
            name: "Drums".to_owned(),
            muted: true,
            solo: false,
            color: 0,
            height: 0,
        }
    );
    assert_eq!(
        fail(
            &mut doc,
            update(PlaylistTrackId(55), PlaylistTrackPatch::default())
        ),
        not_found("playlist track", 55)
    );
}

#[test]
fn playlist_tracks_are_added_at_an_index_and_moved() {
    let mut doc = document();
    let order = |doc: &Document| -> Vec<PlaylistTrackId> {
        doc.project().playlist.tracks.iter().map(|t| t.id).collect()
    };
    let add_at = |doc: &mut Document, index: u32| {
        let applied = run(
            doc,
            Command::AddPlaylistTrack {
                name: None,
                index: Some(index),
            },
        );
        PlaylistTrackId(applied.created[0])
    };
    let a = add_playlist_track(&mut doc);
    let b = add_playlist_track(&mut doc);
    // Above the first, between the two, and past the end, which is the end.
    let top = add_at(&mut doc, 0);
    assert_eq!(order(&doc), [top, a, b]);
    let middle = add_at(&mut doc, 2);
    assert_eq!(order(&doc), [top, a, middle, b]);
    let last = add_at(&mut doc, 99);
    assert_eq!(order(&doc), [top, a, middle, b, last]);
    // A track keeps the name its number in line gave it.
    assert_eq!(doc.project().playlist.tracks[0].name, "Track 3");

    // A track moves with its clips, which name it by id.
    let clip = add_clip(&mut doc, b, PATTERN, 0);
    let moved = |id, index| Command::MovePlaylistTrack { id, index };
    let applied = run(&mut doc, moved(b, 0));
    assert_eq!(applied.label, "Move playlist track");
    assert_eq!(applied.touched, touched(|t| t.playlist = true));
    assert!(applied.created.is_empty());
    assert_eq!(order(&doc), [b, top, a, middle, last]);
    assert_eq!(doc.project().playlist.clips[0].id, clip);
    assert_eq!(doc.project().playlist.clips[0].track, b);
    run(&mut doc, moved(b, 99));
    assert_eq!(order(&doc), [top, a, middle, last, b]);
    // Moving a track to where it is changes nothing.
    let history = doc.history();
    let applied = run(&mut doc, moved(b, 4));
    assert!(applied.touched.is_empty());
    assert_eq!(doc.history(), history);
    assert_eq!(
        fail(&mut doc, moved(PlaylistTrackId(55), 0)),
        not_found("playlist track", 55)
    );

    // A drag across several places is one undo step.
    let steps = doc.history().entries.len();
    for index in [3, 2, 1] {
        doc.dispatch(moved(b, index), Some(4)).unwrap();
    }
    assert_eq!(doc.history().entries.len(), steps + 1);
    assert_eq!(order(&doc), [top, b, a, middle, last]);
    doc.undo().unwrap();
    assert_eq!(order(&doc), [top, a, middle, last, b]);
}

#[test]
fn remove_playlist_track_takes_its_clips() {
    let mut doc = document();
    let first = add_playlist_track(&mut doc);
    let second = add_playlist_track(&mut doc);
    add_clip(&mut doc, first, PATTERN, 0);
    let kept = add_clip(&mut doc, second, PATTERN, 0);
    add_clip(&mut doc, first, PATTERN, 3840);

    let applied = run(&mut doc, Command::RemovePlaylistTrack { id: first });
    assert_eq!(applied.label, "Delete playlist track");
    assert_eq!(applied.touched, touched(|t| t.playlist = true));
    let clips: Vec<ClipId> = doc.project().playlist.clips.iter().map(|c| c.id).collect();
    assert_eq!(clips, [kept]);
    assert_eq!(doc.project().playlist.tracks.len(), 1);
    assert_eq!(
        fail(&mut doc, Command::RemovePlaylistTrack { id: first }),
        not_found("playlist track", first.0)
    );
}

fn clip(track: PlaylistTrackId, start: u32, length: Option<u32>) -> ClipInit {
    ClipInit {
        track,
        start,
        length,
        offset: None,
        muted: None,
        content: ClipContent::Pattern { pattern: PATTERN },
    }
}

#[test]
fn add_clips_creates_ids_in_order_and_keeps_clips_sorted() {
    let mut doc = document();
    let lane = add_playlist_track(&mut doc);
    let applied = run(
        &mut doc,
        Command::AddClips {
            clips: vec![
                clip(lane, 7680, None),
                clip(lane, 0, Some(100)),
                clip(lane, 0, None),
            ],
        },
    );
    assert_eq!(applied.label, "Add clips");
    assert_eq!(applied.touched, touched(|t| t.playlist = true));
    let ids = &applied.created;
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));

    let clips = &doc.project().playlist.clips;
    let order: Vec<u32> = clips.iter().map(|c| c.id.0).collect();
    assert_eq!(order, [ids[1], ids[2], ids[0]]);
    assert_eq!(clips[0].length, 100);
    // One pass of a 16-step pattern.
    assert_eq!(clips[1].length, 16 * TICKS_PER_STEP);
    assert_eq!(clips[1].offset, 0);
    assert!(!clips[1].muted);
    assert_eq!(clips[1].track, lane);
    assert_eq!(clips[1].content, ClipContent::Pattern { pattern: PATTERN });
}

#[test]
fn a_clip_can_be_added_with_an_offset_and_muted() {
    let mut doc = document();
    let lane = add_playlist_track(&mut doc);
    let init = ClipInit {
        offset: Some(480),
        muted: Some(true),
        ..clip(lane, 960, Some(1_000))
    };
    let applied = run(&mut doc, Command::AddClips { clips: vec![init] });
    let added = &doc.project().playlist.clips[0];
    assert_eq!(added.id, ClipId(applied.created[0]));
    assert_eq!((added.start, added.length, added.offset), (960, 1_000, 480));
    assert!(added.muted);

    // Both are optional on the wire, as they were not there before.
    let json = r#"{ "type": "addClips", "clips": [
        { "track": 2, "start": 0, "content": { "type": "pattern", "pattern": 1 } } ] }"#;
    let command: Command = serde_json::from_str(json).unwrap();
    assert_eq!(
        command,
        Command::AddClips {
            clips: vec![clip(lane, 0, None)]
        }
    );
    let deep = ClipInit {
        offset: Some(MAX_SONG_TICKS + 1),
        ..clip(lane, 0, None)
    };
    assert_invalid(
        fail(&mut doc, Command::AddClips { clips: vec![deep] }),
        "further into what it plays than the longest song lasts",
    );
}

/// An audio clip of `sample` on `track`, at unity and with no fades.
fn audio(sample: SampleId, mixer_track: TrackId) -> ClipContent {
    ClipContent::Audio {
        sample,
        mixer_track,
        output: Default::default(),
        normalize: false,
        gain: 1.0,
        pan: 0.0,
        fade_in: 0,
        fade_out: 0,
        reverse: false,
        pitch: 0.0,

        stretch: Default::default(),
    }
}

fn audio_clip(track: PlaylistTrackId, start: u32, length: u32, content: ClipContent) -> ClipInit {
    ClipInit {
        track,
        start,
        length: Some(length),
        offset: None,
        muted: None,
        content,
    }
}

fn add_audio_clip(doc: &mut Document, lane: PlaylistTrackId, content: ClipContent) -> ClipId {
    let clips = vec![audio_clip(lane, 0, 3_840, content)];
    ClipId(run(doc, Command::AddClips { clips }).created[0])
}

fn clip_of(doc: &Document, id: ClipId) -> Clip {
    let clips = &doc.project().playlist.clips;
    let clip = clips.iter().find(|clip| clip.id == id);
    clip.expect("the clip exists").clone()
}

#[test]
fn an_audio_clip_is_added_with_its_values_brought_into_range() {
    let mut doc = document();
    let lane = add_playlist_track(&mut doc);
    let vocal = add_sample(&mut doc, "vocal");
    let bus = add_mixer_track(&mut doc);
    let content = ClipContent::Audio {
        sample: vocal,
        mixer_track: bus,
        output: Default::default(),
        normalize: false,
        gain: 0.5,
        pan: -0.25,
        fade_in: 240,
        fade_out: 480,
        reverse: true,
        pitch: -3.5,

        stretch: Default::default(),
    };
    let init = ClipInit {
        offset: Some(120),
        ..audio_clip(lane, 960, 7_680, content.clone())
    };
    let applied = run(&mut doc, Command::AddClips { clips: vec![init] });
    assert_eq!(applied.label, "Add clip");
    assert_eq!(applied.touched, touched(|t| t.playlist = true));
    let id = ClipId(applied.created[0]);
    assert_eq!(
        clip_of(&doc, id),
        Clip {
            id,
            track: lane,
            start: 960,
            length: 7_680,
            offset: 120,
            muted: false,
            content,
        }
    );

    // What a control sends past the end of its travel is brought back.
    let wild = ClipContent::Audio {
        sample: vocal,
        mixer_track: TrackId::MASTER,
        output: Default::default(),
        normalize: false,
        gain: 9.0,
        pan: -4.0,
        fade_in: u32::MAX,
        fade_out: MAX_SONG_TICKS + 1,
        reverse: false,
        pitch: 100.0,

        stretch: Default::default(),
    };
    let tamed = add_audio_clip(&mut doc, lane, wild);
    assert_eq!(
        clip_of(&doc, tamed).content,
        ClipContent::Audio {
            sample: vocal,
            mixer_track: TrackId::MASTER,
            output: Default::default(),
            normalize: false,
            gain: MAX_GAIN,
            pan: -1.0,
            fade_in: MAX_SONG_TICKS,
            fade_out: MAX_SONG_TICKS,
            reverse: false,
            pitch: MAX_TUNE_SEMITONES,

            stretch: Default::default(),
        }
    );

    // It is stored the way every clip is: tagged with its type.
    let json = serde_json::to_value(clip_of(&doc, id)).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "id": id.0, "track": lane.0, "start": 960, "length": 7680, "offset": 120,
            "muted": false,
            "content": {
                "type": "audio", "sample": vocal.0, "mixerTrack": bus.0, "gain": 0.5,
                "pan": -0.25, "fadeIn": 240, "fadeOut": 480, "reverse": true, "pitch": -3.5
            }
        })
    );
}

#[test]
fn an_audio_clip_needs_a_length_a_sample_and_a_mixer_track() {
    let mut doc = document();
    let lane = add_playlist_track(&mut doc);
    let vocal = add_sample(&mut doc, "vocal");
    let add = |content: ClipContent, length: Option<u32>| Command::AddClips {
        clips: vec![
            clip(lane, 0, None),
            ClipInit {
                length,
                ..audio_clip(lane, 0, 1, content)
            },
        ],
    };
    let good = audio(vocal, TrackId::MASTER);
    assert_invalid(
        fail(&mut doc, add(good.clone(), None)),
        "an audio clip has to be given a length",
    );
    assert_invalid(
        fail(&mut doc, add(good.clone(), Some(0))),
        "at least one tick long",
    );
    assert_eq!(
        fail(
            &mut doc,
            add(audio(SampleId(77), TrackId::MASTER), Some(960))
        ),
        not_found("sample", 77)
    );
    assert_eq!(
        fail(&mut doc, add(audio(vocal, TrackId(88)), Some(960))),
        not_found("mixer track", 88)
    );
    let with = |change: fn(&mut f32, &mut f32, &mut f32)| {
        let mut content = audio(vocal, TrackId::MASTER);
        if let ClipContent::Audio {
            gain, pan, pitch, ..
        } = &mut content
        {
            change(gain, pan, pitch);
        }
        add(content, Some(960))
    };
    assert_invalid(
        fail(&mut doc, with(|gain, _, _| *gain = f32::NAN)),
        "the clip gain is not a number",
    );
    assert_invalid(
        fail(&mut doc, with(|_, pan, _| *pan = f32::NAN)),
        "the pan is not a number",
    );
    assert_invalid(
        fail(&mut doc, with(|_, _, pitch| *pitch = f32::NAN)),
        "the pitch is not a number",
    );
    run(&mut doc, add(good, Some(960)));
}

#[test]
fn update_audio_clips_changes_what_is_particular_to_audio() {
    let mut doc = document();
    let lane = add_playlist_track(&mut doc);
    let vocal = add_sample(&mut doc, "vocal");
    let bus = add_mixer_track(&mut doc);
    let a = add_audio_clip(&mut doc, lane, audio(vocal, TrackId::MASTER));
    let b = add_audio_clip(&mut doc, lane, audio(vocal, TrackId::MASTER));
    let pattern_clip = add_clip(&mut doc, lane, PATTERN, 0);
    let update = |updates: Vec<(ClipId, AudioClipPatch)>| Command::UpdateAudioClips {
        updates: updates
            .into_iter()
            .map(|(id, patch)| AudioClipUpdate { id, patch })
            .collect(),
    };
    let patch = |change: fn(&mut AudioClipPatch)| {
        let mut patch = AudioClipPatch::default();
        change(&mut patch);
        patch
    };

    type Change = fn(&mut AudioClipPatch);
    let cases: [(Change, &str); 7] = [
        (|p| p.gain = Some(0.5), "Change clip gain"),
        (|p| p.pan = Some(0.5), "Change clip pan"),
        (|p| p.fade_in = Some(240), "Change clip fade"),
        (|p| p.fade_out = Some(480), "Change clip fade"),
        (|p| p.reverse = Some(true), "Reverse clip"),
        (|p| p.pitch = Some(7.0), "Change clip pitch"),
        (|p| p.mixer_track = Some(TrackId(4)), "Route clip"),
    ];
    assert_eq!(bus, TrackId(4));
    for (change, expected) in cases {
        let applied = run(&mut doc, update(vec![(a, patch(change))]));
        assert_eq!(applied.label, expected);
        assert_eq!(applied.touched, touched(|t| t.playlist = true));
        assert!(applied.created.is_empty());
    }
    assert_eq!(
        clip_of(&doc, a).content,
        ClipContent::Audio {
            sample: vocal,
            mixer_track: bus,
            output: Default::default(),
            normalize: false,
            gain: 0.5,
            pan: 0.5,
            fade_in: 240,
            fade_out: 480,
            reverse: true,
            pitch: 7.0,

            stretch: Default::default(),
        }
    );
    // The other clip, and where the clip sits, are as they were.
    assert_eq!(clip_of(&doc, b).content, audio(vocal, TrackId::MASTER));
    assert_eq!(clip_of(&doc, a).length, 3_840);

    // Several at once, one of them twice, with values out of range.
    let both = update(vec![
        (a, patch(|p| p.gain = Some(7.0))),
        (b, patch(|p| p.pitch = Some(-90.0))),
        (a, patch(|p| p.pan = Some(-7.0))),
    ]);
    let applied = run(&mut doc, both);
    assert_eq!(applied.label, "Change audio clips");
    let ClipContent::Audio { gain, pan, .. } = clip_of(&doc, a).content else {
        panic!("not an audio clip");
    };
    assert_eq!((gain, pan), (MAX_GAIN, -1.0));
    let ClipContent::Audio { pitch, .. } = clip_of(&doc, b).content else {
        panic!("not an audio clip");
    };
    assert_eq!(pitch, -MAX_TUNE_SEMITONES);

    // A patch that sets what is there changes nothing.
    let history = doc.history();
    let applied = run(&mut doc, update(vec![(a, patch(|p| p.gain = Some(2.0)))]));
    assert!(applied.touched.is_empty());
    assert_eq!(doc.history(), history);

    let good = patch(|p| p.gain = Some(0.1));
    assert_invalid(
        fail(&mut doc, update(vec![(a, good), (pattern_clip, good)])),
        &format!("clip {} is not an audio clip", pattern_clip.0),
    );
    assert_eq!(
        fail(&mut doc, update(vec![(a, good), (ClipId(900), good)])),
        not_found("clip", 900)
    );
    assert_eq!(
        fail(
            &mut doc,
            update(vec![(a, patch(|p| p.mixer_track = Some(TrackId(88))))])
        ),
        not_found("mixer track", 88)
    );
    assert_invalid(
        fail(
            &mut doc,
            update(vec![(a, good), (b, patch(|p| p.gain = Some(f32::NAN)))]),
        ),
        "the clip gain is not a number",
    );

    // A fader drag on a clip is one undo step.
    let before = doc.project().clone();
    let steps = doc.history().entries.len();
    for value in [0.2, 0.4, 0.6] {
        let drag = AudioClipPatch {
            gain: Some(value),
            ..AudioClipPatch::default()
        };
        doc.dispatch(update(vec![(a, drag)]), Some(5)).unwrap();
    }
    assert_eq!(doc.history().entries.len(), steps + 1);
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
}

#[test]
fn a_sample_an_audio_clip_plays_cannot_be_removed() {
    let mut doc = document();
    let lane = add_playlist_track(&mut doc);
    run(
        &mut doc,
        Command::UpdatePlaylistTrack {
            id: lane,
            patch: PlaylistTrackPatch {
                name: Some("Vocals".to_owned()),
                muted: None,
                solo: None,
                color: None,
                height: None,
            },
        },
    );
    let vocal = add_sample(&mut doc, "vocal");
    let clip = add_audio_clip(&mut doc, lane, audio(vocal, TrackId::MASTER));
    assert_invalid(
        fail(&mut doc, Command::RemoveSample { id: vocal }),
        "the sample is still used by an audio clip on the playlist track \"Vocals\"",
    );
    // A muted clip still uses it.
    let mute = ClipPatch {
        muted: Some(true),
        ..ClipPatch::default()
    };
    run(&mut doc, update_clips(vec![(clip, mute)]));
    assert_invalid(
        fail(&mut doc, Command::RemoveSample { id: vocal }),
        "still used by an audio clip",
    );
    run(&mut doc, Command::RemoveClips { clips: vec![clip] });
    run(&mut doc, Command::RemoveSample { id: vocal });
}

#[test]
fn removing_a_mixer_track_sends_its_audio_clips_to_the_master() {
    let mut doc = document();
    let lane = add_playlist_track(&mut doc);
    let vocal = add_sample(&mut doc, "vocal");
    let bus = add_mixer_track(&mut doc);
    let other = add_mixer_track(&mut doc);
    let on_bus = add_audio_clip(&mut doc, lane, audio(vocal, bus));
    let on_other = add_audio_clip(&mut doc, lane, audio(vocal, other));
    let pattern_clip = add_clip(&mut doc, lane, PATTERN, 0);
    let before = doc.project().clone();

    let applied = run(&mut doc, Command::RemoveMixerTrack { id: bus });
    assert_eq!(
        applied.touched,
        touched(|t| {
            t.mixer = true;
            t.playlist = true;
        })
    );
    assert_eq!(clip_of(&doc, on_bus).content, audio(vocal, TrackId::MASTER));
    assert_eq!(clip_of(&doc, on_other).content, audio(vocal, other));
    assert_eq!(
        clip_of(&doc, pattern_clip),
        before
            .playlist
            .clips
            .iter()
            .find(|c| c.id == pattern_clip)
            .unwrap()
            .clone()
    );
    // One undo puts the track back, and the clip back on it.
    doc.undo().unwrap();
    assert!(same_content(doc.project(), &before));

    // A track no clip plays into leaves the playlist alone.
    let spare = add_mixer_track(&mut doc);
    let applied = run(&mut doc, Command::RemoveMixerTrack { id: spare });
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
}

#[test]
fn check_names_each_broken_rule_of_an_audio_clip() {
    let mut doc = document();
    let lane = add_playlist_track(&mut doc);
    let vocal = add_sample(&mut doc, "vocal");
    add_audio_clip(&mut doc, lane, audio(vocal, TrackId::MASTER));
    let valid = doc.project().clone();
    valid.check().unwrap();

    type Fields<'a> = (
        &'a mut SampleId,
        &'a mut TrackId,
        &'a mut f32,
        &'a mut f32,
        &'a mut f32,
        &'a mut u32,
        &'a mut u32,
    );
    /// The sample, mixer track, gain, pan, pitch and fades of the clip.
    fn fields(project: &mut Project) -> Fields<'_> {
        let ClipContent::Audio {
            sample,
            mixer_track,
            gain,
            pan,
            fade_in,
            fade_out,
            pitch,
            ..
        } = &mut project.playlist.clips[0].content
        else {
            panic!("not an audio clip");
        };
        (sample, mixer_track, gain, pan, pitch, fade_in, fade_out)
    }
    let cases: [(&str, Damage); 9] = [
        ("plays sample 999", |p| *fields(p).0 = SampleId(999)),
        ("plays into mixer track 999", |p| {
            *fields(p).1 = TrackId(999)
        }),
        ("has gain 2.5", |p| *fields(p).2 = 2.5),
        ("has gain NaN", |p| *fields(p).2 = f32::NAN),
        ("has pan -1.5", |p| *fields(p).3 = -1.5),
        ("is pitched by 49 semitones", |p| *fields(p).4 = 49.0),
        ("is pitched by NaN semitones", |p| *fields(p).4 = f32::NAN),
        ("a fade longer than the longest song", |p| {
            *fields(p).5 = MAX_SONG_TICKS + 1;
        }),
        ("a fade longer than the longest song", |p| {
            *fields(p).6 = u32::MAX;
        }),
    ];
    for (index, (words, damage)) in cases.into_iter().enumerate() {
        let mut project = valid.clone();
        damage(&mut project);
        match project.check() {
            Ok(()) => panic!("case {index} (\"{words}\") passed the check"),
            Err(problem) => assert!(
                problem.contains(words),
                "case {index}: \"{problem}\" lacks \"{words}\""
            ),
        }
    }
}

#[test]
fn add_clips_validates_every_clip_before_adding_any() {
    let mut doc = document();
    let lane = add_playlist_track(&mut doc);
    let add = |bad: ClipInit| Command::AddClips {
        clips: vec![clip(lane, 0, None), bad],
    };
    assert_invalid(
        fail(&mut doc, add(clip(lane, 0, Some(0)))),
        "at least one tick long",
    );
    assert_invalid(
        fail(&mut doc, add(clip(lane, u32::MAX, None))),
        "the clip would end past the end of the longest song, which has 1000000 beats",
    );
    // A clip may end on the last tick of the longest song and no later.
    assert_invalid(
        fail(&mut doc, add(clip(lane, MAX_SONG_TICKS - 99, Some(100)))),
        "past the end of the longest song",
    );
    assert_invalid(
        fail(&mut doc, add(clip(lane, 0, Some(MAX_SONG_TICKS + 1)))),
        "past the end of the longest song",
    );
    assert_invalid(
        fail(&mut doc, add(clip(lane, u32::MAX, Some(u32::MAX)))),
        "past the end of the longest song",
    );
    run(
        &mut doc,
        Command::AddClips {
            clips: vec![clip(lane, MAX_SONG_TICKS - 100, Some(100))],
        },
    );
    assert_eq!(
        fail(&mut doc, add(clip(PlaylistTrackId(55), 0, None))),
        not_found("playlist track", 55)
    );
    let unknown_pattern = ClipInit {
        content: ClipContent::Pattern {
            pattern: PatternId(90),
        },
        ..clip(lane, 0, None)
    };
    assert_eq!(
        fail(&mut doc, add(unknown_pattern)),
        not_found("pattern", 90)
    );
    assert!(
        run(&mut doc, Command::AddClips { clips: Vec::new() })
            .touched
            .is_empty()
    );
}

#[test]
fn remove_clips_removes_them() {
    let mut doc = document();
    let lane = add_playlist_track(&mut doc);
    let a = add_clip(&mut doc, lane, PATTERN, 0);
    let b = add_clip(&mut doc, lane, PATTERN, 3840);
    let remove = |clips| Command::RemoveClips { clips };

    assert_eq!(
        fail(&mut doc, remove(vec![a, ClipId(600)])),
        not_found("clip", 600)
    );
    let applied = run(&mut doc, remove(vec![a]));
    assert_eq!(applied.label, "Delete clip");
    assert_eq!(applied.touched, touched(|t| t.playlist = true));
    let clips: Vec<ClipId> = doc.project().playlist.clips.iter().map(|c| c.id).collect();
    assert_eq!(clips, [b]);
    assert_eq!(fail(&mut doc, remove(vec![a])), not_found("clip", a.0));
}

fn update_clips(updates: Vec<(ClipId, ClipPatch)>) -> Command {
    Command::UpdateClips {
        updates: updates
            .into_iter()
            .map(|(id, patch)| ClipUpdate { id, patch })
            .collect(),
    }
}

#[test]
fn update_clips_changes_clips_and_keeps_them_sorted() {
    let mut doc = document();
    let first = add_playlist_track(&mut doc);
    let second = add_playlist_track(&mut doc);
    let a = add_clip(&mut doc, first, PATTERN, 0);
    let b = add_clip(&mut doc, first, PATTERN, 3840);

    let applied = run(
        &mut doc,
        update_clips(vec![(
            a,
            ClipPatch {
                track: Some(second),
                start: Some(9000),
                length: Some(50),
                offset: Some(7),
                muted: Some(true),
            },
        )]),
    );
    assert_eq!(applied.label, "Move clip");
    assert_eq!(applied.touched, touched(|t| t.playlist = true));
    let clips = &doc.project().playlist.clips;
    let order: Vec<ClipId> = clips.iter().map(|c| c.id).collect();
    assert_eq!(order, [b, a]);
    assert_eq!(
        clips[1],
        Clip {
            id: a,
            track: second,
            start: 9000,
            length: 50,
            offset: 7,
            muted: true,
            content: ClipContent::Pattern { pattern: PATTERN },
        }
    );

    let resize = ClipPatch {
        length: Some(10),
        ..ClipPatch::default()
    };
    let applied = run(
        &mut doc,
        update_clips(vec![(a, resize.clone()), (b, resize)]),
    );
    assert_eq!(applied.label, "Resize clips");
    let unmute = ClipPatch {
        muted: Some(false),
        ..ClipPatch::default()
    };
    assert_eq!(
        run(&mut doc, update_clips(vec![(a, unmute)])).label,
        "Unmute clip"
    );
}

#[test]
fn update_clips_validates_before_changing_anything() {
    let mut doc = document();
    let lane = add_playlist_track(&mut doc);
    let a = add_clip(&mut doc, lane, PATTERN, 0);
    let b = add_clip(&mut doc, lane, PATTERN, 3840);
    let good = ClipPatch {
        start: Some(5),
        ..ClipPatch::default()
    };
    let with = |bad: ClipPatch| update_clips(vec![(a, good.clone()), (b, bad)]);

    let zero = ClipPatch {
        length: Some(0),
        ..ClipPatch::default()
    };
    assert_invalid(fail(&mut doc, with(zero)), "at least one tick long");
    let far = ClipPatch {
        start: Some(u32::MAX),
        ..ClipPatch::default()
    };
    assert_invalid(
        fail(&mut doc, with(far)),
        "past the end of the longest song",
    );
    let long = ClipPatch {
        length: Some(MAX_SONG_TICKS - 3_839),
        ..ClipPatch::default()
    };
    assert_invalid(
        fail(&mut doc, with(long)),
        "past the end of the longest song",
    );
    let deep = ClipPatch {
        offset: Some(MAX_SONG_TICKS + 1),
        ..ClipPatch::default()
    };
    assert_invalid(
        fail(&mut doc, with(deep)),
        "further into what it plays than the longest song lasts",
    );
    let fits = ClipPatch {
        length: Some(MAX_SONG_TICKS - 3_840),
        offset: Some(MAX_SONG_TICKS),
        ..ClipPatch::default()
    };
    run(&mut doc, update_clips(vec![(b, fits)]));
    let lost = ClipPatch {
        track: Some(PlaylistTrackId(55)),
        ..ClipPatch::default()
    };
    assert_eq!(fail(&mut doc, with(lost)), not_found("playlist track", 55));
    assert_eq!(
        fail(
            &mut doc,
            update_clips(vec![(a, good.clone()), (ClipId(600), good.clone())])
        ),
        not_found("clip", 600)
    );

    let same = ClipPatch {
        start: Some(0),
        ..ClipPatch::default()
    };
    assert!(
        run(&mut doc, update_clips(vec![(a, same)]))
            .touched
            .is_empty()
    );
}

// Batch

#[test]
fn a_batch_is_one_undo_step() {
    let mut doc = document();
    let before = doc.project().clone();
    let applied = doc
        .dispatch(
            Command::Batch {
                label: None,
                commands: vec![
                    Command::AddSample {
                        name: "Kick".to_owned(),
                        path: SamplePath::Factory("kick.wav".to_owned()),
                    },
                    Command::AddPattern { name: None },
                    tempo(99.0),
                    Command::AddPlaylistTrack {
                        name: None,
                        index: None,
                    },
                ],
            },
            None,
        )
        .unwrap();
    doc.project().check().unwrap();
    assert_eq!(applied.label, "Add sample");
    assert_eq!(applied.created.len(), 3);
    let pattern = PatternId(applied.created[1]);
    assert_eq!(
        applied.touched,
        touched(|t| {
            t.settings = true;
            t.samples = true;
            t.playlist = true;
            t.pattern_list = true;
            t.patterns = vec![pattern];
        })
    );
    assert_eq!(doc.history().entries.len(), 1);
    assert_eq!(label(&doc), "Add sample");

    doc.undo().unwrap();
    assert!(same_content(doc.project(), &before));
}

#[test]
fn a_batch_that_fails_changes_nothing() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let next_id = doc.project().next_id;
    let error = fail(
        &mut doc,
        Command::Batch {
            label: Some("Doomed".to_owned()),
            commands: vec![
                Command::AddPattern { name: None },
                toggle(kick, 0),
                Command::AddMixerTrack { name: None },
                Command::RemoveChannel { id: kick },
                // The channel is gone by now, so this fails.
                toggle(kick, 1),
            ],
        },
    );
    assert_eq!(error, not_found("channel", kick.0));
    assert_eq!(doc.project().next_id, next_id);
}

#[test]
fn a_batch_takes_its_label_or_its_first_commands() {
    let mut doc = document();
    let batch = |label: Option<&str>, commands| Command::Batch {
        label: label.map(str::to_owned),
        commands,
    };
    let applied = run(
        &mut doc,
        batch(Some("Set up drums"), vec![tempo(100.0), tempo(101.0)]),
    );
    assert_eq!(applied.label, "Set up drums");
    assert_eq!(label(&doc), "Set up drums");

    let nested = batch(
        None,
        vec![
            batch(None, vec![Command::AddPattern { name: None }]),
            tempo(102.0),
        ],
    );
    assert_eq!(run(&mut doc, nested).label, "Add pattern");

    // A blank label is no label.
    let blank = batch(Some("  "), vec![tempo(103.0)]);
    assert_eq!(run(&mut doc, blank).label, "Change tempo");
    assert_eq!(run(&mut doc, batch(Some(""), Vec::new())).label, "Edit");

    let entries = doc.history().entries.len();
    let empty = run(&mut doc, batch(None, Vec::new()));
    assert!(empty.touched.is_empty());
    assert_eq!(doc.history().entries.len(), entries);
}

#[test]
fn later_commands_of_a_batch_see_what_earlier_ones_did() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let applied = run(
        &mut doc,
        Command::Batch {
            label: None,
            commands: vec![toggle(kick, 0), toggle(kick, 1), toggle(kick, 0)],
        },
    );
    // Step 0 was turned on and off again inside the batch.
    assert_eq!(applied.created.len(), 2);
    let notes = lane_notes(&doc, PATTERN, kick);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].start, TICKS_PER_STEP);
    assert_eq!(notes[0].id, NoteId(applied.created[1]));
}

#[test]
fn a_pattern_added_and_removed_in_one_batch_is_not_reported_as_changed() {
    let mut doc = document();
    let next = doc.project().next_id;
    let applied = run(
        &mut doc,
        Command::Batch {
            label: None,
            commands: vec![
                Command::AddPattern { name: None },
                Command::RemovePattern {
                    id: PatternId(next),
                },
            ],
        },
    );
    assert_eq!(applied.created, [next]);
    assert!(applied.touched.patterns.is_empty());
    assert_eq!(doc.project().patterns.len(), 1);
}

// History

#[test]
fn undo_and_redo_walk_the_history() {
    let mut doc = document();
    assert_eq!(doc.undo(), None);
    assert_eq!(doc.redo(), None);

    let (kick, _) = add_channel(&mut doc, "Kick");
    run(&mut doc, toggle(kick, 0));
    run(&mut doc, tempo(90.0));
    let labels: Vec<String> = doc
        .history()
        .entries
        .into_iter()
        .map(|entry| entry.label)
        .collect();
    assert_eq!(labels, ["Add channel", "Toggle step", "Change tempo"]);
    assert_eq!(doc.history().cursor, 3);

    assert_eq!(doc.undo(), Some(touched(|t| t.settings = true)));
    assert_eq!(doc.undo(), Some(touched(|t| t.patterns = vec![PATTERN])));
    assert_eq!(doc.history().cursor, 1);
    assert_eq!(doc.history().entries.len(), 3);
    assert!(lane_notes(&doc, PATTERN, kick).is_empty());

    assert_eq!(doc.redo(), Some(touched(|t| t.patterns = vec![PATTERN])));
    assert_eq!(lane_notes(&doc, PATTERN, kick).len(), 1);
    assert_eq!(doc.redo(), Some(touched(|t| t.settings = true)));
    assert_eq!(doc.redo(), None);
}

#[test]
fn a_new_edit_discards_what_was_undone() {
    let mut doc = document();
    run(&mut doc, tempo(90.0));
    run(&mut doc, tempo(91.0));
    run(&mut doc, tempo(92.0));
    doc.undo().unwrap();
    doc.undo().unwrap();

    // An edit that changes nothing leaves the redo steps alone.
    doc.dispatch(tempo(90.0), None).unwrap();
    assert_eq!(doc.history().entries.len(), 3);

    doc.dispatch(Command::AddPattern { name: None }, None)
        .unwrap();
    let labels: Vec<String> = doc
        .history()
        .entries
        .into_iter()
        .map(|entry| entry.label)
        .collect();
    assert_eq!(labels, ["Change tempo", "Add pattern"]);
    assert_eq!(doc.redo(), None);
    assert_eq!(doc.project().settings.tempo_bpm, 90.0);
}

#[test]
fn jump_moves_to_any_point_of_the_history() {
    let mut doc = document();
    let start = doc.project().clone();
    let (kick, _) = add_channel(&mut doc, "Kick");
    run(&mut doc, toggle(kick, 0));
    let second = add_pattern(&mut doc);
    run(&mut doc, tempo(90.0));
    let end = doc.project().clone();

    let back = doc.jump(0);
    assert!(same_content(doc.project(), &start));
    assert_eq!(doc.history().cursor, 0);
    // The second pattern no longer exists, so only the first is listed.
    assert_eq!(
        back,
        touched(|t| {
            t.settings = true;
            t.channels = true;
            t.mixer = true;
            t.pattern_list = true;
            t.patterns = vec![PATTERN];
        })
    );

    let forward = doc.jump(99);
    assert_eq!(doc.project(), &end);
    assert_eq!(doc.history().cursor, 4);
    assert!(forward.patterns.contains(&second));

    assert!(doc.jump(4).is_empty());
    let partial = doc.jump(2);
    assert_eq!(doc.history().cursor, 2);
    assert_eq!(
        partial,
        touched(|t| {
            t.settings = true;
            t.pattern_list = true;
        })
    );
    assert_eq!(doc.project().settings.tempo_bpm, 120.0);
    assert_eq!(lane_notes(&doc, PATTERN, kick).len(), 1);
}

#[test]
fn undo_keeps_ids_retired() {
    let mut doc = document();
    let (kick, kick_track) = add_channel(&mut doc, "Kick");
    let after_add = doc.project().clone();
    doc.undo().unwrap();
    assert_eq!(doc.project().next_id, after_add.next_id);

    // Redo brings back the very same ids.
    doc.redo().unwrap();
    assert_eq!(doc.project(), &after_add);

    // An edit made after an undo never reuses an undone id.
    doc.undo().unwrap();
    let (snare, snare_track) = add_channel(&mut doc, "Snare");
    for id in [snare.0, snare_track.0] {
        assert!(id != kick.0 && id != kick_track.0);
        assert!(id >= after_add.next_id);
    }
}

// Gestures

fn volume(id: ChannelId, volume: f32) -> Command {
    update_channel(
        id,
        ChannelPatch {
            volume: Some(volume),
            ..ChannelPatch::default()
        },
    )
}

#[test]
fn dispatches_of_one_gesture_collapse_into_one_undo_step() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let before = doc.project().clone();
    for step in 1..=50 {
        doc.dispatch(volume(kick, step as f32 / 100.0), Some(7))
            .unwrap();
    }
    assert_eq!(doc.history().entries.len(), 2);
    assert_eq!(label(&doc), "Change channel volume");
    let after = doc.project().clone();
    assert_eq!(after.channel(kick).unwrap().volume, 0.5);

    assert_eq!(doc.undo(), Some(touched(|t| t.channels = true)));
    assert_eq!(doc.project(), &before);
    doc.redo().unwrap();
    assert_eq!(doc.project(), &after);
}

#[test]
fn a_gesture_ends_when_anything_else_happens() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let entries = |doc: &Document| doc.history().entries.len();
    let level = |doc: &Document| doc.project().channel(kick).unwrap().volume;
    let drag = |doc: &mut Document, gesture| {
        let next = level(doc) - 0.01;
        doc.dispatch(volume(kick, next), gesture).unwrap();
    };

    drag(&mut doc, Some(1));
    drag(&mut doc, Some(1));
    assert_eq!(entries(&doc), 2);

    // Another gesture id.
    drag(&mut doc, Some(2));
    assert_eq!(entries(&doc), 3);
    // Going back to the first id is a new gesture too.
    drag(&mut doc, Some(1));
    assert_eq!(entries(&doc), 4);

    // No gesture id never merges, before or after.
    drag(&mut doc, None);
    drag(&mut doc, None);
    assert_eq!(entries(&doc), 6);
    drag(&mut doc, Some(1));
    assert_eq!(entries(&doc), 7);

    // A dispatch without a gesture in between, even one that changes nothing.
    doc.dispatch(tempo(120.0), None).unwrap();
    drag(&mut doc, Some(1));
    assert_eq!(entries(&doc), 8);

    // A save in between.
    doc.mark_saved();
    drag(&mut doc, Some(1));
    assert_eq!(entries(&doc), 9);
    assert!(doc.is_dirty());

    // An undo and redo in between.
    doc.undo().unwrap();
    doc.redo().unwrap();
    drag(&mut doc, Some(1));
    assert_eq!(entries(&doc), 10);

    // A jump in between, even one that goes nowhere.
    doc.jump(10);
    drag(&mut doc, Some(1));
    assert_eq!(entries(&doc), 11);

    // A dispatch of the same gesture that changes nothing does not end it.
    let same = level(&doc);
    doc.dispatch(volume(kick, same), Some(1)).unwrap();
    drag(&mut doc, Some(1));
    assert_eq!(entries(&doc), 11);

    // Nor does a dispatch that fails.
    doc.dispatch(volume(ChannelId(77), 0.5), Some(1))
        .unwrap_err();
    drag(&mut doc, Some(1));
    assert_eq!(entries(&doc), 11);
}

#[test]
fn a_gesture_can_span_different_commands() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    let ids = add_notes(&mut doc, kick, vec![note(0, 60), note(240, 62)]);
    let before = doc.project().clone();
    let entries = doc.history().entries.len();

    for offset in 1..=20 {
        let patch = |start: u32, key: u8| NotePatch {
            start: Some(start),
            key: Some(key),
            ..NotePatch::default()
        };
        let moved = update_notes(
            kick,
            vec![
                (ids[0], patch(offset * 10, 60 + offset as u8)),
                (ids[1], patch(240 + offset * 10, 62 + offset as u8)),
            ],
        );
        doc.dispatch(moved, Some(3)).unwrap();
        doc.project().check().unwrap();
    }
    doc.dispatch(volume(kick, 0.3), Some(3)).unwrap();
    doc.dispatch(toggle(kick, 8), Some(3)).unwrap();
    assert_eq!(doc.history().entries.len(), entries + 1);
    // The step keeps the label of the gesture's first command.
    assert_eq!(label(&doc), "Move notes");
    let after = doc.project().clone();

    doc.undo().unwrap();
    assert!(same_content(doc.project(), &before));
    doc.redo().unwrap();
    assert_eq!(doc.project(), &after);
}

#[test]
fn a_gesture_that_returns_to_its_start_leaves_no_step() {
    let mut doc = document();
    let (kick, _) = add_channel(&mut doc, "Kick");
    doc.mark_saved();
    let before = doc.project().clone();

    doc.dispatch(volume(kick, 0.2), Some(1)).unwrap();
    assert!(doc.is_dirty());
    assert_eq!(doc.history().entries.len(), 2);
    let applied = doc
        .dispatch(volume(kick, DEFAULT_CHANNEL_VOLUME), Some(1))
        .unwrap();
    // The dispatch itself did change the channel back.
    assert_eq!(applied.touched, touched(|t| t.channels = true));
    assert_eq!(doc.project(), &before);
    assert_eq!(doc.history().entries.len(), 1);
    assert!(!doc.is_dirty());

    // The gesture can carry on and makes a fresh step.
    doc.dispatch(volume(kick, 0.4), Some(1)).unwrap();
    assert_eq!(doc.history().entries.len(), 2);
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
}

// Dirty tracking

#[test]
fn the_document_is_dirty_when_it_differs_from_the_saved_state() {
    let mut doc = document();
    assert!(!doc.is_dirty());

    run(&mut doc, tempo(90.0));
    assert!(doc.is_dirty());
    doc.mark_saved();
    assert!(!doc.is_dirty());

    run(&mut doc, tempo(91.0));
    assert!(doc.is_dirty());
    doc.undo().unwrap();
    assert!(!doc.is_dirty(), "undoing back to the saved state is clean");
    doc.undo().unwrap();
    assert!(doc.is_dirty(), "undoing past the saved state is dirty");
    doc.redo().unwrap();
    assert!(!doc.is_dirty());
    doc.redo().unwrap();
    assert!(doc.is_dirty());
}

#[test]
fn the_document_stays_dirty_once_the_saved_state_is_discarded() {
    let mut doc = document();
    run(&mut doc, tempo(90.0));
    run(&mut doc, tempo(91.0));
    doc.mark_saved();
    doc.undo().unwrap();
    doc.undo().unwrap();

    // This drops the two undone steps, and the saved state with them.
    doc.dispatch(tempo(95.0), None).unwrap();
    assert!(doc.is_dirty());
    doc.undo().unwrap();
    assert!(doc.is_dirty());
    doc.redo().unwrap();
    assert!(doc.is_dirty());
    doc.dispatch(tempo(96.0), None).unwrap();
    assert!(doc.is_dirty(), "two steps in is no longer the saved state");

    doc.mark_saved();
    assert!(!doc.is_dirty());
}

// Patches and snapshots

#[test]
fn a_patch_carries_only_the_touched_sections() {
    let mut doc = document();
    assert_eq!(doc.revision(), 0);
    let (kick, _) = add_channel(&mut doc, "Kick");

    let applied = doc.dispatch(toggle(kick, 0), None).unwrap();
    let patch = doc.patch(&applied.touched);
    assert_eq!(patch.revision, 1);
    assert_eq!(doc.revision(), 1);
    assert_eq!(patch.settings, None);
    assert_eq!(patch.samples, None);
    assert_eq!(patch.channels, None);
    assert_eq!(patch.mixer, None);
    assert_eq!(patch.playlist, None);
    assert_eq!(patch.pattern_order, None);
    assert_eq!(patch.patterns, doc.project().patterns);
    assert_eq!(patch.history, doc.history());
    assert!(patch.dirty);

    let json = serde_json::to_value(&patch).unwrap();
    let mut keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["dirty", "history", "patterns", "revision"]);

    let second = add_pattern(&mut doc);
    let applied = doc
        .dispatch(Command::RemovePattern { id: PATTERN }, None)
        .unwrap();
    let patch = doc.patch(&applied.touched);
    assert_eq!(patch.revision, 2);
    assert_eq!(patch.pattern_order, Some(vec![second]));
    assert!(patch.patterns.is_empty());

    let everything = Touched::all(doc.project());
    let patch = doc.patch(&everything);
    assert_eq!(patch.revision, 3);
    assert_eq!(patch.settings.as_ref(), Some(&doc.project().settings));
    assert_eq!(patch.samples.as_ref(), Some(&doc.project().samples));
    assert_eq!(patch.channels.as_ref(), Some(&doc.project().channels));
    assert_eq!(patch.mixer.as_ref(), Some(&doc.project().mixer));
    assert_eq!(patch.playlist.as_ref(), Some(&doc.project().playlist));
    assert_eq!(patch.patterns, doc.project().patterns);

    doc.mark_saved();
    let patch = doc.patch(&Touched::default());
    assert_eq!(patch.revision, 4);
    assert!(!patch.dirty);
}

#[test]
fn a_snapshot_holds_the_whole_document() {
    let mut doc = document();
    run(&mut doc, tempo(90.0));
    doc.patch(&Touched::default());
    let snapshot = doc.snapshot(Some("C:/Music/Song.windfall".to_owned()));
    assert_eq!(snapshot.revision, 1);
    assert_eq!(&snapshot.project, doc.project());
    assert_eq!(snapshot.history, doc.history());
    assert!(snapshot.dirty);
    assert_eq!(snapshot.path.as_deref(), Some("C:/Music/Song.windfall"));
    assert_eq!(doc.snapshot(None).path, None);
}

// Project::check

/// A project that uses every part of the model.
fn full_project() -> Project {
    let mut doc = document();
    let sample = add_sample(&mut doc, "kick");
    let applied = run(
        &mut doc,
        Command::AddChannel {
            name: None,
            sample: Some(sample),
            instrument: None,
            index: None,
            mixer_track: None,
        },
    );
    let kick = ChannelId(applied.created[0]);
    let (hat, hat_track) = add_channel(&mut doc, "Hat");
    add_notes(&mut doc, kick, vec![note(0, 60), note(240, 60)]);
    add_notes(&mut doc, hat, vec![note(0, 60)]);
    run(
        &mut doc,
        Command::SetSamplerEnvelope {
            id: kick,
            envelope: Some(Envelope::default()),
        },
    );
    let bus = add_mixer_track(&mut doc);
    run(&mut doc, send(hat_track, bus, Some(0.5)));
    let lane = add_playlist_track(&mut doc);
    add_clip(&mut doc, lane, PATTERN, 0);
    add_clip(&mut doc, lane, PATTERN, 3840);
    let (_, kick_track) = (kick, TrackId(applied.created[1]));
    add_effect(&mut doc, kick_track, EffectKind::Reverb);
    add_effect(&mut doc, kick_track, EffectKind::Limiter);
    add_effect(&mut doc, TrackId::MASTER, EffectKind::Compressor);
    add_instrument(&mut doc);
    doc.project().clone()
}

#[test]
fn check_accepts_a_project_built_by_commands() {
    full_project().check().unwrap();
}

/// Breaks one rule of a valid project.
type Damage = fn(&mut Project);

#[test]
fn check_names_each_broken_rule() {
    fn sampler(project: &mut Project) -> &mut SamplerSettings {
        let ChannelSource::Sampler(sampler) = &mut project.channels[0].source else {
            panic!("the first channel is not a sampler");
        };
        sampler
    }
    fn synth(project: &mut Project) -> &mut SynthParams {
        let ChannelSource::Instrument { params, .. } = &mut project.channels[2].source else {
            panic!("the third channel is not an instrument");
        };
        match params {
            InstrumentParams::SubtractiveSynth(synth) => synth,
            _ => panic!("expected the subtractive synth"),
        }
    }
    fn reverb(project: &mut Project) -> &mut ReverbParams {
        let EffectParams::Reverb(reverb) = &mut project.mixer.tracks[1].effects[0].params else {
            panic!("the first effect is not a reverb");
        };
        reverb
    }
    fn first_note(project: &mut Project) -> &mut Note {
        &mut project.patterns[0].lanes[0].notes[0]
    }
    let cases: Vec<(&str, Damage)> = vec![
        ("format version", |p| p.format_version = 2),
        ("tempo", |p| p.settings.tempo_bpm = 5.0),
        ("tempo", |p| p.settings.tempo_bpm = f64::NAN),
        ("beats per bar", |p| p.settings.time_signature.numerator = 0),
        ("beat unit", |p| p.settings.time_signature.denominator = 5),
        ("swing", |p| p.settings.swing = 1.5),
        ("more than one sample", |p| {
            let copy = p.samples[0].clone();
            p.samples.push(copy);
        }),
        ("forward slashes", |p| {
            p.samples[0].path = SamplePath::Project("a\\b.wav".to_owned());
        }),
        ("reserved for the master", |p| p.samples[0].id = SampleId(0)),
        ("not below the next id", |p| p.next_id = 3),
        ("no master", |p| p.mixer.tracks.clear()),
        ("not the master", |p| p.mixer.tracks.swap(0, 1)),
        ("master track has an output", |p| {
            p.mixer.tracks[0].output = Some(p.mixer.tracks[1].id);
        }),
        ("master track has sends", |p| {
            let target = p.mixer.tracks[1].id;
            p.mixer.tracks[0].sends.push(Send { target, gain: 1.0 });
        }),
        ("500 inserts, Master and one Current utility", |p| {
            let extra = p.mixer.tracks[1].clone();
            p.mixer.tracks.resize(MAX_MIXER_TRACKS + 1, extra);
        }),
        ("more than one mixer track", |p| {
            let copy = p.mixer.tracks[1].clone();
            p.mixer.tracks.push(copy);
        }),
        ("volume", |p| p.mixer.tracks[1].volume = 2.5),
        ("pan", |p| p.mixer.tracks[1].pan = f32::NAN),
        ("not 0xRRGGBB", |p| p.mixer.tracks[1].color = 0x1_000_000),
        ("outputs to mixer track 999", |p| {
            p.mixer.tracks[1].output = Some(TrackId(999));
        }),
        ("sends to mixer track 999", |p| {
            p.mixer.tracks[2].sends[0].target = TrackId(999);
        }),
        ("more than one send", |p| {
            let copy = p.mixer.tracks[2].sends[0];
            p.mixer.tracks[2].sends.push(copy);
        }),
        ("send with gain", |p| p.mixer.tracks[2].sends[0].gain = -1.0),
        ("loops back", |p| {
            p.mixer.tracks[1].output = Some(p.mixer.tracks[1].id);
        }),
        ("loops back", |p| {
            p.mixer.tracks[3].output = Some(p.mixer.tracks[2].id);
        }),
        ("more than one channel", |p| {
            let copy = p.channels[0].clone();
            p.channels.push(copy);
        }),
        ("volume", |p| p.channels[0].volume = -0.5),
        ("pan", |p| p.channels[0].pan = 2.0),
        ("not 0xRRGGBB", |p| p.channels[0].color = u32::MAX),
        ("plays into mixer track 999", |p| {
            p.channels[0].mixer_track = TrackId(999);
        }),
        ("plays sample 999", |p| {
            sampler(p).sample = Some(SampleId(999));
        }),
        ("root key", |p| sampler(p).root_key = 128),
        ("tuned", |p| sampler(p).tune = 49.0),
        ("sample gain", |p| sampler(p).gain = 3.0),
        ("outside 0 to 1", |p| sampler(p).end = 1.5),
        ("not before its end", |p| sampler(p).start = 1.0),
        ("envelope attack", |p| {
            sampler(p).envelope = Some(Envelope {
                attack_ms: -1.0,
                ..Envelope::default()
            });
        }),
        ("envelope sustain", |p| {
            sampler(p).envelope = Some(Envelope {
                sustain: 2.0,
                ..Envelope::default()
            });
        }),
        ("instrument setting outside its range", |p| {
            synth(p).gain = 9.0
        }),
        ("instrument setting outside its range", |p| {
            synth(p).glide_ms = f32::NAN;
        }),
        ("more than one effect", |p| {
            let copy = p.mixer.tracks[1].effects[0].clone();
            p.mixer.tracks[0].effects.push(copy);
        }),
        ("effect 999 is not below the next id", |p| {
            p.mixer.tracks[1].effects[0].id = EffectId(999);
        }),
        ("effects, more than the limit", |p| {
            let mut next = p.next_id;
            let copy = p.mixer.tracks[1].effects[0].clone();
            let effects = &mut p.mixer.tracks[1].effects;
            while effects.len() <= MAX_EFFECT_SLOTS {
                effects.push(EffectSlot {
                    impulse: None,
                    id: EffectId(next),
                    ..copy.clone()
                });
                next += 1;
            }
            p.next_id = next;
        }),
        ("has mix 1.5", |p| p.mixer.tracks[1].effects[0].mix = 1.5),
        ("has mix NaN", |p| {
            p.mixer.tracks[1].effects[0].mix = f32::NAN
        }),
        ("effect 14 has a setting outside its range", |p| {
            reverb(p).decay_s = 1.0e6;
        }),
        ("has a setting outside its range", |p| {
            reverb(p).mix = f32::NAN;
        }),
        ("no patterns", |p| {
            p.playlist.clips.clear();
            p.patterns.clear();
        }),
        ("more than one pattern", |p| {
            let mut copy = p.patterns[0].clone();
            copy.lanes.clear();
            p.patterns.push(copy);
        }),
        ("steps long", |p| p.patterns[0].length_steps = 0),
        ("steps long", |p| p.patterns[0].length_steps = 1025),
        ("not 0xRRGGBB", |p| p.patterns[0].color = 0x1_000_000),
        ("lanes sorted", |p| p.patterns[0].lanes.swap(0, 1)),
        ("lanes sorted", |p| {
            let copy = p.patterns[0].lanes[0].clone();
            p.patterns[0].lanes.insert(0, copy);
        }),
        ("the channel does not exist", |p| {
            p.channels.remove(1);
        }),
        ("the lane is empty", |p| {
            p.patterns[0].lanes[0].notes.clear()
        }),
        ("notes are not sorted", |p| {
            p.patterns[0].lanes[0].notes.swap(0, 1);
        }),
        ("more than one note", |p| {
            let id = p.patterns[0].lanes[0].notes[0].id;
            p.patterns[0].lanes[1].notes[0].id = id;
        }),
        ("has no length", |p| first_note(p).length = 0),
        ("ends past the last tick", |p| {
            first_note(p).start = 1;
            first_note(p).length = u32::MAX;
        }),
        ("has key", |p| first_note(p).key = 128),
        ("velocity", |p| first_note(p).velocity = 1.5),
        ("pan", |p| first_note(p).pan = -1.5),
        ("more than one playlist track", |p| {
            let copy = p.playlist.tracks[0].clone();
            p.playlist.tracks.push(copy);
        }),
        ("clips are not sorted", |p| p.playlist.clips.swap(0, 1)),
        ("more than one clip", |p| {
            p.playlist.clips[1].id = p.playlist.clips[0].id;
        }),
        ("on playlist track 999", |p| {
            p.playlist.clips[0].track = PlaylistTrackId(999);
        }),
        ("has no length", |p| p.playlist.clips[0].length = 0),
        ("ends past the end of the longest song", |p| {
            p.playlist.clips[1].length = u32::MAX;
        }),
        (
            "ends past the end of the longest song, tick 960000000",
            |p| {
                let clip = &mut p.playlist.clips[1];
                clip.length = MAX_SONG_TICKS - clip.start + 1;
            },
        ),
        ("an offset past the end of the longest song", |p| {
            p.playlist.clips[0].offset = MAX_SONG_TICKS + 1;
        }),
        ("plays pattern 999", |p| {
            p.playlist.clips[0].content = ClipContent::Pattern {
                pattern: PatternId(999),
            };
        }),
    ];
    for (index, (words, damage)) in cases.into_iter().enumerate() {
        let mut project = full_project();
        damage(&mut project);
        match project.check() {
            Ok(()) => panic!("case {index} (\"{words}\") passed the check"),
            Err(problem) => assert!(
                problem.contains(words),
                "case {index}: \"{problem}\" lacks \"{words}\""
            ),
        }
    }
}

// Effects

fn effect_ids(doc: &Document, track_id: TrackId) -> Vec<EffectId> {
    track(doc, track_id).effects.iter().map(|e| e.id).collect()
}

fn slot(doc: &Document, track_id: TrackId, effect: EffectId) -> EffectSlot {
    let track = track(doc, track_id);
    track.effect(effect).expect("the effect exists").clone()
}

fn set_param(track: TrackId, effect: EffectId, param: usize, value: f32) -> Command {
    Command::SetEffectParam {
        track,
        effect,
        param: param as u32,
        value,
    }
}

fn param_index<P: ParamSet>(id: &str) -> usize {
    P::index_of(id).expect("the setting exists")
}

#[test]
fn add_effect_puts_a_default_effect_on_the_track() {
    let mut doc = document();
    let track_id = add_mixer_track(&mut doc);
    let applied = run(
        &mut doc,
        Command::AddEffect {
            track: track_id,
            kind: EffectKind::Reverb,
            index: None,
        },
    );
    assert_eq!(applied.label, "Add effect");
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
    let reverb = EffectId(applied.created[0]);
    assert_eq!(applied.created.len(), 1);
    assert_eq!(
        slot(&doc, track_id, reverb),
        EffectSlot {
            impulse: None,
            id: reverb,
            enabled: true,
            mix: 1.0,
            params: EffectParams::Reverb(ReverbParams::default()),
        }
    );

    // The end by default and for an index past it, anywhere else by index.
    let limiter = add_effect(&mut doc, track_id, EffectKind::Limiter);
    let at = |index| Command::AddEffect {
        track: track_id,
        kind: EffectKind::Eq,
        index: Some(index),
    };
    let first = EffectId(run(&mut doc, at(0)).created[0]);
    let middle = EffectId(run(&mut doc, at(2)).created[0]);
    let last = EffectId(run(&mut doc, at(99)).created[0]);
    assert_eq!(
        effect_ids(&doc, track_id),
        [first, reverb, middle, limiter, last]
    );
    for kind in EffectKind::ALL {
        let id = add_effect(&mut doc, TrackId::MASTER, kind);
        assert_eq!(slot(&doc, TrackId::MASTER, id).kind(), kind);
        assert_eq!(
            slot(&doc, TrackId::MASTER, id).params,
            kind.default_params()
        );
        run(
            &mut doc,
            Command::RemoveEffect {
                track: TrackId::MASTER,
                effect: id,
            },
        );
    }

    let unknown = Command::AddEffect {
        track: TrackId(999),
        kind: EffectKind::Eq,
        index: None,
    };
    assert_eq!(fail(&mut doc, unknown), not_found("mixer track", 999));
}

#[test]
fn a_track_holds_a_limited_number_of_effects() {
    let mut doc = document();
    let full = add_mixer_track(&mut doc);
    let other = add_mixer_track(&mut doc);
    let outsider = add_effect(&mut doc, other, EffectKind::Delay);
    let mut ids = Vec::new();
    for _ in 0..MAX_EFFECT_SLOTS {
        ids.push(add_effect(&mut doc, full, EffectKind::Eq));
    }
    assert_eq!(MAX_EFFECT_SLOTS, 10);

    let add = Command::AddEffect {
        track: full,
        kind: EffectKind::Eq,
        index: Some(0),
    };
    assert_invalid(fail(&mut doc, add), "is full: it holds 10 effects");
    let duplicate = Command::DuplicateEffect {
        track: full,
        effect: ids[0],
    };
    assert_invalid(fail(&mut doc, duplicate), "is full");
    let move_in = Command::MoveEffect {
        track: other,
        effect: outsider,
        to_track: Some(full),
        index: 0,
    };
    assert_invalid(fail(&mut doc, move_in), "is full");

    // A full track can still be reordered, and has room again once an
    // effect leaves.
    run(
        &mut doc,
        Command::MoveEffect {
            track: full,
            effect: ids[0],
            to_track: None,
            index: 9,
        },
    );
    run(
        &mut doc,
        Command::RemoveEffect {
            track: full,
            effect: ids[3],
        },
    );
    add_effect(&mut doc, full, EffectKind::Reverb);
}

#[test]
fn remove_effect_takes_one_effect_out_of_the_chain() {
    let mut doc = document();
    let track_id = add_mixer_track(&mut doc);
    let other = add_mixer_track(&mut doc);
    let eq = add_effect(&mut doc, track_id, EffectKind::Eq);
    let reverb = add_effect(&mut doc, track_id, EffectKind::Reverb);
    let elsewhere = add_effect(&mut doc, other, EffectKind::Delay);

    let remove = |track, effect| Command::RemoveEffect { track, effect };
    assert_eq!(
        fail(&mut doc, remove(track_id, EffectId(999))),
        not_found("effect", 999)
    );
    assert_eq!(
        fail(&mut doc, remove(TrackId(999), eq)),
        not_found("mixer track", 999)
    );
    // An effect is addressed through the track it is on.
    assert_eq!(
        fail(&mut doc, remove(track_id, elsewhere)),
        not_found("effect", elsewhere.0)
    );

    let applied = run(&mut doc, remove(track_id, eq));
    assert_eq!(applied.label, "Delete effect");
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
    assert_eq!(effect_ids(&doc, track_id), [reverb]);
    assert_eq!(effect_ids(&doc, other), [elsewhere]);
    // Its id is never handed out again.
    let next = add_effect(&mut doc, track_id, EffectKind::Eq);
    assert!(next.0 > elsewhere.0);
}

#[test]
fn move_effect_reorders_a_chain() {
    let mut doc = document();
    let track_id = add_mixer_track(&mut doc);
    let a = add_effect(&mut doc, track_id, EffectKind::Eq);
    let b = add_effect(&mut doc, track_id, EffectKind::Compressor);
    let c = add_effect(&mut doc, track_id, EffectKind::Limiter);
    let to = |effect, index| Command::MoveEffect {
        track: track_id,
        effect,
        to_track: None,
        index,
    };

    let applied = run(&mut doc, to(a, 2));
    assert_eq!(applied.label, "Move effect");
    assert_eq!(effect_ids(&doc, track_id), [b, c, a]);
    run(&mut doc, to(a, 0));
    assert_eq!(effect_ids(&doc, track_id), [a, b, c]);
    // Past the end means the end, and naming the track itself is the same
    // as naming none.
    run(&mut doc, to(b, 99));
    assert_eq!(effect_ids(&doc, track_id), [a, c, b]);
    run(
        &mut doc,
        Command::MoveEffect {
            track: track_id,
            effect: b,
            to_track: Some(track_id),
            index: 1,
        },
    );
    assert_eq!(effect_ids(&doc, track_id), [a, b, c]);

    // Moving an effect to where it is changes nothing.
    let history = doc.history();
    assert!(run(&mut doc, to(b, 1)).touched.is_empty());
    assert_eq!(doc.history(), history);
    assert_eq!(
        fail(&mut doc, to(EffectId(999), 0)),
        not_found("effect", 999)
    );
}

#[test]
fn move_effect_takes_an_effect_to_another_track() {
    let mut doc = document();
    let from = add_mixer_track(&mut doc);
    let to = add_mixer_track(&mut doc);
    let eq = add_effect(&mut doc, from, EffectKind::Eq);
    let reverb = add_effect(&mut doc, from, EffectKind::Reverb);
    let x = add_effect(&mut doc, to, EffectKind::Delay);
    let y = add_effect(&mut doc, to, EffectKind::Limiter);
    let decay = param_index::<ReverbParams>("decayS");
    run(&mut doc, set_param(from, reverb, decay, 7.5));
    run(
        &mut doc,
        Command::UpdateEffect {
            track: from,
            effect: reverb,
            patch: EffectSlotPatch {
                enabled: Some(false),
                mix: Some(0.25),
            },
        },
    );
    let moved = slot(&doc, from, reverb);

    let applied = run(
        &mut doc,
        Command::MoveEffect {
            track: from,
            effect: reverb,
            to_track: Some(to),
            index: 1,
        },
    );
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
    assert!(applied.created.is_empty());
    assert_eq!(effect_ids(&doc, from), [eq]);
    assert_eq!(effect_ids(&doc, to), [x, reverb, y]);
    // Same id, same switch, same mix, same settings.
    assert_eq!(slot(&doc, to, reverb), moved);

    run(
        &mut doc,
        Command::MoveEffect {
            track: to,
            effect: reverb,
            to_track: Some(TrackId::MASTER),
            index: 99,
        },
    );
    assert_eq!(effect_ids(&doc, TrackId::MASTER), [reverb]);

    let unknown = Command::MoveEffect {
        track: from,
        effect: eq,
        to_track: Some(TrackId(999)),
        index: 0,
    };
    assert_eq!(fail(&mut doc, unknown), not_found("mixer track", 999));
    // The effect is no longer where the command says.
    let stale = Command::MoveEffect {
        track: from,
        effect: reverb,
        to_track: Some(to),
        index: 0,
    };
    assert_eq!(fail(&mut doc, stale), not_found("effect", reverb.0));
}

#[test]
fn update_effect_switches_and_mixes() {
    let mut doc = document();
    let track_id = add_mixer_track(&mut doc);
    let effect = add_effect(&mut doc, track_id, EffectKind::Delay);
    let update = |patch| Command::UpdateEffect {
        track: track_id,
        effect,
        patch,
    };
    let enabled = |enabled| EffectSlotPatch {
        enabled: Some(enabled),
        mix: None,
    };
    let mix = |mix| EffectSlotPatch {
        enabled: None,
        mix: Some(mix),
    };

    let applied = run(&mut doc, update(enabled(false)));
    assert_eq!(applied.label, "Switch effect off");
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
    assert!(!slot(&doc, track_id, effect).enabled);
    assert_eq!(
        run(&mut doc, update(enabled(true))).label,
        "Switch effect on"
    );
    assert!(slot(&doc, track_id, effect).enabled);

    assert_eq!(run(&mut doc, update(mix(0.4))).label, "Change effect mix");
    assert_eq!(slot(&doc, track_id, effect).mix, 0.4);
    run(&mut doc, update(mix(7.0)));
    assert_eq!(slot(&doc, track_id, effect).mix, 1.0);
    run(&mut doc, update(mix(f32::NEG_INFINITY)));
    assert_eq!(slot(&doc, track_id, effect).mix, 0.0);
    assert_invalid(
        fail(&mut doc, update(mix(f32::NAN))),
        "the mix is not a number",
    );

    let both = EffectSlotPatch {
        enabled: Some(false),
        mix: Some(0.5),
    };
    assert_eq!(run(&mut doc, update(both)).label, "Change effect");
    let changed = slot(&doc, track_id, effect);
    assert_eq!((changed.enabled, changed.mix), (false, 0.5));
    // The settings were left alone throughout.
    assert_eq!(changed.params, EffectKind::Delay.default_params());

    let history = doc.history();
    assert!(run(&mut doc, update(both)).touched.is_empty());
    let nothing = update(EffectSlotPatch::default());
    assert!(run(&mut doc, nothing).touched.is_empty());
    assert_eq!(doc.history(), history);
    let unknown = Command::UpdateEffect {
        track: track_id,
        effect: EffectId(999),
        patch: both,
    };
    assert_eq!(fail(&mut doc, unknown), not_found("effect", 999));
}

#[test]
fn set_effect_param_sets_one_setting_within_its_range() {
    let mut doc = document();
    let track_id = add_mixer_track(&mut doc);
    let effect = add_effect(&mut doc, track_id, EffectKind::Compressor);
    let compressor = |doc: &Document| match slot(doc, track_id, effect).params {
        EffectParams::Compressor(params) => params,
        other => panic!("{other:?} is not a compressor"),
    };
    let index = param_index::<CompressorParams>;
    let (threshold, ratio) = (index("thresholdDb"), index("ratio"));
    let (auto_makeup, detector) = (index("autoMakeup"), index("detector"));

    let applied = run(&mut doc, set_param(track_id, effect, threshold, -30.0));
    assert_eq!(applied.label, "Change Threshold");
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
    assert_eq!(
        compressor(&doc),
        CompressorParams {
            threshold_db: -30.0,
            ..CompressorParams::default()
        }
    );

    // A value outside the range is brought into it, and infinity is the
    // end of the range.
    run(&mut doc, set_param(track_id, effect, threshold, 12.0));
    assert_eq!(compressor(&doc).threshold_db, 0.0);
    run(&mut doc, set_param(track_id, effect, threshold, -500.0));
    assert_eq!(compressor(&doc).threshold_db, -60.0);
    run(&mut doc, set_param(track_id, effect, ratio, f32::INFINITY));
    assert_eq!(compressor(&doc).ratio, 100.0);
    run(
        &mut doc,
        set_param(track_id, effect, ratio, f32::NEG_INFINITY),
    );
    assert_eq!(compressor(&doc).ratio, 1.0);

    // A toggle is 0 or 1 and a choice is the index of the choice.
    run(&mut doc, set_param(track_id, effect, auto_makeup, 1.0));
    assert!(compressor(&doc).auto_makeup);
    run(&mut doc, set_param(track_id, effect, auto_makeup, 0.0));
    assert!(!compressor(&doc).auto_makeup);
    run(&mut doc, set_param(track_id, effect, detector, 1.0));
    assert_eq!(compressor(&doc).detector, DetectorMode::Rms);
    run(&mut doc, set_param(track_id, effect, detector, 0.0));
    assert_eq!(compressor(&doc).detector, DetectorMode::Peak);

    let before = compressor(&doc);
    assert_invalid(
        fail(&mut doc, set_param(track_id, effect, threshold, f32::NAN)),
        "the value is not a number",
    );
    let count = EffectKind::Compressor.descriptors().len();
    assert_invalid(
        fail(&mut doc, set_param(track_id, effect, count, 1.0)),
        &format!("the Compressor has no setting number {count}"),
    );
    assert_eq!(
        fail(&mut doc, set_param(track_id, EffectId(999), 0, 1.0)),
        not_found("effect", 999)
    );
    assert_eq!(
        fail(&mut doc, set_param(TrackId(999), effect, 0, 1.0)),
        not_found("mixer track", 999)
    );
    assert_eq!(compressor(&doc), before);

    // Setting a value it already has is no edit.
    let history = doc.history();
    let same = set_param(track_id, effect, ratio, before.ratio);
    assert!(run(&mut doc, same).touched.is_empty());
    assert_eq!(doc.history(), history);
}

#[test]
fn every_setting_of_every_effect_can_be_set_by_its_index() {
    let mut doc = document();
    let track_id = add_mixer_track(&mut doc);
    for kind in EffectKind::ALL {
        let mut doc = Document::new(doc.project().clone());
        let effect = add_effect(&mut doc, track_id, kind);
        for (index, info) in kind.descriptors().iter().enumerate() {
            for value in [info.min, info.max, info.default] {
                run(&mut doc, set_param(track_id, effect, index, value));
                let params = slot(&doc, track_id, effect).params;
                assert_eq!(params.get(index), Some(value), "{kind:?} {}", info.id);
            }
            run(
                &mut doc,
                set_param(track_id, effect, index, info.max + 1.0e6),
            );
            let params = slot(&doc, track_id, effect).params;
            assert_eq!(params.get(index), Some(info.max), "{kind:?} {}", info.id);
        }
    }
}

#[test]
fn set_effect_params_replaces_every_setting() {
    let mut doc = document();
    let track_id = add_mixer_track(&mut doc);
    let effect = add_effect(&mut doc, track_id, EffectKind::Limiter);
    let set = |params| Command::SetEffectParams {
        track: track_id,
        effect,
        params,
    };
    let preset = LimiterParams {
        ceiling_db: -1.0,
        input_gain_db: 6.0,
        release_ms: 50.0,
        lookahead_ms: 2.0,
        ..LimiterParams::default()
    };

    let applied = run(&mut doc, set(EffectParams::Limiter(preset)));
    assert_eq!(applied.label, "Change effect settings");
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
    assert_eq!(
        slot(&doc, track_id, effect).params,
        EffectParams::Limiter(preset)
    );

    // Values outside their ranges are brought into them, as when they are
    // set one by one.
    let wild = LimiterParams {
        ceiling_db: 3.0,
        input_gain_db: -100.0,
        release_ms: f32::INFINITY,
        lookahead_ms: -0.0,
        ..LimiterParams::default()
    };
    run(&mut doc, set(EffectParams::Limiter(wild)));
    assert_eq!(
        slot(&doc, track_id, effect).params,
        EffectParams::Limiter(LimiterParams {
            ceiling_db: 0.0,
            input_gain_db: -12.0,
            release_ms: 1000.0,
            lookahead_ms: 0.1,
            ..LimiterParams::default()
        })
    );

    let before = slot(&doc, track_id, effect);
    let broken = LimiterParams {
        release_ms: f32::NAN,
        ..preset
    };
    assert_invalid(
        fail(&mut doc, set(EffectParams::Limiter(broken))),
        "the value is not a number",
    );
    assert_invalid(
        fail(&mut doc, set(EffectKind::Reverb.default_params())),
        "these are the settings of a Reverb, and this is a Limiter",
    );
    assert_eq!(slot(&doc, track_id, effect), before);
    let unknown = Command::SetEffectParams {
        track: track_id,
        effect: EffectId(999),
        params: EffectParams::Limiter(preset),
    };
    assert_eq!(fail(&mut doc, unknown), not_found("effect", 999));
}

#[test]
fn duplicate_effect_copies_an_effect_right_after_it() {
    let mut doc = document();
    let track_id = add_mixer_track(&mut doc);
    let eq = add_effect(&mut doc, track_id, EffectKind::Eq);
    let delay = add_effect(&mut doc, track_id, EffectKind::Delay);
    let feedback = param_index::<DelayParams>("feedback");
    run(&mut doc, set_param(track_id, delay, feedback, 0.25));
    run(
        &mut doc,
        Command::UpdateEffect {
            track: track_id,
            effect: delay,
            patch: EffectSlotPatch {
                enabled: Some(false),
                mix: Some(0.5),
            },
        },
    );

    let applied = run(
        &mut doc,
        Command::DuplicateEffect {
            track: track_id,
            effect: delay,
        },
    );
    assert_eq!(applied.label, "Duplicate effect");
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
    let copy = EffectId(applied.created[0]);
    assert_eq!(effect_ids(&doc, track_id), [eq, delay, copy]);
    assert_eq!(
        slot(&doc, track_id, copy),
        EffectSlot {
            impulse: None,
            id: copy,
            ..slot(&doc, track_id, delay)
        }
    );
    let first = Command::DuplicateEffect {
        track: track_id,
        effect: eq,
    };
    let second = EffectId(run(&mut doc, first).created[0]);
    assert_eq!(effect_ids(&doc, track_id), [eq, second, delay, copy]);

    let unknown = Command::DuplicateEffect {
        track: track_id,
        effect: EffectId(999),
    };
    assert_eq!(fail(&mut doc, unknown), not_found("effect", 999));
}

#[test]
fn a_setting_is_named_in_the_history() {
    let mut doc = document();
    let track_id = add_mixer_track(&mut doc);
    let (channel, _) = add_instrument(&mut doc);
    // Every setting of every effect and of the synth has a label made of
    // its own name.
    for kind in EffectKind::ALL {
        let mut doc = Document::new(doc.project().clone());
        let effect = add_effect(&mut doc, track_id, kind);
        for (param, info) in kind.descriptors().iter().enumerate() {
            let other = if info.default == info.max {
                info.min
            } else {
                info.max
            };
            let previous_history = doc.history();
            let applied = run(&mut doc, set_param(track_id, effect, param, other));
            assert_eq!(applied.label, format!("Change {}", info.name));
            if info.min == info.max {
                // A fixed descriptor, such as the final echo unit's send
                // without a destination, must remain a history-free no-op.
                assert!(applied.touched.is_empty());
                assert_eq!(doc.history(), previous_history);
            } else {
                assert_eq!(label(&doc), applied.label);
            }
        }
    }
    let synth = InstrumentKind::SubtractiveSynth.descriptors();
    for (param, info) in synth.iter().enumerate() {
        let other = if info.default == info.max {
            info.min
        } else {
            info.max
        };
        let applied = run(&mut doc, set_synth(channel, param, other));
        assert_eq!(applied.label, format!("Change {}", info.name));
    }
    let named = |id: &str| format!("Change {}", synth[param_index::<SynthParams>(id)].name);
    assert_eq!(named("filter.cutoffHz"), "Change Cutoff");
    assert_eq!(named("oscillators.1.level"), "Change Osc 2 level");
}

#[test]
fn replace_effect_puts_a_new_effect_in_the_same_place() {
    let mut doc = document();
    let track_id = add_mixer_track(&mut doc);
    let eq = add_effect(&mut doc, track_id, EffectKind::Eq);
    let delay = add_effect(&mut doc, track_id, EffectKind::Delay);
    let limiter = add_effect(&mut doc, track_id, EffectKind::Limiter);
    let feedback = param_index::<DelayParams>("feedback");
    run(&mut doc, set_param(track_id, delay, feedback, 0.25));
    run(
        &mut doc,
        Command::UpdateEffect {
            track: track_id,
            effect: delay,
            patch: EffectSlotPatch {
                enabled: Some(false),
                mix: Some(0.5),
            },
        },
    );
    let steps = doc.history().entries.len();

    let replace = |effect: EffectId, kind: EffectKind| Command::ReplaceEffect {
        track: track_id,
        effect,
        kind,
    };
    let applied = run(&mut doc, replace(delay, EffectKind::Reverb));
    assert_eq!(applied.label, "Replace effect");
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
    assert_eq!(doc.history().entries.len(), steps + 1);
    let reverb = EffectId(applied.created[0]);
    assert_ne!(reverb, delay);
    assert_eq!(effect_ids(&doc, track_id), [eq, reverb, limiter]);
    assert_eq!(
        slot(&doc, track_id, reverb),
        EffectSlot {
            impulse: None,
            id: reverb,
            enabled: true,
            mix: 1.0,
            params: EffectKind::Reverb.default_params(),
        }
    );

    // One undo brings the old effect back as it was, in its place.
    doc.undo().unwrap();
    assert_eq!(effect_ids(&doc, track_id), [eq, delay, limiter]);
    let old = slot(&doc, track_id, delay);
    assert!(!old.enabled);
    assert_eq!(old.mix, 0.5);
    doc.redo().unwrap();

    // The same kind again is a fresh effect with an id of its own.
    let decay = param_index::<ReverbParams>("decayS");
    run(&mut doc, set_param(track_id, reverb, decay, 9.0));
    let applied = run(&mut doc, replace(reverb, EffectKind::Reverb));
    let fresh = EffectId(applied.created[0]);
    assert_ne!(fresh, reverb);
    assert_eq!(
        slot(&doc, track_id, fresh).params,
        EffectKind::Reverb.default_params()
    );

    // A full chain has room for it: nothing is added.
    for _ in 3..MAX_EFFECT_SLOTS {
        add_effect(&mut doc, track_id, EffectKind::Eq);
    }
    let applied = run(&mut doc, replace(eq, EffectKind::Compressor));
    let first = effect_ids(&doc, track_id)[0];
    assert_eq!(first, EffectId(applied.created[0]));
    assert_eq!(effect_ids(&doc, track_id).len(), MAX_EFFECT_SLOTS);

    assert_eq!(
        fail(&mut doc, replace(EffectId(999), EffectKind::Eq)),
        not_found("effect", 999)
    );
    // The effect is looked for on the track that is named.
    let elsewhere = Command::ReplaceEffect {
        track: TrackId::MASTER,
        effect: fresh,
        kind: EffectKind::Eq,
    };
    assert_eq!(fail(&mut doc, elsewhere), not_found("effect", fresh.0));
    let nowhere = Command::ReplaceEffect {
        track: TrackId(999),
        effect: fresh,
        kind: EffectKind::Eq,
    };
    assert_eq!(fail(&mut doc, nowhere), not_found("mixer track", 999));
}

#[test]
fn remove_mixer_track_takes_its_effects_and_undo_brings_them_back() {
    let mut doc = document();
    let (_, track_id) = add_channel(&mut doc, "Pad");
    let reverb = add_effect(&mut doc, track_id, EffectKind::Reverb);
    let limiter = add_effect(&mut doc, track_id, EffectKind::Limiter);
    run(&mut doc, set_param(track_id, reverb, 0, 0.9));
    let master = add_effect(&mut doc, TrackId::MASTER, EffectKind::Eq);
    let before = track(&doc, track_id);

    // `run` undoes and redoes, and checks both.
    run(&mut doc, Command::RemoveMixerTrack { id: track_id });
    assert!(doc.project().mixer.track(track_id).is_none());
    assert_eq!(effect_ids(&doc, TrackId::MASTER), [master]);
    let gone = Command::RemoveEffect {
        track: track_id,
        effect: reverb,
    };
    assert_eq!(fail(&mut doc, gone), not_found("mixer track", track_id.0));

    doc.undo().unwrap();
    assert_eq!(track(&doc, track_id), before);
    assert_eq!(effect_ids(&doc, track_id), [reverb, limiter]);
}

#[test]
fn a_knob_drag_on_an_effect_is_one_undo_step() {
    let mut doc = document();
    let track_id = add_mixer_track(&mut doc);
    let effect = add_effect(&mut doc, track_id, EffectKind::Reverb);
    let decay = param_index::<ReverbParams>("decayS");
    let size = param_index::<ReverbParams>("size");
    let start = doc.project().clone();
    let steps = doc.history().entries.len();

    for value in [2.0, 2.5, 3.0, 4.25] {
        let applied = doc
            .dispatch(set_param(track_id, effect, decay, value), Some(7))
            .unwrap();
        assert_eq!(applied.touched, touched(|t| t.mixer = true));
    }
    // Another setting of the same effect under the same gesture, and the
    // slot's own mix.
    doc.dispatch(set_param(track_id, effect, size, 0.9), Some(7))
        .unwrap();
    let mix = Command::UpdateEffect {
        track: track_id,
        effect,
        patch: EffectSlotPatch {
            enabled: None,
            mix: Some(0.5),
        },
    };
    doc.dispatch(mix, Some(7)).unwrap();
    // The step is named after the setting the drag began on.
    assert_eq!(doc.history().entries.len(), steps + 1);
    assert_eq!(label(&doc), "Change Decay");
    let EffectParams::Reverb(reverb) = slot(&doc, track_id, effect).params else {
        panic!("not a reverb");
    };
    assert_eq!((reverb.decay_s, reverb.size), (4.25, 0.9));

    doc.undo().unwrap();
    assert_eq!(doc.project(), &start);
    doc.redo().unwrap();
    assert_eq!(slot(&doc, track_id, effect).mix, 0.5);

    // A drag that ends where it began leaves no step.
    let steps = doc.history().entries.len();
    for value in [9.0, 12.0, reverb.decay_s] {
        doc.dispatch(set_param(track_id, effect, decay, value), Some(8))
            .unwrap();
    }
    assert_eq!(doc.history().entries.len(), steps);

    // Without a gesture every change is a step of its own.
    for value in [2.0, 3.0] {
        doc.dispatch(set_param(track_id, effect, decay, value), None)
            .unwrap();
    }
    assert_eq!(doc.history().entries.len(), steps + 2);
}

// Instruments

fn synth(doc: &Document, channel: ChannelId) -> SynthParams {
    let channel = doc.project().channel(channel).expect("the channel exists");
    let ChannelSource::Instrument { params, .. } = &channel.source else {
        panic!("the channel is not an instrument");
    };
    let synth = match params {
        InstrumentParams::SubtractiveSynth(synth) => synth,
        _ => panic!("expected the subtractive synth"),
    };
    *synth
}

fn set_synth(channel: ChannelId, param: usize, value: f32) -> Command {
    Command::SetInstrumentParam {
        channel,
        param: param as u32,
        value,
    }
}

#[test]
fn add_channel_can_make_an_instrument_channel() {
    let mut doc = document();
    let applied = run(
        &mut doc,
        Command::AddChannel {
            name: None,
            sample: None,
            instrument: Some(InstrumentKind::SubtractiveSynth),
            index: None,
            mixer_track: None,
        },
    );
    assert_eq!(applied.label, "Add channel");
    assert_eq!(
        applied.touched,
        touched(|t| {
            t.channels = true;
            t.mixer = true;
        })
    );
    let (id, track_id) = (ChannelId(applied.created[0]), TrackId(applied.created[1]));
    let channel = doc.project().channel(id).unwrap().clone();
    // Named after the instrument, routed to a track of its own like any
    // other new channel, and at the instrument's default settings.
    assert_eq!(channel.name, "Subtractive synth");
    assert_eq!(channel.mixer_track, track_id);
    assert_eq!(track(&doc, track_id).name, "Subtractive synth");
    assert_eq!(track(&doc, track_id).output, Some(TrackId::MASTER));
    assert_eq!(channel.volume, DEFAULT_CHANNEL_VOLUME);
    assert_eq!(synth(&doc, id), SynthParams::default());
    assert_eq!(channel.source.sample(), None);

    let named = run(
        &mut doc,
        Command::AddChannel {
            name: Some("Lead".to_owned()),
            sample: None,
            instrument: Some(InstrumentKind::SubtractiveSynth),
            index: Some(0),
            mixer_track: Some(track_id),
        },
    );
    assert_eq!(named.created.len(), 1);
    assert_eq!(doc.project().channels[0].name, "Lead");
    assert_eq!(doc.project().channels[0].mixer_track, track_id);

    // A channel is a sampler or an instrument, never both.
    let sample = add_sample(&mut doc, "kick");
    let both = Command::AddChannel {
        name: None,
        sample: Some(sample),
        instrument: Some(InstrumentKind::SubtractiveSynth),
        index: None,
        mixer_track: None,
    };
    assert_invalid(fail(&mut doc, both), "a sample or an instrument, not both");
}

#[test]
fn an_instrument_channel_takes_notes_like_any_other() {
    let mut doc = document();
    let (channel, _) = add_instrument(&mut doc);
    run(
        &mut doc,
        Command::ToggleStep {
            pattern: PATTERN,
            channel,
            step: 2,
        },
    );
    add_notes(&mut doc, channel, vec![note(960, 72)]);
    assert_eq!(lane_notes(&doc, PATTERN, channel).len(), 2);
    // Its notes go with it, as a sampler's do.
    run(&mut doc, Command::RemoveChannel { id: channel });
    assert!(doc.project().patterns[0].lanes.is_empty());
}

#[test]
fn duplicate_channel_copies_an_instruments_settings() {
    let mut doc = document();
    let (channel, track_id) = add_instrument(&mut doc);
    let gain = param_index::<SynthParams>("gain");
    let mode = param_index::<SynthParams>("voiceMode");
    run(&mut doc, set_synth(channel, gain, 0.6));
    run(&mut doc, set_synth(channel, mode, 1.0));
    add_notes(&mut doc, channel, vec![note(0, 60), note(240, 64)]);

    let applied = run(&mut doc, Command::DuplicateChannel { id: channel });
    let copy = ChannelId(applied.created[0]);
    assert_eq!(synth(&doc, copy), synth(&doc, channel));
    assert_eq!(synth(&doc, copy).gain, 0.6);
    assert_eq!(synth(&doc, copy).voice_mode, VoiceMode::Mono);
    let copied = doc.project().channel(copy).unwrap();
    assert_eq!(copied.name, "Subtractive synth #2");
    assert_eq!(copied.mixer_track, track_id);
    assert_eq!(lane_notes(&doc, PATTERN, copy).len(), 2);

    // The two are separate from here on.
    run(&mut doc, set_synth(copy, gain, 0.1));
    assert_eq!(synth(&doc, channel).gain, 0.6);
}

#[test]
fn set_instrument_param_sets_one_setting_within_its_range() {
    let mut doc = document();
    let (channel, _) = add_instrument(&mut doc);
    let index = param_index::<SynthParams>;

    let applied = run(&mut doc, set_synth(channel, index("glideMs"), 120.0));
    assert_eq!(applied.label, "Change Glide");
    assert_eq!(applied.touched, touched(|t| t.channels = true));
    assert_eq!(
        synth(&doc, channel),
        SynthParams {
            glide_ms: 120.0,
            ..SynthParams::default()
        }
    );
    run(&mut doc, set_synth(channel, index("glideMs"), 1.0e9));
    assert_eq!(synth(&doc, channel).glide_ms, 2000.0);
    run(&mut doc, set_synth(channel, index("polyphony"), 3.4));
    assert_eq!(synth(&doc, channel).polyphony, 3);
    run(
        &mut doc,
        set_synth(channel, index("oscillators.1.level"), 0.5),
    );
    assert_eq!(synth(&doc, channel).oscillators[1].level, 0.5);
    run(&mut doc, set_synth(channel, index("filter.mode"), 1.0));
    assert_ne!(
        synth(&doc, channel).filter.mode,
        SynthParams::default().filter.mode
    );

    let before = synth(&doc, channel);
    assert_invalid(
        fail(&mut doc, set_synth(channel, index("gain"), f32::NAN)),
        "the value is not a number",
    );
    let count = InstrumentKind::SubtractiveSynth.descriptors().len();
    assert_invalid(
        fail(&mut doc, set_synth(channel, count, 0.0)),
        &format!("the Subtractive synth has no setting number {count}"),
    );
    assert_eq!(
        fail(&mut doc, set_synth(ChannelId(999), 0, 0.0)),
        not_found("channel", 999)
    );
    assert_eq!(synth(&doc, channel), before);
}

#[test]
fn every_setting_of_the_synth_can_be_set_by_its_index() {
    let mut doc = document();
    let (channel, _) = add_instrument(&mut doc);
    let kind = InstrumentKind::SubtractiveSynth;
    for (index, info) in kind.descriptors().iter().enumerate() {
        for value in [info.min, info.max, info.default] {
            run(&mut doc, set_synth(channel, index, value));
            let params = InstrumentParams::SubtractiveSynth(synth(&doc, channel));
            assert_eq!(params.get(index), Some(value), "{}", info.id);
        }
    }
    assert_eq!(synth(&doc, channel), SynthParams::default());
}

#[test]
fn set_instrument_params_replaces_every_setting() {
    let mut doc = document();
    let (channel, _) = add_instrument(&mut doc);
    let set = |params| Command::SetInstrumentParams {
        channel,
        params: InstrumentParams::SubtractiveSynth(params),
    };
    let mut preset = SynthParams {
        unison_voices: 5,
        glide_ms: 40.0,
        gain: 0.5,
        ..SynthParams::default()
    };
    preset.oscillators[2].level = 0.8;

    let applied = run(&mut doc, set(preset));
    assert_eq!(applied.label, "Change instrument settings");
    assert_eq!(applied.touched, touched(|t| t.channels = true));
    assert_eq!(synth(&doc, channel), preset);

    let wild = SynthParams {
        unison_voices: 200,
        gain: 50.0,
        pan: -3.0,
        ..preset
    };
    run(&mut doc, set(wild));
    assert_eq!(
        synth(&doc, channel),
        SynthParams {
            unison_voices: 7,
            gain: 2.0,
            pan: -1.0,
            ..preset
        }
    );

    let before = synth(&doc, channel);
    let broken = SynthParams {
        unison_spread: f32::NAN,
        ..preset
    };
    assert_invalid(fail(&mut doc, set(broken)), "the value is not a number");
    assert_eq!(synth(&doc, channel), before);
}

#[test]
fn sampler_and_instrument_commands_do_not_cross() {
    let mut doc = document();
    let sample = add_sample(&mut doc, "kick");
    let (drum, _) = add_channel(&mut doc, "Drum");
    let (lead, _) = add_instrument(&mut doc);
    let rename = ChannelPatch {
        name: Some("Lead".to_owned()),
        ..ChannelPatch::default()
    };
    run(&mut doc, update_channel(lead, rename));

    let on_instrument = [
        Command::SetChannelSample {
            id: lead,
            sample: Some(sample),
        },
        Command::SetChannelSample {
            id: lead,
            sample: None,
        },
        Command::UpdateSampler {
            id: lead,
            patch: SamplerPatch {
                gain: Some(0.5),
                ..SamplerPatch::default()
            },
        },
        Command::UpdateSampler {
            id: lead,
            patch: SamplerPatch::default(),
        },
        Command::SetSamplerEnvelope {
            id: lead,
            envelope: Some(Envelope::default()),
        },
        Command::SetSamplerEnvelope {
            id: lead,
            envelope: None,
        },
    ];
    let message = "the channel \"Lead\" plays an instrument, so it has no sampler settings";
    for command in on_instrument {
        assert_eq!(
            fail(&mut doc, command),
            CommandError::Invalid(message.to_owned())
        );
    }

    let on_sampler = [
        set_synth(drum, 0, 0.5),
        Command::SetInstrumentParams {
            channel: drum,
            params: InstrumentKind::SubtractiveSynth.default_params(),
        },
    ];
    let message = "the channel \"Drum\" is a sampler, so it has no instrument settings";
    for command in on_sampler {
        assert_eq!(
            fail(&mut doc, command),
            CommandError::Invalid(message.to_owned())
        );
    }

    // The commands every channel shares work on both.
    for id in [drum, lead] {
        let patch = ChannelPatch {
            volume: Some(0.5),
            pan: Some(-0.25),
            muted: Some(true),
            ..ChannelPatch::default()
        };
        run(&mut doc, update_channel(id, patch));
    }
    // An instrument channel uses no sample, so it holds none back.
    run(&mut doc, Command::RemoveSample { id: sample });
}

#[test]
fn a_knob_drag_on_an_instrument_is_one_undo_step() {
    let mut doc = document();
    let (channel, _) = add_instrument(&mut doc);
    let cutoff = param_index::<SynthParams>("filter.cutoffHz");
    let start = doc.project().clone();
    let steps = doc.history().entries.len();

    for value in [8_000.0, 4_000.0, 2_000.0, 500.0] {
        let applied = doc
            .dispatch(set_synth(channel, cutoff, value), Some(3))
            .unwrap();
        assert_eq!(applied.touched, touched(|t| t.channels = true));
    }
    assert_eq!(doc.history().entries.len(), steps + 1);
    assert_eq!(label(&doc), "Change Cutoff");
    assert_eq!(synth(&doc, channel).filter.cutoff_hz, 500.0);
    doc.undo().unwrap();
    assert_eq!(doc.project(), &start);
    doc.redo().unwrap();
    assert_eq!(synth(&doc, channel).filter.cutoff_hz, 500.0);

    // The fader of the same channel moved under the same gesture joins it,
    // and a gesture that ends where it began leaves no step.
    let steps = doc.history().entries.len();
    doc.dispatch(set_synth(channel, cutoff, 900.0), Some(4))
        .unwrap();
    doc.dispatch(volume(channel, 0.3), Some(4)).unwrap();
    doc.dispatch(set_synth(channel, cutoff, 500.0), Some(4))
        .unwrap();
    assert_eq!(doc.history().entries.len(), steps + 1);
    doc.dispatch(volume(channel, DEFAULT_CHANNEL_VOLUME), Some(4))
        .unwrap();
    assert_eq!(doc.history().entries.len(), steps);
}

// JSON of the new parts of the model

#[test]
fn a_track_with_effects_has_this_json() {
    let mut doc = document();
    let track_id = add_mixer_track(&mut doc);
    add_effect(&mut doc, track_id, EffectKind::Compressor);
    let limiter = add_effect(&mut doc, track_id, EffectKind::Limiter);
    run(
        &mut doc,
        Command::UpdateEffect {
            track: track_id,
            effect: limiter,
            patch: EffectSlotPatch {
                enabled: Some(false),
                mix: Some(0.5),
            },
        },
    );
    let json = serde_json::to_string_pretty(&track(&doc, track_id)).unwrap();
    assert_eq!(
        json,
        r#"{
  "current": false,
  "id": 2,
  "name": "Insert 1",
  "color": 15026253,
  "volume": 1.0,
  "pan": 0.0,
  "muted": false,
  "solo": false,
  "output": 0,
  "sends": [],
  "effects": [
    {
      "id": 3,
      "enabled": true,
      "mix": 1.0,
      "params": {
        "type": "compressor",
        "sidechain": false,
        "thresholdDb": -18.0,
        "ratio": 4.0,
        "attackMs": 10.0,
        "releaseMs": 120.0,
        "kneeDb": 6.0,
        "makeupDb": 0.0,
        "autoMakeup": false,
        "detector": "peak",
        "mix": 1.0
      }
    },
    {
      "id": 4,
      "enabled": false,
      "mix": 0.5,
      "params": {
        "type": "limiter",
        "ceilingDb": -0.3,
        "inputGainDb": 0.0,
        "releaseMs": 100.0,
        "lookaheadMs": 5.0,
        "mode": "limit",
        "thresholdDb": -18.0,
        "ratio": 4.0,
        "kneeDb": 6.0,
        "attackMs": 10.0,
        "gateThresholdDb": -90.0,
        "gateReleaseMs": 200.0
      }
    }
  ],
  "latencyOffsetMs": 0.0
}"#
    );
    let back: MixerTrack = serde_json::from_str(&json).unwrap();
    assert_eq!(back, track(&doc, track_id));
    // Every kind of effect is told apart by the `type` of its settings.
    for kind in EffectKind::ALL {
        let value = serde_json::to_value(kind.default_params()).unwrap();
        let tag = serde_json::to_value(kind).unwrap();
        assert_eq!(value["type"], tag, "{kind:?}");
    }
}

#[test]
fn an_instrument_channel_has_this_json() {
    let mut doc = document();
    let (channel, _) = add_instrument(&mut doc);
    let json = serde_json::to_string_pretty(doc.project().channel(channel).unwrap()).unwrap();
    assert_eq!(
        json,
        r#"{
  "voice": {
    "arpeggiator": {
      "mode": "off",
      "rate": "sixteenth",
      "gate": 0.75,
      "rangeOctaves": 1
    },
    "echo": {
      "enabled": false,
      "time": {
        "unit": "milliseconds",
        "ms": 250.0
      },
      "feedback": 0.5,
      "pitchSemitones": 0,
      "repeats": 3
    },
    "polyphony": {
      "maxVoices": 32,
      "monoLegato": false,
      "portamentoMs": 0.0
    },
    "envelopes": {
      "filter": {
        "enabled": false,
        "attackMs": 2.0,
        "decayMs": 200.0,
        "sustain": 0.8,
        "releaseMs": 150.0,
        "depth": 0.0
      },
      "pitch": {
        "enabled": false,
        "attackMs": 2.0,
        "decayMs": 200.0,
        "sustain": 0.8,
        "releaseMs": 150.0,
        "depth": 0.0
      },
      "pan": {
        "enabled": false,
        "attackMs": 2.0,
        "decayMs": 200.0,
        "sustain": 0.8,
        "releaseMs": 150.0,
        "depth": 0.0
      },
      "lfo": {
        "enabled": false,
        "shape": "sine",
        "target": "filter",
        "rateHz": 5.0,
        "depth": 0.0
      }
    }
  },
  "id": 2,
  "name": "Subtractive synth",
  "color": 15026253,
  "volume": 0.8,
  "pan": 0.0,
  "muted": false,
  "solo": false,
  "mixerTrack": 3,
  "source": {
    "type": "instrument",
    "params": {
      "type": "subtractiveSynth",
      "oscillators": [
        {
          "waveform": "saw",
          "level": 1.0,
          "coarse": 0,
          "fineCents": 0.0,
          "pulseWidth": 0.5,
          "pan": 0.0
        },
        {
          "waveform": "saw",
          "level": 0.0,
          "coarse": 0,
          "fineCents": 0.0,
          "pulseWidth": 0.5,
          "pan": 0.0
        },
        {
          "waveform": "saw",
          "level": 0.0,
          "coarse": 0,
          "fineCents": 0.0,
          "pulseWidth": 0.5,
          "pan": 0.0
        }
      ],
      "unisonVoices": 1,
      "unisonDetuneCents": 20.0,
      "unisonSpread": 0.7,
      "filter": {
        "mode": "lowPass",
        "slope": "db12",
        "cutoffHz": 20000.0,
        "resonance": 0.1,
        "keyTracking": 0.0,
        "envelopeOctaves": 0.0,
        "velocity": 0.0,
        "drive": 0.0
      },
      "ampEnvelope": {
        "attackMs": 2.0,
        "decayMs": 200.0,
        "sustain": 0.8,
        "releaseMs": 150.0
      },
      "filterEnvelope": {
        "attackMs": 2.0,
        "decayMs": 300.0,
        "sustain": 0.3,
        "releaseMs": 200.0
      },
      "lfos": [
        {
          "shape": "sine",
          "rateHz": 5.0,
          "pitchSemitones": 0.0,
          "cutoffOctaves": 0.0,
          "amp": 0.0,
          "pulseWidth": 0.0
        },
        {
          "shape": "sine",
          "rateHz": 5.0,
          "pitchSemitones": 0.0,
          "cutoffOctaves": 0.0,
          "amp": 0.0,
          "pulseWidth": 0.0
        }
      ],
      "voiceMode": "poly",
      "glideMs": 0.0,
      "polyphony": 16,
      "ampVelocity": 0.7,
      "gain": 0.25,
      "pan": 0.0
    }
  }
}"#
    );
    let back: Channel = serde_json::from_str(&json).unwrap();
    assert_eq!(&back, doc.project().channel(channel).unwrap());
}

#[test]
fn the_effect_and_instrument_commands_have_this_json() {
    let (track, effect) = (TrackId(3), EffectId(4));
    let cases = [
        (
            Command::AddEffect {
                track,
                kind: EffectKind::Reverb,
                index: None,
            },
            r#"{"type":"addEffect","track":3,"kind":"reverb","index":null}"#,
        ),
        (
            Command::RemoveEffect { track, effect },
            r#"{"type":"removeEffect","track":3,"effect":4}"#,
        ),
        (
            Command::MoveEffect {
                track,
                effect,
                to_track: Some(TrackId(0)),
                index: 2,
            },
            r#"{"type":"moveEffect","track":3,"effect":4,"toTrack":0,"index":2}"#,
        ),
        (
            Command::UpdateEffect {
                track,
                effect,
                patch: EffectSlotPatch {
                    enabled: Some(false),
                    mix: None,
                },
            },
            r#"{"type":"updateEffect","track":3,"effect":4,"patch":{"enabled":false,"mix":null}}"#,
        ),
        (
            Command::SetEffectParam {
                track,
                effect,
                param: 1,
                value: 2.5,
            },
            r#"{"type":"setEffectParam","track":3,"effect":4,"param":1,"value":2.5}"#,
        ),
        (
            Command::SetEffectParams {
                track,
                effect,
                params: EffectKind::Limiter.default_params(),
            },
            r#"{"type":"setEffectParams","track":3,"effect":4,"params":{"type":"limiter","ceilingDb":-0.3,"inputGainDb":0.0,"releaseMs":100.0,"lookaheadMs":5.0,"mode":"limit","thresholdDb":-18.0,"ratio":4.0,"kneeDb":6.0,"attackMs":10.0,"gateThresholdDb":-90.0,"gateReleaseMs":200.0}}"#,
        ),
        (
            Command::DuplicateEffect { track, effect },
            r#"{"type":"duplicateEffect","track":3,"effect":4}"#,
        ),
        (
            Command::SetInstrumentParam {
                channel: ChannelId(2),
                param: 0,
                value: 1.0,
            },
            r#"{"type":"setInstrumentParam","channel":2,"param":0,"value":1.0}"#,
        ),
        (
            Command::AddChannel {
                name: None,
                sample: None,
                instrument: Some(InstrumentKind::SubtractiveSynth),
                index: None,
                mixer_track: None,
            },
            r#"{"type":"addChannel","name":null,"sample":null,"instrument":"subtractiveSynth","index":null,"mixerTrack":null}"#,
        ),
    ];
    for (command, json) in cases {
        assert_eq!(serde_json::to_string(&command).unwrap(), json);
        assert_eq!(serde_json::from_str::<Command>(json).unwrap(), command);
    }
    let params = Command::SetInstrumentParams {
        channel: ChannelId(2),
        params: InstrumentKind::SubtractiveSynth.default_params(),
    };
    let json = serde_json::to_string(&params).unwrap();
    assert!(
        json.starts_with(
            r#"{"type":"setInstrumentParams","channel":2,"params":{"type":"subtractiveSynth","#
        ),
        "{json}"
    );
    assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), params);

    // Fields a caller may leave out.
    let short: Command =
        serde_json::from_str(r#"{"type":"moveEffect","track":3,"effect":4,"index":0}"#).unwrap();
    assert_eq!(
        short,
        Command::MoveEffect {
            track,
            effect,
            to_track: None,
            index: 0,
        }
    );
    let short: Command =
        serde_json::from_str(r#"{"type":"updateEffect","track":3,"effect":4,"patch":{"mix":0.5}}"#)
            .unwrap();
    assert_eq!(
        short,
        Command::UpdateEffect {
            track,
            effect,
            patch: EffectSlotPatch {
                enabled: None,
                mix: Some(0.5),
            },
        }
    );
    let short: Command =
        serde_json::from_str(r#"{"type":"addEffect","track":3,"kind":"eq"}"#).unwrap();
    assert_eq!(
        short,
        Command::AddEffect {
            track,
            kind: EffectKind::Eq,
            index: None,
        }
    );
}

// Document::relink_sample

fn project_path(name: &str) -> SamplePath {
    SamplePath::Project(name.to_owned())
}

fn add_project_sample(doc: &mut Document, name: &str) -> SampleId {
    let applied = run(
        doc,
        Command::AddSample {
            name: name.to_owned(),
            path: project_path(&format!("{name}.wav")),
        },
    );
    SampleId(applied.created[0])
}

fn path_of(doc: &Document, sample: SampleId) -> SamplePath {
    let sample = doc.project().sample(sample).expect("the sample exists");
    sample.path.clone()
}

#[test]
fn relink_sample_changes_the_path_without_a_history_step() {
    let mut doc = document();
    let kick = add_project_sample(&mut doc, "kick");
    let snare = add_project_sample(&mut doc, "snare");
    doc.mark_saved();
    let history = doc.history();

    let relinked = doc
        .relink_sample(kick, project_path("kick (2).wav"))
        .unwrap();
    assert_eq!(relinked, touched(|t| t.samples = true));
    assert_eq!(path_of(&doc, kick), project_path("kick (2).wav"));
    assert_eq!(path_of(&doc, snare), project_path("snare.wav"));
    assert_eq!(doc.history(), history);
    assert!(!doc.is_dirty());
    doc.project().check().unwrap();

    // The patch for it carries the samples, and nothing else of the project.
    let patch = doc.patch(&relinked);
    assert_eq!(patch.samples.as_deref(), Some(&doc.project().samples[..]));
    assert!(patch.channels.is_none() && patch.mixer.is_none() && patch.settings.is_none());
    assert!(!patch.dirty);

    // The same path again is no change at all.
    let again = doc
        .relink_sample(kick, project_path("kick (2).wav"))
        .unwrap();
    assert!(again.is_empty());

    // A document with unsaved changes stays that way too.
    add_project_sample(&mut doc, "hat");
    assert!(doc.is_dirty());
    doc.relink_sample(snare, project_path("drums/snare.wav"))
        .unwrap();
    assert!(doc.is_dirty());
    assert_eq!(doc.history().entries.len(), history.entries.len() + 1);
}

#[test]
fn undo_and_redo_bring_a_relinked_sample_back_under_its_new_path() {
    let mut doc = document();
    let kick = add_project_sample(&mut doc, "kick");
    let (channel, _) = add_channel(&mut doc, "Kick");
    run(
        &mut doc,
        Command::SetChannelSample {
            id: channel,
            sample: Some(kick),
        },
    );
    run(
        &mut doc,
        Command::SetChannelSample {
            id: channel,
            sample: None,
        },
    );
    // The history now adds the sample, uses it, lets go of it and removes
    // it, and the last two of those steps are undone.
    run(&mut doc, Command::RemoveSample { id: kick });
    doc.undo().unwrap();
    doc.undo().unwrap();
    let cursor = doc.history().cursor;
    let entries = doc.history().entries.len();

    let new_path = project_path("moved/kick.wav");
    doc.relink_sample(kick, new_path.clone()).unwrap();
    assert_eq!(doc.history().cursor, cursor);
    assert_eq!(doc.history().entries.len(), entries);

    // Forward, through the step that removes the sample, and back again.
    while doc.redo().is_some() {
        doc.project().check().unwrap();
    }
    assert!(doc.project().sample(kick).is_none());
    doc.undo().unwrap();
    assert_eq!(path_of(&doc, kick), new_path);
    // All the way back to before the sample was added, and forward again.
    while doc.undo().is_some() {
        doc.project().check().unwrap();
    }
    assert!(doc.project().samples.is_empty());
    doc.redo().unwrap();
    assert_eq!(path_of(&doc, kick), new_path);
    doc.jump(u32::MAX);
    doc.jump(cursor);
    assert_eq!(path_of(&doc, kick), new_path);
    doc.project().check().unwrap();
}

#[test]
fn sample_sources_include_removed_and_undone_history_and_relink_exact_paths() {
    let mut doc = document();
    let removed = add_project_sample(&mut doc, "removed");
    run(&mut doc, Command::RemoveSample { id: removed });
    let undone = add_project_sample(&mut doc, "undone");
    doc.undo().unwrap();
    assert!(doc.project().samples.is_empty());
    let sources = || {
        doc.sample_sources()
            .map(|(id, path)| (id, path.clone()))
            .collect::<Vec<_>>()
    };
    assert!(sources().contains(&(removed, project_path("removed.wav"))));
    assert!(sources().contains(&(undone, project_path("undone.wav"))));
    let history = doc.history();
    let dirty = doc.is_dirty();
    let before = doc.project().clone();
    assert_eq!(
        doc.relink_sample_source(
            undone,
            &project_path("another.wav"),
            project_path("moved.wav")
        )
        .unwrap_err(),
        not_found("sample", undone.0)
    );
    assert_eq!(doc.project(), &before);
    assert_eq!(doc.history(), history);
    assert_eq!(doc.is_dirty(), dirty);
    assert!(
        doc.relink_sample_source(
            undone,
            &project_path("undone.wav"),
            project_path("moved/undone.wav")
        )
        .unwrap()
        .is_empty()
    );
    assert!(
        doc.relink_sample_source(
            removed,
            &project_path("removed.wav"),
            project_path("moved/removed.wav")
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(doc.history(), history);
    assert_eq!(doc.is_dirty(), dirty);
    doc.redo().unwrap();
    assert_eq!(path_of(&doc, undone), project_path("moved/undone.wav"));
    doc.undo().unwrap();
    doc.undo().unwrap();
    assert_eq!(path_of(&doc, removed), project_path("moved/removed.wav"));
    doc.project().check().unwrap();
}

#[test]
fn a_sample_that_only_the_history_holds_can_be_relinked() {
    let mut doc = document();
    let kick = add_project_sample(&mut doc, "kick");
    doc.undo().unwrap();
    assert!(doc.project().samples.is_empty());

    let relinked = doc
        .relink_sample(kick, project_path("new/kick.wav"))
        .unwrap();
    assert!(relinked.is_empty(), "nothing in the project changed");
    doc.redo().unwrap();
    assert_eq!(path_of(&doc, kick), project_path("new/kick.wav"));
}

#[test]
fn relink_sample_refuses_what_would_break_the_project() {
    let mut doc = document();
    let kick = add_project_sample(&mut doc, "kick");
    let snare = add_project_sample(&mut doc, "snare");
    let old = add_project_sample(&mut doc, "old");
    run(&mut doc, Command::RemoveSample { id: old });
    let before = doc.project().clone();
    let history = doc.history();

    let mut refused = |id, path: SamplePath| {
        let error = doc.relink_sample(id, path).unwrap_err();
        assert_eq!(doc.project(), &before);
        assert_eq!(doc.history(), history);
        error
    };
    assert_eq!(
        refused(SampleId(999), project_path("x.wav")),
        not_found("sample", 999)
    );
    assert_invalid(
        refused(kick, project_path("../kick.wav")),
        "the sample path is not valid: a relative path must stay inside its folder",
    );
    assert_invalid(refused(kick, project_path("")), "the path is empty");
    // No two samples share a path, now or at any point of the history.
    assert_invalid(
        refused(kick, project_path("snare.wav")),
        "another sample of the project already has that path",
    );
    assert_invalid(
        refused(snare, project_path("old.wav")),
        "another sample of the project already has that path",
    );

    // Undoing the removal still gives a valid project.
    doc.undo().unwrap();
    doc.project().check().unwrap();
}

// Automation

fn add_automation(doc: &mut Document, target: AutomationTarget) -> AutomationId {
    let command = Command::AddAutomation {
        name: None,
        target,
        points: None,
    };
    AutomationId(run(doc, command).created[0])
}

fn automation(doc: &Document, id: AutomationId) -> Automation {
    let found = doc.project().automation(id);
    found.expect("the automation exists").clone()
}

fn point(tick: u32, value: f32) -> AutomationPoint {
    AutomationPoint {
        tick,
        value,
        curve: 0.0,
        hold: false,
    }
}

fn automation_clip(track: PlaylistTrackId, start: u32, id: AutomationId) -> ClipInit {
    ClipInit {
        track,
        start,
        length: None,
        offset: None,
        muted: None,
        content: ClipContent::Automation { automation: id },
    }
}

fn automation_ids(doc: &Document) -> Vec<AutomationId> {
    doc.project().automations.iter().map(|a| a.id).collect()
}

/// A project with one of everything an automation can move, and the
/// targets that name them.
fn automatable() -> (Document, Vec<AutomationTarget>) {
    let mut doc = document();
    let (kick, kick_track) = add_channel(&mut doc, "Kick");
    let (lead, _) = add_instrument(&mut doc);
    let bus = add_mixer_track(&mut doc);
    run(&mut doc, send(kick_track, bus, Some(0.5)));
    let reverb = add_effect(&mut doc, bus, EffectKind::Reverb);
    let decay = param_index::<ReverbParams>("decayS") as u32;
    let cutoff = param_index::<SynthParams>("filter.cutoffHz") as u32;
    let targets = vec![
        AutomationTarget::ChannelVolume { channel: kick },
        AutomationTarget::ChannelPan { channel: kick },
        AutomationTarget::TrackVolume { track: kick_track },
        AutomationTarget::TrackPan {
            track: TrackId::MASTER,
        },
        AutomationTarget::SendGain {
            track: kick_track,
            target: bus,
        },
        AutomationTarget::EffectParam {
            track: bus,
            effect: reverb,
            param: decay,
        },
        AutomationTarget::EffectMix {
            track: bus,
            effect: reverb,
        },
        AutomationTarget::InstrumentParam {
            channel: lead,
            param: cutoff,
        },
        AutomationTarget::Tempo,
    ];
    (doc, targets)
}

#[test]
fn default_automation_names_are_numbered_and_tell_their_targets_apart() {
    let mut doc = document();
    let add = |doc: &mut Document, target: AutomationTarget| {
        let command = Command::AddAutomation {
            name: None,
            target,
            points: None,
        };
        let id = AutomationId(run(doc, command).created[0]);
        automation(doc, id).name
    };

    // A second and a third automation of one target are numbered.
    let (kick, kick_track) = add_channel(&mut doc, "Kick");
    let pan = AutomationTarget::ChannelPan { channel: kick };
    assert_eq!(add(&mut doc, pan), "Kick pan");
    assert_eq!(add(&mut doc, pan), "Kick pan 2");
    assert_eq!(add(&mut doc, pan), "Kick pan 3");
    // A number that is free again is used again.
    let second = doc.project().automations[1].id;
    run(&mut doc, Command::RemoveAutomation { id: second });
    assert_eq!(add(&mut doc, pan), "Kick pan 2");
    assert_eq!(add(&mut doc, AutomationTarget::Tempo), "Tempo");
    assert_eq!(add(&mut doc, AutomationTarget::Tempo), "Tempo 2");

    // The channel and the mixer track of the same name.
    let volume = AutomationTarget::ChannelVolume { channel: kick };
    let fader = AutomationTarget::TrackVolume { track: kick_track };
    assert_eq!(add(&mut doc, volume), "Kick volume");
    assert_eq!(add(&mut doc, fader), "Kick track volume");
    let track_pan = AutomationTarget::TrackPan { track: kick_track };
    assert_eq!(add(&mut doc, track_pan), "Kick track pan");

    // Two reverbs on one track, and a third on another track.
    let decay = param_index::<ReverbParams>("decayS") as u32;
    let first = add_effect(&mut doc, kick_track, EffectKind::Reverb);
    add_effect(&mut doc, kick_track, EffectKind::Delay);
    let second = add_effect(&mut doc, kick_track, EffectKind::Reverb);
    let other = add_effect(&mut doc, TrackId::MASTER, EffectKind::Reverb);
    let setting = |track: TrackId, effect: EffectId| AutomationTarget::EffectParam {
        track,
        effect,
        param: decay,
    };
    let mix = |track: TrackId, effect: EffectId| AutomationTarget::EffectMix { track, effect };
    assert_eq!(
        add(&mut doc, setting(kick_track, first)),
        "Kick Reverb Decay"
    );
    assert_eq!(
        add(&mut doc, setting(kick_track, second)),
        "Kick Reverb 2 Decay"
    );
    assert_eq!(
        add(&mut doc, setting(TrackId::MASTER, other)),
        "Master Reverb Decay"
    );
    assert_eq!(add(&mut doc, mix(kick_track, first)), "Kick Reverb mix");
    assert_eq!(add(&mut doc, mix(kick_track, second)), "Kick Reverb 2 mix");
    assert_eq!(
        add(&mut doc, mix(TrackId::MASTER, other)),
        "Master Reverb mix"
    );

    // Two channels that the user gave one name still get two names.
    let (twin, _) = add_channel(&mut doc, "Kick");
    let twin_volume = AutomationTarget::ChannelVolume { channel: twin };
    assert_eq!(add(&mut doc, twin_volume), "Kick volume 2");

    // No two automations ended up with one name, and a name that is given
    // is taken as it is.
    let names: Vec<&str> = doc
        .project()
        .automations
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), names.len(), "{names:?}");
    let given = Command::AddAutomation {
        name: Some("Kick pan".to_owned()),
        target: pan,
        points: None,
    };
    let id = AutomationId(run(&mut doc, given).created[0]);
    assert_eq!(automation(&doc, id).name, "Kick pan");
    // The numbering does not mind capitals.
    let renamed = Command::UpdateAutomation {
        id,
        patch: AutomationPatch {
            name: Some("TEMPO 3".to_owned()),
            ..AutomationPatch::default()
        },
    };
    run(&mut doc, renamed);
    assert_eq!(add(&mut doc, AutomationTarget::Tempo), "Tempo 4");
}

#[test]
fn an_automation_starts_as_one_point_on_the_value_its_target_has() {
    let (mut doc, targets) = automatable();
    let kick = doc.project().channels[0].id;
    run(
        &mut doc,
        Command::UpdateChannel {
            id: kick,
            patch: ChannelPatch {
                pan: Some(0.5),
                ..ChannelPatch::default()
            },
        },
    );
    // Each says what kind of thing it moves: the channel Kick and the
    // mixer track Kick that was made for it do not share a name.
    let names = [
        "Kick volume",
        "Kick pan",
        "Kick track volume",
        "Master track pan",
        "Kick to Insert 3 send",
        "Insert 3 Reverb Decay",
        "Insert 3 Reverb mix",
        "Subtractive synth Cutoff",
        "Tempo",
    ];
    // What each target has now, as a share of its range: a channel volume
    // of 0.8 and a fader at 1 on the square taper, a send of 0.5, a decay
    // of 1.8 s between 0.1 and 20 on a logarithmic scale, and 120 bpm.
    let values = [
        (0.4_f32).sqrt(),
        0.75,
        (0.5_f32).sqrt(),
        0.5,
        0.5,
        ((1.8_f32 / 0.1).ln() / (200.0_f32).ln()),
        1.0,
        0.0,
        0.214_843_75,
    ];
    for (index, target) in targets.iter().enumerate() {
        let applied = run(
            &mut doc,
            Command::AddAutomation {
                name: None,
                target: *target,
                points: None,
            },
        );
        assert_eq!(applied.label, "Add automation");
        assert_eq!(applied.touched, touched(|t| t.automations = true));
        let id = AutomationId(applied.created[0]);
        let added = automation(&doc, id);
        assert_eq!(added.name, names[index]);
        assert_eq!(added.color, palette_color(index));
        assert_eq!(added.target, *target);
        let [only] = added.points[..] else {
            panic!("expected one point, got {:?}", added.points);
        };
        assert_eq!((only.tick, only.curve, only.hold), (0, 0.0, false));
        if index == 7 {
            // The cutoff, wherever the synth's default puts it.
            let range = doc.project().automation_range(target).unwrap();
            let stored = doc.project().automation_stored_value(target).unwrap();
            assert!((range.value(only.value) - stored).abs() < stored * 1e-4);
        } else {
            assert!(
                (only.value - values[index]).abs() < 1e-6,
                "{}: {}",
                names[index],
                only.value
            );
        }
        assert_eq!(doc.project().automations.last().unwrap().id, id);
    }
    assert_eq!(doc.project().automations.len(), targets.len());

    // A name of its own, and the way it is stored.
    let named = Command::AddAutomation {
        name: Some("Riser".to_owned()),
        target: targets[0],
        points: Some(vec![point(0, 0.25), point(960, 1.0)]),
    };
    let id = AutomationId(run(&mut doc, named).created[0]);
    let json = serde_json::to_value(automation(&doc, id)).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "id": id.0, "name": "Riser", "color": palette_color(9),
            "target": { "type": "channelVolume", "channel": kick.0 },
            "points": [
                { "tick": 0, "value": 0.25, "curve": 0.0, "hold": false },
                { "tick": 960, "value": 1.0, "curve": 0.0, "hold": false }
            ]
        })
    );
    let tempo = serde_json::to_value(AutomationTarget::Tempo).unwrap();
    assert_eq!(tempo, serde_json::json!({ "type": "tempo" }));
    // A point may leave its curve and hold out.
    let short: AutomationPoint = serde_json::from_str(r#"{ "tick": 5, "value": 0.5 }"#).unwrap();
    assert_eq!(short, point(5, 0.5));
}

#[test]
fn an_automation_needs_something_to_move() {
    let (mut doc, targets) = automatable();
    let before = doc.project().clone();
    let kick = before.channels[0].id;
    let lead = before.channels[1].id;
    let (kick_track, bus) = (before.mixer.tracks[1].id, before.mixer.tracks[3].id);
    let reverb = before.mixer.tracks[3].effects[0].id;
    let add = |target| Command::AddAutomation {
        name: None,
        target,
        points: None,
    };
    let missing = [
        (
            AutomationTarget::ChannelVolume {
                channel: ChannelId(900),
            },
            not_found("channel", 900),
        ),
        (
            AutomationTarget::ChannelPan {
                channel: ChannelId(900),
            },
            not_found("channel", 900),
        ),
        (
            AutomationTarget::TrackVolume {
                track: TrackId(900),
            },
            not_found("mixer track", 900),
        ),
        (
            AutomationTarget::TrackPan {
                track: TrackId(900),
            },
            not_found("mixer track", 900),
        ),
        (
            AutomationTarget::SendGain {
                track: TrackId(900),
                target: bus,
            },
            not_found("mixer track", 900),
        ),
        (
            AutomationTarget::SendGain {
                track: kick_track,
                target: TrackId(901),
            },
            not_found("mixer track", 901),
        ),
        (
            AutomationTarget::EffectMix {
                track: bus,
                effect: EffectId(900),
            },
            not_found("effect", 900),
        ),
        // The effect is on another track than the one named.
        (
            AutomationTarget::EffectMix {
                track: kick_track,
                effect: reverb,
            },
            not_found("effect", reverb.0),
        ),
        (
            AutomationTarget::EffectParam {
                track: TrackId(900),
                effect: reverb,
                param: 0,
            },
            not_found("mixer track", 900),
        ),
        (
            AutomationTarget::InstrumentParam {
                channel: ChannelId(900),
                param: 0,
            },
            not_found("channel", 900),
        ),
    ];
    for (target, error) in missing {
        assert_eq!(fail(&mut doc, add(target)), error, "{target:?}");
    }
    let invalid = [
        (
            AutomationTarget::SendGain {
                track: bus,
                target: kick_track,
            },
            "the mixer track \"Insert 3\" has no send to \"Kick\"",
        ),
        (
            AutomationTarget::EffectParam {
                track: bus,
                effect: reverb,
                param: 400,
            },
            "the Reverb has no setting number 400",
        ),
        (
            AutomationTarget::InstrumentParam {
                channel: lead,
                param: 400,
            },
            "the Subtractive synth has no setting number 400",
        ),
        (
            AutomationTarget::InstrumentParam {
                channel: kick,
                param: 0,
            },
            "is a sampler, so it has no instrument settings",
        ),
    ];
    for (target, words) in invalid {
        assert_invalid(fail(&mut doc, add(target)), words);
    }
    assert_eq!(doc.project(), &before);
    // Every one of the real targets has a range and a value.
    for target in &targets {
        assert!(before.automation_range(target).is_some(), "{target:?}");
        assert!(before.automation_stored_value(target).is_some());
    }
}

#[test]
fn a_curve_is_checked_and_its_values_brought_into_range() {
    let mut doc = document();
    let id = add_automation(&mut doc, AutomationTarget::Tempo);
    let set = |points| Command::SetAutomationPoints { id, points };
    let wild = vec![
        AutomationPoint {
            tick: 0,
            value: -3.0,
            curve: 9.0,
            hold: true,
        },
        AutomationPoint {
            tick: 0,
            value: 7.0,
            curve: -9.0,
            hold: false,
        },
        point(MAX_SONG_TICKS, 0.5),
    ];
    let applied = run(&mut doc, set(wild));
    assert_eq!(applied.label, "Change automation curve");
    assert_eq!(applied.touched, touched(|t| t.automations = true));
    assert!(applied.created.is_empty());
    assert_eq!(
        automation(&doc, id).points,
        [
            AutomationPoint {
                tick: 0,
                value: 0.0,
                curve: 1.0,
                hold: true,
            },
            AutomationPoint {
                tick: 0,
                value: 1.0,
                curve: -1.0,
                hold: false,
            },
            point(MAX_SONG_TICKS, 0.5),
        ]
    );

    assert_invalid(
        fail(&mut doc, set(Vec::new())),
        "an automation needs at least one point",
    );
    assert_invalid(
        fail(&mut doc, set(vec![point(960, 0.5), point(959, 0.5)])),
        "the points of an automation must be in order of time",
    );
    assert_invalid(
        fail(
            &mut doc,
            set(vec![point(0, 0.5), point(MAX_SONG_TICKS + 1, 0.5)]),
        ),
        "a point of the automation is past the end of the longest song, which has 1000000 beats",
    );
    assert_invalid(
        fail(&mut doc, set(vec![point(0, f32::NAN)])),
        "the value of a point is not a number",
    );
    let bent = AutomationPoint {
        curve: f32::NAN,
        ..point(0, 0.5)
    };
    assert_invalid(
        fail(&mut doc, set(vec![bent])),
        "the curve of a point is not a number",
    );
    let many = |count: usize| (0..count as u32).map(|tick| point(tick, 0.5)).collect();
    assert_invalid(
        fail(&mut doc, set(many(MAX_AUTOMATION_POINTS + 1))),
        "an automation can have 4096 points at most, not 4097",
    );
    run(&mut doc, set(many(MAX_AUTOMATION_POINTS)));
    assert_eq!(
        fail(
            &mut doc,
            Command::SetAutomationPoints {
                id: AutomationId(900),
                points: vec![point(0, 0.5)]
            }
        ),
        not_found("automation", 900)
    );
    // The same checks stand at the door of a new automation.
    let add = Command::AddAutomation {
        name: None,
        target: AutomationTarget::Tempo,
        points: Some(Vec::new()),
    };
    assert_invalid(fail(&mut doc, add), "at least one point");

    // Dragging a point is one undo step, however many moves it takes.
    let before = doc.project().clone();
    let steps = doc.history().entries.len();
    for value in [0.1, 0.2, 0.3] {
        let drag = set(vec![point(0, value), point(960, 1.0)]);
        doc.dispatch(drag, Some(11)).unwrap();
    }
    assert_eq!(doc.history().entries.len(), steps + 1);
    assert_eq!(automation(&doc, id).points[0].value, 0.3);
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
    // A curve set to what it is changes nothing.
    let history = doc.history();
    let same = automation(&doc, id).points;
    let applied = run(&mut doc, set(same));
    assert!(applied.touched.is_empty());
    assert_eq!(doc.history(), history);
}

#[test]
fn an_automation_is_renamed_recolored_and_duplicated() {
    let (mut doc, targets) = automatable();
    let first = add_automation(&mut doc, targets[0]);
    let second = add_automation(&mut doc, AutomationTarget::Tempo);
    let update = |id, patch| Command::UpdateAutomation { id, patch };
    let applied = run(
        &mut doc,
        update(
            first,
            AutomationPatch {
                name: Some("Fade".to_owned()),
                color: None,
            },
        ),
    );
    assert_eq!(applied.label, "Rename automation");
    assert_eq!(applied.touched, touched(|t| t.automations = true));
    let applied = run(
        &mut doc,
        update(
            first,
            AutomationPatch {
                name: None,
                color: Some(0x112233),
            },
        ),
    );
    assert_eq!(applied.label, "Change automation color");
    let changed = automation(&doc, first);
    assert_eq!((changed.name.as_str(), changed.color), ("Fade", 0x112233));
    let bad = AutomationPatch {
        name: None,
        color: Some(0x1_000_000),
    };
    assert_invalid(fail(&mut doc, update(first, bad)), "not a 0xRRGGBB color");
    assert_eq!(
        fail(
            &mut doc,
            update(AutomationId(900), AutomationPatch::default())
        ),
        not_found("automation", 900)
    );

    // A copy has the target and the curve, a name of its own and no clips.
    let lane = add_playlist_track(&mut doc);
    run(
        &mut doc,
        Command::SetAutomationPoints {
            id: first,
            points: vec![point(0, 0.0), point(480, 1.0)],
        },
    );
    run(
        &mut doc,
        Command::AddClips {
            clips: vec![automation_clip(lane, 0, first)],
        },
    );
    let applied = run(&mut doc, Command::DuplicateAutomation { id: first });
    assert_eq!(applied.label, "Duplicate automation");
    assert_eq!(applied.touched, touched(|t| t.automations = true));
    let copy = AutomationId(applied.created[0]);
    assert_eq!(automation_ids(&doc), [first, copy, second]);
    assert_eq!(
        automation(&doc, copy),
        Automation {
            id: copy,
            name: "Fade #2".to_owned(),
            ..automation(&doc, first)
        }
    );
    assert_eq!(doc.project().playlist.clips.len(), 1);
    assert_eq!(
        fail(
            &mut doc,
            Command::DuplicateAutomation {
                id: AutomationId(900)
            }
        ),
        not_found("automation", 900)
    );
}

#[test]
fn an_automation_clip_is_a_window_onto_its_curve() {
    let mut doc = document();
    let lane = add_playlist_track(&mut doc);
    let id = add_automation(&mut doc, AutomationTarget::Tempo);
    let add = |init: ClipInit| Command::AddClips { clips: vec![init] };

    // One point: the clip is a bar long, so there is something to hold.
    let applied = run(&mut doc, add(automation_clip(lane, 0, id)));
    assert_eq!(applied.label, "Add clip");
    assert_eq!(applied.touched, touched(|t| t.playlist = true));
    let short = ClipId(applied.created[0]);
    assert_eq!(clip_of(&doc, short).length, 3_840);
    assert_eq!(
        clip_of(&doc, short).content,
        ClipContent::Automation { automation: id }
    );

    // A longer curve: the clip shows all of it.
    run(
        &mut doc,
        Command::SetAutomationPoints {
            id,
            points: vec![point(0, 0.2), point(9_600, 0.4)],
        },
    );
    let whole = ClipId(run(&mut doc, add(automation_clip(lane, 3_840, id))).created[0]);
    assert_eq!(clip_of(&doc, whole).length, 9_600);
    // In 3/4 a bar is three beats.
    run(
        &mut doc,
        Command::UpdateSettings {
            patch: SettingsPatch {
                time_signature: Some(TimeSignature {
                    numerator: 3,
                    denominator: 4,
                }),
                ..SettingsPatch::default()
            },
        },
    );
    let other = add_automation(&mut doc, AutomationTarget::Tempo);
    let waltz = ClipId(run(&mut doc, add(automation_clip(lane, 0, other))).created[0]);
    assert_eq!(clip_of(&doc, waltz).length, 2_880);

    // A window of its own, onto the same curve, as many times as wanted.
    let window = ClipInit {
        length: Some(960),
        offset: Some(4_800),
        ..automation_clip(lane, 20_000, id)
    };
    let part = ClipId(run(&mut doc, add(window)).created[0]);
    let placed = clip_of(&doc, part);
    assert_eq!((placed.length, placed.offset), (960, 4_800));
    let json = serde_json::to_value(&placed.content).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "type": "automation", "automation": id.0 })
    );
    assert_eq!(
        fail(&mut doc, add(automation_clip(lane, 0, AutomationId(900)))),
        not_found("automation", 900)
    );
    // It is moved and trimmed like any clip, and is not an audio clip.
    let trim = ClipPatch {
        length: Some(100),
        offset: Some(50),
        ..ClipPatch::default()
    };
    run(&mut doc, update_clips(vec![(part, trim)]));
    let audio_patch = AudioClipUpdate {
        id: part,
        patch: AudioClipPatch::default(),
    };
    let update = Command::UpdateAudioClips {
        updates: vec![audio_patch],
    };
    assert_invalid(fail(&mut doc, update), "is not an audio clip");

    // Deleting the automation takes every clip of it, and nothing else.
    let before = doc.project().clone();
    let applied = run(&mut doc, Command::RemoveAutomation { id });
    assert_eq!(applied.label, "Delete automation");
    assert_eq!(
        applied.touched,
        touched(|t| {
            t.automations = true;
            t.playlist = true;
        })
    );
    assert_eq!(automation_ids(&doc), [other]);
    let left: Vec<ClipId> = doc.project().playlist.clips.iter().map(|c| c.id).collect();
    assert_eq!(left, [waltz]);
    doc.undo().unwrap();
    assert!(same_content(doc.project(), &before));
    assert_eq!(
        fail(
            &mut doc,
            Command::RemoveAutomation {
                id: AutomationId(900)
            }
        ),
        not_found("automation", 900)
    );
    // An automation no clip shows leaves the playlist alone.
    let spare = add_automation(&mut doc, AutomationTarget::Tempo);
    let applied = run(&mut doc, Command::RemoveAutomation { id: spare });
    assert_eq!(applied.touched, touched(|t| t.automations = true));
}

#[test]
fn what_an_automation_moves_takes_the_automation_with_it() {
    let (mut doc, targets) = automatable();
    let lane = add_playlist_track(&mut doc);
    let ids: Vec<AutomationId> = targets
        .iter()
        .map(|target| add_automation(&mut doc, *target))
        .collect();
    let clips: Vec<ClipId> = ids
        .iter()
        .map(|id| {
            let add = Command::AddClips {
                clips: vec![automation_clip(lane, 0, *id)],
            };
            ClipId(run(&mut doc, add).created[0])
        })
        .collect();
    let pattern_clip = add_clip(&mut doc, lane, PATTERN, 0);
    let start = doc.project().clone();
    let kick = start.channels[0].id;
    let lead = start.channels[1].id;
    let (kick_track, bus) = (start.mixer.tracks[1].id, start.mixer.tracks[3].id);
    let reverb = start.mixer.tracks[3].effects[0].id;

    // What is left after a command, as indices into `targets`.
    let left = |doc: &Document| -> Vec<usize> {
        let automations = doc.project().automations.iter();
        automations
            .map(|a| ids.iter().position(|id| *id == a.id).unwrap())
            .collect()
    };
    let clips_left = |doc: &Document| -> Vec<usize> {
        let playlist = doc.project().playlist.clips.iter();
        playlist
            .filter_map(|clip| clips.iter().position(|id| *id == clip.id))
            .collect()
    };
    let cases: [(Command, &[usize], &str); 6] = [
        // The channel: its volume and pan.
        (
            Command::RemoveChannel { id: kick },
            &[0, 1],
            "Delete channel",
        ),
        // The instrument channel: its cutoff.
        (Command::RemoveChannel { id: lead }, &[7], "Delete channel"),
        // The track the kick plays into: its fader, and the send from it.
        (
            Command::RemoveMixerTrack { id: kick_track },
            &[2, 4],
            "Delete mixer track",
        ),
        // The bus: the send to it, and its effect's setting and mix.
        (
            Command::RemoveMixerTrack { id: bus },
            &[4, 5, 6],
            "Delete mixer track",
        ),
        (
            Command::RemoveEffect {
                track: bus,
                effect: reverb,
            },
            &[5, 6],
            "Delete effect",
        ),
        (send(kick_track, bus, None), &[4], "Remove send"),
    ];
    for (command, gone, label) in cases {
        let applied = run(&mut doc, command.clone());
        assert_eq!(applied.label, label);
        assert!(applied.touched.automations && applied.touched.playlist);
        let expected: Vec<usize> = (0..targets.len()).filter(|i| !gone.contains(i)).collect();
        assert_eq!(left(&doc), expected, "{command:?}");
        assert_eq!(clips_left(&doc), expected, "{command:?}");
        // The clip of the pattern, and the project, are otherwise whole.
        assert!(
            doc.project()
                .playlist
                .clips
                .iter()
                .any(|c| c.id == pattern_clip)
        );
        doc.project().check().unwrap();
        // One undo brings back the automations, their clips and the rest.
        doc.undo().unwrap();
        assert!(same_content(doc.project(), &start), "{command:?}");
    }

    // An effect that is replaced is another effect.
    let replace = Command::ReplaceEffect {
        track: bus,
        effect: reverb,
        kind: EffectKind::Reverb,
    };
    run(&mut doc, replace);
    assert_eq!(left(&doc), [0, 1, 2, 3, 4, 7, 8]);
    doc.undo().unwrap();

    // Changing a send's level, muting, renaming and moving things inside
    // the project leave every automation alone.
    let harmless = [
        send(kick_track, bus, Some(1.5)),
        Command::MoveChannel { id: kick, index: 1 },
        Command::UpdateMixerTrack {
            id: bus,
            patch: MixerTrackPatch {
                muted: Some(true),
                ..MixerTrackPatch::default()
            },
        },
        Command::SetEffectParam {
            track: bus,
            effect: reverb,
            param: 1,
            value: 9.0,
        },
    ];
    for command in harmless {
        let applied = run(&mut doc, command);
        assert!(!applied.touched.automations && !applied.touched.playlist);
    }
    assert_eq!(left(&doc).len(), targets.len());
}

#[test]
fn an_effect_that_moves_to_another_track_keeps_its_automations() {
    let (mut doc, targets) = automatable();
    let bus = doc.project().mixer.tracks[3].id;
    let kick_track = doc.project().mixer.tracks[1].id;
    let reverb = doc.project().mixer.tracks[3].effects[0].id;
    let setting = add_automation(&mut doc, targets[5]);
    let mix = add_automation(&mut doc, targets[6]);
    let other = add_automation(&mut doc, targets[0]);
    let before = doc.project().clone();

    // Within its own chain nothing about the automations changes.
    let eq = add_effect(&mut doc, bus, EffectKind::Eq);
    let within = Command::MoveEffect {
        track: bus,
        effect: reverb,
        to_track: None,
        index: 1,
    };
    let applied = run(&mut doc, within);
    assert_eq!(applied.touched, touched(|t| t.mixer = true));
    assert_eq!(effect_ids(&doc, bus), [eq, reverb]);

    let across = Command::MoveEffect {
        track: bus,
        effect: reverb,
        to_track: Some(kick_track),
        index: 0,
    };
    let applied = run(&mut doc, across);
    assert_eq!(applied.label, "Move effect");
    assert_eq!(
        applied.touched,
        touched(|t| {
            t.mixer = true;
            t.automations = true;
        })
    );
    let param = param_index::<ReverbParams>("decayS") as u32;
    assert_eq!(
        automation(&doc, setting).target,
        AutomationTarget::EffectParam {
            track: kick_track,
            effect: reverb,
            param,
        }
    );
    assert_eq!(
        automation(&doc, mix).target,
        AutomationTarget::EffectMix {
            track: kick_track,
            effect: reverb,
        }
    );
    assert_eq!(
        automation(&doc, other),
        before.automation(other).unwrap().clone()
    );
    doc.project().check().unwrap();

    // And back again, to exactly where they were.
    let back = Command::MoveEffect {
        track: kick_track,
        effect: reverb,
        to_track: Some(bus),
        index: 0,
    };
    run(&mut doc, back);
    assert_eq!(doc.project().automations, before.automations);
    // A drag across tracks and on under one gesture is one undo step,
    // which puts the automations back where they were as well.
    let start = doc.project().clone();
    let steps = doc.history().entries.len();
    let hop = |track, to| Command::MoveEffect {
        track,
        effect: reverb,
        to_track: Some(to),
        index: 0,
    };
    doc.dispatch(hop(bus, kick_track), Some(21)).unwrap();
    doc.dispatch(hop(kick_track, TrackId::MASTER), Some(21))
        .unwrap();
    assert_eq!(doc.history().entries.len(), steps + 1);
    assert_eq!(
        automation(&doc, mix).target,
        AutomationTarget::EffectMix {
            track: TrackId::MASTER,
            effect: reverb,
        }
    );
    doc.undo().unwrap();
    assert_eq!(doc.project(), &start);
}

#[test]
fn check_names_each_broken_rule_of_an_automation() {
    let (mut doc, targets) = automatable();
    let lane = add_playlist_track(&mut doc);
    for target in &targets {
        let id = add_automation(&mut doc, *target);
        run(
            &mut doc,
            Command::AddClips {
                clips: vec![automation_clip(lane, 0, id)],
            },
        );
    }
    let first = doc.project().automations[0].id;
    run(
        &mut doc,
        Command::SetAutomationPoints {
            id: first,
            points: vec![point(0, 0.0), point(960, 1.0)],
        },
    );
    let valid = doc.project().clone();
    valid.check().unwrap();

    let cases: [(&str, Damage); 17] = [
        ("more than one automation", |p| {
            p.automations[1].id = p.automations[0].id;
        }),
        ("is not below the next id", |p| {
            p.automations[0].id = AutomationId(p.next_id);
        }),
        ("not 0xRRGGBB", |p| p.automations[0].color = 0x1_000_000),
        ("has no points", |p| p.automations[0].points.clear()),
        ("does not keep its points in order", |p| {
            p.automations[0].points.swap(0, 1);
        }),
        ("a point past the end of the longest song", |p| {
            p.automations[0].points[1].tick = MAX_SONG_TICKS + 1;
        }),
        ("a point with value 1.5", |p| {
            p.automations[0].points[0].value = 1.5;
        }),
        ("a point with value NaN", |p| {
            p.automations[0].points[0].value = f32::NAN;
        }),
        ("a point with curve -2", |p| {
            p.automations[0].points[0].curve = -2.0;
        }),
        ("more than the limit of 4096", |p| {
            p.automations[0].points = vec![p.automations[0].points[0]; 4_097];
        }),
        // What each kind of target needs.
        ("moves something the project does not have", |p| {
            p.channels.remove(0);
            p.patterns[0].lanes.clear();
        }),
        ("moves something the project does not have", |p| {
            p.mixer.tracks[1].sends.clear();
        }),
        ("moves something the project does not have", |p| {
            p.mixer.tracks[3].effects.clear();
        }),
        ("moves something the project does not have", |p| {
            let AutomationTarget::EffectParam { param, .. } = &mut p.automations[5].target else {
                panic!("not an effect setting");
            };
            *param = 400;
        }),
        ("moves something the project does not have", |p| {
            // The effect is there, on another track than the target says.
            let effect = p.mixer.tracks[3].effects.remove(0);
            p.mixer.tracks[2].effects.push(effect);
        }),
        ("moves something the project does not have", |p| {
            let AutomationTarget::InstrumentParam { channel, .. } = &mut p.automations[7].target
            else {
                panic!("not an instrument setting");
            };
            // A sampler has no settings to move.
            *channel = p.channels[0].id;
        }),
        ("shows automation 999", |p| {
            let clip = p.playlist.clips.iter_mut();
            let shown = clip.filter_map(|clip| match &mut clip.content {
                ClipContent::Automation { automation } => Some(automation),
                _ => None,
            });
            for automation in shown.take(1) {
                *automation = AutomationId(999);
            }
        }),
    ];
    for (index, (words, damage)) in cases.into_iter().enumerate() {
        let mut project = valid.clone();
        damage(&mut project);
        match project.check() {
            Ok(()) => panic!("case {index} (\"{words}\") passed the check"),
            Err(problem) => assert!(
                problem.contains(words),
                "case {index}: \"{problem}\" lacks \"{words}\""
            ),
        }
    }
}

#[test]
fn spectral_settings_load_old_audio_and_round_trip_with_undo() {
    let mut doc = Document::new(Project::new("spectral"));
    let lane = add_playlist_track(&mut doc);
    let sample = add_sample(&mut doc, "spectral");
    let id = add_audio_clip(&mut doc, lane, audio(sample, TrackId::MASTER));
    let before = doc.project().clone();
    let mut old = serde_json::to_value(&before).unwrap();
    old["playlist"]["clips"][0]["content"]
        .as_object_mut()
        .unwrap()
        .remove("stretch");
    let loaded: Project = serde_json::from_value(old).unwrap();
    assert_eq!(loaded, before);
    let settings = ClipStretch::Spectral {
        ratio: 1.5,
        quality: ClipStretchQuality::High,
        formants: true,
    };
    run(
        &mut doc,
        Command::UpdateAudioClips {
            updates: vec![AudioClipUpdate {
                id,
                patch: AudioClipPatch {
                    stretch: Some(settings),
                    pitch: Some(12.0),
                    ..Default::default()
                },
            }],
        },
    );
    doc.project().check().unwrap();
    let encoded = serde_json::to_vec(doc.project()).unwrap();
    let loaded: Project = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(&loaded, doc.project());
    let after = doc.project().clone();
    doc.undo().unwrap();
    assert_eq!(doc.project(), &before);
    doc.redo().unwrap();
    assert_eq!(doc.project(), &after);
    for ratio in [0.0, 0.249, 4.001, f64::NAN] {
        assert!(
            doc.dispatch(
                Command::UpdateAudioClips {
                    updates: vec![AudioClipUpdate {
                        id,
                        patch: AudioClipPatch {
                            stretch: Some(ClipStretch::Spectral {
                                ratio,
                                quality: ClipStretchQuality::Fast,
                                formants: false
                            }),
                            ..Default::default()
                        }
                    }]
                },
                None
            )
            .is_err()
        );
        assert_eq!(doc.project(), &after);
    }
}
