//! Deterministic execution-management algorithms.
//!
//! This crate converts an already risk-approved parent order into auditable
//! child instructions. It never contacts a broker and cannot bypass OMS/risk.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use follon_domain::{price_deviation_bps, validate_canonical_id, Decimal, Side};

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

/// Broker-neutral parent request accepted only after a risk approval.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParentOrder {
    /// Stable idempotency identity.
    pub parent_order_id: String,
    /// Account selected by the approved intent.
    pub account_id: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Economic side.
    pub side: Side,
    /// Total positive quantity.
    pub quantity: Decimal,
    /// Optional parent limit inherited by child orders.
    pub limit_price: Option<Decimal>,
}

impl ParentOrder {
    /// Validates broker-neutral identity and economics.
    pub fn validate(&self) -> Result<(), ExecutionError> {
        validate_canonical_id("parent_order_id", &self.parent_order_id)?;
        validate_canonical_id("account_id", &self.account_id)?;
        validate_canonical_id("instrument_id", &self.instrument_id)?;
        if self.quantity <= Decimal::ZERO
            || self.limit_price.is_some_and(|price| price <= Decimal::ZERO)
        {
            return Err(ExecutionError(
                "parent order requires positive quantity and price".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Child instruction type understood by an adapter mapping layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildOrderKind {
    /// Immediately executable market order.
    Market,
    /// Price-protected limit order.
    Limit,
    /// Triggered stop order.
    Stop,
    /// Triggered order with a post-trigger limit.
    StopLimit,
}

/// One immutable child instruction in an execution plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildInstruction {
    /// Parent-derived idempotency identity.
    pub child_order_id: String,
    /// Zero-based scheduling offset.
    pub scheduled_after_seconds: u64,
    /// Venue identity, if smart routing selected one.
    pub venue: Option<String>,
    /// Positive child quantity.
    pub quantity: Decimal,
    /// Adapter-neutral order kind.
    pub kind: ChildOrderKind,
    /// Optional price carried from parent or routing protection.
    pub limit_price: Option<Decimal>,
    /// Optional trigger price for stop/bracket behavior.
    pub stop_price: Option<Decimal>,
}

/// A complete deterministic result; child plus unallocated quantity equals the parent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionPlan {
    /// Parent identity.
    pub parent_order_id: String,
    /// Stable algorithm/version label.
    pub algorithm: String,
    /// Ordered child instructions.
    pub children: Vec<ChildInstruction>,
    /// Quantity not scheduled because observed liquidity was insufficient.
    pub unallocated_quantity: Decimal,
}

impl ExecutionPlan {
    /// Proves quantity conservation and valid child identities.
    pub fn validate_against(&self, parent: &ParentOrder) -> Result<(), ExecutionError> {
        if self.parent_order_id != parent.parent_order_id || self.algorithm.is_empty() {
            return Err(ExecutionError(
                "execution plan identity mismatch".to_owned(),
            ));
        }
        let mut total = self.unallocated_quantity;
        if total < Decimal::ZERO {
            return Err(ExecutionError(
                "unallocated execution quantity cannot be negative".to_owned(),
            ));
        }
        let mut prior_offset = 0;
        for (index, child) in self.children.iter().enumerate() {
            validate_canonical_id("child_order_id", &child.child_order_id)?;
            if child.quantity <= Decimal::ZERO
                || (index > 0 && child.scheduled_after_seconds < prior_offset)
                || child
                    .limit_price
                    .is_some_and(|price| price <= Decimal::ZERO)
                || child.stop_price.is_some_and(|price| price <= Decimal::ZERO)
            {
                return Err(ExecutionError("invalid child instruction".to_owned()));
            }
            prior_offset = child.scheduled_after_seconds;
            total = total.checked_add(child.quantity)?;
        }
        if total != parent.quantity {
            return Err(ExecutionError(
                "execution plan does not conserve parent quantity".to_owned(),
            ));
        }
        Ok(())
    }
}

/// One normalized execution used exclusively for transaction-cost analysis.
///
/// The record is independent of a broker wire format. It cannot create, amend,
/// or reconcile an order; it only measures a completed or partial execution
/// against frozen benchmarks supplied at parent-order arrival.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TcaFill {
    /// Immutable execution identity.
    pub execution_id: String,
    /// Positive executed quantity.
    pub quantity: Decimal,
    /// Positive executed unit price.
    pub price: Decimal,
    /// Non-negative all-in fee in the parent settlement currency.
    pub fee: Decimal,
}

/// Immutable input to one parent-order transaction-cost measurement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionCostInput {
    /// Stable analysis identity; one completed report is idempotent per ID.
    pub analysis_id: String,
    /// Strategy version identity responsible for the parent order.
    pub strategy_id: String,
    /// Parent-order identity from the OMS/EMS boundary.
    pub parent_order_id: String,
    /// Stable EMS algorithm/version label, for example `twap-v1`.
    pub execution_algorithm: String,
    /// Canonical order-type bucket, for example `market` or `limit`.
    pub order_type: String,
    /// Economic side of the parent order.
    pub side: Side,
    /// Fresh independent mark captured before the parent was released.
    pub arrival_price: Decimal,
    /// Frozen EMS benchmark price, such as the planned TWAP or VWAP target.
    pub target_price: Decimal,
    /// Positive parent quantity authorized by OMS/risk.
    pub requested_quantity: Decimal,
    /// Immutable normalized fills observed for the parent so far.
    pub fills: Vec<TcaFill>,
}

/// Exact per-parent implementation-shortfall evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionCostReport {
    /// Input analysis identity.
    pub analysis_id: String,
    /// Strategy identity.
    pub strategy_id: String,
    /// Parent-order identity.
    pub parent_order_id: String,
    /// EMS algorithm/version bucket.
    pub execution_algorithm: String,
    /// Order-type bucket.
    pub order_type: String,
    /// Parent side.
    pub side: Side,
    /// Frozen arrival benchmark.
    pub arrival_price: Decimal,
    /// Frozen algorithm benchmark.
    pub target_price: Decimal,
    /// Authorized parent quantity.
    pub requested_quantity: Decimal,
    /// Quantity with confirmed fills.
    pub filled_quantity: Decimal,
    /// Quantity that remains unfilled at this measurement point.
    pub unfilled_quantity: Decimal,
    /// Exact fill-weighted execution price; absent only when no fills exist.
    pub execution_vwap: Option<Decimal>,
    /// Sum of all attributed broker/exchange/regulatory fees.
    pub fees: Decimal,
    /// Signed adverse price cost versus arrival, excluding fees. Positive is
    /// worse for the selected side; negative is price improvement.
    pub arrival_price_cost: Decimal,
    /// Arrival price cost in exact basis points over filled arrival notional.
    pub arrival_price_cost_bps: Decimal,
    /// Signed adverse price cost versus the EMS target, excluding fees.
    pub target_price_cost: Decimal,
    /// Target price cost in exact basis points over filled target notional.
    pub target_price_cost_bps: Decimal,
    /// Arrival implementation shortfall including all attributed fees.
    pub arrival_total_cost: Decimal,
    /// Target implementation shortfall including all attributed fees.
    pub target_total_cost: Decimal,
}

