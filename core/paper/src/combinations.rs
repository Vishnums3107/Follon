//! Adapter-v3 atomic execution evidence and the PAPER combination lifecycle.
use super::*;

/// One complete native combination execution, assembled by the adapter.
/// This is incremental execution evidence, never a cumulative status callback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerComboExecution {
    /// Stable identity for this complete atomic group.
    pub execution_id: String,
    /// Immutable OMS identity.
    pub client_order_id: String,
    /// Native identity shared by every leg.
    pub broker_order_id: String,
    /// Positive whole combination units executed in this group.
    pub units: Decimal,
    /// Exactly one execution for each approved instrument, in any arrival order.
    pub legs: Vec<BrokerComboExecutionLeg>,
}

/// Exact leg economics and attribution within one atomic group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerComboExecutionLeg {
    /// Broker execution identity, unique across the account.
    pub execution_id: String,
    /// Canonical approved leg identity.
    pub instrument_id: String,
    /// Approved side, checked independently of the quantity.
    pub side: Side,
    /// Exact contracts, equal to group units times the approved ratio.
    pub quantity: Decimal,
    /// Positive execution price in the account's normalized value units.
    pub price: Decimal,
    /// Non-negative exact commission in account currency.
    pub fee: Decimal,
    /// Canonical UTC execution time.
    pub executed_at: String,
}

impl BrokerEvent {
    pub(super) fn client_order_id(&self) -> &str {
        match self {
            Self::ComboExecution(fill) => &fill.client_order_id,
            Self::Acknowledged {
                client_order_id, ..
            }
            | Self::Execution {
                client_order_id, ..
            }
            | Self::Cancelled {
                client_order_id, ..
            }
            | Self::CancelRejected {
                client_order_id, ..
            }
            | Self::Expired {
                client_order_id, ..
            }
            | Self::ReplaceRequested {
                client_order_id, ..
            }
            | Self::Replaced {
                client_order_id, ..
            }
            | Self::ReplaceRejected {
                client_order_id, ..
            }
            | Self::Rejected {
                client_order_id, ..
            } => client_order_id,
        }
    }
}

