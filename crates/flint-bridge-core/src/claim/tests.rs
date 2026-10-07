use super::*;

pub(crate) struct TestScope(String);

impl TestScope {
    pub(crate) fn new() -> Self {
        Self(uuid::Uuid::new_v4().simple().to_string())
    }

    pub(crate) fn name(&self) -> &str {
        &self.0
    }
}

impl Drop for TestScope {
    fn drop(&mut self) {
        for extension in ["lock", "owner"] {
            let path = directory().join(format!("{}.{extension}", self.0));
            if let Err(error) = fs::remove_file(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    if std::thread::panicking() {
                        eprintln!("Cannot remove test claim {}: {error}", path.display());
                    } else {
                        panic!("Cannot remove test claim {}: {error}", path.display());
                    }
                }
            }
        }
    }
}

fn owner(host: &str) -> ClaimOwner {
    ClaimOwner {
        host: host.into(),
        runtime_version: "3.13".into(),
        bridge_version: "0.1.0".into(),
    }
}

#[test]
fn missing_or_invalid_diagnostics_do_not_allow_another_owner() {
    let scope = TestScope::new();
    let owner = owner("python");
    let ClaimOutcome::Acquired(_claim) = acquire_for_test(scope.name(), &owner).unwrap() else {
        panic!("first owner is refused");
    };
    let path = directory().join(format!("{}.owner", scope.name()));
    fs::write(&path, b"invalid json").unwrap();
    assert!(matches!(
        acquire_for_test(scope.name(), &owner).unwrap(),
        ClaimOutcome::Occupied(None)
    ));
    fs::remove_file(&path).unwrap();
    assert!(matches!(
        acquire_for_test(scope.name(), &owner).unwrap(),
        ClaimOutcome::Occupied(None)
    ));
}
