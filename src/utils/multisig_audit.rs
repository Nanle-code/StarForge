//! Multisig signer/threshold change detection and audit logging.
//!
//! Detects changes to an account's signer set (adds, removes, weight edits)
//! and thresholds by diffing polled/inspected signer snapshots against the
//! last known state, then records every change into a local **append-only**
//! integrity-protected audit log for operators.
//!
//! # Integrity model
//!
//! The audit log is a newline-delimited JSON file where every record stores a
//! `sha256` digest of its own canonical payload plus the digest of the record
//! that preceded it (`prev_hash`). This forms a hash chain: rewriting,
//! deleting, reordering, or appending to any record can be detected by
//! [`verify_audit_log`]. The log file is only ever opened in append mode.
//!
//! # Monitoring
//!
//! When monitoring is enabled with an optional [`MonitoringBaseline`], a
//! change that deviates from the operator's declared expected configuration is
//! flagged as *unexpected* and surfaced as an alert. Without a baseline, any
//! change is treated as unexpected while monitoring is on.

use crate::utils::multisig::{Signer, Thresholds};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// Hash of an empty prefix; used as `prev_hash` for the first log record.
const PREV_HASH_EMPTY: &str = "0";

/// A snapshot of an account's signer configuration. This is the "last known"
/// state that every inspection is diffed against.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignerState {
    pub account_id: String,
    pub network: String,
    pub master_weight: u8,
    pub thresholds: Thresholds,
    pub signers: Vec<Signer>,
    pub captured_at: String,
}

/// A single signer weight change: `public_key` moved from `old_weight` to
/// `new_weight`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeightChange {
    pub public_key: String,
    pub old_weight: u8,
    pub new_weight: u8,
}

/// A single threshold change on one of the three weight levels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThresholdChange {
    /// One of `"low"`, `"medium"`, `"high"`.
    pub level: String,
    pub old_value: u8,
    pub new_value: u8,
}

/// The result of diffing two [`SignerState`] snapshots for the same account.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignerSetDiff {
    pub account_id: String,
    pub network: String,
    pub added: Vec<Signer>,
    pub removed: Vec<Signer>,
    pub weights_changed: Vec<WeightChange>,
    pub thresholds_changed: Vec<ThresholdChange>,
    pub master_weight_changed: Option<(u8, u8)>,
}

impl SignerSetDiff {
    /// True when at least one discrete change was detected.
    pub fn changed(&self) -> bool {
        !self.added.is_empty()
            || !self.removed.is_empty()
            || !self.weights_changed.is_empty()
            || !self.thresholds_changed.is_empty()
            || self.master_weight_changed.is_some()
    }
}

/// Category of a change recorded in the audit log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// First observation; a baseline was established (no prior state existed).
    Baseline,
    AddSigner,
    RemoveSigner,
    WeightChange,
    ThresholdChange,
    MasterWeightChange,
}

/// One line of the append-only audit log.
///
/// `seq` is monotonically increasing, `prev_hash` chains to the previous
/// record, and `hash` is the integrity digest of this record's payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditLogRecord {
    pub seq: u64,
    /// RFC3339 UTC timestamp of the observation.
    pub at: String,
    pub account_id: String,
    pub network: String,
    pub kind: ChangeKind,
    pub added: Vec<Signer>,
    pub removed: Vec<Signer>,
    pub weights_changed: Vec<WeightChange>,
    pub thresholds_changed: Vec<ThresholdChange>,
    pub master_weight_changed: Option<(u8, u8)>,
    /// True when this change was flagged as unexpected while monitoring.
    pub alert: bool,
    /// Optional alert message(s) attached to the record.
    pub note: Option<String>,
    /// Hash of the preceding record; `"0"` for the first record.
    pub prev_hash: String,
    /// Integrity digest of this record.
    pub hash: String,
}

