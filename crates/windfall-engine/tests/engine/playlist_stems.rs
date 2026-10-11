//! Stems by playlist track: what each lane's stream holds, how it differs
//! from a mixer stem, and that the streams line up with the mix.

use windfall_engine::{
    PlaylistStem, PlaylistStemError, PlaylistStemOptions, RenderOptions, StemMode, StemOptions,
    Streamed, playlist_stem_project, playlist_stems, render, render_playlist_stems, stems,
};
use windfall_ipc::PlayMode;
use windfall_project::arrangement::Arrangement;
use windfall_project::{
    ArrangementBook, AutomationTarget, ClipAudioOutput, ClipContent, ClipId, PlaylistTrackId,
    TrackId,
};

use crate::support::{Rig, left, level, limiter, peak};

const RATE: u32 = 48_000;

/// Frames in a tick at 120 bpm and 48 kHz.
const TICK: usize = 25;

struct Streams {
    list: Vec<PlaylistStem>,
    audio: Vec<Vec<f32>>,
    outcome: Streamed,
}

impl Streams {
    /// The left side of the stream of this lane, or of the mix for `None`.
    fn of(&self, track: Option<PlaylistTrackId>) -> Vec<f32> {
        let found = self.list.iter().position(|stem| stem.track == track);
        left(&self.audio[found.expect("the stream was rendered")])
    }

    fn names(&self) -> Vec<&str> {
        self.list.iter().map(|stem| stem.name.as_str()).collect()
    }
}

fn all() -> PlaylistStemOptions {
    PlaylistStemOptions {
        tracks: None,
        include_mix: true,
        numbered: false,
    }
}

fn song(block: usize) -> RenderOptions {
    RenderOptions {
        mode: PlayMode::Song,
        sample_rate: RATE,
        block_frames: block,
        tail_secs: 0.0,
        ..RenderOptions::default()
    }
}

fn render_streams(
    rig: &Rig,
    options: &RenderOptions,
    stem_options: &PlaylistStemOptions,
) -> Streams {
    let list = playlist_stems(&rig.project, stem_options).unwrap();
    let mut audio = vec![Vec::new(); list.len()];
    let outcome = render_playlist_stems(
        &rig.project,
        &rig.pool,
        options,
        stem_options,
        &mut |stream, block| {
            assert_eq!(block.len() % 2, 0);
            audio[stream].extend_from_slice(block);
            true
        },
        &mut |_| true,
    )
    .unwrap();
    Streams {
        list,
        audio,
        outcome,
    }
}

fn same_bits(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits())
}

/// One mixer track that everything plays into, a master at half, and five
/// lanes: a pattern, an audio clip, the same pattern again a bar on, a
/// curve that holds the mixer track's fader at half, and a spare.
struct Studio {
    rig: Rig,
    room: TrackId,
    keys: PlaylistTrackId,
    vocal: PlaylistTrackId,
    reprise: PlaylistTrackId,
    curves: PlaylistTrackId,
    spare: PlaylistTrackId,
    vocal_clip: ClipId,
}

fn studio() -> Studio {
    let mut rig = Rig::new();
    let room = rig.track();
    rig.track_mut(room).name = "Room".into();
    rig.track_mut(TrackId::MASTER).volume = 0.5;
    let piano = rig.channel_on(level(RATE, 0.25, 0.5), room);
    rig.note(piano, 0, 240);
    let lanes = [
        rig.playlist_track(),
        rig.playlist_track(),
        rig.playlist_track(),
        rig.playlist_track(),
        rig.playlist_track(),
    ];
    for (track, name) in rig
        .project
        .playlist
        .tracks
        .iter_mut()
        .zip(["Keys", "Vocal", "keys", "Curves", "Spare"])
    {
        track.name = name.into();
    }
    let [keys, vocal, reprise, curves, spare] = lanes;
    let pattern = rig.first_pattern();
    rig.clip(keys, pattern, 0, 1_920);
    let vocal_clip = rig.audio_clip(vocal, level(RATE, 0.125, 4.0), room, 0, 3_840);
    rig.clip(reprise, pattern, 3_840, 1_920);
    let fader = rig.automation(AutomationTarget::TrackVolume { track: room }, &[(0, 0.5)]);
    rig.automation_clip(curves, fader, 0, 5_760);
    Studio {
        rig,
        room,
        keys,
        vocal,
        reprise,
        curves,
        spare,
        vocal_clip,
    }
}

