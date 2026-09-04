//! Deterministic FX reference and pricing contracts.
//!
//! This crate owns no network transport, credentials, clock, or order route.
//! A caller supplies every timestamp, value date, and vendor snapshot.  The
//! resulting mark can then be passed through the normal Risk/OMS path.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal, DomainError};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// FX reference or pricing validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FxError(pub String);

impl fmt::Display for FxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for FxError {}

impl From<DomainError> for FxError {
    fn from(error: DomainError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DecimalError> for FxError {
    fn from(error: follon_domain::DecimalError) -> Self {
        Self(error.0)
    }
}

/// FX product represented by an instrument and a pricing snapshot.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum FxProduct {
    /// Two-currency spot exchange for a specified value date.
    Spot,
    /// A single outright FX forward.
    Forward,
    /// An FX swap with a near and far value date.
    Swap,
}

impl FxProduct {
    /// Stable wire and risk-bucket representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Spot => "FX_SPOT",
            Self::Forward => "FX_FORWARD",
            Self::Swap => "FX_SWAP",
        }
    }

    /// Stable lower-case bucket used by canonical risk contracts.
    pub const fn risk_bucket(self) -> &'static str {
        match self {
            Self::Spot => "fx_spot",
            Self::Forward => "fx_forward",
            Self::Swap => "fx_swap",
        }
    }
}

/// Validated base/quote pair.  One base unit costs the quoted price in quote currency.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct FxPair {
    base_currency: String,
    quote_currency: String,
}

impl FxPair {
    /// Creates a canonical, non-self FX pair.
    pub fn new(
        base_currency: impl Into<String>,
        quote_currency: impl Into<String>,
    ) -> Result<Self, FxError> {
        let pair = Self {
            base_currency: base_currency.into(),
            quote_currency: quote_currency.into(),
        };
        pair.validate()?;
        Ok(pair)
    }

    /// Validates ISO-4217-style currencies and rejects a self-pair.
    pub fn validate(&self) -> Result<(), FxError> {
        for (name, currency) in [
            ("FX base currency", &self.base_currency),
            ("FX quote currency", &self.quote_currency),
        ] {
            if currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_uppercase()) {
                return Err(FxError(format!(
                    "{name} must be an uppercase three-letter code"
                )));
            }
        }
        if self.base_currency == self.quote_currency {
            return Err(FxError(
                "FX base and quote currencies must differ".to_owned(),
            ));
        }
        Ok(())
    }

    /// Returns the base currency.
    pub fn base_currency(&self) -> &str {
        &self.base_currency
    }

    /// Returns the quote currency.
    pub fn quote_currency(&self) -> &str {
        &self.quote_currency
    }

    /// Returns a stable display-independent pair key.
    pub fn key(&self) -> String {
        format!("{}/{}", self.base_currency, self.quote_currency)
    }
}

/// A calendar value date validated without a local timezone or machine clock.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct FxValueDate(String);

impl FxValueDate {
    /// Creates a canonical `YYYY-MM-DD` value date.
    pub fn new(value: impl Into<String>) -> Result<Self, FxError> {
        let date = Self(value.into());
        date.validate()?;
        Ok(date)
    }

    /// Returns the canonical date text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validates a real UTC calendar date.
    pub fn validate(&self) -> Result<(), FxError> {
        if self.0.len() != 10
            || self.0.as_bytes().get(4) != Some(&b'-')
            || self.0.as_bytes().get(7) != Some(&b'-')
        {
            return Err(FxError(
                "FX value date must use canonical YYYY-MM-DD form".to_owned(),
            ));
        }
        let timestamp = format!("{}T00:00:00Z", self.0);
        validate_utc_timestamp("FX value date", &timestamp)?;
        Ok(())
    }
}

/// A positive bid/ask quote for one base unit in quote currency.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FxOutrightQuote {
    /// Bid price.
    pub bid: Decimal,
    /// Ask price.
    pub ask: Decimal,
}

impl FxOutrightQuote {
    /// Validates bid/ask ordering.
    pub fn validate(&self) -> Result<(), FxError> {
        if self.bid <= Decimal::ZERO || self.ask < self.bid {
            return Err(FxError(
                "FX quote requires positive bid and ask >= bid".to_owned(),
            ));
        }
        Ok(())
    }

    /// Returns the exact fixed-point midpoint.
    pub fn midpoint(&self) -> Result<Decimal, FxError> {
        self.validate()?;
        Ok(self
            .bid
            .checked_add(self.ask)?
            .checked_div(Decimal::from_integer(2)?)?)
    }
}

