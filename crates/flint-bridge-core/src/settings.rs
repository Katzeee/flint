use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BridgeOptions {
    host: String,
    address: String,
    port: u16,
    name: String,
    runtime_version: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl BridgeOptions {
    pub(crate) fn into_parts(self) -> Result<(Identity, BridgeSettings), String> {
        let settings = BridgeSettings {
            address: self.address,
            port: self.port,
            name: self.name,
            enabled: self.enabled,
        };
        if self.host.trim().is_empty() || !settings.valid() {
            return Err("invalid bridge configuration".into());
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

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct BridgeSettings {
    pub(crate) address: String,
    pub(crate) port: u16,
    pub(crate) name: String,
    pub(crate) enabled: bool,
}

impl BridgeSettings {
    pub(crate) fn valid(&self) -> bool {
        !self.address.trim().is_empty() && self.port != 0 && !self.name.trim().is_empty()
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
    pub(crate) settings: BridgeSettings,
}

#[derive(Debug, PartialEq)]
#[repr(u32)]
pub(crate) enum ApplyResult {
    Applied = 0,
    Busy = 1,
    Invalid = 2,
}
