//! Local commercial-control operator CLI.
//!
//! This binary never receives card numbers, credentials, raw customer identity,
//! or broker secrets. Payment and identity systems are external evidence sources;
//! this tool records only pseudonymous references and SHA-256 evidence hashes.

use std::collections::BTreeSet;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use follon_cli::write_immutable;
use follon_commercial::{
    build_release_manifest, derive_entitlement, execute_retention_candidate,
    generate_release_keypair, plan_expired_retention, plan_privacy_erasure, sign_release_manifest,
    verify_entitled_self_host_readiness, verify_release_artifacts, verify_release_signature,
    BillingStatus, CommercialLedger, CommercialPlan, DataAsset, DataClassification, PrivacyRequest,
    PrivacyRequestKind, ReleaseSignature, SelfHostConfig, SubscriptionObservation,
    TenantProvisioning, TrustedReleaseKey,
};
use serde::Deserialize;

const COMMERCIAL_INPUT_SCHEMA_VERSION: u32 = 1;
const DATA_INVENTORY_SCHEMA_VERSION: u32 = 1;
const PRIVACY_REQUEST_SCHEMA_VERSION: u32 = 1;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.is_empty() || matches!(arguments[0].as_str(), "help" | "--help" | "-h") {
        print_usage();
        return Ok(());
    }
    match arguments[0].as_str() {
        "provision" => provision(&arguments[1..]),
        "subscription" => subscription(&arguments[1..]),
        "entitlement" => entitlement(&arguments[1..]),
        "retention-plan" => retention_plan(&arguments[1..]),
        "privacy-plan" => privacy_plan(&arguments[1..]),
        "retention-execute" => retention_execute(&arguments[1..]),
        "release-manifest" => release_manifest(&arguments[1..]),
        "release-keygen" => release_keygen(&arguments[1..]),
        "release-sign" => release_sign(&arguments[1..]),
        "release-verify" => release_verify(&arguments[1..]),
        "self-host-validate" => self_host_validate(&arguments[1..]),
        "self-host-readiness" => self_host_readiness(&arguments[1..]),
        command => {
            Err(format!("unsupported follon-admin command: {command}\n\n{}", usage()).into())
        }
    }
}

fn provision(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let input = required_positional(arguments, 0, "provisioning input")?;
    let ledger_path = PathBuf::from(required_option(arguments, "--ledger")?);
    let event_id = required_option(arguments, "--event-id")?;
    let actor = required_option(arguments, "--actor")?;
    reject_unexpected(arguments, &[0], &["--ledger", "--event-id", "--actor"])?;
    let provisioning = load_provisioning(Path::new(input))?;
    ensure_parent(&ledger_path)?;
    let mut ledger = CommercialLedger::open(&ledger_path)?;
    let record = ledger.provision_tenant(event_id, actor, &provisioning)?;
    println!("{}", record.canonical_json());
    Ok(())
}

fn subscription(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let input = required_positional(arguments, 0, "subscription input")?;
    let ledger_path = PathBuf::from(required_option(arguments, "--ledger")?);
    let event_id = required_option(arguments, "--event-id")?;
    let actor = required_option(arguments, "--actor")?;
    let observed_at = required_option(arguments, "--observed-at")?;
    reject_unexpected(
        arguments,
        &[0],
        &["--ledger", "--event-id", "--actor", "--observed-at"],
    )?;
    let observation = load_subscription(Path::new(input))?;
    ensure_parent(&ledger_path)?;
    let mut ledger = CommercialLedger::open(&ledger_path)?;
    let record = ledger.record_subscription(event_id, actor, observed_at, &observation)?;
    println!("{}", record.canonical_json());
    Ok(())
}

fn entitlement(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let tenant_id = required_positional(arguments, 0, "tenant_id")?;
    let ledger_path = PathBuf::from(required_option(arguments, "--ledger")?);
    let as_of = required_option(arguments, "--as-of")?;
    reject_unexpected(arguments, &[0], &["--ledger", "--as-of"])?;
    let records = CommercialLedger::read_verified(&ledger_path)?;
    println!(
        "{}",
        derive_entitlement(&records, tenant_id, as_of)?.canonical_json()
    );
    Ok(())
}