/// Price terms for a spot/forward outright or both legs of an FX swap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FxPriceTerms {
    /// One executable outright for the stated value date.
    Outright {
        /// Value date of the exchange.
        value_date: FxValueDate,
        /// Bid/ask outright price.
        quote: FxOutrightQuote,
    },
    /// Independently priced near and far legs of an FX swap.
    Swap {
        /// Earlier value date.
        near_value_date: FxValueDate,
        /// Later value date.
        far_value_date: FxValueDate,
        /// Near-leg outright bid/ask price.
        near_quote: FxOutrightQuote,
        /// Far-leg outright bid/ask price.
        far_quote: FxOutrightQuote,
    },
}

impl FxPriceTerms {
    fn validate_for(&self, product: FxProduct) -> Result<(), FxError> {
        match (product, self) {
            (FxProduct::Spot | FxProduct::Forward, Self::Outright { value_date, quote }) => {
                value_date.validate()?;
                quote.validate()
            }
            (
                FxProduct::Swap,
                Self::Swap {
                    near_value_date,
                    far_value_date,
                    near_quote,
                    far_quote,
                },
            ) => {
                near_value_date.validate()?;
                far_value_date.validate()?;
                if near_value_date >= far_value_date {
                    return Err(FxError(
                        "FX swap near value date must precede far value date".to_owned(),
                    ));
                }
                near_quote.validate()?;
                far_quote.validate()
            }
            _ => Err(FxError(
                "FX pricing terms do not match the product kind".to_owned(),
            )),
        }
    }
}

/// Immutable versioned FX price observation from a normalized data source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FxPricingSnapshot {
    /// Immutable snapshot identity.
    pub snapshot_id: String,
    /// Immutable pricing/reference contract version.
    pub reference_version: String,
    /// Canonical instrument identity.
    pub instrument_id: String,
    /// Spot, forward, or swap product kind.
    pub product: FxProduct,
    /// Base and quote currencies.
    pub pair: FxPair,
    /// Value dates and bid/ask prices.
    pub terms: FxPriceTerms,
    /// Canonical normalized source identity.
    pub source_id: String,
    /// Monotonic source sequence for this instrument/source stream.
    pub source_sequence: u64,
    /// Time reported by the source.
    pub source_time: String,
    /// Time received by the normalized feed boundary.
    pub received_at: String,
}

impl FxPricingSnapshot {
    /// Validates the immutable pricing observation.
    pub fn validate(&self) -> Result<(), FxError> {
        for (name, value) in [
            ("FX snapshot_id", &self.snapshot_id),
            ("FX reference_version", &self.reference_version),
            ("FX instrument_id", &self.instrument_id),
            ("FX source_id", &self.source_id),
        ] {
            validate_canonical_id(name, value)?;
        }
        self.pair.validate()?;
        self.terms.validate_for(self.product)?;
        validate_utc_timestamp("FX source_time", &self.source_time)?;
        validate_utc_timestamp("FX received_at", &self.received_at)?;
        if self.source_time > self.received_at {
            return Err(FxError(
                "FX source time cannot follow received time".to_owned(),
            ));
        }
        Ok(())
    }

    /// Gets the exact midpoint for a value date after explicit freshness validation.
    pub fn midpoint_at(
        &self,
        value_date: &FxValueDate,
        as_of: &str,
        maximum_age_seconds: i64,
    ) -> Result<Decimal, FxError> {
        self.validate()?;
        ensure_fresh(&self.received_at, as_of, maximum_age_seconds)?;
        match &self.terms {
            FxPriceTerms::Outright {
                value_date: observed_date,
                quote,
            } if observed_date == value_date => quote.midpoint(),
            FxPriceTerms::Swap {
                near_value_date,
                far_value_date,
                near_quote,
                far_quote,
            } if near_value_date == value_date => near_quote.midpoint(),
            FxPriceTerms::Swap {
                near_value_date: _,
                far_value_date,
                near_quote: _,
                far_quote,
            } if far_value_date == value_date => far_quote.midpoint(),
            _ => Err(FxError(
                "FX pricing snapshot does not contain the requested value date".to_owned(),
            )),
        }
    }

    /// Returns a stable content-addressed record for source datasets and audit evidence.
    pub fn canonical_record(&self) -> Result<String, FxError> {
        self.validate()?;
        let terms = match &self.terms {
            FxPriceTerms::Outright { value_date, quote } => format!(
                "OUTRIGHT|{}|{}|{}",
                value_date.as_str(),
                quote.bid,
                quote.ask
            ),
            FxPriceTerms::Swap {
                near_value_date,
                far_value_date,
                near_quote,
                far_quote,
            } => format!(
                "SWAP|{}|{}|{}|{}|{}|{}",
                near_value_date.as_str(),
                far_value_date.as_str(),
                near_quote.bid,
                near_quote.ask,
                far_quote.bid,
                far_quote.ask
            ),
        };
        Ok(format!(
            "FX_PRICE_V1|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.snapshot_id,
            self.reference_version,
            self.instrument_id,
            self.product.as_str(),
            self.pair.key(),
            terms,
            self.source_id,
            self.source_sequence,
            self.source_time,
            self.received_at
        ))
    }

