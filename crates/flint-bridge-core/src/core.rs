use crate::{
    claim::{self, ClaimOutcome, ClaimOwner},
    connection,
    execution_binding::OwnedExecutionBinding,
    execution_coordinator::ExecutionCoordinator,
    settings::{ApplyResult, BridgeOptions, BridgeSettings, SettingsSnapshot},
    state::BridgeState,
};
use std::{
    io,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tokio::sync::{watch, Notify};
use tokio_util::sync::CancellationToken;

/// Why no Bridge core was created. Each kind has a stable code in the C ABI.
#[derive(Debug, PartialEq)]
pub(crate) enum CreationError {
    InvalidConfiguration(String),
    Claimed(Option<ClaimOwner>),
    System(String),
}

impl CreationError {
    pub(crate) fn code(&self) -> u32 {
        match self {
            Self::InvalidConfiguration(_) => 1,
            Self::Claimed(_) => 2,
            Self::System(_) => 3,
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::InvalidConfiguration(message) => {
                format!("Invalid Bridge configuration: {message}")
            }
            Self::Claimed(Some(owner)) => {
                format!("Another Bridge already owns this process: {owner}")
            }
            Self::Claimed(None) => "Another Bridge already owns this process".into(),
            Self::System(message) => message.clone(),
        }
    }
}

pub struct BridgeCore {
    bridge_state: Arc<Mutex<BridgeState>>,
    execution_coordinator: Arc<ExecutionCoordinator>,
    dispatcher: Mutex<Option<thread::JoinHandle<()>>>,
    shutdown: CancellationToken,
    reconnect_notify: Arc<Notify>,
    settings_tx: watch::Sender<Arc<SettingsSnapshot>>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
    _claim: claim::ProcessClaim,
}

impl BridgeCore {
    pub(crate) fn new(
        options: BridgeOptions,
        execution_binding: OwnedExecutionBinding,
    ) -> Result<Self, CreationError> {
        Self::start(options, execution_binding, claim::acquire)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        options: BridgeOptions,
        execution_binding: OwnedExecutionBinding,
        scope: &str,
    ) -> Result<Self, CreationError> {
        Self::start(options, execution_binding, |owner| {
            claim::acquire_for_test(scope, owner)
        })
    }

    fn start(
        options: BridgeOptions,
        execution_binding: OwnedExecutionBinding,
        acquire: impl FnOnce(&ClaimOwner) -> io::Result<ClaimOutcome>,
    ) -> Result<Self, CreationError> {
        let (identity, settings) = options
            .into_parts()
            .map_err(CreationError::InvalidConfiguration)?;
        let owner = ClaimOwner {
            host: identity.host.clone(),
            runtime_version: identity.runtime_version.clone(),
            bridge_version: env!("CARGO_PKG_VERSION").into(),
        };
        // Acquire before starting the thread so a second Bridge cannot register.
        let claim = match acquire(&owner) {
            Ok(ClaimOutcome::Acquired(claim)) => claim,
            Ok(ClaimOutcome::Occupied(owner)) => return Err(CreationError::Claimed(owner)),
            Err(error) => {
                return Err(CreationError::System(format!(
                    "Cannot claim the process: {error}"
                )))
            }
        };
        let settings = Arc::new(SettingsSnapshot {
            revision: 0,
            settings,
        });
        let state = Arc::new(Mutex::new(BridgeState::new(settings.clone())));
        let (settings_tx, settings_rx) = watch::channel(settings);
        let (execution_coordinator, schedule, dispatcher) =
            ExecutionCoordinator::start(execution_binding, state.clone())
                .map_err(CreationError::System)?;
        let shutdown = CancellationToken::new();
        let reconnect_notify = Arc::new(Notify::new());
        let thread_state = state.clone();
        let thread_shutdown = shutdown.clone();
        let thread_reconnect_notify = reconnect_notify.clone();
        let thread = thread::Builder::new()
            .name("flint-bridge-core".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Tokio runtime");
                let flush_state = thread_state.clone();
                runtime.block_on(async {
                    tokio::select! {
                        _ = async {
                            let mut timer = tokio::time::interval(Duration::from_millis(20));
                            loop {
                                timer.tick().await;
                                flush_state.lock().unwrap().flush_output();
                            }
                        } => {},
                        _ = connection::run(
                            identity,
                            settings_rx,
                            thread_state,
                            schedule,
                            thread_shutdown,
                            thread_reconnect_notify,
                        ) => {},
                    }
                });
            });
        let thread = match thread {
            Ok(thread) => thread,
            Err(error) => {
                execution_coordinator.revoke();
                let _ = dispatcher.join();
                return Err(CreationError::System(format!(
                    "Cannot start the Bridge thread: {error}"
                )));
            }
        };
        Ok(Self {
            bridge_state: state,
            execution_coordinator,
            dispatcher: Mutex::new(Some(dispatcher)),
            shutdown,
            reconnect_notify,
            settings_tx,
            thread: Mutex::new(Some(thread)),
            _claim: claim,
        })
    }

    pub(crate) fn apply_settings(&self, settings: BridgeSettings) -> ApplyResult {
        self.bridge_state
            .lock()
            .unwrap()
            .apply_settings(settings, &self.settings_tx)
    }

    pub(crate) fn reconnect(&self) -> bool {
        if !self.bridge_state.lock().unwrap().reconnect() {
            return false;
        }
        self.reconnect_notify.notify_one();
        true
    }

    pub(crate) fn connected(&self) -> bool {
        self.bridge_state.lock().unwrap().connected()
    }

    pub(crate) fn busy(&self) -> bool {
        self.bridge_state.lock().unwrap().busy()
    }

    pub(crate) fn stopped(&self) -> bool {
        self.bridge_state.lock().unwrap().stopped()
    }

    pub(crate) fn instance_id(&self) -> String {
        self.bridge_state.lock().unwrap().instance_id().to_owned()
    }

    pub(crate) fn status_json(&self) -> String {
        serde_json::to_string(&self.bridge_state.lock().unwrap().status()).unwrap()
    }

    /// Ends the connection and cancels code that has not started. Returns whether
    /// host code is no longer active, so destruction cannot outrun it.
    pub(crate) fn stop(&self) -> bool {
        self.bridge_state.lock().unwrap().stop();
        self.shutdown.cancel();
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
        self.execution_coordinator.cancel_unstarted();
        !self.busy()
    }
}

impl Drop for BridgeCore {
    fn drop(&mut self) {
        self.stop();
        self.execution_coordinator.revoke();
        if let Some(dispatcher) = self.dispatcher.lock().unwrap().take() {
            if dispatcher.thread().id() != thread::current().id() {
                let _ = dispatcher.join();
            }
        }
    }
}
