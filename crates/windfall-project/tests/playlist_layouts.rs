//! Independent arrangement layouts, an automation clip's own curve and
//! linked instrument/audio tracks, through the public document: every edit
//! is one undo step, redoes to the same ids and survives a save and reload.

use serde_json::json;
use windfall_project::arrangement::TrackKind;
use windfall_project::*;

/// Dispatches, then proves the edit is exactly one history entry whose undo
/// restores the project, whose redo repeats it, and which a file keeps.
fn edit(doc: &mut Document, command: Command) -> Vec<u32> {
    let before = doc.project().clone();
    let history = doc.history().cursor;
    let applied = doc.dispatch(command, None).unwrap();
    let after = doc.project().clone();
    doc.project().check().unwrap();
    assert!(!applied.touched.is_empty(), "the command changed nothing");
    assert_eq!(doc.history().cursor, history + 1);
    doc.undo().unwrap();
    let mut expected = before;
    expected.next_id = after.next_id;
    assert_eq!(*doc.project(), expected);
    doc.redo().unwrap();
    assert_eq!(*doc.project(), after);
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("layouts.windfall");
    file::save(doc.project(), &path).unwrap();
    assert_eq!(file::load(&path).unwrap(), after);
    applied.created
}

fn command(value: serde_json::Value) -> Command {
    serde_json::from_value(value).unwrap()
}

fn refused(doc: &mut Document, command: Command) {
    let before = doc.snapshot(None);
    assert!(doc.dispatch(command, None).is_err());
    assert_eq!(doc.snapshot(None), before);
}

fn clip(doc: &Document, id: u32) -> Clip {
    let clips = &doc.project().playlist.clips;
    clips.iter().find(|clip| clip.id.0 == id).unwrap().clone()
}

/// Two lanes, two pattern clips on the first and one on the second.
fn fixture() -> (Document, [u32; 2], [u32; 3]) {
    let mut doc = Document::new(Project::new("Layouts"));
    let tracks = [0, 1].map(|index| {
        edit(
            &mut doc,
            command(json!({"type": "addPlaylistTrack", "name": format!("Lane {index}")})),
        )[0]
    });
    let clips = edit(
        &mut doc,
        command(json!({"type": "addClips", "clips": [
            {"track": tracks[0], "start": 0, "length": 3840, "content": {"type": "pattern", "pattern": 1}},
            {"track": tracks[0], "start": 3840, "length": 1920, "offset": 240, "content": {"type": "pattern", "pattern": 1}},
            {"track": tracks[1], "start": 960, "length": 960, "muted": true, "content": {"type": "pattern", "pattern": 1}},
        ]})),
    );
    // In the order the playlist keeps them: by start, then id.
    assert_eq!(
        doc.project()
            .playlist
            .clips
            .iter()
            .map(|clip| clip.id.0)
            .collect::<Vec<_>>(),
        [clips[0], clips[2], clips[1]]
    );
    (doc, tracks, [clips[0], clips[2], clips[1]])
}

fn book(doc: &Document) -> &ArrangementBook {
    &doc.project().playlist.arrangement_book
}