impl BrokerComboExecution {
    fn validate(&self, intent: &ComboIntent) -> Result<(), PaperError> {
        validate_canonical_id("combo execution ID", &self.execution_id)?;
        validate_canonical_id("combo client ID", &self.client_order_id)?;
        validate_canonical_id("combo broker ID", &self.broker_order_id)?;
        if self.units <= Decimal::ZERO
            || self.units.scaled() % follon_domain::DECIMAL_SCALE != 0
            || self.legs.len() != intent.legs.len()
        {
            return Err(PaperError(
                "combination execution requires whole units and every leg".to_owned(),
            ));
        }
        let mut instruments = BTreeSet::new();
        let mut ids = BTreeSet::from([self.execution_id.as_str()]);
        for leg in &self.legs {
            validate_canonical_id("combo leg execution ID", &leg.execution_id)?;
            validate_utc_timestamp("combo leg execution time", &leg.executed_at)?;
            let approved = intent
                .legs
                .iter()
                .find(|approved| approved.instrument_id == leg.instrument_id)
                .ok_or_else(|| {
                    PaperError("combination execution has an unapproved instrument".to_owned())
                })?;
            if !instruments.insert(&leg.instrument_id)
                || !ids.insert(&leg.execution_id)
                || leg.side != approved.side
                || leg.quantity
                    != self
                        .units
                        .checked_mul(Decimal::from_integer(i64::from(approved.ratio))?)?
                || leg.price <= Decimal::ZERO
                || leg.fee < Decimal::ZERO
            {
                return Err(PaperError(
                    "combination execution has duplicate identities or inconsistent leg economics"
                        .to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn fills(&self) -> Vec<Fill> {
        self.legs
            .iter()
            .map(|leg| Fill {
                execution_id: leg.execution_id.clone(),
                order_id: self.client_order_id.clone(),
                instrument_id: leg.instrument_id.clone(),
                side: leg.side,
                quantity: leg.quantity,
                price: leg.price,
                fee: leg.fee,
                executed_at: leg.executed_at.clone(),
            })
            .collect()
    }
}

impl PaperComboOrder {
    fn acknowledge(&mut self, broker_id: &str) -> Result<(), PaperError> {
        validate_canonical_id("combo broker ID", broker_id)?;
        if self
            .broker_order_id
            .as_deref()
            .is_some_and(|id| id != broker_id)
        {
            return Err(PaperError(
                "combination evidence has an unrecognized broker ID".to_owned(),
            ));
        }
        self.broker_order_id = Some(broker_id.to_owned());
        if !self.broker_order_versions.iter().any(|id| id == broker_id) {
            self.broker_order_versions.push(broker_id.to_owned());
        }
        if self.evidence_error.is_some() || self.is_terminal() {
            return Ok(());
        }
        if self.oms.state == OrderState::PendingSubmit {
            self.oms
                .transition(OrderState::Submitted, "COMBO_BROKER_EVIDENCE")?;
        }
        if matches!(self.oms.state, OrderState::Submitted | OrderState::Unknown) {
            self.oms
                .transition(self.working_state(), "COMBO_BROKER_ACKNOWLEDGED")?;
        }
        Ok(())
    }

    fn working_state(&self) -> OrderState {
        if self.filled_quantity == Decimal::ZERO {
            OrderState::Acknowledged
        } else {
            OrderState::PartiallyFilled
        }
    }
}

impl<B: PaperBrokerAdapter> PaperTradingService<B> {
    pub(super) fn cancel_combo_order(
        &mut self,
        id: &str,
        operation: Option<OrderOperation>,
    ) -> Result<(), PaperError> {
        if !self.broker_connected {
            return Err(PaperError(
                "reconnect and reconcile before combination cancellation".to_owned(),
            ));
        }
        let order = self.combo_order_mut(id)?;
        if matches!(
            order.oms.state,
            OrderState::PendingCancel | OrderState::Cancelled
        ) {
            return Ok(());
        }
        if order.evidence_error.is_some()
            || !matches!(
                order.oms.state,
                OrderState::Acknowledged | OrderState::PartiallyFilled
            )
        {
            return Err(PaperError(
                "only acknowledged or partially filled combinations can be cancelled".to_owned(),
            ));
        }
        order
            .oms
            .transition(OrderState::PendingCancel, "PAPER_COMBO_CANCEL_REQUESTED")?;
        if let Some(operation) = operation {
            self.order_operations.push(operation);
        }
        // Intent is durable before the external command, including crash/restart ambiguity.
        self.persist()?;
        let request = BrokerCancelRequest {
            account_id: self.account.account_id.clone(),
            client_order_id: id.to_owned(),
        };
        if let Err(error) = self.broker.cancel(&request) {
            self.combo_order_mut(id)?
                .oms
                .transition(OrderState::Unknown, "PAPER_COMBO_CANCEL_OUTCOME_UNKNOWN")?;
            self.broker_connected = false;
            self.persist()?;
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn apply_combo_event(&mut self, event: BrokerEvent) -> Result<(), PaperError> {
        let id = event.client_order_id().to_owned();
        validate_canonical_id("combo client ID", &id)?;
        match event {
            BrokerEvent::ComboExecution(mut execution) => {
                // Canonicalize leg ordering, so a reordered exact replay remains idempotent.
                execution.legs.sort_by(|a, b| a.instrument_id.cmp(&b.instrument_id));
                // Reached only through `apply_broker_event`'s own dispatch, which
                // has already confirmed this identity. Resolved rather than
                // asserted anyway: this crate's production code contains no
                // panics, and a fill path is the last place to introduce the
                // first one -- a future caller that skips the dispatch check
                // should get a refusal, not an abort mid-fill.
                let order = self.combo_orders.get(&id).ok_or_else(|| {
                    PaperError("paper OMS does not know combination".to_owned())
                })?;
                execution.validate(&order.oms.intent)?;
                if let Some(previous) = order.executions.get(&execution.execution_id) {
                    return if previous == &execution { Ok(()) } else {
                        Err(PaperError("combination execution identity reused with changed evidence".to_owned()))
                    };
                }
                if self.execution_ids.contains(&execution.execution_id)
                    || execution.legs.iter().any(|leg| self.execution_ids.contains(&leg.execution_id)) {
                    return Err(PaperError("combination execution overlaps previously applied evidence".to_owned()));
                }
                let total = order.filled_quantity.checked_add(execution.units)?;
                if total > order.oms.intent.combo_quantity {
                    return Err(PaperError("combination execution exceeds approved units".to_owned()));
                }
                let strategy = order.oms.intent.strategy_id.clone();
                let order = self.combo_order_mut(&id)?;
                if matches!(order.oms.state, OrderState::Cancelled | OrderState::Expired | OrderState::Rejected) {
                    order.oms.transition(OrderState::Unknown, "LATE_COMBO_EXECUTION_AFTER_TERMINAL")?;
                }
                order.acknowledge(&execution.broker_order_id)?;
                // synchronize rolls back every accounting projection and the OMS on any error.
                for fill in execution.fills() { self.apply_accounted_fill(&fill, &strategy)?; }
                self.execution_ids.insert(execution.execution_id.clone());
                let order = self.combo_order_mut(&id)?;
                order.filled_quantity = total;
                if order.evidence_error.is_none() {
                    if total == order.oms.intent.combo_quantity {
                        order.oms.transition(OrderState::Filled, "BROKER_COMBO_FULL_FILL")?;
                    } else if order.oms.state == OrderState::Acknowledged {
                        order.oms.transition(OrderState::PartiallyFilled, "BROKER_COMBO_PARTIAL_FILL")?;
                    }
                }
                order.executions.insert(execution.execution_id.clone(), execution);
            }
            BrokerEvent::Acknowledged { broker_order_id, .. } => self.combo_order_mut(&id)?.acknowledge(&broker_order_id)?,
            BrokerEvent::CancelRejected { reason, .. } => {
                validate_broker_reason("combo cancel rejection", &reason)?;
                let order = self.combo_order_mut(&id)?;
                if order.evidence_error.is_none() && matches!(order.oms.state, OrderState::PendingCancel | OrderState::Unknown) {
                    order.oms.transition(order.working_state(), reason)?;
                }
            }
            BrokerEvent::Cancelled { reason, .. } => self.finish_combo_as(&id, &reason, OrderState::Cancelled)?,
            BrokerEvent::Expired { reason, .. } => self.finish_combo_as(&id, &reason, OrderState::Expired)?,
            BrokerEvent::Rejected { reason, .. } => self.finish_combo_as(&id, &reason, OrderState::Rejected)?,
            _ => return Err(PaperError("combination requires complete atomic execution evidence; replacement is unsupported".to_owned())),
        }
        Ok(())
    }

    pub(super) fn apply_accounted_fill(
        &mut self,
        fill: &Fill,
        strategy: &str,
    ) -> Result<(), PaperError> {
        let portfolio = self
            .portfolios
            .entry(fill.instrument_id.clone())
            .or_insert_with(|| Portfolio::new(&self.account.account_id, &fill.instrument_id));
        if self.risk_policy.short_exposure.is_some() {
            portfolio.apply_signed_fill(fill)?;
        } else {
            portfolio.apply_fill(fill)?;
        }
        self.apply_tax_lot_fill(fill)?;
        self.apply_strategy_attribution_fill(fill, strategy)?;
        let gross = fill.price.checked_mul(fill.quantity)?;
        self.cash = match fill.side {
            Side::Buy => self.cash.checked_sub(gross.checked_add(fill.fee)?)?,
            Side::Sell => self.cash.checked_add(gross.checked_sub(fill.fee)?)?,
        };
        self.execution_ids.insert(fill.execution_id.clone());
        Ok(())
    }

    fn finish_combo_as(
        &mut self,
        id: &str,
        reason: &str,
        state: OrderState,
    ) -> Result<(), PaperError> {
        validate_broker_reason("combo terminal reason", reason)?;
        let order = self.combo_order_mut(id)?;
        if order.evidence_error.is_some() || order.is_terminal() {
            return Ok(());
        }
        if order.oms.state == OrderState::PendingSubmit {
            order.oms.transition(OrderState::Submitted, reason)?;
        }
        order.oms.transition(state, reason)?;
        Ok(())
    }
}

/// A versioned, canonical extension of the existing combination journal record.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PersistentComboExecutionState {
    schema_version: u32,
    filled_quantity: String,
    executions: Vec<PersistentComboExecution>,
    evidence_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PersistentComboExecution {
    execution_id: String,
    client_order_id: String,
    broker_order_id: String,
    units: String,
    legs: Vec<PersistentComboExecutionLeg>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PersistentComboExecutionLeg {
    execution_id: String,
    instrument_id: String,
    side: String,
    quantity: String,
    price: String,
    fee: String,
    executed_at: String,
}

impl PaperComboOrder {
    pub(super) fn execution_state(&self) -> Option<PersistentComboExecutionState> {
        if self.executions.is_empty() && self.evidence_error.is_none() {
            return None;
        }
        Some(PersistentComboExecutionState {
            schema_version: 1,
            filled_quantity: self.filled_quantity.to_string(),
            evidence_error: self.evidence_error.clone(),
            executions: self
                .executions
                .values()
                .map(|e| PersistentComboExecution {
                    execution_id: e.execution_id.clone(),
                    client_order_id: e.client_order_id.clone(),
                    broker_order_id: e.broker_order_id.clone(),
                    units: e.units.to_string(),
                    legs: e
                        .legs
                        .iter()
                        .map(|l| PersistentComboExecutionLeg {
                            execution_id: l.execution_id.clone(),
                            instrument_id: l.instrument_id.clone(),
                            side: l.side.as_str().to_owned(),
                            quantity: l.quantity.to_string(),
                            price: l.price.to_string(),
                            fee: l.fee.to_string(),
                            executed_at: l.executed_at.clone(),
                        })
                        .collect(),
                })
                .collect(),
        })
    }

    pub(super) fn restore_executions(
        &mut self,
        state: Option<PersistentComboExecutionState>,
    ) -> Result<(), PaperError> {
        if let Some(state) = state {
            if state.schema_version != 1 {
                return Err(PaperError(
                    "unsupported combination execution journal version".to_owned(),
                ));
            }
            self.filled_quantity = decimal("combo filled units", &state.filled_quantity)?;
            self.evidence_error = state.evidence_error;
            if let Some(error) = &self.evidence_error {
                validate_broker_reason("combo evidence error", error)?;
            }
            let mut total = Decimal::ZERO;
            let mut ids = BTreeSet::new();
            for e in state.executions {
                let execution = BrokerComboExecution {
                    execution_id: e.execution_id,
                    client_order_id: e.client_order_id,
                    broker_order_id: e.broker_order_id,
                    units: decimal("combo units", &e.units)?,
                    legs: e
                        .legs
                        .into_iter()
                        .map(|l| {
                            Ok(BrokerComboExecutionLeg {
                                execution_id: l.execution_id,
                                instrument_id: l.instrument_id,
                                side: match l.side.as_str() {
                                    "BUY" => Side::Buy,
                                    "SELL" => Side::Sell,
                                    _ => {
                                        return Err(PaperError(
                                            "invalid persisted combo side".to_owned(),
                                        ))
                                    }
                                },
                                quantity: decimal("leg quantity", &l.quantity)?,
                                price: decimal("leg price", &l.price)?,
                                fee: decimal("leg fee", &l.fee)?,
                                executed_at: l.executed_at,
                            })
                        })
                        .collect::<Result<_, PaperError>>()?,
                };
                execution.validate(&self.oms.intent)?;
                if execution.client_order_id != self.oms.order_id
                    || !self
                        .broker_order_versions
                        .contains(&execution.broker_order_id)
                    || !ids.insert(execution.execution_id.clone())
                    || execution
                        .legs
                        .iter()
                        .any(|leg| !ids.insert(leg.execution_id.clone()))
                {
                    return Err(PaperError(
                        "inconsistent persisted combination execution identity".to_owned(),
                    ));
                }
                total = total.checked_add(execution.units)?;
                self.executions
                    .insert(execution.execution_id.clone(), execution);
            }
            if total != self.filled_quantity {
                return Err(PaperError(
                    "persisted combination units do not match receipts".to_owned(),
                ));
            }
        }
        if self.filled_quantity < Decimal::ZERO
            || self.filled_quantity > self.oms.intent.combo_quantity
            || (self.oms.state == OrderState::Filled
                && self.filled_quantity != self.oms.intent.combo_quantity)
            || (self.oms.state == OrderState::PartiallyFilled
                && (self.filled_quantity == Decimal::ZERO
                    || self.filled_quantity == self.oms.intent.combo_quantity))
            || (matches!(
                self.oms.state,
                OrderState::Cancelled | OrderState::Rejected | OrderState::Expired
            ) && self.filled_quantity == self.oms.intent.combo_quantity)
        {
            return Err(PaperError(
                "persisted combination lifecycle disagrees with filled units".to_owned(),
            ));
        }
        Ok(())
    }
}

impl IbkrPaperAdapter {
    /// Queues complete normalized model evidence. No broker connection is implied.
    pub fn queue_combo_fill(&mut self, execution: BrokerComboExecution) -> Result<(), PaperError> {
        let order = self
            .combos
            .get(&execution.client_order_id)
            .ok_or_else(|| PaperError("paper broker does not know combination".to_owned()))?;
        if order.broker_order_id != execution.broker_order_id
            || execution.units <= Decimal::ZERO
            || execution.units.scaled() % follon_domain::DECIMAL_SCALE != 0
            || execution.legs.len() != order.request.legs.len()
        {
            return Err(PaperError("invalid model combination execution".to_owned()));
        }
        let mut seen = BTreeSet::new();
        let mut positions = self.positions.clone();
        let mut cash = self.cash;
        let total = order.filled_quantity.checked_add(execution.units)?;
        for leg in &execution.legs {
            let requested = order
                .request
                .legs
                .iter()
                .find(|r| r.instrument_id == leg.instrument_id)
                .ok_or_else(|| PaperError("unknown model combination leg".to_owned()))?;
            let ratio = Decimal::from_integer(i64::from(requested.ratio))?;
            if !seen.insert(&leg.instrument_id)
                || leg.side != requested.side
                || leg.quantity != execution.units.checked_mul(ratio)?
                || total.checked_mul(ratio)? > requested.quantity
                || leg.price <= Decimal::ZERO
                || leg.fee < Decimal::ZERO
            {
                return Err(PaperError(
                    "model combination execution violates approved ratios or units".to_owned(),
                ));
            }
            let position = positions.entry(leg.instrument_id.clone()).or_default();
            *position = match leg.side {
                Side::Buy => position.checked_add(leg.quantity)?,
                Side::Sell => position.checked_sub(leg.quantity)?,
            };
            let gross = leg.price.checked_mul(leg.quantity)?;
            cash = match leg.side {
                Side::Buy => cash.checked_sub(gross.checked_add(leg.fee)?)?,
                Side::Sell => cash.checked_add(gross.checked_sub(leg.fee)?)?,
            };
        }
        let order = self
            .combos
            .get_mut(&execution.client_order_id)
            .ok_or_else(|| PaperError("paper broker does not know combination".to_owned()))?;
        order.filled_quantity = total;
        let first = &order.request.legs[0];
        order.state = if total.checked_mul(Decimal::from_integer(i64::from(first.ratio))?)?
            == first.quantity
        {
            OrderState::Filled
        } else {
            OrderState::PartiallyFilled
        };
        self.positions = positions;
        self.cash = cash;
        self.pending_events
            .push_back(BrokerEvent::ComboExecution(execution));
        Ok(())
    }
}
