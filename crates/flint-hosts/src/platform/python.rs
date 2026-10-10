//! The Python platform: a host module whose `manager` attaches the Bridge,
//! entered through the platform's `flint_bridge.attach.start`.

use super::PlatformEntry;
use crate::{
    bridge::Attach,
    layout::{CORE, Layout, NATIVE_CORE, PYTHON_LIBRARY, join},
};
use flint_contracts::{
    attach::{CpythonPlan, RuntimePlan},
    host_settings::HostSettings,
};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Entry {
    pub module: &'static str,
}

pub(super) fn library(root: &str) -> Layout {
    Layout::default()
        .tree(root, PYTHON_LIBRARY)
        .file(&join(root, &format!("flint_bridge/{CORE}")), NATIVE_CORE)
}

/// Attach imports `module` from the Python library staged as the import root.
pub(crate) fn attach(module: &'static str) -> Attach {
    Attach {
        layout: library(""),
        entry: PlatformEntry::Python(Entry { module }),
    }
}

pub(super) fn plan(entry: &Entry, root: &Path, settings: HostSettings) -> RuntimePlan {
    RuntimePlan::Cpython(CpythonPlan {
        import_root: root.to_path_buf(),
        module: entry.module.into(),
        settings,
    })
}