#[test]
fn first_layout_keeps_the_present_timeline_and_an_empty_layout_starts_clean() {
    let (mut doc, tracks, clips) = fixture();
    let saved = doc.project().playlist.clips.clone();
    let created = edit(
        &mut doc,
        command(json!({"type": "addArrangementLayout", "name": "Bridge", "copy": false})),
    );
    assert_eq!(created.len(), 1, "an empty layout creates no clips");
    let layouts = &book(&doc).arrangements;
    assert_eq!(layouts.len(), 2);
    assert_eq!(layouts[0].name, "Arrangement 1");
    assert_eq!(
        layouts[0].clips.iter().map(|id| id.0).collect::<Vec<_>>(),
        clips
    );
    assert_eq!(
        layouts[0].tracks.iter().map(|id| id.0).collect::<Vec<_>>(),
        tracks
    );
    assert_eq!(layouts[1].id, created[0]);
    assert_eq!(layouts[1].name, "Bridge");
    assert!(layouts[1].clips.is_empty());
    assert_eq!(
        layouts[1].tracks, layouts[0].tracks,
        "the lanes are there to place on"
    );
    assert_eq!(book(&doc).active, Some(created[0]));
    assert_eq!(doc.project().playlist.clips, saved, "no stored clip moved");

    // What is placed now belongs to the new layout alone.
    let first = layouts[0].id;
    let placed = edit(
        &mut doc,
        command(json!({"type": "addClips", "clips": [
            {"track": tracks[1], "start": 7680, "length": 960, "content": {"type": "pattern", "pattern": 1}},
        ]})),
    )[0];
    assert_eq!(book(&doc).arrangements[1].clips, vec![ClipId(placed)]);
    assert_eq!(
        book(&doc).arrangements[0]
            .clips
            .iter()
            .map(|id| id.0)
            .collect::<Vec<_>>(),
        clips
    );
    edit(&mut doc, Command::SwitchArrangement { id: first });
    assert_eq!(book(&doc).active, Some(first));
    for (id, before) in clips.iter().zip(&saved) {
        assert_eq!(&clip(&doc, *id), before);
    }

    // A second empty layout does not capture the timeline again.
    let third = edit(
        &mut doc,
        command(json!({"type": "addArrangementLayout", "name": "Outro", "copy": false})),
    );
    assert_eq!(book(&doc).arrangements.len(), 3);
    assert_eq!(book(&doc).active, Some(third[0]));

    refused(
        &mut doc,
        command(json!({"type": "addArrangementLayout", "name": "  ", "copy": true})),
    );
}

#[test]
fn a_copied_layout_owns_independent_clip_positions() {
    let (mut doc, tracks, clips) = fixture();
    edit(
        &mut doc,
        Command::AddClipGroup {
            clips: vec![ClipId(clips[0]), ClipId(clips[2])],
        },
    );
    let originals: Vec<Clip> = clips.iter().map(|id| clip(&doc, *id)).collect();
    let created = edit(
        &mut doc,
        command(json!({"type": "addArrangementLayout", "name": "Variation", "copy": true})),
    );
    let (layout, copies) = (created[0], &created[1..]);
    assert_eq!(copies.len(), 3);
    let book_now = book(&doc).clone();
    assert_eq!(book_now.active, Some(layout));
    assert_eq!(
        book_now.arrangements[1]
            .clips
            .iter()
            .map(|id| id.0)
            .collect::<Vec<_>>(),
        copies
    );
    for (copy, original) in copies.iter().zip(&originals) {
        assert!(!clips.contains(copy), "a copy is a new clip entity");
        let mut expected = original.clone();
        expected.id = ClipId(*copy);
        assert_eq!(
            clip(&doc, *copy),
            expected,
            "same lane, geometry, mute and content"
        );
    }
    // The copied clips are grouped among themselves, not with the originals.
    assert_eq!(book_now.clip_groups.len(), 2);
    assert_eq!(
        book_now.clip_groups[1].clips,
        vec![ClipId(copies[0]), ClipId(copies[2])]
    );
    assert_ne!(book_now.clip_groups[1].id, book_now.clip_groups[0].id);

    // Move, resize, mute and retrack the copy: the original layout is untouched.
    edit(
        &mut doc,
        command(json!({"type": "updateClips", "updates": [
            {"id": copies[0], "patch": {"start": 7680, "length": 960, "offset": 120, "muted": true, "track": tracks[1]}},
        ]})),
    );
    edit(
        &mut doc,
        command(json!({"type": "removeClips", "clips": [copies[1]]})),
    );
    for (id, original) in clips.iter().zip(&originals) {
        assert_eq!(&clip(&doc, *id), original);
    }
    let moved = clip(&doc, copies[0]);
    assert_eq!((moved.start, moved.length, moved.offset), (7680, 960, 120));
    assert_eq!(moved.track.0, tracks[1]);

    // And the other way round: editing the first layout leaves the copy alone.
    let first = book(&doc).arrangements[0].id;
    edit(&mut doc, Command::SwitchArrangement { id: first });
    edit(
        &mut doc,
        command(json!({"type": "updateClips", "updates": [
            {"id": clips[2], "patch": {"start": 1920}},
        ]})),
    );
    let mut untouched = originals[2].clone();
    untouched.id = ClipId(copies[2]);
    assert_eq!(clip(&doc, copies[2]), untouched);
    assert_eq!(clip(&doc, clips[2]).start, 1920);
    assert_eq!(
        book(&doc).arrangements[1].clips,
        vec![ClipId(copies[0]), ClipId(copies[2])]
    );
}

