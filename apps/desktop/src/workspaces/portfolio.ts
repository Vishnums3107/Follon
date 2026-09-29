// Portfolio workspace.

import { parseFundLedgerStatement, parseFxPricingDashboard, parseMultiAssetExpansionPlan, parseStatementReconciliation } from "../evidence/index.js";
import { appendAdvancedEvidenceRows } from "./advanced-evidence.js";
import { displayName, formatTime } from "./format.js";
import { appendTableOrEmpty, createPanel, renderFeatureEvidence, renderMetrics } from "./panels.js";
import { liveDashboard, operationsDashboard, optionsDashboard, paperDashboard } from "./snapshot.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";

export function renderPortfolio(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const operations = operationsDashboard(snapshot);
  const paper = paperDashboard(snapshot);
  const live = liveDashboard(snapshot);
  const options = optionsDashboard(snapshot);
  const positionCount = (operations?.positions.length ?? 0) + (paper?.positions.length ?? 0) + (live?.positions.length ?? 0);
  renderMetrics(summaryRoot, [
    ["Visible positions", String(positionCount), "Operations, PAPER, and controlled-LIVE internal ledgers"],
    ["Net attribution", operations === undefined ? "Unavailable" : `${operations.attribution.net_pnl} ${operations.currency}`, "Realized, unrealized, fee, dividend, and action movements"],
    ["Options reconciliation", options === undefined ? "Unavailable" : options.reconciliation.clean ? "Clean" : `${options.reconciliation.issues.length} issue(s)`, "BACKTEST / PAPER / LIVE declared books", options?.reconciliation.clean ? "good" : "warn"],
    ["Scenario points", String(options?.strategy.scenarios.length ?? 0), "Deterministic multi-leg expiry outcomes"],
  ]);
  const positions = createPanel("Positions and realized P&L", "Aggregated internal portfolio evidence. A browser view cannot assert broker synchronization; reconciliation evidence remains explicit.");
  const positionRows: string[][] = [];
  for (const item of operations?.positions ?? []) {
    positionRows.push(["OPERATIONS", item.instrument_id, item.quantity, item.average_cost, item.mark_price, item.realized_pnl]);
  }
  for (const item of paper?.positions ?? []) {
    positionRows.push(["PAPER", item.instrument_id, item.quantity, item.average_cost, "—", item.realized_pnl]);
  }
  for (const item of live?.positions ?? []) {
    positionRows.push(["LIVE", item.instrument_id, item.quantity, item.average_cost, "—", item.realized_pnl]);
  }
  appendTableOrEmpty(positions, ["Source", "Instrument", "Quantity", "Average cost", "Mark", "Realized P&L"], positionRows, "No internal positions are present in the latest snapshots.");
  root.append(positions);

  const attribution = createPanel("P&L attribution", "Immutable accounting movements grouped by instrument and category.");
  appendTableOrEmpty(attribution, ["Instrument", "Category", "Amount"], (operations?.attribution.rows ?? []).map((row) => [
    row.instrument_id, displayName(row.category), `${row.amount} ${operations?.currency ?? ""}`,
  ]), "No attribution rows are available.");
  root.append(attribution);

  const fundLedgerPanel = createPanel(
    "Personal fund ledger and tax lots",
    "Trade-to-cash reconciliation, balanced double-entry accounting journals, realized/unrealized P&L, and FIFO/SpecId tax lot disposition (PORT-01)."
  );
  fundLedgerPanel.id = "fund-ledger-panel";
  appendAdvancedEvidenceRows(
    fundLedgerPanel,
    snapshot,
    context,
    "fund_ledger_statement",
    parseFundLedgerStatement,
    ["Lot ID / Journal", "Instrument", "Acquired (UTC)", "Quantity", "Cost Basis", "Realized P&L", "Disposition & State"],
    (statement) => statement.tax_lots.map((lot) => [
      lot.lot_id,
      lot.instrument_id,
      lot.acquired_at,
      lot.quantity,
      lot.cost_basis,
      statement.realized_pnl,
      `${lot.disposition}; balanced: ${statement.balanced}`,
    ]),
    "No typed fund-ledger statement is published."
  );
  root.append(fundLedgerPanel);

  const multiAssetPanel = createPanel(
    "Multi-asset lifecycle, exercise, roll and settlement",
    "Coordinate options roll calendars, automatic cash-settled/physical assignment, FX currency spot-forward hedging, and futures delivery windows (PORT-02)."
  );
  multiAssetPanel.id = "multi-asset-panel";
  appendAdvancedEvidenceRows(
    multiAssetPanel,
    snapshot,
    context,
    "multi_asset_expansion_plan",
    parseMultiAssetExpansionPlan,
    ["Plan ID", "Asset Class", "Lifecycle Action", "Target Date (UTC)", "Contract Quantity", "Est. Cash Impact", "Settlement State"],
    (plan) => plan.lifecycle_actions.map((action) => [
      plan.plan_id,
      plan.asset_class,
      `${action.action_kind} (${action.instrument_id})`,
      action.target_date,
      String(action.contract_quantity),
      action.estimated_cash_flow_usd,
      plan.operational_verdict,
    ]),
    "No typed multi-asset expansion plan is published."
  );
  root.append(multiAssetPanel);

  const statementReconPanel = createPanel(
    "Broker statement reconciliation",
    "Independent verification of broker cash balances and held positions against internal multi-currency ledger (PORT-01, OPS-04)."
  );
  statementReconPanel.id = "statement-recon-panel";
  appendAdvancedEvidenceRows(
    statementReconPanel,
    snapshot,
    context,
    "statement_reconciliation",
    parseStatementReconciliation,
    ["Account ID", "As-Of (UTC)", "Verdict", "Incidents", "Statement Hash", "Details"],
    (recon) => [
      [
        recon.account_id,
        recon.as_of,
        recon.clean ? "CLEAN" : "INCIDENTS DETECTED",
        String(recon.incident_count),
        recon.statement_csv_hash.slice(0, 16) + "...",
        recon.incidents.length === 0
          ? "All balances and positions verified"
          : recon.incidents.map((i) => i.incident_kind === "CASH_MISMATCH"
              ? `Cash diff: internal ${i.internal_balance} ${i.currency} vs broker ${i.broker_balance}`
              : `Position diff on ${i.instrument_id}: internal ${i.internal_quantity} vs broker ${i.broker_quantity}`
            ).join("; "),
      ]
    ],
    "No broker statement reconciliation artifact is published."
  );
  root.append(statementReconPanel);

  const fxPricingPanel = createPanel(
    "Deterministic FX valuation and snapshots",
    "Fresh spot, forward, and swap midpoints, bid/ask spreads, and source sequences (FX-01, DATA-07)."
  );
  fxPricingPanel.id = "fx-pricing-panel";
  appendAdvancedEvidenceRows(
    fxPricingPanel,
    snapshot,
    context,
    "fx_pricing_dashboard",
    parseFxPricingDashboard,
    ["Query ID", "Pair", "Product", "Value Date", "Midpoint", "Bid / Ask", "Spread (bps)", "Age", "Status"],
    (fx) => fx.evaluations.map((e) => [
      e.query_id,
      e.pair,
      e.product,
      e.value_date,
      e.midpoint,
      `${e.bid} / ${e.ask}`,
      e.spread_bps,
      `${e.age_seconds}s`,
      e.fresh ? "FRESH" : "STALE",
    ]),
    "No FX pricing dashboard artifact is published."
  );
  root.append(fxPricingPanel);

  if (options !== undefined) {
    const scenario = createPanel("Options scenario and book reconciliation", `Compared at ${formatTime(options.reconciliation.reconciled_at)} using independently fingerprinted exports.`);
    appendTableOrEmpty(scenario, ["Underlying", "Strategy P&L", "Leg detail"], options.strategy.scenarios.map((item) => [
      item.underlying_price, `${item.total_pnl} ${options.chain.currency}`, item.legs.map((leg) => `${leg.option_id}: ${leg.pnl}`).join(" | "),
    ]), "No scenario rows are available.");
    root.append(scenario);
  }
  root.append(renderFeatureEvidence(context, ["replay", "paper", "operations", "options", "execution-risk", "accounting"]));
}
