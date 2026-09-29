// Execution blotter workspace.

import { parseCapabilityExecutionPlanner, parseExecutionCoachBenchmark, parseOrderDecisionPassport } from "../evidence/index.js";
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { OrderTicket } from "../orders/OrderTicket.js";
import { ComboTicket } from "../orders/ComboTicket.js";
import { appendAdvancedEvidenceRows, appendUnavailableEvidence } from "./advanced-evidence.js";
import { field, record } from "./format.js";
import { appendTableOrEmpty, createPanel, renderFeatureEvidence, renderMetrics } from "./panels.js";
import { liveDashboard, paperDashboard } from "./snapshot.js";
import { mountedTickets } from "./ticket-mounts.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";

const OMS_LIFECYCLE_COVERAGE: ReadonlyArray<readonly [string, string, string]> = [
  ["Fill before acknowledgement", "Handled", "Execution evidence is authoritative and idempotent"],
  ["Fill while cancellation pending", "Handled", "Cumulative quantity advances without losing cancel intent"],
  ["Partial fill then terminal outcome", "Handled", "Filled quantity is preserved for cancel, reject, or expiry"],
  ["Cancel rejection", "Handled", "Explicit broker evidence restores the valid working state"],
  ["Replace lifecycle", "Handled", "Requested, replaced, and rejected outcomes are auditable"],
  ["Broker order versions", "Handled", "Every modified broker version retains lineage"],
  ["Late terminal messages", "Handled", "Safe no-op unless authoritative execution changes quantity"],
  ["UNKNOWN resolution", "Handled", "A new durable resolution step preserves prior history"],
  ["Terminal status vs cumulative fill", "Handled", "Non-filled terminal states cannot claim full quantity"],
];

