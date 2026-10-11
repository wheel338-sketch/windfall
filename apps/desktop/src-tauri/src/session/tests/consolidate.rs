//! Consolidating playlist tracks: what is printed, what is muted, and that
//! it is one undo step that leaves nothing behind when it cannot finish.

use windfall_engine::{RenderOptions, render_streaming_checked};
use windfall_ipc::PlayMode;
use windfall_project::{
    AutomationPoint, AutomationTarget, Clip, ClipAudioOutput, ClipContent, ClipId, ClipInit,
    Command, PlaylistTrackId, PlaylistTrackPatch, Project, SampleId, TickRange, TrackId,
};

use super::{Rig, SAMPLE_RATE};
use crate::session::consolidate::{consolidate_plan, consolidate_source};

fn audio(sample: SampleId) -> ClipContent {
    ClipContent::Audio {
        sample,
        mixer_track: TrackId::MASTER,
        output: Default::default(),
        normalize: false,
        gain: 0.5,
        pan: 0.0,
        fade_in: 0,
        fade_out: 0,
        reverse: false,
        pitch: 0.0,
        stretch: Default::default(),
    }
}

struct Seed {
    drums: PlaylistTrackId,
    other: PlaylistTrackId,
    curves: PlaylistTrackId,
    /// The drum lane's pattern clip, audio clip and already muted clip.
    sources: [ClipId; 3],
    /// The other lane's overlapping audio clip.
    overlapping: ClipId,
    /// The fader ride on the curve lane.
    ride: ClipId,
}

/// A kick on every beat. The drum lane holds a pattern clip, an audio clip
/// and a muted pattern clip; another lane holds an audio clip that overlaps
/// them; a third holds a curve that pulls the kick channel's volume down.
fn seed(rig: &Rig) -> Seed {
    let session = &rig.session;
    for step in [0, 4, 8, 12] {
        let command = Command::ToggleStep {
            pattern: rig.pattern(),
            channel: rig.channel(0),
            step,
        };
        session.dispatch(command, None).unwrap();
    }
    let [drums, other, curves] = ["Drums", "Other", "Curves"].map(|name| {
        let command = Command::AddPlaylistTrack {
            name: Some(name.into()),
            index: None,
        };
        PlaylistTrackId(session.dispatch(command, None).unwrap().created[0])
    });
    let automation = session
        .dispatch(
            Command::AddAutomation {
                name: Some("Kick ride".into()),
                target: AutomationTarget::ChannelVolume {
                    channel: rig.channel(0),
                },
                points: Some(vec![AutomationPoint {
                    tick: 0,
                    value: 0.25,
                    curve: 0.0,
                    hold: false,
                }]),
            },
            None,
        )
        .unwrap()
        .created[0];
    let pattern = ClipContent::Pattern {
        pattern: rig.pattern(),
    };
    let clip = |track, start, length, muted, content| ClipInit {
        track,
        start,
        length: Some(length),
        offset: None,
        muted: Some(muted),
        content,
    };
    let sample = rig.project().samples[0].id;
    let created = session
        .dispatch(
            Command::AddClips {
                clips: vec![
                    clip(drums, 960, 1_920, false, pattern.clone()),
                    clip(drums, 3_840, 960, false, audio(sample)),
                    clip(drums, 5_760, 960, true, pattern),
                    clip(other, 960, 3_840, false, audio(sample)),
                    clip(
                        curves,
                        0,
                        7_680,
                        false,
                        ClipContent::Automation {
                            automation: windfall_project::AutomationId(automation),
                        },
                    ),
                ],
            },
            None,
        )
        .unwrap()
        .created;
    let id = |index: usize| ClipId(created[index]);
    Seed {
        drums,
        other,
        curves,
        sources: [id(0), id(1), id(2)],
        overlapping: id(3),
        ride: id(4),
    }
}

fn clip(project: &Project, id: ClipId) -> &Clip {
    let clips = &project.playlist.clips;
    clips.iter().find(|clip| clip.id == id).unwrap()
}

