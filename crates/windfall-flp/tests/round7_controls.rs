//! Decoded public controls that gained a stand-in control in round seven.
//! Every stream is synthetic, every expected number is worked out here from
//! the public rule the importer states, and every audible check compares the
//! processor against a reference written out from its definition in f64.
mod common;
use common::Events;
use windfall_dsp::{AnyEffect, EffectParams};
use windfall_flp::{ConvertOptions, FileFormat, Header, import};

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
fn effect_params(c: &windfall_flp::Conversion) -> EffectParams {
    c.project.mixer.tracks[0].effects[0].params
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
/// The project as it is saved and opened again.
fn reopened(c: &windfall_flp::Conversion) -> windfall_project::Project {
    let json = windfall_project::file::to_json(&c.project).unwrap();
    let project = windfall_project::file::from_json(&json).unwrap();
    assert_eq!(project, c.project);
    project
}

const RATE: f64 = 8000.0;

/// Sixteen bands from fifteen two-section one-pole lowpasses at 40 Hz to
/// 16 kHz in equal ratios, each band the difference of two neighbours, then a
/// whole-sample delay, a level and a balance. No feedback, fully wet.
fn sixteen_band_reference(
    input: &[f64],
    delay_ms: &[f64; 16],
    level: &[f64; 16],
    pan: &[f64; 16],
    right: bool,
) -> Vec<f64> {
    let mut lowpassed = vec![vec![0.0; input.len()]; 15];
    for (edge, out) in lowpassed.iter_mut().enumerate() {
        let hz = 40.0 * 400.0_f64.powf(edge as f64 / 14.0);
        let pole = (-std::f64::consts::TAU * hz.min(RATE * 0.45) / RATE).exp();
        let (mut a, mut b) = (0.0, 0.0);
        for (n, x) in input.iter().enumerate() {
            a = (1.0 - pole) * x + pole * a;
            b = (1.0 - pole) * a + pole * b;
            out[n] = b;
        }
    }
    let mut output = vec![0.0; input.len()];
    for band in 0..16 {
        let delay = (delay_ms[band] * RATE / 1000.0).round() as usize;
        let balance = if right {
            (1.0 + pan[band]).min(1.0)
        } else {
            (1.0 - pan[band]).min(1.0)
        };
        for (m, out) in output.iter_mut().skip(delay).enumerate() {
            let upto = if band < 15 {
                lowpassed[band][m]
            } else {
                input[m]
            };
            let below = if band == 0 {
                0.0
            } else {
                lowpassed[band - 1][m]
            };
            *out += (upto - below) * level[band] * balance;
        }
    }
    output
}

#[test]
fn multiband_delay_sixteen_bands_and_eight_banks_import_play_and_reopen() {
    // Public layout: version, one unnamed word, then eight banks of sixteen
    // delay, sixteen volume and sixteen pan floats.
    let mut state = vec![0_u8; 1544];
    let source = |bank: usize, row: usize, band: usize| -> f32 {
        match row {
            0 => 5.0 + 2.5 * band as f32 + 20.0 * bank as f32,
            1 => 0.2 + 0.05 * band as f32 - 0.02 * bank as f32,
            _ => -0.75 + 0.1 * band as f32 - 0.03 * bank as f32,
        }
    };
    for bank in 0..8 {
        for row in 0..3 {
            for band in 0..16 {
                let at = 8 + bank * 192 + row * 64 + band * 4;
                state[at..at + 4].copy_from_slice(&source(bank, row, band).to_le_bytes());
            }
        }
    }
    // Three values outside the stand-in's range, one per row, in bank 8.
    for (row, band, raw) in [(0, 3, 5000.0_f32), (1, 4, 1.5), (2, 5, -3.0)] {
        let at = 8 + 7 * 192 + row * 64 + band * 4;
        state[at..at + 4].copy_from_slice(&raw.to_le_bytes());
    }
    let c = effect("Multiband Delay", &state);
    let params = effect_params(&c);
    assert_eq!(value(&params, "layout"), 1.0, "the sixteen-band layout");
    assert_eq!(value(&params, "bank"), 1.0);
    let mut checked = 0;
    for bank in 0..8 {
        for (row, id, low, high) in [
            (0, "timeMs", 1.0, 1000.0),
            (1, "level", 0.0, 1.0),
            (2, "pan", -1.0, 1.0),
        ] {
            for band in 0..16 {
                let raw = match (bank, row, band) {
                    (7, 0, 3) => 5000.0,
                    (7, 1, 4) => 1.5,
                    (7, 2, 5) => -3.0,
                    _ => source(bank, row, band),
                };
                let want = raw.clamp(low, high);
                let got = value(&params, &format!("banks.{bank}.{id}.{band}"));
                assert_eq!(got, want, "bank {bank} {id} band {band}");
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 384);
    assert_eq!(value(&params, "banks.7.timeMs.3"), 1000.0);
    assert_eq!(value(&params, "banks.7.level.4"), 1.0);
    assert_eq!(value(&params, "banks.7.pan.5"), -1.0);
    let report = format!("{:?}", c.report);
    assert!(report.contains("bank_8/pan.15="));
    assert!(report.contains("banks.7.pan.15"));
    assert!(!report.contains("no corresponding stand-in control"));

    // What it sounds like. Feedback and mix have no public field, so the
    // test sets them to none and fully wet to hear the bands alone.
    let mut audible = params;
    for id in ["feedback.0", "feedback.1", "feedback.2"] {
        set(&mut audible, id, 0.0);
    }
    set(&mut audible, "mix", 1.0);
    let input: Vec<f64> = (0..1600)
        .map(|n| {
            if n == 0 {
                1.0
            } else {
                0.3 * (0.37 * n as f64).sin() * (-(n as f64) / 200.0).exp()
            }
        })
        .collect();
    let input: Vec<f64> = input.iter().map(|x| f64::from(*x as f32)).collect();
    for bank in [0_usize, 5] {
        set(&mut audible, "bank", bank as f32 + 1.0);
        let mut fx = AnyEffect::new(&audible);
        fx.prepare(RATE as f32, 128);
        assert!(fx.set_params(&audible));
        let mut left: Vec<f32> = input.iter().map(|x| *x as f32).collect();
        let mut right = left.clone();
        fx.process(&mut left, &mut right);
        let delay: [f64; 16] = std::array::from_fn(|band| f64::from(source(bank, 0, band)));
        let level: [f64; 16] = std::array::from_fn(|band| f64::from(source(bank, 1, band)));
        let pan: [f64; 16] = std::array::from_fn(|band| f64::from(source(bank, 2, band)));
        let (mut worst, mut peak) = (0.0_f64, 0.0_f64);
        for (out, side) in [(&left, false), (&right, true)] {
            let want = sixteen_band_reference(&input, &delay, &level, &pan, side);
            for (got, want) in out.iter().zip(&want) {
                worst = worst.max((f64::from(*got) - want).abs());
                peak = peak.max(want.abs());
            }
        }
        println!(
            "R7 multiband bank={} worst={worst:.3e} peak={peak:.4}",
            bank + 1
        );
        assert!(peak > 0.05, "bank {bank}: the reference is audible");
        assert!(worst < 5e-6, "bank {bank}: {worst}");
    }

    let project = reopened(&c);
    assert_eq!(project.mixer.tracks[0].effects[0].params, params);
    assert_eq!(project.retained_plugins[0].state, state);
}

fn instrument(name: &str, b: &[u8]) -> windfall_flp::Conversion {
    let mut e = stream();
    e.byte(21, 2);
    e.text(201, name);
    e.data(213, b);
    imported(&e)
}
fn instrument_params(project: &windfall_project::Project) -> windfall_dsp::InstrumentParams {
    let windfall_project::ChannelSource::Instrument { params, .. } = project.channels[0].source
    else {
        panic!("instrument")
    };
    params
}
fn instrument_value(params: &windfall_dsp::InstrumentParams, id: &str) -> f32 {
    let index = params
        .kind()
        .descriptors()
        .iter()
        .position(|p| p.id == id)
        .unwrap_or_else(|| panic!("no control {id}"));
    params.get(index).unwrap()
}
/// The amplitude of the `hz` component over a whole number of its cycles.
fn component(signal: &[f32], hz: f64, rate: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (n, x) in signal.iter().enumerate() {
        let angle = std::f64::consts::TAU * hz * n as f64 / rate;
        re += f64::from(*x) * angle.cos();
        im += f64::from(*x) * angle.sin();
    }
    2.0 * (re * re + im * im).sqrt() / signal.len() as f64
}

/// Public SimSynth state: 45 doubles, 76 reserved bytes, twenty flag words.
fn simsynth_state(values: &[(usize, f64)], flags: &[usize]) -> Vec<u8> {
    let mut state = vec![0_u8; 516];
    for (index, v) in values {
        state[index * 8..index * 8 + 8].copy_from_slice(&v.to_le_bytes());
    }
    for flag in flags {
        state[436 + flag * 4..440 + flag * 4].copy_from_slice(&1_u32.to_le_bytes());
    }
    state
}

#[test]
fn simsynth_pulse_width_octave_layers_filter_outputs_and_lfo_delay_import_play_and_reopen() {
    // Oscillator n: pw at 7n, level at 7n+3, shape at 7n+6 (0 triangle,
    // .1 saw, .2 square, .3 sine). Flags: on, octave, two octaves, warm.
    let state = simsynth_state(
        &[
            (0, 0.75), // osc1 pw
            (1, 0.5),
            (2, 0.5),
            (3, 1.0),
            (6, 0.2),  // square
            (7, 0.25), // osc2 pw, on a saw: carried, not heard
            (8, 0.5),
            (9, 0.5),
            (10, 0.0),
            (13, 0.1),
            (15, 0.5),
            (16, 0.5),
            (17, 0.0),
            (20, 0.3),
            (21, 0.5), // lfo_del
            (22, 0.5),
            (26, 0.5),  // svf_cut
            (27, 0.5),  // svf_emph
            (30, 0.5),  // svf_kb
            (32, 0.4),  // svf_high
            (33, 0.7),  // svf_band
            (37, 1.0),  // amp sustain
            (39, 0.05), // amp level, low enough for a linear output stage
        ],
        &[0, 4, 5, 8, 10, 12, 13, 14],
    );
    let c = instrument("SimSynth", &state);
    let params = instrument_params(&c.project);
    // .5 - asin((pw - .5) * 2) / pi: a third of the cycle for .75, two
    // thirds for .25.
    assert_eq!(
        instrument_value(&params, "oscillators.0.waveform"),
        4.0,
        "pulse"
    );
    assert!((instrument_value(&params, "oscillators.0.pulseWidth") - 1.0 / 3.0).abs() < 1e-6);
    assert_eq!(
        instrument_value(&params, "oscillators.1.waveform"),
        2.0,
        "saw"
    );
    assert!((instrument_value(&params, "oscillators.1.pulseWidth") - 2.0 / 3.0).abs() < 1e-6);
    for (id, want) in [
        ("oscillators.0.octaveUp", 0.0),
        ("oscillators.0.twoOctavesUp", 0.0),
        ("oscillators.1.octaveUp", 1.0),
        ("oscillators.1.twoOctavesUp", 0.0),
        ("oscillators.2.octaveUp", 0.0),
        ("oscillators.2.twoOctavesUp", 1.0),
        ("highMix", 0.4),
        ("bandMix", 0.7),
    ] {
        assert_eq!(instrument_value(&params, id), want, "{id}");
    }
    let lfos = c.project.channels[0].voice.envelopes.lfos;
    // 4 * .5^3 seconds.
    assert_eq!((lfos.pitch.delay_ms, lfos.pitch.retrigger), (500.0, true));
    assert_eq!((lfos.cutoff.delay_ms, lfos.cutoff.retrigger), (500.0, true));
    let report = format!("{:?}", c.report);
    assert!(!report.contains("pulse-width shaping has no stand-in control"));
    assert!(
        report.contains("osc1_pw") && report.contains("svf_band") && report.contains("lfo_del")
    );

    // One held A3. Overtone n of a pulse high for a share d is sin(pi n d)/n
    // and the filter scales it by |1 + b k s + h s^2| / |s^2 + k s + 1| at
    // s = j tan(pi f / rate) / tan(pi fc / rate), k = 1/Q. Cutoff is MIDI
    // note 100 (2637.02 Hz) and Q is .707 + 9 * (.5 * .8) from the stated
    // rules; b = .7 and h = .4 are the two imported levels.
    let rate = 48_000.0;
    let mut synth = windfall_dsp::AnyInstrument::new(&params);
    synth.prepare(rate as f32, 64);
    assert!(synth.set_params(&params));
    synth.note_on(57, 1.0);
    let mut left = vec![0.0_f32; 72_000];
    let mut right = left.clone();
    for (l, r) in left.chunks_mut(64).zip(right.chunks_mut(64)) {
        synth.process(l, r);
    }
    let held = &left[24_000..];
    let cutoff = 440.0 * 2.0_f64.powf((100.0 - 69.0) / 12.0);
    let k = 1.0 / (0.707 + 9.0 * 0.4);
    let model = |n: f64| {
        let s = (std::f64::consts::PI * 220.0 * n / rate).tan()
            / (std::f64::consts::PI * cutoff / rate).tan();
        let source = (std::f64::consts::PI * n / 3.0).sin().abs() / n;
        let above = ((1.0 - 0.4 * s * s).powi(2) + (0.7 * k * s).powi(2)).sqrt();
        let below = ((1.0 - s * s).powi(2) + (k * s).powi(2)).sqrt();
        source * above / below
    };
    let first = component(held, 220.0, rate);
    assert!(first > 1e-4, "silent: {first}");
    for n in [2.0, 3.0, 4.0, 5.0, 7.0, 8.0] {
        let got = component(held, 220.0 * n, rate) / first;
        let want = model(n) / model(1.0);
        println!("R7 simsynth overtone {n}: got={got:.5} want={want:.5}");
        assert!((got - want).abs() < 0.01, "overtone {n}: {got} vs {want}");
    }

    let project = reopened(&c);
    assert_eq!(instrument_params(&project), params);
    assert_eq!(project.channels[0].voice.envelopes.lfos, lfos);
    assert_eq!(project.retained_plugins[0].state, state);

    // A switched-off source filter has no band or high output to mix in.
    let mut off = simsynth_state(&[(32, 0.4), (33, 0.7), (3, 1.0)], &[0]);
    off[436 + 14 * 4] = 0;
    let c = instrument("SimSynth", &off);
    let params = instrument_params(&c.project);
    assert_eq!(instrument_value(&params, "highMix"), 0.0);
    assert_eq!(instrument_value(&params, "bandMix"), 0.0);
}
