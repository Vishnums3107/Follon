//! Deterministic historical market-data normalization, bar construction, and actions.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Bar, Decimal, DomainError};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub mod rights;

pub use rights::{
    CorporateActionPolicy, DataRightsAndSemanticsReceipt, DataRightsLedger, LicenseTier,
};

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
        validate_utc_timestamp("trade event_time", &self.event_time)?;
        if self.price <= Decimal::ZERO || self.quantity <= Decimal::ZERO {
            return Err(DomainError(
                "trade must have positive price and positive quantity".to_owned(),
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
        let mut grouped: BTreeMap<(i64, String), Vec<Trade>> = BTreeMap::new();
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
                .entry((bucket, trade.instrument_id.clone()))
                .or_default()
                .push(trade);
        }
        let mut bars = Vec::with_capacity(grouped.len());
        for ((bucket, instrument_id), mut bucket_trades) in grouped {
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
        validate_utc_timestamp("corporate action effective_at", effective_at)?;
        if *value <= Decimal::ZERO {
            return Err(DomainError(
                "corporate action must have a positive value".to_owned(),
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
    validate_utc_timestamp("timestamp", value)?;
    OffsetDateTime::parse(value, &Rfc3339).map_err(|error| MarketDataError(error.to_string()))
}

/// Normalized top-of-book quote with both source and local receive time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quote {
    /// Provider event time in canonical UTC.
    pub event_time: String,
    /// Local adapter receive time in canonical UTC.
    pub received_at: String,
    /// Canonical resolved instrument identifier.
    pub instrument_id: String,
    /// Immutable provider quote identity.
    pub quote_id: String,
    /// Monotonic provider sequence for the instrument/channel.
    pub source_sequence: u64,
    /// Positive best bid.
    pub bid_price: Decimal,
    /// Non-negative displayed bid quantity.
    pub bid_quantity: Decimal,
    /// Positive best ask.
    pub ask_price: Decimal,
    /// Non-negative displayed ask quantity.
    pub ask_quantity: Decimal,
}

impl Quote {
    /// Rejects crossed, unsequenced, non-canonical, or time-traveling quotes.
    pub fn validate(&self) -> Result<(), MarketDataError> {
        validate_canonical_id("quote instrument_id", &self.instrument_id)?;
        validate_canonical_id("quote_id", &self.quote_id)?;
        let event_time = parse_utc(&self.event_time)?;
        let received_at = parse_utc(&self.received_at)?;
        if received_at < event_time
            || self.source_sequence == 0
            || self.bid_price <= Decimal::ZERO
            || self.ask_price <= Decimal::ZERO
            || self.bid_price > self.ask_price
            || self.bid_quantity < Decimal::ZERO
            || self.ask_quantity < Decimal::ZERO
        {
            return Err(MarketDataError("invalid normalized quote".to_owned()));
        }
        Ok(())
    }
}

/// Feed-quality condition for one explicit observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeedStatus {
    /// Sequence and timing are inside policy.
    Healthy,
    /// Source-to-receive latency exceeded policy.
    Delayed,
    /// The quote is too old for the supplied decision time.
    Stale,
    /// One or more provider sequences are missing.
    SequenceGap,
    /// A lower sequence arrived after a newer quote.
    OutOfOrder,
    /// An immutable provider quote was observed again.
    Duplicate,
}

/// Explicit quality thresholds; no wall clock is consulted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedQualityPolicy {
    /// Maximum source-to-adapter delay.
    pub maximum_transport_delay_milliseconds: i128,
    /// Maximum source age at a risk/strategy decision.
    pub maximum_staleness_seconds: i64,
}

/// Auditable result from one feed-quality observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeedQualitySnapshot {
    /// Instrument observed.
    pub instrument_id: String,
    /// Quote identity.
    pub quote_id: String,
    /// Provider sequence.
    pub source_sequence: u64,
    /// Derived quality state.
    pub status: FeedStatus,
    /// Exact transport delay.
    pub transport_delay_milliseconds: i128,
    /// Exact age at the supplied decision instant.
    pub staleness_seconds: i64,
    /// First missing sequence, when a gap is found.
    pub missing_sequence_from: Option<u64>,
    /// Last missing sequence, when a gap is found.
    pub missing_sequence_to: Option<u64>,
}

/// Deterministic per-instrument feed sequence and freshness monitor.
#[derive(Default)]
pub struct FeedQualityMonitor {
    last_sequence: BTreeMap<String, u64>,
    quote_ids: BTreeSet<String>,
}

impl FeedQualityMonitor {
    /// Observes one normalized quote at an explicit decision time.
    pub fn observe(
        &mut self,
        quote: &Quote,
        as_of: &str,
        policy: &FeedQualityPolicy,
    ) -> Result<FeedQualitySnapshot, MarketDataError> {
        quote.validate()?;
        if policy.maximum_transport_delay_milliseconds < 0
            || policy.maximum_staleness_seconds < 0
            || self.quote_ids.len() > 10_000_000
        {
            return Err(MarketDataError(
                "invalid feed-quality policy or state".to_owned(),
            ));
        }
        let event_time = parse_utc(&quote.event_time)?;
        let received_at = parse_utc(&quote.received_at)?;
        let as_of = parse_utc(as_of)?;
        if as_of < received_at {
            return Err(MarketDataError(
                "feed decision time precedes adapter receipt".to_owned(),
            ));
        }
        let transport_delay_milliseconds = (received_at - event_time).whole_milliseconds();
        let staleness_seconds = (as_of - event_time).whole_seconds();
        let duplicate = self.quote_ids.contains(&quote.quote_id);
        let prior = self.last_sequence.get(&quote.instrument_id).copied();
        let out_of_order = prior.is_some_and(|sequence| quote.source_sequence < sequence);
        let gap = prior.and_then(|sequence| {
            sequence
                .checked_add(1)
                .filter(|expected| quote.source_sequence > *expected)
                .map(|expected| (expected, quote.source_sequence - 1))
        });
        let status = if duplicate {
            FeedStatus::Duplicate
        } else if out_of_order || prior == Some(quote.source_sequence) {
            FeedStatus::OutOfOrder
        } else if gap.is_some() {
            FeedStatus::SequenceGap
        } else if staleness_seconds > policy.maximum_staleness_seconds {
            FeedStatus::Stale
        } else if transport_delay_milliseconds > policy.maximum_transport_delay_milliseconds {
            FeedStatus::Delayed
        } else {
            FeedStatus::Healthy
        };
        if !duplicate {
            self.quote_ids.insert(quote.quote_id.clone());
        }
        if matches!(
            status,
            FeedStatus::Healthy | FeedStatus::Delayed | FeedStatus::Stale | FeedStatus::SequenceGap
        ) {
            self.last_sequence
                .insert(quote.instrument_id.clone(), quote.source_sequence);
        }
        Ok(FeedQualitySnapshot {
            instrument_id: quote.instrument_id.clone(),
            quote_id: quote.quote_id.clone(),
            source_sequence: quote.source_sequence,
            status,
            transport_delay_milliseconds,
            staleness_seconds,
            missing_sequence_from: gap.map(|value| value.0),
            missing_sequence_to: gap.map(|value| value.1),
        })
    }
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
    fn builder_emits_canonical_time_then_instrument_order() {
        let mut earlier = trade("2026-01-02T14:30:01Z", 1, "90", "1");
        earlier.instrument_id = "inst.us_equity.qqq".to_owned();
        earlier.trade_id = "trade-qqq-001".to_owned();
        let later = trade("2026-01-02T14:31:01Z", 1, "100", "1");
        let bars = BarBuilder::new(60, "America/New_York")
            .unwrap()
            .build([later, earlier])
            .unwrap();
        assert_eq!(bars[0].0, "2026-01-02T14:30:00Z");
        assert_eq!(bars[0].1.instrument_id, "inst.us_equity.qqq");
        assert_eq!(bars[1].0, "2026-01-02T14:31:00Z");
        assert_eq!(bars[1].1.instrument_id, "inst.us_equity.spy");
    }

    #[test]
    fn market_data_rejects_noncanonical_utc_spellings() {
        assert!(trade("2026-01-02T14:30:00.000Z", 1, "100", "1")
            .validate()
            .is_err());
        assert!(CorporateAction::Split {
            action_id: "action-split-001".to_owned(),
            instrument_id: "inst.us_equity.spy".to_owned(),
            effective_at: "2026-01-03T00:00:00+00:00".to_owned(),
            ratio: Decimal::from_integer(2).unwrap(),
        }
        .validate()
        .is_err());
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

    #[test]
    fn quote_monitor_detects_delay_staleness_gaps_duplicates_and_reordering() {
        let policy = FeedQualityPolicy {
            maximum_transport_delay_milliseconds: 100,
            maximum_staleness_seconds: 2,
        };
        let quote = |id: &str, sequence: u64, event: &str, received: &str| Quote {
            event_time: event.to_owned(),
            received_at: received.to_owned(),
            instrument_id: "instrument.spy".to_owned(),
            quote_id: id.to_owned(),
            source_sequence: sequence,
            bid_price: Decimal::from_str("100").unwrap(),
            bid_quantity: Decimal::from_str("10").unwrap(),
            ask_price: Decimal::from_str("100.01").unwrap(),
            ask_quantity: Decimal::from_str("12").unwrap(),
        };
        let mut monitor = FeedQualityMonitor::default();
        let first = quote(
            "quote.one",
            10,
            "2026-01-02T14:30:00Z",
            "2026-01-02T14:30:00Z",
        );
        assert_eq!(
            monitor
                .observe(&first, "2026-01-02T14:30:01Z", &policy)
                .unwrap()
                .status,
            FeedStatus::Healthy
        );
        let gap = monitor
            .observe(
                &quote(
                    "quote.gap",
                    13,
                    "2026-01-02T14:30:01Z",
                    "2026-01-02T14:30:02Z",
                ),
                "2026-01-02T14:30:04Z",
                &policy,
            )
            .unwrap();
        assert_eq!(gap.status, FeedStatus::SequenceGap);
        assert_eq!(gap.missing_sequence_from, Some(11));
        assert_eq!(gap.missing_sequence_to, Some(12));
        assert_eq!(
            monitor
                .observe(&first, "2026-01-02T14:30:04Z", &policy)
                .unwrap()
                .status,
            FeedStatus::Duplicate
        );
        assert_eq!(
            monitor
                .observe(
                    &quote(
                        "quote.old",
                        12,
                        "2026-01-02T14:30:02Z",
                        "2026-01-02T14:30:02Z",
                    ),
                    "2026-01-02T14:30:03Z",
                    &policy,
                )
                .unwrap()
                .status,
            FeedStatus::OutOfOrder
        );

        let delayed = FeedQualityMonitor::default()
            .observe(
                &quote(
                    "quote.delayed",
                    1,
                    "2026-01-02T14:30:00Z",
                    "2026-01-02T14:30:01Z",
                ),
                "2026-01-02T14:30:01Z",
                &policy,
            )
            .unwrap();
        assert_eq!(delayed.status, FeedStatus::Delayed);

        let stale = FeedQualityMonitor::default()
            .observe(
                &quote(
                    "quote.stale",
                    1,
                    "2026-01-02T14:30:00Z",
                    "2026-01-02T14:30:00Z",
                ),
                "2026-01-02T14:30:03Z",
                &policy,
            )
            .unwrap();
        assert_eq!(stale.status, FeedStatus::Stale);
    }
}
