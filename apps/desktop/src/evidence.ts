export type EvidenceEvent = Readonly<{
  event_id: string;
  event_type: string;
  schema_version: number;
  event_time: string;
  receive_time: string;
  account_id: string | null;
  strategy_id: string | null;
  instrument_id: string | null;
  correlation_id: string;
  causation_id: string | null;
  actor: string;
  source: string;
  payload: Record<string, unknown>;
  software_version: string;
  configuration_version: string;
}>;

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
  benchmark_schema_version: 1;
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

/** Parses and validates canonical NDJSON before it is shown as evidence. */
export function parseEvidenceLog(ndjson: string): EvidenceEvent[] {
  const eventIds = new Set<string>();
  const events: EvidenceEvent[] = [];
  for (const [index, line] of ndjson.split(/\r?\n/).filter(Boolean).entries()) {
    let candidate: unknown;
    try {
      candidate = JSON.parse(line);
    } catch {
      throw new Error(`Line ${index + 1} is not valid JSON.`);
    }
    if (!isEvidenceEvent(candidate)) {
      throw new Error(`Line ${index + 1} is not a valid Follon event envelope.`);
    }
    if (eventIds.has(candidate.event_id)) {
      throw new Error(`Line ${index + 1} repeats event ID ${candidate.event_id}.`);
    }
    if (candidate.causation_id !== null && !eventIds.has(candidate.causation_id)) {
      throw new Error(`Line ${index + 1} references an unseen causation event.`);
    }
    eventIds.add(candidate.event_id);
    events.push(candidate);
  }
  if (events.length === 0) {
    throw new Error("The selected event log is empty.");
  }
  return events;
}

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

/** Parses a counterfactual replay experiment scenario (DUR-02). */
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

/** Parses an adversarial research gate evaluation report (DUR-06). */
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

const phaseByEventType: Readonly<Record<string, string>> = {
  "market.bar.v1": "Market bar",
  "intent.created.v1": "Strategy intent",
  "risk.decision.v1": "Risk decision",
  "order.state_changed.v1": "OMS transition",
  "execution.fill.v1": "Simulated fill",
  "portfolio.position_updated.v1": "Position update",
  "portfolio.pnl_updated.v1": "P&L update",
  "audit.trail.v1": "Audit trail",
};

/** Renders server-owned evidence only; it never creates a trading transition. */
export function renderEvidence(root: HTMLElement, events: readonly EvidenceEvent[]): void {
  root.replaceChildren();
  const heading = document.createElement("h1");
  heading.textContent = "Simulation evidence";
  root.append(heading);

  const environment = document.createElement("p");
  environment.textContent = "SIMULATION — no broker connectivity";
  root.append(environment);

  if (events.length === 0) {
    const empty = document.createElement("p");
    empty.textContent = "Waiting for an immutable event trail.";
    root.append(empty);
    return;
  }

  const trail = document.createElement("ol");
  trail.setAttribute("aria-label", "Causal event trail");
  for (const event of events) {
    const item = document.createElement("li");
    const phase = document.createElement("strong");
    phase.textContent = phaseByEventType[event.event_type] ?? event.event_type;
    item.append(phase, document.createTextNode(` — ${event.event_time}`));

    const details = document.createElement("pre");
    details.textContent = JSON.stringify(
      {
        event_id: event.event_id,
        correlation_id: event.correlation_id,
        causation_id: event.causation_id,
        actor: event.actor,
        source: event.source,
        payload: event.payload,
      },
      null,
      2,
    );
    item.append(details);
    trail.append(item);
  }
  root.append(trail);

  const current = latestPortfolio(events);
  if (current !== undefined) {
    const summary = document.createElement("p");
    summary.textContent = `Current simulated P&L: ${String(current.total_pnl ?? "unavailable")}`;
    root.append(summary);
  }
}

