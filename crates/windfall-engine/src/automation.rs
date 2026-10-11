//! Automation: curves on the playlist that move a fader, a knob or the
//! tempo while the song plays.
//!
//! # Lanes
//!
//! The control side compiles every automation clip that sounds into lanes,
//! one per target. A lane is the whole story of its target along the song:
//! a row of spans that follow one another, each either a stretch of some
//! clip's curve or the value the last clip left behind. Which clip wins
//! where several overlap is settled there, so the audio thread only looks
//! a tick up.
//!
//! The value of a target is therefore a function of the place in the song
//! and nothing else. Before its first clip a target has no value from
//! automation and keeps the one stored in the project; from there on it
//! follows the clips and holds between them. Playback that begins in the
//! middle of a song finds every target where it would be had the song
//! played from the top.
//!
//! # Following a lane
//!
//! The audio thread looks at the lanes on a fixed grid of [`GRID`] frames,
//! counted from the engine's first frame and not from wherever a buffer
//! happens to begin, which keeps a render the same for every block size.
//! At each look it works out what every target is to be one grid step on
//! and sets it on its way there:
//!
//! - A gain or a pan is a ramp. It is aimed at the next value to arrive
//!   exactly one grid step later, so its course is the curve read every
//!   [`GRID`] frames and joined by straight lines, with no steps to hear.
//! - A setting of an effect or instrument is handed to the processor,
//!   which glides to it the way it does when a knob is turned.
//!
//! A corner of the curve, a jump in it or the edge of a clip therefore
//! takes effect within one grid step of its tick: at most 64 frames, which
//! is 1.33 ms at 48 kHz. A jump is spread over that step.
//!
//! Where playback begins, or is moved to, a gain or pan does not glide in
//! from its stored value: it is simply at the curve's value from the first
//! frame, so a song that opens on a fade in from silence opens silent. A
//! setting of an effect or instrument takes the few milliseconds its
//! processor glides in.
//!
//! # Letting go
//!
//! A target returns to its stored value when the song arrives, playing,
//! at a place before the target's first clip: over the 5 ms a fader move
//! takes. A song that stops does not let go at once. Its targets stay
//! where the song left them while anything of it is still to be heard, a
//! reverb's tail for one, and return over 100 ms once that has rung out or
//! the transport is used again. The processor decides when, and tells
//! [`step`] by looking at no place in the song at all. A gain or pan makes
//! its way back as one ramp. A setting of an effect or instrument is
//! walked back by the looks that follow, each of which hands the processor
//! the next value on the way.
//!
//! Nothing a lane does is written to the plan. The stored values stay
//! what they are.

use std::collections::HashMap;

use windfall_project::{
    AutomationId, AutomationPoint, AutomationRange, AutomationTarget, ClipContent, ClipId,
    EffectId, Playlist, Project, curve_value,
};

use crate::plan::{IdIndex, Plan, silent_playlist_tracks};
use crate::ramp::Ramp;
use crate::state::PlanState;

/// Frames between two looks at the automation.
pub(crate) const GRID: u64 = 64;

/// Everything automation does to one target along the song.
#[derive(Debug)]
pub(crate) struct Lane {
    /// What the lane moves, as the project names it. It tells a lane of one
    /// plan from the lanes of the next.
    pub key: AutomationTarget,
    target: Target,
    range: AutomationRange,
    /// What the target does from the start of its first clip on. Each span
    /// lasts until the next begins, and the last one for good.
    spans: Vec<Span>,
    /// The curves the spans read.
    curves: Vec<Vec<AutomationPoint>>,
    /// What the target does while a pattern plays by itself: the event
    /// lane each pattern has for it, sorted by the pattern's index in
    /// [`Plan::patterns`]. A pattern that is not listed leaves the target
    /// alone.
    patterns: Vec<PatternCurve>,
}

/// The event lane one pattern has for a target.
#[derive(Debug)]
struct PatternCurve {
    /// Index into [`Plan::patterns`].
    pattern: usize,
    /// Index into the lane's curves.
    curve: usize,
    /// The lane's id, which the engine reports its value under.
    automation: AutomationId,
}

/// A place automation is read at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Place {
    /// A tick of the song.
    Song(f64),
    /// A tick of the pattern at this index of [`Plan::patterns`], while it
    /// plays by itself.
    Pattern { index: usize, tick: f64 },
}

#[derive(Debug)]
struct Span {
    /// The song tick the span begins on.
    start: f64,
    /// The automation whose clip made the span.
    automation: AutomationId,
    source: Source,
}

#[derive(Debug)]
enum Source {
    /// A clip is playing: the curve at this index, read `shift` ticks
    /// ahead of the song.
    Curve { curve: usize, shift: f64 },
    /// No clip is playing: the value the last one ended on.
    Hold(f32),
    /// A pattern clip is playing: the curve of the pattern's event lane,
    /// read at the place in the pattern, which goes around every `length`
    /// ticks from the song tick `origin`.
    Loop {
        curve: usize,
        origin: f64,
        length: f64,
    },
}

