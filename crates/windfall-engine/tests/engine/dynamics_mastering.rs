//! Dynamics family through saved routing, automation, offline/live paths and
//! the callback allocation guard. DSP reference tests live in windfall-dsp.
use crate::support::Rig;
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

#[test]
fn bass_harmonic_amount_automation_matches_live_render_with_compensated_latency() {
    verify(EffectKind::BassHarmonics, "amount", 13);
}

#[test]
fn mastering_limiter_gain_automation_matches_live_export_and_fixed_true_peak_latency() {
    verify(EffectKind::MasterLimiter, "gainDb", 78);
}

#[test]
fn two_stage_drive_automation_matches_live_export_without_delay_or_callback_allocation() {
    verify(EffectKind::StageStack, "driveDb", 0);
}


#[test]
fn transient_attack_automation_matches_live_export_without_callback_allocations() {
    verify(EffectKind::TransientShaper, "attack", 0);
}

#[test]
fn shaped_transient_outputs_recombine_in_the_real_mixer_and_destinations_process_independently() {
    use windfall_dsp::multiband::{TransientListen, TransientShaperParams};
    use windfall_project::EffectOutputRoute;
    let settings = TransientShaperParams { attack: 0.4, sustain: -0.25, ..Default::default() };
    let (baseline, _) = song(Some(EffectParams::TransientShaper(settings)));
    let (expected, _) = guarded_song(&baseline, RATE as usize, 257);
    for mask in 0..4 {
        let (mut rig, source) = song(Some(EffectParams::TransientShaper(settings)));
        let effect = rig.track_mut(source).effects[0].id;
        let attack = rig.track(); let sustain = rig.track();
        for (output,target) in [(0,attack),(1,sustain)] {
            if mask & (1<<output) != 0 {
                rig.project.mixer.effect_outputs.push(EffectOutputRoute { effect, output, target });
            }
        }
        rig.project.check().unwrap();
        let (actual, _) = guarded_song(&rig, RATE as usize, 137);
        assert!(difference(&actual, &expected) < 2e-6, "recombined mask {mask}");
        if mask == 3 {
            // Both parts have left the source: disconnecting its ordinary
            // output cannot remove them. Muting Attack independently should
            // leave the exact Sustain-listen result heard through the host.
            rig.track_mut(source).output = None;
            rig.track_mut(attack).muted = true;
            let (body, _) = guarded_song(&rig, RATE as usize, 53);
            let (listen, _) = song(Some(EffectParams::TransientShaper(TransientShaperParams {
                listen: TransientListen::Sustain, ..settings
            })));
            let (reference, _) = guarded_song(&listen, RATE as usize, 211);
            assert!(difference(&body, &reference) < 2e-6);
            assert!(difference(&body, &expected) > 0.01);
            // A disabled slot emits no auxiliary part.
            rig.track_mut(source).effects[0].enabled = false;
            let (disabled, _) = guarded_song(&rig, RATE as usize, 71);
            assert!(disabled.iter().all(|x| x.abs() < 1e-7));
        }
    }
}
