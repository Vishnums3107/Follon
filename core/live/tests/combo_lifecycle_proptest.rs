//! Model-based property test for the controlled-LIVE atomic-combination lifecycle.
//!
//! The LIVE counterpart of `core/paper/tests/combo_lifecycle_proptest.rs`.
//! Arbitrary 2-4 leg debit or credit combinations are submitted through the
//! real canary path (activation, four-eyes approval, managed-secret connection)
//! and driven through random whole-unit fills, broker re-deliveries and an
//! optional cancellation. After every step the service is checked against an
//! independent model written here from the contract:
//!
//! - filled units equal the sum of distinct applied executions;
//! - the lifecycle state follows from filled units and cancellation alone;
//! - every leg's position is `side * ratio * filled units`;
//! - cash moves by exactly each leg's signed gross plus its fee;
//! - a working combination counts as one working order, a finished one as none;
//! - replaying any earlier execution changes nothing at all;
//! - no incident is raised, and reconciliation is clean after every step.
//!
//! The broker is a model written in this file, not a repository adapter, so the
//! reconciliation check compares the service against genuinely independent
//! arithmetic. Only the crate's public API is used.

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};

use follon_domain::{
    ComboIntent, ComboIntentLeg, ComboPriceLimit, Decimal, OrderState, Side, TimeInForce,
};
use follon_live::{
    combo_intent_fingerprint, LiveAccount, LiveActivation, LiveActivationRequest, LiveApproval,
    LiveBrokerAccountSnapshot, LiveBrokerAdapter, LiveBrokerComboExecution,
    LiveBrokerComboExecutionLeg, LiveBrokerComboRequest, LiveBrokerEvent, LiveBrokerOrderRequest,
    LiveBrokerOrderSnapshot, LiveBrokerPositionSnapshot, LiveBrokerSubmitResult,
    LiveComboMarketData, LiveError, LiveKillSwitchRegistry, LiveMarketData, LiveRiskPolicy,
    LiveRunMode, LiveSubmitOutcome, LiveTradingService, ShortExposurePolicy,
};
use follon_secrets::{SecretError, SecretMaterial, SecretProvider, SecretReference};
use proptest::prelude::*;

const ACCOUNT: &str = "acct.live.proptest";
const INITIAL_CASH_CENTS: i64 = 100_000 * 100;
const REQUESTER: &str = "operator.requester.001";
const APPROVER: &str = "operator.approver.001";
const EXECUTED_AT: &str = "2026-01-02T14:31:00Z";

