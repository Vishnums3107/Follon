//! Scheduled (TWAP/VWAP/arrival), iceberg and algo-wheel planners.

use follon_domain::Decimal;

use crate::*;

/// Plans a Time-Weighted Average Price (TWAP) execution schedule.
pub fn plan_twap_execution(
    parent: &ParentOrder,
    duration_seconds: u64,
    num_slices: usize,
    kind: ChildOrderKind,
) -> Result<ExecutionPlan, ExecutionError> {
    parent.validate()?;
    if num_slices == 0 || num_slices > 1000 {
        return Err(ExecutionError(
            "TWAP num_slices must be between 1 and 1000".to_owned(),
        ));
    }
    let num_dec = Decimal::from_integer(num_slices as i64)?;
    let raw_slice_qty = parent.quantity.checked_div(num_dec)?;
    if raw_slice_qty <= Decimal::ZERO {
        return Err(ExecutionError(
            "TWAP slice quantity is too small for parent quantity".to_owned(),
        ));
    }

    let interval_seconds = if num_slices > 1 {
        duration_seconds / (num_slices as u64 - 1)
    } else {
        0
    };

    let mut children = Vec::with_capacity(num_slices);
    let mut scheduled_qty = Decimal::ZERO;

    for i in 0..num_slices {
        let child_qty = if i == num_slices - 1 {
            parent.quantity.checked_sub(scheduled_qty)?
        } else {
            raw_slice_qty
        };

        if child_qty > Decimal::ZERO {
            scheduled_qty = scheduled_qty.checked_add(child_qty)?;
            children.push(ChildInstruction {
                child_order_id: format!("{}-twap-{}", parent.parent_order_id, i + 1),
                scheduled_after_seconds: (i as u64) * interval_seconds,
                venue: None,
                quantity: child_qty,
                kind,
                limit_price: parent.limit_price,
                stop_price: None,
            });
        }
    }

    let unallocated_quantity = parent.quantity.checked_sub(scheduled_qty)?;
    let plan = ExecutionPlan {
        parent_order_id: parent.parent_order_id.clone(),
        algorithm: "follon-twap-v1".to_owned(),
        children,
        unallocated_quantity,
    };
    plan.validate_against(parent)?;
    Ok(plan)
}

/// Plans a Volume-Weighted Average Price (VWAP) execution schedule using an empirical volume curve.
pub fn plan_vwap_execution(
    parent: &ParentOrder,
    interval_seconds: u64,
    volume_profile: &[Decimal],
    kind: ChildOrderKind,
) -> Result<ExecutionPlan, ExecutionError> {
    parent.validate()?;
    if volume_profile.is_empty() || volume_profile.len() > 1000 {
        return Err(ExecutionError(
            "VWAP volume_profile must contain between 1 and 1000 slices".to_owned(),
        ));
    }

    let mut profile_sum = Decimal::ZERO;
    for fraction in volume_profile {
        if *fraction < Decimal::ZERO {
            return Err(ExecutionError(
                "VWAP volume profile fractions cannot be negative".to_owned(),
            ));
        }
        profile_sum = profile_sum.checked_add(*fraction)?;
    }
    if profile_sum <= Decimal::ZERO {
        return Err(ExecutionError(
            "VWAP volume profile sum must be positive".to_owned(),
        ));
    }

    let mut children = Vec::with_capacity(volume_profile.len());
    let mut scheduled_qty = Decimal::ZERO;
    let total_slices = volume_profile.len();

    for (index, fraction) in volume_profile.iter().enumerate() {
        let child_qty = if index == total_slices - 1 {
            parent.quantity.checked_sub(scheduled_qty)?
        } else {
            parent
                .quantity
                .checked_mul(*fraction)?
                .checked_div(profile_sum)?
        };

        if child_qty > Decimal::ZERO {
            scheduled_qty = scheduled_qty.checked_add(child_qty)?;
            children.push(ChildInstruction {
                child_order_id: format!("{}-vwap-{}", parent.parent_order_id, index + 1),
                scheduled_after_seconds: (index as u64) * interval_seconds,
                venue: None,
                quantity: child_qty,
                kind,
                limit_price: parent.limit_price,
                stop_price: None,
            });
        }
    }

    let unallocated_quantity = parent.quantity.checked_sub(scheduled_qty)?;
    let plan = ExecutionPlan {
        parent_order_id: parent.parent_order_id.clone(),
        algorithm: "follon-vwap-v1".to_owned(),
        children,
        unallocated_quantity,
    };
    plan.validate_against(parent)?;
    Ok(plan)
}

