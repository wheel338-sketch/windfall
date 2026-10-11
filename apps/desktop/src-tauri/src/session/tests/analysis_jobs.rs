//! Actual Session/Document/Controller and native WAV analysis lifecycle tests.
//! The private authored copying adapter proves infrastructure, never inference.
use super::Rig;
use crate::session::ClipPlace;
use std::sync::Arc;
use windfall_analysis as native;
use windfall_core::AudioBuffer;
use windfall_ipc::{AnalysisApply, AnalysisJob, AnalysisStatus};
use windfall_ipc::{AnalysisModel, AnalysisOutputRole, AnalysisProvenance, AnalysisSubmit};
use windfall_project::ClipId;
use windfall_project::SampleId;
#[cfg(windows)]
mod dos_device;

struct CopyAdapter;
impl native::AnalysisAdapter for CopyAdapter {
    fn supports(&self, _task: native::InferenceTask) -> bool { true }
    fn id(&self) -> &str {
        "test-only-copy"
    }
    fn version(&self) -> &str {
        "1"
    }
    fn run(
        &self,
        input: &native::CapturedInput,
        model: &native::VerifiedModel,
        context: &mut native::WorkerContext,
        artifacts: &mut native::ArtifactWriter,
    ) -> native::Result<()> {
        assert_eq!(model.bytes(), MODEL);
        for (index, role) in model.manifest().outputs.iter().enumerate() {
            artifacts.start_audio(
                native::RelativeName::new(format!("copy-{index}.wav"))?,
                &role.role,
                context.work(),
            )?;
            for chunk in input.selected_samples().chunks(4096) {
                artifacts.write_audio(chunk, context.work())?;
            }
            artifacts.finish_audio(context.work())?;
        }
        Ok(())
    }
}
fn fixture(rig: &Rig) -> (ClipId, String, Vec<u8>) {
    rig.session
        .inner
        .analysis
        .install_test_adapter(Arc::new(CopyAdapter));
    let path = rig.file("authored.model");
    std::fs::write(&path, MODEL).unwrap();
    rig.session.analysis_model_import(&path, model()).unwrap();
    let path = rig.file("original.wav");
    let audio =
        AudioBuffer::from_interleaved(48_000, 2, (0..100).flat_map(|_| [0.25, -0.25]).collect());
    windfall_codec::write_wav(&path, &audio, windfall_codec::WavSampleFormat::Float32).unwrap();
    let original = std::fs::read(&path).unwrap();
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
    (ClipId(*result.created.last().unwrap()), path, original)
}
fn ready(rig: &Rig, clip: ClipId) -> AnalysisJob {
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
fn apply(job: &AnalysisJob, replace_original: bool) -> AnalysisApply {
    AnalysisApply {
        ticket: job.ticket.clone(),
        request: job.request.clone(),
        replace_original,
        selected_roles: None,
        separate_tracks: false,
        mute_original: false,
    }
}

#[test]
fn analysis_actual_batch_undo_redo_save_reopen_preserves_original_and_committed_outputs() {
    let rig = Rig::new();
    let (clip, path, original) = fixture(&rig);
    let before = rig.session.document_snapshot();
    let job = ready(&rig, clip);
    let review = rig.session.analysis_review(&job.ticket).unwrap();
    assert_eq!(review.artifacts.len(), 1);
    assert_eq!(review.artifacts[0].frames, "100");
    assert_eq!(review.artifacts[0].frame_origin, "0");
    assert_eq!(rig.session.document_snapshot(), before);
    let admitted = rig.session.inner.analysis.test_manager().usage();
    let result = rig.session.analysis_apply(apply(&job, true)).unwrap();
    assert_eq!(rig.session.inner.analysis.final_calls(), (0, 0));
    eprintln!(
        "analysis Session one output: ready quota={admitted:?}; consumed quota={:?}; final allocator calls={:?}",
        rig.session.inner.analysis.test_manager().usage(),
        rig.session.inner.analysis.final_calls()
    );
    assert_eq!(result.created.len(), 2);
    assert_eq!(
        rig.session.document_snapshot().history.cursor,
        before.history.cursor + 1
    );
    let sample = SampleId(result.created[0]);
    assert_eq!(
        rig.session.state().pool.get(sample).unwrap().samples(),
        &[0.25, -0.25].repeat(100)
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let edited = rig.project();
    assert_eq!(
        rig.session.analysis_status(&job.job).unwrap().status,
        AnalysisStatus::Consumed
    );
    assert!(rig.session.analysis_apply(apply(&job, true)).is_err());
    assert_eq!(
        rig.session.analysis_cancel(&job.job).unwrap().status,
        AnalysisStatus::Consumed
    );
    rig.session.analysis_forget(&job.job).unwrap();
    rig.session.undo().unwrap();
    let mut restored = before.project;
    restored.next_id = rig.project().next_id;
    assert_eq!(rig.project(), restored);
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), edited);
    let saved = rig.file("analysis.windfall");
    rig.session.project_save(Some(&saved)).unwrap();
    let rig = rig.restart();
    rig.session.project_open(&saved).unwrap();
    assert_eq!(rig.project(), edited);
    assert_eq!(
        rig.session.state().pool.get(sample).unwrap().samples(),
        &[0.25, -0.25].repeat(100)
    );
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
fn analysis_applied_outputs_preserve_the_source_output_route() {
    for output in [
        windfall_project::ClipAudioOutput::Mixer,
        windfall_project::ClipAudioOutput::Direct,
    ] {
        let rig = Rig::new();
        let (clip, _, _) = fixture(&rig);
        rig.session
            .dispatch(
                windfall_project::Command::UpdateAudioClips {
                    updates: vec![windfall_project::AudioClipUpdate {
                        id: clip,
                        patch: windfall_project::AudioClipPatch {
                            output: Some(output),
                            ..Default::default()
                        },
                    }],
                },
                None,
            )
            .unwrap();
        let job = ready(&rig, clip);
        let result = rig.session.analysis_apply(apply(&job, false)).unwrap();
        let applied = rig.project();
        let applied_clip = applied
            .playlist
            .clips
            .iter()
            .find(|clip| Some(&clip.id.0) == result.created.last())
            .unwrap();
        assert!(matches!(
            applied_clip.content,
            windfall_project::ClipContent::Audio { output: actual, .. } if actual == output
        ));
    }
}

#[test]
fn analysis_apply_refuses_a_superseded_engine_plan_without_consuming_review() {
    let rig = Rig::new();
    let (clip, _, _) = fixture(&rig);
    let job = ready(&rig, clip);
    let hold = rig.session.hold("analysis:prepared");
    let request = apply(&job, false);
    let worker = rig.session.background(move |s| s.analysis_apply(request));
    hold.wait();
    let before = rig.session.document_snapshot();
    let pool = rig.session.state().pool.clone();
    // Advance only the engine's publication generation. Document/source guards
    // remain current, so the prepared engine lease must be the refusing guard.
    rig.session.controller().set_project(&before.project, &pool);
    hold.release();
    let error = worker.join().unwrap().unwrap_err();
    assert!(error.contains("analysis:stale"), "{error}");
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(
        rig.session.analysis_status(&job.job).unwrap().status,
        AnalysisStatus::Ready
    );
}

#[test]
fn analysis_capture_rechecks_document_loaded_identity_and_recording_authority() {
    for change in ["edit", "source", "recording"] {
        let rig = Rig::new();
        let (clip, _, _) = fixture(&rig);
        let hold = rig.session.hold("analysis:captured");
        let worker = rig
            .session
            .background(move |s| s.analysis_submit(request(clip)));
        hold.wait();
        match change {
            "edit" => {
                rig.session
                    .dispatch(
                        windfall_project::Command::AddPlaylistTrack {
                            name: None,
                            index: None,
                        },
                        None,
                    )
                    .unwrap();
            }
            "source" => {
                let mut state = rig.session.state();
                let sample = state
                    .document
                    .project()
                    .playlist
                    .clips
                    .iter()
                    .find(|c| c.id == clip)
                    .unwrap()
                    .content
                    .sample()
                    .unwrap();
                state.pool.insert(
                    sample,
                    AudioBuffer::from_interleaved(48_000, 2, vec![0.0; 200]),
                );
            }
            _ => super::archive::start_recording(&rig),
        }
        let before = rig.session.document_snapshot();
        hold.release();
        let error = worker.join().unwrap().unwrap_err();
        assert!(
            error.contains(if change == "recording" {
                "recording"
            } else {
                "analysis:stale"
            }),
            "{error}"
        );
        assert_eq!(rig.session.document_snapshot(), before);
        assert_eq!(
            rig.session
                .inner
                .analysis
                .test_manager()
                .usage()
                .retained_jobs,
            0
        );
        rig.session.recording_cancel();
    }
}

#[test]
fn analysis_final_apply_rechecks_full_pool_load_intent_and_preserves_published_finals() {
    for change in [
        "edit",
        "replace",
        "otherSource",
        "loading",
        "loaded",
        "failed",
        "samplerRequest",
        "cancel",
        "recording",
    ] {
        let rig = Rig::new();
        let (clip, _, _) = fixture(&rig);
        let job = ready(&rig, clip);
        let hold = rig.session.hold("analysis:prepared");
        let request = apply(&job, false);
        let worker = rig.session.background(move |s| s.analysis_apply(request));
        hold.wait();
        match change {
            "edit" => {
                rig.session
                    .dispatch(
                        windfall_project::Command::AddPlaylistTrack {
                            name: None,
                            index: None,
                        },
                        None,
                    )
                    .unwrap();
            }
            "replace" => {
                rig.session.project_new().unwrap();
            }
            "otherSource" => {
                let mut state = rig.session.state();
                let id = state.document.project().samples[0].id;
                state
                    .pool
                    .insert(id, AudioBuffer::from_interleaved(48_000, 2, vec![0.0; 20]));
            }
            "loading" => {
                rig.session.state().loading.insert(SampleId(u32::MAX));
            }
            "loaded" => {
                rig.session.state().loaded.insert(SampleId(u32::MAX));
            }
            "failed" => {
                rig.session.state().failed.insert(SampleId(u32::MAX));
            }
            "samplerRequest" => {
                rig.session.sampler_preparation_begin().unwrap();
            }
            "cancel" => rig.session.analysis_cancel_preparation(),
            _ => super::archive::start_recording(&rig),
        }
        let before = rig.session.document_snapshot();
        hold.release();
        let error = worker.join().unwrap().unwrap_err();
        let kind = match change {
            "cancel" => "analysis:cancelled",
            "recording" => "recording",
            _ => "analysis:stale",
        };
        assert!(error.contains(kind), "{change}: {error}");
        assert_eq!(rig.session.document_snapshot(), before);
        assert_eq!(
            rig.session.analysis_status(&job.job).unwrap().status,
            AnalysisStatus::Ready
        );
        let published = rig.session.inner.analysis.test_manager().usage();
        assert_eq!(published.published_files, 1);
        let outputs = rig.folder.path().join("recordings/Analysis/Outputs");
        let final_path = std::fs::read_dir(&outputs)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let bytes = std::fs::read(&final_path).unwrap();
        assert_eq!(
            rig.session.analysis_cancel(&job.job).unwrap().status,
            AnalysisStatus::Cancelled
        );
        rig.session.analysis_forget(&job.job).unwrap();
        assert_eq!(std::fs::read(final_path).unwrap(), bytes);
        rig.session.recording_cancel();
    }
}

#[test]
fn analysis_actual_file_content_changes_with_same_size_and_timestamp_are_stale() {
    let rig = Rig::new();
    let (clip, path, _) = fixture(&rig);
    let job = ready(&rig, clip);
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let identity = crate::library::file_identity(std::path::Path::new(&path)).unwrap();
    let original_size = std::fs::metadata(&path).unwrap().len();
    let replacement = rig.file("same-size-replacement.wav");
    windfall_codec::write_wav(
        &replacement,
        &AudioBuffer::from_interleaved(48_000, 2, vec![0.5; 200]),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
    // Change contents in place, preserving the native file identity as well
    // as size and mtime. File-key replacement alone cannot pass this test.
    std::fs::write(&path, std::fs::read(replacement).unwrap()).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(
        crate::library::file_identity(std::path::Path::new(&path)).unwrap(),
        identity
    );
    assert_eq!(std::fs::metadata(&path).unwrap().len(), original_size);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        modified
    );
    let before = rig.session.document_snapshot();
    assert!(
        rig.session
            .analysis_apply(apply(&job, false))
            .unwrap_err()
            .contains("analysis:stale")
    );
    assert!(
        rig.session
            .analysis_submit(request(clip))
            .unwrap_err()
            .contains("analysis:stale")
    );
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn analysis_revision_request_ranges_and_absent_models_refuse_without_document_changes() {
    let rig = Rig::new();
    let (clip, _, _) = fixture(&rig);
    let before = rig.session.document_snapshot();
    for range in [
        ("1", "1"),
        ("100", "101"),
        ("01", "100"),
        ("0", "18446744073709551616"),
    ] {
        let mut r = request(clip);
        r.start_frame = range.0.into();
        r.end_frame = range.1.into();
        assert!(rig.session.analysis_submit(r).is_err());
    }
    let mut absent = request(clip);
    absent.model_id = "absent".into();
    assert!(
        rig.session
            .analysis_submit(absent)
            .unwrap_err()
            .contains("analysis:modelAbsent")
    );
    let job = ready(&rig, clip);
    let mut obsolete = apply(&job, false);
    obsolete.request = "0".into();
    assert!(
        rig.session
            .analysis_apply(obsolete)
            .unwrap_err()
            .contains("analysis:stale")
    );
    let mut next_model = model();
    next_model.revision = "2".into();
    rig.session
        .analysis_model_import(&rig.file("authored.model"), next_model)
        .unwrap();
    assert!(
        rig.session
            .analysis_apply(apply(&job, false))
            .unwrap_err()
            .contains("analysis:stale")
    );
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn analysis_partial_range_adds_aligned_audio_and_complete_range_replacement_is_required() {
    let rig = Rig::new();
    let (clip, _, _) = fixture(&rig);
    let mut r = request(clip);
    r.start_frame = "25".into();
    r.end_frame = "75".into();
    let job = rig.session.analysis_submit(r).unwrap();
    rig.session
        .inner
        .analysis
        .test_manager()
        .wait(native::JobId(job.job.parse().unwrap()))
        .unwrap();
    let before = rig.session.document_snapshot();
    assert!(
        rig.session
            .analysis_apply(apply(&job, true))
            .unwrap_err()
            .contains("complete rendered range")
    );
    assert_eq!(
        rig.session
            .inner
            .analysis
            .test_manager()
            .usage()
            .published_files,
        0
    );
    assert_eq!(rig.session.document_snapshot(), before);
    let result = rig.session.analysis_apply(apply(&job, false)).unwrap();
    let project = rig.project();
    let derived = project
        .playlist
        .clips
        .iter()
        .find(|c| c.id.0 == result.created[1])
        .unwrap();
    assert_eq!((derived.start, derived.length, derived.offset), (101, 2, 0));
    assert!(project.playlist.clips.iter().any(|c| c.id == clip));
    assert_eq!(
        rig.session
            .state()
            .pool
            .get(SampleId(result.created[0]))
            .unwrap()
            .frames(),
        50
    );
}

#[test]
fn analysis_sampler_preparation_failure_preserves_actual_document_pool_history_and_controller() {
    use windfall_project::{
        ClipStretchQuality, Command, SamplerKeyRange, SamplerPatch, SamplerStretch,
    };
    let rig = Rig::new();
    let (clip, _, _) = fixture(&rig);
    let channel = rig.channel(0);
    rig.session
        .dispatch(
            Command::UpdateSampler {
                id: channel,
                patch: SamplerPatch {
                    stretch: Some(SamplerStretch::Spectral {
                        ratio: 1.0,
                        quality: ClipStretchQuality::Fast,
                        formants: false,
                        range: SamplerKeyRange {
                            first: 60,
                            last: 60,
                        },
                    }),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    let job = ready(&rig, clip);
    {
        let mut state = rig.session.state();
        let mut bounded = windfall_engine::SamplePool::with_sampler_budget(1);
        for sample in &state.document.project().samples {
            if let Some(audio) = state.pool.get(sample.id) {
                bounded.insert(sample.id, audio.clone());
            }
        }
        state.pool = bounded;
    }
    let before = rig.session.document_snapshot();
    let pool = rig.session.state().pool.clone();
    assert!(rig.session.controller().sampler_key_supported(channel, 60));
    assert!(
        rig.session
            .analysis_apply(apply(&job, false))
            .unwrap_err()
            .contains("analysis:samplerPreparation")
    );
    assert_eq!(rig.session.document_snapshot(), before);
    assert!(rig.session.state().pool.same_sources(&pool));
    assert!(rig.session.controller().sampler_key_supported(channel, 60));
    assert_eq!(
        rig.session.analysis_status(&job.job).unwrap().status,
        AnalysisStatus::Ready
    );
}

#[test]
fn analysis_preserves_pending_source_attachment_barrier_through_apply_and_undo() {
    let rig = Rig::new();
    let (clip, _, _) = fixture(&rig);
    let job = ready(&rig, clip);
    let missing = rig.project().samples[0].id;
    {
        let mut state = rig.session.state();
        state.pool.remove(missing);
        state.loaded.remove(&missing);
        state.failed.remove(&missing);
        state.loading.insert(missing);
    }
    rig.session.analysis_apply(apply(&job, false)).unwrap();
    assert!(rig.session.state().pool.get(missing).is_none());
    assert!(!rig.session.state().loaded.contains(&missing));
    assert!(rig.session.state().loading.contains(&missing));
    rig.session.undo().unwrap();
    assert!(rig.session.state().pool.get(missing).is_none());
    assert!(rig.session.state().loading.contains(&missing));
}

#[test]
fn analysis_sixteen_outputs_prepare_metadata_and_guarded_commit_without_foundation_allocations_or_frees()
 {
    let rig = Rig::new();
    let (clip, _, _) = fixture(&rig);
    let mut spec = model();
    spec.revision = "2".into();
    spec.outputs = (0..16)
        .map(|index| AnalysisOutputRole {
            role: format!("lifecycle-copy-{index}"),
            channels: 2,
        })
        .collect();
    rig.session
        .analysis_model_import(&rig.file("authored.model"), spec)
        .unwrap();
    let mut r = request(clip);
    r.model_revision = "2".into();
    let job = rig.session.analysis_submit(r).unwrap();
    assert_eq!(
        rig.session
            .inner
            .analysis
            .test_manager()
            .wait(native::JobId(job.job.parse().unwrap()))
            .unwrap()
            .status,
        native::JobStatus::Ready
    );
    let result = rig.session.analysis_apply(apply(&job, false)).unwrap();
    assert_eq!(result.created.len(), 32);
    assert_eq!(rig.session.inner.analysis.final_calls(), (0, 0));
    let usage = rig.session.inner.analysis.test_manager().usage();
    assert_eq!(usage.published_files, 16);
    assert_eq!(usage.reserved_disk_bytes, 0);
    assert_eq!(usage.reserved_output_files, 0);
    eprintln!(
        "analysis Session sixteen outputs: consumed quota={usage:?}; final allocator calls={:?}",
        rig.session.inner.analysis.final_calls()
    );
}

#[test]
fn analysis_persistent_outputs_are_in_portable_archive_after_cancel_forget_and_shutdown() {
    let rig = Rig::new();
    let (clip, _, _) = fixture(&rig);
    let job = ready(&rig, clip);
    let result = rig.session.analysis_apply(apply(&job, false)).unwrap();
    let sample = SampleId(result.created[0]);
    rig.session.analysis_cancel(&job.job).unwrap();
    rig.session.analysis_forget(&job.job).unwrap();
    rig.session.analysis_shutdown();
    let archive = rig.file("analysis-portable.zip");
    rig.session.project_archive_save(&archive).unwrap();
    let rig = rig.restart();
    rig.session.project_open(&archive).unwrap();
    assert_eq!(
        rig.session.state().pool.get(sample).unwrap().samples(),
        &[0.25, -0.25].repeat(100)
    );
}

struct BlockingAdapter {
    entered: Arc<std::sync::Barrier>,
    release: Arc<std::sync::Barrier>,
}
impl native::AnalysisAdapter for BlockingAdapter {
    fn id(&self) -> &str {
        "test-only-copy"
    }
    fn version(&self) -> &str {
        "1"
    }
    fn run(
        &self,
        input: &native::CapturedInput,
        model: &native::VerifiedModel,
        context: &mut native::WorkerContext,
        artifacts: &mut native::ArtifactWriter,
    ) -> native::Result<()> {
        self.entered.wait();
        self.release.wait();
        context.checkpoint(1)?;
        CopyAdapter.run(input, model, context, artifacts)
    }
}
#[test]
fn analysis_queue_running_cancellation_and_service_shutdown_join_outside_state() {
    let rig = Rig::new();
    let (clip, _, _) = fixture(&rig);
    let entered = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    rig.session
        .inner
        .analysis
        .install_test_adapter(Arc::new(BlockingAdapter {
            entered: entered.clone(),
            release: release.clone(),
        }));
    let running = rig.session.analysis_submit(request(clip)).unwrap();
    entered.wait();
    let queued = rig.session.analysis_submit(request(clip)).unwrap();
    assert_eq!(queued.status, AnalysisStatus::Queued);
    assert_eq!(
        rig.session.analysis_cancel(&queued.job).unwrap().status,
        AnalysisStatus::Cancelled
    );
    rig.session.analysis_forget(&queued.job).unwrap();
    let manager = rig.session.inner.analysis.test_manager();
    let sequence = manager
        .snapshot(native::JobId(running.job.parse().unwrap()))
        .unwrap()
        .sequence;
    let shutdown = rig.session.background(|s| s.analysis_shutdown());
    let cancelling = manager
        .wait_for_change(native::JobId(running.job.parse().unwrap()), sequence)
        .unwrap();
    assert_eq!(cancelling.status, native::JobStatus::Cancelling);
    let before = rig.session.document_snapshot(); // join has not taken State
    assert!(!rig.session.analysis_capability().available);
    release.wait();
    shutdown.join().unwrap();
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(
        rig.session.analysis_status(&running.job).unwrap().status,
        AnalysisStatus::Cancelled
    );
    rig.session.analysis_forget(&running.job).unwrap();
    assert_eq!(manager.usage().retained_jobs, 0);
    let first_id: u64 = running.job.parse().unwrap();
    drop(manager);
    let rig = rig.restart();
    let (clip, _, _) = fixture(&rig);
    let next = ready(&rig, clip);
    assert!(next.job.parse::<u64>().unwrap() > first_id);
}

#[test]
fn analysis_publication_collision_never_changes_competitor_document_or_original() {
    let rig = Rig::new();
    let (clip, source, original) = fixture(&rig);
    let job = ready(&rig, clip);
    let output = rig
        .folder
        .path()
        .join("recordings/Analysis/Outputs")
        .join(format!("Analysis-{}-{}-0.wav", std::process::id(), job.job));
    std::fs::write(&output, b"competitor-owned-bytes").unwrap();
    let before = rig.session.document_snapshot();
    assert!(
        rig.session
            .analysis_apply(apply(&job, false))
            .unwrap_err()
            .contains("analysis:collision")
    );
    assert_eq!(rig.session.document_snapshot(), before);
    rig.session.analysis_cancel(&job.job).unwrap();
    rig.session.analysis_forget(&job.job).unwrap();
    rig.session.analysis_shutdown();
    assert_eq!(std::fs::read(output).unwrap(), b"competitor-owned-bytes");
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[cfg(windows)]
#[test]
fn analysis_final_source_and_output_handles_hold_native_no_write_no_delete_authority() {
    let rig = Rig::new();
    let (clip, source, original) = fixture(&rig);
    let job = ready(&rig, clip);
    let hold = rig.session.hold("analysis:prepared");
    let r = apply(&job, false);
    let worker = rig.session.background(move |s| s.analysis_apply(r));
    hold.wait();
    let output = std::fs::read_dir(rig.folder.path().join("recordings/Analysis/Outputs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let output_bytes = std::fs::read(&output).unwrap();
    assert!(std::fs::write(&source, b"replacement").is_err());
    assert!(std::fs::remove_file(&source).is_err());
    assert!(std::fs::write(&output, b"replacement").is_err());
    assert!(std::fs::remove_file(&output).is_err());
    hold.release();
    worker.join().unwrap().unwrap();
    assert_eq!(std::fs::read(source).unwrap(), original);
    assert_eq!(std::fs::read(output).unwrap(), output_bytes);
}

const MODEL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../crates/windfall-analysis/tests/fixtures/lifecycle-cpu-v1.model"
));

#[cfg(windows)]
fn junction(link: &std::path::Path, target: &std::path::Path) {
    let result = std::process::Command::new("cmd.exe")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[cfg(windows)]
#[test]
fn analysis_ordinary_parent_namespace_cannot_be_replaced_during_final_preparation() {
    use std::os::windows::fs::OpenOptionsExt;
    let rig = Rig::new();
    let (_, source, original) = fixture(&rig);
    let parent = rig.folder.path().join("ordinary");
    let moved = rig.folder.path().join("renamed");
    std::fs::create_dir(&parent).unwrap();
    let path = parent.join("original.wav");
    std::fs::rename(source, &path).unwrap();
    let result = rig
        .session
        .add_audio_clip_from_file(
            &crate::paths::display(&path),
            ClipPlace {
                track: None,
                start: 0,
                mixer_track: None,
            },
        )
        .unwrap();
    let job = ready(&rig, ClipId(*result.created.last().unwrap()));
    let hold = rig.session.hold("analysis:prepared");
    let r = apply(&job, false);
    let worker = rig.session.background(move |s| s.analysis_apply(r));
    hold.wait();
    assert!(
        std::fs::rename(&parent, &moved).is_err(),
        "parent rename lost namespace authority"
    );
    assert!(
        std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(&parent)
            .is_err(),
        "reparse mutation obtained directory write authority"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    hold.release();
    worker.join().unwrap().unwrap();
    // Acquired namespace handles are actually released after the final guards.
    std::fs::rename(&parent, &moved).unwrap();
    assert_eq!(std::fs::read(moved.join("original.wav")).unwrap(), original);
}

#[cfg(windows)]
#[test]
fn analysis_junction_retarget_after_hash_cannot_install_a_changed_source_namespace() {
    let rig = Rig::new();
    let (_, source, original) = fixture(&rig);
    let first = rig.folder.path().join("first");
    let second = rig.folder.path().join("second");
    let alias = rig.folder.path().join("alias");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    std::fs::rename(&source, first.join("original.wav")).unwrap();
    windfall_codec::write_wav(
        second.join("original.wav"),
        &AudioBuffer::from_interleaved(48_000, 2, vec![0.75; 200]),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
    junction(&alias, &first);
    let source = crate::paths::display(&alias.join("original.wav"));
    let result = rig
        .session
        .add_audio_clip_from_file(
            &source,
            ClipPlace {
                track: None,
                start: 0,
                mixer_track: None,
            },
        )
        .unwrap();
    let clip = ClipId(*result.created.last().unwrap());
    let before = rig.session.document_snapshot();
    let submitted = rig.session.analysis_submit(request(clip));
    // A conservative guard may refuse the existing reparse namespace at capture.
    // A guard that accepts it must retain that namespace through final commit.
    let job = match submitted {
        Err(reason) => {
            assert!(reason.contains("analysis:sourceNamespace"), "{reason}");
            assert_eq!(rig.session.document_snapshot(), before);
            assert_eq!(
                rig.session
                    .inner
                    .analysis
                    .test_manager()
                    .usage()
                    .published_files,
                0
            );
            assert_eq!(std::fs::read(first.join("original.wav")).unwrap(), original);
            return;
        }
        Ok(job) => job,
    };
    rig.session
        .inner
        .analysis
        .test_manager()
        .wait(native::JobId(job.job.parse().unwrap()))
        .unwrap();
    let identity = crate::library::file_identity(std::path::Path::new(&source)).unwrap();
    let metadata = std::fs::metadata(&source).unwrap();
    let hold = rig.session.hold("analysis:prepared");
    let r = apply(&job, false);
    let worker = rig.session.background(move |s| s.analysis_apply(r));
    hold.wait(); // actual hash/identity check completed; source file remains open
    assert_eq!(
        crate::library::file_identity(std::path::Path::new(&source)).unwrap(),
        identity
    );
    assert_eq!(std::fs::metadata(&source).unwrap().len(), metadata.len());
    assert_eq!(
        std::fs::metadata(&source).unwrap().modified().unwrap(),
        metadata.modified().unwrap()
    );
    assert_eq!(std::fs::read(&source).unwrap(), original);
    std::fs::remove_dir(&alias).unwrap();
    junction(&alias, &second);
    assert_ne!(std::fs::read(&source).unwrap(), original);
    hold.release();
    assert!(
        worker.join().unwrap().is_err(),
        "retargeted junction was applied"
    );
    assert_eq!(rig.session.document_snapshot(), before);
}

#[cfg(windows)]
#[test]
fn analysis_substituted_drive_is_refused_before_capture_and_after_retarget() {
    let rig = Rig::new();
    let (_, source, original) = fixture(&rig);
    let first = rig.folder.path().join("dos-first");
    let second = rig.folder.path().join("dos-second");
    std::fs::create_dir(&first).unwrap();
    std::fs::create_dir(&second).unwrap();
    std::fs::copy(source, first.join("original.wav")).unwrap();
    windfall_codec::write_wav(
        second.join("original.wav"),
        &AudioBuffer::from_interleaved(48_000, 2, vec![0.75; 200]),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
    let modified = std::fs::metadata(first.join("original.wav"))
        .unwrap()
        .modified()
        .unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(second.join("original.wav"))
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let mut drive = dos_device::Drive::new(format!(r"\??\{}", first.display()));
    let source = format!(r"{}\original.wav", drive.name());
    let result = rig
        .session
        .add_audio_clip_from_file(
            &source,
            ClipPlace {
                track: None,
                start: 0,
                mixer_track: None,
            },
        )
        .unwrap();
    let clip = ClipId(*result.created.last().unwrap());
    let before = rig.session.document_snapshot();
    let sample = before.project.samples.last().unwrap();
    assert_eq!(
        sample.path,
        windfall_project::SamplePath::External(source.clone())
    );
    let loaded_identity = rig.session.state().pool.get(sample.id).unwrap().identity();
    let original_file_identity =
        crate::library::file_identity(&first.join("original.wav")).unwrap();
    let source_length = std::fs::metadata(&source).unwrap().len();
    let error = rig.session.analysis_submit(request(clip)).unwrap_err();
    assert!(error.contains("analysis:sourceNamespace"), "{error}");
    assert!(error.contains("substituted or chained"), "{error}");
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert_eq!(rig.session.document_snapshot(), before);
    drive.push(format!(r"\??\{}", second.display())).unwrap();
    assert_ne!(std::fs::read(&source).unwrap(), original);
    assert_eq!(std::fs::metadata(&source).unwrap().len(), source_length);
    assert_eq!(
        std::fs::metadata(&source).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(
        crate::library::file_identity(&first.join("original.wav")).unwrap(),
        original_file_identity
    );
    assert_eq!(
        rig.session.state().pool.get(sample.id).unwrap().identity(),
        loaded_identity
    );
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(std::fs::read(first.join("original.wav")).unwrap(), original);
    let error = rig.session.analysis_submit(request(clip)).unwrap_err();
    assert!(error.contains("analysis:sourceNamespace"), "{error}");
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(
        rig.session
            .inner
            .analysis
            .test_manager()
            .usage()
            .retained_jobs,
        0
    );
}

#[cfg(windows)]
#[test]
fn analysis_direct_volume_drive_mapping_is_retained_through_final_apply() {
    use std::path::{Component, Prefix};
    let rig = Rig::new();
    let (_, path, original) = fixture(&rig);
    let path = std::path::Path::new(&path);
    let letter = match path.components().next().unwrap() {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => char::from(letter),
            _ => panic!("Fixture must be on a local drive"),
        },
        _ => panic!("Fixture must be absolute"),
    };
    let target = dos_device::query(&format!("{letter}:")).unwrap().unwrap()[0].clone();
    let mut drive = dos_device::Drive::new(target.clone());
    let relative = path.strip_prefix(format!(r"{letter}:\")).unwrap();
    let source = format!(r"{}\{}", drive.name(), relative.display());
    let result = rig
        .session
        .add_audio_clip_from_file(
            &source,
            ClipPlace {
                track: None,
                start: 0,
                mixer_track: None,
            },
        )
        .unwrap();
    let job = ready(&rig, ClipId(*result.created.last().unwrap()));
    let hold = rig.session.hold("analysis:prepared");
    let request = apply(&job, false);
    let worker = rig.session.background(move |s| s.analysis_apply(request));
    hold.wait();
    // A local DOS retarget attempt must not replace the effective mapping while
    // production SourceFile owns it, even though the original NT object lives.
    let replacement = format!(r"\??\{}", rig.folder.path().display());
    let _retarget = drive.push(replacement.clone());
    assert_eq!(dos_device::query(drive.name()).unwrap().unwrap()[0], target);
    assert_eq!(std::fs::read(&source).unwrap(), original);
    hold.release();
    worker.join().unwrap().unwrap();
    assert_eq!(
        rig.session.analysis_status(&job.job).unwrap().status,
        AnalysisStatus::Consumed
    );
    // Authority is process-volatile, and releases after the successful apply.
    drive.push(replacement.clone()).unwrap();
    assert_eq!(
        dos_device::query(drive.name()).unwrap().unwrap()[0],
        replacement
    );
}
fn model() -> AnalysisModel {
    AnalysisModel {
        onnx: None,        id: "test-only-lifecycle-copy".into(),
        version: "1".into(),
        revision: "1".into(),
        sha256: "07f6812eb7ac451adc4b010bb18b2fe098ce235498880c80ead3fad519e47d33".into(),
        bytes: MODEL.len().to_string(),
        max_bytes: "4096".into(),
        provenance: AnalysisProvenance {
            origin: "authored:windfall/lifecycle-cpu-v1".into(),
            source_revision: "fixture-v1".into(),
            author: "Windfall contributors".into(),
            license_spdx: "CC0-1.0".into(),
            license_reference: "tests/fixtures/README.md".into(),
            adapter_id: "test-only-copy".into(),
            adapter_version: "1".into(),
            device: "cpu".into(),
        },
        sample_rate: 48_000,
        input_channels: 2,
        max_input_frames: "48000".into(),
        outputs: vec![AnalysisOutputRole {
            role: "lifecycle-copy".into(),
            channels: 2,
        }],
    }
}
fn request(clip: ClipId) -> AnalysisSubmit {
    AnalysisSubmit {
        amount: None,        task: Default::default(),
        clip,
        model_id: model().id,
        model_version: "1".into(),
        model_revision: "1".into(),
        start_frame: "0".into(),
        end_frame: "100".into(),
    }
}

#[test]
fn analysis_stem_review_preview_selected_remix_layout_is_aligned_and_one_undo() {
    let rig = Rig::new();
    let (clip,path,original) = fixture(&rig);
    let weights = rig.file("authored.model");
    let mut spec = model();
    spec.revision = "2".into();
    spec.outputs = ["drums","bass","vocals","other"].into_iter().map(|role|
        AnalysisOutputRole{role:role.into(),channels:2}).collect();
    rig.session.analysis_model_import(&weights,spec).unwrap();
    let mut submit = request(clip);
    submit.task = windfall_ipc::AnalysisTask::StemSeparation;
    submit.model_revision = "2".into();
    let job = rig.session.analysis_submit(submit).unwrap();
    rig.session.inner.analysis.test_manager().wait(native::JobId(job.job.parse().unwrap())).unwrap();
    let review = rig.session.analysis_review(&job.ticket).unwrap();
    assert_eq!(review.task,windfall_ipc::AnalysisTask::StemSeparation);
    let before = rig.session.document_snapshot();
    rig.session.analysis_preview(&job.ticket,"vocals").unwrap();
    assert_eq!(rig.session.document_snapshot(),before);
    let mut request = apply(&job,false);
    request.selected_roles = Some(vec!["drums".into(),"vocals".into()]);
    request.separate_tracks = true; request.mute_original = true;
    let result = rig.session.analysis_apply(request).unwrap();
    assert_eq!(result.created.len(),8);
    let project = rig.project();
    assert_eq!(project.mixer.tracks.len(),before.project.mixer.tracks.len()+2);
    assert_eq!(project.playlist.tracks.len(),before.project.playlist.tracks.len()+2);
    assert_eq!(rig.session.document_snapshot().history.cursor,before.history.cursor+1);
    let added:Vec<_>=project.playlist.clips.iter().filter(|c|c.id!=clip).collect();
    assert_eq!(added.len(),2); assert_eq!(added[0].start,added[1].start);
    assert_eq!(added[0].length,added[1].length); assert_ne!(added[0].track,added[1].track);
    assert!(project.playlist.clips.iter().find(|c|c.id==clip).unwrap().muted);
    assert_eq!(std::fs::read(path).unwrap(),original);
    rig.session.undo().unwrap();
    assert_eq!(rig.project().playlist.tracks,before.project.playlist.tracks);
    rig.session.redo().unwrap();
    let saved=rig.file("remix.windfall");rig.session.project_save(Some(&saved)).unwrap();
    let rig=rig.restart();rig.session.project_open(&saved).unwrap();
    assert_eq!(rig.project().playlist,project.playlist);
}

#[test]
fn analysis_dereverb_modes_reach_adapter_and_refuse_mismatched_model_roles() {
    for (task,role) in [(windfall_ipc::AnalysisTask::DereverbGeneral,"dereverb-general"),
        (windfall_ipc::AnalysisTask::DereverbVocal,"dereverb-vocal")] {
        let rig=Rig::new();let (clip,_,_)=fixture(&rig);
        let mut spec=model();spec.revision="2".into();
        spec.outputs=vec![AnalysisOutputRole{role:role.into(),channels:2}];
        rig.session.analysis_model_import(&rig.file("authored.model"),spec).unwrap();
        let mut submit=request(clip);submit.model_revision="2".into();submit.task=task;
        let job=rig.session.analysis_submit(submit).unwrap();
        rig.session.inner.analysis.test_manager().wait(native::JobId(job.job.parse().unwrap())).unwrap();
        let review=rig.session.analysis_review(&job.ticket).unwrap();
        assert_eq!(review.task,task);assert_eq!(review.artifacts[0].role,role);
        rig.session.analysis_apply(apply(&job,true)).unwrap();
    }
    let rig=Rig::new();let (clip,_,_)=fixture(&rig);let before=rig.session.document_snapshot();
    let mut submit=request(clip);submit.task=windfall_ipc::AnalysisTask::DereverbVocal;
    assert!(rig.session.analysis_submit(submit).is_err());
    assert_eq!(rig.session.document_snapshot(),before);
}

#[test]
fn analysis_unknown_duplicate_or_empty_stem_selection_leaves_ready_review_unchanged() {
    let rig=Rig::new();let (clip,_,_)=fixture(&rig);let job=ready(&rig,clip);
    let before=rig.session.document_snapshot();
    for roles in [vec![],vec!["unknown".into()],vec!["lifecycle-copy".into(),"lifecycle-copy".into()]] {
        let mut request=apply(&job,false);request.selected_roles=Some(roles);
        assert!(rig.session.analysis_apply(request).is_err());
        assert_eq!(rig.session.document_snapshot(),before);
        assert_eq!(rig.session.analysis_status(&job.job).unwrap().status,AnalysisStatus::Ready);
    }
}
#[test]
fn analysis_production_local_import_is_explicit_and_never_fakes_inference() {
    let rig = Rig::new();
    let before = rig.session.document_snapshot();
    let capability = rig.session.analysis_capability();
    assert!(capability.native);
    assert!(capability.available);
    assert!(capability.classical_dereverb);
    assert!(capability.models.is_empty());
    assert!(
        rig.session
            .analysis_submit(request(ClipId(1)))
            .unwrap_err()
            .contains("analysis:modelAbsent")
    );
    let path = rig.file("authored.model");
    std::fs::write(&path, MODEL).unwrap();
    let mut bad = model();
    bad.sha256 = "00".repeat(32);
    assert!(rig.session.analysis_model_import(&path, bad).is_err());
    assert!(rig.session.analysis_capability().models.is_empty());
    assert_eq!(
        rig.session.analysis_model_import(&path, model()).unwrap(),
        model()
    );
    let capability = rig.session.analysis_capability();
    assert_eq!(capability.models, vec![model()]);
    assert!(capability.available);
    assert!(capability.classical_dereverb);
    assert!(
        rig.session
            .analysis_submit(request(ClipId(1)))
            .unwrap_err()
            .contains("analysis:unavailable")
    );
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn builtin_dereverb_without_weights_reviews_amount_and_applies_one_saved_undo_step() {
    for task in [windfall_ipc::AnalysisTask::DereverbGeneral, windfall_ipc::AnalysisTask::DereverbVocal] {
        let rig = Rig::new();
        assert!(rig.session.analysis_capability().classical_dereverb);
        let path = rig.file("authored-dry.wav");
        let audio = AudioBuffer::from_interleaved(8_000, 2, (0..8_000).flat_map(|i| {
            let x = (std::f32::consts::TAU * 220.0 * i as f32 / 8_000.0).sin() * 0.2;
            [x, -0.5*x]
        }).collect());
        windfall_codec::write_wav(&path, &audio, windfall_codec::WavSampleFormat::Float32).unwrap();
        let original = std::fs::read(&path).unwrap();
        let imported = rig.session.add_audio_clip_from_file(&path, ClipPlace { track: None, start: 0, mixer_track: None }).unwrap();
        let clip = ClipId(*imported.created.last().unwrap());
        let before = rig.session.document_snapshot();
        let mut request = request(clip);
        request.task = task; request.model_id = native::BUILTIN_DEREVERB_ID.into();
        request.end_frame = "8000".into(); request.amount = Some(0.4);
        let job = rig.session.analysis_submit(request).unwrap();
        let settled = rig.session.inner.analysis.test_manager().wait(native::JobId(job.job.parse().unwrap())).unwrap();
        assert_eq!(settled.status, native::JobStatus::Ready, "{settled:?}");
        let review = rig.session.analysis_review(&job.ticket).unwrap();
        assert_eq!(review.task, task); assert_eq!(review.amount, 0.4);
        assert_eq!(review.model.bytes, "0"); assert_eq!(review.artifacts[0].frames, "8000");
        assert_eq!(review.artifacts[0].frame_origin, "0");
        assert!(rig.session.analysis_capability().models.is_empty());
        rig.session.analysis_preview(&job.ticket, &review.artifacts[0].role).unwrap();
        rig.session.analysis_preview(&job.ticket, "dry-input").unwrap();
        let result = rig.session.analysis_apply(apply(&job, true)).unwrap();
        assert_eq!(rig.session.document_snapshot().history.cursor, before.history.cursor + 1);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let derived = SampleId(result.created[0]);
        let edited = rig.project();
        let rendered = rig.session.state().pool.get(derived).unwrap().samples().to_vec();
        rig.session.undo().unwrap();
        let mut restored = before.project; restored.next_id = rig.project().next_id;
        assert_eq!(rig.project(), restored);
        rig.session.redo().unwrap(); assert_eq!(rig.project(), edited);
        let saved = rig.file("dereverb.windfall"); rig.session.project_save(Some(&saved)).unwrap();
        let rig = rig.restart(); rig.session.project_open(&saved).unwrap();
        assert_eq!(rig.project(), edited);
        assert_eq!(rig.session.state().pool.get(derived).unwrap().samples(), rendered);
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}

#[test]
fn builtin_descriptor_cannot_be_imported_as_fake_weights() {
    let rig = Rig::new();
    let mut spec = model(); spec.id = native::BUILTIN_DEREVERB_ID.into();
    let path = rig.file("fake.model"); std::fs::write(&path, MODEL).unwrap();
    assert!(rig.session.analysis_model_import(&path, spec).unwrap_err().contains("not an imported model"));
    assert!(rig.session.analysis_capability().models.is_empty());
}

#[test]
fn explicit_pack_registration_is_restored_on_actual_session_restart() {
    let rig = Rig::new();
    let path = rig.file("authored.model"); std::fs::write(&path, MODEL).unwrap();
    rig.session.analysis_model_import(&path, model()).unwrap();
    let rig = rig.restart();
    assert_eq!(rig.session.analysis_capability().models, vec![model()]);
    assert!(rig.session.analysis_capability().classical_dereverb);
    // Registration survives, inference support is still a separate native gate.
    assert!(rig.session.analysis_submit(request(ClipId(1))).unwrap_err().contains("analysis:unavailable"));
}
