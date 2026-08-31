//! Integration tests for multisig signer/threshold change detection and the
//! append-only signed audit log.
//!
//! Covers:
//! - Diff detection against fixture JSON files (add / remove / weight /
//!   threshold changes)
//! - Append-only audit log integrity (hash chain) and tamper detection
//! - Monitoring / unexpected-change alerts (with and without a baseline)

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use starforge::utils::multisig::Thresholds;
use starforge::utils::multisig_audit::{
    diff_signer_states, inspect_signer_set, read_audit_log, verify_audit_log, ChangeKind,
    MonitoringBaseline, SignerSetDiff, SignerState,
};

static HOME_LOCK: Mutex<()> = Mutex::new(());

/// Serialises tests that replace the process-wide `HOME` / `USERPROFILE`.
///
/// The returned guard must stay alive for the duration of the test so later
/// tests cannot repoint `HOME` while this one is still doing file I/O.
fn home_lock(home: &Path) -> std::sync::MutexGuard<'static, ()> {
    let guard = HOME_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    std::env::set_var("HOME", home);
    std::env::set_var("USERPROFILE", home);
    guard
}

/// Resolve the home directory the same way the audit module does: honour the
/// `HOME` / `USERPROFILE` env vars set by [`home_lock`], falling back to
/// `dirs::home_dir()`.
fn test_home_dir() -> PathBuf {
    for var in ["USERPROFILE", "HOME"] {
        if let Some(v) = std::env::var_os(var) {
            if let Some(s) = v.to_str() {
                if !s.is_empty() && !s.trim().is_empty() {
                    return PathBuf::from(s);
                }
            }
        }
    }
    dirs::home_dir().expect("home directory")
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("multisig_signer_state")
}

fn fixture_path(name: &str) -> PathBuf {
    fixture_dir().join(name)
}

fn load_state(name: &str) -> SignerState {
    let contents = std::fs::read_to_string(fixture_path(name))
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", name, e));
    serde_json::from_str(&contents).expect("fixture must deserialize as SignerState")
}

fn load_baseline() -> MonitoringBaseline {
    let contents = std::fs::read_to_string(fixture_path("baseline.json")).unwrap();
    serde_json::from_str(&contents).expect("baseline fixture must deserialize")
}

fn signer_keys(signers: &[starforge::utils::multisig::Signer]) -> Vec<&str> {
    signers.iter().map(|s| s.public_key.as_str()).collect()
}

// ── Diff detection against fixtures ──────────────────────────────────────────

#[test]
fn fixtures_diff_detects_added_signer() {
    let previous = load_state("escrow_testnet.json");
    let current = load_state("escrow_testnet_added.json");
    let diff = diff_signer_states(&previous, &current).unwrap();
    assert!(diff.changed());
    assert_eq!(diff.added.len(), 1, "exactly one signer should be added");
    assert_eq!(signer_keys(&diff.added), vec!["GARBITERSIGNER01"]);
    assert!(diff.removed.is_empty());
    assert!(diff.weights_changed.is_empty());
    assert!(diff.thresholds_changed.is_empty());
}

#[test]
fn fixtures_diff_detects_removed_signer() {
    let previous = load_state("escrow_testnet.json");
    let current = load_state("escrow_testnet_removed.json");
    let diff = diff_signer_states(&previous, &current).unwrap();
    assert!(diff.changed());
    assert_eq!(
        diff.removed.len(),
        1,
        "exactly one signer should be removed"
    );
    assert_eq!(signer_keys(&diff.removed), vec!["GSELLERSIGNER001"]);
    assert!(diff.added.is_empty());
}

#[test]
fn fixtures_diff_detects_threshold_changes() {
    let previous = load_state("escrow_testnet_added.json");
    let current = load_state("escrow_testnet_threshold.json");
    let diff = diff_signer_states(&previous, &current).unwrap();
    assert!(diff.changed());
    assert!(diff.removed.is_empty());
    assert!(diff.added.is_empty());
    assert_eq!(diff.thresholds_changed.len(), 2, "low and high changed");
    assert!(diff
        .thresholds_changed
        .iter()
        .any(|t| t.level == "high" && t.old_value == 2 && t.new_value == 3));
    assert!(diff
        .thresholds_changed
        .iter()
        .any(|t| t.level == "low" && t.old_value == 1 && t.new_value == 2));
}

#[test]
fn fixtures_diff_unchanged_for_identical_state() {
    let previous = load_state("escrow_testnet.json");
    let current = load_state("escrow_testnet.json");
    let diff = diff_signer_states(&previous, &current).unwrap();
    assert!(!diff.changed());
}

#[test]
fn fixtures_diff_rejects_different_accounts() {
    let mut previous = load_state("escrow_testnet.json");
    previous.account_id = "GSOMEOTHERACCOUNT1".to_string();
    let current = load_state("escrow_testnet.json");
    assert!(diff_signer_states(&previous, &current).is_err());
}

#[test]
fn changed_flag_true_only_with_changes() {
    let empty = SignerSetDiff {
        account_id: "GAAA".to_string(),
        network: "testnet".to_string(),
        added: vec![],
        removed: vec![],
        weights_changed: vec![],
        thresholds_changed: vec![],
        master_weight_changed: None,
    };
    assert!(!empty.changed());
}

