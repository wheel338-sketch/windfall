//! Nothing that runs on the audio thread may touch the allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use windfall_dsp::blocks::noise::Rng;
use windfall_dsp::LofiParams;
use windfall_dsp::{
    AnyEffect, AnyInstrument, BalanceParams, BassShelfParams, ChannelMuteParams, ChorusParams,
    CompressorParams, DcBlockParams, DelayParams, DistortionParams, EffectKind, EffectParams,
    EffectSlot, EqParams, FastLowpassParams, FlangerParams, InstrumentKind, InstrumentParams,
    LimiterParams, PhaserParams, PolarityParams, ReverbParams, SelectableFilterParams,
    SoftClipperParams, StereoMatrixParams, SynthParams,
};

use crate::support::{noise, random_params};

thread_local! {
    static WATCHING: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
    static BYTES: Cell<usize> = const { Cell::new(0) };
}

/// The system allocator, counting every call made on a thread while that
/// thread is inside [`allocator_calls`].
pub struct CountingAllocator;

fn count(bytes: usize) {
    // The thread-locals hold plain values with no destructor, so reading
    // them here cannot allocate. `try_with` covers a thread being torn down.
    if WATCHING.try_with(Cell::get).unwrap_or(false) {
        let _ = CALLS.try_with(|calls| calls.set(calls.get() + 1));
        let _ = BYTES.try_with(|total| total.set(total.get() + bytes));
    }
}

// SAFETY: every method forwards to the system allocator unchanged. Counting
// reads and writes thread-local cells and nothing else.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: the caller upholds `GlobalAlloc::alloc_zeroed`'s contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count(0);
        // SAFETY: the caller upholds `GlobalAlloc::dealloc`'s contract.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count(new_size);
        // SAFETY: the caller upholds `GlobalAlloc::realloc`'s contract.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

/// Runs `work` and returns how many times it allocated, reallocated or
/// freed memory.
pub(super) fn allocator_calls(work: impl FnOnce()) -> usize {
    CALLS.set(0);
    BYTES.set(0);
    WATCHING.set(true);
    work();
    WATCHING.set(false);
    CALLS.get()
}

#[test]
fn the_counter_sees_allocations_and_frees() {
    let mut kept = None;
    assert_eq!(allocator_calls(|| kept = Some(vec![1_u8; 64])), 1);
    assert_eq!(allocator_calls(|| drop(kept.take())), 1);
    assert_eq!(allocator_calls(|| assert_eq!(2 + 2, 4)), 0);
}

/// Newly allocated payload, separate from frees and pre-existing processor data.
pub(super) fn allocated_bytes(work: impl FnOnce()) -> usize {
    allocator_calls(work);
    BYTES.get()
}

