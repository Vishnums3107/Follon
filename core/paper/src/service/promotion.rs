//! PAPER session records, the promotion gate and the dashboard read model.

use follon_domain::OrderState;
use follon_instrument::TradingCalendar;

use crate::*;

impl<B: PaperBrokerAdapter> PaperTradingService<B> {
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

    pub(super) fn unexplained_incident_count(&self) -> u32 {
        self.incidents
            .values()
            .filter(|incident| incident.unexplained())
            .count() as u32
    }
}
