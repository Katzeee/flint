use crate::{
    execution::{run_execution, ExecuteEvent, Outbound},
    settings::{BridgeSettings, Identity, SettingsSnapshot},
    state::State,
};
use flint_protocol::timing::{HEARTBEAT_ACK_TIMEOUT, HEARTBEAT_INTERVAL};
use flint_protocol::{envelope::Payload, *};
use futures_util::SinkExt;
use std::{
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::TcpStream,
    sync::{mpsc as async_mpsc, watch},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type Wire = flint_protocol::framing::Wire<TcpStream>;

async fn connect(settings: &BridgeSettings) -> Result<Wire, String> {
    let stream = tokio::time::timeout(
        Duration::from_secs(10),
        TcpStream::connect((settings.address.as_str(), settings.port)),
    )
    .await
    .map_err(|_| "connection timed out".to_string())?
    .map_err(|error| error.to_string())?;
    Ok(framed(stream))
}

async fn ack(wire: &mut Wire, request_id: &str) -> Result<InstanceAck, String> {
    let response = tokio::time::timeout(Duration::from_secs(10), read_envelope(wire))
        .await
        .map_err(|_| "response timed out".to_string())?
        .map_err(|error| error.to_string())?;
    if response.request_id != request_id {
        return Err("handshake request ID mismatch".into());
    }
    match response.payload {
        Some(Payload::InstanceAck(ack)) if ack.success => Ok(ack),
        _ => Err("bridge handshake rejected".into()),
    }
}

async fn run_heartbeat(mut wire: Wire, instance_id: String) -> Result<(), String> {
    loop {
        let request_id = Uuid::new_v4().simple().to_string();
        wire.send(envelope(
            request_id.clone(),
            Payload::Heartbeat(Heartbeat {
                instance_id: instance_id.clone(),
            }),
        ))
        .await
        .map_err(|error| error.to_string())?;
        tokio::time::timeout(HEARTBEAT_ACK_TIMEOUT, ack(&mut wire, &request_id))
            .await
            .map_err(|_| "heartbeat acknowledgement timed out".to_string())??;
        tokio::time::sleep(HEARTBEAT_INTERVAL).await;
    }
}

async fn run_session(
    identity: &Identity,
    settings_snapshot: &SettingsSnapshot,
    bridge_id: &str,
    state: Arc<Mutex<State>>,
    events: mpsc::Sender<ExecuteEvent>,
    outbound: &mut async_mpsc::UnboundedReceiver<Outbound>,
) -> Result<(), String> {
    let mut heartbeat_wire = connect(&settings_snapshot.settings).await?;
    let request_id = Uuid::new_v4().simple().to_string();
    heartbeat_wire
        .send(envelope(
            request_id.clone(),
            Payload::RegisterInstance(RegisterInstance {
                pid: std::process::id(),
                name_hint: identity.host.clone(),
                instance_name: settings_snapshot.settings.name.clone(),
                instance_type: identity.host.clone(),
                bridge_id: bridge_id.into(),
                runtime_version: identity.runtime_version.clone(),
                bridge_version: env!("CARGO_PKG_VERSION").into(),
            }),
        ))
        .await
        .map_err(|error| error.to_string())?;
    let identity = ack(&mut heartbeat_wire, &request_id).await?;
    if identity.instance_id.is_empty() || identity.session_token.is_empty() {
        return Err("incomplete instance registration".into());
    }
    let mut execution_wire = connect(&settings_snapshot.settings).await?;
    let request_id = Uuid::new_v4().simple().to_string();
    execution_wire
        .send(envelope(
            request_id.clone(),
            Payload::RegisterExecutionChannel(RegisterExecutionChannel {
                instance_id: identity.instance_id.clone(),
                pid: std::process::id(),
                session_token: identity.session_token,
            }),
        ))
        .await
        .map_err(|error| error.to_string())?;
    ack(&mut execution_wire, &request_id).await?;
    let generation = state
        .lock()
        .unwrap()
        .complete_registration(settings_snapshot, identity.instance_id.clone())?;
    let heartbeat = run_heartbeat(heartbeat_wire, identity.instance_id);
    let execution = run_execution(execution_wire, generation, state.clone(), events, outbound);
    tokio::select! {
        result = heartbeat => result,
        result = execution => result,
    }
}

pub(crate) async fn run(
    identity: Identity,
    mut settings: watch::Receiver<Arc<SettingsSnapshot>>,
    state: Arc<Mutex<State>>,
    events: mpsc::Sender<ExecuteEvent>,
    mut outbound: async_mpsc::UnboundedReceiver<Outbound>,
    stop: CancellationToken,
    reconnect: Arc<tokio::sync::Notify>,
) {
    let bridge_id = Uuid::new_v4().simple().to_string();
    let mut delay = 0;
    while !stop.is_cancelled() {
        let current = settings.borrow_and_update().clone();
        if !current.settings.enabled {
            tokio::select! {
                _ = settings.changed() => {}
                _ = stop.cancelled() => break,
            }
            continue;
        }
        let mut changed = false;
        let mut requested = false;
        let result = tokio::select! {
            biased;
            change = settings.changed() => {
                changed = change.is_ok();
                Err("bridge settings changed".into())
            },
            result = run_session(&identity, &current, &bridge_id, state.clone(), events.clone(), &mut outbound) => result,
            _ = reconnect.notified() => {
                requested = true;
                Ok(())
            },
            _ = stop.cancelled() => Ok(()),
        };
        state.lock().unwrap().finish_session(
            &current,
            requested,
            stop.is_cancelled(),
            result.as_ref().err().cloned(),
        );
        if stop.is_cancelled() {
            break;
        }
        if changed || requested || settings.has_changed().unwrap_or(false) {
            delay = 0;
            continue;
        }
        delay = if result.is_ok() {
            0
        } else {
            (delay * 2 + 1).min(10)
        };
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(delay)) => {},
            _ = settings.changed() => {
                delay = 0;
            },
            _ = reconnect.notified() => {
                delay = 0;
            },
            _ = stop.cancelled() => break,
        }
    }
}
