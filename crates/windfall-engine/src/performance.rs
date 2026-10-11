//! Bounded, sample-clock clip launcher. It never seeks or starts the song.
//! Pattern data stays in the immutable plan; only cursors and note ownership
//! live here. Last request per track wins until the musical boundary.
use windfall_core::PPQ;
use windfall_project::{LaunchQuantization, PatternId, TimeSignature};

use crate::plan::{NoteEvent, Plan};

pub const TRACKS: usize = 32;
const HELD: usize = 512;
const EVENTS: usize = HELD * 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaunchRequest {
    pub track: u8,
    /// None stops this track at the requested boundary.
    pub pattern: Option<PatternId>,
    pub quantization: LaunchQuantization,
}

#[derive(Clone, Copy)]
struct Pending {
    request: LaunchRequest,
    at: f64,
}
#[derive(Clone, Copy, Default)]
struct Slot {
    pending: Option<Pending>,
    pattern: Option<PatternId>,
    origin: f64,
    cycle: u64,
    next: usize,
}
#[derive(Clone, Copy)]
struct Held {
    track: u8,
    token: u64,
    end: f64,
}
#[derive(Clone, Copy)]
pub(crate) enum Event {
    On { token: u64, note: NoteEvent },
    Off { token: u64 },
}

pub(crate) struct Launcher {
    slots: [Slot; TRACKS],
    held: [Option<Held>; HELD],
    events: Vec<Event>,
    tick: f64,
    token: u64,
    pub dropped: u64,
}

/// Inclusive boundary: an input admitted on a boundary can launch there.
pub fn quantize(tick: f64, origin: f64, signature: TimeSignature, q: LaunchQuantization) -> f64 {
    let unit = match q {
        LaunchQuantization::Immediate => return tick,
        LaunchQuantization::Beat => signature.ticks_per_beat(),
        LaunchQuantization::Bar => signature.ticks_per_bar(),
    } as f64;
    origin + (((tick - origin) / unit - 1e-9).ceil().max(0.0)) * unit
}

fn boundary(plan: &Plan, tick: f64, q: LaunchQuantization) -> f64 {
    let meters = plan.meters.as_ref().ok();
    let segment =
        meters.and_then(|all| all.iter().rev().find(|m| f64::from(m.start_tick()) <= tick));
    let origin = segment.map_or(0.0, |m| f64::from(m.start_tick()));
    let signature = segment.map_or(plan.signature, |m| m.signature());
    let at = quantize(tick, origin, signature, q);
    // An unaligned meter change itself is a new downbeat, even when the
    // preceding bar would otherwise end later.
    if q != LaunchQuantization::Immediate {
        meters
            .and_then(|all| all.iter().find(|m| f64::from(m.start_tick()) > tick))
            .map_or(at, |m| at.min(f64::from(m.start_tick())))
    } else {
        at
    }
}

