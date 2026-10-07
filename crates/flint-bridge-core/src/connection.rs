use crate::{
    execution::{run_execution, Outbound},
    settings::{BridgeSettings, Identity, SettingsSnapshot},
    state::{BridgeState, Obstacle, ObstacleKind},
};
use flint_contracts::protocol::timing::{HEARTBEAT_ACK_TIMEOUT, HEARTBEAT_INTERVAL};
use flint_contracts::protocol::{envelope::Payload, *};
use futures_util::SinkExt;
use std::{
    convert::Infallible,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::TcpStream,
    sync::{mpsc as async_mpsc, watch},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type Wire = flint_contracts::protocol::framing::Wire<TcpStream>;

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

async fn run_heartbeat(mut wire: Wire, instance_id: String) -> Result<Infallible, String> {
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

fn obstacle(kind: ObstacleKind) -> impl FnOnce(String) -> Obstacle {
    move |message| Obstacle { kind, message }
}

/// Runs one connection session until it fails; only the caller ends it otherwise.
async fn run_session(
    identity: &Identity,
    settings_snapshot: &SettingsSnapshot,
    bridge_id: &str,
    state: Arc<Mutex<BridgeState>>,
    schedule: mpsc::Sender<u64>,
    outbound: &mut async_mpsc::UnboundedReceiver<Outbound>,
) -> Result<Infallible, Obstacle> {
    let mut heartbeat_wire = connect(&settings_snapshot.settings)
        .await
        .map_err(obstacle(ObstacleKind::Unreachable))?;
    let (execution_wire, instance_id) =
        register(identity, settings_snapshot, bridge_id, &mut heartbeat_wire)
            .await
            .map_err(obstacle(ObstacleKind::Registration))?;
    let generation = state
        .lock()
        .unwrap()
        .complete_registration(settings_snapshot, instance_id.clone())
        .map_err(obstacle(ObstacleKind::Registration))?;
    let heartbeat = run_heartbeat(heartbeat_wire, instance_id);
    let execution = run_execution(
        execution_wire,
        generation,
        state.clone(),
        schedule,
        outbound,
    );
    let Err(message) = tokio::select! {
        result = heartbeat => result,
        result = execution => result,
    };
    Err(obstacle(ObstacleKind::Lost)(message))
}

/// Registers the instance on the heartbeat connection, then opens its execution channel.
async fn register(
    identity: &Identity,
    settings_snapshot: &SettingsSnapshot,
    bridge_id: &str,
    heartbeat_wire: &mut Wire,
) -> Result<(Wire, String), String> {
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
    let registered = ack(heartbeat_wire, &request_id).await?;
    if registered.instance_id.is_empty() || registered.session_token.is_empty() {
        return Err("incomplete instance registration".into());
    }
    let mut execution_wire = connect(&settings_snapshot.settings).await?;
    let request_id = Uuid::new_v4().simple().to_string();
    execution_wire
        .send(envelope(
            request_id.clone(),
            Payload::RegisterExecutionChannel(RegisterExecutionChannel {
                instance_id: registered.instance_id.clone(),
                pid: std::process::id(),
                session_token: registered.session_token,
            }),
        ))
        .await
        .map_err(|error| error.to_string())?;
    ack(&mut execution_wire, &request_id).await?;
    Ok((execution_wire, registered.instance_id))
}

pub(crate) async fn run(
    identity: Identity,
    mut settings: watch::Receiver<Arc<SettingsSnapshot>>,
    state: Arc<Mutex<BridgeState>>,
    schedule: mpsc::Sender<u64>,
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
        let obstacle = tokio::select! {
            biased;
            change = settings.changed() => {
                changed = change.is_ok();
                None
            },
            Err(obstacle) = run_session(&identity, &current, &bridge_id, state.clone(), schedule.clone(), &mut outbound) => Some(obstacle),
            _ = reconnect.notified() => {
                requested = true;
                None
            },
            _ = stop.cancelled() => None,
        };
        let failed = obstacle.is_some() && !stop.is_cancelled();
        state
            .lock()
            .unwrap()
            .finish_session(&current, obstacle.filter(|_| failed));
        if stop.is_cancelled() {
            break;
        }
        if changed || requested || settings.has_changed().unwrap_or(false) {
            delay = 0;
            continue;
        }
        delay = if failed { (delay * 2 + 1).min(10) } else { 0 };
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
