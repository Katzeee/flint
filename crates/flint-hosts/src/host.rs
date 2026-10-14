//! Built-in host integrations, distinct from open Bridge registration identifiers.

use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// A host integration Flint knows by name. This does not promise that the host
/// is discoverable, attachable, or ready to execute on the current platform.
/// Standalone variants identify their language-specific integration; application
/// hosts choose their runtime inside their own implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Display, EnumString, IntoStaticStr, EnumIter)]
#[cfg_attr(feature = "typescript", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum HostKind {
    Maya,
    Max,
    Blender,
    Houdini,
    Unity,
    StandalonePython,
    StandaloneCsharp,
}

impl HostKind {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Maya => "Maya",
            Self::Max => "3ds Max",
            Self::Blender => "Blender",
            Self::Houdini => "Houdini",
            Self::Unity => "Unity",
            Self::StandalonePython => "Python",
            Self::StandaloneCsharp => "C#",
        }
    }

    pub(crate) fn matches_process(self, name: &str, command: &[OsString]) -> bool {
        match self {
            Self::Maya => matches!(name, "maya.exe" | "maya"),
            Self::Max => name == "3dsmax.exe",
            Self::Blender => matches!(name, "blender.exe" | "blender"),
            Self::Houdini => matches!(
                name,
                "houdini.exe" | "houdinifx.exe" | "houdinicore.exe" | "houdini" | "houdinifx" | "houdinicore"
            ),
            // Asset import workers share the editor executable; batch-mode editors remain hosts.
            Self::Unity => {
                name == "unity.exe"
                    && !command
                        .iter()
                        .skip(1)
                        .any(|arg| arg.eq_ignore_ascii_case("-assetImportWorker"))
                    || command.windows(2).any(|pair| {
                        pair[0].eq_ignore_ascii_case("-name")
                            && pair[1]
                                .to_string_lossy()
                                .to_ascii_lowercase()
                                .starts_with("assetimportworker")
                    })
            }
            Self::StandalonePython | Self::StandaloneCsharp => false,
        }
    }
}
