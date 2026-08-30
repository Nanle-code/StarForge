//! Contract Storage Migration Tool
//!
//! Provides a self-contained engine for migrating Soroban contract storage
//! snapshots between contract versions. The workflow is snapshot-based:
//!
//!   1. Export contract storage to a JSON snapshot (e.g. via
//!      `starforge inspect storage --json > snapshot.json`, or hand-rolled).
//!   2. Author a migration rules file describing how fields should change
//!      between versions (`starforge migrate init`).
//!   3. Dry-run the rules against sample data to catch mistakes
//!      (`starforge migrate test`).
//!   4. Run the migration to produce a transformed snapshot, with an
//!      automatic backup of the original for safety (`starforge migrate run`).
//!   5. Validate the migrated snapshot against the rules' expected schema
//!      (`starforge migrate validate`).
//!   6. Roll back to the pre-migration snapshot if anything goes wrong
//!      (`starforge migrate rollback`).
//!
//! All migrations are recorded in a local history log so they can be
//! audited or reversed later, mirroring the `upgrade` command's proposal
//! history model.

use crate::utils::{config, migration_testing, print as p, state_diff, state_transition};
use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Args, Subcommand};
use colored::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

// ── CLI definition ───────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum MigrateCommands {
    /// Create or capture a storage snapshot with metadata
    Snapshot(SnapshotArgs),
    /// Restore a storage snapshot from file or backup
    Restore(RestoreArgs),
    /// Generate a starter migration rules file
    Init(InitArgs),
    /// Generate a migration script from state diff or template
    Generate(GenerateArgs),
    /// Run a migration against a storage snapshot
    Run(RunArgs),
    /// Validate a (migrated) snapshot against a rules file and transition invariants
    Validate(ValidateArgs),
    /// Dry-run migration rules against sample data using testing framework
    Test(TestArgs),
    /// Restore a snapshot from an automatic pre-migration backup
    Rollback(RollbackArgs),
    /// Show migration history
    History(HistoryArgs),
    /// Print a migration usage guide
    Docs(DocsArgs),
    /// Diff two state snapshots to show what changed between versions
    Diff(DiffArgs),
}

#[derive(Args)]
pub struct SnapshotArgs {
    /// Contract ID for the snapshot
    #[arg(
        long,
        default_value = "C0000000000000000000000000000000000000000000000000000000"
    )]
    pub contract_id: String,
    /// Version label for the snapshot
    #[arg(long, default_value = "v1")]
    pub version: String,
    /// Output file path for the snapshot JSON
    #[arg(long, default_value = "snapshot.json")]
    pub output: PathBuf,
    /// Optional key-value entries as JSON string (e.g. '{"balance":"100"}')
    #[arg(long)]
    pub entries: Option<String>,
}

#[derive(Args)]
pub struct RestoreArgs {
    /// Source snapshot or backup file path to restore from
    #[arg(long)]
    pub source: PathBuf,
    /// Target destination file path
    #[arg(long)]
    pub destination: PathBuf,
    /// Skip confirmation prompt
    #[arg(long, default_value = "false")]
    pub yes: bool,
}

#[derive(Args)]
pub struct InitArgs {
    /// Where to write the generated rules template
    #[arg(long, default_value = "migration-rules.json")]
    pub output: PathBuf,
    /// Version label the migration starts from
    #[arg(long, default_value = "v1")]
    pub from_version: String,
    /// Version label the migration targets
    #[arg(long, default_value = "v2")]
    pub to_version: String,
}

#[derive(Args)]
pub struct GenerateArgs {
    /// Path to older state snapshot (JSON)
    #[arg(long)]
    pub before: PathBuf,
    /// Path to newer state snapshot (JSON)
    #[arg(long)]
    pub after: PathBuf,
    /// Where to save the generated migration rules JSON
    #[arg(long, default_value = "migration-rules.json")]
    pub output: PathBuf,
    /// Version label the migration starts from
    #[arg(long, default_value = "v1")]
    pub from_version: String,
    /// Version label the migration targets
    #[arg(long, default_value = "v2")]
    pub to_version: String,
}

#[derive(Args)]
pub struct RunArgs {
    /// Contract ID the snapshot belongs to (for record-keeping)
    #[arg(long)]
    pub contract_id: String,
    /// Path to the source storage snapshot (JSON)
    #[arg(long)]
    pub snapshot: PathBuf,
    /// Path to the migration rules file (JSON)
    #[arg(long)]
    pub rules: PathBuf,
    /// Where to write the migrated snapshot
    #[arg(long)]
    pub output: PathBuf,
    /// Network the contract is associated with
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Skip the confirmation prompt
    #[arg(long, default_value = "false")]
    pub yes: bool,
}

#[derive(Args)]
pub struct ValidateArgs {
    /// Path to the snapshot to validate
    #[arg(long)]
    pub snapshot: PathBuf,
    /// Path to the migration rules file the snapshot should conform to
    #[arg(long)]
    pub rules: PathBuf,
    /// Optional path to older pre-migration snapshot for transition validation
    #[arg(long)]
    pub before: Option<PathBuf>,
}

#[derive(Args)]
pub struct TestArgs {
    /// Path to a sample snapshot used for the dry run
    #[arg(long)]
    pub sample: PathBuf,
    /// Path to the migration rules file to test
    #[arg(long)]
    pub rules: PathBuf,
    /// Optional path to expected output snapshot file for scenario assertion
    #[arg(long)]
    pub expected: Option<PathBuf>,
}

#[derive(Args)]
pub struct RollbackArgs {
    /// Migration ID to roll back (see `starforge migrate history`)
    #[arg(long)]
    pub migration_id: String,
    /// Where to restore the pre-migration snapshot to
    #[arg(long)]
    pub output: PathBuf,
    /// Skip the confirmation prompt
    #[arg(long, default_value = "false")]
    pub yes: bool,
}

#[derive(Args)]
pub struct HistoryArgs {
    /// Filter by contract ID (optional)
    #[arg(long)]
    pub contract_id: Option<String>,
}