/// Half from the automated fader (a value of 0.5 is -6 dB) and half from
/// the master: what one unit of level on the room track comes out as.
const THROUGH: f32 = 0.25;

#[test]
fn a_lane_is_a_stem_even_where_lanes_share_a_pattern_a_channel_and_a_mixer_track() {
    let studio = studio();
    let rig = &studio.rig;
    let streams = render_streams(rig, &song(480), &all());
    assert_eq!(streams.names(), ["Mix", "Keys", "Vocal", "keys 2"]);
    assert!(streams.outcome.completed);
    assert_eq!(streams.outcome.dropped_clips, 0);

    // Everything here plays into one mixer track, so the mixer has a
    // single stem to give where the playlist has three.
    let mixer = StemOptions {
        mode: StemMode::ToMaster,
        tracks: None,
        include_mix: false,
        numbered: false,
    };
    let mixer_stems = stems(&rig.project, &mixer).unwrap();
    assert_eq!(mixer_stems.len(), 1);
    assert_eq!(mixer_stems[0].track, Some(studio.room));

    let mix = streams.of(None);
    let keys = streams.of(Some(studio.keys));
    let vocal = streams.of(Some(studio.vocal));
    let reprise = streams.of(Some(studio.reprise));
    let early = 1_000;
    let late = 3_840 * TICK + 1_000;
    assert_eq!(mix[early], (0.25 + 0.125) * THROUGH);
    assert_eq!(
        keys[early],
        0.25 * THROUGH,
        "the pattern's note, through the fader ride and the master"
    );
    assert_eq!(
        keys[late], 0.0,
        "the same pattern on another lane is not this lane's"
    );
    assert_eq!(vocal[early], 0.125 * THROUGH);
    assert_eq!(vocal[late], 0.0);
    assert_eq!(reprise[early], 0.0);
    assert_eq!(reprise[late], 0.25 * THROUGH);

    // Every stream is as long as the mix, which is as long as a plain render.
    let plain = render(&rig.project, &rig.pool, &song(480), &mut |_| true);
    assert_eq!(streams.outcome.frames as usize, plain.frames());
    assert_eq!(plain.frames(), 5_760 * TICK);
    for audio in &streams.audio {
        assert_eq!(audio.len(), plain.frames() * 2);
    }
    assert!(same_bits(&streams.audio[0], plain.samples()));

    // Nothing on the way is level-dependent, so the lanes add up to the mix.
    for frame in 0..mix.len() {
        let sum = keys[frame] + vocal[frame] + reprise[frame];
        assert!(
            (sum - mix[frame]).abs() < 1e-6,
            "frame {frame}: {sum} not {}",
            mix[frame]
        );
    }
}

#[test]
fn the_names_number_the_lanes_by_their_place_on_the_timeline() {
    let studio = studio();
    let numbered = PlaylistStemOptions {
        numbered: true,
        ..all()
    };
    let list = playlist_stems(&studio.rig.project, &numbered).unwrap();
    let names: Vec<&str> = list.iter().map(|stem| stem.name.as_str()).collect();
    assert_eq!(names, ["Mix", "01 Keys", "02 Vocal", "03 keys"]);
    let tracks: Vec<_> = list.iter().map(|stem| stem.track).collect();
    assert_eq!(
        tracks,
        [
            None,
            Some(studio.keys),
            Some(studio.vocal),
            Some(studio.reprise)
        ]
    );

    // A name that cannot be part of a file name is cleaned, an empty one
    // gets the lane's place.
    let mut rig = studio.rig;
    rig.project.playlist.tracks[0].name = "Keys: <left>/".into();
    rig.project.playlist.tracks[1].name = "  ".into();
    let list = playlist_stems(&rig.project, &all()).unwrap();
    let names: Vec<&str> = list.iter().map(|stem| stem.name.as_str()).collect();
    assert_eq!(names, ["Mix", "Keys left", "Track 2", "keys"]);
}