    /// Converts the received timestamp to Unix seconds for existing accounting projections.
    pub fn received_at_epoch_seconds(&self) -> Result<i64, FxError> {
        self.validate()?;
        parse_utc(&self.received_at).map(|instant| instant.unix_timestamp())
    }
}

/// Deterministic, immutable-in-input pricing collection selected at an explicit time.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FxPricingBook {
    snapshots: BTreeMap<String, FxPricingSnapshot>,
}

impl FxPricingBook {
    /// Builds a book from source snapshots.  Input order cannot affect results.
    pub fn from_snapshots(
        snapshots: impl IntoIterator<Item = FxPricingSnapshot>,
    ) -> Result<Self, FxError> {
        let mut book = Self::default();
        let mut source_identities = BTreeSet::new();
        for snapshot in snapshots {
            snapshot.validate()?;
            let source_identity = (
                snapshot.instrument_id.clone(),
                snapshot.source_id.clone(),
                snapshot.source_sequence,
            );
            if !source_identities.insert(source_identity) {
                return Err(FxError(
                    "FX pricing book has ambiguous source sequence evidence".to_owned(),
                ));
            }
            if book
                .snapshots
                .insert(snapshot.snapshot_id.clone(), snapshot)
                .is_some()
            {
                return Err(FxError(
                    "FX pricing book has duplicate snapshot_id".to_owned(),
                ));
            }
        }
        Ok(book)
    }

    /// Selects the latest eligible snapshot and its exact midpoint for a value date.
    pub fn midpoint_at(
        &self,
        instrument_id: &str,
        product: FxProduct,
        value_date: &FxValueDate,
        as_of: &str,
        maximum_age_seconds: i64,
    ) -> Result<(String, Decimal), FxError> {
        validate_canonical_id("FX instrument_id", instrument_id)?;
        validate_utc_timestamp("FX pricing as_of", as_of)?;
        if maximum_age_seconds < 0 {
            return Err(FxError(
                "FX maximum quote age cannot be negative".to_owned(),
            ));
        }
        let candidate = self
            .snapshots
            .values()
            .filter(|snapshot| {
                snapshot.instrument_id == instrument_id
                    && snapshot.product == product
                    && snapshot.received_at.as_str() <= as_of
                    && snapshot
                        .midpoint_at(value_date, as_of, maximum_age_seconds)
                        .is_ok()
            })
            .max_by(|left, right| {
                left.received_at
                    .cmp(&right.received_at)
                    .then_with(|| left.source_time.cmp(&right.source_time))
                    .then_with(|| left.source_sequence.cmp(&right.source_sequence))
                    .then_with(|| left.snapshot_id.cmp(&right.snapshot_id))
            })
            .ok_or_else(|| FxError("missing fresh FX pricing snapshot".to_owned()))?;
        Ok((
            candidate.snapshot_id.clone(),
            candidate.midpoint_at(value_date, as_of, maximum_age_seconds)?,
        ))
    }
}

fn ensure_fresh(received_at: &str, as_of: &str, maximum_age_seconds: i64) -> Result<(), FxError> {
    if maximum_age_seconds < 0 {
        return Err(FxError(
            "FX maximum quote age cannot be negative".to_owned(),
        ));
    }
    validate_utc_timestamp("FX pricing as_of", as_of)?;
    let received = parse_utc(received_at)?;
    let evaluation = parse_utc(as_of)?;
    let age = evaluation
        .unix_timestamp()
        .checked_sub(received.unix_timestamp())
        .ok_or_else(|| FxError("FX quote age overflow".to_owned()))?;
    if age < 0 || age > maximum_age_seconds {
        return Err(FxError(
            "FX price is unavailable or stale at evaluation time".to_owned(),
        ));
    }
    Ok(())
}

