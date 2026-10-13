//! Exhaustive export inventory. A new registry kind must make an explicit
//! asset decision here. Nonempty fixtures exercise content, not runtime IDs.
use super::*;
use windfall_dsp::{EffectKind, EffectParams, InstrumentAsset};
use windfall_dsp::instrument_asset::{LayerPcm, SpectralBank};
use windfall_project::{EffectId, EffectSlot, SamplePlayback};

fn instrument_fixture(program: &mut Program, kind: InstrumentKind) {
    let mut asset = None;
    let mut sample_instrument = None;
    let mut sample_playback = None;
    match kind {
        InstrumentKind::DrumVoice => asset = Some(InstrumentAsset::DrumKeys { patches: vec![windfall_dsp::DrumKeyPatch { key: 60, params: Default::default() }] }),
        InstrumentKind::LayerKick => asset = Some(InstrumentAsset::Pcm(LayerPcm { sample_rate: 48_000, samples: vec![1, 2, -3] })),
        InstrumentKind::SnapshotMorph => asset = Some(InstrumentAsset::Spectra(SpectralBank::harmonic_snapshots())),
        InstrumentKind::HarmonicFilter | InstrumentKind::SpectralResynth | InstrumentKind::ColorScan => asset = Some(InstrumentAsset::Spectra(SpectralBank::harmonics())),
        InstrumentKind::MetalVoice => asset = Some(InstrumentAsset::Spectra(SpectralBank::metallic())),
        InstrumentKind::NumberVoice => asset = Some(InstrumentAsset::Spectra(SpectralBank::numbered(123456))),
        InstrumentKind::SliceMap | InstrumentKind::SliceDeck | InstrumentKind::GrainCloud | InstrumentKind::WaveRide => {
            let mut playback = SamplePlayback { sample: Some(SampleId(2)), sample_b: Some(SampleId(2)), ..Default::default() };
            playback.slices[0].label = "owned slice".into();
            playback.curve.points[0].value = 0.25;
            sample_playback = Some(playback);
        }
        InstrumentKind::MultiSampler | InstrumentKind::SamplePlayer | InstrumentKind::PadBank => {
            sample_instrument = Some(SampleInstrumentAsset::Zones { zones: vec![windfall_dsp::sample_bank::SampleZone { sample: 2, key_low: 36, key_high: 36, ..Default::default() }] });
        }
        InstrumentKind::SoundFont => {
            *program = bank_program();
            return;
        }
        InstrumentKind::PianoKeys => sample_instrument = Some(SampleInstrumentAsset::Piano { electric: true, version: 1 }),
        InstrumentKind::TextVoice => sample_instrument = Some(SampleInstrumentAsset::Speech { text: "hello".into(), pitch: 60.0, duration: 0.5, version: 1, source: Some(SampleId(2)) }),
        // Generated tables/oscillators and saved Copy controls carry all data.
        InstrumentKind::SubtractiveSynth | InstrumentKind::FourOp | InstrumentKind::MatrixFm
        | InstrumentKind::RingHybrid | InstrumentKind::HarmonicStack | InstrumentKind::PartialMorph
        | InstrumentKind::Inharmonic | InstrumentKind::Resynth | InstrumentKind::SeedPatch
        | InstrumentKind::ScanSynth | InstrumentKind::Pluck | InstrumentKind::FingerBass
        | InstrumentKind::AcousticString | InstrumentKind::Membrane | InstrumentKind::DrumRack
        | InstrumentKind::Kick | InstrumentKind::AcidLine | InstrumentKind::TripleOsc
        | InstrumentKind::WaveLane | InstrumentKind::MacroVoice | InstrumentKind::SpeechVoice
        | InstrumentKind::MidiOut | InstrumentKind::VintagePoly | InstrumentKind::DualPoly
        | InstrumentKind::CompactSynth | InstrumentKind::VintageSaw | InstrumentKind::PocketSynth
        | InstrumentKind::GrooveSynth | InstrumentKind::HybridSeq | InstrumentKind::MatrixSynth
        | InstrumentKind::PackSynth | InstrumentKind::Dashboard => {}
    }
    program.project.channels[0].source = ChannelSource::Instrument { params: kind.default_params(), asset };
    program.project.channels[0].sample_instrument = sample_instrument;
    program.project.channels[0].sample_playback = sample_playback;
}

