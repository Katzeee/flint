use super::*;

fn owner(host: &str) -> ClaimOwner {
    ClaimOwner {
        host: host.into(),
        runtime_version: "3.13".into(),
        bridge_version: "0.1.0".into(),
    }
}

#[test]
fn a_claim_reports_its_owner_and_is_exclusive_until_dropped() {
    let scope = uuid::Uuid::new_v4().simple().to_string();
    let expected = owner("python 场景");
    let ClaimOutcome::Acquired(first) = acquire_for_test(&scope, &expected).unwrap() else {
        panic!("first owner is refused");
    };
    let contender = owner("csharp");
    let ClaimOutcome::Occupied(Some(recorded)) = acquire_for_test(&scope, &contender).unwrap()
    else {
        panic!("second owner is accepted or loses the owner descriptor");
    };
    assert_eq!(recorded, expected);
    drop(first);
    let ClaimOutcome::Acquired(_next) = acquire_for_test(&scope, &contender).unwrap() else {
        panic!("claim is not released");
    };
    let ClaimOutcome::Occupied(Some(recorded)) = acquire_for_test(&scope, &expected).unwrap()
    else {
        panic!("new owner is not recorded");
    };
    assert_eq!(recorded, contender);
}

#[test]
fn missing_or_invalid_diagnostics_do_not_allow_another_owner() {
    let scope = uuid::Uuid::new_v4().simple().to_string();
    let owner = owner("python");
    let ClaimOutcome::Acquired(_claim) = acquire_for_test(&scope, &owner).unwrap() else {
        panic!("first owner is refused");
    };
    let path = directory().join(format!("{scope}.owner"));
    fs::write(&path, b"invalid json").unwrap();
    assert!(matches!(
        acquire_for_test(&scope, &owner).unwrap(),
        ClaimOutcome::Occupied(None)
    ));
    fs::remove_file(&path).unwrap();
    assert!(matches!(
        acquire_for_test(&scope, &owner).unwrap(),
        ClaimOutcome::Occupied(None)
    ));
}
