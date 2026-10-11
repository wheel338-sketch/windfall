//! What the controller hub captures, as pure values with the time passed
//! in: where a moment falls on the playhead, the notes of a recording pass,
//! the rolling log of everything played, and the movements of a pass of
//! automation recording. Nothing here locks, sends or reads a clock.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use windfall_engine::Playhead;
use windfall_ipc::{PlayMode, ScoreLogState};
use windfall_project::{
    AutomationPoint, AutomationTarget, ChannelId, MAX_AUTOMATION_POINTS, MAX_PATTERN_TICKS,
    MAX_SONG_TICKS, NoteInit, PatternId, curve_value,
};

/// Notes one recording pass keeps at most.
pub(super) const MAX_TAKE_NOTES: usize = 4096;
/// Finished notes the rolling log keeps at most.
pub(super) const SCORE_LOG_NOTES: usize = 16_384;
/// How far back the rolling log reaches.
pub(super) const SCORE_LOG_SPAN: Duration = Duration::from_secs(30 * 60);
/// Keys that can be down in the log at once.
const SCORE_LOG_OPEN: usize = 256;
/// Controls one pass of automation recording follows at most.
pub(super) const MAX_AUTOMATION_LANES: usize = 64;
/// Movements one control keeps in a pass at most.
pub(super) const MAX_LANE_SAMPLES: usize = 8192;
/// A recorded movement smaller than this, of the control's whole range, is
/// not worth a point.
const POINT_TOLERANCE: f32 = 0.002;

/// A place on the playhead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Position {
    /// Within the pattern in pattern mode, within the song in song mode.
    pub tick: f64,
    pub mode: PlayMode,
    pub pattern: PatternId,
    /// The length the pattern loops at, in pattern mode.
    pub pattern_ticks: Option<u32>,
}

/// Follows the playhead the engine publishes, so that any moment near now
/// can be given its tick. The engine publishes once a buffer; between two
/// readings the playhead is taken to move at the tempo.
#[derive(Debug, Clone, Default)]
pub(super) struct TransportClock {
    reading: Option<(Playhead, Instant)>,
    /// When playback was first seen running.
    started: Option<Instant>,
    /// The pattern has come round at least once since then.
    wrapped: bool,
}

impl TransportClock {
    /// Takes a reading of the transport at `now`. Returns true when
    /// playback has just stopped.
    pub fn observe(&mut self, playhead: &Playhead, now: Instant) -> bool {
        let was_playing = self.playing();
        if !playhead.playing {
            self.reading = Some((*playhead, now));
            self.started = None;
            self.wrapped = false;
            return was_playing;
        }
        match &mut self.reading {
            Some((held, _)) if held.playing && held.tick == playhead.tick => {
                // The same buffer: keep the moment it was first seen.
                *held = *playhead;
            }
            Some((held, at)) if held.playing => {
                if playhead.tick < held.tick {
                    self.wrapped = true;
                }
                *held = *playhead;
                *at = now;
            }
            _ => {
                self.reading = Some((*playhead, now));
                self.started = Some(now);
                self.wrapped = false;
            }
        }
        false
    }

    pub fn playing(&self) -> bool {
        self.reading.is_some_and(|(playhead, _)| playhead.playing)
    }

    /// Where the playhead was at `moment`, moved by `offset_seconds`.
    /// `None` while stopped and for a moment before playback began.
    pub fn position_at(&self, moment: Instant, offset_seconds: f64) -> Option<Position> {
        let (playhead, at) = self.reading?;
        let started = self.started?;
        if !playhead.playing || moment < started {
            return None;
        }
        let since = if moment >= at {
            moment.duration_since(at).as_secs_f64()
        } else {
            -at.duration_since(moment).as_secs_f64()
        };
        let rate = if playhead.ticks_per_second.is_finite() {
            playhead.ticks_per_second.max(0.0)
        } else {
            0.0
        };
        let mut tick = playhead.tick + (since + offset_seconds) * rate;
        let looped = playhead
            .pattern_ticks
            .filter(|length| playhead.mode == PlayMode::Pattern && *length > 0)
            .map(f64::from);
        match looped {
            Some(length) if tick >= length || (tick < 0.0 && self.wrapped) => {
                tick = tick.rem_euclid(length);
            }
            _ => tick = tick.max(0.0),
        }
        Some(Position {
            tick,
            mode: playhead.mode,
            pattern: playhead.pattern,
            pattern_ticks: playhead.pattern_ticks,
        })
    }
}

/// One finished note of a recording pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TakenNote {
    pub channel: ChannelId,
    pub pattern: PatternId,
    pub mode: PlayMode,
    pub start: f64,
    pub length: f64,
    pub key: u8,
    pub velocity: f32,
}

