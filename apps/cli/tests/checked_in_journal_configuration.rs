//! The checked-in PAPER and LIVE journals must open under their checked-in
//! configurations.
//!
//! `core/paper` and `core/live` each test that their fixture journal still
//! parses. A journal also records its configuration fingerprint, and a service
//! refuses to reopen it under a different one. So changing a fixture
//! configuration without regenerating its journal passed `cargo test` and
//! failed only the evidence pipeline, which is not one of the measured suites
//! (conformance-audit items 51 and 61). This drives the real status CLIs over
//! copies of the fixtures, exactly as pipeline steps 14a and 15 do.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CLI crate must be inside the workspace")
        .to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "follon-journal-configuration-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("scratch directory is creatable");
    path
}

fn assert_opens(command: &mut Command, fixture: &str, configuration: &str) {
    let output = command.output().expect("status CLI starts");
    assert!(
        output.status.success(),
        "{fixture} does not open under {configuration}. Regenerate the journal with the current \
         status binary against that configuration, never by hand.\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_paper_journal_fixture_opens_under_its_configuration() {
    let root = repository_root();
    let workspace = scratch("paper");
    // Opened through a copy: a test must never be able to modify the fixture.
    let journal = workspace.join("journal-v2.ndjson");
    fs::copy(
        root.join("tests/fixtures/paper/journal-v2.ndjson"),
        &journal,
    )
    .unwrap();
    assert_opens(
        Command::new(env!("CARGO_BIN_EXE_follon-paper-status"))
            .arg(&journal)
            .arg(workspace.join("dashboard.json"))
            .arg("--config")
            .arg(root.join("tests/fixtures/config/paper-v2.json")),
        "tests/fixtures/paper/journal-v2.ndjson",
        "tests/fixtures/config/paper-v2.json",
    );
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn the_live_journal_fixture_opens_under_its_configuration() {
    let root = repository_root();
    let workspace = scratch("live");
    let journal = workspace.join("journal-v1.ndjson");
    fs::copy(root.join("tests/fixtures/live/journal-v1.ndjson"), &journal).unwrap();
    assert_opens(
        Command::new(env!("CARGO_BIN_EXE_follon-live-status"))
            .arg(&journal)
            .arg(workspace.join("dashboard.json"))
            .args(["--opened-at", "2026-08-11T13:30:00Z", "--config"])
            .arg(root.join("tests/fixtures/config/live-v1.json")),
        "tests/fixtures/live/journal-v1.ndjson",
        "tests/fixtures/config/live-v1.json",
    );
    fs::remove_dir_all(workspace).unwrap();
}
