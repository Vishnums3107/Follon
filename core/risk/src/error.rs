//! Aggregate risk evaluation failure.

use std::fmt;

/// Aggregate risk evaluation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskError(pub String);

impl fmt::Display for RiskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RiskError {}

impl From<follon_domain::DecimalError> for RiskError {
    fn from(error: follon_domain::DecimalError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DomainError> for RiskError {
    fn from(error: follon_domain::DomainError) -> Self {
        Self(error.0)
    }
}
