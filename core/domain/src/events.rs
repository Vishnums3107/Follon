//! News/event payloads and the canonical event envelope.

use std::fmt;

use crate::*;

/// Declared origin of a normalized news headline.
///
/// The replay/local-fixture slice recognizes these stable labels only. They
/// describe fixture provenance and do not imply that a vendor transport is
/// configured or connected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NewsSource {
    /// Dow Jones-compatible fixture provenance.
    DowJones,
    /// Bloomberg B-PIPE-compatible fixture provenance.
    BloombergBPipe,
    /// Refinitiv MRN-compatible fixture provenance.
    RefinitivMrn,
    /// SEC EDGAR-compatible fixture provenance.
    SecEdgar,
    /// Federal Reserve or BLS-compatible fixture provenance.
    FedBls,
}

impl NewsSource {
    /// Stable contract representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DowJones => "DOW_JONES",
            Self::BloombergBPipe => "BLOOMBERG_BPIPE",
            Self::RefinitivMrn => "REFINITIV_MRN",
            Self::SecEdgar => "SEC_EDGAR",
            Self::FedBls => "FED_BLS",
        }
    }

    /// Parses the stable contract representation.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "DOW_JONES" => Ok(Self::DowJones),
            "BLOOMBERG_BPIPE" => Ok(Self::BloombergBPipe),
            "REFINITIV_MRN" => Ok(Self::RefinitivMrn),
            "SEC_EDGAR" => Ok(Self::SecEdgar),
            "FED_BLS" => Ok(Self::FedBls),
            _ => Err(DomainError("invalid news source".to_owned())),
        }
    }
}

/// A deterministic taxonomy assigned by a declared local classifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventTaxonomy {
    /// Quarterly or annual earnings announcement.
    EarningsRelease,
    /// Executive guidance revision.
    GuidanceRevision,
    /// Merger, acquisition, or buyout.
    MergerAcquisition,
    /// Regulatory or FDA trial decision.
    FdaDecision,
    /// Consumer Price Index release.
    MacroCpi,
    /// Federal Reserve interest-rate decision.
    MacroFedRate,
    /// Corporate litigation or settlement.
    Litigation,
}

impl EventTaxonomy {
    /// Stable contract representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EarningsRelease => "EARNINGS_RELEASE",
            Self::GuidanceRevision => "GUIDANCE_REVISION",
            Self::MergerAcquisition => "M_AND_A",
            Self::FdaDecision => "FDA_DECISION",
            Self::MacroCpi => "MACRO_CPI",
            Self::MacroFedRate => "MACRO_FED_RATE",
            Self::Litigation => "LITIGATION",
        }
    }

    /// Parses the stable contract representation.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "EARNINGS_RELEASE" => Ok(Self::EarningsRelease),
            "GUIDANCE_REVISION" => Ok(Self::GuidanceRevision),
            "M_AND_A" => Ok(Self::MergerAcquisition),
            "FDA_DECISION" => Ok(Self::FdaDecision),
            "MACRO_CPI" => Ok(Self::MacroCpi),
            "MACRO_FED_RATE" => Ok(Self::MacroFedRate),
            "LITIGATION" => Ok(Self::Litigation),
            _ => Err(DomainError("invalid news taxonomy".to_owned())),
        }
    }
}

/// Schema-validated payload of a `news.headline.v1` evidence event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewsHeadline {
    /// Source-native, canonical headline identity.
    pub news_id: String,
    /// Declared fixture/local-file source.
    pub source: NewsSource,
    /// Immutable headline text.
    pub headline: String,
    /// Lowercase SHA-256 hash of the source body retained outside this payload.
    pub raw_body_hash: String,
    /// Source-provided sequence number.
    pub sequence_number: u64,
    /// Source publication time in UTC Unix nanoseconds.
    pub event_time_ns: u64,
    /// Local fixture-ingress time in UTC Unix nanoseconds.
    pub receive_time_ns: u64,
    /// Canonical instruments explicitly associated with the item.
    pub entity_tickers: Vec<String>,
}

impl NewsHeadline {
    /// Validates the versioned payload contract.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_canonical_id("news_id", &self.news_id)?;
        if self.headline.trim().is_empty()
            || self.raw_body_hash.len() != 64
            || !self
                .raw_body_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || self.event_time_ns == 0
            || self.receive_time_ns == 0
        {
            return Err(DomainError("invalid news headline payload".to_owned()));
        }
        for instrument_id in &self.entity_tickers {
            validate_canonical_id("news entity_ticker", instrument_id)?;
        }
        Ok(())
    }
}

