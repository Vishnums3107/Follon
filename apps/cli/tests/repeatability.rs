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
        "artifact.manifest.json",
        "experiments.ndjson",
    ] {
        assert_identical(&first.join(name), &second.join(name));
    }
    // The conservative fully-paid profile travels inside the main artifact.
    let default_artifact: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(first.join("artifact.json")).expect("artifact must exist"),
    )
    .expect("artifact must be JSON");
    assert_eq!(default_artifact["artifact_schema_version"], 3);
    assert_eq!(
        default_artifact["advanced_account"]["margin"]["initial_margin"],
        "100.00000000"
    );
    for retired in [
        "artifact.advanced-account.json",
        "artifact.advanced-report.md",
    ] {
        assert!(
            !first.join(retired).exists(),
            "{retired} is no longer published"
        );
    }
    let default_manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(first.join("artifact.manifest.json"))
            .expect("default completion manifest must exist"),
    )
    .expect("default completion manifest must be JSON");
    assert_eq!(default_manifest["manifest_schema_version"], 3);
    assert!(default_manifest.get("advanced_account").is_none());

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
fn advanced_account_configuration_publishes_margin_aware_economics_in_the_artifact() {
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

    for suffix in ["json", "report.md", "manifest.json"] {
        assert_identical(
            &first.with_extension(suffix),
            &second.with_extension(suffix),
        );
    }
    let artifact_text = fs::read_to_string(&first).unwrap();
    let artifact: serde_json::Value = serde_json::from_str(&artifact_text).unwrap();
    assert_eq!(artifact["artifact_schema_version"], 3);
    let advanced = &artifact["advanced_account"];
    assert_eq!(advanced["advanced_report_schema_version"], 1);
    assert_eq!(advanced["margin"]["initial_margin"], "50.00000000");
    // The operator-facing report carries the same economics.
    let report = fs::read_to_string(first.with_extension("report.md")).unwrap();
    assert!(report.contains("## Advanced account"));
    assert!(report.contains("| Initial margin | 50.00000000 |"));
    // The manifest binds the economics through the artifact hash alone.
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(first.with_extension("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["manifest_schema_version"], 3);
    assert_eq!(
        manifest["artifact_sha256"],
        follon_cli::sha256_text(&artifact_text)
    );
    assert_eq!(
        manifest["artifact_fingerprint"],
        artifact["artifact_fingerprint"]
    );

    fs::remove_dir_all(workspace).expect("scoped advanced directory is removable");
}
