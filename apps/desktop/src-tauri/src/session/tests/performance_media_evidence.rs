//! Device-free processor -> controller -> Session wire -> mounted media evidence.
//! Capture bytes are produced only by this native test, never authored in TS.
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use windfall_codec::{WavSampleFormat, write_wav};
use windfall_core::AudioBuffer;
use windfall_ipc::{MediaRenderAudio, PlayMode, RealtimeFrame, RecordingSource, TransportPatch};
use windfall_project::{
    ChannelId, ClipInit, ClipStretchQuality, Command, InstrumentKind, NoteInit, SamplerKeyRange,
    SamplerPatch, SamplerStretch, TickRange,
};

use super::{Rig, SAMPLE_RATE};
use crate::session::ClipPlace;
use crate::session::recording::{CaptureHandle, Sink};
use crate::sync::lock;

const RANGE: TickRange = TickRange {
    start: 960,
    end: 1921,
};
const CHECKED: &str = "../src/features/performance-media/fixtures/native-media.json";

fn synth(rig: &Rig, name: &str) -> ChannelId {
    ChannelId(
        rig.session
            .dispatch(
                Command::AddChannel {
                    name: Some(name.into()),
                    sample: None,
                    instrument: Some(InstrumentKind::TripleOsc),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    )
}

fn note_step(
    rig: &mut Rig,
    wire: &Arc<Mutex<Vec<String>>>,
    name: &str,
    frames: usize,
    expected: &[(ChannelId, u8, f32)],
) -> Value {
    let pcm = rig.run(frames);
    if !expected.is_empty() {
        assert!(
            pcm.iter().any(|sample| sample.abs() > 1e-6),
            "{name}: note must reach DSP too"
        );
    }
    rig.session.realtime_tick();
    let texts = std::mem::take(&mut *lock(wire));
    assert_eq!(texts.len(), 1, "exactly one Session publication");
    let frame: RealtimeFrame = serde_json::from_str(&texts[0]).unwrap();
    assert_eq!(frame.active_note_poses.len(), expected.len(), "{name}");
    for &(channel, key, velocity) in expected {
        let pose = frame
            .active_note_poses
            .iter()
            .find(|pose| pose.channel == channel)
            .unwrap();
        assert_eq!(pose.key, key, "{name}");
        assert_eq!(pose.velocity, velocity, "{name}");
    }
    let clock = frame
        .media_clock
        .as_ref()
        .expect("native audio frame clock");
    assert_eq!(clock.sample_rate, SAMPLE_RATE);
    json!({"name":name,"realtime":texts[0]})
}

fn capture_notes() -> Value {
    let mut rig = Rig::new();
    let selected = synth(&rig, "Moss selected");
    let other = synth(&rig, "Moss other");
    rig.session
        .dispatch(
            Command::AddNotes {
                pattern: rig.pattern(),
                channel: selected,
                notes: vec![NoteInit {
                    start: 0,
                    length: 120,
                    key: 60,
                    velocity: Some(1.0),
                    pan: None,
                    expression: None,
                }],
            },
            None,
        )
        .unwrap();
    rig.session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Pattern),
            pattern: Some(rig.pattern()),
            ..Default::default()
        })
        .unwrap();
    let project = rig.project();
    let wire = Arc::new(Mutex::new(Vec::new()));
    let sink = wire.clone();
    rig.session.subscribe_realtime(
        "media-evidence",
        Box::new(move |frame| {
            lock(&sink).push(serde_json::to_string(frame).unwrap());
            true
        }),
    );
    let mut steps = vec![note_step(&mut rig, &wire, "rest", 64, &[])];
    rig.session.transport_play().unwrap();
    steps.push(note_step(
        &mut rig,
        &wire,
        "sequenced-on",
        64,
        &[(selected, 60, 1.0)],
    ));
    // 120 ticks = 3000 frames at 120 BPM. The sequenced owner ends, even
    // while its instrument release tail continues to produce audio.
    steps.push(note_step(&mut rig, &wire, "sequenced-off", 3200, &[]));
    rig.session.transport_stop();
    rig.session.audition_note_on(other, 65, 1.0);
    steps.push(note_step(
        &mut rig,
        &wire,
        "other-live-on",
        64,
        &[(other, 65, 1.0)],
    ));
    rig.session.audition_note_on(selected, 62, 1.0);
    steps.push(note_step(
        &mut rig,
        &wire,
        "selected-live-on",
        64,
        &[(selected, 62, 1.0), (other, 65, 1.0)],
    ));
    rig.session.audition_note_off(selected, 62);
    steps.push(note_step(
        &mut rig,
        &wire,
        "selected-live-off",
        64,
        &[(other, 65, 1.0)],
    ));
    rig.session.audition_note_off(other, 65);
    steps.push(note_step(&mut rig, &wire, "other-live-off", 64, &[]));
    rig.session.audition_note_on(selected, 63, 1.0);
    steps.push(note_step(
        &mut rig,
        &wire,
        "before-replacement",
        64,
        &[(selected, 63, 1.0)],
    ));
    rig.session.project_new().unwrap();
    let loaded = serde_json::to_string(&rig.session.document_snapshot()).unwrap();
    let mut replaced = note_step(&mut rig, &wire, "replacement", 64, &[]);
    replaced["projectLoaded"] = json!(loaded);
    steps.push(replaced);
    json!({"schema":1,"project":project,"selected":selected,"other":other,"steps":steps})
}