#[test]
fn part_of_the_lanes_gives_the_same_stems_whatever_the_block_size() {
    let studio = studio();
    let rig = &studio.rig;
    let whole = render_streams(rig, &song(480), &all());
    let only = PlaylistStemOptions {
        tracks: Some(vec![studio.reprise, studio.keys]),
        include_mix: false,
        numbered: false,
    };
    for block in [64, 1_024] {
        let part = render_streams(rig, &song(block), &only);
        assert_eq!(
            part.names(),
            ["Keys", "keys 2"],
            "in playlist order, whatever order was asked"
        );
        for lane in [studio.keys, studio.reprise] {
            assert!(same_bits(&part.of(Some(lane)), &whole.of(Some(lane))));
        }
        assert_eq!(part.outcome.frames, whole.outcome.frames);
    }

    // A stem is the song with only that lane's pattern and audio clips placed.
    let alone = playlist_stem_project(&rig.project, studio.vocal);
    assert_eq!(alone.playlist.clips.len(), rig.project.playlist.clips.len());
    for (stem, original) in alone.playlist.clips.iter().zip(&rig.project.playlist.clips) {
        let sound = !matches!(original.content, ClipContent::Automation { .. });
        assert_eq!(stem.muted, sound && original.track != studio.vocal);
    }
    let rendered = render(&alone, &rig.pool, &song(480), &mut |_| true);
    assert!(same_bits(
        &left(rendered.samples()),
        &whole.of(Some(studio.vocal))
    ));
}

#[test]
fn an_automatic_tail_ends_every_stem_where_it_ends_the_mix() {
    let studio = studio();
    let rig = &studio.rig;
    let options = RenderOptions {
        tail_secs: 2.0,
        auto_tail: true,
        ..song(480)
    };
    let only = PlaylistStemOptions {
        tracks: Some(vec![studio.keys]),
        include_mix: false,
        numbered: false,
    };
    let stems = render_streams(rig, &options, &only);
    let mix = render(&rig.project, &rig.pool, &options, &mut |_| true);
    assert_eq!(stems.outcome.frames as usize, mix.frames());
    assert_eq!(stems.audio[0].len(), mix.frames() * 2);
}

