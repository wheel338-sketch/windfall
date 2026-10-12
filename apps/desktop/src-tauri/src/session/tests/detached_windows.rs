use super::{Rig, lock};
use crate::events::Event;
use std::sync::{Arc, Barrier, Mutex};
use windfall_project::Command;

#[test]
fn two_windows_racing_edits_share_one_document_and_ordered_revision_stream() {
    let rig = Rig::new();
    let barrier = Arc::new(Barrier::new(3));
    let generation = rig.session.window_document_snapshot().generation;
    let threads = (0..2)
        .map(|index| {
            let session = rig.session.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let command = Command::AddPattern {
                    name: Some(format!("Window {index}")),
                };
                // Optimistic preparation may refuse a concurrent edit. Retry
                // that explicit refusal against the same document identity.
                session
                    .dispatch_from_window(generation, command.clone(), None)
                    .or_else(|_| session.dispatch_from_window(generation, command, None))
                    .unwrap()
                    .patch
                    .revision
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let mut revisions = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    revisions.sort_unstable();
    assert_eq!(revisions, vec![1, 2]);
    let patches = rig
        .events
        .take()
        .into_iter()
        .filter_map(|event| match event {
            Event::ProjectPatch(patch) => Some(patch.revision),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(patches, vec![1, 2]);
    let snapshot = rig.session.document_snapshot();
    assert_eq!(snapshot.history.entries.len(), 2);
    assert!(
        snapshot
            .project
            .patterns
            .iter()
            .any(|pattern| pattern.name == "Window 0")
    );
    assert!(
        snapshot
            .project
            .patterns
            .iter()
            .any(|pattern| pattern.name == "Window 1")
    );
}

#[test]
fn stale_window_dispatch_and_history_cannot_touch_a_replacement_document() {
    let rig = Rig::new();
    let old = rig.session.window_document_snapshot();
    rig.session.project_new().unwrap();
    let fresh = rig.session.window_document_snapshot();
    assert_ne!(old.generation, fresh.generation);
    let command = Command::AddPattern {
        name: Some("Delayed old window".into()),
    };
    assert!(
        rig.session
            .dispatch_from_window(old.generation, command, None)
            .is_err()
    );
    assert!(rig.session.undo_from_window(old.generation).is_err());
    assert!(rig.session.redo_from_window(old.generation).is_err());
    assert!(
        rig.session
            .history_jump_from_window(old.generation, 0)
            .is_err()
    );
    assert_eq!(rig.session.document_snapshot(), fresh.snapshot);
    assert_eq!(rig.session.window_document_generation(), fresh.generation);
}

#[test]
fn prepared_window_edit_loses_admission_when_the_project_is_replaced() {
    let rig = Rig::new();
    let generation = rig.session.window_document_snapshot().generation;
    let hold = rig.session.hold("sampler:prepared");
    let editing = rig.session.background(move |session| {
        session.dispatch_from_window(
            generation,
            Command::AddPattern {
                name: Some("Held window".into()),
            },
            None,
        )
    });
    hold.wait();
    let fresh = rig.session.project_new().unwrap();
    rig.events.take();
    hold.release();
    assert!(editing.join().unwrap().is_err());
    assert_eq!(rig.session.document_snapshot(), fresh);
    assert!(rig.events.take().is_empty());
}

#[test]
fn close_reopen_stale_cleanup_and_replacement_release_realtime_owners() {
    let rig = Rig::new();
    let sent = Arc::new(Mutex::new(Vec::new()));
    let sender = |tag| {
        let sent = sent.clone();
        Box::new(move |_: &windfall_ipc::RealtimeFrame| {
            lock(&sent).push(tag);
            true
        }) as super::super::realtime::FrameSender
    };
    rig.session
        .subscribe_realtime_owned("panel-mixer", Some("old".into()), sender(1));
    rig.session.realtime_tick();
    rig.session
        .subscribe_realtime_owned("panel-mixer", Some("new".into()), sender(2));
    rig.session.unsubscribe_realtime_owned("panel-mixer", "old");
    rig.session.project_new().unwrap();
    rig.session.realtime_tick();
    assert_eq!(*lock(&sent), vec![1, 2]);
    rig.session.unsubscribe_realtime_owned("panel-mixer", "new");
    rig.session.realtime_tick();
    assert_eq!(*lock(&sent), vec![1, 2]);
    rig.session
        .subscribe_realtime_owned("panel-mixer", Some("reopened".into()), sender(3));
    rig.session.realtime_tick();
    rig.session.unsubscribe_realtime("panel-mixer");
    rig.session.realtime_tick();
    assert_eq!(*lock(&sent), vec![1, 2, 3]);
}