fn retention_plan(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let inventory_path = PathBuf::from(required_positional(arguments, 0, "data inventory")?);
    let data_root = PathBuf::from(required_option(arguments, "--data-root")?);
    let tenant_id = required_option(arguments, "--tenant-id")?;
    let as_of = required_option(arguments, "--as-of")?;
    let output = PathBuf::from(required_option(arguments, "--output")?);
    reject_unexpected(
        arguments,
        &[0],
        &["--data-root", "--tenant-id", "--as-of", "--output"],
    )?;
    let inventory = load_inventory(&inventory_path)?;
    if inventory.tenant_id != tenant_id {
        return Err("retention plan tenant_id must match the inventory tenant_id".into());
    }
    let plan = plan_expired_retention(&data_root, &inventory.assets, tenant_id, as_of)?;
    publish_immutable(&output, &plan.canonical_json())?;
    println!("{}", plan.fingerprint()?);
    Ok(())
}

fn privacy_plan(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let inventory_path = PathBuf::from(required_positional(arguments, 0, "data inventory")?);
    let request_path = PathBuf::from(required_positional(arguments, 1, "privacy request")?);
    let data_root = PathBuf::from(required_option(arguments, "--data-root")?);
    let as_of = required_option(arguments, "--as-of")?;
    let output = PathBuf::from(required_option(arguments, "--output")?);
    reject_unexpected(arguments, &[0, 1], &["--data-root", "--as-of", "--output"])?;
    let inventory = load_inventory(&inventory_path)?;
    let request = load_privacy_request(&request_path)?;
    if inventory.tenant_id != request.tenant_id {
        return Err("privacy request tenant_id must match the inventory tenant_id".into());
    }
    let plan = plan_privacy_erasure(&data_root, &inventory.assets, &request, as_of)?;
    publish_immutable(&output, &plan.canonical_json())?;
    println!("{}", plan.fingerprint()?);
    Ok(())
}

fn retention_execute(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let plan_path = PathBuf::from(required_positional(arguments, 0, "retention plan")?);
    let data_root = PathBuf::from(required_option(arguments, "--data-root")?);
    let asset_id = required_option(arguments, "--asset-id")?;
    let confirmed_plan_hash = required_option(arguments, "--confirm-plan-hash")?;
    let executed_at = required_option(arguments, "--executed-at")?;
    let actor = required_option(arguments, "--actor")?;
    let receipt_path = PathBuf::from(required_option(arguments, "--receipt")?);
    reject_unexpected(
        arguments,
        &[0],
        &[
            "--data-root",
            "--asset-id",
            "--confirm-plan-hash",
            "--executed-at",
            "--actor",
            "--receipt",
        ],
    )?;
    let plan = follon_commercial::RetentionPlan::parse_canonical(&read_regular_text(&plan_path)?)?;
    let receipt = execute_retention_candidate(
        &data_root,
        &plan,
        asset_id,
        confirmed_plan_hash,
        executed_at,
        actor,
    )?;
    publish_immutable(&receipt_path, &receipt.canonical_json())?;
    println!("{}", receipt.canonical_json());
    Ok(())
}

fn release_manifest(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let allowed = [
        "--release-id",
        "--version",
        "--created-at",
        "--source-revision",
        "--sbom-sha256",
        "--artifacts-root",
        "--artifact",
        "--output",
    ];
    reject_option_only_arguments(arguments, &allowed, "--artifact")?;
    let specs = repeated_option(arguments, "--artifact")?
        .into_iter()
        .map(parse_artifact_spec)
        .collect::<Result<Vec<_>, _>>()?;
    if specs.is_empty() {
        return Err(
            "release-manifest requires at least one --artifact <artifact_id=relative_path>".into(),
        );
    }
    let output = PathBuf::from(required_option(arguments, "--output")?);
    let manifest = build_release_manifest(
        required_option(arguments, "--release-id")?,
        required_option(arguments, "--version")?,
        required_option(arguments, "--created-at")?,
        required_option(arguments, "--source-revision")?,
        required_option(arguments, "--sbom-sha256")?,
        Path::new(required_option(arguments, "--artifacts-root")?),
        &specs,
    )?;
    publish_immutable(&output, &manifest.canonical_json())?;
    println!("{}", manifest.fingerprint()?);
    Ok(())
}

