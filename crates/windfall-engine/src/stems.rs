//! Rendering a project as it comes: a block at a time, and its mixer
//! tracks apart from each other as stems.
//!
//! [`render_streaming`] is [`render`](crate::render) without the buffer: it
//! hands the audio on as it is made, so a long song never has to fit in
//! memory. [`render_stems`] does the same for several streams at once.
//!
//! # What a stem is
//!
//! A stem is one mixer track's part of the song as a stereo stream. The
//! master track is not a stem; its stream is the mix, which can be had
//! alongside. There are two kinds of stem, and they answer two questions.
//!
//! [`StemMode::TrackOutputs`] is what each track's meter shows: the sound
//! that leaves the track, after its effects, its fader and its pan, and
//! before it goes on to its output and its sends. A track that others
//! play into, a bus, is a stem like any other and holds what it was sent,
//! processed. A track's stem therefore knows nothing of what a bus or the
//! master does to it later. If every track plays straight into a master
//! that has no effects and stands at unity, the stems add up to the mix.
//! Put a reverb on a bus and they do not: the dry tracks and the bus are
//! all there, each as it left its own track. All stems come out of one
//! pass over the song.
//!
//! [`StemMode::ToMaster`] is what each track adds to the mix: the song
//! with only what plays straight into that track sounding, a channel's
//! notes or an audio clip, followed all the way to the output through the
//! track's effects, its sends, the buses it goes through and the master's
//! own effects and fader. A vocal's stem then carries the vocal's share of
//! the reverb bus. Where everything on the way is linear (faders, pans,
//! equalisers, reverbs, delays) the stems of all the tracks add up to the
//! mix exactly. A compressor or limiter on a bus or on the master works
//! on what it is given, so on a stem it reacts to that track alone, as it
//! does when the track is soloed, and the stems then do not add up. No
//! single pass can do this, because every stem needs every bus effect to
//! itself: the song is rendered once for each stem, and once more for the
//! mix, whose length the stems take.
//!
//! In both kinds, voices and audio clips take their slots exactly as in
//! the mix, so what the mix leaves out for want of a slot is left out of
//! the stems too, and mute and solo are as the project has them.
//!
//! # Time
//!
//! Every stream starts where the song starts and is exactly as long as
//! the mix, to the frame. The engine delays every path through the mixer
//! to match the slowest, so what leaves a track is behind the notes by a
//! figure of its own, and the master by the latency of the engine. Each
//! stream has what it is behind left off its front, which puts a note on
//! the first tick on the first frame of the mix and of its stem alike. An
//! automatic tail ends all of them where it ends the mix.

use windfall_core::{db_to_gain, samples_per_tick};
pub use windfall_ipc::StemMode;
use windfall_ipc::{PlayMode, TransportPatch};
use windfall_project::{ClipContent, Project, TrackId};

use crate::controller::Controller;
use crate::mixer::Mixer;
use crate::plan::{Plan, compile_render as compile};
use crate::pool::SamplePool;
use crate::processor::Processor;
use crate::render::{RenderOptions, TAIL_HOLD_SECONDS, TAIL_SILENCE_DB};

mod playlist;
pub use playlist::{
    PlaylistStem, PlaylistStemError, PlaylistStemOptions, playlist_stem_project, playlist_stems,
    render_playlist_stems,
};

/// The name of the stream that is the whole mix.
const MIX_NAME: &str = "Mix";

/// Most characters of a track's name that go into the name of its stem.
const MAX_NAME_CHARS: usize = 60;

/// Which stems to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StemOptions {
    pub mode: StemMode,
    /// The mixer tracks to render. `None` is every track that has
    /// something to give: with [`StemMode::TrackOutputs`] every track a
    /// channel or an audio clip plays into, straight or by way of other
    /// tracks, and with [`StemMode::ToMaster`] every track one plays
    /// straight into. The master is never among them.
    pub tracks: Option<Vec<TrackId>>,
    /// Renders the whole mix as well, as the first stream.
    pub include_mix: bool,
    /// Puts each track's place in the mixer in front of its name, as in
    /// "03 Bass", so that the stems sort the way the mixer shows them.
    pub numbered: bool,
}

/// One stream of a stem render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stem {
    /// A name for the stream that is safe as part of a file name and that
    /// no other stream of the render has, whatever the letter case.
    pub name: String,
    /// The mixer track the stem is of. `None` for the mix.
    pub track: Option<TrackId>,
}

/// Why stems cannot be rendered. The messages are written to be shown to
/// the user as they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StemError {
    Timeline(String),
    Plugin(String),
    SamplerPreparation(crate::sampler_processing::SamplerPreparationError),
    /// The master track was asked for as a stem.
    Master,
    /// A track was asked for that the project does not have.
    UnknownTrack(TrackId),
    /// No track was asked for, and the mix was not either.
    NothingChosen,
    /// Every track with something to give was asked for, and there is
    /// none: nothing plays into any mixer track but the master.
    NoTracks,
}

