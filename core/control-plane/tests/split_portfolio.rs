//! The replay engine's portfolio follows a stock split (delivery state E8.2).
//!
//! The backtest ledger applied a split to its own book; the engine's portfolio, the one a
//! strategy's execution callbacks and the event stream project, did not. These tests hold
//! the engine's half: the scaling is exact, it changes nothing when it must refuse, it
//! applies once, and it records what it did.

use std::collections::BTreeMap;
use std::str::FromStr;

use follon_control_plane::{
    DeterministicFillModel, EngineError, EventSink, InMemoryEventStore, MarketPreconditions,
    Portfolio, ReplayEngine, RiskPolicy, Strategy,
};
use follon_domain::{
    Bar, Decimal, EventEnvelope, EventPayload, Fill, OrderIntent, OrderType, Side, TimeInForce,
};
use follon_instrument::{
    AssetClass, Instrument, InstrumentRegistry, InstrumentVersion, StaticTradingCalendar,
    TradingSession,
};

const SPY: &str = "inst.us_equity.spy";
const QQQ: &str = "inst.us_equity.qqq";
// Trades in a lot of the smallest quantity a decimal holds, so a dust position can exist.
const DUST: &str = "inst.us_equity.dust";

fn amount(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

fn fill(side: Side, quantity: &str, price: &str, fee: &str) -> Fill {
    Fill {
        execution_id: "execution.split.001".to_owned(),
        order_id: "order.split.001".to_owned(),
        instrument_id: SPY.to_owned(),
        side,
        quantity: amount(quantity),
        price: amount(price),
        fee: amount(fee),
        executed_at: "2026-01-02T14:30:00Z".to_owned(),
    }
}

// The portfolio on its own.

#[test]
fn a_split_multiplies_the_quantity_and_divides_the_cost_and_nothing_else_moves() {
    let mut portfolio = Portfolio::new("acct-paper-001", SPY);
    portfolio
        .apply_fill(&fill(Side::Buy, "3", "100", "0.30"))
        .unwrap();
    portfolio
        .apply_fill(&fill(Side::Sell, "1", "110", "0.10"))
        .unwrap();
    let before = portfolio.position_snapshot();
    assert_eq!(before.quantity, amount("2"));
    assert_eq!(before.average_cost, amount("100.10"));
    assert_eq!(before.realized_pnl, amount("9.80"));
    let unrealized_before = portfolio
        .pnl_snapshot(amount("110"))
        .unwrap()
        .unrealized_pnl;

    portfolio.apply_split(amount("2")).unwrap();

    let after = portfolio.position_snapshot();
    assert_eq!(after.quantity, amount("4"));
    assert_eq!(after.average_cost, amount("50.05"));
    // Realized P&L is history and does not move. The position's total cost does not
    // either, and neither does its unrealized P&L at the same value, priced per new share.
    assert_eq!(after.realized_pnl, before.realized_pnl);
    assert_eq!(
        after.quantity.checked_mul(after.average_cost).unwrap(),
        before.quantity.checked_mul(before.average_cost).unwrap()
    );
    assert_eq!(
        portfolio.pnl_snapshot(amount("55")).unwrap().unrealized_pnl,
        unrealized_before
    );
    assert_eq!(after.account_id, before.account_id);
    assert_eq!(after.instrument_id, before.instrument_id);
}

#[test]
fn a_reverse_split_and_its_inverse_return_the_position() {
    let mut portfolio = Portfolio::new("acct-paper-001", SPY);
    portfolio
        .apply_fill(&fill(Side::Buy, "4", "100", "0.40"))
        .unwrap();
    let start = portfolio.position_snapshot();

    portfolio.apply_split(amount("0.5")).unwrap();
    assert_eq!(portfolio.position_snapshot().quantity, amount("2"));
    assert_eq!(portfolio.position_snapshot().average_cost, amount("200.20"));
    portfolio.apply_split(amount("2")).unwrap();
    assert_eq!(portfolio.position_snapshot().quantity, start.quantity);
    assert_eq!(
        portfolio.position_snapshot().average_cost,
        start.average_cost
    );
}

#[test]
fn a_flat_position_is_unchanged_by_a_split() {
    let mut portfolio = Portfolio::new("acct-paper-001", SPY);
    portfolio.apply_split(amount("3")).unwrap();
    let position = portfolio.position_snapshot();
    assert_eq!(position.quantity, Decimal::ZERO);
    assert_eq!(position.average_cost, Decimal::ZERO);
    assert_eq!(position.realized_pnl, Decimal::ZERO);
}

#[test]
fn a_short_position_scales_like_a_long_one() {
    let mut portfolio = Portfolio::new("acct-paper-001", SPY);
    portfolio
        .apply_signed_fill(&fill(Side::Sell, "4", "5", "0"))
        .unwrap();
    assert_eq!(portfolio.position_snapshot().quantity, amount("-4"));
    assert_eq!(portfolio.position_snapshot().average_cost, amount("5"));

    portfolio.apply_split(amount("2")).unwrap();

    // Twice the shares short, each opened for half the proceeds.
    assert_eq!(portfolio.position_snapshot().quantity, amount("-8"));
    assert_eq!(portfolio.position_snapshot().average_cost, amount("2.5"));
}

#[test]
fn a_split_ratio_must_be_positive_and_a_refusal_changes_nothing() {
    let mut portfolio = Portfolio::new("acct-paper-001", SPY);
    portfolio
        .apply_fill(&fill(Side::Buy, "3", "100", "0.30"))
        .unwrap();
    let before = portfolio.position_snapshot();
    for ratio in ["0", "-2"] {
        let error = portfolio.apply_split(amount(ratio)).unwrap_err();
        assert_eq!(error.0, "split ratio must be positive", "ratio {ratio}");
        let after = portfolio.position_snapshot();
        assert_eq!(after.quantity, before.quantity);
        assert_eq!(after.average_cost, before.average_cost);
    }
}

#[test]
fn a_split_that_rounds_a_held_position_to_nothing_is_refused_and_changes_nothing() {
    // One unit of the smallest quantity, halved, is a quantity the decimal cannot
    // hold. The portfolio refuses rather than keep a cost with no quantity behind it.
    let mut portfolio = Portfolio::recover(
        "acct-paper-001",
        SPY,
        amount("0.00000001"),
        amount("100"),
        Decimal::ZERO,
    )
    .unwrap();
    let error = portfolio.apply_split(amount("0.5")).unwrap_err();
    assert_eq!(
        error.0,
        "split would round the held position down to nothing"
    );
    assert_eq!(portfolio.position_snapshot().quantity, amount("0.00000001"));
    assert_eq!(portfolio.position_snapshot().average_cost, amount("100"));
}

// The engine.

struct Scripted {
    account: &'static str,
    tag: &'static str,
    /// The orders it places, in order, each on the first bar of its instrument.
    script: Vec<(&'static str, Side, &'static str, Option<&'static str>)>,
    next: usize,
}

impl Scripted {
    fn new(
        account: &'static str,
        tag: &'static str,
        script: Vec<(&'static str, Side, &'static str, Option<&'static str>)>,
    ) -> Self {
        Self {
            account,
            tag,
            script,
            next: 0,
        }
    }
}

impl Strategy for Scripted {
    fn on_bar(&mut self, bar: &Bar, replay_time: &str) -> Result<Option<OrderIntent>, EngineError> {
        let Some(&(instrument, side, quantity, limit)) = self.script.get(self.next) else {
            return Ok(None);
        };
        if bar.instrument_id != instrument {
            return Ok(None);
        }
        self.next += 1;
        Ok(Some(OrderIntent {
            intent_id: format!("intent-{}-{}", self.tag, self.next),
            account_id: self.account.to_owned(),
            strategy_id: format!("strategy-{}", self.tag),
            instrument_id: instrument.to_owned(),
            correlation_id: format!("corr-{}-{}", self.tag, self.next),
            side,
            quantity: amount(quantity),
            order_type: if limit.is_some() {
                OrderType::Limit
            } else {
                OrderType::Market
            },
            limit_price: limit.map(amount),
            time_in_force: TimeInForce::Day,
            rationale: "split portfolio regression".to_owned(),
            created_at: replay_time.to_owned(),
            strategy_version: "strategy-split-v1".to_owned(),
            configuration_version: "cfg-v1".to_owned(),
            environment: "SIMULATION".to_owned(),
        }))
    }
}

fn bar(instrument: &str) -> Bar {
    Bar {
        instrument_id: instrument.to_owned(),
        open: amount("100"),
        high: amount("101"),
        low: amount("99"),
        close: amount("100"),
        volume: amount("1000"),
        interval_seconds: 60,
        exchange_timezone: "America/New_York".to_owned(),
    }
}

fn market() -> (InstrumentRegistry, StaticTradingCalendar) {
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
    for (id, symbol, lot) in [
        (SPY, "SPY", "1"),
        (QQQ, "QQQ", "1"),
        (DUST, "DUST", "0.00000001"),
    ] {
        instruments
            .register(InstrumentVersion {
                instrument: Instrument {
                    instrument_id: id.to_owned(),
                    symbol: symbol.to_owned(),
                    exchange_symbol: symbol.to_owned(),
                    asset_class: AssetClass::Etf,
                    venue: "venue.nyse_arca".to_owned(),
                    currency: "USD".to_owned(),
                    broker_ids: BTreeMap::new(),
                    tick_size: amount("0.01"),
                    lot_size: amount(lot),
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

/// A replay whose bars and splits, in the order they happen, are one minute apart from
/// 14:30. A split moves the engine's clock to the time it is applied, so a later bar must
/// come after it, and the harness keeps one counter for both.
struct Replay {
    engine: ReplayEngine,
    store: InMemoryEventStore,
    instruments: InstrumentRegistry,
    calendar: StaticTradingCalendar,
    minute: u32,
}

impl Replay {
    fn new() -> Self {
        let (instruments, calendar) = market();
        Self {
            engine: ReplayEngine::new(
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
            .unwrap(),
            store: InMemoryEventStore::default(),
            instruments,
            calendar,
            minute: 0,
        }
    }

    /// The time the next bar or split takes.
    fn next_time(&self) -> String {
        let minutes = 30 + self.minute;
        format!(
            "2026-01-02T{:02}:{:02}:00Z",
            14 + minutes / 60,
            minutes % 60
        )
    }

    /// Processes one bar of `instrument` for `strategy`'s account.
    fn bar(&mut self, strategy: &mut Scripted, instrument: &str) {
        let time = self.next_time();
        self.minute += 1;
        let market = MarketPreconditions {
            instruments: &self.instruments,
            calendar: &self.calendar,
        };
        let account = strategy.account;
        self.engine
            .process_bar_with_market_preconditions(
                &mut self.store,
                strategy,
                account,
                &time,
                bar(instrument),
                &market,
            )
            .unwrap();
    }

    /// The strategy's order is placed on one bar and fills on the next.
    fn fills(&mut self, strategy: &mut Scripted, instrument: &str) {
        self.bar(strategy, instrument);
        self.bar(strategy, instrument);
    }

    /// Applies a split at the next time.
    fn split(
        &mut self,
        action_id: &str,
        instrument: &str,
        ratio: &str,
    ) -> Result<Vec<EventEnvelope>, EngineError> {
        let applied_at = self.next_time();
        self.minute += 1;
        self.split_at(action_id, instrument, ratio, &applied_at)
    }

    /// Applies a split at a time of the caller's choosing, which the harness does not track.
    fn split_at(
        &mut self,
        action_id: &str,
        instrument: &str,
        ratio: &str,
        applied_at: &str,
    ) -> Result<Vec<EventEnvelope>, EngineError> {
        self.engine.apply_split(
            &mut self.store,
            action_id,
            instrument,
            amount(ratio),
            applied_at,
        )
    }

    fn event_count(&self) -> usize {
        self.store.events().len()
    }
}

fn position_of(event: &EventEnvelope) -> (String, String, Decimal, Decimal) {
    match &event.payload {
        EventPayload::Position(position) => (
            position.account_id.clone(),
            position.instrument_id.clone(),
            position.quantity,
            position.average_cost,
        ),
        other => panic!("expected a position event, found {other:?}"),
    }
}

#[test]
fn a_split_scales_every_holder_of_the_instrument_in_a_stable_order_and_no_other() {
    let mut replay = Replay::new();
    // Acquired in the opposite order to the accounts' sort order, and holding a second
    // instrument, so neither the order nor the instrument filter can be an accident. The
    // flat 0.10 fee divides evenly by each quantity, so every average is exact:
    // 200.10 / 2 = 100.05, 400.10 / 4 = 100.025 and 500.10 / 5 = 100.02.
    let mut second = Scripted::new(
        "acct-b",
        "b",
        vec![(SPY, Side::Buy, "4", None), (QQQ, Side::Buy, "5", None)],
    );
    let mut first = Scripted::new("acct-a", "a", vec![(SPY, Side::Buy, "2", None)]);
    replay.fills(&mut second, SPY);
    replay.fills(&mut second, QQQ);
    replay.fills(&mut first, SPY);

    let applied_at = replay.next_time();
    let events = replay.split("action-split-spy-001", SPY, "2").unwrap();

    let positions: Vec<_> = events.iter().map(position_of).collect();
    assert_eq!(
        positions,
        vec![
            (
                "acct-a".to_owned(),
                SPY.to_owned(),
                amount("4"),
                amount("50.025")
            ),
            (
                "acct-b".to_owned(),
                SPY.to_owned(),
                amount("8"),
                amount("50.0125")
            ),
        ]
    );
    for (event, account) in events.iter().zip(["acct-a", "acct-b"]) {
        assert_eq!(event.account_id.as_deref(), Some(account));
        assert_eq!(event.event_type, "portfolio.position_updated.v1");
        assert_eq!(event.actor, "portfolio_engine");
        assert_eq!(event.source, "corporate_action");
        assert_eq!(
            event.correlation_id,
            "corr-corporate-action-action-split-spy-001"
        );
        assert_eq!(event.causation_id, None);
        assert_eq!(event.strategy_id, None);
        assert_eq!(event.instrument_id.as_deref(), Some(SPY));
        assert_eq!(event.event_time, applied_at);
        assert_eq!(event.receive_time, applied_at);
    }
    // The events are in the store, appended once and validated, and are its last two.
    let stored = replay.store.events();
    assert_eq!(&stored[stored.len() - 2..], events.as_slice());
    assert_eq!(replay.engine.clock.now(), applied_at);

    // The other instrument did not move: a later split of it finds its five shares.
    let qqq = replay.split("action-split-qqq-001", QQQ, "3").unwrap();
    assert_eq!(
        qqq.iter().map(position_of).collect::<Vec<_>>(),
        vec![(
            "acct-b".to_owned(),
            QQQ.to_owned(),
            amount("15"),
            amount("33.34")
        )]
    );
}

#[test]
fn a_split_records_nothing_for_an_account_that_holds_nothing_or_is_flat() {
    let mut replay = Replay::new();
    let count = replay.event_count();
    assert!(replay
        .split("action-split-001", SPY, "2")
        .unwrap()
        .is_empty());
    assert_eq!(replay.event_count(), count);

    // Held, then sold out: the account has a portfolio, and it is flat.
    let mut flat = Scripted::new(
        "acct-a",
        "a",
        vec![(SPY, Side::Buy, "2", None), (SPY, Side::Sell, "2", None)],
    );
    replay.fills(&mut flat, SPY);
    replay.fills(&mut flat, SPY);
    let count = replay.event_count();
    assert!(replay
        .split("action-split-002", SPY, "2")
        .unwrap()
        .is_empty());
    assert_eq!(replay.event_count(), count);
}

#[test]
fn an_action_applies_once_and_a_repeat_changes_nothing() {
    let mut replay = Replay::new();
    let mut holder = Scripted::new("acct-a", "a", vec![(SPY, Side::Buy, "2", None)]);
    replay.fills(&mut holder, SPY);
    replay.split("action-split-001", SPY, "2").unwrap();
    let count = replay.event_count();

    let error = replay.split("action-split-001", SPY, "2").unwrap_err();
    assert!(error.0.contains("already applied"), "{}", error.0);
    assert_eq!(replay.event_count(), count);
    // Not scaled twice: the next distinct split finds four shares, not eight.
    let events = replay.split("action-split-002", SPY, "2").unwrap();
    assert_eq!(position_of(&events[0]).2, amount("8"));
}

#[test]
fn a_split_is_refused_while_an_order_rests_in_the_instrument_and_nothing_changes() {
    let mut replay = Replay::new();
    let mut holder = Scripted::new("acct-a", "a", vec![(SPY, Side::Buy, "2", None)]);
    replay.fills(&mut holder, SPY);
    // A buy limit below the bar's low can never fill: it rests.
    let mut resting = Scripted::new("acct-b", "b", vec![(SPY, Side::Buy, "1", Some("96.00"))]);
    replay.bar(&mut resting, SPY);
    let count = replay.event_count();
    let clock = replay.engine.clock.now().to_owned();

    let error = replay.split("action-split-001", SPY, "2").unwrap_err();
    assert!(
        error
            .0
            .contains("cannot rest across inst.us_equity.spy's split action-split-001"),
        "{}",
        error.0
    );
    // Nothing was recorded, the clock did not move, and the position was not scaled.
    assert_eq!(replay.event_count(), count);
    assert_eq!(replay.engine.clock.now(), clock);
    // The refusal did not consume the action: the same identity applies to another
    // instrument, and the held position is still two shares, not four.
    assert!(replay
        .split("action-split-001", QQQ, "2")
        .unwrap()
        .is_empty());
    let mut bystander = Scripted::new("acct-a", "a2", vec![(SPY, Side::Sell, "2", None)]);
    replay.fills(&mut bystander, SPY);
    let sold = replay
        .store
        .events()
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            EventPayload::Position(position) if position.account_id == "acct-a" => {
                Some(position.clone())
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        sold.quantity,
        Decimal::ZERO,
        "two shares were held and two sold"
    );
}

#[test]
fn an_order_resting_in_another_instrument_does_not_block_a_split() {
    let mut replay = Replay::new();
    let mut holder = Scripted::new("acct-a", "a", vec![(SPY, Side::Buy, "2", None)]);
    replay.fills(&mut holder, SPY);
    let mut resting = Scripted::new("acct-b", "b", vec![(QQQ, Side::Buy, "1", Some("96.00"))]);
    replay.bar(&mut resting, QQQ);

    let events = replay.split("action-split-001", SPY, "2").unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(position_of(&events[0]).2, amount("4"));
}

#[test]
fn a_split_one_holder_cannot_take_is_refused_for_every_holder_and_nothing_changes() {
    let mut replay = Replay::new();
    // acct-a holds a whole share of DUST, which halves without trouble. acct-b holds the
    // smallest quantity a decimal can, which halving would round to nothing. acct-a sorts
    // first, so a split applied holder by holder would have scaled it, and recorded it,
    // before acct-b refused.
    let mut whole = Scripted::new("acct-a", "a", vec![(DUST, Side::Buy, "1", None)]);
    let mut dust = Scripted::new("acct-b", "b", vec![(DUST, Side::Buy, "0.00000001", None)]);
    replay.fills(&mut whole, DUST);
    replay.fills(&mut dust, DUST);
    let count = replay.event_count();
    let clock = replay.engine.clock.now().to_owned();

    let error = replay.split("action-reverse-001", DUST, "0.5").unwrap_err();
    assert_eq!(
        error.0,
        "split would round the held position down to nothing"
    );
    assert_eq!(replay.event_count(), count);
    assert_eq!(replay.engine.clock.now(), clock);

    // Neither holder was scaled and the identity was not consumed: a 1:1 split under the
    // same identity reports both positions as they were.
    let events = replay.split("action-reverse-001", DUST, "1").unwrap();
    assert_eq!(
        events
            .iter()
            .map(position_of)
            .map(|(account, _, quantity, _)| (account, quantity))
            .collect::<Vec<_>>(),
        vec![
            ("acct-a".to_owned(), amount("1")),
            ("acct-b".to_owned(), amount("0.00000001"))
        ]
    );
}

#[test]
fn a_malformed_split_is_refused_before_anything_changes() {
    let mut replay = Replay::new();
    let mut holder = Scripted::new("acct-a", "a", vec![(SPY, Side::Buy, "2", None)]);
    replay.fills(&mut holder, SPY);
    let count = replay.event_count();
    let clock = replay.engine.clock.now().to_owned();

    let later = replay.next_time();
    for (name, action, instrument, ratio, applied_at) in [
        ("a zero ratio", "action-split-001", SPY, "0", later.as_str()),
        (
            "a negative ratio",
            "action-split-001",
            SPY,
            "-2",
            later.as_str(),
        ),
        // No holder's own scaling would refuse these, so only the engine's check does.
        (
            "a zero ratio for an instrument nobody holds",
            "action-split-001",
            QQQ,
            "0",
            later.as_str(),
        ),
        (
            "a negative ratio for an instrument nobody holds",
            "action-split-001",
            QQQ,
            "-2",
            later.as_str(),
        ),
        (
            "an action id that is not canonical",
            "Not Canonical",
            SPY,
            "2",
            later.as_str(),
        ),
        (
            "an instrument that is not canonical",
            "action-split-001",
            "Not Canonical",
            "2",
            later.as_str(),
        ),
        (
            "a time before the replay's own",
            "action-split-001",
            SPY,
            "2",
            "2026-01-02T14:00:00Z",
        ),
        (
            "a time that is not UTC",
            "action-split-001",
            SPY,
            "2",
            "2026-01-02 15:00:00",
        ),
    ] {
        assert!(
            replay
                .split_at(action, instrument, ratio, applied_at)
                .is_err(),
            "{name} was accepted"
        );
        assert_eq!(replay.event_count(), count, "{name}");
        assert_eq!(replay.engine.clock.now(), clock, "{name}");
    }
    // None of them consumed the identity or touched the position.
    let events = replay.split("action-split-001", SPY, "2").unwrap();
    assert_eq!(position_of(&events[0]).2, amount("4"));
}

/// A sink that refuses every event, to hold that the engine reports a sink's failure.
struct RefusingSink;

impl EventSink for RefusingSink {
    fn append(&mut self, _event: &EventEnvelope) -> Result<(), EngineError> {
        Err(EngineError("the sink is refusing".to_owned()))
    }
}

#[test]
fn a_sinks_refusal_is_reported_and_not_swallowed() {
    let mut replay = Replay::new();
    let mut holder = Scripted::new("acct-a", "a", vec![(SPY, Side::Buy, "2", None)]);
    replay.fills(&mut holder, SPY);
    let applied_at = replay.next_time();
    let error = replay
        .engine
        .apply_split(
            &mut RefusingSink,
            "action-split-001",
            SPY,
            amount("2"),
            &applied_at,
        )
        .unwrap_err();
    assert_eq!(error.0, "the sink is refusing");
}