/// Random settings for one kind of effect.
fn random_effect_params(kind: EffectKind, rng: &mut Rng) -> EffectParams {
    match kind {
        EffectKind::Eq => EffectParams::Eq(random_params::<EqParams>(rng)),
        EffectKind::Compressor => EffectParams::Compressor(random_params::<CompressorParams>(rng)),
        EffectKind::Limiter => EffectParams::Limiter(random_params::<LimiterParams>(rng)),
        EffectKind::Reverb => EffectParams::Reverb(random_params::<ReverbParams>(rng)),
        EffectKind::Delay => EffectParams::Delay(random_params::<DelayParams>(rng)),
        EffectKind::Balance => EffectParams::Balance(random_params::<BalanceParams>(rng)),
        EffectKind::DcBlock => EffectParams::DcBlock(random_params::<DcBlockParams>(rng)),
        EffectKind::ChannelMute => {
            EffectParams::ChannelMute(random_params::<ChannelMuteParams>(rng))
        }
        EffectKind::Polarity => EffectParams::Polarity(random_params::<PolarityParams>(rng)),
        EffectKind::StereoMatrix => {
            EffectParams::StereoMatrix(random_params::<StereoMatrixParams>(rng))
        }
        EffectKind::SoftClipper => {
            EffectParams::SoftClipper(random_params::<SoftClipperParams>(rng))
        }
        EffectKind::Distortion => EffectParams::Distortion(random_params::<DistortionParams>(rng)),
        EffectKind::FastLowpass => {
            EffectParams::FastLowpass(random_params::<FastLowpassParams>(rng))
        }
        EffectKind::SelectableFilter => {
            EffectParams::SelectableFilter(random_params::<SelectableFilterParams>(rng))
        }
        EffectKind::BassShelf => EffectParams::BassShelf(random_params::<BassShelfParams>(rng)),
        EffectKind::Lofi => EffectParams::Lofi(random_params::<LofiParams>(rng)),
        EffectKind::Chorus => EffectParams::Chorus(random_params::<ChorusParams>(rng)),
        EffectKind::Flanger => EffectParams::Flanger(random_params::<FlangerParams>(rng)),
        EffectKind::Phaser => EffectParams::Phaser(random_params::<PhaserParams>(rng)),
        EffectKind::BandSplit => {
            EffectParams::BandSplit(random_params::<windfall_dsp::BandSplitParams>(rng))
        }
        EffectKind::MultibandCompressor => EffectParams::MultibandCompressor(random_params::<
            windfall_dsp::MultibandCompressorParams,
        >(rng)),
        EffectKind::MultibandMaximizer => EffectParams::MultibandMaximizer(random_params::<
            windfall_dsp::MultibandMaximizerParams,
        >(rng)),
        EffectKind::TransientShaper => {
            EffectParams::TransientShaper(random_params::<windfall_dsp::TransientShaperParams>(rng))
        }
        EffectKind::TransientSplit => {
            EffectParams::TransientSplit(random_params::<windfall_dsp::TransientSplitParams>(rng))
        }
        EffectKind::OneKnob => {
            EffectParams::OneKnob(random_params::<windfall_dsp::OneKnobParams>(rng))
        }
        EffectKind::BassHarmonics => {
            EffectParams::BassHarmonics(random_params::<windfall_dsp::BassHarmonicsParams>(rng))
        }
        EffectKind::Exciter => {
            EffectParams::Exciter(random_params::<windfall_dsp::ExciterParams>(rng))
        }
        EffectKind::Waveshaper => {
            EffectParams::Waveshaper(random_params::<windfall_dsp::WaveshaperParams>(rng))
        }
        EffectKind::Overdrive => {
            EffectParams::Overdrive(random_params::<windfall_dsp::OverdriveParams>(rng))
        }
        EffectKind::GuitarRack => {
            EffectParams::GuitarRack(random_params::<windfall_dsp::GuitarRackParams>(rng))
        }
        EffectKind::DriveChain => {
            EffectParams::DriveChain(random_params::<windfall_dsp::DriveChainParams>(rng))
        }
        EffectKind::VintageChorus => {
            EffectParams::VintageChorus(random_params::<windfall_dsp::VintageChorusParams>(rng))
        }
        EffectKind::HyperChorus => {
            EffectParams::HyperChorus(random_params::<windfall_dsp::HyperChorusParams>(rng))
        }
        EffectKind::VintagePhaser => {
            EffectParams::VintagePhaser(random_params::<windfall_dsp::VintagePhaserParams>(rng))
        }
        EffectKind::StackedFlanger => {
            EffectParams::StackedFlanger(random_params::<windfall_dsp::StackedFlangerParams>(rng))
        }
        EffectKind::BandDelay => {
            EffectParams::BandDelay(random_params::<windfall_dsp::BandDelayParams>(rng))
        }
        EffectKind::Room => EffectParams::Room(random_params::<windfall_dsp::RoomParams>(rng)),
        EffectKind::Spreader => {
            EffectParams::Spreader(random_params::<windfall_dsp::SpreaderParams>(rng))
        }
        EffectKind::StereoEnhancer => {
            EffectParams::StereoEnhancer(random_params::<windfall_dsp::StereoEnhancerParams>(rng))
        }
        EffectKind::VolumeGate => EffectParams::VolumeGate(random_params::<
            windfall_dsp::performance::VolumeGateParams,
        >(rng)),
        EffectKind::TimeTransport => EffectParams::TimeTransport(random_params::<
            windfall_dsp::performance::TimeTransportParams,
        >(rng)),
        EffectKind::Scratch => EffectParams::Scratch(random_params::<
            windfall_dsp::performance::ScratchParams,
        >(rng)),
        EffectKind::PerformanceRack => EffectParams::PerformanceRack(random_params::<
            windfall_dsp::performance::PerformanceRackParams,
        >(rng)),
        EffectKind::Convolver => EffectParams::Convolver(random_params::<
            windfall_dsp::spectral::ConvolverParams,
        >(rng)),
        EffectKind::FrequencyShifter => EffectParams::FrequencyShifter(random_params::<
            windfall_dsp::spectral::FrequencyShifterParams,
        >(rng)),
        EffectKind::PitchShift => EffectParams::PitchShift(random_params::<
            windfall_dsp::spectral::PitchShiftParams,
        >(rng)),
        EffectKind::PitchCorrect => EffectParams::PitchCorrect(random_params::<
            windfall_dsp::spectral::PitchCorrectParams,
        >(rng)),
        EffectKind::Vocoder => {
            EffectParams::Vocoder(random_params::<windfall_dsp::spectral::VocoderParams>(rng))
        }
        EffectKind::EchoBank => EffectParams::EchoBank(random_params::<
            windfall_dsp::echo_bank::EchoBankParams,
        >(rng)),
        EffectKind::FrequencyDelay => EffectParams::FrequencyDelay(random_params::<
            windfall_dsp::frequency_delay::FrequencyDelayParams,
        >(rng)),
        EffectKind::SevenBand => {
            EffectParams::SevenBand(random_params::<windfall_dsp::eqbank::SevenBandParams>(rng))
        }
        EffectKind::MorphEq => {
            EffectParams::MorphEq(random_params::<windfall_dsp::eqbank::MorphEqParams>(rng))
        }
        EffectKind::FilterBank => {
            EffectParams::FilterBank(random_params::<windfall_dsp::eqbank::FilterBankParams>(rng))
        }
        EffectKind::XyPad => {
            EffectParams::XyPad(random_params::<windfall_dsp::control::XyPadParams>(rng))
        }
        EffectKind::XyzPad => {
            EffectParams::XyzPad(random_params::<windfall_dsp::control::XyzPadParams>(rng))
        }
        EffectKind::PanLfo => {
            EffectParams::PanLfo(random_params::<windfall_dsp::control::PanLfoParams>(rng))
        }
        EffectKind::EnvelopeFollower => EffectParams::EnvelopeFollower(random_params::<
            windfall_dsp::control::EnvelopeFollowerParams,
        >(rng)),
        EffectKind::NoteEnvelope => EffectParams::NoteEnvelope(random_params::<
            windfall_dsp::control::NoteEnvelopeParams,
        >(rng)),
        EffectKind::LushSpace => {
            EffectParams::LushSpace(random_params::<windfall_dsp::lush::LushSpaceParams>(rng))
        }
        EffectKind::Tuner => {
            EffectParams::Tuner(random_params::<windfall_dsp::tuner::TunerParams>(rng))
        }
        EffectKind::StageStack => EffectParams::StageStack(random_params::<
            windfall_dsp::mastering::StageStackParams,
        >(rng)),
        EffectKind::ControlSurface => EffectParams::ControlSurface(random_params::<
            windfall_dsp::surface::ControlSurfaceParams,
        >(rng)),
        EffectKind::SendTap => {
            EffectParams::SendTap(random_params::<windfall_dsp::sendtap::SendTapParams>(rng))
        }
        EffectKind::MasterLimiter => EffectParams::MasterLimiter(random_params::<windfall_dsp::mastering::MasterLimiterParams>(rng)),
    }
}

