//! Real Risk/OMS gateway backing the desktop trading commands with an
//! in-process PAPER trading engine (`follon_paper::PaperTradingService`).
//!
//! Scope, honestly stated:
//! - There is no live market-data feed wired into this desktop boundary, so a
//!   fresh market observation cannot be produced automatically. Every request
//!   therefore carries an explicit, operator-attested reference price (see
//!   `trading::OrderIntent::reference_price`) rather than a fabricated or
//!   stale one, and risk gating (price collar, notional limits) is evaluated
//!   against exactly that attested value.
//! - There is no connected external broker (real IBKR connectivity lives in
//!   `follon_ibkr_paper_adapter::IbkrPaperGatewayAdapter`, which bridges to a
//!   real TWS/IB Gateway process via `python/ibkr-gateway`, and requires an
//!   actual running IBKR Gateway to configure). Instead this gateway uses the
//!   in-process, deterministic `follon_paper::IbkrPaperAdapter`. A market
//!   order, or a limit order that is genuinely marketable against the
//!   caller-supplied reference price, fills immediately in full at that
//!   reference price; a non-marketable limit order rests until cancelled --
//!   there is no matching engine or order book to interact with beyond that.
//!   Swapping in the real external adapter later requires no change above
//!   this module's `RiskOmsGateway` boundary: `PaperOmsGateway` would simply
//!   be generic over a different concrete `PaperBrokerAdapter`. That adapter
//!   declares only single DAY orders, so behind it the combination ticket and
//!   GTC orders would be refused before any order exists (delivery state
//!   E5.1).
//!
//! Every other Risk/OMS guarantee (rate limiting, price collar, kill
//! switches, idempotent submission, durable append-only audit journal) is the
//! real thing: this module is a thin translation layer over
//! `follon_paper::PaperTradingService`, not a reimplementation of it.

use std::path::PathBuf;
use std::sync::Mutex;

use follon_domain::{
    ComboIntent, ComboIntentLeg, ComboPriceLimit, Decimal, OrderIntent as DomainOrderIntent,
    OrderState, OrderType as DomainOrderType, Side as DomainSide, TimeInForce as DomainTimeInForce,
};
use follon_paper::{
    BrokerCancelRequest, BrokerComboExecution, BrokerComboExecutionLeg, BrokerComboRequest,
    BrokerOrderRequest, BrokerSubmitResult, IbkrPaperAdapter, KillSwitchRegistry, PaperAccount,
    PaperBrokerAdapter, PaperBrokerCapabilities, PaperComboMarketData, PaperError, PaperMarketData,
    PaperRiskPolicy, PaperTradingService, ShortExposurePolicy,
};
use serde::Deserialize;
use time::OffsetDateTime;

use crate::trading::{
    CancelOrderIntent, ClosePositionIntent, ComboOrderIntent, ComboPriceLimitKind, CommandReceipt,
    CommandStatus, OrderIntent, OrderSide, OrderType, RiskOmsGateway, TimeInForce,
    TradingCommandError, TradingCommandKind,
};

/// Wraps the in-process simulated IBKR paper adapter so a successfully
/// acknowledged order is evaluated against a reference price set by the
/// caller just before submission -- there is no real matching engine behind
/// this desktop boundary to wait on. A market order, or a limit order that is
/// genuinely marketable against that reference price, fills immediately in
/// full at the reference price; a non-marketable limit order is left
/// resting (`Acknowledged`), exactly like a real limit order untouched until
/// the price moves, and remains cancellable.
///
/// A combination is evaluated the same way, as one unit: its net price at the
/// per-leg reference observations must satisfy the approved net-price
/// protection, sign included, or the whole group rests. When it is marketable
/// it fills as one complete atomic execution at those observations; no leg
/// ever fills on its own.
struct ManualFillAdapter {
    inner: IbkrPaperAdapter,
    next_reference: Option<(Decimal, String)>,
    next_combo_reference: Option<ArmedComboReference>,
}

/// The per-leg observations and protection the next `submit_combo()` call
/// evaluates fillability against.
struct ArmedComboReference {
    price_limit: ComboPriceLimit,
    combo_quantity: Decimal,
    marks: Vec<(String, Decimal)>,
    executed_at: String,
}

impl ArmedComboReference {
    fn mark_for(&self, instrument_id: &str) -> Option<Decimal> {
        self.marks
            .iter()
            .find(|(observed, _)| observed == instrument_id)
            .map(|(_, mark)| *mark)
    }

    /// The signed net price of one unit at the reference observations:
    /// positive is a debit, negative a credit, matching
    /// `ComboIntent::protected_net_price`. `None` if any leg is unobserved --
    /// an unobserved leg is never priced from anything else.
    fn net_price(&self, request: &BrokerComboRequest) -> Result<Option<Decimal>, PaperError> {
        let mut net = Decimal::ZERO;
        for leg in &request.legs {
            let Some(mark) = self.mark_for(&leg.instrument_id) else {
                return Ok(None);
            };
            let leg_net = mark.checked_mul(Decimal::from_integer(i64::from(leg.ratio))?)?;
            net = match leg.side {
                DomainSide::Buy => net.checked_add(leg_net)?,
                DomainSide::Sell => net.checked_sub(leg_net)?,
            };
        }
        Ok(Some(net))
    }
}

impl ManualFillAdapter {
    fn new(account: &PaperAccount) -> Result<Self, PaperError> {
        Ok(Self {
            inner: IbkrPaperAdapter::new(account)?,
            next_reference: None,
            next_combo_reference: None,
        })
    }

    /// Arms the reference price/time the next `submit()` call will evaluate
    /// fillability against.
    fn arm_next_reference(&mut self, reference_price: Decimal, executed_at: String) {
        self.next_reference = Some((reference_price, executed_at));
    }

    /// Arms the per-leg observations the next `submit_combo()` call will
    /// evaluate fillability against.
    fn arm_next_combo_reference(&mut self, reference: ArmedComboReference) {
        self.next_combo_reference = Some(reference);
    }

    /// Drops an armed combination reference no broker call consumed, e.g.
    /// after a risk rejection, so it can never price a later request.
    fn disarm_combo_reference(&mut self) {
        self.next_combo_reference = None;
    }
}

impl PaperBrokerAdapter for ManualFillAdapter {
    /// The model's set, less replacement: this adapter forwards combinations
    /// and every time in force to the model, but not `replace`, for which it
    /// inherits the trait's refusal. The desktop exposes no replacement.
    fn capabilities(&self, account_id: &str) -> Result<PaperBrokerCapabilities, PaperError> {
        Ok(PaperBrokerCapabilities {
            replacement: false,
            ..self.inner.capabilities(account_id)?
        })
    }