#[derive(Args)]
pub struct DocsArgs {
    /// Optional path to write the documentation to (prints to stdout otherwise)
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Args)]
pub struct DiffArgs {
    /// Path to the first (older) state snapshot (JSON)
    #[arg(long)]
    pub before: PathBuf,
    /// Path to the second (newer) state snapshot (JSON)
    #[arg(long)]
    pub after: PathBuf,
    /// Output format: console (default), json
    #[arg(long, default_value = "console")]
    pub format: String,
    /// Version label for the before snapshot
    #[arg(long, default_value = "v1")]
    pub from_version: String,
    /// Version label for the after snapshot
    #[arg(long, default_value = "v2")]
    pub to_version: String,
    /// Generate migration rules from the diff and save to file
    #[arg(long)]
    pub generate_rules: Option<PathBuf>,
    /// Save diff report to file
    #[arg(long)]
    pub output: Option<PathBuf>,
}

// ── Data structures ──────────────────────────────────────────────────────────

/// A single storage key/value transformation step, applied in order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TransformOp {
    /// Rename a storage key, preserving its value.
    RenameKey { from: String, to: String },
    /// Insert a new key with a default value if it does not already exist.
    AddField { key: String, default: Value },
    /// Remove a key entirely.
    RemoveField { key: String },
    /// Coerce a key's value to a new primitive type (string/number/bool).
    CastType { key: String, to_type: String },
    /// Replace one literal value with another for a given key (e.g. enum remaps).
    RemapValue {
        key: String,
        from_value: Value,
        to_value: Value,
    },
}

/// Migration rules describing how to move storage from one schema version
/// to another, plus the expected post-migration schema used for validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationRules {
    pub from_version: String,
    pub to_version: String,
    #[serde(default)]
    pub ops: Vec<TransformOp>,
    /// Keys that must be present after migration.
    #[serde(default)]
    pub required_keys: Vec<String>,
    /// Keys that are expected to disappear after migration (e.g. deprecated fields).
    #[serde(default)]
    pub forbidden_keys: Vec<String>,
}

/// A storage snapshot: a flat map of storage key -> JSON value, plus metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSnapshot {
    #[serde(default)]
    pub contract_id: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub captured_at: Option<String>,
    pub entries: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MigrationStatus {
    Completed,
    CompletedWithWarnings,
    Failed,
    RolledBack,
}

