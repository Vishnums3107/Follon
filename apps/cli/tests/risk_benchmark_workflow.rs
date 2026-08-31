//! End-to-end validation for the frozen local risk-latency benchmark artifact.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn risk_benchmark_records_an_explicit_local_measurement() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/config/risk-benchmark-v1.json");
    let workspace = std::env::temp_dir().join(format!(
        "follon-risk-benchmark-workflow-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&workspace);
    fs::create_dir_all(&workspace).unwrap();
    let output = workspace.join("benchmark.json");
    let result = Command::new(env!("CARGO_BIN_EXE_follon-risk-benchmark"))
        .args([fixture.to_str().unwrap(), output.to_str().unwrap()])
        .output()
        .expect("risk benchmark command should start");
    assert!(
        result.status.success(),
        "risk benchmark command failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let artifact: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(artifact["benchmark_schema_version"], 1);
    assert_eq!(artifact["measurement"]["threshold_micros"], 5_000);
    assert!(artifact["measurement"]["p99_micros"].as_u64().is_some());
    assert!(artifact["input_sha256"].as_str().is_some());
    fs::remove_dir_all(&workspace).unwrap();
}