/** Renders PAPER risk and reconciliation evidence. */
export function renderPaperDashboard(root: HTMLElement, dashboard: PaperDashboard): void {
  root.replaceChildren();
  const heading = document.createElement("h1");
  heading.textContent = "Paper operations dashboard";
  root.append(heading);

  const environment = document.createElement("p");
  environment.textContent = `Environment: ${dashboard.environment}. Use the Order Ticket for active requests.`;
  root.append(environment);

  const summary = document.createElement("dl");
  const values: ReadonlyArray<readonly [string, string]> = [
    ["Account", dashboard.account_id],
    ["Configuration", dashboard.configuration_fingerprint],
    ["Broker session", dashboard.broker_connected ? "Connected" : "Disconnected / reconnect required"],
    ["Audit sequence", String(dashboard.audit_sequence)],
    ["Audit head", dashboard.audit_head_hash],
    ["Durable journal", dashboard.persistence_healthy ? "Healthy" : "FAILED — operations halted"],
    ["Internal cash", dashboard.internal_cash],
    ["Working orders", String(dashboard.working_orders)],
    ["Unknown orders", String(dashboard.unknown_orders)],
    ["Unexplained discrepancies", String(dashboard.unexplained_incidents)],
    ["Last reconciliation", dashboard.last_reconciled_at ?? "Not yet reconciled"],
    ["Last reconciliation result", dashboard.last_reconciliation_clean === null
      ? "Not yet reconciled"
      : dashboard.last_reconciliation_clean ? "Clean" : "Discrepancy detected"],
    ["Paper-day gate", `${dashboard.clean_paper_days}/${dashboard.required_paper_days}`],
    ["Promotion status", dashboard.promotion_eligible ? "Evidence gate complete" : "Evidence gate incomplete"],
    ["Complete auditability", dashboard.complete_auditability ? "Yes" : "No"],
  ];
  for (const [label, value] of values) {
    const term = document.createElement("dt");
    term.textContent = label;
    const detail = document.createElement("dd");
    detail.textContent = value;
    summary.append(term, detail);
  }
  root.append(summary);

  const killSwitches = document.createElement("p");
  killSwitches.textContent = dashboard.active_kill_switches.length === 0
    ? "Kill switches: clear"
    : `Kill switches: ${dashboard.active_kill_switches.join(", ")}`;
  root.append(killSwitches);

  const positionsHeading = document.createElement("h2");
  positionsHeading.textContent = "Internal positions";
  root.append(positionsHeading);
  if (dashboard.positions.length === 0) {
    const empty = document.createElement("p");
    empty.textContent = "No internal paper positions.";
    root.append(empty);
    return;
  }
  const table = document.createElement("table");
  const header = document.createElement("tr");
  for (const label of ["Instrument", "Quantity", "Average cost", "Realized P&L"]) {
    const cell = document.createElement("th");
    cell.scope = "col";
    cell.textContent = label;
    header.append(cell);
  }
  table.append(header);
  for (const position of dashboard.positions) {
    const row = document.createElement("tr");
    for (const value of [position.instrument_id, position.quantity, position.average_cost, position.realized_pnl]) {
      const cell = document.createElement("td");
      cell.textContent = value;
      row.append(cell);
    }
    table.append(row);
  }
  root.append(table);
}