/// The place in a pattern `length` ticks long that began at `origin`.
fn looped(tick: f64, origin: f64, length: f64) -> f64 {
    (tick - origin).rem_euclid(length)
}

/// What a lane moves, as the plan names it.
#[derive(Debug, Clone, Copy)]
enum Target {
    /// The channel at this index of [`Plan::channels`].
    ChannelGain(usize),
    ChannelPan(usize),
    /// The track at this index of [`Plan::tracks`].
    TrackGain(usize),
    TrackPan(usize),
    TrackParam {
        track: usize,
        param: usize,
    },
    /// A send: the edge at index `edge` of track `track`.
    Send {
        track: usize,
        edge: usize,
    },
    /// Effects are found by id, because an effect that is fading out can
    /// take a place in a chain after the lanes are made.
    EffectParam {
        effect: EffectId,
        param: usize,
    },
    EffectMix(EffectId),
    InstrumentParam {
        channel: usize,
        param: usize,
    },
    /// The tempo is not set on anything: it is built into the plan's tempo
    /// map, which the sequencer keeps time by.
    Tempo,
}

impl Target {
    /// True for a gain or a pan, which the engine ramps itself. The others
    /// are settings handed to a processor.
    fn ramps(self) -> bool {
        match self {
            Target::ChannelGain(_)
            | Target::ChannelPan(_)
            | Target::TrackGain(_)
            | Target::TrackPan(_)
            | Target::Send { .. } => true,
            Target::EffectParam { .. }
            | Target::EffectMix(_)
            | Target::InstrumentParam { .. }
            | Target::TrackParam { .. }
            | Target::Tempo => false,
        }
    }
}

/// What a lane is doing, for [`PlanState`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LaneState {
    /// The lane has its target in hand.
    pub engaged: bool,
    /// The value it last gave it, 0 to 1, and the automation that came
    /// from.
    pub value: f32,
    pub automation: u32,
    /// The way back to its stored value that the lane's target is on, once
    /// the lane has let go of it.
    back: Option<Back>,
}

/// A target's way back to its stored value: the frame it set out on and
/// the frames it takes.
#[derive(Debug, Clone, Copy)]
struct Back {
    began: u64,
    frames: u64,
}

/// One automation clip, as the flattening of a lane sees it.
struct Piece {
    start: u32,
    end: u32,
    offset: u32,
    /// Position of the clip's playlist track, from the top.
    row: usize,
    id: ClipId,
    automation: AutomationId,
    curve: usize,
    /// For a pattern clip, the length of the pattern its curve goes around
    /// in. `None` for an automation clip, whose curve is read once.
    repeat: Option<u32>,
}

impl Piece {
    /// Whether this clip has its way over `other` where both play: the one
    /// on the playlist track nearer the top, then the one that starts
    /// later, then the one with the lower id.
    fn beats(&self, other: &Piece) -> bool {
        let rank = |piece: &Piece| (piece.row, u32::MAX - piece.start, piece.id);
        rank(self) < rank(other)
    }
}

impl Lane {
    /// The value, 0 to 1, that automation gives the target at `tick` of
    /// the song and the automation it comes from, or `None` before the
    /// target's first clip.
    pub fn value_at(&self, tick: f64) -> Option<(f32, AutomationId)> {
        let after = self.spans.partition_point(|span| span.start <= tick);
        let span = &self.spans[after.checked_sub(1)?];
        let value = match span.source {
            Source::Curve { curve, shift } => curve_value(&self.curves[curve], tick + shift),
            Source::Hold(value) => value,
            Source::Loop {
                curve,
                origin,
                length,
            } => curve_value(&self.curves[curve], looped(tick, origin, length)),
        };
        Some((value, span.automation))
    }

    /// The value, 0 to 1, the target has at `place` and the automation or
    /// event lane it comes from. `None` where nothing moves the target: in
    /// the song before its first clip, and in a pattern that has no event
    /// lane for it.
    pub fn value_in(&self, place: Place) -> Option<(f32, AutomationId)> {
        match place {
            Place::Song(tick) => self.value_at(tick),
            Place::Pattern { index, tick } => {
                let at = self
                    .patterns
                    .binary_search_by_key(&index, |entry| entry.pattern)
                    .ok()?;
                let entry = &self.patterns[at];
                Some((
                    curve_value(&self.curves[entry.curve], tick),
                    entry.automation,
                ))
            }
        }
    }

    /// [`value_in`](Self::value_in) in the target's own unit.
    pub fn real_in(&self, place: Place) -> Option<f32> {
        self.value_in(place)
            .map(|(value, _)| self.range.value(value))
    }

