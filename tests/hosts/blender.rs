use super::dcc::verify_host;
use anyhow::Result;

#[test]
#[ignore = "real host: set FLINT_BLENDER_EXE and run `cargo xtask test hosts`"]
fn blender_active_connection() -> Result<()> {
    verify_host(
        "blender",
        "FLINT_BLENDER_EXE",
        "blender.exe",
        include_str!("../fixtures/blender_scene.py"),
    )
}