#[derive(Debug, Clone, Copy)]
struct OpenNote {
    channel: ChannelId,
    key: u8,
    velocity: f32,
    at: Position,
}

/// The notes played during one pass of note recording.
#[derive(Debug, Default)]
pub(super) struct NoteTake {
    open: Vec<OpenNote>,
    done: Vec<TakenNote>,
    dropped: u32,
}

impl NoteTake {
    pub fn pending(&self) -> u32 {
        (self.open.len() + self.done.len()) as u32
    }

    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.open.is_empty() && self.done.is_empty()
    }

    /// A key going down at `at`. A key already down is ended there first.
    pub fn note_on(&mut self, channel: ChannelId, key: u8, velocity: f32, at: Position) {
        self.note_off(channel, key, at);
        if self.open.len() + self.done.len() >= MAX_TAKE_NOTES {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.open.push(OpenNote {
            channel,
            key,
            velocity: velocity.clamp(0.0, 1.0),
            at,
        });
    }

    /// A key coming up at `at`.
    pub fn note_off(&mut self, channel: ChannelId, key: u8, at: Position) {
        let Some(index) = self
            .open
            .iter()
            .position(|note| note.channel == channel && note.key == key)
        else {
            return;
        };
        let note = self.open.swap_remove(index);
        self.finish(note, at.tick);
    }

    /// Ends every key still down at `tick`, as when playback stops.
    pub fn close_all(&mut self, tick: f64) {
        for note in std::mem::take(&mut self.open) {
            self.finish(note, tick);
        }
    }

    fn finish(&mut self, note: OpenNote, end: f64) {
        let start = note.at.tick;
        let length = match note.at.pattern_ticks.map(f64::from) {
            // The pattern came round under a held key: the note is kept up
            // to the end of the pass it began in.
            Some(pattern) if note.at.mode == PlayMode::Pattern && end < start => pattern - start,
            _ => end - start,
        };
        self.done.push(TakenNote {
            channel: note.channel,
            pattern: note.at.pattern,
            mode: note.at.mode,
            start,
            length: length.max(1.0),
            key: note.key,
            velocity: note.velocity,
        });
    }

    /// Hands over the finished notes in the order they began and starts an
    /// empty pass. Keys still down stay down.
    pub fn take(&mut self) -> Vec<TakenNote> {
        self.dropped = 0;
        let mut notes = std::mem::take(&mut self.done);
        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
        notes
    }
}

/// A note as a command takes it: its start snapped to `quantize` ticks when
/// that is not 0, and moved by `shift` ticks. `wrap` is the pattern length
/// a snapped start is folded back into. `None` when the note would start
/// past the longest pattern.
pub(super) fn note_init(
    note: &TakenNote,
    shift: f64,
    quantize: u32,
    wrap: Option<u32>,
) -> Option<NoteInit> {
    let mut start = (note.start + shift).max(0.0);
    if quantize > 0 {
        let grid = f64::from(quantize);
        start = (start / grid).round() * grid;
    }
    if let Some(length) = wrap.filter(|length| *length > 0)
        && start >= f64::from(length)
    {
        start = start.rem_euclid(f64::from(length));
    }
    let start = start.round();
    if !start.is_finite() || start >= f64::from(MAX_PATTERN_TICKS) {
        return None;
    }
    let length = note.length.round().clamp(1.0, f64::from(MAX_PATTERN_TICKS));
    Some(NoteInit {
        start: start as u32,
        length: length as u32,
        key: note.key.min(127),
        velocity: Some(note.velocity.clamp(0.0, 1.0)),
        pan: None,
        expression: None,
    })
}

#[derive(Debug, Clone, Copy)]
struct LoggedNote {
    channel: ChannelId,
    key: u8,
    velocity: f32,
    on: Instant,
    off: Instant,
    /// The playhead when the key went down, if playback was running.
    tick: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
struct OpenLogged {
    channel: ChannelId,
    key: u8,
    velocity: f32,
    on: Instant,
    tick: Option<f64>,
}

/// What a dump of the log puts into a pattern.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ScoreDump {
    /// Notes for each channel, in the order the channels were first played.
    pub lanes: Vec<(ChannelId, Vec<NoteInit>)>,
    /// Notes left out because they would start past the longest pattern.
    pub skipped: u32,
}

/// The rolling log of everything played: a bounded history that is always
/// on. Dumping it does not empty it, so undoing a dump loses nothing.
#[derive(Debug, Default)]
pub(super) struct ScoreLog {
    notes: VecDeque<LoggedNote>,
    open: Vec<OpenLogged>,
}

