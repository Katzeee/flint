use super::dcc::verify_host;
use anyhow::Result;

#[test]
#[ignore = "real host: set FLINT_MAYA_EXE and run `cargo xtask test hosts`"]
fn exported_package_registers_and_executes() -> Result<()> {
    verify_host(
        "maya",
        "FLINT_MAYA_EXE",
        "maya.exe",
        include_str!("../../fixtures/maya_scene.py"),
    )
}
