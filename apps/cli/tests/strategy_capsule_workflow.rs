//! Operator-boundary acceptance tests for portable strategy capsules.
//!
//! Each test evaluates a real Python strategy through `follon-backtest
//! --python-worker`, then drives `capsule-package` and `capsule-verify` as an
//! operator would. They need a Python interpreter and are skipped, with a
//! message, only when none is on `PATH`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CLI crate must be inside the workspace")
        .to_path_buf()
}

fn python_executable() -> Option<PathBuf> {
    ["python", "python3"].into_iter().find_map(|candidate| {
        Command::new(candidate)
            .args([
                "-c",
                "import pathlib,sys;print(pathlib.Path(sys.executable).resolve())",
            ])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|output| PathBuf::from(output.trim()))
            .filter(|path| path.is_absolute() && path.is_file())
    })
}

fn sdk_source() -> PathBuf {
    repository_root().join("python/strategy-sdk/src")
}

fn bars() -> PathBuf {
    repository_root().join("tests/fixtures/historical-bars/spy-one-minute.csv")
}

fn configuration() -> PathBuf {
    repository_root().join("tests/fixtures/config/backtest-v1.json")
}

struct Workspace(PathBuf);

impl Workspace {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("follon-capsule-{name}-{}", std::process::id()));
        assert!(path.starts_with(std::env::temp_dir()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("scoped workspace is creatable");
        Self(path)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn succeeded(output: &Output) -> bool {
    output.status.success()
}

fn describe(output: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A strategy bundle holding the repository's worker example, optionally
/// preceded by one extra source line.
fn write_bundle(root: &Path, prefix: &str) -> PathBuf {
    fs::create_dir_all(root).expect("bundle root is creatable");
    let source =
        fs::read_to_string(repository_root().join("python/examples/worker_buy_once_strategy.py"))
            .expect("worker example is readable");
    let strategy_file = root.join("worker_buy_once_strategy.py");
    fs::write(&strategy_file, format!("{prefix}{source}")).expect("strategy is writable");
    strategy_file
}

/// Runs the SDK's lock command and returns the bundle hash it printed.
fn lock(python: &Path, bundle: &Path, strategy_file: &Path, output: &Path) -> String {
    let result = Command::new(python)
        .args(["-m", "follon_strategy_sdk.bundle_lock", "--bundle-root"])
        .arg(bundle)
        .arg("--strategy-file")
        .arg(strategy_file)
        .args(["--class-name", "WorkerBuyOnceStrategy", "--output"])
        .arg(output)
        .env_clear()
        .env("PYTHONPATH", sdk_source())
        .envs(std::env::var_os("SYSTEMROOT").map(|root| ("SYSTEMROOT", root)))
        .output()
        .expect("lock command starts");
    assert!(succeeded(&result), "lock failed\n{}", describe(&result));
    String::from_utf8(result.stdout)
        .expect("bundle hash is UTF-8")
        .trim()
        .to_owned()
}

fn evaluate(python: &Path, bundle: &Path, strategy_file: &Path, hash: &str, artifact: &Path) {
    evaluate_with(python, bundle, strategy_file, hash, artifact, None);
}

/// As `evaluate`, applying the corporate actions in `actions` when they are given.
fn evaluate_with(
    python: &Path,
    bundle: &Path,
    strategy_file: &Path,
    hash: &str,
    artifact: &Path,
    actions: Option<&Path>,
) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_follon-backtest"));
    command
        .current_dir(repository_root())
        .arg(bars())
        .arg(artifact)
        .arg("--config")
        .arg(configuration());
    if let Some(actions) = actions {
        command.arg("--actions").arg(actions);
    }
    let result = command
        .arg("--python-worker")
        .arg(python)
        .arg(strategy_file)
        .arg("WorkerBuyOnceStrategy")
        .arg(bundle)
        .args(["strategy-example-001", "strategy-example-v1", hash])
        .env("FOLLON_STRATEGY_SDK_PATH", sdk_source())
        .output()
        .expect("backtest starts");
    assert!(
        succeeded(&result),
        "evaluation failed\n{}",
        describe(&result)
    );
}

/// A directory whose `follon_strategy_sdk` refuses to import. A replay that
/// can see it -- as its working directory or through the caller's
/// `FOLLON_STRATEGY_SDK_PATH` -- fails instead of reproducing its receipt.
fn hostile_directory(root: &Path) -> PathBuf {
    let hostile = root.join("hostile");
    fs::create_dir_all(hostile.join("follon_strategy_sdk")).expect("hostile sdk is creatable");
    fs::write(
        hostile.join("follon_strategy_sdk/__init__.py"),
        "raise ImportError('the replay imported the caller environment')\n",
    )
    .expect("hostile sdk is writable");
    hostile
}

fn package(
    python: &Path,
    bundle: &Path,
    lock: &Path,
    artifact: &Path,
    output: &Path,
    hostile: &Path,
) -> Output {
    package_with(python, bundle, lock, artifact, output, hostile, None)
}

/// As `package`, replaying with the corporate actions in `actions` when they are given.
fn package_with(
    python: &Path,
    bundle: &Path,
    lock: &Path,
    artifact: &Path,
    output: &Path,
    hostile: &Path,
    actions: Option<&Path>,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_follon-backtest"));
    command
        .current_dir(hostile)
        .env("FOLLON_STRATEGY_SDK_PATH", hostile)
        .arg("capsule-package")
        .arg("--bundle-root")
        .arg(bundle)
        .arg("--sdk-root")
        .arg(sdk_source().join("follon_strategy_sdk"))
        .arg("--lock")
        .arg(lock)
        .arg("--config")
        .arg(configuration())
        .arg("--evaluation")
        .arg(artifact)
        .arg("--bars")
        .arg(bars());
    if let Some(actions) = actions {
        command.arg("--actions").arg(actions);
    }
    command
        .arg("--python")
        .arg(python)
        .args(["--packaged-at", "2026-09-07T12:00:00Z", "--output"])
        .arg(output)
        .output()
        .expect("capsule-package starts")
}

fn verify(
    python: &Path,
    capsule: &Path,
    bars: &Path,
    hostile: &Path,
    trusted_key: Option<&Path>,
) -> Output {
    verify_with(python, capsule, bars, hostile, trusted_key, None)
}

/// As `verify`, replaying with the corporate actions in `actions` when they are given.
fn verify_with(
    python: &Path,
    capsule: &Path,
    bars: &Path,
    hostile: &Path,
    trusted_key: Option<&Path>,
    actions: Option<&Path>,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_follon-backtest"));
    command
        .current_dir(hostile)
        .env("FOLLON_STRATEGY_SDK_PATH", hostile)
        .arg("capsule-verify")
        .arg(capsule)
        .arg("--bars")
        .arg(bars);
    if let Some(actions) = actions {
        command.arg("--actions").arg(actions);
    }
    command.arg("--python").arg(python);
    if let Some(trusted_key) = trusted_key {
        command.arg("--trusted-key").arg(trusted_key);
    }
    command.output().expect("capsule-verify starts")
}

/// Writes an Ed25519 keypair with `follon-admin release-keygen`.
fn keygen(directory: &Path, key_id: &str) -> (PathBuf, PathBuf) {
    fs::create_dir_all(directory).expect("key directory is creatable");
    let private_key = directory.join("capsule-signing.pk8");
    let trusted_key = directory.join("trusted-capsule-key.json");
    let result = Command::new(env!("CARGO_BIN_EXE_follon-admin"))
        .args(["release-keygen", "--key-id", key_id, "--private-key"])
        .arg(&private_key)
        .arg("--trusted-key")
        .arg(&trusted_key)
        .output()
        .expect("release-keygen starts");
    assert!(succeeded(&result), "keygen failed\n{}", describe(&result));
    (private_key, trusted_key)
}

fn sign(capsule: &Path, private_key: &Path, key_id: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_follon-backtest"))
        .arg("capsule-sign")
        .arg(capsule)
        .arg("--private-key")
        .arg(private_key)
        .args(["--key-id", key_id, "--signed-at", "2026-09-07T12:05:00Z"])
        .output()
        .expect("capsule-sign starts")
}

#[test]
fn a_capsule_seals_and_reproduces_its_evaluation_despite_a_hostile_environment() {
    let Some(python) = python_executable() else {
        eprintln!("Python is unavailable; the capsule workflow was skipped");
        return;
    };
    let workspace = Workspace::new("seal");
    let bundle = workspace.0.join("bundle");
    let strategy_file = write_bundle(&bundle, "");
    let lock_path = workspace.0.join("dependency.lock");
    let hash = lock(&python, &bundle, &strategy_file, &lock_path);
    let artifact = workspace.0.join("evaluation/python-backtest.json");
    evaluate(&python, &bundle, &strategy_file, &hash, &artifact);
    let hostile = hostile_directory(&workspace.0);
    let capsule = workspace.0.join("capsule");

    let packaged = package(&python, &bundle, &lock_path, &artifact, &capsule, &hostile);
    assert!(
        succeeded(&packaged),
        "packaging failed\n{}",
        describe(&packaged)
    );

    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(capsule.join("capsule-manifest.json")).expect("manifest exists"),
    )
    .expect("manifest is JSON");
    assert_eq!(manifest["export_disposition"], "VERIFIED_PORTABLE");
    assert_eq!(manifest["bundle_sha256"], hash.as_str());
    assert_eq!(manifest["strategy_id"], "strategy-example-001");
    let evaluation: serde_json::Value =
        serde_json::from_slice(&fs::read(&artifact).expect("artifact exists"))
            .expect("artifact is JSON");
    assert_eq!(
        evaluation["specification"]["strategy_bundle_hash"],
        hash.as_str(),
        "the evaluation must have recorded the hash the capsule carries"
    );
    assert_eq!(
        fs::read(capsule.join("evaluation-receipt.json")).expect("receipt exists"),
        fs::read(artifact.with_extension("manifest.json")).expect("completion manifest exists"),
    );
    assert_eq!(
        fs::read(capsule.join("dependency.lock")).expect("capsule lock exists"),
        fs::read(&lock_path).expect("lock exists"),
    );
    assert_eq!(
        fs::read(capsule.join("configuration.json")).expect("capsule configuration exists"),
        fs::read(configuration()).expect("configuration exists"),
    );

    let verified = verify(&python, &capsule, &bars(), &hostile, None);
    assert!(
        succeeded(&verified),
        "verification failed\n{}",
        describe(&verified)
    );
    assert!(String::from_utf8_lossy(&verified.stdout).contains("VERIFIED_PORTABLE"));

    // Different bars pass every static check but cannot reproduce the receipt.
    let shifted_bars = workspace.0.join("shifted.csv");
    fs::write(
        &shifted_bars,
        fs::read_to_string(bars())
            .expect("bars are readable")
            .replacen(
                "2026-01-02T14:32:00Z,inst.us_equity.spy,100.00000000,101.00000000,99.00000000,100.00000000",
                "2026-01-02T14:32:00Z,inst.us_equity.spy,100.00000000,101.00000000,99.00000000,100.50000000",
                1,
            ),
    )
    .expect("shifted bars are writable");
    let diverged = verify(&python, &capsule, &shifted_bars, &hostile, None);
    assert!(
        !succeeded(&diverged),
        "a replay over different bars was accepted\n{}",
        describe(&diverged)
    );
}

#[test]
fn a_capsule_refuses_an_evaluation_of_different_source() {
    let Some(python) = python_executable() else {
        eprintln!("Python is unavailable; the capsule workflow was skipped");
        return;
    };
    let workspace = Workspace::new("source");
    let bundle = workspace.0.join("bundle");
    let strategy_file = write_bundle(&bundle, "");
    let evaluated_lock = workspace.0.join("evaluated.lock");
    let hash = lock(&python, &bundle, &strategy_file, &evaluated_lock);
    let artifact = workspace.0.join("evaluation/python-backtest.json");
    evaluate(&python, &bundle, &strategy_file, &hash, &artifact);
    let hostile = hostile_directory(&workspace.0);

    // The source changes after the evaluation ran.
    write_bundle(&bundle, "# edited after evaluation\n");
    let stale_lock = package(
        &python,
        &bundle,
        &evaluated_lock,
        &artifact,
        &workspace.0.join("stale-lock"),
        &hostile,
    );
    assert!(
        !succeeded(&stale_lock)
            && String::from_utf8_lossy(&stale_lock.stderr)
                .contains("does not hash to the locked bundle hash"),
        "a bundle that no longer matches its lock was not refused by the lock check\n{}",
        describe(&stale_lock)
    );
    let relocked = workspace.0.join("relocked.lock");
    lock(&python, &bundle, &strategy_file, &relocked);
    let fresh_lock = package(
        &python,
        &bundle,
        &relocked,
        &artifact,
        &workspace.0.join("fresh-lock"),
        &hostile,
    );
    // The replay would also diverge, but the binding check must refuse first.
    assert!(
        !succeeded(&fresh_lock)
            && String::from_utf8_lossy(&fresh_lock.stderr)
                .contains("the evaluation was not run with this strategy bundle"),
        "an evaluation of other source was not refused by the bundle binding\n{}",
        describe(&fresh_lock)
    );
    assert!(!workspace.0.join("stale-lock").exists());
    assert!(!workspace.0.join("fresh-lock").exists());
}

#[test]
fn a_capsule_refuses_a_strategy_that_needs_an_installed_package() {
    let Some(python) = python_executable() else {
        eprintln!("Python is unavailable; the capsule workflow was skipped");
        return;
    };
    let has_pip = Command::new(&python)
        .args(["-c", "import pip"])
        .output()
        .is_ok_and(|output| output.status.success());
    if !has_pip {
        eprintln!("pip is not installed; the un-vendored dependency refusal was skipped");
        return;
    }
    let workspace = Workspace::new("site");
    let bundle = workspace.0.join("bundle");
    // `pip` lives in site-packages, not in the bundle and not in the stdlib.
    let strategy_file = write_bundle(&bundle, "import pip  # not vendored\n");
    let lock_path = workspace.0.join("dependency.lock");
    let hash = lock(&python, &bundle, &strategy_file, &lock_path);
    let artifact = workspace.0.join("evaluation/python-backtest.json");
    // An ordinary evaluation can see site-packages, so it succeeds.
    evaluate(&python, &bundle, &strategy_file, &hash, &artifact);
    let hostile = hostile_directory(&workspace.0);
    let capsule = workspace.0.join("capsule");

    let packaged = package(&python, &bundle, &lock_path, &artifact, &capsule, &hostile);
    assert!(
        !succeeded(&packaged)
            && String::from_utf8_lossy(&packaged.stderr).contains("No module named 'pip'"),
        "a capsule depending on an installed package was not refused by its replay\n{}",
        describe(&packaged)
    );
    assert!(!capsule.exists());
}

#[test]
fn a_signed_capsule_verifies_only_under_its_trusted_key() {
    let Some(python) = python_executable() else {
        eprintln!("Python is unavailable; the capsule workflow was skipped");
        return;
    };
    let workspace = Workspace::new("signed");
    let bundle = workspace.0.join("bundle");
    let strategy_file = write_bundle(&bundle, "");
    let lock_path = workspace.0.join("dependency.lock");
    let hash = lock(&python, &bundle, &strategy_file, &lock_path);
    let artifact = workspace.0.join("evaluation/python-backtest.json");
    evaluate(&python, &bundle, &strategy_file, &hash, &artifact);
    let hostile = hostile_directory(&workspace.0);
    let capsule = workspace.0.join("capsule");
    let packaged = package(&python, &bundle, &lock_path, &artifact, &capsule, &hostile);
    assert!(
        succeeded(&packaged),
        "packaging failed\n{}",
        describe(&packaged)
    );

    let (private_key, trusted_key) = keygen(&workspace.0.join("key"), "capsule.key.author");
    // Same identity, different key material: only the cryptography can tell.
    let (_, impostor_key) = keygen(&workspace.0.join("impostor"), "capsule.key.author");

    let unsigned = verify(&python, &capsule, &bars(), &hostile, Some(&trusted_key));
    assert!(
        !succeeded(&unsigned) && String::from_utf8_lossy(&unsigned.stderr).contains("not signed"),
        "an unsigned capsule passed a trusted-key verification\n{}",
        describe(&unsigned)
    );

    let signed = sign(&capsule, &private_key, "capsule.key.author");
    assert!(succeeded(&signed), "signing failed\n{}", describe(&signed));
    assert!(capsule.join("capsule-signature.json").is_file());
    let resigned = sign(&capsule, &private_key, "capsule.key.author");
    assert!(!succeeded(&resigned), "a capsule was signed twice");

    let trusted = verify(&python, &capsule, &bars(), &hostile, Some(&trusted_key));
    assert!(
        succeeded(&trusted),
        "trusted verification failed\n{}",
        describe(&trusted)
    );
    assert!(String::from_utf8_lossy(&trusted.stdout)
        .contains("signed by trusted key capsule.key.author"));

    let unchecked = verify(&python, &capsule, &bars(), &hostile, None);
    assert!(
        succeeded(&unchecked),
        "verification failed\n{}",
        describe(&unchecked)
    );
    assert!(String::from_utf8_lossy(&unchecked.stdout).contains("not checked"));

    let impostor = verify(&python, &capsule, &bars(), &hostile, Some(&impostor_key));
    assert!(
        !succeeded(&impostor)
            && String::from_utf8_lossy(&impostor.stderr)
                .contains("capsule signature verification failed"),
        "a capsule verified under another key with the same identity\n{}",
        describe(&impostor)
    );
}

/// A corporate-action file with one split, dated at the first bar so that no order rests
/// across it. The dataset content hash covers it, and the replay applies it.
fn write_actions(path: &Path, ratio: &str) {
    write_actions_at(path, "2026-01-02T14:31:00Z", ratio);
}

fn write_actions_at(path: &Path, effective_at: &str, ratio: &str) {
    fs::write(
        path,
        format!(
            "action_id,instrument_id,action_type,effective_at,value\naction-split-001,inst.us_equity.spy,SPLIT,{effective_at},{ratio}\n"
        ),
    )
    .expect("actions are writable");
}

#[test]
fn a_capsule_of_an_evaluation_with_corporate_actions_reproduces_only_with_them() {
    let Some(python) = python_executable() else {
        eprintln!("Python is unavailable; the capsule workflow was skipped");
        return;
    };
    let workspace = Workspace::new("actions");
    let bundle = workspace.0.join("bundle");
    let strategy_file = write_bundle(&bundle, "");
    let lock_path = workspace.0.join("dependency.lock");
    let hash = lock(&python, &bundle, &strategy_file, &lock_path);
    let actions = workspace.0.join("actions.csv");
    write_actions(&actions, "2.00000000");
    let artifact = workspace.0.join("evaluation/python-backtest.json");
    evaluate_with(
        &python,
        &bundle,
        &strategy_file,
        &hash,
        &artifact,
        Some(&actions),
    );
    let evaluation: serde_json::Value =
        serde_json::from_slice(&fs::read(&artifact).expect("artifact exists"))
            .expect("artifact is JSON");
    assert_eq!(
        evaluation["performance"]["corporate_action_count"], 1,
        "the split must have been applied for this test to mean anything"
    );
    let hostile = hostile_directory(&workspace.0);

    // Without the actions the evaluation cannot be replayed, and packaging says why.
    let without = package(
        &python,
        &bundle,
        &lock_path,
        &artifact,
        &workspace.0.join("without"),
        &hostile,
    );
    assert!(
        !succeeded(&without)
            && String::from_utf8_lossy(&without.stderr)
                .contains("applied 1 corporate action(s), which the replay needs"),
        "a capsule of an evaluation with corporate actions was packaged without them\n{}",
        describe(&without)
    );
    assert!(!workspace.0.join("without").exists());

    let capsule = workspace.0.join("capsule");
    let packaged = package_with(
        &python,
        &bundle,
        &lock_path,
        &artifact,
        &capsule,
        &hostile,
        Some(&actions),
    );
    assert!(
        succeeded(&packaged),
        "packaging failed\n{}",
        describe(&packaged)
    );
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(capsule.join("capsule-manifest.json")).expect("manifest exists"),
    )
    .expect("manifest is JSON");
    assert_eq!(manifest["export_disposition"], "VERIFIED_PORTABLE");
    assert!(manifest["replay_instruction_command"]
        .as_str()
        .expect("replay command is text")
        .contains("--actions"));

    let verified = verify_with(&python, &capsule, &bars(), &hostile, None, Some(&actions));
    assert!(
        succeeded(&verified),
        "verification failed\n{}",
        describe(&verified)
    );

    // The bars alone pass every static check but cannot reproduce the receipt, and the
    // refusal points at the missing input.
    let bare = verify(&python, &capsule, &bars(), &hostile, None);
    assert!(
        !succeeded(&bare)
            && String::from_utf8_lossy(&bare.stderr).contains("pass them with --actions"),
        "a replay without the corporate actions was accepted\n{}",
        describe(&bare)
    );

    // A different action changes the dataset the receipt is bound to.
    let other = workspace.0.join("other-actions.csv");
    write_actions(&other, "3.00000000");
    let different = verify_with(&python, &capsule, &bars(), &hostile, None, Some(&other));
    assert!(
        !succeeded(&different)
            && String::from_utf8_lossy(&different.stderr).contains("did not reproduce"),
        "a replay with different corporate actions was accepted\n{}",
        describe(&different)
    );
}