impl std::fmt::Display for MigrationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationStatus::Completed => write!(f, "completed"),
            MigrationStatus::CompletedWithWarnings => write!(f, "completed (warnings)"),
            MigrationStatus::Failed => write!(f, "failed"),
            MigrationStatus::RolledBack => write!(f, "rolled_back"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationRecord {
    pub id: String,
    pub contract_id: String,
    pub network: String,
    pub from_version: String,
    pub to_version: String,
    pub entries_migrated: usize,
    pub entries_unchanged: usize,
    pub warnings: Vec<String>,
    pub status: MigrationStatus,
    pub source_checksum: String,
    pub output_checksum: String,
    pub backup_path: String,
    pub output_path: String,
    pub timestamp: String,
}

/// Result of applying a set of transform ops to a snapshot.
pub struct MigrationReport {
    pub snapshot: StorageSnapshot,
    pub entries_migrated: usize,
    pub entries_unchanged: usize,
    pub warnings: Vec<String>,
}

/// Result of validating a snapshot against a rules file's expected schema.
#[derive(Debug, Default)]
pub struct ValidationReport {
    pub missing_required: Vec<String>,
    pub present_forbidden: Vec<String>,
    pub type_issues: Vec<String>,
}

impl ValidationReport {
    pub fn is_ok(&self) -> bool {
        self.missing_required.is_empty()
            && self.present_forbidden.is_empty()
            && self.type_issues.is_empty()
    }
}

// ── Storage helpers (local migration history + backups) ─────────────────────

fn migrate_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("migrations");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn backups_dir() -> Result<PathBuf> {
    let dir = migrate_dir()?.join("backups");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn history_path() -> Result<PathBuf> {
    Ok(migrate_dir()?.join("history.json"))
}

fn load_history() -> Result<Vec<MigrationRecord>> {
    let path = history_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

fn save_history(history: &[MigrationRecord]) -> Result<()> {
    fs::write(history_path()?, serde_json::to_string_pretty(history)?)?;
    Ok(())
}

// ── Snapshot I/O ──────────────────────────────────────────────────────────────

pub fn load_snapshot(path: &PathBuf) -> Result<StorageSnapshot> {
    let data = fs::read_to_string(path)
        .with_context(|| format!("Failed to read snapshot: {}", path.display()))?;
    serde_json::from_str(&data)
        .with_context(|| format!("Failed to parse snapshot JSON: {}", path.display()))
}

pub fn save_snapshot(snapshot: &StorageSnapshot, path: &PathBuf) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, serde_json::to_string_pretty(snapshot)?)?;
    Ok(())
}

pub fn load_rules(path: &PathBuf) -> Result<MigrationRules> {
    let data = fs::read_to_string(path)
        .with_context(|| format!("Failed to read rules file: {}", path.display()))?;
    serde_json::from_str(&data)
        .with_context(|| format!("Failed to parse rules JSON: {}", path.display()))
}

/// Compute a stable SHA-256 checksum of a snapshot's entries, independent of
/// key ordering, so it can be used to detect drift or verify integrity.
pub fn snapshot_checksum(snapshot: &StorageSnapshot) -> String {
    let mut hasher = Sha256::new();
    // BTreeMap iterates in sorted key order, so this is deterministic.
    for (k, v) in &snapshot.entries {
        hasher.update(k.as_bytes());
        hasher.update(b"=");
        hasher.update(v.to_string().as_bytes());
        hasher.update(b";");
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

// ── Migration engine ──────────────────────────────────────────────────────────

/// Apply a single transform op to the working entry map. Returns `true` if
/// the op changed something, and may push a human-readable warning.
fn apply_op(
    entries: &mut BTreeMap<String, Value>,
    op: &TransformOp,
    warnings: &mut Vec<String>,
) -> bool {
    match op {
        TransformOp::RenameKey { from, to } => {
            if let Some(val) = entries.remove(from) {
                if entries.contains_key(to) {
                    warnings.push(format!(
                        "RenameKey: target key '{}' already existed and was overwritten",
                        to
                    ));
                }
                entries.insert(to.clone(), val);
                true
            } else {
                warnings.push(format!(
                    "RenameKey: source key '{}' not found, skipped",
                    from
                ));
                false
            }
        }
        TransformOp::AddField { key, default } => {
            if entries.contains_key(key) {
                false
            } else {
                entries.insert(key.clone(), default.clone());
                true
            }
        }
        TransformOp::RemoveField { key } => entries.remove(key).is_some(),
        TransformOp::CastType { key, to_type } => match entries.get(key) {
            Some(val) => match cast_value(val, to_type) {
                Some(new_val) => {
                    let changed = &new_val != val;
                    entries.insert(key.clone(), new_val);
                    changed
                }
                None => {
                    warnings.push(format!(
                        "CastType: could not cast key '{}' (value {}) to '{}'",
                        key, val, to_type
                    ));
                    false
                }
            },
            None => {
                warnings.push(format!("CastType: key '{}' not found, skipped", key));
                false
            }
        },
        TransformOp::RemapValue {
            key,
            from_value,
            to_value,
        } => match entries.get(key) {
            Some(val) if val == from_value => {
                entries.insert(key.clone(), to_value.clone());
                true
            }
            Some(_) => false,
            None => {
                warnings.push(format!("RemapValue: key '{}' not found, skipped", key));
                false
            }
        },
    }
}

fn cast_value(val: &Value, to_type: &str) -> Option<Value> {
    match to_type {
        "string" => Some(Value::String(match val {
            Value::String(s) => s.clone(),
            other => other.to_string().trim_matches('"').to_string(),
        })),
        "number" => match val {
            Value::Number(_) => Some(val.clone()),
            Value::String(s) => s
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number),
            Value::Bool(b) => Some(Value::Number((*b as u64).into())),
            _ => None,
        },
        "bool" => match val {
            Value::Bool(_) => Some(val.clone()),
            Value::String(s) => match s.to_lowercase().as_str() {
                "true" | "1" => Some(Value::Bool(true)),
                "false" | "0" => Some(Value::Bool(false)),
                _ => None,
            },
            Value::Number(n) => Some(Value::Bool(n.as_f64().unwrap_or(0.0) != 0.0)),
            _ => None,
        },
        _ => None,
    }
}

/// Apply a full rule set to a snapshot, returning the transformed snapshot
/// alongside a report of what changed. This is the core, side-effect-free
/// migration engine — used by both `run` (writes to disk) and `test`
/// (dry-run, no writes).
pub fn apply_rules(snapshot: &StorageSnapshot, rules: &MigrationRules) -> MigrationReport {
    let mut entries = snapshot.entries.clone();
    let before_keys: BTreeMap<String, Value> = entries.clone();
    let mut warnings = Vec::new();
    let mut migrated = 0usize;

    for op in &rules.ops {
        if apply_op(&mut entries, op, &mut warnings) {
            migrated += 1;
        }
    }

    let unchanged = entries
        .iter()
        .filter(|(k, v)| before_keys.get(*k) == Some(*v))
        .count();

    let new_snapshot = StorageSnapshot {
        contract_id: snapshot.contract_id.clone(),
        version: Some(rules.to_version.clone()),
        captured_at: Some(Utc::now().to_rfc3339()),
        entries,
    };

    MigrationReport {
        snapshot: new_snapshot,
        entries_migrated: migrated,
        entries_unchanged: unchanged,
        warnings,
    }
}

/// Validate a snapshot's entries against a rules file's `required_keys` /
/// `forbidden_keys` lists, plus best-effort type sanity checks for any
/// `CastType` ops, to confirm the cast actually "stuck".
pub fn validate_snapshot(snapshot: &StorageSnapshot, rules: &MigrationRules) -> ValidationReport {
    let mut report = ValidationReport::default();

    for key in &rules.required_keys {
        if !snapshot.entries.contains_key(key) {
            report.missing_required.push(key.clone());
        }
    }
    for key in &rules.forbidden_keys {
        if snapshot.entries.contains_key(key) {
            report.present_forbidden.push(key.clone());
        }
    }
    for op in &rules.ops {
        if let TransformOp::CastType { key, to_type } = op {
            if let Some(val) = snapshot.entries.get(key) {
                let matches = match to_type.as_str() {
                    "string" => val.is_string(),
                    "number" => val.is_number(),
                    "bool" => val.is_boolean(),
                    _ => true,
                };
                if !matches {
                    report.type_issues.push(format!(
                        "key '{}' expected type '{}' but found {}",
                        key, to_type, val
                    ));
                }
            }
        }
    }

    report
}

// ── Command handlers ──────────────────────────────────────────────────────────

// ── Command handlers ──────────────────────────────────────────────────────────

pub fn handle(cmd: MigrateCommands) -> Result<()> {
    match cmd {
        MigrateCommands::Snapshot(args) => handle_snapshot(args),
        MigrateCommands::Restore(args) => handle_restore(args),
        MigrateCommands::Init(args) => handle_init(args),
        MigrateCommands::Generate(args) => handle_generate(args),
        MigrateCommands::Run(args) => handle_run(args),
        MigrateCommands::Validate(args) => handle_validate(args),
        MigrateCommands::Test(args) => handle_test(args),
        MigrateCommands::Rollback(args) => handle_rollback(args),
        MigrateCommands::History(args) => handle_history(args),
        MigrateCommands::Docs(args) => handle_docs(args),
        MigrateCommands::Diff(args) => handle_diff(args),
    }
}

fn handle_snapshot(args: SnapshotArgs) -> Result<()> {
    p::header("Create Storage Snapshot");

    let entries: BTreeMap<String, Value> = if let Some(ref raw_json) = args.entries {
        serde_json::from_str(raw_json)
            .with_context(|| format!("Failed to parse JSON entries string: {}", raw_json))?
    } else {
        let mut default_entries = BTreeMap::new();
        default_entries.insert(
            "schema_version".to_string(),
            Value::String(args.version.clone()),
        );
        default_entries.insert("balance".to_string(), Value::String("1000".to_string()));
        default_entries.insert(
            "owner".to_string(),
            Value::String("G0000000000000000000000000000000000000000000000000000000".to_string()),
        );
        default_entries
    };

    let snapshot = StorageSnapshot {
        contract_id: Some(args.contract_id.clone()),
        version: Some(args.version.clone()),
        captured_at: Some(Utc::now().to_rfc3339()),
        entries,
    };

    save_snapshot(&snapshot, &args.output)?;

    p::success(&format!(
        "Storage snapshot written to {}",
        args.output.display()
    ));
    p::kv("Contract ID", &args.contract_id);
    p::kv("Version", &args.version);
    p::kv("Entries count", &snapshot.entries.len().to_string());
    p::kv("Checksum", &snapshot_checksum(&snapshot));
    Ok(())
}

fn handle_restore(args: RestoreArgs) -> Result<()> {
    p::header("Restore Storage Snapshot");

    let snapshot = load_snapshot(&args.source)?;
    p::kv("Source", &args.source.display().to_string());
    p::kv("Entries", &snapshot.entries.len().to_string());
    p::kv("Destination", &args.destination.display().to_string());

    if !args.yes {
        println!();
        print!(
            "  Restore snapshot to {}? [y/N] ",
            args.destination.display()
        );
        use std::io::BufRead;
        let line = std::io::stdin()
            .lock()
            .lines()
            .next()
            .unwrap_or(Ok(String::new()))?;
        if !matches!(line.trim().to_lowercase().as_str(), "y" | "yes") {
            p::info("Restore cancelled.");
            return Ok(());
        }
    }

    save_snapshot(&snapshot, &args.destination)?;
    p::success(&format!(
        "Snapshot restored to {}",
        args.destination.display()
    ));
    Ok(())
}

fn handle_init(args: InitArgs) -> Result<()> {
    p::header("Initialize Migration Rules");

    if args.output.exists() {
        anyhow::bail!(
            "File already exists: {}. Choose a different --output path.",
            args.output.display()
        );
    }

    let template = MigrationRules {
        from_version: args.from_version.clone(),
        to_version: args.to_version.clone(),
        ops: vec![
            TransformOp::RenameKey {
                from: "old_field_name".to_string(),
                to: "new_field_name".to_string(),
            },
            TransformOp::AddField {
                key: "schema_version".to_string(),
                default: Value::String(args.to_version.clone()),
            },
            TransformOp::CastType {
                key: "balance".to_string(),
                to_type: "number".to_string(),
            },
            TransformOp::RemoveField {
                key: "deprecated_field".to_string(),
            },
        ],
        required_keys: vec!["schema_version".to_string()],
        forbidden_keys: vec!["deprecated_field".to_string()],
    };

    fs::write(&args.output, serde_json::to_string_pretty(&template)?)?;

    p::success(&format!(
        "Wrote migration rules template to {}",
        args.output.display()
    ));
    p::info("Edit the `ops` array to describe your real schema changes, then:");
    println!(
        "  {}",
        format!(
            "starforge migrate test --sample <snapshot.json> --rules {}",
            args.output.display()
        )
        .cyan()
    );
    Ok(())
}

fn handle_generate(args: GenerateArgs) -> Result<()> {
    p::header("Generate Migration Rules from Diff");

    let before_snapshot = load_snapshot(&args.before)?;
    let after_snapshot = load_snapshot(&args.after)?;

    let report = state_diff::diff_snapshots(
        &before_snapshot.entries,
        &after_snapshot.entries,
        Some(args.from_version.clone()),
        Some(args.to_version.clone()),
    );

    let diff_rules = state_diff::generate_migration_rules_from_diff(&report);
    let ops: Vec<TransformOp> = diff_rules
        .iter()
        .map(|r| match r.op.as_str() {
            "rename_key" => TransformOp::RenameKey {
                from: r.from_key.clone().unwrap_or_default(),
                to: r.key.clone(),
            },
            "remove_field" => TransformOp::RemoveField { key: r.key.clone() },
            "add_field" => TransformOp::AddField {
                key: r.key.clone(),
                default: r.default_value.clone().unwrap_or(Value::Null),
            },
            "cast_type" => TransformOp::CastType {
                key: r.key.clone(),
                to_type: r
                    .target_type
                    .clone()
                    .unwrap_or_else(|| "string".to_string()),
            },
            _ => TransformOp::RemoveField { key: r.key.clone() },
        })
        .collect();

    let migration_rules = MigrationRules {
        from_version: args.from_version.clone(),
        to_version: args.to_version.clone(),
        ops,
        required_keys: after_snapshot.entries.keys().cloned().collect(),
        forbidden_keys: report.removed.iter().map(|e| e.key.clone()).collect(),
    };

    fs::write(
        &args.output,
        serde_json::to_string_pretty(&migration_rules)?,
    )?;

    p::success(&format!(
        "Wrote generated migration rules to {}",
        args.output.display()
    ));
    p::kv("From version", &args.from_version);
    p::kv("To version", &args.to_version);
    p::kv("Operations count", &migration_rules.ops.len().to_string());
    Ok(())
}

fn handle_run(args: RunArgs) -> Result<()> {
    p::header("Run Storage Migration");
    config::validate_contract_id(&args.contract_id)?;
    config::validate_network(&args.network)?;

    p::step(1, 5, "Loading source snapshot and rules…");
    let source = load_snapshot(&args.snapshot)?;
    let rules = load_rules(&args.rules)?;
    let source_checksum = snapshot_checksum(&source);
    p::kv("Entries in source", &source.entries.len().to_string());
    p::kv("From version", &rules.from_version);
    p::kv("To version", &rules.to_version);

    if let Some(v) = &source.version {
        if v != &rules.from_version {
            p::warn(&format!(
                "Snapshot version '{}' does not match rules' from_version '{}'",
                v, rules.from_version
            ));
        }
    }

    p::step(2, 5, "Backing up source snapshot…");
    let migration_id = format!("mig-{}", &source_checksum[..12]);
    let backup_path = backups_dir()?.join(format!("{}.json", migration_id));
    save_snapshot(&source, &backup_path)?;
    p::kv_accent("Backup", &backup_path.display().to_string());

    if !args.yes {
        println!();
        print!(
            "  Apply {} transformation step(s) and write output to {}? [y/N] ",
            rules.ops.len(),
            args.output.display()
        );
        use std::io::BufRead;
        let line = std::io::stdin()
            .lock()
            .lines()
            .next()
            .unwrap_or(Ok(String::new()))?;
        if !matches!(line.trim().to_lowercase().as_str(), "y" | "yes") {
            p::info("Migration cancelled.");
            return Ok(());
        }
    }

    p::step(3, 5, "Applying transformation rules…");
    let report = apply_rules(&source, &rules);
    for w in &report.warnings {
        p::warn(w);
    }

    p::step(4, 5, "Validating migrated data integrity…");
    let validation = validate_snapshot(&report.snapshot, &rules);
    let status = if !validation.is_ok() {
        MigrationStatus::Failed
    } else if !report.warnings.is_empty() {
        MigrationStatus::CompletedWithWarnings
    } else {
        MigrationStatus::Completed
    };

    if !validation.is_ok() {
        for k in &validation.missing_required {
            p::warn(&format!("Missing required key after migration: {}", k));
        }
        for k in &validation.present_forbidden {
            p::warn(&format!(
                "Forbidden key still present after migration: {}",
                k
            ));
        }
        for issue in &validation.type_issues {
            p::warn(issue);
        }
    }

    p::step(5, 5, "Writing migrated snapshot and recording history…");
    save_snapshot(&report.snapshot, &args.output)?;
    let output_checksum = snapshot_checksum(&report.snapshot);

    let mut history = load_history()?;
    history.push(MigrationRecord {
        id: migration_id.clone(),
        contract_id: args.contract_id.clone(),
        network: args.network.clone(),
        from_version: rules.from_version.clone(),
        to_version: rules.to_version.clone(),
        entries_migrated: report.entries_migrated,
        entries_unchanged: report.entries_unchanged,
        warnings: report.warnings.clone(),
        status: status.clone(),
        source_checksum,
        output_checksum: output_checksum.clone(),
        backup_path: backup_path.display().to_string(),
        output_path: args.output.display().to_string(),
        timestamp: Utc::now().to_rfc3339(),
    });
    save_history(&history)?;

    println!();
    p::separator();
    p::kv_accent("Migration ID", &migration_id);
    p::kv("Status", &status.to_string());
    p::kv("Entries migrated", &report.entries_migrated.to_string());
    p::kv("Entries unchanged", &report.entries_unchanged.to_string());
    p::kv("Output", &args.output.display().to_string());
    p::kv("Output checksum", &output_checksum);
    println!();
    if status == MigrationStatus::Failed {
        p::warn("Migration completed but failed validation. Review the issues above.");
        p::info(&format!(
            "Roll back with: starforge migrate rollback --migration-id {} --output {}",
            migration_id,
            args.snapshot.display()
        ));
    } else {
        p::success("Migration complete.");
        p::info(&format!(
            "If anything looks wrong: starforge migrate rollback --migration-id {} --output {}",
            migration_id,
            args.snapshot.display()
        ));
    }
    p::separator();
    Ok(())
}

fn handle_validate(args: ValidateArgs) -> Result<()> {
    p::header("Validate Migrated Snapshot");

    let snapshot = load_snapshot(&args.snapshot)?;
    let rules = load_rules(&args.rules)?;
    let report = validate_snapshot(&snapshot, &rules);

    p::kv("Snapshot", &args.snapshot.display().to_string());
    p::kv("Entries", &snapshot.entries.len().to_string());
    println!();

    if let Some(ref before_path) = args.before {
        let before_snapshot = load_snapshot(before_path)?;
        let mut inv_rules = Vec::new();

        for key in &rules.required_keys {
            inv_rules
                .push(state_transition::TransitionInvariantRule::RequiredKey { key: key.clone() });
        }
        for key in &rules.forbidden_keys {
            inv_rules
                .push(state_transition::TransitionInvariantRule::ForbiddenKey { key: key.clone() });
        }

        let options = state_transition::TransitionValidationOptions {
            from_version: Some(rules.from_version.clone()),
            to_version: Some(rules.to_version.clone()),
            rules: inv_rules,
            check_checksum_integrity: true,
        };

        let transition_report = state_transition::validate_state_transition(
            &before_snapshot.entries,
            &snapshot.entries,
            &options,
        );

        if !transition_report.valid {
            for err in &transition_report.errors {
                p::warn(&format!("[Transition Error] {}: {}", err.key, err.message));
            }
        } else {
            p::success("State transition invariant check passed.");
        }
    }

    if report.is_ok() {
        p::success("Snapshot satisfies all rules: required keys present, forbidden keys absent, types match.");
        return Ok(());
    }

    for k in &report.missing_required {
        p::warn(&format!("Missing required key: {}", k));
    }
    for k in &report.present_forbidden {
        p::warn(&format!("Forbidden key still present: {}", k));
    }
    for issue in &report.type_issues {
        p::warn(issue);
    }
    anyhow::bail!(
        "Validation failed: {} missing, {} forbidden present, {} type issue(s)",
        report.missing_required.len(),
        report.present_forbidden.len(),
        report.type_issues.len()
    );
}

fn handle_test(args: TestArgs) -> Result<()> {
    p::header("Migration Dry Run & Framework Test");

    let sample = load_snapshot(&args.sample)?;
    let rules = load_rules(&args.rules)?;
    let before_keys: Vec<String> = sample.entries.keys().cloned().collect();

    let report = apply_rules(&sample, &rules);
    let after_keys: Vec<String> = report.snapshot.entries.keys().cloned().collect();

    if let Some(ref expected_path) = args.expected {
        let expected_snapshot = load_snapshot(expected_path)?;
        let tc = migration_testing::MigrationTestCase {
            name: "cli_dry_run_test".to_string(),
            description: Some("Dry run test scenario".to_string()),
            initial_state: sample.entries.clone(),
            expected_state: Some(expected_snapshot.entries.clone()),
            invariant_rules: vec![],
        };

        let test_res = migration_testing::MigrationTestRunner::run_test_case(&tc, |init| {
            let rep = apply_rules(
                &StorageSnapshot {
                    contract_id: sample.contract_id.clone(),
                    version: sample.version.clone(),
                    captured_at: None,
                    entries: init.clone(),
                },
                &rules,
            );
            (rep.snapshot.entries, rep.warnings)
        });

        if test_res.passed {
            p::success("Migration framework test scenario PASSED expected assertions.");
        } else {
            p::warn("Migration framework test scenario FAILED expected assertions:");
            for err in &test_res.errors {
                p::warn(err);
            }
        }
    }

    let added: Vec<_> = after_keys
        .iter()
        .filter(|k| !before_keys.contains(k))
        .collect();
    let removed: Vec<_> = before_keys
        .iter()
        .filter(|k| !after_keys.contains(k))
        .collect();

    p::kv("Sample entries (before)", &before_keys.len().to_string());
    p::kv("Sample entries (after)", &after_keys.len().to_string());
    p::kv("Fields added", &added.len().to_string());
    p::kv("Fields removed", &removed.len().to_string());
    p::kv(
        "Ops applied successfully",
        &report.entries_migrated.to_string(),
    );
    println!();

    if !added.is_empty() {
        println!("  {}", "Added:".green().bold());
        for k in &added {
            println!("    + {}", k.green());
        }
    }
    if !removed.is_empty() {
        println!("  {}", "Removed:".red().bold());
        for k in &removed {
            println!("    - {}", k.red());
        }
    }
    if !report.warnings.is_empty() {
        println!();
        println!("  {}", "Warnings:".yellow().bold());
        for w in &report.warnings {
            p::warn(w);
        }
    }

    let validation = validate_snapshot(&report.snapshot, &rules);
    println!();
    if validation.is_ok() {
        p::success("Dry run passed validation. No output was written — re-run with `migrate run` to apply for real.");
    } else {
        p::warn("Dry run completed but the result would fail validation. Fix your rules before running for real.");
    }
    Ok(())
}

fn handle_rollback(args: RollbackArgs) -> Result<()> {
    p::header("Rollback Migration");

    let mut history = load_history()?;
    let record = history
        .iter_mut()
        .find(|r| r.id == args.migration_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Migration '{}' not found. Run `starforge migrate history` to see available migrations.",
                args.migration_id
            )
        })?;

    let backup_path = PathBuf::from(&record.backup_path);
    if !backup_path.exists() {
        anyhow::bail!(
            "Backup snapshot for migration '{}' is missing on disk: {}",
            args.migration_id,
            backup_path.display()
        );
    }

    p::separator();
    p::kv("Migration ID", &record.id);
    p::kv("Contract ID", &record.contract_id);
    p::kv_accent("Restoring from backup", &backup_path.display().to_string());
    p::kv("Restoring to", &args.output.display().to_string());

    if !args.yes {
        println!();
        print!(
            "  Overwrite {} with the pre-migration backup? [y/N] ",
            args.output.display()
        );
        use std::io::BufRead;
        let line = std::io::stdin()
            .lock()
            .lines()
            .next()
            .unwrap_or(Ok(String::new()))?;
        if !matches!(line.trim().to_lowercase().as_str(), "y" | "yes") {
            p::info("Rollback cancelled.");
            return Ok(());
        }
    }

    let backup = load_snapshot(&backup_path)?;
    save_snapshot(&backup, &args.output)?;
    record.status = MigrationStatus::RolledBack;
    save_history(&history)?;

    println!();
    p::success(&format!(
        "Restored pre-migration snapshot to {}",
        args.output.display()
    ));
    p::separator();
    Ok(())
}

