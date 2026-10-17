//! Host settings shared by Bridge creation, reconfiguration, and external attach.
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HostSettings {
    pub address: String,
    pub port: u16,
    pub name: String,
    /// Empty selects this machine's Bridge token.
    #[serde(default)]
    pub token: String,
}

impl HostSettings {
    pub fn valid(&self) -> bool {
        !self.address.trim().is_empty() && self.port != 0 && !self.name.trim().is_empty()
    }
}
