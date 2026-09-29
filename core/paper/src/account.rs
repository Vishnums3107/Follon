//! The PAPER account a service operates.

use follon_domain::{validate_canonical_id, Decimal};

use crate::*;

/// Explicit account configuration for the paper-only operational service.
#[derive(Clone, Debug)]
pub struct PaperAccount {
    /// Canonical account identity supplied to every broker request.
    pub account_id: String,
    /// Single reporting currency for this milestone.
    pub currency: String,
    /// Independent internal opening cash balance.
    pub initial_cash: Decimal,
    /// Must be the literal value `PAPER`.
    pub environment: String,
}

impl PaperAccount {
    /// Validates that an account cannot be accidentally configured for live execution.
    pub fn validate(&self) -> Result<(), PaperError> {
        validate_canonical_id("paper account_id", &self.account_id)?;
        if self.currency.len() != 3
            || !self
                .currency
                .bytes()
                .all(|character| character.is_ascii_uppercase())
            || self.initial_cash < Decimal::ZERO
            || self.environment != "PAPER"
        {
            return Err(PaperError(
                "paper account must use a currency, non-negative cash, and PAPER environment"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}
