use super::*;
use windfall_project::{Command, SettingsPatch};
fn edit() -> Command {
    Command::UpdateSettings {
        patch: SettingsPatch {
            name: Some("Frozen edit".into()),
            ..Default::default()
        },
    }
}
#[test]
fn installation_freezes_ui_controller_and_background_admission_and_cancel_restores_history() {
    let rig = Rig::new();
    rig.session
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    name: Some("Before freeze".into()),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    let before = rig.session.document_snapshot();
    let shutdown = rig.session.begin_update_shutdown().unwrap();
    assert!(rig.session.dispatch(edit(), None).is_err());
    assert!(rig.session.dispatch_quiet(edit(), None).is_err());
    assert!(rig.session.project_new().is_err());
    assert!(rig.session.undo().is_none());
    assert_eq!(rig.session.document_snapshot(), before);
    shutdown.rollback().unwrap();
    rig.session.dispatch(edit(), None).unwrap();
    assert_eq!(
        rig.session.document_snapshot().project.settings.name,
        "Frozen edit"
    );
    rig.session.undo().unwrap();
    assert_eq!(rig.session.document_snapshot().project, before.project);
}
#[test]
fn frozen_save_is_exact_and_failed_installer_drop_restarts_the_device_and_edits() {
    let rig = Rig::new();
    rig.session.dispatch(edit(), None).unwrap();
    let mut shutdown = rig.session.begin_update_shutdown().unwrap();
    let target = rig.folder.path().join("update-save.windfall");
    shutdown.save(Some(target.to_str().unwrap())).unwrap();
    assert!(!rig.session.document_snapshot().dirty);
    assert_eq!(
        windfall_project::file::load(&target).unwrap().settings.name,
        "Frozen edit"
    );
    shutdown.quiesce().unwrap();
    assert!(!rig.device.status().running);
    // Same RAII path as a rejected package/installer or lost native worker.
    drop(shutdown);
    assert!(rig.device.status().running);
    rig.session
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    name: Some("Resumed work".into()),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    assert_eq!(
        rig.session.document_snapshot().project.settings.name,
        "Resumed work"
    );
    assert!(rig.session.begin_update_shutdown().is_ok());
}

#[test]
fn prepared_background_and_controller_edits_cannot_cross_the_unsaved_decision_freeze() {
    for controller in [false, true] {
        let rig = Rig::new();
        let hold = rig.session.hold("sampler:prepared");
        let pending = rig.session.background(move |session| {
            if controller {
                session.dispatch_quiet(edit(), None)
            } else {
                session.dispatch(edit(), None)
            }
        });
        hold.wait();
        let before = rig.session.document_snapshot();
        let shutdown = rig.session.begin_update_shutdown().unwrap();
        hold.release();
        assert!(pending.join().unwrap().is_err());
        assert_eq!(rig.session.document_snapshot(), before);
        shutdown.rollback().unwrap();
        rig.session.dispatch(edit(), None).unwrap();
        assert_eq!(
            rig.session.document_snapshot().history.entries.len(),
            before.history.entries.len() + 1
        );
    }
}

#[test]
fn unsaved_decision_waits_for_an_inflight_capture_guard_and_refuses_a_nonanswer() {
    let rig = Rig::new();
    let hold = rig.session.hold("update-test:capturing");
    let capturing = rig.session.background(|session| {
        let _capture = session.recording_idle().unwrap();
        session.pause("update-test:capturing");
    });
    hold.wait();
    let waiting = rig
        .session
        .background(|session| session.begin_update_shutdown());
    assert!(still_running(&waiting));
    hold.release();
    capturing.join().unwrap();
    waiting.join().unwrap().unwrap().rollback().unwrap();

    let hold = rig.session.hold("update-test:capturing");
    let capturing = rig.session.background(|session| {
        let _capture = session.recording_idle().unwrap();
        session.pause("update-test:capturing");
    });
    hold.wait();
    let started = Instant::now();
    let error = match rig.session.begin_update_shutdown() {
        Ok(_) => panic!("unanswered capture admitted installation"),
        Err(error) => error,
    };
    assert!(error.contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(7));
    assert!(
        !rig.session
            .inner
            .update_installing
            .load(std::sync::atomic::Ordering::Acquire)
    );
    assert!(rig.device.status().running);
    hold.release();
    capturing.join().unwrap();
}

#[cfg(windows)]
#[path = "update_shutdown_native.rs"]
mod native;
