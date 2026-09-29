// PAPER, controlled-live, operations and options dashboards: types, parsers and guards.

import { hasExactKeys, isCanonicalId, isDecimal, isHash, isNonNegativeInteger, isPositiveInteger, isUtcTimestamp } from "./core.js";

export type PaperDashboard = Readonly<{
  dashboard_schema_version: 2;
  environment: "PAPER";
  account_id: string;
  configuration_fingerprint: string;
  broker_connected: boolean;
  persistence_healthy: boolean;
  audit_sequence: number;
  audit_head_hash: string;
  internal_cash: string;
  working_orders: number;
  unknown_orders: number;
  active_kill_switches: readonly string[];
  unexplained_incidents: number;
  last_reconciled_at: string | null;
  last_reconciliation_clean: boolean | null;
  clean_paper_days: number;
  required_paper_days: 30;
  promotion_eligible: boolean;
  complete_auditability: boolean;
  positions: readonly PaperDashboardPosition[];
}>;

export type PaperDashboardPosition = Readonly<{
  instrument_id: string;
  quantity: string;
  average_cost: string;
  realized_pnl: string;
}>;

export type LiveMonitoringDashboard = Readonly<{
  dashboard_schema_version: 2;
  environment: "LIVE";
  mode: "SHADOW" | "CANARY";
  account_id: string;
  configuration_fingerprint: string;
  broker_connected: boolean;
  audit_healthy: boolean;
  audit_sequence: number;
  audit_head_hash: string;
  active_kill_switches: readonly string[];
  working_orders: number;
  unknown_orders: number;
  unresolved_incidents: number;
  last_reconciled_at: string | null;
  last_reconciliation_clean: boolean | null;
  clean_live_days: number;
  required_live_days: 60;
  promotion_eligible: boolean;
  complete_auditability: boolean;
  internal_cash: string;
  positions: readonly PaperDashboardPosition[];
}>;

export type OperationsDashboard = Readonly<{
  dashboard_schema_version: 1;
  environment: "SIMULATION" | "PAPER" | "LIVE";
  as_of: string;
  account_id: string;
  currency: string;
  starting_equity: string;
  configuration: OperationsConfiguration;
  reproducibility: ReproducibilityEvidence;
  risk: OperationsRisk;
  operational_health: OperationsHealth;
  attribution: OperationsAttribution;
  alerts: readonly OperationsAlert[];
  schedules: readonly OperationsSchedule[];
  journal: OperationsJournal;
  positions: readonly OperationsPosition[];
  projection_fingerprint: string;
}>;

export type OperationsConfiguration = Readonly<{
  configuration_content_hash: string;
  configuration_id: string;
  configuration_version: string;
  fingerprint: string;
  parameter_set_fingerprint: string;
}>;

export type ReproducibilityEvidence = Readonly<{
  strategy_id: string;
  strategy_version: string;
  strategy_bundle_hash: string;
  dataset_id: string;
  dataset_version: string;
  dataset_hash: string;
  replay_event_hash: string;
}>;

export type OperationsRisk = Readonly<{
  state: "NORMAL" | "WARNING" | "CRITICAL";
  cash: string;
  current_equity: string;
  effective_peak_equity: string;
  gross_exposure: string;
  largest_position_exposure: string;
  drawdown_bps: string;
  open_positions: number;
  limits: readonly OperationsRiskLimit[];
}>;

export type OperationsRiskLimit = Readonly<{
  limit_id: string;
  current: string;
  limit: string;
  breached: boolean;
}>;

export type OperationsHealth = Readonly<{
  audit_healthy: boolean;
  reconciliation_healthy: boolean;
  broker_connected: boolean;
  active_kill_switches: readonly string[];
  working_orders: number;
  unknown_orders: number;
  unresolved_incidents: number;
}>;

export type OperationsAttribution = Readonly<{
  net_pnl: string;
  totals: Readonly<Record<AttributionCategory, string>>;
  rows: readonly OperationsAttributionRow[];
}>;

type AttributionCategory = "REALIZED_PNL" | "UNREALIZED_PNL" | "FEES" | "DIVIDENDS" | "CORPORATE_ACTIONS";

export type OperationsAttributionRow = Readonly<{
  instrument_id: string;
  category: AttributionCategory;
  amount: string;
}>;

