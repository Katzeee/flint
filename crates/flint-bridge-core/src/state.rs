use crate::{
    execution::{ExecuteEvent, ExecutionReport, ExecutionState, Outbound},
    settings::{ApplyResult, BridgeSettings, SettingsSnapshot},
};
use flint_protocol::HostExecuteRequest;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ConnectionStatus {
    Disabled,
    Connecting,
    Connected,
    Reconnecting,
}

#[derive(Serialize)]
pub(crate) struct StatusSnapshot<'a> {
    connection: ConnectionStatus,
    last_error: &'a Option<String>,
    busy: bool,
    settings: &'a BridgeSettings,
    instance_id: &'a str,
}

pub(crate) struct State {
    settings_snapshot: Arc<SettingsSnapshot>,
    connection: ConnectionStatus,
    last_error: Option<String>,
    instance_id: String,
    generation: u64,
    execution: ExecutionState,
}

// Callers hold the same mutex for settings changes, registration commits, and
// execution admission: checking a revision and publishing state are indivisible.
impl State {
    pub(crate) fn new(settings_snapshot: Arc<SettingsSnapshot>) -> Self {
        let mut state = Self {
            settings_snapshot,
            connection: ConnectionStatus::Disabled,
            last_error: None,
            instance_id: String::new(),
            generation: 0,
            execution: ExecutionState::default(),
        };
        state.reconnect();
        state
    }

    pub(crate) fn connected(&self) -> bool {
        self.connection == ConnectionStatus::Connected
    }

    pub(crate) fn busy(&self) -> bool {
        self.execution.busy()
    }

    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub(crate) fn status(&self) -> StatusSnapshot<'_> {
        StatusSnapshot {
            connection: self.connection,
            last_error: &self.last_error,
            busy: self.busy(),
            settings: &self.settings_snapshot.settings,
            instance_id: &self.instance_id,
        }
    }

    pub(crate) fn apply_settings(
        &mut self,
        settings: BridgeSettings,
        updates: &watch::Sender<Arc<SettingsSnapshot>>,
    ) -> ApplyResult {
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

    pub(crate) fn reconnect(&mut self) {
        self.instance_id.clear();
        self.connection = if self.settings_snapshot.settings.enabled {
            ConnectionStatus::Connecting
        } else {
            ConnectionStatus::Disabled
        };
        self.last_error = None;
    }

    pub(crate) fn complete_registration(
        &mut self,
        settings_snapshot: &SettingsSnapshot,
        instance_id: String,
    ) -> Result<u64, String> {
        if self.settings_snapshot.revision != settings_snapshot.revision {
            return Err("bridge settings changed during registration".into());
        }
        self.generation += 1;
        self.connection = ConnectionStatus::Connected;
        self.last_error = None;
        self.instance_id = instance_id;
        Ok(self.generation)
    }

    pub(crate) fn finish_session(
        &mut self,
        settings_snapshot: &SettingsSnapshot,
        requested: bool,
        stopping: bool,
        error: Option<String>,
    ) {
        if self.settings_snapshot.revision != settings_snapshot.revision {
            return;
        }
        self.instance_id.clear();
        if requested || stopping {
            self.reconnect();
        } else {
            self.connection = ConnectionStatus::Reconnecting;
            self.last_error = error;
        }
        // Host code can outlive this connection; only its result clears busy.
    }

    pub(crate) fn begin_execution(
        &mut self,
        generation: u64,
        request_id: String,
        request: HostExecuteRequest,
    ) -> Option<ExecuteEvent> {
        if !self.connected() || self.generation != generation {
            return None;
        }
        self.execution.begin(generation, request_id, request)
    }

    pub(crate) fn report_execution(
        &mut self,
        report: ExecutionReport,
        outbound: &mpsc::UnboundedSender<Outbound>,
    ) -> bool {
        self.execution.report(report, outbound)
    }
}

#[cfg(test)]
mod tests;