#[test]
fn mute_solo_and_empty_lanes_decide_who_has_something_to_give() {
    let Studio {
        mut rig,
        keys,
        vocal,
        reprise,
        curves,
        spare,
        vocal_clip,
        ..
    } = studio();
    let asked = |tracks: Vec<PlaylistTrackId>| PlaylistStemOptions {
        tracks: Some(tracks),
        include_mix: false,
        numbered: false,
    };
    let chosen = |rig: &Rig| -> Vec<Option<PlaylistTrackId>> {
        let list = playlist_stems(&rig.project, &all()).unwrap();
        list.iter().map(|stem| stem.track).collect()
    };
    assert_eq!(chosen(&rig), [None, Some(keys), Some(vocal), Some(reprise)]);
    for silent in [spare, curves] {
        assert_eq!(
            playlist_stems(&rig.project, &asked(vec![silent])),
            Err(PlaylistStemError::Silent(silent)),
            "a lane with no pattern or audio clip"
        );
    }
    assert_eq!(
        playlist_stems(&rig.project, &asked(vec![PlaylistTrackId(9_999)])),
        Err(PlaylistStemError::UnknownTrack(PlaylistTrackId(9_999)))
    );
    assert_eq!(
        playlist_stems(&rig.project, &asked(Vec::new())),
        Err(PlaylistStemError::NothingChosen)
    );

    // A muted clip gives nothing; neither does a muted lane.
    rig.clip_mut(vocal_clip).muted = true;
    assert_eq!(chosen(&rig), [None, Some(keys), Some(reprise)]);
    rig.clip_mut(vocal_clip).muted = false;
    rig.project.playlist.tracks[1].muted = true;
    assert_eq!(chosen(&rig), [None, Some(keys), Some(reprise)]);
    assert_eq!(
        playlist_stems(&rig.project, &asked(vec![vocal])),
        Err(PlaylistStemError::Silent(vocal))
    );
    rig.project.playlist.tracks[1].muted = false;

    // While a lane is soloed, the mix plays only it, and so do the stems.
    // The curve's lane is silenced with the rest, in the stem as in the mix.
    rig.project.playlist.tracks[0].solo = true;
    assert_eq!(chosen(&rig), [None, Some(keys)]);
    let soloed = render_streams(&rig, &song(480), &all());
    assert_eq!(
        soloed.of(Some(keys))[1_000],
        0.25 * 0.5,
        "the fader ride is silenced by solo"
    );
    assert!(same_bits(&soloed.of(Some(keys)), &soloed.of(None)));
    rig.project.playlist.tracks[0].solo = false;

    // Nothing at all to give.
    for clip in &mut rig.project.playlist.clips {
        clip.muted = true;
    }
    assert_eq!(
        playlist_stems(&rig.project, &all()),
        Err(PlaylistStemError::NoTracks)
    );
    let mut nothing = |_: usize, _: &[f32]| true;
    assert_eq!(
        render_playlist_stems(
            &rig.project,
            &rig.pool,
            &song(480),
            &all(),
            &mut nothing,
            &mut |_| true
        ),
        Err(PlaylistStemError::NoTracks)
    );
}

#[test]
fn a_level_dependent_master_reacts_to_each_lane_alone_and_stays_aligned() {
    let studio = studio();
    let mut rig = studio.rig;
    // Holds the output to about 0.0316: the mix and the keys are over it,
    // the vocal alone is just under. It also puts the master 5 ms behind.
    rig.effect(TrackId::MASTER, limiter(-30.0));
    let streams = render_streams(&rig, &song(480), &all());
    let (mix, keys, vocal) = (
        streams.of(None),
        streams.of(Some(studio.keys)),
        streams.of(Some(studio.vocal)),
    );
    let ceiling = 10.0_f32.powf(-30.0 / 20.0);
    let window = 4_000..20_000;
    assert!(peak(&mix[window.clone()]) <= ceiling * 1.001);
    assert!(peak(&keys[window.clone()]) <= ceiling * 1.001);
    assert!(
        keys[10_000] > ceiling * 0.9,
        "the keys alone still reach the ceiling"
    );
    assert!(
        (vocal[10_000] - 0.125 * THROUGH).abs() < 1e-4,
        "the vocal alone is left as it is"
    );
    // So the stems do not add up to the mix: each was limited for itself.
    assert!(keys[10_000] + vocal[10_000] > mix[10_000] * 1.8);
    // What the limiter is behind is left off the front of every stream:
    // the note on the first tick is on the first frames of its stem.
    assert!(keys[..64].iter().all(|sample| *sample > 0.0));
    assert!(vocal[200] > 0.0);
    assert_eq!(streams.audio[1].len(), streams.audio[0].len());
}

