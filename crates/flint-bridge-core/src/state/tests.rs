use super::*;
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
            state
                .complete_registration(&snapshot, "obsolete".into())
                .is_err(),
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
        assert!(state
            .begin_execution(0, "obsolete-request".into(), HostExecuteRequest::default())
            .is_none());
        assert!(latest.borrow().settings == *replacement, "{case}");
        assert!(state.settings_snapshot.settings == *replacement, "{case}");
        assert_eq!(state.settings_snapshot.revision, latest.borrow().revision);
        assert!(latest.borrow().revision > snapshot.revision, "{case}");
        if replacement.enabled {
            assert!(
                state
                    .complete_registration(&latest.borrow(), "current".into())
                    .is_ok(),
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
            state
                .complete_registration(&latest.borrow(), "current".into())
                .unwrap();
        }
        let expected = serde_json::to_value(state.status()).unwrap();
        state.finish_session(&old, Some(lost("obsolete")));
        assert_eq!(serde_json::to_value(state.status()).unwrap(), expected);
    }
}

#[test]
fn current_session_failure_clears_connection_but_preserves_host_execution() {
    let snapshot = initial();
    let mut state = BridgeState::new(snapshot.clone());
    let generation = state
        .complete_registration(&snapshot, "current".into())
        .unwrap();
    let id = state
        .begin_execution(generation, "request".into(), HostExecuteRequest::default())
        .unwrap();
    state.finish_session(&snapshot, Some(lost("connection lost")));
    assert_eq!(
        state.connection,
        Connection::Retrying {
            obstacle: lost("connection lost")
        }
    );
    assert!(state.busy());
    let (outbound, mut received) = mpsc::unbounded_channel();
    assert!(state.finish_execution(id, Ok(()), &outbound));
    assert!(!state.busy());
    assert_eq!(received.try_recv().unwrap().generation, generation);
}

#[test]
fn stop_is_terminal_even_when_registration_or_session_cleanup_arrives_late() {
    let snapshot = initial();
    let mut state = BridgeState::new(snapshot.clone());
    let (updates, _) = watch::channel(snapshot.clone());
    let generation = state
        .complete_registration(&snapshot, "current".into())
        .unwrap();
    let active = state
        .begin_execution(generation, "active".into(), HostExecuteRequest::default())
        .unwrap();
    state.stop();
    assert!(state.busy());
    assert!(!state.reconnect());
    assert_eq!(
        state.apply_settings(settings("changed", true), &updates),
        ApplyResult::Stopped
    );
    assert!(state
        .complete_registration(&snapshot, "late".into())
        .is_err());
    state.finish_session(&snapshot, Some(lost("late failure")));
    state.finish_session(&snapshot, None);
    assert!(state.stopped());
    assert!(state.instance_id().is_empty());
    assert!(state
        .begin_execution(generation, "new".into(), HostExecuteRequest::default())
        .is_none());
    let (outbound, _) = mpsc::unbounded_channel();
    state.finish_execution(active, Ok(()), &outbound);
    assert!(!state.busy());
    assert!(state.stopped());
}