export type OperationsAlert = Readonly<{
  alert_id: string;
  severity: "WARNING" | "CRITICAL";
  code: string;
  subject: string;
  summary: string;
}>;

export type OperationsSchedule = Readonly<{
  schedule_id: string;
  purpose: string;
  time_utc: string;
  enabled: boolean;
  next_due_at: string;
  due: boolean;
  last_completed_at: string | null;
}>;

export type OperationsJournal = Readonly<{
  healthy: boolean;
  sequence: number;
  head_hash: string;
  failure_reason: string | null;
}>;

export type OperationsPosition = Readonly<{
  instrument_id: string;
  quantity: string;
  mark_price: string;
  average_cost: string;
  realized_pnl: string;
}>;

export type OptionsDashboard = Readonly<{
  option_dashboard_schema_version: 1;
  as_of: string;
  configuration_file_hash: string;
  model_version: "follon-european-black-scholes-fixed-v1";
  chain: OptionsChainEvidence;
  run_identity: OptionsRunIdentity;
  analytics: readonly OptionAnalyticsEvidence[];
  strategy: OptionsStrategyEvidence;
  reconciliation: OptionsReconciliationEvidence;
}>;

export type OptionsChainEvidence = Readonly<{
  chain_id: string;
  chain_snapshot_hash: string;
  currency: string;
  reference_version: string;
  underlying_instrument_id: string;
  underlying_mark: string;
}>;

export type OptionsRunIdentity = Readonly<{
  strategy_bundle_hash: string;
  configuration_hash: string;
  dataset_hash: string;
  replay_event_hash: string;
  chain_snapshot_hash: string;
  model_version: "follon-european-black-scholes-fixed-v1";
}>;

export type OptionAnalyticsEvidence = Readonly<{
  option_id: string;
  expiration_at: string;
  right: "CALL" | "PUT";
  strike: string;
  bid: string;
  ask: string;
  market_premium: string;
  implied_volatility: string;
  model_price: string;
  delta: string;
  gamma: string;
  vega: string;
  theta: string;
  rho: string;
}>;

export type OptionsStrategyEvidence = Readonly<{
  strategy_id: string;
  strategy_version: string;
  scenarios: readonly OptionExpiryScenario[];
}>;

export type OptionExpiryScenario = Readonly<{
  underlying_price: string;
  total_pnl: string;
  legs: readonly OptionScenarioLeg[];
}>;

export type OptionScenarioLeg = Readonly<{
  leg_id: string;
  option_id: string;
  intrinsic_value: string;
  pnl: string;
}>;

export type OptionsReconciliationEvidence = Readonly<{
  backtest_book: OptionBookEvidence;
  clean: boolean;
  reconciled_at: string;
  paper_book: OptionBookEvidence;
  live_book: OptionBookEvidence;
  issues: readonly OptionReconciliationIssue[];
}>;

export type OptionBookEvidence = Readonly<{
  account_id: string;
  book_hash: string;
  environment: "BACKTEST" | "PAPER" | "LIVE";
  run_identity: OptionsRunIdentity;
  run_identity_hash: string;
  source_export_hash: string;
  source_export_id: string;
}>;

export type OptionReconciliationIssue = Readonly<{
  category: "IDENTITY_MISMATCH" | "CASH_MISMATCH" | "POSITION_MISMATCH";
  subject: string;
  expected: string;
  observed: string;
}>;

/** Parses a server-owned PAPER operations evidence snapshot. */
export function parsePaperDashboard(json: string): PaperDashboard {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Paper dashboard is not valid JSON.");
  }
  if (!isPaperDashboard(value)) {
    throw new Error("Paper dashboard does not match the v2 evidence contract.");
  }
  return value;
}

/** Parses a controlled-live monitoring evidence projection. */
export function parseLiveMonitoringDashboard(json: string): LiveMonitoringDashboard {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Controlled-live monitoring dashboard is not valid JSON.");
  }
  if (!isLiveMonitoringDashboard(value)) {
    throw new Error("Controlled-live monitoring dashboard does not match the v2 evidence contract.");
  }
  return value;
}

/** Parses the versioned operator-workbench evidence dashboard. */
export function parseOperationsDashboard(json: string): OperationsDashboard {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Operations dashboard is not valid JSON.");
  }
  if (!isOperationsDashboard(value)) {
    throw new Error("Operations dashboard does not match the v1 evidence contract.");
  }
  return value;
}

