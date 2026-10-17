//! Executions: submitted by control clients, run by a Bridge session, and kept in workflow records.
use super::*;
use crate::store::now;
use state::Job;

pub(super) fn execute(
    backend: &BackendHandle,
    req: ExecuteRequest,
) -> Result<(String, oneshot::Receiver<ExecuteResponse>), Failure> {
    let request_id = Uuid::new_v4().simple().to_string();
    let (tx, rx) = oneshot::channel();
    let mut state = backend.0.state.lock().unwrap();
    if state.stopping {
        return Err(Failure::new(FailureCode::BackendStopping));
    }
    let Some((session_id, session)) = state.session_for_instance(&req.instance_id) else {
        return Err(Failure::new(FailureCode::InstanceOffline));
    };
    let Some(sender) = session.sender.clone() else {
        return Err(Failure::with_message(
            FailureCode::InstanceOffline,
            "the execution channel is not ready",
        ));
    };
    let session_id = session_id.clone();
    if state.jobs.values().any(|j| j.session_id == session_id) {
        return Err(Failure::new(FailureCode::InstanceBusy));
    }
    let execution_id = backend
        .0
        .store
        .append(&req.workflow_id, &req.instance_id, req.code.clone(), req.name)?;
    let command = HostExecuteRequest {
        code: req.code,
        filename: req.filename,
    };
    state.jobs.insert(
        request_id.clone(),
        Job {
            session_id: session_id.clone(),
            workflow: req.workflow_id,
            execution: execution_id.clone(),
            result: tx,
        },
    );
    if sender
        .try_send(envelope(request_id, Payload::HostExecuteRequest(command)))
        .is_err()
    {
        drop(state);
        disconnect(backend, &session_id);
        return Err(Failure::with_message(
            FailureCode::ConnectionFailed,
            "the execution channel is closed",
        ));
    }
    Ok((execution_id, rx))
}

/// Applies a message from a session's execution channel to the job its request ID names.
pub(super) fn receive(backend: &BackendHandle, session_id: &str, message: Envelope) -> Result<()> {
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
            backend.0.store.update(&job.workflow, &job.execution, |e| {
                e.stdout += &update.stdout_delta;
                e.stderr += &update.stderr_delta;
            })?;
        }
        Payload::HostExecuteResponse(response) => {
            let job = state.jobs.remove(&message.request_id).unwrap();
            drop(state);
            finish(backend, job, response);
        }
        _ => anyhow::bail!("unexpected host message"),
    }
    Ok(())
}

fn finish(backend: &BackendHandle, job: Job, response: HostExecuteResponse) {
    let persisted = backend.0.store.update(&job.workflow, &job.execution, |entry| {
        entry.status = response.status;
        entry.finished_at = Some(now());
        entry.traceback = response.traceback.clone();
        entry.error = response.error.clone();
    });
    let response = match persisted {
        Ok(()) => ExecuteResponse {
            execution_id: job.execution,
            status: response.status,
            traceback: response.traceback,
            error: response.error,
        },
        Err(error) => ExecuteResponse {
            execution_id: job.execution,
            status: ExecutionStatus::Failed as i32,
            traceback: None,
            error: Some(Failure::caused_by(FailureCode::ResultPersistenceFailed, &error)),
        },
    };
    let _ = job.result.send(response);
}

pub(super) fn fail_disconnected(backend: &BackendHandle, jobs: Vec<Job>) {
    for job in jobs {
        let response = HostExecuteResponse {
            status: ExecutionStatus::Failed as i32,
            traceback: None,
            error: Some(Failure::new(FailureCode::ExecutionDisconnected)),
        };
        finish(backend, job, response);
    }
}
