use super::*;
use crate::{Effect, ParamSet};

const RATE: f32 = 8000.0;
fn sine(hz: f32, len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| (std::f32::consts::TAU * hz * i as f32 / RATE).sin() * 0.2)
        .collect()
}

#[test]
fn spatial_band_impulse_is_delayed_and_complementary_bands_recombine() {
    let mut fx = BandDelay::default();
    fx.prepare(RATE, 128);
    fx.set_params(&BandDelayParams {
        time_ms: [10.0; 3],
        feedback: [0.0; 3],
        pan: [0.0; 3],
        mix: 1.0,
        ..Default::default()
    });
    let mut l = vec![0.0; 400];
    let mut r = l.clone();
    l[0] = 1.0;
    r[0] = 1.0;
    fx.process(&mut l, &mut r);
    for (i, x) in l.iter().enumerate() {
        let expected = if i == 80 { 1.0 } else { 0.0 };
        assert!((x - expected).abs() < 2e-6, "sample {i}: {x}");
    }
    assert_eq!(l, r);
    assert_eq!(fx.latency_samples(), 0); // intentional effect delay, no PDC
}

#[test]
fn spatial_vintage_chorus_returns_a_delayed_toned_impulse() {
    let mut fx = VintageChorus::default();
    fx.prepare(RATE, 128);
    fx.set_params(&VintageChorusParams {
        delay_ms: 10.0,
        depth_ms: 0.0,
        rate_hz: 0.0,
        mix: 1.0,
        ..Default::default()
    });
    let mut l = vec![0.0; 500];
    let mut r = l.clone();
    l[0] = 1.0;
    fx.process(&mut l, &mut r);
    assert!(l[..80].iter().all(|x| *x == 0.0));
    assert!(l[80..150].iter().any(|x| x.abs() > 0.05));
    assert!(r.iter().all(|x| *x == 0.0));
}

#[test]
fn spatial_room_has_early_reflections_and_a_decaying_short_tail() {
    let mut fx = Room::default();
    fx.prepare(RATE, 128);
    fx.set_params(&RoomParams {
        pre_delay_ms: 0.0,
        mix: 1.0,
        decay_s: 0.25,
        ..Default::default()
    });
    let mut l = vec![0.0; RATE as usize];
    let mut r = l.clone();
    l[0] = 1.0;
    fx.process(&mut l, &mut r);
    assert!(l[..20].iter().all(|x| *x == 0.0));
    assert!(l[20..200].iter().any(|x| x.abs() > 0.02));
    let early: f32 = l[..2000].iter().chain(&r[..2000]).map(|x| x * x).sum();
    let late: f32 = l[6000..].iter().chain(&r[6000..]).map(|x| x * x).sum();
    assert!(early > 0.01 && late < early * 1e-4, "{early} -> {late}");
}

fn feedback_test<E: Effect + Default>(params: E::Params) {
    let mut fx = E::default();
    fx.prepare(RATE, 127);
    fx.set_params(&params);
    let mut l = [0.0; 127];
    let mut r = [0.0; 127];
    let mut peak = 0.0_f32;
    for block in 0..400 {
        l.fill(0.0);
        r.fill(0.0);
        if block == 0 {
            l[0] = 1.0;
            r[0] = -0.5;
        }
        fx.process(&mut l, &mut r);
        for x in l.iter().chain(&r) {
            assert!(x.is_finite());
            peak = peak.max(x.abs());
        }
    }
    assert!(peak > 0.001 && peak < 20.0, "unstable peak {peak}");
}
#[test]
fn spatial_feedback_is_finite_at_extreme_signed_settings() {
    for sign in [-1.0, 1.0] {
        feedback_test::<StackedFlanger>(StackedFlangerParams {
            feedback: sign * f32::MAX,
            delay_ms: 6.0,
            depth_ms: 8.0,
            rate1_hz: 5.0,
            rate2_hz: 4.2,
            tone_hz: 20_000.0,
            mix: 1.0,
            ..Default::default()
        });
        feedback_test::<VintagePhaser>(VintagePhaserParams {
            feedback: sign * f32::MAX,
            rate_hz: 5.0,
            min_hz: 20.0,
            max_hz: 10_000.0,
            color_hz: 12_000.0,
            mix: 1.0,
            ..Default::default()
        });
        feedback_test::<BandDelay>(BandDelayParams {
            feedback: [sign * f32::MAX; 3],
            time_ms: [5.0, 9.0, 13.0],
            mix: 1.0,
            ..Default::default()
        });
    }
    feedback_test::<Room>(RoomParams {
        decay_s: f32::MAX,
        damping: 0.0,
        mix: 1.0,
        ..Default::default()
    });
}

