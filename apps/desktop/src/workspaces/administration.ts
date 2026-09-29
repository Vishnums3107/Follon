// Administration workspace.

import { parseAdapterQualification, parseCompatibilityMatrix, parseContinuityPolicy, parseGatewayQualificationMatrix, parseModelEvaluationBenchmark, parseOperationsDiagnosisRunbook, parseRecoveryDrillResult, parseWorkspaceSnapshotManifest } from "../evidence/index.js";
import { appendAdvancedEvidenceRows } from "./advanced-evidence.js";
import { field, shortHash } from "./format.js";
import { appendDefinition, appendTableOrEmpty, createPanel, renderFeatureEvidence, renderMetrics } from "./panels.js";
import type { WorkspaceContext, WorkspaceSnapshot } from "./types.js";

export function renderAdministration(summaryRoot: HTMLElement, root: HTMLElement, snapshot: WorkspaceSnapshot, context: WorkspaceContext): void {
  const provisioned = snapshot.commercial.filter((item) => field(item.data, "event_type") === "commercial.tenant_provisioned.v1").length;
  const subscriptions = snapshot.commercial.filter((item) => field(item.data, "event_type").includes("subscription")).length;
  const releaseArtifacts = snapshot.commercial_artifacts.filter((item) => /release|signature|trusted-key/i.test(item.name));
  const selfHostArtifacts = snapshot.commercial_artifacts.filter((item) => /self-host|readiness/i.test(item.name));
  renderMetrics(summaryRoot, [
    ["Provisioning records", String(provisioned), "Pseudonymous tenant evidence"],
    ["Subscription observations", String(subscriptions), "External payment facts recorded without card data"],
    ["Release evidence", String(releaseArtifacts.length), "Manifest, signature, and trusted-key artifacts", releaseArtifacts.length > 0 ? "good" : "warn"],
    ["Self-host readiness", String(selfHostArtifacts.length), "Verified entitlement and signed release evidence", selfHostArtifacts.length > 0 ? "good" : "warn"],
  ]);
  const tenants = createPanel("Commercial ledger", "Typed, pseudonymous commercial facts; password and MFA secret material is never projected into this dashboard.");
  appendTableOrEmpty(tenants, ["Sequence", "Tenant", "Event", "Actor", "Occurred", "Record hash", "Artifact"], snapshot.commercial.map((item) => [
    field(item.data, "sequence"), field(item.data, "tenant_id"), field(item.data, "event_type"), field(item.data, "actor"),
    field(item.data, "occurred_at"), shortHash(field(item.data, "record_hash")), item.artifact,
  ]), "No commercial ledger records are available.", (index) => context.onOpenArtifact(snapshot.commercial[index]?.artifact ?? ""));
  root.append(tenants);

  const controls = createPanel("Deployment and administrative controls", "Implemented evidence primitives and their enforced operating boundary.");
  appendTableOrEmpty(controls, ["Capability", "Repository implementation", "Dashboard integration", "Remaining external dependency"], [
    ["Provisioning", "Typed tenant and workspace record", provisioned > 0 ? "Evidence visible" : "No local evidence", "Customer onboarding acceptance"],
    ["Entitlement", "Deterministic PAID / GRACE / denied derivation", subscriptions > 0 ? "Ledger evidence visible" : "No subscription observation", "Payment-provider validation and gateway enforcement"],
    ["Privacy / retention", "Hash-bound plan and confirmed single-file execution", artifactCount(snapshot, /privacy|retention/i) > 0 ? "Artifacts visible" : "No local plan artifact", "Reviewed request, legal hold, and authorized operator"],
    ["Signed release", "Manifest plus detached Ed25519 verification", releaseArtifacts.length > 0 ? "Evidence visible" : "No local signed release evidence", "Offline HSM/KMS signing and independent review"],
    ["Customer IAM / MFA / RBAC", "Argon2id, TOTP, hashed one-time recovery codes, password rotation, opaque sessions, lockout, revocation, tenant isolation, and server-side roles", "Capability and runtime auth mode visible", "Production enrollment, out-of-band delivery, support, and acceptance evidence"],
    ["Transactional PostgreSQL", "Checksum-bound schema, forced RLS, event-plus-outbox transaction, idempotency, balanced journals, and complete product projections", "gRPC and database health visible", "Backup/restore drill and production secret/TLS custody"],
    ["React / Tauri", "Vite production bundle and least-privilege Tauri v2 native host", "This interface is React-owned", "Signed installer promotion and OS-specific acceptance"],
    ["gRPC topology", "Versioned scheduled, cancel-before-replace passive, and atomic options-combination EMS plus portfolio-risk and margin APIs with production mTLS requirement", "Trading API health visible", "Production certificate issuance, ingress, monitoring, and load acceptance"],
    ["Controlled-LIVE IBKR", "Signed artifact verification, two-reviewer binding, canary envelope, initial snapshot, and emergency stop", "Capability boundary visible", "Reviewed vendor transport, broker credentials, and capital-bearing acceptance"],
    ["Self-host readiness", "Loopback, managed-secret, signature, entitlement checks", selfHostArtifacts.length > 0 ? "Evidence visible" : "No readiness receipt", "Customer deployment, backups, TLS, monitoring, and on-call"],
  ], "No administrative control mapping is available.");
  root.append(controls);

  const boundary = createPanel("Privileged-action boundary", "These operations intentionally stay outside the web process.");
  appendDefinition(boundary, [
    ["Never accepted by this server", "Broker credentials, payment cards, private signing keys, password/MFA material, or live approval secrets"],
    ["Operator-only commands", "Provisioning, retention execution, release signing, entitlement checks, kill switches, schedule completion, and journal append"],
    ["Why", "They require stronger identity, confirmation, filesystem, two-person, offline-signing, or broker boundaries than the evidence API provides"],
  ]);
  root.append(boundary);

  const watchdogPanel = createPanel(
    "Operational watchdog, recovery and failure drills",
    "Continuous health monitoring, stale-feed thresholds, restart budgets, and simulated partition/recovery drills (LIFE-04/05/06/09/10/11)."
  );
  watchdogPanel.id = "watchdog-recovery-panel";
  appendAdvancedEvidenceRows(
    watchdogPanel,
    snapshot,
    context,
    "continuity_policy",
    parseContinuityPolicy,
    ["Watchdog Check / Drill", "Target Component", "Policy Threshold", "Observed State", "Recovery Procedure"],
    (policy) => [
      ["Heartbeat policy", policy.policy_id, `${policy.heartbeat_interval_seconds}s`, "No observed watchdog telemetry published", policy.broker_disconnect_action],
      ["Feed freshness policy", policy.policy_id, `${policy.feed_stale_threshold_seconds}s`, "No observed watchdog telemetry published", "Hold or escalate according to policy"],
      ["Restart budget policy", policy.policy_id, `${policy.max_restarts_per_hour}/hour`, "No observed watchdog telemetry published", "Budget exhaustion outcome requires a recovery-drill record"],
    ],
    "No typed continuity policy is published; watchdog state is unknown."
  );
  root.append(watchdogPanel);

  const adapterQualificationPanel = createPanel(
    "Broker and venue adapter qualification suite",
    "Protocol conformance tests, latency profiling, fee schedule verification, and simulated failover audits for exchange and broker connectors (LIFE-07, PORT-02)."
  );
  adapterQualificationPanel.id = "adapter-qualification-panel";
  appendAdvancedEvidenceRows(
    adapterQualificationPanel,
    snapshot,
    context,
    "adapter_qualification",
    parseAdapterQualification,
    ["Adapter ID", "Venue / Counterparty", "Protocol / Channel", "Order Lifecycle Coverage", "Fee Audit State", "Failover Invariant", "Qualification Status"],
    (qualification) => [[
      qualification.qualification_id,
      qualification.venue,
      `${qualification.asset_class}; ${qualification.adapter_version}`,
      qualification.supported_capabilities.join(", "),
      `Reconciliation pass rate ${qualification.reconciliation_pass_rate_pct}`,
      qualification.single_writer_fenced ? "Single-writer fenced" : "Not single-writer fenced",
      `${qualification.operational_gate_status}; expires ${qualification.expires_at}`,
    ]],
    "No typed adapter qualification is published."
  );
  root.append(adapterQualificationPanel);

  const operationsAssistantPanel = createPanel(
    "Personal operations assistant and runbook diagnosis",
    "Automated incident diagnosis proposing tested, idempotent runbook steps for non-trading infrastructure recovery without risk of unapproved live trading restarts (AI-05)."
  );
  operationsAssistantPanel.id = "operations-assistant-panel";
  appendAdvancedEvidenceRows(
    operationsAssistantPanel,
    snapshot,
    context,
    "operations_diagnosis_runbook",
    parseOperationsDiagnosisRunbook,
    ["Diagnosis ID", "Incident Target", "Failing Component", "Diagnosed Root Cause", "Proposed Idempotent Runbook", "Isolation & Gate Status"],
    (diagnosis) => [[
      diagnosis.diagnosis_id,
      diagnosis.incident_id,
      diagnosis.failing_component,
      diagnosis.root_cause_summary,
      diagnosis.proposed_runbook_steps.map((step) => `${step.step_number}. ${step.action_name} on ${step.target_service} (${step.is_idempotent ? "idempotent" : "not certified"})`).join(" | "),
      `isolated: ${diagnosis.trading_path_isolated}; ${diagnosis.approval_required}`,
    ]],
    "No typed operations diagnosis is published."
  );
  root.append(operationsAssistantPanel);

  const modelEvaluationPanel = createPanel(
    "AI model evaluation and portability benchmark",
    "Systematic comparison of AI assistance models evaluating factuality, citation precision, prompt injection resistance, and latency on retained operator tasks (AI-06)."
  );
  modelEvaluationPanel.id = "model-evaluation-panel";
  appendAdvancedEvidenceRows(
    modelEvaluationPanel,
    snapshot,
    context,
    "model_evaluation_benchmark",
    parseModelEvaluationBenchmark,
    ["Model Identifier", "Evaluation Task Set", "Factuality (bps)", "Citation Precision (bps)", "Injection Resistance (bps)", "Hallucination Rate (bps)", "Avg Latency", "Qualification Status"],
    (benchmark) => [[
      benchmark.model_identifier,
      benchmark.evaluation_dataset_id,
      String(benchmark.factuality_score_bps),
      String(benchmark.citation_precision_bps),
      String(benchmark.injection_resistance_score_bps),
      String(benchmark.hallucination_rate_bps),
      `${benchmark.average_latency_ms} ms`,
      benchmark.disposition,
    ]],
    "No typed model-evaluation benchmark is published."
  );
  root.append(modelEvaluationPanel);

  const workspaceRebuildPanel = createPanel(
    "Workspace disaster recovery and snapshot rebuild",
    "Content-addressed point-in-time snapshot manifests and empirical disaster recovery drills (DUR-04, DUR-08, LIFE-01, LIFE-02, LIFE-03)."
  );
  workspaceRebuildPanel.id = "workspace-rebuild-panel";
  appendAdvancedEvidenceRows(
    workspaceRebuildPanel,
    snapshot,
    context,
    "workspace_snapshot_manifest",
    parseWorkspaceSnapshotManifest,
    ["Manifest ID", "As-Of Time", "Retained / Source Events", "Window Kind", "Active Accounts", "Positions Hash", "Ledger Balance Hash", "Diagnostics Count"],
    (manifest) => [[
      manifest.manifest_id,
      manifest.as_of_time,
      `${manifest.retained_event_count} / ${manifest.source_event_count}`,
      `${manifest.event_window.window_kind} (${manifest.event_window.first_event_time ?? "start"} to ${manifest.event_window.last_event_time ?? "end"})`,
      manifest.active_accounts.join(", ") || "None",
      shortHash(manifest.positions_fingerprint),
      shortHash(manifest.ledger_balance_fingerprint),
      String(manifest.diagnostics.length),
    ]],
    "No typed workspace snapshot manifest is published."
  );
  appendAdvancedEvidenceRows(
    workspaceRebuildPanel,
    snapshot,
    context,
    "recovery_drill_result",
    parseRecoveryDrillResult,
    ["Drill ID", "Scenario", "Injected Fault", "Measured RTO / Target", "Measured RPO / Target", "Reconciliation Match", "Drill Passed", "Executed At"],
    (drill) => [[
      drill.drill_id,
      drill.scenario_name,
      drill.injected_fault,
      `${drill.measured_rto_seconds}s / ${drill.target_rto_seconds}s`,
      `${drill.measured_rpo_events_lost} events / ${drill.target_rpo_events_lost}`,
      drill.reconciliation_hash_matched ? "MATCHED" : "MISMATCH",
      drill.drill_passed ? "PASSED" : "FAILED",
      drill.executed_at,
    ]],
    "No typed disaster recovery drill result is published."
  );
  root.append(workspaceRebuildPanel);

  const gatewayMatrixPanel = createPanel(
    "Granular gateway route qualification matrix",
    "Per-route certified capabilities, order types, latency bounds, and single-writer fencing epochs (DUR-10, LIFE-07)."
  );
  gatewayMatrixPanel.id = "gateway-matrix-panel";
  appendAdvancedEvidenceRows(
    gatewayMatrixPanel,
    snapshot,
    context,
    "gateway_qualification_matrix",
    parseGatewayQualificationMatrix,
    ["Matrix ID", "Environment", "Gateway ID", "Fencing Epoch", "Capabilities (Asset / State / Latency / Slices)", "Evaluated At", "Expires At"],
    (matrix) => [[
      matrix.matrix_id,
      matrix.environment,
      matrix.gateway_id,
      String(matrix.fencing_epoch),
      matrix.qualified_capabilities.map((c) => `${c.capability_id} (${c.asset_class}): ${c.qualification_state}, p99: ${c.measured_p99_latency_ms}ms, slices: ${c.max_supported_slices}, acc: ${c.reconciliation_accuracy_bps}bps`).join(" | "),
      matrix.evaluated_at,
      matrix.expires_at,
    ]],
    "No typed gateway qualification matrix is published."
  );
  root.append(gatewayMatrixPanel);

  const compatibilityMatrixPanel = createPanel(
    "Multi-year schema version compatibility matrix",
    "Engine schema compatibility, reader migration functions, and golden corpus regression proofs (DUR-12)."
  );
  compatibilityMatrixPanel.id = "compatibility-matrix-panel";
  appendAdvancedEvidenceRows(
    compatibilityMatrixPanel,
    snapshot,
    context,
    "compatibility_matrix",
    parseCompatibilityMatrix,
    ["Matrix ID", "Engine Version", "Registered Schemas (Current / Oldest / Migration)", "Backward Compatible", "Golden Corpus Size", "Verified At"],
    (matrix) => [[
      matrix.matrix_id,
      matrix.engine_version,
      matrix.registered_schemas.map((s) => `${s.schema_name} (v${s.current_version} <- v${s.oldest_supported_version}): ${s.migration_status}`).join(" | "),
      matrix.backward_compatibility_verified ? "VERIFIED" : "UNVERIFIED",
      `${matrix.golden_corpus_size} fixtures`,
      matrix.verified_at,
    ]],
    "No typed compatibility matrix is published."
  );
  root.append(compatibilityMatrixPanel);

  root.append(renderFeatureEvidence(context, ["commercial", "identity", "platform"]));
}

function artifactCount(snapshot: WorkspaceSnapshot, pattern: RegExp): number {
  return snapshot.commercial_artifacts.filter((artifact) => pattern.test(artifact.name)).length;
}
