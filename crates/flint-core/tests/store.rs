use flint_core::store::Store;

#[test]
fn completed_records_survive_restart_and_interrupted_work_is_explicit() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path().into()).unwrap();
    let workflow = store.create("场景".into(), "test".into()).unwrap();
    let first = store
        .append(
            &workflow,
            "host",
            "print(1)".into(),
            "one".into(),
            "request1".into(),
        )
        .unwrap();
    store
        .update(&workflow, &first, |e| {
            e.status = "succeeded".into();
            e.stdout = "1\n".into();
        })
        .unwrap();
    let second = store
        .append(
            &workflow,
            "host",
            "running()".into(),
            "two".into(),
            "request2".into(),
        )
        .unwrap();
    drop(store);
    let store = Store::open(directory.path().into()).unwrap();
    assert_eq!(store.execution(&workflow, &first).unwrap().stdout, "1\n");
    let interrupted = store.execution(&workflow, &second).unwrap();
    assert_eq!(interrupted.status, "failed");
    assert!(interrupted.error.unwrap().contains("unknown"));
    assert!(store.load("../outside").is_err());
}
