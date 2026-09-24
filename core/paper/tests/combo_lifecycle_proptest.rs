//! Model-based property test for the PAPER atomic-combination lifecycle.
//!
//! Generates arbitrary 2-4 leg combinations (any sides, ratios 1-3, debit or
//! credit), drives each one through a random sequence of whole-unit atomic
//! fills, replays of already-applied evidence, and an optional cancellation,
//! and after every step checks the service against an independent model
//! written here from the contract, not from `core/paper`'s implementation:
//!
//! - filled units equal the sum of distinct applied executions, never more;
//! - the lifecycle state follows from filled units and cancellation alone;
//! - every leg's position is `side * ratio * filled units`;
//! - cash moves by exactly each leg's signed gross plus its fee;
//! - a working combination counts as one working order, a finished one as none;
//! - replaying any earlier execution changes nothing at all;
//! - reconciliation against the broker's own view is clean after every step.
//!
//! Only the crate's public API is used, so this also pins what an operator
//! surface (the desktop gateway, the gRPC route) can observe.

use std::str::FromStr;

use follon_domain::{
    ComboIntent, ComboIntentLeg, ComboPriceLimit, Decimal, OrderState, Side, TimeInForce,
};
use follon_paper::{
    BrokerAccountSnapshot, BrokerCancelRequest, BrokerComboExecution, BrokerComboExecutionLeg,
    BrokerComboRequest, BrokerEvent, BrokerOrderRequest, BrokerSubmitResult, IbkrPaperAdapter,
    KillSwitchRegistry, PaperAccount, PaperBrokerAdapter, PaperComboMarketData, PaperError,
    PaperMarketData, PaperRiskPolicy, PaperTradingService, ShortExposurePolicy,
};
use proptest::prelude::*;

const ACCOUNT: &str = "acct.paper.proptest";
const INITIAL_CASH: &str = "100000";
const DECIDED_AT: &str = "2026-01-02T14:30:00Z";
const EXECUTED_AT: &str = "2026-01-02T14:31:00Z";

fn dec(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

fn cents(value: u32) -> Decimal {
    dec(&format!("{}.{:02}", value / 100, value % 100))
}

/// The real paper model, plus the ability to re-deliver an execution the OMS
/// has already consumed -- exactly what a reconnecting broker may do.
struct ReplayingAdapter {
    inner: IbkrPaperAdapter,
    redeliver: Vec<BrokerEvent>,
}

impl PaperBrokerAdapter for ReplayingAdapter {
    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError> {
        self.inner.submit(request)
    }
    fn submit_combo(
        &mut self,
        request: &BrokerComboRequest,
    ) -> Result<BrokerSubmitResult, PaperError> {
        self.inner.submit_combo(request)
    }
    fn cancel(&mut self, request: &BrokerCancelRequest) -> Result<(), PaperError> {
        self.inner.cancel(request)
    }
    fn poll(&mut self, account_id: &str) -> Result<Vec<BrokerEvent>, PaperError> {
        let mut events = self.inner.poll(account_id)?;
        events.append(&mut self.redeliver);
        Ok(events)
    }
    fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
        self.inner.snapshot(account_id)
    }
    fn reconnect(&mut self, account_id: &str) -> Result<(), PaperError> {
        self.inner.reconnect(account_id)
    }
}

#[derive(Clone, Debug)]
struct GeneratedLeg {
    buy: bool,
    ratio: u32,
    price_cents: u32,
    fee_cents: u32,
}

#[derive(Clone, Debug)]
enum Step {
    /// Fill this many whole units, clamped to what remains.
    Fill(u32),
    /// Re-deliver the n-th applied execution (modulo how many exist).
    Replay(usize),
    Cancel,
}

fn legs_strategy() -> impl Strategy<Value = Vec<GeneratedLeg>> {
    prop::collection::vec(
        (any::<bool>(), 1u32..=3, 100u32..=5_000, 0u32..=150).prop_map(
            |(buy, ratio, price_cents, fee_cents)| GeneratedLeg {
                buy,
                ratio,
                price_cents,
                fee_cents,
            },
        ),
        2..=4,
    )
}

fn steps_strategy() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(
        prop_oneof![
            4 => (1u32..=3).prop_map(Step::Fill),
            2 => any::<usize>().prop_map(Step::Replay),
            1 => Just(Step::Cancel),
        ],
        1..=8,
    )
}

