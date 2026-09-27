//! Deployed gRPC topology for broker-neutral execution planning, portfolio
//! risk, multi-currency margin valuation, and configured risk-gated PAPER
//! combination submission.
//!
//! Every write RPC requires a bearer session from an operator in the
//! configured operator directory: password plus a mandatory TOTP second
//! factor. `SubmitPaperCombo` needs a role that grants PAPER trading, and
//! `ActivatePaperKillSwitch` and `ReleasePaperKillSwitch` need one that grants
//! kill-switch operation (risk_manager), each in the request's tenant. The
//! directory serves one tenant, so a route is reachable only by that tenant's
//! operators, and the PAPER journal records who submitted and who moved a
//! switch.
//!
//! `ActivateLiveKillSwitch` and `ReleaseLiveKillSwitch` need the same
//! kill-switch permission, against a configured controlled-LIVE route. That
//! route opens the controlled-LIVE journal with an adapter that refuses every
//! broker operation, so it can halt controlled LIVE but never place, cancel or
//! reconcile an order. The LIVE journal records the operator of every change.

use std::collections::BTreeMap;
use std::env;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use follon_accounting::{
    value_margin_account, Currency, FxBook, FxQuote, MarginPolicy, MarginPosition, MarginRate,
};
use follon_domain::{
    validate_canonical_id, ComboIntent, ComboIntentLeg, Decimal, OrderState, Side, TimeInForce,
};
use follon_execution::{
    plan_execution, plan_option_combo, plan_passive_repricing, ChildInstruction as CoreChild,
    ChildOrderKind as CoreChildOrderKind, ComboPriceLimit, ExecutionAlgorithm, OptionComboLeg,
    ParentOrder, PassiveMarketObservation, PassiveRepricePolicy,
};
use follon_identity::{IdentityService, LoginOutcome, OperatorDirectory, Permission};
use follon_live::{
    LiveBrokerAccountSnapshot, LiveBrokerAdapter, LiveBrokerEvent, LiveBrokerOrderRequest,
    LiveBrokerSubmitResult, LiveConfiguration, LiveError, LiveKillSwitchScope, LiveTradingService,
};
use follon_paper::{
    IbkrPaperAdapter, KillSwitchRegistry, KillSwitchScope, PaperAccount, PaperComboMarketData,
    PaperMarketData, PaperRiskPolicy, PaperTradingService, ShortExposurePolicy,
};
use follon_postgres::{PersistenceError, PostgresStore};
use follon_risk::{
    evaluate_portfolio_risk, CandidateOrder as CoreCandidateOrder, PortfolioRiskPolicy,
    PortfolioRiskSnapshot, RestingOrder as CoreRestingOrder, RiskPosition,
};
use follon_secrets::SecretMaterial;
use serde::Deserialize;
use tokio::signal;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use tonic::{Request, Response, Status};

/// Generated versioned protobuf service contracts.
#[allow(missing_docs)]
pub mod api {
    tonic::include_proto!("follon.trading.v1");
}

use api::trading_operating_system_server::{TradingOperatingSystem, TradingOperatingSystemServer};
use api::{
    BeginOperatorLoginRequest, BeginOperatorLoginResponse, CompleteOperatorLoginRequest,
    LiveKillSwitchRequest, LiveKillSwitchResponse, OperatorSession, PaperKillSwitchRequest,
    PaperKillSwitchResponse, RevokeOperatorSessionRequest, RevokeOperatorSessionResponse,
};
use api::{
    BucketLimit, CancelReplaceInstruction, ChildInstruction, ChildOrderKind, ComboLegInstruction,
    ComboPriceLimitKind, CurrencyAmount, ExecutionAlgorithmKind, ExecutionPlanRequest,
    ExecutionPlanResponse, ExecutionSide, HealthRequest, HealthResponse, MarginAccountRequest,
    MarginAccountResponse, OmsOrderState, OptionComboRequest, OptionComboResponse,
    OrderTimeInForceKind, PaperComboMarketObservation, PassiveRepricingRequest,
    PassiveRepricingResponse, PortfolioRiskRequest, PortfolioRiskResponse, RiskMetrics,
    SubmitPaperComboRequest, SubmitPaperComboResponse,
};

type PaperComboRoute = Arc<Mutex<PaperTradingService<IbkrPaperAdapter>>>;
type LiveKillSwitchRoute = Arc<Mutex<LiveTradingService<KillSwitchOnlyLiveAdapter>>>;
type OperatorIdentity = Arc<Mutex<IdentityService>>;

#[derive(Clone)]
struct OperatingSystemService {
    database: Option<Arc<Mutex<PostgresStore>>>,
    paper_combo_route: Option<PaperComboRoute>,
    /// Controlled-LIVE kill switches only; `None` refuses every LIVE change.
    live_kill_switch_route: Option<LiveKillSwitchRoute>,
    /// Operators allowed to call write RPCs; `None` refuses every write.
    identity: Option<OperatorIdentity>,
    transport_tls: bool,
}

impl OperatingSystemService {
    fn identity(&self) -> Result<std::sync::MutexGuard<'_, IdentityService>, Status> {
        self.identity
            .as_ref()
            .ok_or_else(|| {
                Status::failed_precondition(
                    "operator identity is not configured; no write is accepted",
                )
            })?
            .lock()
            .map_err(|_| Status::internal("operator identity lock poisoned"))
    }

    /// Activates or releases one PAPER kill switch for an operator whose role
    /// grants kill-switch operation. Authorization precedes every other check,
    /// and the change is journaled with the operator and the server's time.
    fn operate_paper_kill_switch(
        &self,
        request: Request<PaperKillSwitchRequest>,
        activate: bool,
    ) -> Result<Response<PaperKillSwitchResponse>, Status> {
        let token = bearer_token(&request)?;
        let request = request.into_inner();
        validate_tenant(&request.tenant_id)?;
        let now = now_epoch_seconds()?;
        let operator = self
            .identity()?
            .authorize(
                &token,
                &request.tenant_id,
                Permission::KillSwitchOperate,
                now,
            )
            .map_err(|_| Status::permission_denied("access denied"))?;
        let scope = KillSwitchScope::from_key(&request.scope)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let route = self.paper_combo_route.as_ref().ok_or_else(|| {
            Status::failed_precondition(
                "PAPER Risk/OMS route is not configured; no kill switch was changed",
            )
        })?;
        let operated_at = utc_timestamp(now)?;
        let mut service = route
            .lock()
            .map_err(|_| Status::internal("PAPER route lock poisoned"))?;
        let changed = if activate {
            service.activate_kill_switch_as(scope.clone(), &operator.user_id, &operated_at)
        } else {
            service.release_kill_switch_as(scope.clone(), &operator.user_id, &operated_at)
        }
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
        Ok(Response::new(PaperKillSwitchResponse {
            scope: scope.as_key(),
            changed,
            active_kill_switches: service.kill_switches().active_keys(),
            operated_by: operator.user_id,
            operated_at,
        }))
    }

    /// Activates or releases one controlled-LIVE kill switch, under exactly
    /// the PAPER rule: authorization precedes every other check, and the LIVE
    /// journal records the operator and the server's time (E3.3c).
    fn operate_live_kill_switch(
        &self,
        request: Request<LiveKillSwitchRequest>,
        activate: bool,
    ) -> Result<Response<LiveKillSwitchResponse>, Status> {
        let token = bearer_token(&request)?;
        let request = request.into_inner();
        validate_tenant(&request.tenant_id)?;
        let now = now_epoch_seconds()?;
        let operator = self
            .identity()?
            .authorize(
                &token,
                &request.tenant_id,
                Permission::KillSwitchOperate,
                now,
            )
            .map_err(|_| Status::permission_denied("access denied"))?;
        let scope = LiveKillSwitchScope::from_key(&request.scope)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let route = self.live_kill_switch_route.as_ref().ok_or_else(|| {
            Status::failed_precondition(
                "controlled-LIVE kill-switch route is not configured; no kill switch was changed",
            )
        })?;
        let operated_at = utc_timestamp(now)?;
        let mut service = route
            .lock()
            .map_err(|_| Status::internal("controlled-LIVE route lock poisoned"))?;
        let changed = if activate {
            service.activate_kill_switch(scope.clone(), &operator.user_id, &operated_at)
        } else {
            service.deactivate_kill_switch(&scope, &operator.user_id, &operated_at)
        }
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
        Ok(Response::new(LiveKillSwitchResponse {
            scope: scope.as_key(),
            changed,
            active_kill_switches: service.kill_switches().active_keys(),
            operated_by: operator.user_id,
            operated_at,
        }))
    }
}

/// The controlled-LIVE route's broker adapter. The route exists to move kill
/// switches, which need no broker, so every broker operation is refused: this
/// process can halt controlled LIVE but can never trade it.
struct KillSwitchOnlyLiveAdapter;

impl KillSwitchOnlyLiveAdapter {
    fn refused<T>() -> Result<T, LiveError> {
        Err(LiveError(
            "the trading API's controlled-LIVE route operates kill switches only and holds no broker connection"
                .to_owned(),
        ))
    }
}

impl LiveBrokerAdapter for KillSwitchOnlyLiveAdapter {
    fn connect(&mut self, _: &str, _: &SecretMaterial) -> Result<(), LiveError> {
        Self::refused()
    }

    fn submit(&mut self, _: &LiveBrokerOrderRequest) -> Result<LiveBrokerSubmitResult, LiveError> {
        Self::refused()
    }

    fn cancel(&mut self, _: &str) -> Result<(), LiveError> {
        Self::refused()
    }

    fn poll(&mut self) -> Result<Vec<LiveBrokerEvent>, LiveError> {
        Self::refused()
    }

    fn snapshot(&mut self, _: &str) -> Result<LiveBrokerAccountSnapshot, LiveError> {
        Self::refused()
    }

    fn reconnect(&mut self, _: &str, _: &SecretMaterial) -> Result<(), LiveError> {
        Self::refused()
    }
}

/// The opaque session token from `authorization: Bearer <token>`. Anything
/// else is refused before the request body is even read.
fn bearer_token<T>(request: &Request<T>) -> Result<String, Status> {
    let malformed = || Status::unauthenticated("a bearer operator session is required");
    let header = request
        .metadata()
        .get("authorization")
        .ok_or_else(malformed)?
        .to_str()
        .map_err(|_| malformed())?;
    let token = header.strip_prefix("Bearer ").ok_or_else(malformed)?;
    if token.len() != 64
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(malformed());
    }
    Ok(token.to_owned())
}

fn now_epoch_seconds() -> Result<i64, Status> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .ok_or_else(|| Status::internal("system clock is before the Unix epoch"))
}

/// The canonical second-precision UTC timestamp (`YYYY-MM-DDTHH:MM:SSZ`) the
/// PAPER journal requires, for one epoch second.
fn utc_timestamp(epoch_seconds: i64) -> Result<String, Status> {
    let at = time::OffsetDateTime::from_unix_timestamp(epoch_seconds)
        .map_err(|_| Status::internal("system clock is out of range"))?;
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second()
    ))
}