/// One deterministic aggregate slice across parent-order TCA reports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionCostBucket {
    /// Strategy identity used for grouping.
    pub strategy_id: String,
    /// EMS algorithm/version used for grouping.
    pub execution_algorithm: String,
    /// Order-type bucket used for grouping.
    pub order_type: String,
    /// Number of included parent orders.
    pub order_count: u64,
    /// Sum of confirmed fills.
    pub filled_quantity: Decimal,
    /// Sum of attributed fees.
    pub fees: Decimal,
    /// Sum of signed arrival price cost before fees.
    pub arrival_price_cost: Decimal,
    /// Sum of signed target price cost before fees.
    pub target_price_cost: Decimal,
    /// Sum of arrival implementation shortfall including fees.
    pub arrival_total_cost: Decimal,
    /// Sum of target implementation shortfall including fees.
    pub target_total_cost: Decimal,
}

/// A stable batch of individual reports and review-ready aggregate slices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionCostBatch {
    /// Each requested parent-order measurement in analysis-ID order.
    pub reports: Vec<TransactionCostReport>,
    /// Aggregate evidence sorted by strategy, algorithm, then order type.
    pub buckets: Vec<TransactionCostBucket>,
}

/// Measures a parent order against frozen arrival and EMS benchmarks.
///
/// A positive cost is adverse for both buys and sells. The function refuses to
/// infer an arrival or target price from later data, which prevents TCA
/// look-ahead and makes the result suitable for immutable daily evidence.
pub fn analyze_transaction_cost(
    input: &TransactionCostInput,
) -> Result<TransactionCostReport, ExecutionError> {
    for (name, value) in [
        ("TCA analysis_id", input.analysis_id.as_str()),
        ("TCA strategy_id", input.strategy_id.as_str()),
        ("TCA parent_order_id", input.parent_order_id.as_str()),
        (
            "TCA execution_algorithm",
            input.execution_algorithm.as_str(),
        ),
        ("TCA order_type", input.order_type.as_str()),
    ] {
        validate_canonical_id(name, value)?;
    }
    if input.requested_quantity <= Decimal::ZERO
        || input.arrival_price <= Decimal::ZERO
        || input.target_price <= Decimal::ZERO
        || input.fills.len() > 100_000
    {
        return Err(ExecutionError(
            "transaction-cost input has invalid economics or exceeds bounded fills".to_owned(),
        ));
    }

    let mut execution_ids = BTreeSet::new();
    let mut filled_quantity = Decimal::ZERO;
    let mut execution_notional = Decimal::ZERO;
    let mut fees = Decimal::ZERO;
    for fill in &input.fills {
        validate_canonical_id("TCA execution_id", &fill.execution_id)?;
        if !execution_ids.insert(&fill.execution_id)
            || fill.quantity <= Decimal::ZERO
            || fill.price <= Decimal::ZERO
            || fill.fee < Decimal::ZERO
        {
            return Err(ExecutionError(
                "transaction-cost fills must be unique and economically valid".to_owned(),
            ));
        }
        filled_quantity = filled_quantity.checked_add(fill.quantity)?;
        execution_notional =
            execution_notional.checked_add(fill.price.checked_mul(fill.quantity)?)?;
        fees = fees.checked_add(fill.fee)?;
    }
    if filled_quantity > input.requested_quantity {
        return Err(ExecutionError(
            "transaction-cost fills exceed the authorized parent quantity".to_owned(),
        ));
    }
    let unfilled_quantity = input.requested_quantity.checked_sub(filled_quantity)?;
    let execution_vwap = if filled_quantity == Decimal::ZERO {
        None
    } else {
        Some(execution_notional.checked_div(filled_quantity)?)
    };
    let arrival_price_cost = benchmark_cost(
        input.side,
        input.arrival_price,
        execution_notional,
        filled_quantity,
    )?;
    let target_price_cost = benchmark_cost(
        input.side,
        input.target_price,
        execution_notional,
        filled_quantity,
    )?;
    let arrival_notional = input.arrival_price.checked_mul(filled_quantity)?;
    let target_notional = input.target_price.checked_mul(filled_quantity)?;
    Ok(TransactionCostReport {
        analysis_id: input.analysis_id.clone(),
        strategy_id: input.strategy_id.clone(),
        parent_order_id: input.parent_order_id.clone(),
        execution_algorithm: input.execution_algorithm.clone(),
        order_type: input.order_type.clone(),
        side: input.side,
        arrival_price: input.arrival_price,
        target_price: input.target_price,
        requested_quantity: input.requested_quantity,
        filled_quantity,
        unfilled_quantity,
        execution_vwap,
        fees,
        arrival_price_cost,
        arrival_price_cost_bps: cost_bps(arrival_price_cost, arrival_notional)?,
        target_price_cost,
        target_price_cost_bps: cost_bps(target_price_cost, target_notional)?,
        arrival_total_cost: arrival_price_cost.checked_add(fees)?,
        target_total_cost: target_price_cost.checked_add(fees)?,
    })
}

