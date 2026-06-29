//! Contract state diffing and migration tooling.
//!
//! This module provides the building blocks for safely upgrading Soroban
//! contracts that carry persistent state:
//!
//! * **Snapshots** — capture a contract's storage entries (from a live RPC
//!   inspection or an offline JSON export) into a versioned, hashable record.
//! * **Diffing** — compute a structured difference between two snapshots
//!   (added / removed / modified / unchanged keys).
//! * **Migration scripts** — generate a `soroban_sdk` migration function and a
//!   machine-readable migration plan from a diff.
//! * **Validation** — check a state transition against a configurable policy
//!   and surface risky changes (e.g. dropping persistent keys).
//! * **Testing framework** — apply a migration's operations to a base snapshot
//!   entirely offline and compare the result against an expected snapshot.
//! * **Rollback** — derive the inverse operations needed to restore a prior
//!   snapshot.
//!
//! All persistence follows the same `~/.starforge/...` + `serde_json` pattern
//! used elsewhere in the codebase (see `deploy_history.rs`).

use crate::utils::config;
use crate::utils::soroban::ContractInspectResult;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Snapshot model
// ---------------------------------------------------------------------------

/// A single key/value entry in a contract's storage, tagged with its
/// durability scope (`instance`, `persistent`, or `temporary`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateEntry {
    pub key: String,
    pub value: String,
    #[serde(default = "default_durability")]
    pub durability: String,
}

fn default_durability() -> String {
    "instance".to_string()
}

/// A point-in-time capture of a contract's on-chain state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Unique snapshot identifier (UUID).
    pub id: String,
    pub contract_id: String,
    pub network: String,
    /// WASM hash of the executable backing the contract, when known.
    pub wasm_hash: Option<String>,
    /// Ledger sequence the snapshot was taken at.
    pub ledger_seq: u32,
    /// Optional human-friendly label (e.g. `v1`, `pre-upgrade`).
    pub label: Option<String>,
    pub timestamp: String,
    pub entries: Vec<StateEntry>,
    /// Deterministic SHA-256 over the canonicalized entries.
    pub state_hash: String,
}

impl StateSnapshot {
    /// Build a snapshot from a live RPC inspection result.
    pub fn from_inspect(
        result: &ContractInspectResult,
        network: &str,
        label: Option<String>,
    ) -> Self {
        let entries: Vec<StateEntry> = result
            .instance_storage
            .iter()
            .map(|e| StateEntry {
                key: e.key.clone(),
                value: e.value.clone(),
                durability: normalize_durability(&result.storage_durability),
            })
            .collect();

        Self::new(
            &result.contract_id,
            network,
            result.wasm_hash.clone(),
            result
                .last_modified_ledger_seq
                .unwrap_or(result.latest_ledger),
            label,
            entries,
        )
    }

    /// Construct a snapshot, computing its `id`, `timestamp` and `state_hash`.
    pub fn new(
        contract_id: &str,
        network: &str,
        wasm_hash: Option<String>,
        ledger_seq: u32,
        label: Option<String>,
        mut entries: Vec<StateEntry>,
    ) -> Self {
        // Canonical ordering keeps `state_hash` stable regardless of RPC order.
        entries.sort_by(|a, b| (&a.durability, &a.key).cmp(&(&b.durability, &b.key)));
        let state_hash = compute_state_hash(&entries);
        Self {
            id: new_id(),
            contract_id: contract_id.to_string(),
            network: network.to_string(),
            wasm_hash,
            ledger_seq,
            label,
            timestamp: now_rfc3339(),
            entries,
            state_hash,
        }
    }

    /// Look up an entry by `(durability, key)`.
    pub fn get(&self, durability: &str, key: &str) -> Option<&StateEntry> {
        self.entries
            .iter()
            .find(|e| e.durability == durability && e.key == key)
    }
}

