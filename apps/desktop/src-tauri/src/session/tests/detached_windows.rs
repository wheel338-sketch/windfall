use super::{lock, Rig};
use crate::events::Event;
use std::sync::{Arc, Barrier, Mutex};
use windfall_project::Command;

#[test]
fn two_windows_racing_edits_share_one_document_and_ordered_revision_stream() {
    let rig = Rig::new();
    let barrier = Arc::new(Barrier::new(3));
    let threads = (0..2)
        .map(|index| {
            let session = rig.session.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                session
                    .dispatch(
                        Command::AddPattern {
                            name: Some(format!("Window {index}")),
                        },
                        None,
                    )
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
    assert!(snapshot
        .project
        .patterns
        .iter()
        .any(|pattern| pattern.name == "Window 0"));
    assert!(snapshot
        .project
        .patterns
        .iter()
        .any(|pattern| pattern.name == "Window 1"));
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