/// Produces per-order and strategy/algorithm/order-type TCA slices.
pub fn analyze_transaction_costs(
    inputs: &[TransactionCostInput],
) -> Result<TransactionCostBatch, ExecutionError> {
    if inputs.is_empty() || inputs.len() > 100_000 {
        return Err(ExecutionError(
            "transaction-cost batch requires between one and 100000 parent orders".to_owned(),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut reports = Vec::with_capacity(inputs.len());
    let mut bucket_totals: BTreeMap<(String, String, String), TransactionCostBucket> =
        BTreeMap::new();
    for input in inputs {
        if !seen.insert(&input.analysis_id) {
            return Err(ExecutionError(
                "transaction-cost batch contains duplicate analysis_id".to_owned(),
            ));
        }
        let report = analyze_transaction_cost(input)?;
        let bucket = bucket_totals
            .entry((
                report.strategy_id.clone(),
                report.execution_algorithm.clone(),
                report.order_type.clone(),
            ))
            .or_insert_with(|| TransactionCostBucket {
                strategy_id: report.strategy_id.clone(),
                execution_algorithm: report.execution_algorithm.clone(),
                order_type: report.order_type.clone(),
                order_count: 0,
                filled_quantity: Decimal::ZERO,
                fees: Decimal::ZERO,
                arrival_price_cost: Decimal::ZERO,
                target_price_cost: Decimal::ZERO,
                arrival_total_cost: Decimal::ZERO,
                target_total_cost: Decimal::ZERO,
            });
        bucket.order_count = bucket
            .order_count
            .checked_add(1)
            .ok_or_else(|| ExecutionError("transaction-cost order count overflowed".to_owned()))?;
        bucket.filled_quantity = bucket.filled_quantity.checked_add(report.filled_quantity)?;
        bucket.fees = bucket.fees.checked_add(report.fees)?;
        bucket.arrival_price_cost = bucket
            .arrival_price_cost
            .checked_add(report.arrival_price_cost)?;
        bucket.target_price_cost = bucket
            .target_price_cost
            .checked_add(report.target_price_cost)?;
        bucket.arrival_total_cost = bucket
            .arrival_total_cost
            .checked_add(report.arrival_total_cost)?;
        bucket.target_total_cost = bucket
            .target_total_cost
            .checked_add(report.target_total_cost)?;
        reports.push(report);
    }
    reports.sort_by(|left, right| left.analysis_id.cmp(&right.analysis_id));
    Ok(TransactionCostBatch {
        reports,
        buckets: bucket_totals.into_values().collect(),
    })
}

impl TransactionCostBatch {
    /// Stable JSON evidence with fixed-point numbers encoded as strings.
    pub fn canonical_json(&self) -> String {
        let reports = self
            .reports
            .iter()
            .map(TransactionCostReport::canonical_json)
            .collect::<Vec<_>>()
            .join(",");
        let buckets = self
            .buckets
            .iter()
            .map(|bucket| {
                format!(
                    "{{\"arrival_price_cost\":\"{}\",\"arrival_total_cost\":\"{}\",\"execution_algorithm\":{},\"fees\":\"{}\",\"filled_quantity\":\"{}\",\"order_count\":{},\"order_type\":{},\"strategy_id\":{},\"target_price_cost\":\"{}\",\"target_total_cost\":\"{}\"}}",
                    bucket.arrival_price_cost,
                    bucket.arrival_total_cost,
                    json_string(&bucket.execution_algorithm),
                    bucket.fees,
                    bucket.filled_quantity,
                    bucket.order_count,
                    json_string(&bucket.order_type),
                    json_string(&bucket.strategy_id),
                    bucket.target_price_cost,
                    bucket.target_total_cost,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"buckets\":[{}],\"reports\":[{}],\"transaction_cost_schema_version\":1}}",
            buckets, reports
        )
    }

    /// Compact review pack for execution-quality and daily risk discussion.
    /// The complete immutable order-level evidence remains in [`Self::canonical_json`];
    /// this human-facing view caps tables so a large session cannot turn a daily
    /// control review into an unbounded report.
    pub fn markdown_report(&self) -> String {
        const MAX_REVIEW_ROWS: usize = 12;
        let mut report = String::from(
            "# Follon Transaction-Cost Analysis\n\n## Aggregate execution quality\n\n| Strategy | Algorithm | Order type | Orders | Filled quantity | Arrival cost | Target cost | Fees | Arrival total | Target total |\n| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
        );
        for bucket in self.buckets.iter().take(MAX_REVIEW_ROWS) {
            report.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                bucket.strategy_id,
                bucket.execution_algorithm,
                bucket.order_type,
                bucket.order_count,
                bucket.filled_quantity,
                bucket.arrival_price_cost,
                bucket.target_price_cost,
                bucket.fees,
                bucket.arrival_total_cost,
                bucket.target_total_cost,
            ));
        }
        if self.buckets.len() > MAX_REVIEW_ROWS {
            report.push_str(&format!(
                "\n_{} additional aggregate bucket(s) are retained in the canonical JSON artifact._\n",
                self.buckets.len() - MAX_REVIEW_ROWS,
            ));
        }
        report.push_str("\n## Parent-order detail\n\n| Analysis | Parent | Side | Requested | Filled | Unfilled | Execution VWAP | Arrival bps | Target bps | Fees |\n| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
        for item in self.reports.iter().take(MAX_REVIEW_ROWS) {
            report.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                item.analysis_id,
                item.parent_order_id,
                side_label(item.side),
                item.requested_quantity,
                item.filled_quantity,
                item.unfilled_quantity,
                item.execution_vwap
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "N/A".to_owned()),
                item.arrival_price_cost_bps,
                item.target_price_cost_bps,
                item.fees,
            ));
        }
        if self.reports.len() > MAX_REVIEW_ROWS {
            report.push_str(&format!(
                "\n_{} additional parent-order report(s) are retained in the canonical JSON artifact._\n",
                self.reports.len() - MAX_REVIEW_ROWS,
            ));
        }
        report
    }
}

impl TransactionCostReport {
    fn canonical_json(&self) -> String {
        format!(
            "{{\"analysis_id\":{},\"arrival_price\":\"{}\",\"arrival_price_cost\":\"{}\",\"arrival_price_cost_bps\":\"{}\",\"arrival_total_cost\":\"{}\",\"execution_algorithm\":{},\"execution_vwap\":{},\"fees\":\"{}\",\"filled_quantity\":\"{}\",\"order_type\":{},\"parent_order_id\":{},\"requested_quantity\":\"{}\",\"side\":{},\"strategy_id\":{},\"target_price\":\"{}\",\"target_price_cost\":\"{}\",\"target_price_cost_bps\":\"{}\",\"target_total_cost\":\"{}\",\"unfilled_quantity\":\"{}\"}}",
            json_string(&self.analysis_id),
            self.arrival_price,
            self.arrival_price_cost,
            self.arrival_price_cost_bps,
            self.arrival_total_cost,
            json_string(&self.execution_algorithm),
            self.execution_vwap.map(|value| format!("\"{value}\"")).unwrap_or_else(|| "null".to_owned()),
            self.fees,
            self.filled_quantity,
            json_string(&self.order_type),
            json_string(&self.parent_order_id),
            self.requested_quantity,
            json_string(side_label(self.side)),
            json_string(&self.strategy_id),
            self.target_price,
            self.target_price_cost,
            self.target_price_cost_bps,
            self.target_total_cost,
            self.unfilled_quantity,
        )
    }
}

fn benchmark_cost(
    side: Side,
    benchmark_price: Decimal,
    execution_notional: Decimal,
    filled_quantity: Decimal,
) -> Result<Decimal, ExecutionError> {
    let benchmark_notional = benchmark_price.checked_mul(filled_quantity)?;
    match side {
        Side::Buy => execution_notional
            .checked_sub(benchmark_notional)
            .map_err(Into::into),
        Side::Sell => benchmark_notional
            .checked_sub(execution_notional)
            .map_err(Into::into),
    }
}

fn cost_bps(cost: Decimal, benchmark_notional: Decimal) -> Result<Decimal, ExecutionError> {
    if benchmark_notional == Decimal::ZERO {
        return Ok(Decimal::ZERO);
    }
    Ok(cost
        .checked_mul(Decimal::from_integer(10_000)?)?
        .checked_div(benchmark_notional)?)
}

fn side_label(side: Side) -> &'static str {
    match side {
        Side::Buy => "BUY",
        Side::Sell => "SELL",
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}

/// Deterministic advanced execution algorithm.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionAlgorithm {
    /// One child for the complete quantity.
    Immediate,
    /// Equal fixed-point slices with remainder units distributed deterministically.
    Twap {
        /// Number of slices.
        slice_count: u32,
        /// Seconds between slices.
        interval_seconds: u64,
    },
    /// Forecast-volume-weighted schedule with exact remainder conservation.
    Vwap {
        /// Positive forecast volume for each consecutive window.
        forecast_market_volumes: Vec<Decimal>,
        /// Seconds between forecast windows.
        interval_seconds: u64,
    },
    /// Caps each slice to a configured share of observed market volume.
    Participation {
        /// Participation in basis points, `(0, 10000]`.
        participation_bps: u32,
        /// Deterministic observed volumes for consecutive windows.
        observed_market_volumes: Vec<Decimal>,
        /// Seconds between observation windows.
        interval_seconds: u64,
    },
    /// Arrival-price schedule. Higher urgency deterministically front-loads
    /// quantity while preserving the complete fixed-point parent quantity.
    ArrivalPrice {
        /// Number of child slices.
        slice_count: u32,
        /// Seconds between slices.
        interval_seconds: u64,
        /// Front-loading urgency in basis points, `[0, 10000]`.
        urgency_bps: u32,
    },
}

