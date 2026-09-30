// FX pricing, statements, continuity, capital, capability and lifecycle planning evidence.

import { hasExactKeys, isCanonicalId, isDecimal, isHash, isNonNegativeInteger, isPositiveInteger, isUtcTimestamp } from "./core.js";

export type FxPricingEvaluation = Readonly<{
  age_seconds: number;
  ask: string;
  bid: string;
  fresh: boolean;
  instrument_id: string;
  midpoint: string;
  pair: string;
  product: string;
  query_id: string;
  received_at: string;
  snapshot_id: string;
  source_id: string;
  source_sequence: number;
  source_time: string;
  spread_bps: string;
  value_date: string;
}>;

export type FxPricingSnapshotRecord = Readonly<{
  canonical_record: string;
  instrument_id: string;
  pair: string;
  product: string;
  received_at: string;
  reference_version: string;
  snapshot_id: string;
  source_id: string;
  source_sequence: number;
  source_time: string;
}>;

export type FxPricingDashboard = Readonly<{
  fx_pricing_schema_version: 1;
  as_of: string;
  configuration_content_hash: string;
  evaluations: ReadonlyArray<FxPricingEvaluation>;
  max_age_seconds: number;
  snapshot_count: number;
  snapshots: ReadonlyArray<FxPricingSnapshotRecord>;
}>;

export type StatementIncident = Readonly<{
  incident_kind: "CASH_MISMATCH" | "POSITION_MISMATCH";
  broker_balance?: string;
  currency?: string;
  internal_balance?: string;
  broker_quantity?: string;
  instrument_id?: string;
  internal_quantity?: string;
}>;

export type StatementReconciliation = Readonly<{
  statement_reconciliation_schema_version: 1;
  account_id: string;
  as_of: string;
  clean: boolean;
  configuration_content_hash: string;
  incident_count: number;
  incidents: ReadonlyArray<StatementIncident>;
  statement_csv_hash: string;
}>;

export type ContinuityPolicy = Readonly<{
  policy_schema_version: 1;
  policy_id: string;
  unattended_interval_minutes: number;
  heartbeat_interval_seconds: number;
  max_restarts_per_hour: number;
  away_mode_permitted: boolean;
  broker_disconnect_action: "RETAIN_UNKNOWN_AND_ESCALATE" | "CANCEL_LOCAL_WORKING_ONLY" | "HOLD_STATE";
  feed_stale_threshold_seconds: number;
  created_at: string;
}>;

export type AssumptionRegimeMonitor = Readonly<{
  regime_schema_version: 1;
  regime_id: string;
  as_of_time: string;
  lookback_bars: number;
  current_regime: "LOW_VOL_TRENDING" | "ELEVATED_VOL_TRENDING" | "HIGH_VOL_CHOPPY" | "LIQUIDITY_COMPRESSION" | "EXTREME_STRESS";
  indicators: Readonly<{
    realized_vol_annual_bps: number;
    effective_spread_bps: number;
    trend_strength_bps: number;
    cross_asset_correlation_bps: number;
  }>;
  impacted_strategy_assumptions: ReadonlyArray<Readonly<{
    strategy_id: string;
    assumed_condition: string;
    observed_condition: string;
    breach_status: "COMPATIBLE" | "ELEVATED_RISK" | "ASSUMPTION_VIOLATED";
  }>>;
  model_version: string;
  created_at: string;
}>;

export type FeedSubstitutionParity = Readonly<{
  parity_schema_version: 1;
  comparison_id: string;
  primary_provider: string;
  candidate_provider: string;
  sample_start: string;
  sample_end: string;
  symbol_match_pct: string;
  timestamp_variance_micros_p99: number;
  adjustment_parity_verified: boolean;
  parity_disposition: "QUALIFIED_FOR_SUBSTITUTION" | "DEFICIENT_COVERAGE" | "TIMESTAMP_DESYNC" | "UNRECONCILED_SPLITS";
  created_at: string;
}>;

export type ExecutionCoachBenchmark = Readonly<{
  coach_schema_version: 1;
  analysis_id: string;
  order_id: string;
  instrument_id: string;
  arrival_price: string;
  target_price: string;
  realized_vwap: string;
  pre_trade_estimated_cost_bps: number;
  realized_shortfall_bps: number;
  slippage_drag_bps: number;
  market_impact_bps: number;
  fee_drag_bps: number;
  execution_grade: "OPTIMAL" | "ACCEPTABLE" | "ELEVATED_SLIPPAGE" | "DEFICIENT_ROUTING";
  created_at: string;
}>;

export type ScenarioLossSimulation = Readonly<{
  simulation_schema_version: 1;
  simulation_id: string;
  account_id: string;
  scenario_name: string;
  shock_assumptions: Readonly<{
    equity_shock_pct: string;
    volatility_multiplier: string;
    spread_expansion_multiplier: string;
    financing_rate_shock_bps: number;
  }>;
  estimated_loss_usd: string;
  estimated_loss_bps: number;
  liquidity_haircut_usd: string;
  stressed_margin_utilization_pct: string;
  capital_adequate: boolean;
  created_at: string;
}>;