fn release_keygen(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let key_id = required_option(arguments, "--key-id")?;
    let private_key_path = PathBuf::from(required_option(arguments, "--private-key")?);
    let trusted_key_path = PathBuf::from(required_option(arguments, "--trusted-key")?);
    reject_unexpected(
        arguments,
        &[],
        &["--key-id", "--private-key", "--trusted-key"],
    )?;
    let (mut private_key, trusted_key) = generate_release_keypair(key_id)?;
    let write_result = write_new_private_key(&private_key_path, &private_key)
        .and_then(|_| publish_immutable(&trusted_key_path, &trusted_key.canonical_json()));
    private_key.fill(0);
    write_result?;
    println!("{}", trusted_key.canonical_json());
    Ok(())
}

fn release_sign(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = PathBuf::from(required_positional(arguments, 0, "release manifest")?);
    let private_key_path = PathBuf::from(required_option(arguments, "--private-key")?);
    let key_id = required_option(arguments, "--key-id")?;
    let signed_at = required_option(arguments, "--signed-at")?;
    let output = PathBuf::from(required_option(arguments, "--output")?);
    reject_unexpected(
        arguments,
        &[0],
        &["--private-key", "--key-id", "--signed-at", "--output"],
    )?;
    let manifest_source = read_regular_text(&manifest_path)?;
    let mut private_key = read_regular_bytes(&private_key_path)?;
    let signing_result = sign_release_manifest(&manifest_source, &private_key, key_id, signed_at);
    private_key.fill(0);
    let signature = signing_result?;
    publish_immutable(&output, &signature.canonical_json())?;
    println!("{}", signature.canonical_json());
    Ok(())
}

fn release_verify(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = PathBuf::from(required_positional(arguments, 0, "release manifest")?);
    let signature_path = PathBuf::from(required_positional(arguments, 1, "release signature")?);
    let trusted_key_path = PathBuf::from(required_positional(arguments, 2, "trusted release key")?);
    let artifact_root = PathBuf::from(required_option(arguments, "--artifacts-root")?);
    reject_unexpected(arguments, &[0, 1, 2], &["--artifacts-root"])?;
    let manifest_source = read_regular_text(&manifest_path)?;
    let signature = ReleaseSignature::parse_canonical(&read_regular_text(&signature_path)?)?;
    let trusted_key = TrustedReleaseKey::parse_canonical(&read_regular_text(&trusted_key_path)?)?;
    let manifest = verify_release_signature(&manifest_source, &signature, &trusted_key)?;
    verify_release_artifacts(&manifest, &artifact_root)?;
    println!("{}", manifest.fingerprint()?);
    Ok(())
}

fn self_host_validate(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = PathBuf::from(required_positional(arguments, 0, "self-host config")?);
    reject_unexpected(arguments, &[0], &[])?;
    let config = SelfHostConfig::parse_canonical(&read_regular_text(&config_path)?)?;
    println!("{}", config.fingerprint()?);
    Ok(())
}

