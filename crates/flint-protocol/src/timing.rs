use std::time::Duration;

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
pub const HEARTBEAT_ACK_TIMEOUT: Duration = Duration::from_secs(5);
pub const HEARTBEAT_IDLE_TIMEOUT: Duration = Duration::from_secs(8);
