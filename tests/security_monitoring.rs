use serde_json::json;
use starforge::utils::security::{
    anomaly::AnomalyDetector, default_rules, evaluate_event, threat_intel::ThreatFeed,
    IncidentStore,
};
use tempfile::TempDir;

#[test]
fn security_event_rules_detect_admin_changes() {
    let rules = default_rules();
    let events = evaluate_event(
        &rules,
        "CABC123",
        100,
        "evt-1",
        &["admin".into()],
        &json!({"action": "set_admin", "new_admin": "GAAA"}),
    );
    assert!(!events.is_empty());
    assert_eq!(events[0].rule_id, "admin-change");
}

#[test]
fn anomaly_detector_flags_rate_spike() {
    let mut detector = AnomalyDetector::new("CABC123");
    for _ in 0..20 {
        detector.record_event(None);
    }
    let finding = detector.record_event(None);
    assert!(finding.is_some());
}

#[test]
fn threat_intel_matches_known_patterns() {
    let feed = ThreatFeed::default_feed();
    let matches = feed.match_event("possible drain attack detected");
    assert!(!matches.is_empty());
}

#[test]
fn incident_store_create_and_list() {
    let _home_guard = home_lock();
    let home = TempDir::new().unwrap();
    std::env::set_var("HOME", home.path());

    let incident = IncidentStore::create(
        "CABC123",
        "high",
        "Test incident",
        "Automated test incident",
    )
    .unwrap();
    let all = IncidentStore::load_all().unwrap();
    assert!(all.iter().any(|i| i.id == incident.id));
}

/// Serialises tests that replace the process-wide `HOME`.
///
/// `std::env::set_var` affects every thread in the binary while libtest runs
/// these tests in parallel, so without this two tests race and one reads back
/// paths under the other's temp home.
#[allow(dead_code)]
fn home_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