#[test]
fn effects_never_allocate_after_prepare() {
    const SIZES: [usize; 8] = [128, 1, 7, 64, 480, 512, 33, 256];
    for kind in EffectKind::ALL {
        let mut rng = Rng::new(kind as u32 + 1);
        // Everything the control side would do ahead of time: build the
        // slot, prepare it, take its meter, and line up the settings and
        // audio it is going to be given.
        let mut slot = EffectSlot::new(AnyEffect::new(&kind.default_params()));
        slot.prepare(48_000.0, 512);
        let meter = slot.effect().gain_reduction();
        let settings: Vec<EffectParams> = (0..64)
            .map(|_| random_effect_params(kind, &mut rng))
            .collect();
        let wrong_kind = EffectKind::ALL[(kind as usize + 1) % EffectKind::ALL.len()];
        let wrong = wrong_kind.default_params();
        let mut left = noise(11, 0.8, 512);
        let mut right = noise(12, 0.8, 512);
        let mut loudest = 0.0_f32;

        let calls = allocator_calls(|| {
            for step in 0..800_usize {
                match step % 40 {
                    3 => assert!(slot.set_params(&settings[step % settings.len()])),
                    9 => slot.set_tempo(60.0 + step as f32),
                    14 => slot.set_enabled(step % 80 < 40),
                    17 => slot.set_mix((step % 7) as f32 / 6.0),
                    23 => assert!(!slot.set_params(&wrong)),
                    31 => slot.reset(),
                    _ => {}
                }
                // Setting one control by its index, as automation does.
                if step % 5 == 0 {
                    let mut params = settings[step % settings.len()];
                    params.set(step % 4, 0.5);
                    slot.set_params(&params.sanitized());
                }
                let frames = SIZES[step % SIZES.len()];
                slot.process(&mut left[..frames], &mut right[..frames]);
                loudest = loudest.max(left[0].abs());
                let _ = (slot.latency_samples(), slot.tail_samples());
                if let Some(meter) = &meter {
                    let _ = meter.take_db();
                }
                // Keep the input lively: the slot processes in place.
                left[step % 512] = 0.9;
                right[(step * 7) % 512] = -0.9;
            }
        });
        assert_eq!(calls, 0, "{} touched the allocator", kind.name());
        assert!(loudest > 0.0);
    }
}

