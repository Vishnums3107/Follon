// Backtest explorer workspace.

import { parseExperimentLineage, parsePortfolioExperiment, parseRobustnessEvaluation } from "../evidence/index.js";
import { appendAdvancedEvidenceRows } from "./advanced-evidence.js";
import { field, isRecord, record, shortHash, text } from "./format.js";
import { appendDefinition, appendTableOrEmpty, createPanel, renderFeatureEvidence, renderMetrics } from "./panels.js";
import { optionsDashboard } from "./snapshot.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";

export function renderBacktestExplorer(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const fills = snapshot.events.filter((item) =>
    field(item.data, "event_type") === "execution.fill.v1" &&
    (field(item.data, "source") === "simulator" || field(item.data, "actor") === "simulator")
  );
  const taggedExperiments = snapshot.experiments.filter((item) => {
    const tags = record(item.data.tags);
    return Boolean(field(tags, "regime") || field(tags, "sensitivity") || field(tags, "scenario"));
  });
  renderMetrics(summaryRoot, [
    ["Completed runs", String(snapshot.backtests.length), "Immutable backtest artifacts"],
    ["Recorded fills", String(fills.length), "Canonical simulated execution.fill.v1 evidence"],
    ["Experiment records", String(snapshot.experiments.length), `${taggedExperiments.length} with regime or sensitivity dimensions`],
    ["Completion manifests", String(snapshot.manifests.length), "SHA-256 publication records"],
  ]);
  const runs = createPanel("Run comparison", "Compare performance, accounting, and provenance without rerunning or mutating results.");
  appendTableOrEmpty(runs, ["Artifact", "Strategy", "Dataset", "Trades", "Net P&L", "Return bps", "Max drawdown", "Fingerprint"], snapshot.backtests.map((run) => {
    const dataset = record(run.specification.dataset);
    return [run.artifact, field(run.specification, "strategy_version") || shortHash(field(run.specification, "strategy_bundle_hash")),
      field(dataset, "dataset_id") || "Legacy artifact", field(run.performance, "trade_count"), field(run.performance, "net_pnl") || field(run.report, "realized_pnl"),
      field(run.performance, "return_bps"), field(run.performance, "max_drawdown_bps"), shortHash(text(run.artifact_fingerprint))];
  }), "No backtest result artifacts are available.", (index) => context.onOpenArtifact(snapshot.backtests[index]?.artifact ?? ""));
  root.append(runs);

  // Schema-3 artifacts carry the complete economics; older runs are not
  // back-filled, so they are simply absent from this table.
  const advancedRuns = snapshot.backtests.filter((run) => isRecord(run.advanced_account));
  const economics = createPanel(
    "Advanced-account economics",
    "Multi-currency cash, long and short positions, margin, financing, and attributed charges, carried inside each schema-3 artifact. These supersede the single-currency run comparison figures.",
  );
  economics.id = "advanced-account-economics";
  appendTableOrEmpty(economics, ["Artifact", "Base currency", "Net liquidation", "Initial margin", "Maintenance margin", "Excess liquidity", "Margin call", "Realized P&L", "Unrealized P&L", "Execution charges", "Financing charges"], advancedRuns.map((run) => {
    const advanced = record(run.advanced_account);
    const margin = record(advanced.margin);
    return [run.artifact, field(margin, "base_currency"), field(margin, "net_liquidation_value"), field(margin, "initial_margin"),
      field(margin, "maintenance_margin"), field(margin, "excess_liquidity"), field(margin, "margin_call"), field(advanced, "realized_pnl"),
      field(advanced, "unrealized_pnl"), field(advanced, "execution_charges"), field(advanced, "financing_charges")];
  }), "No indexed run carries advanced-account economics; they appear in artifact schema 3.", (index) => context.onOpenArtifact(advancedRuns[index]?.artifact ?? ""));
  root.append(economics);

  const trades = createPanel("Trade evidence", "Inspect each canonical simulated execution rather than relying only on aggregate trade counts.");
  appendTableOrEmpty(trades, ["Time", "Execution", "Order", "Instrument", "Side", "Quantity", "Price", "Fee", "Source"], fills.map((item) => {
    const payload = record(item.data.payload);
    return [
      field(payload, "executed_at") || field(item.data, "event_time"),
      field(payload, "execution_id"),
      field(payload, "order_id"),
      field(payload, "instrument_id") || field(item.data, "instrument_id"),
      field(payload, "side"),
      field(payload, "quantity"),
      field(payload, "price"),
      field(payload, "fee"),
      item.artifact,
    ];
  }), "No canonical fill events are available for the indexed backtests.", (index) => context.onOpenArtifact(fills[index]?.artifact ?? ""));
  root.append(trades);

  const dimensions = createPanel("Regime and sensitivity dimensions", "Experiment tags remain attached to immutable run identities; missing tags are reported explicitly rather than inferred from results.");
  appendTableOrEmpty(dimensions, ["Experiment", "Run", "Regime", "Sensitivity / scenario", "Other dimensions", "Specification", "Source"], snapshot.experiments.map((item) => {
    const tags = record(item.data.tags);
    const otherTags = Object.entries(tags)
      .filter(([key]) => key !== "regime" && key !== "sensitivity" && key !== "scenario")
      .map(([key, value]) => `${key}=${text(value)}`)
      .join(" | ");
    return [
      field(item.data, "experiment_id"),
      field(item.data, "run_id"),
      field(tags, "regime") || "Not tagged",
      field(tags, "sensitivity") || field(tags, "scenario") || "Not tagged",
      otherTags || "None",
      shortHash(field(item.data, "specification_fingerprint")),
      item.artifact,
    ];
  }), "No experiment records are available; regime and sensitivity comparisons require tagged immutable runs.", (index) => context.onOpenArtifact(snapshot.experiments[index]?.artifact ?? ""));
  root.append(dimensions);

  const executionModel = createPanel("Execution realism model", "Every run binds these deterministic assumptions through its immutable configuration fingerprint.");
  appendDefinition(executionModel, [
    ["Quoted spread", "Buys pay and sells concede half of the configured full spread"],
    ["Slippage", "Configured basis points are applied unfavourably after half-spread"],
    ["Tick grid", "The spread-and-slippage price is rounded onto the instrument's tick grid against the trader: buys up, sells down"],
    ["Limit protection", "The final grid price can never violate the order limit"],
    ["Increment changes", "A lot or tick change that leaves a working order off the new increments stops the replay rather than filling it"],
    ["Latency", "A configured number of complete market bars must pass before fill eligibility"],
    ["Partial fills", "An optional per-bar quantity cap persists remaining quantity as a working order"],
    ["Trading halts", "Version-controlled venue or instrument halt windows block strategy evaluation"],
    ["Survivorship", "Every replay bar must belong to an effective-dated point-in-time universe interval"],
    ["Short and borrow", "Explicit shortability, borrow availability/recalls, and daily financing accrue without look-ahead"],
    ["Delistings", "A versioned terminal settlement closes long or short positions and preserves realized P&L evidence"],
    ["Capital", "Fresh FX and portfolio-wide initial margin are evaluated before an advanced-account fill is committed"],
  ]);
  root.append(executionModel);

  const manifests = createPanel("Publication manifests", "A manifest binds the artifact, events, report, configuration, and specification hashes.");
  appendTableOrEmpty(manifests, ["Manifest", "Artifact hash", "Events hash", "Report hash", "Configuration"], snapshot.manifests.map((item) => [
    item.artifact, shortHash(field(item.data, "artifact_sha256")), shortHash(field(item.data, "events_sha256")),
    shortHash(field(item.data, "report_sha256")), shortHash(field(item.data, "configuration_hash")),
  ]), "No completion manifests are available.", (index) => context.onOpenArtifact(snapshot.manifests[index]?.artifact ?? ""));
  root.append(manifests);

  const options = optionsDashboard(snapshot);
  if (options !== undefined) {
    const scenarios = createPanel("Options expiry scenarios", `${options.strategy.strategy_id} / ${options.strategy.strategy_version}; deterministic European expiry payoff.`);
    appendTableOrEmpty(scenarios, ["Underlying", "Total P&L", "Legs"], options.strategy.scenarios.map((scenario) => [
      scenario.underlying_price, scenario.total_pnl, scenario.legs.map((leg) => `${leg.leg_id}: ${leg.pnl}`).join(" | "),
    ]), "No option scenarios are available.");
    root.append(scenarios);
  }

  const failedIdeaPanel = createPanel(
    "Experiment graph and failed-idea memory",
    "Retain rejected hypotheses, parameter candidates, and branch history; selecting a winner never erases the trials that produced it (RES-04)."
  );
  failedIdeaPanel.id = "failed-idea-memory";
  appendAdvancedEvidenceRows(
    failedIdeaPanel,
    snapshot,
    context,
    "experiment_lineage",
    parseExperimentLineage,
    ["Trial ID & Parameters", "Specification Hash", "Return (bps)", "Max Drawdown", "Disposition", "Failure / Promotion Rationale"],
    (lineage) => lineage.candidate_trials.map((trial) => [
      trial.trial_id,
      shortHash(trial.specification_hash),
      trial.return_bps,
      trial.max_drawdown_bps,
      trial.disposition,
      lineage.rejection_reasons.find((reason) => reason.trial_id === trial.trial_id)?.reason ?? "No disposition rationale published",
    ]),
    "No typed experiment-lineage record is published.",
  );
  root.append(failedIdeaPanel);

  const robustnessPanel = createPanel(
    "Robustness laboratory",
    "Held-out evaluations, walk-forward windows, leakage verification, parameter neighborhood stability, and cost stress shocks (RES-05)."
  );
  robustnessPanel.id = "robustness-lab-panel";
  appendAdvancedEvidenceRows(
    robustnessPanel,
    snapshot,
    context,
    "robustness_evaluation",
    parseRobustnessEvaluation,
    ["Dimension", "Configuration & Window", "In-Sample", "Out-of-Sample", "Drawdown", "Robustness Finding"],
    (evaluation) => evaluation.walk_forward_windows.map((window) => [
      window.window_id,
      `${window.in_sample_start} to ${window.in_sample_end} -> ${window.out_of_sample_start} to ${window.out_of_sample_end}`,
      `${window.in_sample_return_bps} bps`,
      `${window.out_of_sample_return_bps} bps`,
      `${window.max_drawdown_bps} bps`,
      `${evaluation.disposition}; leakage violations ${evaluation.leakage_checks.quarantine_violations}`,
    ]),
    "No typed robustness evaluation is published."
  );
  root.append(robustnessPanel);

  const portfolioExpPanel = createPanel(
    "Portfolio experiment engine",
    "Simulate concurrent strategies sharing cash, order contention, fees, turnover caps, and portfolio allocation rules (RES-06)."
  );
  portfolioExpPanel.id = "portfolio-experiment-panel";
  appendAdvancedEvidenceRows(
    portfolioExpPanel,
    snapshot,
    context,
    "portfolio_experiment",
    parsePortfolioExperiment,
    ["Experiment ID", "Strategies Joined", "Allocated Capital", "Combined Return", "Drawdown", "Diversification Ratio", "Order Contention"],
    (experiment) => [[
      experiment.experiment_id,
      experiment.strategies.map((strategy) => `${strategy.strategy_id}@${strategy.strategy_version} (${strategy.target_weight_bps} bps)`).join(" + "),
      `${experiment.allocated_cash} ${experiment.currency}`,
      `${experiment.joint_performance.combined_return_bps} bps`,
      `${experiment.joint_performance.combined_max_drawdown_bps} bps`,
      `${experiment.joint_performance.diversification_ratio_bps} bps`,
      `${experiment.order_contention_events} event(s)`,
    ]],
    "No typed multi-strategy portfolio experiment is published."
  );
  root.append(portfolioExpPanel);

  root.append(renderFeatureEvidence(context, ["research", "replay", "options"]));
}
