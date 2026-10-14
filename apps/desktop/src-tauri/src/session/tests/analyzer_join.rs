//! Captures the real channel text, without a window or a device. The UI suite
//! decodes these strings; it does not create analyzer frames of its own.
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};
use windfall_codec::{WavSampleFormat, write_wav};
use windfall_core::AudioBuffer;
use windfall_ipc::{PlayMode, TransportPatch};

use super::*;
use crate::paths;
use crate::session::ClipPlace;

const SLOT: &str = "scope";
const CHECKED: &str = "../src/features/analyzers/fixtures/native-join.json";

struct Capture {
    wire: Wire,
    realtime: Arc<Mutex<Vec<String>>>,
    steps: Vec<Value>,
}

impl Capture {
    fn new(bench: &Bench) -> Self {
        let wire = Wire::default();
        bench
            .rig
            .session
            .analyzer_subscribe(WINDOW, SLOT, wire.sender());
        Self {
            wire,
            realtime: realtime_wire(&bench.rig, WINDOW),
            steps: Vec::new(),
        }
    }

    fn take(&mut self, bench: &Bench) -> (Vec<String>, Vec<String>) {
        let texts = std::mem::take(&mut *lock(&self.wire.texts));
        for text in &texts {
            assert!(text.len() <= ANALYZER_EVENT_BYTES);
            if let AnalyzerEvent::Frame(frame) = serde_json::from_str(text).unwrap() {
                bench.ack(SLOT, &frame);
            }
        }
        bench.rig.session.realtime_tick_at(bench.now);
        (texts, std::mem::take(&mut *lock(&self.realtime)))
    }

    fn step(&mut self, bench: &mut Bench, name: &str, request: u32, ticks: usize) {
        let mut analyzer = Vec::new();
        let mut realtime = Vec::new();
        for _ in 0..ticks {
            bench.tick(true);
            let (a, r) = self.take(bench);
            analyzer.extend(a);
            realtime.extend(r);
        }
        self.steps
            .push(json!({"name":name,"request":request,"analyzer":analyzer,"realtime":realtime}));
    }

    fn select(
        &mut self,
        bench: &mut Bench,
        name: &str,
        request: u32,
        source: AnalyzerSource,
        effect: Option<EffectId>,
    ) {
        let select = AnalyzerSelect {
            slot: SLOT.to_owned(),
            request,
            generation: bench.generation(),
            source,
            effect,
            want: AnalyzerWant {
                scope: true,
                spectrum: true,
                vector: true,
                envelope: true,
            },
        };
        let answer = bench
            .rig
            .session
            .analyzer_select_at(WINDOW, select.clone(), bench.now);
        assert_ne!(answer.phase, AnalyzerPhase::Refused, "{answer:?}");
        self.step(bench, name, request, 12);
        let step = self.steps.last_mut().unwrap();
        step["select"] = serde_json::to_value(select).unwrap();
        // Selection replies go through the same frontend standing path as events.
        step["answer"] = serde_json::to_value(answer).unwrap();
        assert!(
            step["analyzer"].as_array().unwrap().iter().any(|text| {
                let event: AnalyzerEvent = serde_json::from_str(text.as_str().unwrap()).unwrap();
                matches!(event, AnalyzerEvent::Frame(_))
            }),
            "no audio in {name}"
        );
    }
}

/// Samples are our own pure tones, loaded through the ordinary native clip
/// preparation path. Their frequencies/amplitudes are independent references.
fn tone(bench: &Bench, name: &str, hz: f32, amplitude: f32, track: TrackId) {
    let pcm: Vec<f32> = (0..SAMPLE_RATE * 12)
        .flat_map(|frame| {
            let value =
                amplitude * (std::f32::consts::TAU * hz * frame as f32 / SAMPLE_RATE as f32).sin();
            [value, value]
        })
        .collect();
    let path = bench.rig.folder.path().join(name);
    write_wav(
        &path,
        &AudioBuffer::from_interleaved(SAMPLE_RATE, 2, pcm),
        WavSampleFormat::Float32,
    )
    .unwrap();
    bench
        .rig
        .session
        .add_audio_clip_from_file(
            &paths::display(&path),
            ClipPlace {
                track: None,
                start: 0,
                mixer_track: Some(track),
            },
        )
        .unwrap();
}

