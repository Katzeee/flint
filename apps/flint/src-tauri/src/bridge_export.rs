use anyhow::{ensure, Result};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum BridgeExport {
    Python {
        #[arg(long, default_value = "flint-python.zip")]
        output: PathBuf,
    },
    Csharp {
        #[arg(long, default_value = "flint-csharp.zip")]
        output: PathBuf,
    },
    Unity {
        #[arg(long, default_value = "flint-unity.tgz")]
        output: PathBuf,
    },
}

impl BridgeExport {
    pub fn write(&self) -> Result<serde_json::Value> {
        let (output, extension, bytes) = match self {
            Self::Python { output } => (
                output,
                "zip",
                &include_bytes!(concat!(env!("OUT_DIR"), "/flint-python.zip"))[..],
            ),
            Self::Csharp { output } => {
                #[cfg(not(windows))]
                anyhow::bail!("C# export is available only on Windows");
                #[cfg(windows)]
                {
                    (
                        output,
                        "zip",
                        &include_bytes!(concat!(env!("OUT_DIR"), "/flint-csharp.zip"))[..],
                    )
                }
            }
            Self::Unity { output } => {
                #[cfg(not(all(windows, target_arch = "x86_64")))]
                anyhow::bail!("Unity export is available only on Windows x64");
                #[cfg(all(windows, target_arch = "x86_64"))]
                {
                    (
                        output,
                        "tgz",
                        &include_bytes!(concat!(env!("OUT_DIR"), "/flint-unity.tgz"))[..],
                    )
                }
            }
        };
        ensure!(
            output.extension().is_some_and(|value| value == extension),
            "Expected a .{extension} output file"
        );
        std::fs::write(output, bytes)?;
        Ok(serde_json::json!({"path":output,"version":env!("CARGO_PKG_VERSION")}))
    }
}