#[test]
fn a_print_belongs_to_its_lane_and_an_arrangement_chooses_the_lanes() {
    let studio = studio();
    let mut rig = studio.rig;
    // A finished print on the spare lane plays straight to the output.
    let print = rig.audio_clip(
        studio.spare,
        level(RATE, 0.2, 4.0),
        TrackId::MASTER,
        0,
        3_840,
    );
    let clip = rig.clip_mut(print);
    if let ClipContent::Audio { output, .. } = &mut clip.content {
        *output = ClipAudioOutput::Direct;
    }
    let streams = render_streams(&rig, &song(480), &all());
    assert_eq!(streams.names(), ["Mix", "Keys", "Vocal", "keys 2", "Spare"]);
    assert_eq!(
        streams.of(Some(studio.spare))[1_000],
        0.2,
        "past the master, as it plays"
    );
    assert_eq!(streams.of(Some(studio.keys))[1_000], 0.25 * THROUGH);
    assert_eq!(streams.of(None)[1_000], (0.25 + 0.125) * THROUGH + 0.2);

    // With an arrangement active, its lanes and clips are the song.
    rig.project.playlist.arrangement_book = ArrangementBook {
        active: Some(900),
        arrangements: vec![Arrangement {
            id: 900,
            name: "Verse".into(),
            clips: vec![studio.vocal_clip, print],
            tracks: vec![studio.spare, studio.vocal, studio.keys],
        }],
        ..Default::default()
    };
    let numbered = PlaylistStemOptions {
        numbered: true,
        ..all()
    };
    let layout = render_streams(&rig, &song(480), &numbered);
    assert_eq!(layout.names(), ["Mix", "01 Spare", "02 Vocal"]);
    // The curve's clip is not in this layout, so the fader is at unity.
    assert_eq!(layout.of(Some(studio.vocal))[1_000], 0.125 * 0.5);
    assert_eq!(layout.outcome.frames as usize, 3_840 * TICK);
    assert_eq!(
        playlist_stems(
            &rig.project,
            &PlaylistStemOptions {
                tracks: Some(vec![studio.reprise]),
                include_mix: false,
                numbered: false,
            }
        ),
        Err(PlaylistStemError::UnknownTrack(studio.reprise)),
        "a lane the active arrangement does not show"
    );
}

#[test]
fn a_playlist_stem_render_stops_when_told_and_reports_all_of_its_passes() {
    let studio = studio();
    let rig = &studio.rig;
    // Stopped by the sink part way into the second lane.
    let mut taken = 0;
    let stopped = render_playlist_stems(
        &rig.project,
        &rig.pool,
        &song(480),
        &all(),
        &mut |stream, _| {
            taken += usize::from(stream == 2);
            !(stream == 2 && taken == 3)
        },
        &mut |_| true,
    )
    .unwrap();
    assert!(!stopped.completed);

    // Stopped by the progress callback at once.
    let cancelled = render_playlist_stems(
        &rig.project,
        &rig.pool,
        &song(480),
        &all(),
        &mut |_, _| true,
        &mut |_| false,
    )
    .unwrap();
    assert!(!cancelled.completed);

    // Progress covers the mix and the three lanes, in order, up to all of it.
    let mut reports = Vec::new();
    let done = render_playlist_stems(
        &rig.project,
        &rig.pool,
        &song(4_800),
        &all(),
        &mut |_, _| true,
        &mut |fraction| {
            reports.push(fraction);
            true
        },
    )
    .unwrap();
    assert!(done.completed);
    assert!(reports.is_sorted());
    assert_eq!(*reports.last().unwrap(), 1.0);
    assert!(
        reports
            .iter()
            .any(|fraction| *fraction > 0.2 && *fraction < 0.3)
    );

    // Lanes are the song's: a pattern render has none.
    let pattern = RenderOptions {
        mode: PlayMode::Pattern,
        ..song(480)
    };
    assert_eq!(
        render_playlist_stems(
            &rig.project,
            &rig.pool,
            &pattern,
            &all(),
            &mut |_, _| true,
            &mut |_| true
        ),
        Err(PlaylistStemError::PatternMode)
    );
    let _ = studio.curves;
}