impl AuditLogRecord {
    /// Canonical payload bytes this record's `hash` is computed over. The
    /// stored `hash` itself is intentionally excluded.
    fn content_bytes(&self) -> Vec<u8> {
        let payload = serde_json::json!({
            "seq": self.seq,
            "at": self.at,
            "account_id": self.account_id,
            "network": self.network,
            "kind": self.kind,
            "added": self.added,
            "removed": self.removed,
            "weights_changed": self.weights_changed,
            "thresholds_changed": self.thresholds_changed,
            "master_weight_changed": self.master_weight_changed,
            "alert": self.alert,
            "note": self.note,
            "prev_hash": self.prev_hash,
        });
        serde_json::to_vec(&payload).expect("serializing an audit record is infallible")
    }

    /// SHA-256 digest of the record's canonical payload.
    pub fn compute_hash(&self) -> String {
        hex::encode(Sha256::digest(self.content_bytes()))
    }

    fn new_baseline(state: &SignerState) -> Self {
        AuditLogRecord {
            seq: 0,
            at: state.captured_at.clone(),
            account_id: state.account_id.clone(),
            network: state.network.clone(),
            kind: ChangeKind::Baseline,
            added: Vec::new(),
            removed: Vec::new(),
            weights_changed: Vec::new(),
            thresholds_changed: Vec::new(),
            master_weight_changed: None,
            alert: false,
            note: Some("first observation; baseline established".to_string()),
            prev_hash: PREV_HASH_EMPTY.to_string(),
            hash: String::new(),
        }
    }
}

/// Declared expected signer configuration used by monitoring to distinguish
/// expected maintenance from unexpected (potentially malicious) changes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonitoringBaseline {
    /// The expected signer set. Empty means "no signer baseline configured".
    #[serde(default)]
    pub signers: Vec<Signer>,
    /// The expected thresholds. `None` means thresholds are not restricted.
    pub thresholds: Option<Thresholds>,
    /// The expected master weight. `None` means not restricted.
    pub master_weight: Option<u8>,
}

/// Result of running an inspection against the audit pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectionOutcome {
    pub changed: bool,
    pub diff: SignerSetDiff,
    /// True when monitoring is enabled and an unexpected change was detected.
    pub alert: bool,
    pub alerts: Vec<String>,
}

/// Diff two signer snapshots for the same account and network.
pub fn diff_signer_states(previous: &SignerState, current: &SignerState) -> Result<SignerSetDiff> {
    if previous.account_id != current.account_id {
        bail!(
            "Cannot diff signer states for different accounts: {} vs {}",
            previous.account_id,
            current.account_id
        );
    }
    if previous.network != current.network {
        bail!(
            "Cannot diff signer states on different networks: {} vs {}",
            previous.network,
            current.network
        );
    }

    let prev_by_key: HashMap<&str, &Signer> = previous
        .signers
        .iter()
        .map(|s| (s.public_key.as_str(), s))
        .collect();
    let curr_by_key: HashMap<&str, &Signer> = current
        .signers
        .iter()
        .map(|s| (s.public_key.as_str(), s))
        .collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut weights_changed = Vec::new();

    for (key, prev_signer) in &prev_by_key {
        match curr_by_key.get(key) {
            None => removed.push((*prev_signer).clone()),
            Some(curr_signer) => {
                if curr_signer.weight != prev_signer.weight {
                    weights_changed.push(WeightChange {
                        public_key: key.to_string(),
                        old_weight: prev_signer.weight,
                        new_weight: curr_signer.weight,
                    });
                }
            }
        }
    }
    for (key, curr_signer) in &curr_by_key {
        if !prev_by_key.contains_key(key) {
            added.push((*curr_signer).clone());
        }
    }
    added.sort_by(|a, b| a.public_key.cmp(&b.public_key));
    removed.sort_by(|a, b| a.public_key.cmp(&b.public_key));
    weights_changed.sort_by(|a, b| a.public_key.cmp(&b.public_key));

    let mut thresholds_changed = Vec::new();
    for (level, old_value, new_value) in [
        ("low", previous.thresholds.low, current.thresholds.low),
        (
            "medium",
            previous.thresholds.medium,
            current.thresholds.medium,
        ),
        ("high", previous.thresholds.high, current.thresholds.high),
    ] {
        if old_value != new_value {
            thresholds_changed.push(ThresholdChange {
                level: level.to_string(),
                old_value,
                new_value,
            });
        }
    }

    let master_weight_changed = if previous.master_weight != current.master_weight {
        Some((previous.master_weight, current.master_weight))
    } else {
        None
    };

    Ok(SignerSetDiff {
        account_id: previous.account_id.clone(),
        network: previous.network.clone(),
        added,
        removed,
        weights_changed,
        thresholds_changed,
        master_weight_changed,
    })
}

