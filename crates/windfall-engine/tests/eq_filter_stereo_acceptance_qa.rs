//! The equalizer, filter and stereo effects in the engine: their controls
//! reach native playback, history and project files, automation is heard,
//! and what is played live is what is exported.
#[allow(dead_code)]
#[path = "engine/support.rs"]
mod support;

use support::{Rig, sine};
use windfall_dsp::control::PanLfoParams;
use windfall_dsp::eqbank::SevenBandParams;
use windfall_dsp::{SpreaderParams, StereoEnhancerParams};
use windfall_engine::{Processor, RenderOptions, render};
use windfall_ipc::PlayMode;
use windfall_project::*;

fn audio(project: &Project, rig: &Rig) -> Vec<f32> {
    let result = render(project, &rig.pool, &RenderOptions::default(), &mut |_| true);
    assert!(result.samples().iter().all(|sample| sample.is_finite()));
    assert!(result.samples().iter().any(|sample| sample.abs() > 0.001));
    result.samples().to_vec()
}

/// The two renderings differ by more than rounding. A tail may have changed
/// length; the song they share is compared.
fn distinct(a: &[f32], b: &[f32], what: &str) {
    let energy = a.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>();
    let difference = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| f64::from(x - y).powi(2))
        .sum::<f64>();
    assert!(difference / energy > 0.0001, "{what} must change the audio");
}

fn master_effects(project: &Project) -> Vec<EffectSlot> {
    project
        .mixer
        .tracks
        .iter()
        .find(|track| track.id == TrackId::MASTER)
        .unwrap()
        .effects
        .clone()
}

fn index_of(kind: EffectKind, id: &str) -> u32 {
    kind.descriptors()
        .iter()
        .position(|info| info.id == id)
        .unwrap_or_else(|| panic!("{} has no control {id}", kind.name())) as u32
}

fn set(doc: &mut Document, effect: EffectId, id: &str, value: f32) {
    let kind = master_effects(doc.project())
        .iter()
        .find(|slot| slot.id == effect)
        .unwrap()
        .params
        .kind();
    doc.dispatch(
        Command::SetEffectParam {
            track: TrackId::MASTER,
            effect,
            param: index_of(kind, id),
            value,
        },
        None,
    )
    .unwrap();
}

/// A 440 Hz and a 12 kHz tone on the left of center and the right of it,
/// so that a stereo effect has something to work on.
fn tones(rig: &mut Rig, track: TrackId, start: u32) {
    for (hz, pan) in [(440.0, -0.6), (12_000.0, 0.6)] {
        let channel = rig.channel_on(sine(48_000, hz, 1.0), track);
        rig.channel_mut(channel).pan = pan;
        rig.note(channel, start, 1920);
    }
}

