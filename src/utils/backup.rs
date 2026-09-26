use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use zip::write::FileOptions;
use zip::ZipWriter;

use crate::utils::config;
use crate::utils::crypto::{decrypt_secret, encrypt_secret};
use crate::utils::soroban::ContractInspectResult;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackupStatus {
    Created,
    Verified,
    VerificationFailed,
}

impl std::fmt::Display for BackupStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            BackupStatus::Created => "created",
            BackupStatus::Verified => "verified",
            BackupStatus::VerificationFailed => "verification-failed",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRecord {
    pub id: String,
    pub label: String,
    pub created_at: String,
    pub sources: Vec<String>,
    pub archive_path: PathBuf,
    pub encrypted: bool,
    pub checksum: String,
    pub size_bytes: u64,
    pub status: BackupStatus,
    pub verified_at: Option<String>,
    pub replicated_regions: Vec<String>,
    #[serde(default)]
    pub contract_state: Option<ContractStateMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationConfig {
    pub label: String,
    pub sources: Vec<String>,
    pub interval_hours: u64,
    pub encrypt: bool,
    pub region: String,
    pub last_run: Option<String>,
    #[serde(default)]
    pub contract: Option<ContractScheduleConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractStateMetadata {
    pub contract_id: String,
    pub source_network: String,
    pub latest_ledger: u32,
    pub last_modified_ledger_seq: Option<u32>,
    pub live_until_ledger_seq: Option<u32>,
    pub entry_count: usize,
    pub manifest_path: PathBuf,
    #[serde(default)]
    pub restore_history: Vec<CrossNetworkRestore>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractScheduleConfig {
    pub contract_id: String,
    pub network: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractStateManifest {
    pub version: u32,
    pub backup_id: Option<String>,
    pub contract_id: String,
    pub source_network: String,
    pub captured_at: String,
    pub latest_ledger: u32,
    pub executable: String,
    pub wasm_hash: Option<String>,
    pub storage_durability: String,
    pub last_modified_ledger_seq: Option<u32>,
    pub live_until_ledger_seq: Option<u32>,
    pub instance_storage: Vec<ContractStateEntry>,
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractStateEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossNetworkRestore {
    pub restored_at: String,
    pub target_network: String,
    pub target_contract_id: Option<String>,
    pub output_path: PathBuf,
    pub verified: bool,
}

fn backups_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("backups");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn replicas_dir(region: &str) -> Result<PathBuf> {
    let dir = backups_dir()?.join("replicas").join(region);
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn records_file() -> Result<PathBuf> {
    Ok(backups_dir()?.join("records.json"))
}

fn automation_file() -> Result<PathBuf> {
    Ok(backups_dir()?.join("automation.json"))
}

pub fn list_backups() -> Result<Vec<BackupRecord>> {
    let path = records_file()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn save_records(records: &[BackupRecord]) -> Result<()> {
    fs::write(records_file()?, serde_json::to_string_pretty(records)?)?;
    Ok(())
}

pub fn load_backup(id: &str) -> Result<BackupRecord> {
    list_backups()?
        .into_iter()
        .find(|r| r.id == id || r.id.starts_with(id))
        .ok_or_else(|| anyhow::anyhow!("No backup found with ID prefix '{}'", id))
}

fn zip_sources(sources: &[PathBuf]) -> Result<Vec<u8>> {
    let buf = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(buf);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for src in sources {
        if !src.exists() {
            anyhow::bail!("Backup source not found: {}", src.display());
        }
        add_path_to_zip(&mut zip, src, src, options)?;
    }

    let buf = zip.finish()?;
    Ok(buf.into_inner())
}

fn add_path_to_zip<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    base: &Path,
    path: &Path,
    options: zip::write::SimpleFileOptions,
) -> Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            add_path_to_zip(zip, base, &entry.path(), options)?;
        }
    } else {
        let rel = path
            .strip_prefix(base.parent().unwrap_or(base))
            .unwrap_or(path);
        zip.start_file(rel.to_string_lossy(), options)?;
        let mut f = fs::File::open(path)?;
        let mut contents = Vec::new();
        f.read_to_end(&mut contents)?;
        zip.write_all(&contents)?;
    }
    Ok(())
}

pub fn create_backup(
    sources: &[PathBuf],
    label: &str,
    encrypt: bool,
    passphrase: Option<&str>,
    region: &str,
) -> Result<BackupRecord> {
    if encrypt && passphrase.is_none() {
        anyhow::bail!("A passphrase is required to create an encrypted backup");
    }

    let archive_bytes = zip_sources(sources)?;
    let checksum = hex::encode(Sha256::digest(&archive_bytes));
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let (filename, write_bytes): (String, Vec<u8>) = if encrypt {
        let b64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &archive_bytes);
        let bundle = encrypt_secret(passphrase.unwrap(), &b64, None)?;
        (format!("{}.bak.enc", id), bundle.into_bytes())
    } else {
        (format!("{}.zip", id), archive_bytes.clone())
    };

    let archive_path = backups_dir()?.join(&filename);
    fs::write(&archive_path, &write_bytes)
        .with_context(|| format!("Failed to write backup archive {}", archive_path.display()))?;

    let mut record = BackupRecord {
        id,
        label: label.to_string(),
        created_at: now,
        sources: sources.iter().map(|p| p.display().to_string()).collect(),
        archive_path,
        encrypted: encrypt,
        checksum,
        size_bytes: archive_bytes.len() as u64,
        status: BackupStatus::Created,
        verified_at: None,
        replicated_regions: Vec::new(),
        contract_state: None,
    };

    replicate(&mut record, region)?;

    let mut records = list_backups()?;
    records.push(record.clone());
    save_records(&records)?;
    Ok(record)
}

pub fn create_contract_state_backup(
    inspect: &ContractInspectResult,
    source_network: &str,
    label: &str,
    encrypt: bool,
    passphrase: Option<&str>,
    region: &str,
) -> Result<BackupRecord> {
    let manifest = build_contract_manifest(inspect, source_network);
    let tmp = tempfile::tempdir()?;
    let manifest_path = tmp
        .path()
        .join(format!("{}-state-manifest.json", inspect.contract_id));
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

    let mut record = create_backup(
        std::slice::from_ref(&manifest_path),
        label,
        encrypt,
        passphrase,
        region,
    )?;
    let durable_manifest_path = backups_dir()?.join(format!("{}.contract-state.json", record.id));
    let mut durable_manifest = manifest;
    durable_manifest.backup_id = Some(record.id.clone());
    durable_manifest.checksum = contract_manifest_checksum(&durable_manifest);
    fs::write(
        &durable_manifest_path,
        serde_json::to_string_pretty(&durable_manifest)?,
    )?;

    record.contract_state = Some(ContractStateMetadata {
        contract_id: inspect.contract_id.clone(),
        source_network: source_network.to_string(),
        latest_ledger: inspect.latest_ledger,
        last_modified_ledger_seq: inspect.last_modified_ledger_seq,
        live_until_ledger_seq: inspect.live_until_ledger_seq,
        entry_count: inspect.instance_storage.len(),
        manifest_path: durable_manifest_path,
        restore_history: Vec::new(),
    });

    let mut records = list_backups()?;
    if let Some(existing) = records.iter_mut().find(|r| r.id == record.id) {
        *existing = record.clone();
    }
    save_records(&records)?;
    Ok(record)
}

fn build_contract_manifest(
    inspect: &ContractInspectResult,
    source_network: &str,
) -> ContractStateManifest {
    let mut manifest = ContractStateManifest {
        version: 1,
        backup_id: None,
        contract_id: inspect.contract_id.clone(),
        source_network: source_network.to_string(),
        captured_at: Utc::now().to_rfc3339(),
        latest_ledger: inspect.latest_ledger,
        executable: inspect.executable.clone(),
        wasm_hash: inspect.wasm_hash.clone(),
        storage_durability: inspect.storage_durability.clone(),
        last_modified_ledger_seq: inspect.last_modified_ledger_seq,
        live_until_ledger_seq: inspect.live_until_ledger_seq,
        instance_storage: inspect
            .instance_storage
            .iter()
            .map(|entry| ContractStateEntry {
                key: entry.key.clone(),
                value: entry.value.clone(),
            })
            .collect(),
        checksum: String::new(),
    };
    manifest.checksum = contract_manifest_checksum(&manifest);
    manifest
}

pub fn load_contract_manifest(id: &str) -> Result<ContractStateManifest> {
    let record = load_backup(id)?;
    let metadata = record
        .contract_state
        .ok_or_else(|| anyhow::anyhow!("Backup '{}' is not a contract state backup", record.id))?;
    let raw = fs::read_to_string(&metadata.manifest_path).with_context(|| {
        format!(
            "Failed to read contract state manifest {}",
            metadata.manifest_path.display()
        )
    })?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn verify_contract_state_backup(
    id: &str,
    passphrase: Option<&str>,
) -> Result<ContractStateManifest> {
    verify_backup(id, passphrase)?;
    let manifest = load_contract_manifest(id)?;
    let expected = contract_manifest_checksum(&manifest);
    if manifest.checksum != expected {
        anyhow::bail!(
            "Contract state manifest checksum mismatch for backup '{}'",
            id
        );
    }
    Ok(manifest)
}

pub fn restore_contract_state_backup(
    id: &str,
    target_network: &str,
    target_contract_id: Option<&str>,
    output_dir: &Path,
    passphrase: Option<&str>,
) -> Result<CrossNetworkRestore> {
    let manifest = verify_contract_state_backup(id, passphrase)?;
    fs::create_dir_all(output_dir)?;

    let output_path = output_dir.join(format!(
        "{}-to-{}-restore-plan.json",
        manifest.contract_id, target_network
    ));
    let restore_plan = serde_json::json!({
        "version": 1,
        "backup_id": id,
        "source_network": manifest.source_network,
        "source_contract_id": manifest.contract_id,
        "target_network": target_network,
        "target_contract_id": target_contract_id,
        "latest_ledger": manifest.latest_ledger,
        "wasm_hash": manifest.wasm_hash,
        "storage_durability": manifest.storage_durability,
        "entries": manifest.instance_storage,
        "generated_at": Utc::now().to_rfc3339(),
        "notes": [
            "Review this plan before replaying state on the target network.",
            "Soroban state writes must be applied by a migration contract or authorized admin invocation."
        ]
    });
    fs::write(&output_path, serde_json::to_string_pretty(&restore_plan)?)?;

    let restore = CrossNetworkRestore {
        restored_at: Utc::now().to_rfc3339(),
        target_network: target_network.to_string(),
        target_contract_id: target_contract_id.map(str::to_string),
        output_path,
        verified: true,
    };

    let mut records = list_backups()?;
    if let Some(record) = records
        .iter_mut()
        .find(|r| r.id == id || r.id.starts_with(id))
    {
        if let Some(metadata) = record.contract_state.as_mut() {
            metadata.restore_history.push(restore.clone());
        }
    }
    save_records(&records)?;
    Ok(restore)
}

fn contract_manifest_checksum(manifest: &ContractStateManifest) -> String {
    let mut clone = manifest.clone();
    clone.checksum.clear();
    let bytes = serde_json::to_vec(&clone).unwrap_or_default();
    hex::encode(Sha256::digest(bytes))
}

fn replicate(record: &mut BackupRecord, region: &str) -> Result<()> {
    let dest_dir = replicas_dir(region)?;
    let filename = record
        .archive_path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Invalid archive path"))?;
    let dest = dest_dir.join(filename);
    fs::copy(&record.archive_path, &dest)?;
    record.replicated_regions.push(region.to_string());
    Ok(())
}

/// Replicate an existing backup to an additional region.
pub fn replicate_existing(id: &str, region: &str) -> Result<BackupRecord> {
    let mut records = list_backups()?;
    let record = records
        .iter_mut()
        .find(|r| r.id == id || r.id.starts_with(id))
        .ok_or_else(|| anyhow::anyhow!("No backup found with ID prefix '{}'", id))?;
    if record.replicated_regions.iter().any(|r| r == region) {
        anyhow::bail!("Backup already replicated to region '{}'", region);
    }
    replicate(record, region)?;
    let updated = record.clone();
    save_records(&records)?;
    Ok(updated)
}

fn read_archive_bytes(record: &BackupRecord, passphrase: Option<&str>) -> Result<Vec<u8>> {
    let raw = fs::read(&record.archive_path)
        .with_context(|| format!("Failed to read archive {}", record.archive_path.display()))?;
    if record.encrypted {
        let passphrase = passphrase
            .ok_or_else(|| anyhow::anyhow!("A passphrase is required to read this backup"))?;
        let bundle = String::from_utf8(raw).context("Encrypted backup file is not valid UTF-8")?;
        let b64 = decrypt_secret(passphrase, &bundle)?;
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
            .context("Failed to decode decrypted backup payload")?;
        Ok(bytes)
    } else {
        Ok(raw)
    }
}

/// Verify a backup's archive integrity by recomputing its checksum.
pub fn verify_backup(id: &str, passphrase: Option<&str>) -> Result<BackupRecord> {
    let mut records = list_backups()?;
    let record = records
        .iter_mut()
        .find(|r| r.id == id || r.id.starts_with(id))
        .ok_or_else(|| anyhow::anyhow!("No backup found with ID prefix '{}'", id))?;

    let bytes = read_archive_bytes(record, passphrase)?;
    let checksum = hex::encode(Sha256::digest(&bytes));

    record.status = if checksum == record.checksum {
        BackupStatus::Verified
    } else {
        BackupStatus::VerificationFailed
    };
    record.verified_at = Some(Utc::now().to_rfc3339());
    let updated = record.clone();
    save_records(&records)?;

    if updated.status == BackupStatus::VerificationFailed {
        anyhow::bail!("Backup '{}' failed integrity verification", updated.id);
    }
    Ok(updated)
}

/// Restore a backup's archive contents into `dest_dir`.
pub fn restore_backup(id: &str, dest_dir: &Path, passphrase: Option<&str>) -> Result<Vec<String>> {
    let record = load_backup(id)?;
    let bytes = read_archive_bytes(&record, passphrase)?;
    extract_zip(&bytes, dest_dir)
}

fn extract_zip(bytes: &[u8], dest_dir: &Path) -> Result<Vec<String>> {
    fs::create_dir_all(dest_dir)?;
    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader)?;
    let mut extracted = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => dest_dir.join(path),
            None => continue,
        };
        if file.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
            extracted.push(outpath.display().to_string());
        }
    }
    Ok(extracted)
}

