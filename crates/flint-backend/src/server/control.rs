use super::*;

pub(super) async fn connection(backend: BackendHandle, socket: TcpStream) -> Result<()> {
    let (mut wire, request) = first_message(socket, FIRST_MESSAGE_TIMEOUT).await?;
    let id = request.request_id;
    let response = dispatch(&backend, request.payload.unwrap()).await;
    let stop = matches!(response, Payload::StopBackendResponse(_));
    let sent = wire.send(envelope(id, response)).await;
    if stop {
        backend.0.shutdown.cancel();
    }
    sent?;
    Ok(())
}
async fn dispatch(backend: &BackendHandle, request: Payload) -> Payload {
    if backend.0.state.lock().unwrap().stopping && !matches!(request, Payload::PingRequest(_)) {
        return failure(ErrorCode::BackendStopping, "Backend is stopping");
    }
    match request {
        Payload::PingRequest(_) => Payload::PingResponse(backend.status()),
        Payload::ShowWindowRequest(_) => {
            backend.0.show_window.notify_one();
            Payload::ShowWindowResponse(ShowWindowResponse { accepted: true })
        }
        Payload::StopBackendRequest(_) => stop_backend(backend),
        Payload::ListInstancesRequest(req) => list_instances(backend, req),
        Payload::StartWorkflowRequest(req) => start_workflow(backend, req),
        Payload::ExecuteRequest(req) => execute(backend, req).await,
        Payload::GetExecutionRequest(req) => get_execution(backend, req),
        _ => failure(ErrorCode::UnknownRequest, "Not a control request"),
    }
}

fn stop_backend(backend: &BackendHandle) -> Payload {
    let mut state = backend.0.state.lock().unwrap();
    if !state.jobs.is_empty() {
        failure(ErrorCode::BackendBusy, "Executions are still active")
    } else {
        state.stopping = true;
        Payload::StopBackendResponse(StopBackendResponse { stopping: true })
    }
}

fn list_instances(backend: &BackendHandle, req: ListInstancesRequest) -> Payload {
    Payload::ListInstancesResponse(ListInstancesResponse {
        instances: backend
            .instances()
            .into_iter()
            .filter(|i| {
                req.instance_type
                    .as_ref()
                    .map_or(true, |t| t == &i.instance_type)
            })
            .collect(),
    })
}

fn start_workflow(backend: &BackendHandle, req: StartWorkflowRequest) -> Payload {
    match backend.0.store.create(req.name, req.description) {
        Ok(workflow_id) => Payload::StartWorkflowResponse(StartWorkflowResponse { workflow_id }),
        Err(e) => failure(ErrorCode::InternalError, e.to_string()),
    }
}

fn get_execution(backend: &BackendHandle, req: GetExecutionRequest) -> Payload {
    if backend.0.store.load(&req.workflow_id).is_err() {
        return failure(ErrorCode::WorkflowNotFound, "Workflow not found");
    }
    match backend
        .0
        .store
        .execution(&req.workflow_id, &req.execution_id)
    {
        Ok(e) => Payload::GetExecutionResponse(GetExecutionResponse {
            execution_id: e.execution_id,
            workflow_id: e.workflow_id,
            name: e.name,
            instance_id: e.instance_id,
            status: match e.status.as_str() {
                "succeeded" => ExecutionStatus::Succeeded,
                "failed" => ExecutionStatus::Failed,
                "pending" => ExecutionStatus::Pending,
                _ => ExecutionStatus::Running,
            } as i32,
            stdout: e.stdout,
            stderr: e.stderr,
            started_at: e.started_at,
            finished_at: e.finished_at,
            traceback: e.traceback,
            error: e.error,
            updated_at: e.updated_at,
            code: if req.view == ExecutionView::Full as i32 {
                Some(e.code)
            } else {
                None
            },
        }),
        Err(e) => failure(ErrorCode::ExecutionNotFound, e.to_string()),
    }
}

async fn execute(backend: &BackendHandle, req: ExecuteRequest) -> Payload {
    let request_id = Uuid::new_v4().simple().to_string();
    let (tx, mut rx) = oneshot::channel();
    let timer = CancellationToken::new();
    let execution_id;
    {
        let mut state = backend.0.state.lock().unwrap();
        if state.stopping {
            return failure(ErrorCode::BackendStopping, "Backend is stopping");
        }
        let Some(session) = state.sessions.get(&req.instance_id) else {
            return failure(ErrorCode::InstanceOffline, "Host is offline");
        };
        let Some(sender) = session.sender.clone() else {
            return failure(ErrorCode::InstanceOffline, "Execution channel is not ready");
        };
        if state.jobs.values().any(|j| j.instance == req.instance_id) {
            return failure(ErrorCode::InstanceBusy, "Instance is busy");
        }
        execution_id = match backend.0.store.append(
            &req.workflow_id,
            &req.instance_id,
            req.code.clone(),
            req.name.clone(),
            request_id.clone(),
        ) {
            Ok(id) => id,
            Err(e) => return failure(ErrorCode::WorkflowNotFound, e.to_string()),
        };
        let command = HostExecuteRequest {
            execution_id: execution_id.clone(),
            code: req.code,
            workflow_id: req.workflow_id.clone(),
            execution_name: Some(req.name),
            filename: req.filename,
        };
        state.jobs.insert(
            request_id.clone(),
            Job {
                instance: req.instance_id.clone(),
                workflow: req.workflow_id,
                execution: execution_id.clone(),
                output_sequence: 0,
                result: tx,
                timer: timer.clone(),
            },
        );
        if sender
            .try_send(envelope(
                request_id.clone(),
                Payload::HostExecuteRequest(command),
            ))
            .is_err()
        {
            drop(state);
            disconnect(backend, &req.instance_id);
            return failure(ErrorCode::ConnectionFailed, "Execution channel is closed");
        }
    }
    let handle = backend.clone();
    let rid = request_id.clone();
    let eid = execution_id.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = timer.cancelled() => {},
            _ = handle.0.shutdown.cancelled() => {},
            _ = tokio::time::sleep(Duration::from_secs(600)) => finish(&handle, &rid, ExecutionResult {
                execution_id: eid, status: ExecutionStatus::Failed as i32, traceback: None,
                error: Some("Execution response timed out; host code may still be running".into()),
            }),
        }
    });
    match tokio::time::timeout(Duration::from_secs(5), &mut rx).await {
        Ok(Ok(result)) => Payload::ExecutionResult(result),
        Ok(Err(_)) => failure(ErrorCode::ConnectionFailed, "Execution response lost"),
        Err(_) => Payload::ExecutionResult(ExecutionResult {
            execution_id,
            status: ExecutionStatus::Running as i32,
            traceback: None,
            error: None,
        }),
    }
}