/** Parses a frozen option-chain analytics and cross-environment evidence snapshot. */
export function parseOptionsDashboard(json: string): OptionsDashboard {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    throw new Error("Options dashboard is not valid JSON.");
  }
  if (!isOptionsDashboard(value)) {
    throw new Error("Options dashboard does not match the v1 deterministic European-options contract.");
  }
  return value;
}

function isPaperDashboard(value: unknown): value is PaperDashboard {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  const expected = new Set([
    "dashboard_schema_version", "environment", "account_id", "configuration_fingerprint", "broker_connected", "persistence_healthy", "audit_sequence", "audit_head_hash", "internal_cash", "working_orders",
    "unknown_orders", "active_kill_switches", "unexplained_incidents", "last_reconciled_at", "last_reconciliation_clean", "clean_paper_days",
    "required_paper_days", "promotion_eligible", "complete_auditability", "positions",
  ]);
  if (Object.keys(candidate).length !== expected.size || Object.keys(candidate).some((key) => !expected.has(key))) {
    return false;
  }
  return (
    candidate.dashboard_schema_version === 2 &&
    candidate.environment === "PAPER" &&
    isCanonicalId(candidate.account_id) &&
    typeof candidate.configuration_fingerprint === "string" && /^[a-f0-9]{64}$/.test(candidate.configuration_fingerprint) &&
    typeof candidate.broker_connected === "boolean" &&
    typeof candidate.persistence_healthy === "boolean" &&
    isNonNegativeInteger(candidate.audit_sequence) &&
    isHash(candidate.audit_head_hash) &&
    isDecimal(candidate.internal_cash) &&
    isNonNegativeInteger(candidate.working_orders) &&
    isNonNegativeInteger(candidate.unknown_orders) &&
    Array.isArray(candidate.active_kill_switches) &&
    candidate.active_kill_switches.every((value) => typeof value === "string" && value.length > 0) &&
    new Set(candidate.active_kill_switches).size === candidate.active_kill_switches.length &&
    isNonNegativeInteger(candidate.unexplained_incidents) &&
    (candidate.last_reconciled_at === null || isUtcTimestamp(candidate.last_reconciled_at)) &&
    (candidate.last_reconciliation_clean === null || typeof candidate.last_reconciliation_clean === "boolean") &&
    (candidate.last_reconciled_at === null) === (candidate.last_reconciliation_clean === null) &&
    isNonNegativeInteger(candidate.clean_paper_days) &&
    candidate.required_paper_days === 30 &&
    typeof candidate.promotion_eligible === "boolean" &&
    typeof candidate.complete_auditability === "boolean" &&
    Array.isArray(candidate.positions) &&
    candidate.positions.every(isPaperDashboardPosition)
  );
}

function isLiveMonitoringDashboard(value: unknown): value is LiveMonitoringDashboard {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  const expected = new Set([
    "dashboard_schema_version", "environment", "mode", "account_id", "configuration_fingerprint", "broker_connected", "audit_healthy",
    "audit_sequence", "audit_head_hash", "active_kill_switches", "working_orders", "unknown_orders", "unresolved_incidents",
    "last_reconciled_at", "last_reconciliation_clean", "clean_live_days", "required_live_days", "promotion_eligible", "complete_auditability", "internal_cash", "positions",
  ]);
  if (Object.keys(candidate).length !== expected.size || Object.keys(candidate).some((key) => !expected.has(key))) {
    return false;
  }
  return (
    candidate.dashboard_schema_version === 2 &&
    candidate.environment === "LIVE" &&
    (candidate.mode === "SHADOW" || candidate.mode === "CANARY") &&
    isCanonicalId(candidate.account_id) &&
    isHash(candidate.configuration_fingerprint) &&
    typeof candidate.broker_connected === "boolean" &&
    typeof candidate.audit_healthy === "boolean" &&
    isPositiveInteger(candidate.audit_sequence) &&
    isHash(candidate.audit_head_hash) &&
    Array.isArray(candidate.active_kill_switches) &&
    candidate.active_kill_switches.every((entry) => typeof entry === "string" && entry.length > 0) &&
    new Set(candidate.active_kill_switches).size === candidate.active_kill_switches.length &&
    isNonNegativeInteger(candidate.working_orders) &&
    isNonNegativeInteger(candidate.unknown_orders) &&
    isNonNegativeInteger(candidate.unresolved_incidents) &&
    (candidate.last_reconciled_at === null || isUtcTimestamp(candidate.last_reconciled_at)) &&
    (candidate.last_reconciliation_clean === null || typeof candidate.last_reconciliation_clean === "boolean") &&
    (candidate.last_reconciled_at === null) === (candidate.last_reconciliation_clean === null) &&
    isNonNegativeInteger(candidate.clean_live_days) &&
    candidate.required_live_days === 60 &&
    typeof candidate.promotion_eligible === "boolean" &&
    typeof candidate.complete_auditability === "boolean" &&
    isDecimal(candidate.internal_cash) &&
    Array.isArray(candidate.positions) &&
    candidate.positions.every(isPaperDashboardPosition)
  );
}

