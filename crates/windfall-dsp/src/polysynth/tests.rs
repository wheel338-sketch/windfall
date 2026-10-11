use super::*;
use crate::{AnyInstrument, InstrumentKind, InstrumentParams};

fn render(p: InstrumentParams, key: u8, count: usize, block: usize) -> (Vec<f32>, Vec<f32>) {
    let mut synth = AnyInstrument::new(&p);
    synth.prepare(48000.0, 1024);
    synth.note_on(key, 1.0);
    let mut l = vec![0.0; count];
    let mut r = l.clone();
    for (l, r) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
        synth.process(l, r);
    }
    (l, r)
}
fn energy(x: &[f32]) -> f64 {
    x.iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / x.len() as f64
}
fn amplitude(x: &[f32], hz: f64) -> f64 {
    let mut re = 0.0;
    let mut im = 0.0;
    let mut weight = 0.0;
    for (i, &v) in x.iter().enumerate() {
        let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / x.len() as f64).cos();
        let a = std::f64::consts::TAU * hz * i as f64 / 48000.0;
        re += v as f64 * w * a.cos();
        im += v as f64 * w * a.sin();
        weight += w;
    }
    2.0 * re.hypot(im) / weight
}
fn different(a: InstrumentParams, b: InstrumentParams) {
    let a = render(a, 60, 12000, 64).0;
    let b = render(b, 60, 12000, 17).0;
    let d: Vec<_> = a.iter().zip(&b).map(|(a, b)| a - b).collect();
    assert!(energy(&d) > energy(&a).max(energy(&b)) * 1e-4);
}
fn contract(kind: InstrumentKind) {
    let p = kind.default_params();
    assert_eq!(render(p, 69, 12000, 1), render(p, 69, 12000, 257));
    let (l, r) = render(p, 69, 12000, 64);
    assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() <= 4.0));
    assert!(energy(&l) > 1e-8);
    let mut synth = AnyInstrument::new(&p);
    synth.prepare(48000.0, 64);
    for key in 20..90 {
        synth.note_on(key, 1.0);
        synth.process(&mut [0.0; 64], &mut [0.0; 64]);
    }
    assert!(synth.active_voices() <= 16);
    synth.all_notes_off();
    for _ in 0..(synth.tail_samples() / 64 + 20) {
        synth.process(&mut [0.0; 64], &mut [0.0; 64]);
    }
    assert_eq!(synth.active_voices(), 0);
    let mut l = [1.0; 64];
    let mut r = l;
    synth.process(&mut l, &mut r);
    assert_eq!(l, [0.0; 64]);
    assert_eq!(r, l);
    synth.reset();
    synth.note_on(60, 1.0);
    synth.process(&mut l, &mut r);
    let mut other = AnyInstrument::new(&p);
    other.prepare(48000.0, 64);
    other.note_on(60, 1.0);
    let mut a = [0.0; 64];
    let mut b = a;
    other.process(&mut a, &mut b);
    assert_eq!((l, r), (a, b));
}

#[test]
fn vintage_poly_tuning_filter_pwm_chorus_unison_and_partition() {
    contract(InstrumentKind::VintagePoly);
    let p = VintagePolyParams {
        drift: 0.0,
        sub: 0.0,
        pulse: 0.0,
        filter_amount: 0.0,
        key_track: 0.0,
        cutoff_hz: 18000.0,
        resonance: 0.0,
        ..Default::default()
    };
    let x = render(InstrumentParams::VintagePoly(p), 69, 60000, 64).0;
    let steady = &x[12000..];
    let fundamental = amplitude(steady, 440.0);
    assert!(fundamental > amplitude(steady, 438.0) * 10.0);
    assert!(fundamental > amplitude(steady, 442.0) * 10.0);
    let low = render(
        InstrumentParams::VintagePoly(VintagePolyParams {
            cutoff_hz: 300.0,
            ..p
        }),
        69,
        60000,
        64,
    )
    .0;
    assert!(amplitude(&low[12000..], 1760.0) < amplitude(steady, 1760.0) * 0.1);
    different(
        InstrumentParams::VintagePoly(p),
        InstrumentParams::VintagePoly(VintagePolyParams {
            pulse: 1.0,
            saw: 0.0,
            pwm: 0.35,
            ..p
        }),
    );
    different(
        InstrumentParams::VintagePoly(p),
        InstrumentParams::VintagePoly(VintagePolyParams { chorus: 0.8, ..p }),
    );
    let (l, r) = render(
        InstrumentParams::VintagePoly(VintagePolyParams { unison: 5, ..p }),
        60,
        12000,
        64,
    );
    assert!(energy(&l.iter().zip(r).map(|(a, b)| a - b).collect::<Vec<_>>()) > 1e-6);
}

