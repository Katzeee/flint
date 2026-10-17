use super::*;
use flint_contracts::config::bridge_token;
use std::time::Instant;

pub(super) async fn connection(backend: BackendHandle, socket: TcpStream) -> Result<()> {
    let (wire, first) = first_message(socket, FIRST_MESSAGE_TIMEOUT).await?;
    dispatch(backend, wire, first).await
}

async fn dispatch(backend: BackendHandle, wire: Wire, first: Envelope) -> Result<()> {
    match first.payload.unwrap() {
        Payload::RegisterInstance(req) => heartbeat_connection(backend, wire, first.request_id, req).await,
        Payload::RegisterExecutionChannel(req) => execution_connection(backend, wire, first.request_id, req).await,
        _ => anyhow::bail!("expected a Bridge registration"),
    }
}

/// Tells the Bridge why its registration was refused before closing the connection.
async fn reject(mut wire: Wire, request_id: String, reason: &str) -> Result<()> {
    let failure = Failure::with_message(FailureCode::RegistrationRejected, reason);
    wire.send(envelope(request_id, Payload::Failure(failure.clone())))
        .await?;
    Err(failure.into())
}

// Compares in constant time so response timing does not reveal the token.
fn token_matches(expected: &str, presented: &str) -> bool {
    !expected.is_empty()
        && expected.len() == presented.len()
        && expected
            .bytes()
            .zip(presented.bytes())
            .fold(0, |difference, (a, b)| difference | (a ^ b))
            == 0
}

async fn heartbeat_connection(
    backend: BackendHandle,
    mut wire: Wire,
    request_id: String,
    req: RegisterInstance,
) -> Result<()> {
    let token = match bridge_token::load_or_create() {
        Ok(token) => token,
        Err(error) => {
            return reject(
                wire,
                request_id,
                &format!("the backend cannot read its Bridge token: {error}"),
            )
            .await;
        }
    };
    if !token_matches(&token, &req.token) {
        return reject(wire, request_id, "the Bridge token does not match this backend").await;
    }
    let (session_id, cancel, superseded) = backend.0.state.lock().unwrap().register(req);
    executions::fail_disconnected(&backend, superseded);
    let result: Result<()> = async {
        wire.send(envelope(request_id, Payload::SessionRegistered(SessionRegistered { session_id: session_id.clone() }))).await?;
        loop {
            let message = tokio::select! {
                _ = cancel.cancelled() => break,
                msg = tokio::time::timeout(HEARTBEAT_IDLE_TIMEOUT, read_envelope(&mut wire)) => msg.context("heartbeat timed out")??,
            };
            anyhow::ensure!(matches!(message.payload, Some(Payload::Heartbeat(_))), "invalid heartbeat");
            {
                let mut state = backend.0.state.lock().unwrap();
                let Some(session) = state.sessions.get_mut(&session_id) else { break; };
                session.heartbeat = Instant::now();
            }
            wire.send(envelope(message.request_id, Payload::Ack(Ack {}))).await?;
        }
        Ok(())
    }.await;
    disconnect(&backend, &session_id);
    result
}

async fn execution_connection(
    backend: BackendHandle,
    mut wire: Wire,
    request_id: String,
    req: RegisterExecutionChannel,
) -> Result<()> {
    let (sender, mut receiver) = mpsc::channel(16);
    let admitted = {
        let mut state = backend.0.state.lock().unwrap();
        match state.sessions.get_mut(&req.session_id) {
            None => Err("the session is not registered"),
            Some(session) if session.sender.is_some() => Err("the execution channel is already connected"),
            Some(session) => {
                session.sender = Some(sender);
                Ok(session.cancel.clone())
            }
        }
    };
    let cancel = match admitted {
        Ok(cancel) => cancel,
        Err(reason) => return reject(wire, request_id, reason).await,
    };
    let result: Result<()> = async {
        wire.send(envelope(request_id, Payload::Ack(Ack {}))).await?;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                next = receiver.recv() => { if let Some(message) = next { wire.send(message).await?; } else { break; } },
                next = read_envelope(&mut wire) => {
                    let message = next.context("execution channel closed")?;
                    executions::receive(&backend, &req.session_id, message)?;
                }
            }
        }
        Ok(())
    }.await;
    disconnect(&backend, &req.session_id);
    result
}

#[cfg(test)]
mod tests;