fn fixture(params: EffectParams) -> (Rig, Document, EffectId) {
    let mut rig = Rig::new();
    tones(&mut rig, TrackId::MASTER, 240);
    let mut doc = Document::new(rig.project.clone());
    let effect = EffectId(
        doc.dispatch(
            Command::AddEffect {
                track: TrackId::MASTER,
                kind: params.kind(),
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    doc.dispatch(
        Command::SetEffectParams {
            track: TrackId::MASTER,
            effect,
            params,
        },
        None,
    )
    .unwrap();
    (rig, doc, effect)
}

/// The project comes back from disk with the same effect and the same
/// audio, and a live processor plays what the export holds, however the
/// device cuts its blocks.
fn roundtrip_and_live(doc: &Document, rig: &Rig, expected: &[f32]) {
    let path = std::env::temp_dir().join(format!(
        "windfall-eq-filter-stereo-qa-{}-{}.windfall",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    file::save(doc.project(), &path).unwrap();
    let loaded = file::load(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(master_effects(&loaded), master_effects(doc.project()));
    assert_eq!(audio(&loaded, rig), expected);
    for block in [7, 64, 127] {
        let (mut processor, controller) = Processor::new(48_000);
        controller.set_project(&loaded, &rig.pool);
        controller.play();
        let latency = controller.latency_frames() as usize;
        let mut live = vec![0.0; (36_000 + latency) * 2];
        for chunk in live.chunks_mut(block * 2) {
            processor.process(chunk);
        }
        assert_eq!(
            &live[latency * 2..(latency + 36_000) * 2],
            &expected[..72_000],
            "blocks of {block}"
        );
    }
}

/// A curve on one control is heard, and the song sounds the same played
/// live in any block size as it does exported.
fn automation_is_heard_live_and_offline(params: EffectParams, id: &str) {
    let mut rig = Rig::new();
    let track = rig.track();
    tones(&mut rig, track, 0);
    let effect = rig.effect(track, params);
    let lane = rig.playlist_track();
    let pattern = rig.first_pattern();
    rig.clip(lane, pattern, 0, 1920);
    let options = RenderOptions {
        mode: PlayMode::Song,
        block_frames: 101,
        ..Default::default()
    };
    let still = render(&rig.project, &rig.pool, &options, &mut |_| true);
    let automation = rig.automation(
        AutomationTarget::EffectParam {
            track,
            effect,
            param: index_of(params.kind(), id),
        },
        &[(0, 0.05), (480, 0.95), (960, 0.2), (1920, 0.8)],
    );
    let lane = rig.playlist_track();
    rig.automation_clip(lane, automation, 0, 1920);
    let moved = render(&rig.project, &rig.pool, &options, &mut |_| true);
    assert!(moved.samples().iter().all(|sample| sample.is_finite()));
    distinct(still.samples(), moved.samples(), "automation");
    for block in [64, 137] {
        assert_eq!(
            rig.play_song(48_000, moved.frames(), block),
            moved.samples(),
            "live blocks of {block}"
        );
    }
}

/// The whole contract for one processor: each edit in `edits` changes the
/// native audio and is one undo step that restores it exactly, the result
/// survives the file and matches live playback, and `automated` can be
/// moved by a curve.
fn accept(params: EffectParams, edits: &[(&str, f32)], automated: &str) {
    let (rig, mut doc, effect) = fixture(params);
    let mut before = audio(doc.project(), &rig);
    for (id, value) in edits {
        set(&mut doc, effect, id, *value);
        let after = audio(doc.project(), &rig);
        distinct(&before, &after, id);
        doc.undo().unwrap();
        assert_eq!(audio(doc.project(), &rig), before, "undo of {id}");
        doc.redo().unwrap();
        assert_eq!(audio(doc.project(), &rig), after, "redo of {id}");
        before = after;
    }
    roundtrip_and_live(&doc, &rig, &before);
    automation_is_heard_live_and_offline(params, automated);
}

#[test]
fn seven_band_faders_reach_native_audio_history_files_and_automation() {
    accept(
        EffectParams::SevenBand(SevenBandParams::default()),
        &[("gainsDb.3", 12.0), ("gainsDb.6", -15.0), ("gainsDb.2", -9.0)],
        "gainsDb.3",
    );
}

#[test]
fn spreader_controls_reach_native_audio_history_files_and_automation() {
    accept(
        EffectParams::Spreader(SpreaderParams::default()),
        &[("width", 2.0), ("haasMs", 25.0), ("amount", 1.0)],
        "width",
    );
}

#[test]
fn stereo_enhancer_phase_offset_and_inversion_reach_native_audio_history_and_files() {
    accept(
        EffectParams::StereoEnhancer(StereoEnhancerParams::default()),
        &[
            ("phaseOffsetMs", 12.0),
            ("invert", 1.0),
            ("width", 0.0),
            ("balance", -1.0),
        ],
        "phaseOffsetMs",
    );
}

#[test]
fn pan_motion_shape_sync_and_volume_reach_native_audio_history_and_files() {
    accept(
        EffectParams::PanLfo(PanLfoParams::default()),
        &[
            ("shape", 3.0),
            ("sync", 1.0),
            ("beats", 0.25),
            ("volumeDepth", 1.0),
        ],
        "depth",
    );
}
