//! The browser panel and sample information.

use std::fs;

use windfall_ipc::{BrowserEntryKind, BrowserRootKind};
use windfall_project::{Command, SampleId, SamplePath};

use super::{Rig, factory_file};
use crate::paths;
use crate::samples::PEAK_BUCKETS;
use crate::session::PROJECT_REPLACED;

fn checked_import(
    rig: &Rig,
    path: &str,
    token: windfall_ipc::LibraryFileToken,
    destination: &str,
) -> Result<windfall_project::DispatchResult, String> {
    match destination {
        "rack" => rig.session.browser_add_channel(path, None, token),
        "playlist" => rig.session.browser_add_clip(
            path,
            crate::session::ClipPlace {
                track: None,
                start: 0,
                mixer_track: None,
            },
            token,
        ),
        "replacement" => {
            rig.session
                .browser_replace_sample(rig.project().channels[0].id, path, token)
        }
        _ => panic!("unknown destination"),
    }
}

fn prepared_sampler_import_fixture() -> (Rig, String) {
    use windfall_codec::{WavSampleFormat, write_wav};
    use windfall_core::AudioBuffer;
    use windfall_project::{ClipStretchQuality, SamplerKeyRange, SamplerPatch, SamplerStretch};
    let rig = Rig::new();
    let folder = rig.folder.path().join("Sources");
    fs::create_dir(&folder).unwrap();
    let old = folder.join("old.wav");
    write_wav(
        &old,
        &AudioBuffer::from_interleaved(48_000, 1, vec![0.25; 4800]),
        WavSampleFormat::Float32,
    )
    .unwrap();
    rig.session
        .set_channel_sample_from_file(rig.channel(0), &paths::display(&old))
        .unwrap();
    {
        let mut state = rig.session.state();
        let mut limited = windfall_engine::SamplePool::with_sampler_budget(256 * 1024);
        for (id, source) in state.pool.iter() {
            limited.insert(id, source.clone());
        }
        state.pool = limited;
    }
    rig.session
        .dispatch(
            Command::UpdateSampler {
                id: rig.channel(0),
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
    let project = rig.project();
    assert!(!rig.session.state().pool.needs_sampler_preparation(&project));
    rig.session
        .browser_add_root(&paths::display(&folder))
        .unwrap();
    (rig, paths::display(&folder))
}

fn refused_file_sampler_replacement(browser: bool) {
    use windfall_codec::{WavSampleFormat, write_wav};
    use windfall_core::AudioBuffer;
    let (mut rig, folder) = prepared_sampler_import_fixture();
    let file = std::path::Path::new(&folder).join("over-budget.wav");
    write_wav(
        &file,
        &AudioBuffer::from_interleaved(48_000, 2, vec![0.75; 48_000 * 2]),
        WavSampleFormat::Float32,
    )
    .unwrap();
    let path = paths::display(&file);
    // Save a clean document and retain a real redo entry before failure.
    rig.session
        .project_save(Some(&rig.file("saved.windfall")))
        .unwrap();
    rig.session
        .dispatch(
            Command::AddChannel {
                name: Some("Redo must survive".into()),
                sample: None,
                instrument: None,
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap();
    rig.session.undo().unwrap();
    let before = rig.session.document_snapshot();
    let pool = rig.session.state().pool.clone();
    let retained = pool.sampler_retained_bytes();
    rig.run(960);
    rig.session.controller().note_on(rig.channel(0), 60, 1.0);
    let audible = rig.run(6000);
    assert!(audible.iter().any(|value| value.abs() > 0.01));
    let result = if browser {
        rig.session.browser_replace_sample(
            rig.channel(0),
            &path,
            rig.session.library_file(&path).unwrap(),
        )
    } else {
        rig.session
            .set_channel_sample_from_file(rig.channel(0), &path)
    };
    let after = rig.session.document_snapshot();
    rig.session.controller().note_on(rig.channel(0), 60, 1.0);
    let after_audio = rig.run(6000);
    let export = rig.session.export_audio(windfall_ipc::ExportOptions {
        path: rig.file("after-refusal.wav"),
        mode: windfall_ipc::PlayMode::Pattern,
        tail_secs: 0.0,
        bit_depth: windfall_ipc::BitDepth::Float32,
        ..Default::default()
    });
    if export.is_ok() {
        assert!(rig.events.wait_for_export().error.is_none());
    }
    assert!(
        result.is_err(),
        "{} replacement committed history {} -> {}, changed project={}, left old playback={}, export={export:?}",
        if browser { "checked" } else { "ordinary" },
        before.history.cursor,
        after.history.cursor,
        before.project != after.project,
        audible == after_audio
    );
    assert!(result.unwrap_err().contains("budget"));
    assert_eq!(after, before, "dirty/history/redo must remain unchanged");
    assert_eq!(after_audio, audible, "audible plan must remain unchanged");
    assert!(
        export.is_ok(),
        "refused edit must not block exporting the old source"
    );
    let state = rig.session.state();
    assert_eq!(state.pool.len(), pool.len());
    for (id, source) in pool.iter() {
        assert_eq!(state.pool.get(id).unwrap().identity(), source.identity());
    }
    assert_eq!(state.pool.sampler_retained_bytes(), retained);
    assert!(
        !state
            .pool
            .needs_sampler_preparation(state.document.project())
    );
}

#[test]
fn ordinary_file_sampler_budget_refusal_is_transactional() {
    refused_file_sampler_replacement(false);
}
#[test]
fn checked_file_sampler_budget_refusal_is_transactional() {
    refused_file_sampler_replacement(true);
}

fn routed_import(
    rig: &Rig,
    path: &str,
    browser: bool,
    destination: &str,
) -> Result<windfall_project::DispatchResult, String> {
    if browser {
        return checked_import(rig, path, rig.session.library_file(path)?, destination);
    }
    match destination {
        "rack" => rig.session.add_channel_from_file(path, None),
        "playlist" => rig.session.add_audio_clip_from_file(
            path,
            crate::session::ClipPlace {
                track: None,
                start: 0,
                mixer_track: None,
            },
        ),
        "replacement" => rig
            .session
            .set_channel_sample_from_file(rig.channel(0), path),
        _ => panic!("unknown destination"),
    }
}

fn settled_missing_source_fixture(spectral: bool) -> (Rig, String, SampleId) {
    use windfall_project::{ClipStretchQuality, SamplerKeyRange, SamplerPatch, SamplerStretch};
    let rig = Rig::new();
    let folder = rig.folder.path().join("Restored");
    fs::create_dir(&folder).unwrap();
    let path = paths::display(&folder.join("missing.wav"));
    let added = rig
        .session
        .dispatch(
            Command::AddSample {
                name: "Missing assigned source".into(),
                path: SamplePath::External(path.clone()),
            },
            None,
        )
        .unwrap();
    let sample = SampleId(added.created[0]);
    rig.wait_until_loaded_or_failed(sample); // Real failed decoder, no synthetic pool/failure edits.
    rig.session
        .dispatch(
            Command::SetChannelSample {
                id: rig.channel(0),
                sample: Some(sample),
            },
            None,
        )
        .unwrap();
    for channel in rig.project().channels.iter().skip(1) {
        rig.session
            .dispatch(Command::RemoveChannel { id: channel.id }, None)
            .unwrap();
    }
    if spectral {
        rig.session
            .dispatch(
                Command::UpdateSampler {
                    id: rig.channel(0),
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
    }
    rig.session
        .dispatch(
            Command::ToggleStep {
                pattern: rig.pattern(),
                channel: rig.channel(0),
                step: 0,
            },
            None,
        )
        .unwrap();
    rig.session
        .browser_add_root(&paths::display(&folder))
        .unwrap();
    let saved = rig
        .session
        .project_save(Some(&rig.file("Saved/missing.windfall")))
        .unwrap();
    // Exercise the review's actual missing-source Open ordering as well as
    // the initial background failure. Open settles the missing decoder before
    // any restore/import; only the budget uses the existing test seam.
    rig.session.project_new().unwrap();
    rig.session.project_open(&saved).unwrap();
    {
        let mut state = rig.session.state();
        let mut limited = windfall_engine::SamplePool::with_sampler_budget(256 * 1024);
        for (id, audio) in state.pool.iter() {
            limited.insert(id, audio.clone());
        }
        state.pool = limited;
    }
    rig.session
        .dispatch(
            Command::AddChannel {
                name: Some("Preserve this redo branch".into()),
                sample: None,
                instrument: None,
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap();
    rig.session.undo().unwrap();
    let before = rig.session.document_snapshot();
    assert!(before.history.cursor < before.history.entries.len() as u32);
    let state = rig.session.state();
    assert!(state.failed.contains(&sample));
    assert!(!state.loaded.contains(&sample));
    assert!(!state.loading.contains(&sample));
    assert!(!state.pool.contains(sample));
    drop(state);
    (rig, path, sample)
}

fn same_path_missing_source_recovery(browser: bool, spectral: bool) {
    use windfall_codec::{WavSampleFormat, write_wav};
    use windfall_core::AudioBuffer;
    let (mut rig, path, sample) = settled_missing_source_fixture(spectral);
    write_wav(
        &path,
        &AudioBuffer::from_interleaved(48_000, 1, vec![0.75; 4800]),
        WavSampleFormat::Float32,
    )
    .unwrap();
    let restored = rig
        .session
        .inner
        .cache
        .decode(std::path::Path::new(&path))
        .unwrap();
    let before = rig.session.document_snapshot();
    let edits = rig.session.state().edits;
    let (pool, mut loaded, loading, mut failed) = {
        let state = rig.session.state();
        (
            state.pool.clone(),
            state.loaded.clone(),
            state.loading.clone(),
            state.failed.clone(),
        )
    };
    routed_import(&rig, &path, browser, "replacement").unwrap();
    assert_eq!(
        rig.session.document_snapshot(),
        before,
        "recovery must preserve revision/dirty/document/history/redo"
    );
    {
        let state = rig.session.state();
        let attached = state.pool.get(sample);
        assert!(
            attached.is_some(),
            "{browser}/{spectral}: identical replacement returned success but the settled failed source is still absent"
        );
        assert_eq!(attached.unwrap().identity(), restored.identity());
        assert_eq!(state.pool.len(), pool.len() + 1);
        for (id, audio) in pool.iter() {
            assert_eq!(state.pool.get(id).unwrap().identity(), audio.identity());
        }
        loaded.insert(sample);
        failed.remove(&sample);
        assert_eq!(state.loaded, loaded);
        assert_eq!(state.loading, loading);
        assert_eq!(state.failed, failed);
        assert_eq!(state.edits, edits);
        assert!(!state.pool.needs_sampler_preparation(&before.project));
    }
    assert!(
        rig.session
            .controller()
            .sampler_key_supported(rig.channel(0), 60)
    );
    assert_eq!(
        rig.session
            .controller()
            .sampler_key_supported(rig.channel(0), 59),
        !spectral
    );
    rig.run(960);
    rig.session.controller().note_on(rig.channel(0), 60, 1.0);
    assert!(
        rig.run(6000).iter().any(|value| value.abs() > 0.01),
        "restored source must reach the runtime plan"
    );
    let export_path = rig.file("restored.wav");
    rig.session
        .export_audio(windfall_ipc::ExportOptions {
            path: export_path.clone(),
            mode: windfall_ipc::PlayMode::Pattern,
            sample_rate: super::SAMPLE_RATE,
            tail_secs: 0.0,
            auto_tail: false,
            bit_depth: windfall_ipc::BitDepth::Float32,
            ..Default::default()
        })
        .unwrap();
    assert!(rig.events.wait_for_export().error.is_none());
    let exported = windfall_codec::decode_file(&export_path).unwrap();
    rig.session.transport_seek(0.0);
    rig.session.transport_play().unwrap();
    let played = rig.run(exported.frames());
    rig.session.transport_stop();
    rig.run(960);
    assert!(played.iter().any(|value| value.abs() > 0.01));
    assert_eq!(played.len(), exported.samples().len());
    let mismatch = played
        .iter()
        .zip(exported.samples())
        .enumerate()
        .find(|(_, (played, exported))| played.to_bits() != exported.to_bits());
    assert!(
        mismatch.is_none(),
        "restored playback/export differ: {mismatch:?}"
    );
    // Once held, another identical replacement must remain a genuine no-op.
    routed_import(&rig, &path, browser, "replacement").unwrap();
    assert_eq!(rig.session.document_snapshot(), before);
    assert_eq!(
        rig.session.state().pool.get(sample).unwrap().identity(),
        restored.identity()
    );
}

#[test]
fn ordinary_same_path_missing_spectral_source_recovers_without_musical_edit() {
    same_path_missing_source_recovery(false, true);
}
#[test]
fn checked_same_path_missing_spectral_source_recovers_without_musical_edit() {
    same_path_missing_source_recovery(true, true);
}
#[test]
fn ordinary_same_path_missing_tape_source_recovers_without_musical_edit() {
    same_path_missing_source_recovery(false, false);
}
#[test]
fn checked_same_path_missing_tape_source_recovers_without_musical_edit() {
    same_path_missing_source_recovery(true, false);
}

fn same_path_missing_source_budget_refusal(browser: bool) {
    use windfall_codec::{WavSampleFormat, write_wav};
    use windfall_core::AudioBuffer;
    let (mut rig, path, sample) = settled_missing_source_fixture(true);
    write_wav(
        &path,
        &AudioBuffer::from_interleaved(48_000, 2, vec![0.75; 48_000 * 2]),
        WavSampleFormat::Float32,
    )
    .unwrap();
    let before = rig.session.document_snapshot();
    let edits = rig.session.state().edits;
    let (pool, loaded, loading, failed) = {
        let state = rig.session.state();
        (
            state.pool.clone(),
            state.loaded.clone(),
            state.loading.clone(),
            state.failed.clone(),
        )
    };
    let result = routed_import(&rig, &path, browser, "replacement");
    assert!(
        result.is_err(),
        "{browser}: over-budget identical missing-source replacement incorrectly returned success"
    );
    assert!(result.unwrap_err().contains("budget"));
    assert_eq!(rig.session.document_snapshot(), before);
    {
        let state = rig.session.state();
        assert!(state.pool.same_sources(&pool));
        assert_eq!(
            state.pool.sampler_retained_bytes(),
            pool.sampler_retained_bytes()
        );
        assert!(!state.pool.contains(sample));
        assert_eq!(state.loaded, loaded);
        assert_eq!(state.loading, loading);
        assert_eq!(state.failed, failed);
        assert_eq!(state.edits, edits);
    }
    rig.run(960);
    rig.session.controller().note_on(rig.channel(0), 60, 1.0);
    assert!(rig.run(6000).iter().all(|value| *value == 0.0));
}

#[test]
fn ordinary_same_path_missing_source_budget_refusal_is_atomic() {
    same_path_missing_source_budget_refusal(false);
}
#[test]
fn checked_same_path_missing_source_budget_refusal_is_atomic() {
    same_path_missing_source_budget_refusal(true);
}

fn successful_file_sampler_replacement(browser: bool) {
    use windfall_codec::{WavSampleFormat, write_wav};
    use windfall_core::AudioBuffer;
    let (mut rig, folder) = prepared_sampler_import_fixture();
    // Isolate the prepared channel for an actual playback/export comparison.
    for channel in rig.project().channels.iter().skip(1) {
        rig.session
            .dispatch(Command::RemoveChannel { id: channel.id }, None)
            .unwrap();
    }
    rig.session
        .dispatch(
            Command::ToggleStep {
                pattern: rig.pattern(),
                channel: rig.channel(0),
                step: 0,
            },
            None,
        )
        .unwrap();
    let file = std::path::Path::new(&folder).join("good.wav");
    write_wav(
        &file,
        &AudioBuffer::from_interleaved(48_000, 1, vec![0.75; 2400]),
        WavSampleFormat::Float32,
    )
    .unwrap();
    let path = paths::display(&file);
    let before = rig.session.document_snapshot();
    let result = routed_import(&rig, &path, browser, "replacement").unwrap();
    let after = rig.session.document_snapshot();
    assert_eq!(after.history.cursor, before.history.cursor + 1);
    let source = after.project.channels[0].source.sample().unwrap();
    assert!(result.created.contains(&source.0));
    assert_eq!(
        rig.session.state().pool.get(source).unwrap().samples()[0],
        0.75
    );
    assert!(
        !rig.session
            .state()
            .pool
            .needs_sampler_preparation(&after.project)
    );
    assert!(
        rig.session
            .controller()
            .sampler_key_supported(rig.channel(0), 60)
    );
    assert!(
        !rig.session
            .controller()
            .sampler_key_supported(rig.channel(0), 59)
    );
    rig.run(960);
    let export_path = rig.file("prepared.wav");
    rig.session
        .export_audio(windfall_ipc::ExportOptions {
            path: export_path.clone(),
            mode: windfall_ipc::PlayMode::Pattern,
            sample_rate: super::SAMPLE_RATE,
            tail_secs: 0.0,
            auto_tail: false,
            bit_depth: windfall_ipc::BitDepth::Float32,
            ..Default::default()
        })
        .unwrap();
    assert!(rig.events.wait_for_export().error.is_none());
    let exported = windfall_codec::decode_file(&export_path).unwrap();
    rig.session.transport_seek(0.0);
    rig.session.transport_play().unwrap();
    let played = rig.run(exported.frames());
    rig.session.transport_stop();
    rig.run(960);
    assert!(played.iter().any(|value| value.abs() > 0.01));
    assert_eq!(played.len(), exported.samples().len());
    let mismatch = played
        .iter()
        .zip(exported.samples())
        .enumerate()
        .find(|(_, (played, exported))| played.to_bits() != exported.to_bits());
    assert!(
        mismatch.is_none(),
        "playback/export must use the same prepared source: {mismatch:?}"
    );
    // Identical replacement is a musical no-op, preserving a real redo tail.
    rig.session
        .dispatch(
            Command::AddChannel {
                name: Some("Keep redo".into()),
                sample: None,
                instrument: None,
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap();
    rig.session.undo().unwrap();
    let unchanged = rig.session.document_snapshot();
    let identity = rig.session.state().pool.get(source).unwrap().identity();
    routed_import(&rig, &path, browser, "replacement").unwrap();
    assert_eq!(rig.session.document_snapshot(), unchanged);
    assert_eq!(
        rig.session.state().pool.get(source).unwrap().identity(),
        identity
    );
    rig.session.undo().unwrap();
    let mut restored = rig.project();
    restored.next_id = before.project.next_id;
    assert_eq!(restored, before.project);
    rig.session.redo().unwrap();
    let mut restored = rig.project();
    restored.next_id = after.project.next_id;
    assert_eq!(restored, after.project);
    assert!(
        !rig.session
            .state()
            .pool
            .needs_sampler_preparation(&restored)
    );
    assert_eq!(
        rig.session.state().pool.get(source).unwrap().identity(),
        identity
    );
    let saved = rig
        .session
        .project_save(Some(&rig.file("prepared.windfall")))
        .unwrap();
    rig.session.project_new().unwrap();
    rig.session.project_open(&saved).unwrap();
    let reopened = rig.session.document_snapshot();
    assert_eq!(
        rig.session.state().pool.get(source).unwrap().samples()[0],
        0.75
    );
    assert!(
        !rig.session
            .state()
            .pool
            .needs_sampler_preparation(&reopened.project)
    );
    routed_import(&rig, &path, browser, "replacement").unwrap();
    assert_eq!(rig.session.document_snapshot(), reopened);
}

#[test]
fn ordinary_file_sampler_replacement_prepares_once_for_playback_export_and_undo() {
    successful_file_sampler_replacement(false);
}
#[test]
fn checked_file_sampler_replacement_prepares_once_for_playback_export_and_undo() {
    successful_file_sampler_replacement(true);
}

#[test]
fn file_import_rechecks_guards_after_off_lock_preparation_at_all_destinations() {
    use windfall_codec::{WavSampleFormat, write_wav};
    use windfall_core::AudioBuffer;
    struct Capture;
    impl crate::session::recording::CaptureHandle for Capture {
        fn frames(&self) -> u64 {
            0
        }
        fn failed(&self) -> bool {
            false
        }
        fn finish(self: Box<Self>) -> Result<u64, String> {
            Ok(0)
        }
    }
    for browser in [false, true] {
        for destination in ["rack", "playlist", "replacement"] {
            for change in [
                "file",
                "edit",
                "New",
                "Open",
                "root",
                "source",
                "pending-load",
                "recording",
                "folder",
            ] {
                if !browser && change == "root" {
                    continue;
                }
                let (rig, folder) = prepared_sampler_import_fixture();
                let file = std::path::Path::new(&folder).join("good.wav");
                write_wav(
                    &file,
                    &AudioBuffer::from_interleaved(48_000, 1, vec![0.75; 2400]),
                    WavSampleFormat::Float32,
                )
                .unwrap();
                let path = paths::display(&file);
                let saved = rig
                    .session
                    .project_save(Some(&rig.file("saved.windfall")))
                    .unwrap();
                let channel = rig.channel(0);
                let token = browser.then(|| rig.session.library_file(&path).unwrap());
                let hold = rig.session.hold("import:prepared");
                let worker = rig
                    .session
                    .background(move |session| match (token, destination) {
                        (Some(token), "rack") => session.browser_add_channel(&path, None, token),
                        (Some(token), "playlist") => session.browser_add_clip(
                            &path,
                            crate::session::ClipPlace {
                                track: None,
                                start: 0,
                                mixer_track: None,
                            },
                            token,
                        ),
                        (Some(token), _) => session.browser_replace_sample(channel, &path, token),
                        (None, "rack") => session.add_channel_from_file(&path, None),
                        (None, "playlist") => session.add_audio_clip_from_file(
                            &path,
                            crate::session::ClipPlace {
                                track: None,
                                start: 0,
                                mixer_track: None,
                            },
                        ),
                        (None, _) => session.set_channel_sample_from_file(channel, &path),
                    });
                hold.wait();
                match change {
                    "file" => write_wav(
                        &file,
                        &AudioBuffer::from_interleaved(48_000, 1, vec![0.5; 3000]),
                        WavSampleFormat::Float32,
                    )
                    .unwrap(),
                    "edit" => {
                        rig.session
                            .dispatch(
                                Command::AddChannel {
                                    name: Some("Concurrent edit".into()),
                                    sample: None,
                                    instrument: None,
                                    index: None,
                                    mixer_track: None,
                                },
                                None,
                            )
                            .unwrap();
                    }
                    "New" => {
                        rig.session.project_new().unwrap();
                    }
                    "Open" => {
                        rig.session.project_open(&saved).unwrap();
                    }
                    "root" => {
                        rig.session.browser_remove_root(&folder).unwrap();
                    }
                    "source" => {
                        let mut state = rig.session.state();
                        let sample = state.document.project().channels[0]
                            .source
                            .sample()
                            .unwrap();
                        state.pool.insert(
                            sample,
                            AudioBuffer::from_interleaved(48_000, 1, vec![0.1; 4800]),
                        );
                    }
                    "pending-load" => {
                        let mut state = rig.session.state();
                        let sample = state.document.project().samples[0].id;
                        state.loading.insert(sample);
                    }
                    "recording" => {
                        rig.session
                            .recording_start_with(
                                windfall_ipc::RecordingSource {
                                    host: "Synthetic".into(),
                                    device: "Synthetic".into(),
                                    left: 0,
                                    right: None,
                                    alignment: None,
                                    loop_recording: None,
                                    monitor: None,
                                    armed_tracks: None,
                                    mixer_tap: None,
                                },
                                0,
                                None,
                                |_, _, _| Ok(Box::new(Capture)),
                            )
                            .unwrap();
                    }
                    "folder" => {
                        rig.session
                            .project_save(Some(&rig.file("Moved/song.windfall")))
                            .unwrap();
                    }
                    _ => unreachable!(),
                }
                let before = rig.session.document_snapshot();
                let sources = rig.session.state().pool.clone();
                hold.release();
                assert!(
                    worker.join().unwrap().is_err(),
                    "{browser}/{destination}/{change} published stale preparation"
                );
                assert_eq!(rig.session.document_snapshot(), before);
                let state = rig.session.state();
                assert_eq!(state.pool.len(), sources.len());
                for (id, audio) in sources.iter() {
                    assert_eq!(state.pool.get(id).unwrap().identity(), audio.identity());
                }
                drop(state);
                if change == "recording" {
                    assert!(rig.session.recording_state().active);
                    rig.session.recording_cancel();
                }
            }
        }
    }
}

// Real reload/redo decodes stop off-lock, before installing their older audio.
fn pending_source_load(destination: &str, redo: bool) {
    use windfall_codec::{WavSampleFormat, write_wav};
    use windfall_core::AudioBuffer;

    let rig = Rig::new();
    let folder = rig.folder.path().join("Samples");
    fs::create_dir(&folder).unwrap();
    let file = folder.join("tone.wav");
    let path = paths::display(&file);
    let write = |frames, level| {
        write_wav(
            &file,
            &AudioBuffer::from_interleaved(48_000, 1, vec![level; frames]),
            WavSampleFormat::Float32,
        )
        .unwrap();
    };
    rig.session
        .browser_add_root(&paths::display(&folder))
        .unwrap();
    let sample = if redo {
        write(480, 0.25);
        let result = rig
            .session
            .browser_add_channel(&path, None, rig.session.library_file(&path).unwrap())
            .unwrap();
        let sample = SampleId(result.created[0]);
        rig.session.undo().unwrap();
        let audio = AudioBuffer::from_interleaved(48_000, 1, vec![0.0; 480]);
        for i in 0..65 {
            let other = folder.join(format!("evict-{i}.wav"));
            write_wav(&other, &audio, WavSampleFormat::Float32).unwrap();
            rig.session.inner.cache.decode(&other).unwrap();
        }
        assert!(rig.session.inner.cache.peek(&file).is_none());
        sample
    } else {
        let result = rig
            .session
            .dispatch(
                Command::AddSample {
                    name: "Missing tone".into(),
                    path: SamplePath::External(path.clone()),
                },
                None,
            )
            .unwrap();
        let sample = SampleId(result.created[0]);
        rig.wait_until_loaded_or_failed(sample);
        assert!(rig.session.state().failed.contains(&sample));
        write(480, 0.25);
        sample
    };
    let hold = rig.session.hold("samples:decoded");
    let reload = if redo {
        rig.session.redo().unwrap();
        None
    } else {
        Some(rig.session.background(|session| session.samples_reload()))
    };
    hold.wait();
    assert!(rig.session.state().loading.contains(&sample));
    assert!(!rig.has_audio(sample));
    write(960, 0.75);
    rig.session.library_refresh();
    let token = rig.session.library_file(&path).unwrap();
    assert_eq!(
        rig.session
            .browser_sample_info(&path, &token)
            .unwrap()
            .frames,
        960
    );
    let before = rig.session.document_snapshot();
    let result = checked_import(&rig, &path, token, destination);
    let after = rig.session.document_snapshot();
    let imported_frames = rig
        .session
        .state()
        .pool
        .get(sample)
        .map(AudioBuffer::frames);
    hold.release();
    if let Some(reload) = reload {
        assert_eq!(reload.join().unwrap(), 0);
    } else {
        // An incorrect import sets loaded before the older pending redo ends.
        let deadline = std::time::Instant::now() + super::PATIENCE;
        while rig.session.state().loading.contains(&sample) {
            assert!(std::time::Instant::now() < deadline, "redo never finished");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    let held = rig.session.state().pool.get(sample).unwrap().clone();
    assert_eq!(held.frames(), 480);
    assert_eq!(held.samples()[0], 0.25);
    assert!(
        result.is_err(),
        "{destination} accepted {imported_frames:?} frames before pending {} installed {} older frames",
        if redo { "redo" } else { "reload" },
        held.frames()
    );
    assert!(result.unwrap_err().contains("still loading"));
    assert_eq!(after, before, "refusal must not mutate history or geometry");
    // Reopen explicitly reloads the current disk source. Normal same-version
    // deduplication and undo then remain available at every destination.
    let saved = rig
        .session
        .project_save(Some(&rig.file("Saved/song")))
        .unwrap();
    rig.session.project_open(&saved).unwrap();
    assert_eq!(rig.session.state().pool.get(sample).unwrap().frames(), 960);
    let before = rig.project();
    checked_import(
        &rig,
        &path,
        rig.session.library_file(&path).unwrap(),
        destination,
    )
    .unwrap();
    assert_eq!(rig.project().samples.len(), before.samples.len());
    rig.session.undo().unwrap();
    let mut current = rig.project();
    current.next_id = before.next_id;
    assert_eq!(current, before);
}

#[test]
fn checked_rack_import_refuses_pending_reload() {
    pending_source_load("rack", false);
}
#[test]
fn checked_playlist_import_refuses_pending_reload() {
    pending_source_load("playlist", false);
}
#[test]
fn checked_replacement_import_refuses_pending_reload() {
    pending_source_load("replacement", false);
}
#[test]
fn checked_rack_import_refuses_pending_redo_after_cache_eviction() {
    pending_source_load("rack", true);
}
#[test]
fn checked_playlist_import_refuses_pending_redo_after_cache_eviction() {
    pending_source_load("playlist", true);
}
#[test]
fn checked_replacement_import_refuses_pending_redo_after_cache_eviction() {
    pending_source_load("replacement", true);
}

#[test]
fn checked_import_refuses_reusing_a_loaded_older_file_version_at_every_destination() {
    use windfall_codec::{WavSampleFormat, write_wav};
    use windfall_core::AudioBuffer;

    for destination in ["rack", "playlist", "replacement"] {
        let rig = Rig::new();
        let samples = rig.folder.path().join("Samples");
        fs::create_dir(&samples).unwrap();
        let file = samples.join("tone.wav");
        let write = |frames, level| {
            write_wav(
                &file,
                &AudioBuffer::from_interleaved(48_000, 1, vec![level; frames]),
                WavSampleFormat::Float32,
            )
            .unwrap();
        };
        write(480, 0.25);
        let path = paths::display(&file);
        rig.session
            .browser_add_root(&paths::display(&samples))
            .unwrap();
        let token = rig.session.library_file(&path).unwrap();
        let first = rig
            .session
            .browser_add_channel(&path, None, token.clone())
            .unwrap();
        let sample = SampleId(first.created[0]);
        let old = rig.session.state().pool.get(sample).unwrap().clone();
        // Unchanged imports must deduplicate and undo as one normal edit.
        rig.session.browser_add_channel(&path, None, token).unwrap();
        assert_eq!(
            rig.project()
                .samples
                .iter()
                .filter(|s| s.id == sample)
                .count(),
            1
        );
        rig.session.undo().unwrap();
        write(960, 0.75);
        rig.session.library_refresh();
        let token = rig.session.library_file(&path).unwrap();
        assert_eq!(
            rig.session
                .browser_sample_info(&path, &token)
                .unwrap()
                .frames,
            960
        );
        rig.session.browser_preview(&path, &token).unwrap();
        let before = rig.session.document_snapshot();
        let result = checked_import(&rig, &path, token, destination);
        assert!(
            result.is_err(),
            "{destination} accepted new audio but retained {} old frames",
            rig.session.state().pool.get(sample).unwrap().frames()
        );
        assert_eq!(rig.session.document_snapshot(), before);
        assert_eq!(
            rig.session.state().pool.get(sample).unwrap().samples(),
            old.samples()
        );
        assert!(result.unwrap_err().contains("older or unverified version"));

        // Saving does not persist an unundoable source replacement. Reopening
        // deliberately reloads linked external audio from its current version.
        let saved = rig
            .session
            .project_save(Some(&rig.file("Saved/song")))
            .unwrap();
        rig.session.project_open(&saved).unwrap();
        assert_eq!(rig.session.state().pool.get(sample).unwrap().frames(), 960);
        assert_eq!(
            rig.session.state().pool.get(sample).unwrap().samples()[0],
            0.75
        );
        let before = rig.project();
        let token = rig.session.library_file(&path).unwrap();
        checked_import(&rig, &path, token, destination).unwrap();
        assert_eq!(rig.project().samples.len(), before.samples.len());
        if destination == "playlist" {
            let clip = rig.project().playlist.clips.last().unwrap().clone();
            let ticks = (0.02 * before.settings.tempo_bpm * f64::from(windfall_core::PPQ) / 60.0)
                .ceil() as u32;
            assert_eq!(clip.length, ticks);
        }
        rig.session.undo().unwrap();
        let mut current = rig.project();
        current.next_id = before.next_id;
        assert_eq!(current, before);
        assert_eq!(rig.session.state().pool.get(sample).unwrap().frames(), 960);
    }
}

#[test]
fn unchanged_file_imports_deduplicate_across_save_after_decoded_cache_eviction() {
    use windfall_codec::{WavSampleFormat, write_wav};
    use windfall_core::AudioBuffer;
    let rig = Rig::new();
    let folder = rig.folder.path().join("Samples");
    fs::create_dir(&folder).unwrap();
    let file = folder.join("tone.wav");
    let audio = AudioBuffer::from_interleaved(48_000, 1, vec![0.25; 480]);
    write_wav(&file, &audio, WavSampleFormat::Float32).unwrap();
    rig.session
        .browser_add_root(&paths::display(&folder))
        .unwrap();
    let path = paths::display(&file);
    let first = rig
        .session
        .browser_add_channel(&path, None, rig.session.library_file(&path).unwrap())
        .unwrap();
    let sample = SampleId(first.created[0]);
    let original = rig.session.state().pool.get(sample).unwrap().identity();
    for i in 0..65 {
        let other = folder.join(format!("{i}.wav"));
        write_wav(&other, &audio, WavSampleFormat::Float32).unwrap();
        rig.session.inner.cache.decode(&other).unwrap();
    }
    assert!(rig.session.inner.cache.peek(&file).is_none());
    // The imported External asset still resolves to the same file after save,
    // although a newly classified import would use a Project-relative path.
    rig.session
        .project_save(Some(&rig.file("dedup.windfall")))
        .unwrap();
    for browser in [false, true] {
        for destination in ["rack", "playlist", "replacement"] {
            let before = rig.project();
            routed_import(&rig, &path, browser, destination).unwrap();
            assert_eq!(rig.project().samples.len(), before.samples.len());
            assert_eq!(
                rig.session.state().pool.get(sample).unwrap().identity(),
                original
            );
            rig.session.undo().unwrap();
            let mut current = rig.project();
            current.next_id = before.next_id;
            assert_eq!(current, before);
        }
    }
}

#[cfg(windows)]
fn unchanged_case_alias_import(browser: bool, destination: &str) {
    use windfall_codec::{WavSampleFormat, write_wav};
    use windfall_core::AudioBuffer;
    for rename in [false, true] {
        let rig = Rig::new();
        let folder = rig.folder.path().join("CaseSource");
        fs::create_dir(&folder).unwrap();
        let file = folder.join("Tone.wav");
        let alias = folder.join("tOnE.WAV");
        let write = |file: &std::path::Path, frames, level| {
            write_wav(
                file,
                &AudioBuffer::from_interleaved(48_000, 1, vec![level; frames]),
                WavSampleFormat::Float32,
            )
            .unwrap();
        };
        write(&file, 480, 0.25);
        rig.session
            .browser_add_root(&paths::display(&folder))
            .unwrap();
        let first = rig
            .session
            .add_channel_from_file(&paths::display(&file), None)
            .unwrap();
        let sample = SampleId(first.created[0]);
        let original = rig.session.state().pool.get(sample).unwrap().identity();
        for i in 0..65 {
            let other = folder.join(format!("evict-{i}.wav"));
            write(&other, 8, 0.1);
            rig.session.inner.cache.decode(&other).unwrap();
        }
        assert!(rig.session.inner.cache.peek(&file).is_none());
        if rename {
            fs::rename(&file, &alias).unwrap();
        }
        rig.session.library_refresh();
        let path = paths::display(&alias);
        let before = rig.project();
        let result = routed_import(&rig, &path, browser, destination);
        assert!(
            result.is_ok(),
            "{browser}/{destination}/rename={rename}: unchanged Windows alias refused: {result:?}"
        );
        assert_eq!(rig.project().samples.len(), before.samples.len());
        assert_eq!(
            rig.session.state().pool.get(sample).unwrap().identity(),
            original
        );
        rig.session.undo().unwrap();
        let mut undone = rig.project();
        undone.next_id = before.next_id;
        assert_eq!(undone, before);
        // Casing equivalence must not weaken genuine source-version checks.
        write(&alias, 960, 0.75);
        rig.session.library_refresh();
        let before = rig.session.document_snapshot();
        assert!(routed_import(&rig, &path, browser, destination).is_err());
        assert_eq!(rig.session.document_snapshot(), before);
        let state = rig.session.state();
        assert_eq!(state.pool.get(sample).unwrap().identity(), original);
        assert_eq!(state.pool.get(sample).unwrap().frames(), 480);
    }
}

#[cfg(windows)]
#[test]
fn ordinary_case_alias_rack_preserves_loaded_version() {
    unchanged_case_alias_import(false, "rack");
}
#[cfg(windows)]
#[test]
fn checked_case_alias_rack_preserves_loaded_version() {
    unchanged_case_alias_import(true, "rack");
}
#[cfg(windows)]
#[test]
fn ordinary_case_alias_playlist_preserves_loaded_version() {
    unchanged_case_alias_import(false, "playlist");
}
#[cfg(windows)]
#[test]
fn checked_case_alias_playlist_preserves_loaded_version() {
    unchanged_case_alias_import(true, "playlist");
}
#[cfg(windows)]
#[test]
fn ordinary_case_alias_replacement_preserves_loaded_version() {
    unchanged_case_alias_import(false, "replacement");
}
#[cfg(windows)]
#[test]
fn checked_case_alias_replacement_preserves_loaded_version() {
    unchanged_case_alias_import(true, "replacement");
}

#[test]
fn checked_imports_keep_native_project_guards_across_new_and_open_at_every_destination() {
    for change in ["New", "Open"] {
        for destination in ["rack", "playlist", "replacement"] {
            let rig = Rig::new();
            let saved = rig
                .session
                .project_save(Some(&rig.file("Saved/song")))
                .unwrap();
            let path = factory_file("Drums/Kicks/Kick Punch.wav");
            let token = rig.session.library_file(&path).unwrap();
            let channel = rig.project().channels[0].id;
            let hold = rig.session.hold("import:decoded");
            let session = rig.session.clone();
            let job = std::thread::spawn(move || match destination {
                "rack" => session.browser_add_channel(&path, None, token),
                "playlist" => session.browser_add_clip(
                    &path,
                    crate::session::ClipPlace {
                        track: None,
                        start: 0,
                        mixer_track: None,
                    },
                    token,
                ),
                _ => session.browser_replace_sample(channel, &path, token),
            });
            hold.wait();
            if change == "New" {
                rig.session.project_new().unwrap();
            } else {
                rig.session.project_open(&saved).unwrap();
            }
            assert_eq!(rig.project().channels[0].id, channel);
            let before = rig.session.document_snapshot();
            drop(hold);
            assert!(job.join().unwrap().is_err());
            assert_eq!(rig.session.document_snapshot(), before);
        }
    }
}

#[test]
fn library_results_audition_import_undo_and_metadata_restart_use_the_existing_loader() {
    let rig = Rig::new();
    let file = factory_file("Drums/Kicks/Kick Punch.wav");
    let token = rig.session.library_file(&file).unwrap();
    let before = rig.project();
    rig.session.browser_sample_info(&file, &token).unwrap();
    rig.session.browser_preview(&file, &token).unwrap();
    let result = rig
        .session
        .browser_add_channel(&file, None, token.clone())
        .unwrap();
    assert_eq!(result.created.len(), 3);
    assert_eq!(rig.project().channels.len(), before.channels.len() + 1);
    rig.session.undo().unwrap();
    let mut current = rig.project();
    current.next_id = before.next_id;
    assert_eq!(current, before);
    let metadata = windfall_ipc::LibraryMetadata {
        favorite: true,
        tags: vec!["punchy".into()],
    };
    rig.session
        .library_set_metadata(&file, metadata.clone())
        .unwrap();
    // Favorites and tags never dirty the project or add history.
    let mut current = rig.project();
    current.next_id = before.next_id;
    assert_eq!(current, before);
    let rig = rig.restart();
    assert_eq!(rig.session.library_metadata(&file).unwrap(), metadata);
    // Cached decoded audio cannot rescue a deleted library file.
    let samples = rig.folder.path().join("Samples");
    fs::create_dir(&samples).unwrap();
    let copied = samples.join("kick.wav");
    fs::copy(&file, &copied).unwrap();
    rig.session
        .browser_add_root(&paths::display(&samples))
        .unwrap();
    let path = paths::display(&copied);
    let token = rig.session.library_file(&path).unwrap();
    rig.session.browser_preview(&path, &token).unwrap();
    fs::remove_file(&copied).unwrap();
    assert!(rig.session.browser_add_channel(&path, None, token).is_err());
    let mut current = rig.project();
    current.next_id = before.next_id;
    assert_eq!(current, before);
}

#[test]
fn root_removal_during_decode_prevents_library_import_even_when_audio_is_cached() {
    let rig = Rig::new();
    let samples = rig.folder.path().join("Samples");
    fs::create_dir(&samples).unwrap();
    let file = samples.join("kick.wav");
    fs::copy(factory_file("Drums/Kicks/Kick Punch.wav"), &file).unwrap();
    let root = paths::display(&samples);
    let path = paths::display(&file);
    rig.session.browser_add_root(&root).unwrap();
    let token = rig.session.library_file(&path).unwrap();
    let before = rig.project();
    let hold = rig.session.hold("import:decoded");
    let session = rig.session.clone();
    let job = std::thread::spawn(move || session.browser_add_channel(&path, None, token));
    hold.wait();
    rig.session.browser_remove_root(&root).unwrap();
    drop(hold);
    assert!(job.join().unwrap().is_err());
    let mut current = rig.project();
    current.next_id = before.next_id;
    assert_eq!(current, before);
}

#[test]
fn guarded_library_clip_and_replace_imports_preserve_document_guards() {
    let rig = Rig::new();
    let file = factory_file("Drums/Kicks/Kick Punch.wav");
    let token = rig.session.library_file(&file).unwrap();
    let channel = rig.project().channels[0].id;
    rig.session
        .browser_replace_sample(channel, &file, token.clone())
        .unwrap();
    let before = rig.project();
    rig.session
        .browser_add_clip(
            &file,
            crate::session::ClipPlace {
                track: None,
                start: 0,
                mixer_track: None,
            },
            token,
        )
        .unwrap();
    assert_eq!(
        rig.project().playlist.clips.len(),
        before.playlist.clips.len() + 1
    );
    rig.session.undo().unwrap();
    let mut current = rig.project();
    current.next_id = before.next_id;
    assert_eq!(current, before);
    let token = rig.session.library_file(&file).unwrap();
    let hold = rig.session.hold("import:decoded");
    let session = rig.session.clone();
    let job = std::thread::spawn(move || session.browser_add_channel(&file, None, token));
    hold.wait();
    rig.session.project_new().unwrap();
    drop(hold);
    assert!(job.join().unwrap().is_err());
}

#[test]
fn checked_library_imports_refuse_an_active_synthetic_take_without_changing_it() {
    struct Capture;
    impl crate::session::recording::CaptureHandle for Capture {
        fn frames(&self) -> u64 {
            0
        }
        fn failed(&self) -> bool {
            false
        }
        fn finish(self: Box<Self>) -> Result<u64, String> {
            Ok(0)
        }
    }
    let rig = Rig::new();
    let file = factory_file("Drums/Kicks/Kick Punch.wav");
    let token = rig.session.library_file(&file).unwrap();
    let before = rig.project();
    let capture = rig
        .session
        .recording_start_with(
            windfall_ipc::RecordingSource {
                host: "Synthetic".into(),
                device: "Synthetic".into(),
                left: 0,
                right: None,
                alignment: None,
                loop_recording: None,
                monitor: None,
                armed_tracks: None,
                mixer_tap: None,
            },
            0,
            None,
            |_, _, _| Ok(Box::new(Capture)),
        )
        .unwrap();
    assert!(
        rig.session
            .browser_add_channel(&file, None, token.clone())
            .is_err()
    );
    assert!(
        rig.session
            .browser_replace_sample(before.channels[0].id, &file, token.clone())
            .is_err()
    );
    assert!(
        rig.session
            .browser_add_clip(
                &file,
                crate::session::ClipPlace {
                    track: None,
                    start: 0,
                    mixer_track: None
                },
                token
            )
            .is_err()
    );
    assert_eq!(rig.project(), before);
    assert_eq!(rig.session.recording_state(), capture);
    rig.session.recording_cancel();
}

#[test]
fn the_factory_content_is_the_first_root_and_lists_as_folders_of_sounds() {
    let rig = Rig::new();
    let roots = rig.session.browser_roots();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].name, "Factory");
    assert_eq!(roots[0].kind, BrowserRootKind::Factory);
    assert_eq!(roots[0].path, paths::display(rig.session.factory_dir()));
    assert_eq!(roots[0].path, factory_file(""));

    let top = rig.session.browser_list(&roots[0].path).unwrap();
    let summary: Vec<(&str, BrowserEntryKind)> = top
        .iter()
        .map(|entry| (entry.name.as_str(), entry.kind))
        .collect();
    assert_eq!(
        summary,
        [
            ("Bass", BrowserEntryKind::Folder),
            ("Drums", BrowserEntryKind::Folder),
            ("Loops", BrowserEntryKind::Folder),
            ("LICENSE.md", BrowserEntryKind::Other),
            ("README.md", BrowserEntryKind::Other),
        ]
    );

    let hats = rig
        .session
        .browser_list(&factory_file("Drums/Hats"))
        .unwrap();
    let names: Vec<&str> = hats.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Hat Closed 1.wav",
            "Hat Closed 2.wav",
            "Hat Closed 3.wav",
            "Hat Open 1.wav",
            "Hat Open 2.wav",
            "Hat Pedal.wav",
        ]
    );
    assert!(
        hats.iter()
            .all(|entry| entry.kind == BrowserEntryKind::Audio)
    );
    // What the browser lists is what the other calls take.
    assert_eq!(hats[0].path, factory_file("Drums/Hats/Hat Closed 1.wav"));
    assert!(rig.session.sample_info(&hats[0].path).is_ok());

    let error = rig.session.browser_list(&rig.file("gone")).unwrap_err();
    assert!(error.contains("does not exist"), "{error}");
    assert!(rig.session.browser_list("Drums").is_err());
}

#[test]
fn user_folders_can_be_added_and_removed_and_are_remembered() {
    let rig = Rig::new();
    let session = &rig.session;
    let kits = rig.folder.path().join("My Kits");
    let loops = rig.folder.path().join("Loops");
    fs::create_dir_all(&kits).unwrap();
    fs::create_dir_all(&loops).unwrap();
    let (kits, loops) = (paths::display(&kits), paths::display(&loops));

    // A trailing separator is not part of the folder's name.
    let roots = session
        .browser_add_root(&format!("{kits}{}", std::path::MAIN_SEPARATOR))
        .unwrap();
    assert_eq!(roots.len(), 2);
    assert_eq!(roots[0].kind, BrowserRootKind::Factory);
    assert_eq!(
        (
            roots[1].name.as_str(),
            roots[1].path.as_str(),
            roots[1].kind
        ),
        ("My Kits", kits.as_str(), BrowserRootKind::User)
    );
    let roots = session.browser_add_root(&loops).unwrap();
    assert_eq!(roots[2].name, "Loops");

    let error = session.browser_add_root(&kits).unwrap_err();
    assert!(error.contains("already in the browser"), "{error}");
    let error = session.browser_add_root(&roots[0].path).unwrap_err();
    assert!(error.contains("already in the browser"), "{error}");
    let error = session.browser_add_root(&rig.file("nowhere")).unwrap_err();
    assert!(error.contains("is not a folder"), "{error}");
    assert_eq!(
        session.browser_add_root(" ").unwrap_err(),
        "Choose a folder to add."
    );

    let error = session.browser_remove_root(&roots[0].path).unwrap_err();
    assert_eq!(error, "The factory library cannot be removed.");
    let error = session
        .browser_remove_root(&rig.file("nowhere"))
        .unwrap_err();
    assert!(error.contains("is not in the browser"), "{error}");
    assert_eq!(session.browser_roots(), roots);

    let rig = rig.restart();
    assert_eq!(rig.session.browser_roots(), roots);
    let left = rig.session.browser_remove_root(&kits).unwrap();
    assert_eq!(left.len(), 2);
    assert_eq!(left[1].path, loops);
    // The folder itself is not touched.
    assert!(std::path::Path::new(&kits).is_dir());
    let rig = rig.restart();
    assert_eq!(rig.session.browser_roots(), left);
}

#[test]
fn sample_info_describes_a_file_with_a_waveform_overview() {
    let rig = Rig::new();
    let file = factory_file("Drums/Kicks/Kick Punch.wav");

    let info = rig.session.sample_info(&file).unwrap();
    assert_eq!(info.path, file);
    assert_eq!(info.name, "Kick Punch");
    assert_eq!(info.sample_rate, windfall_factory::SAMPLE_RATE);
    assert!(info.channels >= 1);
    assert!(info.frames > 1_000);
    let seconds = info.frames as f64 / f64::from(info.sample_rate);
    assert!((info.duration_secs - seconds).abs() < 1e-9);

    assert_eq!(info.peaks.len(), PEAK_BUCKETS * 2);
    let (pairs, _) = info.peaks.as_chunks::<2>();
    assert!(pairs.iter().all(|[low, high]| low <= high));
    // A kick is loud at the start and has died away by the end.
    let height = |pair: &[f32; 2]| pair[1] - pair[0];
    assert!(pairs[..32].iter().map(height).fold(0.0, f32::max) > 0.5);
    assert!(height(&pairs[PEAK_BUCKETS - 1]) < 0.05);

    assert_eq!(rig.session.sample_info(&file).unwrap(), info);

    let error = rig.session.sample_info(&factory_file("Drums")).unwrap_err();
    assert!(error.contains("is a folder"), "{error}");
    let error = rig
        .session
        .sample_info(&factory_file("README.md"))
        .unwrap_err();
    assert!(error.contains("README.md"), "{error}");
    assert!(rig.session.sample_info("Kick Punch.wav").is_err());
}

#[test]
fn sample_info_by_id_resolves_the_samples_of_the_project() {
    let rig = Rig::new();
    let session = &rig.session;
    let project = rig.project();

    // A factory sample is stored relative to the factory folder; the
    // result names the file in full.
    let kick = &project.samples[0];
    assert_eq!(
        kick.path,
        SamplePath::Factory("Drums/Kicks/Kick Punch.wav".to_owned())
    );
    let info = session.sample_info_by_id(kick.id).unwrap();
    let file = factory_file("Drums/Kicks/Kick Punch.wav");
    assert_eq!(info.path, file);
    assert_eq!(info, session.sample_info(&file).unwrap());

    assert_eq!(
        session.sample_info_by_id(SampleId(999)).unwrap_err(),
        "sample 999 does not exist"
    );

    // In the pool, but its file is not there.
    let missing = rig.file("missing.wav");
    let added = session
        .dispatch(
            Command::AddSample {
                name: "Missing".to_owned(),
                path: SamplePath::External(missing),
            },
            None,
        )
        .unwrap();
    let error = session
        .sample_info_by_id(SampleId(added.created[0]))
        .unwrap_err();
    assert!(error.starts_with("Could not load"), "{error}");
    assert!(error.contains("missing.wav"), "{error}");

    // A project sample of a project with no folder cannot be found at all.
    let added = session
        .dispatch(
            Command::AddSample {
                name: "Homeless".to_owned(),
                path: SamplePath::Project("sounds/own.wav".to_owned()),
            },
            None,
        )
        .unwrap();
    assert_eq!(
        session
            .sample_info_by_id(SampleId(added.created[0]))
            .unwrap_err(),
        "Missing sample: sounds/own.wav"
    );
}

#[test]
fn sample_info_for_a_project_that_is_gone_is_not_handed_out() {
    let rig = Rig::new();
    let session = &rig.session;
    let kick = rig.project().samples[0].id;

    let hold = session.hold("sample-info:read");
    let asking = session.background(move |session| session.sample_info_by_id(kick));
    hold.wait();
    // The id means a sample of the new project now.
    session.project_new().unwrap();
    hold.release();
    assert_eq!(asking.join().unwrap().unwrap_err(), PROJECT_REPLACED);

    assert_eq!(session.sample_info_by_id(kick).unwrap().name, "Kick Punch");
}
