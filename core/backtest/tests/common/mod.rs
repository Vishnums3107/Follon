//! Fixtures the corporate-action tests of the backtest crate share (delivery state E8.2 and
//! E8.3): five one-minute bars of one instrument, its session and reference data, a replay
//! engine and a runner, so each test states only what differs.

// Each test file uses a different part of this module.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::str::FromStr;

use follon_backtest::{
    BacktestError, BacktestInput, BacktestRunner, BacktestSpec, CompletedBacktest, DatasetManifest,
};
use follon_control_plane::{
    CorporateActionEffect, DeterministicFillModel, EngineError, HistoricalBar, MarketPreconditions,
    ReplayEngine, RiskPolicy, Strategy,
};
use follon_domain::{Bar, Decimal, OrderIntent, OrderType, Side, TimeInForce};
use follon_instrument::{
    AssetClass, Instrument, InstrumentRegistry, InstrumentVersion, StaticTradingCalendar,
    TradingSession,
};
use follon_market_data::CorporateAction;

pub const ACCOUNT: &str = "acct-paper-001";
pub const INSTRUMENT: &str = "inst.us_equity.spy";

pub fn amount(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

/// Buys one share on its first bar and sells `sell_quantity` on its fourth. Each order
/// fills on the bar after the one that produced it, so the entry fills on the second bar
/// and the exit on the fifth. It keeps every corporate-action effect it is told of.
pub struct RoundTrip {
    pub bars_seen: u32,
    pub sell_quantity: i64,
    pub effects: Vec<CorporateActionEffect>,
}

impl RoundTrip {
    pub fn new(sell_quantity: i64) -> Self {
        Self {
            bars_seen: 0,
            sell_quantity,
            effects: Vec::new(),
        }
    }
}

impl Strategy for RoundTrip {
    fn on_bar(&mut self, bar: &Bar, replay_time: &str) -> Result<Option<OrderIntent>, EngineError> {
        self.bars_seen += 1;
        let (side, quantity, tag) = match self.bars_seen {
            1 => (Side::Buy, 1, "entry"),
            4 => (Side::Sell, self.sell_quantity, "exit"),
            _ => return Ok(None),
        };
        Ok(Some(OrderIntent {
            intent_id: format!("intent-split-{tag}"),
            account_id: ACCOUNT.to_owned(),
            strategy_id: "strategy-split-001".to_owned(),
            instrument_id: bar.instrument_id.clone(),
            correlation_id: format!("corr-split-{tag}"),
            side,
            quantity: Decimal::from_integer(quantity)?,
            order_type: OrderType::Market,
            limit_price: None,
            time_in_force: TimeInForce::Day,
            rationale: "corporate action regression".to_owned(),
            created_at: replay_time.to_owned(),
            strategy_version: "strategy-split-v1".to_owned(),
            configuration_version: "cfg-v1".to_owned(),
            environment: "SIMULATION".to_owned(),
        }))
    }

    fn on_corporate_action(&mut self, effect: &CorporateActionEffect) -> Result<(), EngineError> {
        self.effects.push(effect.clone());
        Ok(())
    }
}

pub fn bar_at(price: &str) -> Bar {
    bar_of(INSTRUMENT, price)
}

pub fn bar_of(instrument: &str, price: &str) -> Bar {
    let close = amount(price);
    Bar {
        instrument_id: instrument.to_owned(),
        open: close,
        high: close.checked_add(amount("1")).unwrap(),
        low: close.checked_sub(amount("1")).unwrap(),
        close,
        volume: amount("1000"),
        interval_seconds: 60,
        exchange_timezone: "America/New_York".to_owned(),
    }
}

/// Five bars, 14:30 to 14:34. The first two trade near 100 and the last three near 50,
/// because a 2:1 split lands between the second and the third.
pub fn input(actions: Vec<CorporateAction>) -> BacktestInput {
    let prices = ["100", "100", "50", "50", "50"];
    BacktestInput {
        account_id: ACCOUNT.to_owned(),
        currency: "USD".to_owned(),
        initial_cash: amount("1000"),
        bars: prices
            .iter()
            .enumerate()
            .map(|(index, price)| HistoricalBar {
                event_time: format!("2026-01-02T14:{:02}:00Z", 30 + index),
                bar: bar_at(price),
            })
            .collect(),
        corporate_actions: actions,
    }
}

pub fn split(action_id: &str, effective_at: &str, ratio: &str) -> CorporateAction {
    CorporateAction::Split {
        action_id: action_id.to_owned(),
        instrument_id: INSTRUMENT.to_owned(),
        effective_at: effective_at.to_owned(),
        ratio: amount(ratio),
    }
}

pub fn dividend(action_id: &str, effective_at: &str, per_share: &str) -> CorporateAction {
    CorporateAction::CashDividend {
        action_id: action_id.to_owned(),
        instrument_id: INSTRUMENT.to_owned(),
        effective_at: effective_at.to_owned(),
        amount: amount(per_share),
    }
}

pub fn spec(input: &BacktestInput) -> BacktestSpec {
    let dataset_bars: Vec<_> = input
        .bars
        .iter()
        .map(|bar| (bar.event_time.clone(), bar.bar.clone()))
        .collect();
    BacktestSpec {
        strategy_bundle_hash: "a".repeat(64),
        dataset: DatasetManifest::from_market_data(
            "dataset.spy",
            "v1",
            "reference-example-1",
            "universe.spy",
            &dataset_bars,
            &input.corporate_actions,
        )
        .unwrap(),
        configuration_id: "config.test".to_owned(),
        configuration_version: "cfg-v1".to_owned(),
        configuration_hash: "b".repeat(64),
        seed: 7,
        engine_version: "engine-v1".to_owned(),
        starts_at: "2026-01-02T14:30:00Z".to_owned(),
        ends_at: "2026-01-02T14:34:00Z".to_owned(),
    }
}

pub fn market() -> (InstrumentRegistry, StaticTradingCalendar) {
    market_of(&[INSTRUMENT])
}

/// The session and reference data for each of `instrument_ids`.
pub fn market_of(instrument_ids: &[&str]) -> (InstrumentRegistry, StaticTradingCalendar) {
    let calendar = StaticTradingCalendar::new(
        "cal.us_equities.nyse",
        vec![TradingSession {
            exchange_date: "2026-01-02".to_owned(),
            opens_at: "2026-01-02T14:30:00Z".to_owned(),
            closes_at: "2026-01-02T21:00:00Z".to_owned(),
        }],
    )
    .unwrap();
    let mut instruments = InstrumentRegistry::default();
    for instrument_id in instrument_ids {
        let symbol = instrument_id.rsplit('.').next().unwrap().to_uppercase();
        instruments
            .register(InstrumentVersion {
                instrument: Instrument {
                    instrument_id: (*instrument_id).to_owned(),
                    symbol: symbol.clone(),
                    exchange_symbol: symbol,
                    asset_class: AssetClass::Etf,
                    venue: "venue.nyse_arca".to_owned(),
                    currency: "USD".to_owned(),
                    broker_ids: BTreeMap::new(),
                    tick_size: amount("0.01"),
                    lot_size: amount("1"),
                    multiplier: amount("1"),
                    trading_calendar_id: "cal.us_equities.nyse".to_owned(),
                },
                effective_from: "2026-01-01T00:00:00Z".to_owned(),
                effective_to: None,
                reference_version: "reference-example-1".to_owned(),
            })
            .unwrap();
    }
    (instruments, calendar)
}

pub fn engine() -> ReplayEngine {
    ReplayEngine::new(
        "2026-01-02T14:30:00Z",
        "engine-v1",
        "cfg-v1",
        RiskPolicy {
            version: "risk-v1".to_owned(),
            global_kill_switch: false,
            max_quantity: amount("10"),
            max_notional: amount("10000"),
            max_price_deviation_bps: amount("500"),
            max_news_slippage_bps: None,
            max_news_spread_multiplier_bps: None,
        },
        DeterministicFillModel {
            spread_bps: Decimal::ZERO,
            slippage_bps: Decimal::ZERO,
            flat_fee: amount("0.10"),
            latency_bars: 0,
            max_fill_quantity: None,
        },
    )
    .unwrap()
}

/// Buys one share of `instrument` on the first bar it sees of it and does nothing else.
pub struct BuyOnce {
    pub instrument: &'static str,
    pub bought: bool,
}

impl BuyOnce {
    pub fn new(instrument: &'static str) -> Self {
        Self {
            instrument,
            bought: false,
        }
    }
}

impl Strategy for BuyOnce {
    fn on_bar(&mut self, bar: &Bar, replay_time: &str) -> Result<Option<OrderIntent>, EngineError> {
        if self.bought || bar.instrument_id != self.instrument {
            return Ok(None);
        }
        self.bought = true;
        Ok(Some(OrderIntent {
            intent_id: "intent-buy-once".to_owned(),
            account_id: ACCOUNT.to_owned(),
            strategy_id: "strategy-split-001".to_owned(),
            instrument_id: bar.instrument_id.clone(),
            correlation_id: "corr-buy-once".to_owned(),
            side: Side::Buy,
            quantity: Decimal::from_integer(1)?,
            order_type: OrderType::Market,
            limit_price: None,
            time_in_force: TimeInForce::Day,
            rationale: "corporate action valuation regression".to_owned(),
            created_at: replay_time.to_owned(),
            strategy_version: "strategy-split-v1".to_owned(),
            configuration_version: "cfg-v1".to_owned(),
            environment: "SIMULATION".to_owned(),
        }))
    }
}

/// Runs `input`, whose bars may name several instruments, with `strategy`.
pub fn run_input(
    strategy: &mut impl Strategy,
    input: &BacktestInput,
    instrument_ids: &[&str],
) -> Result<CompletedBacktest, BacktestError> {
    let spec = spec(input);
    let (instruments, calendar) = market_of(instrument_ids);
    let market = MarketPreconditions {
        instruments: &instruments,
        calendar: &calendar,
    };
    BacktestRunner::new(spec, engine())
        .unwrap()
        .run(strategy, input, &market)
}

/// Runs the five bars with `strategy` and the corporate `actions`.
pub fn run_with(
    strategy: &mut impl Strategy,
    actions: Vec<CorporateAction>,
) -> Result<CompletedBacktest, BacktestError> {
    let input = input(actions);
    let spec = spec(&input);
    let (instruments, calendar) = market();
    let market = MarketPreconditions {
        instruments: &instruments,
        calendar: &calendar,
    };
    BacktestRunner::new(spec, engine())
        .unwrap()
        .run(strategy, &input, &market)
}

pub fn events(completed: &CompletedBacktest) -> Vec<serde_json::Value> {
    completed
        .canonical_events
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

pub fn positions(events: &[serde_json::Value]) -> Vec<&serde_json::Value> {
    events
        .iter()
        .filter(|event| event["event_type"] == "portfolio.position_updated.v1")
        .collect()
}

pub fn decimal_of(value: &serde_json::Value) -> Decimal {
    amount(value.as_str().unwrap())
}