/// A file whose only action falls after the last bar applies nothing, yet the dataset's
/// content hash covers it. Packaging such an evaluation without the file passed the applied
/// count check and then failed to reproduce the receipt without naming the cause
/// (delivery state E8.6, found in review).
#[test]
fn packaging_an_evaluation_whose_actions_applied_nothing_still_points_at_the_actions() {
    let Some(python) = python_executable() else {
        eprintln!("Python is unavailable; the capsule workflow was skipped");
        return;
    };
    let workspace = Workspace::new("late-actions");
    let bundle = workspace.0.join("bundle");
    let strategy_file = write_bundle(&bundle, "");
    let lock_path = workspace.0.join("dependency.lock");
    let hash = lock(&python, &bundle, &strategy_file, &lock_path);
    let actions = workspace.0.join("actions.csv");
    write_actions_at(&actions, "2030-01-01T00:00:00Z", "2.00000000");
    let artifact = workspace.0.join("evaluation/python-backtest.json");
    evaluate_with(
        &python,
        &bundle,
        &strategy_file,
        &hash,
        &artifact,
        Some(&actions),
    );
    let evaluation: serde_json::Value =
        serde_json::from_slice(&fs::read(&artifact).expect("artifact exists"))
            .expect("artifact is JSON");
    assert_eq!(
        evaluation["performance"]["corporate_action_count"], 0,
        "the split must fall outside the bars for this test to mean anything"
    );
    let hostile = hostile_directory(&workspace.0);

    let without = package(
        &python,
        &bundle,
        &lock_path,
        &artifact,
        &workspace.0.join("without"),
        &hostile,
    );
    let stderr = String::from_utf8_lossy(&without.stderr);
    assert!(
        !succeeded(&without)
            && stderr.contains("did not reproduce")
            && stderr.contains("package it with the same file"),
        "the refusal did not name the missing actions\n{}",
        describe(&without)
    );
    assert!(!workspace.0.join("without").exists());

    let packaged = package_with(
        &python,
        &bundle,
        &lock_path,
        &artifact,
        &workspace.0.join("with"),
        &hostile,
        Some(&actions),
    );
    assert!(
        succeeded(&packaged),
        "packaging with the actions failed\n{}",
        describe(&packaged)
    );
}
