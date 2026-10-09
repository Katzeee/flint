//! The Python platform: a host module whose `manager` attaches the Bridge,
//! entered through the platform's `flint_bridge.attach.start`.

use super::PlatformEntry;
use crate::{
    attach::AttachRequest,
    bridge::Attach,
    layout::{join, Layout, CORE, NATIVE_CORE, PYTHON_LIBRARY},
};
use flint_contracts::attach::{CpythonPlan, RuntimePlan};
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

pub(super) fn plan(
    entry: &Entry,
    root: &Path,
    request: &AttachRequest,
    error_path: &Path,
) -> RuntimePlan {
    RuntimePlan::Cpython(CpythonPlan {
        source: source(
            entry.module,
            &root.display().to_string(),
            request,
            error_path,
        ),
    })
}

/// Make the staged library importable and hand the request to the platform's
/// `flint_bridge.attach`. JSON string encoding yields valid Python string literals.
fn source(module: &str, root: &str, request: &AttachRequest, error_path: &Path) -> String {
    let literal = |value: &str| serde_json::to_string(value).unwrap();
    let request = serde_json::json!({
        "module": module,
        "address": request.address,
        "port": request.port,
        "name": request.name,
        "error_path": error_path,
    });
    format!(
        "import sys\n\
         if {root} not in sys.path:\n    sys.path.insert(0, {root})\n\
         from flint_bridge.attach import start\n\
         start({request})\n",
        root = literal(root),
        request = literal(&request.to_string()),
    )
}

#[cfg(test)]
mod tests;