fn handle_history(args: HistoryArgs) -> Result<()> {
    p::header("Migration History");

    let history = load_history()?;
    let filtered: Vec<_> = history
        .iter()
        .filter(|r| {
            args.contract_id
                .as_deref()
                .map_or(true, |id| r.contract_id == id)
        })
        .collect();

    if filtered.is_empty() {
        p::info("No migrations recorded yet.");
        return Ok(());
    }

    p::separator();
    println!(
        "  {:<16}  {:<10}  {:<10}  {:<22}  {:<12}  {}",
        "Migration ID".dimmed(),
        "From".dimmed(),
        "To".dimmed(),
        "Status".dimmed(),
        "Migrated".dimmed(),
        "Timestamp".dimmed(),
    );
    println!("  {}", "─".repeat(86).dimmed());

    for record in &filtered {
        let status_colored = match record.status {
            MigrationStatus::Completed => record.status.to_string().green().to_string(),
            MigrationStatus::CompletedWithWarnings => {
                record.status.to_string().yellow().to_string()
            }
            MigrationStatus::Failed => record.status.to_string().red().to_string(),
            MigrationStatus::RolledBack => record.status.to_string().cyan().to_string(),
        };
        println!(
            "  {:<16}  {:<10}  {:<10}  {:<22}  {:<12}  {}",
            record.id.white(),
            record.from_version.dimmed(),
            record.to_version.dimmed(),
            status_colored,
            record.entries_migrated.to_string().white(),
            record
                .timestamp
                .get(..16)
                .unwrap_or(&record.timestamp)
                .dimmed(),
        );
    }
    p::separator();
    Ok(())
}

