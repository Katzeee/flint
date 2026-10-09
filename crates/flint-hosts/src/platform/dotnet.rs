//! The .NET platform: a host type whose static `Manager` attaches the Bridge,
//! entered through the platform's `Flint.Bridge.Attach.Initialize`.

use super::PlatformEntry;
use crate::{
    attach::AttachRequest,
    bridge::Attach,
    layout::{CORE, DOTNET_BINDING, Layout, NATIVE_CORE, join},
};
use flint_contracts::attach::{MonoPlan, RuntimePlan};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Entry {
    /// The host assembly, compiled with the platform binding.
    pub assembly: &'static str,
    /// The host type exposing the static `Manager`.
    pub manager: &'static str,
    /// Preferred Mono scripting domain.
    pub domain: &'static str,
}

pub(super) fn library(root: &str) -> Layout {
    Layout::default()
        .tree(root, DOTNET_BINDING)
        .file(&join(root, CORE), NATIVE_CORE)
}

/// Attach loads the host assembly with the native core beside it.
pub(crate) fn attach(entry: Entry, assembly: &'static [u8]) -> Attach {
    Attach {
        layout: Layout::default().file(entry.assembly, assembly).file(CORE, NATIVE_CORE),
        entry: PlatformEntry::Dotnet(entry),
    }
}

pub(super) fn plan(entry: &Entry, root: &Path, request: &AttachRequest) -> RuntimePlan {
    RuntimePlan::Mono(MonoPlan {
        assembly: root.join(entry.assembly).display().to_string(),
        domain: entry.domain.into(),
        namespace: "Flint.Bridge".into(),
        class: "Attach".into(),
        method: "Initialize".into(),
        argument: serde_json::json!({
            "manager": entry.manager,
            "native_library": root.join(CORE),
            "address": request.address,
            "port": request.port,
            "name": request.name,
        })
        .to_string(),
    })
}