#[test]
fn vintage_poly_glide_and_envelope_release_have_sample_duration() {
    let p = VintagePolyParams {
        polyphony: 1,
        glide_ms: 100.0,
        drift: 0.0,
        ..Default::default()
    };
    let mut synth = VintagePoly::default();
    use crate::Instrument;
    synth.set_params(&p);
    synth.prepare(48000.0, 64);
    synth.note_on(60, 1.0);
    synth.process(&mut [0.0; 64], &mut [0.0; 64]);
    synth.note_on(72, 1.0);
    let mut notes = core::Notes::default();
    notes.note_on(60, 1.0, 1, 100.0, 48000.0);
    notes.slots[0].start = false;
    notes.note_on(72, 1.0, 1, 100.0, 48000.0);
    assert_eq!(notes.active(), 1);
    synth.process(&mut [0.0; 2400], &mut [0.0; 2400]);
    for _ in 0..2400 {
        notes.slots[0].glide();
    }
    let pitch = notes.slots[0].pitch;
    assert!((pitch - 66.0).abs() < 0.02);
    synth.process(&mut [0.0; 2400], &mut [0.0; 2400]);
    for _ in 0..2400 {
        notes.slots[0].glide();
    }
    assert_eq!(notes.slots[0].pitch, 72.0);
    synth.note_off(72);
    synth.process(&mut [0.0; 7300], &mut [0.0; 7300]);
    assert_eq!(synth.active_voices(), 0);
}

#[test]
fn vintage_poly_high_saw_suppresses_folded_inharmonic_components() {
    let p = VintagePolyParams {
        drift: 0.0,
        saw: 1.0,
        pulse: 0.0,
        sub: 0.0,
        cutoff_hz: 20000.0,
        resonance: 0.0,
        filter_amount: 0.0,
        key_track: 0.0,
        ..Default::default()
    };
    let x = render(InstrumentParams::VintagePoly(p), 108, 60000, 64).0;
    let hz = 440.0 * 2.0_f64.powf((108.0 - 69.0) / 12.0);
    let base = amplitude(&x[12000..], hz);
    assert!(base > 0.01);
    for harmonic in [7.0, 9.0, 11.0] {
        let folded = (48000.0 - hz * harmonic).abs();
        assert!(amplitude(&x[12000..], folded) < base * 0.08);
    }
}

#[test]
fn dual_poly_banks_cross_modulation_and_each_filter_response_sound() {
    contract(InstrumentKind::DualPoly);
    let p = DualPolyParams::default();
    different(
        InstrumentParams::DualPoly(p),
        InstrumentParams::DualPoly(DualPolyParams {
            balance: 1.0,
            coarse_b: 12.0,
            ..p
        }),
    );
    different(
        InstrumentParams::DualPoly(p),
        InstrumentParams::DualPoly(DualPolyParams {
            cross_mod: 2.0,
            ..p
        }),
    );
    for response in [PolyResponse::Low2, PolyResponse::Band, PolyResponse::High] {
        different(
            InstrumentParams::DualPoly(p),
            InstrumentParams::DualPoly(DualPolyParams { response, ..p }),
        );
    }
    let p = DualPolyParams {
        balance: 0.0,
        sub: 0.0,
        drift: 0.0,
        filter_amount: 0.0,
        key_track: 0.0,
        cutoff_hz: 18000.0,
        ..p
    };
    let x = render(InstrumentParams::DualPoly(p), 69, 60000, 64).0;
    assert!(amplitude(&x[12000..], 440.0) > amplitude(&x[12000..], 442.0) * 10.0);
    let low = render(
        InstrumentParams::DualPoly(DualPolyParams {
            cutoff_hz: 300.0,
            ..p
        }),
        69,
        60000,
        64,
    )
    .0;
    assert!(amplitude(&low[12000..], 1760.0) < amplitude(&x[12000..], 1760.0) * 0.1);
}

