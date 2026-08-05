//! Local, non-live demonstration of the deterministic first vertical slice.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use follon_control_plane::{
    import_historical_bars, load_persisted_market_bars, BuyOnceStrategy, DeterministicFillModel,
    FileEventStore, MarketPreconditions, ReplayEngine, RiskPolicy,
};
use follon_domain::Decimal;
use follon_instrument::{
    AssetClass, Instrument, InstrumentRegistry, InstrumentVersion, StaticTradingCalendar,
    TradingSession,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = env::args().skip(1).collect();
    let replay_persisted_events = arguments
        .first()
        .is_some_and(|argument| argument == "--event-log");
    let input_argument = if replay_persisted_events {
        arguments.get(1)
    } else {
        arguments.first()
    };
    let output_argument = if replay_persisted_events {
        arguments.get(2)
    } else {
        arguments.get(1)
    };
    let input_path = input_argument
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/fixtures/historical-bars/spy-one-minute.csv"));
    let output_path = output_argument
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("var/follon-events.ndjson"));
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut engine = ReplayEngine::new(
        "2026-01-02T14:30:00Z",
        "core-0.1.0",
        "cfg-example-1",
        RiskPolicy {
            version: "risk-example-1".to_owned(),
            global_kill_switch: false,
            max_quantity: decimal("10")?,
            max_notional: decimal("10000")?,
        },
        DeterministicFillModel {
            slippage_bps: decimal("0")?,
            flat_fee: decimal("0.10")?,
        },
    )?;
    let mut strategy = BuyOnceStrategy::new(
        "acct-paper-001",
        "strategy-example-001",
        "strategy-example-v1",
        "cfg-example-1",
        decimal("100")?,
    );
    let mut store = FileEventStore::open(&output_path)?;
    let (instruments, calendar) = example_market_dependencies()?;
    let market = MarketPreconditions {
        instruments: &instruments,
        calendar: &calendar,
    };
    let historical_bars = if replay_persisted_events {
        load_persisted_market_bars(&input_path)?
    } else {
        import_historical_bars(&fs::read_to_string(&input_path)?)?
    };
    for historical_bar in historical_bars {
        let result = engine.process_bar_with_market_preconditions(
            &mut store,
            &mut strategy,
            "acct-paper-001",
            &historical_bar.event_time,
            historical_bar.bar,
            &market,
        )?;
        for event in result.events {
            println!("{}", event.canonical_json());
        }
        if let Some(position) = result.position {
            eprintln!(
                "position {} {} @ {}",
                position.instrument_id, position.quantity, position.average_cost
            );
        }
        if let Some(pnl) = result.pnl {
            eprintln!("simulated P&L {}", pnl.total_pnl);
        }
    }
    eprintln!("persisted event log: {}", output_path.display());
    Ok(())
}

fn decimal(value: &str) -> Result<Decimal, follon_domain::DecimalError> {
    Decimal::from_str(value)
}

fn example_market_dependencies(
) -> Result<(InstrumentRegistry, StaticTradingCalendar), Box<dyn std::error::Error>> {
    let calendar = StaticTradingCalendar::new(
        "cal.us_equities.nyse",
        vec![TradingSession {
            exchange_date: "2026-01-02".to_owned(),
            opens_at: "2026-01-02T14:30:00Z".to_owned(),
            closes_at: "2026-01-02T21:00:00Z".to_owned(),
        }],
    )?;
    let mut instruments = InstrumentRegistry::default();
    instruments.register(InstrumentVersion {
        instrument: Instrument {
            instrument_id: "inst.us_equity.spy".to_owned(),
            symbol: "SPY".to_owned(),
            exchange_symbol: "SPY".to_owned(),
            asset_class: AssetClass::Etf,
            venue: "venue.nyse_arca".to_owned(),
            currency: "USD".to_owned(),
            broker_ids: BTreeMap::new(),
            tick_size: decimal("0.01")?,
            lot_size: decimal("1")?,
            multiplier: decimal("1")?,
            trading_calendar_id: "cal.us_equities.nyse".to_owned(),
        },
        effective_from: "2026-01-01T00:00:00Z".to_owned(),
        effective_to: None,
        reference_version: "reference-example-1".to_owned(),
    })?;
    Ok((instruments, calendar))
}
