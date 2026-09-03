CREATE TABLE IF NOT EXISTS news_headlines (
    news_id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    headline TEXT NOT NULL,
    raw_body_hash TEXT NOT NULL CHECK (length(raw_body_hash) = 64),
    sequence_number BIGINT NOT NULL CHECK (sequence_number >= 0),
    event_time_ns BIGINT NOT NULL CHECK (event_time_ns > 0),
    receive_time_ns BIGINT NOT NULL CHECK (receive_time_ns > 0),
    entity_tickers JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (news_id ~ '^[a-z0-9._-]+$')
);

CREATE INDEX IF NOT EXISTS news_headlines_event_time_idx
    ON news_headlines (event_time_ns DESC);

CREATE INDEX IF NOT EXISTS news_headlines_source_idx
    ON news_headlines (source, event_time_ns DESC);

CREATE TABLE IF NOT EXISTS news_sentiments (
    event_id TEXT PRIMARY KEY,
    causation_news_id TEXT NOT NULL REFERENCES news_headlines(news_id),
    instrument_id TEXT NOT NULL,
    taxonomy TEXT NOT NULL,
    sentiment_polarity_bps INTEGER NOT NULL CHECK (sentiment_polarity_bps BETWEEN -10000 AND 10000),
    confidence_bps INTEGER NOT NULL CHECK (confidence_bps BETWEEN 0 AND 10000),
    novelty_score_bps INTEGER NOT NULL CHECK (novelty_score_bps BETWEEN 0 AND 10000),
    surprise_magnitude_bps INTEGER NOT NULL,
    event_time_ns BIGINT NOT NULL CHECK (event_time_ns > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (event_id ~ '^[a-z0-9._-]+$'),
    CHECK (instrument_id ~ '^[a-z0-9._-]+$')
);

CREATE INDEX IF NOT EXISTS news_sentiments_instrument_time_idx
    ON news_sentiments (instrument_id, event_time_ns DESC);

CREATE INDEX IF NOT EXISTS news_sentiments_taxonomy_idx
    ON news_sentiments (taxonomy, event_time_ns DESC);

CREATE INDEX IF NOT EXISTS news_sentiments_causation_idx
    ON news_sentiments (causation_news_id);