/// Compute a deterministic hash over storage entries. Entries are sorted by
/// `(durability, key)` so the hash is independent of insertion order.
pub fn compute_state_hash(entries: &[StateEntry]) -> String {
    let mut sorted: Vec<&StateEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| (&a.durability, &a.key).cmp(&(&b.durability, &b.key)));
    let mut hasher = Sha256::new();
    for e in sorted {
        hasher.update(e.durability.as_bytes());
        hasher.update(b"|");
        hasher.update(e.key.as_bytes());
        hasher.update(b"=");
        hasher.update(e.value.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

fn normalize_durability(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if lower.contains("persistent") {
        "persistent".to_string()
    } else if lower.contains("temporary") || lower.contains("temp") {
        "temporary".to_string()
    } else {
        "instance".to_string()
    }
}

// ---------------------------------------------------------------------------
// Diffing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
    Unchanged,
}

impl ChangeKind {
    pub fn symbol(&self) -> &'static str {
        match self {
            ChangeKind::Added => "+",
            ChangeKind::Removed => "-",
            ChangeKind::Modified => "~",
            ChangeKind::Unchanged => "=",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryDiff {
    pub durability: String,
    pub key: String,
    pub kind: ChangeKind,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateDiff {
    pub contract_id: String,
    pub from_id: String,
    pub to_id: String,
    pub from_hash: String,
    pub to_hash: String,
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
    pub unchanged: usize,
    pub entries: Vec<EntryDiff>,
}

impl StateDiff {
    /// Whether the two snapshots are byte-for-byte identical in state.
    pub fn is_identical(&self) -> bool {
        self.added == 0 && self.removed == 0 && self.modified == 0
    }

    /// Total number of changed entries.
    pub fn change_count(&self) -> usize {
        self.added + self.removed + self.modified
    }
}

/// Compute the diff that transforms `from` into `to`.
pub fn diff_snapshots(from: &StateSnapshot, to: &StateSnapshot) -> StateDiff {
    let mut from_map: BTreeMap<(String, String), &StateEntry> = BTreeMap::new();
    for e in &from.entries {
        from_map.insert((e.durability.clone(), e.key.clone()), e);
    }
    let mut to_map: BTreeMap<(String, String), &StateEntry> = BTreeMap::new();
    for e in &to.entries {
        to_map.insert((e.durability.clone(), e.key.clone()), e);
    }

    let mut entries = Vec::new();
    let (mut added, mut removed, mut modified, mut unchanged) = (0, 0, 0, 0);

    // Union of all keys, in deterministic order.
    let mut all_keys: Vec<(String, String)> =
        from_map.keys().chain(to_map.keys()).cloned().collect();
    all_keys.sort();
    all_keys.dedup();

    for key in all_keys {
        let old = from_map.get(&key);
        let new = to_map.get(&key);
        let (kind, old_value, new_value) = match (old, new) {
            (Some(o), Some(n)) if o.value == n.value => {
                unchanged += 1;
                (
                    ChangeKind::Unchanged,
                    Some(o.value.clone()),
                    Some(n.value.clone()),
                )
            }
            (Some(o), Some(n)) => {
                modified += 1;
                (
                    ChangeKind::Modified,
                    Some(o.value.clone()),
                    Some(n.value.clone()),
                )
            }
            (None, Some(n)) => {
                added += 1;
                (ChangeKind::Added, None, Some(n.value.clone()))
            }
            (Some(o), None) => {
                removed += 1;
                (ChangeKind::Removed, Some(o.value.clone()), None)
            }
            (None, None) => unreachable!("key came from one of the maps"),
        };
        entries.push(EntryDiff {
            durability: key.0,
            key: key.1,
            kind,
            old_value,
            new_value,
        });
    }

    StateDiff {
        contract_id: to.contract_id.clone(),
        from_id: from.id.clone(),
        to_id: to.id.clone(),
        from_hash: from.state_hash.clone(),
        to_hash: to.state_hash.clone(),
        added,
        removed,
        modified,
        unchanged,
        entries,
    }
}

// ---------------------------------------------------------------------------
// Migration operations, plans and scripts
// ---------------------------------------------------------------------------

/// A single, reversible state-mutation operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MigrationOperation {
    /// Insert or overwrite a key.
    SetKey {
        durability: String,
        key: String,
        value: String,
    },
    /// Delete a key.
    RemoveKey { durability: String, key: String },
}

impl MigrationOperation {
    fn durability(&self) -> &str {
        match self {
            MigrationOperation::SetKey { durability, .. } => durability,
            MigrationOperation::RemoveKey { durability, .. } => durability,
        }
    }
    fn key(&self) -> &str {
        match self {
            MigrationOperation::SetKey { key, .. } => key,
            MigrationOperation::RemoveKey { key, .. } => key,
        }
    }
}

/// A machine-readable migration plan derived from a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub contract_id: String,
    pub from_hash: String,
    pub to_hash: String,
    pub created_at: String,
    pub operations: Vec<MigrationOperation>,
}

/// Turn a diff into an ordered list of operations that transform the `from`
/// state into the `to` state.
pub fn operations_from_diff(diff: &StateDiff) -> Vec<MigrationOperation> {
    let mut ops = Vec::new();
    for e in &diff.entries {
        match e.kind {
            ChangeKind::Added | ChangeKind::Modified => {
                if let Some(v) = &e.new_value {
                    ops.push(MigrationOperation::SetKey {
                        durability: e.durability.clone(),
                        key: e.key.clone(),
                        value: v.clone(),
                    });
                }
            }
            ChangeKind::Removed => ops.push(MigrationOperation::RemoveKey {
                durability: e.durability.clone(),
                key: e.key.clone(),
            }),
            ChangeKind::Unchanged => {}
        }
    }
    ops
}

/// Build a migration plan from a diff.
pub fn generate_migration_plan(diff: &StateDiff) -> MigrationPlan {
    MigrationPlan {
        contract_id: diff.contract_id.clone(),
        from_hash: diff.from_hash.clone(),
        to_hash: diff.to_hash.clone(),
        created_at: now_rfc3339(),
        operations: operations_from_diff(diff),
    }
}

/// Render a `soroban_sdk` migration function from a plan. The generated code is
/// a starting point: storage key/value reconstruction is emitted as commented
/// guidance because on-chain types are contract specific.
pub fn generate_migration_script(plan: &MigrationPlan) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "// Auto-generated migration for contract {}\n",
        plan.contract_id
    ));
    body.push_str(&format!("// from state {}\n", short_hash(&plan.from_hash)));
    body.push_str(&format!("//   to state {}\n", short_hash(&plan.to_hash)));
    body.push_str(&format!("// generated at {}\n", plan.created_at));
    body.push_str("// Operations: ");
    let sets = plan
        .operations
        .iter()
        .filter(|o| matches!(o, MigrationOperation::SetKey { .. }))
        .count();
    let removes = plan.operations.len() - sets;
    body.push_str(&format!("{} set, {} remove\n\n", sets, removes));

    body.push_str("#![no_std]\n");
    body.push_str("use soroban_sdk::{contract, contractimpl, Env, Symbol, Address};\n\n");
    body.push_str("#[contract]\n");
    body.push_str("pub struct Migration;\n\n");
    body.push_str("#[contractimpl]\n");
    body.push_str("impl Migration {\n");
    body.push_str("    /// Apply storage migration. Requires admin authorization.\n");
    body.push_str("    pub fn migrate(env: Env, admin: Address) {\n");
    body.push_str("        admin.require_auth();\n\n");

    for op in &plan.operations {
        match op {
            MigrationOperation::SetKey {
                durability,
                key,
                value,
            } => {
                body.push_str(&format!(
                    "        // SET [{}] {} = {}\n",
                    durability,
                    key,
                    truncate(value, 60)
                ));
                body.push_str(&format!(
                    "        // env.storage().{}().set(&{}, &/* decode value */);\n",
                    storage_accessor(durability),
                    key_literal(key)
                ));
            }
            MigrationOperation::RemoveKey { durability, key } => {
                body.push_str(&format!("        // REMOVE [{}] {}\n", durability, key));
                body.push_str(&format!(
                    "        // env.storage().{}().remove(&{});\n",
                    storage_accessor(durability),
                    key_literal(key)
                ));
            }
        }
    }

    body.push_str("\n        // Record migration provenance for auditing.\n");
    body.push_str("        env.events().publish(\n");
    body.push_str("            (Symbol::new(&env, \"migrate\"),),\n");
    body.push_str(&format!(
        "            (Symbol::new(&env, \"{}\"),),\n",
        short_hash(&plan.to_hash)
    ));
    body.push_str("        );\n");
    body.push_str("    }\n");
    body.push_str("}\n");
    body
}