fn service() -> PaperTradingService<ReplayingAdapter> {
    let account = PaperAccount {
        account_id: ACCOUNT.to_owned(),
        currency: "USD".to_owned(),
        initial_cash: dec(INITIAL_CASH),
        environment: "PAPER".to_owned(),
    };
    let policy = PaperRiskPolicy {
        version: "paper-risk-proptest-v1".to_owned(),
        trading_calendar_id: "cal.us_equities.nyse.v1".to_owned(),
        max_order_quantity: dec("100"),
        max_order_notional: dec("50000"),
        max_price_deviation_bps: dec("100"),
        max_open_orders: 10,
        max_position_quantity: dec("1000"),
        max_realized_loss: dec("10000"),
        max_market_data_age_seconds: 5,
        max_order_rate: 20,
        order_rate_window_seconds: 60,
        portfolio_risk: None,
        short_exposure: Some(ShortExposurePolicy {
            max_short_quantity: dec("1000"),
        }),
        // Combinations are not tick-checked; plain-order rules need a listing.
        instrument_tick_sizes: std::collections::BTreeMap::from([(
            "inst.us_equity.spy".to_owned(),
            dec("0.01"),
        )]),
    };
    let adapter = ReplayingAdapter {
        inner: IbkrPaperAdapter::new(&account).unwrap(),
        redeliver: Vec::new(),
    };
    PaperTradingService::new(
        account,
        policy,
        KillSwitchRegistry::new("paper-kills-v1").unwrap(),
        adapter,
    )
    .unwrap()
}

fn instrument(index: usize) -> String {
    format!("inst.us_option.proptest.leg{index}")
}

/// Independent oracle: signed net per unit, debit positive.
fn net_cents(legs: &[GeneratedLeg]) -> i64 {
    legs.iter()
        .map(|leg| {
            let value = i64::from(leg.price_cents) * i64::from(leg.ratio);
            if leg.buy {
                value
            } else {
                -value
            }
        })
        .sum()
}

fn intent(legs: &[GeneratedLeg], units: u32, net: i64) -> ComboIntent {
    let limit = cents(u32::try_from(net.unsigned_abs()).unwrap());
    ComboIntent {
        intent_id: "intent.proptest.combo".to_owned(),
        account_id: ACCOUNT.to_owned(),
        strategy_id: "strategy.proptest".to_owned(),
        correlation_id: "corr.proptest".to_owned(),
        legs: legs
            .iter()
            .enumerate()
            .map(|(index, leg)| ComboIntentLeg {
                instrument_id: instrument(index),
                side: if leg.buy { Side::Buy } else { Side::Sell },
                ratio: leg.ratio,
                limit_price: cents(leg.price_cents),
            })
            .collect(),
        combo_quantity: Decimal::from_integer(i64::from(units)).unwrap(),
        price_limit: if net > 0 {
            ComboPriceLimit::MaximumDebit(limit)
        } else {
            ComboPriceLimit::MinimumCredit(limit)
        },
        time_in_force: TimeInForce::Day,
        rationale: "property test".to_owned(),
        created_at: DECIDED_AT.to_owned(),
        strategy_version: "proptest.v1".to_owned(),
        configuration_version: "proptest.v1".to_owned(),
        environment: "PAPER".to_owned(),
    }
}

