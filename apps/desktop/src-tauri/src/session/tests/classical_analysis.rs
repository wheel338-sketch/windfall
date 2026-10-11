//! Written, not executed. Real authored PCM through public Session entrypoints.
use super::Rig;
use crate::session::ClipPlace;
use windfall_core::AudioBuffer;
use windfall_ipc::AudioAnalysisEdit;
use windfall_project::{AudioDenoiseSettings, AudioNoteOverride, ClipId, SampleId};

fn fixture(rig: &Rig) -> (ClipId, String, Vec<u8>) {
    let path = rig.file("calibrated-note.wav");
    let audio = AudioBuffer::from_interleaved(
        16000,
        1,
        (0..16000)
            .map(|i| (0.2 * (std::f64::consts::TAU * 440.0 * i as f64 / 16000.0).sin()) as f32)
            .collect(),
    );
    windfall_codec::write_wav(&path, &audio, windfall_codec::WavSampleFormat::Float32).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let result = rig
        .session
        .add_audio_clip_from_file(
            &path,
            ClipPlace {
                track: None,
                start: 100,
                mixer_track: None,
            },
        )
        .unwrap();
    (ClipId(*result.created.last().unwrap()), path, bytes)
}
fn reviewed(rig: &Rig, id: ClipId) -> AudioAnalysisEdit {
    let preview = rig.session.audio_editor_open(id).unwrap();
    let review = rig
        .session
        .audio_analysis_detect(preview.token, false)
        .unwrap();
    AudioAnalysisEdit {
        token: review.token,
        recipe: review.recipe,
    }
}
#[test]
fn classical_pitch_edit_one_step_undo_save_reopen_keeps_estimates_and_original() {
    let rig = Rig::new();
    let (clip, path, bytes) = fixture(&rig);
    let before = rig.session.document_snapshot();
    let mut edit = reviewed(&rig, clip);
    assert_eq!(edit.recipe.analysis.notes.len(), 1);
    let detected = edit.recipe.analysis.clone();
    let note = &detected.notes[0];
    edit.recipe.overrides.push(AudioNoteOverride {
        note: 0,
        midi_pitch: note.midi_pitch + 12.0,
        start_frame: note.start_frame,
        end_frame: note.end_frame,
    });
    rig.session
        .audio_analysis_audition(edit.clone(), false)
        .unwrap();
    assert_eq!(rig.session.document_snapshot(), before);
    let result = rig.session.audio_analysis_apply(edit.clone()).unwrap();
    let sample = SampleId(result.created[0]);
    let next_clip = ClipId(result.created[1]);
    assert_eq!(
        rig.session.document_snapshot().history.cursor,
        before.history.cursor + 1
    );
    assert_eq!(
        rig.project()
            .sample(sample)
            .unwrap()
            .processing
            .as_ref()
            .unwrap()
            .analysis,
        detected
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let rendered = rig.session.state().pool.get(sample).unwrap().clone();
    let mut work = windfall_analysis::Work::new(
        Default::default(),
        100_000_000_000,
        std::time::Duration::from_secs(120),
    )
    .unwrap();
    let output_notes = windfall_analysis::classical::detect(&rendered, false, &mut work).unwrap();
    assert!((output_notes.notes[0].midi_pitch - 81.0).abs() < 0.15);
    rig.session.undo().unwrap();
    assert!(rig.project().playlist.clips.iter().any(|c| c.id == clip));
    rig.session.redo().unwrap();
    let song = rig.file("edited.windfall");
    rig.session.project_save(Some(&song)).unwrap();
    let rig = rig.restart();
    rig.session.project_open(&song).unwrap();
    let reopened = reviewed(&rig, next_clip);
    assert_eq!(reopened.recipe, edit.recipe);
    assert_eq!(
        rig.session.state().pool.get(sample).unwrap().samples(),
        rendered.samples()
    );
    // Saved estimates retain the detection-time view even after song tempo
    // changes the playlist window's current duration.
    rig.session
        .dispatch(
            windfall_project::Command::UpdateSettings {
                patch: windfall_project::SettingsPatch {
                    tempo_bpm: Some(90.0),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    let reopened = reviewed(&rig, next_clip);
    assert_eq!(reopened.recipe, edit.recipe);
    rig.session
        .audio_analysis_audition(reopened, false)
        .unwrap();
}

#[test]
fn audio_score_under_tempo_ramp_uses_absolute_onsets_and_offsets() {
    use windfall_project::*;
    let rig = Rig::new();
    let (clip, _, _) = fixture(&rig);
    let result = rig
        .session
        .dispatch(
            Command::AddAutomation {
                name: None,
                target: AutomationTarget::Tempo,
                points: Some(vec![
                    AutomationPoint {
                        tick: 0,
                        value: (60.0 - 10.0) / 512.0,
                        curve: 0.0,
                        hold: false,
                    },
                    AutomationPoint {
                        tick: 1920,
                        value: (120.0 - 10.0) / 512.0,
                        curve: 0.0,
                        hold: false,
                    },
                ]),
            },
            None,
        )
        .unwrap();
    let track = rig
        .project()
        .playlist
        .clips
        .iter()
        .find(|c| c.id == clip)
        .unwrap()
        .track;
    rig.session
        .prepare_clip_command(Command::AddClips {
            clips: vec![ClipInit {
                track,
                start: 0,
                length: Some(1920),
                offset: Some(0),
                muted: Some(false),
                content: ClipContent::Automation {
                    automation: AutomationId(result.created[0]),
                },
            }],
        })
        .unwrap();
    let edit = reviewed(&rig, clip);
    let n = &edit.recipe.analysis.notes[0];
    let start_frame = n.start_frame;
    let end_frame = n.end_frame;
    rig.session.audio_analysis_send_midi(edit, vec![0]).unwrap();
    let project = rig.project();
    let note = &project.patterns.last().unwrap().lanes[0].notes[0];
    let base_time = 2.0 * (1.0 + 100.0 / 1920.0_f64).ln();
    // Invert the independent analytic ramp integral t=2*ln(1+tick/1920).
    let tick = |frame: u32| {
        let t = base_time + f64::from(frame) / 16000.0;
        (1920.0 * ((t / 2.0).exp() - 1.0)).round() as u32 - 100
    };
    assert_eq!(note.start, tick(start_frame));
    assert_eq!(note.start + note.length, tick(end_frame));
    assert_eq!(note.key, 69);
    assert!(note.velocity > 0.4);
}
#[test]
fn audio_score_review_inserts_calibrated_notes_as_one_persisted_undo_step() {
    let rig = Rig::new();
    let (clip, _, _) = fixture(&rig);
    let edit = reviewed(&rig, clip);
    let before = rig.session.document_snapshot();
    rig.session.audio_analysis_send_midi(edit, vec![0]).unwrap();
    let after = rig.project();
    let pattern = after.patterns.last().unwrap();
    let note = &pattern.lanes[0].notes[0];
    assert_eq!(note.key, 69);
    assert_eq!(note.start, 0);
    assert_eq!(note.length, 1920);
    assert!((0.4..0.8).contains(&note.velocity));
    assert_eq!(
        rig.session.document_snapshot().history.cursor,
        before.history.cursor + 1
    );
    rig.session.undo().unwrap();
    assert_eq!(rig.project().patterns.len(), before.project.patterns.len());
    rig.session.redo().unwrap();
    let path = rig.file("score.windfall");
    rig.session.project_save(Some(&path)).unwrap();
    let rig = rig.restart();
    rig.session.project_open(&path).unwrap();
    assert_eq!(rig.project().patterns.last().unwrap(), pattern);
}
#[test]
fn classical_cancel_at_prepared_publication_preserves_project_and_review() {
    let rig = Rig::new();
    let (clip, _, _) = fixture(&rig);
    let mut edit = reviewed(&rig, clip);
    edit.recipe.markers.last_mut().unwrap().target_frame = 24000;
    let before = rig.session.document_snapshot();
    let hold = rig.session.hold("audio-editor:prepared");
    let worker = rig
        .session
        .background(move |s| s.audio_analysis_apply(edit));
    hold.wait();
    rig.session.audio_analysis_cancel();
    hold.release();
    assert!(worker.join().unwrap().unwrap_err().contains("cancelled"));
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(reviewed(&rig, clip).recipe.overrides.is_empty());
}
#[test]
fn immutable_estimates_bad_warp_and_stale_source_cannot_apply() {
    let rig = Rig::new();
    let (clip, _, _) = fixture(&rig);
    let mut edit = reviewed(&rig, clip);
    let before = rig.session.document_snapshot();
    edit.recipe.analysis.notes[0].midi_pitch = 60.0;
    assert!(rig
        .session
        .audio_analysis_apply(edit)
        .unwrap_err()
        .contains("cannot be rewritten"));
    assert_eq!(rig.session.document_snapshot(), before);
    let mut edit = reviewed(&rig, clip);
    edit.recipe.markers.last_mut().unwrap().target_frame = 1;
    assert!(rig.session.audio_analysis_apply(edit).is_err());
    assert_eq!(rig.session.document_snapshot(), before);
    let edit = reviewed(&rig, clip);
    rig.session
        .dispatch(
            windfall_project::Command::UpdateSettings {
                patch: windfall_project::SettingsPatch {
                    tempo_bpm: Some(130.0),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    assert!(rig.session.audio_analysis_send_midi(edit, vec![0]).is_err());
}
#[test]
fn noise_profile_dry_wet_and_apply_preserve_source_and_saved_profile() {
    let rig = Rig::new();
    let (clip, path, bytes) = fixture(&rig);
    let mut edit = reviewed(&rig, clip);
    edit.recipe.denoise = Some(AudioDenoiseSettings {
        profile_start: 0,
        profile_end: 2000,
        reduction_db: 12.0,
        wet: 0.5,
    });
    // This tonal profile deliberately exercises the transport contract, not
    // signal quality; independent noise-only SNR truth lives in classical_tools.
    let before = rig.session.document_snapshot();
    rig.session
        .audio_analysis_audition(edit.clone(), true)
        .unwrap();
    rig.session
        .audio_analysis_audition(edit.clone(), false)
        .unwrap();
    assert_eq!(rig.session.document_snapshot(), before);
    let result = rig.session.audio_analysis_apply(edit.clone()).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert_eq!(
        rig.project()
            .sample(SampleId(result.created[0]))
            .unwrap()
            .processing
            .as_ref()
            .unwrap()
            .denoise,
        edit.recipe.denoise
    );
}