fn modulation_test<E: Effect + Default>(params: E::Params) {
    let mut fx = E::default();
    fx.prepare(RATE, 800);
    fx.set_params(&params);
    // 500 Hz repeats exactly every 16 samples. Both blocks have identical
    // steady input phase and are past delay/filter startup.
    let mut l = sine(500.0, 16_000);
    let mut r = l.clone();
    for (l, r) in l.chunks_mut(800).zip(r.chunks_mut(800)) {
        fx.process(l, r);
    }
    let change: f32 = l[8000..8800]
        .iter()
        .zip(&l[12_000..12_800])
        .map(|(a, b)| (a - b).abs())
        .sum();
    assert!(change > 1.0, "no measurable modulation: {change}");
}
#[test]
fn spatial_choruses_and_phaser_modulate_a_steady_sine_across_blocks() {
    modulation_test::<VintageChorus>(VintageChorusParams::default());
    modulation_test::<HyperChorus>(HyperChorusParams::default());
    modulation_test::<VintagePhaser>(VintagePhaserParams::default());
    modulation_test::<StackedFlanger>(StackedFlangerParams::default());
}

#[test]
fn spatial_enhancer_locks_low_sine_more_mono_than_high_sine() {
    let measure = |hz| {
        let mut fx = StereoEnhancer::default();
        fx.prepare(RATE, 128);
        fx.set_params(&StereoEnhancerParams {
            bass_hz: 300.0,
            width: 1.5,
            ..Default::default()
        });
        let mut l = sine(hz, 8000);
        let mut r: Vec<_> = l.iter().map(|x| x * 0.25).collect();
        fx.process(&mut l, &mut r);
        let side: f32 = l[2000..]
            .iter()
            .zip(&r[2000..])
            .map(|(l, r)| (l - r).powi(2))
            .sum();
        let mid: f32 = l[2000..]
            .iter()
            .zip(&r[2000..])
            .map(|(l, r)| (l + r).powi(2))
            .sum();
        side / mid
    };
    let low = measure(40.0);
    let high = measure(2000.0);
    assert!(low < high * 0.01 && high > 0.1, "low {low}, high {high}");
}

#[test]
fn spatial_spreader_preserves_mono_fold_down_including_live_edits() {
    let mut fx = Spreader::default();
    fx.prepare(RATE, 1);
    let l = sine(321.0, 4000);
    let r = sine(731.0, 4000);
    for i in 0..l.len() {
        if i == 1733 {
            fx.set_params(&SpreaderParams {
                width: 2.0,
                haas_ms: 27.0,
                amount: 1.0,
                mix: 0.8,
            });
        }
        let (mut ol, mut or) = ([l[i]], [r[i]]);
        fx.process(&mut ol, &mut or);
        assert!((ol[0] + or[0] - l[i] - r[i]).abs() < 1e-6);
    }
}

