//! Atomic multi-leg combination intents and net-price protection.

use crate::*;

/// Net-price protection for one atomic multi-leg combination.
///
/// This is a domain contract rather than an execution-planner detail: pre-trade
/// risk has to reason about the protected net price of a combination before any
/// plan exists, so the same type is used by `core/execution`'s planner, by the
/// risk gate, and by the environment services.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComboPriceLimit {
    /// Total debit per combination unit may not exceed this positive amount.
    MaximumDebit(Decimal),
    /// Total credit per combination unit may not fall below this positive amount.
    MinimumCredit(Decimal),
}

impl ComboPriceLimit {
    /// Stable wire representation of the protection kind.
    pub const fn kind(self) -> &'static str {
        match self {
            Self::MaximumDebit(_) => "MAXIMUM_DEBIT",
            Self::MinimumCredit(_) => "MINIMUM_CREDIT",
        }
    }

    /// The positive protection amount, whichever kind this is.
    pub const fn amount(self) -> Decimal {
        match self {
            Self::MaximumDebit(amount) | Self::MinimumCredit(amount) => amount,
        }
    }

    /// Checks a signed protected net price against this protection.
    ///
    /// The sign convention is fixed across the repository: a **positive** net
    /// price is a debit the account pays, a **negative** net price is a credit
    /// the account receives. A debit-protected combination that priced to a
    /// credit is refused rather than silently accepted, because an operator who
    /// asked for a maximum debit did not ask to be filled at an unreviewed
    /// credit — the economics are not the ones they approved.
    pub fn check_net_price(self, protected_net_price: Decimal) -> Result<(), DomainError> {
        match self {
            Self::MaximumDebit(limit) => {
                if limit <= Decimal::ZERO
                    || protected_net_price < Decimal::ZERO
                    || protected_net_price > limit
                {
                    return Err(DomainError(
                        "combination exceeds its maximum debit".to_owned(),
                    ));
                }
            }
            Self::MinimumCredit(limit) => {
                let credit = Decimal::ZERO.checked_sub(protected_net_price)?;
                if limit <= Decimal::ZERO || credit < limit {
                    return Err(DomainError(
                        "combination is below its minimum credit".to_owned(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// One ratio leg of a multi-leg combination intent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComboIntentLeg {
    /// Canonical instrument identity for this leg.
    pub instrument_id: String,
    /// Economic side for this leg.
    pub side: Side,
    /// Positive contracts per combination unit.
    pub ratio: u32,
    /// Positive protected leg price used to prove the net price.
    pub limit_price: Decimal,
}

/// A strategy or operator request for one atomic multi-leg combination.
///
/// This is the combination analogue of [`OrderIntent`], and exists for the same
/// reason: it is the *only* shape a risk gate is willing to assess. A
/// combination must never be decomposed into independently marketable legs —
/// either an adapter executes every leg atomically or it rejects the request
/// before transmitting any of them — so the risk gate assesses the combination
/// as a single economic unit and the OMS tracks it as a single order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComboIntent {
    /// Intent identity, and the OMS idempotency key it derives.
    pub intent_id: String,
    /// Target account identity.
    pub account_id: String,
    /// Originating strategy identity.
    pub strategy_id: String,
    /// Correlates the resulting causal chain.
    pub correlation_id: String,
    /// Exact ratio legs, executed as one atomic group.
    pub legs: Vec<ComboIntentLeg>,
    /// Positive number of combination units requested.
    pub combo_quantity: Decimal,
    /// Net-price protection for the whole combination.
    pub price_limit: ComboPriceLimit,
    /// Time in force for the combination as a unit.
    pub time_in_force: TimeInForce,
    /// Human-readable strategy rationale or signal reference.
    pub rationale: String,
    /// UTC creation time supplied by the replay clock.
    pub created_at: String,
    /// Immutable strategy-bundle version.
    pub strategy_version: String,
    /// Immutable configuration version.
    pub configuration_version: String,
    /// Requested execution environment.
    pub environment: String,
}

/// Inclusive leg-count bounds for one combination.
///
/// The lower bound is two because a one-leg "combination" is a plain order and
/// must take the plain order path, which has strictly more risk coverage. The
/// upper bound matches `core/execution`'s planner and the paper broker request.
pub const COMBO_LEG_BOUNDS: std::ops::RangeInclusive<usize> = 2..=16;

/// Inclusive bounds for a single leg's ratio.
pub const COMBO_LEG_RATIO_BOUNDS: std::ops::RangeInclusive<u32> = 1..=10_000;

impl ComboIntent {
    /// Validates every field required before risk can assess the combination.
    pub fn validate(&self) -> Result<(), DomainError> {
        for (name, value) in [
            ("combo intent_id", self.intent_id.as_str()),
            ("combo account_id", self.account_id.as_str()),
            ("combo strategy_id", self.strategy_id.as_str()),
            ("combo correlation_id", self.correlation_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        validate_utc_timestamp("combo intent created_at", &self.created_at)?;
        if self.combo_quantity <= Decimal::ZERO || self.rationale.is_empty() {
            return Err(DomainError(
                "combo intent quantity and rationale are required".to_owned(),
            ));
        }
        if !COMBO_LEG_BOUNDS.contains(&self.legs.len()) {
            return Err(DomainError(format!(
                "combo intent must contain between {} and {} legs",
                COMBO_LEG_BOUNDS.start(),
                COMBO_LEG_BOUNDS.end()
            )));
        }
        // Duplicate instruments are refused rather than netted. Netting them
        // here would change the economics the operator approved, and it would
        // also defeat the aggregate position projection the risk gate performs
        // per instrument.
        let mut seen: Vec<&str> = Vec::with_capacity(self.legs.len());
        for leg in &self.legs {
            validate_canonical_id("combo leg instrument_id", &leg.instrument_id)?;
            if seen.contains(&leg.instrument_id.as_str()) {
                return Err(DomainError(
                    "combo intent legs must reference distinct instruments".to_owned(),
                ));
            }
            seen.push(leg.instrument_id.as_str());
            if !COMBO_LEG_RATIO_BOUNDS.contains(&leg.ratio) {
                return Err(DomainError(format!(
                    "combo leg ratio must be between {} and {}",
                    COMBO_LEG_RATIO_BOUNDS.start(),
                    COMBO_LEG_RATIO_BOUNDS.end()
                )));
            }
            if leg.limit_price <= Decimal::ZERO {
                return Err(DomainError(
                    "combo leg limit price must be positive".to_owned(),
                ));
            }
        }
        if self.price_limit.amount() <= Decimal::ZERO {
            return Err(DomainError(
                "combo price limit must be a positive amount".to_owned(),
            ));
        }
        self.price_limit
            .check_net_price(self.protected_net_price()?)
    }

    /// The signed protected net price of one combination unit.
    ///
    /// Positive is a debit the account pays; negative is a credit it receives.
    pub fn protected_net_price(&self) -> Result<Decimal, DomainError> {
        let mut net = Decimal::ZERO;
        for leg in &self.legs {
            let ratio = Decimal::from_integer(i64::from(leg.ratio))?;
            let leg_net = leg.limit_price.checked_mul(ratio)?;
            net = match leg.side {
                Side::Buy => net.checked_add(leg_net)?,
                Side::Sell => net.checked_sub(leg_net)?,
            };
        }
        Ok(net)
    }

    /// The exact contract quantity for one leg, after applying its ratio.
    pub fn leg_quantity(&self, leg: &ComboIntentLeg) -> Result<Decimal, DomainError> {
        let ratio = Decimal::from_integer(i64::from(leg.ratio))?;
        Ok(self.combo_quantity.checked_mul(ratio)?)
    }

    /// The absolute gross notional the whole combination puts at risk.
    ///
    /// Every leg contributes its own magnitude regardless of side, because a
    /// two-sided combination still exposes the account to both legs until it
    /// is closed. A net-price view would understate that exposure, which is
    /// the exact failure mode an aggregate risk limit exists to prevent.
    pub fn gross_notional(&self) -> Result<Decimal, DomainError> {
        let mut gross = Decimal::ZERO;
        for leg in &self.legs {
            let quantity = self.leg_quantity(leg)?;
            gross = gross.checked_add(quantity.checked_mul(leg.limit_price)?)?;
        }
        Ok(gross)
    }

    /// The signed quantity this combination projects onto one instrument.
    pub fn projected_leg_delta(&self, leg: &ComboIntentLeg) -> Result<Decimal, DomainError> {
        let quantity = self.leg_quantity(leg)?;
        Ok(match leg.side {
            Side::Buy => quantity,
            Side::Sell => Decimal::ZERO.checked_sub(quantity)?,
        })
    }
}
