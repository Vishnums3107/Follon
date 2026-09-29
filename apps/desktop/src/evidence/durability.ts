// Decision reconstruction, counterfactual, snapshot, drill, compatibility and scanner evidence.

import { hasExactKeys, isCanonicalId, isDecimal, isHash, isNonNegativeInteger, isPositiveInteger, isUtcTimestamp } from "./core.js";

export type CausalNodeEvidence = Readonly<{
  node_id: string;
  event_type: string;
  actor: string;
  event_time: string;
  available_at: string;
  causation_id?: string | null;
  content_hash: string;
  summary: string;
}>;

export type CausalEdgeEvidence = Readonly<{
  from_node_id: string;
  to_node_id: string;
  relation: string;
}>;

export type DecisionReconstruction = Readonly<{
  reconstruction_schema_version: 1;
  reconstruction_id: string;
  target_event_id: string;
  target_entity_type: "fill" | "order_intent" | "risk_rejection" | "position" | "alert";
  causal_chain: readonly CausalNodeEvidence[];
  edges: readonly CausalEdgeEvidence[];
  configuration_hash: string;
  integrity_status: "VERIFIED" | "INCOMPLETE_CHAIN" | "TIMESTAMP_ANOMALY";
  verified_at: string;
}>;

export type CounterfactualIntervention = Readonly<{
  intervention_type: "RISK_COLLAR_ADJUSTMENT" | "NETWORK_LATENCY_INJECTION" | "DATA_BAR_CORRUPTION" | "VOLATILITY_SHOCK";
  parameter_name: string;
  baseline_value: string;
  counterfactual_value: string;
}>;

export type CounterfactualDeltaMetrics = Readonly<{
  fill_count_delta: number;
  pnl_delta_usd: string;
  max_drawdown_delta_bps: number;
  risk_rejection_count_delta: number;
}>;

export type CounterfactualScenario = Readonly<{
  scenario_schema_version: 1;
  scenario_id: string;
  baseline_run_id: string;
  seed: number;
  interventions: readonly CounterfactualIntervention[];
  delta_metrics: CounterfactualDeltaMetrics;
  divergence_event_id: string;
  created_at: string;
}>;

export type DataRightsAndSemanticsReceipt = Readonly<{
  receipt_schema_version: 1;
  receipt_id: string;
  provider_id: string;
  dataset_id: string;
  license_tier: "INTERNAL_RESEARCH_ONLY" | "COMMERCIAL_REPLAY" | "ENTERPRISE_REDISTRIBUTABLE";
  redistribution_permitted: boolean;
  corporate_action_policy: "RAW_SPLIT_AND_DIVIDEND_ADJUSTED" | "RAW_UNADJUSTED" | "SPLIT_ADJUSTED_ONLY";
  semantic_parity_score_bps: number;
  verified_at: string;
  expires_at: string;
}>;

export type WorkspaceSnapshotManifest = Readonly<{
  snapshot_schema_version: 1;
  manifest_id: string;
  as_of_time: string;
  created_at: string;
  content_hash: string;
  retained_event_count: number;
  source_event_count: number;
  event_window: Readonly<{
    window_kind: string;
    first_event_time: string | null;
    last_event_time: string | null;
  }>;
  active_accounts: readonly string[];
  positions_fingerprint: string;
  ledger_balance_fingerprint: string;
  diagnostics: ReadonlyArray<Readonly<{
    artifact: string;
    code: string;
    detail: string;
  }>>;
}>;

export type AttentionBudget = Readonly<{
  budget_schema_version: 1;
  budget_id: string;
  session_date: string;
  cognitive_load_score_bps: number;
  interruptions_per_hour: number;
  active_alarms_count: number;
  suppressed_duplicates_count: number;
  escalated_critical_tasks: readonly string[];
  budget_exhausted: boolean;
  calculated_at: string;
}>;

export type AdversarialProbeResult = Readonly<{
  probe_name: "LOOKAHEAD_LEAKAGE_PROBE" | "PRICE_JITTER_PROBE" | "TRANSACTION_COST_SHOCK" | "PARAMETER_CLIFF_PROBE" | "REGIME_STRESS_PROBE";
  probe_description: string;
  passed: boolean;
  degradation_bps: number;
  threshold_bps: number;
}>;

