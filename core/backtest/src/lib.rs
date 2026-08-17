//! Reproducible backtest metadata, exact accounting, and portable reports.
//!
//! A completed backtest is a decision artifact only when its immutable inputs
//! and generated event stream fingerprint can be reconstructed.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::fs::{self, OpenOptions};
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};

use follon_control_plane::{
    EngineError, HistoricalBar, InMemoryEventStore, MarketPreconditions, ReplayEngine, Strategy,
};
use follon_domain::{
    validate_canonical_id, validate_utc_timestamp, Bar, Decimal, DecimalError, DomainError,
    EventPayload, Fill, Side,
};
use follon_market_data::CorporateAction;
use sha2::{Digest, Sha256};

/// Published cross-runtime provenance fingerprint contract version.
pub const BACKTEST_PROVENANCE_VERSION: u32 = 2;

/// Versioned, content-addressed normalized dataset supplied to a backtest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetManifest {
    /// Canonical immutable dataset identity.
    pub dataset_id: String,
    /// Immutable dataset version supplied by the importer pipeline.
    pub dataset_version: String,
    /// Effective reference-data revision used to resolve instruments.
    pub reference_data_version: String,
    /// Canonical universe definition identity.
    pub universe_id: String,
    /// UTC inclusive dataset start.
    pub starts_at: String,
    /// UTC inclusive dataset end.
    pub ends_at: String,
    /// SHA-256 digest of canonical normalized input rows.
    pub content_hash: String,
}

impl DatasetManifest {
    /// Creates a manifest whose content hash is stable for identical bar input.
    pub fn from_bars(
        dataset_id: impl Into<String>,
        dataset_version: impl Into<String>,
        reference_data_version: impl Into<String>,
        universe_id: impl Into<String>,
        bars: &[(String, Bar)],
    ) -> Result<Self, BacktestError> {
        Self::from_market_data(
            dataset_id,
            dataset_version,
            reference_data_version,
            universe_id,
            bars,
            &[],
        )
    }

    /// Creates a manifest from every normalized input that can affect a run.
    ///
    /// Corporate actions are deliberately included in the content address.
    /// Omitting them would permit two economically different simulations to
    /// claim the same dataset identity.
    pub fn from_market_data(
        dataset_id: impl Into<String>,
        dataset_version: impl Into<String>,
        reference_data_version: impl Into<String>,
        universe_id: impl Into<String>,
        bars: &[(String, Bar)],
        actions: &[CorporateAction],
    ) -> Result<Self, BacktestError> {
        let starts_at = bars
            .first()
            .map(|(time, _)| time.clone())
            .ok_or_else(|| BacktestError("dataset cannot be empty".to_owned()))?;
        let ends_at = bars
            .last()
            .map(|(time, _)| time.clone())
            .expect("checked above");
        let manifest = Self {
            dataset_id: dataset_id.into(),
            dataset_version: dataset_version.into(),
            reference_data_version: reference_data_version.into(),
            universe_id: universe_id.into(),
            starts_at,
            ends_at,
            content_hash: hash_market_data(bars, actions)?,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validates immutable identity, time range, and digest shape.
    pub fn validate(&self) -> Result<(), BacktestError> {
        for (name, value) in [
            ("dataset_id", self.dataset_id.as_str()),
            ("universe_id", self.universe_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        if self.dataset_version.is_empty()
            || self.reference_data_version.is_empty()
            || !is_utc(&self.starts_at)
            || !is_utc(&self.ends_at)
            || self.starts_at > self.ends_at
            || !is_sha256(&self.content_hash)
        {
            return Err(BacktestError("invalid dataset manifest".to_owned()));
        }
        Ok(())
    }

    /// Stable JSON representation embedded in every portable result artifact.
    pub fn canonical_json(&self) -> String {
        format!(
            "{{\"content_hash\":{},\"dataset_id\":{},\"dataset_version\":{},\"ends_at\":{},\"reference_data_version\":{},\"starts_at\":{},\"universe_id\":{}}}",
            json_string(&self.content_hash),
            json_string(&self.dataset_id),
            json_string(&self.dataset_version),
            json_string(&self.ends_at),
            json_string(&self.reference_data_version),
            json_string(&self.starts_at),
            json_string(&self.universe_id),
        )
    }
}

/// Complete immutable specification of one reproducible backtest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BacktestSpec {
    /// Hash of the exact strategy bundle that emitted intents.
    pub strategy_bundle_hash: String,
    /// Versioned normalized input data.
    pub dataset: DatasetManifest,
    /// Canonical configuration family identity.
    pub configuration_id: String,
    /// Immutable configuration version.
    pub configuration_version: String,
    /// SHA-256 of the canonical configuration document.
    pub configuration_hash: String,
    /// Explicit deterministic seed, even when the initial fill model has no randomness.
    pub seed: u64,
    /// Backtest engine version.
    pub engine_version: String,
    /// UTC inclusive requested run start.
    pub starts_at: String,
    /// UTC inclusive requested run end.
    pub ends_at: String,
}

impl BacktestSpec {
    /// Validates every input required for a reproducible decision artifact.
    pub fn validate(&self) -> Result<(), BacktestError> {
        self.dataset.validate()?;
        validate_canonical_id("configuration_id", &self.configuration_id)?;
        if !is_sha256(&self.strategy_bundle_hash)
            || self.configuration_version.is_empty()
            || !is_sha256(&self.configuration_hash)
            || self.engine_version.is_empty()
            || !is_utc(&self.starts_at)
            || !is_utc(&self.ends_at)
            || self.starts_at > self.ends_at
            || self.starts_at < self.dataset.starts_at
            || self.ends_at > self.dataset.ends_at
        {
            return Err(BacktestError(
                "invalid reproducibility specification".to_owned(),
            ));
        }
        Ok(())
    }

    /// Stable SHA-256 fingerprint of every declared run input.
    pub fn fingerprint(&self) -> Result<String, BacktestError> {
        self.validate()?;
        Ok(sha256(&format!(
            "provenance={}\nstrategy={}\ndataset_id={}\ndataset_version={}\ndataset_hash={}\nreference_data={}\nuniverse={}\nconfig_id={}\nconfig_version={}\nconfig_hash={}\nseed={}\nengine={}\nstarts={}\nends={}\n",
            BACKTEST_PROVENANCE_VERSION,
            self.strategy_bundle_hash,
            self.dataset.dataset_id,
            self.dataset.dataset_version,
            self.dataset.content_hash,
            self.dataset.reference_data_version,
            self.dataset.universe_id,
            self.configuration_id,
            self.configuration_version,
            self.configuration_hash,
            self.seed,
            self.engine_version,
            self.starts_at,
            self.ends_at,
        )))
    }

    /// Stable, self-describing JSON representation of every declared run input.
    pub fn canonical_json(&self) -> Result<String, BacktestError> {
        self.validate()?;
        Ok(format!(
            "{{\"configuration_hash\":{},\"configuration_id\":{},\"configuration_version\":{},\"dataset\":{},\"ends_at\":{},\"engine_version\":{},\"provenance_version\":{},\"seed\":{},\"starts_at\":{},\"strategy_bundle_hash\":{}}}",
            json_string(&self.configuration_hash),
            json_string(&self.configuration_id),
            json_string(&self.configuration_version),
            self.dataset.canonical_json(),
            json_string(&self.ends_at),
            json_string(&self.engine_version),
            BACKTEST_PROVENANCE_VERSION,
            self.seed,
            json_string(&self.starts_at),
            json_string(&self.strategy_bundle_hash),
        ))
    }
}

/// Exact position projection used by the backtest ledger.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerPosition {
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Signed quantity; first milestone does not permit shorts.
    pub quantity: Decimal,
    /// Exact average cost including buy fees.
    pub average_cost: Decimal,
    /// Exact realized P&L net of sell fees.
    pub realized_pnl: Decimal,
}

/// One immutable accounting movement derived from a fill or corporate action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountingEntry {
    /// Canonical, idempotent accounting identity.
    pub entry_id: String,
    /// UTC time at which the economic effect is recognized.
    pub occurred_at: String,
    /// Stable entry family: `FILL`, `SPLIT`, or `CASH_DIVIDEND`.
    pub entry_type: String,
    /// Canonical affected instrument.
    pub instrument_id: String,
    /// Exact change in held units.
    pub quantity_delta: Decimal,
    /// Exact change in cash in the report currency.
    pub cash_delta: Decimal,
}

/// Independent exact ledger for cash, fills, and corporate actions.
#[derive(Clone, Debug)]
pub struct BacktestLedger {
    currency: String,
    cash: Decimal,
    positions: BTreeMap<String, LedgerPosition>,
    execution_ids: BTreeSet<String>,
    corporate_action_ids: BTreeSet<String>,
    entries: Vec<AccountingEntry>,
    fees: Decimal,
    dividends: Decimal,
}

impl BacktestLedger {
    /// Creates a ledger with exact initial cash in one currency.
    pub fn new(currency: impl Into<String>, initial_cash: Decimal) -> Result<Self, BacktestError> {
        let currency = currency.into();
        if currency.len() != 3
            || !currency.bytes().all(|byte| byte.is_ascii_uppercase())
            || initial_cash < Decimal::ZERO
        {
            return Err(BacktestError("invalid opening ledger balance".to_owned()));
        }
        Ok(Self {
            currency,
            cash: initial_cash,
            positions: BTreeMap::new(),
            execution_ids: BTreeSet::new(),
            corporate_action_ids: BTreeSet::new(),
            entries: Vec::new(),
            fees: Decimal::ZERO,
            dividends: Decimal::ZERO,
        })
    }

