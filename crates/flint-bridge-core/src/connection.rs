use crate::{
    execution::run_execution,
    settings::{HostSettings, Identity, SettingsSnapshot},
    state::{BridgeState, Obstacle, ObstacleKind},
};
use anyhow::{Context, Result};
use flint_contracts::config::bridge_token;
use flint_contracts::protocol::timing::{HEARTBEAT_ACK_TIMEOUT, HEARTBEAT_INTERVAL};
use flint_contracts::protocol::{envelope::Payload, *};
use futures_util::SinkExt;
use std::{
    convert::Infallible,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use tokio::{net::TcpStream, sync::watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

type Wire = flint_contracts::protocol::framing::Wire<TcpStream>;

async fn connect(settings: &HostSettings) -> Result<Wire> {
    let stream = tokio::time::timeout(
        Duration::from_secs(10),
        TcpStream::connect((settings.address.as_str(), settings.port)),
    )
    .await
    .context("connection timed out")??;
    Ok(framed(stream))
}

async fn read_response(wire: &mut Wire, request_id: &str) -> Result<Payload> {
    let response = tokio::time::timeout(Duration::from_secs(10), read_envelope(wire))
        .await
        .context("response timed out")??;
    anyhow::ensure!(response.request_id == request_id, "handshake request ID mismatch");
    match response.payload.unwrap() {
        Payload::Failure(failure) => Err(failure.into()),
        payload => Ok(payload),
    }
}

async fn read_ack(wire: &mut Wire, request_id: &str) -> Result<()> {
    anyhow::ensure!(
        matches!(read_response(wire, request_id).await?, Payload::Ack(_)),
        "expected acknowledgement"
    );
    Ok(())
}

async fn run_heartbeat(mut wire: Wire) -> Result<Infallible> {
    loop {
        let request_id = Uuid::new_v4().simple().to_string();
        wire.send(envelope(request_id.clone(), Payload::Heartbeat(Heartbeat {})))
            .await?;
        tokio::time::timeout(HEARTBEAT_ACK_TIMEOUT, read_ack(&mut wire, &request_id))
            .await
            .context("heartbeat acknowledgement timed out")??;
        tokio::time::sleep(HEARTBEAT_INTERVAL).await;
    }
}

fn obstacle<E: Into<anyhow::Error>>(kind: ObstacleKind) -> impl FnOnce(E) -> Obstacle {
    move |error| Obstacle {
        kind,
        message: format!("{:#}", error.into()),
    }
}

/// Runs one connection session until it fails; only the caller ends it otherwise.
async fn run_session(
    identity: &Identity,
    settings_snapshot: &SettingsSnapshot,
    instance_id: &str,
    state: Arc<Mutex<BridgeState>>,
    schedule: mpsc::Sender<u64>,
) -> std::result::Result<Infallible, Obstacle> {
    let mut heartbeat_wire = connect(&settings_snapshot.settings)
        .await
        .map_err(obstacle(ObstacleKind::Unreachable))?;
    let execution_wire = register(identity, settings_snapshot, instance_id, &mut heartbeat_wire)
        .await
        .map_err(obstacle(ObstacleKind::Registration))?;
    state
        .lock()
        .unwrap()
        .complete_registration(settings_snapshot)
        .map_err(obstacle(ObstacleKind::Registration))?;
    let heartbeat = run_heartbeat(heartbeat_wire);
    let execution = run_execution(execution_wire, state.clone(), schedule);
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
    instance_id: &str,
    heartbeat_wire: &mut Wire,
) -> Result<Wire> {
    let token = match settings_snapshot.settings.token.as_str() {
        "" => bridge_token::read().context("cannot read this machine's Bridge token")?,
        token => token.into(),
    };
    let request_id = Uuid::new_v4().simple().to_string();
    heartbeat_wire
        .send(envelope(
            request_id.clone(),
            Payload::RegisterInstance(RegisterInstance {
                pid: std::process::id(),
                instance_name: settings_snapshot.settings.name.clone(),
                instance_type: identity.host.clone(),
                instance_id: instance_id.into(),
                runtime_version: identity.runtime_version.clone(),
                bridge_version: env!("CARGO_PKG_VERSION").into(),
                token,
            }),
        ))
        .await?;
    let Payload::SessionRegistered(registered) = read_response(heartbeat_wire, &request_id).await? else {
        anyhow::bail!("expected session registration");
    };
    let mut execution_wire = connect(&settings_snapshot.settings).await?;
    let request_id = Uuid::new_v4().simple().to_string();
    execution_wire
        .send(envelope(
            request_id.clone(),
            Payload::RegisterExecutionChannel(RegisterExecutionChannel {
                session_id: registered.session_id,
            }),
        ))
        .await?;
    read_ack(&mut execution_wire, &request_id).await?;
    Ok(execution_wire)
}

pub(crate) async fn run(
    instance_id: String,
    identity: Identity,
    mut settings: watch::Receiver<Arc<SettingsSnapshot>>,
    state: Arc<Mutex<BridgeState>>,
    schedule: mpsc::Sender<u64>,
    stop: CancellationToken,
    reconnect: Arc<tokio::sync::Notify>,
) {
    let mut delay = 0;
    while !stop.is_cancelled() {
        let current = settings.borrow_and_update().clone();
        let mut changed = false;
        let mut requested = false;
        let obstacle = tokio::select! {
            biased;
            change = settings.changed() => {
                changed = change.is_ok();
                None
            },
            Err(obstacle) = run_session(&identity, &current, &instance_id, state.clone(), schedule.clone()) => Some(obstacle),
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
