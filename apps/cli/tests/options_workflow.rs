//! End-to-end validation for immutable options analytics and reconciliation evidence.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_follon-options"))
}

fn run(command: &mut Command) -> String {
    let output = command.output().expect("options command should start");
    assert!(
        output.status.success(),
        "options command failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8(output.stdout).expect("command output must be UTF-8")
}

#[test]
fn independently_fingerprinted_option_books_publish_repeatable_reconciliation_evidence() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/config/options-v1.json");
    let workspace =
        std::env::temp_dir().join(format!("follon-options-workflow-{}", std::process::id()));
    let _ = fs::remove_dir_all(&workspace);
    fs::create_dir_all(&workspace).unwrap();

    let dashboard = workspace.join("options-dashboard.json");
    for _ in 0..2 {
        run(command().args([
            "analyze",
            fixture.to_str().unwrap(),
            dashboard.to_str().unwrap(),
        ]));
    }
    let evidence: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&dashboard).unwrap()).unwrap();
    assert_ne!(
        evidence["configuration_file_hash"],
        evidence["run_identity"]["configuration_hash"]
    );
    assert_eq!(
        evidence["run_identity"]["configuration_hash"],
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    );
    let reconciliation = &evidence["reconciliation"];
    assert_eq!(reconciliation["clean"], true);
    let hashes = [
        reconciliation["backtest_book"]["book_hash"]
            .as_str()
            .unwrap(),
        reconciliation["paper_book"]["book_hash"].as_str().unwrap(),
        reconciliation["live_book"]["book_hash"].as_str().unwrap(),
    ];
    assert!(hashes.iter().all(|hash| hash.len() == 64));
    assert_ne!(hashes[0], hashes[1]);
    assert_ne!(hashes[1], hashes[2]);
    assert_eq!(
        reconciliation["paper_book"]["source_export_id"],
        "export.options.paper.001"
    );
    assert_eq!(reconciliation["live_book"]["environment"], "LIVE");
    assert_eq!(
        reconciliation["paper_book"]["run_identity_hash"],
        reconciliation["backtest_book"]["run_identity_hash"]
    );
    assert_eq!(
        reconciliation["live_book"]["run_identity"]["configuration_hash"],
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    );

    let report = workspace.join("options-report.md");
    run(command().args([
        "report",
        fixture.to_str().unwrap(),
        report.to_str().unwrap(),
    ]));
    let report_text = fs::read_to_string(&report).unwrap();
    assert!(report_text.contains("BACKTEST book hash"));
    assert!(report_text.contains("PAPER book hash"));
    assert!(report_text.contains("LIVE book hash"));

    fs::remove_dir_all(&workspace).unwrap();
}