    /// Applies a fill exactly once after the simulator/execution layer normalizes it.
    pub fn apply_fill(&mut self, fill: &Fill) -> Result<(), BacktestError> {
        if fill.quantity <= Decimal::ZERO || fill.price <= Decimal::ZERO || fill.fee < Decimal::ZERO
        {
            return Err(BacktestError("invalid fill for backtest ledger".to_owned()));
        }
        for (name, value) in [
            ("execution_id", fill.execution_id.as_str()),
            ("order_id", fill.order_id.as_str()),
            ("instrument_id", fill.instrument_id.as_str()),
        ] {
            validate_canonical_id(name, value)?;
        }
        if !is_utc(&fill.executed_at) {
            return Err(BacktestError("fill execution time must be UTC".to_owned()));
        }
        if self.execution_ids.contains(&fill.execution_id) {
            return Err(BacktestError(
                "duplicate execution in backtest ledger".to_owned(),
            ));
        }
        let position = self
            .positions
            .entry(fill.instrument_id.clone())
            .or_insert(LedgerPosition {
                instrument_id: fill.instrument_id.clone(),
                quantity: Decimal::ZERO,
                average_cost: Decimal::ZERO,
                realized_pnl: Decimal::ZERO,
            });
        let gross = fill.price.checked_mul(fill.quantity)?;
        let (quantity_delta, cash_delta) = match fill.side {
            Side::Buy => {
                let required_cash = gross.checked_add(fill.fee)?;
                if required_cash > self.cash {
                    return Err(BacktestError("insufficient simulated cash".to_owned()));
                }
                let prior_cost = position.average_cost.checked_mul(position.quantity)?;
                position.quantity = position.quantity.checked_add(fill.quantity)?;
                position.average_cost = prior_cost
                    .checked_add(required_cash)?
                    .checked_div(position.quantity)?;
                self.cash = self.cash.checked_sub(required_cash)?;
                (fill.quantity, Decimal::ZERO.checked_sub(required_cash)?)
            }
            Side::Sell => {
                if fill.quantity > position.quantity {
                    return Err(BacktestError(
                        "short positions are not enabled in the first milestone".to_owned(),
                    ));
                }
                let realized = fill
                    .price
                    .checked_sub(position.average_cost)?
                    .checked_mul(fill.quantity)?;
                position.realized_pnl = position
                    .realized_pnl
                    .checked_add(realized.checked_sub(fill.fee)?)?;
                position.quantity = position.quantity.checked_sub(fill.quantity)?;
                if position.quantity == Decimal::ZERO {
                    position.average_cost = Decimal::ZERO;
                }
                let proceeds = gross.checked_sub(fill.fee)?;
                self.cash = self.cash.checked_add(proceeds)?;
                (Decimal::ZERO.checked_sub(fill.quantity)?, proceeds)
            }
        };
        self.fees = self.fees.checked_add(fill.fee)?;
        self.execution_ids.insert(fill.execution_id.clone());
        self.entries.push(AccountingEntry {
            entry_id: format!("accounting-{}", fill.execution_id),
            occurred_at: fill.executed_at.clone(),
            entry_type: "FILL".to_owned(),
            instrument_id: fill.instrument_id.clone(),
            quantity_delta,
            cash_delta,
        });
        Ok(())
    }

    /// Applies split or cash-dividend economics to held positions.
    pub fn apply_corporate_action(
        &mut self,
        action: &CorporateAction,
    ) -> Result<(), BacktestError> {
        action.validate()?;
        if self.corporate_action_ids.contains(action.action_id()) {
            return Err(BacktestError(
                "duplicate corporate action in backtest ledger".to_owned(),
            ));
        }
        let (instrument_id, value, split) = match action {
            CorporateAction::Split {
                instrument_id,
                ratio,
                ..
            } => (instrument_id, *ratio, true),
            CorporateAction::CashDividend {
                instrument_id,
                amount,
                ..
            } => (instrument_id, *amount, false),
        };
        let Some(position) = self.positions.get_mut(instrument_id) else {
            self.corporate_action_ids
                .insert(action.action_id().to_owned());
            return Ok(());
        };
        let (entry_type, quantity_delta, cash_delta) = if split {
            let previous_quantity = position.quantity;
            position.quantity = position.quantity.checked_mul(value)?;
            position.average_cost = position.average_cost.checked_div(value)?;
            (
                "SPLIT",
                position.quantity.checked_sub(previous_quantity)?,
                Decimal::ZERO,
            )
        } else {
            let cash_delta = position.quantity.checked_mul(value)?;
            self.cash = self.cash.checked_add(cash_delta)?;
            self.dividends = self.dividends.checked_add(cash_delta)?;
            ("CASH_DIVIDEND", Decimal::ZERO, cash_delta)
        };
        self.corporate_action_ids
            .insert(action.action_id().to_owned());
        self.entries.push(AccountingEntry {
            entry_id: format!("accounting-{}", action.action_id()),
            occurred_at: action.effective_at().to_owned(),
            entry_type: entry_type.to_owned(),
            instrument_id: instrument_id.to_owned(),
            quantity_delta,
            cash_delta,
        });
        Ok(())
    }