    /// True when some pattern has an event lane for the target.
    pub fn in_patterns(&self) -> bool {
        !self.patterns.is_empty()
    }

    /// The same in the target's own unit.
    pub fn real_at(&self, tick: f64) -> Option<f32> {
        self.value_at(tick)
            .map(|(value, _)| self.range.value(value))
    }

    /// What the target is coming from as the song arrives at `tick`, in
    /// its own unit: the value it has just before. It differs from
    /// [`real_at`](Self::real_at) where the lane jumps.
    pub fn real_before(&self, tick: f64) -> Option<f32> {
        let after = self.spans.partition_point(|span| span.start < tick);
        let span = &self.spans[after.checked_sub(1)?];
        let value = match span.source {
            Source::Curve { curve, shift } => curve_before(&self.curves[curve], tick + shift),
            Source::Hold(value) => value,
            Source::Loop {
                curve,
                origin,
                length,
            } => {
                // Arriving at the end of a round is arriving at the end of
                // the pattern, not at its start.
                let place = looped(tick, origin, length);
                let place = if place == 0.0 && tick > span.start {
                    length
                } else {
                    place
                };
                curve_before(&self.curves[curve], place)
            }
        };
        Some(self.range.value(value))
    }

    /// True for the lane of the tempo.
    pub fn is_tempo(&self) -> bool {
        matches!(self.target, Target::Tempo)
    }

    /// The ticks at which the lane's course has a corner or a jump, from
    /// tick 0 up to `end`: where a span begins and where a curve has a
    /// point, with the stretches between two points that are bent cut into
    /// pieces of `grain` ticks. Between two neighbours the value moves in a
    /// straight line, or near enough.
    pub fn corners(&self, end: f64, grain: f64) -> Vec<f64> {
        let mut corners = vec![0.0];
        for (index, span) in self.spans.iter().enumerate() {
            let until = self.spans.get(index + 1).map_or(end, |next| next.start);
            let until = until.min(end);
            if span.start >= until {
                continue;
            }
            corners.push(span.start);
            let Source::Curve { curve, shift } = span.source else {
                continue;
            };
            let points = &self.curves[curve];
            for (at, point) in points.iter().enumerate() {
                let tick = f64::from(point.tick) - shift;
                if tick > span.start && tick < until {
                    corners.push(tick);
                }
                let Some(next) = points.get(at + 1) else {
                    continue;
                };
                if point.hold || point.curve == 0.0 {
                    continue;
                }
                let stop = (f64::from(next.tick) - shift).min(until);
                let mut inside = tick.max(span.start) + grain;
                while inside < stop {
                    corners.push(inside);
                    inside += grain;
                }
            }
        }
        corners.push(end);
        corners.sort_by(f64::total_cmp);
        corners.dedup();
        corners
    }
}

/// The value a curve has just before `tick`: what [`curve_value`] gives
/// everywhere but on a tick where the curve jumps, where this is the value
/// it jumps from.
fn curve_before(points: &[AutomationPoint], tick: f64) -> f32 {
    let before = points.partition_point(|point| f64::from(point.tick) < tick);
    let Some(from) = before.checked_sub(1).map(|index| &points[index]) else {
        return points.first().map_or(0.0, |point| point.value);
    };
    let Some(to) = points.get(before) else {
        return from.value;
    };
    if from.hold {
        return from.value;
    }
    let span = f64::from(to.tick) - f64::from(from.tick);
    let part = (tick - f64::from(from.tick)) / span;
    let shaped = windfall_project::curve_shape(part, from.curve) as f32;
    from.value + (to.value - from.value) * shaped
}

