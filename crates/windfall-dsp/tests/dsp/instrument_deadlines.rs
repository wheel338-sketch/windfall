//! Non-ignored offline load gates for the parity instruments. Run this module
//! with --test-threads=1 for acceptance; it needs no audio device or content IO.
//! Construction/preparation are measured separately from the guarded callback.
//! The original stress spectra, PCM and key-program values here are CC0-1.0.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use windfall_dsp::drums::DrumMode;
use windfall_dsp::instrument_asset::{
    LayerPcm, MAX_LAYER_FRAMES, MIN_LAYER_RATE, SpectralBank, SpectralFrame, SpectralPartial,
};
use windfall_dsp::polysynth::{GrooveMode, HybridAlgorithm, PackPatch};
use windfall_dsp::{
    AnyInstrument, DrumKeyPatch, InstrumentAsset, InstrumentKind, InstrumentParams,
};

use super::realtime::{allocated_bytes, allocator_calls};

const RATE: usize = 48_000;
const BLOCK: usize = 128;
const BLOCKS: usize = 750; // Exactly two seconds, never padded with idle audio.
const MAX_RATIO: f64 = 0.75;
const MAX_PEAK: f32 = 256.0;
const MAX_PREPARATION: Duration = Duration::from_secs(15);
const MAX_PREPARATION_BYTES: usize = 64 * 1024 * 1024;
const MAX_TAIL_FRAMES: usize = RATE * 60;

// Serialize just these profiles when the harness uses multiple workers. This
// test-only lock is acquired before preparation/timing, never inside a callback.
static PROFILES: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct PcmStats {
    nonfinite: bool,
    peak: f32,
    energy: f64,
    hash: u64,
}

impl PcmStats {
    fn observe(&mut self, left: &[f32], right: &[f32]) {
        for &sample in left.iter().chain(right) {
            self.nonfinite |= !sample.is_finite();
            self.peak = self.peak.max(sample.abs());
            self.energy += f64::from(sample) * f64::from(sample);
            self.hash = self.hash.rotate_left(7) ^ u64::from(sample.to_bits());
        }
    }

    fn assert_bounded(&self, label: &str) {
        assert!(!self.nonfinite, "{label}: nonfinite output");
        assert!(self.peak <= MAX_PEAK, "{label}: peak {}", self.peak);
    }
}

/// Maximize the dimensions that affect work, rather than turning attack to
/// four seconds or closing the filter and accidentally measuring silent audio.
/// Every available stage is enabled; matrices have all 72 routes nonzero.
fn dense_params(kind: InstrumentKind) -> InstrumentParams {
    let mut params = kind.default_params();
    for (index, info) in kind.descriptors().iter().enumerate() {
        let leaf = info.id.rsplit('.').next().unwrap();
        let value = match leaf {
            "polyphony" | "unison" | "chorus" | "delay" | "phaser" | "reverb"
            | "echo" | "space" | "width" | "air" | "crunch" | "drive" | "motion"
            | "saw" | "pulse" | "sub" | "noise" | "pwm" | "crossMod" | "index"
            | "sustain" | "feedback" | "cutoffHz" | "decayMs" | "decaySeconds"
            | "body" | "tone" | "noiseMix" | "snap" | "shimmerHz" | "shimmerDepth"
            | "scanRate" | "stereoWidth" => Some(info.max),
            "attackMs" | "attack" => Some(info.min),
            "damping" => Some(info.min),
            "releaseMs" => Some(40.0),
            "gain" | "level" => Some(if info.id.starts_with("operators.") {
                1.0
            } else {
                0.25
            }),
            "ring" | "sawMix" | "morph" | "filterMix" => Some(0.5),
            _ if info.id.starts_with("fm.") => Some(info.max),
            // A ring depth of one on every cyclic route silences the graph at
            // startup. Half depth exercises every product and retains carriers.
            _ if info.id.starts_with("ring.") => Some(0.5),
            _ if info.id.starts_with("output.") => Some(1.0),
            _ => None,
        };
        if let Some(value) = value {
            assert!(params.set(index, value), "{}: {}", kind.name(), info.id);
        }
    }
    if let InstrumentParams::HybridSeq(ref mut p) = params {
        p.sequencer = true;
        p.length = 16;
        p.division = 0.125;
        p.gate = 1.0;
        for (index, step) in p.steps.iter_mut().enumerate() {
            step.enabled = true;
            step.velocity = 1.0;
            step.pitch = (index % 5) as i8 - 2;
        }
    }
    if let InstrumentParams::LayerKick(ref mut p) = params {
        // Both branches sound; slowest legal layer playback keeps every PCM
        // voice busy even at the highest note of the test chord.
        p.sample_mix = 0.5;
        p.sample_tune = -24.0;
        p.sample_start = 0.0;
    }
    assert_eq!(params, params.sanitized());
    params
}