fn effect_fixture(kind: EffectKind) -> EffectSlot {
    use windfall_dsp::asset::{CurvePoint, CurveRef, ShapeRef};
    let points = [CurvePoint::new(0.0, 0.25), CurvePoint::new(1.0, 0.75)];
    let mut params = kind.default_params();
    let mut impulse = None;
    match kind {
        EffectKind::Waveshaper => {
            let EffectParams::Waveshaper(ref mut p) = params else { unreachable!() };
            p.shape = ShapeRef::new(&points).unwrap();
        }
        EffectKind::VolumeGate => {
            let EffectParams::VolumeGate(ref mut p) = params else { unreachable!() };
            p.time_curve = CurveRef::new(&points).unwrap();
            p.volume_curve = CurveRef::new(&points).unwrap();
        }
        EffectKind::Scratch => {
            let EffectParams::Scratch(ref mut p) = params else { unreachable!() };
            p.position_curve = CurveRef::new(&points).unwrap();
            p.source_id = 2;
        }
        EffectKind::Convolver => impulse = Some(windfall_project::ConvolverImpulse { name: "owned IR".into(), sample_rate: 48_000, group_delay: 0, eq_gains: None, frames: vec![[1.0, -0.5], [0.25, 0.125]] }),
        EffectKind::Eq | EffectKind::Compressor | EffectKind::Limiter | EffectKind::Reverb
        | EffectKind::Delay | EffectKind::Balance | EffectKind::DcBlock | EffectKind::ChannelMute
        | EffectKind::Polarity | EffectKind::StereoMatrix | EffectKind::SoftClipper
        | EffectKind::Distortion | EffectKind::FastLowpass | EffectKind::SelectableFilter
        | EffectKind::BassShelf | EffectKind::Lofi | EffectKind::Chorus | EffectKind::Flanger
        | EffectKind::Phaser | EffectKind::BandSplit | EffectKind::MultibandCompressor
        | EffectKind::MultibandMaximizer | EffectKind::TransientShaper | EffectKind::TransientSplit
        | EffectKind::OneKnob | EffectKind::BassHarmonics | EffectKind::Exciter
        | EffectKind::Overdrive | EffectKind::GuitarRack | EffectKind::DriveChain
        | EffectKind::VintageChorus | EffectKind::HyperChorus | EffectKind::VintagePhaser
        | EffectKind::StackedFlanger | EffectKind::BandDelay | EffectKind::Room
        | EffectKind::Spreader | EffectKind::StereoEnhancer | EffectKind::TimeTransport
        | EffectKind::PerformanceRack | EffectKind::FrequencyShifter | EffectKind::PitchShift
        | EffectKind::PitchCorrect | EffectKind::Vocoder | EffectKind::EchoBank
        | EffectKind::FrequencyDelay | EffectKind::SevenBand | EffectKind::MorphEq
        | EffectKind::FilterBank | EffectKind::XyPad | EffectKind::XyzPad | EffectKind::PanLfo
        | EffectKind::EnvelopeFollower | EffectKind::NoteEnvelope | EffectKind::LushSpace
        | EffectKind::Tuner | EffectKind::StageStack | EffectKind::ControlSurface
        | EffectKind::SendTap | EffectKind::MasterLimiter | EffectKind::FormulaSource
        | EffectKind::KeyboardSource | EffectKind::VoltageSource | EffectKind::MusicLighting => {}
    }
    EffectSlot { id: EffectId(10), enabled: true, mix: 1.0, params, impulse }
}

