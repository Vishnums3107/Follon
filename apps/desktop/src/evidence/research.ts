// Research, robustness, knowledge, mandate, passport, exposure and ledger evidence.

import { hasExactKeys, isCanonicalId, isDecimal, isHash, isNonNegativeInteger, isPositiveInteger, isUtcTimestamp } from "./core.js";

export type ResearchHypothesis = Readonly<{
  hypothesis_schema_version: 1;
  hypothesis_id: string;
  title: string;
  mechanism: string;
  universe: readonly string[];
  evaluation_horizon: Readonly<{
    start_time: string;
    end_time: string;
    holding_period: string;
  }>;
  assumptions: readonly string[];
  failure_criteria: readonly string[];
  frozen_evaluation_plan: Readonly<{
    dataset_id: string;
    dataset_version: string;
    dataset_hash: string;
    cost_model: string;
    slippage_bps: number;
    fee_model: string;
  }>;
  predecessor_id: string | null;
  status: "DRAFT" | "FROZEN" | "EVALUATING" | "CONFIRMED" | "REJECTED";
  created_at: string;
  frozen_at: string | null;
}>;

export type ExperimentLineage = Readonly<{
  lineage_schema_version: 1;
  lineage_id: string;
  hypothesis_id: string;
  parent_run_ids: readonly string[];
  input_fingerprints: ReadonlyArray<Readonly<{ name: string; fingerprint: string }>>;
  output_fingerprints: ReadonlyArray<Readonly<{ name: string; fingerprint: string }>>;
  candidate_trials: ReadonlyArray<Readonly<{
    trial_id: string;
    specification_hash: string;
    return_bps: string;
    max_drawdown_bps: string;
    disposition: "PROMOTED" | "REJECTED" | "BENCHMARK";
  }>>;
  failed_candidates_count: number;
  rejection_reasons: ReadonlyArray<Readonly<{ trial_id: string; reason: string }>>;
  created_at: string;
}>;

export type ResearchJob = Readonly<{
  job_schema_version: 1;
  job_id: string;
  idempotency_key: string;
  strategy_id: string;
  strategy_version: string;
  dataset_id: string;
  dataset_version: string;
  frozen_specification_hash: string;
  state_version: number;
  state: "QUEUED" | "RUNNING" | "COMPLETED" | "FAILED" | "CANCELLED";
  worker_lease: Readonly<{
    lease_id: string;
    worker_id: string;
    acquired_at: string;
    expires_at: string;
  }> | null;
  output_manifest_hash: string | null;
  failure_reason: string | null;
  created_at: string;
  updated_at: string;
}>;

export type AssistantEvidence = Readonly<{
  assistant_evidence_schema_version: 1;
  query_id: string;
  model_version: string;
  prompt_template_version: string;
  retrieved_record_ids: readonly string[];
  generated_output: string;
  tool_attempts: ReadonlyArray<Readonly<{
    tool_name: string;
    arguments_hash: string;
    status: "SUCCESS" | "FAILED" | "BLOCKED";
    evidence_id: string;
  }>>;
  uncertainty_score_bps: number;
  human_disposition: "ACCEPTED" | "REJECTED" | "AMENDED" | "PENDING";
  created_at: string;
}>;

export type RobustnessEvaluation = Readonly<{
  evaluation_schema_version: 1;
  evaluation_id: string;
  strategy_version: string;
  hypothesis_id: string;
  walk_forward_windows: ReadonlyArray<Readonly<{
    window_id: string;
    in_sample_start: string;
    in_sample_end: string;
    out_of_sample_start: string;
    out_of_sample_end: string;
    in_sample_return_bps: number;
    out_of_sample_return_bps: number;
    max_drawdown_bps: number;
  }>>;
  leakage_checks: Readonly<{
    survivorship_bias_verified: boolean;
    lookahead_bias_verified: boolean;
    corporate_action_adjusted: boolean;
    quarantine_violations: number;
  }>;
  parameter_stability: Readonly<{
    perturbation_percent: number;
    neighborhood_variance_bps: number;
    degradation_cliff_detected: boolean;
  }>;
  cost_shocks: ReadonlyArray<Readonly<{
    slippage_multiplier: string;
    fee_multiplier: string;
    stressed_return_bps: number;
  }>>;
  uncertainty_score_bps: number;
  disposition: "ROBUST" | "FRAGILE" | "LEAKAGE_DETECTED" | "DEGRADED";
  created_at: string;
}>;

export type PortfolioExperiment = Readonly<{
  portfolio_experiment_schema_version: 1;
  experiment_id: string;
  allocated_cash: string;
  currency: string;
  strategies: ReadonlyArray<Readonly<{
    strategy_id: string;
    strategy_version: string;
    target_weight_bps: number;
    realized_pnl: string;
    max_drawdown_bps: number;
  }>>;
  joint_constraints: Readonly<{
    max_gross_exposure_bps: number;
    max_single_instrument_bps: number;
    turnover_cap_daily_bps: number;
  }>;
  joint_performance: Readonly<{
    combined_return_bps: number;
    combined_max_drawdown_bps: number;
    diversification_ratio_bps: number;
    total_fee_drag: string;
  }>;
  order_contention_events: number;
  created_at: string;
}>;