/// Schema-validated payload of a `news.sentiment.v1` evidence event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SentimentVector {
    /// Deterministic local-classifier output identity.
    pub event_id: String,
    /// Source `NewsHeadline.news_id` that caused this vector.
    pub causation_news_id: String,
    /// Source publication time inherited from its headline, in UTC Unix nanoseconds.
    pub event_time_ns: u64,
    /// Canonical target instrument.
    pub instrument_id: String,
    /// Declared deterministic classification.
    pub taxonomy: EventTaxonomy,
    /// Directional integer basis points in the inclusive range -10000..=10000.
    pub sentiment_polarity_bps: i32,
    /// Integer confidence basis points in the inclusive range 0..=10000.
    pub confidence_bps: u32,
    /// Integer novelty basis points in the inclusive range 0..=10000.
    pub novelty_score_bps: u32,
    /// Extracted numeric surprise, expressed in integer basis points.
    pub surprise_magnitude_bps: i32,
}

impl SentimentVector {
    /// Validates the versioned payload contract.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_canonical_id("sentiment event_id", &self.event_id)?;
        validate_canonical_id("causation_news_id", &self.causation_news_id)?;
        validate_canonical_id("instrument_id", &self.instrument_id)?;
        if self.event_time_ns == 0
            || !(-10_000..=10_000).contains(&self.sentiment_polarity_bps)
            || self.confidence_bps > 10_000
            || self.novelty_score_bps > 10_000
        {
            return Err(DomainError("invalid news sentiment payload".to_owned()));
        }
        Ok(())
    }

    /// Computes signal power using integer basis-point arithmetic only.
    pub fn signal_power_bps(&self) -> i64 {
        (i64::from(self.sentiment_polarity_bps)
            * i64::from(self.confidence_bps)
            * i64::from(self.novelty_score_bps))
            / 100_000_000
    }
}

/// First-slice event families supported by the stable envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventPayload {
    /// A normalized historical bar.
    MarketBar(Bar),
    /// A canonical trading request.
    OrderIntent(OrderIntent),
    /// A risk approval or rejection.
    RiskDecision(RiskDecision),
    /// An OMS lifecycle transition.
    OrderState(OrderStateChange),
    /// A simulator execution.
    Fill(Fill),
    /// A position projection.
    Position(PositionSnapshot),
    /// A P&L projection.
    Pnl(PnlSnapshot),
    /// An immutable audit trail summary.
    Audit(AuditTrail),
    /// A normalized fixture/local-file news headline.
    NewsHeadline(NewsHeadline),
    /// A deterministic local sentiment vector.
    NewsSentiment(SentimentVector),
}