#[test]
fn every_instrument_and_effect_kind_has_an_exhaustive_export_asset_fixture() {
    for kind in InstrumentKind::ALL {
        let mut program = crate::clap::tests::program();
        instrument_fixture(&mut program, kind);
        let reopened = Program::decode(&program.encode().unwrap_or_else(|e| panic!("{kind:?}: {e}"))).unwrap();
        assert_eq!(reopened.project, program.project, "{kind:?} asset content lost");
        assert_eq!(reopened.samples.len(), program.samples.len());
        assert_eq!(reopened.soundfonts.len(), program.soundfonts.len());
    }
    for kind in EffectKind::ALL {
        let mut program = crate::clap::tests::program();
        program.project.mixer.tracks[0].effects.push(effect_fixture(kind));
        let pins = windfall_dsp::asset::AssetPins::for_effects(program.project.mixer.tracks[0].effects.iter().map(|e| &e.params));
        let encoded = program.encode().unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        let reopened = Program::decode(&encoded).unwrap();
        assert_eq!(reopened.project, program.project, "{kind:?} asset content lost");
        assert_eq!(reopened.samples[0].data, program.samples[0].data);
        drop(program);
        drop(pins);
        windfall_dsp::asset::collect_retired();
        assert_eq!(reopened.encode().unwrap(), encoded, "inactive recalled curve roots lost for {kind:?}");
    }
}

fn rack_bank_program(bytes: Option<Vec<u8>>) -> Program {
    use windfall_project::{ChannelRack, RackEdge, RackEndpoint, RackGraph, RackNode, RackNodeAsset, RackProcessor};
    let mut program = crate::clap::tests::program();
    program.project.channels[0].source = ChannelSource::Instrument { params: InstrumentKind::SubtractiveSynth.default_params(), asset: None };
    let processors = [RackProcessor::Instrument { params: InstrumentKind::SoundFont.default_params() }, RackProcessor::NoteInput, RackProcessor::AudioOutput];
    let mut graph = RackGraph { next_id: 4, ..Default::default() };
    for (index, processor) in processors.into_iter().enumerate() {
        graph.nodes.push(RackNode { id: index as u32 + 1, name: "rack bank".into(), position: [0.0, 0.0], muted: false, solo: false, processor, asset: None, hosted: None });
    }
    graph.nodes[0].asset = Some(RackNodeAsset::SoundFont { name: "raw SF2".into(), bank: 0, program: 0, bytes });
    graph.edges = vec![
        RackEdge { from: RackEndpoint { node: 2, port: 3 }, to: RackEndpoint { node: 1, port: 2 }, gain: 1.0, parameter: None },
        RackEdge { from: RackEndpoint { node: 1, port: 1 }, to: RackEndpoint { node: 3, port: 0 }, gain: 1.0, parameter: None },
    ];
    program.project.settings.modular_racks.push(ChannelRack { channel: program.project.channels[0].id, graph });
    program
}

#[test]
fn rack_raw_bank_bytes_share_packet_bounds_and_damaged_recall_is_visible_silence() {
    for bytes in [None, Some(b"broken SF2".to_vec())] {
        let program = rack_bank_program(bytes);
        let saved = program.encode().unwrap();
        assert!(program.validate_soundfont_sources().is_err());
        let mut guest = GuestEngine::activate(&program, 48_000.0).unwrap();
        assert!(guest.preparation_errors()[0].contains("Rack SoundFont"));
        guest.midi(0, 60, 127);
        assert!(guest.render(64).unwrap().iter().all(|v| *v == 0.0));
        assert_eq!(program.encode().unwrap(), saved, "playback silence must preserve original bank data");
    }
    let good = rack_bank_program(Some(sf2::bytes()));
    let mut guest = GuestEngine::activate(&good, 48_000.0).unwrap();
    assert!(guest.preparation_errors().is_empty());
    guest.midi(0, 60, 127);
    let mut peak = 0.0f32;
    for _ in 0..64 { peak = peak.max(guest.render(64).unwrap().iter().map(|v| v.abs()).fold(0.0, f32::max)); }
    assert!(peak > 0.001);
    let too_large = rack_bank_program(Some(vec![0; MAX_RAW_BANK_BYTES + 1]));
    assert!(too_large.encode().err().unwrap().contains("16 MiB"));
    // Main source bytes and rack source bytes share one export limit.
    let mut combined = bank_program();
    combined.soundfonts[0].bytes = vec![0; MAX_RAW_BANK_BYTES];
    combined.project.settings.modular_racks = good.project.settings.modular_racks;
    assert!(combined.encode().err().unwrap().contains("16 MiB"));
}

