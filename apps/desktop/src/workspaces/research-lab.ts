// Research lab workspace.

import { parseCounterfactualScenario, parseDataRightsAndSemanticsReceipt, parseFeedSubstitutionParity, parseResearchHypothesis } from "../evidence/index.js";
import { appendAdvancedEvidenceRows } from "./advanced-evidence.js";
import { field, formatTime, shortHash } from "./format.js";
import { appendTableOrEmpty, createPanel, renderFeatureEvidence, renderMetrics } from "./panels.js";
import { optionsDashboard } from "./snapshot.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";
import { renderOptionsPayoffVisualizer } from "./visualizers.js";

export function renderResearchLab(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const options = optionsDashboard(snapshot);
  renderMetrics(summaryRoot, [
    ["Datasets", String(snapshot.datasets.length), "CSV and immutable Parquet receipts indexed with row and schema metadata"],
    ["Notebooks", String(snapshot.notebooks.length), "Inert Jupyter metadata; notebook code and outputs are never executed"],
    ["Experiments", String(snapshot.experiments.length), "Immutable experiment catalogue records"],
    ["Backtest artifacts", String(snapshot.backtests.length), "Reproducible completed runs"],
  ]);
  const datasets = createPanel("Dataset inventory", "Historical inputs currently available to deterministic research workflows, including verified Parquet receipts.");
  appendTableOrEmpty(datasets, ["Dataset", "Version / format", "Rows", "Columns", "Modified"], snapshot.datasets.map((dataset) => [
    dataset.dataset_id || dataset.name,
    [dataset.dataset_version, dataset.storage_format || "CSV"].filter(Boolean).join(" / "),
    String(dataset.rows), dataset.columns.join(", "), formatTime(dataset.modified_at),
  ]), "No indexed datasets are available.", (index) => context.onOpenArtifact(snapshot.datasets[index]?.name ?? ""));
  root.append(datasets);

  const notebooks = createPanel("Notebook inventory", "Jupyter notebooks are indexed as inert research evidence. The dashboard never executes cells, JavaScript outputs, or embedded HTML.");
  appendTableOrEmpty(notebooks, ["Notebook", "Format", "Cells", "Code", "Markdown", "Outputs", "Kernel / language", "Modified"], snapshot.notebooks.map((notebook) => [
    notebook.artifact, `nbformat ${notebook.nbformat}`, String(notebook.cell_count), String(notebook.code_cells), String(notebook.markdown_cells),
    String(notebook.output_count), [notebook.kernel, notebook.language].filter(Boolean).join(" / "), formatTime(notebook.modified_at),
  ]), "No Jupyter notebook evidence is currently indexed.", (index) => context.onOpenArtifact(snapshot.notebooks[index]?.artifact ?? ""));
  root.append(notebooks);

  const experiments = createPanel("Experiment catalogue", "Content-addressed runs and linked output identities.");
  appendTableOrEmpty(experiments, ["Experiment", "Run", "Artifact fingerprint", "Event output", "Source"], snapshot.experiments.map((record) => [
    field(record.data, "experiment_id"), field(record.data, "run_id"), shortHash(field(record.data, "artifact_fingerprint")),
    shortHash(field(record.data, "event_output_hash")), record.artifact,
  ]), "No experiment catalogue records are available.", (index) => context.onOpenArtifact(snapshot.experiments[index]?.artifact ?? ""));
  root.append(experiments);

  if (options !== undefined) {
    const chain = createPanel("Frozen option chain", `${options.chain.underlying_instrument_id} at ${options.chain.underlying_mark} ${options.chain.currency}; ${options.model_version}.`);
    appendTableOrEmpty(chain, ["Contract", "Right", "Strike", "Bid", "Ask", "IV", "Delta", "Gamma", "Theta"], options.analytics.map((item) => [
      item.option_id, item.right, item.strike, item.bid, item.ask, item.implied_volatility, item.delta, item.gamma, item.theta,
    ]), "No option analytics are available.");
    root.append(chain);
  }

  const hypothesesPanel = createPanel(
    "Hypothesis notebook",
    "Record expected mechanisms, horizons, universe, assumptions, failure criteria, and frozen evaluation plans before optimization (RES-01)."
  );
  hypothesesPanel.id = "hypotheses-panel";
  appendAdvancedEvidenceRows(
    hypothesesPanel,
    snapshot,
    context,
    "research_hypothesis",
    parseResearchHypothesis,
    ["Hypothesis ID", "Status", "Economic Mechanism", "Target Universe", "Horizon", "Frozen Evaluation Plan", "Falsification Criteria"],
    (hypothesis) => [[
      hypothesis.hypothesis_id,
      hypothesis.status,
      hypothesis.mechanism,
      hypothesis.universe.join(", "),
      `${hypothesis.evaluation_horizon.start_time} to ${hypothesis.evaluation_horizon.end_time} (${hypothesis.evaluation_horizon.holding_period})`,
      `${hypothesis.frozen_evaluation_plan.dataset_id}@${hypothesis.frozen_evaluation_plan.dataset_version}; ${hypothesis.frozen_evaluation_plan.slippage_bps} bps; ${hypothesis.frozen_evaluation_plan.fee_model}`,
      hypothesis.failure_criteria.join(" | "),
    ]],
    "No typed research hypotheses are published.",
  );
  root.append(hypothesesPanel);

  const qualityPanel = createPanel(
    "Data quality console",
    "Inspect gaps, schema stability, receipts, and affected-run lookups; quarantined inputs cannot enter research (DATA-01)."
  );
  qualityPanel.id = "data-quality-console";
  const qualityRows = snapshot.datasets.map((dataset) => [
    dataset.dataset_id || dataset.name,
    dataset.storage_format || "CSV",
    String(dataset.rows),
    "Not evaluated by a published quality report",
    dataset.content_sha256 ? "Storage receipt indexed" : "Metadata only",
    `${snapshot.backtests.length} indexed run(s)`,
  ]);
  appendTableOrEmpty(
    qualityPanel,
    ["Dataset", "Format", "Row Count", "Continuity & Gaps", "Schema & Receipt", "Affected Runs"],
    qualityRows,
    "No dataset quality telemetry available.",
  );
  root.append(qualityPanel);

  const feedSubstitutionPanel = createPanel(
    "Deterministic feed substitution and parity verification",
    "Audit secondary or substitute market feeds against primary reference data, enforcing timestamp tolerance, quote continuity, and fixed-point basis drift (DATA-06)."
  );
  feedSubstitutionPanel.id = "feed-substitution-panel";
  appendAdvancedEvidenceRows(
    feedSubstitutionPanel,
    snapshot,
    context,
    "feed_substitution_parity",
    parseFeedSubstitutionParity,
    ["Primary Feed", "Candidate Feed", "Alignment Window", "Tolerance (ms)", "Max Drift (bps)", "Parity Disposition", "Evidence Receipt"],
    (parity) => [[
      parity.primary_provider,
      parity.candidate_provider,
      `${parity.sample_start} to ${parity.sample_end}`,
      `${parity.timestamp_variance_micros_p99 / 1000} ms p99`,
      parity.symbol_match_pct,
      parity.parity_disposition,
      parity.adjustment_parity_verified ? "Adjustment parity verified" : "Adjustment parity not verified",
    ]],
    "No typed feed parity evaluation is published."
  );
  root.append(feedSubstitutionPanel);

  const inputCorrectionPanel = createPanel(
    "Input correction and data rights ledger",
    "Verification receipts recording market data provider licenses, redistribution entitlements, corporate-action adjustment semantics, and affected lineage (DUR-03, DATA-01, DATA-03)."
  );
  inputCorrectionPanel.id = "input-correction-panel";
  appendAdvancedEvidenceRows(
    inputCorrectionPanel,
    snapshot,
    context,
    "data_rights_and_semantics_receipt",
    parseDataRightsAndSemanticsReceipt,
    ["Receipt ID", "Provider", "Dataset", "License Tier", "Redistributable", "Corporate Action Policy", "Semantic Parity", "Verified At"],
    (receipt) => [[
      receipt.receipt_id,
      receipt.provider_id,
      receipt.dataset_id,
      receipt.license_tier,
      receipt.redistribution_permitted ? "YES" : "NO",
      receipt.corporate_action_policy,
      `${receipt.semantic_parity_score_bps} bps`,
      receipt.verified_at,
    ]],
    "No typed data rights and semantics receipt is published."
  );
  root.append(inputCorrectionPanel);

  const counterfactualPanel = createPanel(
    "Operator-attested counterfactual result comparison",
    "Compare caller-supplied baseline and intervention results without mutating production history; this view does not execute or prove a replay (DUR-02)."
  );
  counterfactualPanel.id = "counterfactual-panel";
  appendAdvancedEvidenceRows(
    counterfactualPanel,
    snapshot,
    context,
    "counterfactual_scenario",
    parseCounterfactualScenario,
    ["Scenario ID", "Baseline Run", "Seed", "Interventions", "Divergence Event", "P&L Delta USD", "Max DD Delta (bps)", "Risk Rejections Delta"],
    (scenario) => [[
      scenario.scenario_id,
      scenario.baseline_run_id,
      String(scenario.seed),
      scenario.interventions.map((iv) => `${iv.intervention_type}: ${iv.parameter_name} (${iv.baseline_value} -> ${iv.counterfactual_value})`).join(" | "),
      scenario.divergence_event_id,
      scenario.delta_metrics.pnl_delta_usd,
      `${scenario.delta_metrics.max_drawdown_delta_bps} bps`,
      String(scenario.delta_metrics.risk_rejection_count_delta),
    ]],
    "No typed operator-attested counterfactual result is published."
  );
  root.append(counterfactualPanel);
  const payoffVisualizer = renderOptionsPayoffVisualizer(options);
  if (payoffVisualizer !== undefined) root.append(payoffVisualizer);

  root.append(renderFeatureEvidence(context, ["market-data", "research", "options"]));
}
