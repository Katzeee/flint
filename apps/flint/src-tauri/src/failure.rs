//! Failure serialization at the application boundary, shared by CLI and Tauri.
use crate::attach::AttachError;
use flint_backend::BindError;
use flint_contracts::protocol::{Failure, FailureCode};
use flint_hosts::HostError;

/// Recovers the typed errors whose codes are known; anything else is a failed command.
pub fn from_error(error: anyhow::Error) -> Failure {
    recover::<Failure>(error, Into::into)
        .or_else(|error| recover::<AttachError>(error, Into::into))
        .or_else(|error| recover::<BindError>(error, Into::into))
        .or_else(|error| recover(error, host))
        .unwrap_or_else(|error| Failure::caused_by(FailureCode::CommandFailed, error.as_ref()))
}

fn recover<E>(error: anyhow::Error, code: fn(E) -> Failure) -> Result<Failure, anyhow::Error>
where
    E: std::fmt::Display + std::fmt::Debug + Send + Sync + 'static,
{
    error.downcast::<E>().map(code)
}

/// `flint-hosts` has no wire contract, so its errors are coded here.
pub fn host(error: HostError) -> Failure {
    let code = match error {
        HostError::NotAHost(_) => FailureCode::InvalidArguments,
        HostError::Platform(_) => FailureCode::CommandFailed,
    };
    Failure::caused_by(code, &error)
}
