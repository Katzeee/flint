use crate::state::BridgeState;
use flint_contracts::protocol::{envelope::Payload, *};
use futures_util::SinkExt;
use serde::Serialize;
use std::convert::Infallible;
use std::sync::{mpsc, Arc, Mutex};
use tokio::{net::TcpStream, sync::mpsc as async_mpsc};

type Wire = flint_contracts::protocol::framing::Wire<TcpStream>;

pub(crate) const STOPPED: &str = "Bridge stopped before host execution";
pub(crate) const DROPPED: &str = "Host dropped the execution before it started";
const PREPARATION_FAILED: &str = "preparation_failed";
const EXECUTION_FAILED: &str = "execution_failed";
const OUTPUT_LIMIT: usize = 64 * 1024;

#[derive(Serialize)]
pub(crate) struct ExecutionRequest {
    request_id: String,
    workflow_id: String,
    execution_id: String,
    code: String,
    filename: Option<String>,
}

/// Execution progress, including who resumes it when preparation completes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Stage {
    Scheduled,
    /// The current ticket still owns continuation after prepare completes.
    Preparing,
    /// Preparation completion must schedule a new ticket to continue.
    Awaiting,
    Prepared {
        result_id: usize,
    },
    Running,
}

pub(crate) struct Failure {
    pub(crate) traceback: Option<String>,
    pub(crate) error: Option<String>,
}

impl Failure {
    pub(crate) fn new(error: &str) -> Self {
        Self {
            traceback: None,
            error: Some(error.into()),
        }
    }
}

pub(crate) struct Execution {
    pub(crate) id: u64,
    pub(crate) request: ExecutionRequest,
    pub(crate) stage: Stage,
    outbound: async_mpsc::UnboundedSender<Envelope>,
    sequence: u64,
    stdout: String,
    stderr: String,
}

impl Execution {
    pub(crate) fn new(
        id: u64,
        request_id: String,
        request: HostExecuteRequest,
        outbound: async_mpsc::UnboundedSender<Envelope>,
    ) -> Self {
        Self {
            id,
            request: ExecutionRequest {
                request_id,
                workflow_id: request.workflow_id,
                execution_id: request.execution_id,
                code: request.code,
                filename: request.filename,
            },
            stage: Stage::Scheduled,
            outbound,
            sequence: 0,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

impl BridgeState {
    pub(crate) fn execution_mut(&mut self, id: u64) -> Option<&mut Execution> {
        self.execution
            .as_mut()
            .filter(|execution| execution.id == id)
    }

    /// Reports the terminal result after any buffered output. A failure without
    /// its own error is classified by the stage that failed.
    pub(crate) fn finish_execution(&mut self, id: u64, result: Result<(), Failure>) -> bool {
        let Some(stage) = self.execution_mut(id).map(|execution| execution.stage) else {
            return false;
        };
        self.flush_output();
        let execution = self.execution.take().unwrap();
        let (status, traceback, error) = match result {
            Ok(()) => (ExecutionStatus::Succeeded, None, None),
            Err(failure) => (
                ExecutionStatus::Failed,
                failure.traceback,
                failure.error.or_else(|| {
                    Some(
                        if stage == Stage::Running {
                            EXECUTION_FAILED
                        } else {
                            PREPARATION_FAILED
                        }
                        .into(),
                    )
                }),
            ),
        };
        execution
            .outbound
            .send(envelope(
                execution.request.request_id,
                Payload::ExecutionResult(ExecutionResult {
                    execution_id: execution.request.execution_id,
                    status: status as i32,
                    traceback,
                    error,
                }),
            ))
            .is_ok()
    }

    /// Small writes are combined here; the network runtime flushes them on its own schedule.
    pub(crate) fn buffer_output(&mut self, id: u64, stdout: &str, stderr: &str) -> bool {
        if self.execution_mut(id).is_none() {
            return false;
        }
        for (mut text, is_error) in [(stdout, false), (stderr, true)] {
            while !text.is_empty() {
                let execution = self.execution.as_mut().unwrap();
                let available = OUTPUT_LIMIT - execution.stdout.len() - execution.stderr.len();
                let mut end = text.len().min(available);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                let buffer = if is_error {
                    &mut execution.stderr
                } else {
                    &mut execution.stdout
                };
                buffer.push_str(&text[..end]);
                text = &text[end..];
                if end == 0 || execution.stdout.len() + execution.stderr.len() == OUTPUT_LIMIT {
                    self.flush_output();
                }
            }
        }
        true
    }

    pub(crate) fn flush_output(&mut self) {
        let Some(execution) = self.execution.as_mut() else {
            return;
        };
        if execution.stdout.is_empty() && execution.stderr.is_empty() {
            return;
        }
        execution.sequence += 1;
        let _ = execution.outbound.send(envelope(
            execution.request.request_id.clone(),
            Payload::ExecutionOutputUpdate(ExecutionOutputUpdate {
                workflow_id: execution.request.workflow_id.clone(),
                execution_id: execution.request.execution_id.clone(),
                sequence: execution.sequence,
                stdout_delta: std::mem::take(&mut execution.stdout),
                stderr_delta: std::mem::take(&mut execution.stderr),
            }),
        ));
    }
}

pub(crate) async fn run_execution(
    mut wire: Wire,
    state: Arc<Mutex<BridgeState>>,
    schedule: mpsc::Sender<u64>,
) -> Result<Infallible, String> {
    // Each session owns its receiver; executions retain only this session's sender.
    let (outbound_tx, mut outbound_rx) = async_mpsc::unbounded_channel();
    loop {
        tokio::select! {
            incoming = read_envelope(&mut wire) => {
                let message = incoming.map_err(|error| error.to_string())?;
                let Some(Payload::HostExecuteRequest(request)) = message.payload else {
                    return Err("unexpected execution message".into());
                };
                let execution_id = request.execution_id.clone();
                let admitted = state.lock().unwrap().begin_execution(
                    message.request_id.clone(), request, outbound_tx.clone(),
                );
                if let Some(id) = admitted {
                    schedule.send(id)
                        .map_err(|_| "execution dispatcher stopped".to_string())?;
                } else {
                    wire.send(envelope(message.request_id, Payload::ExecutionResult(ExecutionResult {
                        execution_id,
                        status: ExecutionStatus::Failed as i32,
                        traceback: None,
                        error: Some("instance_busy".into()),
                    }))).await.map_err(|error| error.to_string())?;
                }
            }
            outgoing = outbound_rx.recv() => {
                let outgoing = outgoing.ok_or("outbound channel closed")?;
                wire.send(outgoing).await.map_err(|error| error.to_string())?;
            }
        }
    }
}
