//! Execution planning failure.

use std::fmt;

/// Execution planning failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionError(pub String);

impl fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ExecutionError {}

impl From<follon_domain::DecimalError> for ExecutionError {
    fn from(error: follon_domain::DecimalError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DomainError> for ExecutionError {
    fn from(error: follon_domain::DomainError) -> Self {
        Self(error.0)
    }
}
