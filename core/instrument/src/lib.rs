//! Canonical instrument reference data and explicit exchange-session models.
//!
//! Reference data and calendars are selected by replay time; this crate never
//! queries a machine clock or treats a display ticker as a durable identity.

use std::collections::{BTreeMap, HashMap};

use follon_domain::{validate_canonical_id, validate_utc_timestamp, Decimal, DomainError};

/// Asset classes accepted in the first release while retaining extension names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetClass {
    /// Common stock.
    Equity,
    /// Exchange-traded fund.
    Etf,
    /// Reserved extension point; no option-chain behavior exists yet.
    Option,
    /// Reserved extension point; no futures behavior exists yet.
    Future,
}

impl AssetClass {
    /// Stable wire representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Equity => "EQUITY",
            Self::Etf => "ETF",
            Self::Option => "OPTION",
            Self::Future => "FUTURE",
        }
    }
}

/// Reference data required before an event reaches strategy or portfolio logic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Instrument {
    /// Permanent internal identity; never a display ticker.
    pub instrument_id: String,
    /// User-visible symbol, which may change over time.
    pub symbol: String,
    /// Venue-specific symbol.
    pub exchange_symbol: String,
    /// Product class.
    pub asset_class: AssetClass,
    /// Trading venue identity.
    pub venue: String,
    /// ISO currency code.
    pub currency: String,
    /// Broker-specific identifiers keyed by a canonical adapter identity.
    pub broker_ids: BTreeMap<String, String>,
    /// Exact minimum price increment.
    pub tick_size: Decimal,
    /// Exact minimum trade quantity.
    pub lot_size: Decimal,
    /// Exact contract multiplier (one for first-slice equities/ETFs).
    pub multiplier: Decimal,
    /// Explicit calendar configuration selected by the instrument.
    pub trading_calendar_id: String,
}

