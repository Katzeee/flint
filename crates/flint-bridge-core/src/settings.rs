pub(crate) use flint_contracts::host_settings::HostSettings;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BridgeOptions {
    host: String,
    runtime_version: String,
    settings: HostSettings,
}

impl BridgeOptions {
    pub(crate) fn into_parts(self) -> anyhow::Result<(Identity, HostSettings)> {
        let settings = self.settings;
        if self.host.trim().is_empty() {
            anyhow::bail!("the host is empty");
        }
        if !settings.valid() {
            anyhow::bail!("the address and instance name must be set and the port nonzero");
        }
        Ok((
            Identity {
                host: self.host,
                runtime_version: self.runtime_version,
            },
            settings,
        ))
    }
}

pub(crate) struct Identity {
    pub(crate) host: String,
    pub(crate) runtime_version: String,
}

// A revision identifies a settings application, including A -> B -> A changes.
// Registration may publish its state only while its snapshot is still current.
pub(crate) struct SettingsSnapshot {
    pub(crate) revision: u64,
    pub(crate) settings: HostSettings,
}

#[derive(Debug, PartialEq)]
#[repr(u32)]
pub(crate) enum ApplyResult {
    Applied = 0,
    Busy = 1,
    Invalid = 2,
    Stopped = 3,
}
