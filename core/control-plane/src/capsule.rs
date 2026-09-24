//! Portable strategy capsules (DUR-07, ASSET-04).
//!
//! A capsule is a directory of five files that together let a Python strategy
//! evaluation be re-run and checked somewhere other than where it was made:
//!
//! | File | Contents |
//! | --- | --- |
//! | [`CAPSULE_BUNDLE_FILE`] | The strategy archive: the exact length-framed byte stream the SDK's `strategy_bundle_hash` is computed over, so its SHA-256 *is* the bundle hash a worker announces and a backtest records |
//! | [`CAPSULE_LOCK_FILE`] | The dependency lock the SDK wrote: runtime identity, entry point, and every source file's size and digest |
//! | [`CAPSULE_CONFIGURATION_FILE`] | The exact backtest configuration bytes the evaluation ran with |
//! | [`CAPSULE_RECEIPT_FILE`] | The evaluation's completion manifest, byte for byte |
//! | [`CAPSULE_MANIFEST_FILE`] | The `strategy-capsule-manifest` v1 document binding the four above |
//!
//! Nothing here takes a caller's word for a hash or a disposition. Every digest
//! in a manifest is computed from bytes, and [`CapsuleContents::seal`] issues
//! `VERIFIED_PORTABLE` only when handed replay output that reproduces the
//! receipt byte for byte. Running that replay needs the backtest runner and a
//! Python interpreter, so it lives in `follon-backtest capsule-package` and
//! `capsule-verify`, not in this crate.
//!
//! `VERIFIED_PORTABLE` means the capsule's own copies of the strategy and SDK,
//! extracted to a fresh directory and run by an interpreter with no `site`
//! packages and no inherited import path, reproduced the receipt on the
//! recorded runtime target. It is not a claim about any other runtime target,
//! and it says nothing about whether the strategy is any good. The dataset is
//! referenced by content hash, never carried, so the capsule redistributes no
//! market data.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use follon_domain::{validate_canonical_id, validate_utc_timestamp};
use sha2::{Digest, Sha256};

use crate::{is_sha256, require_exact_json_fields, EngineError};

/// Framing version shared with the SDK's `strategy_bundle_hash`.
pub const STRATEGY_BUNDLE_FORMAT: &str = "follon-strategy-bundle-v2";
/// Capsule member holding the sealed manifest.
pub const CAPSULE_MANIFEST_FILE: &str = "capsule-manifest.json";
/// Capsule member holding the strategy archive.
pub const CAPSULE_BUNDLE_FILE: &str = "strategy-bundle.bin";
/// Capsule member holding the dependency lock.
pub const CAPSULE_LOCK_FILE: &str = "dependency.lock";
/// Capsule member holding the evaluated backtest configuration.
pub const CAPSULE_CONFIGURATION_FILE: &str = "configuration.json";
/// Capsule member holding the evaluation's completion manifest.
pub const CAPSULE_RECEIPT_FILE: &str = "evaluation-receipt.json";
/// Directory name the SDK namespace is extracted under, so it is importable.
pub const SDK_PACKAGE_DIRECTORY: &str = "follon_strategy_sdk";

const CAPSULE_SCHEMA_VERSION: u64 = 1;
const LOCK_SCHEMA_VERSION: u64 = 1;
const NAMESPACES: [&str; 2] = ["strategy", "sdk"];
const MAX_BUNDLE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024;
const MANIFEST_FIELDS: [&str; 12] = [
    "bundle_sha256",
    "capsule_id",
    "capsule_schema_version",
    "configuration_sha256",
    "dependency_lockfile_sha256",
    "evaluation_receipt_id",
    "export_disposition",
    "packaged_at",
    "replay_instruction_command",
    "runtime_target",
    "strategy_id",
    "strategy_version",
];

/// Export disposition certifying portable execution safety.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapsuleExportDisposition {
    /// The capsule's own contents reproduced its evaluation receipt.
    VerifiedPortable,
    /// Missing cryptographic dependency lockfile.
    MissingDependencyLock,
    /// Strategy lacks an immutable, evaluated backtest receipt.
    UnverifiedEvaluation,
    /// Bound dataset has restricted redistribution rights.
    RestrictedDatasetRights,
}

impl CapsuleExportDisposition {
    /// Returns the canonical uppercase string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VerifiedPortable => "VERIFIED_PORTABLE",
            Self::MissingDependencyLock => "MISSING_DEPENDENCY_LOCK",
            Self::UnverifiedEvaluation => "UNVERIFIED_EVALUATION",
            Self::RestrictedDatasetRights => "RESTRICTED_DATASET_RIGHTS",
        }
    }

    fn parse(value: &str) -> Result<Self, EngineError> {
        [
            Self::VerifiedPortable,
            Self::MissingDependencyLock,
            Self::UnverifiedEvaluation,
            Self::RestrictedDatasetRights,
        ]
        .into_iter()
        .find(|disposition| disposition.as_str() == value)
        .ok_or_else(|| EngineError("unknown capsule export disposition".to_owned()))
    }
}

/// Where a worker starts inside a strategy bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundleEntryPoint {
    /// Strategy-namespace path of the source file defining the class.
    pub strategy_file: String,
    /// Strategy class the worker instantiates.
    pub class_name: String,
}

/// One locked source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockedFile {
    /// `/`-separated path relative to its namespace root.
    pub path: String,
    /// Exact size in bytes.
    pub bytes: u64,
    /// SHA-256 of the exact contents.
    pub sha256: String,
}

/// One namespace of a strategy bundle, in archive order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundleNamespace {
    /// `strategy` or `sdk`.
    pub name: String,
    /// Files in strictly ascending path order.
    pub files: Vec<LockedFile>,
}

/// The dependency lock the strategy SDK writes for a bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrategyBundleLock {
    /// `implementation|major.minor.micro|platform` of the interpreter that locked it.
    pub runtime: String,
    /// Worker entry point.
    pub entry_point: BundleEntryPoint,
    /// Exactly the `strategy` then `sdk` namespaces.
    pub namespaces: Vec<BundleNamespace>,
    /// The bundle hash the lock describes.
    pub strategy_bundle_hash: String,
}