export type KnowledgeSnapshot = Readonly<{
  knowledge_schema_version: 1;
  snapshot_id: string;
  as_of_time: string;
  entity_nodes: ReadonlyArray<Readonly<{
    entity_id: string;
    entity_type: "COMPANY" | "INSTRUMENT" | "FILING" | "HEADLINE" | "MACRO_EVENT";
    name: string;
    identifier: string;
  }>>;
  relationships: ReadonlyArray<Readonly<{
    source_entity_id: string;
    relation_type: string;
    target_entity_id: string;
    effective_time: string;
    provenance_hash: string;
  }>>;
  source_lineage_hashes: readonly string[];
  created_at: string;
}>;

export type EventExposureCalendar = Readonly<{
  calendar_schema_version: 1;
  calendar_id: string;
  as_of_time: string;
  timezone: string;
  scheduled_events: ReadonlyArray<Readonly<{
    event_id: string;
    instrument_id: string;
    category: "EARNINGS" | "DIVIDEND" | "STOCK_SPLIT" | "TRADING_HALT" | "OPTION_EXPIRY" | "SETTLEMENT";
    scheduled_time: string;
    status: "SCHEDULED" | "CONFIRMED" | "CANCELLED" | "COMPLETED";
    source_evidence: string;
  }>>;
  quarantined_events_count: number;
  created_at: string;
}>;

export type AutomationMandate = Readonly<{
  mandate_schema_version: 1;
  mandate_id: string;
  owner: string;
  allowed_tasks: readonly string[];
  resource_limits: Readonly<{
    max_cpu_cores: number;
    max_memory_mb: number;
    max_duration_seconds: number;
    max_storage_bytes: number;
  }>;
  cancellation_policy: Readonly<{
    stop_on_first_error: boolean;
    checkpoint_interval_seconds: number;
  }>;
  broker_access_permitted: false;
  created_at: string;
  expires_at: string;
}>;

export type OrderDecisionPassport = Readonly<{
  passport_schema_version: 1;
  passport_id: string;
  intent_id: string;
  order_id: string;
  instrument_id: string;
  signal_attribution: Readonly<{
    strategy_version: string;
    model_event_id: string;
    opportunity_description: string;
    signal_power_bps: number;
  }>;
  risk_evaluation: Readonly<{
    policy_version: string;
    approved: boolean;
    evaluated_limits: readonly string[];
    headroom_remaining_bps: number;
  }>;
  routing_plan: Readonly<{
    algorithm: string;
    allocated_slices_count: number;
    primary_venue: string;
    capability_version: string;
  }>;
  executions: ReadonlyArray<Readonly<{
    execution_id: string;
    venue: string;
    quantity: string;
    price: string;
    fee: string;
    executed_at: string;
  }>>;
  accounting_consequences: Readonly<{
    journal_entry_id: string;
    realized_pnl: string;
    cash_delta: string;
    position_after: string;
  }>;
  created_at: string;
}>;

export type ExposureGraph = Readonly<{
  exposure_schema_version: 1;
  graph_id: string;
  account_id: string;
  as_of_time: string;
  gross_exposure: string;
  net_exposure: string;
  factors: ReadonlyArray<Readonly<{
    factor_name: string;
    loading_bps: number;
    factor_variance_pct: string;
  }>>;
  sectors: ReadonlyArray<Readonly<{
    sector_name: string;
    exposure_usd: string;
    weight_bps: number;
  }>>;
  top_concentrations: ReadonlyArray<Readonly<{
    instrument_id: string;
    position_value: string;
    portfolio_pct: string;
  }>>;
  unreconciled_discrepancy: boolean;
  created_at: string;
}>;

export type FundLedgerStatement = Readonly<{
  ledger_schema_version: 1;
  statement_id: string;
  account_id: string;
  period_start: string;
  period_end: string;
  starting_cash: string;
  ending_cash: string;
  realized_pnl: string;
  unrealized_pnl: string;
  fee_totals: Readonly<{
    exchange_fees: string;
    brokerage_commissions: string;
    borrow_financing: string;
  }>;
  tax_lots: ReadonlyArray<Readonly<{
    lot_id: string;
    instrument_id: string;
    acquired_at: string;
    quantity: string;
    cost_basis: string;
    disposition: "OPEN" | "CLOSED_FIFO" | "CLOSED_SPECID";
  }>>;
  balanced: boolean;
  created_at: string;
}>;