/// Renders a stretch of a project as the session's own pool plays it.
fn rendered(rig: &Rig, project: &Project, range: Option<TickRange>) -> Vec<f32> {
    let pool = rig.session.state().pool.clone();
    let options = RenderOptions {
        region: range,
        mode: PlayMode::Song,
        sample_rate: SAMPLE_RATE,
        tail_secs: 0.0,
        ..Default::default()
    };
    let mut buffer = Vec::new();
    let result = render_streaming_checked(
        project,
        &pool,
        &options,
        &mut |block| {
            buffer.extend_from_slice(block);
            true
        },
        &mut |_| true,
    )
    .unwrap();
    assert!(result.completed);
    buffer
}

fn apart(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    let differences = a.iter().zip(b).map(|(a, b)| (a - b).abs());
    differences.fold(0.0, f32::max)
}

fn bounce_files(rig: &Rig) -> usize {
    let folder = rig.session.store().recordings_dir().join("Bounces");
    std::fs::read_dir(folder).map_or(0, |entries| entries.count())
}

#[test]
fn consolidating_a_track_prints_its_clips_and_mutes_them_in_one_undo_step() {
    let rig = Rig::new();
    let seed = seed(&rig);
    let before = rig.session.document_snapshot();
    let song_before = rendered(&rig, &before.project, None);

    let result = rig
        .session
        .consolidate_playlist_tracks(vec![seed.drums])
        .unwrap();
    let after = rig.session.document_snapshot();
    assert_eq!(result.created.len(), 2, "a sample and its clip");
    assert_eq!(
        after.history.entries.len(),
        before.history.entries.len() + 1
    );
    assert_eq!(
        after.history.entries.last().unwrap().label,
        "Consolidate track"
    );
    assert_eq!(
        after.project.playlist.tracks, before.project.playlist.tracks,
        "no lane flag is used as the render switch"
    );

    // The lane's pattern and audio clips are muted, not deleted. What was
    // muted stays so, and every other lane and the curve are as they were.
    for source in seed.sources {
        let mut expected = clip(&before.project, source).clone();
        expected.muted = true;
        assert_eq!(clip(&after.project, source), &expected);
    }
    for kept in [seed.overlapping, seed.ride] {
        assert_eq!(clip(&after.project, kept), clip(&before.project, kept));
    }

    // One print on the lane, over the span of what was rendered.
    let print = clip(&after.project, ClipId(result.created[1]));
    assert_eq!(
        (
            print.track,
            print.start,
            print.length,
            print.offset,
            print.muted
        ),
        (seed.drums, 960, 3_840, 0, false)
    );
    let ClipContent::Audio { sample, output, .. } = print.content else {
        panic!("the print is an audio clip");
    };
    assert_eq!(sample.0, result.created[0]);
    assert_eq!(
        output,
        ClipAudioOutput::Direct,
        "the render already passed the mixer"
    );
    let asset = after.project.sample(sample).unwrap();
    assert_eq!(asset.name, "Drums (consolidated)");
    assert_eq!(bounce_files(&rig), 1);

    // The print is the lane alone: the overlapping clip of the other lane
    // and the lane's own muted clip are not in it, and the curve is.
    let range = TickRange {
        start: 960,
        end: 4_800,
    };
    let printed = rig.session.state().pool.get(sample).unwrap().clone();
    let mut alone = before.project.clone();
    alone
        .playlist
        .clips
        .retain(|clip| clip.id != seed.overlapping && clip.id != seed.sources[2]);
    let reference = rendered(&rig, &alone, Some(range));
    assert!(reference.iter().any(|value| value.abs() > 0.001));
    assert!(apart(printed.samples(), &reference) < 1e-6);
    let mut unridden = alone.clone();
    unridden.playlist.clips.retain(|clip| clip.id != seed.ride);
    assert!(
        apart(&rendered(&rig, &unridden, Some(range)), &reference) > 0.001,
        "the fader ride is printed with the lane"
    );

    // The song sounds the same before and after.
    let song_after = rendered(&rig, &after.project, None);
    assert!(apart(&song_before, &song_after) < 1e-5);

    // One undo brings the sources back and takes the print out; redo
    // repeats it, with the rendered file still there to play.
    rig.session.undo().unwrap();
    let mut restored = rig.project();
    restored.next_id = before.project.next_id;
    assert_eq!(restored, before.project);
    assert_eq!(bounce_files(&rig), 1);
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), after.project);
    assert!(rig.session.state().pool.get(sample).is_some());

    // A saved project keeps the print.
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("consolidated.windfall");
    windfall_project::file::save(&after.project, &path).unwrap();
    assert_eq!(windfall_project::file::load(&path).unwrap(), after.project);
}