impl EventPayload {
    /// Namespaced event type with an explicit compatibility version.
    pub const fn event_type(&self) -> &'static str {
        match self {
            Self::MarketBar(_) => "market.bar.v1",
            Self::OrderIntent(_) => "intent.created.v1",
            Self::RiskDecision(_) => "risk.decision.v1",
            Self::OrderState(_) => "order.state_changed.v1",
            Self::Fill(_) => "execution.fill.v1",
            Self::Position(_) => "portfolio.position_updated.v1",
            Self::Pnl(_) => "portfolio.pnl_updated.v1",
            Self::Audit(_) => "audit.trail.v1",
            Self::NewsHeadline(_) => "news.headline.v1",
            Self::NewsSentiment(_) => "news.sentiment.v1",
        }
    }

    fn canonical_json(&self) -> String {
        match self {
            Self::MarketBar(bar) => format!(
                "{{\"close\":\"{}\",\"exchange_timezone\":{},\"high\":\"{}\",\"instrument_id\":{},\"interval_seconds\":{},\"low\":\"{}\",\"open\":\"{}\",\"volume\":\"{}\"}}",
                bar.close, json_string(&bar.exchange_timezone), bar.high, json_string(&bar.instrument_id), bar.interval_seconds, bar.low, bar.open, bar.volume
            ),
            Self::OrderIntent(intent) => format!(
                "{{\"account_id\":{},\"configuration_version\":{},\"correlation_id\":{},\"created_at\":{},\"environment\":{},\"instrument_id\":{},\"intent_id\":{},\"limit_price\":{},\"order_type\":{},\"quantity\":\"{}\",\"rationale\":{},\"side\":{},\"strategy_id\":{},\"strategy_version\":{},\"time_in_force\":{}}}",
                json_string(&intent.account_id), json_string(&intent.configuration_version), json_string(&intent.correlation_id), json_string(&intent.created_at), json_string(&intent.environment), json_string(&intent.instrument_id), json_string(&intent.intent_id), option_decimal_json(intent.limit_price), json_string(intent.order_type.as_str()), intent.quantity, json_string(&intent.rationale), json_string(intent.side.as_str()), json_string(&intent.strategy_id), json_string(&intent.strategy_version), json_string(intent.time_in_force.as_str())
            ),
            Self::RiskDecision(decision) => format!(
                "{{\"actor\":{},\"approved\":{},\"correlation_id\":{},\"decided_at\":{},\"decision_id\":{},\"evaluated_limits\":{},\"intent_id\":{},\"policy_version\":{},\"reason_codes\":{},\"source\":\"risk\"}}",
                json_string(&decision.actor), decision.approved, json_string(&decision.correlation_id), json_string(&decision.decided_at), json_string(&decision.decision_id), json_string(&decision.evaluated_limits), json_string(&decision.intent_id), json_string(&decision.policy_version), json_strings(&decision.reason_codes)
            ),
            Self::OrderState(change) => format!(
                "{{\"new_state\":{},\"order_id\":{},\"previous_state\":{},\"reason\":{}}}",
                json_string(change.new_state.as_str()), json_string(&change.order_id), change.previous_state.map(|state| json_string(state.as_str())).unwrap_or_else(|| "null".to_owned()), json_string(&change.reason)
            ),
            Self::Fill(fill) => format!(
                "{{\"executed_at\":{},\"execution_id\":{},\"fee\":\"{}\",\"instrument_id\":{},\"order_id\":{},\"price\":\"{}\",\"quantity\":\"{}\",\"side\":{}}}",
                json_string(&fill.executed_at), json_string(&fill.execution_id), fill.fee, json_string(&fill.instrument_id), json_string(&fill.order_id), fill.price, fill.quantity, json_string(fill.side.as_str())
            ),
            Self::Position(position) => format!(
                "{{\"account_id\":{},\"average_cost\":\"{}\",\"instrument_id\":{},\"quantity\":\"{}\",\"realized_pnl\":\"{}\"}}",
                json_string(&position.account_id), position.average_cost, json_string(&position.instrument_id), position.quantity, position.realized_pnl
            ),
            Self::Pnl(pnl) => format!(
                "{{\"account_id\":{},\"instrument_id\":{},\"mark_price\":\"{}\",\"realized_pnl\":\"{}\",\"total_pnl\":\"{}\",\"unrealized_pnl\":\"{}\"}}",
                json_string(&pnl.account_id), json_string(&pnl.instrument_id), pnl.mark_price, pnl.realized_pnl, pnl.total_pnl, pnl.unrealized_pnl
            ),
            Self::Audit(audit) => format!(
                "{{\"correlation_id\":{},\"event_ids\":{},\"summary\":{}}}",
                json_string(&audit.correlation_id), json_strings(&audit.event_ids), json_string(&audit.summary)
            ),
            Self::NewsHeadline(headline) => format!(
                "{{\"entity_tickers\":{},\"event_time_ns\":{},\"headline\":{},\"news_id\":{},\"raw_body_hash\":{},\"receive_time_ns\":{},\"sequence_number\":{},\"source\":{}}}",
                json_strings(&headline.entity_tickers), headline.event_time_ns, json_string(&headline.headline),
                json_string(&headline.news_id), json_string(&headline.raw_body_hash), headline.receive_time_ns,
                headline.sequence_number, json_string(headline.source.as_str())
            ),
            Self::NewsSentiment(sentiment) => format!(
                "{{\"causation_news_id\":{},\"confidence_bps\":{},\"event_id\":{},\"event_time_ns\":{},\"instrument_id\":{},\"novelty_score_bps\":{},\"sentiment_polarity_bps\":{},\"surprise_magnitude_bps\":{},\"taxonomy\":{}}}",
                json_string(&sentiment.causation_news_id), sentiment.confidence_bps, json_string(&sentiment.event_id),
                sentiment.event_time_ns, json_string(&sentiment.instrument_id), sentiment.novelty_score_bps,
                sentiment.sentiment_polarity_bps, sentiment.surprise_magnitude_bps,
                json_string(sentiment.taxonomy.as_str())
            ),
        }
    }
}