/// Builds the lanes of a project for a plan made of it. Clips that are
/// muted, or on a muted playlist track, count for nothing, and an
/// automation whose target the plan does not have is left out.
pub(crate) fn compile(project: &Project, playlist: &Playlist, plan: &Plan) -> Vec<Lane> {
    let silent_tracks = silent_playlist_tracks(playlist);
    let row_of = |clip_track| {
        let rows = playlist.tracks.iter();
        rows.clone().position(|track| track.id == clip_track)
    };
    let automations = IdIndex::new(project.automations.iter().map(|a| a.id.0));

    // The clips of each target, in the order the targets first turn up
    // among the automations, which keeps the lanes in a fixed order.
    let mut lanes: Building = Vec::new();
    let mut by_target: HashMap<AutomationTarget, usize> = HashMap::new();
    for clip in &playlist.clips {
        let ClipContent::Automation { automation: id } = clip.content else {
            continue;
        };
        let end = clip.start.saturating_add(clip.length);
        let Some(row) = row_of(clip.track) else {
            continue;
        };
        if clip.muted || silent_tracks.contains(&clip.track) || end == clip.start {
            continue;
        }
        let Some(automation) = automations.get(id.0).map(|at| &project.automations[at]) else {
            continue;
        };
        if automation.points.is_empty() {
            continue;
        }
        let Some(at) = lane_of(
            &automation.target,
            project,
            plan,
            &mut lanes,
            &mut by_target,
        ) else {
            continue;
        };
        let (lane, pieces, curves) = &mut lanes[at];
        let curve = *curves.entry(id).or_insert_with(|| {
            lane.curves.push(automation.points.clone());
            lane.curves.len() - 1
        });
        pieces.push(Piece {
            start: clip.start,
            end,
            offset: clip.offset,
            row,
            id: clip.id,
            automation: id,
            curve,
            repeat: None,
        });
    }

    // The event lanes of the patterns. Each has its curve once per lane,
    // whatever number of clips play the pattern.
    let mut seen: Vec<AutomationTarget> = Vec::new();
    for (index, pattern) in project.patterns.iter().enumerate() {
        debug_assert_eq!(plan.patterns[index].id, pattern.id);
        for events in &pattern.events {
            if events.points.is_empty() || events.target == AutomationTarget::Tempo {
                continue;
            }
            let Some(at) = lane_of(&events.target, project, plan, &mut lanes, &mut by_target)
            else {
                continue;
            };
            if !seen.contains(&events.target) {
                seen.push(events.target);
            }
            let (lane, pieces, curves) = &mut lanes[at];
            let curve = *curves.entry(events.id).or_insert_with(|| {
                lane.curves.push(events.points.clone());
                lane.curves.len() - 1
            });
            // By itself the pattern plays its lane from its start.
            lane.patterns.push(PatternCurve {
                pattern: index,
                curve,
                automation: events.id,
            });
            // On the playlist every clip of the pattern plays it, around
            // and around, as it plays the notes.
            let length = plan.patterns[index].length;
            for clip in &playlist.clips {
                let ClipContent::Pattern { pattern: id } = clip.content else {
                    continue;
                };
                let end = clip.start.saturating_add(clip.length);
                if id != pattern.id
                    || clip.muted
                    || silent_tracks.contains(&clip.track)
                    || end == clip.start
                {
                    continue;
                }
                let Some(row) = row_of(clip.track) else {
                    continue;
                };
                pieces.push(Piece {
                    start: clip.start,
                    end,
                    offset: clip.offset % length,
                    row,
                    id: clip.id,
                    automation: events.id,
                    curve,
                    repeat: Some(length),
                });
            }
        }
    }

    // The order the automations have in the project, so that the lanes do
    // not depend on where their clips happen to sit. Targets only patterns
    // move come after, in the order the patterns first name them.
    let order = |lane: &Lane| {
        let automations = project.automations.iter();
        let among = automations.clone().position(|a| a.target == lane.key);
        let named = seen.iter().position(|target| *target == lane.key);
        (among.unwrap_or(usize::MAX), named.unwrap_or(usize::MAX))
    };
    let mut lanes: Vec<Lane> = lanes
        .into_iter()
        .map(|(mut lane, pieces, _)| {
            lane.spans = flatten(&pieces, &lane.curves);
            lane
        })
        .collect();
    lanes.sort_by_key(order);
    lanes
}

/// The lanes being built, each with the clips of its target and where the
/// curve of each automation or event lane sits among its curves.
type Building = Vec<(Lane, Vec<Piece>, HashMap<AutomationId, usize>)>;

/// Finds the lane of `target` among those being built, starting one when
/// the target has none yet. `None` when the plan has no such target.
fn lane_of(
    target: &AutomationTarget,
    project: &Project,
    plan: &Plan,
    lanes: &mut Building,
    by_target: &mut HashMap<AutomationTarget, usize>,
) -> Option<usize> {
    if let Some(at) = by_target.get(target) {
        return Some(*at);
    }
    let (resolved, range) = resolve(target, plan).zip(project.automation_range(target))?;
    let lane = Lane {
        key: *target,
        target: resolved,
        range,
        spans: Vec::new(),
        curves: Vec::new(),
        patterns: Vec::new(),
    };
    lanes.push((lane, Vec::new(), HashMap::new()));
    by_target.insert(*target, lanes.len() - 1);
    Some(lanes.len() - 1)
}