#[test]
fn removing_a_layout_removes_only_the_clips_it_alone_placed() {
    let (mut doc, _, clips) = fixture();
    let created = edit(
        &mut doc,
        command(json!({"type": "addArrangementLayout", "name": "Variation", "copy": true})),
    );
    let (layout, copies) = (created[0], created[1..].to_vec());
    let first = book(&doc).arrangements[0].id;
    // The variation also borrows one clip of the first layout.
    let mut shared: Vec<ClipId> = copies.iter().copied().map(ClipId).collect();
    shared.push(ClipId(clips[0]));
    let lanes = book(&doc).arrangements[1].tracks.clone();
    edit(
        &mut doc,
        Command::SetArrangementReferences {
            id: layout,
            clips: shared,
            tracks: lanes,
        },
    );
    edit(&mut doc, Command::RemoveArrangement { id: layout });
    let left: Vec<u32> = doc
        .project()
        .playlist
        .clips
        .iter()
        .map(|clip| clip.id.0)
        .collect();
    for copy in &copies {
        assert!(
            !left.contains(copy),
            "a clip only this layout placed goes with it"
        );
    }
    for original in clips {
        assert!(
            left.contains(&original),
            "a clip another layout lists stays"
        );
    }
    assert_eq!(book(&doc).active, Some(first));
    assert_eq!(book(&doc).arrangements.len(), 1);
    refused(&mut doc, Command::RemoveArrangement { id: first });
}

