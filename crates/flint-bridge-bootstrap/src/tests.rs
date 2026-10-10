use super::*;

#[test]
fn unreadable_plan_reports_failure_to_the_injector() {
    let directory = tempfile::tempdir().unwrap();
    let plan_path = directory.path().join("missing-plan.json");
    let error_path = directory.path().join("startup.error");
    let status = run(BootstrapRequest {
        plan_path: plan_path.clone(),
        error_path: error_path.clone(),
    });
    assert_ne!(status, 0);
    let report = std::fs::read_to_string(error_path).unwrap();
    assert!(report.contains("cannot read attach plan"), "{report}");
    assert!(report.contains(&plan_path.display().to_string()), "{report}");
}
