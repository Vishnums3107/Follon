//! Durable journal persistence, restore and the configuration fingerprint.

use follon_accounting::{Currency, ShortTaxLot, TaxLot, TaxLotBook, TaxLotBookSnapshot};
use follon_control_plane::{OmsComboOrder, OmsOrder, Portfolio};
use follon_domain::{
    validate_canonical_id, validate_utc_timestamp, ComboIntent, Decimal, OrderIntent, OrderState,
};
use follon_instrument::TradingSession;
use std::collections::{BTreeMap, BTreeSet};

use crate::*;

impl<B: PaperBrokerAdapter> PaperTradingService<B> {
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

    pub(super) fn ensure_persistence_healthy(&self) -> Result<(), PaperError> {
        if self.persistence_healthy {
            Ok(())
        } else {
            Err(PaperError(
                "paper OMS is fail-closed after a durable journal write failure".to_owned(),
            ))
        }
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
            corporate_actions: self
                .corporate_actions
                .iter()
                .map(PersistentCorporateAction::from)
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
        let corporate_actions = restore_corporate_actions(state.corporate_actions)?;
        self.orders = orders;
        self.risk_evidence = risk_evidence;
        self.corporate_actions = corporate_actions;
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
}
