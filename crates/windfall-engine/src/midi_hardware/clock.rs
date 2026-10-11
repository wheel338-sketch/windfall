//! MIDI clock, both ways, as pure state machines with the time passed in.
//!
//! [`ClockOut`] turns readings of the transport into start, stop, song
//! position and the twenty-four pulses of each quarter note. [`ClockIn`]
//! turns the same messages from a device into a tempo and transport events.
//! Neither reads a clock or sends anything itself, so tests drive them with
//! made-up time and the MIDI worker drives them with real time.

use std::time::{Duration, Instant};

use windfall_ipc::PlayMode;

use super::message::MidiMessage;
use crate::Playhead;

/// Clock pulses in a quarter note.
pub const PULSES_PER_QUARTER: u32 = 24;
/// Ticks between two pulses.
const PULSE_TICKS: f64 = (windfall_core::PPQ / PULSES_PER_QUARTER) as f64;
/// Ticks in a sixteenth note, the unit of the song position pointer.
const SIXTEENTH_TICKS: f64 = (windfall_core::PPQ / 4) as f64;
/// A step of the playhead further than this from where it was expected is
/// a jump: a loop, a seek or a new pattern.
const JUMP_TICKS: f64 = (windfall_core::PPQ / 4) as f64;
/// How much of the gap between the expected and the published playhead is
/// taken up each time one is published. The playhead is published once a
/// buffer; following it loosely keeps the buffer's own timing out of the
/// pulses.
const FOLLOW: f64 = 0.15;
/// Pulses sent in one go at most: the six of a sixteenth note, which is the
/// most a start can have to catch up. A longer backlog is dropped, not sent
/// as a burst.
const MAX_BURST: u32 = 6;
/// How close to its tick a pulse may be sent early, in ticks.
const EARLY_TICKS: f64 = 0.02;

/// Sends MIDI clock for the transport it is shown.
#[derive(Debug, Clone)]
pub struct ClockOut {
    running: bool,
    /// Where the playhead is taken to be, and when that was.
    tick: f64,
    at: Instant,
    rate: f64,
    /// The playhead as the engine last published it.
    seen: f64,
    /// The tick of the next pulse.
    next: f64,
    pulses: u64,
}

impl ClockOut {
    pub fn new(now: Instant) -> Self {
        Self {
            running: false,
            tick: 0.0,
            at: now,
            rate: 0.0,
            seen: 0.0,
            next: 0.0,
            pulses: 0,
        }
    }

    /// True between a start and a stop that were sent.
    pub fn running(&self) -> bool {
        self.running
    }

    /// Pulses sent since this clock was made.
    pub fn pulses(&self) -> u64 {
        self.pulses
    }

    /// Reads the transport and sends, through `send`, every message due at
    /// `now`: start or song position and continue when playback begins, the
    /// pulses that have come due, and stop when playback ends.
    pub fn poll(&mut self, playhead: &Playhead, now: Instant, send: &mut dyn FnMut(&[u8])) {
        if !playhead.playing {
            self.stop(send);
            return;
        }
        let rate = if playhead.ticks_per_second.is_finite() && playhead.ticks_per_second > 0.0 {
            playhead.ticks_per_second
        } else {
            f64::from(windfall_core::PPQ) * 2.0
        };
        if !self.running {
            self.running = true;
            self.tick = playhead.tick.max(0.0);
            self.at = now;
            self.seen = playhead.tick;
            self.rate = rate;
            let sixteenths = ((self.tick / SIXTEENTH_TICKS) as u32).min(0x3fff);
            if sixteenths == 0 {
                // Playback is first seen a buffer or so after it began.
                // Within the first sixteenth that is still a start from the
                // top, and the pulses already missed are caught up at once.
                send(&[0xfa]);
                self.next = 0.0;
            } else {
                send(&[0xf2, (sixteenths & 127) as u8, (sixteenths >> 7) as u8]);
                send(&[0xfb]);
                self.next = (self.tick / PULSE_TICKS).ceil() * PULSE_TICKS;
            }
        }
        let elapsed = now.saturating_duration_since(self.at).as_secs_f64();
        let mut tick = self.tick + elapsed * self.rate;
        self.rate = rate;
        if playhead.tick != self.seen {
            self.seen = playhead.tick;
            let gap = playhead.tick - tick;
            let wrap = playhead
                .pattern_ticks
                .filter(|_| playhead.mode == PlayMode::Pattern)
                .map(f64::from);
            if gap.abs() <= JUMP_TICKS {
                tick += gap * FOLLOW;
            } else if let Some(length) = wrap.filter(|length| (gap + length).abs() <= JUMP_TICKS) {
                // The pattern came round. The pulse grid carries on through
                // the seam, since a pattern is a whole number of steps long.
                tick -= length;
                self.next -= length;
                tick += (playhead.tick - tick) * FOLLOW;
            } else {
                // A seek, a song loop or another pattern: start over from
                // where the playhead is, without a burst and without a gap
                // longer than one pulse.
                tick = playhead.tick.max(0.0);
                self.next = (tick / PULSE_TICKS).ceil() * PULSE_TICKS;
            }
        }
        let mut sent = 0;
        while self.next <= tick + EARLY_TICKS && sent < MAX_BURST {
            send(&[0xf8]);
            self.pulses += 1;
            self.next += PULSE_TICKS;
            sent += 1;
        }
        if self.next <= tick {
            self.next = (tick / PULSE_TICKS).floor() * PULSE_TICKS + PULSE_TICKS;
        }
        self.tick = tick;
        self.at = now;
    }

