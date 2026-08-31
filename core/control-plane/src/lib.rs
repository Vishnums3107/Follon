//! Deterministic, non-live trading control-plane building blocks.
//!
//! This crate owns the first vertical slice only. It has no broker adapter,
//! wall clock, database driver, or strategy-runtime dependency.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::str::FromStr;

use follon_domain::{
    price_deviation_bps, validate_canonical_id, validate_utc_timestamp, AuditTrail, Bar, Decimal,
    DecimalError, DomainError, EventEnvelope, EventPayload, Fill, OrderIntent, OrderState,
    OrderStateChange, OrderType, PnlSnapshot, PositionSnapshot, RiskDecision, Side, TimeInForce,
};
use follon_instrument::{InstrumentRegistry, TradingCalendar};
use sha2::{Digest, Sha256};

/// Error returned by the deterministic trading kernel.
#[derive(Debug)]
pub struct EngineError(pub String);

impl std::fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for EngineError {}

impl From<DomainError> for EngineError {
    fn from(error: DomainError) -> Self {
        Self(error.0)
    }
}

impl From<DecimalError> for EngineError {
    fn from(error: DecimalError) -> Self {
        Self(error.0)
    }
}

impl From<io::Error> for EngineError {
    fn from(error: io::Error) -> Self {
        Self(error.to_string())
    }
}

/// A controllable logical UTC clock used by replay and tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayClock {
    now: String,
}

impl ReplayClock {
    /// Starts the clock at a caller-provided UTC timestamp.
    pub fn new(initial_time: impl Into<String>) -> Result<Self, EngineError> {
        let now = initial_time.into();
        validate_utc_timestamp("replay clock initial time", &now)?;
        Ok(Self { now })
    }

    /// Returns the current logical timestamp.
    pub fn now(&self) -> &str {
        &self.now
    }

    /// Advances time explicitly. Production replay parsing will enforce UTC.
    pub fn advance_to(&mut self, time: impl Into<String>) -> Result<(), EngineError> {
        let time = time.into();
        validate_utc_timestamp("replay clock time", &time)?;
        if time < self.now {
            return Err(EngineError("replay clock cannot move backwards".to_owned()));
        }
        self.now = time;
        Ok(())
    }
}

/// Append-only destination for validated event envelopes.
pub trait EventSink {
    /// Persists one event exactly once by event identity.
    fn append(&mut self, event: &EventEnvelope) -> Result<(), EngineError>;
}

/// In-memory event log used by deterministic tests and projections.
#[derive(Default)]
pub struct InMemoryEventStore {
    events: Vec<EventEnvelope>,
    event_ids: HashSet<String>,
}

impl InMemoryEventStore {
    /// Returns events in their immutable append order.
    pub fn events(&self) -> &[EventEnvelope] {
        &self.events
    }
}

impl EventSink for InMemoryEventStore {
    fn append(&mut self, event: &EventEnvelope) -> Result<(), EngineError> {
        event.validate()?;
        if !self.event_ids.insert(event.event_id.clone()) {
            return Err(EngineError(format!(
                "duplicate event ID: {}",
                event.event_id
            )));
        }
        self.events.push(event.clone());
        Ok(())
    }
}

/// Newline-delimited, canonical JSON event store for the local replay slice.
pub struct FileEventStore {
    file: File,
    event_ids: HashSet<String>,
}

impl FileEventStore {
    /// Opens or creates an append-only local event log.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, EngineError> {
        let path = path.as_ref();
        let mut event_ids = HashSet::new();
        if path.exists() {
            for (index, line) in fs::read_to_string(path)?
                .lines()
                .filter(|line| !line.is_empty())
                .enumerate()
            {
                let record = parse_canonical_event(line, index + 1)?;
                let object = record.as_object().expect("validated event is an object");
                let event_id = json_required_string(object, "event_id", index + 1)?;
                if let Some(causation_id) = object
                    .get("causation_id")
                    .and_then(serde_json::Value::as_str)
                {
                    if !event_ids.contains(causation_id) {
                        return Err(EngineError(format!(
                            "event line {} references a causation ID that has not been persisted",
                            index + 1
                        )));
                    }
                }
                if !event_ids.insert(event_id.to_owned()) {
                    return Err(EngineError(
                        "existing event log contains a duplicate event ID".to_owned(),
                    ));
                }
            }
        }
        Ok(Self {
            file: OpenOptions::new().create(true).append(true).open(path)?,
            event_ids,
        })
    }
}

/// A normalized historical bar paired with its source event time in UTC.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoricalBar {
    /// Source time in UTC, retained separately from local receipt time.
    pub event_time: String,
    /// Validated normalized market payload.
    pub bar: Bar,
}

/// Immutable market prerequisites applied before a normalized bar reaches a strategy.
pub struct MarketPreconditions<'a> {
    /// Effective-dated canonical instrument reference data.
    pub instruments: &'a InstrumentRegistry,
    /// Explicit exchange-session dependency; never the local machine clock.
    pub calendar: &'a dyn TradingCalendar,
}

impl MarketPreconditions<'_> {
    fn validate(&self, bar: &Bar, event_time: &str) -> Result<(), EngineError> {
        let version = self
            .instruments
            .resolve(&bar.instrument_id, event_time)
            .ok_or_else(|| {
                EngineError("no effective instrument reference data for market event".to_owned())
            })?;
        if version.instrument.trading_calendar_id != self.calendar.calendar_id() {
            return Err(EngineError(
                "instrument and replay calendar do not match".to_owned(),
            ));
        }
        if !self
            .calendar
            .is_instrument_open_at(&bar.instrument_id, event_time)
        {
            return Err(EngineError(
                "market event is outside an explicit tradable session or occurs during a configured halt"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

/// Imports the deliberately small v1 historical-bar CSV format.
///
/// The initial importer accepts unquoted CSV only. Provider-specific parsers
/// belong in adapters; this boundary receives already selected US equity/ETF
/// rows and validates the canonical bar before it can reach a strategy.
pub fn import_historical_bars(csv: &str) -> Result<Vec<HistoricalBar>, EngineError> {
    const HEADER: &str =
        "event_time,instrument_id,open,high,low,close,volume,interval_seconds,exchange_timezone";
    let mut lines = csv.lines().filter(|line| !line.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| EngineError("historical-bar CSV is empty".to_owned()))?;
    if header.trim_start_matches('\u{feff}').trim_end_matches('\r') != HEADER {
        return Err(EngineError(
            "historical-bar CSV header does not match v1 contract".to_owned(),
        ));
    }
    let mut bars = Vec::new();
    let mut identities = BTreeSet::new();
    let mut previous_key: Option<(String, String)> = None;
    for (index, line) in lines.enumerate() {
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        if fields.len() != 9 {
            return Err(EngineError(format!(
                "invalid historical-bar row {}",
                index + 2
            )));
        }
        validate_utc_timestamp("historical bar event_time", fields[0]).map_err(|error| {
            EngineError(format!("invalid historical-bar row {}: {error}", index + 2))
        })?;
        let decimal = |field: usize| -> Result<Decimal, EngineError> {
            Decimal::from_str(fields[field]).map_err(|error| {
                EngineError(format!("invalid decimal on row {}: {}", index + 2, error))
            })
        };
        let interval_seconds = fields[7]
            .parse::<u32>()
            .map_err(|_| EngineError(format!("invalid interval on row {}", index + 2)))?;
        let bar = Bar {
            instrument_id: fields[1].to_owned(),
            open: decimal(2)?,
            high: decimal(3)?,
            low: decimal(4)?,
            close: decimal(5)?,
            volume: decimal(6)?,
            interval_seconds,
            exchange_timezone: fields[8].to_owned(),
        };
        bar.validate()?;
        let key = (fields[0].to_owned(), bar.instrument_id.clone());
        if !identities.insert(key.clone()) {
            return Err(EngineError(format!(
                "duplicate historical bar on row {}",
                index + 2
            )));
        }
        if previous_key
            .as_ref()
            .is_some_and(|previous| key < *previous)
        {
            return Err(EngineError(format!(
                "historical-bar rows are not in canonical event-time/instrument order at row {}",
                index + 2
            )));
        }
        previous_key = Some(key);
        bars.push(HistoricalBar {
            event_time: fields[0].to_owned(),
            bar,
        });
    }
    if bars.is_empty() {
        return Err(EngineError(
            "historical-bar CSV contains no data rows".to_owned(),
        ));
    }
    Ok(bars)
}

/// Loads normalized market-bar events from a canonical persisted NDJSON log.
///
/// Non-market events are retained as evidence in the source log but are not
/// replay inputs. The loader rejects malformed, duplicate, and out-of-order
/// source market events rather than silently repairing them.
pub fn load_persisted_market_bars(
    path: impl AsRef<std::path::Path>,
) -> Result<Vec<HistoricalBar>, EngineError> {
    let contents = fs::read_to_string(path)?;
    let mut event_ids = HashSet::new();
    let mut bars = Vec::new();
    let mut previous_market_key: Option<(String, String)> = None;
    let mut bar_identities = BTreeSet::new();

    for (index, line) in contents.lines().filter(|line| !line.is_empty()).enumerate() {
        let record = parse_canonical_event(line, index + 1)?;
        let object = record
            .as_object()
            .ok_or_else(|| EngineError(format!("event line {} is not an object", index + 1)))?;
        let event_id = json_required_string(object, "event_id", index + 1)?;
        validate_canonical_id("event_id", event_id)?;
        if !event_ids.insert(event_id.to_owned()) {
            return Err(EngineError(format!(
                "duplicate persisted event ID: {event_id}"
            )));
        }
        if json_required_string(object, "event_type", index + 1)? != "market.bar.v1" {
            continue;
        }
        let event_time = json_required_string(object, "event_time", index + 1)?.to_owned();
        let payload = object
            .get("payload")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| {
                EngineError(format!(
                    "market event line {} has no object payload",
                    index + 1
                ))
            })?;
        let decimal = |field: &str| -> Result<Decimal, EngineError> {
            Decimal::from_str(json_required_string(payload, field, index + 1)?).map_err(|error| {
                EngineError(format!(
                    "invalid {field} on event line {}: {error}",
                    index + 1
                ))
            })
        };
        let interval_seconds = payload
            .get("interval_seconds")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                EngineError(format!(
                    "invalid interval_seconds on event line {}",
                    index + 1
                ))
            })?;
        let bar = Bar {
            instrument_id: json_required_string(payload, "instrument_id", index + 1)?.to_owned(),
            open: decimal("open")?,
            high: decimal("high")?,
            low: decimal("low")?,
            close: decimal("close")?,
            volume: decimal("volume")?,
            interval_seconds,
            exchange_timezone: json_required_string(payload, "exchange_timezone", index + 1)?
                .to_owned(),
        };
        bar.validate()?;
        let key = (event_time.clone(), bar.instrument_id.clone());
        if !bar_identities.insert(key.clone())
            || previous_market_key
                .as_ref()
                .is_some_and(|previous| key < *previous)
        {
            return Err(EngineError(format!(
                "market event line {} is duplicate or out of canonical order",
                index + 1
            )));
        }
        previous_market_key = Some(key);
        bars.push(HistoricalBar { event_time, bar });
    }
    if bars.is_empty() {
        return Err(EngineError(
            "persisted event log contains no market bars".to_owned(),
        ));
    }
    Ok(bars)
}

fn json_required_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
    line: usize,
) -> Result<&'a str, EngineError> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| EngineError(format!("event line {line} has no {field}")))
}

