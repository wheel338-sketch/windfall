//! Stems by playlist track: each lane of the timeline as a file of its own.
//!
//! # What a playlist stem is
//!
//! A mixer stem follows a mixer track. A playlist stem follows the music
//! that is placed on one lane of the timeline: the song with only that
//! lane's pattern and audio clips placed, heard at the output. Two lanes may
//! play the same pattern, the same channel or the same mixer track, so no
//! tap on the mixer can tell them apart; the song is rendered once for each
//! lane, and once more for the mix when it is wanted or has to say how long
//! an automatic tail is.
//!
//! Everything between a clip and the output stays in place. A stem passes
//! through its channels' and clips' mixer tracks, their sends, the buses
//! they reach and the master's own effects and fader, as it does when the
//! lane is soloed. Where that path is linear the stems of all the lanes add
//! up to the mix. A compressor, limiter or other level-dependent processor
//! on a bus or on the master reacts to each stem alone, and a sidechain
//! detector hears only the lane being rendered: a kick on another lane does
//! not duck this one. Those stems do not add up to the mix, by design.
//!
//! Automation is control, not sound. Every automation clip that plays in
//! the mix plays in every stem, wherever it sits, so a fader ride or a tempo
//! change is the same in each file. A finished print that plays straight to
//! the output belongs to the lane it sits on.
//!
//! Mute and solo are as the project has them. A lane that the mix does not
//! play, because it is muted or because another lane is soloed, has nothing
//! to give and is left out; asking for it by id is an error. Within a stem
//! the lane is alone, so a note is never cut by a note of another lane, and
//! an audio clip never loses its slot to one.
//!
//! With an active arrangement the lanes are the ones that arrangement
//! lists, in its order, with the clips it lists.
//!
//! # Time
//!
//! Every stream starts where the song starts and is as long as the mix, to
//! the frame: the muted clips still take up their room on the timeline, and
//! an automatic tail ends every stem where it ends the mix.

use std::collections::HashSet;

use windfall_ipc::PlayMode;
use windfall_project::{ClipContent, PlaylistTrackId, Project};

use super::{MIX_NAME, Pass, StemError, Streamed, compile, file_name_part};
use crate::plan::{arrangement_playlist, silent_playlist_tracks};
use crate::pool::SamplePool;
use crate::render::RenderOptions;

/// Which playlist stems to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistStemOptions {
    /// The playlist tracks to render. `None` is every lane of the layout
    /// that the mix plays and that holds a pattern or audio clip.
    pub tracks: Option<Vec<PlaylistTrackId>>,
    /// Renders the whole mix as well, as the first stream.
    pub include_mix: bool,
    /// Puts each lane's place on the timeline in front of its name, as in
    /// "03 Bass", so that the stems sort the way the playlist shows them.
    pub numbered: bool,
}

/// One stream of a playlist stem render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistStem {
    /// A name for the stream that is safe as part of a file name and that
    /// no other stream of the render has, whatever the letter case.
    pub name: String,
    /// The playlist track the stem is of. `None` for the mix.
    pub track: Option<PlaylistTrackId>,
}

/// Why playlist stems cannot be rendered. The messages are written to be
/// shown to the user as they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistStemError {
    /// The renderer failed the way it can for any export.
    Render(StemError),
    /// Playlist tracks exist in the song only.
    PatternMode,
    /// A track was asked for that the active layout does not show.
    UnknownTrack(PlaylistTrackId),
    /// A track was asked for that the mix does not play, or that holds no
    /// pattern or audio clip.
    Silent(PlaylistTrackId),
    /// No track was asked for, and the mix was not either.
    NothingChosen,
    /// Every lane with something to give was asked for, and there is none.
    NoTracks,
}

impl std::fmt::Display for PlaylistStemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Render(error) => std::fmt::Display::fmt(error, f),
            Self::PatternMode => f.write_str(
                "Playlist tracks belong to the song. Export the song to get a file for each of them.",
            ),
            Self::UnknownTrack(id) => write!(
                f,
                "The playlist has no track with the id {} in this arrangement.",
                id.0
            ),
            Self::Silent(id) => write!(
                f,
                "Playlist track {} plays nothing: it is muted, silenced by solo, or holds no pattern or audio clip.",
                id.0
            ),
            Self::NothingChosen => f.write_str("Choose at least one playlist track to export."),
            Self::NoTracks => f.write_str(
                "No playlist track plays a pattern or audio clip, so there are no tracks to export.",
            ),
        }
    }
}

