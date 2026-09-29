//! End-to-end validation for the frozen local risk-latency benchmark artifact.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

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
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let result = Command::new(env!("CARGO_BIN_EXE_follon-risk-benchmark"))
        .args([fixture.to_str().unwrap(), output.to_str().unwrap()])
        .output()
        .expect("risk benchmark command should start");
    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(
        result.status.success(),
        "risk benchmark command failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
    );
    let artifact: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(artifact["benchmark_schema_version"], 2);
    // The fixture's own time is 2026-08-30. The artifact says when the command
    // actually ran, and keeps the scenario time under a different name.
    let measured_at = OffsetDateTime::parse(
        artifact["measured_at"].as_str().expect("measured_at"),
        &Rfc3339,
    )
    .expect("measured_at is an RFC 3339 time")
    .unix_timestamp() as u64;
    assert!(
        (before..=after).contains(&measured_at),
        "measured_at {measured_at} is outside the run [{before}, {after}]"
    );
    assert_eq!(artifact["scenario_observed_at"], "2026-08-30T21:30:00Z");
    assert!(artifact.get("observed_at").is_none());
    assert_eq!(artifact["measurement"]["threshold_micros"], 5_000);
    assert!(artifact["measurement"]["p99_micros"].as_u64().is_some());
    assert!(artifact["input_sha256"].as_str().is_some());
    fs::remove_dir_all(&workspace).unwrap();
}
