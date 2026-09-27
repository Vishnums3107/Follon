//! End-to-end validation for `follon-repair-quotes` over the checked-in quote
//! fixtures: two instruments, three gaps, and a recovery batch that fills two
//! of them and corroborates one recorded quote.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/market-data")
        .join(name)
}

fn workspace(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("follon-quote-repair-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn repair(recorded: &Path, recovery: &Path, output: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_follon-repair-quotes"))
        .args(["--recorded", recorded.to_str().unwrap()])
        .args(["--recovery", recovery.to_str().unwrap()])
        .args(["--output-dir", output.to_str().unwrap()])
        .args(extra)
        .output()
        .expect("repair command should start")
}

fn describe(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn range(instrument: &str, from: u64, to: u64) -> serde_json::Value {
    serde_json::json!({"from": from, "instrument_id": instrument, "to": to})
}

#[test]
fn repair_writes_the_repaired_stream_and_a_hash_bound_record() {
    let root = workspace("record");
    let output = root.join("out");
    let result = repair(
        &fixture("quotes-recorded-v1.csv"),
        &fixture("quotes-recovery-v1.csv"),
        &output,
        &[],
    );
    assert!(result.status.success(), "{}", describe(&result));

    let repaired = fs::read_to_string(output.join("repaired-quotes.csv")).unwrap();
    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(output.join("gap-repair.json")).unwrap()).unwrap();
    let spy = "inst.us_equity.spy";
    let qqq = "inst.us_equity.qqq";
    assert_eq!(record["gap_repair_schema_version"], 1);
    assert_eq!(
        record["gaps"],
        serde_json::json!([range(qqq, 2, 2), range(spy, 4, 5), range(spy, 8, 8)])
    );
    assert_eq!(
        record["recovered"],
        serde_json::json!([range(qqq, 2, 2), range(spy, 4, 5)])
    );
    assert_eq!(record["residual"], serde_json::json!([range(spy, 8, 8)]));
    assert_eq!(record["complete"], false);
    assert_eq!(record["corroborated"], 1);
    assert_eq!(record["recorded_quotes"], 10);
    assert_eq!(record["recovery_quotes"], 4);
    assert_eq!(record["repaired_quotes"], 13);
    assert_eq!(
        record["repaired_sha256"],
        follon_cli::sha256_text(&repaired)
    );
    // Instrument, then sequence; the unfilled sequence 8 is absent, not invented.
    let rows: Vec<&str> = repaired.lines().skip(1).collect();
    assert_eq!(rows.len(), 13);
    assert!(rows[0].contains(",quote.qqq.1,1,"));
    assert!(rows.iter().any(|row| row.contains(",quote.spy.4,4,")));
    assert!(!rows.iter().any(|row| row.contains(",8,")));

    // A second identical run is idempotent against the immutable artifacts.
    let again = repair(
        &fixture("quotes-recorded-v1.csv"),
        &fixture("quotes-recovery-v1.csv"),
        &output,
        &[],
    );
    assert!(again.status.success(), "{}", describe(&again));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn require_complete_fails_on_a_residual_gap_and_passes_once_it_is_filled() {
    let root = workspace("complete");
    let partial = root.join("partial");
    let result = repair(
        &fixture("quotes-recorded-v1.csv"),
        &fixture("quotes-recovery-v1.csv"),
        &partial,
        &["--require-complete"],
    );
    assert!(!result.status.success(), "{}", describe(&result));
    assert!(String::from_utf8_lossy(&result.stderr).contains("1 residual gap(s) remain"));
    // The evidence of what remains is still written.
    assert!(partial.join("gap-repair.json").exists());

    let full_recovery = root.join("recovery-full.csv");
    let mut batch = fs::read_to_string(fixture("quotes-recovery-v1.csv")).unwrap();
    batch.push_str("2026-01-02T14:30:08Z,2026-01-02T14:30:08Z,inst.us_equity.spy,quote.spy.8,8,470.16,100,470.18,200\n");
    fs::write(&full_recovery, batch).unwrap();
    let complete = root.join("complete");
    let result = repair(
        &fixture("quotes-recorded-v1.csv"),
        &full_recovery,
        &complete,
        &["--require-complete"],
    );
    assert!(result.status.success(), "{}", describe(&result));
    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(complete.join("gap-repair.json")).unwrap())
            .unwrap();
    assert_eq!(record["complete"], true);
    assert_eq!(record["residual"], serde_json::json!([]));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_recovery_batch_that_contradicts_the_recording_writes_nothing() {
    let root = workspace("conflict");
    let hostile = root.join("recovery-conflict.csv");
    // The corroborating copy of spy sequence 3 now carries a different bid.
    let batch = fs::read_to_string(fixture("quotes-recovery-v1.csv"))
        .unwrap()
        .replace(",quote.spy.3,3,470.11,", ",quote.spy.3,3,470.09,");
    fs::write(&hostile, batch).unwrap();
    let output = root.join("out");
    let result = repair(&fixture("quotes-recorded-v1.csv"), &hostile, &output, &[]);
    assert!(!result.status.success(), "{}", describe(&result));
    assert!(String::from_utf8_lossy(&result.stderr).contains("conflicts with the recorded quote"));
    assert!(!output.exists(), "a refused repair must write nothing");
    fs::remove_dir_all(&root).unwrap();
}
