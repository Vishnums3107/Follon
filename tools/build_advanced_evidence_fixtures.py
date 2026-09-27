#!/usr/bin/env python3
"""Build and validate canonical fixtures for all 32 advanced evidence contracts.

Validates each canonical document against its corresponding JSON schema in
contracts/json-schema/v1/ before writing to tests/fixtures/config/advanced/.
"""

from __future__ import annotations

import json
from pathlib import Path
import jsonschema

REPO_ROOT = Path(__file__).resolve().parents[1]
SCHEMA_DIR = REPO_ROOT / "contracts" / "json-schema" / "v1"
TARGET_DIR = REPO_ROOT / "tests" / "fixtures" / "config" / "advanced"


FIXTURES: dict[str, tuple[str, dict]] = {
    "research-hypothesis.json": (
        "research-hypothesis.schema.json",
        {
            "hypothesis_schema_version": 1,
            "hypothesis_id": "hyp.momentum-cross-v1",
            "title": "Moving Average Trend Continuation on Volume Surge",
            "mechanism": "Volume-weighted momentum breakout continuation past 20-day high with trailing stop",
            "universe": ["inst.us_equity.spy", "inst.us_equity.qqq"],
            "evaluation_horizon": {
                "start_time": "2026-01-01T00:00:00Z",
                "end_time": "2026-06-30T23:59:59Z",
                "holding_period": "1 to 5 market bars",
            },
            "assumptions": [
                "Continuous market bar feeds without unadjusted splits",
                "Slippage bounded to 5 bps under liquid market sessions",
            ],
            "failure_criteria": [
                "Gross drawdown exceeds 1200 bps",
                "Realized annual Sharpe ratio falls below 0.50",
            ],
            "frozen_evaluation_plan": {
                "dataset_id": "ds.sp500.bars.v1",
                "dataset_version": "2026-01-01.1",
                "dataset_hash": "a" * 64,
                "cost_model": "tier-1-maker-taker",
                "slippage_bps": 5,
                "fee_model": "fixed-exchange-per-share",
            },
            "predecessor_id": None,
            "status": "FROZEN",
            "created_at": "2026-01-01T00:00:00Z",
            "frozen_at": "2026-01-01T01:00:00Z",
        },
    ),
    "experiment-lineage.json": (
        "experiment-lineage.schema.json",
        {
            "lineage_schema_version": 1,
            "lineage_id": "lin.trend-opt-001",
            "hypothesis_id": "hyp.momentum-cross-v1",
            "parent_run_ids": ["run.benchmark.001"],
            "input_fingerprints": [{"name": "strategy-bundle", "fingerprint": "b" * 64}],
            "output_fingerprints": [{"name": "events-ndjson", "fingerprint": "c" * 64}],
            "candidate_trials": [
                {
                    "trial_id": "trial.001",
                    "specification_hash": "d" * 64,
                    "return_bps": "1420.00",
                    "max_drawdown_bps": "850.00",
                    "disposition": "BENCHMARK",
                },
                {
                    "trial_id": "trial.002",
                    "specification_hash": "e" * 64,
                    "return_bps": "-310.50",
                    "max_drawdown_bps": "1420.00",
                    "disposition": "REJECTED",
                },
                {
                    "trial_id": "trial.003",
                    "specification_hash": "f" * 64,
                    "return_bps": "1890.25",
                    "max_drawdown_bps": "710.00",
                    "disposition": "PROMOTED",
                },
            ],
            "failed_candidates_count": 1,
            "rejection_reasons": [
                {"trial_id": "trial.002", "reason": "Excessive turnover and fee drag from whipsaw signals"},
            ],
            "created_at": "2026-01-02T00:00:00Z",
        },
    ),
    "research-job.json": (
        "research-job.schema.json",
        {
            "job_schema_version": 1,
            "job_id": "job.eval.001",
            "idempotency_key": "idem.eval.001",
            "strategy_id": "strat.trend.v1",
            "strategy_version": "1.0.0",
            "dataset_id": "ds.sp500.bars.v1",
            "dataset_version": "2026-01-01.1",
            "frozen_specification_hash": "1" * 64,
            "state_version": 1,
            "state": "QUEUED",
            "worker_lease": None,
            "output_manifest_hash": None,
            "failure_reason": None,
            "created_at": "2026-01-02T00:00:00Z",
            "updated_at": "2026-01-02T00:00:00Z",
        },
    ),
    "assistant-evidence.json": (
        "assistant-evidence.schema.json",
        {
            "assistant_evidence_schema_version": 1,
            "query_id": "query.exp.7f2a",
            "model_version": "follon-copilot-v1",
            "prompt_template_version": "risk-explainer-v1",
            "retrieved_record_ids": ["risk.decision.v1", "conf.risk.v1"],
            "generated_output": "Pre-trade risk rejected order: gross exposure of 105,000 USD exceeded account ceiling of 100,000 USD.",
            "tool_attempts": [
                {
                    "tool_name": "query_evidence",
                    "arguments_hash": "2" * 64,
                    "status": "SUCCESS",
                    "evidence_id": "risk.decision.v1",
                },
            ],
            "uncertainty_score_bps": 250,
            "human_disposition": "ACCEPTED",
            "created_at": "2026-01-02T00:00:00Z",
        },
    ),
    "robustness-evaluation.json": (
        "robustness-evaluation.schema.json",
        {
            "evaluation_schema_version": 1,
            "evaluation_id": "eval.trend-walkforward-001",
            "strategy_version": "strat.trend.v1",
            "hypothesis_id": "hyp.momentum-cross-v1",
            "walk_forward_windows": [
                {
                    "window_id": "win.001",
                    "in_sample_start": "2025-01-01T00:00:00Z",
                    "in_sample_end": "2025-06-30T23:59:59Z",
                    "out_of_sample_start": "2025-07-01T00:00:00Z",
                    "out_of_sample_end": "2025-09-30T23:59:59Z",
                    "in_sample_return_bps": 1240,
                    "out_of_sample_return_bps": 480,
                    "max_drawdown_bps": 540,
                },
            ],
            "leakage_checks": {
                "survivorship_bias_verified": True,
                "lookahead_bias_verified": True,
                "corporate_action_adjusted": True,
                "quarantine_violations": 0,
            },
            "parameter_stability": {
                "perturbation_percent": 15,
                "neighborhood_variance_bps": 120,
                "degradation_cliff_detected": False,
            },
            "cost_shocks": [
                {
                    "slippage_multiplier": "2.0",
                    "fee_multiplier": "2.0",
                    "stressed_return_bps": 920,
                },
            ],
            "uncertainty_score_bps": 180,
            "disposition": "ROBUST",
            "created_at": "2026-09-04T12:00:00Z",
        },
    ),
    "portfolio-experiment.json": (
        "portfolio-experiment.schema.json",
        {
            "portfolio_experiment_schema_version": 1,
            "experiment_id": "port-exp.dual-alpha-001",
            "allocated_cash": "100000.00",
            "currency": "USD",
            "strategies": [
                {
                    "strategy_id": "strat.trend.v1",
                    "strategy_version": "2026-01-01.1",
                    "target_weight_bps": 6000,
                    "realized_pnl": "12400.00",
                    "max_drawdown_bps": 510,
                },
                {
                    "strategy_id": "strat.meanrev.v1",
                    "strategy_version": "2026-01-01.1",
                    "target_weight_bps": 4000,
                    "realized_pnl": "9000.00",
                    "max_drawdown_bps": 420,
                },
            ],
            "joint_constraints": {
                "max_gross_exposure_bps": 10000,
                "max_single_instrument_bps": 2500,
                "turnover_cap_daily_bps": 2000,
            },
            "joint_performance": {
                "combined_return_bps": 2140,
                "combined_max_drawdown_bps": 510,
                "diversification_ratio_bps": 1420,
                "total_fee_drag": "350.50",
            },
            "order_contention_events": 2,
            "created_at": "2026-09-04T14:00:00Z",
        },
    ),
    "knowledge-snapshot.json": (
        "knowledge-snapshot.schema.json",
        {
            "knowledge_schema_version": 1,
            "snapshot_id": "know.sp500.pit.001",
            "as_of_time": "2026-01-16T15:00:00Z",
            "entity_nodes": [
                {
                    "entity_id": "comp.sp500.aapl",
                    "entity_type": "COMPANY",
                    "name": "Apple Inc.",
                    "identifier": "AAPL",
                },
                {
                    "entity_id": "inst.us_equity.aapl",
                    "entity_type": "INSTRUMENT",
                    "name": "Apple Inc. Common Stock",
                    "identifier": "US0378331005",
                },
            ],
            "relationships": [
                {
                    "source_entity_id": "comp.sp500.aapl",
                    "relation_type": "ISSUES_INSTRUMENT",
                    "target_entity_id": "inst.us_equity.aapl",
                    "effective_time": "2026-01-01T00:00:00Z",
                    "provenance_hash": "a" * 64,
                },
            ],
            "source_lineage_hashes": ["b" * 64],
            "created_at": "2026-01-16T15:05:00Z",
        },
    ),
    "event-exposure-calendar.json": (
        "event-exposure-calendar.schema.json",
        {
            "calendar_schema_version": 1,
            "calendar_id": "cal.equities.2026q1",
            "as_of_time": "2026-01-01T00:00:00Z",
            "timezone": "America/New_York",
            "scheduled_events": [
                {
                    "event_id": "ev.earn.aapl.2026q1",
                    "instrument_id": "inst.us_equity.aapl",
                    "category": "EARNINGS",
                    "scheduled_time": "2026-01-22T21:30:00Z",
                    "status": "SCHEDULED",
                    "source_evidence": "cal.ir.aapl.2026q1",
                },
            ],
            "quarantined_events_count": 0,
            "created_at": "2026-01-01T00:00:00Z",
        },
    ),
    "automation-mandate.json": (
        "automation-mandate.schema.json",
        {
            "mandate_schema_version": 1,
            "mandate_id": "mandate.nightly.001",
            "owner": "operator.solo",
            "allowed_tasks": ["walk-forward-sweep", "cost-sensitivity-shock"],
            "resource_limits": {
                "max_cpu_cores": 4,
                "max_memory_mb": 8192,
                "max_duration_seconds": 14400,
                "max_storage_bytes": 104857600,
            },
            "cancellation_policy": {
                "stop_on_first_error": True,
                "checkpoint_interval_seconds": 300,
            },
            "broker_access_permitted": False,
            "created_at": "2026-01-01T20:00:00Z",
            "expires_at": "2026-01-02T06:00:00Z",
        },
    ),
    "order-decision-passport.json": (
        "order-decision-passport.schema.json",
        {
            "passport_schema_version": 1,
            "passport_id": "passport.ord.9b41",
            "intent_id": "intent.001",
            "order_id": "ord.9b41",
            "instrument_id": "inst.us_equity.spy",
            "signal_attribution": {
                "strategy_version": "strat.trend.v1",
                "model_event_id": "evt.sig.001",
                "opportunity_description": "Volume-weighted 20-day breakout continuation",
                "signal_power_bps": 8500,
            },
            "risk_evaluation": {
                "policy_version": "pol.risk.v1",
                "approved": True,
                "evaluated_limits": ["gross_exposure", "daily_loss_limit"],
                "headroom_remaining_bps": 4500,
            },
            "routing_plan": {
                "algorithm": "twap-v1",
                "allocated_slices_count": 3,
                "primary_venue": "venue.nasdaq",
                "capability_version": "cap.nasdaq.v1",
            },
            "executions": [
                {
                    "execution_id": "exec.001",
                    "venue": "venue.nasdaq",
                    "quantity": "100",
                    "price": "512.40",
                    "fee": "0.15",
                    "executed_at": "2026-09-04T14:30:00Z",
                },
            ],
            "accounting_consequences": {
                "journal_entry_id": "jrn.7f1a",
                "realized_pnl": "0.00",
                "cash_delta": "-51240.15",
                "position_after": "100",
            },
            "created_at": "2026-09-04T14:30:05Z",
        },
    ),
    "exposure-graph.json": (
        "exposure-graph.schema.json",
        {
            "exposure_schema_version": 1,
            "graph_id": "exp-graph.paper.001",
            "account_id": "acct.paper.001",
            "as_of_time": "2026-09-04T15:00:00Z",
            "gross_exposure": "100000.00",
            "net_exposure": "25000.00",
            "factors": [
                {
                    "factor_name": "Momentum (12-1M)",
                    "loading_bps": 4200,
                    "factor_variance_pct": "34.5%",
                },
                {
                    "factor_name": "Market Beta",
                    "loading_bps": 9800,
                    "factor_variance_pct": "52.0%",
                },
            ],
            "sectors": [
                {
                    "sector_name": "Technology",
                    "exposure_usd": "45000.00",
                    "weight_bps": 4500,
                },
            ],
            "top_concentrations": [
                {
                    "instrument_id": "inst.us_equity.spy",
                    "position_value": "51240.00",
                    "portfolio_pct": "51.2%",
                },
            ],
            "unreconciled_discrepancy": False,
            "created_at": "2026-09-04T15:00:05Z",
        },
    ),
    "fund-ledger-statement.json": (
        "fund-ledger-statement.schema.json",
        {
            "ledger_schema_version": 1,
            "statement_id": "stmt.reconciliation.001",
            "account_id": "acct.paper.001",
            "period_start": "2026-09-01T00:00:00Z",
            "period_end": "2026-09-04T23:59:59Z",
            "starting_cash": "50000.00",
            "ending_cash": "52480.00",
            "realized_pnl": "2480.00",
            "unrealized_pnl": "1030.00",
            "fee_totals": {
                "exchange_fees": "14.20",
                "brokerage_commissions": "0.00",
                "borrow_financing": "0.00",
            },
            "tax_lots": [
                {
                    "lot_id": "lot.spy.001",
                    "instrument_id": "inst.us_equity.spy",
                    "acquired_at": "2026-09-02T14:35:00Z",
                    "quantity": "100",
                    "cost_basis": "502.10",
                    "disposition": "OPEN",
                },
            ],
            "balanced": True,
            "created_at": "2026-09-05T00:00:00Z",
        },
    ),
    "continuity-policy.json": (
        "continuity-policy.schema.json",
        {
            "policy_schema_version": 1,
            "policy_id": "cont-pol.standard.v1",
            "unattended_interval_minutes": 30,
            "heartbeat_interval_seconds": 5,
            "max_restarts_per_hour": 3,
            "away_mode_permitted": True,
            "broker_disconnect_action": "RETAIN_UNKNOWN_AND_ESCALATE",
            "feed_stale_threshold_seconds": 3,
            "created_at": "2026-09-01T00:00:00Z",
        },
    ),
    "assumption-regime-monitor.json": (
        "assumption-regime-monitor.schema.json",
        {
            "regime_schema_version": 1,
            "regime_id": "regime.spread-regime.001",
            "as_of_time": "2026-09-04T12:00:00Z",
            "lookback_bars": 200,
            "current_regime": "LOW_VOL_TRENDING",
            "indicators": {
                "realized_vol_annual_bps": 1450,
                "effective_spread_bps": 18,
                "trend_strength_bps": 6500,
                "cross_asset_correlation_bps": 4200,
            },
            "impacted_strategy_assumptions": [
                {
                    "strategy_id": "strat.trend.v1",
                    "assumed_condition": "Effective spread < 25 bps",
                    "observed_condition": "Observed spread 18 bps",
                    "breach_status": "COMPATIBLE",
                },
            ],
            "model_version": "regime-hdbscan-v1",
            "created_at": "2026-09-04T12:05:00Z",
        },
    ),
    "feed-substitution-parity.json": (
        "feed-substitution-parity.schema.json",
        {
            "parity_schema_version": 1,
            "comparison_id": "feed-parity.arca-bats-001",
            "primary_provider": "feed.sip.nyse-arca.v1",
            "candidate_provider": "feed.direct.bats.v1",
            "sample_start": "2026-01-01T00:00:00Z",
            "sample_end": "2026-06-30T23:59:59Z",
            "symbol_match_pct": "99.98",
            "timestamp_variance_micros_p99": 15000,
            "adjustment_parity_verified": True,
            "parity_disposition": "QUALIFIED_FOR_SUBSTITUTION",
            "created_at": "2026-09-04T14:00:00Z",
        },
    ),
    "execution-coach-benchmark.json": (
        "execution-coach-benchmark.schema.json",
        {
            "coach_schema_version": 1,
            "analysis_id": "coach.analysis.001",
            "order_id": "ord.9b41",
            "instrument_id": "inst.us-equity.spy",
            "arrival_price": "512.20",
            "target_price": "512.25",
            "realized_vwap": "512.40",
            "pre_trade_estimated_cost_bps": 8,
            "realized_shortfall_bps": 12,
            "slippage_drag_bps": 4,
            "market_impact_bps": 5,
            "fee_drag_bps": 3,
            "execution_grade": "OPTIMAL",
            "created_at": "2026-09-04T16:00:00Z",
        },
    ),
    "scenario-loss-simulation.json": (
        "scenario-loss-simulation.schema.json",
        {
            "simulation_schema_version": 1,
            "simulation_id": "loss-sim.stress-2008-crash",
            "account_id": "acct.paper.01",
            "scenario_name": "2008 Financial Crisis Replay",
            "shock_assumptions": {
                "equity_shock_pct": "-40.0",
                "volatility_multiplier": "2.8",
                "spread_expansion_multiplier": "4.0",
                "financing_rate_shock_bps": 150,
            },
            "estimated_loss_usd": "17550.00",
            "estimated_loss_bps": 1755,
            "liquidity_haircut_usd": "2400.00",
            "stressed_margin_utilization_pct": "68.5",
            "capital_adequate": True,
            "created_at": "2026-09-04T18:00:00Z",
        },
    ),
    "capital-allocation-plan.json": (
        "capital-allocation-plan.schema.json",
        {
            "allocation_schema_version": 1,
            "plan_id": "alloc-plan.q3-001",
            "total_capital_usd": "100000.00",
            "cash_reserve_bps": 1000,
            "allocations": [
                {
                    "strategy_id": "strat.trend.v1",
                    "allocated_capital_usd": "60000.00",
                    "target_weight_bps": 6000,
                    "expected_sharpe": "1.45",
                },
                {
                    "strategy_id": "strat.meanrev.v1",
                    "allocated_capital_usd": "30000.00",
                    "target_weight_bps": 3000,
                    "expected_sharpe": "1.20",
                },
            ],
            "risk_policy_version": "risk-policy-2026-v1",
            "approved_by_policy": True,
            "created_at": "2026-09-04T19:00:00Z",
        },
    ),
    "sandbox-installation-preview.json": (
        "sandbox-installation-preview.schema.json",
        {
            "preview_schema_version": 1,
            "preview_id": "preview.pkg-trend-001",
            "asset_id": "pkg.strat.trend-breakout.v1",
            "asset_version": "1.0.0",
            "manifest_hash": "4a5b6c7d8e9f0123456789abcdef0123456789abcdef0123456789abcdef0123",
            "declared_permissions": ["READ_MARKET_DATA", "EMIT_ORDER_INTENT"],
            "resource_caps": {
                "max_memory_mb": 4096,
                "max_cpu_percent": 50,
                "filesystem_isolated": True,
            },
            "untrusted_capabilities_detected": 0,
            "rollback_snapshot_id": "snap.preinstall.001",
            "disposition": "QUALIFIED_FOR_ISOLATED_INSTALL",
            "created_at": "2026-09-04T20:00:00Z",
        },
    ),
    "adapter-qualification.json": (
        "adapter-qualification.schema.json",
        {
            "qualification_schema_version": 1,
            "qualification_id": "qual.ibkr.rest-ws.001",
            "venue": "venue.interactive-brokers",
            "asset_class": "US_EQUITY",
            "adapter_version": "ibkr-adapter-v2.1",
            "supported_capabilities": ["LIMIT_ORDER", "MARKET_ORDER", "CANCEL_REPLACE", "ORDER_STATUS_POLL"],
            "single_writer_fenced": True,
            "reconciliation_pass_rate_pct": "99.99",
            "operational_gate_status": "QUALIFIED",
            "created_at": "2026-09-04T21:00:00Z",
            "expires_at": "2027-09-04T21:00:00Z",
        },
    ),
    "champion-challenger-evaluation.json": (
        "champion-challenger-evaluation.schema.json",
        {
            "champion_challenger_schema_version": 1,
            "evaluation_id": "eval.champ.trend-v1",
            "champion_strategy_id": "strat.trend.v1",
            "challenger_strategy_id": "strat.trend.v2",
            "evaluation_window_start": "2026-06-01T00:00:00Z",
            "evaluation_window_end": "2026-09-01T23:59:59Z",
            "champion_return_bps": 840,
            "challenger_return_bps": 1120,
            "champion_max_drawdown_bps": 420,
            "challenger_max_drawdown_bps": 380,
            "information_ratio_diff_bps": 45,
            "drift_detected": False,
            "recommendation": "CONTINUE_SHADOW_MONITORING",
            "created_at": "2026-09-04T12:00:00Z",
        },
    ),
    "capability-execution-planner.json": (
        "capability-execution-planner.schema.json",
        {
            "planner_schema_version": 1,
            "plan_id": "plan.exec.twap-01",
            "parent_order_id": "ord.9b41",
            "target_venue": "venue.nasdaq",
            "algorithm": "TWAP_SLICED",
            "max_volume_participation_pct": "15.0",
            "passive_pegging_offset_bps": 10,
            "schedule_slices": [
                {
                    "slice_sequence": 1,
                    "planned_release_time": "2026-09-04T14:30:00Z",
                    "allocated_quantity": "33.00",
                    "order_kind": "LIMIT_PASSIVE",
                },
                {
                    "slice_sequence": 2,
                    "planned_release_time": "2026-09-04T14:35:00Z",
                    "allocated_quantity": "33.00",
                    "order_kind": "LIMIT_PASSIVE",
                },
            ],
            "supported_capabilities_verified": True,
            "disposition": "VALIDATED_FOR_DISPATCH",
            "created_at": "2026-09-04T14:25:00Z",
        },
    ),
    "operations-diagnosis-runbook.json": (
        "operations-diagnosis-runbook.schema.json",
        {
            "diagnosis_schema_version": 1,
            "diagnosis_id": "diag.ops.feed-stale-01",
            "incident_id": "inc.feed.001",
            "failing_component": "US-Equities Quote Feed",
            "root_cause_summary": "Quote feed latency exceeded 3s threshold due to gateway reconnect",
            "cited_evidence_ids": ["ev.quote.stale.001", "heartbeat.feed.01"],
            "proposed_runbook_steps": [
                {
                    "step_number": 1,
                    "action_name": "Restart Feed Receiver",
                    "target_service": "feed-receiver",
                    "command_template": "systemctl restart follon-feed-receiver",
                    "is_idempotent": True,
                },
            ],
            "idempotency_certified": True,
            "trading_path_isolated": True,
            "approval_required": "OPERATOR_CONFIRMATION",
            "created_at": "2026-09-04T15:00:00Z",
        },
    ),
    "model-evaluation-benchmark.json": (
        "model-evaluation-benchmark.schema.json",
        {
            "model_evaluation_schema_version": 1,
            "benchmark_id": "eval.model.gemini-pro",
            "model_identifier": "gemini-1.5-pro",
            "evaluation_dataset_id": "ds.eval.research-ops.v1",
            "factuality_score_bps": 9850,
            "citation_precision_bps": 9920,
            "injection_resistance_score_bps": 9980,
            "hallucination_rate_bps": 12,
            "average_latency_ms": 480,
            "token_cost_usd_per_million": "1.25",
            "disposition": "QUALIFIED_FOR_ASSISTANCE",
            "evaluated_at": "2026-09-04T16:00:00Z",
        },
    ),
    "strategy-capsule-manifest.json": (
        "strategy-capsule-manifest.schema.json",
        {
            "capsule_schema_version": 1,
            "capsule_id": "capsule.trend.v1",
            "strategy_id": "strat.trend.v1",
            "strategy_version": "v1.0.0",
            "bundle_sha256": "1" * 64,
            "configuration_sha256": "2" * 64,
            "dependency_lockfile_sha256": "3" * 64,
            "runtime_target": "follon-runtime-py312-v1",
            "evaluation_receipt_id": "eval.golden.001",
            "replay_instruction_command": "follon-cli replay --capsule capsule.trend.v1.tar.gz",
            "export_disposition": "VERIFIED_PORTABLE",
            "packaged_at": "2026-09-01T16:00:00Z",
        },
    ),
    "multi-asset-expansion-plan.json": (
        "multi-asset-expansion-plan.schema.json",
        {
            "expansion_schema_version": 1,
            "plan_id": "plan.asset.opt-roll-q3",
            "asset_class": "EQUITY_OPTION",
            "underlying_universe": ["inst.us_equity.spy"],
            "lifecycle_actions": [
                {
                    "action_id": "act.roll.spy.c515",
                    "instrument_id": "opt.spy.20260918.c515",
                    "action_kind": "OPTION_ROLL",
                    "target_date": "2026-09-18T20:00:00Z",
                    "contract_quantity": 10,
                    "estimated_cash_flow_usd": "-1450.00",
                },
            ],
            "margin_requirement_usd": "25000.00",
            "settlement_currency": "USD",
            "reconciliation_clean": True,
            "operational_verdict": "READY_FOR_LIFECYCLE_EXECUTION",
            "created_at": "2026-09-04T18:00:00Z",
        },
    ),
    "decision-reconstruction.json": (
        "decision-reconstruction.schema.json",
        {
            "reconstruction_schema_version": 1,
            "reconstruction_id": "recon.fill.9b41a2c",
            "target_event_id": "evt.fill.9b41a2c",
            "target_entity_type": "fill",
            "causal_chain": [
                {
                    "node_id": "evt.bar.spy.001",
                    "event_type": "market.bar.v1",
                    "actor": "market-feed",
                    "event_time": "2026-09-01T14:30:00Z",
                    "available_at": "2026-09-01T14:30:00Z",
                    "content_hash": "a" * 64,
                    "summary": "Market bar SPY close 500.00",
                },
                {
                    "node_id": "evt.sig.spy.001",
                    "event_type": "signal.generated.v1",
                    "actor": "strategy-engine",
                    "event_time": "2026-09-01T14:30:01Z",
                    "available_at": "2026-09-01T14:30:01Z",
                    "causation_id": "evt.bar.spy.001",
                    "content_hash": "b" * 64,
                    "summary": "Trend signal buy 100 SPY",
                },
            ],
            "edges": [
                {
                    "from_node_id": "evt.bar.spy.001",
                    "to_node_id": "evt.sig.spy.001",
                    "relation": "CAUSED_SIGNAL",
                },
            ],
            "configuration_hash": "c" * 64,
            "integrity_status": "VERIFIED",
            "verified_at": "2026-09-01T14:30:05Z",
        },
    ),
    "data-rights-and-semantics-receipt.json": (
        "data-rights-and-semantics-receipt.schema.json",
        {
            "receipt_schema_version": 1,
            "receipt_id": "drsr.polygon.us-equity-l1",
            "provider_id": "provider.polygon",
            "dataset_id": "ds.us_equity.1m",
            "license_tier": "COMMERCIAL_REPLAY",
            "redistribution_permitted": False,
            "corporate_action_policy": "RAW_SPLIT_AND_DIVIDEND_ADJUSTED",
            "semantic_parity_score_bps": 9980,
            "verified_at": "2026-09-01T08:00:00Z",
            "expires_at": "2027-09-01T08:00:00Z",
        },
    ),
    "workspace-snapshot-manifest.json": (
        "workspace-snapshot-manifest.schema.json",
        {
            "snapshot_schema_version": 1,
            "manifest_id": "snapshot.2026-09-01.eod",
            "as_of_time": "2026-09-01T20:00:00Z",
            "created_at": "2026-09-01T20:01:00Z",
            "content_hash": "d" * 64,
            "retained_event_count": 5000,
            "source_event_count": 5000,
            "event_window": {
                "window_kind": "full_day_session",
                "first_event_time": "2026-09-01T13:30:00Z",
                "last_event_time": "2026-09-01T20:00:00Z",
            },
            "active_accounts": ["acct.paper.01", "acct.paper.02"],
            "positions_fingerprint": "e" * 64,
            "ledger_balance_fingerprint": "f" * 64,
            "diagnostics": [],
        },
    ),
    "market-scanner.json": (
        "market-scanner.schema.json",
        {
            "scanner_schema_version": 1,
            "scanner_id": "scan.us-equity-momentum",
            "universe_id": "univ.us-equity.liquid-top500",
            "as_of_time": "2026-09-01T15:30:00Z",
            "indicator_columns": [
                {
                    "column_id": "col.momentum-20d",
                    "name": "20-Day Momentum",
                    "timeframe": "1D",
                    "definition": "Rate of change over 20 daily close prices in basis points",
                },
                {
                    "column_id": "col.rsi-14",
                    "name": "14-Period RSI",
                    "timeframe": "1D",
                    "definition": "Wilder Relative Strength Index over 14 daily periods",
                },
            ],
            "candidates": [
                {
                    "rank": 1,
                    "instrument_id": "inst.us_equity.spy",
                    "symbol": "SPY",
                    "close_price": "560.25000000",
                    "momentum_score_bps": 420,
                    "rsi_14": "62.45000000",
                    "matched_conditions": ["BREAKOUT_20D_HIGH", "RSI_BETWEEN_50_AND_70"],
                    "rationale": "Strong upward trend continuation above 20-day high with unoverbought RSI",
                },
                {
                    "rank": 2,
                    "instrument_id": "inst.us_equity.qqq",
                    "symbol": "QQQ",
                    "close_price": "485.10000000",
                    "momentum_score_bps": 385,
                    "rsi_14": "58.12000000",
                    "matched_conditions": ["VOLUME_SURGE_150PCT", "MOMENTUM_POSITIVE"],
                    "rationale": "High relative volume with positive 20-day momentum score",
                },
            ],
            "quarantined_count": 0,
            "created_at": "2026-09-01T15:30:05Z",
        },
    ),
    "news-revision-timeline.json": (
        "news-revision-timeline.schema.json",
        {
            "revision_timeline_schema_version": 1,
            "timeline_id": "rev.timeline.earnings-001",
            "target_event_id": "evt.news.001",
            "chain": [
                {
                    "version_sequence": 1,
                    "received_at": "2026-09-01T11:00:00Z",
                    "source_id": "DOW_JONES",
                    "kind": "INITIAL_REPORT",
                    "headline": "Acme Corp reports Q3 EPS $1.20 vs $1.10 expected",
                    "entity_confidence_bps": 9500,
                    "content_hash": "a" * 64,
                    "supersedes_sequence": None,
                },
                {
                    "version_sequence": 2,
                    "received_at": "2026-09-01T11:05:00Z",
                    "source_id": "REUTERS",
                    "kind": "SYNDICATED_DUPLICATE",
                    "headline": "Acme Corp beats earnings estimates in third quarter",
                    "entity_confidence_bps": 9200,
                    "content_hash": "b" * 64,
                    "supersedes_sequence": 1,
                },
                {
                    "version_sequence": 3,
                    "received_at": "2026-09-01T11:15:00Z",
                    "source_id": "DOW_JONES",
                    "kind": "CORRECTION",
                    "headline": "CORRECTION: Acme Corp Q3 GAAP EPS was $1.15, adjusted $1.20",
                    "entity_confidence_bps": 9800,
                    "content_hash": "c" * 64,
                    "supersedes_sequence": 1,
                },
            ],
            "conflict_detected": False,
            "created_at": "2026-09-01T11:15:05Z",
        },
    ),
    "strategy-composition-spec.json": (
        "strategy-composition-spec.schema.json",
        {
            "composition_schema_version": 1,
            "composition_id": "comp.strat.trend-v1",
            "strategy_id": "strat.trend.v1",
            "strategy_version": "1.0.0",
            "signals": [
                {
                    "signal_id": "sig.ema-cross",
                    "indicator_ref": "ind.ema.12-26",
                    "condition": "FAST_EMA > SLOW_EMA",
                    "weight_bps": 6000,
                },
                {
                    "signal_id": "sig.vol-filter",
                    "indicator_ref": "ind.atr.14",
                    "condition": "ATR_14 > ATR_BASELINE",
                    "weight_bps": 4000,
                },
            ],
            "sizing_rule": {
                "sizing_type": "VOLATILITY_TARGETED",
                "target_value": "1500_BPS_ANNUAL_VOL",
            },
            "entry_criteria": [
                "SIGNAL_SUM_WEIGHT_BPS >= 5000",
                "MARKET_SESSION_NORMAL",
            ],
            "exit_criteria": [
                "TRAILING_STOP_TRIGGERED",
                "FAST_EMA < SLOW_EMA",
            ],
            "portfolio_constraints": {
                "max_leverage_bps": 10000,
                "max_single_position_bps": 2500,
                "stop_loss_pct": "2.50000000",
            },
            "code_hash": "d" * 64,
            "visual_representation_hash": "e" * 64,
            "created_at": "2026-09-01T09:00:00Z",
        },
    ),
}


def validate_and_write() -> None:
    TARGET_DIR.mkdir(parents=True, exist_ok=True)
    print(f"Validating and writing {len(FIXTURES)} canonical advanced evidence fixtures...")
    for filename, (schema_name, data) in sorted(FIXTURES.items()):
        schema_path = SCHEMA_DIR / schema_name
        if not schema_path.exists():
            raise FileNotFoundError(f"Schema not found: {schema_path}")
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
        # Validate against JSON schema
        jsonschema.validate(instance=data, schema=schema)
        # Write canonical sorted JSON
        target_path = TARGET_DIR / filename
        # LF on every platform: these are checked-in fixtures (audit item 75).
        target_path.write_text(
            json.dumps(data, sort_keys=True, separators=(",", ":")) + "\n",
            encoding="utf-8",
            newline="\n",
        )
        print(f"  [OK] {filename:<40} (validated against {schema_name})")
    print(f"\nAll {len(FIXTURES)} advanced fixtures successfully validated and published to {TARGET_DIR}.")


if __name__ == "__main__":
    validate_and_write()
