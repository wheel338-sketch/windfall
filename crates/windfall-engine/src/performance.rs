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
    On {
        token: u64,
        note: NoteEvent,
        pattern: PatternId,
        start: f64,
    },
    Off {
        token: u64,
        stopped: bool,
    },
}

pub(crate) struct Launcher {
    slots: [Slot; TRACKS],
    held: [Option<Held>; HELD],
    events: Vec<Event>,
    accepted: Vec<LaunchRequest>,
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
            accepted: Vec::with_capacity(TRACKS),
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
    /// Tempo/meter/mixer edits keep the exact owners and note ends. A changed
    /// pattern or channel-index layout is released before its new data arrives.
    pub fn compatible(&self, old: &Plan, new: &Plan) -> bool {
        if old.channels.len() != new.channels.len()
            || old
                .channels
                .iter()
                .zip(&new.channels)
                .any(|(a, b)| a.id != b.id)
        {
            return false;
        }
        self.slots.iter().filter_map(|slot| slot.pattern).all(|id| {
            let Some(a) = old.pattern_ids.get(id.0).map(|i| &old.patterns[i]) else {
                return false;
            };
            let Some(b) = new.pattern_ids.get(id.0).map(|i| &new.patterns[i]) else {
                return false;
            };
            a.length == b.length
                && a.events.len() == b.events.len()
                && a.events.iter().zip(&b.events).all(|(x, y)| {
                    x.source == y.source
                        && x.tick == y.tick
                        && x.length == y.length
                        && x.channel == y.channel
                        && x.key == y.key
                        && x.velocity == y.velocity
                        && x.pan == y.pan
                        && x.expression == y.expression
                })
        })
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
    pub fn accepted_len(&self) -> usize {
        self.accepted.len()
    }
    pub fn accepted(&self, index: usize) -> LaunchRequest {
        self.accepted[index]
    }
    pub fn tick(&self) -> f64 {
        self.tick
    }
    pub fn advance(&mut self, plan: &Plan, frames: usize, rate: u32) {
        if let Some(bpm) = self.tempo_override(plan) {
            self.tick += frames as f64 * bpm * f64::from(PPQ) / (60.0 * f64::from(rate));
            return;
        }
        let delta = frames as f64 * plan.tempo_bpm * f64::from(PPQ) / (60.0 * f64::from(rate));
        self.tick = plan.unwarp(plan.warp(self.tick) + delta);
    }
    pub fn tempo_override(&self, plan: &Plan) -> Option<f64> {
        if !plan.performance_tempo {
            return None;
        }
        (0..TRACKS)
            .filter_map(|track| {
                let (id, phase, _) = self.clip_phase(plan, track, self.tick)?;
                plan.performance_clips[plan.performance_clip_ids.get(id.0)?].tempo(phase)
            })
            .next_back()
    }
    pub fn slot(&self, track: usize) -> (Option<PatternId>, Option<PatternId>, bool) {
        let slot = self.slots[track];
        (
            slot.pattern,
            slot.pending.and_then(|p| p.request.pattern),
            slot.pending.is_some_and(|p| p.request.pattern.is_none()),
        )
    }
    /// Read-only clip phase; it does not consume, launch or reorder events.
    pub fn clip_phase(
        &self,
        plan: &Plan,
        track: usize,
        tick: f64,
    ) -> Option<(PatternId, f64, f64)> {
        let slot = self.slots[track];
        let id = slot.pattern?;
        let length = f64::from(plan.patterns[plan.pattern_ids.get(id.0)?].length);
        let elapsed = (tick - slot.origin).max(0.0);
        let cycle = (elapsed / length).floor();
        Some((id, elapsed - cycle * length, slot.origin + cycle * length))
    }
    fn off_track(&mut self, track: u8) {
        for held in &mut self.held {
            if let Some(note) = held.filter(|note| note.track == track) {
                self.events.push(Event::Off {
                    token: note.token,
                    stopped: true,
                });
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
        self.accepted.clear();
        for track in 0..TRACKS {
            let slot = self.slots[track];
            if let Some(pending) = slot.pending.filter(|p| p.at <= self.tick + 1e-7) {
                self.off_track(track as u8);
                self.slots[track] = Slot {
                    pattern: pending.request.pattern,
                    origin: pending.at,
                    ..Slot::default()
                };
                // A notification only: do not change pending/held/event order.
                self.accepted.push(LaunchRequest {
                    quantization: LaunchQuantization::Immediate,
                    ..pending.request
                });
            }
        }
        for held in &mut self.held {
            if let Some(note) = held.filter(|note| note.end <= self.tick + 1e-7) {
                self.events.push(Event::Off {
                    token: note.token,
                    stopped: false,
                });
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
                self.events.push(Event::On {
                    token,
                    note,
                    pattern: pattern.id,
                    start: at,
                });
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
    let mut render_pool = pool.clone();
    if !project.plugins.is_empty() {
        let factory = pool
            .plugin_factory()
            .ok_or("Native performance render provider is unavailable")?;
        let render = factory
            .render_factory()
            .ok_or("Native performance capture needs an independent render-owner snapshot")?;
        render_pool.set_plugin_factory(render);
    }
    let prepared =
        crate::Controller::prepare_project(project, &render_pool).map_err(|e| e.to_string())?;
    let controller = crate::Controller::new();
    controller.set_prepared_project(project, prepared);
    let mut processor = controller.attach(rate);
    let latency = controller.latency_frames() as usize;
    let mut audio = vec![0.0; (frames + latency) * 2];
    let mut at = 0;
    let mut next = 0;
    while at < frames + latency {
        // Captured frames are output-timeline positions. The warm-up region
        // trimmed below must also offset control admission; otherwise a causal
        // processor's attack is moved before the recorded launch by trimming.
        while next < launches.len() && launches[next].frame + latency as u64 == at as u64 {
            controller
                .performance_launch(launches[next].request)
                .map_err(str::to_owned)?;
            next += 1;
        }
        let until = launches
            .get(next)
            .map_or(frames + latency, |e| e.frame as usize + latency);
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
    if let Some(error) = render_pool.plugin_render_error() {
        return Err(error);
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
    fn accepted_log_contains_only_the_winning_launch_at_its_quantized_boundary() {
        let plan = note_plan();
        let pattern = plan.patterns[0].id;
        let mut launcher = Launcher::new();
        launcher.tick = 100.0;
        let request = LaunchRequest {
            track: 0,
            pattern: Some(pattern),
            quantization: LaunchQuantization::Beat,
        };
        launcher.request(request, &plan);
        launcher.request(
            LaunchRequest {
                pattern: None,
                ..request
            },
            &plan,
        );
        launcher.request(request, &plan);
        launcher.tick = 959.9;
        launcher.step(&plan, 48000);
        assert_eq!(launcher.accepted_len(), 0);
        launcher.tick = 960.0;
        launcher.step(&plan, 48000);
        assert_eq!(launcher.accepted_len(), 1);
        assert_eq!(
            launcher.accepted(0),
            LaunchRequest {
                quantization: LaunchQuantization::Immediate,
                ..request
            }
        );
        launcher.step(&plan, 48000);
        assert_eq!(launcher.accepted_len(), 0);
    }

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

    fn note_plan() -> Plan {
        let project = Project::new("owned notes");
        let mut plan = compile(&project, &SamplePool::new());
        plan.patterns[0].events = vec![NoteEvent {
            scripted: false,
            source: windfall_project::NoteId(100),
            tick: 0.0,
            length: 960.0,
            channel: 0,
            key: 60,
            velocity: 1.0,
            pan: 0.0,
            expression: windfall_dsp::NoteExpression::default(),
        }];
        plan
    }

    #[test]
    fn queued_boundary_retimes_at_new_meter_and_preserves_tempo_owned_note_end() {
        let mut plan = note_plan();
        let pattern = plan.patterns[0].id;
        let mut launch = Launcher::new();
        launch.tick = 100.0;
        let request = LaunchRequest {
            track: 0,
            pattern: Some(pattern),
            quantization: LaunchQuantization::Bar,
        };
        launch.request(request, &plan);
        assert_eq!(launch.slots[0].pending.unwrap().at, 3840.0);
        let change = windfall_project::MeterChange {
            id: windfall_project::MeterChangeId(100),
            tick: 720,
            signature: TimeSignature {
                numerator: 7,
                denominator: 8,
            },
        };
        plan.meters = windfall_project::MeterMap::checked(plan.signature, &[change])
            .map(|map| map.segments().to_vec());
        plan.tempo_bpm = 73.0;
        launch.retime(&plan);
        assert_eq!(launch.slots[0].pending.unwrap().at, 720.0);
        launch.tick = 719.9;
        launch.step(&plan, 48000);
        assert_eq!(launch.events_len(), 0);
        launch.tick = 720.0;
        launch.step(&plan, 48000);
        let token = match launch.event(0) {
            Event::On { token, .. } => token,
            _ => panic!("launch on downbeat"),
        };
        assert_eq!(launch.held.iter().flatten().next().unwrap().end, 1680.0);
        let mut faster = plan.clone();
        faster.tempo_bpm = 190.0;
        assert!(launch.compatible(&plan, &faster));
        launch.retime(&faster);
        assert_eq!(launch.held.iter().flatten().next().unwrap().token, token);
        launch.tick = 1680.0;
        launch.step(&faster, 48000);
        assert!(matches!(launch.event(0), Event::Off { token: released, .. } if released == token));
        assert!(launch.held.iter().all(Option::is_none));
    }

    #[test]
    fn launch_stop_launch_race_releases_old_owner_before_new_owner() {
        let plan = note_plan();
        let pattern = plan.patterns[0].id;
        let mut launch = Launcher::new();
        let request = LaunchRequest {
            track: 0,
            pattern: Some(pattern),
            quantization: LaunchQuantization::Immediate,
        };
        launch.request(request, &plan);
        launch.step(&plan, 48000);
        let first = match launch.event(0) {
            Event::On { token, .. } => token,
            _ => panic!("note on"),
        };
        launch.request(
            LaunchRequest {
                pattern: None,
                ..request
            },
            &plan,
        );
        launch.request(request, &plan);
        launch.step(&plan, 48000);
        assert!(matches!(launch.event(0), Event::Off { token, .. } if token == first));
        let second = match launch.event(1) {
            Event::On { token, .. } => token,
            _ => panic!("replacement note"),
        };
        assert_ne!(first, second);
        launch.request(request, &plan);
        launch.request(
            LaunchRequest {
                pattern: None,
                ..request
            },
            &plan,
        );
        launch.step(&plan, 48000);
        assert_eq!(launch.events_len(), 1);
        assert!(matches!(launch.event(0), Event::Off { token, .. } if token == second));
        assert!(!launch.active());
    }

    #[test]
    fn overload_drops_only_ons_and_all_accepted_owners_release_without_allocator_calls() {
        let mut plan = note_plan();
        let note = plan.patterns[0].events[0];
        plan.patterns[0].events = vec![note; HELD + 1];
        let request = LaunchRequest {
            track: 0,
            pattern: Some(plan.patterns[0].id),
            quantization: LaunchQuantization::Immediate,
        };
        let mut launch = Launcher::new();
        let calls = crate::test_alloc::allocator_calls(|| {
            launch.request(request, &plan);
            launch.step(&plan, 48000);
            launch.step(&plan, 48000);
        });
        assert_eq!(calls, 0);
        assert_eq!(launch.held.iter().flatten().count(), HELD);
        assert!(launch.dropped > 0);
        let tokens: Vec<_> = launch
            .held
            .iter()
            .flatten()
            .map(|note| note.token)
            .collect();
        assert_eq!(
            crate::test_alloc::allocator_calls(|| launch.release(true)),
            0
        );
        assert_eq!(launch.events_len(), HELD);
        for token in tokens {
            assert_eq!(launch.events.iter().filter(|event| matches!(event, Event::Off { token: released, .. } if *released == token)).count(), 1);
        }
        assert!(!launch.active());
        assert_eq!(launch.events.capacity(), EVENTS);
    }

    #[test]
    fn processor_launch_retrigger_stop_and_tempo_adoption_do_not_call_allocator() {
        use windfall_project::{ChannelId, Command, Document, InstrumentKind, NoteInit};
        let mut doc = Document::new(Project::new("callback allocation"));
        let channel = ChannelId(
            doc.dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::SubtractiveSynth),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
        );
        let pattern = doc.project().patterns[0].id;
        doc.dispatch(
            Command::AddNotes {
                pattern,
                channel,
                notes: vec![NoteInit {
                    start: 0,
                    length: 960,
                    key: 60,
                    velocity: Some(0.8),
                    pan: None,
                    expression: None,
                }],
            },
            None,
        )
        .unwrap();
        let controller = crate::Controller::new();
        controller.set_project(doc.project(), &SamplePool::new());
        let mut processor = controller.attach(48000);
        let mut buffer = [0.0; 960];
        processor.process(&mut buffer);
        let request = LaunchRequest {
            track: 0,
            pattern: Some(pattern),
            quantization: LaunchQuantization::Immediate,
        };
        controller.performance_launch(request).unwrap();
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut buffer)),
            0
        );
        controller.performance_launch(request).unwrap();
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut buffer)),
            0
        );
        let mut faster = doc.project().clone();
        faster.settings.tempo_bpm = 180.0;
        controller.set_project(&faster, &SamplePool::new());
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut buffer)),
            0
        );
        controller.performance_stop_all();
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut buffer)),
            0
        );
    }
}
