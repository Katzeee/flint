use super::*;

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
    if req.pid == 0 || req.bridge_id.is_empty() {
        return reject(wire, request_id, "the Bridge identity is incomplete").await;
    }
    // A host process owns at most one Bridge. Reconnecting the same Bridge
    // reuses its bridge_id; a distinct bridge_id on the same pid means a
    // prior session is defunct (the process-level claim prevents two live
    // Bridges from one process). Replace either to keep one instance per pid.
    let superseded: Vec<String> = backend
        .0
        .state
        .lock()
        .unwrap()
        .sessions
        .iter()
        .filter(|(_, s)| s.registration.bridge_id == req.bridge_id || s.registration.pid == req.pid)
        .map(|(id, _)| id.clone())
        .collect();
    for id in superseded {
        disconnect(&backend, &id);
    }
    let hint: String = req
        .name_hint
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(32)
        .collect();
    let id = format!(
        "{}-{}",
        if hint.is_empty() { "host" } else { &hint },
        &Uuid::new_v4().simple().to_string()[..8]
    );
    let token = Uuid::new_v4().simple().to_string();
    let cancel = CancellationToken::new();
    backend.0.state.lock().unwrap().sessions.insert(
        id.clone(),
        Session {
            registration: req,
            token: token.clone(),
            cancel: cancel.clone(),
            sender: None,
            heartbeat: Instant::now(),
        },
    );
    let result: Result<()> = async {
        wire.send(envelope(request_id, Payload::InstanceAck(InstanceAck { instance_id: id.clone(), session_token: token }))).await?;
        loop {
            let message = tokio::select! {
                _ = cancel.cancelled() => break,
                msg = tokio::time::timeout(HEARTBEAT_IDLE_TIMEOUT, read_envelope(&mut wire)) => msg.context("heartbeat timed out")??,
            };
            anyhow::ensure!(matches!(message.payload, Some(Payload::Heartbeat(ref h)) if h.instance_id == id), "invalid heartbeat");
            if let Some(session) = backend.0.state.lock().unwrap().sessions.get_mut(&id) { session.heartbeat = Instant::now(); }
            wire.send(envelope(message.request_id, Payload::InstanceAck(InstanceAck::default()))).await?;
        }
        Ok(())
    }.await;
    disconnect(&backend, &id);
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
        match state.sessions.get_mut(&req.instance_id) {
            None => Err("the instance is not registered"),
            Some(session) if session.registration.pid != req.pid || session.token != req.session_token => {
                Err("the execution channel identity does not match its registration")
            }
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
        wire.send(envelope(request_id, Payload::InstanceAck(InstanceAck::default()))).await?;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                next = receiver.recv() => { if let Some(message) = next { wire.send(message).await?; } else { break; } },
                next = read_envelope(&mut wire) => {
                    let message = next.context("execution channel closed")?;
                    process_message(&backend, &req.instance_id, message)?;
                }
            }
        }
        Ok(())
    }.await;
    disconnect(&backend, &req.instance_id);
    result
}

fn process_message(backend: &BackendHandle, instance: &str, message: Envelope) -> Result<()> {
    let mut state = backend.0.state.lock().unwrap();
    let Some(job) = state.jobs.get_mut(&message.request_id) else {
        return Ok(());
    };
    anyhow::ensure!(job.instance == instance, "response belongs to another instance");
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
            drop(state);
            finish(backend, &message.request_id, result);
        }
        _ => anyhow::bail!("unexpected host message"),
    }
    Ok(())
}
