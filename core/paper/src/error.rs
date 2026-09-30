//! Paper-operations failure type and its conversions.

use follon_control_plane::EngineError;

/// Paper-operations construction, adapter, accounting, or reconciliation failure.
#[derive(Debug)]
pub struct PaperError(pub String);

impl std::fmt::Display for PaperError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PaperError {}

impl From<EngineError> for PaperError {
    fn from(error: EngineError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DomainError> for PaperError {
    fn from(error: follon_domain::DomainError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DecimalError> for PaperError {
    fn from(error: follon_domain::DecimalError) -> Self {
        Self(error.0)
    }
}

impl From<follon_accounting::AccountingError> for PaperError {
    fn from(error: follon_accounting::AccountingError) -> Self {
        Self(error.0)
    }
}

impl From<follon_risk::RiskError> for PaperError {
    fn from(error: follon_risk::RiskError) -> Self {
        Self(error.0)
    }
}
