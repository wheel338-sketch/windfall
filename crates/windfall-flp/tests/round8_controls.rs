//! Decoded public fields whose stand-in has a matching control, carried in
//! round eight, and the omitted fields filed outside the native table.
//! Every stream is synthetic, every expected number is worked out here from
//! the rule the importer states, and every audible check compares the
//! processor with a reference worked out here, not with its own output for
//! the same settings, except where a test says so.
mod common;
use common::Events;
use std::f64::consts::{PI, TAU};
use windfall_dsp::{AnyEffect, AnyInstrument, EffectParams, InstrumentParams};
use windfall_flp::{ConvertOptions, FileFormat, Header, Outcome, ReportSection, import};

fn stream() -> Events {
    let mut e = Events::new(false);
    e.text(199, "9.0.0");
    e.word(66, 120);
    e.word(64, 0);
    e.byte(21, 0);
    e
}
fn imported(e: &Events) -> windfall_flp::Conversion {
    let file = common::file(
        Header {
            format: FileFormat::Project,
            channel_count: 1,
            ppq: 96,
        },
        &e.bytes,
    );
    let c = import(&file, &ConvertOptions::default()).unwrap();
    c.project.check().unwrap();
    c
}
fn effect(name: &str, b: &[u8]) -> windfall_flp::Conversion {
    let mut e = stream();
    e.text(204, "Master");
    e.text(201, name);
    e.data(213, b);
    e.word(98, 0);
    e.dword(147, u32::MAX);
    imported(&e)
}
fn instrument(name: &str, b: &[u8]) -> windfall_flp::Conversion {
    let mut e = stream();
    e.byte(21, 2);
    e.text(201, name);
    e.data(213, b);
    imported(&e)
}
fn effect_params(c: &windfall_flp::Conversion) -> EffectParams {
    c.project.mixer.tracks[0].effects[0].params
}
fn instrument_params(c: &windfall_flp::Conversion) -> InstrumentParams {
    let windfall_project::ChannelSource::Instrument { params, .. } = c.project.channels[0].source
    else {
        panic!("instrument")
    };
    params
}
fn value(params: &EffectParams, id: &str) -> f32 {
    let index = params
        .kind()
        .descriptors()
        .iter()
        .position(|p| p.id == id)
        .unwrap_or_else(|| panic!("no control {id}"));
    params.get(index).unwrap()
}
fn set(params: &mut EffectParams, id: &str, to: f32) {
    let index = params
        .kind()
        .descriptors()
        .iter()
        .position(|p| p.id == id)
        .unwrap_or_else(|| panic!("no control {id}"));
    assert!(params.set(index, to));
}
fn instrument_value(params: &InstrumentParams, id: &str) -> f32 {
    let index = params
        .kind()
        .descriptors()
        .iter()
        .position(|p| p.id == id)
        .unwrap_or_else(|| panic!("no control {id}"));
    params.get(index).unwrap()
}
fn instrument_set(params: &mut InstrumentParams, id: &str, to: f32) {
    let index = params
        .kind()
        .descriptors()
        .iter()
        .position(|p| p.id == id)
        .unwrap_or_else(|| panic!("no control {id}"));
    assert!(params.set(index, to));
}
/// The project as it is saved and opened again.
fn reopened(c: &windfall_flp::Conversion) -> windfall_project::Project {
    let json = windfall_project::file::to_json(&c.project).unwrap();
    let project = windfall_project::file::from_json(&json).unwrap();
    assert_eq!(project, c.project);
    project
}
/// The effect saved and opened again plays the imported settings and
/// keeps the original state whole.
fn reopens(c: &windfall_flp::Conversion, state: &[u8]) {
    let project = reopened(c);
    assert_eq!(project.mixer.tracks[0].effects[0].params, effect_params(c));
    assert_eq!(project.retained_plugins[0].state, state);
}
/// Little-endian 32-bit words, as the public layouts list them.
fn words(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn put(state: &mut [u8], word: usize, v: i32) {
    state[word * 4..word * 4 + 4].copy_from_slice(&v.to_le_bytes());
}
fn put_float(state: &mut [u8], at: usize, v: f32) {
    state[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
/// Plays `left` and `right` through the effect at `rate`, in blocks of 128.
fn play(
    params: &EffectParams,
    rate: f64,
    tempo: f32,
    left: &[f32],
    right: &[f32],
) -> [Vec<f32>; 2] {
    let mut fx = AnyEffect::new(params);
    fx.prepare(rate as f32, 128);
    assert!(fx.set_params(params));
    fx.set_tempo(tempo);
    let (mut l, mut r) = (left.to_vec(), right.to_vec());
    for (a, b) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
        fx.process(a, b);
    }
    [l, r]
}
/// The complex amplitude of the `hz` component of `signal[range]`.
fn dft(signal: &[f32], range: std::ops::Range<usize>, hz: f64, rate: f64) -> (f64, f64) {
    let (mut re, mut im) = (0.0, 0.0);
    let start = range.start;
    for (offset, x) in signal[range].iter().enumerate() {
        let angle = TAU * hz * (start + offset) as f64 / rate;
        re += f64::from(*x) * angle.cos();
        im -= f64::from(*x) * angle.sin();
    }
    (re, im)
}
fn magnitude((re, im): (f64, f64)) -> f64 {
    (re * re + im * im).sqrt()
}
/// The amplitude of a steady `hz` sine over a whole number of its cycles.
fn amplitude(signal: &[f32], range: std::ops::Range<usize>, hz: f64, rate: f64) -> f64 {
    let len = range.len() as f64;
    2.0 * magnitude(dft(signal, range, hz, rate)) / len
}
fn sine(hz: f64, rate: f64, len: usize, level: f64) -> Vec<f32> {
    (0..len)
        .map(|n| (level * (TAU * hz * n as f64 / rate).sin()) as f32)
        .collect()
}
fn worst(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .fold(0.0_f32, |worst, (a, b)| worst.max((a - b).abs()))
}

#[test]
fn pitch_shifter_wet_and_formant_correction_import_play_and_reopen() {
    // Public prefix (marked unfinished): version, pitch and formant in
    // cents, channel, wet, ..., formant_correction at word 12.
    let mut state = words(&[0; 15]);
    put(&mut state, 1, 1200);
    put(&mut state, 4, 2500);
    let c = effect("Pitch Shifter", &state);
    let params = effect_params(&c);
    assert_eq!(value(&params, "semitones"), 12.0);
    // wet / 10000, the range the dataset gives the mix of the same
    // generation's Frequency Shifter.
    assert_eq!(value(&params, "mix"), 0.25);
    assert_eq!(value(&params, "mode"), 0.0, "no correction: full mix");
    let report = format!("{:?}", c.report);
    assert!(report.contains("wet=2500 -> mix=0.25"), "{report}");
    assert!(!report.contains("wet=2500: omitted"));
    // Outside the read range it is clamped and never voids the prefix.
    for (raw, want) in [(-5, 0.0), (10_000, 1.0), (40_000, 1.0)] {
        let mut other = state.clone();
        put(&mut other, 4, raw);
        let other = effect_params(&effect("Pitch Shifter", &other));
        assert_eq!(value(&other, "mix"), want, "wet={raw}");
        assert_eq!(value(&other, "semitones"), 12.0, "wet={raw}");
    }
    // Formant correction on keeps the formants: the stand-in's voice mode.
    let mut corrected = state.clone();
    put(&mut corrected, 12, 1);
    let corrected = effect("Pitch Shifter", &corrected);
    assert_eq!(value(&effect_params(&corrected), "mode"), 1.0);

    // The mix crossfades the shifted voice with the dry input, which the
    // stand-in delays by its latency: (1 - m) x[n - L] + m wet[n].
    let rate = 48_000.0;
    let input = sine(220.0, rate, 24_000, 0.4);
    let mut fx = AnyEffect::new(&params);
    fx.prepare(rate as f32, 128);
    let latency = fx.latency_samples();
    assert!(latency > 0);
    let [quarter, _] = play(&params, rate, 120.0, &input, &input);
    let mut full = params;
    set(&mut full, "mix", 1.0);
    let [wet, _] = play(&full, rate, 120.0, &input, &input);
    let mut far = 0.0_f32;
    let want: Vec<f32> = (0..input.len())
        .map(|n| {
            let dry = if n >= latency {
                input[n - latency]
            } else {
                0.0
            };
            far = far.max((wet[n] - dry).abs());
            0.75 * dry + 0.25 * wet[n]
        })
        .collect();
    let apart = worst(&quarter, &want);
    println!("R8 pitch shifter mix=0.25: worst={apart:.3e}, shifted vs dry {far:.3}");
    assert!(far > 0.05, "the octave is heard");
    assert!(apart < 1e-5, "{apart}");
    // The voice mode is what plays, and it is not the full mix mode.
    let corrected = effect_params(&corrected);
    let [voice, _] = play(&corrected, rate, 120.0, &input, &input);
    assert!(worst(&voice, &quarter) > 1e-3, "the voice mode is heard");
    reopens(&c, &state);
}

/// The steady response of the stage phaser's left side at `hz` with its
/// sweep frozen at the middle: twelve first-order allpasses at the
/// staggered stage frequencies, fed back through a one-pole low-pass at
/// the color frequency, as the stand-in defines it.
fn phaser_response(
    min_hz: f64,
    max_hz: f64,
    feedback: f64,
    color_hz: f64,
    hz: f64,
    rate: f64,
) -> f64 {
    let center = (min_hz.min(max_hz).ln() + (max_hz.ln() - min_hz.ln()).abs() * 0.5).exp();
    let w = TAU * hz / rate;
    let z1 = (w.cos(), -w.sin()); // z^-1
    let mul = |a: (f64, f64), b: (f64, f64)| (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0);
    let div = |a: (f64, f64), b: (f64, f64)| {
        let d = b.0 * b.0 + b.1 * b.1;
        ((a.0 * b.0 + a.1 * b.1) / d, (a.1 * b.0 - a.0 * b.1) / d)
    };
    let mut chain = (1.0, 0.0);
    for stage in 0..12 {
        let f = (center * (0.72 + 0.055 * stage as f64)).min(0.45 * rate);
        let g = (PI * f / rate).tan();
        let a = ((g - 1.0) / (g + 1.0)).clamp(-0.9995, 0.9995);
        let stage = div((a + z1.0, z1.1), (1.0 + a * z1.0, a * z1.1));
        chain = mul(chain, stage);
    }
    let pole = (-TAU * color_hz.min(rate * 0.45) / rate).exp().min(0.9999);
    let lowpass = div((1.0 - pole, 0.0), (1.0 - pole * z1.0, -pole * z1.1));
    let loop_gain = mul(mul((feedback, 0.0), z1), mul(lowpass, chain));
    let h = div(chain, (1.0 - loop_gain.0, -loop_gain.1));
    (h.0 * h.0 + h.1 * h.1).sqrt()
}

#[test]
fn vintage_phaser_feedback_color_and_tempo_sync_import_play_and_reopen() {
    // 34 public words: phaser_min 1, phaser_max 2, phaser_feedback 3,
    // mod_speed 7, mod_tempo 8, levels_mix 22, color_freq 27.
    let mut state = words(&[0; 34]);
    put(&mut state, 1, 57_459);
    put(&mut state, 2, 1_024_000);
    put(&mut state, 3, 5_250);
    put(&mut state, 22, 100);
    put(&mut state, 27, 65_000);
    let c = effect("Vintage Phaser", &state);
    let params = effect_params(&c);
    // Linear fits of the dataset ranges to the stand-in's: 0..650000 to
    // 200..12000 Hz for the color, as for the sweep's ends.
    assert!(
        (value(&params, "colorHz") - (200.0 + 65_000.0 / 650_000.0 * 11_800.0) as f32).abs() < 1e-3
    );
    assert!((value(&params, "feedback") - 0.6).abs() < 1e-6);
    assert_eq!(value(&params, "mix"), 1.0);
    assert_eq!(value(&params, "rateHz"), 0.0);
    assert_eq!(value(&params, "sync"), 0.0);
    reopens(&c, &state);

    // The sweep frozen at its middle, a sine at a time, against the
    // response written out from the definition: twelve allpasses with
    // their feedback through a one-pole low-pass at the imported color.
    let rate = 48_000.0;
    let (lo, hi) = (
        f64::from(value(&params, "minHz")),
        f64::from(value(&params, "maxHz")),
    );
    let color = f64::from(value(&params, "colorHz"));
    let mut changed = 0.0_f64;
    for hz in [300.0, 800.0, 1_500.0, 3_000.0, 6_000.0] {
        let input = sine(hz, rate, 48_000, 0.2);
        let [left, _] = play(&params, rate, 120.0, &input, &input);
        let got = amplitude(&left, 24_000..48_000, hz, rate) / 0.2;
        let want = phaser_response(lo, hi, 0.6, color, hz, rate);
        let other = phaser_response(lo, hi, 0.6, 2_500.0, hz, rate);
        changed = changed.max((want - other).abs());
        println!(
            "R8 vintage phaser {hz} Hz: got={got:.5} want={want:.5} at 2500 Hz color {other:.5}"
        );
        assert!((got - want).abs() < 2e-3, "{hz} Hz: {got} vs {want}");
    }
    assert!(changed > 0.01, "the color is heard: {changed}");

    // Tempo sync: the fitted rate becomes the cycle that plays it at the
    // import tempo, 120 / 60 / rate beats.
    put(&mut state, 7, 49_152);
    put(&mut state, 8, 1);
    let c = effect("Vintage Phaser", &state);
    let synced = effect_params(&c);
    let rate_hz = value(&synced, "rateHz");
    assert!((rate_hz - 1.0).abs() < 1e-6);
    assert_eq!(value(&synced, "sync"), 1.0);
    assert!((value(&synced, "cycleBeats") - 2.0).abs() < 1e-5);
    let mut free = synced;
    set(&mut free, "sync", 0.0);
    let input = sine(700.0, rate, 48_000, 0.3);
    let [a, b] = play(&synced, rate, 120.0, &input, &input);
    let [c_left, c_right] = play(&free, rate, 120.0, &input, &input);
    let apart = worst(&a, &c_left).max(worst(&b, &c_right));
    assert!(
        apart < 1e-4,
        "synced at 120 BPM plays the fitted rate: {apart}"
    );
    reopens(&c, &state);
}

#[test]
fn vintage_chorus_tempo_sync_keeps_the_fitted_rate_at_the_import_tempo() {
    // 18 public words: mod_speed 3, delay_time1 6, wet 8, mod_tempo_sync 16.
    let mut state = words(&[0; 18]);
    put(&mut state, 3, 49_152);
    put(&mut state, 6, 1_000);
    put(&mut state, 8, 6_000);
    let c = effect("Vintage Chorus", &state);
    let free = effect_params(&c);
    assert_eq!(value(&free, "sync"), 0.0);
    put(&mut state, 16, 1);
    let c = effect("Vintage Chorus", &state);
    let synced = effect_params(&c);
    let rate_hz = f64::from(value(&synced, "rateHz"));
    assert!(rate_hz > 0.0);
    assert_eq!(value(&synced, "sync"), 1.0);
    let beats = (120.0 / 60.0 / rate_hz).clamp(0.125, 16.0);
    assert!((f64::from(value(&synced, "cycleBeats")) - beats).abs() < 1e-5);
    assert!(format!("{:?}", c.report).contains("mod_tempo_sync=1 -> sync=1"));
    // Synced at 120 BPM it plays what the free rate plays.
    let rate = 48_000.0;
    let input = sine(440.0, rate, 48_000, 0.3);
    let [a, b] = play(&synced, rate, 120.0, &input, &input);
    let [c_left, c_right] = play(&free, rate, 120.0, &input, &input);
    let moved = worst(&a, &input);
    let apart = worst(&a, &c_left).max(worst(&b, &c_right));
    println!(
        "R8 vintage chorus synced vs free: {apart:.3e} (chorus moves the input by {moved:.3})"
    );
    assert!(moved > 0.01);
    assert!(apart < 1e-4, "{apart}");
    reopens(&c, &state);
}

#[test]
fn fruity_chorus_wet_only_takes_the_dry_signal_out() {
    // 13 public words: delay 1, depth 2, ..., wetonly 12 (0..1024).
    let mut state = words(&[0; 13]);
    put(&mut state, 1, 512);
    let half = effect_params(&effect("Fruity Chorus", &state));
    assert_eq!(
        value(&half, "mix"),
        0.5,
        "off keeps the stand-in's even mix"
    );
    put(&mut state, 12, 1024);
    let c = effect("Fruity Chorus", &state);
    let wet_only = effect_params(&c);
    assert_eq!(value(&wet_only, "mix"), 1.0);
    assert_eq!(value(&wet_only, "delayMs"), 15.5);
    assert_eq!(value(&wet_only, "depthMs"), 0.0);
    // An impulse: wet only, nothing comes out at once, and every later
    // frame is twice what the even mix gives, which is half dry and half
    // wet with no dry signal after the first frame.
    let rate = 48_000.0;
    let mut input = vec![0.0_f32; 4_800];
    input[0] = 1.0;
    let [only, _] = play(&wet_only, rate, 120.0, &input, &input);
    let [even, _] = play(&half, rate, 120.0, &input, &input);
    assert_eq!(even[0], 0.5);
    assert_eq!(only[0], 0.0);
    let doubled: Vec<f32> = even.iter().map(|x| 2.0 * x).collect();
    let apart = worst(&only[1..], &doubled[1..]);
    let heard = only.iter().fold(0.0_f32, |peak, x| peak.max(x.abs()));
    println!("R8 fruity chorus wet only: worst={apart:.3e}, wet peak {heard:.3}");
    assert!(heard > 0.1, "the wet signal is there");
    assert!(apart < 1e-6, "{apart}");
    reopens(&c, &state);
}

#[test]
fn stereo_shaper_delay_holds_one_side_back_by_the_stated_rule() {
    // 9 public words: reserved, r2l, l2l, r2r, l2r, delay, dephase, iodiff,
    // prepost. Straight through: l2l and r2r at 12800 (unity).
    let mut state = words(&[0; 9]);
    put(&mut state, 2, 12_800);
    put(&mut state, 3, 12_800);
    let rate = 8_000.0;
    let left: Vec<f32> = (0..2_000).map(|n| (0.37 * n as f32).sin() * 0.5).collect();
    let right: Vec<f32> = (0..2_000).map(|n| (0.11 * n as f32).cos() * 0.4).collect();
    for (delay, left_ms, right_ms) in [(2_048, 0.0, 25.0), (-1_024, 12.5, 0.0), (0, 0.0, 0.0)] {
        put(&mut state, 5, delay);
        let c = effect("Fruity Stereo Shaper", &state);
        let params = effect_params(&c);
        assert_eq!(value(&params, "leftDelayMs"), left_ms, "delay={delay}");
        assert_eq!(value(&params, "rightDelayMs"), right_ms, "delay={delay}");
        // |delay| / 4096 of 50 ms, in whole samples at 8 kHz.
        let shift = |ms: f32| (f64::from(ms) * rate / 1000.0).round() as usize;
        let [l, r] = play(&params, rate, 120.0, &left, &right);
        for (out, input, ms) in [(&l, &left, left_ms), (&r, &right, right_ms)] {
            let d = shift(ms);
            let want: Vec<f32> = (0..input.len())
                .map(|n| if n >= d { input[n - d] } else { 0.0 })
                .collect();
            let apart = worst(out, &want);
            assert!(apart < 1e-6, "delay={delay}: {apart}");
        }
        assert!(format!("{:?}", c.report).contains(&format!("delay={delay}: a positive delay")));
        reopens(&c, &state);
    }
}

#[test]
fn panomatic_lfo_target_sends_the_amount_to_pan_volume_or_both() {
    // Six public words: pan, vol, lfo_shape, lfo_target, lfo_amount,
    // lfo_speed. Centered, unity, a sine at half the amount.
    let rate = 8_000.0_f32;
    for (target, pan_depth, volume_depth) in [(0, 0.5, 0.0), (1, 0.0, 0.5), (2, 0.5, 0.5)] {
        let state = words(&[128, 256, 0, target, 64, 5_500]);
        let c = effect("Fruity PanOMatic", &state);
        let params = effect_params(&c);
        assert_eq!(value(&params, "depth"), pan_depth, "target {target}");
        assert_eq!(
            value(&params, "volumeDepth"),
            volume_depth,
            "target {target}"
        );
        assert_eq!(value(&params, "pan"), 0.0);
        assert_eq!(value(&params, "gain"), 1.0);
        let hz = value(&params, "rateHz");
        assert!((hz - (0.01 + 0.1 * 19.99) as f32).abs() < 1e-5);
        // The law, written out: wave = sin(2 pi phase), pan = depth * wave,
        // gain = 1 - volume_depth (1 + wave) / 2; the side the pan moves
        // away from is attenuated by the pan.
        let input = vec![0.5_f32; 8_000];
        let [l, r] = play(&params, f64::from(rate), 120.0, &input, &input);
        let step = f64::from(hz / rate);
        let mut phase = 0.0_f64;
        let mut apart = 0.0_f64;
        for (left, right) in l.iter().zip(&r) {
            let wave = (TAU * phase).sin();
            let pan = (f64::from(pan_depth) * wave).clamp(-1.0, 1.0);
            let gain = 1.0 - f64::from(volume_depth) * (0.5 + 0.5 * wave);
            let want = [
                0.5 * gain * (1.0 - pan.max(0.0)),
                0.5 * gain * (1.0 + pan.min(0.0)),
            ];
            apart = apart
                .max((f64::from(*left) - want[0]).abs())
                .max((f64::from(*right) - want[1]).abs());
            phase = (phase + step).fract();
        }
        println!("R8 panomatic target {target}: worst={apart:.3e}");
        assert!(apart < 1e-4, "target {target}: {apart}");
        reopens(&c, &state);
    }
}

/// Where `signal` crosses zero going up after `from`, in fractional frames.
fn up_crossings(signal: &[f32], from: usize) -> Vec<f64> {
    (from..signal.len() - 1)
        .filter(|&n| signal[n] < 0.0 && signal[n + 1] >= 0.0)
        .map(|n| {
            let (a, b) = (f64::from(signal[n]), f64::from(signal[n + 1]));
            n as f64 + a / (a - b)
        })
        .collect()
}
fn bassdrum_state(base: f32, peak: f32) -> Vec<u8> {
    // SSLF, then 1284 bytes of names and 23 floats: out_level,
    // sampling_ratio, duration, butt, drive, attack, base, peak,
    // slide_time, ..., click_amount (the sixteenth).
    let mut payload = vec![0_u8; 1376];
    for (index, v) in [
        (0, 1.0),
        (1, 0.0),
        (2, 1500.0),
        (6, base),
        (7, peak),
        (8, 150.0),
        (15, 0.0),
    ] {
        put_float(&mut payload, 1284 + index * 4, v);
    }
    let mut state = b"SSLF".to_vec();
    state.extend((payload.len() as u32).to_le_bytes());
    state.extend(payload);
    state
}

#[test]
fn bassdrum_peak_and_base_give_the_pitch_drop() {
    let state = bassdrum_state(100.0, 200.0);
    let c = instrument("BassDrum", &state);
    let params = instrument_params(&c);
    // 12 log2(peak / base) semitones, within the stand-in's 0..60.
    assert_eq!(instrument_value(&params, "pitchDropSemitones"), 12.0);
    assert_eq!(instrument_value(&params, "pitchHz"), 100.0);
    assert_eq!(instrument_value(&params, "pitchDecayMs"), 150.0);
    for (peak, want) in [(100.0, 0.0), (50.0, 0.0), (3_200.0, 60.0), (6_400.0, 60.0)] {
        let other = instrument_params(&instrument("BassDrum", &bassdrum_state(100.0, peak)));
        assert_eq!(
            instrument_value(&other, "pitchDropSemitones"),
            want,
            "peak {peak}"
        );
    }
    // Without a peak the stand-in keeps its own drop.
    let unset = instrument_params(&instrument("BassDrum", &bassdrum_state(100.0, 0.0)));
    assert_eq!(instrument_value(&unset, "pitchDropSemitones"), 36.0);
    assert!(format!("{:?}", c.report).contains("peak=200: with base it gives pitchDropSemitones"));

    // The body's pitch starts an octave up and glides down: counted in
    // cycles between zero crossings, against the integral of
    // base * 2^(drop/12 * e^(-t / 150 ms)), with the body's own frequency
    // at no drop measured from the same synth.
    let rate = 48_000.0;
    let render = |params: &InstrumentParams| {
        let mut synth = AnyInstrument::new(params);
        synth.prepare(rate as f32, 64);
        assert!(synth.set_params(params));
        synth.note_on(60, 1.0);
        let mut left = vec![0.0_f32; 2_880];
        let mut right = left.clone();
        for (l, r) in left.chunks_mut(64).zip(right.chunks_mut(64)) {
            synth.process(l, r);
        }
        left
    };
    let mut flat = params;
    instrument_set(&mut flat, "pitchDropSemitones", 0.0);
    let flat_crossings = up_crossings(&render(&flat), 240);
    let body = (flat_crossings.len() - 1) as f64
        / (flat_crossings[flat_crossings.len() - 1] - flat_crossings[0])
        * rate;
    let crossings = up_crossings(&render(&params), 240);
    let (start, end) = (crossings[0], crossings[crossings.len() - 1]);
    let decay = 0.150 * rate;
    let mut model = 0.0;
    let mut t = start;
    while t < end {
        let dt = (end - t).min(0.25);
        model += body * 2.0_f64.powf((t + dt / 2.0).mul_add(-1.0 / decay, 0.0).exp()) / rate * dt;
        t += dt;
    }
    let counted = (crossings.len() - 1) as f64;
    println!("R8 bassdrum: body {body:.2} Hz, {counted} cycles counted, {model:.3} by the model");
    assert!(counted >= 3.0);
    assert!((counted - model).abs() < 0.05, "{counted} vs {model}");
    let project = reopened(&c);
    assert_eq!(project.retained_plugins[0].state, state);
}

#[test]
fn vocoder_band_count_is_carried_within_the_stand_in_range() {
    // version, num_bands, filter, reserved, left_right, one float per band,
    // then twelve words up to mix_wet.
    let state = |bands: usize| {
        let mut state = vec![0_u8; 17 + bands * 4 + 48];
        put(&mut state, 1, bands as i32);
        let wet = 17 + bands * 4 + 11 * 4;
        state[wet..wet + 4].copy_from_slice(&65_536_i32.to_le_bytes());
        state
    };
    for (bands, want) in [(24, 24.0), (4, 16.0), (64, 32.0)] {
        let c = effect("Fruity Vocoder", &state(bands));
        let params = effect_params(&c);
        assert_eq!(value(&params, "bandCount"), want, "{bands} bands");
        assert_eq!(value(&params, "mix"), 1.0);
        assert!(
            format!("{:?}", c.report).contains(&format!("num_bands={bands} -> bandCount={want}")),
            "{bands}"
        );
        reopens(&c, &state(bands));
    }
    // The count is the stand-in's own control, so what it sounds like is
    // the stand-in's (its DSP tests): here it is enough that 24 bands do
    // not play like the 16 the stand-in had before, with a modulator
    // through the synth carrier.
    let mut params = effect_params(&effect("Fruity Vocoder", &state(24)));
    set(&mut params, "synthCarrier", 1.0);
    let mut sixteen = params;
    set(&mut sixteen, "bandCount", 16.0);
    let rate = 48_000.0;
    let modulator: Vec<f32> = (0..24_000)
        .map(|n| {
            let t = n as f64 / rate;
            ((TAU * 180.0 * t).sin() * 0.3 + (TAU * 1_300.0 * t).sin() * 0.2) as f32
        })
        .collect();
    let [a, _] = play(&params, rate, 120.0, &modulator, &modulator);
    let [b, _] = play(&sixteen, rate, 120.0, &modulator, &modulator);
    let heard = a.iter().fold(0.0_f32, |peak, x| peak.max(x.abs()));
    assert!(heard > 1e-3, "the vocoder sounds");
    assert!(worst(&a, &b) > 1e-4, "24 bands are not 16");
}

fn pitcher_state(keys: &[usize]) -> Vec<u8> {
    // SSLF payload: header, correction_speed at 4, ..., hz at 96, ...,
    // twelve key_on bytes from 136, C first.
    let mut payload = [0_u8; 152];
    put_float(&mut payload, 4, 0.0);
    put_float(&mut payload, 96, (440.0 - 415.3) / 50.9);
    for key in keys {
        payload[136 + key] = 1;
    }
    let mut state = b"SSLF".to_vec();
    state.extend(152_u32.to_le_bytes());
    state.extend(payload);
    state
}

#[test]
fn pitcher_enabled_keys_set_the_root_and_scale_they_form() {
    // D major: D E F# G A B C#.
    let d_major = [2, 4, 6, 7, 9, 11, 1];
    let state = pitcher_state(&d_major);
    let c = effect("Pitcher", &state);
    let params = effect_params(&c);
    assert_eq!(value(&params, "root"), 2.0);
    assert_eq!(value(&params, "scale"), 1.0, "major");
    assert!((value(&params, "tuningHz") - 440.0).abs() < 1e-3);
    assert_eq!(value(&params, "speedMs"), 0.0);
    assert!(format!("{:?}", c.report).contains(
        "enabled keys [C# D E F# G A B] -> root=2 scale=1 (the stand-in's major scale on D)"
    ));
    for (keys, root, scale) in [
        (&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11][..], 0.0, 0.0),
        (&[0, 2, 4, 7, 9][..], 0.0, 3.0),
        (&[9, 11, 0, 2, 4, 5, 7][..], 0.0, 1.0),
        (&[0, 1][..], 0.0, 0.0),
    ] {
        let other = effect("Pitcher", &pitcher_state(keys));
        let p = effect_params(&other);
        assert_eq!(
            (value(&p, "root"), value(&p, "scale")),
            (root, scale),
            "{keys:?}"
        );
        if keys.len() == 2 {
            assert!(format!("{:?}", other.report).contains("form none of the stand-in's scales"));
        }
    }
    reopens(&c, &state);

    // A held tone 30 cents above C4: in D major it is corrected to C#4,
    // the nearest key the scale has (B4 is 130 cents away); chromatic, to
    // C4.
    let rate = 48_000.0;
    let sung = 261.625_565 * 2.0_f64.powf(30.0 / 1200.0);
    let input = sine(sung, rate, 72_000, 0.5);
    let mut chromatic = params;
    set(&mut chromatic, "scale", 0.0);
    set(&mut chromatic, "root", 0.0);
    for (label, params, expected, other) in [
        ("D major", params, 277.182_631, 261.625_565),
        ("chromatic", chromatic, 261.625_565, 277.182_631),
    ] {
        let [out, _] = play(&params, rate, 120.0, &input, &input);
        let at = |hz: f64| amplitude(&out, 48_000..72_000, hz, rate);
        let (want, not, unchanged) = (at(expected), at(other), at(sung));
        println!(
            "R8 pitcher {label}: {expected} Hz {want:.4}, {other} Hz {not:.4}, input {unchanged:.4}"
        );
        assert!(want > 2.0 * not && want > 2.0 * unchanged, "{label}");
    }
}

/// One reverb, impulse in, wet only: with the pre-delay `ms` it is the
/// same reverb with 10 ms of pre-delay held back by the difference. Its
/// modulation is turned off for this, so that the two are the same
/// time-invariant network.
fn reverb_holds_back_by_its_pre_delay(params: &EffectParams, ms: f64) {
    let rate = 48_000.0;
    let mut input = vec![0.0_f32; 48_000];
    input[0] = 1.0;
    let mut params = *params;
    set(&mut params, "modulation", 0.0);
    let mut early = params;
    set(&mut early, "preDelayMs", 10.0);
    let [late, _] = play(&params, rate, 120.0, &input, &input);
    let [soon, _] = play(&early, rate, 120.0, &input, &input);
    let shift = (ms * rate / 1000.0).round() as usize - 480;
    let want: Vec<f32> = (0..input.len())
        .map(|n| if n >= shift { soon[n - shift] } else { 0.0 })
        .collect();
    let apart = worst(&late, &want);
    let heard = late.iter().fold(0.0_f32, |peak, x| peak.max(x.abs()));
    println!("R8 reverb pre-delay {ms} ms: worst={apart:.3e}, peak {heard:.4}");
    assert!(heard > 1e-3, "the reverb sounds");
    assert!(apart < 1e-6, "{apart}");
}

#[test]
fn reeverb_pre_delay_and_modulation_depth_import_play_and_reopen() {
    // Reeverb 2: sixteen words and two bytes; predelay 3 (0..384), mod 15
    // (0..2048); dry 11 and wet 13 give the mix, er 12 the early level.
    let mut state = words(&[1, 20, 200, 192, 50, 80, 15, 200, 100, 0, 0, 0, 0, 128, 0, 0]);
    state.extend([0, 0]);
    let c = effect("Fruity Reeverb 2", &state);
    let params = effect_params(&c);
    assert_eq!(value(&params, "preDelayMs"), 125.0, "192 / 384 of 250 ms");
    assert_eq!(value(&params, "modulation"), 0.0);
    assert_eq!(value(&params, "mix"), 1.0);
    assert_eq!(value(&params, "earlyLevel"), 0.0);
    reverb_holds_back_by_its_pre_delay(&params, 125.0);
    reopens(&c, &state);
    let mut depth = state.clone();
    put(&mut depth, 15, 1_024);
    let deep = effect("Fruity Reeverb 2", &depth);
    let deep_params = effect_params(&deep);
    assert_eq!(value(&deep_params, "modulation"), 0.5, "1024 / 2048");
    // The depth is the stand-in's own modulation control: it is heard.
    let rate = 48_000.0;
    let input = sine(523.0, rate, 48_000, 0.3);
    let [still, _] = play(&params, rate, 120.0, &input, &input);
    let [moving, _] = play(&deep_params, rate, 120.0, &input, &input);
    assert!(worst(&still, &moving) > 1e-4, "the modulation is heard");
    reopens(&deep, &depth);

    // Reeverb: eleven words, predelay 3 taken in milliseconds (0..250).
    let state = words(&[
        1, 0, 65_536, 100, 32_768, 32_768, 0, 6_553, 65_536, 0, 65_536,
    ]);
    let c = effect("Fruity Reeverb", &state);
    let params = effect_params(&c);
    assert_eq!(value(&params, "preDelayMs"), 100.0);
    assert_eq!(value(&params, "mix"), 1.0);
    reverb_holds_back_by_its_pre_delay(&params, 100.0);
    reopens(&c, &state);
}

/// The magnitude of the high cut and low cut the stand-in delay has in its
/// feedback: two state-variable filters at Q 0.707.
fn feedback_filters(hz: f64, low_cut: f64, high_cut: f64, rate: f64) -> f64 {
    let k = 1.0 / 0.707;
    let response = |cutoff: f64, high: bool| {
        let s = (PI * hz / rate).tan() / (PI * cutoff / rate).tan();
        let below = ((1.0 - s * s).powi(2) + (k * s).powi(2)).sqrt();
        if high { s * s / below } else { 1.0 / below }
    };
    response(low_cut, true) * response(high_cut, false)
}

/// The second echo's spectrum over the first's at `hz`: the feedback times
/// the filters in its path.
fn echo_ratio(signal: &[f32], first: usize, gap: usize, hz: f64, rate: f64) -> f64 {
    magnitude(dft(signal, first + gap..first + 2 * gap, hz, rate))
        / magnitude(dft(signal, first..first + gap, hz, rate))
}

#[test]
fn delay_stereo_offset_feedback_cut_cutoff_and_own_tempo_import_play_and_reopen() {
    let rate = 48_000.0;
    let mut impulse = vec![0.0_f32; 30_000];
    impulse[0] = 1.0;

    // Fruity Delay 2: input_pan, input_vol, dry, fb_vol, time (48 to a
    // step), time_stereo_offset (-512..512), fb_mode, fb_cut (0..128).
    let state = words(&[0, 160, 0, 64, 48, 256, 0, 64]);
    let c = effect("Fruity Delay 2", &state);
    let params = effect_params(&c);
    assert_eq!(value(&params, "stereoOffsetMs"), 25.0);
    assert_eq!(value(&params, "highCutHz"), 10_250.0);
    assert_eq!(value(&params, "feedback"), 0.25);
    assert_eq!(value(&params, "mix"), 1.0);
    assert_eq!(value(&params, "sync"), 1.0);
    // A sixteenth at 120 BPM is 6000 frames; the right side waits 25 ms
    // more.
    let [l, r] = play(&params, rate, 120.0, &impulse, &impulse);
    assert!((l[6_000] - 1.0).abs() < 1e-4 && (r[7_200] - 1.0).abs() < 1e-4);
    assert!(r[6_000].abs() < 1e-6 && l[7_200].abs() < 1e-6);
    for hz in [1_000.0, 6_000.0, 12_000.0, 18_000.0] {
        let got = echo_ratio(&l, 6_000, 6_000, hz, rate);
        let want = 0.25 * feedback_filters(hz, 20.0, 10_250.0, rate);
        println!("R8 delay 2 second echo at {hz} Hz: got={got:.5} want={want:.5}");
        assert!((got - want).abs() < 3e-3, "{hz} Hz: {got} vs {want}");
    }
    reopens(&c, &state);

    // Fruity Delay: version, input, fb, cutoff (0..1024), tempo (0..1024),
    // steps, mode. Its own tempo, 60 + 220 * 768/1024 = 225 BPM, counts
    // four steps as a beat: 266.67 ms, 12800 frames, whatever the
    // project's tempo.
    let state = words(&[1, 1_024, 512, 512, 768, 4, 0]);
    let c = effect("Fruity Delay", &state);
    let params = effect_params(&c);
    assert_eq!(value(&params, "sync"), 0.0);
    assert!((value(&params, "timeMs") - 60_000.0 / 225.0).abs() < 1e-3);
    assert_eq!(value(&params, "highCutHz"), 10_250.0);
    assert_eq!(value(&params, "feedback"), 0.25);
    assert_eq!(value(&params, "mix"), 0.5);
    assert!(format!("{:?}", c.report).contains("its own tempo, 60 + 220 * knob BPM"));
    let mut long = vec![0.0_f32; 40_000];
    long[0] = 1.0;
    let [l, _] = play(&params, rate, 90.0, &long, &long);
    let echo = (1..20_000)
        .max_by(|a, b| l[*a].abs().total_cmp(&l[*b].abs()))
        .unwrap();
    assert!(echo.abs_diff(12_800) <= 1, "first echo at {echo}");
    assert!((l[echo] - 0.5).abs() < 0.01, "{}", l[echo]);
    for hz in [1_000.0, 6_000.0, 12_000.0] {
        let got = echo_ratio(&l, 12_798, 12_800, hz, rate);
        let want = 0.25 * feedback_filters(hz, 20.0, 10_250.0, rate);
        println!("R8 delay second echo at {hz} Hz: got={got:.5} want={want:.5}");
        assert!((got - want).abs() < 3e-3, "{hz} Hz: {got} vs {want}");
    }
    reopens(&c, &state);
}

#[test]
fn omitted_fields_outside_the_native_table_are_counted_and_listed_by_device() {
    let listed = |c: &windfall_flp::Conversion| {
        c.report
            .omitted_controls
            .iter()
            .map(|device| {
                (
                    device.device.clone(),
                    device.instances,
                    device.fields.join(" "),
                )
            })
            .collect::<Vec<_>>()
    };
    let omitted_lines = |c: &windfall_flp::Conversion, section: ReportSection| {
        c.report
            .category(section)
            .lines
            .iter()
            .filter(|line| line.outcome == Outcome::Dropped && line.text.contains("omitted"))
            .count()
    };
    // The synth switches are omitted fields now, not notes of the line
    // that says the synth was approximated.
    let c = instrument("SimSynth", &[0; 516]);
    assert_eq!(
        listed(&c),
        [(
            "SimSynth".to_owned(),
            1,
            "osc1_warm osc2_warm osc3_warm lfo_trackamp chorus_on".to_owned()
        )]
    );
    assert_eq!(omitted_lines(&c, ReportSection::Channels), 5);
    assert!(
        c.report
            .category(ReportSection::Channels)
            .lines
            .iter()
            .all(|line| !line.text.contains("warm/unison"))
    );
    let c = instrument("3x Osc", &[0; 92]);
    assert_eq!(
        listed(&c),
        [(
            "3x Osc".to_owned(),
            1,
            "osc1_ofs osc1_detune osc2_ofs osc2_detune osc3_ofs osc3_detune phase_rand osc1_invert osc2_invert osc3_invert osc3_am".to_owned()
        )]
    );
    assert_eq!(omitted_lines(&c, ReportSection::Channels), 11);
    assert!(
        c.report
            .category(ReportSection::Channels)
            .lines
            .iter()
            .all(|line| {
                !line.text.contains("stereo detune of its oscillators")
                    && !line.text.contains("no longer modulates")
            })
    );
    // The effects read by number.
    for (name, bytes, fields) in [
        (
            "Fruity Reeverb 2",
            66,
            "bass cross stereo mod_speed tempo_predelay mid_side",
        ),
        ("Fruity Reeverb", 44, "color"),
        ("Fruity Delay 2", 32, "input_pan"),
        (
            "Fruity Parametric EQ 2",
            148,
            "1_order 2_order 3_order 4_order 5_order 6_order 7_order",
        ),
    ] {
        let c = effect(name, &vec![0; bytes]);
        assert_eq!(
            listed(&c),
            [(name.to_owned(), 1, fields.to_owned())],
            "{name}"
        );
        assert_eq!(
            omitted_lines(&c, ReportSection::Effects),
            fields.split_whitespace().count(),
            "{name}"
        );
    }
    // A channel's own envelope and LFO: the pan envelope with a predelay
    // and an attack tension, and its LFO with a predelay.
    let mut e = stream();
    let mut envelope = [0_u8; 68];
    put(&mut envelope, 1, 1); // enabled
    put(&mut envelope, 2, 300); // predelay, 100 is none
    put(&mut envelope, 3, 1_000); // attack
    put(&mut envelope, 4, 100); // hold: none
    put(&mut envelope, 9, 300); // LFO predelay
    put(&mut envelope, 10, 100); // LFO attack: none
    put(&mut envelope, 11, 64); // LFO amount
    put(&mut envelope, 12, 32_950); // LFO speed: sixteen steps
    put(&mut envelope, 14, 50); // attack tension
    e.data(218, &envelope);
    let c = imported(&e);
    assert_eq!(
        listed(&c),
        [
            (
                "Channel envelope".to_owned(),
                1,
                "predelay attack_tension".to_owned()
            ),
            ("Channel LFO".to_owned(), 1, "predelay".to_owned()),
        ]
    );
    assert_eq!(omitted_lines(&c, ReportSection::Channels), 3);
}
