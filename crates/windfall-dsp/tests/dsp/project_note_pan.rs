//! Registry-complete admission for project note pan. Existing asset-backed
//! families have separate analytical fixtures in sample_bank and zones tests.
use windfall_core::PanLaw;
use windfall_dsp::{AnyInstrument, InstrumentKind, NoteExpression};
use crate::realtime::allocator_calls;

fn covered(kind: InstrumentKind) -> bool {
    use InstrumentKind::*;
    match kind {
        SubtractiveSynth => false,
        FourOp => true,
        MatrixFm => true,
        RingHybrid => true,
        HarmonicStack => true,
        PartialMorph => true,
        Inharmonic => true,
        Resynth => true,
        SeedPatch => true,
        ScanSynth => true,
        Pluck => true,
        FingerBass => true,
        AcousticString => true,
        Membrane => true,
        DrumRack => true,
        Kick => true,
        DrumVoice => true,
        SliceMap => false,
        SliceDeck => false,
        GrainCloud => false,
        WaveRide => false,
        AcidLine => true,
        TripleOsc => true,
        WaveLane => true,
        MacroVoice => true,
        SpeechVoice => true,
        MidiOut => false,
        VintagePoly => true,
        DualPoly => true,
        CompactSynth => true,
        VintageSaw => true,
        PocketSynth => true,
        GrooveSynth => true,
        HybridSeq => true,
        MatrixSynth => true,
        PackSynth => true,
        LayerKick => true,
        SnapshotMorph => true,
        HarmonicFilter => true,
        MetalVoice => true,
        NumberVoice => true,
        SpectralResynth => true,
        ColorScan => true,
        Dashboard => false,
        MultiSampler => false,
        SamplePlayer => false,
        SoundFont => false,
        PadBank => false,
        PianoKeys => false,
        TextVoice => false,
    }
}
fn unit(kind: InstrumentKind) -> AnyInstrument {
    let mut params = kind.default_params();
    // Effects which cross-feed stereo remain independently covered by their
    // family suites; isolate per-note gains here, before their sends.
    for (index, descriptor) in kind.descriptors().iter().enumerate() {
        if ["chorus", "delay", "phaser", "reverb"].contains(&descriptor.id) {
            params.set(index, 0.0);
        }
    }
    let mut instrument = AnyInstrument::new(&params);
    instrument.prepare(48_000.0, 512);
    instrument
}
fn pcm(unit: &mut AnyInstrument, frames: usize, block: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
    for (l, r) in l.chunks_mut(block).zip(r.chunks_mut(block)) { unit.process(l, r); }
    (l, r)
}
fn reference(law: PanLaw, pan: f32) -> (f32, f32) {
    let power = ((pan+1.0)*std::f32::consts::FRAC_PI_4).sin_cos();
    let power = (power.1 * std::f32::consts::SQRT_2, power.0 * std::f32::consts::SQRT_2);
    match law {
        PanLaw::Balance => (1.0-pan.max(0.0), 1.0+pan.min(0.0)),
        PanLaw::Linear => (1.0-pan, 1.0+pan),
        PanLaw::ConstantPower => power,
        PanLaw::Compromise => ((power.0*(1.0-pan)).sqrt(), (power.1*(1.0+pan)).sqrt()),
    }
}
fn expected(kind: InstrumentKind, sample: f32, gain: f32) -> f32 {
    // The analog boundary saturator is after the voice summation. Invert its
    // centred PCM to independently measure the gain at that summation bus.
    if matches!(kind, InstrumentKind::TripleOsc | InstrumentKind::WaveLane | InstrumentKind::MacroVoice) {
        (sample.atanh()*gain).tanh()
    } else { sample * gain }
}
#[test]
fn every_registered_remaining_family_applies_each_note_law_to_measured_pcm() {
    for kind in InstrumentKind::ALL {
        if !covered(kind) { continue; }
        for law in [PanLaw::Balance, PanLaw::ConstantPower, PanLaw::Compromise, PanLaw::Linear] {
            for pan in [-0.65, 0.0, 0.35, 0.8] {
                let (mut panned, mut center) = (unit(kind), unit(kind));
                panned.set_note_pan_law(law);
                panned.note_on_expression(36, 0.4, pan, NoteExpression::NEUTRAL);
                center.note_on_expression(36, 0.4, 0.0, NoteExpression::NEUTRAL);
                let baseline = pcm(&mut center, 1024, 17);
                let actual = pcm(&mut panned, 1024, 127);
                assert!(baseline.0.iter().chain(&baseline.1).any(|v| v.abs()>1e-7), "silent fixture: {kind:?}");
                let gains = reference(law, pan);
                for i in 0..1024 {
                    assert!((actual.0[i]-expected(kind,baseline.0[i],gains.0)).abs()<2e-6, "{kind:?} {law:?} {pan} left {i}");
                    assert!((actual.1[i]-expected(kind,baseline.1[i],gains.1)).abs()<2e-6, "{kind:?} {law:?} {pan} right {i}");
                }
                panned.reset();
                panned.note_on_expression(36, 0.4, pan, NoteExpression::NEUTRAL);
                assert_eq!(actual, pcm(&mut panned,1024,127), "reset lost law: {kind:?}");
            }
        }
    }
}
#[test]
fn sounding_family_law_changes_ramp_on_the_frame_clock_without_allocator_calls() {
    for kind in InstrumentKind::ALL {
        if !covered(kind) { continue; }
        let mut whole = None;
        for block in [1,17,64,127,512] {
            let (mut panned, mut center) = (unit(kind), unit(kind));
            panned.note_on_expression(36,0.4,0.65,NoteExpression::NEUTRAL);
            center.note_on_expression(36,0.4,0.0,NoteExpression::NEUTRAL);
            pcm(&mut panned,97,block); pcm(&mut center,97,block);
            assert_eq!(allocator_calls(|| panned.set_note_pan_law(PanLaw::Linear)),0);
            let baseline=pcm(&mut center,512,block);
            let (mut l,mut r)=([0.0;512],[0.0;512]);
            for (l,r) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
                assert_eq!(allocator_calls(|| panned.process(l,r)),0);
            }
            let from=reference(PanLaw::Balance,0.65); let to=reference(PanLaw::Linear,0.65);
            for i in 0..512 {
                let part=(i as f32/240.0).min(1.0);
                assert!((l[i]-expected(kind,baseline.0[i],from.0+(to.0-from.0)*part)).abs()<2e-6,"{kind:?} ramp left {i}");
                assert!((r[i]-expected(kind,baseline.1[i],from.1+(to.1-from.1)*part)).abs()<2e-6,"{kind:?} ramp right {i}");
            }
            let actual=(l,r);
            if let Some(reference)=&whole { assert_eq!(&actual,reference,"{kind:?} partition {block}"); } else { whole=Some(actual); }
        }
    }
}
