use super::{Store, StoreError};
use flint_contracts::protocol::{ExecutionStatus, Failure, FailureCode};

fn record(store: &Store, workflow: &str, code: &str) -> String {
    store.append(workflow, "host", code.into(), code.into()).unwrap()
}

#[test]
fn restart_preserves_completed_records_and_marks_interrupted_outcomes_unknown() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().into()).unwrap();
    let workflow = store.create("场景".into(), "test".into()).unwrap();
    let execution = record(&store, &workflow, "print(1)");
    store
        .update(&workflow, &execution, |e| {
            e.status = ExecutionStatus::Succeeded;
            e.stdout = "1\n".into();
        })
        .unwrap();
    let failed = record(&store, &workflow, "host_failure()");
    store
        .update(&workflow, &failed, |entry| {
            entry.status = ExecutionStatus::Failed;
            entry.error = Some(Failure {
                code: "custom.host_error".into(),
                message: "具体原因".into(),
            });
            entry.traceback = Some("host stack".into());
        })
        .unwrap();
    let running = record(&store, &workflow, "running()");
    drop(store);
    let store = Store::open(directory.path().into()).unwrap();
    let restored = store.execution(&workflow, &execution).unwrap();
    assert_eq!(restored.status, ExecutionStatus::Succeeded);
    assert_eq!(restored.stdout, "1\n");
    let failed = store.execution(&workflow, &failed).unwrap();
    assert_eq!(failed.status, ExecutionStatus::Failed);
    assert_eq!(failed.error.as_ref().unwrap().code, "custom.host_error");
    assert_eq!(failed.error.unwrap().message, "具体原因");
    assert_eq!(failed.traceback.as_deref(), Some("host stack"));
    let interrupted = store.execution(&workflow, &running).unwrap();
    assert_eq!(interrupted.status, ExecutionStatus::Failed);
    assert!(interrupted.error.unwrap().is(FailureCode::ExecutionInterrupted));
}

#[test]
fn an_unreadable_record_fails_only_the_operations_that_need_it() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().into()).unwrap();
    let readable = store.create("Readable".into(), "".into()).unwrap();
    let template = store.create("Template".into(), "".into()).unwrap();
    record(&store, &template, "print(1)");
    drop(store);
    let template_path = directory.path().join(format!("{template}.json"));
    let template: serde_json::Value = serde_json::from_slice(&std::fs::read(&template_path).unwrap()).unwrap();
    std::fs::remove_file(template_path).unwrap();
    let mut unspecified_status = template.clone();
    unspecified_status["execs"][0]["status"] = "unspecified".into();
    let mut blank_failure = template;
    blank_failure["execs"][0]["error"] = serde_json::json!({"code": " ", "message": "no code"});
    let records = [
        ("corrupt", "not json".to_string()),
        (
            "older",
            r#"{"schema_version":1,"execs":[{"error":"text"}]}"#.to_string(),
        ),
        ("blank_failure", blank_failure.to_string()),
        ("unspecified_status", unspecified_status.to_string()),
    ];
    for (id, bytes) in &records {
        std::fs::write(directory.path().join(format!("{id}.json")), bytes).unwrap();
    }

    let store = Store::open(directory.path().into()).unwrap();
    let listed: Vec<_> = store
        .list()
        .unwrap()
        .into_iter()
        .map(|summary| summary.workflow_id)
        .collect();
    assert_eq!(listed, [readable]);
    for (id, _) in &records {
        let id = *id;
        let error = store.load(id).unwrap_err();
        let StoreError::Unreadable { .. } = &error else {
            panic!("{id}: {error:?}");
        };
        let failure = Failure::from(error);
        assert!(failure.is(FailureCode::WorkflowUnreadable));
        assert!(failure.message.contains(&format!("{id}.json")), "{failure}");
        if id == "older" {
            assert!(failure.message.contains("unsupported schema version 1"), "{failure}");
        }
    }
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
fn workflow_index_tracks_updates_and_survives_restart() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().into()).unwrap();
    let first = store.create("First".into(), "".into()).unwrap();
    let second = store.create("Second".into(), "".into()).unwrap();
    let execution = record(&store, &first, "print(1)");
    assert_eq!(store.list().unwrap()[0].running_count, 1);
    store
        .update(&first, &execution, |e| {
            e.status = ExecutionStatus::Failed;
            e.finished_at = Some("2099-01-01T00:00:00+00:00".into());
        })
        .unwrap();
    for host in ["second-host", "host"] {
        let execution = store.append(&first, host, "print(2)".into(), "".into()).unwrap();
        store
            .update(&first, &execution, |e| e.status = ExecutionStatus::Succeeded)
            .unwrap();
    }
    drop(store);
    let store = Store::open(directory.path().into()).unwrap();
    let index = store.list().unwrap();
    assert_eq!(index.len(), 2);
    assert_eq!(index[0].workflow_id, first);
    assert_eq!(index[0].failed_count, 1);
    assert_eq!(index[0].running_count, 0);
    assert_eq!(index[0].execution_count, 3);
    assert_eq!(index[0].instance_ids, ["host", "second-host"]);
    assert_eq!(index[1].workflow_id, second);
    assert_eq!(index[1].execution_count, 0);
}