fn storage_accessor(durability: &str) -> &'static str {
    match durability {
        "persistent" => "persistent",
        "temporary" => "temporary",
        _ => "instance",
    }
}

/// Produce a Rust literal for a storage key. Bare identifiers become
/// `Symbol::new` calls; anything else is emitted as a placeholder.
fn key_literal(key: &str) -> String {
    if !key.is_empty()
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && key.len() <= 32
    {
        format!("Symbol::new(&env, \"{}\")", key)
    } else {
        format!("/* key: {} */", truncate(key, 40))
    }
}

// ---------------------------------------------------------------------------
// State transition validation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn label(&self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationFinding {
    pub severity: Severity,
    pub category: String,
    pub message: String,
}

/// Policy that governs which state transitions are considered safe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationPolicy {
    /// Removing a persistent key is treated as critical (data loss) unless allowed.
    pub allow_persistent_removal: bool,
    /// Removing an instance key is treated as a warning unless allowed.
    pub allow_instance_removal: bool,
    /// Maximum number of changed entries before flagging a large migration.
    pub max_changes: usize,
}

impl Default for MigrationPolicy {
    fn default() -> Self {
        Self {
            allow_persistent_removal: false,
            allow_instance_removal: true,
            max_changes: 100,
        }
    }
}

