//! The live analyzer through the native session: real slots, real
//! selections, a processor run by hand, and the events exactly as the JSON
//! text a window is sent.
//!
//! The fixture tells its sources apart by ear: track A is panned hard left
//! and plays a low note, track B hard right and a high one. So A has a
//! silent right side, B a silent left side, and the mix has both.
//!
//! Time is the test's own: every step runs some audio and then one service
//! tick 17 ms later than the last.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use windfall_engine::Processor;
use windfall_ipc::{
    ANALYZER_EVENT_BYTES, AnalyzerAck, AnalyzerEvent, AnalyzerFrame, AnalyzerObservation,
    AnalyzerPhase, AnalyzerReason, AnalyzerSelect, AnalyzerSource, AnalyzerStatus, AnalyzerWant,
};
use windfall_project::{
    ChannelId, Command, EffectId, EffectKind, InstrumentKind, MixerTrackPatch, TrackId,
};

use super::{PATIENCE, Rig, SAMPLE_RATE};
use crate::session::{ACK_TIMEOUT, AUDIO_TIMEOUT, AnalyzerSender};
use crate::sync::lock;

const WINDOW: &str = "main";
const TICK: Duration = Duration::from_millis(17);
/// Track B's note: C7, 2093 Hz, so that even an oscillator two octaves
/// under it is far above the 400 Hz the low note is measured against.
const HIGH_KEY: u8 = 96;

/// The analyzer's quotas are counted for the whole process, so these tests
/// take turns among themselves.
static TURN: Mutex<()> = Mutex::new(());

fn turn() -> MutexGuard<'static, ()> {
    TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What a window's slot was sent, as text.
#[derive(Clone, Default)]
struct Wire {
    texts: Arc<Mutex<Vec<String>>>,
    closed: Arc<AtomicBool>,
}

impl Wire {
    fn sender(&self) -> AnalyzerSender {
        let wire = self.clone();
        Box::new(move |text| {
            if wire.closed.load(Ordering::Acquire) {
                return false;
            }
            lock(&wire.texts).push(text.to_owned());
            true
        })
    }

    /// Decodes and removes everything sent so far. Each event is within
    /// the size limit and survives the wire.
    fn take(&self) -> Vec<AnalyzerEvent> {
        std::mem::take(&mut *lock(&self.texts))
            .into_iter()
            .map(|text| {
                assert!(text.len() <= ANALYZER_EVENT_BYTES, "{} bytes", text.len());
                serde_json::from_str(&text).unwrap_or_else(|error| panic!("{error}: {text}"))
            })
            .collect()
    }
}

fn frames(events: &[AnalyzerEvent]) -> Vec<&AnalyzerFrame> {
    events
        .iter()
        .filter_map(|event| match event {
            AnalyzerEvent::Frame(frame) => Some(&**frame),
            AnalyzerEvent::Status(_) => None,
        })
        .collect()
}

fn statuses(events: &[AnalyzerEvent]) -> Vec<&AnalyzerStatus> {
    events
        .iter()
        .filter_map(|event| match event {
            AnalyzerEvent::Status(status) => Some(status),
            AnalyzerEvent::Frame(_) => None,
        })
        .collect()
}

struct Bench {
    rig: Rig,
    now: Instant,
    a: TrackId,
    b: TrackId,
    low: ChannelId,
    high: ChannelId,
}

