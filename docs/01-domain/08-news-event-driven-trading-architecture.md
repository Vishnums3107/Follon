# News replay-to-paper slice

## Scope and boundary

This implementation is a deterministic **local-fixture/replay-to-paper**
vertical slice. It does not connect to Dow Jones, Bloomberg, Refinitiv, SEC,
Federal Reserve, BLS, a broker, or any live trading venue. Source names in a
fixture are provenance labels defined by the versioned contract; they are not
claims that a provider integration is available.

The local classifier is a small deterministic keyword baseline. It produces
integer basis-point values for replay evidence; it is not a Loughran-McDonald
implementation, a trained NLP model, a recommendation, or a latency claim.

## Implemented sequence

```text
validated local NDJSON headline fixture
  -> news.headline.v1 evidence envelope
  -> deterministic local sentiment vector
  -> news.sentiment.v1 evidence envelope
  -> isolated Python on_news_headline / on_news_sentiment callback
  -> declarative order intent
  -> news-aware risk decision
  -> PAPER/simulation OMS and deterministic fill
  -> portfolio / audit evidence
  -> read-only desktop projection
```

Strategies cannot access an adapter, credential, order transport, or the
desktop. A headline callback cannot create an intent; only a sentiment callback
may return one. Every returned intent is validated by the control plane and
must pass the risk decision before OMS can create an order.

## Versioned payload contracts

`contracts/json-schema/v1/news-headline.schema.json` describes the payload of
`news.headline.v1`. It includes a canonical `news_id`, declared `source`,
headline text, lowercase SHA-256 body hash, provider sequence, source/receive
Unix-nanosecond timestamps, and canonical entity instrument IDs.

`contracts/json-schema/v1/news-sentiment.schema.json` describes the payload of
`news.sentiment.v1`: a deterministic output `event_id`, causative `news_id`,
target instrument, taxonomy, integer polarity/confidence/novelty/surprise
scores, and source event time. The immutable Follon envelope adds the evidence
event ID and stores the direct envelope causation link from sentiment to
headline, intent to sentiment, risk decision to intent, and subsequent OMS,
fill, portfolio, and audit events.

The local keyword classifier identifies itself as `keyword-finance` version
`v1`. Derived sentiment envelopes carry the immutable actor stamp
`news_classifier.keyword-finance-v1`; the payload's source event time and the
envelope causation ID retain the source evidence link. This is bounded local
fixture provenance, not a claim that arbitrary external model output has been
verified.

The Protobuf messages define the same payload values for a future API surface.
They are not served by the current gRPC service, so there is no advertised news
RPC endpoint.

## Local ingress and ordering

`follon_news::ingest_local_headlines_ndjson` accepts only one schema-valid
payload per non-empty line and rejects malformed JSON, unknown fields, invalid
IDs, invalid lowercase hashes, invalid timestamps, and a receipt timestamp
earlier than the source event. `ReplayNewsFeed` rejects duplicate
headlines/vectors, orphaned sentiment, and a sentiment whose declared source
time differs from its causal headline. Its total ordering is:

1. availability time: headline receipt time, inherited by its causal sentiment;
2. declared source label;
3. source sequence number;
4. event kind (headline before its sentiment vector);
5. source event time;
6. canonical identity.

Source timestamps remain in the payload and envelope event-time evidence. The
replay clock uses the availability timestamp without a local timezone. Because
the envelope contract is second precision, a sub-second availability is rounded
up to the next canonical second before a strategy callback; it is
never truncated to an earlier instant. Nanosecond ordering remains in the feed
key, while the ceiling rule prevents a strategy from consuming a headline or
its derived sentiment before it was available.

## News shock risk inputs

For a news-driven intent, the replay supplies immutable `NewsShockContext`
facts: a pre-headline reference price and optional current/baseline full
spreads. `RiskPolicy` can enforce optional fixed-point/integer-BPS limits for:

- movement from the pre-headline reference (`NEWS_SLIPPAGE_EXCEEDED`); and
- current/baseline spread multiplication (`LIQUIDITY_HOLE_DETECTED`).

The risks are evaluated with the ordinary quantity, notional, price-collar, and
kill-switch policy. A rejection emits risk and audit evidence but never creates
an OMS order or simulator fill.

## Evidence desktop

The News Cockpit filters actual `WorkspaceSnapshot.events` data. It joins stored
sentiment payloads to their stored headlines, calculates displayed signal power
from integer payload values, and lists causally linked risk decisions. It shows
an empty state when no evidence exists. It has no trading controls and does not
invent sample headlines, active providers, or policy results.

## Verified local evidence

The control-plane integration test replays the same headline/sentiment fixture
twice and compares canonical event JSON. It also proves a news intent reaches
risk, simulation, fill, portfolio and audit evidence, while a rejected shock
collar creates no OMS order. Python worker tests verify headline and sentiment
frame dispatch and strict unknown-field rejection. These tests do not prove
provider licensing, provider data quality, external paper-session operation,
or live execution readiness.
