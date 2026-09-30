// Strategy studio workspace.

import { parseAdversarialEvaluation, parseAssistantEvidence, parseAutomationMandate, parseChampionChallengerEvaluation, parseResearchJob, parseRobustnessEvaluation, parseStrategyCapsuleManifest, parseStrategyCompositionSpec } from "../evidence/index.js";
import { appendAdvancedEvidenceRows } from "./advanced-evidence.js";
import { shortHash, strategyIdentityRows } from "./format.js";
import { appendDefinition, appendTableOrEmpty, createPanel, renderFeatureEvidence, renderMetrics } from "./panels.js";
import { operationsDashboard } from "./snapshot.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";

export function renderStrategyStudio(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const operations = operationsDashboard(snapshot);
  const identities = strategyIdentityRows(snapshot, operations);
  renderMetrics(summaryRoot, [
    ["Strategy identities", String(identities.length), "Versioned strategy and bundle combinations"],
    ["Configuration identities", String(new Set(identities.map((row) => row[3]).filter(Boolean)).size), "Exact source-bound configuration hashes"],
    ["Worker boundary", "Trusted simulation worker", "Environment clearing is not filesystem, network, or resource isolation", "warn"],
    ["Broker access", "No desktop adapter route", "A worker must still be treated as untrusted until sandboxed and gateway-authorized", "warn"],
  ]);
  const identityPanel = createPanel("Version and deployment identities", "Every strategy run binds source, configuration, dataset, engine, and event output.");
  appendTableOrEmpty(identityPanel, ["Strategy", "Version", "Bundle", "Configuration", "Dataset", "Engine / source"], identities, "No strategy identities are available.");
  root.append(identityPanel);
  const boundary = createPanel("Worker contract", "The browser does not execute strategy code. The current local process worker validates identity, but it is only approved for trusted simulation bundles until a resource and network sandbox is evidenced.");
  appendDefinition(boundary, [
    ["Input", "Immutable strategy context and normalized market bars"],
    ["Output", "One validated order intent or null"],
    ["Identity", "SHA-256 of strategy tree, SDK source, and Python runtime"],
    ["Environment", "Cleared; only an explicit non-secret SDK path may be supplied"],
    ["Sandbox status", "No filesystem/network/resource sandbox is represented by this desktop projection"],
    ["Forbidden", "Broker adapters, credentials, unverified dependencies, and browser execution"],
  ]);
  root.append(boundary);

  const compositionPanel = createPanel(
    "Strategy composition studio",
    "Declarative signals, sizing rules, entry/exit criteria, and portfolio constraints; code and visual views share one versioned spec (RES-02)."
  );
  compositionPanel.id = "strategy-composition-panel";
  appendAdvancedEvidenceRows(
    compositionPanel,
    snapshot,
    context,
    "strategy_composition_spec",
    parseStrategyCompositionSpec,
    ["Strategy ID / Version", "Signal Criteria", "Sizing Rule", "Entry Criteria", "Exit Criteria", "Portfolio Constraints", "Code & Visual Hashes"],
    (spec) => [
      [
        `${spec.strategy_id} (${spec.strategy_version})`,
        spec.signals.map((s) => `${s.signal_id}: ${s.condition} (weight ${s.weight_bps} bps)`).join(" | "),
        `${spec.sizing_rule.sizing_type} (${spec.sizing_rule.target_value})`,
        spec.entry_criteria.join(", "),
        spec.exit_criteria.join(", "),
        `Max Lev: ${spec.portfolio_constraints.max_leverage_bps} bps | Max Pos: ${spec.portfolio_constraints.max_single_position_bps} bps | Stop: ${spec.portfolio_constraints.stop_loss_pct}`,
        `Code: ${shortHash(spec.code_hash)} | Visual: ${shortHash(spec.visual_representation_hash)}`,
      ],
    ],
    "No typed strategy composition specifications are published."
  );
  root.append(compositionPanel);

  const copilotPanel = createPanel(
    "Read-only research copilot",
    "Evidence-grounded assistant; every explanation cites immutable hashes, and absent evidence triggers an explicit UNKNOWN (AI-01)."
  );
  copilotPanel.id = "research-copilot-panel";
  appendAdvancedEvidenceRows(
    copilotPanel,
    snapshot,
    context,
    "assistant_evidence",
    parseAssistantEvidence,
    ["Query / Topic", "Cited Evidence IDs", "Model & Template", "Disposition", "Explanation Summary"],
    (evidence) => [[
      evidence.query_id,
      evidence.retrieved_record_ids.join(", ") || "None",
      `${evidence.model_version} (${evidence.prompt_template_version})`,
      `${evidence.human_disposition}; uncertainty ${evidence.uncertainty_score_bps} bps`,
      evidence.generated_output,
    ]],
    "No typed research-assistant evidence is published.",
  );
  root.append(copilotPanel);

  const criticPanel = createPanel(
    "Strategy drafting assistant and critic",
    "Translate plain-language hypotheses into typed rules, propose falsification tests, and diagnose missing costs or data bias (AI-02, AI-03)."
  );
  criticPanel.id = "strategy-critic-panel";
  appendAdvancedEvidenceRows(
    criticPanel,
    snapshot,
    context,
    "assistant_evidence",
    parseAssistantEvidence,
    ["Analysis Scope", "Critic Finding", "Severity", "Proposed Falsification Test", "Status"],
    (evidence) => [[
      evidence.query_id,
      evidence.generated_output,
      `${evidence.uncertainty_score_bps} bps uncertainty`,
      evidence.tool_attempts.map((attempt) => `${attempt.tool_name}: ${attempt.status}`).join(" | ") || "No tool attempt",
      evidence.human_disposition,
    ]],
    "No typed strategy-critique evidence is published."
  );
  root.append(criticPanel);

  const schedulerPanel = createPanel(
    "Budgeted research scheduler",
    "Overnight automated experiment execution with CPU/time/spend limits, periodic checkpointing, and zero broker credentials (AI-04)."
  );
  schedulerPanel.id = "research-scheduler-panel";
  appendAdvancedEvidenceRows(
    schedulerPanel,
    snapshot,
    context,
    "automation_mandate",
    parseAutomationMandate,
    ["Mandate ID", "Owner", "Allowed Templates", "Resource Caps (CPU / RAM / Duration)", "Checkpointing", "Broker Boundary"],
    (mandate) => [[
      mandate.mandate_id,
      mandate.owner,
      mandate.allowed_tasks.join(", "),
      `${mandate.resource_limits.max_cpu_cores} cores / ${mandate.resource_limits.max_memory_mb} MB / ${mandate.resource_limits.max_duration_seconds} s`,
      `${mandate.cancellation_policy.checkpoint_interval_seconds}s; stop on first error: ${mandate.cancellation_policy.stop_on_first_error}`,
      mandate.broker_access_permitted ? "Broker access granted" : "Broker access prohibited",
    ]],
    "No typed research automation mandate is published."
  );
  appendAdvancedEvidenceRows(
    schedulerPanel,
    snapshot,
    context,
    "research_job",
    parseResearchJob,
    ["Job ID", "Strategy", "Dataset", "State", "Lease", "Failure reason"],
    (job) => [[
      job.job_id,
      `${job.strategy_id}@${job.strategy_version}`,
      `${job.dataset_id}@${job.dataset_version}`,
      `${job.state} (v${job.state_version})`,
      job.worker_lease === null ? "No worker lease" : `${job.worker_lease.worker_id} until ${job.worker_lease.expires_at}`,
      job.failure_reason ?? "",
    ]],
    "No typed research job receipts are published."
  );
  root.append(schedulerPanel);

  const championChallengerPanel = createPanel(
    "Champion vs challenger shadow evaluation",
    "Shadow-evaluate challenger strategy iterations against active champions, measuring drift, information ratio delta, and automated retirement triggers (RES-08)."
  );
  championChallengerPanel.id = "champion-challenger-panel";
  appendAdvancedEvidenceRows(
    championChallengerPanel,
    snapshot,
    context,
    "champion_challenger_evaluation",
    parseChampionChallengerEvaluation,
    ["Champion Strategy", "Challenger Candidate", "Window Start / End", "Champion Return (bps)", "Challenger Return (bps)", "Drift State", "Lifecycle Recommendation"],
    (evaluation) => [[
      evaluation.champion_strategy_id,
      evaluation.challenger_strategy_id,
      `${evaluation.evaluation_window_start} to ${evaluation.evaluation_window_end}`,
      String(evaluation.champion_return_bps),
      String(evaluation.challenger_return_bps),
      evaluation.drift_detected ? "Drift detected" : "No drift detected",
      evaluation.recommendation,
    ]],
    "No typed champion/challenger evaluation is published."
  );
  root.append(championChallengerPanel);

  const strategyInvalidationPanel = createPanel(
    "Strategy falsification and invalidation explorer",
    "Connect frozen hypothesis falsification conditions to synthetic stress injections, data quality changes, and model drift alerts without mutating historical records (RES-01, RES-04, RES-05, RES-08, AI-03)."
  );
  strategyInvalidationPanel.id = "strategy-invalidation-panel";
  appendAdvancedEvidenceRows(
    strategyInvalidationPanel,
    snapshot,
    context,
    "robustness_evaluation",
    parseRobustnessEvaluation,
    ["Strategy / Hypothesis", "Falsification Condition", "Stress Test Applied", "Observed Invalidation Margin", "Review Task Status"],
    (evaluation) => [[
      `${evaluation.hypothesis_id} (${evaluation.strategy_version})`,
      `Leakage violations: ${evaluation.leakage_checks.quarantine_violations}; cliff: ${evaluation.parameter_stability.degradation_cliff_detected}`,
      evaluation.cost_shocks.map((shock) => `${shock.slippage_multiplier}x slip / ${shock.fee_multiplier}x fee`).join(" | "),
      `Neighborhood variance ${evaluation.parameter_stability.neighborhood_variance_bps} bps`,
      evaluation.disposition,
    ]],
    "No typed robustness evaluation is published."
  );
  appendAdvancedEvidenceRows(
    strategyInvalidationPanel,
    snapshot,
    context,
    "adversarial_evaluation",
    parseAdversarialEvaluation,
    ["Evaluation ID", "Strategy", "Attested Probes Passed", "Composite Robustness", "Certification Status", "Blocking Failure Reasons", "Evaluated At"],
    (evaluation) => [[
      evaluation.evaluation_id,
      evaluation.strategy_version,
      `${evaluation.probes.filter((p) => p.passed).length}/${evaluation.probes.length} probes`,
      `${evaluation.composite_robustness_score_bps} bps`,
      evaluation.gate_passed ? "INPUTS_PASS" : "INPUTS_FAIL",
      evaluation.blocking_failure_reasons.join(" | ") || "None",
      evaluation.evaluated_at,
    ]],
    "No typed operator-attested adversarial evaluation is published."
  );
  root.append(strategyInvalidationPanel);

  const strategyCapsulePanel = createPanel(
    "Portable strategy capsule and reproducibility manifest",
    "Self-contained strategy archive with bundle hash, pinned dependencies, runtime target, and deterministic replay instruction (DUR-07)."
  );
  strategyCapsulePanel.id = "strategy-capsule-panel";
  appendAdvancedEvidenceRows(
    strategyCapsulePanel,
    snapshot,
    context,
    "strategy_capsule_manifest",
    parseStrategyCapsuleManifest,
    ["Capsule ID", "Strategy Version", "Bundle Hash", "Config Hash", "Lockfile Hash", "Runtime Target", "Export Disposition", "Replay Command"],
    (capsule) => [[
      capsule.capsule_id,
      `${capsule.strategy_id}@${capsule.strategy_version}`,
      shortHash(capsule.bundle_sha256),
      shortHash(capsule.configuration_sha256),
      shortHash(capsule.dependency_lockfile_sha256),
      capsule.runtime_target,
      capsule.export_disposition,
      capsule.replay_instruction_command,
    ]],
    "No typed strategy capsule manifest is published."
  );
  root.append(strategyCapsulePanel);

  root.append(renderFeatureEvidence(context, ["research", "replay"]));
}
