//! Edits: patches, undo, the dirty flag, and samples that come in with an
//! edit.

use std::fs;

use windfall_codec::{WavSampleFormat, write_wav};
use windfall_core::AudioBuffer;
use windfall_ipc::TransportPatch;
use windfall_project::{
    ChannelId, Command, PatternId, ProjectPatch, SampleId, SamplePath, SettingsPatch, TrackId,
};

use super::{Rig, factory_file, rms};
use crate::events::Event;

fn add_pattern() -> Command {
    Command::AddPattern { name: None }
}

fn rename(name: &str) -> Command {
    Command::UpdateSettings {
        patch: SettingsPatch {
            name: Some(name.to_owned()),
            ..SettingsPatch::default()
        },
    }
}

/// Writes a short audio file into the rig's folder and returns its path.
fn write_tone(rig: &Rig, name: &str) -> String {
    let path = rig.file(name);
    let buffer = AudioBuffer::from_interleaved(44_100, 1, vec![0.5; 2_205]);
    write_wav(&path, &buffer, WavSampleFormat::Int16).unwrap();
    path
}

fn sample_of(rig: &Rig, channel: usize) -> Option<SampleId> {
    rig.project().channels[channel].source.sample()
}

#[test]
fn every_change_sends_one_patch_and_revisions_count_up_by_one() {
    let rig = Rig::new();
    let session = &rig.session;
    let mut returned: Vec<ProjectPatch> = Vec::new();

    returned.push(session.dispatch(add_pattern(), None).unwrap().patch);
    let toggled = session
        .dispatch(
            Command::ToggleStep {
                pattern: rig.pattern(),
                channel: rig.channel(0),
                step: 3,
            },
            None,
        )
        .unwrap();
    assert_eq!(toggled.created.len(), 1);
    returned.push(toggled.patch);
    returned.push(session.undo().unwrap());
    returned.push(session.redo().unwrap());
    returned.push(session.history_jump(0));
    // A jump past the end of the history goes to its end.
    returned.push(session.history_jump(99));
    // A command that changes nothing still answers with a patch.
    let same_name = rig.project().settings.name;
    returned.push(session.dispatch(rename(&same_name), None).unwrap().patch);
    session.project_save(Some(&rig.file("counted"))).unwrap();
    returned.push(session.dispatch(rename("Counted"), Some(7)).unwrap().patch);
    returned.push(
        session
            .dispatch(rename("Counted twice"), Some(7))
            .unwrap()
            .patch,
    );

    let sent = rig.take_patches();
    let revisions: Vec<u64> = sent.iter().map(|patch| patch.revision).collect();
    assert_eq!(revisions, (1..=10).collect::<Vec<u64>>());
    // The save is the one patch no call returned.
    let mut sent_by_calls = sent.clone();
    let save = sent_by_calls.remove(7);
    assert_eq!(sent_by_calls, returned);
    assert!(!save.dirty);
    assert!(save.settings.is_none() && save.patterns.is_empty());

    assert_eq!(session.document_snapshot().revision, 10);
    // One gesture is one undo step.
    assert_eq!(sent[9].history.cursor, sent[7].history.cursor + 1);
    assert_eq!(sent[8].history, sent[9].history);

    // Only the sections that changed travel.
    assert!(sent[0].pattern_order.is_some() && sent[0].channels.is_none());
    assert_eq!(sent[1].patterns.len(), 1);
    assert!(sent[1].pattern_order.is_none());
    assert!(sent[6].settings.is_none());
    assert_eq!(sent[8].settings.as_ref().unwrap().name, "Counted");
}

#[test]
fn a_command_that_fails_changes_nothing_and_sends_nothing() {
    let rig = Rig::new();
    let before = rig.session.document_snapshot();

    let error = rig
        .session
        .dispatch(
            Command::RemoveChannel {
                id: ChannelId(12_345),
            },
            None,
        )
        .unwrap_err();
    assert_eq!(
        windfall_core::english::english(&error),
        "channel 12345 does not exist"
    );
    assert_eq!(rig.session.undo(), None);
    assert_eq!(rig.session.redo(), None);

    assert_eq!(rig.session.document_snapshot(), before);
    assert!(rig.events.take().is_empty());
}

#[test]
fn a_value_out_of_range_is_clamped_as_the_document_does() {
    let rig = Rig::new();
    let patch = rig
        .session
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    tempo_bpm: Some(9_999.0),
                    ..SettingsPatch::default()
                },
            },
            None,
        )
        .unwrap()
        .patch;
    assert_eq!(
        patch.settings.unwrap().tempo_bpm,
        windfall_project::MAX_TEMPO_BPM
    );
}

