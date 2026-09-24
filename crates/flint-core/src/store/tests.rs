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
fn completed_records_survive_restart() {
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
    drop(store);
    let store = Store::open(directory.path().into()).unwrap();
    let restored = store.execution(&workflow, &execution).unwrap();
    assert_eq!(restored.status, "succeeded");
    assert_eq!(restored.stdout, "1\n");
}

#[test]
fn interrupted_execution_has_unknown_outcome_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().into()).unwrap();
    let workflow = store.create("interrupted".into(), "test".into()).unwrap();
    let execution = record(&store, &workflow, "running()", "request1");
    drop(store);
    let store = Store::open(directory.path().into()).unwrap();
    let interrupted = store.execution(&workflow, &execution).unwrap();
    assert_eq!(interrupted.status, "failed");
    assert!(interrupted.error.unwrap().contains("unknown"));
}

#[test]
fn workflow_ids_cannot_escape_the_store() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().into()).unwrap();
    assert!(store.load("../outside").is_err());
}