fn partition<E: Effect>(mut whole: E, mut pieces: E, edit: E::Params) {
    let (mut l, mut r) = (sine(271.0, 6000), sine(809.0, 6000));
    l[1200..].fill(0.0);
    r[1200..].fill(0.0);
    let (mut pl, mut pr) = (l.clone(), r.clone());
    // Same edit at the same frame, exercised inside a ringing tail.
    for (start, end) in [(0, 1777), (1777, 6000)] {
        if start != 0 {
            whole.set_params(&edit);
            pieces.set_params(&edit);
        }
        whole.process(&mut l[start..end], &mut r[start..end]);
        let sizes = [1, 31, 128, 7, 53];
        let mut at = start;
        let mut n = 0;
        while at < end {
            let to = (at + sizes[n % sizes.len()]).min(end);
            pieces.process(&mut pl[at..to], &mut pr[at..to]);
            at = to;
            n += 1;
        }
    }
    assert_eq!(l, pl);
    assert_eq!(r, pr);
}
#[test]
fn spatial_room_is_exactly_partition_invariant_with_tail_automation() {
    let (mut a, mut b) = (Room::default(), Room::default());
    for fx in [&mut a, &mut b] {
        fx.prepare(RATE, 6000);
    }
    partition(
        a,
        b,
        RoomParams {
            size: 0.8,
            decay_s: 1.2,
            pre_delay_ms: 17.0,
            mix: 0.8,
            ..Default::default()
        },
    );
}

#[test]
fn spatial_existing_echo_bank_substitute_is_partition_invariant() {
    use crate::echo_bank::{EchoBank, EchoBankParams};
    let mut p = EchoBankParams {
        dry: 0.2,
        wet: 0.8,
        ..Default::default()
    };
    for (i, unit) in p.units.iter_mut().enumerate() {
        unit.enabled = i < 4;
        unit.sync = false;
        unit.time_ms = 11.0 + i as f32 * 13.0;
        unit.next_send = if i < 3 { 0.4 } else { 0.0 };
        unit.feedback = 0.25;
        unit.output_pan = i as f32 * 0.15 - 0.4;
    }
    let (mut a, mut b) = (EchoBank::default(), EchoBank::default());
    for fx in [&mut a, &mut b] {
        fx.prepare(RATE, 6000);
        fx.set_params(&p);
    }
    p.units[1].time_ms = 80.0;
    p.units[2].output_pan = -0.5;
    partition(a, b, p);
}

#[test]
fn spatial_other_new_circuits_are_partition_invariant() {
    macro_rules! check {
        ($effect:ty, $params:expr) => {{
            let (mut a, mut b) = (<$effect>::default(), <$effect>::default());
            a.prepare(RATE, 6000);
            b.prepare(RATE, 6000);
            partition(a, b, $params);
        }};
    }
    check!(
        VintageChorus,
        VintageChorusParams {
            depth_ms: 9.0,
            mix: 0.8,
            ..Default::default()
        }
    );
    check!(
        HyperChorus,
        HyperChorusParams {
            spread: 0.3,
            rate_hz: 3.0,
            ..Default::default()
        }
    );
    check!(
        VintagePhaser,
        VintagePhaserParams {
            feedback: -0.7,
            color_hz: 300.0,
            ..Default::default()
        }
    );
    check!(
        StackedFlanger,
        StackedFlangerParams {
            rate2_hz: 3.0,
            feedback: -0.65,
            ..Default::default()
        }
    );
    check!(
        BandDelay,
        BandDelayParams {
            time_ms: [11.0, 23.0, 17.0],
            ..Default::default()
        }
    );
    check!(
        Spreader,
        SpreaderParams {
            haas_ms: 25.0,
            amount: 0.9,
            ..Default::default()
        }
    );
    check!(
        StereoEnhancer,
        StereoEnhancerParams {
            bass_hz: 400.0,
            width: 2.0,
            ..Default::default()
        }
    );
}

