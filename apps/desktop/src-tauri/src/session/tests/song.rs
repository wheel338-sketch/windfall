//! The playlist beyond pattern clips: audio clips and automation.

use std::fs;
use std::path::Path;

use windfall_codec::{WavSampleFormat, write_wav};
use windfall_core::AudioBuffer;
use windfall_ipc::{BitDepth, ExportFormat, ExportOptions, PlayMode, TransportPatch};
use windfall_project::file;
use windfall_project::{
    AudioClipPatch, AudioClipUpdate, AutomationId, AutomationPoint, AutomationTarget, ChannelId,
    Clip, ClipContent, ClipId, ClipInit, ClipPatch, ClipUpdate, Command, EffectKind,
    InstrumentKind, NoteInit, PatternId, PlaylistTrackId, Project, SampleId, SamplePath, TrackId,
};

use super::{Rig, SAMPLE_RATE, factory_file, rms};
use crate::events::Event;
use crate::paths;
use crate::session::ClipPlace;

/// A new playlist track and a new mixer track, with the clip on `start`.
fn new_tracks(start: u32) -> ClipPlace {
    ClipPlace {
        track: None,
        start,
        mixer_track: None,
    }
}

fn clip_of(project: &Project, id: u32) -> Clip {
    let clips = project.playlist.clips.iter();
    let clip = clips.clone().find(|clip| clip.id == ClipId(id));
    clip.expect("the clip exists").clone()
}

/// Equal apart from `next_id`, which undo leaves alone.
fn same_content(a: &Project, b: &Project) -> bool {
    let mut b = b.clone();
    b.next_id = a.next_id;
    *a == b
}

fn play_song(rig: &Rig) {
    rig.session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Song),
            loop_song: Some(false),
            ..TransportPatch::default()
        })
        .unwrap();
    assert!(rig.session.transport_play().unwrap().playing);
}

