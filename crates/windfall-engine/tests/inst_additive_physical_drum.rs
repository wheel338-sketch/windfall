//! I3/I4 document, disk, history and public live/offline acceptance.
//! Numerical musical behavior is independently covered by each DSP family.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use windfall_dsp::*;
use windfall_engine::{render, Processor, RenderOptions, SamplePool};
use windfall_project::*;

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

struct SavedProject(PathBuf);
impl SavedProject {
    fn new(project: &Project) -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "windfall-builtins-qa-{}-{stamp}-{}.windfall",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        file::save(project, &path).unwrap();
        Self(path)
    }
}
impl Drop for SavedProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn audio(project: &Project, block: usize) -> Vec<f32> {
    let output = render(
        project,
        &SamplePool::new(),
        &RenderOptions {
            block_frames: block,
            ..Default::default()
        },
        &mut |_| true,
    );
    assert!(!output.samples().is_empty());
    assert!(output.samples().iter().all(|sample| sample.is_finite()));
    output.samples().to_vec()
}

fn energy(audio: &[f32]) -> f64 {
    audio
        .iter()
        .map(|&sample| f64::from(sample).powi(2))
        .sum::<f64>()
        / audio.len() as f64
}

fn acceptance(kind: InstrumentKind, initial: InstrumentParams, changed: InstrumentParams) {
    acceptance_key(kind, initial, changed, 60);
}

fn acceptance_key(
    kind: InstrumentKind,
    initial: InstrumentParams,
    changed: InstrumentParams,
    key: u8,
) {
    acceptance_data(kind, initial, changed, key, None, None);
}