/// Validate the transition described by `diff` against `policy`.
pub fn validate_transition(diff: &StateDiff, policy: &MigrationPolicy) -> Vec<ValidationFinding> {
    let mut findings = Vec::new();

    if diff.is_identical() {
        findings.push(ValidationFinding {
            severity: Severity::Info,
            category: "noop".to_string(),
            message: "Source and target states are identical; migration is a no-op.".to_string(),
        });
        return findings;
    }

    for e in &diff.entries {
        if e.kind == ChangeKind::Removed {
            match e.durability.as_str() {
                "persistent" if !policy.allow_persistent_removal => {
                    findings.push(ValidationFinding {
                        severity: Severity::Critical,
                        category: "data-loss".to_string(),
                        message: format!(
                            "Persistent key '{}' is removed; this destroys on-chain data.",
                            e.key
                        ),
                    });
                }
                "instance" if !policy.allow_instance_removal => {
                    findings.push(ValidationFinding {
                        severity: Severity::Warning,
                        category: "removal".to_string(),
                        message: format!("Instance key '{}' is removed.", e.key),
                    });
                }
                _ => {}
            }
        }
    }

    if diff.change_count() > policy.max_changes {
        findings.push(ValidationFinding {
            severity: Severity::Warning,
            category: "scale".to_string(),
            message: format!(
                "Migration touches {} entries (policy limit {}); review carefully.",
                diff.change_count(),
                policy.max_changes
            ),
        });
    }

    if findings.is_empty() {
        findings.push(ValidationFinding {
            severity: Severity::Info,
            category: "ok".to_string(),
            message: format!(
                "Transition validated: {} added, {} modified, {} removed.",
                diff.added, diff.modified, diff.removed
            ),
        });
    }

    findings
}

/// Whether a set of findings contains a blocking (critical) issue.
pub fn has_blocking_findings(findings: &[ValidationFinding]) -> bool {
    findings.iter().any(|f| f.severity == Severity::Critical)
}

// ---------------------------------------------------------------------------
// Migration testing framework (offline)
// ---------------------------------------------------------------------------

/// Apply a list of operations to a base snapshot, producing the resulting
/// state. This runs entirely offline and is the core of the testing framework.
pub fn apply_operations(base: &StateSnapshot, ops: &[MigrationOperation]) -> StateSnapshot {
    let mut map: BTreeMap<(String, String), StateEntry> = BTreeMap::new();
    for e in &base.entries {
        map.insert((e.durability.clone(), e.key.clone()), e.clone());
    }

    for op in ops {
        let k = (op.durability().to_string(), op.key().to_string());
        match op {
            MigrationOperation::SetKey {
                durability,
                key,
                value,
            } => {
                map.insert(
                    k,
                    StateEntry {
                        key: key.clone(),
                        value: value.clone(),
                        durability: durability.clone(),
                    },
                );
            }
            MigrationOperation::RemoveKey { .. } => {
                map.remove(&k);
            }
        }
    }

    let entries: Vec<StateEntry> = map.into_values().collect();
    StateSnapshot::new(
        &base.contract_id,
        &base.network,
        base.wasm_hash.clone(),
        base.ledger_seq,
        Some("migration-test-result".to_string()),
        entries,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationTestResult {
    pub passed: bool,
    pub expected_hash: String,
    pub actual_hash: String,
    pub mismatches: Vec<EntryDiff>,
}

/// Apply `ops` to `base` and assert the result matches `expected`. Returns a
/// structured result listing any mismatching entries.
pub fn test_migration(
    base: &StateSnapshot,
    ops: &[MigrationOperation],
    expected: &StateSnapshot,
) -> MigrationTestResult {
    let actual = apply_operations(base, ops);
    let diff = diff_snapshots(expected, &actual);
    let mismatches: Vec<EntryDiff> = diff
        .entries
        .into_iter()
        .filter(|e| e.kind != ChangeKind::Unchanged)
        .collect();
    MigrationTestResult {
        passed: mismatches.is_empty(),
        expected_hash: expected.state_hash.clone(),
        actual_hash: actual.state_hash.clone(),
        mismatches,
    }
}

/// Derive the operations needed to roll back from `current` to `target`.
/// This is simply the diff in the reverse direction.
pub fn rollback_operations(
    current: &StateSnapshot,
    target: &StateSnapshot,
) -> Vec<MigrationOperation> {
    let reverse = diff_snapshots(current, target);
    operations_from_diff(&reverse)
}

// ---------------------------------------------------------------------------
// Persistence (~/.starforge/state-snapshots/)
// ---------------------------------------------------------------------------

fn snapshots_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("state-snapshots");
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    }
    Ok(dir)
}

