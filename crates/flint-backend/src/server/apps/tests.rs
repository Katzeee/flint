use super::*;

#[test]
fn apps_join_by_instance_identity_rather_than_pid() {
    let registered = |id: &str, pid| App {
        instance_id: id.into(),
        host: "maya".into(),
        pid,
        registration: Some(Registration::default()),
        ..Default::default()
    };
    let candidate = |id: &str, pid| {
        (
            id.into(),
            HostCandidate {
                pid,
                host: HostKind::Maya,
                executable: format!("maya-{pid}.exe"),
            },
        )
    };
    // A remote instance can share a PID with an unrelated local process.
    let apps = join(
        vec![registered("connected", 10), registered("remote", 20)],
        vec![candidate("local", 20), candidate("connected", 10)],
        |_| false,
    );
    let summary: Vec<_> = apps
        .iter()
        .map(|app| {
            (
                app.instance_id.as_str(),
                app.pid,
                app.local,
                app.registration.is_some(),
                app.candidate.as_ref().map(|candidate| candidate.executable.as_str()),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("connected", 10, true, true, Some("maya-10.exe")),
            ("remote", 20, false, true, None),
            ("local", 20, true, false, Some("maya-20.exe")),
        ]
    );
}