fn observable(service: &PaperTradingService<ReplayingAdapter>, id: &str) -> String {
    let order = service.combo_order(id).unwrap();
    format!(
        "{:?}|{}|{:?}",
        order.oms.state,
        order.filled_quantity,
        service.dashboard()
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn combination_lifecycle_matches_an_independent_model(
        legs in legs_strategy(),
        units in 1u32..=6,
        steps in steps_strategy(),
    ) {
        let net = net_cents(&legs);
        prop_assume!(net != 0);

        let mut service = service();
        let market = PaperComboMarketData {
            marks: legs
                .iter()
                .enumerate()
                .map(|(index, leg)| PaperMarketData {
                    instrument_id: instrument(index),
                    mark_price: cents(leg.price_cents),
                    observed_at: DECIDED_AT.to_owned(),
                })
                .collect(),
        };
        let outcome = service.submit_combo_intent(intent(&legs, units, net), market, DECIDED_AT).unwrap();
        prop_assert!(outcome.decision.approved, "{:?}", outcome.decision.reason_codes);
        let id = outcome.order_id.unwrap();
        let broker_order_id = service.combo_order(&id).unwrap().broker_order_id.clone().unwrap();

        // The model.
        let mut filled: u32 = 0;
        let mut cancelled = false;
        let mut cash_cents: i64 = i64::from(INITIAL_CASH.parse::<u32>().unwrap()) * 100;
        let mut applied: Vec<BrokerComboExecution> = Vec::new();

        for (step_index, step) in steps.iter().enumerate() {
            match step {
                Step::Fill(requested) => {
                    let chunk = (*requested).min(units - filled);
                    if cancelled || chunk == 0 {
                        continue;
                    }
                    let group = format!("proptest-group-{step_index}");
                    let execution = BrokerComboExecution {
                        execution_id: group.clone(),
                        client_order_id: id.clone(),
                        broker_order_id: broker_order_id.clone(),
                        units: Decimal::from_integer(i64::from(chunk)).unwrap(),
                        legs: legs
                            .iter()
                            .enumerate()
                            .map(|(index, leg)| BrokerComboExecutionLeg {
                                execution_id: format!("{group}-leg-{index}"),
                                instrument_id: instrument(index),
                                side: if leg.buy { Side::Buy } else { Side::Sell },
                                quantity: Decimal::from_integer(i64::from(chunk * leg.ratio)).unwrap(),
                                price: cents(leg.price_cents),
                                fee: cents(leg.fee_cents),
                                executed_at: EXECUTED_AT.to_owned(),
                            })
                            .collect(),
                    };
                    service.broker_mut().inner.queue_combo_fill(execution.clone()).unwrap();
                    service.synchronize().unwrap();
                    filled += chunk;
                    for leg in &legs {
                        let gross = i64::from(leg.price_cents) * i64::from(leg.ratio * chunk);
                        cash_cents += if leg.buy { -gross } else { gross };
                        cash_cents -= i64::from(leg.fee_cents);
                    }
                    applied.push(execution);
                }
                Step::Replay(which) => {
                    if applied.is_empty() {
                        continue;
                    }
                    let before = observable(&service, &id);
                    let execution = applied[which % applied.len()].clone();
                    service.broker_mut().redeliver.push(BrokerEvent::ComboExecution(execution));
                    service.synchronize().unwrap();
                    prop_assert_eq!(observable(&service, &id), before, "a replay changed state");
                }
                Step::Cancel => {
                    if filled == units {
                        continue;
                    }
                    service.cancel_order(&id).unwrap();
                    service.synchronize().unwrap();
                    // A retry after completion is idempotent.
                    service.cancel_order(&id).unwrap();
                    cancelled = true;
                }
            }

            let order = service.combo_order(&id).unwrap();
            prop_assert_eq!(order.filled_quantity, Decimal::from_integer(i64::from(filled)).unwrap());
            let expected_state = if filled == units {
                OrderState::Filled
            } else if cancelled {
                OrderState::Cancelled
            } else if filled == 0 {
                OrderState::Acknowledged
            } else {
                OrderState::PartiallyFilled
            };
            prop_assert_eq!(order.oms.state, expected_state);

            let dashboard = service.dashboard();
            let working = u32::from(!matches!(expected_state, OrderState::Filled | OrderState::Cancelled));
            prop_assert_eq!(dashboard.working_orders, working);
            prop_assert_eq!(dec(&dashboard.internal_cash), cents_signed(cash_cents));
            for (index, leg) in legs.iter().enumerate() {
                let expected = i64::from(filled * leg.ratio) * if leg.buy { 1 } else { -1 };
                let actual = dashboard
                    .positions
                    .iter()
                    .find(|position| position.instrument_id == instrument(index))
                    .map_or(Decimal::ZERO, |position| dec(&position.quantity));
                prop_assert_eq!(actual, Decimal::from_integer(expected).unwrap(), "leg {}", index);
            }

            let reconciled_at = format!("2026-01-02T15:{:02}:00Z", step_index + 1);
            let report = service.reconcile(&reconciled_at).unwrap();
            prop_assert!(report.is_clean(), "step {} reconciliation: {:?}", step_index, report.issues);
        }
    }
}

fn cents_signed(value: i64) -> Decimal {
    let magnitude = cents(u32::try_from(value.unsigned_abs()).unwrap());
    if value < 0 {
        Decimal::ZERO.checked_sub(magnitude).unwrap()
    } else {
        magnitude
    }
}
