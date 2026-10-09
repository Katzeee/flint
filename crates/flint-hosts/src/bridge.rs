//! Each host's Bridge: the files it installs with and attaches with, and its attach entry.

use crate::{
    HostKind,
    layout::{Format, Layout, write_archive},
    platform::{Platform, PlatformEntry, dotnet, python},
};
use anyhow::Result;
use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};
use strum::IntoEnumIterator;

macro_rules! bridges {
    ($path:literal) => {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../bridges/", $path))
    };
}

pub(crate) struct Install {
    pub format: Format,
    pub layout: Layout,
}

impl Install {
    pub fn zip(layout: Layout) -> Self {
        Self {
            format: Format::Zip,
            layout,
        }
    }
}

pub struct Attach {
    pub(crate) layout: Layout,
    pub(crate) entry: PlatformEntry,
}

/// Standalone hosts install their platform library directly, so they declare no install.
pub(crate) struct HostBridge {
    pub install: Option<Install>,
    pub attach: Option<Attach>,
}

pub(crate) fn bridge(host: HostKind) -> HostBridge {
    let python_library = |root| Platform::Python.library(root);
    match host {
        HostKind::Maya => HostBridge {
            install: Some(Install::zip(
                Layout::default()
                    .file("flint.mod", bridges!("hosts/maya/flint.mod"))
                    .file("flint/plug-ins/flint_plugin.py", bridges!("hosts/maya/flint_plugin.py"))
                    .file("flint/scripts/flint_maya.py", bridges!("hosts/maya/flint_maya.py"))
                    .merge(python_library("flint/scripts")),
            )),
            attach: Some(python::attach("flint_bridge.maya")),
        },
        HostKind::Max => HostBridge {
            install: cfg!(windows).then(|| {
                Install::zip(
                    Layout::default()
                        .file(
                            "Flint.bundle/PackageContents.xml",
                            bridges!("hosts/max/PackageContents.xml"),
                        )
                        .file(
                            "Flint.bundle/Contents/Scripts/flint_startup.ms",
                            bridges!("hosts/max/flint_startup.ms"),
                        )
                        .file(
                            "Flint.bundle/Contents/Scripts/flint_settings.mcr",
                            bridges!("hosts/max/flint_settings.mcr"),
                        )
                        .file(
                            "Flint.bundle/Contents/Python/flint_startup.py",
                            bridges!("hosts/max/flint_startup.py"),
                        )
                        .file(
                            "Flint.bundle/Contents/Python/flint_max.py",
                            bridges!("hosts/max/flint_max.py"),
                        )
                        .merge(python_library("Flint.bundle/Contents/Python")),
                )
            }),
            attach: Some(python::attach("flint_bridge.max")),
        },
        HostKind::Blender => HostBridge {
            install: Some(Install::zip(
                Layout::default()
                    .file("flint_blender/__init__.py", bridges!("hosts/blender/addon/__init__.py"))
                    .merge(python_library("flint_blender")),
            )),
            attach: Some(python::attach("flint_bridge.blender")),
        },
        HostKind::Unity => unity(),
        HostKind::StandalonePython => HostBridge {
            install: None,
            attach: Some(python::attach("flint_bridge.standalone_python")),
        },
        HostKind::StandaloneCsharp => HostBridge {
            install: None,
            attach: None,
        },
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn unity() -> HostBridge {
    use crate::layout::{CORE, NATIVE_CORE, UNITY_ADAPTER, UNITY_PACKAGE};
    HostBridge {
        install: Some(Install {
            format: Format::Tgz,
            layout: Layout::default()
                .tree("package", UNITY_PACKAGE)
                .versioned("package/package.json")
                .file("package/Editor/Flint.Unity.dll", UNITY_ADAPTER)
                .file(&format!("package/Editor/Plugins/{CORE}"), NATIVE_CORE),
        }),
        attach: Some(dotnet::attach(
            dotnet::Entry {
                assembly: "Flint.Unity.dll",
                manager: "Flint.Unity.EditorBridge",
                domain: "Unity Child Domain",
            },
            UNITY_ADAPTER,
        )),
    }
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn unity() -> HostBridge {
    HostBridge {
        install: None,
        attach: None,
    }
}

/// What `flint bridge export` writes: a host's install or a platform library.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportTarget {
    Host(HostKind),
    Platform(Platform),
}

impl ExportTarget {
    fn install(self) -> Option<Install> {
        match self {
            Self::Host(host) => bridge(host).install,
            Self::Platform(platform) => platform.install(),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Host(host) => host.into(),
            Self::Platform(platform) => platform.name(),
        }
    }

    /// Targets this build can export.
    pub fn available() -> impl Iterator<Item = ExportTarget> {
        HostKind::iter()
            .map(Self::Host)
            .chain(Platform::iter().map(Self::Platform))
            .filter(|target| target.install().is_some())
    }
}

impl fmt::Display for ExportTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl FromStr for ExportTarget {
    type Err = anyhow::Error;

    fn from_str(name: &str) -> Result<Self> {
        name.parse()
            .map(Self::Host)
            .or_else(|_| name.parse().map(Self::Platform))
            .map_err(|_| anyhow::anyhow!("unknown export target {name}"))
    }
}

/// Write `target` in its native format to `output`, or to `flint-<target>.<ext>`.
pub fn export(target: ExportTarget, output: Option<&Path>) -> Result<PathBuf> {
    let install = target
        .install()
        .ok_or_else(|| anyhow::anyhow!("{target} has no export on this platform"))?;
    let extension = install.format.extension();
    let output = output.map_or_else(
        || PathBuf::from(format!("flint-{target}.{extension}")),
        Path::to_path_buf,
    );
    anyhow::ensure!(
        output.extension().is_some_and(|value| value == extension),
        "expected a .{extension} output file"
    );
    write_archive(&install.layout, install.format, &output)?;
    Ok(output)
}

#[cfg(test)]
mod tests;
