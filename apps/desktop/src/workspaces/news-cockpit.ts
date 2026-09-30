// News cockpit workspace.

import { parseAssumptionRegimeMonitor, parseEventExposureCalendar, parseKnowledgeSnapshot, parseNewsRevisionTimeline } from "../evidence/index.js";
import { appendAdvancedEvidenceRows } from "./advanced-evidence.js";
import { field, record, shortHash, text } from "./format.js";
import { appendTableOrEmpty, createPanel, renderFeatureEvidence, renderMetrics } from "./panels.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";

export function renderNewsCockpit(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const headlines = snapshot.events.filter((item) => field(item.data, "event_type") === "news.headline.v1");
  const sentiments = snapshot.events.filter((item) => field(item.data, "event_type") === "news.sentiment.v1");
  const headlinesByNewsId = new Map(headlines.map((item) => [field(record(item.data.payload), "news_id"), item]));
  const sentimentEventIds = new Set(sentiments.flatMap((item) => [
    field(item.data, "event_id"),
    field(record(item.data.payload), "event_id"),
  ]).filter(Boolean));
  const riskDecisions = snapshot.events.filter((item) =>
    field(item.data, "event_type") === "risk.decision.v1" && sentimentEventIds.has(field(item.data, "causation_id"))
  );
  const sources = new Set(headlines.map((item) => field(record(item.data.payload), "source")).filter(Boolean));
  renderMetrics(summaryRoot, [
    ["Headlines", String(headlines.length), "Validated news.headline.v1 envelopes"],
    ["Sentiment vectors", String(sentiments.length), "Deterministic news.sentiment.v1 evidence"],
    ["Sources", String(sources.size), "Declared provenance labels in stored headline evidence"],
    ["Linked risk decisions", String(riskDecisions.length), "Causation links from sentiment through pre-trade risk"],
  ]);

  const headlinePanel = createPanel("Headline evidence", "Stored source labels and headlines are rendered from immutable events; an empty workspace does not infer providers or market activity.");
  appendTableOrEmpty(headlinePanel, ["Time", "Source", "News ID", "Headline", "Instruments", "Artifact"], headlines.map((item) => {
    const payload = record(item.data.payload);
    const instruments = Array.isArray(payload.entity_tickers) ? payload.entity_tickers.map(text).join(", ") : "";
    return [
      field(item.data, "event_time"), field(payload, "source"), field(payload, "news_id"), field(payload, "headline"), instruments, item.artifact,
    ];
  }), "No headline evidence is available.", (index) => context.onOpenArtifact(headlines[index]?.artifact ?? ""));
  root.append(headlinePanel);

  const sentimentPanel = createPanel("Sentiment signals", "Signal power is derived from stored integer-BPS polarity and confidence values; no browser-side classifier is used.");
  appendTableOrEmpty(sentimentPanel, ["Time", "Instrument", "Taxonomy", "Polarity", "Confidence", "Novelty", "Surprise", "Signal power", "Headline", "Artifact"], sentiments.map((item) => {
    const payload = record(item.data.payload);
    const signalPower = sentimentSignalPower(payload.sentiment_polarity_bps, payload.confidence_bps);
    const headline = headlinesByNewsId.get(field(payload, "causation_news_id"));
    return [
      field(item.data, "event_time"), field(payload, "instrument_id"), field(payload, "taxonomy"),
      field(payload, "sentiment_polarity_bps"), field(payload, "confidence_bps"), field(payload, "novelty_score_bps"),
      field(payload, "surprise_magnitude_bps"), signalPower,
      headline === undefined ? "No stored headline link" : field(record(headline.data.payload), "headline"), item.artifact,
    ];
  }), "No sentiment evidence is available.", (index) => context.onOpenArtifact(sentiments[index]?.artifact ?? ""));
  root.append(sentimentPanel);

  const riskPanel = createPanel("Causally linked risk decisions", "Only recorded risk decisions with a direct stored causation link to a sentiment event are shown.");
  appendTableOrEmpty(riskPanel, ["Time", "Decision", "Intent", "Reason codes", "Causation", "Artifact"], riskDecisions.map((item) => {
    const payload = record(item.data.payload);
    const reasonCodes = Array.isArray(payload.reason_codes) ? payload.reason_codes.map(text).join(", ") : field(payload, "reason_code");
    return [
      field(item.data, "event_time"), field(payload, "decision") || field(payload, "outcome"), field(payload, "intent_id"),
      reasonCodes, field(item.data, "causation_id"), item.artifact,
    ];
  }), "No stored risk decisions are causally linked to news sentiment.", (index) => context.onOpenArtifact(riskDecisions[index]?.artifact ?? ""));
  root.append(riskPanel);

  const knowledgePanel = createPanel(
    "Point-in-time knowledge graph",
    "Attributable links connecting companies, instruments, filings, headlines, and exposures with effective availability timestamps (DATA-02)."
  );
  knowledgePanel.id = "knowledge-graph-panel";
  appendAdvancedEvidenceRows(
    knowledgePanel,
    snapshot,
    context,
    "knowledge_snapshot",
    parseKnowledgeSnapshot,
    ["Source Entity", "Type", "Relation", "Target Entity", "Effective As-Of Time", "Provenance Hash"],
    (knowledge) => {
      const nodes = new Map(knowledge.entity_nodes.map((node) => [node.entity_id, node]));
      return knowledge.relationships.map((relationship) => [
        relationship.source_entity_id,
        nodes.get(relationship.source_entity_id)?.entity_type ?? "Unknown",
        relationship.relation_type,
        relationship.target_entity_id,
        relationship.effective_time,
        shortHash(relationship.provenance_hash),
      ]);
    },
    "No typed point-in-time knowledge snapshot is published."
  );
  root.append(knowledgePanel);

  const revisionPanel = createPanel(
    "News revision and novelty timeline",
    "Track original announcements versus syndicated duplicates, corrections, and model interpretations without overwriting history (DATA-03)."
  );
  revisionPanel.id = "news-revision-panel";
  appendAdvancedEvidenceRows(
    revisionPanel,
    snapshot,
    context,
    "news_revision_timeline",
    parseNewsRevisionTimeline,
    ["Seq", "Received Time", "Source", "Kind", "Headline", "Entity Confidence", "Supersedes Seq", "Hash"],
    (timeline) =>
      timeline.chain.map((entry) => [
        `#${entry.version_sequence}`,
        entry.received_at,
        entry.source_id,
        entry.kind,
        entry.headline,
        `${entry.entity_confidence_bps} bps`,
        entry.supersedes_sequence !== null ? `#${entry.supersedes_sequence}` : "None",
        shortHash(entry.content_hash),
      ]),
    "No typed news-revision timeline records are published."
  );
  root.append(revisionPanel);

  const calendarPanel = createPanel(
    "Event exposure calendar",
    "Point-in-time schedule for earnings announcements, corporate actions, trading halts, options expiry, and settlement dates (DATA-04)."
  );
  calendarPanel.id = "event-exposure-calendar";
  appendAdvancedEvidenceRows(
    calendarPanel,
    snapshot,
    context,
    "event_exposure_calendar",
    parseEventExposureCalendar,
    ["Scheduled (UTC)", "Instrument", "Category", "Event Detail", "Status", "Source Evidence"],
    (calendar) => calendar.scheduled_events.map((event) => [
      event.scheduled_time,
      event.instrument_id,
      event.category,
      event.event_id,
      event.status,
      event.source_evidence,
    ]),
    "No typed event exposure calendar is published."
  );
  root.append(calendarPanel);

  const regimeMonitorPanel = createPanel(
    "Assumption and regime drift monitor",
    "Continuous monitoring of baseline economic assumptions, spread regimes, volatility bands, and market liquidity to identify out-of-regime research models (DATA-05)."
  );
  regimeMonitorPanel.id = "regime-monitor-panel";
  appendAdvancedEvidenceRows(
    regimeMonitorPanel,
    snapshot,
    context,
    "assumption_regime_monitor",
    parseAssumptionRegimeMonitor,
    ["Assumption ID", "Model Scope", "Baseline Parameter", "Observed Regime", "Drift (bps)", "Threshold (bps)", "Monitor State"],
    (monitor) => monitor.impacted_strategy_assumptions.map((assumption) => [
      monitor.regime_id,
      assumption.strategy_id,
      assumption.assumed_condition,
      `${monitor.current_regime}: ${assumption.observed_condition}`,
      String(monitor.indicators.effective_spread_bps),
      "Not published by this contract",
      assumption.breach_status,
    ]),
    "No typed assumption-regime monitor is published."
  );
  root.append(regimeMonitorPanel);

  root.append(renderFeatureEvidence(context, ["news", "research", "execution-risk"]));
}

/** Missing or malformed evidence must never be presented as a neutral signal. */
export function sentimentSignalPower(polarity: unknown, confidence: unknown): string {
  if (typeof polarity !== "number" || typeof confidence !== "number" ||
      !Number.isSafeInteger(polarity) || !Number.isSafeInteger(confidence) ||
      Math.abs(polarity) > 10_000 || confidence < 0 || confidence > 10_000) return "Unavailable";
  return `${Math.round(Math.abs(polarity) * confidence / 10_000)} bps`;
}
