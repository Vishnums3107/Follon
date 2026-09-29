//! PAPER orders, combination orders and their risk evidence.

use follon_control_plane::{OmsComboOrder, OmsOrder};
use follon_domain::{
    validate_canonical_id, ComboIntent, Decimal, OrderIntent, OrderState, RiskDecision, Side,
};
use follon_instrument::TradingSession;
use std::collections::BTreeMap;

use crate::*;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PaperRiskContext {
    pub(crate) open_orders: usize,
    pub(crate) position_quantity: Decimal,
    pub(crate) available_cash: Decimal,
    pub(crate) realized_pnl: Decimal,
}

/// Internal paper OMS record with exact independently-accounted fill quantity.
#[derive(Clone, Debug)]
pub struct PaperOrder {
    /// Durable OMS order and legal lifecycle state.
    pub oms: OmsOrder,
    /// Broker order identity once known.
    pub broker_order_id: Option<String>,
    /// Every broker-native order identity issued for this immutable OMS order.
    pub broker_order_versions: Vec<String>,
    /// State to restore after a replacement acceptance or rejection.
    pub(crate) replace_return_state: Option<OrderState>,
    /// Exact fill quantity observed through normalized broker execution IDs.
    pub filled_quantity: Decimal,
    /// Exact fresh market observation used for approval and outstanding-cash reservation.
    pub market: PaperMarketData,
}

impl PaperOrder {
    pub(crate) fn is_terminal(&self) -> bool {
        matches!(
            self.oms.state,
            OrderState::RiskRejected
                | OrderState::Filled
                | OrderState::Cancelled
                | OrderState::Rejected
                | OrderState::Expired
        )
    }

    pub(crate) fn working(&self) -> bool {
        !self.is_terminal()
    }

    pub(crate) fn reserved_cash(&self) -> Result<Decimal, PaperError> {
        if !self.working() || self.oms.intent.side != Side::Buy {
            return Ok(Decimal::ZERO);
        }
        let remaining = self.oms.intent.quantity.checked_sub(self.filled_quantity)?;
        remaining
            .checked_mul(self.market.mark_price)
            .map_err(Into::into)
    }
}

/// Internal paper OMS record for one atomic multi-leg combination.
#[derive(Clone, Debug)]
pub struct PaperComboOrder {
    /// Durable OMS combination order and legal lifecycle state.
    pub oms: OmsComboOrder,
    /// Broker order identity once known. One identity for the whole group: an
    /// atomic combination is a single broker order, not one per leg.
    pub broker_order_id: Option<String>,
    /// Every broker-native order identity issued for this immutable OMS order.
    pub broker_order_versions: Vec<String>,
    /// Exact per-leg observation used for approval and cash reservation.
    pub market: PaperComboMarketData,
    /// Independently accounted whole combination units, never a leg quantity.
    pub filled_quantity: Decimal,
    /// Accepted complete execution receipts, retained for exact retry validation.
    pub(crate) executions: BTreeMap<String, BrokerComboExecution>,
    /// Malformed/drained evidence cannot be cleared by a later status message.
    pub(crate) evidence_error: Option<String>,
}

impl PaperComboOrder {
    pub(crate) fn is_terminal(&self) -> bool {
        matches!(
            self.oms.state,
            OrderState::RiskRejected
                | OrderState::Filled
                | OrderState::Cancelled
                | OrderState::Rejected
                | OrderState::Expired
        )
    }

    pub(crate) fn working(&self) -> bool {
        !self.is_terminal()
    }

    /// Cash a working combination still has committed.
    ///
    /// Only the unfilled net debit reserves cash, matching the approval gate.
    pub(crate) fn reserved_cash(&self) -> Result<Decimal, PaperError> {
        if !self.working() {
            return Ok(Decimal::ZERO);
        }
        let net_price = self.oms.intent.protected_net_price()?;
        if net_price <= Decimal::ZERO {
            return Ok(Decimal::ZERO);
        }
        net_price
            .checked_mul(
                self.oms
                    .intent
                    .combo_quantity
                    .checked_sub(self.filled_quantity)?,
            )
            .map_err(Into::into)
    }
}

/// Immutable record of a combination decision and the observation behind it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperComboRiskEvidence {
    /// The complete immutable combination intent evaluated by risk, including
    /// rejected ones.
    pub intent: ComboIntent,
    /// The versioned risk result.
    pub decision: RiskDecision,
    /// The exact validated per-leg observation evaluated by risk.
    pub market: PaperComboMarketData,
    /// The authenticated operator who submitted the combination, when it
    /// arrived through an authenticated route; `None` for a direct caller.
    pub submitted_by: Option<String>,
}

/// Immutable record of the decision and exact market observation that produced it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperRiskEvidence {
    /// The complete immutable intent evaluated by risk, including rejected intents.
    pub intent: OrderIntent,
    /// The versioned risk result.
    pub decision: RiskDecision,
    /// The exact validated price observation evaluated by risk.
    pub market: PaperMarketData,
    /// The authenticated operator who submitted the order, when it arrived
    /// through an authenticated route; `None` for a direct caller (E5.2b).
    pub submitted_by: Option<String>,
}

/// An authoritative exchange session that may count toward the paper gate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaperTradingSession {
    /// Immutable calendar identity that produced the session.
    pub calendar_id: String,
    /// Explicit regular exchange session including the exchange-local date.
    pub session: TradingSession,
}

impl PaperTradingSession {
    pub(crate) fn validate(&self) -> Result<(), PaperError> {
        validate_canonical_id("paper calendar_id", &self.calendar_id)?;
        self.session.validate()?;
        validate_exchange_date(&self.session.exchange_date)?;
        Ok(())
    }
}

/// Result of a paper OMS submission attempt, including mandatory risk evidence.
#[derive(Clone, Debug)]
pub struct PaperSubmitOutcome {
    /// Risk decision emitted whether or not an executable order exists.
    pub decision: RiskDecision,
    /// OMS idempotency identity when the decision was approved.
    pub order_id: Option<String>,
    /// Resulting OMS lifecycle state when an order exists.
    pub state: Option<OrderState>,
}
