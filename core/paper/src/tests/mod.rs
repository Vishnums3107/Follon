//! Unit tests for the PAPER OMS, risk gate, journal and reconciliation.
//!
//! Shared fixtures live here; the tests themselves are grouped by topic in the child modules.

use super::*;
use follon_control_plane::OmsComboOrder;
use follon_domain::{ComboIntent, Decimal, OrderIntent, OrderState, OrderType, Side, TimeInForce};
use follon_instrument::StaticTradingCalendar;
use follon_instrument::TradingSession;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

include!("../combo_lifecycle_tests.rs");

mod combo_orders;
mod combo_risk;
mod instrument_grid;
mod journal;
mod oms;
mod operators;
mod portfolio_risk;
mod risk;
mod routes;
mod tax_lots;

fn account() -> PaperAccount {
    PaperAccount {
        account_id: "acct.paper.001".to_owned(),
        currency: "USD".to_owned(),
        initial_cash: decimal("initial cash", "100000").unwrap(),
        environment: "PAPER".to_owned(),
    }
}

fn policy() -> PaperRiskPolicy {
    PaperRiskPolicy {
        version: "paper-risk-v1".to_owned(),
        trading_calendar_id: "cal.us_equities.nyse.v1".to_owned(),
        max_order_quantity: decimal("quantity", "100").unwrap(),
        max_order_notional: decimal("notional", "50000").unwrap(),
        max_price_deviation_bps: decimal("price collar", "100").unwrap(),
        max_open_orders: 10,
        max_position_quantity: decimal("position", "1000").unwrap(),
        max_realized_loss: decimal("loss", "10000").unwrap(),
        max_market_data_age_seconds: 5,
        max_order_rate: 20,
        order_rate_window_seconds: 60,
        portfolio_risk: None,
        short_exposure: None,
        instrument_tick_sizes: test_tick_sizes(),
        instrument_lot_sizes: test_lot_sizes(),
    }
}

fn test_tick_sizes() -> BTreeMap<String, Decimal> {
    [
        "inst.us_equity.spy",
        "inst.us_equity.qqq",
        "inst.us_option.spy.near",
        "inst.us_option.spy.far",
    ]
    .into_iter()
    .map(|instrument| (instrument.to_owned(), decimal("tick", "0.01").unwrap()))
    .collect()
}

/// A lot of one for each listed instrument: every whole quantity passes,
/// so only a test that sets a coarser lot exercises the lot rule.
fn test_lot_sizes() -> BTreeMap<String, Decimal> {
    test_tick_sizes()
        .into_keys()
        .map(|instrument| (instrument, decimal("lot", "1").unwrap()))
        .collect()
}

/// The same policy with net short exposure explicitly permitted, bounded at
/// 1,000 per instrument. Combination tests that need a short leg use this;
/// none of them may quietly widen `policy()` instead.
fn policy_permitting_shorts() -> PaperRiskPolicy {
    PaperRiskPolicy {
        short_exposure: Some(ShortExposurePolicy {
            max_short_quantity: decimal("short bound", "1000").unwrap(),
        }),
        ..policy()
    }
}

fn service_permitting_shorts() -> PaperTradingService<IbkrPaperAdapter> {
    let account = account();
    let adapter = IbkrPaperAdapter::new(&account).unwrap();
    PaperTradingService::new(
        account,
        policy_permitting_shorts(),
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        adapter,
    )
    .unwrap()
}

fn service() -> PaperTradingService<IbkrPaperAdapter> {
    service_with(policy())
}

fn service_with(policy: PaperRiskPolicy) -> PaperTradingService<IbkrPaperAdapter> {
    let account = account();
    let adapter = IbkrPaperAdapter::new(&account).unwrap();
    PaperTradingService::new(
        account,
        policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        adapter,
    )
    .unwrap()
}

