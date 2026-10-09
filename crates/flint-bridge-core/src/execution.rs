use crate::state::BridgeState;
use anyhow::Context;
use flint_contracts::protocol::{envelope::Payload, *};
use futures_util::SinkExt;
use serde::Serialize;
use std::convert::Infallible;
use std::sync::{Arc, Mutex, mpsc};
use tokio::{net::TcpStream, sync::mpsc as async_mpsc};

type Wire = flint_contracts::protocol::framing::Wire<TcpStream>;

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
    /// The step being run still owns continuation after prepare completes.
    Preparing,
    /// Preparation completion must post a new step to continue.
    Awaiting,
    Prepared {
        result_id: usize,
    },
    Running,
}

impl Stage {
    fn failure_code(self) -> FailureCode {
        if self == Self::Running {
            FailureCode::ExecutionFailed
        } else {
            FailureCode::PreparationFailed
        }
    }
}

/// An execution outcome reported as protocol data rather than a Rust error.
pub(crate) enum ExecutionFailure {
    Stopped,
    SchedulingRejected,
    /// Absent values take the defaults of the stage that failed.
    Host {
        code: Option<String>,
        message: Option<String>,
        traceback: Option<String>,
    },
}

impl ExecutionFailure {
    fn into_result(self, execution_id: String, stage: Stage) -> ExecutionResult {
        let failed = |error, traceback| ExecutionResult {
            execution_id,
            status: ExecutionStatus::Failed as i32,
            traceback,
            error: Some(error),
        };
        match self {
            Self::Stopped => failed(Failure::new(FailureCode::BridgeStopped), None),
            Self::SchedulingRejected => failed(Failure::new(FailureCode::SchedulingRejected), None),
            Self::Host {
                code,
                message,
                traceback,
            } => {
                let default = stage.failure_code();
                let error = Failure {
                    code: code.unwrap_or_else(|| default.as_str().into()),
                    message: message.unwrap_or_else(|| default.description().into()),
                };
                failed(error, traceback)
            }
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
        self.execution.as_mut().filter(|execution| execution.id == id)
    }

    /// Reports the terminal result after buffered output.
    pub(crate) fn finish_execution(&mut self, id: u64, result: Result<(), ExecutionFailure>) -> bool {
        let Some(stage) = self.execution_mut(id).map(|execution| execution.stage) else {
            return false;
        };
        self.flush_output();
        let execution = self.execution.take().unwrap();
        let execution_id = execution.request.execution_id;
        let result = match result {
            Ok(()) => ExecutionResult {
                execution_id,
                status: ExecutionStatus::Succeeded as i32,
                traceback: None,
                error: None,
            },
            Err(failure) => failure.into_result(execution_id, stage),
        };
        execution
            .outbound
            .send(envelope(execution.request.request_id, Payload::ExecutionResult(result)))
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
) -> anyhow::Result<Infallible> {
    // Each session owns its receiver; executions retain only this session's sender.
    let (outbound_tx, mut outbound_rx) = async_mpsc::unbounded_channel();
    loop {
        tokio::select! {
            incoming = read_envelope(&mut wire) => {
                let message = incoming?;
                let Some(Payload::HostExecuteRequest(request)) = message.payload else {
                    anyhow::bail!("unexpected execution message");
                };
                let execution_id = request.execution_id.clone();
                let admitted = state.lock().unwrap().begin_execution(
                    message.request_id.clone(), request, outbound_tx.clone(),
                );
                if let Some(id) = admitted {
                    schedule.send(id)
                        .context("execution dispatcher stopped")?;
                } else {
                    wire.send(envelope(message.request_id, Payload::ExecutionResult(ExecutionResult {
                        execution_id,
                        status: ExecutionStatus::Failed as i32,
                        traceback: None,
                        error: Some(Failure::new(FailureCode::InstanceBusy)),
                    }))).await?;
                }
            }
            outgoing = outbound_rx.recv() => {
                let outgoing = outgoing.context("outbound channel closed")?;
                wire.send(outgoing).await?;
            }
        }
    }
}
