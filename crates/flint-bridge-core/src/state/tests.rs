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
                ConnectionStatus::Connecting
            } else {
                ConnectionStatus::Disabled
            }
        );
        assert!(state.instance_id().is_empty());
        assert!(state.last_error.is_none());
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
            ending.lock().unwrap().finish_session(
                &old,
                false,
                false,
                Some("obsolete error".into()),
            );
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
    state.finish_session(&snapshot, false, false, Some("connection lost".into()));
    assert!(!state.connected());
    assert!(state.instance_id().is_empty());
    assert!(state.busy());
    assert_eq!(state.last_error.as_deref(), Some("connection lost"));
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