fn params_check<P: ParamSet + serde::Serialize + serde::de::DeserializeOwned>() {
    let default = P::default();
    assert_eq!(default, default.sanitized());
    assert_eq!(
        default,
        serde_json::from_str::<P>(&serde_json::to_string(&default).unwrap()).unwrap()
    );
    assert_eq!(default, serde_json::from_str::<P>("{}").unwrap());
    for (i, d) in P::descriptors().iter().enumerate() {
        assert_eq!(default.get(i), Some(d.default), "{} {}", P::NAME, d.id);
        assert_eq!(P::index_of(d.id), Some(i));
        for value in [f32::NAN, f32::INFINITY, -f32::MAX, f32::MAX] {
            let mut p = default;
            assert!(p.set(i, value));
            let x = p.get(i).unwrap();
            assert!(x.is_finite() && x >= d.min && x <= d.max);
        }
    }
    assert_eq!(default.get(P::descriptors().len()), None);
}
#[test]
fn spatial_params_have_stable_descriptors_defaults_and_json() {
    params_check::<VintageChorusParams>();
    params_check::<HyperChorusParams>();
    params_check::<VintagePhaserParams>();
    params_check::<StackedFlangerParams>();
    params_check::<BandDelayParams>();
    params_check::<RoomParams>();
    params_check::<SpreaderParams>();
    params_check::<StereoEnhancerParams>();
}

fn hostile<E: Effect + Default>() {
    let mut fx = E::default();
    let (mut l, mut r) = ([f32::NAN, f32::MAX, f32::INFINITY, -f32::MAX], [0.0; 4]);
    fx.process(&mut l, &mut r);
    assert!(l.iter().chain(&r).all(|x| x.is_finite()));
    for sr in [f32::NAN, 1.0, 8000.0, 192_000.0] {
        fx.prepare(sr, 4);
        let mut params = E::Params::default();
        for i in 0..E::Params::descriptors().len() {
            params.set(i, f32::MAX);
        }
        fx.set_params(&params);
        fx.set_tempo(f32::NAN);
        l = [f32::NAN, f32::MAX, f32::INFINITY, -f32::MAX];
        fx.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|x| x.is_finite()));
        fx.reset();
        fx.process(&mut [], &mut []);
    }
}
#[test]
fn spatial_hostile_input_and_sample_rates_remain_finite() {
    hostile::<VintageChorus>();
    hostile::<HyperChorus>();
    hostile::<VintagePhaser>();
    hostile::<StackedFlanger>();
    hostile::<BandDelay>();
    hostile::<Room>();
    hostile::<Spreader>();
    hostile::<StereoEnhancer>();
}

fn tail_check<E: Effect + Default>() {
    let mut fx = E::default();
    fx.prepare(1000.0, 1);
    let mut l = [1.0];
    let mut r = [-0.5];
    fx.process(&mut l, &mut r);
    for _ in 0..fx.tail_samples() {
        l = [0.0];
        r = [0.0];
        fx.process(&mut l, &mut r);
    }
    for _ in 0..1000 {
        l = [0.0];
        r = [0.0];
        fx.process(&mut l, &mut r);
        assert_eq!(l, [0.0]);
        assert_eq!(r, [0.0]);
    }
}
#[test]
fn spatial_reported_tails_really_expire() {
    tail_check::<VintageChorus>();
    tail_check::<HyperChorus>();
    tail_check::<VintagePhaser>();
    tail_check::<StackedFlanger>();
    tail_check::<BandDelay>();
    tail_check::<Room>();
    tail_check::<Spreader>();
    tail_check::<StereoEnhancer>();
}