    /// Returns immutable accounting movements in their application order.
    pub fn entries(&self) -> &[AccountingEntry] {
        &self.entries
    }

    /// Returns an exact immutable balance report.
    pub fn report(
        &self,
        marks: &BTreeMap<String, Decimal>,
    ) -> Result<BacktestReport, BacktestError> {
        let mut unrealized = Decimal::ZERO;
        let mut realized = Decimal::ZERO;
        let mut market_value = Decimal::ZERO;
        for position in self.positions.values() {
            let mark = marks.get(&position.instrument_id).ok_or_else(|| {
                BacktestError(format!("missing mark for {}", position.instrument_id))
            })?;
            unrealized = unrealized.checked_add(
                mark.checked_sub(position.average_cost)?
                    .checked_mul(position.quantity)?,
            )?;
            realized = realized.checked_add(position.realized_pnl)?;
            market_value = market_value.checked_add(mark.checked_mul(position.quantity)?)?;
        }
        Ok(BacktestReport {
            currency: self.currency.clone(),
            cash: self.cash,
            realized_pnl: realized,
            unrealized_pnl: unrealized,
            market_value,
            total_equity: self.cash.checked_add(market_value)?,
            positions: self.positions.values().cloned().collect(),
            total_fees: self.fees,
            total_dividends: self.dividends,
        })
    }
}

/// Portable report produced by a completed simulation run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BacktestReport {
    /// Currency of all exact values in this first single-currency report.
    pub currency: String,
    /// Available simulated cash.
    pub cash: Decimal,
    /// Accumulated exact realized P&L.
    pub realized_pnl: Decimal,
    /// Mark-to-market exact P&L.
    pub unrealized_pnl: Decimal,
    /// Exact marked value of open positions.
    pub market_value: Decimal,
    /// Cash plus current marked open-position value.
    pub total_equity: Decimal,
    /// Current positions in canonical-instrument order.
    pub positions: Vec<LedgerPosition>,
    /// Total fees charged by the simulated execution model.
    pub total_fees: Decimal,
    /// Total cash dividends recognized by the ledger.
    pub total_dividends: Decimal,
}

/// One exact valuation point in a completed backtest equity curve.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EquityPoint {
    /// Replay-clock instant of the valuation.
    pub event_time: String,
    /// Exact total equity after all economics at this instant.
    pub total_equity: Decimal,
}

/// Portable performance report derived entirely from the immutable ledger curve.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PerformanceReport {
    /// Exact opening cash selected by the immutable run input.
    pub starting_equity: Decimal,
    /// Exact ending equity from the final accounting report.
    pub ending_equity: Decimal,
    /// Ending equity minus starting equity.
    pub net_pnl: Decimal,
    /// Net return in exact basis points; zero when opening equity is zero.
    pub return_bps: Decimal,
    /// Largest peak-to-trough drawdown in exact basis points.
    pub max_drawdown_bps: Decimal,
    /// Count of canonical domain events included in the output identity.
    pub event_count: u64,
    /// Count of simulated fills posted to the ledger.
    pub trade_count: u64,
    /// Count of corporate actions applied during the selected period.
    pub corporate_action_count: u64,
    /// Ordered exact valuations used to derive all return metrics.
    pub equity_curve: Vec<EquityPoint>,
}

impl PerformanceReport {
    /// Derives exact performance metrics without using a wall clock or floats.
    pub fn from_equity_curve(
        starting_equity: Decimal,
        equity_curve: Vec<EquityPoint>,
        event_count: u64,
        trade_count: u64,
        corporate_action_count: u64,
    ) -> Result<Self, BacktestError> {
        let ending_equity = equity_curve
            .last()
            .map(|point| point.total_equity)
            .unwrap_or(starting_equity);
        let net_pnl = ending_equity.checked_sub(starting_equity)?;
        let basis_points = Decimal::from_integer(10_000)?;
        let return_bps = if starting_equity > Decimal::ZERO {
            net_pnl
                .checked_mul(basis_points)?
                .checked_div(starting_equity)?
        } else {
            Decimal::ZERO
        };
        let mut peak = starting_equity;
        let mut max_drawdown_bps = Decimal::ZERO;
        for point in &equity_curve {
            if point.total_equity > peak {
                peak = point.total_equity;
            }
            if peak > Decimal::ZERO && point.total_equity < peak {
                let drawdown = peak
                    .checked_sub(point.total_equity)?
                    .checked_mul(basis_points)?
                    .checked_div(peak)?;
                max_drawdown_bps = max_drawdown_bps.max(drawdown);
            }
        }
        Ok(Self {
            starting_equity,
            ending_equity,
            net_pnl,
            return_bps,
            max_drawdown_bps,
            event_count,
            trade_count,
            corporate_action_count,
            equity_curve,
        })
    }

    /// Stable JSON report consumable by non-Rust report renderers.
    pub fn canonical_json(&self) -> String {
        let curve = self
            .equity_curve
            .iter()
            .map(|point| {
                format!(
                    "{{\"event_time\":{},\"total_equity\":\"{}\"}}",
                    json_string(&point.event_time),
                    point.total_equity,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"corporate_action_count\":{},\"ending_equity\":\"{}\",\"equity_curve\":[{}],\"event_count\":{},\"max_drawdown_bps\":\"{}\",\"net_pnl\":\"{}\",\"return_bps\":\"{}\",\"starting_equity\":\"{}\",\"trade_count\":{}}}",
            self.corporate_action_count,
            self.ending_equity,
            curve,
            self.event_count,
            self.max_drawdown_bps,
            self.net_pnl,
            self.return_bps,
            self.starting_equity,
            self.trade_count,
        )
    }
}

/// Immutable record attached to a completed result; without it the result is exploratory only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BacktestArtifact {
    /// Complete immutable specification needed to reconstruct this result.
    pub specification: BacktestSpec,
    /// Fingerprint of all declared run inputs.
    pub specification_fingerprint: String,
    /// SHA-256 fingerprint of canonical output event lines in append order.
    pub event_output_hash: String,
    /// Exact report snapshot.
    pub report: BacktestReport,
    /// Immutable accounting entries supporting the report totals.
    pub accounting_entries: Vec<AccountingEntry>,
    /// Exact performance summary and equity curve.
    pub performance: PerformanceReport,
}

impl BacktestArtifact {
    /// Creates a reproducibility artifact from a validated spec and canonical output events.
    pub fn new(
        spec: &BacktestSpec,
        canonical_events: &[String],
        report: BacktestReport,
    ) -> Result<Self, BacktestError> {
        let performance = PerformanceReport::from_equity_curve(
            report.total_equity,
            Vec::new(),
            u64::try_from(canonical_events.len())
                .map_err(|_| BacktestError("too many canonical events".to_owned()))?,
            0,
            0,
        )?;
        Self::with_run_details(spec, canonical_events, report, Vec::new(), performance)
    }

