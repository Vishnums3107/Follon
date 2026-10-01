//! The PAPER trading service: OMS, risk gate, accounting and reconciliation.

use follon_accounting::TaxLotBook;
use follon_control_plane::Portfolio;
use follon_domain::Decimal;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::*;

/// Paper-only OMS service with independent accounting and mandatory reconciliation.
pub struct PaperTradingService<B> {
    pub(crate) account: PaperAccount,
    pub(crate) risk_policy: PaperRiskPolicy,
    kill_switches: KillSwitchRegistry,
    /// Operator-attributed kill-switch changes, oldest first (E3.3b). A local
    /// change through [`Self::activate_kill_switch`] adds none, as before.
    kill_switch_operations: Vec<KillSwitchOperation>,
    /// Operator-attributed order commands, oldest first (E5.2b). A direct
    /// [`Self::cancel_order`] adds none.
    pub(crate) order_operations: Vec<OrderOperation>,
    pub(crate) broker: B,
    broker_route_fingerprint: String,
    pub(crate) broker_connected: bool,
    pub(crate) cash: Decimal,
    pub(crate) orders: BTreeMap<String, PaperOrder>,
    risk_evidence: BTreeMap<String, PaperRiskEvidence>,
    /// Atomic multi-leg combinations, tracked separately from `orders` because
    /// a combination is one order over several instruments and does not fit the
    /// single-instrument shape of [`PaperOrder`]. Every risk counter that reads
    /// `orders` reads this map too -- a combination that were invisible to the
    /// single-order gate would be a hole in exactly the limits it is subject to.
    pub(crate) combo_orders: BTreeMap<String, PaperComboOrder>,
    combo_risk_evidence: BTreeMap<String, PaperComboRiskEvidence>,
    pub(crate) portfolios: BTreeMap<String, Portfolio>,
    /// Independent FIFO long-lot cost-basis ledger, kept in lockstep with
    /// `portfolios` from the same fills. `Portfolio` tracks a single running
    /// average cost for OMS/risk decisions; this book instead retains
    /// individual acquisition lots so a real disposal reports an auditable,
    /// tax-lot-accurate realized gain/loss, not just the average-cost figure.
    pub(crate) tax_lots: TaxLotBook,
    /// Last observed mark per instrument, from every past risk evaluation this
    /// service has performed. Feeds the Slice-1 aggregate risk composition's
    /// multi-instrument exposure calculation; a position with no cached mark
    /// yet falls back to its own average cost. Not a live market-data feed --
    /// see [`PortfolioRiskComposition`].
    marks: BTreeMap<String, Decimal>,
    /// Highest real equity (cash plus every marked position) this service has
    /// ever observed, updated unconditionally from every risk evaluation --
    /// independent of whether Slice-1 composition is even configured, so
    /// enabling it later does not start drawdown tracking from a fresh,
    /// artificially favorable baseline. Feeds the Slice-2 `PortfolioRiskSnapshot`'s
    /// real `peak_equity` (see [`PortfolioRiskComposition`]).
    peak_equity: Decimal,
    /// UTC calendar date (`YYYY-MM-DD`, sliced from a risk decision's own
    /// canonical `decided_at`) of the current session-start daily-loss
    /// baseline. `None` until the first risk evaluation this service has ever
    /// performed. Reset -- not maxed, unlike `peak_equity` -- at the first
    /// evaluation whose decision time falls on a new UTC calendar day.
    daily_baseline_date: Option<String>,
    /// Real equity observed at the first risk evaluation of
    /// `daily_baseline_date`. Meaningless while `daily_baseline_date` is
    /// `None`. Feeds the Slice-2b `PortfolioRiskSnapshot`'s real `daily_pnl`
    /// (`equity - daily_baseline_equity`) -- see [`PortfolioRiskComposition`].
    daily_baseline_equity: Decimal,
    /// Net signed quantity each strategy has contributed to each instrument
    /// (`instrument_id -> strategy_id -> quantity`), updated from every real
    /// fill. Feeds the Slice-2d aggregate-risk snapshot's real per-strategy
    /// `RiskPosition` rows -- see [`PortfolioRiskComposition`] and
    /// [`Self::apply_strategy_attribution_fill`].
    pub(crate) strategy_attribution: BTreeMap<String, BTreeMap<String, Decimal>>,
    pub(crate) execution_ids: BTreeSet<String>,
    /// Operator-attested corporate actions, in the order applied (E8.4b).
    corporate_actions: Vec<PaperCorporateActionReceipt>,
    incidents: BTreeMap<String, ReconciliationIncident>,
    last_reconciled_at: Option<String>,
    last_reconciliation_clean: Option<bool>,
    latest_reconciliation: Option<ReconciliationReport>,
    paper_days: BTreeMap<String, PersistentPaperDay>,
    next_reconciliation: u64,
    persistence_healthy: bool,
    pub(crate) journal: Option<FilePaperJournal>,
}

