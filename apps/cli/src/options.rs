//! Immutable deterministic options-chain analytics, scenario, and reconciliation evidence.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use follon_cli::{sha256_text, write_immutable};
use follon_domain::{validate_utc_timestamp, Decimal};
use follon_options::{
    analyze_chain, evaluate_expiry_scenarios, reconcile_option_books_at, OptionBook,
    OptionBookPosition, OptionChain, OptionContract, OptionEnvironment, OptionLegSide, OptionQuote,
    OptionRight, OptionRunIdentity, OptionStrategy, OptionStrategyLeg, OPTION_MODEL_VERSION,
};
use serde::Deserialize;

const DEFAULT_CONFIGURATION: &str = "tests/fixtures/config/options-v1.json";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionsConfigurationDocument {
    schema_version: u32,
    reconciled_at: String,
    chain: ChainDocument,
    risk_free_rate: String,
    strategy: StrategyDocument,
    expiry_scenarios: Vec<String>,
    books: BooksDocument,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunIdentityDocument {
    strategy_bundle_hash: String,
    configuration_hash: String,
    dataset_hash: String,
    replay_event_hash: String,
    chain_snapshot_hash: String,
    model_version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChainDocument {
    chain_id: String,
    underlying_instrument_id: String,
    snapshot_at: String,
    underlying_mark: String,
    reference_version: String,
    contracts: Vec<ContractDocument>,
    quotes: Vec<QuoteDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContractDocument {
    option_id: String,
    underlying_instrument_id: String,
    expiration_at: String,
    strike: String,
    right: String,
    multiplier: String,
    currency: String,
    reference_version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuoteDocument {
    option_id: String,
    observed_at: String,
    bid: String,
    ask: String,
    last: String,
    volume: u64,
    open_interest: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrategyDocument {
    strategy_id: String,
    strategy_version: String,
    legs: Vec<LegDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegDocument {
    leg_id: String,
    option_id: String,
    side: String,
    quantity: String,
    entry_premium: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BooksDocument {
    backtest: BookDocument,
    paper: BookDocument,
    live: BookDocument,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BookDocument {
    account_id: String,
    source_export_id: String,
    source_export_hash: String,
    as_of: String,
    currency: String,
    run_identity: RunIdentityDocument,
    cash: String,
    positions: Vec<BookPositionDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BookPositionDocument {
    option_id: String,
    quantity: String,
    average_entry_premium: String,
    mark_premium: String,
    realized_pnl: String,
}

struct RuntimeOptions {
    chain: OptionChain,
    risk_free_rate: Decimal,
    strategy: OptionStrategy,
    scenarios: Vec<Decimal>,
    backtest: OptionBook,
    paper: OptionBook,
    live: OptionBook,
    configuration_hash: String,
    reconciled_at: String,
}

enum Command {
    Validate {
        configuration_path: PathBuf,
    },
    Analyze {
        configuration_path: PathBuf,
        output_path: PathBuf,
    },
    Report {
        configuration_path: PathBuf,
        output_path: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match parse_command(env::args().skip(1).collect())? {
        Command::Validate { configuration_path } => {
            let runtime = load_runtime(&configuration_path)?;
            println!(
                "{{\"chain_snapshot_hash\":{},\"configuration_file_hash\":{},\"model_version\":{},\"valid\":true}}",
                json_string(&runtime.chain.fingerprint()?),
                json_string(&runtime.configuration_hash),
                json_string(OPTION_MODEL_VERSION),
            );
        }
        Command::Analyze {
            configuration_path,
            output_path,
        } => {
            let runtime = load_runtime(&configuration_path)?;
            let dashboard = canonical_dashboard_json(&runtime)?;
            publish(&output_path, &dashboard)?;
            eprintln!("options dashboard: {}", output_path.display());
        }
        Command::Report {
            configuration_path,
            output_path,
        } => {
            let runtime = load_runtime(&configuration_path)?;
            let report = markdown_report(&runtime)?;
            publish(&output_path, &report)?;
            eprintln!("options report: {}", output_path.display());
        }
    }
    Ok(())
}

fn parse_command(arguments: Vec<String>) -> Result<Command, Box<dyn std::error::Error>> {
    let Some((command, remainder)) = arguments.split_first() else {
        return Err(usage().into());
    };
    match command.as_str() {
        "validate-config" => {
            if remainder.len() > 1
                || remainder
                    .first()
                    .is_some_and(|value| value.starts_with('-'))
            {
                return Err("usage: follon-options validate-config [options.json]".into());
            }
            Ok(Command::Validate {
                configuration_path: remainder
                    .first()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIGURATION)),
            })
        }
        "analyze" => {
            let (configuration_path, output_path) =
                parse_artifact_paths(remainder, "var/follon-options-dashboard.json")?;
            Ok(Command::Analyze {
                configuration_path,
                output_path,
            })
        }
        "report" => {
            let (configuration_path, output_path) =
                parse_artifact_paths(remainder, "var/follon-options-report.md")?;
            Ok(Command::Report {
                configuration_path,
                output_path,
            })
        }
        _ => Err(usage().into()),
    }
}

fn parse_artifact_paths(
    arguments: &[String],
    default_output: &str,
) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    if arguments.len() > 2 || arguments.iter().any(|value| value.starts_with('-')) {
        return Err("options analyze/report accepts [options.json] [output]".into());
    }
    Ok((
        arguments
            .first()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIGURATION)),
        arguments
            .get(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(default_output)),
    ))
}

fn load_runtime(path: &Path) -> Result<RuntimeOptions, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err("options configuration must be between 1 byte and 1 MiB".into());
    }
    let source = String::from_utf8(bytes)?;
    let configuration_hash = sha256_text(&source);
    let document: OptionsConfigurationDocument = serde_json::from_str(&source)?;
    if document.schema_version != 1 {
        return Err("unsupported options configuration schema version".into());
    }
    let chain = OptionChain {
        chain_id: document.chain.chain_id,
        underlying_instrument_id: document.chain.underlying_instrument_id,
        snapshot_at: document.chain.snapshot_at,
        underlying_mark: decimal(&document.chain.underlying_mark)?,
        reference_version: document.chain.reference_version,
        contracts: document
            .chain
            .contracts
            .into_iter()
            .map(|contract| {
                Ok(OptionContract {
                    option_id: contract.option_id,
                    underlying_instrument_id: contract.underlying_instrument_id,
                    expiration_at: contract.expiration_at,
                    strike: decimal(&contract.strike)?,
                    right: OptionRight::parse(&contract.right)?,
                    multiplier: decimal(&contract.multiplier)?,
                    currency: contract.currency,
                    reference_version: contract.reference_version,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
        quotes: document
            .chain
            .quotes
            .into_iter()
            .map(|quote| {
                Ok(OptionQuote {
                    option_id: quote.option_id,
                    observed_at: quote.observed_at,
                    bid: decimal(&quote.bid)?,
                    ask: decimal(&quote.ask)?,
                    last: decimal(&quote.last)?,
                    volume: quote.volume,
                    open_interest: quote.open_interest,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
    };
    chain.validate()?;
    validate_utc_timestamp("options reconciled_at", &document.reconciled_at)?;
    if document.reconciled_at.as_str() < chain.snapshot_at.as_str() {
        return Err("options reconciled_at cannot precede the frozen chain snapshot".into());
    }
    let strategy = OptionStrategy {
        strategy_id: document.strategy.strategy_id,
        strategy_version: document.strategy.strategy_version,
        legs: document
            .strategy
            .legs
            .into_iter()
            .map(|leg| {
                Ok(OptionStrategyLeg {
                    leg_id: leg.leg_id,
                    option_id: leg.option_id,
                    side: OptionLegSide::parse(&leg.side)?,
                    quantity: decimal(&leg.quantity)?,
                    entry_premium: decimal(&leg.entry_premium)?,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
    };
    strategy.validate(&chain)?;
    let scenarios = document
        .expiry_scenarios
        .into_iter()
        .map(|value| decimal(&value).map_err(|error| error.into()))
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let backtest = build_book(OptionEnvironment::Backtest, &chain, document.books.backtest)?;
    let paper = build_book(OptionEnvironment::Paper, &chain, document.books.paper)?;
    let live = build_book(OptionEnvironment::Live, &chain, document.books.live)?;
    Ok(RuntimeOptions {
        chain,
        risk_free_rate: decimal(&document.risk_free_rate)?,
        strategy,
        scenarios,
        backtest,
        paper,
        live,
        configuration_hash,
        reconciled_at: document.reconciled_at,
    })
}

fn build_book(
    environment: OptionEnvironment,
    chain: &OptionChain,
    document: BookDocument,
) -> Result<OptionBook, Box<dyn std::error::Error>> {
    let book = OptionBook {
        environment,
        account_id: document.account_id,
        source_export_id: document.source_export_id,
        source_export_hash: document.source_export_hash,
        as_of: document.as_of,
        currency: document.currency,
        cash: decimal(&document.cash)?,
        identity: OptionRunIdentity {
            strategy_bundle_hash: document.run_identity.strategy_bundle_hash,
            configuration_hash: document.run_identity.configuration_hash,
            dataset_hash: document.run_identity.dataset_hash,
            replay_event_hash: document.run_identity.replay_event_hash,
            chain_snapshot_hash: document.run_identity.chain_snapshot_hash,
            model_version: document.run_identity.model_version,
        },
        positions: document
            .positions
            .into_iter()
            .map(|position| {
                Ok(OptionBookPosition {
                    option_id: position.option_id,
                    quantity: decimal(&position.quantity)?,
                    average_entry_premium: decimal(&position.average_entry_premium)?,
                    mark_premium: decimal(&position.mark_premium)?,
                    realized_pnl: decimal(&position.realized_pnl)?,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
    };
    book.validate(chain)?;
    Ok(book)
}

fn canonical_dashboard_json(
    runtime: &RuntimeOptions,
) -> Result<String, Box<dyn std::error::Error>> {
    let analytics = analyze_chain(&runtime.chain, runtime.risk_free_rate)?;
    let scenarios =
        evaluate_expiry_scenarios(&runtime.chain, &runtime.strategy, &runtime.scenarios)?;
    let reconciliation = reconcile_option_books_at(
        &runtime.chain,
        &runtime.backtest,
        &runtime.paper,
        &runtime.live,
        &runtime.reconciled_at,
    )?;
    let analytics = analytics
        .iter()
        .map(|item| {
            let contract = runtime.chain.contract(&item.option_id).expect("validated analytics contract");
            let quote = runtime.chain.quote(&item.option_id).expect("validated analytics quote");
            format!(
                "{{\"ask\":\"{}\",\"bid\":\"{}\",\"delta\":\"{}\",\"expiration_at\":{},\"gamma\":\"{}\",\"implied_volatility\":\"{}\",\"market_premium\":\"{}\",\"model_price\":\"{}\",\"option_id\":{},\"rho\":\"{}\",\"right\":{},\"strike\":\"{}\",\"theta\":\"{}\",\"vega\":\"{}\"}}",
                quote.ask,
                quote.bid,
                item.greeks.delta,
                json_string(&contract.expiration_at),
                item.greeks.gamma,
                item.implied_volatility,
                item.market_premium,
                item.greeks.model_price,
                json_string(&item.option_id),
                item.greeks.rho,
                json_string(contract.right.as_str()),
                contract.strike,
                item.greeks.theta,
                item.greeks.vega,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let scenarios = scenarios
        .iter()
        .map(|scenario| {
            let legs = scenario
                .legs
                .iter()
                .map(|leg| {
                    format!(
                        "{{\"intrinsic_value\":\"{}\",\"leg_id\":{},\"option_id\":{},\"pnl\":\"{}\"}}",
                        leg.intrinsic_value,
                        json_string(&leg.leg_id),
                        json_string(&leg.option_id),
                        leg.pnl,
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"legs\":[{}],\"total_pnl\":\"{}\",\"underlying_price\":\"{}\"}}",
                legs, scenario.total_pnl, scenario.underlying_price
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let reconciliation: serde_json::Value = serde_json::from_str(&reconciliation.canonical_json())?;
    Ok(format!(
        "{{\"analytics\":[{}],\"as_of\":{},\"chain\":{{\"chain_id\":{},\"chain_snapshot_hash\":{},\"currency\":{},\"reference_version\":{},\"underlying_instrument_id\":{},\"underlying_mark\":\"{}\"}},\"configuration_file_hash\":{},\"model_version\":{},\"option_dashboard_schema_version\":1,\"reconciliation\":{},\"run_identity\":{{\"chain_snapshot_hash\":{},\"configuration_hash\":{},\"dataset_hash\":{},\"model_version\":{},\"replay_event_hash\":{},\"strategy_bundle_hash\":{}}},\"strategy\":{{\"scenarios\":[{}],\"strategy_id\":{},\"strategy_version\":{}}}}}",
        analytics,
        json_string(&runtime.chain.snapshot_at),
        json_string(&runtime.chain.chain_id),
        json_string(&runtime.chain.fingerprint()?),
        json_string(&runtime.chain.contracts[0].currency),
        json_string(&runtime.chain.reference_version),
        json_string(&runtime.chain.underlying_instrument_id),
        runtime.chain.underlying_mark,
        json_string(&runtime.configuration_hash),
        json_string(OPTION_MODEL_VERSION),
        serde_json::to_string(&reconciliation)?,
        json_string(&runtime.backtest.identity.chain_snapshot_hash),
        json_string(&runtime.backtest.identity.configuration_hash),
        json_string(&runtime.backtest.identity.dataset_hash),
        json_string(&runtime.backtest.identity.model_version),
        json_string(&runtime.backtest.identity.replay_event_hash),
        json_string(&runtime.backtest.identity.strategy_bundle_hash),
        scenarios,
        json_string(&runtime.strategy.strategy_id),
        json_string(&runtime.strategy.strategy_version),
    ))
}

fn markdown_report(runtime: &RuntimeOptions) -> Result<String, Box<dyn std::error::Error>> {
    let analytics = analyze_chain(&runtime.chain, runtime.risk_free_rate)?;
    let scenarios =
        evaluate_expiry_scenarios(&runtime.chain, &runtime.strategy, &runtime.scenarios)?;
    let reconciliation = reconcile_option_books_at(
        &runtime.chain,
        &runtime.backtest,
        &runtime.paper,
        &runtime.live,
        &runtime.reconciled_at,
    )?;
    let currency = &runtime.chain.contracts[0].currency;
    let mut report = format!(
        "# Follon Options Report\n\n- As of: `{}`\n- Chain: `{}` (`{}`)\n- Underlying: `{}` at {} {}\n- Chain snapshot hash: `{}`\n- Model: `{}`\n- Strategy bundle hash: `{}`\n- Configuration hash: `{}`\n- Dataset hash: `{}`\n- Replay event hash: `{}`\n\n## Implied volatility and Greeks\n\n| Contract | Right | Strike | Mid premium | Implied vol | Delta | Gamma | Vega | Theta | Rho |\n| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
        runtime.chain.snapshot_at,
        runtime.chain.chain_id,
        runtime.chain.reference_version,
        runtime.chain.underlying_instrument_id,
        runtime.chain.underlying_mark,
        currency,
        runtime.chain.fingerprint()?,
        OPTION_MODEL_VERSION,
        runtime.backtest.identity.strategy_bundle_hash,
        runtime.backtest.identity.configuration_hash,
        runtime.backtest.identity.dataset_hash,
        runtime.backtest.identity.replay_event_hash,
    );
    for item in &analytics {
        let contract = runtime
            .chain
            .contract(&item.option_id)
            .expect("validated contract");
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            item.option_id,
            contract.right.as_str(),
            contract.strike,
            item.market_premium,
            item.implied_volatility,
            item.greeks.delta,
            item.greeks.gamma,
            item.greeks.vega,
            item.greeks.theta,
            item.greeks.rho,
        ));
    }
    report.push_str(
        "\n## Expiry scenarios\n\n| Underlying price | Strategy P&L |\n| ---: | ---: |\n",
    );
    for scenario in &scenarios {
        report.push_str(&format!(
            "| {} | {} {} |\n",
            scenario.underlying_price, scenario.total_pnl, currency
        ));
    }
    report.push_str(&format!(
        "\n## Cross-environment reconciliation\n\n- Status: **{}**\n- Reconciled at: `{}`\n- BACKTEST book hash: `{}`; run identity `{}` (account `{}`, source `{}` / `{}`)\n- PAPER book hash: `{}`; run identity `{}` (account `{}`, source `{}` / `{}`)\n- LIVE book hash: `{}`; run identity `{}` (account `{}`, source `{}` / `{}`)\n",
        if reconciliation.is_clean() {
            "CLEAN"
        } else {
            "DIFFERENCES FOUND"
        },
        reconciliation.reconciled_at,
        reconciliation.backtest_book.book_hash,
        reconciliation.backtest_book.run_identity_hash,
        reconciliation.backtest_book.account_id,
        reconciliation.backtest_book.source_export_id,
        reconciliation.backtest_book.source_export_hash,
        reconciliation.paper_book.book_hash,
        reconciliation.paper_book.run_identity_hash,
        reconciliation.paper_book.account_id,
        reconciliation.paper_book.source_export_id,
        reconciliation.paper_book.source_export_hash,
        reconciliation.live_book.book_hash,
        reconciliation.live_book.run_identity_hash,
        reconciliation.live_book.account_id,
        reconciliation.live_book.source_export_id,
        reconciliation.live_book.source_export_hash,
    ));
    if reconciliation.issues.is_empty() {
        report.push_str("- BACKTEST, PAPER, and LIVE books agree on compared cash, positions, marks, realized P&L, and run identity for the bound chain. Their source and book hashes remain environment-specific evidence.\n");
    } else {
        report.push_str(
            "\n| Category | Subject | Expected | Observed |\n| --- | --- | --- | --- |\n",
        );
        for issue in &reconciliation.issues {
            report.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                issue.category, issue.subject, issue.expected, issue.observed
            ));
        }
    }
    Ok(report)
}

fn publish(path: &Path, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_immutable(path, contents)
}

fn decimal(value: &str) -> Result<Decimal, follon_domain::DecimalError> {
    Decimal::from_str(value)
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization cannot fail")
}

fn usage() -> &'static str {
    "usage:\n  follon-options validate-config [options.json]\n  follon-options analyze [options.json] [dashboard.json]\n  follon-options report [options.json] [report.md]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_command_is_strict_and_fixture_is_reproducible() {
        assert!(parse_command(vec!["analyze".to_owned(), "--bad".to_owned()]).is_err());
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/config/options-v1.json");
        if fixture.exists() {
            let runtime = load_runtime(&fixture).unwrap();
            let first = canonical_dashboard_json(&runtime).unwrap();
            let second = canonical_dashboard_json(&runtime).unwrap();
            assert_eq!(first, second);
            assert!(first.contains("\"clean\":true"));
        }
    }
}
