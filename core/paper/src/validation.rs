//! Shared input validation helpers.

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal};
use std::collections::BTreeSet;
use std::str::FromStr;

use crate::*;

pub(crate) fn validate_exchange_date(value: &str) -> Result<(), PaperError> {
    if value.len() != 10
        || !value.bytes().enumerate().all(|(index, character)| {
            matches!(index, 4 | 7) && character == b'-'
                || !matches!(index, 4 | 7) && character.is_ascii_digit()
        })
    {
        return Err(PaperError("exchange date must be YYYY-MM-DD".to_owned()));
    }
    let timestamp = format!("{value}T00:00:00Z");
    validate_utc_timestamp("exchange date", &timestamp)?;
    Ok(())
}

pub(crate) fn decimal(name: &str, value: &str) -> Result<Decimal, PaperError> {
    Decimal::from_str(value).map_err(|error| PaperError(format!("invalid {name}: {error}")))
}

pub(crate) fn validate_broker_reason(name: &str, value: &str) -> Result<(), PaperError> {
    if value.trim().is_empty() || value.len() > 1_024 {
        return Err(PaperError(format!(
            "{name} must contain 1 to 1024 characters"
        )));
    }
    Ok(())
}

pub(crate) fn validate_broker_snapshot(snapshot: &BrokerAccountSnapshot) -> Result<(), PaperError> {
    let mut order_versions = BTreeSet::new();
    let mut broker_order_ids = BTreeSet::new();
    for order in &snapshot.orders {
        validate_canonical_id("broker snapshot client_order_id", &order.client_order_id)?;
        validate_canonical_id("broker snapshot broker_order_id", &order.broker_order_id)?;
        if order.filled_quantity < Decimal::ZERO
            || !order_versions.insert((
                order.client_order_id.as_str(),
                order.broker_order_id.as_str(),
            ))
            || !broker_order_ids.insert(order.broker_order_id.as_str())
        {
            return Err(PaperError(
                "broker snapshot has duplicate broker order versions or negative filled quantity"
                    .to_owned(),
            ));
        }
    }
    let mut instrument_ids = BTreeSet::new();
    for position in &snapshot.positions {
        validate_canonical_id("broker snapshot instrument_id", &position.instrument_id)?;
        if !instrument_ids.insert(position.instrument_id.as_str()) {
            return Err(PaperError(
                "broker snapshot contains duplicate instrument position".to_owned(),
            ));
        }
    }
    Ok(())
}