#[test]
fn the_dirty_flag_follows_edits_saves_and_undo() {
    let rig = Rig::new();
    let session = &rig.session;
    let dirty = || session.document_snapshot().dirty;
    assert!(!dirty());

    assert!(session.dispatch(rename("One"), None).unwrap().patch.dirty);
    session.project_save(Some(&rig.file("dirty"))).unwrap();
    assert!(!dirty());
    // Every window hears that the project is clean.
    assert!(!rig.take_patches().last().unwrap().dirty);

    assert!(session.dispatch(rename("Two"), None).unwrap().patch.dirty);
    // Back at what was saved.
    assert!(!session.undo().unwrap().dirty);
    // Before what was saved.
    assert!(session.undo().unwrap().dirty);
    assert!(!session.redo().unwrap().dirty);
    assert!(session.redo().unwrap().dirty);

    // Saving again with no path uses the file the project has.
    session.project_save(None).unwrap();
    assert!(!dirty());
    assert!(session.history_jump(0).dirty);
}

#[test]
fn adding_a_channel_from_a_file_is_one_undo_step() {
    let rig = Rig::new();
    let session = &rig.session;
    let before = rig.project();
    let file = factory_file("Drums/Percussion/Cowbell.wav");

    let result = session.add_channel_from_file(&file, Some(0)).unwrap();
    let [sample, channel, track] = result.created[..] else {
        panic!("created {:?}", result.created);
    };
    let (sample, channel, track) = (SampleId(sample), ChannelId(channel), TrackId(track));
    let labels = |patch: &ProjectPatch| -> Vec<String> {
        patch
            .history
            .entries
            .iter()
            .map(|e| e.label.clone())
            .collect()
    };
    assert_eq!(labels(&result.patch), ["Add channel"]);
    assert_eq!(rig.take_patches(), std::slice::from_ref(&result.patch));

    let project = rig.project();
    assert_eq!(project.channels.len(), 5);
    assert_eq!(project.channels[0].id, channel);
    assert_eq!(project.channels[0].name, "Cowbell");
    assert_eq!(project.channels[0].mixer_track, track);
    assert_eq!(project.mixer.track(track).unwrap().name, "Cowbell");
    assert_eq!(sample_of(&rig, 0), Some(sample));
    let asset = project.sample(sample).unwrap();
    assert_eq!(asset.name, "Cowbell");
    assert_eq!(
        asset.path,
        SamplePath::Factory("Drums/Percussion/Cowbell.wav".to_owned())
    );
    assert!(rig.has_audio(sample));

    // One undo takes all of it away, and the engine lets go of the audio.
    let undone = session.undo().unwrap();
    assert_eq!(undone.history.cursor, 0);
    let mut expected = before.clone();
    // Ids that were handed out are never handed out again.
    expected.next_id = rig.project().next_id;
    assert_eq!(rig.project(), expected);
    assert!(!rig.has_audio(sample));

    // Redo brings back the same ids, and the audio with them at once.
    session.redo().unwrap();
    assert_eq!(rig.project(), project);
    assert!(rig.has_audio(sample));
}

#[test]
fn a_file_the_project_already_uses_is_not_added_again() {
    let rig = Rig::new();
    let session = &rig.session;
    let kick_file = factory_file("Drums/Kicks/Kick Punch.wav");
    let kick_sample = sample_of(&rig, 0).unwrap();
    let audio_before = {
        let state = session.state();
        state.pool.get(kick_sample).unwrap().samples().as_ptr()
    };

    let added = session.add_channel_from_file(&kick_file, None).unwrap();
    assert_eq!(added.created.len(), 3);
    assert_eq!(added.created[0], kick_sample.0);
    assert_eq!(rig.project().samples.len(), 4);
    assert_eq!(rig.project().channels.len(), 5);
    assert_eq!(sample_of(&rig, 4), Some(kick_sample));
    assert!(added.patch.samples.is_none());

    // The same file on another channel: still one sample, still one copy
    // of the audio.
    let clap = rig.channel(1);
    let changed = session
        .set_channel_sample_from_file(clap, &kick_file)
        .unwrap();
    assert_eq!(changed.created, [kick_sample.0]);
    assert_eq!(
        changed.patch.history.entries.last().unwrap().label,
        "Change channel sample"
    );
    assert_eq!(sample_of(&rig, 1), Some(kick_sample));
    assert_eq!(rig.project().samples.len(), 4);
    let state = session.state();
    assert_eq!(
        state.pool.get(kick_sample).unwrap().samples().as_ptr(),
        audio_before
    );
    assert_eq!(state.pool.len(), 4);
}

