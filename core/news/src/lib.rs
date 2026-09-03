//! Fixture/local-file news ingress and deterministic sentiment representation.
//!
//! This module deliberately has no vendor transport. It validates declared local
//! fixture data, emits canonical domain values, and derives reproducible integer
//! sentiment vectors for replay and paper simulation.

pub mod nlp;
pub use nlp::NlpSentimentEngine;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use follon_domain::DomainError;
use serde_json::{Map, Value};

pub use follon_domain::{EventTaxonomy, NewsHeadline, NewsSource, SentimentVector};

/// Failure during news event processing or validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewsError(pub String);

impl fmt::Display for NewsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for NewsError {}

impl From<DomainError> for NewsError {
    fn from(error: DomainError) -> Self {
        Self(error.0)
    }
}

impl From<follon_domain::DecimalError> for NewsError {
    fn from(error: follon_domain::DecimalError) -> Self {
        Self(error.0)
    }
}

/// Parses a newline-delimited local headline fixture.
///
/// Every non-empty line must be exactly the `news-headline.schema.json` payload
/// object. The input format is deliberately payload-only: the control plane
/// creates the immutable `news.headline.v1` envelope and its evidence identity.
/// This parser performs no network, credential, or provider action.
pub fn ingest_local_headlines_ndjson(input: &str) -> Result<Vec<NewsHeadline>, NewsError> {
    let mut headlines = Vec::new();
    for (index, line) in input.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)
            .map_err(|_| NewsError(format!("news fixture line {} is not valid JSON", index + 1)))?;
        let object = value.as_object().ok_or_else(|| {
            NewsError(format!("news fixture line {} must be an object", index + 1))
        })?;
        require_exact_fields(
            object,
            &[
                "entity_tickers",
                "event_time_ns",
                "headline",
                "news_id",
                "raw_body_hash",
                "receive_time_ns",
                "sequence_number",
                "source",
            ],
            index + 1,
        )?;
        let entity_tickers = object
            .get("entity_tickers")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                NewsError(format!(
                    "news fixture line {} has invalid entity_tickers",
                    index + 1
                ))
            })?
            .iter()
            .map(|value| {
                value.as_str().map(str::to_owned).ok_or_else(|| {
                    NewsError(format!(
                        "news fixture line {} has invalid entity_tickers",
                        index + 1
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let headline = NewsHeadline {
            news_id: required_string(object, "news_id", index + 1)?.to_owned(),
            source: NewsSource::parse(required_string(object, "source", index + 1)?)?,
            headline: required_string(object, "headline", index + 1)?.to_owned(),
            raw_body_hash: required_string(object, "raw_body_hash", index + 1)?.to_owned(),
            sequence_number: required_u64(object, "sequence_number", index + 1)?,
            event_time_ns: required_u64(object, "event_time_ns", index + 1)?,
            receive_time_ns: required_u64(object, "receive_time_ns", index + 1)?,
            entity_tickers,
        };
        headline.validate()?;
        headlines.push(headline);
    }
    if headlines.is_empty() {
        return Err(NewsError(
            "news fixture contains no headline events".to_owned(),
        ));
    }
    Ok(headlines)
}

/// Converts a Unix-nanosecond event time to the canonical replay-clock second.
///
/// Nanosecond order remains in the news payload and feed sort key. The replay
/// clock is intentionally second-precision, so events occurring in the same
/// second retain their deterministic payload tie-break ordering.
pub fn replay_time_from_unix_ns(event_time_ns: u64) -> Result<String, NewsError> {
    if event_time_ns == 0 {
        return Err(NewsError("event_time_ns must be positive".to_owned()));
    }
    let seconds = event_time_ns / 1_000_000_000;
    let days = seconds / 86_400;
    let seconds_of_day = seconds % 86_400;
    let days = i64::try_from(days)
        .map_err(|_| NewsError("event_time_ns is outside the replay clock range".to_owned()))?;
    let (year, month, day) = civil_from_days(days);
    if !(0..=9_999).contains(&year) {
        return Err(NewsError(
            "event_time_ns is outside the replay clock range".to_owned(),
        ));
    }
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds_of_day / 3_600,
        (seconds_of_day % 3_600) / 60,
        seconds_of_day % 60
    ))
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i64, u64, u64) {
    // Howard Hinnant's civil-from-days algorithm, with 1970-01-01 as day 0.
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u64, day as u64)
}

fn require_exact_fields(
    object: &Map<String, Value>,
    expected: &[&str],
    line_number: usize,
) -> Result<(), NewsError> {
    if object.len() != expected.len() || expected.iter().any(|field| !object.contains_key(*field)) {
        return Err(NewsError(format!(
            "news fixture line {line_number} has missing or unknown fields"
        )));
    }
    Ok(())
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    line_number: usize,
) -> Result<&'a str, NewsError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            NewsError(format!(
                "news fixture line {line_number} has invalid {field}"
            ))
        })
}