/** Parses a frozen research hypothesis and evaluation plan. */
export function parseResearchHypothesis(json: string): ResearchHypothesis {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Research hypothesis is not valid JSON.");
  }
  if (!isResearchHypothesis(value)) {
    throw new Error("Research hypothesis does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses an experiment lineage and candidate trial memory record. */
export function parseExperimentLineage(json: string): ExperimentLineage {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Experiment lineage is not valid JSON.");
  }
  if (!isExperimentLineage(value)) {
    throw new Error("Experiment lineage does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses an idempotent typed research job record. */
export function parseResearchJob(json: string): ResearchJob {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Research job is not valid JSON.");
  }
  if (!isResearchJob(value)) {
    throw new Error("Research job does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a read-only research assistant evidence record. */
export function parseAssistantEvidence(json: string): AssistantEvidence {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Assistant evidence is not valid JSON.");
  }
  if (!isAssistantEvidence(value)) {
    throw new Error("Assistant evidence does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a deterministic robustness evaluation record (RES-05). */
export function parseRobustnessEvaluation(json: string): RobustnessEvaluation {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Robustness evaluation is not valid JSON.");
  }
  if (!isRobustnessEvaluation(value)) {
    throw new Error("Robustness evaluation does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a multi-strategy portfolio experiment record (RES-06). */
export function parsePortfolioExperiment(json: string): PortfolioExperiment {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Portfolio experiment is not valid JSON.");
  }
  if (!isPortfolioExperiment(value)) {
    throw new Error("Portfolio experiment does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a point-in-time knowledge snapshot (DATA-02). */
export function parseKnowledgeSnapshot(json: string): KnowledgeSnapshot {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Knowledge snapshot is not valid JSON.");
  }
  if (!isKnowledgeSnapshot(value)) {
    throw new Error("Knowledge snapshot does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a point-in-time event exposure calendar record (DATA-04). */
export function parseEventExposureCalendar(json: string): EventExposureCalendar {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Event exposure calendar is not valid JSON.");
  }
  if (!isEventExposureCalendar(value)) {
    throw new Error("Event exposure calendar does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a bounded research automation mandate (AI-04). */
export function parseAutomationMandate(json: string): AutomationMandate {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Automation mandate is not valid JSON.");
  }
  if (!isAutomationMandate(value)) {
    throw new Error("Automation mandate does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses an order decision passport linking intent through accounting (EXEC-02). */
export function parseOrderDecisionPassport(json: string): OrderDecisionPassport {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Order decision passport is not valid JSON.");
  }
  if (!isOrderDecisionPassport(value)) {
    throw new Error("Order decision passport does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a multi-factor cross-strategy exposure graph (RISK-01). */
export function parseExposureGraph(json: string): ExposureGraph {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Exposure graph is not valid JSON.");
  }
  if (!isExposureGraph(value)) {
    throw new Error("Exposure graph does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses an attributable personal fund ledger statement (PORT-01). */
export function parseFundLedgerStatement(json: string): FundLedgerStatement {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Fund ledger statement is not valid JSON.");
  }
  if (!isFundLedgerStatement(value)) {
    throw new Error("Fund ledger statement does not match the v1 evidence contract.");
  }
  return value;
}

function isResearchHypothesis(value: unknown): value is ResearchHypothesis {
  if (!hasExactKeys(value, [
    "assumptions",
    "created_at",
    "evaluation_horizon",
    "failure_criteria",
    "frozen_at",
    "frozen_evaluation_plan",
    "hypothesis_id",
    "hypothesis_schema_version",
    "mechanism",
    "predecessor_id",
    "status",
    "title",
    "universe",
  ])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  if (candidate.hypothesis_schema_version !== 1 || !isCanonicalId(candidate.hypothesis_id)) return false;
  if (typeof candidate.title !== "string" || candidate.title.length === 0) return false;
  if (typeof candidate.mechanism !== "string" || candidate.mechanism.length === 0) return false;
  if (!Array.isArray(candidate.universe) || candidate.universe.length === 0 || !candidate.universe.every(isCanonicalId)) return false;
  if (!hasExactKeys(candidate.evaluation_horizon, ["end_time", "holding_period", "start_time"])) return false;
  const horizon = candidate.evaluation_horizon as Record<string, unknown>;
  if (!isUtcTimestamp(horizon.start_time) || !isUtcTimestamp(horizon.end_time) || typeof horizon.holding_period !== "string" || horizon.holding_period.length === 0) return false;
  if (!Array.isArray(candidate.assumptions) || candidate.assumptions.length === 0 || !candidate.assumptions.every((a) => typeof a === "string" && a.length > 0)) return false;
  if (!Array.isArray(candidate.failure_criteria) || candidate.failure_criteria.length === 0 || !candidate.failure_criteria.every((f) => typeof f === "string" && f.length > 0)) return false;
  if (!hasExactKeys(candidate.frozen_evaluation_plan, ["cost_model", "dataset_hash", "dataset_id", "dataset_version", "fee_model", "slippage_bps"])) return false;
  const plan = candidate.frozen_evaluation_plan as Record<string, unknown>;
  if (!isCanonicalId(plan.dataset_id) || typeof plan.dataset_version !== "string" || !isHash(plan.dataset_hash) || typeof plan.cost_model !== "string" || !isNonNegativeInteger(plan.slippage_bps) || typeof plan.fee_model !== "string") return false;
  if (candidate.predecessor_id !== null && !isCanonicalId(candidate.predecessor_id)) return false;
  const validStatus = ["DRAFT", "FROZEN", "EVALUATING", "CONFIRMED", "REJECTED"];
  if (typeof candidate.status !== "string" || !validStatus.includes(candidate.status)) return false;
  if (!isUtcTimestamp(candidate.created_at)) return false;
  if (candidate.frozen_at !== null && !isUtcTimestamp(candidate.frozen_at)) return false;
  return true;
}

function isExperimentLineage(value: unknown): value is ExperimentLineage {
  if (!hasExactKeys(value, [
    "candidate_trials",
    "created_at",
    "failed_candidates_count",
    "hypothesis_id",
    "input_fingerprints",
    "lineage_id",
    "lineage_schema_version",
    "output_fingerprints",
    "parent_run_ids",
    "rejection_reasons",
  ])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  if (candidate.lineage_schema_version !== 1 || !isCanonicalId(candidate.lineage_id) || !isCanonicalId(candidate.hypothesis_id)) return false;
  if (!Array.isArray(candidate.parent_run_ids) || !candidate.parent_run_ids.every(isCanonicalId)) return false;
  const isNamedHash = (entry: unknown) => hasExactKeys(entry, ["fingerprint", "name"]) && typeof (entry as Record<string, unknown>).name === "string" && isHash((entry as Record<string, unknown>).fingerprint);
  if (!Array.isArray(candidate.input_fingerprints) || !candidate.input_fingerprints.every(isNamedHash)) return false;
  if (!Array.isArray(candidate.output_fingerprints) || !candidate.output_fingerprints.every(isNamedHash)) return false;
  if (!Array.isArray(candidate.candidate_trials)) return false;
  const validDisposition = ["PROMOTED", "REJECTED", "BENCHMARK"];
  for (const trial of candidate.candidate_trials) {
    if (!hasExactKeys(trial, ["disposition", "max_drawdown_bps", "return_bps", "specification_hash", "trial_id"])) return false;
    const t = trial as Record<string, unknown>;
    if (!isCanonicalId(t.trial_id) || !isHash(t.specification_hash) || !isDecimal(t.return_bps) || !isDecimal(t.max_drawdown_bps) || typeof t.disposition !== "string" || !validDisposition.includes(t.disposition)) return false;
  }
  if (!isNonNegativeInteger(candidate.failed_candidates_count)) return false;
  if (!Array.isArray(candidate.rejection_reasons)) return false;
  for (const r of candidate.rejection_reasons) {
    if (!hasExactKeys(r, ["reason", "trial_id"])) return false;
    const item = r as Record<string, unknown>;
    if (!isCanonicalId(item.trial_id) || typeof item.reason !== "string" || item.reason.length === 0) return false;
  }
  return isUtcTimestamp(candidate.created_at);
}

function isResearchJob(value: unknown): value is ResearchJob {
  if (!hasExactKeys(value, [
    "created_at",
    "dataset_id",
    "dataset_version",
    "failure_reason",
    "frozen_specification_hash",
    "idempotency_key",
    "job_id",
    "job_schema_version",
    "output_manifest_hash",
    "state",
    "state_version",
    "strategy_id",
    "strategy_version",
    "updated_at",
    "worker_lease",
  ])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  if (candidate.job_schema_version !== 1 || !isCanonicalId(candidate.job_id) || !isCanonicalId(candidate.idempotency_key) || !isCanonicalId(candidate.strategy_id) || typeof candidate.strategy_version !== "string") return false;
  if (!isCanonicalId(candidate.dataset_id) || typeof candidate.dataset_version !== "string") return false;
  if (!isHash(candidate.frozen_specification_hash)) return false;
  if (!isPositiveInteger(candidate.state_version)) return false;
  const validState = ["QUEUED", "RUNNING", "COMPLETED", "FAILED", "CANCELLED"];
  if (typeof candidate.state !== "string" || !validState.includes(candidate.state)) return false;
  if (candidate.worker_lease !== null) {
    if (!hasExactKeys(candidate.worker_lease, ["acquired_at", "expires_at", "lease_id", "worker_id"])) return false;
    const lease = candidate.worker_lease as Record<string, unknown>;
    if (!isCanonicalId(lease.lease_id) || !isCanonicalId(lease.worker_id) || !isUtcTimestamp(lease.acquired_at) || !isUtcTimestamp(lease.expires_at)) return false;
  }
  if (candidate.output_manifest_hash !== null && !isHash(candidate.output_manifest_hash)) return false;
  if (candidate.failure_reason !== null && (typeof candidate.failure_reason !== "string" || candidate.failure_reason.length === 0)) return false;
  return isUtcTimestamp(candidate.created_at) && isUtcTimestamp(candidate.updated_at);
}

function isAssistantEvidence(value: unknown): value is AssistantEvidence {
  if (!hasExactKeys(value, [
    "assistant_evidence_schema_version",
    "created_at",
    "generated_output",
    "human_disposition",
    "model_version",
    "prompt_template_version",
    "query_id",
    "retrieved_record_ids",
    "tool_attempts",
    "uncertainty_score_bps",
  ])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  if (candidate.assistant_evidence_schema_version !== 1 || !isCanonicalId(candidate.query_id)) return false;
  if (typeof candidate.model_version !== "string" || candidate.model_version.length === 0) return false;
  if (typeof candidate.prompt_template_version !== "string" || candidate.prompt_template_version.length === 0) return false;
  if (!Array.isArray(candidate.retrieved_record_ids) || candidate.retrieved_record_ids.length === 0 || !candidate.retrieved_record_ids.every((r) => typeof r === "string" && r.length > 0)) return false;
  if (typeof candidate.generated_output !== "string" || candidate.generated_output.length === 0) return false;
  if (!Array.isArray(candidate.tool_attempts)) return false;
  for (const t of candidate.tool_attempts) {
    if (!hasExactKeys(t, ["arguments_hash", "evidence_id", "status", "tool_name"])) return false;
    const attempt = t as Record<string, unknown>;
    if (typeof attempt.tool_name !== "string" || !isHash(attempt.arguments_hash) || !["SUCCESS", "FAILED", "BLOCKED"].includes(String(attempt.status)) || typeof attempt.evidence_id !== "string") return false;
  }
  if (!isNonNegativeInteger(candidate.uncertainty_score_bps) || candidate.uncertainty_score_bps > 10000) return false;
  const validDisp = ["ACCEPTED", "REJECTED", "AMENDED", "PENDING"];
  if (typeof candidate.human_disposition !== "string" || !validDisp.includes(candidate.human_disposition)) return false;
  return isUtcTimestamp(candidate.created_at);
}

function isRobustnessEvaluation(value: unknown): value is RobustnessEvaluation {
  if (!hasExactKeys(value, [
    "cost_shocks",
    "created_at",
    "disposition",
    "evaluation_id",
    "evaluation_schema_version",
    "hypothesis_id",
    "leakage_checks",
    "parameter_stability",
    "strategy_version",
    "uncertainty_score_bps",
    "walk_forward_windows",
  ])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  if (candidate.evaluation_schema_version !== 1 || !isCanonicalId(candidate.evaluation_id) || !isCanonicalId(candidate.hypothesis_id) || typeof candidate.strategy_version !== "string") return false;
  if (!Array.isArray(candidate.walk_forward_windows) || candidate.walk_forward_windows.length === 0) return false;
  for (const w of candidate.walk_forward_windows) {
    if (!hasExactKeys(w, [
      "in_sample_end", "in_sample_return_bps", "in_sample_start",
      "max_drawdown_bps", "out_of_sample_end", "out_of_sample_return_bps",
      "out_of_sample_start", "window_id",
    ])) return false;
    const window = w as Record<string, unknown>;
    if (!isCanonicalId(window.window_id) || !isUtcTimestamp(window.in_sample_start) || !isUtcTimestamp(window.in_sample_end) || !isUtcTimestamp(window.out_of_sample_start) || !isUtcTimestamp(window.out_of_sample_end) || typeof window.in_sample_return_bps !== "number" || typeof window.out_of_sample_return_bps !== "number" || typeof window.max_drawdown_bps !== "number") return false;
  }
  if (!hasExactKeys(candidate.leakage_checks, [
    "corporate_action_adjusted", "lookahead_bias_verified", "quarantine_violations", "survivorship_bias_verified",
  ])) return false;
  const lk = candidate.leakage_checks as Record<string, unknown>;
  if (typeof lk.survivorship_bias_verified !== "boolean" || typeof lk.lookahead_bias_verified !== "boolean" || typeof lk.corporate_action_adjusted !== "boolean" || !isNonNegativeInteger(lk.quarantine_violations)) return false;
  if (!hasExactKeys(candidate.parameter_stability, [
    "degradation_cliff_detected", "neighborhood_variance_bps", "perturbation_percent",
  ])) return false;
  const ps = candidate.parameter_stability as Record<string, unknown>;
  if (!isNonNegativeInteger(ps.perturbation_percent) || !isNonNegativeInteger(ps.neighborhood_variance_bps) || typeof ps.degradation_cliff_detected !== "boolean") return false;
  if (!Array.isArray(candidate.cost_shocks)) return false;
  for (const cs of candidate.cost_shocks) {
    if (!hasExactKeys(cs, ["fee_multiplier", "slippage_multiplier", "stressed_return_bps"])) return false;
    const shock = cs as Record<string, unknown>;
    if (typeof shock.slippage_multiplier !== "string" || typeof shock.fee_multiplier !== "string" || typeof shock.stressed_return_bps !== "number") return false;
  }
  if (!isNonNegativeInteger(candidate.uncertainty_score_bps) || candidate.uncertainty_score_bps > 10000) return false;
  if (!["ROBUST", "FRAGILE", "LEAKAGE_DETECTED", "DEGRADED"].includes(String(candidate.disposition))) return false;
  return isUtcTimestamp(candidate.created_at);
}

function isPortfolioExperiment(value: unknown): value is PortfolioExperiment {
  if (!hasExactKeys(value, [
    "allocated_cash",
    "created_at",
    "currency",
    "experiment_id",
    "joint_constraints",
    "joint_performance",
    "order_contention_events",
    "portfolio_experiment_schema_version",
    "strategies",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.portfolio_experiment_schema_version !== 1 || !isCanonicalId(candidate.experiment_id)) return false;
  if (!isDecimal(candidate.allocated_cash) || typeof candidate.currency !== "string" || candidate.currency.length !== 3) return false;
  if (!Array.isArray(candidate.strategies) || candidate.strategies.length < 2) return false;
  for (const s of candidate.strategies) {
    if (!hasExactKeys(s, ["max_drawdown_bps", "realized_pnl", "strategy_id", "strategy_version", "target_weight_bps"])) return false;
    const strat = s as Record<string, unknown>;
    if (!isCanonicalId(strat.strategy_id) || typeof strat.strategy_version !== "string" || !isNonNegativeInteger(strat.target_weight_bps) || !isDecimal(strat.realized_pnl) || typeof strat.max_drawdown_bps !== "number") return false;
  }
  if (!hasExactKeys(candidate.joint_constraints, ["max_gross_exposure_bps", "max_single_instrument_bps", "turnover_cap_daily_bps"])) return false;
  const jc = candidate.joint_constraints as Record<string, unknown>;
  if (!isNonNegativeInteger(jc.max_gross_exposure_bps) || !isNonNegativeInteger(jc.max_single_instrument_bps) || !isNonNegativeInteger(jc.turnover_cap_daily_bps)) return false;
  if (!hasExactKeys(candidate.joint_performance, ["combined_max_drawdown_bps", "combined_return_bps", "diversification_ratio_bps", "total_fee_drag"])) return false;
  const jp = candidate.joint_performance as Record<string, unknown>;
  if (typeof jp.combined_return_bps !== "number" || typeof jp.combined_max_drawdown_bps !== "number" || !isNonNegativeInteger(jp.diversification_ratio_bps) || !isDecimal(jp.total_fee_drag)) return false;
  if (!isNonNegativeInteger(candidate.order_contention_events)) return false;
  return isUtcTimestamp(candidate.created_at);
}

function isKnowledgeSnapshot(value: unknown): value is KnowledgeSnapshot {
  if (!hasExactKeys(value, [
    "as_of_time",
    "created_at",
    "entity_nodes",
    "knowledge_schema_version",
    "relationships",
    "snapshot_id",
    "source_lineage_hashes",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.knowledge_schema_version !== 1 || !isCanonicalId(candidate.snapshot_id)) return false;
  if (!isUtcTimestamp(candidate.as_of_time) || !isUtcTimestamp(candidate.created_at)) return false;
  if (!Array.isArray(candidate.entity_nodes) || !Array.isArray(candidate.relationships) || !Array.isArray(candidate.source_lineage_hashes)) return false;
  for (const n of candidate.entity_nodes) {
    if (!hasExactKeys(n, ["entity_id", "entity_type", "identifier", "name"])) return false;
    const node = n as Record<string, unknown>;
    if (!isCanonicalId(node.entity_id) || !["COMPANY", "INSTRUMENT", "FILING", "HEADLINE", "MACRO_EVENT"].includes(String(node.entity_type)) || typeof node.name !== "string" || typeof node.identifier !== "string") return false;
  }
  for (const r of candidate.relationships) {
    if (!hasExactKeys(r, ["effective_time", "provenance_hash", "relation_type", "source_entity_id", "target_entity_id"])) return false;
    const rel = r as Record<string, unknown>;
    if (!isCanonicalId(rel.source_entity_id) || !isCanonicalId(rel.target_entity_id) || typeof rel.relation_type !== "string" || !isUtcTimestamp(rel.effective_time) || !isHash(rel.provenance_hash)) return false;
  }
  return candidate.source_lineage_hashes.every(isHash);
}

function isEventExposureCalendar(value: unknown): value is EventExposureCalendar {
  if (!hasExactKeys(value, [
    "as_of_time",
    "calendar_id",
    "calendar_schema_version",
    "created_at",
    "quarantined_events_count",
    "scheduled_events",
    "timezone",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.calendar_schema_version !== 1 || !isCanonicalId(candidate.calendar_id)) return false;
  if (!isUtcTimestamp(candidate.as_of_time) || !isUtcTimestamp(candidate.created_at) || typeof candidate.timezone !== "string") return false;
  if (!isNonNegativeInteger(candidate.quarantined_events_count)) return false;
  if (!Array.isArray(candidate.scheduled_events)) return false;
  for (const ev of candidate.scheduled_events) {
    if (!hasExactKeys(ev, ["category", "event_id", "instrument_id", "scheduled_time", "source_evidence", "status"])) return false;
    const e = ev as Record<string, unknown>;
    if (!isCanonicalId(e.event_id) || !isCanonicalId(e.instrument_id) || !["EARNINGS", "DIVIDEND", "STOCK_SPLIT", "TRADING_HALT", "OPTION_EXPIRY", "SETTLEMENT"].includes(String(e.category)) || !isUtcTimestamp(e.scheduled_time) || !["SCHEDULED", "CONFIRMED", "CANCELLED", "COMPLETED"].includes(String(e.status)) || typeof e.source_evidence !== "string") return false;
  }
  return true;
}

function isAutomationMandate(value: unknown): value is AutomationMandate {
  if (!hasExactKeys(value, [
    "allowed_tasks",
    "broker_access_permitted",
    "cancellation_policy",
    "created_at",
    "expires_at",
    "mandate_id",
    "mandate_schema_version",
    "owner",
    "resource_limits",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.mandate_schema_version !== 1 || !isCanonicalId(candidate.mandate_id) || typeof candidate.owner !== "string") return false;
  if (candidate.broker_access_permitted !== false) return false;
  if (!Array.isArray(candidate.allowed_tasks) || candidate.allowed_tasks.length === 0 || !candidate.allowed_tasks.every((t) => typeof t === "string")) return false;
  if (!hasExactKeys(candidate.resource_limits, ["max_cpu_cores", "max_duration_seconds", "max_memory_mb", "max_storage_bytes"])) return false;
  const rl = candidate.resource_limits as Record<string, unknown>;
  if (!isPositiveInteger(rl.max_cpu_cores) || !isPositiveInteger(rl.max_memory_mb) || !isPositiveInteger(rl.max_duration_seconds) || !isPositiveInteger(rl.max_storage_bytes)) return false;
  if (!hasExactKeys(candidate.cancellation_policy, ["checkpoint_interval_seconds", "stop_on_first_error"])) return false;
  const cp = candidate.cancellation_policy as Record<string, unknown>;
  if (typeof cp.stop_on_first_error !== "boolean" || !isPositiveInteger(cp.checkpoint_interval_seconds)) return false;
  return isUtcTimestamp(candidate.created_at) && isUtcTimestamp(candidate.expires_at);
}

function isOrderDecisionPassport(value: unknown): value is OrderDecisionPassport {
  if (!hasExactKeys(value, [
    "accounting_consequences",
    "created_at",
    "executions",
    "instrument_id",
    "intent_id",
    "order_id",
    "passport_id",
    "passport_schema_version",
    "risk_evaluation",
    "routing_plan",
    "signal_attribution",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.passport_schema_version !== 1 || !isCanonicalId(candidate.passport_id) || !isCanonicalId(candidate.intent_id) || !isCanonicalId(candidate.order_id) || !isCanonicalId(candidate.instrument_id)) return false;
  if (!hasExactKeys(candidate.signal_attribution, ["model_event_id", "opportunity_description", "signal_power_bps", "strategy_version"])) return false;
  const sa = candidate.signal_attribution as Record<string, unknown>;
  if (typeof sa.strategy_version !== "string" || !isCanonicalId(sa.model_event_id) || typeof sa.opportunity_description !== "string" || typeof sa.signal_power_bps !== "number") return false;
  if (!hasExactKeys(candidate.risk_evaluation, ["approved", "evaluated_limits", "headroom_remaining_bps", "policy_version"])) return false;
  const re = candidate.risk_evaluation as Record<string, unknown>;
  if (typeof re.policy_version !== "string" || typeof re.approved !== "boolean" || !Array.isArray(re.evaluated_limits) || typeof re.headroom_remaining_bps !== "number") return false;
  if (!hasExactKeys(candidate.routing_plan, ["algorithm", "allocated_slices_count", "capability_version", "primary_venue"])) return false;
  const rp = candidate.routing_plan as Record<string, unknown>;
  if (typeof rp.algorithm !== "string" || !isPositiveInteger(rp.allocated_slices_count) || !isCanonicalId(rp.primary_venue) || typeof rp.capability_version !== "string") return false;
  if (!Array.isArray(candidate.executions) || candidate.executions.length === 0) return false;
  for (const ex of candidate.executions) {
    if (!hasExactKeys(ex, ["executed_at", "execution_id", "fee", "price", "quantity", "venue"])) return false;
    const exec = ex as Record<string, unknown>;
    if (!isCanonicalId(exec.execution_id) || !isCanonicalId(exec.venue) || !isDecimal(exec.quantity) || !isDecimal(exec.price) || !isDecimal(exec.fee) || !isUtcTimestamp(exec.executed_at)) return false;
  }
  if (!hasExactKeys(candidate.accounting_consequences, ["cash_delta", "journal_entry_id", "position_after", "realized_pnl"])) return false;
  const ac = candidate.accounting_consequences as Record<string, unknown>;
  if (!isCanonicalId(ac.journal_entry_id) || !isDecimal(ac.realized_pnl) || !isDecimal(ac.cash_delta) || !isDecimal(ac.position_after)) return false;
  return isUtcTimestamp(candidate.created_at);
}

function isExposureGraph(value: unknown): value is ExposureGraph {
  if (!hasExactKeys(value, [
    "account_id",
    "as_of_time",
    "created_at",
    "exposure_schema_version",
    "factors",
    "graph_id",
    "gross_exposure",
    "net_exposure",
    "sectors",
    "top_concentrations",
    "unreconciled_discrepancy",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.exposure_schema_version !== 1 || !isCanonicalId(candidate.graph_id) || !isCanonicalId(candidate.account_id)) return false;
  if (!isUtcTimestamp(candidate.as_of_time) || !isUtcTimestamp(candidate.created_at)) return false;
  if (!isDecimal(candidate.gross_exposure) || !isDecimal(candidate.net_exposure) || typeof candidate.unreconciled_discrepancy !== "boolean") return false;
  if (!Array.isArray(candidate.factors) || !Array.isArray(candidate.sectors) || !Array.isArray(candidate.top_concentrations)) return false;
  for (const f of candidate.factors) {
    if (!hasExactKeys(f, ["factor_name", "factor_variance_pct", "loading_bps"])) return false;
    const fac = f as Record<string, unknown>;
    if (typeof fac.factor_name !== "string" || typeof fac.loading_bps !== "number" || typeof fac.factor_variance_pct !== "string") return false;
  }
  for (const s of candidate.sectors) {
    if (!hasExactKeys(s, ["exposure_usd", "sector_name", "weight_bps"])) return false;
    const sec = s as Record<string, unknown>;
    if (typeof sec.sector_name !== "string" || !isDecimal(sec.exposure_usd) || typeof sec.weight_bps !== "number") return false;
  }
  for (const c of candidate.top_concentrations) {
    if (!hasExactKeys(c, ["instrument_id", "portfolio_pct", "position_value"])) return false;
    const conc = c as Record<string, unknown>;
    if (!isCanonicalId(conc.instrument_id) || !isDecimal(conc.position_value) || typeof conc.portfolio_pct !== "string") return false;
  }
  return true;
}

function isFundLedgerStatement(value: unknown): value is FundLedgerStatement {
  if (!hasExactKeys(value, [
    "account_id",
    "balanced",
    "created_at",
    "ending_cash",
    "fee_totals",
    "ledger_schema_version",
    "period_end",
    "period_start",
    "realized_pnl",
    "starting_cash",
    "statement_id",
    "tax_lots",
    "unrealized_pnl",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.ledger_schema_version !== 1 || !isCanonicalId(candidate.statement_id) || !isCanonicalId(candidate.account_id)) return false;
  if (!isUtcTimestamp(candidate.period_start) || !isUtcTimestamp(candidate.period_end) || !isUtcTimestamp(candidate.created_at)) return false;
  if (!isDecimal(candidate.starting_cash) || !isDecimal(candidate.ending_cash) || !isDecimal(candidate.realized_pnl) || !isDecimal(candidate.unrealized_pnl) || typeof candidate.balanced !== "boolean") return false;
  if (!hasExactKeys(candidate.fee_totals, ["brokerage_commissions", "borrow_financing", "exchange_fees"])) return false;
  const ft = candidate.fee_totals as Record<string, unknown>;
  if (!isDecimal(ft.exchange_fees) || !isDecimal(ft.brokerage_commissions) || !isDecimal(ft.borrow_financing)) return false;
  if (!Array.isArray(candidate.tax_lots)) return false;
  for (const tl of candidate.tax_lots) {
    if (!hasExactKeys(tl, ["acquired_at", "cost_basis", "disposition", "instrument_id", "lot_id", "quantity"])) return false;
    const lot = tl as Record<string, unknown>;
    if (!isCanonicalId(lot.lot_id) || !isCanonicalId(lot.instrument_id) || !isUtcTimestamp(lot.acquired_at) || !isDecimal(lot.quantity) || !isDecimal(lot.cost_basis) || !["OPEN", "CLOSED_FIFO", "CLOSED_SPECID"].includes(String(lot.disposition))) return false;
  }
  return true;
}
