//! Deterministic historical market-data normalization, bar construction, and actions.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use follon_domain::{validate_canonical_id, Bar, Decimal, DomainError};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// A normalized trade used solely to construct reproducible historical bars.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trade {
    /// Source event time in UTC.
    pub event_time: String,
    /// Canonical resolved instrument identifier.
    pub instrument_id: String,
    /// Immutable source identity used to detect duplicated vendor records.
    pub trade_id: String,
    /// Monotonic source sequence used to break same-timestamp ordering ties.
    pub source_sequence: u64,
    /// Exact trade price.
    pub price: Decimal,
    /// Exact trade quantity.
    pub quantity: Decimal,
}

impl Trade {
    /// Rejects unnormalized and non-UTC source records.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_canonical_id("instrument_id", &self.instrument_id)?;
        validate_canonical_id("trade_id", &self.trade_id)?;
        if !self.event_time.ends_with('Z')
            || OffsetDateTime::parse(&self.event_time, &Rfc3339).is_err()
            || self.price <= Decimal::ZERO
            || self.quantity <= Decimal::ZERO
        {
            return Err(DomainError(
                "trade must have UTC time, positive price, and positive quantity".to_owned(),
            ));
        }
        Ok(())
    }
}

/// A deterministic bar builder with no access to wall-clock time.
pub struct BarBuilder {
    interval_seconds: i64,
    exchange_timezone: String,
}

impl BarBuilder {
    /// Creates a fixed-interval builder retaining exchange-local display context.
    pub fn new(
        interval_seconds: u32,
        exchange_timezone: impl Into<String>,
    ) -> Result<Self, DomainError> {
        let exchange_timezone = exchange_timezone.into();
        if interval_seconds == 0 || exchange_timezone.is_empty() {
            return Err(DomainError(
                "bar interval and exchange timezone are required".to_owned(),
            ));
        }
        Ok(Self {
            interval_seconds: i64::from(interval_seconds),
            exchange_timezone,
        })
    }

    /// Sorts trades by source time and builds stable OHLCV bars.
    pub fn build(
        &self,
        trades: impl IntoIterator<Item = Trade>,
    ) -> Result<Vec<(String, Bar)>, MarketDataError> {
        let mut grouped: BTreeMap<(String, i64), Vec<Trade>> = BTreeMap::new();
        let mut trade_ids = BTreeSet::new();
        let mut source_sequences = BTreeSet::new();
        for trade in trades {
            trade.validate()?;
            if !trade_ids.insert(trade.trade_id.clone()) {
                return Err(MarketDataError(format!(
                    "duplicate trade ID: {}",
                    trade.trade_id
                )));
            }
            if !source_sequences.insert((trade.instrument_id.clone(), trade.source_sequence)) {
                return Err(MarketDataError(format!(
                    "duplicate source sequence for {}: {}",
                    trade.instrument_id, trade.source_sequence
                )));
            }
            let timestamp = parse_utc(&trade.event_time)?;
            let bucket = timestamp.unix_timestamp().div_euclid(self.interval_seconds)
                * self.interval_seconds;
            grouped
                .entry((trade.instrument_id.clone(), bucket))
                .or_default()
                .push(trade);
        }
        let mut bars = Vec::with_capacity(grouped.len());
        for ((instrument_id, bucket), mut bucket_trades) in grouped {
            bucket_trades.sort_by(|left, right| {
                left.event_time
                    .cmp(&right.event_time)
                    .then_with(|| left.source_sequence.cmp(&right.source_sequence))
                    .then_with(|| left.trade_id.cmp(&right.trade_id))
            });
            let first = bucket_trades
                .first()
                .expect("grouped trade bucket is not empty");
            let last = bucket_trades
                .last()
                .expect("grouped trade bucket is not empty");
            let mut high = first.price;
            let mut low = first.price;
            let mut volume = Decimal::ZERO;
            for trade in &bucket_trades {
                high = high.max(trade.price);
                low = low.min(trade.price);
                volume = volume.checked_add(trade.quantity)?;
            }
            let event_time = OffsetDateTime::from_unix_timestamp(bucket)
                .map_err(|error| MarketDataError(error.to_string()))?
                .format(&Rfc3339)
                .map_err(|error| MarketDataError(error.to_string()))?;
            let bar = Bar {
                instrument_id,
                open: first.price,
                high,
                low,
                close: last.price,
                volume,
                interval_seconds: self.interval_seconds as u32,
                exchange_timezone: self.exchange_timezone.clone(),
            };
            bar.validate()?;
            bars.push((event_time, bar));
        }
        Ok(bars)
    }
}