fn required_u64(
    object: &Map<String, Value>,
    field: &str,
    line_number: usize,
) -> Result<u64, NewsError> {
    object.get(field).and_then(Value::as_u64).ok_or_else(|| {
        NewsError(format!(
            "news fixture line {line_number} has invalid {field}"
        ))
    })
}

/// A news event payload during deterministic backtest replay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NewsReplayItem {
    /// Headline event ingress.
    Headline(NewsHeadline),
    /// Extracted sentiment vector event.
    Sentiment(SentimentVector),
}

impl NewsReplayItem {
    /// Returns the canonical event timestamp in nanoseconds UTC.
    pub fn event_time_ns(&self) -> u64 {
        match self {
            Self::Headline(h) => h.event_time_ns,
            Self::Sentiment(s) => s.event_time_ns,
        }
    }
}

/// Chronologically ordered news event feed for replay backtesting.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReplayNewsFeed {
    events: Vec<NewsReplayItem>,
    headline_ids: BTreeSet<String>,
    headline_sources: BTreeMap<String, NewsSource>,
    headline_sequences: BTreeMap<String, u64>,
    sentiment_ids: BTreeSet<String>,
}

impl ReplayNewsFeed {
    /// Creates an empty replay feed.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            headline_ids: BTreeSet::new(),
            headline_sources: BTreeMap::new(),
            headline_sequences: BTreeMap::new(),
            sentiment_ids: BTreeSet::new(),
        }
    }

    /// Adds a validated item once and preserves canonical replay order.
    ///
    /// A sentiment vector is accepted only after its causative headline has
    /// entered the feed. This forbids an orphaned evidence chain even when a
    /// caller supplies events out of order.
    pub fn push(&mut self, item: NewsReplayItem) -> Result<(), NewsError> {
        match &item {
            NewsReplayItem::Headline(headline) => {
                headline.validate()?;
                if !self.headline_ids.insert(headline.news_id.clone()) {
                    return Err(NewsError("duplicate news_id in replay input".to_owned()));
                }
                self.headline_sequences
                    .insert(headline.news_id.clone(), headline.sequence_number);
                self.headline_sources
                    .insert(headline.news_id.clone(), headline.source);
            }
            NewsReplayItem::Sentiment(sentiment) => {
                sentiment.validate()?;
                if !self.headline_ids.contains(&sentiment.causation_news_id) {
                    return Err(NewsError(
                        "news sentiment has no preceding causation headline".to_owned(),
                    ));
                }
                if !self.sentiment_ids.insert(sentiment.event_id.clone()) {
                    return Err(NewsError(
                        "duplicate sentiment event_id in replay input".to_owned(),
                    ));
                }
            }
        }
        self.events.push(item);
        self.events.sort_by(|left, right| {
            replay_item_sort_key(left, &self.headline_sources, &self.headline_sequences).cmp(
                &replay_item_sort_key(right, &self.headline_sources, &self.headline_sequences),
            )
        });
        Ok(())
    }

    /// Builds a complete local-fixture replay feed in deterministic order.
    ///
    /// The classifier runs after a headline is accepted so every derived
    /// sentiment vector has a concrete causal parent. Input order never affects
    /// the resulting event order or vector identities.
    pub fn from_headlines(
        mut headlines: Vec<NewsHeadline>,
        classifier: &NlpSentimentEngine,
    ) -> Result<Self, NewsError> {
        headlines.sort_by(|left, right| {
            (left.event_time_ns, &left.news_id).cmp(&(right.event_time_ns, &right.news_id))
        });
        let mut feed = Self::new();
        for headline in headlines {
            feed.push(NewsReplayItem::Headline(headline.clone()))?;
            for sentiment in classifier.extract_sentiment_vectors(&headline)? {
                feed.push(NewsReplayItem::Sentiment(sentiment))?;
            }
        }
        Ok(feed)
    }

    /// Returns all chronologically sorted events.
    pub fn events(&self) -> &[NewsReplayItem] {
        &self.events
    }

    /// Filters sentiment events targeting a specific instrument ID.
    pub fn sentiment_events_for(&self, instrument_id: &str) -> Vec<SentimentVector> {
        self.events
            .iter()
            .filter_map(|e| match e {
                NewsReplayItem::Sentiment(s) if s.instrument_id == instrument_id => Some(s.clone()),
                _ => None,
            })
            .collect()
    }
}

