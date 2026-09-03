//! Deployed gRPC topology for broker-neutral execution planning, portfolio
//! risk, and multi-currency margin valuation.

use std::collections::BTreeMap;
use std::env;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use follon_accounting::{
    value_margin_account, Currency, FxBook, FxQuote, MarginPolicy, MarginPosition, MarginRate,
};
use follon_domain::{validate_canonical_id, Decimal, Side};
use follon_execution::{
    plan_execution, plan_option_combo, plan_passive_repricing, ChildInstruction as CoreChild,
    ChildOrderKind as CoreChildOrderKind, ComboPriceLimit, ExecutionAlgorithm, OptionComboLeg,
    ParentOrder, PassiveMarketObservation, PassiveRepricePolicy,
};
use follon_postgres::PostgresStore;
use follon_risk::{
    evaluate_portfolio_risk, CandidateOrder as CoreCandidateOrder, PortfolioRiskPolicy,
    PortfolioRiskSnapshot, RestingOrder as CoreRestingOrder, RiskPosition,
};
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
    BucketLimit, CancelReplaceInstruction, ChildInstruction, ChildOrderKind, ComboLegInstruction,
    ComboPriceLimitKind, CurrencyAmount, ExecutionAlgorithmKind, ExecutionPlanRequest,
    ExecutionPlanResponse, ExecutionSide, HealthRequest, HealthResponse, MarginAccountRequest,
    MarginAccountResponse, OptionComboRequest, OptionComboResponse, PassiveRepricingRequest,
    PassiveRepricingResponse, PortfolioRiskRequest, PortfolioRiskResponse, RiskMetrics,
};

#[derive(Clone)]
struct OperatingSystemService {
    database: Option<Arc<Mutex<PostgresStore>>>,
    transport_tls: bool,
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

struct RuntimeConfig {
    bind: SocketAddr,
    production: bool,
    database_url: Option<String>,
    database_ca: Option<PathBuf>,
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
        Ok(config)
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name).map(PathBuf::from)
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
    let database = if let Some(database_url) = &config.database_url {
        let mut store = if config.production {
            PostgresStore::connect_tls(database_url, config.database_ca.as_deref())?
        } else {
            PostgresStore::connect_development(database_url)?
        };
        store.migrate()?;
        Some(Arc::new(Mutex::new(store)))
    } else {
        None
    };
    let transport_tls = config.tls_certificate.is_some();
    let service = OperatingSystemService {
        database,
        transport_tls,
    };
    let mut server = Server::builder();
    if let (Some(certificate_path), Some(private_key_path)) =
        (&config.tls_certificate, &config.tls_private_key)
    {
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
        "follon-trading-api listening on {} (tls={}, production={})",
        config.bind, transport_tls, config.production
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

    fn service() -> OperatingSystemService {
        OperatingSystemService {
            database: None,
            transport_tls: false,
        }
    }

    #[test]
    fn execution_side_rejects_unspecified() {
        assert!(side(ExecutionSide::Unspecified as i32).is_err());
        assert_eq!(side(ExecutionSide::Buy as i32).unwrap(), Side::Buy);
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
}