fn parse_canonical_event(line: &str, line_number: usize) -> Result<serde_json::Value, EngineError> {
    let record: serde_json::Value = serde_json::from_str(line).map_err(|error| {
        EngineError(format!("invalid JSON event on line {line_number}: {error}"))
    })?;
    if serde_json::to_string(&record).map_err(|error| EngineError(error.to_string()))? != line {
        return Err(EngineError(format!(
            "event line {line_number} is not canonical JSON"
        )));
    }
    let object = record
        .as_object()
        .ok_or_else(|| EngineError(format!("event line {line_number} is not an object")))?;
    let event_id = json_required_string(object, "event_id", line_number)?;
    let correlation_id = json_required_string(object, "correlation_id", line_number)?;
    validate_canonical_id("event_id", event_id)?;
    validate_canonical_id("correlation_id", correlation_id)?;
    for field in [
        "event_type",
        "actor",
        "source",
        "software_version",
        "configuration_version",
    ] {
        json_required_string(object, field, line_number)?;
    }
    validate_utc_timestamp(
        "persisted event_time",
        json_required_string(object, "event_time", line_number)?,
    )?;
    validate_utc_timestamp(
        "persisted receive_time",
        json_required_string(object, "receive_time", line_number)?,
    )?;
    if object
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
        || !object
            .get("payload")
            .is_some_and(serde_json::Value::is_object)
    {
        return Err(EngineError(format!(
            "event line {line_number} has an invalid schema or payload"
        )));
    }
    for field in ["account_id", "strategy_id", "instrument_id", "causation_id"] {
        if let Some(value) = object.get(field) {
            if !value.is_null() {
                let identifier = value.as_str().ok_or_else(|| {
                    EngineError(format!("event line {line_number} has invalid {field}"))
                })?;
                validate_canonical_id(field, identifier)?;
            }
        } else {
            return Err(EngineError(format!(
                "event line {line_number} has no {field}"
            )));
        }
    }
    Ok(record)
}

impl EventSink for FileEventStore {
    fn append(&mut self, event: &EventEnvelope) -> Result<(), EngineError> {
        event.validate()?;
        if !self.event_ids.insert(event.event_id.clone()) {
            return Err(EngineError(format!(
                "duplicate event ID: {}",
                event.event_id
            )));
        }
        self.file.write_all(event.canonical_json().as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        self.file.sync_data()?;
        Ok(())
    }
}

/// The only strategy interaction point in the trading kernel.
pub trait Strategy {
    /// Handles one normalized bar and may emit exactly one declarative intent.
    fn on_bar(&mut self, bar: &Bar, replay_time: &str) -> Result<Option<OrderIntent>, EngineError>;

    /// Receives an execution after the replay portfolio has applied it.
    ///
    /// The default deliberately does nothing. Isolated workers use this hook to
    /// construct their next immutable portfolio snapshot; it never permits a
    /// strategy to alter a fill, broker state, or risk decision.
    fn on_execution(
        &mut self,
        _fill: &Fill,
        _position: &PositionSnapshot,
    ) -> Result<(), EngineError> {
        Ok(())
    }
}

/// Immutable identity expected from an isolated strategy worker process.
#[derive(Clone, Debug)]
pub struct StrategyWorkerIdentity {
    /// Account context supplied to every callback.
    pub account_id: String,
    /// Canonical strategy identity.
    pub strategy_id: String,
    /// Immutable strategy release selected for this run.
    pub strategy_version: String,
    /// Immutable configuration selected for this run.
    pub configuration_version: String,
    /// SHA-256 identity of the complete declared strategy bundle.
    pub strategy_bundle_hash: String,
    /// Execution environment; the first worker implementation permits simulation only.
    pub environment: String,
}

impl StrategyWorkerIdentity {
    fn validate(&self) -> Result<(), EngineError> {
        for (name, value) in [
            ("account_id", self.account_id.as_str()),
            ("strategy_id", self.strategy_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        if self.strategy_version.is_empty()
            || self.configuration_version.is_empty()
            || self.environment != "SIMULATION"
            || !is_sha256(&self.strategy_bundle_hash)
        {
            return Err(EngineError("invalid strategy worker identity".to_owned()));
        }
        Ok(())
    }
}

/// Explicit single-currency starting balance for bounded worker services.
///
/// It is intentionally limited to the deterministic replay account. A worker
/// receives a snapshot only; it cannot mutate cash, positions, adapters, or
/// credentials.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrategyWorkerServicesConfig {
    /// ISO currency used by the replay account and every current position.
    pub currency: String,
    /// Exact opening cash supplied by the immutable backtest input.
    pub initial_cash: Decimal,
}

impl StrategyWorkerServicesConfig {
    fn validate(&self) -> Result<(), EngineError> {
        if self.currency.len() != 3
            || !self.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            || self.initial_cash < Decimal::ZERO
        {
            return Err(EngineError(
                "invalid strategy worker service account snapshot".to_owned(),
            ));
        }
        Ok(())
    }
}

/// One exact custom metric returned by a bounded isolated strategy callback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrategyWorkerMetric {
    /// Canonical metric name.
    pub name: String,
    /// Exact reported value.
    pub value: Decimal,
    /// Replay-clock time at which the strategy measured the value.
    pub observed_at: String,
    /// Stable canonical tag key/value pairs.
    pub tags: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
struct WorkerPortfolioPosition {
    quantity: Decimal,
    average_cost: Decimal,
    mark_price: Decimal,
}

#[derive(Clone, Debug)]
struct WorkerRuntimeServices {
    currency: String,
    cash: Decimal,
    positions: BTreeMap<String, WorkerPortfolioPosition>,
    history: Vec<(String, Bar)>,
    state: serde_json::Map<String, serde_json::Value>,
    state_fingerprint: String,
    latest_metrics: Vec<StrategyWorkerMetric>,
}

impl WorkerRuntimeServices {
    const MAX_HISTORY_RECORDS: usize = 1_000_000;
    const MAX_STATE_BYTES: usize = 65_536;
    const MAX_METRICS: usize = 10_000;

    fn new(config: StrategyWorkerServicesConfig) -> Result<Self, EngineError> {
        config.validate()?;
        let state = serde_json::Map::new();
        Ok(Self {
            currency: config.currency,
            cash: config.initial_cash,
            positions: BTreeMap::new(),
            history: Vec::new(),
            state_fingerprint: json_fingerprint(&state)?,
            state,
            latest_metrics: Vec::new(),
        })
    }

    fn observe_bar(&mut self, bar: &Bar, replay_time: &str) -> Result<(), EngineError> {
        validate_utc_timestamp("worker replay_time", replay_time)?;
        if self.history.len() >= Self::MAX_HISTORY_RECORDS {
            return Err(EngineError(
                "strategy worker history exceeds its bounded service contract".to_owned(),
            ));
        }
        if let Some(position) = self.positions.get_mut(&bar.instrument_id) {
            position.mark_price = bar.close;
        }
        self.history.push((replay_time.to_owned(), bar.clone()));
        Ok(())
    }

    fn apply_execution(
        &mut self,
        fill: &Fill,
        position: &PositionSnapshot,
    ) -> Result<(), EngineError> {
        if fill.instrument_id != position.instrument_id
            || fill.quantity <= Decimal::ZERO
            || fill.price <= Decimal::ZERO
            || fill.fee < Decimal::ZERO
        {
            return Err(EngineError(
                "worker service snapshot received an invalid execution".to_owned(),
            ));
        }
        let gross = fill.price.checked_mul(fill.quantity)?;
        self.cash = match fill.side {
            Side::Buy => self.cash.checked_sub(gross.checked_add(fill.fee)?)?,
            Side::Sell => self.cash.checked_add(gross.checked_sub(fill.fee)?)?,
        };
        self.positions.insert(
            position.instrument_id.clone(),
            WorkerPortfolioPosition {
                quantity: position.quantity,
                average_cost: position.average_cost,
                mark_price: fill.price,
            },
        );
        Ok(())
    }