    /// Creates an artifact containing every report input produced by a completed runner.
    pub fn with_run_details(
        spec: &BacktestSpec,
        canonical_events: &[String],
        report: BacktestReport,
        accounting_entries: Vec<AccountingEntry>,
        performance: PerformanceReport,
    ) -> Result<Self, BacktestError> {
        Ok(Self {
            specification: spec.clone(),
            specification_fingerprint: spec.fingerprint()?,
            event_output_hash: sha256(&canonical_events.join("\n")),
            report,
            accounting_entries,
            performance,
        })
    }

    /// Stable digest of the complete portable result, including its report.
    pub fn fingerprint(&self) -> String {
        sha256(&format!(
            "specification={}\nevents={}\nreport={}\nperformance={}\nentries={}\n",
            self.specification_fingerprint,
            self.event_output_hash,
            self.report.canonical_json(),
            self.performance.canonical_json(),
            accounting_entries_json(&self.accounting_entries),
        ))
    }

    /// Portable canonical JSON suitable for an immutable result artifact.
    pub fn canonical_json(&self) -> String {
        format!(
            "{{\"accounting_entries\":{},\"artifact_schema_version\":2,\"artifact_fingerprint\":\"{}\",\"event_output_hash\":\"{}\",\"performance\":{},\"report\":{},\"specification\":{},\"specification_fingerprint\":\"{}\"}}",
            accounting_entries_json(&self.accounting_entries),
            self.fingerprint(),
            self.event_output_hash,
            self.performance.canonical_json(),
            self.report.canonical_json(),
            self.specification
                .canonical_json()
                .expect("validated artifact specification remains valid"),
            self.specification_fingerprint,
        )
    }

    /// Renders a portable human-readable report without querying mutable state.
    pub fn markdown_report(&self) -> String {
        let mut report = format!(
            "# Follon Backtest Report\n\n\
             - Artifact fingerprint: `{}`\n\
             - Specification fingerprint: `{}`\n\
             - Configuration: `{}` / `{}` (`{}`)\n\
             - Event-output hash: `{}`\n\n\
             ## Performance\n\n\
             | Metric | Exact value |\n\
             | --- | ---: |\n\
             | Starting equity | {} {} |\n\
             | Ending equity | {} {} |\n\
             | Net P&L | {} {} |\n\
             | Return | {} bps |\n\
             | Maximum drawdown | {} bps |\n\
             | Simulated fills | {} |\n\
             | Applied corporate actions | {} |\n\
             | Fees | {} {} |\n\
             | Dividends | {} {} |\n\n\
             ## Positions\n\n\
             | Instrument | Quantity | Average cost | Realized P&L |\n\
             | --- | ---: | ---: | ---: |\n",
            self.fingerprint(),
            self.specification_fingerprint,
            self.specification.configuration_id,
            self.specification.configuration_version,
            self.specification.configuration_hash,
            self.event_output_hash,
            self.performance.starting_equity,
            self.report.currency,
            self.performance.ending_equity,
            self.report.currency,
            self.performance.net_pnl,
            self.report.currency,
            self.performance.return_bps,
            self.performance.max_drawdown_bps,
            self.performance.trade_count,
            self.performance.corporate_action_count,
            self.report.total_fees,
            self.report.currency,
            self.report.total_dividends,
            self.report.currency,
        );
        for position in &self.report.positions {
            writeln!(
                report,
                "| {} | {} | {} | {} |",
                position.instrument_id,
                position.quantity,
                position.average_cost,
                position.realized_pnl,
            )
            .expect("writing to a string cannot fail");
        }
        report.push_str("\n## Accounting entries\n\n");
        report.push_str("| Time | Type | Instrument | Quantity change | Cash change |\n");
        report.push_str("| --- | --- | --- | ---: | ---: |\n");
        for entry in &self.accounting_entries {
            writeln!(
                report,
                "| {} | {} | {} | {} | {} |",
                entry.occurred_at,
                entry.entry_type,
                entry.instrument_id,
                entry.quantity_delta,
                entry.cash_delta,
            )
            .expect("writing to a string cannot fail");
        }
        report
    }
}

impl BacktestReport {
    /// Portable canonical JSON using strings for every exact monetary value.
    pub fn canonical_json(&self) -> String {
        let positions = self
            .positions
            .iter()
            .map(|position| {
                format!(
                    "{{\"average_cost\":\"{}\",\"instrument_id\":{},\"quantity\":\"{}\",\"realized_pnl\":\"{}\"}}",
                    position.average_cost,
                    json_string(&position.instrument_id),
                    position.quantity,
                    position.realized_pnl,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"cash\":\"{}\",\"currency\":{},\"market_value\":\"{}\",\"positions\":[{}],\"realized_pnl\":\"{}\",\"total_dividends\":\"{}\",\"total_equity\":\"{}\",\"total_fees\":\"{}\",\"unrealized_pnl\":\"{}\"}}",
            self.cash,
            json_string(&self.currency),
            self.market_value,
            positions,
            self.realized_pnl,
            self.total_dividends,
            self.total_equity,
            self.total_fees,
            self.unrealized_pnl,
        )
    }
}

/// Immutable, queryable identity of a completed reproducible experiment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExperimentRecord {
    /// Canonical experiment family identity selected by the researcher.
    pub experiment_id: String,
    /// Canonical immutable run identity within that experiment.
    pub run_id: String,
    /// User-provided deterministic labels, ordered by key for portable export.
    pub tags: BTreeMap<String, String>,
    /// Immutable input identity copied from the completed artifact.
    pub specification_fingerprint: String,
    /// Immutable event-output identity copied from the completed artifact.
    pub event_output_hash: String,
    /// Immutable whole-artifact identity, including the exact report snapshot.
    pub artifact_fingerprint: String,
}

impl ExperimentRecord {
    /// Builds result metadata without consulting a machine clock or mutable state.
    pub fn from_artifact(
        experiment_id: impl Into<String>,
        run_id: impl Into<String>,
        tags: BTreeMap<String, String>,
        artifact: &BacktestArtifact,
    ) -> Result<Self, BacktestError> {
        let record = Self {
            experiment_id: experiment_id.into(),
            run_id: run_id.into(),
            tags,
            specification_fingerprint: artifact.specification_fingerprint.clone(),
            event_output_hash: artifact.event_output_hash.clone(),
            artifact_fingerprint: artifact.fingerprint(),
        };
        record.validate()?;
        Ok(record)
    }

    /// Validates that metadata cannot silently point at a malformed artifact.
    pub fn validate(&self) -> Result<(), BacktestError> {
        validate_canonical_id("experiment_id", &self.experiment_id)?;
        validate_canonical_id("run_id", &self.run_id)?;
        for (key, value) in &self.tags {
            validate_canonical_id("experiment tag", key)?;
            if value.is_empty() || value.len() > 256 {
                return Err(BacktestError(
                    "experiment tag values must contain 1 to 256 characters".to_owned(),
                ));
            }
        }
        if !is_sha256(&self.specification_fingerprint)
            || !is_sha256(&self.event_output_hash)
            || !is_sha256(&self.artifact_fingerprint)
        {
            return Err(BacktestError(
                "invalid experiment record fingerprint".to_owned(),
            ));
        }
        Ok(())
    }