impl StrategyBundleLock {
    /// Parses a lock, refusing anything but the SDK's canonical encoding.
    ///
    /// Canonical bytes are required so the lock's own SHA-256 is a stable
    /// identity: two locks describing the same bundle cannot differ.
    pub fn parse(bytes: &[u8]) -> Result<Self, EngineError> {
        let value = canonical_json_object(bytes, "dependency lock")?;
        let object = value
            .as_object()
            .ok_or_else(|| EngineError("dependency lock is not an object".to_owned()))?;
        require_exact_json_fields(
            object,
            &[
                "bundle_format",
                "entry_point",
                "lock_schema_version",
                "namespaces",
                "runtime",
                "strategy_bundle_hash",
            ],
            "dependency lock",
        )?;
        if object
            .get("lock_schema_version")
            .and_then(serde_json::Value::as_u64)
            != Some(LOCK_SCHEMA_VERSION)
        {
            return Err(EngineError(
                "unsupported dependency lock schema version".to_owned(),
            ));
        }
        if string_field(object, "bundle_format", "dependency lock")? != STRATEGY_BUNDLE_FORMAT {
            return Err(EngineError("unsupported strategy bundle format".to_owned()));
        }
        let runtime = string_field(object, "runtime", "dependency lock")?.to_owned();
        validate_runtime(&runtime)?;
        let strategy_bundle_hash =
            string_field(object, "strategy_bundle_hash", "dependency lock")?.to_owned();
        if !is_sha256(&strategy_bundle_hash) {
            return Err(EngineError(
                "dependency lock bundle hash is not a lowercase SHA-256".to_owned(),
            ));
        }

        let raw_namespaces = object
            .get("namespaces")
            .and_then(serde_json::Value::as_array)
            .filter(|namespaces| namespaces.len() == NAMESPACES.len())
            .ok_or_else(|| {
                EngineError("dependency lock must list the strategy and sdk namespaces".to_owned())
            })?;
        let mut namespaces = Vec::with_capacity(NAMESPACES.len());
        for (raw, expected_name) in raw_namespaces.iter().zip(NAMESPACES) {
            let namespace = raw.as_object().ok_or_else(|| {
                EngineError("dependency lock namespace is not an object".to_owned())
            })?;
            require_exact_json_fields(namespace, &["files", "name"], "dependency lock namespace")?;
            if string_field(namespace, "name", "dependency lock namespace")? != expected_name {
                return Err(EngineError(
                    "dependency lock namespaces must be strategy then sdk".to_owned(),
                ));
            }
            let raw_files = namespace
                .get("files")
                .and_then(serde_json::Value::as_array)
                .filter(|files| !files.is_empty())
                .ok_or_else(|| {
                    EngineError(format!("{expected_name} namespace locks no source files"))
                })?;
            let mut files: Vec<LockedFile> = Vec::with_capacity(raw_files.len());
            for raw_file in raw_files {
                let file = raw_file.as_object().ok_or_else(|| {
                    EngineError("dependency lock file entry is not an object".to_owned())
                })?;
                require_exact_json_fields(
                    file,
                    &["bytes", "path", "sha256"],
                    "dependency lock file entry",
                )?;
                let path = string_field(file, "path", "dependency lock file entry")?.to_owned();
                validate_bundle_path(&path)?;
                if files.last().is_some_and(|previous| previous.path >= path) {
                    return Err(EngineError(
                        "dependency lock files must be unique and in ascending path order"
                            .to_owned(),
                    ));
                }
                let bytes = file
                    .get("bytes")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| {
                        EngineError("dependency lock file size is not an integer".to_owned())
                    })?;
                let sha256 = string_field(file, "sha256", "dependency lock file entry")?.to_owned();
                if !is_sha256(&sha256) {
                    return Err(EngineError(
                        "dependency lock file digest is not a lowercase SHA-256".to_owned(),
                    ));
                }
                files.push(LockedFile {
                    path,
                    bytes,
                    sha256,
                });
            }
            namespaces.push(BundleNamespace {
                name: expected_name.to_owned(),
                files,
            });
        }

        let entry = object
            .get("entry_point")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| {
                EngineError("dependency lock entry point is not an object".to_owned())
            })?;
        require_exact_json_fields(
            entry,
            &["class_name", "strategy_file"],
            "dependency lock entry point",
        )?;
        let entry_point = BundleEntryPoint {
            strategy_file: string_field(entry, "strategy_file", "dependency lock entry point")?
                .to_owned(),
            class_name: string_field(entry, "class_name", "dependency lock entry point")?
                .to_owned(),
        };
        if !namespaces[0]
            .files
            .iter()
            .any(|file| file.path == entry_point.strategy_file)
        {
            return Err(EngineError(
                "entry point is not a file of the strategy namespace".to_owned(),
            ));
        }
        if !is_python_identifier(&entry_point.class_name) {
            return Err(EngineError(
                "entry point class is not an ASCII Python identifier".to_owned(),
            ));
        }
        Ok(Self {
            runtime,
            entry_point,
            namespaces,
            strategy_bundle_hash,
        })
    }
}

/// One source file read out of a verified strategy archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundleSource {
    /// `strategy` or `sdk`.
    pub namespace: &'static str,
    /// `/`-separated path relative to the namespace root.
    pub path: String,
    /// Exact file contents.
    pub contents: Vec<u8>,
}

/// Builds the strategy archive from a strategy tree and an SDK package tree.
///
/// Byte for byte this is the stream the SDK's `strategy_bundle_hash` feeds to
/// SHA-256: every `*.py` file under each root in ascending relative-path
/// order, length-framed, followed by the runtime identity. Any symlink in
/// either tree is refused.
pub fn build_strategy_bundle(
    strategy_root: &Path,
    sdk_root: &Path,
    runtime: &str,
) -> Result<Vec<u8>, EngineError> {
    validate_runtime(runtime)?;
    let mut archive = Vec::new();
    archive.extend_from_slice(STRATEGY_BUNDLE_FORMAT.as_bytes());
    archive.push(0);
    for (name, root) in NAMESPACES.into_iter().zip([strategy_root, sdk_root]) {
        let metadata = fs::symlink_metadata(root)?;
        if !metadata.is_dir() {
            return Err(EngineError(format!(
                "{name} bundle root must be a non-symlink directory"
            )));
        }
        let mut sources = Vec::new();
        collect_sources(root, "", &mut sources)?;
        if sources.is_empty() {
            return Err(EngineError(format!(
                "{name} bundle root contains no Python source files"
            )));
        }
        sources.sort();
        push_u32_framed(&mut archive, name.as_bytes())?;
        for (relative, path) in sources {
            let contents = fs::read(&path)?;
            push_u32_framed(&mut archive, relative.as_bytes())?;
            archive.extend_from_slice(&(contents.len() as u64).to_be_bytes());
            archive.extend_from_slice(&contents);
            if archive.len() as u64 > MAX_BUNDLE_BYTES {
                return Err(EngineError("strategy bundle exceeds 64 MiB".to_owned()));
            }
        }
    }
    push_u32_framed(&mut archive, runtime.as_bytes())?;
    Ok(archive)
}