/// Classify a diff against a monitoring baseline. With no baseline, every
/// change is considered unexpected. With a baseline, only deviations from the
/// declared configuration are unexpected.
pub fn unexpected_change_alerts(
    diff: &SignerSetDiff,
    baseline: Option<&MonitoringBaseline>,
) -> Vec<String> {
    let mut alerts = Vec::new();
    if !diff.changed() {
        return alerts;
    }

    match baseline {
        None => {
            for signer in &diff.added {
                alerts.push(format!("unexpected signer added: {}", signer.public_key));
            }
            for signer in &diff.removed {
                alerts.push(format!("unexpected signer removed: {}", signer.public_key));
            }
            for change in &diff.weights_changed {
                alerts.push(format!(
                    "unexpected weight change for {}: {} -> {}",
                    change.public_key, change.old_weight, change.new_weight
                ));
            }
            for change in &diff.thresholds_changed {
                alerts.push(format!(
                    "unexpected {} threshold change: {} -> {}",
                    change.level, change.old_value, change.new_value
                ));
            }
            if let Some((old, new)) = diff.master_weight_changed {
                alerts.push(format!(
                    "unexpected master weight change: {} -> {}",
                    old, new
                ));
            }
        }
        Some(baseline) => {
            if !baseline.signers.is_empty() {
                let expected_keys: HashSet<&str> = baseline
                    .signers
                    .iter()
                    .map(|s| s.public_key.as_str())
                    .collect();
                for signer in &diff.added {
                    if !expected_keys.contains(signer.public_key.as_str()) {
                        alerts.push(format!("unexpected signer added: {}", signer.public_key));
                    }
                }
                for signer in &diff.removed {
                    if expected_keys.contains(signer.public_key.as_str()) {
                        alerts.push(format!("expected signer removed: {}", signer.public_key));
                    }
                }
                for change in &diff.weights_changed {
                    let deviates = baseline
                        .signers
                        .iter()
                        .find(|s| s.public_key == change.public_key)
                        .map(|baseline_signer| baseline_signer.weight != change.old_weight)
                        .unwrap_or(false);
                    if deviates {
                        alerts.push(format!(
                            "signer {} weight {} deviates from baseline {}",
                            change.public_key, change.new_weight, change.old_weight
                        ));
                    }
                }
            }
            if let Some(thresholds) = &baseline.thresholds {
                for change in &diff.thresholds_changed {
                    let expected = match change.level.as_str() {
                        "low" => thresholds.low,
                        "medium" => thresholds.medium,
                        _ => thresholds.high,
                    };
                    if expected != change.old_value {
                        alerts.push(format!(
                            "{} threshold change {} -> {} deviates from baseline {}",
                            change.level, change.old_value, change.new_value, expected
                        ));
                    }
                }
            }
            if let Some(expected) = baseline.master_weight {
                if let Some((old, new)) = diff.master_weight_changed {
                    if expected != old {
                        alerts.push(format!(
                            "master weight change {} -> {} deviates from baseline {}",
                            old, new, expected
                        ));
                    }
                }
            }
        }
    }
    alerts
}

