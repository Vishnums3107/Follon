//! Deterministic FX pricing, snapshot validation, and immutable reporting CLI.
//!
//! Evaluates spot, forward, and swap pricing snapshots strictly using fixed-point
//! decimal arithmetic, explicit valuation times, and verifiable source sequences.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use follon_cli::{sha256_text, write_immutable};
use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal};
use follon_fx::{
    FxError, FxOutrightQuote, FxPair, FxPriceTerms, FxPricingBook, FxPricingSnapshot, FxProduct,
    FxValueDate,
};
use serde::Deserialize;

const DEFAULT_CONFIGURATION: &str = "tests/fixtures/config/fx-v1.json";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FxConfigurationDocument {
    schema_version: u32,
    as_of: String,
    default_max_age_seconds: i64,
    snapshots: Vec<FxSnapshotDocument>,
    queries: Vec<FxQueryDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FxSnapshotDocument {
    snapshot_id: String,
    reference_version: String,
    instrument_id: String,
    product: String,
    base_currency: String,
    quote_currency: String,
    terms: FxTermsDocument,
    source_id: String,
    source_sequence: u64,
    source_time: String,
    received_at: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FxTermsDocument {
    Outright {
        value_date: String,
        bid: String,
        ask: String,
    },
    Swap {
        near_value_date: String,
        far_value_date: String,
        near_bid: String,
        near_ask: String,
        far_bid: String,
        far_ask: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FxQueryDocument {
    query_id: String,
    instrument_id: String,
    product: String,
    value_date: String,
}

struct RuntimeFx {
    configuration_content_hash: String,
    as_of: String,
    default_max_age_seconds: i64,
    book: FxPricingBook,
    snapshots: BTreeMap<String, FxPricingSnapshot>,
    queries: Vec<FxQueryDocument>,
}

struct ExecutionArguments {
    configuration_path: PathBuf,
    output_path: PathBuf,
    as_of: Option<String>,
    max_age_seconds: Option<i64>,
}

enum Command {
    Validate { configuration_path: PathBuf },
    Price(ExecutionArguments),
    Report(ExecutionArguments),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match parse_command(env::args().skip(1).collect())? {
        Command::Validate { configuration_path } => {
            let runtime = load_runtime(&configuration_path)?;
            println!(
                "{{\"configuration_content_hash\":{},\"snapshot_count\":{},\"query_count\":{},\"valid\":true}}",
                json_string(&runtime.configuration_content_hash),
                runtime.snapshots.len(),
                runtime.queries.len(),
            );
        }
        Command::Price(args) => {
            let runtime = load_runtime(&args.configuration_path)?;
            let evaluation_as_of = args.as_of.unwrap_or(runtime.as_of.clone());
            let max_age = args
                .max_age_seconds
                .unwrap_or(runtime.default_max_age_seconds);
            let dashboard = canonical_fx_pricing_json(&runtime, &evaluation_as_of, max_age)?;
            publish(&args.output_path, &dashboard)?;
            eprintln!("fx pricing dashboard: {}", args.output_path.display());
        }
        Command::Report(args) => {
            let runtime = load_runtime(&args.configuration_path)?;
            let evaluation_as_of = args.as_of.unwrap_or(runtime.as_of.clone());
            let max_age = args
                .max_age_seconds
                .unwrap_or(runtime.default_max_age_seconds);
            let report = markdown_report(&runtime, &evaluation_as_of, max_age)?;
            publish(&args.output_path, &report)?;
            eprintln!("fx report: {}", args.output_path.display());
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
            if remainder.len() > 1 || remainder.first().is_some_and(|v| v.starts_with('-')) {
                return Err("usage: follon-fx validate-config [fx.json]".into());
            }
            Ok(Command::Validate {
                configuration_path: remainder
                    .first()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIGURATION)),
            })
        }
        "price" => Ok(Command::Price(parse_execution_arguments(
            remainder,
            "var/follon-fx-pricing.json",
        )?)),
        "report" => Ok(Command::Report(parse_execution_arguments(
            remainder,
            "var/follon-fx-report.md",
        )?)),
        _ => Err(usage().into()),
    }
}

fn parse_execution_arguments(
    arguments: &[String],
    default_output: &str,
) -> Result<ExecutionArguments, Box<dyn std::error::Error>> {
    let mut positional = Vec::new();
    let mut as_of = None;
    let mut max_age = None;
    let mut index = 0;

    while index < arguments.len() {
        match arguments[index].as_str() {
            "--as-of" => {
                if as_of.is_some() {
                    return Err("--as-of may be specified only once".into());
                }
                index += 1;
                let val = required(arguments, index, "--as-of")?;
                validate_utc_timestamp("--as-of", val)?;
                as_of = Some(val.to_owned());
            }
            "--max-age" => {
                if max_age.is_some() {
                    return Err("--max-age may be specified only once".into());
                }
                index += 1;
                let val = required(arguments, index, "--max-age")?;
                let seconds: i64 = val
                    .parse()
                    .map_err(|_| "--max-age must be a positive integer")?;
                if seconds < 0 {
                    return Err("--max-age cannot be negative".into());
                }
                max_age = Some(seconds);
            }
            value if value.starts_with('-') => {
                return Err(format!("unsupported argument: {value}").into());
            }
            value => positional.push(PathBuf::from(value)),
        }
        index += 1;
    }

    if positional.len() > 2 {
        return Err(
            "command accepts [fx.json] [output_file] [--as-of <UTC>] [--max-age <sec>]".into(),
        );
    }

    let config_path = positional
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIGURATION));
    let output_path = positional
        .get(1)
        .cloned()
        .unwrap_or_else(|| PathBuf::from(default_output));

    Ok(ExecutionArguments {
        configuration_path: config_path,
        output_path,
        as_of,
        max_age_seconds: max_age,
    })
}