fn snapshot_path(id: &str) -> Result<PathBuf> {
    Ok(snapshots_dir()?.join(format!("{}.json", id)))
}

/// Persist a snapshot to `~/.starforge/state-snapshots/<id>.json`.
pub fn save_snapshot(snapshot: &StateSnapshot) -> Result<PathBuf> {
    let path = snapshot_path(&snapshot.id)?;
    let json = serde_json::to_string_pretty(snapshot)?;
    fs::write(&path, json).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(path)
}

/// Load all stored snapshots, most-recent first.
pub fn list_snapshots() -> Result<Vec<StateSnapshot>> {
    let dir = snapshots_dir()?;
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let contents = fs::read_to_string(&path)?;
        if let Ok(snap) = serde_json::from_str::<StateSnapshot>(&contents) {
            out.push(snap);
        }
    }
    out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(out)
}

/// Resolve a snapshot by id, label, or the literal `latest`. When `contract`
/// is supplied, only snapshots for that contract are considered.
pub fn resolve_snapshot(reference: &str, contract: Option<&str>) -> Result<StateSnapshot> {
    let snapshots = list_snapshots()?;
    let filtered: Vec<StateSnapshot> = snapshots
        .into_iter()
        .filter(|s| contract.map(|c| s.contract_id == c).unwrap_or(true))
        .collect();

    if reference == "latest" {
        return filtered
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No snapshots found"));
    }

    // Exact id match first, then label match.
    if let Some(s) = filtered.iter().find(|s| s.id == reference) {
        return Ok(s.clone());
    }
    if let Some(s) = filtered
        .iter()
        .find(|s| s.label.as_deref() == Some(reference))
    {
        return Ok(s.clone());
    }
    // Allow short-id prefix matches for convenience.
    let prefix_matches: Vec<&StateSnapshot> = filtered
        .iter()
        .filter(|s| s.id.starts_with(reference))
        .collect();
    match prefix_matches.len() {
        1 => Ok(prefix_matches[0].clone()),
        0 => anyhow::bail!("No snapshot matching '{}'", reference),
        _ => anyhow::bail!(
            "Ambiguous snapshot reference '{}' matches multiple snapshots",
            reference
        ),
    }
}