/** Renders audited controlled-live state without exposing credentials or order controls. */
export function renderLiveMonitoringDashboard(root: HTMLElement, dashboard: LiveMonitoringDashboard): void {
  root.replaceChildren();
  const heading = document.createElement("h1");
  heading.textContent = "Controlled-live monitoring dashboard";
  root.append(heading);

  const boundary = document.createElement("p");
  boundary.textContent = `Environment: LIVE / ${dashboard.mode}. Monitoring only; no credential, approval, or order control is available here.`;
  root.append(boundary);

  const summary = document.createElement("dl");
  const values: ReadonlyArray<readonly [string, string]> = [
    ["Account", dashboard.account_id],
    ["Configuration", dashboard.configuration_fingerprint],
    ["Audit journal", dashboard.audit_healthy ? `Healthy (sequence ${dashboard.audit_sequence})` : "FAILED — controlled operations halted"],
    ["Audit head", dashboard.audit_head_hash],
    ["Broker session", dashboard.broker_connected ? "Connected" : "Disconnected / reconnect required"],
    ["Internal cash", dashboard.internal_cash],
    ["Working orders", String(dashboard.working_orders)],
    ["Unknown orders", String(dashboard.unknown_orders)],
    ["Unresolved discrepancies", String(dashboard.unresolved_incidents)],
    ["Last reconciliation", dashboard.last_reconciled_at ?? "Not yet reconciled"],
    ["Last reconciliation result", dashboard.last_reconciliation_clean === null
      ? "Not yet reconciled"
      : dashboard.last_reconciliation_clean ? "Clean" : "Discrepancy detected"],
    ["Controlled-live-day gate", `${dashboard.clean_live_days}/${dashboard.required_live_days}`],
    ["Complete auditability", dashboard.complete_auditability ? "Yes" : "No"],
    ["Promotion status", dashboard.promotion_eligible ? "Evidence gate complete" : "Evidence gate incomplete"],
  ];
  for (const [label, value] of values) {
    const term = document.createElement("dt");
    term.textContent = label;
    const detail = document.createElement("dd");
    detail.textContent = value;
    summary.append(term, detail);
  }
  root.append(summary);

  const killSwitches = document.createElement("p");
  killSwitches.textContent = dashboard.active_kill_switches.length === 0
    ? "Kill switches: clear"
    : `Kill switches: ${dashboard.active_kill_switches.join(", ")}`;
  root.append(killSwitches);

  const positionsHeading = document.createElement("h2");
  positionsHeading.textContent = "Internal positions";
  root.append(positionsHeading);
  if (dashboard.positions.length === 0) {
    const empty = document.createElement("p");
    empty.textContent = "No internal controlled-live positions.";
    root.append(empty);
    return;
  }
  const table = document.createElement("table");
  const header = document.createElement("tr");
  for (const label of ["Instrument", "Quantity", "Average cost", "Realized P&L"]) {
    const cell = document.createElement("th");
    cell.scope = "col";
    cell.textContent = label;
    header.append(cell);
  }
  table.append(header);
  for (const position of dashboard.positions) {
    const row = document.createElement("tr");
    for (const value of [position.instrument_id, position.quantity, position.average_cost, position.realized_pnl]) {
      const cell = document.createElement("td");
      cell.textContent = value;
      row.append(cell);
    }
    table.append(row);
  }
  root.append(table);
}