/// Reads a strategy archive exactly as its lock describes it.
///
/// The framing has no file counts, so the lock drives the parse: every
/// namespace, path, size and digest must match in order, the runtime tail must
/// equal the lock's, nothing may follow it, and the whole archive must hash to
/// the lock's bundle hash.
pub fn open_strategy_bundle(
    archive: &[u8],
    lock: &StrategyBundleLock,
) -> Result<Vec<BundleSource>, EngineError> {
    if sha256_hex(archive) != lock.strategy_bundle_hash {
        return Err(EngineError(
            "strategy archive does not hash to the locked bundle hash".to_owned(),
        ));
    }
    let mut reader = ArchiveReader {
        bytes: archive,
        position: 0,
    };
    let mut header = STRATEGY_BUNDLE_FORMAT.as_bytes().to_vec();
    header.push(0);
    if reader.take(header.len())? != header.as_slice() {
        return Err(EngineError(
            "strategy archive has an unknown format header".to_owned(),
        ));
    }
    let mut sources = Vec::new();
    for (namespace, name) in lock.namespaces.iter().zip(NAMESPACES) {
        if reader.u32_framed()? != name.as_bytes() {
            return Err(EngineError(format!(
                "strategy archive does not open the {name} namespace where the lock says"
            )));
        }
        for file in &namespace.files {
            if reader.u32_framed()? != file.path.as_bytes() {
                return Err(EngineError(format!(
                    "strategy archive does not contain {name}/{} where the lock says",
                    file.path
                )));
            }
            let length = reader.u64()?;
            if length != file.bytes {
                return Err(EngineError(format!(
                    "{name}/{} has a different size from its lock entry",
                    file.path
                )));
            }
            let length = usize::try_from(length)
                .map_err(|_| EngineError("strategy archive entry is too large".to_owned()))?;
            let contents = reader.take(length)?;
            if sha256_hex(contents) != file.sha256 {
                return Err(EngineError(format!(
                    "{name}/{} does not match its locked digest",
                    file.path
                )));
            }
            sources.push(BundleSource {
                namespace: name,
                path: file.path.clone(),
                contents: contents.to_vec(),
            });
        }
    }
    if reader.u32_framed()? != lock.runtime.as_bytes() {
        return Err(EngineError(
            "strategy archive was built for a different runtime than its lock".to_owned(),
        ));
    }
    if reader.position != archive.len() {
        return Err(EngineError(
            "strategy archive has bytes after its runtime identity".to_owned(),
        ));
    }
    Ok(sources)
}

/// Paths of a strategy bundle extracted for a worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedStrategyBundle {
    /// Root of the extracted strategy namespace; the worker's bundle root.
    pub strategy_root: PathBuf,
    /// The only import root the worker needs; it contains the SDK package.
    pub sdk_search_path: PathBuf,
    /// The entry point's extracted source file.
    pub strategy_file: PathBuf,
}

/// Writes verified archive sources into a directory that must not yet exist.
pub fn extract_strategy_bundle(
    sources: &[BundleSource],
    entry_point: &BundleEntryPoint,
    destination: &Path,
) -> Result<ExtractedStrategyBundle, EngineError> {
    fs::create_dir(destination)?;
    let strategy_root = destination.join("strategy");
    let sdk_search_path = destination.join("sdk");
    let sdk_root = sdk_search_path.join(SDK_PACKAGE_DIRECTORY);
    for source in sources {
        validate_bundle_path(&source.path)?;
        let root = match source.namespace {
            "strategy" => &strategy_root,
            "sdk" => &sdk_root,
            _ => {
                return Err(EngineError(
                    "strategy archive source has an unknown namespace".to_owned(),
                ))
            }
        };
        let target = source
            .path
            .split('/')
            .fold(root.clone(), |path, component| path.join(component));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?
            .write_all(&source.contents)?;
    }
    let strategy_file = entry_point
        .strategy_file
        .split('/')
        .fold(strategy_root.clone(), |path, component| {
            path.join(component)
        });
    if !strategy_file.is_file() {
        return Err(EngineError(
            "entry point was not extracted from the strategy namespace".to_owned(),
        ));
    }
    Ok(ExtractedStrategyBundle {
        strategy_root,
        sdk_search_path,
        strategy_file,
    })
}

/// Strategy capability capsule manifest matching `strategy-capsule-manifest.schema.json`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrategyCapsuleManifest {
    /// `capsule.<strategy_id>.<first 16 hex of the bundle hash>`.
    pub capsule_id: String,
    /// Strategy identity from the evaluated configuration.
    pub strategy_id: String,
    /// Strategy version from the evaluated configuration.
    pub strategy_version: String,
    /// SHA-256 of the strategy archive, which is the strategy bundle hash.
    pub bundle_sha256: String,
    /// SHA-256 of the exact configuration bytes.
    pub configuration_sha256: String,
    /// SHA-256 of the exact dependency lock bytes.
    pub dependency_lockfile_sha256: String,
    /// Runtime identity the bundle hash binds.
    pub runtime_target: String,
    /// `evaluation.<SHA-256 of the receipt bytes>`.
    pub evaluation_receipt_id: String,
    /// How to re-run the replay that sealed the capsule.
    pub replay_instruction_command: String,
    /// Certified disposition.
    pub export_disposition: CapsuleExportDisposition,
    /// When the capsule was sealed.
    pub packaged_at: String,
}

