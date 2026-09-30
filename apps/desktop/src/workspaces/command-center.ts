// Daily operating brief and command-center workspace.

import { parseAttentionBudget, parseContinuityPolicy, parseMarketScanner } from "../evidence/index.js";
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { OrderTicket } from "../orders/OrderTicket.js";
import { appendAdvancedEvidenceRows, typedAdvancedEvidence } from "./advanced-evidence.js";
import { displayName, strategyIdentityRows } from "./format.js";
import { appendTableOrEmpty, createPanel, renderArtifactPanel, renderMetrics, renderProjectionIntegrityPanel } from "./panels.js";
import { liveDashboard, operationsDashboard, paperDashboard } from "./snapshot.js";
import { mountedTickets } from "./ticket-mounts.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";
import { renderAttentionGauge } from "./visualizers.js";

export function renderCommandCenter(
  summaryRoot: HTMLElement,
  root: HTMLElement,
  snapshot: WorkspaceSnapshot,
  context: WorkspaceContext,
): void {
  const services = context.status === null ? [] : Object.values(context.status.services);
  const healthyServices = services.filter((service) => service.status === "healthy").length;
  const operations = operationsDashboard(snapshot);
  const paper = paperDashboard(snapshot);
  const live = liveDashboard(snapshot);
  const strategyIdentities = strategyIdentityRows(snapshot, operations);
  const openGateCount = (paper?.promotion_eligible ? 0 : 1) + (live?.promotion_eligible ? 0 : 1);
  const alertCount = (operations?.alerts.length ?? 0) + (paper?.unexplained_incidents ?? 0) +
    (live?.unresolved_incidents ?? 0) + (paper?.unknown_orders ?? 0) + (live?.unknown_orders ?? 0);
  renderMetrics(summaryRoot, [
    ["Runtime services", services.length === 0 ? "Unavailable" : `${healthyServices}/${services.length}`, "Dashboard, gRPC kernel, PostgreSQL, and object storage", healthyServices === services.length ? "good" : "bad", true],
    ["Indexed evidence", String(snapshot.counts.artifacts ?? 0), "Immutable artifacts across the complete repository"],
    ["Operator attention", String(alertCount), "Alerts, unknown orders, and unresolved discrepancies", alertCount === 0 ? "good" : "bad"],
    ["Promotion gates", `${openGateCount} open`, "PAPER and controlled-LIVE dashboard evidence", openGateCount === 0 ? "good" : "warn"],
  ]);

  const briefPanel = createPanel(
    "Daily Operating Brief",
    "Consolidated operational readiness for the current trading session. Unknown states prevent an all-clear summary.",
  );
  briefPanel.id = "daily-brief";

  const hasUnknownState = context.status === null || services.length === 0 ||
    paper === undefined || (paper?.unknown_orders ?? 0) > 0 || (paper?.unexplained_incidents ?? 0) > 0 ||
    (live?.unknown_orders ?? 0) > 0 || (live?.unresolved_incidents ?? 0) > 0 ||
    healthyServices < services.length;

  const briefStatusText = hasUnknownState
    ? "ATTENTION REQUIRED · Active discrepancies, unknown orders, or unverified feed state detected"
    : "NOMINAL · All monitored dependencies healthy, no unknown orders or reconciliation incidents";

  const briefBadgeClass = hasUnknownState ? "f-badge f-badge--warn" : "f-badge f-badge--good";

  const headerDiv = document.createElement("div");
  headerDiv.className = "brief-header";
  const statusBadge = document.createElement("span");
  statusBadge.className = briefBadgeClass;
  statusBadge.textContent = briefStatusText;
  headerDiv.append(statusBadge);
  briefPanel.append(headerDiv);

  const briefStatements = [
    [
      "Connectivity & Feeds",
      services.length === 0
        ? "UNKNOWN: Runtime services unreachable"
        : `${healthyServices}/${services.length} services healthy · PAPER: ${paper?.broker_connected ? "CONNECTED" : "DISCONNECTED"}`,
      context.status === null ? "No status response" : "Linked: /api/v1/status & paper-dashboard.json",
      healthyServices === services.length && paper?.broker_connected ? "HEALTHY" : "ATTENTION",
    ],
    [
      "Order Lifecycle & Reconcile",
      `Unknown orders: ${(paper?.unknown_orders ?? 0) + (live?.unknown_orders ?? 0)} · Unresolved incidents: ${(paper?.unexplained_incidents ?? 0) + (live?.unresolved_incidents ?? 0)}`,
      paper ? "Linked: paper-dashboard.json" : "No paper projection",
      (paper?.unknown_orders ?? 0) === 0 && (paper?.unexplained_incidents ?? 0) === 0 ? "HEALTHY" : "ATTENTION",
    ],
    [
      "Risk Headroom & Policy",
      operations ? `State: ${operations.risk.state} · Evaluated limits: ${operations.risk.limits.length}` : "UNKNOWN: No operations projection",
      operations ? "Linked: operations-dashboard.json" : "No operations projection",
      operations?.risk.state === "NORMAL" ? "HEALTHY" : "ATTENTION",
    ],
    [
      "Operational Gates",
      `${openGateCount} gates require external evidence before production promotion`,
      "Linked: 03-roadmap-and-gates.md",
      "MONITORED",
    ],
  ];

  appendTableOrEmpty(
    briefPanel,
    ["Dimension", "Operational Observation", "Evidence Anchor", "Disposition"],
    briefStatements,
    "No brief data available.",
  );
  root.append(briefPanel);

  const projectionIntegrity = renderProjectionIntegrityPanel(snapshot, context);
  if (projectionIntegrity !== undefined) root.append(projectionIntegrity);

  const orderControlPanel = createPanel("Active Trading Control", "Submit declarative PAPER intents to the configured Risk/OMS route. Controlled-LIVE remains evidence-only in this workstation.");
  const orderControlContainer = document.createElement("div");
  orderControlPanel.append(orderControlContainer);
  root.append(orderControlPanel);
  try {
    mountedTickets.order = createRoot(orderControlContainer);
    mountedTickets.order.render(createElement(OrderTicket, {
      defaultAccountId: paper?.account_id ?? "",
      defaultEnvironment: "PAPER",
    }));
  } catch {
    // Non-browser or mock DOM testing environment
  }

  const serviceSection = createPanel("Runtime and dependencies", "Live health from the container boundary.");
  const serviceRows = context.status === null ? [] : Object.entries(context.status.services).map(([name, service]) => [
    displayName(name), service.status.toUpperCase(), service.detail,
  ]);
  appendTableOrEmpty(serviceSection, ["Service", "Status", "Detail"], serviceRows, "Runtime health is unavailable.");
  root.append(serviceSection);

  const operatingStatus = createPanel("System, broker, strategy, and risk status", "One evidence-backed operating view; an unavailable source is never treated as healthy.");
  appendTableOrEmpty(operatingStatus, ["Area", "Status", "Evidence"], [
    ["System dependencies", services.length === 0 ? "UNAVAILABLE" : healthyServices === services.length ? "HEALTHY" : "DEGRADED", services.length === 0 ? "No runtime status" : `${healthyServices}/${services.length} healthy`],
    ["Strategy identities", strategyIdentities.length > 0 ? "EVIDENCED" : "UNAVAILABLE", `${strategyIdentities.length} versioned identity record(s)`],
    ["Risk", operations?.risk.state ?? "UNAVAILABLE", operations === undefined ? "No operations projection" : `${operations.risk.limits.length} evaluated limit(s)`],
    ["PAPER broker", paper === undefined ? "UNAVAILABLE" : paper.broker_connected ? "CONNECTED" : "DISCONNECTED", paper?.account_id ?? "No PAPER projection"],
    ["Controlled-LIVE broker", live === undefined ? "UNAVAILABLE" : live.broker_connected ? "CONNECTED" : "DISCONNECTED", live?.account_id ?? "No controlled-LIVE projection"],
  ], "No operating-status evidence is available.");
  root.append(operatingStatus);

  const environment = createPanel("Environment readiness", "Code capability is separated from observed operating evidence.");
  appendTableOrEmpty(environment, ["Environment / gate", "Observed", "Required", "Decision"], [
    ["Historical research", `${snapshot.backtests.length} indexed backtest artifact(s)`, "Frozen, reproducible run evidence", snapshot.backtests.length > 0 ? "Evidence indexed" : "No evidence indexed"],
    ["PAPER sessions", paper === undefined ? "No PAPER dashboard" : String(paper.clean_paper_days), paper === undefined ? "PAPER dashboard required" : String(paper.required_paper_days), paper?.promotion_eligible ? "Eligible according to projection" : "Not eligible or unknown"],
    ["Controlled LIVE sessions", live === undefined ? "No controlled-LIVE dashboard" : String(live.clean_live_days), live === undefined ? "Controlled-LIVE dashboard required" : String(live.required_live_days), live?.promotion_eligible ? "Eligible according to projection" : "Not eligible or unknown"],
    ["Commercial operation", `${snapshot.commercial.length} ledger record(s)`, "External customer and entitlement evidence", "Not inferred from local records"],
  ], "No gate data is available.");
  root.append(environment);

  const attention = createPanel("Attention queue", "Conditions requiring operator investigation; no browser mutation is offered.");
  const rows: string[][] = [];
  for (const alert of operations?.alerts ?? []) {
    rows.push([alert.severity, alert.code, alert.subject, alert.summary]);
  }
  if ((paper?.unknown_orders ?? 0) > 0) rows.push(["CRITICAL", "PAPER_UNKNOWN", paper?.account_id ?? "PAPER", `${paper?.unknown_orders} unknown order(s)`]);
  if ((paper?.unexplained_incidents ?? 0) > 0) rows.push(["CRITICAL", "PAPER_RECONCILIATION", paper?.account_id ?? "PAPER", `${paper?.unexplained_incidents} unexplained incident(s)`]);
  if ((live?.unknown_orders ?? 0) > 0) rows.push(["CRITICAL", "LIVE_UNKNOWN", live?.account_id ?? "LIVE", `${live?.unknown_orders} unknown order(s)`]);
  if ((live?.unresolved_incidents ?? 0) > 0) rows.push(["CRITICAL", "LIVE_RECONCILIATION", live?.account_id ?? "LIVE", `${live?.unresolved_incidents} unresolved incident(s)`]);
  appendTableOrEmpty(attention, ["Severity", "Code", "Subject", "Summary"], rows, "No active evidence-backed alerts.");
  root.append(attention);

  const scannerPanel = createPanel(
    "Explainable market scanner",
    "Screen instruments against versioned indicators and point-in-time universe conditions with ranked reasons (SOLO-04)."
  );
  scannerPanel.id = "market-scanner-panel";
  appendAdvancedEvidenceRows(
    scannerPanel,
    snapshot,
    context,
    "market_scanner",
    parseMarketScanner,
    ["Rank", "Symbol / Instrument", "Close Price", "Momentum (bps)", "RSI (14)", "Matched Conditions", "Rationale"],
    (scanner) =>
      scanner.candidates.map((cand) => [
        `#${cand.rank}`,
        `${cand.symbol} (${cand.instrument_id})`,
        cand.close_price,
        `${cand.momentum_score_bps} bps`,
        cand.rsi_14,
        cand.matched_conditions.join(", "),
        cand.rationale,
      ]),
    "No typed market-scanner candidate records are published."
  );
  root.append(scannerPanel);

  const consolidatedAttention = createPanel(
    "Consolidated attention queue",
    "Grouped incidents with underlying root causes, suppression of duplicates, and acknowledgement deadlines (SOLO-05)."
  );
  consolidatedAttention.id = "consolidated-attention-panel";
  appendTableOrEmpty(
    consolidatedAttention,
    ["Severity", "Code", "Subject", "Summary"],
    rows,
    "No consolidated attention items."
  );
  root.append(consolidatedAttention);

  const sessionPlaybooks = createPanel(
    "Session playbooks and away mode",
    "Structured operational phases (prepare, observe, operate, reconcile, review) with bounded unattended intervals (SOLO-06)."
  );
  sessionPlaybooks.id = "session-playbooks-panel";
  appendAdvancedEvidenceRows(
    sessionPlaybooks,
    snapshot,
    context,
    "continuity_policy",
    parseContinuityPolicy,
    ["Policy", "Away mode permitted", "Unattended interval", "Heartbeat", "Feed stale threshold", "Broker disconnect action", "Restart budget"],
    (policy) => [[
      policy.policy_id,
      String(policy.away_mode_permitted),
      `${policy.unattended_interval_minutes} min`,
      `${policy.heartbeat_interval_seconds} s`,
      `${policy.feed_stale_threshold_seconds} s`,
      policy.broker_disconnect_action,
      `${policy.max_restarts_per_hour}/hour`,
    ]],
    "No typed continuity policy is published."
  );
  root.append(sessionPlaybooks);

  const awayDeskPanel = createPanel(
    "Desk departure & away readiness check",
    "Supervise active protections, authorized unattended interval, broker connectivity, kill-switch readiness, and escalation routing before leaving the trading desk (SOLO-05, SOLO-06, EXEC-01)."
  );
  awayDeskPanel.id = "away-desk-readiness-panel";
  appendAdvancedEvidenceRows(
    awayDeskPanel,
    snapshot,
    context,
    "continuity_policy",
    parseContinuityPolicy,
    ["Policy", "Away mode permitted", "Unattended interval", "Heartbeat", "Feed stale threshold", "Disconnect response"],
    (policy) => [[
      policy.policy_id,
      String(policy.away_mode_permitted),
      `${policy.unattended_interval_minutes} min`,
      `${policy.heartbeat_interval_seconds} s`,
      `${policy.feed_stale_threshold_seconds} s`,
      policy.broker_disconnect_action,
    ]],
    "No typed away-mode policy is published; readiness is unknown."
  );
  appendAdvancedEvidenceRows(
    awayDeskPanel,
    snapshot,
    context,
    "attention_budget",
    parseAttentionBudget,
    ["Budget ID", "Session Date", "Cognitive Load", "Interruption Rate", "Alarms (Active / Suppressed)", "Escalated Tasks", "Budget Exhausted"],
    (budget) => [[
      budget.budget_id,
      budget.session_date,
      `${budget.cognitive_load_score_bps} bps`,
      `${budget.interruptions_per_hour}/hr`,
      `${budget.active_alarms_count} active / ${budget.suppressed_duplicates_count} suppressed`,
      budget.escalated_critical_tasks.join(", ") || "None",
      budget.budget_exhausted ? "EXHAUSTED" : "NOMINAL",
    ]],
    "No typed attention budget is published."
  );
  const attentionBudget = typedAdvancedEvidence(snapshot, "attention_budget", parseAttentionBudget)[0]?.data;
  const attentionGauge = renderAttentionGauge(attentionBudget);
  if (attentionGauge !== undefined) awayDeskPanel.append(attentionGauge);
  root.append(awayDeskPanel);

  root.append(renderArtifactPanel("Recent evidence", context.artifacts.slice(0, 12), context.onOpenArtifact));
}
