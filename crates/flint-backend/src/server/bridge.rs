use super::*;

pub(super) async fn connection(backend: BackendHandle, socket: TcpStream) -> Result<()> {
    let (wire, first) = first_message(socket, FIRST_MESSAGE_TIMEOUT).await?;
    dispatch(backend, wire, first).await
}

async fn dispatch(backend: BackendHandle, wire: Wire, first: Envelope) -> Result<()> {
    match first.payload.unwrap() {
        Payload::RegisterInstance(req) => {
            heartbeat_connection(backend, wire, first.request_id, req).await
        }
        Payload::RegisterExecutionChannel(req) => {
            execution_connection(backend, wire, first.request_id, req).await
        }
        _ => anyhow::bail!("Expected Bridge registration"),
    }
}

async fn heartbeat_connection(
    backend: BackendHandle,
    mut wire: Wire,
    request_id: String,
    req: RegisterInstance,
) -> Result<()> {
    anyhow::ensure!(
        req.pid != 0 && !req.bridge_id.is_empty(),
        "Invalid bridge identity"
    );
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
        .filter(|(_, s)| s.bridge_id == req.bridge_id || s.info.pid == req.pid)
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
            info: InstanceInfo {
                instance_id: id.clone(),
                instance_name: req.instance_name,
                instance_type: req.instance_type,
                pid: req.pid,
                runtime_version: req.runtime_version,
                bridge_version: req.bridge_version,
                execution_ready: false,
            },
            bridge_id: req.bridge_id,
            token: token.clone(),
            cancel: cancel.clone(),
            sender: None,
            exec_generation: String::new(),
            heartbeat: Instant::now(),
        },
    );
    let result: Result<()> = async {
        wire.send(envelope(request_id, Payload::InstanceAck(InstanceAck { success: true, instance_id: id.clone(), session_token: token, ..Default::default() }))).await?;
        loop {
            let message = tokio::select! {
                _ = cancel.cancelled() => break,
                msg = tokio::time::timeout(HEARTBEAT_IDLE_TIMEOUT, read_envelope(&mut wire)) => msg??,
            };
            anyhow::ensure!(matches!(message.payload, Some(Payload::Heartbeat(ref h)) if h.instance_id == id), "Invalid heartbeat");
            if let Some(session) = backend.0.state.lock().unwrap().sessions.get_mut(&id) { session.heartbeat = Instant::now(); }
            wire.send(envelope(message.request_id, Payload::InstanceAck(InstanceAck { success: true, ..Default::default() }))).await?;
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
    let generation = Uuid::new_v4().simple().to_string();
    let cancel = {
        let mut state = backend.0.state.lock().unwrap();
        let session = state
            .sessions
            .get_mut(&req.instance_id)
            .context("Instance not registered")?;
        anyhow::ensure!(
            session.info.pid == req.pid && session.token == req.session_token,
            "Execution channel identity mismatch"
        );
        anyhow::ensure!(
            session.sender.is_none(),
            "Execution channel already connected"
        );
        session.sender = Some(sender);
        session.exec_generation = generation.clone();
        session.info.execution_ready = true;
        session.cancel.clone()
    };
    let result: Result<()> = async {
        wire.send(envelope(request_id, Payload::InstanceAck(InstanceAck { success: true, ..Default::default() }))).await?;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                next = receiver.recv() => { if let Some(message) = next { wire.send(message).await?; } else { break; } },
                next = read_envelope(&mut wire) => {
                    let message = next.context("Execution channel closed")?;
                    process_message(&backend, &req.instance_id, message)?;
                }
            }
        }
        Ok(())
    }.await;
    let owns = backend
        .0
        .state
        .lock()
        .unwrap()
        .sessions
        .get(&req.instance_id)
        .map_or(false, |s| s.exec_generation == generation);
    if owns {
        disconnect(&backend, &req.instance_id);
    }
    result
}

fn process_message(backend: &BackendHandle, instance: &str, message: Envelope) -> Result<()> {
    let mut state = backend.0.state.lock().unwrap();
    let Some(job) = state.jobs.get_mut(&message.request_id) else {
        return Ok(());
    };
    anyhow::ensure!(
        job.instance == instance,
        "Response belongs to another instance"
    );
    match message.payload.unwrap() {
        Payload::ExecutionOutputUpdate(update) => {
            anyhow::ensure!(
                update.execution_id == job.execution && update.workflow_id == job.workflow,
                "Output identity mismatch"
            );
            if update.sequence <= job.output_sequence {
                return Ok(());
            }
            anyhow::ensure!(
                update.sequence == job.output_sequence + 1,
                "Output sequence gap"
            );
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
                "Invalid execution result"
            );
            drop(state);
            finish(backend, &message.request_id, result);
        }
        _ => anyhow::bail!("Unexpected host message"),
    }
    Ok(())
}