function isOperationsDashboard(value: unknown): value is OperationsDashboard {
  if (!hasExactKeys(value, [
    "account_id", "alerts", "as_of", "attribution", "configuration", "currency", "dashboard_schema_version", "environment", "journal", "operational_health", "positions", "projection_fingerprint", "reproducibility", "risk", "schedules", "starting_equity",
  ])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    candidate.dashboard_schema_version === 1 &&
    (candidate.environment === "SIMULATION" || candidate.environment === "PAPER" || candidate.environment === "LIVE") &&
    isUtcTimestamp(candidate.as_of) &&
    isCanonicalId(candidate.account_id) &&
    typeof candidate.currency === "string" && /^[A-Z]{3}$/.test(candidate.currency) &&
    isDecimal(candidate.starting_equity) &&
    isOperationsConfiguration(candidate.configuration) &&
    isReproducibilityEvidence(candidate.reproducibility) &&
    isOperationsRisk(candidate.risk) &&
    isOperationsHealth(candidate.operational_health) &&
    isOperationsAttribution(candidate.attribution) &&
    Array.isArray(candidate.alerts) && candidate.alerts.every(isOperationsAlert) &&
    Array.isArray(candidate.schedules) && candidate.schedules.every(isOperationsSchedule) &&
    Array.isArray(candidate.positions) && candidate.positions.every(isOperationsPosition) &&
    isOperationsJournal(candidate.journal) && isHash(candidate.projection_fingerprint)
  );
}

