//! Deterministic quote-stream gap repair from an explicit recovery batch.
//!
//! Reads a recorded v1 quote CSV and a recovery batch, such as a vendor
//! replay of the gap window, in the same contract. It writes the repaired
//! stream and a hash-bound repair record. A gap the batch does not fill stays
//! declared in the record, and `--require-complete` turns any residual gap
//! into a failing exit after both artifacts are written. A refused repair
//! writes nothing.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use follon_cli::{sha256_text, write_immutable};
use follon_market_data::{import_quotes, quotes_to_csv, repair_quote_gaps, SequenceRange};
use serde_json::{json, Value};

const USAGE: &str = "usage: follon-repair-quotes --recorded quotes.csv --recovery recovery.csv --output-dir DIR [--require-complete]";

#[derive(Debug, Eq, PartialEq)]
struct CommandArguments {
    recorded: PathBuf,
    recovery: PathBuf,
    output_dir: PathBuf,
    require_complete: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = parse_arguments(std::env::args().skip(1).collect())?;
    let recorded_source = fs::read_to_string(&arguments.recorded)?;
    let recovery_source = fs::read_to_string(&arguments.recovery)?;
    let recorded = import_quotes(&recorded_source)?;
    let recovery = import_quotes(&recovery_source)?;
    let repair = repair_quote_gaps(&recorded, &recovery)?;

    let repaired = quotes_to_csv(&repair.quotes);
    let ranges = |ranges: &[SequenceRange]| -> Value {
        ranges
            .iter()
            .map(|range| {
                json!({
                    "from": range.from,
                    "instrument_id": range.instrument_id,
                    "to": range.to,
                })
            })
            .collect()
    };
    let record = json!({
        "complete": repair.is_complete(),
        "corroborated": repair.corroborated,
        "gap_repair_schema_version": 1,
        "gaps": ranges(&repair.gaps),
        "recorded_quotes": recorded.len(),
        "recorded_redeliveries": repair.recorded_redeliveries,
        "recorded_sha256": sha256_text(&recorded_source),
        "recovered": ranges(&repair.recovered),
        "recovery_quotes": recovery.len(),
        "recovery_sha256": sha256_text(&recovery_source),
        "repaired_quotes": repair.quotes.len(),
        "repaired_sha256": sha256_text(&repaired),
        "residual": ranges(&repair.residual),
    });
    let record = format!("{}\n", serde_json::to_string_pretty(&record)?);

    fs::create_dir_all(&arguments.output_dir)?;
    let repaired_path = arguments.output_dir.join("repaired-quotes.csv");
    let record_path = arguments.output_dir.join("gap-repair.json");
    write_immutable(&repaired_path, &repaired)?;
    write_immutable(&record_path, &record)?;
    println!("gaps={}", repair.gaps.len());
    println!("recovered={}", repair.recovered.len());
    println!("residual={}", repair.residual.len());
    println!("output={}", repaired_path.display());
    println!("record={}", record_path.display());
    println!("sha256={}", sha256_text(&repaired));
    if arguments.require_complete && !repair.is_complete() {
        return Err(format!(
            "{} residual gap(s) remain after repair; see {}",
            repair.residual.len(),
            record_path.display()
        )
        .into());
    }
    Ok(())
}

fn parse_arguments(arguments: Vec<String>) -> Result<CommandArguments, Box<dyn Error>> {
    let mut recorded = None;
    let mut recovery = None;
    let mut output_dir = None;
    let mut require_complete = false;
    let mut values = arguments.into_iter();
    while let Some(argument) = values.next() {
        let slot = match argument.as_str() {
            "--recorded" => &mut recorded,
            "--recovery" => &mut recovery,
            "--output-dir" => &mut output_dir,
            "--require-complete" => {
                if require_complete {
                    return Err("--require-complete may be supplied only once".into());
                }
                require_complete = true;
                continue;
            }
            other => return Err(format!("unknown argument: {other}\n{USAGE}").into()),
        };
        if slot.is_some() {
            return Err(format!("{argument} may be supplied only once").into());
        }
        let value = values
            .next()
            .ok_or_else(|| format!("{argument} requires a value"))?;
        *slot = Some(PathBuf::from(value));
    }
    Ok(CommandArguments {
        recorded: recorded.ok_or(USAGE)?,
        recovery: recovery.ok_or(USAGE)?,
        output_dir: output_dir.ok_or(USAGE)?,
        require_complete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn arguments_are_strict() {
        let parsed = parse_arguments(strings(&[
            "--recorded",
            "a.csv",
            "--recovery",
            "b.csv",
            "--output-dir",
            "out",
            "--require-complete",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            CommandArguments {
                recorded: PathBuf::from("a.csv"),
                recovery: PathBuf::from("b.csv"),
                output_dir: PathBuf::from("out"),
                require_complete: true,
            }
        );
        assert!(parse_arguments(strings(&["--recorded", "a.csv", "--recovery", "b.csv"])).is_err());
        assert!(parse_arguments(strings(&["--recorded", "a.csv", "--recorded", "b.csv"])).is_err());
        assert!(parse_arguments(strings(&["--recorded"])).is_err());
        assert!(parse_arguments(strings(&["--unknown"])).is_err());
    }
}