impl std::error::Error for PlaylistStemError {}

impl From<StemError> for PlaylistStemError {
    fn from(error: StemError) -> Self {
        Self::Render(error)
    }
}

/// The lanes of the active layout that the mix plays and that hold sound,
/// in the order the playlist shows them, each with its place among all the
/// lanes, counted from one.
fn giving(project: &Project) -> Vec<(usize, PlaylistTrackId)> {
    let playlist = arrangement_playlist(&project.playlist);
    let silent = silent_playlist_tracks(&playlist);
    let sounding: HashSet<PlaylistTrackId> = playlist
        .clips
        .iter()
        .filter(|clip| {
            !clip.muted
                && clip.length > 0
                && !matches!(clip.content, ClipContent::Automation { .. })
        })
        .map(|clip| clip.track)
        .collect();
    playlist
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, track)| !silent.contains(&track.id) && sounding.contains(&track.id))
        .map(|(index, track)| (index + 1, track.id))
        .collect()
}

/// The streams [`render_playlist_stems`] makes for a project, in the order
/// it numbers them: the mix first if it is asked for, then the lanes in
/// playlist order.
pub fn playlist_stems(
    project: &Project,
    options: &PlaylistStemOptions,
) -> Result<Vec<PlaylistStem>, PlaylistStemError> {
    let playlist = arrangement_playlist(&project.playlist);
    let giving = giving(project);
    let chosen: Vec<(usize, PlaylistTrackId)> = match &options.tracks {
        Some(ids) => {
            for &id in ids {
                if !playlist.tracks.iter().any(|track| track.id == id) {
                    return Err(PlaylistStemError::UnknownTrack(id));
                }
                if !giving.iter().any(|&(_, track)| track == id) {
                    return Err(PlaylistStemError::Silent(id));
                }
            }
            giving
                .into_iter()
                .filter(|(_, track)| ids.contains(track))
                .collect()
        }
        None if giving.is_empty() => return Err(PlaylistStemError::NoTracks),
        None => giving,
    };
    if chosen.is_empty() && !options.include_mix {
        return Err(PlaylistStemError::NothingChosen);
    }

    let digits = playlist.tracks.len().to_string().len().max(2);
    let mut stems: Vec<PlaylistStem> = Vec::with_capacity(chosen.len() + 1);
    let mut add = |wanted: String, track: Option<PlaylistTrackId>| {
        let taken = |name: &str| {
            let name = name.to_lowercase();
            stems.iter().any(|stem| stem.name.to_lowercase() == name)
        };
        let mut name = wanted.clone();
        let mut copy = 2;
        while taken(&name) {
            name = format!("{wanted} {copy}");
            copy += 1;
        }
        stems.push(PlaylistStem { name, track });
    };
    if options.include_mix {
        add(MIX_NAME.to_owned(), None);
    }
    for (place, id) in chosen {
        let track = playlist.tracks.iter().find(|track| track.id == id);
        let mut name = track.map_or_else(String::new, |track| file_name_part(&track.name));
        if name.is_empty() {
            name = format!("Track {place}");
        }
        if options.numbered {
            name = format!("{place:0digits$} {name}");
        }
        add(name, Some(id));
    }
    Ok(stems)
}

/// The project a playlist stem is rendered from: `project` with the pattern
/// and audio clips of every other lane muted. Muted clips keep their room,
/// so the song is as long as it was. Lanes the mix does not play are muted
/// outright before solo is cleared, which keeps their automation as silent
/// as the mix has it, and every other automation clip keeps playing.
pub fn playlist_stem_project(project: &Project, track: PlaylistTrackId) -> Project {
    let silent = silent_playlist_tracks(&arrangement_playlist(&project.playlist));
    let mut stem = project.clone();
    // Video placements have their own timeline lane, outside playlist track
    // IDs. Keep their length, but include sound only in the mix/mixer stems.
    for video in &mut stem.settings.stage_media.videos {
        video.muted = true;
    }
    for lane in &mut stem.playlist.tracks {
        lane.muted = lane.muted || silent.contains(&lane.id);
        lane.solo = false;
    }
    for clip in &mut stem.playlist.clips {
        if clip.track != track && !matches!(clip.content, ClipContent::Automation { .. }) {
            clip.muted = true;
        }
    }
    stem
}