fn authored_pcm(frame: usize) -> [f32; 2] {
    let t = frame as f32 / SAMPLE_RATE as f32;
    [
        0.25 * (std::f32::consts::TAU * 440.0 * t).sin(),
        0.125 * (std::f32::consts::TAU * 1875.0 * t).cos(),
    ]
}

fn render_fixture(rig: &Rig) -> MediaRenderAudio {
    let pcm = (0..SAMPLE_RATE as usize * 2)
        .flat_map(authored_pcm)
        .collect();
    let path = rig.file("media-reference.wav");
    write_wav(
        &path,
        &AudioBuffer::from_interleaved(SAMPLE_RATE, 2, pcm),
        WavSampleFormat::Float32,
    )
    .unwrap();
    rig.session
        .add_audio_clip_from_file(
            &path,
            ClipPlace {
                track: None,
                start: 0,
                mixer_track: None,
            },
        )
        .unwrap();
    let guard = rig.session.timeline_state();
    rig.session
        .media_render_audio(RANGE, guard.generation, guard.revision)
        .unwrap()
}

#[test]
fn media_render_audio_returns_exact_region_frame_alignment_and_authored_stereo_pcm() {
    let rig = Rig::new();
    let before = rig.session.controller().transport();
    let audio = render_fixture(&rig);
    assert_eq!((audio.sample_rate, audio.channels), (48_000, 2));
    // Deliberately not divisible by 2000 (one 24-fps frame). No tail or
    // rounding to video cadence may add/drop the final 25 PCM frames.
    assert_eq!(audio.samples.len(), 24_025 * 2);
    assert!(audio.samples.iter().all(|sample| sample.is_finite()));
    // Exclude the real engine's initial seek fade, then compare both sides
    // against the original WAV's absolute source-frame index, not a re-render.
    for frame in 512..24_025 - 512 {
        for (side, expected) in authored_pcm(24_000 + frame).into_iter().enumerate() {
            assert!(
                (audio.samples[frame * 2 + side] - expected).abs() < 2e-6,
                "frame={frame} side={side}"
            );
        }
    }
    assert_eq!(
        rig.session.controller().transport(),
        before,
        "offline render cannot play live audio"
    );
    let guard = rig.session.timeline_state();
    assert_eq!(
        rig.session
            .media_render_audio(RANGE, guard.generation, guard.revision)
            .unwrap()
            .samples,
        audio.samples,
        "repeated snapshots must be deterministic"
    );
}