    /// Sends stop if a start was sent, as when the clock is switched off
    /// while playing.
    pub fn stop(&mut self, send: &mut dyn FnMut(&[u8])) {
        if self.running {
            self.running = false;
            send(&[0xfc]);
        }
    }

    /// When the next pulse comes due, while playback runs.
    pub fn next_due(&self) -> Option<Instant> {
        if !self.running || self.rate <= 0.0 {
            return None;
        }
        let seconds = ((self.next - self.tick) / self.rate).clamp(0.0, 1.0);
        Some(self.at + Duration::from_secs_f64(seconds))
    }
}

/// What a device's clock asks of the transport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClockInEvent {
    Start,
    Continue,
    Stop,
    /// The device's tempo in beats per minute, to a tenth.
    Tempo(f64),
}

/// How many pulse intervals the tempo is averaged over: one quarter note.
const AVERAGED: usize = PULSES_PER_QUARTER as usize;
/// Pulses needed before a tempo is believed.
const SETTLE: usize = 8;
/// A silence this long means the clock stopped and its tempo is forgotten.
const SILENCE: Duration = Duration::from_millis(500);

/// Follows the clock a device sends.
#[derive(Debug, Clone)]
pub struct ClockIn {
    last: Option<Instant>,
    intervals: [f64; AVERAGED],
    count: usize,
    at: usize,
    reported: Option<f64>,
}

impl Default for ClockIn {
    fn default() -> Self {
        Self {
            last: None,
            intervals: [0.0; AVERAGED],
            count: 0,
            at: 0,
            reported: None,
        }
    }
}

impl ClockIn {
    /// The tempo last worked out, while the clock is running.
    pub fn bpm(&self) -> Option<f64> {
        self.reported
    }

    /// Takes one message captured at `at`. Answers what the transport
    /// should do about it, if anything.
    pub fn message(&mut self, message: MidiMessage, at: Instant) -> Option<ClockInEvent> {
        match message {
            MidiMessage::Start => Some(ClockInEvent::Start),
            MidiMessage::Continue => Some(ClockInEvent::Continue),
            MidiMessage::Stop => Some(ClockInEvent::Stop),
            MidiMessage::Clock => self.pulse(at),
            _ => None,
        }
    }

