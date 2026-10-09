use super::*;

pub(super) async fn connection(backend: BackendHandle, socket: TcpStream) -> Result<()> {
    let (mut wire, request) = first_message(socket, FIRST_MESSAGE_TIMEOUT).await?;
    let id = request.request_id;
    let response = dispatch(&backend, request.payload.unwrap())
        .await
        .unwrap_or_else(Payload::Failure);
    let stop = matches!(response, Payload::StopBackendResponse(_));
    let sent = wire.send(envelope(id, response)).await;
    if stop {
        backend.0.shutdown.cancel();
    }
    sent?;
    Ok(())
}

async fn dispatch(backend: &BackendHandle, request: Payload) -> Result<Payload, Failure> {
    if backend.0.state.lock().unwrap().stopping && !matches!(request, Payload::PingRequest(_)) {
        return Err(Failure::new(FailureCode::BackendStopping));
    }
    Ok(match request {
        Payload::PingRequest(_) => Payload::PingResponse(backend.status()),
        Payload::StopBackendRequest(_) => {
            backend.begin_stop()?;
            Payload::StopBackendResponse(StopBackendResponse {})
        }
        Payload::ListInstancesRequest(req) => list_instances(backend, req),
        Payload::StartWorkflowRequest(req) => {
            let workflow_id = backend.0.store.create(req.name, req.description)?;
            Payload::StartWorkflowResponse(StartWorkflowResponse { workflow_id })
        }
        Payload::ListWorkflowsRequest(_) => Payload::ListWorkflowsResponse(ListWorkflowsResponse {
            workflows: backend.0.store.list()?,
        }),
        Payload::GetWorkflowRequest(req) => {
            let workflow = backend.0.store.load(&req.workflow_id)?;
            Payload::GetWorkflowResponse(GetWorkflowResponse {
                workflow_id: workflow.workflow_id,
                name: workflow.name,
                description: workflow.description,
                created_at: workflow.created_at,
                execs: workflow
                    .execs
                    .into_iter()
                    .map(|execution| execution_response(execution, ExecutionView::Full))
                    .collect(),
            })
        }
        Payload::ExecuteRequest(req) => Payload::ExecutionResult(execute(backend, req).await?),
        Payload::GetExecutionRequest(req) => get_execution(backend, req)?,
        _ => return Err(Failure::new(FailureCode::UnknownRequest)),
    })
}

fn list_instances(backend: &BackendHandle, req: ListInstancesRequest) -> Payload {
    Payload::ListInstancesResponse(ListInstancesResponse {
        instances: backend
            .instances()
            .into_iter()
            .filter(|i| req.instance_type.as_ref().is_none_or(|t| t == &i.instance_type))
            .collect(),
    })
}

fn get_execution(backend: &BackendHandle, req: GetExecutionRequest) -> Result<Payload, Failure> {
    let e = backend.0.store.execution(&req.workflow_id, &req.execution_id)?;
    Ok(Payload::GetExecutionResponse(execution_response(e, req.view())))
}

fn execution_response(e: crate::store::Execution, view: ExecutionView) -> GetExecutionResponse {
    GetExecutionResponse {
        execution_id: e.execution_id,
        workflow_id: e.workflow_id,
        name: e.name,
        instance_id: e.instance_id,
        status: e.status as i32,
        stdout: e.stdout,
        stderr: e.stderr,
        started_at: e.started_at,
        finished_at: e.finished_at,
        traceback: e.traceback,
        error: e.error,
        updated_at: e.updated_at,
        code: if view == ExecutionView::Full {
            Some(e.code)
        } else {
            None
        },
    }
}

async fn execute(backend: &BackendHandle, req: ExecuteRequest) -> Result<ExecutionResult, Failure> {
    let request_id = Uuid::new_v4().simple().to_string();
    let (tx, mut rx) = oneshot::channel();
    let timer = CancellationToken::new();
    let execution_id;
    {
        let mut state = backend.0.state.lock().unwrap();
        if state.stopping {
            return Err(Failure::new(FailureCode::BackendStopping));
        }
        let Some(session) = state.sessions.get(&req.instance_id) else {
            return Err(Failure::new(FailureCode::InstanceOffline));
        };
        let Some(sender) = session.sender.clone() else {
            return Err(Failure::with_message(
                FailureCode::InstanceOffline,
                "the execution channel is not ready",
            ));
        };
        if state.jobs.values().any(|j| j.instance == req.instance_id) {
            return Err(Failure::new(FailureCode::InstanceBusy));
        }
        execution_id =
            backend
                .0
                .store
                .append(&req.workflow_id, &req.instance_id, req.code.clone(), req.name.clone())?;
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
            .try_send(envelope(request_id.clone(), Payload::HostExecuteRequest(command)))
            .is_err()
        {
            drop(state);
            disconnect(backend, &req.instance_id);
            return Err(Failure::with_message(
                FailureCode::ConnectionFailed,
                "the execution channel is closed",
            ));
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
                error: Some(Failure::new(FailureCode::ExecutionTimeout)),
            }),
        }
    });
    match tokio::time::timeout(Duration::from_secs(5), &mut rx).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(_)) => Err(Failure::with_message(
            FailureCode::ConnectionFailed,
            "the execution response was lost",
        )),
        Err(_) => Ok(ExecutionResult {
            execution_id,
            status: ExecutionStatus::Running as i32,
            traceback: None,
            error: None,
        }),
    }
}
