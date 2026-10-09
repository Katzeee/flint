use super::*;

#[test]
fn every_declaration_builds() {
    for host in HostKind::iter() {
        bridge(host);
    }
    for platform in Platform::iter() {
        platform.install();
    }
}

#[test]
fn export_targets_round_trip_through_their_names() {
    for target in HostKind::iter()
        .map(ExportTarget::Host)
        .chain(Platform::iter().map(ExportTarget::Platform))
    {
        assert_eq!(target.to_string().parse::<ExportTarget>().unwrap(), target);
    }
}

#[test]
#[cfg(all(windows, target_arch = "x86_64"))]
fn the_unity_package_manifest_carries_flint_version() {
    let install = bridge(HostKind::Unity).install.unwrap();
    let (_, manifest) = install
        .layout
        .files()
        .find(|(path, _)| *path == "package/package.json")
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(manifest).unwrap();
    assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
}
