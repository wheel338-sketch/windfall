//! Deterministic reference DSP through actual Session workers/document/pool.
use super::Rig;
use crate::session::{
    ClipPlace,
    analysis_jobs::{manifest, model_wire},
};
use native::separation::{PreparedSeparation, ReferenceBackend, SeparationBackend};
use std::sync::Arc;
use windfall_analysis as native;
use windfall_core::AudioBuffer;
use windfall_ipc::{AnalysisApply, AnalysisRuntimeSettings, AnalysisSubmit, AnalysisTask};
use windfall_project::{ClipId, Command};
mod real_ort;
const WEIGHTS: &[u8] = b"authored reference-backend fixture; not an ONNX model";
fn work() -> native::Work {
    native::Work::new(
        native::CancelToken::default(),
        10_000_000_000,
        std::time::Duration::from_secs(600),
    )
    .unwrap()
}
fn model() -> native::ModelManifest {
    let sha = native::ContentFingerprint::reader(std::io::Cursor::new(WEIGHTS), 1024, &mut work())
        .unwrap()
        .sha256;
    use native::TensorDimension::{Fixed, Symbol};
    native::ModelManifest {
        id: "reference-stems-test-only".into(),
        version: "1".into(),
        revision: 1,
        sha256: sha,
        bytes: WEIGHTS.len() as u64,
        max_bytes: 1024,
        sample_rate: 48000,
        input_channels: 2,
        max_input_frames: 48_000 * 3600,
        provenance: native::ModelProvenance {
            origin: "authored:test".into(),
            source_revision: "1".into(),
            author: "Windfall".into(),
            license_spdx: "CC0-1.0".into(),
            license_reference: "authored reference fixture".into(),
            adapter_id: native::separation::ADAPTER_ID.into(),
            adapter_version: "1".into(),
            device: "cpu".into(),
        },
        outputs: ["drums", "bass", "vocals", "other"]
            .map(|role| native::OutputRole {
                role: role.into(),
                channels: 2,
            })
            .to_vec(),
        onnx: Some(native::OnnxContract {
            opset: 17,
            inputs: vec![native::TensorSpec {
                name: "mix".into(),
                shape: vec![Fixed(1), Fixed(2), Symbol("frames".into())],
            }],
            outputs: vec![native::TensorSpec {
                name: "stems".into(),
                shape: vec![Fixed(1), Fixed(4), Fixed(2), Symbol("frames".into())],
            }],
            sample_rate: 48000,
            segment_frames: 4096,
            overlap_frames: 1024,
            context_frames: 16,
            layout: Default::default(),
            stem_roles: ["drums", "bass", "vocals", "other"]
                .map(String::from)
                .to_vec(),
        }),
    }
}
fn fixture(rig: &Rig, backend: Arc<dyn SeparationBackend>) -> (ClipId, Vec<f32>) {
    // A regular file satisfies native path preflight; no ORT call is made by
    // the reference backend, and this fixture never claims model quality.
    let library = rig.file("test-only-runtime-placeholder.dll");
    std::fs::write(&library, b"test only").unwrap();
    rig.session
        .analysis_runtime_configure(AnalysisRuntimeSettings {
            library_path: Some(library),
            ..Default::default()
        })
        .unwrap();
    rig.session.inner.analysis.install_test_adapter(Arc::new(
        native::separation::ProductionAdapter {
            separation: native::separation::SeparationAdapter { backend },
        },
    ));
    let path = rig.file("reference.model");
    std::fs::write(&path, WEIGHTS).unwrap();
    rig.session
        .analysis_model_import(&path, model_wire(&model()))
        .unwrap();
    let samples: Vec<_> = (0..5003)
        .flat_map(|i| {
            let x = (i as f32 * 220.0 * std::f32::consts::TAU / 44100.0).sin() * 0.4;
            [x, -x * 0.7]
        })
        .collect();
    let path = rig.file("mix.wav");
    windfall_codec::write_wav(
        &path,
        &AudioBuffer::from_interleaved(44100, 2, samples.clone()),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
    let result = rig
        .session
        .add_audio_clip_from_file(
            &path,
            ClipPlace {
                track: None,
                start: 97,
                mixer_track: None,
            },
        )
        .unwrap();
    (ClipId(*result.created.last().unwrap()), samples)
}
fn request(clip: ClipId) -> AnalysisSubmit {
    AnalysisSubmit {
        amount: None,
        task: AnalysisTask::StemSeparation,
        clip,
        model_id: model().id,
        model_version: "1".into(),
        model_revision: "1".into(),
        start_frame: "0".into(),
        end_frame: "5003".into(),
    }
}
fn ready(rig: &Rig, clip: ClipId) -> windfall_ipc::AnalysisJob {
    let job = rig.session.analysis_submit(request(clip)).unwrap();
    let snapshot = rig
        .session
        .inner
        .analysis
        .test_manager()
        .wait(native::JobId(job.job.parse().unwrap()))
        .unwrap();
    assert_eq!(snapshot.status, native::JobStatus::Ready, "{snapshot:?}");
    rig.session.analysis_status(&job.job).unwrap()
}
fn apply(job: &windfall_ipc::AnalysisJob) -> AnalysisApply {
    AnalysisApply {
        ticket: job.ticket.clone(),
        request: job.request.clone(),
        replace_original: false,
        selected_roles: None,
        separate_tracks: true,
        mute_original: true,
    }
}
#[test]
fn reference_separation_real_job_resamples_publishes_four_aligned_tracks_in_one_saved_undo_step() {
    let rig = Rig::new();
    let (clip, samples) = fixture(&rig, Arc::new(ReferenceBackend));
    let before = rig.session.document_snapshot();
    let job = ready(&rig, clip);
    let review = rig.session.analysis_review(&job.ticket).unwrap();
    assert_eq!(review.artifacts.len(), 4);
    assert!(
        review
            .artifacts
            .iter()
            .all(|a| a.sample_rate == 44100 && a.frames == "5003" && a.frame_origin == "0")
    );
    rig.session.analysis_apply(apply(&job)).unwrap();
    let edited = rig.project();
    assert_eq!(
        rig.session.document_snapshot().history.cursor,
        before.history.cursor + 1
    );
    assert_eq!(
        edited.playlist.tracks.len(),
        before.project.playlist.tracks.len() + 4
    );
    assert_eq!(
        edited.mixer.tracks.len(),
        before.project.mixer.tracks.len() + 4
    );
    assert!(
        edited
            .playlist
            .clips
            .iter()
            .find(|c| c.id == clip)
            .unwrap()
            .muted
    );
    let new_clips: Vec<_> = edited
        .playlist
        .clips
        .iter()
        .filter(|c| {
            !before
                .project
                .playlist
                .clips
                .iter()
                .any(|old| old.id == c.id)
        })
        .collect();
    assert_eq!(new_clips.len(), 4);
    assert!(new_clips.iter().all(|c| c.start == 97 && c.offset == 0));
    let sources: Vec<_> = new_clips
        .iter()
        .map(|c| {
            rig.session
                .state()
                .pool
                .get(c.content.sample().unwrap())
                .unwrap()
                .clone()
        })
        .collect();
    let sum: Vec<f32> = (0..samples.len())
        .map(|i| sources.iter().map(|s| s.samples()[i]).sum())
        .collect();
    let (power, error) = samples[128..samples.len() - 128]
        .iter()
        .zip(&sum[128..sum.len() - 128])
        .fold((0.0f64, 0.0f64), |(p, e), (&x, &y)| {
            (p + f64::from(x).powi(2), e + f64::from(x - y).powi(2))
        });
    assert!(10.0 * (power / error.max(1e-30)).log10() > 35.0);
    rig.session.undo().unwrap();
    let mut restored = before.project;
    restored.next_id = rig.project().next_id;
    assert_eq!(rig.project(), restored);
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), edited);
    let saved = rig.file("separated.windfall");
    rig.session.project_save(Some(&saved)).unwrap();
    let rig = rig.restart();
    rig.session.project_open(&saved).unwrap();
    assert_eq!(rig.project(), edited);
    for (clip, expected) in new_clips.iter().zip(sources) {
        assert_eq!(
            rig.session
                .state()
                .pool
                .get(clip.content.sample().unwrap())
                .unwrap()
                .samples(),
            expected.samples()
        );
    }
}
#[test]
fn separated_apply_cancellation_and_stale_preparation_preserve_document_history_and_pool() {
    for cancel in [true, false] {
        let rig = Rig::new();
        let (clip, _) = fixture(&rig, Arc::new(ReferenceBackend));
        let job = ready(&rig, clip);
        let hold = rig.session.hold("analysis:prepared");
        let session = rig.session.clone();
        let request = apply(&job);
        let worker = std::thread::spawn(move || session.analysis_apply(request));
        hold.wait();
        if cancel {
            rig.session.analysis_cancel_preparation();
        } else {
            rig.session
                .dispatch(
                    Command::UpdateSettings {
                        patch: windfall_project::SettingsPatch {
                            name: Some("intervening edit".into()),
                            ..Default::default()
                        },
                    },
                    None,
                )
                .unwrap();
        }
        let before = rig.session.document_snapshot();
        let pool = rig.session.state().pool.clone();
        hold.release();
        let error = worker.join().unwrap().unwrap_err();
        assert!(
            error.contains(if cancel {
                "analysis:cancelled"
            } else {
                "analysis:stale"
            }),
            "{error}"
        );
        assert_eq!(rig.session.document_snapshot(), before);
        assert!(rig.session.state().pool.same_sources(&pool));
        assert_eq!(
            rig.session.analysis_status(&job.job).unwrap().status,
            windfall_ipc::AnalysisStatus::Ready
        );
        rig.session.analysis_cancel(&job.job).unwrap();
    }
}
#[test]
fn running_separation_cancellation_retires_spool_without_project_history_or_pool_change() {
    struct HeldBackend {
        entered: Arc<std::sync::Barrier>,
        release: Arc<std::sync::Barrier>,
    }
    struct HeldPrepared {
        inner: Box<dyn PreparedSeparation>,
        entered: Arc<std::sync::Barrier>,
        release: Arc<std::sync::Barrier>,
    }
    impl SeparationBackend for HeldBackend {
        fn prepare(
            &self,
            model: &native::ModelManifest,
            bytes: &[u8],
            work: &mut native::Work,
        ) -> native::Result<Box<dyn PreparedSeparation>> {
            Ok(Box::new(HeldPrepared {
                inner: ReferenceBackend.prepare(model, bytes, work)?,
                entered: self.entered.clone(),
                release: self.release.clone(),
            }))
        }
    }
    impl PreparedSeparation for HeldPrepared {
        fn run_chunk(
            &mut self,
            input: &[f32],
            work: &mut native::Work,
        ) -> native::Result<Vec<Vec<f32>>> {
            self.entered.wait();
            self.release.wait();
            self.inner.run_chunk(input, work)
        }
    }
    let rig = Rig::new();
    let entered = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let (clip, _) = fixture(
        &rig,
        Arc::new(HeldBackend {
            entered: entered.clone(),
            release: release.clone(),
        }),
    );
    let before = rig.session.document_snapshot();
    let pool = rig.session.state().pool.clone();
    let job = rig.session.analysis_submit(request(clip)).unwrap();
    entered.wait();
    rig.session.analysis_cancel(&job.job).unwrap();
    release.wait();
    let result = rig
        .session
        .inner
        .analysis
        .test_manager()
        .wait(native::JobId(job.job.parse().unwrap()))
        .unwrap();
    assert_eq!(result.status, native::JobStatus::Cancelled);
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(rig.session.state().pool.same_sources(&pool));
    assert_eq!(
        rig.session
            .inner
            .analysis
            .test_manager()
            .usage()
            .published_files,
        0
    );
    let stage = rig.session.inner.analysis.test_stage();
    assert_eq!(
        std::fs::read_dir(stage.join("Captures")).unwrap().count(),
        0
    );
}
#[test]
fn separation_settings_persist_and_missing_runtime_or_pack_never_mutates_project() {
    let rig = Rig::new();
    let (clip, _) = fixture(&rig, Arc::new(ReferenceBackend));
    let mut settings = rig.session.analysis_runtime_settings().unwrap();
    settings.model_id = Some(model().id);
    settings.model_version = Some("1".into());
    rig.session
        .analysis_runtime_configure(settings.clone())
        .unwrap();
    let before = rig.session.document_snapshot();
    let pool = rig.session.state().pool.clone();
    let mut absent = request(clip);
    absent.model_id = "not-imported".into();
    assert!(
        rig.session
            .analysis_submit(absent)
            .unwrap_err()
            .contains("modelAbsent")
    );
    let path = settings.library_path.clone().unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(
        rig.session
            .analysis_submit(request(clip))
            .unwrap_err()
            .contains("unavailable")
    );
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(rig.session.state().pool.same_sources(&pool));
    let rig = rig.restart();
    let restored = rig.session.analysis_runtime_settings().unwrap();
    assert_eq!(restored.model_id, settings.model_id);
    assert_eq!(restored.library_path, settings.library_path);
    assert!(!rig.session.analysis_capability().stem_separation);
}
#[test]
fn strict_song_decode_matches_whole_decode_for_import_formats_and_stops_on_callback_error() {
    use windfall_codec::{
        Encoder, EncoderSettings, FlacBitDepth, Mp3Channels, Mp3Settings, WavSampleFormat,
    };
    let rig = Rig::new();
    let samples: Vec<_> = (0..5003)
        .map(|i| (i as f32 * 220.0 * std::f32::consts::TAU / 48000.0).sin() * 0.3)
        .collect();
    for settings in [
        EncoderSettings::Wav {
            format: WavSampleFormat::Float32,
        },
        EncoderSettings::Flac {
            depth: FlacBitDepth::Int24,
            level: 5,
        },
        EncoderSettings::Vorbis { quality: 6.0 },
        EncoderSettings::Mp3 {
            settings: Mp3Settings {
                channels: Mp3Channels::Mono,
                ..Default::default()
            },
        },
    ] {
        let path = rig.file(&format!("stream.{}", settings.format().extension()));
        let mut encoder = Encoder::open(&path, &settings, 48000, 1).unwrap();
        encoder.write(&samples).unwrap();
        encoder.finalize().unwrap();
        let reference =
            windfall_codec::decode_file_strict_with(&path, &Default::default()).unwrap();
        let mut streamed = Vec::new();
        let mut max_chunk = 0;
        let info = windfall_codec::decode_file_stream_strict(
            &path,
            &Default::default(),
            &mut |rate, channels, block| {
                assert_eq!(
                    (rate, channels),
                    (reference.sample_rate(), reference.channels())
                );
                max_chunk = max_chunk.max(block.len());
                streamed.extend_from_slice(block);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(info.frames, Some(reference.frames() as u64));
        assert_eq!(streamed, reference.samples(), "{settings:?}");
        assert!(max_chunk <= 4096);
        let mut calls = 0;
        let error = windfall_codec::decode_file_stream_strict(
            &path,
            &Default::default(),
            &mut |_, _, _| {
                calls += 1;
                Err(windfall_codec::CodecError::Corrupt(
                    "operator cancelled".into(),
                ))
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("operator cancelled"));
        assert_eq!(calls, 1);
    }
}
