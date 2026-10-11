//! Integrated post-slot processing through the public processor, no device.
#[allow(dead_code)]
#[path = "engine/support.rs"]
mod support;

use support::{Rig, rms, run, sine};
use windfall_core::AudioBuffer;
use windfall_dsp::{
    ChannelMuteParams, DistortionParams, EffectParams, EqBand, EqParams, TrackParams,
};

const RATE: u32 = 48_000;

fn rendered(params: TrackParams, effects: Vec<EffectParams>, frequency: f64) -> Vec<f32> {
    let mut rig = Rig::new();
    let track = rig.track();
    rig.track_mut(track).processing = params;
    for effect in effects {
        rig.effect(track, effect);
    }
    let channel = rig.channel_on(sine(RATE, frequency, 1.0), track);
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(channel, 60, 1.0);
    run(&mut processor, 32_000, 137)
}

#[test]
fn three_independent_eq_bands_boost_their_region_and_bypass_is_neutral() {
    for (band, frequency) in [(0, 40.0), (1, 1000.0), (2, 12000.0)] {
        let mut params = TrackParams::default();
        let setting = match band {
            0 => &mut params.low,
            1 => &mut params.mid,
            _ => &mut params.high,
        };
        setting.gain_db = 6.0;
        setting.frequency_hz = match band {
            0 => 200.0,
            1 => 1000.0,
            _ => 4000.0,
        };
        let neutral = rendered(TrackParams::default(), vec![], frequency);
        let shaped = rendered(params, vec![], frequency);
        let ratio = rms(&shaped[16_000..]) / rms(&neutral[16_000..]);
        assert!(
            (1.7..2.1).contains(&ratio),
            "band {band} measured gain {ratio}"
        );
        params.eq_enabled = false;
        let bypass = rendered(params, vec![], frequency);
        assert!(
            bypass
                .iter()
                .zip(&neutral)
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
    }
}

#[test]
fn integrated_eq_matches_a_post_distortion_eq_slot_and_differs_from_pre_slot_eq() {
    let mut params = TrackParams::default();
    params.low.frequency_hz = 800.0;
    params.low.gain_db = -12.0;
    params.mid.frequency_hz = 3000.0;
    params.mid.gain_db = 6.0;
    let disabled = EqBand {
        enabled: false,
        ..EqBand::default()
    };
    let eq = EffectParams::Eq(EqParams {
        low_shelf: params.low,
        peak1: params.mid,
        peak2: disabled,
        peak3: disabled,
        high_shelf: params.high,
        ..EqParams::default()
    });
    let drive = EffectParams::Distortion(DistortionParams {
        drive_db: 18.0,
        shape: 1.0,
        output_db: -12.0,
    });
    let integrated = rendered(params, vec![drive], 200.0);
    let correct = rendered(TrackParams::default(), vec![drive, eq], 200.0);
    let wrong = rendered(TrackParams::default(), vec![eq, drive], 200.0);
    let difference = |other: &[f32]| {
        integrated[16_000..]
            .iter()
            .zip(&other[16_000..])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max)
    };
    assert!(
        difference(&correct) < 1e-5,
        "post-slot reference mismatch {}",
        difference(&correct)
    );
    assert!(
        difference(&wrong) > 0.01,
        "fixture must distinguish a pre-slot EQ"
    );
}

#[test]
fn stereo_utilities_have_independent_polarity_swap_and_signed_width_on_one_track_only() {
    for (params, expected) in [
        (
            TrackParams {
                invert_left: true,
                ..Default::default()
            },
            [-0.2, 0.1],
        ),
        (
            TrackParams {
                invert_right: true,
                ..Default::default()
            },
            [0.2, -0.1],
        ),
        (
            TrackParams {
                swap: true,
                ..Default::default()
            },
            [0.1, 0.2],
        ),
        (
            TrackParams {
                separation: 1.0,
                ..Default::default()
            },
            [0.15, 0.15],
        ),
        (
            TrackParams {
                separation: -1.0,
                ..Default::default()
            },
            [0.25, 0.05],
        ),
    ] {
        let mut rig = Rig::new();
        let target = rig.track();
        let untouched = rig.track();
        rig.track_mut(target).processing = params;
        let constant = |pair: [f32; 2]| {
            AudioBuffer::from_interleaved(RATE, 2, pair.into_iter().cycle().take(96_000).collect())
        };
        let a = rig.channel_on(constant([0.2, 0.1]), target);
        let b = rig.channel_on(constant([0.03, -0.02]), untouched);
        let (mut processor, controller) = rig.processor(RATE);
        controller.note_on(a, 60, 1.0);
        controller.note_on(b, 60, 1.0);
        let audio = run(&mut processor, 4096, 73);
        for frame in audio[4096..].as_chunks::<2>().0 {
            assert!((frame[0] - (expected[0] + 0.03)).abs() < 1e-6);
            assert!((frame[1] - (expected[1] - 0.02)).abs() < 1e-6);
        }
    }
}

#[test]
fn integrated_stereo_swap_follows_slot_channel_mute() {
    let mut rig = Rig::new();
    let target = rig.track();
    rig.track_mut(target).processing.swap = true;
    rig.effect(
        target,
        EffectParams::ChannelMute(ChannelMuteParams {
            right: true,
            ..Default::default()
        }),
    );
    let channel = rig.channel_on(
        AudioBuffer::from_interleaved(
            RATE,
            2,
            [0.2, 0.1].into_iter().cycle().take(96_000).collect(),
        ),
        target,
    );
    let (mut processor, controller) = rig.processor(RATE);
    controller.note_on(channel, 60, 1.0);
    let audio = run(&mut processor, 4096, 127);
    for frame in audio[4096..].as_chunks::<2>().0 {
        assert!(frame[0].abs() < 1e-6);
        assert!((frame[1] - 0.2).abs() < 1e-6);
    }
}