fn intent(intent_id: &str, created_at: &str) -> OrderIntent {
    OrderIntent {
        intent_id: intent_id.to_owned(),
        account_id: "acct.paper.001".to_owned(),
        strategy_id: "strategy.paper.001".to_owned(),
        instrument_id: "inst.us_equity.spy".to_owned(),
        correlation_id: format!("corr-{intent_id}"),
        side: Side::Buy,
        quantity: decimal("quantity", "1").unwrap(),
        order_type: OrderType::Market,
        limit_price: None,
        time_in_force: TimeInForce::Day,
        rationale: "paper acceptance test".to_owned(),
        created_at: created_at.to_owned(),
        strategy_version: "strategy-paper-v1".to_owned(),
        configuration_version: "config-paper-v1".to_owned(),
        environment: "PAPER".to_owned(),
    }
}

fn market(observed_at: &str) -> PaperMarketData {
    PaperMarketData {
        instrument_id: "inst.us_equity.spy".to_owned(),
        mark_price: decimal("mark", "100").unwrap(),
        observed_at: observed_at.to_owned(),
    }
}

fn market_at_price(price: &str, observed_at: &str) -> PaperMarketData {
    PaperMarketData {
        instrument_id: "inst.us_equity.spy".to_owned(),
        mark_price: decimal("mark", price).unwrap(),
        observed_at: observed_at.to_owned(),
    }
}

/// A long call vertical on two distinct option instruments: buy the near
/// strike at 7.50, sell the far strike at 5.00, for a 2.50 net debit per
/// combination unit.
fn combo_intent(intent_id: &str, created_at: &str) -> ComboIntent {
    ComboIntent {
        intent_id: intent_id.to_owned(),
        account_id: "acct.paper.001".to_owned(),
        strategy_id: "strategy.paper.001".to_owned(),
        correlation_id: format!("corr-{intent_id}"),
        legs: vec![
            follon_domain::ComboIntentLeg {
                instrument_id: "inst.us_option.spy.near".to_owned(),
                side: Side::Buy,
                ratio: 1,
                limit_price: decimal("near", "7.50").unwrap(),
            },
            follon_domain::ComboIntentLeg {
                instrument_id: "inst.us_option.spy.far".to_owned(),
                side: Side::Sell,
                ratio: 1,
                limit_price: decimal("far", "5").unwrap(),
            },
        ],
        combo_quantity: decimal("units", "4").unwrap(),
        price_limit: follon_domain::ComboPriceLimit::MaximumDebit(decimal("cap", "2.50").unwrap()),
        time_in_force: TimeInForce::Day,
        rationale: "paper combo acceptance test".to_owned(),
        created_at: created_at.to_owned(),
        strategy_version: "strategy-paper-v1".to_owned(),
        configuration_version: "config-paper-v1".to_owned(),
        environment: "PAPER".to_owned(),
    }
}

/// Marks that sit exactly on each leg's own limit price, so the per-leg
/// price collar reads zero deviation unless a test moves one.
fn combo_market(observed_at: &str) -> PaperComboMarketData {
    PaperComboMarketData {
        marks: vec![
            PaperMarketData {
                instrument_id: "inst.us_option.spy.near".to_owned(),
                mark_price: decimal("mark", "7.50").unwrap(),
                observed_at: observed_at.to_owned(),
            },
            PaperMarketData {
                instrument_id: "inst.us_option.spy.far".to_owned(),
                mark_price: decimal("mark", "5").unwrap(),
                observed_at: observed_at.to_owned(),
            },
        ],
    }
}

fn paper_session(exchange_date: &str) -> PaperTradingSession {
    let (opens_at, closes_at) = if exchange_date >= "2026-03-09" {
        ("13:30:00Z", "20:00:00Z")
    } else {
        ("14:30:00Z", "21:00:00Z")
    };
    PaperTradingSession {
        calendar_id: "cal.us_equities.nyse.v1".to_owned(),
        session: TradingSession {
            exchange_date: exchange_date.to_owned(),
            opens_at: format!("{exchange_date}T{opens_at}"),
            closes_at: format!("{exchange_date}T{closes_at}"),
        },
    }
}

fn paper_calendar(sessions: &[PaperTradingSession]) -> StaticTradingCalendar {
    StaticTradingCalendar::new(
        "cal.us_equities.nyse.v1",
        sessions.iter().map(|value| value.session.clone()).collect(),
    )
    .expect("test paper calendar")
}