fn load_runtime(path: &Path) -> Result<RuntimeFx, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    if bytes.is_empty() || bytes.len() > 2 * 1024 * 1024 {
        return Err("FX configuration must be between 1 byte and 2 MiB".into());
    }
    let source = String::from_utf8(bytes)?;
    let configuration_content_hash = sha256_text(&source);
    let document: FxConfigurationDocument = serde_json::from_str(&source)?;

    if document.schema_version != 1 {
        return Err("unsupported FX configuration schema version".into());
    }
    validate_utc_timestamp("FX configuration as_of", &document.as_of)?;
    if document.default_max_age_seconds < 0 {
        return Err("default_max_age_seconds cannot be negative".into());
    }

    let mut snapshots = BTreeMap::new();
    let mut snapshot_vec = Vec::new();

    for raw in document.snapshots {
        let product = parse_product(&raw.product)?;
        let pair = FxPair::new(&raw.base_currency, &raw.quote_currency)?;
        let terms = match raw.terms {
            FxTermsDocument::Outright {
                value_date,
                bid,
                ask,
            } => FxPriceTerms::Outright {
                value_date: FxValueDate::new(value_date)?,
                quote: FxOutrightQuote {
                    bid: decimal(&bid)?,
                    ask: decimal(&ask)?,
                },
            },
            FxTermsDocument::Swap {
                near_value_date,
                far_value_date,
                near_bid,
                near_ask,
                far_bid,
                far_ask,
            } => FxPriceTerms::Swap {
                near_value_date: FxValueDate::new(near_value_date)?,
                far_value_date: FxValueDate::new(far_value_date)?,
                near_quote: FxOutrightQuote {
                    bid: decimal(&near_bid)?,
                    ask: decimal(&near_ask)?,
                },
                far_quote: FxOutrightQuote {
                    bid: decimal(&far_bid)?,
                    ask: decimal(&far_ask)?,
                },
            },
        };

        let snapshot = FxPricingSnapshot {
            snapshot_id: raw.snapshot_id.clone(),
            reference_version: raw.reference_version,
            instrument_id: raw.instrument_id,
            product,
            pair,
            terms,
            source_id: raw.source_id,
            source_sequence: raw.source_sequence,
            source_time: raw.source_time,
            received_at: raw.received_at,
        };
        snapshot.validate()?;

        if snapshots
            .insert(raw.snapshot_id.clone(), snapshot.clone())
            .is_some()
        {
            return Err(format!("duplicate snapshot_id: {}", raw.snapshot_id).into());
        }
        snapshot_vec.push(snapshot);
    }

    for query in &document.queries {
        validate_canonical_id("FX query_id", &query.query_id)?;
        validate_canonical_id("FX instrument_id", &query.instrument_id)?;
        parse_product(&query.product)?;
        FxValueDate::new(&query.value_date)?;
    }

    let book = FxPricingBook::from_snapshots(snapshot_vec)?;

    Ok(RuntimeFx {
        configuration_content_hash,
        as_of: document.as_of,
        default_max_age_seconds: document.default_max_age_seconds,
        book,
        snapshots,
        queries: document.queries,
    })
}

