use super::Store;

fn record(store: &Store, workflow: &str, code: &str, request_id: &str) -> String {
    store
        .append(
            workflow,
            "host",
            code.into(),
            code.into(),
            request_id.into(),
        )
        .unwrap()
}

#[test]
fn restart_preserves_completed_records_and_marks_interrupted_outcomes_unknown() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().into()).unwrap();
    let workflow = store.create("场景".into(), "test".into()).unwrap();
    let execution = record(&store, &workflow, "print(1)", "request1");
    store
        .update(&workflow, &execution, |e| {
            e.status = "succeeded".into();
            e.stdout = "1\n".into();
        })
        .unwrap();
    let running = record(&store, &workflow, "running()", "request2");
    drop(store);
    let store = Store::open(directory.path().into()).unwrap();
    let restored = store.execution(&workflow, &execution).unwrap();
    assert_eq!(restored.status, "succeeded");
    assert_eq!(restored.stdout, "1\n");
    let interrupted = store.execution(&workflow, &running).unwrap();
    assert_eq!(interrupted.status, "failed");
    assert!(interrupted.error.unwrap().contains("unknown"));
}

#[test]
fn workflow_ids_cannot_escape_the_store() {
    let directory = tempfile::tempdir().unwrap();
    let outside = Store::open(directory.path().into()).unwrap();
    let workflow = outside.create("Outside".into(), "".into()).unwrap();
    let store = Store::open(directory.path().join("store")).unwrap();
    assert!(store.load(&format!("../{workflow}")).is_err());
}

#[test]
fn workflow_index_tracks_updates_and_survives_restart_without_execution_payloads() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().into()).unwrap();
    let first = store.create("First".into(), "".into()).unwrap();
    let second = store.create("Second".into(), "".into()).unwrap();
    let execution = record(&store, &first, "sensitive_source()", "request1");
    assert_eq!(store.list().unwrap()[0].running_count, 1);
    store
        .update(&first, &execution, |e| {
            e.status = "failed".into();
            e.stderr = "failure details".into();
            e.finished_at = Some("2099-01-01T00:00:00+00:00".into());
        })
        .unwrap();
    drop(store);
    let store = Store::open(directory.path().into()).unwrap();
    let index = store.list().unwrap();
    assert_eq!(index.len(), 2);
    assert_eq!(index[0].workflow_id, first);
    assert_eq!(index[0].failed_count, 1);
    assert_eq!(index[0].running_count, 0);
    assert_eq!(index[0].instance_ids, ["host"]);
    assert_eq!(index[1].workflow_id, second);
    assert_eq!(index[1].execution_count, 0);
    let json = serde_json::to_string(&index).unwrap();
    assert!(!json.contains("sensitive_source"));
    assert!(!json.contains("failure details"));
}
