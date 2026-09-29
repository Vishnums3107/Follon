//! The PAPER trading service: OMS, risk gate, accounting and reconciliation.

use follon_accounting::{
    Currency, FxBook, MarginPolicy, MarginPosition, ShortTaxLot, TaxLot, TaxLotBook,
    TaxLotBookSnapshot, TaxLotSelection,
};
use follon_control_plane::{OmsComboOrder, OmsOrder, Portfolio};
use follon_domain::{
    price_deviation_bps, validate_canonical_id, validate_utc_timestamp, ComboIntent, Decimal, Fill,
    OrderIntent, OrderState, RiskDecision, Side, TimeInForce,
};
use follon_instrument::{TradingCalendar, TradingSession};
use follon_risk::{CandidateOrder, PortfolioRiskSnapshot, RestingOrder, RiskPosition};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

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
    incidents: BTreeMap<String, ReconciliationIncident>,
    last_reconciled_at: Option<String>,
    last_reconciliation_clean: Option<bool>,
    latest_reconciliation: Option<ReconciliationReport>,
    paper_days: BTreeMap<String, PersistentPaperDay>,
    next_reconciliation: u64,
    persistence_healthy: bool,
    pub(crate) journal: Option<FilePaperJournal>,
}

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

    /// Activates a kill switch without involving strategy or broker processes.
    pub fn activate_kill_switch(&mut self, scope: KillSwitchScope) -> Result<bool, PaperError> {
        self.ensure_persistence_healthy()?;
        let changed = self.kill_switches.activate(scope)?;
        self.persist()?;
        Ok(changed)
    }

    /// Explicitly deactivates one kill switch. It does not mutate past decisions.
    pub fn deactivate_kill_switch(&mut self, scope: &KillSwitchScope) -> Result<bool, PaperError> {
        self.ensure_persistence_healthy()?;
        let changed = self.kill_switches.deactivate(scope);
        if changed {
            self.persist()?;
        }
        Ok(changed)
    }

    /// Activates a kill switch for an authenticated operator (E3.3b). The
    /// change is journaled with the operator and time. Activating a switch
    /// that is already active changes nothing and journals nothing.
    pub fn activate_kill_switch_as(
        &mut self,
        scope: KillSwitchScope,
        operator: &str,
        operated_at: &str,
    ) -> Result<bool, PaperError> {
        self.operate_kill_switch_as(scope, KillSwitchAction::Activate, operator, operated_at)
    }

    /// Releases a kill switch for an authenticated operator, journaled exactly
    /// as [`Self::activate_kill_switch_as`] journals an activation.
    pub fn release_kill_switch_as(
        &mut self,
        scope: KillSwitchScope,
        operator: &str,
        operated_at: &str,
    ) -> Result<bool, PaperError> {
        self.operate_kill_switch_as(scope, KillSwitchAction::Release, operator, operated_at)
    }

    /// Operator-attributed kill-switch changes, oldest first.
    pub fn kill_switch_operations(&self) -> &[KillSwitchOperation] {
        &self.kill_switch_operations
    }

    /// Operator-attributed order commands, oldest first (E5.2b).
    pub fn order_operations(&self) -> &[OrderOperation] {
        &self.order_operations
    }

    fn operate_kill_switch_as(
        &mut self,
        scope: KillSwitchScope,
        action: KillSwitchAction,
        operator: &str,
        operated_at: &str,
    ) -> Result<bool, PaperError> {
        self.ensure_persistence_healthy()?;
        scope.validate()?;
        validate_canonical_id("kill-switch operator", operator)?;
        validate_utc_timestamp("kill-switch operation time", operated_at)?;
        let changed = match action {
            KillSwitchAction::Activate => self.kill_switches.activate(scope.clone())?,
            KillSwitchAction::Release => self.kill_switches.deactivate(&scope),
        };
        if changed {
            self.kill_switch_operations.push(KillSwitchOperation {
                scope: scope.as_key(),
                action,
                operator: operator.to_owned(),
                operated_at: operated_at.to_owned(),
            });
            self.persist()?;
        }
        Ok(changed)
    }

    /// Applies risk, creates a client-idempotent order, then attempts paper submission.
    ///
    /// A transport error changes the newly-created order to `UNKNOWN` before
    /// returning an error. Call [`Self::reconnect_and_reconcile`] rather than
    /// blindly retrying an ambiguous submission.
    pub fn submit_intent(
        &mut self,
        intent: OrderIntent,
        market: PaperMarketData,
        decided_at: &str,
    ) -> Result<PaperSubmitOutcome, PaperError> {
        self.submit_intent_as(intent, market, decided_at, None)
    }

    /// Submits a single order on behalf of an authenticated operator, whose
    /// identity is journaled with the risk evidence, as
    /// [`Self::submit_combo_intent_as`] does for a combination. An idempotent
    /// retry must come from the same submitter (delivery state E5.2b).
    pub fn submit_intent_as(
        &mut self,
        intent: OrderIntent,
        market: PaperMarketData,
        decided_at: &str,
        submitted_by: Option<&str>,
    ) -> Result<PaperSubmitOutcome, PaperError> {
        self.ensure_persistence_healthy()?;
        if let Some(operator) = submitted_by {
            validate_canonical_id("paper order submitted_by", operator)?;
        }
        intent.validate()?;
        validate_utc_timestamp("paper risk decision time", decided_at)?;
        market.validate()?;
        if intent.environment != "PAPER" {
            return Err(PaperError(
                "paper OMS refuses an intent outside the PAPER environment".to_owned(),
            ));
        }
        if intent.account_id != self.account.account_id {
            return Err(PaperError(
                "paper intent account does not match service".to_owned(),
            ));
        }
        if !self.broker_connected {
            return Err(PaperError(
                "paper broker session is disconnected; reconnect and reconcile before submission"
                    .to_owned(),
            ));
        }
        if market.instrument_id != intent.instrument_id {
            return Err(PaperError(
                "paper market observation instrument does not match intent".to_owned(),
            ));
        }
        let observed_at = OffsetDateTime::parse(&market.observed_at, &Rfc3339)
            .map_err(|error| PaperError(error.to_string()))?;
        let decision_time = OffsetDateTime::parse(decided_at, &Rfc3339)
            .map_err(|error| PaperError(error.to_string()))?;
        let age = (decision_time - observed_at).whole_seconds();
        if age < 0
            || u64::try_from(age).unwrap_or(u64::MAX) > self.risk_policy.max_market_data_age_seconds
        {
            return Err(PaperError(
                "paper market observation is stale or later than the risk decision".to_owned(),
            ));
        }
        let order_id = format!("order-{}", intent.intent_id);
        if let Some(existing) = self.orders.get(&order_id) {
            if existing.oms.intent != intent {
                return Err(PaperError(
                    "paper order idempotency key was reused with different intent data".to_owned(),
                ));
            }
            let evidence = self
                .risk_evidence
                .get(&format!("paper-risk-{}", intent.intent_id))
                .ok_or_else(|| {
                    PaperError("existing paper order is missing its risk evidence".to_owned())
                })?;
            if evidence.market != market || evidence.decision.decided_at != decided_at {
                return Err(PaperError(
                    "paper order retry must use the original market observation and decision time"
                        .to_owned(),
                ));
            }
            if evidence.submitted_by.as_deref() != submitted_by {
                return Err(PaperError(
                    "paper order retry must come from the original submitter".to_owned(),
                ));
            }
            return Ok(PaperSubmitOutcome {
                decision: evidence.decision.clone(),
                order_id: Some(order_id),
                state: Some(existing.oms.state),
            });
        }

        self.ensure_route_carries(intent.time_in_force, false)?;
        let decision = self.evaluate_risk(&intent, &market, decided_at)?;
        let evidence = PaperRiskEvidence {
            intent: intent.clone(),
            decision: decision.clone(),
            market: market.clone(),
            submitted_by: submitted_by.map(str::to_owned),
        };
        if !decision.approved {
            self.risk_evidence
                .insert(decision.decision_id.clone(), evidence);
            self.persist()?;
            return Ok(PaperSubmitOutcome {
                decision,
                order_id: None,
                state: None,
            });
        }
        let mut oms = OmsOrder::from_approved_intent(intent, &decision)?;
        oms.transition(OrderState::Approved, "PAPER_RISK_APPROVED")?;
        oms.transition(OrderState::PendingSubmit, "PAPER_SUBMISSION_REQUESTED")?;
        let request = BrokerOrderRequest::from_order(&oms);
        self.orders.insert(
            oms.order_id.clone(),
            PaperOrder {
                oms,
                broker_order_id: None,
                broker_order_versions: Vec::new(),
                replace_return_state: None,
                filled_quantity: Decimal::ZERO,
                market,
            },
        );
        self.risk_evidence
            .insert(decision.decision_id.clone(), evidence);
        // Persist `PENDING_SUBMIT` before crossing the external broker boundary.
        self.persist()?;

        let submission = self.broker.submit(&request);
        match submission {
            Ok(BrokerSubmitResult::Acknowledged { broker_order_id }) => {
                validate_canonical_id("broker order_id", &broker_order_id)?;
                let order = self.order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "PAPER_SUBMISSION_SENT")?;
                order
                    .oms
                    .transition(OrderState::Acknowledged, "IBKR_PAPER_ACKNOWLEDGED")?;
                order.broker_order_id = Some(broker_order_id.clone());
                order.broker_order_versions.push(broker_order_id);
                let state = order.oms.state;
                self.persist()?;
                Ok(PaperSubmitOutcome {
                    decision,
                    order_id: Some(request.client_order_id),
                    state: Some(state),
                })
            }
            Ok(BrokerSubmitResult::Rejected { reason }) => {
                validate_broker_reason("broker rejection reason", &reason)?;
                let order = self.order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "PAPER_SUBMISSION_SENT")?;
                order.oms.transition(OrderState::Rejected, reason)?;
                let state = order.oms.state;
                self.persist()?;
                Ok(PaperSubmitOutcome {
                    decision,
                    order_id: Some(request.client_order_id),
                    state: Some(state),
                })
            }
            Ok(BrokerSubmitResult::Unknown { reason }) => {
                validate_broker_reason("broker unknown-outcome reason", &reason)?;
                let order = self.order_mut(&request.client_order_id)?;
                order.oms.transition(OrderState::Unknown, reason)?;
                self.persist()?;
                Ok(PaperSubmitOutcome {
                    decision,
                    order_id: Some(request.client_order_id),
                    state: Some(OrderState::Unknown),
                })
            }
            Err(error) => {
                let order = self.order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Unknown, "PAPER_TRANSPORT_OUTCOME_UNKNOWN")?;
                self.broker_connected = false;
                self.persist()?;
                Err(error)
            }
        }
    }

    /// Submits one atomic multi-leg combination through the full risk gate.
    ///
    /// This is the combination analogue of [`Self::submit_intent`] and follows
    /// the same order of operations for the same reasons: validate, refuse a
    /// disconnected session, answer an idempotent retry from stored evidence,
    /// evaluate risk, persist `PENDING_SUBMIT` **before** crossing the broker
    /// boundary, then record the normalized outcome. A transport failure leaves
    /// the combination explicitly `UNKNOWN` rather than guessing.
    ///
    /// A combination is submitted as one broker order. If the adapter does not
    /// support a native atomic combination it rejects the whole request, and no
    /// leg is ever transmitted on its own — that boundary is the adapter's, and
    /// this method does not work around it by splitting the group.
    pub fn submit_combo_intent(
        &mut self,
        intent: ComboIntent,
        market: PaperComboMarketData,
        decided_at: &str,
    ) -> Result<PaperSubmitOutcome, PaperError> {
        self.submit_combo_intent_as(intent, market, decided_at, None)
    }

    /// Submits a combination on behalf of an authenticated operator, whose
    /// identity is journaled with the risk evidence. An idempotent retry must
    /// come from the same submitter, so one operator cannot claim, or replay
    /// as their own, another operator's order.
    pub fn submit_combo_intent_as(
        &mut self,
        intent: ComboIntent,
        market: PaperComboMarketData,
        decided_at: &str,
        submitted_by: Option<&str>,
    ) -> Result<PaperSubmitOutcome, PaperError> {
        self.ensure_persistence_healthy()?;
        if let Some(operator) = submitted_by {
            validate_canonical_id("paper combo submitted_by", operator)?;
        }
        intent.validate()?;
        validate_utc_timestamp("paper combo risk decision time", decided_at)?;
        market.validate_for(&intent)?;
        if intent.environment != "PAPER" {
            return Err(PaperError(
                "paper OMS refuses a combination outside the PAPER environment".to_owned(),
            ));
        }
        if intent.account_id != self.account.account_id {
            return Err(PaperError(
                "paper combo intent account does not match service".to_owned(),
            ));
        }
        if !self.broker_connected {
            return Err(PaperError(
                "paper broker session is disconnected; reconnect and reconcile before submission"
                    .to_owned(),
            ));
        }

        let order_id = OmsComboOrder::order_id_for(&intent.intent_id);
        if let Some(existing) = self.combo_orders.get(&order_id) {
            if existing.oms.intent != intent {
                return Err(PaperError(
                    "paper combination idempotency key was reused with different intent data"
                        .to_owned(),
                ));
            }
            let evidence = self
                .combo_risk_evidence
                .get(&format!("paper-combo-risk-{}", intent.intent_id))
                .ok_or_else(|| {
                    PaperError(
                        "existing paper combination order is missing its risk evidence".to_owned(),
                    )
                })?;
            if evidence.market != market || evidence.decision.decided_at != decided_at {
                return Err(PaperError(
                    "paper combination retry must use the original market observation and decision time"
                        .to_owned(),
                ));
            }
            if evidence.submitted_by.as_deref() != submitted_by {
                return Err(PaperError(
                    "paper combination retry must come from the original submitter".to_owned(),
                ));
            }
            return Ok(PaperSubmitOutcome {
                decision: evidence.decision.clone(),
                order_id: Some(order_id),
                state: Some(existing.oms.state),
            });
        }

        self.ensure_route_carries(intent.time_in_force, true)?;
        // `evaluate_combo_risk` performs its own staleness check and updates
        // the mark cache, peak equity and daily baseline, exactly as the
        // single-order path relies on `evaluate_risk` to do.
        let decision = self.evaluate_combo_risk(&intent, &market, decided_at)?;
        let evidence = PaperComboRiskEvidence {
            intent: intent.clone(),
            decision: decision.clone(),
            market: market.clone(),
            submitted_by: submitted_by.map(str::to_owned),
        };
        if !decision.approved {
            self.combo_risk_evidence
                .insert(decision.decision_id.clone(), evidence);
            self.persist()?;
            return Ok(PaperSubmitOutcome {
                decision,
                order_id: None,
                state: None,
            });
        }

        let mut oms = OmsComboOrder::from_approved_intent(intent, &decision)?;
        oms.transition(OrderState::Approved, "PAPER_COMBO_RISK_APPROVED")?;
        oms.transition(
            OrderState::PendingSubmit,
            "PAPER_COMBO_SUBMISSION_REQUESTED",
        )?;
        let request = BrokerComboRequest::from_combo_order(&oms)?;
        self.combo_orders.insert(
            oms.order_id.clone(),
            PaperComboOrder {
                oms,
                broker_order_id: None,
                broker_order_versions: Vec::new(),
                market,
                filled_quantity: Decimal::ZERO,
                executions: BTreeMap::new(),
                evidence_error: None,
            },
        );
        self.combo_risk_evidence
            .insert(decision.decision_id.clone(), evidence);
        // Persist `PENDING_SUBMIT` before crossing the external broker boundary.
        self.persist()?;

        let submission = self.broker.submit_combo(&request);
        match submission {
            Ok(BrokerSubmitResult::Acknowledged { broker_order_id }) => {
                validate_canonical_id("broker order_id", &broker_order_id)?;
                let order = self.combo_order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "PAPER_COMBO_SUBMISSION_SENT")?;
                order
                    .oms
                    .transition(OrderState::Acknowledged, "IBKR_PAPER_COMBO_ACKNOWLEDGED")?;
                order.broker_order_id = Some(broker_order_id.clone());
                order.broker_order_versions.push(broker_order_id);
                let state = order.oms.state;
                self.persist()?;
                Ok(PaperSubmitOutcome {
                    decision,
                    order_id: Some(request.client_order_id),
                    state: Some(state),
                })
            }
            Ok(BrokerSubmitResult::Rejected { reason }) => {
                validate_broker_reason("broker rejection reason", &reason)?;
                let order = self.combo_order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Submitted, "PAPER_COMBO_SUBMISSION_SENT")?;
                order.oms.transition(OrderState::Rejected, reason)?;
                let state = order.oms.state;
                self.persist()?;
                Ok(PaperSubmitOutcome {
                    decision,
                    order_id: Some(request.client_order_id),
                    state: Some(state),
                })
            }
            Ok(BrokerSubmitResult::Unknown { reason }) => {
                validate_broker_reason("broker unknown-outcome reason", &reason)?;
                let order = self.combo_order_mut(&request.client_order_id)?;
                order.oms.transition(OrderState::Unknown, reason)?;
                self.persist()?;
                Ok(PaperSubmitOutcome {
                    decision,
                    order_id: Some(request.client_order_id),
                    state: Some(OrderState::Unknown),
                })
            }
            Err(error) => {
                // An adapter that cannot execute an atomic combination returns
                // an error here, and that is a *transport* outcome like any
                // other: the request may or may not have reached the venue, so
                // the combination goes to `UNKNOWN` and the session is marked
                // disconnected rather than assumed untouched.
                let order = self.combo_order_mut(&request.client_order_id)?;
                order
                    .oms
                    .transition(OrderState::Unknown, "PAPER_COMBO_TRANSPORT_OUTCOME_UNKNOWN")?;
                self.broker_connected = false;
                self.persist()?;
                Err(error)
            }
        }
    }

    /// The durable combination order for one client identity, if known.
    pub fn combo_order(&self, order_id: &str) -> Option<&PaperComboOrder> {
        self.combo_orders.get(order_id)
    }

    /// The immutable combination risk evidence for one decision identity.
    pub fn combo_risk_evidence(&self, decision_id: &str) -> Option<&PaperComboRiskEvidence> {
        self.combo_risk_evidence.get(decision_id)
    }

    pub(crate) fn combo_order_mut(
        &mut self,
        order_id: &str,
    ) -> Result<&mut PaperComboOrder, PaperError> {
        self.combo_orders
            .get_mut(order_id)
            .ok_or_else(|| PaperError("paper OMS does not know combination order".to_owned()))
    }

    /// Requests cancellation. A transport failure leaves the order explicitly `UNKNOWN`.
    pub fn cancel_order(&mut self, order_id: &str) -> Result<(), PaperError> {
        self.cancel_order_attributed(order_id, None)
    }

    /// Requests cancellation of a single or combination order for an
    /// authenticated operator. The request is journaled with the operator and
    /// time, together with the order's move to `PENDING_CANCEL` and before the
    /// broker is asked. A retry of a cancellation already accepted changes
    /// nothing and journals nothing, as a repeated kill-switch change does
    /// (delivery state E5.2b).
    pub fn cancel_order_as(
        &mut self,
        order_id: &str,
        operator: &str,
        operated_at: &str,
    ) -> Result<(), PaperError> {
        validate_canonical_id("order operator", operator)?;
        validate_utc_timestamp("order operation time", operated_at)?;
        self.cancel_order_attributed(
            order_id,
            Some(OrderOperation {
                order_id: order_id.to_owned(),
                action: OrderOperationAction::CancelRequested,
                operator: operator.to_owned(),
                operated_at: operated_at.to_owned(),
            }),
        )
    }

    fn cancel_order_attributed(
        &mut self,
        order_id: &str,
        operation: Option<OrderOperation>,
    ) -> Result<(), PaperError> {
        self.ensure_persistence_healthy()?;
        if self.combo_orders.contains_key(order_id) {
            return self.cancel_combo_order(order_id, operation);
        }
        if !self.broker_connected {
            return Err(PaperError(
                "paper broker session is disconnected; reconnect and reconcile before cancellation"
                    .to_owned(),
            ));
        }
        let state = self
            .orders
            .get(order_id)
            .ok_or_else(|| PaperError("paper OMS does not know order".to_owned()))?
            .oms
            .state;
        // A retry after the original cancellation was durably accepted or
        // completed is an idempotent success. Re-sending to the adapter could
        // manufacture an avoidable broker error and turn authoritative
        // cancellation evidence into ambiguity.
        if matches!(state, OrderState::PendingCancel | OrderState::Cancelled) {
            return Ok(());
        }
        if !matches!(
            state,
            OrderState::Acknowledged | OrderState::PartiallyFilled
        ) {
            return Err(PaperError(
                "only acknowledged or partially filled paper orders can be cancelled".to_owned(),
            ));
        }
        self.order_mut(order_id)?
            .oms
            .transition(OrderState::PendingCancel, "PAPER_CANCEL_REQUESTED")?;
        if let Some(operation) = operation {
            self.order_operations.push(operation);
        }
        // Durable before the external command, as submission and combination
        // cancellation are: a crash after the broker accepts the cancellation
        // cannot leave the journal saying the order still works, or losing
        // who asked for it (E5.2b).
        self.persist()?;
        let cancel = BrokerCancelRequest {
            account_id: self.account.account_id.clone(),
            client_order_id: order_id.to_owned(),
        };
        if let Err(error) = self.broker.cancel(&cancel) {
            self.order_mut(order_id)?
                .oms
                .transition(OrderState::Unknown, "PAPER_CANCEL_OUTCOME_UNKNOWN")?;
            self.broker_connected = false;
            self.persist()?;
            return Err(error);
        }
        Ok(())
    }

    /// Requests a price-only replacement that cannot increase the approved limit risk.
    ///
    /// The immutable order intent and client identity remain unchanged. A broker
    /// result is resolved asynchronously as `Replaced` or `ReplaceRejected`.
    pub fn replace_order(
        &mut self,
        order_id: &str,
        replacement_limit_price: Decimal,
    ) -> Result<(), PaperError> {
        self.ensure_persistence_healthy()?;
        if !self.broker_connected {
            return Err(PaperError(
                "paper broker session is disconnected; reconnect and reconcile before replacement"
                    .to_owned(),
            ));
        }
        // Before the order moves to `PENDING_REPLACE`: a route that cannot
        // replace would otherwise leave it `UNKNOWN` and the session
        // disconnected (delivery state E5.1).
        if !self
            .broker
            .capabilities(&self.account.account_id)?
            .replacement
        {
            return Err(PaperError(
                "the configured paper broker route cannot replace orders; the order was left unchanged"
                    .to_owned(),
            ));
        }
        let request = {
            let order = self.order_mut(order_id)?;
            if !matches!(
                order.oms.state,
                OrderState::Acknowledged | OrderState::PartiallyFilled
            ) {
                return Err(PaperError(
                    "only acknowledged or partially filled paper orders can be replaced".to_owned(),
                ));
            }
            let prior_limit = order.oms.intent.limit_price.ok_or_else(|| {
                PaperError("only paper limit orders support price replacement".to_owned())
            })?;
            if replacement_limit_price <= Decimal::ZERO
                || (order.oms.intent.side == Side::Buy && replacement_limit_price > prior_limit)
                || (order.oms.intent.side == Side::Sell && replacement_limit_price < prior_limit)
            {
                return Err(PaperError(
                    "replacement may only reduce the originally approved limit risk".to_owned(),
                ));
            }
            let previous_broker_order_id = order.broker_order_id.clone().ok_or_else(|| {
                PaperError("paper replacement requires an acknowledged broker order ID".to_owned())
            })?;
            order.replace_return_state = Some(order.oms.state);
            order
                .oms
                .transition(OrderState::PendingReplace, "PAPER_REPLACE_REQUESTED")?;
            BrokerReplaceRequest {
                account_id: order.oms.intent.account_id.clone(),
                client_order_id: order.oms.order_id.clone(),
                previous_broker_order_id,
                limit_price: replacement_limit_price,
            }
        };
        if let Err(error) = self.broker.replace(&request) {
            self.order_mut(order_id)?
                .oms
                .transition(OrderState::Unknown, "PAPER_REPLACE_OUTCOME_UNKNOWN")?;
            self.broker_connected = false;
            self.persist()?;
            return Err(error);
        }
        self.persist()
    }

    /// Drains broker evidence, applies unique executions, and preserves every ambiguity.
    pub fn synchronize(&mut self) -> Result<usize, PaperError> {
        self.ensure_persistence_healthy()?;
        if !self.broker_connected {
            return Err(PaperError(
                "paper broker session is disconnected".to_owned(),
            ));
        }
        let events = match self.broker.poll(&self.account.account_id) {
            Ok(events) => events,
            Err(error) => {
                self.broker_connected = false;
                self.persist()?;
                return Err(error);
            }
        };
        let count = events.len();
        let mut apply_errors = Vec::new();
        for event in events {
            // `poll` drains broker evidence, so an event that fails to apply can
            // never be re-fetched: aborting the whole loop on the first failure
            // would silently and permanently lose every event still queued
            // behind it. Snapshot the mutable state first so a failed
            // application leaves no partial mutation in memory, then keep
            // applying the remaining events in the batch.
            let orders_snapshot = self.orders.clone();
            let combo_snapshot = self.combo_orders.clone();
            let combo_id = event.client_order_id().to_owned();
            let portfolios_snapshot = self.portfolios.clone();
            let tax_lots_snapshot = self.tax_lots.clone();
            let strategy_attribution_snapshot = self.strategy_attribution.clone();
            let execution_ids_snapshot = self.execution_ids.clone();
            let cash_snapshot = self.cash;
            if let Err(error) = self.apply_broker_event(event) {
                self.orders = orders_snapshot;
                self.combo_orders = combo_snapshot;
                self.portfolios = portfolios_snapshot;
                self.tax_lots = tax_lots_snapshot;
                self.strategy_attribution = strategy_attribution_snapshot;
                self.execution_ids = execution_ids_snapshot;
                self.cash = cash_snapshot;
                if let Some(order) = self.combo_orders.get_mut(&combo_id) {
                    order.evidence_error = Some(error.0.clone());
                    if order.oms.state != OrderState::Unknown
                        && order.oms.state != OrderState::Filled
                    {
                        order
                            .oms
                            .transition(OrderState::Unknown, "COMBINATION_EXECUTION_ANOMALY")?;
                    }
                    self.persist()?;
                }
                apply_errors.push(error.0);
                continue;
            }
            // Broker evidence is append-only. Persist each arrival so resolving an
            // UNKNOWN or correcting an earlier terminal observation never rewrites
            // the durable history.
            self.persist()?;
        }
        if !apply_errors.is_empty() {
            return Err(PaperError(format!(
                "paper broker synchronize failed to apply {} of {count} broker events (each rolled back cleanly): {}",
                apply_errors.len(),
                apply_errors.join("; "),
            )));
        }
        Ok(count)
    }

    /// Reconnects, drains delayed broker evidence, then immediately reconciles.
    pub fn reconnect_and_reconcile(
        &mut self,
        reconciled_at: &str,
    ) -> Result<ReconciliationReport, PaperError> {
        self.ensure_persistence_healthy()?;
        if let Err(error) = self.broker.reconnect(&self.account.account_id) {
            self.broker_connected = false;
            self.persist()?;
            return Err(error);
        }
        self.broker_connected = true;
        self.synchronize()?;
        self.reconcile(reconciled_at)
    }

    /// Compares the broker snapshot with independent OMS, position, and cash state.
    pub fn reconcile(&mut self, reconciled_at: &str) -> Result<ReconciliationReport, PaperError> {
        self.ensure_persistence_healthy()?;
        validate_utc_timestamp("paper reconciliation time", reconciled_at)?;
        if !self.broker_connected {
            return Err(PaperError(
                "paper broker session is disconnected; reconnect before reconciliation".to_owned(),
            ));
        }
        let snapshot = match self.broker.snapshot(&self.account.account_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.broker_connected = false;
                self.persist()?;
                return Err(error);
            }
        };
        validate_broker_snapshot(&snapshot)?;
        let reconciliation_id = format!("reconciliation-{:08}", self.next_reconciliation);
        self.next_reconciliation += 1;
        let mut raw_issues = Vec::new();
        let mut broker_orders: BTreeMap<&str, Vec<&BrokerOrderSnapshot>> = BTreeMap::new();
        for broker_order in &snapshot.orders {
            broker_orders
                .entry(broker_order.client_order_id.as_str())
                .or_default()
                .push(broker_order);
        }
        let order_views = self
            .orders
            .iter()
            .map(|(id, o)| {
                (
                    id,
                    o.working(),
                    &o.broker_order_id,
                    &o.broker_order_versions,
                    o.filled_quantity,
                    o.oms.state,
                )
            })
            .chain(self.combo_orders.iter().map(|(id, o)| {
                (
                    id,
                    o.working(),
                    &o.broker_order_id,
                    &o.broker_order_versions,
                    o.filled_quantity,
                    o.oms.state,
                )
            }));
        for (order_id, working, broker_order_id, versions, filled_quantity, state) in order_views {
            match broker_orders.get(order_id.as_str()) {
                None if working => raw_issues.push((
                    "MISSING_BROKER_ORDER",
                    order_id.clone(),
                    "internal working order is absent from broker snapshot".to_owned(),
                )),
                Some(brokers) => {
                    let broker = broker_order_id.as_ref().and_then(|current| {
                        brokers
                            .iter()
                            .copied()
                            .find(|candidate| candidate.broker_order_id == *current)
                    });
                    if broker.is_none() {
                        raw_issues.push((
                            "BROKER_ORDER_ID_MISMATCH",
                            order_id.clone(),
                            format!(
                                "internal_current={:?},broker_versions={}",
                                broker_order_id,
                                brokers
                                    .iter()
                                    .map(|candidate| candidate.broker_order_id.as_str())
                                    .collect::<Vec<_>>()
                                    .join(",")
                            ),
                        ));
                    }
                    if brokers
                        .iter()
                        .any(|candidate| !versions.contains(&candidate.broker_order_id))
                    {
                        raw_issues.push((
                            "BROKER_ORDER_VERSION_MISMATCH",
                            order_id.clone(),
                            "broker snapshot includes an unrecognized broker order version"
                                .to_owned(),
                        ));
                    }
                    let broker_filled =
                        brokers.iter().try_fold(Decimal::ZERO, |total, candidate| {
                            total
                                .checked_add(candidate.filled_quantity)
                                .map_err(PaperError::from)
                        })?;
                    if filled_quantity != broker_filled {
                        raw_issues.push((
                            "FILLED_QUANTITY_MISMATCH",
                            order_id.clone(),
                            format!("internal={},broker={broker_filled}", filled_quantity,),
                        ));
                    }
                    if let Some(broker) = broker {
                        if state != broker.state {
                            raw_issues.push((
                                "ORDER_STATE_MISMATCH",
                                order_id.clone(),
                                format!(
                                    "internal={},broker={}",
                                    state.as_str(),
                                    broker.state.as_str()
                                ),
                            ));
                        }
                    }
                }
                None => {}
            }
        }
        for broker in &snapshot.orders {
            if !self.orders.contains_key(&broker.client_order_id)
                && !self.combo_orders.contains_key(&broker.client_order_id)
            {
                raw_issues.push((
                    "UNEXPECTED_BROKER_ORDER",
                    broker.client_order_id.clone(),
                    "broker snapshot contains no matching internal order".to_owned(),
                ));
            }
        }
        for (order_id, internal) in &self.combo_orders {
            if let Some(error) = &internal.evidence_error {
                raw_issues.push((
                    "COMBINATION_EXECUTION_ANOMALY",
                    order_id.clone(),
                    error.clone(),
                ));
            }
        }

        let broker_positions: BTreeMap<_, _> = snapshot
            .positions
            .iter()
            .map(|position| (position.instrument_id.as_str(), position.quantity))
            .collect();
        let instrument_ids: BTreeSet<_> = self
            .portfolios
            .keys()
            .map(String::as_str)
            .chain(broker_positions.keys().copied())
            .collect();
        for instrument_id in instrument_ids {
            let internal = self
                .portfolios
                .get(instrument_id)
                .map(|portfolio| portfolio.position_snapshot().quantity)
                .unwrap_or(Decimal::ZERO);
            let broker = broker_positions
                .get(instrument_id)
                .copied()
                .unwrap_or(Decimal::ZERO);
            if internal != broker {
                raw_issues.push((
                    "POSITION_QUANTITY_MISMATCH",
                    instrument_id.to_owned(),
                    format!("internal={internal},broker={broker}"),
                ));
            }
        }
        if self.cash != snapshot.cash {
            raw_issues.push((
                "CASH_MISMATCH",
                self.account.account_id.clone(),
                format!("internal={},broker={}", self.cash, snapshot.cash),
            ));
        }

        let issues = raw_issues
            .into_iter()
            .enumerate()
            .map(|(index, (category, subject, detail))| {
                let existing = self
                    .incidents
                    .values()
                    .find(|incident| {
                        incident.issue.category == category
                            && incident.issue.subject == subject
                            && incident.unexplained()
                    })
                    .map(|incident| incident.issue.incident_id.clone());
                let incident_id = existing.unwrap_or_else(|| {
                    format!(
                        "incident-{}-{:03}",
                        reconciliation_id.trim_start_matches("reconciliation-"),
                        index + 1
                    )
                });
                let issue = ReconciliationIssue {
                    incident_id: incident_id.clone(),
                    category: category.to_owned(),
                    subject,
                    detail,
                };
                self.incidents
                    .entry(incident_id)
                    .or_insert_with(|| ReconciliationIncident {
                        issue: issue.clone(),
                        explanation: None,
                    });
                issue
            })
            .collect();
        let report = ReconciliationReport {
            reconciliation_id,
            reconciled_at: reconciled_at.to_owned(),
            issues,
        };
        self.last_reconciled_at = Some(reconciled_at.to_owned());
        self.last_reconciliation_clean = Some(report.is_clean());
        self.latest_reconciliation = Some(report.clone());
        self.persist()?;
        Ok(report)
    }

    /// Records an attributable explanation for one reconciliation incident.
    pub fn explain_incident(
        &mut self,
        incident_id: &str,
        explanation: impl Into<String>,
    ) -> Result<(), PaperError> {
        self.ensure_persistence_healthy()?;
        validate_canonical_id("incident_id", incident_id)?;
        let explanation = explanation.into();
        if explanation.trim().is_empty() || explanation.len() > 1_024 {
            return Err(PaperError(
                "incident explanation must contain 1 to 1024 characters".to_owned(),
            ));
        }
        let incident = self
            .incidents
            .get_mut(incident_id)
            .ok_or_else(|| PaperError("unknown reconciliation incident".to_owned()))?;
        incident.explanation = Some(explanation);
        self.persist()?;
        Ok(())
    }

    /// Records one closed exchange session for the measured 30-paper-day gate.
    ///
    /// The caller must supply the exact session selected from its versioned exchange
    /// calendar. A checkpoint taken before the session closes cannot count.
    pub fn record_paper_session(
        &mut self,
        paper_session: &PaperTradingSession,
        report: &ReconciliationReport,
        calendar: &dyn TradingCalendar,
    ) -> Result<(), PaperError> {
        self.ensure_persistence_healthy()?;
        paper_session.validate()?;
        if calendar.calendar_id() != self.risk_policy.trading_calendar_id
            || calendar.calendar_id() != paper_session.calendar_id
            || calendar.session_for_exchange_date(&paper_session.session.exchange_date)
                != Some(&paper_session.session)
        {
            return Err(PaperError(
                "paper-day gate requires the exact session from the configured calendar".to_owned(),
            ));
        }
        if paper_session.calendar_id != self.risk_policy.trading_calendar_id {
            return Err(PaperError(
                "paper-session calendar does not match the configured paper calendar".to_owned(),
            ));
        }
        let expected_reconciliation_id = format!(
            "reconciliation-{:08}",
            self.next_reconciliation.checked_sub(1).ok_or_else(|| {
                PaperError("paper-session gate has no completed reconciliation".to_owned())
            })?
        );
        if self.last_reconciled_at.as_deref() != Some(report.reconciled_at.as_str())
            || self.last_reconciliation_clean != Some(report.is_clean())
            || self.latest_reconciliation.as_ref() != Some(report)
            || report.reconciliation_id != expected_reconciliation_id
            || !report.is_clean()
        {
            return Err(PaperError(
                "paper-session gate requires the latest actual reconciliation report".to_owned(),
            ));
        }
        if report.reconciled_at < paper_session.session.closes_at {
            return Err(PaperError(
                "paper-session reconciliation must occur at or after the session close".to_owned(),
            ));
        }
        if self.unexplained_incident_count() != 0 {
            return Err(PaperError(
                "paper-session gate refuses unresolved reconciliation incidents".to_owned(),
            ));
        }
        let exchange_date = &paper_session.session.exchange_date;
        let day = PersistentPaperDay {
            calendar_id: paper_session.calendar_id.clone(),
            session_opens_at: paper_session.session.opens_at.clone(),
            session_closes_at: paper_session.session.closes_at.clone(),
            clean: true,
        };
        match self.paper_days.get(exchange_date) {
            Some(existing) if *existing == day => Ok(()),
            Some(_) => Err(PaperError(
                "paper-session gate record cannot be overwritten with conflicting evidence"
                    .to_owned(),
            )),
            None => {
                self.paper_days.insert(exchange_date.to_owned(), day);
                self.persist()?;
                Ok(())
            }
        }
    }

    /// Returns the measured 30-day promotion state, never a predictive estimate.
    pub fn promotion_status(&self) -> PaperPromotionStatus {
        let clean_paper_days = self.paper_days.values().filter(|day| day.clean).count() as u32;
        let unexplained_incidents = self.unexplained_incident_count();
        let complete_auditability = self.persistence_healthy
            && self
                .journal
                .as_ref()
                .is_some_and(|journal| journal.sequence() > 0);
        PaperPromotionStatus {
            clean_paper_days,
            required_paper_days: 30,
            unexplained_incidents,
            complete_auditability,
            eligible_for_next_gate: clean_paper_days >= 30
                && unexplained_incidents == 0
                && complete_auditability,
        }
    }

    /// Creates a deterministic read-only dashboard projection for paper operations.
    pub fn dashboard(&self) -> PaperDashboard {
        let status = self.promotion_status();
        let positions = self
            .portfolios
            .iter()
            .map(|(instrument_id, portfolio)| {
                let position = portfolio.position_snapshot();
                PaperDashboardPosition {
                    instrument_id: instrument_id.clone(),
                    quantity: position.quantity.to_string(),
                    average_cost: position.average_cost.to_string(),
                    realized_pnl: position.realized_pnl.to_string(),
                }
            })
            .collect();
        PaperDashboard {
            dashboard_schema_version: 2,
            environment: self.account.environment.clone(),
            account_id: self.account.account_id.clone(),
            configuration_fingerprint: self.configuration_fingerprint(),
            broker_connected: self.broker_connected,
            persistence_healthy: self.persistence_healthy,
            audit_sequence: self
                .journal
                .as_ref()
                .map(FilePaperJournal::sequence)
                .unwrap_or(0),
            audit_head_hash: self
                .journal
                .as_ref()
                .map(|journal| journal.head_hash().to_owned())
                .unwrap_or_else(|| "0".repeat(64)),
            internal_cash: self.cash.to_string(),
            working_orders: self.working_order_count() as u32,
            unknown_orders: self
                .orders
                .values()
                .filter(|order| order.oms.state == OrderState::Unknown)
                .count() as u32,
            active_kill_switches: self.kill_switches.active_keys(),
            unexplained_incidents: status.unexplained_incidents,
            last_reconciled_at: self.last_reconciled_at.clone(),
            last_reconciliation_clean: self.last_reconciliation_clean,
            clean_paper_days: status.clean_paper_days,
            required_paper_days: status.required_paper_days,
            promotion_eligible: status.eligible_for_next_gate,
            complete_auditability: status.complete_auditability,
            positions,
        }
    }

    /// Returns canonical JSON for a server-owned read-only dashboard stream.
    pub fn canonical_dashboard_json(&self) -> Result<String, PaperError> {
        serde_json::to_string(&self.dashboard()).map_err(|error| PaperError(error.to_string()))
    }

    pub(crate) fn order_mut(&mut self, order_id: &str) -> Result<&mut PaperOrder, PaperError> {
        self.orders
            .get_mut(order_id)
            .ok_or_else(|| PaperError("paper OMS does not know order".to_owned()))
    }

    fn evaluate_risk(
        &mut self,
        intent: &OrderIntent,
        market: &PaperMarketData,
        decided_at: &str,
    ) -> Result<RiskDecision, PaperError> {
        self.marks
            .insert(intent.instrument_id.clone(), market.mark_price);
        // Peak-equity tracking runs unconditionally, independent of whether
        // Slice-2 composition is even configured, so enabling it later does
        // not start drawdown tracking from a fresh, artificially favorable
        // baseline -- the same reasoning as the unconditional `marks` update
        // above.
        let observed_equity = self.current_equity()?;
        if observed_equity > self.peak_equity {
            self.peak_equity = observed_equity;
        }
        // Session-start daily-loss baseline: reset (not maxed) whenever the
        // UTC calendar date of this decision differs from the stored
        // baseline date -- including the very first evaluation ever
        // (`daily_baseline_date` starts `None`). `decided_at` is already
        // `validate_utc_timestamp`-checked by every caller (canonical
        // second-precision UTC, `YYYY-MM-DDTHH:MM:SSZ`), so its first 10
        // bytes are exactly its UTC calendar date.
        let decision_date = &decided_at[..10];
        if self.daily_baseline_date.as_deref() != Some(decision_date) {
            self.daily_baseline_date = Some(decision_date.to_owned());
            self.daily_baseline_equity = observed_equity;
        }
        let current_position = self
            .portfolios
            .get(&intent.instrument_id)
            .map(|portfolio| portfolio.position_snapshot().quantity)
            .unwrap_or(Decimal::ZERO);
        let realized_pnl = self
            .portfolios
            .values()
            .try_fold(Decimal::ZERO, |total, portfolio| {
                total.checked_add(portfolio.position_snapshot().realized_pnl)
            })?;
        let reserved_cash = self.total_reserved_cash()?;
        let context = PaperRiskContext {
            open_orders: self.working_order_count(),
            position_quantity: current_position,
            available_cash: self.cash.checked_sub(reserved_cash)?,
            realized_pnl,
        };
        let estimated_notional = intent.quantity.checked_mul(market.mark_price)?;
        let requested_price_deviation_bps = intent
            .limit_price
            .map(|price| price_deviation_bps(market.mark_price, price))
            .transpose()?
            .unwrap_or(Decimal::ZERO);
        let projected_position = match intent.side {
            Side::Buy => context.position_quantity.checked_add(intent.quantity)?,
            Side::Sell => context.position_quantity.checked_sub(intent.quantity)?,
        };
        let recent_order_count = self.recent_order_count(decided_at)?;
        let mut reasons = self.kill_switches.rejection_reasons(intent);
        if intent.quantity > self.risk_policy.max_order_quantity {
            reasons.push("MAX_ORDER_QUANTITY_EXCEEDED".to_owned());
        }
        if estimated_notional > self.risk_policy.max_order_notional {
            reasons.push("MAX_ORDER_NOTIONAL_EXCEEDED".to_owned());
        }
        if requested_price_deviation_bps > self.risk_policy.max_price_deviation_bps {
            reasons.push("PRICE_COLLAR_EXCEEDED".to_owned());
        }
        if let Some(reason) = self
            .risk_policy
            .tick_rejection(&intent.instrument_id, intent.limit_price)
        {
            reasons.push(reason.to_owned());
        }
        if let Some(reason) = self
            .risk_policy
            .lot_rejection(&intent.instrument_id, intent.quantity)
        {
            reasons.push(reason.to_owned());
        }
        if self.conflicts_with_working_order(&intent.instrument_id, intent.side) {
            reasons.push("SELF_TRADE_RISK".to_owned());
        }
        if recent_order_count >= self.risk_policy.max_order_rate {
            reasons.push("MAX_ORDER_RATE_EXCEEDED".to_owned());
        }
        if self.has_unknown_order() {
            reasons.push("UNKNOWN_ORDER_REQUIRES_RECONCILIATION".to_owned());
        }
        if self.unexplained_incident_count() > 0 {
            reasons.push("UNEXPLAINED_INCIDENTS_REQUIRE_REVIEW".to_owned());
        }
        if context.open_orders >= self.risk_policy.max_open_orders {
            reasons.push("MAX_OPEN_ORDERS_EXCEEDED".to_owned());
        }
        if self
            .risk_policy
            .breaches_position_limit(projected_position)?
        {
            reasons.push("POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned());
        }
        if intent.side == Side::Buy && estimated_notional > context.available_cash {
            reasons.push("INSUFFICIENT_INTERNAL_CASH".to_owned());
        }
        let realized_loss = if context.realized_pnl < Decimal::ZERO {
            Decimal::ZERO.checked_sub(context.realized_pnl)?
        } else {
            Decimal::ZERO
        };
        if realized_loss > self.risk_policy.max_realized_loss {
            reasons.push("MAX_REALIZED_LOSS_EXCEEDED".to_owned());
        }
        let mut portfolio_risk_limits = String::new();
        if let Some(composition) = self.risk_policy.portfolio_risk.as_ref() {
            if let Some((decision, margin_used)) =
                self.portfolio_risk_decision(composition, intent, market, decided_at)?
            {
                reasons.extend(
                    decision
                        .reason_codes
                        .into_iter()
                        // `SELF_TRADE_RISK` is already independently detected above from
                        // the same working-order state; every other reason this composed
                        // kernel can produce is new coverage (see `PortfolioRiskComposition`).
                        .filter(|reason| reason != "APPROVED" && reason != "SELF_TRADE_RISK"),
                );
                portfolio_risk_limits = format!(
                    ",portfolio_risk_policy_version={},portfolio_gross_exposure={},portfolio_net_exposure={},portfolio_leverage_bps={},portfolio_concentration_bps={},portfolio_peak_equity={},portfolio_drawdown_bps={},portfolio_daily_baseline_equity={},portfolio_daily_pnl={},portfolio_margin_used={},portfolio_margin_utilization_bps={},portfolio_sector_gross={},portfolio_asset_class_gross={},portfolio_currency_gross={},portfolio_strategy_gross={}",
                    decision.policy_version,
                    decision.metrics.gross_exposure,
                    decision.metrics.net_exposure,
                    decision.metrics.leverage_bps,
                    decision.metrics.concentration_bps,
                    self.peak_equity,
                    decision.metrics.drawdown_bps,
                    self.daily_baseline_equity,
                    observed_equity.checked_sub(self.daily_baseline_equity)?,
                    margin_used,
                    decision.metrics.margin_utilization_bps,
                    render_bucket_map(&decision.metrics.sector_gross),
                    render_bucket_map(&decision.metrics.asset_class_gross),
                    render_bucket_map(&decision.metrics.currency_gross),
                    render_bucket_map(&decision.metrics.strategy_gross),
                );
            }
        }
        let approved = reasons.is_empty();
        if approved {
            reasons.push("APPROVED".to_owned());
        }
        Ok(RiskDecision {
            decision_id: format!("paper-risk-{}", intent.intent_id),
            intent_id: intent.intent_id.clone(),
            approved,
            reason_codes: reasons,
            policy_version: self.risk_policy.version.clone(),
            decided_at: decided_at.to_owned(),
            correlation_id: intent.correlation_id.clone(),
            actor: "paper_risk_engine".to_owned(),
            evaluated_limits: format!(
                "max_order_quantity={},max_order_notional={},max_price_deviation_bps={},max_open_orders={},max_position_quantity={},max_realized_loss={},max_market_data_age_seconds={},max_order_rate={},order_rate_window_seconds={},recent_order_count={},market_instrument_id={},market_mark_price={},market_observed_at={},requested_price={},requested_price_deviation_bps={},estimated_notional={},projected_position={},available_cash={},instrument_tick_size={},instrument_lot_size={}{}",
                self.risk_policy.max_order_quantity,
                self.risk_policy.max_order_notional,
                self.risk_policy.max_price_deviation_bps,
                self.risk_policy.max_open_orders,
                self.risk_policy.max_position_quantity,
                self.risk_policy.max_realized_loss,
                self.risk_policy.max_market_data_age_seconds,
                self.risk_policy.max_order_rate,
                self.risk_policy.order_rate_window_seconds,
                recent_order_count,
                market.instrument_id,
                market.mark_price,
                market.observed_at,
                intent.limit_price.map_or_else(|| "MARKET".to_owned(), |price| price.to_string()),
                requested_price_deviation_bps,
                estimated_notional,
                projected_position,
                context.available_cash,
                self.risk_policy
                    .instrument_tick_sizes
                    .get(&intent.instrument_id)
                    .map_or_else(|| "UNCONFIGURED".to_owned(), ToString::to_string),
                self.risk_policy
                    .instrument_lot_sizes
                    .get(&intent.instrument_id)
                    .map_or_else(|| "UNCONFIGURED".to_owned(), ToString::to_string),
                portfolio_risk_limits,
            ),
        })
    }

    /// Assesses a multi-leg combination against the full paper risk policy.
    ///
    /// This is assessment only: it creates no order, contacts no broker, and
    /// leaves the OMS untouched. It exists as a separate entry point rather
    /// than a variant of the single-order gate because a combination is one
    /// economic unit with several instruments, and almost every rule has to be
    /// restated in those terms — the notional is the *gross* of all legs, the
    /// price collar is per leg against that leg's own mark, and the position
    /// projection is per instrument.
    ///
    /// Two things are deliberately *not* restated, because a combination is one
    /// order: it counts once against the open-order limit and once against the
    /// order-rate limit.
    pub fn evaluate_combo_risk(
        &mut self,
        intent: &ComboIntent,
        market: &PaperComboMarketData,
        decided_at: &str,
    ) -> Result<RiskDecision, PaperError> {
        intent.validate()?;
        validate_utc_timestamp("paper combo risk decision time", decided_at)?;
        if intent.combo_quantity.scaled() % follon_domain::DECIMAL_SCALE != 0 {
            return Err(PaperError(
                "PAPER combinations require whole combination units".to_owned(),
            ));
        }
        market.validate_for(intent)?;
        if intent.account_id != self.account.account_id {
            return Err(PaperError(
                "paper combo intent account does not match service".to_owned(),
            ));
        }
        // Staleness is a hard error rather than a rejection reason, matching
        // `submit_intent`: a decision made against an observation the policy
        // considers unusable is not a "no", it is not a decision at all, and
        // must not be recorded as evidence of one.
        let observed_at = OffsetDateTime::parse(market.oldest_observed_at()?, &Rfc3339)
            .map_err(|error| PaperError(error.to_string()))?;
        let decision_time = OffsetDateTime::parse(decided_at, &Rfc3339)
            .map_err(|error| PaperError(error.to_string()))?;
        let age = (decision_time - observed_at).whole_seconds();
        if age < 0
            || u64::try_from(age).unwrap_or(u64::MAX) > self.risk_policy.max_market_data_age_seconds
        {
            return Err(PaperError(
                "paper combo observation is stale or later than the risk decision".to_owned(),
            ));
        }

        // Unconditional, exactly as the single-order gate does it: the mark
        // cache, the peak-equity high-water-mark and the daily-loss baseline
        // must not depend on whether aggregate composition happens to be
        // configured, or enabling it later would start from an artificially
        // favourable baseline.
        for mark in &market.marks {
            self.marks
                .insert(mark.instrument_id.clone(), mark.mark_price);
        }
        let observed_equity = self.current_equity()?;
        if observed_equity > self.peak_equity {
            self.peak_equity = observed_equity;
        }
        let decision_date = &decided_at[..10];
        if self.daily_baseline_date.as_deref() != Some(decision_date) {
            self.daily_baseline_date = Some(decision_date.to_owned());
            self.daily_baseline_equity = observed_equity;
        }

        let reserved_cash = self.total_reserved_cash()?;
        let available_cash = self.cash.checked_sub(reserved_cash)?;
        let realized_pnl = self
            .portfolios
            .values()
            .try_fold(Decimal::ZERO, |total, portfolio| {
                total.checked_add(portfolio.position_snapshot().realized_pnl)
            })?;
        let open_orders = self.working_order_count();
        let recent_order_count = self.recent_order_count(decided_at)?;

        let gross_notional = intent.gross_notional()?;
        let net_price = intent.protected_net_price()?;
        // A debit is cash out the door now; a credit is not cash in that may be
        // spent, so only a debit is charged against available cash. The short
        // leg's obligation is covered by the gross-notional and aggregate
        // limits, not by this check.
        let net_debit = if net_price > Decimal::ZERO {
            net_price.checked_mul(intent.combo_quantity)?
        } else {
            Decimal::ZERO
        };

        let mut reasons = self.kill_switches.combo_rejection_reasons(intent);
        reasons.extend(
            self.risk_policy
                .combo_tick_rejections(intent)
                .into_iter()
                .map(str::to_owned),
        );

        // Each leg's own contract quantity is what the broker sees, so the
        // per-order quantity limit binds the largest leg rather than the
        // combination unit count. A ten-lot butterfly is not a ten-lot order.
        let mut largest_leg_quantity = Decimal::ZERO;
        let mut widest_leg_deviation_bps = Decimal::ZERO;
        let mut leg_evidence = Vec::with_capacity(intent.legs.len());
        for leg in &intent.legs {
            let mark = market.mark_for(&leg.instrument_id).ok_or_else(|| {
                PaperError(format!(
                    "paper combo observation is missing a mark for {}",
                    leg.instrument_id
                ))
            })?;
            let leg_quantity = intent.leg_quantity(leg)?;
            // The lot rule binds what the broker sees, each leg's own contract
            // quantity, exactly as it binds a plain order's (E3.6c).
            if let Some(reason) = self
                .risk_policy
                .lot_rejection(&leg.instrument_id, leg_quantity)
            {
                reasons.push(reason.to_owned());
            }
            largest_leg_quantity = largest_leg_quantity.max(leg_quantity);
            let deviation_bps = price_deviation_bps(mark.mark_price, leg.limit_price)?;
            widest_leg_deviation_bps = widest_leg_deviation_bps.max(deviation_bps);

            let held = self
                .portfolios
                .get(&leg.instrument_id)
                .map(|portfolio| portfolio.position_snapshot().quantity)
                .unwrap_or(Decimal::ZERO);
            let projected = held.checked_add(intent.projected_leg_delta(leg)?)?;
            if self.risk_policy.breaches_position_limit(projected)? {
                reasons.push("POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED".to_owned());
            }
            // Self-trade is assessed per leg against every working order, and a
            // breach on any one leg rejects the whole structure: the group is
            // atomic, so there is no version of it that omits the offending leg.
            if self.conflicts_with_working_order(&leg.instrument_id, leg.side) {
                reasons.push("SELF_TRADE_RISK".to_owned());
            }
            leg_evidence.push(format!(
                "{}:{}:{}:{}:{}:{}",
                leg.instrument_id,
                leg.side.as_str(),
                leg_quantity,
                leg.limit_price,
                mark.mark_price,
                deviation_bps
            ));
        }

        if largest_leg_quantity > self.risk_policy.max_order_quantity {
            reasons.push("MAX_ORDER_QUANTITY_EXCEEDED".to_owned());
        }
        if gross_notional > self.risk_policy.max_order_notional {
            reasons.push("MAX_ORDER_NOTIONAL_EXCEEDED".to_owned());
        }
        if widest_leg_deviation_bps > self.risk_policy.max_price_deviation_bps {
            reasons.push("PRICE_COLLAR_EXCEEDED".to_owned());
        }
        if recent_order_count >= self.risk_policy.max_order_rate {
            reasons.push("MAX_ORDER_RATE_EXCEEDED".to_owned());
        }
        if self.has_unknown_order() {
            reasons.push("UNKNOWN_ORDER_REQUIRES_RECONCILIATION".to_owned());
        }
        if self.unexplained_incident_count() > 0 {
            reasons.push("UNEXPLAINED_INCIDENTS_REQUIRE_REVIEW".to_owned());
        }
        if open_orders >= self.risk_policy.max_open_orders {
            reasons.push("MAX_OPEN_ORDERS_EXCEEDED".to_owned());
        }
        if net_debit > available_cash {
            reasons.push("INSUFFICIENT_INTERNAL_CASH".to_owned());
        }
        let realized_loss = if realized_pnl < Decimal::ZERO {
            Decimal::ZERO.checked_sub(realized_pnl)?
        } else {
            Decimal::ZERO
        };
        if realized_loss > self.risk_policy.max_realized_loss {
            reasons.push("MAX_REALIZED_LOSS_EXCEEDED".to_owned());
        }

        let mut portfolio_risk_limits = String::new();
        if let Some(composition) = self.risk_policy.portfolio_risk.as_ref() {
            if let Some((decision, margin_used)) =
                self.combo_portfolio_risk_decision(composition, intent, market, decided_at)?
            {
                reasons.extend(
                    decision
                        .reason_codes
                        .into_iter()
                        // `SELF_TRADE_RISK` is already detected per leg above
                        // from the same working-order state.
                        .filter(|reason| reason != "APPROVED" && reason != "SELF_TRADE_RISK"),
                );
                portfolio_risk_limits = format!(
                    ",portfolio_risk_policy_version={},portfolio_gross_exposure={},portfolio_net_exposure={},portfolio_leverage_bps={},portfolio_concentration_bps={},portfolio_peak_equity={},portfolio_drawdown_bps={},portfolio_daily_baseline_equity={},portfolio_daily_pnl={},portfolio_margin_used={},portfolio_margin_utilization_bps={}",
                    decision.policy_version,
                    decision.metrics.gross_exposure,
                    decision.metrics.net_exposure,
                    decision.metrics.leverage_bps,
                    decision.metrics.concentration_bps,
                    self.peak_equity,
                    decision.metrics.drawdown_bps,
                    self.daily_baseline_equity,
                    observed_equity.checked_sub(self.daily_baseline_equity)?,
                    margin_used,
                    decision.metrics.margin_utilization_bps,
                );
            }
        }

        reasons.sort();
        reasons.dedup();
        let approved = reasons.is_empty();
        if approved {
            reasons.push("APPROVED".to_owned());
        }
        Ok(RiskDecision {
            // A distinct prefix from the single-order gate's `paper-risk-`, so
            // a combination decision can never be mistaken for, or collide
            // with, a plain order's decision under the same intent identity.
            decision_id: format!("paper-combo-risk-{}", intent.intent_id),
            intent_id: intent.intent_id.clone(),
            approved,
            reason_codes: reasons,
            policy_version: self.risk_policy.version.clone(),
            decided_at: decided_at.to_owned(),
            correlation_id: intent.correlation_id.clone(),
            actor: "paper_risk_engine".to_owned(),
            evaluated_limits: format!(
                "combo_legs={},combo_quantity={},combo_price_limit_kind={},combo_price_limit_amount={},combo_protected_net_price={},combo_net_debit={},combo_gross_notional={},largest_leg_quantity={},widest_leg_deviation_bps={},max_order_quantity={},max_order_notional={},max_price_deviation_bps={},max_open_orders={},max_position_quantity={},max_realized_loss={},max_market_data_age_seconds={},max_order_rate={},order_rate_window_seconds={},recent_order_count={},available_cash={},oldest_observed_at={},legs=[{}],combo_tick_sizes=[{}],combo_lot_sizes=[{}]{}",
                intent.legs.len(),
                intent.combo_quantity,
                intent.price_limit.kind(),
                intent.price_limit.amount(),
                net_price,
                net_debit,
                gross_notional,
                largest_leg_quantity,
                widest_leg_deviation_bps,
                self.risk_policy.max_order_quantity,
                self.risk_policy.max_order_notional,
                self.risk_policy.max_price_deviation_bps,
                self.risk_policy.max_open_orders,
                self.risk_policy.max_position_quantity,
                self.risk_policy.max_realized_loss,
                self.risk_policy.max_market_data_age_seconds,
                self.risk_policy.max_order_rate,
                self.risk_policy.order_rate_window_seconds,
                recent_order_count,
                available_cash,
                market.oldest_observed_at()?,
                leg_evidence.join("|"),
                self.risk_policy.combo_tick_evidence(intent),
                self.risk_policy.combo_lot_evidence(intent),
                portfolio_risk_limits,
            ),
        })
    }

    /// Counts orders whose own risk decision falls inside the rate window.
    ///
    /// Rate-window membership is keyed off each order's own risk *decision*
    /// time (`RiskDecision::decided_at`, sourced from `risk_evidence`), not the
    /// caller-supplied `OrderIntent::created_at`. `created_at` is stamped by
    /// the strategy or operator that originated the intent and is not otherwise
    /// constrained to reflect wall-clock reality, so counting against it would
    /// let a caller understate its own submission rate and silently bypass
    /// `MAX_ORDER_RATE_EXCEEDED` by backdating `created_at` on new intents.
    pub(crate) fn recent_order_count(&self, decided_at: &str) -> Result<u32, PaperError> {
        let decision_time = OffsetDateTime::parse(decided_at, &Rfc3339)
            .map_err(|error| PaperError(error.to_string()))?;
        let rate_window_start = decision_time
            - time::Duration::seconds(self.risk_policy.order_rate_window_seconds as i64);
        let within_window = |order_decided_at: &str| -> Result<bool, PaperError> {
            let parsed = OffsetDateTime::parse(order_decided_at, &Rfc3339)
                .map_err(|error| PaperError(error.to_string()))?;
            Ok(parsed > rate_window_start && parsed <= decision_time)
        };
        let mut count = 0u32;
        for order in self.orders.values() {
            let decision_id = format!("paper-risk-{}", order.oms.intent.intent_id);
            let order_decided_at = self
                .risk_evidence
                .get(&decision_id)
                .map(|evidence| evidence.decision.decided_at.as_str())
                .ok_or_else(|| {
                    PaperError("paper order is missing its originating risk evidence".to_owned())
                })?;
            if within_window(order_decided_at)? {
                count += 1;
            }
        }
        // A combination counts once, not once per leg: it is one broker
        // submission. Leaving it out entirely would let an operator submit an
        // unlimited number of combinations inside a rate window that a plain
        // order would be refused in.
        for order in self.combo_orders.values() {
            let decision_id = format!("paper-combo-risk-{}", order.oms.intent.intent_id);
            let order_decided_at = self
                .combo_risk_evidence
                .get(&decision_id)
                .map(|evidence| evidence.decision.decided_at.as_str())
                .ok_or_else(|| {
                    PaperError(
                        "paper combination order is missing its originating risk evidence"
                            .to_owned(),
                    )
                })?;
            if within_window(order_decided_at)? {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Non-terminal orders of both kinds. A combination counts once.
    pub(crate) fn working_order_count(&self) -> usize {
        self.orders.values().filter(|order| order.working()).count()
            + self
                .combo_orders
                .values()
                .filter(|order| order.working())
                .count()
    }

    /// Whether any order of either kind is in the `UNKNOWN` safety state.
    pub(crate) fn has_unknown_order(&self) -> bool {
        self.orders
            .values()
            .any(|order| order.oms.state == OrderState::Unknown)
            || self.combo_orders.values().any(|order| {
                order.oms.state == OrderState::Unknown || order.evidence_error.is_some()
            })
    }

    /// Cash committed by every working order of either kind.
    pub(crate) fn total_reserved_cash(&self) -> Result<Decimal, PaperError> {
        let mut reserved = Decimal::ZERO;
        for order in self.orders.values() {
            reserved = reserved.checked_add(order.reserved_cash()?)?;
        }
        for order in self.combo_orders.values() {
            reserved = reserved.checked_add(order.reserved_cash()?)?;
        }
        Ok(reserved)
    }

    /// Whether a working order of either kind would trade against `side` on
    /// `instrument_id`.
    ///
    /// A combination's legs count individually here: a working short leg is a
    /// real resting sell on that instrument however the group is labelled, and
    /// a plain buy submitted against it is the same self-trade it would be
    /// against a plain sell.
    pub(crate) fn conflicts_with_working_order(&self, instrument_id: &str, side: Side) -> bool {
        self.orders.values().any(|order| {
            order.working()
                && order.oms.intent.instrument_id == instrument_id
                && order.oms.intent.side != side
        }) || self.combo_orders.values().any(|order| {
            order.working()
                && order
                    .oms
                    .intent
                    .legs
                    .iter()
                    .any(|leg| leg.instrument_id == instrument_id && leg.side != side)
        })
    }

    /// Real point-in-time equity: cash plus every non-zero position marked at
    /// its cached observed mark, falling back to average cost when this
    /// instrument has never been independently quoted. Shared by peak-equity
    /// tracking (always) and the Slice-1/2 aggregate-risk snapshot (when
    /// composed).
    fn current_equity(&self) -> Result<Decimal, PaperError> {
        let mut equity = self.cash;
        for portfolio in self.portfolios.values() {
            let snapshot = portfolio.position_snapshot();
            if snapshot.quantity == Decimal::ZERO {
                continue;
            }
            let mark = self
                .marks
                .get(&snapshot.instrument_id)
                .copied()
                .unwrap_or(snapshot.average_cost);
            equity = equity.checked_add(snapshot.quantity.checked_mul(mark)?)?;
        }
        Ok(equity)
    }

    /// Builds the aggregate-risk snapshot/candidate from real service state
    /// and calls the composed `core/risk` kernel. Returns `Ok(None)` when
    /// computed equity is not yet positive (e.g. a brand-new, zero-funded,
    /// zero-position account) -- a benign boundary condition, not an error;
    /// the existing per-order checks still apply on their own. On `Some`, the
    /// second tuple element is the real margin requirement computed for the
    /// decision (`Decimal::ZERO` when `margin_rates` is not configured),
    /// returned alongside the decision because `core/risk::AggregateRiskMetrics`
    /// only ever reports the *ratio* (`margin_utilization_bps`), not the raw
    /// currency amount that produced it.
    fn portfolio_risk_decision(
        &self,
        composition: &PortfolioRiskComposition,
        intent: &OrderIntent,
        market: &PaperMarketData,
        decided_at: &str,
    ) -> Result<Option<(follon_risk::PortfolioRiskDecision, Decimal)>, PaperError> {
        let Some((snapshot, margin_used)) = self.portfolio_risk_state(composition, decided_at)?
        else {
            return Ok(None);
        };
        let candidate = self.risk_candidate(
            composition,
            &intent.intent_id,
            &intent.account_id,
            &intent.strategy_id,
            &intent.instrument_id,
            intent.side,
            intent.quantity,
            market.mark_price,
        )?;
        let decision = follon_risk::evaluate_portfolio_risk_with_candidates(
            &composition.policy,
            &snapshot,
            std::slice::from_ref(&candidate),
        )?;
        Ok(Some((decision, margin_used)))
    }

    /// Builds the real aggregate-risk snapshot from service state, without any
    /// candidate.
    ///
    /// Extracted so the single-order and combination paths observe exactly the
    /// same portfolio, equity, peak, daily baseline and margin figures; the
    /// only difference between them is which candidates are then added.
    fn portfolio_risk_state(
        &self,
        composition: &PortfolioRiskComposition,
        decided_at: &str,
    ) -> Result<Option<(PortfolioRiskSnapshot, Decimal)>, PaperError> {
        let equity = self.current_equity()?;
        if equity <= Decimal::ZERO {
            return Ok(None);
        }
        let mut positions = Vec::new();
        for portfolio in self.portfolios.values() {
            let snapshot = portfolio.position_snapshot();
            if snapshot.quantity == Decimal::ZERO {
                continue;
            }
            let mark = self
                .marks
                .get(&snapshot.instrument_id)
                .copied()
                .unwrap_or(snapshot.average_cost);
            let bucket = composition.instrument_buckets.get(&snapshot.instrument_id);
            let asset_class = bucket
                .map(|bucket| bucket.asset_class.clone())
                .unwrap_or_else(|| "unclassified".to_owned());
            let sector = bucket
                .map(|bucket| bucket.sector.clone())
                .unwrap_or_else(|| "unclassified".to_owned());
            let currency = bucket
                .map(|bucket| bucket.currency.clone())
                .unwrap_or_else(|| self.account.currency.clone());
            // Slice 2d: split the aggregate position into one `RiskPosition`
            // row per strategy that has ever traded this instrument, plus
            // one "unattributed" remainder row for whatever the tracked
            // strategies do not account for (a legacy journal, or a fill
            // predating this ledger). Every row's quantity always
            // reconciles exactly against `snapshot.quantity` (see
            // `apply_strategy_attribution_fill`), so gross/net exposure can
            // never be mis-stated by this split, only how it is attributed.
            let mut attributed_total = Decimal::ZERO;
            if let Some(strategies) = self.strategy_attribution.get(&snapshot.instrument_id) {
                for (strategy_id, quantity) in strategies {
                    if *quantity == Decimal::ZERO {
                        continue;
                    }
                    attributed_total = attributed_total.checked_add(*quantity)?;
                    positions.push(RiskPosition {
                        account_id: snapshot.account_id.clone(),
                        strategy_id: strategy_id.clone(),
                        instrument_id: snapshot.instrument_id.clone(),
                        asset_class: asset_class.clone(),
                        sector: sector.clone(),
                        currency: currency.clone(),
                        quantity: *quantity,
                        mark_price: mark,
                        multiplier: Decimal::from_integer(1)?,
                        delta: Decimal::ZERO,
                        gamma: Decimal::ZERO,
                    });
                }
            }
            let remainder = snapshot.quantity.checked_sub(attributed_total)?;
            if remainder != Decimal::ZERO {
                positions.push(RiskPosition {
                    account_id: snapshot.account_id,
                    strategy_id: "unattributed".to_owned(),
                    instrument_id: snapshot.instrument_id,
                    asset_class,
                    sector,
                    currency,
                    quantity: remainder,
                    mark_price: mark,
                    multiplier: Decimal::from_integer(1)?,
                    delta: Decimal::ZERO,
                    gamma: Decimal::ZERO,
                });
            }
        }
        let mut resting_orders = self
            .orders
            .values()
            .filter(|order| order.working())
            .map(|order| RestingOrder {
                order_id: order.oms.order_id.clone(),
                account_id: order.oms.intent.account_id.clone(),
                instrument_id: order.oms.intent.instrument_id.clone(),
                side: order.oms.intent.side,
            })
            .collect::<Vec<_>>();
        // A working combination contributes one resting row *per leg*, because
        // the aggregate kernel's self-trade check is per instrument and a
        // working short leg is a real resting sell on that instrument however
        // the group is labelled. Every one of those rows carries the same
        // `order_id`, and the kernel counts *distinct* order identities against
        // `max_open_orders`, so a four-leg combination stays one open order.
        for order in self.combo_orders.values().filter(|order| order.working()) {
            for leg in &order.oms.intent.legs {
                resting_orders.push(RestingOrder {
                    order_id: order.oms.order_id.clone(),
                    account_id: order.oms.intent.account_id.clone(),
                    instrument_id: leg.instrument_id.clone(),
                    side: leg.side,
                });
            }
        }
        // Real, computed only when `margin_rates` is configured (Slice 2c):
        // the *currently held* margin requirement, not a projection that
        // includes the candidate order -- the same "pre-trade observed, not
        // post-trade projected" convention `equity`/`peak_equity`/
        // `daily_baseline_equity` already use above. Every asset class among
        // currently held positions must have a configured rate or this fails
        // closed with a technical error rather than silently under-counting
        // margin -- an intentional operator-configuration requirement, not a
        // soft risk rejection.
        let margin_used = if let Some(rates) = composition.margin_rates.as_ref() {
            let mut margin_positions = Vec::new();
            for portfolio in self.portfolios.values() {
                let snapshot = portfolio.position_snapshot();
                if snapshot.quantity == Decimal::ZERO {
                    continue;
                }
                let mark = self
                    .marks
                    .get(&snapshot.instrument_id)
                    .copied()
                    .unwrap_or(snapshot.average_cost);
                let bucket = composition.instrument_buckets.get(&snapshot.instrument_id);
                margin_positions.push(MarginPosition {
                    instrument_id: snapshot.instrument_id,
                    asset_class: bucket
                        .map(|bucket| bucket.asset_class.clone())
                        .unwrap_or_else(|| "unclassified".to_owned()),
                    currency: Currency::new(
                        bucket
                            .map(|bucket| bucket.currency.clone())
                            .unwrap_or_else(|| self.account.currency.clone()),
                    )?,
                    quantity: snapshot.quantity,
                    mark_price: mark,
                    multiplier: Decimal::from_integer(1)?,
                });
            }
            let account_currency = Currency::new(self.account.currency.clone())?;
            let mut cash_by_currency = BTreeMap::new();
            cash_by_currency.insert(account_currency.clone(), self.cash);
            let margin_policy = MarginPolicy {
                base_currency: account_currency,
                // Never actually consulted: every position and cash balance
                // here is denominated in the account's own currency, so
                // `FxBook::convert` always takes its same-currency fast path
                // and never reaches a freshness check.
                maximum_fx_age_seconds: i64::MAX,
                rates: rates.clone(),
            };
            let as_of_epoch_seconds = OffsetDateTime::parse(decided_at, &Rfc3339)
                .map_err(|error| PaperError(error.to_string()))?
                .unix_timestamp();
            follon_accounting::value_margin_account(
                &cash_by_currency,
                &margin_positions,
                &FxBook::default(),
                &margin_policy,
                as_of_epoch_seconds,
            )?
            .initial_margin
        } else {
            Decimal::ZERO
        };
        let snapshot = PortfolioRiskSnapshot {
            equity,
            // Real, durable running high-water-mark (see `peak_equity` on
            // `PaperTradingService`) -- never below `equity` itself, since
            // `evaluate_risk` updates it from the same observation before
            // this function ever runs.
            peak_equity: self.peak_equity.max(equity),
            // Real, durable session-start baseline (see `daily_baseline_equity`
            // on `PaperTradingService`) -- `evaluate_risk` updates it from the
            // same observation before this function ever runs.
            daily_pnl: equity.checked_sub(self.daily_baseline_equity)?,
            margin_used,
            positions,
            resting_orders,
            recent_order_count: 0,
        };
        Ok(Some((snapshot, margin_used)))
    }

    /// Builds one aggregate-risk candidate row, classified by the operator's
    /// attested bucket table. Shared by the single-order and combination paths
    /// so a combination leg is classified exactly as the same instrument would
    /// be on its own.
    #[allow(clippy::too_many_arguments)]
    fn risk_candidate(
        &self,
        composition: &PortfolioRiskComposition,
        intent_id: &str,
        account_id: &str,
        strategy_id: &str,
        instrument_id: &str,
        side: Side,
        quantity: Decimal,
        mark_price: Decimal,
    ) -> Result<CandidateOrder, PaperError> {
        let bucket = composition.instrument_buckets.get(instrument_id);
        Ok(CandidateOrder {
            intent_id: intent_id.to_owned(),
            account_id: account_id.to_owned(),
            strategy_id: strategy_id.to_owned(),
            instrument_id: instrument_id.to_owned(),
            asset_class: bucket
                .map(|bucket| bucket.asset_class.clone())
                .unwrap_or_else(|| "unclassified".to_owned()),
            sector: bucket
                .map(|bucket| bucket.sector.clone())
                .unwrap_or_else(|| "unclassified".to_owned()),
            currency: bucket
                .map(|bucket| bucket.currency.clone())
                .unwrap_or_else(|| self.account.currency.clone()),
            side,
            quantity,
            mark_price,
            multiplier: Decimal::from_integer(1)?,
            delta: Decimal::ZERO,
            gamma: Decimal::ZERO,
        })
    }

    /// The composed aggregate-risk decision for a whole combination.
    ///
    /// Every leg is submitted to the kernel as a simultaneous candidate, so
    /// bucket, concentration and exposure limits see the structure as it will
    /// actually exist after an atomic fill. Assessing legs one at a time would
    /// approve a group that breaches a limit only jointly — see
    /// `follon_risk::evaluate_portfolio_risk_with_candidates`.
    fn combo_portfolio_risk_decision(
        &self,
        composition: &PortfolioRiskComposition,
        intent: &ComboIntent,
        market: &PaperComboMarketData,
        decided_at: &str,
    ) -> Result<Option<(follon_risk::PortfolioRiskDecision, Decimal)>, PaperError> {
        let Some((snapshot, margin_used)) = self.portfolio_risk_state(composition, decided_at)?
        else {
            return Ok(None);
        };
        let mut candidates = Vec::with_capacity(intent.legs.len());
        for leg in &intent.legs {
            let mark = market.mark_for(&leg.instrument_id).ok_or_else(|| {
                PaperError(format!(
                    "paper combo observation is missing a mark for {}",
                    leg.instrument_id
                ))
            })?;
            candidates.push(self.risk_candidate(
                composition,
                &intent.intent_id,
                &intent.account_id,
                &intent.strategy_id,
                &leg.instrument_id,
                leg.side,
                intent.leg_quantity(leg)?,
                mark.mark_price,
            )?);
        }
        let decision = follon_risk::evaluate_portfolio_risk_with_candidates(
            &composition.policy,
            &snapshot,
            &candidates,
        )?;
        Ok(Some((decision, margin_used)))
    }

    pub(crate) fn apply_broker_event(&mut self, event: BrokerEvent) -> Result<(), PaperError> {
        if self.combo_orders.contains_key(event.client_order_id()) {
            return self.apply_combo_event(event);
        }
        match event {
            BrokerEvent::ComboExecution(_) => {
                return Err(PaperError(
                    "combo execution does not name a combination".to_owned(),
                ))
            }
            BrokerEvent::Acknowledged {
                client_order_id,
                broker_order_id,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_canonical_id("broker order_id", &broker_order_id)?;
                let order = self.order_mut(&client_order_id)?;
                if let Some(existing) = &order.broker_order_id {
                    if existing != &broker_order_id {
                        if order.broker_order_versions.contains(&broker_order_id) {
                            // An acknowledgement for an earlier broker version is late
                            // evidence, not permission to roll the active version back.
                            return Ok(());
                        }
                        return Err(PaperError(
                            "broker reused client order ID with an unrecognized broker ID"
                                .to_owned(),
                        ));
                    }
                }
                if order.broker_order_id.is_none() {
                    order.broker_order_id = Some(broker_order_id.clone());
                }
                if !order.broker_order_versions.contains(&broker_order_id) {
                    order.broker_order_versions.push(broker_order_id);
                }
                if order.is_terminal() {
                    return Ok(());
                }
                transition_to_acknowledged(order, "IBKR_PAPER_ACKNOWLEDGED_EVENT")?;
            }
            BrokerEvent::Execution {
                execution_id,
                client_order_id,
                broker_order_id,
                quantity,
                price,
                fee,
                executed_at,
            } => {
                validate_canonical_id("broker execution_id", &execution_id)?;
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_canonical_id("broker order_id", &broker_order_id)?;
                validate_utc_timestamp("broker execution time", &executed_at)?;
                if quantity <= Decimal::ZERO || price <= Decimal::ZERO || fee < Decimal::ZERO {
                    return Err(PaperError("invalid broker execution values".to_owned()));
                }
                if self.execution_ids.contains(&execution_id) {
                    return Ok(());
                }
                let (instrument_id, side, order_id, strategy_id) = {
                    let order = self.order_mut(&client_order_id)?;
                    if let Some(existing) = &order.broker_order_id {
                        if existing != &broker_order_id
                            && !order.broker_order_versions.contains(&broker_order_id)
                        {
                            return Err(PaperError(
                                "broker execution has an unrecognized broker order version"
                                    .to_owned(),
                            ));
                        }
                    } else {
                        order.broker_order_id = Some(broker_order_id.clone());
                    }
                    if !order.broker_order_versions.contains(&broker_order_id) {
                        order.broker_order_versions.push(broker_order_id.clone());
                    }
                    if matches!(
                        order.oms.state,
                        OrderState::Cancelled | OrderState::Rejected | OrderState::Expired
                    ) {
                        order.oms.transition(
                            OrderState::Unknown,
                            "LATE_BROKER_EXECUTION_AFTER_TERMINAL",
                        )?;
                    }
                    transition_to_acknowledged(order, "BROKER_EXECUTION_CONFIRMED_ORDER")?;
                    let total = order.filled_quantity.checked_add(quantity)?;
                    if total > order.oms.intent.quantity {
                        return Err(PaperError(
                            "broker execution exceeds internal requested quantity".to_owned(),
                        ));
                    }
                    order.filled_quantity = total;
                    if total == order.oms.intent.quantity {
                        if order.oms.state != OrderState::Filled {
                            order
                                .oms
                                .transition(OrderState::Filled, "BROKER_FULL_FILL")?;
                        }
                    } else if order.oms.state == OrderState::Acknowledged {
                        order
                            .oms
                            .transition(OrderState::PartiallyFilled, "BROKER_PARTIAL_FILL")?;
                    }
                    (
                        order.oms.intent.instrument_id.clone(),
                        order.oms.intent.side,
                        order.oms.order_id.clone(),
                        order.oms.intent.strategy_id.clone(),
                    )
                };
                self.execution_ids.insert(execution_id.clone());
                let fill = Fill {
                    execution_id,
                    order_id,
                    instrument_id: instrument_id.clone(),
                    side,
                    quantity,
                    price,
                    fee,
                    executed_at,
                };
                self.apply_accounted_fill(&fill, &strategy_id)?;
            }
            BrokerEvent::Cancelled {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_broker_reason("broker cancellation reason", &reason)?;
                let order = self.order_mut(&client_order_id)?;
                transition_to_terminal(order, OrderState::Cancelled, &reason)?;
            }
            BrokerEvent::CancelRejected {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_broker_reason("broker cancel rejection reason", &reason)?;
                let order = self.order_mut(&client_order_id)?;
                match order.oms.state {
                    OrderState::PendingCancel | OrderState::Unknown => {
                        restore_working_state(order, "BROKER_CANCEL_REJECTED")?;
                    }
                    OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired => {}
                    _ => {
                        return Err(PaperError(
                            "broker cancel rejection is incompatible with OMS state".to_owned(),
                        ))
                    }
                }
            }
            BrokerEvent::Expired {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_broker_reason("broker expiry reason", &reason)?;
                transition_to_terminal(
                    self.order_mut(&client_order_id)?,
                    OrderState::Expired,
                    &reason,
                )?;
            }
            BrokerEvent::ReplaceRequested {
                client_order_id,
                previous_broker_order_id,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_canonical_id("broker previous_order_id", &previous_broker_order_id)?;
                let order = self.order_mut(&client_order_id)?;
                if order.broker_order_id.as_deref() != Some(previous_broker_order_id.as_str()) {
                    return Err(PaperError(
                        "broker replacement does not match active broker order".to_owned(),
                    ));
                }
                match order.oms.state {
                    OrderState::Acknowledged | OrderState::PartiallyFilled => {
                        order.replace_return_state = Some(order.oms.state);
                        order
                            .oms
                            .transition(OrderState::PendingReplace, "BROKER_REPLACE_REQUESTED")?;
                    }
                    OrderState::PendingReplace => {}
                    OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired => {}
                    _ => {
                        return Err(PaperError(
                            "broker replacement is incompatible with OMS state".to_owned(),
                        ))
                    }
                }
            }
            BrokerEvent::Replaced {
                client_order_id,
                previous_broker_order_id,
                broker_order_id,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_canonical_id("broker previous_order_id", &previous_broker_order_id)?;
                validate_canonical_id("broker replacement_order_id", &broker_order_id)?;
                let order = self.order_mut(&client_order_id)?;
                if order.broker_order_id.as_deref() != Some(previous_broker_order_id.as_str()) {
                    if order.broker_order_versions.contains(&broker_order_id) {
                        return Ok(());
                    }
                    return Err(PaperError(
                        "broker replacement does not match active broker order".to_owned(),
                    ));
                }
                match order.oms.state {
                    OrderState::PendingReplace | OrderState::Unknown => {
                        if !order.broker_order_versions.contains(&broker_order_id) {
                            order.broker_order_versions.push(broker_order_id.clone());
                        }
                        order.broker_order_id = Some(broker_order_id);
                        restore_replacement_state(order, "BROKER_REPLACED")?;
                    }
                    OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired => {}
                    _ => {
                        return Err(PaperError(
                            "broker replacement is incompatible with OMS state".to_owned(),
                        ))
                    }
                }
            }
            BrokerEvent::ReplaceRejected {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_broker_reason("broker replacement rejection reason", &reason)?;
                let order = self.order_mut(&client_order_id)?;
                match order.oms.state {
                    OrderState::PendingReplace | OrderState::Unknown => {
                        restore_replacement_state(order, "BROKER_REPLACE_REJECTED")?;
                    }
                    OrderState::Filled
                    | OrderState::Cancelled
                    | OrderState::Rejected
                    | OrderState::Expired => {}
                    _ => {
                        return Err(PaperError(
                            "broker replacement rejection is incompatible with OMS state"
                                .to_owned(),
                        ))
                    }
                }
            }
            BrokerEvent::Rejected {
                client_order_id,
                reason,
            } => {
                validate_canonical_id("broker client_order_id", &client_order_id)?;
                validate_broker_reason("broker rejection reason", &reason)?;
                let order = self.order_mut(&client_order_id)?;
                transition_to_terminal(order, OrderState::Rejected, &reason)?;
            }
        }
        Ok(())
    }

    /// Applies a fill to the FIFO long/short books, closing opposite inventory
    /// first. A crossing fill splits its fee exactly once; the remainder is
    /// assigned to its opening portion, preserving total fees at fixed precision.
    pub(crate) fn apply_tax_lot_fill(&mut self, fill: &Fill) -> Result<(), PaperError> {
        let currency = Currency::new(self.account.currency.clone())?;
        let available = match fill.side {
            Side::Buy => self
                .tax_lots
                .short_lots(&fill.instrument_id)
                .iter()
                .try_fold(Decimal::ZERO, |total, lot| {
                    total.checked_add(lot.remaining_quantity)
                })?,
            Side::Sell => self
                .tax_lots
                .lots(&fill.instrument_id)
                .iter()
                .try_fold(Decimal::ZERO, |total, lot| {
                    total.checked_add(lot.remaining_quantity)
                })?,
        };
        let closing = available.min(fill.quantity);
        let opening = fill.quantity.checked_sub(closing)?;
        let close_fee = if closing == fill.quantity {
            fill.fee
        } else {
            fill.fee.checked_mul(closing)?.checked_div(fill.quantity)?
        };
        if closing > Decimal::ZERO {
            match fill.side {
                Side::Buy => {
                    self.tax_lots.cover(
                        &format!("taxcover-{}", fill.execution_id),
                        &fill.instrument_id,
                        &currency,
                        closing,
                        fill.price,
                        close_fee,
                        &fill.executed_at,
                        TaxLotSelection::Fifo,
                    )?;
                }
                Side::Sell => {
                    self.tax_lots.dispose(
                        &format!("taxdisposal-{}", fill.execution_id),
                        &fill.instrument_id,
                        &currency,
                        closing,
                        fill.price,
                        close_fee,
                        &fill.executed_at,
                        TaxLotSelection::Fifo,
                    )?;
                }
            }
        }
        if opening == Decimal::ZERO {
            return Ok(());
        }
        let open_fee = fill.fee.checked_sub(close_fee)?;
        match fill.side {
            Side::Buy => {
                let gross = fill.price.checked_mul(opening)?;
                let unit_cost = gross.checked_add(open_fee)?.checked_div(opening)?;
                self.tax_lots.acquire(TaxLot {
                    lot_id: format!("taxlot-{}", fill.execution_id),
                    instrument_id: fill.instrument_id.clone(),
                    currency,
                    opened_at: fill.executed_at.clone(),
                    remaining_quantity: opening,
                    unit_cost,
                })?;
            }
            Side::Sell => {
                self.tax_lots.open_short(ShortTaxLot {
                    lot_id: format!("taxshort-{}", fill.execution_id),
                    instrument_id: fill.instrument_id.clone(),
                    currency,
                    opened_at: fill.executed_at.clone(),
                    remaining_quantity: opening,
                    unit_proceeds: fill
                        .price
                        .checked_mul(opening)?
                        .checked_sub(open_fee)?
                        .checked_div(opening)?,
                })?;
            }
        }
        Ok(())
    }

    /// Updates each strategy's own net signed contribution to one
    /// instrument's aggregate position: a buy adds, a sell subtracts, never
    /// clamped or floored at zero. This is a deliberately separate,
    /// paper-local bookkeeping layer, not a change to `Portfolio` or
    /// `PositionSnapshot` (the canonical position of record, also part of
    /// the audit event schema) -- see
    /// docs/06-delivery/14-master-plan-conformance-audit.md (row 5.7) for why
    /// that remains explicitly out of scope. Letting a value go negative
    /// (a strategy net-sold more than it net-bought, e.g. because another
    /// strategy holds shares of the same instrument) is intentional: summed
    /// with every other strategy's tracked value, it always reconciles
    /// exactly against `Portfolio`'s own aggregate quantity, so
    /// `portfolio_risk_decision`'s per-strategy `RiskPosition` rows plus one
    /// "unattributed" remainder row can never mis-state total gross/net
    /// exposure, only how it is attributed across strategies.
    pub(crate) fn apply_strategy_attribution_fill(
        &mut self,
        fill: &Fill,
        strategy_id: &str,
    ) -> Result<(), PaperError> {
        let instrument_attribution = self
            .strategy_attribution
            .entry(fill.instrument_id.clone())
            .or_default();
        let current = instrument_attribution
            .get(strategy_id)
            .copied()
            .unwrap_or(Decimal::ZERO);
        let updated = match fill.side {
            Side::Buy => current.checked_add(fill.quantity)?,
            Side::Sell => current.checked_sub(fill.quantity)?,
        };
        instrument_attribution.insert(strategy_id.to_owned(), updated);
        Ok(())
    }

    /// Remaining open FIFO tax lots for one instrument, oldest first.
    pub fn tax_lots(&self, instrument_id: &str) -> &[TaxLot] {
        self.tax_lots.lots(instrument_id)
    }

    /// Cumulative FIFO-realized tax P&L in the account's reporting currency,
    /// independent of `Portfolio`'s average-cost realized P&L.
    pub fn realized_tax_pnl(&self) -> Result<Decimal, PaperError> {
        let currency = Currency::new(self.account.currency.clone())?;
        Ok(self.tax_lots.realized(&currency))
    }

    pub(crate) fn persist(&mut self) -> Result<(), PaperError> {
        let state = self.persistent_state();
        if let Some(journal) = &mut self.journal {
            if let Err(error) = journal.append(state) {
                self.persistence_healthy = false;
                return Err(error);
            }
        }
        Ok(())
    }

    fn ensure_persistence_healthy(&self) -> Result<(), PaperError> {
        if self.persistence_healthy {
            Ok(())
        } else {
            Err(PaperError(
                "paper OMS is fail-closed after a durable journal write failure".to_owned(),
            ))
        }
    }

    /// Refuses a new order the configured broker route cannot carry. Callers
    /// run it before risk is evaluated, because evaluation itself updates the
    /// mark cache and equity baselines, so a refusal records, moves and
    /// transmits nothing (delivery state E5.1).
    fn ensure_route_carries(
        &self,
        time_in_force: TimeInForce,
        combination: bool,
    ) -> Result<(), PaperError> {
        let capabilities = self.broker.capabilities(&self.account.account_id)?;
        if combination && !capabilities.combinations {
            return Err(PaperError(
                "the configured paper broker route cannot execute combinations; nothing was recorded or transmitted"
                    .to_owned(),
            ));
        }
        if time_in_force == TimeInForce::GoodTilCancelled && !capabilities.good_til_cancelled {
            return Err(PaperError(
                "the configured paper broker route carries only DAY orders; the GTC intent was refused before anything was recorded or transmitted"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    pub(crate) fn persistent_state(&self) -> PersistentPaperState {
        let orders = self
            .orders
            .iter()
            .map(|(order_id, order)| {
                (
                    order_id.clone(),
                    PersistentOrder {
                        intent: PersistentIntent::from(&order.oms.intent),
                        state: order.oms.state.as_str().to_owned(),
                        broker_order_id: order.broker_order_id.clone(),
                        broker_order_versions: order.broker_order_versions.clone(),
                        replace_return_state: order
                            .replace_return_state
                            .map(OrderState::as_str)
                            .map(str::to_owned),
                        filled_quantity: order.filled_quantity.to_string(),
                        market: PersistentMarketData::from(&order.market),
                    },
                )
            })
            .collect();
        let risk_evidence = self
            .risk_evidence
            .iter()
            .map(|(decision_id, evidence)| {
                (decision_id.clone(), PersistentRiskEvidence::from(evidence))
            })
            .collect();
        let combo_orders = self
            .combo_orders
            .iter()
            .map(|(order_id, order)| {
                (
                    order_id.clone(),
                    PersistentComboOrder {
                        intent: PersistentComboIntent::from(&order.oms.intent),
                        state: order.oms.state.as_str().to_owned(),
                        broker_order_id: order.broker_order_id.clone(),
                        broker_order_versions: order.broker_order_versions.clone(),
                        execution_state: order.execution_state(),
                        market: order
                            .market
                            .marks
                            .iter()
                            .map(PersistentMarketData::from)
                            .collect(),
                    },
                )
            })
            .collect();
        let combo_risk_evidence = self
            .combo_risk_evidence
            .iter()
            .map(|(decision_id, evidence)| {
                (
                    decision_id.clone(),
                    PersistentComboRiskEvidence::from(evidence),
                )
            })
            .collect();
        let positions = self
            .portfolios
            .iter()
            .map(|(instrument_id, portfolio)| {
                let position = portfolio.position_snapshot();
                (
                    instrument_id.clone(),
                    PersistentPosition {
                        quantity: position.quantity.to_string(),
                        average_cost: position.average_cost.to_string(),
                        realized_pnl: position.realized_pnl.to_string(),
                    },
                )
            })
            .collect();
        let incidents = self
            .incidents
            .iter()
            .map(|(incident_id, incident)| {
                (
                    incident_id.clone(),
                    PersistentIncident {
                        category: incident.issue.category.clone(),
                        subject: incident.issue.subject.clone(),
                        detail: incident.issue.detail.clone(),
                        explanation: incident.explanation.clone(),
                    },
                )
            })
            .collect();
        let tax_lot_snapshot = self.tax_lots.snapshot();
        let tax_lots = PersistentTaxLotBook {
            short_lots: tax_lot_snapshot
                .short_lots
                .into_iter()
                .map(|(instrument, lots)| {
                    (
                        instrument,
                        lots.into_iter()
                            .map(|lot| PersistentTaxLot {
                                lot_id: lot.lot_id,
                                opened_at: lot.opened_at,
                                remaining_quantity: lot.remaining_quantity.to_string(),
                                unit_cost: lot.unit_proceeds.to_string(),
                            })
                            .collect(),
                    )
                })
                .collect(),
            applied_short_lot_ids: tax_lot_snapshot.applied_short_lot_ids.into_iter().collect(),
            applied_cover_ids: tax_lot_snapshot.applied_cover_ids.into_iter().collect(),
            lots: tax_lot_snapshot
                .lots
                .into_iter()
                .map(|(instrument_id, lots)| {
                    let persisted = lots
                        .into_iter()
                        .map(|lot| PersistentTaxLot {
                            lot_id: lot.lot_id,
                            opened_at: lot.opened_at,
                            remaining_quantity: lot.remaining_quantity.to_string(),
                            unit_cost: lot.unit_cost.to_string(),
                        })
                        .collect();
                    (instrument_id, persisted)
                })
                .collect(),
            applied_lot_ids: tax_lot_snapshot.applied_lot_ids.into_iter().collect(),
            applied_disposal_ids: tax_lot_snapshot.applied_disposal_ids.into_iter().collect(),
            realized_by_currency: tax_lot_snapshot
                .realized_by_currency
                .into_iter()
                .map(|(currency, amount)| (currency.as_str().to_owned(), amount.to_string()))
                .collect(),
        };
        PersistentPaperState {
            configuration_fingerprint: self.configuration_fingerprint(),
            account_id: self.account.account_id.clone(),
            currency: self.account.currency.clone(),
            cash: self.cash.to_string(),
            orders,
            risk_evidence,
            combo_orders,
            combo_risk_evidence,
            positions,
            execution_ids: self.execution_ids.iter().cloned().collect(),
            active_kill_switches: self.kill_switches.active_keys(),
            incidents,
            last_reconciled_at: self.last_reconciled_at.clone(),
            last_reconciliation_clean: self.last_reconciliation_clean,
            paper_days: self.paper_days.clone(),
            next_reconciliation: self.next_reconciliation,
            broker_connected: self.broker_connected,
            latest_reconciliation: self
                .latest_reconciliation
                .as_ref()
                .map(PersistentReconciliationReport::from),
            tax_lots,
            marks: self
                .marks
                .iter()
                .map(|(instrument_id, mark)| (instrument_id.clone(), mark.to_string()))
                .collect(),
            peak_equity: Some(self.peak_equity.to_string()),
            daily_baseline_date: self.daily_baseline_date.clone(),
            daily_baseline_equity: self
                .daily_baseline_date
                .as_ref()
                .map(|_| self.daily_baseline_equity.to_string()),
            strategy_attribution: self
                .strategy_attribution
                .iter()
                .map(|(instrument_id, strategies)| {
                    (
                        instrument_id.clone(),
                        strategies
                            .iter()
                            .map(|(strategy_id, quantity)| {
                                (strategy_id.clone(), quantity.to_string())
                            })
                            .collect(),
                    )
                })
                .collect(),
            kill_switch_operations: self
                .kill_switch_operations
                .iter()
                .map(|operation| PersistentKillSwitchOperation {
                    scope: operation.scope.clone(),
                    action: operation.action.as_str().to_owned(),
                    operator: operation.operator.clone(),
                    operated_at: operation.operated_at.clone(),
                })
                .collect(),
            order_operations: self
                .order_operations
                .iter()
                .map(|operation| PersistentOrderOperation {
                    order_id: operation.order_id.clone(),
                    action: operation.action.as_str().to_owned(),
                    operator: operation.operator.clone(),
                    operated_at: operation.operated_at.clone(),
                })
                .collect(),
        }
    }

    pub(crate) fn restore(&mut self, state: PersistentPaperState) -> Result<(), PaperError> {
        if state.account_id != self.account.account_id || state.currency != self.account.currency {
            return Err(PaperError(
                "paper journal account or currency does not match supplied configuration"
                    .to_owned(),
            ));
        }
        if state.configuration_fingerprint != self.configuration_fingerprint() {
            return Err(PaperError(
                "paper journal configuration fingerprint does not match supplied configuration"
                    .to_owned(),
            ));
        }
        self.cash = decimal("journal cash", &state.cash)?;
        let mut orders = BTreeMap::new();
        for (order_id, persisted) in state.orders {
            let intent = OrderIntent::try_from(persisted.intent)?;
            if intent.account_id != self.account.account_id || intent.environment != "PAPER" {
                return Err(PaperError(
                    "persisted paper order has an incompatible account or environment".to_owned(),
                ));
            }
            if let Some(broker_order_id) = &persisted.broker_order_id {
                validate_canonical_id("persisted broker_order_id", broker_order_id)?;
            }
            let mut broker_order_versions = persisted.broker_order_versions;
            if let Some(broker_order_id) = &persisted.broker_order_id {
                if broker_order_versions.is_empty() {
                    broker_order_versions.push(broker_order_id.clone());
                }
            }
            let mut unique_versions = BTreeSet::new();
            for broker_order_id in &broker_order_versions {
                validate_canonical_id("persisted broker_order_version", broker_order_id)?;
                if !unique_versions.insert(broker_order_id) {
                    return Err(PaperError(
                        "persisted broker order versions are duplicated".to_owned(),
                    ));
                }
            }
            if let Some(broker_order_id) = &persisted.broker_order_id {
                if !unique_versions.contains(broker_order_id) {
                    return Err(PaperError(
                        "persisted active broker ID is absent from versions".to_owned(),
                    ));
                }
            }
            let filled_quantity = decimal("persisted filled quantity", &persisted.filled_quantity)?;
            if filled_quantity < Decimal::ZERO || filled_quantity > intent.quantity {
                return Err(PaperError(
                    "persisted filled quantity is invalid".to_owned(),
                ));
            }
            let market = PaperMarketData::try_from(persisted.market)?;
            if market.instrument_id != intent.instrument_id {
                return Err(PaperError(
                    "persisted paper market observation does not match intent instrument"
                        .to_owned(),
                ));
            }
            let oms = OmsOrder::recover(
                order_id.clone(),
                intent,
                parse_order_state(&persisted.state)?,
            )?;
            let replace_return_state = persisted
                .replace_return_state
                .as_deref()
                .map(parse_order_state)
                .transpose()?;
            if oms.state == OrderState::PendingReplace && replace_return_state.is_none() {
                return Err(PaperError(
                    "persisted pending replacement has no return state".to_owned(),
                ));
            }
            orders.insert(
                order_id,
                PaperOrder {
                    oms,
                    broker_order_id: persisted.broker_order_id,
                    broker_order_versions,
                    replace_return_state,
                    filled_quantity,
                    market,
                },
            );
        }
        let mut risk_evidence = BTreeMap::new();
        for (decision_id, persisted) in state.risk_evidence {
            validate_canonical_id("persisted risk decision_id", &decision_id)?;
            let evidence = PaperRiskEvidence::try_from(persisted)?;
            if decision_id != evidence.decision.decision_id {
                return Err(PaperError(
                    "persisted risk decision identity does not match its key".to_owned(),
                ));
            }
            if evidence.decision.policy_version != self.risk_policy.version
                || evidence.decision.actor != "paper_risk_engine"
            {
                return Err(PaperError(
                    "persisted risk evidence is incompatible with supplied policy".to_owned(),
                ));
            }
            if risk_evidence.contains_key(&decision_id) {
                return Err(PaperError("duplicate persisted risk evidence".to_owned()));
            }
            risk_evidence.insert(decision_id, evidence);
        }
        for order in orders.values() {
            let decision_id = format!("paper-risk-{}", order.oms.intent.intent_id);
            let evidence = risk_evidence.get(&decision_id).ok_or_else(|| {
                PaperError("persisted paper order is missing risk evidence".to_owned())
            })?;
            if !evidence.decision.approved
                || evidence.intent != order.oms.intent
                || evidence.market != order.market
            {
                return Err(PaperError(
                    "persisted paper order does not match approved risk evidence".to_owned(),
                ));
            }
        }
        let mut combo_orders = BTreeMap::new();
        for (order_id, persisted) in state.combo_orders {
            let intent = ComboIntent::try_from(persisted.intent)?;
            if intent.account_id != self.account.account_id || intent.environment != "PAPER" {
                return Err(PaperError(
                    "persisted paper combination has an incompatible account or environment"
                        .to_owned(),
                ));
            }
            let mut broker_order_versions = persisted.broker_order_versions;
            if let Some(broker_order_id) = &persisted.broker_order_id {
                validate_canonical_id("persisted combo broker_order_id", broker_order_id)?;
                if broker_order_versions.is_empty() {
                    broker_order_versions.push(broker_order_id.clone());
                }
            }
            let mut unique_versions = BTreeSet::new();
            for broker_order_id in &broker_order_versions {
                validate_canonical_id("persisted combo broker_order_version", broker_order_id)?;
                if !unique_versions.insert(broker_order_id) {
                    return Err(PaperError(
                        "persisted combination broker order versions are duplicated".to_owned(),
                    ));
                }
            }
            if let Some(broker_order_id) = &persisted.broker_order_id {
                if !unique_versions.contains(broker_order_id) {
                    return Err(PaperError(
                        "persisted active combination broker ID is absent from versions".to_owned(),
                    ));
                }
            }
            let market = combo_market_from_persisted(persisted.market)?;
            // The observation must still price exactly the legs it was stored
            // against. A journal that lost a leg's mark, or gained one, cannot
            // reproduce the decision that approved the combination.
            market.validate_for(&intent)?;
            let oms = OmsComboOrder::recover(
                order_id.clone(),
                intent,
                parse_order_state(&persisted.state)?,
            )?;
            let mut order = PaperComboOrder {
                oms,
                broker_order_id: persisted.broker_order_id,
                broker_order_versions,
                market,
                filled_quantity: Decimal::ZERO,
                executions: BTreeMap::new(),
                evidence_error: None,
            };
            order.restore_executions(persisted.execution_state)?;
            combo_orders.insert(order_id, order);
        }
        let mut combo_risk_evidence = BTreeMap::new();
        for (decision_id, persisted) in state.combo_risk_evidence {
            validate_canonical_id("persisted combo risk decision_id", &decision_id)?;
            let evidence = PaperComboRiskEvidence::try_from(persisted)?;
            if decision_id != evidence.decision.decision_id {
                return Err(PaperError(
                    "persisted combination risk decision identity does not match its key"
                        .to_owned(),
                ));
            }
            if evidence.decision.policy_version != self.risk_policy.version
                || evidence.decision.actor != "paper_risk_engine"
            {
                return Err(PaperError(
                    "persisted combination risk evidence is incompatible with supplied policy"
                        .to_owned(),
                ));
            }
            if combo_risk_evidence.contains_key(&decision_id) {
                return Err(PaperError(
                    "duplicate persisted combination risk evidence".to_owned(),
                ));
            }
            combo_risk_evidence.insert(decision_id, evidence);
        }
        for order in combo_orders.values() {
            let decision_id = format!("paper-combo-risk-{}", order.oms.intent.intent_id);
            let evidence = combo_risk_evidence.get(&decision_id).ok_or_else(|| {
                PaperError("persisted paper combination is missing risk evidence".to_owned())
            })?;
            if !evidence.decision.approved
                || evidence.intent != order.oms.intent
                || evidence.market != order.market
            {
                return Err(PaperError(
                    "persisted paper combination does not match approved risk evidence".to_owned(),
                ));
            }
        }
        let mut portfolios = BTreeMap::new();
        for (instrument_id, persisted) in state.positions {
            portfolios.insert(
                instrument_id.clone(),
                if self.risk_policy.short_exposure.is_some() {
                    Portfolio::recover_signed
                } else {
                    Portfolio::recover
                }(
                    &self.account.account_id,
                    instrument_id,
                    decimal("persisted position quantity", &persisted.quantity)?,
                    decimal("persisted average cost", &persisted.average_cost)?,
                    decimal("persisted realized pnl", &persisted.realized_pnl)?,
                )?,
            );
        }
        let account_currency = Currency::new(self.account.currency.clone())?;
        let mut lots = BTreeMap::new();
        for (instrument_id, persisted_lots) in state.tax_lots.lots {
            let mut instrument_lots = Vec::with_capacity(persisted_lots.len());
            for persisted in persisted_lots {
                instrument_lots.push(TaxLot {
                    lot_id: persisted.lot_id,
                    instrument_id: instrument_id.clone(),
                    currency: account_currency.clone(),
                    opened_at: persisted.opened_at,
                    remaining_quantity: decimal(
                        "persisted tax lot remaining quantity",
                        &persisted.remaining_quantity,
                    )?,
                    unit_cost: decimal("persisted tax lot unit cost", &persisted.unit_cost)?,
                });
            }
            lots.insert(instrument_id, instrument_lots);
        }
        let mut realized_by_currency = BTreeMap::new();
        for (currency, amount) in state.tax_lots.realized_by_currency {
            realized_by_currency.insert(
                Currency::new(currency)?,
                decimal("persisted realized tax pnl", &amount)?,
            );
        }
        let tax_lots = TaxLotBook::recover(TaxLotBookSnapshot {
            lots,
            applied_lot_ids: state.tax_lots.applied_lot_ids.into_iter().collect(),
            applied_disposal_ids: state.tax_lots.applied_disposal_ids.into_iter().collect(),
            realized_by_currency,
            short_lots: state
                .tax_lots
                .short_lots
                .into_iter()
                .map(|(instrument_id, lots)| {
                    let lots = lots
                        .into_iter()
                        .map(|lot| {
                            Ok(ShortTaxLot {
                                lot_id: lot.lot_id,
                                instrument_id: instrument_id.clone(),
                                currency: account_currency.clone(),
                                opened_at: lot.opened_at,
                                remaining_quantity: decimal(
                                    "short lot quantity",
                                    &lot.remaining_quantity,
                                )?,
                                unit_proceeds: decimal("short lot proceeds", &lot.unit_cost)?,
                            })
                        })
                        .collect::<Result<Vec<_>, PaperError>>()?;
                    Ok((instrument_id, lots))
                })
                .collect::<Result<_, PaperError>>()?,
            applied_short_lot_ids: state.tax_lots.applied_short_lot_ids.into_iter().collect(),
            applied_cover_ids: state.tax_lots.applied_cover_ids.into_iter().collect(),
        })?;
        let mut execution_ids = BTreeSet::new();
        for execution_id in state.execution_ids {
            validate_canonical_id("persisted execution_id", &execution_id)?;
            if !execution_ids.insert(execution_id) {
                return Err(PaperError(
                    "paper journal contains duplicate execution identity".to_owned(),
                ));
            }
        }
        let mut combo_execution_ids = BTreeSet::new();
        for order in combo_orders.values() {
            for execution in order.executions.values() {
                for id in std::iter::once(&execution.execution_id)
                    .chain(execution.legs.iter().map(|leg| &leg.execution_id))
                {
                    if !execution_ids.contains(id) || !combo_execution_ids.insert(id) {
                        return Err(PaperError("persisted combination execution is missing or duplicated in account receipts".to_owned()));
                    }
                }
            }
        }
        let mut marks = BTreeMap::new();
        for (instrument_id, mark) in state.marks {
            validate_canonical_id("persisted mark instrument_id", &instrument_id)?;
            let mark = decimal("persisted mark price", &mark)?;
            if mark <= Decimal::ZERO {
                return Err(PaperError("persisted mark price is invalid".to_owned()));
            }
            marks.insert(instrument_id, mark);
        }
        let mut strategy_attribution = BTreeMap::new();
        for (instrument_id, strategies) in state.strategy_attribution {
            validate_canonical_id("persisted attribution instrument_id", &instrument_id)?;
            let mut parsed_strategies = BTreeMap::new();
            for (strategy_id, quantity) in strategies {
                validate_canonical_id("persisted attribution strategy_id", &strategy_id)?;
                // Deliberately signed, not required positive: a strategy's
                // own tracked contribution can legitimately be negative (see
                // `apply_strategy_attribution_fill`).
                let quantity = decimal("persisted attribution quantity", &quantity)?;
                parsed_strategies.insert(strategy_id, quantity);
            }
            strategy_attribution.insert(instrument_id, parsed_strategies);
        }
        let persisted_peak_equity = state
            .peak_equity
            .map(|value| decimal("persisted peak equity", &value))
            .transpose()?;
        if persisted_peak_equity.is_some_and(|value| value <= Decimal::ZERO) {
            return Err(PaperError("persisted peak equity is invalid".to_owned()));
        }
        let persisted_daily_baseline_equity = state
            .daily_baseline_equity
            .map(|value| decimal("persisted daily-loss baseline equity", &value))
            .transpose()?;
        match (&state.daily_baseline_date, &persisted_daily_baseline_equity) {
            (Some(date), Some(_)) => validate_exchange_date(date)?,
            (None, None) => {}
            _ => {
                return Err(PaperError(
                    "persisted daily-loss baseline date and equity must be present together"
                        .to_owned(),
                ))
            }
        }
        let mut restored_switches = KillSwitchRegistry::new(self.kill_switches.version.clone())?;
        for scope in state.active_kill_switches {
            restored_switches.activate(parse_kill_switch_scope(&scope)?)?;
        }
        let mut kill_switch_operations = Vec::with_capacity(state.kill_switch_operations.len());
        for persisted in state.kill_switch_operations {
            let scope = parse_kill_switch_scope(&persisted.scope)?;
            let action = match persisted.action.as_str() {
                "ACTIVATE" => KillSwitchAction::Activate,
                "RELEASE" => KillSwitchAction::Release,
                _ => {
                    return Err(PaperError(
                        "persisted kill-switch action is invalid".to_owned(),
                    ))
                }
            };
            validate_canonical_id("persisted kill-switch operator", &persisted.operator)?;
            validate_utc_timestamp(
                "persisted kill-switch operation time",
                &persisted.operated_at,
            )?;
            kill_switch_operations.push(KillSwitchOperation {
                scope: scope.as_key(),
                action,
                operator: persisted.operator,
                operated_at: persisted.operated_at,
            });
        }
        let mut incidents = BTreeMap::new();
        for (incident_id, persisted) in state.incidents {
            validate_canonical_id("persisted incident_id", &incident_id)?;
            if persisted.category.is_empty()
                || persisted.subject.is_empty()
                || persisted.detail.is_empty()
            {
                return Err(PaperError(
                    "persisted reconciliation incident is invalid".to_owned(),
                ));
            }
            incidents.insert(
                incident_id.clone(),
                ReconciliationIncident {
                    issue: ReconciliationIssue {
                        incident_id,
                        category: persisted.category,
                        subject: persisted.subject,
                        detail: persisted.detail,
                    },
                    explanation: persisted.explanation,
                },
            );
        }
        for (date, paper_day) in &state.paper_days {
            validate_exchange_date(date)?;
            validate_canonical_id("persisted paper calendar_id", &paper_day.calendar_id)?;
            if paper_day.calendar_id != self.risk_policy.trading_calendar_id {
                return Err(PaperError(
                    "persisted paper-day calendar does not match supplied policy".to_owned(),
                ));
            }
            let session = TradingSession {
                exchange_date: date.clone(),
                opens_at: paper_day.session_opens_at.clone(),
                closes_at: paper_day.session_closes_at.clone(),
            };
            session.validate()?;
        }
        if let Some(last_reconciled_at) = &state.last_reconciled_at {
            validate_utc_timestamp("persisted last reconciliation time", last_reconciled_at)?;
            if state.last_reconciliation_clean.is_none() {
                return Err(PaperError(
                    "persisted last reconciliation has no cleanliness result".to_owned(),
                ));
            }
        } else if state.last_reconciliation_clean.is_some() {
            return Err(PaperError(
                "persisted reconciliation cleanliness has no timestamp".to_owned(),
            ));
        }
        if state.next_reconciliation == 0 {
            return Err(PaperError(
                "persisted reconciliation sequence is invalid".to_owned(),
            ));
        }
        let latest_reconciliation = state
            .latest_reconciliation
            .map(ReconciliationReport::try_from)
            .transpose()?;
        match (
            &latest_reconciliation,
            &state.last_reconciled_at,
            state.last_reconciliation_clean,
        ) {
            (Some(report), Some(reconciled_at), Some(clean))
                if report.reconciled_at == *reconciled_at && report.is_clean() == clean => {}
            (None, None, None) => {}
            // Legacy v2 journals did not retain the exact report. Recovery is allowed,
            // but that legacy checkpoint cannot be used to record a new gate day.
            (None, Some(_), Some(_)) => {}
            _ => {
                return Err(PaperError(
                    "persisted paper reconciliation evidence is inconsistent".to_owned(),
                ))
            }
        }
        if let Some(report) = &latest_reconciliation {
            let expected = format!(
                "reconciliation-{:08}",
                state.next_reconciliation.saturating_sub(1)
            );
            if report.reconciliation_id != expected {
                return Err(PaperError(
                    "persisted latest paper reconciliation identity is invalid".to_owned(),
                ));
            }
        }
        let mut order_operations = Vec::with_capacity(state.order_operations.len());
        for persisted in state.order_operations {
            let action = match persisted.action.as_str() {
                "CANCEL_REQUESTED" => OrderOperationAction::CancelRequested,
                _ => {
                    return Err(PaperError(
                        "persisted order operation action is invalid".to_owned(),
                    ))
                }
            };
            if !orders.contains_key(&persisted.order_id)
                && !combo_orders.contains_key(&persisted.order_id)
            {
                return Err(PaperError(
                    "persisted order operation names an unknown order".to_owned(),
                ));
            }
            validate_canonical_id("persisted order operator", &persisted.operator)?;
            validate_utc_timestamp("persisted order operation time", &persisted.operated_at)?;
            order_operations.push(OrderOperation {
                order_id: persisted.order_id,
                action,
                operator: persisted.operator,
                operated_at: persisted.operated_at,
            });
        }
        self.orders = orders;
        self.risk_evidence = risk_evidence;
        self.combo_orders = combo_orders;
        self.combo_risk_evidence = combo_risk_evidence;
        self.portfolios = portfolios;
        self.tax_lots = tax_lots;
        self.marks = marks;
        self.strategy_attribution = strategy_attribution;
        self.execution_ids = execution_ids;
        self.kill_switches = restored_switches;
        self.kill_switch_operations = kill_switch_operations;
        self.order_operations = order_operations;
        self.incidents = incidents;
        self.last_reconciled_at = state.last_reconciled_at;
        self.last_reconciliation_clean = state.last_reconciliation_clean;
        self.latest_reconciliation = latest_reconciliation;
        self.paper_days = state.paper_days;
        self.next_reconciliation = state.next_reconciliation;
        self.broker_connected = state.broker_connected;
        // `cash`/`portfolios`/`marks` are already restored above, so
        // `current_equity()` reflects real recovered state here. A journal
        // that never tracked peak equity (`persisted_peak_equity` absent)
        // bootstraps its peak to that real equity -- the honest value for a
        // peak that starts being tracked only from this reopen onward.
        let recovered_equity = self.current_equity()?;
        self.peak_equity = persisted_peak_equity
            .unwrap_or(recovered_equity)
            .max(recovered_equity);
        // Unlike peak equity, a daily-loss baseline is never maxed against
        // recovered equity: it is either the exact value durably persisted
        // from earlier the same UTC day, or (absent -- a legacy journal, or
        // one that has never evaluated risk) left unset so the very next
        // `evaluate_risk` call establishes an honest fresh baseline from real
        // recovered state.
        self.daily_baseline_date = state.daily_baseline_date;
        self.daily_baseline_equity = persisted_daily_baseline_equity.unwrap_or(Decimal::ZERO);
        Ok(())
    }

    pub(crate) fn configuration_fingerprint(&self) -> String {
        let initial_cash = self.account.initial_cash.to_string();
        let max_order_quantity = self.risk_policy.max_order_quantity.to_string();
        let max_order_notional = self.risk_policy.max_order_notional.to_string();
        let max_price_deviation_bps = self.risk_policy.max_price_deviation_bps.to_string();
        let max_open_orders = self.risk_policy.max_open_orders.to_string();
        let max_position_quantity = self.risk_policy.max_position_quantity.to_string();
        let max_realized_loss = self.risk_policy.max_realized_loss.to_string();
        let max_market_data_age_seconds = self.risk_policy.max_market_data_age_seconds.to_string();
        let max_order_rate = self.risk_policy.max_order_rate.to_string();
        let order_rate_window_seconds = self.risk_policy.order_rate_window_seconds.to_string();
        // Every portfolio-risk field, through the policy's exhaustive
        // `canonical_parts`, plus this composition's reference data and
        // margin rates. Version 1 of this part listed the fields by hand and
        // left out five limits, so a journal reopened under changed limits
        // (delivery state E7.4). Absent for every configuration without
        // aggregate-risk composition, whose fingerprint is unchanged.
        let portfolio_risk_parts: Vec<String> = self
            .risk_policy
            .portfolio_risk
            .as_ref()
            .map(portfolio_risk_fingerprint_parts)
            .unwrap_or_default();
        let mut parts = vec![
            "paper-configuration-v3",
            &self.account.account_id,
            &self.account.currency,
            &initial_cash,
            &self.account.environment,
            &self.risk_policy.version,
            &self.risk_policy.trading_calendar_id,
            &max_order_quantity,
            &max_order_notional,
            &max_price_deviation_bps,
            &max_open_orders,
            &max_position_quantity,
            &max_realized_loss,
            &max_market_data_age_seconds,
            &max_order_rate,
            &order_rate_window_seconds,
            &self.kill_switches.version,
        ];
        let short_bound = self
            .risk_policy
            .short_exposure
            .as_ref()
            .map(|policy| policy.max_short_quantity.to_string());
        if let Some(bound) = &short_bound {
            parts.push("paper-short-exposure-v1");
            parts.push(bound);
        }
        if !self.broker_route_fingerprint.is_empty() {
            parts.push("paper-broker-route-fingerprint-v1");
            parts.push(&self.broker_route_fingerprint);
        }
        if !portfolio_risk_parts.is_empty() {
            parts.push("paper-portfolio-risk-v2");
            for part in &portfolio_risk_parts {
                parts.push(part);
            }
        }
        // Always present: every configuration now lists its tick sizes, and a
        // journal or decision made under one tick table must not be reopened
        // under another.
        let tick_sizes = render_instrument_table(&self.risk_policy.instrument_tick_sizes);
        parts.push("paper-instrument-ticks-v1");
        parts.push(&tick_sizes);
        // Likewise the lot table (E3.6c).
        let lot_sizes = render_instrument_table(&self.risk_policy.instrument_lot_sizes);
        parts.push("paper-instrument-lots-v1");
        parts.push(&lot_sizes);
        hash_fingerprint_parts(&parts)
    }

    fn unexplained_incident_count(&self) -> u32 {
        self.incidents
            .values()
            .filter(|incident| incident.unexplained())
            .count() as u32
    }
}

/// A portfolio-risk composition's fingerprint parts, every field included:
/// the policy's own, then its instrument buckets and margin rates.
fn portfolio_risk_fingerprint_parts(composition: &PortfolioRiskComposition) -> Vec<String> {
    let PortfolioRiskComposition {
        policy,
        instrument_buckets,
        margin_rates,
    } = composition;
    let mut parts = policy.canonical_parts();
    parts.push(format!(
        "instrument_buckets={}",
        instrument_buckets
            .iter()
            .map(|(instrument_id, bucket)| format!(
                "{instrument_id}:{}:{}:{}",
                bucket.asset_class, bucket.currency, bucket.sector
            ))
            .collect::<Vec<_>>()
            .join("|")
    ));
    parts.push(format!(
        "margin_rates={}",
        margin_rates.as_ref().map_or_else(
            || "none".to_owned(),
            |rates| rates
                .iter()
                .map(|(asset_class, rate)| format!(
                    "{asset_class}:{}:{}",
                    rate.initial_bps, rate.maintenance_bps
                ))
                .collect::<Vec<_>>()
                .join("|")
        )
    ));
    parts
}

/// A per-instrument reference table (ticks or lots) in its canonical,
/// instrument-ordered form, for the configuration fingerprint.
fn render_instrument_table(table: &BTreeMap<String, Decimal>) -> String {
    table
        .iter()
        .map(|(instrument_id, value)| format!("{instrument_id}:{value}"))
        .collect::<Vec<_>>()
        .join("|")
}

/// The first instrument listed in exactly one of two per-instrument tables,
/// checking the tick table's instruments before the lot table's.
pub(crate) fn unpaired_instrument<'a>(
    tick_sizes: &'a BTreeMap<String, Decimal>,
    lot_sizes: &'a BTreeMap<String, Decimal>,
) -> Option<&'a str> {
    tick_sizes
        .keys()
        .find(|instrument_id| !lot_sizes.contains_key(*instrument_id))
        .or_else(|| {
            lot_sizes
                .keys()
                .find(|instrument_id| !tick_sizes.contains_key(*instrument_id))
        })
        .map(String::as_str)
}

/// Each combination leg's entry in a per-instrument reference table, in leg
/// order, with `UNCONFIGURED` for an instrument the table does not list.
pub(crate) fn render_leg_entries(
    intent: &ComboIntent,
    table: &BTreeMap<String, Decimal>,
) -> String {
    intent
        .legs
        .iter()
        .map(|leg| {
            format!(
                "{}:{}",
                leg.instrument_id,
                table
                    .get(&leg.instrument_id)
                    .map_or_else(|| "UNCONFIGURED".to_owned(), ToString::to_string)
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

pub(crate) fn hash_fingerprint_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// Renders a bucket-exposure map deterministically (`BTreeMap` iteration is
/// already sorted) for the `evaluated_limits` evidence string and the
/// configuration fingerprint.
fn render_bucket_map(buckets: &BTreeMap<String, Decimal>) -> String {
    buckets
        .iter()
        .map(|(bucket, amount)| format!("{bucket}:{amount}"))
        .collect::<Vec<_>>()
        .join("|")
}

fn transition_to_acknowledged(order: &mut PaperOrder, reason: &str) -> Result<(), PaperError> {
    match order.oms.state {
        OrderState::PendingSubmit => {
            order.oms.transition(OrderState::Submitted, reason)?;
            order.oms.transition(OrderState::Acknowledged, reason)?;
        }
        OrderState::Submitted | OrderState::Unknown => {
            order.oms.transition(OrderState::Acknowledged, reason)?;
        }
        OrderState::Acknowledged
        | OrderState::PartiallyFilled
        | OrderState::Filled
        | OrderState::PendingCancel
        | OrderState::PendingReplace => {}
        _ => {
            return Err(PaperError(
                "broker acknowledgement is incompatible with internal OMS state".to_owned(),
            ))
        }
    }
    Ok(())
}

fn restore_working_state(order: &mut PaperOrder, reason: &str) -> Result<(), PaperError> {
    let state = if order.filled_quantity == Decimal::ZERO {
        OrderState::Acknowledged
    } else {
        OrderState::PartiallyFilled
    };
    order.oms.transition(state, reason)?;
    Ok(())
}

fn restore_replacement_state(order: &mut PaperOrder, reason: &str) -> Result<(), PaperError> {
    let state = order.replace_return_state.take().unwrap_or_else(|| {
        if order.filled_quantity == Decimal::ZERO {
            OrderState::Acknowledged
        } else {
            OrderState::PartiallyFilled
        }
    });
    order.oms.transition(state, reason)?;
    Ok(())
}

fn transition_to_terminal(
    order: &mut PaperOrder,
    terminal: OrderState,
    reason: &str,
) -> Result<(), PaperError> {
    debug_assert!(matches!(
        terminal,
        OrderState::Cancelled | OrderState::Rejected | OrderState::Expired
    ));
    match order.oms.state {
        OrderState::PendingSubmit => {
            order.oms.transition(OrderState::Submitted, reason)?;
            order.oms.transition(terminal, reason)?;
        }
        OrderState::Submitted
        | OrderState::Acknowledged
        | OrderState::PartiallyFilled
        | OrderState::PendingCancel
        | OrderState::PendingReplace
        | OrderState::Unknown => {
            order.oms.transition(terminal, reason)?;
        }
        state if state == terminal => {}
        // A later, conflicting terminal status is retained as late evidence but
        // never overwrites the original terminal conclusion.
        OrderState::Filled | OrderState::Cancelled | OrderState::Rejected | OrderState::Expired => {
        }
        _ => {
            return Err(PaperError(
                "broker terminal event is incompatible with OMS state".to_owned(),
            ))
        }
    }
    if matches!(
        order.oms.state,
        OrderState::Cancelled | OrderState::Rejected | OrderState::Expired
    ) && order.filled_quantity >= order.oms.intent.quantity
    {
        return Err(PaperError(
            "non-filled terminal state cannot have cumulative quantity equal to requested quantity"
                .to_owned(),
        ));
    }
    Ok(())
}