#[tonic::async_trait]
impl TradingOperatingSystem for OperatingSystemService {
    async fn check_health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        let database_ready = if let Some(database) = &self.database {
            database
                .lock()
                .map_err(|_| Status::internal("database lock poisoned"))?
                .health_check()
                .is_ok()
        } else {
            false
        };
        let status = if self.database.is_none() || database_ready {
            "SERVING"
        } else {
            "DEGRADED"
        };
        Ok(Response::new(HealthResponse {
            status: status.to_owned(),
            database_ready,
            transport_tls: self.transport_tls,
            service_version: env!("CARGO_PKG_VERSION").to_owned(),
        }))
    }

    async fn plan_execution(
        &self,
        request: Request<ExecutionPlanRequest>,
    ) -> Result<Response<ExecutionPlanResponse>, Status> {
        let request = request.into_inner();
        validate_tenant(&request.tenant_id)?;
        validate_id("strategy_id", &request.strategy_id)?;
        let parent = ParentOrder {
            parent_order_id: request.parent_order_id,
            account_id: request.account_id,
            instrument_id: request.instrument_id,
            side: side(request.side)?,
            quantity: decimal("quantity", &request.quantity)?,
            limit_price: request
                .limit_price
                .as_deref()
                .map(|value| decimal("limit_price", value))
                .transpose()?,
        };
        let algorithm = match ExecutionAlgorithmKind::try_from(request.algorithm) {
            Ok(ExecutionAlgorithmKind::Immediate) => ExecutionAlgorithm::Immediate,
            Ok(ExecutionAlgorithmKind::Twap) => ExecutionAlgorithm::Twap {
                slice_count: request.slice_count,
                interval_seconds: request.interval_seconds,
            },
            Ok(ExecutionAlgorithmKind::Vwap) => ExecutionAlgorithm::Vwap {
                forecast_market_volumes: request
                    .forecast_market_volumes
                    .iter()
                    .map(|value| decimal("forecast_market_volume", value))
                    .collect::<Result<Vec<_>, _>>()?,
                interval_seconds: request.interval_seconds,
            },
            Ok(ExecutionAlgorithmKind::Participation) => ExecutionAlgorithm::Participation {
                participation_bps: request.participation_bps,
                interval_seconds: request.interval_seconds,
                observed_market_volumes: request
                    .observed_market_volumes
                    .iter()
                    .map(|value| decimal("observed_market_volume", value))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            Ok(ExecutionAlgorithmKind::ArrivalPrice) => ExecutionAlgorithm::ArrivalPrice {
                slice_count: request.slice_count,
                interval_seconds: request.interval_seconds,
                urgency_bps: request.urgency_bps,
            },
            _ => return Err(Status::invalid_argument("execution algorithm is required")),
        };
        let plan = plan_execution(&parent, &algorithm)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        Ok(Response::new(ExecutionPlanResponse {
            parent_order_id: plan.parent_order_id,
            children: plan.children.into_iter().map(child_instruction).collect(),
            unallocated_quantity: plan.unallocated_quantity.to_string(),
        }))
    }

    async fn plan_passive_repricing(
        &self,
        request: Request<PassiveRepricingRequest>,
    ) -> Result<Response<PassiveRepricingResponse>, Status> {
        let request = request.into_inner();
        validate_tenant(&request.tenant_id)?;
        validate_id("strategy_id", &request.strategy_id)?;
        let parent = ParentOrder {
            parent_order_id: request.parent_order_id,
            account_id: request.account_id,
            instrument_id: request.instrument_id,
            side: side(request.side)?,
            quantity: decimal("quantity", &request.quantity)?,
            limit_price: request
                .hard_limit_price
                .as_deref()
                .map(|value| decimal("hard_limit_price", value))
                .transpose()?,
        };
        let policy = PassiveRepricePolicy {
            initial_limit_price: decimal("initial_limit_price", &request.initial_limit_price)?,
            tick_size: decimal("tick_size", &request.tick_size)?,
            maximum_chase_bps: request.maximum_chase_bps,
            maximum_replacements: request.maximum_replacements,
            minimum_replace_interval_seconds: request.minimum_replace_interval_seconds,
        };
        let observations = request
            .observations
            .iter()
            .map(|observation| {
                Ok(PassiveMarketObservation {
                    observed_after_seconds: observation.observed_after_seconds,
                    best_bid: decimal("observation.best_bid", &observation.best_bid)?,
                    best_ask: decimal("observation.best_ask", &observation.best_ask)?,
                })
            })
            .collect::<Result<Vec<_>, Status>>()?;
        let plan = plan_passive_repricing(&parent, &policy, &observations)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        Ok(Response::new(PassiveRepricingResponse {
            initial: Some(child_instruction(plan.initial)),
            replacements: plan
                .replacements
                .into_iter()
                .map(|instruction| CancelReplaceInstruction {
                    cancel_child_id: instruction.cancel_child_order_id,
                    replacement: Some(child_instruction(instruction.replacement)),
                })
                .collect(),
        }))
    }

    async fn plan_option_combo(
        &self,
        request: Request<OptionComboRequest>,
    ) -> Result<Response<OptionComboResponse>, Status> {
        let request = request.into_inner();
        validate_tenant(&request.tenant_id)?;
        validate_id("account_id", &request.account_id)?;
        validate_id("strategy_id", &request.strategy_id)?;
        let limit = decimal("price_limit", &request.price_limit)?;
        let price_limit = match ComboPriceLimitKind::try_from(request.price_limit_kind) {
            Ok(ComboPriceLimitKind::MaximumDebit) => ComboPriceLimit::MaximumDebit(limit),
            Ok(ComboPriceLimitKind::MinimumCredit) => ComboPriceLimit::MinimumCredit(limit),
            _ => {
                return Err(Status::invalid_argument(
                    "combo price limit kind is required",
                ))
            }
        };
        let legs = request
            .legs
            .iter()
            .map(|leg| {
                Ok(OptionComboLeg {
                    instrument_id: leg.instrument_id.clone(),
                    side: side(leg.side)?,
                    ratio: leg.ratio,
                    limit_price: decimal("leg.limit_price", &leg.limit_price)?,
                })
            })
            .collect::<Result<Vec<_>, Status>>()?;
        let plan = plan_option_combo(
            &request.combo_id,
            decimal("combo_quantity", &request.combo_quantity)?,
            price_limit,
            &legs,
        )
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
        Ok(Response::new(OptionComboResponse {
            combo_id: plan.combo_id,
            combo_quantity: plan.combo_quantity.to_string(),
            protected_net_price: plan.protected_net_price.to_string(),
            legs: plan
                .legs
                .into_iter()
                .map(|leg| ComboLegInstruction {
                    child_id: leg.child_order_id,
                    instrument_id: leg.instrument_id,
                    side: execution_side(leg.side) as i32,
                    quantity: leg.quantity.to_string(),
                    limit_price: leg.limit_price.to_string(),
                })
                .collect(),
        }))
    }

    async fn submit_paper_combo(
        &self,
        request: Request<SubmitPaperComboRequest>,
    ) -> Result<Response<SubmitPaperComboResponse>, Status> {
        let token = bearer_token(&request)?;
        let request = request.into_inner();
        validate_tenant(&request.tenant_id)?;
        // Authorization precedes every other check, so an unauthorized caller
        // learns nothing about the route, the intent, or the market data.
        let operator = self
            .identity()?
            .authorize(
                &token,
                &request.tenant_id,
                Permission::PaperTrade,
                now_epoch_seconds()?,
            )
            .map_err(|_| Status::permission_denied("access denied"))?;
        let intent = paper_combo_intent(&request)?;
        let market = paper_combo_market(&request.observations)?;
        let route = self.paper_combo_route.as_ref().ok_or_else(|| {
            Status::failed_precondition(
                "PAPER combination Risk/OMS route is not configured; no order was sent",
            )
        })?;
        let mut service = route
            .lock()
            .map_err(|_| Status::internal("PAPER combination route lock poisoned"))?;
        let outcome = service
            .submit_combo_intent_as(intent, market, &request.decided_at, Some(&operator.user_id))
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        Ok(Response::new(SubmitPaperComboResponse {
            decision_id: outcome.decision.decision_id,
            approved: outcome.decision.approved,
            reason_codes: outcome.decision.reason_codes,
            policy_version: outcome.decision.policy_version,
            order_id: outcome.order_id,
            state: outcome
                .state
                .map(oms_order_state)
                .unwrap_or(OmsOrderState::Unspecified) as i32,
            submitted_by: operator.user_id,
        }))
    }

    async fn begin_operator_login(
        &self,
        request: Request<BeginOperatorLoginRequest>,
    ) -> Result<Response<BeginOperatorLoginResponse>, Status> {
        let request = request.into_inner();
        validate_tenant(&request.tenant_id)?;
        let mut identity = self.identity()?;
        let outcome = identity
            .begin_login(
                &request.tenant_id,
                &request.email,
                &request.password,
                now_epoch_seconds()?,
            )
            .map_err(|_| Status::unauthenticated("authentication failed"))?;
        match outcome {
            LoginOutcome::MfaRequired {
                challenge_token,
                expires_at_epoch_seconds,
            } => Ok(Response::new(BeginOperatorLoginResponse {
                challenge_token,
                expires_at_epoch_seconds,
            })),
            // The directory refuses an operator without TOTP, so this cannot
            // happen; if it ever does, the session is revoked, never issued.
            LoginOutcome::Authenticated(session) => {
                identity.revoke_session(&session.token);
                Err(Status::failed_precondition(
                    "operator login requires a second factor",
                ))
            }
        }
    }

    async fn complete_operator_login(
        &self,
        request: Request<CompleteOperatorLoginRequest>,
    ) -> Result<Response<OperatorSession>, Status> {
        let request = request.into_inner();
        let session = self
            .identity()?
            .complete_totp(
                &request.challenge_token,
                &request.totp_code,
                now_epoch_seconds()?,
            )
            .map_err(|_| Status::unauthenticated("authentication failed"))?;
        Ok(Response::new(OperatorSession {
            session_token: session.token,
            expires_at_epoch_seconds: session.expires_at_epoch_seconds,
        }))
    }

    async fn revoke_operator_session(
        &self,
        request: Request<RevokeOperatorSessionRequest>,
    ) -> Result<Response<RevokeOperatorSessionResponse>, Status> {
        let token = bearer_token(&request)?;
        let revoked = self.identity()?.revoke_session(&token);
        Ok(Response::new(RevokeOperatorSessionResponse { revoked }))
    }

    async fn activate_paper_kill_switch(
        &self,
        request: Request<PaperKillSwitchRequest>,
    ) -> Result<Response<PaperKillSwitchResponse>, Status> {
        self.operate_paper_kill_switch(request, true)
    }

    async fn release_paper_kill_switch(
        &self,
        request: Request<PaperKillSwitchRequest>,
    ) -> Result<Response<PaperKillSwitchResponse>, Status> {
        self.operate_paper_kill_switch(request, false)
    }

    async fn activate_live_kill_switch(
        &self,
        request: Request<LiveKillSwitchRequest>,
    ) -> Result<Response<LiveKillSwitchResponse>, Status> {
        self.operate_live_kill_switch(request, true)
    }

    async fn release_live_kill_switch(
        &self,
        request: Request<LiveKillSwitchRequest>,
    ) -> Result<Response<LiveKillSwitchResponse>, Status> {
        self.operate_live_kill_switch(request, false)
    }

    async fn evaluate_portfolio_risk(
        &self,
        request: Request<PortfolioRiskRequest>,
    ) -> Result<Response<PortfolioRiskResponse>, Status> {
        let request = request.into_inner();
        validate_tenant(&request.tenant_id)?;
        let policy = risk_policy(
            request
                .policy
                .as_ref()
                .ok_or_else(|| Status::invalid_argument("policy is required"))?,
        )?;
        let snapshot = PortfolioRiskSnapshot {
            equity: decimal("equity", &request.equity)?,
            peak_equity: decimal("peak_equity", &request.peak_equity)?,
            daily_pnl: decimal("daily_pnl", &request.daily_pnl)?,
            margin_used: decimal("margin_used", &request.margin_used)?,
            positions: request
                .positions
                .iter()
                .map(risk_position)
                .collect::<Result<Vec<_>, _>>()?,
            resting_orders: request
                .resting_orders
                .iter()
                .map(|order| {
                    Ok(CoreRestingOrder {
                        order_id: order.order_id.clone(),
                        account_id: order.account_id.clone(),
                        instrument_id: order.instrument_id.clone(),
                        side: side(order.side)?,
                    })
                })
                .collect::<Result<Vec<_>, Status>>()?,
            recent_order_count: request.recent_order_count,
        };
        let candidate = request
            .candidate
            .as_ref()
            .map(|candidate| -> Result<CoreCandidateOrder, Status> {
                Ok(CoreCandidateOrder {
                    intent_id: candidate.intent_id.clone(),
                    account_id: candidate.account_id.clone(),
                    strategy_id: candidate.strategy_id.clone(),
                    instrument_id: candidate.instrument_id.clone(),
                    asset_class: candidate.asset_class.clone(),
                    sector: candidate.sector.clone(),
                    currency: candidate.currency.clone(),
                    side: side(candidate.side)?,
                    quantity: decimal("candidate.quantity", &candidate.quantity)?,
                    mark_price: decimal("candidate.reference_price", &candidate.reference_price)?,
                    multiplier: decimal("candidate.multiplier", &candidate.multiplier)?,
                    delta: decimal("candidate.delta", &candidate.delta)?,
                    gamma: decimal("candidate.gamma", &candidate.gamma)?,
                })
            })
            .transpose()?;
        let decision = evaluate_portfolio_risk(&policy, &snapshot, candidate.as_ref())
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let metrics = decision.metrics;
        Ok(Response::new(PortfolioRiskResponse {
            approved: decision.approved,
            reason_codes: decision.reason_codes,
            policy_version: decision.policy_version,
            metrics: Some(RiskMetrics {
                gross_exposure: metrics.gross_exposure.to_string(),
                net_exposure: metrics.net_exposure.to_string(),
                leverage_bps: metrics.leverage_bps.to_string(),
                concentration_bps: metrics.concentration_bps.to_string(),
                drawdown_bps: metrics.drawdown_bps.to_string(),
                margin_utilization_bps: metrics.margin_utilization_bps.to_string(),
                total_delta: metrics.total_delta.to_string(),
                total_gamma: metrics.total_gamma.to_string(),
            }),
        }))
    }

    async fn value_margin_account(
        &self,
        request: Request<MarginAccountRequest>,
    ) -> Result<Response<MarginAccountResponse>, Status> {
        let request = request.into_inner();
        validate_tenant(&request.tenant_id)?;
        let base_currency = currency("base_currency", &request.base_currency)?;
        let mut fx = FxBook::default();
        for rate in &request.fx_rates {
            fx.upsert(FxQuote {
                base: currency("fx.base_currency", &rate.base_currency)?,
                quote: currency("fx.quote_currency", &rate.quote_currency)?,
                quote_rate: decimal("fx.rate", &rate.rate)?,
                observed_at_epoch_seconds: rate.observed_at_epoch_seconds,
            })
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        }
        let mut cash = BTreeMap::new();
        for amount in &request.cash {
            let currency = currency("cash.currency", &amount.currency)?;
            if cash
                .insert(currency, decimal("cash.amount", &amount.amount)?)
                .is_some()
            {
                return Err(Status::invalid_argument("duplicate cash currency"));
            }
        }
        let policy = MarginPolicy {
            base_currency,
            maximum_fx_age_seconds: request.maximum_fx_age_seconds,
            rates: request
                .margin_rates
                .iter()
                .map(|rate| {
                    (
                        rate.asset_class.clone(),
                        MarginRate {
                            initial_bps: rate.initial_bps,
                            maintenance_bps: rate.maintenance_bps,
                        },
                    )
                })
                .collect(),
        };
        let positions = request
            .positions
            .iter()
            .map(|position| {
                Ok(MarginPosition {
                    instrument_id: position.instrument_id.clone(),
                    asset_class: position.asset_class.clone(),
                    currency: currency("position.currency", &position.currency)?,
                    quantity: decimal("position.quantity", &position.quantity)?,
                    mark_price: decimal("position.mark_price", &position.mark_price)?,
                    multiplier: decimal("position.multiplier", &position.multiplier)?,
                })
            })
            .collect::<Result<Vec<_>, Status>>()?;
        let snapshot =
            value_margin_account(&cash, &positions, &fx, &policy, request.as_of_epoch_seconds)
                .map_err(|error| Status::invalid_argument(error.to_string()))?;
        Ok(Response::new(MarginAccountResponse {
            base_currency: snapshot.base_currency.as_str().to_owned(),
            cash_value: snapshot.cash_value.to_string(),
            position_market_value: snapshot.position_market_value.to_string(),
            net_liquidation_value: snapshot.net_liquidation_value.to_string(),
            initial_margin: snapshot.initial_margin.to_string(),
            maintenance_margin: snapshot.maintenance_margin.to_string(),
            excess_liquidity: snapshot.excess_liquidity.to_string(),
            margin_call: snapshot.margin_call,
            exposure_by_currency: snapshot
                .exposure_by_currency
                .into_iter()
                .map(|(currency, amount)| CurrencyAmount {
                    currency: currency.as_str().to_owned(),
                    amount: amount.to_string(),
                })
                .collect(),
        }))
    }
}