fn capture() -> Value {
    let mut bench = Bench::new();
    let session = bench.rig.session.clone();
    session.audition_note_off(bench.low, 48);
    session.audition_note_off(bench.high, HIGH_KEY);
    bench.rig.run(48_000);
    tone(&bench, "left-440.wav", 440.0, 0.8, bench.a);
    tone(&bench, "right-1875.wav", 1875.0, 0.2, bench.b);
    session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Song),
            loop_song: Some(true),
            ..Default::default()
        })
        .unwrap();
    session.transport_play().unwrap();
    let limiter = add_effect(&bench, bench.a, EffectKind::Limiter);
    set(
        &bench,
        bench.a,
        limiter,
        EffectKind::Limiter,
        "ceilingDb",
        -12.0,
    );
    let tuner = add_effect(&bench, bench.a, EffectKind::Tuner);
    set(
        &bench,
        bench.a,
        tuner,
        EffectKind::Tuner,
        "referenceHz",
        432.0,
    );
    let snapshot = session.document_snapshot();
    session
        .mixer_waveform_tracks(
            vec![bench.a, bench.b],
            bench.generation(),
            snapshot.revision,
        )
        .unwrap();
    let mut joined = Capture::new(&bench);
    let (a, b) = (bench.a, bench.b);
    joined.select(&mut bench, "left", 1, track(a), None);
    joined.select(&mut bench, "right", 2, track(b), None);
    joined.select(&mut bench, "mix", 3, AnalyzerSource::MixOutput, None);
    session.transport_seek(960.0);
    joined.step(&mut bench, "seek", 3, 12);
    // Loop within a short checked region, so the captured realtime feed includes
    // a backwards tick while playing rather than only a fabricated clock reset.
    let guard = session.timeline_state();
    let revision = session.document_snapshot().revision;
    session
        .timeline_region(
            Some(windfall_project::TickRange {
                start: 960,
                end: 1920,
            }),
            guard.generation,
            revision,
        )
        .unwrap();
    joined.step(&mut bench, "loop", 3, 40);
    joined.select(&mut bench, "limiter", 4, track(a), Some(limiter));
    // Drive its release with actual lower audio, leaving the slot's owner intact.
    session.transport_stop();
    joined.step(&mut bench, "decay", 4, 30);
    session.transport_play().unwrap();
    joined.select(&mut bench, "tuner", 5, track(a), Some(tuner));
    joined.step(&mut bench, "tuner-settled", 5, 30);
    session
        .dispatch(
            Command::MoveEffect {
                track: a,
                effect: tuner,
                to_track: Some(b),
                index: 0,
            },
            None,
        )
        .unwrap();
    joined.step(&mut bench, "tuner-old-source", 5, 6);
    joined.select(&mut bench, "tuner-new-source", 6, track(b), Some(tuner));
    joined.select(&mut bench, "pdc", 7, track(a), None);
    session
        .dispatch(
            Command::UpdateMixerTrack {
                id: a,
                patch: MixerTrackPatch {
                    latency_offset_ms: Some(10.0),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    joined.step(&mut bench, "pdc-change", 7, 12);
    // A UI stall withholds the exact ack. Native keeps one outstanding frame.
    bench.tick(true);
    let held = std::mem::take(&mut *lock(&joined.wire.texts));
    let mut last = None;
    for _ in 0..10 {
        if last.is_some() {
            break;
        }
        bench.tick(true);
        for text in std::mem::take(&mut *lock(&joined.wire.texts))
            .into_iter()
            .chain(held.clone())
        {
            if let AnalyzerEvent::Frame(frame) = serde_json::from_str(&text).unwrap() {
                last = Some(frame);
            }
        }
    }
    let held = last.expect("a frame whose ack the window withholds");
    for _ in 0..140 {
        bench.tick(true);
    }
    // Before ACK_TIMEOUT, source standing may change without sending another
    // frame. Once stalled, neither frames nor statuses may accumulate.
    let standing = std::mem::take(&mut *lock(&joined.wire.texts));
    for text in standing {
        assert!(
            matches!(serde_json::from_str::<AnalyzerEvent>(&text).unwrap(),
            AnalyzerEvent::Status(status) if status.reason.is_none()
                && matches!(status.phase, AnalyzerPhase::AwaitingAudio | AnalyzerPhase::Live)
                && status.ticket.as_ref() == Some(&held.ticket))
        );
    }
    for _ in 0..10 {
        bench.tick(true);
    }
    assert!(lock(&joined.wire.texts).is_empty());
    bench.ack(SLOT, &held);
    joined.step(&mut bench, "ui-recovery", 7, 12);
    assert!(
        joined.steps.last().unwrap()["analyzer"]
            .as_array()
            .unwrap()
            .iter()
            .any(|text| text.as_str().unwrap().contains("viewStalled"))
    );
    for _ in 0..40 {
        bench.tick(false);
    }
    let (analyzer, realtime) = joined.take(&bench);
    joined
        .steps
        .push(json!({"name":"audio-stall","request":7,"analyzer":analyzer,"realtime":realtime}));
    joined.step(&mut bench, "audio-recovery", 7, 12);
    bench.rig.processor = Processor::new(SAMPLE_RATE).0;
    bench.tick(false);
    bench.rig.processor = Processor::reopen(session.controller(), 44_100);
    // Reopening retires the old stream and stops transport. Resume the new
    // stream so subsequent source-lifecycle observations contain real audio.
    session.transport_play().unwrap();
    joined.step(&mut bench, "rate", 7, 20);
    joined.select(&mut bench, "invalid-source", 8, track(b), None);
    let bad_sample = snapshot
        .project
        .samples
        .iter()
        .find(|sample| sample.name == "right-1875")
        .expect("our imported tone")
        .id;
    session.state().pool.insert(
        bad_sample,
        AudioBuffer::from_interleaved(SAMPLE_RATE, 2, vec![f32::NAN; SAMPLE_RATE as usize * 24]),
    );
    session
        .dispatch(
            Command::UpdateMixerTrack {
                id: b,
                patch: MixerTrackPatch {
                    volume: Some(0.95),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    joined.step(&mut bench, "invalid", 8, 12);
    joined.select(&mut bench, "before-delete", 9, track(a), None);
    session
        .dispatch(Command::RemoveMixerTrack { id: a }, None)
        .unwrap();
    joined.step(&mut bench, "deleted", 9, 10);
    let (_, replacement) = add_synth(&bench.rig, -1.0);
    tone(&bench, "replacement-880.wav", 880.0, 0.3, replacement);
    joined.select(&mut bench, "replacement", 10, track(replacement), None);
    session.project_new().unwrap();
    let loaded = bench
        .rig
        .events
        .take()
        .into_iter()
        .find(|event| matches!(event, crate::events::Event::ProjectLoaded(_)))
        .expect("native project-loaded event")
        .payload()
        .unwrap();
    joined.step(&mut bench, "project-replaced", 10, 10);
    let renewed = session.analyzer_select_at(
        WINDOW,
        AnalyzerSelect {
            slot: SLOT.to_owned(),
            request: 11,
            generation: bench.generation(),
            source: track(replacement),
            effect: None,
            want: AnalyzerWant {
                scope: true,
                spectrum: true,
                vector: true,
                envelope: true,
            },
        },
        bench.now,
    );
    assert_eq!(renewed.reason, Some(AnalyzerReason::SourceMissing));
    let last = joined.steps.last_mut().unwrap();
    last["projectLoaded"] = json!(loaded);
    last["answer"] = serde_json::to_value(renewed).unwrap();
    json!({"schema":1,"project":snapshot.project,"tracks":[a,b],"limiter":limiter,"tuner":tuner,"steps":joined.steps})
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// Compare all payloads, with only process-issued identities and performance
/// counters removed. Wire text in the fixture itself is never rewritten.
fn comparable(mut value: Value) -> Value {
    for sample in value["project"]["samples"].as_array_mut().unwrap() {
        sample.as_object_mut().unwrap().remove("path");
    }
    if let Some(steps) = value["steps"].as_array_mut() {
        for step in steps {
            if let Some(ticket) = step
                .get_mut("answer")
                .and_then(|answer| answer.get_mut("ticket"))
                .and_then(Value::as_object_mut)
            {
                for key in ["service", "subscription", "stream", "selection"] {
                    ticket.remove(key);
                }
            }
            for family in ["analyzer", "realtime"] {
                for text in step[family].as_array_mut().unwrap() {
                    let mut payload: Value = serde_json::from_str(text.as_str().unwrap()).unwrap();
                    if let Some(ticket) = payload.get_mut("ticket").and_then(Value::as_object_mut) {
                        for key in ["service", "subscription", "stream", "selection"] {
                            ticket.remove(key);
                        }
                    }
                    if let Some(object) = payload.as_object_mut() {
                        for key in ["cpu", "xruns"] {
                            object.remove(key);
                        }
                    }
                    *text = payload;
                }
            }
        }
    }
    value
}

#[test]
fn native_window_join_capture_and_checked_fixture_agree() {
    let _turn = turn();
    let captured = capture();
    if let Some(relative) = std::env::var_os("WINDFALL_ANALYZER_CAPTURE_DIR") {
        let relative = PathBuf::from(relative);
        assert_eq!(
            relative.components().next(),
            Some(Component::Normal(std::ffi::OsStr::new("target")))
        );
        assert!(
            relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        );
        let output = root().join(relative);
        fs::create_dir_all(&output).unwrap();
        fs::write(
            output.join("native-join.json"),
            serde_json::to_vec_pretty(&captured).unwrap(),
        )
        .unwrap();
    }
    let checked = Path::new(env!("CARGO_MANIFEST_DIR")).join(CHECKED);
    if checked.exists() {
        let old: Value = serde_json::from_slice(&fs::read(checked).unwrap()).unwrap();
        crate::session::tests::native_capture::assert_matches(
            comparable(old),
            comparable(captured),
            "native capture changed: review and recapture, never silently accept drift",
        );
    } else {
        assert!(
            std::env::var_os("WINDFALL_ANALYZER_CAPTURE_DIR").is_some(),
            "native fixtures have not been captured; follow docs/qa/2026-10-12-p2-analyzers.md"
        );
    }
}