/// Plans an Arrival Price optimal execution schedule with front-loaded urgency decay.
pub fn plan_arrival_price_execution(
    parent: &ParentOrder,
    total_horizon_seconds: u64,
    urgency_bps: Decimal,
    kind: ChildOrderKind,
) -> Result<ExecutionPlan, ExecutionError> {
    parent.validate()?;
    if urgency_bps <= Decimal::ZERO || urgency_bps > Decimal::from_integer(10_000)? {
        return Err(ExecutionError(
            "arrival price urgency_bps must be between 1 and 10000".to_owned(),
        ));
    }

    let num_slices = 4;
    let interval = total_horizon_seconds / num_slices as u64;

    let weights = [
        Decimal::from_scaled(40_000_000),
        Decimal::from_scaled(30_000_000),
        Decimal::from_scaled(20_000_000),
        Decimal::from_scaled(10_000_000),
    ];

    let mut children = Vec::with_capacity(num_slices);
    let mut scheduled_qty = Decimal::ZERO;

    for (index, weight) in weights.iter().enumerate() {
        let child_qty = if index == num_slices - 1 {
            parent.quantity.checked_sub(scheduled_qty)?
        } else {
            parent.quantity.checked_mul(*weight)?
        };

        if child_qty > Decimal::ZERO {
            scheduled_qty = scheduled_qty.checked_add(child_qty)?;
            children.push(ChildInstruction {
                child_order_id: format!("{}-arrival-{}", parent.parent_order_id, index + 1),
                scheduled_after_seconds: (index as u64) * interval,
                venue: None,
                quantity: child_qty,
                kind,
                limit_price: parent.limit_price,
                stop_price: None,
            });
        }
    }

    let unallocated_quantity = parent.quantity.checked_sub(scheduled_qty)?;
    let plan = ExecutionPlan {
        parent_order_id: parent.parent_order_id.clone(),
        algorithm: "follon-arrival-price-v1".to_owned(),
        children,
        unallocated_quantity,
    };
    plan.validate_against(parent)?;
    Ok(plan)
}

/// Deterministic advanced execution algorithm.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionAlgorithm {
    /// One child for the complete quantity.
    Immediate,
    /// Equal fixed-point slices with remainder units distributed deterministically.
    Twap {
        /// Number of slices.
        slice_count: u32,
        /// Seconds between slices.
        interval_seconds: u64,
    },
    /// Forecast-volume-weighted schedule with exact remainder conservation.
    Vwap {
        /// Positive forecast volume for each consecutive window.
        forecast_market_volumes: Vec<Decimal>,
        /// Seconds between forecast windows.
        interval_seconds: u64,
    },
    /// Caps each slice to a configured share of observed market volume.
    Participation {
        /// Participation in basis points, `(0, 10000]`.
        participation_bps: u32,
        /// Deterministic observed volumes for consecutive windows.
        observed_market_volumes: Vec<Decimal>,
        /// Seconds between observation windows.
        interval_seconds: u64,
    },
    /// Arrival-price schedule. Higher urgency deterministically front-loads
    /// quantity while preserving the complete fixed-point parent quantity.
    ArrivalPrice {
        /// Number of child slices.
        slice_count: u32,
        /// Seconds between slices.
        interval_seconds: u64,
        /// Front-loading urgency in basis points, `[0, 10000]`.
        urgency_bps: u32,
    },
    /// Sequential, display-size limit or market slices with exact fixed-point conservation.
    Iceberg {
        /// Visible display quantity per child slice.
        display_quantity: Decimal,
        /// Minimum elapsed seconds between consecutive child slices.
        interval_seconds: u64,
    },
    /// Weighted allocation of a parent order across bounded, non-wheel sub-algorithms.
    AlgoWheel {
        /// Deterministic allocations with non-wheel sub-algorithms and positive basis points.
        allocations: Vec<AlgoWheelAllocation>,
    },
}

/// One deterministic allocation branch in an algorithm wheel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlgoWheelAllocation {
    /// Sub-algorithm to execute for this share. Must not be another AlgoWheel.
    pub algorithm: ExecutionAlgorithm,
    /// Share in basis points, `(0, 10000]`.
    pub weight_bps: u32,
}