fn variants(kind: InstrumentKind) -> Vec<InstrumentParams> {
    let base = dense_params(kind);
    match base {
        InstrumentParams::GrooveSynth(p) => [
            GrooveMode::Analog,
            GrooveMode::Phase,
            GrooveMode::Ring,
            GrooveMode::Table,
        ]
        .into_iter()
        .map(|mode| InstrumentParams::GrooveSynth(windfall_dsp::GrooveSynthParams { mode, ..p }))
        .collect(),
        InstrumentParams::HybridSeq(p) => [
            HybridAlgorithm::Serial,
            HybridAlgorithm::Pairs,
            HybridAlgorithm::Stack,
            HybridAlgorithm::Parallel,
        ]
        .into_iter()
        .map(|algorithm| {
            InstrumentParams::HybridSeq(windfall_dsp::HybridSeqParams { algorithm, ..p })
        })
        .collect(),
        // Cover every pack, including all three multisample, table and FM banks;
        // an FM-only or default subtractive patch cannot stand in for pack cost.
        InstrumentParams::PackSynth(p) => [
            PackPatch::EmberBass,
            PackPatch::EmberGlass,
            PackPatch::EmberKeys,
            PackPatch::EmberBell,
            PackPatch::PrismLead,
            PackPatch::PrismSweep,
            PackPatch::PrismMallet,
            PackPatch::PrismChime,
            PackPatch::MeadowPad,
            PackPatch::MeadowAir,
            PackPatch::MeadowPluck,
            PackPatch::MeadowMetal,
        ]
        .into_iter()
        .map(|patch| InstrumentParams::PackSynth(windfall_dsp::PackSynthParams { patch, ..p }))
        .collect(),
        InstrumentParams::DrumVoice(p) => [
            DrumMode::Kick,
            DrumMode::Snare,
            DrumMode::Hat,
            DrumMode::Tom,
        ]
        .into_iter()
        .map(|mode| InstrumentParams::DrumVoice(windfall_dsp::DrumVoiceParams { mode, ..p }))
        .collect(),
        _ => vec![base],
    }
}

/// Original deterministic stress fixtures, prepared only on the control side.
/// They fill the accepted asset capacities, including all 256 scan columns.
fn maximum_asset(params: &InstrumentParams) -> Option<InstrumentAsset> {
    let kind = params.kind();
    match kind {
        InstrumentKind::LayerKick => {
            let samples: Vec<_> = (0..MAX_LAYER_FRAMES)
                .map(|index| (index as f32 * 0.071).sin() * 0.6)
                .collect();
            let pcm = LayerPcm::from_samples(MIN_LAYER_RATE, &samples);
            assert_eq!(pcm.samples.len(), MAX_LAYER_FRAMES);
            Some(InstrumentAsset::Pcm(pcm))
        }
        InstrumentKind::DrumVoice => {
            let InstrumentParams::DrumVoice(mut params) = *params else {
                unreachable!();
            };
            params.level = 1.0;
            Some(InstrumentAsset::DrumKeys {
                patches: (0..128)
                    .map(|key| DrumKeyPatch { key, params })
                    .collect(),
            })
        }
        InstrumentKind::NumberVoice => {
            let bank = SpectralBank::numbered(u32::MAX);
            assert_eq!(bank.frames[0].partials.len(), 64);
            Some(InstrumentAsset::Spectra(bank))
        }
        InstrumentKind::SnapshotMorph
        | InstrumentKind::HarmonicFilter
        | InstrumentKind::MetalVoice
        | InstrumentKind::SpectralResynth
        | InstrumentKind::ColorScan => {
            let frames = match kind {
                InstrumentKind::SnapshotMorph => 2,
                InstrumentKind::SpectralResynth | InstrumentKind::ColorScan => 256,
                _ => 1,
            };
            Some(InstrumentAsset::Spectra(SpectralBank {
                frames: (0..frames)
                    .map(|frame| SpectralFrame {
                        partials: (0..64)
                            .map(|partial| SpectralPartial {
                                ratio: if kind == InstrumentKind::MetalVoice {
                                    1.0 + partial as f32 * 0.271
                                } else {
                                    (partial + 1) as f32
                                },
                                level: 0.75 + (frame % 5) as f32 * 0.05,
                                phase: ((partial * 7 + frame) % 64) as f32 / 64.0,
                                pan: if partial % 2 == 0 { -0.5 } else { 0.5 },
                            })
                            .collect(),
                    })
                    .collect(),
                frame_seconds: 0.001,
                root_hz: 440.0,
                preset_number: None,
            }))
        }
        _ => None,
    }
}