impl std::fmt::Display for StemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeline(error) => f.write_str(error),
            Self::Plugin(error) => f.write_str(error),
            Self::SamplerPreparation(error) => std::fmt::Display::fmt(error, f),
            Self::Master => f.write_str(
                "The master track cannot be a stem. Its sound is the mix, which can be exported with the stems.",
            ),
            Self::UnknownTrack(id) => {
                write!(f, "The project has no mixer track with the id {}.", id.0)
            }
            Self::NothingChosen => f.write_str("Choose at least one mixer track to export."),
            Self::NoTracks => f.write_str(
                "Nothing plays into a mixer track other than the master, so there are no stems to export.",
            ),
        }
    }
}

impl std::error::Error for StemError {}

/// What a streamed render came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Streamed {
    /// Frames of each stream. Every stream of a render is the same length.
    pub frames: u64,
    /// Audio clips that are in none of the streams, as
    /// [`Rendered::dropped_clips`](crate::Rendered::dropped_clips) counts
    /// them.
    pub dropped_clips: u32,
    /// The render ran to its end. It did not if `progress` or the sink
    /// returned `false`: the streams are then cut short and of no use.
    pub completed: bool,
    /// Convenience report; checked streaming/stem APIs return Plugin errors.
    pub plugin_error: Option<String>,
}

/// The streams [`render_stems`] makes for a project, in the order it
/// numbers them: the mix first if it is asked for, then the tracks in
/// mixer order.
pub fn stems(project: &Project, options: &StemOptions) -> Result<Vec<Stem>, StemError> {
    let tracks = &project.mixer.tracks;
    let chosen: Vec<usize> = match &options.tracks {
        Some(ids) => {
            for &id in ids {
                if id == TrackId::MASTER {
                    return Err(StemError::Master);
                }
                if project.mixer.track(id).is_none_or(|track| track.current) {
                    return Err(StemError::UnknownTrack(id));
                }
            }
            let listed = |index: &usize| *index > 0 && ids.contains(&tracks[*index].id);
            (0..tracks.len()).filter(listed).collect()
        }
        None => {
            let gives = sources(project, options.mode == StemMode::TrackOutputs);
            let chosen: Vec<usize> = (1..tracks.len()).filter(|&index| gives[index]).collect();
            if chosen.is_empty() {
                return Err(StemError::NoTracks);
            }
            chosen
        }
    };
    if chosen.is_empty() && !options.include_mix {
        return Err(StemError::NothingChosen);
    }

    let digits = tracks.len().saturating_sub(1).to_string().len().max(2);
    let mut stems: Vec<Stem> = Vec::with_capacity(chosen.len() + 1);
    let mut add = |wanted: String, track: Option<TrackId>| {
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
        stems.push(Stem { name, track });
    };
    if options.include_mix {
        add(MIX_NAME.to_owned(), None);
    }
    for index in chosen {
        let track = &tracks[index];
        let mut name = file_name_part(&track.name);
        if name.is_empty() {
            name = format!("Track {index}");
        }
        if options.numbered {
            name = format!("{index:0digits$} {name}");
        }
        add(name, Some(track.id));
    }
    Ok(stems)
}

/// For each mixer track, by its place in the mixer, whether a channel or
/// an audio clip plays into it. With `through`, tracks that such a track
/// feeds by its output or a send count as well, and the tracks those feed.
fn sources(project: &Project, through: bool) -> Vec<bool> {
    let tracks = &project.mixer.tracks;
    let place = |id: TrackId| tracks.iter().position(|track| track.id == id);
    let mut gives = vec![false; tracks.len()];
    let channels = project.channels.iter().map(|channel| channel.mixer_track);
    let clips = project
        .playlist
        .clips
        .iter()
        .filter_map(|clip| match clip.content {
            ClipContent::Audio {
                mixer_track,
                output,
                ..
            } if output.is_mixer() => Some(mixer_track),
            _ => None,
        });
    let videos = project.settings.stage_media.videos.iter().filter(|video| !video.muted).map(|video| video.mixer_track);
    for index in channels.chain(clips).chain(videos).filter_map(place) {
        gives[index] = true;
    }
    // Routing has no cycles, so this many rounds reach every track.
    for _ in 0..tracks.len() * usize::from(through) {
        for (index, track) in tracks.iter().enumerate() {
            if !gives[index] {
                continue;
            }
            let sends = track.sends.iter().map(|send| send.target);
            let output = track
                .output
                .filter(|_| !track.external_output.is_some_and(|route| route.exclusive));
            let effect_outputs = project
                .mixer
                .effect_outputs
                .iter()
                .filter(|route| track.effects.iter().any(|slot| slot.id == route.effect))
                .map(|route| route.target);
            for target in output
                .into_iter()
                .chain(sends)
                .chain(effect_outputs)
                .filter_map(place)
            {
                gives[target] = true;
            }
        }
    }
    gives
}