#[test]
fn several_tracks_get_a_print_each_in_one_undo_step() {
    let rig = Rig::new();
    let seed = seed(&rig);
    let before = rig.session.document_snapshot();
    // Asked for in the other order; the lanes come out in playlist order.
    let result = rig
        .session
        .consolidate_playlist_tracks(vec![seed.other, seed.drums])
        .unwrap();
    let after = rig.session.document_snapshot();
    assert_eq!(
        after.history.entries.len(),
        before.history.entries.len() + 1
    );
    assert_eq!(
        after.history.entries.last().unwrap().label,
        "Consolidate tracks"
    );
    assert_eq!(result.created.len(), 4);
    let prints = [
        clip(&after.project, ClipId(result.created[1])),
        clip(&after.project, ClipId(result.created[3])),
    ];
    assert_eq!(
        (prints[0].track, prints[0].start, prints[0].length),
        (seed.drums, 960, 3_840)
    );
    assert_eq!(
        (prints[1].track, prints[1].start, prints[1].length),
        (seed.other, 960, 3_840)
    );
    assert_eq!(bounce_files(&rig), 2);
    assert!(clip(&after.project, seed.overlapping).muted);
    assert!(!clip(&after.project, seed.ride).muted);

    // Each print holds its own lane only.
    let pool = rig.session.state().pool.clone();
    let range = TickRange {
        start: 960,
        end: 4_800,
    };
    for (print, kept) in [(prints[0], seed.drums), (prints[1], seed.other)] {
        let mut alone = before.project.clone();
        alone.playlist.clips.retain(|clip| {
            clip.track == kept || matches!(clip.content, ClipContent::Automation { .. })
        });
        let printed = pool.get(print.content.sample().unwrap()).unwrap();
        assert!(apart(printed.samples(), &rendered(&rig, &alone, Some(range))) < 1e-6);
    }

    rig.session.undo().unwrap();
    let mut restored = rig.project();
    restored.next_id = before.project.next_id;
    assert_eq!(restored, before.project);
}

#[test]
fn a_consolidation_that_cannot_finish_changes_nothing_and_leaves_no_file() {
    let rig = Rig::new();
    let seed = seed(&rig);
    let before = rig.session.document_snapshot();
    let refused = |tracks: Vec<PlaylistTrackId>| {
        let error = rig.session.consolidate_playlist_tracks(tracks).unwrap_err();
        assert_eq!(rig.session.document_snapshot(), before);
        assert_eq!(bounce_files(&rig), 0);
        error
    };
    assert_eq!(
        refused(Vec::new()),
        "Choose at least one playlist track to consolidate."
    );
    assert_eq!(
        refused(vec![PlaylistTrackId(999_999)]),
        "A chosen playlist track no longer exists."
    );
    // A lane that holds only a curve has nothing to print, and stops the
    // lanes asked for with it.
    assert_eq!(
        refused(vec![seed.drums, seed.curves]),
        "\"Curves\" has no unmuted pattern or audio clip to consolidate."
    );

    // An edit made while the lanes render stops the commit.
    let hold = rig.session.hold("consolidate:rendered");
    let tracks = vec![seed.drums, seed.other];
    let worker = rig
        .session
        .background(move |session| session.consolidate_playlist_tracks(tracks));
    hold.wait();
    assert_eq!(bounce_files(&rig), 2, "both lanes were rendered");
    let rename = Command::UpdatePlaylistTrack {
        id: seed.other,
        patch: PlaylistTrackPatch {
            name: Some("Renamed".into()),
            ..Default::default()
        },
    };
    rig.session.dispatch(rename, None).unwrap();
    let edited = rig.session.document_snapshot();
    hold.release();
    let error = worker.join().unwrap().unwrap_err();
    assert!(error.contains("changed while consolidating"), "{error}");
    assert_eq!(rig.session.document_snapshot(), edited);
    assert_eq!(
        bounce_files(&rig),
        0,
        "the renders of a refused commit are removed"
    );
}

