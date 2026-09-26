//! Upgrade rehearsal against a captured ledger snapshot (#927).
//!
//! Static checks (`migrate introspect`, `migrate hazards`) catch layout
//! mistakes before an upgrade; a rehearsal adds a *dynamic* check against
//! real state. The engine here is deliberately pure and self-contained: it
//! never touches the network and does not execute WASM. Instead it:
//!
//! 1. parses a `rehearsal.toml` script describing the calls to replay,
//! 2. compares the storage layout recorded in a ledger snapshot ("old") with
//!    the layout the new contract expects ("new") to surface decode failures,
//! 3. models each scripted call against the snapshot state, and
//! 4. renders a [`RehearsalReport`] as text (for a governance proposal) or JSON
//!    (for automation).
//!
//! The report is intentionally stable and deterministic so it can be attached
//! verbatim to an on-chain upgrade proposal.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;

// ── Script (rehearsal.toml) ──────────────────────────────────────────────────

/// A single `[[calls]]` entry in a rehearsal script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RehearsalCall {
    /// Human-readable label for the call, used in the report.
    pub name: String,
    /// Contract function (or storage key) to invoke.
    pub function: String,
    /// Arguments passed to the function, recorded in the report.
    #[serde(default)]
    pub args: Vec<Value>,
    /// Expected return value. When omitted the call only has to succeed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect: Option<Value>,
}

/// A `[[storage]]` entry declaring a key the *new* contract expects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageLayoutEntry {
    /// Storage key (usually a `DataKey` variant name).
    pub key: String,
    /// Soroban value type the new contract decodes the key as (e.g. `u128`).
    pub value_type: String,
    /// Storage tier the key lives in.
    #[serde(default)]
    pub tier: StorageTier,
}

/// Parsed `rehearsal.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RehearsalScript {
    /// Optional contract ID; falls back to the `--contract` flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
    /// Optional description included in the rendered report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Calls replayed against the snapshot.
    #[serde(default)]
    pub calls: Vec<RehearsalCall>,
    /// Storage layout the new contract is expected to use.
    #[serde(default)]
    pub storage: Vec<StorageLayoutEntry>,
}

impl RehearsalScript {
    /// Parse a rehearsal script from TOML, validating the required fields.
    pub fn parse(raw: &str) -> Result<Self> {
        let script: RehearsalScript =
            toml::from_str(raw).context("Failed to parse rehearsal script as TOML")?;

        if script.calls.is_empty() {
            anyhow::bail!("Rehearsal script must define at least one [[calls]] entry");
        }
        for call in &script.calls {
            if call.name.trim().is_empty() {
                anyhow::bail!("Every [[calls]] entry must set a non-empty `name`");
            }
            if call.function.trim().is_empty() {
                anyhow::bail!("Rehearsal call '{}' must set a `function`", call.name);
            }
        }
        Ok(script)
    }
}

// ── Ledger snapshot ──────────────────────────────────────────────────────────

/// Storage tier in Soroban (instance, persistent, temporary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StorageTier {
    /// Contract instance storage.
    #[default]
    Instance,
    /// Persistent storage with an explicit TTL.
    Persistent,
    /// Temporary storage that can be evicted.
    Temporary,
}

impl std::fmt::Display for StorageTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageTier::Instance => write!(f, "instance"),
            StorageTier::Persistent => write!(f, "persistent"),
            StorageTier::Temporary => write!(f, "temporary"),
        }
    }
}

/// A single captured storage entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntry {
    /// Storage key.
    pub key: String,
    /// Decoded value as captured in the snapshot.
    #[serde(default)]
    pub value: Value,
    /// Type tag of the stored value (e.g. `u128`, `address`).
    #[serde(default, alias = "type")]
    pub value_type: String,
    /// Storage tier the entry lives in.
    #[serde(default)]
    pub tier: StorageTier,
}

/// A ledger snapshot captured from a live contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerSnapshot {
    /// Contract the snapshot was captured from.
    #[serde(default)]
    pub contract_id: Option<String>,
    /// WASM hash of the contract at capture time.
    #[serde(default)]
    pub wasm_hash: Option<String>,
    /// Ledger sequence the snapshot was captured at.
    #[serde(default)]
    pub ledger: Option<u32>,
    /// Captured storage entries.
    #[serde(default)]
    pub entries: Vec<LedgerEntry>,
}

impl LedgerSnapshot {
    /// Parse a ledger snapshot from JSON.
    pub fn parse(raw: &str) -> Result<Self> {
        serde_json::from_str(raw).context("Failed to parse ledger snapshot as JSON")
    }
}

