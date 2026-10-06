use super::*;

fn classify(name: &str, args: &[&str]) -> Option<HostKind> {
    host_kind(name, &args.iter().map(OsString::from).collect::<Vec<_>>())
}

#[test]
fn unity_import_workers_are_not_host_candidates() {
    for worker in ["AssetImportWorker0", "AssetImportWorker1"] {
        assert_eq!(
            classify(
                "unity.exe",
                &[
                    "Unity.exe",
                    "-adb2",
                    "-batchMode",
                    "-noUpm",
                    "-name",
                    worker,
                    "-projectPath",
                    "F:/Project"
                ]
            ),
            None
        );
    }
    assert_eq!(
        classify("unity.exe", &["Unity.exe", "-assetImportWorker"]),
        None
    );
    assert_eq!(
        classify("unity.exe", &["Unity.exe", "-NAME", "ASSETIMPORTWORKER2"]),
        None
    );
}

#[test]
fn editors_remain_discoverable_without_a_window_or_command_line() {
    for args in [
        vec![],
        vec!["Unity.exe", "-projectPath", "F:/Project"],
        vec!["Unity.exe", "-adb2", "-batchMode", "-nographics"],
        vec!["Unity.exe", "-projectPath", "F:/AssetImportWorker0"],
    ] {
        assert_eq!(classify("unity.exe", &args), Some(HostKind::Unity));
    }
    assert_eq!(classify("maya.exe", &[]), Some(HostKind::Maya));
    assert_eq!(
        classify("blender.exe", &["blender.exe", "--background"]),
        Some(HostKind::Blender)
    );
    assert_eq!(classify("3dsmax.exe", &[]), Some(HostKind::Max));
    assert_eq!(classify("UnityCrashHandler64.exe", &[]), None);
}
