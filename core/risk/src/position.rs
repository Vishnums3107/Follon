//! Marked positions, resting orders and candidate orders fed to the evaluator.

use follon_domain::{validate_canonical_id, Decimal, Side};
use follon_fx::{FxPricingSnapshot, FxValueDate};

use crate::*;

/// Fully attributed marked exposure contribution, filled or still working.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskPosition {
    /// Account identity.
    pub account_id: String,
    /// Strategy identity.
    pub strategy_id: String,
    /// Instrument identity.
    pub instrument_id: String,
    /// Stable asset-class label.
    pub asset_class: String,
    /// Stable sector or risk-bucket label.
    pub sector: String,
    /// Three-letter currency.
    pub currency: String,
    /// Signed quantity; shorts are negative.
    pub quantity: Decimal,
    /// Positive mark in instrument currency.
    pub mark_price: Decimal,
    /// Positive contract multiplier.
    pub multiplier: Decimal,
    /// Signed position delta in underlying units.
    pub delta: Decimal,
    /// Signed position gamma.
    pub gamma: Decimal,
}

impl RiskPosition {
    pub(crate) fn validate(&self) -> Result<(), RiskError> {
        for (name, value) in [
            ("risk account_id", &self.account_id),
            ("risk strategy_id", &self.strategy_id),
            ("risk instrument_id", &self.instrument_id),
            ("risk asset_class", &self.asset_class),
            ("risk sector", &self.sector),
        ] {
            validate_canonical_id(name, value)?;
        }
        if self.currency.len() != 3
            || !self.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            || self.mark_price <= Decimal::ZERO
            || self.multiplier <= Decimal::ZERO
        {
            return Err(RiskError("invalid marked risk position".to_owned()));
        }
        Ok(())
    }

    pub(crate) fn signed_exposure(&self) -> Result<Decimal, RiskError> {
        Ok(self
            .quantity
            .checked_mul(self.mark_price)?
            .checked_mul(self.multiplier)?)
    }
}

/// Existing working order used for order-rate and self-trade protection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestingOrder {
    /// Order identity.
    pub order_id: String,
    /// Account identity.
    pub account_id: String,
    /// Instrument identity.
    pub instrument_id: String,
    /// Resting side.
    pub side: Side,
}

/// Candidate order evaluated as part of the aggregate portfolio.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateOrder {
    /// Stable intent identity.
    pub intent_id: String,
    /// Account identity.
    pub account_id: String,
    /// Strategy identity.
    pub strategy_id: String,
    /// Instrument identity.
    pub instrument_id: String,
    /// Asset-class bucket.
    pub asset_class: String,
    /// Sector bucket.
    pub sector: String,
    /// Currency bucket.
    pub currency: String,
    /// Side.
    pub side: Side,
    /// Positive quantity.
    pub quantity: Decimal,
    /// Positive fresh mark.
    pub mark_price: Decimal,
    /// Positive multiplier.
    pub multiplier: Decimal,
    /// Candidate delta contribution.
    pub delta: Decimal,
    /// Candidate gamma contribution.
    pub gamma: Decimal,
}

impl CandidateOrder {
    pub(crate) fn validate(&self) -> Result<(), RiskError> {
        let position = RiskPosition {
            account_id: self.account_id.clone(),
            strategy_id: self.strategy_id.clone(),
            instrument_id: self.instrument_id.clone(),
            asset_class: self.asset_class.clone(),
            sector: self.sector.clone(),
            currency: self.currency.clone(),
            quantity: self.quantity,
            mark_price: self.mark_price,
            multiplier: self.multiplier,
            delta: self.delta,
            gamma: self.gamma,
        };
        validate_canonical_id("candidate intent_id", &self.intent_id)?;
        position.validate()?;
        if self.quantity <= Decimal::ZERO {
            return Err(RiskError("candidate quantity must be positive".to_owned()));
        }
        Ok(())
    }

    pub(crate) fn signed_exposure(&self) -> Result<Decimal, RiskError> {
        let unsigned = self
            .quantity
            .checked_mul(self.mark_price)?
            .checked_mul(self.multiplier)?;
        match self.side {
            Side::Buy => Ok(unsigned),
            Side::Sell => negate(unsigned),
        }
    }
}

/// Identifiers and bucket selection for one FX candidate created from frozen pricing evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FxRiskOrderIdentity {
    /// Stable intent identity.
    pub intent_id: String,
    /// Target account identity.
    pub account_id: String,
    /// Originating strategy identity.
    pub strategy_id: String,
    /// Canonical target FX instrument identity.
    pub instrument_id: String,
    /// Stable risk sector, normally `fx`.
    pub sector: String,
}

/// Explicit replay-time context for selecting an FX risk mark.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FxRiskPricingContext {
    /// Contract value date selected for the mark.
    pub value_date: FxValueDate,
    /// Explicit risk-evaluation timestamp from the replay or request context.
    pub as_of: String,
    /// Maximum accepted source-receive age.
    pub maximum_quote_age_seconds: i64,
}

/// A generic risk candidate with the exact FX price evidence that created it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FxRiskCandidate {
    /// Candidate supplied to the ordinary portfolio risk evaluator.
    pub candidate: CandidateOrder,
    /// Immutable market-data snapshot used to derive the candidate mark.
    pub pricing_snapshot_id: String,
    /// Immutable pricing/reference contract version.
    pub pricing_reference_version: String,
    /// Contract value date selected for the mark.
    pub value_date: FxValueDate,
}

impl FxRiskCandidate {
    /// Creates a candidate from an explicit-time FX price snapshot.
    ///
    /// The result contains no order transport. It must still be supplied to
    /// [`evaluate_portfolio_risk`] and then enter the normal Risk/OMS flow.
    pub fn from_pricing_snapshot(
        identity: FxRiskOrderIdentity,
        snapshot: &FxPricingSnapshot,
        side: Side,
        quantity: Decimal,
        multiplier: Decimal,
        pricing: FxRiskPricingContext,
    ) -> Result<Self, RiskError> {
        snapshot.validate().map_err(|error| RiskError(error.0))?;
        if identity.instrument_id != snapshot.instrument_id {
            return Err(RiskError(
                "FX risk candidate instrument does not match pricing evidence".to_owned(),
            ));
        }
        let mark_price = snapshot
            .midpoint_at(
                &pricing.value_date,
                &pricing.as_of,
                pricing.maximum_quote_age_seconds,
            )
            .map_err(|error| RiskError(error.0))?;
        if quantity <= Decimal::ZERO || multiplier <= Decimal::ZERO {
            return Err(RiskError(
                "FX risk candidate quantity and multiplier must be positive".to_owned(),
            ));
        }
        let absolute_delta = quantity.checked_mul(multiplier)?;
        let delta = match side {
            Side::Buy => absolute_delta,
            Side::Sell => negate(absolute_delta)?,
        };
        let candidate = CandidateOrder {
            intent_id: identity.intent_id,
            account_id: identity.account_id,
            strategy_id: identity.strategy_id,
            instrument_id: identity.instrument_id,
            asset_class: snapshot.product.risk_bucket().to_owned(),
            sector: identity.sector,
            currency: snapshot.pair.quote_currency().to_owned(),
            side,
            quantity,
            mark_price,
            multiplier,
            delta,
            gamma: Decimal::ZERO,
        };
        candidate.validate()?;
        Ok(Self {
            candidate,
            pricing_snapshot_id: snapshot.snapshot_id.clone(),
            pricing_reference_version: snapshot.reference_version.clone(),
            value_date: pricing.value_date,
        })
    }
}