/** Renders risk, attribution, schedules, replay identities, and journal evidence. */
export function renderOperationsDashboard(root: HTMLElement, dashboard: OperationsDashboard): void {
  root.replaceChildren();
  const heading = document.createElement("h1");
  heading.textContent = "Operations workbench";
  root.append(heading);

  const boundary = document.createElement("p");
  boundary.textContent = `Environment: ${dashboard.environment}. This is an evidence view; use the active controls in Command Center or Execution Blotter to submit a request.`;
  root.append(boundary);

  const summary = document.createElement("dl");
  appendDefinitionList(summary, [
    ["As of", dashboard.as_of],
    ["Account", dashboard.account_id],
    ["Risk state", dashboard.risk.state],
    ["Current equity", `${dashboard.risk.current_equity} ${dashboard.currency}`],
    ["Gross exposure", `${dashboard.risk.gross_exposure} ${dashboard.currency}`],
    ["Drawdown", `${dashboard.risk.drawdown_bps} bps`],
    ["Open positions", String(dashboard.risk.open_positions)],
    ["Audit health", dashboard.operational_health.audit_healthy ? "Healthy" : "FAILED"],
    ["Reconciliation", dashboard.operational_health.reconciliation_healthy ? "Clean" : "Discrepancy detected"],
    ["Broker session", dashboard.operational_health.broker_connected ? "Connected" : "Disconnected"],
    ["Operations journal", dashboard.journal.healthy ? `Healthy (sequence ${dashboard.journal.sequence})` : "FAILED"],
  ]);
  root.append(summary);

  const alertsHeading = document.createElement("h2");
  alertsHeading.textContent = "Alerts";
  root.append(alertsHeading);
  if (dashboard.alerts.length === 0) {
    const clear = document.createElement("p");
    clear.textContent = "No active deterministic alerts.";
    root.append(clear);
  } else {
    const alerts = document.createElement("ul");
    for (const alert of dashboard.alerts) {
      const item = document.createElement("li");
      item.textContent = `${alert.severity}: ${alert.summary} (${alert.code} / ${alert.subject})`;
      alerts.append(item);
    }
    root.append(alerts);
  }

  const riskHeading = document.createElement("h2");
  riskHeading.textContent = "Risk limits";
  root.append(riskHeading);
  root.append(tableFromRows(
    ["Limit", "Current", "Limit", "Status"],
    dashboard.risk.limits.map((limit) => [
      limit.limit_id,
      limit.current,
      limit.limit,
      limit.breached ? "BREACHED" : "Within limit",
    ]),
  ));

  const attributionHeading = document.createElement("h2");
  attributionHeading.textContent = `Attribution — net ${dashboard.attribution.net_pnl} ${dashboard.currency}`;
  root.append(attributionHeading);
  root.append(tableFromRows(
    ["Instrument", "Category", "Amount"],
    dashboard.attribution.rows.map((row) => [row.instrument_id, row.category, `${row.amount} ${dashboard.currency}`]),
  ));

  const scheduleHeading = document.createElement("h2");
  scheduleHeading.textContent = "Schedule";
  root.append(scheduleHeading);
  root.append(tableFromRows(
    ["Schedule", "Next due", "State", "Purpose"],
    dashboard.schedules.map((schedule) => [
      schedule.schedule_id,
      schedule.next_due_at,
      !schedule.enabled ? "Disabled" : schedule.due ? "Due" : "Scheduled",
      schedule.purpose,
    ]),
  ));

  const replayHeading = document.createElement("h2");
  replayHeading.textContent = "Replay and configuration evidence";
  root.append(replayHeading);
  const replay = document.createElement("dl");
  appendDefinitionList(replay, [
    ["Configuration", `${dashboard.configuration.configuration_id} / ${dashboard.configuration.configuration_version}`],
    ["Configuration bytes", dashboard.configuration.configuration_content_hash],
    ["Parameter revision", dashboard.configuration.parameter_set_fingerprint],
    ["Strategy", `${dashboard.reproducibility.strategy_id} / ${dashboard.reproducibility.strategy_version}`],
    ["Strategy bundle", dashboard.reproducibility.strategy_bundle_hash],
    ["Dataset", `${dashboard.reproducibility.dataset_id} / ${dashboard.reproducibility.dataset_version}`],
    ["Dataset hash", dashboard.reproducibility.dataset_hash],
    ["Replay event hash", dashboard.reproducibility.replay_event_hash],
    ["Journal head", dashboard.journal.head_hash],
    ["Projection fingerprint", dashboard.projection_fingerprint],
  ]);
  root.append(replay);

  if (dashboard.journal.failure_reason !== null) {
    const failure = document.createElement("p");
    failure.textContent = `Journal verification failure: ${dashboard.journal.failure_reason}`;
    root.append(failure);
  }
}