    fn adapter_configuration_fingerprint(&self, account_id: &str) -> Result<String, PaperError> {
        self.inner.adapter_configuration_fingerprint(account_id)
    }

    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError> {
        let result = self.inner.submit(request)?;
        if let (BrokerSubmitResult::Acknowledged { .. }, Some((reference_price, executed_at))) =
            (&result, self.next_reference.take())
        {
            let marketable = match request.limit_price {
                None => true,
                Some(limit) => match request.side {
                    follon_domain::Side::Buy => reference_price <= limit,
                    follon_domain::Side::Sell => reference_price >= limit,
                },
            };
            if marketable {
                self.inner.queue_fill(
                    &request.client_order_id,
                    request.quantity,
                    reference_price,
                    Decimal::ZERO,
                    &executed_at,
                )?;
            }
        }
        Ok(result)
    }

    fn submit_combo(
        &mut self,
        request: &BrokerComboRequest,
    ) -> Result<BrokerSubmitResult, PaperError> {
        let result = self.inner.submit_combo(request)?;
        let (BrokerSubmitResult::Acknowledged { broker_order_id }, Some(reference)) =
            (&result, self.next_combo_reference.take())
        else {
            return Ok(result);
        };
        let Some(net_price) = reference.net_price(request)? else {
            return Ok(result);
        };
        // Marketable only if the net at the observations satisfies the
        // approved protection *including its sign*: a debit-protected
        // combination observed at a credit is a different trade and rests.
        if reference.price_limit.check_net_price(net_price).is_err() {
            return Ok(result);
        }
        let mut legs = Vec::with_capacity(request.legs.len());
        for (index, leg) in request.legs.iter().enumerate() {
            let Some(price) = reference.mark_for(&leg.instrument_id) else {
                return Ok(result);
            };
            legs.push(BrokerComboExecutionLeg {
                execution_id: format!("{}.fill.leg-{index}", request.client_order_id),
                instrument_id: leg.instrument_id.clone(),
                side: leg.side,
                quantity: leg.quantity,
                price,
                fee: Decimal::ZERO,
                executed_at: reference.executed_at.clone(),
            });
        }
        self.inner.queue_combo_fill(BrokerComboExecution {
            execution_id: format!("{}.fill", request.client_order_id),
            client_order_id: request.client_order_id.clone(),
            broker_order_id: broker_order_id.clone(),
            units: reference.combo_quantity,
            legs,
        })?;
        Ok(result)
    }

    fn cancel(&mut self, request: &BrokerCancelRequest) -> Result<(), PaperError> {
        self.inner.cancel(request)
    }

    fn poll(&mut self, account_id: &str) -> Result<Vec<follon_paper::BrokerEvent>, PaperError> {
        self.inner.poll(account_id)
    }

    fn snapshot(
        &mut self,
        account_id: &str,
    ) -> Result<follon_paper::BrokerAccountSnapshot, PaperError> {
        self.inner.snapshot(account_id)
    }

    fn reconnect(&mut self, account_id: &str) -> Result<(), PaperError> {
        self.inner.reconnect(account_id)
    }
}

/// Real `RiskOmsGateway` implementation backed by `PaperTradingService`.
pub struct PaperOmsGateway {
    service: Mutex<PaperTradingService<ManualFillAdapter>>,
}

impl PaperOmsGateway {
    fn map_error(context: &str, error: PaperError) -> TradingCommandError {
        TradingCommandError::Validation(format!("{context}: {error}"))
    }

    /// Formats the current instant as the canonical second-precision UTC
    /// timestamp this codebase requires everywhere, without depending on the
    /// `time` crate's RFC3339 formatter (whose fractional-second output
    /// varies) to guarantee the exact `YYYY-MM-DDTHH:MM:SSZ` shape.
    fn now_canonical() -> String {
        let now = OffsetDateTime::now_utc();
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            now.year(),
            u8::from(now.month()),
            now.day(),
            now.hour(),
            now.minute(),
            now.second()
        )
    }

    fn domain_side(side: &OrderSide) -> DomainSide {
        match side {
            OrderSide::Buy => DomainSide::Buy,
            OrderSide::Sell => DomainSide::Sell,
        }
    }

    fn domain_order_type(order_type: &OrderType) -> DomainOrderType {
        match order_type {
            OrderType::Market => DomainOrderType::Market,
            OrderType::Limit => DomainOrderType::Limit,
        }
    }

    fn domain_time_in_force(time_in_force: &TimeInForce) -> DomainTimeInForce {
        match time_in_force {
            TimeInForce::Day => DomainTimeInForce::Day,
            TimeInForce::GoodTilCancelled => DomainTimeInForce::GoodTilCancelled,
        }
    }

    fn parse_decimal(name: &str, value: &str) -> Result<Decimal, TradingCommandError> {
        value
            .parse::<Decimal>()
            .map_err(|error| TradingCommandError::Validation(format!("{name}: {error}")))
    }

    fn ensure_account<B: PaperBrokerAdapter>(
        service: &PaperTradingService<B>,
        requested_account_id: &str,
    ) -> Result<(), TradingCommandError> {
        if service.account_id() != requested_account_id {
            return Err(TradingCommandError::Validation(
                "account_id does not match the configured PAPER route".to_owned(),
            ));
        }
        Ok(())
    }

    /// The authoritative OMS state of an order in either map.
    ///
    /// Cancellation is one command for both kinds of order, so its receipt
    /// must read both: reading only the plain-order map would report every
    /// combination as `UNKNOWN` however cleanly it had been cancelled.
    fn any_order_state<B: PaperBrokerAdapter>(
        service: &PaperTradingService<B>,
        order_id: &str,
    ) -> Option<OrderState> {
        service
            .order(order_id)
            .map(|order| order.oms.state)
            .or_else(|| service.combo_order(order_id).map(|order| order.oms.state))
    }

    fn domain_combo_intent(intent: &ComboOrderIntent) -> Result<ComboIntent, TradingCommandError> {
        let amount = Self::parse_decimal("price_limit", &intent.price_limit)?;
        let legs = intent
            .legs
            .iter()
            .map(|leg| {
                Ok(ComboIntentLeg {
                    instrument_id: leg.instrument_id.clone(),
                    side: Self::domain_side(&leg.side),
                    ratio: leg.ratio,
                    limit_price: Self::parse_decimal("leg limit_price", &leg.limit_price)?,
                })
            })
            .collect::<Result<Vec<_>, TradingCommandError>>()?;
        Ok(ComboIntent {
            intent_id: intent.intent_id.clone(),
            account_id: intent.account_id.clone(),
            strategy_id: intent.strategy_id.clone(),
            correlation_id: intent.correlation_id.clone(),
            legs,
            combo_quantity: Self::parse_decimal("combo_quantity", &intent.combo_quantity)?,
            price_limit: match intent.price_limit_kind {
                ComboPriceLimitKind::MaximumDebit => ComboPriceLimit::MaximumDebit(amount),
                ComboPriceLimitKind::MinimumCredit => ComboPriceLimit::MinimumCredit(amount),
            },
            time_in_force: Self::domain_time_in_force(&intent.time_in_force),
            rationale: intent.rationale.clone(),
            created_at: intent.created_at.clone(),
            strategy_version: intent.strategy_version.clone(),
            configuration_version: intent.configuration_version.clone(),
            environment: "PAPER".to_owned(),
        })
    }

    /// One observation per leg, taken only from that leg's own attested
    /// reference. Nothing here can fill a gap from a limit price.
    fn combo_market(
        intent: &ComboOrderIntent,
    ) -> Result<PaperComboMarketData, TradingCommandError> {
        Ok(PaperComboMarketData {
            marks: intent
                .legs
                .iter()
                .map(|leg| {
                    Ok(PaperMarketData {
                        instrument_id: leg.instrument_id.clone(),
                        mark_price: Self::parse_decimal(
                            "leg reference_price",
                            &leg.reference_price,
                        )?,
                        observed_at: leg.reference_observed_at.clone(),
                    })
                })
                .collect::<Result<Vec<_>, TradingCommandError>>()?,
        })
    }

    fn command_status(state: OrderState) -> CommandStatus {
        match state {
            OrderState::Created | OrderState::PendingRisk | OrderState::Approved => {
                CommandStatus::AcceptedForRisk
            }
            OrderState::RiskRejected => CommandStatus::RiskRejected,
            OrderState::PendingSubmit | OrderState::Submitted => CommandStatus::PendingSubmit,
            OrderState::Acknowledged => CommandStatus::Acknowledged,
            OrderState::PartiallyFilled => CommandStatus::PartiallyFilled,
            OrderState::Filled => CommandStatus::Filled,
            OrderState::PendingCancel => CommandStatus::PendingCancel,
            OrderState::PendingReplace => CommandStatus::PendingReplace,
            OrderState::Cancelled => CommandStatus::Cancelled,
            OrderState::Rejected => CommandStatus::Rejected,
            OrderState::Expired => CommandStatus::Expired,
            OrderState::Unknown => CommandStatus::Unknown,
        }
    }
}

