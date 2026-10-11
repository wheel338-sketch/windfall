//! Dynamics family through saved routing, automation, offline/live paths and
//! the callback allocation guard. DSP reference tests live in windfall-dsp.
use crate::support::{Rig, run};
use windfall_core::AudioBuffer;
use windfall_dsp::{EffectKind, EffectParams};
use windfall_engine::{RenderOptions, render};
use windfall_ipc::PlayMode;
use windfall_project::{AutomationTarget, TrackId};

const RATE: u32 = 48_000;

fn source() -> AudioBuffer {
    let samples = (0..RATE * 2).flat_map(|n| {
        let t = f64::from(n) / f64::from(RATE);
        let hit = if n % 12_000 < 1_200 { 1.0 } else { 0.35 };
        let x = hit * (0.35 * (std::f64::consts::TAU * 60.0 * t).sin()
            + 0.2 * (std::f64::consts::TAU * 1_000.0 * t).sin()
            + 0.15 * (std::f64::consts::TAU * 8_000.0 * t).sin());
        [x as f32, (x * 0.5) as f32]
    }).collect();
    AudioBuffer::from_interleaved(RATE, 2, samples)
}

fn song(params: Option<EffectParams>) -> (Rig, TrackId) {
    let mut rig = Rig::new();
    let track = rig.track();
    let lane = rig.playlist_track();
    rig.audio_clip(lane, source(), track, 0, 960);
    if let Some(params) = params { rig.effect(track, params); }
    (rig, track)
}

fn difference(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(a,b)| (a-b).abs()).fold(0.0, f32::max)
}

fn guarded_song(rig: &Rig, frames: usize, block: usize) -> (Vec<f32>, usize) {
    let (mut processor, controller) = rig.song_processor(RATE);
    let latency = controller.latency_frames() as usize;
    controller.play();
    let mut result = vec![0.0; (frames + latency) * 2];
    for piece in result.chunks_mut(block * 2) {
        assert_eq!(crate::realtime::allocator_calls(|| processor.process(piece)), 0);
    }
    (result[latency * 2..].to_vec(), latency)
}

fn verify(kind: EffectKind, id: &str, expected_latency: usize) {
    let (mut rig, track) = song(Some(kind.default_params()));
    let effect = rig.track_mut(track).effects[0].id;
    let index = kind.descriptors().iter().position(|p| p.id == id).unwrap();
    let automation = rig.automation(AutomationTarget::EffectParam { track, effect, param: index as u32 },
        &[(0, 0.1), (240, 0.9), (480, 0.2), (720, 0.8)]);
    let lane = rig.playlist_track();
    rig.automation_clip(lane, automation, 0, 960);
    rig.project.check().unwrap();
    let options = RenderOptions { sample_rate: RATE, mode: PlayMode::Song, tail_secs: 0.5,
        block_frames: 137, ..Default::default() };
    let exported = render(&rig.project, &rig.pool, &options, &mut |_| true);
    let (played, latency) = guarded_song(&rig, exported.frames(), 251);
    assert_eq!(latency, expected_latency, "{kind:?}");
    assert!(difference(exported.samples(), &played) < 2e-6, "live/export {kind:?}");
    let (small, _) = guarded_song(&rig, exported.frames(), 17);
    assert!(difference(&played, &small) < 2e-6, "partition {kind:?}");
    assert!(played.iter().all(|x| x.is_finite()));
    let (static_rig, _) = song(Some(kind.default_params()));
    let static_export = render(&static_rig.project, &static_rig.pool, &options, &mut |_| true);
    assert!(difference(exported.samples(), static_export.samples()) > 1e-3, "audible automation {kind:?}");
    // A rendered tail must decay rather than stay audible forever.
    assert!(played[played.len() - 256..].iter().all(|x| x.abs() < 2e-5), "tail {kind:?}");
}

#[test]
fn multiband_compressor_automation_is_audible_and_matches_live_render_without_allocations() {
    verify(EffectKind::MultibandCompressor, "low.thresholdDb", 0);
}

#[test]
fn maximizer_band_automation_matches_live_render_with_both_pdc_stages() {
    verify(EffectKind::MultibandMaximizer, "lowCeilingDb", 96);
}

#[test]
fn one_knob_amount_is_audible_and_matches_live_render_without_callback_allocations() {
    verify(EffectKind::OneKnob, "amount", 96);
}