#[test]
fn make_unique_gives_an_automation_clip_its_own_curve() {
    let (mut doc, tracks, _) = fixture();
    let automation = edit(
        &mut doc,
        command(
            json!({"type": "addAutomation", "name": "Tempo ride", "target": {"type": "tempo"},
            "points": [
                {"tick": 0, "value": 0.25, "curve": 0.5, "hold": false},
                {"tick": 1920, "value": 0.75, "curve": 0.0, "hold": true},
                {"tick": 3840, "value": 0.5, "curve": 0.0, "hold": false},
            ]}),
        ),
    )[0];
    let placed = edit(
        &mut doc,
        command(json!({"type": "addClips", "clips": [
            {"track": tracks[1], "start": 0, "length": 3840, "content": {"type": "automation", "automation": automation}},
            {"track": tracks[1], "start": 3840, "length": 1920, "offset": 480, "muted": true, "content": {"type": "automation", "automation": automation}},
        ]})),
    );
    let original = doc
        .project()
        .automation(AutomationId(automation))
        .unwrap()
        .clone();
    let (kept, chosen) = (clip(&doc, placed[0]), clip(&doc, placed[1]));

    let created = edit(
        &mut doc,
        Command::MakeUnique {
            clip: ClipId(placed[1]),
        },
    );
    assert_eq!(created.len(), 1);
    let copy = AutomationId(created[0]);
    assert_ne!(copy, original.id);
    let copied = doc.project().automation(copy).unwrap().clone();
    assert_eq!(copied.target, original.target);
    assert_eq!(copied.points, original.points);
    assert_eq!(copied.color, original.color);
    assert_eq!(copied.name, "Tempo ride #2");
    // The copy sits right after its source in the automation list.
    let order: Vec<AutomationId> = doc.project().automations.iter().map(|a| a.id).collect();
    assert_eq!(order, vec![original.id, copy]);
    // Only the chosen clip is redirected; its window on the curve is kept.
    let mut redirected = chosen.clone();
    redirected.content = ClipContent::Automation { automation: copy };
    assert_eq!(clip(&doc, placed[1]), redirected);
    assert_eq!(clip(&doc, placed[0]), kept);

    // Editing the copy leaves the original curve as it was.
    edit(
        &mut doc,
        command(
            json!({"type": "setAutomationPoints", "id": copy.0, "points": [
                {"tick": 0, "value": 1.0, "curve": 0.0, "hold": false},
                {"tick": 960, "value": 0.0, "curve": -0.25, "hold": false},
            ]}),
        ),
    );
    assert_eq!(doc.project().automation(original.id).unwrap(), &original);
    // Writing an LFO into the original leaves the copy as it was edited.
    let edited = doc.project().automation(copy).unwrap().clone();
    edit(
        &mut doc,
        Command::GenerateAutomationLfo {
            expected: original.clone(),
            start: 0,
            end: 1920,
            resolution: 240,
            lfo: CurveLfo {
                wave: CurveLfoWave::Triangle,
                period: 960,
                phase: 0.0,
                center: 0.5,
                depth: 0.5,
                width: 0.5,
                seed: 1,
            },
        },
    );
    assert_eq!(doc.project().automation(copy).unwrap(), &edited);
    assert_ne!(
        doc.project().automation(original.id).unwrap().points,
        original.points
    );

    // Removing the copy takes only its own clip.
    edit(&mut doc, Command::RemoveAutomation { id: copy });
    assert!(
        doc.project()
            .playlist
            .clips
            .iter()
            .all(|clip| clip.id.0 != placed[1])
    );
    assert_eq!(clip(&doc, placed[0]), kept);

    refused(
        &mut doc,
        Command::MakeUnique {
            clip: ClipId(9_999),
        },
    );
}

#[test]
fn make_unique_of_every_clip_kind_is_one_batch() {
    let (mut doc, tracks, clips) = fixture();
    let automation = edit(
        &mut doc,
        command(json!({"type": "addAutomation", "target": {"type": "tempo"}})),
    )[0];
    let curve = edit(
        &mut doc,
        command(json!({"type": "addClips", "clips": [
            {"track": tracks[1], "start": 0, "length": 960, "content": {"type": "automation", "automation": automation}},
        ]})),
    )[0];
    let created = edit(
        &mut doc,
        Command::Batch {
            label: Some("Make unique".into()),
            commands: vec![
                Command::MakeUnique {
                    clip: ClipId(clips[0]),
                },
                Command::MakeUnique {
                    clip: ClipId(curve),
                },
            ],
        },
    );
    assert_eq!(created.len(), 2);
    assert_eq!(
        clip(&doc, clips[0]).content,
        ClipContent::Pattern {
            pattern: PatternId(created[0])
        }
    );
    assert_eq!(
        clip(&doc, curve).content,
        ClipContent::Automation {
            automation: AutomationId(created[1])
        }
    );
    assert_eq!(doc.history().entries.last().unwrap().label, "Make unique");
}

fn audio_clip(track: u32, sample: u32, mixer: u32, start: u32) -> serde_json::Value {
    json!({"track": track, "start": start, "length": 960, "content": {
        "type": "audio", "sample": sample, "mixerTrack": mixer, "gain": 1.0, "pan": 0.0,
        "fadeIn": 0, "fadeOut": 0, "reverse": false, "pitch": 0.0, "stretch": {"mode": "tape"},
    }})
}