fn start_maximum(unit: &mut AnyInstrument, voices: usize) {
    for key in 36..36 + voices as u8 {
        unit.note_on(key, 1.0);
    }
}

fn profile(params: InstrumentParams, voices: usize) {
    let label = format!("{params:?}");
    for (index, info) in params.kind().descriptors().iter().enumerate() {
        if info.id == "polyphony" {
            assert_eq!(
                info.max, voices as f32,
                "{label}: declared voice limit changed"
            );
            assert_eq!(params.get(index), Some(info.max));
        }
        if info.id == "unison" {
            assert_eq!(info.max, 7.0, "{label}: declared unison limit changed");
            assert_eq!(params.get(index), Some(info.max));
        }
    }
    let started = Instant::now();
    let mut prepared = None;
    let bytes = allocated_bytes(|| {
        let asset = maximum_asset(&params);
        if let Some(asset) = &asset {
            // new_with_asset refuses invalid data by falling back to defaults.
            // Reject it here so such a fallback cannot pass a maximum-load gate.
            assert_eq!(asset.validate(params.kind()), Ok(()));
        }
        let mut unit = AnyInstrument::new_with_asset(&params, asset.as_ref());
        unit.prepare(RATE as f32, BLOCK);
        prepared = Some(unit);
    });
    let preparation = started.elapsed();
    assert!(
        preparation < MAX_PREPARATION,
        "{label}: preparation {preparation:?}"
    );
    assert!(
        bytes <= MAX_PREPARATION_BYTES,
        "{label}: preparation allocated {bytes} bytes"
    );
    let mut unit = prepared.unwrap();
    let tail = unit.tail_samples();
    assert!(tail <= MAX_TAIL_FRAMES, "{label}: unbounded tail {tail}");

    let mut left = [0.0; BLOCK];
    let mut right = [0.0; BLOCK];
    let mut best = Duration::MAX;
    let mut reference = None;
    let mut slowest_block = Duration::ZERO;
    let mut overall_peak = 0.0_f32;
    // All three trials execute the same fixed two seconds at full load. Taking
    // the fastest whole trial tolerates an OS scheduling interruption without
    // allowing cheap idle/tail frames to dilute the maximum-load ratio.
    for _ in 0..3 {
        let mut stats = PcmStats::default();
        let mut full_load = true;
        let started = Instant::now();
        let calls = allocator_calls(|| {
            unit.reset();
            unit.set_params(&params);
            unit.set_tempo(120.0);
            start_maximum(&mut unit, voices);
            for block in 0..BLOCKS {
                // Re-trigger finite PCM/drum patches before they can expire,
                // and exercise bounded steals in continuously held synths.
                if block > 0 && block % 16 == 0 {
                    start_maximum(&mut unit, voices);
                }
                full_load &= unit.active_voices() == voices;
                let at = Instant::now();
                unit.process(&mut left, &mut right);
                slowest_block = slowest_block.max(at.elapsed());
                full_load &= unit.active_voices() == voices;
                stats.observe(&left, &right);
                if block % 32 == 0 {
                    unit.set_params(&params);
                    unit.set_tempo(120.0);
                }
            }
        });
        best = best.min(started.elapsed());
        assert_eq!(calls, 0, "{label}: process/events/reset touched the allocator");
        assert!(full_load, "{label}: did not hold the declared {voices} voices");
        stats.assert_bounded(&label);
        assert!(stats.energy > 1e-8, "{label}: maximum-load patch was silent");
        if let Some(hash) = reference {
            assert_eq!(stats.hash, hash, "{label}: reset changed the deterministic PCM");
        }
        reference = Some(stats.hash);
        overall_peak = overall_peak.max(stats.peak);
    }
    let audio_seconds = (BLOCKS * BLOCK) as f64 / RATE as f64;
    let ratio = best.as_secs_f64() / audio_seconds;
    eprintln!(
        "{}: voices={voices}, prepare={preparation:?}, allocated={bytes}, \
         two-second render={best:?}, ratio={ratio:.4}, slowest128={slowest_block:?}, \
         peak={overall_peak}, tail={tail} frames",
        params.kind().name(),
    );
    assert!(
        ratio < MAX_RATIO,
        "{label}: realtime ratio {ratio:.4} >= {MAX_RATIO}"
    );

    // Release, drain the complete declared serial effects tail, then demand
    // exact silence. This phase has its own allocator and ratio measurements.
    let mut stats = PcmStats::default();
    let mut drained = false;
    let release_blocks = RATE * 5 / BLOCK;
    let tail_blocks = tail.div_ceil(BLOCK) + 2;
    let started = Instant::now();
    let calls = allocator_calls(|| {
        for key in 0..128 {
            unit.note_off(key);
        }
        for _ in 0..release_blocks {
            unit.process(&mut left, &mut right);
            stats.observe(&left, &right);
        }
        drained = unit.active_voices() == 0;
        for _ in 0..tail_blocks {
            unit.process(&mut left, &mut right);
            stats.observe(&left, &right);
        }
    });
    let tail_elapsed = started.elapsed();
    assert_eq!(calls, 0, "{label}: release/tail touched the allocator");
    assert!(drained, "{label}: voices exceeded the five-second release allowance");
    stats.assert_bounded(&label);
    assert!(
        left.iter().chain(&right).all(|x| *x == 0.0),
        "{label}: tail did not end"
    );
    let tail_seconds = ((release_blocks + tail_blocks) * BLOCK) as f64 / RATE as f64;
    assert!(
        tail_elapsed.as_secs_f64() < tail_seconds * MAX_RATIO,
        "{label}: tail realtime ratio"
    );
    eprintln!(
        "{}: released/tail render={tail_elapsed:?}, audio={tail_seconds:.3}s",
        params.kind().name()
    );

    // Exercise a real control transition as well as the repeated publications
    // in the timing loop. Build the Copy settings outside the allocator guard.
    let mut changed = params;
    let output_index = params
        .kind()
        .descriptors()
        .iter()
        .position(|info| matches!(info.id, "gain" | "level"))
        .expect("every parity instrument exposes an output level");
    assert!(changed.set(output_index, 0.2));
    assert_ne!(changed, params);
    let mut event_stats = PcmStats::default();
    let calls = allocator_calls(|| {
        unit.reset();
        unit.set_params(&changed);
        start_maximum(&mut unit, voices);
        unit.process(&mut left, &mut right);
        event_stats.observe(&left, &right);
        unit.set_params(&params);
        unit.process(&mut left, &mut right);
        event_stats.observe(&left, &right);
        unit.all_notes_off();
        unit.process(&mut left, &mut right);
        event_stats.observe(&left, &right);
        unit.reset();
        unit.process(&mut left, &mut right);
    });
    assert_eq!(calls, 0, "{label}: control/stop/reset touched the allocator");
    event_stats.assert_bounded(&label);
    assert_eq!(unit.active_voices(), 0);
    assert!(left.iter().chain(&right).all(|x| *x == 0.0));
}