/// Build one audit record per changed category from a diff.
pub fn build_change_records(
    diff: &SignerSetDiff,
    alert: bool,
    alerts: &[String],
) -> Vec<AuditLogRecord> {
    let note = if alert { Some(alerts.join("; ")) } else { None };
    let base = || AuditLogRecord {
        seq: 0,
        at: Utc::now().to_rfc3339(),
        account_id: diff.account_id.clone(),
        network: diff.network.clone(),
        kind: ChangeKind::WeightChange,
        added: Vec::new(),
        removed: Vec::new(),
        weights_changed: Vec::new(),
        thresholds_changed: Vec::new(),
        master_weight_changed: None,
        alert,
        note: note.clone(),
        prev_hash: String::new(),
        hash: String::new(),
    };

    let mut records = Vec::new();
    if !diff.added.is_empty() {
        let mut record = base();
        record.kind = ChangeKind::AddSigner;
        record.added = diff.added.clone();
        records.push(record);
    }
    if !diff.removed.is_empty() {
        let mut record = base();
        record.kind = ChangeKind::RemoveSigner;
        record.removed = diff.removed.clone();
        records.push(record);
    }
    if !diff.weights_changed.is_empty() {
        let mut record = base();
        record.kind = ChangeKind::WeightChange;
        record.weights_changed = diff.weights_changed.clone();
        records.push(record);
    }
    if !diff.thresholds_changed.is_empty() {
        let mut record = base();
        record.kind = ChangeKind::ThresholdChange;
        record.thresholds_changed = diff.thresholds_changed.clone();
        records.push(record);
    }
    if let Some(master) = diff.master_weight_changed {
        let mut record = base();
        record.kind = ChangeKind::MasterWeightChange;
        record.master_weight_changed = Some(master);
        records.push(record);
    }
    records
}

// ---------------------------------------------------------------------------
// Append-only audit log
// ---------------------------------------------------------------------------

fn audit_log_file() -> Result<PathBuf> {
    Ok(crate::utils::audit::audit_dir()?.join("multisig_signer_changes.jsonl"))
}

/// Append a record to the append-only audit log, filling in `seq`, `prev_hash`,
/// and the integrity `hash`. The file is only ever opened in append mode.
pub fn append_audit_record(record: &mut AuditLogRecord) -> Result<()> {
    let path = audit_log_file()?;
    let previous = read_audit_log()?.pop();
    record.seq = previous.as_ref().map(|r| r.seq + 1).unwrap_or(1);
    record.prev_hash = previous
        .map(|r| r.hash)
        .unwrap_or_else(|| PREV_HASH_EMPTY.to_string());
    record.hash = record.compute_hash();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("Failed to open {} for append", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(record)?)
        .with_context(|| format!("Failed to append to {}", path.display()))?;
    file.flush()?;
    Ok(())
}

/// Read every record currently in the append-only audit log.
pub fn read_audit_log() -> Result<Vec<AuditLogRecord>> {
    let path = audit_log_file()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file =
        fs::File::open(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let mut records = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        records.push(serde_json::from_str(&line)?);
    }
    Ok(records)
}

/// Verify the integrity of every record as currently stored: each record's
/// self-hash must match its payload and the `prev_hash` chain must be intact.
/// Returns a list of human-readable problems; an empty list means the log is
/// intact.
pub fn verify_audit_log(records: &[AuditLogRecord]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut expected_prev = PREV_HASH_EMPTY.to_string();
    for record in records.iter() {
        if record.compute_hash() != record.hash {
            problems.push(format!(
                "record {} self-hash mismatch (payload tampered?)",
                record.seq
            ));
        }
        if record.prev_hash != expected_prev {
            problems.push(format!(
                "record {} has broken chain: expected prev_hash {}, found {}",
                record.seq, expected_prev, record.prev_hash
            ));
        }
        expected_prev = record.hash.clone();
    }
    problems
}

// ---------------------------------------------------------------------------
// Last-known state persistence
// ---------------------------------------------------------------------------

fn state_dir() -> Result<PathBuf> {
    let dir = crate::utils::audit::audit_dir()?.join("multisig_state");
    fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    Ok(dir)
}

fn state_path(account_id: &str, network: &str) -> Result<PathBuf> {
    let key = format!("{}__{}", sanitize_key(network), sanitize_key(account_id));
    Ok(state_dir()?.join(format!("{}.json", key)))
}