    /// Portable canonical JSON for experiment search/export adapters.
    pub fn canonical_json(&self) -> String {
        let tags = self
            .tags
            .iter()
            .map(|(key, value)| format!("{}:{}", json_string(key), json_string(value)))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"artifact_fingerprint\":\"{}\",\"event_output_hash\":\"{}\",\"experiment_id\":{},\"run_id\":{},\"specification_fingerprint\":\"{}\",\"tags\":{{{}}}}}",
            self.artifact_fingerprint,
            self.event_output_hash,
            json_string(&self.experiment_id),
            json_string(&self.run_id),
            self.specification_fingerprint,
            tags,
        )
    }

    /// Parses one persisted canonical experiment record before indexing it.
    pub fn from_canonical_json(value: &str) -> Result<Self, BacktestError> {
        let frame: serde_json::Value = serde_json::from_str(value)
            .map_err(|error| BacktestError(format!("invalid experiment JSON: {error}")))?;
        if serde_json::to_string(&frame).map_err(|error| BacktestError(error.to_string()))? != value
        {
            return Err(BacktestError(
                "experiment record is not canonical JSON".to_owned(),
            ));
        }
        let object = frame
            .as_object()
            .ok_or_else(|| BacktestError("experiment record is not an object".to_owned()))?;
        let expected_fields = BTreeSet::from([
            "artifact_fingerprint",
            "event_output_hash",
            "experiment_id",
            "run_id",
            "specification_fingerprint",
            "tags",
        ]);
        if object.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected_fields {
            return Err(BacktestError(
                "experiment record has missing or unknown fields".to_owned(),
            ));
        }
        let tags = object
            .get("tags")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| BacktestError("experiment record has no tag object".to_owned()))?
            .iter()
            .map(|(key, value)| {
                value
                    .as_str()
                    .map(|value| (key.clone(), value.to_owned()))
                    .ok_or_else(|| BacktestError("experiment tag is not a string".to_owned()))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let record = Self {
            experiment_id: experiment_json_string(object, "experiment_id")?,
            run_id: experiment_json_string(object, "run_id")?,
            tags,
            specification_fingerprint: experiment_json_string(object, "specification_fingerprint")?,
            event_output_hash: experiment_json_string(object, "event_output_hash")?,
            artifact_fingerprint: experiment_json_string(object, "artifact_fingerprint")?,
        };
        record.validate()?;
        Ok(record)
    }
}

/// In-memory, append-only experiment index for local search and export.
#[derive(Default)]
pub struct ExperimentCatalog {
    records: BTreeMap<(String, String), ExperimentRecord>,
}

impl ExperimentCatalog {
    /// Records a completed run once; a conflicting overwrite is rejected.
    pub fn record(&mut self, record: ExperimentRecord) -> Result<(), BacktestError> {
        record.validate()?;
        let key = (record.experiment_id.clone(), record.run_id.clone());
        match self.records.get(&key) {
            Some(existing) if existing != &record => Err(BacktestError(
                "experiment run already exists with different immutable metadata".to_owned(),
            )),
            Some(_) => Ok(()),
            None => {
                self.records.insert(key, record);
                Ok(())
            }
        }
    }

    /// Finds immutable records with an exact tag value in stable key order.
    pub fn find_by_tag(&self, key: &str, value: &str) -> Vec<&ExperimentRecord> {
        self.records
            .values()
            .filter(|record| record.tags.get(key).is_some_and(|tag| tag == value))
            .collect()
    }

    /// Looks up one immutable record by its experiment and run identities.
    pub fn get(&self, experiment_id: &str, run_id: &str) -> Option<&ExperimentRecord> {
        self.records
            .get(&(experiment_id.to_owned(), run_id.to_owned()))
    }

    /// Exports all metadata records as stable newline-delimited canonical JSON.
    pub fn export_ndjson(&self) -> String {
        self.records
            .values()
            .map(ExperimentRecord::canonical_json)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Durable append-only local experiment catalog suitable for a single-node deployment.
///
/// The file format is canonical NDJSON. A duplicate immutable record is an
/// idempotent write; an attempt to overwrite a run identity is rejected.
pub struct FileExperimentStore {
    path: PathBuf,
    catalog: ExperimentCatalog,
}

impl FileExperimentStore {
    /// Opens and fully validates an existing experiment index before accepting writes.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BacktestError> {
        let path = path.as_ref().to_path_buf();
        let mut catalog = ExperimentCatalog::default();
        if path.exists() {
            for (index, line) in fs::read_to_string(&path)
                .map_err(|error| BacktestError(error.to_string()))?
                .lines()
                .filter(|line| !line.is_empty())
                .enumerate()
            {
                let record = ExperimentRecord::from_canonical_json(line).map_err(|error| {
                    BacktestError(format!(
                        "invalid experiment record on line {}: {error}",
                        index + 1
                    ))
                })?;
                catalog.record(record)?;
            }
        }
        Ok(Self { path, catalog })
    }

    /// Makes a durable, idempotent metadata write before exposing it to callers.
    pub fn record(&mut self, record: ExperimentRecord) -> Result<(), BacktestError> {
        record.validate()?;
        match self.catalog.get(&record.experiment_id, &record.run_id) {
            Some(existing) if existing == &record => return Ok(()),
            Some(_) => {
                return Err(BacktestError(
                    "experiment run already exists with different immutable metadata".to_owned(),
                ));
            }
            None => {}
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| BacktestError(error.to_string()))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| BacktestError(error.to_string()))?;
        file.write_all(record.canonical_json().as_bytes())
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_data())
            .map_err(|error| BacktestError(error.to_string()))?;
        self.catalog.record(record)
    }

    /// Searches validated records with the same deterministic semantics as the in-memory index.
    pub fn find_by_tag(&self, key: &str, value: &str) -> Vec<&ExperimentRecord> {
        self.catalog.find_by_tag(key, value)
    }