export type AdversarialEvaluation = Readonly<{
  adversarial_schema_version: 1;
  evaluation_id: string;
  strategy_version: string;
  probes: readonly AdversarialProbeResult[];
  composite_robustness_score_bps: number;
  gate_passed: boolean;
  blocking_failure_reasons: readonly string[];
  evaluated_at: string;
}>;

export type RecoveryDrillResult = Readonly<{
  drill_schema_version: 1;
  drill_id: string;
  scenario_name: string;
  injected_fault: "DISK_PRESSURE_ABRUPT_TERMINATION" | "DROPPED_BROKER_EXECUTION_ACK" | "CORRUPT_POSTGRES_CHECKPOINT" | "SPLIT_BRAIN_HOST_PARTITION";
  measured_rto_seconds: number;
  target_rto_seconds: number;
  measured_rpo_events_lost: number;
  target_rpo_events_lost: number;
  reconciliation_hash_matched: boolean;
  drill_passed: boolean;
  executed_at: string;
}>;

export type QualifiedCapability = Readonly<{
  capability_id: string;
  asset_class: string;
  qualification_state: "CERTIFIED" | "PROVISIONAL" | "REJECTED";
  measured_p99_latency_ms: number;
  max_supported_slices: number;
  reconciliation_accuracy_bps: number;
}>;

export type GatewayQualificationMatrix = Readonly<{
  matrix_schema_version: 1;
  matrix_id: string;
  environment: "PAPER" | "CONTROLLED_LIVE" | "SIMULATION";
  gateway_id: string;
  qualified_capabilities: readonly QualifiedCapability[];
  fencing_epoch: number;
  evaluated_at: string;
  expires_at: string;
}>;

export type CapitalAllocation = Readonly<{
  strategy_id: string;
  recommended_capital_usd: string;
  risk_budget_share_bps: number;
  marginal_risk_contribution_bps: number;
}>;

export type CapitalAllocationProposal = Readonly<{
  proposal_schema_version: 1;
  proposal_id: string;
  total_equity_usd: string;
  target_annual_volatility_bps: number;
  max_drawdown_limit_bps: number;
  allocations: readonly CapitalAllocation[];
  portfolio_diversification_ratio_bps: number;
  proposal_status: "RECOMMENDED" | "UNDER_REVIEW" | "RATIFIED" | "REJECTED";
  policy_version: string;
  proposed_at: string;
}>;

export type SchemaCompatibilityEntry = Readonly<{
  schema_name: string;
  current_version: number;
  oldest_supported_version: number;
  migration_status: "CURRENT" | "AUTOMATIC_UPGRADE" | "DEPRECATED";
}>;

export type CompatibilityMatrix = Readonly<{
  compatibility_schema_version: 1;
  matrix_id: string;
  engine_version: string;
  registered_schemas: readonly SchemaCompatibilityEntry[];
  backward_compatibility_verified: boolean;
  golden_corpus_size: number;
  verified_at: string;
}>;

export type MarketScannerCandidate = Readonly<{
  rank: number;
  instrument_id: string;
  symbol: string;
  close_price: string;
  momentum_score_bps: number;
  rsi_14: string;
  matched_conditions: readonly string[];
  rationale: string;
}>;

export type MarketScannerIndicator = Readonly<{
  column_id: string;
  name: string;
  timeframe: string;
  definition: string;
}>;

export type MarketScanner = Readonly<{
  scanner_schema_version: 1;
  scanner_id: string;
  universe_id: string;
  as_of_time: string;
  indicator_columns: readonly MarketScannerIndicator[];
  candidates: readonly MarketScannerCandidate[];
  quarantined_count: number;
  created_at: string;
}>;

export type NewsRevisionEntry = Readonly<{
  version_sequence: number;
  received_at: string;
  source_id: string;
  kind: "INITIAL_REPORT" | "SYNDICATED_DUPLICATE" | "CORRECTION" | "EDITORIAL_REVISION" | "MODEL_INTERPRETATION";
  headline: string;
  entity_confidence_bps: number;
  content_hash: string;
  supersedes_sequence: number | null;
}>;

export type NewsRevisionTimeline = Readonly<{
  revision_timeline_schema_version: 1;
  timeline_id: string;
  target_event_id: string;
  chain: readonly NewsRevisionEntry[];
  conflict_detected: boolean;
  created_at: string;
}>;

export type StrategyCompositionSignal = Readonly<{
  signal_id: string;
  indicator_ref: string;
  condition: string;
  weight_bps: number;
}>;