fn replay_item_sort_key<'a>(
    item: &'a NewsReplayItem,
    headline_sources: &BTreeMap<String, NewsSource>,
    headline_sequences: &BTreeMap<String, u64>,
) -> (u64, &'static str, u64, u8, &'a str) {
    match item {
        NewsReplayItem::Headline(headline) => (
            headline.event_time_ns,
            headline.source.as_str(),
            headline.sequence_number,
            0,
            &headline.news_id,
        ),
        NewsReplayItem::Sentiment(sentiment) => (
            sentiment.event_time_ns,
            headline_sources
                .get(&sentiment.causation_news_id)
                .expect("accepted sentiment has a causation headline")
                .as_str(),
            *headline_sequences
                .get(&sentiment.causation_news_id)
                .expect("accepted sentiment has a causation headline"),
            1,
            &sentiment.event_id,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headline_validation() {
        let headline = NewsHeadline {
            news_id: "news.dj.20260901.001".to_owned(),
            source: NewsSource::DowJones,
            headline: "Apple Reports Record Q3 Earnings Beat".to_owned(),
            raw_body_hash: "a".repeat(64),
            sequence_number: 42,
            event_time_ns: 1_700_000_000_000_000_000,
            receive_time_ns: 1_700_000_000_000_050_000,
            entity_tickers: vec!["aapl.us".to_owned()],
        };
        assert!(headline.validate().is_ok());
    }

    #[test]
    fn test_sentiment_vector_bounds() {
        let valid_vector = SentimentVector {
            event_id: "sent.001".to_owned(),
            causation_news_id: "news.dj.20260901.001".to_owned(),
            event_time_ns: 1_700_000_000_000_000_000,
            instrument_id: "aapl.us".to_owned(),
            taxonomy: EventTaxonomy::EarningsRelease,
            sentiment_polarity_bps: 8500, // +0.8500
            confidence_bps: 9000,         // 90.00%
            novelty_score_bps: 10000,     // 100.00%
            surprise_magnitude_bps: 250,
        };
        assert!(valid_vector.validate().is_ok());
        assert_eq!(valid_vector.signal_power_bps(), 7650);
        let negative_fraction = SentimentVector {
            event_id: "sent.negative.001".to_owned(),
            sentiment_polarity_bps: -1,
            confidence_bps: 1,
            novelty_score_bps: 1,
            ..valid_vector.clone()
        };
        assert_eq!(negative_fraction.signal_power_bps(), 0);

        let invalid_polarity = SentimentVector {
            sentiment_polarity_bps: 12000, // Invalid > 10000
            ..valid_vector.clone()
        };
        assert!(invalid_polarity.validate().is_err());
    }

    #[test]
    fn test_replay_news_feed_sorting() {
        let mut feed = ReplayNewsFeed::new();
        let headline_one = NewsHeadline {
            news_id: "news.001".to_owned(),
            source: NewsSource::DowJones,
            headline: "Apple earnings beat".to_owned(),
            raw_body_hash: "a".repeat(64),
            sequence_number: 1,
            event_time_ns: 1_000,
            receive_time_ns: 1_000,
            entity_tickers: vec!["tsla.us".to_owned()],
        };
        let headline_two = NewsHeadline {
            news_id: "news.002".to_owned(),
            source: NewsSource::DowJones,
            headline: "Apple earnings beat".to_owned(),
            raw_body_hash: "b".repeat(64),
            sequence_number: 2,
            event_time_ns: 2_000,
            receive_time_ns: 2_000,
            entity_tickers: vec!["aapl.us".to_owned()],
        };
        let s2 = SentimentVector {
            event_id: "sent.002".to_owned(),
            causation_news_id: "news.002".to_owned(),
            event_time_ns: 2000,
            instrument_id: "aapl.us".to_owned(),
            taxonomy: EventTaxonomy::EarningsRelease,
            sentiment_polarity_bps: 5000,
            confidence_bps: 9000,
            novelty_score_bps: 10000,
            surprise_magnitude_bps: 100,
        };
        let s1 = SentimentVector {
            event_id: "sent.001".to_owned(),
            causation_news_id: "news.001".to_owned(),
            event_time_ns: 1000,
            instrument_id: "tsla.us".to_owned(),
            taxonomy: EventTaxonomy::MacroCpi,
            sentiment_polarity_bps: -3000,
            confidence_bps: 9000,
            novelty_score_bps: 10000,
            surprise_magnitude_bps: -50,
        };

        // The accepted causation parent may arrive first; the replay feed itself
        // retains chronological order regardless of insertion order.
        feed.push(NewsReplayItem::Headline(headline_two))
            .expect("push");
        feed.push(NewsReplayItem::Headline(headline_one))
            .expect("push");
        feed.push(NewsReplayItem::Sentiment(s2)).expect("push");
        feed.push(NewsReplayItem::Sentiment(s1)).expect("push");

        // Verify feed sorts chronologically: s1 (1000) before s2 (2000)
        let events = feed.events();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].event_time_ns(), 1000);
        assert_eq!(events[2].event_time_ns(), 2000);

        // Verify instrument filtering
        let aapl_events = feed.sentiment_events_for("aapl.us");
        assert_eq!(aapl_events.len(), 1);
        assert_eq!(aapl_events[0].event_id, "sent.002");
    }

    #[test]
    fn local_fixture_rejects_malformed_duplicate_and_unknown_input() {
        let valid = r#"{"entity_tickers":["aapl.us"],"event_time_ns":1788260400000000000,"headline":"Apple reports earnings beat","news_id":"news.fixture.001","raw_body_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","receive_time_ns":1788260400000000001,"sequence_number":1,"source":"DOW_JONES"}"#;
        let parsed = ingest_local_headlines_ndjson(valid).expect("valid fixture");
        assert_eq!(parsed.len(), 1);
        assert!(ingest_local_headlines_ndjson("{not-json}").is_err());
        assert!(ingest_local_headlines_ndjson(&format!("{valid}\n{valid}")).is_ok());
        let feed = ReplayNewsFeed::from_headlines(
            ingest_local_headlines_ndjson(&format!("{valid}\n{valid}")).unwrap(),
            &NlpSentimentEngine::new(),
        );
        assert!(feed.is_err());
        assert!(ingest_local_headlines_ndjson(
            r#"{"entity_tickers":[],"event_time_ns":1,"headline":"x","news_id":"news.fixture.001","raw_body_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","receive_time_ns":1,"sequence_number":1,"source":"DOW_JONES","unexpected":true}"#
        )
        .is_err());
    }

    #[test]
    fn fixture_replay_order_is_independent_of_input_order() {
        let early = NewsHeadline {
            news_id: "news.fixture.early".to_owned(),
            source: NewsSource::DowJones,
            headline: "Apple reports earnings beat".to_owned(),
            raw_body_hash: "c".repeat(64),
            sequence_number: 1,
            event_time_ns: 1_788_260_400_000_000_000,
            receive_time_ns: 1_788_260_400_000_000_001,
            entity_tickers: vec!["aapl.us".to_owned()],
        };
        let late = NewsHeadline {
            news_id: "news.fixture.late".to_owned(),
            source: NewsSource::DowJones,
            headline: "Tesla earnings miss".to_owned(),
            raw_body_hash: "d".repeat(64),
            sequence_number: 2,
            event_time_ns: 1_788_260_401_000_000_000,
            receive_time_ns: 1_788_260_401_000_000_001,
            entity_tickers: vec!["tsla.us".to_owned()],
        };
        let classifier = NlpSentimentEngine::new();
        let first = ReplayNewsFeed::from_headlines(vec![late.clone(), early.clone()], &classifier)
            .expect("first replay");
        let second =
            ReplayNewsFeed::from_headlines(vec![early, late], &classifier).expect("second replay");
        assert_eq!(first, second);
        assert_eq!(
            replay_time_from_unix_ns(1_788_260_400_000_000_000).unwrap(),
            "2026-09-01T11:00:00Z"
        );
    }
}