    fn service_payload(&self, replay_time: &str) -> serde_json::Value {
        let history = self
            .history
            .iter()
            .map(|(event_time, bar)| {
                serde_json::json!({
                    "event_time": event_time,
                    "bar": worker_bar_payload(bar),
                })
            })
            .collect::<Vec<_>>();
        let positions = self
            .positions
            .iter()
            .map(|(instrument_id, position)| {
                serde_json::json!({
                    "instrument_id": instrument_id,
                    "quantity": position.quantity.to_string(),
                    "average_cost": position.average_cost.to_string(),
                    "mark_price": position.mark_price.to_string(),
                    "currency": self.currency,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "history": { "as_of": replay_time, "records": history },
            "portfolio": {
                "as_of": replay_time,
                "positions": positions,
                "cash_by_currency": [{ "currency": self.currency, "amount": self.cash.to_string() }],
            },
            "state": { "values": self.state },
        })
    }

    fn store_output(
        &mut self,
        state: &serde_json::Value,
        metrics: &serde_json::Value,
        replay_time: &str,
    ) -> Result<(), EngineError> {
        let state = state.as_object().ok_or_else(|| {
            EngineError("strategy worker service state is not an object".to_owned())
        })?;
        require_exact_json_fields(
            state,
            &["fingerprint", "values"],
            "strategy worker service state",
        )?;
        let fingerprint = json_value_string(state, "fingerprint")?;
        if !is_sha256(fingerprint) {
            return Err(EngineError(
                "strategy worker service state has an invalid fingerprint".to_owned(),
            ));
        }
        let values = state
            .get("values")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| {
                EngineError("strategy worker service state values are not an object".to_owned())
            })?;
        let computed = json_fingerprint(values)?;
        if computed != fingerprint {
            return Err(EngineError(
                "strategy worker service state fingerprint does not bind its values".to_owned(),
            ));
        }
        let metrics = metrics
            .as_array()
            .ok_or_else(|| EngineError("strategy worker metrics are not an array".to_owned()))?;
        if metrics.len() > Self::MAX_METRICS {
            return Err(EngineError(
                "strategy worker metrics exceed the bounded service contract".to_owned(),
            ));
        }
        let mut parsed_metrics = Vec::with_capacity(metrics.len());
        for metric in metrics {
            let metric = metric
                .as_object()
                .ok_or_else(|| EngineError("strategy worker metric is not an object".to_owned()))?;
            require_exact_json_fields(
                metric,
                &["name", "observed_at", "tags", "value"],
                "strategy worker metric",
            )?;
            let name = json_value_string(metric, "name")?.to_owned();
            validate_canonical_id("strategy worker metric name", &name)?;
            let observed_at = json_value_string(metric, "observed_at")?.to_owned();
            validate_utc_timestamp("strategy worker metric observed_at", &observed_at)?;
            if observed_at.as_str() > replay_time {
                return Err(EngineError(
                    "strategy worker metric contains look-ahead time".to_owned(),
                ));
            }
            let value = Decimal::from_str(json_value_string(metric, "value")?)?;
            let tags = metric
                .get("tags")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| {
                    EngineError("strategy worker metric tags are not an array".to_owned())
                })?;
            if tags.len() > 32 {
                return Err(EngineError(
                    "strategy worker metric has too many tags".to_owned(),
                ));
            }
            let mut keys = BTreeSet::new();
            let mut parsed_tags = Vec::with_capacity(tags.len());
            for tag in tags {
                let tag = tag.as_object().ok_or_else(|| {
                    EngineError("strategy worker metric tag is not an object".to_owned())
                })?;
                require_exact_json_fields(tag, &["key", "value"], "strategy worker metric tag")?;
                let key = json_value_string(tag, "key")?.to_owned();
                let value = json_value_string(tag, "value")?.to_owned();
                validate_canonical_id("strategy worker metric tag key", &key)?;
                validate_canonical_id("strategy worker metric tag value", &value)?;
                if !keys.insert(key.clone()) {
                    return Err(EngineError(
                        "strategy worker metric has duplicate tag keys".to_owned(),
                    ));
                }
                parsed_tags.push((key, value));
            }
            parsed_metrics.push(StrategyWorkerMetric {
                name,
                value,
                observed_at,
                tags: parsed_tags,
            });
        }
        self.state = values.clone();
        self.state_fingerprint = fingerprint.to_owned();
        self.latest_metrics = parsed_metrics;
        Ok(())
    }
}

/// Stdio adapter for the versioned isolated strategy-worker protocol.
///
/// The child receives only normalized market bars and immutable strategy
/// context. Its output is parsed and validated as an intent before the risk
/// engine sees it; a worker never receives adapters or credentials.
pub struct ProcessStrategyWorker {
    identity: StrategyWorkerIdentity,
    services: Option<WorkerRuntimeServices>,
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl ProcessStrategyWorker {
    /// Starts a worker process and verifies its announced immutable bundle identity.
    pub fn spawn(
        program: impl AsRef<OsStr>,
        arguments: impl IntoIterator<Item = OsString>,
        identity: StrategyWorkerIdentity,
    ) -> Result<Self, EngineError> {
        Self::spawn_inner(program, arguments, identity, None)
    }

    /// Starts a worker with bounded point-in-time data, portfolio, state, and
    /// metrics services enabled for every callback.
    pub fn spawn_with_services(
        program: impl AsRef<OsStr>,
        arguments: impl IntoIterator<Item = OsString>,
        identity: StrategyWorkerIdentity,
        services: StrategyWorkerServicesConfig,
    ) -> Result<Self, EngineError> {
        Self::spawn_inner(
            program,
            arguments,
            identity,
            Some(WorkerRuntimeServices::new(services)?),
        )
    }

    fn spawn_inner(
        program: impl AsRef<OsStr>,
        arguments: impl IntoIterator<Item = OsString>,
        identity: StrategyWorkerIdentity,
        services: Option<WorkerRuntimeServices>,
    ) -> Result<Self, EngineError> {
        identity.validate()?;
        let mut command = Command::new(program);
        command
            .args(arguments)
            .env_clear()
            .env("PYTHONIOENCODING", "utf-8");
        if let Some(sdk_path) = std::env::var_os("FOLLON_STRATEGY_SDK_PATH") {
            command.env("PYTHONPATH", sdk_path);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| EngineError(format!("strategy worker did not start: {error}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| EngineError("strategy worker stdin could not be captured".to_owned()))?;
        let stdout = child.stdout.take().ok_or_else(|| {
            EngineError("strategy worker stdout could not be captured".to_owned())
        })?;
        let mut worker = Self {
            identity,
            services,
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
        };
        worker.verify_ready()?;
        Ok(worker)
    }

    fn verify_ready(&mut self) -> Result<(), EngineError> {
        let frame = self.read_frame()?;
        let object = frame.as_object().ok_or_else(|| {
            EngineError("strategy worker ready frame is not an object".to_owned())
        })?;
        require_exact_json_fields(
            object,
            &[
                "bundle_hash",
                "protocol_version",
                "strategy_id",
                "strategy_version",
                "type",
            ],
            "strategy worker ready frame",
        )?;
        if json_value_string(object, "type")? != "ready"
            || json_value_u64(object, "protocol_version")? != 1
            || json_value_string(object, "bundle_hash")? != self.identity.strategy_bundle_hash
            || json_value_string(object, "strategy_id")? != self.identity.strategy_id
            || json_value_string(object, "strategy_version")? != self.identity.strategy_version
        {
            return Err(EngineError(
                "strategy worker does not match the declared immutable bundle".to_owned(),
            ));
        }
        Ok(())
    }

    fn read_frame(&mut self) -> Result<serde_json::Value, EngineError> {
        let mut line = String::new();
        let bytes = self.stdout.read_line(&mut line)?;
        if bytes == 0 {
            return Err(EngineError(
                "strategy worker closed stdout before returning a response".to_owned(),
            ));
        }
        serde_json::from_str(&line)
            .map_err(|error| EngineError(format!("strategy worker emitted invalid JSON: {error}")))
    }

    fn request_intent(
        &mut self,
        bar: &Bar,
        replay_time: &str,
    ) -> Result<Option<OrderIntent>, EngineError> {
        let mut frame = serde_json::json!({
            "protocol_version": 1,
            "type": "market_bar",
            "context": {
                "account_id": self.identity.account_id,
                "strategy_id": self.identity.strategy_id,
                "strategy_version": self.identity.strategy_version,
                "configuration_version": self.identity.configuration_version,
                "replay_time": replay_time,
                "environment": self.identity.environment,
            },
            "bar": worker_bar_payload(bar),
        });
        if let Some(services) = self.services.as_mut() {
            services.observe_bar(bar, replay_time)?;
            frame
                .as_object_mut()
                .expect("worker request frame remains an object")
                .insert("services".to_owned(), services.service_payload(replay_time));
        }
        let serialized =
            serde_json::to_string(&frame).expect("serializing a JSON worker frame cannot fail");
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| EngineError("strategy worker stdin is already closed".to_owned()))?;
        stdin.write_all(serialized.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;

        let response = self.read_frame()?;
        let object = response
            .as_object()
            .ok_or_else(|| EngineError("strategy worker response is not an object".to_owned()))?;
        if json_value_u64(object, "protocol_version")? != 1 {
            return Err(EngineError(
                "strategy worker returned an unsupported protocol version".to_owned(),
            ));
        }
        match json_value_string(object, "type")? {
            "strategy_output" => {
                if let Some(services) = self.services.as_mut() {
                    require_exact_json_fields(
                        object,
                        &["intent", "metrics", "protocol_version", "state", "type"],
                        "strategy worker enriched output frame",
                    )?;
                    services.store_output(
                        object.get("state").expect("required output state exists"),
                        object
                            .get("metrics")
                            .expect("required output metrics exist"),
                        replay_time,
                    )?;
                } else {
                    require_exact_json_fields(
                        object,
                        &["intent", "protocol_version", "type"],
                        "strategy worker output frame",
                    )?;
                }
                match object.get("intent") {
                    Some(serde_json::Value::Null) => Ok(None),
                    Some(serde_json::Value::Object(intent)) => {
                        let intent = parse_worker_intent(intent)?;
                        if intent.account_id != self.identity.account_id
                            || intent.strategy_id != self.identity.strategy_id
                            || intent.strategy_version != self.identity.strategy_version
                            || intent.configuration_version != self.identity.configuration_version
                            || intent.environment != self.identity.environment
                        {
                            return Err(EngineError(
                                "worker intent does not match its immutable execution context"
                                    .to_owned(),
                            ));
                        }
                        Ok(Some(intent))
                    }
                    _ => Err(EngineError(
                        "strategy worker output has no nullable intent object".to_owned(),
                    )),
                }
            }
            "error" => {
                let has_valid_fields = object.keys().all(|field| {
                    matches!(
                        field.as_str(),
                        "code" | "message" | "protocol_version" | "type"
                    )
                }) && matches!(object.len(), 3 | 4);
                if !has_valid_fields {
                    return Err(EngineError(
                        "strategy worker error frame has missing or unknown fields".to_owned(),
                    ));
                }
                if object.contains_key("message") {
                    json_value_string(object, "message")?;
                }
                Err(EngineError(format!(
                    "strategy worker rejected the callback: {}",
                    json_value_string(object, "code")?
                )))
            }
            _ => Err(EngineError(
                "strategy worker returned an unexpected frame type".to_owned(),
            )),
        }
    }

    /// Returns the fingerprint of state retained after the latest enriched callback.
    pub fn state_fingerprint(&self) -> Option<&str> {
        self.services
            .as_ref()
            .map(|services| services.state_fingerprint.as_str())
    }

    /// Returns validated custom metrics from the latest enriched callback.
    pub fn latest_metrics(&self) -> Option<&[StrategyWorkerMetric]> {
        self.services
            .as_ref()
            .map(|services| services.latest_metrics.as_slice())
    }
}

impl Strategy for ProcessStrategyWorker {
    fn on_bar(&mut self, bar: &Bar, replay_time: &str) -> Result<Option<OrderIntent>, EngineError> {
        self.request_intent(bar, replay_time)
    }

    fn on_execution(
        &mut self,
        fill: &Fill,
        position: &PositionSnapshot,
    ) -> Result<(), EngineError> {
        if let Some(services) = self.services.as_mut() {
            services.apply_execution(fill, position)?;
        }
        Ok(())
    }
}

impl Drop for ProcessStrategyWorker {
    fn drop(&mut self) {
        self.stdin.take();
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn json_value_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<&'a str, EngineError> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| EngineError(format!("strategy worker has no {field}")))
}

fn json_value_u64(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<u64, EngineError> {
    object
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| EngineError(format!("strategy worker has no unsigned {field}")))
}

fn require_exact_json_fields(
    object: &serde_json::Map<String, serde_json::Value>,
    expected: &[&str],
    context: &str,
) -> Result<(), EngineError> {
    if object.len() != expected.len() || expected.iter().any(|field| !object.contains_key(*field)) {
        return Err(EngineError(format!(
            "{context} has missing or unknown fields"
        )));
    }
    Ok(())
}

fn parse_worker_intent(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<OrderIntent, EngineError> {
    require_exact_json_fields(
        object,
        &[
            "account_id",
            "configuration_version",
            "correlation_id",
            "created_at",
            "environment",
            "instrument_id",
            "intent_id",
            "limit_price",
            "order_type",
            "quantity",
            "rationale",
            "side",
            "strategy_id",
            "strategy_version",
            "time_in_force",
        ],
        "strategy worker intent",
    )?;
    let side = match json_value_string(object, "side")? {
        "BUY" => Side::Buy,
        "SELL" => Side::Sell,
        _ => return Err(EngineError("worker intent has an invalid side".to_owned())),
    };
    let order_type = match json_value_string(object, "order_type")? {
        "MARKET" => OrderType::Market,
        "LIMIT" => OrderType::Limit,
        _ => {
            return Err(EngineError(
                "worker intent has an invalid order type".to_owned(),
            ));
        }
    };
    let time_in_force = match json_value_string(object, "time_in_force")? {
        "DAY" => TimeInForce::Day,
        "GTC" => TimeInForce::GoodTilCancelled,
        _ => {
            return Err(EngineError(
                "worker intent has an invalid time in force".to_owned(),
            ));
        }
    };
    let limit_price = match object.get("limit_price") {
        Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(value)) => Some(Decimal::from_str(value)?),
        _ => {
            return Err(EngineError(
                "worker intent limit_price must be a decimal string or null".to_owned(),
            ));
        }
    };
    let intent = OrderIntent {
        intent_id: json_value_string(object, "intent_id")?.to_owned(),
        account_id: json_value_string(object, "account_id")?.to_owned(),
        strategy_id: json_value_string(object, "strategy_id")?.to_owned(),
        instrument_id: json_value_string(object, "instrument_id")?.to_owned(),
        correlation_id: json_value_string(object, "correlation_id")?.to_owned(),
        side,
        quantity: Decimal::from_str(json_value_string(object, "quantity")?)?,
        order_type,
        limit_price,
        time_in_force,
        rationale: json_value_string(object, "rationale")?.to_owned(),
        created_at: json_value_string(object, "created_at")?.to_owned(),
        strategy_version: json_value_string(object, "strategy_version")?.to_owned(),
        configuration_version: json_value_string(object, "configuration_version")?.to_owned(),
        environment: json_value_string(object, "environment")?.to_owned(),
    };
    intent.validate()?;
    Ok(intent)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn worker_bar_payload(bar: &Bar) -> serde_json::Value {
    serde_json::json!({
        "instrument_id": bar.instrument_id,
        "open": bar.open.to_string(),
        "high": bar.high.to_string(),
        "low": bar.low.to_string(),
        "close": bar.close.to_string(),
        "volume": bar.volume.to_string(),
        "interval_seconds": bar.interval_seconds,
        "exchange_timezone": bar.exchange_timezone,
    })
}

fn json_fingerprint(
    values: &serde_json::Map<String, serde_json::Value>,
) -> Result<String, EngineError> {
    let canonical = serde_json::to_string(values)
        .map_err(|error| EngineError(format!("cannot serialize strategy worker state: {error}")))?;
    if canonical.len() > WorkerRuntimeServices::MAX_STATE_BYTES {
        return Err(EngineError(
            "strategy worker state exceeds the bounded service contract".to_owned(),
        ));
    }
    Ok(format!("{:x}", Sha256::digest(canonical.as_bytes())))
}

/// A deterministic example strategy used by the first replay test.
pub struct BuyOnceStrategy {
    account_id: String,
    strategy_id: String,
    strategy_version: String,
    configuration_version: String,
    threshold: Decimal,
    submitted: bool,
}

impl BuyOnceStrategy {
    /// Creates a strategy that emits one market buy when close is at or below a threshold.
    pub fn new(
        account_id: impl Into<String>,
        strategy_id: impl Into<String>,
        strategy_version: impl Into<String>,
        configuration_version: impl Into<String>,
        threshold: Decimal,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            strategy_id: strategy_id.into(),
            strategy_version: strategy_version.into(),
            configuration_version: configuration_version.into(),
            threshold,
            submitted: false,
        }
    }
}

impl Strategy for BuyOnceStrategy {
    fn on_bar(&mut self, bar: &Bar, replay_time: &str) -> Result<Option<OrderIntent>, EngineError> {
        if self.submitted || bar.close > self.threshold {
            return Ok(None);
        }
        self.submitted = true;
        Ok(Some(OrderIntent {
            intent_id: "intent-example-000001".to_owned(),
            account_id: self.account_id.clone(),
            strategy_id: self.strategy_id.clone(),
            instrument_id: bar.instrument_id.clone(),
            correlation_id: "corr-example-000001".to_owned(),
            side: Side::Buy,
            quantity: Decimal::from_integer(1)?,
            order_type: OrderType::Market,
            limit_price: None,
            time_in_force: follon_domain::TimeInForce::Day,
            rationale: "close crossed configured entry threshold".to_owned(),
            created_at: replay_time.to_owned(),
            strategy_version: self.strategy_version.clone(),
            configuration_version: self.configuration_version.clone(),
            environment: "SIMULATION".to_owned(),
        }))
    }
}

/// Versioned deterministic risk limits for the first vertical slice.
#[derive(Clone, Debug)]
pub struct RiskPolicy {
    /// Immutable policy version used as decision evidence.
    pub version: String,
    /// Independent global control that blocks all new trading.
    pub global_kill_switch: bool,
    /// Maximum allowed absolute intent quantity.
    pub max_quantity: Decimal,
    /// Maximum allowed estimated notional at the current bar close.
    pub max_notional: Decimal,
    /// Maximum absolute limit-price distance from the current mark in basis points.
    pub max_price_deviation_bps: Decimal,
}

impl RiskPolicy {
    /// Validates policy identity and positive deterministic limits at engine construction.
    pub fn validate(&self) -> Result<(), EngineError> {
        let ten_thousand = Decimal::from_integer(10_000)?;
        if self.version.is_empty()
            || self.max_quantity <= Decimal::ZERO
            || self.max_notional <= Decimal::ZERO
            || self.max_price_deviation_bps < Decimal::ZERO
            || self.max_price_deviation_bps >= ten_thousand
        {
            return Err(EngineError("invalid deterministic risk policy".to_owned()));
        }
        Ok(())
    }

    /// Evaluates every executable request before an order exists.
    pub fn evaluate(
        &self,
        intent: &OrderIntent,
        bar: &Bar,
        replay_time: &str,
    ) -> Result<RiskDecision, EngineError> {
        intent.validate()?;
        let estimated_notional = intent.quantity.checked_mul(bar.close)?;
        let requested_price_deviation_bps = intent
            .limit_price
            .map(|price| price_deviation_bps(bar.close, price))
            .transpose()?
            .unwrap_or(Decimal::ZERO);
        let mut reason_codes = Vec::new();
        if self.global_kill_switch {
            reason_codes.push("KILL_SWITCH_ACTIVE".to_owned());
        }
        if intent.quantity > self.max_quantity {
            reason_codes.push("MAX_QUANTITY_EXCEEDED".to_owned());
        }
        if estimated_notional > self.max_notional {
            reason_codes.push("MAX_NOTIONAL_EXCEEDED".to_owned());
        }
        if requested_price_deviation_bps > self.max_price_deviation_bps {
            reason_codes.push("PRICE_COLLAR_EXCEEDED".to_owned());
        }
        if bar.close <= Decimal::ZERO {
            reason_codes.push("INVALID_MARK_PRICE".to_owned());
        }
        let approved = reason_codes.is_empty();
        if approved {
            reason_codes.push("APPROVED".to_owned());
        }
        Ok(RiskDecision {
            decision_id: format!("risk-{}", intent.intent_id),
            intent_id: intent.intent_id.clone(),
            approved,
            reason_codes,
            policy_version: self.version.clone(),
            decided_at: replay_time.to_owned(),
            correlation_id: intent.correlation_id.clone(),
            actor: "risk_engine".to_owned(),
            evaluated_limits: format!(
                "max_quantity={},max_notional={},max_price_deviation_bps={},reference_price={},requested_price={},requested_price_deviation_bps={},estimated_notional={}",
                self.max_quantity,
                self.max_notional,
                self.max_price_deviation_bps,
                bar.close,
                intent.limit_price.map_or_else(|| "MARKET".to_owned(), |price| price.to_string()),
                requested_price_deviation_bps,
                estimated_notional,
            ),
        })
    }
}

/// OMS order whose legal transitions are enforced independently of a broker.
#[derive(Clone, Debug)]
pub struct OmsOrder {
    /// Client-generated idempotency identity.
    pub order_id: String,
    /// Original approved intent.
    pub intent: OrderIntent,
    /// Current lifecycle state.
    pub state: OrderState,
}

impl OmsOrder {
    /// Creates an OMS order after, and only after, a risk approval.
    pub fn from_approved_intent(
        intent: OrderIntent,
        decision: &RiskDecision,
    ) -> Result<Self, EngineError> {
        if !decision.approved || decision.intent_id != intent.intent_id {
            return Err(EngineError(
                "an OMS order requires a matching risk approval".to_owned(),
            ));
        }
        Ok(Self {
            order_id: format!("order-{}", intent.intent_id),
            intent,
            state: OrderState::Created,
        })
    }

    /// Rebuilds a previously persisted OMS order after validating its durable identity.
    ///
    /// Recovery is intentionally explicit: a restart may restore an order, but
    /// it must never invent a new client identity or silently change a state.
    pub fn recover(
        order_id: impl Into<String>,
        intent: OrderIntent,
        state: OrderState,
    ) -> Result<Self, EngineError> {
        intent.validate()?;
        let order_id = order_id.into();
        validate_canonical_id("order_id", &order_id)?;
        if order_id != format!("order-{}", intent.intent_id) {
            return Err(EngineError(
                "persisted OMS order ID does not match its intent identity".to_owned(),
            ));
        }
        Ok(Self {
            order_id,
            intent,
            state,
        })
    }

    /// Applies a legal lifecycle transition and returns the corresponding evidence.
    pub fn transition(
        &mut self,
        next: OrderState,
        reason: impl Into<String>,
    ) -> Result<OrderStateChange, EngineError> {
        if !is_valid_transition(self.state, next) {
            return Err(EngineError(format!(
                "invalid OMS transition {} -> {}",
                self.state.as_str(),
                next.as_str()
            )));
        }
        let change = OrderStateChange {
            order_id: self.order_id.clone(),
            previous_state: Some(self.state),
            new_state: next,
            reason: reason.into(),
        };
        self.state = next;
        Ok(change)
    }
}

fn is_valid_transition(from: OrderState, to: OrderState) -> bool {
    matches!(
        (from, to),
        (
            OrderState::Created,
            OrderState::Approved | OrderState::RiskRejected
        ) | (OrderState::Approved, OrderState::PendingSubmit)
            | (
                OrderState::PendingSubmit,
                OrderState::Submitted | OrderState::Unknown
            )
            | (
                OrderState::Submitted,
                OrderState::Acknowledged
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired
                    | OrderState::Unknown
            )
            | (
                OrderState::Acknowledged,
                OrderState::PartiallyFilled
                    | OrderState::Filled
                    | OrderState::PendingCancel
                    | OrderState::PendingReplace
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired
                    | OrderState::Unknown
            )
            | (
                OrderState::PartiallyFilled,
                OrderState::Filled
                    | OrderState::PendingCancel
                    | OrderState::PendingReplace
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired
                    | OrderState::Unknown
            )
            | (
                OrderState::PendingCancel,
                OrderState::Acknowledged
                    | OrderState::PartiallyFilled
                    | OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired
                    | OrderState::Unknown
            )
            | (
                OrderState::PendingReplace,
                OrderState::Acknowledged
                    | OrderState::PartiallyFilled
                    | OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired
                    | OrderState::Unknown
            )
            | (
                OrderState::Unknown,
                OrderState::Acknowledged
                    | OrderState::PartiallyFilled
                    | OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired
                    | OrderState::PendingReplace
            )
            // A newly discovered execution is authoritative evidence.  Preserve the
            // previously persisted terminal state, enter UNKNOWN, then resolve from
            // the broker evidence rather than silently dropping the execution.
            | (
                OrderState::Cancelled | OrderState::Rejected | OrderState::Expired,
                OrderState::Unknown
            )
    )
}

/// Deterministic fill model used exclusively for non-live replay/simulation.
#[derive(Clone, Debug)]
pub struct DeterministicFillModel {
    /// Full quoted bid/ask spread in basis points. A fill pays half the spread.
    pub spread_bps: Decimal,
    /// Slippage expressed in basis points, applied unfavourably.
    pub slippage_bps: Decimal,
    /// Exact flat fee per fill.
    pub flat_fee: Decimal,
    /// Number of complete market bars to wait before the first fill attempt.
    pub latency_bars: u32,
    /// Optional maximum quantity executable on each eligible market bar.
    pub max_fill_quantity: Option<Decimal>,
}

impl DeterministicFillModel {
    /// Validates fees, spread, and slippage before a simulation begins.
    pub fn validate(&self) -> Result<(), EngineError> {
        let ten_thousand = Decimal::from_integer(10_000)?;
        let two = Decimal::from_integer(2)?;
        let half_spread_bps = self.spread_bps.checked_div(two)?;
        let adverse_bps = self.slippage_bps.checked_add(half_spread_bps)?;
        if self.spread_bps < Decimal::ZERO
            || self.spread_bps >= ten_thousand
            || self.slippage_bps < Decimal::ZERO
            || self.slippage_bps >= ten_thousand
            || adverse_bps >= ten_thousand
            || self.flat_fee < Decimal::ZERO
            || self.latency_bars > 1_000_000
            || self
                .max_fill_quantity
                .is_some_and(|quantity| quantity <= Decimal::ZERO)
        {
            return Err(EngineError(
                "fill spread and slippage must be in [0, 10000) bps, their combined adverse adjustment must be below 10000 bps, fees cannot be negative, latency cannot exceed 1000000 bars, and an optional fill cap must be positive"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    /// Produces a fill if the current bar can satisfy the order model.
    pub fn fill(
        &self,
        order: &OmsOrder,
        bar: &Bar,
        replay_time: &str,
    ) -> Result<Option<Fill>, EngineError> {
        self.fill_quantity(
            order,
            bar,
            replay_time,
            order.intent.quantity,
            format!("exec-{}", order.intent.intent_id),
        )
    }

    fn fill_quantity(
        &self,
        order: &OmsOrder,
        bar: &Bar,
        replay_time: &str,
        quantity: Decimal,
        execution_id: String,
    ) -> Result<Option<Fill>, EngineError> {
        if quantity <= Decimal::ZERO || quantity > order.intent.quantity {
            return Err(EngineError(
                "simulated fill quantity must be positive and cannot exceed order quantity"
                    .to_owned(),
            ));
        }
        let base_price = match order.intent.order_type {
            OrderType::Market => bar.close,
            OrderType::Limit => {
                let limit = order
                    .intent
                    .limit_price
                    .expect("validated limit order has a price");
                match order.intent.side {
                    Side::Buy if bar.low <= limit => std::cmp::min(bar.close, limit),
                    Side::Sell if bar.high >= limit => std::cmp::max(bar.close, limit),
                    _ => return Ok(None),
                }
            }
        };
        let basis_points = Decimal::from_integer(10_000)?;
        let full_spread_divisor = Decimal::from_integer(20_000)?;
        let spread_adjustment = base_price
            .checked_mul(self.spread_bps)?
            .checked_div(full_spread_divisor)?;
        let slippage_adjustment = base_price
            .checked_mul(self.slippage_bps)?
            .checked_div(basis_points)?;
        let adverse_adjustment = spread_adjustment.checked_add(slippage_adjustment)?;
        let price = match order.intent.side {
            Side::Buy => base_price.checked_add(adverse_adjustment)?,
            Side::Sell => base_price.checked_sub(adverse_adjustment)?,
        };
        if price <= Decimal::ZERO {
            return Err(EngineError(
                "deterministic fill model produced a non-positive price".to_owned(),
            ));
        }
        if let Some(limit) = order.intent.limit_price {
            let violates_limit = match order.intent.side {
                Side::Buy => price > limit,
                Side::Sell => price < limit,
            };
            if violates_limit {
                return Ok(None);
            }
        }
        Ok(Some(Fill {
            execution_id,
            order_id: order.order_id.clone(),
            instrument_id: order.intent.instrument_id.clone(),
            side: order.intent.side,
            quantity,
            price,
            fee: self.flat_fee,
            executed_at: replay_time.to_owned(),
        }))
    }
}

/// Exact single-instrument portfolio projection for the first slice.
#[derive(Clone, Debug)]
pub struct Portfolio {
    account_id: String,
    instrument_id: String,
    quantity: Decimal,
    average_cost: Decimal,
    realized_pnl: Decimal,
}

impl Portfolio {
    /// Starts with no position and no realized P&L.
    pub fn new(account_id: impl Into<String>, instrument_id: impl Into<String>) -> Self {
        Self {
            account_id: account_id.into(),
            instrument_id: instrument_id.into(),
            quantity: Decimal::ZERO,
            average_cost: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
        }
    }

    /// Restores a persisted portfolio projection after validating its invariants.
    pub fn recover(
        account_id: impl Into<String>,
        instrument_id: impl Into<String>,
        quantity: Decimal,
        average_cost: Decimal,
        realized_pnl: Decimal,
    ) -> Result<Self, EngineError> {
        let account_id = account_id.into();
        let instrument_id = instrument_id.into();
        validate_canonical_id("portfolio account_id", &account_id)?;
        validate_canonical_id("portfolio instrument_id", &instrument_id)?;
        if quantity < Decimal::ZERO
            || average_cost < Decimal::ZERO
            || quantity == Decimal::ZERO && average_cost != Decimal::ZERO
        {
            return Err(EngineError("persisted portfolio is invalid".to_owned()));
        }
        Ok(Self {
            account_id,
            instrument_id,
            quantity,
            average_cost,
            realized_pnl,
        })
    }

    /// Applies a fill once to the internal ledger.
    pub fn apply_fill(&mut self, fill: &Fill) -> Result<(), EngineError> {
        if fill.instrument_id != self.instrument_id || fill.quantity <= Decimal::ZERO {
            return Err(EngineError(
                "fill does not match portfolio or has invalid quantity".to_owned(),
            ));
        }
        match fill.side {
            Side::Buy => {
                let prior_cost = self.average_cost.checked_mul(self.quantity)?;
                let fill_cost = fill
                    .price
                    .checked_mul(fill.quantity)?
                    .checked_add(fill.fee)?;
                let new_quantity = self.quantity.checked_add(fill.quantity)?;
                self.average_cost = prior_cost
                    .checked_add(fill_cost)?
                    .checked_div(new_quantity)?;
                self.quantity = new_quantity;
            }
            Side::Sell => {
                if fill.quantity > self.quantity {
                    return Err(EngineError(
                        "first slice does not permit short positions".to_owned(),
                    ));
                }
                let gross_realized = fill
                    .price
                    .checked_sub(self.average_cost)?
                    .checked_mul(fill.quantity)?;
                self.realized_pnl = self
                    .realized_pnl
                    .checked_add(gross_realized.checked_sub(fill.fee)?)?;
                self.quantity = self.quantity.checked_sub(fill.quantity)?;
                if self.quantity == Decimal::ZERO {
                    self.average_cost = Decimal::ZERO;
                }
            }
        }
        Ok(())
    }

    /// Returns a rebuildable position projection.
    pub fn position_snapshot(&self) -> PositionSnapshot {
        PositionSnapshot {
            account_id: self.account_id.clone(),
            instrument_id: self.instrument_id.clone(),
            quantity: self.quantity,
            average_cost: self.average_cost,
            realized_pnl: self.realized_pnl,
        }
    }

    /// Returns exact P&L at a supplied mark.
    pub fn pnl_snapshot(&self, mark_price: Decimal) -> Result<PnlSnapshot, EngineError> {
        let unrealized_pnl = mark_price
            .checked_sub(self.average_cost)?
            .checked_mul(self.quantity)?;
        Ok(PnlSnapshot {
            account_id: self.account_id.clone(),
            instrument_id: self.instrument_id.clone(),
            mark_price,
            realized_pnl: self.realized_pnl,
            unrealized_pnl,
            total_pnl: self.realized_pnl.checked_add(unrealized_pnl)?,
        })
    }
}

/// Result projection of one processed historical bar.
#[derive(Clone, Debug)]
pub struct ReplayResult {
    /// Events produced in causal append order.
    pub events: Vec<EventEnvelope>,
    /// Latest position projection when an execution occurred.
    pub position: Option<PositionSnapshot>,
    /// Latest P&L projection when an execution occurred.
    pub pnl: Option<PnlSnapshot>,
}

#[derive(Clone, Debug)]
struct SimulatedWorkingOrder {
    order: OmsOrder,
    remaining_quantity: Decimal,
    eligible_on_bar: u64,
    fill_sequence: u32,
    causation_id: String,
}

/// Orchestrates a single deterministic historical-bar workflow.
pub struct ReplayEngine {
    /// Logical replay time.
    pub clock: ReplayClock,
    /// Immutable engine version included in every event.
    pub software_version: String,
    /// Immutable configuration version included in every event.
    pub configuration_version: String,
    sequence: u64,
    bar_sequence: u64,
    policy: RiskPolicy,
    fill_model: DeterministicFillModel,
    portfolios: BTreeMap<(String, String), Portfolio>,
    working_orders: BTreeMap<String, SimulatedWorkingOrder>,
}

impl ReplayEngine {
    /// Creates a replay engine with explicit configuration and deterministic dependencies.
    pub fn new(
        initial_time: impl Into<String>,
        software_version: impl Into<String>,
        configuration_version: impl Into<String>,
        policy: RiskPolicy,
        fill_model: DeterministicFillModel,
    ) -> Result<Self, EngineError> {
        let software_version = software_version.into();
        let configuration_version = configuration_version.into();
        if software_version.is_empty() || configuration_version.is_empty() {
            return Err(EngineError(
                "engine and configuration versions are required".to_owned(),
            ));
        }
        policy.validate()?;
        fill_model.validate()?;
        Ok(Self {
            clock: ReplayClock::new(initial_time)?,
            software_version,
            configuration_version,
            sequence: 0,
            bar_sequence: 0,
            policy,
            fill_model,
            portfolios: BTreeMap::new(),
            working_orders: BTreeMap::new(),
        })
    }

    /// Processes one historical bar through strategy, risk, OMS, simulation, and portfolio.
    pub fn process_bar(
        &mut self,
        sink: &mut impl EventSink,
        strategy: &mut impl Strategy,
        account_id: &str,
        event_time: &str,
        bar: Bar,
    ) -> Result<ReplayResult, EngineError> {
        bar.validate()?;
        self.clock.advance_to(event_time)?;
        self.bar_sequence = self
            .bar_sequence
            .checked_add(1)
            .ok_or_else(|| EngineError("replay bar sequence overflow".to_owned()))?;
        let mut events = Vec::new();
        let mut latest_position = None;
        let mut latest_pnl = None;
        let market_correlation = format!("corr-market-{:012}", self.sequence + 1);
        let market_event = self.emit(
            sink,
            EventPayload::MarketBar(bar.clone()),
            event_time,
            &market_correlation,
            None,
            "market_data",
            "historical_import",
            None,
            None,
            Some(&bar.instrument_id),
        )?;
        let market_event_id = market_event.event_id.clone();
        events.push(market_event);

        let eligible_order_ids: Vec<_> = self
            .working_orders
            .iter()
            .filter(|(_, working)| {
                working.order.intent.account_id == account_id
                    && working.order.intent.instrument_id == bar.instrument_id
                    && working.eligible_on_bar <= self.bar_sequence
            })
            .map(|(order_id, _)| order_id.clone())
            .collect();
        for order_id in eligible_order_ids {
            let mut working = self
                .working_orders
                .remove(&order_id)
                .expect("eligible order came from working-order index");
            if let Some((position, pnl)) = self.attempt_simulated_fill(
                sink,
                &mut events,
                strategy,
                account_id,
                &bar,
                &mut working,
            )? {
                latest_position = Some(position);
                latest_pnl = Some(pnl);
            }
            if working.remaining_quantity > Decimal::ZERO {
                self.working_orders.insert(order_id, working);
            }
        }

        let Some(intent) = strategy.on_bar(&bar, self.clock.now())? else {
            return Ok(ReplayResult {
                events,
                position: latest_position,
                pnl: latest_pnl,
            });
        };
        intent.validate()?;
        if intent.account_id != account_id
            || intent.configuration_version != self.configuration_version
        {
            return Err(EngineError(
                "strategy intent does not match replay account or configuration".to_owned(),
            ));
        }
        if self
            .working_orders
            .contains_key(&format!("order-{}", intent.intent_id))
        {
            return Err(EngineError(
                "simulator rejected a duplicate working-order identity".to_owned(),
            ));
        }

        let current_time = self.clock.now().to_owned();
        let intent_event = self.emit(
            sink,
            EventPayload::OrderIntent(intent.clone()),
            &current_time,
            &intent.correlation_id,
            Some(&market_event_id),
            &intent.strategy_id,
            "strategy_worker",
            Some(account_id),
            Some(&intent.strategy_id),
            Some(&intent.instrument_id),
        )?;
        let intent_event_id = intent_event.event_id.clone();
        events.push(intent_event);

        let decision = self.policy.evaluate(&intent, &bar, self.clock.now())?;
        let current_time = self.clock.now().to_owned();
        let decision_event = self.emit(
            sink,
            EventPayload::RiskDecision(decision.clone()),
            &current_time,
            &intent.correlation_id,
            Some(&intent_event_id),
            "risk_engine",
            "trading_core",
            Some(account_id),
            Some(&intent.strategy_id),
            Some(&intent.instrument_id),
        )?;
        let decision_event_id = decision_event.event_id.clone();
        events.push(decision_event);

        if !decision.approved {
            let audit = self.audit_event(
                sink,
                &intent,
                &decision_event_id,
                events.iter().map(|event| event.event_id.clone()).collect(),
                "intent was rejected before OMS order creation",
            )?;
            events.push(audit);
            return Ok(ReplayResult {
                events,
                position: latest_position,
                pnl: latest_pnl,
            });
        }

        let mut order = OmsOrder::from_approved_intent(intent.clone(), &decision)?;
        let created = OrderStateChange {
            order_id: order.order_id.clone(),
            previous_state: None,
            new_state: OrderState::Created,
            reason: "created from auditable risk approval".to_owned(),
        };
        self.emit_order_change(sink, &mut events, &intent, &decision_event_id, created)?;
        for (state, reason) in [
            (OrderState::Approved, "risk approval accepted by OMS"),
            (
                OrderState::PendingSubmit,
                "queued for deterministic simulator",
            ),
            (OrderState::Submitted, "simulator submission accepted"),
            (
                OrderState::Acknowledged,
                "simulator acknowledged submission",
            ),
        ] {
            let change = order.transition(state, reason)?;
            self.emit_order_change(sink, &mut events, &intent, &decision_event_id, change)?;
        }
        let eligible_on_bar = self
            .bar_sequence
            .checked_add(u64::from(self.fill_model.latency_bars))
            .ok_or_else(|| EngineError("simulated order latency overflow".to_owned()))?;
        let mut working = SimulatedWorkingOrder {
            remaining_quantity: intent.quantity,
            eligible_on_bar,
            fill_sequence: 0,
            causation_id: events
                .last()
                .expect("acknowledgement event was emitted")
                .event_id
                .clone(),
            order,
        };
        if eligible_on_bar <= self.bar_sequence {
            if let Some((position, pnl)) = self.attempt_simulated_fill(
                sink,
                &mut events,
                strategy,
                account_id,
                &bar,
                &mut working,
            )? {
                latest_position = Some(position);
                latest_pnl = Some(pnl);
            }
        } else {
            let audit = self.audit_event(
                sink,
                &intent,
                &working.causation_id,
                events.iter().map(|event| event.event_id.clone()).collect(),
                "approved order remains acknowledged until its deterministic latency elapses",
            )?;
            working.causation_id = audit.event_id.clone();
            events.push(audit);
        }
        if working.remaining_quantity > Decimal::ZERO {
            self.working_orders
                .insert(working.order.order_id.clone(), working);
        }
        Ok(ReplayResult {
            events,
            position: latest_position,
            pnl: latest_pnl,
        })
    }

    fn attempt_simulated_fill(
        &mut self,
        sink: &mut impl EventSink,
        events: &mut Vec<EventEnvelope>,
        strategy: &mut impl Strategy,
        account_id: &str,
        bar: &Bar,
        working: &mut SimulatedWorkingOrder,
    ) -> Result<Option<(PositionSnapshot, PnlSnapshot)>, EngineError> {
        let intent = working.order.intent.clone();
        let quantity = self
            .fill_model
            .max_fill_quantity
            .map_or(working.remaining_quantity, |limit| {
                std::cmp::min(working.remaining_quantity, limit)
            });
        let next_fill_sequence = working
            .fill_sequence
            .checked_add(1)
            .ok_or_else(|| EngineError("simulated fill sequence overflow".to_owned()))?;
        let is_single_full_fill = working.fill_sequence == 0
            && quantity == working.order.intent.quantity
            && quantity == working.remaining_quantity;
        let execution_id = if is_single_full_fill {
            format!("exec-{}", intent.intent_id)
        } else {
            format!("exec-{}-{next_fill_sequence:06}", intent.intent_id)
        };
        let Some(fill) = self.fill_model.fill_quantity(
            &working.order,
            bar,
            self.clock.now(),
            quantity,
            execution_id,
        )?
        else {
            let audit = self.audit_event(
                sink,
                &intent,
                &working.causation_id,
                events.iter().map(|event| event.event_id.clone()).collect(),
                "eligible working limit order remained unfilled on the current bar",
            )?;
            working.causation_id = audit.event_id.clone();
            events.push(audit);
            return Ok(None);
        };

        let remaining_after_fill = working.remaining_quantity.checked_sub(fill.quantity)?;
        let target_state = if remaining_after_fill == Decimal::ZERO {
            Some(OrderState::Filled)
        } else if working.order.state == OrderState::Acknowledged {
            Some(OrderState::PartiallyFilled)
        } else {
            None
        };
        if let Some(state) = target_state {
            let reason = if state == OrderState::Filled {
                "deterministic simulator cumulatively filled order"
            } else {
                "deterministic simulator partially filled order"
            };
            let change = working.order.transition(state, reason)?;
            self.emit_order_change(sink, events, &intent, &working.causation_id, change)?;
            working.causation_id = events
                .last()
                .expect("order state event was emitted")
                .event_id
                .clone();
        }

        let current_time = self.clock.now().to_owned();
        let fill_event = self.emit(
            sink,
            EventPayload::Fill(fill.clone()),
            &current_time,
            &intent.correlation_id,
            Some(&working.causation_id),
            "simulator",
            "simulator",
            Some(account_id),
            Some(&intent.strategy_id),
            Some(&intent.instrument_id),
        )?;
        let fill_event_id = fill_event.event_id.clone();
        events.push(fill_event);
        working.remaining_quantity = remaining_after_fill;
        working.fill_sequence = next_fill_sequence;

        let (position, pnl) = {
            let portfolio = self
                .portfolios
                .entry((account_id.to_owned(), intent.instrument_id.clone()))
                .or_insert_with(|| Portfolio::new(account_id, &intent.instrument_id));
            portfolio.apply_fill(&fill)?;
            (
                portfolio.position_snapshot(),
                portfolio.pnl_snapshot(bar.close)?,
            )
        };
        strategy.on_execution(&fill, &position)?;
        let current_time = self.clock.now().to_owned();
        let position_event = self.emit(
            sink,
            EventPayload::Position(position.clone()),
            &current_time,
            &intent.correlation_id,
            Some(&fill_event_id),
            "portfolio_engine",
            "trading_core",
            Some(account_id),
            Some(&intent.strategy_id),
            Some(&intent.instrument_id),
        )?;
        let position_event_id = position_event.event_id.clone();
        events.push(position_event);
        let current_time = self.clock.now().to_owned();
        let pnl_event = self.emit(
            sink,
            EventPayload::Pnl(pnl.clone()),
            &current_time,
            &intent.correlation_id,
            Some(&position_event_id),
            "portfolio_engine",
            "trading_core",
            Some(account_id),
            Some(&intent.strategy_id),
            Some(&intent.instrument_id),
        )?;
        let pnl_event_id = pnl_event.event_id.clone();
        events.push(pnl_event);
        let note = if remaining_after_fill == Decimal::ZERO {
            "source bar through cumulative simulated fill and exact portfolio update"
        } else {
            "source bar through partial simulated fill and exact portfolio update"
        };
        let audit = self.audit_event(
            sink,
            &intent,
            &pnl_event_id,
            events.iter().map(|event| event.event_id.clone()).collect(),
            note,
        )?;
        working.causation_id = audit.event_id.clone();
        events.push(audit);
        Ok(Some((position, pnl)))
    }

    /// Processes a bar only after its reference data and trading session resolve.
    pub fn process_bar_with_market_preconditions(
        &mut self,
        sink: &mut impl EventSink,
        strategy: &mut impl Strategy,
        account_id: &str,
        event_time: &str,
        bar: Bar,
        market: &MarketPreconditions<'_>,
    ) -> Result<ReplayResult, EngineError> {
        market.validate(&bar, event_time)?;
        self.process_bar(sink, strategy, account_id, event_time, bar)
    }

    fn emit_order_change(
        &mut self,
        sink: &mut impl EventSink,
        events: &mut Vec<EventEnvelope>,
        intent: &OrderIntent,
        causation_id: &str,
        change: OrderStateChange,
    ) -> Result<(), EngineError> {
        let current_time = self.clock.now().to_owned();
        let event = self.emit(
            sink,
            EventPayload::OrderState(change),
            &current_time,
            &intent.correlation_id,
            Some(causation_id),
            "oms",
            "trading_core",
            Some(&intent.account_id),
            Some(&intent.strategy_id),
            Some(&intent.instrument_id),
        )?;
        events.push(event);
        Ok(())
    }

    fn audit_event(
        &mut self,
        sink: &mut impl EventSink,
        intent: &OrderIntent,
        causation_id: &str,
        event_ids: Vec<String>,
        summary: &str,
    ) -> Result<EventEnvelope, EngineError> {
        let current_time = self.clock.now().to_owned();
        self.emit(
            sink,
            EventPayload::Audit(AuditTrail {
                correlation_id: intent.correlation_id.clone(),
                event_ids,
                summary: summary.to_owned(),
            }),
            &current_time,
            &intent.correlation_id,
            Some(causation_id),
            "audit",
            "trading_core",
            Some(&intent.account_id),
            Some(&intent.strategy_id),
            Some(&intent.instrument_id),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        sink: &mut impl EventSink,
        payload: EventPayload,
        event_time: &str,
        correlation_id: &str,
        causation_id: Option<&str>,
        actor: &str,
        source: &str,
        account_id: Option<&str>,
        strategy_id: Option<&str>,
        instrument_id: Option<&str>,
    ) -> Result<EventEnvelope, EngineError> {
        self.sequence += 1;
        let event = EventEnvelope {
            event_id: format!("evt-{:012}", self.sequence),
            event_type: payload.event_type().to_owned(),
            schema_version: 1,
            event_time: event_time.to_owned(),
            receive_time: self.clock.now().to_owned(),
            account_id: account_id.map(str::to_owned),
            strategy_id: strategy_id.map(str::to_owned),
            instrument_id: instrument_id.map(str::to_owned),
            correlation_id: correlation_id.to_owned(),
            causation_id: causation_id.map(str::to_owned),
            actor: actor.to_owned(),
            source: source.to_owned(),
            payload,
            software_version: self.software_version.clone(),
            configuration_version: self.configuration_version.clone(),
        };
        sink.append(&event)?;
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::str::FromStr;

    use follon_instrument::{
        AssetClass, Instrument, InstrumentVersion, StaticTradingCalendar, TradingHalt,
        TradingSession,
    };

    use super::*;

    fn bar() -> Bar {
        Bar {
            instrument_id: "inst.us_equity.spy".to_owned(),
            open: Decimal::from_str("100.00").unwrap(),
            high: Decimal::from_str("101.00").unwrap(),
            low: Decimal::from_str("99.00").unwrap(),
            close: Decimal::from_str("100.00").unwrap(),
            volume: Decimal::from_str("1000").unwrap(),
            interval_seconds: 60,
            exchange_timezone: "America/New_York".to_owned(),
        }
    }

    fn engine() -> ReplayEngine {
        ReplayEngine::new(
            "2026-01-02T14:30:00Z",
            "core-0.1.0",
            "cfg-example-1",
            RiskPolicy {
                version: "risk-example-1".to_owned(),
                global_kill_switch: false,
                max_quantity: Decimal::from_integer(10).unwrap(),
                max_notional: Decimal::from_integer(10_000).unwrap(),
                max_price_deviation_bps: Decimal::from_integer(500).unwrap(),
            },
            DeterministicFillModel {
                spread_bps: Decimal::ZERO,
                slippage_bps: Decimal::from_str("0").unwrap(),
                flat_fee: Decimal::from_str("0.10").unwrap(),
                latency_bars: 0,
                max_fill_quantity: None,
            },
        )
        .unwrap()
    }

    fn simulated_order(
        side: Side,
        order_type: OrderType,
        limit_price: Option<Decimal>,
    ) -> OmsOrder {
        OmsOrder {
            order_id: "order-intent-fill-model-001".to_owned(),
            intent: OrderIntent {
                intent_id: "intent-fill-model-001".to_owned(),
                account_id: "acct-paper-001".to_owned(),
                strategy_id: "strategy-fill-model-001".to_owned(),
                instrument_id: "inst.us_equity.spy".to_owned(),
                correlation_id: "corr-fill-model-001".to_owned(),
                side,
                quantity: Decimal::from_integer(1).unwrap(),
                order_type,
                limit_price,
                time_in_force: TimeInForce::Day,
                rationale: "deterministic execution-model test".to_owned(),
                created_at: "2026-01-02T14:31:00Z".to_owned(),
                strategy_version: "strategy-fill-model-v1".to_owned(),
                configuration_version: "cfg-fill-model-v1".to_owned(),
                environment: "SIMULATION".to_owned(),
            },
            state: OrderState::Acknowledged,
        }
    }

    fn strategy() -> BuyOnceStrategy {
        BuyOnceStrategy::new(
            "acct-paper-001",
            "strategy-example-001",
            "strategy-example-v1",
            "cfg-example-1",
            Decimal::from_integer(100).unwrap(),
        )
    }

    fn market_dependencies() -> (InstrumentRegistry, StaticTradingCalendar) {
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
        instruments
            .register(InstrumentVersion {
                instrument: Instrument {
                    instrument_id: "inst.us_equity.spy".to_owned(),
                    symbol: "SPY".to_owned(),
                    exchange_symbol: "SPY".to_owned(),
                    asset_class: AssetClass::Etf,
                    venue: "venue.nyse_arca".to_owned(),
                    currency: "USD".to_owned(),
                    broker_ids: BTreeMap::new(),
                    tick_size: Decimal::from_str("0.01").unwrap(),
                    lot_size: Decimal::from_integer(1).unwrap(),
                    multiplier: Decimal::from_integer(1).unwrap(),
                    trading_calendar_id: "cal.us_equities.nyse".to_owned(),
                },
                effective_from: "2026-01-01T00:00:00Z".to_owned(),
                effective_to: None,
                reference_version: "reference-test-1".to_owned(),
            })
            .unwrap();
        (instruments, calendar)
    }

    #[test]
    fn importer_validates_and_normalizes_a_historical_bar() {
        let imported = import_historical_bars(
            "event_time,instrument_id,open,high,low,close,volume,interval_seconds,exchange_timezone\n\
             2026-01-02T14:31:00Z,inst.us_equity.spy,100,101,99,100,1000,60,America/New_York\n",
        )
        .unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].bar.close, Decimal::from_integer(100).unwrap());
    }

    #[test]
    fn persisted_market_events_can_be_replayed_with_identical_output() {
        let path = std::env::temp_dir().join(format!(
            "follon-replay-{}-{}.ndjson",
            std::process::id(),
            "persisted-market-events"
        ));
        let _ = std::fs::remove_file(&path);

        let mut source_engine = engine();
        let mut source_strategy = strategy();
        let mut file_store = FileEventStore::open(&path).unwrap();
        let source = source_engine
            .process_bar(
                &mut file_store,
                &mut source_strategy,
                "acct-paper-001",
                "2026-01-02T14:31:00Z",
                bar(),
            )
            .unwrap();
        drop(file_store);

        let persisted_input = load_persisted_market_bars(&path).unwrap();
        assert_eq!(persisted_input.len(), 1);
        let mut replay_engine = engine();
        let mut replay_strategy = strategy();
        let mut replay_store = InMemoryEventStore::default();
        let replay = replay_engine
            .process_bar(
                &mut replay_store,
                &mut replay_strategy,
                "acct-paper-001",
                &persisted_input[0].event_time,
                persisted_input[0].bar.clone(),
            )
            .unwrap();
        let source_json: Vec<_> = source
            .events
            .iter()
            .map(EventEnvelope::canonical_json)
            .collect();
        let replay_json: Vec<_> = replay
            .events
            .iter()
            .map(EventEnvelope::canonical_json)
            .collect();
        assert_eq!(source_json, replay_json);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn market_preconditions_block_bars_outside_an_explicit_session() {
        let (instruments, calendar) = market_dependencies();
        let market = MarketPreconditions {
            instruments: &instruments,
            calendar: &calendar,
        };
        let mut replay = engine();
        let mut store = InMemoryEventStore::default();
        let mut example_strategy = strategy();
        assert!(replay
            .process_bar_with_market_preconditions(
                &mut store,
                &mut example_strategy,
                "acct-paper-001",
                "2026-01-02T21:00:00Z",
                bar(),
                &market,
            )
            .is_err());
    }

    #[test]
    fn market_preconditions_block_halted_bars_before_strategy_evaluation() {
        let (instruments, _) = market_dependencies();
        let calendar = StaticTradingCalendar::new_with_halts(
            "cal.us_equities.nyse",
            vec![TradingSession {
                exchange_date: "2026-01-02".to_owned(),
                opens_at: "2026-01-02T14:30:00Z".to_owned(),
                closes_at: "2026-01-02T21:00:00Z".to_owned(),
            }],
            vec![TradingHalt {
                halt_id: "halt.spy.001".to_owned(),
                instrument_id: Some("inst.us_equity.spy".to_owned()),
                starts_at: "2026-01-02T14:31:00Z".to_owned(),
                ends_at: "2026-01-02T14:32:00Z".to_owned(),
                reason: "test suspension".to_owned(),
            }],
        )
        .unwrap();
        let market = MarketPreconditions {
            instruments: &instruments,
            calendar: &calendar,
        };
        let mut replay = engine();
        let mut store = InMemoryEventStore::default();
        let mut example_strategy = strategy();

        assert!(replay
            .process_bar_with_market_preconditions(
                &mut store,
                &mut example_strategy,
                "acct-paper-001",
                "2026-01-02T14:31:00Z",
                bar(),
                &market,
            )
            .is_err());
        let resumed = replay
            .process_bar_with_market_preconditions(
                &mut store,
                &mut example_strategy,
                "acct-paper-001",
                "2026-01-02T14:32:00Z",
                bar(),
                &market,
            )
            .unwrap();
        assert!(resumed.position.is_some());
    }

    #[test]
    fn identical_inputs_produce_identical_canonical_events() {
        let mut first_engine = engine();
        let mut first_store = InMemoryEventStore::default();
        let first = first_engine
            .process_bar(
                &mut first_store,
                &mut strategy(),
                "acct-paper-001",
                "2026-01-02T14:31:00Z",
                bar(),
            )
            .unwrap();
        let mut second_engine = engine();
        let mut second_store = InMemoryEventStore::default();
        let second = second_engine
            .process_bar(
                &mut second_store,
                &mut strategy(),
                "acct-paper-001",
                "2026-01-02T14:31:00Z",
                bar(),
            )
            .unwrap();
        let first_json: Vec<_> = first
            .events
            .iter()
            .map(EventEnvelope::canonical_json)
            .collect();
        let second_json: Vec<_> = second
            .events
            .iter()
            .map(EventEnvelope::canonical_json)
            .collect();
        assert_eq!(first_json, second_json);
        assert!(first
            .events
            .iter()
            .any(|event| event.event_type == "risk.decision.v1"));
        assert!(first
            .events
            .iter()
            .any(|event| event.event_type == "audit.trail.v1"));
    }

    #[test]
    fn risk_rejection_cannot_create_an_oms_order() {
        let mut replay = engine();
        replay.policy.global_kill_switch = true;
        let mut store = InMemoryEventStore::default();
        let result = replay
            .process_bar(
                &mut store,
                &mut strategy(),
                "acct-paper-001",
                "2026-01-02T14:31:00Z",
                bar(),
            )
            .unwrap();
        assert!(result
            .events
            .iter()
            .any(|event| event.event_type == "risk.decision.v1"));
        assert!(!result
            .events
            .iter()
            .any(|event| event.event_type == "order.state_changed.v1"));
        assert!(result
            .events
            .iter()
            .any(|event| event.event_type == "audit.trail.v1"));
    }

    #[test]
    fn risk_price_collar_rejects_a_limit_far_from_the_reference_mark() {
        let policy = RiskPolicy {
            version: "risk-price-collar-v1".to_owned(),
            global_kill_switch: false,
            max_quantity: Decimal::from_integer(10).unwrap(),
            max_notional: Decimal::from_integer(10_000).unwrap(),
            max_price_deviation_bps: Decimal::from_integer(100).unwrap(),
        };
        let order = simulated_order(
            Side::Buy,
            OrderType::Limit,
            Some(Decimal::from_integer(105).unwrap()),
        );
        let decision = policy
            .evaluate(&order.intent, &bar(), "2026-01-02T14:31:00Z")
            .unwrap();
        assert!(!decision.approved);
        assert!(decision
            .reason_codes
            .contains(&"PRICE_COLLAR_EXCEEDED".to_owned()));
        assert!(decision
            .evaluated_limits
            .contains("requested_price_deviation_bps=500.00000000"));
    }

    #[test]
    fn deterministic_fill_model_applies_half_spread_and_slippage_unfavourably() {
        let model = DeterministicFillModel {
            spread_bps: Decimal::from_str("20").unwrap(),
            slippage_bps: Decimal::from_str("5").unwrap(),
            flat_fee: Decimal::from_str("0.10").unwrap(),
            latency_bars: 0,
            max_fill_quantity: None,
        };
        model.validate().unwrap();

        let buy = model
            .fill(
                &simulated_order(Side::Buy, OrderType::Market, None),
                &bar(),
                "2026-01-02T14:31:00Z",
            )
            .unwrap()
            .unwrap();
        let sell = model
            .fill(
                &simulated_order(Side::Sell, OrderType::Market, None),
                &bar(),
                "2026-01-02T14:31:00Z",
            )
            .unwrap()
            .unwrap();

        assert_eq!(buy.price, Decimal::from_str("100.15").unwrap());
        assert_eq!(sell.price, Decimal::from_str("99.85").unwrap());
    }

    #[test]
    fn deterministic_fill_model_never_executes_beyond_a_limit() {
        let model = DeterministicFillModel {
            spread_bps: Decimal::from_str("20").unwrap(),
            slippage_bps: Decimal::from_str("5").unwrap(),
            flat_fee: Decimal::ZERO,
            latency_bars: 0,
            max_fill_quantity: None,
        };

        let blocked = model
            .fill(
                &simulated_order(
                    Side::Buy,
                    OrderType::Limit,
                    Some(Decimal::from_integer(100).unwrap()),
                ),
                &bar(),
                "2026-01-02T14:31:00Z",
            )
            .unwrap();
        let marketable = model
            .fill(
                &simulated_order(
                    Side::Buy,
                    OrderType::Limit,
                    Some(Decimal::from_str("100.20").unwrap()),
                ),
                &bar(),
                "2026-01-02T14:31:00Z",
            )
            .unwrap()
            .unwrap();

        assert!(blocked.is_none());
        assert_eq!(marketable.price, Decimal::from_str("100.15").unwrap());
    }

    #[test]
    fn deterministic_fill_model_rejects_invalid_execution_costs() {
        let negative_spread = DeterministicFillModel {
            spread_bps: Decimal::from_str("-0.01").unwrap(),
            slippage_bps: Decimal::ZERO,
            flat_fee: Decimal::ZERO,
            latency_bars: 0,
            max_fill_quantity: None,
        };
        let excessive_combined_cost = DeterministicFillModel {
            spread_bps: Decimal::from_integer(4).unwrap(),
            slippage_bps: Decimal::from_integer(9_999).unwrap(),
            flat_fee: Decimal::ZERO,
            latency_bars: 0,
            max_fill_quantity: None,
        };
        let zero_fill_cap = DeterministicFillModel {
            spread_bps: Decimal::ZERO,
            slippage_bps: Decimal::ZERO,
            flat_fee: Decimal::ZERO,
            latency_bars: 0,
            max_fill_quantity: Some(Decimal::ZERO),
        };

        assert!(negative_spread.validate().is_err());
        assert!(excessive_combined_cost.validate().is_err());
        assert!(zero_fill_cap.validate().is_err());
    }

    #[test]
    fn replay_engine_persists_latency_and_partial_fills_across_bars() {
        struct BuyThreeOnce {
            submitted: bool,
        }

        impl Strategy for BuyThreeOnce {
            fn on_bar(
                &mut self,
                bar: &Bar,
                replay_time: &str,
            ) -> Result<Option<OrderIntent>, EngineError> {
                if self.submitted {
                    return Ok(None);
                }
                self.submitted = true;
                Ok(Some(OrderIntent {
                    intent_id: "intent-partial-001".to_owned(),
                    account_id: "acct-paper-001".to_owned(),
                    strategy_id: "strategy-partial-001".to_owned(),
                    instrument_id: bar.instrument_id.clone(),
                    correlation_id: "corr-partial-001".to_owned(),
                    side: Side::Buy,
                    quantity: Decimal::from_integer(3)?,
                    order_type: OrderType::Market,
                    limit_price: None,
                    time_in_force: TimeInForce::Day,
                    rationale: "latency and partial-fill regression".to_owned(),
                    created_at: replay_time.to_owned(),
                    strategy_version: "strategy-partial-v1".to_owned(),
                    configuration_version: "cfg-example-1".to_owned(),
                    environment: "SIMULATION".to_owned(),
                }))
            }
        }

        let mut replay = engine();
        replay.fill_model.latency_bars = 1;
        replay.fill_model.max_fill_quantity = Some(Decimal::from_integer(1).unwrap());
        let mut store = InMemoryEventStore::default();
        let mut strategy = BuyThreeOnce { submitted: false };

        let submitted = replay
            .process_bar(
                &mut store,
                &mut strategy,
                "acct-paper-001",
                "2026-01-02T14:31:00Z",
                bar(),
            )
            .unwrap();
        assert!(submitted.position.is_none());
        assert!(!submitted
            .events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::Fill(_))));

        let first_partial = replay
            .process_bar(
                &mut store,
                &mut strategy,
                "acct-paper-001",
                "2026-01-02T14:32:00Z",
                bar(),
            )
            .unwrap();
        assert_eq!(
            first_partial.position.as_ref().unwrap().quantity,
            Decimal::from_integer(1).unwrap()
        );
        assert!(first_partial.events.iter().any(|event| matches!(
            &event.payload,
            EventPayload::OrderState(change)
                if change.new_state == OrderState::PartiallyFilled
        )));

        let second_partial = replay
            .process_bar(
                &mut store,
                &mut strategy,
                "acct-paper-001",
                "2026-01-02T14:33:00Z",
                bar(),
            )
            .unwrap();
        assert_eq!(
            second_partial.position.as_ref().unwrap().quantity,
            Decimal::from_integer(2).unwrap()
        );
        assert!(!second_partial.events.iter().any(|event| matches!(
            &event.payload,
            EventPayload::OrderState(change) if change.new_state == OrderState::Filled
        )));

        let completed = replay
            .process_bar(
                &mut store,
                &mut strategy,
                "acct-paper-001",
                "2026-01-02T14:34:00Z",
                bar(),
            )
            .unwrap();
        assert_eq!(
            completed.position.as_ref().unwrap().quantity,
            Decimal::from_integer(3).unwrap()
        );
        assert!(completed.events.iter().any(|event| matches!(
            &event.payload,
            EventPayload::OrderState(change) if change.new_state == OrderState::Filled
        )));
        assert!(replay.working_orders.is_empty());

        let execution_ids: Vec<_> = store
            .events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::Fill(fill) => Some(fill.execution_id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            execution_ids,
            vec![
                "exec-intent-partial-001-000001",
                "exec-intent-partial-001-000002",
                "exec-intent-partial-001-000003"
            ]
        );
    }

    #[test]
    fn invalid_oms_transition_is_rejected() {
        let intent = strategy()
            .on_bar(&bar(), "2026-01-02T14:31:00Z")
            .unwrap()
            .unwrap();
        let decision = engine()
            .policy
            .evaluate(&intent, &bar(), "2026-01-02T14:31:00Z")
            .unwrap();
        let mut order = OmsOrder::from_approved_intent(intent, &decision).unwrap();
        assert!(order
            .transition(OrderState::Filled, "skip submission")
            .is_err());
    }

    #[test]
    fn replay_clock_rejects_noncanonical_and_backward_time() {
        assert!(ReplayClock::new("2026-01-02T14:30:00+00:00").is_err());
        let mut clock = ReplayClock::new("2026-01-02T14:30:00Z").unwrap();
        assert!(clock.advance_to("2026-01-02T14:29:59Z").is_err());
        assert_eq!(clock.now(), "2026-01-02T14:30:00Z");
    }

    #[test]
    fn historical_bar_import_rejects_duplicate_and_out_of_order_rows() {
        let header = "event_time,instrument_id,open,high,low,close,volume,interval_seconds,exchange_timezone";
        let first = "2026-01-02T14:31:00Z,inst.us_equity.spy,100,101,99,100,10,60,America/New_York";
        let earlier =
            "2026-01-02T14:30:00Z,inst.us_equity.spy,100,101,99,100,10,60,America/New_York";
        assert!(import_historical_bars(&format!("{header}\n{first}\n{first}\n")).is_err());
        assert!(import_historical_bars(&format!("{header}\n{first}\n{earlier}\n")).is_err());
    }

    #[test]
    fn worker_boundary_rejects_unknown_fields_and_noncanonical_hashes() {
        let mut intent = serde_json::json!({
            "account_id": "acct.paper.001",
            "configuration_version": "cfg-v1",
            "correlation_id": "corr-worker-001",
            "created_at": "2026-01-02T14:31:00Z",
            "environment": "SIMULATION",
            "instrument_id": "inst.us_equity.spy",
            "intent_id": "intent-worker-001",
            "limit_price": null,
            "order_type": "MARKET",
            "quantity": "1",
            "rationale": "worker boundary test",
            "side": "BUY",
            "strategy_id": "strategy-worker-001",
            "strategy_version": "v1",
            "time_in_force": "DAY"
        })
        .as_object()
        .unwrap()
        .clone();
        assert!(parse_worker_intent(&intent).is_ok());
        intent.insert("unexpected".to_owned(), serde_json::Value::Bool(true));
        assert!(parse_worker_intent(&intent).is_err());

        let identity = StrategyWorkerIdentity {
            account_id: "acct.paper.001".to_owned(),
            strategy_id: "strategy-worker-001".to_owned(),
            strategy_version: "v1".to_owned(),
            configuration_version: "cfg-v1".to_owned(),
            strategy_bundle_hash: "A".repeat(64),
            environment: "SIMULATION".to_owned(),
        };
        assert!(identity.validate().is_err());
    }

    #[test]
    fn enriched_worker_services_are_point_in_time_bound_and_state_fingerprinted() {
        let replay_time = "2026-01-02T14:31:00Z";
        let mut services = WorkerRuntimeServices::new(StrategyWorkerServicesConfig {
            currency: "USD".to_owned(),
            initial_cash: Decimal::from_integer(1_000).unwrap(),
        })
        .unwrap();
        services.observe_bar(&bar(), replay_time).unwrap();

        let values = serde_json::json!({"phase": 1}).as_object().unwrap().clone();
        let fingerprint = json_fingerprint(&values).unwrap();
        services
            .store_output(
                &serde_json::json!({"fingerprint": fingerprint, "values": values}),
                &serde_json::json!([{
                    "name": "strategy.signal",
                    "value": "1.25",
                    "observed_at": replay_time,
                    "tags": [{"key": "regime", "value": "baseline"}],
                }]),
                replay_time,
            )
            .unwrap();

        let fill = Fill {
            execution_id: "exec.worker.001".to_owned(),
            order_id: "order.worker.001".to_owned(),
            instrument_id: "inst.us_equity.spy".to_owned(),
            side: Side::Buy,
            quantity: Decimal::from_integer(1).unwrap(),
            price: Decimal::from_integer(100).unwrap(),
            fee: Decimal::from_str("0.10").unwrap(),
            executed_at: replay_time.to_owned(),
        };
        let position = PositionSnapshot {
            account_id: "acct.paper.001".to_owned(),
            instrument_id: fill.instrument_id.clone(),
            quantity: Decimal::from_integer(1).unwrap(),
            average_cost: Decimal::from_str("100.10").unwrap(),
            realized_pnl: Decimal::ZERO,
        };
        services.apply_execution(&fill, &position).unwrap();

        let payload = services.service_payload(replay_time);
        assert_eq!(payload["history"]["records"].as_array().unwrap().len(), 1);
        assert_eq!(
            payload["portfolio"]["cash_by_currency"][0]["amount"],
            "899.90000000"
        );
        assert_eq!(
            payload["portfolio"]["positions"][0]["average_cost"],
            "100.10000000"
        );
        assert_eq!(services.state_fingerprint, fingerprint);
        assert_eq!(services.latest_metrics[0].name, "strategy.signal");
    }

    #[test]
    fn enriched_worker_services_reject_tampered_state_and_lookahead_metrics() {
        let replay_time = "2026-01-02T14:31:00Z";
        let mut services = WorkerRuntimeServices::new(StrategyWorkerServicesConfig {
            currency: "USD".to_owned(),
            initial_cash: Decimal::from_integer(1).unwrap(),
        })
        .unwrap();
        let values = serde_json::json!({"phase": 1}).as_object().unwrap().clone();
        assert!(services
            .store_output(
                &serde_json::json!({"fingerprint": "0".repeat(64), "values": values}),
                &serde_json::json!([]),
                replay_time,
            )
            .is_err());
        let values = serde_json::Map::new();
        let fingerprint = json_fingerprint(&values).unwrap();
        assert!(services
            .store_output(
                &serde_json::json!({"fingerprint": fingerprint, "values": values}),
                &serde_json::json!([{
                    "name": "strategy.signal",
                    "value": "1",
                    "observed_at": "2026-01-02T14:31:01Z",
                    "tags": [],
                }]),
                replay_time,
            )
            .is_err());
    }

    #[test]
    fn replay_engine_accumulates_portfolio_state_across_fills() {
        struct BuyEveryBar {
            sequence: u32,
        }

        impl Strategy for BuyEveryBar {
            fn on_bar(
                &mut self,
                bar: &Bar,
                replay_time: &str,
            ) -> Result<Option<OrderIntent>, EngineError> {
                self.sequence += 1;
                Ok(Some(OrderIntent {
                    intent_id: format!("intent-cumulative-{:03}", self.sequence),
                    account_id: "acct-paper-001".to_owned(),
                    strategy_id: "strategy-cumulative-001".to_owned(),
                    instrument_id: bar.instrument_id.clone(),
                    correlation_id: format!("corr-cumulative-{:03}", self.sequence),
                    side: Side::Buy,
                    quantity: Decimal::from_integer(1)?,
                    order_type: OrderType::Market,
                    limit_price: None,
                    time_in_force: TimeInForce::Day,
                    rationale: "cumulative portfolio regression".to_owned(),
                    created_at: replay_time.to_owned(),
                    strategy_version: "v1".to_owned(),
                    configuration_version: "cfg-example-1".to_owned(),
                    environment: "SIMULATION".to_owned(),
                }))
            }
        }

        let mut replay = engine();
        let mut store = InMemoryEventStore::default();
        let mut strategy = BuyEveryBar { sequence: 0 };
        replay
            .process_bar(
                &mut store,
                &mut strategy,
                "acct-paper-001",
                "2026-01-02T14:30:00Z",
                bar(),
            )
            .unwrap();
        let second = replay
            .process_bar(
                &mut store,
                &mut strategy,
                "acct-paper-001",
                "2026-01-02T14:31:00Z",
                bar(),
            )
            .unwrap();
        assert_eq!(
            second.position.unwrap().quantity,
            Decimal::from_integer(2).unwrap()
        );
    }
}