/// Plans immediate, TWAP, VWAP, participation, or arrival-price execution.
pub fn plan_execution(
    parent: &ParentOrder,
    algorithm: &ExecutionAlgorithm,
) -> Result<ExecutionPlan, ExecutionError> {
    parent.validate()?;
    let kind = if parent.limit_price.is_some() {
        ChildOrderKind::Limit
    } else {
        ChildOrderKind::Market
    };
    let (label, quantities, interval_seconds) = match algorithm {
        ExecutionAlgorithm::Immediate => ("immediate-v1", vec![parent.quantity], 0),
        ExecutionAlgorithm::Twap {
            slice_count,
            interval_seconds,
        } => {
            if *slice_count == 0 || *slice_count > 10_000 || *interval_seconds == 0 {
                return Err(ExecutionError("invalid TWAP configuration".to_owned()));
            }
            (
                "twap-v1",
                split_exact(parent.quantity, *slice_count)?,
                *interval_seconds,
            )
        }
        ExecutionAlgorithm::Vwap {
            forecast_market_volumes,
            interval_seconds,
        } => {
            if forecast_market_volumes.is_empty()
                || forecast_market_volumes.len() > 10_000
                || *interval_seconds == 0
                || forecast_market_volumes
                    .iter()
                    .any(|volume| *volume <= Decimal::ZERO)
            {
                return Err(ExecutionError("invalid VWAP configuration".to_owned()));
            }
            (
                "vwap-v1",
                split_weighted_exact(parent.quantity, forecast_market_volumes)?,
                *interval_seconds,
            )
        }
        ExecutionAlgorithm::Participation {
            participation_bps,
            observed_market_volumes,
            interval_seconds,
        } => {
            if !(1..=10_000).contains(participation_bps)
                || observed_market_volumes.is_empty()
                || observed_market_volumes.len() > 10_000
                || *interval_seconds == 0
                || observed_market_volumes
                    .iter()
                    .any(|volume| *volume < Decimal::ZERO)
            {
                return Err(ExecutionError(
                    "invalid participation configuration".to_owned(),
                ));
            }
            let rate = Decimal::from_integer(i64::from(*participation_bps))?
                .checked_div(Decimal::from_integer(10_000)?)?;
            let mut remaining = parent.quantity;
            let mut scheduled = Vec::new();
            for volume in observed_market_volumes {
                if remaining == Decimal::ZERO {
                    break;
                }
                let capacity = volume.checked_mul(rate)?;
                let quantity = capacity.min(remaining);
                if quantity > Decimal::ZERO {
                    scheduled.push(quantity);
                    remaining = remaining.checked_sub(quantity)?;
                }
            }
            ("participation-v1", scheduled, *interval_seconds)
        }
        ExecutionAlgorithm::ArrivalPrice {
            slice_count,
            interval_seconds,
            urgency_bps,
        } => {
            if *slice_count == 0
                || *slice_count > 10_000
                || *interval_seconds == 0
                || *urgency_bps > 10_000
            {
                return Err(ExecutionError(
                    "invalid arrival-price configuration".to_owned(),
                ));
            }
            let mut weights = Vec::with_capacity(*slice_count as usize);
            for index in 0..*slice_count {
                let remaining_rank = i64::from(*slice_count - index);
                let urgency_weight = i64::from(*urgency_bps)
                    .checked_mul(remaining_rank)
                    .ok_or_else(|| ExecutionError("arrival-price weight overflowed".to_owned()))?;
                let weight = 10_000_i64
                    .checked_add(urgency_weight)
                    .ok_or_else(|| ExecutionError("arrival-price weight overflowed".to_owned()))?;
                weights.push(Decimal::from_integer(weight)?);
            }
            (
                "arrival-price-v1",
                split_weighted_exact(parent.quantity, &weights)?,
                *interval_seconds,
            )
        }
    };
    let mut allocated = Decimal::ZERO;
    let children = quantities
        .into_iter()
        .enumerate()
        .map(|(index, quantity)| {
            allocated = allocated.checked_add(quantity)?;
            let offset = u64::try_from(index)
                .ok()
                .and_then(|value| value.checked_mul(interval_seconds))
                .ok_or_else(|| ExecutionError("execution schedule overflowed".to_owned()))?;
            Ok(ChildInstruction {
                child_order_id: format!("{}.child.{:04}", parent.parent_order_id, index + 1),
                scheduled_after_seconds: offset,
                venue: None,
                quantity,
                kind,
                limit_price: parent.limit_price,
                stop_price: None,
            })
        })
        .collect::<Result<Vec<_>, ExecutionError>>()?;
    let plan = ExecutionPlan {
        parent_order_id: parent.parent_order_id.clone(),
        algorithm: label.to_owned(),
        children,
        unallocated_quantity: parent.quantity.checked_sub(allocated)?,
    };
    plan.validate_against(parent)?;
    Ok(plan)
}

fn split_exact(quantity: Decimal, parts: u32) -> Result<Vec<Decimal>, ExecutionError> {
    let divisor = i128::from(parts);
    let base = quantity.scaled() / divisor;
    let remainder = quantity.scaled() % divisor;
    let mut result = Vec::with_capacity(parts as usize);
    for index in 0..parts {
        let extra = i128::from(index) < remainder;
        let scaled = base
            .checked_add(i128::from(extra))
            .ok_or_else(|| ExecutionError("TWAP slice overflowed".to_owned()))?;
        if scaled > 0 {
            result.push(Decimal::from_scaled(scaled));
        }
    }
    Ok(result)
}

fn split_weighted_exact(
    quantity: Decimal,
    weights: &[Decimal],
) -> Result<Vec<Decimal>, ExecutionError> {
    let total_weight = weights.iter().try_fold(0_i128, |total, weight| {
        total
            .checked_add(weight.scaled())
            .ok_or_else(|| ExecutionError("VWAP weight total overflowed".to_owned()))
    })?;
    if quantity <= Decimal::ZERO || total_weight <= 0 {
        return Err(ExecutionError("invalid VWAP inputs".to_owned()));
    }
    let mut allocated = 0_i128;
    let mut result = Vec::with_capacity(weights.len());
    for (index, weight) in weights.iter().enumerate() {
        let scaled = if index + 1 == weights.len() {
            quantity
                .scaled()
                .checked_sub(allocated)
                .ok_or_else(|| ExecutionError("VWAP allocation overflowed".to_owned()))?
        } else {
            quantity
                .scaled()
                .checked_mul(weight.scaled())
                .and_then(|value| value.checked_div(total_weight))
                .ok_or_else(|| ExecutionError("VWAP allocation overflowed".to_owned()))?
        };
        if scaled <= 0 {
            return Err(ExecutionError(
                "VWAP window would round to zero at configured precision".to_owned(),
            ));
        }
        allocated = allocated
            .checked_add(scaled)
            .ok_or_else(|| ExecutionError("VWAP allocation overflowed".to_owned()))?;
        result.push(Decimal::from_scaled(scaled));
    }
    Ok(result)
}

/// Strict bounded policy for a passive cancel-and-replace sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PassiveRepricePolicy {
    /// Initial passive working price, distinct from the parent's hard collar.
    pub initial_limit_price: Decimal,
    /// Exact venue tick size used to reject unrouteable prices.
    pub tick_size: Decimal,
    /// Maximum adverse movement from the initial price in basis points.
    pub maximum_chase_bps: u32,
    /// Maximum number of replacement orders after the initial child.
    pub maximum_replacements: u32,
    /// Minimum elapsed schedule time between replacements.
    pub minimum_replace_interval_seconds: u64,
}

