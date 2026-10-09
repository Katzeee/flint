//! Built-in failure codes and their default descriptions. The wire code stays open to host extensions.
use super::Failure;
use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum FailureCode {
    InternalError,
    UnknownRequest,
    InvalidArguments,
    WorkflowNotFound,
    WorkflowUnreadable,
    ExecutionNotFound,
    InstanceOffline,
    InstanceBusy,
    BackendBusy,
    BackendStopping,
    BackendLocked,
    BackendUnavailable,
    ConnectionFailed,
    RegistrationRejected,
    ExecutionTimeout,
    ExecutionDisconnected,
    ExecutionInterrupted,
    ResultPersistenceFailed,
    PreparationFailed,
    ExecutionFailed,
    BridgeStopped,
    SchedulingRejected,
    CommandFailed,
    Interrupted,
    AttachFailed,
    AttachTimeout,
}

impl FailureCode {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::InternalError => "internal operation failed",
            Self::UnknownRequest => "not a supported request",
            Self::InvalidArguments => "invalid arguments",
            Self::WorkflowNotFound => "workflow not found",
            Self::WorkflowUnreadable => "workflow record cannot be read",
            Self::ExecutionNotFound => "execution not found",
            Self::InstanceOffline => "host is offline",
            Self::InstanceBusy => "instance is busy",
            Self::BackendBusy => "executions are still active",
            Self::BackendStopping => "backend is stopping",
            Self::BackendLocked => "another backend owns this runtime",
            Self::BackendUnavailable => "the backend is not running or did not respond",
            Self::ConnectionFailed => "connection failed",
            Self::RegistrationRejected => "the backend rejected the Bridge registration",
            Self::ExecutionTimeout => "execution response timed out; host code may still be running",
            Self::ExecutionDisconnected => "host disconnected; execution outcome is unknown",
            Self::ExecutionInterrupted => "backend interrupted; host execution outcome is unknown",
            Self::ResultPersistenceFailed => "could not persist the execution result",
            Self::PreparationFailed => "host preparation failed",
            Self::ExecutionFailed => "host execution failed",
            Self::BridgeStopped => "Bridge stopped before host execution",
            Self::SchedulingRejected => "host rejected execution scheduling",
            Self::CommandFailed => "command failed",
            Self::Interrupted => "stopped waiting; submitted host code may still be running",
            Self::AttachFailed => "could not attach the Bridge",
            Self::AttachTimeout => "the injected Bridge did not register before the timeout",
        }
    }
}

impl Failure {
    pub fn new(code: FailureCode) -> Self {
        Self::with_message(code, code.description())
    }

    pub fn with_message(code: FailureCode, message: impl Into<String>) -> Self {
        Self {
            code: code.as_str().into(),
            message: message.into(),
        }
    }

    /// Describes the failure by the error and each of its causes.
    pub fn caused_by(code: FailureCode, error: &(dyn Error + 'static)) -> Self {
        let mut message = error.to_string();
        let mut source = error.source();
        while let Some(cause) = source {
            message.push_str(": ");
            message.push_str(&cause.to_string());
            source = cause.source();
        }
        Self::with_message(code, message)
    }

    pub fn is(&self, code: FailureCode) -> bool {
        self.code == code.as_str()
    }
}

impl From<FailureCode> for Failure {
    fn from(code: FailureCode) -> Self {
        Self::new(code)
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl Error for Failure {}
