//! Configuration-fingerprint parts shared by the risk policy, registry and service.

use follon_domain::{ComboIntent, Decimal};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use crate::*;

/// A portfolio-risk composition's fingerprint parts, every field included:
/// the policy's own, then its instrument buckets and margin rates.
pub(crate) fn portfolio_risk_fingerprint_parts(
    composition: &PortfolioRiskComposition,
) -> Vec<String> {
    let PortfolioRiskComposition {
        policy,
        instrument_buckets,
        margin_rates,
    } = composition;
    let mut parts = policy.canonical_parts();
    parts.push(format!(
        "instrument_buckets={}",
        instrument_buckets
            .iter()
            .map(|(instrument_id, bucket)| format!(
                "{instrument_id}:{}:{}:{}",
                bucket.asset_class, bucket.currency, bucket.sector
            ))
            .collect::<Vec<_>>()
            .join("|")
    ));
    parts.push(format!(
        "margin_rates={}",
        margin_rates.as_ref().map_or_else(
            || "none".to_owned(),
            |rates| rates
                .iter()
                .map(|(asset_class, rate)| format!(
                    "{asset_class}:{}:{}",
                    rate.initial_bps, rate.maintenance_bps
                ))
                .collect::<Vec<_>>()
                .join("|")
        )
    ));
    parts
}

/// A per-instrument reference table (ticks or lots) in its canonical,
/// instrument-ordered form, for the configuration fingerprint.
pub(crate) fn render_instrument_table(table: &BTreeMap<String, Decimal>) -> String {
    table
        .iter()
        .map(|(instrument_id, value)| format!("{instrument_id}:{value}"))
        .collect::<Vec<_>>()
        .join("|")
}

/// The first instrument listed in exactly one of two per-instrument tables,
/// checking the tick table's instruments before the lot table's.
pub(crate) fn unpaired_instrument<'a>(
    tick_sizes: &'a BTreeMap<String, Decimal>,
    lot_sizes: &'a BTreeMap<String, Decimal>,
) -> Option<&'a str> {
    tick_sizes
        .keys()
        .find(|instrument_id| !lot_sizes.contains_key(*instrument_id))
        .or_else(|| {
            lot_sizes
                .keys()
                .find(|instrument_id| !tick_sizes.contains_key(*instrument_id))
        })
        .map(String::as_str)
}

/// Each combination leg's entry in a per-instrument reference table, in leg
/// order, with `UNCONFIGURED` for an instrument the table does not list.
pub(crate) fn render_leg_entries(
    intent: &ComboIntent,
    table: &BTreeMap<String, Decimal>,
) -> String {
    intent
        .legs
        .iter()
        .map(|leg| {
            format!(
                "{}:{}",
                leg.instrument_id,
                table
                    .get(&leg.instrument_id)
                    .map_or_else(|| "UNCONFIGURED".to_owned(), ToString::to_string)
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

pub(crate) fn hash_fingerprint_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}
