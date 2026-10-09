use super::*;
use flint_contracts::protocol::envelope::Payload;
use tokio::sync::mpsc;

fn settings(name: &str, enabled: bool) -> BridgeSettings {
    BridgeSettings {
        address: "127.0.0.1".into(),
        port: 6321,
        name: name.into(),
        enabled,
    }
}

fn lost(message: &str) -> Obstacle {
    Obstacle {
        kind: ObstacleKind::Lost,
        message: message.into(),
    }
}

fn initial() -> Arc<SettingsSnapshot> {
    Arc::new(SettingsSnapshot {
        revision: 0,
        settings: settings("original", true),
    })
}

#[test]
fn settings_change_rejects_a_registration_waiting_to_commit() {
    for (case, replacements) in [
        ("renamed", vec![settings("changed", true)]),
        ("disabled", vec![settings("original", false)]),
        (
            "returned to the original settings",
            vec![settings("changed", true), settings("original", true)],
        ),
    ] {
        let snapshot = initial();
        let mut state = BridgeState::new(snapshot.clone());
        let (updates, latest) = watch::channel(snapshot.clone());
        for replacement in &replacements {
            assert_eq!(
                state.apply_settings(replacement.clone(), &updates),
                ApplyResult::Applied,
                "{case}"
            );
        }
        let replacement = replacements.last().unwrap();
        assert!(
            state.complete_registration(&snapshot, "obsolete".into()).is_err(),
            "{case}: accepted the obsolete registration"
        );
        assert_eq!(
            state.connection,
            if replacement.enabled {
                Connection::Connecting
            } else {
                Connection::Disabled
            },
            "{case}"
        );
        assert!(
            state
                .begin_execution(
                    "obsolete-request".into(),
                    HostExecuteRequest::default(),
                    mpsc::unbounded_channel().0
                )
                .is_none()
        );
        assert!(latest.borrow().settings == *replacement, "{case}");
        assert!(state.settings_snapshot.settings == *replacement, "{case}");
        assert_eq!(state.settings_snapshot.revision, latest.borrow().revision);
        assert!(latest.borrow().revision > snapshot.revision, "{case}");
        if replacement.enabled {
            assert!(
                state.complete_registration(&latest.borrow(), "current".into()).is_ok(),
                "{case}: rejected the current registration"
            );
            assert!(state.connected(), "{case}");
        }
    }
}

#[test]
fn old_session_cleanup_cannot_overwrite_new_settings_or_registration() {
    for enabled in [true, false] {
        let old = initial();
        let mut state = BridgeState::new(old.clone());
        state.complete_registration(&old, "old".into()).unwrap();
        let (updates, latest) = watch::channel(old.clone());
        assert_eq!(
            state.apply_settings(settings("changed", enabled), &updates),
            ApplyResult::Applied
        );
        if enabled {
            state.complete_registration(&latest.borrow(), "current".into()).unwrap();
        }
        let expected = serde_json::to_value(state.status()).unwrap();
        state.finish_session(&old, Some(lost("obsolete")));
        assert_eq!(serde_json::to_value(state.status()).unwrap(), expected);
    }
}

#[test]
fn current_session_failure_keeps_execution_but_does_not_reroute_its_reports() {
    let snapshot = initial();
    let mut state = BridgeState::new(snapshot.clone());
    state.complete_registration(&snapshot, "old".into()).unwrap();
    let (old_outbound, old_received) = mpsc::unbounded_channel();
    let id = state
        .begin_execution("old-request".into(), HostExecuteRequest::default(), old_outbound)
        .unwrap();
    assert!(state.buffer_output(id, "buffered before disconnect", ""));
    state.finish_session(&snapshot, Some(lost("connection lost")));
    assert_eq!(
        state.connection,
        Connection::Retrying {
            obstacle: lost("connection lost")
        }
    );
    assert!(state.busy());
    drop(old_received);

    state.complete_registration(&snapshot, "new".into()).unwrap();
    let (new_outbound, mut new_received) = mpsc::unbounded_channel();
    assert!(
        state
            .begin_execution("overlap".into(), HostExecuteRequest::default(), new_outbound.clone())
            .is_none()
    );
    assert!(state.buffer_output(id, "late output", ""));
    assert!(!state.finish_execution(id, Ok(())), "the old receiver is closed");
    assert!(!state.busy());
    assert!(matches!(new_received.try_recv(), Err(mpsc::error::TryRecvError::Empty)));

    let current = state
        .begin_execution("new-request".into(), HostExecuteRequest::default(), new_outbound)
        .unwrap();
    assert_ne!(current, id);
    assert!(!state.buffer_output(id, "obsolete output", ""));
    assert!(!state.finish_execution(id, Ok(())));
    assert!(state.busy());
    assert!(state.buffer_output(current, "new output", ""));
    assert!(state.finish_execution(current, Ok(())));
    assert!(!state.busy());
    let output = new_received.try_recv().unwrap();
    assert_eq!(output.request_id, "new-request");
    let Some(Payload::ExecutionOutputUpdate(output)) = output.payload else {
        panic!("expected the new execution's output");
    };
    assert_eq!(output.stdout_delta, "new output");
    let result = new_received.try_recv().unwrap();
    assert_eq!(result.request_id, "new-request");
    assert!(matches!(result.payload, Some(Payload::ExecutionResult(_))));
    assert!(matches!(
        new_received.try_recv(),
        Err(mpsc::error::TryRecvError::Disconnected)
    ));
}

#[test]
fn stop_is_terminal_even_when_registration_or_session_cleanup_arrives_late() {
    let snapshot = initial();
    let mut state = BridgeState::new(snapshot.clone());
    let (updates, _) = watch::channel(snapshot.clone());
    state.complete_registration(&snapshot, "current".into()).unwrap();
    let (outbound, _received) = mpsc::unbounded_channel();
    let active = state
        .begin_execution("active".into(), HostExecuteRequest::default(), outbound.clone())
        .unwrap();
    state.stop();
    assert!(state.busy());
    assert!(!state.reconnect());
    assert_eq!(
        state.apply_settings(settings("changed", true), &updates),
        ApplyResult::Stopped
    );
    assert!(state.complete_registration(&snapshot, "late".into()).is_err());
    state.finish_session(&snapshot, Some(lost("late failure")));
    state.finish_session(&snapshot, None);
    assert!(state.stopped());
    assert!(state.instance_id().is_empty());
    assert!(
        state
            .begin_execution("new".into(), HostExecuteRequest::default(), outbound)
            .is_none()
    );
    state.finish_execution(active, Ok(()));
    assert!(!state.busy());
    assert!(state.stopped());
}
