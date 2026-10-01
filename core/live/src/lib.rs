//! Controlled-live safety boundary, approvals, audited canary execution, and recovery evidence.
//!
//! This crate intentionally contains no concrete broker wire client and no credential source.
//! A deployment must supply both an audited [`LiveBrokerAdapter`] and a managed
//! [`SecretProvider`]. The core defaults to shadow mode, requires separate human
//! requester/approver identities for every canary order, and fails closed on any
//! audit-journal uncertainty.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use follon_accounting::{
    Currency, FxBook, MarginPolicy, MarginPosition, ShortTaxLot, TaxLot, TaxLotBook,
    TaxLotBookSnapshot, TaxLotSelection,
};
use follon_control_plane::{EngineError, OmsComboOrder, OmsOrder, Portfolio};
use follon_domain::{
    price_deviation_bps, reduces_position_with_working, validate_canonical_id,
    validate_utc_timestamp, ComboIntent, Decimal, Fill, OrderIntent, OrderState, RiskDecision,
    Side, TimeInForce,
};
use follon_instrument::{TradingCalendar, TradingSession};

mod combinations;
mod configuration;
mod corporate_actions;

pub use configuration::LiveConfiguration;
use corporate_actions::{restore_live_corporate_actions, PersistentLiveCorporateAction};
pub use corporate_actions::{LiveCorporateAction, LiveCorporateActionReceipt};

pub use combinations::{
    combo_intent_fingerprint, LiveBrokerComboExecution, LiveBrokerComboExecutionLeg,
    LiveBrokerComboLeg, LiveBrokerComboRequest, LiveComboMarketData, LiveComboOrder,
};
use follon_risk::{CandidateOrder, PortfolioRiskSnapshot, RestingOrder, RiskPosition};
use follon_secrets::{SecretMaterial, SecretProvider, SecretReference};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const LIVE_JOURNAL_SCHEMA_VERSION: u32 = 1;
const MAX_LIVE_JOURNAL_BYTES: u64 = 128 * 1024 * 1024;

/// Controlled-live configuration, authorization, persistence, or broker failure.
#[derive(Debug)]
pub struct LiveError(pub String);

impl std::fmt::Display for LiveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for LiveError {}

impl From<EngineError> for LiveError {
    fn from(error: EngineError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DomainError> for LiveError {
    fn from(error: follon_domain::DomainError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DecimalError> for LiveError {
    fn from(error: follon_domain::DecimalError) -> Self {
        Self(error.0)
    }
}

impl From<follon_secrets::SecretError> for LiveError {
    fn from(error: follon_secrets::SecretError) -> Self {
        Self(error.0)
    }
}

impl From<follon_accounting::AccountingError> for LiveError {
    fn from(error: follon_accounting::AccountingError) -> Self {
        Self(error.0)
    }
}

impl From<follon_risk::RiskError> for LiveError {
    fn from(error: follon_risk::RiskError) -> Self {
        Self(error.0)
    }
}

/// Explicitly bounded LIVE account. It is never constructed from an environment variable.
#[derive(Clone, Debug)]
pub struct LiveAccount {
    /// Canonical broker account identity.
    pub account_id: String,
    /// Single reporting currency for this controlled-live phase.
    pub currency: String,
    /// Independently tracked opening cash balance.
    pub initial_cash: Decimal,
    /// Hard ceiling on deployed live capital, independent of broker buying power.
    pub max_deployed_capital: Decimal,
    /// Must be the literal `LIVE` value.
    pub environment: String,
    /// Managed credential reference. Secret bytes never enter configuration or audit records.
    pub credential_reference: SecretReference,
}

impl LiveAccount {
    /// Validates the controlled-live account boundary.
    pub fn validate(&self) -> Result<(), LiveError> {
        validate_canonical_id("live account_id", &self.account_id)?;
        if self.currency.len() != 3
            || !self
                .currency
                .bytes()
                .all(|value| value.is_ascii_uppercase())
            || self.initial_cash < Decimal::ZERO
            || self.max_deployed_capital <= Decimal::ZERO
            || self.max_deployed_capital > self.initial_cash
            || self.environment != "LIVE"
        {
            return Err(LiveError(
                "live account must have uppercase currency, bounded capital, and LIVE environment"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

/// Operator-attested reference data the live OMS cannot otherwise derive:
/// there is no sector/asset-class taxonomy anywhere in this codebase, so
/// composing `core/risk`'s bucket checks requires the operator to supply one
/// directly, exactly like every other existing caller of
/// `follon_risk::evaluate_portfolio_risk` (the `follon-risk-benchmark` CLI and
/// the gRPC `EvaluatePortfolioRisk` RPC) already does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstrumentBucket {
    /// Stable asset-class label fed to the aggregate risk kernel.
    pub asset_class: String,
    /// Three-letter currency fed to the aggregate risk kernel.
    pub currency: String,
    /// Stable sector or risk-bucket label fed to the aggregate risk kernel.
    pub sector: String,
}

/// Composes `core/risk`'s aggregate portfolio kernel into the real live
/// order-gating path: gross/net exposure, leverage, concentration, and
/// sector/asset-class/currency bucket limits (Slice 1); drawdown, via a
/// durable peak-equity high-water-mark (Slice 2a); and daily loss, via a
/// durable session-start equity baseline (Slice 2b). Present only when an
/// operator has explicitly configured it; `evaluate_risk` is byte-for-byte
/// unchanged when this is `None`.
///
/// Margin-utilization and strategy-bucket limits are deliberately not
/// exposed here: `core/live` has no wired margin model, and `Portfolio` does
/// not attribute existing positions to a strategy, so those two specific
/// checks in `follon_risk::PortfolioRiskPolicy` would either be permanently
/// unreachable or actively misleading if wired in now. `evaluate_risk`
/// supplies fixed, non-configurable neutral values for exactly those fields
/// so they can never fire, rather than exposing operator-facing knobs that
/// silently do nothing. See
/// docs/06-delivery/14-master-plan-conformance-audit.md (row 5.7) for the
/// full boundary and the remaining Slice 2 scope.
#[derive(Clone, Debug, PartialEq)]
pub struct PortfolioRiskComposition {
    /// The aggregate policy's real, operator-configured bucket/exposure limits.
    pub policy: follon_risk::PortfolioRiskPolicy,
    /// Reference data for every instrument this account may hold or trade.
    /// An instrument missing from this map is treated as `"unclassified"` for
    /// asset class and sector, and as the account's own currency for
    /// currency -- an honest reflection of missing reference data, not a
    /// fabricated guess.
    pub instrument_buckets: BTreeMap<String, InstrumentBucket>,
    /// Slice-2c margin-utilization composition: an operator-authored initial/
    /// maintenance margin rate per asset class, reused from the same
    /// classification already required for bucket/exposure composition.
    /// `None` preserves `max_margin_utilization_bps`'s inert behavior exactly
    /// (`margin_used` stays fixed at zero). `core/live` positions and cash
    /// are always denominated in the account's own currency -- this codebase
    /// has no cross-currency position support -- so this deliberately does
    /// not expose a base currency or FX freshness window: valuation always
    /// converts within a single currency, which requires no FX quote at all.
    pub margin_rates: Option<BTreeMap<String, follon_accounting::MarginRate>>,
}

/// Immutable live risk and canary limits.
#[derive(Clone, Debug)]
pub struct LiveRiskPolicy {
    /// Immutable policy version recorded in every decision.
    pub version: String,
    /// Versioned session calendar governing the daily live gate.
    pub trading_calendar_id: String,
    /// Maximum one-order quantity.
    pub max_order_quantity: Decimal,
    /// Maximum one-order estimated notional.
    pub max_order_notional: Decimal,
    /// Maximum absolute limit-price distance from the fresh mark in basis points.
    pub max_price_deviation_bps: Decimal,
    /// Maximum one-order canary notional; this must not exceed `max_order_notional`.
    pub canary_max_order_notional: Decimal,
    /// Maximum number of live canary submissions since activation.
    pub canary_max_orders: u32,
    /// Maximum number of non-terminal orders.
    pub max_open_orders: usize,
    /// Maximum long-only position quantity per instrument.
    pub max_position_quantity: Decimal,
    /// Maximum aggregate realized loss before new live entries are blocked.
    pub max_realized_loss: Decimal,
    /// Maximum age of the exact market observation at decision time.
    pub max_market_data_age_seconds: u64,
    /// Maximum order submissions permitted within `order_rate_window_seconds`.
    pub max_order_rate: u32,
    /// Rolling window, in seconds, over which `max_order_rate` is enforced.
    pub order_rate_window_seconds: u64,
    /// Slice-1 aggregate portfolio-risk composition; `None` preserves today's
    /// behavior exactly.
    pub portfolio_risk: Option<PortfolioRiskComposition>,
    /// Operator permission for net short exposure; `None` refuses every short,
    /// which is the behavior every configuration had before this field existed.
    pub short_exposure: Option<ShortExposurePolicy>,
    /// Venue tick size per tradable instrument. A plain order for an
    /// instrument that is not listed is refused, and so is a limit price off
    /// its instrument's grid, before a broker can reject it.
    pub instrument_tick_sizes: BTreeMap<String, Decimal>,
    /// Venue lot size per tradable instrument: the exact quantity increment.
    /// A plain order or a combination leg whose quantity is not a whole
    /// number of lots is refused, and so is one on an unlisted instrument,
    /// before a broker can reject it.
    pub instrument_lot_sizes: BTreeMap<String, Decimal>,
}

impl LiveRiskPolicy {
    /// The tick-grid rejection for one plain order, if any.
    fn tick_rejection(
        &self,
        instrument_id: &str,
        limit_price: Option<Decimal>,
    ) -> Option<&'static str> {
        match self.instrument_tick_sizes.get(instrument_id) {
            None => Some("INSTRUMENT_TICK_SIZE_UNCONFIGURED"),
            Some(tick) => limit_price
                .is_some_and(|price| price.scaled() % tick.scaled() != 0)
                .then_some("LIMIT_PRICE_OFF_TICK_GRID"),
        }
    }

    /// The lot-size rejection for one order quantity, if any (E3.6c). A plain
    /// order's quantity and each combination leg's contract quantity meet
    /// this same rule.
    pub(crate) fn lot_rejection(
        &self,
        instrument_id: &str,
        quantity: Decimal,
    ) -> Option<&'static str> {
        match self.instrument_lot_sizes.get(instrument_id) {
            None => Some("INSTRUMENT_LOT_SIZE_UNCONFIGURED"),
            Some(lot) => {
                (quantity.scaled() % lot.scaled() != 0).then_some("ORDER_QUANTITY_OFF_LOT_SIZE")
            }
        }
    }

    /// Tick-grid rejections for a combination (E3.6b). Each leg meets exactly
    /// the rule a plain order on its instrument meets: it must be listed and
    /// its protected limit price must sit on its grid. The net price limit
    /// must then sit on the finest grid among the legs. That errs toward
    /// refusal; a venue's own complex-order increment is not modelled.
    pub(crate) fn combo_tick_rejections(&self, intent: &ComboIntent) -> Vec<&'static str> {
        let mut reasons: Vec<&'static str> = intent
            .legs
            .iter()
            .filter_map(|leg| self.tick_rejection(&leg.instrument_id, Some(leg.limit_price)))
            .collect();
        let finest = intent
            .legs
            .iter()
            .filter_map(|leg| self.instrument_tick_sizes.get(&leg.instrument_id))
            .min();
        if finest.is_some_and(|tick| intent.price_limit.amount().scaled() % tick.scaled() != 0) {
            reasons.push("COMBO_NET_PRICE_OFF_TICK_GRID");
        }
        reasons
    }

    /// Each leg's configured tick, for the decision evidence.
    pub(crate) fn combo_tick_evidence(&self, intent: &ComboIntent) -> String {
        render_leg_entries(intent, &self.instrument_tick_sizes)
    }

    /// Each leg's configured lot size, for the decision evidence.
    pub(crate) fn combo_lot_evidence(&self, intent: &ComboIntent) -> String {
        render_leg_entries(intent, &self.instrument_lot_sizes)
    }
}

/// Each combination leg's entry in a per-instrument reference table, in leg
/// order, with `UNCONFIGURED` for an instrument the table does not list.
fn render_leg_entries(intent: &ComboIntent, table: &BTreeMap<String, Decimal>) -> String {
    intent
        .legs
        .iter()
        .map(|leg| {
            format!(
                "{}:{}",
                leg.instrument_id,
                table
                    .get(&leg.instrument_id)
                    .map_or_else(|| "UNCONFIGURED".to_owned(), ToString::to_string)
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Operator permission for net short exposure at the controlled-live boundary.
///
/// Absent by default, and for the same reason it is absent by default in
/// `core/paper`: the short-sell guard exists because an uncovered short has
/// unbounded loss, and `core/live` holds no option reference data, so it cannot
/// *prove* that a combination's short leg is covered by its long one. It does
/// not assume it. Shorting stays refused unless an operator explicitly permits
/// it and states an absolute per-instrument bound.
///
/// This is deliberately a separate type from `follon_paper::ShortExposurePolicy`
/// rather than a shared one. The two environments are configured, reviewed and
/// approved independently, and permitting shorts in PAPER must never be capable
/// of permitting them with real capital as a side effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortExposurePolicy {
    /// Maximum absolute net short quantity permitted per instrument.
    pub max_short_quantity: Decimal,
}

impl LiveRiskPolicy {
    /// Whether a projected per-instrument position breaches this policy.
    ///
    /// Shared by the single-order and combination gates so a combination leg is
    /// judged by exactly the rule a plain order on the same instrument would
    /// meet -- neither stricter nor looser.
    fn breaches_position_limit(&self, projected: Decimal) -> Result<bool, LiveError> {
        if projected > self.max_position_quantity {
            return Ok(true);
        }
        if projected < Decimal::ZERO {
            return Ok(match self.short_exposure.as_ref() {
                None => true,
                Some(permission) => {
                    Decimal::ZERO.checked_sub(projected)? > permission.max_short_quantity
                }
            });
        }
        Ok(false)
    }
}

impl LiveRiskPolicy {
    /// Validates all non-negotiable live limits.
    pub fn validate(&self) -> Result<(), LiveError> {
        validate_canonical_id("live trading_calendar_id", &self.trading_calendar_id)?;
        let ten_thousand = Decimal::from_integer(10_000)?;
        if self.version.is_empty()
            || self.max_order_quantity <= Decimal::ZERO
            || self.max_order_notional <= Decimal::ZERO
            || self.max_price_deviation_bps < Decimal::ZERO
            || self.max_price_deviation_bps >= ten_thousand
            || self.canary_max_order_notional <= Decimal::ZERO
            || self.canary_max_order_notional > self.max_order_notional
            || self.canary_max_orders == 0
            || self.max_open_orders == 0
            || self.max_position_quantity <= Decimal::ZERO
            || self.max_realized_loss < Decimal::ZERO
            || self.max_market_data_age_seconds == 0
            || self.max_order_rate == 0
            || self.order_rate_window_seconds == 0
        {
            return Err(LiveError("invalid controlled-live risk policy".to_owned()));
        }
        // An empty table would refuse every order; that is a configuration
        // mistake, not a stricter policy.
        if self.instrument_tick_sizes.is_empty()
            || self
                .instrument_tick_sizes
                .iter()
                .any(|(instrument_id, tick)| {
                    validate_canonical_id("tick-size instrument_id", instrument_id).is_err()
                        || *tick <= Decimal::ZERO
                })
        {
            return Err(LiveError(
                "controlled-live risk policy needs a positive tick size per listed instrument"
                    .to_owned(),
            ));
        }
        // The same holds for the lot table (E3.6c).
        if self.instrument_lot_sizes.is_empty()
            || self
                .instrument_lot_sizes
                .iter()
                .any(|(instrument_id, lot)| {
                    validate_canonical_id("lot-size instrument_id", instrument_id).is_err()
                        || *lot <= Decimal::ZERO
                })
        {
            return Err(LiveError(
                "controlled-live risk policy needs a positive lot size per listed instrument"
                    .to_owned(),
            ));
        }
        // Each listed instrument needs both increments (E3.6f). One listed in
        // a single table would pass startup, then be refused at order time
        // with the other table's `..._UNCONFIGURED` code.
        if let Some(instrument_id) =
            unpaired_instrument(&self.instrument_tick_sizes, &self.instrument_lot_sizes)
        {
            return Err(LiveError(format!(
                "controlled-live risk policy lists {instrument_id} in only one of its tick and lot tables"
            )));
        }
        if self
            .short_exposure
            .as_ref()
            .is_some_and(|permission| permission.max_short_quantity <= Decimal::ZERO)
        {
            // `None` already expresses "no shorting", so a zero bound can only
            // be a configuration mistake, and fails closed rather than silently
            // agreeing with itself.
            return Err(LiveError(
                "live short-exposure permission must state a positive bound".to_owned(),
            ));
        }
        if let Some(composition) = &self.portfolio_risk {
            composition.policy.validate()?;
            for bucket in composition.instrument_buckets.values() {
                if validate_canonical_id("instrument bucket asset_class", &bucket.asset_class)
                    .is_err()
                    || validate_canonical_id("instrument bucket sector", &bucket.sector).is_err()
                    || bucket.currency.len() != 3
                    || !bucket
                        .currency
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase())
                {
                    return Err(LiveError(
                        "invalid instrument bucket reference data".to_owned(),
                    ));
                }
            }
            if let Some(rates) = &composition.margin_rates {
                if rates.is_empty() {
                    return Err(LiveError(
                        "configured margin_rates must not be empty".to_owned(),
                    ));
                }
                for (asset_class, rate) in rates {
                    if validate_canonical_id("margin rate asset_class", asset_class).is_err()
                        || rate.initial_bps > 10_000
                        || rate.maintenance_bps > rate.initial_bps
                        || rate.maintenance_bps == 0
                    {
                        return Err(LiveError("invalid margin rate".to_owned()));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Exact market observation evaluated at the controlled-live boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveMarketData {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Positive fixed-point mark.
    pub mark_price: Decimal,
    /// Canonical UTC observation time.
    pub observed_at: String,
}

impl LiveMarketData {
    fn validate(&self) -> Result<(), LiveError> {
        validate_canonical_id("live market instrument_id", &self.instrument_id)?;
        validate_utc_timestamp("live market observed_at", &self.observed_at)?;
        if self.mark_price <= Decimal::ZERO {
            return Err(LiveError("live market mark must be positive".to_owned()));
        }
        Ok(())
    }
}

/// Mode selected before an operational run begins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveRunMode {
    /// Records production-market decisions but cannot connect to or submit at a broker.
    Shadow,
    /// Allows bounded LIVE submissions only after connection and per-order approval.
    Canary,
}

impl LiveRunMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Shadow => "SHADOW",
            Self::Canary => "CANARY",
        }
    }
}

/// Time-bounded, four-eyes activation for one shadow or canary run.
#[derive(Clone, Debug)]
pub struct LiveActivation {
    /// Canonical immutable activation identity.
    pub activation_id: String,
    /// Shadow or canary scope.
    pub mode: LiveRunMode,
    /// SHA-256 fingerprint of the exact account and policy configuration.
    pub configuration_fingerprint: String,
    /// Operator requesting activation.
    pub requested_by: String,
    /// Different operator approving activation.
    pub approved_by: String,
    /// Canonical UTC activation time.
    pub activated_at: String,
    /// Canonical UTC expiry. New live work fails after this instant.
    pub expires_at: String,
}

/// Human approval material used to bind a controlled-live activation to immutable controls.
#[derive(Clone, Debug)]
pub struct LiveActivationRequest {
    /// Canonical immutable activation identity.
    pub activation_id: String,
    /// Shadow or canary scope.
    pub mode: LiveRunMode,
    /// Operator requesting activation.
    pub requested_by: String,
    /// Different operator approving activation.
    pub approved_by: String,
    /// Canonical UTC activation time.
    pub activated_at: String,
    /// Canonical UTC expiry.
    pub expires_at: String,
}

/// Four-eyes human authorization record for a controlled-live news canary order.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LiveNewsCanaryApproval {
    /// Canonical approval identity.
    pub approval_id: String,
    /// Human operator submitting the request.
    pub requested_by: String,
    /// Independent human operator verifying and approving the intent.
    pub approved_by: String,
    /// Associated news headline/sentiment event ID.
    pub news_event_id: String,
    /// Approved intent ID.
    pub intent_id: String,
    /// UTC signature timestamp.
    pub signed_at: String,
}

impl LiveNewsCanaryApproval {
    /// Validates four-eyes separation and canonical IDs.
    pub fn validate(&self) -> Result<(), LiveError> {
        validate_canonical_id("news approval_id", &self.approval_id)?;
        validate_canonical_id("news requested_by", &self.requested_by)?;
        validate_canonical_id("news approved_by", &self.approved_by)?;
        validate_canonical_id("news news_event_id", &self.news_event_id)?;
        validate_canonical_id("news intent_id", &self.intent_id)?;
        validate_utc_timestamp("news signed_at", &self.signed_at)?;
        if self.requested_by == self.approved_by {
            return Err(LiveError(
                "live news canary approval requires distinct requester and approver identities"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    /// Stable SHA-256 signature hash.
    pub fn signature_hash(&self) -> Result<String, LiveError> {
        self.validate()?;
        let mut hasher = Sha256::new();
        hasher.update(format!(
            "approval_id={}\nrequested_by={}\napproved_by={}\nnews_event_id={}\nintent_id={}\nsigned_at={}\n",
            self.approval_id, self.requested_by, self.approved_by, self.news_event_id, self.intent_id, self.signed_at
        ));
        Ok(format!("{:x}", hasher.finalize()))
    }
}

/// Managed macro event blackout window gate.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LiveMacroBlackoutWindow {
    /// Blackout window identity.
    pub window_id: String,
    /// Event taxonomy / description (e.g. CPI, FOMC).
    pub event_label: String,
    /// UTC start of blackout buffer.
    pub starts_at: String,
    /// UTC end of blackout buffer.
    pub ends_at: String,
}

impl LiveMacroBlackoutWindow {
    /// Validates window bounds.
    pub fn validate(&self) -> Result<(), LiveError> {
        validate_canonical_id("blackout window_id", &self.window_id)?;
        validate_utc_timestamp("blackout starts_at", &self.starts_at)?;
        validate_utc_timestamp("blackout ends_at", &self.ends_at)?;
        if self.event_label.trim().is_empty() || self.ends_at <= self.starts_at {
            return Err(LiveError(
                "blackout window requires non-empty label and ends_at after starts_at".to_owned(),
            ));
        }
        Ok(())
    }

    /// Returns true if `as_of_time` falls inside the blackout window.
    pub fn is_active_at(&self, as_of_time: &str) -> bool {
        self.starts_at.as_str() <= as_of_time && as_of_time <= self.ends_at.as_str()
    }
}

/// Evaluates real-time live execution news shock shield and returns reason codes.
pub fn evaluate_live_news_shock_shield(
    reference_price: Decimal,
    execution_price: Decimal,
    max_news_slippage_bps: Decimal,
    current_spread: Option<Decimal>,
    baseline_spread: Option<Decimal>,
    max_spread_multiplier_bps: Option<Decimal>,
) -> Result<Vec<String>, LiveError> {
    let mut reasons = Vec::new();
    let deviation = price_deviation_bps(reference_price, execution_price)?;
    if deviation > max_news_slippage_bps {
        reasons.push("LIVE_NEWS_SLIPPAGE_COLLAR_EXCEEDED".to_owned());
    }
    if let (Some(max_mult), Some(spread), Some(baseline)) =
        (max_spread_multiplier_bps, current_spread, baseline_spread)
    {
        if baseline > Decimal::ZERO {
            let max_allowed = baseline
                .checked_mul(max_mult)?
                .checked_div(Decimal::from_integer(10_000)?)?;
            if spread > max_allowed {
                reasons.push("LIVE_LIQUIDITY_HOLE_DETECTED".to_owned());
            }
        }
    }
    Ok(reasons)
}

impl LiveActivation {
    /// Creates an activation cryptographically bound to the supplied immutable live controls.
    ///
    /// This is the preferred deployment entry point: callers cannot accidentally copy a
    /// fingerprint from a different account, risk policy, or kill-switch revision.
    pub fn for_configuration(
        request: LiveActivationRequest,
        account: &LiveAccount,
        policy: &LiveRiskPolicy,
        kill_switches: &LiveKillSwitchRegistry,
    ) -> Result<Self, LiveError> {
        let activation = Self {
            activation_id: request.activation_id,
            mode: request.mode,
            configuration_fingerprint: configuration_fingerprint(account, policy, kill_switches),
            requested_by: request.requested_by,
            approved_by: request.approved_by,
            activated_at: request.activated_at,
            expires_at: request.expires_at,
        };
        activation.validate(&configuration_fingerprint(account, policy, kill_switches))?;
        Ok(activation)
    }

    fn validate(&self, expected_configuration_fingerprint: &str) -> Result<(), LiveError> {
        for (name, value) in [
            ("live activation_id", self.activation_id.as_str()),
            ("live activation requester", self.requested_by.as_str()),
            ("live activation approver", self.approved_by.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        validate_utc_timestamp("live activation time", &self.activated_at)?;
        validate_utc_timestamp("live activation expiry", &self.expires_at)?;
        if self.requested_by == self.approved_by
            || self.activated_at >= self.expires_at
            || self.configuration_fingerprint != expected_configuration_fingerprint
        {
            return Err(LiveError(
                "live activation requires distinct approver, valid interval, and exact configuration"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn active_at(&self, timestamp: &str) -> Result<bool, LiveError> {
        validate_utc_timestamp("live activation check time", timestamp)?;
        Ok(self.activated_at.as_str() <= timestamp && timestamp < self.expires_at.as_str())
    }
}

/// Four-eyes approval bound to exact intent bytes and configuration.
#[derive(Clone, Debug)]
pub struct LiveApproval {
    /// Canonical approval identity.
    pub approval_id: String,
    /// Intent identity being approved.
    pub intent_id: String,
    /// SHA-256 fingerprint of the exact intent.
    pub intent_fingerprint: String,
    /// Matching operational configuration fingerprint.
    pub configuration_fingerprint: String,
    /// Strategy/operator requester identity.
    pub requested_by: String,
    /// Human approval identity; it must differ from the requester.
    pub approved_by: String,
    /// Canonical UTC approval time.
    pub approved_at: String,
    /// Canonical UTC expiry; each approval is single-use and time bounded.
    pub expires_at: String,
}

impl LiveApproval {
    fn validate(&self, expected_configuration_fingerprint: &str) -> Result<(), LiveError> {
        for (name, value) in [
            ("live approval_id", self.approval_id.as_str()),
            ("live approval intent_id", self.intent_id.as_str()),
            ("live approval requester", self.requested_by.as_str()),
            ("live approval approver", self.approved_by.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        validate_utc_timestamp("live approval time", &self.approved_at)?;
        validate_utc_timestamp("live approval expiry", &self.expires_at)?;
        if self.requested_by == self.approved_by
            || self.approved_at >= self.expires_at
            || self.intent_fingerprint.len() != 64
            || !self
                .intent_fingerprint
                .bytes()
                .all(|value| value.is_ascii_hexdigit())
            || self.configuration_fingerprint != expected_configuration_fingerprint
        {
            return Err(LiveError(
                "live approval must bind exact configuration and use distinct approver".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Normalized LIVE broker submission request created only by the controlled OMS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveBrokerOrderRequest {
    /// OMS client idempotency key.
    pub client_order_id: String,
    /// Configured live account identity.
    pub account_id: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Requested side.
    pub side: Side,
    /// Exact requested quantity.
    pub quantity: Decimal,
    /// Optional limit price.
    pub limit_price: Option<Decimal>,
}

impl LiveBrokerOrderRequest {
    fn from_order(order: &OmsOrder) -> Self {
        Self {
            client_order_id: order.order_id.clone(),
            account_id: order.intent.account_id.clone(),
            instrument_id: order.intent.instrument_id.clone(),
            side: order.intent.side,
            quantity: order.intent.quantity,
            limit_price: order.intent.limit_price,
        }
    }

    fn validate(&self) -> Result<(), LiveError> {
        for (name, value) in [
            ("live broker client_order_id", self.client_order_id.as_str()),
            ("live broker account_id", self.account_id.as_str()),
            ("live broker instrument_id", self.instrument_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        if self.quantity <= Decimal::ZERO
            || self.limit_price.is_some_and(|value| value <= Decimal::ZERO)
        {
            return Err(LiveError("invalid live broker request".to_owned()));
        }
        Ok(())
    }
}

/// A price-only, risk-reducing modification of a working live limit order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveBrokerReplaceRequest {
    /// Immutable OMS client identity.
    pub client_order_id: String,
    /// Current broker-native order identity being superseded.
    pub previous_broker_order_id: String,
    /// New positive limit price.
    pub limit_price: Decimal,
}

impl LiveBrokerReplaceRequest {
    fn validate(&self) -> Result<(), LiveError> {
        validate_canonical_id("live replace client_order_id", &self.client_order_id)?;
        validate_canonical_id(
            "live replace previous_broker_order_id",
            &self.previous_broker_order_id,
        )?;
        if self.limit_price <= Decimal::ZERO {
            return Err(LiveError(
                "live replacement limit price must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Broker result for one idempotent live submission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveBrokerSubmitResult {
    /// Broker definitively accepted the client order identity.
    Acknowledged {
        /// Broker-native immutable order identity.
        broker_order_id: String,
    },
    /// Broker definitively refused the request.
    Rejected {
        /// Stable rejection reason safe for audit logs.
        reason: String,
    },
    /// Submission outcome cannot be proved until reconciliation.
    Unknown {
        /// Stable ambiguity reason safe for audit logs.
        reason: String,
    },
}

/// Normalized asynchronous evidence from an audited live broker adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveBrokerEvent {
    /// One complete atomic combination execution assembled by the adapter.
    ComboExecution(LiveBrokerComboExecution),
    /// Broker acknowledgement.
    Acknowledged {
        /// OMS client idempotency key.
        client_order_id: String,
        /// Broker-native immutable order identity.
        broker_order_id: String,
    },
    /// One exact broker execution.
    Execution {
        /// Broker execution identity.
        execution_id: String,
        /// OMS client idempotency key.
        client_order_id: String,
        /// Broker-native order identity.
        broker_order_id: String,
        /// Filled quantity.
        quantity: Decimal,
        /// Execution price.
        price: Decimal,
        /// Commission in account currency.
        fee: Decimal,
        /// Canonical UTC execution time.
        executed_at: String,
    },
    /// Broker cancellation confirmation.
    Cancelled {
        /// OMS client idempotency key.
        client_order_id: String,
        /// Stable cancellation reason.
        reason: String,
    },
    /// Broker rejected a requested cancellation; the order remains working.
    CancelRejected {
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Stable broker reason.
        reason: String,
    },
    /// Broker observed time-in-force expiry.
    Expired {
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Stable broker reason.
        reason: String,
    },
    /// Broker observed a modification request before its result is known.
    ReplaceRequested {
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Current broker order identity being replaced.
        previous_broker_order_id: String,
    },
    /// Broker accepted a modification and assigned a new native order identity.
    Replaced {
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Previous broker order identity.
        previous_broker_order_id: String,
        /// Replacement broker order identity.
        broker_order_id: String,
    },
    /// Broker rejected a modification; the prior broker version remains working.
    ReplaceRejected {
        /// OMS client idempotency identity.
        client_order_id: String,
        /// Stable broker reason.
        reason: String,
    },
    /// Broker rejection confirmation.
    Rejected {
        /// OMS client idempotency key.
        client_order_id: String,
        /// Stable rejection reason.
        reason: String,
    },
}

/// One order in an independent broker snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveBrokerOrderSnapshot {
    /// OMS client idempotency identity.
    pub client_order_id: String,
    /// Broker-native order identity.
    pub broker_order_id: String,
    /// Normalized broker lifecycle state.
    pub state: OrderState,
    /// Exact broker-reported filled quantity.
    pub filled_quantity: Decimal,
}

/// One position in an independent broker snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveBrokerPositionSnapshot {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Signed exact quantity reported by the broker.
    pub quantity: Decimal,
}

/// Independent broker view required for live reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveBrokerAccountSnapshot {
    /// Orders visible at broker.
    pub orders: Vec<LiveBrokerOrderSnapshot>,
    /// Positions visible at broker.
    pub positions: Vec<LiveBrokerPositionSnapshot>,
    /// Broker-reported cash in account currency.
    pub cash: Decimal,
}

/// What one controlled-LIVE broker adapter can carry to its venue.
///
/// The service consults this before it consumes an approval, spends a canary
/// slot or creates an order, so a request the adapter cannot carry is refused
/// with nothing recorded, nothing consumed and nothing transmitted. Without it
/// the refusal came back as a transport failure: the order became `UNKNOWN`,
/// the session disconnected, and the approval and the canary slot stayed spent
/// (delivery state E5.7).
///
/// The derived default is the narrowest set, single DAY orders. An adapter
/// declares anything more. It is a separate type from
/// `follon_paper::PaperBrokerCapabilities`: the environments are configured and
/// reviewed independently, and what PAPER may carry must never widen what
/// controlled LIVE is willing to attempt.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveBrokerCapabilities {
    /// Executes an atomic multi-leg combination as one order.
    pub combinations: bool,
    /// Carries a good-til-cancelled time in force to the venue as such,
    /// rather than dropping or rewriting it.
    pub good_til_cancelled: bool,
    /// Replaces a working order's limit price.
    pub replacement: bool,
}

/// Audited live broker interface. Implementations must be deployment-edge code.
pub trait LiveBrokerAdapter {
    /// Declares what this adapter can carry.
    ///
    /// The default is [`LiveBrokerCapabilities::default`], single DAY orders,
    /// so an adapter that declares nothing is never handed a combination, a
    /// GTC order or a replacement it would refuse, drop or rewrite.
    fn capabilities(&self) -> LiveBrokerCapabilities {
        LiveBrokerCapabilities::default()
    }
    /// Establishes a live session using secret bytes only at the adapter boundary.
    fn connect(&mut self, account_id: &str, credential: &SecretMaterial) -> Result<(), LiveError>;
    /// Submits one OMS-generated idempotent live order.
    fn submit(
        &mut self,
        request: &LiveBrokerOrderRequest,
    ) -> Result<LiveBrokerSubmitResult, LiveError>;
    /// Submits an atomic multi-leg combination.
    ///
    /// Only reached when [`Self::capabilities`] declares combinations. The
    /// default refuses. An adapter that cannot execute a combination
    /// atomically must reject the whole request *before* transmitting any leg:
    /// there is no acceptable partial outcome for a group whose legs only make
    /// sense together, and the OMS never works around this by splitting it.
    fn submit_combo(
        &mut self,
        _request: &LiveBrokerComboRequest,
    ) -> Result<LiveBrokerSubmitResult, LiveError> {
        Err(LiveError(
            "live broker adapter does not support native combinations".to_owned(),
        ))
    }
    /// Requests cancellation by client idempotency identity.
    fn cancel(&mut self, client_order_id: &str) -> Result<(), LiveError>;
    /// Requests a price-only replacement. The result arrives through [`LiveBrokerEvent`].
    ///
    /// Only reached when [`Self::capabilities`] declares replacement.
    fn replace(&mut self, request: &LiveBrokerReplaceRequest) -> Result<(), LiveError> {
        request.validate()?;
        Err(LiveError(
            "live broker adapter does not support order replacement".to_owned(),
        ))
    }
    /// Drains normalized asynchronous broker evidence.
    fn poll(&mut self) -> Result<Vec<LiveBrokerEvent>, LiveError>;
    /// Gets independent broker state for reconciliation.
    fn snapshot(&mut self, account_id: &str) -> Result<LiveBrokerAccountSnapshot, LiveError>;
    /// Re-establishes a previously configured live session after transport loss.
    fn reconnect(&mut self, account_id: &str, credential: &SecretMaterial)
        -> Result<(), LiveError>;
}

/// Scope for an independently operated controlled-live kill switch.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LiveKillSwitchScope {
    /// Stops every new controlled-live order.
    Global,
    /// Stops one account.
    Account(String),
    /// Stops one strategy.
    Strategy(String),
    /// Stops one instrument.
    Instrument(String),
}

impl LiveKillSwitchScope {
    /// Stable audit and dashboard identity.
    pub fn as_key(&self) -> String {
        match self {
            Self::Global => "global".to_owned(),
            Self::Account(value) => format!("account:{value}"),
            Self::Strategy(value) => format!("strategy:{value}"),
            Self::Instrument(value) => format!("instrument:{value}"),
        }
    }

    /// Parses a scope from its stable key, the inverse of [`Self::as_key`]:
    /// `global`, `account:<id>`, `strategy:<id>` or `instrument:<id>`, with a
    /// canonical identity.
    pub fn from_key(value: &str) -> Result<Self, LiveError> {
        if value == "global" {
            return Ok(Self::Global);
        }
        for (prefix, constructor) in [
            ("account:", Self::Account as fn(String) -> Self),
            ("strategy:", Self::Strategy as fn(String) -> Self),
            ("instrument:", Self::Instrument as fn(String) -> Self),
        ] {
            if let Some(identifier) = value.strip_prefix(prefix) {
                let scope = constructor(identifier.to_owned());
                scope.validate()?;
                return Ok(scope);
            }
        }
        Err(LiveError(
            "kill-switch scope must be global, account:<id>, strategy:<id> or instrument:<id>"
                .to_owned(),
        ))
    }

    fn validate(&self) -> Result<(), LiveError> {
        match self {
            Self::Global => Ok(()),
            Self::Account(value) => {
                validate_canonical_id("live kill account", value).map_err(Into::into)
            }
            Self::Strategy(value) => {
                validate_canonical_id("live kill strategy", value).map_err(Into::into)
            }
            Self::Instrument(value) => {
                validate_canonical_id("live kill instrument", value).map_err(Into::into)
            }
        }
    }
}

/// Versioned independent live kill-switch registry.
#[derive(Clone, Debug)]
pub struct LiveKillSwitchRegistry {
    /// Immutable registry revision.
    pub version: String,
    active: BTreeSet<LiveKillSwitchScope>,
}

impl LiveKillSwitchRegistry {
    /// Creates an empty registry with an immutable revision.
    pub fn new(version: impl Into<String>) -> Result<Self, LiveError> {
        let registry = Self {
            version: version.into(),
            active: BTreeSet::new(),
        };
        if registry.version.is_empty() {
            return Err(LiveError("live kill-switch version is required".to_owned()));
        }
        Ok(registry)
    }

    /// Activates one scope independently of strategy and broker health.
    pub fn activate(&mut self, scope: LiveKillSwitchScope) -> Result<bool, LiveError> {
        scope.validate()?;
        Ok(self.active.insert(scope))
    }

    /// Explicitly deactivates one scope.
    pub fn deactivate(&mut self, scope: &LiveKillSwitchScope) -> bool {
        self.active.remove(scope)
    }

    /// Lists active scopes deterministically.
    pub fn active_keys(&self) -> Vec<String> {
        self.active
            .iter()
            .map(LiveKillSwitchScope::as_key)
            .collect()
    }

    fn rejection_reasons(&self, intent: &OrderIntent) -> Vec<String> {
        self.reasons_for(
            &intent.account_id,
            &intent.strategy_id,
            std::slice::from_ref(&intent.instrument_id),
        )
    }

    /// Kill-switch reasons for a combination.
    ///
    /// An instrument switch on *any* leg halts the whole combination. There is
    /// no partial execution to fall back on -- the group is atomic -- so a
    /// single halted leg halts the structure.
    fn combo_rejection_reasons(&self, intent: &ComboIntent) -> Vec<String> {
        let instruments = intent
            .legs
            .iter()
            .map(|leg| leg.instrument_id.clone())
            .collect::<Vec<_>>();
        self.reasons_for(&intent.account_id, &intent.strategy_id, &instruments)
    }

    fn reasons_for(
        &self,
        account_id: &str,
        strategy_id: &str,
        instrument_ids: &[String],
    ) -> Vec<String> {
        let mut scopes = vec![
            LiveKillSwitchScope::Global,
            LiveKillSwitchScope::Account(account_id.to_owned()),
            LiveKillSwitchScope::Strategy(strategy_id.to_owned()),
        ];
        scopes.extend(
            instrument_ids
                .iter()
                .map(|instrument_id| LiveKillSwitchScope::Instrument(instrument_id.clone())),
        );
        let mut reasons = scopes
            .iter()
            .filter(|scope| self.active.contains(*scope))
            .map(|scope| {
                format!(
                    "KILL_SWITCH_{}",
                    scope.as_key().to_ascii_uppercase().replace(':', "_")
                )
            })
            .collect::<Vec<_>>();
        reasons.dedup();
        reasons
    }
}

/// Durable internal OMS record for one canary order.
#[derive(Clone, Debug)]
pub struct LiveOrder {
    /// Legal OMS lifecycle state.
    pub oms: OmsOrder,
    /// Single-use approval consumed by this order.
    pub approval_id: String,
    /// Exact market observation used for risk and cash reservation.
    pub market: LiveMarketData,
    /// Immutable risk outcome that authorized this exact order.
    pub decision: LiveRiskDecision,
    /// Broker identity after it becomes known.
    pub broker_order_id: Option<String>,
    /// Every broker-native order identity issued for this immutable OMS order.
    pub broker_order_versions: Vec<String>,
    /// State to restore after a replacement acceptance or rejection.
    replace_return_state: Option<OrderState>,
    /// Exact independently-accounted fill quantity.
    pub filled_quantity: Decimal,
}

impl LiveOrder {
    fn working(&self) -> bool {
        !matches!(
            self.oms.state,
            OrderState::RiskRejected
                | OrderState::Filled
                | OrderState::Cancelled
                | OrderState::Rejected
                | OrderState::Expired
        )
    }

    fn reserved_cash(&self) -> Result<Decimal, LiveError> {
        if !self.working() || self.oms.intent.side != Side::Buy {
            return Ok(Decimal::ZERO);
        }
        self.oms
            .intent
            .quantity
            .checked_sub(self.filled_quantity)?
            .checked_mul(self.market.mark_price)
            .map_err(Into::into)
    }
}

/// Registered approval and whether it has already authorized one canary submission.
#[derive(Clone, Debug)]
pub struct RegisteredLiveApproval {
    /// Immutable approval material.
    pub approval: LiveApproval,
    /// Single-use replay guard.
    pub consumed: bool,
}

/// Live risk decision emitted before any canary OMS order exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveRiskDecision {
    /// Stable decision identity.
    pub decision_id: String,
    /// Approved state.
    pub approved: bool,
    /// Machine-readable outcomes.
    pub reason_codes: Vec<String>,
    /// Policy revision evaluated.
    pub policy_version: String,
    /// UTC decision time.
    pub decided_at: String,
    /// Exact market observation fingerprint.
    pub market_fingerprint: String,
    /// Exact evaluated inputs and thresholds retained for explanations and audit.
    pub evaluated_limits: String,
}

/// Result of a shadow recording or controlled-live canary submission.
#[derive(Clone, Debug)]
pub enum LiveSubmitOutcome {
    /// A production-data shadow decision was recorded and never reached a broker.
    ShadowRecorded {
        /// Risk result recorded in the audit chain.
        decision: LiveRiskDecision,
    },
    /// A canary risk rejection created no broker order.
    RiskRejected {
        /// Rejection result recorded in the audit chain.
        decision: LiveRiskDecision,
    },
    /// A canary order was attempted under one consumed approval.
    CanaryOrder {
        /// Original risk result bound to this order.
        decision: LiveRiskDecision,
        /// OMS order identity.
        order_id: String,
        /// Current OMS state.
        state: OrderState,
    },
}

/// One immutable reconciliation difference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveReconciliationIssue {
    /// Stable issue identity.
    pub incident_id: String,
    /// Machine-readable discrepancy category.
    pub category: String,
    /// Canonical order, instrument, or account subject.
    pub subject: String,
    /// Deterministic comparison detail.
    pub detail: String,
}

/// Independent broker reconciliation result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveReconciliationReport {
    /// Stable reconciliation identity.
    pub reconciliation_id: String,
    /// UTC reconciliation completion time.
    pub reconciled_at: String,
    /// Differences which were observed at this checkpoint.
    pub issues: Vec<LiveReconciliationIssue>,
}

impl LiveReconciliationReport {
    /// Whether all independent broker and internal values agreed.
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

/// A discrepancy remains blocking until it has an explicit accountable explanation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveIncident {
    /// Original observed issue.
    pub issue: LiveReconciliationIssue,
    /// An attributable operator explanation, if one has been recorded.
    pub explanation: Option<String>,
}

impl LiveIncident {
    fn unexplained(&self) -> bool {
        self.explanation.is_none()
    }
}

/// Measured 60-small-capital-live-day gate state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LivePromotionStatus {
    /// Number of distinct clean closed live sessions.
    pub clean_live_days: u32,
    /// Required controlled-live sessions, always sixty.
    pub required_live_days: u32,
    /// Outstanding incidents without an explanation.
    pub unresolved_incidents: u32,
    /// Whether the service has journal evidence for all recorded state changes.
    pub complete_auditability: bool,
    /// Whether the measured gate is complete.
    pub eligible_for_next_gate: bool,
}

/// Read-only controlled-live monitoring projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LiveMonitoringDashboard {
    /// Schema version.
    pub dashboard_schema_version: u32,
    /// Always `LIVE` for this projection.
    pub environment: String,
    /// `SHADOW` or `CANARY`.
    pub mode: String,
    /// Canonical account ID.
    pub account_id: String,
    /// Exact configuration fingerprint.
    pub configuration_fingerprint: String,
    /// Whether an adapter session is currently established.
    pub broker_connected: bool,
    /// Whether durable audit writes have stayed healthy in this process.
    pub audit_healthy: bool,
    /// Current append-only audit sequence number.
    pub audit_sequence: u64,
    /// SHA-256 hash at the head of the audit chain.
    pub audit_head_hash: String,
    /// Active safety controls.
    pub active_kill_switches: Vec<String>,
    /// Number of non-terminal live orders.
    pub working_orders: u32,
    /// Number of ambiguous orders requiring reconciliation.
    pub unknown_orders: u32,
    /// Unexplained reconciliation incident count.
    pub unresolved_incidents: u32,
    /// Latest reconciliation time, if any.
    pub last_reconciled_at: Option<String>,
    /// Cleanliness of latest reconciliation, if any.
    pub last_reconciliation_clean: Option<bool>,
    /// Measured clean controlled-live days.
    pub clean_live_days: u32,
    /// Required controlled-live days.
    pub required_live_days: u32,
    /// Whether the next gate is eligible.
    pub promotion_eligible: bool,
    /// Whether every retained live-state transition is covered by the durable audit chain.
    pub complete_auditability: bool,
    /// Exact internal cash.
    pub internal_cash: String,
    /// Current position rows, deterministic by instrument.
    pub positions: Vec<LiveMonitoringPosition>,
}

/// One monitored internal live position.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LiveMonitoringPosition {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Exact quantity.
    pub quantity: String,
    /// Exact average cost.
    pub average_cost: String,
    /// Exact realized P&L.
    pub realized_pnl: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LiveJournalRecord {
    schema_version: u32,
    sequence: u64,
    previous_hash: String,
    event_type: String,
    occurred_at: String,
    actor: String,
    correlation_id: String,
    state: PersistentLiveState,
    entry_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentLiveState {
    configuration_fingerprint: String,
    account_id: String,
    currency: String,
    cash: String,
    orders: BTreeMap<String, PersistentLiveOrder>,
    approvals: BTreeMap<String, PersistentLiveApproval>,
    positions: BTreeMap<String, PersistentPosition>,
    execution_ids: Vec<String>,
    active_kill_switches: Vec<String>,
    incidents: BTreeMap<String, PersistentIncident>,
    live_days: BTreeMap<String, PersistentLiveDay>,
    canary_submissions: u32,
    next_reconciliation: u64,
    last_reconciled_at: Option<String>,
    last_reconciliation_clean: Option<bool>,
    latest_reconciliation: Option<PersistentReconciliationReport>,
    /// Missing on a journal written before this field existed; an empty
    /// book is exactly correct for one, since no fill could have been
    /// applied to a tax-lot ledger that did not yet exist.
    #[serde(default)]
    tax_lots: PersistentTaxLotBook,
    /// Last observed mark per instrument (Decimal-as-string, matching every
    /// other persisted decimal field). Missing on a journal written before
    /// this field existed; an empty map is correct for one -- every position
    /// simply falls back to its own average cost until re-quoted.
    #[serde(default)]
    marks: BTreeMap<String, String>,
    /// Highest observed real equity (Decimal-as-string). `None` on a journal
    /// written before this field existed; `restore()` bootstraps it to the
    /// account's real equity computed from the rest of the just-restored
    /// state, which is the honest value for a peak that was never tracked
    /// before now.
    #[serde(default)]
    peak_equity: Option<String>,
    /// UTC calendar date of the current session-start daily-loss baseline
    /// (Decimal-as-string equity paired below). `None` on a journal written
    /// before this field existed, or before any risk evaluation has ever run.
    #[serde(default)]
    daily_baseline_date: Option<String>,
    /// Equity observed at the first risk evaluation of `daily_baseline_date`
    /// (Decimal-as-string). Present if and only if `daily_baseline_date` is.
    #[serde(default)]
    daily_baseline_equity: Option<String>,
    /// Net signed per-strategy contribution to each instrument
    /// (`instrument_id -> strategy_id -> quantity`, Decimal-as-string).
    /// Missing or absent entries on a journal written before this field
    /// existed are correct as empty: no fill could have been attributed to a
    /// strategy-attribution ledger that did not yet exist.
    #[serde(default)]
    strategy_attribution: BTreeMap<String, BTreeMap<String, String>>,
    /// Atomic multi-leg combination orders. Absent in a document written
    /// before the combination path existed; an empty map is exactly correct
    /// for one. As in `core/paper`, this default makes the persisted *type*
    /// tolerant and does not on its own let an older journal **file** reopen --
    /// the reader also requires each line to re-serialize byte-for-byte.
    #[serde(default)]
    combo_orders: BTreeMap<String, PersistentLiveComboOrder>,
    /// Operator-attested corporate actions, in the order applied (E8.4b). Never
    /// written while empty, so a journal without one re-serializes byte-for-byte.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    corporate_actions: Vec<PersistentLiveCorporateAction>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct PersistentTaxLotBook {
    lots: BTreeMap<String, Vec<PersistentTaxLot>>,
    applied_lot_ids: Vec<String>,
    applied_disposal_ids: Vec<String>,
    realized_by_currency: BTreeMap<String, String>,
    #[serde(default)]
    short_lots: BTreeMap<String, Vec<PersistentTaxLot>>,
    #[serde(default)]
    applied_short_lot_ids: Vec<String>,
    #[serde(default)]
    applied_cover_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentTaxLot {
    lot_id: String,
    opened_at: String,
    remaining_quantity: String,
    unit_cost: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentReconciliationReport {
    reconciliation_id: String,
    reconciled_at: String,
    issues: Vec<PersistentReconciliationIssue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentReconciliationIssue {
    incident_id: String,
    category: String,
    subject: String,
    detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentLiveComboOrder {
    intent: PersistentComboIntent,
    approval_id: String,
    state: String,
    /// One persisted mark per leg, reusing the single-instrument observation
    /// shape rather than inventing a second format to migrate.
    market: Vec<PersistentMarketData>,
    decision: PersistentRiskDecision,
    broker_order_id: Option<String>,
    #[serde(default)]
    broker_order_versions: Vec<String>,
    filled_quantity: String,
    #[serde(default)]
    executions: Vec<PersistentLiveComboExecution>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentLiveComboExecution {
    execution_id: String,
    client_order_id: String,
    broker_order_id: String,
    units: String,
    legs: Vec<PersistentLiveComboExecutionLeg>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentLiveComboExecutionLeg {
    execution_id: String,
    instrument_id: String,
    side: String,
    quantity: String,
    price: String,
    fee: String,
    executed_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentComboIntent {
    intent_id: String,
    account_id: String,
    strategy_id: String,
    correlation_id: String,
    legs: Vec<PersistentComboLeg>,
    combo_quantity: String,
    /// `MAXIMUM_DEBIT` or `MINIMUM_CREDIT`, matching `ComboPriceLimit::kind`.
    price_limit_kind: String,
    price_limit_amount: String,
    time_in_force: String,
    rationale: String,
    created_at: String,
    strategy_version: String,
    configuration_version: String,
    environment: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentComboLeg {
    instrument_id: String,
    side: String,
    ratio: u32,
    limit_price: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentLiveOrder {
    intent: PersistentIntent,
    approval_id: String,
    state: String,
    market: PersistentMarketData,
    decision: PersistentRiskDecision,
    broker_order_id: Option<String>,
    #[serde(default)]
    broker_order_versions: Vec<String>,
    #[serde(default)]
    replace_return_state: Option<String>,
    filled_quantity: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentRiskDecision {
    decision_id: String,
    approved: bool,
    reason_codes: Vec<String>,
    policy_version: String,
    decided_at: String,
    market_fingerprint: String,
    evaluated_limits: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentLiveApproval {
    approval: PersistentApproval,
    consumed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentApproval {
    approval_id: String,
    intent_id: String,
    intent_fingerprint: String,
    configuration_fingerprint: String,
    requested_by: String,
    approved_by: String,
    approved_at: String,
    expires_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentIntent {
    intent_id: String,
    account_id: String,
    strategy_id: String,
    instrument_id: String,
    correlation_id: String,
    side: String,
    quantity: String,
    order_type: String,
    limit_price: Option<String>,
    time_in_force: String,
    rationale: String,
    created_at: String,
    strategy_version: String,
    configuration_version: String,
    environment: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentMarketData {
    instrument_id: String,
    mark_price: String,
    observed_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentPosition {
    quantity: String,
    average_cost: String,
    realized_pnl: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistentIncident {
    category: String,
    subject: String,
    detail: String,
    explanation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct PersistentLiveDay {
    calendar_id: String,
    session_opens_at: String,
    session_closes_at: String,
    clean: bool,
    audit_head_hash: String,
}

/// Append-only, hash-chained, process-exclusive controlled-live audit journal.
pub struct LiveAuditJournal {
    path: PathBuf,
    file: File,
    next_sequence: u64,
    previous_hash: String,
    latest: Option<PersistentLiveState>,
}

impl LiveAuditJournal {
    /// Opens and validates the full journal before any controlled-live action is allowed.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LiveError> {
        let path = path.as_ref().to_path_buf();
        // `symlink_metadata` never follows a link, so a dangling one is
        // refused too. The `exists()` check this replaced followed it, found
        // nothing, and the open below created the journal at its target
        // (E3.11).
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(LiveError(
                    "live audit journal path must not be a symbolic link".to_owned(),
                ));
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(LiveError(error.to_string()));
            }
            _ => {}
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| LiveError(error.to_string()))?;
        }
        let mut file =
            follon_file_safety::open(&path, follon_file_safety::Access::ReadAppend, true)
                .map_err(|error| LiveError(error.to_string()))?;
        file.try_lock_exclusive().map_err(|error| {
            LiveError(format!(
                "live audit journal is already open by another process: {error}"
            ))
        })?;
        let mut latest = None;
        let mut next_sequence = 1;
        let mut previous_hash = "0".repeat(64);
        let metadata = file
            .metadata()
            .map_err(|error| LiveError(error.to_string()))?;
        if metadata.len() > MAX_LIVE_JOURNAL_BYTES {
            return Err(LiveError(format!(
                "live audit journal exceeds {} bytes; archive and verify it before recovery",
                MAX_LIVE_JOURNAL_BYTES
            )));
        }
        if metadata.len() > 0 {
            let mut contents = String::new();
            file.read_to_string(&mut contents)
                .map_err(|error| LiveError(error.to_string()))?;
            for (index, line) in contents.lines().enumerate() {
                if line.is_empty() {
                    return Err(LiveError(format!(
                        "live journal has an empty line at {}",
                        index + 1
                    )));
                }
                let record: LiveJournalRecord = serde_json::from_str(line).map_err(|error| {
                    LiveError(format!("invalid live journal line {}: {error}", index + 1))
                })?;
                if serde_json::to_string(&record).map_err(|error| LiveError(error.to_string()))?
                    != line
                {
                    return Err(LiveError(format!(
                        "live journal line {} is not canonical JSON",
                        index + 1
                    )));
                }
                if record.schema_version != LIVE_JOURNAL_SCHEMA_VERSION
                    || record.sequence != next_sequence
                    || record.previous_hash != previous_hash
                    || record.entry_hash != audit_record_hash(&record)?
                {
                    return Err(LiveError(format!(
                        "live journal integrity check failed at line {}",
                        index + 1
                    )));
                }
                validate_audit_metadata(
                    &record.event_type,
                    &record.occurred_at,
                    &record.actor,
                    &record.correlation_id,
                )?;
                previous_hash = record.entry_hash.clone();
                next_sequence += 1;
                latest = Some(record.state);
            }
        }
        Ok(Self {
            path,
            file,
            next_sequence,
            previous_hash,
            latest,
        })
    }

    /// Returns the journal path for backup and disaster-recovery procedures.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn latest(&self) -> Option<&PersistentLiveState> {
        self.latest.as_ref()
    }

    fn sequence(&self) -> u64 {
        self.next_sequence.saturating_sub(1)
    }

    fn head_hash(&self) -> &str {
        &self.previous_hash
    }

    fn append(
        &mut self,
        event_type: &str,
        occurred_at: &str,
        actor: &str,
        correlation_id: &str,
        state: PersistentLiveState,
    ) -> Result<(), LiveError> {
        validate_audit_metadata(event_type, occurred_at, actor, correlation_id)?;
        let mut record = LiveJournalRecord {
            schema_version: LIVE_JOURNAL_SCHEMA_VERSION,
            sequence: self.next_sequence,
            previous_hash: self.previous_hash.clone(),
            event_type: event_type.to_owned(),
            occurred_at: occurred_at.to_owned(),
            actor: actor.to_owned(),
            correlation_id: correlation_id.to_owned(),
            state,
            entry_hash: String::new(),
        };
        record.entry_hash = audit_record_hash(&record)?;
        let serialized =
            serde_json::to_string(&record).map_err(|error| LiveError(error.to_string()))?;
        self.file
            .write_all(serialized.as_bytes())
            .and_then(|_| self.file.write_all(b"\n"))
            .and_then(|_| self.file.sync_data())
            .map_err(|error| LiveError(error.to_string()))?;
        self.next_sequence += 1;
        self.previous_hash = record.entry_hash;
        self.latest = Some(record.state);
        Ok(())
    }
}

fn audit_record_hash(record: &LiveJournalRecord) -> Result<String, LiveError> {
    let mut unsigned = record.clone();
    unsigned.entry_hash.clear();
    let canonical =
        serde_json::to_string(&unsigned).map_err(|error| LiveError(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
}

fn validate_audit_metadata(
    event_type: &str,
    occurred_at: &str,
    actor: &str,
    correlation_id: &str,
) -> Result<(), LiveError> {
    if !event_type.starts_with("live.") || event_type.len() > 128 {
        return Err(LiveError("live audit event_type is invalid".to_owned()));
    }
    validate_utc_timestamp("live audit occurred_at", occurred_at)?;
    validate_canonical_id("live audit actor", actor)?;
    validate_canonical_id("live audit correlation_id", correlation_id)?;
    Ok(())
}

/// Controlled-live service with shadow/canary separation and append-only audit evidence.
pub struct LiveTradingService<B> {
    account: LiveAccount,
    policy: LiveRiskPolicy,
    activation: LiveActivation,
    kill_switches: LiveKillSwitchRegistry,
    broker: B,
    broker_connected: bool,
    cash: Decimal,
    orders: BTreeMap<String, LiveOrder>,
    /// Atomic multi-leg combinations, tracked separately from `orders` because
    /// a combination is one order over several instruments and does not fit the
    /// single-instrument shape of [`LiveOrder`]. Every risk counter that reads
    /// `orders` reads this map too -- a combination invisible to the
    /// single-order gate would be a hole in exactly the limits it is subject
    /// to, with real capital behind it.
    combo_orders: BTreeMap<String, LiveComboOrder>,
    approvals: BTreeMap<String, RegisteredLiveApproval>,
    portfolios: BTreeMap<String, Portfolio>,
    /// Independent FIFO long-lot cost-basis ledger, kept in lockstep with
    /// `portfolios` from the same fills. See `core/paper`'s identical field
    /// for the full rationale: `Portfolio` tracks a single running average
    /// cost for OMS/risk decisions; this book retains individual acquisition
    /// lots so a real disposal reports an auditable, tax-lot-accurate
    /// realized gain/loss.
    tax_lots: TaxLotBook,
    /// Last observed mark per instrument, from every past risk evaluation this
    /// service has performed. Feeds the Slice-1 aggregate risk composition's
    /// multi-instrument exposure calculation; a position with no cached mark
    /// yet falls back to its own average cost. Not a live market-data feed --
    /// see [`PortfolioRiskComposition`].
    marks: BTreeMap<String, Decimal>,
    /// Highest real equity (cash plus every marked position) this service has
    /// ever observed, updated unconditionally from every risk evaluation --
    /// independent of whether Slice-1 composition is even configured, so
    /// enabling it later does not start drawdown tracking from a fresh,
    /// artificially favorable baseline. Feeds the Slice-2 `PortfolioRiskSnapshot`'s
    /// real `peak_equity` (see [`PortfolioRiskComposition`]).
    peak_equity: Decimal,
    /// UTC calendar date (`YYYY-MM-DD`, sliced from a risk decision's own
    /// canonical `decided_at`) of the current session-start daily-loss
    /// baseline. `None` until the first risk evaluation this service has ever
    /// performed. Reset -- not maxed, unlike `peak_equity` -- at the first
    /// evaluation whose decision time falls on a new UTC calendar day.
    daily_baseline_date: Option<String>,
    /// Real equity observed at the first risk evaluation of
    /// `daily_baseline_date`. Meaningless while `daily_baseline_date` is
    /// `None`. Feeds the Slice-2b `PortfolioRiskSnapshot`'s real `daily_pnl`
    /// (`equity - daily_baseline_equity`) -- see [`PortfolioRiskComposition`].
    daily_baseline_equity: Decimal,
    /// Net signed quantity each strategy has contributed to each instrument
    /// (`instrument_id -> strategy_id -> quantity`), updated from every real
    /// fill. Feeds the Slice-2d aggregate-risk snapshot's real per-strategy
    /// `RiskPosition` rows -- see [`PortfolioRiskComposition`] and
    /// [`Self::apply_strategy_attribution_fill`].
    strategy_attribution: BTreeMap<String, BTreeMap<String, Decimal>>,
    execution_ids: BTreeSet<String>,
    /// Operator-attested corporate actions, in the order applied (E8.4b).
    corporate_actions: Vec<LiveCorporateActionReceipt>,
    incidents: BTreeMap<String, LiveIncident>,
    live_days: BTreeMap<String, PersistentLiveDay>,
    canary_submissions: u32,
    next_reconciliation: u64,
    last_reconciled_at: Option<String>,
    last_reconciliation_clean: Option<bool>,
    latest_reconciliation: Option<LiveReconciliationReport>,
    journal: LiveAuditJournal,
    audit_healthy: bool,
}

impl<B: LiveBrokerAdapter> LiveTradingService<B> {
    /// Opens a fail-closed controlled-live service from an integrity-checked audit journal.
    pub fn open_durable(
        account: LiveAccount,
        policy: LiveRiskPolicy,
        activation: LiveActivation,
        kill_switches: LiveKillSwitchRegistry,
        broker: B,
        journal_path: impl AsRef<Path>,
        opened_at: &str,
    ) -> Result<Self, LiveError> {
        account.validate()?;
        policy.validate()?;
        validate_utc_timestamp("live service opened_at", opened_at)?;
        let configuration_fingerprint =
            configuration_fingerprint(&account, &policy, &kill_switches);
        activation.validate(&configuration_fingerprint)?;
        let journal = LiveAuditJournal::open(journal_path)?;
        let latest = journal.latest().cloned();
        let mut service = Self {
            cash: account.initial_cash,
            peak_equity: account.initial_cash,
            daily_baseline_date: None,
            daily_baseline_equity: Decimal::ZERO,
            strategy_attribution: BTreeMap::new(),
            account,
            policy,
            activation,
            kill_switches,
            broker,
            broker_connected: false,
            orders: BTreeMap::new(),
            combo_orders: BTreeMap::new(),
            approvals: BTreeMap::new(),
            portfolios: BTreeMap::new(),
            tax_lots: TaxLotBook::default(),
            marks: BTreeMap::new(),
            execution_ids: BTreeSet::new(),
            corporate_actions: Vec::new(),
            incidents: BTreeMap::new(),
            live_days: BTreeMap::new(),
            canary_submissions: 0,
            next_reconciliation: 1,
            last_reconciled_at: None,
            last_reconciliation_clean: None,
            latest_reconciliation: None,
            journal,
            audit_healthy: true,
        };
        if let Some(state) = latest {
            service.restore(state)?;
            // A broker session never survives process recovery. Persist this fact before any reconnect.
            service.persist(
                "live.service.restarted.v1",
                &service.activation.approved_by.clone(),
                opened_at,
                &service.activation.activation_id.clone(),
            )?;
        } else {
            service.persist(
                "live.service.initialized.v1",
                &service.activation.approved_by.clone(),
                opened_at,
                &service.activation.activation_id.clone(),
            )?;
        }
        Ok(service)
    }

    /// Returns the immutable LIVE configuration fingerprint bound to activation and approvals.
    pub fn configuration_fingerprint(&self) -> String {
        configuration_fingerprint(&self.account, &self.policy, &self.kill_switches)
    }

    /// Returns the mode chosen by the time-bounded activation.
    pub fn mode(&self) -> LiveRunMode {
        self.activation.mode
    }

    /// Returns mutable adapter access only for tightly scoped operational tests.
    pub fn broker_mut(&mut self) -> &mut B {
        &mut self.broker
    }

    /// Returns the independently controlled kill-switch registry.
    pub fn kill_switches(&self) -> &LiveKillSwitchRegistry {
        &self.kill_switches
    }

    /// Registers one unconsumed four-eyes approval before a canary submission.
    pub fn register_approval(
        &mut self,
        approval: LiveApproval,
        recorded_at: &str,
        actor: &str,
    ) -> Result<(), LiveError> {
        self.ensure_audit_healthy()?;
        validate_utc_timestamp("live approval recorded_at", recorded_at)?;
        validate_canonical_id("live approval recording actor", actor)?;
        approval.validate(&self.configuration_fingerprint())?;
        if actor != approval.approved_by {
            return Err(LiveError(
                "live approval must be recorded by its distinct approving operator".to_owned(),
            ));
        }
        if approval.approved_at.as_str() > recorded_at
            || approval.expires_at.as_str() <= recorded_at
        {
            return Err(LiveError(
                "live approval is not active when registered".to_owned(),
            ));
        }
        match self.approvals.get(&approval.approval_id) {
            Some(existing) if existing.approval_equivalent(&approval) => return Ok(()),
            Some(_) => {
                return Err(LiveError(
                    "live approval ID was reused with different data".to_owned(),
                ))
            }
            None => {}
        }
        let approval_id = approval.approval_id.clone();
        self.approvals.insert(
            approval_id.clone(),
            RegisteredLiveApproval {
                approval,
                consumed: false,
            },
        );
        self.persist(
            "live.approval.registered.v1",
            actor,
            recorded_at,
            &approval_id,
        )
    }

    /// Activates a live kill switch without requiring broker connectivity.
    pub fn activate_kill_switch(
        &mut self,
        scope: LiveKillSwitchScope,
        actor: &str,
        occurred_at: &str,
    ) -> Result<bool, LiveError> {
        self.ensure_audit_healthy()?;
        validate_canonical_id("live kill actor", actor)?;
        validate_utc_timestamp("live kill time", occurred_at)?;
        let changed = self.kill_switches.activate(scope)?;
        self.persist(
            "live.kill_switch.activated.v1",
            actor,
            occurred_at,
            "kill-switch",
        )?;
        Ok(changed)
    }

    /// Deactivates a live kill switch explicitly and durably.
    pub fn deactivate_kill_switch(
        &mut self,
        scope: &LiveKillSwitchScope,
        actor: &str,
        occurred_at: &str,
    ) -> Result<bool, LiveError> {
        self.ensure_audit_healthy()?;
        validate_canonical_id("live kill actor", actor)?;
        validate_utc_timestamp("live kill time", occurred_at)?;
        let changed = self.kill_switches.deactivate(scope);
        if changed {
            self.persist(
                "live.kill_switch.deactivated.v1",
                actor,
                occurred_at,
                "kill-switch",
            )?;
        }
        Ok(changed)
    }

    /// Connects only a canary run through a managed secret provider.
    ///
    /// Secret bytes are never placed in an audit record, configuration, error, or dashboard.
    pub fn connect<S: SecretProvider>(
        &mut self,
        provider: &S,
        actor: &str,
        occurred_at: &str,
    ) -> Result<(), LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(occurred_at)?;
        validate_canonical_id("live connection actor", actor)?;
        self.persist(
            "live.credential.requested.v1",
            actor,
            occurred_at,
            "credential",
        )?;
        let credential = match provider.resolve(&self.account.credential_reference) {
            Ok(credential) => credential,
            Err(error) => {
                self.persist(
                    "live.credential.unavailable.v1",
                    actor,
                    occurred_at,
                    "credential",
                )?;
                return Err(error.into());
            }
        };
        if let Err(error) = self.broker.connect(&self.account.account_id, &credential) {
            self.broker_connected = false;
            self.persist(
                "live.broker.connection_failed.v1",
                actor,
                occurred_at,
                "connection",
            )?;
            return Err(error);
        }
        self.broker_connected = true;
        self.persist("live.broker.connected.v1", actor, occurred_at, "connection")
    }

    /// Reconnects a canary session, then immediately synchronizes and reconciles.
    pub fn reconnect_and_reconcile<S: SecretProvider>(
        &mut self,
        provider: &S,
        actor: &str,
        reconciled_at: &str,
    ) -> Result<LiveReconciliationReport, LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(reconciled_at)?;
        validate_canonical_id("live reconnect actor", actor)?;
        self.persist(
            "live.reconnect.requested.v1",
            actor,
            reconciled_at,
            "reconnect",
        )?;
        let credential = match provider.resolve(&self.account.credential_reference) {
            Ok(credential) => credential,
            Err(error) => {
                self.broker_connected = false;
                self.persist(
                    "live.credential.unavailable.v1",
                    actor,
                    reconciled_at,
                    "credential",
                )?;
                return Err(error.into());
            }
        };
        if let Err(error) = self.broker.reconnect(&self.account.account_id, &credential) {
            self.broker_connected = false;
            self.persist(
                "live.reconnect.failed.v1",
                actor,
                reconciled_at,
                "reconnect",
            )?;
            return Err(error);
        }
        self.broker_connected = true;
        self.persist(
            "live.reconnect.succeeded.v1",
            actor,
            reconciled_at,
            "reconnect",
        )?;
        self.synchronize(actor, reconciled_at)?;
        self.reconcile(actor, reconciled_at)
    }

    /// Records a production-data shadow decision. Shadow mode has no adapter submit path.
    pub fn record_shadow_intent(
        &mut self,
        intent: OrderIntent,
        market: LiveMarketData,
        decided_at: &str,
        actor: &str,
    ) -> Result<LiveSubmitOutcome, LiveError> {
        self.ensure_audit_healthy()?;
        if self.activation.mode != LiveRunMode::Shadow {
            return Err(LiveError(
                "shadow recording is disabled for a canary run".to_owned(),
            ));
        }
        if intent.environment != "SHADOW" {
            return Err(LiveError(
                "shadow service accepts only SHADOW intents".to_owned(),
            ));
        }
        if intent.account_id != self.account.account_id {
            return Err(LiveError(
                "shadow intent account does not match controlled-live account".to_owned(),
            ));
        }
        if !self.activation.active_at(decided_at)? {
            return Err(LiveError(
                "controlled-live shadow activation is absent or expired".to_owned(),
            ));
        }
        validate_canonical_id("shadow actor", actor)?;
        let decision = self.evaluate_risk(&intent, &market, decided_at, true)?;
        self.persist(
            "live.shadow.intent_recorded.v1",
            actor,
            decided_at,
            &intent.correlation_id,
        )?;
        Ok(LiveSubmitOutcome::ShadowRecorded { decision })
    }

    /// Performs a bounded live canary submission after exact per-order approval.
    pub fn submit_canary_intent(
        &mut self,
        intent: OrderIntent,
        market: LiveMarketData,
        approval_id: &str,
        decided_at: &str,
        actor: &str,
    ) -> Result<LiveSubmitOutcome, LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(decided_at)?;
        validate_canonical_id("live submit actor", actor)?;
        validate_canonical_id("live approval_id", approval_id)?;
        if !self.broker_connected {
            return Err(LiveError(
                "live canary broker session is not connected".to_owned(),
            ));
        }
        if intent.environment != "LIVE" || intent.account_id != self.account.account_id {
            return Err(LiveError(
                "canary accepts only matching LIVE account intents".to_owned(),
            ));
        }
        intent.validate()?;
        let expected_fingerprint = intent_fingerprint(&intent)?;
        let order_id = format!("order-{}", intent.intent_id);
        if let Some(existing) = self.orders.get(&order_id) {
            if existing.oms.intent != intent
                || existing.approval_id != approval_id
                || existing.market != market
                || existing.decision.decided_at != decided_at
            {
                return Err(LiveError(
                    "live idempotency key was reused with different data".to_owned(),
                ));
            }
            return Ok(LiveSubmitOutcome::CanaryOrder {
                decision: existing.decision.clone(),
                order_id,
                state: existing.oms.state,
            });
        }
        // Before the approval is looked at, let alone consumed: a refusal here
        // leaves the approval, the canary budget and the session untouched.
        self.ensure_route_carries(intent.time_in_force, false)?;
        let registered = self.approvals.get(approval_id).ok_or_else(|| {
            LiveError("live canary submission lacks a registered approval".to_owned())
        })?;
        if registered.consumed
            || registered.approval.intent_id != intent.intent_id
            || registered.approval.intent_fingerprint != expected_fingerprint
            || registered.approval.configuration_fingerprint != self.configuration_fingerprint()
            || registered.approval.approved_at.as_str() > decided_at
            || registered.approval.expires_at.as_str() <= decided_at
        {
            return Err(LiveError(
                "live approval is expired, consumed, or does not bind this exact intent".to_owned(),
            ));
        }
        let decision = self.evaluate_risk(&intent, &market, decided_at, false)?;
        if !decision.approved {
            self.persist(
                "live.risk.rejected.v1",
                actor,
                decided_at,
                &intent.correlation_id,
            )?;
            return Ok(LiveSubmitOutcome::RiskRejected { decision });
        }
        let core_decision = RiskDecision {
            decision_id: decision.decision_id.clone(),
            intent_id: intent.intent_id.clone(),
            approved: true,
            reason_codes: decision.reason_codes.clone(),
            policy_version: self.policy.version.clone(),
            decided_at: decided_at.to_owned(),
            correlation_id: intent.correlation_id.clone(),
            actor: "live_risk_engine".to_owned(),
            evaluated_limits: decision.evaluated_limits.clone(),
        };
        let mut oms = OmsOrder::from_approved_intent(intent, &core_decision)?;
        oms.transition(OrderState::Approved, "LIVE_RISK_APPROVED")?;
        oms.transition(
            OrderState::PendingSubmit,
            "LIVE_CANARY_SUBMISSION_REQUESTED",
        )?;
        let request = LiveBrokerOrderRequest::from_order(&oms);
        request.validate()?;
        let correlation_id = oms.intent.correlation_id.clone();
        self.orders.insert(
            oms.order_id.clone(),
            LiveOrder {
                oms,
                approval_id: approval_id.to_owned(),
                market,
                decision: decision.clone(),
                broker_order_id: None,
                broker_order_versions: Vec::new(),
                replace_return_state: None,
                filled_quantity: Decimal::ZERO,
            },
        );
        self.approvals
            .get_mut(approval_id)
            .ok_or_else(|| {
                LiveError("registered live approval disappeared before consumption".to_owned())
            })?
            .consumed = true;
        self.canary_submissions = self
            .canary_submissions
            .checked_add(1)
            .ok_or_else(|| LiveError("live canary submission counter overflowed".to_owned()))?;
        // The durable audit record precedes the irreversible external broker call.
        self.persist(
            "live.order.pending_submission.v1",
            actor,
            decided_at,
            &correlation_id,
        )?;
        match self.broker.submit(&request) {
            Ok(LiveBrokerSubmitResult::Acknowledged { broker_order_id }) => {
                validate_canonical_id("live broker_order_id", &broker_order_id)?;
                let order = self.order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "LIVE_CANARY_SUBMISSION_SENT")?;
                order
                    .oms
                    .transition(OrderState::Acknowledged, "LIVE_BROKER_ACKNOWLEDGED")?;
                order.broker_order_id = Some(broker_order_id.clone());
                order.broker_order_versions.push(broker_order_id);
                let state = order.oms.state;
                self.persist(
                    "live.order.acknowledged.v1",
                    actor,
                    decided_at,
                    &correlation_id,
                )?;
                Ok(LiveSubmitOutcome::CanaryOrder {
                    decision,
                    order_id: request.client_order_id,
                    state,
                })
            }
            Ok(LiveBrokerSubmitResult::Rejected { reason }) => {
                validate_reason("live broker rejection reason", &reason)?;
                let order = self.order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "LIVE_CANARY_SUBMISSION_SENT")?;
                order.oms.transition(OrderState::Rejected, reason)?;
                let state = order.oms.state;
                self.persist("live.order.rejected.v1", actor, decided_at, &correlation_id)?;
                Ok(LiveSubmitOutcome::CanaryOrder {
                    decision,
                    order_id: request.client_order_id,
                    state,
                })
            }
            Ok(LiveBrokerSubmitResult::Unknown { reason }) => {
                validate_reason("live broker unknown reason", &reason)?;
                self.order_mut(&request.client_order_id)?
                    .oms
                    .transition(OrderState::Unknown, reason)?;
                self.persist("live.order.unknown.v1", actor, decided_at, &correlation_id)?;
                Ok(LiveSubmitOutcome::CanaryOrder {
                    decision,
                    order_id: request.client_order_id,
                    state: OrderState::Unknown,
                })
            }
            Err(error) => {
                self.order_mut(&request.client_order_id)?
                    .oms
                    .transition(OrderState::Unknown, "LIVE_TRANSPORT_OUTCOME_UNKNOWN")?;
                self.broker_connected = false;
                self.persist(
                    "live.order.transport_unknown.v1",
                    actor,
                    decided_at,
                    &correlation_id,
                )?;
                Err(error)
            }
        }
    }

    /// Refuses what the configured adapter cannot carry, before anything is
    /// recorded, consumed or transmitted (delivery state E5.7).
    fn ensure_route_carries(
        &self,
        time_in_force: TimeInForce,
        combination: bool,
    ) -> Result<(), LiveError> {
        let capabilities = self.broker.capabilities();
        if combination && !capabilities.combinations {
            return Err(LiveError(
                "the configured live broker adapter cannot execute combinations; nothing was recorded, consumed or transmitted"
                    .to_owned(),
            ));
        }
        if time_in_force == TimeInForce::GoodTilCancelled && !capabilities.good_til_cancelled {
            return Err(LiveError(
                "the configured live broker adapter carries only DAY orders; the GTC intent was refused before anything was recorded, consumed or transmitted"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    /// Requests cancellation; a transport failure remains explicitly `UNKNOWN`.
    pub fn cancel_order(
        &mut self,
        order_id: &str,
        actor: &str,
        occurred_at: &str,
    ) -> Result<(), LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(occurred_at)?;
        validate_canonical_id("live cancel actor", actor)?;
        if self.combo_orders.contains_key(order_id) {
            return self.cancel_combo_order(order_id, actor, occurred_at);
        }
        let state = self.order_mut(order_id)?.oms.state;
        if !matches!(
            state,
            OrderState::Acknowledged | OrderState::PartiallyFilled
        ) {
            return Err(LiveError(
                "only acknowledged or partially filled live orders may cancel".to_owned(),
            ));
        }
        self.order_mut(order_id)?
            .oms
            .transition(OrderState::PendingCancel, "LIVE_CANCEL_REQUESTED")?;
        self.persist("live.order.pending_cancel.v1", actor, occurred_at, order_id)?;
        if let Err(error) = self.broker.cancel(order_id) {
            self.order_mut(order_id)?
                .oms
                .transition(OrderState::Unknown, "LIVE_CANCEL_OUTCOME_UNKNOWN")?;
            self.broker_connected = false;
            self.persist("live.order.cancel_unknown.v1", actor, occurred_at, order_id)?;
            return Err(error);
        }
        self.persist("live.order.cancel_sent.v1", actor, occurred_at, order_id)
    }

    /// Requests a price-only replacement that cannot increase the approved limit risk.
    ///
    /// Replacement preserves the exact approved intent's quantity, side, and
    /// identity. It is therefore limited to a more conservative limit price and
    /// remains subject to the active canary window and durable audit trail.
    pub fn replace_order(
        &mut self,
        order_id: &str,
        replacement_limit_price: Decimal,
        actor: &str,
        occurred_at: &str,
    ) -> Result<(), LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(occurred_at)?;
        validate_canonical_id("live replacement actor", actor)?;
        if !self.broker_connected {
            return Err(LiveError(
                "live canary broker session is not connected; reconcile before replacement"
                    .to_owned(),
            ));
        }
        // Before the order moves to `PENDING_REPLACE`: an adapter that cannot
        // replace would otherwise leave it `UNKNOWN` and the session
        // disconnected (delivery state E5.7).
        if !self.broker.capabilities().replacement {
            return Err(LiveError(
                "the configured live broker adapter cannot replace orders; the order was left unchanged"
                    .to_owned(),
            ));
        }
        let request = {
            let order = self.order_mut(order_id)?;
            if !matches!(
                order.oms.state,
                OrderState::Acknowledged | OrderState::PartiallyFilled
            ) {
                return Err(LiveError(
                    "only acknowledged or partially filled live orders may be replaced".to_owned(),
                ));
            }
            let prior_limit = order.oms.intent.limit_price.ok_or_else(|| {
                LiveError("only live limit orders support price replacement".to_owned())
            })?;
            if replacement_limit_price <= Decimal::ZERO
                || (order.oms.intent.side == Side::Buy && replacement_limit_price > prior_limit)
                || (order.oms.intent.side == Side::Sell && replacement_limit_price < prior_limit)
            {
                return Err(LiveError(
                    "live replacement may only reduce the originally approved limit risk"
                        .to_owned(),
                ));
            }
            let previous_broker_order_id = order.broker_order_id.clone().ok_or_else(|| {
                LiveError("live replacement requires an acknowledged broker order ID".to_owned())
            })?;
            order.replace_return_state = Some(order.oms.state);
            order
                .oms
                .transition(OrderState::PendingReplace, "LIVE_REPLACE_REQUESTED")?;
            LiveBrokerReplaceRequest {
                client_order_id: order.oms.order_id.clone(),
                previous_broker_order_id,
                limit_price: replacement_limit_price,
            }
        };
        self.persist(
            "live.order.pending_replace.v1",
            actor,
            occurred_at,
            order_id,
        )?;
        if let Err(error) = self.broker.replace(&request) {
            self.order_mut(order_id)?
                .oms
                .transition(OrderState::Unknown, "LIVE_REPLACE_OUTCOME_UNKNOWN")?;
            self.broker_connected = false;
            self.persist(
                "live.order.replace_unknown.v1",
                actor,
                occurred_at,
                order_id,
            )?;
            return Err(error);
        }
        self.persist("live.order.replace_sent.v1", actor, occurred_at, order_id)
    }

    /// Drains broker evidence and applies each unique execution exactly once.
    pub fn synchronize(&mut self, actor: &str, occurred_at: &str) -> Result<usize, LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(occurred_at)?;
        validate_canonical_id("live synchronization actor", actor)?;
        if !self.broker_connected {
            return Err(LiveError(
                "live canary broker session is not connected".to_owned(),
            ));
        }
        let events = match self.broker.poll() {
            Ok(events) => events,
            Err(error) => {
                self.broker_connected = false;
                self.persist(
                    "live.broker.poll_failed.v1",
                    actor,
                    occurred_at,
                    "broker-events",
                )?;
                return Err(error);
            }
        };
        let count = events.len();
        let mut apply_errors = Vec::new();
        for event in events {
            // `poll` drains broker evidence, so an event that fails to apply can
            // never be re-fetched: aborting the whole loop on the first failure
            // would silently and permanently lose every event still queued
            // behind it. Snapshot the mutable state first so a failed
            // application leaves no partial mutation in memory, then keep
            // applying the remaining events in the batch.
            let orders_snapshot = self.orders.clone();
            let combo_orders_snapshot = self.combo_orders.clone();
            let incidents_snapshot = self.incidents.clone();
            let event_order_id = event.client_order_id().to_owned();
            let combination_evidence = self.combo_orders.contains_key(&event_order_id)
                || matches!(&event, LiveBrokerEvent::ComboExecution(_));
            let portfolios_snapshot = self.portfolios.clone();
            let tax_lots_snapshot = self.tax_lots.clone();
            let strategy_attribution_snapshot = self.strategy_attribution.clone();
            let execution_ids_snapshot = self.execution_ids.clone();
            let cash_snapshot = self.cash;
            if let Err(error) = self.apply_broker_event(event) {
                self.orders = orders_snapshot;
                self.combo_orders = combo_orders_snapshot;
                self.incidents = incidents_snapshot;
                self.portfolios = portfolios_snapshot;
                self.tax_lots = tax_lots_snapshot;
                self.strategy_attribution = strategy_attribution_snapshot;
                self.execution_ids = execution_ids_snapshot;
                self.cash = cash_snapshot;
                if combination_evidence {
                    if let Some(order) = self.combo_orders.get_mut(&event_order_id) {
                        if order.oms.state != OrderState::Unknown
                            && order.oms.state != OrderState::Filled
                        {
                            order.oms.transition(
                                OrderState::Unknown,
                                "LIVE_COMBINATION_EXECUTION_ANOMALY",
                            )?;
                        }
                    }
                    self.record_internal_incident(
                        "COMBINATION_EXECUTION_ANOMALY",
                        event_order_id.clone(),
                        error.0.clone(),
                    );
                    self.persist(
                        "live.combo.execution_anomaly.v1",
                        actor,
                        occurred_at,
                        &event_order_id,
                    )?;
                }
                apply_errors.push(error.0);
                continue;
            }
            self.persist(
                "live.broker.event_applied.v1",
                actor,
                occurred_at,
                "broker-event",
            )?;
        }
        if !apply_errors.is_empty() {
            return Err(LiveError(format!(
                "live broker synchronize failed to apply {} of {count} broker events (each rolled back cleanly): {}",
                apply_errors.len(),
                apply_errors.join("; "),
            )));
        }
        Ok(count)
    }

    /// Compares independently tracked and broker state without overwriting either side.
    pub fn reconcile(
        &mut self,
        actor: &str,
        reconciled_at: &str,
    ) -> Result<LiveReconciliationReport, LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(reconciled_at)?;
        validate_canonical_id("live reconciliation actor", actor)?;
        if !self.broker_connected {
            return Err(LiveError(
                "live canary broker session is not connected".to_owned(),
            ));
        }
        let snapshot = match self.broker.snapshot(&self.account.account_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.broker_connected = false;
                self.persist(
                    "live.broker.snapshot_failed.v1",
                    actor,
                    reconciled_at,
                    "reconciliation",
                )?;
                return Err(error);
            }
        };
        validate_broker_snapshot(&snapshot)?;
        let reconciliation_id = format!("reconciliation-{:08}", self.next_reconciliation);
        self.next_reconciliation += 1;
        let mut raw_issues = Vec::new();
        let mut broker_orders: BTreeMap<&str, Vec<&LiveBrokerOrderSnapshot>> = BTreeMap::new();
        for broker_order in &snapshot.orders {
            broker_orders
                .entry(broker_order.client_order_id.as_str())
                .or_default()
                .push(broker_order);
        }
        let order_views = self
            .orders
            .iter()
            .map(|(order_id, order)| {
                (
                    order_id,
                    order.working(),
                    &order.broker_order_id,
                    &order.broker_order_versions,
                    order.filled_quantity,
                    order.oms.state,
                )
            })
            .chain(self.combo_orders.iter().map(|(order_id, order)| {
                (
                    order_id,
                    order.working(),
                    &order.broker_order_id,
                    &order.broker_order_versions,
                    order.filled_quantity,
                    order.oms.state,
                )
            }));
        for (order_id, working, broker_order_id, versions, filled_quantity, state) in order_views {
            match broker_orders.get(order_id.as_str()) {
                None if working => raw_issues.push((
                    "MISSING_BROKER_ORDER",
                    order_id.clone(),
                    "internal working order is absent from broker snapshot".to_owned(),
                )),
                Some(brokers) => {
                    let broker = broker_order_id.as_ref().and_then(|current| {
                        brokers
                            .iter()
                            .copied()
                            .find(|candidate| candidate.broker_order_id == *current)
                    });
                    if broker.is_none() {
                        raw_issues.push((
                            "BROKER_ORDER_ID_MISMATCH",
                            order_id.clone(),
                            format!(
                                "internal_current={:?},broker_versions={}",
                                broker_order_id,
                                brokers
                                    .iter()
                                    .map(|candidate| candidate.broker_order_id.as_str())
                                    .collect::<Vec<_>>()
                                    .join(",")
                            ),
                        ));
                    }
                    if brokers
                        .iter()
                        .any(|candidate| !versions.contains(&candidate.broker_order_id))
                    {
                        raw_issues.push((
                            "BROKER_ORDER_VERSION_MISMATCH",
                            order_id.clone(),
                            "broker snapshot includes an unrecognized broker order version"
                                .to_owned(),
                        ));
                    }
                    let broker_filled =
                        brokers.iter().try_fold(Decimal::ZERO, |total, candidate| {
                            total
                                .checked_add(candidate.filled_quantity)
                                .map_err(LiveError::from)
                        })?;
                    if filled_quantity != broker_filled {
                        raw_issues.push((
                            "FILLED_QUANTITY_MISMATCH",
                            order_id.clone(),
                            format!("internal={},broker={broker_filled}", filled_quantity,),
                        ));
                    }
                    if let Some(broker) = broker {
                        if state != broker.state {
                            raw_issues.push((
                                "ORDER_STATE_MISMATCH",
                                order_id.clone(),
                                format!(
                                    "internal={},broker={}",
                                    state.as_str(),
                                    broker.state.as_str()
                                ),
                            ));
                        }
                    }
                }
                None => {}
            }
        }
        for broker in &snapshot.orders {
            if !self.orders.contains_key(&broker.client_order_id)
                && !self.combo_orders.contains_key(&broker.client_order_id)
            {
                raw_issues.push((
                    "UNEXPECTED_BROKER_ORDER",
                    broker.client_order_id.clone(),
                    "broker snapshot contains no matching internal order".to_owned(),
                ));
            }
        }
        raw_issues.extend(
            self.incidents
                .values()
                .filter(|incident| {
                    incident.unexplained()
                        && incident.issue.category == "COMBINATION_EXECUTION_ANOMALY"
                })
                .map(|incident| {
                    (
                        "COMBINATION_EXECUTION_ANOMALY",
                        incident.issue.subject.clone(),
                        incident.issue.detail.clone(),
                    )
                }),
        );
        let broker_positions: BTreeMap<_, _> = snapshot
            .positions
            .iter()
            .map(|position| (position.instrument_id.as_str(), position.quantity))
            .collect();
        let instruments: BTreeSet<_> = self
            .portfolios
            .keys()
            .map(String::as_str)
            .chain(broker_positions.keys().copied())
            .collect();
        for instrument_id in instruments {
            let internal = self
                .portfolios
                .get(instrument_id)
                .map(|portfolio| portfolio.position_snapshot().quantity)
                .unwrap_or(Decimal::ZERO);
            let broker = broker_positions
                .get(instrument_id)
                .copied()
                .unwrap_or(Decimal::ZERO);
            if internal != broker {
                raw_issues.push((
                    "POSITION_QUANTITY_MISMATCH",
                    instrument_id.to_owned(),
                    format!("internal={internal},broker={broker}"),
                ));
            }
        }
        if self.cash != snapshot.cash {
            raw_issues.push((
                "CASH_MISMATCH",
                self.account.account_id.clone(),
                format!("internal={},broker={}", self.cash, snapshot.cash),
            ));
        }
        let issues = raw_issues
            .into_iter()
            .enumerate()
            .map(|(index, (category, subject, detail))| {
                let existing = self
                    .incidents
                    .values()
                    .find(|incident| {
                        incident.issue.category == category
                            && incident.issue.subject == subject
                            && incident.unexplained()
                    })
                    .map(|incident| incident.issue.incident_id.clone());
                let incident_id = existing.unwrap_or_else(|| {
                    format!(
                        "incident-{}-{:03}",
                        reconciliation_id.trim_start_matches("reconciliation-"),
                        index + 1
                    )
                });
                let issue = LiveReconciliationIssue {
                    incident_id: incident_id.clone(),
                    category: category.to_owned(),
                    subject,
                    detail,
                };
                self.incidents
                    .entry(incident_id)
                    .or_insert_with(|| LiveIncident {
                        issue: issue.clone(),
                        explanation: None,
                    });
                issue
            })
            .collect();
        let report = LiveReconciliationReport {
            reconciliation_id,
            reconciled_at: reconciled_at.to_owned(),
            issues,
        };
        self.last_reconciled_at = Some(reconciled_at.to_owned());
        self.last_reconciliation_clean = Some(report.is_clean());
        self.latest_reconciliation = Some(report.clone());
        self.persist(
            "live.reconciliation.completed.v1",
            actor,
            reconciled_at,
            &report.reconciliation_id,
        )?;
        Ok(report)
    }

    /// Attaches an attributable explanation to an incident without changing broker or ledger state.
    pub fn explain_incident(
        &mut self,
        incident_id: &str,
        explanation: impl Into<String>,
        actor: &str,
        occurred_at: &str,
    ) -> Result<(), LiveError> {
        self.ensure_audit_healthy()?;
        validate_canonical_id("live incident_id", incident_id)?;
        validate_canonical_id("live incident actor", actor)?;
        validate_utc_timestamp("live incident explanation time", occurred_at)?;
        let explanation = explanation.into();
        if explanation.trim().is_empty() || explanation.len() > 1_024 {
            return Err(LiveError(
                "live incident explanation must contain 1 to 1024 characters".to_owned(),
            ));
        }
        self.incidents
            .get_mut(incident_id)
            .ok_or_else(|| LiveError("unknown live incident".to_owned()))?
            .explanation = Some(explanation);
        self.persist(
            "live.incident.explained.v1",
            actor,
            occurred_at,
            incident_id,
        )
    }

    /// Records one closed, independently reconciled controlled-live session toward the 60-day gate.
    pub fn record_live_session(
        &mut self,
        session: &TradingSession,
        report: &LiveReconciliationReport,
        actor: &str,
        calendar: &dyn TradingCalendar,
    ) -> Result<(), LiveError> {
        self.ensure_audit_healthy()?;
        self.require_canary_active(&report.reconciled_at)?;
        validate_canonical_id("live day actor", actor)?;
        session.validate()?;
        validate_exchange_date(&session.exchange_date)?;
        if calendar.calendar_id() != self.policy.trading_calendar_id
            || calendar.session_for_exchange_date(&session.exchange_date) != Some(session)
        {
            return Err(LiveError(
                "live-day gate requires the exact session from the configured calendar".to_owned(),
            ));
        }
        if self.activation.mode != LiveRunMode::Canary
            || !report.is_clean()
            || self.latest_reconciliation.as_ref() != Some(report)
            || self.last_reconciled_at.as_deref() != Some(report.reconciled_at.as_str())
            || self.last_reconciliation_clean != Some(report.is_clean())
            || report.reconciled_at < session.closes_at
            || report.reconciliation_id
                != format!(
                    "reconciliation-{:08}",
                    self.next_reconciliation.saturating_sub(1)
                )
        {
            return Err(LiveError(
                "live-day gate requires the latest clean canary reconciliation after session close"
                    .to_owned(),
            ));
        }
        let day = PersistentLiveDay {
            calendar_id: self.policy.trading_calendar_id.clone(),
            session_opens_at: session.opens_at.clone(),
            session_closes_at: session.closes_at.clone(),
            clean: report.is_clean() && self.unresolved_incident_count() == 0 && self.audit_healthy,
            audit_head_hash: self.journal.head_hash().to_owned(),
        };
        match self.live_days.get(&session.exchange_date) {
            Some(existing) if *existing == day => return Ok(()),
            Some(_) => {
                return Err(LiveError(
                    "live-day evidence cannot be overwritten".to_owned(),
                ))
            }
            None => {}
        }
        self.live_days.insert(session.exchange_date.clone(), day);
        self.persist(
            "live.gate.session_recorded.v1",
            actor,
            &report.reconciled_at,
            &report.reconciliation_id,
        )
    }

    /// Returns the exact measured controlled-live promotion state.
    pub fn promotion_status(&self) -> LivePromotionStatus {
        let clean_live_days = self.live_days.values().filter(|day| day.clean).count() as u32;
        let unresolved_incidents = self.unresolved_incident_count();
        let complete_auditability = self.audit_healthy && self.journal.sequence() > 0;
        LivePromotionStatus {
            clean_live_days,
            required_live_days: 60,
            unresolved_incidents,
            complete_auditability,
            eligible_for_next_gate: clean_live_days >= 60
                && unresolved_incidents == 0
                && complete_auditability,
        }
    }

    /// Creates a deterministic monitoring projection. It has no trading controls.
    pub fn monitoring_dashboard(&self) -> LiveMonitoringDashboard {
        let promotion = self.promotion_status();
        let positions = self
            .portfolios
            .iter()
            .map(|(instrument_id, portfolio)| {
                let position = portfolio.position_snapshot();
                LiveMonitoringPosition {
                    instrument_id: instrument_id.clone(),
                    quantity: position.quantity.to_string(),
                    average_cost: position.average_cost.to_string(),
                    realized_pnl: position.realized_pnl.to_string(),
                }
            })
            .collect();
        LiveMonitoringDashboard {
            dashboard_schema_version: 2,
            environment: self.account.environment.clone(),
            mode: self.activation.mode.as_str().to_owned(),
            account_id: self.account.account_id.clone(),
            configuration_fingerprint: self.configuration_fingerprint(),
            broker_connected: self.broker_connected,
            audit_healthy: self.audit_healthy,
            audit_sequence: self.journal.sequence(),
            audit_head_hash: self.journal.head_hash().to_owned(),
            active_kill_switches: self.kill_switches.active_keys(),
            working_orders: self.working_order_count() as u32,
            unknown_orders: self.unknown_order_count() as u32,
            unresolved_incidents: promotion.unresolved_incidents,
            last_reconciled_at: self.last_reconciled_at.clone(),
            last_reconciliation_clean: self.last_reconciliation_clean,
            clean_live_days: promotion.clean_live_days,
            required_live_days: promotion.required_live_days,
            promotion_eligible: promotion.eligible_for_next_gate,
            complete_auditability: promotion.complete_auditability,
            internal_cash: self.cash.to_string(),
            positions,
        }
    }

    /// Serializes the strict read-only monitoring contract deterministically.
    pub fn canonical_monitoring_json(&self) -> Result<String, LiveError> {
        serde_json::to_string(&self.monitoring_dashboard())
            .map_err(|error| LiveError(error.to_string()))
    }

    /// Returns the local audit location and integrity state for disaster-recovery monitoring.
    pub fn disaster_recovery_status(&self) -> DisasterRecoveryStatus {
        DisasterRecoveryStatus {
            journal_path: self.journal.path().display().to_string(),
            audit_sequence: self.journal.sequence(),
            audit_head_hash: self.journal.head_hash().to_owned(),
            audit_healthy: self.audit_healthy,
            broker_session_requires_reconnect: !self.broker_connected,
        }
    }

    fn require_canary_active(&self, occurred_at: &str) -> Result<(), LiveError> {
        if self.activation.mode != LiveRunMode::Canary || !self.activation.active_at(occurred_at)? {
            return Err(LiveError(
                "controlled-live canary activation is absent or expired".to_owned(),
            ));
        }
        Ok(())
    }

    fn order_mut(&mut self, order_id: &str) -> Result<&mut LiveOrder, LiveError> {
        self.orders
            .get_mut(order_id)
            .ok_or_else(|| LiveError("unknown live OMS order".to_owned()))
    }

    fn evaluate_risk(
        &mut self,
        intent: &OrderIntent,
        market: &LiveMarketData,
        decided_at: &str,
        shadow: bool,
    ) -> Result<LiveRiskDecision, LiveError> {
        intent.validate()?;
        validate_utc_timestamp("live risk decided_at", decided_at)?;
        market.validate()?;
        if market.instrument_id != intent.instrument_id {
            return Err(LiveError(
                "live market observation does not match intent instrument".to_owned(),
            ));
        }
        self.marks
            .insert(intent.instrument_id.clone(), market.mark_price);
        // Peak-equity tracking runs unconditionally, independent of whether
        // Slice-2 composition is even configured, so enabling it later does
        // not start drawdown tracking from a fresh, artificially favorable
        // baseline -- the same reasoning as the unconditional `marks` update
        // above.
        let observed_equity = self.current_equity()?;
        if observed_equity > self.peak_equity {
            self.peak_equity = observed_equity;
        }
        // Session-start daily-loss baseline: reset (not maxed) whenever the
        // UTC calendar date of this decision differs from the stored
        // baseline date -- including the very first evaluation ever
        // (`daily_baseline_date` starts `None`). `decided_at` is already
        // `validate_utc_timestamp`-checked above (canonical second-precision
        // UTC, `YYYY-MM-DDTHH:MM:SSZ`), so its first 10 bytes are exactly its
        // UTC calendar date.
        let decision_date = &decided_at[..10];
        if self.daily_baseline_date.as_deref() != Some(decision_date) {
            self.daily_baseline_date = Some(decision_date.to_owned());
            self.daily_baseline_equity = observed_equity;
        }
        let observed_at = OffsetDateTime::parse(&market.observed_at, &Rfc3339)
            .map_err(|error| LiveError(error.to_string()))?;
        let decision_at = OffsetDateTime::parse(decided_at, &Rfc3339)
            .map_err(|error| LiveError(error.to_string()))?;
        let age = (decision_at - observed_at).whole_seconds();
        if age < 0
            || u64::try_from(age).unwrap_or(u64::MAX) > self.policy.max_market_data_age_seconds
        {
            return Err(LiveError(
                "live market observation is stale or later than decision".to_owned(),
            ));
        }
        let current_position = self
            .portfolios
            .get(&intent.instrument_id)
            .map(|portfolio| portfolio.position_snapshot().quantity)
            .unwrap_or(Decimal::ZERO);
        let realized_pnl = self
            .portfolios
            .values()
            .try_fold(Decimal::ZERO, |total, portfolio| {
                total.checked_add(portfolio.position_snapshot().realized_pnl)
            })?;
        let reserved_cash = self.total_reserved_cash()?;
        let available_cash = self.cash.checked_sub(reserved_cash)?;
        let estimated_notional = intent.quantity.checked_mul(market.mark_price)?;
        let requested_price_deviation_bps = intent
            .limit_price
            .map(|price| price_deviation_bps(market.mark_price, price))
            .transpose()?
            .unwrap_or(Decimal::ZERO);
        let projected_position = match intent.side {
            Side::Buy => current_position.checked_add(intent.quantity)?,
            Side::Sell => current_position.checked_sub(intent.quantity)?,
        };
        let working_position_delta = self.working_position_delta(&intent.instrument_id)?;
        let committed_position = projected_position.checked_add(working_position_delta)?;
        let rate_window_start =
            decision_at - time::Duration::seconds(self.policy.order_rate_window_seconds as i64);
        // Rate-window membership is keyed off each order's own risk *decision* time
        // (`LiveRiskDecision::decided_at`), not the caller-supplied
        // `OrderIntent::created_at`. `created_at` is stamped by the strategy/operator
        // that originated the intent and is not otherwise constrained to reflect
        // wall-clock reality, so counting against it would let a caller understate
        // its own submission rate and silently bypass `MAX_ORDER_RATE_EXCEEDED` by
        // backdating `created_at` on new intents.
        let recent_order_count = self.recent_order_count(rate_window_start, decision_at)?;
        let mut reasons = self.kill_switches.rejection_reasons(intent);
        if intent.quantity > self.policy.max_order_quantity {
            reasons.push("MAX_ORDER_QUANTITY_EXCEEDED".to_owned());
        }
        if estimated_notional > self.policy.max_order_notional {
            reasons.push("MAX_ORDER_NOTIONAL_EXCEEDED".to_owned());
        }
        if requested_price_deviation_bps > self.policy.max_price_deviation_bps {
            reasons.push("PRICE_COLLAR_EXCEEDED".to_owned());
        }
        if let Some(reason) = self
            .policy
            .tick_rejection(&intent.instrument_id, intent.limit_price)
        {
            reasons.push(reason.to_owned());
        }
        if let Some(reason) = self
            .policy
            .lot_rejection(&intent.instrument_id, intent.quantity)
        {
            reasons.push(reason.to_owned());
        }
        if self.conflicts_with_working_order(&intent.instrument_id, intent.side) {
            reasons.push("SELF_TRADE_RISK".to_owned());
        }
        if recent_order_count >= self.policy.max_order_rate {
            reasons.push("MAX_ORDER_RATE_EXCEEDED".to_owned());
        }
        if !shadow && estimated_notional > self.policy.canary_max_order_notional {
            reasons.push("CANARY_NOTIONAL_EXCEEDED".to_owned());
        }
        if !shadow && self.canary_submissions >= self.policy.canary_max_orders {
            reasons.push("CANARY_ORDER_COUNT_EXCEEDED".to_owned());
        }
        if !shadow && self.has_unknown_order() {
            reasons.push("UNKNOWN_ORDER_REQUIRES_RECONCILIATION".to_owned());
        }
        if !shadow && self.unresolved_incident_count() > 0 {
            reasons.push("UNRESOLVED_INCIDENTS_REQUIRE_REVIEW".to_owned());
        }
        if self.working_order_count() >= self.policy.max_open_orders {
            reasons.push("MAX_OPEN_ORDERS_EXCEEDED".to_owned());
        }
        if self.policy.breaches_position_limit(committed_position)? {
            reasons.push("POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned());
        }
        if intent.side == Side::Buy && estimated_notional > available_cash {
            reasons.push("INSUFFICIENT_INTERNAL_CASH".to_owned());
        }
        if intent.side == Side::Buy {
            let deployed_capital = if self.cash < self.account.initial_cash {
                self.account.initial_cash.checked_sub(self.cash)?
            } else {
                Decimal::ZERO
            };
            let projected_deployed_capital = deployed_capital
                .checked_add(reserved_cash)?
                .checked_add(estimated_notional)?;
            if projected_deployed_capital > self.account.max_deployed_capital {
                reasons.push("DEPLOYED_CAPITAL_CEILING_EXCEEDED".to_owned());
            }
        }
        let realized_loss = if realized_pnl < Decimal::ZERO {
            Decimal::ZERO.checked_sub(realized_pnl)?
        } else {
            Decimal::ZERO
        };
        if realized_loss > self.policy.max_realized_loss {
            reasons.push("MAX_REALIZED_LOSS_EXCEEDED".to_owned());
        }
        let mut portfolio_risk_limits = String::new();
        if let Some(composition) = self.policy.portfolio_risk.as_ref() {
            match self.portfolio_risk_decision(composition, intent, market, decided_at)? {
                Some((decision, margin_used)) => {
                    reasons.extend(
                        decision
                            .reason_codes
                            .into_iter()
                            // `SELF_TRADE_RISK` is already independently detected above from
                            // the same working-order state; every other reason this composed
                            // kernel can produce is new coverage (see `PortfolioRiskComposition`).
                            .filter(|reason| reason != "APPROVED" && reason != "SELF_TRADE_RISK"),
                    );
                    portfolio_risk_limits = format!(
                    ",portfolio_exposure_basis=filled_working_candidate_v2,portfolio_risk_policy_version={},portfolio_gross_exposure={},portfolio_net_exposure={},portfolio_leverage_bps={},portfolio_concentration_bps={},portfolio_peak_equity={},portfolio_drawdown_bps={},portfolio_daily_baseline_equity={},portfolio_daily_pnl={},portfolio_margin_used={},portfolio_margin_utilization_bps={},portfolio_sector_gross={},portfolio_asset_class_gross={},portfolio_currency_gross={},portfolio_strategy_gross={}",
                    decision.policy_version,
                    decision.metrics.gross_exposure,
                    decision.metrics.net_exposure,
                    decision.metrics.leverage_bps,
                    decision.metrics.concentration_bps,
                    self.peak_equity,
                    decision.metrics.drawdown_bps,
                    self.daily_baseline_equity,
                    observed_equity.checked_sub(self.daily_baseline_equity)?,
                    margin_used,
                    decision.metrics.margin_utilization_bps,
                    render_bucket_map(&decision.metrics.sector_gross),
                    render_bucket_map(&decision.metrics.asset_class_gross),
                    render_bucket_map(&decision.metrics.currency_gross),
                    render_bucket_map(&decision.metrics.strategy_gross),
                );
                    portfolio_risk_limits.push_str(&format!(
                        ",portfolio_possible_abs_net_exposure={},portfolio_possible_concentration_bps={},portfolio_possible_abs_delta={},portfolio_possible_abs_gamma={}",
                        decision.metrics.possible_abs_net_exposure,
                        decision.metrics.possible_concentration_bps,
                        decision.metrics.possible_abs_delta,
                        decision.metrics.possible_abs_gamma,
                    ));
                }
                // Equity is not positive, so no aggregate ratio exists to check.
                // Skipping the check outright would let an underwater account
                // open more exposure past every aggregate limit exactly when
                // they matter, so only a trade that moves this position toward
                // flat may pass, counting what working orders already claim of
                // it. The rest is refused (delivery state E7.4b).
                None => {
                    if !self.reduces_open_position(
                        &intent.instrument_id,
                        current_position,
                        projected_position,
                    )? {
                        reasons.push("PORTFOLIO_EQUITY_NOT_POSITIVE".to_owned());
                    }
                }
            }
        }
        let approved = reasons.is_empty();
        if approved {
            reasons.push("APPROVED".to_owned());
        }
        let evaluated_limits = format!(
            "max_order_quantity={},max_order_notional={},max_price_deviation_bps={},canary_max_order_notional={},canary_max_orders={},max_open_orders={},max_position_quantity={},max_realized_loss={},max_market_data_age_seconds={},max_order_rate={},order_rate_window_seconds={},recent_order_count={},market_instrument_id={},market_mark_price={},market_observed_at={},requested_price={},requested_price_deviation_bps={},estimated_notional={},projected_position={},working_position_delta={},committed_position={},available_cash={},instrument_tick_size={},instrument_lot_size={}{}",
            self.policy.max_order_quantity,
            self.policy.max_order_notional,
            self.policy.max_price_deviation_bps,
            self.policy.canary_max_order_notional,
            self.policy.canary_max_orders,
            self.policy.max_open_orders,
            self.policy.max_position_quantity,
            self.policy.max_realized_loss,
            self.policy.max_market_data_age_seconds,
            self.policy.max_order_rate,
            self.policy.order_rate_window_seconds,
            recent_order_count,
            market.instrument_id,
            market.mark_price,
            market.observed_at,
            intent.limit_price.map_or_else(|| "MARKET".to_owned(), |price| price.to_string()),
            requested_price_deviation_bps,
            estimated_notional,
            projected_position,
            working_position_delta,
            committed_position,
            available_cash,
            self.policy
                .instrument_tick_sizes
                .get(&intent.instrument_id)
                .map_or_else(|| "UNCONFIGURED".to_owned(), ToString::to_string),
            self.policy
                .instrument_lot_sizes
                .get(&intent.instrument_id)
                .map_or_else(|| "UNCONFIGURED".to_owned(), ToString::to_string),
            portfolio_risk_limits,
        );
        Ok(LiveRiskDecision {
            decision_id: format!("live-risk-{}", intent.intent_id),
            approved,
            reason_codes: reasons,
            policy_version: self.policy.version.clone(),
            decided_at: decided_at.to_owned(),
            market_fingerprint: market_fingerprint(market),
            evaluated_limits,
        })
    }

    /// Real point-in-time equity: cash plus every non-zero position marked at
    /// its cached observed mark, falling back to average cost when this
    /// instrument has never been independently quoted. Shared by peak-equity
    /// tracking (always) and the Slice-1/2 aggregate-risk snapshot (when
    /// composed).
    fn current_equity(&self) -> Result<Decimal, LiveError> {
        let mut equity = self.cash;
        for portfolio in self.portfolios.values() {
            let snapshot = portfolio.position_snapshot();
            if snapshot.quantity == Decimal::ZERO {
                continue;
            }
            let mark = self
                .marks
                .get(&snapshot.instrument_id)
                .copied()
                .unwrap_or(snapshot.average_cost);
            equity = equity.checked_add(snapshot.quantity.checked_mul(mark)?)?;
        }
        Ok(equity)
    }

    /// Builds the aggregate-risk snapshot/candidate from real service state
    /// and calls the composed `core/risk` kernel. Returns `Ok(None)` when
    /// computed equity is not positive: the kernel's ratios (leverage,
    /// drawdown, concentration) mean nothing against zero or negative equity.
    /// That is not permission to skip the limits. The caller refuses every
    /// order that does not reduce a position (`PORTFOLIO_EQUITY_NOT_POSITIVE`,
    /// delivery state E7.4b), and the per-order checks apply as always.
    /// On `Some`, the second tuple element is the real margin requirement
    /// computed for the decision (`Decimal::ZERO` when `margin_rates` is not
    /// configured), returned alongside the decision because
    /// `core/risk::AggregateRiskMetrics` only ever reports the *ratio*
    /// (`margin_utilization_bps`), not the raw currency amount that produced
    /// it.
    fn portfolio_risk_decision(
        &self,
        composition: &PortfolioRiskComposition,
        intent: &OrderIntent,
        market: &LiveMarketData,
        decided_at: &str,
    ) -> Result<Option<(follon_risk::PortfolioRiskDecision, Decimal)>, LiveError> {
        let Some((snapshot, margin_used)) = self.portfolio_risk_state(composition, decided_at)?
        else {
            return Ok(None);
        };
        let candidate = self.risk_candidate(
            composition,
            &intent.intent_id,
            &intent.account_id,
            &intent.strategy_id,
            &intent.instrument_id,
            intent.side,
            intent.quantity,
            market.mark_price,
        )?;
        let decision = follon_risk::evaluate_portfolio_risk_with_candidates(
            &composition.policy,
            &snapshot,
            std::slice::from_ref(&candidate),
        )?;
        Ok(Some((decision, margin_used)))
    }

    /// Builds one aggregate-risk candidate row, classified by the operator's
    /// attested bucket table. Shared by the single-order and combination paths
    /// so a combination leg is classified exactly as the same instrument would
    /// be on its own.
    #[allow(clippy::too_many_arguments)]
    fn risk_candidate(
        &self,
        composition: &PortfolioRiskComposition,
        intent_id: &str,
        account_id: &str,
        strategy_id: &str,
        instrument_id: &str,
        side: Side,
        quantity: Decimal,
        mark_price: Decimal,
    ) -> Result<CandidateOrder, LiveError> {
        let bucket = composition.instrument_buckets.get(instrument_id);
        Ok(CandidateOrder {
            intent_id: intent_id.to_owned(),
            account_id: account_id.to_owned(),
            strategy_id: strategy_id.to_owned(),
            instrument_id: instrument_id.to_owned(),
            asset_class: bucket
                .map(|bucket| bucket.asset_class.clone())
                .unwrap_or_else(|| "unclassified".to_owned()),
            sector: bucket
                .map(|bucket| bucket.sector.clone())
                .unwrap_or_else(|| "unclassified".to_owned()),
            currency: bucket
                .map(|bucket| bucket.currency.clone())
                .unwrap_or_else(|| self.account.currency.clone()),
            side,
            quantity,
            mark_price,
            multiplier: Decimal::from_integer(1)?,
            delta: Decimal::ZERO,
            gamma: Decimal::ZERO,
        })
    }

    /// Builds the real aggregate-risk snapshot from service state, without any
    /// candidate.
    ///
    /// Extracted so the single-order and combination paths observe exactly the
    /// same portfolio, equity, peak, daily baseline and margin figures; the only
    /// difference between them is which candidates are then added.
    #[allow(clippy::too_many_arguments)]
    fn working_risk_position(
        &self,
        composition: &PortfolioRiskComposition,
        account_id: &str,
        strategy_id: &str,
        instrument_id: &str,
        side: Side,
        quantity: Decimal,
        mark_price: Decimal,
    ) -> Result<RiskPosition, LiveError> {
        let candidate = self.risk_candidate(
            composition,
            "working.exposure",
            account_id,
            strategy_id,
            instrument_id,
            side,
            quantity,
            mark_price,
        )?;
        Ok(RiskPosition {
            account_id: candidate.account_id,
            strategy_id: candidate.strategy_id,
            instrument_id: candidate.instrument_id,
            asset_class: candidate.asset_class,
            sector: candidate.sector,
            currency: candidate.currency,
            quantity: match side {
                Side::Buy => quantity,
                Side::Sell => Decimal::ZERO.checked_sub(quantity)?,
            },
            mark_price: candidate.mark_price,
            multiplier: candidate.multiplier,
            delta: candidate.delta,
            gamma: candidate.gamma,
        })
    }

    fn portfolio_risk_state(
        &self,
        composition: &PortfolioRiskComposition,
        decided_at: &str,
    ) -> Result<Option<(PortfolioRiskSnapshot, Decimal)>, LiveError> {
        let equity = self.current_equity()?;
        if equity <= Decimal::ZERO {
            return Ok(None);
        }
        let mut positions = Vec::new();
        for portfolio in self.portfolios.values() {
            let snapshot = portfolio.position_snapshot();
            if snapshot.quantity == Decimal::ZERO {
                continue;
            }
            let mark = self
                .marks
                .get(&snapshot.instrument_id)
                .copied()
                .unwrap_or(snapshot.average_cost);
            let bucket = composition.instrument_buckets.get(&snapshot.instrument_id);
            let asset_class = bucket
                .map(|bucket| bucket.asset_class.clone())
                .unwrap_or_else(|| "unclassified".to_owned());
            let sector = bucket
                .map(|bucket| bucket.sector.clone())
                .unwrap_or_else(|| "unclassified".to_owned());
            let currency = bucket
                .map(|bucket| bucket.currency.clone())
                .unwrap_or_else(|| self.account.currency.clone());
            // Slice 2d: split the aggregate position into one `RiskPosition`
            // row per strategy that has ever traded this instrument, plus
            // one "unattributed" remainder row for whatever the tracked
            // strategies do not account for (a legacy journal, or a fill
            // predating this ledger). Every row's quantity always
            // reconciles exactly against `snapshot.quantity` (see
            // `apply_strategy_attribution_fill`), so gross/net exposure can
            // never be mis-stated by this split, only how it is attributed.
            let mut attributed_total = Decimal::ZERO;
            if let Some(strategies) = self.strategy_attribution.get(&snapshot.instrument_id) {
                for (strategy_id, quantity) in strategies {
                    if *quantity == Decimal::ZERO {
                        continue;
                    }
                    attributed_total = attributed_total.checked_add(*quantity)?;
                    positions.push(RiskPosition {
                        account_id: snapshot.account_id.clone(),
                        strategy_id: strategy_id.clone(),
                        instrument_id: snapshot.instrument_id.clone(),
                        asset_class: asset_class.clone(),
                        sector: sector.clone(),
                        currency: currency.clone(),
                        quantity: *quantity,
                        mark_price: mark,
                        multiplier: Decimal::from_integer(1)?,
                        delta: Decimal::ZERO,
                        gamma: Decimal::ZERO,
                    });
                }
            }
            let remainder = snapshot.quantity.checked_sub(attributed_total)?;
            if remainder != Decimal::ZERO {
                positions.push(RiskPosition {
                    account_id: snapshot.account_id,
                    strategy_id: "unattributed".to_owned(),
                    instrument_id: snapshot.instrument_id,
                    asset_class,
                    sector,
                    currency,
                    quantity: remainder,
                    mark_price: mark,
                    multiplier: Decimal::from_integer(1)?,
                    delta: Decimal::ZERO,
                    gamma: Decimal::ZERO,
                });
            }
        }
        let mut working_positions = Vec::new();
        // Risk exposure includes quantities the broker may still fill, even though
        // cash, equity and the filled-position ledger have not moved yet (E7.14).
        for order in self.orders.values().filter(|order| order.working()) {
            let intent = &order.oms.intent;
            let remaining = intent.quantity.checked_sub(order.filled_quantity)?;
            if remaining > Decimal::ZERO {
                let mark = self
                    .marks
                    .get(&intent.instrument_id)
                    .copied()
                    .unwrap_or(order.market.mark_price);
                working_positions.push(self.working_risk_position(
                    composition,
                    &intent.account_id,
                    &intent.strategy_id,
                    &intent.instrument_id,
                    intent.side,
                    remaining,
                    mark,
                )?);
            }
        }
        for order in self.combo_orders.values().filter(|order| order.working()) {
            let intent = &order.oms.intent;
            let unfilled = intent.combo_quantity.checked_sub(order.filled_quantity)?;
            if unfilled == Decimal::ZERO {
                continue;
            }
            for leg in &intent.legs {
                let mark = self
                    .marks
                    .get(&leg.instrument_id)
                    .copied()
                    .or_else(|| {
                        order
                            .market
                            .mark_for(&leg.instrument_id)
                            .map(|seen| seen.mark_price)
                    })
                    .ok_or_else(|| LiveError("working combo leg has no market mark".to_owned()))?;
                let quantity =
                    unfilled.checked_mul(Decimal::from_integer(i64::from(leg.ratio))?)?;
                working_positions.push(self.working_risk_position(
                    composition,
                    &intent.account_id,
                    &intent.strategy_id,
                    &leg.instrument_id,
                    leg.side,
                    quantity,
                    mark,
                )?);
            }
        }
        let resting_orders = self
            .orders
            .values()
            .filter(|order| order.working())
            .map(|order| RestingOrder {
                order_id: order.oms.order_id.clone(),
                account_id: order.oms.intent.account_id.clone(),
                instrument_id: order.oms.intent.instrument_id.clone(),
                side: order.oms.intent.side,
            })
            .collect::<Vec<_>>();
        // Real, computed only when `margin_rates` is configured (Slice 2c):
        // the *currently held* margin requirement, not a projection that
        // includes the candidate order -- the same "pre-trade observed, not
        // post-trade projected" convention `equity`/`peak_equity`/
        // `daily_baseline_equity` already use above. Every asset class among
        // currently held positions must have a configured rate or this fails
        // closed with a technical error rather than silently under-counting
        // margin -- an intentional operator-configuration requirement, not a
        // soft risk rejection.
        let margin_used = if let Some(rates) = composition.margin_rates.as_ref() {
            let mut margin_positions = Vec::new();
            for portfolio in self.portfolios.values() {
                let snapshot = portfolio.position_snapshot();
                if snapshot.quantity == Decimal::ZERO {
                    continue;
                }
                let mark = self
                    .marks
                    .get(&snapshot.instrument_id)
                    .copied()
                    .unwrap_or(snapshot.average_cost);
                let bucket = composition.instrument_buckets.get(&snapshot.instrument_id);
                margin_positions.push(MarginPosition {
                    instrument_id: snapshot.instrument_id,
                    asset_class: bucket
                        .map(|bucket| bucket.asset_class.clone())
                        .unwrap_or_else(|| "unclassified".to_owned()),
                    currency: Currency::new(
                        bucket
                            .map(|bucket| bucket.currency.clone())
                            .unwrap_or_else(|| self.account.currency.clone()),
                    )?,
                    quantity: snapshot.quantity,
                    mark_price: mark,
                    multiplier: Decimal::from_integer(1)?,
                });
            }
            let account_currency = Currency::new(self.account.currency.clone())?;
            let mut cash_by_currency = BTreeMap::new();
            cash_by_currency.insert(account_currency.clone(), self.cash);
            let margin_policy = MarginPolicy {
                base_currency: account_currency,
                // Never actually consulted: every position and cash balance
                // here is denominated in the account's own currency, so
                // `FxBook::convert` always takes its same-currency fast path
                // and never reaches a freshness check.
                maximum_fx_age_seconds: i64::MAX,
                rates: rates.clone(),
            };
            let as_of_epoch_seconds = OffsetDateTime::parse(decided_at, &Rfc3339)
                .map_err(|error| LiveError(error.to_string()))?
                .unix_timestamp();
            follon_accounting::value_margin_account(
                &cash_by_currency,
                &margin_positions,
                &FxBook::default(),
                &margin_policy,
                as_of_epoch_seconds,
            )?
            .initial_margin
        } else {
            Decimal::ZERO
        };
        let snapshot = PortfolioRiskSnapshot {
            equity,
            // Real, durable running high-water-mark (see `peak_equity` on
            // `LiveTradingService`) -- never below `equity` itself, since
            // `evaluate_risk` updates it from the same observation before
            // this function ever runs.
            peak_equity: self.peak_equity.max(equity),
            // Real, durable session-start baseline (see `daily_baseline_equity`
            // on `LiveTradingService`) -- `evaluate_risk` updates it from the
            // same observation before this function ever runs.
            daily_pnl: equity.checked_sub(self.daily_baseline_equity)?,
            margin_used,
            positions,
            working_positions,
            resting_orders,
            recent_order_count: 0,
        };
        Ok(Some((snapshot, margin_used)))
    }

    fn apply_broker_event(&mut self, event: LiveBrokerEvent) -> Result<(), LiveError> {
        if self.combo_orders.contains_key(event.client_order_id()) {
            return self.apply_combo_event(event);
        }
        match event {
            LiveBrokerEvent::ComboExecution(_) => {
                return Err(LiveError(
                    "live combo execution does not name a combination".to_owned(),
                ))
            }
            LiveBrokerEvent::Acknowledged {
                client_order_id,
                broker_order_id,
            } => {
                validate_canonical_id("live broker client_order_id", &client_order_id)?;
                validate_canonical_id("live broker_order_id", &broker_order_id)?;
                let order = self.order_mut(&client_order_id)?;
                if let Some(existing) = &order.broker_order_id {
                    if existing != &broker_order_id {
                        if order.broker_order_versions.contains(&broker_order_id) {
                            return Ok(());
                        }
                        return Err(LiveError(
                            "broker reused a live client ID with an unrecognized broker ID"
                                .to_owned(),
                        ));
                    }
                }
                if order.broker_order_id.is_none() {
                    order.broker_order_id = Some(broker_order_id.clone());
                }
                if !order.broker_order_versions.contains(&broker_order_id) {
                    order.broker_order_versions.push(broker_order_id);
                }
                if !order.working() {
                    return Ok(());
                }
                transition_to_acknowledged(order, "LIVE_BROKER_ACKNOWLEDGEMENT")?;
            }
            LiveBrokerEvent::Execution {
                execution_id,
                client_order_id,
                broker_order_id,
                quantity,
                price,
                fee,
                executed_at,
            } => {
                validate_canonical_id("live execution_id", &execution_id)?;
                validate_canonical_id("live execution client_order_id", &client_order_id)?;
                validate_canonical_id("live execution broker_order_id", &broker_order_id)?;
                validate_utc_timestamp("live execution time", &executed_at)?;
                if quantity <= Decimal::ZERO || price <= Decimal::ZERO || fee < Decimal::ZERO {
                    return Err(LiveError("live execution values are invalid".to_owned()));
                }
                if self.combo_execution_owns_id(&execution_id) {
                    return Err(LiveError(
                        "live execution identity belongs to combination evidence".to_owned(),
                    ));
                }
                if self.execution_ids.contains(&execution_id) {
                    return Ok(());
                }
                let (instrument_id, side, order_id, strategy_id) = {
                    let order = self.order_mut(&client_order_id)?;
                    if let Some(existing) = &order.broker_order_id {
                        if existing != &broker_order_id
                            && !order.broker_order_versions.contains(&broker_order_id)
                        {
                            return Err(LiveError(
                                "live execution has an unrecognized broker order version"
                                    .to_owned(),
                            ));
                        }
                    } else {
                        order.broker_order_id = Some(broker_order_id.clone());
                    }
                    if !order.broker_order_versions.contains(&broker_order_id) {
                        order.broker_order_versions.push(broker_order_id.clone());
                    }
                    if matches!(
                        order.oms.state,
                        OrderState::Cancelled | OrderState::Rejected | OrderState::Expired
                    ) {
                        order.oms.transition(
                            OrderState::Unknown,
                            "LATE_LIVE_BROKER_EXECUTION_AFTER_TERMINAL",
                        )?;
                    }
                    transition_to_acknowledged(order, "LIVE_EXECUTION_CONFIRMED_ORDER")?;
                    let total = order.filled_quantity.checked_add(quantity)?;
                    if total > order.oms.intent.quantity {
                        return Err(LiveError(
                            "live execution exceeds requested quantity".to_owned(),
                        ));
                    }
                    order.filled_quantity = total;
                    if total == order.oms.intent.quantity {
                        if order.oms.state != OrderState::Filled {
                            order
                                .oms
                                .transition(OrderState::Filled, "LIVE_BROKER_FULL_FILL")?;
                        }
                    } else if order.oms.state == OrderState::Acknowledged {
                        order
                            .oms
                            .transition(OrderState::PartiallyFilled, "LIVE_BROKER_PARTIAL_FILL")?;
                    }
                    (
                        order.oms.intent.instrument_id.clone(),
                        order.oms.intent.side,
                        order.oms.order_id.clone(),
                        order.oms.intent.strategy_id.clone(),
                    )
                };
                let fill = Fill {
                    execution_id,
                    order_id,
                    instrument_id,
                    side,
                    quantity,
                    price,
                    fee,
                    executed_at,
                };
                self.apply_accounted_fill(&fill, &strategy_id)?;
                if self.cash < Decimal::ZERO {
                    self.record_internal_incident(
                        "LIVE_CASH_OVERDRAFT",
                        self.account.account_id.clone(),
                        "a broker execution exceeded independently available cash".to_owned(),
                    );
                }
            }
            LiveBrokerEvent::Cancelled {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("live cancellation client_order_id", &client_order_id)?;
                validate_reason("live cancellation reason", &reason)?;
                transition_to_terminal(
                    self.order_mut(&client_order_id)?,
                    OrderState::Cancelled,
                    &reason,
                )?;
            }
            LiveBrokerEvent::CancelRejected {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("live cancellation client_order_id", &client_order_id)?;
                validate_reason("live cancel rejection reason", &reason)?;
                let order = self.order_mut(&client_order_id)?;
                match order.oms.state {
                    OrderState::PendingCancel | OrderState::Unknown => {
                        restore_working_state(order, "LIVE_BROKER_CANCEL_REJECTED")?;
                    }
                    OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired => {}
                    _ => {
                        return Err(LiveError(
                            "live cancel rejection is incompatible with OMS state".to_owned(),
                        ))
                    }
                }
            }
            LiveBrokerEvent::Expired {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("live expiry client_order_id", &client_order_id)?;
                validate_reason("live expiry reason", &reason)?;
                transition_to_terminal(
                    self.order_mut(&client_order_id)?,
                    OrderState::Expired,
                    &reason,
                )?;
            }
            LiveBrokerEvent::ReplaceRequested {
                client_order_id,
                previous_broker_order_id,
            } => {
                validate_canonical_id("live replacement client_order_id", &client_order_id)?;
                validate_canonical_id(
                    "live replacement previous_order_id",
                    &previous_broker_order_id,
                )?;
                let order = self.order_mut(&client_order_id)?;
                if order.broker_order_id.as_deref() != Some(previous_broker_order_id.as_str()) {
                    return Err(LiveError(
                        "live replacement does not match active broker order".to_owned(),
                    ));
                }
                match order.oms.state {
                    OrderState::Acknowledged | OrderState::PartiallyFilled => {
                        order.replace_return_state = Some(order.oms.state);
                        order.oms.transition(
                            OrderState::PendingReplace,
                            "LIVE_BROKER_REPLACE_REQUESTED",
                        )?;
                    }
                    OrderState::PendingReplace => {}
                    OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired => {}
                    _ => {
                        return Err(LiveError(
                            "live replacement is incompatible with OMS state".to_owned(),
                        ))
                    }
                }
            }
            LiveBrokerEvent::Replaced {
                client_order_id,
                previous_broker_order_id,
                broker_order_id,
            } => {
                validate_canonical_id("live replacement client_order_id", &client_order_id)?;
                validate_canonical_id(
                    "live replacement previous_order_id",
                    &previous_broker_order_id,
                )?;
                validate_canonical_id("live replacement_order_id", &broker_order_id)?;
                let order = self.order_mut(&client_order_id)?;
                if order.broker_order_id.as_deref() != Some(previous_broker_order_id.as_str()) {
                    if order.broker_order_versions.contains(&broker_order_id) {
                        return Ok(());
                    }
                    return Err(LiveError(
                        "live replacement does not match active broker order".to_owned(),
                    ));
                }
                match order.oms.state {
                    OrderState::PendingReplace | OrderState::Unknown => {
                        if !order.broker_order_versions.contains(&broker_order_id) {
                            order.broker_order_versions.push(broker_order_id.clone());
                        }
                        order.broker_order_id = Some(broker_order_id);
                        restore_replacement_state(order, "LIVE_BROKER_REPLACED")?;
                    }
                    OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired => {}
                    _ => {
                        return Err(LiveError(
                            "live replacement is incompatible with OMS state".to_owned(),
                        ))
                    }
                }
            }
            LiveBrokerEvent::ReplaceRejected {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("live replacement client_order_id", &client_order_id)?;
                validate_reason("live replacement rejection reason", &reason)?;
                let order = self.order_mut(&client_order_id)?;
                match order.oms.state {
                    OrderState::PendingReplace | OrderState::Unknown => {
                        restore_replacement_state(order, "LIVE_BROKER_REPLACE_REJECTED")?;
                    }
                    OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired => {}
                    _ => {
                        return Err(LiveError(
                            "live replacement rejection is incompatible with OMS state".to_owned(),
                        ))
                    }
                }
            }
            LiveBrokerEvent::Rejected {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("live rejection client_order_id", &client_order_id)?;
                validate_reason("live rejection reason", &reason)?;
                transition_to_terminal(
                    self.order_mut(&client_order_id)?,
                    OrderState::Rejected,
                    &reason,
                )?;
            }
        }
        Ok(())
    }

    /// Applies one exact fill through every LIVE accounting projection.
    ///
    /// Plain orders and every leg of an atomic combination share this path so
    /// cash, positions, tax lots, strategy attribution, and receipt identity
    /// cannot diverge by order shape. Signed positions are reachable only when
    /// the independently configured LIVE short-exposure permission exists.
    fn apply_accounted_fill(&mut self, fill: &Fill, strategy_id: &str) -> Result<(), LiveError> {
        let portfolio = self
            .portfolios
            .entry(fill.instrument_id.clone())
            .or_insert_with(|| Portfolio::new(&self.account.account_id, &fill.instrument_id));
        if self.policy.short_exposure.is_some() {
            portfolio.apply_signed_fill(fill)?;
        } else {
            portfolio.apply_fill(fill)?;
        }
        self.apply_tax_lot_fill(fill)?;
        self.apply_strategy_attribution_fill(fill, strategy_id)?;
        let gross = fill.price.checked_mul(fill.quantity)?;
        self.cash = match fill.side {
            Side::Buy => self.cash.checked_sub(gross.checked_add(fill.fee)?)?,
            Side::Sell => self.cash.checked_add(gross.checked_sub(fill.fee)?)?,
        };
        self.execution_ids.insert(fill.execution_id.clone());
        Ok(())
    }

    /// Updates the independent FIFO long/short tax-lot ledger. A crossing fill
    /// closes opposite inventory first and splits its fee exactly once.
    fn apply_tax_lot_fill(&mut self, fill: &Fill) -> Result<(), LiveError> {
        let currency = Currency::new(self.account.currency.clone())?;
        let available = match fill.side {
            Side::Buy => self
                .tax_lots
                .short_lots(&fill.instrument_id)
                .iter()
                .try_fold(Decimal::ZERO, |total, lot| {
                    total.checked_add(lot.remaining_quantity)
                })?,
            Side::Sell => self
                .tax_lots
                .lots(&fill.instrument_id)
                .iter()
                .try_fold(Decimal::ZERO, |total, lot| {
                    total.checked_add(lot.remaining_quantity)
                })?,
        };
        let closing = available.min(fill.quantity);
        let opening = fill.quantity.checked_sub(closing)?;
        let close_fee = if closing == fill.quantity {
            fill.fee
        } else {
            fill.fee.checked_mul(closing)?.checked_div(fill.quantity)?
        };
        if closing > Decimal::ZERO {
            match fill.side {
                Side::Buy => {
                    self.tax_lots.cover(
                        &format!("taxcover-{}", fill.execution_id),
                        &fill.instrument_id,
                        &currency,
                        closing,
                        fill.price,
                        close_fee,
                        &fill.executed_at,
                        TaxLotSelection::Fifo,
                    )?;
                }
                Side::Sell => {
                    self.tax_lots.dispose(
                        &format!("taxdisposal-{}", fill.execution_id),
                        &fill.instrument_id,
                        &currency,
                        closing,
                        fill.price,
                        close_fee,
                        &fill.executed_at,
                        TaxLotSelection::Fifo,
                    )?;
                }
            }
        }
        if opening == Decimal::ZERO {
            return Ok(());
        }
        let open_fee = fill.fee.checked_sub(close_fee)?;
        match fill.side {
            Side::Buy => {
                let gross = fill.price.checked_mul(opening)?;
                let unit_cost = gross.checked_add(open_fee)?.checked_div(opening)?;
                self.tax_lots.acquire(TaxLot {
                    lot_id: format!("taxlot-{}", fill.execution_id),
                    instrument_id: fill.instrument_id.clone(),
                    currency,
                    opened_at: fill.executed_at.clone(),
                    remaining_quantity: opening,
                    unit_cost,
                })?;
            }
            Side::Sell => {
                self.tax_lots.open_short(ShortTaxLot {
                    lot_id: format!("taxshort-{}", fill.execution_id),
                    instrument_id: fill.instrument_id.clone(),
                    currency,
                    opened_at: fill.executed_at.clone(),
                    remaining_quantity: opening,
                    unit_proceeds: fill
                        .price
                        .checked_mul(opening)?
                        .checked_sub(open_fee)?
                        .checked_div(opening)?,
                })?;
            }
        }
        Ok(())
    }

    /// Updates each strategy's own net signed contribution to one
    /// instrument's aggregate position. See `core/paper`'s identical method
    /// for the full rationale: a buy adds, a sell subtracts, never clamped
    /// or floored at zero, so summed across every strategy it always
    /// reconciles exactly against `Portfolio`'s own aggregate quantity. This
    /// is a deliberately separate, live-local bookkeeping layer, not a
    /// change to `Portfolio` or `PositionSnapshot` (the canonical position of
    /// record, also part of the audit event schema) -- see
    /// docs/06-delivery/14-master-plan-conformance-audit.md (row 5.7) for why
    /// that remains explicitly out of scope.
    fn apply_strategy_attribution_fill(
        &mut self,
        fill: &Fill,
        strategy_id: &str,
    ) -> Result<(), LiveError> {
        let instrument_attribution = self
            .strategy_attribution
            .entry(fill.instrument_id.clone())
            .or_default();
        let current = instrument_attribution
            .get(strategy_id)
            .copied()
            .unwrap_or(Decimal::ZERO);
        let updated = match fill.side {
            Side::Buy => current.checked_add(fill.quantity)?,
            Side::Sell => current.checked_sub(fill.quantity)?,
        };
        instrument_attribution.insert(strategy_id.to_owned(), updated);
        Ok(())
    }

    /// Remaining open FIFO tax lots for one instrument, oldest first.
    pub fn tax_lots(&self, instrument_id: &str) -> &[TaxLot] {
        self.tax_lots.lots(instrument_id)
    }

    /// Cumulative FIFO-realized tax P&L in the account's reporting currency,
    /// independent of `Portfolio`'s average-cost realized P&L.
    pub fn realized_tax_pnl(&self) -> Result<Decimal, LiveError> {
        let currency = Currency::new(self.account.currency.clone())?;
        Ok(self.tax_lots.realized(&currency))
    }

    fn record_internal_incident(&mut self, category: &str, subject: String, detail: String) {
        if self.incidents.values().any(|incident| {
            incident.unexplained()
                && incident.issue.category == category
                && incident.issue.subject == subject
        }) {
            return;
        }
        let incident_id = format!("incident-internal-{:03}", self.incidents.len() + 1);
        self.incidents
            .entry(incident_id.clone())
            .or_insert_with(|| LiveIncident {
                issue: LiveReconciliationIssue {
                    incident_id,
                    category: category.to_owned(),
                    subject,
                    detail,
                },
                explanation: None,
            });
    }

    fn persist(
        &mut self,
        event_type: &str,
        actor: &str,
        occurred_at: &str,
        correlation_id: &str,
    ) -> Result<(), LiveError> {
        let state = self.persistent_state();
        if let Err(error) =
            self.journal
                .append(event_type, occurred_at, actor, correlation_id, state)
        {
            self.audit_healthy = false;
            self.broker_connected = false;
            return Err(error);
        }
        Ok(())
    }

    fn ensure_audit_healthy(&self) -> Result<(), LiveError> {
        if self.audit_healthy {
            Ok(())
        } else {
            Err(LiveError(
                "controlled-live service is halted after an audit write failure".to_owned(),
            ))
        }
    }

    fn unresolved_incident_count(&self) -> u32 {
        self.incidents
            .values()
            .filter(|incident| incident.unexplained())
            .count() as u32
    }

    fn persistent_state(&self) -> PersistentLiveState {
        PersistentLiveState {
            configuration_fingerprint: self.configuration_fingerprint(),
            account_id: self.account.account_id.clone(),
            currency: self.account.currency.clone(),
            cash: self.cash.to_string(),
            orders: self
                .orders
                .iter()
                .map(|(order_id, order)| {
                    (
                        order_id.clone(),
                        PersistentLiveOrder {
                            intent: PersistentIntent::from(&order.oms.intent),
                            approval_id: order.approval_id.clone(),
                            state: order.oms.state.as_str().to_owned(),
                            market: PersistentMarketData::from(&order.market),
                            decision: PersistentRiskDecision::from(&order.decision),
                            broker_order_id: order.broker_order_id.clone(),
                            broker_order_versions: order.broker_order_versions.clone(),
                            replace_return_state: order
                                .replace_return_state
                                .map(OrderState::as_str)
                                .map(str::to_owned),
                            filled_quantity: order.filled_quantity.to_string(),
                        },
                    )
                })
                .collect(),
            approvals: self
                .approvals
                .iter()
                .map(|(approval_id, registered)| {
                    (
                        approval_id.clone(),
                        PersistentLiveApproval {
                            approval: PersistentApproval::from(&registered.approval),
                            consumed: registered.consumed,
                        },
                    )
                })
                .collect(),
            positions: self
                .portfolios
                .iter()
                .map(|(instrument_id, portfolio)| {
                    let position = portfolio.position_snapshot();
                    (
                        instrument_id.clone(),
                        PersistentPosition {
                            quantity: position.quantity.to_string(),
                            average_cost: position.average_cost.to_string(),
                            realized_pnl: position.realized_pnl.to_string(),
                        },
                    )
                })
                .collect(),
            execution_ids: self.execution_ids.iter().cloned().collect(),
            active_kill_switches: self.kill_switches.active_keys(),
            incidents: self
                .incidents
                .iter()
                .map(|(incident_id, incident)| {
                    (
                        incident_id.clone(),
                        PersistentIncident {
                            category: incident.issue.category.clone(),
                            subject: incident.issue.subject.clone(),
                            detail: incident.issue.detail.clone(),
                            explanation: incident.explanation.clone(),
                        },
                    )
                })
                .collect(),
            live_days: self.live_days.clone(),
            canary_submissions: self.canary_submissions,
            next_reconciliation: self.next_reconciliation,
            last_reconciled_at: self.last_reconciled_at.clone(),
            last_reconciliation_clean: self.last_reconciliation_clean,
            latest_reconciliation: self
                .latest_reconciliation
                .as_ref()
                .map(PersistentReconciliationReport::from),
            tax_lots: {
                let snapshot = self.tax_lots.snapshot();
                PersistentTaxLotBook {
                    lots: snapshot
                        .lots
                        .into_iter()
                        .map(|(instrument_id, lots)| {
                            let persisted = lots
                                .into_iter()
                                .map(|lot| PersistentTaxLot {
                                    lot_id: lot.lot_id,
                                    opened_at: lot.opened_at,
                                    remaining_quantity: lot.remaining_quantity.to_string(),
                                    unit_cost: lot.unit_cost.to_string(),
                                })
                                .collect();
                            (instrument_id, persisted)
                        })
                        .collect(),
                    applied_lot_ids: snapshot.applied_lot_ids.into_iter().collect(),
                    applied_disposal_ids: snapshot.applied_disposal_ids.into_iter().collect(),
                    realized_by_currency: snapshot
                        .realized_by_currency
                        .into_iter()
                        .map(|(currency, amount)| {
                            (currency.as_str().to_owned(), amount.to_string())
                        })
                        .collect(),
                    short_lots: snapshot
                        .short_lots
                        .into_iter()
                        .map(|(instrument_id, lots)| {
                            let persisted = lots
                                .into_iter()
                                .map(|lot| PersistentTaxLot {
                                    lot_id: lot.lot_id,
                                    opened_at: lot.opened_at,
                                    remaining_quantity: lot.remaining_quantity.to_string(),
                                    unit_cost: lot.unit_proceeds.to_string(),
                                })
                                .collect();
                            (instrument_id, persisted)
                        })
                        .collect(),
                    applied_short_lot_ids: snapshot.applied_short_lot_ids.into_iter().collect(),
                    applied_cover_ids: snapshot.applied_cover_ids.into_iter().collect(),
                }
            },
            marks: self
                .marks
                .iter()
                .map(|(instrument_id, mark)| (instrument_id.clone(), mark.to_string()))
                .collect(),
            peak_equity: Some(self.peak_equity.to_string()),
            daily_baseline_date: self.daily_baseline_date.clone(),
            daily_baseline_equity: self
                .daily_baseline_date
                .as_ref()
                .map(|_| self.daily_baseline_equity.to_string()),
            combo_orders: self
                .combo_orders
                .iter()
                .map(|(order_id, order)| {
                    (
                        order_id.clone(),
                        PersistentLiveComboOrder {
                            intent: PersistentComboIntent::from(&order.oms.intent),
                            approval_id: order.approval_id.clone(),
                            state: order.oms.state.as_str().to_owned(),
                            market: order
                                .market
                                .marks
                                .iter()
                                .map(PersistentMarketData::from)
                                .collect(),
                            decision: PersistentRiskDecision::from(&order.decision),
                            broker_order_id: order.broker_order_id.clone(),
                            broker_order_versions: order.broker_order_versions.clone(),
                            filled_quantity: order.filled_quantity.to_string(),
                            executions: order
                                .executions
                                .values()
                                .map(|execution| PersistentLiveComboExecution {
                                    execution_id: execution.execution_id.clone(),
                                    client_order_id: execution.client_order_id.clone(),
                                    broker_order_id: execution.broker_order_id.clone(),
                                    units: execution.units.to_string(),
                                    legs: execution
                                        .legs
                                        .iter()
                                        .map(|leg| PersistentLiveComboExecutionLeg {
                                            execution_id: leg.execution_id.clone(),
                                            instrument_id: leg.instrument_id.clone(),
                                            side: leg.side.as_str().to_owned(),
                                            quantity: leg.quantity.to_string(),
                                            price: leg.price.to_string(),
                                            fee: leg.fee.to_string(),
                                            executed_at: leg.executed_at.clone(),
                                        })
                                        .collect(),
                                })
                                .collect(),
                        },
                    )
                })
                .collect(),
            strategy_attribution: self
                .strategy_attribution
                .iter()
                .map(|(instrument_id, strategies)| {
                    (
                        instrument_id.clone(),
                        strategies
                            .iter()
                            .map(|(strategy_id, quantity)| {
                                (strategy_id.clone(), quantity.to_string())
                            })
                            .collect(),
                    )
                })
                .collect(),
            corporate_actions: self
                .corporate_actions
                .iter()
                .map(PersistentLiveCorporateAction::from)
                .collect(),
        }
    }

    fn restore(&mut self, state: PersistentLiveState) -> Result<(), LiveError> {
        if state.configuration_fingerprint != self.configuration_fingerprint()
            || state.account_id != self.account.account_id
            || state.currency != self.account.currency
        {
            return Err(LiveError(
                "live audit journal configuration does not match supplied activation".to_owned(),
            ));
        }
        self.cash = decimal("persisted live cash", &state.cash)?;
        let mut approvals = BTreeMap::new();
        for (approval_id, persisted) in state.approvals {
            validate_canonical_id("persisted live approval_id", &approval_id)?;
            let approval = LiveApproval::try_from(persisted.approval)?;
            approval.validate(&self.configuration_fingerprint())?;
            if approval.approval_id != approval_id {
                return Err(LiveError(
                    "persisted live approval key does not match data".to_owned(),
                ));
            }
            approvals.insert(
                approval_id,
                RegisteredLiveApproval {
                    approval,
                    consumed: persisted.consumed,
                },
            );
        }
        let mut orders = BTreeMap::new();
        for (order_id, persisted) in state.orders {
            let intent = OrderIntent::try_from(persisted.intent)?;
            if intent.environment != "LIVE" || intent.account_id != self.account.account_id {
                return Err(LiveError(
                    "persisted live order has incompatible environment or account".to_owned(),
                ));
            }
            validate_canonical_id("persisted order approval_id", &persisted.approval_id)?;
            let approval = approvals.get(&persisted.approval_id).ok_or_else(|| {
                LiveError("persisted live order is missing its approval".to_owned())
            })?;
            if !approval.consumed || approval.approval.intent_id != intent.intent_id {
                return Err(LiveError(
                    "persisted live order approval is invalid".to_owned(),
                ));
            }
            let market = LiveMarketData::try_from(persisted.market)?;
            if market.instrument_id != intent.instrument_id {
                return Err(LiveError(
                    "persisted live market does not match order instrument".to_owned(),
                ));
            }
            let decision = LiveRiskDecision::try_from(persisted.decision)?;
            if !decision.approved
                || decision.decision_id != format!("live-risk-{}", intent.intent_id)
                || decision.policy_version != self.policy.version
                || decision.market_fingerprint != market_fingerprint(&market)
            {
                return Err(LiveError(
                    "persisted live risk decision does not bind the order and policy".to_owned(),
                ));
            }
            if let Some(broker_order_id) = &persisted.broker_order_id {
                validate_canonical_id("persisted live broker_order_id", broker_order_id)?;
            }
            let mut broker_order_versions = persisted.broker_order_versions;
            if let Some(broker_order_id) = &persisted.broker_order_id {
                if broker_order_versions.is_empty() {
                    broker_order_versions.push(broker_order_id.clone());
                }
            }
            let mut unique_versions = BTreeSet::new();
            for broker_order_id in &broker_order_versions {
                validate_canonical_id("persisted live broker_order_version", broker_order_id)?;
                if !unique_versions.insert(broker_order_id) {
                    return Err(LiveError(
                        "persisted live broker order versions are duplicated".to_owned(),
                    ));
                }
            }
            if let Some(broker_order_id) = &persisted.broker_order_id {
                if !unique_versions.contains(broker_order_id) {
                    return Err(LiveError(
                        "persisted active live broker ID is absent from versions".to_owned(),
                    ));
                }
            }
            let filled_quantity =
                decimal("persisted live filled quantity", &persisted.filled_quantity)?;
            if filled_quantity < Decimal::ZERO || filled_quantity > intent.quantity {
                return Err(LiveError(
                    "persisted live filled quantity is invalid".to_owned(),
                ));
            }
            let oms = OmsOrder::recover(
                order_id.clone(),
                intent,
                parse_order_state(&persisted.state)?,
            )?;
            let replace_return_state = persisted
                .replace_return_state
                .as_deref()
                .map(parse_order_state)
                .transpose()?;
            if oms.state == OrderState::PendingReplace && replace_return_state.is_none() {
                return Err(LiveError(
                    "persisted pending live replacement has no return state".to_owned(),
                ));
            }
            orders.insert(
                order_id,
                LiveOrder {
                    oms,
                    approval_id: persisted.approval_id,
                    market,
                    decision,
                    broker_order_id: persisted.broker_order_id,
                    broker_order_versions,
                    replace_return_state,
                    filled_quantity,
                },
            );
        }
        let mut portfolios = BTreeMap::new();
        for (instrument_id, position) in state.positions {
            let quantity = decimal("persisted live position quantity", &position.quantity)?;
            let average_cost = decimal("persisted live average cost", &position.average_cost)?;
            let realized_pnl = decimal("persisted live realized pnl", &position.realized_pnl)?;
            let portfolio = if self.policy.short_exposure.is_some() {
                Portfolio::recover_signed(
                    &self.account.account_id,
                    instrument_id.clone(),
                    quantity,
                    average_cost,
                    realized_pnl,
                )?
            } else {
                Portfolio::recover(
                    &self.account.account_id,
                    instrument_id.clone(),
                    quantity,
                    average_cost,
                    realized_pnl,
                )?
            };
            portfolios.insert(instrument_id, portfolio);
        }
        let account_currency = Currency::new(self.account.currency.clone())?;
        let mut lots = BTreeMap::new();
        for (instrument_id, persisted_lots) in state.tax_lots.lots {
            let mut instrument_lots = Vec::with_capacity(persisted_lots.len());
            for persisted in persisted_lots {
                instrument_lots.push(TaxLot {
                    lot_id: persisted.lot_id,
                    instrument_id: instrument_id.clone(),
                    currency: account_currency.clone(),
                    opened_at: persisted.opened_at,
                    remaining_quantity: decimal(
                        "persisted live tax lot remaining quantity",
                        &persisted.remaining_quantity,
                    )?,
                    unit_cost: decimal("persisted live tax lot unit cost", &persisted.unit_cost)?,
                });
            }
            lots.insert(instrument_id, instrument_lots);
        }
        let mut realized_by_currency = BTreeMap::new();
        for (currency, amount) in state.tax_lots.realized_by_currency {
            realized_by_currency.insert(
                Currency::new(currency)?,
                decimal("persisted live realized tax pnl", &amount)?,
            );
        }
        let mut short_lots = BTreeMap::new();
        for (instrument_id, persisted_lots) in state.tax_lots.short_lots {
            let instrument_lots = persisted_lots
                .into_iter()
                .map(|persisted| {
                    Ok(ShortTaxLot {
                        lot_id: persisted.lot_id,
                        instrument_id: instrument_id.clone(),
                        currency: account_currency.clone(),
                        opened_at: persisted.opened_at,
                        remaining_quantity: decimal(
                            "persisted live short lot remaining quantity",
                            &persisted.remaining_quantity,
                        )?,
                        unit_proceeds: decimal(
                            "persisted live short lot unit proceeds",
                            &persisted.unit_cost,
                        )?,
                    })
                })
                .collect::<Result<Vec<_>, LiveError>>()?;
            short_lots.insert(instrument_id, instrument_lots);
        }
        let tax_lots = TaxLotBook::recover(TaxLotBookSnapshot {
            lots,
            applied_lot_ids: state.tax_lots.applied_lot_ids.into_iter().collect(),
            applied_disposal_ids: state.tax_lots.applied_disposal_ids.into_iter().collect(),
            realized_by_currency,
            short_lots,
            applied_short_lot_ids: state.tax_lots.applied_short_lot_ids.into_iter().collect(),
            applied_cover_ids: state.tax_lots.applied_cover_ids.into_iter().collect(),
        })?;
        let mut execution_ids = BTreeSet::new();
        for execution_id in state.execution_ids {
            validate_canonical_id("persisted live execution_id", &execution_id)?;
            if !execution_ids.insert(execution_id) {
                return Err(LiveError(
                    "persisted live execution ID is duplicated".to_owned(),
                ));
            }
        }
        let mut marks = BTreeMap::new();
        for (instrument_id, mark) in state.marks {
            validate_canonical_id("persisted live mark instrument_id", &instrument_id)?;
            let mark = decimal("persisted live mark price", &mark)?;
            if mark <= Decimal::ZERO {
                return Err(LiveError("persisted live mark price is invalid".to_owned()));
            }
            marks.insert(instrument_id, mark);
        }
        let mut strategy_attribution = BTreeMap::new();
        for (instrument_id, strategies) in state.strategy_attribution {
            validate_canonical_id("persisted live attribution instrument_id", &instrument_id)?;
            let mut parsed_strategies = BTreeMap::new();
            for (strategy_id, quantity) in strategies {
                validate_canonical_id("persisted live attribution strategy_id", &strategy_id)?;
                // Deliberately signed, not required positive: a strategy's
                // own tracked contribution can legitimately be negative (see
                // `apply_strategy_attribution_fill`).
                let quantity = decimal("persisted live attribution quantity", &quantity)?;
                parsed_strategies.insert(strategy_id, quantity);
            }
            strategy_attribution.insert(instrument_id, parsed_strategies);
        }
        let persisted_peak_equity = state
            .peak_equity
            .map(|value| decimal("persisted live peak equity", &value))
            .transpose()?;
        if persisted_peak_equity.is_some_and(|value| value <= Decimal::ZERO) {
            return Err(LiveError(
                "persisted live peak equity is invalid".to_owned(),
            ));
        }
        let persisted_daily_baseline_equity = state
            .daily_baseline_equity
            .map(|value| decimal("persisted live daily-loss baseline equity", &value))
            .transpose()?;
        match (&state.daily_baseline_date, &persisted_daily_baseline_equity) {
            (Some(date), Some(_)) => validate_exchange_date(date)?,
            (None, None) => {}
            _ => {
                return Err(LiveError(
                    "persisted live daily-loss baseline date and equity must be present together"
                        .to_owned(),
                ))
            }
        }
        let mut switches = LiveKillSwitchRegistry::new(self.kill_switches.version.clone())?;
        for scope in state.active_kill_switches {
            switches.activate(parse_kill_switch_scope(&scope)?)?;
        }
        let mut incidents = BTreeMap::new();
        for (incident_id, incident) in state.incidents {
            validate_canonical_id("persisted live incident_id", &incident_id)?;
            if incident.category.is_empty()
                || incident.subject.is_empty()
                || incident.detail.is_empty()
            {
                return Err(LiveError("persisted live incident is invalid".to_owned()));
            }
            incidents.insert(
                incident_id.clone(),
                LiveIncident {
                    issue: LiveReconciliationIssue {
                        incident_id,
                        category: incident.category,
                        subject: incident.subject,
                        detail: incident.detail,
                    },
                    explanation: incident.explanation,
                },
            );
        }
        for (date, day) in &state.live_days {
            validate_exchange_date(date)?;
            validate_canonical_id("persisted live calendar_id", &day.calendar_id)?;
            if day.calendar_id != self.policy.trading_calendar_id || day.audit_head_hash.len() != 64
            {
                return Err(LiveError(
                    "persisted live-day evidence is invalid".to_owned(),
                ));
            }
            TradingSession {
                exchange_date: date.clone(),
                opens_at: day.session_opens_at.clone(),
                closes_at: day.session_closes_at.clone(),
            }
            .validate()?;
        }
        if state.canary_submissions > self.policy.canary_max_orders
            || state.next_reconciliation == 0
        {
            return Err(LiveError("persisted live counters are invalid".to_owned()));
        }
        match (&state.last_reconciled_at, state.last_reconciliation_clean) {
            (Some(timestamp), Some(_)) => {
                validate_utc_timestamp("persisted live reconciliation time", timestamp)?
            }
            (None, None) => {}
            _ => {
                return Err(LiveError(
                    "persisted live reconciliation marker is invalid".to_owned(),
                ))
            }
        }
        let latest_reconciliation = state
            .latest_reconciliation
            .map(LiveReconciliationReport::try_from)
            .transpose()?;
        match (
            &latest_reconciliation,
            &state.last_reconciled_at,
            state.last_reconciliation_clean,
        ) {
            (Some(report), Some(reconciled_at), Some(clean))
                if report.reconciled_at == *reconciled_at && report.is_clean() == clean => {}
            (None, None, None) => {}
            _ => {
                return Err(LiveError(
                    "persisted latest reconciliation evidence is inconsistent".to_owned(),
                ))
            }
        }
        if let Some(report) = &latest_reconciliation {
            let expected = format!(
                "reconciliation-{:08}",
                state.next_reconciliation.saturating_sub(1)
            );
            if report.reconciliation_id != expected {
                return Err(LiveError(
                    "persisted latest reconciliation identity is invalid".to_owned(),
                ));
            }
        }
        let mut combo_orders = BTreeMap::new();
        let mut combo_execution_ids = BTreeSet::new();
        for (order_id, persisted) in state.combo_orders {
            let intent = ComboIntent::try_from(persisted.intent)?;
            if intent.account_id != self.account.account_id || intent.environment != "LIVE" {
                return Err(LiveError(
                    "persisted live combination has an incompatible account or environment"
                        .to_owned(),
                ));
            }
            validate_canonical_id("persisted combo approval_id", &persisted.approval_id)?;
            let mut broker_order_versions = persisted.broker_order_versions;
            if let Some(broker_order_id) = &persisted.broker_order_id {
                validate_canonical_id("persisted combo broker_order_id", broker_order_id)?;
                if broker_order_versions.is_empty() {
                    broker_order_versions.push(broker_order_id.clone());
                }
            }
            let mut unique_versions = BTreeSet::new();
            for broker_order_id in &broker_order_versions {
                validate_canonical_id("persisted combo broker_order_version", broker_order_id)?;
                if !unique_versions.insert(broker_order_id) {
                    return Err(LiveError(
                        "persisted combination broker order versions are duplicated".to_owned(),
                    ));
                }
            }
            if let Some(broker_order_id) = &persisted.broker_order_id {
                if !unique_versions.contains(broker_order_id) {
                    return Err(LiveError(
                        "persisted active combination broker ID is absent from versions".to_owned(),
                    ));
                }
            }
            let mut marks = Vec::with_capacity(persisted.market.len());
            for mark in persisted.market {
                marks.push(LiveMarketData::try_from(mark)?);
            }
            let market = LiveComboMarketData { marks };
            // The observation must still price exactly the legs it was stored
            // against. A journal that lost a leg's mark, or gained one, cannot
            // reproduce the decision that approved the combination.
            market.validate_for(&intent)?;
            let filled_quantity =
                decimal("persisted combo filled units", &persisted.filled_quantity)?;
            if filled_quantity < Decimal::ZERO || filled_quantity > intent.combo_quantity {
                return Err(LiveError(
                    "persisted combination filled units are invalid".to_owned(),
                ));
            }
            let decision = LiveRiskDecision::try_from(persisted.decision)?;
            if decision.decision_id != format!("live-combo-risk-{}", intent.intent_id)
                || decision.policy_version != self.policy.version
                || !decision.approved
            {
                return Err(LiveError(
                    "persisted combination decision does not authorize this order".to_owned(),
                ));
            }
            let state = parse_order_state(&persisted.state)?;
            let mut executions = BTreeMap::new();
            let mut receipt_units = Decimal::ZERO;
            for persisted_execution in persisted.executions {
                let execution = LiveBrokerComboExecution {
                    execution_id: persisted_execution.execution_id,
                    client_order_id: persisted_execution.client_order_id,
                    broker_order_id: persisted_execution.broker_order_id,
                    units: decimal(
                        "persisted live combo execution units",
                        &persisted_execution.units,
                    )?,
                    legs: persisted_execution
                        .legs
                        .into_iter()
                        .map(|leg| {
                            Ok(LiveBrokerComboExecutionLeg {
                                execution_id: leg.execution_id,
                                instrument_id: leg.instrument_id,
                                side: match leg.side.as_str() {
                                    "BUY" => Side::Buy,
                                    "SELL" => Side::Sell,
                                    _ => {
                                        return Err(LiveError(
                                            "invalid persisted live combo side".to_owned(),
                                        ))
                                    }
                                },
                                quantity: decimal(
                                    "persisted live combo leg quantity",
                                    &leg.quantity,
                                )?,
                                price: decimal("persisted live combo leg price", &leg.price)?,
                                fee: decimal("persisted live combo leg fee", &leg.fee)?,
                                executed_at: leg.executed_at,
                            })
                        })
                        .collect::<Result<Vec<_>, LiveError>>()?,
                };
                execution.validate(&intent)?;
                if execution.client_order_id != order_id
                    || !broker_order_versions.contains(&execution.broker_order_id)
                {
                    return Err(LiveError(
                        "persisted live combination execution identity is inconsistent".to_owned(),
                    ));
                }
                for execution_id in std::iter::once(&execution.execution_id)
                    .chain(execution.legs.iter().map(|leg| &leg.execution_id))
                {
                    if !execution_ids.contains(execution_id)
                        || !combo_execution_ids.insert(execution_id.clone())
                    {
                        return Err(LiveError(
                            "persisted live combination execution is missing or duplicated in account receipts"
                                .to_owned(),
                        ));
                    }
                }
                receipt_units = receipt_units.checked_add(execution.units)?;
                if executions
                    .insert(execution.execution_id.clone(), execution)
                    .is_some()
                {
                    return Err(LiveError(
                        "persisted live combination execution identity is duplicated".to_owned(),
                    ));
                }
            }
            if receipt_units != filled_quantity
                || (state == OrderState::Filled && filled_quantity != intent.combo_quantity)
                || (state == OrderState::PartiallyFilled
                    && (filled_quantity == Decimal::ZERO
                        || filled_quantity == intent.combo_quantity))
                || (matches!(
                    state,
                    OrderState::Cancelled | OrderState::Rejected | OrderState::Expired
                ) && filled_quantity == intent.combo_quantity)
            {
                return Err(LiveError(
                    "persisted live combination lifecycle disagrees with execution receipts"
                        .to_owned(),
                ));
            }
            let oms = OmsComboOrder::recover(order_id.clone(), intent, state)?;
            combo_orders.insert(
                order_id,
                LiveComboOrder {
                    oms,
                    approval_id: persisted.approval_id,
                    market,
                    decision,
                    broker_order_id: persisted.broker_order_id,
                    broker_order_versions,
                    filled_quantity,
                    executions,
                },
            );
        }
        let corporate_actions = restore_live_corporate_actions(state.corporate_actions)?;
        self.orders = orders;
        self.combo_orders = combo_orders;
        self.approvals = approvals;
        self.corporate_actions = corporate_actions;
        self.portfolios = portfolios;
        self.tax_lots = tax_lots;
        self.marks = marks;
        self.strategy_attribution = strategy_attribution;
        self.execution_ids = execution_ids;
        self.kill_switches = switches;
        self.incidents = incidents;
        self.live_days = state.live_days;
        self.canary_submissions = state.canary_submissions;
        self.next_reconciliation = state.next_reconciliation;
        self.last_reconciled_at = state.last_reconciled_at;
        self.last_reconciliation_clean = state.last_reconciliation_clean;
        self.latest_reconciliation = latest_reconciliation;
        // `cash`/`portfolios`/`marks` are already restored above, so
        // `current_equity()` reflects real recovered state here. A journal
        // that never tracked peak equity (`persisted_peak_equity` absent)
        // bootstraps its peak to that real equity -- the honest value for a
        // peak that starts being tracked only from this reopen onward.
        let recovered_equity = self.current_equity()?;
        self.peak_equity = persisted_peak_equity
            .unwrap_or(recovered_equity)
            .max(recovered_equity);
        // Unlike peak equity, a daily-loss baseline is never maxed against
        // recovered equity: it is either the exact value durably persisted
        // from earlier the same UTC day, or (absent -- a legacy journal, or
        // one that has never evaluated risk) left unset so the very next
        // `evaluate_risk` call establishes an honest fresh baseline from real
        // recovered state.
        self.daily_baseline_date = state.daily_baseline_date;
        self.daily_baseline_equity = persisted_daily_baseline_equity.unwrap_or(Decimal::ZERO);
        // Restart recovery never assumes a still-valid external session.
        self.broker_connected = false;
        Ok(())
    }
}

/// Read-only recovery/backup state emitted for monitoring and runbooks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisasterRecoveryStatus {
    /// Local append-only journal path.
    pub journal_path: String,
    /// Last durable audit sequence.
    pub audit_sequence: u64,
    /// SHA-256 audit-chain head.
    pub audit_head_hash: String,
    /// Whether the running service has had any audit-write failure.
    pub audit_healthy: bool,
    /// Whether an adapter session must be re-established before canary work.
    pub broker_session_requires_reconnect: bool,
}

impl RegisteredLiveApproval {
    fn approval_equivalent(&self, approval: &LiveApproval) -> bool {
        self.approval.approval_id == approval.approval_id
            && self.approval.intent_id == approval.intent_id
            && self.approval.intent_fingerprint == approval.intent_fingerprint
            && self.approval.configuration_fingerprint == approval.configuration_fingerprint
            && self.approval.requested_by == approval.requested_by
            && self.approval.approved_by == approval.approved_by
            && self.approval.approved_at == approval.approved_at
            && self.approval.expires_at == approval.expires_at
    }
}

/// A portfolio-risk composition's fingerprint parts, every field included:
/// the policy's own, then its instrument buckets and margin rates. LIVE keeps
/// its own copy of this rendering, as it keeps its own risk gate.
fn portfolio_risk_fingerprint_parts(composition: &PortfolioRiskComposition) -> Vec<String> {
    let PortfolioRiskComposition {
        policy,
        instrument_buckets,
        margin_rates,
    } = composition;
    let mut parts = policy.canonical_parts();
    parts.push(format!(
        "instrument_buckets={}",
        instrument_buckets
            .iter()
            .map(|(instrument_id, bucket)| format!(
                "{instrument_id}:{}:{}:{}",
                bucket.asset_class, bucket.currency, bucket.sector
            ))
            .collect::<Vec<_>>()
            .join("|")
    ));
    parts.push(format!(
        "margin_rates={}",
        margin_rates.as_ref().map_or_else(
            || "none".to_owned(),
            |rates| rates
                .iter()
                .map(|(asset_class, rate)| format!(
                    "{asset_class}:{}:{}",
                    rate.initial_bps, rate.maintenance_bps
                ))
                .collect::<Vec<_>>()
                .join("|")
        )
    ));
    parts
}

fn configuration_fingerprint(
    account: &LiveAccount,
    policy: &LiveRiskPolicy,
    kill_switches: &LiveKillSwitchRegistry,
) -> String {
    let initial_cash = account.initial_cash.to_string();
    let max_deployed_capital = account.max_deployed_capital.to_string();
    let max_order_quantity = policy.max_order_quantity.to_string();
    let max_order_notional = policy.max_order_notional.to_string();
    let max_price_deviation_bps = policy.max_price_deviation_bps.to_string();
    let canary_max_order_notional = policy.canary_max_order_notional.to_string();
    let canary_max_orders = policy.canary_max_orders.to_string();
    let max_open_orders = policy.max_open_orders.to_string();
    let max_position_quantity = policy.max_position_quantity.to_string();
    let max_realized_loss = policy.max_realized_loss.to_string();
    let max_market_data_age_seconds = policy.max_market_data_age_seconds.to_string();
    let max_order_rate = policy.max_order_rate.to_string();
    let order_rate_window_seconds = policy.order_rate_window_seconds.to_string();
    // Every portfolio-risk field, through the policy's exhaustive
    // `canonical_parts`, plus this composition's reference data and margin
    // rates. Version 1 of this part listed the fields by hand and left out
    // five limits, so a journal reopened, and an approval stayed valid, under
    // changed limits (delivery state E7.4). Absent for every configuration
    // without aggregate-risk composition, whose fingerprint is unchanged.
    let portfolio_risk_parts: Vec<String> = policy
        .portfolio_risk
        .as_ref()
        .map(portfolio_risk_fingerprint_parts)
        .unwrap_or_default();
    let mut parts = vec![
        "live-configuration-v3",
        &account.account_id,
        &account.currency,
        &initial_cash,
        &max_deployed_capital,
        &account.environment,
        account.credential_reference.as_str(),
        &policy.version,
        &policy.trading_calendar_id,
        &max_order_quantity,
        &max_order_notional,
        &max_price_deviation_bps,
        &canary_max_order_notional,
        &canary_max_orders,
        &max_open_orders,
        &max_position_quantity,
        &max_realized_loss,
        &max_market_data_age_seconds,
        &max_order_rate,
        &order_rate_window_seconds,
        &kill_switches.version,
    ];
    if !portfolio_risk_parts.is_empty() {
        parts.push("live-portfolio-risk-v2");
        for part in &portfolio_risk_parts {
            parts.push(part);
        }
    }
    // Always present: every configuration now lists its tick sizes, and an
    // approval or journal bound to one tick table must not carry to another.
    let tick_sizes = render_instrument_table(&policy.instrument_tick_sizes);
    parts.push("live-instrument-ticks-v1");
    parts.push(&tick_sizes);
    // Likewise the lot table (E3.6c).
    let lot_sizes = render_instrument_table(&policy.instrument_lot_sizes);
    parts.push("live-instrument-lots-v1");
    parts.push(&lot_sizes);
    hash_fingerprint_parts(&parts)
}

/// A per-instrument reference table (ticks or lots) in its canonical,
/// instrument-ordered form, for the configuration fingerprint.
fn render_instrument_table(table: &BTreeMap<String, Decimal>) -> String {
    table
        .iter()
        .map(|(instrument_id, value)| format!("{instrument_id}:{value}"))
        .collect::<Vec<_>>()
        .join("|")
}

/// The first instrument listed in exactly one of two per-instrument tables,
/// checking the tick table's instruments before the lot table's.
fn unpaired_instrument<'a>(
    tick_sizes: &'a BTreeMap<String, Decimal>,
    lot_sizes: &'a BTreeMap<String, Decimal>,
) -> Option<&'a str> {
    tick_sizes
        .keys()
        .find(|instrument_id| !lot_sizes.contains_key(*instrument_id))
        .or_else(|| {
            lot_sizes
                .keys()
                .find(|instrument_id| !tick_sizes.contains_key(*instrument_id))
        })
        .map(String::as_str)
}

fn intent_fingerprint(intent: &OrderIntent) -> Result<String, LiveError> {
    intent.validate()?;
    let quantity = intent.quantity.to_string();
    let limit_price = intent
        .limit_price
        .map(|price| price.to_string())
        .unwrap_or_default();
    Ok(hash_fingerprint_parts(&[
        "live-intent-v1",
        &intent.intent_id,
        &intent.account_id,
        &intent.strategy_id,
        &intent.instrument_id,
        &intent.correlation_id,
        intent.side.as_str(),
        &quantity,
        intent.order_type.as_str(),
        &limit_price,
        intent.time_in_force.as_str(),
        &intent.rationale,
        &intent.created_at,
        &intent.strategy_version,
        &intent.configuration_version,
        &intent.environment,
    ]))
}

fn market_fingerprint(market: &LiveMarketData) -> String {
    let mark_price = market.mark_price.to_string();
    hash_fingerprint_parts(&[
        "live-market-v1",
        &market.instrument_id,
        &mark_price,
        &market.observed_at,
    ])
}

/// Hashes ordered fields with explicit byte lengths, avoiding delimiter ambiguity.
fn hash_fingerprint_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// Renders a bucket-exposure map deterministically (`BTreeMap` iteration is
/// already sorted) for the `evaluated_limits` evidence string and the
/// configuration fingerprint.
fn render_bucket_map(buckets: &BTreeMap<String, Decimal>) -> String {
    buckets
        .iter()
        .map(|(bucket, amount)| format!("{bucket}:{amount}"))
        .collect::<Vec<_>>()
        .join("|")
}

fn transition_to_acknowledged(order: &mut LiveOrder, reason: &str) -> Result<(), LiveError> {
    match order.oms.state {
        OrderState::PendingSubmit => {
            order.oms.transition(OrderState::Submitted, reason)?;
            order.oms.transition(OrderState::Acknowledged, reason)?;
        }
        OrderState::Submitted | OrderState::Unknown => {
            order.oms.transition(OrderState::Acknowledged, reason)?;
        }
        OrderState::Acknowledged
        | OrderState::PartiallyFilled
        | OrderState::Filled
        | OrderState::PendingCancel
        | OrderState::PendingReplace => {}
        _ => {
            return Err(LiveError(
                "broker acknowledgement is incompatible with live OMS state".to_owned(),
            ))
        }
    }
    Ok(())
}

fn restore_working_state(order: &mut LiveOrder, reason: &str) -> Result<(), LiveError> {
    let state = if order.filled_quantity == Decimal::ZERO {
        OrderState::Acknowledged
    } else {
        OrderState::PartiallyFilled
    };
    order.oms.transition(state, reason)?;
    Ok(())
}

fn restore_replacement_state(order: &mut LiveOrder, reason: &str) -> Result<(), LiveError> {
    let state = order.replace_return_state.take().unwrap_or_else(|| {
        if order.filled_quantity == Decimal::ZERO {
            OrderState::Acknowledged
        } else {
            OrderState::PartiallyFilled
        }
    });
    order.oms.transition(state, reason)?;
    Ok(())
}

fn transition_to_terminal(
    order: &mut LiveOrder,
    terminal: OrderState,
    reason: &str,
) -> Result<(), LiveError> {
    debug_assert!(matches!(
        terminal,
        OrderState::Cancelled | OrderState::Rejected | OrderState::Expired
    ));
    match order.oms.state {
        OrderState::PendingSubmit => {
            order.oms.transition(OrderState::Submitted, reason)?;
            order.oms.transition(terminal, reason)?;
        }
        OrderState::Submitted
        | OrderState::Acknowledged
        | OrderState::PartiallyFilled
        | OrderState::PendingCancel
        | OrderState::PendingReplace
        | OrderState::Unknown => {
            order.oms.transition(terminal, reason)?;
        }
        state if state == terminal => {}
        OrderState::Filled | OrderState::Cancelled | OrderState::Rejected | OrderState::Expired => {
        }
        _ => {
            return Err(LiveError(
                "live broker terminal event is incompatible with OMS state".to_owned(),
            ))
        }
    }
    if matches!(
        order.oms.state,
        OrderState::Cancelled | OrderState::Rejected | OrderState::Expired
    ) && order.filled_quantity >= order.oms.intent.quantity
    {
        return Err(LiveError(
            "non-filled live terminal state cannot have cumulative quantity equal to requested quantity"
                .to_owned(),
        ));
    }
    Ok(())
}

fn validate_reason(name: &str, value: &str) -> Result<(), LiveError> {
    if value.trim().is_empty() || value.len() > 1_024 {
        return Err(LiveError(format!(
            "{name} must contain 1 to 1024 characters"
        )));
    }
    Ok(())
}

fn validate_broker_snapshot(snapshot: &LiveBrokerAccountSnapshot) -> Result<(), LiveError> {
    let mut order_versions = BTreeSet::new();
    let mut broker_order_ids = BTreeSet::new();
    for order in &snapshot.orders {
        validate_canonical_id("live snapshot client_order_id", &order.client_order_id)?;
        validate_canonical_id("live snapshot broker_order_id", &order.broker_order_id)?;
        if order.filled_quantity < Decimal::ZERO
            || !order_versions.insert((
                order.client_order_id.as_str(),
                order.broker_order_id.as_str(),
            ))
            || !broker_order_ids.insert(order.broker_order_id.as_str())
        {
            return Err(LiveError(
                "live snapshot has duplicate broker order versions or negative fills".to_owned(),
            ));
        }
    }
    let mut instrument_ids = BTreeSet::new();
    for position in &snapshot.positions {
        validate_canonical_id("live snapshot instrument_id", &position.instrument_id)?;
        if !instrument_ids.insert(position.instrument_id.as_str()) {
            return Err(LiveError(
                "live snapshot has duplicate instrument position".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_exchange_date(value: &str) -> Result<(), LiveError> {
    if value.len() != 10
        || !value.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7) && byte == b'-'
                || !matches!(index, 4 | 7) && byte.is_ascii_digit()
        })
    {
        return Err(LiveError("exchange date must be YYYY-MM-DD".to_owned()));
    }
    validate_utc_timestamp("live exchange date", &format!("{value}T00:00:00Z"))?;
    Ok(())
}

fn decimal(name: &str, value: &str) -> Result<Decimal, LiveError> {
    Decimal::from_str(value).map_err(|error| LiveError(format!("invalid {name}: {error}")))
}

fn parse_order_state(value: &str) -> Result<OrderState, LiveError> {
    match value {
        "CREATED" => Ok(OrderState::Created),
        "PENDING_RISK" => Ok(OrderState::PendingRisk),
        "RISK_REJECTED" => Ok(OrderState::RiskRejected),
        "APPROVED" => Ok(OrderState::Approved),
        "PENDING_SUBMIT" => Ok(OrderState::PendingSubmit),
        "SUBMITTED" => Ok(OrderState::Submitted),
        "ACKNOWLEDGED" => Ok(OrderState::Acknowledged),
        "PARTIALLY_FILLED" => Ok(OrderState::PartiallyFilled),
        "FILLED" => Ok(OrderState::Filled),
        "PENDING_CANCEL" => Ok(OrderState::PendingCancel),
        "PENDING_REPLACE" => Ok(OrderState::PendingReplace),
        "CANCELLED" => Ok(OrderState::Cancelled),
        "REJECTED" => Ok(OrderState::Rejected),
        "EXPIRED" => Ok(OrderState::Expired),
        "UNKNOWN" => Ok(OrderState::Unknown),
        _ => Err(LiveError("persisted live OMS state is invalid".to_owned())),
    }
}

fn parse_kill_switch_scope(value: &str) -> Result<LiveKillSwitchScope, LiveError> {
    LiveKillSwitchScope::from_key(value)
        .map_err(|_| LiveError("persisted live kill-switch scope is invalid".to_owned()))
}

impl From<&OrderIntent> for PersistentIntent {
    fn from(intent: &OrderIntent) -> Self {
        Self {
            intent_id: intent.intent_id.clone(),
            account_id: intent.account_id.clone(),
            strategy_id: intent.strategy_id.clone(),
            instrument_id: intent.instrument_id.clone(),
            correlation_id: intent.correlation_id.clone(),
            side: intent.side.as_str().to_owned(),
            quantity: intent.quantity.to_string(),
            order_type: intent.order_type.as_str().to_owned(),
            limit_price: intent.limit_price.map(|value| value.to_string()),
            time_in_force: intent.time_in_force.as_str().to_owned(),
            rationale: intent.rationale.clone(),
            created_at: intent.created_at.clone(),
            strategy_version: intent.strategy_version.clone(),
            configuration_version: intent.configuration_version.clone(),
            environment: intent.environment.clone(),
        }
    }
}

impl TryFrom<PersistentIntent> for OrderIntent {
    type Error = LiveError;

    fn try_from(value: PersistentIntent) -> Result<Self, Self::Error> {
        let intent = Self {
            intent_id: value.intent_id,
            account_id: value.account_id,
            strategy_id: value.strategy_id,
            instrument_id: value.instrument_id,
            correlation_id: value.correlation_id,
            side: match value.side.as_str() {
                "BUY" => Side::Buy,
                "SELL" => Side::Sell,
                _ => {
                    return Err(LiveError(
                        "persisted live intent side is invalid".to_owned(),
                    ))
                }
            },
            quantity: decimal("persisted live intent quantity", &value.quantity)?,
            order_type: match value.order_type.as_str() {
                "MARKET" => follon_domain::OrderType::Market,
                "LIMIT" => follon_domain::OrderType::Limit,
                _ => return Err(LiveError("persisted live order type is invalid".to_owned())),
            },
            limit_price: value
                .limit_price
                .as_deref()
                .map(|price| decimal("persisted live limit price", price))
                .transpose()?,
            time_in_force: match value.time_in_force.as_str() {
                "DAY" => TimeInForce::Day,
                "GTC" => TimeInForce::GoodTilCancelled,
                _ => {
                    return Err(LiveError(
                        "persisted live time in force is invalid".to_owned(),
                    ))
                }
            },
            rationale: value.rationale,
            created_at: value.created_at,
            strategy_version: value.strategy_version,
            configuration_version: value.configuration_version,
            environment: value.environment,
        };
        intent.validate()?;
        Ok(intent)
    }
}

impl From<&LiveMarketData> for PersistentMarketData {
    fn from(market: &LiveMarketData) -> Self {
        Self {
            instrument_id: market.instrument_id.clone(),
            mark_price: market.mark_price.to_string(),
            observed_at: market.observed_at.clone(),
        }
    }
}

impl TryFrom<PersistentMarketData> for LiveMarketData {
    type Error = LiveError;

    fn try_from(value: PersistentMarketData) -> Result<Self, Self::Error> {
        let market = Self {
            instrument_id: value.instrument_id,
            mark_price: decimal("persisted live market mark", &value.mark_price)?,
            observed_at: value.observed_at,
        };
        market.validate()?;
        Ok(market)
    }
}

impl From<&ComboIntent> for PersistentComboIntent {
    fn from(intent: &ComboIntent) -> Self {
        Self {
            intent_id: intent.intent_id.clone(),
            account_id: intent.account_id.clone(),
            strategy_id: intent.strategy_id.clone(),
            correlation_id: intent.correlation_id.clone(),
            legs: intent
                .legs
                .iter()
                .map(|leg| PersistentComboLeg {
                    instrument_id: leg.instrument_id.clone(),
                    side: leg.side.as_str().to_owned(),
                    ratio: leg.ratio,
                    limit_price: leg.limit_price.to_string(),
                })
                .collect(),
            combo_quantity: intent.combo_quantity.to_string(),
            price_limit_kind: intent.price_limit.kind().to_owned(),
            price_limit_amount: intent.price_limit.amount().to_string(),
            time_in_force: intent.time_in_force.as_str().to_owned(),
            rationale: intent.rationale.clone(),
            created_at: intent.created_at.clone(),
            strategy_version: intent.strategy_version.clone(),
            configuration_version: intent.configuration_version.clone(),
            environment: intent.environment.clone(),
        }
    }
}

impl TryFrom<PersistentComboIntent> for ComboIntent {
    type Error = LiveError;

    fn try_from(intent: PersistentComboIntent) -> Result<Self, Self::Error> {
        let amount = decimal("persisted combo price limit", &intent.price_limit_amount)?;
        let mut legs = Vec::with_capacity(intent.legs.len());
        for leg in intent.legs {
            legs.push(follon_domain::ComboIntentLeg {
                instrument_id: leg.instrument_id,
                side: match leg.side.as_str() {
                    "BUY" => Side::Buy,
                    "SELL" => Side::Sell,
                    _ => return Err(LiveError("persisted combo leg side is invalid".to_owned())),
                },
                ratio: leg.ratio,
                limit_price: decimal("persisted combo leg limit price", &leg.limit_price)?,
            });
        }
        let result = Self {
            intent_id: intent.intent_id,
            account_id: intent.account_id,
            strategy_id: intent.strategy_id,
            correlation_id: intent.correlation_id,
            legs,
            combo_quantity: decimal("persisted combo quantity", &intent.combo_quantity)?,
            price_limit: match intent.price_limit_kind.as_str() {
                "MAXIMUM_DEBIT" => follon_domain::ComboPriceLimit::MaximumDebit(amount),
                "MINIMUM_CREDIT" => follon_domain::ComboPriceLimit::MinimumCredit(amount),
                _ => {
                    return Err(LiveError(
                        "persisted combo price limit kind is invalid".to_owned(),
                    ))
                }
            },
            time_in_force: match intent.time_in_force.as_str() {
                "DAY" => TimeInForce::Day,
                "GTC" => TimeInForce::GoodTilCancelled,
                _ => {
                    return Err(LiveError(
                        "persisted combo time in force is invalid".to_owned(),
                    ))
                }
            },
            rationale: intent.rationale,
            created_at: intent.created_at,
            strategy_version: intent.strategy_version,
            configuration_version: intent.configuration_version,
            environment: intent.environment,
        };
        // Re-validated on the way back in, not trusted because it was once
        // written: a journal is a file on disk and may have been edited.
        result.validate()?;
        Ok(result)
    }
}

impl From<&LiveRiskDecision> for PersistentRiskDecision {
    fn from(decision: &LiveRiskDecision) -> Self {
        Self {
            decision_id: decision.decision_id.clone(),
            approved: decision.approved,
            reason_codes: decision.reason_codes.clone(),
            policy_version: decision.policy_version.clone(),
            decided_at: decision.decided_at.clone(),
            market_fingerprint: decision.market_fingerprint.clone(),
            evaluated_limits: decision.evaluated_limits.clone(),
        }
    }
}

impl TryFrom<PersistentRiskDecision> for LiveRiskDecision {
    type Error = LiveError;

    fn try_from(value: PersistentRiskDecision) -> Result<Self, Self::Error> {
        validate_canonical_id("persisted live risk decision_id", &value.decision_id)?;
        validate_utc_timestamp("persisted live risk decision time", &value.decided_at)?;
        if value.reason_codes.is_empty()
            || value.policy_version.is_empty()
            || value.evaluated_limits.is_empty()
            || value.market_fingerprint.len() != 64
            || !value
                .market_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(LiveError(
                "persisted live risk decision is invalid".to_owned(),
            ));
        }
        for reason in &value.reason_codes {
            validate_reason("persisted live risk reason", reason)?;
        }
        if value.approved != (value.reason_codes.len() == 1 && value.reason_codes[0] == "APPROVED")
        {
            return Err(LiveError(
                "persisted live risk approval outcome is inconsistent".to_owned(),
            ));
        }
        Ok(Self {
            decision_id: value.decision_id,
            approved: value.approved,
            reason_codes: value.reason_codes,
            policy_version: value.policy_version,
            decided_at: value.decided_at,
            market_fingerprint: value.market_fingerprint,
            evaluated_limits: value.evaluated_limits,
        })
    }
}

impl From<&LiveReconciliationReport> for PersistentReconciliationReport {
    fn from(report: &LiveReconciliationReport) -> Self {
        Self {
            reconciliation_id: report.reconciliation_id.clone(),
            reconciled_at: report.reconciled_at.clone(),
            issues: report
                .issues
                .iter()
                .map(|issue| PersistentReconciliationIssue {
                    incident_id: issue.incident_id.clone(),
                    category: issue.category.clone(),
                    subject: issue.subject.clone(),
                    detail: issue.detail.clone(),
                })
                .collect(),
        }
    }
}

impl TryFrom<PersistentReconciliationReport> for LiveReconciliationReport {
    type Error = LiveError;

    fn try_from(value: PersistentReconciliationReport) -> Result<Self, Self::Error> {
        validate_canonical_id("persisted live reconciliation_id", &value.reconciliation_id)?;
        validate_utc_timestamp("persisted live reconciliation time", &value.reconciled_at)?;
        let mut incident_ids = BTreeSet::new();
        let mut issues = Vec::with_capacity(value.issues.len());
        for issue in value.issues {
            validate_canonical_id(
                "persisted live reconciliation incident_id",
                &issue.incident_id,
            )?;
            validate_reason("persisted live reconciliation category", &issue.category)?;
            validate_reason("persisted live reconciliation subject", &issue.subject)?;
            validate_reason("persisted live reconciliation detail", &issue.detail)?;
            if !incident_ids.insert(issue.incident_id.clone()) {
                return Err(LiveError(
                    "persisted live reconciliation repeats an incident ID".to_owned(),
                ));
            }
            issues.push(LiveReconciliationIssue {
                incident_id: issue.incident_id,
                category: issue.category,
                subject: issue.subject,
                detail: issue.detail,
            });
        }
        Ok(Self {
            reconciliation_id: value.reconciliation_id,
            reconciled_at: value.reconciled_at,
            issues,
        })
    }
}

impl From<&LiveApproval> for PersistentApproval {
    fn from(approval: &LiveApproval) -> Self {
        Self {
            approval_id: approval.approval_id.clone(),
            intent_id: approval.intent_id.clone(),
            intent_fingerprint: approval.intent_fingerprint.clone(),
            configuration_fingerprint: approval.configuration_fingerprint.clone(),
            requested_by: approval.requested_by.clone(),
            approved_by: approval.approved_by.clone(),
            approved_at: approval.approved_at.clone(),
            expires_at: approval.expires_at.clone(),
        }
    }
}

impl TryFrom<PersistentApproval> for LiveApproval {
    type Error = LiveError;

    fn try_from(value: PersistentApproval) -> Result<Self, Self::Error> {
        Ok(Self {
            approval_id: value.approval_id,
            intent_id: value.intent_id,
            intent_fingerprint: value.intent_fingerprint,
            configuration_fingerprint: value.configuration_fingerprint,
            requested_by: value.requested_by,
            approved_by: value.approved_by,
            approved_at: value.approved_at,
            expires_at: value.expires_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::mem;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use follon_domain::OrderType;
    use follon_instrument::StaticTradingCalendar;

    use super::*;
    include!("combo_lifecycle_tests.rs");
    include!("corporate_action_tests.rs");

    static JOURNAL_SEQUENCE: AtomicUsize = AtomicUsize::new(1);

    #[derive(Debug)]
    struct TestBroker {
        connected: bool,
        submitted: u32,
        cancelled: u32,
        replaced: u32,
        fail_cancel: bool,
        /// Models a transport failure on a combination the adapter declared.
        reject_combos: bool,
        /// What this adapter declares it can carry.
        capabilities: LiveBrokerCapabilities,
        events: Vec<LiveBrokerEvent>,
        snapshot: LiveBrokerAccountSnapshot,
    }

    impl TestBroker {
        fn new() -> Self {
            Self {
                connected: false,
                submitted: 0,
                cancelled: 0,
                replaced: 0,
                fail_cancel: false,
                reject_combos: false,
                capabilities: LiveBrokerCapabilities {
                    combinations: true,
                    ..LiveBrokerCapabilities::default()
                },
                events: Vec::new(),
                snapshot: LiveBrokerAccountSnapshot {
                    orders: Vec::new(),
                    positions: Vec::new(),
                    cash: amount("1000"),
                },
            }
        }

        fn queue_combo_fill(
            &mut self,
            execution: LiveBrokerComboExecution,
        ) -> Result<(), LiveError> {
            let broker_order = self
                .snapshot
                .orders
                .iter_mut()
                .find(|order| {
                    order.client_order_id == execution.client_order_id
                        && order.broker_order_id == execution.broker_order_id
                })
                .ok_or_else(|| LiveError("test broker does not know combination".to_owned()))?;
            broker_order.filled_quantity =
                broker_order.filled_quantity.checked_add(execution.units)?;
            broker_order.state = OrderState::PartiallyFilled;
            for leg in &execution.legs {
                let signed = match leg.side {
                    Side::Buy => leg.quantity,
                    Side::Sell => Decimal::ZERO.checked_sub(leg.quantity)?,
                };
                if let Some(position) = self
                    .snapshot
                    .positions
                    .iter_mut()
                    .find(|position| position.instrument_id == leg.instrument_id)
                {
                    position.quantity = position.quantity.checked_add(signed)?;
                } else {
                    self.snapshot.positions.push(LiveBrokerPositionSnapshot {
                        instrument_id: leg.instrument_id.clone(),
                        quantity: signed,
                    });
                }
                let gross = leg.price.checked_mul(leg.quantity)?;
                self.snapshot.cash = match leg.side {
                    Side::Buy => self
                        .snapshot
                        .cash
                        .checked_sub(gross.checked_add(leg.fee)?)?,
                    Side::Sell => self
                        .snapshot
                        .cash
                        .checked_add(gross.checked_sub(leg.fee)?)?,
                };
            }
            self.events.push(LiveBrokerEvent::ComboExecution(execution));
            Ok(())
        }
    }

    impl LiveBrokerAdapter for TestBroker {
        fn capabilities(&self) -> LiveBrokerCapabilities {
            self.capabilities
        }

        fn connect(
            &mut self,
            account_id: &str,
            credential: &SecretMaterial,
        ) -> Result<(), LiveError> {
            assert_eq!(account_id, "acct.live.001");
            assert_eq!(credential.expose_to(|bytes| bytes.len()), 4);
            self.connected = true;
            Ok(())
        }

        fn submit(
            &mut self,
            request: &LiveBrokerOrderRequest,
        ) -> Result<LiveBrokerSubmitResult, LiveError> {
            assert!(self.connected);
            self.submitted += 1;
            let broker_order_id = format!("broker-{}", request.client_order_id);
            self.snapshot.orders.push(LiveBrokerOrderSnapshot {
                client_order_id: request.client_order_id.clone(),
                broker_order_id: broker_order_id.clone(),
                state: OrderState::Acknowledged,
                filled_quantity: Decimal::ZERO,
            });
            Ok(LiveBrokerSubmitResult::Acknowledged { broker_order_id })
        }

        fn submit_combo(
            &mut self,
            request: &LiveBrokerComboRequest,
        ) -> Result<LiveBrokerSubmitResult, LiveError> {
            assert!(self.connected);
            if self.reject_combos {
                return Err(LiveError(
                    "live broker adapter does not support native combinations".to_owned(),
                ));
            }
            self.submitted += 1;
            let broker_order_id = format!("broker-{}", request.client_order_id);
            self.snapshot.orders.push(LiveBrokerOrderSnapshot {
                client_order_id: request.client_order_id.clone(),
                broker_order_id: broker_order_id.clone(),
                state: OrderState::Acknowledged,
                filled_quantity: Decimal::ZERO,
            });
            Ok(LiveBrokerSubmitResult::Acknowledged { broker_order_id })
        }

        fn cancel(&mut self, _client_order_id: &str) -> Result<(), LiveError> {
            self.cancelled += 1;
            if self.fail_cancel {
                return Err(LiveError("test live cancel transport failed".to_owned()));
            }
            Ok(())
        }

        fn replace(&mut self, request: &LiveBrokerReplaceRequest) -> Result<(), LiveError> {
            request.validate()?;
            self.replaced += 1;
            Ok(())
        }

        fn poll(&mut self) -> Result<Vec<LiveBrokerEvent>, LiveError> {
            Ok(mem::take(&mut self.events))
        }

        fn snapshot(&mut self, account_id: &str) -> Result<LiveBrokerAccountSnapshot, LiveError> {
            assert_eq!(account_id, "acct.live.001");
            Ok(self.snapshot.clone())
        }

        fn reconnect(
            &mut self,
            account_id: &str,
            credential: &SecretMaterial,
        ) -> Result<(), LiveError> {
            self.connect(account_id, credential)
        }
    }

    struct TestSecrets;

    impl SecretProvider for TestSecrets {
        fn resolve(
            &self,
            reference: &SecretReference,
        ) -> Result<SecretMaterial, follon_secrets::SecretError> {
            assert_eq!(reference.as_str(), "secret.broker.test.acct-live-001");
            SecretMaterial::new(b"test".to_vec())
        }
    }

    fn amount(value: &str) -> Decimal {
        Decimal::from_str(value).expect("test decimal")
    }

    fn journal_path(label: &str) -> PathBuf {
        let sequence = JOURNAL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "follon-live-{label}-{}-{sequence}.ndjson",
            std::process::id()
        ))
    }

    fn account() -> LiveAccount {
        LiveAccount {
            account_id: "acct.live.001".to_owned(),
            currency: "USD".to_owned(),
            initial_cash: amount("1000"),
            max_deployed_capital: amount("100"),
            environment: "LIVE".to_owned(),
            credential_reference: SecretReference::new("secret.broker.test.acct-live-001")
                .expect("test secret reference"),
        }
    }

    fn policy() -> LiveRiskPolicy {
        LiveRiskPolicy {
            version: "live-risk-v1".to_owned(),
            trading_calendar_id: "calendar.nyse.v1".to_owned(),
            max_order_quantity: amount("10"),
            max_order_notional: amount("100"),
            max_price_deviation_bps: amount("100"),
            canary_max_order_notional: amount("50"),
            canary_max_orders: 2,
            max_open_orders: 2,
            max_position_quantity: amount("10"),
            max_realized_loss: amount("100"),
            max_market_data_age_seconds: 60,
            max_order_rate: 20,
            order_rate_window_seconds: 60,
            portfolio_risk: None,
            short_exposure: None,
            instrument_tick_sizes: [
                "inst.us_equity.spy",
                "inst.us_equity.qqq",
                "inst.us_option.spy.near",
                "inst.us_option.spy.far",
            ]
            .into_iter()
            .map(|instrument| (instrument.to_owned(), amount("0.01")))
            .collect(),
            // A lot of one: every whole quantity passes, so only a test that
            // sets a coarser lot exercises the lot rule.
            instrument_lot_sizes: [
                "inst.us_equity.spy",
                "inst.us_equity.qqq",
                "inst.us_option.spy.near",
                "inst.us_option.spy.far",
            ]
            .into_iter()
            .map(|instrument| (instrument.to_owned(), amount("1")))
            .collect(),
        }
    }

    #[test]
    fn live_position_limit_counts_unfilled_working_orders() {
        let path = journal_path("working-position-limit");
        let mut risk_policy = policy();
        risk_policy.max_position_quantity = amount("3");
        let mut service = test_service_with_policy(LiveRunMode::Canary, &path, risk_policy);
        let first = intent("LIVE", "intent.live.position.first");
        let approval = approval_for(&service, &first);
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        service
            .submit_canary_intent(
                first,
                market(),
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("first working order");
        let second = service
            .evaluate_risk(
                &intent("LIVE", "intent.live.position.second"),
                &market(),
                "2026-01-02T14:30:01Z",
                false,
            )
            .expect("second assessment");
        assert!(!second.approved);
        assert!(second
            .reason_codes
            .contains(&"POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned()));
        assert!(second
            .evaluated_limits
            .contains("working_position_delta=2.00000000,committed_position=4.00000000"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn live_aggregate_limit_counts_unfilled_working_orders() {
        let path = journal_path("working-aggregate-limit");
        let mut risk_policy = policy();
        let mut aggregate = permissive_portfolio_risk_policy();
        aggregate.max_gross_exposure = amount("30");
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: aggregate,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let mut service = test_service_with_policy(LiveRunMode::Canary, &path, risk_policy);
        let first = intent("LIVE", "intent.live.aggregate.first");
        let approval = approval_for(&service, &first);
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        service
            .submit_canary_intent(
                first,
                market(),
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("first working order");
        let second = service
            .evaluate_risk(
                &intent("LIVE", "intent.live.aggregate.second"),
                &market(),
                "2026-01-02T14:30:01Z",
                false,
            )
            .expect("second assessment");
        assert!(!second.approved);
        assert!(second
            .reason_codes
            .contains(&"MAX_GROSS_EXPOSURE_EXCEEDED".to_owned()));
        assert!(second
            .evaluated_limits
            .contains("portfolio_gross_exposure=40.00000000"));
        assert!(second
            .evaluated_limits
            .contains("portfolio_exposure_basis=filled_working_candidate_v2"));
        let _ = fs::remove_file(&path);
    }

    /// The same policy with net short exposure explicitly permitted, bounded at
    /// 10 per instrument. Combination tests that need a short leg use this;
    /// none of them may quietly widen `policy()` instead.
    fn policy_permitting_shorts() -> LiveRiskPolicy {
        LiveRiskPolicy {
            short_exposure: Some(ShortExposurePolicy {
                max_short_quantity: amount("10"),
            }),
            ..policy()
        }
    }

    fn activation(
        mode: LiveRunMode,
        account: &LiveAccount,
        policy: &LiveRiskPolicy,
        switches: &LiveKillSwitchRegistry,
    ) -> LiveActivation {
        LiveActivation {
            activation_id: "activation.live.001".to_owned(),
            mode,
            configuration_fingerprint: configuration_fingerprint(account, policy, switches),
            requested_by: "operator.requester.001".to_owned(),
            approved_by: "operator.approver.001".to_owned(),
            activated_at: "2026-01-02T14:00:00Z".to_owned(),
            expires_at: "2026-12-31T23:59:59Z".to_owned(),
        }
    }

    fn test_service(mode: LiveRunMode, path: &Path) -> LiveTradingService<TestBroker> {
        let account = account();
        let policy = policy();
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation = activation(mode, &account, &policy, &switches);
        LiveTradingService::open_durable(
            account,
            policy,
            activation,
            switches,
            TestBroker::new(),
            path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service")
    }

    fn test_service_with_policy(
        mode: LiveRunMode,
        path: &Path,
        policy: LiveRiskPolicy,
    ) -> LiveTradingService<TestBroker> {
        let account = account();
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation = activation(mode, &account, &policy, &switches);
        LiveTradingService::open_durable(
            account,
            policy,
            activation,
            switches,
            TestBroker::new(),
            path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service")
    }

    /// A permissive aggregate policy: every real limit is wide enough that
    /// nothing rejects until a test deliberately tightens one field.
    /// `max_drawdown_bps` at `10000` (100%) can never trigger -- the ratio is
    /// always strictly below `10000` since equity must be positive to reach
    /// this kernel at all -- unlike a `0` sentinel, which would be the
    /// tightest possible drawdown limit now that drawdown is genuinely
    /// computed (Slice 2), not permanently zeroed (Slice 1).
    fn permissive_portfolio_risk_policy() -> follon_risk::PortfolioRiskPolicy {
        follon_risk::PortfolioRiskPolicy {
            version: "portfolio-risk-v1".to_owned(),
            global_kill_switch: false,
            max_gross_exposure: amount("100000"),
            max_abs_net_exposure: amount("100000"),
            max_leverage_bps: amount("10000"),
            max_concentration_bps: amount("10000"),
            // Never trip on daily loss unless a test deliberately overrides
            // it: `i64::MAX`, the same "no real limit" sentinel the CLI
            // loader uses when an operator omits `max_daily_loss`.
            max_daily_loss: Decimal::from_integer(i64::MAX).unwrap(),
            max_drawdown_bps: amount("10000"),
            max_margin_utilization_bps: Decimal::ZERO,
            max_abs_delta: Decimal::ZERO,
            max_abs_gamma: Decimal::ZERO,
            max_open_orders: usize::MAX,
            max_order_rate: u32::MAX,
            allowed_instruments: BTreeSet::new(),
            restricted_instruments: BTreeSet::new(),
            sector_limits: BTreeMap::new(),
            asset_class_limits: BTreeMap::new(),
            currency_limits: BTreeMap::new(),
            strategy_limits: BTreeMap::new(),
            max_news_slippage_bps: None,
            max_spread_multiplier_bps: None,
        }
    }

    fn intent(environment: &str, intent_id: &str) -> OrderIntent {
        OrderIntent {
            intent_id: intent_id.to_owned(),
            account_id: "acct.live.001".to_owned(),
            strategy_id: "strategy.live.001".to_owned(),
            instrument_id: "inst.us_equity.spy".to_owned(),
            correlation_id: format!("corr-{intent_id}"),
            side: Side::Buy,
            quantity: amount("2"),
            order_type: OrderType::Market,
            limit_price: None,
            time_in_force: TimeInForce::Day,
            rationale: "controlled-live test intent".to_owned(),
            created_at: "2026-01-02T14:30:00Z".to_owned(),
            strategy_version: "strategy-live-v1".to_owned(),
            configuration_version: "config-live-v1".to_owned(),
            environment: environment.to_owned(),
        }
    }

    fn market() -> LiveMarketData {
        LiveMarketData {
            instrument_id: "inst.us_equity.spy".to_owned(),
            mark_price: amount("10"),
            observed_at: "2026-01-02T14:30:00Z".to_owned(),
        }
    }

    fn live_session(exchange_date: &str) -> TradingSession {
        let (opens_at, closes_at) = if exchange_date >= "2026-03-09" {
            ("13:30:00Z", "20:00:00Z")
        } else {
            ("14:30:00Z", "21:00:00Z")
        };
        TradingSession {
            exchange_date: exchange_date.to_owned(),
            opens_at: format!("{exchange_date}T{opens_at}"),
            closes_at: format!("{exchange_date}T{closes_at}"),
        }
    }

    fn live_calendar(sessions: &[TradingSession]) -> StaticTradingCalendar {
        StaticTradingCalendar::new("calendar.nyse.v1", sessions.to_vec())
            .expect("test live calendar")
    }

    fn approval_for(
        service: &LiveTradingService<TestBroker>,
        intent: &OrderIntent,
    ) -> LiveApproval {
        LiveApproval {
            approval_id: "approval.live.001".to_owned(),
            intent_id: intent.intent_id.clone(),
            intent_fingerprint: intent_fingerprint(intent).expect("test intent fingerprint"),
            configuration_fingerprint: service.configuration_fingerprint(),
            requested_by: "operator.requester.001".to_owned(),
            approved_by: "operator.approver.001".to_owned(),
            approved_at: "2026-01-02T14:30:00Z".to_owned(),
            expires_at: "2026-01-02T15:00:00Z".to_owned(),
        }
    }

    /// A long call vertical: buy the near strike at 7.50, sell the far at 5.00,
    /// for a 2.50 net debit per combination unit. Two units, so 25 gross
    /// notional and a 5 net debit against the test account's 50 canary ceiling
    /// and 100 deployed-capital ceiling.
    fn combo_intent(intent_id: &str) -> ComboIntent {
        ComboIntent {
            intent_id: intent_id.to_owned(),
            account_id: "acct.live.001".to_owned(),
            strategy_id: "strategy.live.001".to_owned(),
            correlation_id: format!("corr-{intent_id}"),
            legs: vec![
                follon_domain::ComboIntentLeg {
                    instrument_id: "inst.us_option.spy.near".to_owned(),
                    side: Side::Buy,
                    ratio: 1,
                    limit_price: amount("7.50"),
                },
                follon_domain::ComboIntentLeg {
                    instrument_id: "inst.us_option.spy.far".to_owned(),
                    side: Side::Sell,
                    ratio: 1,
                    limit_price: amount("5"),
                },
            ],
            combo_quantity: amount("2"),
            price_limit: follon_domain::ComboPriceLimit::MaximumDebit(amount("2.50")),
            time_in_force: TimeInForce::Day,
            rationale: "controlled-live combination test".to_owned(),
            created_at: "2026-01-02T14:30:00Z".to_owned(),
            strategy_version: "strategy-live-v1".to_owned(),
            configuration_version: "config-live-v1".to_owned(),
            environment: "LIVE".to_owned(),
        }
    }

    /// Marks sitting exactly on each leg's own limit price, so the per-leg
    /// collar reads zero deviation unless a test moves one.
    fn combo_market() -> LiveComboMarketData {
        LiveComboMarketData {
            marks: vec![
                LiveMarketData {
                    instrument_id: "inst.us_option.spy.near".to_owned(),
                    mark_price: amount("7.50"),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                },
                LiveMarketData {
                    instrument_id: "inst.us_option.spy.far".to_owned(),
                    mark_price: amount("5"),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                },
            ],
        }
    }

    #[test]
    fn live_combo_risk_approves_a_priced_vertical_and_records_exact_evidence() {
        let path = journal_path("combo-approve");
        let mut service =
            test_service_with_policy(LiveRunMode::Canary, &path, policy_permitting_shorts());
        let decision = service
            .evaluate_combo_risk(
                &combo_intent("intent.live.combo.001"),
                &combo_market(),
                "2026-01-02T14:30:02Z",
                false,
            )
            .expect("combination assessment");
        assert!(decision.approved, "{:?}", decision.reason_codes);
        // A combination decision must never collide with a plain order's.
        assert_eq!(
            decision.decision_id,
            "live-combo-risk-intent.live.combo.001"
        );
        assert!(decision.evaluated_limits.contains("combo_legs=2"));
        // 2 units * (7.50 + 5.00) = 25 gross; net debit 2 * 2.50 = 5.
        assert!(decision
            .evaluated_limits
            .contains("combo_gross_notional=25.00000000"));
        assert!(decision
            .evaluated_limits
            .contains("combo_net_debit=5.00000000"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn live_combo_legs_meet_the_plain_order_tick_rule_and_the_net_the_finest_grid() {
        let path = journal_path("combo-ticks");
        let decide = |ticks: &[(&str, &str)], near: &str, far: &str, cap: &str| {
            let _ = fs::remove_file(&path);
            let mut policy = policy_permitting_shorts();
            for leg in ["inst.us_option.spy.near", "inst.us_option.spy.far"] {
                policy.instrument_tick_sizes.remove(leg);
                // A leg with no tick is listed in neither table, or the
                // service refuses to open (E3.6f).
                if !ticks.iter().any(|(instrument, _)| *instrument == leg) {
                    policy.instrument_lot_sizes.remove(leg);
                }
            }
            for (instrument, tick) in ticks {
                policy
                    .instrument_tick_sizes
                    .insert((*instrument).to_owned(), amount(tick));
            }
            let mut service = test_service_with_policy(LiveRunMode::Canary, &path, policy);
            let mut intent = combo_intent("intent.live.combo.ticks");
            intent.legs[0].limit_price = amount(near);
            intent.legs[1].limit_price = amount(far);
            intent.price_limit = follon_domain::ComboPriceLimit::MaximumDebit(amount(cap));
            let mut market = combo_market();
            market.marks[0].mark_price = amount(near);
            market.marks[1].mark_price = amount(far);
            service
                .evaluate_combo_risk(&intent, &market, "2026-01-02T14:30:02Z", false)
                .expect("combination assessment")
        };
        let near = "inst.us_option.spy.near";
        let far = "inst.us_option.spy.far";

        let on_grid = decide(&[(near, "0.05"), (far, "0.01")], "7.50", "5", "2.51");
        assert!(on_grid.approved, "{:?}", on_grid.reason_codes);
        assert!(on_grid.evaluated_limits.contains(
            "combo_tick_sizes=[inst.us_option.spy.near:0.05000000|inst.us_option.spy.far:0.01000000]"
        ));
        let unlisted = decide(&[(near, "0.01")], "7.50", "5", "2.50");
        assert!(unlisted
            .reason_codes
            .contains(&"INSTRUMENT_TICK_SIZE_UNCONFIGURED".to_owned()));
        let off_leg = decide(&[(near, "0.05"), (far, "0.01")], "7.52", "5.02", "2.50");
        assert_eq!(
            off_leg.reason_codes,
            vec!["LIMIT_PRICE_OFF_TICK_GRID".to_owned()]
        );
        let off_net = decide(&[(near, "0.01"), (far, "0.01")], "7.50", "5", "2.505");
        assert_eq!(
            off_net.reason_codes,
            vec!["COMBO_NET_PRICE_OFF_TICK_GRID".to_owned()]
        );
        let coarse = decide(&[(near, "0.05"), (far, "0.05")], "7.50", "5", "2.51");
        assert_eq!(
            coarse.reason_codes,
            vec!["COMBO_NET_PRICE_OFF_TICK_GRID".to_owned()]
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn live_combo_leg_quantities_meet_the_plain_order_lot_rule() {
        let path = journal_path("combo-lots");
        let near = "inst.us_option.spy.near";
        let far = "inst.us_option.spy.far";
        // A ratio-2 near leg at 6.00 against a ratio-1 far leg at 5.00: each
        // unit sends two near contracts and one far contract to the broker.
        let decide = |lots: &[(&str, &str)], units: &str| {
            let _ = fs::remove_file(&path);
            let mut policy = policy_permitting_shorts();
            for leg in [near, far] {
                policy.instrument_lot_sizes.remove(leg);
                // A leg with no lot is listed in neither table, or the
                // service refuses to open (E3.6f).
                if !lots.iter().any(|(instrument, _)| *instrument == leg) {
                    policy.instrument_tick_sizes.remove(leg);
                }
            }
            for (instrument, lot) in lots {
                policy
                    .instrument_lot_sizes
                    .insert((*instrument).to_owned(), amount(lot));
            }
            let mut service = test_service_with_policy(LiveRunMode::Canary, &path, policy);
            let mut intent = combo_intent("intent.live.combo.lots");
            intent.combo_quantity = amount(units);
            intent.legs[0].ratio = 2;
            intent.legs[0].limit_price = amount("6");
            intent.price_limit = follon_domain::ComboPriceLimit::MaximumDebit(amount("7"));
            let mut market = combo_market();
            market.marks[0].mark_price = amount("6");
            service
                .evaluate_combo_risk(&intent, &market, "2026-01-02T14:30:02Z", false)
                .expect("combination assessment")
        };

        // Two units are not a whole near lot of 4; only each leg's own
        // quantity (4 near, 2 far) can approve this.
        let on_lot = decide(&[(near, "4"), (far, "2")], "2");
        assert!(on_lot.approved, "{:?}", on_lot.reason_codes);
        assert!(on_lot.evaluated_limits.contains(
            "combo_lot_sizes=[inst.us_option.spy.near:4.00000000|inst.us_option.spy.far:2.00000000]"
        ));
        let off_lot = decide(&[(near, "2"), (far, "2")], "1");
        assert_eq!(
            off_lot.reason_codes,
            vec!["ORDER_QUANTITY_OFF_LOT_SIZE".to_owned()]
        );
        let unlisted = decide(&[(near, "1")], "2");
        // A combination's codes are sorted.
        assert_eq!(
            unlisted.reason_codes,
            vec![
                "INSTRUMENT_LOT_SIZE_UNCONFIGURED".to_owned(),
                "INSTRUMENT_TICK_SIZE_UNCONFIGURED".to_owned()
            ]
        );
        let _ = fs::remove_file(&path);
    }

    /// The canary notional ceiling is charged the gross, not the net.
    ///
    /// This is the controlled-LIVE decision that matters most in this slice.
    /// The canary exists to bound how much capital one controlled-LIVE order
    /// can put at risk, and a two-sided structure puts both legs at risk until
    /// it is closed. Charging the net would let an arbitrarily large spread
    /// through an arbitrarily small canary ceiling -- 5 of net debit against a
    /// 50 ceiling here, while the structure actually commits 75 of gross.
    #[test]
    fn live_combo_canary_ceiling_is_charged_the_gross_not_the_net() {
        let path = journal_path("combo-canary-notional");
        let mut service =
            test_service_with_policy(LiveRunMode::Canary, &path, policy_permitting_shorts());
        let mut intent = combo_intent("intent.live.combo.002");
        // 6 units: 75 gross, past the 50 canary ceiling, while the net debit is
        // only 15 and would pass a net-based check comfortably.
        intent.combo_quantity = amount("6");
        let decision = service
            .evaluate_combo_risk(&intent, &combo_market(), "2026-01-02T14:30:02Z", false)
            .expect("combination assessment");
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"CANARY_NOTIONAL_EXCEEDED".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("combo_gross_notional=75.00000000"));
        assert!(decision
            .evaluated_limits
            .contains("combo_net_debit=15.00000000"));
        let _ = fs::remove_file(&path);
    }

    /// Shadow mode relaxes exactly the canary checks and nothing else.
    #[test]
    fn live_combo_shadow_relaxes_only_the_canary_ceilings() {
        let path = journal_path("combo-shadow");
        let mut service =
            test_service_with_policy(LiveRunMode::Shadow, &path, policy_permitting_shorts());
        let mut intent = combo_intent("intent.live.combo.003");
        intent.combo_quantity = amount("6");
        let decision = service
            .evaluate_combo_risk(&intent, &combo_market(), "2026-01-02T14:30:02Z", true)
            .expect("shadow assessment");
        assert!(!decision
            .reason_codes
            .contains(&"CANARY_NOTIONAL_EXCEEDED".to_owned()));
        // But a real limit still binds: 75 gross is under the 100 order-notional
        // ceiling, so confirm shadow did not simply approve everything.
        assert!(decision.approved, "{:?}", decision.reason_codes);

        let mut oversized = combo_intent("intent.live.combo.004");
        oversized.combo_quantity = amount("10");
        let decision = service
            .evaluate_combo_risk(&oversized, &combo_market(), "2026-01-02T14:30:02Z", true)
            .expect("shadow assessment");
        assert!(decision
            .reason_codes
            .contains(&"MAX_ORDER_NOTIONAL_EXCEEDED".to_owned()));
        let _ = fs::remove_file(&path);
    }

    /// The deployed-capital ceiling is charged the net debit.
    ///
    /// Unlike the canary ceiling, this one measures cash actually committed,
    /// and a credit structure commits none. Charging it the gross would refuse
    /// combinations that deploy no capital at all.
    #[test]
    fn live_combo_deployed_capital_ceiling_is_charged_the_net_debit() {
        let path = journal_path("combo-deployed");
        let mut service =
            test_service_with_policy(LiveRunMode::Canary, &path, policy_permitting_shorts());
        // A credit structure deploys nothing, so it must not be refused for the
        // deployed-capital ceiling however large its gross is.
        let mut credit = combo_intent("intent.live.combo.005");
        credit.legs[0].side = Side::Sell;
        credit.legs[1].side = Side::Buy;
        credit.price_limit = follon_domain::ComboPriceLimit::MinimumCredit(amount("2"));
        let decision = service
            .evaluate_combo_risk(&credit, &combo_market(), "2026-01-02T14:30:02Z", false)
            .expect("credit assessment");
        assert!(!decision
            .reason_codes
            .contains(&"DEPLOYED_CAPITAL_CEILING_EXCEEDED".to_owned()));
        assert!(!decision
            .reason_codes
            .contains(&"INSUFFICIENT_INTERNAL_CASH".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("combo_net_debit=0.00000000"));
        let _ = fs::remove_file(&path);
    }

    /// A combination's short leg is refused until an operator permits it.
    ///
    /// Controlled-LIVE holds no option reference data either, so it cannot
    /// prove the short far-strike leg is covered by the long near one, and does
    /// not assume it. The permission is a separate type from `core/paper`'s on
    /// purpose: permitting shorts in PAPER must never permit them with real
    /// capital as a side effect.
    #[test]
    fn live_combo_refuses_a_short_leg_until_an_operator_permits_it() {
        let path = journal_path("combo-short");
        let mut default_service = test_service(LiveRunMode::Canary, &path);
        let decision = default_service
            .evaluate_combo_risk(
                &combo_intent("intent.live.combo.006"),
                &combo_market(),
                "2026-01-02T14:30:02Z",
                false,
            )
            .expect("assessment");
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned()));

        let permitting_path = journal_path("combo-short-permitted");
        let mut permitting = test_service_with_policy(
            LiveRunMode::Canary,
            &permitting_path,
            policy_permitting_shorts(),
        );
        let decision = permitting
            .evaluate_combo_risk(
                &combo_intent("intent.live.combo.006"),
                &combo_market(),
                "2026-01-02T14:30:02Z",
                false,
            )
            .expect("assessment");
        assert!(decision.approved, "{:?}", decision.reason_codes);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&permitting_path);
    }

    #[test]
    fn live_combo_is_halted_by_a_kill_switch_on_any_single_leg() {
        let path = journal_path("combo-kill");
        let mut service =
            test_service_with_policy(LiveRunMode::Canary, &path, policy_permitting_shorts());
        service
            .activate_kill_switch(
                LiveKillSwitchScope::Instrument("inst.us_option.spy.far".to_owned()),
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("kill switch");
        let decision = service
            .evaluate_combo_risk(
                &combo_intent("intent.live.combo.007"),
                &combo_market(),
                "2026-01-02T14:30:02Z",
                false,
            )
            .expect("assessment");
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"KILL_SWITCH_INSTRUMENT_INST.US_OPTION.SPY.FAR".to_owned()));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn live_combo_refuses_an_incomplete_or_stale_observation() {
        let path = journal_path("combo-observation");
        let mut service =
            test_service_with_policy(LiveRunMode::Canary, &path, policy_permitting_shorts());
        let intent = combo_intent("intent.live.combo.008");

        // One leg unquoted: the gate cannot price the structure and must not
        // guess, least of all with real capital behind it.
        let mut partial = combo_market();
        partial.marks.truncate(1);
        assert!(service
            .evaluate_combo_risk(&intent, &partial, "2026-01-02T14:30:02Z", false)
            .is_err());

        // One leg quoted twice, the other not at all: the count matches but the
        // coverage does not.
        let mut duplicated = combo_market();
        duplicated.marks[1] = duplicated.marks[0].clone();
        assert!(service
            .evaluate_combo_risk(&intent, &duplicated, "2026-01-02T14:30:02Z", false)
            .is_err());

        // Freshness is the stalest leg's: one fresh quote must not launder an
        // old one beside it.
        let mut half_stale = combo_market();
        half_stale.marks[0].observed_at = "2026-01-02T14:20:00Z".to_owned();
        assert!(service
            .evaluate_combo_risk(&intent, &half_stale, "2026-01-02T14:30:02Z", false)
            .is_err());
        let _ = fs::remove_file(&path);
    }

    /// An approval fingerprint binds the exact structure, leg by leg.
    ///
    /// An operator approves a specific combination. If the fingerprint covered
    /// less than every leg's instrument, side, ratio and protected price, an
    /// approval for a two-leg debit spread could be consumed by a materially
    /// different trade.
    #[test]
    fn live_combo_fingerprint_binds_every_leg_of_the_approved_structure() {
        let base = combo_intent("intent.live.combo.009");
        let fingerprint = combo_intent_fingerprint(&base).expect("fingerprint");
        assert_eq!(
            fingerprint,
            combo_intent_fingerprint(&combo_intent("intent.live.combo.009")).expect("stable")
        );

        let mutate = |apply: &dyn Fn(&mut ComboIntent)| {
            let mut altered = combo_intent("intent.live.combo.009");
            apply(&mut altered);
            combo_intent_fingerprint(&altered).expect("fingerprint")
        };
        // Every one of these is a different trade and must produce a different
        // fingerprint.
        // These two also move the price limit, because a ratio or leg-price
        // change moves the protected net price with it and `ComboIntent` would
        // otherwise refuse the mutated intent as exceeding its own cap. The
        // point of each mutation is that it is a *different valid trade*, not
        // an invalid one.
        assert_ne!(
            fingerprint,
            mutate(&|i| {
                i.legs[0].ratio = 2;
                i.price_limit = follon_domain::ComboPriceLimit::MaximumDebit(amount("10"));
            })
        );
        assert_ne!(
            fingerprint,
            mutate(&|i| {
                i.legs[0].limit_price = amount("8");
                i.price_limit = follon_domain::ComboPriceLimit::MaximumDebit(amount("3"));
            })
        );
        assert_ne!(fingerprint, mutate(&|i| i.combo_quantity = amount("3")));
        assert_ne!(
            fingerprint,
            mutate(&|i| i.legs[0].instrument_id = "inst.us_option.spy.other".to_owned())
        );
        assert_ne!(
            fingerprint,
            mutate(&|i| {
                i.legs[0].side = Side::Sell;
                i.legs[1].side = Side::Buy;
                i.price_limit = follon_domain::ComboPriceLimit::MinimumCredit(amount("2"));
            })
        );
        // Swapping the two legs is a different document, not the same one.
        assert_ne!(fingerprint, mutate(&|i| i.legs.swap(0, 1)));
    }

    #[test]
    fn live_combo_assessment_creates_no_order_and_consumes_no_approval() {
        let path = journal_path("combo-inert");
        let mut service =
            test_service_with_policy(LiveRunMode::Canary, &path, policy_permitting_shorts());
        let decision = service
            .evaluate_combo_risk(
                &combo_intent("intent.live.combo.010"),
                &combo_market(),
                "2026-01-02T14:30:02Z",
                false,
            )
            .expect("assessment");
        assert!(decision.approved, "{:?}", decision.reason_codes);
        // Assessment only, until E1.4b lands a submission path.
        assert!(service.orders.is_empty());
        assert_eq!(service.broker_mut().submitted, 0);
        let _ = fs::remove_file(&path);
    }

    fn combo_approval_for(
        service: &LiveTradingService<TestBroker>,
        intent: &ComboIntent,
    ) -> LiveApproval {
        LiveApproval {
            approval_id: "approval.live.001".to_owned(),
            intent_id: intent.intent_id.clone(),
            intent_fingerprint: combo_intent_fingerprint(intent).expect("combo fingerprint"),
            configuration_fingerprint: service.configuration_fingerprint(),
            requested_by: "operator.requester.001".to_owned(),
            approved_by: "operator.approver.001".to_owned(),
            approved_at: "2026-01-02T14:30:00Z".to_owned(),
            expires_at: "2026-01-02T15:00:00Z".to_owned(),
        }
    }

    /// Registers the approval, connects, and returns a canary service ready to
    /// submit exactly this combination.
    fn canary_ready(path: &Path, intent: &ComboIntent) -> LiveTradingService<TestBroker> {
        let mut service =
            test_service_with_policy(LiveRunMode::Canary, path, policy_permitting_shorts());
        let approval = combo_approval_for(&service, intent);
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        service
    }

    #[test]
    fn live_combo_submission_consumes_one_approval_and_one_canary_slot() {
        let path = journal_path("combo-submit");
        let intent = combo_intent("intent.live.combo.100");
        let mut service = canary_ready(&path, &intent);
        let outcome = service
            .submit_canary_combo_intent(
                intent.clone(),
                combo_market(),
                "approval.live.001",
                "2026-01-02T14:30:02Z",
                "operator.requester.001",
            )
            .expect("bounded combination submission");
        assert!(matches!(
            outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let order = service
            .combo_order("combo-order-intent.live.combo.100")
            .expect("durable combination");
        assert_eq!(order.oms.intent.legs.len(), 2);
        // One broker order for the whole group, not one per leg.
        assert_eq!(order.broker_order_versions.len(), 1);
        assert_eq!(service.broker_mut().submitted, 1);
        // One canary slot, not one per leg: counting legs would exhaust an
        // operator's canary budget on a single ordinary structure.
        assert_eq!(service.canary_submissions, 1);
        // And the approval is spent.
        assert!(service.approvals["approval.live.001"].consumed);
        let _ = fs::remove_file(&path);
    }

    /// An approval bound to a plain order cannot authorize a combination.
    ///
    /// The two fingerprint functions are domain-separated by their prefixes, so
    /// this holds even when both intents carry the same identity.
    #[test]
    fn live_combo_refuses_an_approval_bound_to_a_different_shape_or_structure() {
        let path = journal_path("combo-approval-binding");
        let combination = combo_intent("intent.live.combo.101");
        let mut service =
            test_service_with_policy(LiveRunMode::Canary, &path, policy_permitting_shorts());
        // An approval carrying the *plain order* fingerprint for the same id.
        let plain = OrderIntent {
            intent_id: combination.intent_id.clone(),
            correlation_id: combination.correlation_id.clone(),
            ..intent("LIVE", "intent.live.placeholder")
        };
        let mut mismatched = combo_approval_for(&service, &combination);
        mismatched.intent_fingerprint = intent_fingerprint(&plain).expect("plain fingerprint");
        service
            .register_approval(mismatched, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("registration");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("connection");
        assert!(service
            .submit_canary_combo_intent(
                combination.clone(),
                combo_market(),
                "approval.live.001",
                "2026-01-02T14:30:02Z",
                "operator.requester.001",
            )
            .is_err());
        assert!(service.combo_orders.is_empty());
        assert_eq!(service.broker_mut().submitted, 0);
        let _ = fs::remove_file(&path);

        // And an approval bound to a *different combination* is refused too.
        let other_path = journal_path("combo-approval-structure");
        let mut altered = combo_intent("intent.live.combo.101");
        altered.legs[0].limit_price = amount("7");
        altered.price_limit = follon_domain::ComboPriceLimit::MaximumDebit(amount("2.50"));
        let mut service = canary_ready(&other_path, &altered);
        assert!(service
            .submit_canary_combo_intent(
                combination,
                combo_market(),
                "approval.live.001",
                "2026-01-02T14:30:02Z",
                "operator.requester.001",
            )
            .is_err());
        let _ = fs::remove_file(&other_path);
    }

    #[test]
    fn live_combo_submission_requires_an_active_canary_and_a_connected_session() {
        // Shadow mode records decisions but must never submit.
        let shadow_path = journal_path("combo-shadow-submit");
        let intent = combo_intent("intent.live.combo.102");
        let mut shadow = test_service_with_policy(
            LiveRunMode::Shadow,
            &shadow_path,
            policy_permitting_shorts(),
        );
        assert!(shadow
            .submit_canary_combo_intent(
                intent.clone(),
                combo_market(),
                "approval.live.001",
                "2026-01-02T14:30:02Z",
                "operator.requester.001",
            )
            .is_err());
        let _ = fs::remove_file(&shadow_path);

        // A canary that has not connected must not submit either.
        let path = journal_path("combo-disconnected");
        let mut service =
            test_service_with_policy(LiveRunMode::Canary, &path, policy_permitting_shorts());
        let approval = combo_approval_for(&service, &intent);
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("approval");
        assert!(service
            .submit_canary_combo_intent(
                intent,
                combo_market(),
                "approval.live.001",
                "2026-01-02T14:30:02Z",
                "operator.requester.001",
            )
            .is_err());
        assert_eq!(service.broker_mut().submitted, 0);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn live_combo_submission_is_idempotent_and_refuses_a_changed_retry() {
        let path = journal_path("combo-idempotent");
        let intent = combo_intent("intent.live.combo.103");
        let mut service = canary_ready(&path, &intent);
        service
            .submit_canary_combo_intent(
                intent.clone(),
                combo_market(),
                "approval.live.001",
                "2026-01-02T14:30:02Z",
                "operator.requester.001",
            )
            .expect("first submission");
        service
            .submit_canary_combo_intent(
                intent.clone(),
                combo_market(),
                "approval.live.001",
                "2026-01-02T14:30:02Z",
                "operator.requester.001",
            )
            .expect("idempotent repeat");
        // The repeat reached no broker and consumed no second canary slot.
        assert_eq!(service.broker_mut().submitted, 1);
        assert_eq!(service.canary_submissions, 1);

        // A retry that re-prices the original is refused: a retry is a retry,
        // not a new decision wearing an old identity.
        let mut moved = combo_market();
        moved.marks[0].mark_price = amount("7.51");
        assert!(service
            .submit_canary_combo_intent(
                intent,
                moved,
                "approval.live.001",
                "2026-01-02T14:30:02Z",
                "operator.requester.001",
            )
            .is_err());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_working_live_combination_is_visible_to_every_single_order_risk_counter() {
        let path = journal_path("combo-counters");
        let intent = combo_intent("intent.live.combo.104");
        let mut service = canary_ready(&path, &intent);
        service
            .submit_canary_combo_intent(
                intent,
                combo_market(),
                "approval.live.001",
                "2026-01-02T14:30:02Z",
                "operator.requester.001",
            )
            .expect("submission");

        assert_eq!(service.working_order_count(), 1);
        // 2 units * 2.50 net debit.
        assert_eq!(
            service.total_reserved_cash().expect("reserved"),
            amount("5")
        );
        // The combination's short far-strike leg is a real resting sell, so a
        // plain buy on that instrument is a self-trade.
        assert!(service.conflicts_with_working_order("inst.us_option.spy.far", Side::Buy));
        assert!(!service.conflicts_with_working_order("inst.us_option.spy.far", Side::Sell));
        assert!(service.conflicts_with_working_order("inst.us_option.spy.near", Side::Sell));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_live_combination_survives_a_durable_journal_reopen() {
        let path = journal_path("combo-recovery");
        let intent = combo_intent("intent.live.combo.105");
        let market = combo_market();
        let mut service = canary_ready(&path, &intent);
        service
            .submit_canary_combo_intent(
                intent.clone(),
                market.clone(),
                "approval.live.001",
                "2026-01-02T14:30:02Z",
                "operator.requester.001",
            )
            .expect("submission");
        let before = service
            .combo_order("combo-order-intent.live.combo.105")
            .expect("durable combination")
            .clone();
        drop(service);

        let account = account();
        let policy = policy_permitting_shorts();
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("switches");
        let activation = activation(LiveRunMode::Canary, &account, &policy, &switches);
        let reopened = LiveTradingService::open_durable(
            account,
            policy,
            activation,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T15:00:00Z",
        )
        .expect("reopen");
        let after = reopened
            .combo_order("combo-order-intent.live.combo.105")
            .expect("recovered combination");
        // The whole structure comes back exactly: every leg, its ratio, its
        // protected price, the price-limit kind, and the per-leg observation
        // that priced it.
        assert_eq!(after.oms.intent, intent);
        assert_eq!(after.market, market);
        assert_eq!(after.oms.state, before.oms.state);
        assert_eq!(after.broker_order_id, before.broker_order_id);
        assert_eq!(after.approval_id, "approval.live.001");
        // And the reservation it implies survives with it, so a restart does
        // not free capital the combination still has committed.
        assert_eq!(
            reopened.total_reserved_cash().expect("reserved"),
            amount("5")
        );
        // The consumed canary slot survives too: a restart must not hand an
        // operator a fresh canary budget.
        assert_eq!(reopened.canary_submissions, 1);
        let _ = fs::remove_file(&path);
    }

    /// A declared adapter whose combination submission fails is a transport
    /// outcome, so the attempt is recorded rather than erased. An adapter that
    /// declares nothing never gets that far (see
    /// `live_combo_on_an_adapter_that_cannot_execute_combinations_is_refused_before_anything_is_spent`).
    #[test]
    fn live_combo_transport_failure_leaves_the_group_unknown_and_keeps_the_approval_spent() {
        let path = journal_path("combo-transport");
        let intent = combo_intent("intent.live.combo.106");
        let mut service = canary_ready(&path, &intent);
        service.broker_mut().reject_combos = true;
        let result = service.submit_canary_combo_intent(
            intent,
            combo_market(),
            "approval.live.001",
            "2026-01-02T14:30:02Z",
            "operator.requester.001",
        );
        assert!(result.is_err());
        let order = service
            .combo_order("combo-order-intent.live.combo.106")
            .expect("durable combination");
        assert_eq!(order.oms.state, OrderState::Unknown);
        // The attempt happened. Releasing the approval or rewinding the canary
        // counter would let one approval authorize a second attempt at a trade
        // whose outcome is unknown.
        assert!(service.approvals["approval.live.001"].consumed);
        assert_eq!(service.canary_submissions, 1);
        assert!(service.has_unknown_order());
        let _ = fs::remove_file(&path);
    }

    /// A LIVE service whose aggregate composition is configured and whose
    /// policy permits shorts, holding a short of 2 sold at 10, plus a marks
    /// helper for what it would take to put it underwater.
    fn service_holding_a_short_under_aggregate_risk(label: &str) -> LiveTradingService<TestBroker> {
        let path = journal_path(label);
        let mut risk_policy = policy_permitting_shorts();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: permissive_portfolio_risk_policy(),
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let mut service = test_service_with_policy(LiveRunMode::Canary, &path, risk_policy);
        service
            .apply_accounted_fill(
                &Fill {
                    execution_id: "execution.live.underwater.short".to_owned(),
                    order_id: "order.live.underwater.short".to_owned(),
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    side: Side::Sell,
                    quantity: amount("2"),
                    price: amount("10"),
                    fee: Decimal::ZERO,
                    executed_at: "2026-01-02T14:31:00Z".to_owned(),
                },
                "strategy.live.001",
            )
            .expect("a short position");
        service
    }

    /// With equity not positive no aggregate ratio can be computed. Skipping
    /// the limits then would let an underwater account open exposure past every
    /// one of them, so only a trade that moves a position toward flat passes
    /// (delivery state E7.4b).
    #[test]
    fn an_underwater_live_account_may_only_reduce_a_position() {
        let mut service = service_holding_a_short_under_aggregate_risk("underwater-single");
        // Cash is 1,020 against a short of 2. A mark of 600 takes equity to
        // 1,020 - 2 * 600 = -180.
        let mut assess = |id: &str, side: Side, quantity: &str| {
            let mut order = intent("LIVE", id);
            order.side = side;
            order.quantity = amount(quantity);
            let market = LiveMarketData {
                instrument_id: "inst.us_equity.spy".to_owned(),
                mark_price: amount("600"),
                observed_at: "2026-01-02T14:32:00Z".to_owned(),
            };
            service
                .evaluate_risk(&order, &market, "2026-01-02T14:32:00Z", true)
                .expect("a risk decision")
        };
        let refused = "PORTFOLIO_EQUITY_NOT_POSITIVE".to_owned();

        let adding = assess("intent.live.underwater.add", Side::Sell, "1");
        assert!(adding.reason_codes.contains(&refused));
        // The kernel that cannot run is not reported as having run.
        assert!(!adding.evaluated_limits.contains("portfolio_gross_exposure"));
        let reversing = assess("intent.live.underwater.flip", Side::Buy, "3");
        assert!(reversing.reason_codes.contains(&refused));
        // Buying the short back, in part or entirely, moves it toward flat.
        for (id, quantity) in [
            ("intent.live.underwater.part", "1"),
            ("intent.live.underwater.all", "2"),
        ] {
            let reducing = assess(id, Side::Buy, quantity);
            assert!(!reducing.reason_codes.contains(&refused), "{id}");
        }
    }

    /// A combination is one atomic group, so it reduces risk only if every leg
    /// moves its own position toward flat.
    #[test]
    fn an_underwater_live_account_may_only_close_every_leg_of_a_combination() {
        let path = journal_path("underwater-combo");
        let mut risk_policy = policy_permitting_shorts();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: permissive_portfolio_risk_policy(),
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let mut service = test_service_with_policy(LiveRunMode::Canary, &path, risk_policy);
        for (instrument, side, price) in [
            ("inst.us_option.spy.near", Side::Buy, "7.50"),
            ("inst.us_option.spy.far", Side::Sell, "5"),
        ] {
            service
                .apply_accounted_fill(
                    &Fill {
                        execution_id: format!("execution.live.underwater.{instrument}"),
                        order_id: "order.live.underwater.vertical".to_owned(),
                        instrument_id: instrument.to_owned(),
                        side,
                        quantity: amount("4"),
                        price: amount(price),
                        fee: Decimal::ZERO,
                        executed_at: "2026-01-02T14:31:00Z".to_owned(),
                    },
                    "strategy.live.001",
                )
                .expect("a leg of the vertical");
        }
        // Long 4 near, short 4 far. The short leg's mark rising to 30,000
        // takes equity far below zero.
        let market = LiveComboMarketData {
            marks: vec![
                LiveMarketData {
                    instrument_id: "inst.us_option.spy.near".to_owned(),
                    mark_price: amount("7.50"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
                LiveMarketData {
                    instrument_id: "inst.us_option.spy.far".to_owned(),
                    mark_price: amount("30000"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
            ],
        };
        let mut assess =
            |id: &str, near: Side, far: Side, limit: follon_domain::ComboPriceLimit| {
                let mut structure = combo_intent(id);
                structure.combo_quantity = amount("1");
                structure.legs[0].side = near;
                structure.legs[0].limit_price = amount("7.50");
                structure.legs[1].side = far;
                structure.legs[1].limit_price = amount("30000");
                structure.price_limit = limit;
                service
                    .evaluate_combo_risk(&structure, &market, "2026-01-02T14:32:00Z", true)
                    .expect("a combination decision")
            };
        let refused = "PORTFOLIO_EQUITY_NOT_POSITIVE".to_owned();

        // Adding to both legs, and closing one leg while adding to the other,
        // are refused.
        let adding = assess(
            "intent.live.underwater.add",
            Side::Buy,
            Side::Sell,
            follon_domain::ComboPriceLimit::MinimumCredit(amount("29992.50")),
        );
        assert!(adding.reason_codes.contains(&refused));
        let mixed = assess(
            "intent.live.underwater.mixed",
            Side::Sell,
            Side::Sell,
            follon_domain::ComboPriceLimit::MinimumCredit(amount("30007.50")),
        );
        assert!(mixed.reason_codes.contains(&refused));
        // Order does not matter: the leg that adds may come first or last.
        let mixed_the_other_way = assess(
            "intent.live.underwater.mixed-reversed",
            Side::Buy,
            Side::Buy,
            follon_domain::ComboPriceLimit::MaximumDebit(amount("30007.50")),
        );
        assert!(mixed_the_other_way.reason_codes.contains(&refused));
        // Closing one unit of both legs is not refused for the equity.
        let closing = assess(
            "intent.live.underwater.close",
            Side::Sell,
            Side::Buy,
            follon_domain::ComboPriceLimit::MaximumDebit(amount("29992.50")),
        );
        assert!(!closing.reason_codes.contains(&refused));
    }

    /// Puts an order that is still working into the service, as a canary submission would,
    /// without spending an approval or a canary slot.
    fn rest_order(
        service: &mut LiveTradingService<TestBroker>,
        id: &str,
        side: Side,
        quantity: &str,
        filled: &str,
    ) {
        let mut order = intent("LIVE", id);
        order.side = side;
        order.quantity = amount(quantity);
        let market = market();
        let policy_version = service.policy.version.clone();
        let decision = LiveRiskDecision {
            decision_id: format!("live-risk-{id}"),
            approved: true,
            reason_codes: vec!["APPROVED".to_owned()],
            policy_version: policy_version.clone(),
            decided_at: "2026-01-02T14:31:00Z".to_owned(),
            market_fingerprint: market_fingerprint(&market),
            evaluated_limits: String::new(),
        };
        let core_decision = RiskDecision {
            decision_id: decision.decision_id.clone(),
            intent_id: id.to_owned(),
            approved: true,
            reason_codes: decision.reason_codes.clone(),
            policy_version,
            decided_at: decision.decided_at.clone(),
            correlation_id: order.correlation_id.clone(),
            actor: "live_risk_engine".to_owned(),
            evaluated_limits: String::new(),
        };
        let mut oms = OmsOrder::from_approved_intent(order, &core_decision).expect("an order");
        oms.transition(OrderState::Approved, "LIVE_RISK_APPROVED")
            .expect("approved");
        oms.transition(
            OrderState::PendingSubmit,
            "LIVE_CANARY_SUBMISSION_REQUESTED",
        )
        .expect("pending");
        service.orders.insert(
            oms.order_id.clone(),
            LiveOrder {
                oms,
                approval_id: "approval.live.rest".to_owned(),
                market,
                decision,
                broker_order_id: None,
                broker_order_versions: Vec::new(),
                replace_return_state: None,
                filled_quantity: amount(filled),
            },
        );
    }

    /// Two orders that each close the whole short are each a reduction alone and would
    /// together reverse it into a long, so an underwater account is refused the second
    /// (delivery state E7.4b, found in review): what is already working claims part of the
    /// position first.
    #[test]
    fn an_underwater_live_account_cannot_reverse_a_position_with_two_working_reductions() {
        let mut service = service_holding_a_short_under_aggregate_risk("underwater-working");
        let refused = "PORTFOLIO_EQUITY_NOT_POSITIVE".to_owned();
        let assess =
            |service: &mut LiveTradingService<TestBroker>, id: &str, side: Side, quantity: &str| {
                let mut order = intent("LIVE", id);
                order.side = side;
                order.quantity = amount(quantity);
                let market = LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("600"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                };
                service
                    .evaluate_risk(&order, &market, "2026-01-02T14:32:00Z", true)
                    .expect("a risk decision")
            };
        // Short 2 at a mark of 600 is underwater, and buying it all back alone is a reduction.
        assert!(
            !assess(&mut service, "intent.live.claims.alone", Side::Buy, "2")
                .reason_codes
                .contains(&refused)
        );

        // A sale that is working adds to the short and claims nothing of it.
        rest_order(
            &mut service,
            "intent.live.claims.sale",
            Side::Sell,
            "5",
            "0",
        );
        assert!(
            !assess(&mut service, "intent.live.claims.opposite", Side::Buy, "2")
                .reason_codes
                .contains(&refused)
        );

        // A purchase of the whole short that is working claims all of it.
        rest_order(
            &mut service,
            "intent.live.claims.cover",
            Side::Buy,
            "2",
            "0",
        );
        for (id, quantity) in [
            ("intent.live.claims.second", "2"),
            ("intent.live.claims.one", "1"),
        ] {
            assert!(
                assess(&mut service, id, Side::Buy, quantity)
                    .reason_codes
                    .contains(&refused),
                "{id}"
            );
        }
    }

    /// A part-filled order claims only what it has left.
    #[test]
    fn an_underwater_live_account_counts_only_the_unfilled_part_of_a_working_reduction() {
        let mut service = service_holding_a_short_under_aggregate_risk("underwater-part-filled");
        let refused = "PORTFOLIO_EQUITY_NOT_POSITIVE".to_owned();
        let fill = |id: &str, side: Side, price: &str| Fill {
            execution_id: format!("execution.live.claims.{id}"),
            order_id: format!("order.live.claims.{id}"),
            instrument_id: "inst.us_equity.spy".to_owned(),
            side,
            quantity: amount(if side == Side::Sell { "2" } else { "1" }),
            price: amount(price),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:30Z".to_owned(),
        };
        // Short 4 in all, then a purchase of 2 of which 1 has filled, at the mark of 600:
        // the short is 3, equity is 1,040 - 600 - 3 * 600 = -1,360, and the 1 still working
        // claims 1 of the 3.
        service
            .apply_accounted_fill(&fill("more", Side::Sell, "10"), "strategy.live.001")
            .expect("a larger short");
        service
            .apply_accounted_fill(&fill("part", Side::Buy, "600"), "strategy.live.001")
            .expect("the part that filled");
        rest_order(&mut service, "intent.live.claims.part", Side::Buy, "2", "1");
        let mut assess = |id: &str, quantity: &str| {
            let mut order = intent("LIVE", id);
            order.side = Side::Buy;
            order.quantity = amount(quantity);
            let market = LiveMarketData {
                instrument_id: "inst.us_equity.spy".to_owned(),
                mark_price: amount("600"),
                observed_at: "2026-01-02T14:32:00Z".to_owned(),
            };
            service
                .evaluate_risk(&order, &market, "2026-01-02T14:32:00Z", true)
                .expect("a risk decision")
        };
        // The other 2 may be bought back and not one more.
        assert!(!assess("intent.live.claims.rest", "2")
            .reason_codes
            .contains(&refused));
        assert!(assess("intent.live.claims.over", "3")
            .reason_codes
            .contains(&refused));
    }

    /// A combination's legs claim their instruments as plain orders do, by the units unfilled.
    #[test]
    fn an_underwater_live_account_counts_the_legs_of_a_working_combination() {
        let path = journal_path("underwater-combo-working");
        let mut risk_policy = policy_permitting_shorts();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: permissive_portfolio_risk_policy(),
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let mut service = test_service_with_policy(LiveRunMode::Canary, &path, risk_policy);
        for (instrument, side, price) in [
            ("inst.us_option.spy.near", Side::Buy, "7.50"),
            ("inst.us_option.spy.far", Side::Sell, "5"),
        ] {
            service
                .apply_accounted_fill(
                    &Fill {
                        execution_id: format!("execution.live.claims.{instrument}"),
                        order_id: "order.live.claims.vertical".to_owned(),
                        instrument_id: instrument.to_owned(),
                        side,
                        quantity: amount("4"),
                        price: amount(price),
                        fee: Decimal::ZERO,
                        executed_at: "2026-01-02T14:31:00Z".to_owned(),
                    },
                    "strategy.live.001",
                )
                .expect("a leg of the vertical");
        }
        let market = LiveComboMarketData {
            marks: vec![
                LiveMarketData {
                    instrument_id: "inst.us_option.spy.near".to_owned(),
                    mark_price: amount("7.50"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
                LiveMarketData {
                    instrument_id: "inst.us_option.spy.far".to_owned(),
                    mark_price: amount("30000"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
            ],
        };
        // Long 4 near, short 4 far: closing sells the near and buys the far.
        // A leg's ratio multiplies the units, so one unit at a ratio of three closes three
        // contracts.
        let closing = |id: &str, units: &str, ratio: u32| {
            let mut structure = combo_intent(id);
            structure.combo_quantity = amount(units);
            structure.legs[0].side = Side::Sell;
            structure.legs[0].ratio = ratio;
            structure.legs[0].limit_price = amount("7.50");
            structure.legs[1].side = Side::Buy;
            structure.legs[1].ratio = ratio;
            structure.legs[1].limit_price = amount("30000");
            structure.price_limit = follon_domain::ComboPriceLimit::MaximumDebit(amount("90000"));
            structure
        };
        let refused = "PORTFOLIO_EQUITY_NOT_POSITIVE".to_owned();

        // Three contracts of the closing are working, so of the four held one more fits and
        // two more would reverse the position.
        let resting = closing("intent.live.claims.combo.resting", "1", 3);
        let resting_decision = LiveRiskDecision {
            decision_id: "live-risk-resting".to_owned(),
            approved: true,
            reason_codes: vec!["APPROVED".to_owned()],
            policy_version: service.policy.version.clone(),
            decided_at: "2026-01-02T14:31:00Z".to_owned(),
            market_fingerprint: String::new(),
            evaluated_limits: String::new(),
        };
        let core_decision = RiskDecision {
            decision_id: resting_decision.decision_id.clone(),
            intent_id: resting.intent_id.clone(),
            approved: true,
            reason_codes: resting_decision.reason_codes.clone(),
            policy_version: resting_decision.policy_version.clone(),
            decided_at: resting_decision.decided_at.clone(),
            correlation_id: resting.correlation_id.clone(),
            actor: "live_risk_engine".to_owned(),
            evaluated_limits: String::new(),
        };
        let mut oms =
            OmsComboOrder::from_approved_intent(resting, &core_decision).expect("a combination");
        oms.transition(OrderState::Approved, "LIVE_COMBO_RISK_APPROVED")
            .expect("approved");
        oms.transition(
            OrderState::PendingSubmit,
            "LIVE_CANARY_COMBO_SUBMISSION_REQUESTED",
        )
        .expect("pending");
        service.combo_orders.insert(
            oms.order_id.clone(),
            LiveComboOrder {
                oms,
                approval_id: "approval.live.rest".to_owned(),
                market: market.clone(),
                decision: resting_decision,
                broker_order_id: None,
                broker_order_versions: Vec::new(),
                filled_quantity: Decimal::ZERO,
                executions: BTreeMap::new(),
            },
        );
        let one = service
            .evaluate_combo_risk(
                &closing("intent.live.claims.combo.one", "1", 1),
                &market,
                "2026-01-02T14:32:00Z",
                true,
            )
            .expect("a combination decision");
        assert!(!one.reason_codes.contains(&refused));
        let two = service
            .evaluate_combo_risk(
                &closing("intent.live.claims.combo.two", "2", 1),
                &market,
                "2026-01-02T14:32:00Z",
                true,
            )
            .expect("a combination decision");
        assert!(two.reason_codes.contains(&refused));
        let _ = fs::remove_file(&path);
    }

    /// An adapter that declares nothing is never handed a combination, a GTC
    /// order or a replacement.
    #[test]
    fn an_adapter_that_declares_nothing_carries_only_single_day_orders() {
        struct Silent;

        impl LiveBrokerAdapter for Silent {
            fn connect(&mut self, _: &str, _: &SecretMaterial) -> Result<(), LiveError> {
                Err(LiveError("silent".to_owned()))
            }
            fn submit(
                &mut self,
                _: &LiveBrokerOrderRequest,
            ) -> Result<LiveBrokerSubmitResult, LiveError> {
                Err(LiveError("silent".to_owned()))
            }
            fn cancel(&mut self, _: &str) -> Result<(), LiveError> {
                Err(LiveError("silent".to_owned()))
            }
            fn poll(&mut self) -> Result<Vec<LiveBrokerEvent>, LiveError> {
                Err(LiveError("silent".to_owned()))
            }
            fn snapshot(&mut self, _: &str) -> Result<LiveBrokerAccountSnapshot, LiveError> {
                Err(LiveError("silent".to_owned()))
            }
            fn reconnect(&mut self, _: &str, _: &SecretMaterial) -> Result<(), LiveError> {
                Err(LiveError("silent".to_owned()))
            }
        }

        assert_eq!(
            Silent.capabilities(),
            LiveBrokerCapabilities {
                combinations: false,
                good_til_cancelled: false,
                replacement: false,
            }
        );
    }

    /// A combination the adapter did not declare is refused before anything is
    /// spent: the approval stays usable, the canary budget is intact, the
    /// session stays connected and nothing is left `UNKNOWN`. The same approval
    /// then works once the adapter can carry it (delivery state E5.7).
    #[test]
    fn live_combo_on_an_adapter_that_cannot_execute_combinations_is_refused_before_anything_is_spent(
    ) {
        let path = journal_path("combo-undeclared");
        let intent = combo_intent("intent.live.combo.107");
        let mut service = canary_ready(&path, &intent);
        service.broker_mut().capabilities = LiveBrokerCapabilities::default();
        let submit = |service: &mut LiveTradingService<TestBroker>| {
            service.submit_canary_combo_intent(
                intent.clone(),
                combo_market(),
                "approval.live.001",
                "2026-01-02T14:30:02Z",
                "operator.requester.001",
            )
        };
        let error = submit(&mut service).expect_err("an undeclared combination is refused");
        assert!(
            error.0.contains("cannot execute combinations"),
            "{}",
            error.0
        );
        assert!(service
            .combo_order("combo-order-intent.live.combo.107")
            .is_none());
        assert!(!service.approvals["approval.live.001"].consumed);
        assert_eq!(service.canary_submissions, 0);
        assert!(!service.has_unknown_order());
        assert!(service.broker_connected);
        assert_eq!(service.broker_mut().submitted, 0);

        service.broker_mut().capabilities.combinations = true;
        assert!(matches!(
            submit(&mut service).expect("the same approval works once declared"),
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        assert_eq!(service.canary_submissions, 1);
        let _ = fs::remove_file(&path);
    }

    /// The broker request carries no time in force, so a GTC intent would
    /// silently become whatever the adapter places. A DAY-only adapter refuses
    /// it before the approval is spent.
    #[test]
    fn live_gtc_intent_on_a_day_only_adapter_is_refused_before_anything_is_spent() {
        let path = journal_path("gtc-undeclared");
        let mut service = test_service(LiveRunMode::Canary, &path);
        let mut gtc = intent("LIVE", "intent.live.gtc.001");
        gtc.time_in_force = TimeInForce::GoodTilCancelled;
        let approval = approval_for(&service, &gtc);
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let submit = |service: &mut LiveTradingService<TestBroker>| {
            service.submit_canary_intent(
                gtc.clone(),
                market(),
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
        };
        let error = submit(&mut service).expect_err("a GTC intent is refused");
        assert!(error.0.contains("carries only DAY orders"), "{}", error.0);
        assert!(service.orders.is_empty());
        assert!(!service.approvals["approval.live.001"].consumed);
        assert_eq!(service.canary_submissions, 0);
        assert!(service.broker_connected);
        assert_eq!(service.broker_mut().submitted, 0);

        service.broker_mut().capabilities.good_til_cancelled = true;
        assert!(matches!(
            submit(&mut service).expect("the same approval works once declared"),
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let _ = fs::remove_file(&path);
    }

    /// A replacement the adapter cannot carry leaves the order working instead
    /// of `UNKNOWN` with the session disconnected.
    #[test]
    fn live_replacement_on_an_adapter_that_cannot_replace_leaves_the_order_working() {
        let path = journal_path("replace-undeclared");
        let mut service = test_service(LiveRunMode::Canary, &path);
        let mut limit = intent("LIVE", "intent.live.replace.001");
        limit.order_type = OrderType::Limit;
        limit.limit_price = Some(amount("10"));
        let approval = approval_for(&service, &limit);
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        service
            .submit_canary_intent(
                limit,
                market(),
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        let order_id = "order-intent.live.replace.001";

        let error = service
            .replace_order(
                order_id,
                amount("9"),
                "operator.requester.001",
                "2026-01-02T14:31:00Z",
            )
            .expect_err("an undeclared replacement is refused");
        assert!(error.0.contains("cannot replace orders"), "{}", error.0);
        assert_eq!(service.orders[order_id].oms.state, OrderState::Acknowledged);
        assert!(!service.has_unknown_order());
        assert!(service.broker_connected);
        assert_eq!(service.broker_mut().replaced, 0);

        service.broker_mut().capabilities.replacement = true;
        service
            .replace_order(
                order_id,
                amount("9"),
                "operator.requester.001",
                "2026-01-02T14:31:00Z",
            )
            .expect("a declared replacement is sent");
        assert_eq!(service.broker_mut().replaced, 1);
        assert_eq!(
            service.orders[order_id].oms.state,
            OrderState::PendingReplace
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn canary_is_four_eyes_durable_and_reconciles_before_a_live_day_counts() {
        let path = journal_path("canary");
        let mut service = test_service(LiveRunMode::Canary, &path);
        let order_intent = intent("LIVE", "intent.live.001");
        let approval = approval_for(&service, &order_intent);
        assert!(service
            .register_approval(
                approval.clone(),
                "2026-01-02T14:30:00Z",
                "operator.requester.001"
            )
            .is_err());
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let outcome = service
            .submit_canary_intent(
                order_intent.clone(),
                market(),
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        assert!(matches!(
            outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let repeated = service
            .submit_canary_intent(
                order_intent,
                market(),
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("idempotent repeat");
        assert!(matches!(repeated, LiveSubmitOutcome::CanaryOrder { .. }));
        assert_eq!(service.broker_mut().submitted, 1);

        let broker = service.broker_mut();
        broker.events.push(LiveBrokerEvent::Execution {
            execution_id: "execution.live.001".to_owned(),
            client_order_id: "order-intent.live.001".to_owned(),
            broker_order_id: "broker-order-intent.live.001".to_owned(),
            quantity: amount("2"),
            price: amount("10"),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:00Z".to_owned(),
        });
        broker.snapshot.orders[0].state = OrderState::Filled;
        broker.snapshot.orders[0].filled_quantity = amount("2");
        broker.snapshot.positions.push(LiveBrokerPositionSnapshot {
            instrument_id: "inst.us_equity.spy".to_owned(),
            quantity: amount("2"),
        });
        broker.snapshot.cash = amount("980");
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
            .expect("broker event synchronization");
        let tax_lots = service.tax_lots("inst.us_equity.spy");
        assert_eq!(tax_lots.len(), 1);
        assert_eq!(tax_lots[0].remaining_quantity, amount("2"));
        assert_eq!(tax_lots[0].unit_cost, amount("10"));
        assert_eq!(service.realized_tax_pnl().unwrap(), Decimal::ZERO);
        let report = service
            .reconcile("operator.approver.001", "2026-01-02T21:01:00Z")
            .expect("independent reconciliation");
        assert!(report.is_clean());
        let session = live_session("2026-01-02");
        let calendar = live_calendar(std::slice::from_ref(&session));
        service
            .record_live_session(&session, &report, "operator.approver.001", &calendar)
            .expect("post-close clean evidence");
        let dashboard = service.monitoring_dashboard();
        assert_eq!(dashboard.clean_live_days, 1);
        assert_eq!(dashboard.unresolved_incidents, 0);
        assert!(!dashboard.promotion_eligible);

        drop(service);
        let recovered = test_service(LiveRunMode::Canary, &path);
        assert!(!recovered.monitoring_dashboard().broker_connected);
        assert_eq!(recovered.monitoring_dashboard().clean_live_days, 1);
        let recovered_lots = recovered.tax_lots("inst.us_equity.spy");
        assert_eq!(recovered_lots.len(), 1);
        assert_eq!(recovered_lots[0].remaining_quantity, amount("2"));
        assert_eq!(recovered_lots[0].unit_cost, amount("10"));
        assert_eq!(recovered.realized_tax_pnl().unwrap(), Decimal::ZERO);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn sixty_configured_clean_sessions_are_required_and_recoverable() {
        let path = journal_path("sixty-day-gate");
        let dates = [
            "2026-01-02",
            "2026-01-05",
            "2026-01-06",
            "2026-01-07",
            "2026-01-08",
            "2026-01-09",
            "2026-01-12",
            "2026-01-13",
            "2026-01-14",
            "2026-01-15",
            "2026-01-16",
            "2026-01-20",
            "2026-01-21",
            "2026-01-22",
            "2026-01-23",
            "2026-01-26",
            "2026-01-27",
            "2026-01-28",
            "2026-01-29",
            "2026-01-30",
            "2026-02-02",
            "2026-02-03",
            "2026-02-04",
            "2026-02-05",
            "2026-02-06",
            "2026-02-09",
            "2026-02-10",
            "2026-02-11",
            "2026-02-12",
            "2026-02-13",
            "2026-02-17",
            "2026-02-18",
            "2026-02-19",
            "2026-02-20",
            "2026-02-23",
            "2026-02-24",
            "2026-02-25",
            "2026-02-26",
            "2026-02-27",
            "2026-03-02",
            "2026-03-03",
            "2026-03-04",
            "2026-03-05",
            "2026-03-06",
            "2026-03-09",
            "2026-03-10",
            "2026-03-11",
            "2026-03-12",
            "2026-03-13",
            "2026-03-16",
            "2026-03-17",
            "2026-03-18",
            "2026-03-19",
            "2026-03-20",
            "2026-03-23",
            "2026-03-24",
            "2026-03-25",
            "2026-03-26",
            "2026-03-27",
            "2026-03-30",
        ];
        assert_eq!(dates.len(), 60);
        let sessions: Vec<_> = dates.iter().map(|date| live_session(date)).collect();
        let calendar = live_calendar(&sessions);
        let mut service = test_service(LiveRunMode::Canary, &path);
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:01:00Z",
            )
            .expect("managed-secret connection");

        for (index, session) in sessions.iter().enumerate() {
            let report = service
                .reconcile("operator.approver.001", &session.closes_at)
                .expect("clean independent reconciliation");
            assert!(report.is_clean());
            service
                .record_live_session(session, &report, "operator.approver.001", &calendar)
                .expect("calendar-backed session evidence");
            assert_eq!(service.promotion_status().clean_live_days, index as u32 + 1);
            assert_eq!(
                service.promotion_status().eligible_for_next_gate,
                index == 59
            );
        }
        drop(service);

        let recovered = test_service(LiveRunMode::Canary, &path);
        let promotion = recovered.promotion_status();
        assert_eq!(promotion.clean_live_days, 60);
        assert!(promotion.complete_auditability);
        assert!(promotion.eligible_for_next_gate);
        assert!(!recovered.monitoring_dashboard().broker_connected);
        drop(recovered);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn shadow_records_a_decision_without_requesting_a_broker_connection_or_submit() {
        let path = journal_path("shadow");
        let mut service = test_service(LiveRunMode::Shadow, &path);
        let outcome = service
            .record_shadow_intent(
                intent("SHADOW", "intent.shadow.001"),
                market(),
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("shadow decision");
        assert!(matches!(outcome, LiveSubmitOutcome::ShadowRecorded { .. }));
        assert!(!service.monitoring_dashboard().broker_connected);
        assert_eq!(service.broker_mut().submitted, 0);
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn controlled_live_refuses_off_grid_limits_and_unlisted_instruments() {
        let path = journal_path("tick-grid");
        let mut service = test_service_with_policy(LiveRunMode::Shadow, &path, policy());
        let mut off_grid = intent("SHADOW", "intent.shadow.tick.001");
        off_grid.order_type = OrderType::Limit;
        // 5 bps from the mark, well inside the collar: only the grid is wrong.
        off_grid.limit_price = Some(amount("10.005"));
        let LiveSubmitOutcome::ShadowRecorded { decision } = service
            .record_shadow_intent(
                off_grid,
                market(),
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("shadow evidence")
        else {
            panic!("shadow mode must retain a shadow decision");
        };
        assert!(!decision.approved);
        assert_eq!(
            decision.reason_codes,
            vec!["LIMIT_PRICE_OFF_TICK_GRID".to_owned()]
        );
        assert!(decision
            .evaluated_limits
            .contains("instrument_tick_size=0.01"));

        // A validated policy lists an instrument in both tables or in neither
        // (E3.6f), and iwm is in neither.
        let mut unlisted = intent("SHADOW", "intent.shadow.tick.002");
        unlisted.instrument_id = "inst.us_equity.iwm".to_owned();
        let mut iwm = market();
        iwm.instrument_id = "inst.us_equity.iwm".to_owned();
        let LiveSubmitOutcome::ShadowRecorded { decision } = service
            .record_shadow_intent(
                unlisted,
                iwm,
                "2026-01-02T14:30:01Z",
                "operator.requester.001",
            )
            .expect("shadow evidence")
        else {
            panic!("shadow mode must retain a shadow decision");
        };
        assert!(!decision.approved);
        assert_eq!(
            decision.reason_codes,
            vec![
                "INSTRUMENT_TICK_SIZE_UNCONFIGURED".to_owned(),
                "INSTRUMENT_LOT_SIZE_UNCONFIGURED".to_owned()
            ]
        );
        for evidence in [
            "instrument_tick_size=UNCONFIGURED",
            "instrument_lot_size=UNCONFIGURED",
        ] {
            assert!(decision.evaluated_limits.contains(evidence));
        }
        assert_eq!(service.broker_mut().submitted, 0);
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn a_live_tick_table_is_validated_and_bound_into_the_configuration_fingerprint() {
        assert!(policy().validate().is_ok());
        for broken in [
            BTreeMap::new(),
            BTreeMap::from([("inst.us_equity.spy".to_owned(), Decimal::ZERO)]),
            BTreeMap::from([("INST.SPY".to_owned(), amount("0.01"))]),
        ] {
            let policy = LiveRiskPolicy {
                instrument_tick_sizes: broken.clone(),
                ..policy()
            };
            assert!(policy.validate().is_err(), "accepted tick table {broken:?}");
        }
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").unwrap();
        let mut coarser = policy();
        coarser
            .instrument_tick_sizes
            .insert("inst.us_equity.spy".to_owned(), amount("0.05"));
        assert_ne!(
            configuration_fingerprint(&account(), &policy(), &switches),
            configuration_fingerprint(&account(), &coarser, &switches),
            "an approval must not carry across a changed tick table"
        );
    }

    #[test]
    fn controlled_live_refuses_off_lot_quantities() {
        // An instrument with no lot size is covered with the tick rule: a
        // validated policy lists it in neither table (E3.6f).
        let path = journal_path("lot-size");
        let mut policy = policy();
        policy
            .instrument_lot_sizes
            .insert("inst.us_equity.spy".to_owned(), amount("5"));
        let mut service = test_service_with_policy(LiveRunMode::Shadow, &path, policy);
        // Two shares against a five-share lot; every other limit passes.
        let LiveSubmitOutcome::ShadowRecorded { decision } = service
            .record_shadow_intent(
                intent("SHADOW", "intent.shadow.lot.001"),
                market(),
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("shadow evidence")
        else {
            panic!("shadow mode must retain a shadow decision");
        };
        assert!(!decision.approved);
        assert_eq!(
            decision.reason_codes,
            vec!["ORDER_QUANTITY_OFF_LOT_SIZE".to_owned()]
        );
        assert!(decision
            .evaluated_limits
            .contains("instrument_lot_size=5.00000000"));

        let mut whole_lot = intent("SHADOW", "intent.shadow.lot.002");
        whole_lot.quantity = amount("5");
        let LiveSubmitOutcome::ShadowRecorded { decision } = service
            .record_shadow_intent(
                whole_lot,
                market(),
                "2026-01-02T14:30:01Z",
                "operator.requester.001",
            )
            .expect("shadow evidence")
        else {
            panic!("shadow mode must retain a shadow decision");
        };
        assert!(decision.approved, "{:?}", decision.reason_codes);
        assert_eq!(service.broker_mut().submitted, 0);
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn the_live_tick_and_lot_tables_must_list_the_same_instruments() {
        let iwm = "inst.us_equity.iwm";
        let mut lot_only = policy();
        lot_only
            .instrument_lot_sizes
            .insert(iwm.to_owned(), amount("1"));
        let mut tick_only = policy();
        tick_only
            .instrument_tick_sizes
            .insert(iwm.to_owned(), amount("0.01"));
        let path = journal_path("unpaired-tables");
        for unpaired in [lot_only, tick_only] {
            assert_eq!(
                unpaired.validate().unwrap_err().0,
                "controlled-live risk policy lists inst.us_equity.iwm in only one of its tick and lot tables"
            );
            // The service refuses to open, before it can decide anything.
            let account = account();
            let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
            let activation = activation(LiveRunMode::Shadow, &account, &unpaired, &switches);
            assert!(LiveTradingService::open_durable(
                account,
                unpaired,
                activation,
                switches,
                TestBroker::new(),
                &path,
                "2026-01-02T14:00:00Z",
            )
            .is_err());
            assert!(!path.exists(), "a refused service created a journal");
        }
    }

    #[test]
    fn a_live_lot_table_is_validated_and_bound_into_the_configuration_fingerprint() {
        for broken in [
            BTreeMap::new(),
            BTreeMap::from([("inst.us_equity.spy".to_owned(), Decimal::ZERO)]),
            BTreeMap::from([("INST.SPY".to_owned(), amount("1"))]),
        ] {
            let policy = LiveRiskPolicy {
                instrument_lot_sizes: broken.clone(),
                ..policy()
            };
            assert!(policy.validate().is_err(), "accepted lot table {broken:?}");
        }
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").unwrap();
        let mut round_lots = policy();
        round_lots
            .instrument_lot_sizes
            .insert("inst.us_equity.spy".to_owned(), amount("100"));
        assert_ne!(
            configuration_fingerprint(&account(), &policy(), &switches),
            configuration_fingerprint(&account(), &round_lots, &switches),
            "an approval must not carry across a changed lot table"
        );
    }

    #[test]
    fn controlled_live_price_collar_rejection_retains_exact_limits() {
        let path = journal_path("price-collar");
        let mut service = test_service(LiveRunMode::Shadow, &path);
        let mut far_limit = intent("SHADOW", "intent.shadow.price-collar.001");
        far_limit.order_type = OrderType::Limit;
        far_limit.limit_price = Some(amount("11"));
        let outcome = service
            .record_shadow_intent(
                far_limit,
                market(),
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("shadow rejection evidence");
        let LiveSubmitOutcome::ShadowRecorded { decision } = outcome else {
            panic!("shadow mode must retain a shadow decision");
        };
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"PRICE_COLLAR_EXCEEDED".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("requested_price_deviation_bps=1000.00000000"));
        assert_eq!(service.broker_mut().submitted, 0);
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn controlled_live_self_trade_risk_rejects_an_opposite_side_canary_order() {
        let path = journal_path("self-trade");
        let mut service = test_service(LiveRunMode::Canary, &path);
        let buy_intent = intent("LIVE", "intent.live.self-trade.buy");
        let mut buy_approval = approval_for(&service, &buy_intent);
        buy_approval.approval_id = "approval.live.self-trade.buy".to_owned();
        service
            .register_approval(
                buy_approval,
                "2026-01-02T14:30:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let buy_outcome = service
            .submit_canary_intent(
                buy_intent,
                market(),
                "approval.live.self-trade.buy",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        assert!(matches!(
            buy_outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));

        let mut sell_intent = intent("LIVE", "intent.live.self-trade.sell");
        sell_intent.side = Side::Sell;
        let mut sell_approval = approval_for(&service, &sell_intent);
        sell_approval.approval_id = "approval.live.self-trade.sell".to_owned();
        service
            .register_approval(
                sell_approval,
                "2026-01-02T14:30:01Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let sell_outcome = service
            .submit_canary_intent(
                sell_intent,
                market(),
                "approval.live.self-trade.sell",
                "2026-01-02T14:30:01Z",
                "operator.requester.001",
            )
            .expect("risk evaluation completes");
        let LiveSubmitOutcome::RiskRejected { decision } = sell_outcome else {
            panic!("an opposite-side order against a resting order must be rejected");
        };
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"SELF_TRADE_RISK".to_owned()));
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn controlled_live_order_rate_limit_rejects_submissions_beyond_the_configured_window() {
        let path = journal_path("order-rate");
        let account = account();
        let mut rate_limited_policy = policy();
        rate_limited_policy.max_order_rate = 2;
        rate_limited_policy.canary_max_orders = 10;
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation = activation(
            LiveRunMode::Canary,
            &account,
            &rate_limited_policy,
            &switches,
        );
        let mut service = LiveTradingService::open_durable(
            account,
            rate_limited_policy,
            activation,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");

        let first_intent = intent("LIVE", "intent.live.rate.001");
        let mut first_approval = approval_for(&service, &first_intent);
        first_approval.approval_id = "approval.live.rate.001".to_owned();
        service
            .register_approval(
                first_approval,
                "2026-01-02T14:30:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let first = service
            .submit_canary_intent(
                first_intent,
                market(),
                "approval.live.rate.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("first submission");
        assert!(matches!(first, LiveSubmitOutcome::CanaryOrder { .. }));

        let second_intent = intent("LIVE", "intent.live.rate.002");
        let mut second_approval = approval_for(&service, &second_intent);
        second_approval.approval_id = "approval.live.rate.002".to_owned();
        service
            .register_approval(
                second_approval,
                "2026-01-02T14:30:01Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let second = service
            .submit_canary_intent(
                second_intent,
                market(),
                "approval.live.rate.002",
                "2026-01-02T14:30:01Z",
                "operator.requester.001",
            )
            .expect("second submission");
        assert!(matches!(second, LiveSubmitOutcome::CanaryOrder { .. }));

        let third_intent = intent("LIVE", "intent.live.rate.003");
        let mut third_approval = approval_for(&service, &third_intent);
        third_approval.approval_id = "approval.live.rate.003".to_owned();
        service
            .register_approval(
                third_approval,
                "2026-01-02T14:30:02Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let third = service
            .submit_canary_intent(
                third_intent,
                market(),
                "approval.live.rate.003",
                "2026-01-02T14:30:02Z",
                "operator.requester.001",
            )
            .expect("third evaluation completes");
        let LiveSubmitOutcome::RiskRejected { decision } = third else {
            panic!("a submission beyond the configured order rate must be rejected");
        };
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"MAX_ORDER_RATE_EXCEEDED".to_owned()));
        assert!(decision.evaluated_limits.contains("recent_order_count=2"));
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn every_portfolio_risk_limit_is_part_of_the_live_configuration_fingerprint() {
        // Version 1 of the portfolio-risk part omitted these, so a journal
        // reopened, and an approval stayed valid, under any change to them
        // (delivery state E7.4).
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("switches");
        let fingerprint = |change: &dyn Fn(&mut PortfolioRiskComposition)| {
            let mut composition = PortfolioRiskComposition {
                policy: permissive_portfolio_risk_policy(),
                instrument_buckets: BTreeMap::new(),
                margin_rates: None,
            };
            change(&mut composition);
            let mut risk_policy = policy();
            risk_policy.portfolio_risk = Some(composition);
            configuration_fingerprint(&account(), &risk_policy, &switches)
        };
        let base = fingerprint(&|_| {});
        type Change = Box<dyn Fn(&mut PortfolioRiskComposition)>;
        let changes: Vec<(&str, Change)> = vec![
            (
                "max_daily_loss",
                Box::new(|c| c.policy.max_daily_loss = amount("1234")),
            ),
            (
                "max_drawdown_bps",
                Box::new(|c| c.policy.max_drawdown_bps = amount("1234")),
            ),
            (
                "max_margin_utilization_bps",
                Box::new(|c| {
                    c.policy.max_margin_utilization_bps = amount("1234");
                }),
            ),
            (
                "strategy_limits",
                Box::new(|c| {
                    c.policy
                        .strategy_limits
                        .insert("strategy.live.001".to_owned(), amount("1000"));
                }),
            ),
            (
                "margin_rates",
                Box::new(|c| {
                    c.margin_rates = Some(BTreeMap::from([(
                        "equity".to_owned(),
                        follon_accounting::MarginRate {
                            initial_bps: 5_000,
                            maintenance_bps: 2_500,
                        },
                    )]));
                }),
            ),
        ];
        for (limit, change) in &changes {
            assert_ne!(
                fingerprint(change.as_ref()),
                base,
                "{limit} is not in the fingerprint"
            );
        }
    }

    #[test]
    fn live_portfolio_risk_composition_rejects_when_gross_exposure_limit_is_exceeded() {
        let path = journal_path("portfolio-risk-gross");
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy.max_gross_exposure = amount("10");
        let mut risk_policy = policy();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let mut service = test_service_with_policy(LiveRunMode::Shadow, &path, risk_policy);
        let outcome = service
            .record_shadow_intent(
                intent("SHADOW", "intent.shadow.portfolio-risk-gross.001"),
                market(),
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("shadow decision");
        let LiveSubmitOutcome::ShadowRecorded { decision } = outcome else {
            panic!("shadow mode must retain a shadow decision");
        };
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"MAX_GROSS_EXPOSURE_EXCEEDED".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("portfolio_gross_exposure=20.00000000"));
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn live_portfolio_risk_composition_rejects_when_sector_bucket_limit_is_exceeded() {
        let path = journal_path("portfolio-risk-sector");
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy
            .sector_limits
            .insert("index".to_owned(), amount("10"));
        let mut instrument_buckets = BTreeMap::new();
        instrument_buckets.insert(
            "inst.us_equity.spy".to_owned(),
            InstrumentBucket {
                asset_class: "equity".to_owned(),
                currency: "USD".to_owned(),
                sector: "index".to_owned(),
            },
        );
        let mut risk_policy = policy();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets,
            margin_rates: None,
        });
        let mut service = test_service_with_policy(LiveRunMode::Shadow, &path, risk_policy);
        let outcome = service
            .record_shadow_intent(
                intent("SHADOW", "intent.shadow.portfolio-risk-sector.001"),
                market(),
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("shadow decision");
        let LiveSubmitOutcome::ShadowRecorded { decision } = outcome else {
            panic!("shadow mode must retain a shadow decision");
        };
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"SECTOR_LIMIT_EXCEEDED:index".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("portfolio_sector_gross=index:20.00000000"));
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn live_portfolio_risk_composition_rejects_a_restricted_instrument() {
        let path = journal_path("portfolio-risk-restricted");
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy
            .restricted_instruments
            .insert("inst.us_equity.spy".to_owned());
        let mut risk_policy = policy();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let mut service = test_service_with_policy(LiveRunMode::Shadow, &path, risk_policy);
        let outcome = service
            .record_shadow_intent(
                intent("SHADOW", "intent.shadow.portfolio-risk-restricted.001"),
                market(),
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("shadow decision");
        let LiveSubmitOutcome::ShadowRecorded { decision } = outcome else {
            panic!("shadow mode must retain a shadow decision");
        };
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"RESTRICTED_INSTRUMENT".to_owned()));
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn live_portfolio_risk_composition_rejects_when_drawdown_limit_is_exceeded() {
        let path = journal_path("portfolio-risk-drawdown");
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy.max_drawdown_bps = amount("2000");
        // Never trip on leverage; this test only wants drawdown exercised.
        portfolio_policy.max_leverage_bps = amount("1000000");
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = amount("1000");
        risk_policy.max_order_notional = amount("200000");
        risk_policy.canary_max_order_notional = amount("200000");
        risk_policy.max_position_quantity = amount("2000");
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let big_account = LiveAccount {
            initial_cash: amount("100000"),
            max_deployed_capital: amount("100000"),
            ..account()
        };
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation = activation(LiveRunMode::Canary, &big_account, &risk_policy, &switches);
        let mut service = LiveTradingService::open_durable(
            big_account,
            risk_policy,
            activation,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service");

        // Buy and fill 1,000 shares at 100 -- equity moves from 100,000 cash
        // into a position of equal value, establishing a 100,000 peak.
        let order_intent = OrderIntent {
            quantity: amount("1000"),
            ..intent("LIVE", "intent.live.drawdown.001")
        };
        let approval = approval_for(&service, &order_intent);
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let outcome = service
            .submit_canary_intent(
                order_intent,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                },
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        assert!(matches!(
            outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let broker = service.broker_mut();
        broker.events.push(LiveBrokerEvent::Execution {
            execution_id: "execution.live.drawdown.001".to_owned(),
            client_order_id: "order-intent.live.drawdown.001".to_owned(),
            broker_order_id: "broker-order-intent.live.drawdown.001".to_owned(),
            quantity: amount("1000"),
            price: amount("100"),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:00Z".to_owned(),
        });
        broker.snapshot.orders[0].state = OrderState::Filled;
        broker.snapshot.orders[0].filled_quantity = amount("1000");
        broker.snapshot.positions.push(LiveBrokerPositionSnapshot {
            instrument_id: "inst.us_equity.spy".to_owned(),
            quantity: amount("1000"),
        });
        broker.snapshot.cash = Decimal::ZERO;
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
            .expect("broker event synchronization");

        // Sell 1 share at 70: a real, computed 30% drawdown from the
        // 100,000 peak (equity is now 1,000 * 70 = 70,000), which exceeds
        // the configured 20% limit.
        let mut sell = intent("LIVE", "intent.live.drawdown.sell");
        sell.side = Side::Sell;
        sell.quantity = amount("1");
        let sell_approval = LiveApproval {
            approval_id: "approval.live.drawdown.sell".to_owned(),
            ..approval_for(&service, &sell)
        };
        service
            .register_approval(
                sell_approval,
                "2026-01-02T14:32:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let result = service
            .submit_canary_intent(
                sell,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("70"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
                "approval.live.drawdown.sell",
                "2026-01-02T14:32:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = result else {
            panic!("drawdown breach must be risk-rejected");
        };
        assert!(decision
            .reason_codes
            .contains(&"MAX_DRAWDOWN_EXCEEDED".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("portfolio_peak_equity=100000.00000000"));
        assert!(decision
            .evaluated_limits
            .contains("portfolio_drawdown_bps=3000.00000000"));
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn live_portfolio_risk_composition_rejects_when_daily_loss_limit_is_exceeded() {
        let path = journal_path("portfolio-risk-daily-loss");
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy.max_daily_loss = amount("2000");
        // Never trip on leverage; this test only wants daily loss exercised.
        portfolio_policy.max_leverage_bps = amount("1000000");
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = amount("1000");
        risk_policy.max_order_notional = amount("200000");
        risk_policy.canary_max_order_notional = amount("200000");
        risk_policy.max_position_quantity = amount("2000");
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let big_account = LiveAccount {
            initial_cash: amount("100000"),
            max_deployed_capital: amount("100000"),
            ..account()
        };
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation = activation(LiveRunMode::Canary, &big_account, &risk_policy, &switches);
        let mut service = LiveTradingService::open_durable(
            big_account,
            risk_policy,
            activation,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service");

        // Buy and fill 1,000 shares at 100 -- the very first risk evaluation
        // ever establishes the session-start baseline at pure cash equity
        // (100,000; no position exists yet).
        let order_intent = OrderIntent {
            quantity: amount("1000"),
            ..intent("LIVE", "intent.live.daily-loss.001")
        };
        let approval = approval_for(&service, &order_intent);
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let outcome = service
            .submit_canary_intent(
                order_intent,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                },
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        assert!(matches!(
            outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let broker = service.broker_mut();
        broker.events.push(LiveBrokerEvent::Execution {
            execution_id: "execution.live.daily-loss.001".to_owned(),
            client_order_id: "order-intent.live.daily-loss.001".to_owned(),
            broker_order_id: "broker-order-intent.live.daily-loss.001".to_owned(),
            quantity: amount("1000"),
            price: amount("100"),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:00Z".to_owned(),
        });
        broker.snapshot.orders[0].state = OrderState::Filled;
        broker.snapshot.orders[0].filled_quantity = amount("1000");
        broker.snapshot.positions.push(LiveBrokerPositionSnapshot {
            instrument_id: "inst.us_equity.spy".to_owned(),
            quantity: amount("1000"),
        });
        broker.snapshot.cash = Decimal::ZERO;
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
            .expect("broker event synchronization");

        // Sell 1 share at 70 later the same UTC day: real equity falls from
        // the baseline's 100,000 (pure cash, observed before the buy filled)
        // to 70,000 -- a genuine 30,000 daily loss exceeding the configured
        // 2,000 limit.
        let mut sell = intent("LIVE", "intent.live.daily-loss.sell");
        sell.side = Side::Sell;
        sell.quantity = amount("1");
        let sell_approval = LiveApproval {
            approval_id: "approval.live.daily-loss.sell".to_owned(),
            ..approval_for(&service, &sell)
        };
        service
            .register_approval(
                sell_approval,
                "2026-01-02T14:32:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let result = service
            .submit_canary_intent(
                sell,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("70"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
                "approval.live.daily-loss.sell",
                "2026-01-02T14:32:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = result else {
            panic!("daily-loss breach must be risk-rejected");
        };
        assert!(decision
            .reason_codes
            .contains(&"MAX_DAILY_LOSS_EXCEEDED".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("portfolio_daily_baseline_equity=100000.00000000"));
        assert!(decision
            .evaluated_limits
            .contains("portfolio_daily_pnl=-30000.00000000"));
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn live_peak_equity_survives_a_durable_journal_reopen() {
        let path = journal_path("peak-equity");
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = amount("1001");
        risk_policy.max_order_notional = amount("200000");
        risk_policy.canary_max_order_notional = amount("200000");
        risk_policy.max_position_quantity = amount("2000");
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: permissive_portfolio_risk_policy(),
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let big_account = LiveAccount {
            initial_cash: amount("100000"),
            max_deployed_capital: amount("100000"),
            ..account()
        };
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation_record =
            activation(LiveRunMode::Canary, &big_account, &risk_policy, &switches);
        let mut durable = LiveTradingService::open_durable(
            big_account.clone(),
            risk_policy.clone(),
            activation_record,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service");

        // Buy and fill 1,000 shares at 100 -- equity starts at 100,000.
        let order_intent = OrderIntent {
            quantity: amount("1000"),
            ..intent("LIVE", "intent.live.peak.001")
        };
        let approval = approval_for(&durable, &order_intent);
        durable
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        durable
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let outcome = durable
            .submit_canary_intent(
                order_intent,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                },
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        assert!(matches!(
            outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let broker = durable.broker_mut();
        broker.events.push(LiveBrokerEvent::Execution {
            execution_id: "execution.live.peak.001".to_owned(),
            client_order_id: "order-intent.live.peak.001".to_owned(),
            broker_order_id: "broker-order-intent.live.peak.001".to_owned(),
            quantity: amount("1000"),
            price: amount("100"),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:00Z".to_owned(),
        });
        broker.snapshot.orders[0].state = OrderState::Filled;
        broker.snapshot.orders[0].filled_quantity = amount("1000");
        broker.snapshot.positions.push(LiveBrokerPositionSnapshot {
            instrument_id: "inst.us_equity.spy".to_owned(),
            quantity: amount("1000"),
        });
        broker.snapshot.cash = Decimal::ZERO;
        durable
            .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
            .expect("broker event synchronization");

        // Re-quote to 150: a real mark-to-market gain pushes equity to
        // 150,000, raising the peak. This order is rejected on quantity
        // alone, but the peak-equity update is unconditional.
        let mut spike = intent("LIVE", "intent.live.peak.spike");
        spike.quantity = amount("1002");
        let spike_approval = LiveApproval {
            approval_id: "approval.live.peak.spike".to_owned(),
            ..approval_for(&durable, &spike)
        };
        durable
            .register_approval(
                spike_approval,
                "2026-01-02T14:32:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let spiked = durable
            .submit_canary_intent(
                spike,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("150"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
                "approval.live.peak.spike",
                "2026-01-02T14:32:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = spiked else {
            panic!("over-quantity canary submission must be risk-rejected");
        };
        assert!(decision
            .evaluated_limits
            .contains("portfolio_peak_equity=150000.00000000"));

        // Re-quote back down to 100: equity falls back to 100,000, but the
        // peak must not fall with it.
        let mut retreat = intent("LIVE", "intent.live.peak.retreat");
        retreat.quantity = amount("1002");
        let retreat_approval = LiveApproval {
            approval_id: "approval.live.peak.retreat".to_owned(),
            ..approval_for(&durable, &retreat)
        };
        durable
            .register_approval(
                retreat_approval,
                "2026-01-02T14:33:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let retreated = durable
            .submit_canary_intent(
                retreat,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:33:00Z".to_owned(),
                },
                "approval.live.peak.retreat",
                "2026-01-02T14:33:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = retreated else {
            panic!("over-quantity canary submission must be risk-rejected");
        };
        assert!(decision
            .evaluated_limits
            .contains("portfolio_peak_equity=150000.00000000"));
        drop(durable);

        // Reopen: the durable peak (150,000) must survive, not reset to
        // today's current equity (100,000).
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation_record =
            activation(LiveRunMode::Canary, &big_account, &risk_policy, &switches);
        let mut reopened = LiveTradingService::open_durable(
            big_account.clone(),
            risk_policy,
            activation_record,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:34:00Z",
        )
        .expect("reopened test service");
        reopened
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:34:00Z",
            )
            .expect("managed-secret reconnection");
        let mut probe = intent("LIVE", "intent.live.peak.after-reopen");
        probe.quantity = amount("1002");
        let probe_approval = LiveApproval {
            approval_id: "approval.live.peak.after-reopen".to_owned(),
            ..approval_for(&reopened, &probe)
        };
        reopened
            .register_approval(
                probe_approval,
                "2026-01-02T14:34:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let result = reopened
            .submit_canary_intent(
                probe,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:34:00Z".to_owned(),
                },
                "approval.live.peak.after-reopen",
                "2026-01-02T14:34:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = result else {
            panic!("over-quantity canary submission must be risk-rejected");
        };
        assert!(decision
            .evaluated_limits
            .contains("portfolio_peak_equity=150000.00000000"));
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn live_daily_loss_baseline_resets_at_a_new_utc_calendar_day() {
        let path = journal_path("daily-loss-reset");
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = amount("1001");
        risk_policy.max_order_notional = amount("200000");
        risk_policy.canary_max_order_notional = amount("200000");
        risk_policy.max_position_quantity = amount("2000");
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: permissive_portfolio_risk_policy(),
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let big_account = LiveAccount {
            initial_cash: amount("100000"),
            max_deployed_capital: amount("100000"),
            ..account()
        };
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation_record =
            activation(LiveRunMode::Canary, &big_account, &risk_policy, &switches);
        let mut service = LiveTradingService::open_durable(
            big_account,
            risk_policy,
            activation_record,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service");

        // Buy and fill 1,000 shares at 100 -- the very first risk evaluation
        // ever establishes the day-1 baseline at pure cash equity (100,000).
        let order_intent = OrderIntent {
            quantity: amount("1000"),
            ..intent("LIVE", "intent.live.daily-reset.001")
        };
        let approval = approval_for(&service, &order_intent);
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let outcome = service
            .submit_canary_intent(
                order_intent,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                },
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        assert!(matches!(
            outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let broker = service.broker_mut();
        broker.events.push(LiveBrokerEvent::Execution {
            execution_id: "execution.live.daily-reset.001".to_owned(),
            client_order_id: "order-intent.live.daily-reset.001".to_owned(),
            broker_order_id: "broker-order-intent.live.daily-reset.001".to_owned(),
            quantity: amount("1000"),
            price: amount("100"),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:00Z".to_owned(),
        });
        broker.snapshot.orders[0].state = OrderState::Filled;
        broker.snapshot.orders[0].filled_quantity = amount("1000");
        broker.snapshot.positions.push(LiveBrokerPositionSnapshot {
            instrument_id: "inst.us_equity.spy".to_owned(),
            quantity: amount("1000"),
        });
        broker.snapshot.cash = Decimal::ZERO;
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
            .expect("broker event synchronization");

        // Re-quote to 150 later the same UTC day: equity rises to 150,000
        // against the still-standing day-1 baseline of 100,000, a real
        // +50,000 daily gain. This order is rejected on quantity alone, but
        // the mark update (and hence the daily-P&L read) is unconditional.
        let mut spike = intent("LIVE", "intent.live.daily-reset.spike");
        spike.quantity = amount("1002");
        let spike_approval = LiveApproval {
            approval_id: "approval.live.daily-reset.spike".to_owned(),
            ..approval_for(&service, &spike)
        };
        service
            .register_approval(
                spike_approval,
                "2026-01-02T14:32:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let spiked = service
            .submit_canary_intent(
                spike,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("150"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
                "approval.live.daily-reset.spike",
                "2026-01-02T14:32:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = spiked else {
            panic!("over-quantity canary submission must be risk-rejected");
        };
        assert!(decision
            .evaluated_limits
            .contains("portfolio_daily_baseline_equity=100000.00000000"));
        assert!(decision
            .evaluated_limits
            .contains("portfolio_daily_pnl=50000.00000000"));

        // The next UTC calendar day resets the baseline to that day's own
        // first observed equity (150,000, the mark is unchanged), not the
        // prior day's 100,000 -- proving a genuine reset rather than a
        // carried-over accumulation. The approval window is extended past
        // the default same-day expiry so this later-day submission itself
        // is not rejected on approval expiry before reaching risk
        // composition.
        let mut next_day = intent("LIVE", "intent.live.daily-reset.day2");
        next_day.quantity = amount("1002");
        let next_day_approval = LiveApproval {
            approval_id: "approval.live.daily-reset.day2".to_owned(),
            approved_at: "2026-01-03T09:00:00Z".to_owned(),
            expires_at: "2026-01-03T10:00:00Z".to_owned(),
            ..approval_for(&service, &next_day)
        };
        service
            .register_approval(
                next_day_approval,
                "2026-01-03T09:00:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let after_rollover = service
            .submit_canary_intent(
                next_day,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("150"),
                    observed_at: "2026-01-03T09:00:00Z".to_owned(),
                },
                "approval.live.daily-reset.day2",
                "2026-01-03T09:00:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = after_rollover else {
            panic!("over-quantity canary submission must be risk-rejected");
        };
        assert!(decision
            .evaluated_limits
            .contains("portfolio_daily_baseline_equity=150000.00000000"));
        assert!(decision
            .evaluated_limits
            .contains("portfolio_daily_pnl=0.00000000"));
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn live_daily_loss_baseline_survives_a_durable_journal_reopen() {
        let path = journal_path("daily-loss-durability");
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = amount("1001");
        risk_policy.max_order_notional = amount("200000");
        risk_policy.canary_max_order_notional = amount("200000");
        risk_policy.max_position_quantity = amount("2000");
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: permissive_portfolio_risk_policy(),
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let big_account = LiveAccount {
            initial_cash: amount("100000"),
            max_deployed_capital: amount("100000"),
            ..account()
        };
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation_record =
            activation(LiveRunMode::Canary, &big_account, &risk_policy, &switches);
        let mut durable = LiveTradingService::open_durable(
            big_account.clone(),
            risk_policy.clone(),
            activation_record,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service");

        // Buy and fill 1,000 shares at 100 -- the very first risk evaluation
        // ever establishes the baseline at pure cash equity (100,000).
        let order_intent = OrderIntent {
            quantity: amount("1000"),
            ..intent("LIVE", "intent.live.daily-durability.001")
        };
        let approval = approval_for(&durable, &order_intent);
        durable
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        durable
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let outcome = durable
            .submit_canary_intent(
                order_intent,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                },
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        assert!(matches!(
            outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let broker = durable.broker_mut();
        broker.events.push(LiveBrokerEvent::Execution {
            execution_id: "execution.live.daily-durability.001".to_owned(),
            client_order_id: "order-intent.live.daily-durability.001".to_owned(),
            broker_order_id: "broker-order-intent.live.daily-durability.001".to_owned(),
            quantity: amount("1000"),
            price: amount("100"),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:00Z".to_owned(),
        });
        broker.snapshot.orders[0].state = OrderState::Filled;
        broker.snapshot.orders[0].filled_quantity = amount("1000");
        broker.snapshot.positions.push(LiveBrokerPositionSnapshot {
            instrument_id: "inst.us_equity.spy".to_owned(),
            quantity: amount("1000"),
        });
        broker.snapshot.cash = Decimal::ZERO;
        durable
            .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
            .expect("broker event synchronization");

        // Re-quote to 150: equity rises to 150,000 -- a real +50,000 gain
        // against the day's still-standing 100,000 baseline. This order is
        // rejected on quantity alone, but the mark update (and hence the
        // daily-P&L read) is unconditional.
        let mut spike = intent("LIVE", "intent.live.daily-durability.spike");
        spike.quantity = amount("1002");
        let spike_approval = LiveApproval {
            approval_id: "approval.live.daily-durability.spike".to_owned(),
            ..approval_for(&durable, &spike)
        };
        durable
            .register_approval(
                spike_approval,
                "2026-01-02T14:32:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let spiked = durable
            .submit_canary_intent(
                spike,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("150"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
                "approval.live.daily-durability.spike",
                "2026-01-02T14:32:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = spiked else {
            panic!("over-quantity canary submission must be risk-rejected");
        };
        assert!(decision
            .evaluated_limits
            .contains("portfolio_daily_baseline_equity=100000.00000000"));
        drop(durable);

        // Reopen later the same UTC day: the durable baseline (100,000) must
        // survive, not reset to today's current equity (150,000).
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation_record =
            activation(LiveRunMode::Canary, &big_account, &risk_policy, &switches);
        let mut reopened = LiveTradingService::open_durable(
            big_account.clone(),
            risk_policy,
            activation_record,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:34:00Z",
        )
        .expect("reopened test service");
        reopened
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:34:00Z",
            )
            .expect("managed-secret reconnection");
        let mut probe = intent("LIVE", "intent.live.daily-durability.after-reopen");
        probe.quantity = amount("1002");
        let probe_approval = LiveApproval {
            approval_id: "approval.live.daily-durability.after-reopen".to_owned(),
            ..approval_for(&reopened, &probe)
        };
        reopened
            .register_approval(
                probe_approval,
                "2026-01-02T14:34:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let result = reopened
            .submit_canary_intent(
                probe,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("150"),
                    observed_at: "2026-01-02T14:34:00Z".to_owned(),
                },
                "approval.live.daily-durability.after-reopen",
                "2026-01-02T14:34:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = result else {
            panic!("over-quantity canary submission must be risk-rejected");
        };
        assert!(decision
            .evaluated_limits
            .contains("portfolio_daily_baseline_equity=100000.00000000"));
        assert!(decision
            .evaluated_limits
            .contains("portfolio_daily_pnl=50000.00000000"));
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn live_portfolio_risk_composition_rejects_when_margin_utilization_limit_is_exceeded() {
        let path = journal_path("portfolio-risk-margin");
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy.max_margin_utilization_bps = amount("4000");
        // Never trip on leverage; this test only wants margin utilization
        // exercised.
        portfolio_policy.max_leverage_bps = amount("1000000");
        let mut instrument_buckets = BTreeMap::new();
        instrument_buckets.insert(
            "inst.us_equity.spy".to_owned(),
            InstrumentBucket {
                asset_class: "equity".to_owned(),
                currency: "USD".to_owned(),
                sector: "index".to_owned(),
            },
        );
        let mut margin_rates = BTreeMap::new();
        margin_rates.insert(
            "equity".to_owned(),
            follon_accounting::MarginRate {
                initial_bps: 5000,
                maintenance_bps: 2500,
            },
        );
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = amount("1000");
        risk_policy.max_order_notional = amount("200000");
        risk_policy.canary_max_order_notional = amount("200000");
        risk_policy.max_position_quantity = amount("2000");
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets,
            margin_rates: Some(margin_rates),
        });
        let big_account = LiveAccount {
            initial_cash: amount("100000"),
            max_deployed_capital: amount("100000"),
            ..account()
        };
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation = activation(LiveRunMode::Canary, &big_account, &risk_policy, &switches);
        let mut service = LiveTradingService::open_durable(
            big_account,
            risk_policy,
            activation,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service");

        // Buy and fill 1,000 shares at 100 -- cash is fully spent, so equity
        // (100,000) equals the position's mark value exactly.
        let order_intent = OrderIntent {
            quantity: amount("1000"),
            ..intent("LIVE", "intent.live.margin.001")
        };
        let approval = approval_for(&service, &order_intent);
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let outcome = service
            .submit_canary_intent(
                order_intent,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                },
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        assert!(matches!(
            outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let broker = service.broker_mut();
        broker.events.push(LiveBrokerEvent::Execution {
            execution_id: "execution.live.margin.001".to_owned(),
            client_order_id: "order-intent.live.margin.001".to_owned(),
            broker_order_id: "broker-order-intent.live.margin.001".to_owned(),
            quantity: amount("1000"),
            price: amount("100"),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:00Z".to_owned(),
        });
        broker.snapshot.orders[0].state = OrderState::Filled;
        broker.snapshot.orders[0].filled_quantity = amount("1000");
        broker.snapshot.positions.push(LiveBrokerPositionSnapshot {
            instrument_id: "inst.us_equity.spy".to_owned(),
            quantity: amount("1000"),
        });
        broker.snapshot.cash = Decimal::ZERO;
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
            .expect("broker event synchronization");

        // A second order against the same mark: real margin_used is now
        // 100,000 * 50% = 50,000 against equity of 100,000, a genuine 50%
        // utilization exceeding the configured 40% limit.
        let mut sell = intent("LIVE", "intent.live.margin.sell");
        sell.side = Side::Sell;
        sell.quantity = amount("1");
        let sell_approval = LiveApproval {
            approval_id: "approval.live.margin.sell".to_owned(),
            ..approval_for(&service, &sell)
        };
        service
            .register_approval(
                sell_approval,
                "2026-01-02T14:32:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let result = service
            .submit_canary_intent(
                sell,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
                "approval.live.margin.sell",
                "2026-01-02T14:32:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = result else {
            panic!("margin-utilization breach must be risk-rejected");
        };
        assert!(decision
            .reason_codes
            .contains(&"MAX_MARGIN_UTILIZATION_EXCEEDED".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("portfolio_margin_used=50000.00000000"));
        assert!(decision
            .evaluated_limits
            .contains("portfolio_margin_utilization_bps=5000.00000000"));
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn live_portfolio_risk_composition_fails_closed_when_a_held_position_has_no_margin_rate() {
        let path = journal_path("portfolio-risk-margin-gap");
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy.max_leverage_bps = amount("1000000");
        let mut instrument_buckets = BTreeMap::new();
        instrument_buckets.insert(
            "inst.us_equity.spy".to_owned(),
            InstrumentBucket {
                asset_class: "equity".to_owned(),
                currency: "USD".to_owned(),
                sector: "index".to_owned(),
            },
        );
        // A margin_rates map that covers a *different* asset class than the
        // one actually held: `value_margin_account` requires a rate for
        // every asset class among currently held positions, so this must
        // fail closed with a technical error rather than silently treating
        // the uncovered position as zero margin.
        let mut margin_rates = BTreeMap::new();
        margin_rates.insert(
            "option".to_owned(),
            follon_accounting::MarginRate {
                initial_bps: 5000,
                maintenance_bps: 2500,
            },
        );
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = amount("1000");
        risk_policy.max_order_notional = amount("200000");
        risk_policy.canary_max_order_notional = amount("200000");
        risk_policy.max_position_quantity = amount("2000");
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets,
            margin_rates: Some(margin_rates),
        });
        let big_account = LiveAccount {
            initial_cash: amount("100000"),
            max_deployed_capital: amount("100000"),
            ..account()
        };
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation = activation(LiveRunMode::Canary, &big_account, &risk_policy, &switches);
        let mut service = LiveTradingService::open_durable(
            big_account,
            risk_policy,
            activation,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service");
        let order_intent = OrderIntent {
            quantity: amount("1000"),
            ..intent("LIVE", "intent.live.margin-gap.001")
        };
        let approval = approval_for(&service, &order_intent);
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let outcome = service
            .submit_canary_intent(
                order_intent,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                },
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        assert!(matches!(
            outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let broker = service.broker_mut();
        broker.events.push(LiveBrokerEvent::Execution {
            execution_id: "execution.live.margin-gap.001".to_owned(),
            client_order_id: "order-intent.live.margin-gap.001".to_owned(),
            broker_order_id: "broker-order-intent.live.margin-gap.001".to_owned(),
            quantity: amount("1000"),
            price: amount("100"),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:00Z".to_owned(),
        });
        broker.snapshot.orders[0].state = OrderState::Filled;
        broker.snapshot.orders[0].filled_quantity = amount("1000");
        broker.snapshot.positions.push(LiveBrokerPositionSnapshot {
            instrument_id: "inst.us_equity.spy".to_owned(),
            quantity: amount("1000"),
        });
        broker.snapshot.cash = Decimal::ZERO;
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
            .expect("broker event synchronization");

        let mut sell = intent("LIVE", "intent.live.margin-gap.sell");
        sell.side = Side::Sell;
        sell.quantity = amount("1");
        let sell_approval = LiveApproval {
            approval_id: "approval.live.margin-gap.sell".to_owned(),
            ..approval_for(&service, &sell)
        };
        service
            .register_approval(
                sell_approval,
                "2026-01-02T14:32:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let error = service
            .submit_canary_intent(
                sell,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
                "approval.live.margin-gap.sell",
                "2026-01-02T14:32:00Z",
                "operator.requester.001",
            )
            .unwrap_err();
        assert!(error.0.contains("missing margin policy"));
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn live_portfolio_risk_composition_rejects_when_strategy_limit_is_exceeded() {
        let path = journal_path("portfolio-risk-strategy");
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        // Never trip on leverage/concentration/gross; this test only wants
        // the strategy-bucket check exercised.
        portfolio_policy.max_leverage_bps = amount("1000000");
        portfolio_policy.max_gross_exposure = amount("1000000");
        let mut strategy_limits = BTreeMap::new();
        strategy_limits.insert("strategy.beta".to_owned(), amount("25000"));
        portfolio_policy.strategy_limits = strategy_limits;
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = amount("1000");
        risk_policy.max_order_notional = amount("200000");
        risk_policy.canary_max_order_notional = amount("200000");
        risk_policy.max_position_quantity = amount("2000");
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let big_account = LiveAccount {
            initial_cash: amount("100000"),
            max_deployed_capital: amount("100000"),
            ..account()
        };
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation = activation(LiveRunMode::Canary, &big_account, &risk_policy, &switches);
        let mut service = LiveTradingService::open_durable(
            big_account,
            risk_policy,
            activation,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service");

        // Strategy "strategy.live.001" (the default test strategy) buys and
        // fills 100 shares at 100 -- a real, durably attributed 10,000
        // position for that strategy alone.
        let order_intent = OrderIntent {
            quantity: amount("100"),
            ..intent("LIVE", "intent.live.strategy-alpha.001")
        };
        let approval = approval_for(&service, &order_intent);
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let outcome = service
            .submit_canary_intent(
                order_intent,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                },
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        assert!(matches!(
            outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let broker = service.broker_mut();
        broker.events.push(LiveBrokerEvent::Execution {
            execution_id: "execution.live.strategy-alpha.001".to_owned(),
            client_order_id: "order-intent.live.strategy-alpha.001".to_owned(),
            broker_order_id: "broker-order-intent.live.strategy-alpha.001".to_owned(),
            quantity: amount("100"),
            price: amount("100"),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:00Z".to_owned(),
        });
        broker.snapshot.orders[0].state = OrderState::Filled;
        broker.snapshot.orders[0].filled_quantity = amount("100");
        broker.snapshot.positions.push(LiveBrokerPositionSnapshot {
            instrument_id: "inst.us_equity.spy".to_owned(),
            quantity: amount("100"),
        });
        broker.snapshot.cash = amount("99000");
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
            .expect("broker event synchronization");

        // A second, distinct strategy ("strategy.beta") submits a 300-share
        // buy of the *same* instrument at the same mark. Its own candidate
        // notional alone (30,000) already exceeds the configured 25,000
        // strategy.beta limit, independent of strategy.live.001's already-
        // filled 10,000 position -- proving the two strategies are tracked
        // and limited separately, not pooled into one aggregate bucket.
        let mut beta_buy = intent("LIVE", "intent.live.strategy-beta.001");
        beta_buy.strategy_id = "strategy.beta".to_owned();
        beta_buy.quantity = amount("300");
        let beta_approval = LiveApproval {
            approval_id: "approval.live.strategy-beta.001".to_owned(),
            ..approval_for(&service, &beta_buy)
        };
        service
            .register_approval(
                beta_approval,
                "2026-01-02T14:32:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let result = service
            .submit_canary_intent(
                beta_buy,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
                "approval.live.strategy-beta.001",
                "2026-01-02T14:32:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = result else {
            panic!("strategy-limit breach must be risk-rejected");
        };
        assert!(decision
            .reason_codes
            .contains(&"STRATEGY_LIMIT_EXCEEDED:strategy.beta".to_owned()));
        // Total gross exposure reflects both strategies exactly (10,000 from
        // the filled strategy.live.001 position plus 30,000 from the
        // rejected strategy.beta candidate), proving the per-strategy split
        // never mis-states the true aggregate.
        assert!(decision
            .evaluated_limits
            .contains("portfolio_gross_exposure=40000.00000000"));
        assert!(decision
            .evaluated_limits
            .contains("strategy.beta:30000.00000000"));
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn live_strategy_attribution_survives_a_durable_journal_reopen() {
        let path = journal_path("strategy-attribution");
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy.max_leverage_bps = amount("1000000");
        portfolio_policy.max_gross_exposure = amount("1000000");
        let mut strategy_limits = BTreeMap::new();
        strategy_limits.insert("strategy.beta".to_owned(), amount("25000"));
        portfolio_policy.strategy_limits = strategy_limits;
        let mut risk_policy = policy();
        risk_policy.max_order_quantity = amount("1000");
        risk_policy.max_order_notional = amount("200000");
        risk_policy.canary_max_order_notional = amount("200000");
        risk_policy.max_position_quantity = amount("2000");
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let big_account = LiveAccount {
            initial_cash: amount("100000"),
            max_deployed_capital: amount("100000"),
            ..account()
        };
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation_record =
            activation(LiveRunMode::Canary, &big_account, &risk_policy, &switches);
        let mut durable = LiveTradingService::open_durable(
            big_account.clone(),
            risk_policy.clone(),
            activation_record,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service");

        // Strategy "strategy.live.001" buys and fills 100 shares at 100.
        let order_intent = OrderIntent {
            quantity: amount("100"),
            ..intent("LIVE", "intent.live.attribution-durability.alpha")
        };
        let approval = approval_for(&durable, &order_intent);
        durable
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        durable
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let outcome = durable
            .submit_canary_intent(
                order_intent,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                },
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        assert!(matches!(
            outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let broker = durable.broker_mut();
        broker.events.push(LiveBrokerEvent::Execution {
            execution_id: "execution.live.attribution-durability.alpha".to_owned(),
            client_order_id: "order-intent.live.attribution-durability.alpha".to_owned(),
            broker_order_id: "broker-order-intent.live.attribution-durability.alpha".to_owned(),
            quantity: amount("100"),
            price: amount("100"),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:00Z".to_owned(),
        });
        broker.snapshot.orders[0].state = OrderState::Filled;
        broker.snapshot.orders[0].filled_quantity = amount("100");
        broker.snapshot.positions.push(LiveBrokerPositionSnapshot {
            instrument_id: "inst.us_equity.spy".to_owned(),
            quantity: amount("100"),
        });
        broker.snapshot.cash = amount("99000");
        durable
            .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
            .expect("broker event synchronization");
        drop(durable);

        // Reopen: the durable per-strategy attribution (100 shares owned by
        // strategy.live.001) must survive, so a fresh strategy.beta candidate
        // still sees the correct pre-existing gross exposure and its own
        // limit is still evaluated against exactly its own contribution.
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation_record =
            activation(LiveRunMode::Canary, &big_account, &risk_policy, &switches);
        let mut reopened = LiveTradingService::open_durable(
            big_account.clone(),
            risk_policy,
            activation_record,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:32:00Z",
        )
        .expect("reopened test service");
        reopened
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:32:00Z",
            )
            .expect("managed-secret reconnection");
        let mut beta_buy = intent("LIVE", "intent.live.attribution-durability.beta");
        beta_buy.strategy_id = "strategy.beta".to_owned();
        beta_buy.quantity = amount("300");
        let beta_approval = LiveApproval {
            approval_id: "approval.live.attribution-durability.beta".to_owned(),
            ..approval_for(&reopened, &beta_buy)
        };
        reopened
            .register_approval(
                beta_approval,
                "2026-01-02T14:32:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let result = reopened
            .submit_canary_intent(
                beta_buy,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("100"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
                "approval.live.attribution-durability.beta",
                "2026-01-02T14:32:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = result else {
            panic!("strategy-limit breach must be risk-rejected");
        };
        assert!(decision
            .reason_codes
            .contains(&"STRATEGY_LIMIT_EXCEEDED:strategy.beta".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("portfolio_gross_exposure=40000.00000000"));
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn live_portfolio_risk_composition_uses_a_durable_mark_cache_after_journal_reopen() {
        let path = journal_path("portfolio-risk-marks");
        let mut portfolio_policy = permissive_portfolio_risk_policy();
        portfolio_policy.max_gross_exposure = amount("40");
        let mut risk_policy = policy();
        risk_policy.portfolio_risk = Some(PortfolioRiskComposition {
            policy: portfolio_policy,
            instrument_buckets: BTreeMap::new(),
            margin_rates: None,
        });
        let mut service = test_service_with_policy(LiveRunMode::Canary, &path, risk_policy.clone());

        // Buy and fill A (spy) at 10, so its average cost is 10.
        let order_intent = intent("LIVE", "intent.live.marks.001");
        let approval = approval_for(&service, &order_intent);
        service
            .register_approval(approval, "2026-01-02T14:30:00Z", "operator.approver.001")
            .expect("four-eyes approval");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");
        let outcome = service
            .submit_canary_intent(
                order_intent,
                market(),
                "approval.live.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("bounded submission");
        assert!(matches!(
            outcome,
            LiveSubmitOutcome::CanaryOrder {
                state: OrderState::Acknowledged,
                ..
            }
        ));
        let broker = service.broker_mut();
        broker.events.push(LiveBrokerEvent::Execution {
            execution_id: "execution.live.marks.001".to_owned(),
            client_order_id: "order-intent.live.marks.001".to_owned(),
            broker_order_id: "broker-order-intent.live.marks.001".to_owned(),
            quantity: amount("2"),
            price: amount("10"),
            fee: Decimal::ZERO,
            executed_at: "2026-01-02T14:31:00Z".to_owned(),
        });
        broker.snapshot.orders[0].state = OrderState::Filled;
        broker.snapshot.orders[0].filled_quantity = amount("2");
        broker.snapshot.positions.push(LiveBrokerPositionSnapshot {
            instrument_id: "inst.us_equity.spy".to_owned(),
            quantity: amount("2"),
        });
        broker.snapshot.cash = amount("980");
        service
            .synchronize("operator.approver.001", "2026-01-02T14:31:00Z")
            .expect("broker event synchronization");

        // Re-quote A at 25 via a second canary attempt that is rejected on
        // quantity alone -- the mark observation is cached unconditionally
        // before any check runs.
        let mut requote = intent("LIVE", "intent.live.marks.requote");
        requote.quantity = amount("1000");
        let requote_approval = LiveApproval {
            approval_id: "approval.live.marks.requote".to_owned(),
            ..approval_for(&service, &requote)
        };
        service
            .register_approval(
                requote_approval,
                "2026-01-02T14:32:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let requoted = service
            .submit_canary_intent(
                requote,
                LiveMarketData {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    mark_price: amount("25"),
                    observed_at: "2026-01-02T14:32:00Z".to_owned(),
                },
                "approval.live.marks.requote",
                "2026-01-02T14:32:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = requoted else {
            panic!("over-quantity canary submission must be risk-rejected");
        };
        assert!(decision
            .reason_codes
            .contains(&"MAX_ORDER_QUANTITY_EXCEEDED".to_owned()));
        drop(service);

        // Reopen: the cached mark for A (25) must survive and be used, not
        // fall back to its average cost (10).
        let mut reopened = test_service_with_policy(LiveRunMode::Canary, &path, risk_policy);
        reopened
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:33:00Z",
            )
            .expect("managed-secret reconnection");
        let second_intent = OrderIntent {
            instrument_id: "inst.us_equity.qqq".to_owned(),
            quantity: amount("1"),
            ..intent("LIVE", "intent.live.marks.002")
        };
        let second_approval = LiveApproval {
            approval_id: "approval.live.marks.002".to_owned(),
            ..approval_for(&reopened, &second_intent)
        };
        reopened
            .register_approval(
                second_approval,
                "2026-01-02T14:33:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let second_market = LiveMarketData {
            instrument_id: "inst.us_equity.qqq".to_owned(),
            mark_price: amount("10"),
            observed_at: "2026-01-02T14:33:00Z".to_owned(),
        };
        let result = reopened
            .submit_canary_intent(
                second_intent,
                second_market,
                "approval.live.marks.002",
                "2026-01-02T14:33:00Z",
                "operator.requester.001",
            )
            .expect("risk-rejected outcome");
        let LiveSubmitOutcome::RiskRejected { decision } = result else {
            panic!("gross-exposure breach must be risk-rejected");
        };
        assert!(decision
            .reason_codes
            .contains(&"MAX_GROSS_EXPOSURE_EXCEEDED".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("portfolio_gross_exposure=60.00000000"));
        drop(reopened);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn controlled_live_order_rate_limit_counts_decision_time_not_caller_supplied_created_at() {
        let path = journal_path("order-rate-backdate");
        let account = account();
        let mut rate_limited_policy = policy();
        rate_limited_policy.max_order_rate = 2;
        rate_limited_policy.canary_max_orders = 10;
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation = activation(
            LiveRunMode::Canary,
            &account,
            &rate_limited_policy,
            &switches,
        );
        let mut service = LiveTradingService::open_durable(
            account,
            rate_limited_policy,
            activation,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:00:00Z",
        )
        .expect("test service");
        service
            .connect(
                &TestSecrets,
                "operator.approver.001",
                "2026-01-02T14:30:00Z",
            )
            .expect("managed-secret connection");

        // Every intent claims a `created_at` far outside the rate window, but each
        // is actually decided within it: a caller must not be able to understate
        // its own submission rate and bypass `MAX_ORDER_RATE_EXCEEDED` by
        // backdating the intent's self-reported `created_at`.
        let mut first_intent = intent("LIVE", "intent.live.rate.backdate.001");
        first_intent.created_at = "2020-01-01T00:00:00Z".to_owned();
        let mut first_approval = approval_for(&service, &first_intent);
        first_approval.approval_id = "approval.live.rate.backdate.001".to_owned();
        service
            .register_approval(
                first_approval,
                "2026-01-02T14:30:00Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let first = service
            .submit_canary_intent(
                first_intent,
                market(),
                "approval.live.rate.backdate.001",
                "2026-01-02T14:30:00Z",
                "operator.requester.001",
            )
            .expect("first submission");
        assert!(matches!(first, LiveSubmitOutcome::CanaryOrder { .. }));

        let mut second_intent = intent("LIVE", "intent.live.rate.backdate.002");
        second_intent.created_at = "2020-01-01T00:00:00Z".to_owned();
        let mut second_approval = approval_for(&service, &second_intent);
        second_approval.approval_id = "approval.live.rate.backdate.002".to_owned();
        service
            .register_approval(
                second_approval,
                "2026-01-02T14:30:01Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let second = service
            .submit_canary_intent(
                second_intent,
                market(),
                "approval.live.rate.backdate.002",
                "2026-01-02T14:30:01Z",
                "operator.requester.001",
            )
            .expect("second submission");
        assert!(matches!(second, LiveSubmitOutcome::CanaryOrder { .. }));

        let mut third_intent = intent("LIVE", "intent.live.rate.backdate.003");
        third_intent.created_at = "2020-01-01T00:00:00Z".to_owned();
        let mut third_approval = approval_for(&service, &third_intent);
        third_approval.approval_id = "approval.live.rate.backdate.003".to_owned();
        service
            .register_approval(
                third_approval,
                "2026-01-02T14:30:02Z",
                "operator.approver.001",
            )
            .expect("four-eyes approval");
        let third = service
            .submit_canary_intent(
                third_intent,
                market(),
                "approval.live.rate.backdate.003",
                "2026-01-02T14:30:02Z",
                "operator.requester.001",
            )
            .expect("third evaluation completes");
        let LiveSubmitOutcome::RiskRejected { decision } = third else {
            panic!("a submission beyond the configured order rate must be rejected");
        };
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"MAX_ORDER_RATE_EXCEEDED".to_owned()));
        drop(service);
        std::fs::remove_file(path).expect("remove test journal");
    }

    #[test]
    fn tampered_audit_journal_refuses_recovery() {
        let path = journal_path("tampered");
        let service = test_service(LiveRunMode::Shadow, &path);
        drop(service);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open test journal")
            .write_all(b"{not-json}\n")
            .expect("append tamper evidence");
        let account = account();
        let policy = policy();
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation = activation(LiveRunMode::Shadow, &account, &policy, &switches);
        assert!(LiveTradingService::open_durable(
            account,
            policy,
            activation,
            switches,
            TestBroker::new(),
            &path,
            "2026-01-02T14:31:00Z",
        )
        .is_err());
        std::fs::remove_file(path).expect("remove test journal");
    }

    /// Links `link` to `target`, which need not exist. Returns false where
    /// this account cannot create a symbolic link, such as Windows without
    /// Developer Mode, after saying so.
    fn symlink_to(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(target, link);
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(target, link);
        if let Err(error) = &linked {
            eprintln!("cannot create a symbolic link ({error}); the refusal was not exercised");
        }
        linked.is_ok()
    }

    #[test]
    fn a_live_journal_refuses_a_symbolic_link_even_a_dangling_one() {
        let target = journal_path("link-target");
        let link = journal_path("link");
        let _ = fs::remove_file(&target);
        let _ = fs::remove_file(&link);
        if !symlink_to(&target, &link) {
            return;
        }
        let refusal = "live audit journal path must not be a symbolic link";

        // Nothing exists at the target, so following the link finds nothing.
        // Opening must not create the journal there.
        assert_eq!(LiveAuditJournal::open(&link).err().unwrap().0, refusal);
        assert!(!target.exists(), "the journal was created through the link");
        let account = account();
        let policy = policy();
        let switches = LiveKillSwitchRegistry::new("live-kills-v1").expect("test switches");
        let activation = activation(LiveRunMode::Shadow, &account, &policy, &switches);
        assert_eq!(
            LiveTradingService::open_durable(
                account,
                policy,
                activation,
                switches,
                TestBroker::new(),
                &link,
                "2026-01-02T14:31:00Z",
            )
            .err()
            .unwrap()
            .0,
            refusal
        );
        assert!(!target.exists(), "the journal was created through the link");

        // A link to a real journal is refused too.
        drop(test_service(LiveRunMode::Shadow, &target));
        assert_eq!(LiveAuditJournal::open(&link).err().unwrap().0, refusal);
        fs::remove_file(&link).unwrap();
        fs::remove_file(&target).unwrap();
    }

    #[test]
    fn test_controlled_live_news_safety_kernel() {
        // 1. Four-eyes approval validation
        let approval = LiveNewsCanaryApproval {
            approval_id: "app.001".to_owned(),
            requested_by: "trader.alice".to_owned(),
            approved_by: "risk.bob".to_owned(),
            news_event_id: "news.dj.001".to_owned(),
            intent_id: "intent.001".to_owned(),
            signed_at: "2026-09-01T14:00:00Z".to_owned(),
        };
        assert!(approval.validate().is_ok());
        assert!(approval.signature_hash().is_ok());

        // Four-eyes failure (requester == approver)
        let same_operator = LiveNewsCanaryApproval {
            approved_by: "trader.alice".to_owned(),
            ..approval.clone()
        };
        assert!(same_operator.validate().is_err());

        // 2. Macro blackout window
        let blackout = LiveMacroBlackoutWindow {
            window_id: "window.cpi.001".to_owned(),
            event_label: "US CPI Release".to_owned(),
            starts_at: "2026-09-01T13:28:00Z".to_owned(),
            ends_at: "2026-09-01T13:32:00Z".to_owned(),
        };
        assert!(blackout.validate().is_ok());
        assert!(blackout.is_active_at("2026-09-01T13:30:00Z"));
        assert!(!blackout.is_active_at("2026-09-01T14:00:00Z"));

        // 3. Live news shock shield evaluation
        let clean_shield = evaluate_live_news_shock_shield(
            Decimal::from_str("100").unwrap(),
            Decimal::from_str("100.20").unwrap(),
            Decimal::from_str("50").unwrap(),
            Some(Decimal::from_str("0.04").unwrap()),
            Some(Decimal::from_str("0.02").unwrap()),
            Some(Decimal::from_str("30000").unwrap()),
        )
        .expect("shield");
        assert!(clean_shield.is_empty());

        let shock_shield = evaluate_live_news_shock_shield(
            Decimal::from_str("100").unwrap(),
            Decimal::from_str("101.00").unwrap(), // 100 BPS > 50 BPS max
            Decimal::from_str("50").unwrap(),
            Some(Decimal::from_str("0.10").unwrap()), // 5.0x > 3.0x max
            Some(Decimal::from_str("0.02").unwrap()),
            Some(Decimal::from_str("30000").unwrap()),
        )
        .expect("shield");
        assert!(shock_shield.contains(&"LIVE_NEWS_SLIPPAGE_COLLAR_EXCEEDED".to_owned()));
        assert!(shock_shield.contains(&"LIVE_LIQUIDITY_HOLE_DETECTED".to_owned()));
    }
}
