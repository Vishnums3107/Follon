//! End-to-end validation for immutable transaction-cost artifacts.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn tca_workflow_writes_a_hash_bound_machine_and_human_eod_artifact() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/config/tca-v1.json");
    let workspace =
        std::env::temp_dir().join(format!("follon-tca-workflow-{}", std::process::id()));
    let _ = fs::remove_dir_all(&workspace);
    fs::create_dir_all(&workspace).unwrap();
    let output = workspace.join("tca.json");
    let result = Command::new(env!("CARGO_BIN_EXE_follon-tca"))
        .args([fixture.to_str().unwrap(), output.to_str().unwrap()])
        .output()
        .expect("TCA command should start");
    assert!(
        result.status.success(),
        "TCA command failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let artifact: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(
        artifact["transaction_cost"]["transaction_cost_schema_version"],
        1
    );
    assert_eq!(
        artifact["transaction_cost"]["reports"][0]["unfilled_quantity"],
        "0.00000000"
    );
    assert!(
        artifact["transaction_cost"]["reports"][0]["arrival_total_cost"]
            .as_str()
            .is_some()
    );
    let report_path = workspace.join("tca.report.md");
    let manifest_path = workspace.join("tca.manifest.json");
    assert!(fs::read_to_string(report_path)
        .unwrap()
        .contains("Transaction-Cost Analysis"));
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(manifest_path).unwrap()).unwrap();
    assert!(manifest["artifact_sha256"].as_str().is_some());
    assert!(manifest["report_sha256"].as_str().is_some());
    fs::remove_dir_all(&workspace).unwrap();
}
