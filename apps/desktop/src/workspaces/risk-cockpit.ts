// Risk cockpit workspace.

import { parseCapitalAllocationPlan, parseCapitalAllocationProposal, parseExposureGraph, parseScenarioLossSimulation } from "../evidence/index.js";
import { appendAdvancedEvidenceRows, typedAdvancedEvidence } from "./advanced-evidence.js";
import { displayName, reconciliationText } from "./format.js";
import { appendDefinition, appendTableOrEmpty, createPanel, renderEmpty, renderFeatureEvidence, renderMetrics } from "./panels.js";
import { liveDashboard, operationsDashboard, paperDashboard } from "./snapshot.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";
import { renderFactorExposureBars } from "./visualizers.js";

export function renderRiskCockpit(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const operations = operationsDashboard(snapshot);
  const paper = paperDashboard(snapshot);
  const live = liveDashboard(snapshot);
  const limits = operations?.risk.limits ?? [];
  const breaches = limits.filter((limit) => limit.breached).length;
  const killSwitches = [...(operations?.operational_health.active_kill_switches ?? []), ...(paper?.active_kill_switches ?? []), ...(live?.active_kill_switches ?? [])];
  renderMetrics(summaryRoot, [
    ["Risk state", operations?.risk.state ?? "Unavailable", "Deterministic workbench projection", operations?.risk.state === "NORMAL" ? "good" : operations === undefined ? "warn" : "bad"],
    ["Current equity", operations === undefined ? "Unavailable" : `${operations.risk.current_equity} ${operations.currency}`, "Marked internal equity"],
    ["Limit breaches", String(breaches), `${limits.length} configured cockpit limits`, breaches === 0 ? "good" : "bad"],
    ["Active kill switches", String(killSwitches.length), killSwitches.length === 0 ? "All monitored scopes clear" : killSwitches.join(", "), killSwitches.length === 0 ? "good" : "bad"],
  ]);
  const exposure = createPanel("Exposure and loss control", "Fixed-point values from the selected immutable operations projection.");
  if (operations === undefined) {
    renderEmpty(exposure, "Operations projection unavailable", "Generate an operations dashboard artifact to populate risk controls.");
  } else {
    appendDefinition(exposure, [
      ["Cash", `${operations.risk.cash} ${operations.currency}`], ["Gross exposure", `${operations.risk.gross_exposure} ${operations.currency}`],
      ["Largest position", `${operations.risk.largest_position_exposure} ${operations.currency}`], ["Drawdown", `${operations.risk.drawdown_bps} bps`],
      ["Peak equity", `${operations.risk.effective_peak_equity} ${operations.currency}`], ["Open positions", String(operations.risk.open_positions)],
    ]);
  }
  root.append(exposure);

  const limitPanel = createPanel("Versioned risk limits", "Current values are shown beside their exact evaluated limits.");
  appendTableOrEmpty(limitPanel, ["Limit", "Current", "Threshold", "Status"], limits.map((limit) => [
    displayName(limit.limit_id), limit.current, limit.limit, limit.breached ? "BREACHED" : "Within limit",
  ]), "No risk-limit projection is available.");
  root.append(limitPanel);

  const alerts = createPanel("Alerts and reconciliation", "Audit, broker, schedule, kill-switch, and reconciliation conditions.");
  const alertRows = (operations?.alerts ?? []).map((alert) => [alert.severity, alert.code, alert.subject, alert.summary]);
  alertRows.push(["PAPER", "RECONCILIATION", paper?.account_id ?? "No snapshot", reconciliationText(paper?.last_reconciliation_clean, paper?.last_reconciled_at)]);
  alertRows.push(["LIVE", "RECONCILIATION", live?.account_id ?? "No snapshot", reconciliationText(live?.last_reconciliation_clean, live?.last_reconciled_at)]);
  appendTableOrEmpty(alerts, ["Scope", "Code", "Subject", "State"], alertRows, "No alerts or reconciliation records are available.");
  root.append(alerts);

  const exposureGraphPanel = createPanel(
    "Cross-strategy and factor exposure graph",
    "Decompose exposures across common factors (Momentum, Value, Volatility, Size), sectors, and currencies with concentration limits (RISK-01)."
  );
  exposureGraphPanel.id = "exposure-graph-panel";
  appendAdvancedEvidenceRows(
    exposureGraphPanel,
    snapshot,
    context,
    "exposure_graph",
    parseExposureGraph,
    ["Factor / Category", "Dimension", "Loading (bps)", "Variance Contributed", "Reconciled Status"],
    (graph) => [
      ...graph.factors.map((factor) => [
        "Systematic factor",
        factor.factor_name,
        `${factor.loading_bps} bps`,
        factor.factor_variance_pct,
        graph.unreconciled_discrepancy ? "Unreconciled discrepancy" : "No discrepancy reported",
      ]),
      ...graph.sectors.map((sector) => [
        "Sector concentration",
        sector.sector_name,
        sector.exposure_usd,
        `${sector.weight_bps} bps`,
        graph.unreconciled_discrepancy ? "Unreconciled discrepancy" : "No discrepancy reported",
      ]),
    ],
    "No typed exposure graph is published."
  );
  const exposureGraph = typedAdvancedEvidence(snapshot, "exposure_graph", parseExposureGraph)[0]?.data;
  const factorBars = renderFactorExposureBars(exposureGraph);
  if (factorBars !== undefined) exposureGraphPanel.append(factorBars);
  root.append(exposureGraphPanel);

  const scenarioLossPanel = createPanel(
    "Scenario loss lab and stress testing",
    "Stress-testing portfolio against hypothetical multi-factor shocks, historic crash replays, interest rate jumps, and liquidity freezes without modifying active state (RISK-02)."
  );
  scenarioLossPanel.id = "scenario-loss-panel";
  appendAdvancedEvidenceRows(
    scenarioLossPanel,
    snapshot,
    context,
    "scenario_loss_simulation",
    parseScenarioLossSimulation,
    ["Scenario ID", "Stress Scenario", "Shocks Applied", "Baseline Value", "Stressed Value", "Max Loss (USD / bps)", "Capital Adequacy"],
    (simulation) => [[
      simulation.simulation_id,
      simulation.scenario_name,
      `Equity ${simulation.shock_assumptions.equity_shock_pct}; vol ${simulation.shock_assumptions.volatility_multiplier}; spreads ${simulation.shock_assumptions.spread_expansion_multiplier}; financing ${simulation.shock_assumptions.financing_rate_shock_bps} bps`,
      "Not published by this contract",
      "Not published by this contract",
      `${simulation.estimated_loss_usd} / ${simulation.estimated_loss_bps} bps; liquidity haircut ${simulation.liquidity_haircut_usd}`,
      simulation.capital_adequate ? "Capital adequate" : "Capital inadequate",
    ]],
    "No typed scenario-loss simulation is published."
  );
  root.append(scenarioLossPanel);

  const capitalAllocationPanel = createPanel(
    "Capital allocation and strategy capacity planning",
    "Tiered strategy capital allocations, gross leverage caps, and capacity limits ensuring non-overlapping risk budgets and orderly scaling (RISK-03)."
  );
  capitalAllocationPanel.id = "capital-allocation-panel";
  appendAdvancedEvidenceRows(
    capitalAllocationPanel,
    snapshot,
    context,
    "capital_allocation_plan",
    parseCapitalAllocationPlan,
    ["Strategy Sleeve", "Allocation (bps)", "Allocated Capital", "Gross Leverage Cap", "Estimated Capacity", "Utilization", "Allocation State"],
    (plan) => plan.allocations.map((allocation) => [
      allocation.strategy_id,
      `${allocation.target_weight_bps} bps`,
      allocation.allocated_capital_usd,
      "Not published by this contract",
      "Not published by this contract",
      "Not published by this contract",
      plan.approved_by_policy ? `Policy approved: ${plan.risk_policy_version}` : `Not policy approved: ${plan.risk_policy_version}`,
    ]),
    "No typed capital-allocation plan is published."
  );
  root.append(capitalAllocationPanel);

  const jointCorrelationPanel = createPanel(
    "Joint strategy loss and capital allocation proposal",
    "Council-evaluated capital allocation proposal with volatility targeting, diversification benefits, and drawdown limits (DUR-11, RES-06, RISK-01)."
  );
  jointCorrelationPanel.id = "joint-correlation-panel";
  appendAdvancedEvidenceRows(
    jointCorrelationPanel,
    snapshot,
    context,
    "capital_allocation_proposal",
    parseCapitalAllocationProposal,
    ["Proposal ID", "Total Equity USD", "Target Vol (bps)", "Max Drawdown Limit", "Diversification Ratio", "Allocations (Strategy / Capital / Risk)", "Proposal Status", "Policy Version"],
    (proposal) => [[
      proposal.proposal_id,
      proposal.total_equity_usd,
      `${proposal.target_annual_volatility_bps} bps`,
      `${proposal.max_drawdown_limit_bps} bps`,
      `${proposal.portfolio_diversification_ratio_bps} bps`,
      proposal.allocations.map((a) => `${a.strategy_id}: $${a.recommended_capital_usd} (${a.risk_budget_share_bps} bps, marginal: ${a.marginal_risk_contribution_bps} bps)`).join(" | "),
      proposal.proposal_status,
      proposal.policy_version,
    ]],
    "No typed capital allocation proposal is published."
  );
  root.append(jointCorrelationPanel);

  root.append(renderFeatureEvidence(context, ["paper", "controlled-live", "operations", "execution-risk"]));
}
