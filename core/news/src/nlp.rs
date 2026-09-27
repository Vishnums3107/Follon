//! Deterministic local headline classification and entity resolution.
//!
//! The classifier is a deliberately small, versioned keyword baseline for
//! fixtures and replay evidence. It is not a vendor feed, a trained model, or a
//! claim of financial-language coverage or latency.

use crate::{
    validate_headline_availability, EventTaxonomy, NewsError, NewsHeadline, SentimentVector,
};

/// A financial lexicon & entity-resolution NLP sentiment engine.
#[derive(Clone, Debug, Default)]
pub struct NlpSentimentEngine;

impl NlpSentimentEngine {
    /// Immutable identifier for the bounded local classifier.
    pub const MODEL_ID: &'static str = "keyword-finance";
    /// Immutable version for the classifier rules that produced a vector.
    pub const MODEL_VERSION: &'static str = "v1";
    /// Immutable actor stamp carried by derived sentiment evidence envelopes.
    pub const EVIDENCE_ACTOR: &'static str = "news_classifier.keyword-finance-v1";

    /// Creates a new in-memory NLP sentiment engine instance.
    pub fn new() -> Self {
        Self
    }

    /// Returns the stable model identity for replay provenance.
    pub const fn model_id(&self) -> &'static str {
        Self::MODEL_ID
    }

    /// Returns the stable model version for replay provenance.
    pub const fn model_version(&self) -> &'static str {
        Self::MODEL_VERSION
    }

    /// Extracts deterministic [`SentimentVector`] instances from a normalized headline.
    pub fn extract_sentiment_vectors(
        &self,
        headline: &NewsHeadline,
    ) -> Result<Vec<SentimentVector>, NewsError> {
        validate_headline_availability(headline)?;

        let text_lower = headline.headline.to_lowercase();
        let target_instruments = self.resolve_entities(&text_lower, &headline.entity_tickers);
        if target_instruments.is_empty() {
            return Ok(Vec::new());
        }

        let taxonomy = self.classify_taxonomy(&text_lower);
        let polarity_bps = self.calculate_polarity_bps(&text_lower);
        let confidence_bps = self.calculate_confidence_bps(&text_lower, polarity_bps);
        let surprise_bps = self.extract_surprise_bps(&text_lower);

        let mut vectors = Vec::new();
        for (index, instrument_id) in target_instruments.into_iter().enumerate() {
            let event_id = format!("sent.{}.{}", headline.news_id, index + 1);
            let vector = SentimentVector {
                event_id,
                causation_news_id: headline.news_id.clone(),
                event_time_ns: headline.event_time_ns,
                instrument_id,
                taxonomy,
                sentiment_polarity_bps: polarity_bps,
                confidence_bps,
                novelty_score_bps: 10000, // Primary source novelty
                surprise_magnitude_bps: surprise_bps,
            };
            vector.validate()?;
            vectors.push(vector);
        }

        Ok(vectors)
    }

    /// Resolves canonical instrument identifiers from text and explicit tickers.
    fn resolve_entities(&self, text_lower: &str, explicit_tickers: &[String]) -> Vec<String> {
        let mut resolved = Vec::new();
        for ticker in explicit_tickers {
            if !resolved.contains(ticker) {
                resolved.push(ticker.clone());
            }
        }

        let tokens = tokenize_words(text_lower);

        let dictionary = [
            ("apple", "aapl.us"),
            ("aapl", "aapl.us"),
            ("tesla", "tsla.us"),
            ("tsla", "tsla.us"),
            ("nvidia", "nvda.us"),
            ("nvda", "nvda.us"),
            ("microsoft", "msft.us"),
            ("msft", "msft.us"),
            ("amazon", "amzn.us"),
            ("amzn", "amzn.us"),
            ("s&p", "spy.us"),
            ("spy", "spy.us"),
            ("cpi", "spy.us"),
            ("inflation", "spy.us"),
            ("fed", "spy.us"),
            ("fomc", "spy.us"),
        ];

        for (keyword, instrument) in dictionary {
            if contains_keyword(&tokens, keyword) && !resolved.contains(&instrument.to_string()) {
                resolved.push(instrument.to_string());
            }
        }

        resolved
    }

    /// Categorizes headline text into an [`EventTaxonomy`].
    fn classify_taxonomy(&self, text_lower: &str) -> EventTaxonomy {
        let tokens = tokenize_words(text_lower);
        let has = |keyword: &str| contains_keyword(&tokens, keyword);

        if has("cpi") || has("inflation") {
            EventTaxonomy::MacroCpi
        } else if has("fed") || has("fomc") || has("rate cut") || has("rate hike") {
            EventTaxonomy::MacroFedRate
        } else if has("earnings") || has("eps") || has("q1") || has("q2") || has("q3") || has("q4")
        {
            EventTaxonomy::EarningsRelease
        } else if has("acquire")
            || has("acquires")
            || has("acquired")
            || has("acquiring")
            || has("merger")
            || has("mergers")
            || has("buyout")
            || has("buyouts")
            || has("deal")
            || has("deals")
        {
            EventTaxonomy::MergerAcquisition
        } else if has("fda") || has("trial") || has("trials") || has("drug") || has("drugs") {
            EventTaxonomy::FdaDecision
        } else if has("guidance")
            || has("outlook")
            || has("forecast")
            || has("forecasts")
            || has("forecasted")
        {
            EventTaxonomy::GuidanceRevision
        } else if has("lawsuit") || has("lawsuits") || has("litigation") || has("sec investigation")
        {
            EventTaxonomy::Litigation
        } else {
            EventTaxonomy::EarningsRelease
        }
    }

    /// Scores financial sentiment polarity in integer basis points (-10000 to +10000).
    fn calculate_polarity_bps(&self, text_lower: &str) -> i32 {
        let tokens = tokenize_words(text_lower);
        let positive_words = [
            "beat",
            "beats",
            "beating",
            "record",
            "surge",
            "surges",
            "surged",
            "raise",
            "raises",
            "raised",
            "growth",
            "higher",
            "outperform",
            "profit",
            "approval",
            "approved",
            "gain",
            "gains",
            "cools",
            "strong",
            "bullish",
        ];
        let negative_words = [
            "miss", "misses", "missed", "drop", "drops", "dropped", "fall", "falls", "cut", "cuts",
            "lowered", "warning", "warns", "loss", "losses", "lawsuit", "rejected", "decline",
            "declines", "plunge", "plunges", "weak", "bearish",
        ];

        let mut pos_count = 0i32;
        let mut neg_count = 0i32;

        for word in positive_words {
            if contains_keyword(&tokens, word) {
                pos_count += 1;
            }
        }
        for word in negative_words {
            if contains_keyword(&tokens, word) {
                neg_count += 1;
            }
        }

        let total = pos_count + neg_count;
        if total == 0 {
            return 0;
        }

        // Integer arithmetic keeps the result reproducible across platforms and
        // avoids introducing floating-point values into a trading signal path.
        ((pos_count - neg_count) * 9_000 / total).clamp(-10_000, 10_000)
    }

    /// Calculates confidence score in basis points (0 to 10000).
    fn calculate_confidence_bps(&self, text_lower: &str, polarity_bps: i32) -> u32 {
        if polarity_bps == 0 {
            return 5000;
        }
        let tokens = tokenize_words(text_lower);
        let high_confidence_markers = [
            "reports",
            "quarterly",
            "official",
            "officials",
            "sec",
            "q1",
            "q2",
            "q3",
            "q4",
            "earnings",
            "revenue",
            "revenues",
            "cpi",
            "fed",
            "fda",
        ];
        let marker_matches = high_confidence_markers
            .iter()
            .filter(|m| contains_keyword(&tokens, m))
            .count();

        let base_confidence = 7500u32; // 75.00%
        let boost = (marker_matches as u32) * 500;
        (base_confidence + boost).min(9800)
    }

    /// Extracts numerical surprise deltas in basis points.
    fn extract_surprise_bps(&self, text_lower: &str) -> i32 {
        let tokens = tokenize_words(text_lower);
        let has = |keyword: &str| contains_keyword(&tokens, keyword);
        if has("beat") || has("beats") || has("beating") || has("record") {
            250 // +2.50% default surprise delta
        } else if has("miss") || has("misses") || has("missed") || has("cut") || has("cuts") {
            -250 // -2.50% default surprise delta
        } else {
            0
        }
    }
}