/// Imports the normalized v1 trade CSV contract used by deterministic bar construction.
///
/// Provider adapters must preserve their immutable trade identity and monotonic
/// source sequence. The core refuses an ambiguous order instead of inventing
/// an OHLC open or close from file order.
pub fn import_trades(csv: &str) -> Result<Vec<Trade>, MarketDataError> {
    const HEADER: &str = "event_time,instrument_id,trade_id,source_sequence,price,quantity";
    let mut lines = csv.lines().filter(|line| !line.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| MarketDataError("trade CSV is empty".to_owned()))?;
    if header.trim_start_matches('\u{feff}').trim_end_matches('\r') != HEADER {
        return Err(MarketDataError(
            "trade CSV header does not match v1 contract".to_owned(),
        ));
    }

    let mut trades = Vec::new();
    for (index, line) in lines.enumerate() {
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        if fields.len() != 6 {
            return Err(MarketDataError(format!("invalid trade row {}", index + 2)));
        }
        let trade = Trade {
            event_time: fields[0].to_owned(),
            instrument_id: fields[1].to_owned(),
            trade_id: fields[2].to_owned(),
            source_sequence: fields[3].parse().map_err(|_| {
                MarketDataError(format!("invalid source sequence on row {}", index + 2))
            })?,
            price: Decimal::from_str(fields[4]).map_err(|error| {
                MarketDataError(format!("invalid trade price on row {}: {error}", index + 2))
            })?,
            quantity: Decimal::from_str(fields[5]).map_err(|error| {
                MarketDataError(format!(
                    "invalid trade quantity on row {}: {error}",
                    index + 2
                ))
            })?,
        };
        trade.validate()?;
        trades.push(trade);
    }
    if trades.is_empty() {
        return Err(MarketDataError(
            "trade CSV contains no data rows".to_owned(),
        ));
    }
    Ok(trades)
}

/// Versioned corporate action that changes historical price/quantity interpretation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CorporateAction {
    /// A split ratio such as 2:1, effective from the supplied UTC time.
    Split {
        /// Immutable canonical corporate-action identity.
        action_id: String,
        /// Canonical instrument affected by the action.
        instrument_id: String,
        /// UTC instant at which the action takes effect.
        effective_at: String,
        /// Exact new-units-per-old-unit split ratio.
        ratio: Decimal,
    },
    /// A cash dividend per unit, used for total-return adjusted bars.
    CashDividend {
        /// Immutable canonical corporate-action identity.
        action_id: String,
        /// Canonical instrument affected by the action.
        instrument_id: String,
        /// UTC instant at which the action takes effect.
        effective_at: String,
        /// Exact cash dividend per unit in the bar currency.
        amount: Decimal,
    },
}

impl CorporateAction {
    /// Immutable action identity used to de-duplicate and order an action feed.
    pub fn action_id(&self) -> &str {
        match self {
            Self::Split { action_id, .. } | Self::CashDividend { action_id, .. } => action_id,
        }
    }

    /// Canonical instrument affected by this action.
    pub fn instrument_id(&self) -> &str {
        match self {
            Self::Split { instrument_id, .. } | Self::CashDividend { instrument_id, .. } => {
                instrument_id
            }
        }
    }

    /// UTC instant at which this action takes effect.
    pub fn effective_at(&self) -> &str {
        match self {
            Self::Split { effective_at, .. } | Self::CashDividend { effective_at, .. } => {
                effective_at
            }
        }
    }

    /// Stable input row used in dataset content addressing.
    pub fn canonical_record(&self) -> String {
        match self {
            Self::Split {
                action_id,
                instrument_id,
                effective_at,
                ratio,
            } => format!("SPLIT|{action_id}|{instrument_id}|{effective_at}|{ratio}"),
            Self::CashDividend {
                action_id,
                instrument_id,
                effective_at,
                amount,
            } => format!("CASH_DIVIDEND|{action_id}|{instrument_id}|{effective_at}|{amount}"),
        }
    }

