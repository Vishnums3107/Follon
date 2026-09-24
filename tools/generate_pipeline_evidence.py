#!/usr/bin/env python3
"""Deterministic end-to-end evidence workflow generator for Follon.

Executes all core CLI tools across all 12 Enduring Capabilities (DUR-01 to DUR-12)
in sequential, attributable order to populate the var/ evidence directory with
verifiable, immutable artifacts for the desktop terminal and audit pipelines.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
VAR_DIR = REPOSITORY_ROOT / "var"


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    h.update(path.read_bytes())
    return h.hexdigest()


def run_step(
    description: str,
    cmd: list[str],
    targets: list[Path] | None = None,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    if targets:
        for target in targets:
            try:
                target.unlink(missing_ok=True)
            except OSError:
                pass
    print(f"\n[+] {description}")
    print(f"    Command: {' '.join(cmd)}")
    result = subprocess.run(
        cmd,
        cwd=str(REPOSITORY_ROOT),
        capture_output=True,
        text=True,
        env=None if env is None else {**os.environ, **env},
    )
    if result.returncode != 0:
        print(f"[-] FAILED (exit code {result.returncode})")
        if result.stdout:
            print("--- STDOUT ---")
            print(result.stdout)
        if result.stderr:
            print("--- STDERR ---")
            print(result.stderr)
        sys.exit(1)
    if result.stdout.strip():
        for line in result.stdout.strip().splitlines()[:5]:
            print(f"    {line}")
    print("    [OK]")
    return result


def main() -> None:
    VAR_DIR.mkdir(parents=True, exist_ok=True)
    print("=================================================================")
    print("   FOLLON QUANTITATIVE TRADING OS - PIPELINE EVIDENCE GENERATOR   ")
    print("=================================================================")
    print(f"Repository Root: {REPOSITORY_ROOT}")
    print(f"Artifact Target: {VAR_DIR}\n")

    # 1. Bar Construction from Trades
    bars_target = VAR_DIR / "follon-bars.csv"
    run_step(
        "Step 1: Constructing canonical minute bars from tick trade feed",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-build-bars", "--",
            "tests/fixtures/historical-bars/spy-trades-v1.csv",
            str(bars_target),
        ],
        targets=[bars_target],
    )

    # 2. Backtest Replay & Advanced Account Economics (DUR-03)
    backtest_targets = [
        VAR_DIR / "follon-backtest-artifact.json",
        VAR_DIR / "follon-backtest-artifact.events.ndjson",
        VAR_DIR / "follon-backtest-artifact.report.md",
        VAR_DIR / "follon-backtest-artifact.manifest.json",
        VAR_DIR / "follon-backtest-artifact.advanced-account.json",
        VAR_DIR / "follon-backtest-artifact.advanced-report.md",
    ]
    run_step(
        "Step 2: Executing deterministic replay backtest & advanced margin projection",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-backtest", "--",
            "tests/fixtures/historical-bars/spy-one-minute.csv",
            str(VAR_DIR / "follon-backtest-artifact.json"),
        ],
        targets=backtest_targets,
    )

    # 3. Adversarial Research Gate 5-Probe Stress Audit (DUR-06)
    adv_target = VAR_DIR / "adversarial-eval.json"
    run_step(
        "Step 3: Conducting Adversarial Research Gate 5-probe stress evaluation",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-backtest", "--",
            "adversarial",
            "tests/fixtures/config/adversarial-v1.json",
            str(adv_target),
        ],
        targets=[adv_target],
    )

    # 4. Counterfactual Safety Laboratory Interventions (DUR-02)
    cf_target = VAR_DIR / "counterfactual.json"
    run_step(
        "Step 4: Executing Counterfactual Safety Lab intervention scenario",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-backtest", "--",
            "counterfactual",
            "tests/fixtures/config/counterfactual-v1.json",
            str(cf_target),
        ],
        targets=[cf_target],
    )

    # 5. Options Analytics & Cross-Environment Reconciliation
    opt_dash = VAR_DIR / "follon-options-dashboard.json"
    opt_rep = VAR_DIR / "follon-options-report.md"
    run_step(
        "Step 5a: Evaluating Options Black-Scholes Greeks, IV, and Scenario Dashboard",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-options", "--",
            "analyze",
            "tests/fixtures/config/options-v1.json",
            str(opt_dash),
        ],
        targets=[opt_dash],
    )
    run_step(
        "Step 5b: Generating Options Markdown Verification Report",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-options", "--",
            "report",
            "tests/fixtures/config/options-v1.json",
            str(opt_rep),
        ],
        targets=[opt_rep],
    )

    # 6. Deterministic FX Valuation & Snapshot Pricing
    fx_price = VAR_DIR / "follon-fx-pricing.json"
    fx_rep = VAR_DIR / "follon-fx-report.md"
    run_step(
        "Step 6a: Evaluating Deterministic FX Pricing Dashboard (Spot, Forward, Swap)",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-fx", "--",
            "price",
            "tests/fixtures/config/fx-v1.json",
            str(fx_price),
            "--as-of", "2026-09-05T12:00:00Z",
        ],
        targets=[fx_price],
    )
    run_step(
        "Step 6b: Generating Deterministic FX Markdown Report",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-fx", "--",
            "report",
            "tests/fixtures/config/fx-v1.json",
            str(fx_rep),
            "--as-of", "2026-09-05T12:00:00Z",
        ],
        targets=[fx_rep],
    )

    # 7. Operations Cockpit, Schedules, and Broker Statement Reconciliation
    journal_path = VAR_DIR / "follon-operations.journal.ndjson"
    ops_dash = VAR_DIR / "follon-operations-dashboard.json"
    ops_sched = VAR_DIR / "follon-operations-schedule.json"
    ops_recon = VAR_DIR / "follon-statement-reconciliation.json"
    run_step(
        "Step 7a: Projecting Operations Dashboard & Health State",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-operations", "--",
            "dashboard",
            "tests/fixtures/config/operations-v1.json",
            str(ops_dash),
            "--as-of", "2026-08-10T16:30:00Z",
            "--journal", str(journal_path),
        ],
        targets=[ops_dash],
    )
    run_step(
        "Step 7b: Generating Operations Schedule Plan",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-operations", "--",
            "schedule",
            "tests/fixtures/config/operations-v1.json",
            str(ops_sched),
            "--as-of", "2026-08-10T16:30:00Z",
            "--journal", str(journal_path),
        ],
        targets=[ops_sched],
    )
    run_step(
        "Step 7c: Ingesting Broker CSV Statement and Reconciling Internal Ledger",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-operations", "--",
            "reconcile-statement",
            "tests/fixtures/config/operations-v1.json",
            "tests/fixtures/broker-statement-v1.csv",
            str(ops_recon),
            "--as-of", "2026-08-10T16:30:00Z",
        ],
        targets=[ops_recon],
    )

    # 8. Continuous Recovery Game-Day Drill Compiler (DUR-08)
    drill_target = VAR_DIR / "recovery-drill.json"
    run_step(
        "Step 8: Compiling Continuous Recovery Game-Day Drill (DUR-08)",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-operations", "--",
            "recovery-drill",
            "tests/fixtures/config/recovery-drill-v1.json",
            str(drill_target),
        ],
        targets=[drill_target],
    )

    # 9. Attention Budget & Cognitive Load Controller (DUR-05)
    budget_target = VAR_DIR / "attention-budget.json"
    run_step(
        "Step 9: Evaluating Operator Attention Budget & Cognitive Load (DUR-05)",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-operations", "--",
            "attention-budget",
            "tests/fixtures/config/attention-budget-v1.json",
            str(budget_target),
        ],
        targets=[budget_target],
    )

    # 10. Golden Corpus Schema Compatibility Matrix (DUR-01)
    compat_target = VAR_DIR / "compat-matrix.json"
    run_step(
        "Step 10: Verifying Golden Corpus Compatibility Matrix (DUR-01)",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-operations", "--",
            "compatibility-matrix",
            str(compat_target),
        ],
        targets=[compat_target],
    )

    # 11. Transaction-Cost Analysis (TCA) Execution Benchmarks
    tca_targets = [
        VAR_DIR / "follon-tca-report.json",
        VAR_DIR / "follon-tca-report.manifest.json",
        VAR_DIR / "follon-tca-report.report.md",
    ]
    run_step(
        "Step 11: Computing Parent-Order TCA Implementation Shortfall Evidence",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-tca", "--",
            "tests/fixtures/config/tca-v1.json",
            str(VAR_DIR / "follon-tca-report.json"),
        ],
        targets=tca_targets,
    )

    # 12. Risk Engine Latency Benchmark
    risk_bench = VAR_DIR / "follon-risk-benchmark.json"
    run_step(
        "Step 12: Executing Risk Engine p99 Latency Benchmark Measurement",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-risk-benchmark", "--",
            "tests/fixtures/config/risk-benchmark-v1.json",
            str(risk_bench),
        ],
        targets=[risk_bench],
    )

    # 13. Equal-Risk-Contribution Capital Allocation Proposal (DUR-11)
    cap_target = VAR_DIR / "capital-proposal.json"
    run_step(
        "Step 13: Building Equal-Risk-Contribution Capital Allocation Proposal (DUR-11)",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-risk-benchmark", "--",
            "capital-proposal",
            "tests/fixtures/config/capital-allocation-v1.json",
            str(cap_target),
        ],
        targets=[cap_target],
    )

    # 14. Paper Trading Status & Gateway Qualification (DUR-10)
    paper_dash = VAR_DIR / "follon-paper-dashboard.json"
    run_step(
        "Step 14a: Projecting Paper Trading Dashboard Snapshot",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-paper-status", "--",
            "tests/fixtures/paper/journal-v2.ndjson",
            str(paper_dash),
            "--config", "tests/fixtures/config/paper-v2.json",
        ],
        targets=[paper_dash],
    )
    gqm_target = VAR_DIR / "gateway-matrix.json"
    run_step(
        "Step 14b: Qualifying Gateway Routes Across Capabilities & Latency (DUR-10)",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-paper-status", "--",
            "gateway-matrix",
            "tests/fixtures/config/gateway-matrix-v1.json",
            str(gqm_target),
        ],
        targets=[gqm_target],
    )

    # 15. Controlled Live Status Dashboard
    #
    # `follon-live-status` durably appends a `live.service.restarted.v1` audit
    # event to whatever journal it opens -- intentional for a real LIVE
    # journal (every open must be recorded), but this pipeline's input is a
    # checked-in, immutable test fixture, not a live journal. Opening it
    # in place silently mutated `tests/fixtures/live/journal-v1.ndjson` by
    # two lines on every pipeline run (found and fixed 2026-09-20, see
    # docs/06-delivery/14-master-plan-conformance-audit.md item 45). Copy it
    # into var/ first, matching every other step's write-to-var/-only
    # discipline, so the fixture stays a frozen input.
    live_journal_copy = VAR_DIR / "follon-live-journal.ndjson"
    live_journal_copy.write_bytes(
        (REPOSITORY_ROOT / "tests" / "fixtures" / "live" / "journal-v1.ndjson").read_bytes()
    )
    live_dash = VAR_DIR / "follon-live-dashboard.json"
    run_step(
        "Step 15: Projecting Controlled-Live Monitoring Snapshot",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-live-status", "--",
            str(live_journal_copy),
            str(live_dash),
            "--opened-at", "2026-08-11T13:30:00Z",
            "--config", "tests/fixtures/config/live-v1.json",
        ],
        targets=[live_dash],
    )

    # 16. Commercial Ledger Provisioning & Subscription
    comm_ledger = VAR_DIR / "follon-commercial.ledger.ndjson"
    run_step(
        "Step 16a: Ingesting Commercial Provisioning Audit Record",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "provision",
            "tests/fixtures/config/commercial-provisioning-v1.json",
            "--ledger", str(comm_ledger),
            "--event-id", "event.provision.acme.001",
            "--actor", "operator.alice",
        ],
        targets=[comm_ledger],
    )
    run_step(
        "Step 16b: Ingesting Commercial Subscription Invariant Record",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "subscription",
            "tests/fixtures/config/commercial-subscription-v1.json",
            "--ledger", str(comm_ledger),
            "--event-id", "event.subscription.acme.001",
            "--actor", "billing.stripe",
            "--observed-at", "2026-08-12T09:01:00Z",
        ],
    )
    run_step(
        "Step 16c: Ingesting Commercial Self-Hosted Provisioning Audit Record",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "provision",
            "tests/fixtures/config/commercial-self-host-provisioning-v1.json",
            "--ledger", str(comm_ledger),
            "--event-id", "event.provision.hosted.001",
            "--actor", "operator.alice",
        ],
    )
    run_step(
        "Step 16d: Ingesting Commercial Self-Hosted Subscription Invariant Record",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "subscription",
            "tests/fixtures/config/commercial-self-host-subscription-v1.json",
            "--ledger", str(comm_ledger),
            "--event-id", "event.subscription.hosted.001",
            "--actor", "billing.stripe",
            "--observed-at", "2026-09-07T12:01:00Z",
        ],
    )

    # 16e. Commercial Retention Execution (DUR-11)
    cust_data_dir = VAR_DIR / "customer-data"
    cust_data_dir.mkdir(parents=True, exist_ok=True)
    (cust_data_dir / "expired-customer.txt").write_text("expired customer data\n", encoding="utf-8")
    (cust_data_dir / "audit-hold.txt").write_text("audit hold data\n", encoding="utf-8")
    (cust_data_dir / "privacy-customer.txt").write_text("privacy customer data\n", encoding="utf-8")

    retention_plan = VAR_DIR / "commercial-retention-plan.json"
    run_step(
        "Step 16e(i): Compiling Commercial Retention Execution Plan",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "retention-plan",
            "tests/fixtures/config/commercial-data-inventory-v1.json",
            "--data-root", str(cust_data_dir),
            "--tenant-id", "tenant.acme",
            "--as-of", "2026-08-12T09:00:00Z",
            "--output", str(retention_plan),
        ],
        targets=[retention_plan],
    )
    retention_plan_hash = sha256_file(retention_plan)
    retention_receipt = VAR_DIR / "commercial-retention-receipt.json"
    run_step(
        "Step 16e(ii): Executing Retention Deletion with Plan Hash Concurrency Fence",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "retention-execute",
            str(retention_plan),
            "--data-root", str(cust_data_dir),
            "--asset-id", "asset.expired.customer",
            "--confirm-plan-hash", retention_plan_hash,
            "--executed-at", "2026-08-12T09:05:00Z",
            "--actor", "operator.alice",
            "--receipt", str(retention_receipt),
        ],
        targets=[retention_receipt],
    )

    # 16f. Commercial Privacy Plan and Erasure Execution (DUR-11)
    privacy_plan = VAR_DIR / "commercial-privacy-plan.json"
    run_step(
        "Step 16f(i): Compiling Commercial Privacy Erasure Plan",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "privacy-plan",
            "tests/fixtures/config/commercial-data-inventory-v1.json",
            "tests/fixtures/config/commercial-privacy-erasure-v1.json",
            "--data-root", str(cust_data_dir),
            "--as-of", "2026-08-12T09:00:00Z",
            "--output", str(privacy_plan),
        ],
        targets=[privacy_plan],
    )
    privacy_plan_hash = sha256_file(privacy_plan)
    privacy_receipt = VAR_DIR / "commercial-privacy-receipt.json"
    run_step(
        "Step 16f(ii): Executing Privacy Erasure with Plan Hash Concurrency Fence",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "retention-execute",
            str(privacy_plan),
            "--data-root", str(cust_data_dir),
            "--asset-id", "asset.privacy.customer",
            "--confirm-plan-hash", privacy_plan_hash,
            "--executed-at", "2026-08-12T09:06:00Z",
            "--actor", "operator.alice",
            "--receipt", str(privacy_receipt),
        ],
        targets=[privacy_receipt],
    )

    # 16g. Advanced Evidence Contract Validation (DUR-01 through DUR-12)
    #
    # `build_advanced_evidence_fixtures.py` validates 32 hand-authored example
    # documents against their JSON schemas -- a legitimate contract test. It
    # does NOT compute real evidence: no domain crate or CLI backs 29 of the
    # 32 categories at all. Of the 3 that do, `decision-reconstruction` is
    # computed in step 16h from the real step-2 journal and
    # `strategy-capsule-manifest` in step 16i from a real Python-worker
    # evaluation; `data-rights-and-semantics-receipt` is not invoked, because
    # nothing measures its parity score. These are schema-validation fixtures, not evidence,
    # so they stay in tests/fixtures/ and are deliberately not copied into
    # var/, which the desktop dashboard reads as real, dated evidence. Doing
    # so previously violated the dashboard's own zero-synthetic-data invariant
    # (see docs/06-delivery/14-master-plan-conformance-audit.md item 45) the
    # same way items 23-24 already found and fixed once before.
    run_step(
        "Step 16g: Validating Advanced Evidence Fixture Contracts",
        [
            sys.executable, "tools/build_advanced_evidence_fixtures.py",
        ],
    )

    # 16h. Decision provenance reconstruction (DUR-01), computed, not typed.
    #
    # Walks the causal chain of the latest fill in the real step-2 backtest
    # journal. The CLI refuses a journal that does not hash to its manifest's
    # `events_sha256`, binds the manifest's own `configuration_hash`, and
    # hashes each node's exact persisted line. `--verified-at` is explicit so
    # a re-run over the same journal reproduces the same document.
    reconstruction_target = VAR_DIR / "decision-reconstruction.json"
    run_step(
        "Step 16h: Reconstructing Decision Provenance from the Backtest Journal",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-operations", "--",
            "decision-reconstruction",
            str(VAR_DIR / "follon-backtest-artifact.events.ndjson"),
            str(VAR_DIR / "follon-backtest-artifact.manifest.json"),
            str(reconstruction_target),
            "--verified-at", "2026-09-07T12:00:00Z",
        ],
        targets=[reconstruction_target],
    )

    # 16i. Portable strategy capsule (DUR-07, ASSET-04), sealed only after a
    # sandboxed replay of its own contents reproduces a real evaluation.
    #
    # (i) The SDK locks the example worker strategy bundle. (ii) The Python
    # worker is evaluated through the real backtest runner, which verifies the
    # worker's announced bundle hash and records it in the artifact. (iii)
    # `capsule-package` rebuilds the archive from the trees, checks it against
    # the lock and the evaluation, replays the capsule's own copies with no
    # site packages or inherited import path, and seals the manifest only if
    # the replay reproduces the completion manifest byte for byte. (iv)
    # `capsule-verify` re-checks the sealed capsule from disk and replays it
    # again. `--packaged-at` is explicit so a re-run reproduces the manifest.
    python = str(Path(sys.executable).resolve())
    sdk_source = REPOSITORY_ROOT / "python" / "strategy-sdk" / "src"
    evaluation_dir = VAR_DIR / "strategy-evaluation"
    capsule_dir = VAR_DIR / "strategy-capsule"
    for stale in (evaluation_dir, capsule_dir):
        shutil.rmtree(stale, ignore_errors=True)
    evaluation_dir.mkdir(parents=True)
    capsule_lock = evaluation_dir / "dependency.lock"
    locked = run_step(
        "Step 16i(i): Locking the Python worker strategy bundle",
        [
            python, "-m", "follon_strategy_sdk.bundle_lock",
            "--bundle-root", "python/examples",
            "--strategy-file", "python/examples/worker_buy_once_strategy.py",
            "--class-name", "WorkerBuyOnceStrategy",
            "--output", str(capsule_lock),
        ],
        env={"PYTHONPATH": str(sdk_source)},
    )
    bundle_hash = locked.stdout.strip()
    evaluation_artifact = evaluation_dir / "python-worker-backtest.json"
    run_step(
        "Step 16i(ii): Evaluating the locked strategy through the isolated Python worker",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-backtest", "--",
            "tests/fixtures/historical-bars/spy-one-minute.csv",
            str(evaluation_artifact),
            "--python-worker", python,
            "python/examples/worker_buy_once_strategy.py", "WorkerBuyOnceStrategy",
            "python/examples", "strategy-example-001", "strategy-example-v1", bundle_hash,
        ],
        env={"FOLLON_STRATEGY_SDK_PATH": str(sdk_source)},
    )
    run_step(
        "Step 16i(iii): Sealing the portable strategy capsule after a sandboxed replay",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-backtest", "--",
            "capsule-package",
            "--bundle-root", "python/examples",
            "--sdk-root", str(sdk_source / "follon_strategy_sdk"),
            "--lock", str(capsule_lock),
            "--config", "tests/fixtures/config/backtest-v1.json",
            "--evaluation", str(evaluation_artifact),
            "--bars", "tests/fixtures/historical-bars/spy-one-minute.csv",
            "--python", python,
            "--packaged-at", "2026-09-07T12:00:00Z",
            "--output", str(capsule_dir),
        ],
    )
    run_step(
        "Step 16i(iv): Independently re-verifying and replaying the sealed capsule",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-backtest", "--",
            "capsule-verify", str(capsule_dir),
            "--bars", "tests/fixtures/historical-bars/spy-one-minute.csv",
            "--python", python,
        ],
    )


    # 17. News Sentiment NLP Stream
    news_replay = VAR_DIR / "follon-news-replay.ndjson"
    run_step(
        "Step 17: Scoring and Replaying News Sentiment Feed Vectors",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-news", "--",
            "replay",
            "tests/fixtures/news/2026-09-01-headlines.ndjson",
            "--output", str(news_replay),
        ],
        targets=[news_replay],
    )

    # 18. CycloneDX / Lockfile-Backed Software Bill of Materials (SBOM)
    sbom_target = VAR_DIR / "follon-sbom.json"
    run_step(
        "Step 18: Generating CycloneDX / Lockfile-Backed SBOM",
        [
            sys.executable, "tools/generate_sbom.py",
            "--source-revision", "2d18b40f44c1254e8ac52e9699318af4e7011474",
            "--output", str(sbom_target),
        ],
        targets=[sbom_target],
    )
    sbom_hash = sha256_file(sbom_target)

    # 19. Release Keypair Generation
    priv_key = VAR_DIR / "release-signing.pk8"
    pub_key = VAR_DIR / "trusted-release-key.json"
    run_step(
        "Step 19: Generating Cryptographic Release Keypair",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "release-keygen",
            "--key-id", "release.key.follon.001",
            "--private-key", str(priv_key),
            "--trusted-key", str(pub_key),
        ],
        targets=[priv_key, pub_key],
    )

    # 20. Release Manifest Compilation
    manifest_target = VAR_DIR / "release-manifest.json"
    run_step(
        "Step 20: Compiling Content-Addressed Immutable Release Manifest",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "release-manifest",
            "--release-id", "release.follon.2026.09.07",
            "--version", "0.1.0",
            "--created-at", "2026-09-07T12:00:00Z",
            "--source-revision", "2d18b40f44c1254e8ac52e9699318af4e7011474",
            "--sbom-sha256", sbom_hash,
            "--artifacts-root", str(VAR_DIR),
            "--artifact", "follon.bars=follon-bars.csv",
            "--output", str(manifest_target),
        ],
        targets=[manifest_target],
    )
    manifest_hash = sha256_file(manifest_target)

    # 21. Release Manifest Cryptographic Signing & Verification
    signature_target = VAR_DIR / "release-signature.json"
    run_step(
        "Step 21a: Signing Release Manifest with Private Release Key",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "release-sign",
            str(manifest_target),
            "--private-key", str(priv_key),
            "--key-id", "release.key.follon.001",
            "--signed-at", "2026-09-07T12:01:00Z",
            "--output", str(signature_target),
        ],
        targets=[signature_target],
    )
    sig_hash = sha256_file(signature_target)
    run_step(
        "Step 21b: Verifying Release Manifest Signature and Artifacts",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "release-verify",
            str(manifest_target),
            str(signature_target),
            str(pub_key),
            "--artifacts-root", str(VAR_DIR),
        ],
    )

    # 22. Self-Host Configuration Validation & Readiness Verification
    self_host_config_target = VAR_DIR / "self-host-config.json"
    self_host_config_target.unlink(missing_ok=True)
    self_host_data = {
        "bind_address": "127.0.0.1",
        "instance_id": "instance.hosted.001",
        "ledger_relative_path": "follon-commercial.ledger.ndjson",
        "release_manifest_hash": manifest_hash,
        "release_signature_hash": sig_hash,
        "retention_policy_id": "retention.standard.1",
        "secret_provider_kind": "managed_command",
        "self_host_config_schema_version": 1,
        "storage_relative_path": "storage",
        "tenant_id": "tenant.hosted",
        "trusted_release_key_id": "release.key.follon.001",
    }
    self_host_config_target.write_text(
        json.dumps(self_host_data, sort_keys=True, separators=(",", ":")),
        encoding="utf-8",
    )
    run_step(
        "Step 22a: Validating Self-Host Configuration Schema & Identity",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "self-host-validate",
            str(self_host_config_target),
        ],
    )
    readiness_target = VAR_DIR / "follon-self-host-readiness.json"
    run_step(
        "Step 22b: Verifying Entitled Self-Host Deployment Readiness Receipt",
        [
            "cargo", "run", "-q", "-p", "follon-cli", "--bin", "follon-admin", "--",
            "self-host-readiness",
            str(self_host_config_target),
            str(manifest_target),
            str(signature_target),
            str(pub_key),
            "--artifacts-root", str(VAR_DIR),
            "--ledger", str(comm_ledger),
            "--as-of", "2026-09-07T12:02:00Z",
            "--output", str(readiness_target),
        ],
        targets=[readiness_target],
    )

    # 23. Tamper-Evident Acceptance Status Ledger Gate Counts
    acceptance_target = VAR_DIR / "follon-acceptance-status.json"
    run_step(
        "Step 23: Auditing External Acceptance Ledgers & Real Gate Counts",
        [
            sys.executable, "tools/acceptance_evidence.py",
            str(VAR_DIR),
            "--output", str(acceptance_target),
        ],
        targets=[acceptance_target],
    )

    # 24. Summary of Populated Evidence
    print("\n=================================================================")
    print("                    EVIDENCE INVENTORY SUMMARY                    ")
    print("=================================================================")
    artifacts = sorted(VAR_DIR.glob("*"))
    for art in artifacts:
        if art.is_file():
            size_kb = art.stat().st_size / 1024
            digest = sha256_file(art)[:16]
            print(f"  * {art.name:<45} | {size_kb:7.2f} KB | SHA256: {digest}...")

    print(f"\nSuccessfully populated {len([a for a in artifacts if a.is_file()])} immutable evidence artifacts in {VAR_DIR}.")
    # Deliberately not a capability claim: most advanced-evidence categories
    # still have no computation behind them (see step 16g), and no step here
    # moves an external gate. Report only what was measured.
    print("Every pipeline step exited 0. This is local engineering evidence; it closes no external gate "
          "(see docs/06-delivery/16-delivery-state.md).")


if __name__ == "__main__":
    main()