fn self_host_readiness(arguments: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = PathBuf::from(required_positional(arguments, 0, "self-host config")?);
    let manifest_path = PathBuf::from(required_positional(arguments, 1, "release manifest")?);
    let signature_path = PathBuf::from(required_positional(arguments, 2, "release signature")?);
    let trusted_key_path = PathBuf::from(required_positional(arguments, 3, "trusted release key")?);
    let artifact_root = PathBuf::from(required_option(arguments, "--artifacts-root")?);
    let ledger_path = PathBuf::from(required_option(arguments, "--ledger")?);
    let as_of = required_option(arguments, "--as-of")?;
    let output = PathBuf::from(required_option(arguments, "--output")?);
    reject_unexpected(
        arguments,
        &[0, 1, 2, 3],
        &["--artifacts-root", "--ledger", "--as-of", "--output"],
    )?;
    let config = SelfHostConfig::parse_canonical(&read_regular_text(&config_path)?)?;
    let entitlement = derive_entitlement(
        &CommercialLedger::read_verified(&ledger_path)?,
        &config.tenant_id,
        as_of,
    )?;
    let readiness = verify_entitled_self_host_readiness(
        &config,
        &read_regular_text(&manifest_path)?,
        &read_regular_text(&signature_path)?,
        &TrustedReleaseKey::parse_canonical(&read_regular_text(&trusted_key_path)?)?,
        &artifact_root,
        &entitlement,
    )?;
    publish_immutable(&output, &readiness)?;
    println!("{readiness}");
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvisioningDocument {
    commercial_provisioning_schema_version: u32,
    tenant_id: String,
    workspace_id: String,
    plan: String,
    retention_policy_id: String,
    self_hosted: bool,
    provisioned_at: String,
}

fn load_provisioning(path: &Path) -> Result<TenantProvisioning, Box<dyn std::error::Error>> {
    let document: ProvisioningDocument = serde_json::from_str(&read_regular_text(path)?)?;
    if document.commercial_provisioning_schema_version != COMMERCIAL_INPUT_SCHEMA_VERSION {
        return Err("unsupported commercial provisioning schema version".into());
    }
    let provisioning = TenantProvisioning {
        tenant_id: document.tenant_id,
        workspace_id: document.workspace_id,
        plan: CommercialPlan::parse(&document.plan)?,
        retention_policy_id: document.retention_policy_id,
        self_hosted: document.self_hosted,
        provisioned_at: document.provisioned_at,
    };
    provisioning.validate()?;
    Ok(provisioning)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubscriptionDocument {
    commercial_subscription_schema_version: u32,
    subscription_id: String,
    tenant_id: String,
    plan: String,
    status: String,
    effective_at: String,
    expires_at: String,
    payment_provider: String,
    external_customer_ref: String,
    payment_evidence_hash: String,
}

fn load_subscription(path: &Path) -> Result<SubscriptionObservation, Box<dyn std::error::Error>> {
    let document: SubscriptionDocument = serde_json::from_str(&read_regular_text(path)?)?;
    if document.commercial_subscription_schema_version != COMMERCIAL_INPUT_SCHEMA_VERSION {
        return Err("unsupported commercial subscription schema version".into());
    }
    let subscription = SubscriptionObservation {
        subscription_id: document.subscription_id,
        tenant_id: document.tenant_id,
        plan: CommercialPlan::parse(&document.plan)?,
        status: BillingStatus::parse(&document.status)?,
        effective_at: document.effective_at,
        expires_at: document.expires_at,
        payment_provider: document.payment_provider,
        external_customer_ref: document.external_customer_ref,
        payment_evidence_hash: document.payment_evidence_hash,
    };
    subscription.validate()?;
    Ok(subscription)
}

struct DataInventory {
    tenant_id: String,
    assets: Vec<DataAsset>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataInventoryDocument {
    data_inventory_schema_version: u32,
    tenant_id: String,
    assets: Vec<DataAssetDocument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataAssetDocument {
    asset_id: String,
    tenant_id: String,
    relative_path: String,
    classification: String,
    created_at: String,
    retain_until: String,
    subject_hashes: Vec<String>,
    legal_hold: bool,
}

fn load_inventory(path: &Path) -> Result<DataInventory, Box<dyn std::error::Error>> {
    let document: DataInventoryDocument = serde_json::from_str(&read_regular_text(path)?)?;
    if document.data_inventory_schema_version != DATA_INVENTORY_SCHEMA_VERSION {
        return Err("unsupported data inventory schema version".into());
    }
    let mut asset_ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut assets = Vec::with_capacity(document.assets.len());
    for document_asset in document.assets {
        let asset = DataAsset {
            asset_id: document_asset.asset_id,
            tenant_id: document_asset.tenant_id,
            relative_path: document_asset.relative_path,
            classification: DataClassification::parse(&document_asset.classification)?,
            created_at: document_asset.created_at,
            retain_until: document_asset.retain_until,
            subject_hashes: document_asset.subject_hashes,
            legal_hold: document_asset.legal_hold,
        };
        asset.validate()?;
        if asset.tenant_id != document.tenant_id
            || !asset_ids.insert(asset.asset_id.clone())
            || !paths.insert(asset.relative_path.clone())
        {
            return Err(
                "data inventory assets must belong to its tenant and have unique IDs and paths"
                    .into(),
            );
        }
        assets.push(asset);
    }
    Ok(DataInventory {
        tenant_id: document.tenant_id,
        assets,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivacyRequestDocument {
    privacy_request_schema_version: u32,
    request_id: String,
    tenant_id: String,
    subject_hash: String,
    kind: String,
    requested_at: String,
}

fn load_privacy_request(path: &Path) -> Result<PrivacyRequest, Box<dyn std::error::Error>> {
    let document: PrivacyRequestDocument = serde_json::from_str(&read_regular_text(path)?)?;
    if document.privacy_request_schema_version != PRIVACY_REQUEST_SCHEMA_VERSION {
        return Err("unsupported privacy request schema version".into());
    }
    let request = PrivacyRequest {
        request_id: document.request_id,
        tenant_id: document.tenant_id,
        subject_hash: document.subject_hash,
        kind: PrivacyRequestKind::parse(&document.kind)?,
        requested_at: document.requested_at,
    };
    request.validate()?;
    Ok(request)
}

fn publish_immutable(path: &Path, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    ensure_parent(path)?;
    reject_symlink_output(path)?;
    write_immutable(path, contents)
}

fn write_new_private_key(path: &Path, contents: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    ensure_parent(path)?;
    reject_symlink_output(path)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    if let Err(error) = file.write_all(contents).and_then(|_| file.sync_data()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error.into());
    }
    Ok(())
}

fn read_regular_text(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let bytes = read_regular_bytes(path)?;
    String::from_utf8(bytes).map_err(|_| "file must contain UTF-8 text".into())
}

fn read_regular_bytes(path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("input must be a regular non-symbolic-link file".into());
    }
    Ok(fs::read(path)?)
}

fn ensure_parent(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let metadata = fs::symlink_metadata(parent)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("output parent must be a regular directory".into());
    }
    Ok(())
}

fn reject_symlink_output(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() && fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err("refusing to write through a symbolic link".into());
    }
    Ok(())
}

fn required_positional<'a>(
    arguments: &'a [String],
    index: usize,
    label: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    arguments
        .get(index)
        .filter(|value| !value.starts_with("--"))
        .map(String::as_str)
        .ok_or_else(|| format!("missing {label}").into())
}

fn required_option<'a>(
    arguments: &'a [String],
    option: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    let occurrences = arguments
        .iter()
        .enumerate()
        .filter(|(_, value)| value.as_str() == option)
        .collect::<Vec<_>>();
    if occurrences.len() != 1 {
        return Err(format!("{option} must be supplied exactly once").into());
    }
    let index = occurrences[0].0;
    let value = arguments
        .get(index + 1)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| format!("{option} requires a value"))?;
    Ok(value)
}