struct EvaluationRecord {
    query_id: String,
    instrument_id: String,
    product: String,
    pair: String,
    value_date: String,
    midpoint: Decimal,
    bid: Decimal,
    ask: Decimal,
    spread_bps: Decimal,
    snapshot_id: String,
    source_id: String,
    source_sequence: u64,
    source_time: String,
    received_at: String,
    age_seconds: i64,
    fresh: bool,
}

fn evaluate_queries(
    runtime: &RuntimeFx,
    as_of: &str,
    max_age_seconds: i64,
) -> Result<Vec<EvaluationRecord>, Box<dyn std::error::Error>> {
    let mut records = Vec::new();
    let as_of_instant = parse_utc(as_of)?;

    for query in &runtime.queries {
        let product = parse_product(&query.product)?;
        let value_date = FxValueDate::new(&query.value_date)?;

        let (snapshot_id, midpoint) = runtime
            .book
            .midpoint_at(
                &query.instrument_id,
                product,
                &value_date,
                as_of,
                max_age_seconds,
            )
            .map_err(|error| {
                format!(
                    "FX query '{}' ({} {} value_date {}) could not be priced: {error}",
                    query.query_id, query.instrument_id, query.product, query.value_date
                )
            })?;

        let snapshot = runtime
            .snapshots
            .get(&snapshot_id)
            .ok_or_else(|| format!("snapshot {snapshot_id} missing from book"))?;

        let (bid, ask) = match &snapshot.terms {
            FxPriceTerms::Outright { quote, .. } => (quote.bid, quote.ask),
            FxPriceTerms::Swap {
                near_value_date,
                near_quote,
                far_quote,
                ..
            } => {
                if near_value_date == &value_date {
                    (near_quote.bid, near_quote.ask)
                } else {
                    (far_quote.bid, far_quote.ask)
                }
            }
        };

        let spread = ask.checked_sub(bid)?;
        let ten_thousand = Decimal::from_integer(10000)?;
        let spread_bps = spread.checked_mul(ten_thousand)?.checked_div(midpoint)?;

        let received_instant = parse_utc(&snapshot.received_at)?;
        let age_seconds = as_of_instant
            .unix_timestamp()
            .checked_sub(received_instant.unix_timestamp())
            .unwrap_or(0);

        records.push(EvaluationRecord {
            query_id: query.query_id.clone(),
            instrument_id: query.instrument_id.clone(),
            product: query.product.clone(),
            pair: snapshot.pair.key(),
            value_date: query.value_date.clone(),
            midpoint,
            bid,
            ask,
            spread_bps,
            snapshot_id,
            source_id: snapshot.source_id.clone(),
            source_sequence: snapshot.source_sequence,
            source_time: snapshot.source_time.clone(),
            received_at: snapshot.received_at.clone(),
            age_seconds,
            fresh: age_seconds <= max_age_seconds,
        });
    }
    Ok(records)
}