/// Restore a backup into a temporary directory and confirm the extracted files exist,
/// without disturbing the real environment. Used for periodic recovery testing.
pub fn test_restore(id: &str, passphrase: Option<&str>) -> Result<usize> {
    let tmp = tempfile::tempdir()?;
    let extracted = restore_backup(id, tmp.path(), passphrase)?;
    for path in &extracted {
        if !Path::new(path).exists() {
            anyhow::bail!(
                "Recovery test failed: expected file '{}' missing after restore",
                path
            );
        }
    }
    Ok(extracted.len())
}

/// Find the most recent backup for `label` created at or before `at`.
pub fn find_point_in_time(label: &str, at: DateTime<Utc>) -> Result<BackupRecord> {
    let mut candidates: Vec<BackupRecord> = list_backups()?
        .into_iter()
        .filter(|r| r.label == label)
        .filter(|r| {
            DateTime::parse_from_rfc3339(&r.created_at)
                .map(|dt| dt.with_timezone(&Utc) <= at)
                .unwrap_or(false)
        })
        .collect();
    candidates.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No backup found for label '{}' at or before {}", label, at))
}

pub fn list_automation() -> Result<Vec<AutomationConfig>> {
    let path = automation_file()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn save_automation(configs: &[AutomationConfig]) -> Result<()> {
    fs::write(automation_file()?, serde_json::to_string_pretty(configs)?)?;
    Ok(())
}

pub fn configure_automation(
    label: &str,
    sources: Vec<PathBuf>,
    interval_hours: u64,
    encrypt: bool,
    region: &str,
) -> Result<AutomationConfig> {
    let mut configs = list_automation()?;
    configs.retain(|c| c.label != label);
    let cfg = AutomationConfig {
        label: label.to_string(),
        sources: sources.iter().map(|p| p.display().to_string()).collect(),
        interval_hours,
        encrypt,
        region: region.to_string(),
        last_run: None,
        contract: None,
    };
    configs.push(cfg.clone());
    save_automation(&configs)?;
    Ok(cfg)
}

pub fn configure_contract_automation(
    label: &str,
    contract_id: &str,
    network: &str,
    interval_hours: u64,
    encrypt: bool,
    region: &str,
) -> Result<AutomationConfig> {
    let mut configs = list_automation()?;
    configs.retain(|c| c.label != label);
    let cfg = AutomationConfig {
        label: label.to_string(),
        sources: Vec::new(),
        interval_hours,
        encrypt,
        region: region.to_string(),
        last_run: None,
        contract: Some(ContractScheduleConfig {
            contract_id: contract_id.to_string(),
            network: network.to_string(),
        }),
    };
    configs.push(cfg.clone());
    save_automation(&configs)?;
    Ok(cfg)
}

pub fn mark_automation_ran(label: &str, ran_at: DateTime<Utc>) -> Result<()> {
    let mut configs = list_automation()?;
    if let Some(cfg) = configs.iter_mut().find(|cfg| cfg.label == label) {
        cfg.last_run = Some(ran_at.to_rfc3339());
    }
    save_automation(&configs)?;
    Ok(())
}

/// Run any automated backup configs that are due. Returns labels backed up.
/// Encrypted automation configs require `passphrase` to be supplied.
pub fn run_automation(passphrase: Option<&str>) -> Result<Vec<String>> {
    let mut configs = list_automation()?;
    let mut ran = Vec::new();
    let now = Utc::now();

    for cfg in configs.iter_mut() {
        if cfg.contract.is_some() {
            continue;
        }
        let due = match &cfg.last_run {
            None => true,
            Some(last) => DateTime::parse_from_rfc3339(last)
                .map(|dt| {
                    now.signed_duration_since(dt.with_timezone(&Utc))
                        .num_hours() as u64
                        >= cfg.interval_hours
                })
                .unwrap_or(true),
        };
        if !due {
            continue;
        }
        let sources: Vec<PathBuf> = cfg.sources.iter().map(PathBuf::from).collect();
        create_backup(&sources, &cfg.label, cfg.encrypt, passphrase, &cfg.region)?;
        cfg.last_run = Some(now.to_rfc3339());
        ran.push(cfg.label.clone());
    }

    save_automation(&configs)?;
    Ok(ran)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_source_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn zip_and_extract_roundtrip() {
        let src_dir = tempdir().unwrap();
        let f1 = make_source_file(src_dir.path(), "a.wasm", "hello-wasm");

        let bytes = zip_sources(std::slice::from_ref(&f1)).unwrap();
        let out_dir = tempdir().unwrap();
        let extracted = extract_zip(&bytes, out_dir.path()).unwrap();
        assert_eq!(extracted.len(), 1);
        let contents = fs::read_to_string(&extracted[0]).unwrap();
        assert_eq!(contents, "hello-wasm");
    }

    #[test]
    fn checksum_changes_when_contents_change() {
        let dir = tempdir().unwrap();
        let f1 = make_source_file(dir.path(), "a.txt", "v1");
        let b1 = zip_sources(std::slice::from_ref(&f1)).unwrap();
        let f2 = make_source_file(dir.path(), "a.txt", "v2");
        let b2 = zip_sources(&[f2]).unwrap();
        assert_ne!(
            hex::encode(Sha256::digest(&b1)),
            hex::encode(Sha256::digest(&b2))
        );
    }
}
