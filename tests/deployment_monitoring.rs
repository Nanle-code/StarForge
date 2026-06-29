//! Integration tests for the deployment monitoring service (D-70):
//! tracking, health assessment, failure detection, and dashboard summary.

use starforge::utils::deploy_history::{DeployRecord, DeployStatus};
use starforge::utils::deploy_monitor as dm;

fn record(
    id: &str,
    status: DeployStatus,
    contract: Option<&str>,
    network: &str,
    ts: &str,
) -> DeployRecord {
    DeployRecord {
        id: id.to_string(),
        contract_id: contract.map(|c| c.to_string()),
        wasm_path: "contract.wasm".to_string(),
        wasm_hash: "abc123".to_string(),
        network: network.to_string(),
        wallet: "deployer".to_string(),
        timestamp: ts.to_string(),
        status,
        error: None,
        previous_id: None,
        approved_by: None,
        verification_passed: false,
    }
}

fn epoch(ts: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(ts)
        .unwrap()
        .timestamp()
}

#[test]
fn tracking_health_failure_and_dashboard_pipeline() {
    let now = epoch("2026-03-01T01:00:00+00:00");

    let records = vec![
        record(
            "d1",
            DeployStatus::Success,
            Some("CLIVE"),
            "testnet",
            "2026-03-01T00:59:00+00:00",
        ),
        record(
            "d2",
            DeployStatus::Success,
            Some("CGONE"),
            "mainnet",
            "2026-03-01T00:58:00+00:00",
        ),
        record(
            "d3",
            DeployStatus::Failed,
            None,
            "testnet",
            "2026-03-01T00:30:00+00:00",
        ),
        record(
            "d4",
            DeployStatus::Pending,
            None,
            "testnet",
            "2026-03-01T00:00:00+00:00",
        ),
    ];

    // Health assessment with deterministic probes (no network access).
    let healths: Vec<dm::DeploymentHealth> = records
        .iter()
        .map(|r| {
            let probe = match r.id.as_str() {
                "d1" => dm::LivenessProbe {
                    network_reachable: true,
                    contract_live: Some(true),
                    age_secs: 60,
                },
                "d2" => dm::LivenessProbe {
                    network_reachable: true,
                    contract_live: Some(false),
                    age_secs: 120,
                },
                "d3" => dm::LivenessProbe {
                    network_reachable: true,
                    contract_live: None,
                    age_secs: 1800,
                },
                _ => dm::LivenessProbe {
                    network_reachable: true,
                    contract_live: None,
                    age_secs: dm::STUCK_PENDING_SECS + 60,
                },
            };
            dm::assess_health(r, &probe)
        })
        .collect();

    assert_eq!(healths[0].health, dm::HealthStatus::Healthy); // live contract
    assert_eq!(healths[1].health, dm::HealthStatus::Unhealthy); // missing on-chain
    assert_eq!(healths[2].health, dm::HealthStatus::Unhealthy); // failed deploy
    assert_eq!(healths[3].health, dm::HealthStatus::Unhealthy); // stuck pending

    // Aggregate dashboard summary.
    let summary = dm::summarize(&healths);
    assert_eq!(summary.total, 4);
    assert_eq!(summary.healthy, 1);
    assert_eq!(summary.unhealthy, 3);
    assert_eq!(summary.by_network["testnet"].healthy, 1);
    assert_eq!(summary.by_network["mainnet"].unhealthy, 1);
    assert!((summary.health_rate() - 25.0).abs() < f64::EPSILON);

    // Failure detection over the same history.
    let failures = dm::detect_failures(&records, now);
    assert_eq!(failures.len(), 2); // d3 failed + d4 stuck pending
    assert!(failures.iter().any(|f| f.kind == "deploy-failed"));
    assert!(failures.iter().any(|f| f.kind == "stuck-pending"));
}

#[test]
fn status_change_tracking_for_real_time_updates() {
    let healths = vec![dm::DeploymentHealth {
        deployment_id: "d1".into(),
        contract_id: Some("C1".into()),
        network: "testnet".into(),
        deploy_status: DeployStatus::Success,
        health: dm::HealthStatus::Unhealthy,
        checks: vec![],
        checked_at: "t".into(),
    }];

    // Previously healthy -> now unhealthy is reported as a transition.
    let previous = dm::snapshot_from(&[dm::DeploymentHealth {
        deployment_id: "d1".into(),
        contract_id: Some("C1".into()),
        network: "testnet".into(),
        deploy_status: DeployStatus::Success,
        health: dm::HealthStatus::Healthy,
        checks: vec![],
        checked_at: "t0".into(),
    }]);

    let changes = dm::diff_snapshot(&previous, &healths);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].from, Some(dm::HealthStatus::Healthy));
    assert_eq!(changes[0].to, dm::HealthStatus::Unhealthy);

    // A second pass with no change yields no transitions.
    let same = dm::snapshot_from(&healths);
    assert!(dm::diff_snapshot(&same, &healths).is_empty());
}