impl ScoreLog {
    pub fn note_on(
        &mut self,
        channel: ChannelId,
        key: u8,
        velocity: f32,
        at: Instant,
        tick: Option<f64>,
    ) {
        // A key struck again while down ends its first note there.
        self.note_off(channel, key, at);
        if self.open.len() >= SCORE_LOG_OPEN {
            self.open.remove(0);
        }
        self.open.push(OpenLogged {
            channel,
            key,
            velocity: velocity.clamp(0.0, 1.0),
            on: at,
            tick,
        });
    }

    pub fn note_off(&mut self, channel: ChannelId, key: u8, at: Instant) {
        let Some(index) = self
            .open
            .iter()
            .position(|note| note.channel == channel && note.key == key)
        else {
            return;
        };
        let note = self.open.remove(index);
        if self.notes.len() >= SCORE_LOG_NOTES {
            self.notes.pop_front();
        }
        self.notes.push_back(LoggedNote {
            channel: note.channel,
            key: note.key,
            velocity: note.velocity,
            on: note.on,
            off: at.max(note.on),
            tick: note.tick,
        });
    }

    /// Forgets what is older than the log reaches.
    pub fn prune(&mut self, now: Instant) {
        let old = |on: Instant| now.saturating_duration_since(on) > SCORE_LOG_SPAN;
        while self.notes.front().is_some_and(|note| old(note.on)) {
            self.notes.pop_front();
        }
        self.open.retain(|note| !old(note.on));
    }

    pub fn clear(&mut self) {
        self.notes.clear();
        self.open.clear();
    }

    pub fn state(&self, now: Instant) -> ScoreLogState {
        ScoreLogState {
            notes: self.notes.len() as u32,
            seconds: self.notes.front().map_or(0.0, |note| {
                now.saturating_duration_since(note.on).as_secs_f32()
            }),
        }
    }

    /// The finished notes that began within `window` before `now`, or all
    /// of them with no window, laid out at `tempo_bpm`. The first note
    /// lands where it was in its bar if playback was running when it was
    /// played, and on the first tick otherwise. `quantize` snaps starts to
    /// a grid of that many ticks when it is not 0.
    pub fn dump(
        &self,
        now: Instant,
        window: Option<Duration>,
        tempo_bpm: f64,
        bar_ticks: u32,
        quantize: u32,
    ) -> ScoreDump {
        let wanted = |note: &&LoggedNote| {
            window.is_none_or(|window| now.saturating_duration_since(note.on) <= window)
        };
        let mut notes: Vec<&LoggedNote> = self.notes.iter().filter(wanted).collect();
        notes.sort_by_key(|note| note.on);
        let mut dump = ScoreDump {
            lanes: Vec::new(),
            skipped: 0,
        };
        let Some(first) = notes.first() else {
            return dump;
        };
        let rate = tempo_bpm.max(1.0) * f64::from(windfall_core::PPQ) / 60.0;
        let origin = first.on;
        let anchor = first
            .tick
            .filter(|_| bar_ticks > 0)
            .map_or(0.0, |tick| tick.rem_euclid(f64::from(bar_ticks)));
        for note in notes {
            let start = anchor + note.on.duration_since(origin).as_secs_f64() * rate;
            let taken = TakenNote {
                channel: note.channel,
                pattern: PatternId(0),
                mode: PlayMode::Pattern,
                start,
                length: note.off.duration_since(note.on).as_secs_f64() * rate,
                key: note.key,
                velocity: note.velocity,
            };
            let Some(init) = note_init(&taken, 0.0, quantize, None) else {
                dump.skipped += 1;
                continue;
            };
            match dump
                .lanes
                .iter_mut()
                .find(|(channel, _)| *channel == note.channel)
            {
                Some((_, lane)) => lane.push(init),
                None => dump.lanes.push((note.channel, vec![init])),
            }
        }
        dump
    }
}

/// The movements of one control in a pass of automation recording.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct AutomationLane {
    pub target: AutomationTarget,
    /// The control's value, 0 to 1, before its first movement of the pass.
    pub before: f32,
    /// Song tick and value, 0 to 1, in the order they were made.
    pub samples: Vec<(f64, f32)>,
}

/// One pass of automation recording.
#[derive(Debug, Default)]
pub(super) struct AutomationTake {
    lanes: Vec<AutomationLane>,
    dropped: u32,
}

impl AutomationTake {
    pub fn targets(&self) -> u32 {
        self.lanes.len() as u32
    }

    pub fn pending(&self) -> u32 {
        self.lanes
            .iter()
            .map(|lane| lane.samples.len() as u32)
            .sum()
    }

