use super::dcc::verify_host;
use anyhow::Result;

#[test]
#[ignore = "real host: set FLINT_MAX_EXE and run `cargo xtask test hosts`"]
fn exported_package_registers_and_executes() -> Result<()> {
    verify_host(
        "max",
        "FLINT_MAX_EXE",
        "3dsmax.exe",
        include_str!("../../fixtures/max_scene.py"),
    )
}