fn dec(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

fn cents(value: i64) -> Decimal {
    let magnitude = dec(&format!(
        "{}.{:02}",
        value.unsigned_abs() / 100,
        value.unsigned_abs() % 100
    ));
    if value < 0 {
        Decimal::ZERO.checked_sub(magnitude).unwrap()
    } else {
        magnitude
    }
}

struct Secrets;

impl SecretProvider for Secrets {
    fn resolve(&self, _reference: &SecretReference) -> Result<SecretMaterial, SecretError> {
        SecretMaterial::new(b"proptest".to_vec())
    }
}

struct ModelCombo {
    broker_order_id: String,
    state: OrderState,
    filled_units: Decimal,
    /// First leg as (ratio, total contracts), to tell a complete fill.
    first_leg: (u32, Decimal),
}

/// An independent broker model: tracks its own orders, positions and cash.
#[derive(Default)]
struct ModelBroker {
    combos: BTreeMap<String, ModelCombo>,
    positions: BTreeMap<String, Decimal>,
    cash_cents: i64,
    events: Vec<LiveBrokerEvent>,
    redeliver: Vec<LiveBrokerEvent>,
}

impl ModelBroker {
    fn fill(
        &mut self,
        execution: LiveBrokerComboExecution,
        fees_cents: i64,
        signed_gross_cents: i64,
    ) {
        let combo = self.combos.get_mut(&execution.client_order_id).unwrap();
        combo.filled_units = combo.filled_units.checked_add(execution.units).unwrap();
        let (ratio, contracts) = combo.first_leg;
        combo.state = if combo
            .filled_units
            .checked_mul(Decimal::from_integer(i64::from(ratio)).unwrap())
            .unwrap()
            == contracts
        {
            OrderState::Filled
        } else {
            OrderState::PartiallyFilled
        };
        for leg in &execution.legs {
            let position = self
                .positions
                .entry(leg.instrument_id.clone())
                .or_insert(Decimal::ZERO);
            *position = match leg.side {
                Side::Buy => position.checked_add(leg.quantity).unwrap(),
                Side::Sell => position.checked_sub(leg.quantity).unwrap(),
            };
        }
        self.cash_cents += signed_gross_cents - fees_cents;
        self.events.push(LiveBrokerEvent::ComboExecution(execution));
    }
}

impl LiveBrokerAdapter for ModelBroker {
    fn connect(
        &mut self,
        _account_id: &str,
        _credential: &SecretMaterial,
    ) -> Result<(), LiveError> {
        Ok(())
    }
    fn submit(
        &mut self,
        _request: &LiveBrokerOrderRequest,
    ) -> Result<LiveBrokerSubmitResult, LiveError> {
        Ok(LiveBrokerSubmitResult::Rejected {
            reason: "MODEL_PLAIN_ORDERS_UNUSED".to_owned(),
        })
    }
    fn submit_combo(
        &mut self,
        request: &LiveBrokerComboRequest,
    ) -> Result<LiveBrokerSubmitResult, LiveError> {
        let broker_order_id = format!("model-live-combo-{}", self.combos.len() + 1);
        self.combos.insert(
            request.client_order_id.clone(),
            ModelCombo {
                broker_order_id: broker_order_id.clone(),
                state: OrderState::Acknowledged,
                filled_units: Decimal::ZERO,
                first_leg: (request.legs[0].ratio, request.legs[0].quantity),
            },
        );
        Ok(LiveBrokerSubmitResult::Acknowledged { broker_order_id })
    }
    fn cancel(&mut self, client_order_id: &str) -> Result<(), LiveError> {
        let combo = self
            .combos
            .get_mut(client_order_id)
            .ok_or_else(|| LiveError("model broker does not know order".to_owned()))?;
        combo.state = OrderState::Cancelled;
        self.events.push(LiveBrokerEvent::Cancelled {
            client_order_id: client_order_id.to_owned(),
            reason: "MODEL_CANCELLED".to_owned(),
        });
        Ok(())
    }
    fn poll(&mut self) -> Result<Vec<LiveBrokerEvent>, LiveError> {
        let mut events = std::mem::take(&mut self.events);
        events.append(&mut self.redeliver);
        Ok(events)
    }
    fn snapshot(&mut self, _account_id: &str) -> Result<LiveBrokerAccountSnapshot, LiveError> {
        Ok(LiveBrokerAccountSnapshot {
            orders: self
                .combos
                .iter()
                .map(|(id, combo)| LiveBrokerOrderSnapshot {
                    client_order_id: id.clone(),
                    broker_order_id: combo.broker_order_id.clone(),
                    state: combo.state,
                    filled_quantity: combo.filled_units,
                })
                .collect(),
            positions: self
                .positions
                .iter()
                .map(|(instrument_id, quantity)| LiveBrokerPositionSnapshot {
                    instrument_id: instrument_id.clone(),
                    quantity: *quantity,
                })
                .collect(),
            cash: cents(self.cash_cents),
        })
    }
    fn reconnect(
        &mut self,
        _account_id: &str,
        _credential: &SecretMaterial,
    ) -> Result<(), LiveError> {
        Ok(())
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
    Fill(u32),
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

fn instrument(index: usize) -> String {
    format!("inst.us_option.live.proptest.leg{index}")
}

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
    let limit = cents(net.abs());
    ComboIntent {
        intent_id: "intent.live.proptest.combo".to_owned(),
        account_id: ACCOUNT.to_owned(),
        strategy_id: "strategy.live.proptest".to_owned(),
        correlation_id: "corr.live.proptest".to_owned(),
        legs: legs
            .iter()
            .enumerate()
            .map(|(index, leg)| ComboIntentLeg {
                instrument_id: instrument(index),
                side: if leg.buy { Side::Buy } else { Side::Sell },
                ratio: leg.ratio,
                limit_price: cents(i64::from(leg.price_cents)),
            })
            .collect(),
        combo_quantity: Decimal::from_integer(i64::from(units)).unwrap(),
        price_limit: if net > 0 {
            ComboPriceLimit::MaximumDebit(limit)
        } else {
            ComboPriceLimit::MinimumCredit(limit)
        },
        time_in_force: TimeInForce::Day,
        rationale: "live property test".to_owned(),
        created_at: "2026-01-02T14:30:00Z".to_owned(),
        strategy_version: "proptest.v1".to_owned(),
        configuration_version: "proptest.v1".to_owned(),
        environment: "LIVE".to_owned(),
    }
}

static CASE: AtomicUsize = AtomicUsize::new(0);

fn canary_service(combo: &ComboIntent) -> (LiveTradingService<ModelBroker>, std::path::PathBuf) {
    let account = LiveAccount {
        account_id: ACCOUNT.to_owned(),
        currency: "USD".to_owned(),
        initial_cash: cents(INITIAL_CASH_CENTS),
        max_deployed_capital: dec("50000"),
        environment: "LIVE".to_owned(),
        credential_reference: SecretReference::new("secret.broker.test.acct-live-proptest")
            .unwrap(),
    };
    let policy = LiveRiskPolicy {
        version: "live-risk-proptest-v1".to_owned(),
        trading_calendar_id: "calendar.nyse.v1".to_owned(),
        max_order_quantity: dec("100"),
        max_order_notional: dec("50000"),
        max_price_deviation_bps: dec("100"),
        canary_max_order_notional: dec("50000"),
        canary_max_orders: 10,
        max_open_orders: 10,
        max_position_quantity: dec("1000"),
        max_realized_loss: dec("10000"),
        max_market_data_age_seconds: 60,
        max_order_rate: 20,
        order_rate_window_seconds: 60,
        portfolio_risk: None,
        short_exposure: Some(ShortExposurePolicy {
            max_short_quantity: dec("1000"),
        }),
        // Every leg is tick-checked like a plain order (E3.6b); generated
        // prices are whole cents, so each leg is listed at a one-cent tick.
        instrument_tick_sizes: (0..4)
            .map(instrument)
            .chain(["inst.us_equity.spy".to_owned()])
            .map(|instrument| (instrument, dec("0.01")))
            .collect(),
    };
    let switches = LiveKillSwitchRegistry::new("live-kills-proptest-v1").unwrap();
    let activation = LiveActivation::for_configuration(
        LiveActivationRequest {
            activation_id: "activation.live.proptest".to_owned(),
            mode: LiveRunMode::Canary,
            requested_by: REQUESTER.to_owned(),
            approved_by: APPROVER.to_owned(),
            activated_at: "2026-01-02T14:00:00Z".to_owned(),
            expires_at: "2026-12-31T23:59:59Z".to_owned(),
        },
        &account,
        &policy,
        &switches,
    )
    .unwrap();
    let scratch = std::env::temp_dir().join(format!(
        "follon-live-combo-proptest-{}-{}",
        std::process::id(),
        CASE.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let broker = ModelBroker {
        cash_cents: INITIAL_CASH_CENTS,
        ..ModelBroker::default()
    };
    let mut service = LiveTradingService::open_durable(
        account,
        policy,
        activation,
        switches,
        broker,
        scratch.join("journal.ndjson"),
        "2026-01-02T14:00:00Z",
    )
    .unwrap();
    let approval = LiveApproval {
        approval_id: "approval.live.proptest".to_owned(),
        intent_id: combo.intent_id.clone(),
        intent_fingerprint: combo_intent_fingerprint(combo).unwrap(),
        configuration_fingerprint: service.configuration_fingerprint(),
        requested_by: REQUESTER.to_owned(),
        approved_by: APPROVER.to_owned(),
        approved_at: "2026-01-02T14:30:00Z".to_owned(),
        expires_at: "2026-01-02T15:00:00Z".to_owned(),
    };
    service
        .register_approval(approval, "2026-01-02T14:30:00Z", APPROVER)
        .unwrap();
    service
        .connect(&Secrets, APPROVER, "2026-01-02T14:30:00Z")
        .unwrap();
    (service, scratch)
}

fn observable(service: &LiveTradingService<ModelBroker>, id: &str) -> String {
    let order = service.combo_order(id).unwrap();
    let mut dashboard = service.monitoring_dashboard();
    // The audit chain grows with every synchronize call by design; everything
    // else must be byte-identical after a replay.
    dashboard.audit_sequence = 0;
    dashboard.audit_head_hash.clear();
    format!(
        "{:?}|{}|{:?}",
        order.oms.state, order.filled_quantity, dashboard
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn live_combination_lifecycle_matches_an_independent_model(
        legs in legs_strategy(),
        units in 1u32..=6,
        steps in steps_strategy(),
    ) {
        let net = net_cents(&legs);
        prop_assume!(net != 0);
        let combo = intent(&legs, units, net);
        let (mut service, scratch) = canary_service(&combo);
        let market = LiveComboMarketData {
            marks: legs
                .iter()
                .enumerate()
                .map(|(index, leg)| LiveMarketData {
                    instrument_id: instrument(index),
                    mark_price: cents(i64::from(leg.price_cents)),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                })
                .collect(),
        };
        let outcome = service
            .submit_canary_combo_intent(combo, market, "approval.live.proptest", "2026-01-02T14:30:02Z", REQUESTER)
            .unwrap();
        let id = match outcome {
            LiveSubmitOutcome::CanaryOrder { order_id, state, .. } => {
                prop_assert_eq!(state, OrderState::Acknowledged);
                order_id
            }
            other => return Err(TestCaseError::fail(format!("not submitted: {other:?}"))),
        };
        let broker_order_id = service.combo_order(&id).unwrap().broker_order_id.clone().unwrap();

        let mut filled: u32 = 0;
        let mut cancelled = false;
        let mut cash_cents = INITIAL_CASH_CENTS;
        let mut applied: Vec<LiveBrokerComboExecution> = Vec::new();
        let mut second = 0u32;
        let mut clock = || {
            second += 1;
            format!("2026-01-02T15:{:02}:{:02}Z", second / 60, second % 60)
        };

        for (step_index, step) in steps.iter().enumerate() {
            match step {
                Step::Fill(requested) => {
                    let chunk = (*requested).min(units - filled);
                    if cancelled || chunk == 0 {
                        continue;
                    }
                    let group = format!("live-proptest-group-{step_index}");
                    let mut fees = 0i64;
                    let mut signed_gross = 0i64;
                    for leg in &legs {
                        let gross = i64::from(leg.price_cents) * i64::from(leg.ratio * chunk);
                        signed_gross += if leg.buy { -gross } else { gross };
                        fees += i64::from(leg.fee_cents);
                    }
                    let execution = LiveBrokerComboExecution {
                        execution_id: group.clone(),
                        client_order_id: id.clone(),
                        broker_order_id: broker_order_id.clone(),
                        units: Decimal::from_integer(i64::from(chunk)).unwrap(),
                        legs: legs
                            .iter()
                            .enumerate()
                            .map(|(index, leg)| LiveBrokerComboExecutionLeg {
                                execution_id: format!("{group}-leg-{index}"),
                                instrument_id: instrument(index),
                                side: if leg.buy { Side::Buy } else { Side::Sell },
                                quantity: Decimal::from_integer(i64::from(chunk * leg.ratio)).unwrap(),
                                price: cents(i64::from(leg.price_cents)),
                                fee: cents(i64::from(leg.fee_cents)),
                                executed_at: EXECUTED_AT.to_owned(),
                            })
                            .collect(),
                    };
                    service.broker_mut().fill(execution.clone(), fees, signed_gross);
                    service.synchronize(APPROVER, &clock()).unwrap();
                    filled += chunk;
                    cash_cents += signed_gross - fees;
                    applied.push(execution);
                }
                Step::Replay(which) => {
                    if applied.is_empty() {
                        continue;
                    }
                    let before = observable(&service, &id);
                    let execution = applied[which % applied.len()].clone();
                    service.broker_mut().redeliver.push(LiveBrokerEvent::ComboExecution(execution));
                    service.synchronize(APPROVER, &clock()).unwrap();
                    prop_assert_eq!(observable(&service, &id), before, "a replay changed state");
                }
                Step::Cancel => {
                    if filled == units {
                        continue;
                    }
                    service.cancel_order(&id, APPROVER, &clock()).unwrap();
                    service.synchronize(APPROVER, &clock()).unwrap();
                    service.cancel_order(&id, APPROVER, &clock()).unwrap();
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

            let dashboard = service.monitoring_dashboard();
            let working = u32::from(!matches!(expected_state, OrderState::Filled | OrderState::Cancelled));
            prop_assert_eq!(dashboard.working_orders, working);
            prop_assert_eq!(dashboard.unresolved_incidents, 0);
            prop_assert_eq!(dec(&dashboard.internal_cash), cents(cash_cents));
            for (index, leg) in legs.iter().enumerate() {
                let expected = i64::from(filled * leg.ratio) * if leg.buy { 1 } else { -1 };
                let actual = dashboard
                    .positions
                    .iter()
                    .find(|position| position.instrument_id == instrument(index))
                    .map_or(Decimal::ZERO, |position| dec(&position.quantity));
                prop_assert_eq!(actual, Decimal::from_integer(expected).unwrap(), "leg {}", index);
            }

            let report = service.reconcile(APPROVER, &clock()).unwrap();
            prop_assert!(report.is_clean(), "step {} reconciliation: {:?}", step_index, report.issues);
        }
        drop(service);
        let _ = std::fs::remove_dir_all(scratch);
    }
}