fn add_synth(rig: &Rig, pan: f32) -> (ChannelId, TrackId) {
    let added = rig
        .session
        .dispatch(
            Command::AddChannel {
                name: None,
                sample: None,
                instrument: Some(InstrumentKind::SubtractiveSynth),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap();
    let (channel, track) = (ChannelId(added.created[0]), TrackId(added.created[1]));
    rig.session
        .dispatch(
            Command::UpdateMixerTrack {
                id: track,
                patch: MixerTrackPatch {
                    pan: Some(pan),
                    ..MixerTrackPatch::default()
                },
            },
            None,
        )
        .unwrap();
    (channel, track)
}

impl Bench {
    fn new() -> Self {
        // Other tests of this binary hold processors too, and only so many
        // streams of one process get capture seats. Wait for one that does.
        let deadline = Instant::now() + PATIENCE;
        let rig = loop {
            let rig = Rig::new();
            if rig.session.analyzer_capability().sample_rate.is_some() {
                break rig;
            }
            drop(rig);
            assert!(Instant::now() < deadline, "no stream with capture seats");
            std::thread::sleep(Duration::from_millis(20));
        };
        let (low, a) = add_synth(&rig, -1.0);
        let (high, b) = add_synth(&rig, 1.0);
        let mut bench = Self {
            rig,
            now: Instant::now(),
            a,
            b,
            low,
            high,
        };
        bench.hold();
        bench.rig.run(4_800);
        bench
    }

    /// Holds a low note on track A and a high one on track B.
    fn hold(&self) {
        self.rig.session.audition_note_on(self.low, 48, 1.0);
        self.rig.session.audition_note_on(self.high, HIGH_KEY, 1.0);
    }

    fn generation(&self) -> u64 {
        self.rig.session.timeline_state().generation
    }

    fn select(&self, slot: &str, request: u32, source: AnalyzerSource) -> AnalyzerStatus {
        self.select_with(slot, request, source, None)
    }

    fn select_with(
        &self,
        slot: &str,
        request: u32,
        source: AnalyzerSource,
        effect: Option<EffectId>,
    ) -> AnalyzerStatus {
        self.rig.session.analyzer_select(
            WINDOW,
            AnalyzerSelect {
                slot: slot.to_owned(),
                request,
                generation: self.generation(),
                source,
                effect,
                want: AnalyzerWant {
                    scope: true,
                    spectrum: true,
                    vector: true,
                    envelope: true,
                },
            },
        )
    }

    /// Moves the clock on by one tick and runs the service, with or without
    /// audio having played in between.
    fn tick(&mut self, audio: bool) {
        if audio {
            self.rig.run(960);
        }
        self.now += TICK;
        self.rig.session.analyzer_tick(self.now);
    }

    fn ack(&self, slot: &str, frame: &AnalyzerFrame) {
        self.rig.session.analyzer_ack(
            WINDOW,
            AnalyzerAck {
                slot: slot.to_owned(),
                sequence: frame.sequence.clone(),
            },
        );
    }

    /// Runs audio and the service for `ticks` ticks, acknowledging every
    /// frame `slot` is sent, and returns what it was sent.
    fn run(&mut self, wire: &Wire, slot: &str, ticks: usize) -> Vec<AnalyzerEvent> {
        let mut all = Vec::new();
        for _ in 0..ticks {
            self.tick(true);
            let events = wire.take();
            for frame in frames(&events) {
                self.ack(slot, frame);
            }
            all.extend(events);
        }
        all
    }

    /// Runs until `slot` has been sent a frame for `request`, and returns
    /// everything sent on the way, that frame last.
    fn until_frame(&mut self, wire: &Wire, slot: &str, request: u32) -> Vec<AnalyzerEvent> {
        let mut all = Vec::new();
        // A process whose other tests hold every analyzer has this one
        // wait: the service keeps asking, and real time has to pass.
        let deadline = Instant::now() + PATIENCE;
        loop {
            all.extend(self.run(wire, slot, 1));
            if frames(&all).iter().any(|frame| frame.request == request) {
                return all;
            }
            assert!(
                Instant::now() < deadline,
                "no frame for {request}: {all:#?}"
            );
            if statuses(&all)
                .last()
                .is_some_and(|status| status.reason == Some(AnalyzerReason::Preparation))
            {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

fn track(track: TrackId) -> AnalyzerSource {
    AnalyzerSource::TrackPostFader { track }
}

fn sounding(frame: &AnalyzerFrame, side: usize) -> bool {
    let level = &frame.levels[side];
    level.valid && level.peak.is_some_and(|peak| peak > 1.0e-3) && !level.silent
}

fn silent(frame: &AnalyzerFrame, side: usize) -> bool {
    let level = &frame.levels[side];
    level.valid && level.silent && level.peak == Some(0.0) && level.peak_db.is_none()
}

/// The share of a frame's spectrum power that lies under 400 Hz, over the
/// sides that sound.
fn low_share(frame: &AnalyzerFrame) -> f64 {
    let spectrum = frame.spectrum.as_ref().expect("a spectrum was asked for");
    let (mut low, mut all) = (0.0_f64, 0.0_f64);
    for bins in [&spectrum.left, &spectrum.right] {
        for (bin, power) in bins.iter().enumerate() {
            all += f64::from(*power);
            if bin as f32 * spectrum.bin_hz < 400.0 {
                low += f64::from(*power);
            }
        }
    }
    low / all
}

fn number(text: &str) -> u64 {
    text.parse()
        .unwrap_or_else(|_| panic!("{text} is not a count"))
}

#[test]
fn each_slot_is_sent_its_own_source_and_no_frame_of_an_earlier_choice() {
    let _turn = turn();
    let mut bench = Bench::new();
    let session = bench.rig.session.clone();
    let capability = session.analyzer_capability();
    assert!(capability.native);
    assert_eq!(capability.sample_rate, Some(SAMPLE_RATE));
    assert_eq!((capability.fft_size, capability.bins), (1024, 513));

    // A slot that never subscribed is told so.
    let stranger = bench.select("scope", 1, AnalyzerSource::MixOutput);
    assert_eq!(stranger.phase, AnalyzerPhase::Refused);
    assert_eq!(stranger.reason, Some(AnalyzerReason::NotSubscribed));

    let scope = Wire::default();
    let subscribed = session.analyzer_subscribe(WINDOW, "scope", scope.sender());
    assert_eq!(subscribed.phase, AnalyzerPhase::Idle);
    assert!(
        bench.run(&scope, "scope", 4).is_empty(),
        "nothing is sent before a selection"
    );

    // Track A: hard left, low.
    let chosen = bench.select("scope", 1, track(bench.a));
    assert_eq!(chosen.phase, AnalyzerPhase::Preparing);
    assert_eq!(chosen.request, 1);
    assert_eq!(chosen.source, Some(track(bench.a)));
    assert!(chosen.source_name.is_some());
    let events = bench.until_frame(&scope, "scope", 1);
    // Selected is not live: the slot hears it is waiting for audio before
    // it is sent a frame, and live once real audio has been analyzed.
    let told: Vec<AnalyzerPhase> = statuses(&events)
        .iter()
        .map(|status| status.phase)
        .collect();
    assert!(told.contains(&AnalyzerPhase::AwaitingAudio), "{told:?}");
    let on_a = frames(&bench.run(&scope, "scope", 6))
        .last()
        .map(|frame| (*frame).clone())
        .expect("frames keep coming");
    assert_eq!(on_a.request, 1);
    assert!(sounding(&on_a, 0), "{:?}", on_a.levels);
    assert!(silent(&on_a, 1), "{:?}", on_a.levels);
    assert!(low_share(&on_a) > 0.2, "{}", low_share(&on_a));
    // The window is 1024 frames of the stream's own clock.
    assert_eq!(on_a.clock.sample_rate, SAMPLE_RATE);
    assert_eq!(
        number(&on_a.clock.end_frame) - number(&on_a.clock.first_frame),
        1024
    );
    assert_eq!(on_a.clock.device_latency_frames, None);
    let scoped = on_a.scope.as_ref().unwrap();
    assert_eq!((scoped.left.len(), scoped.right.len()), (1024, 1024));
    assert!(scoped.right.iter().all(|sample| *sample == Some(0.0)));
    assert!(
        scoped
            .left
            .iter()
            .any(|sample| sample.is_some_and(|s| s.abs() > 1.0e-3))
    );
    assert!(
        on_a.vector
            .as_ref()
            .is_some_and(|points| !points.is_empty())
    );
    assert_eq!(on_a.envelope.as_ref().unwrap().bins.len(), 128);
    assert_eq!(on_a.spectrum.as_ref().unwrap().left.len(), 513);
    assert_eq!(on_a.spectrum.as_ref().unwrap().bands.len(), 64);
    assert!(on_a.observations.is_empty());
    assert_eq!(on_a.reduction_db, None);

    // The same slot moves to track B. From the answer on, nothing with the
    // old request or the old selection is sent, and what arrives is B: a
    // silent left side, a sounding right one, a high note.
    let chosen = bench.select("scope", 2, track(bench.b));
    assert_eq!(chosen.request, 2);
    let mut later = bench.until_frame(&scope, "scope", 2);
    later.extend(bench.run(&scope, "scope", 6));
    for frame in frames(&later) {
        assert_eq!(frame.request, 2);
        assert_ne!(frame.ticket.selection, on_a.ticket.selection);
        assert!(silent(frame, 0), "{:?}", frame.levels);
        assert!(sounding(frame, 1), "{:?}", frame.levels);
    }
    let on_b = (*frames(&later).last().unwrap()).clone();
    assert!(low_share(&on_b) < 0.02, "{}", low_share(&on_b));
    assert!(
        frames(&later)[0].gap,
        "the first frame of a selection joins nothing"
    );
    // A late acknowledgement of the old selection's frame changes nothing.
    bench.ack("scope", &on_a);

    // A second slot looks at the mix at the same time: both sides sound.
    let mix = Wire::default();
    session.analyzer_subscribe(WINDOW, "mix", mix.sender());
    bench.select("mix", 1, AnalyzerSource::MixOutput);
    let events = bench.until_frame(&mix, "mix", 1);
    let on_mix = (*frames(&events).last().unwrap()).clone();
    assert!(
        sounding(&on_mix, 0) && sounding(&on_mix, 1),
        "{:?}",
        on_mix.levels
    );
    assert_ne!(on_mix.ticket.selection, on_b.ticket.selection);
    assert_ne!(on_mix.ticket.subscription, on_b.ticket.subscription);
    assert_eq!(on_mix.ticket.stream, on_b.ticket.stream);
    assert_eq!(on_mix.ticket.project, bench.generation().to_string());
    // The first slot was not disturbed by it.
    let still = bench.run(&scope, "scope", 4);
    assert!(
        frames(&still)
            .iter()
            .all(|frame| silent(frame, 0) && sounding(frame, 1))
    );
    mix.take();

    // A third distinct source has no seat. It is refused, and neither of
    // the two that are running is taken away for it.
    let third = Wire::default();
    session.analyzer_subscribe(WINDOW, "third", third.sender());
    let refused = bench.select("third", 1, track(bench.a));
    assert_eq!(refused.phase, AnalyzerPhase::Refused);
    assert_eq!(refused.reason, Some(AnalyzerReason::SourceCapacity));
    let still = bench.run(&scope, "scope", 4);
    assert!(!frames(&still).is_empty());
    assert!(third.take().is_empty());
    // The same source as another slot is shared: one selection, two tickets.
    let shared = bench.select("third", 2, AnalyzerSource::MixOutput);
    assert_ne!(shared.phase, AnalyzerPhase::Refused);
    let events = bench.until_frame(&third, "third", 2);
    let on_shared = (*frames(&events).last().unwrap()).clone();
    assert_eq!(on_shared.ticket.selection, on_mix.ticket.selection);
    assert_ne!(on_shared.ticket.subscription, on_mix.ticket.subscription);

    // A fifth slot is one too many.
    session.analyzer_subscribe(WINDOW, "fourth", Wire::default().sender());
    let full = session.analyzer_subscribe(WINDOW, "fifth", Wire::default().sender());
    assert_eq!(full.reason, Some(AnalyzerReason::SubscriptionCapacity));
    assert_eq!(
        session
            .analyzer_subscribe(WINDOW, "not a slot", Wire::default().sender())
            .reason,
        Some(AnalyzerReason::InvalidSlot)
    );

    // A window that closes takes its slots with it.
    session.analyzer_window_closed(WINDOW);
    bench.tick(true);
    assert!(scope.take().is_empty());
    assert_eq!(
        bench.select("scope", 3, AnalyzerSource::MixOutput).reason,
        Some(AnalyzerReason::NotSubscribed)
    );
}

#[test]
fn a_seek_a_deleted_track_a_new_project_and_a_new_stream_each_end_what_was_shown() {
    let _turn = turn();
    let mut bench = Bench::new();
    let session = bench.rig.session.clone();
    let wire = Wire::default();
    session.analyzer_subscribe(WINDOW, "scope", wire.sender());

    // A seek: the analysis breaks, and the frames after it start a new
    // epoch whose window lies wholly after the break.
    bench.select("scope", 1, track(bench.a));
    let events = bench.until_frame(&wire, "scope", 1);
    let before = (*frames(&events).last().unwrap()).clone();
    session.transport_seek(960.0);
    let events = bench.run(&wire, "scope", 12);
    let after: Vec<&AnalyzerFrame> = frames(&events)
        .into_iter()
        .filter(|frame| number(&frame.clock.epoch) > number(&before.clock.epoch))
        .collect();
    assert!(!after.is_empty(), "{events:#?}");
    assert!(after[0].gap);
    assert!(number(&after[0].clock.first_frame) >= number(&before.clock.end_frame));
    assert!(after.iter().skip(1).all(|frame| !frame.gap));
    assert!(
        number(&after[0].counters.resets) > number(&before.counters.resets),
        "{:?}",
        after[0].counters
    );

    // Track B is selected and then deleted: the slot is told the source is
    // gone, and is sent no frame of it afterwards.
    bench.select("scope", 2, track(bench.b));
    bench.until_frame(&wire, "scope", 2);
    session
        .dispatch(Command::RemoveMixerTrack { id: bench.b }, None)
        .unwrap();
    let events = bench.run(&wire, "scope", 8);
    let told = statuses(&events);
    assert!(
        told.iter()
            .any(|status| status.phase == AnalyzerPhase::Refused
                && status.reason == Some(AnalyzerReason::SourceMissing)),
        "{events:#?}"
    );
    // Whatever was already on its way before the edit is all there is.
    let events = bench.run(&wire, "scope", 8);
    assert!(frames(&events).is_empty(), "{events:#?}");
    let gone = bench.select("scope", 3, track(bench.b));
    assert_eq!(gone.reason, Some(AnalyzerReason::SourceMissing));

    // A new project replaces this one. Every slot is told, nothing of the
    // old project is sent again, and a selection made for the old project
    // is refused.
    bench.select("scope", 4, AnalyzerSource::MixOutput);
    let events = bench.until_frame(&wire, "scope", 4);
    let old = (*frames(&events).last().unwrap()).clone();
    let old_generation = bench.generation();
    session.project_new().unwrap();
    assert_ne!(bench.generation(), old_generation);
    let events = bench.run(&wire, "scope", 8);
    assert!(frames(&events).is_empty(), "{events:#?}");
    assert!(
        statuses(&events)
            .iter()
            .any(|status| status.reason == Some(AnalyzerReason::ProjectReplaced)),
        "{events:#?}"
    );
    let stale = session.analyzer_select(
        WINDOW,
        AnalyzerSelect {
            slot: "scope".to_owned(),
            request: 5,
            generation: old_generation,
            source: AnalyzerSource::MixOutput,
            effect: None,
            want: AnalyzerWant::default(),
        },
    );
    assert_eq!(stale.reason, Some(AnalyzerReason::StaleProject));
    bench.select("scope", 6, AnalyzerSource::MixOutput);
    let events = bench.until_frame(&wire, "scope", 6);
    let new = (*frames(&events).last().unwrap()).clone();
    assert_eq!(new.ticket.project, bench.generation().to_string());
    assert_ne!(new.ticket.selection, old.ticket.selection);

    // The stream closes and reopens at another rate. In between the slot
    // waits for a stream; afterwards its frames carry the new stream, the
    // new rate and a new selection.
    bench.rig.processor = Processor::new(SAMPLE_RATE).0;
    bench.tick(false);
    let events = wire.take();
    assert!(
        statuses(&events)
            .iter()
            .any(|status| status.phase == AnalyzerPhase::WaitingForStream),
        "{events:#?}"
    );
    assert_eq!(session.analyzer_capability().sample_rate, None);
    bench.rig.processor = Processor::reopen(session.controller(), 44_100);
    let events = bench.until_frame(&wire, "scope", 6);
    let reopened = (*frames(&events).last().unwrap()).clone();
    assert_eq!(reopened.clock.sample_rate, 44_100);
    assert_ne!(reopened.ticket.stream, new.ticket.stream);
    assert_ne!(reopened.ticket.selection, new.ticket.selection);
    assert!(frames(&events)[0].gap);
    assert!(
        statuses(&events)
            .iter()
            .any(|status| status.sample_rate == Some(44_100)),
        "{events:#?}"
    );
}

#[test]
fn a_view_or_a_stream_that_stops_keeping_up_is_stalled_openly_and_resumes_under_a_new_ticket() {
    let _turn = turn();
    let mut bench = Bench::new();
    let session = bench.rig.session.clone();
    let wire = Wire::default();
    session.analyzer_subscribe(WINDOW, "scope", wire.sender());
    bench.select("scope", 1, AnalyzerSource::MixOutput);
    let events = bench.until_frame(&wire, "scope", 1);
    let live = (*frames(&events).last().unwrap()).clone();

    // The view stops acknowledging. It holds one frame and is sent no more,
    // however long the audio goes on.
    let mut held = None;
    for _ in 0..8 {
        bench.tick(true);
        let events = wire.take();
        if let Some(frame) = frames(&events).first() {
            assert!(held.is_none(), "a second frame was sent unacknowledged");
            held = Some((*frame).clone());
        }
    }
    let held = held.expect("one frame is sent");
    let stalling = (ACK_TIMEOUT.as_millis() / TICK.as_millis()) as usize + 20;
    for _ in 0..stalling {
        bench.tick(true);
    }
    assert!(
        wire.take().is_empty(),
        "a stalled view is sent nothing at all"
    );
    // An acknowledgement of some other frame does not wake it.
    session.analyzer_ack(
        WINDOW,
        AnalyzerAck {
            slot: "scope".to_owned(),
            sequence: (number(&held.sequence) + 7).to_string(),
        },
    );
    bench.tick(true);
    assert!(wire.take().is_empty());
    // The right one does: it hears that it was stalled, and what follows
    // has another ticket, joins nothing, and counts what was skipped.
    bench.ack("scope", &held);
    let events = bench.run(&wire, "scope", 8);
    let first = statuses(&events)[0];
    assert_eq!(first.phase, AnalyzerPhase::Stalled);
    assert_eq!(first.reason, Some(AnalyzerReason::ViewStalled));
    let resumed = frames(&events);
    assert!(resumed[0].gap);
    assert_ne!(resumed[0].ticket.subscription, live.ticket.subscription);
    assert!(number(&resumed[0].counters.skipped_events) > 0);
    assert!(
        statuses(&events)
            .iter()
            .any(|status| status.phase == AnalyzerPhase::Live),
        "{events:#?}"
    );

    // Now the audio stops arriving while the view keeps up.
    let before = (*resumed.last().unwrap()).clone();
    let silent_ticks = (AUDIO_TIMEOUT.as_millis() / TICK.as_millis()) as usize + 4;
    let mut events = Vec::new();
    for _ in 0..silent_ticks {
        bench.tick(false);
        events.extend(wire.take());
    }
    assert!(
        frames(&events).is_empty(),
        "the same window is never sent twice"
    );
    assert!(
        statuses(&events)
            .iter()
            .any(|status| status.phase == AnalyzerPhase::Stalled
                && status.reason == Some(AnalyzerReason::AudioStalled)),
        "{events:#?}"
    );
    // It comes back: what was shown before the stall is not continued.
    let events = bench.until_frame(&wire, "scope", 1);
    let back = frames(&events)[0];
    assert!(back.gap);
    assert_ne!(back.ticket.selection, before.ticket.selection);
    assert!(number(&back.clock.first_frame) >= number(&before.clock.end_frame));

    // A window that is gone is forgotten the first time it cannot be sent.
    wire.closed.store(true, Ordering::Release);
    bench.run(&wire, "scope", 6);
    assert_eq!(
        bench.select("scope", 2, AnalyzerSource::MixOutput).reason,
        Some(AnalyzerReason::NotSubscribed)
    );
}

/// Sets the setting of an effect that has this id among the descriptors of
/// its kind.
fn set(bench: &Bench, track: TrackId, effect: EffectId, kind: EffectKind, id: &str, value: f32) {
    let settings = kind.descriptors();
    let param = settings.iter().position(|info| info.id == id).unwrap();
    bench
        .rig
        .session
        .dispatch(
            Command::SetEffectParam {
                track,
                effect,
                param: param as u32,
                value,
            },
            None,
        )
        .unwrap();
}

fn add_effect(bench: &Bench, track: TrackId, kind: EffectKind) -> EffectId {
    let command = Command::AddEffect {
        track,
        kind,
        index: None,
    };
    EffectId(bench.rig.session.dispatch(command, None).unwrap().created[0])
}

#[test]
fn an_effect_slot_is_sent_the_limiters_history_and_the_tuners_calibrated_reading() {
    let _turn = turn();
    let mut bench = Bench::new();
    let session = bench.rig.session.clone();
    let wire = Wire::default();
    session.analyzer_subscribe(WINDOW, "effect", wire.sender());

    // A limiter driven hard on track A.
    let limiter = add_effect(&bench, bench.a, EffectKind::Limiter);
    set(
        &bench,
        bench.a,
        limiter,
        EffectKind::Limiter,
        "inputGainDb",
        24.0,
    );
    set(
        &bench,
        bench.a,
        limiter,
        EffectKind::Limiter,
        "ceilingDb",
        -12.0,
    );
    // An effect's readings come with its own track only.
    let wrong = bench.select_with("effect", 1, track(bench.b), Some(limiter));
    assert_eq!(wrong.reason, Some(AnalyzerReason::EffectNotOnSource));
    let wrong = bench.select_with("effect", 2, AnalyzerSource::MixOutput, Some(limiter));
    assert_eq!(wrong.reason, Some(AnalyzerReason::EffectNotOnSource));
    let wrong = bench.select_with("effect", 3, track(bench.a), Some(EffectId(999_999)));
    assert_eq!(wrong.reason, Some(AnalyzerReason::EffectMissing));

    let chosen = bench.select_with("effect", 4, track(bench.a), Some(limiter));
    assert_eq!(chosen.effect, Some(limiter));
    let mut events = bench.until_frame(&wire, "effect", 4);
    events.extend(bench.run(&wire, "effect", 20));
    let sent = frames(&events);
    // Block after block, end to end on the engine's clock, across frames.
    let mut next = None;
    let mut deepest = 0.0_f32;
    let mut blocks = 0;
    for frame in &sent {
        if frame.gap {
            next = None;
        }
        for observation in &frame.observations {
            let AnalyzerObservation::Dynamics {
                first_frame,
                frames,
                input_peak,
                output_peak,
                reduction_db,
                gate_db,
            } = observation
            else {
                panic!("a limiter reports dynamics: {observation:?}");
            };
            if let Some(next) = next {
                assert_eq!(number(first_frame), next);
            }
            next = Some(number(first_frame) + u64::from(*frames));
            assert!(*output_peak <= 10.0_f32.powf(-12.0 / 20.0) * 1.001);
            assert!(input_peak.is_finite() && *input_peak >= 0.0);
            assert_eq!(*gate_db, 0.0);
            deepest = deepest.max(*reduction_db);
            blocks += 1;
        }
    }
    assert!(blocks > 40, "{blocks} blocks");
    assert!(deepest > 3.0, "the limiter turned down {deepest} dB");
    // The window's own figure is the limiter's, never worked out from audio.
    let last = sent.last().unwrap();
    assert!(
        last.reduction_db
            .is_some_and(|db| db > 3.0 && db <= deepest + 0.01)
    );
    assert_eq!(last.counters.dropped_observations, "0");

    // The limiter is removed: the slot is told, and is sent no more of it.
    session
        .dispatch(
            Command::RemoveEffect {
                track: bench.a,
                effect: limiter,
            },
            None,
        )
        .unwrap();
    let events = bench.run(&wire, "effect", 8);
    assert!(
        statuses(&events)
            .iter()
            .any(|status| status.reason == Some(AnalyzerReason::EffectMissing)),
        "{events:#?}"
    );
    assert!(frames(&bench.run(&wire, "effect", 6)).is_empty());

    // A tuner on track B, calibrated to A4 = 432 Hz, hears A4 at 440 Hz.
    session.audition_note_off(bench.high, HIGH_KEY);
    session.audition_note_on(bench.high, 69, 1.0);
    let tuner = add_effect(&bench, bench.b, EffectKind::Tuner);
    set(
        &bench,
        bench.b,
        tuner,
        EffectKind::Tuner,
        "referenceHz",
        432.0,
    );
    bench.select_with("effect", 5, track(bench.b), Some(tuner));
    let mut events = bench.until_frame(&wire, "effect", 5);
    events.extend(bench.run(&wire, "effect", 40));
    let reading = frames(&events)
        .iter()
        .flat_map(|frame| frame.observations.iter())
        .filter_map(|observation| match observation {
            AnalyzerObservation::Tuner {
                midi_note: Some(note),
                cents: Some(cents),
                frequency_hz: Some(hz),
                confidence,
                ..
            } => Some((*note, *cents, *hz, *confidence)),
            _ => None,
        })
        .last()
        .expect("the tuner found a pitch");
    // The synth's own patch decides which octave of the A is strongest, so
    // the octave is left open: an A, that many cents sharp of 432 tuning.
    let sharp = 1200.0 * (440.0_f32 / 432.0).log2();
    assert_eq!(reading.0 % 12, 9, "{reading:?}");
    assert!(
        (reading.1 - sharp).abs() < 5.0,
        "{reading:?} against {sharp}"
    );
    let octaves = (reading.2 / 440.0).log2();
    assert!((octaves - octaves.round()).abs() < 0.01, "{reading:?}");
    assert!(reading.3 > 0.5, "{reading:?}");
}