/// Plans immediate, TWAP, VWAP, participation, arrival-price, iceberg, or algo-wheel execution.
pub fn plan_execution(
    parent: &ParentOrder,
    algorithm: &ExecutionAlgorithm,
) -> Result<ExecutionPlan, ExecutionError> {
    parent.validate()?;
    if let ExecutionAlgorithm::AlgoWheel { allocations } = algorithm {
        return plan_algo_wheel_execution(parent, allocations);
    }
    let kind = if parent.limit_price.is_some() {
        ChildOrderKind::Limit
    } else {
        ChildOrderKind::Market
    };
    let (label, quantities, interval_seconds) = match algorithm {
        ExecutionAlgorithm::Immediate => ("immediate-v1", vec![parent.quantity], 0),
        ExecutionAlgorithm::Twap {
            slice_count,
            interval_seconds,
        } => {
            if *slice_count == 0 || *slice_count > 10_000 || *interval_seconds == 0 {
                return Err(ExecutionError("invalid TWAP configuration".to_owned()));
            }
            (
                "twap-v1",
                split_exact(parent.quantity, *slice_count)?,
                *interval_seconds,
            )
        }
        ExecutionAlgorithm::Vwap {
            forecast_market_volumes,
            interval_seconds,
        } => {
            if forecast_market_volumes.is_empty()
                || forecast_market_volumes.len() > 10_000
                || *interval_seconds == 0
                || forecast_market_volumes
                    .iter()
                    .any(|volume| *volume <= Decimal::ZERO)
            {
                return Err(ExecutionError("invalid VWAP configuration".to_owned()));
            }
            (
                "vwap-v1",
                split_weighted_exact(parent.quantity, forecast_market_volumes)?,
                *interval_seconds,
            )
        }
        ExecutionAlgorithm::Participation {
            participation_bps,
            observed_market_volumes,
            interval_seconds,
        } => {
            if !(1..=10_000).contains(participation_bps)
                || observed_market_volumes.is_empty()
                || observed_market_volumes.len() > 10_000
                || *interval_seconds == 0
                || observed_market_volumes
                    .iter()
                    .any(|volume| *volume < Decimal::ZERO)
            {
                return Err(ExecutionError(
                    "invalid participation configuration".to_owned(),
                ));
            }
            let rate = Decimal::from_integer(i64::from(*participation_bps))?
                .checked_div(Decimal::from_integer(10_000)?)?;
            let mut remaining = parent.quantity;
            let mut scheduled = Vec::new();
            for volume in observed_market_volumes {
                if remaining == Decimal::ZERO {
                    break;
                }
                let capacity = volume.checked_mul(rate)?;
                let quantity = capacity.min(remaining);
                if quantity > Decimal::ZERO {
                    scheduled.push(quantity);
                    remaining = remaining.checked_sub(quantity)?;
                }
            }
            ("participation-v1", scheduled, *interval_seconds)
        }
        ExecutionAlgorithm::ArrivalPrice {
            slice_count,
            interval_seconds,
            urgency_bps,
        } => {
            if *slice_count == 0
                || *slice_count > 10_000
                || *interval_seconds == 0
                || *urgency_bps > 10_000
            {
                return Err(ExecutionError(
                    "invalid arrival-price configuration".to_owned(),
                ));
            }
            let mut weights = Vec::with_capacity(*slice_count as usize);
            for index in 0..*slice_count {
                let remaining_rank = i64::from(*slice_count - index);
                let urgency_weight = i64::from(*urgency_bps)
                    .checked_mul(remaining_rank)
                    .ok_or_else(|| ExecutionError("arrival-price weight overflowed".to_owned()))?;
                let weight = 10_000_i64
                    .checked_add(urgency_weight)
                    .ok_or_else(|| ExecutionError("arrival-price weight overflowed".to_owned()))?;
                weights.push(Decimal::from_integer(weight)?);
            }
            (
                "arrival-price-v1",
                split_weighted_exact(parent.quantity, &weights)?,
                *interval_seconds,
            )
        }
        ExecutionAlgorithm::Iceberg {
            display_quantity,
            interval_seconds,
        } => {
            if *display_quantity <= Decimal::ZERO || *interval_seconds == 0 {
                return Err(ExecutionError("invalid iceberg configuration".to_owned()));
            }
            let mut remaining = parent.quantity;
            let mut quantities = Vec::new();
            while remaining > Decimal::ZERO {
                if quantities.len() >= 10_000 {
                    return Err(ExecutionError(
                        "iceberg slice count exceeds maximum allowed".to_owned(),
                    ));
                }
                let slice = remaining.min(*display_quantity);
                quantities.push(slice);
                remaining = remaining.checked_sub(slice)?;
            }
            ("iceberg-v1", quantities, *interval_seconds)
        }
        ExecutionAlgorithm::AlgoWheel { .. } => unreachable!("handled above"),
    };
    let mut allocated = Decimal::ZERO;
    let children = quantities
        .into_iter()
        .enumerate()
        .map(|(index, quantity)| {
            allocated = allocated.checked_add(quantity)?;
            let offset = u64::try_from(index)
                .ok()
                .and_then(|value| value.checked_mul(interval_seconds))
                .ok_or_else(|| ExecutionError("execution schedule overflowed".to_owned()))?;
            Ok(ChildInstruction {
                child_order_id: format!("{}.child.{:04}", parent.parent_order_id, index + 1),
                scheduled_after_seconds: offset,
                venue: None,
                quantity,
                kind,
                limit_price: parent.limit_price,
                stop_price: None,
            })
        })
        .collect::<Result<Vec<_>, ExecutionError>>()?;
    let plan = ExecutionPlan {
        parent_order_id: parent.parent_order_id.clone(),
        algorithm: label.to_owned(),
        children,
        unallocated_quantity: parent.quantity.checked_sub(allocated)?,
    };
    plan.validate_against(parent)?;
    Ok(plan)
}

