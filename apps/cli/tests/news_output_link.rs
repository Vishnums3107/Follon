//! `follon-news replay --output` refuses a symbolic link (delivery state E7.1).
//!
//! It opened its output with `File::create`, which follows a link and
//! truncates whatever the link points at.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn headlines() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/news/2026-09-01-headlines.ndjson")
}

/// Links `link` to `target`, which need not exist. Returns false where this
/// account cannot create a symbolic link, such as Windows without Developer
/// Mode, after saying so.
fn symlink_to(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(target, link);
    if let Err(error) = &linked {
        eprintln!("cannot create a symbolic link ({error}); the refusal was not exercised");
    }
    linked.is_ok()
}

fn replay_to(output: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_follon-news"))
        .arg("replay")
        .arg(headlines())
        .arg("--output")
        .arg(output)
        .output()
        .expect("follon-news runs")
}

#[test]
fn news_replay_output_refuses_a_symbolic_link_and_leaves_its_target_intact() {
    let workspace =
        std::env::temp_dir().join(format!("follon-news-output-link-{}", std::process::id()));
    let _ = fs::remove_dir_all(&workspace);
    fs::create_dir_all(&workspace).unwrap();
    let target = workspace.join("elsewhere.ndjson");
    let link = workspace.join("sentiment.ndjson");
    if !symlink_to(&target, &link) {
        fs::remove_dir_all(&workspace).unwrap();
        return;
    }

    // Dangling: nothing is created at the target.
    let output = replay_to(&link);
    assert!(!output.status.success(), "a dangling link was accepted");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("symbolic link"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!target.exists(), "the output was written through the link");

    // A link to an existing file: it is neither truncated nor overwritten.
    fs::write(&target, "operator data\n").unwrap();
    let output = replay_to(&link);
    assert!(!output.status.success(), "a link to a file was accepted");
    assert_eq!(fs::read_to_string(&target).unwrap(), "operator data\n");

    // A plain output path still works.
    let plain = workspace.join("plain.ndjson");
    let output = replay_to(&plain);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!fs::read_to_string(&plain).unwrap().is_empty());
    fs::remove_dir_all(&workspace).unwrap();
}
