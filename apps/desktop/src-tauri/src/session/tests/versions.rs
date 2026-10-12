use super::{Rig, rms, still_running};
use std::fs;
use windfall_project::{Command, SettingsPatch, file};
fn rename(rig: &Rig, name: &str) {
    rig.session
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    name: Some(name.into()),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
}
#[test]
fn padding_numbered_stems_collisions_and_older_files_are_preserved() {
    let rig = Rig::new();
    let base = rig
        .session
        .project_save(Some(&rig.file("夜 Song")))
        .unwrap();
    let bytes = fs::read(&base).unwrap();
    fs::write(rig.file("夜 Song (001).windfall"), b"occupied").unwrap();
    let second = rig.session.project_save_new_version(None).unwrap();
    assert!(second.ends_with("夜 Song (002).windfall"));
    assert_eq!(
        rig.session.document_snapshot().path.as_deref(),
        Some(second.as_str())
    );
    let third = rig.session.project_save_new_version(None).unwrap();
    assert!(third.ends_with("夜 Song (003).windfall"));
    assert_eq!(fs::read(base).unwrap(), bytes);
    assert_eq!(
        fs::read(rig.file("夜 Song (001).windfall")).unwrap(),
        b"occupied"
    );
    assert_eq!(file::load(second).unwrap(), file::load(third).unwrap());
}
#[test]
fn edits_and_replacement_while_saving_do_not_mark_other_work_clean() {
    for replacement in [false, true] {
        let rig = Rig::new();
        rig.session.project_save(Some(&rig.file("Song"))).unwrap();
        rename(&rig, "Captured");
        let held = rig.session.hold("save:write");
        let work = rig.session.background(|s| s.project_save_new_version(None));
        held.wait();
        if replacement {
            rig.session.project_new().unwrap();
        }
        rename(&rig, "Concurrent");
        let before = rig.session.document_snapshot();
        held.release();
        let saved = work.join().unwrap().unwrap();
        assert_eq!(file::load(&saved).unwrap().settings.name, "Captured");
        let after = rig.session.document_snapshot();
        assert_eq!(after.project.settings.name, "Concurrent");
        assert!(after.dirty);
        if replacement {
            assert_eq!(after, before);
        } else {
            assert_eq!(after.path.as_deref(), Some(saved.as_str()));
        }
    }
}
#[test]
fn same_session_saves_are_serialized_and_process_competitor_is_not_overwritten() {
    let rig = Rig::new();
    rig.session.project_save(Some(&rig.file("Song"))).unwrap();
    let held = rig.session.hold("save:write");
    let first = rig.session.background(|s| s.project_save_new_version(None));
    held.wait();
    let second = rig.session.background(|s| s.project_save_new_version(None));
    assert!(still_running(&second));
    fs::write(rig.file("Song (001).windfall"), b"competitor").unwrap();
    held.release();
    assert!(
        first
            .join()
            .unwrap()
            .unwrap()
            .ends_with("Song (002).windfall")
    );
    assert!(
        second
            .join()
            .unwrap()
            .unwrap()
            .ends_with("Song (003).windfall")
    );
    assert_eq!(
        fs::read(rig.file("Song (001).windfall")).unwrap(),
        b"competitor"
    );
}
#[test]
fn independent_saving_owners_race_atomic_publication_without_overwrite() {
    let rig = Rig::new();
    let project = rig.project();
    let played = rig.session.played();
    let base = std::path::PathBuf::from(rig.file("Same.windfall"));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let (project, base, barrier) = (project.clone(), base.clone(), barrier.clone());
            std::thread::spawn(move || {
                let (path, written, prepared_path) =
                    crate::session::versions::write(&project, &played, &base, |_, target| {
                        if target.file_name().unwrap() == "Same (001).windfall" {
                            barrier.wait();
                        }
                        Ok(target.to_path_buf())
                    })
                    .unwrap();
                assert_eq!(path, prepared_path);
                assert_eq!(written, project);
                path
            })
        })
        .collect();
    let paths: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_ne!(paths[0], paths[1]);
    for path in paths {
        assert_eq!(file::load(path).unwrap(), project);
    }
}
#[test]
fn recording_waits_for_numbered_save_and_keeps_its_take() {
    let rig = Rig::new();
    let held = rig.session.hold("save:write");
    let base = rig.file("Recording.windfall");
    let work = rig
        .session
        .background(move |session| session.project_save_new_version(Some(&base)));
    held.wait();
    let recording = rig
        .session
        .background(super::archive::start_recording_session);
    assert!(still_running(&recording));
    held.release();
    let saved = work.join().unwrap().unwrap();
    recording.join().unwrap();
    assert_eq!(rig.session.document_snapshot().path, Some(saved.clone()));
    assert!(rig.session.recording_state().active);
    assert_eq!(file::load(saved).unwrap(), rig.project());
    assert!(
        rig.session
            .project_save_new_version(None)
            .unwrap_err()
            .contains("recording")
    );
    rig.session.recording_cancel();
}
#[test]
fn unsaved_base_failed_write_and_carry_samples_use_ordinary_workflow() {
    let rig = Rig::new();
    assert!(rig.session.project_save_new_version(None).is_err());
    let before = rig.session.document_snapshot();
    fs::write(rig.file("blocked"), b"file not folder").unwrap();
    assert!(
        rig.session
            .project_save_new_version(Some(&rig.file("blocked/Song")))
            .is_err()
    );
    assert_eq!(rig.session.document_snapshot(), before);
    let source = rig.file("nested/tone.wav");
    fs::create_dir_all(std::path::Path::new(&source).parent().unwrap()).unwrap();
    windfall_codec::write_wav(
        &source,
        &windfall_core::AudioBuffer::from_interleaved(48000, 2, vec![0.2; 960]),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
    rig.session.project_save(Some(&rig.file("Song"))).unwrap();
    rig.session.add_channel_from_file(&source, None).unwrap();
    let saved = rig
        .session
        .project_save_new_version(Some(&rig.file("new/Song")))
        .unwrap();
    assert!(
        saved.ends_with("new/Song (001).windfall") || saved.ends_with("new\\Song (001).windfall")
    );
    assert_eq!(
        fs::read(rig.file("new/nested/tone.wav")).unwrap(),
        fs::read(source).unwrap()
    );
    rig.session.project_open(&saved).unwrap();
    assert!(rig.has_audio(rig.project().samples.last().unwrap().id));
}

#[test]
fn backup_shaped_numbered_destination_carries_relinks_and_reopens_audio() {
    for concurrent_edit in [false, true] {
        let rig = Rig::new();
        let home = rig
            .session
            .project_save(Some(&rig.file("home/Song")))
            .unwrap();
        let source = rig.file("home/sounds/tone.wav");
        fs::create_dir_all(std::path::Path::new(&source).parent().unwrap()).unwrap();
        windfall_codec::write_wav(
            &source,
            &windfall_core::AudioBuffer::from_interleaved(48000, 2, vec![0.2; 960]),
            windfall_codec::WavSampleFormat::Float32,
        )
        .unwrap();
        let added = rig.session.add_channel_from_file(&source, None).unwrap();
        let sample = windfall_project::SampleId(added.created[0]);
        rig.session
            .dispatch(
                Command::ToggleStep {
                    pattern: rig.pattern(),
                    channel: windfall_project::ChannelId(added.created[1]),
                    step: 0,
                },
                None,
            )
            .unwrap();
        let audio = rig.session.state().pool.get(sample).unwrap().clone();
        rig.session.project_save(None).unwrap();
        let old = fs::read(&home).unwrap();
        rename(&rig, "Backup source");
        let backup = rig
            .session
            .write_backup("2026-10-06 18-12-00")
            .unwrap()
            .unwrap();
        let old_backup = fs::read(&backup).unwrap();
        rig.session
            .project_open(&crate::paths::display(&backup))
            .unwrap();
        rename(&rig, "Captured");
        let base = rig.file("away/Backup/Song 2026-10-06 18-13-05.windfall");
        fs::create_dir_all(std::path::Path::new(&base).parent().unwrap()).unwrap();
        fs::write(&base, b"old backup").unwrap();
        let occupied = rig.file("away/Backup/Song 2026-10-06 18-13-05 (001).windfall");
        fs::write(&occupied, b"older version").unwrap();
        // Exercise relinking too: the final destination holds another sound.
        let collision = rig.file("away/Backup/sounds/tone.wav");
        fs::create_dir_all(std::path::Path::new(&collision).parent().unwrap()).unwrap();
        fs::write(&collision, b"another sound").unwrap();
        let held = rig.session.hold("save:write");
        let work = rig
            .session
            .background(move |s| s.project_save_new_version(Some(&base)));
        held.wait();
        if concurrent_edit {
            rename(&rig, "Concurrent");
        }
        let history = rig.session.document_snapshot().history;
        held.release();
        let saved = work.join().unwrap().unwrap();
        assert!(saved.ends_with("Song 2026-10-06 18-13-05 (002).windfall"));
        let final_root = file::sample_dir(std::path::Path::new(&saved)).unwrap();
        assert_eq!(rig.session.state().sample_dir.as_ref(), Some(&final_root));
        assert_eq!(final_root, rig.folder.path().join("away/Backup"));
        let written = file::load(&saved).unwrap();
        let expected_path = windfall_project::SamplePath::Project("sounds/tone (2).wav".into());
        assert_eq!(written.sample(sample).unwrap().path, expected_path);
        assert_eq!(rig.project().sample(sample).unwrap().path, expected_path);
        assert_eq!(
            fs::read(final_root.join("sounds/tone (2).wav")).unwrap(),
            fs::read(&source).unwrap()
        );
        assert_eq!(fs::read(&home).unwrap(), old);
        assert_eq!(fs::read(&backup).unwrap(), old_backup);
        assert_eq!(fs::read(&occupied).unwrap(), b"older version");
        assert_eq!(
            fs::read(rig.file("away/Backup/Song 2026-10-06 18-13-05.windfall")).unwrap(),
            b"old backup"
        );
        assert_eq!(fs::read(&collision).unwrap(), b"another sound");
        let snapshot = rig.session.document_snapshot();
        assert_eq!(snapshot.history, history);
        assert_eq!(snapshot.dirty, concurrent_edit);
        assert_eq!(written.settings.name, "Captured");
        assert_eq!(
            snapshot.project.settings.name,
            if concurrent_edit {
                "Concurrent"
            } else {
                "Captured"
            }
        );
        assert!(rig.has_audio(sample));
        assert!(rig.session.undo().is_some());
        assert!(rig.session.redo().is_some());
        assert_eq!(rig.project().sample(sample).unwrap().path, expected_path);
        fs::remove_file(&source).unwrap();
        let mut rig = rig.restart();
        rig.session.project_open(&saved).unwrap();
        assert!(rig.has_audio(sample));
        assert_eq!(
            rig.session.state().pool.get(sample).unwrap().samples(),
            audio.samples()
        );
        assert_eq!(rig.session.state().sample_dir.as_ref(), Some(&final_root));
        rig.session.transport_play().unwrap();
        assert!(rms(&rig.run(4800)) > 0.0, "the reopened sampler must play");
    }
}

#[test]
fn numbered_publication_child() {
    let Some(folder) = std::env::var_os("WINDFALL_VERSION_FAULT_ROOT") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    let stage = std::env::var("WINDFALL_VERSION_FAULT_STAGE").unwrap();
    let project = file::load(folder.join("input.windfall")).unwrap();
    let _ = crate::session::versions::write_checked(
        &project,
        &windfall_project::ProjectSession {
            mode: windfall_project::PlayMode::Pattern,
            loop_song: false,
            pattern: None,
        },
        &folder.join("Song.windfall"),
        |_, _| {
            crate::session::files::copy_into_place_checked(
                &folder.join("input-audio.raw"),
                &folder.join("copied-audio.raw"),
                &mut |copied| {
                    if stage == "PartialCopy" && copied >= 64 * 1024 {
                        std::process::exit(73);
                    }
                    Ok(())
                },
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        },
        &mut |boundary| {
            if format!("{boundary:?}") == stage {
                std::process::exit(73);
            }
            Ok(())
        },
    );
    panic!("the requested publication boundary was not reached");
}
#[test]
fn large_numbered_project_survives_real_child_termination_at_every_publication_boundary() {
    use windfall_project::{Document, NoteInit, Project};
    let rig = Rig::new();
    let mut doc = Document::new(rig.project());
    for batch in 0..40 {
        doc.dispatch(
            Command::AddNotes {
                pattern: rig.pattern(),
                channel: rig.channel(0),
                notes: (0..500)
                    .map(|i| NoteInit {
                        start: (i % 16) * 240,
                        length: 120,
                        key: 48 + (batch % 24) as u8,
                        velocity: Some(0.5),
                        pan: Some(0.25),
                        expression: None,
                    })
                    .collect(),
            },
            None,
        )
        .unwrap();
    }
    let project: Project = doc.project().clone();
    let json = file::to_json(&project).unwrap();
    assert!(json.len() > 1_000_000);
    for stage in [
        "Reservation",
        "Copy",
        "PartialCopy",
        "Written",
        "Rename",
        "Published",
    ] {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("input.windfall"), &json).unwrap();
        let audio = vec![7; 1024 * 1024];
        fs::write(root.path().join("input-audio.raw"), &audio).unwrap();
        fs::write(root.path().join("Song.windfall"), b"previous version").unwrap();
        fs::write(
            root.path().join("Song (001).windfall"),
            b"competing process",
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "session::tests::versions::numbered_publication_child",
                "--nocapture",
            ])
            .env("WINDFALL_VERSION_FAULT_ROOT", root.path())
            .env("WINDFALL_VERSION_FAULT_STAGE", stage)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73), "{stage}");
        assert_eq!(
            fs::read(root.path().join("Song.windfall")).unwrap(),
            b"previous version"
        );
        assert_eq!(
            fs::read(root.path().join("Song (001).windfall")).unwrap(),
            b"competing process"
        );
        assert_eq!(
            fs::read(root.path().join("input-audio.raw")).unwrap(),
            audio
        );
        let copied = root.path().join("copied-audio.raw");
        if copied.exists() {
            assert_eq!(fs::read(&copied).unwrap(), audio);
        }
        if stage == "PartialCopy" {
            assert!(!copied.exists());
        }
        let published = root.path().join("Song (002).windfall");
        if stage == "Published" {
            assert_eq!(file::load(&published).unwrap(), project);
        } else {
            assert!(!published.exists(), "{stage}");
        }
        let (saved, actual, ()) = crate::session::versions::write(
            &project,
            &windfall_project::ProjectSession {
                mode: windfall_project::PlayMode::Pattern,
                loop_song: false,
                pattern: None,
            },
            &root.path().join("Song.windfall"),
            |_, _| {
                crate::session::files::copy_into_place_checked(
                    &root.path().join("input-audio.raw"),
                    &copied,
                    &mut |_| Ok(()),
                )
                .map_err(|error| error.to_string())
                .map(|_| ())
            },
        )
        .unwrap();
        assert_eq!(fs::read(copied).unwrap(), audio);
        assert_eq!(actual, project);
        assert_eq!(file::load(saved).unwrap(), project);
    }
}
#[test]
fn injected_numbered_io_faults_never_expose_partial_projects_or_change_the_snapshot() {
    use crate::session::versions::{Publication, write_checked};
    let rig = Rig::new();
    let original = rig.project();
    for boundary in [
        Publication::Reservation,
        Publication::Copy,
        Publication::Written,
        Publication::Rename,
    ] {
        let root = tempfile::tempdir().unwrap();
        let result = write_checked(
            &original,
            &windfall_project::ProjectSession {
                mode: windfall_project::PlayMode::Pattern,
                loop_song: false,
                pattern: None,
            },
            &root.path().join("Song.windfall"),
            |_, _| Ok(()),
            &mut |stage| {
                if stage == boundary {
                    Err("injected disk failure".into())
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.unwrap_err().contains("injected"));
        assert!(!root.path().join("Song (001).windfall").exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        assert_eq!(rig.project(), original);
    }
}

#[test]
fn interrupted_production_sample_copy_preserves_source_and_final_names() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.raw");
    let target = root.path().join("target.raw");
    let audio = vec![0x53; 1024 * 1024];
    fs::write(&source, &audio).unwrap();
    for existing in [false, true] {
        if existing {
            fs::write(&target, b"competitor").unwrap();
        }
        let failed =
            crate::session::files::copy_into_place_checked(&source, &target, &mut |copied| {
                if copied >= 64 * 1024 {
                    Err(std::io::Error::other("injected copy fault"))
                } else {
                    Ok(())
                }
            });
        assert!(failed.unwrap_err().to_string().contains("injected"));
        assert_eq!(fs::read(&source).unwrap(), audio);
        if existing {
            assert_eq!(fs::read(&target).unwrap(), b"competitor");
        } else {
            assert!(!target.exists());
        }
        assert_eq!(
            fs::read_dir(root.path()).unwrap().count(),
            if existing { 2 } else { 1 }
        );
    }
}
