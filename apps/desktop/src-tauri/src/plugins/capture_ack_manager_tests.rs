//! The same capture/ack queues and status publication used by Manager's worker.
use super::*;

fn request(token: u64, serial: u64) -> crate::plugins::PendingUpdate {
    crate::plugins::PendingUpdate {
        binding: 11,
        token,
        revision: 3,
        update: crate::plugins::Update::Capture {
            target: windfall_project::PluginTarget::Effect {
                effect: windfall_project::EffectId(9),
            },
            serial,
        },
    }
}

fn pending(token: u64, serial: u64, restart: bool, warning: &str) -> CaptureOutcome {
    CaptureOutcome::Accepted {
        restart,
        acknowledgement: CaptureAcknowledgement::Pending {
            ticket: CaptureAckTicket { token, serial },
            warning: warning.into(),
        },
    }
}

#[test]
fn accepted_state_is_never_recaptured_and_retry_never_reemits_restart() {
    let mut updates = CaptureUpdates::default();
    updates.push(request(7, 4));
    let mut captures = 0;
    let progress = updates.capture(|_| {
        captures += 1;
        Ok(pending(7, 5, true, "completion unconfirmed"))
    });
    assert!(progress.restart);
    assert!(progress.error.is_none());
    assert!(updates.has_pending_ack());
    assert!(!updates.is_empty());

    let mut deliveries = 0;
    for _ in 0..2 {
        updates.retry_ack(|ticket| {
            assert_eq!(
                ticket,
                CaptureAckTicket {
                    token: 7,
                    serial: 5
                }
            );
            deliveries += 1;
            Err("owner stopped".into())
        });
        let progress = updates.capture(|_| panic!("accepted bytes were recaptured"));
        assert!(!progress.restart);
        assert!(progress.error.is_none());
        assert_eq!(updates.warning(), Some("owner stopped"));
    }
    updates.retry_ack(|_| {
        deliveries += 1;
        Ok(CaptureAckCompletion::Acknowledged)
    });
    assert_eq!(captures, 1);
    assert_eq!(deliveries, 3);
    assert!(updates.is_empty());
    assert_eq!(updates.warning(), None);
    assert!(!updates.capture(|_| unreachable!()).restart);
}

#[test]
fn precommit_refusal_is_retryable_capture_and_obsolete_has_no_ack_ticket() {
    let mut updates = CaptureUpdates::default();
    updates.push(request(7, 4));
    let progress = updates.capture(|_| Err("recording is active".into()));
    assert_eq!(progress.error.as_deref(), Some("recording is active"));
    assert!(!progress.restart);
    assert!(!updates.is_empty());
    assert!(!updates.has_pending_ack());
    updates.retry_ack(|_| panic!("a refused capture has no postcommit ticket"));
    let progress = updates.capture(|_| Ok(CaptureOutcome::Obsolete));
    assert!(progress.error.is_none());
    assert!(!progress.restart);
    assert!(updates.is_empty());
}

#[test]
fn later_accepted_serial_coalesces_one_ticket_per_owner_without_regression() {
    let mut updates = CaptureUpdates::default();
    for serial in [3, 5, 4, 5] {
        updates.push(request(7, serial));
        updates.capture(|_| Ok(pending(7, serial, false, "retry latest")));
    }
    let mut deliveries = 0;
    updates.retry_ack(|ticket| {
        deliveries += 1;
        assert_eq!(
            ticket,
            CaptureAckTicket {
                token: 7,
                serial: 5
            }
        );
        Err("still waiting".into())
    });
    assert_eq!(deliveries, 1);
    assert!(updates.has_pending_ack());
}

