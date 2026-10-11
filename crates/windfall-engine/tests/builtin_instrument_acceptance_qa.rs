//! Public document-to-render acceptance for six complete synthesis workflows.
//! Numerical musical behavior is independently covered by each DSP family.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use windfall_dsp::analog::{AcidLineParams, TripleOscParams};
use windfall_dsp::*;
use windfall_engine::{Processor, RenderOptions, SamplePool, render};
use windfall_project::*;

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

#[path = "builtin_instrument_acceptance_qa/polysynth.rs"]
mod polysynth_acceptance;

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
    doc.dispatch(
        Command::AddNotes {
            pattern,
            channel,
            notes: vec![NoteInit {
                start: 240,
                length: 1920,
                key: 60,
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
    doc.dispatch(
        Command::SetInstrumentParams {
            channel,
            params: changed,
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
fn four_op_fm_modulation_reaches_project_history_disk_and_renderer() {
    let initial = FourOpParams::default();
    let mut changed = initial;
    changed.operators[1].level = 1.0;
    changed.operators[1].ratio = 2.0;
    acceptance(
        InstrumentKind::FourOp,
        InstrumentParams::FourOp(initial),
        InstrumentParams::FourOp(changed),
    );
}

#[test]
fn pluck_string_brightness_reaches_project_history_disk_and_renderer() {
    let initial = PluckParams {
        brightness: 0.15,
        ..Default::default()
    };
    let changed = PluckParams {
        brightness: 0.9,
        ..initial
    };
    acceptance(
        InstrumentKind::Pluck,
        InstrumentParams::Pluck(initial),
        InstrumentParams::Pluck(changed),
    );
}

#[test]
fn acoustic_string_dispersion_reaches_project_history_disk_and_renderer() {
    let initial = AcousticStringParams {
        stiffness: 0.0,
        ..Default::default()
    };
    let changed = AcousticStringParams {
        stiffness: 0.8,
        ..initial
    };
    acceptance(
        InstrumentKind::AcousticString,
        InstrumentParams::AcousticString(initial),
        InstrumentParams::AcousticString(changed),
    );
}

#[test]
fn kick_pitch_sweep_reaches_project_history_disk_and_renderer() {
    let initial = KickParams {
        pitch_drop_semitones: 0.0,
        ..Default::default()
    };
    let changed = KickParams {
        pitch_drop_semitones: 36.0,
        ..initial
    };
    acceptance(
        InstrumentKind::Kick,
        InstrumentParams::Kick(initial),
        InstrumentParams::Kick(changed),
    );
}

#[test]
fn triple_oscillator_pitch_reaches_project_history_disk_and_renderer() {
    let initial = TripleOscParams::default();
    let mut changed = initial;
    changed.oscillators[0].semitones = 12.0;
    acceptance(
        InstrumentKind::TripleOsc,
        InstrumentParams::TripleOsc(initial),
        InstrumentParams::TripleOsc(changed),
    );
}

#[test]
fn acid_internal_sequence_pitch_reaches_project_history_disk_and_renderer() {
    let initial = AcidLineParams {
        sequencer: true,
        ..Default::default()
    };
    let mut changed = initial;
    changed.steps[1].pitch_offset = 12;
    changed.steps[1].accent = true;
    changed.steps[1].slide = true;
    acceptance(
        InstrumentKind::AcidLine,
        InstrumentParams::AcidLine(initial),
        InstrumentParams::AcidLine(changed),
    );
}