    /// Validates canonical action identity and exact values.
    pub fn validate(&self) -> Result<(), DomainError> {
        let (action_id, instrument_id, effective_at, value) = match self {
            Self::Split {
                action_id,
                instrument_id,
                effective_at,
                ratio,
            } => (action_id, instrument_id, effective_at, ratio),
            Self::CashDividend {
                action_id,
                instrument_id,
                effective_at,
                amount,
            } => (action_id, instrument_id, effective_at, amount),
        };
        validate_canonical_id("action_id", action_id)?;
        validate_canonical_id("instrument_id", instrument_id)?;
        if !effective_at.ends_with('Z')
            || OffsetDateTime::parse(effective_at, &Rfc3339).is_err()
            || *value <= Decimal::ZERO
        {
            return Err(DomainError(
                "corporate action must have UTC time and positive value".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Imports the deliberately small v1 corporate-action CSV contract.
///
/// The importer preserves source action identity and rejects duplicate action
/// rows rather than silently choosing one provider revision over another.
pub fn import_corporate_actions(csv: &str) -> Result<Vec<CorporateAction>, MarketDataError> {
    const HEADER: &str = "action_id,instrument_id,action_type,effective_at,value";
    let mut lines = csv.lines().filter(|line| !line.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| MarketDataError("corporate-action CSV is empty".to_owned()))?;
    if header.trim_start_matches('\u{feff}').trim_end_matches('\r') != HEADER {
        return Err(MarketDataError(
            "corporate-action CSV header does not match v1 contract".to_owned(),
        ));
    }

    let mut action_ids = std::collections::BTreeSet::new();
    let mut actions = Vec::new();
    for (index, line) in lines.enumerate() {
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        if fields.len() != 5 {
            return Err(MarketDataError(format!(
                "invalid corporate-action row {}",
                index + 2
            )));
        }
        if !action_ids.insert(fields[0].to_owned()) {
            return Err(MarketDataError(format!(
                "duplicate corporate action ID on row {}: {}",
                index + 2,
                fields[0]
            )));
        }
        let value = fields[4].parse().map_err(|error| {
            MarketDataError(format!(
                "invalid corporate-action value on row {}: {error}",
                index + 2
            ))
        })?;
        let action = match fields[2] {
            "SPLIT" => CorporateAction::Split {
                action_id: fields[0].to_owned(),
                instrument_id: fields[1].to_owned(),
                effective_at: fields[3].to_owned(),
                ratio: value,
            },
            "CASH_DIVIDEND" => CorporateAction::CashDividend {
                action_id: fields[0].to_owned(),
                instrument_id: fields[1].to_owned(),
                effective_at: fields[3].to_owned(),
                amount: value,
            },
            _ => {
                return Err(MarketDataError(format!(
                    "unsupported corporate-action type on row {}",
                    index + 2
                )));
            }
        };
        action.validate()?;
        actions.push(action);
    }
    if actions.is_empty() {
        return Err(MarketDataError(
            "corporate-action CSV contains no data rows".to_owned(),
        ));
    }
    actions.sort_by(|left, right| {
        left.effective_at()
            .cmp(right.effective_at())
            .then_with(|| left.action_id().cmp(right.action_id()))
    });
    Ok(actions)
}

/// Applies a declared action version to bars before each action's effective time.
pub fn adjust_bars_for_corporate_actions(
    bars: impl IntoIterator<Item = (String, Bar)>,
    actions: &[CorporateAction],
) -> Result<Vec<(String, Bar)>, MarketDataError> {
    let mut ordered_actions = actions.to_vec();
    ordered_actions.sort_by(|left, right| {
        left.effective_at()
            .cmp(right.effective_at())
            .then_with(|| left.action_id().cmp(right.action_id()))
    });
    let mut action_ids = std::collections::BTreeSet::new();
    for action in &ordered_actions {
        action.validate()?;
        if !action_ids.insert(action.action_id()) {
            return Err(MarketDataError(format!(
                "duplicate corporate action ID: {}",
                action.action_id()
            )));
        }
    }
    let mut adjusted = Vec::new();
    for (event_time, mut bar) in bars {
        parse_utc(&event_time)?;
        for action in &ordered_actions {
            match action {
                CorporateAction::Split {
                    instrument_id,
                    effective_at,
                    ratio,
                    ..
                } if bar.instrument_id == *instrument_id
                    && event_time.as_str() < effective_at.as_str() =>
                {
                    bar.open = bar.open.checked_div(*ratio)?;
                    bar.high = bar.high.checked_div(*ratio)?;
                    bar.low = bar.low.checked_div(*ratio)?;
                    bar.close = bar.close.checked_div(*ratio)?;
                    bar.volume = bar.volume.checked_mul(*ratio)?;
                }
                CorporateAction::CashDividend {
                    instrument_id,
                    effective_at,
                    amount,
                    ..
                } if bar.instrument_id == *instrument_id
                    && event_time.as_str() < effective_at.as_str() =>
                {
                    for price in [&mut bar.open, &mut bar.high, &mut bar.low, &mut bar.close] {
                        *price = price.checked_sub(*amount)?;
                        if *price <= Decimal::ZERO {
                            return Err(MarketDataError(
                                "dividend adjustment made a bar price non-positive".to_owned(),
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
        bar.validate()?;
        adjusted.push((event_time, bar));
    }
    Ok(adjusted)
}

/// Market-data validation or deterministic construction failure.
#[derive(Debug)]
pub struct MarketDataError(pub String);

impl std::fmt::Display for MarketDataError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for MarketDataError {}

impl From<DomainError> for MarketDataError {
    fn from(error: DomainError) -> Self {
        Self(error.0)
    }
}
impl From<follon_domain::DecimalError> for MarketDataError {
    fn from(error: follon_domain::DecimalError) -> Self {
        Self(error.0)
    }
}

fn parse_utc(value: &str) -> Result<OffsetDateTime, MarketDataError> {
    if !value.ends_with('Z') {
        return Err(MarketDataError("timestamp must be UTC".to_owned()));
    }
    OffsetDateTime::parse(value, &Rfc3339).map_err(|error| MarketDataError(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn trade(time: &str, sequence: u64, price: &str, quantity: &str) -> Trade {
        Trade {
            event_time: time.to_owned(),
            instrument_id: "inst.us_equity.spy".to_owned(),
            trade_id: format!("trade-{sequence:03}"),
            source_sequence: sequence,
            price: Decimal::from_str(price).unwrap(),
            quantity: Decimal::from_str(quantity).unwrap(),
        }
    }

    #[test]
    fn builder_sorts_trades_and_uses_fixed_time_buckets() {
        let builder = BarBuilder::new(60, "America/New_York").unwrap();
        let bars = builder
            .build([
                trade("2026-01-02T14:30:50Z", 2, "101", "2"),
                trade("2026-01-02T14:30:01Z", 1, "100", "1"),
            ])
            .unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].0, "2026-01-02T14:30:00Z");
        assert_eq!(bars[0].1.open, Decimal::from_integer(100).unwrap());
        assert_eq!(bars[0].1.close, Decimal::from_integer(101).unwrap());
        assert_eq!(bars[0].1.volume, Decimal::from_integer(3).unwrap());
    }

    #[test]
    fn split_adjustment_is_exact_and_versioned() {
        let builder = BarBuilder::new(60, "America/New_York").unwrap();
        let bars = builder
            .build([trade("2026-01-02T14:30:00Z", 1, "100", "1")])
            .unwrap();
        let adjusted = adjust_bars_for_corporate_actions(
            bars,
            &[CorporateAction::Split {
                action_id: "action-split-001".to_owned(),
                instrument_id: "inst.us_equity.spy".to_owned(),
                effective_at: "2026-01-03T00:00:00Z".to_owned(),
                ratio: Decimal::from_integer(2).unwrap(),
            }],
        )
        .unwrap();
        assert_eq!(adjusted[0].1.close, Decimal::from_integer(50).unwrap());
        assert_eq!(adjusted[0].1.volume, Decimal::from_integer(2).unwrap());
    }

    #[test]
    fn action_importer_preserves_identity_and_sorts_effective_time() {
        let actions = import_corporate_actions(
            "action_id,instrument_id,action_type,effective_at,value\n\
             action-dividend-001,inst.us_equity.spy,CASH_DIVIDEND,2026-01-03T00:00:00Z,0.5\n\
             action-split-001,inst.us_equity.spy,SPLIT,2026-01-02T00:00:00Z,2\n",
        )
        .unwrap();
        assert_eq!(actions[0].action_id(), "action-split-001");
        assert_eq!(actions[1].action_id(), "action-dividend-001");
        assert!(import_corporate_actions(
            "action_id,instrument_id,action_type,effective_at,value\n\
             action-split-001,inst.us_equity.spy,SPLIT,2026-01-02T00:00:00Z,2\n\
             action-split-001,inst.us_equity.spy,SPLIT,2026-01-03T00:00:00Z,3\n",
        )
        .is_err());
    }

    #[test]
    fn trade_importer_and_builder_use_source_sequence_for_same_timestamp_order() {
        let trades = import_trades(
            "event_time,instrument_id,trade_id,source_sequence,price,quantity\n\
             2026-01-02T14:30:00Z,inst.us_equity.spy,trade-002,2,101,1\n\
             2026-01-02T14:30:00Z,inst.us_equity.spy,trade-001,1,100,1\n",
        )
        .unwrap();
        let bars = BarBuilder::new(60, "America/New_York")
            .unwrap()
            .build(trades)
            .unwrap();
        assert_eq!(bars[0].1.open, Decimal::from_integer(100).unwrap());
        assert_eq!(bars[0].1.close, Decimal::from_integer(101).unwrap());
    }

    #[test]
    fn bar_builder_rejects_ambiguous_duplicate_source_sequences() {
        let builder = BarBuilder::new(60, "America/New_York").unwrap();
        let mut second = trade("2026-01-02T14:30:01Z", 1, "101", "1");
        second.trade_id = "trade-duplicate-sequence".to_owned();
        assert!(builder
            .build([trade("2026-01-02T14:30:00Z", 1, "100", "1"), second])
            .is_err());
    }
}