impl StrategyCapsuleManifest {
    /// Formats the manifest as canonical sorted-key JSON matching the v1 schema.
    pub fn to_json(&self) -> String {
        serde_json::json!({
            "bundle_sha256": self.bundle_sha256,
            "capsule_id": self.capsule_id,
            "capsule_schema_version": CAPSULE_SCHEMA_VERSION,
            "configuration_sha256": self.configuration_sha256,
            "dependency_lockfile_sha256": self.dependency_lockfile_sha256,
            "evaluation_receipt_id": self.evaluation_receipt_id,
            "export_disposition": self.export_disposition.as_str(),
            "packaged_at": self.packaged_at,
            "replay_instruction_command": self.replay_instruction_command,
            "runtime_target": self.runtime_target,
            "strategy_id": self.strategy_id,
            "strategy_version": self.strategy_version,
        })
        .to_string()
    }

    /// Parses a manifest, refusing any encoding but [`Self::to_json`]'s.
    pub fn parse(bytes: &[u8]) -> Result<Self, EngineError> {
        let value = canonical_json_object(bytes, "capsule manifest")?;
        let object = value
            .as_object()
            .ok_or_else(|| EngineError("capsule manifest is not an object".to_owned()))?;
        require_exact_json_fields(object, &MANIFEST_FIELDS, "capsule manifest")?;
        if object
            .get("capsule_schema_version")
            .and_then(serde_json::Value::as_u64)
            != Some(CAPSULE_SCHEMA_VERSION)
        {
            return Err(EngineError(
                "unsupported capsule manifest schema version".to_owned(),
            ));
        }
        let text = |field: &str| string_field(object, field, "capsule manifest").map(str::to_owned);
        let manifest = Self {
            capsule_id: text("capsule_id")?,
            strategy_id: text("strategy_id")?,
            strategy_version: text("strategy_version")?,
            bundle_sha256: text("bundle_sha256")?,
            configuration_sha256: text("configuration_sha256")?,
            dependency_lockfile_sha256: text("dependency_lockfile_sha256")?,
            runtime_target: text("runtime_target")?,
            evaluation_receipt_id: text("evaluation_receipt_id")?,
            replay_instruction_command: text("replay_instruction_command")?,
            export_disposition: CapsuleExportDisposition::parse(&text("export_disposition")?)?,
            packaged_at: text("packaged_at")?,
        };
        validate_utc_timestamp("capsule packaged_at", &manifest.packaged_at)?;
        Ok(manifest)
    }
}

/// Everything a capsule binds, cross-checked, before any manifest exists.
#[derive(Clone, Debug)]
pub struct CapsuleContents {
    /// Parsed dependency lock.
    pub lock: StrategyBundleLock,
    /// Sources read out of the archive, in archive order.
    pub sources: Vec<BundleSource>,
    /// Strategy identity from the configuration.
    pub strategy_id: String,
    /// Strategy version from the configuration.
    pub strategy_version: String,
    lock_bytes: Vec<u8>,
    bundle: Vec<u8>,
    configuration: Vec<u8>,
    receipt: Vec<u8>,
}

impl CapsuleContents {
    /// Cross-checks the four bound members against each other.
    ///
    /// The archive must open exactly as the lock describes, the receipt must
    /// record the configuration's own SHA-256, and the configuration must name
    /// a canonical strategy identity.
    pub fn new(
        lock_bytes: Vec<u8>,
        bundle: Vec<u8>,
        configuration: Vec<u8>,
        receipt: Vec<u8>,
    ) -> Result<Self, EngineError> {
        let lock = StrategyBundleLock::parse(&lock_bytes)?;
        let sources = open_strategy_bundle(&bundle, &lock)?;

        let configuration_value: serde_json::Value = serde_json::from_slice(&configuration)
            .map_err(|error| EngineError(format!("capsule configuration is not JSON: {error}")))?;
        let strategy = configuration_value
            .get("strategy")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| {
                EngineError("capsule configuration has no strategy object".to_owned())
            })?;
        let strategy_id =
            string_field(strategy, "strategy_id", "configuration strategy")?.to_owned();
        let strategy_version =
            string_field(strategy, "strategy_version", "configuration strategy")?.to_owned();
        validate_canonical_id("strategy_id", &strategy_id)?;