fn acceptance_data(
    kind: InstrumentKind,
    initial: InstrumentParams,
    changed: InstrumentParams,
    key: u8,
    initial_asset: Option<InstrumentAsset>,
    changed_asset: Option<InstrumentAsset>,
) {
    let mut doc = Document::new(Project::new("Built-in acceptance QA"));
    let channel = ChannelId(
        doc.dispatch(
            Command::AddChannel {
                name: None,
                sample: None,
                instrument: Some(kind),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let pattern = doc.project().patterns[0].id;
    doc.dispatch(
        Command::SetInstrumentParams {
            channel,
            params: initial,
        },
        None,
    )
    .unwrap();
    if let Some(asset) = &initial_asset {
        let expected = doc.project().channel(channel).unwrap().source.clone();
        doc.dispatch(
            Command::SetInstrumentAsset {
                channel,
                expected,
                asset: Some(asset.clone()),
            },
            None,
        )
        .unwrap();
    }
    doc.dispatch(
        Command::AddNotes {
            pattern,
            channel,
            notes: vec![NoteInit {
                start: 240,
                length: 1920,
                key,
                velocity: Some(1.0),
                pan: None,
                expression: None,
            }],
        },
        None,
    )
    .unwrap();
    let baseline = audio(doc.project(), 64);
    assert!(
        energy(&baseline) > 1e-8,
        "{kind:?} initial patch must sound"
    );
    let mut commands = vec![Command::SetInstrumentParams {
        channel,
        params: changed,
    }];
    if initial_asset != changed_asset {
        let mut expected = doc.project().channel(channel).unwrap().source.clone();
        if let ChannelSource::Instrument { params, .. } = &mut expected {
            *params = changed;
        }
        commands.push(Command::SetInstrumentAsset {
            channel,
            expected,
            asset: changed_asset,
        });
    }
    doc.dispatch(
        Command::Batch {
            label: Some("Edit instrument".to_owned()),
            commands,
        },
        None,
    )
    .unwrap();
    let edited = audio(doc.project(), 7);
    assert!(energy(&edited) > 1e-8, "{kind:?} edited patch must sound");
    let difference: Vec<_> = baseline.iter().zip(&edited).map(|(a, b)| a - b).collect();
    assert!(
        energy(&difference) / energy(&baseline).max(energy(&edited)) > 1e-4,
        "{kind:?} musical parameter edit must reach the public renderer"
    );
    doc.undo().unwrap();
    assert_eq!(
        audio(doc.project(), 127),
        baseline,
        "{kind:?} undo restores exact sound"
    );
    doc.redo().unwrap();
    assert_eq!(
        audio(doc.project(), 1000),
        edited,
        "{kind:?} redo restores exact sound"
    );
    let saved = SavedProject::new(doc.project());
    let loaded = file::load(&saved.0).unwrap();
    assert_eq!(
        loaded.channel(channel).unwrap().source,
        doc.project().channel(channel).unwrap().source
    );
    assert_eq!(
        audio(&loaded, 256),
        edited,
        "{kind:?} disk reopen restores exact sound"
    );

    // Public live playback must agree with offline rendering after host PDC.
    // Compare a prefix ending before the pattern boundary/retrigger.
    let (mut processor, controller) = Processor::new(48_000);
    controller.set_project(&loaded, &SamplePool::new());
    controller.play();
    let latency = controller.latency_frames() as usize;
    let frames = 36_000 + latency;
    let mut live = vec![0.0; frames * 2];
    for chunk in live.chunks_mut(128 * 2) {
        processor.process(chunk);
    }
    assert_eq!(
        &live[latency * 2..(latency + 36_000) * 2],
        &edited[..36_000 * 2],
        "{kind:?} live/offline PCM differs"
    );
}

#[test]
fn finger_bass_tone_reaches_history_disk_and_live_offline_pcm() {
    let initial = FingerBassParams {
        tone: 0.1,
        ..Default::default()
    };
    let changed = FingerBassParams {
        tone: 0.95,
        ..initial
    };
    acceptance(
        InstrumentKind::FingerBass,
        InstrumentParams::FingerBass(initial),
        InstrumentParams::FingerBass(changed),
    );
}

#[test]
fn single_membrane_strike_reaches_history_disk_and_live_offline_pcm() {
    let initial = MembraneParams {
        strike_position: 0.0,
        ..Default::default()
    };
    let changed = MembraneParams {
        strike_position: 0.75,
        damping: 0.9,
        body: 0.9,
        ..initial
    };
    acceptance(
        InstrumentKind::Membrane,
        InstrumentParams::Membrane(initial),
        InstrumentParams::Membrane(changed),
    );
}

#[test]
fn sixteen_pad_membrane_model_reaches_history_disk_and_live_offline_pcm() {
    let initial = DrumRackParams::default();
    let mut changed = initial;
    changed.pads[0].strike_position = 0.85;
    changed.pads[0].damping = 0.9;
    changed.pads[0].body = 0.8;
    acceptance_key(
        InstrumentKind::DrumRack,
        InstrumentParams::DrumRack(initial),
        InstrumentParams::DrumRack(changed),
        36,
    );
}

#[test]
fn per_key_drum_program_reaches_history_disk_and_live_offline_pcm() {
    use windfall_dsp::drums::{DrumMode, DrumVoiceParams};
    let params = InstrumentKind::DrumVoice.default_params();
    let bank = |mode| {
        Some(InstrumentAsset::DrumKeys {
            patches: vec![DrumKeyPatch {
                key: 60,
                params: DrumVoiceParams {
                    mode,
                    ..Default::default()
                },
            }],
        })
    };
    acceptance_data(
        InstrumentKind::DrumVoice,
        params,
        params,
        60,
        bank(DrumMode::Kick),
        bank(DrumMode::Snare),
    );
}

#[test]
fn layer_kick_sample_changes_history_disk_and_live_offline_pcm() {
    let params = InstrumentParams::LayerKick(LayerKickParams {
        sample_mix: 0.7,
        ..Default::default()
    });
    let asset = InstrumentAsset::Pcm(windfall_dsp::instrument_asset::LayerPcm::from_samples(
        48_000,
        &(0..4096)
            .map(|i| (std::f32::consts::TAU * 500.0 * i as f32 / 48_000.0).sin() * 0.5)
            .collect::<Vec<_>>(),
    ));
    acceptance_data(
        InstrumentKind::LayerKick,
        params,
        params,
        36,
        None,
        Some(asset),
    );
}

#[test]
fn immutable_snapshot_morph_reaches_history_disk_and_live_offline_pcm() {
    let initial = SnapshotMorphParams::default();
    let changed = SnapshotMorphParams {
        morph: 1.0,
        ..initial
    };
    let bank = Some(InstrumentAsset::Spectra(
        windfall_dsp::instrument_asset::SpectralBank::harmonic_snapshots(),
    ));
    acceptance_data(
        InstrumentKind::SnapshotMorph,
        InstrumentParams::SnapshotMorph(initial),
        InstrumentParams::SnapshotMorph(changed),
        60,
        bank.clone(),
        bank,
    );
}

#[test]
fn harmonic_filter_cutoff_reaches_history_disk_and_live_offline_pcm() {
    let initial = HarmonicFilterParams {
        cutoff_hz: 20000.0,
        ..Default::default()
    };
    let changed = HarmonicFilterParams {
        cutoff_hz: 300.0,
        resonance: 0.7,
        ..initial
    };
    acceptance(
        InstrumentKind::HarmonicFilter,
        InstrumentParams::HarmonicFilter(initial),
        InstrumentParams::HarmonicFilter(changed),
    );
}

#[test]
fn metallic_shimmer_reaches_history_disk_and_live_offline_pcm() {
    let initial = MetalVoiceParams {
        shimmer_depth: 0.0,
        ..Default::default()
    };
    let changed = MetalVoiceParams {
        shimmer_depth: 1.0,
        shimmer_hz: 9.0,
        stretch: 0.8,
        ..initial
    };
    acceptance(
        InstrumentKind::MetalVoice,
        InstrumentParams::MetalVoice(initial),
        InstrumentParams::MetalVoice(changed),
    );
}

#[test]
fn full_width_numbered_patch_reaches_history_disk_and_live_offline_pcm() {
    let params = InstrumentKind::NumberVoice.default_params();
    acceptance_data(
        InstrumentKind::NumberVoice,
        params,
        params,
        60,
        Some(InstrumentAsset::Spectra(
            windfall_dsp::instrument_asset::SpectralBank::numbered(0),
        )),
        Some(InstrumentAsset::Spectra(
            windfall_dsp::instrument_asset::SpectralBank::numbered(u32::MAX),
        )),
    );
}

#[test]
fn spectral_frame_edit_reaches_history_disk_and_live_offline_pcm() {
    let a = SpectralResynthParams {
        scan: false,
        ..Default::default()
    };
    let b = SpectralResynthParams { position: 1.0, ..a };
    let bank = Some(InstrumentAsset::Spectra(
        windfall_dsp::instrument_asset::SpectralBank::harmonic_snapshots(),
    ));
    acceptance_data(
        InstrumentKind::SpectralResynth,
        InstrumentParams::SpectralResynth(a),
        InstrumentParams::SpectralResynth(b),
        60,
        bank.clone(),
        bank,
    );
}

#[test]
fn color_image_scan_reaches_history_disk_and_live_offline_pcm() {
    let a = ColorScanParams {
        scan: false,
        ..Default::default()
    };
    let b = ColorScanParams {
        scan: true,
        scan_rate: 2.0,
        ..a
    };
    let bank = Some(InstrumentAsset::Spectra(
        windfall_dsp::instrument_asset::SpectralBank::harmonic_snapshots(),
    ));
    acceptance_data(
        InstrumentKind::ColorScan,
        InstrumentParams::ColorScan(a),
        InstrumentParams::ColorScan(b),
        60,
        bank.clone(),
        bank,
    );
}
