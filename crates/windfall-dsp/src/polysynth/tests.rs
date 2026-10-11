use crate::{AnyInstrument, InstrumentKind, InstrumentParams};
use super::*;

fn render(p: InstrumentParams, key: u8, count: usize, block: usize) -> (Vec<f32>, Vec<f32>) {
    let mut synth = AnyInstrument::new(&p); synth.prepare(48000.0, 1024); synth.note_on(key, 1.0);
    let mut l = vec![0.0; count]; let mut r = l.clone();
    for (l, r) in l.chunks_mut(block).zip(r.chunks_mut(block)) { synth.process(l, r); }
    (l, r)
}
fn energy(x: &[f32]) -> f64 { x.iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / x.len() as f64 }
fn amplitude(x: &[f32], hz: f64) -> f64 {
    let mut re = 0.0; let mut im = 0.0; let mut weight = 0.0;
    for (i, &v) in x.iter().enumerate() {
        let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / x.len() as f64).cos();
        let a = std::f64::consts::TAU * hz * i as f64 / 48000.0;
        re += v as f64 * w * a.cos(); im += v as f64 * w * a.sin(); weight += w;
    }
    2.0 * re.hypot(im) / weight
}
fn different(a: InstrumentParams, b: InstrumentParams) {
    let a = render(a, 60, 12000, 64).0; let b = render(b, 60, 12000, 17).0;
    let d: Vec<_> = a.iter().zip(&b).map(|(a,b)| a-b).collect();
    assert!(energy(&d) > energy(&a).max(energy(&b)) * 1e-4);
}
fn contract(kind: InstrumentKind) {
    let p = kind.default_params();
    assert_eq!(render(p, 69, 12000, 1), render(p, 69, 12000, 257));
    let (l, r) = render(p, 69, 12000, 64);
    assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() <= 4.0));
    assert!(energy(&l) > 1e-8);
    let mut synth = AnyInstrument::new(&p); synth.prepare(48000.0, 64);
    for key in 20..90 { synth.note_on(key, 1.0); synth.process(&mut [0.0;64], &mut [0.0;64]); }
    assert!(synth.active_voices() <= 16);
    synth.all_notes_off();
    for _ in 0..(synth.tail_samples() / 64 + 20) { synth.process(&mut [0.0;64], &mut [0.0;64]); }
    assert_eq!(synth.active_voices(), 0);
    let mut l = [1.0;64]; let mut r = l; synth.process(&mut l, &mut r);
    assert_eq!(l, [0.0;64]); assert_eq!(r, l);
    synth.reset(); synth.note_on(60, 1.0); synth.process(&mut l, &mut r);
    let mut other = AnyInstrument::new(&p); other.prepare(48000.0, 64); other.note_on(60,1.0);
    let mut a = [0.0;64]; let mut b = a; other.process(&mut a,&mut b); assert_eq!((l,r),(a,b));
}

#[test]
fn vintage_poly_tuning_filter_pwm_chorus_unison_and_partition() {
    contract(InstrumentKind::VintagePoly);
    let p = VintagePolyParams { drift: 0.0, sub: 0.0, pulse: 0.0, filter_amount: 0.0, key_track: 0.0, cutoff_hz: 18000.0, resonance: 0.0, ..Default::default() };
    let x = render(InstrumentParams::VintagePoly(p),69,60000,64).0;
    let steady = &x[12000..]; let fundamental = amplitude(steady,440.0);
    assert!(fundamental > amplitude(steady,438.0) * 10.0);
    assert!(fundamental > amplitude(steady,442.0) * 10.0);
    let low = render(InstrumentParams::VintagePoly(VintagePolyParams { cutoff_hz: 300.0, ..p }),69,60000,64).0;
    assert!(amplitude(&low[12000..],1760.0) < amplitude(steady,1760.0) * 0.1);
    different(InstrumentParams::VintagePoly(p), InstrumentParams::VintagePoly(VintagePolyParams { pulse:1.0, saw:0.0, pwm:0.35, ..p }));
    different(InstrumentParams::VintagePoly(p), InstrumentParams::VintagePoly(VintagePolyParams { chorus:0.8, ..p }));
    let (l,r) = render(InstrumentParams::VintagePoly(VintagePolyParams { unison:5, ..p }),60,12000,64);
    assert!(energy(&l.iter().zip(r).map(|(a,b)| a-b).collect::<Vec<_>>()) > 1e-6);
}

#[test]
fn vintage_poly_glide_and_envelope_release_have_sample_duration() {
    let p = VintagePolyParams { polyphony:1, glide_ms:100.0, drift:0.0, ..Default::default() };
    let mut synth = VintagePoly::default();
    use crate::Instrument;
    synth.set_params(&p); synth.prepare(48000.0,64); synth.note_on(60,1.0);
    synth.process(&mut [0.0;64],&mut [0.0;64]); synth.note_on(72,1.0);
    let mut notes = core::Notes::default(); notes.note_on(60,1.0,1,100.0,48000.0); notes.slots[0].start = false; notes.note_on(72,1.0,1,100.0,48000.0); assert_eq!(notes.active(),1);
    synth.process(&mut [0.0;2400],&mut [0.0;2400]);
    for _ in 0..2400 { notes.slots[0].glide(); } let pitch = notes.slots[0].pitch;
    assert!((pitch - 66.0).abs() < 0.02);
    synth.process(&mut [0.0;2400],&mut [0.0;2400]);
    for _ in 0..2400 { notes.slots[0].glide(); } assert_eq!(notes.slots[0].pitch,72.0);
    synth.note_off(72); synth.process(&mut [0.0;7300],&mut [0.0;7300]);
    assert_eq!(synth.active_voices(),0);
}

