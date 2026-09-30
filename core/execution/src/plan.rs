//! Parent orders, child instructions and the execution plan container.

use follon_domain::{validate_canonical_id, Decimal, Side};

use crate::*;

/// Broker-neutral parent request accepted only after a risk approval.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParentOrder {
    /// Stable idempotency identity.
    pub parent_order_id: String,
    /// Account selected by the approved intent.
    pub account_id: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Economic side.
    pub side: Side,
    /// Total positive quantity.
    pub quantity: Decimal,
    /// Optional parent limit inherited by child orders.
    pub limit_price: Option<Decimal>,
}

impl ParentOrder {
    /// Validates broker-neutral identity and economics.
    pub fn validate(&self) -> Result<(), ExecutionError> {
        validate_canonical_id("parent_order_id", &self.parent_order_id)?;
        validate_canonical_id("account_id", &self.account_id)?;
        validate_canonical_id("instrument_id", &self.instrument_id)?;
        if self.quantity <= Decimal::ZERO
            || self.limit_price.is_some_and(|price| price <= Decimal::ZERO)
        {
            return Err(ExecutionError(
                "parent order requires positive quantity and price".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Child instruction type understood by an adapter mapping layer.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ChildOrderKind {
    /// Immediately executable market order.
    Market,
    /// Price-protected limit order.
    Limit,
    /// Triggered stop order.
    Stop,
    /// Triggered order with a post-trigger limit.
    StopLimit,
}

/// One immutable child instruction in an execution plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildInstruction {
    /// Parent-derived idempotency identity.
    pub child_order_id: String,
    /// Zero-based scheduling offset.
    pub scheduled_after_seconds: u64,
    /// Venue identity, if smart routing selected one.
    pub venue: Option<String>,
    /// Positive child quantity.
    pub quantity: Decimal,
    /// Adapter-neutral order kind.
    pub kind: ChildOrderKind,
    /// Optional price carried from parent or routing protection.
    pub limit_price: Option<Decimal>,
    /// Optional trigger price for stop/bracket behavior.
    pub stop_price: Option<Decimal>,
}

/// A complete deterministic result; child plus unallocated quantity equals the parent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionPlan {
    /// Parent identity.
    pub parent_order_id: String,
    /// Stable algorithm/version label.
    pub algorithm: String,
    /// Ordered child instructions.
    pub children: Vec<ChildInstruction>,
    /// Quantity not scheduled because observed liquidity was insufficient.
    pub unallocated_quantity: Decimal,
}

impl ExecutionPlan {
    /// Proves quantity conservation and valid child identities.
    pub fn validate_against(&self, parent: &ParentOrder) -> Result<(), ExecutionError> {
        if self.parent_order_id != parent.parent_order_id || self.algorithm.is_empty() {
            return Err(ExecutionError(
                "execution plan identity mismatch".to_owned(),
            ));
        }
        let mut total = self.unallocated_quantity;
        if total < Decimal::ZERO {
            return Err(ExecutionError(
                "unallocated execution quantity cannot be negative".to_owned(),
            ));
        }
        let mut prior_offset = 0;
        for (index, child) in self.children.iter().enumerate() {
            validate_canonical_id("child_order_id", &child.child_order_id)?;
            if child.quantity <= Decimal::ZERO
                || (index > 0 && child.scheduled_after_seconds < prior_offset)
                || child
                    .limit_price
                    .is_some_and(|price| price <= Decimal::ZERO)
                || child.stop_price.is_some_and(|price| price <= Decimal::ZERO)
            {
                return Err(ExecutionError("invalid child instruction".to_owned()));
            }
            prior_offset = child.scheduled_after_seconds;
            total = total.checked_add(child.quantity)?;
        }
        if total != parent.quantity {
            return Err(ExecutionError(
                "execution plan does not conserve parent quantity".to_owned(),
            ));
        }
        Ok(())
    }
}