fn handle_docs(args: DocsArgs) -> Result<()> {
    let docs = migration_docs();
    match args.output {
        Some(path) => {
            fs::write(&path, docs)?;
            p::success(&format!("Wrote migration guide to {}", path.display()));
        }
        None => println!("{}", docs),
    }
    Ok(())
}

fn handle_diff(args: DiffArgs) -> Result<()> {
    p::header("Contract State Diff");

    let before_snapshot = load_snapshot(&args.before)?;
    let after_snapshot = load_snapshot(&args.after)?;

    p::kv("Before", &args.before.display().to_string());
    p::kv("After", &args.after.display().to_string());
    p::kv("Keys before", &before_snapshot.entries.len().to_string());
    p::kv("Keys after", &after_snapshot.entries.len().to_string());
    p::separator();

    let report = state_diff::diff_snapshots(
        &before_snapshot.entries,
        &after_snapshot.entries,
        Some(args.from_version.clone()),
        Some(args.to_version.clone()),
    );

    match args.format.as_str() {
        "json" => {
            let json_out = serde_json::to_string_pretty(&report)?;
            if let Some(ref out_path) = args.output {
                fs::write(out_path, &json_out)?;
                p::success(&format!("Diff report saved to {}", out_path.display()));
            } else {
                println!("{}", json_out);
            }
        }
        _ => {
            let console_out = state_diff::render_diff_console(&report);
            if let Some(ref out_path) = args.output {
                fs::write(out_path, &console_out)?;
                p::success(&format!("Diff report saved to {}", out_path.display()));
            } else {
                println!("{}", console_out);
            }
        }
    }

    println!();
    p::header("Diff Summary");
    p::kv("Added", &format!("+{}", report.summary.count_added));
    p::kv("Removed", &format!("-{}", report.summary.count_removed));
    p::kv("Modified", &format!("~{}", report.summary.count_modified));
    p::kv(
        "Type changes",
        &report.summary.count_type_changed.to_string(),
    );
    p::kv("Unchanged", &report.summary.count_unchanged.to_string());
    p::kv("Total changes", &report.summary.total_changes.to_string());

    if let Some(ref rules_path) = args.generate_rules {
        let diff_rules = state_diff::generate_migration_rules_from_diff(&report);
        let ops: Vec<TransformOp> = diff_rules
            .iter()
            .map(|r| match r.op.as_str() {
                "remove_field" => TransformOp::RemoveField { key: r.key.clone() },
                "add_field" => TransformOp::AddField {
                    key: r.key.clone(),
                    default: r.default_value.clone().unwrap_or(Value::Null),
                },
                "cast_type" => TransformOp::CastType {
                    key: r.key.clone(),
                    to_type: r
                        .target_type
                        .clone()
                        .unwrap_or_else(|| "string".to_string()),
                },
                _ => TransformOp::RemoveField { key: r.key.clone() },
            })
            .collect();
        let migration_rules = MigrationRules {
            from_version: args.from_version.clone(),
            to_version: args.to_version.clone(),
            ops,
            required_keys: after_snapshot.entries.keys().cloned().collect(),
            forbidden_keys: report.removed.iter().map(|e| e.key.clone()).collect(),
        };
        fs::write(rules_path, serde_json::to_string_pretty(&migration_rules)?)?;
        println!();
        p::success(&format!(
            "Migration rules generated from diff at {}",
            rules_path.display()
        ));
        p::info(&format!(
            "Test with: starforge migrate test --sample {} --rules {}",
            args.before.display(),
            rules_path.display()
        ));
    }

    Ok(())
}