impl Instrument {
    /// Validates first-slice reference data without accepting options/futures behavior.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_canonical_id("instrument_id", &self.instrument_id)?;
        validate_canonical_id("venue", &self.venue)?;
        validate_canonical_id("trading_calendar_id", &self.trading_calendar_id)?;
        if self.symbol.is_empty()
            || self.exchange_symbol.is_empty()
            || self.currency.len() != 3
            || !self.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            || self.tick_size <= Decimal::ZERO
            || self.lot_size <= Decimal::ZERO
            || self.multiplier <= Decimal::ZERO
        {
            return Err(DomainError(
                "instrument has invalid trading reference data".to_owned(),
            ));
        }
        for (adapter, broker_id) in &self.broker_ids {
            validate_canonical_id("broker adapter", adapter)?;
            if broker_id.is_empty() {
                return Err(DomainError(
                    "broker instrument ID cannot be empty".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// One effective-dated immutable version of reference data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstrumentVersion {
    /// The instrument identity carried by market and order events.
    pub instrument: Instrument,
    /// UTC instant at which this version becomes valid.
    pub effective_from: String,
    /// UTC instant at which this version ceases to be valid, if superseded.
    pub effective_to: Option<String>,
    /// Immutable vendor/reference-data revision.
    pub reference_version: String,
}

impl InstrumentVersion {
    /// Validates dates sufficiently for deterministic lexical UTC comparison.
    pub fn validate(&self) -> Result<(), DomainError> {
        self.instrument.validate()?;
        validate_utc_timestamp("instrument effective_from", &self.effective_from)?;
        if let Some(effective_to) = &self.effective_to {
            validate_utc_timestamp("instrument effective_to", effective_to)?;
            if effective_to <= &self.effective_from {
                return Err(DomainError(
                    "instrument version has invalid effective dates".to_owned(),
                ));
            }
        }
        if self.reference_version.is_empty() {
            return Err(DomainError(
                "instrument version has invalid effective dates".to_owned(),
            ));
        }
        Ok(())
    }

    fn applies_at(&self, utc_time: &str) -> bool {
        self.effective_from.as_str() <= utc_time
            && self
                .effective_to
                .as_deref()
                .is_none_or(|end| utc_time < end)
    }
}

/// In-memory, deterministic registry for versioned instrument reference data.
#[derive(Default)]
pub struct InstrumentRegistry {
    versions: HashMap<String, Vec<InstrumentVersion>>,
}

impl InstrumentRegistry {
    /// Adds a reference-data version only when it cannot overlap an existing version.
    pub fn register(&mut self, version: InstrumentVersion) -> Result<(), DomainError> {
        version.validate()?;
        let instrument_id = version.instrument.instrument_id.clone();
        let entries = self.versions.entry(instrument_id).or_default();
        if entries
            .iter()
            .any(|existing| ranges_overlap(existing, &version))
        {
            return Err(DomainError(
                "instrument reference-data ranges overlap".to_owned(),
            ));
        }
        entries.push(version);
        entries.sort_by(|left, right| left.effective_from.cmp(&right.effective_from));
        Ok(())
    }

    /// Resolves the version that was effective at a replayed UTC instant.
    pub fn resolve(&self, instrument_id: &str, utc_time: &str) -> Option<&InstrumentVersion> {
        self.versions
            .get(instrument_id)?
            .iter()
            .find(|version| version.applies_at(utc_time))
    }
}

fn ranges_overlap(left: &InstrumentVersion, right: &InstrumentVersion) -> bool {
    let left_ends_after_right_starts = left
        .effective_to
        .as_deref()
        .is_none_or(|end| end > right.effective_from.as_str());
    let right_ends_after_left_starts = right
        .effective_to
        .as_deref()
        .is_none_or(|end| end > left.effective_from.as_str());
    left_ends_after_right_starts && right_ends_after_left_starts
}

/// A configured exchange session, including its exchange-local date context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TradingSession {
    /// Exchange-local date, retained to explain holiday and early-close decisions.
    pub exchange_date: String,
    /// UTC inclusive start of the regular session.
    pub opens_at: String,
    /// UTC exclusive end of the regular session.
    pub closes_at: String,
}

impl TradingSession {
    /// Validates a session without using local machine time.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_utc_timestamp("session opens_at", &self.opens_at)?;
        validate_utc_timestamp("session closes_at", &self.closes_at)?;
        if self.exchange_date.len() != 10 || self.opens_at >= self.closes_at {
            return Err(DomainError("trading session is invalid".to_owned()));
        }
        Ok(())
    }

    /// Whether an instant belongs to this regular session.
    pub fn contains(&self, utc_time: &str) -> bool {
        self.opens_at.as_str() <= utc_time && utc_time < self.closes_at.as_str()
    }
}

/// Explicit session source used by market-data and risk checks.
pub trait TradingCalendar: Send + Sync {
    /// Stable calendar identity.
    fn calendar_id(&self) -> &str;
    /// Looks up the session that contains a supplied UTC instant.
    fn session_at(&self, utc_time: &str) -> Option<&TradingSession>;

    /// States whether an order is permitted to enter the regular session.
    fn is_open_at(&self, utc_time: &str) -> bool {
        self.session_at(utc_time).is_some()
    }
}

/// Static, version-controlled calendar appropriate for historical replay fixtures.
pub struct StaticTradingCalendar {
    calendar_id: String,
    sessions: Vec<TradingSession>,
}

impl StaticTradingCalendar {
    /// Creates an explicit calendar after rejecting overlapping or malformed sessions.
    pub fn new(
        calendar_id: impl Into<String>,
        mut sessions: Vec<TradingSession>,
    ) -> Result<Self, DomainError> {
        let calendar_id = calendar_id.into();
        validate_canonical_id("calendar_id", &calendar_id)?;
        if sessions.is_empty() {
            return Err(DomainError(
                "trading calendar must contain at least one explicit session".to_owned(),
            ));
        }
        for session in &sessions {
            session.validate()?;
        }
        sessions.sort_by(|left, right| left.opens_at.cmp(&right.opens_at));
        if sessions
            .windows(2)
            .any(|pair| pair[0].closes_at > pair[1].opens_at)
        {
            return Err(DomainError("trading calendar sessions overlap".to_owned()));
        }
        Ok(Self {
            calendar_id,
            sessions,
        })
    }
}

impl TradingCalendar for StaticTradingCalendar {
    fn calendar_id(&self) -> &str {
        &self.calendar_id
    }

    fn session_at(&self, utc_time: &str) -> Option<&TradingSession> {
        self.sessions
            .iter()
            .find(|session| session.contains(utc_time))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn version(
        effective_from: &str,
        effective_to: Option<&str>,
        symbol: &str,
    ) -> InstrumentVersion {
        InstrumentVersion {
            instrument: Instrument {
                instrument_id: "inst.us_equity.spy".to_owned(),
                symbol: symbol.to_owned(),
                exchange_symbol: symbol.to_owned(),
                asset_class: AssetClass::Etf,
                venue: "venue.nyse_arca".to_owned(),
                currency: "USD".to_owned(),
                broker_ids: BTreeMap::from([("adapter.ibkr".to_owned(), "756733".to_owned())]),
                tick_size: Decimal::from_str("0.01").unwrap(),
                lot_size: Decimal::from_integer(1).unwrap(),
                multiplier: Decimal::from_integer(1).unwrap(),
                trading_calendar_id: "cal.us_equities.nyse".to_owned(),
            },
            effective_from: effective_from.to_owned(),
            effective_to: effective_to.map(str::to_owned),
            reference_version: "ref-001".to_owned(),
        }
    }

    #[test]
    fn registry_resolves_the_version_effective_at_replay_time() {
        let mut registry = InstrumentRegistry::default();
        registry
            .register(version(
                "2026-01-01T00:00:00Z",
                Some("2026-02-01T00:00:00Z"),
                "SPY",
            ))
            .unwrap();
        registry
            .register(version("2026-02-01T00:00:00Z", None, "SPY2"))
            .unwrap();
        assert_eq!(
            registry
                .resolve("inst.us_equity.spy", "2026-02-02T00:00:00Z")
                .unwrap()
                .instrument
                .symbol,
            "SPY2"
        );
    }

    #[test]
    fn calendar_requires_explicit_regular_session() {
        assert!(StaticTradingCalendar::new("cal.us_equities.nyse", Vec::new()).is_err());
        let calendar = StaticTradingCalendar::new(
            "cal.us_equities.nyse",
            vec![TradingSession {
                exchange_date: "2026-01-02".to_owned(),
                opens_at: "2026-01-02T14:30:00Z".to_owned(),
                closes_at: "2026-01-02T21:00:00Z".to_owned(),
            }],
        )
        .unwrap();
        assert!(calendar.is_open_at("2026-01-02T14:31:00Z"));
        assert!(!calendar.is_open_at("2026-01-02T21:00:00Z"));
    }
}
