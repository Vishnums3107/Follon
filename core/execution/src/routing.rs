//! Venue quotes, capabilities and smart order routing.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use follon_domain::{validate_canonical_id, Decimal, Side};

use crate::*;

/// Venue quote used for deterministic smart routing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VenueQuote {
    /// Canonical venue identity.
    pub venue: String,
    /// Available displayed or approved quantity.
    pub available_quantity: Decimal,
    /// Executable price.
    pub price: Decimal,
    /// Exact fee per unit.
    pub fee_per_unit: Decimal,
    /// Stable latency rank used only after total-price comparison.
    pub latency_rank: u32,
}

/// Allocates quantity to best all-in venues without exceeding displayed liquidity.
pub fn smart_route(
    parent: &ParentOrder,
    quotes: &[VenueQuote],
) -> Result<ExecutionPlan, ExecutionError> {
    parent.validate()?;
    if quotes.is_empty() || quotes.len() > 1_000 {
        return Err(ExecutionError(
            "smart routing requires venue quotes".to_owned(),
        ));
    }
    let mut ordered = quotes.to_vec();
    for quote in &ordered {
        validate_canonical_id("venue", &quote.venue)?;
        if quote.available_quantity <= Decimal::ZERO
            || quote.price <= Decimal::ZERO
            || quote.fee_per_unit < Decimal::ZERO
        {
            return Err(ExecutionError("invalid smart-routing quote".to_owned()));
        }
    }
    ordered.sort_by(|left, right| compare_quotes(parent.side, left, right));
    let mut remaining = parent.quantity;
    let mut children = Vec::new();
    for quote in ordered {
        if remaining == Decimal::ZERO {
            break;
        }
        let quantity = quote.available_quantity.min(remaining);
        let protected = match (parent.side, parent.limit_price) {
            (Side::Buy, Some(limit)) if quote.price > limit => continue,
            (Side::Sell, Some(limit)) if quote.price < limit => continue,
            _ => Some(quote.price),
        };
        let index = children.len() + 1;
        children.push(ChildInstruction {
            child_order_id: format!("{}.route.{index:04}", parent.parent_order_id),
            scheduled_after_seconds: 0,
            venue: Some(quote.venue),
            quantity,
            kind: ChildOrderKind::Limit,
            limit_price: protected,
            stop_price: None,
        });
        remaining = remaining.checked_sub(quantity)?;
    }
    let plan = ExecutionPlan {
        parent_order_id: parent.parent_order_id.clone(),
        algorithm: "smart-router-v1".to_owned(),
        children,
        unallocated_quantity: remaining,
    };
    plan.validate_against(parent)?;
    Ok(plan)
}

fn compare_quotes(side: Side, left: &VenueQuote, right: &VenueQuote) -> Ordering {
    let left_all_in = match side {
        Side::Buy => left.price.checked_add(left.fee_per_unit),
        Side::Sell => left.price.checked_sub(left.fee_per_unit),
    };
    let right_all_in = match side {
        Side::Buy => right.price.checked_add(right.fee_per_unit),
        Side::Sell => right.price.checked_sub(right.fee_per_unit),
    };
    let price_order = match (left_all_in, right_all_in, side) {
        (Ok(left_price), Ok(right_price), Side::Buy) => left_price.cmp(&right_price),
        (Ok(left_price), Ok(right_price), Side::Sell) => right_price.cmp(&left_price),
        _ => Ordering::Equal,
    };
    // Larger size first as the last tie-break: several quotes from one venue
    // (depth) that otherwise tie must route the same whatever order they
    // arrive in. Quotes still tied after this are identical.
    price_order
        .then_with(|| left.latency_rank.cmp(&right.latency_rank))
        .then_with(|| left.venue.cmp(&right.venue))
        .then_with(|| right.available_quantity.cmp(&left.available_quantity))
}

/// Versioned trading capabilities declared for a specific venue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VenueCapability {
    /// Canonical venue identity.
    pub venue: String,
    /// Stable version label of the capability contract, e.g. "venue.cap.v1".
    pub capability_version: String,
    /// Set of supported child order kinds on this venue.
    pub supported_order_kinds: BTreeSet<ChildOrderKind>,
    /// Whether this venue accepts iceberg orders.
    pub supports_iceberg: bool,
    /// Minimum order quantity accepted by the venue if configured.
    pub min_quantity: Option<Decimal>,
    /// Maximum order quantity accepted by the venue if configured.
    pub max_quantity: Option<Decimal>,
}

