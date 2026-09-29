//! Transaction-cost analysis inputs, reports and batch aggregation.

use std::collections::{BTreeMap, BTreeSet};

use follon_domain::{validate_canonical_id, Decimal, Side};

use crate::*;

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

pub(crate) fn side_label(side: Side) -> &'static str {
    match side {
        Side::Buy => "BUY",
        Side::Sell => "SELL",
    }
}

pub(crate) fn order_kind_label(kind: ChildOrderKind) -> &'static str {
    match kind {
        ChildOrderKind::Market => "MARKET",
        ChildOrderKind::Limit => "LIMIT",
        ChildOrderKind::Stop => "STOP",
        ChildOrderKind::StopLimit => "STOP_LIMIT",
    }
}

pub(crate) fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}