/// Load a snapshot from an arbitrary JSON file (offline import).
pub fn load_snapshot_file(path: &std::path::Path) -> Result<StateSnapshot> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_str(&contents).with_context(|| "Failed to parse snapshot JSON")
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn short_hash(hash: &str) -> String {
    if hash.len() > 12 {
        format!("{}…", &hash[..12])
    } else {
        hash.to_string()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}…", &s[..max])
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, value: &str, dur: &str) -> StateEntry {
        StateEntry {
            key: key.to_string(),
            value: value.to_string(),
            durability: dur.to_string(),
        }
    }

    fn snap(contract: &str, entries: Vec<StateEntry>) -> StateSnapshot {
        StateSnapshot::new(contract, "testnet", Some("hash".into()), 1, None, entries)
    }

    #[test]
    fn state_hash_is_order_independent() {
        let a = compute_state_hash(&[entry("A", "1", "instance"), entry("B", "2", "instance")]);
        let b = compute_state_hash(&[entry("B", "2", "instance"), entry("A", "1", "instance")]);
        assert_eq!(a, b);
    }

    #[test]
    fn diff_detects_all_change_kinds() {
        let from = snap(
            "C1",
            vec![
                entry("keep", "v", "instance"),
                entry("change", "old", "instance"),
                entry("drop", "x", "persistent"),
            ],
        );
        let to = snap(
            "C1",
            vec![
                entry("keep", "v", "instance"),
                entry("change", "new", "instance"),
                entry("add", "y", "instance"),
            ],
        );
        let diff = diff_snapshots(&from, &to);
        assert_eq!(diff.added, 1);
        assert_eq!(diff.removed, 1);
        assert_eq!(diff.modified, 1);
        assert_eq!(diff.unchanged, 1);
        assert!(!diff.is_identical());
    }

    #[test]
    fn identical_snapshots_diff_clean() {
        let entries = vec![entry("a", "1", "instance"), entry("b", "2", "persistent")];
        let from = snap("C", entries.clone());
        let to = snap("C", entries);
        let diff = diff_snapshots(&from, &to);
        assert!(diff.is_identical());
        assert_eq!(diff.unchanged, 2);
    }

    #[test]
    fn apply_operations_reconstructs_target() {
        let from = snap(
            "C",
            vec![entry("a", "1", "instance"), entry("b", "2", "instance")],
        );
        let to = snap(
            "C",
            vec![entry("a", "1", "instance"), entry("c", "3", "instance")],
        );
        let diff = diff_snapshots(&from, &to);
        let ops = operations_from_diff(&diff);
        let result = apply_operations(&from, &ops);
        assert_eq!(result.state_hash, to.state_hash);
    }

    #[test]
    fn test_migration_passes_for_correct_ops() {
        let from = snap("C", vec![entry("a", "1", "instance")]);
        let to = snap("C", vec![entry("a", "2", "instance")]);
        let ops = operations_from_diff(&diff_snapshots(&from, &to));
        let result = test_migration(&from, &ops, &to);
        assert!(result.passed, "mismatches: {:?}", result.mismatches);
    }

    #[test]
    fn test_migration_fails_for_wrong_ops() {
        let from = snap("C", vec![entry("a", "1", "instance")]);
        let to = snap("C", vec![entry("a", "2", "instance")]);
        let wrong = vec![MigrationOperation::SetKey {
            durability: "instance".into(),
            key: "a".into(),
            value: "999".into(),
        }];
        let result = test_migration(&from, &wrong, &to);
        assert!(!result.passed);
        assert_eq!(result.mismatches.len(), 1);
    }

    #[test]
    fn rollback_inverts_migration() {
        let v1 = snap(
            "C",
            vec![entry("a", "1", "instance"), entry("b", "2", "instance")],
        );
        let v2 = snap("C", vec![entry("a", "9", "instance")]);
        let rollback = rollback_operations(&v2, &v1);
        let restored = apply_operations(&v2, &rollback);
        assert_eq!(restored.state_hash, v1.state_hash);
    }

    #[test]
    fn validation_flags_persistent_removal() {
        let from = snap("C", vec![entry("vault", "100", "persistent")]);
        let to = snap("C", vec![]);
        let diff = diff_snapshots(&from, &to);
        let findings = validate_transition(&diff, &MigrationPolicy::default());
        assert!(has_blocking_findings(&findings));
    }

    #[test]
    fn validation_allows_safe_additions() {
        let from = snap("C", vec![entry("a", "1", "instance")]);
        let to = snap(
            "C",
            vec![entry("a", "1", "instance"), entry("b", "2", "instance")],
        );
        let diff = diff_snapshots(&from, &to);
        let findings = validate_transition(&diff, &MigrationPolicy::default());
        assert!(!has_blocking_findings(&findings));
    }

    #[test]
    fn migration_script_mentions_operations() {
        let from = snap("CABC", vec![entry("old", "1", "persistent")]);
        let to = snap("CABC", vec![entry("new", "2", "persistent")]);
        let plan = generate_migration_plan(&diff_snapshots(&from, &to));
        let script = generate_migration_script(&plan);
        assert!(script.contains("fn migrate"));
        assert!(script.contains("SET"));
        assert!(script.contains("REMOVE"));
    }
}