/// Settles what a target does along the song: at every tick the clip that
/// wins among those playing or, when none is, the value left by the clip
/// that ended last.
fn flatten(pieces: &[Piece], curves: &[Vec<AutomationPoint>]) -> Vec<Span> {
    let mut edges: Vec<u32> = pieces
        .iter()
        .flat_map(|piece| [piece.start, piece.end])
        .collect();
    edges.sort_unstable();
    edges.dedup();

    let mut spans: Vec<Span> = Vec::new();
    // The clip the last span was made of, so that one clip that wins over
    // several edges in a row stays one span.
    let mut playing: Option<ClipId> = None;
    for edge in edges {
        let covering = pieces
            .iter()
            .filter(|piece| piece.start <= edge && edge < piece.end);
        let winner = covering.reduce(|best, piece| if piece.beats(best) { piece } else { best });
        let span = match winner {
            Some(piece) if playing == Some(piece.id) => continue,
            Some(piece) => {
                playing = Some(piece.id);
                let source = match piece.repeat {
                    None => Source::Curve {
                        curve: piece.curve,
                        shift: f64::from(piece.offset) - f64::from(piece.start),
                    },
                    Some(length) => Source::Loop {
                        curve: piece.curve,
                        origin: f64::from(piece.start) - f64::from(piece.offset),
                        length: f64::from(length),
                    },
                };
                Span {
                    start: f64::from(edge),
                    automation: piece.automation,
                    source,
                }
            }
            None => {
                playing = None;
                // Of the clips that have ended, the one that ended last.
                let ended = pieces.iter().filter(|piece| piece.end <= edge);
                let last = ended.reduce(|best, piece| {
                    let later = piece.end > best.end;
                    let wins = piece.end == best.end && piece.beats(best);
                    if later || wins { piece } else { best }
                });
                let Some(last) = last else {
                    continue;
                };
                // The value its curve has where the clip ends. A pattern
                // clip that ends with a round of its pattern ends at the
                // pattern's end, not back at its start.
                let end = f64::from(last.offset) + f64::from(last.end - last.start);
                let end = match last.repeat.map(f64::from) {
                    Some(length) if end.rem_euclid(length) == 0.0 => length,
                    Some(length) => end.rem_euclid(length),
                    None => end,
                };
                Span {
                    start: f64::from(edge),
                    automation: last.automation,
                    source: Source::Hold(curve_value(&curves[last.curve], end)),
                }
            }
        };
        spans.push(span);
    }
    spans
}

/// Finds what an automation moves in a plan.
fn resolve(target: &AutomationTarget, plan: &Plan) -> Option<Target> {
    let channel = |id: windfall_project::ChannelId| plan.channel(id);
    let track = |id: windfall_project::TrackId| plan.track_ids.get(id.0);
    Some(match *target {
        AutomationTarget::ChannelVolume { channel: id } => Target::ChannelGain(channel(id)?),
        AutomationTarget::ChannelPan { channel: id } => Target::ChannelPan(channel(id)?),
        AutomationTarget::TrackVolume { track: id } => Target::TrackGain(track(id)?),
        AutomationTarget::TrackPan { track: id } => Target::TrackPan(track(id)?),
        AutomationTarget::TrackParam { track: id, param } => Target::TrackParam {
            track: track(id)?,
            param: param as usize,
        },
        AutomationTarget::SidechainGain {
            track: from,
            target: to,
        } => {
            let from = track(from)?;
            let edges = plan.tracks[from].edges.iter();
            let edge = edges
                .clone()
                .position(|edge| edge.send && edge.sidechain && edge.target_id == to)?;
            Target::Send { track: from, edge }
        }
        AutomationTarget::SendGain {
            track: from,
            target: to,
        } => {
            let from = track(from)?;
            let edges = plan.tracks[from].edges.iter();
            let edge = edges
                .clone()
                .position(|edge| edge.send && !edge.sidechain && edge.target_id == to)?;
            Target::Send { track: from, edge }
        }
        AutomationTarget::EffectParam { effect, param, .. } => {
            plan.effect_ids.get(effect.0)?;
            Target::EffectParam {
                effect,
                param: param as usize,
            }
        }
        AutomationTarget::EffectMix { effect, .. } => {
            plan.effect_ids.get(effect.0)?;
            Target::EffectMix(effect)
        }
        AutomationTarget::InstrumentParam { channel: id, param } => {
            let channel = channel(id)?;
            plan.channels[channel].instrument?;
            Target::InstrumentParam {
                channel,
                param: param as usize,
            }
        }
        AutomationTarget::Tempo => Target::Tempo,
    })
}

/// One look at the lanes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Look {
    /// The frame the look is taken on.
    pub now: u64,
    /// The frame the targets are to arrive on: the next step of the grid.
    pub until: u64,
    /// The place in the song the targets are to be at on frame `until`:
    /// where the song will be playing then, or where a song that has
    /// stopped is being held. `None` lets go of every target, which sends
    /// each back to its stored value.
    pub tick: Option<Place>,
    /// The place in the song that frame `now` plays, when playback has
    /// only just begun there or been moved there.
    pub landed: Option<Place>,
    /// Frames a target that is let go of at this look takes to get back to
    /// its stored value.
    pub back: u64,
}

