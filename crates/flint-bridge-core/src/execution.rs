use crate::state::State;
use flint_protocol::{envelope::Payload, *};
use futures_util::SinkExt;
use serde::{Deserialize, Serialize};
use std::sync::{mpsc, Arc, Mutex};
use tokio::{net::TcpStream, sync::mpsc as async_mpsc};

type Wire = flint_protocol::framing::Wire<TcpStream>;

#[derive(Serialize)]
pub(crate) struct ExecuteEvent {
    request_id: String,
    workflow_id: String,
    execution_id: String,
    code: String,
    filename: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ExecutionReport {
    Output {
        request_id: String,
        stdout: String,
        stderr: String,
    },
    Result {
        request_id: String,
        succeeded: bool,
        traceback: Option<String>,
        error: Option<String>,
    },
}

struct Active {
    generation: u64,
    request_id: String,
    workflow_id: String,
    execution_id: String,
    sequence: u64,
}

pub(crate) struct Outbound {
    pub(crate) generation: u64,
    pub(crate) envelope: Envelope,
}

#[derive(Default)]
pub(crate) struct ExecutionState {
    active: Option<Active>,
}

impl ExecutionState {
    pub(crate) fn busy(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn begin(
        &mut self,
        generation: u64,
        request_id: String,
        request: HostExecuteRequest,
    ) -> Option<ExecuteEvent> {
        if self.busy() {
            return None;
        }
        self.active = Some(Active {
            generation,
            request_id: request_id.clone(),
            workflow_id: request.workflow_id.clone(),
            execution_id: request.execution_id.clone(),
            sequence: 0,
        });
        Some(ExecuteEvent {
            request_id,
            workflow_id: request.workflow_id,
            execution_id: request.execution_id,
            code: request.code,
            filename: request.filename,
        })
    }

    pub(crate) fn report(
        &mut self,
        report: ExecutionReport,
        outbound: &async_mpsc::UnboundedSender<Outbound>,
    ) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        let (generation, envelope) = match report {
            ExecutionReport::Output {
                request_id,
                stdout,
                stderr,
            } => {
                if request_id != active.request_id {
                    return false;
                }
                if stdout.is_empty() && stderr.is_empty() {
                    return true;
                }
                active.sequence += 1;
                (
                    active.generation,
                    envelope(
                        request_id,
                        Payload::ExecutionOutputUpdate(ExecutionOutputUpdate {
                            workflow_id: active.workflow_id.clone(),
                            execution_id: active.execution_id.clone(),
                            sequence: active.sequence,
                            stdout_delta: stdout,
                            stderr_delta: stderr,
                        }),
                    ),
                )
            }
            ExecutionReport::Result {
                request_id,
                succeeded,
                traceback,
                error,
            } => {
                if request_id != active.request_id {
                    return false;
                }
                let generation = active.generation;
                let envelope = envelope(
                    request_id,
                    Payload::ExecutionResult(ExecutionResult {
                        execution_id: active.execution_id.clone(),
                        status: if succeeded {
                            ExecutionStatus::Succeeded
                        } else {
                            ExecutionStatus::Failed
                        } as i32,
                        traceback,
                        error,
                    }),
                );
                self.active = None;
                (generation, envelope)
            }
        };
        outbound
            .send(Outbound {
                generation,
                envelope,
            })
            .is_ok()
    }
}

pub(crate) async fn run_execution(
    mut wire: Wire,
    generation: u64,
    state: Arc<Mutex<State>>,
    events: mpsc::Sender<ExecuteEvent>,
    outbound: &mut async_mpsc::UnboundedReceiver<Outbound>,
) -> Result<(), String> {
    loop {
        tokio::select! {
            incoming = read_envelope(&mut wire) => {
                let message = incoming.map_err(|error| error.to_string())?;
                let Some(Payload::HostExecuteRequest(request)) = message.payload else {
                    return Err("unexpected execution message".into());
                };
                let execution_id = request.execution_id.clone();
                let event = state.lock().unwrap().begin_execution(
                    generation, message.request_id.clone(), request,
                );
                if let Some(event) = event {
                    events.send(event)
                        .map_err(|_| "host event receiver closed".to_string())?;
                } else {
                    wire.send(envelope(message.request_id, Payload::ExecutionResult(ExecutionResult {
                        execution_id,
                        status: ExecutionStatus::Failed as i32,
                        traceback: None,
                        error: Some("instance_busy".into()),
                    }))).await.map_err(|error| error.to_string())?;
                }
            }
            outgoing = outbound.recv() => {
                let outgoing = outgoing.ok_or("outbound channel closed")?;
                // Host code may finish after reconnecting. Its reports belong
                // only to the connection that originally accepted the request.
                if outgoing.generation == generation {
                    wire.send(outgoing.envelope).await.map_err(|error| error.to_string())?;
                }
            }
        }
    }
}
