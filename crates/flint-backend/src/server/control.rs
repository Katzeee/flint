use super::*;

pub(super) async fn connection(backend: BackendHandle, socket: TcpStream) -> Result<()> {
    let (mut wire, request) = first_message(socket, FIRST_MESSAGE_TIMEOUT).await?;
    let id = request.request_id;
    let payload = request.payload.unwrap();
    if let Payload::ExecuteRequest(request) = payload {
        let detach = request.detach;
        let (execution_id, completion) = match executions::execute(&backend, request) {
            Ok(submitted) => submitted,
            Err(failure) => {
                wire.send(envelope(id, Payload::Failure(failure))).await?;
                return Ok(());
            }
        };
        if detach {
            let response = ExecuteResponse {
                execution_id,
                status: ExecutionStatus::Running as i32,
                traceback: None,
                error: None,
            };
            wire.send(envelope(id, Payload::ExecuteResponse(response))).await?;
            return Ok(());
        }
        tokio::select! {
            response = completion => {
                let response = response.context("execution response was lost")?;
                wire.send(envelope(id, Payload::ExecuteResponse(response))).await?;
            }
            _ = read_envelope(&mut wire) => {}
        }
        return Ok(());
    }
    let response = dispatch(&backend, payload).await.unwrap_or_else(Payload::Failure);
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
        Payload::GetBridgeTokenRequest(_) => Payload::GetBridgeTokenResponse(GetBridgeTokenResponse {
            token: flint_contracts::config::bridge_token::load_or_create()
                .map_err(|error| Failure::caused_by(FailureCode::InternalError, &error))?,
        }),
        Payload::ListAppsRequest(req) => Payload::ListAppsResponse(ListAppsResponse {
            apps: apps::list(backend, req.host).await?,
        }),
        Payload::GetAppInfoRequest(req) => Payload::GetAppInfoResponse(apps::info(backend, req).await?),
        Payload::FocusAppRequest(req) => {
            apps::focus(req.pid).await?;
            Payload::FocusAppResponse(FocusAppResponse {})
        }
        Payload::AttachRequest(req) => Payload::AttachResponse(AttachResponse {
            app: Some(apps::attach(backend, req).await?),
        }),
        Payload::StartWorkflowRequest(req) => {
            let workflow_id = backend.0.store.create(req.name, req.description)?;
            Payload::StartWorkflowResponse(StartWorkflowResponse { workflow_id })
        }
        Payload::ListWorkflowsRequest(_) => Payload::ListWorkflowsResponse(ListWorkflowsResponse {
            workflows: backend.0.store.list()?,
        }),
        Payload::GetWorkflowRequest(req) => Payload::GetWorkflowResponse(GetWorkflowResponse {
            workflow: Some(backend.0.store.load(&req.workflow_id)?),
        }),
        Payload::GetExecutionRequest(req) => {
            let mut execution = backend.0.store.execution(&req.workflow_id, &req.execution_id)?;
            if req.view() == ExecutionView::Summary {
                execution.code = None;
            }
            Payload::GetExecutionResponse(GetExecutionResponse {
                execution: Some(execution),
            })
        }
        _ => return Err(Failure::new(FailureCode::UnknownRequest)),
    })
}