impl RiskOmsGateway for PaperOmsGateway {
    fn submit_order(&self, intent: OrderIntent) -> Result<CommandReceipt, TradingCommandError> {
        let reference_price = Self::parse_decimal("reference_price", &intent.reference_price)?;
        let decided_at = Self::now_canonical();
        let domain_intent = DomainOrderIntent {
            intent_id: intent.intent_id.clone(),
            account_id: intent.account_id,
            strategy_id: intent.strategy_id,
            instrument_id: intent.instrument_id.clone(),
            correlation_id: intent.correlation_id,
            side: Self::domain_side(&intent.side),
            quantity: Self::parse_decimal("quantity", &intent.quantity)?,
            order_type: Self::domain_order_type(&intent.order_type),
            limit_price: intent
                .limit_price
                .as_deref()
                .map(|price| Self::parse_decimal("limit_price", price))
                .transpose()?,
            time_in_force: Self::domain_time_in_force(&intent.time_in_force),
            rationale: intent.rationale,
            created_at: intent.created_at,
            strategy_version: intent.strategy_version,
            configuration_version: intent.configuration_version,
            environment: "PAPER".to_owned(),
        };
        let market = PaperMarketData {
            instrument_id: intent.instrument_id,
            mark_price: reference_price,
            observed_at: intent.reference_observed_at,
        };

        let mut service = self
            .service
            .lock()
            .expect("paper OMS mutex is not poisoned");
        service
            .broker_mut()
            .arm_next_reference(reference_price, decided_at.clone());
        let outcome = service
            .submit_intent(domain_intent, market, &decided_at)
            .map_err(|error| Self::map_error("submit_order", error))?;
        if !outcome.decision.approved {
            return Ok(CommandReceipt {
                command: TradingCommandKind::SubmitOrder,
                request_id: intent.intent_id,
                status: CommandStatus::RiskRejected,
                order_id: None,
                message: format!(
                    "risk rejected: {}",
                    outcome.decision.reason_codes.join(", ")
                ),
            });
        }
        // Drains the fill just queued by `ManualFillAdapter::submit` so the
        // receipt below reflects the order's real, current OMS state rather
        // than the merely-submitted state `submit_intent` itself returns.
        service
            .synchronize()
            .map_err(|error| Self::map_error("submit_order", error))?;
        let order_id = outcome.order_id;
        let state = order_id
            .as_deref()
            .and_then(|id| service.order(id))
            .map(|order| order.oms.state)
            .or(outcome.state);
        let status = state
            .map(Self::command_status)
            .unwrap_or(CommandStatus::Unknown);
        Ok(CommandReceipt {
            command: TradingCommandKind::SubmitOrder,
            request_id: intent.intent_id,
            status,
            order_id,
            message: format!(
                "accepted: order is now {}",
                state.map_or_else(|| "UNKNOWN".to_owned(), |state| format!("{state:?}"))
            ),
        })
    }

    fn submit_combo(
        &self,
        intent: ComboOrderIntent,
    ) -> Result<CommandReceipt, TradingCommandError> {
        let domain_intent = Self::domain_combo_intent(&intent)?;
        let market = Self::combo_market(&intent)?;
        let reference = ArmedComboReference {
            price_limit: domain_intent.price_limit,
            combo_quantity: domain_intent.combo_quantity,
            marks: market
                .marks
                .iter()
                .map(|mark| (mark.instrument_id.clone(), mark.mark_price))
                .collect(),
            executed_at: Self::now_canonical(),
        };

        let mut service = self
            .service
            .lock()
            .expect("paper OMS mutex is not poisoned");
        // An idempotent retry of an approved combination must present the
        // original decision time, which only the durable risk evidence knows;
        // a fresh clock reading would make core/paper refuse the retry as a
        // changed request. A previously *rejected* intent created no order,
        // so it is simply evaluated again at the current time.
        let decided_at = service
            .combo_risk_evidence(&format!("paper-combo-risk-{}", intent.intent_id))
            .filter(|evidence| evidence.decision.approved)
            .map_or_else(
                || reference.executed_at.clone(),
                |evidence| evidence.decision.decided_at.clone(),
            );
        service.broker_mut().arm_next_combo_reference(reference);
        let submitted = service.submit_combo_intent(domain_intent, market, &decided_at);
        service.broker_mut().disarm_combo_reference();
        let outcome = submitted.map_err(|error| Self::map_error("submit_combo", error))?;
        if !outcome.decision.approved {
            return Ok(CommandReceipt {
                command: TradingCommandKind::SubmitCombo,
                request_id: intent.intent_id,
                status: CommandStatus::RiskRejected,
                order_id: None,
                message: format!(
                    "risk rejected: {}",
                    outcome.decision.reason_codes.join(", ")
                ),
            });
        }
        // Drains the atomic execution `ManualFillAdapter::submit_combo` may
        // have queued, so the receipt reports the combination's real state.
        service
            .synchronize()
            .map_err(|error| Self::map_error("submit_combo", error))?;
        let state = outcome
            .order_id
            .as_deref()
            .and_then(|id| service.combo_order(id))
            .map(|order| order.oms.state)
            .or(outcome.state);
        Ok(CommandReceipt {
            command: TradingCommandKind::SubmitCombo,
            request_id: intent.intent_id,
            status: state
                .map(Self::command_status)
                .unwrap_or(CommandStatus::Unknown),
            order_id: outcome.order_id,
            message: format!(
                "accepted as one atomic order: combination is now {}",
                state.map_or_else(|| "UNKNOWN".to_owned(), |state| format!("{state:?}"))
            ),
        })
    }