fn parse_utc(value: &str) -> Result<OffsetDateTime, FxError> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| FxError("invalid canonical FX UTC timestamp".to_owned()))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn amount(value: &str) -> Decimal {
        Decimal::from_str(value).expect("decimal")
    }

    fn snapshot(
        snapshot_id: &str,
        sequence: u64,
        received_at: &str,
        product: FxProduct,
        terms: FxPriceTerms,
    ) -> FxPricingSnapshot {
        FxPricingSnapshot {
            snapshot_id: snapshot_id.to_owned(),
            reference_version: "fx.price.v1".to_owned(),
            instrument_id: "instrument.fx.eur-usd".to_owned(),
            product,
            pair: FxPair::new("EUR", "USD").unwrap(),
            terms,
            source_id: "source.fixture".to_owned(),
            source_sequence: sequence,
            source_time: "2026-01-02T10:00:00Z".to_owned(),
            received_at: received_at.to_owned(),
        }
    }

    fn outright(value_date: &str, bid: &str, ask: &str) -> FxPriceTerms {
        FxPriceTerms::Outright {
            value_date: FxValueDate::new(value_date).unwrap(),
            quote: FxOutrightQuote {
                bid: amount(bid),
                ask: amount(ask),
            },
        }
    }

    #[test]
    fn pricing_book_is_input_order_independent_and_selects_latest_fresh_quote() {
        let older = snapshot(
            "fx.snapshot.001",
            1,
            "2026-01-02T10:00:01Z",
            FxProduct::Spot,
            outright("2026-01-06", "1.1000", "1.1002"),
        );
        let newer = snapshot(
            "fx.snapshot.002",
            2,
            "2026-01-02T10:00:02Z",
            FxProduct::Spot,
            outright("2026-01-06", "1.1004", "1.1006"),
        );
        let forward = FxPricingBook::from_snapshots(vec![older.clone(), newer.clone()]).unwrap();
        let replay = FxPricingBook::from_snapshots(vec![newer, older]).unwrap();
        assert_eq!(forward, replay);
        assert_eq!(
            forward
                .midpoint_at(
                    "instrument.fx.eur-usd",
                    FxProduct::Spot,
                    &FxValueDate::new("2026-01-06").unwrap(),
                    "2026-01-02T10:00:03Z",
                    5,
                )
                .unwrap(),
            ("fx.snapshot.002".to_owned(), amount("1.1005"))
        );
        assert_eq!(
            forward
                .snapshots
                .get("fx.snapshot.002")
                .unwrap()
                .canonical_record()
                .unwrap(),
            "FX_PRICE_V1|fx.snapshot.002|fx.price.v1|instrument.fx.eur-usd|FX_SPOT|EUR/USD|OUTRIGHT|2026-01-06|1.10040000|1.10060000|source.fixture|2|2026-01-02T10:00:00Z|2026-01-02T10:00:02Z"
        );
    }

    #[test]
    fn pricing_rejects_stale_dates_and_ambiguous_source_evidence() {
        let spot = snapshot(
            "fx.snapshot.001",
            1,
            "2026-01-02T10:00:00Z",
            FxProduct::Spot,
            outright("2026-01-06", "1.1000", "1.1002"),
        );
        assert!(spot
            .midpoint_at(
                &FxValueDate::new("2026-01-06").unwrap(),
                "2026-01-02T10:00:06Z",
                5,
            )
            .is_err());
        assert!(spot
            .midpoint_at(
                &FxValueDate::new("2026-01-07").unwrap(),
                "2026-01-02T10:00:01Z",
                5,
            )
            .is_err());

        let duplicate_sequence = snapshot(
            "fx.snapshot.002",
            1,
            "2026-01-02T10:00:02Z",
            FxProduct::Spot,
            outright("2026-01-06", "1.1004", "1.1006"),
        );
        assert!(FxPricingBook::from_snapshots(vec![spot, duplicate_sequence]).is_err());
    }

    #[test]
    fn swap_requires_ordered_dates_and_prices_both_legs() {
        let swap = snapshot(
            "fx.snapshot.swap.001",
            1,
            "2026-01-02T10:00:01Z",
            FxProduct::Swap,
            FxPriceTerms::Swap {
                near_value_date: FxValueDate::new("2026-01-06").unwrap(),
                far_value_date: FxValueDate::new("2026-02-06").unwrap(),
                near_quote: FxOutrightQuote {
                    bid: amount("1.1000"),
                    ask: amount("1.1002"),
                },
                far_quote: FxOutrightQuote {
                    bid: amount("1.1020"),
                    ask: amount("1.1024"),
                },
            },
        );
        assert_eq!(
            swap.midpoint_at(
                &FxValueDate::new("2026-02-06").unwrap(),
                "2026-01-02T10:00:02Z",
                5,
            )
            .unwrap(),
            amount("1.1022")
        );
        let invalid = FxPriceTerms::Swap {
            near_value_date: FxValueDate::new("2026-02-06").unwrap(),
            far_value_date: FxValueDate::new("2026-01-06").unwrap(),
            near_quote: FxOutrightQuote {
                bid: amount("1.1"),
                ask: amount("1.2"),
            },
            far_quote: FxOutrightQuote {
                bid: amount("1.1"),
                ask: amount("1.2"),
            },
        };
        let invalid_snapshot = snapshot(
            "fx.snapshot.swap.002",
            2,
            "2026-01-02T10:00:02Z",
            FxProduct::Swap,
            invalid,
        );
        assert!(invalid_snapshot.validate().is_err());
    }
}
