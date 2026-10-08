use crate::{
    execution::Execution,
    settings::{ApplyResult, BridgeSettings, SettingsSnapshot},
};
use flint_contracts::protocol::{Envelope, HostExecuteRequest};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::{mpsc::UnboundedSender, watch};

/// What the Bridge connection is doing now. A failure exists only inside the
/// state it explains, so leaving that state is what clears it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum Connection {
    Stopped,
    Disabled,
    Connecting,
    Connected { instance_id: String },
    Retrying { obstacle: Obstacle },
}

/// Why the last connection session ended; the Bridge retries after a delay.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Obstacle {
    pub(crate) kind: ObstacleKind,
    pub(crate) message: String,
}

/// The session stage that failed, which tells the user where to look.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ObstacleKind {
    /// No connection to the Bridge endpoint.
    Unreachable,
    /// Connected, but the backend did not complete registration.
    Registration,
    /// A registered session ended.
    Lost,
}

#[derive(Serialize)]
pub(crate) struct StatusSnapshot<'a> {
    connection: &'a Connection,
    busy: bool,
    settings: &'a BridgeSettings,
}

pub(crate) struct BridgeState {
    settings_snapshot: Arc<SettingsSnapshot>,
    connection: Connection,
    last_execution_id: u64,
    pub(crate) execution: Option<Execution>,
}

// Callers hold the same mutex for settings changes, registration commits, and
// execution admission: checking a revision and publishing state are indivisible.
impl BridgeState {
    pub(crate) fn new(settings_snapshot: Arc<SettingsSnapshot>) -> Self {
        let mut state = Self {
            settings_snapshot,
            connection: Connection::Disabled,
            last_execution_id: 0,
            execution: None,
        };
        state.reconnect();
        state
    }

    pub(crate) fn connected(&self) -> bool {
        matches!(self.connection, Connection::Connected { .. })
    }

    pub(crate) fn busy(&self) -> bool {
        self.execution.is_some()
    }

    pub(crate) fn stopped(&self) -> bool {
        matches!(self.connection, Connection::Stopped)
    }

    pub(crate) fn stop(&mut self) {
        self.connection = Connection::Stopped;
    }

    pub(crate) fn instance_id(&self) -> &str {
        match &self.connection {
            Connection::Connected { instance_id } => instance_id,
            _ => "",
        }
    }

    pub(crate) fn status(&self) -> StatusSnapshot<'_> {
        StatusSnapshot {
            connection: &self.connection,
            busy: self.busy(),
            settings: &self.settings_snapshot.settings,
        }
    }

    pub(crate) fn apply_settings(
        &mut self,
        settings: BridgeSettings,
        updates: &watch::Sender<Arc<SettingsSnapshot>>,
    ) -> ApplyResult {
        if self.stopped() {
            return ApplyResult::Stopped;
        }
        if !settings.valid() {
            return ApplyResult::Invalid;
        }
        if self.busy() {
            return ApplyResult::Busy;
        }
        if self.settings_snapshot.settings == settings {
            return ApplyResult::Applied;
        }
        self.settings_snapshot = Arc::new(SettingsSnapshot {
            revision: self.settings_snapshot.revision + 1,
            settings,
        });
        self.reconnect();
        updates.send_replace(self.settings_snapshot.clone());
        ApplyResult::Applied
    }

    pub(crate) fn reconnect(&mut self) -> bool {
        if self.stopped() {
            return false;
        }
        self.connection = if self.settings_snapshot.settings.enabled {
            Connection::Connecting
        } else {
            Connection::Disabled
        };
        true
    }

    pub(crate) fn complete_registration(
        &mut self,
        settings_snapshot: &SettingsSnapshot,
        instance_id: String,
    ) -> anyhow::Result<()> {
        if self.stopped() {
            anyhow::bail!("bridge stopped during registration");
        }
        if self.settings_snapshot.revision != settings_snapshot.revision {
            anyhow::bail!("bridge settings changed during registration");
        }
        self.connection = Connection::Connected { instance_id };
        Ok(())
    }

    /// A session without an obstacle ended on request and starts over.
    pub(crate) fn finish_session(
        &mut self,
        settings_snapshot: &SettingsSnapshot,
        obstacle: Option<Obstacle>,
    ) {
        if self.stopped() || self.settings_snapshot.revision != settings_snapshot.revision {
            return;
        }
        match obstacle {
            Some(obstacle) => self.connection = Connection::Retrying { obstacle },
            None => {
                self.reconnect();
            }
        }
        // Host code can outlive this connection; only its result clears busy.
    }

    pub(crate) fn begin_execution(
        &mut self,
        request_id: String,
        request: HostExecuteRequest,
        outbound: UnboundedSender<Envelope>,
    ) -> Option<u64> {
        if !self.connected() || self.busy() {
            return None;
        }
        self.last_execution_id += 1;
        self.execution = Some(Execution::new(
            self.last_execution_id,
            request_id,
            request,
            outbound,
        ));
        Some(self.last_execution_id)
    }
}

#[cfg(test)]
mod tests;
