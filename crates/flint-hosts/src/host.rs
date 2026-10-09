//! Built-in host integrations, distinct from open Bridge registration identifiers.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// A host integration Flint knows by name. This does not promise that the host
/// is discoverable, attachable, or ready to execute on the current platform.
/// Standalone variants identify their language-specific integration; application
/// hosts choose their runtime inside their own implementation.
#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
    Serialize,
    Deserialize,
    Display,
    EnumString,
    IntoStaticStr,
    EnumIter,
)]
#[cfg_attr(feature = "typescript", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum HostKind {
    Maya,
    Max,
    Blender,
    Unity,
    StandalonePython,
    StandaloneCsharp,
}