export function renderExecutionBlotter(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const paper = paperDashboard(snapshot);
  const live = liveDashboard(snapshot);
  const executionEvents = snapshot.events.filter((item) => isExecutionEvent(field(item.data, "event_type")));
  const orders = executionEvents.filter((item) => field(item.data, "event_type").includes("order"));
  const fills = executionEvents.filter((item) => /fill|execution/i.test(field(item.data, "event_type")));
  renderMetrics(summaryRoot, [
    ["Lifecycle events", String(executionEvents.length), "Intent, risk, OMS, fill, cancel, replace, and terminal evidence"],
    ["Order transitions", String(orders.length), "Canonical order state changes"],
    ["Executions", String(fills.length), "Idempotent execution evidence"],
    ["Unknown orders", String((paper?.unknown_orders ?? 0) + (live?.unknown_orders ?? 0)), "PAPER and controlled-LIVE unresolved state", (paper?.unknown_orders ?? 0) + (live?.unknown_orders ?? 0) === 0 ? "good" : "bad"],
  ]);
  const environment = createPanel("Execution environments", "Simulation, PAPER, and controlled-LIVE remain visibly distinct.");
  appendTableOrEmpty(environment, ["Environment", "Account", "Broker", "Working", "Unknown", "Audit", "Gate"], [
    ["SIMULATION", "Fixture/backtest accounts", "No broker", "Evidence trail", "0", "Canonical events", "Research available"],
    ["PAPER", paper?.account_id ?? "No snapshot", paper === undefined ? "Unavailable" : paper.broker_connected ? "Connected" : "Disconnected", String(paper?.working_orders ?? 0), String(paper?.unknown_orders ?? 0), paper?.complete_auditability ? "Complete" : "Incomplete", `${paper?.clean_paper_days ?? 0}/${paper?.required_paper_days ?? 30}`],
    ["LIVE / SHADOW-CANARY", live?.account_id ?? "No snapshot", live === undefined ? "Unavailable" : live.broker_connected ? "Connected" : "Disconnected", String(live?.working_orders ?? 0), String(live?.unknown_orders ?? 0), live?.complete_auditability ? "Complete" : "Incomplete", `${live?.clean_live_days ?? 0}/${live?.required_live_days ?? 60}`],
  ], "No execution environment snapshots are available.");
  root.append(environment);

  const ticket = createPanel("Order ticket", "Submit a declarative PAPER intent to the configured Risk/OMS route. Controlled-LIVE is not exposed by this ticket.");
  const ticketRoot = document.createElement("div");
  ticket.append(ticketRoot);
  root.append(ticket);
  try {
    mountedTickets.order = createRoot(ticketRoot);
    mountedTickets.order.render(createElement(OrderTicket, {
      defaultAccountId: paper?.account_id ?? "",
      defaultEnvironment: "PAPER",
    }));
  } catch {
    // Non-browser or mock DOM testing environment
  }

  const comboTicket = createPanel(
    "Combination ticket",
    "Submit one atomic multi-leg PAPER combination to the configured Risk/OMS route as a single order. Controlled-LIVE is not exposed by this ticket.",
  );
  const comboTicketRoot = document.createElement("div");
  comboTicket.append(comboTicketRoot);
  root.append(comboTicket);
  try {
    mountedTickets.combo = createRoot(comboTicketRoot);
    mountedTickets.combo.render(createElement(ComboTicket, {
      defaultAccountId: paper?.account_id ?? "",
    }));
  } catch {
    // Non-browser or mock DOM testing environment
  }

  const blotter = createPanel("Causal execution blotter", "Every row links event, causation, correlation, actor, and normalized lifecycle payload.");
  const blotterRows = executionEvents.map((item) => {
    const payload = record(item.data.payload);
    return [field(item.data, "event_time"), field(item.data, "event_type"), field(payload, "order_id") || field(payload, "intent_id") || field(payload, "execution_id"),
      field(payload, "new_state") || field(payload, "status") || (payload.approved === true ? "APPROVED" : payload.approved === false ? "REJECTED" : field(payload, "reason")),
      field(payload, "quantity") || field(payload, "filled_quantity") || field(payload, "cumulative_quantity"), field(item.data, "correlation_id"), item.artifact];
  });
  appendTableOrEmpty(blotter, ["Time", "Phase", "Order / intent", "State / decision", "Quantity", "Correlation", "Source"], blotterRows, "No execution lifecycle events are available.", (index) => context.onOpenArtifact(executionEvents[index]?.artifact ?? ""));
  root.append(blotter);

  const commandBoundary = createPanel(
    "Order-management boundary",
    "Cancellation and close-position requests remain unavailable until a separately qualified native Risk/OMS route is supplied. Inspect immutable evidence here; do not treat this browser surface as a command channel.",
  );
  appendUnavailableEvidence(commandBoundary, "No native command route is currently configured.");
  root.append(commandBoundary);

  const riskDecisions = snapshot.events.filter((item) => field(item.data, "event_type") === "risk.decision.v1");
  const risk = createPanel("Explainable risk decisions", "Every approval and rejection exposes the exact rule outcomes, evaluated inputs and thresholds, policy version, and decision actor.");
  appendTableOrEmpty(risk, ["Time", "Decision", "Intent", "Outcome", "Reason codes", "Evaluated inputs and limits", "Policy", "Actor"], riskDecisions.map((item) => {
    const payload = record(item.data.payload);
    return [
      field(item.data, "event_time"),
      field(payload, "decision_id"),
      field(payload, "intent_id"),
      payload.approved === true ? "APPROVED" : payload.approved === false ? "REJECTED" : "UNKNOWN",
      stringList(payload.reason_codes).join(", "),
      field(payload, "evaluated_limits"),
      field(payload, "policy_version"),
      field(payload, "actor"),
    ];
  }), "No immutable risk-decision events are available.", (index) => context.onOpenArtifact(riskDecisions[index]?.artifact ?? ""));
  root.append(risk);

  const tcaRows: Array<{ artifact: string; values: string[] }> = [];
  const benchmarkRows: Array<{ artifact: string; values: string[] }> = [];
  for (const evidence of snapshot.execution_evidence) {
    const transactionCost = record(evidence.data.transaction_cost);
    const reports = Array.isArray(transactionCost.reports) ? transactionCost.reports : [];
    for (const candidate of reports) {
      const report = record(candidate);
      tcaRows.push({
        artifact: evidence.artifact,
        values: [evidence.artifact, field(report, "analysis_id"), field(report, "strategy_id"), field(report, "side"), field(report, "filled_quantity"), field(report, "execution_vwap"), field(report, "arrival_total_cost"), field(report, "target_total_cost")],
      });
    }
    const measurement = record(evidence.data.measurement);
    if (measurement.p99_micros !== undefined) {
      benchmarkRows.push({
        artifact: evidence.artifact,
        values: [evidence.artifact, field(evidence.data, "observed_at"), field(evidence.data, "policy_version"), field(measurement, "p99_micros"), field(measurement, "threshold_micros"), measurement.within_threshold === true ? "Within local threshold" : "Outside local threshold"],
      });
    }
  }
  const tca = createPanel("Transaction-cost analysis", "Immutable implementation-shortfall evidence measured from caller-supplied frozen arrival and target benchmarks; it is not a broker-statement acceptance claim.");
  appendTableOrEmpty(tca, ["Artifact", "Analysis", "Strategy", "Side", "Filled", "VWAP", "Arrival total", "Target total"], tcaRows.map((row) => row.values), "No transaction-cost artifact is indexed.", (index) => context.onOpenArtifact(tcaRows[index]?.artifact ?? ""));
  root.append(tca);

  const benchmark = createPanel("Local risk-evaluator benchmark", "Explicit-hardware local timing observation only; production availability and load evidence remain separate gates.");
  appendTableOrEmpty(benchmark, ["Artifact", "Observed at", "Policy", "p99 (µs)", "Threshold (µs)", "Result"], benchmarkRows.map((row) => row.values), "No local risk benchmark artifact is indexed.", (index) => context.onOpenArtifact(benchmarkRows[index]?.artifact ?? ""));
  root.append(benchmark);

  const lifecycle = createPanel("Broker lifecycle condition coverage", "Explicit handling for the out-of-order and modification cases recorded in the system review.");
  appendTableOrEmpty(lifecycle, ["Condition", "Implementation", "Invariant"], OMS_LIFECYCLE_COVERAGE.map((row) => [...row]), "No lifecycle coverage metadata is available.");
  root.append(lifecycle);

  const passportPanel = createPanel(
    "Order decision passport",
    "One unified attributable audit trail from market opportunity signal, policy inputs, and risk approval through OMS routing, child fills, and ledger consequences (EXEC-02)."
  );
  passportPanel.id = "decision-passport-panel";
  appendAdvancedEvidenceRows(
    passportPanel,
    snapshot,
    context,
    "order_decision_passport",
    parseOrderDecisionPassport,
    ["Passport ID", "Opportunity Signal", "Risk Pre-Trade Evaluation", "OMS Routing Plan", "Executions & Fees", "Journal Consequences"],
    (passport) => [[
      passport.passport_id,
      `${passport.signal_attribution.strategy_version}; ${passport.signal_attribution.signal_power_bps} bps; ${passport.signal_attribution.opportunity_description}`,
      `${passport.risk_evaluation.approved ? "Approved" : "Rejected"}; ${passport.risk_evaluation.evaluated_limits.join(", ")}; ${passport.risk_evaluation.headroom_remaining_bps} bps`,
      `${passport.routing_plan.algorithm}; ${passport.routing_plan.allocated_slices_count} slice(s) at ${passport.routing_plan.primary_venue}`,
      passport.executions.map((execution) => `${execution.quantity} @ ${execution.price}; fee ${execution.fee}`).join(" | ") || "No execution recorded",
      `${passport.accounting_consequences.journal_entry_id}; cash ${passport.accounting_consequences.cash_delta}; position ${passport.accounting_consequences.position_after}`,
    ]],
    "No typed order-decision passport is published."
  );
  root.append(passportPanel);

  const executionCoachPanel = createPanel(
    "Execution coach, post-trade benchmark & replay-vs-live diff",
    "Post-trade attribution decomposing slippage against arrival price, interval VWAP, and replay counterfactuals to isolate routing alpha, fee leakage, and market impact (EXEC-03, RES-07)."
  );
  executionCoachPanel.id = "execution-coach-panel";
  appendAdvancedEvidenceRows(
    executionCoachPanel,
    snapshot,
    context,
    "execution_coach_benchmark",
    parseExecutionCoachBenchmark,
    ["Benchmark ID", "Order / Strategy", "Filled Quantity", "Arrival Slippage", "VWAP Slippage", "Replay vs Live Diff", "Coach Recommendation"],
    (benchmark) => [[
      benchmark.analysis_id,
      benchmark.order_id,
      "Not published by this contract",
      `${benchmark.realized_shortfall_bps} bps`,
      `${benchmark.realized_vwap} vs target ${benchmark.target_price}`,
      `${benchmark.slippage_drag_bps + benchmark.market_impact_bps + benchmark.fee_drag_bps} bps decomposed drag`,
      benchmark.execution_grade,
    ]],
    "No typed execution-coach benchmark is published."
  );
  root.append(executionCoachPanel);

  const executionPlannerPanel = createPanel(
    "Capability-aware execution schedule and venue routing",
    "Plan algorithmic child slices (TWAP, VWAP, Passive Peg) while verifying venue order kind support, iceberg capabilities, and volume participation caps (EXEC-04)."
  );
  executionPlannerPanel.id = "execution-planner-panel";
  appendAdvancedEvidenceRows(
    executionPlannerPanel,
    snapshot,
    context,
    "capability_execution_planner",
    parseCapabilityExecutionPlanner,
    ["Plan ID", "Parent Order", "Target Venue", "Algorithm", "Max Participation", "Passive Peg Offset", "Slices Planned", "Capability State"],
    (plan) => [[
      plan.plan_id,
      plan.parent_order_id,
      plan.target_venue,
      plan.algorithm,
      plan.max_volume_participation_pct,
      `${plan.passive_pegging_offset_bps} bps`,
      plan.schedule_slices.map((slice) => `${slice.slice_sequence}: ${slice.allocated_quantity}`).join(" | "),
      plan.disposition,
    ]],
    "No typed execution plan is published."
  );
  root.append(executionPlannerPanel);

  root.append(renderFeatureEvidence(context, ["replay", "paper", "controlled-live", "execution-risk"]));
}

function isExecutionEvent(eventType: string): boolean {
  return /intent|risk\.decision|order|execution|fill|cancel|replace|reject|expir/i.test(eventType);
}

function stringList(value: unknown): string[] {
  return Array.isArray(value) && value.every((item) => typeof item === "string") ? value : [];
}
