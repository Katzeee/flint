use crate::{
    claim::{self, ClaimOutcome, ClaimOwner},
    connection,
    execution::{ExecuteEvent, ExecutionReport, Outbound},
    settings::{ApplyResult, BridgeOptions, BridgeSettings, SettingsSnapshot},
    state::State,
};
use std::{
    io,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};
use tokio::sync::{mpsc as async_mpsc, watch, Notify};
use tokio_util::sync::CancellationToken;

pub struct BridgeCore {
    state: Arc<Mutex<State>>,
    events_rx: Mutex<mpsc::Receiver<ExecuteEvent>>,
    outbound_tx: async_mpsc::UnboundedSender<Outbound>,
    shutdown: CancellationToken,
    reconnect_notify: Arc<Notify>,
    settings_tx: watch::Sender<Arc<SettingsSnapshot>>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
    _claim: claim::ProcessClaim,
}

impl BridgeCore {
    pub(crate) fn new(options: BridgeOptions) -> Result<Self, String> {
        Self::start(options, claim::acquire)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(options: BridgeOptions, scope: &str) -> Result<Self, String> {
        Self::start(options, |owner| claim::acquire_for_test(scope, owner))
    }

    fn start(
        options: BridgeOptions,
        acquire: impl FnOnce(&ClaimOwner) -> io::Result<ClaimOutcome>,
    ) -> Result<Self, String> {
        let (identity, settings) = options.into_parts()?;
        let owner = ClaimOwner {
            host: identity.host.clone(),
            runtime_version: identity.runtime_version.clone(),
            bridge_version: env!("CARGO_PKG_VERSION").into(),
        };
        // Acquire before starting the thread so a second Bridge cannot register.
        let claim = match acquire(&owner) {
            Ok(ClaimOutcome::Acquired(claim)) => claim,
            Ok(ClaimOutcome::Occupied(owner)) => {
                return Err(match owner {
                    Some(owner) => format!("another Bridge already owns this process: {owner}"),
                    None => "another Bridge already owns this process".into(),
                })
            }
            Err(error) => return Err(format!("cannot claim the process: {error}")),
        };
        let settings = Arc::new(SettingsSnapshot {
            revision: 0,
            settings,
        });
        let state = Arc::new(Mutex::new(State::new(settings.clone())));
        let (settings_tx, settings_rx) = watch::channel(settings);
        let (events_tx, events_rx) = mpsc::channel();
        let (outbound_tx, outbound_rx) = async_mpsc::unbounded_channel();
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
                runtime.block_on(connection::run(
                    identity,
                    settings_rx,
                    thread_state,
                    events_tx,
                    outbound_rx,
                    thread_shutdown,
                    thread_reconnect_notify,
                ));
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            state,
            events_rx: Mutex::new(events_rx),
            outbound_tx,
            shutdown,
            reconnect_notify,
            settings_tx,
            thread: Mutex::new(Some(thread)),
            _claim: claim,
        })
    }

    pub(crate) fn apply_settings(&self, settings: BridgeSettings) -> ApplyResult {
        self.state
            .lock()
            .unwrap()
            .apply_settings(settings, &self.settings_tx)
    }

    pub(crate) fn reconnect(&self) -> bool {
        self.state.lock().unwrap().reconnect();
        self.reconnect_notify.notify_one();
        true
    }

    pub(crate) fn report_execution(&self, report: ExecutionReport) -> bool {
        self.state
            .lock()
            .unwrap()
            .report_execution(report, &self.outbound_tx)
    }

    pub(crate) fn poll(&self, timeout: Duration) -> Option<ExecuteEvent> {
        self.events_rx.lock().unwrap().recv_timeout(timeout).ok()
    }

    pub(crate) fn connected(&self) -> bool {
        self.state.lock().unwrap().connected()
    }

    pub(crate) fn busy(&self) -> bool {
        self.state.lock().unwrap().busy()
    }

    pub(crate) fn instance_id(&self) -> String {
        self.state.lock().unwrap().instance_id().to_owned()
    }

    pub(crate) fn status_json(&self) -> String {
        serde_json::to_string(&self.state.lock().unwrap().status()).unwrap()
    }

    pub(crate) fn stop(&self) {
        self.shutdown.cancel();
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
    }
}

impl Drop for BridgeCore {
    fn drop(&mut self) {
        self.stop();
    }
}
