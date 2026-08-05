//! Deterministic normalized-trade to canonical-bar command.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use follon_cli::{sha256_text, write_immutable};
use follon_market_data::{import_trades, BarBuilder};

const DEFAULT_TRADES_PATH: &str = "tests/fixtures/historical-bars/spy-trades-v1.csv";
const DEFAULT_BARS_PATH: &str = "var/follon-bars.csv";
const DEFAULT_EXCHANGE_TIMEZONE: &str = "America/New_York";
const DEFAULT_INTERVAL_SECONDS: u32 = 60;

#[derive(Debug, Eq, PartialEq)]
struct CommandArguments {
    trades_path: PathBuf,
    bars_path: PathBuf,
    interval_seconds: u32,
    exchange_timezone: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = parse_arguments(std::env::args().skip(1).collect())?;
    if let Some(parent) = arguments.bars_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let source = fs::read_to_string(&arguments.trades_path)?;
    let trades = import_trades(&source)?;
    let bars = BarBuilder::new(
        arguments.interval_seconds,
        arguments.exchange_timezone.clone(),
    )?
    .build(trades)?;

    let mut output = String::from(
        "event_time,instrument_id,open,high,low,close,volume,interval_seconds,exchange_timezone\n",
    );
    for (event_time, bar) in &bars {
        output.push_str(&format!(
            "{event_time},{},{},{},{},{},{},{},{}\n",
            bar.instrument_id,
            bar.open,
            bar.high,
            bar.low,
            bar.close,
            bar.volume,
            bar.interval_seconds,
            bar.exchange_timezone
        ));
    }

    write_immutable(&arguments.bars_path, &output)?;
    println!("bars={}", bars.len());
    println!("output={}", arguments.bars_path.display());
    println!("sha256={}", sha256_text(&output));
    Ok(())
}

fn parse_arguments(arguments: Vec<String>) -> Result<CommandArguments, Box<dyn Error>> {
    let mut positionals = Vec::new();
    let mut interval_seconds = None;
    let mut exchange_timezone = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--interval-seconds" => {
                if interval_seconds.is_some() {
                    return Err("--interval-seconds may be supplied only once".into());
                }
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or("--interval-seconds requires a value")?;
                let parsed = value
                    .parse::<u32>()
                    .map_err(|_| "--interval-seconds must be an integer")?;
                if parsed == 0 {
                    return Err("--interval-seconds must be positive".into());
                }
                interval_seconds = Some(parsed);
            }
            "--exchange-timezone" => {
                if exchange_timezone.is_some() {
                    return Err("--exchange-timezone may be supplied only once".into());
                }
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or("--exchange-timezone requires a value")?;
                if value.trim().is_empty() || value.contains(',') || value.contains('\n') {
                    return Err("--exchange-timezone must be a non-empty CSV-safe value".into());
                }
                exchange_timezone = Some(value.clone());
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown option: {value}").into());
            }
            value => positionals.push(value.to_owned()),
        }
        index += 1;
    }
    if positionals.len() > 2 {
        return Err(
            "usage: follon-build-bars [trades.csv] [bars.csv] [--interval-seconds N] [--exchange-timezone TZ]"
                .into(),
        );
    }
    Ok(CommandArguments {
        trades_path: positionals
            .first()
            .map_or_else(|| PathBuf::from(DEFAULT_TRADES_PATH), PathBuf::from),
        bars_path: positionals
            .get(1)
            .map_or_else(|| PathBuf::from(DEFAULT_BARS_PATH), PathBuf::from),
        interval_seconds: interval_seconds.unwrap_or(DEFAULT_INTERVAL_SECONDS),
        exchange_timezone: exchange_timezone
            .unwrap_or_else(|| DEFAULT_EXCHANGE_TIMEZONE.to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_strict_and_have_operational_defaults() {
        let defaults = parse_arguments(Vec::new()).unwrap();
        assert_eq!(defaults.trades_path, PathBuf::from(DEFAULT_TRADES_PATH));
        assert_eq!(defaults.bars_path, PathBuf::from(DEFAULT_BARS_PATH));
        assert_eq!(defaults.interval_seconds, 60);

        let explicit = parse_arguments(vec![
            "input.csv".to_owned(),
            "output.csv".to_owned(),
            "--interval-seconds".to_owned(),
            "300".to_owned(),
            "--exchange-timezone".to_owned(),
            "UTC".to_owned(),
        ])
        .unwrap();
        assert_eq!(explicit.interval_seconds, 300);
        assert_eq!(explicit.exchange_timezone, "UTC");
        assert!(parse_arguments(vec!["--unknown".to_owned()]).is_err());
        assert!(parse_arguments(vec!["--interval-seconds".to_owned(), "0".to_owned()]).is_err());
    }
}