fn canonical_fx_pricing_json(
    runtime: &RuntimeFx,
    as_of: &str,
    max_age_seconds: i64,
) -> Result<String, Box<dyn std::error::Error>> {
    let evaluations = evaluate_queries(runtime, as_of, max_age_seconds)?;
    let evals_json = evaluations
        .iter()
        .map(|e| {
            format!(
                "{{\"age_seconds\":{},\"ask\":\"{}\",\"bid\":\"{}\",\"fresh\":{},\"instrument_id\":{},\"midpoint\":\"{}\",\"pair\":{},\"product\":{},\"query_id\":{},\"received_at\":{},\"snapshot_id\":{},\"source_id\":{},\"source_sequence\":{},\"source_time\":{},\"spread_bps\":\"{}\",\"value_date\":{}}}",
                e.age_seconds,
                e.ask,
                e.bid,
                e.fresh,
                json_string(&e.instrument_id),
                e.midpoint,
                json_string(&e.pair),
                json_string(&e.product),
                json_string(&e.query_id),
                json_string(&e.received_at),
                json_string(&e.snapshot_id),
                json_string(&e.source_id),
                e.source_sequence,
                json_string(&e.source_time),
                e.spread_bps,
                json_string(&e.value_date),
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    let snapshots_json = runtime
        .snapshots
        .values()
        .map(|s| {
            format!(
                "{{\"canonical_record\":{},\"instrument_id\":{},\"pair\":{},\"product\":{},\"received_at\":{},\"reference_version\":{},\"snapshot_id\":{},\"source_id\":{},\"source_sequence\":{},\"source_time\":{}}}",
                json_string(&s.canonical_record().unwrap_or_default()),
                json_string(&s.instrument_id),
                json_string(&s.pair.key()),
                json_string(s.product.as_str()),
                json_string(&s.received_at),
                json_string(&s.reference_version),
                json_string(&s.snapshot_id),
                json_string(&s.source_id),
                s.source_sequence,
                json_string(&s.source_time),
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    Ok(format!(
        "{{\"as_of\":{},\"configuration_content_hash\":{},\"evaluations\":[{}],\"fx_pricing_schema_version\":1,\"max_age_seconds\":{},\"snapshot_count\":{},\"snapshots\":[{}]}}",
        json_string(as_of),
        json_string(&runtime.configuration_content_hash),
        evals_json,
        max_age_seconds,
        runtime.snapshots.len(),
        snapshots_json,
    ))
}

fn markdown_report(
    runtime: &RuntimeFx,
    as_of: &str,
    max_age_seconds: i64,
) -> Result<String, Box<dyn std::error::Error>> {
    let evaluations = evaluate_queries(runtime, as_of, max_age_seconds)?;
    let mut report = format!(
        "# Follon FX Valuation and Reference Report\n\n- Evaluation As-Of: `{as_of}`\n- Configuration Hash: `{}`\n- Max Quote Age: `{max_age_seconds}s`\n- Total Snapshots Registered: `{}`\n- Queries Evaluated: `{}`\n\n## Evaluated Midpoints & Spreads\n\n| Query ID | Pair | Product | Value Date | Midpoint | Bid | Ask | Spread (bps) | Age | Snapshot ID | Status |\n| --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- |\n",
        runtime.configuration_content_hash,
        runtime.snapshots.len(),
        evaluations.len(),
    );

    for e in &evaluations {
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {}s | {} | {} |\n",
            e.query_id,
            e.pair,
            e.product,
            e.value_date,
            e.midpoint,
            e.bid,
            e.ask,
            e.spread_bps,
            e.age_seconds,
            e.snapshot_id,
            if e.fresh { "**FRESH**" } else { "*STALE*" },
        ));
    }

    report.push_str("\n## Ingested Pricing Snapshots\n\n| Snapshot ID | Instrument ID | Product | Pair | Source ID | Sequence | Source Time | Received At |\n| --- | --- | --- | --- | --- | ---: | --- | --- |\n");
    for s in runtime.snapshots.values() {
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            s.snapshot_id,
            s.instrument_id,
            s.product.as_str(),
            s.pair.key(),
            s.source_id,
            s.source_sequence,
            s.source_time,
            s.received_at,
        ));
    }

    Ok(report)
}

fn parse_product(value: &str) -> Result<FxProduct, FxError> {
    match value {
        "FX_SPOT" | "SPOT" => Ok(FxProduct::Spot),
        "FX_FORWARD" | "FORWARD" => Ok(FxProduct::Forward),
        "FX_SWAP" | "SWAP" => Ok(FxProduct::Swap),
        _ => Err(FxError(format!("unknown FX product: {value}"))),
    }
}

fn parse_utc(value: &str) -> Result<time::OffsetDateTime, FxError> {
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map_err(|_| FxError(format!("invalid canonical UTC timestamp: {value}")))
}

fn decimal(value: &str) -> Result<Decimal, follon_domain::DecimalError> {
    Decimal::from_str(value)
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization cannot fail")
}

fn required<'a>(
    arguments: &'a [String],
    index: usize,
    flag: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    arguments
        .get(index)
        .map(String::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{flag} requires a value").into())
}

fn publish(path: &Path, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_immutable(path, contents)
}

fn usage() -> &'static str {
    "usage:\n  follon-fx validate-config [fx.json]\n  follon-fx price [fx.json] [dashboard.json] [--as-of <UTC>] [--max-age <sec>]\n  follon-fx report [fx.json] [report.md] [--as-of <UTC>] [--max-age <sec>]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_command_validates_syntax() {
        assert!(parse_command(vec!["unknown".to_owned()]).is_err());
        assert!(parse_command(vec!["validate-config".to_owned()]).is_ok());
        assert!(parse_command(vec![
            "price".to_owned(),
            "--as-of".to_owned(),
            "2026-09-05T12:00:00Z".to_owned(),
        ])
        .is_ok());
        assert!(parse_command(vec![
            "price".to_owned(),
            "--max-age".to_owned(),
            "-5".to_owned(),
        ])
        .is_err());
    }

    #[test]
    fn fx_fixture_is_deterministic_and_reproducible() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config/fx-v1.json");
        assert!(fixture.exists());

        let runtime = load_runtime(&fixture).unwrap();
        assert_eq!(runtime.snapshots.len(), 5);
        assert_eq!(runtime.queries.len(), 6);

        let as_of = "2026-09-05T12:00:00Z";
        let first = canonical_fx_pricing_json(&runtime, as_of, 60).unwrap();
        let second = canonical_fx_pricing_json(&runtime, as_of, 60).unwrap();
        assert_eq!(first, second);
        assert!(first.contains("\"fresh\":true"));
        assert!(first.contains("fx.snapshot.eur-usd.spot.001"));

        let report = markdown_report(&runtime, as_of, 60).unwrap();
        assert!(report.contains("# Follon FX Valuation and Reference Report"));
        assert!(report.contains("FRESH"));
    }

    /// Regression test for the silent-drop bug in `evaluate_queries`: a query
    /// naming a value date with no matching pricing snapshot used to vanish
    /// from the output with no error, no warning, and no non-zero exit code.
    /// It must now fail the whole command with an error that names the
    /// specific unpriceable query.
    #[test]
    fn unpriceable_query_fails_the_command_instead_of_vanishing_silently() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config/fx-v1.json");
        let mut document: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&fixture).unwrap()).unwrap();
        document["queries"]
            .as_array_mut()
            .expect("queries array")
            .push(serde_json::json!({
                "query_id": "query.unpriceable.eur-usd.spot",
                "instrument_id": "instrument.fx.eur-usd",
                "product": "FX_SPOT",
                "value_date": "2027-01-01"
            }));

        let path =
            std::env::temp_dir().join(format!("follon-fx-unpriceable-{}.json", std::process::id()));
        std::fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();

        let runtime = load_runtime(&path).unwrap();
        assert_eq!(runtime.queries.len(), 7);

        let as_of = "2026-09-05T12:00:00Z";
        let error = canonical_fx_pricing_json(&runtime, as_of, 60)
            .expect_err("query with no matching snapshot must fail, not vanish");
        let message = error.to_string();
        assert!(message.contains("query.unpriceable.eur-usd.spot"));
        assert!(message.contains("could not be priced"));

        // The Markdown report path must fail the same way.
        assert!(markdown_report(&runtime, as_of, 60).is_err());

        std::fs::remove_file(&path).unwrap();
    }
}
