//! End-to-end validation for immutable operations change and schedule evidence.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

use follon_domain::Decimal;
use follon_operations::{ParameterControl, ParameterSet, ParameterValue};

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_follon-operations"))
}

fn run(command: &mut Command) -> String {
    let output = command.output().expect("operations command should start");
    assert!(
        output.status.success(),
        "operations command failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8(output.stdout).expect("command output must be UTF-8")
}

fn decimal(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

fn risk_parameter(parameter_id: &str, value: &str, minimum: &str, maximum: &str) -> ParameterValue {
    ParameterValue {
        parameter_id: parameter_id.to_owned(),
        value: decimal(value),
        minimum: decimal(minimum),
        maximum: decimal(maximum),
        control: ParameterControl::TwoPerson,
        approval: None,
    }
}

fn target_approval_subject_hash(previous_parameter_set_fingerprint: String) -> String {
    ParameterSet {
        parameter_set_id: "params.mean_revert".to_owned(),
        revision: "8".to_owned(),
        previous_revision: Some("7".to_owned()),
        previous_parameter_set_fingerprint: Some(previous_parameter_set_fingerprint),
        values: vec![
            ParameterValue {
                parameter_id: "entry_zscore".to_owned(),
                value: decimal("2.25"),
                minimum: decimal("1.0"),
                maximum: decimal("3.0"),
                control: ParameterControl::Standard,
                approval: None,
            },
            risk_parameter("risk.max_gross_exposure", "10000", "1000", "20000"),
            risk_parameter(
                "risk.max_single_instrument_exposure",
                "6000",
                "100",
                "10000",
            ),
            risk_parameter("risk.max_drawdown_bps", "1000", "100", "5000"),
            risk_parameter("risk.max_working_orders", "5", "1", "100"),
            risk_parameter("risk.max_unknown_orders", "0", "0", "100"),
            risk_parameter("risk.max_unresolved_incidents", "0", "0", "100"),
        ],
    }
    .approval_subject_fingerprint()
    .unwrap()
}

#[test]
fn parameter_change_and_journal_backed_schedule_workflow_is_reproducible() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/config/operations-v1.json");
    let workspace =
        std::env::temp_dir().join(format!("follon-operations-workflow-{}", std::process::id()));
    let _ = fs::remove_dir_all(&workspace);
    fs::create_dir_all(&workspace).unwrap();
    let target = workspace.join("operations-v2.json");
    let previous_source = fs::read_to_string(&fixture).unwrap();
    let fixture_validation: serde_json::Value = serde_json::from_str(&run(
        command().args(["validate-config", fixture.to_str().unwrap()])
    ))
    .expect("validation output must be JSON");
    let previous_parameter_set_fingerprint = fixture_validation["parameter_set_fingerprint"]
        .as_str()
        .expect("validation must print a parameter-set fingerprint")
        .to_owned();
    let target_subject_hash =
        target_approval_subject_hash(previous_parameter_set_fingerprint.clone());
    let mut target_document: serde_json::Value = serde_json::from_str(&previous_source).unwrap();
    target_document["configuration"]["configuration_version"] =
        serde_json::Value::String("2026.08.10.2".to_owned());
    target_document["parameters"]["revision"] = serde_json::Value::String("8".to_owned());
    target_document["parameters"]["previous_revision"] = serde_json::Value::String("7".to_owned());
    target_document["parameters"]["previous_parameter_set_fingerprint"] =
        serde_json::Value::String(previous_parameter_set_fingerprint);
    let values = target_document["parameters"]["values"]
        .as_array_mut()
        .expect("operations fixture must include parameter values");
    values[0]["value"] = serde_json::Value::String("2.25".to_owned());
    for value in values {
        if let Some(approval) = value["approval"].as_object_mut() {
            approval.insert(
                "approval_subject_hash".to_owned(),
                serde_json::Value::String(target_subject_hash.clone()),
            );
        }
    }
    let target_source = serde_json::to_string_pretty(&target_document).unwrap();
    fs::write(&target, target_source).unwrap();

    let change_path = workspace.join("parameter-changes.json");
    run(command().args([
        "config-diff",
        fixture.to_str().unwrap(),
        target.to_str().unwrap(),
        change_path.to_str().unwrap(),
    ]));
    let changes: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&change_path).unwrap()).unwrap();
    assert_eq!(changes["changes"].as_array().unwrap().len(), 7);
    assert_eq!(changes["changes"][0]["parameter_id"], "entry_zscore");
    assert_eq!(changes["changes"][0]["change_kind"], "MODIFIED");

    let journal_path = workspace.join("operations.journal.ndjson");
    let completion = run(command().args([
        "complete-schedule",
        fixture.to_str().unwrap(),
        "--journal",
        journal_path.to_str().unwrap(),
        "--schedule-id",
        "schedule.reconcile",
        "--entry-id",
        "journal.schedule.reconcile.20260810",
        "--actor",
        "operator.alice",
        "--occurred-at",
        "2026-08-10T21:20:00Z",
    ]));
    assert!(completion.contains("operations.schedule_completed.v2"));

    let schedule_path = workspace.join("schedule.json");
    run(command().args([
        "schedule",
        fixture.to_str().unwrap(),
        schedule_path.to_str().unwrap(),
        "--as-of",
        "2026-08-10T21:30:00Z",
        "--journal",
        journal_path.to_str().unwrap(),
    ]));
    let schedule: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&schedule_path).unwrap()).unwrap();
    let schedules = schedule["schedules"].as_array().unwrap();
    let reconcile = schedules
        .iter()
        .find(|value| value["schedule_id"] == "schedule.reconcile")
        .unwrap();
    assert_eq!(reconcile["due"], false);
    assert_eq!(reconcile["last_completed_at"], "2026-08-10T21:20:00Z");
    let report = schedules
        .iter()
        .find(|value| value["schedule_id"] == "schedule.daily_report")
        .unwrap();
    assert_eq!(report["due"], true);

    fs::remove_dir_all(&workspace).unwrap();
}