#[test]
fn setting_a_channel_sample_from_a_new_file_adds_the_sample_in_the_same_step() {
    let rig = Rig::new();
    let session = &rig.session;
    let file = write_tone(&rig, "tone.wav");
    let clap = rig.channel(1);

    let changed = session.set_channel_sample_from_file(clap, &file).unwrap();
    let sample = SampleId(changed.created[0]);
    assert_eq!(changed.created.len(), 1);
    assert_eq!(sample_of(&rig, 1), Some(sample));
    // The project has no folder yet, so the file is stored by its full path.
    assert_eq!(
        rig.project().sample(sample).unwrap().path,
        SamplePath::External(file.clone())
    );
    assert!(rig.has_audio(sample));

    session.undo().unwrap();
    assert_eq!(rig.project().samples.len(), 4);
    assert_ne!(sample_of(&rig, 1), Some(sample));
    assert_eq!(session.undo(), None);

    // Once the project is saved beside the file, the file is stored
    // relative to the project.
    session.project_save(Some(&rig.file("beside"))).unwrap();
    let again = session.add_channel_from_file(&file, None).unwrap();
    assert_eq!(
        rig.project()
            .sample(SampleId(again.created[0]))
            .unwrap()
            .path,
        SamplePath::Project("tone.wav".to_owned())
    );
}

#[test]
fn a_file_that_cannot_be_read_is_refused_and_the_project_is_untouched() {
    let rig = Rig::new();
    let session = &rig.session;
    let before = session.document_snapshot();

    let text = rig.file("notes.wav");
    fs::write(&text, "these are not samples").unwrap();
    let error =
        windfall_core::english::english(&session.add_channel_from_file(&text, None).unwrap_err());
    assert!(error.contains("notes.wav"), "{error}");
    assert!(error.contains("not a supported audio file"), "{error}");

    let missing = rig.file("missing.wav");
    let error = windfall_core::english::english(
        &session.add_channel_from_file(&missing, None).unwrap_err(),
    );
    assert!(error.starts_with("Could not load"), "{error}");
    let error = session
        .set_channel_sample_from_file(rig.channel(0), &missing)
        .unwrap_err();
    assert!(error.contains("missing.wav"), "{error}");

    let error = session.add_channel_from_file("kick.wav", None).unwrap_err();
    assert_eq!(
        windfall_core::english::english(&error),
        "\"kick.wav\" is not a full path"
    );

    // A good file for a channel that does not exist: the sample must not
    // stay behind when the step as a whole fails.
    let good = factory_file("Drums/Percussion/Cowbell.wav");
    let error = session
        .set_channel_sample_from_file(ChannelId(999), &good)
        .unwrap_err();
    assert_eq!(
        windfall_core::english::english(&error),
        "channel 999 does not exist"
    );

    assert_eq!(session.document_snapshot(), before);
    assert!(rig.events.take().is_empty());
    assert_eq!(session.state().pool.len(), 4);
}

#[test]
fn a_sample_added_by_a_plain_command_is_decoded_off_the_callers_thread() {
    let rig = Rig::new();
    let session = &rig.session;
    let file = write_tone(&rig, "late.wav");

    let added = session
        .dispatch(
            Command::AddSample {
                name: "Late".to_owned(),
                path: SamplePath::External(file),
            },
            None,
        )
        .unwrap();
    let sample = SampleId(added.created[0]);
    rig.wait_until_loaded_or_failed(sample);
    assert!(rig.has_audio(sample));

    // One whose file is not there is reported once, and leaves the
    // project as the command made it.
    let missing = rig.file("nowhere.wav");
    let added = session
        .dispatch(
            Command::AddSample {
                name: "Nowhere".to_owned(),
                path: SamplePath::External(missing.clone()),
            },
            None,
        )
        .unwrap();
    let sample = SampleId(added.created[0]);
    let warnings = rig.events.wait_for(|events| {
        events.iter().find_map(|event| match event {
            Event::ProjectWarnings(warnings) => Some(warnings.clone()),
            _ => None,
        })
    });
    assert_eq!(
        warnings
            .iter()
            .map(|warning| windfall_core::english::english(warning))
            .collect::<Vec<_>>(),
        [format!("Missing sample: {missing}")]
    );
    assert!(!rig.has_audio(sample));
    assert!(rig.project().sample(sample).is_some());

    // The next edit to the samples does not report it a second time.
    rig.events.take();
    let added = session
        .dispatch(
            Command::AddSample {
                name: "Later".to_owned(),
                path: SamplePath::External(write_tone(&rig, "later.wav")),
            },
            None,
        )
        .unwrap();
    rig.wait_until_loaded_or_failed(SampleId(added.created[0]));
    assert!(rig.has_audio(SampleId(added.created[0])));
    assert!(
        !rig.events
            .take()
            .iter()
            .any(|event| matches!(event, Event::ProjectWarnings(_)))
    );
}