/// Looks at every lane and sets its target on its way to the value it is
/// to have at the next step of the grid.
///
/// Runs on the audio thread, so it must not allocate.
pub(crate) fn step(plan: &Plan, state: &mut PlanState, look: Look) {
    let Look {
        now,
        until,
        tick,
        landed,
        ..
    } = look;
    let (mut engaged, mut returning) = (0, 0);
    for (index, lane) in plan.lanes.iter().enumerate() {
        let Some((value, automation)) = tick.and_then(|place| lane.value_in(place)) else {
            if let_go(plan, state, index, look) {
                returning += 1;
            }
            continue;
        };
        // Playback landed inside the lane: the target is where the curve
        // has it at once, and moves on from there.
        if let Some(here) = landed.and_then(|place| lane.real_in(place)) {
            set(plan, state, lane.target, here, now, 0);
        }
        let real = lane.range.value(value);
        set(
            plan,
            state,
            lane.target,
            real,
            now,
            until.saturating_sub(now),
        );
        state.lanes[index] = LaneState {
            engaged: true,
            value,
            automation: automation.0,
            back: None,
        };
        engaged += 1;
    }
    state.engaged = engaged;
    state.returning = returning;
}

/// Moves the target of a lane that has no value for it on toward its
/// stored value: sets it on its way if the lane had it in hand until now,
/// and takes a setting that is on its way one step further. Returns true
/// while later looks have more of the way to walk.
fn let_go(plan: &Plan, state: &mut PlanState, index: usize, look: Look) -> bool {
    let lane = &plan.lanes[index];
    let mut held = state.lanes[index];
    if held.engaged {
        held.engaged = false;
        held.back = Some(Back {
            began: look.now,
            frames: look.back.max(1),
        });
    }
    let on_its_way = held.back.zip(stored(plan, lane.target));
    held.back = on_its_way.and_then(|(back, stored)| {
        if lane.target.ramps() {
            // A ramp finds its own way from here.
            set(plan, state, lane.target, stored, look.now, back.frames);
            return None;
        }
        let passed = look.until.saturating_sub(back.began);
        if passed >= back.frames {
            set(plan, state, lane.target, stored, look.now, 0);
            return None;
        }
        // In the automation's own 0 to 1, so that a setting on a
        // logarithmic scale moves evenly to the ear.
        let part = passed as f32 / back.frames as f32;
        let home = lane.range.normalized(stored);
        let value = lane.range.value(held.value + (home - held.value) * part);
        set(plan, state, lane.target, value, look.now, 0);
        Some(back)
    });
    state.lanes[index] = held;
    held.back.is_some()
}

/// Sets a target on its way to `value`, to be there `frames` frames after
/// `now`, or at once when that is none. A channel or track that mute or
/// solo silences stays silent.
fn set(plan: &Plan, state: &mut PlanState, target: Target, value: f32, now: u64, frames: u64) {
    let aim = |ramp: &mut Ramp, value: f32| {
        if frames == 0 {
            *ramp = Ramp::at_rest(value);
        } else {
            ramp.retarget(value, now, frames);
        }
    };
    match target {
        Target::ChannelGain(channel) => {
            let audible = plan.channels[channel].audible;
            let gain = if audible { value } else { 0.0 };
            aim(&mut state.channels[channel].gain, gain);
        }
        Target::ChannelPan(channel) => aim(&mut state.channels[channel].pan, value),
        Target::TrackGain(track) => {
            let gain = if plan.tracks[track].audible {
                value
            } else {
                0.0
            };
            aim(&mut state.tracks[track].gain, gain);
        }
        Target::TrackPan(track) => aim(&mut state.tracks[track].pan, value),
        Target::TrackParam { track, param } => state.track_processing[track].automate(param, value),
        Target::Send { track, edge } => {
            let slot = plan.tracks[track].edges[edge].slot;
            aim(&mut state.edges[slot], value);
        }
        Target::EffectParam { effect, param } => {
            let place = plan.effect_ids.get(effect.0);
            let (track, place) = match place {
                Some(place) => plan.effect_places[place],
                None => return,
            };
            if let Some(unit) = &mut state.chains[track][place] {
                unit.automate(param, value);
            }
        }
        Target::EffectMix(effect) => {
            let place = plan.effect_ids.get(effect.0);
            let (track, place) = match place {
                Some(place) => plan.effect_places[place],
                None => return,
            };
            if let Some(unit) = &mut state.chains[track][place] {
                unit.automate_mix(value);
            }
        }
        Target::InstrumentParam { channel, param } => {
            if let Some(unit) = state.instrument(channel) {
                unit.automate(param, value);
            }
        }
        Target::Tempo => {}
    }
}

