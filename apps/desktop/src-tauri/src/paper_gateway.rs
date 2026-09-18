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
//!   be generic over a different concrete `PaperBrokerAdapter`.
//!
//! Every other Risk/OMS guarantee (rate limiting, price collar, kill
//! switches, idempotent submission, durable append-only audit journal) is the
//! real thing: this module is a thin translation layer over
//! `follon_paper::PaperTradingService`, not a reimplementation of it.

use std::path::PathBuf;
use std::sync::Mutex;

use follon_domain::{
    Decimal, OrderIntent as DomainOrderIntent, OrderState, OrderType as DomainOrderType,
    Side as DomainSide, TimeInForce as DomainTimeInForce,
};
use follon_paper::{
    BrokerCancelRequest, BrokerOrderRequest, BrokerSubmitResult, IbkrPaperAdapter,
    KillSwitchRegistry, PaperAccount, PaperBrokerAdapter, PaperError, PaperMarketData,
    PaperRiskPolicy, PaperTradingService,
};
use serde::Deserialize;
use time::OffsetDateTime;

use crate::trading::{
    CancelOrderIntent, ClosePositionIntent, CommandReceipt, CommandStatus, OrderIntent, OrderSide,
    OrderType, RiskOmsGateway, TimeInForce, TradingCommandError, TradingCommandKind,
};

/// Wraps the in-process simulated IBKR paper adapter so a successfully
/// acknowledged order is evaluated against a reference price set by the
/// caller just before submission -- there is no real matching engine behind
/// this desktop boundary to wait on. A market order, or a limit order that is
/// genuinely marketable against that reference price, fills immediately in
/// full at the reference price; a non-marketable limit order is left
/// resting (`Acknowledged`), exactly like a real limit order untouched until
/// the price moves, and remains cancellable.
struct ManualFillAdapter {
    inner: IbkrPaperAdapter,
    next_reference: Option<(Decimal, String)>,
}

impl ManualFillAdapter {
    fn new(account: &PaperAccount) -> Result<Self, PaperError> {
        Ok(Self {
            inner: IbkrPaperAdapter::new(account)?,
            next_reference: None,
        })
    }

    /// Arms the reference price/time the next `submit()` call will evaluate
    /// fillability against.
    fn arm_next_reference(&mut self, reference_price: Decimal, executed_at: String) {
        self.next_reference = Some((reference_price, executed_at));
    }
}

impl PaperBrokerAdapter for ManualFillAdapter {
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
        let state = service.order(&intent.order_id).map(|order| order.oms.state);
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
/// Intentionally mirrors the `PaperConfigurationDocument` shape parsed by
/// `follon-paper-status` (`apps/cli/src/paper.rs`) closely enough that the
/// same operator-authored file can back both the read-only CLI dashboard and
/// this live desktop gateway, though the two are parsed independently to
/// avoid a cross-workspace dependency between the CLI and Tauri crates.
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
    kill_switch_version: String,
    journal_path: String,
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
    use crate::trading::{ExecutionEnvironment, OrderIntent, OrderSide, OrderType, TimeInForce};
    use follon_domain::OrderState;

    fn test_gateway(name: &str) -> (PaperOmsGateway, PathBuf, PathBuf) {
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

    #[test]
    fn bootstrap_falls_back_to_none_without_the_environment_variable() {
        std::env::remove_var("FOLLON_DESKTOP_PAPER_CONFIG");
        assert!(bootstrap().is_none());
    }
}
