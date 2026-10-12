use super::*;
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

async fn heartbeat_connection(
    backend: BackendHandle,
    mut wire: Wire,
    request_id: String,
    req: RegisterInstance,
) -> Result<()> {
    if req.pid == 0
        || req.instance_id.len() != 16
        || !req
            .instance_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return reject(wire, request_id, "the instance identity is invalid").await;
    }
    let (session_id, cancel, superseded) = backend.0.state.lock().unwrap().register(req);
    fail_disconnected(&backend, superseded);
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
                    process_message(&backend, &req.session_id, message)?;
                }
            }
        }
        Ok(())
    }.await;
    disconnect(&backend, &req.session_id);
    result
}

fn process_message(backend: &BackendHandle, session_id: &str, message: Envelope) -> Result<()> {
    let mut state = backend.0.state.lock().unwrap();
    if !state.sessions.contains_key(session_id) {
        return Ok(());
    }
    let Some(job) = state.jobs.get_mut(&message.request_id) else {
        return Ok(());
    };
    anyhow::ensure!(job.session_id == session_id, "response belongs to another session");
    match message.payload.unwrap() {
        Payload::ExecutionOutputUpdate(update) => {
            anyhow::ensure!(
                update.execution_id == job.execution && update.workflow_id == job.workflow,
                "output identity mismatch"
            );
            if update.sequence <= job.output_sequence {
                return Ok(());
            }
            anyhow::ensure!(update.sequence == job.output_sequence + 1, "output sequence gap");
            backend.0.store.update(&job.workflow, &job.execution, |e| {
                e.stdout += &update.stdout_delta;
                e.stderr += &update.stderr_delta;
            })?;
            job.output_sequence = update.sequence;
        }
        Payload::ExecutionResult(result) => {
            anyhow::ensure!(
                result.execution_id == job.execution
                    && matches!(
                        ExecutionStatus::try_from(result.status),
                        Ok(ExecutionStatus::Succeeded | ExecutionStatus::Failed)
                    ),
                "invalid execution result"
            );
            let job = state.jobs.remove(&message.request_id).unwrap();
            drop(state);
            finish_job(backend, job, result);
        }
        _ => anyhow::bail!("unexpected host message"),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