fn paper_combo_intent(request: &SubmitPaperComboRequest) -> Result<ComboIntent, Status> {
    if request.environment != "PAPER" {
        return Err(Status::invalid_argument(
            "SubmitPaperCombo accepts the PAPER environment only",
        ));
    }
    let price = decimal("price_limit", &request.price_limit)?;
    let price_limit = match ComboPriceLimitKind::try_from(request.price_limit_kind) {
        Ok(ComboPriceLimitKind::MaximumDebit) => ComboPriceLimit::MaximumDebit(price),
        Ok(ComboPriceLimitKind::MinimumCredit) => ComboPriceLimit::MinimumCredit(price),
        _ => {
            return Err(Status::invalid_argument(
                "combo price limit kind is required",
            ))
        }
    };
    let time_in_force = match OrderTimeInForceKind::try_from(request.time_in_force) {
        Ok(OrderTimeInForceKind::Day) => TimeInForce::Day,
        Ok(OrderTimeInForceKind::GoodTilCancelled) => TimeInForce::GoodTilCancelled,
        _ => return Err(Status::invalid_argument("time in force is required")),
    };
    let legs = request
        .legs
        .iter()
        .map(|leg| {
            Ok(ComboIntentLeg {
                instrument_id: leg.instrument_id.clone(),
                side: side(leg.side)?,
                ratio: leg.ratio,
                limit_price: decimal("leg.limit_price", &leg.limit_price)?,
            })
        })
        .collect::<Result<Vec<_>, Status>>()?;
    let intent = ComboIntent {
        intent_id: request.intent_id.clone(),
        account_id: request.account_id.clone(),
        strategy_id: request.strategy_id.clone(),
        correlation_id: request.correlation_id.clone(),
        legs,
        combo_quantity: decimal("combo_quantity", &request.combo_quantity)?,
        price_limit,
        time_in_force,
        rationale: request.rationale.clone(),
        created_at: request.created_at.clone(),
        strategy_version: request.strategy_version.clone(),
        configuration_version: request.configuration_version.clone(),
        environment: request.environment.clone(),
    };
    intent
        .validate()
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
    Ok(intent)
}

fn paper_combo_market(
    observations: &[PaperComboMarketObservation],
) -> Result<PaperComboMarketData, Status> {
    Ok(PaperComboMarketData {
        marks: observations
            .iter()
            .map(|observation| {
                Ok(PaperMarketData {
                    instrument_id: observation.instrument_id.clone(),
                    mark_price: decimal("observation.mark_price", &observation.mark_price)?,
                    observed_at: observation.observed_at.clone(),
                })
            })
            .collect::<Result<Vec<_>, Status>>()?,
    })
}

fn oms_order_state(state: OrderState) -> OmsOrderState {
    match state {
        OrderState::Created => OmsOrderState::Created,
        OrderState::PendingRisk => OmsOrderState::PendingRisk,
        OrderState::RiskRejected => OmsOrderState::RiskRejected,
        OrderState::Approved => OmsOrderState::Approved,
        OrderState::PendingSubmit => OmsOrderState::PendingSubmit,
        OrderState::Submitted => OmsOrderState::Submitted,
        OrderState::Acknowledged => OmsOrderState::Acknowledged,
        OrderState::PartiallyFilled => OmsOrderState::PartiallyFilled,
        OrderState::Filled => OmsOrderState::Filled,
        OrderState::PendingCancel => OmsOrderState::PendingCancel,
        OrderState::PendingReplace => OmsOrderState::PendingReplace,
        OrderState::Cancelled => OmsOrderState::Cancelled,
        OrderState::Rejected => OmsOrderState::Rejected,
        OrderState::Expired => OmsOrderState::Expired,
        OrderState::Unknown => OmsOrderState::Unknown,
    }
}

fn risk_policy(policy: &api::PortfolioRiskPolicy) -> Result<PortfolioRiskPolicy, Status> {
    Ok(PortfolioRiskPolicy {
        version: policy.version.clone(),
        global_kill_switch: policy.global_kill_switch,
        max_gross_exposure: decimal("max_gross_exposure", &policy.max_gross_exposure)?,
        max_abs_net_exposure: decimal("max_abs_net_exposure", &policy.max_abs_net_exposure)?,
        max_leverage_bps: decimal("max_leverage_bps", &policy.max_leverage_bps)?,
        max_concentration_bps: decimal("max_concentration_bps", &policy.max_concentration_bps)?,
        max_daily_loss: decimal("max_daily_loss", &policy.max_daily_loss)?,
        max_drawdown_bps: decimal("max_drawdown_bps", &policy.max_drawdown_bps)?,
        max_margin_utilization_bps: decimal(
            "max_margin_utilization_bps",
            &policy.max_margin_utilization_bps,
        )?,
        max_abs_delta: decimal("max_abs_delta", &policy.max_abs_delta)?,
        max_abs_gamma: decimal("max_abs_gamma", &policy.max_abs_gamma)?,
        max_open_orders: usize::try_from(policy.max_open_orders)
            .map_err(|_| Status::invalid_argument("max_open_orders is too large"))?,
        max_order_rate: policy.max_order_rate,
        allowed_instruments: policy.allowed_instruments.iter().cloned().collect(),
        restricted_instruments: policy.restricted_instruments.iter().cloned().collect(),
        sector_limits: bucket_limits("sector_limits", &policy.sector_limits)?,
        asset_class_limits: bucket_limits("asset_class_limits", &policy.asset_class_limits)?,
        currency_limits: bucket_limits("currency_limits", &policy.currency_limits)?,
        strategy_limits: bucket_limits("strategy_limits", &policy.strategy_limits)?,
        max_news_slippage_bps: None,
        max_spread_multiplier_bps: None,
    })
}