fn reject_unexpected(
    arguments: &[String],
    positional_indices: &[usize],
    options: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let expected = positional_indices.len() + options.len() * 2;
    if arguments.len() != expected {
        return Err(format!("unexpected or missing command arguments\n\n{}", usage()).into());
    }
    for option in options {
        let index = arguments
            .iter()
            .position(|value| value == option)
            .ok_or_else(|| format!("missing {option}"))?;
        if index + 1 >= arguments.len() || arguments[index + 1].starts_with("--") {
            return Err(format!("{option} requires a value").into());
        }
    }
    Ok(())
}

fn repeated_option<'a>(
    arguments: &'a [String],
    option: &str,
) -> Result<Vec<&'a str>, Box<dyn std::error::Error>> {
    let mut values = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] == option {
            let value = arguments
                .get(index + 1)
                .filter(|value| !value.starts_with("--"))
                .ok_or_else(|| format!("{option} requires a value"))?;
            values.push(value.as_str());
            index += 2;
        } else {
            index += 1;
        }
    }
    Ok(values)
}

fn parse_artifact_spec(value: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let (artifact_id, relative_path) = value
        .split_once('=')
        .filter(|(artifact_id, relative_path)| {
            !artifact_id.is_empty() && !relative_path.is_empty() && !relative_path.contains('=')
        })
        .ok_or("--artifact must be exactly artifact_id=relative_path")?;
    Ok((artifact_id.to_owned(), relative_path.to_owned()))
}