#[test]
fn vintage_poly_high_saw_suppresses_folded_inharmonic_components() {
    let p = VintagePolyParams { drift:0.0, saw:1.0, pulse:0.0, sub:0.0, cutoff_hz:20000.0, resonance:0.0, filter_amount:0.0, key_track:0.0, ..Default::default() };
    let x = render(InstrumentParams::VintagePoly(p),108,60000,64).0;
    let hz = 440.0 * 2.0_f64.powf((108.0-69.0)/12.0);
    let base = amplitude(&x[12000..],hz);
    assert!(base > 0.01);
    for harmonic in [7.0, 9.0, 11.0] {
        let folded = (48000.0 - hz * harmonic).abs();
        assert!(amplitude(&x[12000..],folded) < base * 0.08);
    }
}

#[test]
fn dual_poly_banks_cross_modulation_and_each_filter_response_sound() {
    contract(InstrumentKind::DualPoly);
    let p=DualPolyParams::default();
    different(InstrumentParams::DualPoly(p),InstrumentParams::DualPoly(DualPolyParams { balance:1.0,coarse_b:12.0,..p }));
    different(InstrumentParams::DualPoly(p),InstrumentParams::DualPoly(DualPolyParams { cross_mod:2.0,..p }));
    for response in [PolyResponse::Low2,PolyResponse::Band,PolyResponse::High] {
        different(InstrumentParams::DualPoly(p),InstrumentParams::DualPoly(DualPolyParams { response,..p }));
    }
    let p=DualPolyParams { balance:0.0,sub:0.0,drift:0.0,filter_amount:0.0,key_track:0.0,cutoff_hz:18000.0,..p };
    let x=render(InstrumentParams::DualPoly(p),69,60000,64).0;
    assert!(amplitude(&x[12000..],440.0)>amplitude(&x[12000..],442.0)*10.0);
    let low=render(InstrumentParams::DualPoly(DualPolyParams { cutoff_hz:300.0,..p }),69,60000,64).0;
    assert!(amplitude(&low[12000..],1760.0)<amplitude(&x[12000..],1760.0)*0.1);
}

#[test]
fn compact_synth_quick_oscillators_ring_and_filter_modes_are_distinct() {
    contract(InstrumentKind::CompactSynth);
    let p=CompactSynthParams::default();
    different(InstrumentParams::CompactSynth(p),InstrumentParams::CompactSynth(CompactSynthParams { coarse:12.0,ring:0.7,..p }));
    for response in [CompactResponse::Band,CompactResponse::High] {
        different(InstrumentParams::CompactSynth(p),InstrumentParams::CompactSynth(CompactSynthParams { response,..p }));
    }
    let p=CompactSynthParams { wave_a:crate::Waveform::Sine,mix:0.0,filter_amount:0.0,key_track:0.0,cutoff_hz:18000.0,..p };
    let x=render(InstrumentParams::CompactSynth(p),69,60000,64).0;
    assert!(amplitude(&x[12000..],440.0)>amplitude(&x[12000..],442.0)*10.0);
    different(InstrumentParams::CompactSynth(p),InstrumentParams::CompactSynth(CompactSynthParams { cutoff_hz:200.0,..p }));
}

#[test]
fn vintage_saw_asymmetric_charge_pitch_contour_and_saturating_filter() {
    contract(InstrumentKind::VintageSaw);
    let p=VintageSawParams { drift:0.0,asymmetry:0.0,pulse:0.0,sub:0.0,drive:0.0,filter_amount:0.0,key_track:0.0,cutoff_hz:18000.0,..Default::default() };
    let x=render(InstrumentParams::VintageSaw(p),69,60000,64).0;
    assert!(amplitude(&x[12000..],440.0)>amplitude(&x[12000..],442.0)*10.0);
    for changed in [VintageSawParams { asymmetry:1.0,..p },VintageSawParams { pitch_drop:12.0,..p },VintageSawParams { drive:1.0,resonance:0.8,..p },VintageSawParams { cutoff_hz:300.0,..p }] {
        different(InstrumentParams::VintageSaw(p),InstrumentParams::VintageSaw(changed));
    }
}

#[test]
fn pocket_synth_each_shape_tunes_and_mobile_controls_reach_audio() {
    contract(InstrumentKind::PocketSynth);
    let p=PocketSynthParams { waveform:crate::Waveform::Sine,sub:0.0,filter_amount:0.0,key_track:0.0,cutoff_hz:18000.0,..Default::default() };
    for waveform in [crate::Waveform::Sine,crate::Waveform::Saw,crate::Waveform::Pulse,crate::Waveform::Triangle,crate::Waveform::Square] {
        let x=render(InstrumentParams::PocketSynth(PocketSynthParams { waveform,..p }),69,60000,64).0;
        assert!(amplitude(&x[12000..],440.0)>amplitude(&x[12000..],442.0)*10.0);
    }
    for changed in [PocketSynthParams { cutoff_hz:200.0,..p },PocketSynthParams { vibrato:1.0,..p },PocketSynthParams { delay:0.8,..p },PocketSynthParams { sub:0.8,..p }] {
        different(InstrumentParams::PocketSynth(p),InstrumentParams::PocketSynth(changed));
    }
}