#[test]
fn deleting_the_pattern_the_transport_plays_moves_the_transport_first() {
    let rig = Rig::new();
    let session = &rig.session;
    let first = rig.pattern();
    let second = PatternId(session.dispatch(add_pattern(), None).unwrap().created[0]);
    session
        .transport_set(TransportPatch {
            pattern: Some(second),
            ..TransportPatch::default()
        })
        .unwrap();
    rig.events.take();

    session
        .dispatch(Command::RemovePattern { id: second }, None)
        .unwrap();
    let events = rig.events.take();
    let [Event::TransportState(transport), Event::ProjectPatch(patch)] = &events[..] else {
        panic!("{events:#?}");
    };
    assert_eq!(transport.pattern, first);
    assert_eq!(patch.pattern_order.as_deref(), Some(&[first][..]));

    // Undo brings the pattern back; the transport stays where it is.
    session.undo().unwrap();
    assert_eq!(session.transport_state().pattern, first);
}

#[test]
fn a_file_that_finishes_decoding_after_its_project_is_gone_is_not_added() {
    let rig = Rig::new();
    let session = &rig.session;
    let clap = rig.channel(1);

    for onto_a_channel in [false, true] {
        // The file is decoded, and the edit that uses it is next.
        let hold = session.hold("import:decoded");
        let importing = session.background(move |session| {
            let file = factory_file("Drums/Percussion/Cowbell.wav");
            if onto_a_channel {
                session.set_channel_sample_from_file(clap, &file)
            } else {
                session.add_channel_from_file(&file, None)
            }
        });
        hold.wait();

        // Every new project hands out the same ids, so the id of the clap
        // names a channel in this one too.
        let fresh = session.project_new().unwrap();
        assert!(fresh.project.channels.iter().any(|c| c.id == clap));
        rig.events.take();

        hold.release();
        assert_eq!(
            windfall_core::english::english(&importing.join().unwrap().unwrap_err()),
            "\"Cowbell.wav\" was not added, because another project was opened while it was loading."
        );
        assert_eq!(session.document_snapshot(), fresh);
        assert!(rig.events.take().is_empty());
        assert_eq!(session.state().pool.len(), 4);
    }
}

#[test]
fn reloading_samples_picks_up_files_that_have_turned_up() {
    let mut rig = Rig::new();
    let session = rig.session.clone();
    let late = rig.file("late.wav");
    let added = session
        .dispatch(
            Command::AddSample {
                name: "Late".to_owned(),
                path: SamplePath::External(late.clone()),
            },
            None,
        )
        .unwrap();
    let sample = SampleId(added.created[0]);
    let added = session
        .dispatch(
            Command::AddChannel {
                name: None,
                sample: Some(sample),
                instrument: None,
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap();
    let channel = ChannelId(added.created[0]);
    rig.wait_until_loaded_or_failed(sample);
    assert!(!rig.has_audio(sample));
    let before = session.document_snapshot();
    rig.events.take();

    // Still not there: it is reported again, and nothing else happens.
    assert_eq!(session.samples_reload(), 1);
    assert_eq!(
        super::english_events(rig.events.take()),
        [Event::ProjectWarnings(vec![format!(
            "Missing sample: {late}"
        )])]
    );

    write_tone(&rig, "late.wav");
    assert_eq!(session.samples_reload(), 0);
    assert!(rig.has_audio(sample));
    // The engine was handed it: the channel sounds.
    session.audition_note_on(channel, 60, 1.0);
    assert!(rms(&rig.run(1_200)) > 0.01);
    // The project itself is as it was.
    assert_eq!(session.document_snapshot(), before);
    assert!(rig.events.take().is_empty());

    assert_eq!(session.samples_reload(), 0);
    assert!(rig.events.take().is_empty());
}

#[test]
fn a_reload_that_outlives_its_project_leaves_the_next_one_alone() {
    let rig = Rig::new();
    let session = &rig.session;
    let add = |file: String| Command::AddSample {
        name: "Late".to_owned(),
        path: SamplePath::External(file),
    };
    let added = session.dispatch(add(rig.file("late.wav")), None).unwrap();
    let sample = SampleId(added.created[0]);
    rig.wait_until_loaded_or_failed(sample);
    write_tone(&rig, "late.wav");

    // The file is decoded and about to go into the pool.
    let hold = session.hold("samples:decoded");
    let reloading = session.background(|session| session.samples_reload());
    hold.wait();

    // The next project gives the same id to a sample whose file is missing.
    session.project_new().unwrap();
    let added = session.dispatch(add(rig.file("other.wav")), None).unwrap();
    let other = SampleId(added.created[0]);
    assert_eq!(other, sample);
    rig.wait_until_loaded_or_failed(other);

    hold.release();
    // It is still missing, and must not have been given the other's audio.
    assert_eq!(reloading.join().unwrap(), 1);
    assert!(!rig.has_audio(other));
    assert!(!session.state().pool.contains(other));
}