/// The value the plan itself has for a target, where one can be set.
fn stored(plan: &Plan, target: Target) -> Option<f32> {
    let effect = |id: EffectId| {
        let (track, place) = plan.effect_places[plan.effect_ids.get(id.0)?];
        Some(&plan.tracks[track].effects[place])
    };
    match target {
        // A plan's gains have mute and solo in them already, and `set`
        // keeps what is silent silent.
        Target::ChannelGain(channel) => Some(plan.channels[channel].gain),
        Target::ChannelPan(channel) => Some(plan.channels[channel].pan),
        Target::TrackGain(track) => Some(plan.tracks[track].gain),
        Target::TrackPan(track) => Some(plan.tracks[track].pan),
        Target::TrackParam { track, param } => {
            use windfall_dsp::ParamSet;
            plan.tracks[track].processing.get(param)
        }
        Target::Send { track, edge } => Some(plan.tracks[track].edges[edge].gain),
        Target::EffectParam { effect: id, param } => {
            if let Some(binding) = plan.plugins.iter().find(|binding| {
                binding.target == windfall_project::PluginTarget::Effect { effect: id }
            }) {
                binding.parameters.get(param).map(|param| param.value)
            } else {
                effect(id)?.params.get(param)
            }
        }
        Target::EffectMix(id) => Some(effect(id)?.mix),
        Target::InstrumentParam { channel, param } => {
            if let Some(binding) = plan.plugins.iter().find(|binding| {
                binding.target
                    == windfall_project::PluginTarget::Instrument {
                        channel: plan.channels[channel].id,
                    }
            }) {
                binding.parameters.get(param).map(|param| param.value)
            } else {
                plan.channels[channel].instrument?.get(param)
            }
        }
        Target::Tempo => None,
    }
}