// ── Storage decode check ─────────────────────────────────────────────────────

/// A storage key that the new contract can no longer decode as-is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageDecodeError {
    /// Key that failed to decode.
    pub key: String,
    /// Type recorded in the snapshot.
    pub snapshot_type: String,
    /// Type the new contract expects.
    pub expected_type: String,
    /// Human-readable explanation included in the report.
    pub message: String,
}

/// Outcome of comparing the snapshot layout with the new contract's layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageLayoutComparison {
    /// Number of keys present in the snapshot.
    pub checked_keys: usize,
    /// Keys introduced by the new contract with no snapshot value.
    pub added_keys: Vec<String>,
    /// Snapshot keys the new contract no longer references.
    pub removed_keys: Vec<String>,
    /// Keys whose stored value can no longer be decoded.
    pub decode_errors: Vec<StorageDecodeError>,
    /// True when every snapshot key still decodes under the new layout.
    pub is_decodable: bool,
}

fn types_match(snapshot_type: &str, expected_type: &str) -> bool {
    snapshot_type
        .trim()
        .eq_ignore_ascii_case(expected_type.trim())
}

/// Compare the storage layout recorded in a snapshot (`old`) with the layout
/// the new contract expects (`new`).
///
/// A key that exists in both but whose declared type changed — or that moved
/// between storage tiers — cannot be decoded in place and is reported as a
/// [`StorageDecodeError`]. A deliberately broken change such as narrowing an
/// `u128` balance to `u32` is therefore caught before the upgrade is proposed.
pub fn compare_storage_layout(
    old: &[StorageLayoutEntry],
    new: &[StorageLayoutEntry],
) -> StorageLayoutComparison {
    let old_by_key: BTreeMap<&str, &StorageLayoutEntry> =
        old.iter().map(|entry| (entry.key.as_str(), entry)).collect();
    let new_by_key: BTreeMap<&str, &StorageLayoutEntry> =
        new.iter().map(|entry| (entry.key.as_str(), entry)).collect();

    let mut decode_errors = Vec::new();
    let mut added_keys = Vec::new();
    let mut removed_keys = Vec::new();

    for (key, new_entry) in &new_by_key {
        match old_by_key.get(key) {
            None => added_keys.push((*key).to_string()),
            Some(old_entry) => {
                if !types_match(&old_entry.value_type, &new_entry.value_type) {
                    decode_errors.push(StorageDecodeError {
                        key: (*key).to_string(),
                        snapshot_type: old_entry.value_type.clone(),
                        expected_type: new_entry.value_type.clone(),
                        message: format!(
                            "stored value for '{key}' is {} but the new contract expects {}",
                            old_entry.value_type, new_entry.value_type
                        ),
                    });
                }
                if old_entry.tier != new_entry.tier {
                    decode_errors.push(StorageDecodeError {
                        key: (*key).to_string(),
                        snapshot_type: old_entry.tier.to_string(),
                        expected_type: new_entry.tier.to_string(),
                        message: format!(
                            "key '{key}' moved from {} to {} storage",
                            old_entry.tier, new_entry.tier
                        ),
                    });
                }
            }
        }
    }

    for key in old_by_key.keys() {
        if !new_by_key.contains_key(key) {
            removed_keys.push((*key).to_string());
        }
    }

    let is_decodable = decode_errors.is_empty();

    StorageLayoutComparison {
        checked_keys: old_by_key.len(),
        added_keys,
        removed_keys,
        decode_errors,
        is_decodable,
    }
}

// ── In-memory rehearsal environment ──────────────────────────────────────────

/// Deterministic, in-memory stand-in for the upgraded contract.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RehearsalEnvironment {
    /// Contract under rehearsal.
    pub contract_id: String,
    /// WASM hash currently deployed (from the snapshot, when available).
    pub old_wasm_hash: String,
    /// WASM hash being rehearsed.
    pub new_wasm_hash: String,
    /// Ledger sequence the snapshot was captured at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_ledger: Option<u32>,
    /// Snapshot values keyed by storage key.
    #[serde(default)]
    pub storage: BTreeMap<String, Value>,
    /// Storage layout observed in the snapshot (the "old" layout).
    #[serde(default)]
    pub observed_layout: Vec<StorageLayoutEntry>,
    /// Storage layout the new contract expects (the "new" layout).
    #[serde(default)]
    pub expected_layout: Vec<StorageLayoutEntry>,
}