        let receipt_value: serde_json::Value = serde_json::from_slice(&receipt)
            .map_err(|error| EngineError(format!("evaluation receipt is not JSON: {error}")))?;
        if receipt_value
            .get("configuration_hash")
            .and_then(serde_json::Value::as_str)
            != Some(sha256_hex(&configuration).as_str())
        {
            return Err(EngineError(
                "evaluation receipt was not produced from this configuration".to_owned(),
            ));
        }
        Ok(Self {
            lock,
            sources,
            strategy_id,
            strategy_version,
            lock_bytes,
            bundle,
            configuration,
            receipt,
        })
    }

    /// Exact configuration bytes the evaluation ran with.
    pub fn configuration(&self) -> &[u8] {
        &self.configuration
    }

    /// Exact evaluation receipt bytes.
    pub fn receipt(&self) -> &[u8] {
        &self.receipt
    }

    /// Canonical capsule identity for these contents.
    pub fn capsule_id(&self) -> String {
        format!(
            "capsule.{}.{}",
            self.strategy_id,
            &self.lock.strategy_bundle_hash[..16]
        )
    }

    /// Canonical evaluation receipt identity: a content address of the receipt.
    pub fn evaluation_receipt_id(&self) -> String {
        format!("evaluation.{}", sha256_hex(&self.receipt))
    }

    /// Seals a `VERIFIED_PORTABLE` manifest, but only for a replay that
    /// reproduced the receipt byte for byte.
    ///
    /// `reproduced_receipt` must be the completion manifest a replay of the
    /// capsule's own contents produced. Anything else is refused, so there is
    /// no path to this disposition that skips the replay's evidence.
    pub fn seal(
        &self,
        packaged_at: &str,
        replay_instruction_command: &str,
        reproduced_receipt: &[u8],
    ) -> Result<StrategyCapsuleManifest, EngineError> {
        validate_utc_timestamp("capsule packaged_at", packaged_at)?;
        if replay_instruction_command.trim().is_empty() {
            return Err(EngineError(
                "capsule replay instruction is required".to_owned(),
            ));
        }
        if reproduced_receipt != self.receipt.as_slice() {
            return Err(EngineError(
                "capsule replay did not reproduce the evaluation receipt".to_owned(),
            ));
        }
        Ok(StrategyCapsuleManifest {
            capsule_id: self.capsule_id(),
            strategy_id: self.strategy_id.clone(),
            strategy_version: self.strategy_version.clone(),
            bundle_sha256: sha256_hex(&self.bundle),
            configuration_sha256: sha256_hex(&self.configuration),
            dependency_lockfile_sha256: sha256_hex(&self.lock_bytes),
            runtime_target: self.lock.runtime.clone(),
            evaluation_receipt_id: self.evaluation_receipt_id(),
            replay_instruction_command: replay_instruction_command.to_owned(),
            export_disposition: CapsuleExportDisposition::VerifiedPortable,
            packaged_at: packaged_at.to_owned(),
        })
    }

    /// Writes a sealed capsule into a directory that must not yet exist.
    ///
    /// The manifest is checked against these contents first, and a partial
    /// directory is removed if any write fails.
    pub fn write_sealed(
        &self,
        manifest: &StrategyCapsuleManifest,
        destination: &Path,
    ) -> Result<(), EngineError> {
        self.check_manifest(manifest)?;
        fs::create_dir(destination)?;
        let written = [
            (CAPSULE_BUNDLE_FILE, self.bundle.as_slice()),
            (CAPSULE_LOCK_FILE, self.lock_bytes.as_slice()),
            (CAPSULE_CONFIGURATION_FILE, self.configuration.as_slice()),
            (CAPSULE_RECEIPT_FILE, self.receipt.as_slice()),
            (CAPSULE_MANIFEST_FILE, manifest.to_json().as_bytes()),
        ]
        .into_iter()
        .try_for_each(|(name, contents)| {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination.join(name))?
                .write_all(contents)
        });
        if let Err(error) = written {
            let _ = fs::remove_dir_all(destination);
            return Err(error.into());
        }
        Ok(())
    }

    fn check_manifest(&self, manifest: &StrategyCapsuleManifest) -> Result<(), EngineError> {
        let bindings = [
            ("capsule_id", manifest.capsule_id.clone(), self.capsule_id()),
            (
                "strategy_id",
                manifest.strategy_id.clone(),
                self.strategy_id.clone(),
            ),
            (
                "strategy_version",
                manifest.strategy_version.clone(),
                self.strategy_version.clone(),
            ),
            (
                "bundle_sha256",
                manifest.bundle_sha256.clone(),
                sha256_hex(&self.bundle),
            ),
            (
                "configuration_sha256",
                manifest.configuration_sha256.clone(),
                sha256_hex(&self.configuration),
            ),
            (
                "dependency_lockfile_sha256",
                manifest.dependency_lockfile_sha256.clone(),
                sha256_hex(&self.lock_bytes),
            ),
            (
                "runtime_target",
                manifest.runtime_target.clone(),
                self.lock.runtime.clone(),
            ),
            (
                "evaluation_receipt_id",
                manifest.evaluation_receipt_id.clone(),
                self.evaluation_receipt_id(),
            ),
        ];
        if let Some((field, _, _)) = bindings
            .iter()
            .find(|(_, claimed, computed)| claimed != computed)
        {
            return Err(EngineError(format!(
                "capsule manifest {field} does not match the capsule contents"
            )));
        }
        if manifest.export_disposition != CapsuleExportDisposition::VerifiedPortable {
            return Err(EngineError(
                "a sealed capsule must carry the VERIFIED_PORTABLE disposition its replay earned"
                    .to_owned(),
            ));
        }
        validate_utc_timestamp("capsule packaged_at", &manifest.packaged_at)?;
        Ok(())
    }
}

/// A capsule read from disk whose manifest matches its contents.
#[derive(Clone, Debug)]
pub struct SealedStrategyCapsule {
    /// The parsed manifest.
    pub manifest: StrategyCapsuleManifest,
    /// The cross-checked contents.
    pub contents: CapsuleContents,
}

/// Reads a capsule directory and checks every static binding.
///
/// The directory must hold exactly the five capsule members as regular files.
/// Every digest in the manifest is recomputed from those files. This does not
/// replay the evaluation; `follon-backtest capsule-verify` does both.
pub fn read_strategy_capsule(directory: &Path) -> Result<SealedStrategyCapsule, EngineError> {
    if !fs::symlink_metadata(directory)?.is_dir() {
        return Err(EngineError(
            "capsule must be a non-symlink directory".to_owned(),
        ));
    }
    let mut members = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| EngineError("capsule member name is not UTF-8".to_owned()))?;
        if !entry.file_type()?.is_file() {
            return Err(EngineError(format!(
                "capsule member {name} is not a regular file"
            )));
        }
        members.push(name);
    }
    members.sort();
    let mut expected = [
        CAPSULE_BUNDLE_FILE,
        CAPSULE_CONFIGURATION_FILE,
        CAPSULE_LOCK_FILE,
        CAPSULE_MANIFEST_FILE,
        CAPSULE_RECEIPT_FILE,
    ];
    expected.sort_unstable();
    if members != expected {
        return Err(EngineError(
            "capsule must contain exactly its five members".to_owned(),
        ));
    }
    let read = |name: &str, limit: u64| -> Result<Vec<u8>, EngineError> {
        let path = directory.join(name);
        if fs::metadata(&path)?.len() > limit {
            return Err(EngineError(format!("capsule member {name} is too large")));
        }
        Ok(fs::read(path)?)
    };
    let manifest =
        StrategyCapsuleManifest::parse(&read(CAPSULE_MANIFEST_FILE, MAX_DOCUMENT_BYTES)?)?;
    let contents = CapsuleContents::new(
        read(CAPSULE_LOCK_FILE, MAX_DOCUMENT_BYTES)?,
        read(CAPSULE_BUNDLE_FILE, MAX_BUNDLE_BYTES)?,
        read(CAPSULE_CONFIGURATION_FILE, MAX_DOCUMENT_BYTES)?,
        read(CAPSULE_RECEIPT_FILE, MAX_DOCUMENT_BYTES)?,
    )?;
    contents.check_manifest(&manifest)?;
    Ok(SealedStrategyCapsule { manifest, contents })
}

fn collect_sources(
    directory: &Path,
    prefix: &str,
    sources: &mut Vec<(String, PathBuf)>,
) -> Result<(), EngineError> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| EngineError("strategy bundle path is not UTF-8".to_owned()))?;
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(EngineError(format!(
                "strategy bundle cannot contain a symlink: {relative}"
            )));
        }
        if file_type.is_dir() {
            collect_sources(&entry.path(), &relative, sources)?;
        } else if file_type.is_file() && name.ends_with(".py") {
            sources.push((relative, entry.path()));
        }
    }
    Ok(())
}