#[test]
fn compact_synth_quick_oscillators_ring_and_filter_modes_are_distinct() {
    contract(InstrumentKind::CompactSynth);
    let p = CompactSynthParams::default();
    different(
        InstrumentParams::CompactSynth(p),
        InstrumentParams::CompactSynth(CompactSynthParams {
            coarse: 12.0,
            ring: 0.7,
            ..p
        }),
    );
    for response in [CompactResponse::Band, CompactResponse::High] {
        different(
            InstrumentParams::CompactSynth(p),
            InstrumentParams::CompactSynth(CompactSynthParams { response, ..p }),
        );
    }
    let p = CompactSynthParams {
        wave_a: crate::Waveform::Sine,
        mix: 0.0,
        filter_amount: 0.0,
        key_track: 0.0,
        cutoff_hz: 18000.0,
        ..p
    };
    let x = render(InstrumentParams::CompactSynth(p), 69, 60000, 64).0;
    assert!(amplitude(&x[12000..], 440.0) > amplitude(&x[12000..], 442.0) * 10.0);
    different(
        InstrumentParams::CompactSynth(p),
        InstrumentParams::CompactSynth(CompactSynthParams {
            cutoff_hz: 200.0,
            ..p
        }),
    );
}

#[test]
fn vintage_saw_asymmetric_charge_pitch_contour_and_saturating_filter() {
    contract(InstrumentKind::VintageSaw);
    let p = VintageSawParams {
        drift: 0.0,
        asymmetry: 0.0,
        pulse: 0.0,
        sub: 0.0,
        drive: 0.0,
        filter_amount: 0.0,
        key_track: 0.0,
        cutoff_hz: 18000.0,
        ..Default::default()
    };
    let x = render(InstrumentParams::VintageSaw(p), 69, 60000, 64).0;
    assert!(amplitude(&x[12000..], 440.0) > amplitude(&x[12000..], 442.0) * 10.0);
    for changed in [
        VintageSawParams {
            asymmetry: 1.0,
            ..p
        },
        VintageSawParams {
            pitch_drop: 12.0,
            ..p
        },
        VintageSawParams {
            drive: 1.0,
            resonance: 0.8,
            ..p
        },
        VintageSawParams {
            cutoff_hz: 300.0,
            ..p
        },
    ] {
        different(
            InstrumentParams::VintageSaw(p),
            InstrumentParams::VintageSaw(changed),
        );
    }
}

#[test]
fn pocket_synth_each_shape_tunes_and_mobile_controls_reach_audio() {
    contract(InstrumentKind::PocketSynth);
    let p = PocketSynthParams {
        waveform: crate::Waveform::Sine,
        sub: 0.0,
        filter_amount: 0.0,
        key_track: 0.0,
        cutoff_hz: 18000.0,
        ..Default::default()
    };
    for waveform in [
        crate::Waveform::Sine,
        crate::Waveform::Saw,
        crate::Waveform::Pulse,
        crate::Waveform::Triangle,
        crate::Waveform::Square,
    ] {
        let x = render(
            InstrumentParams::PocketSynth(PocketSynthParams { waveform, ..p }),
            69,
            60000,
            64,
        )
        .0;
        assert!(amplitude(&x[12000..], 440.0) > amplitude(&x[12000..], 442.0) * 10.0);
    }
    for changed in [
        PocketSynthParams {
            cutoff_hz: 200.0,
            ..p
        },
        PocketSynthParams { vibrato: 1.0, ..p },
        PocketSynthParams { delay: 0.8, ..p },
        PocketSynthParams { sub: 0.8, ..p },
    ] {
        different(
            InstrumentParams::PocketSynth(p),
            InstrumentParams::PocketSynth(changed),
        );
    }
}