fn child_instruction(child: CoreChild) -> ChildInstruction {
    ChildInstruction {
        child_id: child.child_order_id,
        quantity: child.quantity.to_string(),
        release_after_seconds: child.scheduled_after_seconds,
        limit_price: child.limit_price.map(|value| value.to_string()),
        venue: child.venue,
        order_kind: match child.kind {
            CoreChildOrderKind::Market => ChildOrderKind::Market,
            CoreChildOrderKind::Limit => ChildOrderKind::Limit,
            CoreChildOrderKind::Stop => ChildOrderKind::Stop,
            CoreChildOrderKind::StopLimit => ChildOrderKind::StopLimit,
        } as i32,
        stop_price: child.stop_price.map(|value| value.to_string()),
    }
}

fn bucket_limits(name: &str, limits: &[BucketLimit]) -> Result<BTreeMap<String, Decimal>, Status> {
    let mut result = BTreeMap::new();
    for limit in limits {
        if result
            .insert(limit.key.clone(), decimal(name, &limit.limit)?)
            .is_some()
        {
            return Err(Status::invalid_argument(format!("duplicate {name} key")));
        }
    }
    Ok(result)
}

fn risk_position(position: &api::PortfolioPosition) -> Result<RiskPosition, Status> {
    Ok(RiskPosition {
        account_id: position.account_id.clone(),
        strategy_id: position.strategy_id.clone(),
        instrument_id: position.instrument_id.clone(),
        asset_class: position.asset_class.clone(),
        sector: position.sector.clone(),
        currency: position.currency.clone(),
        quantity: decimal("position.quantity", &position.quantity)?,
        mark_price: decimal("position.mark_price", &position.mark_price)?,
        multiplier: decimal("position.multiplier", &position.multiplier)?,
        delta: decimal("position.delta", &position.delta)?,
        gamma: decimal("position.gamma", &position.gamma)?,
    })
}

fn side(value: i32) -> Result<Side, Status> {
    match ExecutionSide::try_from(value) {
        Ok(ExecutionSide::Buy) => Ok(Side::Buy),
        Ok(ExecutionSide::Sell) => Ok(Side::Sell),
        _ => Err(Status::invalid_argument("side is required")),
    }
}

fn execution_side(side: Side) -> ExecutionSide {
    match side {
        Side::Buy => ExecutionSide::Buy,
        Side::Sell => ExecutionSide::Sell,
    }
}

fn decimal(name: &str, value: &str) -> Result<Decimal, Status> {
    Decimal::from_str(value)
        .map_err(|error| Status::invalid_argument(format!("invalid {name}: {error}")))
}

fn currency(name: &str, value: &str) -> Result<Currency, Status> {
    Currency::new(value)
        .map_err(|error| Status::invalid_argument(format!("invalid {name}: {error}")))
}

fn validate_tenant(tenant_id: &str) -> Result<(), Status> {
    validate_id("tenant_id", tenant_id)
}