/// Splits lowercase text into alphanumeric word tokens on any non-alphanumeric
/// boundary (whitespace, punctuation, symbols). This is the basis for
/// whole-word keyword matching: raw substring `.contains()` checks are prone
/// to false positives on short keywords (e.g. `"fed"` inside `"federal"` or
/// `"fedex"`, or `"gain"` inside `"against"`), which whole-word tokenization
/// eliminates.
fn tokenize_words(text_lower: &str) -> Vec<&str> {
    text_lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect()
}

/// Returns true when `keyword` appears in `tokens` as a contiguous run of
/// whole-word matches. `keyword` is itself tokenized the same way as the
/// source text, so:
///   - a single-word keyword (the common case, e.g. `"fed"`) reduces to an
///     exact-token equality check rather than a substring scan;
///   - a multi-word phrase keyword (e.g. `"rate cut"`, `"sec investigation"`)
///     still requires its words to appear adjacently and in order, so
///     legitimate phrase-level matches are preserved rather than broken by
///     over-tokenizing;
///   - a keyword containing punctuation (e.g. `"s&p"`) tokenizes to the same
///     word sequence (`["s", "p"]`) as the equivalent text, so it still
///     matches "S&P", "S & P", etc. as adjacent whole words.
fn contains_keyword(tokens: &[&str], keyword: &str) -> bool {
    let keyword_tokens = tokenize_words(keyword);
    if keyword_tokens.is_empty() || keyword_tokens.len() > tokens.len() {
        return false;
    }
    tokens
        .windows(keyword_tokens.len())
        .any(|window| window == keyword_tokens.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewsSource;

    #[test]
    fn test_nlp_extracts_earnings_beat() {
        let engine = NlpSentimentEngine::new();
        assert_eq!(engine.model_id(), "keyword-finance");
        assert_eq!(engine.model_version(), "v1");
        let headline = NewsHeadline {
            news_id: "news.001".to_owned(),
            source: NewsSource::DowJones,
            headline: "Apple Reports Record Q3 Earnings Beat & Raises Forecast".to_owned(),
            raw_body_hash: "a".repeat(64),
            sequence_number: 1,
            event_time_ns: 1000,
            receive_time_ns: 1050,
            entity_tickers: vec!["aapl.us".to_owned()],
        };

        let vectors = engine.extract_sentiment_vectors(&headline).expect("nlp");
        assert_eq!(vectors.len(), 1);
        let vec = &vectors[0];
        assert_eq!(vec.instrument_id, "aapl.us");
        assert_eq!(vec.taxonomy, EventTaxonomy::EarningsRelease);
        assert!(vec.sentiment_polarity_bps > 5000);
        assert!(vec.confidence_bps >= 8500);
        assert_eq!(vec.surprise_magnitude_bps, 250);
    }

    #[test]
    fn test_nlp_extracts_macro_cpi_cools() {
        let engine = NlpSentimentEngine::new();
        let headline = NewsHeadline {
            news_id: "news.002".to_owned(),
            source: NewsSource::FedBls,
            headline: "US CPI Inflation Cools to 2.4% YoY Growth".to_owned(),
            raw_body_hash: "b".repeat(64),
            sequence_number: 2,
            event_time_ns: 2000,
            receive_time_ns: 2050,
            entity_tickers: Vec::new(),
        };

        let vectors = engine.extract_sentiment_vectors(&headline).expect("nlp");
        assert_eq!(vectors.len(), 1);
        let vec = &vectors[0];
        assert_eq!(vec.instrument_id, "spy.us");
        assert_eq!(vec.taxonomy, EventTaxonomy::MacroCpi);
        assert!(vec.sentiment_polarity_bps > 0);
    }

    /// Regression test for the `"fed"` substring-collision bug: "FedEx" must
    /// not be treated as containing the macro-policy keyword `"fed"`. Before
    /// the word-boundary fix this headline both spuriously resolved a
    /// `spy.us` entity vector and was misclassified as `MacroFedRate` instead
    /// of `EarningsRelease`.
    #[test]
    fn test_nlp_fedex_headline_is_not_confused_with_fed_policy() {
        let engine = NlpSentimentEngine::new();
        let headline = NewsHeadline {
            news_id: "news.003".to_owned(),
            source: NewsSource::DowJones,
            headline: "FedEx Reports Record Q3 Earnings Beat & Raises Forecast".to_owned(),
            raw_body_hash: "c".repeat(64),
            sequence_number: 3,
            event_time_ns: 3000,
            receive_time_ns: 3050,
            entity_tickers: vec!["fdx.us".to_owned()],
        };

        let vectors = engine.extract_sentiment_vectors(&headline).expect("nlp");
        // Only the explicitly supplied FedEx ticker resolves; "FedEx" must not
        // spuriously trigger the "fed" -> spy.us macro-policy entity mapping.
        assert_eq!(vectors.len(), 1);
        assert!(vectors.iter().all(|v| v.instrument_id != "spy.us"));
        let vec = &vectors[0];
        assert_eq!(vec.instrument_id, "fdx.us");
        assert_eq!(vec.taxonomy, EventTaxonomy::EarningsRelease);
    }

    /// Regression test for the `"disapproval"` substring-collision bug:
    /// `"disapproval"` must not be treated as containing the positive
    /// keyword `"approval"`. With no other polarity keyword present, the
    /// headline should score neutral rather than spuriously positive.
    #[test]
    fn test_nlp_disapproval_does_not_match_positive_approval_keyword() {
        let engine = NlpSentimentEngine::new();
        let headline = NewsHeadline {
            news_id: "news.004".to_owned(),
            source: NewsSource::DowJones,
            headline: "Regulators Voice Disapproval of the Merger".to_owned(),
            raw_body_hash: "d".repeat(64),
            sequence_number: 4,
            event_time_ns: 4000,
            receive_time_ns: 4050,
            entity_tickers: vec!["aapl.us".to_owned()],
        };

        let vectors = engine.extract_sentiment_vectors(&headline).expect("nlp");
        assert_eq!(vectors.len(), 1);
        assert_eq!(vectors[0].sentiment_polarity_bps, 0);
    }

    /// Regression test for the `"against"` substring-collision bug:
    /// `"against"` must not be treated as containing the positive keyword
    /// `"gain"`. The only real polarity keyword in this headline is the
    /// negative `"lawsuit"`, so the score must be negative rather than
    /// spuriously canceled out to neutral.
    #[test]
    fn test_nlp_against_does_not_match_positive_gain_keyword() {
        let engine = NlpSentimentEngine::new();
        let headline = NewsHeadline {
            news_id: "news.005".to_owned(),
            source: NewsSource::DowJones,
            headline: "Company Files Lawsuit Against Regulator".to_owned(),
            raw_body_hash: "e".repeat(64),
            sequence_number: 5,
            event_time_ns: 5000,
            receive_time_ns: 5050,
            entity_tickers: vec!["aapl.us".to_owned()],
        };

        let vectors = engine.extract_sentiment_vectors(&headline).expect("nlp");
        assert_eq!(vectors.len(), 1);
        assert_eq!(vectors[0].sentiment_polarity_bps, -9000);
    }
}