#[test]
fn groove_synth_real_analog_phase_ring_table_paths_and_tempo_motion() {
    contract(InstrumentKind::GrooveSynth);
    let p = GrooveSynthParams::default();
    for mode in [GrooveMode::Phase, GrooveMode::Ring, GrooveMode::Table] {
        different(
            InstrumentParams::GrooveSynth(p),
            InstrumentParams::GrooveSynth(GrooveSynthParams { mode, ..p }),
        );
        let mode_patch = GrooveSynthParams { mode, ..p };
        different(
            InstrumentParams::GrooveSynth(mode_patch),
            InstrumentParams::GrooveSynth(GrooveSynthParams {
                shape: 1.0,
                index: 4.0,
                mix: 1.0,
                ratio: 5.0,
                ..mode_patch
            }),
        );
    }
    different(
        InstrumentParams::GrooveSynth(p),
        InstrumentParams::GrooveSynth(GrooveSynthParams {
            gate: 1.0,
            division: 0.25,
            ..p
        }),
    );
    let patch = InstrumentParams::GrooveSynth(GrooveSynthParams {
        gate: 1.0,
        motion: 2.0,
        ..p
    });
    let mut a = crate::AnyInstrument::new(&patch);
    let mut b = crate::AnyInstrument::new(&patch);
    a.prepare(48000.0, 64);
    b.prepare(48000.0, 64);
    a.set_tempo(80.0);
    b.set_tempo(160.0);
    a.note_on(60, 1.0);
    b.note_on(60, 1.0);
    let mut x = vec![0.0; 24000];
    let mut y = x.clone();
    let mut r = x.clone();
    a.process(&mut x, &mut r);
    b.process(&mut y, &mut r);
    assert!(energy(&x.iter().zip(y).map(|(a, b)| a - b).collect::<Vec<_>>()) > 1e-6);
}

#[test]
fn hybrid_sequence_algorithms_feedback_key_scaling_and_subtractive_path() {
    contract(InstrumentKind::HybridSeq);
    let mut p = HybridSeqParams::default();
    p.operators[1].ratio = 2.0;
    p.operators[3].ratio = 3.0;
    p.operators[5].ratio = 5.0;
    for algorithm in [
        HybridAlgorithm::Serial,
        HybridAlgorithm::Stack,
        HybridAlgorithm::Parallel,
    ] {
        different(
            InstrumentParams::HybridSeq(p),
            InstrumentParams::HybridSeq(HybridSeqParams { algorithm, ..p }),
        );
    }
    let mut feedback = p;
    feedback.operators[1].feedback = 2.0;
    different(
        InstrumentParams::HybridSeq(p),
        InstrumentParams::HybridSeq(feedback),
    );
    different(
        InstrumentParams::HybridSeq(p),
        InstrumentParams::HybridSeq(HybridSeqParams { saw_mix: 1.0, ..p }),
    );
    let mut scaled = p;
    scaled.operators[0].key_scale = 1.0;
    let a = render(InstrumentParams::HybridSeq(p), 72, 12000, 64).0;
    let b = render(InstrumentParams::HybridSeq(scaled), 72, 12000, 64).0;
    assert!(energy(&b) > energy(&a) * 1.1);
}

#[test]
fn hybrid_sequence_step_pitch_gate_velocity_and_tempo_are_real() {
    let p = HybridSeqParams {
        sequencer: true,
        ..Default::default()
    };
    let mut changed = p;
    changed.steps[1].pitch = 12;
    changed.steps[2].velocity = 0.15;
    changed.steps[3].enabled = false;
    different(
        InstrumentParams::HybridSeq(p),
        InstrumentParams::HybridSeq(changed),
    );
    let a = render(InstrumentParams::HybridSeq(changed), 60, 36000, 1);
    let b = render(InstrumentParams::HybridSeq(changed), 60, 36000, 257);
    assert_eq!(a, b);
    let mut synth = AnyInstrument::new(&InstrumentParams::HybridSeq(changed));
    synth.prepare(48000.0, 64);
    synth.set_tempo(240.0);
    synth.note_on(60, 1.0);
    let mut x = vec![0.0; 36000];
    let mut r = x.clone();
    synth.process(&mut x, &mut r);
    assert!(energy(&x.iter().zip(a.0).map(|(a, b)| a - b).collect::<Vec<_>>()) > 1e-6);
    synth.note_off(60);
    synth.process(&mut [0.0; 8000], &mut [0.0; 8000]);
    assert_eq!(synth.active_voices(), 0);
}