    fn cancel_order(
        &self,
        intent: CancelOrderIntent,
    ) -> Result<CommandReceipt, TradingCommandError> {
        let mut service = self
            .service
            .lock()
            .expect("paper OMS mutex is not poisoned");
        Self::ensure_account(&*service, &intent.account_id)?;
        service
            .cancel_order(&intent.order_id)
            .map_err(|error| Self::map_error("cancel_order", error))?;
        // Drains the cancellation confirmation the adapter just queued so the
        // receipt below reflects the order's real, current OMS state rather
        // than the merely-requested `PendingCancel` state.
        service
            .synchronize()
            .map_err(|error| Self::map_error("cancel_order", error))?;
        let state = Self::any_order_state(&*service, &intent.order_id);
        let status = state
            .map(Self::command_status)
            .unwrap_or(CommandStatus::Unknown);
        Ok(CommandReceipt {
            command: TradingCommandKind::CancelOrder,
            request_id: intent.request_id,
            status,
            order_id: Some(intent.order_id),
            message: format!(
                "cancellation requested: order is now {}",
                state.map_or_else(|| "UNKNOWN".to_owned(), |state| format!("{state:?}"))
            ),
        })
    }

    fn close_position(
        &self,
        intent: ClosePositionIntent,
    ) -> Result<CommandReceipt, TradingCommandError> {
        let reference_price = Self::parse_decimal("reference_price", &intent.reference_price)?;
        let decided_at = Self::now_canonical();
        let mut service = self
            .service
            .lock()
            .expect("paper OMS mutex is not poisoned");
        Self::ensure_account(&*service, &intent.account_id)?;
        let dashboard = service.dashboard();
        let position = dashboard
            .positions
            .iter()
            .find(|position| position.instrument_id == intent.instrument_id)
            .ok_or_else(|| {
                TradingCommandError::Validation(format!(
                    "close_position: no open position for {}",
                    intent.instrument_id
                ))
            })?;
        let quantity = Self::parse_decimal("position quantity", &position.quantity)?;
        if quantity == Decimal::ZERO {
            return Err(TradingCommandError::Validation(format!(
                "close_position: no open position for {}",
                intent.instrument_id
            )));
        }
        let side = if quantity > Decimal::ZERO {
            DomainSide::Sell
        } else {
            DomainSide::Buy
        };
        let magnitude = if quantity > Decimal::ZERO {
            quantity
        } else {
            Decimal::ZERO
                .checked_sub(quantity)
                .map_err(|error| Self::map_error("close_position", error.into()))?
        };
        let domain_intent = DomainOrderIntent {
            intent_id: intent.request_id.clone(),
            account_id: intent.account_id,
            strategy_id: "desktop.manual".to_owned(),
            instrument_id: intent.instrument_id.clone(),
            correlation_id: intent.correlation_id,
            side,
            quantity: magnitude,
            order_type: DomainOrderType::Market,
            limit_price: None,
            time_in_force: DomainTimeInForce::Day,
            rationale: intent.rationale,
            created_at: decided_at.clone(),
            strategy_version: "desktop.v1".to_owned(),
            configuration_version: "desktop.v1".to_owned(),
            environment: "PAPER".to_owned(),
        };
        let market = PaperMarketData {
            instrument_id: intent.instrument_id,
            mark_price: reference_price,
            observed_at: intent.reference_observed_at,
        };
        service
            .broker_mut()
            .arm_next_reference(reference_price, decided_at.clone());
        let outcome = service
            .submit_intent(domain_intent, market, &decided_at)
            .map_err(|error| Self::map_error("close_position", error))?;
        if !outcome.decision.approved {
            return Ok(CommandReceipt {
                command: TradingCommandKind::ClosePosition,
                request_id: intent.request_id,
                status: CommandStatus::RiskRejected,
                order_id: None,
                message: format!(
                    "risk rejected: {}",
                    outcome.decision.reason_codes.join(", ")
                ),
            });
        }
        service
            .synchronize()
            .map_err(|error| Self::map_error("close_position", error))?;
        let status = outcome
            .order_id
            .as_deref()
            .and_then(|order_id| service.order(order_id))
            .map(|order| Self::command_status(order.oms.state))
            .unwrap_or(CommandStatus::Unknown);
        Ok(CommandReceipt {
            command: TradingCommandKind::ClosePosition,
            request_id: intent.request_id,
            status,
            order_id: outcome.order_id,
            message: "close requested".to_owned(),
        })
    }
}

/// On-disk shape of the desktop's PAPER trading configuration.
///
/// It is the flat version-1 `paper-command-route` document without that
/// document's `schema_version`, `adapter_kind` and optional `ibkr_bridge`: the
/// desktop always composes the model. It is not the nested
/// document `follon-paper-status` reads (`apps/cli/src/paper.rs`). Both
/// refuse unknown fields, so one file cannot serve both. This comment
/// previously claimed that it could.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesktopPaperConfiguration {
    account_id: String,
    currency: String,
    initial_cash: String,
    risk_policy_version: String,
    trading_calendar_id: String,
    max_order_quantity: String,
    max_order_notional: String,
    max_price_deviation_bps: String,
    max_open_orders: usize,
    max_position_quantity: String,
    max_realized_loss: String,
    max_market_data_age_seconds: u64,
    max_order_rate: u32,
    order_rate_window_seconds: u64,
    /// Required tick size per tradable instrument, in the same shape as the
    /// version-1 `paper-command-route` document. An order for an unlisted
    /// instrument, or a limit off its grid, is refused before the broker.
    instrument_tick_sizes: std::collections::BTreeMap<String, String>,
    /// Required lot size per tradable instrument, in the same shape as the
    /// version-1 `paper-command-route` document. An order for an unlisted
    /// instrument, or a quantity that is not a whole number of lots, is
    /// refused before the broker.
    instrument_lot_sizes: std::collections::BTreeMap<String, String>,
    /// Optional, explicitly bounded net-short permission, in the same shape
    /// as the version-1 `paper-command-route` document. Absent means every
    /// net short position -- and so almost every spread with a short leg --
    /// is refused, exactly as before this field existed.
    #[serde(default)]
    short_exposure: Option<DesktopShortExposure>,
    kill_switch_version: String,
    journal_path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesktopShortExposure {
    max_short_quantity: String,
}

fn decimal(name: &str, value: &str) -> Result<Decimal, String> {
    value
        .parse::<Decimal>()
        .map_err(|error| format!("{name}: {error}"))
}

