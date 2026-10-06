use super::*;
use std::{
    sync::{mpsc as sync_mpsc, Mutex},
    thread,
    time::Duration,
};

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
    for replacement in [settings("changed", true), settings("original", false)] {
        let snapshot = initial();
        let state = Arc::new(Mutex::new(State::new(snapshot.clone())));
        let (updates, latest) = watch::channel(snapshot.clone());
        let (ready_tx, ready_rx) = sync_mpsc::channel();
        let (resume_tx, resume_rx) = sync_mpsc::channel();
        let registering = state.clone();
        let old = snapshot.clone();
        let registration = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            resume_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            registering
                .lock()
                .unwrap()
                .complete_registration(&old, "obsolete".into())
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let applied = state
            .lock()
            .unwrap()
            .apply_settings(replacement.clone(), &updates);
        resume_tx.send(()).unwrap();
        assert_eq!(applied, ApplyResult::Applied);
        assert!(registration.join().unwrap().is_err());

        let mut state = state.lock().unwrap();
        assert_eq!(
            state.connection,
            if replacement.enabled {
                Connection::Connecting
            } else {
                Connection::Disabled
            }
        );
        assert!(state
            .begin_execution(0, "obsolete-request".into(), HostExecuteRequest::default())
            .is_none());
        assert!(Arc::ptr_eq(&state.settings_snapshot, &latest.borrow()));
        if replacement.enabled {
            assert!(state
                .complete_registration(&latest.borrow(), "current".into())
                .is_ok());
            assert!(state.connected());
        }
    }
}

#[test]
fn old_session_cleanup_cannot_overwrite_new_settings_or_registration() {
    for enabled in [true, false] {
        let old = initial();
        let state = Arc::new(Mutex::new(State::new(old.clone())));
        state
            .lock()
            .unwrap()
            .complete_registration(&old, "old".into())
            .unwrap();
        let (updates, latest) = watch::channel(old.clone());
        let (ready_tx, ready_rx) = sync_mpsc::channel();
        let (resume_tx, resume_rx) = sync_mpsc::channel();
        let ending = state.clone();
        let cleanup = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            resume_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            ending
                .lock()
                .unwrap()
                .finish_session(&old, Some(lost("obsolete")));
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let expected = {
            let mut state = state.lock().unwrap();
            assert_eq!(
                state.apply_settings(settings("changed", enabled), &updates),
                ApplyResult::Applied
            );
            if enabled {
                state
                    .complete_registration(&latest.borrow(), "current".into())
                    .unwrap();
            }
            serde_json::to_value(state.status()).unwrap()
        };
        resume_tx.send(()).unwrap();
        cleanup.join().unwrap();
        assert_eq!(
            serde_json::to_value(state.lock().unwrap().status()).unwrap(),
            expected
        );
    }
}

#[test]
fn returning_to_the_same_settings_does_not_revive_an_old_registration() {
    let old = initial();
    let mut state = State::new(old.clone());
    let (updates, latest) = watch::channel(old.clone());
    state.apply_settings(settings("changed", true), &updates);
    state.apply_settings(old.settings.clone(), &updates);
    assert!(state
        .complete_registration(&old, "obsolete".into())
        .is_err());
    assert!(state
        .complete_registration(&latest.borrow(), "current".into())
        .is_ok());
}

#[test]
fn current_session_failure_clears_connection_but_preserves_host_execution() {
    let snapshot = initial();
    let mut state = State::new(snapshot.clone());
    let generation = state
        .complete_registration(&snapshot, "current".into())
        .unwrap();
    assert!(state
        .begin_execution(generation, "request".into(), HostExecuteRequest::default())
        .is_some());
    state.finish_session(&snapshot, Some(lost("connection lost")));
    assert_eq!(
        state.connection,
        Connection::Retrying {
            obstacle: lost("connection lost")
        }
    );
    assert!(state.busy());
    let (outbound, mut received) = mpsc::unbounded_channel();
    assert!(state.report_execution(
        ExecutionReport::Result {
            request_id: "request".into(),
            succeeded: true,
            traceback: None,
            error: None,
        },
        &outbound
    ));
    assert!(!state.busy());
    assert_eq!(received.try_recv().unwrap().generation, generation);
}

#[test]
fn stop_is_terminal_even_when_registration_or_session_cleanup_arrives_late() {
    let snapshot = initial();
    let mut state = State::new(snapshot.clone());
    let (updates, _) = watch::channel(snapshot.clone());
    let generation = state
        .complete_registration(&snapshot, "current".into())
        .unwrap();
    state
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
    state.report_execution(
        ExecutionReport::Result {
            request_id: "active".into(),
            succeeded: true,
            traceback: None,
            error: None,
        },
        &outbound,
    );
    assert!(!state.busy());
    assert!(state.stopped());
}