export type CapitalAllocationPlan = Readonly<{
  allocation_schema_version: 1;
  plan_id: string;
  total_capital_usd: string;
  cash_reserve_bps: number;
  allocations: ReadonlyArray<Readonly<{
    strategy_id: string;
    allocated_capital_usd: string;
    target_weight_bps: number;
    expected_sharpe: string;
  }>>;
  risk_policy_version: string;
  approved_by_policy: boolean;
  created_at: string;
}>;

export type SandboxInstallationPreview = Readonly<{
  preview_schema_version: 1;
  preview_id: string;
  asset_id: string;
  asset_version: string;
  manifest_hash: string;
  declared_permissions: readonly string[];
  resource_caps: Readonly<{
    max_memory_mb: number;
    max_cpu_percent: number;
    filesystem_isolated: boolean;
  }>;
  untrusted_capabilities_detected: number;
  rollback_snapshot_id: string;
  disposition: "QUALIFIED_FOR_ISOLATED_INSTALL" | "PERMISSION_OVERREACH_REJECTED" | "TAMPERED_MANIFEST_REJECTED";
  created_at: string;
}>;

export type AdapterQualification = Readonly<{
  qualification_schema_version: 1;
  qualification_id: string;
  venue: string;
  asset_class: "US_EQUITY" | "EQUITY_OPTION" | "SPOT_FX" | "COMMODITY_FUTURES";
  adapter_version: string;
  supported_capabilities: readonly string[];
  single_writer_fenced: boolean;
  reconciliation_pass_rate_pct: string;
  operational_gate_status: "QUALIFIED" | "PROVISIONAL" | "EXPIRED" | "REVOKED";
  created_at: string;
  expires_at: string;
}>;

export type ChampionChallengerEvaluation = Readonly<{
  champion_challenger_schema_version: 1;
  evaluation_id: string;
  champion_strategy_id: string;
  challenger_strategy_id: string;
  evaluation_window_start: string;
  evaluation_window_end: string;
  champion_return_bps: number;
  challenger_return_bps: number;
  champion_max_drawdown_bps: number;
  challenger_max_drawdown_bps: number;
  information_ratio_diff_bps: number;
  drift_detected: boolean;
  recommendation: "RETAIN_CHAMPION" | "PROMOTE_CHALLENGER" | "INITIATE_RETIREMENT_REVIEW" | "CONTINUE_SHADOW_MONITORING";
  created_at: string;
}>;

export type CapabilityExecutionPlanner = Readonly<{
  planner_schema_version: 1;
  plan_id: string;
  parent_order_id: string;
  target_venue: string;
  algorithm: "TWAP_SLICED" | "VWAP_PARTICIPATION" | "PASSIVE_PEG_QUEUE" | "ICEBERG_DISCRETIONARY";
  max_volume_participation_pct: string;
  passive_pegging_offset_bps: number;
  schedule_slices: ReadonlyArray<Readonly<{
    slice_sequence: number;
    planned_release_time: string;
    allocated_quantity: string;
    order_kind: string;
  }>>;
  supported_capabilities_verified: boolean;
  disposition: "VALIDATED_FOR_DISPATCH" | "UNSUPPORTED_ORDER_KIND" | "PARTICIPATION_CAP_EXCEEDED" | "VENUE_CAPABILITY_REJECTED";
  created_at: string;
}>;

export type OperationsDiagnosisRunbook = Readonly<{
  diagnosis_schema_version: 1;
  diagnosis_id: string;
  incident_id: string;
  failing_component: string;
  root_cause_summary: string;
  cited_evidence_ids: readonly string[];
  proposed_runbook_steps: ReadonlyArray<Readonly<{
    step_number: number;
    action_name: string;
    target_service: string;
    command_template: string;
    is_idempotent: boolean;
  }>>;
  idempotency_certified: boolean;
  trading_path_isolated: boolean;
  approval_required: "OPERATOR_CONFIRMATION" | "AUTOMATED_IDEMPOTENT" | "ESCALATION_BLOCKED";
  created_at: string;
}>;

export type ModelEvaluationBenchmark = Readonly<{
  model_evaluation_schema_version: 1;
  benchmark_id: string;
  model_identifier: string;
  evaluation_dataset_id: string;
  factuality_score_bps: number;
  citation_precision_bps: number;
  injection_resistance_score_bps: number;
  hallucination_rate_bps: number;
  average_latency_ms: number;
  token_cost_usd_per_million: string;
  disposition: "QUALIFIED_FOR_ASSISTANCE" | "UNRELIABLE_CITATION" | "VULNERABLE_TO_INJECTION" | "EXCESSIVE_LATENCY";
  evaluated_at: string;
}>;

export type StrategyCapsuleManifest = Readonly<{
  capsule_schema_version: 1;
  capsule_id: string;
  strategy_id: string;
  strategy_version: string;
  bundle_sha256: string;
  configuration_sha256: string;
  dependency_lockfile_sha256: string;
  runtime_target: string;
  evaluation_receipt_id: string;
  replay_instruction_command: string;
  export_disposition: "VERIFIED_PORTABLE" | "MISSING_DEPENDENCY_LOCK" | "UNVERIFIED_EVALUATION" | "RESTRICTED_DATASET_RIGHTS";
  packaged_at: string;
}>;