impl RehearsalEnvironment {
    /// Build an environment from an optional snapshot and the new layout.
    pub fn from_snapshot(
        snapshot: Option<&LedgerSnapshot>,
        contract_id: &str,
        old_wasm_hash: &str,
        new_wasm_hash: &str,
        expected_layout: &[StorageLayoutEntry],
    ) -> Self {
        let mut storage = BTreeMap::new();
        let mut observed_layout = Vec::new();
        let mut snapshot_ledger = None;

        if let Some(snapshot) = snapshot {
            snapshot_ledger = snapshot.ledger;
            for entry in &snapshot.entries {
                storage.insert(entry.key.clone(), entry.value.clone());
                observed_layout.push(StorageLayoutEntry {
                    key: entry.key.clone(),
                    value_type: entry.value_type.clone(),
                    tier: entry.tier,
                });
            }
        }

        Self {
            contract_id: contract_id.to_string(),
            old_wasm_hash: old_wasm_hash.to_string(),
            new_wasm_hash: new_wasm_hash.to_string(),
            snapshot_ledger,
            storage,
            observed_layout,
            expected_layout: expected_layout.to_vec(),
        }
    }
}

// ── Call simulation ──────────────────────────────────────────────────────────

/// Status of a single scripted call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallStatus {
    /// The call executed and matched its expectation (or had none).
    Passed,
    /// The call executed but returned an unexpected value.
    Failed,
    /// The call could not be executed in the rehearsal environment.
    Error,
}

/// Result of replaying a single [`RehearsalCall`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallOutcome {
    /// Label from the script.
    pub name: String,
    /// Function that was invoked.
    pub function: String,
    /// Arguments that were passed.
    #[serde(default)]
    pub args: Vec<Value>,
    /// Pass/fail/error status.
    pub status: CallStatus,
    /// Expected value, when the script declared one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    /// Observed value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<Value>,
    /// Execution error, when the call could not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Replay one call against the in-memory environment.
///
/// The rehearsal engine does not execute WASM; it resolves a call to the
/// snapshot value of the matching storage key (`total_supply`, `get_admin`,
/// `read_balance`, …). Functions with no storage binding are treated as no-ops
/// that acknowledge success with `"ok"`.
pub fn simulate_call(
    env: &RehearsalEnvironment,
    call: &RehearsalCall,
) -> std::result::Result<Value, String> {
    for candidate in [call.function.as_str(), call.name.as_str()] {
        if let Some(value) = env.storage.get(candidate) {
            return Ok(value.clone());
        }
    }

    for prefix in ["get_", "read_"] {
        if let Some(key) = call.function.strip_prefix(prefix) {
            if let Some(value) = env.storage.get(key) {
                return Ok(value.clone());
            }
        }
    }

    Ok(Value::String("ok".to_string()))
}

// ── Report ───────────────────────────────────────────────────────────────────

/// Full rehearsal report, attachable to a governance proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RehearsalReport {
    /// Contract under rehearsal.
    pub contract_id: String,
    /// WASM hash currently deployed.
    pub old_wasm_hash: String,
    /// WASM hash being rehearsed.
    pub new_wasm_hash: String,
    /// Ledger sequence of the snapshot, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_ledger: Option<u32>,
    /// Storage decode comparison.
    pub storage: StorageLayoutComparison,
    /// Outcome of every scripted call.
    pub calls: Vec<CallOutcome>,
    /// Number of calls that passed.
    pub calls_passed: usize,
    /// Number of calls that failed or errored.
    pub calls_failed: usize,
    /// Overall pass/fail: decodable storage and no failed calls.
    pub passed: bool,
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unrenderable>".to_string())
}

fn render_args(args: &[Value]) -> String {
    args.iter().map(compact).collect::<Vec<_>>().join(", ")
}