export type StrategyCompositionSizing = Readonly<{
  sizing_type: "FIXED_CAPITAL" | "VOLATILITY_TARGETED" | "RISK_PARITY" | "KELLY_FRACTION";
  target_value: string;
}>;

export type StrategyCompositionPortfolioConstraints = Readonly<{
  max_leverage_bps: number;
  max_single_position_bps: number;
  stop_loss_pct: string;
}>;

export type StrategyCompositionSpec = Readonly<{
  composition_schema_version: 1;
  composition_id: string;
  strategy_id: string;
  strategy_version: string;
  signals: readonly StrategyCompositionSignal[];
  sizing_rule: StrategyCompositionSizing;
  entry_criteria: readonly string[];
  exit_criteria: readonly string[];
  portfolio_constraints: StrategyCompositionPortfolioConstraints;
  code_hash: string;
  visual_representation_hash: string;
  created_at: string;
}>;

/** Parses a complete attributable decision provenance reconstruction (DUR-01). */
export function parseDecisionReconstruction(json: string): DecisionReconstruction {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Decision reconstruction is not valid JSON.");
  }
  if (!isDecisionReconstruction(value)) {
    throw new Error("Decision reconstruction does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses caller-supplied counterfactual comparison evidence (DUR-02). */
export function parseCounterfactualScenario(json: string): CounterfactualScenario {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Counterfactual scenario is not valid JSON.");
  }
  if (!isCounterfactualScenario(value)) {
    throw new Error("Counterfactual scenario does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a data rights and semantics verification receipt (DUR-03). */
export function parseDataRightsAndSemanticsReceipt(json: string): DataRightsAndSemanticsReceipt {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Data rights and semantics receipt is not valid JSON.");
  }
  if (!isDataRightsAndSemanticsReceipt(value)) {
    throw new Error("Data rights and semantics receipt does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses an immutable point-in-time workspace snapshot manifest (DUR-04). */
export function parseWorkspaceSnapshotManifest(json: string): WorkspaceSnapshotManifest {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Workspace snapshot manifest is not valid JSON.");
  }
  if (!isWorkspaceSnapshotManifest(value)) {
    throw new Error("Workspace snapshot manifest does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a solo-operator attention budget recording cognitive load and suppression state (DUR-05). */
export function parseAttentionBudget(json: string): AttentionBudget {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Attention budget is not valid JSON.");
  }
  if (!isAttentionBudget(value)) {
    throw new Error("Attention budget does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses certification of caller-supplied adversarial probe results (DUR-06). */
export function parseAdversarialEvaluation(json: string): AdversarialEvaluation {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Adversarial evaluation is not valid JSON.");
  }
  if (!isAdversarialEvaluation(value)) {
    throw new Error("Adversarial evaluation does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a game-day disaster recovery drill result (DUR-08). */
export function parseRecoveryDrillResult(json: string): RecoveryDrillResult {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Recovery drill result is not valid JSON.");
  }
  if (!isRecoveryDrillResult(value)) {
    throw new Error("Recovery drill result does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a granular gateway qualification capability matrix (DUR-10). */
export function parseGatewayQualificationMatrix(json: string): GatewayQualificationMatrix {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Gateway qualification matrix is not valid JSON.");
  }
  if (!isGatewayQualificationMatrix(value)) {
    throw new Error("Gateway qualification matrix does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a council-evaluated capital allocation proposal (DUR-11). */
export function parseCapitalAllocationProposal(json: string): CapitalAllocationProposal {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Capital allocation proposal is not valid JSON.");
  }
  if (!isCapitalAllocationProposal(value)) {
    throw new Error("Capital allocation proposal does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a multi-year schema version compatibility matrix (DUR-12). */
export function parseCompatibilityMatrix(json: string): CompatibilityMatrix {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Compatibility matrix is not valid JSON.");
  }
  if (!isCompatibilityMatrix(value)) {
    throw new Error("Compatibility matrix does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses an explainable market scanner result record (SOLO-04). */
export function parseMarketScanner(json: string): MarketScanner {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Market scanner is not valid JSON.");
  }
  if (!isMarketScanner(value)) {
    throw new Error("Market scanner does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a news revision and novelty timeline record (DATA-03). */
export function parseNewsRevisionTimeline(json: string): NewsRevisionTimeline {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("News revision timeline is not valid JSON.");
  }
  if (!isNewsRevisionTimeline(value)) {
    throw new Error("News revision timeline does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a strategy composition specification record (RES-02). */
export function parseStrategyCompositionSpec(json: string): StrategyCompositionSpec {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Strategy composition specification is not valid JSON.");
  }
  if (!isStrategyCompositionSpec(value)) {
    throw new Error("Strategy composition specification does not match the v1 evidence contract.");
  }
  return value;
}

function isCausalNodeEvidence(value: unknown): value is CausalNodeEvidence {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const val = value as Record<string, unknown>;
  if (!isCanonicalId(val.node_id) || typeof val.event_type !== "string" || typeof val.actor !== "string") return false;
  if (!isUtcTimestamp(val.event_time) || !isUtcTimestamp(val.available_at) || !isHash(val.content_hash) || typeof val.summary !== "string") return false;
  if ("causation_id" in val && val.causation_id !== null && !isCanonicalId(val.causation_id)) return false;
  const keys = Object.keys(val);
  const allowed = ["node_id", "event_type", "actor", "event_time", "available_at", "causation_id", "content_hash", "summary"];
  return keys.every((k) => allowed.includes(k)) && ["node_id", "event_type", "actor", "event_time", "available_at", "content_hash", "summary"].every((k) => k in val);
}

function isCausalEdgeEvidence(value: unknown): value is CausalEdgeEvidence {
  if (!hasExactKeys(value, ["from_node_id", "relation", "to_node_id"])) return false;
  const val = value as Record<string, unknown>;
  return isCanonicalId(val.from_node_id) && isCanonicalId(val.to_node_id) && typeof val.relation === "string";
}

function isDecisionReconstruction(value: unknown): value is DecisionReconstruction {
  if (!hasExactKeys(value, [
    "causal_chain",
    "configuration_hash",
    "edges",
    "integrity_status",
    "reconstruction_id",
    "reconstruction_schema_version",
    "target_entity_type",
    "target_event_id",
    "verified_at",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.reconstruction_schema_version !== 1 || !isCanonicalId(c.reconstruction_id) || typeof c.target_event_id !== "string") return false;
  if (!["fill", "order_intent", "risk_rejection", "position", "alert"].includes(String(c.target_entity_type))) return false;
  if (!Array.isArray(c.causal_chain) || !Array.isArray(c.edges)) return false;
  if (!c.causal_chain.every(isCausalNodeEvidence) || !c.edges.every(isCausalEdgeEvidence)) return false;
  if (typeof c.configuration_hash !== "string") return false;
  if (!["VERIFIED", "INCOMPLETE_CHAIN", "TIMESTAMP_ANOMALY"].includes(String(c.integrity_status))) return false;
  return isUtcTimestamp(c.verified_at);
}

function isCounterfactualScenario(value: unknown): value is CounterfactualScenario {
  if (!hasExactKeys(value, [
    "baseline_run_id",
    "created_at",
    "delta_metrics",
    "divergence_event_id",
    "interventions",
    "scenario_id",
    "scenario_schema_version",
    "seed",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.scenario_schema_version !== 1 || !isCanonicalId(c.scenario_id) || typeof c.baseline_run_id !== "string" || !Number.isSafeInteger(c.seed)) return false;
  if (!Array.isArray(c.interventions) || c.interventions.length === 0) return false;
  for (const iv of c.interventions) {
    if (!hasExactKeys(iv, ["baseline_value", "counterfactual_value", "intervention_type", "parameter_name"])) return false;
    const it = iv as Record<string, unknown>;
    if (!["RISK_COLLAR_ADJUSTMENT", "NETWORK_LATENCY_INJECTION", "DATA_BAR_CORRUPTION", "VOLATILITY_SHOCK"].includes(String(it.intervention_type))) return false;
    if (typeof it.parameter_name !== "string" || typeof it.baseline_value !== "string" || typeof it.counterfactual_value !== "string") return false;
  }
  if (!hasExactKeys(c.delta_metrics, ["fill_count_delta", "max_drawdown_delta_bps", "pnl_delta_usd", "risk_rejection_count_delta"])) return false;
  const dm = c.delta_metrics as Record<string, unknown>;
  if (!Number.isSafeInteger(dm.fill_count_delta) || !isDecimal(dm.pnl_delta_usd) || !Number.isSafeInteger(dm.max_drawdown_delta_bps) || !Number.isSafeInteger(dm.risk_rejection_count_delta)) return false;
  if (typeof c.divergence_event_id !== "string" || !isUtcTimestamp(c.created_at)) return false;
  return true;
}

function isDataRightsAndSemanticsReceipt(value: unknown): value is DataRightsAndSemanticsReceipt {
  if (!hasExactKeys(value, [
    "corporate_action_policy",
    "dataset_id",
    "expires_at",
    "license_tier",
    "provider_id",
    "receipt_id",
    "receipt_schema_version",
    "redistribution_permitted",
    "semantic_parity_score_bps",
    "verified_at",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.receipt_schema_version !== 1 || !isCanonicalId(c.receipt_id) || typeof c.provider_id !== "string" || typeof c.dataset_id !== "string") return false;
  if (!["INTERNAL_RESEARCH_ONLY", "COMMERCIAL_REPLAY", "ENTERPRISE_REDISTRIBUTABLE"].includes(String(c.license_tier))) return false;
  if (typeof c.redistribution_permitted !== "boolean") return false;
  if (!["RAW_SPLIT_AND_DIVIDEND_ADJUSTED", "RAW_UNADJUSTED", "SPLIT_ADJUSTED_ONLY"].includes(String(c.corporate_action_policy))) return false;
  if (!isNonNegativeInteger(c.semantic_parity_score_bps) || c.semantic_parity_score_bps > 10000) return false;
  return isUtcTimestamp(c.verified_at) && isUtcTimestamp(c.expires_at);
}

function isWorkspaceSnapshotManifest(value: unknown): value is WorkspaceSnapshotManifest {
  if (!hasExactKeys(value, [
    "active_accounts",
    "as_of_time",
    "content_hash",
    "created_at",
    "diagnostics",
    "event_window",
    "ledger_balance_fingerprint",
    "manifest_id",
    "positions_fingerprint",
    "retained_event_count",
    "snapshot_schema_version",
    "source_event_count",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.snapshot_schema_version !== 1 || !isCanonicalId(c.manifest_id) || !isUtcTimestamp(c.as_of_time) || !isUtcTimestamp(c.created_at)) return false;
  if (!isHash(c.content_hash) || !isNonNegativeInteger(c.retained_event_count) || !isNonNegativeInteger(c.source_event_count)) return false;
  if (!hasExactKeys(c.event_window, ["first_event_time", "last_event_time", "window_kind"])) return false;
  const ew = c.event_window as Record<string, unknown>;
  if (typeof ew.window_kind !== "string") return false;
  if (ew.first_event_time !== null && !isUtcTimestamp(ew.first_event_time)) return false;
  if (ew.last_event_time !== null && !isUtcTimestamp(ew.last_event_time)) return false;
  if (!Array.isArray(c.active_accounts) || !c.active_accounts.every((a) => typeof a === "string")) return false;
  if (!isHash(c.positions_fingerprint) || !isHash(c.ledger_balance_fingerprint)) return false;
  if (!Array.isArray(c.diagnostics)) return false;
  for (const d of c.diagnostics) {
    if (!hasExactKeys(d, ["artifact", "code", "detail"])) return false;
    const diag = d as Record<string, unknown>;
    if (typeof diag.artifact !== "string" || typeof diag.code !== "string" || typeof diag.detail !== "string") return false;
  }
  return true;
}

function isAttentionBudget(value: unknown): value is AttentionBudget {
  if (!hasExactKeys(value, [
    "active_alarms_count",
    "budget_exhausted",
    "budget_id",
    "budget_schema_version",
    "calculated_at",
    "cognitive_load_score_bps",
    "escalated_critical_tasks",
    "interruptions_per_hour",
    "session_date",
    "suppressed_duplicates_count",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.budget_schema_version !== 1 || !isCanonicalId(c.budget_id)) return false;
  if (typeof c.session_date !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(c.session_date)) return false;
  if (!isNonNegativeInteger(c.cognitive_load_score_bps) || c.cognitive_load_score_bps > 10000) return false;
  if (typeof c.interruptions_per_hour !== "number" || c.interruptions_per_hour < 0) return false;
  if (!isNonNegativeInteger(c.active_alarms_count) || !isNonNegativeInteger(c.suppressed_duplicates_count)) return false;
  if (!Array.isArray(c.escalated_critical_tasks) || !c.escalated_critical_tasks.every((t) => typeof t === "string")) return false;
  if (typeof c.budget_exhausted !== "boolean" || !isUtcTimestamp(c.calculated_at)) return false;
  return true;
}

function isAdversarialEvaluation(value: unknown): value is AdversarialEvaluation {
  if (!hasExactKeys(value, [
    "adversarial_schema_version",
    "blocking_failure_reasons",
    "composite_robustness_score_bps",
    "evaluated_at",
    "evaluation_id",
    "gate_passed",
    "probes",
    "strategy_version",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.adversarial_schema_version !== 1 || !isCanonicalId(c.evaluation_id) || typeof c.strategy_version !== "string") return false;
  if (!Array.isArray(c.probes) || c.probes.length < 5) return false;
  for (const p of c.probes) {
    if (!hasExactKeys(p, ["degradation_bps", "passed", "probe_description", "probe_name", "threshold_bps"])) return false;
    const probe = p as Record<string, unknown>;
    if (!["LOOKAHEAD_LEAKAGE_PROBE", "PRICE_JITTER_PROBE", "TRANSACTION_COST_SHOCK", "PARAMETER_CLIFF_PROBE", "REGIME_STRESS_PROBE"].includes(String(probe.probe_name))) return false;
    if (typeof probe.probe_description !== "string" || typeof probe.passed !== "boolean") return false;
    if (!Number.isSafeInteger(probe.degradation_bps) || !Number.isSafeInteger(probe.threshold_bps)) return false;
  }
  if (!isNonNegativeInteger(c.composite_robustness_score_bps) || c.composite_robustness_score_bps > 10000) return false;
  if (typeof c.gate_passed !== "boolean" || !Array.isArray(c.blocking_failure_reasons) || !c.blocking_failure_reasons.every((r) => typeof r === "string")) return false;
  return isUtcTimestamp(c.evaluated_at);
}

function isRecoveryDrillResult(value: unknown): value is RecoveryDrillResult {
  if (!hasExactKeys(value, [
    "drill_id",
    "drill_passed",
    "drill_schema_version",
    "executed_at",
    "injected_fault",
    "measured_rpo_events_lost",
    "measured_rto_seconds",
    "reconciliation_hash_matched",
    "scenario_name",
    "target_rpo_events_lost",
    "target_rto_seconds",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.drill_schema_version !== 1 || !isCanonicalId(c.drill_id) || typeof c.scenario_name !== "string") return false;
  if (!["DISK_PRESSURE_ABRUPT_TERMINATION", "DROPPED_BROKER_EXECUTION_ACK", "CORRUPT_POSTGRES_CHECKPOINT", "SPLIT_BRAIN_HOST_PARTITION"].includes(String(c.injected_fault))) return false;
  if (!isNonNegativeInteger(c.measured_rto_seconds) || !isNonNegativeInteger(c.target_rto_seconds)) return false;
  if (!isNonNegativeInteger(c.measured_rpo_events_lost) || !isNonNegativeInteger(c.target_rpo_events_lost)) return false;
  if (typeof c.reconciliation_hash_matched !== "boolean" || typeof c.drill_passed !== "boolean") return false;
  return isUtcTimestamp(c.executed_at);
}

function isGatewayQualificationMatrix(value: unknown): value is GatewayQualificationMatrix {
  if (!hasExactKeys(value, [
    "environment",
    "evaluated_at",
    "expires_at",
    "fencing_epoch",
    "gateway_id",
    "matrix_id",
    "matrix_schema_version",
    "qualified_capabilities",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.matrix_schema_version !== 1 || !isCanonicalId(c.matrix_id) || !["PAPER", "CONTROLLED_LIVE", "SIMULATION"].includes(String(c.environment)) || typeof c.gateway_id !== "string") return false;
  if (!isPositiveInteger(c.fencing_epoch) || !isUtcTimestamp(c.evaluated_at) || !isUtcTimestamp(c.expires_at)) return false;
  if (!Array.isArray(c.qualified_capabilities) || c.qualified_capabilities.length === 0) return false;
  for (const q of c.qualified_capabilities) {
    if (!hasExactKeys(q, ["asset_class", "capability_id", "max_supported_slices", "measured_p99_latency_ms", "qualification_state", "reconciliation_accuracy_bps"])) return false;
    const cap = q as Record<string, unknown>;
    if (!isCanonicalId(cap.capability_id) || typeof cap.asset_class !== "string") return false;
    if (!["CERTIFIED", "PROVISIONAL", "REJECTED"].includes(String(cap.qualification_state))) return false;
    if (!isNonNegativeInteger(cap.measured_p99_latency_ms) || !isPositiveInteger(cap.max_supported_slices)) return false;
    if (!isNonNegativeInteger(cap.reconciliation_accuracy_bps) || cap.reconciliation_accuracy_bps > 10000) return false;
  }
  return true;
}

function isCapitalAllocationProposal(value: unknown): value is CapitalAllocationProposal {
  if (!hasExactKeys(value, [
    "allocations",
    "max_drawdown_limit_bps",
    "policy_version",
    "portfolio_diversification_ratio_bps",
    "proposal_id",
    "proposal_schema_version",
    "proposal_status",
    "proposed_at",
    "target_annual_volatility_bps",
    "total_equity_usd",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.proposal_schema_version !== 1 || !isCanonicalId(c.proposal_id) || !isDecimal(c.total_equity_usd)) return false;
  if (!isNonNegativeInteger(c.target_annual_volatility_bps) || !isNonNegativeInteger(c.max_drawdown_limit_bps) || c.max_drawdown_limit_bps > 10000) return false;
  if (!isNonNegativeInteger(c.portfolio_diversification_ratio_bps)) return false;
  if (!["RECOMMENDED", "UNDER_REVIEW", "RATIFIED", "REJECTED"].includes(String(c.proposal_status))) return false;
  if (typeof c.policy_version !== "string" || !isUtcTimestamp(c.proposed_at)) return false;
  if (!Array.isArray(c.allocations) || c.allocations.length === 0) return false;
  for (const a of c.allocations) {
    if (!hasExactKeys(a, ["marginal_risk_contribution_bps", "recommended_capital_usd", "risk_budget_share_bps", "strategy_id"])) return false;
    const alloc = a as Record<string, unknown>;
    if (!isCanonicalId(alloc.strategy_id) || !isDecimal(alloc.recommended_capital_usd)) return false;
    if (!isNonNegativeInteger(alloc.risk_budget_share_bps) || alloc.risk_budget_share_bps > 10000 || !Number.isSafeInteger(alloc.marginal_risk_contribution_bps)) return false;
  }
  return true;
}

function isCompatibilityMatrix(value: unknown): value is CompatibilityMatrix {
  if (!hasExactKeys(value, [
    "backward_compatibility_verified",
    "compatibility_schema_version",
    "engine_version",
    "golden_corpus_size",
    "matrix_id",
    "registered_schemas",
    "verified_at",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.compatibility_schema_version !== 1 || !isCanonicalId(c.matrix_id) || typeof c.engine_version !== "string") return false;
  if (typeof c.backward_compatibility_verified !== "boolean" || !isNonNegativeInteger(c.golden_corpus_size) || !isUtcTimestamp(c.verified_at)) return false;
  if (!Array.isArray(c.registered_schemas) || c.registered_schemas.length === 0) return false;
  for (const s of c.registered_schemas) {
    if (!hasExactKeys(s, ["current_version", "migration_status", "oldest_supported_version", "schema_name"])) return false;
    const sch = s as Record<string, unknown>;
    if (typeof sch.schema_name !== "string" || !isPositiveInteger(sch.current_version) || !isPositiveInteger(sch.oldest_supported_version)) return false;
    if (!["CURRENT", "AUTOMATIC_UPGRADE", "DEPRECATED"].includes(String(sch.migration_status))) return false;
  }
  return true;
}

function isMarketScanner(value: unknown): value is MarketScanner {
  if (!hasExactKeys(value, [
    "as_of_time",
    "candidates",
    "created_at",
    "indicator_columns",
    "quarantined_count",
    "scanner_id",
    "scanner_schema_version",
    "universe_id",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.scanner_schema_version !== 1 || !isCanonicalId(c.scanner_id) || typeof c.universe_id !== "string") return false;
  if (!isUtcTimestamp(c.as_of_time) || !isUtcTimestamp(c.created_at) || !isNonNegativeInteger(c.quarantined_count)) return false;
  if (!Array.isArray(c.indicator_columns) || c.indicator_columns.length === 0 || !Array.isArray(c.candidates)) return false;
  for (const col of c.indicator_columns) {
    if (!hasExactKeys(col, ["column_id", "definition", "name", "timeframe"])) return false;
    const cl = col as Record<string, unknown>;
    if (typeof cl.column_id !== "string" || typeof cl.name !== "string" || typeof cl.timeframe !== "string" || typeof cl.definition !== "string") return false;
  }
  for (const cand of c.candidates) {
    if (!hasExactKeys(cand, [
      "close_price",
      "instrument_id",
      "matched_conditions",
      "momentum_score_bps",
      "rank",
      "rationale",
      "rsi_14",
      "symbol",
    ])) return false;
    const cd = cand as Record<string, unknown>;
    if (!isPositiveInteger(cd.rank) || !isCanonicalId(cd.instrument_id) || typeof cd.symbol !== "string") return false;
    if (!isDecimal(cd.close_price) || !Number.isSafeInteger(cd.momentum_score_bps) || !isDecimal(cd.rsi_14)) return false;
    if (!Array.isArray(cd.matched_conditions) || typeof cd.rationale !== "string") return false;
  }
  return true;
}

function isNewsRevisionTimeline(value: unknown): value is NewsRevisionTimeline {
  if (!hasExactKeys(value, [
    "chain",
    "conflict_detected",
    "created_at",
    "revision_timeline_schema_version",
    "target_event_id",
    "timeline_id",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.revision_timeline_schema_version !== 1 || !isCanonicalId(c.timeline_id) || typeof c.target_event_id !== "string") return false;
  if (typeof c.conflict_detected !== "boolean" || !isUtcTimestamp(c.created_at) || !Array.isArray(c.chain) || c.chain.length === 0) return false;
  for (const item of c.chain) {
    if (!hasExactKeys(item, [
      "content_hash",
      "entity_confidence_bps",
      "headline",
      "kind",
      "received_at",
      "source_id",
      "supersedes_sequence",
      "version_sequence",
    ])) return false;
    const entry = item as Record<string, unknown>;
    if (!isPositiveInteger(entry.version_sequence) || !isUtcTimestamp(entry.received_at) || typeof entry.source_id !== "string") return false;
    if (!["INITIAL_REPORT", "SYNDICATED_DUPLICATE", "CORRECTION", "EDITORIAL_REVISION", "MODEL_INTERPRETATION"].includes(String(entry.kind))) return false;
    if (typeof entry.headline !== "string" || !isNonNegativeInteger(entry.entity_confidence_bps) || entry.entity_confidence_bps > 10000) return false;
    if (!isHash(entry.content_hash)) return false;
    if (entry.supersedes_sequence !== null && !isPositiveInteger(entry.supersedes_sequence)) return false;
  }
  return true;
}

function isStrategyCompositionSpec(value: unknown): value is StrategyCompositionSpec {
  if (!hasExactKeys(value, [
    "code_hash",
    "composition_id",
    "composition_schema_version",
    "created_at",
    "entry_criteria",
    "exit_criteria",
    "portfolio_constraints",
    "signals",
    "sizing_rule",
    "strategy_id",
    "strategy_version",
    "visual_representation_hash",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.composition_schema_version !== 1 || !isCanonicalId(c.composition_id) || !isCanonicalId(c.strategy_id)) return false;
  if (typeof c.strategy_version !== "string" || !isUtcTimestamp(c.created_at)) return false;
  if (!isHash(c.code_hash) || !isHash(c.visual_representation_hash)) return false;
  if (!Array.isArray(c.signals) || c.signals.length === 0 || !Array.isArray(c.entry_criteria) || !Array.isArray(c.exit_criteria)) return false;
  for (const sig of c.signals) {
    if (!hasExactKeys(sig, ["condition", "indicator_ref", "signal_id", "weight_bps"])) return false;
    const s = sig as Record<string, unknown>;
    if (!isCanonicalId(s.signal_id) || typeof s.indicator_ref !== "string" || typeof s.condition !== "string" || !isNonNegativeInteger(s.weight_bps)) return false;
  }
  if (!hasExactKeys(c.sizing_rule, ["sizing_type", "target_value"])) return false;
  const sr = c.sizing_rule as Record<string, unknown>;
  if (!["FIXED_CAPITAL", "VOLATILITY_TARGETED", "RISK_PARITY", "KELLY_FRACTION"].includes(String(sr.sizing_type)) || typeof sr.target_value !== "string") return false;
  if (!hasExactKeys(c.portfolio_constraints, ["max_leverage_bps", "max_single_position_bps", "stop_loss_pct"])) return false;
  const pc = c.portfolio_constraints as Record<string, unknown>;
  if (!isNonNegativeInteger(pc.max_leverage_bps) || !isNonNegativeInteger(pc.max_single_position_bps) || typeof pc.stop_loss_pct !== "string") return false;
  return true;
}