/// Plans sequential, display-size iceberg execution with exact fixed-point conservation.
pub fn plan_iceberg_execution(
    parent: &ParentOrder,
    display_quantity: Decimal,
    interval_seconds: u64,
    kind: ChildOrderKind,
) -> Result<ExecutionPlan, ExecutionError> {
    parent.validate()?;
    if display_quantity <= Decimal::ZERO || interval_seconds == 0 {
        return Err(ExecutionError("invalid iceberg configuration".to_owned()));
    }
    let mut remaining = parent.quantity;
    let mut children = Vec::new();
    let mut index = 0_usize;
    while remaining > Decimal::ZERO {
        if index >= 10_000 {
            return Err(ExecutionError(
                "iceberg slice count exceeds maximum allowed".to_owned(),
            ));
        }
        let slice = remaining.min(display_quantity);
        let offset = u64::try_from(index)
            .ok()
            .and_then(|val| val.checked_mul(interval_seconds))
            .ok_or_else(|| ExecutionError("iceberg schedule offset overflowed".to_owned()))?;
        children.push(ChildInstruction {
            child_order_id: format!("{}.child.{:04}", parent.parent_order_id, index + 1),
            scheduled_after_seconds: offset,
            venue: None,
            quantity: slice,
            kind,
            limit_price: parent.limit_price,
            stop_price: None,
        });
        remaining = remaining.checked_sub(slice)?;
        index += 1;
    }
    let plan = ExecutionPlan {
        parent_order_id: parent.parent_order_id.clone(),
        algorithm: "iceberg-v1".to_owned(),
        children,
        unallocated_quantity: Decimal::ZERO,
    };
    plan.validate_against(parent)?;
    Ok(plan)
}