fn push_u32_framed(archive: &mut Vec<u8>, value: &[u8]) -> Result<(), EngineError> {
    let length = u32::try_from(value.len())
        .map_err(|_| EngineError("strategy bundle field is too long".to_owned()))?;
    archive.extend_from_slice(&length.to_be_bytes());
    archive.extend_from_slice(value);
    Ok(())
}

struct ArchiveReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ArchiveReader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], EngineError> {
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| EngineError("strategy archive is truncated".to_owned()))?;
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }

    fn u32_framed(&mut self) -> Result<&'a [u8], EngineError> {
        let mut length = [0_u8; 4];
        length.copy_from_slice(self.take(4)?);
        self.take(u32::from_be_bytes(length) as usize)
    }

    fn u64(&mut self) -> Result<u64, EngineError> {
        let mut value = [0_u8; 8];
        value.copy_from_slice(self.take(8)?);
        Ok(u64::from_be_bytes(value))
    }
}

fn canonical_json_object(bytes: &[u8], context: &str) -> Result<serde_json::Value, EngineError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| EngineError(format!("{context} is not JSON: {error}")))?;
    if serde_json::to_vec(&value).ok().as_deref() != Some(bytes) {
        return Err(EngineError(format!(
            "{context} is not canonical sorted-key compact JSON"
        )));
    }
    Ok(value)
}

fn string_field<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
    context: &str,
) -> Result<&'a str, EngineError> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| EngineError(format!("{context} {field} must be a non-empty string")))
}

fn validate_runtime(runtime: &str) -> Result<(), EngineError> {
    let parts: Vec<&str> = runtime.split('|').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() && byte != b'|')
        })
    {
        return Err(EngineError(
            "runtime identity must be implementation|version|platform".to_owned(),
        ));
    }
    Ok(())
}

fn validate_bundle_path(path: &str) -> Result<(), EngineError> {
    let valid = path.len() <= 1024
        && path.ends_with(".py")
        && path.split('/').all(|component| {
            !component.is_empty()
                && component != "."
                && component != ".."
                && component
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        });
    if !valid {
        return Err(EngineError(format!(
            "strategy bundle path is not a safe relative source path: {path}"
        )));
    }
    Ok(())
}

