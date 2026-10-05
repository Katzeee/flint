use super::*;
use serde_json::{json, Value};

fn options() -> String {
    json!({"host":"python", "address":"127.0.0.1", "port":6321,
        "name":"test", "runtime_version":"test", "enabled":false})
    .to_string()
}

unsafe fn creation_result(options: *const c_char) -> (*mut BridgeCore, Option<String>) {
    let mut error = ptr::null_mut();
    let core = flint_bridge_create(options, &mut error);
    let message = if error.is_null() {
        None
    } else {
        let message = CStr::from_ptr(error).to_str().unwrap().to_owned();
        flint_bridge_string_free(error);
        Some(message)
    };
    (core, message)
}

fn create(options: &str) -> (*mut BridgeCore, Option<String>) {
    let options = CString::new(options).unwrap();
    unsafe { creation_result(options.as_ptr()) }
}

#[test]
fn production_creation_enforces_the_process_claim_until_destruction() {
    let (first, error) = create(&options());
    assert!(error.is_none());
    assert!(!first.is_null());
    let (second, error) = create(&options());
    unsafe {
        flint_bridge_destroy(first);
    }
    if !second.is_null() {
        unsafe {
            flint_bridge_destroy(second);
        }
        panic!("a second Bridge owns the same process");
    }
    let error = error.expect("claim conflict has an error message");
    assert!(error.contains("another Bridge already owns this process"));
    assert!(error.contains("host=python"));
    assert!(error.contains("runtime_version=test"));
    assert!(error.contains(&format!("bridge_version={}", env!("CARGO_PKG_VERSION"))));
    let (third, error) = create(&options());
    assert!(error.is_none());
    assert!(!third.is_null());
    unsafe {
        flint_bridge_destroy(third);
    }
}

#[test]
fn create_rejects_invalid_configuration() {
    let (core, error) = unsafe { creation_result(ptr::null()) };
    assert!(core.is_null());
    assert_eq!(error.as_deref(), Some("Bridge configuration is null"));
    assert!(unsafe { flint_bridge_create(ptr::null(), ptr::null_mut()) }.is_null());
    let (core, error) = unsafe { creation_result([255u8, 0].as_ptr().cast()) };
    assert!(core.is_null());
    assert!(error.unwrap().contains("not UTF-8"));
    let (core, error) = create("{}");
    assert!(core.is_null());
    assert!(error.unwrap().contains("missing field `host`"));
    let valid = options();
    let mut unknown: Value = serde_json::from_str(&valid).unwrap();
    unknown["extra"] = true.into();
    let mut claim_override: Value = serde_json::from_str(&valid).unwrap();
    claim_override["claim_id"] = "different-process".into();
    let mut empty_host = unknown.clone();
    empty_host.as_object_mut().unwrap().remove("extra");
    empty_host["host"] = "".into();
    let mut zero_port = empty_host.clone();
    zero_port["host"] = "python".into();
    zero_port["port"] = 0.into();
    for config in [
        "not json".to_string(),
        unknown.to_string(),
        claim_override.to_string(),
        empty_host.to_string(),
        zero_port.to_string(),
    ] {
        let (core, error) = create(&config);
        assert!(core.is_null(), "accepted {config}");
        assert!(error.is_some_and(|message| !message.is_empty()));
    }
}

#[test]
fn null_handles_are_ignored() {
    let core = ptr::null_mut();
    unsafe {
        assert!(flint_bridge_poll(core, 0).is_null());
        assert!(!flint_bridge_report_execution(core, c"{}".as_ptr()));
        assert!(!flint_bridge_connected(core));
        assert!(!flint_bridge_busy(core));
        assert!(flint_bridge_instance_id(core).is_null());
        assert!(flint_bridge_status_json(core).is_null());
        flint_bridge_reconnect(core);
        assert_eq!(
            flint_bridge_apply_settings(core, c"{}".as_ptr()),
            ApplyResult::Invalid as u32
        );
        flint_bridge_stop(core);
        flint_bridge_destroy(core);
        flint_bridge_string_free(ptr::null_mut());
    }
}
