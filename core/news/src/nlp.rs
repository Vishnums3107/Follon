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
            if text_lower.contains(keyword) && !resolved.contains(&instrument.to_string()) {
                resolved.push(instrument.to_string());
            }
        }

        resolved
    }

    /// Categorizes headline text into an [`EventTaxonomy`].
    fn classify_taxonomy(&self, text_lower: &str) -> EventTaxonomy {
        if text_lower.contains("cpi") || text_lower.contains("inflation") {
            EventTaxonomy::MacroCpi
        } else if text_lower.contains("fed")
            || text_lower.contains("fomc")
            || text_lower.contains("rate cut")
            || text_lower.contains("rate hike")
        {
            EventTaxonomy::MacroFedRate
        } else if text_lower.contains("earnings")
            || text_lower.contains("eps")
            || text_lower.contains("q1")
            || text_lower.contains("q2")
            || text_lower.contains("q3")
            || text_lower.contains("q4")
        {
            EventTaxonomy::EarningsRelease
        } else if text_lower.contains("acquire")
            || text_lower.contains("merger")
            || text_lower.contains("buyout")
            || text_lower.contains("deal")
        {
            EventTaxonomy::MergerAcquisition
        } else if text_lower.contains("fda")
            || text_lower.contains("trial")
            || text_lower.contains("drug")
        {
            EventTaxonomy::FdaDecision
        } else if text_lower.contains("guidance")
            || text_lower.contains("outlook")
            || text_lower.contains("forecast")
        {
            EventTaxonomy::GuidanceRevision
        } else if text_lower.contains("lawsuit")
            || text_lower.contains("litigation")
            || text_lower.contains("sec investigation")
        {
            EventTaxonomy::Litigation
        } else {
            EventTaxonomy::EarningsRelease
        }
    }

    /// Scores financial sentiment polarity in integer basis points (-10000 to +10000).
    fn calculate_polarity_bps(&self, text_lower: &str) -> i32 {
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
            if text_lower.contains(word) {
                pos_count += 1;
            }
        }
        for word in negative_words {
            if text_lower.contains(word) {
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
        let high_confidence_markers = [
            "reports",
            "quarterly",
            "official",
            "sec",
            "q1",
            "q2",
            "q3",
            "q4",
            "earnings",
            "revenue",
            "cpi",
            "fed",
            "fda",
        ];
        let marker_matches = high_confidence_markers
            .iter()
            .filter(|m| text_lower.contains(**m))
            .count();

        let base_confidence = 7500u32; // 75.00%
        let boost = (marker_matches as u32) * 500;
        (base_confidence + boost).min(9800)
    }

    /// Extracts numerical surprise deltas in basis points.
    fn extract_surprise_bps(&self, text_lower: &str) -> i32 {
        if text_lower.contains("beat") || text_lower.contains("record") {
            250 // +2.50% default surprise delta
        } else if text_lower.contains("miss") || text_lower.contains("cut") {
            -250 // -2.50% default surprise delta
        } else {
            0
        }
    }
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
}
