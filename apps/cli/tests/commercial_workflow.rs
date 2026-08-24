//! End-to-end checks for commercial evidence, privacy retention, and signed self-host readiness.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use sha2::{Digest, Sha256};

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_follon-admin"))
}

fn run(command: &mut Command) -> String {
    let output = command.output().expect("admin command should start");
    assert!(
        output.status.success(),
        "admin command failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8(output.stdout).expect("admin output must be UTF-8")
}

fn sha256(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

#[test]
fn commercial_evidence_privacy_and_signed_self_host_workflow_is_fail_closed() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = repository.join("tests/fixtures/config");
    let workspace = std::env::temp_dir().join(format!(
        "follon-commercial-workflow-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&workspace).unwrap();

    let ledger = workspace.join("ledger/commercial.ndjson");
    run(command()
        .arg("provision")
        .arg(fixtures.join("commercial-provisioning-v1.json"))
        .arg("--ledger")
        .arg(&ledger)
        .arg("--event-id")
        .arg("event.provision.acme.001")
        .arg("--actor")
        .arg("operator.alice"));
    run(command()
        .arg("subscription")
        .arg(fixtures.join("commercial-subscription-v1.json"))
        .arg("--ledger")
        .arg(&ledger)
        .arg("--event-id")
        .arg("event.subscription.acme.001")
        .arg("--actor")
        .arg("billing.stripe")
        .arg("--observed-at")
        .arg("2026-08-12T09:01:00Z"));
    let entitlement: serde_json::Value = serde_json::from_str(&run(command()
        .arg("entitlement")
        .arg("tenant.acme")
        .arg("--ledger")
        .arg(&ledger)
        .arg("--as-of")
        .arg("2026-08-12T10:00:00Z")))
    .unwrap();
    assert_eq!(entitlement["access"], "FULL");
    assert_eq!(entitlement["maximum_members"], 1);

    let data_root = workspace.join("customer-data");
    fs::create_dir_all(&data_root).unwrap();
    fs::write(data_root.join("expired-customer.txt"), "expired data").unwrap();
    fs::write(data_root.join("audit-hold.txt"), "held audit evidence").unwrap();
    fs::write(data_root.join("privacy-customer.txt"), "privacy data").unwrap();
    let inventory = fixtures.join("commercial-data-inventory-v1.json");
    let retention_plan = workspace.join("evidence/retention-plan.json");
    let retention_plan_hash = run(command()
        .arg("retention-plan")
        .arg(&inventory)
        .arg("--data-root")
        .arg(&data_root)
        .arg("--tenant-id")
        .arg("tenant.acme")
        .arg("--as-of")
        .arg("2026-08-12T10:00:00Z")
        .arg("--output")
        .arg(&retention_plan))
    .trim()
    .to_owned();
    // Same reviewed plan is idempotently published and therefore repeatable.
    assert_eq!(
        retention_plan_hash,
        run(command()
            .arg("retention-plan")
            .arg(&inventory)
            .arg("--data-root")
            .arg(&data_root)
            .arg("--tenant-id")
            .arg("tenant.acme")
            .arg("--as-of")
            .arg("2026-08-12T10:00:00Z")
            .arg("--output")
            .arg(&retention_plan))
        .trim()
    );
    run(command()
        .arg("retention-execute")
        .arg(&retention_plan)
        .arg("--data-root")
        .arg(&data_root)
        .arg("--asset-id")
        .arg("asset.expired.customer")
        .arg("--confirm-plan-hash")
        .arg(&retention_plan_hash)
        .arg("--executed-at")
        .arg("2026-08-12T10:01:00Z")
        .arg("--actor")
        .arg("operator.alice")
        .arg("--receipt")
        .arg(workspace.join("evidence/retention-receipt.json")));
    assert!(!data_root.join("expired-customer.txt").exists());
    assert!(data_root.join("audit-hold.txt").exists());

    let privacy_plan = workspace.join("evidence/privacy-plan.json");
    let privacy_plan_hash = run(command()
        .arg("privacy-plan")
        .arg(&inventory)
        .arg(fixtures.join("commercial-privacy-erasure-v1.json"))
        .arg("--data-root")
        .arg(&data_root)
        .arg("--as-of")
        .arg("2026-08-12T10:02:00Z")
        .arg("--output")
        .arg(&privacy_plan))
    .trim()
    .to_owned();
    run(command()
        .arg("retention-execute")
        .arg(&privacy_plan)
        .arg("--data-root")
        .arg(&data_root)
        .arg("--asset-id")
        .arg("asset.privacy.customer")
        .arg("--confirm-plan-hash")
        .arg(&privacy_plan_hash)
        .arg("--executed-at")
        .arg("2026-08-12T10:03:00Z")
        .arg("--actor")
        .arg("privacy.operator")
        .arg("--receipt")
        .arg(workspace.join("evidence/privacy-receipt.json")));
    assert!(!data_root.join("privacy-customer.txt").exists());

    let artifacts = workspace.join("release");
    fs::create_dir_all(&artifacts).unwrap();
    let executable = artifacts.join("follon-admin");
    fs::write(&executable, "verified admin release bytes").unwrap();
    let manifest_path = workspace.join("release-manifest.json");
    run(command()
        .arg("release-manifest")
        .arg("--release-id")
        .arg("release.acme.001")
        .arg("--version")
        .arg("0.1.0")
        .arg("--created-at")
        .arg("2026-08-12T10:04:00Z")
        .arg("--source-revision")
        .arg("e".repeat(40))
        .arg("--sbom-sha256")
        .arg("d".repeat(64))
        .arg("--artifacts-root")
        .arg(&artifacts)
        .arg("--artifact")
        .arg("follon.admin=follon-admin")
        .arg("--output")
        .arg(&manifest_path));
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    let private_key = workspace.join("release-signing.pk8");
    let trusted_key = workspace.join("trusted-release-key.json");
    run(command()
        .arg("release-keygen")
        .arg("--key-id")
        .arg("release.key.acme.001")
        .arg("--private-key")
        .arg(&private_key)
        .arg("--trusted-key")
        .arg(&trusted_key));
    let signature = workspace.join("release-signature.json");
    run(command()
        .arg("release-sign")
        .arg(&manifest_path)
        .arg("--private-key")
        .arg(&private_key)
        .arg("--key-id")
        .arg("release.key.acme.001")
        .arg("--signed-at")
        .arg("2026-08-12T10:05:00Z")
        .arg("--output")
        .arg(&signature));
    run(command()
        .arg("release-verify")
        .arg(&manifest_path)
        .arg(&signature)
        .arg(&trusted_key)
        .arg("--artifacts-root")
        .arg(&artifacts));

    let self_host_provisioning = workspace.join("self-host-provisioning.json");
    fs::write(
        &self_host_provisioning,
        "{\"commercial_provisioning_schema_version\":1,\"tenant_id\":\"tenant.hosted\",\"workspace_id\":\"workspace.hosted\",\"plan\":\"SELF_HOSTED\",\"retention_policy_id\":\"retention.standard.1\",\"self_hosted\":true,\"provisioned_at\":\"2026-08-12T10:05:00Z\"}",
    )
    .unwrap();
    let self_host_subscription = workspace.join("self-host-subscription.json");
    fs::write(
        &self_host_subscription,
        "{\"commercial_subscription_schema_version\":1,\"subscription_id\":\"subscription.hosted.001\",\"tenant_id\":\"tenant.hosted\",\"plan\":\"SELF_HOSTED\",\"status\":\"PAID\",\"effective_at\":\"2026-08-12T10:05:00Z\",\"expires_at\":\"2026-09-12T10:05:00Z\",\"payment_provider\":\"stripe\",\"external_customer_ref\":\"customer.hosted.001\",\"payment_evidence_hash\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}",
    )
    .unwrap();
    let self_host_ledger = workspace.join("ledger/self-host.ndjson");
    run(command()
        .arg("provision")
        .arg(&self_host_provisioning)
        .arg("--ledger")
        .arg(&self_host_ledger)
        .arg("--event-id")
        .arg("event.provision.hosted.001")
        .arg("--actor")
        .arg("operator.alice"));
    run(command()
        .arg("subscription")
        .arg(&self_host_subscription)
        .arg("--ledger")
        .arg(&self_host_ledger)
        .arg("--event-id")
        .arg("event.subscription.hosted.001")
        .arg("--actor")
        .arg("billing.stripe")
        .arg("--observed-at")
        .arg("2026-08-12T10:05:01Z"));
    let self_host = format!(
        "{{\"bind_address\":\"127.0.0.1\",\"instance_id\":\"instance.hosted.001\",\"ledger_relative_path\":\"ledger/commercial.ndjson\",\"release_manifest_hash\":\"{}\",\"release_signature_hash\":\"{}\",\"retention_policy_id\":\"retention.standard.1\",\"secret_provider_kind\":\"managed_command\",\"self_host_config_schema_version\":1,\"storage_relative_path\":\"storage\",\"tenant_id\":\"tenant.hosted\",\"trusted_release_key_id\":\"release.key.acme.001\"}}",
        sha256(manifest.as_bytes()),
        sha256(&fs::read(&signature).unwrap()),
    );
    let self_host_path = workspace.join("self-host.json");
    fs::write(&self_host_path, self_host).unwrap();
    run(command().arg("self-host-validate").arg(&self_host_path));
    let readiness = workspace.join("evidence/self-host-readiness.json");
    let denied = command()
        .arg("self-host-readiness")
        .arg(&self_host_path)
        .arg(&manifest_path)
        .arg(&signature)
        .arg(&trusted_key)
        .arg("--artifacts-root")
        .arg(&artifacts)
        .arg("--ledger")
        .arg(&ledger)
        .arg("--as-of")
        .arg("2026-08-12T10:06:00Z")
        .arg("--output")
        .arg(&readiness)
        .output()
        .unwrap();
    assert!(
        !denied.status.success(),
        "wrong tenant entitlement must fail closed"
    );
    let readiness_output = run(command()
        .arg("self-host-readiness")
        .arg(&self_host_path)
        .arg(&manifest_path)
        .arg(&signature)
        .arg(&trusted_key)
        .arg("--artifacts-root")
        .arg(&artifacts)
        .arg("--ledger")
        .arg(&self_host_ledger)
        .arg("--as-of")
        .arg("2026-08-12T10:06:00Z")
        .arg("--output")
        .arg(&readiness));
    assert!(readiness_output.contains("\"state\":\"READY\""));
    fs::write(&executable, "tampered").unwrap();
    let failed = command()
        .arg("release-verify")
        .arg(&manifest_path)
        .arg(&signature)
        .arg(&trusted_key)
        .arg("--artifacts-root")
        .arg(&artifacts)
        .output()
        .unwrap();
    assert!(
        !failed.status.success(),
        "tampered release must fail closed"
    );

    fs::remove_dir_all(workspace).unwrap();
}
