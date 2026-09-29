//! Intent and combination submission through risk, OMS and the broker route.

use follon_control_plane::{OmsComboOrder, OmsOrder};
use follon_domain::{
    validate_canonical_id, validate_utc_timestamp, ComboIntent, Decimal, OrderIntent, OrderState,
    TimeInForce,
};
use std::collections::BTreeMap;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::*;

impl<B: PaperBrokerAdapter> PaperTradingService<B> {
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
}