/// One deterministic top-of-book observation used by passive repricing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PassiveMarketObservation {
    /// Schedule time relative to initial submission.
    pub observed_after_seconds: u64,
    /// Positive best bid.
    pub best_bid: Decimal,
    /// Positive best ask.
    pub best_ask: Decimal,
}

/// Atomic cancel-and-replace instruction; cancel confirmation precedes transmit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CancelReplaceInstruction {
    /// Exact working child that must be cancelled and confirmed first.
    pub cancel_child_order_id: String,
    /// Replacement child; it restates the remaining quantity at a new limit.
    pub replacement: ChildInstruction,
}

/// Complete bounded passive-price plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PassiveRepricePlan {
    /// Initial price-protected child.
    pub initial: ChildInstruction,
    /// Ordered replacements, each contingent on prior cancel confirmation.
    pub replacements: Vec<CancelReplaceInstruction>,
}

/// Plans a passive, monotonic, strictly collared cancel-and-replace sequence.
///
/// The plan never crosses the observed spread, exceeds the parent hard limit,
/// moves away from the market, or schedules simultaneous live child orders.
pub fn plan_passive_repricing(
    parent: &ParentOrder,
    policy: &PassiveRepricePolicy,
    observations: &[PassiveMarketObservation],
) -> Result<PassiveRepricePlan, ExecutionError> {
    parent.validate()?;
    if policy.initial_limit_price <= Decimal::ZERO
        || policy.tick_size <= Decimal::ZERO
        || policy.maximum_chase_bps > 10_000
        || policy.maximum_replacements > 10_000
        || policy.minimum_replace_interval_seconds == 0
        || policy.initial_limit_price.scaled() % policy.tick_size.scaled() != 0
        || observations.is_empty()
        || observations.len() > 100_000
    {
        return Err(ExecutionError(
            "invalid passive repricing policy".to_owned(),
        ));
    }
    if let Some(hard_limit) = parent.limit_price {
        let outside = match parent.side {
            Side::Buy => policy.initial_limit_price > hard_limit,
            Side::Sell => policy.initial_limit_price < hard_limit,
        };
        if outside {
            return Err(ExecutionError(
                "initial passive price violates the parent limit".to_owned(),
            ));
        }
    }

    let initial = ChildInstruction {
        child_order_id: format!("{}.passive.0000", parent.parent_order_id),
        scheduled_after_seconds: 0,
        venue: None,
        quantity: parent.quantity,
        kind: ChildOrderKind::Limit,
        limit_price: Some(policy.initial_limit_price),
        stop_price: None,
    };
    let mut current_id = initial.child_order_id.clone();
    let mut current_price = policy.initial_limit_price;
    let mut prior_observation = None;
    let mut last_replace_at = 0_u64;
    let mut replacements = Vec::new();
    for observation in observations {
        if observation.best_bid <= Decimal::ZERO
            || observation.best_ask <= observation.best_bid
            || observation.best_bid.scaled() % policy.tick_size.scaled() != 0
            || observation.best_ask.scaled() % policy.tick_size.scaled() != 0
            || (prior_observation.is_none()
                && match parent.side {
                    Side::Buy => policy.initial_limit_price >= observation.best_ask,
                    Side::Sell => policy.initial_limit_price <= observation.best_bid,
                })
            || prior_observation.is_some_and(|prior| observation.observed_after_seconds <= prior)
        {
            return Err(ExecutionError(
                "invalid passive market observation".to_owned(),
            ));
        }
        prior_observation = Some(observation.observed_after_seconds);
        if replacements.len() >= policy.maximum_replacements as usize
            || observation.observed_after_seconds
                < last_replace_at
                    .checked_add(policy.minimum_replace_interval_seconds)
                    .ok_or_else(|| ExecutionError("passive schedule overflowed".to_owned()))?
        {
            continue;
        }

        let mut candidate = match parent.side {
            Side::Buy => observation.best_bid,
            Side::Sell => observation.best_ask,
        };
        if let Some(hard_limit) = parent.limit_price {
            candidate = match parent.side {
                Side::Buy => candidate.min(hard_limit),
                Side::Sell => candidate.max(hard_limit),
            };
        }
        let more_aggressive = match parent.side {
            Side::Buy => candidate > current_price && candidate < observation.best_ask,
            Side::Sell => candidate < current_price && candidate > observation.best_bid,
        };
        if !more_aggressive
            || price_deviation_bps(policy.initial_limit_price, candidate)?
                > Decimal::from_integer(i64::from(policy.maximum_chase_bps))?
        {
            continue;
        }
        let replacement_number = replacements.len() + 1;
        let replacement_id = format!("{}.passive.{replacement_number:04}", parent.parent_order_id);
        replacements.push(CancelReplaceInstruction {
            cancel_child_order_id: current_id,
            replacement: ChildInstruction {
                child_order_id: replacement_id.clone(),
                scheduled_after_seconds: observation.observed_after_seconds,
                venue: None,
                quantity: parent.quantity,
                kind: ChildOrderKind::Limit,
                limit_price: Some(candidate),
                stop_price: None,
            },
        });
        current_id = replacement_id;
        current_price = candidate;
        last_replace_at = observation.observed_after_seconds;
    }
    Ok(PassiveRepricePlan {
        initial,
        replacements,
    })
}

/// Venue quote used for deterministic smart routing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VenueQuote {
    /// Canonical venue identity.
    pub venue: String,
    /// Available displayed or approved quantity.
    pub available_quantity: Decimal,
    /// Executable price.
    pub price: Decimal,
    /// Exact fee per unit.
    pub fee_per_unit: Decimal,
    /// Stable latency rank used only after total-price comparison.
    pub latency_rank: u32,
}

/// Allocates quantity to best all-in venues without exceeding displayed liquidity.
pub fn smart_route(
    parent: &ParentOrder,
    quotes: &[VenueQuote],
) -> Result<ExecutionPlan, ExecutionError> {
    parent.validate()?;
    if quotes.is_empty() || quotes.len() > 1_000 {
        return Err(ExecutionError(
            "smart routing requires venue quotes".to_owned(),
        ));
    }
    let mut ordered = quotes.to_vec();
    for quote in &ordered {
        validate_canonical_id("venue", &quote.venue)?;
        if quote.available_quantity <= Decimal::ZERO
            || quote.price <= Decimal::ZERO
            || quote.fee_per_unit < Decimal::ZERO
        {
            return Err(ExecutionError("invalid smart-routing quote".to_owned()));
        }
    }
    ordered.sort_by(|left, right| compare_quotes(parent.side, left, right));
    let mut remaining = parent.quantity;
    let mut children = Vec::new();
    for quote in ordered {
        if remaining == Decimal::ZERO {
            break;
        }
        let quantity = quote.available_quantity.min(remaining);
        let protected = match (parent.side, parent.limit_price) {
            (Side::Buy, Some(limit)) if quote.price > limit => continue,
            (Side::Sell, Some(limit)) if quote.price < limit => continue,
            _ => Some(quote.price),
        };
        let index = children.len() + 1;
        children.push(ChildInstruction {
            child_order_id: format!("{}.route.{index:04}", parent.parent_order_id),
            scheduled_after_seconds: 0,
            venue: Some(quote.venue),
            quantity,
            kind: ChildOrderKind::Limit,
            limit_price: protected,
            stop_price: None,
        });
        remaining = remaining.checked_sub(quantity)?;
    }
    let plan = ExecutionPlan {
        parent_order_id: parent.parent_order_id.clone(),
        algorithm: "smart-router-v1".to_owned(),
        children,
        unallocated_quantity: remaining,
    };
    plan.validate_against(parent)?;
    Ok(plan)
}