/** Renders a frozen options chain and reconciliation evidence projection. */
export function renderOptionsDashboard(root: HTMLElement, dashboard: OptionsDashboard): void {
  root.replaceChildren();
  const heading = document.createElement("h1");
  heading.textContent = "Options chain and scenario evidence";
  root.append(heading);

  const boundary = document.createElement("p");
  boundary.textContent = "Deterministic European-option analytics. Use the active trading controls for supported order-entry requests.";
  root.append(boundary);

  const summary = document.createElement("dl");
  appendDefinitionList(summary, [
    ["As of", dashboard.as_of],
    ["Chain", dashboard.chain.chain_id],
    ["Underlying", `${dashboard.chain.underlying_instrument_id} at ${dashboard.chain.underlying_mark} ${dashboard.chain.currency}`],
    ["Reference version", dashboard.chain.reference_version],
    ["Pricing model", dashboard.model_version],
    ["Reconciliation", dashboard.reconciliation.clean ? "CLEAN across BACKTEST, PAPER, and LIVE" : "DIFFERENCES FOUND"],
    ["Chain snapshot hash", dashboard.chain.chain_snapshot_hash],
  ]);
  root.append(summary);

  const reconciliationHeading = document.createElement("h2");
  reconciliationHeading.textContent = "Cross-environment reconciliation";
  root.append(reconciliationHeading);
  root.append(tableFromRows(
    ["Environment", "Account", "Source export", "Source hash", "Run identity", "Book hash"],
    [dashboard.reconciliation.backtest_book, dashboard.reconciliation.paper_book, dashboard.reconciliation.live_book].map((book) => [
      book.environment,
      book.account_id,
      book.source_export_id,
      book.source_export_hash,
      book.run_identity_hash,
      book.book_hash,
    ]),
  ));
  if (dashboard.reconciliation.issues.length === 0) {
    const clean = document.createElement("p");
    clean.textContent = "The bound BACKTEST, PAPER, and LIVE books agree on the compared economics and run identity. Their source exports remain independently fingerprinted.";
    root.append(clean);
  } else {
    root.append(tableFromRows(
      ["Category", "Subject", "Expected", "Observed"],
      dashboard.reconciliation.issues.map((issue) => [issue.category, issue.subject, issue.expected, issue.observed]),
    ));
  }

  const analyticsHeading = document.createElement("h2");
  analyticsHeading.textContent = "Implied volatility and Greeks";
  root.append(analyticsHeading);
  root.append(tableFromRows(
    ["Contract", "Right", "Strike", "Mid", "Implied vol", "Delta", "Gamma", "Vega", "Theta", "Rho"],
    dashboard.analytics.map((option) => [
      option.option_id,
      option.right,
      option.strike,
      option.market_premium,
      option.implied_volatility,
      option.delta,
      option.gamma,
      option.vega,
      option.theta,
      option.rho,
    ]),
  ));

  const scenariosHeading = document.createElement("h2");
  scenariosHeading.textContent = `Expiry scenarios — ${dashboard.strategy.strategy_id} / ${dashboard.strategy.strategy_version}`;
  root.append(scenariosHeading);
  root.append(tableFromRows(
    ["Underlying price", "Strategy P&L"],
    dashboard.strategy.scenarios.map((scenario) => [
      scenario.underlying_price,
      `${scenario.total_pnl} ${dashboard.chain.currency}`,
    ]),
  ));

  const provenanceHeading = document.createElement("h2");
  provenanceHeading.textContent = "Reproducibility evidence";
  root.append(provenanceHeading);
  const provenance = document.createElement("dl");
  appendDefinitionList(provenance, [
    ["Configuration file", dashboard.configuration_file_hash],
    ["Bound configuration", dashboard.run_identity.configuration_hash],
    ["Strategy bundle", dashboard.run_identity.strategy_bundle_hash],
    ["Dataset", dashboard.run_identity.dataset_hash],
    ["Replay event output", dashboard.run_identity.replay_event_hash],
    ["Reconciled at", dashboard.reconciliation.reconciled_at],
  ]);
  root.append(provenance);
}

function latestPortfolio(events: readonly EvidenceEvent[]): Record<string, unknown> | undefined {
  return [...events].reverse().find((event) => event.event_type === "portfolio.pnl_updated.v1")?.payload;
}