#[test]
fn rapid_matrix_edits_across_every_prepared_tap_never_allocate_or_free() {
    let mut slot = EffectSlot::new(AnyEffect::new(&EffectKind::StereoMatrix.default_params()));
    slot.prepare(48_000.0, 1);
    slot.set_mix(0.5);
    let (mut left, mut right) = ([1.0], [-1.0]);
    let calls = allocator_calls(|| {
        // Distinct taps on every sample fill the entire 0..=2400 bound;
        // repeated visits must merge entries rather than grow the mixture.
        for n in 0..4802 {
            let delay = (n % 2401) as f32 / 48.0;
            slot.set_params(&EffectParams::StereoMatrix(StereoMatrixParams {
                left_delay_ms: delay,
                right_delay_ms: delay,
                ..Default::default()
            }));
            left[0] = 1.0;
            right[0] = -1.0;
            slot.process(&mut left, &mut right);
        }
        slot.set_enabled(false);
        slot.set_mix(0.0);
        slot.reset();
        slot.process(&mut left, &mut right);
    });
    assert_eq!(calls, 0, "full prepared tap range touched the allocator");
    assert!(left[0].is_finite() && right[0].is_finite());
}

#[test]
fn a_slot_longer_than_its_prepared_block_still_does_not_allocate() {
    let mut slot = EffectSlot::new(AnyEffect::new(&EffectKind::Reverb.default_params()));
    slot.prepare(44_100.0, 64);
    let mut left = noise(1, 0.5, 4_096);
    let mut right = noise(2, 0.5, 4_096);
    let calls = allocator_calls(|| {
        slot.process(&mut left, &mut right);
        slot.set_enabled(false);
        slot.process(&mut left, &mut right);
    });
    assert_eq!(calls, 0);
}

#[test]
fn the_synth_never_allocates_after_prepare() {
    const SIZES: [usize; 8] = [128, 1, 7, 64, 480, 512, 33, 256];
    let mut rng = Rng::new(99);
    let kind = InstrumentKind::SubtractiveSynth;
    let mut instrument = AnyInstrument::new(&kind.default_params());
    instrument.prepare(48_000.0, 512);
    let settings: Vec<InstrumentParams> = (0..32)
        .map(|_| InstrumentParams::SubtractiveSynth(random_params::<SynthParams>(&mut rng)))
        .collect();
    let mut left = vec![0.0_f32; 512];
    let mut right = vec![0.0_f32; 512];
    let mut loudest = 0.0_f32;

    let calls = allocator_calls(|| {
        for step in 0..1_500_usize {
            let key = (36 + (step * 7) % 60) as u8;
            match step % 30 {
                0 | 2 | 4 | 6 | 8 | 10 => instrument.note_on(key, 0.3 + (step % 7) as f32 * 0.1),
                12 | 14 | 16 => instrument.note_off(key),
                19 => assert!(instrument.set_params(&settings[step % settings.len()])),
                21 => instrument.set_tempo(90.0 + step as f32 * 0.1),
                25 if step % 150 == 25 => instrument.all_notes_off(),
                27 if step % 600 == 27 => instrument.reset(),
                _ => {}
            }
            if step % 9 == 0 {
                let mut params = settings[step % settings.len()];
                params.set(step % 50, 0.25);
                instrument.set_params(&params);
            }
            // A burst of far more notes than there are voices.
            if step % 200 == 100 {
                for key in 20..100 {
                    instrument.note_on(key, 0.5);
                }
            }
            let frames = SIZES[step % SIZES.len()];
            instrument.process(&mut left[..frames], &mut right[..frames]);
            loudest = loudest.max(left[0].abs());
            let _ = (
                instrument.active_voices(),
                instrument.latency_samples(),
                instrument.tail_samples(),
            );
        }
    });
    assert_eq!(calls, 0, "the synth touched the allocator");
    assert!(loudest > 0.0);
}

