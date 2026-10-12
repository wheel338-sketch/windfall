//! The transport: where the playhead is and which notes fall on which
//! output frame.
//!
//! Musical time is tied to output frames by a [`Clock`], a straight line
//! from an anchor frame. A note plays on the first frame whose time is at
//! or after the note's tick, and that frame is computed from the anchor
//! alone, never by adding up block lengths. However the output is cut into
//! buffers, a note therefore lands on the same frame, exactly once.
//!
//! The clock does not go back at a loop point. It counts on, and the
//! sequencer remembers the clock tick at which the pass now playing began.
//! A tick on the clock therefore names one moment for as long as playback
//! is not moved by hand, which is what lets a sounding note keep the tick
//! it ends on: when the tempo changes, the clock is bent from that frame
//! on and the note's end moves with it.
//!
//! # A tempo that automation moves
//!
//! The clock stays a straight line even then. In song mode it counts the
//! plan's *warped* ticks ([`Plan::warp`]): the tick at which each moment
//! of the song would come at the stored tempo. Every place in the song is
//! warped on its way to the clock and every reading of the clock is
//! unwarped on its way back, so notes, clip edges and the playhead are
//! all placed by the tempo map, exactly, from one anchor. With a steady
//! tempo a tick is its own warped tick and nothing changes.

use windfall_core::samples_per_tick;
use windfall_ipc::PlayMode;
use windfall_project::PatternId;

use crate::plan::{FALLBACK_LOOP_TICKS, Plan, PlanPattern};

/// How close to a frame boundary, in frames, a note may fall past it and
/// still be played on it. It absorbs floating-point rounding, so a note
/// that sits exactly on a frame in exact arithmetic is never a frame late.
const FRAME_TOLERANCE: f64 = 1e-6;

/// Note-ons the sequencer hands over in one round. A stretch of audio that
/// holds more is worked through in several rounds, so this bounds memory
/// and never the number of notes that play.
pub(crate) const MAX_TRIGGERS: usize = 1024;

/// A straight line from ticks to frames.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Clock {
    anchor_frame: u64,
    /// Tick at the anchor frame.
    anchor_tick: f64,
    frames_per_tick: f64,
}

impl Clock {
    pub(crate) fn control_default(rate: u32, bpm: f64) -> Self {
        Self {
            anchor_frame: 0,
            anchor_tick: 0.0,
            frames_per_tick: samples_per_tick(bpm, f64::from(rate)),
        }
    }
    /// First frame at or after `tick`, never before the anchor. A tick of
    /// infinity gives a frame that never comes.
    pub fn frame_of(&self, tick: f64) -> u64 {
        let frames = ((tick - self.anchor_tick) * self.frames_per_tick - FRAME_TOLERANCE).ceil();
        self.anchor_frame.saturating_add(frames.max(0.0) as u64)
    }

    /// Tick at `frame`. A frame before the anchor reads as the anchor.
    pub fn tick_at(&self, frame: u64) -> f64 {
        self.anchor_tick + frame.saturating_sub(self.anchor_frame) as f64 / self.frames_per_tick
    }

    /// Frames the clock takes to get from tick `from` to tick `to`, with
    /// the fraction of a frame kept.
    pub fn frames_between(&self, from: f64, to: f64) -> f64 {
        (to - from) * self.frames_per_tick
    }
}

/// What separates notes already behind the playhead from notes still to
/// come, at the moment the clock was last replaced.
#[derive(Debug, Clone, Copy)]
enum Floor {
    /// Playback jumped to this clock tick. Notes before it are skipped.
    Tick(f64),
    /// The tempo map changed under the song and the clock was set up
    /// anew. Notes up to and on this clock tick had played by then. The
    /// ones after it, `behind` ticks of which lie before the clock's
    /// anchor, had not.
    After { tick: f64, behind: f64 },
    /// The tempo changed at `frame`. Notes the old clock placed before that
    /// frame have played, and notes it placed at or after it have not.
    /// Asking the old clock keeps a note sitting right on the change from
    /// being played twice or not at all.
    Clock { clock: Clock, frame: u64 },
}

/// A note or an audio clip to start on an exact frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Trigger {
    pub frame: u64,
    /// Position in the order the sequencer found the notes of one round.
    /// Sorting by frame and then by this gives one fixed order for notes
    /// that share a frame.
    order: u32,
    pub what: Fire,
}

/// What a [`Trigger`] starts.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Fire {
    Click {
        accent: bool,
    },
    Note {
        source: crate::note_curves::CurveSource,
        /// Index into [`Plan::channels`].
        channel: usize,
        key: u8,
        velocity: f32,
        pan: f32,
        expression: windfall_dsp::NoteExpression,
        /// Clock tick on which the note ends. Only samplers with an
        /// envelope and instruments care.
        end: f64,
    },
    Clip {
        /// Index into [`Plan::audio_clips`].
        clip: usize,
        /// Clock tick of the start of the pass of the song the clip is
        /// part of, which the clip's own ticks are counted from.
        origin: f64,
    },
}

/// How far the notes of a block have been handed over: every note before
/// `frame`, and the first `started` notes on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cursor {
    pub frame: u64,
    pub started: usize,
}

impl Cursor {
    /// Nothing on `frame` has been handed over yet.
    pub fn at(frame: u64) -> Self {
        Self { frame, started: 0 }
    }
}

/// The note-ons of one round, in a buffer allocated once.
pub(crate) struct Triggers {
    items: Vec<Trigger>,
    /// Most notes one round holds. `items` was allocated with room for
    /// them and never grows.
    limit: usize,
    /// Where the round began.
    from: Cursor,
    /// Notes on the first frame passed over so far, because an earlier
    /// round handed them over.
    passed: usize,
    /// Frame of the earliest note there was no room for.
    left_out: Option<u64>,
}

impl Triggers {
    pub fn new() -> Self {
        Self::with_capacity(MAX_TRIGGERS)
    }

