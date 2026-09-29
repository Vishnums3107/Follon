//! Broker synchronization and reconciliation against independent internal state.

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal, OrderState};
use std::collections::{BTreeMap, BTreeSet};

use crate::*;

impl<B: PaperBrokerAdapter> PaperTradingService<B> {
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
}