/// Renders the playlist stems of a project and hands each to `sink` a block
/// at a time: the index of the stream in the list [`playlist_stems`] gives
/// for the same project and options, and its next frames as interleaved
/// stereo. The streams arrive one after the other.
///
/// Every stream is the same audio for every block size, starts where the
/// song starts and is as long as the mix. Returning `false` from `sink` or
/// from `progress` ends the render. `progress` is given the share of all
/// the work that is done.
pub fn render_playlist_stems(
    project: &Project,
    pool: &SamplePool,
    options: &RenderOptions,
    stem_options: &PlaylistStemOptions,
    sink: &mut dyn FnMut(usize, &[f32]) -> bool,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Result<Streamed, PlaylistStemError> {
    if options.mode != PlayMode::Song {
        return Err(PlaylistStemError::PatternMode);
    }
    options.check_region().map_err(StemError::Timeline)?;
    if let Some(error) = pool.video_audio_error(project) {
        return Err(StemError::Timeline(error).into());
    }
    let list = playlist_stems(project, stem_options)?;
    let prepared_pool = pool
        .prepare_plugin_render()
        .prepare_samplers(project, &mut || true, &mut |_, _, _| {})
        .map_err(StemError::SamplerPreparation)?;
    let pool = &prepared_pool;
    let mix = list.iter().position(|stem| stem.track.is_none());
    let tracks: Vec<(usize, PlaylistTrackId)> = list
        .iter()
        .enumerate()
        .filter_map(|(stream, stem)| Some((stream, stem.track?)))
        .collect();

    // The mix comes first even when it is not wanted, where only it can
    // say how long an automatic tail is.
    let linked_tempo = compile(project, pool).linked_tempo();
    let mix_pass = mix.is_some() || options.auto_tail || linked_tempo;
    let mut tempo_trajectory = Vec::new();
    let mut frame_map = None;
    let passes = tracks.len() + usize::from(mix_pass);
    let share =
        |pass: usize| move |fraction: f32| (pass as f32 + fraction.clamp(0.0, 1.0)) / passes as f32;
    let mut length = None;
    let mut frames = 0;
    let mut dropped_clips = 0;
    if mix_pass {
        let scale = share(0);
        let pass = Pass::new(compile(project, pool), options, &[]);
        let done = pass?.run_clock(
            None,
            &mut |_, block| mix.is_none_or(|stream| sink(stream, block)),
            &mut |fraction| progress(scale(fraction)),
            linked_tempo.then_some(&mut tempo_trajectory),
        )?;
        if !done.completed {
            return Ok(done);
        }
        length = Some(done.frames as usize);
        frames = done.frames;
        dropped_clips = done.dropped_clips;
        frame_map = Some(done.frame_map);
    }
    let tempo_trajectory: std::sync::Arc<[f64]> = tempo_trajectory.into();
    for (number, &(stream, id)) in tracks.iter().enumerate() {
        let scale = share(number + usize::from(mix_pass));
        let stem = playlist_stem_project(project, id);
        let mut plan = compile(&stem, pool);
        plan.tempo_replay = tempo_trajectory.clone();
        let pass = Pass::new(plan, options, &[]);
        let done = pass?.run(
            length,
            &mut |_, block| sink(stream, block),
            &mut |fraction| progress(scale(fraction)),
        )?;
        if !done.completed {
            return Ok(done);
        }
        frames = done.frames;
        if frame_map.is_none() {
            frame_map = Some(done.frame_map);
        }
        // A lane alone never drops more clips than the mix does; report the
        // most any stream of the render left out.
        dropped_clips = dropped_clips.max(done.dropped_clips);
    }
    Ok(Streamed {
        frame_map: frame_map.unwrap_or_default(),
        frames,
        dropped_clips,
        completed: true,
        plugin_error: None,
    })
}