fn bessel(order: usize, x: f64) -> f64 {
    let factorial = |n: usize| (1..=n).fold(1.0, |a, b| a * b as f64);
    (0..18)
        .map(|k| {
            (-1.0_f64).powi(k as i32) * (x * 0.5).powi((2 * k + order) as i32)
                / (factorial(k) * factorial(k + order))
        })
        .sum()
}

#[test]
fn matrix_synth_fm_sidebands_match_bessel_and_ring_matches_product_identity() {
    contract(InstrumentKind::MatrixSynth);
    let mut p = MatrixSynthParams {
        filter_mix: 0.0,
        ..Default::default()
    };
    p.operators[1].ratio = 3.0;
    p.fm[0][1] = 2.0;
    let x = render(InstrumentParams::MatrixSynth(p), 69, 60000, 64).0;
    let carrier = amplitude(&x[12000..], 440.0);
    let sideband = amplitude(&x[12000..], 1760.0);
    // Modulator amplitude = level .5 * operator sustain .8, so depth 2 = .8 rad.
    let reference = bessel(1, 0.8) / bessel(0, 0.8);
    assert!((sideband / carrier - reference).abs() < 0.01);
    p.fm[0][1] = 0.0;
    p.ring[0][1] = 1.0;
    let x = render(InstrumentParams::MatrixSynth(p), 69, 60000, 64).0;
    let difference = amplitude(&x[12000..], 880.0);
    let sum = amplitude(&x[12000..], 1760.0);
    assert!(difference > 0.001);
    assert!((difference / sum - 1.0).abs() < 0.01);
    assert!(amplitude(&x[12000..], 440.0) < sum * 0.01);
}

#[test]
fn matrix_synth_cycles_feedback_key_scaling_modulation_and_filter_are_audible() {
    let p = MatrixSynthParams::default();
    let mut changed = p;
    changed.fm[0][1] = 2.0;
    changed.fm[1][0] = 1.0;
    changed.operators[0].feedback = 1.0;
    different(
        InstrumentParams::MatrixSynth(p),
        InstrumentParams::MatrixSynth(changed),
    );
    let a = render(InstrumentParams::MatrixSynth(changed), 60, 24000, 1);
    let b = render(InstrumentParams::MatrixSynth(changed), 60, 24000, 257);
    assert_eq!(a, b);
    assert!(a.0.iter().chain(&a.1).all(|s| s.is_finite()));
    different(
        InstrumentParams::MatrixSynth(p),
        InstrumentParams::MatrixSynth(MatrixSynthParams {
            lfo_index: 2.0,
            lfo_pitch: 2.0,
            ..p
        }),
    );
    different(
        InstrumentParams::MatrixSynth(p),
        InstrumentParams::MatrixSynth(MatrixSynthParams {
            cutoff_hz: 100.0,
            ..p
        }),
    );
    let base = MatrixSynthParams {
        filter_mix: 0.0,
        ..p
    };
    let mut scaled = base;
    scaled.operators[0].key_scale = 1.0;
    let a = render(InstrumentParams::MatrixSynth(base), 72, 24000, 64).0;
    let b = render(InstrumentParams::MatrixSynth(scaled), 72, 24000, 64).0;
    assert!((energy(&b) / energy(&a) - 4.0).abs() < 0.02);
}