fn compare_quotes(side: Side, left: &VenueQuote, right: &VenueQuote) -> Ordering {
    let left_all_in = match side {
        Side::Buy => left.price.checked_add(left.fee_per_unit),
        Side::Sell => left.price.checked_sub(left.fee_per_unit),
    };
    let right_all_in = match side {
        Side::Buy => right.price.checked_add(right.fee_per_unit),
        Side::Sell => right.price.checked_sub(right.fee_per_unit),
    };
    let price_order = match (left_all_in, right_all_in, side) {
        (Ok(left_price), Ok(right_price), Side::Buy) => left_price.cmp(&right_price),
        (Ok(left_price), Ok(right_price), Side::Sell) => right_price.cmp(&left_price),
        _ => Ordering::Equal,
    };
    price_order
        .then_with(|| left.latency_rank.cmp(&right.latency_rank))
        .then_with(|| left.venue.cmp(&right.venue))
}

/// Creates linked profit-taking and stop-loss children for an approved entry.
pub fn bracket_children(
    parent: &ParentOrder,
    take_profit_price: Decimal,
    stop_price: Decimal,
    stop_limit_price: Option<Decimal>,
) -> Result<Vec<ChildInstruction>, ExecutionError> {
    parent.validate()?;
    if take_profit_price <= Decimal::ZERO
        || stop_price <= Decimal::ZERO
        || stop_limit_price.is_some_and(|price| price <= Decimal::ZERO)
    {
        return Err(ExecutionError("invalid bracket prices".to_owned()));
    }
    let exit_kind = if stop_limit_price.is_some() {
        ChildOrderKind::StopLimit
    } else {
        ChildOrderKind::Stop
    };
    Ok(vec![
        ChildInstruction {
            child_order_id: format!("{}.bracket.profit", parent.parent_order_id),
            scheduled_after_seconds: 0,
            venue: None,
            quantity: parent.quantity,
            kind: ChildOrderKind::Limit,
            limit_price: Some(take_profit_price),
            stop_price: None,
        },
        ChildInstruction {
            child_order_id: format!("{}.bracket.stop", parent.parent_order_id),
            scheduled_after_seconds: 0,
            venue: None,
            quantity: parent.quantity,
            kind: exit_kind,
            limit_price: stop_limit_price,
            stop_price: Some(stop_price),
        },
    ])
}

/// Stateful trailing-stop calculation with a monotonic favorable reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrailingStop {
    side: Side,
    trail_bps: u32,
    favorable_reference: Decimal,
    stop_price: Decimal,
}

impl TrailingStop {
    /// Creates a trailing stop for closing a long (`Sell`) or short (`Buy`) position.
    pub fn new(side: Side, trail_bps: u32, initial_mark: Decimal) -> Result<Self, ExecutionError> {
        if !(1..10_000).contains(&trail_bps) || initial_mark <= Decimal::ZERO {
            return Err(ExecutionError("invalid trailing-stop input".to_owned()));
        }
        let stop_price = trailing_price(side, trail_bps, initial_mark)?;
        Ok(Self {
            side,
            trail_bps,
            favorable_reference: initial_mark,
            stop_price,
        })
    }

    /// Advances only on a favorable mark and returns the current stop.
    pub fn update(&mut self, mark: Decimal) -> Result<Decimal, ExecutionError> {
        if mark <= Decimal::ZERO {
            return Err(ExecutionError(
                "trailing-stop mark must be positive".to_owned(),
            ));
        }
        let favorable = match self.side {
            Side::Sell => mark > self.favorable_reference,
            Side::Buy => mark < self.favorable_reference,
        };
        if favorable {
            self.favorable_reference = mark;
            self.stop_price = trailing_price(self.side, self.trail_bps, mark)?;
        }
        Ok(self.stop_price)
    }

    /// Current trigger price.
    pub const fn stop_price(&self) -> Decimal {
        self.stop_price
    }
}

fn trailing_price(
    side: Side,
    trail_bps: u32,
    reference: Decimal,
) -> Result<Decimal, ExecutionError> {
    let distance = reference
        .checked_mul(Decimal::from_integer(i64::from(trail_bps))?)?
        .checked_div(Decimal::from_integer(10_000)?)?;
    match side {
        Side::Sell => Ok(reference.checked_sub(distance)?),
        Side::Buy => Ok(reference.checked_add(distance)?),
    }
}

/// Net-price protection for a synchronized listed-option combination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComboPriceLimit {
    /// Total debit per combination may not exceed this positive amount.
    MaximumDebit(Decimal),
    /// Total credit per combination may not be below this positive amount.
    MinimumCredit(Decimal),
}

/// One ratio leg in a synchronized option combination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionComboLeg {
    /// Canonical option instrument identity.
    pub instrument_id: String,
    /// Economic side for this leg.
    pub side: Side,
    /// Positive integer contracts per combination unit.
    pub ratio: u32,
    /// Positive protected leg price used to prove the net price.
    pub limit_price: Decimal,
}

/// One adapter-neutral child of a synchronized option combination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComboLegInstruction {
    /// Deterministic child identity.
    pub child_order_id: String,
    /// Canonical option identity.
    pub instrument_id: String,
    /// Leg side.
    pub side: Side,
    /// Exact contract quantity after applying the ratio.
    pub quantity: Decimal,
    /// Protected leg price.
    pub limit_price: Decimal,
}

/// Exact synchronized option-combination plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionComboPlan {
    /// Stable combination identity.
    pub combo_id: String,
    /// Positive number of combination units.
    pub combo_quantity: Decimal,
    /// Signed protected net price: positive is debit, negative is credit.
    pub protected_net_price: Decimal,
    /// Complete set of ratio-conserving legs submitted as one atomic group.
    pub legs: Vec<ComboLegInstruction>,
}

/// Plans a synchronized, net-price-protected listed-option combination.
///
/// This does not degrade a combination into independently marketable legs.
/// An adapter must either support an atomic native combination or reject the
/// plan before any child is transmitted.
pub fn plan_option_combo(
    combo_id: &str,
    combo_quantity: Decimal,
    price_limit: ComboPriceLimit,
    legs: &[OptionComboLeg],
) -> Result<OptionComboPlan, ExecutionError> {
    use std::collections::BTreeSet;

    validate_canonical_id("combo_id", combo_id)?;
    if combo_quantity <= Decimal::ZERO || !(2..=16).contains(&legs.len()) {
        return Err(ExecutionError("invalid option combination".to_owned()));
    }
    let mut instruments = BTreeSet::new();
    let mut protected_net_price = Decimal::ZERO;
    let mut instructions = Vec::with_capacity(legs.len());
    for (index, leg) in legs.iter().enumerate() {
        validate_canonical_id("combo instrument_id", &leg.instrument_id)?;
        if !instruments.insert(leg.instrument_id.as_str())
            || leg.ratio == 0
            || leg.ratio > 10_000
            || leg.limit_price <= Decimal::ZERO
        {
            return Err(ExecutionError("invalid option combination leg".to_owned()));
        }
        let ratio = Decimal::from_integer(i64::from(leg.ratio))?;
        let leg_net = leg.limit_price.checked_mul(ratio)?;
        protected_net_price = match leg.side {
            Side::Buy => protected_net_price.checked_add(leg_net)?,
            Side::Sell => protected_net_price.checked_sub(leg_net)?,
        };
        instructions.push(ComboLegInstruction {
            child_order_id: format!("{combo_id}.leg.{:04}", index + 1),
            instrument_id: leg.instrument_id.clone(),
            side: leg.side,
            quantity: combo_quantity.checked_mul(ratio)?,
            limit_price: leg.limit_price,
        });
    }
    match price_limit {
        ComboPriceLimit::MaximumDebit(limit) => {
            if limit <= Decimal::ZERO
                || protected_net_price < Decimal::ZERO
                || protected_net_price > limit
            {
                return Err(ExecutionError(
                    "option combination exceeds maximum debit".to_owned(),
                ));
            }
        }
        ComboPriceLimit::MinimumCredit(limit) => {
            let credit = Decimal::ZERO.checked_sub(protected_net_price)?;
            if limit <= Decimal::ZERO || credit < limit {
                return Err(ExecutionError(
                    "option combination is below minimum credit".to_owned(),
                ));
            }
        }
    }
    Ok(OptionComboPlan {
        combo_id: combo_id.to_owned(),
        combo_quantity,
        protected_net_price,
        legs: instructions,
    })
}