    fn with_capacity(capacity: usize) -> Self {
        let limit = capacity.max(1);
        Self {
            items: Vec::with_capacity(limit),
            limit,
            from: Cursor::at(0),
            passed: 0,
            left_out: None,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn get(&self, index: usize) -> Trigger {
        self.items[index]
    }

    fn begin(&mut self, from: Cursor) {
        self.items.clear();
        self.from = from;
        self.passed = 0;
        self.left_out = None;
    }

    /// Takes a note in the order the sequencer finds them. Notes that share
    /// a frame are always found in the same order, whatever stretch of
    /// frames is being searched, which is what makes counting them a way to
    /// carry on in the middle of a frame.
    fn push(&mut self, mut trigger: Trigger) {
        if trigger.frame == self.from.frame && self.passed < self.from.started {
            self.passed += 1;
        } else if self.items.len() < self.limit {
            trigger.order = self.items.len() as u32;
            self.items.push(trigger);
        } else {
            let earliest = self
                .left_out
                .map_or(trigger.frame, |frame| frame.min(trigger.frame));
            self.left_out = Some(earliest);
        }
    }

    /// Puts the notes in playing order. If some were left out, only the
    /// notes up to the earliest of those stay, and the place to carry on
    /// from is returned.
    ///
    /// Once the buffer is full it stays full, so of the notes on any one
    /// frame the ones that got in were all found before the ones that did
    /// not. The notes that stay are therefore exactly the first so many in
    /// playing order.
    fn finish(&mut self) -> Option<Cursor> {
        self.items
            .sort_unstable_by_key(|trigger| (trigger.frame, trigger.order));
        let frame = self.left_out?;
        let kept = self.items.partition_point(|trigger| trigger.frame <= frame);
        self.items.truncate(kept);
        let on_frame = self
            .items
            .iter()
            .rev()
            .take_while(|trigger| trigger.frame == frame)
            .count();
        let before = if frame == self.from.frame {
            self.from.started
        } else {
            0
        };
        Some(Cursor {
            frame,
            started: before + on_frame,
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Sequencer {
    sample_rate: f64,
    playing: bool,
    mode: PlayMode,
    pattern: PatternId,
    loop_song: bool,
    recording: bool,
    metronome: bool,
    region: Option<windfall_project::TickRange>,
    clock: Clock,
    /// Clock tick at which the pass of the pattern or song now playing
    /// began. Each time around adds the length of what was played.
    pass_start: f64,
    floor: Floor,
    /// Where playback last started or was moved to by a seek. It is the
    /// playhead while stopped, so stopping returns here, and the next
    /// `play` starts here.
    position: f64,
    /// Passes left before playback ends by itself. `None` follows the
    /// transport's own looping. The offline renderer sets it.
    passes_left: Option<u32>,
    /// Ticks by which jumps have moved the clock's reading since
    /// [`Sequencer::take_clock_shift`] last asked.
    shift: f64,
    /// Playback has started somewhere, or been moved, since
    /// [`Sequencer::take_jump`] last asked.
    jumped: bool,
}

impl Sequencer {
    /// Bend the host clock without moving an already sounding note's end.
    #[cfg(feature = "plugin-export")]
    pub fn set_plugin_tempo(&mut self, tempo: f64, now: u64) {
        let frames_per_tick = samples_per_tick(tempo, self.sample_rate);
        if self.clock.frames_per_tick == frames_per_tick {
            return;
        }
        let before = self.clock;
        self.clock = Clock {
            anchor_frame: now,
            anchor_tick: before.tick_at(now),
            frames_per_tick,
        };
        if self.playing && now > before.anchor_frame {
            self.floor = Floor::Clock {
                clock: before,
                frame: now,
            };
        }
    }

    pub fn new(sample_rate: u32, plan: &Plan) -> Self {
        let sample_rate = f64::from(sample_rate);
        Self {
            sample_rate,
            playing: false,
            mode: PlayMode::Pattern,
            pattern: PatternId(0),
            loop_song: false,
            recording: false,
            metronome: false,
            region: None,
            clock: Clock {
                anchor_frame: 0,
                anchor_tick: 0.0,
                frames_per_tick: samples_per_tick(plan.tempo_bpm, sample_rate),
            },
            pass_start: 0.0,
            floor: Floor::Tick(0.0),
            position: 0.0,
            passes_left: None,
            shift: 0.0,
            jumped: false,
        }
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    /// True while the playlist is what plays.
    pub fn in_song(&self) -> bool {
        self.playing && self.mode == PlayMode::Song
    }

    /// What the transport is set to play.
    pub fn mode(&self) -> PlayMode {
        self.mode
    }

    pub fn set_metronome(&mut self, enabled: bool) {
        self.metronome = enabled;
    }

    /// Clock tick at which the pass of the pattern or song now playing
    /// began.
    pub fn pass_start(&self) -> f64 {
        self.pass_start
    }

    /// Whether playback has started, or been moved to another place, since
    /// this was last asked. Whatever plays on from before a jump, such as
    /// an audio clip, has to be ended and found again at the new place.
    pub fn take_jump(&mut self) -> bool {
        std::mem::take(&mut self.jumped)
    }

    /// The clock that note ends are measured on. It keeps running while the
    /// transport is stopped, so notes left ringing still end on time.
    pub fn clock(&self) -> Clock {
        self.clock
    }

    /// How far jumps have moved the clock's reading since this was last
    /// asked. A note that was sounding across a jump must have its end
    /// moved by as much to last as long as it would have.
    pub fn take_clock_shift(&mut self) -> f64 {
        std::mem::take(&mut self.shift)
    }

    /// Where stopping returns the playhead to.
    pub fn start(&self) -> f64 {
        self.position
    }

    /// The playhead in ticks as of frame `now`.
    pub fn tick(&self, plan: &Plan, now: u64) -> f64 {
        if !self.playing {
            return self.position;
        }
        let tick = self.clock.tick_at(now) - self.pass_start;
        if self.mode == PlayMode::Song && self.region.is_some() {
            return self.place(plan, self.region_tick(plan, tick));
        }
        let length = self.length(plan);
        // The wrap itself happens while the next block is processed.
        let wrapped = if tick >= length { tick - length } else { tick };
        self.place(plan, wrapped.max(0.0))
    }

    /// The place in the song that frame `frame`, now or a little ahead,
    /// plays: what automation is read at. `None` while the playlist is not
    /// what plays. A frame past the end of the song reads as the start of
    /// the next time around, or as the end when there is none.
    pub fn song_tick_at(&self, plan: &Plan, frame: u64) -> Option<f64> {
        if !self.in_song() {
            return None;
        }
        let length = self.length(plan);
        let mut tick = self.clock.tick_at(frame) - self.pass_start;
        if self.region.is_some() {
            return Some(self.place(plan, self.region_tick(plan, tick)));
        }
        if tick >= length {
            tick = if self.loops() { tick - length } else { length };
        }
        Some(plan.unwarp(tick.max(0.0)))
    }

    /// The place in the pattern that frame `frame`, now or a little ahead,
    /// plays while the pattern plays by itself: what its event lanes are
    /// read at, with the pattern's index in [`Plan::patterns`]. `None`
    /// while the transport is stopped or the playlist is what plays. A
    /// frame past the end of the pattern reads as the next time around.
    pub fn pattern_tick_at(&self, plan: &Plan, frame: u64) -> Option<(usize, f64)> {
        if !self.playing || self.mode != PlayMode::Pattern {
            return None;
        }
        let index = plan
            .pattern_ids
            .get(self.pattern.0)
            .or((!plan.patterns.is_empty()).then_some(0))?;
        let length = f64::from(plan.patterns[index].length);
        let tick = (self.clock.tick_at(frame) - self.pass_start).max(0.0);
        Some((index, if tick >= length { tick % length } else { tick }))
    }

    /// The place in the pattern or song of a reading of the clock, counted
    /// from the start of the pass.
    fn place(&self, plan: &Plan, counted: f64) -> f64 {
        match self.mode {
            PlayMode::Pattern => counted,
            PlayMode::Song => plan.unwarp(counted),
        }
    }

    /// What the clock counts from the start of a pass to a place in the
    /// pattern or song: the inverse of [`place`](Self::place).
    fn counted(&self, plan: &Plan, tick: f64) -> f64 {
        match self.mode {
            PlayMode::Pattern => tick,
            PlayMode::Song => plan.warp(tick),
        }
    }

    /// The playhead as it is heard on frame `now` through an engine whose
    /// output is `latency` frames behind the notes: where the playhead was
    /// that many frames ago. Before playback has been running that long it
    /// reads where playback started.
    pub fn tick_heard(&self, plan: &Plan, now: u64, latency: u64) -> f64 {
        if !self.playing || latency == 0 {
            return self.tick(plan, now);
        }
        let tick = self.clock.tick_at(now.saturating_sub(latency)) - self.pass_start;
        if self.mode == PlayMode::Song && self.region.is_some() {
            return self.place(plan, self.region_tick(plan, tick));
        }
        let length = self.length(plan);
        let counted = if tick >= length {
            tick - length
        } else if tick < 0.0 && self.pass_start > 0.0 {
            // The pass now being worked out has not reached the output yet:
            // what is heard is still the end of the one before.
            (tick + length).max(0.0)
        } else {
            tick.max(0.0)
        };
        self.place(plan, counted)
    }

    /// Follows a new plan from frame `now`: picks up a tempo change without
    /// moving the playhead, and keeps the playhead inside whatever is
    /// playing if that got shorter.
    ///
    /// The clock is bent whether or not the transport is running, because
    /// notes left ringing after playback ended are still timed by it.
    ///
    /// `old` is the plan being replaced. When the song is playing and the
    /// two plans map its tempo differently, every reading of the clock
    /// means another place in the song than before. The clock is then set
    /// up anew on frame `now`, at the place the song has reached, and
    /// true is returned: whatever holds a clock tick of the pass that is
    /// playing, such as the end of a sounding note, has to be moved with
    /// [`Sequencer::moved`].
    pub fn set_plan(&mut self, plan: &Plan, old: &Plan, now: u64) -> bool {
        let frames_per_tick = samples_per_tick(plan.tempo_bpm, self.sample_rate);
        if self.in_song() && plan.tempo_map != old.tempo_map {
            let before = self.clock;
            let origin = self.pass_start;
            let moved = |tick: f64| Self::moved(plan, old, origin, tick);
            let anchor_tick = moved(before.tick_at(now));
            self.floor = match self.floor {
                // Nothing has played on this clock yet.
                Floor::Tick(tick) if now <= before.anchor_frame => Floor::Tick(moved(tick)),
                _ => {
                    // The last tick whose frame was before `now`.
                    let last = now.saturating_sub(1);
                    let played = before.tick_at(last) + FRAME_TOLERANCE / before.frames_per_tick;
                    let tick = moved(played).min(anchor_tick);
                    Floor::After {
                        tick,
                        behind: anchor_tick - tick,
                    }
                }
            };
            self.clock = Clock {
                anchor_frame: now,
                anchor_tick,
                frames_per_tick,
            };
            self.keep_in_range(plan, now);
            return true;
        }
        if frames_per_tick != self.clock.frames_per_tick {
            if now > self.clock.anchor_frame {
                let old = self.clock;
                self.clock = Clock {
                    anchor_frame: now,
                    anchor_tick: old.tick_at(now),
                    frames_per_tick,
                };
                if self.playing {
                    self.floor = Floor::Clock {
                        clock: old,
                        frame: now,
                    };
                }
            } else {
                self.clock.frames_per_tick = frames_per_tick;
            }
        }
        self.keep_in_range(plan, now);
        false
    }

    /// Where a clock tick of the pass of the song that began on clock tick
    /// `origin` lies once `plan` has replaced `old`: at the same place in
    /// the song. Ticks from before the pass, and ticks that never come,
    /// stay what they are.
    pub fn moved(plan: &Plan, old: &Plan, origin: f64, tick: f64) -> f64 {
        if tick.is_finite() && tick >= origin {
            origin + plan.warp(old.unwarp(tick - origin))
        } else {
            tick
        }
    }

    /// Starts playback from `from`, or from the stopped position when that
    /// is `None`. Either way the stopped position stays where it is.
    /// `passes` limits how many times the pattern or song plays before
    /// stopping by itself.
    pub fn play(&mut self, plan: &Plan, now: u64, passes: Option<u32>, from: Option<f64>) {
        if self.playing {
            return;
        }
        self.playing = true;
        self.passes_left = passes;
        let from = from.unwrap_or(self.position);
        let from = if self.mode == PlayMode::Song {
            self.region
                .map_or(from, |r| from.clamp(f64::from(r.start), f64::from(r.end)))
        } else {
            from
        };
        let requested = self.counted(plan, from);
        let from = if self.mode == PlayMode::Song && self.region.is_some() && !self.recording {
            crate::timeline::aligned_tick(plan, from, self.sample_rate)
        } else {
            from
        };
        let from = self.counted(plan, from);
        self.jump(from, now);
        // The first sample is aligned, but events at the original musical
        // start still belong to that sample. Do not advance the event floor.
        self.floor = Floor::Tick(requested);
        self.keep_in_range(plan, now);
    }

    /// Stops and returns the playhead to where playback last started.
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Moves the playhead. Returns true when that interrupted playback.
    pub fn seek(&mut self, tick: f64, plan: &Plan, now: u64) -> bool {
        let tick = if tick.is_finite() { tick.max(0.0) } else { 0.0 };
        let tick = if self.mode == PlayMode::Song {
            self.region
                .map_or(tick, |r| tick.clamp(f64::from(r.start), f64::from(r.end)))
        } else {
            tick
        };
        self.position = tick;
        if !self.playing {
            return false;
        }
        let aligned = if self.mode == PlayMode::Song && self.region.is_some() {
            crate::timeline::aligned_tick(plan, tick, self.sample_rate)
        } else {
            tick
        };
        self.jump(self.counted(plan, aligned), now);
        self.floor = Floor::Tick(self.counted(plan, tick));
        self.keep_in_range(plan, now);
        true
    }

    /// Applies transport settings. Returns true when playback restarted from
    /// the top because the mode changed while playing.
    pub fn set_transport(
        &mut self,
        mode: PlayMode,
        pattern: PatternId,
        loop_song: bool,
        plan: &Plan,
        now: u64,
    ) -> bool {
        let mode_changed = mode != self.mode;
        self.mode = mode;
        self.pattern = pattern;
        self.loop_song = loop_song;
        if mode_changed {
            // A pattern position means nothing on the playlist and the
            // other way round.
            self.position = 0.0;
            if self.playing {
                self.jump(0.0, now);
            }
        }
        self.keep_in_range(plan, now);
        mode_changed && self.playing
    }

    /// Finds the notes that start in frames `from.frame..to`, leaving out
    /// the ones `from` says were handed over already, and puts them in
    /// `triggers` in the order they must be started.
    ///
    /// When there are more notes than `triggers` holds, it gets the earliest
    /// of them, and the place they reach up to is returned: start them,
    /// [`advance`](Self::advance) to that frame, and ask again from there.
    /// `None` means the notes are all there.
    ///
    /// The transport itself is not moved, so asking twice gives the same
    /// answer.
    pub fn collect(
        &self,
        plan: &Plan,
        from: Cursor,
        to: u64,
        triggers: &mut Triggers,
    ) -> Option<Cursor> {
        let mut end = to;
        loop {
            triggers.begin(from);
            let mut ahead = *self;
            ahead.walk(plan, from.frame, end, |sequencer, first, last| {
                sequencer.gather(plan, first, last, triggers);
            });
            let next = match triggers.finish() {
                Some(next) => next,
                None if end == to => return None,
                None => Cursor::at(end),
            };
            if next != from {
                return Some(next);
            }
            // Later frames filled the buffer before a note on the very
            // first frame turned up. Looking at that frame alone gets its
            // notes in.
            end = from.frame + 1;
        }
    }

    /// Moves the transport through frames `from..to`: around any loop point
    /// in that range, and to a stop at the end of the last pass.
    pub fn advance(&mut self, plan: &Plan, from: u64, to: u64) {
        self.walk(plan, from, to, |_, _, _| {});
    }

    /// Goes through frames `from..to` one pass of the pattern or song at a
    /// time. `visit` is shown each stretch of frames while the transport
    /// stands as it does when they play.
    fn walk(&mut self, plan: &Plan, from: u64, to: u64, mut visit: impl FnMut(&Self, u64, u64)) {
        let mut cursor = from;
        while self.playing && cursor < to {
            let length = self.length(plan);
            // First frame at or past the end of the pattern or song.
            let end_frame = self.clock.frame_of(self.pass_start + length).max(cursor);
            // Notes just before the end can round onto that frame, so this
            // pass still owns it.
            let pass_end = to.min(end_frame + 1);
            visit(self, cursor, pass_end);
            if end_frame >= to {
                break;
            }
            if self.another_pass() {
                self.pass_start += length;
                cursor = end_frame;
            } else {
                self.finish();
            }
        }
    }

    fn gather(&self, plan: &Plan, from: u64, to: u64, triggers: &mut Triggers) {
        match self.mode {
            PlayMode::Pattern => {
                if let Some(pattern) = self.pattern(plan) {
                    self.gather_pattern(pattern, from, to, triggers);
                }
            }
            PlayMode::Song => self.gather_song(plan, from, to, triggers),
        }
        if self.metronome {
            self.gather_clicks(plan, from, to, triggers);
        }
    }

    fn gather_clicks(&self, plan: &Plan, from: u64, to: u64, triggers: &mut Triggers) {
        let in_song = self.mode == PlayMode::Song;
        let (meters, length) = if in_song {
            (&plan.meters, self.song_end(plan))
        } else if let Some(pattern) = self.pattern(plan) {
            (&pattern.meters, pattern.length)
        } else {
            return;
        };
        let Ok(meters) = meters else {
            return;
        };
        let (low, high) = self.window(from, to);
        let (low, high) = if in_song {
            (plan.unwarp(low), plan.unwarp(high))
        } else {
            (low, high)
        };
        for (index, segment) in meters.iter().enumerate() {
            let start = f64::from(segment.start_tick());
            let end = f64::from(
                meters
                    .get(index + 1)
                    .map_or(length, |next| next.start_tick())
                    .min(length),
            );
            if start > high || end < low {
                continue;
            }
            let signature = segment.signature();
            let beat = f64::from(signature.ticks_per_beat());
            let mut position = ((low - start) / beat).floor().max(0.0) as u32;
            loop {
                let tick = start + f64::from(position) * beat;
                if tick > high || tick >= end {
                    break;
                }
                let counted = if in_song { plan.warp(tick) } else { tick };
                if let Some(frame) = self.due(counted, from, to) {
                    triggers.push(Trigger {
                        frame,
                        order: 0,
                        what: Fire::Click {
                            accent: position.is_multiple_of(u32::from(signature.numerator)),
                        },
                    });
                }
                position += 1;
            }
        }
    }

    fn gather_pattern(&self, pattern: &PlanPattern, from: u64, to: u64, triggers: &mut Triggers) {
        let (low, high) = self.window(from, to);
        let first = pattern.events.partition_point(|event| event.tick < low);
        for event in &pattern.events[first..] {
            if event.tick > high {
                break;
            }
            let Some(frame) = self.due(event.tick, from, to) else {
                continue;
            };
            triggers.push(Trigger {
                frame,
                order: 0,
                what: Fire::Note {
                    source: crate::note_curves::CurveSource {
                        pattern: pattern.id,
                        note: event.source,
                        start: self.pass_start + event.tick,
                        end: self.pass_start + event.tick + event.length,
                        song_origin: None,
                        base_pan: event.pan,
                        base_expression: event.expression,
                        active: !pattern.curves_for(event.source).is_empty(),
                    },
                    channel: event.channel,
                    key: event.key,
                    velocity: event.velocity,
                    pan: event.pan,
                    expression: event.expression,
                    end: self.pass_start + (event.tick + event.length),
                },
            });
        }
    }

    fn gather_song(&self, plan: &Plan, from: u64, to: u64, triggers: &mut Triggers) {
        // The window is in what the clock counts. The clips and notes are
        // looked up by their places in the song.
        let (low, high) = self.window(from, to);
        let (low, high) = (plan.unwarp(low), plan.unwarp(high));
        let first = plan
            .clips
            .partition_point(|clip| f64::from(clip.reach) < low);
        for clip in &plan.clips[first..] {
            let clip_start = f64::from(clip.start);
            let clip_end = f64::from(clip.end);
            if clip_start > high {
                break;
            }
            if clip_end < low {
                continue;
            }
            let pattern = &plan.patterns[clip.pattern];
            let pattern_length = f64::from(pattern.length);
            // Timeline tick at which the clip's first, possibly cut off,
            // repeat of the pattern begins.
            let origin = clip_start - f64::from(clip.offset);
            let first_repeat = ((low - origin) / pattern_length).floor().max(0.0) as u64;
            let last_repeat = ((high.min(clip_end) - origin) / pattern_length)
                .floor()
                .max(0.0) as u64;
            for repeat in first_repeat..=last_repeat {
                let shift = origin + repeat as f64 * pattern_length;
                let start = pattern
                    .events
                    .partition_point(|event| shift + event.tick < low);
                for event in &pattern.events[start..] {
                    let tick = shift + event.tick;
                    if tick > high {
                        break;
                    }
                    if tick < clip_start || tick >= clip_end {
                        continue;
                    }
                    if clip.only.is_some_and(|only| only != event.channel) {
                        continue;
                    }
                    let Some(frame) = self.due(plan.warp(tick), from, to) else {
                        continue;
                    };
                    let end = if event.scripted { tick + event.length } else { (tick + event.length).min(clip_end) };
                    triggers.push(Trigger {
                        frame,
                        order: 0,
                        what: Fire::Note {
                            source: crate::note_curves::CurveSource {
                                pattern: pattern.id,
                                note: event.source,
                                start: self.pass_start + plan.warp(tick),
                                end: self.pass_start
                                    + plan.warp(end),
                                song_origin: Some(self.pass_start),
                                base_pan: event.pan,
                                base_expression: event.expression,
                                active: !pattern.curves_for(event.source).is_empty(),
                            },
                            channel: event.channel,
                            key: event.key,
                            velocity: event.velocity,
                            pan: event.pan,
                            expression: event.expression,
                            // Ordinary held notes respect the clip window;
                            // scripted notes keep the transformed duration.
                            end: self.pass_start + plan.warp(end),
                        },
                    });
                }
            }
        }

        // The audio clips that start in these frames, after the notes, so
        // that everything on one frame is always found in the same order.
        let first = plan
            .audio_clips
            .partition_point(|clip| f64::from(clip.start) < low);
        for (index, clip) in plan.audio_clips.iter().enumerate().skip(first) {
            let start = f64::from(clip.start);
            if start > high {
                break;
            }
            if let Some(frame) = self.due(plan.warp(start), from, to) {
                triggers.push(Trigger {
                    frame,
                    order: 0,
                    what: Fire::Clip {
                        clip: index,
                        origin: self.pass_start,
                    },
                });
            }
        }
    }

    /// A range of ticks into the pass sure to contain every note due in
    /// frames `from..to`. It is wider than needed; [`Self::due`] decides
    /// exactly.
    fn window(&self, from: u64, to: u64) -> (f64, f64) {
        let tick_per_frame = 1.0 / self.clock.frames_per_tick;
        // After a tempo change, notes between the last frame of the old
        // tempo and the first frame of the new one are still due.
        let behind = match &self.floor {
            Floor::Tick(_) => 0.0,
            Floor::After { behind, .. } => *behind,
            Floor::Clock { clock, .. } => 1.0 / clock.frames_per_tick,
        };
        let to = to.max(from);
        (
            self.clock.tick_at(from) - self.pass_start - 2.0 * tick_per_frame - 2.0 * behind,
            self.clock.tick_at(to) - self.pass_start + tick_per_frame,
        )
    }

    /// The frame a note plays on that the clock counts `tick` ticks into
    /// the pass, if that is inside `from..to` and the note is not already
    /// behind the playhead.
    fn due(&self, tick: f64, from: u64, to: u64) -> Option<u64> {
        let tick = self.pass_start + tick;
        let ahead = match &self.floor {
            Floor::Tick(floor) => tick >= *floor,
            Floor::After { tick: floor, .. } => tick > *floor,
            Floor::Clock { clock, frame } => clock.frame_of(tick) >= *frame,
        };
        let frame = self.clock.frame_of(tick);
        (ahead && frame >= from && frame < to).then_some(frame)
    }

    /// Length of what is playing, the pattern or the whole song, as the
    /// clock counts it.
    fn length(&self, plan: &Plan) -> f64 {
        match self.mode {
            PlayMode::Pattern => f64::from(
                self.pattern(plan)
                    .map_or(FALLBACK_LOOP_TICKS, |pattern| pattern.length),
            ),
            PlayMode::Song => plan.warp(f64::from(self.song_end(plan))),
        }
    }

    pub fn set_region(
        &mut self,
        region: Option<windfall_project::TickRange>,
        plan: &Plan,
        now: u64,
    ) -> bool {
        self.region = region;
        if self.mode != PlayMode::Song {
            return false;
        }
        let tick = self.raw_song_tick(plan, now);
        if let Some(range) = region
            && (tick < f64::from(range.start) || tick >= f64::from(range.end))
        {
            return self.seek(f64::from(range.start), plan, now);
        }
        false
    }

    pub fn region(&self) -> Option<windfall_project::TickRange> {
        self.region
    }
    pub fn navigation_enabled(&self) -> bool {
        self.passes_left.is_none()
    }
    pub fn song_looping(&self) -> bool {
        self.loop_song && self.navigation_enabled()
    }
    pub fn set_recording(&mut self, recording: bool) {
        self.recording = recording;
    }
    /// Keep the fractional frame phase of a recording region across passes.
    pub fn wrap_recording_region(&mut self, plan: &Plan) -> bool {
        let Some(range) = self.region else {
            return false;
        };
        if !self.recording || !self.song_looping() {
            return false;
        }
        self.pass_start += plan.warp(f64::from(range.end)) - plan.warp(f64::from(range.start));
        self.floor = Floor::Tick(plan.warp(f64::from(range.start)));
        self.jumped = true;
        true
    }
    pub fn raw_song_tick(&self, plan: &Plan, now: u64) -> f64 {
        if self.playing {
            plan.unwarp((self.clock.tick_at(now) - self.pass_start).max(0.0))
        } else {
            self.position
        }
    }
    pub fn song_frame(&self, plan: &Plan, tick: u32) -> u64 {
        self.clock
            .frame_of(self.pass_start + plan.warp(f64::from(tick)))
    }
    pub fn navigation_frame(&self, plan: &Plan, tick: u32, now: u64) -> Option<u64> {
        self.due(plan.warp(f64::from(tick)), now, u64::MAX)
    }
    /// Selected loops return to their start, including automation lookahead.
    fn region_tick(&self, plan: &Plan, tick: f64) -> f64 {
        let range = self.region.expect("a selected song region exists");
        let start = plan.warp(f64::from(range.start));
        let end = plan.warp(f64::from(range.end));
        if tick >= end && self.loops() {
            start + (tick - end) % (end - start)
        } else {
            tick.clamp(start, end)
        }
    }
    /// Pause/end retains the boundary as the resumed position.
    pub fn pause_at(&mut self, tick: f64) {
        self.playing = false;
        self.position = tick;
    }

    fn song_end(&self, plan: &Plan) -> u32 {
        match self.region {
            Some(range) => range.end,
            None => plan.song_end,
        }
    }

    /// The transport's pattern, or the first one when that id is gone.
    pub(crate) fn pattern<'a>(&self, plan: &'a Plan) -> Option<&'a PlanPattern> {
        plan.pattern_ids
            .get(self.pattern.0)
            .map(|index| &plan.patterns[index])
            .or(plan.patterns.first())
    }

    fn loops(&self) -> bool {
        match self.passes_left {
            Some(passes) => passes > 1,
            None => self.mode == PlayMode::Pattern || self.loop_song,
        }
    }

    /// Decides at the end of a pass whether to go around again.
    fn another_pass(&mut self) -> bool {
        let again = self.loops();
        if let Some(passes) = &mut self.passes_left {
            *passes = passes.saturating_sub(1);
        }
        again
    }

    /// Restarts the clock on frame `now`, reading `tick` at the top of a
    /// pass.
    fn jump(&mut self, tick: f64, now: u64) {
        self.jumped = true;
        self.shift += tick - self.clock.tick_at(now);
        self.clock.anchor_frame = now;
        self.clock.anchor_tick = tick;
        self.pass_start = 0.0;
        self.floor = Floor::Tick(tick);
    }

    /// Playback ran out by itself. Voices are left to ring, and the clock
    /// runs on for them.
    fn finish(&mut self) {
        self.playing = false;
        self.passes_left = None;
        self.position = 0.0;
    }

    /// Brings the playhead back inside the pattern or song when a jump or a
    /// change to the plan left it past the end.
    fn keep_in_range(&mut self, plan: &Plan, now: u64) {
        if !self.playing {
            return;
        }
        if self.mode == PlayMode::Song
            && let Some(range) = self.region
        {
            let tick = self.raw_song_tick(plan, now);
            if tick < f64::from(range.start) {
                self.jump(
                    plan.warp(crate::timeline::aligned_tick(
                        plan,
                        f64::from(range.start),
                        self.sample_rate,
                    )),
                    now,
                );
                self.floor = Floor::Tick(plan.warp(f64::from(range.start)));
            } else if tick >= f64::from(range.end) {
                if self.loops() {
                    self.jump(
                        plan.warp(crate::timeline::aligned_tick(
                            plan,
                            f64::from(range.start),
                            self.sample_rate,
                        )),
                        now,
                    );
                    self.floor = Floor::Tick(plan.warp(f64::from(range.start)));
                } else {
                    self.pause_at(f64::from(range.end));
                }
            }
            return;
        }
        let length = self.length(plan);
        if length <= 0.0 {
            self.finish();
            return;
        }
        // Reaching the end exactly on frame `now` is an ordinary wrap, and
        // `advance` handles it. Only a playhead that is already past the end
        // needs moving.
        let end = self.pass_start + length;
        let jumped_past = matches!(self.floor, Floor::Tick(tick) if tick >= end);
        if jumped_past || self.clock.frame_of(end) < now {
            if self.loops() {
                let tick = (self.clock.tick_at(now) - self.pass_start) % length;
                self.jump(tick, now);
            } else {
                self.finish();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use windfall_project::{
        Channel, ChannelId, ChannelSource, Clip, ClipContent, ClipId, Lane, Note, NoteId,
        PlaylistTrack, PlaylistTrackId, Project, SamplerSettings, TrackId,
    };

    use super::*;
    use crate::plan::compile;
    use crate::pool::SamplePool;

    /// A project whose first pattern is `steps` steps long and holds one
    /// note per entry of `notes`: the tick it starts on and its key.
    fn project_with(steps: u32, notes: &[(u32, u8)]) -> Project {
        let mut project = Project::new("t");
        project.channels.push(Channel {
            sample_playback: None,
            id: ChannelId(10),
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
            mixer_track: TrackId::MASTER,
            source: ChannelSource::Sampler(SamplerSettings::default()),
        });
        project.patterns[0].length_steps = steps;
        project.patterns[0].lanes.push(Lane {
            channel: ChannelId(10),
            notes: notes
                .iter()
                .enumerate()
                .map(|(index, &(start, key))| Note {
                    id: NoteId(100 + index as u32),
                    start,
                    length: 60,
                    key,
                    velocity: 1.0,
                    pan: 0.0,
                    expression: Default::default(),
                })
                .collect(),
        });
        project
    }

    /// Plays frames `0..frames` in blocks of `block` with a trigger buffer
    /// of `capacity` notes, the way the processor does, and returns the
    /// frame and key of every note in the order they were handed over.
    fn played(
        mut sequencer: Sequencer,
        plan: &Plan,
        frames: u64,
        block: u64,
        capacity: usize,
    ) -> Vec<(u64, u8)> {
        let mut triggers = Triggers::with_capacity(capacity);
        let mut notes = Vec::new();
        let mut base = 0;
        while base < frames {
            let end = (base + block).min(frames);
            let mut cursor = Cursor::at(base);
            loop {
                let next = sequencer.collect(plan, cursor, end, &mut triggers);
                assert!(triggers.len() <= capacity);
                for index in 0..triggers.len() {
                    let trigger = triggers.get(index);
                    assert!(trigger.frame >= cursor.frame && trigger.frame < end);
                    let Fire::Note { key, .. } = trigger.what else {
                        panic!("{trigger:?} is not a note");
                    };
                    notes.push((trigger.frame, key));
                }
                let Some(next) = next else {
                    break;
                };
                sequencer.advance(plan, cursor.frame, next.frame);
                cursor = next;
            }
            sequencer.advance(plan, cursor.frame, end);
            base = end;
        }
        notes
    }

    #[test]
    fn a_note_exactly_on_a_frame_plays_on_that_frame() {
        // At 69 bpm and 44.1 kHz, step 23 falls exactly on frame 220500, but
        // the floating-point product comes out a hair above it.
        let clock = Clock {
            anchor_frame: 0,
            anchor_tick: 0.0,
            frames_per_tick: samples_per_tick(69.0, 44_100.0),
        };
        assert!(23.0 * 240.0 * clock.frames_per_tick > 220_500.0);
        assert_eq!(clock.frame_of(23.0 * 240.0), 220_500);
        assert_eq!(clock.frame_of(0.0), 0);
        assert_eq!(clock.frame_of(-5.0), 0);
    }

    #[test]
    fn a_tick_that_never_comes_has_no_frame() {
        let clock = Clock {
            anchor_frame: 1_000,
            anchor_tick: 0.0,
            frames_per_tick: 25.0,
        };
        assert_eq!(clock.frame_of(f64::INFINITY), u64::MAX);
        assert_eq!(clock.frame_of(f64::NEG_INFINITY), 1_000);
    }

    #[test]
    fn stop_returns_to_where_playback_started() {
        let plan = compile(&Project::new("t"), &SamplePool::new());
        let mut sequencer = Sequencer::new(48_000, &plan);
        sequencer.seek(480.0, &plan, 0);
        sequencer.play(&plan, 0, None, None);
        sequencer.advance(&plan, 0, 10_000);
        assert!(sequencer.tick(&plan, 10_000) > 480.0);
        sequencer.stop();
        assert_eq!(sequencer.tick(&plan, 10_000), 480.0);
    }

    #[test]
    fn playing_from_elsewhere_keeps_the_place_stop_returns_to() {
        let plan = compile(&Project::new("t"), &SamplePool::new());
        let mut sequencer = Sequencer::new(48_000, &plan);
        sequencer.seek(480.0, &plan, 0);
        sequencer.play(&plan, 0, None, Some(1_000.0));
        assert_eq!(sequencer.tick(&plan, 0), 1_000.0);
        assert_eq!(sequencer.start(), 480.0);
        sequencer.stop();
        assert_eq!(sequencer.tick(&plan, 0), 480.0);
    }

    #[test]
    fn playing_an_empty_song_stops_at_once() {
        let plan = compile(&Project::new("t"), &SamplePool::new());
        let mut sequencer = Sequencer::new(48_000, &plan);
        sequencer.set_transport(PlayMode::Song, PatternId(1), true, &plan, 0);
        sequencer.play(&plan, 0, None, None);
        assert!(!sequencer.playing());
    }

    #[test]
    fn asking_for_notes_does_not_move_the_transport() {
        let plan = compile(&project_with(4, &[(0, 60), (900, 61)]), &SamplePool::new());
        let mut sequencer = Sequencer::new(48_000, &plan);
        sequencer.play(&plan, 0, None, None);
        let mut triggers = Triggers::new();
        // Two and a half times around the 24000-frame pattern.
        for _ in 0..2 {
            assert_eq!(
                sequencer.collect(&plan, Cursor::at(0), 60_000, &mut triggers),
                None
            );
            let frames: Vec<u64> = (0..triggers.len())
                .map(|index| triggers.get(index).frame)
                .collect();
            assert_eq!(frames, [0, 22_500, 24_000, 46_500, 48_000]);
        }
        assert_eq!(sequencer.tick(&plan, 0), 0.0);
    }

    #[test]
    fn more_notes_than_the_buffer_holds_come_in_rounds_and_none_is_lost() {
        // Crowds on the first frame, on a later frame, on the last tick of
        // the pattern and on the first tick, which share a frame at the
        // loop point, and a lone note after each crowd.
        let mut notes = Vec::new();
        for key in 0..9 {
            notes.push((0, key));
            notes.push((100, 20 + key));
            notes.push((2_159, 40 + key));
        }
        notes.extend([(40, 100), (130, 101), (500, 102)]);
        let mut project = project_with(9, &notes);
        project.settings.tempo_bpm = 522.0;
        let plan = compile(&project, &SamplePool::new());
        let mut sequencer = Sequencer::new(8_000, &plan);
        sequencer.play(&plan, 0, None, None);

        // At 8 kHz and 522 bpm a tick is under a frame. The pattern is 2069
        // frames long, so this is three passes and the first 293 frames of
        // a fourth, which hold everything up to tick 130.
        let frames = 6_500;
        let reference = played(sequencer, &plan, frames, frames, 4_096);
        assert_eq!(reference.len(), 30 * 3 + 20);
        assert!(reference.is_sorted_by_key(|note| note.0));
        // Each pass ends on the frame the next begins on, and the old
        // pass's notes come first.
        let wrap = reference.iter().position(|note| note.1 == 40).unwrap();
        assert_eq!(reference[wrap].0, reference[wrap + 9].0);
        assert_eq!(reference[wrap + 9].1, 0);

        for capacity in [1, 2, 3, 4, 7, 9, 10, 26] {
            for block in [1, 7, 64, 256, 6_500] {
                assert_eq!(
                    played(sequencer, &plan, frames, block, capacity),
                    reference,
                    "a buffer of {capacity} notes in blocks of {block}"
                );
            }
        }
    }

    #[test]
    fn notes_found_out_of_order_come_in_rounds_too() {
        // In song mode the notes of a clip that starts later are found
        // after the notes of every clip before it, however early they play.
        let mut project = project_with(4, &[(0, 60), (0, 61), (240, 62), (480, 63), (720, 64)]);
        project.settings.tempo_bpm = 300.0;
        let pattern = project.patterns[0].id;
        project.playlist.tracks.push(PlaylistTrack {
            id: PlaylistTrackId(50),
            name: String::new(),
            muted: false,
            solo: false,
            color: 0,
            height: 0,
        });
        let clip = |id: u32, start: u32, offset: u32| Clip {
            id: ClipId(id),
            track: PlaylistTrackId(50),
            start,
            length: 1_920,
            offset,
            muted: false,
            content: ClipContent::Pattern { pattern },
        };
        project.playlist.clips = vec![
            clip(60, 0, 0),
            clip(61, 0, 480),
            clip(62, 240, 0),
            clip(63, 241, 720),
            clip(64, 480, 0),
        ];
        let plan = compile(&project, &SamplePool::new());
        let mut sequencer = Sequencer::new(8_000, &plan);
        sequencer.set_transport(PlayMode::Song, pattern, true, &plan, 0);
        sequencer.play(&plan, 0, None, None);

        let frames = 9_000;
        let reference = played(sequencer, &plan, frames, frames, 4_096);
        assert!(reference.len() > 60);
        assert!(reference.is_sorted_by_key(|note| note.0));
        for capacity in [1, 2, 3, 5, 8] {
            for block in [1, 64, 256, 9_000] {
                assert_eq!(
                    played(sequencer, &plan, frames, block, capacity),
                    reference,
                    "a buffer of {capacity} notes in blocks of {block}"
                );
            }
        }
    }

    #[test]
    fn a_tempo_change_moves_the_end_of_a_note_and_keeps_its_tick() {
        let plan = compile(&project_with(4, &[(0, 60)]), &SamplePool::new());
        let mut sequencer = Sequencer::new(48_000, &plan);
        sequencer.play(&plan, 0, None, None);
        let mut triggers = Triggers::new();
        sequencer.collect(&plan, Cursor::at(0), 100, &mut triggers);
        let Fire::Note { end, .. } = triggers.get(0).what else {
            panic!("not a note");
        };
        // 60 ticks at 25 frames a tick.
        assert_eq!(sequencer.clock().frame_of(end), 1_500);

        // Half way through the note the tempo halves: the 30 ticks left
        // take 50 frames each.
        sequencer.advance(&plan, 0, 750);
        let mut slow = project_with(4, &[(0, 60)]);
        slow.settings.tempo_bpm = 60.0;
        sequencer.set_plan(&compile(&slow, &SamplePool::new()), &plan, 750);
        assert_eq!(sequencer.clock().frame_of(end), 750 + 1_500);
        assert_eq!(sequencer.take_clock_shift(), 0.0);
    }

    #[test]
    fn a_jump_reports_how_far_it_moved_the_clock() {
        let plan = compile(&project_with(4, &[(0, 60)]), &SamplePool::new());
        let mut sequencer = Sequencer::new(48_000, &plan);
        sequencer.play(&plan, 0, None, None);
        assert_eq!(sequencer.take_clock_shift(), 0.0);
        // Frame 2500 is tick 100, and the seek makes it tick 700.
        sequencer.advance(&plan, 0, 2_500);
        sequencer.seek(700.0, &plan, 2_500);
        assert_eq!(sequencer.take_clock_shift(), 600.0);
        assert_eq!(sequencer.take_clock_shift(), 0.0);
    }
}