/// Builds a real `PaperOmsGateway` from the JSON configuration file named by
/// the `FOLLON_DESKTOP_PAPER_CONFIG` environment variable, if set.
///
/// Returns `None` (never an error) when the variable is unset, the file is
/// missing, or the file fails to parse/validate -- the caller falls back to
/// `TradingCommandState::unavailable()` in that case, so a desktop install
/// with no PAPER configuration behaves exactly as it always has. Any problem
/// is logged to stderr so an operator who did intend to configure trading
/// can see why it did not take effect.
pub fn bootstrap() -> Option<PaperOmsGateway> {
    let path = std::env::var_os("FOLLON_DESKTOP_PAPER_CONFIG").map(PathBuf::from)?;
    match bootstrap_from_path(&path) {
        Ok(gateway) => Some(gateway),
        Err(error) => {
            eprintln!(
                "desktop PAPER trading route disabled: {error} (from {})",
                path.display()
            );
            None
        }
    }
}

fn bootstrap_from_path(path: &std::path::Path) -> Result<PaperOmsGateway, String> {
    let contents =
        std::fs::read_to_string(path).map_err(|error| format!("cannot read config: {error}"))?;
    let document: DesktopPaperConfiguration =
        serde_json::from_str(&contents).map_err(|error| format!("invalid config: {error}"))?;

    let account = PaperAccount {
        account_id: document.account_id,
        currency: document.currency,
        initial_cash: decimal("initial_cash", &document.initial_cash)?,
        environment: "PAPER".to_owned(),
    };
    let risk_policy = PaperRiskPolicy {
        version: document.risk_policy_version,
        trading_calendar_id: document.trading_calendar_id,
        max_order_quantity: decimal("max_order_quantity", &document.max_order_quantity)?,
        max_order_notional: decimal("max_order_notional", &document.max_order_notional)?,
        max_price_deviation_bps: decimal(
            "max_price_deviation_bps",
            &document.max_price_deviation_bps,
        )?,
        max_open_orders: document.max_open_orders,
        max_position_quantity: decimal("max_position_quantity", &document.max_position_quantity)?,
        max_realized_loss: decimal("max_realized_loss", &document.max_realized_loss)?,
        max_market_data_age_seconds: document.max_market_data_age_seconds,
        max_order_rate: document.max_order_rate,
        order_rate_window_seconds: document.order_rate_window_seconds,
        // The desktop's flat, `deny_unknown_fields` configuration document
        // does not yet expose Slice-1 aggregate portfolio-risk composition;
        // `core/paper::evaluate_risk` still gains it for every caller once an
        // operator adopts the CLI/journal configuration path (see
        // `follon_paper::PortfolioRiskComposition`).
        portfolio_risk: None,
        // Net short exposure is refused unless the operator-authored
        // configuration file states a bound. The desktop UI never grants it:
        // it has no operator-authenticated surface on which to take that
        // decision, so the permission lives only in the file the operator
        // controls, alongside every other risk limit.
        short_exposure: document
            .short_exposure
            .map(|permission| {
                Ok::<_, String>(ShortExposurePolicy {
                    max_short_quantity: decimal(
                        "short_exposure.max_short_quantity",
                        &permission.max_short_quantity,
                    )?,
                })
            })
            .transpose()?,
        instrument_tick_sizes: document
            .instrument_tick_sizes
            .iter()
            .map(|(instrument_id, tick)| {
                Ok::<_, String>((
                    instrument_id.clone(),
                    decimal("instrument_tick_sizes", tick)?,
                ))
            })
            .collect::<Result<_, _>>()?,
        instrument_lot_sizes: document
            .instrument_lot_sizes
            .iter()
            .map(|(instrument_id, lot)| {
                Ok::<_, String>((instrument_id.clone(), decimal("instrument_lot_sizes", lot)?))
            })
            .collect::<Result<_, _>>()?,
    };
    let kill_switches = KillSwitchRegistry::new(document.kill_switch_version)
        .map_err(|error| format!("kill switch registry: {error}"))?;
    let broker =
        ManualFillAdapter::new(&account).map_err(|error| format!("broker adapter: {error}"))?;
    let service = PaperTradingService::open_durable(
        account,
        risk_policy,
        kill_switches,
        broker,
        &document.journal_path,
    )
    .map_err(|error| format!("paper trading service: {error}"))?;
    Ok(PaperOmsGateway {
        service: Mutex::new(service),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::{
        ComboLegIntent, ExecutionEnvironment, OrderIntent, OrderSide, OrderType, TimeInForce,
    };
    use follon_domain::OrderState;

    fn test_gateway(name: &str) -> (PaperOmsGateway, PathBuf, PathBuf) {
        test_gateway_with(name, "")
    }

    #[test]
    fn the_manual_fill_adapter_declares_everything_but_replacement() {
        // It forwards combinations and every time in force to the model, but
        // not `replace`. Declaring replacement would let the OMS move an
        // order to PENDING_REPLACE and then record the trait's refusal as
        // UNKNOWN, disconnecting the session.
        let account = PaperAccount {
            account_id: "acct.desktop.paper.test".to_owned(),
            currency: "USD".to_owned(),
            initial_cash: decimal("initial_cash", "100000").unwrap(),
            environment: "PAPER".to_owned(),
        };
        let adapter = ManualFillAdapter::new(&account).unwrap();
        assert_eq!(
            adapter.capabilities("acct.desktop.paper.test").unwrap(),
            PaperBrokerCapabilities {
                combinations: true,
                good_til_cancelled: true,
                replacement: false,
            }
        );
    }

    /// `extra` is spliced verbatim into the configuration document, e.g. an
    /// optional `"short_exposure": {...},` entry.
    fn test_gateway_with(name: &str, extra: &str) -> (PaperOmsGateway, PathBuf, PathBuf) {
        let scratch = std::env::temp_dir().join(format!(
            "follon-desktop-paper-gateway-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let journal_path = scratch.join("journal.ndjson");
        let config_path = scratch.join("config.json");
        std::fs::write(
            &config_path,
            format!(
                r#"{{
                    "account_id": "acct.desktop.paper.test",
                    "currency": "USD",
                    "initial_cash": "100000",
                    "risk_policy_version": "risk.desktop.test.v1",
                    "trading_calendar_id": "cal.us_equities.nyse",
                    "max_order_quantity": "1000",
                    "max_order_notional": "250000",
                    "max_price_deviation_bps": "500",
                    "max_open_orders": 50,
                    "max_position_quantity": "5000",
                    "max_realized_loss": "50000",
                    "max_market_data_age_seconds": 300,
                    "max_order_rate": 20,
                    "order_rate_window_seconds": 60,
                    "instrument_tick_sizes": {{
                        "inst.us_equity.aapl": "0.01",
                        "inst.opt.spy.c500": "0.01",
                        "inst.opt.spy.c505": "0.01"
                    }},
                    "instrument_lot_sizes": {{
                        "inst.us_equity.aapl": "1",
                        "inst.opt.spy.c500": "1",
                        "inst.opt.spy.c505": "1"
                    }},
                    {extra}
                    "kill_switch_version": "kill.desktop.test.v1",
                    "journal_path": {:?}
                }}"#,
                journal_path.to_str().unwrap()
            ),
        )
        .unwrap();
        let gateway = bootstrap_from_path(&config_path).expect("valid test configuration");
        (gateway, scratch, journal_path)
    }

    fn cleanup(scratch: PathBuf) {
        let _ = std::fs::remove_dir_all(scratch);
    }

    fn order_intent(
        intent_id: &str,
        order_type: OrderType,
        limit_price: Option<&str>,
    ) -> OrderIntent {
        OrderIntent {
            intent_id: intent_id.to_owned(),
            account_id: "acct.desktop.paper.test".to_owned(),
            strategy_id: "desktop.manual".to_owned(),
            instrument_id: "inst.us_equity.aapl".to_owned(),
            correlation_id: format!("corr-{intent_id}"),
            side: OrderSide::Buy,
            quantity: "10".to_owned(),
            order_type,
            limit_price: limit_price.map(str::to_owned),
            time_in_force: TimeInForce::Day,
            rationale: "gateway integration test".to_owned(),
            created_at: PaperOmsGateway::now_canonical(),
            strategy_version: "desktop.v1".to_owned(),
            configuration_version: "desktop.v1".to_owned(),
            environment: ExecutionEnvironment::Paper,
            parent_intent_id: None,
            reference_price: "150.00000000".to_owned(),
            reference_observed_at: PaperOmsGateway::now_canonical(),
        }
    }

    #[test]
    fn submit_order_fills_immediately_through_real_risk_and_oms() {
        let (gateway, scratch, _journal) = test_gateway("submit-fill");
        let receipt = gateway
            .submit_order(order_intent("intent.desktop.001", OrderType::Market, None))
            .expect("market order should be accepted");
        assert_eq!(receipt.status, CommandStatus::Filled);
        assert!(receipt.message.contains("Filled"));
        let order_id = receipt.order_id.expect("filled order has an id");

        let service = gateway.service.lock().unwrap();
        let order = service.order(&order_id).expect("order exists");
        assert_eq!(order.oms.state, OrderState::Filled);
        assert_eq!(order.filled_quantity, "10".parse::<Decimal>().unwrap());
        let dashboard = service.dashboard();
        let position = dashboard
            .positions
            .iter()
            .find(|position| position.instrument_id == "inst.us_equity.aapl")
            .expect("position exists after fill");
        assert_eq!(position.quantity, "10.00000000");
        drop(service);
        cleanup(scratch);
    }

    #[test]
    fn submit_order_fills_a_marketable_limit_order_at_the_reference_price() {
        let (gateway, scratch, _journal) = test_gateway("submit-limit-marketable");
        // Reference price is 150.00 (see `order_intent`); a buy limit at or
        // above that is immediately marketable.
        let receipt = gateway
            .submit_order(order_intent(
                "intent.desktop.002",
                OrderType::Limit,
                Some("150.50000000"),
            ))
            .expect("marketable limit order should be accepted");
        let order_id = receipt.order_id.expect("filled order has an id");
        let service = gateway.service.lock().unwrap();
        let order = service.order(&order_id).expect("order exists");
        assert_eq!(order.oms.state, OrderState::Filled);
        drop(service);
        cleanup(scratch);
    }

    #[test]
    fn submit_order_leaves_a_non_marketable_limit_order_resting() {
        let (gateway, scratch, _journal) = test_gateway("submit-limit-resting");
        // Reference price is 150.00 (see `order_intent`); a buy limit below
        // that is not marketable and must stay resting, not fill.
        let receipt = gateway
            .submit_order(order_intent(
                "intent.desktop.007",
                OrderType::Limit,
                Some("149.50000000"),
            ))
            .expect("resting limit order should be accepted");
        let order_id = receipt.order_id.expect("order was created");
        let service = gateway.service.lock().unwrap();
        let order = service.order(&order_id).expect("order exists");
        assert_eq!(order.oms.state, OrderState::Acknowledged);
        assert_eq!(order.filled_quantity, Decimal::ZERO);
        drop(service);
        cleanup(scratch);
    }

    #[test]
    fn submit_order_rejected_by_real_risk_engine_creates_no_order() {
        let (gateway, scratch, _journal) = test_gateway("submit-reject");
        // Notional (10 * 150 = 1500) is fine, but the reference price is set
        // absurdly far from a plausible limit to trip the real price collar.
        let mut intent = order_intent("intent.desktop.003", OrderType::Limit, Some("400.00000000"));
        intent.reference_price = "150.00000000".to_owned();
        let receipt = gateway
            .submit_order(intent)
            .expect("a risk rejection is a normal, successful outcome");
        assert_eq!(receipt.status, CommandStatus::RiskRejected);
        assert!(receipt.order_id.is_none());
        assert!(receipt.message.contains("PRICE_COLLAR_EXCEEDED"));
        cleanup(scratch);
    }

    #[test]
    fn submit_order_off_the_configured_tick_grid_is_refused_before_the_broker() {
        let (gateway, scratch, _journal) = test_gateway("submit-off-grid");
        // 149.995 is inside the collar around the 150.00 reference but off the
        // configured 0.01 grid, so only the tick check can refuse it.
        let receipt = gateway
            .submit_order(order_intent(
                "intent.desktop.tick.001",
                OrderType::Limit,
                Some("149.99500000"),
            ))
            .expect("a risk rejection is a normal, successful outcome");
        assert_eq!(receipt.status, CommandStatus::RiskRejected);
        assert!(receipt.order_id.is_none());
        assert!(receipt.message.contains("LIMIT_PRICE_OFF_TICK_GRID"));
        cleanup(scratch);
    }

    #[test]
    fn submit_order_off_the_configured_lot_size_is_refused_before_the_broker() {
        let (gateway, scratch, _journal) = test_gateway("submit-off-lot");
        // 10.5 shares is inside every quantity and notional limit but is not a
        // whole number of the configured one-share lots, so only the lot check
        // can refuse it.
        let mut intent = order_intent("intent.desktop.lot.001", OrderType::Market, None);
        intent.quantity = "10.50000000".to_owned();
        let receipt = gateway
            .submit_order(intent)
            .expect("a risk rejection is a normal, successful outcome");
        assert_eq!(receipt.status, CommandStatus::RiskRejected);
        assert!(receipt.order_id.is_none());
        assert!(receipt.message.contains("ORDER_QUANTITY_OFF_LOT_SIZE"));
        cleanup(scratch);
    }

    #[test]
    fn close_position_flattens_a_real_filled_position() {
        let (gateway, scratch, _journal) = test_gateway("close-position");
        gateway
            .submit_order(order_intent("intent.desktop.004", OrderType::Market, None))
            .expect("opening order should be accepted");

        let close_receipt = gateway
            .close_position(ClosePositionIntent {
                request_id: "request.close.004".to_owned(),
                account_id: "acct.desktop.paper.test".to_owned(),
                instrument_id: "inst.us_equity.aapl".to_owned(),
                correlation_id: "corr-close-004".to_owned(),
                environment: ExecutionEnvironment::Paper,
                rationale: "flatten test position".to_owned(),
                reference_price: "151.00000000".to_owned(),
                reference_observed_at: PaperOmsGateway::now_canonical(),
            })
            .expect("close should be accepted");
        assert_eq!(close_receipt.status, CommandStatus::Filled);

        let service = gateway.service.lock().unwrap();
        let dashboard = service.dashboard();
        let position = dashboard
            .positions
            .iter()
            .find(|position| position.instrument_id == "inst.us_equity.aapl");
        assert!(position.is_none_or(|position| position.quantity == "0.00000000"));
        drop(service);
        cleanup(scratch);
    }

    #[test]
    fn close_position_without_an_open_position_is_rejected() {
        let (gateway, scratch, _journal) = test_gateway("close-empty");
        let error = gateway
            .close_position(ClosePositionIntent {
                request_id: "request.close.005".to_owned(),
                account_id: "acct.desktop.paper.test".to_owned(),
                instrument_id: "inst.us_equity.aapl".to_owned(),
                correlation_id: "corr-close-005".to_owned(),
                environment: ExecutionEnvironment::Paper,
                rationale: "no position to close".to_owned(),
                reference_price: "151.00000000".to_owned(),
                reference_observed_at: PaperOmsGateway::now_canonical(),
            })
            .unwrap_err();
        assert!(
            matches!(error, TradingCommandError::Validation(message) if message.contains("no open position"))
        );
        cleanup(scratch);
    }

    #[test]
    fn cancel_order_requests_cancellation_through_real_oms() {
        let (gateway, scratch, _journal) = test_gateway("cancel");
        // A limit below the reference price is not marketable and never
        // fills, so it stays cancellable, but it must stay within the test
        // config's 500 bps price collar (reference 150.00) or the real risk
        // engine rejects it outright before an order is even created.
        let receipt = gateway
            .submit_order(order_intent(
                "intent.desktop.006",
                OrderType::Limit,
                Some("145.00000000"),
            ))
            .expect("resting limit order should be accepted");
        let order_id = receipt.order_id.expect("order was created");
        {
            let service = gateway.service.lock().unwrap();
            let order = service.order(&order_id).expect("order exists");
            assert_eq!(order.oms.state, OrderState::Acknowledged);
        }

        let cancel_receipt = gateway
            .cancel_order(CancelOrderIntent {
                request_id: "request.cancel.006".to_owned(),
                account_id: "acct.desktop.paper.test".to_owned(),
                order_id: order_id.clone(),
                correlation_id: "corr-cancel-006".to_owned(),
                environment: ExecutionEnvironment::Paper,
            })
            .expect("cancel should be accepted");
        assert_eq!(cancel_receipt.status, CommandStatus::Cancelled);
        let service = gateway.service.lock().unwrap();
        let order = service.order(&order_id).expect("order still exists");
        assert_eq!(order.oms.state, OrderState::Cancelled);
        drop(service);

        let retry_receipt = gateway
            .cancel_order(CancelOrderIntent {
                request_id: "request.cancel.006".to_owned(),
                account_id: "acct.desktop.paper.test".to_owned(),
                order_id: order_id.clone(),
                correlation_id: "corr-cancel-006".to_owned(),
                environment: ExecutionEnvironment::Paper,
            })
            .expect("an identical cancellation retry must be idempotent");
        assert_eq!(retry_receipt.status, CommandStatus::Cancelled);
        assert_eq!(retry_receipt.order_id.as_deref(), Some(order_id.as_str()));
        cleanup(scratch);
    }

    #[test]
    fn cancel_order_rejects_a_mismatched_account_without_mutating_the_order() {
        let (gateway, scratch, _journal) = test_gateway("cancel-account-isolation");
        let receipt = gateway
            .submit_order(order_intent(
                "intent.desktop.008",
                OrderType::Limit,
                Some("145.00000000"),
            ))
            .expect("resting limit order should be accepted");
        let order_id = receipt.order_id.expect("order was created");

        let error = gateway
            .cancel_order(CancelOrderIntent {
                request_id: "request.cancel.008".to_owned(),
                account_id: "acct.desktop.paper.other".to_owned(),
                order_id: order_id.clone(),
                correlation_id: "corr-cancel-008".to_owned(),
                environment: ExecutionEnvironment::Paper,
            })
            .expect_err("another account must not be able to cancel this order");
        assert!(
            matches!(error, TradingCommandError::Validation(message) if message.contains("account_id does not match"))
        );

        let service = gateway.service.lock().unwrap();
        assert_eq!(
            service.order(&order_id).expect("order remains").oms.state,
            OrderState::Acknowledged
        );
        drop(service);
        cleanup(scratch);
    }

    const SHORT_PERMISSION: &str = r#""short_exposure": { "max_short_quantity": "10" },"#;
    const LONG_LEG: &str = "inst.opt.spy.c500";
    const SHORT_LEG: &str = "inst.opt.spy.c505";

    /// A two-leg vertical: buy `LONG_LEG`, sell `SHORT_LEG`, two units.
    /// Each tuple is (limit price, attested reference price).
    fn vertical(
        intent_id: &str,
        long: (&str, &str),
        short: (&str, &str),
        maximum_debit: &str,
    ) -> ComboOrderIntent {
        let leg = |instrument_id: &str, side, (limit, reference): (&str, &str)| ComboLegIntent {
            instrument_id: instrument_id.to_owned(),
            side,
            ratio: 1,
            limit_price: limit.to_owned(),
            reference_price: reference.to_owned(),
            reference_observed_at: PaperOmsGateway::now_canonical(),
        };
        ComboOrderIntent {
            intent_id: intent_id.to_owned(),
            account_id: "acct.desktop.paper.test".to_owned(),
            strategy_id: "desktop.manual".to_owned(),
            correlation_id: format!("corr-{intent_id}"),
            legs: vec![
                leg(LONG_LEG, OrderSide::Buy, long),
                leg(SHORT_LEG, OrderSide::Sell, short),
            ],
            combo_quantity: "2".to_owned(),
            price_limit_kind: ComboPriceLimitKind::MaximumDebit,
            price_limit: maximum_debit.to_owned(),
            time_in_force: TimeInForce::Day,
            rationale: "gateway combination test".to_owned(),
            created_at: PaperOmsGateway::now_canonical(),
            strategy_version: "desktop.v1".to_owned(),
            configuration_version: "desktop.v1".to_owned(),
            environment: ExecutionEnvironment::Paper,
        }
    }

    /// Net 2.00 debit at the references, protected at 2.60: marketable.
    fn marketable_vertical(intent_id: &str) -> ComboOrderIntent {
        vertical(intent_id, ("50.5", "50"), ("47.9", "48"), "2.6")
    }

    /// Net 2.00 debit at the references, protected at 1.50: not marketable.
    fn resting_vertical(intent_id: &str) -> ComboOrderIntent {
        vertical(intent_id, ("49", "50"), ("47.5", "48"), "1.5")
    }

    fn position(gateway: &PaperOmsGateway, instrument_id: &str) -> Option<String> {
        let service = gateway.service.lock().unwrap();
        service
            .dashboard()
            .positions
            .into_iter()
            .find(|position| position.instrument_id == instrument_id)
            .map(|position| position.quantity)
    }

    #[test]
    fn submit_combo_fills_atomically_through_real_risk_and_oms() {
        let (gateway, scratch, _journal) = test_gateway_with("combo-fill", SHORT_PERMISSION);
        let intent = marketable_vertical("intent.desktop.combo.001");

        let receipt = gateway
            .submit_combo(intent.clone())
            .expect("a marketable combination should be accepted");

        assert_eq!(receipt.command, TradingCommandKind::SubmitCombo);
        assert_eq!(receipt.status, CommandStatus::Filled);
        let order_id = receipt.order_id.clone().expect("one OMS order exists");
        {
            let service = gateway.service.lock().unwrap();
            let order = service.combo_order(&order_id).expect("combination exists");
            assert_eq!(order.oms.state, OrderState::Filled);
            assert_eq!(order.filled_quantity, "2".parse::<Decimal>().unwrap());
            // One combination is one order: no plain order was created for a leg.
            assert!(service.order(&order_id).is_none());
            assert_eq!(service.dashboard().working_orders, 0);
        }
        assert_eq!(position(&gateway, LONG_LEG).as_deref(), Some("2.00000000"));
        assert_eq!(
            position(&gateway, SHORT_LEG).as_deref(),
            Some("-2.00000000")
        );

        // An identical retry is answered from durable evidence, not re-executed.
        let retry = gateway
            .submit_combo(intent)
            .expect("an identical retry must be idempotent");
        assert_eq!(retry.order_id, receipt.order_id);
        assert_eq!(retry.status, CommandStatus::Filled);
        assert_eq!(position(&gateway, LONG_LEG).as_deref(), Some("2.00000000"));
        assert_eq!(
            position(&gateway, SHORT_LEG).as_deref(),
            Some("-2.00000000")
        );
        cleanup(scratch);
    }

    #[test]
    fn submit_combo_with_a_short_leg_is_refused_without_configured_permission() {
        let (gateway, scratch, _journal) = test_gateway("combo-no-short");

        let receipt = gateway
            .submit_combo(marketable_vertical("intent.desktop.combo.002"))
            .expect("a risk rejection is a normal, successful outcome");

        assert_eq!(receipt.status, CommandStatus::RiskRejected);
        assert!(receipt.order_id.is_none());
        assert!(receipt
            .message
            .contains("POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED"));
        assert!(position(&gateway, LONG_LEG).is_none());
        assert!(position(&gateway, SHORT_LEG).is_none());
        cleanup(scratch);
    }

    #[test]
    fn non_marketable_combo_rests_and_its_cancellation_is_visible() {
        let (gateway, scratch, _journal) = test_gateway_with("combo-cancel", SHORT_PERMISSION);
        let receipt = gateway
            .submit_combo(resting_vertical("intent.desktop.combo.003"))
            .expect("a resting combination should be accepted");
        assert_eq!(receipt.status, CommandStatus::Acknowledged);
        let order_id = receipt.order_id.expect("combination was created");
        assert!(position(&gateway, LONG_LEG).is_none());

        let cancel = CancelOrderIntent {
            request_id: "request.cancel.combo.003".to_owned(),
            account_id: "acct.desktop.paper.test".to_owned(),
            order_id: order_id.clone(),
            correlation_id: "corr-cancel-combo-003".to_owned(),
            environment: ExecutionEnvironment::Paper,
        };
        let cancelled = gateway
            .cancel_order(cancel.clone())
            .expect("the combination should be cancellable");
        assert_eq!(cancelled.status, CommandStatus::Cancelled);
        assert!(cancelled.message.contains("Cancelled"));
        assert_eq!(
            gateway
                .service
                .lock()
                .unwrap()
                .combo_order(&order_id)
                .expect("combination remains")
                .oms
                .state,
            OrderState::Cancelled
        );

        let retry = gateway
            .cancel_order(cancel)
            .expect("an identical cancellation retry must be idempotent");
        assert_eq!(retry.status, CommandStatus::Cancelled);
        cleanup(scratch);
    }

    #[test]
    fn debit_protected_combo_observed_at_a_credit_rests_rather_than_fills() {
        let (gateway, scratch, _journal) = test_gateway_with("combo-sign", SHORT_PERMISSION);
        // Protected net 0.50 debit; the references price the structure at a
        // 2.00 *credit*. That is not the trade the operator approved.
        let receipt = gateway
            .submit_combo(vertical(
                "intent.desktop.combo.004",
                ("49", "48"),
                ("48.5", "50"),
                "0.5",
            ))
            .expect("the combination should be accepted by risk");
        assert_eq!(receipt.status, CommandStatus::Acknowledged);
        let order_id = receipt.order_id.expect("combination was created");
        assert_eq!(
            gateway
                .service
                .lock()
                .unwrap()
                .combo_order(&order_id)
                .expect("combination exists")
                .filled_quantity,
            Decimal::ZERO
        );
        cleanup(scratch);
    }

    #[test]
    fn each_leg_is_collared_against_its_own_attested_reference() {
        let (gateway, scratch, _journal) = test_gateway_with("combo-collar", SHORT_PERMISSION);
        // The long leg's limit (60) is 2000 bps from its reference (50). A
        // gateway that priced a leg from anything but its own attested
        // observation -- its limit, say -- would let this through.
        let receipt = gateway
            .submit_combo(vertical(
                "intent.desktop.combo.006",
                ("60", "50"),
                ("47.9", "48"),
                "12.1",
            ))
            .expect("a risk rejection is a normal outcome");
        assert_eq!(receipt.status, CommandStatus::RiskRejected);
        assert!(receipt.message.contains("PRICE_COLLAR_EXCEEDED"));
        cleanup(scratch);
    }

    #[test]
    fn a_risk_rejected_combo_leaves_no_armed_reference_behind() {
        let (gateway, scratch, _journal) = test_gateway("combo-disarm");
        gateway
            .submit_combo(marketable_vertical("intent.desktop.combo.005"))
            .expect("a risk rejection is a normal outcome");
        assert!(gateway
            .service
            .lock()
            .unwrap()
            .broker_mut()
            .next_combo_reference
            .is_none());
        cleanup(scratch);
    }

    #[test]
    fn bootstrap_falls_back_to_none_without_the_environment_variable() {
        std::env::remove_var("FOLLON_DESKTOP_PAPER_CONFIG");
        assert!(bootstrap().is_none());
    }
}