/// Carries the lanes of the plan being replaced over to the state of the
/// new plan: a lane that had its target in hand keeps it, and a gain or pan
/// it was moving carries on from where it was instead of starting from the
/// stored value. The look at the lanes that follows moves each on from
/// there. A setting that was being walked back to its stored value is not
/// carried: its processor is handed the new plan's value and glides the
/// rest of the way by itself.
///
/// Runs on the audio thread, so it must not allocate.
pub(crate) fn take_over(plan: &Plan, state: &mut PlanState, old_plan: &Plan, old: &PlanState) {
    for (index, lane) in plan.lanes.iter().enumerate() {
        let before = old_plan.lanes.iter().position(|old| old.key == lane.key);
        let Some(before) = before.filter(|before| old.lanes[*before].engaged) else {
            continue;
        };
        state.lanes[index] = old.lanes[before];
        state.engaged += 1;
        match (lane.target, old_plan.lanes[before].target) {
            (Target::ChannelGain(new), Target::ChannelGain(was)) => {
                state.channels[new].gain = old.channels[was].gain;
            }
            (Target::ChannelPan(new), Target::ChannelPan(was)) => {
                state.channels[new].pan = old.channels[was].pan;
            }
            (Target::TrackGain(new), Target::TrackGain(was)) => {
                state.tracks[new].gain = old.tracks[was].gain;
            }
            (Target::TrackPan(new), Target::TrackPan(was)) => {
                state.tracks[new].pan = old.tracks[was].pan;
            }
            (
                Target::Send { track, edge },
                Target::Send {
                    track: was,
                    edge: at,
                },
            ) => {
                let slot = plan.tracks[track].edges[edge].slot;
                state.edges[slot] = old.edges[old_plan.tracks[was].edges[at].slot];
            }
            // An effect or instrument was moved across with all it had.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(tick: u32, value: f32) -> AutomationPoint {
        AutomationPoint {
            tick,
            value,
            curve: 0.0,
            hold: false,
        }
    }

    fn piece(id: u32, row: usize, start: u32, end: u32, curve: usize) -> Piece {
        Piece {
            start,
            end,
            offset: 0,
            row,
            id: ClipId(id),
            automation: AutomationId(100 + curve as u32),
            curve,
            repeat: None,
        }
    }

    /// The spans as their start, the automation they come from, and
    /// whether a clip is playing.
    fn summary(spans: &[Span]) -> Vec<(f64, u32, bool)> {
        spans
            .iter()
            .map(|span| {
                let playing = matches!(span.source, Source::Curve { .. });
                (span.start, span.automation.0, playing)
            })
            .collect()
    }

    #[test]
    fn a_lane_follows_its_clips_and_holds_between_them() {
        let curves = vec![vec![point(0, 0.0), point(1_000, 1.0)], vec![point(0, 0.5)]];
        // Two clips with a gap between them.
        let pieces = [piece(1, 0, 1_000, 1_500, 0), piece(2, 0, 3_000, 4_000, 1)];
        let spans = flatten(&pieces, &curves);
        assert_eq!(
            summary(&spans),
            [
                (1_000.0, 100, true),
                (1_500.0, 100, false),
                (3_000.0, 101, true),
                (4_000.0, 101, false),
            ]
        );
        let lane = Lane {
            key: AutomationTarget::Tempo,
            target: Target::Tempo,
            range: AutomationRange::TEMPO,
            spans,
            curves,
            patterns: Vec::new(),
        };
        // Nothing before the first clip, the curve inside it, and its last
        // value from its end until the next clip.
        assert_eq!(lane.value_at(999.0), None);
        let value = |tick: f64| lane.value_at(tick).unwrap().0;
        assert_eq!(value(1_000.0), 0.0);
        assert!((value(1_250.0) - 0.25).abs() < 1e-6);
        assert!((value(1_500.0) - 0.5).abs() < 1e-6);
        assert!((value(2_999.0) - 0.5).abs() < 1e-6);
        assert_eq!(value(3_000.0), 0.5);
        assert_eq!(value(9_000_000.0), 0.5);
        assert_eq!(lane.value_at(2_000.0).unwrap().1, AutomationId(100));
        assert!((lane.real_at(1_250.0).unwrap() - 138.0).abs() < 1e-3);
    }

    #[test]
    fn of_clips_that_overlap_the_one_nearest_the_top_wins_then_the_later_one() {
        let curves = vec![
            vec![point(0, 0.1)],
            vec![point(0, 0.2)],
            vec![point(0, 0.3)],
        ];
        // A long clip on the second row, a short one above it, and one
        // that starts later on the second row as well.
        let pieces = [
            piece(1, 1, 0, 4_000, 0),
            piece(2, 0, 1_000, 2_000, 1),
            piece(3, 1, 3_000, 5_000, 2),
        ];
        let spans = flatten(&pieces, &curves);
        assert_eq!(
            summary(&spans),
            [
                (0.0, 100, true),
                (1_000.0, 101, true),
                (2_000.0, 100, true),
                (3_000.0, 102, true),
                (5_000.0, 102, false),
            ]
        );

        // On one row and one tick the lower id wins, and of clips that end
        // together the same one leaves its value behind.
        let twins = [piece(8, 0, 0, 1_000, 1), piece(7, 0, 0, 1_000, 2)];
        let spans = flatten(&twins, &curves);
        assert_eq!(summary(&spans), [(0.0, 102, true), (1_000.0, 102, false)]);
    }

    #[test]
    fn a_jump_has_a_value_on_each_side() {
        let mut points = vec![
            point(0, 0.2),
            point(100, 0.4),
            point(100, 0.8),
            point(200, 1.0),
        ];
        points[2].hold = true;
        assert_eq!(curve_before(&points, 0.0), 0.2);
        assert!((curve_before(&points, 50.0) - 0.3).abs() < 1e-6);
        assert!((curve_before(&points, 100.0) - 0.4).abs() < 1e-6);
        assert_eq!(curve_value(&points, 100.0), 0.8);
        // A hold jumps at the next point.
        assert_eq!(curve_before(&points, 200.0), 0.8);
        assert_eq!(curve_value(&points, 200.0), 1.0);
        assert_eq!(curve_before(&points, 900.0), 1.0);

        // The lane jumps where a clip begins, too.
        let curves = vec![vec![point(0, 0.5)]];
        let lane = Lane {
            key: AutomationTarget::Tempo,
            target: Target::Tempo,
            range: AutomationRange::TEMPO,
            spans: flatten(&[piece(1, 0, 1_000, 2_000, 0)], &curves),
            curves,
            patterns: Vec::new(),
        };
        assert_eq!(lane.real_before(1_000.0), None);
        assert_eq!(lane.real_at(1_000.0), Some(266.0));
        assert_eq!(lane.real_before(1_000.5), Some(266.0));
        assert_eq!(lane.real_before(2_000.0), Some(266.0));
        assert_eq!(
            lane.corners(3_000.0, 60.0),
            [0.0, 1_000.0, 2_000.0, 3_000.0]
        );
    }

    #[test]
    fn a_clip_is_a_window_onto_its_curve() {
        let curves = vec![vec![point(0, 0.0), point(1_000, 1.0)]];
        let mut window = piece(1, 0, 5_000, 5_200, 0);
        window.offset = 500;
        let lane = Lane {
            key: AutomationTarget::Tempo,
            target: Target::Tempo,
            range: AutomationRange::TEMPO,
            spans: flatten(&[window], &curves),
            curves,
            patterns: Vec::new(),
        };
        let value = |tick: f64| lane.value_at(tick).unwrap().0;
        assert!((value(5_000.0) - 0.5).abs() < 1e-6);
        assert!((value(5_100.0) - 0.6).abs() < 1e-6);
        // Held where the window closes, not where the curve goes on to.
        assert!((value(5_200.0) - 0.7).abs() < 1e-6);
        assert!((value(8_000.0) - 0.7).abs() < 1e-6);
    }
}
