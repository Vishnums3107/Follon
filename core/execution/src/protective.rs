//! Bracket children and trailing stops.

use follon_domain::{Decimal, Side};

use crate::*;

/// Creates linked profit-taking and stop-loss children for an approved entry.
pub fn bracket_children(
    parent: &ParentOrder,
    take_profit_price: Decimal,
    stop_price: Decimal,
    stop_limit_price: Option<Decimal>,
) -> Result<Vec<ChildInstruction>, ExecutionError> {
    parent.validate()?;
    if take_profit_price <= Decimal::ZERO
        || stop_price <= Decimal::ZERO
        || stop_limit_price.is_some_and(|price| price <= Decimal::ZERO)
    {
        return Err(ExecutionError("invalid bracket prices".to_owned()));
    }
    let exit_kind = if stop_limit_price.is_some() {
        ChildOrderKind::StopLimit
    } else {
        ChildOrderKind::Stop
    };
    Ok(vec![
        ChildInstruction {
            child_order_id: format!("{}.bracket.profit", parent.parent_order_id),
            scheduled_after_seconds: 0,
            venue: None,
            quantity: parent.quantity,
            kind: ChildOrderKind::Limit,
            limit_price: Some(take_profit_price),
            stop_price: None,
        },
        ChildInstruction {
            child_order_id: format!("{}.bracket.stop", parent.parent_order_id),
            scheduled_after_seconds: 0,
            venue: None,
            quantity: parent.quantity,
            kind: exit_kind,
            limit_price: stop_limit_price,
            stop_price: Some(stop_price),
        },
    ])
}

/// Stateful trailing-stop calculation with a monotonic favorable reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrailingStop {
    side: Side,
    trail_bps: u32,
    favorable_reference: Decimal,
    stop_price: Decimal,
}

impl TrailingStop {
    /// Creates a trailing stop for closing a long (`Sell`) or short (`Buy`) position.
    pub fn new(side: Side, trail_bps: u32, initial_mark: Decimal) -> Result<Self, ExecutionError> {
        if !(1..10_000).contains(&trail_bps) || initial_mark <= Decimal::ZERO {
            return Err(ExecutionError("invalid trailing-stop input".to_owned()));
        }
        let stop_price = trailing_price(side, trail_bps, initial_mark)?;
        Ok(Self {
            side,
            trail_bps,
            favorable_reference: initial_mark,
            stop_price,
        })
    }

    /// Advances only on a favorable mark and returns the current stop.
    pub fn update(&mut self, mark: Decimal) -> Result<Decimal, ExecutionError> {
        if mark <= Decimal::ZERO {
            return Err(ExecutionError(
                "trailing-stop mark must be positive".to_owned(),
            ));
        }
        let favorable = match self.side {
            Side::Sell => mark > self.favorable_reference,
            Side::Buy => mark < self.favorable_reference,
        };
        if favorable {
            self.favorable_reference = mark;
            self.stop_price = trailing_price(self.side, self.trail_bps, mark)?;
        }
        Ok(self.stop_price)
    }

    /// Current trigger price.
    pub const fn stop_price(&self) -> Decimal {
        self.stop_price
    }
}

fn trailing_price(
    side: Side,
    trail_bps: u32,
    reference: Decimal,
) -> Result<Decimal, ExecutionError> {
    let distance = reference
        .checked_mul(Decimal::from_integer(i64::from(trail_bps))?)?
        .checked_div(Decimal::from_integer(10_000)?)?;
    match side {
        Side::Sell => Ok(reference.checked_sub(distance)?),
        Side::Buy => Ok(reference.checked_add(distance)?),
    }
}