#[test]
fn media_render_audio_refuses_stale_invalid_over_budget_and_dropped_clip_snapshots() {
    let rig = Rig::new();
    render_fixture(&rig);
    let before = rig.session.document_snapshot();
    let guard = rig.session.timeline_state();
    for (generation, revision) in [
        (guard.generation + 1, guard.revision),
        (guard.generation, guard.revision + 1),
    ] {
        assert!(
            rig.session
                .media_render_audio(RANGE, generation, revision)
                .unwrap_err()
                .contains("changed")
        );
    }
    for range in [
        TickRange {
            start: 960,
            end: 960,
        },
        TickRange {
            start: 961,
            end: 960,
        },
    ] {
        assert!(
            rig.session
                .media_render_audio(range, guard.generation, guard.revision)
                .is_err()
        );
    }
    assert!(
        rig.session
            .media_render_audio(
                TickRange {
                    start: 0,
                    end: 4801
                },
                guard.generation,
                guard.revision
            )
            .unwrap_err()
            .contains("shorter")
    );
    assert_eq!(
        rig.session.document_snapshot(),
        before,
        "refusals must not edit history"
    );
    let clip = rig.project().playlist.clips[0].clone();
    rig.session
        .dispatch(
            Command::AddClips {
                clips: vec![
                    ClipInit {
                        track: clip.track,
                        start: 0,
                        length: Some(clip.length),
                        offset: None,
                        muted: None,
                        content: clip.content,
                    };
                    windfall_engine::MAX_AUDIO_CLIPS
                ],
            },
            None,
        )
        .unwrap();
    let guard = rig.session.timeline_state();
    let crowded = rig.session.document_snapshot();
    assert!(
        rig.session
            .media_render_audio(RANGE, guard.generation, guard.revision)
            .unwrap_err()
            .contains("capacity")
    );
    assert_eq!(rig.session.document_snapshot(), crowded);
    rig.session.project_new().unwrap();
    assert!(
        rig.session
            .media_render_audio(RANGE, guard.generation, guard.revision)
            .unwrap_err()
            .contains("changed")
    );
}