function isEvidenceEvent(value: unknown): value is EvidenceEvent {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  const envelopeFields = new Set([
    "event_id", "event_type", "schema_version", "event_time", "receive_time",
    "account_id", "strategy_id", "instrument_id", "correlation_id", "causation_id",
    "actor", "source", "payload", "software_version", "configuration_version",
  ]);
  const isCanonicalId = (field: unknown): field is string =>
    typeof field === "string" && /^[a-z0-9._-]+$/.test(field);
  const isNullableCanonicalId = (field: unknown): field is string | null =>
    field === null || isCanonicalId(field);
  const isCanonicalUtcTimestamp = (field: unknown): field is string => {
    if (typeof field !== "string") return false;
    const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d+)?Z$/.exec(field);
    if (match === null) return false;
    const [year, month, day, hour, minute, second] = match.slice(1, 7).map(Number);
    if (year < 1 || month < 1 || month > 12 || day < 1 || hour > 23 || minute > 59 || second > 59) {
      return false;
    }
    const parsed = new Date(Date.UTC(year, month - 1, day, hour, minute, second));
    return parsed.getUTCFullYear() === year && parsed.getUTCMonth() === month - 1 &&
      parsed.getUTCDate() === day && parsed.getUTCHours() === hour &&
      parsed.getUTCMinutes() === minute && parsed.getUTCSeconds() === second;
  };
  return (
    Object.keys(candidate).every((field) => envelopeFields.has(field)) &&
    isCanonicalId(candidate.event_id) &&
    typeof candidate.event_type === "string" &&
    /^([a-z]+\.)+[a-z_]+\.v[1-9][0-9]*$/.test(candidate.event_type) &&
    typeof candidate.schema_version === "number" && Number.isInteger(candidate.schema_version) && candidate.schema_version >= 1 &&
    isCanonicalUtcTimestamp(candidate.event_time) &&
    isCanonicalUtcTimestamp(candidate.receive_time) &&
    isNullableCanonicalId(candidate.account_id) &&
    isNullableCanonicalId(candidate.strategy_id) &&
    isNullableCanonicalId(candidate.instrument_id) &&
    isCanonicalId(candidate.correlation_id) &&
    isNullableCanonicalId(candidate.causation_id) &&
    typeof candidate.actor === "string" && candidate.actor.length > 0 &&
    typeof candidate.source === "string" && candidate.source.length > 0 &&
    candidate.payload !== null &&
    typeof candidate.payload === "object" &&
    !Array.isArray(candidate.payload) &&
    typeof candidate.software_version === "string" && candidate.software_version.length > 0 &&
    typeof candidate.configuration_version === "string" && candidate.configuration_version.length > 0
  );
}

function appendDefinitionList(target: HTMLDListElement, values: ReadonlyArray<readonly [string, string]>): void {
  for (const [label, value] of values) {
    const term = document.createElement("dt");
    term.textContent = label;
    const detail = document.createElement("dd");
    detail.textContent = value;
    target.append(term, detail);
  }
}

function tableFromRows(headers: readonly string[], rows: readonly (readonly string[])[]): HTMLTableElement {
  const table = document.createElement("table");
  const header = document.createElement("tr");
  for (const label of headers) {
    const cell = document.createElement("th");
    cell.scope = "col";
    cell.textContent = label;
    header.append(cell);
  }
  table.append(header);
  for (const values of rows) {
    const row = document.createElement("tr");
    for (const value of values) {
      const cell = document.createElement("td");
      cell.textContent = value;
      row.append(cell);
    }
    table.append(row);
  }
  return table;
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

function hasExactKeys(value: unknown, expected: readonly string[]): value is Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const keys = Object.keys(value);
  return keys.length === expected.length && keys.every((key) => expected.includes(key));
}

function isCanonicalId(value: unknown): value is string {
  return typeof value === "string" && /^[a-z0-9._-]+$/.test(value);
}

function isDecimal(value: unknown): value is string {
  return typeof value === "string" && /^-?[0-9]+(?:\.[0-9]{1,8})?$/.test(value);
}

function isNonNegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isPositiveInteger(value: unknown): value is number {
  return isNonNegativeInteger(value) && value > 0;
}

function isHash(value: unknown): value is string {
  return typeof value === "string" && /^[a-f0-9]{64}$/.test(value);
}

function isUtcTimestamp(value: unknown): value is string {
  return typeof value === "string" && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(value);
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
    "benchmark_schema_version",
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
  if (c.benchmark_schema_version !== 1 || !isCanonicalId(c.benchmark_id) || typeof c.model_identifier !== "string" || !isCanonicalId(c.evaluation_dataset_id)) return false;
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