export type MultiAssetExpansionPlan = Readonly<{
  expansion_schema_version: 1;
  plan_id: string;
  asset_class: "EQUITY_OPTION" | "SPOT_FX" | "COMMODITY_FUTURES" | "INDEX_FUTURES";
  underlying_universe: readonly string[];
  lifecycle_actions: ReadonlyArray<Readonly<{
    action_id: string;
    instrument_id: string;
    action_kind: "OPTION_ROLL" | "OPTION_EXERCISE" | "FUTURES_ROLL" | "FX_SPOT_CONVERSION";
    target_date: string;
    contract_quantity: number;
    estimated_cash_flow_usd: string;
  }>>;
  margin_requirement_usd: string;
  settlement_currency: string;
  reconciliation_clean: boolean;
  operational_verdict: "READY_FOR_LIFECYCLE_EXECUTION" | "MARGIN_HEADROOM_BREACH" | "UNRECONCILED_LEG_MISMATCH" | "GATEWAY_DISCONNECTED";
  created_at: string;
}>;

/** Parses a solo session continuity and recovery policy (SOLO-06, LIFE-04/05/06). */
export function parseContinuityPolicy(json: string): ContinuityPolicy {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Continuity policy is not valid JSON.");
  }
  if (!isContinuityPolicy(value)) {
    throw new Error("Continuity policy does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses an assumption-aware regime monitor record (DATA-05). */
export function parseAssumptionRegimeMonitor(json: string): AssumptionRegimeMonitor {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Assumption regime monitor is not valid JSON.");
  }
  if (!isAssumptionRegimeMonitor(value)) {
    throw new Error("Assumption regime monitor does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a feed substitution parity record (DATA-06). */
export function parseFeedSubstitutionParity(json: string): FeedSubstitutionParity {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Feed substitution parity is not valid JSON.");
  }
  if (!isFeedSubstitutionParity(value)) {
    throw new Error("Feed substitution parity does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses an execution coach benchmark analysis record (EXEC-03, RES-07). */
export function parseExecutionCoachBenchmark(json: string): ExecutionCoachBenchmark {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Execution coach benchmark is not valid JSON.");
  }
  if (!isExecutionCoachBenchmark(value)) {
    throw new Error("Execution coach benchmark does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a scenario stress loss simulation record (RISK-02). */
export function parseScenarioLossSimulation(json: string): ScenarioLossSimulation {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Scenario loss simulation is not valid JSON.");
  }
  if (!isScenarioLossSimulation(value)) {
    throw new Error("Scenario loss simulation does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a capital allocation plan record (RISK-03). */
export function parseCapitalAllocationPlan(json: string): CapitalAllocationPlan {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Capital allocation plan is not valid JSON.");
  }
  if (!isCapitalAllocationPlan(value)) {
    throw new Error("Capital allocation plan does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a sandboxed installation preview record (ASSET-03, ASSET-04). */
export function parseSandboxInstallationPreview(json: string): SandboxInstallationPreview {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Sandbox installation preview is not valid JSON.");
  }
  if (!isSandboxInstallationPreview(value)) {
    throw new Error("Sandbox installation preview does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses an adapter qualification and single-writer fencing record (LIFE-07, PORT-02). */
export function parseAdapterQualification(json: string): AdapterQualification {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Adapter qualification is not valid JSON.");
  }
  if (!isAdapterQualification(value)) {
    throw new Error("Adapter qualification does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a deterministic FX pricing dashboard (v1). */
export function parseFxPricingDashboard(json: string): FxPricingDashboard {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("FX pricing dashboard is not valid JSON.");
  }
  if (!isFxPricingDashboard(value)) {
    throw new Error("FX pricing dashboard does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a broker statement reconciliation artifact (v1). */
export function parseStatementReconciliation(json: string): StatementReconciliation {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Statement reconciliation is not valid JSON.");
  }
  if (!isStatementReconciliation(value)) {
    throw new Error("Statement reconciliation does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a champion/challenger evaluation and strategy retirement record (RES-08). */
export function parseChampionChallengerEvaluation(json: string): ChampionChallengerEvaluation {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Champion challenger evaluation is not valid JSON.");
  }
  if (!isChampionChallengerEvaluation(value)) {
    throw new Error("Champion challenger evaluation does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a capability-aware execution planner record (EXEC-04). */
export function parseCapabilityExecutionPlanner(json: string): CapabilityExecutionPlanner {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Capability execution planner is not valid JSON.");
  }
  if (!isCapabilityExecutionPlanner(value)) {
    throw new Error("Capability execution planner does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses an operations diagnosis runbook record (AI-05). */
export function parseOperationsDiagnosisRunbook(json: string): OperationsDiagnosisRunbook {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Operations diagnosis runbook is not valid JSON.");
  }
  if (!isOperationsDiagnosisRunbook(value)) {
    throw new Error("Operations diagnosis runbook does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a model evaluation and portability benchmark record (AI-06). */
export function parseModelEvaluationBenchmark(json: string): ModelEvaluationBenchmark {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Model evaluation benchmark is not valid JSON.");
  }
  if (!isModelEvaluationBenchmark(value)) {
    throw new Error("Model evaluation benchmark does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a portable strategy capsule manifest record (ASSET-04). */
export function parseStrategyCapsuleManifest(json: string): StrategyCapsuleManifest {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Strategy capsule manifest is not valid JSON.");
  }
  if (!isStrategyCapsuleManifest(value)) {
    throw new Error("Strategy capsule manifest does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a multi-asset expansion and lifecycle plan record (PORT-02). */
export function parseMultiAssetExpansionPlan(json: string): MultiAssetExpansionPlan {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Multi-asset expansion plan is not valid JSON.");
  }
  if (!isMultiAssetExpansionPlan(value)) {
    throw new Error("Multi-asset expansion plan does not match the v1 evidence contract.");
  }
  return value;
}

function isContinuityPolicy(value: unknown): value is ContinuityPolicy {
  if (!hasExactKeys(value, [
    "away_mode_permitted",
    "broker_disconnect_action",
    "created_at",
    "feed_stale_threshold_seconds",
    "heartbeat_interval_seconds",
    "max_restarts_per_hour",
    "policy_id",
    "policy_schema_version",
    "unattended_interval_minutes",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.policy_schema_version !== 1 || !isCanonicalId(candidate.policy_id)) return false;
  if (!isPositiveInteger(candidate.unattended_interval_minutes) || !isPositiveInteger(candidate.heartbeat_interval_seconds) || !isPositiveInteger(candidate.max_restarts_per_hour) || !isPositiveInteger(candidate.feed_stale_threshold_seconds)) return false;
  if (typeof candidate.away_mode_permitted !== "boolean") return false;
  if (!["RETAIN_UNKNOWN_AND_ESCALATE", "CANCEL_LOCAL_WORKING_ONLY", "HOLD_STATE"].includes(String(candidate.broker_disconnect_action))) return false;
  return isUtcTimestamp(candidate.created_at);
}

function isAssumptionRegimeMonitor(value: unknown): value is AssumptionRegimeMonitor {
  if (!hasExactKeys(value, [
    "as_of_time",
    "created_at",
    "current_regime",
    "impacted_strategy_assumptions",
    "indicators",
    "lookback_bars",
    "model_version",
    "regime_id",
    "regime_schema_version",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.regime_schema_version !== 1 || !isCanonicalId(candidate.regime_id)) return false;
  if (!isUtcTimestamp(candidate.as_of_time) || !isUtcTimestamp(candidate.created_at) || !isPositiveInteger(candidate.lookback_bars)) return false;
  if (!["LOW_VOL_TRENDING", "ELEVATED_VOL_TRENDING", "HIGH_VOL_CHOPPY", "LIQUIDITY_COMPRESSION", "EXTREME_STRESS"].includes(String(candidate.current_regime))) return false;
  if (!hasExactKeys(candidate.indicators, ["cross_asset_correlation_bps", "effective_spread_bps", "realized_vol_annual_bps", "trend_strength_bps"])) return false;
  const ind = candidate.indicators as Record<string, unknown>;
  if (typeof ind.realized_vol_annual_bps !== "number" || typeof ind.effective_spread_bps !== "number" || typeof ind.trend_strength_bps !== "number" || typeof ind.cross_asset_correlation_bps !== "number") return false;
  if (!Array.isArray(candidate.impacted_strategy_assumptions)) return false;
  for (const a of candidate.impacted_strategy_assumptions) {
    if (!hasExactKeys(a, ["assumed_condition", "breach_status", "observed_condition", "strategy_id"])) return false;
    const item = a as Record<string, unknown>;
    if (!isCanonicalId(item.strategy_id) || typeof item.assumed_condition !== "string" || typeof item.observed_condition !== "string" || !["COMPATIBLE", "ELEVATED_RISK", "ASSUMPTION_VIOLATED"].includes(String(item.breach_status))) return false;
  }
  return typeof candidate.model_version === "string" && candidate.model_version.length > 0;
}

function isFeedSubstitutionParity(value: unknown): value is FeedSubstitutionParity {
  if (!hasExactKeys(value, [
    "adjustment_parity_verified",
    "candidate_provider",
    "comparison_id",
    "created_at",
    "parity_disposition",
    "parity_schema_version",
    "primary_provider",
    "sample_end",
    "sample_start",
    "symbol_match_pct",
    "timestamp_variance_micros_p99",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.parity_schema_version !== 1 || !isCanonicalId(candidate.comparison_id)) return false;
  if (typeof candidate.primary_provider !== "string" || typeof candidate.candidate_provider !== "string") return false;
  if (!isUtcTimestamp(candidate.sample_start) || !isUtcTimestamp(candidate.sample_end) || !isUtcTimestamp(candidate.created_at)) return false;
  if (typeof candidate.symbol_match_pct !== "string" || !isNonNegativeInteger(candidate.timestamp_variance_micros_p99) || typeof candidate.adjustment_parity_verified !== "boolean") return false;
  return ["QUALIFIED_FOR_SUBSTITUTION", "DEFICIENT_COVERAGE", "TIMESTAMP_DESYNC", "UNRECONCILED_SPLITS"].includes(String(candidate.parity_disposition));
}

function isExecutionCoachBenchmark(value: unknown): value is ExecutionCoachBenchmark {
  if (!hasExactKeys(value, [
    "analysis_id",
    "arrival_price",
    "coach_schema_version",
    "created_at",
    "execution_grade",
    "fee_drag_bps",
    "instrument_id",
    "market_impact_bps",
    "order_id",
    "pre_trade_estimated_cost_bps",
    "realized_shortfall_bps",
    "realized_vwap",
    "slippage_drag_bps",
    "target_price",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.coach_schema_version !== 1 || !isCanonicalId(candidate.analysis_id) || !isCanonicalId(candidate.order_id) || !isCanonicalId(candidate.instrument_id)) return false;
  if (!isDecimal(candidate.arrival_price) || !isDecimal(candidate.target_price) || !isDecimal(candidate.realized_vwap)) return false;
  if (typeof candidate.pre_trade_estimated_cost_bps !== "number" || typeof candidate.realized_shortfall_bps !== "number" || typeof candidate.slippage_drag_bps !== "number" || typeof candidate.market_impact_bps !== "number" || typeof candidate.fee_drag_bps !== "number") return false;
  if (!["OPTIMAL", "ACCEPTABLE", "ELEVATED_SLIPPAGE", "DEFICIENT_ROUTING"].includes(String(candidate.execution_grade))) return false;
  return isUtcTimestamp(candidate.created_at);
}

function isScenarioLossSimulation(value: unknown): value is ScenarioLossSimulation {
  if (!hasExactKeys(value, [
    "account_id",
    "capital_adequate",
    "created_at",
    "estimated_loss_bps",
    "estimated_loss_usd",
    "liquidity_haircut_usd",
    "scenario_name",
    "shock_assumptions",
    "simulation_id",
    "simulation_schema_version",
    "stressed_margin_utilization_pct",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.simulation_schema_version !== 1 || !isCanonicalId(candidate.simulation_id) || !isCanonicalId(candidate.account_id) || typeof candidate.scenario_name !== "string") return false;
  if (!hasExactKeys(candidate.shock_assumptions, ["equity_shock_pct", "financing_rate_shock_bps", "spread_expansion_multiplier", "volatility_multiplier"])) return false;
  const sa = candidate.shock_assumptions as Record<string, unknown>;
  if (typeof sa.equity_shock_pct !== "string" || typeof sa.volatility_multiplier !== "string" || typeof sa.spread_expansion_multiplier !== "string" || typeof sa.financing_rate_shock_bps !== "number") return false;
  if (!isDecimal(candidate.estimated_loss_usd) || typeof candidate.estimated_loss_bps !== "number" || !isDecimal(candidate.liquidity_haircut_usd) || typeof candidate.stressed_margin_utilization_pct !== "string" || typeof candidate.capital_adequate !== "boolean") return false;
  return isUtcTimestamp(candidate.created_at);
}

function isCapitalAllocationPlan(value: unknown): value is CapitalAllocationPlan {
  if (!hasExactKeys(value, [
    "allocation_schema_version",
    "allocations",
    "approved_by_policy",
    "cash_reserve_bps",
    "created_at",
    "plan_id",
    "risk_policy_version",
    "total_capital_usd",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.allocation_schema_version !== 1 || !isCanonicalId(candidate.plan_id) || !isDecimal(candidate.total_capital_usd) || !isNonNegativeInteger(candidate.cash_reserve_bps) || candidate.cash_reserve_bps > 10000) return false;
  if (!Array.isArray(candidate.allocations) || candidate.allocations.length === 0) return false;
  for (const al of candidate.allocations) {
    if (!hasExactKeys(al, ["allocated_capital_usd", "expected_sharpe", "strategy_id", "target_weight_bps"])) return false;
    const a = al as Record<string, unknown>;
    if (!isCanonicalId(a.strategy_id) || !isDecimal(a.allocated_capital_usd) || !isNonNegativeInteger(a.target_weight_bps) || a.target_weight_bps > 10000 || typeof a.expected_sharpe !== "string") return false;
  }
  return typeof candidate.risk_policy_version === "string" && typeof candidate.approved_by_policy === "boolean" && isUtcTimestamp(candidate.created_at);
}

function isSandboxInstallationPreview(value: unknown): value is SandboxInstallationPreview {
  if (!hasExactKeys(value, [
    "asset_id",
    "asset_version",
    "created_at",
    "declared_permissions",
    "disposition",
    "manifest_hash",
    "preview_id",
    "preview_schema_version",
    "resource_caps",
    "rollback_snapshot_id",
    "untrusted_capabilities_detected",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.preview_schema_version !== 1 || !isCanonicalId(candidate.preview_id) || !isCanonicalId(candidate.asset_id) || typeof candidate.asset_version !== "string" || !isHash(candidate.manifest_hash)) return false;
  if (!Array.isArray(candidate.declared_permissions) || !candidate.declared_permissions.every((p) => typeof p === "string")) return false;
  if (!hasExactKeys(candidate.resource_caps, ["filesystem_isolated", "max_cpu_percent", "max_memory_mb"])) return false;
  const rc = candidate.resource_caps as Record<string, unknown>;
  if (!isPositiveInteger(rc.max_memory_mb) || !isPositiveInteger(rc.max_cpu_percent) || typeof rc.filesystem_isolated !== "boolean") return false;
  if (!isNonNegativeInteger(candidate.untrusted_capabilities_detected) || !isCanonicalId(candidate.rollback_snapshot_id)) return false;
  if (!["QUALIFIED_FOR_ISOLATED_INSTALL", "PERMISSION_OVERREACH_REJECTED", "TAMPERED_MANIFEST_REJECTED"].includes(String(candidate.disposition))) return false;
  return isUtcTimestamp(candidate.created_at);
}

function isAdapterQualification(value: unknown): value is AdapterQualification {
  if (!hasExactKeys(value, [
    "adapter_version",
    "asset_class",
    "created_at",
    "expires_at",
    "operational_gate_status",
    "qualification_id",
    "qualification_schema_version",
    "reconciliation_pass_rate_pct",
    "single_writer_fenced",
    "supported_capabilities",
    "venue",
  ])) return false;
  const candidate = value as Record<string, unknown>;
  if (candidate.qualification_schema_version !== 1 || !isCanonicalId(candidate.qualification_id) || !isCanonicalId(candidate.venue)) return false;
  if (!["US_EQUITY", "EQUITY_OPTION", "SPOT_FX", "COMMODITY_FUTURES"].includes(String(candidate.asset_class)) || typeof candidate.adapter_version !== "string") return false;
  if (!Array.isArray(candidate.supported_capabilities) || candidate.supported_capabilities.length === 0 || !candidate.supported_capabilities.every((c) => typeof c === "string")) return false;
  if (typeof candidate.single_writer_fenced !== "boolean" || typeof candidate.reconciliation_pass_rate_pct !== "string") return false;
  if (!["QUALIFIED", "PROVISIONAL", "EXPIRED", "REVOKED"].includes(String(candidate.operational_gate_status))) return false;
  return isUtcTimestamp(candidate.created_at) && isUtcTimestamp(candidate.expires_at);
}

function isChampionChallengerEvaluation(value: unknown): value is ChampionChallengerEvaluation {
  if (!hasExactKeys(value, [
    "challenger_max_drawdown_bps",
    "challenger_return_bps",
    "challenger_strategy_id",
    "champion_challenger_schema_version",
    "champion_max_drawdown_bps",
    "champion_return_bps",
    "champion_strategy_id",
    "created_at",
    "drift_detected",
    "evaluation_id",
    "evaluation_window_end",
    "evaluation_window_start",
    "information_ratio_diff_bps",
    "recommendation",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.champion_challenger_schema_version !== 1 || !isCanonicalId(c.evaluation_id) || !isCanonicalId(c.champion_strategy_id) || !isCanonicalId(c.challenger_strategy_id)) return false;
  if (!isUtcTimestamp(c.evaluation_window_start) || !isUtcTimestamp(c.evaluation_window_end) || !isUtcTimestamp(c.created_at)) return false;
  if (typeof c.champion_return_bps !== "number" || typeof c.challenger_return_bps !== "number" || typeof c.champion_max_drawdown_bps !== "number" || typeof c.challenger_max_drawdown_bps !== "number" || typeof c.information_ratio_diff_bps !== "number" || typeof c.drift_detected !== "boolean") return false;
  return ["RETAIN_CHAMPION", "PROMOTE_CHALLENGER", "INITIATE_RETIREMENT_REVIEW", "CONTINUE_SHADOW_MONITORING"].includes(String(c.recommendation));
}

function isCapabilityExecutionPlanner(value: unknown): value is CapabilityExecutionPlanner {
  if (!hasExactKeys(value, [
    "algorithm",
    "created_at",
    "disposition",
    "max_volume_participation_pct",
    "parent_order_id",
    "passive_pegging_offset_bps",
    "plan_id",
    "planner_schema_version",
    "schedule_slices",
    "supported_capabilities_verified",
    "target_venue",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.planner_schema_version !== 1 || !isCanonicalId(c.plan_id) || !isCanonicalId(c.parent_order_id) || !isCanonicalId(c.target_venue)) return false;
  if (!["TWAP_SLICED", "VWAP_PARTICIPATION", "PASSIVE_PEG_QUEUE", "ICEBERG_DISCRETIONARY"].includes(String(c.algorithm))) return false;
  if (typeof c.max_volume_participation_pct !== "string" || typeof c.passive_pegging_offset_bps !== "number" || typeof c.supported_capabilities_verified !== "boolean") return false;
  if (!Array.isArray(c.schedule_slices) || c.schedule_slices.length === 0) return false;
  for (const s of c.schedule_slices) {
    if (!hasExactKeys(s, ["allocated_quantity", "order_kind", "planned_release_time", "slice_sequence"])) return false;
    const slice = s as Record<string, unknown>;
    if (!isPositiveInteger(slice.slice_sequence) || !isUtcTimestamp(slice.planned_release_time) || !isDecimal(slice.allocated_quantity) || typeof slice.order_kind !== "string") return false;
  }
  if (!["VALIDATED_FOR_DISPATCH", "UNSUPPORTED_ORDER_KIND", "PARTICIPATION_CAP_EXCEEDED", "VENUE_CAPABILITY_REJECTED"].includes(String(c.disposition))) return false;
  return isUtcTimestamp(c.created_at);
}

function isOperationsDiagnosisRunbook(value: unknown): value is OperationsDiagnosisRunbook {
  if (!hasExactKeys(value, [
    "approval_required",
    "cited_evidence_ids",
    "created_at",
    "diagnosis_id",
    "diagnosis_schema_version",
    "failing_component",
    "idempotency_certified",
    "incident_id",
    "proposed_runbook_steps",
    "root_cause_summary",
    "trading_path_isolated",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.diagnosis_schema_version !== 1 || !isCanonicalId(c.diagnosis_id) || !isCanonicalId(c.incident_id)) return false;
  if (typeof c.failing_component !== "string" || typeof c.root_cause_summary !== "string" || typeof c.idempotency_certified !== "boolean" || typeof c.trading_path_isolated !== "boolean") return false;
  if (!Array.isArray(c.cited_evidence_ids) || c.cited_evidence_ids.length === 0 || !c.cited_evidence_ids.every((id) => typeof id === "string")) return false;
  if (!Array.isArray(c.proposed_runbook_steps) || c.proposed_runbook_steps.length === 0) return false;
  for (const st of c.proposed_runbook_steps) {
    if (!hasExactKeys(st, ["action_name", "command_template", "is_idempotent", "step_number", "target_service"])) return false;
    const step = st as Record<string, unknown>;
    if (!isPositiveInteger(step.step_number) || typeof step.action_name !== "string" || typeof step.target_service !== "string" || typeof step.command_template !== "string" || typeof step.is_idempotent !== "boolean") return false;
  }
  if (!["OPERATOR_CONFIRMATION", "AUTOMATED_IDEMPOTENT", "ESCALATION_BLOCKED"].includes(String(c.approval_required))) return false;
  return isUtcTimestamp(c.created_at);
}

function isModelEvaluationBenchmark(value: unknown): value is ModelEvaluationBenchmark {
  if (!hasExactKeys(value, [
    "average_latency_ms",
    "benchmark_id",
    "model_evaluation_schema_version",
    "citation_precision_bps",
    "disposition",
    "evaluated_at",
    "evaluation_dataset_id",
    "factuality_score_bps",
    "hallucination_rate_bps",
    "injection_resistance_score_bps",
    "model_identifier",
    "token_cost_usd_per_million",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.model_evaluation_schema_version !== 1 || !isCanonicalId(c.benchmark_id) || typeof c.model_identifier !== "string" || !isCanonicalId(c.evaluation_dataset_id)) return false;
  if (!isNonNegativeInteger(c.factuality_score_bps) || c.factuality_score_bps > 10000) return false;
  if (!isNonNegativeInteger(c.citation_precision_bps) || c.citation_precision_bps > 10000) return false;
  if (!isNonNegativeInteger(c.injection_resistance_score_bps) || c.injection_resistance_score_bps > 10000) return false;
  if (!isNonNegativeInteger(c.hallucination_rate_bps) || c.hallucination_rate_bps > 10000) return false;
  if (!isNonNegativeInteger(c.average_latency_ms) || typeof c.token_cost_usd_per_million !== "string") return false;
  if (!["QUALIFIED_FOR_ASSISTANCE", "UNRELIABLE_CITATION", "VULNERABLE_TO_INJECTION", "EXCESSIVE_LATENCY"].includes(String(c.disposition))) return false;
  return isUtcTimestamp(c.evaluated_at);
}

function isStrategyCapsuleManifest(value: unknown): value is StrategyCapsuleManifest {
  if (!hasExactKeys(value, [
    "bundle_sha256",
    "capsule_id",
    "capsule_schema_version",
    "configuration_sha256",
    "dependency_lockfile_sha256",
    "evaluation_receipt_id",
    "export_disposition",
    "packaged_at",
    "replay_instruction_command",
    "runtime_target",
    "strategy_id",
    "strategy_version",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.capsule_schema_version !== 1 || !isCanonicalId(c.capsule_id) || !isCanonicalId(c.strategy_id) || typeof c.strategy_version !== "string") return false;
  if (!isHash(c.bundle_sha256) || !isHash(c.configuration_sha256) || !isHash(c.dependency_lockfile_sha256)) return false;
  if (typeof c.runtime_target !== "string" || !isCanonicalId(c.evaluation_receipt_id) || typeof c.replay_instruction_command !== "string") return false;
  if (!["VERIFIED_PORTABLE", "MISSING_DEPENDENCY_LOCK", "UNVERIFIED_EVALUATION", "RESTRICTED_DATASET_RIGHTS"].includes(String(c.export_disposition))) return false;
  return isUtcTimestamp(c.packaged_at);
}

function isMultiAssetExpansionPlan(value: unknown): value is MultiAssetExpansionPlan {
  if (!hasExactKeys(value, [
    "asset_class",
    "created_at",
    "expansion_schema_version",
    "lifecycle_actions",
    "margin_requirement_usd",
    "operational_verdict",
    "plan_id",
    "reconciliation_clean",
    "settlement_currency",
    "underlying_universe",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.expansion_schema_version !== 1 || !isCanonicalId(c.plan_id)) return false;
  if (!["EQUITY_OPTION", "SPOT_FX", "COMMODITY_FUTURES", "INDEX_FUTURES"].includes(String(c.asset_class))) return false;
  if (!Array.isArray(c.underlying_universe) || c.underlying_universe.length === 0 || !c.underlying_universe.every((u) => typeof u === "string")) return false;
  if (!Array.isArray(c.lifecycle_actions) || c.lifecycle_actions.length === 0) return false;
  for (const a of c.lifecycle_actions) {
    if (!hasExactKeys(a, ["action_id", "action_kind", "contract_quantity", "estimated_cash_flow_usd", "instrument_id", "target_date"])) return false;
    const act = a as Record<string, unknown>;
    if (!isCanonicalId(act.action_id) || !isCanonicalId(act.instrument_id)) return false;
    if (!["OPTION_ROLL", "OPTION_EXERCISE", "FUTURES_ROLL", "FX_SPOT_CONVERSION"].includes(String(act.action_kind))) return false;
    if (!isUtcTimestamp(act.target_date) || !isPositiveInteger(act.contract_quantity) || !isDecimal(act.estimated_cash_flow_usd)) return false;
  }
  if (!isDecimal(c.margin_requirement_usd) || typeof c.settlement_currency !== "string" || typeof c.reconciliation_clean !== "boolean") return false;
  if (!["READY_FOR_LIFECYCLE_EXECUTION", "MARGIN_HEADROOM_BREACH", "UNRECONCILED_LEG_MISMATCH", "GATEWAY_DISCONNECTED"].includes(String(c.operational_verdict))) return false;
  return isUtcTimestamp(c.created_at);
}

function isFxPricingDashboard(value: unknown): value is FxPricingDashboard {
  if (!hasExactKeys(value, [
    "as_of",
    "configuration_content_hash",
    "evaluations",
    "fx_pricing_schema_version",
    "max_age_seconds",
    "snapshot_count",
    "snapshots",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.fx_pricing_schema_version !== 1 || !isUtcTimestamp(c.as_of)) return false;
  if (!isHash(c.configuration_content_hash)) return false;
  if (!isNonNegativeInteger(c.max_age_seconds) || !isNonNegativeInteger(c.snapshot_count)) return false;
  if (!Array.isArray(c.evaluations) || !Array.isArray(c.snapshots)) return false;
  for (const e of c.evaluations) {
    if (!hasExactKeys(e, [
      "age_seconds",
      "ask",
      "bid",
      "fresh",
      "instrument_id",
      "midpoint",
      "pair",
      "product",
      "query_id",
      "received_at",
      "snapshot_id",
      "source_id",
      "source_sequence",
      "source_time",
      "spread_bps",
      "value_date",
    ])) return false;
    const ev = e as Record<string, unknown>;
    if (!isCanonicalId(ev.query_id) || !isCanonicalId(ev.instrument_id) || !isCanonicalId(ev.snapshot_id) || !isCanonicalId(ev.source_id)) return false;
    if (typeof ev.product !== "string" || typeof ev.pair !== "string" || typeof ev.value_date !== "string") return false;
    if (!isDecimal(ev.midpoint) || !isDecimal(ev.bid) || !isDecimal(ev.ask) || !isDecimal(ev.spread_bps)) return false;
    if (!isUtcTimestamp(ev.source_time) || !isUtcTimestamp(ev.received_at)) return false;
    if (typeof ev.fresh !== "boolean" || typeof ev.age_seconds !== "number" || typeof ev.source_sequence !== "number") return false;
  }
  for (const s of c.snapshots) {
    if (!hasExactKeys(s, [
      "canonical_record",
      "instrument_id",
      "pair",
      "product",
      "received_at",
      "reference_version",
      "snapshot_id",
      "source_id",
      "source_sequence",
      "source_time",
    ])) return false;
    const sn = s as Record<string, unknown>;
    if (!isCanonicalId(sn.snapshot_id) || !isCanonicalId(sn.instrument_id) || !isCanonicalId(sn.source_id)) return false;
    if (typeof sn.canonical_record !== "string" || typeof sn.pair !== "string" || typeof sn.product !== "string" || typeof sn.reference_version !== "string") return false;
    if (!isUtcTimestamp(sn.source_time) || !isUtcTimestamp(sn.received_at) || typeof sn.source_sequence !== "number") return false;
  }
  return true;
}

function isStatementReconciliation(value: unknown): value is StatementReconciliation {
  if (!hasExactKeys(value, [
    "account_id",
    "as_of",
    "clean",
    "configuration_content_hash",
    "incident_count",
    "incidents",
    "statement_csv_hash",
    "statement_reconciliation_schema_version",
  ])) return false;
  const c = value as Record<string, unknown>;
  if (c.statement_reconciliation_schema_version !== 1 || !isCanonicalId(c.account_id)) return false;
  if (!isUtcTimestamp(c.as_of) || typeof c.clean !== "boolean") return false;
  if (!isHash(c.configuration_content_hash) || !isHash(c.statement_csv_hash)) return false;
  if (!isNonNegativeInteger(c.incident_count) || !Array.isArray(c.incidents)) return false;
  for (const inc of c.incidents) {
    const item = inc as Record<string, unknown>;
    if (item.incident_kind === "CASH_MISMATCH") {
      if (!hasExactKeys(item, ["broker_balance", "currency", "incident_kind", "internal_balance"])) return false;
      const cur = String(item.currency);
      if (cur.length !== 3 || !isDecimal(item.internal_balance) || !isDecimal(item.broker_balance)) return false;
    } else if (item.incident_kind === "POSITION_MISMATCH") {
      if (!hasExactKeys(item, ["broker_quantity", "incident_kind", "instrument_id", "internal_quantity"])) return false;
      if (!isCanonicalId(item.instrument_id) || !isDecimal(item.internal_quantity) || !isDecimal(item.broker_quantity)) return false;
    } else {
      return false;
    }
  }
  return true;
}