impl Launcher {
    pub fn new() -> Self {
        Self {
            slots: [Slot::default(); TRACKS],
            held: [None; HELD],
            events: Vec::with_capacity(EVENTS),
            tick: 0.0,
            token: 0,
            dropped: 0,
        }
    }
    pub fn request(&mut self, request: LaunchRequest, plan: &Plan) {
        if usize::from(request.track) >= TRACKS
            || request
                .pattern
                .is_some_and(|id| plan.pattern_ids.get(id.0).is_none())
        {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.slots[usize::from(request.track)].pending = Some(Pending {
            at: boundary(plan, self.tick, request.quantization),
            request,
        });
    }
    pub fn retime(&mut self, plan: &Plan) {
        for slot in &mut self.slots {
            if let Some(pending) = &mut slot.pending {
                pending.at = boundary(plan, self.tick, pending.request.quantization);
            }
            if slot
                .pattern
                .is_some_and(|id| plan.pattern_ids.get(id.0).is_none())
            {
                slot.pattern = None;
            }
            if let Some(pattern) = slot
                .pattern
                .and_then(|id| plan.pattern_ids.get(id.0))
                .map(|i| &plan.patterns[i])
            {
                let elapsed = (self.tick - slot.origin).max(0.0);
                slot.cycle = (elapsed / f64::from(pattern.length)).floor() as u64;
                let phase = elapsed - slot.cycle as f64 * f64::from(pattern.length);
                slot.next = pattern
                    .events
                    .partition_point(|note| note.tick < phase - 1e-7);
                if slot.next == pattern.events.len() {
                    slot.next = 0;
                    slot.cycle = slot.cycle.saturating_add(1);
                }
            }
        }
    }
    pub fn active(&self) -> bool {
        self.slots
            .iter()
            .any(|s| s.pattern.is_some() || s.pending.is_some())
            || self.held.iter().any(Option::is_some)
    }
    pub fn event(&self, index: usize) -> Event {
        self.events[index]
    }
    pub fn events_len(&self) -> usize {
        self.events.len()
    }
    pub fn tick(&self) -> f64 {
        self.tick
    }
    pub fn advance(&mut self, plan: &Plan, frames: usize, rate: u32) {
        let delta = frames as f64 * plan.tempo_bpm * f64::from(PPQ) / (60.0 * f64::from(rate));
        self.tick = plan.unwarp(plan.warp(self.tick) + delta);
    }
    pub fn slot(&self, track: usize) -> (Option<PatternId>, Option<PatternId>, bool) {
        let slot = self.slots[track];
        (
            slot.pattern,
            slot.pending.and_then(|p| p.request.pattern),
            slot.pending.is_some_and(|p| p.request.pattern.is_none()),
        )
    }
    fn off_track(&mut self, track: u8) {
        for held in &mut self.held {
            if let Some(note) = held.filter(|note| note.track == track) {
                self.events.push(Event::Off { token: note.token });
                *held = None;
            }
        }
    }
    /// Releases are emitted before a replacement plan takes ownership.
    /// Active pads continue at their musical phase; no old note is retained.
    pub fn release(&mut self, stop: bool) {
        self.events.clear();
        for track in 0..TRACKS {
            self.off_track(track as u8);
        }
        if stop {
            self.slots.fill(Slot::default());
        }
    }
    pub fn step(&mut self, plan: &Plan, rate: u32) {
        self.events.clear();
        for track in 0..TRACKS {
            let slot = self.slots[track];
            if let Some(pending) = slot.pending.filter(|p| p.at <= self.tick + 1e-7) {
                self.off_track(track as u8);
                self.slots[track] = Slot {
                    pattern: pending.request.pattern,
                    origin: pending.at,
                    ..Slot::default()
                };
            }
        }
        for held in &mut self.held {
            if let Some(note) = held.filter(|note| note.end <= self.tick + 1e-7) {
                self.events.push(Event::Off { token: note.token });
                *held = None;
            }
        }
        for track in 0..TRACKS {
            let slot = &mut self.slots[track];
            let Some(pattern) = slot
                .pattern
                .and_then(|id| plan.pattern_ids.get(id.0))
                .map(|index| &plan.patterns[index])
            else {
                continue;
            };
            if pattern.events.is_empty() {
                continue;
            }
            // Each accepted on consumes a held slot. Overload discards ons,
            // never offs. Cap work per frame as well as memory under overload.
            for _ in 0..HELD {
                let note = pattern.events[slot.next];
                let at = slot.origin + slot.cycle as f64 * f64::from(pattern.length) + note.tick;
                if at > self.tick + 1e-7 {
                    break;
                }
                slot.next += 1;
                if slot.next == pattern.events.len() {
                    slot.next = 0;
                    slot.cycle = slot.cycle.saturating_add(1);
                }
                let Some(held) = self.held.iter_mut().find(|held| held.is_none()) else {
                    self.dropped = self.dropped.saturating_add(1);
                    continue;
                };
                let Some(token) = self.token.checked_add(1) else {
                    self.dropped = self.dropped.saturating_add(1);
                    continue;
                };
                self.token = token;
                *held = Some(Held {
                    track: track as u8,
                    token,
                    end: at + note.length,
                });
                self.events.push(Event::On { token, note });
            }
        }
        self.advance(plan, 1, rate);
    }
}

/// An admitted launch at a sample offset from the beginning of a performance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapturedLaunch {
    pub frame: u64,
    pub request: LaunchRequest,
}