/// The sixteen-band layout written out from its definition in f64: fifteen
/// two-section one-pole lowpasses at the fixed edges, neighbouring differences
/// as bands, then a whole-sample delay, level and balance per band.
fn wide_reference(
    input: &[f64],
    time_ms: &[f32; WIDE_BANDS],
    level: &[f32; WIDE_BANDS],
    pan: &[f32; WIDE_BANDS],
    right: bool,
) -> Vec<f64> {
    let rate = f64::from(RATE);
    let mut lowpassed = vec![vec![0.0; input.len()]; WIDE_BANDS - 1];
    for (edge, out) in lowpassed.iter_mut().enumerate() {
        let hz = 40.0 * 400.0_f64.powf(edge as f64 / 14.0);
        let pole = (-std::f64::consts::TAU * hz.min(rate * 0.45) / rate).exp();
        let (mut a, mut b) = (0.0, 0.0);
        for (n, x) in input.iter().enumerate() {
            a = (1.0 - pole) * x + pole * a;
            b = (1.0 - pole) * a + pole * b;
            out[n] = b;
        }
    }
    let mut output = vec![0.0; input.len()];
    for band in 0..WIDE_BANDS {
        let delay = (f64::from(time_ms[band]) * rate / 1000.0).round() as usize;
        let side = f64::from(pan[band]);
        let balance = if right {
            (1.0 + side).min(1.0)
        } else {
            (1.0 - side).min(1.0)
        };
        for (m, out) in output.iter_mut().skip(delay).enumerate() {
            let upto = if band < WIDE_BANDS - 1 {
                lowpassed[band][m]
            } else {
                input[m]
            };
            let below = if band == 0 {
                0.0
            } else {
                lowpassed[band - 1][m]
            };
            *out += (upto - below) * f64::from(level[band]) * balance;
        }
    }
    output
}

#[test]
fn spatial_band_sixteen_layout_matches_its_written_out_definition() {
    // Whole-sample delays at 8 kHz, a different one per band, with uneven
    // levels and pans so no band can stand in for another.
    let time_ms: [f32; WIDE_BANDS] = std::array::from_fn(|band| 5.0 + 2.5 * band as f32);
    let level: [f32; WIDE_BANDS] = std::array::from_fn(|band| 0.25 + 0.05 * band as f32);
    let pan: [f32; WIDE_BANDS] = std::array::from_fn(|band| -0.75 + 0.1 * band as f32);
    let mut params = BandDelayParams {
        layout: BandLayout::Sixteen,
        bank: 3,
        feedback: [0.0; 3],
        mix: 1.0,
        ..Default::default()
    };
    params.banks[2] = BandBank {
        time_ms,
        level,
        pan,
    };
    let mut fx = BandDelay::default();
    fx.prepare(RATE, 128);
    fx.set_params(&params);
    let input: Vec<f64> = (0..1200)
        .map(|n| {
            if n == 0 {
                1.0
            } else {
                0.3 * (0.37 * n as f64).sin() * (-(n as f64) / 200.0).exp()
            }
        })
        .collect();
    let mut l: Vec<f32> = input.iter().map(|x| *x as f32).collect();
    let mut r = l.clone();
    fx.process(&mut l, &mut r);
    let (mut worst, mut peak) = (0.0_f64, 0.0_f64);
    for (out, right) in [(&l, false), (&r, true)] {
        let want = wide_reference(
            &input
                .iter()
                .map(|x| f64::from(*x as f32))
                .collect::<Vec<_>>(),
            &time_ms,
            &level,
            &pan,
            right,
        );
        for (got, want) in out.iter().zip(&want) {
            worst = worst.max((f64::from(*got) - want).abs());
            peak = peak.max(want.abs());
        }
    }
    println!("BAND16 reference worst={worst:.3e} peak={peak:.4}");
    assert!(peak > 0.05, "the reference must be audible: {peak}");
    assert!(worst < 5e-6, "worst difference {worst}");
    // Another bank plays other values: the default bank is 250 ms, so nothing
    // has come back yet inside this window.
    let mut other = BandDelay::default();
    other.prepare(RATE, 128);
    other.set_params(&BandDelayParams { bank: 4, ..params });
    let mut l: Vec<f32> = input.iter().map(|x| *x as f32).collect();
    let mut r = l.clone();
    other.process(&mut l, &mut r);
    assert!(l.iter().chain(&r).take(2 * 1999).all(|x| x.abs() < 2e-6));
}