#[test]
fn media_render_audio_propagates_sampler_preparation_failure_without_a_partial_result() {
    let rig = Rig::new();
    render_fixture(&rig);
    rig.session
        .dispatch(
            Command::UpdateSampler {
                id: rig.channel(0),
                patch: SamplerPatch {
                    stretch: Some(SamplerStretch::Spectral {
                        ratio: 1.5,
                        quality: ClipStretchQuality::Fast,
                        formants: true,
                        range: SamplerKeyRange {
                            first: 60,
                            last: 60,
                        },
                    }),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    // Replace only the control-side cache with a zero-byte fault budget. The
    // installed live plan retains its healthy bank; offline preparation must
    // return its own failure instead of exposing a partial media audio result.
    {
        let mut state = rig.session.state();
        let mut limited = windfall_engine::SamplePool::with_sampler_budget(0);
        for (id, audio) in state.pool.iter() {
            limited.insert(id, audio.clone());
        }
        state.pool = limited;
    }
    let guard = rig.session.timeline_state();
    let before = rig.session.document_snapshot();
    assert!(
        rig.session
            .media_render_audio(RANGE, guard.generation, guard.revision)
            .unwrap_err()
            .contains("budget")
    );
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(rig.session.state().pool.sampler_retained_bytes(), 0);
}

#[derive(Debug)]
struct FailedRenderProvider;
impl windfall_engine::plugins::PluginFactory for FailedRenderProvider {
    fn render_error(&self) -> Option<String> {
        Some("Media render provider failed".into())
    }
    fn effect(
        &self,
        _: &windfall_project::PluginBinding,
        _: u32,
        _: usize,
    ) -> Result<Box<dyn windfall_engine::plugins::HostedEffect>, String> {
        Err("No healthy media effect".into())
    }
    fn instrument(
        &self,
        _: &windfall_project::PluginBinding,
        _: u32,
        _: usize,
    ) -> Result<Box<dyn windfall_engine::plugins::HostedInstrument>, String> {
        Err("No healthy media instrument".into())
    }
}

#[test]
fn media_render_audio_propagates_plugin_render_failure_without_a_partial_result() {
    let rig = Rig::new();
    render_fixture(&rig);
    rig.session
        .state()
        .pool
        .set_plugin_factory(Arc::new(FailedRenderProvider));
    let guard = rig.session.timeline_state();
    let before = rig.session.document_snapshot();
    assert_eq!(
        rig.session
            .media_render_audio(RANGE, guard.generation, guard.revision)
            .unwrap_err(),
        "Media render provider failed"
    );
    assert_eq!(rig.session.document_snapshot(), before);
}

struct Take;
impl CaptureHandle for Take {
    fn frames(&self) -> u64 {
        480
    }
    fn failed(&self) -> bool {
        false
    }
    fn finish(self: Box<Self>) -> Result<u64, String> {
        Ok(480)
    }
}
fn capture_take(
    _: RecordingSource,
    _: u32,
    mut sink: Sink,
) -> Result<Box<dyn CaptureHandle>, String> {
    sink(&[0.25; 960])?;
    Ok(Box::new(Take))
}

#[test]
fn media_render_audio_obeys_recording_exclusion_before_plugin_capture() {
    let rig = Rig::new();
    render_fixture(&rig);
    rig.session
        .recording_start_with(
            RecordingSource {
                host: "Fake".into(),
                device: "Fake".into(),
                left: 0,
                right: None,
                armed_tracks: None,
                mixer_tap: None,
                alignment: None,
                loop_recording: None,
                monitor: None,
            },
            960,
            None,
            capture_take,
        )
        .unwrap();
    let guard = rig.session.timeline_state();
    let before = rig.session.document_snapshot();
    assert!(
        rig.session
            .media_render_audio(RANGE, guard.generation, guard.revision)
            .is_err()
    );
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(rig.session.recording_state().active);
    rig.session.recording_cancel();
    let guard = rig.session.timeline_state();
    assert_eq!(
        rig.session
            .media_render_audio(RANGE, guard.generation, guard.revision)
            .unwrap()
            .samples
            .len(),
        24_025 * 2
    );
}

fn comparable(mut capture: Value) -> Value {
    // Temporary source WAV paths are not musical evidence. Everything else,
    // including every PCM sample and frame clock, must reproduce exactly.
    for sample in capture["renderProject"]["samples"].as_array_mut().unwrap() {
        sample.as_object_mut().unwrap().remove("path");
    }
    for step in capture["steps"].as_array_mut().unwrap() {
        let mut frame: Value = serde_json::from_str(step["realtime"].as_str().unwrap()).unwrap();
        for key in ["cpu", "xruns"] {
            frame.as_object_mut().unwrap().remove(key);
        }
        step["realtime"] = frame;
    }
    capture
}

#[test]
fn sequenced_and_live_note_session_publications_and_media_render_match_mounted_capture() {
    let mut capture = capture_notes();
    let rig = Rig::new();
    capture["audio"] = serde_json::to_value(render_fixture(&rig)).unwrap();
    capture["range"] = serde_json::to_value(RANGE).unwrap();
    capture["renderProject"] = serde_json::to_value(rig.project()).unwrap();
    capture["renderGuard"] = serde_json::to_value(rig.session.timeline_state()).unwrap();
    if let Some(relative) = std::env::var_os("WINDFALL_MEDIA_CAPTURE_DIR") {
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
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let output = root.join(relative);
        fs::create_dir_all(&output).unwrap();
        fs::write(
            output.join("native-media.json"),
            serde_json::to_vec_pretty(&capture).unwrap(),
        )
        .unwrap();
    }
    let checked = Path::new(env!("CARGO_MANIFEST_DIR")).join(CHECKED);
    if checked.exists() {
        let old: Value = serde_json::from_slice(&fs::read(checked).unwrap()).unwrap();
        super::native_capture::assert_matches(
            comparable(old),
            comparable(capture),
            "native media capture drift requires review",
        );
    } else {
        assert!(
            std::env::var_os("WINDFALL_MEDIA_CAPTURE_DIR").is_some(),
            "Capture native-media.json using docs/qa/2026-10-12-r4-evidence.md; no synthetic fallback"
        );
    }
}