#[test]
fn an_audio_clip_from_a_file_is_one_undo_step_and_plays_in_the_song() {
    let mut rig = Rig::new();
    let session = rig.session.clone();
    let file = factory_file("Bass/Bass Sub.wav");
    let seconds = session.sample_info(&file).unwrap().duration_secs;
    let before = rig.project();
    let steps = session.document_snapshot().history.entries.len();
    rig.events.take();

    let result = session
        .add_audio_clip_from_file(&file, new_tracks(960))
        .unwrap();
    // The sample, the playlist track, the mixer track, and last the clip.
    let [sample, lane, mixer_track, clip] = result.created[..] else {
        panic!("expected four ids, got {:?}", result.created);
    };
    let project = rig.project();
    let asset = project.sample(SampleId(sample)).unwrap();
    assert_eq!(asset.name, "Bass Sub");
    assert_eq!(
        asset.path,
        SamplePath::Factory("Bass/Bass Sub.wav".to_owned())
    );
    let lanes = &project.playlist.tracks;
    assert_eq!(lanes.last().unwrap().id, PlaylistTrackId(lane));
    let track = project.mixer.tracks.last().unwrap();
    assert_eq!(
        (track.id, track.name.as_str()),
        (TrackId(mixer_track), "Bass Sub")
    );
    assert_eq!(track.output, Some(TrackId::MASTER));
    // As long as the file lasts at 120 bpm: 1920 ticks a second.
    let length = (seconds * 1_920.0).ceil() as u32;
    assert_eq!(
        clip_of(&project, clip),
        Clip {
            id: ClipId(clip),
            track: PlaylistTrackId(lane),
            start: 960,
            length,
            offset: 0,
            muted: false,
            content: ClipContent::Audio {
                sample: SampleId(sample),
                mixer_track: TrackId(mixer_track),
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
        }
    );

    // One step in the history, and one patch with all of it.
    let history = session.document_snapshot().history;
    assert_eq!(history.entries.len(), steps + 1);
    assert_eq!(history.entries[steps].label, "Add audio clip");
    match &rig.events.take()[..] {
        [Event::ProjectPatch(patch)] => {
            assert_eq!(patch, &result.patch);
            assert_eq!(patch.samples.as_ref(), Some(&project.samples));
            assert_eq!(patch.mixer.as_ref(), Some(&project.mixer));
            assert_eq!(patch.playlist.as_ref(), Some(&project.playlist));
            assert!(patch.channels.is_none() && patch.patterns.is_empty());
        }
        other => panic!("expected one patch, got {other:?}"),
    }

    // The audio is there to play at once: silence for a beat, then bass.
    assert!(rig.has_audio(SampleId(sample)));
    play_song(&rig);
    let out = rig.run(48_000);
    assert!(out[..24_000 * 2].iter().all(|sample| *sample == 0.0));
    assert!(rms(&out[24_000 * 2..]) > 0.01);
    session.transport_stop();

    // One undo takes all of it away, and one redo brings it back.
    assert!(session.undo().is_some());
    assert!(same_content(&before, &rig.project()));
    assert!(session.redo().is_some());
    assert_eq!(rig.project(), project);
}

#[test]
fn an_audio_clip_uses_the_sample_and_the_tracks_that_are_there() {
    let rig = Rig::new();
    let session = &rig.session;
    // The kick of the default kit is in the pool already.
    let kick = rig.project().samples[0].clone();
    let SamplePath::Factory(relative) = &kick.path else {
        panic!("the kit is factory content");
    };
    let lane = session
        .dispatch(
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0];
    let samples = rig.project().samples.len();
    let tracks = rig.project().mixer.tracks.len();

    let place = ClipPlace {
        track: Some(PlaylistTrackId(lane)),
        start: 0,
        mixer_track: Some(TrackId::MASTER),
    };
    let result = session
        .add_audio_clip_from_file(&factory_file(relative), place)
        .unwrap();
    // The sample it found, and the clip. Nothing else was made.
    let [sample, clip] = result.created[..] else {
        panic!("expected two ids, got {:?}", result.created);
    };
    assert_eq!(SampleId(sample), kick.id);
    let project = rig.project();
    assert_eq!(project.samples.len(), samples);
    assert_eq!(project.mixer.tracks.len(), tracks);
    assert_eq!(project.playlist.tracks.len(), 1);
    let added = clip_of(&project, clip);
    let length = added.length;
    assert_eq!(added.track, PlaylistTrackId(lane));
    assert!(matches!(
        added.content,
        ClipContent::Audio { sample, mixer_track: TrackId::MASTER, .. } if sample == kick.id
    ));

    // A sample of the project goes on the playlist the same way, with a
    // mixer track named after it when no clip plays it yet.
    let clap = rig.project().samples[1].clone();
    let result = session
        .add_audio_clip_from_sample(clap.id, new_tracks(480))
        .unwrap();
    let [lane, mixer_track, clip] = result.created[..] else {
        panic!("expected three ids, got {:?}", result.created);
    };
    let project = rig.project();
    assert_eq!(project.samples.len(), samples);
    assert_eq!(
        project.mixer.track(TrackId(mixer_track)).unwrap().name,
        clap.name
    );
    let added = clip_of(&project, clip);
    assert_eq!((added.track, added.start), (PlaylistTrackId(lane), 480));
    let label = &session.document_snapshot().history.entries;
    assert_eq!(label.last().unwrap().label, "Add audio clip");

    // The kick has a clip, on the master, so another clip of it goes
    // there too and no mixer track is made.
    let result = session
        .add_audio_clip_from_sample(kick.id, new_tracks(480))
        .unwrap();
    let [_, clip] = result.created[..] else {
        panic!("expected two ids, got {:?}", result.created);
    };
    let added = clip_of(&rig.project(), clip);
    assert_eq!(added.length, length);
    assert!(matches!(
        added.content,
        ClipContent::Audio {
            mixer_track: TrackId::MASTER,
            ..
        }
    ));
    assert_eq!(rig.project().mixer.tracks.len(), tracks + 1);

    // A track that is not there is refused, with nothing changed.
    let before = session.document_snapshot();
    let nowhere = ClipPlace {
        track: Some(PlaylistTrackId(999)),
        start: 0,
        mixer_track: None,
    };
    let error = session
        .add_audio_clip_from_sample(kick.id, nowhere)
        .unwrap_err();
    assert_eq!(
        windfall_core::english::english(&error),
        "playlist track 999 does not exist"
    );
    let error = session
        .add_audio_clip_from_sample(SampleId(999), new_tracks(0))
        .unwrap_err();
    assert_eq!(
        windfall_core::english::english(&error),
        "sample 999 does not exist"
    );
    assert_eq!(session.document_snapshot(), before);
}

#[test]
fn audio_dropped_again_shares_the_mixer_track_it_has() {
    let rig = Rig::new();
    let session = &rig.session;
    let file = factory_file("Bass/Bass Sub.wav");
    let tracks = || rig.project().mixer.tracks.len();
    let before = tracks();
    let track_of = |clip: u32| match clip_of(&rig.project(), clip).content {
        ClipContent::Audio { mixer_track, .. } => mixer_track,
        other => panic!("{other:?} is not an audio clip"),
    };

    // The first drop makes a mixer track named after the file.
    let first = session
        .add_audio_clip_from_file(&file, new_tracks(0))
        .unwrap()
        .created;
    let [sample, _, bass, clip] = first[..] else {
        panic!("expected four ids, got {first:?}");
    };
    assert_eq!(track_of(clip), TrackId(bass));
    assert_eq!(tracks(), before + 1);

    // The second and the third join it: the sample, a playlist track and
    // the clip are all that is reported, and one undo takes the drop back.
    for start in [960, 1_920] {
        let again = session
            .add_audio_clip_from_file(&file, new_tracks(start))
            .unwrap()
            .created;
        let [same, _, clip] = again[..] else {
            panic!("expected three ids, got {again:?}");
        };
        assert_eq!(same, sample);
        assert_eq!(track_of(clip), TrackId(bass));
    }
    assert_eq!(tracks(), before + 1);
    let names = rig.project().mixer.tracks;
    let named: Vec<_> = names.iter().filter(|t| t.name == "Bass Sub").collect();
    assert_eq!(named.len(), 1);
    assert!(session.undo().is_some());
    assert_eq!(rig.project().playlist.clips.len(), 2);
    assert_eq!(tracks(), before + 1);

    // The same from the sample in the pool.
    let pooled = session
        .add_audio_clip_from_sample(SampleId(sample), new_tracks(2_880))
        .unwrap()
        .created;
    let [_, newest] = pooled[..] else {
        panic!("expected two ids, got {pooled:?}");
    };
    assert_eq!(track_of(newest), TrackId(bass));

    // It is the clip made last that is followed, wherever it plays now.
    let bus = session
        .dispatch(Command::AddMixerTrack { name: None }, None)
        .unwrap()
        .created[0];
    let route = Command::UpdateAudioClips {
        updates: vec![AudioClipUpdate {
            id: ClipId(newest),
            patch: AudioClipPatch {
                mixer_track: Some(TrackId(bus)),
                ..AudioClipPatch::default()
            },
        }],
    };
    session.dispatch(route, None).unwrap();
    let followed = session
        .add_audio_clip_from_file(&file, new_tracks(3_840))
        .unwrap()
        .created;
    assert_eq!(track_of(*followed.last().unwrap()), TrackId(bus));

    // A mixer track that is asked for is the one that is used.
    let asked = ClipPlace {
        track: None,
        start: 4_800,
        mixer_track: Some(TrackId::MASTER),
    };
    let on_master = session.add_audio_clip_from_file(&file, asked).unwrap();
    assert_eq!(
        track_of(*on_master.created.last().unwrap()),
        TrackId::MASTER
    );

    // With every clip of it gone, the next drop makes a track again.
    let clips = rig.project().playlist.clips;
    let clips = clips.iter().map(|clip| clip.id).collect();
    session
        .dispatch(Command::RemoveClips { clips }, None)
        .unwrap();
    let count = tracks();
    let fresh = session
        .add_audio_clip_from_file(&file, new_tracks(0))
        .unwrap()
        .created;
    assert_eq!(fresh.len(), 4);
    assert_eq!(tracks(), count + 1);
}

#[test]
fn a_file_that_is_not_audio_makes_no_clip() {
    let rig = Rig::new();
    let session = &rig.session;
    let noise = rig.file("noise.wav");
    fs::write(&noise, b"this is not audio").unwrap();
    let before = session.document_snapshot();
    rig.events.take();

    assert!(
        session
            .add_audio_clip_from_file(&noise, new_tracks(0))
            .is_err()
    );
    let missing = rig.file("missing.wav");
    assert!(
        session
            .add_audio_clip_from_file(&missing, new_tracks(0))
            .is_err()
    );
    assert_eq!(
        windfall_core::english::english(
            &session
                .add_audio_clip_from_file("loop.wav", new_tracks(0))
                .unwrap_err()
        ),
        "\"loop.wav\" is not a full path"
    );
    assert_eq!(session.document_snapshot(), before);
    assert!(rig.events.take().is_empty());
}

#[test]
fn a_clip_whose_file_was_still_loading_when_another_project_opened_is_not_added() {
    let rig = Rig::new();
    let session = &rig.session;
    let hold = session.hold("import:decoded");
    let adding = session.background(|session| {
        session.add_audio_clip_from_file(&factory_file("Bass/Bass Sub.wav"), new_tracks(0))
    });
    hold.wait();
    session.project_new().unwrap();
    let opened = session.document_snapshot();
    hold.release();

    let error = adding.join().unwrap().unwrap_err();
    assert_eq!(
        windfall_core::english::english(&error),
        "\"Bass Sub.wav\" was not added, because another project was opened while it was loading."
    );
    assert_eq!(session.document_snapshot(), opened);
}

#[test]
fn a_sample_without_audio_cannot_be_made_into_a_clip() {
    let rig = Rig::new();
    let session = &rig.session;
    let gone = paths::display(&rig.folder.path().join("gone.wav"));
    let sample = SampleId(
        session
            .dispatch(
                Command::AddSample {
                    name: "Gone".to_owned(),
                    path: SamplePath::External(gone),
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    rig.wait_until_loaded_or_failed(sample);
    let before = rig.project();
    let error = session
        .add_audio_clip_from_sample(sample, new_tracks(0))
        .unwrap_err();
    assert_eq!(
        windfall_core::english::english(&error),
        "The audio of \"Gone\" is not loaded, so a clip of it cannot be made. Check that its file is there, then reload the samples."
    );
    assert_eq!(rig.project(), before);
}

fn write_tone(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let buffer = AudioBuffer::from_interleaved(48_000, 2, vec![0.25; 9_600]);
    write_wav(path, &buffer, WavSampleFormat::Int24).unwrap();
}

#[test]
fn the_sample_of_an_audio_clip_is_saved_carried_and_missed_like_any_other() {
    let rig = Rig::new();
    let session = &rig.session;
    let home = session.project_save(Some(&rig.file("home/song"))).unwrap();
    let take = Path::new(&home).with_file_name("takes").join("vocal.wav");
    write_tone(&take);
    let result = session
        .add_audio_clip_from_file(&paths::display(&take), new_tracks(0))
        .unwrap();
    let sample = SampleId(result.created[0]);
    // A file inside the project folder is stored relative to it.
    let own = SamplePath::Project("takes/vocal.wav".to_owned());
    assert_eq!(rig.project().sample(sample).unwrap().path, own);
    // No channel plays it: the clip alone keeps it in use.
    let used_by_a_channel = |project: &Project| {
        let mut channels = project.channels.iter();
        channels.any(|channel| channel.source.sample() == Some(sample))
    };
    assert!(!used_by_a_channel(&rig.project()));
    let error = session
        .dispatch(Command::RemoveSample { id: sample }, None)
        .unwrap_err();
    assert!(
        windfall_core::english::english(&error).contains("still used by an audio clip"),
        "{error}"
    );

    // Saved into another folder, the file goes along.
    let moved = session.project_save(Some(&rig.file("away/song"))).unwrap();
    let carried = Path::new(&moved).with_file_name("takes").join("vocal.wav");
    assert_eq!(fs::read(&carried).unwrap(), fs::read(&take).unwrap());
    assert_eq!(file::load(&moved).unwrap(), rig.project());

    // Opened again it has its audio, and no warning.
    let rig = rig.restart();
    rig.session.project_open(&moved).unwrap();
    assert!(rig.has_audio(sample));
    let warned = |rig: &Rig| -> Vec<String> {
        let events = rig.events.take().into_iter();
        events
            .filter_map(|event| match event {
                Event::ProjectWarnings(warnings) => Some(warnings),
                _ => None,
            })
            .flatten()
            .map(|warning| windfall_core::english::english(&warning))
            .collect()
    };
    assert!(warned(&rig).is_empty());

    // With the file gone the project still opens, says what is missing,
    // and plays the clip as silence.
    fs::remove_file(&carried).unwrap();
    let mut rig = rig.restart();
    rig.session.project_open(&moved).unwrap();
    assert!(!rig.has_audio(sample));
    let warnings = warned(&rig);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].starts_with("Missing sample: "), "{warnings:?}");
    assert!(warnings[0].ends_with("vocal.wav"), "{warnings:?}");
    play_song(&rig);
    assert!(rig.run(9_600).iter().all(|sample| *sample == 0.0));
    rig.session.transport_stop();

    // Put back and reloaded, it plays.
    write_tone(&carried);
    assert_eq!(rig.session.samples_reload(), 0);
    assert!(rig.has_audio(sample));
    play_song(&rig);
    assert!(rms(&rig.run(9_600)) > 0.05);
}

fn point(tick: u32, value: f32) -> AutomationPoint {
    AutomationPoint {
        tick,
        value,
        curve: 0.0,
        hold: false,
    }
}

#[test]
fn a_control_gets_an_automation_and_its_clip_in_one_step() {
    let rig = Rig::new();
    let session = &rig.session;
    let kick = rig.channel(0);
    let before = rig.project();
    let steps = session.document_snapshot().history.entries.len();
    rig.events.take();

    let target = AutomationTarget::ChannelVolume { channel: kick };
    let result = session.automate(target).unwrap();
    // The automation, the playlist track it is put on, and the clip.
    let [automation, lane, clip] = result.created[..] else {
        panic!("expected three ids, got {:?}", result.created);
    };
    let project = rig.project();
    let added = project.automation(AutomationId(automation)).unwrap();
    assert_eq!(added.target, target);
    assert_eq!(added.name, "Kick Punch volume");
    // One point, where the channel's volume is: nothing changes yet.
    let range = project.automation_range(&target).unwrap();
    let [only] = added.points[..] else {
        panic!("expected one point, got {:?}", added.points);
    };
    assert_eq!(only.tick, 0);
    assert_eq!(range.value(only.value), project.channels[0].volume);
    assert_eq!(project.playlist.tracks.last().unwrap().id.0, lane);
    // The playlist is empty, so the clip is four bars long.
    assert_eq!(
        clip_of(&project, clip),
        Clip {
            id: ClipId(clip),
            track: PlaylistTrackId(lane),
            start: 0,
            length: 15_360,
            offset: 0,
            muted: false,
            content: ClipContent::Automation {
                automation: AutomationId(automation)
            },
        }
    );
    let history = session.document_snapshot().history;
    assert_eq!(history.entries.len(), steps + 1);
    assert_eq!(history.entries[steps].label, "Create automation clip");
    match &rig.events.take()[..] {
        [Event::ProjectPatch(patch)] => {
            assert_eq!(patch, &result.patch);
            assert_eq!(patch.automations.as_ref(), Some(&project.automations));
            assert_eq!(patch.playlist.as_ref(), Some(&project.playlist));
            assert!(patch.channels.is_none() && patch.mixer.is_none());
        }
        other => panic!("expected one patch, got {other:?}"),
    }

    // A second one spans the song as it is now: ten bars.
    let extend = ClipPatch {
        length: Some(38_400),
        ..ClipPatch::default()
    };
    let update = Command::UpdateClips {
        updates: vec![ClipUpdate {
            id: ClipId(clip),
            patch: extend,
        }],
    };
    session.dispatch(update, None).unwrap();
    let result = session.automate(AutomationTarget::Tempo).unwrap();
    let project = rig.project();
    assert_eq!(clip_of(&project, result.created[2]).length, 38_400);
    assert_eq!(project.playlist.tracks.len(), 2);

    // One undo takes the automation, its track and its clip away again.
    assert!(session.undo().is_some());
    assert!(session.undo().is_some());
    assert!(session.undo().is_some());
    assert!(same_content(&before, &rig.project()));

    // Something that is not there cannot be automated.
    let unchanged = session.document_snapshot();
    let missing = AutomationTarget::ChannelVolume {
        channel: ChannelId(999),
    };
    assert_eq!(
        windfall_core::english::english(&session.automate(missing).unwrap_err()),
        "channel 999 does not exist"
    );
    assert_eq!(session.document_snapshot(), unchanged);
}

/// Exports the song with a second of tail and returns the bytes of the file
/// and the audio in it.
fn export_song(rig: &Rig, name: &str) -> (Vec<u8>, AudioBuffer) {
    rig.events.take();
    let options = ExportOptions {
        path: rig.file(name),
        format: ExportFormat::Wav,
        bit_depth: BitDepth::Float32,
        sample_rate: SAMPLE_RATE,
        mode: PlayMode::Song,
        pattern_loops: 1,
        tail_secs: 1.0,
        auto_tail: false,
        ..ExportOptions::default()
    };
    rig.session.export_audio(options.clone()).unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
    let bytes = fs::read(&options.path).unwrap();
    (bytes, windfall_codec::decode_file(&options.path).unwrap())
}

fn set_muted(rig: &Rig, clips: &[u32], muted: bool) {
    let updates = clips.iter().map(|clip| ClipUpdate {
        id: ClipId(*clip),
        patch: ClipPatch {
            muted: Some(muted),
            ..ClipPatch::default()
        },
    });
    let update = Command::UpdateClips {
        updates: updates.collect(),
    };
    rig.session.dispatch(update, None).unwrap();
}

/// How far apart two exports are, bar by bar: the level of their
/// difference in each of the four bars of the song.
fn apart_by_bar(a: &AudioBuffer, b: &AudioBuffer) -> Vec<f32> {
    let difference: Vec<f32> = a
        .samples()
        .iter()
        .zip(b.samples())
        .map(|(a, b)| a - b)
        .collect();
    let bars = difference[..4 * 96_000 * 2].chunks(96_000 * 2);
    bars.map(rms).collect()
}

/// The exit check of phase 2: a full song with melody, drums, automation
/// and effects can be made and exported.
#[test]
fn a_song_with_drums_a_melody_audio_effects_and_automation_is_made_and_exported() {
    let mut rig = Rig::new();
    let session = rig.session.clone();
    let run = |command: Command| session.dispatch(command, None).unwrap().created;

    // Drums on the default kit, in the first pattern: kick on the beat,
    // snare on two and four, hats between.
    let drums = rig.pattern();
    for (channel, steps) in [(0, &[0, 4, 8, 12][..]), (3, &[4, 12]), (2, &[2, 6, 10, 14])] {
        for step in steps {
            run(Command::ToggleStep {
                pattern: drums,
                channel: rig.channel(channel),
                step: *step,
            });
        }
    }

    // A melody on the synth, in a pattern of its own.
    let melody = PatternId(run(Command::AddPattern { name: None })[0]);
    let added = run(Command::AddChannel {
        name: Some("Lead".to_owned()),
        sample: None,
        instrument: Some(InstrumentKind::SubtractiveSynth),
        index: None,
        mixer_track: None,
    });
    let (lead, lead_track) = (ChannelId(added[0]), TrackId(added[1]));
    let notes = [57, 60, 64, 67].into_iter().enumerate();
    run(Command::AddNotes {
        pattern: melody,
        channel: lead,
        notes: notes
            .map(|(beat, key)| NoteInit {
                start: beat as u32 * 960,
                length: 720,
                key,
                velocity: None,
                pan: None,
                expression: None,
            })
            .collect(),
    });

    // Effects: a reverb on the lead and a limiter on the master.
    for (track, kind) in [
        (lead_track, EffectKind::Reverb),
        (TrackId::MASTER, EffectKind::Limiter),
    ] {
        run(Command::AddEffect {
            track,
            kind,
            index: None,
        });
    }

    // Arranged on the playlist: four bars of both patterns.
    let lane = |name: &str| {
        let added = run(Command::AddPlaylistTrack {
            name: Some(name.to_owned()),
            index: None,
        });
        PlaylistTrackId(added[0])
    };
    let (drum_lane, lead_lane) = (lane("Drums"), lane("Lead"));
    let pattern_clip = |track, pattern| ClipInit {
        track,
        start: 0,
        length: Some(15_360),
        offset: None,
        muted: None,
        content: ClipContent::Pattern { pattern },
    };
    run(Command::AddClips {
        clips: vec![
            pattern_clip(drum_lane, drums),
            pattern_clip(lead_lane, melody),
        ],
    });

    // An audio clip from the factory content, from the third bar.
    let bass = factory_file("Bass/Bass Sub.wav");
    let place = ClipPlace {
        track: None,
        start: 7_680,
        mixer_track: None,
    };
    let audio_clip = session.add_audio_clip_from_file(&bass, place).unwrap();
    let bass_sample = SampleId(audio_clip.created[0]);
    // Cut to the two bars that are left, so the song stays four bars long.
    let length = clip_of(&rig.project(), audio_clip.created[3]).length;
    run(Command::UpdateClips {
        updates: vec![ClipUpdate {
            id: ClipId(audio_clip.created[3]),
            patch: ClipPatch {
                length: Some(length.min(7_680)),
                ..ClipPatch::default()
            },
        }],
    });

    // Automation: the lead fades in from silence over the first two bars,
    // and its filter opens over all four.
    let volume = session
        .automate(AutomationTarget::TrackVolume { track: lead_track })
        .unwrap()
        .created;
    run(Command::SetAutomationPoints {
        id: AutomationId(volume[0]),
        points: vec![point(0, 0.0), point(7_680, std::f32::consts::FRAC_1_SQRT_2)],
    });
    let settings = InstrumentKind::SubtractiveSynth.descriptors();
    let cutoff = settings
        .iter()
        .position(|info| info.id == "filter.cutoffHz");
    let filter = session
        .automate(AutomationTarget::InstrumentParam {
            channel: lead,
            param: cutoff.unwrap() as u32,
        })
        .unwrap()
        .created;
    run(Command::SetAutomationPoints {
        id: AutomationId(filter[0]),
        points: vec![point(0, 0.35), point(15_360, 0.9)],
    });
    let project = rig.project();
    project.check().unwrap();
    assert_eq!(project.automations.len(), 2);
    assert_eq!(project.playlist.clips.len(), 5);

    // It plays: the song is heard, and the UI is told which automations
    // are moving their controls.
    play_song(&rig);
    assert!(rms(&rig.run(48_000)) > 0.02);
    let frame = session.realtime_tick();
    assert!(frame.playing);
    let moving: Vec<u32> = frame.automated.iter().map(|a| a.automation.0).collect();
    assert_eq!(moving, [volume[0], filter[0]]);
    // The fade is a quarter of the way: tick 1920 of 7680.
    let expected = std::f32::consts::FRAC_1_SQRT_2 * 0.25;
    assert!((frame.automated[0].value - expected).abs() < 0.01);
    // Stopped, the lead's reverb rings on under the fader the song left,
    // and the UI still shows it there. Stop again lets go of it.
    session.transport_stop();
    rig.run(4_800);
    let frame = session.realtime_tick();
    assert!(!frame.playing);
    let held: Vec<u32> = frame.automated.iter().map(|a| a.automation.0).collect();
    assert_eq!(held, moving);
    session.transport_stop();
    rig.run(4_800);
    assert!(session.realtime_tick().automated.is_empty());
    // Playing wrote nothing into the project.
    assert_eq!(rig.project(), project);

    // Saved and opened again in another run of the app, it is the same
    // project, with all of its audio.
    let saved = session
        .project_save(Some(&rig.file("songs/phase two")))
        .unwrap();
    assert!(!session.document_snapshot().dirty);
    let rig = rig.restart();
    let opened = rig.session.project_open(&saved).unwrap();
    assert_eq!(opened.project, project);
    assert!(!opened.dirty);
    assert!(rig.has_audio(bass_sample));
    for sample in &project.samples {
        assert!(rig.has_audio(sample.id), "{sample:?}");
    }
    let warned = rig.events.take().into_iter();
    assert!(
        !warned
            .into_iter()
            .any(|event| matches!(event, Event::ProjectWarnings(_)))
    );

    // Exported twice, the two files are the same to the byte: four bars of
    // 96000 frames and a second of tail.
    let (bytes, song) = export_song(&rig, "phase two.wav");
    let (again, _) = export_song(&rig, "phase two again.wav");
    assert_eq!(bytes, again);
    assert_eq!(song.frames(), 4 * 96_000 + 48_000);
    assert_eq!(song.channels(), 2);
    assert!(rms(song.samples()) > 0.02);

    // The automation is in what was exported. Without the fade, the lead
    // is there from the first bar, so the two differ most at the start
    // and least once the fade has arrived at the stored 0 dB.
    set_muted(&rig, &[volume[2]], true);
    let (_, unfaded) = export_song(&rig, "no fade.wav");
    let apart = apart_by_bar(&song, &unfaded);
    assert!(apart[0] > 0.01, "{apart:?}");
    assert!(
        apart[0] > apart[1] && apart[1] > apart[3] * 4.0,
        "{apart:?}"
    );
    let first_bar = |audio: &AudioBuffer| rms(&audio.samples()[..96_000 * 2]);
    assert!(first_bar(&song) < first_bar(&unfaded));
    set_muted(&rig, &[volume[2]], false);

    // Without the sweep the filter stays wide open, which is brighter all
    // the way through and most of all at the start, where the sweep has
    // the filter nearly shut.
    set_muted(&rig, &[filter[2]], true);
    let (_, unswept) = export_song(&rig, "no sweep.wav");
    let apart = apart_by_bar(&song, &unswept);
    assert!(apart.iter().all(|bar| *bar > 1e-4), "{apart:?}");
    set_muted(&rig, &[filter[2]], false);

    // Back as it was, it exports as it did.
    let (restored, _) = export_song(&rig, "phase two restored.wav");
    assert_eq!(restored, bytes);
}

fn spectral_command(clip: u32) -> Command {
    Command::UpdateAudioClips {
        updates: vec![AudioClipUpdate {
            id: ClipId(clip),
            patch: AudioClipPatch {
                stretch: Some(windfall_project::ClipStretch::Spectral {
                    ratio: 1.5,
                    quality: windfall_project::ClipStretchQuality::Standard,
                    formants: true,
                }),
                pitch: Some(7.0),
                ..Default::default()
            },
        }],
    }
}
#[test]
fn spectral_worker_settings_save_open_and_undo_without_touching_original_sample() {
    let rig = Rig::new();
    let session = rig.session.clone();
    let added = session
        .add_audio_clip_from_file(&factory_file("Bass/Bass Sub.wav"), new_tracks(0))
        .unwrap();
    let clip = *added.created.last().unwrap();
    let before = rig.project();
    session
        .prepare_clip_command(spectral_command(clip))
        .unwrap();
    let after = rig.project();
    assert_eq!(after.samples, before.samples);
    assert_eq!(clip_of(&after, clip).length, clip_of(&before, clip).length);
    let saved = session.project_save(Some(&rig.file("spectral"))).unwrap();
    session.undo().unwrap();
    assert!(same_content(&rig.project(), &before));
    session.redo().unwrap();
    assert!(same_content(&rig.project(), &after));
    session.project_open(&saved).unwrap();
    assert!(same_content(&rig.project(), &after));
}
#[test]
fn spectral_worker_releases_state_and_rejects_an_edit_during_preparation() {
    let rig = Rig::new();
    let session = rig.session.clone();
    let added = session
        .add_audio_clip_from_file(&factory_file("Bass/Bass Sub.wav"), new_tracks(0))
        .unwrap();
    let clip = *added.created.last().unwrap();
    let hold = session.hold("clip:prepared");
    let worker = session.clone();
    let task = std::thread::spawn(move || worker.prepare_clip_command(spectral_command(clip)));
    hold.wait();
    // This would deadlock if preparation retained the session lock.
    session
        .dispatch(
            Command::UpdateAudioClips {
                updates: vec![AudioClipUpdate {
                    id: ClipId(clip),
                    patch: AudioClipPatch {
                        gain: Some(0.5),
                        ..Default::default()
                    },
                }],
            },
            None,
        )
        .unwrap();
    hold.release();
    assert!(
        task.join()
            .unwrap()
            .unwrap_err()
            .contains("project changed")
    );
    assert!(matches!(
        clip_of(&rig.project(), clip).content,
        ClipContent::Audio {
            gain: 0.5,
            stretch: windfall_project::ClipStretch::Tape,
            ..
        }
    ));
}

#[test]
fn uncached_clip_publications_prepare_off_lock_and_install_the_latest_edit() {
    let rig = Rig::new();
    let session = rig.session.clone();
    let added = session
        .add_audio_clip_from_file(&factory_file("Bass/Bass Sub.wav"), new_tracks(0))
        .unwrap();
    let clip = *added.created.last().unwrap();
    let before = session.document_snapshot();
    let hold = session.hold("sampler:prepared");
    let work = session.background(move |session| session.dispatch(spectral_command(clip), None));
    hold.wait();
    assert_eq!(session.document_snapshot(), before);
    session
        .dispatch(
            Command::UpdateAudioClips {
                updates: vec![AudioClipUpdate {
                    id: ClipId(clip),
                    patch: AudioClipPatch {
                        gain: Some(0.25),
                        ..Default::default()
                    },
                }],
            },
            None,
        )
        .unwrap();
    let edited = session.document_snapshot();
    hold.release();
    assert!(
        work.join()
            .unwrap()
            .unwrap_err()
            .contains("project changed")
    );
    assert_eq!(session.document_snapshot(), edited);
    session.dispatch(spectral_command(clip), None).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while session
        .inner
        .preparing_clips
        .load(std::sync::atomic::Ordering::Acquire)
    {
        assert!(
            std::time::Instant::now() < deadline,
            "clip preparation did not complete"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(matches!(
        clip_of(&rig.project(), clip).content,
        ClipContent::Audio {
            gain: 0.25,
            stretch: windfall_project::ClipStretch::Spectral { ratio: 1.5, .. },
            ..
        }
    ));
}