fn validate_id(name: &str, value: &str) -> Result<(), Status> {
    validate_canonical_id(name, value).map_err(|error| Status::invalid_argument(error.to_string()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PaperCommandRouteDocument {
    schema_version: u32,
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
    /// Required tick size per tradable instrument (exact decimal strings).
    instrument_tick_sizes: std::collections::BTreeMap<String, String>,
    /// Required lot size per tradable instrument (exact decimal strings).
    instrument_lot_sizes: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    short_exposure: Option<PaperCommandShortExposureDocument>,
    kill_switch_version: String,
    adapter_kind: String,
    journal_path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PaperCommandShortExposureDocument {
    max_short_quantity: String,
}

struct RuntimeConfig {
    bind: SocketAddr,
    production: bool,
    database_url: Option<String>,
    database_ca: Option<PathBuf>,
    paper_command_route: Option<PathBuf>,
    /// The version-1 controlled-LIVE configuration `follon-live-status` reads.
    live_configuration: Option<PathBuf>,
    /// The controlled-LIVE journal that configuration's service writes.
    live_journal: Option<PathBuf>,
    operator_directory: Option<PathBuf>,
    tls_certificate: Option<PathBuf>,
    tls_private_key: Option<PathBuf>,
    tls_client_ca: Option<PathBuf>,
}

impl RuntimeConfig {
    fn from_environment() -> Result<Self, String> {
        let production =
            env::var("FOLLON_DEPLOYMENT_MODE").is_ok_and(|value| value == "production");
        let bind = env::var("FOLLON_GRPC_BIND")
            .unwrap_or_else(|_| "127.0.0.1:50051".to_owned())
            .parse()
            .map_err(|error| format!("invalid FOLLON_GRPC_BIND: {error}"))?;
        let config = Self {
            bind,
            production,
            database_url: database_url(production)?,
            database_ca: env_path("FOLLON_DATABASE_CA"),
            paper_command_route: env_path("FOLLON_TRADING_API_PAPER_CONFIG"),
            live_configuration: env_path("FOLLON_TRADING_API_LIVE_CONFIG"),
            live_journal: env_path("FOLLON_TRADING_API_LIVE_JOURNAL"),
            operator_directory: env_path("FOLLON_TRADING_API_OPERATOR_DIRECTORY"),
            tls_certificate: env_path("FOLLON_GRPC_TLS_CERTIFICATE"),
            tls_private_key: env_path("FOLLON_GRPC_TLS_PRIVATE_KEY"),
            tls_client_ca: env_path("FOLLON_GRPC_TLS_CLIENT_CA"),
        };
        if production
            && (config.database_url.is_none()
                || config.tls_certificate.is_none()
                || config.tls_private_key.is_none()
                || config.tls_client_ca.is_none())
        {
            return Err(
                "production requires PostgreSQL plus server TLS and client CA files".to_owned(),
            );
        }
        if production
            && !config
                .database_url
                .as_deref()
                .is_some_and(|value| value.contains("sslmode=require"))
        {
            return Err("production PostgreSQL URL must require TLS".to_owned());
        }
        config.validate_paper_command_route_transport()?;
        config.validate_live_kill_switch_route()?;
        config.validate_operator_authentication()?;
        Ok(config)
    }

    /// A controlled-LIVE kill-switch route needs its configuration and its
    /// journal together, accepts changes only from authenticated operators,
    /// and off loopback requires mutual TLS, exactly as a PAPER route does.
    fn validate_live_kill_switch_route(&self) -> Result<(), String> {
        match (&self.live_configuration, &self.live_journal) {
            (None, None) => return Ok(()),
            (Some(_), Some(_)) => {}
            _ => {
                return Err(
                    "a controlled-LIVE route requires both FOLLON_TRADING_API_LIVE_CONFIG and FOLLON_TRADING_API_LIVE_JOURNAL"
                        .to_owned(),
                )
            }
        }
        if self.operator_directory.is_none() {
            return Err(
                "a controlled-LIVE route requires FOLLON_TRADING_API_OPERATOR_DIRECTORY".to_owned(),
            );
        }
        if !self.bind.ip().is_loopback()
            && (tls_identity_paths(self).is_none() || self.tls_client_ca.is_none())
        {
            return Err(
                "a remote controlled-LIVE route requires server TLS and a client CA".to_owned(),
            );
        }
        Ok(())
    }

    /// A PAPER route accepts writes only from authenticated operators, and
    /// operator passwords never cross a plaintext non-loopback transport.
    fn validate_operator_authentication(&self) -> Result<(), String> {
        if self.paper_command_route.is_some() && self.operator_directory.is_none() {
            return Err(
                "a PAPER command route requires FOLLON_TRADING_API_OPERATOR_DIRECTORY".to_owned(),
            );
        }
        if self.operator_directory.is_some()
            && !self.bind.ip().is_loopback()
            && tls_identity_paths(self).is_none()
        {
            return Err("operator login off loopback requires server TLS".to_owned());
        }
        Ok(())
    }

    fn validate_paper_command_route_transport(&self) -> Result<(), String> {
        if self.paper_command_route.is_some()
            && !self.bind.ip().is_loopback()
            && (tls_identity_paths(self).is_none() || self.tls_client_ca.is_none())
        {
            return Err(
                "a remote PAPER command route requires server TLS and a client CA".to_owned(),
            );
        }
        Ok(())
    }
}

fn paper_combo_route_from_path(path: &Path) -> Result<PaperComboRoute, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read PAPER command-route config: {error}"))?;
    let document: PaperCommandRouteDocument = serde_json::from_str(&contents)
        .map_err(|error| format!("invalid PAPER command-route config: {error}"))?;
    if document.schema_version != 1 {
        return Err("unsupported PAPER command-route schema version".to_owned());
    }
    if document.adapter_kind != "IBKR_PAPER_MODEL" {
        return Err("unsupported PAPER command-route adapter kind".to_owned());
    }
    let account = PaperAccount {
        account_id: document.account_id,
        currency: document.currency,
        initial_cash: route_decimal("initial_cash", &document.initial_cash)?,
        environment: "PAPER".to_owned(),
    };
    let policy = PaperRiskPolicy {
        version: document.risk_policy_version,
        trading_calendar_id: document.trading_calendar_id,
        max_order_quantity: route_decimal("max_order_quantity", &document.max_order_quantity)?,
        max_order_notional: route_decimal("max_order_notional", &document.max_order_notional)?,
        max_price_deviation_bps: route_decimal(
            "max_price_deviation_bps",
            &document.max_price_deviation_bps,
        )?,
        max_open_orders: document.max_open_orders,
        max_position_quantity: route_decimal(
            "max_position_quantity",
            &document.max_position_quantity,
        )?,
        max_realized_loss: route_decimal("max_realized_loss", &document.max_realized_loss)?,
        max_market_data_age_seconds: document.max_market_data_age_seconds,
        max_order_rate: document.max_order_rate,
        order_rate_window_seconds: document.order_rate_window_seconds,
        portfolio_risk: None,
        short_exposure: document
            .short_exposure
            .map(|permission| -> Result<ShortExposurePolicy, String> {
                Ok(ShortExposurePolicy {
                    max_short_quantity: route_decimal(
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
                Ok((
                    instrument_id.clone(),
                    route_decimal("instrument_tick_sizes", tick)?,
                ))
            })
            .collect::<Result<_, String>>()?,
        instrument_lot_sizes: document
            .instrument_lot_sizes
            .iter()
            .map(|(instrument_id, lot)| {
                Ok((
                    instrument_id.clone(),
                    route_decimal("instrument_lot_sizes", lot)?,
                ))
            })
            .collect::<Result<_, String>>()?,
    };
    let broker = IbkrPaperAdapter::new(&account)
        .map_err(|error| format!("PAPER command-route adapter: {error}"))?;
    let service = PaperTradingService::open_durable(
        account,
        policy,
        KillSwitchRegistry::new(document.kill_switch_version)
            .map_err(|error| format!("PAPER command-route kill switches: {error}"))?,
        broker,
        &document.journal_path,
    )
    .map_err(|error| format!("PAPER command-route service: {error}"))?;
    Ok(Arc::new(Mutex::new(service)))
}

/// Opens the controlled-LIVE journal under the version-1 configuration
/// `follon-live-status` reads, through the parser both share, so the journal
/// opens only under the fingerprint it was written with. `opened_at` is the
/// server's UTC time, which the LIVE journal records for the restart.
fn live_kill_switch_route_from_paths(
    configuration_path: &Path,
    journal_path: &Path,
    opened_at: &str,
) -> Result<LiveKillSwitchRoute, String> {
    let bytes = std::fs::read(configuration_path)
        .map_err(|error| format!("cannot read controlled-LIVE configuration: {error}"))?;
    let configuration = LiveConfiguration::from_json(&bytes)
        .map_err(|error| format!("controlled-LIVE configuration: {error}"))?;
    let service = LiveTradingService::open_durable(
        configuration.account,
        configuration.risk,
        configuration.activation,
        configuration.kill_switches,
        KillSwitchOnlyLiveAdapter,
        journal_path,
        opened_at,
    )
    .map_err(|error| format!("controlled-LIVE route service: {error}"))?;
    Ok(Arc::new(Mutex::new(service)))
}

/// Loads the operator directory from a regular, bounded file. It holds TOTP
/// secrets, so it is read like the PostgreSQL URL file: never through a
/// symbolic link and never beyond a fixed size.
fn operator_identity_from_path(path: &Path) -> Result<OperatorIdentity, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect operator directory: {error}"))?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > 1_048_576
    {
        return Err("operator directory file is unsafe or oversized".to_owned());
    }
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read operator directory: {error}"))?;
    let service = OperatorDirectory::parse(&contents)
        .and_then(|directory| directory.identity_service())
        .map_err(|error| format!("invalid operator directory: {error}"))?;
    Ok(Arc::new(Mutex::new(service)))
}

fn route_decimal(name: &str, value: &str) -> Result<Decimal, String> {
    Decimal::from_str(value).map_err(|error| format!("invalid {name}: {error}"))
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name).map(PathBuf::from)
}

/// Returns the certificate and private key paths only when both are present.
/// The server installs TLS if and only if this returns `Some`, and the
/// `transport_tls` health flag is derived from this same check so the two can
/// never drift apart (an operator-supplied certificate without a matching
/// private key must never be reported as an active TLS transport).
fn tls_identity_paths(config: &RuntimeConfig) -> Option<(&Path, &Path)> {
    match (&config.tls_certificate, &config.tls_private_key) {
        (Some(certificate_path), Some(private_key_path)) => {
            Some((certificate_path, private_key_path))
        }
        _ => None,
    }
}

fn database_url(production: bool) -> Result<Option<String>, String> {
    let direct = env::var("FOLLON_DATABASE_URL").ok();
    let file = env_path("FOLLON_DATABASE_URL_FILE");
    if direct.is_some() && file.is_some() {
        return Err("set only one PostgreSQL URL source".to_owned());
    }
    if production && direct.is_some() {
        return Err("production PostgreSQL URL must come from FOLLON_DATABASE_URL_FILE".to_owned());
    }
    if let Some(path) = file {
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect PostgreSQL URL file: {error}"))?;
        if !metadata.file_type().is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > 16_384
        {
            return Err("PostgreSQL URL file is unsafe or oversized".to_owned());
        }
        let value = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read PostgreSQL URL file: {error}"))?;
        let value = value.trim_end_matches(['\r', '\n']).to_owned();
        if value.is_empty() || value.contains(['\r', '\n', '\0']) {
            return Err("PostgreSQL URL file contains invalid data".to_owned());
        }
        Ok(Some(value))
    } else {
        Ok(direct)
    }
}

fn read_file(path: &Path, label: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("cannot read {label}: {error}"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::args().any(|argument| argument == "--healthcheck") {
        let target =
            env::var("FOLLON_GRPC_HEALTH_TARGET").unwrap_or_else(|_| "127.0.0.1:50051".to_owned());
        let address: SocketAddr = target.parse().map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid health target: {error}"),
            )
        })?;
        std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_secs(2))?;
        return Ok(());
    }
    let config = RuntimeConfig::from_environment().map_err(std::io::Error::other)?;
    let database = if let Some(database_url) = config.database_url.clone() {
        // `postgres::Client::connect` (the synchronous client `PostgresStore` wraps) drives
        // its own private Tokio runtime internally via `block_on`, so calling it directly on
        // this `#[tokio::main]` async task panics with "Cannot start a runtime from within a
        // runtime." `spawn_blocking` moves it onto a dedicated blocking-pool thread that has
        // no ambient runtime context, which is exactly where that pattern is meant to run.
        let production = config.production;
        let database_ca = config.database_ca.clone();
        let store =
            tokio::task::spawn_blocking(move || -> Result<PostgresStore, PersistenceError> {
                let mut store = if production {
                    PostgresStore::connect_tls(&database_url, database_ca.as_deref())?
                } else {
                    PostgresStore::connect_development(&database_url)?
                };
                store.migrate()?;
                Ok(store)
            })
            .await
            .map_err(|error| {
                std::io::Error::other(format!("database bootstrap task panicked: {error}"))
            })??;
        Some(Arc::new(Mutex::new(store)))
    } else {
        None
    };
    let paper_combo_route = config
        .paper_command_route
        .as_deref()
        .map(paper_combo_route_from_path)
        .transpose()
        .map_err(std::io::Error::other)?;
    let live_kill_switch_route = match (&config.live_configuration, &config.live_journal) {
        (Some(configuration), Some(journal)) => Some(
            live_kill_switch_route_from_paths(
                configuration,
                journal,
                &utc_timestamp(now_epoch_seconds().map_err(std::io::Error::other)?)
                    .map_err(std::io::Error::other)?,
            )
            .map_err(std::io::Error::other)?,
        ),
        _ => None,
    };
    let identity = config
        .operator_directory
        .as_deref()
        .map(operator_identity_from_path)
        .transpose()
        .map_err(std::io::Error::other)?;
    let transport_tls = tls_identity_paths(&config).is_some();
    let service = OperatingSystemService {
        database,
        paper_combo_route: paper_combo_route.clone(),
        live_kill_switch_route: live_kill_switch_route.clone(),
        identity: identity.clone(),
        transport_tls,
    };
    let mut server = Server::builder();
    if let Some((certificate_path, private_key_path)) = tls_identity_paths(&config) {
        let identity = Identity::from_pem(
            read_file(certificate_path, "gRPC TLS certificate")?,
            read_file(private_key_path, "gRPC TLS private key")?,
        );
        let mut tls = ServerTlsConfig::new().identity(identity);
        if let Some(client_ca_path) = &config.tls_client_ca {
            tls = tls.client_ca_root(Certificate::from_pem(read_file(
                client_ca_path,
                "gRPC client CA",
            )?));
        }
        server = server.tls_config(tls)?;
    }
    eprintln!(
        "follon-trading-api listening on {} (tls={}, production={}, paper_combo_route={}, live_kill_switch_route={}, operator_identity={})",
        config.bind,
        transport_tls,
        config.production,
        paper_combo_route.is_some(),
        live_kill_switch_route.is_some(),
        identity.is_some()
    );
    server
        .add_service(TradingOperatingSystemServer::new(service))
        .serve_with_shutdown(config.bind, async {
            let _ = signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use follon_identity::{totp_code, Role};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::OnceLock;

    static PAPER_ROUTE_SEQUENCE: AtomicUsize = AtomicUsize::new(1);
    const OPERATOR_PASSWORD: &str = "Correct-Horse-9-Battery";

    struct TestDirectory {
        json: String,
        trader_secret: Vec<u8>,
        viewer_secret: Vec<u8>,
        risk_secret: Vec<u8>,
    }

    /// A trader, a read-only operator and a risk manager, hashed once per
    /// test binary because Argon2id is deliberately slow.
    fn test_directory() -> &'static TestDirectory {
        static DIRECTORY: OnceLock<TestDirectory> = OnceLock::new();
        DIRECTORY.get_or_init(|| {
            let mut directory = OperatorDirectory::new("tenant.alpha").unwrap();
            let trader_secret = directory
                .add_operator(
                    "user.trader",
                    "trader@example.com",
                    OPERATOR_PASSWORD,
                    &[Role::Trader],
                )
                .unwrap();
            let viewer_secret = directory
                .add_operator(
                    "user.viewer",
                    "viewer@example.com",
                    OPERATOR_PASSWORD,
                    &[Role::ReadOnly],
                )
                .unwrap();
            let risk_secret = directory
                .add_operator(
                    "user.risk",
                    "risk@example.com",
                    OPERATOR_PASSWORD,
                    &[Role::RiskManager],
                )
                .unwrap();
            TestDirectory {
                json: directory.to_json().unwrap(),
                trader_secret,
                viewer_secret,
                risk_secret,
            }
        })
    }

    fn operator_identity() -> OperatorIdentity {
        Arc::new(Mutex::new(
            OperatorDirectory::parse(&test_directory().json)
                .unwrap()
                .identity_service()
                .unwrap(),
        ))
    }

    fn service() -> OperatingSystemService {
        OperatingSystemService {
            database: None,
            paper_combo_route: None,
            live_kill_switch_route: None,
            identity: None,
            transport_tls: false,
        }
    }

    fn authorized<T>(message: T, token: &str) -> Request<T> {
        let mut request = Request::new(message);
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().expect("header value"),
        );
        request
    }

    async fn login(service: &OperatingSystemService, email: &str, secret: &[u8]) -> String {
        let challenge = service
            .begin_operator_login(Request::new(BeginOperatorLoginRequest {
                tenant_id: "tenant.alpha".to_owned(),
                email: email.to_owned(),
                password: OPERATOR_PASSWORD.to_owned(),
            }))
            .await
            .expect("password step")
            .into_inner();
        service
            .complete_operator_login(Request::new(CompleteOperatorLoginRequest {
                challenge_token: challenge.challenge_token,
                totp_code: totp_code(secret, now_epoch_seconds().unwrap()).unwrap(),
            }))
            .await
            .expect("second factor")
            .into_inner()
            .session_token
    }

    async fn trader_token(service: &OperatingSystemService) -> String {
        login(
            service,
            "trader@example.com",
            &test_directory().trader_secret,
        )
        .await
    }

    async fn risk_token(service: &OperatingSystemService) -> String {
        login(service, "risk@example.com", &test_directory().risk_secret).await
    }

    fn kill_switch_request(scope: &str) -> PaperKillSwitchRequest {
        PaperKillSwitchRequest {
            tenant_id: "tenant.alpha".to_owned(),
            scope: scope.to_owned(),
        }
    }

    fn configured_paper_service(name: &str) -> (OperatingSystemService, PaperComboRoute, PathBuf) {
        let (config, scratch) = write_route_config(name, |_| {});
        let route = paper_combo_route_from_path(&config).expect("configured PAPER combo route");
        (
            OperatingSystemService {
                database: None,
                paper_combo_route: Some(route.clone()),
                live_kill_switch_route: None,
                identity: Some(operator_identity()),
                transport_tls: false,
            },
            route,
            scratch,
        )
    }

    /// Writes the test route's configuration with `adjust` applied to it, and
    /// returns the configuration's path and its scratch directory.
    fn write_route_config(
        name: &str,
        adjust: impl FnOnce(&mut serde_json::Value),
    ) -> (PathBuf, PathBuf) {
        let sequence = PAPER_ROUTE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let scratch = std::env::temp_dir().join(format!(
            "follon-trading-api-paper-combo-{}-{sequence}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("create route scratch");
        let journal = scratch.join("journal.ndjson");
        let config = scratch.join("route.json");
        let mut document = serde_json::json!({
            "schema_version": 1,
            "account_id": "acct.grpc.paper.test",
            "currency": "USD",
            "initial_cash": "100000",
            "risk_policy_version": "risk.grpc.paper.v1",
            "trading_calendar_id": "cal.us.options.test",
            "max_order_quantity": "100",
            "max_order_notional": "50000",
            "max_price_deviation_bps": "100",
            "max_open_orders": 10,
            "max_position_quantity": "1000",
            "max_realized_loss": "10000",
            "max_market_data_age_seconds": 5,
            "max_order_rate": 20,
            "order_rate_window_seconds": 60,
            "instrument_tick_sizes": {
                "inst.us_equity.spy": "0.01",
                "inst.us_option.spy.500c": "0.01",
                "inst.us_option.spy.505c": "0.01",
            },
            // Two-contract lots make the lot rule observable at this
            // boundary: the default two-unit request is whole lots on both
            // legs, and a one-unit request is not.
            "instrument_lot_sizes": {
                "inst.us_equity.spy": "1",
                "inst.us_option.spy.500c": "2",
                "inst.us_option.spy.505c": "2",
            },
            "short_exposure": { "max_short_quantity": "1000" },
            "kill_switch_version": "kills.grpc.paper.v1",
            "adapter_kind": "IBKR_PAPER_MODEL",
            "journal_path": journal.to_string_lossy(),
        });
        adjust(&mut document);
        std::fs::write(
            &config,
            serde_json::to_vec_pretty(&document).expect("serialize route config"),
        )
        .expect("write route config");
        (config, scratch)
    }

    #[test]
    fn a_route_listing_an_instrument_in_only_one_table_refuses_to_start() {
        let (config, scratch) = write_route_config("unpaired-tables", |document| {
            document["instrument_lot_sizes"]
                .as_object_mut()
                .expect("lot table")
                .remove("inst.us_option.spy.505c");
        });
        assert_eq!(
            paper_combo_route_from_path(&config)
                .err()
                .expect("an unpaired table must refuse the route"),
            "PAPER command-route service: paper risk policy lists inst.us_option.spy.505c in only one of its tick and lot tables"
        );
        // Refused before its journal is touched (E3.10).
        assert!(!scratch.join("journal.ndjson").exists());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    fn paper_combo_request(intent_id: &str) -> SubmitPaperComboRequest {
        SubmitPaperComboRequest {
            tenant_id: "tenant.alpha".to_owned(),
            intent_id: intent_id.to_owned(),
            account_id: "acct.grpc.paper.test".to_owned(),
            strategy_id: "strategy.grpc.test".to_owned(),
            correlation_id: format!("corr-{intent_id}"),
            combo_quantity: "2".to_owned(),
            price_limit_kind: ComboPriceLimitKind::MaximumDebit as i32,
            price_limit: "2".to_owned(),
            legs: vec![
                api::OptionComboLeg {
                    instrument_id: "inst.us_option.spy.500c".to_owned(),
                    side: ExecutionSide::Buy as i32,
                    ratio: 1,
                    limit_price: "3".to_owned(),
                },
                api::OptionComboLeg {
                    instrument_id: "inst.us_option.spy.505c".to_owned(),
                    side: ExecutionSide::Sell as i32,
                    ratio: 1,
                    limit_price: "1".to_owned(),
                },
            ],
            time_in_force: OrderTimeInForceKind::Day as i32,
            rationale: "gRPC risk-gated combination test".to_owned(),
            created_at: "2026-01-02T14:30:00Z".to_owned(),
            strategy_version: "strategy.grpc.v1".to_owned(),
            configuration_version: "config.grpc.v1".to_owned(),
            environment: "PAPER".to_owned(),
            decided_at: "2026-01-02T14:30:02Z".to_owned(),
            observations: vec![
                PaperComboMarketObservation {
                    instrument_id: "inst.us_option.spy.500c".to_owned(),
                    mark_price: "3".to_owned(),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                },
                PaperComboMarketObservation {
                    instrument_id: "inst.us_option.spy.505c".to_owned(),
                    mark_price: "1".to_owned(),
                    observed_at: "2026-01-02T14:30:00Z".to_owned(),
                },
            ],
        }
    }

    #[test]
    fn execution_side_rejects_unspecified() {
        assert!(side(ExecutionSide::Unspecified as i32).is_err());
        assert_eq!(side(ExecutionSide::Buy as i32).unwrap(), Side::Buy);
    }

    fn base_config() -> RuntimeConfig {
        RuntimeConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            production: false,
            database_url: None,
            database_ca: None,
            paper_command_route: None,
            live_configuration: None,
            live_journal: None,
            operator_directory: None,
            tls_certificate: None,
            tls_private_key: None,
            tls_client_ca: None,
        }
    }

    #[test]
    fn a_live_route_needs_both_paths_an_operator_directory_and_remote_mutual_tls() {
        let mut config = base_config();
        assert!(config.validate_live_kill_switch_route().is_ok());
        config.live_configuration = Some(PathBuf::from("live.json"));
        assert_eq!(
            config.validate_live_kill_switch_route().unwrap_err(),
            "a controlled-LIVE route requires both FOLLON_TRADING_API_LIVE_CONFIG and FOLLON_TRADING_API_LIVE_JOURNAL"
        );
        let mut journal_only = base_config();
        journal_only.live_journal = Some(PathBuf::from("live.ndjson"));
        assert!(journal_only.validate_live_kill_switch_route().is_err());

        config.live_journal = Some(PathBuf::from("live.ndjson"));
        assert_eq!(
            config.validate_live_kill_switch_route().unwrap_err(),
            "a controlled-LIVE route requires FOLLON_TRADING_API_OPERATOR_DIRECTORY"
        );
        config.operator_directory = Some(PathBuf::from("operators.json"));
        assert!(config.validate_live_kill_switch_route().is_ok());

        config.bind = "0.0.0.0:50051".parse().expect("remote bind");
        assert!(config.validate_live_kill_switch_route().is_err());
        config.tls_certificate = Some(PathBuf::from("server.pem"));
        config.tls_private_key = Some(PathBuf::from("server-key.pem"));
        assert_eq!(
            config.validate_live_kill_switch_route().unwrap_err(),
            "a remote controlled-LIVE route requires server TLS and a client CA"
        );
        config.tls_client_ca = Some(PathBuf::from("clients.pem"));
        assert!(config.validate_live_kill_switch_route().is_ok());
    }

    #[test]
    fn transport_tls_requires_both_certificate_and_private_key() {
        let mut config = base_config();
        config.tls_certificate = Some(PathBuf::from("cert.pem"));
        assert!(tls_identity_paths(&config).is_none());

        config.tls_private_key = Some(PathBuf::from("key.pem"));
        assert!(tls_identity_paths(&config).is_some());
    }

    #[test]
    fn paper_command_route_requires_loopback_or_mutual_tls() {
        let mut config = base_config();
        config.paper_command_route = Some(PathBuf::from("paper-route.json"));
        assert!(config.validate_paper_command_route_transport().is_ok());

        config.bind = "0.0.0.0:50051".parse().expect("remote bind");
        assert!(config.validate_paper_command_route_transport().is_err());

        config.tls_certificate = Some(PathBuf::from("server.pem"));
        config.tls_private_key = Some(PathBuf::from("server-key.pem"));
        assert!(config.validate_paper_command_route_transport().is_err());

        config.tls_client_ca = Some(PathBuf::from("clients.pem"));
        assert!(config.validate_paper_command_route_transport().is_ok());
    }

    #[tokio::test]
    async fn health_check_never_claims_tls_when_only_certificate_is_configured() {
        let config = {
            let mut config = base_config();
            config.tls_certificate = Some(PathBuf::from("cert.pem"));
            config
        };
        let transport_tls = tls_identity_paths(&config).is_some();
        let service = OperatingSystemService {
            database: None,
            paper_combo_route: None,
            live_kill_switch_route: None,
            identity: None,
            transport_tls,
        };
        let response = service
            .check_health(Request::new(HealthRequest {}))
            .await
            .unwrap()
            .into_inner();
        assert!(!response.transport_tls);
    }

    #[test]
    fn bucket_limits_reject_duplicates() {
        let limits = vec![
            BucketLimit {
                key: "technology".to_owned(),
                limit: "100.0".to_owned(),
            },
            BucketLimit {
                key: "technology".to_owned(),
                limit: "200.0".to_owned(),
            },
        ];
        assert!(bucket_limits("sector", &limits).is_err());
    }

    #[tokio::test]
    async fn passive_repricing_rpc_preserves_cancel_before_replace_contract() {
        let response = service()
            .plan_passive_repricing(Request::new(PassiveRepricingRequest {
                tenant_id: "tenant.alpha".to_owned(),
                parent_order_id: "order.passive.1".to_owned(),
                account_id: "account.primary".to_owned(),
                strategy_id: "strategy.alpha".to_owned(),
                instrument_id: "inst.us_equity.spy".to_owned(),
                side: ExecutionSide::Buy as i32,
                quantity: "2".to_owned(),
                hard_limit_price: Some("101".to_owned()),
                initial_limit_price: "99".to_owned(),
                tick_size: "0.5".to_owned(),
                maximum_chase_bps: 1_000,
                maximum_replacements: 2,
                minimum_replace_interval_seconds: 5,
                observations: vec![
                    api::PassiveMarketObservation {
                        observed_after_seconds: 5,
                        best_bid: "99".to_owned(),
                        best_ask: "100".to_owned(),
                    },
                    api::PassiveMarketObservation {
                        observed_after_seconds: 10,
                        best_bid: "99.5".to_owned(),
                        best_ask: "100".to_owned(),
                    },
                ],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(
            response.initial.unwrap().limit_price.as_deref(),
            Some("99.00000000")
        );
        assert_eq!(response.replacements.len(), 1);
        assert_eq!(
            response.replacements[0].cancel_child_id,
            "order.passive.1.passive.0000"
        );
        assert_eq!(
            response.replacements[0]
                .replacement
                .as_ref()
                .unwrap()
                .limit_price
                .as_deref(),
            Some("99.50000000")
        );
    }

    #[tokio::test]
    async fn option_combo_rpc_returns_one_atomic_net_protected_group() {
        let response = service()
            .plan_option_combo(Request::new(OptionComboRequest {
                tenant_id: "tenant.alpha".to_owned(),
                combo_id: "combo.vertical.1".to_owned(),
                account_id: "account.primary".to_owned(),
                strategy_id: "strategy.alpha".to_owned(),
                combo_quantity: "2".to_owned(),
                price_limit_kind: ComboPriceLimitKind::MaximumDebit as i32,
                price_limit: "2.5".to_owned(),
                legs: vec![
                    api::OptionComboLeg {
                        instrument_id: "option.spy.500c".to_owned(),
                        side: ExecutionSide::Buy as i32,
                        ratio: 1,
                        limit_price: "3".to_owned(),
                    },
                    api::OptionComboLeg {
                        instrument_id: "option.spy.505c".to_owned(),
                        side: ExecutionSide::Sell as i32,
                        ratio: 1,
                        limit_price: "1".to_owned(),
                    },
                ],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(response.combo_id, "combo.vertical.1");
        assert_eq!(response.combo_quantity, "2.00000000");
        assert_eq!(response.protected_net_price, "2.00000000");
        assert_eq!(response.legs.len(), 2);
        assert_eq!(response.legs[0].quantity, "2.00000000");
        assert_eq!(response.legs[1].side, ExecutionSide::Sell as i32);
    }

    #[tokio::test]
    async fn paper_combo_rpc_persists_one_risk_gated_atomic_order() {
        let (service, route, scratch) = configured_paper_service("submit");
        let token = trader_token(&service).await;
        let request = paper_combo_request("intent.grpc.paper.combo.1");
        let response = service
            .submit_paper_combo(authorized(request.clone(), &token))
            .await
            .expect("risk-gated submission")
            .into_inner();

        assert!(response.approved);
        assert_eq!(response.submitted_by, "user.trader");
        assert_eq!(response.reason_codes, ["APPROVED"]);
        assert_eq!(response.policy_version, "risk.grpc.paper.v1");
        assert_eq!(response.state, OmsOrderState::Acknowledged as i32);
        let order_id = response.order_id.expect("approved order identity");
        {
            let paper = route.lock().expect("PAPER route");
            let order = paper.combo_order(&order_id).expect("durable combo order");
            assert_eq!(order.oms.intent.intent_id, request.intent_id);
            assert_eq!(order.oms.intent.legs.len(), 2);
            assert_eq!(order.oms.state, OrderState::Acknowledged);
            let evidence = paper
                .combo_risk_evidence(&response.decision_id)
                .expect("journaled risk evidence");
            assert_eq!(evidence.submitted_by.as_deref(), Some("user.trader"));
        }

        let repeated = service
            .submit_paper_combo(authorized(request, &token))
            .await
            .expect("idempotent submission")
            .into_inner();
        assert_eq!(repeated.order_id.as_deref(), Some(order_id.as_str()));
        assert_eq!(repeated.decision_id, response.decision_id);
        drop(service);
        drop(route);
        let reopened = paper_combo_route_from_path(&scratch.join("route.json"))
            .expect("reopen durable PAPER combo route");
        let paper = reopened.lock().expect("reopened PAPER route");
        let order = paper
            .combo_order(&order_id)
            .expect("combination survives route restart");
        assert_eq!(order.oms.state, OrderState::Acknowledged);
        drop(paper);
        drop(reopened);
        let _ = std::fs::remove_dir_all(scratch);
    }

    #[tokio::test]
    async fn paper_combo_rpc_returns_risk_rejection_without_an_order() {
        let (service, _route, scratch) = configured_paper_service("risk-rejection");
        let token = trader_token(&service).await;
        let mut request = paper_combo_request("intent.grpc.paper.combo.rejected");
        request.combo_quantity = "101".to_owned();
        let response = service
            .submit_paper_combo(authorized(request, &token))
            .await
            .expect("risk decision")
            .into_inner();

        assert!(!response.approved);
        assert!(response
            .reason_codes
            .contains(&"MAX_ORDER_QUANTITY_EXCEEDED".to_owned()));
        assert!(response.order_id.is_none());
        assert_eq!(response.state, OmsOrderState::Unspecified as i32);
        drop(service);
        let _ = std::fs::remove_dir_all(scratch);
    }

    #[tokio::test]
    async fn paper_combo_rpc_refuses_a_leg_quantity_off_the_routes_lot_table() {
        let (service, _route, scratch) = configured_paper_service("off-lot");
        let token = trader_token(&service).await;
        // One unit sends one contract per leg against the route's
        // two-contract lots; every other limit passes.
        let mut request = paper_combo_request("intent.grpc.paper.combo.off.lot");
        request.combo_quantity = "1".to_owned();
        let response = service
            .submit_paper_combo(authorized(request, &token))
            .await
            .expect("risk decision")
            .into_inner();

        assert!(!response.approved);
        assert_eq!(
            response.reason_codes,
            vec!["ORDER_QUANTITY_OFF_LOT_SIZE".to_owned()]
        );
        assert!(response.order_id.is_none());
        drop(service);
        let _ = std::fs::remove_dir_all(scratch);
    }

    #[tokio::test]
    async fn paper_combo_rpc_refuses_incomplete_per_leg_market_evidence() {
        let (service, _route, scratch) = configured_paper_service("missing-mark");
        let token = trader_token(&service).await;
        let mut request = paper_combo_request("intent.grpc.paper.combo.missing.mark");
        request.observations.pop();
        let error = service
            .submit_paper_combo(authorized(request, &token))
            .await
            .expect_err("every leg requires independent market evidence");

        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert!(error.message().contains("one mark per leg"));
        drop(service);
        let _ = std::fs::remove_dir_all(scratch);
    }

    #[tokio::test]
    async fn paper_combo_rpc_fails_closed_without_a_configured_route() {
        let mut service = service();
        service.identity = Some(operator_identity());
        let token = trader_token(&service).await;
        let error = service
            .submit_paper_combo(authorized(
                paper_combo_request("intent.grpc.paper.combo.unconfigured"),
                &token,
            ))
            .await
            .expect_err("unconfigured route must not act like planning success");

        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert!(error.message().contains("not configured"));
    }

    #[tokio::test]
    async fn paper_combo_rpc_refuses_every_unauthenticated_or_unauthorized_caller() {
        let (service, route, scratch) = configured_paper_service("unauthorized");
        let intent_id = "intent.grpc.paper.combo.unauthorized";
        let request = || paper_combo_request(intent_id);
        let code = |result: Result<Response<SubmitPaperComboResponse>, Status>| {
            result.expect_err("the write must be refused").code()
        };

        // No session, a non-bearer scheme, and a malformed token never reach
        // the identity service.
        assert_eq!(
            code(service.submit_paper_combo(Request::new(request())).await),
            tonic::Code::Unauthenticated
        );
        for header in ["Basic dXNlcjpwYXNz", "Bearer not-a-token", "bearer 00"] {
            let mut unauthenticated = Request::new(request());
            unauthenticated
                .metadata_mut()
                .insert("authorization", header.parse().unwrap());
            assert_eq!(
                code(service.submit_paper_combo(unauthenticated).await),
                tonic::Code::Unauthenticated,
                "{header}"
            );
        }
        // A well-formed token nobody issued.
        assert_eq!(
            code(
                service
                    .submit_paper_combo(authorized(request(), &"ab".repeat(32)))
                    .await
            ),
            tonic::Code::PermissionDenied
        );
        // A real session whose role cannot trade.
        let viewer = login(
            &service,
            "viewer@example.com",
            &test_directory().viewer_secret,
        )
        .await;
        assert_eq!(
            code(
                service
                    .submit_paper_combo(authorized(request(), &viewer))
                    .await
            ),
            tonic::Code::PermissionDenied
        );
        // A trader's session presented for another tenant.
        let trader = trader_token(&service).await;
        let mut other_tenant = request();
        other_tenant.tenant_id = "tenant.beta".to_owned();
        assert_eq!(
            code(
                service
                    .submit_paper_combo(authorized(other_tenant, &trader))
                    .await
            ),
            tonic::Code::PermissionDenied
        );
        // A revoked session.
        let revoked = service
            .revoke_operator_session(authorized(RevokeOperatorSessionRequest {}, &trader))
            .await
            .unwrap()
            .into_inner();
        assert!(revoked.revoked);
        assert_eq!(
            code(
                service
                    .submit_paper_combo(authorized(request(), &trader))
                    .await
            ),
            tonic::Code::PermissionDenied
        );
        // None of these reached the PAPER route.
        let paper = route.lock().unwrap();
        assert!(paper
            .combo_order(&format!("combo-order-{intent_id}"))
            .is_none());
        assert!(paper
            .combo_risk_evidence(&format!("paper-combo-risk-{intent_id}"))
            .is_none());
        drop(paper);
        drop(service);
        drop(route);
        let _ = std::fs::remove_dir_all(scratch);
    }

    #[tokio::test]
    async fn paper_kill_switch_rpcs_require_kill_switch_operation_and_journal_the_operator() {
        let (service, route, scratch) = configured_paper_service("kill-switch");
        let leg = "instrument:inst.us_option.spy.500c";
        let code = |result: Result<Response<PaperKillSwitchResponse>, Status>| {
            result.expect_err("the change must be refused").code()
        };

        // No session, then a trader: trading does not grant kill-switch operation.
        assert_eq!(
            code(
                service
                    .activate_paper_kill_switch(Request::new(kill_switch_request(leg)))
                    .await
            ),
            tonic::Code::Unauthenticated
        );
        let trader = trader_token(&service).await;
        for activate in [true, false] {
            let request = authorized(kill_switch_request(leg), &trader);
            let result = if activate {
                service.activate_paper_kill_switch(request).await
            } else {
                service.release_paper_kill_switch(request).await
            };
            assert_eq!(code(result), tonic::Code::PermissionDenied);
        }
        assert!(route
            .lock()
            .unwrap()
            .kill_switches()
            .active_keys()
            .is_empty());

        // The risk manager activates it, and is recorded as the operator.
        let risk = risk_token(&service).await;
        let activated = service
            .activate_paper_kill_switch(authorized(kill_switch_request(leg), &risk))
            .await
            .expect("a risk manager operates kill switches")
            .into_inner();
        assert!(activated.changed);
        assert_eq!(activated.scope, leg);
        assert_eq!(activated.operated_by, "user.risk");
        assert_eq!(activated.active_kill_switches, vec![leg.to_owned()]);
        let repeat = service
            .activate_paper_kill_switch(authorized(kill_switch_request(leg), &risk))
            .await
            .unwrap()
            .into_inner();
        assert!(!repeat.changed, "a repeat changes nothing");

        // While it is active, a trader's combination on that leg is refused.
        let refused = service
            .submit_paper_combo(authorized(
                paper_combo_request("intent.grpc.paper.combo.halted"),
                &trader,
            ))
            .await
            .expect("risk decision")
            .into_inner();
        assert!(!refused.approved);
        assert!(refused
            .reason_codes
            .contains(&"KILL_SWITCH_INSTRUMENT_INST.US_OPTION.SPY.500C".to_owned()));

        // A malformed scope and another tenant are refused before the route.
        assert_eq!(
            code(
                service
                    .activate_paper_kill_switch(authorized(
                        kill_switch_request("instrument:Not Canonical"),
                        &risk
                    ))
                    .await
            ),
            tonic::Code::InvalidArgument
        );
        let mut other_tenant = kill_switch_request(leg);
        other_tenant.tenant_id = "tenant.beta".to_owned();
        assert_eq!(
            code(
                service
                    .release_paper_kill_switch(authorized(other_tenant, &risk))
                    .await
            ),
            tonic::Code::PermissionDenied
        );

        let released = service
            .release_paper_kill_switch(authorized(kill_switch_request(leg), &risk))
            .await
            .unwrap()
            .into_inner();
        assert!(released.changed);
        assert!(released.active_kill_switches.is_empty());
        let paper = route.lock().unwrap();
        let operations = paper.kill_switch_operations();
        assert_eq!(operations.len(), 2, "exactly the two changes are journaled");
        assert!(operations
            .iter()
            .all(|operation| operation.operator == "user.risk" && operation.scope == leg));
        assert_eq!(operations[0].operated_at, activated.operated_at);
        drop(paper);
        drop(service);
        drop(route);
        let _ = std::fs::remove_dir_all(scratch);
    }

    fn live_kill_switch_request(scope: &str) -> LiveKillSwitchRequest {
        LiveKillSwitchRequest {
            tenant_id: "tenant.alpha".to_owned(),
            scope: scope.to_owned(),
        }
    }

    /// Opens the controlled-LIVE route over the checked-in configuration and
    /// a scratch journal, at the server's time, as `main` does.
    fn open_live_route(journal: &Path) -> LiveKillSwitchRoute {
        let configuration =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config/live-v1.json");
        let opened_at = utc_timestamp(now_epoch_seconds().unwrap()).unwrap();
        live_kill_switch_route_from_paths(&configuration, journal, &opened_at)
            .expect("configured controlled-LIVE route")
    }

    fn live_service(route: &LiveKillSwitchRoute) -> OperatingSystemService {
        OperatingSystemService {
            database: None,
            paper_combo_route: None,
            live_kill_switch_route: Some(route.clone()),
            identity: Some(operator_identity()),
            transport_tls: false,
        }
    }

    #[tokio::test]
    async fn live_kill_switch_rpcs_require_kill_switch_operation_and_journal_the_operator() {
        let sequence = PAPER_ROUTE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let scratch = std::env::temp_dir().join(format!(
            "follon-trading-api-live-kill-{}-{sequence}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).expect("create LIVE route scratch");
        let journal = scratch.join("live.ndjson");
        let scope = "instrument:inst.us_equity.spy";
        let code = |result: Result<Response<LiveKillSwitchResponse>, Status>| {
            result.expect_err("the change must be refused").code()
        };

        let route = open_live_route(&journal);
        let service = live_service(&route);
        // No session, then a trader: trading does not grant kill-switch operation.
        assert_eq!(
            code(
                service
                    .activate_live_kill_switch(Request::new(live_kill_switch_request(scope)))
                    .await
            ),
            tonic::Code::Unauthenticated
        );
        let trader = trader_token(&service).await;
        for activate in [true, false] {
            let request = authorized(live_kill_switch_request(scope), &trader);
            let result = if activate {
                service.activate_live_kill_switch(request).await
            } else {
                service.release_live_kill_switch(request).await
            };
            assert_eq!(code(result), tonic::Code::PermissionDenied);
        }
        assert!(route
            .lock()
            .unwrap()
            .kill_switches()
            .active_keys()
            .is_empty());

        // The risk manager activates it, and is recorded as the operator.
        let risk = risk_token(&service).await;
        let activated = service
            .activate_live_kill_switch(authorized(live_kill_switch_request(scope), &risk))
            .await
            .expect("a risk manager operates kill switches")
            .into_inner();
        assert!(activated.changed);
        assert_eq!(activated.scope, scope);
        assert_eq!(activated.operated_by, "user.risk");
        assert_eq!(activated.active_kill_switches, vec![scope.to_owned()]);
        let repeat = service
            .activate_live_kill_switch(authorized(live_kill_switch_request(scope), &risk))
            .await
            .unwrap()
            .into_inner();
        assert!(!repeat.changed, "a repeat changes nothing");

        // A malformed scope and another tenant are refused before the route.
        assert_eq!(
            code(
                service
                    .activate_live_kill_switch(authorized(
                        live_kill_switch_request("instrument:Not Canonical"),
                        &risk
                    ))
                    .await
            ),
            tonic::Code::InvalidArgument
        );
        let mut other_tenant = live_kill_switch_request(scope);
        other_tenant.tenant_id = "tenant.beta".to_owned();
        assert_eq!(
            code(
                service
                    .release_live_kill_switch(authorized(other_tenant, &risk))
                    .await
            ),
            tonic::Code::PermissionDenied
        );

        // The switch is durable: a restarted route still has it active.
        drop(service);
        drop(route);
        let route = open_live_route(&journal);
        assert_eq!(
            route.lock().unwrap().kill_switches().active_keys(),
            vec![scope.to_owned()]
        );
        let service = live_service(&route);
        let risk = risk_token(&service).await;
        let released = service
            .release_live_kill_switch(authorized(live_kill_switch_request(scope), &risk))
            .await
            .unwrap()
            .into_inner();
        assert!(released.changed);
        assert!(released.active_kill_switches.is_empty());
        drop(service);
        drop(route);

        // The closed journal names the risk manager, at the server's time,
        // for every change. A repeated activation is journaled too.
        let entries: Vec<serde_json::Value> = std::fs::read_to_string(&journal)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let changes: Vec<_> = entries
            .iter()
            .filter(|entry| {
                entry["event_type"]
                    .as_str()
                    .is_some_and(|kind| kind.starts_with("live.kill_switch."))
            })
            .map(|entry| {
                (
                    entry["event_type"].as_str().unwrap().to_owned(),
                    entry["actor"].as_str().unwrap().to_owned(),
                    entry["state"]["active_kill_switches"].clone(),
                )
            })
            .collect();
        let active = serde_json::json!([scope]);
        assert_eq!(
            changes,
            vec![
                (
                    "live.kill_switch.activated.v1".to_owned(),
                    "user.risk".to_owned(),
                    active.clone()
                ),
                (
                    "live.kill_switch.activated.v1".to_owned(),
                    "user.risk".to_owned(),
                    active
                ),
                (
                    "live.kill_switch.deactivated.v1".to_owned(),
                    "user.risk".to_owned(),
                    serde_json::json!([])
                ),
            ]
        );
        assert!(entries
            .iter()
            .any(|entry| entry["occurred_at"] == activated.operated_at.as_str()));
        let _ = std::fs::remove_dir_all(scratch);
    }

    #[tokio::test]
    async fn live_kill_switch_rpcs_fail_closed_without_a_configured_route() {
        let mut service = service();
        service.identity = Some(operator_identity());
        let risk = risk_token(&service).await;
        let status = service
            .activate_live_kill_switch(authorized(live_kill_switch_request("global"), &risk))
            .await
            .expect_err("no route, no change");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn paper_kill_switch_rpcs_fail_closed_without_a_configured_route() {
        let mut service = service();
        service.identity = Some(operator_identity());
        let risk = risk_token(&service).await;
        let status = service
            .activate_paper_kill_switch(authorized(kill_switch_request("global"), &risk))
            .await
            .expect_err("no route, no change");
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
    }

    #[test]
    fn server_time_renders_as_the_canonical_utc_timestamp() {
        assert_eq!(utc_timestamp(0).unwrap(), "1970-01-01T00:00:00Z");
        assert_eq!(
            utc_timestamp(1_700_000_000).unwrap(),
            "2023-11-14T22:13:20Z"
        );
    }

    #[tokio::test]
    async fn operator_login_requires_the_password_and_a_fresh_second_factor() {
        let mut service = service();
        assert_eq!(
            service
                .begin_operator_login(Request::new(BeginOperatorLoginRequest {
                    tenant_id: "tenant.alpha".to_owned(),
                    email: "trader@example.com".to_owned(),
                    password: OPERATOR_PASSWORD.to_owned(),
                }))
                .await
                .expect_err("no directory, no login")
                .code(),
            tonic::Code::FailedPrecondition
        );
        service.identity = Some(operator_identity());
        let begin = |password: &str, tenant: &str| BeginOperatorLoginRequest {
            tenant_id: tenant.to_owned(),
            email: "trader@example.com".to_owned(),
            password: password.to_owned(),
        };
        for (password, tenant) in [
            ("Wrong-Horse-9-Battery", "tenant.alpha"),
            (OPERATOR_PASSWORD, "tenant.beta"),
        ] {
            assert_eq!(
                service
                    .begin_operator_login(Request::new(begin(password, tenant)))
                    .await
                    .expect_err("password step must fail")
                    .code(),
                tonic::Code::Unauthenticated
            );
        }
        let challenge = |service: &OperatingSystemService| {
            let service = service.clone();
            async move {
                service
                    .begin_operator_login(Request::new(begin(OPERATOR_PASSWORD, "tenant.alpha")))
                    .await
                    .unwrap()
                    .into_inner()
                    .challenge_token
            }
        };
        let complete = |token: String, code: String| CompleteOperatorLoginRequest {
            challenge_token: token,
            totp_code: code,
        };
        let now = now_epoch_seconds().unwrap();
        let good = totp_code(&test_directory().trader_secret, now).unwrap();
        let wrong = format!("{:06}", (good.parse::<u32>().unwrap() + 1) % 1_000_000);
        assert_eq!(
            service
                .complete_operator_login(Request::new(complete(challenge(&service).await, wrong)))
                .await
                .expect_err("a wrong code must fail")
                .code(),
            tonic::Code::Unauthenticated
        );
        let session = service
            .complete_operator_login(Request::new(complete(
                challenge(&service).await,
                good.clone(),
            )))
            .await
            .expect("the right code logs in")
            .into_inner();
        assert_eq!(session.session_token.len(), 64);
        // The same code cannot mint a second session.
        assert_eq!(
            service
                .complete_operator_login(Request::new(complete(challenge(&service).await, good)))
                .await
                .expect_err("a replayed code must fail")
                .code(),
            tonic::Code::Unauthenticated
        );
    }

    #[test]
    fn a_paper_route_requires_an_operator_directory_and_remote_login_requires_tls() {
        let mut config = base_config();
        config.paper_command_route = Some(PathBuf::from("paper-route.json"));
        assert!(config.validate_operator_authentication().is_err());
        config.operator_directory = Some(PathBuf::from("operators.json"));
        assert!(config.validate_operator_authentication().is_ok());

        config.paper_command_route = None;
        config.bind = "0.0.0.0:50051".parse().expect("remote bind");
        assert!(config.validate_operator_authentication().is_err());
        config.tls_certificate = Some(PathBuf::from("server.pem"));
        config.tls_private_key = Some(PathBuf::from("server-key.pem"));
        assert!(config.validate_operator_authentication().is_ok());
    }

    #[test]
    fn the_operator_directory_file_loads_only_when_safe_and_valid() {
        let sequence = PAPER_ROUTE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let scratch = std::env::temp_dir().join(format!(
            "follon-trading-api-operators-{}-{sequence}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let valid = scratch.join("operators.json");
        std::fs::write(&valid, &test_directory().json).unwrap();
        assert!(operator_identity_from_path(&valid).is_ok());

        let tampered = scratch.join("tampered.json");
        let mut directory = OperatorDirectory::parse(&test_directory().json).unwrap();
        directory.operators[0].totp_secret_base32 = "MZXW6YTBOI".to_owned();
        std::fs::write(&tampered, serde_json::to_string(&directory).unwrap()).unwrap();
        assert!(operator_identity_from_path(&tampered).is_err());

        let garbage = scratch.join("garbage.json");
        std::fs::write(&garbage, "{").unwrap();
        assert!(operator_identity_from_path(&garbage).is_err());
        assert!(
            operator_identity_from_path(&scratch).is_err(),
            "a directory is not a file"
        );
        assert!(operator_identity_from_path(&scratch.join("missing.json")).is_err());
        let _ = std::fs::remove_dir_all(scratch);
    }
}