#[test]
fn retired_owner_is_terminal_and_does_not_drop_a_sibling_pending_ticket() {
    let mut updates = CaptureUpdates::default();
    updates.push(request(7, 4));
    updates.push(request(8, 6));
    updates.capture(|request| Ok(pending(request.token, 9, false, "uncertain")));
    updates.retry_ack(|ticket| {
        if ticket.token == 7 {
            Ok(CaptureAckCompletion::Retired)
        } else {
            Err("sibling still waiting".into())
        }
    });
    assert_eq!(updates.warning(), Some("sibling still waiting"));
    let mut deliveries = 0;
    updates.retry_ack(|ticket| {
        deliveries += 1;
        assert_eq!(ticket.token, 8);
        Ok(CaptureAckCompletion::Acknowledged)
    });
    assert_eq!(deliveries, 1);
    assert!(updates.is_empty());
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS hosts plugins on the app's Cocoa main thread, which a unit test does not have"
)]
fn manager_publishes_unconfirmed_ack_warning_until_terminal_completion() {
    let folder = tempfile::tempdir().unwrap();
    let manager = crate::plugins::PluginManager::new(folder.path()).unwrap();
    let mut updates = CaptureUpdates::default();
    updates.push(request(7, 4));
    updates.capture(|_| Ok(pending(7, 5, true, "owner stopped")));
    manager.publish_capture_status(true, &updates);
    assert_eq!(
        manager.state().error.as_deref(),
        Some("Native plugin state was accepted; acknowledgement is pending: owner stopped")
    );
    assert_eq!(manager.status.lock().unwrap().error, None);
    updates.retry_ack(|_| Err("completion timed out".into()));
    manager.publish_capture_status(true, &updates);
    assert!(
        manager
            .state()
            .error
            .as_deref()
            .unwrap()
            .ends_with("completion timed out")
    );
    updates.retry_ack(|_| Ok(CaptureAckCompletion::Retired));
    manager.publish_capture_status(true, &updates);
    assert_eq!(manager.state().error, None);
    manager.status.lock().unwrap().error = Some("Unrelated scanner failure".into());
    manager.publish_capture_status(true, &updates);
    assert_eq!(
        manager.state().error.as_deref(),
        Some("Unrelated scanner failure")
    );
}

#[test]
fn status_snapshot_keeps_ack_warning_visible_despite_other_status_producers() {
    let mut status = windfall_ipc::PluginManagerState {
        error: Some("Scanner changed global status".into()),
        ..Default::default()
    };
    crate::plugins::overlay_capture_ack_warning(&mut status, Some("owner stopped"));
    assert_eq!(
        status.error.as_deref(),
        Some("Native plugin state was accepted; acknowledgement is pending: owner stopped")
    );
    status.error = Some("Scanner is busy".into());
    crate::plugins::overlay_capture_ack_warning(&mut status, None);
    assert_eq!(status.error.as_deref(), Some("Scanner is busy"));
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS hosts plugins on the app's Cocoa main thread, which a unit test does not have"
)]
fn pending_ack_completion_preserves_a_scanner_failure_from_the_pending_interval() {
    let folder = tempfile::tempdir().unwrap();
    let manager = crate::plugins::PluginManager::new(folder.path()).unwrap();
    let mut updates = CaptureUpdates::default();
    updates.push(request(7, 4));
    updates.capture(|_| Ok(pending(7, 5, true, "receipt unconfirmed")));
    manager.publish_capture_status(true, &updates);

    // A scan publishes its result while the accepted capture awaits delivery.
    manager.status.lock().unwrap().error = Some("Scanner executable failed".into());
    manager.publish_capture_status(true, &updates);
    assert_eq!(
        manager.state().error.as_deref(),
        Some("Native plugin state was accepted; acknowledgement is pending: receipt unconfirmed")
    );

    updates.retry_ack(|_| Ok(CaptureAckCompletion::Acknowledged));
    manager.publish_capture_status(true, &updates);
    assert_eq!(
        manager.state().error.as_deref(),
        Some("Scanner executable failed")
    );
    assert_eq!(
        manager.status.lock().unwrap().error.as_deref(),
        Some("Scanner executable failed")
    );
}