#[test]
fn pack_synth_own_banks_use_real_subtractive_wavetable_multisample_and_fm() {
    contract(InstrumentKind::PackSynth);
    let p = PackSynthParams {
        width: 0.0,
        motion: 0.0,
        ..Default::default()
    };
    for bank in [
        [
            PackPatch::EmberBass,
            PackPatch::EmberGlass,
            PackPatch::EmberKeys,
            PackPatch::EmberBell,
        ],
        [
            PackPatch::PrismLead,
            PackPatch::PrismSweep,
            PackPatch::PrismMallet,
            PackPatch::PrismChime,
        ],
        [
            PackPatch::MeadowPad,
            PackPatch::MeadowAir,
            PackPatch::MeadowPluck,
            PackPatch::MeadowMetal,
        ],
    ] {
        for patch in bank {
            let initial = PackSynthParams { patch, ..p };
            let x = render(InstrumentParams::PackSynth(initial), 69, 12000, 64).0;
            assert!(energy(&x) > 1e-8);
            different(
                InstrumentParams::PackSynth(initial),
                InstrumentParams::PackSynth(PackSynthParams {
                    tone: 0.1,
                    shape: 0.9,
                    ..initial
                }),
            );
        }
        for pair in bank.windows(2) {
            different(
                InstrumentParams::PackSynth(PackSynthParams {
                    patch: pair[0],
                    ..p
                }),
                InstrumentParams::PackSynth(PackSynthParams {
                    patch: pair[1],
                    ..p
                }),
            );
        }
    }
}

#[test]
fn own_pack_assets_are_mip_limited_and_multisamples_have_independent_roots() {
    let assets = super::pack_assets::PackAssets::prepare();
    for bank in 0..3 {
        for key in [36, 48, 60, 72, 84, 96] {
            let mut position = 0.0;
            let x: Vec<_> = (0..12000)
                .map(|_| assets.sample(bank, key, &mut position, key as f32, 48000.0))
                .collect();
            let hz = 440.0 * 2.0_f64.powf((key as f64 - 69.0) / 12.0);
            assert!(amplitude(&x, hz) > amplitude(&x, hz * 1.3) * 3.0);
            assert!(x.iter().all(|x| x.is_finite()));
        }
        let hz = 6000.0;
        let inc = hz / 96000.0;
        let mut phase = 0.0;
        let x: Vec<_> = (0..48000)
            .map(|_| {
                let a = assets.table(bank, phase, inc, 1.0);
                core::advance(&mut phase, inc);
                let b = assets.table(bank, phase, inc, 1.0);
                core::advance(&mut phase, inc);
                (a + b) * 0.5
            })
            .collect();
        assert!(amplitude(&x, 6000.0) > 0.1);
        assert!(amplitude(&x, 3000.0) < 0.001);
    }
}

const FAMILY: [InstrumentKind; 9] = [
    InstrumentKind::VintagePoly,
    InstrumentKind::DualPoly,
    InstrumentKind::CompactSynth,
    InstrumentKind::VintageSaw,
    InstrumentKind::PocketSynth,
    InstrumentKind::GrooveSynth,
    InstrumentKind::HybridSeq,
    InstrumentKind::MatrixSynth,
    InstrumentKind::PackSynth,
];
fn setting(p: &mut InstrumentParams, id: &str, value: f32) -> bool {
    let Some(index) = p.kind().descriptors().iter().position(|info| info.id == id) else {
        return false;
    };
    p.set(index, value)
}
fn plain(kind: InstrumentKind) -> InstrumentParams {
    let mut p = kind.default_params();
    for (path, value) in [
        ("drift", 0.0),
        ("sub", 0.0),
        ("pulse", 0.0),
        ("noise", 0.0),
        ("drive", 0.0),
        ("asymmetry", 0.0),
        ("filterAmount", 0.0),
        ("lfoFilter", 0.0),
        ("keyTrack", 0.0),
        ("resonance", 0.0),
        ("cutoffHz", 18000.0),
        ("motion", 0.0),
        ("chorus", 0.0),
        ("width", 0.0),
        ("shape", 0.0),
        ("tone", 1.0),
        ("mix", 0.0),
        ("balance", 0.0),
        ("sawMix", 0.0),
        ("index", 0.0),
        ("unison", 1.0),
    ] {
        setting(&mut p, path, value);
    }
    p
}