fn reject_option_only_arguments(
    arguments: &[String],
    allowed_options: &[&str],
    repeatable_option: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut seen = BTreeSet::new();
    let mut index = 0;
    while index < arguments.len() {
        let option = &arguments[index];
        if !allowed_options.contains(&option.as_str()) {
            return Err(format!("unsupported release-manifest argument: {option}").into());
        }
        if !seen.insert(option.as_str()) && option != repeatable_option {
            return Err(format!("{option} must be supplied exactly once").into());
        }
        let Some(value) = arguments.get(index + 1) else {
            return Err(format!("{option} requires a value").into());
        };
        if value.starts_with("--") {
            return Err(format!("{option} requires a value").into());
        }
        index += 2;
    }
    for option in allowed_options {
        if *option != repeatable_option && !seen.contains(option) {
            return Err(format!("missing {option}").into());
        }
    }
    Ok(())
}

fn usage() -> &'static str {
    "Usage:\n  follon-admin provision <provisioning.json> --ledger <ledger.ndjson> --event-id <id> --actor <id>\n  follon-admin subscription <subscription.json> --ledger <ledger.ndjson> --event-id <id> --actor <id> --observed-at <UTC>\n  follon-admin entitlement <tenant-id> --ledger <ledger.ndjson> --as-of <UTC>\n  follon-admin retention-plan <inventory.json> --data-root <directory> --tenant-id <id> --as-of <UTC> --output <plan.json>\n  follon-admin privacy-plan <inventory.json> <privacy-request.json> --data-root <directory> --as-of <UTC> --output <plan.json>\n  follon-admin retention-execute <plan.json> --data-root <directory> --asset-id <id> --confirm-plan-hash <sha256> --executed-at <UTC> --actor <id> --receipt <receipt.json>\n  follon-admin release-manifest --release-id <id> --version <version> --created-at <UTC> --source-revision <git-sha> --sbom-sha256 <sha256> --artifacts-root <directory> --artifact <artifact_id=relative_path> [--artifact <artifact_id=relative_path>] --output <manifest.json>\n  follon-admin release-keygen --key-id <id> --private-key <new.pk8> --trusted-key <trusted-key.json>\n  follon-admin release-sign <manifest.json> --private-key <key.pk8> --key-id <id> --signed-at <UTC> --output <signature.json>\n  follon-admin release-verify <manifest.json> <signature.json> <trusted-key.json> --artifacts-root <directory>\n  follon-admin self-host-validate <self-host.json>\n  follon-admin self-host-readiness <self-host.json> <manifest.json> <signature.json> <trusted-key.json> --artifacts-root <directory> --ledger <ledger.ndjson> --as-of <UTC> --output <readiness.json>"
}

fn print_usage() {
    println!("{}", usage());
}
