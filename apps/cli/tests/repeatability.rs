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