#[test]
fn every_family_row_tunes_to_midi_and_limits_high_note_aliases() {
    for kind in FAMILY {
        let p = plain(kind);
        let x = render(p, 69, 60000, 257).0;
        let fundamental = amplitude(&x[12000..], 440.0);
        assert!(fundamental > 1e-3, "{kind:?} tuning fixture must sound");
        assert!(
            fundamental > amplitude(&x[12000..], 442.0) * 10.0,
            "{kind:?} is mistuned"
        );
        let hz = 440.0 * 2.0_f64.powf((108.0 - 69.0) / 12.0);
        let x = render(p, 108, 60000, 257).0;
        let base = amplitude(&x[12000..], hz);
        for h in [7.0, 9.0, 11.0] {
            assert!(
                amplitude(&x[12000..], (48000.0 - hz * h).abs()) < base * 0.1,
                "{kind:?} high-note folded component"
            );
        }
    }
}

#[test]
fn every_family_row_has_envelopes_glide_and_stereo_voice_behavior() {
    for kind in FAMILY {
        let mut p = plain(kind);
        setting(&mut p, "polyphony", 1.0);
        setting(&mut p, "glideMs", 100.0);
        let mut s = AnyInstrument::new(&p);
        s.prepare(48000.0, 64);
        s.note_on(60, 1.0);
        s.process(&mut [0.0; 6000], &mut [0.0; 6000]);
        s.note_on(72, 1.0);
        assert_eq!(s.active_voices(), 1, "{kind:?} mono legato");
        let mut a = [0.0; 1200];
        let mut b = a;
        s.process(&mut a, &mut b);
        let midpoint = core::increment(61.5, 48000.0) * 48000.0;
        assert!(
            amplitude(&a, midpoint as f64) > amplitude(&a, 523.25),
            "{kind:?} glide moves rather than jumping an octave"
        );
        setting(&mut p, "envelope.releaseMs", 10.0);
        setting(&mut p, "decay", 0.0);
        s.set_params(&p);
        s.process(&mut [0.0; 6000], &mut [0.0; 6000]);
        s.note_off(72);
        s.process(&mut [0.0; 32], &mut [0.0; 32]);
        assert_eq!(
            s.active_voices(),
            1,
            "{kind:?} release must not cut immediately"
        );
        s.process(&mut [0.0; 2100], &mut [0.0; 2100]);
        assert_eq!(s.active_voices(), 0, "{kind:?} release duration");
        // Pocket uses a stereo chorus as the lightweight counterpart of unison.
        let mut wide = p;
        setting(&mut wide, "polyphony", 8.0);
        setting(&mut wide, "glideMs", 0.0);
        if !setting(&mut wide, "unison", 5.0) {
            setting(&mut wide, "width", 1.0);
            setting(&mut wide, "chorus", 1.0);
        }
        let (l, r) = render(wide, 60, 12000, 64);
        assert!(
            energy(&l.iter().zip(r).map(|(a, b)| a - b).collect::<Vec<_>>()) > 1e-8,
            "{kind:?} stereo voices"
        );
    }
}

#[test]
fn every_family_row_filter_sweep_and_live_controls_are_partition_invariant() {
    for kind in FAMILY {
        let initial = plain(kind);
        let mut low = initial;
        if !setting(&mut low, "cutoffHz", 150.0) {
            assert!(setting(&mut low, "tone", 0.0));
        }
        different(initial, low);
        let timeline = |block: usize| {
            let mut s = AnyInstrument::new(&initial);
            s.prepare(48000.0, 257);
            s.note_on(60, 1.0);
            let mut l = vec![0.0; 12000];
            let mut r = l.clone();
            let mut at = 0;
            for end in [197, 701, 913, 12000] {
                if at == 197 {
                    s.set_params(&low);
                }
                if at == 701 {
                    s.note_on(67, 0.7);
                }
                if at == 913 {
                    s.note_off(60);
                }
                while at < end {
                    let next = (at + block).min(end);
                    s.process(&mut l[at..next], &mut r[at..next]);
                    at = next;
                }
            }
            (l, r)
        };
        assert_eq!(
            timeline(1),
            timeline(257),
            "{kind:?} sample-accurate control/notes"
        );
    }
}