#[test]
fn spatial_band_sixteen_bands_recombine_and_layout_changes_do_not_click() {
    let mut params = BandDelayParams {
        layout: BandLayout::Sixteen,
        feedback: [0.0; 3],
        mix: 1.0,
        ..Default::default()
    };
    params.banks[0].time_ms = [10.0; WIDE_BANDS];
    let mut fx = BandDelay::default();
    fx.prepare(RATE, 128);
    fx.set_params(&params);
    let mut l = vec![0.0; 400];
    let mut r = l.clone();
    l[0] = 1.0;
    r[0] = 1.0;
    fx.process(&mut l, &mut r);
    for (i, x) in l.iter().enumerate() {
        let expected = if i == 80 { 1.0 } else { 0.0 };
        assert!((x - expected).abs() < 2e-6, "sample {i}: {x}");
    }
    assert_eq!(l, r);
    // A sounding 100 Hz echo, then the layout flips: the wet signal may fall
    // no faster than the 5 ms fade allows, plus the tone's own slope.
    let mut fx = BandDelay::default();
    fx.prepare(RATE, 128);
    fx.set_params(&params);
    let mut l = sine(100.0, 4000);
    let mut r = l.clone();
    fx.process(&mut l[..2000], &mut r[..2000]);
    fx.set_params(&BandDelayParams {
        layout: BandLayout::Three,
        time_ms: [10.0; 3],
        ..params
    });
    fx.process(&mut l[2000..], &mut r[2000..]);
    let slope = 0.2 * std::f32::consts::TAU * 100.0 / RATE;
    let fade = 0.2 / (0.005 * RATE);
    let mut worst = 0.0_f32;
    for n in 1900..4000 {
        worst = worst.max((l[n] - l[n - 1]).abs());
    }
    println!(
        "BAND16 layout change worst_step={worst:.5} bound={:.5}",
        slope + fade
    );
    assert!(worst <= slope + fade + 1e-4, "step {worst}");
    assert!(
        l[3900..].iter().any(|x| x.abs() > 0.05),
        "the new layout sounds"
    );
}

#[test]
fn spatial_band_delay_keeps_saved_indices_and_opens_older_settings() {
    assert_eq!(BandDelayParams::index_of("mix"), Some(14));
    assert_eq!(BandDelayParams::index_of("layout"), Some(15));
    assert_eq!(BandDelayParams::index_of("bank"), Some(16));
    assert_eq!(BandDelayParams::index_of("banks.0.timeMs.0"), Some(17));
    assert_eq!(BandDelayParams::index_of("banks.7.pan.15"), Some(400));
    assert_eq!(BandDelayParams::descriptors().len(), 401);
    // What a project saved before the sixteen-band layout existed.
    let old = r#"{"lowHz":250.0,"highHz":2500.0,"timeMs":[100.0,200.0,300.0],"level":[0.5,0.6,0.7],"pan":[-1.0,0.0,1.0],"feedback":[0.1,0.2,0.3],"mix":0.4}"#;
    let opened: BandDelayParams = serde_json::from_str(old).unwrap();
    assert_eq!(opened.layout, BandLayout::Three);
    assert_eq!(opened.bank, 1);
    assert_eq!(opened.banks, [BandBank::default(); BANKS]);
    assert_eq!(opened.time_ms, [100.0, 200.0, 300.0]);
    assert_eq!(opened, opened.sanitized());
    let mut edited = opened;
    edited.layout = BandLayout::Sixteen;
    edited.bank = 8;
    edited.banks[7].pan[15] = -0.5;
    let reopened: BandDelayParams =
        serde_json::from_str(&serde_json::to_string(&edited).unwrap()).unwrap();
    assert_eq!(reopened, edited);
}