    #[cfg(test)]
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }

    /// A control moved to `value` at song tick `tick`. `before` is what it
    /// held just before, which matters for its first movement only.
    pub fn record(&mut self, target: AutomationTarget, before: f32, tick: f64, value: f32) {
        if !tick.is_finite() || !value.is_finite() {
            return;
        }
        let value = value.clamp(0.0, 1.0);
        let index = match self.lanes.iter().position(|lane| lane.target == target) {
            Some(index) => index,
            None if self.lanes.len() >= MAX_AUTOMATION_LANES => {
                self.dropped = self.dropped.saturating_add(1);
                return;
            }
            None => {
                self.lanes.push(AutomationLane {
                    target,
                    before: if before.is_finite() {
                        before.clamp(0.0, 1.0)
                    } else {
                        value
                    },
                    samples: Vec::new(),
                });
                self.lanes.len() - 1
            }
        };
        let samples = &mut self.lanes[index].samples;
        if samples.len() >= MAX_LANE_SAMPLES {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        samples.push((tick, value));
    }

    pub fn take(&mut self) -> Vec<AutomationLane> {
        self.dropped = 0;
        std::mem::take(&mut self.lanes)
    }
}

fn point(tick: u32, value: f32) -> AutomationPoint {
    AutomationPoint {
        tick,
        value,
        curve: 0.0,
        hold: false,
    }
}

/// Recorded movements as curve points: on whole ticks in order, the last
/// movement on a tick winning, and without the ones that change nothing.
/// Movements after the playhead went back (a loop) replace what the earlier
/// pass recorded from there on. At most `budget` points come back.
pub(super) fn recorded_points(samples: &[(f64, f32)], budget: usize) -> Vec<AutomationPoint> {
    let mut placed: Vec<AutomationPoint> = Vec::with_capacity(samples.len());
    for &(tick, value) in samples {
        if !tick.is_finite() || tick < 0.0 || tick > f64::from(MAX_SONG_TICKS) {
            continue;
        }
        let tick = tick.round() as u32;
        // A later pass over the same stretch overwrites the earlier one.
        while placed.last().is_some_and(|last| last.tick >= tick) {
            placed.pop();
        }
        placed.push(point(tick, value.clamp(0.0, 1.0)));
    }
    let mut tolerance = POINT_TOLERANCE;
    loop {
        let mut thinned: Vec<AutomationPoint> = Vec::with_capacity(placed.len());
        // The last movement left out since a point was kept. It goes in
        // ahead of the next kept point, so a control that sat still and then
        // moved reads as that and not as one long ramp.
        let mut idle: Option<AutomationPoint> = None;
        for (index, next) in placed.iter().enumerate() {
            let ends = index == 0 || index + 1 == placed.len();
            let moved = thinned
                .last()
                .is_none_or(|kept| (kept.value - next.value).abs() > tolerance);
            if ends || moved {
                thinned.extend(idle.take());
                thinned.push(*next);
            } else {
                idle = Some(*next);
            }
        }
        if thinned.len() <= budget.max(2) || tolerance >= 1.0 {
            thinned.truncate(budget.max(2));
            return thinned;
        }
        tolerance *= 2.0;
    }
}

