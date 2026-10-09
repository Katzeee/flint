//! Desktop commands, events, window, and tray.
mod ipc;
#[cfg(feature = "desktop")]
mod runtime;

#[cfg(feature = "desktop")]
pub(crate) use runtime::run;

/// Exports the same command and event registry that the desktop uses at runtime.
#[cfg(feature = "bindings")]
pub fn export_desktop_bindings(path: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
    ipc::bindings().export(specta_typescript::Typescript::default(), path)?;
    Ok(())
}
