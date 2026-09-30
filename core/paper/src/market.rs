//! Point-in-time market observations for single orders and combinations.

use follon_domain::{validate_canonical_id, validate_utc_timestamp, ComboIntent, Decimal};

use crate::*;

/// Exact market observation required at the paper pre-trade boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperMarketData {
    /// Canonical instrument identity for this mark.
    pub instrument_id: String,
    /// Exact positive mark used to estimate notional and reserve cash.
    pub mark_price: Decimal,
    /// Canonical UTC observation time supplied by the market-data boundary.
    pub observed_at: String,
}

impl PaperMarketData {
    pub(crate) fn validate(&self) -> Result<(), PaperError> {
        validate_canonical_id("paper market instrument_id", &self.instrument_id)?;
        validate_utc_timestamp("paper market observation time", &self.observed_at)?;
        if self.mark_price <= Decimal::ZERO {
            return Err(PaperError(
                "paper market mark price must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

/// One mark per leg of a combination, evaluated as a single observation.
///
/// Kept as a collection of [`PaperMarketData`] rather than a new shape so the
/// durable journal record can reuse the existing per-mark serialisation without
/// a second format to migrate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperComboMarketData {
    /// Exactly one validated mark per combination leg, in leg order.
    pub marks: Vec<PaperMarketData>,
}

impl PaperComboMarketData {
    /// Validates the observation set against the combination it prices.
    ///
    /// Every leg must be quoted. A combination priced from a partial set would
    /// have to guess at least one leg's notional, and guessing is exactly what
    /// a pre-trade gate exists to prevent.
    pub fn validate_for(&self, intent: &ComboIntent) -> Result<(), PaperError> {
        if self.marks.len() != intent.legs.len() {
            return Err(PaperError(
                "paper combo observation must carry exactly one mark per leg".to_owned(),
            ));
        }
        for mark in &self.marks {
            mark.validate()?;
        }
        for leg in &intent.legs {
            if self.mark_for(&leg.instrument_id).is_none() {
                return Err(PaperError(format!(
                    "paper combo observation is missing a mark for {}",
                    leg.instrument_id
                )));
            }
        }
        Ok(())
    }

    /// The validated mark for one leg instrument, if present.
    pub fn mark_for(&self, instrument_id: &str) -> Option<&PaperMarketData> {
        self.marks
            .iter()
            .find(|mark| mark.instrument_id == instrument_id)
    }

    /// The oldest observation time across every leg.
    ///
    /// Freshness for a combination is the freshness of its *stalest* leg: the
    /// group executes atomically, so one stale leg makes the whole priced
    /// structure stale. Taking the newest would let a single fresh quote
    /// launder an arbitrarily old one beside it.
    pub fn oldest_observed_at(&self) -> Result<&str, PaperError> {
        self.marks
            .iter()
            .map(|mark| mark.observed_at.as_str())
            // Canonical second-precision UTC (`validate_utc_timestamp`) sorts
            // lexicographically in exactly timestamp order, so no parsing is
            // needed to find the oldest.
            .min()
            .ok_or_else(|| PaperError("paper combo observation is empty".to_owned()))
    }
}