/// Immutable, append-only envelope around every significant trading event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventEnvelope {
    /// Globally unique immutable event identity.
    pub event_id: String,
    /// Namespaced semantic event name.
    pub event_type: String,
    /// Payload schema version.
    pub schema_version: u32,
    /// Source or logical event time in UTC.
    pub event_time: String,
    /// Local receipt or generation time in UTC.
    pub receive_time: String,
    /// Account when applicable; absence is explicit.
    pub account_id: Option<String>,
    /// Strategy when applicable; absence is explicit.
    pub strategy_id: Option<String>,
    /// Instrument when applicable; absence is explicit.
    pub instrument_id: Option<String>,
    /// Causal workflow identity.
    pub correlation_id: String,
    /// Direct cause event identity, if any.
    pub causation_id: Option<String>,
    /// Actor responsible for this event.
    pub actor: String,
    /// Source subsystem or provider.
    pub source: String,
    /// Validated event-specific payload.
    pub payload: EventPayload,
    /// Immutable engine build version.
    pub software_version: String,
    /// Immutable configuration version.
    pub configuration_version: String,
}

impl EventEnvelope {
    /// Validates stable envelope fields and payload compatibility at ingress.
    pub fn validate(&self) -> Result<(), DomainError> {
        validate_canonical_id("event_id", &self.event_id)?;
        validate_canonical_id("correlation_id", &self.correlation_id)?;
        validate_utc_timestamp("event_time", &self.event_time)?;
        validate_utc_timestamp("receive_time", &self.receive_time)?;
        if self.event_type != self.payload.event_type()
            || self.schema_version == 0
            || self.event_time.is_empty()
            || self.receive_time.is_empty()
            || self.actor.is_empty()
            || self.source.is_empty()
            || self.software_version.is_empty()
            || self.configuration_version.is_empty()
        {
            return Err(DomainError(
                "event envelope is missing required or compatible fields".to_owned(),
            ));
        }
        for (name, value) in [
            ("account_id", self.account_id.as_deref()),
            ("strategy_id", self.strategy_id.as_deref()),
            ("instrument_id", self.instrument_id.as_deref()),
        ] {
            if let Some(value) = value {
                validate_canonical_id(name, value)?;
            }
        }
        match &self.payload {
            EventPayload::NewsHeadline(headline) => headline.validate()?,
            EventPayload::NewsSentiment(sentiment) => sentiment.validate()?,
            _ => {}
        }
        Ok(())
    }

    /// Produces stable JSON for persistence, replay comparisons, and tests.
    pub fn canonical_json(&self) -> String {
        format!(
            "{{\"account_id\":{},\"actor\":{},\"causation_id\":{},\"configuration_version\":{},\"correlation_id\":{},\"event_id\":{},\"event_time\":{},\"event_type\":{},\"instrument_id\":{},\"payload\":{},\"receive_time\":{},\"schema_version\":{},\"software_version\":{},\"source\":{},\"strategy_id\":{}}}",
            option_string_json(&self.account_id), json_string(&self.actor), option_string_json(&self.causation_id), json_string(&self.configuration_version), json_string(&self.correlation_id), json_string(&self.event_id), json_string(&self.event_time), json_string(&self.event_type), option_string_json(&self.instrument_id), self.payload.canonical_json(), json_string(&self.receive_time), self.schema_version, json_string(&self.software_version), json_string(&self.source), option_string_json(&self.strategy_id)
        )
    }
}

fn json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                use fmt::Write;
                write!(&mut escaped, "\\u{:04x}", character as u32)
                    .expect("string formatting cannot fail");
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

fn option_string_json(value: &Option<String>) -> String {
    value
        .as_deref()
        .map(json_string)
        .unwrap_or_else(|| "null".to_owned())
}

fn option_decimal_json(value: Option<Decimal>) -> String {
    value
        .map(|decimal| format!("\"{decimal}\""))
        .unwrap_or_else(|| "null".to_owned())
}

fn json_strings(values: &[String]) -> String {
    let mut json = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str(&json_string(value));
    }
    json.push(']');
    json
}
