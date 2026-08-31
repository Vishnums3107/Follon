//! Operator-boundary acceptance test for byte-identical research outputs.

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

fn run(command: &mut Command) {
    let output = command.output().expect("operator command must start");
    assert!(
        output.status.success(),
        "operator command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_identical(left: &Path, right: &Path) {
    assert_eq!(
        fs::read(left).expect("left output must exist"),
        fs::read(right).expect("right output must exist"),
        "outputs differ: {} and {}",
        left.display(),
        right.display()
    );
}

#[test]
fn complete_cli_pipeline_is_byte_reproducible_and_idempotent() {
    let repository = repository_root();
    let acceptance_root =
        std::env::temp_dir().join(format!("follon-cli-repeatability-{}", std::process::id()));
    assert!(acceptance_root.starts_with(std::env::temp_dir()));
    if acceptance_root.exists() {
        fs::remove_dir_all(&acceptance_root)
            .expect("stale scoped acceptance directory is removable");
    }
    let first = acceptance_root.join("first");
    let second = acceptance_root.join("second");
    let trades = repository.join("tests/fixtures/historical-bars/spy-trades-v1.csv");
    let source_bars = repository.join("tests/fixtures/historical-bars/spy-one-minute.csv");
    let actions = repository.join("tests/fixtures/historical-bars/spy-corporate-actions.csv");
    let configuration = repository.join("tests/fixtures/config/backtest-v1.json");

    for output_root in [&first, &second] {
        run(Command::new(env!("CARGO_BIN_EXE_follon-build-bars"))
            .current_dir(&repository)
            .arg(&trades)
            .arg(output_root.join("bars.csv")));
        run(Command::new(env!("CARGO_BIN_EXE_follon-backtest"))
            .current_dir(&repository)
            .arg(&source_bars)
            .arg(output_root.join("artifact.json"))
            .arg("--config")
            .arg(&configuration)
            .arg("--actions")
            .arg(&actions)
            .arg("--experiment")
            .arg(output_root.join("experiments.ndjson"))
            .arg("experiment.acceptance")
            .arg("run.001"));
    }

    for name in [
        "bars.csv",
        "artifact.json",
        "artifact.events.ndjson",
        "artifact.report.md",
        "artifact.advanced-account.json",
        "artifact.advanced-report.md",
        "artifact.manifest.json",
        "experiments.ndjson",
    ] {
        assert_identical(&first.join(name), &second.join(name));
    }
    let default_advanced: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(first.join("artifact.advanced-account.json"))
            .expect("default advanced account artifact must exist"),
    )
    .expect("default advanced account artifact must be JSON");
    assert_eq!(default_advanced["margin"]["initial_margin"], "100.00000000");
    let default_manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(first.join("artifact.manifest.json"))
            .expect("default completion manifest must exist"),
    )
    .expect("default completion manifest must be JSON");
    assert!(default_manifest["advanced_account"]["artifact_sha256"]
        .as_str()
        .is_some_and(|hash| hash.len() == 64));

    // Publishing an already-completed immutable run is an idempotent operation.
    run(Command::new(env!("CARGO_BIN_EXE_follon-backtest"))
        .current_dir(&repository)
        .arg(&source_bars)
        .arg(first.join("artifact.json"))
        .arg("--config")
        .arg(&configuration)
        .arg("--actions")
        .arg(&actions)
        .arg("--experiment")
        .arg(first.join("experiments.ndjson"))
        .arg("experiment.acceptance")
        .arg("run.001"));

    fs::remove_dir_all(acceptance_root).expect("scoped acceptance directory is removable");
}

#[test]
fn advanced_account_configuration_publishes_hashed_margin_aware_sidecars() {
    let repository = repository_root();
    let workspace =
        std::env::temp_dir().join(format!("follon-advanced-backtest-{}", std::process::id()));
    if workspace.exists() {
        fs::remove_dir_all(&workspace).expect("stale scoped advanced directory is removable");
    }
    let bars = repository.join("tests/fixtures/historical-bars/spy-one-minute.csv");
    let configuration = repository.join("tests/fixtures/config/backtest-advanced-v1.json");
    let first = workspace.join("first").join("artifact.json");
    let second = workspace.join("second").join("artifact.json");

    for artifact in [&first, &second] {
        run(Command::new(env!("CARGO_BIN_EXE_follon-backtest"))
            .current_dir(&repository)
            .arg(&bars)
            .arg(artifact)
            .arg("--config")
            .arg(&configuration));
    }

    for suffix in [
        "advanced-account.json",
        "advanced-report.md",
        "manifest.json",
    ] {
        let first_sidecar = first.with_extension(suffix);
        let second_sidecar = second.with_extension(suffix);
        assert_identical(&first_sidecar, &second_sidecar);
    }
    let advanced: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(first.with_extension("advanced-account.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(advanced["advanced_report_schema_version"], 1);
    assert_eq!(advanced["margin"]["initial_margin"], "50.00000000");
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(first.with_extension("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["manifest_schema_version"], 2);
    assert!(manifest["advanced_account"]["artifact_sha256"]
        .as_str()
        .is_some_and(|hash| hash.len() == 64));

    fs::remove_dir_all(workspace).expect("scoped advanced directory is removable");
}