impl RehearsalReport {
    /// Render a human-readable report (plain text, safe to paste anywhere).
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "Upgrade Rehearsal Report");
        let _ = writeln!(out, "========================");
        let _ = writeln!(out, "Contract:        {}", self.contract_id);
        let _ = writeln!(out, "Old WASM hash:   {}", self.old_wasm_hash);
        let _ = writeln!(out, "New WASM hash:   {}", self.new_wasm_hash);
        match self.snapshot_ledger {
            Some(ledger) => {
                let _ = writeln!(out, "Snapshot ledger: {ledger}");
            }
            None => {
                let _ = writeln!(out, "Snapshot ledger: (none)");
            }
        }
        let _ = writeln!(out);

        let _ = writeln!(
            out,
            "Storage decode check: {}",
            if self.storage.is_decodable {
                "PASS"
            } else {
                "FAIL"
            }
        );
        let _ = writeln!(out, "  keys checked: {}", self.storage.checked_keys);
        for key in &self.storage.added_keys {
            let _ = writeln!(out, "  + new key:     {key} (no snapshot value)");
        }
        for key in &self.storage.removed_keys {
            let _ = writeln!(out, "  - dropped key: {key} (snapshot value orphaned)");
        }
        for error in &self.storage.decode_errors {
            let _ = writeln!(out, "  ! {}: {}", error.key, error.message);
        }
        let _ = writeln!(out);

        let _ = writeln!(
            out,
            "Scripted calls: {}/{} passed",
            self.calls_passed,
            self.calls.len()
        );
        for call in &self.calls {
            let label = match call.status {
                CallStatus::Passed => "PASS",
                CallStatus::Failed => "FAIL",
                CallStatus::Error => "ERROR",
            };
            let _ = writeln!(out, "  [{label}] {}", call.name);
            let _ = writeln!(
                out,
                "         {}({})",
                call.function,
                render_args(&call.args)
            );
            if let Some(expected) = &call.expected {
                let _ = writeln!(out, "         expected: {}", compact(expected));
            }
            if let Some(actual) = &call.actual {
                let _ = writeln!(out, "         actual:   {}", compact(actual));
            }
            if let Some(error) = &call.error {
                let _ = writeln!(out, "         error:    {error}");
            }
        }
        let _ = writeln!(out);

        let _ = writeln!(
            out,
            "Overall: {}",
            if self.passed { "PASS" } else { "FAIL" }
        );
        out
    }

    /// Render the report as pretty JSON for machine consumption.
    pub fn render_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

