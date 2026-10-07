use super::*;
use crate::host::fake::Fake;
use serde_json::{json, Value};
use std::sync::{atomic::Ordering, Arc};

fn options() -> String {
    json!({"host":"python", "address":"127.0.0.1", "port":6321,
        "name":"test", "runtime_version":"test", "enabled":false})
    .to_string()
}

/// A failed creation as its kind code and message.
type Failure = Option<(u32, String)>;

unsafe fn creation_result(
    options: *const c_char,
    host: *const FlintHost,
) -> (*mut BridgeCore, Failure) {
    let mut kind = u32::MAX;
    let mut error = ptr::null_mut();
    let core = flint_bridge_create(options, host, &mut kind, &mut error);
    let failure = if error.is_null() {
        assert_eq!(kind, 0);
        None
    } else {
        let message = CStr::from_ptr(error).to_str().unwrap().to_owned();
        flint_bridge_string_free(error);
        Some((kind, message))
    };
    (core, failure)
}

fn create(options: &str, fake: &Arc<Fake>) -> (*mut BridgeCore, Failure) {
    let options = CString::new(options).unwrap();
    unsafe { creation_result(options.as_ptr(), &fake.callbacks()) }
}

#[test]
fn production_creation_enforces_the_process_claim_until_destruction() {
    let fake = Fake::new();
    let (first, error) = create(&options(), &fake);
    assert!(error.is_none());
    assert!(!first.is_null());
    let (second, error) = create(&options(), &fake);
    unsafe {
        flint_bridge_destroy(first);
    }
    if !second.is_null() {
        unsafe {
            flint_bridge_destroy(second);
        }
        panic!("a second Bridge owns the same process");
    }
    let (kind, error) = error.expect("claim conflict has an error message");
    assert_eq!(kind, 2);
    assert!(error.contains("Another Bridge already owns this process"));
    assert!(error.contains("host=python"));
    assert!(error.contains("runtime_version=test"));
    assert!(error.contains(&format!("bridge_version={}", env!("CARGO_PKG_VERSION"))));
    let (third, error) = create(&options(), &fake);
    assert!(error.is_none());
    assert!(!third.is_null());
    unsafe {
        flint_bridge_destroy(third);
    }
    assert_eq!(fake.released.load(Ordering::SeqCst), 3);
}

#[test]
fn create_rejects_invalid_configuration_and_releases_the_host() {
    let fake = Fake::new();
    let mut released = 0;
    let mut assert_released = |case: &str| {
        released += 1;
        assert_eq!(fake.released.load(Ordering::SeqCst), released, "{case}");
    };
    let (core, error) = unsafe { creation_result(ptr::null(), &fake.callbacks()) };
    assert!(core.is_null());
    assert!(error.is_some_and(|(kind, message)| kind == 1 && !message.is_empty()));
    assert_released("null configuration");
    let (core, error) = unsafe { creation_result(c"{}".as_ptr(), ptr::null()) };
    assert!(core.is_null());
    assert!(error.unwrap().1.contains("execution host is null"));
    assert_eq!(
        fake.released.load(Ordering::SeqCst),
        1,
        "null host has no registration to release"
    );
    assert!(unsafe {
        flint_bridge_create(
            ptr::null(),
            &fake.callbacks(),
            ptr::null_mut(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert_released("null error outputs");
    let (core, error) = unsafe { creation_result([255u8, 0].as_ptr().cast(), &fake.callbacks()) };
    assert!(core.is_null());
    assert!(error.unwrap().1.contains("not UTF-8"));
    assert_released("invalid UTF-8");
    let valid = options();
    let mut empty_host: Value = serde_json::from_str(&valid).unwrap();
    empty_host["host"] = "".into();
    let mut zero_port: Value = serde_json::from_str(&valid).unwrap();
    zero_port["port"] = 0.into();
    for (case, config) in [
        ("invalid JSON", "not json".to_string()),
        ("empty host", empty_host.to_string()),
        ("zero port", zero_port.to_string()),
    ] {
        let (core, error) = create(&config, &fake);
        assert!(core.is_null(), "accepted {config}");
        assert!(error.is_some_and(|(kind, message)| kind == 1 && !message.is_empty()));
        assert_released(case);
    }
}

#[test]
fn null_core_handles_allow_queries_and_repeated_stop() {
    let core = ptr::null_mut();
    unsafe {
        assert!(flint_bridge_stop(core));
        assert!(!flint_bridge_connected(core));
        assert!(!flint_bridge_busy(core));
        assert!(flint_bridge_instance_id(core).is_null());
        assert!(flint_bridge_stop(core));
    }
}