fn migration_docs() -> String {
    r#"# Contract Storage Migration Guide

`starforge migrate` helps you move Soroban contract storage between schema
versions safely, with validation and rollback built in.

## Workflow

1. **Export a snapshot** of the contract's current storage as JSON. Each
   snapshot has the shape:

   ```json
   {
     "contract_id": "C...",
     "version": "v1",
     "captured_at": "2026-01-01T00:00:00Z",
     "entries": { "key": "value", "...": "..." }
   }
   ```

2. **Generate a rules template**:

   ```
   starforge migrate init --output rules.json --from-version v1 --to-version v2
   ```

   Edit the `ops` array. Supported operations:
   - `rename_key` — rename a field, keeping its value.
   - `add_field` — insert a new field with a default value if missing.
   - `remove_field` — drop a deprecated field.
   - `cast_type` — coerce a value to `string`, `number`, or `bool`.
   - `remap_value` — replace one literal value with another (e.g. enum migrations).

   Also fill in `required_keys` (must exist after migration) and
   `forbidden_keys` (must NOT exist after migration) to drive validation.

3. **Dry-run against sample data** before touching anything real:

   ```
   starforge migrate test --sample sample-snapshot.json --rules rules.json
   ```

4. **Run the migration**. The original snapshot is automatically backed up
   before any writes happen:

   ```
   starforge migrate run --contract-id C... --snapshot snapshot.json \
     --rules rules.json --output migrated-snapshot.json
   ```

5. **Validate** the result independently at any time:

   ```
   starforge migrate validate --snapshot migrated-snapshot.json --rules rules.json
   ```

6. **Roll back** if something is wrong, using the migration ID printed by `run`:

   ```
   starforge migrate rollback --migration-id mig-xxxxxxxxxxxx --output snapshot.json
   ```

7. **Review history** of all migrations performed locally:

   ```
   starforge migrate history --contract-id C...
   ```

## Data integrity guarantees

- Every `run` computes a SHA-256 checksum of both the source and output
  snapshots (order-independent over entries) and stores them in the
  migration record for later auditing.
- A full backup of the source snapshot is written before any transform is
  applied, keyed by the migration ID, enabling exact rollback.
- `validate` and the post-run validation step both check `required_keys`,
  `forbidden_keys`, and that `cast_type` operations produced the expected
  JSON type.

## Notes

- This tool operates on local JSON snapshots rather than mutating on-chain
  state directly — pair it with `starforge inspect storage --json` to
  capture real contract state, and your own deployment process to push the
  migrated values back on-chain (e.g. via a migration entrypoint on the
  upgraded contract).
"#
    .to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_with(entries: Vec<(&str, Value)>) -> StorageSnapshot {
        StorageSnapshot {
            contract_id: Some("CTEST".to_string()),
            version: Some("v1".to_string()),
            captured_at: None,
            entries: entries
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        }
    }

    #[test]
    fn rename_key_preserves_value() {
        let snap = snapshot_with(vec![("old", Value::String("hi".into()))]);
        let rules = MigrationRules {
            from_version: "v1".into(),
            to_version: "v2".into(),
            ops: vec![TransformOp::RenameKey {
                from: "old".into(),
                to: "new".into(),
            }],
            required_keys: vec![],
            forbidden_keys: vec![],
        };
        let report = apply_rules(&snap, &rules);
        assert!(!report.snapshot.entries.contains_key("old"));
        assert_eq!(
            report.snapshot.entries.get("new"),
            Some(&Value::String("hi".into()))
        );
        assert_eq!(report.entries_migrated, 1);
    }

    #[test]
    fn add_field_only_when_missing() {
        let snap = snapshot_with(vec![("existing", Value::Bool(true))]);
        let rules = MigrationRules {
            from_version: "v1".into(),
            to_version: "v2".into(),
            ops: vec![
                TransformOp::AddField {
                    key: "existing".into(),
                    default: Value::Bool(false),
                },
                TransformOp::AddField {
                    key: "fresh".into(),
                    default: Value::String("default".into()),
                },
            ],
            required_keys: vec![],
            forbidden_keys: vec![],
        };
        let report = apply_rules(&snap, &rules);
        // Existing field untouched.
        assert_eq!(
            report.snapshot.entries.get("existing"),
            Some(&Value::Bool(true))
        );
        // New field inserted.
        assert_eq!(
            report.snapshot.entries.get("fresh"),
            Some(&Value::String("default".into()))
        );
        assert_eq!(report.entries_migrated, 1);
    }

    #[test]
    fn remove_field_drops_key() {
        let snap = snapshot_with(vec![("gone", Value::Null), ("kept", Value::Null)]);
        let rules = MigrationRules {
            from_version: "v1".into(),
            to_version: "v2".into(),
            ops: vec![TransformOp::RemoveField { key: "gone".into() }],
            required_keys: vec![],
            forbidden_keys: vec![],
        };
        let report = apply_rules(&snap, &rules);
        assert!(!report.snapshot.entries.contains_key("gone"));
        assert!(report.snapshot.entries.contains_key("kept"));
    }

    #[test]
    fn cast_type_string_to_number() {
        let snap = snapshot_with(vec![("balance", Value::String("42".into()))]);
        let rules = MigrationRules {
            from_version: "v1".into(),
            to_version: "v2".into(),
            ops: vec![TransformOp::CastType {
                key: "balance".into(),
                to_type: "number".into(),
            }],
            required_keys: vec![],
            forbidden_keys: vec![],
        };
        let report = apply_rules(&snap, &rules);
        assert_eq!(
            report.snapshot.entries.get("balance").unwrap().is_number(),
            true
        );
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn cast_type_failure_emits_warning() {
        let snap = snapshot_with(vec![("balance", Value::String("not-a-number".into()))]);
        let rules = MigrationRules {
            from_version: "v1".into(),
            to_version: "v2".into(),
            ops: vec![TransformOp::CastType {
                key: "balance".into(),
                to_type: "number".into(),
            }],
            required_keys: vec![],
            forbidden_keys: vec![],
        };
        let report = apply_rules(&snap, &rules);
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn remap_value_swaps_literal() {
        let snap = snapshot_with(vec![("status", Value::String("legacy_active".into()))]);
        let rules = MigrationRules {
            from_version: "v1".into(),
            to_version: "v2".into(),
            ops: vec![TransformOp::RemapValue {
                key: "status".into(),
                from_value: Value::String("legacy_active".into()),
                to_value: Value::String("active".into()),
            }],
            required_keys: vec![],
            forbidden_keys: vec![],
        };
        let report = apply_rules(&snap, &rules);
        assert_eq!(
            report.snapshot.entries.get("status"),
            Some(&Value::String("active".into()))
        );
    }

    #[test]
    fn validation_catches_missing_required_and_forbidden_present() {
        let snap = snapshot_with(vec![("deprecated_field", Value::Bool(true))]);
        let rules = MigrationRules {
            from_version: "v1".into(),
            to_version: "v2".into(),
            ops: vec![],
            required_keys: vec!["schema_version".into()],
            forbidden_keys: vec!["deprecated_field".into()],
        };
        let report = validate_snapshot(&snap, &rules);
        assert!(!report.is_ok());
        assert_eq!(report.missing_required, vec!["schema_version".to_string()]);
        assert_eq!(
            report.present_forbidden,
            vec!["deprecated_field".to_string()]
        );
    }

    #[test]
    fn validation_passes_for_clean_snapshot() {
        let snap = snapshot_with(vec![("schema_version", Value::String("v2".into()))]);
        let rules = MigrationRules {
            from_version: "v1".into(),
            to_version: "v2".into(),
            ops: vec![],
            required_keys: vec!["schema_version".into()],
            forbidden_keys: vec!["deprecated_field".into()],
        };
        let report = validate_snapshot(&snap, &rules);
        assert!(report.is_ok());
    }

    #[test]
    fn checksum_is_deterministic_and_order_independent() {
        let snap_a = snapshot_with(vec![("a", Value::Bool(true)), ("b", Value::Bool(false))]);
        let snap_b = snapshot_with(vec![("b", Value::Bool(false)), ("a", Value::Bool(true))]);
        assert_eq!(snapshot_checksum(&snap_a), snapshot_checksum(&snap_b));
    }

    #[test]
    fn checksum_changes_when_data_changes() {
        let snap_a = snapshot_with(vec![("a", Value::Bool(true))]);
        let snap_b = snapshot_with(vec![("a", Value::Bool(false))]);
        assert_ne!(snapshot_checksum(&snap_a), snapshot_checksum(&snap_b));
    }

    #[test]
    fn full_migration_round_trip_with_rollback_semantics() {
        let original = snapshot_with(vec![
            ("old_field_name", Value::String("hello".into())),
            ("balance", Value::String("100".into())),
            ("deprecated_field", Value::Bool(true)),
        ]);
        let rules = MigrationRules {
            from_version: "v1".into(),
            to_version: "v2".into(),
            ops: vec![
                TransformOp::RenameKey {
                    from: "old_field_name".into(),
                    to: "new_field_name".into(),
                },
                TransformOp::CastType {
                    key: "balance".into(),
                    to_type: "number".into(),
                },
                TransformOp::RemoveField {
                    key: "deprecated_field".into(),
                },
                TransformOp::AddField {
                    key: "schema_version".into(),
                    default: Value::String("v2".into()),
                },
            ],
            required_keys: vec!["schema_version".into(), "new_field_name".into()],
            forbidden_keys: vec!["deprecated_field".into()],
        };

        let report = apply_rules(&original, &rules);
        let validation = validate_snapshot(&report.snapshot, &rules);
        assert!(validation.is_ok());

        // Simulate "rollback": the original snapshot is untouched and still
        // satisfies its own (empty) constraints — i.e. nothing in `apply_rules`
        // mutates the source.
        assert!(original.entries.contains_key("old_field_name"));
        assert!(original.entries.contains_key("deprecated_field"));
    }

    #[test]
    fn snapshot_and_restore_file_io() {
        let temp_dir = tempfile::tempdir().unwrap();
        let snap_file = temp_dir.path().join("snap.json");
        let restored_file = temp_dir.path().join("restored.json");

        let snap = snapshot_with(vec![("key1", Value::String("val1".into()))]);
        save_snapshot(&snap, &snap_file).unwrap();

        let loaded = load_snapshot(&snap_file).unwrap();
        assert_eq!(
            loaded.entries.get("key1"),
            Some(&Value::String("val1".into()))
        );

        save_snapshot(&loaded, &restored_file).unwrap();
        let restored = load_snapshot(&restored_file).unwrap();
        assert_eq!(snapshot_checksum(&snap), snapshot_checksum(&restored));
    }
}