impl VenueCapability {
    /// Validates venue capability identity and bounds.
    pub fn validate(&self) -> Result<(), ExecutionError> {
        validate_canonical_id("venue", &self.venue)?;
        validate_canonical_id("capability_version", &self.capability_version)?;
        if self.supported_order_kinds.is_empty() {
            return Err(ExecutionError(
                "venue capability must declare at least one supported order kind".to_owned(),
            ));
        }
        if let Some(min) = self.min_quantity {
            if min <= Decimal::ZERO {
                return Err(ExecutionError("min_quantity must be positive".to_owned()));
            }
        }
        if let Some(max) = self.max_quantity {
            if max <= Decimal::ZERO {
                return Err(ExecutionError("max_quantity must be positive".to_owned()));
            }
        }
        if let (Some(min), Some(max)) = (self.min_quantity, self.max_quantity) {
            if min > max {
                return Err(ExecutionError(
                    "venue min_quantity cannot exceed max_quantity".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// Deterministic routing decision made for an allocated slice of a parent order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteDecision {
    /// Unique decision identity.
    pub decision_id: String,
    /// Parent order identity.
    pub parent_order_id: String,
    /// Selected venue.
    pub venue: String,
    /// Version of venue capability evaluated.
    pub capability_version: String,
    /// Quantity allocated to this venue.
    pub allocated_quantity: Decimal,
    /// Effective executable price including fees.
    pub all_in_price: Decimal,
    /// Fee per unit.
    pub fee_per_unit: Decimal,
    /// Latency rank used in tie-breaking.
    pub latency_rank: u32,
}

/// Gated smart routing that requires explicit venue capability verification.
///
/// Refuses unknown venues, duplicate capability records, and unsupported order kinds
/// before producing any route decisions.
pub fn smart_route_with_capabilities(
    parent: &ParentOrder,
    quotes: &[VenueQuote],
    capabilities: &[VenueCapability],
) -> Result<(ExecutionPlan, Vec<RouteDecision>), ExecutionError> {
    parent.validate()?;
    if quotes.is_empty() || quotes.len() > 1_000 {
        return Err(ExecutionError(
            "smart routing requires between 1 and 1000 venue quotes".to_owned(),
        ));
    }

    // 1. Validate capability records and check for duplicate venues
    let mut cap_map = BTreeMap::new();
    for cap in capabilities {
        cap.validate()?;
        if cap_map.insert(&cap.venue, cap).is_some() {
            return Err(ExecutionError(format!(
                "duplicate capability record for venue '{}'",
                cap.venue
            )));
        }
    }

    // 2. Validate quotes: every quoted venue MUST have an authoritative capability record.
    // Every routed child is a marketable limit at its venue's quote price,
    // exactly as `smart_route` emits, so slippage is capped at the quoted
    // level even for a market parent. A venue must therefore accept limit
    // orders whatever the parent's kind.
    let required_kind = ChildOrderKind::Limit;

    for quote in quotes {
        validate_canonical_id("venue", &quote.venue)?;
        if quote.available_quantity <= Decimal::ZERO
            || quote.price <= Decimal::ZERO
            || quote.fee_per_unit < Decimal::ZERO
        {
            return Err(ExecutionError("invalid smart-routing quote".to_owned()));
        }
        let cap = cap_map.get(&quote.venue).ok_or_else(|| {
            ExecutionError(format!(
                "routing refused: unknown venue capability for '{}'",
                quote.venue
            ))
        })?;
        if !cap.supported_order_kinds.contains(&required_kind) {
            return Err(ExecutionError(format!(
                "routing refused: venue '{}' does not support required order kind {:?}",
                quote.venue, required_kind
            )));
        }
    }

    // 3. Sort quotes deterministically by best all-in price, then latency rank, then venue
    let mut ordered = quotes.to_vec();
    ordered.sort_by(|left, right| compare_quotes(parent.side, left, right));

    let mut remaining = parent.quantity;
    let mut children = Vec::new();
    let mut decisions = Vec::new();

    for quote in ordered {
        if remaining == Decimal::ZERO {
            break;
        }
        let cap = cap_map.get(&quote.venue).expect("validated");
        let mut available = quote.available_quantity;
        if let Some(max) = cap.max_quantity {
            available = available.min(max);
        }
        if let Some(min) = cap.min_quantity {
            if available < min {
                continue;
            }
        }
        let quantity = available.min(remaining);
        if let Some(min) = cap.min_quantity {
            if quantity < min {
                continue;
            }
        }
        let protected = match (parent.side, parent.limit_price) {
            (Side::Buy, Some(limit)) if quote.price > limit => continue,
            (Side::Sell, Some(limit)) if quote.price < limit => continue,
            _ => Some(quote.price),
        };
        let index = children.len() + 1;
        let child_id = format!("{}.route.{index:04}", parent.parent_order_id);
        children.push(ChildInstruction {
            child_order_id: child_id.clone(),
            scheduled_after_seconds: 0,
            venue: Some(quote.venue.clone()),
            quantity,
            kind: required_kind,
            limit_price: protected,
            stop_price: None,
        });
        let all_in_price = match parent.side {
            Side::Buy => quote.price.checked_add(quote.fee_per_unit)?,
            Side::Sell => quote.price.checked_sub(quote.fee_per_unit)?,
        };
        decisions.push(RouteDecision {
            decision_id: format!("{}.decision.{index:04}", parent.parent_order_id),
            parent_order_id: parent.parent_order_id.clone(),
            venue: quote.venue.clone(),
            capability_version: cap.capability_version.clone(),
            allocated_quantity: quantity,
            all_in_price,
            fee_per_unit: quote.fee_per_unit,
            latency_rank: quote.latency_rank,
        });
        remaining = remaining.checked_sub(quantity)?;
    }

    let plan = ExecutionPlan {
        parent_order_id: parent.parent_order_id.clone(),
        algorithm: "smart-router-v1".to_owned(),
        children,
        unallocated_quantity: remaining,
    };
    plan.validate_against(parent)?;
    Ok((plan, decisions))
}