function isOperationsConfiguration(value: unknown): value is OperationsConfiguration {
  if (!hasExactKeys(value, ["configuration_content_hash", "configuration_id", "configuration_version", "fingerprint", "parameter_set_fingerprint"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isHash(candidate.configuration_content_hash) && isCanonicalId(candidate.configuration_id) &&
    typeof candidate.configuration_version === "string" && candidate.configuration_version.length > 0 &&
    isHash(candidate.fingerprint) && isHash(candidate.parameter_set_fingerprint);
}

function isReproducibilityEvidence(value: unknown): value is ReproducibilityEvidence {
  if (!hasExactKeys(value, ["dataset_hash", "dataset_id", "dataset_version", "replay_event_hash", "strategy_bundle_hash", "strategy_id", "strategy_version"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isHash(candidate.dataset_hash) && isCanonicalId(candidate.dataset_id) &&
    typeof candidate.dataset_version === "string" && candidate.dataset_version.length > 0 &&
    isHash(candidate.replay_event_hash) && isHash(candidate.strategy_bundle_hash) &&
    isCanonicalId(candidate.strategy_id) && typeof candidate.strategy_version === "string" && candidate.strategy_version.length > 0;
}

function isOperationsRisk(value: unknown): value is OperationsRisk {
  if (!hasExactKeys(value, ["cash", "current_equity", "drawdown_bps", "effective_peak_equity", "gross_exposure", "largest_position_exposure", "limits", "open_positions", "state"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  const expectedLimitIds = new Set(["gross_exposure", "single_instrument_exposure", "drawdown_bps", "working_orders", "unknown_orders", "unresolved_incidents"]);
  return (
    (candidate.state === "NORMAL" || candidate.state === "WARNING" || candidate.state === "CRITICAL") &&
    isDecimal(candidate.cash) && isDecimal(candidate.current_equity) && isDecimal(candidate.effective_peak_equity) &&
    isDecimal(candidate.gross_exposure) && isDecimal(candidate.largest_position_exposure) && isDecimal(candidate.drawdown_bps) &&
    isNonNegativeInteger(candidate.open_positions) && Array.isArray(candidate.limits) &&
    candidate.limits.length === expectedLimitIds.size && candidate.limits.every(isOperationsRiskLimit) &&
    new Set(candidate.limits.map((limit) => limit.limit_id)).size === expectedLimitIds.size &&
    candidate.limits.every((limit) => expectedLimitIds.has(limit.limit_id))
  );
}

function isOperationsRiskLimit(value: unknown): value is OperationsRiskLimit {
  if (!hasExactKeys(value, ["breached", "current", "limit", "limit_id"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.breached === "boolean" && isDecimal(candidate.current) && isDecimal(candidate.limit) && isCanonicalId(candidate.limit_id);
}

function isOperationsHealth(value: unknown): value is OperationsHealth {
  if (!hasExactKeys(value, ["active_kill_switches", "audit_healthy", "broker_connected", "reconciliation_healthy", "unknown_orders", "unresolved_incidents", "working_orders"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.audit_healthy === "boolean" && typeof candidate.reconciliation_healthy === "boolean" &&
    typeof candidate.broker_connected === "boolean" && Array.isArray(candidate.active_kill_switches) &&
    candidate.active_kill_switches.every((scope) => typeof scope === "string" && scope.length > 0) &&
    new Set(candidate.active_kill_switches).size === candidate.active_kill_switches.length &&
    isNonNegativeInteger(candidate.working_orders) && isNonNegativeInteger(candidate.unknown_orders) && isNonNegativeInteger(candidate.unresolved_incidents);
}

function isOperationsAttribution(value: unknown): value is OperationsAttribution {
  if (!hasExactKeys(value, ["net_pnl", "rows", "totals"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isDecimal(candidate.net_pnl) && Array.isArray(candidate.rows) && candidate.rows.every(isOperationsAttributionRow) && isAttributionTotals(candidate.totals);
}

function isAttributionTotals(value: unknown): value is Readonly<Record<AttributionCategory, string>> {
  const categories: AttributionCategory[] = ["REALIZED_PNL", "UNREALIZED_PNL", "FEES", "DIVIDENDS", "CORPORATE_ACTIONS"];
  return hasExactKeys(value, categories) && categories.every((category) => isDecimal((value as Record<string, unknown>)[category]));
}

function isOperationsAttributionRow(value: unknown): value is OperationsAttributionRow {
  if (!hasExactKeys(value, ["amount", "category", "instrument_id"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isDecimal(candidate.amount) && isCanonicalId(candidate.instrument_id) && isAttributionCategory(candidate.category);
}

function isAttributionCategory(value: unknown): value is AttributionCategory {
  return value === "REALIZED_PNL" || value === "UNREALIZED_PNL" || value === "FEES" || value === "DIVIDENDS" || value === "CORPORATE_ACTIONS";
}

function isOperationsAlert(value: unknown): value is OperationsAlert {
  if (!hasExactKeys(value, ["alert_id", "code", "severity", "subject", "summary"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isHash(candidate.alert_id) && isCanonicalId(candidate.code) &&
    (candidate.severity === "WARNING" || candidate.severity === "CRITICAL") &&
    typeof candidate.subject === "string" && candidate.subject.length > 0 &&
    typeof candidate.summary === "string" && candidate.summary.length > 0;
}

function isOperationsSchedule(value: unknown): value is OperationsSchedule {
  if (!hasExactKeys(value, ["due", "enabled", "last_completed_at", "next_due_at", "purpose", "schedule_id", "time_utc"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.due === "boolean" && typeof candidate.enabled === "boolean" &&
    (candidate.last_completed_at === null || isUtcTimestamp(candidate.last_completed_at)) &&
    isUtcTimestamp(candidate.next_due_at) && typeof candidate.purpose === "string" && candidate.purpose.length > 0 &&
    isCanonicalId(candidate.schedule_id) && typeof candidate.time_utc === "string" && /^([01][0-9]|2[0-3]):[0-5][0-9]$/.test(candidate.time_utc);
}

function isOperationsJournal(value: unknown): value is OperationsJournal {
  if (!hasExactKeys(value, ["failure_reason", "head_hash", "healthy", "sequence"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  if (typeof candidate.healthy !== "boolean" || !isNonNegativeInteger(candidate.sequence) || !isHash(candidate.head_hash)) {
    return false;
  }
  return candidate.healthy
    ? candidate.failure_reason === null
    : typeof candidate.failure_reason === "string" && candidate.failure_reason.length > 0 &&
      candidate.head_hash === "0000000000000000000000000000000000000000000000000000000000000000";
}

function isOperationsPosition(value: unknown): value is OperationsPosition {
  if (!hasExactKeys(value, ["average_cost", "instrument_id", "mark_price", "quantity", "realized_pnl"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isCanonicalId(candidate.instrument_id) && isDecimal(candidate.quantity) && isDecimal(candidate.mark_price) &&
    isDecimal(candidate.average_cost) && isDecimal(candidate.realized_pnl);
}

function isOptionsDashboard(value: unknown): value is OptionsDashboard {
  if (!hasExactKeys(value, ["analytics", "as_of", "chain", "configuration_file_hash", "model_version", "option_dashboard_schema_version", "reconciliation", "run_identity", "strategy"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return candidate.option_dashboard_schema_version === 1 && isUtcTimestamp(candidate.as_of) && isHash(candidate.configuration_file_hash) &&
    candidate.model_version === "follon-european-black-scholes-fixed-v1" && isOptionsChainEvidence(candidate.chain) &&
    isOptionsRunIdentity(candidate.run_identity) && Array.isArray(candidate.analytics) && candidate.analytics.length > 0 &&
    candidate.analytics.every(isOptionAnalyticsEvidence) && new Set(candidate.analytics.map((option) => option.option_id)).size === candidate.analytics.length &&
    candidate.run_identity.chain_snapshot_hash === candidate.chain.chain_snapshot_hash &&
    candidate.run_identity.model_version === candidate.model_version &&
    isOptionsStrategyEvidence(candidate.strategy) && isOptionsReconciliationEvidence(
      candidate.reconciliation,
      candidate.as_of,
      candidate.chain.chain_snapshot_hash,
      candidate.model_version,
      candidate.run_identity,
    ) &&
    (candidate.reconciliation.clean === (candidate.reconciliation.issues.length === 0));
}

function isOptionsChainEvidence(value: unknown): value is OptionsChainEvidence {
  if (!hasExactKeys(value, ["chain_id", "chain_snapshot_hash", "currency", "reference_version", "underlying_instrument_id", "underlying_mark"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isCanonicalId(candidate.chain_id) && isHash(candidate.chain_snapshot_hash) && typeof candidate.currency === "string" &&
    /^[A-Z]{3}$/.test(candidate.currency) && isCanonicalId(candidate.reference_version) &&
    isCanonicalId(candidate.underlying_instrument_id) && isDecimal(candidate.underlying_mark);
}

function isOptionsRunIdentity(value: unknown): value is OptionsRunIdentity {
  if (!hasExactKeys(value, ["chain_snapshot_hash", "configuration_hash", "dataset_hash", "model_version", "replay_event_hash", "strategy_bundle_hash"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isHash(candidate.chain_snapshot_hash) && isHash(candidate.configuration_hash) && isHash(candidate.dataset_hash) &&
    candidate.model_version === "follon-european-black-scholes-fixed-v1" && isHash(candidate.replay_event_hash) && isHash(candidate.strategy_bundle_hash);
}

function isOptionAnalyticsEvidence(value: unknown): value is OptionAnalyticsEvidence {
  if (!hasExactKeys(value, ["ask", "bid", "delta", "expiration_at", "gamma", "implied_volatility", "market_premium", "model_price", "option_id", "rho", "right", "strike", "theta", "vega"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isDecimal(candidate.ask) && isDecimal(candidate.bid) && isDecimal(candidate.delta) && isUtcTimestamp(candidate.expiration_at) &&
    isDecimal(candidate.gamma) && isDecimal(candidate.implied_volatility) && isDecimal(candidate.market_premium) && isDecimal(candidate.model_price) &&
    isCanonicalId(candidate.option_id) && isDecimal(candidate.rho) && (candidate.right === "CALL" || candidate.right === "PUT") &&
    isDecimal(candidate.strike) && isDecimal(candidate.theta) && isDecimal(candidate.vega);
}

function isOptionsStrategyEvidence(value: unknown): value is OptionsStrategyEvidence {
  if (!hasExactKeys(value, ["scenarios", "strategy_id", "strategy_version"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isCanonicalId(candidate.strategy_id) && typeof candidate.strategy_version === "string" && candidate.strategy_version.length > 0 &&
    Array.isArray(candidate.scenarios) && candidate.scenarios.length > 0 && candidate.scenarios.every(isOptionExpiryScenario);
}

function isOptionExpiryScenario(value: unknown): value is OptionExpiryScenario {
  if (!hasExactKeys(value, ["legs", "total_pnl", "underlying_price"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isDecimal(candidate.total_pnl) && isDecimal(candidate.underlying_price) && Array.isArray(candidate.legs) && candidate.legs.length > 0 && candidate.legs.every(isOptionScenarioLeg);
}

function isOptionScenarioLeg(value: unknown): value is OptionScenarioLeg {
  if (!hasExactKeys(value, ["intrinsic_value", "leg_id", "option_id", "pnl"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isDecimal(candidate.intrinsic_value) && isCanonicalId(candidate.leg_id) && isCanonicalId(candidate.option_id) && isDecimal(candidate.pnl);
}

function isOptionsReconciliationEvidence(
  value: unknown,
  asOf: string,
  chainSnapshotHash: string,
  modelVersion: OptionsDashboard["model_version"],
  referenceIdentity: OptionsRunIdentity,
): value is OptionsReconciliationEvidence {
  if (!hasExactKeys(value, ["backtest_book", "clean", "issues", "live_book", "paper_book", "reconciled_at"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isOptionBookEvidence(candidate.backtest_book, "BACKTEST", chainSnapshotHash, modelVersion) && typeof candidate.clean === "boolean" &&
    isOptionBookEvidence(candidate.paper_book, "PAPER", chainSnapshotHash, modelVersion) &&
    isOptionBookEvidence(candidate.live_book, "LIVE", chainSnapshotHash, modelVersion) &&
    isUtcTimestamp(candidate.reconciled_at) && candidate.reconciled_at >= asOf &&
    sameRunIdentity(candidate.backtest_book.run_identity, referenceIdentity) &&
    (!candidate.clean || (
      candidate.backtest_book.run_identity_hash === candidate.paper_book.run_identity_hash &&
      candidate.paper_book.run_identity_hash === candidate.live_book.run_identity_hash
    )) &&
    Array.isArray(candidate.issues) && candidate.issues.every(isOptionReconciliationIssue);
}

function isOptionBookEvidence(
  value: unknown,
  expectedEnvironment: OptionBookEvidence["environment"],
  chainSnapshotHash: string,
  modelVersion: OptionsDashboard["model_version"],
): value is OptionBookEvidence {
  if (!hasExactKeys(value, ["account_id", "book_hash", "environment", "run_identity", "run_identity_hash", "source_export_hash", "source_export_id"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return isCanonicalId(candidate.account_id) && isHash(candidate.book_hash) && candidate.environment === expectedEnvironment &&
    isOptionsRunIdentity(candidate.run_identity) && candidate.run_identity.chain_snapshot_hash === chainSnapshotHash &&
    candidate.run_identity.model_version === modelVersion && isHash(candidate.run_identity_hash) &&
    isHash(candidate.source_export_hash) && isCanonicalId(candidate.source_export_id);
}

function sameRunIdentity(left: OptionsRunIdentity, right: OptionsRunIdentity): boolean {
  return left.chain_snapshot_hash === right.chain_snapshot_hash &&
    left.configuration_hash === right.configuration_hash &&
    left.dataset_hash === right.dataset_hash &&
    left.model_version === right.model_version &&
    left.replay_event_hash === right.replay_event_hash &&
    left.strategy_bundle_hash === right.strategy_bundle_hash;
}

function isOptionReconciliationIssue(value: unknown): value is OptionReconciliationIssue {
  if (!hasExactKeys(value, ["category", "expected", "observed", "subject"])) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (candidate.category === "IDENTITY_MISMATCH" || candidate.category === "CASH_MISMATCH" || candidate.category === "POSITION_MISMATCH") &&
    typeof candidate.subject === "string" && candidate.subject.length > 0 && typeof candidate.expected === "string" && typeof candidate.observed === "string";
}

function isPaperDashboardPosition(value: unknown): value is PaperDashboardPosition {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    Object.keys(candidate).length === 4 &&
    isCanonicalId(candidate.instrument_id) &&
    isDecimal(candidate.quantity) &&
    isDecimal(candidate.average_cost) &&
    isDecimal(candidate.realized_pnl)
  );
}