// ── End-to-end inspection pipeline ───────────────────────────────────────────

#[test]
fn inspection_records_add_and_remove_with_intact_chain() {
    let home = tempfile::tempdir().expect("temp home");
    let _guard = home_lock(home.path());

    let mut baseline = load_state("escrow_testnet.json");
    baseline.account_id = "GE2EACCOUNT0000001".to_string();

    // First observation establishes the baseline (no change).
    let outcome = inspect_signer_set(&baseline, None, false).unwrap();
    assert!(!outcome.changed);

    // Observe an added signer.
    let mut grown = load_state("escrow_testnet_added.json");
    grown.account_id = baseline.account_id.clone();
    let outcome = inspect_signer_set(&grown, None, true).unwrap();
    assert!(outcome.changed);
    assert!(
        outcome.alert,
        "no baseline -> add while monitoring is an alert"
    );
    assert_eq!(outcome.diff.added.len(), 1);

    // Observe a removed signer.
    let mut shrunk = load_state("escrow_testnet.json");
    shrunk.account_id = baseline.account_id.clone();
    shrunk.signers = baseline.signers.clone();
    shrunk.thresholds = Thresholds {
        low: 1,
        medium: 2,
        high: 2,
    };
    let outcome = inspect_signer_set(&shrunk, None, false).unwrap();
    assert!(outcome.changed);
    assert_eq!(outcome.diff.removed.len(), 1, "arbiter should be removed");

    // The audit log must contain both event kinds and stay intact.
    let records = read_audit_log().unwrap();
    assert!(records.iter().any(|r| r.kind == ChangeKind::AddSigner));
    assert!(records.iter().any(|r| r.kind == ChangeKind::RemoveSigner));
    assert!(verify_audit_log(&records).is_empty());
}

#[test]
fn audit_log_integrity_detects_tampering() {
    let home = tempfile::tempdir().expect("temp home");
    let _guard = home_lock(home.path());

    let mut baseline = load_state("escrow_testnet.json");
    baseline.account_id = "GTAMPERACCOUNT0001".to_string();
    inspect_signer_set(&baseline, None, false).unwrap();

    let mut grown = load_state("escrow_testnet_added.json");
    grown.account_id = baseline.account_id.clone();
    inspect_signer_set(&grown, None, false).unwrap();

    let log_path = test_home_dir()
        .join(".starforge")
        .join("audit")
        .join("multisig_signer_changes.jsonl");
    let contents = std::fs::read_to_string(&log_path)
        .unwrap_or_else(|e| panic!("audit log not found at {}: {}", log_path.display(), e));
    let tampered = contents.replacen("GARBITERSIGNER01", "GARBITERSIGNER99", 1);
    assert_ne!(tampered, contents, "tamper must change the log");
    std::fs::write(&log_path, tampered).unwrap();

    let records = read_audit_log().unwrap();
    assert!(
        !verify_audit_log(&records).is_empty(),
        "tampered log must fail verification"
    );
}

#[test]
fn monitoring_suppresses_alert_when_baseline_match_is_expected() {
    let home = tempfile::tempdir().expect("temp home");
    let _guard = home_lock(home.path());

    let baseline_cfg = load_baseline();
    let mut initial = load_state("escrow_testnet.json");
    initial.account_id = "GBASELINEACCOUNT0001".to_string();
    inspect_signer_set(&initial, Some(&baseline_cfg), true).unwrap();

    // Adding the arbiter matches the baseline signer set -> not unexpected.
    let mut expected = load_state("escrow_testnet_added.json");
    expected.account_id = initial.account_id.clone();
    let outcome = inspect_signer_set(&expected, Some(&baseline_cfg), true).unwrap();
    assert!(outcome.changed);
    assert!(!outcome.alert, "change matching baseline must not alert");
    assert!(outcome.alerts.is_empty());

    // Adding a random stranger deviates from the baseline -> unexpected.
    let mut rogue = expected.clone();
    rogue.signers.push(starforge::utils::multisig::Signer {
        public_key: "GSTRANGERKEY000001".to_string(),
        weight: 1,
        name: None,
    });
    let outcome = inspect_signer_set(&rogue, Some(&baseline_cfg), true).unwrap();
    assert!(outcome.changed);
    assert!(outcome.alert);
    assert!(outcome
        .alerts
        .iter()
        .any(|a| a.contains("GSTRANGERKEY000001")));
}

#[test]
fn baseline_without_monitoring_records_but_does_not_alert() {
    let home = tempfile::tempdir().expect("temp home");
    let _guard = home_lock(home.path());

    let mut baseline = load_state("escrow_testnet.json");
    baseline.account_id = "GNOMONITORACCOUNT01".to_string();
    inspect_signer_set(&baseline, None, false).unwrap();

    let mut grown = load_state("escrow_testnet_added.json");
    grown.account_id = baseline.account_id.clone();
    let outcome = inspect_signer_set(&grown, None, false).unwrap();
    assert!(outcome.changed);
    assert!(
        !outcome.alert,
        "monitoring disabled -> no alert even though changed"
    );
    assert!(outcome.alerts.is_empty());

    let records = read_audit_log().unwrap();
    assert!(records.iter().any(|r| r.kind == ChangeKind::AddSigner));
    assert!(
        !records.last().unwrap().alert,
        "record added without monitoring is not flagged"
    );
}