    /// Forgets the tempo once the clock has been silent for a while.
    pub fn idle(&mut self, now: Instant) {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) > SILENCE)
        {
            *self = Self::default();
        }
    }

    fn pulse(&mut self, at: Instant) -> Option<ClockInEvent> {
        let last = self.last.replace(at)?;
        let gap = at.saturating_duration_since(last);
        if gap > SILENCE || gap.is_zero() {
            // The clock paused, or two pulses carry one time: neither says
            // anything about tempo.
            if gap > SILENCE {
                self.count = 0;
                self.at = 0;
            }
            return None;
        }
        self.intervals[self.at] = gap.as_secs_f64();
        self.at = (self.at + 1) % AVERAGED;
        self.count = (self.count + 1).min(AVERAGED);
        if self.count < SETTLE {
            return None;
        }
        let mean = self.intervals[..self.count].iter().sum::<f64>() / self.count as f64;
        let bpm = 60.0 / (mean * f64::from(PULSES_PER_QUARTER));
        let bpm = (bpm * 10.0).round() / 10.0;
        // The first reading comes early, to lock on quickly. After that only
        // a full quarter note of pulses may change it, which keeps one late
        // pulse from reading as a tempo change.
        let trusted = self.reported.is_none() || self.count == AVERAGED;
        let plausible = bpm.is_finite() && (10.0..=999.0).contains(&bpm);
        if !trusted || !plausible || self.reported.is_some_and(|told| (told - bpm).abs() < 0.15) {
            return None;
        }
        self.reported = Some(bpm);
        Some(ClockInEvent::Tempo(bpm))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windfall_project::PatternId;

    fn playhead(playing: bool, tick: f64, bpm: f64) -> Playhead {
        Playhead {
            playing,
            tick,
            ticks_per_second: bpm * f64::from(windfall_core::PPQ) / 60.0,
            mode: PlayMode::Song,
            pattern: PatternId(1),
            pattern_ticks: None,
            loop_song: false,
        }
    }

    /// Runs a clock against a transport that publishes its playhead once
    /// every `buffer` and is looked at every `poll`, for `length`.
    /// `reading` gives the true transport at a time since the start.
    fn run(
        clock: &mut ClockOut,
        start: Instant,
        length: Duration,
        buffer: Duration,
        poll: Duration,
        mut reading: impl FnMut(Duration) -> Playhead,
    ) -> Vec<(Duration, Vec<u8>)> {
        let mut sent = Vec::new();
        let mut time = Duration::ZERO;
        let mut published = reading(Duration::ZERO);
        let mut next_buffer = buffer;
        while time <= length {
            while next_buffer <= time {
                published = reading(next_buffer);
                next_buffer += buffer;
            }
            let now = start + time;
            clock.poll(&published, now, &mut |bytes| {
                sent.push((time, bytes.to_vec()))
            });
            // Wake for the next pulse when it comes before the next look.
            let wake = clock
                .next_due()
                .map(|due| due.saturating_duration_since(start))
                .filter(|due| *due > time && *due < time + poll);
            time = wake.unwrap_or(time + poll);
        }
        sent
    }

    fn pulses(sent: &[(Duration, Vec<u8>)]) -> Vec<f64> {
        sent.iter()
            .filter(|(_, bytes)| bytes == &[0xf8])
            .map(|(time, _)| time.as_secs_f64())
            .collect()
    }

    #[test]
    fn playback_from_the_top_sends_start_then_evenly_spaced_pulses_then_stop() {
        let start = Instant::now();
        let mut clock = ClockOut::new(start);
        let bpm = 120.0;
        let rate = bpm * 960.0 / 60.0;
        let length = Duration::from_secs(4);
        let sent = run(
            &mut clock,
            start,
            length,
            Duration::from_millis(10),
            Duration::from_millis(2),
            |time| {
                playhead(
                    time < Duration::from_millis(3_990),
                    time.as_secs_f64() * rate,
                    bpm,
                )
            },
        );
        assert_eq!(sent.first().unwrap().1, [0xfa]);
        assert_eq!(sent.last().unwrap().1, [0xfc]);
        assert_eq!(sent.iter().filter(|(_, bytes)| bytes == &[0xfc]).count(), 1);
        let times = pulses(&sent);
        // Two beats a second for four seconds, twenty-four pulses each.
        let expected = 8 * 24;
        assert!(
            (times.len() as i64 - expected).abs() <= 1,
            "{} pulses",
            times.len()
        );
        let interval = 60.0 / bpm / 24.0;
        for pair in times.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(
                (gap - interval).abs() < 0.002,
                "a pulse came {gap} s after the one before"
            );
        }
        let mean = (times[times.len() - 1] - times[0]) / (times.len() - 1) as f64;
        assert!((mean - interval).abs() < 0.000_05, "mean interval {mean}");
        assert!(!clock.running());
        assert_eq!(clock.pulses() as usize, times.len());
    }

    #[test]
    fn playback_from_the_middle_sends_the_position_and_continue() {
        let start = Instant::now();
        let mut clock = ClockOut::new(start);
        let mut sent = Vec::new();
        // Bar 3 of 4/4: 32 sixteenths in.
        clock.poll(&playhead(true, 7_680.0, 120.0), start, &mut |bytes| {
            sent.push(bytes.to_vec());
        });
        assert_eq!(sent[0], [0xf2, 32, 0]);
        assert_eq!(sent[1], [0xfb]);
        // On a pulse boundary, that pulse goes out with it.
        assert_eq!(sent[2], [0xf8]);
        assert!(clock.running());
        // A long way in still fits fourteen bits.
        let mut far = ClockOut::new(start);
        let mut sent = Vec::new();
        far.poll(
            &playhead(true, 240.0 * 300.0 + 10.0, 120.0),
            start,
            &mut |bytes| {
                sent.push(bytes.to_vec());
            },
        );
        // 300 sixteenths: 44 in the low seven bits, 2 above them.
        assert_eq!(sent[0], [0xf2, 44, 2]);
        assert_eq!(sent.len(), 2, "between two pulses nothing more is due");
    }

    #[test]
    fn a_start_seen_late_is_still_a_start_and_catches_its_pulses_up() {
        let start = Instant::now();
        for (tick, caught_up) in [(19.2, 1), (100.0, 3), (239.0, 6)] {
            let mut clock = ClockOut::new(start);
            let mut sent = Vec::new();
            clock.poll(&playhead(true, tick, 120.0), start, &mut |bytes| {
                sent.push(bytes.to_vec());
            });
            assert_eq!(sent[0], [0xfa], "{tick}");
            assert_eq!(sent.len(), 1 + caught_up, "{tick}");
            assert!(sent[1..].iter().all(|bytes| bytes == &[0xf8]));
        }
    }

    #[test]
    fn a_tempo_change_moves_the_pulse_spacing_with_it() {
        let start = Instant::now();
        let mut clock = ClockOut::new(start);
        // 120 bpm for two seconds, then 90.
        let tick_at = |time: Duration| {
            let seconds = time.as_secs_f64();
            if seconds < 2.0 {
                (seconds * 1_920.0, 120.0)
            } else {
                (3_840.0 + (seconds - 2.0) * 1_440.0, 90.0)
            }
        };
        let sent = run(
            &mut clock,
            start,
            Duration::from_secs(5),
            Duration::from_millis(10),
            Duration::from_millis(2),
            |time| {
                let (tick, bpm) = tick_at(time);
                playhead(true, tick, bpm)
            },
        );
        let times = pulses(&sent);
        let late: Vec<f64> = times.iter().copied().filter(|time| *time > 2.5).collect();
        let mean = (late[late.len() - 1] - late[0]) / (late.len() - 1) as f64;
        assert!((mean - 60.0 / 90.0 / 24.0).abs() < 0.000_1, "{mean}");
        let early: Vec<f64> = times.iter().copied().filter(|time| *time < 1.9).collect();
        let mean = (early[early.len() - 1] - early[0]) / (early.len() - 1) as f64;
        assert!((mean - 60.0 / 120.0 / 24.0).abs() < 0.000_1, "{mean}");
    }

    #[test]
    fn a_looping_pattern_keeps_the_pulses_even_across_the_seam() {
        let start = Instant::now();
        let mut clock = ClockOut::new(start);
        let length = 3_840.0;
        let sent = run(
            &mut clock,
            start,
            Duration::from_secs(6),
            Duration::from_millis(10),
            Duration::from_millis(2),
            |time| Playhead {
                mode: PlayMode::Pattern,
                pattern_ticks: Some(3_840),
                ..playhead(true, (time.as_secs_f64() * 1_920.0) % length, 120.0)
            },
        );
        let times = pulses(&sent);
        assert!((times.len() as i64 - 12 * 24).abs() <= 1, "{}", times.len());
        let interval = 60.0 / 120.0 / 24.0;
        for pair in times.windows(2) {
            assert!(((pair[1] - pair[0]) - interval).abs() < 0.002);
        }
        assert_eq!(sent.iter().filter(|(_, bytes)| bytes == &[0xfa]).count(), 1);
    }

    #[test]
    fn a_seek_starts_over_without_a_burst_or_a_long_gap() {
        let start = Instant::now();
        let mut clock = ClockOut::new(start);
        let sent = run(
            &mut clock,
            start,
            Duration::from_secs(3),
            Duration::from_millis(10),
            Duration::from_millis(2),
            |time| {
                let seconds = time.as_secs_f64();
                // After one second the playhead is thrown eight bars on.
                let jump = if seconds >= 1.0 { 30_720.0 } else { 0.0 };
                playhead(true, seconds * 1_920.0 + jump, 120.0)
            },
        );
        let times = pulses(&sent);
        let interval = 60.0 / 120.0 / 24.0;
        for pair in times.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(gap > 0.001, "pulses {gap} s apart are a burst");
            assert!(gap < interval * 2.0 + 0.012, "{gap} s without a pulse");
        }
        assert!((times.len() as i64 - 6 * 24).abs() <= 2, "{}", times.len());
    }

    #[test]
    fn a_stopped_transport_sends_nothing_and_stop_is_sent_once() {
        let start = Instant::now();
        let mut clock = ClockOut::new(start);
        let mut sent: Vec<Vec<u8>> = Vec::new();
        clock.poll(&playhead(false, 0.0, 120.0), start, &mut |bytes| {
            sent.push(bytes.to_vec());
        });
        assert!(sent.is_empty());
        assert_eq!(clock.next_due(), None);
        clock.poll(&playhead(true, 0.0, 120.0), start, &mut |bytes| {
            sent.push(bytes.to_vec());
        });
        assert_eq!(sent, [vec![0xfa], vec![0xf8]]);
        let due = clock.next_due().unwrap();
        let wait = due.duration_since(start).as_secs_f64();
        assert!((wait - 60.0 / 120.0 / 24.0).abs() < 1e-6, "{wait}");
        clock.stop(&mut |bytes| sent.push(bytes.to_vec()));
        clock.stop(&mut |bytes| sent.push(bytes.to_vec()));
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[2], [0xfc]);
        // A tempo that is not a number cannot stall the clock.
        let mut odd = ClockOut::new(start);
        odd.poll(&playhead(true, 0.0, f64::NAN), start, &mut |_| {});
        assert!(odd.next_due().is_some());
    }

    fn feed(
        clock: &mut ClockIn,
        start: Instant,
        bpm: f64,
        pulses: u32,
        jitter: f64,
    ) -> Vec<ClockInEvent> {
        let interval = 60.0 / bpm / 24.0;
        (0..pulses)
            .filter_map(|pulse| {
                // A repeating early/late pattern that averages out.
                let wobble = match pulse % 4 {
                    1 => jitter,
                    3 => -jitter,
                    _ => 0.0,
                };
                let at =
                    start + Duration::from_secs_f64(f64::from(pulse) * interval + wobble + 0.1);
                clock.message(MidiMessage::Clock, at)
            })
            .collect()
    }

    #[test]
    fn a_device_clock_gives_its_tempo_despite_jitter() {
        for bpm in [60.0, 120.0, 174.0, 93.5] {
            let mut clock = ClockIn::default();
            let events = feed(&mut clock, Instant::now(), bpm, 96, 0.000_8);
            let Some(ClockInEvent::Tempo(told)) = events.last().copied() else {
                panic!("a steady clock gives a tempo");
            };
            assert!((told - bpm).abs() <= 0.5, "{bpm} read as {told}");
            assert_eq!(clock.bpm(), Some(told));
            // A steady clock does not keep reporting.
            assert!(events.len() <= 6, "{} reports", events.len());
        }
    }

    #[test]
    fn a_device_tempo_change_is_followed_and_silence_forgets_it() {
        let start = Instant::now();
        let mut clock = ClockIn::default();
        feed(&mut clock, start, 120.0, 48, 0.0);
        assert_eq!(clock.bpm(), Some(120.0));
        let later = start + Duration::from_secs(1) + Duration::from_millis(100);
        let events = feed(&mut clock, later, 100.0, 96, 0.0);
        assert_eq!(events.last(), Some(&ClockInEvent::Tempo(100.0)));
        clock.idle(later + Duration::from_secs(10));
        assert_eq!(clock.bpm(), None);
        // Too few pulses say nothing yet.
        let mut clock = ClockIn::default();
        assert!(feed(&mut clock, start, 120.0, SETTLE as u32, 0.0).is_empty());
    }

    #[test]
    fn transport_messages_pass_through_and_others_are_ignored() {
        let mut clock = ClockIn::default();
        let now = Instant::now();
        assert_eq!(
            clock.message(MidiMessage::Start, now),
            Some(ClockInEvent::Start)
        );
        assert_eq!(
            clock.message(MidiMessage::Continue, now),
            Some(ClockInEvent::Continue)
        );
        assert_eq!(
            clock.message(MidiMessage::Stop, now),
            Some(ClockInEvent::Stop)
        );
        let note = MidiMessage::NoteOn {
            channel: 0,
            key: 60,
            velocity: 100,
        };
        assert_eq!(clock.message(note, now), None);
        // Two pulses with one time say nothing about tempo.
        assert_eq!(clock.message(MidiMessage::Clock, now), None);
        assert_eq!(clock.message(MidiMessage::Clock, now), None);
    }
}
