use super::*;

fn config() -> AttachConfig {
    AttachConfig {
        runtime: "cpython".into(),
        host: "maya".into(),
        address: "127.0.0.1".into(),
        port: 6321,
        name: "Injected Maya".into(),
        payload: r"C:\tools\flint-python.zip".into(),
        core_path: None,
    }
}

#[test]
fn python_bootstrap_starts_attach_on_a_daemon_thread() {
    let source = python_bootstrap(&config(), Path::new(r"C:\temp\42.error"));
    // A Windows path must survive as a literal, and the connection must run off
    // the injected thread so it never blocks holding the GIL.
    assert!(source.contains(r#""C:\\tools\\flint-python.zip""#));
    assert!(source.contains("flint_bridge.attach(host=\"maya\""));
    assert!(source.contains("port=6321"));
    assert!(source.contains("daemon=True).start()"));
    // A failure on that thread is reported where the injector reads it.
    assert!(source.contains(r#"open("C:\\temp\\42.error", 'w'"#));
}

#[test]
fn python_bootstrap_quotes_unusual_names_safely() {
    let mut config = config();
    config.name = "a\"b\\c\n场景".into();
    let source = python_bootstrap(&config, Path::new("42.error"));
    // JSON-encoded fields are valid Python string literals, so the quote,
    // backslash, and newline are escaped rather than breaking the source.
    assert!(source.contains(r#"name="a\"b\\c\n场景""#));
    assert!(source.lines().count() >= 5);
}

#[test]
fn unknown_runtime_is_reported() {
    let mut config = config();
    config.runtime = "ruby".into();
    assert_eq!(
        start(&config, Path::new("42.error")).unwrap_err(),
        "unknown attach runtime: ruby"
    );
}