    /// Exports the validated local catalog without reparsing raw disk contents.
    pub fn export_ndjson(&self) -> String {
        self.catalog.export_ndjson()
    }
}

/// Immutable normalized inputs supplied to a single backtest execution.
#[derive(Clone, Debug)]
pub struct BacktestInput {
    /// Account whose cash and fills are represented in the resulting report.
    pub account_id: String,
    /// Single reporting and accounting currency for this initial capability.
    pub currency: String,
    /// Exact opening cash balance in the reporting currency.
    pub initial_cash: Decimal,
    /// The complete ordered, normalized dataset selected by the manifest.
    pub bars: Vec<HistoricalBar>,
    /// Versioned corporate actions selected with the historical dataset.
    pub corporate_actions: Vec<CorporateAction>,
}

impl BacktestInput {
    fn validate_against(&self, spec: &BacktestSpec) -> Result<(), BacktestError> {
        validate_canonical_id("account_id", &self.account_id)?;
        if self.currency.len() != 3
            || !self.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            || self.initial_cash < Decimal::ZERO
        {
            return Err(BacktestError("invalid backtest account balance".to_owned()));
        }
        if self.bars.is_empty() {
            return Err(BacktestError("backtest input contains no bars".to_owned()));
        }
        let mut previous_key: Option<(String, String)> = None;
        let mut bar_identities = BTreeSet::new();
        let mut instrument_ids = BTreeSet::new();
        let mut dataset_bars = Vec::with_capacity(self.bars.len());
        for historical_bar in &self.bars {
            historical_bar.bar.validate()?;
            let key = (
                historical_bar.event_time.clone(),
                historical_bar.bar.instrument_id.clone(),
            );
            if !is_utc(&historical_bar.event_time)
                || !bar_identities.insert(key.clone())
                || previous_key
                    .as_ref()
                    .is_some_and(|previous| key < *previous)
            {
                return Err(BacktestError(
                    "backtest input bars must be unique and ordered by canonical UTC/instrument"
                        .to_owned(),
                ));
            }
            previous_key = Some(key);
            instrument_ids.insert(historical_bar.bar.instrument_id.clone());
            dataset_bars.push((
                historical_bar.event_time.clone(),
                historical_bar.bar.clone(),
            ));
        }
        for action in &self.corporate_actions {
            if !instrument_ids.contains(action.instrument_id()) {
                return Err(BacktestError(format!(
                    "corporate action references an instrument outside the dataset: {}",
                    action.instrument_id()
                )));
            }
        }
        let actual = DatasetManifest::from_market_data(
            &spec.dataset.dataset_id,
            &spec.dataset.dataset_version,
            &spec.dataset.reference_data_version,
            &spec.dataset.universe_id,
            &dataset_bars,
            &self.corporate_actions,
        )?;
        if actual.content_hash != spec.dataset.content_hash
            || actual.starts_at != spec.dataset.starts_at
            || actual.ends_at != spec.dataset.ends_at
        {
            return Err(BacktestError(
                "backtest input does not match its versioned dataset manifest".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Completed deterministic run plus the exact event stream used to fingerprint it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedBacktest {
    /// The portable result and accounting report.
    pub artifact: BacktestArtifact,
    /// Canonical domain events in their immutable append order.
    pub canonical_events: Vec<String>,
    /// Corporate actions applied to the accounting ledger during this run.
    pub applied_corporate_action_ids: Vec<String>,
}

/// Connects the strategy/risk/OMS replay kernel to versioned accounting artifacts.
///
/// A runner is deliberately single-use. Reusing an engine would change its
/// event sequence and create an output that no longer corresponds to the
/// declared immutable run inputs.
pub struct BacktestRunner {
    spec: BacktestSpec,
    engine: ReplayEngine,
    completed: bool,
}

impl BacktestRunner {
    /// Creates a single-use runner only when the engine matches the declared spec.
    pub fn new(spec: BacktestSpec, engine: ReplayEngine) -> Result<Self, BacktestError> {
        spec.validate()?;
        if engine.software_version != spec.engine_version
            || engine.configuration_version != spec.configuration_version
        {
            return Err(BacktestError(
                "replay engine does not match backtest specification".to_owned(),
            ));
        }
        Ok(Self {
            spec,
            engine,
            completed: false,
        })
    }

    /// Executes selected input through the same validated simulation kernel used by replay.
    pub fn run(
        &mut self,
        strategy: &mut impl Strategy,
        input: &BacktestInput,
        market: &MarketPreconditions<'_>,
    ) -> Result<CompletedBacktest, BacktestError> {
        if self.completed {
            return Err(BacktestError(
                "a backtest runner may execute its immutable specification only once".to_owned(),
            ));
        }
        input.validate_against(&self.spec)?;

        let selected_bars: Vec<_> = input
            .bars
            .iter()
            .filter(|bar| {
                self.spec.starts_at.as_str() <= bar.event_time.as_str()
                    && bar.event_time.as_str() <= self.spec.ends_at.as_str()
            })
            .cloned()
            .collect();
        if selected_bars.is_empty() {
            return Err(BacktestError(
                "backtest time range selects no normalized bars".to_owned(),
            ));
        }

        // From this point the replay engine may have emitted events. Consume
        // the runner before executing so an error cannot be retried against a
        // partially advanced engine.
        self.completed = true;

        let mut actions = input.corporate_actions.clone();
        actions.sort_by(|left, right| {
            left.effective_at()
                .cmp(right.effective_at())
                .then_with(|| left.action_id().cmp(right.action_id()))
        });
        let mut ledger = BacktestLedger::new(&input.currency, input.initial_cash)?;
        let mut marks = BTreeMap::new();
        let mut store = InMemoryEventStore::default();
        let mut canonical_events = Vec::new();
        let mut applied_corporate_action_ids = Vec::new();
        let mut equity_curve = Vec::new();
        let mut next_action = 0;

        for historical_bar in selected_bars {
            while actions
                .get(next_action)
                .is_some_and(|action| action.effective_at() <= historical_bar.event_time.as_str())
            {
                let action = &actions[next_action];
                ledger.apply_corporate_action(action)?;
                applied_corporate_action_ids.push(action.action_id().to_owned());
                next_action += 1;
            }

            let result = self.engine.process_bar_with_market_preconditions(
                &mut store,
                strategy,
                &input.account_id,
                &historical_bar.event_time,
                historical_bar.bar.clone(),
                market,
            )?;
            for event in &result.events {
                if let EventPayload::Fill(fill) = &event.payload {
                    ledger.apply_fill(fill)?;
                }
                canonical_events.push(event.canonical_json());
            }
            marks.insert(
                historical_bar.bar.instrument_id.clone(),
                historical_bar.bar.close,
            );
            equity_curve.push(EquityPoint {
                event_time: historical_bar.event_time,
                total_equity: ledger.report(&marks)?.total_equity,
            });
        }

        // A sink is part of the execution boundary. Compare it with the
        // returned projection so neither path can silently omit an event.
        let persisted_events: Vec<_> = store
            .events()
            .iter()
            .map(|event| event.canonical_json())
            .collect();
        if persisted_events != canonical_events {
            return Err(BacktestError(
                "replay result and persisted event stream diverged".to_owned(),
            ));
        }

        let report = ledger.report(&marks)?;
        let trade_count = u64::try_from(
            ledger
                .entries()
                .iter()
                .filter(|entry| entry.entry_type == "FILL")
                .count(),
        )
        .map_err(|_| BacktestError("too many accounting entries".to_owned()))?;
        let performance = PerformanceReport::from_equity_curve(
            input.initial_cash,
            equity_curve,
            u64::try_from(canonical_events.len())
                .map_err(|_| BacktestError("too many canonical events".to_owned()))?,
            trade_count,
            u64::try_from(applied_corporate_action_ids.len())
                .map_err(|_| BacktestError("too many corporate actions".to_owned()))?,
        )?;
        let artifact = BacktestArtifact::with_run_details(
            &self.spec,
            &canonical_events,
            report,
            ledger.entries().to_vec(),
            performance,
        )?;
        Ok(CompletedBacktest {
            artifact,
            canonical_events,
            applied_corporate_action_ids,
        })
    }
}

/// Backtest construction or accounting failure.
#[derive(Debug)]
pub struct BacktestError(pub String);

impl std::fmt::Display for BacktestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for BacktestError {}

impl From<DomainError> for BacktestError {
    fn from(error: DomainError) -> Self {
        Self(error.0)
    }
}
impl From<DecimalError> for BacktestError {
    fn from(error: DecimalError) -> Self {
        Self(error.0)
    }
}

impl From<EngineError> for BacktestError {
    fn from(error: EngineError) -> Self {
        Self(error.0)
    }
}

fn hash_market_data(
    bars: &[(String, Bar)],
    actions: &[CorporateAction],
) -> Result<String, BacktestError> {
    let mut canonical = String::new();
    let mut previous: Option<(String, String)> = None;
    let mut identities = BTreeSet::new();
    for (event_time, bar) in bars {
        bar.validate()?;
        let key = (event_time.clone(), bar.instrument_id.clone());
        if !is_utc(event_time)
            || !identities.insert(key.clone())
            || previous.as_ref().is_some_and(|value| key < *value)
        {
            return Err(BacktestError(
                "bars must be unique and ordered by canonical UTC/instrument".to_owned(),
            ));
        }
        previous = Some(key);
        writeln!(
            canonical,
            "{event_time}|{}|{}|{}|{}|{}|{}|{}|{}",
            bar.instrument_id,
            bar.open,
            bar.high,
            bar.low,
            bar.close,
            bar.volume,
            bar.interval_seconds,
            bar.exchange_timezone
        )
        .expect("writing to a string cannot fail");
    }
    let mut ordered_actions = actions.to_vec();
    ordered_actions.sort_by(|left, right| {
        left.effective_at()
            .cmp(right.effective_at())
            .then_with(|| left.action_id().cmp(right.action_id()))
    });
    let mut action_ids = std::collections::BTreeSet::new();
    for action in &ordered_actions {
        action.validate()?;
        if !action_ids.insert(action.action_id()) {
            return Err(BacktestError(format!(
                "duplicate corporate action ID: {}",
                action.action_id()
            )));
        }
        writeln!(canonical, "action|{}", action.canonical_record())
            .expect("writing to a string cannot fail");
    }
    Ok(sha256(&canonical))
}

fn sha256(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn is_utc(value: &str) -> bool {
    validate_utc_timestamp("timestamp", value).is_ok()
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}

fn experiment_json_string(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<String, BacktestError> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| BacktestError(format!("experiment record has no {field}")))
}

fn accounting_entries_json(entries: &[AccountingEntry]) -> String {
    let entries = entries
        .iter()
        .map(|entry| {
            format!(
                "{{\"cash_delta\":\"{}\",\"entry_id\":{},\"entry_type\":{},\"instrument_id\":{},\"occurred_at\":{},\"quantity_delta\":\"{}\"}}",
                entry.cash_delta,
                json_string(&entry.entry_id),
                json_string(&entry.entry_type),
                json_string(&entry.instrument_id),
                json_string(&entry.occurred_at),
                entry.quantity_delta,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{entries}]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn bar() -> Bar {
        Bar {
            instrument_id: "inst.us_equity.spy".to_owned(),
            open: Decimal::from_integer(100).unwrap(),
            high: Decimal::from_integer(101).unwrap(),
            low: Decimal::from_integer(99).unwrap(),
            close: Decimal::from_integer(100).unwrap(),
            volume: Decimal::from_integer(10).unwrap(),
            interval_seconds: 60,
            exchange_timezone: "America/New_York".to_owned(),
        }
    }

    #[test]
    fn identical_specifications_have_identical_fingerprints() {
        let dataset = DatasetManifest {
            dataset_id: "dataset.spy".to_owned(),
            dataset_version: "v1".to_owned(),
            reference_data_version: "ref-v1".to_owned(),
            universe_id: "universe.spy".to_owned(),
            content_hash: "b".repeat(64),
            starts_at: "2026-01-02T14:30:00Z".to_owned(),
            ends_at: "2026-01-02T14:31:00Z".to_owned(),
        };
        let spec = BacktestSpec {
            strategy_bundle_hash: "a".repeat(64),
            dataset,
            configuration_id: "config.test".to_owned(),
            configuration_version: "cfg-v1".to_owned(),
            configuration_hash: "b".repeat(64),
            seed: 7,
            engine_version: "engine-v1".to_owned(),
            starts_at: "2026-01-02T14:30:00Z".to_owned(),
            ends_at: "2026-01-02T14:31:00Z".to_owned(),
        };
        assert_eq!(
            spec.fingerprint().unwrap(),
            "6c85e1e5453bcb9fedfe95787a14c73bdfbf5b51b35d058821098c00e8a084a3"
        );
        let mut changed_configuration = spec.clone();
        changed_configuration.configuration_hash = "c".repeat(64);
        assert_ne!(
            spec.fingerprint().unwrap(),
            changed_configuration.fingerprint().unwrap()
        );
        let mut uppercase_hash = spec;
        uppercase_hash.configuration_hash = "A".repeat(64);
        assert!(uppercase_hash.validate().is_err());
    }

    #[test]
    fn ledger_applies_split_and_dividend_with_exact_values() {
        let mut ledger = BacktestLedger::new("USD", Decimal::from_integer(1_000).unwrap()).unwrap();
        ledger
            .apply_fill(&Fill {
                execution_id: "exec-001".to_owned(),
                order_id: "order-001".to_owned(),
                instrument_id: "inst.us_equity.spy".to_owned(),
                side: Side::Buy,
                quantity: Decimal::from_integer(2).unwrap(),
                price: Decimal::from_integer(100).unwrap(),
                fee: Decimal::ZERO,
                executed_at: "2026-01-02T14:30:00Z".to_owned(),
            })
            .unwrap();
        ledger
            .apply_corporate_action(&CorporateAction::Split {
                action_id: "action-split-001".to_owned(),
                instrument_id: "inst.us_equity.spy".to_owned(),
                effective_at: "2026-01-03T00:00:00Z".to_owned(),
                ratio: Decimal::from_integer(2).unwrap(),
            })
            .unwrap();
        ledger
            .apply_corporate_action(&CorporateAction::CashDividend {
                action_id: "action-dividend-001".to_owned(),
                instrument_id: "inst.us_equity.spy".to_owned(),
                effective_at: "2026-01-04T00:00:00Z".to_owned(),
                amount: Decimal::from_str("0.5").unwrap(),
            })
            .unwrap();
        let report = ledger
            .report(&BTreeMap::from([(
                "inst.us_equity.spy".to_owned(),
                Decimal::from_integer(50).unwrap(),
            )]))
            .unwrap();
        assert_eq!(report.cash, Decimal::from_integer(802).unwrap());
        assert_eq!(
            report.positions[0].quantity,
            Decimal::from_integer(4).unwrap()
        );
        assert_eq!(
            report.positions[0].average_cost,
            Decimal::from_integer(50).unwrap()
        );
    }

    fn market_dependencies() -> (
        follon_instrument::InstrumentRegistry,
        follon_instrument::StaticTradingCalendar,
    ) {
        use follon_instrument::{
            AssetClass, Instrument, InstrumentVersion, StaticTradingCalendar, TradingSession,
        };

        let calendar = StaticTradingCalendar::new(
            "cal.us_equities.nyse",
            vec![TradingSession {
                exchange_date: "2026-01-02".to_owned(),
                opens_at: "2026-01-02T14:30:00Z".to_owned(),
                closes_at: "2026-01-02T21:00:00Z".to_owned(),
            }],
        )
        .unwrap();
        let mut instruments = follon_instrument::InstrumentRegistry::default();
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
                reference_version: "reference-example-1".to_owned(),
            })
            .unwrap();
        (instruments, calendar)
    }

    fn runner_engine() -> ReplayEngine {
        use follon_control_plane::{DeterministicFillModel, RiskPolicy};

        ReplayEngine::new(
            "2026-01-02T14:30:00Z",
            "engine-v1",
            "cfg-v1",
            RiskPolicy {
                version: "risk-v1".to_owned(),
                global_kill_switch: false,
                max_quantity: Decimal::from_integer(10).unwrap(),
                max_notional: Decimal::from_integer(10_000).unwrap(),
            },
            DeterministicFillModel {
                spread_bps: Decimal::ZERO,
                slippage_bps: Decimal::ZERO,
                flat_fee: Decimal::from_str("0.10").unwrap(),
                latency_bars: 0,
                max_fill_quantity: None,
            },
        )
        .unwrap()
    }

    fn runner_input() -> BacktestInput {
        let first = bar();
        let mut second = bar();
        second.open = Decimal::from_integer(50).unwrap();
        second.high = Decimal::from_integer(51).unwrap();
        second.low = Decimal::from_integer(49).unwrap();
        second.close = Decimal::from_integer(50).unwrap();
        BacktestInput {
            account_id: "acct-paper-001".to_owned(),
            currency: "USD".to_owned(),
            initial_cash: Decimal::from_integer(1_000).unwrap(),
            bars: vec![
                HistoricalBar {
                    event_time: "2026-01-02T14:30:00Z".to_owned(),
                    bar: first,
                },
                HistoricalBar {
                    event_time: "2026-01-02T14:31:00Z".to_owned(),
                    bar: second,
                },
            ],
            corporate_actions: vec![CorporateAction::Split {
                action_id: "action-split-001".to_owned(),
                instrument_id: "inst.us_equity.spy".to_owned(),
                effective_at: "2026-01-02T14:30:30Z".to_owned(),
                ratio: Decimal::from_integer(2).unwrap(),
            }],
        }
    }

    fn runner_spec(input: &BacktestInput) -> BacktestSpec {
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
            ends_at: "2026-01-02T14:31:00Z".to_owned(),
        }
    }

    fn runner_strategy() -> follon_control_plane::BuyOnceStrategy {
        follon_control_plane::BuyOnceStrategy::new(
            "acct-paper-001",
            "strategy-example-001",
            "strategy-example-v1",
            "cfg-v1",
            Decimal::from_integer(100).unwrap(),
        )
    }

    #[test]
    fn versioned_input_repeatedly_produces_an_identical_complete_artifact() {
        let input = runner_input();
        let spec = runner_spec(&input);
        let (instruments, calendar) = market_dependencies();
        let market = MarketPreconditions {
            instruments: &instruments,
            calendar: &calendar,
        };

        let first = BacktestRunner::new(spec.clone(), runner_engine())
            .unwrap()
            .run(&mut runner_strategy(), &input, &market)
            .unwrap();
        let second = BacktestRunner::new(spec, runner_engine())
            .unwrap()
            .run(&mut runner_strategy(), &input, &market)
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(
            first.applied_corporate_action_ids,
            vec!["action-split-001".to_owned()]
        );
        assert_eq!(
            first.artifact.report.positions[0].quantity,
            Decimal::from_integer(2).unwrap()
        );
        assert_eq!(
            first.artifact.report.positions[0].average_cost,
            Decimal::from_str("50.05").unwrap()
        );
        assert_eq!(
            first.artifact.report.total_equity,
            Decimal::from_str("999.90").unwrap()
        );
        assert_eq!(
            first.artifact.report.total_fees,
            Decimal::from_str("0.10").unwrap()
        );
        assert_eq!(first.artifact.accounting_entries.len(), 2);
        assert_eq!(first.artifact.performance.trade_count, 1);
        assert_eq!(first.artifact.performance.corporate_action_count, 1);
        assert_eq!(first.artifact.performance.equity_curve.len(), 2);
        assert!(first
            .artifact
            .markdown_report()
            .contains("## Accounting entries"));
        let artifact_json: serde_json::Value =
            serde_json::from_str(&first.artifact.canonical_json()).unwrap();
        assert_eq!(artifact_json["artifact_schema_version"], 2);
        assert_eq!(
            artifact_json["specification"]["configuration_hash"],
            "b".repeat(64)
        );
        assert_eq!(artifact_json["performance"]["trade_count"], 1);
    }

    #[test]
    fn experiment_catalog_is_idempotent_and_rejects_conflicting_immutable_runs() {
        let input = runner_input();
        let spec = runner_spec(&input);
        let (instruments, calendar) = market_dependencies();
        let market = MarketPreconditions {
            instruments: &instruments,
            calendar: &calendar,
        };
        let completed = BacktestRunner::new(spec, runner_engine())
            .unwrap()
            .run(&mut runner_strategy(), &input, &market)
            .unwrap();
        let record = ExperimentRecord::from_artifact(
            "experiment-momentum-001",
            "run-001",
            BTreeMap::from([("regime".to_owned(), "baseline".to_owned())]),
            &completed.artifact,
        )
        .unwrap();
        let mut catalog = ExperimentCatalog::default();
        catalog.record(record.clone()).unwrap();
        catalog.record(record).unwrap();
        assert_eq!(catalog.find_by_tag("regime", "baseline").len(), 1);
        assert!(serde_json::from_str::<serde_json::Value>(&catalog.export_ndjson()).is_ok());
    }

    #[test]
    fn file_experiment_store_recovers_validated_idempotent_records() {
        let path = std::env::temp_dir().join(format!(
            "follon-experiments-{}-{}.ndjson",
            std::process::id(),
            "durable-catalog"
        ));
        let _ = std::fs::remove_file(&path);
        let record = ExperimentRecord {
            experiment_id: "experiment-durable-001".to_owned(),
            run_id: "run-001".to_owned(),
            tags: BTreeMap::from([("regime".to_owned(), "baseline".to_owned())]),
            specification_fingerprint: "a".repeat(64),
            event_output_hash: "b".repeat(64),
            artifact_fingerprint: "c".repeat(64),
        };
        let mut store = FileExperimentStore::open(&path).unwrap();
        store.record(record.clone()).unwrap();
        store.record(record.clone()).unwrap();
        drop(store);

        let recovered = FileExperimentStore::open(&path).unwrap();
        assert_eq!(recovered.find_by_tag("regime", "baseline").len(), 1);
        assert_eq!(recovered.export_ndjson(), record.canonical_json());
        std::fs::remove_file(path).unwrap();
    }
}
