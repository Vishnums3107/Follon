//! Immutable transaction-cost-analysis evidence command.
//!
//! The command reads a frozen local analysis input and emits both a canonical
//! JSON artifact and a one-page Markdown review pack. It has no broker,
//! credential, order-control, or wall-clock interface.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use follon_cli::{sha256_text, write_immutable};
use follon_domain::{validate_utc_timestamp, Decimal, Side};
use follon_execution::{analyze_transaction_costs, TcaFill, TransactionCostInput};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TcaDocument {
    schema_version: u32,
    as_of: String,
    analyses: Vec<TcaAnalysisDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TcaAnalysisDocument {
    analysis_id: String,
    strategy_id: String,
    parent_order_id: String,
    execution_algorithm: String,
    order_type: String,
    side: String,
    arrival_price: String,
    target_price: String,
    requested_quantity: String,
    fills: Vec<TcaFillDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TcaFillDocument {
    execution_id: String,
    quantity: String,
    price: String,
    fee: String,
}

struct CommandArguments {
    input_path: PathBuf,
    output_path: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = parse_arguments(env::args().skip(1).collect())?;
    let source = fs::read(&arguments.input_path)?;
    if source.is_empty() || source.len() > 10 * 1024 * 1024 {
        return Err("TCA input must be between 1 byte and 10 MiB".into());
    }
    let document: TcaDocument = serde_json::from_slice(&source)?;
    if document.schema_version != 1 {
        return Err("unsupported transaction-cost input schema version".into());
    }
    validate_utc_timestamp("TCA as_of", &document.as_of)?;
    let inputs = document
        .analyses
        .into_iter()
        .map(tca_input)
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    let batch = analyze_transaction_costs(&inputs)?;
    let input_hash = sha256_text(&String::from_utf8(source)?);
    let artifact = format!(
        "{{\"as_of\":{},\"input_sha256\":\"{}\",\"transaction_cost\":{}}}",
        json_string(&document.as_of),
        input_hash,
        batch.canonical_json(),
    );
    let report = format!(
        "# Follon End-of-Day Execution Pack\n\n- As of: {}\n- Frozen input SHA-256: `{}`\n\n{}",
        document.as_of,
        input_hash,
        batch.markdown_report(),
    );
    let report_path = arguments.output_path.with_extension("report.md");
    let manifest_path = arguments.output_path.with_extension("manifest.json");
    write_immutable(&arguments.output_path, &artifact)?;
    write_immutable(&report_path, &report)?;
    let manifest = format!(
        "{{\"artifact_sha256\":\"{}\",\"input_sha256\":\"{}\",\"manifest_schema_version\":1,\"report_sha256\":\"{}\"}}",
        sha256_text(&artifact),
        input_hash,
        sha256_text(&report),
    );
    write_immutable(&manifest_path, &manifest)?;
    eprintln!("TCA artifact: {}", arguments.output_path.display());
    eprintln!("TCA report: {}", report_path.display());
    eprintln!("TCA manifest: {}", manifest_path.display());
    Ok(())
}

fn tca_input(
    document: TcaAnalysisDocument,
) -> Result<TransactionCostInput, Box<dyn std::error::Error>> {
    let side = match document.side.as_str() {
        "BUY" => Side::Buy,
        "SELL" => Side::Sell,
        _ => return Err("TCA side must be BUY or SELL".into()),
    };
    Ok(TransactionCostInput {
        analysis_id: document.analysis_id,
        strategy_id: document.strategy_id,
        parent_order_id: document.parent_order_id,
        execution_algorithm: document.execution_algorithm,
        order_type: document.order_type,
        side,
        arrival_price: decimal(&document.arrival_price)?,
        target_price: decimal(&document.target_price)?,
        requested_quantity: decimal(&document.requested_quantity)?,
        fills: document
            .fills
            .into_iter()
            .map(|fill| {
                Ok(TcaFill {
                    execution_id: fill.execution_id,
                    quantity: decimal(&fill.quantity)?,
                    price: decimal(&fill.price)?,
                    fee: decimal(&fill.fee)?,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
    })
}

fn decimal(value: &str) -> Result<Decimal, follon_domain::DecimalError> {
    Decimal::from_str(value)
}

fn parse_arguments(arguments: Vec<String>) -> Result<CommandArguments, Box<dyn std::error::Error>> {
    if !(1..=2).contains(&arguments.len()) || arguments.iter().any(|value| value.starts_with('-')) {
        return Err("usage: follon-tca <tca-v1.json> [output.json]".into());
    }
    let input_path = PathBuf::from(&arguments[0]);
    let output_path = arguments
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("var/follon-tca.json"));
    if output_path
        .extension()
        .and_then(|extension| extension.to_str())
        != Some("json")
    {
        return Err("TCA output must use a .json extension".into());
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(CommandArguments {
        input_path,
        output_path,
    })
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parser_requires_a_versioned_input_and_json_output() {
        assert!(parse_arguments(Vec::new()).is_err());
        assert!(parse_arguments(vec!["input.json".to_owned(), "output.md".to_owned()]).is_err());
        let parsed = parse_arguments(vec!["input.json".to_owned()]).unwrap();
        assert_eq!(parsed.input_path, Path::new("input.json"));
        assert_eq!(parsed.output_path, Path::new("var/follon-tca.json"));
    }
}