/// Replays a bounded launch script through the actual audio processor, without
/// song playback. Events on the same frame preserve input order. The caller
/// keeps the matching immutable project/tempo snapshot with the script.
pub fn render_capture(
    project: &windfall_project::Project,
    pool: &crate::SamplePool,
    rate: u32,
    frames: usize,
    block: usize,
    launches: &[CapturedLaunch],
) -> Result<windfall_core::AudioBuffer, String> {
    if !(8000..=192000).contains(&rate)
        || frames > rate as usize * 60
        || launches.len() > 8192
        || launches
            .windows(2)
            .any(|pair| pair[0].frame > pair[1].frame)
        || launches.iter().any(|event| event.frame >= frames as u64)
    {
        return Err("Capture must be ordered, at most 8192 launches and at most 60 seconds".into());
    }
    project.check()?;
    // Native plugins require a render-role factory rather than an active owner.
    // This initial capture path refuses them instead of taking their state.
    if !project.plugins.is_empty() {
        return Err(
            "Native plugin performance capture requires an independent render-owner snapshot"
                .into(),
        );
    }
    let prepared = crate::Controller::prepare_project(project, pool).map_err(|e| e.to_string())?;
    let controller = crate::Controller::new();
    controller.set_prepared_project(project, prepared);
    let mut processor = controller.attach(rate);
    let latency = controller.latency_frames() as usize;
    let mut audio = vec![0.0; (frames + latency) * 2];
    let mut at = 0;
    let mut next = 0;
    while at < frames + latency {
        while next < launches.len() && launches[next].frame == at as u64 {
            controller
                .performance_launch(launches[next].request)
                .map_err(str::to_owned)?;
            next += 1;
        }
        if at == frames {
            controller.performance_stop_all();
        }
        let until = launches
            .get(next)
            .map_or(frames + latency, |e| e.frame as usize);
        let end = (at + block.clamp(1, 4096)).min(until).min(if at < frames {
            frames
        } else {
            frames + latency
        });
        processor.process(&mut audio[at * 2..end * 2]);
        at = end;
    }
    if controller.performance_state().dropped_notes != "0" {
        return Err("Capture exceeded the launcher's note capacity".into());
    }
    audio.drain(..latency * 2);
    Ok(windfall_core::AudioBuffer::from_interleaved(rate, 2, audio))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SamplePool, plan::compile};
    use windfall_project::{Project, StageMedia};

    #[test]
    fn beat_bar_meter_boundaries_are_inclusive() {
        let sig = TimeSignature {
            numerator: 7,
            denominator: 8,
        };
        assert_eq!(quantize(481.0, 0.0, sig, LaunchQuantization::Beat), 960.0);
        assert_eq!(quantize(1.0, 0.0, sig, LaunchQuantization::Bar), 3360.0);
        assert_eq!(quantize(3360.0, 0.0, sig, LaunchQuantization::Bar), 3360.0);
    }
    #[test]
    fn latest_queued_launch_or_stop_wins_without_song_seek() {
        let project = Project::new("launch");
        let plan = compile(&project, &SamplePool::new());
        let mut launch = Launcher::new();
        let pattern = project.patterns[0].id;
        launch.request(
            LaunchRequest {
                track: 0,
                pattern: Some(pattern),
                quantization: LaunchQuantization::Bar,
            },
            &plan,
        );
        launch.request(
            LaunchRequest {
                track: 0,
                pattern: None,
                quantization: LaunchQuantization::Beat,
            },
            &plan,
        );
        launch.step(&plan, 48000);
        assert_eq!(launch.slot(0), (None, None, false));
        assert!(!launch.active());
        assert!(project.settings.stage_media == StageMedia::default());
    }
}