#[test]
fn subtractive_hybrid_matrix_and_pack_callbacks_do_not_allocate_or_free() {
    for kind in [InstrumentKind::VintagePoly,InstrumentKind::DualPoly,InstrumentKind::CompactSynth,
        InstrumentKind::VintageSaw,InstrumentKind::PocketSynth,InstrumentKind::GrooveSynth,
        InstrumentKind::HybridSeq,InstrumentKind::MatrixSynth,InstrumentKind::PackSynth] {
        let mut params=kind.default_params();
        let mut unit=AnyInstrument::new(&params);unit.prepare(48000.0,64);
        let mut l=[0.0;257];let mut r=l;
        let calls=allocator_calls(|| {
            for i in 0..kind.descriptors().len() {
                let info=&kind.descriptors()[i];params.set(i,info.max);unit.set_params(&params);
                unit.note_on((36+i%60) as u8,0.8);unit.process(&mut l,&mut r);
                unit.note_off((36+i%60) as u8);unit.set_tempo(80.0+i as f32);
                if i%7==0 { unit.all_notes_off();unit.reset(); }
            }
            unit.all_notes_off();unit.process(&mut l,&mut r);unit.reset();
        });
        assert_eq!(calls,0,"{kind:?} callback allocation/reallocation/free");
        assert!(l.iter().chain(&r).all(|x|x.is_finite()));
    }
}

#[test]
fn prepared_i3_i4_assets_events_extremes_and_reset_never_allocate_or_free() {
    use windfall_dsp::instrument_asset::{LayerPcm, SpectralBank};
    use windfall_dsp::{DrumKeyPatch, InstrumentAsset};
    let cases = [
        InstrumentKind::FingerBass,
        InstrumentKind::Membrane,
        InstrumentKind::DrumRack,
        InstrumentKind::DrumVoice,
        InstrumentKind::LayerKick,
        InstrumentKind::SnapshotMorph,
        InstrumentKind::HarmonicFilter,
        InstrumentKind::MetalVoice,
        InstrumentKind::NumberVoice,
        InstrumentKind::SpectralResynth,
        InstrumentKind::ColorScan,
    ];
    for kind in cases {
        let asset = match kind {
            InstrumentKind::LayerKick => Some(InstrumentAsset::Pcm(LayerPcm::from_samples(
                48000,
                &[0.0, 0.5, -0.5, 0.25, 0.0],
            ))),
            InstrumentKind::DrumVoice => Some(InstrumentAsset::DrumKeys {
                patches: vec![DrumKeyPatch {
                    key: 36,
                    params: Default::default(),
                }],
            }),
            InstrumentKind::SnapshotMorph => {
                Some(InstrumentAsset::Spectra(SpectralBank::harmonic_snapshots()))
            }
            InstrumentKind::NumberVoice => {
                Some(InstrumentAsset::Spectra(SpectralBank::numbered(u32::MAX)))
            }
            InstrumentKind::MetalVoice => Some(InstrumentAsset::Spectra(SpectralBank::metallic())),
            InstrumentKind::HarmonicFilter
            | InstrumentKind::SpectralResynth
            | InstrumentKind::ColorScan => {
                Some(InstrumentAsset::Spectra(SpectralBank::harmonics()))
            }
            _ => None,
        };
        let mut synth = AnyInstrument::new_with_asset(&kind.default_params(), asset.as_ref());
        synth.prepare(48000.0, 257);
        let mut l = [0.0; 257];
        let mut r = [0.0; 257];
        let mut finite = true;
        let mut peak = 0.0f32;
        let settings: Vec<_> = [
            -f32::MAX,
            f32::MAX,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ]
        .into_iter()
        .map(|value| {
            let mut p = kind.default_params();
            for i in 0..kind.descriptors().len() {
                p.set(i, value);
            }
            p
        })
        .collect();
        let calls = allocator_calls(|| {
            for step in 0..96 {
                synth.set_params(&settings[step % settings.len()]);
                for key in 36..52 {
                    synth.note_on(key, 1.0);
                }
                synth.note_on(255, f32::NAN);
                synth.note_on(60, 0.0);
                let frames = [1, 7, 31, 257][step % 4];
                synth.process(&mut l[..frames], &mut r[..frames]);
                for x in l[..frames].iter().chain(&r[..frames]) {
                    finite &= x.is_finite();
                    peak = peak.max(x.abs());
                }
                synth.note_off(36);
                if step % 7 == 0 {
                    synth.all_notes_off();
                }
                if step % 13 == 0 {
                    synth.reset();
                }
            }
            synth.all_notes_off();
            for _ in 0..16 {
                synth.process(&mut l, &mut r);
            }
            synth.reset();
            synth.process(&mut l, &mut r);
        });
        assert_eq!(calls, 0, "{kind:?} touched the callback allocator");
        assert!(finite && peak < 20000.0, "{kind:?} unbounded PCM");
        assert!(l.iter().chain(&r).all(|x| *x == 0.0));
    }
}