/// Plans an algorithm-wheel execution by allocating a parent across bounded sub-algorithms.
pub fn plan_algo_wheel_execution(
    parent: &ParentOrder,
    allocations: &[AlgoWheelAllocation],
) -> Result<ExecutionPlan, ExecutionError> {
    parent.validate()?;
    if allocations.is_empty() || allocations.len() > 100 {
        return Err(ExecutionError(
            "algorithm wheel requires between 1 and 100 allocations".to_owned(),
        ));
    }
    let mut total_weight = 0_u32;
    for alloc in allocations {
        if matches!(alloc.algorithm, ExecutionAlgorithm::AlgoWheel { .. }) {
            return Err(ExecutionError(
                "algorithm wheel cannot contain nested wheel algorithms".to_owned(),
            ));
        }
        if alloc.weight_bps == 0 || alloc.weight_bps > 10_000 {
            return Err(ExecutionError(
                "algorithm wheel allocation weight must be in (0, 10000]".to_owned(),
            ));
        }
        total_weight = total_weight
            .checked_add(alloc.weight_bps)
            .ok_or_else(|| ExecutionError("algorithm wheel weight overflowed".to_owned()))?;
    }
    if total_weight != 10_000 {
        return Err(ExecutionError(
            "algorithm wheel weights must sum to exactly 10000 basis points".to_owned(),
        ));
    }

    let weights = allocations
        .iter()
        .map(|alloc| Decimal::from_integer(i64::from(alloc.weight_bps)))
        .collect::<Result<Vec<_>, _>>()?;
    let sub_quantities = split_weighted_exact(parent.quantity, &weights)?;

    let mut combined_children = Vec::new();
    let mut total_unallocated = Decimal::ZERO;

    for (wheel_index, (alloc, quantity)) in allocations.iter().zip(sub_quantities).enumerate() {
        let sub_parent = ParentOrder {
            parent_order_id: format!("{}.wheel.{:02}", parent.parent_order_id, wheel_index + 1),
            account_id: parent.account_id.clone(),
            instrument_id: parent.instrument_id.clone(),
            side: parent.side,
            quantity,
            limit_price: parent.limit_price,
        };
        let sub_plan = plan_execution(&sub_parent, &alloc.algorithm)?;
        total_unallocated = total_unallocated.checked_add(sub_plan.unallocated_quantity)?;

        for (child_index, child) in sub_plan.children.into_iter().enumerate() {
            combined_children.push((
                child.scheduled_after_seconds,
                wheel_index,
                child_index,
                child,
            ));
        }
    }

    // Deterministic ordering: sort by scheduled offset, breaking ties by wheel allocation index, then child index
    combined_children.sort_by(
        |(offset_a, wheel_a, child_a, _), (offset_b, wheel_b, child_b, _)| {
            offset_a
                .cmp(offset_b)
                .then_with(|| wheel_a.cmp(wheel_b))
                .then_with(|| child_a.cmp(child_b))
        },
    );

    let children = combined_children
        .into_iter()
        .enumerate()
        .map(|(index, (_, _, _, mut child))| {
            child.child_order_id = format!("{}.child.{:04}", parent.parent_order_id, index + 1);
            child
        })
        .collect();

    let plan = ExecutionPlan {
        parent_order_id: parent.parent_order_id.clone(),
        algorithm: "algo-wheel-v1".to_owned(),
        children,
        unallocated_quantity: total_unallocated,
    };
    plan.validate_against(parent)?;
    Ok(plan)
}

fn split_exact(quantity: Decimal, parts: u32) -> Result<Vec<Decimal>, ExecutionError> {
    let divisor = i128::from(parts);
    let base = quantity.scaled() / divisor;
    let remainder = quantity.scaled() % divisor;
    let mut result = Vec::with_capacity(parts as usize);
    for index in 0..parts {
        let extra = i128::from(index) < remainder;
        let scaled = base
            .checked_add(i128::from(extra))
            .ok_or_else(|| ExecutionError("TWAP slice overflowed".to_owned()))?;
        if scaled > 0 {
            result.push(Decimal::from_scaled(scaled));
        }
    }
    Ok(result)
}

fn split_weighted_exact(
    quantity: Decimal,
    weights: &[Decimal],
) -> Result<Vec<Decimal>, ExecutionError> {
    let total_weight = weights.iter().try_fold(0_i128, |total, weight| {
        total
            .checked_add(weight.scaled())
            .ok_or_else(|| ExecutionError("VWAP weight total overflowed".to_owned()))
    })?;
    if quantity <= Decimal::ZERO || total_weight <= 0 {
        return Err(ExecutionError("invalid VWAP inputs".to_owned()));
    }
    let mut allocated = 0_i128;
    let mut result = Vec::with_capacity(weights.len());
    for (index, weight) in weights.iter().enumerate() {
        let scaled = if index + 1 == weights.len() {
            quantity
                .scaled()
                .checked_sub(allocated)
                .ok_or_else(|| ExecutionError("VWAP allocation overflowed".to_owned()))?
        } else {
            quantity
                .scaled()
                .checked_mul(weight.scaled())
                .and_then(|value| value.checked_div(total_weight))
                .ok_or_else(|| ExecutionError("VWAP allocation overflowed".to_owned()))?
        };
        if scaled <= 0 {
            return Err(ExecutionError(
                "VWAP window would round to zero at configured precision".to_owned(),
            ));
        }
        allocated = allocated
            .checked_add(scaled)
            .ok_or_else(|| ExecutionError("VWAP allocation overflowed".to_owned()))?;
        result.push(Decimal::from_scaled(scaled));
    }
    Ok(result)
}