fn mixer_of(doc: &Document, id: u32) -> u32 {
    match clip(doc, id).content {
        ClipContent::Audio { mixer_track, .. } => mixer_track.0,
        other => panic!("not an audio clip: {other:?}"),
    }
}

#[test]
fn an_audio_track_routes_every_audio_clip_on_it_to_its_mixer_track() {
    let (mut doc, tracks, _) = fixture();
    let created = edit(
        &mut doc,
        command(json!({"type": "addAudioTrack", "name": "Vocals"})),
    );
    let (mixer, lane) = (created[0], created[1]);
    assert_eq!(
        doc.project().mixer.track(TrackId(mixer)).unwrap().name,
        "Vocals"
    );
    let lanes = &doc.project().playlist.tracks;
    assert_eq!(lanes.last().unwrap().id.0, lane);
    assert_eq!(lanes.last().unwrap().name, "Vocals");
    assert_eq!(
        book(&doc).linked_tracks[&PlaylistTrackId(lane)],
        TrackKind::AudioTrack {
            mixer: TrackId(mixer)
        }
    );
    let sample = edit(
        &mut doc,
        Command::AddSample {
            name: "Take".into(),
            path: SamplePath::Project("take.wav".into()),
        },
    )[0];

    // Placed on the audio track: routed there whatever the clip asked for.
    let placed = edit(
        &mut doc,
        command(json!({"type": "addClips", "clips": [
            audio_clip(lane, sample, 0, 0),
            audio_clip(tracks[0], sample, 0, 960),
        ]})),
    );
    assert_eq!(mixer_of(&doc, placed[0]), mixer);
    assert_eq!(
        mixer_of(&doc, placed[1]),
        0,
        "a clip on a plain lane keeps its own routing"
    );

    // Dragged onto the audio track: rerouted in the same undo step.
    edit(
        &mut doc,
        command(
            json!({"type": "updateClips", "updates": [{"id": placed[1], "patch": {"track": lane}}]}),
        ),
    );
    assert_eq!(mixer_of(&doc, placed[1]), mixer);
    // Dragged off it again: the routing it has is kept, as on any plain lane.
    edit(
        &mut doc,
        command(
            json!({"type": "updateClips", "updates": [{"id": placed[1], "patch": {"track": tracks[0]}}]}),
        ),
    );
    assert_eq!(mixer_of(&doc, placed[1]), mixer);

    // The track owns the routing of what sits on it.
    let before = doc.snapshot(None);
    let applied = doc
        .dispatch(
            command(json!({"type": "updateAudioClips", "updates": [{"id": placed[0], "patch": {"mixerTrack": 0}}]})),
            None,
        )
        .unwrap();
    assert!(
        applied.touched.is_empty(),
        "rerouting a clip on an audio track changes nothing"
    );
    assert_eq!(doc.snapshot(None).project, before.project);

    // Linking an existing lane routes what already sits on it.
    let other = edit(
        &mut doc,
        command(json!({"type": "addMixerTrack", "name": "Bus"})),
    )[0];
    edit(
        &mut doc,
        Command::LinkTrack {
            track: PlaylistTrackId(tracks[0]),
            kind: Some(TrackKind::AudioTrack {
                mixer: TrackId(other),
            }),
        },
    );
    assert_eq!(mixer_of(&doc, placed[1]), other);
    assert_eq!(mixer_of(&doc, placed[0]), mixer);

    // Deleting the mixer track drops the link and plays the clips into the master.
    edit(&mut doc, Command::RemoveMixerTrack { id: TrackId(mixer) });
    assert!(
        !book(&doc)
            .linked_tracks
            .contains_key(&PlaylistTrackId(lane))
    );
    assert_eq!(mixer_of(&doc, placed[0]), 0);
    assert!(
        doc.project()
            .playlist
            .tracks
            .iter()
            .any(|track| track.id.0 == lane)
    );

    refused(
        &mut doc,
        Command::LinkTrack {
            track: PlaylistTrackId(tracks[1]),
            kind: Some(TrackKind::AudioTrack {
                mixer: TrackId(9_999),
            }),
        },
    );
}

