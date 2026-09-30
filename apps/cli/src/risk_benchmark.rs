//! Reproducible local risk-decision latency benchmark.
//!
//! This command deliberately measures a frozen, explicitly supplied portfolio
//! policy/snapshot/candidate.  It produces a machine-readable observation, not
//! an availability claim or a substitute for production SLO monitoring.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Instant;

use follon_cli::{sha256_text, write_immutable};
use follon_domain::{validate_utc_timestamp, Decimal, Side};
use follon_risk::{
    evaluate_portfolio_risk, CandidateOrder, PortfolioRiskDecision, PortfolioRiskPolicy,
    PortfolioRiskSnapshot, RestingOrder, RiskPosition,
};
use serde::Deserialize;
use time::OffsetDateTime;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkDocument {
    schema_version: u32,
    observed_at: String,
    warmup_iterations: u32,
    measured_iterations: u32,
    threshold_micros: u64,
    policy: PolicyDocument,
    snapshot: SnapshotDocument,
    candidate: CandidateDocument,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyDocument {
    version: String,
    global_kill_switch: bool,
    max_gross_exposure: String,
    max_abs_net_exposure: String,
    max_leverage_bps: String,
    max_concentration_bps: String,
    max_daily_loss: String,
    max_drawdown_bps: String,
    max_margin_utilization_bps: String,
    max_abs_delta: String,
    max_abs_gamma: String,
    max_open_orders: usize,
    max_order_rate: u32,
    allowed_instruments: Vec<String>,
    restricted_instruments: Vec<String>,
    sector_limits: BTreeMap<String, String>,
    asset_class_limits: BTreeMap<String, String>,
    currency_limits: BTreeMap<String, String>,
    strategy_limits: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotDocument {
    equity: String,
    peak_equity: String,
    daily_pnl: String,
    margin_used: String,
    positions: Vec<PositionDocument>,
    resting_orders: Vec<RestingOrderDocument>,
    recent_order_count: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PositionDocument {
    account_id: String,
    strategy_id: String,
    instrument_id: String,
    asset_class: String,
    sector: String,
    currency: String,
    quantity: String,
    mark_price: String,
    multiplier: String,
    delta: String,
    gamma: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestingOrderDocument {
    order_id: String,
    account_id: String,
    instrument_id: String,
    side: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateDocument {
    intent_id: String,
    account_id: String,
    strategy_id: String,
    instrument_id: String,
    asset_class: String,
    sector: String,
    currency: String,
    side: String,
    quantity: String,
    mark_price: String,
    multiplier: String,
    delta: String,
    gamma: String,
}

struct BenchmarkInput {
    /// The frozen scenario's own as-of time, echoed from the input document. It
    /// is not when the benchmark ran.
    scenario_observed_at: String,
    warmup_iterations: u32,
    measured_iterations: u32,
    threshold_micros: u64,
    policy: PortfolioRiskPolicy,
    snapshot: PortfolioRiskSnapshot,
    candidate: CandidateOrder,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw_args: Vec<String> = env::args().skip(1).collect();
    if let Some(subcommand) = raw_args.first() {
        if subcommand == "capital-proposal" {
            return run_capital_proposal(&raw_args[1..]);
        }
    }
    let (input_path, output_path) = parse_arguments(raw_args)?;
    let source = fs::read(&input_path)?;
    if source.is_empty() || source.len() > 5 * 1024 * 1024 {
        return Err("risk benchmark input must be between 1 byte and 5 MiB".into());
    }
    let document: BenchmarkDocument = serde_json::from_slice(&source)?;
    let input = parse_document(document)?;
    let expected = evaluate_portfolio_risk(&input.policy, &input.snapshot, Some(&input.candidate))?;
    // When this run began measuring. The scenario's own time is a separate
    // field, because a frozen scenario stays the same however often it is run.
    let measured_at = utc_now();
    for _ in 0..input.warmup_iterations {
        let decision =
            evaluate_portfolio_risk(&input.policy, &input.snapshot, Some(&input.candidate))?;
        if decision != expected {
            return Err("risk evaluator produced an inconsistent warmup decision".into());
        }
    }
    let mut samples = Vec::with_capacity(input.measured_iterations as usize);
    for _ in 0..input.measured_iterations {
        let started_at = Instant::now();
        let decision =
            evaluate_portfolio_risk(&input.policy, &input.snapshot, Some(&input.candidate))?;
        if decision != expected {
            return Err("risk evaluator produced an inconsistent measured decision".into());
        }
        samples.push(started_at.elapsed().as_micros());
    }
    samples.sort_unstable();
    let p99_index = ((samples.len() * 99).div_ceil(100)).saturating_sub(1);
    let measurement = Measurement {
        min_micros: samples[0],
        p99_micros: samples[p99_index],
        max_micros: *samples.last().expect("validated non-empty measurement set"),
    };
    let artifact = render_artifact(
        &input,
        &sha256_text(&String::from_utf8(source)?),
        &measured_at,
        &measurement,
        &expected,
    )?;
    publish(&output_path, &artifact)?;
    println!("{artifact}");
    eprintln!("risk benchmark: {}", output_path.display());
    Ok(())
}

/// Latencies of the measured iterations, in microseconds.
struct Measurement {
    min_micros: u128,
    p99_micros: u128,
    max_micros: u128,
}

/// Renders the benchmark artifact, schema version 2.
///
/// Version 1 named the scenario's own time `observed_at`, which read as when the
/// benchmark ran (delivery state E7.9). Version 2 records `measured_at`, when
/// the run began, and keeps the scenario's time as `scenario_observed_at`.
fn render_artifact(
    input: &BenchmarkInput,
    input_sha256: &str,
    measured_at: &str,
    measurement: &Measurement,
    decision: &PortfolioRiskDecision,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(format!(
        "{{\"benchmark_schema_version\":2,\"candidate_id\":{},\"input_sha256\":{},\"measured_at\":{},\"measured_iterations\":{},\"measurement\":{{\"max_micros\":{},\"min_micros\":{},\"p99_micros\":{},\"threshold_micros\":{},\"within_threshold\":{}}},\"policy_version\":{},\"risk_decision\":{{\"approved\":{},\"reason_codes\":{}}},\"scenario_observed_at\":{},\"warmup_iterations\":{}}}",
        json_string(&input.candidate.intent_id),
        json_string(input_sha256),
        json_string(measured_at),
        input.measured_iterations,
        measurement.max_micros,
        measurement.min_micros,
        measurement.p99_micros,
        input.threshold_micros,
        measurement.p99_micros <= u128::from(input.threshold_micros),
        json_string(&input.policy.version),
        decision.approved,
        serde_json::to_string(&decision.reason_codes)?,
        json_string(&input.scenario_observed_at),
        input.warmup_iterations,
    ))
}

/// The current UTC time to the second, as `YYYY-MM-DDTHH:MM:SSZ`.
fn utc_now() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

fn parse_arguments(
    arguments: Vec<String>,
) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    if arguments.len() != 2 || arguments.iter().any(|argument| argument.starts_with('-')) {
        return Err(
            "usage: follon-risk-benchmark <risk-benchmark-v1.json> <new-benchmark.json>".into(),
        );
    }
    Ok((PathBuf::from(&arguments[0]), PathBuf::from(&arguments[1])))
}

fn parse_document(
    document: BenchmarkDocument,
) -> Result<BenchmarkInput, Box<dyn std::error::Error>> {
    if document.schema_version != 1 {
        return Err("unsupported risk benchmark schema version".into());
    }
    validate_utc_timestamp("observed_at", &document.observed_at)?;
    if document.warmup_iterations > 10_000
        || !(10..=100_000).contains(&document.measured_iterations)
        || document.threshold_micros == 0
        || document.threshold_micros > 60_000_000
    {
        return Err("invalid risk benchmark iteration or threshold bounds".into());
    }
    Ok(BenchmarkInput {
        scenario_observed_at: document.observed_at,
        warmup_iterations: document.warmup_iterations,
        measured_iterations: document.measured_iterations,
        threshold_micros: document.threshold_micros,
        policy: policy(document.policy)?,
        snapshot: snapshot(document.snapshot)?,
        candidate: candidate(document.candidate)?,
    })
}

fn policy(document: PolicyDocument) -> Result<PortfolioRiskPolicy, Box<dyn std::error::Error>> {
    if has_duplicates(&document.allowed_instruments)
        || has_duplicates(&document.restricted_instruments)
    {
        return Err(
            "risk benchmark instrument permission lists must not contain duplicates".into(),
        );
    }
    Ok(PortfolioRiskPolicy {
        version: document.version,
        global_kill_switch: document.global_kill_switch,
        max_gross_exposure: decimal(&document.max_gross_exposure)?,
        max_abs_net_exposure: decimal(&document.max_abs_net_exposure)?,
        max_leverage_bps: decimal(&document.max_leverage_bps)?,
        max_concentration_bps: decimal(&document.max_concentration_bps)?,
        max_daily_loss: decimal(&document.max_daily_loss)?,
        max_drawdown_bps: decimal(&document.max_drawdown_bps)?,
        max_margin_utilization_bps: decimal(&document.max_margin_utilization_bps)?,
        max_abs_delta: decimal(&document.max_abs_delta)?,
        max_abs_gamma: decimal(&document.max_abs_gamma)?,
        max_open_orders: document.max_open_orders,
        max_order_rate: document.max_order_rate,
        allowed_instruments: document
            .allowed_instruments
            .into_iter()
            .collect::<BTreeSet<_>>(),
        restricted_instruments: document
            .restricted_instruments
            .into_iter()
            .collect::<BTreeSet<_>>(),
        sector_limits: decimal_map(document.sector_limits)?,
        asset_class_limits: decimal_map(document.asset_class_limits)?,
        currency_limits: decimal_map(document.currency_limits)?,
        strategy_limits: decimal_map(document.strategy_limits)?,
        max_news_slippage_bps: None,
        max_spread_multiplier_bps: None,
    })
}

fn has_duplicates(values: &[String]) -> bool {
    values.iter().collect::<BTreeSet<_>>().len() != values.len()
}

fn snapshot(
    document: SnapshotDocument,
) -> Result<PortfolioRiskSnapshot, Box<dyn std::error::Error>> {
    Ok(PortfolioRiskSnapshot {
        equity: decimal(&document.equity)?,
        peak_equity: decimal(&document.peak_equity)?,
        daily_pnl: decimal(&document.daily_pnl)?,
        margin_used: decimal(&document.margin_used)?,
        positions: document
            .positions
            .into_iter()
            .map(|position| {
                Ok(RiskPosition {
                    account_id: position.account_id,
                    strategy_id: position.strategy_id,
                    instrument_id: position.instrument_id,
                    asset_class: position.asset_class,
                    sector: position.sector,
                    currency: position.currency,
                    quantity: decimal(&position.quantity)?,
                    mark_price: decimal(&position.mark_price)?,
                    multiplier: decimal(&position.multiplier)?,
                    delta: decimal(&position.delta)?,
                    gamma: decimal(&position.gamma)?,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
        resting_orders: document
            .resting_orders
            .into_iter()
            .map(|order| {
                Ok(RestingOrder {
                    order_id: order.order_id,
                    account_id: order.account_id,
                    instrument_id: order.instrument_id,
                    side: side(&order.side)?,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
        recent_order_count: document.recent_order_count,
    })
}

fn candidate(document: CandidateDocument) -> Result<CandidateOrder, Box<dyn std::error::Error>> {
    Ok(CandidateOrder {
        intent_id: document.intent_id,
        account_id: document.account_id,
        strategy_id: document.strategy_id,
        instrument_id: document.instrument_id,
        asset_class: document.asset_class,
        sector: document.sector,
        currency: document.currency,
        side: side(&document.side)?,
        quantity: decimal(&document.quantity)?,
        mark_price: decimal(&document.mark_price)?,
        multiplier: decimal(&document.multiplier)?,
        delta: decimal(&document.delta)?,
        gamma: decimal(&document.gamma)?,
    })
}

fn side(value: &str) -> Result<Side, Box<dyn std::error::Error>> {
    match value {
        "BUY" => Ok(Side::Buy),
        "SELL" => Ok(Side::Sell),
        _ => Err("side must be BUY or SELL".into()),
    }
}

fn decimal(value: &str) -> Result<Decimal, Box<dyn std::error::Error>> {
    Ok(Decimal::from_str(value)?)
}

fn decimal_map(
    values: BTreeMap<String, String>,
) -> Result<BTreeMap<String, Decimal>, Box<dyn std::error::Error>> {
    values
        .into_iter()
        .map(|(key, value)| Ok((key, decimal(&value)?)))
        .collect()
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization cannot fail")
}

fn publish(path: &Path, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_immutable(path, contents)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapitalProposalConfigDocument {
    total_equity_usd: String,
    strategy_ids: Vec<String>,
    target_annual_volatility_bps: u32,
    max_drawdown_limit_bps: u32,
    policy_version: String,
    proposed_at: String,
}

fn run_capital_proposal(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if arguments.is_empty() || arguments.len() > 2 {
        return Err(
            "usage: follon-risk-benchmark capital-proposal <config.json> [output.json]".into(),
        );
    }
    let input_path = PathBuf::from(&arguments[0]);
    let output_path = arguments
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("var/capital-proposal.json"));

    let content = fs::read_to_string(&input_path)?;
    let doc: CapitalProposalConfigDocument = serde_json::from_str(&content)?;
    validate_utc_timestamp("proposed_at", &doc.proposed_at)?;

    let total_equity = Decimal::from_str(&doc.total_equity_usd)?;
    let strategy_id_refs: Vec<&str> = doc.strategy_ids.iter().map(String::as_str).collect();

    let proposal = follon_risk::CapitalAllocationCouncil::build_erc_proposal(
        total_equity,
        &strategy_id_refs,
        doc.target_annual_volatility_bps,
        doc.max_drawdown_limit_bps,
        &doc.policy_version,
        &doc.proposed_at,
    )?;

    let json = proposal.to_json();
    publish(&output_path, &json)?;
    eprintln!("capital allocation proposal: {}", output_path.display());
    eprintln!("proposal id: {}", proposal.proposal_id);
    eprintln!("allocations: {}", proposal.allocations.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unbounded_or_implicit_benchmark_configuration() {
        assert!(parse_arguments(Vec::new()).is_err());
        assert!(parse_arguments(vec!["benchmark.json".to_owned()]).is_err());
        assert!(parse_document(BenchmarkDocument {
            schema_version: 1,
            observed_at: "2026-08-30T21:30:00Z".to_owned(),
            warmup_iterations: 0,
            measured_iterations: 9,
            threshold_micros: 5_000,
            policy: PolicyDocument {
                version: "risk.v1".to_owned(),
                global_kill_switch: false,
                max_gross_exposure: "1".to_owned(),
                max_abs_net_exposure: "1".to_owned(),
                max_leverage_bps: "1".to_owned(),
                max_concentration_bps: "1".to_owned(),
                max_daily_loss: "0".to_owned(),
                max_drawdown_bps: "0".to_owned(),
                max_margin_utilization_bps: "0".to_owned(),
                max_abs_delta: "0".to_owned(),
                max_abs_gamma: "0".to_owned(),
                max_open_orders: 1,
                max_order_rate: 1,
                allowed_instruments: Vec::new(),
                restricted_instruments: Vec::new(),
                sector_limits: BTreeMap::new(),
                asset_class_limits: BTreeMap::new(),
                currency_limits: BTreeMap::new(),
                strategy_limits: BTreeMap::new(),
            },
            snapshot: SnapshotDocument {
                equity: "1".to_owned(),
                peak_equity: "1".to_owned(),
                daily_pnl: "0".to_owned(),
                margin_used: "0".to_owned(),
                positions: Vec::new(),
                resting_orders: Vec::new(),
                recent_order_count: 0
            },
            candidate: CandidateDocument {
                intent_id: "intent.1".to_owned(),
                account_id: "account.1".to_owned(),
                strategy_id: "strategy.1".to_owned(),
                instrument_id: "instrument.1".to_owned(),
                asset_class: "equity".to_owned(),
                sector: "technology".to_owned(),
                currency: "USD".to_owned(),
                side: "BUY".to_owned(),
                quantity: "1".to_owned(),
                mark_price: "1".to_owned(),
                multiplier: "1".to_owned(),
                delta: "0".to_owned(),
                gamma: "0".to_owned()
            },
        })
        .is_err());
    }

    fn fixture_input() -> BenchmarkInput {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config/risk-benchmark-v1.json");
        parse_document(serde_json::from_slice(&fs::read(fixture).unwrap()).unwrap()).unwrap()
    }

    /// The artifact says when it was measured and keeps the frozen scenario's
    /// own time apart, under a name that cannot be read as the run time
    /// (delivery state E7.9).
    #[test]
    fn the_artifact_records_when_it_was_measured_apart_from_the_scenario_time() {
        let input = fixture_input();
        let decision =
            evaluate_portfolio_risk(&input.policy, &input.snapshot, Some(&input.candidate))
                .unwrap();
        let artifact = render_artifact(
            &input,
            &"a".repeat(64),
            "2026-09-29T13:00:01Z",
            &Measurement {
                min_micros: 15,
                p99_micros: 34,
                max_micros: 62,
            },
            &decision,
        )
        .unwrap();
        let document: serde_json::Value = serde_json::from_str(&artifact).unwrap();
        assert_eq!(document["benchmark_schema_version"], 2);
        assert_eq!(document["measured_at"], "2026-09-29T13:00:01Z");
        assert_eq!(document["scenario_observed_at"], "2026-08-30T21:30:00Z");
        assert!(
            document.get("observed_at").is_none(),
            "a field that reads as the run time must not carry the scenario time"
        );
        assert_eq!(document["measurement"]["p99_micros"], 34);
        assert_eq!(document["measurement"]["within_threshold"], true);
    }

    #[test]
    fn the_measurement_time_is_the_current_utc_second() {
        let before = OffsetDateTime::now_utc().unix_timestamp();
        let stamp = utc_now();
        let after = OffsetDateTime::now_utc().unix_timestamp();
        validate_utc_timestamp("measured_at", &stamp).unwrap();
        let measured =
            OffsetDateTime::parse(&stamp, &time::format_description::well_known::Rfc3339)
                .unwrap()
                .unix_timestamp();
        assert!(
            (before..=after).contains(&measured),
            "{stamp} is outside [{before}, {after}]"
        );
    }

    #[test]
    fn capital_proposal_subcommand_processes_fixture_and_emits_valid_json() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config/capital-allocation-v1.json");
        let output_path = std::env::temp_dir().join(format!(
            "follon-capital-proposal-test-{}.json",
            std::process::id()
        ));
        let args = vec![
            fixture.to_str().unwrap().to_owned(),
            output_path.to_str().unwrap().to_owned(),
        ];
        run_capital_proposal(&args).unwrap();
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("\"proposal_schema_version\":1"));
        assert!(content.contains("\"strat.alpha.trend\""));
        assert!(content.contains("\"proposal_status\":\"RECOMMENDED\""));
        let _ = std::fs::remove_file(&output_path);
    }
}