/// A curve with a recorded stretch written into it. Outside the stretch the
/// curve reads exactly as before: it jumps to the recording where that
/// begins and, when `restore` is set, back to itself where it ends. Without
/// `restore` the last recorded value carries on to the curve's next point.
pub(super) fn merge_points(
    existing: &[AutomationPoint],
    recorded: &[AutomationPoint],
    restore: bool,
) -> Vec<AutomationPoint> {
    let (Some(first), Some(last)) = (recorded.first(), recorded.last()) else {
        return existing.to_vec();
    };
    let before: Vec<AutomationPoint> = existing
        .iter()
        .copied()
        .filter(|point| point.tick < first.tick)
        .collect();
    let after: Vec<AutomationPoint> = existing
        .iter()
        .copied()
        .filter(|point| point.tick > last.tick)
        .collect();
    // What can be spent on the recording once the curve's own points and
    // the two seams are counted.
    let room = MAX_AUTOMATION_POINTS.saturating_sub(before.len() + after.len() + 2);
    let recorded = if recorded.len() > room {
        recorded_points(
            &recorded
                .iter()
                .map(|point| (f64::from(point.tick), point.value))
                .collect::<Vec<_>>(),
            room,
        )
    } else {
        recorded.to_vec()
    };
    let mut merged = before;
    if !existing.is_empty() {
        let was = curve_value(existing, f64::from(first.tick));
        if merged.last().is_some_and(|point| !point.hold)
            && (was - first.value).abs() > f32::EPSILON
        {
            // Hold the curve's own value up to the recording, so the ramp
            // into it stays as it was.
            merged.push(point(first.tick, was));
        }
    }
    merged.extend(recorded.iter().copied());
    if restore && !existing.is_empty() {
        let was = curve_value(existing, f64::from(last.tick));
        if (was - last.value).abs() > f32::EPSILON {
            merged.push(point(last.tick, was));
        }
    }
    merged.extend(after);
    merged.truncate(MAX_AUTOMATION_POINTS);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playhead(playing: bool, tick: f64) -> Playhead {
        Playhead {
            playing,
            tick,
            ticks_per_second: 1_920.0,
            mode: PlayMode::Pattern,
            pattern: PatternId(1),
            pattern_ticks: Some(3_840),
            loop_song: false,
        }
    }

    fn at(tick: f64) -> Position {
        Position {
            tick,
            mode: PlayMode::Pattern,
            pattern: PatternId(1),
            pattern_ticks: Some(3_840),
        }
    }

    const KICK: ChannelId = ChannelId(7);

    #[test]
    fn a_moment_gets_the_tick_the_playhead_had_then() {
        let start = Instant::now();
        let ms = Duration::from_millis;
        let mut clock = TransportClock::default();
        assert!(!clock.observe(&playhead(false, 0.0), start));
        assert_eq!(clock.position_at(start, 0.0), None);
        // Playback is first seen 10 ms in, a buffer after it began.
        assert!(!clock.observe(&playhead(true, 19.2), start + ms(10)));
        assert!(clock.playing());
        let tick = |clock: &TransportClock, moment, offset| {
            clock.position_at(moment, offset).unwrap().tick
        };
        assert!((tick(&clock, start + ms(10), 0.0) - 19.2).abs() < 1e-9);
        // 5 ms on at 1920 ticks a second.
        assert!((tick(&clock, start + ms(15), 0.0) - 28.8).abs() < 1e-9);
        // The same buffer seen again keeps its first moment.
        clock.observe(&playhead(true, 19.2), start + ms(14));
        assert!((tick(&clock, start + ms(15), 0.0) - 28.8).abs() < 1e-9);
        // A new buffer re-anchors; a moment before it reads back from it.
        clock.observe(&playhead(true, 38.4), start + ms(20));
        assert!((tick(&clock, start + ms(18), 0.0) - 34.56).abs() < 1e-9);
        // A calibration offset moves the answer by its share of the tempo.
        assert!((tick(&clock, start + ms(20), -0.005) - 28.8).abs() < 1e-9);
        // Before playback was seen there is no place, and never a tick
        // from the end of a pattern that has not come round yet.
        assert_eq!(clock.position_at(start + ms(5), 0.0), None);
        assert_eq!(tick(&clock, start + ms(10), -1.0), 0.0);
    }

    #[test]
    fn a_looping_pattern_folds_ticks_and_a_stop_is_reported_once() {
        let start = Instant::now();
        let ms = Duration::from_millis;
        let mut clock = TransportClock::default();
        clock.observe(&playhead(true, 3_830.0), start);
        // 10 ms on is 19.2 ticks: past the end, folded to the start.
        let folded = clock.position_at(start + ms(10), 0.0).unwrap();
        assert!((folded.tick - 9.2).abs() < 1e-9);
        assert_eq!(folded.pattern_ticks, Some(3_840));
        // After the pattern came round, a moment just before the reading
        // belongs to the end of the pass before.
        clock.observe(&playhead(true, 5.0), start + ms(8));
        let back = clock.position_at(start + ms(3), 0.0).unwrap();
        assert!((back.tick - (3_840.0 + 5.0 - 9.6)).abs() < 1e-9);
        assert!(clock.observe(&playhead(false, 0.0), start + ms(20)));
        assert!(!clock.observe(&playhead(false, 0.0), start + ms(30)));
        assert!(!clock.playing());
        assert_eq!(clock.position_at(start + ms(30), 0.0), None);
        // In song mode nothing folds.
        let mut song = TransportClock::default();
        let reading = Playhead {
            mode: PlayMode::Song,
            ..playhead(true, 3_830.0)
        };
        song.observe(&reading, start);
        let on = song.position_at(start + ms(10), 0.0).unwrap();
        assert!((on.tick - 3_849.2).abs() < 1e-9);
    }

    #[test]
    fn a_pass_pairs_keys_and_keeps_what_was_played() {
        let mut take = NoteTake::default();
        assert!(take.is_empty());
        take.note_on(KICK, 60, 0.8, at(100.0));
        take.note_on(KICK, 64, 2.0, at(150.0));
        take.note_off(KICK, 60, at(340.0));
        // A release with no key down, and one for another channel, do nothing.
        take.note_off(KICK, 61, at(350.0));
        take.note_off(ChannelId(8), 64, at(350.0));
        assert_eq!(take.pending(), 2);
        // Struck again while down: the first note ends where the second begins.
        take.note_on(KICK, 64, 0.5, at(400.0));
        take.close_all(900.0);
        let notes = take.take();
        let summary: Vec<(u8, f64, f64, f32)> = notes
            .iter()
            .map(|note| (note.key, note.start, note.length, note.velocity))
            .collect();
        assert_eq!(
            summary,
            [
                (60, 100.0, 240.0, 0.8),
                (64, 150.0, 250.0, 1.0),
                (64, 400.0, 500.0, 0.5),
            ]
        );
        assert!(take.is_empty());
        assert!(
            notes
                .iter()
                .all(|note| note.channel == KICK && note.pattern == PatternId(1))
        );
    }

    #[test]
    fn a_key_held_across_the_loop_ends_with_its_pass_and_a_full_pass_counts_drops() {
        let mut take = NoteTake::default();
        take.note_on(KICK, 60, 1.0, at(3_800.0));
        take.note_off(KICK, 60, at(50.0));
        // Down and up on one tick is still a note.
        take.note_on(KICK, 61, 1.0, at(10.0));
        take.note_off(KICK, 61, at(10.0));
        let notes = take.take();
        assert_eq!((notes[0].key, notes[0].length), (61, 1.0));
        assert_eq!(
            (notes[1].key, notes[1].start, notes[1].length),
            (60, 3_800.0, 40.0)
        );

        let mut take = NoteTake::default();
        for index in 0..MAX_TAKE_NOTES + 5 {
            take.note_on(KICK, 60, 1.0, at(index as f64));
        }
        take.close_all(1e6);
        assert_eq!(take.dropped(), 5);
        assert_eq!(take.take().len(), MAX_TAKE_NOTES);
        assert_eq!(take.dropped(), 0);
    }

    #[test]
    fn notes_are_shifted_snapped_folded_and_bounded_on_their_way_in() {
        let note = TakenNote {
            channel: KICK,
            pattern: PatternId(1),
            mode: PlayMode::Pattern,
            start: 250.4,
            length: 99.6,
            key: 60,
            velocity: 0.7,
        };
        let plain = note_init(&note, 0.0, 0, None).unwrap();
        assert_eq!((plain.start, plain.length, plain.key), (250, 100, 60));
        assert_eq!(plain.velocity, Some(0.7));
        // Snapped to sixteenths: the length is kept.
        let snapped = note_init(&note, 0.0, 240, None).unwrap();
        assert_eq!((snapped.start, snapped.length), (240, 100));
        // A shift earlier than the start stops at the first tick.
        assert_eq!(note_init(&note, -400.0, 0, None).unwrap().start, 0);
        assert_eq!(note_init(&note, -20.4, 0, None).unwrap().start, 230);
        // A late note snapped onto the end of the pattern is its first step.
        let late = TakenNote {
            start: 3_800.0,
            ..note
        };
        assert_eq!(note_init(&late, 0.0, 240, Some(3_840)).unwrap().start, 0);
        assert_eq!(note_init(&late, 0.0, 240, None).unwrap().start, 3_840);
        let far = TakenNote {
            start: f64::from(MAX_PATTERN_TICKS),
            ..note
        };
        assert_eq!(note_init(&far, 0.0, 0, None), None);
        let short = TakenNote {
            length: 0.2,
            ..note
        };
        assert_eq!(note_init(&short, 0.0, 0, None).unwrap().length, 1);
    }

    #[test]
    fn the_log_lays_played_notes_out_at_the_tempo_and_keeps_them_after_a_dump() {
        let start = Instant::now();
        let ms = Duration::from_millis;
        let mut log = ScoreLog::default();
        // At 120 bpm a quarter note is 500 ms and 960 ticks.
        log.note_on(KICK, 36, 1.0, start, None);
        log.note_off(KICK, 36, start + ms(250));
        log.note_on(ChannelId(9), 60, 0.5, start + ms(500), None);
        log.note_on(KICK, 36, 0.9, start + ms(1_000), None);
        log.note_off(KICK, 36, start + ms(1_125));
        log.note_off(ChannelId(9), 60, start + ms(1_500));
        // Still down: not in a dump.
        log.note_on(KICK, 40, 1.0, start + ms(1_600), None);
        let now = start + ms(2_000);
        assert_eq!(log.state(now).notes, 3);
        assert!((log.state(now).seconds - 2.0).abs() < 1e-3);

        let dump = log.dump(now, None, 120.0, 3_840, 0);
        assert_eq!(dump.skipped, 0);
        let lane = |channel: ChannelId| -> Vec<(u32, u32, u8)> {
            let (_, notes) = dump.lanes.iter().find(|(id, _)| *id == channel).unwrap();
            notes
                .iter()
                .map(|note| (note.start, note.length, note.key))
                .collect()
        };
        assert_eq!(lane(KICK), [(0, 480, 36), (1_920, 240, 36)]);
        assert_eq!(lane(ChannelId(9)), [(960, 1_920, 60)]);
        assert_eq!(
            dump.lanes[0].0, KICK,
            "channels come in the order first played"
        );
        // Dumping takes nothing out of the log.
        assert_eq!(log.dump(now, None, 120.0, 3_840, 0), dump);
        // A window keeps only what began inside it, and starts it at 0.
        let recent = log.dump(now, Some(ms(1_200)), 120.0, 3_840, 0);
        assert_eq!(recent.lanes.len(), 1);
        assert_eq!(recent.lanes[0].1[0].start, 0);
        // Another tempo, and a grid.
        let slow = log.dump(now, None, 60.0, 3_840, 0);
        assert_eq!(slow.lanes[0].1[1].start, 960);
        let snapped = log.dump(now + ms(0), None, 119.0, 3_840, 240);
        assert!(
            snapped
                .lanes
                .iter()
                .all(|(_, notes)| notes.iter().all(|note| note.start % 240 == 0))
        );
        log.clear();
        assert!(log.dump(now, None, 120.0, 3_840, 0).lanes.is_empty());
        assert_eq!(log.state(now), ScoreLogState::default());
    }

    #[test]
    fn the_log_keeps_the_bar_position_of_notes_played_along_and_is_bounded() {
        let start = Instant::now();
        let ms = Duration::from_millis;
        let mut log = ScoreLog::default();
        // Played while the song was at bar 3, beat 2.
        log.note_on(KICK, 36, 1.0, start, Some(2.0 * 3_840.0 + 960.0));
        log.note_off(KICK, 36, start + ms(100));
        let dump = log.dump(start + ms(200), None, 120.0, 3_840, 0);
        assert_eq!(dump.lanes[0].1[0].start, 960);

        let mut log = ScoreLog::default();
        for index in 0..SCORE_LOG_NOTES as u32 + 10 {
            let on = start + Duration::from_micros(u64::from(index) * 10);
            log.note_on(KICK, 60, 1.0, on, None);
            log.note_off(KICK, 60, on + Duration::from_micros(5));
        }
        assert_eq!(log.state(start + ms(500)).notes as usize, SCORE_LOG_NOTES);
        // What is older than the log reaches is forgotten, keys still down too.
        log.note_on(KICK, 61, 1.0, start + ms(400), None);
        log.prune(start + SCORE_LOG_SPAN + ms(300));
        assert_eq!(log.state(start + SCORE_LOG_SPAN + ms(300)).notes, 0);
        log.note_off(KICK, 61, start + SCORE_LOG_SPAN + ms(500));
        assert_eq!(log.state(start + SCORE_LOG_SPAN + ms(500)).notes, 1);
        log.prune(start + SCORE_LOG_SPAN + ms(900));
        assert_eq!(log.state(start + SCORE_LOG_SPAN + ms(900)).notes, 0);
    }

    fn tempo() -> AutomationTarget {
        AutomationTarget::Tempo
    }

    #[test]
    fn a_pass_of_automation_keeps_each_control_apart_and_is_bounded() {
        let mut take = AutomationTake::default();
        assert!(take.is_empty());
        take.record(tempo(), 0.2, 100.0, 0.3);
        take.record(tempo(), 0.9, 200.0, 0.4);
        let volume = AutomationTarget::ChannelVolume { channel: KICK };
        take.record(volume, 0.5, 150.0, 1.7);
        take.record(volume, 0.5, f64::NAN, 0.5);
        take.record(volume, 0.5, 160.0, f32::NAN);
        assert_eq!((take.targets(), take.pending()), (2, 3));
        let lanes = take.take();
        assert_eq!(lanes[0].target, tempo());
        // Only the first movement says what the control held before.
        assert_eq!(lanes[0].before, 0.2);
        assert_eq!(lanes[0].samples, [(100.0, 0.3), (200.0, 0.4)]);
        assert_eq!(lanes[1].samples, [(150.0, 1.0)]);
        assert!(take.is_empty());

        for index in 0..MAX_AUTOMATION_LANES as u32 + 3 {
            let target = AutomationTarget::ChannelPan {
                channel: ChannelId(index),
            };
            take.record(target, 0.5, 0.0, 0.5);
        }
        assert_eq!(take.targets() as usize, MAX_AUTOMATION_LANES);
        assert_eq!(take.dropped(), 3);
    }

    #[test]
    fn recorded_movements_become_ordered_points_without_the_idle_ones() {
        let points = recorded_points(
            &[
                (10.2, 0.1),
                (10.4, 0.15),
                (20.0, 0.15),
                (30.0, 0.150_5),
                (40.0, 0.9),
                (-5.0, 0.3),
                (f64::NAN, 0.3),
                (50.0, 7.0),
            ],
            MAX_AUTOMATION_POINTS,
        );
        let summary: Vec<(u32, f32)> = points
            .iter()
            .map(|point| (point.tick, point.value))
            .collect();
        // Two movements on tick 10 leave the later one. Of the movements
        // that change nothing only the last stays, so the control reads as
        // still until tick 30. The last one is held to the range.
        assert_eq!(summary, [(10, 0.15), (30, 0.150_5), (40, 0.9), (50, 1.0)]);
        assert!(points.iter().all(|point| point.curve == 0.0 && !point.hold));

        // A second pass over the same stretch replaces the first from there.
        let looped = recorded_points(
            &[
                (100.0, 0.1),
                (200.0, 0.2),
                (300.0, 0.3),
                (150.0, 0.8),
                (250.0, 0.9),
            ],
            MAX_AUTOMATION_POINTS,
        );
        let summary: Vec<(u32, f32)> = looped
            .iter()
            .map(|point| (point.tick, point.value))
            .collect();
        assert_eq!(summary, [(100, 0.1), (150, 0.8), (250, 0.9)]);

        // A budget thins further but keeps both ends.
        let ramp: Vec<(f64, f32)> = (0..1_000)
            .map(|step| (f64::from(step), step as f32 / 1_000.0))
            .collect();
        let thin = recorded_points(&ramp, 20);
        assert!(thin.len() <= 20 && thin.len() >= 2, "{}", thin.len());
        assert_eq!((thin[0].tick, thin[thin.len() - 1].tick), (0, 999));
        assert!(recorded_points(&[], 10).is_empty());
    }

    #[test]
    fn a_recorded_stretch_leaves_the_rest_of_the_curve_as_it_was() {
        let existing = [
            point(0, 0.2),
            point(1_000, 0.6),
            point(2_000, 0.6),
            point(3_000, 0.1),
        ];
        let recorded = [point(1_200, 0.9), point(1_500, 0.7), point(1_800, 0.8)];
        let merged = merge_points(&existing, &recorded, true);
        let summary: Vec<(u32, f32)> = merged
            .iter()
            .map(|point| (point.tick, point.value))
            .collect();
        assert_eq!(
            summary,
            [
                (0, 0.2),
                (1_000, 0.6),
                // The curve's own value up to the recording, then the jump.
                (1_200, 0.6),
                (1_200, 0.9),
                (1_500, 0.7),
                (1_800, 0.8),
                // And back to the curve where the recording ends.
                (1_800, 0.6),
                (2_000, 0.6),
                (3_000, 0.1),
            ]
        );
        // Outside the stretch the curve reads exactly as before.
        for tick in [
            0.0, 500.0, 999.0, 1_199.0, 1_801.0, 2_500.0, 3_000.0, 9_000.0,
        ] {
            let was = curve_value(&existing, tick);
            let now = curve_value(&merged, tick);
            assert!((was - now).abs() < 1e-5, "tick {tick}: {was} became {now}");
        }
        // Inside it reads the recording.
        assert!((curve_value(&merged, 1_350.0) - 0.8).abs() < 1e-5);

        // A stretch over points of the curve replaces them.
        let wide = merge_points(&existing, &[point(500, 0.5), point(2_500, 0.5)], true);
        let ticks: Vec<u32> = wide.iter().map(|point| point.tick).collect();
        assert_eq!(ticks, [0, 500, 500, 2_500, 2_500, 3_000]);
        // Nothing recorded changes nothing.
        assert_eq!(merge_points(&existing, &[], true), existing);
    }

    #[test]
    fn a_new_curve_starts_from_what_the_control_held_and_keeps_the_last_movement() {
        let before = [point(0, 0.25)];
        let recorded = [point(960, 0.5), point(1_920, 0.75)];
        let merged = merge_points(&before, &recorded, false);
        let summary: Vec<(u32, f32)> = merged
            .iter()
            .map(|point| (point.tick, point.value))
            .collect();
        assert_eq!(summary, [(0, 0.25), (960, 0.25), (960, 0.5), (1_920, 0.75)]);
        assert_eq!(curve_value(&merged, 500.0), 0.25);
        assert_eq!(curve_value(&merged, 5_000.0), 0.75);
        // A recording from the first tick replaces the starting point.
        let from_top = merge_points(&before, &[point(0, 0.5), point(100, 0.6)], false);
        assert_eq!(from_top.len(), 2);
        assert_eq!(from_top[0].value, 0.5);
        // The result never passes the most points a curve may have.
        let many: Vec<AutomationPoint> = (0..MAX_AUTOMATION_POINTS as u32 + 500)
            .map(|step| point(step * 2, if step % 2 == 0 { 0.0 } else { 1.0 }))
            .collect();
        let bounded = merge_points(&before, &many, false);
        assert!(bounded.len() <= MAX_AUTOMATION_POINTS);
        assert!(bounded.windows(2).all(|pair| pair[0].tick <= pair[1].tick));
    }
}