fn is_python_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNTIME: &str = "cpython|3.12.10|linux";
    /// `_bundle_digest` of [`write_vector_tree`] under [`RUNTIME`], computed by
    /// the Python SDK. `python/strategy-sdk/tests/test_bundle.py` pins the same
    /// value, so the two implementations cannot drift apart silently.
    const PYTHON_VECTOR_HASH: &str =
        "dd161695d4e3fa38bd1c790567c2431df6d0b5c6be447dfc9b2e43193aaea62f";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("follon-capsule-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_vector_tree(root: &Path) -> (PathBuf, PathBuf) {
        let strategy = root.join("strategy");
        let sdk = root.join("sdk");
        fs::create_dir_all(strategy.join("pkg")).unwrap();
        fs::create_dir_all(&sdk).unwrap();
        fs::write(strategy.join("alpha.py"), b"ALPHA = 1\n").unwrap();
        // Byte order puts this first; NTFS directory order puts it last.
        fs::write(strategy.join("Zeta.py"), b"ZETA = 26\n").unwrap();
        fs::write(strategy.join("pkg/beta.py"), b"from alpha import ALPHA\n").unwrap();
        fs::write(strategy.join("notes.txt"), b"not source\n").unwrap();
        fs::write(sdk.join("__init__.py"), b"\"\"\"sdk\"\"\"\n").unwrap();
        (strategy, sdk)
    }

    /// The lock the SDK writes for `files`, built the same way it does.
    fn lock_document(
        files: &[(&str, &str, &[u8])],
        runtime: &str,
        hash: &str,
    ) -> serde_json::Value {
        let namespace = |name: &str| {
            serde_json::json!({
                "files": files
                    .iter()
                    .filter(|(namespace, _, _)| *namespace == name)
                    .map(|(_, path, contents)| serde_json::json!({
                        "bytes": contents.len(),
                        "path": path,
                        "sha256": sha256_hex(contents),
                    }))
                    .collect::<Vec<_>>(),
                "name": name,
            })
        };
        serde_json::json!({
            "bundle_format": STRATEGY_BUNDLE_FORMAT,
            "entry_point": {"class_name": "Alpha", "strategy_file": "alpha.py"},
            "lock_schema_version": 1,
            "namespaces": [namespace("strategy"), namespace("sdk")],
            "runtime": runtime,
            "strategy_bundle_hash": hash,
        })
    }

    const VECTOR_FILES: [(&str, &str, &[u8]); 4] = [
        ("strategy", "Zeta.py", b"ZETA = 26\n"),
        ("strategy", "alpha.py", b"ALPHA = 1\n"),
        ("strategy", "pkg/beta.py", b"from alpha import ALPHA\n"),
        ("sdk", "__init__.py", b"\"\"\"sdk\"\"\"\n"),
    ];

    fn vector_archive(scratch: &Scratch) -> Vec<u8> {
        let (strategy, sdk) = write_vector_tree(&scratch.0);
        build_strategy_bundle(&strategy, &sdk, RUNTIME).unwrap()
    }

    fn vector_lock_bytes(hash: &str) -> Vec<u8> {
        serde_json::to_vec(&lock_document(&VECTOR_FILES, RUNTIME, hash)).unwrap()
    }

    fn vector_contents(scratch: &Scratch) -> CapsuleContents {
        let archive = vector_archive(scratch);
        let lock = vector_lock_bytes(&sha256_hex(&archive));
        let configuration =
            br#"{"strategy":{"strategy_id":"strategy.vector","strategy_version":"v1"}}"#.to_vec();
        let receipt = serde_json::to_vec(
            &serde_json::json!({"configuration_hash": sha256_hex(&configuration)}),
        )
        .unwrap();
        CapsuleContents::new(lock, archive, configuration, receipt).unwrap()
    }

    #[test]
    fn archive_hashes_to_the_python_sdk_bundle_hash() {
        let scratch = Scratch::new("vector");
        let archive = vector_archive(&scratch);
        assert_eq!(sha256_hex(&archive), PYTHON_VECTOR_HASH);
        let lock = StrategyBundleLock::parse(&vector_lock_bytes(PYTHON_VECTOR_HASH)).unwrap();
        let sources = open_strategy_bundle(&archive, &lock).unwrap();
        let paths: Vec<_> = sources
            .iter()
            .map(|source| (source.namespace, source.path.as_str()))
            .collect();
        assert_eq!(
            paths,
            [
                ("strategy", "Zeta.py"),
                ("strategy", "alpha.py"),
                ("strategy", "pkg/beta.py"),
                ("sdk", "__init__.py")
            ]
        );
    }

    #[test]
    fn archive_refuses_symlinked_source() {
        let scratch = Scratch::new("symlink");
        let (strategy, sdk) = write_vector_tree(&scratch.0);
        #[cfg(unix)]
        let linked =
            std::os::unix::fs::symlink(strategy.join("alpha.py"), strategy.join("link.py"));
        #[cfg(windows)]
        let linked =
            std::os::windows::fs::symlink_file(strategy.join("alpha.py"), strategy.join("link.py"));
        if linked.is_err() {
            eprintln!("this account cannot create symlinks; symlink refusal was not exercised");
            return;
        }
        assert!(build_strategy_bundle(&strategy, &sdk, RUNTIME).is_err());
    }

    #[test]
    fn lock_parse_refuses_unsafe_or_noncanonical_locks() {
        let valid = lock_document(&VECTOR_FILES, RUNTIME, PYTHON_VECTOR_HASH);
        assert!(StrategyBundleLock::parse(&serde_json::to_vec(&valid).unwrap()).is_ok());

        let refused = |document: serde_json::Value, why: &str| {
            assert!(
                StrategyBundleLock::parse(&serde_json::to_vec(&document).unwrap()).is_err(),
                "lock accepted despite {why}"
            );
        };
        let with_files =
            |files: &[(&str, &str, &[u8])]| lock_document(files, RUNTIME, PYTHON_VECTOR_HASH);
        // Each unsafe path sits beside a valid entry point, so the path check
        // is the only one that can refuse it.
        for unsafe_path in [
            "../escape.py",
            "C:escape.py",
            "pkg//escape.py",
            "./escape.py",
            "pkg\\escape.py",
            "escape.txt",
        ] {
            let mut files = vec![
                ("strategy", unsafe_path, b"x".as_slice()),
                ("strategy", "alpha.py", b"x".as_slice()),
                ("sdk", "a.py", b"x".as_slice()),
            ];
            files.sort_by_key(|(namespace, path, _)| (*namespace != "strategy", *path));
            refused(with_files(&files), unsafe_path);
        }
        refused(
            with_files(&[
                ("strategy", "pkg/beta.py", b"x"),
                ("strategy", "alpha.py", b"x"),
                ("sdk", "a.py", b"x"),
            ]),
            "descending paths",
        );
        refused(
            with_files(&[
                ("strategy", "alpha.py", b"x"),
                ("strategy", "alpha.py", b"x"),
                ("sdk", "a.py", b"x"),
            ]),
            "a duplicate path",
        );
        refused(
            with_files(&[("strategy", "alpha.py", b"x")]),
            "an empty sdk namespace",
        );

        let mut swapped = valid.clone();
        swapped["namespaces"].as_array_mut().unwrap().swap(0, 1);
        refused(swapped, "sdk before strategy");
        let mut outside = valid.clone();
        outside["entry_point"]["strategy_file"] = "missing.py".into();
        refused(outside, "an entry point outside the strategy namespace");
        let mut class = valid.clone();
        class["entry_point"]["class_name"] = "1Alpha".into();
        refused(class, "a class name that is not an identifier");
        let mut runtime = valid.clone();
        runtime["runtime"] = "cpython 3.12".into();
        refused(runtime, "a malformed runtime identity");
        let mut extra = valid.clone();
        extra["third_party"] = serde_json::json!([]);
        refused(extra, "an unknown field");

        let pretty = serde_json::to_vec_pretty(&valid).unwrap();
        assert!(
            StrategyBundleLock::parse(&pretty).is_err(),
            "lock accepted despite a non-canonical encoding"
        );
    }

    #[test]
    fn archive_must_open_exactly_as_its_lock_describes() {
        let scratch = Scratch::new("open");
        let archive = vector_archive(&scratch);
        let lock_for = |archive: &[u8]| {
            StrategyBundleLock::parse(&vector_lock_bytes(&sha256_hex(archive))).unwrap()
        };
        let lock = lock_for(&archive);
        assert!(open_strategy_bundle(&archive, &lock).is_ok());

        let mut other_hash = lock.clone();
        other_hash.strategy_bundle_hash = "0".repeat(64);
        assert!(open_strategy_bundle(&archive, &other_hash).is_err());

        // Each tamper below re-points the lock's bundle hash at the tampered
        // archive, so only the check under test can refuse it.
        let position = archive
            .windows(b"ALPHA = 1".len())
            .position(|window| window == b"ALPHA = 1")
            .unwrap();
        let mut content = archive.clone();
        content[position] = b'B';
        assert!(open_strategy_bundle(&content, &lock_for(&content)).is_err());

        let mut trailing = archive.clone();
        trailing.push(0);
        assert!(open_strategy_bundle(&trailing, &lock_for(&trailing)).is_err());

        let mut truncated = archive.clone();
        truncated.truncate(archive.len() - 1);
        assert!(open_strategy_bundle(&truncated, &lock_for(&truncated)).is_err());

        let other_runtime = vector_archive_with_runtime(&scratch, "cpython|3.12.11|linux");
        assert!(open_strategy_bundle(&other_runtime, &lock_for(&other_runtime)).is_err());

        let fewer = StrategyBundleLock::parse(
            &serde_json::to_vec(&lock_document(
                &[VECTOR_FILES[0], VECTOR_FILES[1], VECTOR_FILES[3]],
                RUNTIME,
                &sha256_hex(&archive),
            ))
            .unwrap(),
        )
        .unwrap();
        assert!(open_strategy_bundle(&archive, &fewer).is_err());
    }

    fn vector_archive_with_runtime(scratch: &Scratch, runtime: &str) -> Vec<u8> {
        build_strategy_bundle(&scratch.0.join("strategy"), &scratch.0.join("sdk"), runtime).unwrap()
    }

    #[test]
    fn extraction_writes_an_importable_layout_into_a_fresh_directory() {
        let scratch = Scratch::new("extract");
        let contents = vector_contents(&scratch);
        let destination = scratch.0.join("extracted");
        let extracted =
            extract_strategy_bundle(&contents.sources, &contents.lock.entry_point, &destination)
                .unwrap();
        assert_eq!(
            extracted.strategy_file,
            destination.join("strategy").join("alpha.py")
        );
        assert_eq!(
            fs::read(extracted.strategy_root.join("pkg").join("beta.py")).unwrap(),
            b"from alpha import ALPHA\n"
        );
        assert_eq!(
            fs::read(
                extracted
                    .sdk_search_path
                    .join(SDK_PACKAGE_DIRECTORY)
                    .join("__init__.py")
            )
            .unwrap(),
            b"\"\"\"sdk\"\"\"\n"
        );
        assert!(
            extract_strategy_bundle(&contents.sources, &contents.lock.entry_point, &destination)
                .is_err(),
            "extraction reused an existing directory"
        );
    }

    #[test]
    fn manifest_json_has_exactly_the_schema_fields_and_escapes_strings() {
        let scratch = Scratch::new("manifest");
        let contents = vector_contents(&scratch);
        let receipt = contents.receipt().to_vec();
        let command = "follon-backtest capsule-verify \"C:\\capsules\\a b\"";
        let manifest = contents
            .seal("2026-09-07T12:00:00Z", command, &receipt)
            .unwrap();
        let json = manifest.to_json();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["replay_instruction_command"], command);
        assert_eq!(
            StrategyCapsuleManifest::parse(json.as_bytes()).unwrap(),
            manifest
        );

        let schema: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../contracts/json-schema/v1/strategy-capsule-manifest.schema.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let mut required: Vec<_> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field.as_str().unwrap().to_owned())
            .collect();
        required.sort();
        let emitted: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        assert_eq!(emitted, required);
        assert_eq!(MANIFEST_FIELDS.to_vec(), required);
        assert!(value["capsule_id"]
            .as_str()
            .unwrap()
            .starts_with("capsule.strategy.vector."));
    }

    #[test]
    fn only_a_reproducing_replay_seals_a_verified_manifest() {
        let scratch = Scratch::new("seal");
        let contents = vector_contents(&scratch);
        let mut diverged = contents.receipt().to_vec();
        diverged.push(b' ');
        assert!(contents
            .seal("2026-09-07T12:00:00Z", "replay", &diverged)
            .is_err());
        let receipt = contents.receipt().to_vec();
        let manifest = contents
            .seal("2026-09-07T12:00:00Z", "replay", &receipt)
            .unwrap();
        assert_eq!(
            manifest.export_disposition,
            CapsuleExportDisposition::VerifiedPortable
        );
        assert_eq!(manifest.bundle_sha256, contents.lock.strategy_bundle_hash);
        assert!(contents
            .seal("2026-09-07 12:00", "replay", &receipt)
            .is_err());
    }

    #[test]
    fn contents_refuse_a_receipt_from_another_configuration() {
        let scratch = Scratch::new("receipt");
        let archive = vector_archive(&scratch);
        let lock = vector_lock_bytes(&sha256_hex(&archive));
        let configuration =
            br#"{"strategy":{"strategy_id":"strategy.vector","strategy_version":"v1"}}"#.to_vec();
        let receipt = serde_json::to_vec(
            &serde_json::json!({"configuration_hash": sha256_hex(b"another configuration")}),
        )
        .unwrap();
        assert!(CapsuleContents::new(lock, archive, configuration, receipt).is_err());
    }

    #[test]
    fn reading_a_capsule_recomputes_every_binding() {
        let scratch = Scratch::new("read");
        let contents = vector_contents(&scratch);
        let receipt = contents.receipt().to_vec();
        let manifest = contents
            .seal("2026-09-07T12:00:00Z", "replay", &receipt)
            .unwrap();
        let sealed = scratch.0.join("sealed");
        contents.write_sealed(&manifest, &sealed).unwrap();
        assert_eq!(read_strategy_capsule(&sealed).unwrap().manifest, manifest);
        assert!(
            contents.write_sealed(&manifest, &sealed).is_err(),
            "a sealed capsule was overwritten"
        );

        let copy = |name: &str| {
            let target = scratch.0.join(name);
            let _ = fs::remove_dir_all(&target);
            fs::create_dir(&target).unwrap();
            for entry in fs::read_dir(&sealed).unwrap() {
                let entry = entry.unwrap();
                fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
            }
            target
        };
        for member in [
            CAPSULE_BUNDLE_FILE,
            CAPSULE_LOCK_FILE,
            CAPSULE_CONFIGURATION_FILE,
            CAPSULE_RECEIPT_FILE,
            CAPSULE_MANIFEST_FILE,
        ] {
            let tampered = copy("tampered");
            let path = tampered.join(member);
            let mut bytes = fs::read(&path).unwrap();
            let last = bytes.len() - 2;
            bytes[last] ^= 1;
            fs::write(&path, bytes).unwrap();
            assert!(
                read_strategy_capsule(&tampered).is_err(),
                "a capsule with a tampered {member} was accepted"
            );
        }

        let extra = copy("extra");
        fs::write(extra.join("notes.txt"), b"x").unwrap();
        assert!(read_strategy_capsule(&extra).is_err());
        let missing = copy("missing");
        fs::remove_file(missing.join(CAPSULE_RECEIPT_FILE)).unwrap();
        assert!(read_strategy_capsule(&missing).is_err());

        // A well-formed manifest that claims a disposition its replay did not
        // earn, or a hash its contents do not have, is refused.
        for claim in [
            StrategyCapsuleManifest {
                export_disposition: CapsuleExportDisposition::UnverifiedEvaluation,
                ..manifest.clone()
            },
            StrategyCapsuleManifest {
                configuration_sha256: "0".repeat(64),
                ..manifest.clone()
            },
            StrategyCapsuleManifest {
                capsule_id: "capsule.strategy.vector.0000000000000000".to_owned(),
                ..manifest.clone()
            },
        ] {
            let claimed = copy("claimed");
            fs::write(claimed.join(CAPSULE_MANIFEST_FILE), claim.to_json()).unwrap();
            assert!(
                read_strategy_capsule(&claimed).is_err(),
                "an unearned manifest claim was accepted: {}",
                claim.to_json()
            );
        }
    }
}
