//! Invoked by cargo codegen after the protocol bindings are generated.
fn main() -> anyhow::Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("expected the output path"))?;
    flint::export_desktop_bindings(path)
}
