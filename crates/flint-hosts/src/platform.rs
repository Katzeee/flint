//! Language platforms that hosts build their Bridges on.

pub(crate) mod dotnet;
pub(crate) mod python;

use crate::{bridge::Install, layout::Layout};
use flint_contracts::{attach::RuntimePlan, host_settings::HostSettings};
use std::path::Path;
use strum::{EnumDiscriminants, EnumIter, EnumString, IntoStaticStr};

/// How a host enters its platform on attach. Declares [`Platform`], so every
/// platform has an attach entry.
#[derive(EnumDiscriminants)]
#[strum_discriminants(
    name(Platform),
    vis(pub),
    doc = "A language platform, named as `flint bridge export` accepts it.",
    derive(EnumIter, EnumString, IntoStaticStr),
    strum(serialize_all = "snake_case")
)]
pub(crate) enum PlatformEntry {
    Python(python::Entry),
    Dotnet(dotnet::Entry),
}

impl Platform {
    pub fn name(self) -> &'static str {
        self.into()
    }

    /// The platform library under `root`, with the native core it loads.
    pub(crate) fn library(self, root: &str) -> Layout {
        match self {
            Self::Python => python::library(root),
            Self::Dotnet => dotnet::library(root),
        }
    }

    /// The platform library on its own, for processes that host it directly.
    pub(crate) fn install(self) -> Option<Install> {
        match self {
            Self::Python => Some(Install::zip(self.library(""))),
            Self::Dotnet => cfg!(windows).then(|| Install::zip(self.library(""))),
        }
    }
}

impl PlatformEntry {
    /// `root` is the staged attach layout.
    pub(crate) fn plan(&self, root: &Path, settings: HostSettings) -> RuntimePlan {
        match self {
            Self::Python(entry) => python::plan(entry, root, settings),
            Self::Dotnet(entry) => dotnet::plan(entry, root, settings),
        }
    }
}