mod broker_events;
mod corporate_actions;
mod kill_switch;
mod orders;
mod persistence;
mod portfolio_risk;
mod promotion;
mod reconcile;
mod risk;
mod submit;

impl<B: PaperBrokerAdapter> PaperTradingService<B> {
    /// Creates a paper-only OMS service with explicit account, risk, and kill controls.
    pub fn new(
        account: PaperAccount,
        risk_policy: PaperRiskPolicy,
        kill_switches: KillSwitchRegistry,
        broker: B,
    ) -> Result<Self, PaperError> {
        account.validate()?;
        risk_policy.validate()?;
        let broker_route_fingerprint = broker.configuration_fingerprint(&account.account_id)?;
        Ok(Self {
            cash: account.initial_cash,
            peak_equity: account.initial_cash,
            daily_baseline_date: None,
            daily_baseline_equity: Decimal::ZERO,
            strategy_attribution: BTreeMap::new(),
            account,
            risk_policy,
            kill_switches,
            kill_switch_operations: Vec::new(),
            order_operations: Vec::new(),
            broker,
            broker_route_fingerprint,
            broker_connected: true,
            orders: BTreeMap::new(),
            risk_evidence: BTreeMap::new(),
            combo_orders: BTreeMap::new(),
            combo_risk_evidence: BTreeMap::new(),
            portfolios: BTreeMap::new(),
            tax_lots: TaxLotBook::default(),
            marks: BTreeMap::new(),
            execution_ids: BTreeSet::new(),
            corporate_actions: Vec::new(),
            incidents: BTreeMap::new(),
            last_reconciled_at: None,
            last_reconciliation_clean: None,
            latest_reconciliation: None,
            paper_days: BTreeMap::new(),
            next_reconciliation: 1,
            persistence_healthy: true,
            journal: None,
        })
    }

    /// Opens a paper service from a fully validated durable journal snapshot.
    pub fn open_durable(
        account: PaperAccount,
        risk_policy: PaperRiskPolicy,
        kill_switches: KillSwitchRegistry,
        broker: B,
        journal_path: impl AsRef<Path>,
    ) -> Result<Self, PaperError> {
        // Validate the configuration before touching the journal, which
        // `FilePaperJournal::open` creates when absent, so a refused start
        // leaves no journal behind, as controlled LIVE does (E3.10).
        let mut service = Self::new(account, risk_policy, kill_switches, broker)?;
        // A composition that may only reopen a journal opens it without
        // creating one, so that refusal leaves nothing behind either (E3.10b).
        let may_initialize = service
            .broker
            .permits_empty_journal(&service.account.account_id);
        let journal = FilePaperJournal::open_with(journal_path.as_ref(), may_initialize)?
            .filter(|journal| may_initialize || journal.latest().is_some())
            .ok_or_else(|| {
                PaperError(
                    "legacy PAPER adapter routing may only reopen an existing journal".to_owned(),
                )
            })?;
        let latest = journal.latest().cloned();
        if let Some(state) = latest {
            service.restore(state)?;
            // An external broker session never survives process recovery.
            service.broker_connected = false;
        }
        service.journal = Some(journal);
        if service
            .journal
            .as_ref()
            .is_some_and(|journal| journal.latest().is_none())
        {
            service.persist()?;
        }
        Ok(service)
    }

    /// Returns the independently controlled kill-switch registry.
    pub fn kill_switches(&self) -> &KillSwitchRegistry {
        &self.kill_switches
    }

    /// Returns the broker adapter only for controlled paper operational actions/tests.
    pub fn broker_mut(&mut self) -> &mut B {
        &mut self.broker
    }

    /// Returns the canonical account identity owned by this service instance.
    /// Delivery boundaries use this before account-scoped cancel/close actions
    /// so a syntactically valid but mismatched account can never act on an
    /// order or position belonging to the configured route.
    pub fn account_id(&self) -> &str {
        &self.account.account_id
    }

    /// Returns an OMS order by its immutable client identity.
    pub fn order(&self, order_id: &str) -> Option<&PaperOrder> {
        self.orders.get(order_id)
    }

    /// Returns immutable risk and market evidence by its paper decision identity.
    pub fn risk_evidence(&self, decision_id: &str) -> Option<&PaperRiskEvidence> {
        self.risk_evidence.get(decision_id)
    }
}