#[test]
fn script_libraries_and_nested_rack_preset_assets_are_carried_by_content() {
    use windfall_project::{ChannelRack, RackGraph, RackNode, RackNodeAsset, RackProcessor};
    use windfall_project::scripting::{ScriptAsset, ScriptKind, ScriptReference};
    let mut program = crate::clap::tests::program();
    program.project.channels[0].source = ChannelSource::Instrument { params: InstrumentKind::SubtractiveSynth.default_params(), asset: None };
    program.project.settings.scripts.assets.push(ScriptAsset {
        reference: ScriptReference { id: "export-script".into(), revision: 1 },
        language_version: windfall_project::scripting::SCRIPT_VERSION,
        kind: ScriptKind::Piano, name: "own script".into(), source: "set key 61\nemit".into(),
    });
    let mut graph = RackGraph { next_id: 20, ..Default::default() };
    let fixtures = [
        (RackProcessor::Instrument { params: InstrumentKind::SoundFont.default_params() }, RackNodeAsset::SoundFont { name: "owned SF2".into(), bank: 0, program: 0, bytes: Some(sf2::bytes()) }),
        (RackProcessor::Instrument { params: InstrumentKind::LayerKick.default_params() }, RackNodeAsset::Instrument { asset: InstrumentAsset::Pcm(LayerPcm { sample_rate: 48_000, samples: vec![3, -4, 5] }) }),
        (RackProcessor::Effect { params: EffectKind::Convolver.default_params(), mix: 1.0 }, RackNodeAsset::Impulse { impulse: effect_fixture(EffectKind::Convolver).impulse.unwrap() }),
        (RackProcessor::Effect { params: EffectKind::Scratch.default_params(), mix: 1.0 }, RackNodeAsset::Audio { audio: windfall_project::RackAudioAsset { name: "rack scratch".into(), sample_rate: 48_000, frames: Some(vec![[0.25, -0.25]]) } }),
        (RackProcessor::Instrument { params: InstrumentKind::MultiSampler.default_params() }, RackNodeAsset::Zones { zones: vec![windfall_project::RackSampleZone { mapping: Default::default(), audio: windfall_project::RackAudioAsset { name: "rack zone".into(), sample_rate: 48_000, frames: Some(vec![[0.1, -0.1]]) } }] }),
    ];
    for (index, (processor, asset)) in fixtures.into_iter().enumerate() {
        graph.nodes.push(RackNode { id: index as u32 + 1, name: "owned module".into(), position: [0.0, 0.0], muted: false, solo: false, processor, asset: Some(asset), hosted: None });
    }
    let preset = windfall_project::RackPreset::new("owned preset".into(), graph).unwrap();
    let preset = windfall_project::RackPreset::from_json(&preset.to_json().unwrap()).unwrap();
    let outer = RackGraph { next_id: 2, nodes: vec![RackNode { id: 1, name: "nested".into(), position: [0.0, 0.0], muted: false, solo: false, processor: RackProcessor::Rack { graph: Box::new(preset.graph) }, asset: None, hosted: None }], ..Default::default() };
    program.project.settings.modular_racks.push(ChannelRack { channel: program.project.channels[0].id, graph: outer });
    let reopened = Program::decode(&program.encode().unwrap()).unwrap();
    assert_eq!(reopened.project, program.project);
    let windfall_project::RackProcessor::Rack { graph } = &reopened.project.settings.modular_racks[0].graph.nodes[0].processor else { panic!("lost nested graph") };
    assert!(matches!(&graph.nodes[0].asset, Some(RackNodeAsset::SoundFont { bytes: Some(b), .. }) if *b == sf2::bytes()));
}