/// Run a parsed script against an environment and produce the report.
pub fn run_rehearsal(script: &RehearsalScript, env: &RehearsalEnvironment) -> RehearsalReport {
    let storage = compare_storage_layout(&env.observed_layout, &env.expected_layout);

    let mut calls = Vec::with_capacity(script.calls.len());
    for call in &script.calls {
        let outcome = match simulate_call(env, call) {
            Ok(actual) => {
                let status = match &call.expect {
                    Some(expected) if *expected != actual => CallStatus::Failed,
                    _ => CallStatus::Passed,
                };
                CallOutcome {
                    name: call.name.clone(),
                    function: call.function.clone(),
                    args: call.args.clone(),
                    status,
                    expected: call.expect.clone(),
                    actual: Some(actual),
                    error: None,
                }
            }
            Err(error) => CallOutcome {
                name: call.name.clone(),
                function: call.function.clone(),
                args: call.args.clone(),
                status: CallStatus::Error,
                expected: call.expect.clone(),
                actual: None,
                error: Some(error),
            },
        };
        calls.push(outcome);
    }

    let calls_passed = calls
        .iter()
        .filter(|call| call.status == CallStatus::Passed)
        .count();
    let calls_failed = calls.len() - calls_passed;
    let passed = storage.is_decodable && calls_failed == 0;

    RehearsalReport {
        contract_id: env.contract_id.clone(),
        old_wasm_hash: env.old_wasm_hash.clone(),
        new_wasm_hash: env.new_wasm_hash.clone(),
        snapshot_ledger: env.snapshot_ledger,
        storage,
        calls,
        calls_passed,
        calls_failed,
        passed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAPSHOT_JSON: &str =
        include_str!("../../tests/fixtures/upgrade_rehearsal/snapshot.json");
    const SCRIPT_TOML: &str = include_str!("../../tests/fixtures/upgrade_rehearsal/rehearsal.toml");
    const BROKEN_SCRIPT_TOML: &str =
        include_str!("../../tests/fixtures/upgrade_rehearsal/rehearsal_broken.toml");

    fn snapshot() -> LedgerSnapshot {
        LedgerSnapshot::parse(SNAPSHOT_JSON).expect("fixture snapshot parses")
    }

    fn env_for(script: &RehearsalScript) -> RehearsalEnvironment {
        RehearsalEnvironment::from_snapshot(
            Some(&snapshot()),
            "CDEMOCONTRACT",
            "old-hash",
            "new-hash",
            &script.storage,
        )
    }

    #[test]
    fn parses_rehearsal_script_fixture() {
        let script = RehearsalScript::parse(SCRIPT_TOML).expect("script parses");
        assert_eq!(script.calls.len(), 3);
        assert_eq!(script.calls[0].function, "total_supply");
        assert_eq!(script.storage.len(), 2);
    }

    #[test]
    fn good_upgrade_rehearsal_passes() {
        let script = RehearsalScript::parse(SCRIPT_TOML).expect("script parses");
        let env = env_for(&script);
        let report = run_rehearsal(&script, &env);

        assert!(report.passed, "report:\n{}", report.render_text());
        assert!(report.storage.is_decodable);
        assert!(report.storage.decode_errors.is_empty());
        assert_eq!(report.calls_passed, script.calls.len());
        assert_eq!(report.calls_failed, 0);
    }

    #[test]
    fn deliberately_broken_storage_change_is_caught() {
        let script = RehearsalScript::parse(BROKEN_SCRIPT_TOML).expect("script parses");
        let env = env_for(&script);
        let report = run_rehearsal(&script, &env);

        assert!(!report.passed);
        assert!(!report.storage.is_decodable);
        assert!(report
            .storage
            .decode_errors
            .iter()
            .any(|error| error.key == "total_supply"));
    }

    #[test]
    fn compare_storage_layout_detects_type_change() {
        let old = vec![StorageLayoutEntry {
            key: "total_supply".to_string(),
            value_type: "u128".to_string(),
            tier: StorageTier::Persistent,
        }];
        let new = vec![StorageLayoutEntry {
            key: "total_supply".to_string(),
            value_type: "u32".to_string(),
            tier: StorageTier::Persistent,
        }];

        let diff = compare_storage_layout(&old, &new);
        assert!(!diff.is_decodable);
        assert_eq!(diff.decode_errors.len(), 1);
        assert_eq!(diff.decode_errors[0].snapshot_type, "u128");
        assert_eq!(diff.decode_errors[0].expected_type, "u32");
    }

    #[test]
    fn compare_storage_layout_reports_added_and_removed_keys() {
        let old = vec![
            StorageLayoutEntry {
                key: "a".to_string(),
                value_type: "u32".to_string(),
                tier: StorageTier::Instance,
            },
            StorageLayoutEntry {
                key: "b".to_string(),
                value_type: "u32".to_string(),
                tier: StorageTier::Instance,
            },
        ];
        let new = vec![
            StorageLayoutEntry {
                key: "a".to_string(),
                value_type: "u32".to_string(),
                tier: StorageTier::Instance,
            },
            StorageLayoutEntry {
                key: "c".to_string(),
                value_type: "u32".to_string(),
                tier: StorageTier::Instance,
            },
        ];

        let diff = compare_storage_layout(&old, &new);
        assert!(diff.is_decodable);
        assert_eq!(diff.added_keys, vec!["c".to_string()]);
        assert_eq!(diff.removed_keys, vec!["b".to_string()]);
    }

    #[test]
    fn compare_storage_layout_detects_tier_change() {
        let old = vec![StorageLayoutEntry {
            key: "admin".to_string(),
            value_type: "address".to_string(),
            tier: StorageTier::Instance,
        }];
        let new = vec![StorageLayoutEntry {
            key: "admin".to_string(),
            value_type: "address".to_string(),
            tier: StorageTier::Persistent,
        }];

        let diff = compare_storage_layout(&old, &new);
        assert!(!diff.is_decodable);
        assert_eq!(diff.decode_errors.len(), 1);
    }

    #[test]
    fn mismatched_call_expectation_fails() {
        let script = RehearsalScript::parse(
            r#"
[[calls]]
name = "supply_is_wrong"
function = "total_supply"
expect = "9999"
"#,
        )
        .expect("script parses");
        let env = env_for(&script);
        let report = run_rehearsal(&script, &env);

        assert!(!report.passed);
        assert_eq!(report.calls_failed, 1);
        assert_eq!(report.calls[0].status, CallStatus::Failed);
    }

    #[test]
    fn call_without_expectation_still_passes() {
        let script = RehearsalScript::parse(
            r#"
[[calls]]
name = "pause"
function = "set_paused"
args = [true]
"#,
        )
        .expect("script parses");
        let report = run_rehearsal(&script, &env_for(&script));

        assert!(report.passed);
        assert_eq!(report.calls[0].actual, Some(Value::String("ok".to_string())));
    }

    #[test]
    fn report_renders_text_and_json() {
        let script = RehearsalScript::parse(SCRIPT_TOML).expect("script parses");
        let env = env_for(&script);
        let report = run_rehearsal(&script, &env);

        let text = report.render_text();
        assert!(text.contains("Upgrade Rehearsal Report"));
        assert!(text.contains("Storage decode check: PASS"));
        assert!(text.contains("Overall: PASS"));

        let json = report.render_json().expect("json renders");
        let parsed: RehearsalReport = serde_json::from_str(&json).expect("json round-trips");
        assert_eq!(parsed, report);
    }

    #[test]
    fn empty_script_is_rejected() {
        assert!(RehearsalScript::parse("").is_err());
    }
}
