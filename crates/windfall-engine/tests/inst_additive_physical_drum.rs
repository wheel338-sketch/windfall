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