fn sanitize_key(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Load the last known signer state for an account on a network, if any.
pub fn load_last_state(account_id: &str, network: &str) -> Result<Option<SignerState>> {
    let path = state_path(account_id, network)?;
    if !path.exists() {
        return Ok(None);
    }
    let contents =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(Some(serde_json::from_str(&contents)?))
}

/// Persist the current signer state as the new "last known" snapshot.
pub fn save_last_state(state: &SignerState) -> Result<()> {
    let path = state_path(&state.account_id, &state.network)?;
    fs::write(
        &path,
        serde_json::to_string_pretty(state).with_context(|| "Failed to serialize signer state")?,
    )
    .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Inspection pipeline
// ---------------------------------------------------------------------------

/// Inspect a freshly-observed signer state: diff it against the last known
/// snapshot, append change records to the audit log, persist the new state,
/// and — when monitoring is enabled — return alerts for unexpected changes.
///
/// The first observation for an account only establishes a baseline.
pub fn inspect_signer_set(
    current: &SignerState,
    baseline: Option<&MonitoringBaseline>,
    monitoring: bool,
) -> Result<InspectionOutcome> {
    let previous = load_last_state(&current.account_id, &current.network)?;

    let Some(previous) = previous else {
        let mut record = AuditLogRecord::new_baseline(current);
        append_audit_record(&mut record)?;
        save_last_state(current)?;
        let empty = SignerSetDiff {
            account_id: current.account_id.clone(),
            network: current.network.clone(),
            added: Vec::new(),
            removed: Vec::new(),
            weights_changed: Vec::new(),
            thresholds_changed: Vec::new(),
            master_weight_changed: None,
        };
        return Ok(InspectionOutcome {
            changed: false,
            diff: empty,
            alert: false,
            alerts: Vec::new(),
        });
    };

    let diff = diff_signer_states(&previous, current)?;
    save_last_state(current)?;

    if !diff.changed() {
        return Ok(InspectionOutcome {
            changed: false,
            diff,
            alert: false,
            alerts: Vec::new(),
        });
    }

    let alerts = if monitoring {
        unexpected_change_alerts(&diff, baseline)
    } else {
        Vec::new()
    };
    let alert = monitoring && !alerts.is_empty();
    for mut record in build_change_records(&diff, alert, &alerts) {
        append_audit_record(&mut record)?;
    }

    Ok(InspectionOutcome {
        changed: true,
        diff,
        alert,
        alerts,
    })
}

// ---------------------------------------------------------------------------
// Horizon inspection
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct HorizonAccountResponse {
    account_id: String,
    #[serde(rename = "master_weight")]
    master_weight: u32,
    thresholds: HorizonThresholds,
    signers: Vec<HorizonSigner>,
}

#[derive(Debug, Deserialize)]
struct HorizonThresholds {
    #[serde(rename = "low_threshold")]
    low: u32,
    #[serde(rename = "med_threshold")]
    med: u32,
    #[serde(rename = "high_threshold")]
    high: u32,
}

#[derive(Debug, Deserialize)]
struct HorizonSigner {
    key: String,
    weight: u32,
    #[serde(rename = "type")]
    signer_type: String,
}

fn weight_of(value: u32, what: &str) -> Result<u8> {
    u8::try_from(value).with_context(|| {
        format!(
            "{} {} on Stellar is outside the supported 0..=255 range",
            what, value
        )
    })
}

/// Fetch the current signer configuration for an account via Horizon.
///
/// The master signer entry is excluded from `signers` (its weight is captured
/// separately in `master_weight`) so the diff reflects sub-signer changes.
pub async fn fetch_signer_state(account_id: &str, network: &str) -> Result<SignerState> {
    let horizon = crate::utils::horizon::horizon_url(network)?;
    let url = format!("{}/accounts/{}", horizon.trim_end_matches('/'), account_id);
    let client: &Client = crate::utils::horizon::http_client();
    let response = client.get(&url).send().await.with_context(|| {
        format!(
            "Failed to reach Horizon for account {} on {}",
            account_id, network
        )
    })?;

    if response.status() == 404 {
        bail!(
            "Account '{}' not found on {}; it may not be activated yet",
            account_id,
            network
        );
    }
    if !response.status().is_success() {
        bail!(
            "Horizon returned HTTP {} for account '{}' on {}",
            response.status(),
            account_id,
            network
        );
    }

    let parsed: HorizonAccountResponse = response
        .json()
        .await
        .with_context(|| "Failed to parse Horizon account response")?;

    let signers = parsed
        .signers
        .into_iter()
        .filter(|signer| signer.key != parsed.account_id)
        .map(|signer| {
            Ok(Signer {
                public_key: signer.key,
                weight: weight_of(signer.weight, "signer weight")?,
                name: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(SignerState {
        account_id: parsed.account_id,
        network: network.to_string(),
        master_weight: weight_of(parsed.master_weight, "master weight")?,
        thresholds: Thresholds {
            low: weight_of(parsed.thresholds.low, "low threshold")?,
            medium: weight_of(parsed.thresholds.med, "medium threshold")?,
            high: weight_of(parsed.thresholds.high, "high threshold")?,
        },
        signers,
        captured_at: Utc::now().to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer(key: &str, weight: u8) -> Signer {
        Signer {
            public_key: key.to_string(),
            weight,
            name: None,
        }
    }

    fn state(account_id: &str, signers: Vec<Signer>) -> SignerState {
        SignerState {
            account_id: account_id.to_string(),
            network: "testnet".to_string(),
            master_weight: 1,
            thresholds: Thresholds {
                low: 1,
                medium: 2,
                high: 2,
            },
            signers,
            captured_at: "2026-08-01T00:00:00Z".to_string(),
        }
    }

    fn with_home<T>(f: impl FnOnce() -> T) -> T {
        let _guard = crate::utils::lock_home_env();
        let home = tempfile::tempdir().expect("temp home");
        std::env::set_var("HOME", home.path());
        std::env::set_var("USERPROFILE", home.path());
        f()
    }

    #[test]
    fn diff_detects_added_signer() {
        let previous = state("GAAA", vec![signer("GB01", 1), signer("GB02", 1)]);
        let current = state(
            "GAAA",
            vec![signer("GB01", 1), signer("GB02", 1), signer("GB03", 1)],
        );
        let diff = diff_signer_states(&previous, &current).unwrap();
        assert!(diff.changed());
        assert_eq!(diff.added, vec![signer("GB03", 1)]);
        assert!(diff.removed.is_empty());
    }

    #[test]
    fn diff_detects_removed_signer() {
        let previous = state(
            "GAAA",
            vec![signer("GB01", 1), signer("GB02", 1), signer("GB03", 1)],
        );
        let current = state("GAAA", vec![signer("GB01", 1), signer("GB02", 1)]);
        let diff = diff_signer_states(&previous, &current).unwrap();
        assert!(diff.changed());
        assert_eq!(diff.removed, vec![signer("GB03", 1)]);
        assert!(diff.added.is_empty());
    }

    #[test]
    fn diff_detects_weight_change() {
        let previous = state("GAAA", vec![signer("GB01", 1), signer("GB02", 1)]);
        let current = state("GAAA", vec![signer("GB01", 1), signer("GB02", 4)]);
        let diff = diff_signer_states(&previous, &current).unwrap();
        assert_eq!(
            diff.weights_changed,
            vec![WeightChange {
                public_key: "GB02".to_string(),
                old_weight: 1,
                new_weight: 4,
            }]
        );
    }

    #[test]
    fn diff_detects_threshold_change() {
        let previous = state("GAAA", vec![signer("GB01", 1)]);
        let mut current = state("GAAA", vec![signer("GB01", 1)]);
        current.thresholds.high = 3;
        let diff = diff_signer_states(&previous, &current).unwrap();
        assert_eq!(
            diff.thresholds_changed,
            vec![ThresholdChange {
                level: "high".to_string(),
                old_value: 2,
                new_value: 3,
            }]
        );
    }

    #[test]
    fn diff_is_empty_for_identical_states() {
        let previous = state("GAAA", vec![signer("GB01", 1)]);
        let current = previous.clone();
        let diff = diff_signer_states(&previous, &current).unwrap();
        assert!(!diff.changed());
    }

    #[test]
    fn diff_rejects_cross_account() {
        let previous = state("GAAA", vec![]);
        let current = state("GBBB", vec![]);
        assert!(diff_signer_states(&previous, &current).is_err());
    }

    #[test]
    fn audit_log_appends_and_verifies() {
        with_home(|| {
            let current = state("GAAA", vec![signer("GB01", 1), signer("GB02", 1)]);
            let outcome = inspect_signer_set(&current, None, true).unwrap();
            assert_eq!(outcome.changed, false); // first observation: baseline only

            let outcome = inspect_signer_set(&current, None, true).unwrap();
            assert_eq!(outcome.changed, false); // unchanged

            let grown = state(
                "GAAA",
                vec![signer("GB01", 1), signer("GB02", 1), signer("GB03", 1)],
            );
            let outcome = inspect_signer_set(&grown, None, true).unwrap();
            assert!(outcome.changed);
            assert!(outcome.alert);

            let records = read_audit_log().unwrap();
            assert!(records.iter().any(|r| r.kind == ChangeKind::Baseline));
            assert!(records.iter().any(|r| r.kind == ChangeKind::AddSigner));
            assert!(verify_audit_log(&records).is_empty());
        });
    }

    #[test]
    fn audit_log_detects_tamper() {
        with_home(|| {
            let current = state("GAAA", vec![signer("GB01", 1), signer("GB02", 1)]);
            // Baseline (first run) is what we exercise diffs against.
            inspect_signer_set(&current, None, false).unwrap();
            let changed = state(
                "GAAA",
                vec![signer("GB01", 1), signer("GB02", 1), signer("GB03", 1)],
            );
            inspect_signer_set(&changed, None, false).unwrap();

            let path = crate::utils::audit::audit_dir()
                .unwrap()
                .join("multisig_signer_changes.jsonl");
            let contents = fs::read_to_string(&path).unwrap();
            let tampered = contents.replacen("GB03", "GB99", 1);
            fs::write(&path, tampered).unwrap();

            let records = read_audit_log().unwrap();
            assert!(!verify_audit_log(&records).is_empty());
        });
    }

    #[test]
    fn monitoring_without_baseline_alerts_on_change() {
        with_home(|| {
            let baseline_state = state("HASH01", vec![signer("GB01", 1)]);
            inspect_signer_set(&baseline_state, None, true).unwrap();
            let changed = state("HASH01", vec![signer("GB01", 1), signer("GB02", 1)]);
            let outcome = inspect_signer_set(&changed, None, true).unwrap();
            assert!(outcome.changed);
            assert!(outcome.alert);
            assert!(!outcome.alerts.is_empty());
        });
    }

    #[test]
    fn baseline_matching_signers_suppresses_alerts() {
        with_home(|| {
            let baseline = MonitoringBaseline {
                signers: vec![signer("GB01", 1), signer("GB02", 1)],
                thresholds: None,
                master_weight: None,
            };
            let initial = state("HASH02", vec![signer("GB01", 1)]);
            inspect_signer_set(&initial, Some(&baseline), true).unwrap();
            let expected = state("HASH02", vec![signer("GB01", 1), signer("GB02", 1)]);
            let outcome = inspect_signer_set(&expected, Some(&baseline), true).unwrap();
            assert!(outcome.changed);
            assert!(!outcome.alert);
            assert!(outcome.alerts.is_empty());

            let unexpected = state("HASH02", vec![signer("GB01", 1), signer("GB99", 1)]);
            let outcome = inspect_signer_set(&unexpected, Some(&baseline), true).unwrap();
            assert!(outcome.changed);
            assert!(outcome.alert);
        });
    }

    #[test]
    fn recording_remove_signer_produces_remove_record() {
        with_home(|| {
            let initial = state("HASH03", vec![signer("GB01", 1), signer("GB02", 1)]);
            inspect_signer_set(&initial, None, false).unwrap();
            let after_removal = state("HASH03", vec![signer("GB01", 1)]);
            let outcome = inspect_signer_set(&after_removal, None, false).unwrap();
            assert!(outcome.changed);
            assert_eq!(outcome.diff.removed, vec![signer("GB02", 1)]);
            let records = read_audit_log().unwrap();
            assert!(records.iter().any(|r| r.kind == ChangeKind::RemoveSigner));
        });
    }
}