#[test]
fn the_plan_follows_the_active_arrangement_and_the_print_joins_it() {
    let rig = Rig::new();
    let seed = seed(&rig);
    // A copied layout in which the drum lane's audio clip is deleted.
    let created = rig
        .session
        .dispatch(
            Command::AddArrangementLayout {
                name: "Short".into(),
                copy: true,
            },
            None,
        )
        .unwrap()
        .created;
    let (layout, copies) = (created[0], created[1..].to_vec());
    let project = rig.project();
    let copy_of = |original: ClipId| {
        let original = clip(&project, original);
        project
            .playlist
            .clips
            .iter()
            .find(|clip| {
                copies.contains(&clip.id.0)
                    && clip.track == original.track
                    && clip.start == original.start
            })
            .unwrap()
            .id
    };
    let (pattern_copy, audio_copy) = (copy_of(seed.sources[0]), copy_of(seed.sources[1]));
    let remove = Command::RemoveClips {
        clips: vec![audio_copy],
    };
    rig.session.dispatch(remove, None).unwrap();

    let project = rig.project();
    let plan = consolidate_plan(&project, &[seed.drums]).unwrap();
    assert_eq!(plan.len(), 1);
    assert_eq!(
        plan[0].sources,
        vec![pattern_copy],
        "only what this layout shows"
    );
    assert_eq!((plan[0].range.start, plan[0].range.end), (960, 2_880));
    // The render copy keeps every curve playing and silences other sound.
    let source = consolidate_source(&project, &plan[0]);
    for clip in &source.playlist.clips {
        let original = self::clip(&project, clip.id);
        let curve = matches!(clip.content, ClipContent::Automation { .. });
        assert_eq!(
            clip.muted,
            original.muted || (!curve && clip.id != pattern_copy)
        );
    }
    assert!(source.playlist.tracks.iter().all(|track| !track.solo));

    let result = rig
        .session
        .consolidate_playlist_tracks(vec![seed.drums])
        .unwrap();
    let after = rig.project();
    let print = ClipId(result.created[1]);
    assert_eq!(clip(&after, print).length, 1_920);
    let book = &after.playlist.arrangement_book;
    let active = book
        .arrangements
        .iter()
        .find(|item| item.id == layout)
        .unwrap();
    assert!(
        active.clips.contains(&print),
        "the print is part of the layout it was made in"
    );
    assert!(!book.arrangements[0].clips.contains(&print));
    // The first layout's own clips were neither rendered nor muted.
    for original in &seed.sources[..2] {
        assert!(!clip(&after, *original).muted);
    }
    assert!(clip(&after, pattern_copy).muted);
}

#[test]
fn a_muted_or_unsoloed_lane_is_still_consolidated_as_it_sounds_alone() {
    let rig = Rig::new();
    let seed = seed(&rig);
    let flags = |id, patch| {
        let command = Command::UpdatePlaylistTrack { id, patch };
        rig.session.dispatch(command, None).unwrap();
    };
    flags(
        seed.drums,
        PlaylistTrackPatch {
            muted: Some(true),
            ..Default::default()
        },
    );
    flags(
        seed.other,
        PlaylistTrackPatch {
            solo: Some(true),
            ..Default::default()
        },
    );
    let before = rig.project();
    let result = rig
        .session
        .consolidate_playlist_tracks(vec![seed.drums])
        .unwrap();
    let after = rig.project();
    assert_eq!(after.playlist.tracks, before.playlist.tracks);
    let print = clip(&after, ClipId(result.created[1]));
    let printed = rig
        .session
        .state()
        .pool
        .get(print.content.sample().unwrap())
        .unwrap()
        .clone();
    assert!(
        printed.samples().iter().any(|value| value.abs() > 0.001),
        "the live mute and solo flags did not silence the render"
    );
}