#[test]
fn an_instrument_track_is_a_channel_a_mixer_track_and_a_linked_lane() {
    let (mut doc, _, _) = fixture();
    let created = edit(
        &mut doc,
        command(
            json!({"type": "addInstrumentTrack", "name": "Lead", "instrument": "subtractiveSynth"}),
        ),
    );
    assert_eq!(created.len(), 3);
    let (channel, mixer, lane) = (
        ChannelId(created[0]),
        TrackId(created[1]),
        PlaylistTrackId(created[2]),
    );
    let project = doc.project();
    let made = project.channel(channel).unwrap();
    assert_eq!(made.name, "Lead");
    assert_eq!(made.mixer_track, mixer);
    assert!(matches!(made.source, ChannelSource::Instrument { .. }));
    assert_eq!(project.mixer.track(mixer).unwrap().name, "Lead");
    assert_eq!(project.playlist.tracks.last().unwrap().id, lane);
    assert_eq!(project.playlist.tracks.last().unwrap().name, "Lead");
    assert_eq!(
        book(&doc).linked_tracks[&lane],
        TrackKind::Instrument { channel }
    );

    // A sampler instrument track takes its name from the sample.
    let sample = edit(
        &mut doc,
        Command::AddSample {
            name: "Snare".into(),
            path: SamplePath::Project("snare.wav".into()),
        },
    )[0];
    let sampler = edit(
        &mut doc,
        command(json!({"type": "addInstrumentTrack", "sample": sample})),
    );
    assert_eq!(doc.project().playlist.tracks.last().unwrap().name, "Snare");
    assert_eq!(
        doc.project().channel(ChannelId(sampler[0])).unwrap().name,
        "Snare"
    );

    // With an arrangement active, the new lane joins that layout.
    let layout = edit(
        &mut doc,
        command(json!({"type": "addArrangementLayout", "name": "B", "copy": false})),
    )[0];
    let bass = edit(
        &mut doc,
        command(json!({"type": "addInstrumentTrack", "name": "Bass"})),
    );
    let active = book(&doc)
        .arrangements
        .iter()
        .find(|item| item.id == layout)
        .unwrap();
    assert!(active.tracks.contains(&PlaylistTrackId(bass[2])));
    assert!(
        !book(&doc).arrangements[0]
            .tracks
            .contains(&PlaylistTrackId(bass[2]))
    );

    // Deleting the channel unlinks the lane and keeps it.
    edit(&mut doc, Command::RemoveChannel { id: channel });
    assert!(!book(&doc).linked_tracks.contains_key(&lane));
    assert!(
        doc.project()
            .playlist
            .tracks
            .iter()
            .any(|track| track.id == lane)
    );

    refused(
        &mut doc,
        command(
            json!({"type": "addInstrumentTrack", "sample": sample, "instrument": "subtractiveSynth"}),
        ),
    );
}

#[test]
fn files_written_before_layouts_and_linked_tracks_still_load() {
    let (doc, _, _) = fixture();
    let mut value = serde_json::to_value(doc.project()).unwrap();
    let playlist = value["playlist"].as_object_mut().unwrap();
    assert!(
        !playlist.contains_key("arrangementBook"),
        "an empty book is not written"
    );
    let legacy: Project = serde_json::from_value(value).unwrap();
    legacy.check().unwrap();
    assert!(legacy.playlist.arrangement_book.is_empty());
    assert_eq!(&legacy, doc.project());

    // An audio-track link reads back under its own tag.
    let mut doc = doc;
    let created = edit(&mut doc, command(json!({"type": "addAudioTrack"})));
    let written = serde_json::to_value(doc.project()).unwrap();
    assert_eq!(
        written["playlist"]["arrangementBook"]["linkedTracks"][created[1].to_string()],
        json!({"type": "audioTrack", "mixer": created[0]})
    );
}