/// Makes a name fit to be part of a file name on any system: without the
/// characters a path cannot hold, without spaces or dots at its ends,
/// with runs of spaces closed up, and no longer than
/// [`MAX_NAME_CHARS`]. A name Windows keeps for a device, such as `CON`,
/// gets an underscore in front. The result may be empty.
fn file_name_part(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let joined = words.join(" ");
    let cut: String = joined.chars().take(MAX_NAME_CHARS).collect();
    let mut name = cut
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_owned();

    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let numbered = |prefix: &str| {
        stem.strip_prefix(prefix).is_some_and(|rest| {
            rest.len() == 1 && rest != "0" && rest.as_bytes()[0].is_ascii_digit()
        })
    };
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL");
    if device || numbered("COM") || numbered("LPT") {
        name.insert(0, '_');
    }
    name
}

/// Renders a project the way [`render`](crate::render) does and hands the
/// audio to `sink` a block at a time, as interleaved stereo, instead of
/// returning it. The blocks put end to end are exactly what `render`
/// returns, whatever the block size.
///
/// With `auto_tail`, audio that may turn out to lie past the end is kept
/// back until it is known to belong, so `sink` never gets a frame too
/// many. Returning `false` from `sink` or from `progress` ends the render.
pub fn render_streaming(
    project: &Project,
    pool: &SamplePool,
    options: &RenderOptions,
    sink: &mut dyn FnMut(&[f32]) -> bool,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Streamed {
    render_streaming_checked(project, pool, options, sink, progress).unwrap_or_else(|error| {
        Streamed {
            frames: 0,
            dropped_clips: 0,
            completed: false,
            plugin_error: if let crate::render::RenderError::Plugin(error) = error {
                Some(error)
            } else {
                None
            },
        }
    })
}

/// Fallible sampler preparation with the same strict budget as realtime playback.
pub fn render_streaming_checked(
    project: &Project,
    pool: &SamplePool,
    options: &RenderOptions,
    sink: &mut dyn FnMut(&[f32]) -> bool,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Result<Streamed, crate::render::RenderError> {
    options
        .check_region()
        .and_then(|()| pool.video_audio_error(project).map_or(Ok(()), Err))
        .map_err(crate::render::RenderError::Timeline)?;
    let pool = pool.prepare_samplers(project, &mut || true, &mut |_, _, _| {})?;
    let pass = Pass::new(compile(project, &pool), options, &[])
        .map_err(|error| crate::render::RenderError::Plugin(error.to_string()))?;
    pass.run(None, &mut |_, block| sink(block), progress)
        .map_err(|error| match error {
            StemError::Timeline(error) => crate::render::RenderError::Timeline(error),
            StemError::SamplerPreparation(error) => crate::render::RenderError::Sampler(error),
            StemError::Plugin(error) => crate::render::RenderError::Plugin(error),
            error => crate::render::RenderError::Timeline(error.to_string()),
        })
}

/// Renders the stems of a project and hands each to `sink` a block at a
/// time: the index of the stream in the list [`stems`] gives for the same
/// project and options, and its next frames as interleaved stereo. The
/// streams do not arrive in step with each other, and with
/// [`StemMode::ToMaster`] they arrive one after the other.
///
/// Every stream is the same audio for every block size, starts where the
/// song starts and is as long as the mix, which is as long as
/// [`render`](crate::render) makes it. Returning `false` from `sink` or
/// from `progress` ends the render. `progress` is given the share of all
/// the work that is done.
pub fn render_stems(
    project: &Project,
    pool: &SamplePool,
    options: &RenderOptions,
    stem_options: &StemOptions,
    sink: &mut dyn FnMut(usize, &[f32]) -> bool,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Result<Streamed, StemError> {
    options.check_region().map_err(StemError::Timeline)?;
    if let Some(error) = pool.video_audio_error(project) { return Err(StemError::Timeline(error)); }
    let prepared_pool = pool
        .prepare_plugin_render()
        .prepare_samplers(project, &mut || true, &mut |_, _, _| {})
        .map_err(StemError::SamplerPreparation)?;
    let pool = &prepared_pool;
    let list = stems(project, stem_options)?;
    let mix = list.iter().position(|stem| stem.track.is_none());
    let tracks: Vec<(usize, TrackId)> = list
        .iter()
        .enumerate()
        .filter_map(|(stream, stem)| Some((stream, stem.track?)))
        .collect();

    match stem_options.mode {
        StemMode::TrackOutputs => {
            let ids: Vec<TrackId> = tracks.iter().map(|&(_, id)| id).collect();
            let pass = Pass::new(compile(project, pool), options, &ids)?;
            // The pass numbers its streams from the master's on.
            let mut hand_on = |outlet: usize, block: &[f32]| match outlet.checked_sub(1) {
                Some(tap) => sink(tracks[tap].0, block),
                None => mix.is_none_or(|stream| sink(stream, block)),
            };
            pass.run(None, &mut hand_on, progress)
        }
        StemMode::ToMaster => {
            // The mix comes first even when it is not wanted, where only
            // it can say how long an automatic tail is.
            let mix_pass = mix.is_some() || options.auto_tail;
            let passes = tracks.len() + usize::from(mix_pass);
            let share = |pass: usize| {
                move |fraction: f32| (pass as f32 + fraction.clamp(0.0, 1.0)) / passes as f32
            };
            let mut length = None;
            let mut dropped_clips = 0;
            if mix_pass {
                let scale = share(0);
                let pass = Pass::new(compile(project, pool), options, &[])?;
                let done = pass.run(
                    None,
                    &mut |_, block| mix.is_none_or(|stream| sink(stream, block)),
                    &mut |fraction| progress(scale(fraction)),
                )?;
                if !done.completed {
                    return Ok(done);
                }
                length = Some(done.frames as usize);
                dropped_clips = done.dropped_clips;
            }
            let mut frames = length.unwrap_or(0) as u64;
            for (number, &(stream, id)) in tracks.iter().enumerate() {
                let scale = share(number + usize::from(mix_pass));
                let mut plan = compile(project, pool);
                plan.leave_only(id);
                let pass = Pass::new(plan, options, &[])?;
                // Without an automatic tail every pass is as long as the
                // options make it, and needs no length to be told.
                let done = pass.run(
                    length,
                    &mut |_, block| sink(stream, block),
                    &mut |fraction| progress(scale(fraction)),
                )?;
                if !done.completed {
                    return Ok(done);
                }
                frames = done.frames;
                dropped_clips = done.dropped_clips;
            }
            Ok(Streamed {
                frames,
                dropped_clips,
                completed: true,
                plugin_error: None,
            })
        }
    }
}

impl Plan {
    /// Silences everything that does not play straight into the track
    /// with this id: every channel on another track, and every audio clip
    /// on one. They still play, at no level, so that voices and clip
    /// slots are taken exactly as they are when everything sounds.
    fn leave_only(&mut self, id: TrackId) {
        let track = self.track_ids.get(id.0);
        self.videos.retain(|video| Some(video.track) == track);
        for channel in &mut self.channels {
            if Some(channel.track) != track {
                channel.gain = 0.0;
                channel.audible = false;
            }
        }
        for clip in &mut self.audio_clips {
            if clip.direct_output || Some(clip.track) != track {
                clip.gain = 0.0;
            }
        }
    }
}

/// What a processor copies out of the tracks a stem render listens to
/// during one call to `process`: the signal that leaves each, past its
/// fader. All of its memory is set aside up front, so copying allocates
/// nothing.
pub(crate) struct Taps {
    /// The place in the plan of each track listened to.
    tracks: Vec<usize>,
    /// Room for `capacity` frames of each, interleaved, end to end.
    samples: Vec<f32>,
    capacity: usize,
    /// Frames copied since the call began.
    filled: usize,
}

impl Taps {
    /// Taps on the tracks at these places in the plan, with room for
    /// calls of up to `capacity` frames.
    fn new(tracks: Vec<usize>, capacity: usize) -> Self {
        Self {
            samples: vec![0.0; tracks.len() * capacity * 2],
            tracks,
            capacity,
            filled: 0,
        }
    }

    /// Starts over, at the beginning of a call to `process`.
    pub fn rewind(&mut self) {
        self.filled = 0;
    }

    /// Copies the block the mixer has just mixed. After `mix`, the buffer
    /// of a track holds what left its fader.
    pub fn collect(&mut self, mixer: &mut Mixer, frames: usize) {
        let frames = frames.min(self.capacity - self.filled);
        for (tap, &track) in self.tracks.iter().enumerate() {
            let from = mixer.track_mut(track)[..frames].as_flattened();
            let at = (tap * self.capacity + self.filled) * 2;
            self.samples[at..at + frames * 2].copy_from_slice(from);
        }
        self.filled += frames;
    }

    /// What the last call copied out of the track of tap `tap`.
    fn heard(&self, tap: usize) -> &[f32] {
        &self.samples[tap * self.capacity * 2..][..self.filled * 2]
    }
}

/// One stream of a pass on its way out: the master's output, or what
/// leaves a track.
struct Outlet {
    /// Frames still to leave off the front: what the stream is behind.
    skip: usize,
    /// Audio not handed on yet, because it may lie past the end.
    held: Vec<f32>,
    /// Frames handed on.
    sent: usize,
}

impl Outlet {
    fn new(behind: usize) -> Self {
        Self {
            skip: behind,
            held: Vec::new(),
            sent: 0,
        }
    }

    /// Takes in the next block of the stream.
    fn take(&mut self, block: &[f32]) {
        let skipped = self.skip.min(block.len() / 2);
        self.skip -= skipped;
        // Anything that is not a number is kept out of a file, as the
        // processor keeps it out of the output.
        let fresh = block[skipped * 2..].iter();
        self.held
            .extend(fresh.map(|&sample| if sample.is_finite() { sample } else { 0.0 }));
    }

    /// Hands on what is held of the stream's first `frames` frames.
    /// Returns `false` if `sink` does.
    fn release(&mut self, frames: usize, sink: &mut dyn FnMut(&[f32]) -> bool) -> bool {
        let ready = frames.saturating_sub(self.sent).min(self.held.len() / 2);
        if ready == 0 {
            return true;
        }
        let kept = sink(&self.held[..ready * 2]);
        self.held.drain(..ready * 2);
        self.sent += ready;
        kept
    }

    /// Brings the stream to exactly `frames` frames and lets go of the
    /// rest. A track that feeds nothing can be further behind than the
    /// master, and so be short of its last frames, which are silence.
    fn finish(&mut self, frames: usize, sink: &mut dyn FnMut(&[f32]) -> bool) -> bool {
        if !self.release(frames, sink) {
            return false;
        }
        self.held.clear();
        self.held.resize(frames.saturating_sub(self.sent) * 2, 0.0);
        self.release(frames, sink)
    }
}

/// One run of a processor over the pattern or song, set up as
/// [`render`](crate::render) sets its own up.
struct Pass {
    provider: Option<std::sync::Arc<dyn crate::plugins::PluginFactory>>,
    processor: Processor,
    /// Kept for as long as the processor runs: it holds the other ends of
    /// the processor's queues.
    _controller: Controller,
    /// One per stream: the master's first, then one for each tap.
    outlets: Vec<Outlet>,
    /// Frames by which everything comes out of the processor late.
    latency: usize,
    /// The frame of the processor on which the pattern or song ends.
    body_end: usize,
    /// The frame on which the longest tail the options allow ends.
    most: usize,
    auto_tail: bool,
    /// Frames the output has to stay quiet for an automatic tail to end.
    hold: usize,
    silence: f32,
    block: usize,
}

impl Pass {
    /// Sets a processor up to play `plan` as `options` say, listening to
    /// `tracks` each on its own as well as to the master.
    fn new(plan: Plan, options: &RenderOptions, tracks: &[TrackId]) -> Result<Self, StemError> {
        let provider = plan.plugin_factory.clone();
        let sample_rate = options.sample_rate.max(1);
        let pattern = options
            .pattern
            .filter(|id| plan.pattern_ids.get(id.0).is_some())
            .or(plan.patterns.first().map(|pattern| pattern.id));
        let (passes, ticks) = match options.mode {
            PlayMode::Pattern => {
                let length = pattern
                    .and_then(|id| plan.pattern_ids.get(id.0))
                    .map_or(0, |index| plan.patterns[index].length);
                let ticks = u64::from(length) * u64::from(options.pattern_loops);
                (options.pattern_loops, ticks as f64)
            }
            PlayMode::Song => (1, plan.warp(f64::from(plan.song_end))),
        };
        let frames_per_tick = samples_per_tick(plan.tempo_bpm, f64::from(sample_rate));
        let region = options.region.filter(|_| options.mode == PlayMode::Song);
        let body_frames = region.map_or((ticks * frames_per_tick).ceil() as usize, |range| {
            let (first, last) = crate::timeline::region_frames(&plan, range, sample_rate);
            (last - first) as usize
        });
        let tail_frames = if options.tail_secs.is_finite() {
            (f64::from(options.tail_secs.max(0.0)) * f64::from(sample_rate)).round() as usize
        } else {
            0
        };

        let controller = Controller::new();
        controller
            .set_audio_threads(Some(options.audio_threads))
            .map_err(|error| StemError::Plugin(error.into()))?;
        controller.set_plan(plan);
        let (mut processor, _, retirement) = controller
            .try_attach(
                sample_rate,
                crate::project_preparation::AttachmentMode::Independent,
            )
            .map_err(|error| StemError::Plugin(error.to_string()))?;
        drop(retirement);
        controller.set_transport(TransportPatch {
            mode: Some(options.mode),
            pattern,
            loop_song: Some(false),
            ..Default::default()
        });
        if let Some(range) = region {
            controller
                .set_timeline_region(Some(range))
                .expect("region checked before preparation");
            controller.seek(f64::from(range.start));
        }
        if ticks > 0.0 || region.is_some() {
            controller.play_passes(passes);
        }

        let latency = controller.latency_frames() as usize;
        let block = options.block_frames.max(1);
        let mut outlets = vec![Outlet::new(latency)];
        let places = tracks.iter().filter_map(|&id| processor.track_place(id));
        let (places, behind): (Vec<usize>, Vec<usize>) = places.unzip();
        outlets.extend(behind.into_iter().map(Outlet::new));
        if !places.is_empty() {
            processor.listen(Taps::new(places, block));
        }

        Ok(Self {
            provider,
            processor,
            _controller: controller,
            outlets,
            latency,
            body_end: latency + body_frames,
            most: latency + body_frames + tail_frames,
            auto_tail: options.auto_tail,
            hold: (TAIL_HOLD_SECONDS * f64::from(sample_rate)).round() as usize,
            silence: db_to_gain(TAIL_SILENCE_DB),
            block,
        })
    }

    /// Runs the pass. `sink` is handed each stream's audio as it becomes
    /// certain: the number of the stream, zero for the master, and its
    /// next frames. With `length`, the streams are made exactly that many
    /// frames long, whatever the options say about a tail.
    fn run(
        mut self,
        length: Option<usize>,
        sink: &mut dyn FnMut(usize, &[f32]) -> bool,
        progress: &mut dyn FnMut(f32) -> bool,
    ) -> Result<Streamed, StemError> {
        let check = || {
            self.provider
                .as_ref()
                .and_then(|factory| factory.render_error())
                .map_or(Ok(()), |error| Err(StemError::Plugin(error)))
        };
        check()?;
        let most = length.map_or(self.most, |frames| self.latency + frames);
        let auto_tail = self.auto_tail && length.is_none();
        let (body_end, hold) = (self.body_end, self.hold);
        // Once nothing can sound any more the output only dies away, so
        // quiet for this long is quiet for good.
        let rung_out = |processor: &Processor, done: usize, loud_end: usize| {
            done >= body_end && done >= loud_end + hold && processor.settled()
        };

        let mut buffer = vec![0.0; self.block * 2];
        let mut done = 0;
        // One past the last frame of the master that was not silent.
        let mut loud_end = 0;
        let mut completed = true;
        while done < most && completed {
            let frames = self.block.min(most - done);
            let fresh = &mut buffer[..frames * 2];
            self.processor.process(fresh);
            check()?;
            let silence = self.silence;
            let loud = |frame: &[f32; 2]| frame[0].abs() >= silence || frame[1].abs() >= silence;
            if let Some(last) = fresh.as_chunks::<2>().0.iter().rposition(loud) {
                loud_end = done + last + 1;
            }
            done += frames;

            let (master, taps) = self.outlets.split_at_mut(1);
            master[0].take(fresh);
            if let Some(heard) = self.processor.taps() {
                for (tap, outlet) in taps.iter_mut().enumerate() {
                    outlet.take(heard.heard(tap));
                }
            }
            // With an automatic tail, what lies past the last loud frame
            // may yet be cut off.
            let certain = if auto_tail {
                loud_end.max(body_end).min(done)
            } else {
                done
            };
            let certain = certain.saturating_sub(self.latency);
            for (stream, outlet) in self.outlets.iter_mut().enumerate() {
                completed = completed && outlet.release(certain, &mut |block| sink(stream, block));
            }
            completed = completed && progress(done as f32 / most as f32);
            check()?;
            if auto_tail && rung_out(&self.processor, done, loud_end) {
                break;
            }
        }

        // The same length whatever the block size: it only depends on
        // where the sound ends. A tail that was still sounding when it ran
        // out of time is kept whole.
        let end = if auto_tail && rung_out(&self.processor, done, loud_end) {
            loud_end.max(body_end)
        } else {
            done
        };
        let frames = end.saturating_sub(self.latency);
        for (stream, outlet) in self.outlets.iter_mut().enumerate() {
            completed = completed && outlet.finish(frames, &mut |block| sink(stream, block));
        }
        check()?;
        Ok(Streamed {
            frames: frames as u64,
            dropped_clips: self.processor.clips_left_out(),
            completed,
            plugin_error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use windfall_project::{Channel, ChannelId, ChannelSource, MixerTrack, SamplerSettings, Send};

    use super::*;

    fn track(id: u32, name: &str, output: Option<u32>) -> MixerTrack {
        MixerTrack {
            dock: windfall_project::MixerDock::default(),
            external_output: None,
            processing: windfall_dsp::TrackParams::default(),
            current: false,
            latency_offset_ms: 0.0,
            id: TrackId(id),
            name: name.to_owned(),
            color: 0,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            output: output.map(TrackId),
            sidechains: Vec::new(),
            sends: Vec::new(),
            effects: Vec::new(),
            recording: None,
        }
    }

    fn channel(id: u32, track: u32) -> Channel {
        Channel {
            sample_playback: None,
            id: ChannelId(id),
            name: String::new(),
            color: 0,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            group: String::new(),
            voice: Default::default(),
            expression_map: Default::default(),
            sample_instrument: None,
            timing: windfall_project::ChannelTiming::default(),
            mixer_track: TrackId(track),
            source: ChannelSource::Sampler(SamplerSettings::default()),
        }
    }

    fn options(mode: StemMode) -> StemOptions {
        StemOptions {
            mode,
            tracks: None,
            include_mix: false,
            numbered: false,
        }
    }

    fn names(stems: &[Stem]) -> Vec<&str> {
        stems.iter().map(|stem| stem.name.as_str()).collect()
    }

    /// Drums and Bass play into a Bus that plays into the master, Bass
    /// sends to Echo, and Spare has nothing to do with any of them.
    fn band() -> Project {
        let mut project = Project::new("band");
        let tracks = &mut project.mixer.tracks;
        tracks.push(track(1, "Drums", Some(3)));
        tracks.push(track(2, "Bass", Some(3)));
        tracks.push(track(3, "Bus", Some(0)));
        tracks.push(track(4, "Echo", Some(0)));
        tracks.push(track(5, "Spare", Some(0)));
        tracks[2].sends.push(Send {
            target: TrackId(4),
            gain: 0.5,
        });
        project.channels.push(channel(10, 1));
        project.channels.push(channel(11, 2));
        project
    }

    #[test]
    fn names_are_made_safe_for_any_file_system() {
        for (name, safe) in [
            ("Kick", "Kick"),
            ("  Lead   Vox  ", "Lead Vox"),
            ("Kick/Snare", "Kick Snare"),
            ("a<b>c:d\"e\\f|g?h*i", "a b c d e f g h i"),
            ("tab\there\nand\0there", "tab here and there"),
            ("...Hidden...", "Hidden"),
            ("Vox.Dry", "Vox.Dry"),
            ("Übergröße 低音", "Übergröße 低音"),
            ("con", "_con"),
            ("NUL", "_NUL"),
            ("Com1", "_Com1"),
            ("LPT9.old", "_LPT9.old"),
            ("COM0", "COM0"),
            ("COM12", "COM12"),
            ("Console", "Console"),
            ("///", ""),
            ("", ""),
        ] {
            assert_eq!(file_name_part(name), safe, "{name:?}");
        }
        let long = file_name_part(&"long ".repeat(40));
        assert_eq!(long.chars().count(), MAX_NAME_CHARS - 1);
        assert!(long.ends_with("long"));
    }

    #[test]
    fn stems_are_listed_in_mixer_order_whatever_order_they_are_asked_for_in() {
        let project = band();
        let asked = StemOptions {
            tracks: Some(vec![TrackId(4), TrackId(1), TrackId(4)]),
            include_mix: true,
            ..options(StemMode::TrackOutputs)
        };
        let stems = stems(&project, &asked).unwrap();
        assert_eq!(names(&stems), ["Mix", "Drums", "Echo"]);
        let tracks: Vec<_> = stems.iter().map(|stem| stem.track).collect();
        assert_eq!(tracks, [None, Some(TrackId(1)), Some(TrackId(4))]);
    }

    #[test]
    fn asking_for_every_track_leaves_out_the_ones_with_nothing_to_give() {
        let project = band();
        // The bus and the echo get their sound from other tracks, which
        // makes them stems of the first kind and not of the second.
        let outputs = stems(&project, &options(StemMode::TrackOutputs)).unwrap();
        assert_eq!(names(&outputs), ["Drums", "Bass", "Bus", "Echo"]);
        let to_master = stems(&project, &options(StemMode::ToMaster)).unwrap();
        assert_eq!(names(&to_master), ["Drums", "Bass"]);

        // A track that is asked for by name is a stem, silent or not.
        let spare = StemOptions {
            tracks: Some(vec![TrackId(5)]),
            ..options(StemMode::ToMaster)
        };
        assert_eq!(names(&stems(&project, &spare).unwrap()), ["Spare"]);
    }

    #[test]
    fn stems_are_numbered_by_their_place_in_the_mixer() {
        let mut project = band();
        let numbered = StemOptions {
            numbered: true,
            include_mix: true,
            ..options(StemMode::TrackOutputs)
        };
        let stems = |project: &Project| super::stems(project, &numbered).unwrap();
        assert_eq!(
            names(&stems(&project)),
            ["Mix", "01 Drums", "02 Bass", "03 Bus", "04 Echo"]
        );
        // A mixer of a hundred tracks and more gets a third digit.
        for id in 6..=100 {
            project.mixer.tracks.push(track(id, "More", Some(0)));
        }
        project.channels.push(channel(12, 100));
        let stems = stems(&project);
        assert_eq!(stems[1].name, "001 Drums");
        assert_eq!(stems.last().unwrap().name, "100 More");
    }

    #[test]
    fn no_two_stems_share_a_name() {
        let mut project = Project::new("twins");
        for (id, name) in [
            (1, "Drums"),
            (2, "drums"),
            (3, "Drums 2"),
            (4, "Drums"),
            (5, ""),
            (6, "Track 5"),
            (7, "mix"),
            (8, "Drums?"),
        ] {
            project.mixer.tracks.push(track(id, name, Some(0)));
            project.channels.push(channel(id + 20, id));
        }
        let asked = StemOptions {
            include_mix: true,
            ..options(StemMode::TrackOutputs)
        };
        let stems = stems(&project, &asked).unwrap();
        assert_eq!(
            names(&stems),
            [
                "Mix",
                "Drums",
                "drums 2",
                "Drums 2 2",
                "Drums 3",
                "Track 5",
                "Track 5 2",
                "mix 2",
                "Drums 4",
            ]
        );
        let mut lowered: Vec<String> = stems.iter().map(|stem| stem.name.to_lowercase()).collect();
        lowered.sort();
        lowered.dedup();
        assert_eq!(lowered.len(), stems.len());
    }

    #[test]
    fn what_cannot_be_a_stem_is_refused_in_plain_words() {
        let project = band();
        let asked = |tracks: Option<Vec<u32>>, include_mix: bool| {
            let options = StemOptions {
                tracks: tracks.map(|ids| ids.into_iter().map(TrackId).collect()),
                include_mix,
                ..options(StemMode::TrackOutputs)
            };
            stems(&project, &options).map(|stems| stems.len())
        };
        assert_eq!(asked(Some(vec![1, 0]), false), Err(StemError::Master));
        assert_eq!(
            asked(Some(vec![1, 77]), false),
            Err(StemError::UnknownTrack(TrackId(77)))
        );
        assert_eq!(asked(Some(vec![]), false), Err(StemError::NothingChosen));
        // The mix alone is something to export.
        assert_eq!(asked(Some(vec![]), true), Ok(1));
        let empty = Project::new("empty");
        assert_eq!(
            stems(&empty, &options(StemMode::TrackOutputs)),
            Err(StemError::NoTracks)
        );
        assert_eq!(
            StemError::UnknownTrack(TrackId(77)).to_string(),
            "The project has no mixer track with the id 77."
        );
    }

    #[test]
    fn listening_to_tracks_allocates_nothing_while_the_processor_runs() {
        let plan = compile(&band(), &SamplePool::new());
        let options = RenderOptions::default();
        let listened = [TrackId(1), TrackId(3), TrackId(4)];
        let mut pass = Pass::new(plan, &options, &listened).unwrap();
        assert_eq!(pass.outlets.len(), 4);
        let mut block = vec![0.0; options.block_frames * 2];
        // The first call takes in the requests that start playback.
        pass.processor.process(&mut block);
        let calls = crate::test_alloc::allocator_calls(|| {
            for _ in 0..40 {
                pass.processor.process(&mut block);
            }
        });
        assert_eq!(calls, 0);
        let taps = pass.processor.taps().unwrap();
        for tap in 0..listened.len() {
            assert_eq!(taps.heard(tap).len(), block.len());
        }
    }

    #[test]
    fn an_outlet_leaves_off_its_front_and_hands_on_what_is_certain() {
        let mut outlet = Outlet::new(3);
        let mut got: Vec<f32> = Vec::new();
        let mut sink = |block: &[f32]| {
            got.extend_from_slice(block);
            true
        };
        // Frames 0 to 4, the left side counting up and the right not a
        // number, which never gets through.
        let frames: Vec<f32> = (0..5).flat_map(|frame| [frame as f32, f32::NAN]).collect();
        outlet.take(&frames[..4]);
        outlet.take(&frames[4..]);
        assert!(outlet.release(1, &mut sink));
        assert!(outlet.release(1, &mut sink));
        assert!(outlet.finish(4, &mut sink));
        // Frames 3 and 4, and two of silence to make up the length.
        assert_eq!(got, [3.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }
}