fn maximum(kind: InstrumentKind, voices: usize) {
    let _serial = PROFILES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for params in variants(kind) {
        profile(params, voices);
    }
}

macro_rules! maximum_test {
    ($name:ident, $kind:ident, $voices:expr) => {
        #[test]
        fn $name() {
            maximum(InstrumentKind::$kind, $voices);
        }
    };
}

maximum_test!(kepler_maximum_load_deadline, VintagePoly, 16);
maximum_test!(kepler_exo_maximum_load_deadline, DualPoly, 16);
maximum_test!(poizone_maximum_load_deadline, CompactSynth, 16);
maximum_test!(sawer_maximum_load_deadline, VintageSaw, 16);
maximum_test!(minisynth_maximum_load_deadline, PocketSynth, 16);
maximum_test!(gms_maximum_load_deadline, GrooveSynth, 16);
maximum_test!(toxic_biohazard_maximum_load_deadline, HybridSeq, 16);
maximum_test!(sytrus_maximum_load_deadline, MatrixSynth, 16);
maximum_test!(flex_maximum_load_deadline, PackSynth, 16);
maximum_test!(drumaxx_maximum_load_deadline, DrumRack, 16);
maximum_test!(harmor_maximum_load_deadline, SpectralResynth, 16);
maximum_test!(morphine_maximum_load_deadline, SnapshotMorph, 16);
maximum_test!(ogun_maximum_load_deadline, MetalVoice, 16);
maximum_test!(harmless_maximum_load_deadline, HarmonicFilter, 16);
maximum_test!(autogun_maximum_load_deadline, NumberVoice, 8);
maximum_test!(bassdrum_maximum_load_deadline, LayerKick, 8);
maximum_test!(beepmap_maximum_load_deadline, ColorScan, 16);
maximum_test!(boobass_maximum_load_deadline, FingerBass, 1);
maximum_test!(drumpad_maximum_load_deadline, Membrane, 1);
maximum_test!(drumsynth_live_maximum_load_deadline, DrumVoice, 8);