/// One notional-weighted basket leg.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BasketLeg {
    /// Instrument identity.
    pub instrument_id: String,
    /// Leg side.
    pub side: Side,
    /// Weight in basis points; all legs must sum to 10,000.
    pub weight_bps: u32,
    /// Positive reference price for converting notional to quantity.
    pub reference_price: Decimal,
}

/// Converts exact basket notional into independently auditable parent orders.
pub fn plan_basket(
    basket_id: &str,
    account_id: &str,
    total_notional: Decimal,
    legs: &[BasketLeg],
) -> Result<Vec<ParentOrder>, ExecutionError> {
    validate_canonical_id("basket_id", basket_id)?;
    validate_canonical_id("basket account_id", account_id)?;
    if total_notional <= Decimal::ZERO || legs.is_empty() || legs.len() > 1_000 {
        return Err(ExecutionError("invalid basket request".to_owned()));
    }
    let weight_total = legs.iter().try_fold(0_u32, |total, leg| {
        validate_canonical_id("basket instrument_id", &leg.instrument_id)?;
        if leg.weight_bps == 0 || leg.reference_price <= Decimal::ZERO {
            return Err(ExecutionError("invalid basket leg".to_owned()));
        }
        total
            .checked_add(leg.weight_bps)
            .ok_or_else(|| ExecutionError("basket weight overflowed".to_owned()))
    })?;
    if weight_total != 10_000 {
        return Err(ExecutionError(
            "basket weights must sum to 10000 basis points".to_owned(),
        ));
    }
    legs.iter()
        .enumerate()
        .map(|(index, leg)| {
            let leg_notional = total_notional
                .checked_mul(Decimal::from_integer(i64::from(leg.weight_bps))?)?
                .checked_div(Decimal::from_integer(10_000)?)?;
            let parent = ParentOrder {
                parent_order_id: format!("{basket_id}.leg.{:04}", index + 1),
                account_id: account_id.to_owned(),
                instrument_id: leg.instrument_id.clone(),
                side: leg.side,
                quantity: leg_notional.checked_div(leg.reference_price)?,
                limit_price: None,
            };
            parent.validate()?;
            Ok(parent)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn amount(value: &str) -> Decimal {
        Decimal::from_str(value).expect("decimal")
    }

    fn parent(quantity: &str) -> ParentOrder {
        ParentOrder {
            parent_order_id: "parent.exec.001".to_owned(),
            account_id: "account.main".to_owned(),
            instrument_id: "instrument.spy".to_owned(),
            side: Side::Buy,
            quantity: amount(quantity),
            limit_price: Some(amount("100")),
        }
    }

    #[test]
    fn twap_conserves_every_fixed_point_unit() {
        let parent = parent("10.00000001");
        let plan = plan_execution(
            &parent,
            &ExecutionAlgorithm::Twap {
                slice_count: 3,
                interval_seconds: 60,
            },
        )
        .expect("plan");
        assert_eq!(plan.children.len(), 3);
        assert_eq!(plan.unallocated_quantity, Decimal::ZERO);
        assert_eq!(plan.children[2].scheduled_after_seconds, 120);
        plan.validate_against(&parent).expect("conservation");
    }

    #[test]
    fn vwap_follows_forecast_curve_and_conserves_rounding_remainder() {
        let parent = parent("10.00000001");
        let plan = plan_execution(
            &parent,
            &ExecutionAlgorithm::Vwap {
                forecast_market_volumes: vec![amount("1"), amount("2"), amount("1")],
                interval_seconds: 60,
            },
        )
        .expect("plan");
        assert_eq!(plan.children[0].quantity, amount("2.5"));
        assert_eq!(plan.children[1].quantity, amount("5"));
        assert_eq!(plan.children[2].quantity, amount("2.50000001"));
        plan.validate_against(&parent).expect("conservation");
    }

    #[test]
    fn participation_never_exceeds_volume_or_parent() {
        let parent = parent("10");
        let plan = plan_execution(
            &parent,
            &ExecutionAlgorithm::Participation {
                participation_bps: 1_000,
                observed_market_volumes: vec![amount("20"), amount("30")],
                interval_seconds: 30,
            },
        )
        .expect("plan");
        assert_eq!(plan.children[0].quantity, amount("2"));
        assert_eq!(plan.children[1].quantity, amount("3"));
        assert_eq!(plan.unallocated_quantity, amount("5"));
    }

    #[test]
    fn arrival_price_front_loads_and_conserves_every_unit() {
        let parent = parent("10.00000001");
        let plan = plan_execution(
            &parent,
            &ExecutionAlgorithm::ArrivalPrice {
                slice_count: 4,
                interval_seconds: 15,
                urgency_bps: 8_000,
            },
        )
        .expect("arrival plan");
        assert_eq!(plan.algorithm, "arrival-price-v1");
        assert!(plan.children[0].quantity > plan.children[3].quantity);
        assert_eq!(plan.children[3].scheduled_after_seconds, 45);
        plan.validate_against(&parent).expect("conservation");
    }

    #[test]
    fn option_combo_enforces_ratios_and_net_debit_without_leg_risk() {
        let plan = plan_option_combo(
            "combo.vertical.001",
            amount("3"),
            ComboPriceLimit::MaximumDebit(amount("2.50")),
            &[
                OptionComboLeg {
                    instrument_id: "option.spy.500.call".to_owned(),
                    side: Side::Buy,
                    ratio: 1,
                    limit_price: amount("5.00"),
                },
                OptionComboLeg {
                    instrument_id: "option.spy.505.call".to_owned(),
                    side: Side::Sell,
                    ratio: 1,
                    limit_price: amount("2.75"),
                },
            ],
        )
        .expect("protected combo");
        assert_eq!(plan.protected_net_price, amount("2.25"));
        assert_eq!(plan.legs[0].quantity, amount("3"));
        assert!(plan_option_combo(
            "combo.vertical.002",
            amount("1"),
            ComboPriceLimit::MaximumDebit(amount("2")),
            &[
                OptionComboLeg {
                    instrument_id: "option.spy.500.call".to_owned(),
                    side: Side::Buy,
                    ratio: 1,
                    limit_price: amount("5"),
                },
                OptionComboLeg {
                    instrument_id: "option.spy.505.call".to_owned(),
                    side: Side::Sell,
                    ratio: 1,
                    limit_price: amount("2.75"),
                },
            ],
        )
        .is_err());
    }

    #[test]
    fn passive_repricing_is_monotonic_post_only_and_strictly_collared() {
        let mut parent = parent("5");
        parent.limit_price = Some(amount("101"));
        let plan = plan_passive_repricing(
            &parent,
            &PassiveRepricePolicy {
                initial_limit_price: amount("99"),
                tick_size: amount("0.01"),
                maximum_chase_bps: 110,
                maximum_replacements: 4,
                minimum_replace_interval_seconds: 5,
            },
            &[
                PassiveMarketObservation {
                    observed_after_seconds: 5,
                    best_bid: amount("99"),
                    best_ask: amount("100"),
                },
                PassiveMarketObservation {
                    observed_after_seconds: 10,
                    best_bid: amount("100"),
                    best_ask: amount("100.50"),
                },
                PassiveMarketObservation {
                    observed_after_seconds: 15,
                    best_bid: amount("100.50"),
                    best_ask: amount("100.75"),
                },
            ],
        )
        .expect("bounded passive plan");
        assert_eq!(plan.replacements.len(), 1);
        assert_eq!(
            plan.replacements[0].replacement.limit_price,
            Some(amount("100"))
        );
        assert_eq!(
            plan.replacements[0].cancel_child_order_id,
            plan.initial.child_order_id
        );
        assert_eq!(plan.replacements[0].replacement.quantity, parent.quantity);
    }

    #[test]
    fn smart_router_uses_best_all_in_prices_and_respects_limit() {
        let parent = parent("5");
        let plan = smart_route(
            &parent,
            &[
                VenueQuote {
                    venue: "venue.slow".to_owned(),
                    available_quantity: amount("3"),
                    price: amount("99.90"),
                    fee_per_unit: amount("0.05"),
                    latency_rank: 2,
                },
                VenueQuote {
                    venue: "venue.fast".to_owned(),
                    available_quantity: amount("4"),
                    price: amount("99.92"),
                    fee_per_unit: amount("0.01"),
                    latency_rank: 1,
                },
                VenueQuote {
                    venue: "venue.over-limit".to_owned(),
                    available_quantity: amount("5"),
                    price: amount("100.01"),
                    fee_per_unit: Decimal::ZERO,
                    latency_rank: 0,
                },
            ],
        )
        .expect("route");
        assert_eq!(plan.children[0].venue.as_deref(), Some("venue.fast"));
        assert_eq!(plan.children[1].quantity, amount("1"));
        assert_eq!(plan.unallocated_quantity, Decimal::ZERO);
    }

    #[test]
    fn bracket_trailing_and_basket_contracts_are_fail_closed() {
        let parent = parent("2");
        let bracket = bracket_children(&parent, amount("110"), amount("95"), Some(amount("94.50")))
            .expect("bracket");
        assert_eq!(bracket[1].kind, ChildOrderKind::StopLimit);

        let mut trailing = TrailingStop::new(Side::Sell, 500, amount("100")).expect("trail");
        assert_eq!(
            trailing.update(amount("110")).expect("advance"),
            amount("104.5")
        );
        assert_eq!(
            trailing.update(amount("105")).expect("no retreat"),
            amount("104.5")
        );

        let basket = plan_basket(
            "basket.alpha",
            "account.main",
            amount("10000"),
            &[
                BasketLeg {
                    instrument_id: "instrument.spy".to_owned(),
                    side: Side::Buy,
                    weight_bps: 6_000,
                    reference_price: amount("100"),
                },
                BasketLeg {
                    instrument_id: "instrument.tlt".to_owned(),
                    side: Side::Sell,
                    weight_bps: 4_000,
                    reference_price: amount("80"),
                },
            ],
        )
        .expect("basket");
        assert_eq!(basket[0].quantity, amount("60"));
        assert_eq!(basket[1].quantity, amount("50"));
        assert!(plan_basket("basket.bad", "account.main", amount("1"), &[]).is_err());
    }

    #[test]
    fn transaction_cost_analysis_measures_arrival_target_fees_and_partial_fills() {
        let report = analyze_transaction_cost(&TransactionCostInput {
            analysis_id: "tca.alpha.001".to_owned(),
            strategy_id: "strategy.alpha".to_owned(),
            parent_order_id: "parent.alpha.001".to_owned(),
            execution_algorithm: "twap-v1".to_owned(),
            order_type: "limit".to_owned(),
            side: Side::Buy,
            arrival_price: amount("100"),
            target_price: amount("101"),
            requested_quantity: amount("10"),
            fills: vec![
                TcaFill {
                    execution_id: "execution.alpha.001".to_owned(),
                    quantity: amount("4"),
                    price: amount("101"),
                    fee: amount("0.25"),
                },
                TcaFill {
                    execution_id: "execution.alpha.002".to_owned(),
                    quantity: amount("4"),
                    price: amount("102"),
                    fee: amount("0.25"),
                },
            ],
        })
        .expect("TCA report");
        assert_eq!(report.filled_quantity, amount("8"));
        assert_eq!(report.unfilled_quantity, amount("2"));
        assert_eq!(report.execution_vwap, Some(amount("101.5")));
        assert_eq!(report.arrival_price_cost, amount("12"));
        assert_eq!(report.target_price_cost, amount("4"));
        assert_eq!(report.fees, amount("0.5"));
        assert_eq!(report.arrival_total_cost, amount("12.5"));
        assert_eq!(report.target_total_cost, amount("4.5"));
        assert_eq!(report.arrival_price_cost_bps, amount("150"));
        assert_eq!(report.target_price_cost_bps, amount("49.50495049"));
    }

    #[test]
    fn transaction_cost_batch_groups_sides_and_rejects_duplicate_evidence() {
        let buy = TransactionCostInput {
            analysis_id: "tca.alpha.buy".to_owned(),
            strategy_id: "strategy.alpha".to_owned(),
            parent_order_id: "parent.alpha.buy".to_owned(),
            execution_algorithm: "vwap-v1".to_owned(),
            order_type: "market".to_owned(),
            side: Side::Buy,
            arrival_price: amount("100"),
            target_price: amount("100"),
            requested_quantity: amount("1"),
            fills: vec![TcaFill {
                execution_id: "execution.alpha.buy".to_owned(),
                quantity: amount("1"),
                price: amount("101"),
                fee: Decimal::ZERO,
            }],
        };
        let mut sell = buy.clone();
        sell.analysis_id = "tca.alpha.sell".to_owned();
        sell.parent_order_id = "parent.alpha.sell".to_owned();
        sell.side = Side::Sell;
        sell.fills[0].execution_id = "execution.alpha.sell".to_owned();
        sell.fills[0].price = amount("99");
        let batch = analyze_transaction_costs(&[buy.clone(), sell]).expect("grouped TCA");
        assert_eq!(batch.buckets.len(), 1);
        assert_eq!(batch.buckets[0].arrival_price_cost, amount("2"));
        assert!(batch
            .canonical_json()
            .contains("transaction_cost_schema_version"));
        assert!(batch
            .markdown_report()
            .contains("Aggregate execution quality"));

        let mut duplicate = buy;
        duplicate.parent_order_id = "parent.alpha.duplicate".to_owned();
        assert!(analyze_transaction_costs(&[duplicate.clone(), duplicate]).is_err());
    }
}
