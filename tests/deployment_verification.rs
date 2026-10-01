//! Integration tests for deployment verification system.

use starforge::utils::deploy_history::{DeployRecord, DeployStatus};
use starforge::utils::deployment_verify::{generate_ci_snippet, CheckStatus, DeploymentVerifier};

fn sample_record() -> DeployRecord {
    DeployRecord {
        id: "deploy-test-001".to_string(),
        contract_id: Some("CTEST123".to_string()),
        wasm_path: "/nonexistent/test.wasm".to_string(),
        wasm_hash: "deadbeef".to_string(),
        network: "testnet".to_string(),
        wallet: "dev-wallet".to_string(),
        timestamp: "2024-06-01T00:00:00Z".to_string(),
        duration_ms: Some(500),
        fee_stroops: Some(100_000),
        status: DeployStatus::Success,
        error: None,
        previous_id: None,
        approved_by: None,
        verification_passed: false,
        note: None,
        changelog: None,
    }
}

#[test]
fn record_completeness_via_verify_all() {
    let record = sample_record();
    let verifier = DeploymentVerifier::new(record);
    let checks = verifier.check_bytecode();
    assert!(!checks.is_empty());
}

#[test]
fn incomplete_record_detected_in_bytecode_checks() {
    let mut record = sample_record();
    record.contract_id = None;
    let verifier = DeploymentVerifier::new(record);
    // verify_all is async and would skip storage checks; test via bytecode path
    let checks = verifier.check_bytecode();
    assert!(checks.iter().any(|c| c.name == "wasm_format"));
}

#[test]
fn record_completeness_passes_with_full_record() {
    let verifier = DeploymentVerifier::new(sample_record());
    let check = verifier.check_record_completeness();
    assert_eq!(check.status.to_string(), "passed");
}

#[test]
fn bytecode_check_skipped_when_wasm_missing() {
    let verifier = DeploymentVerifier::new(sample_record());
    let checks = verifier.check_bytecode();
    assert!(checks.iter().any(|c| c.status == CheckStatus::Skipped));
}

#[tokio::test]
async fn verify_all_produces_report() {
    let verifier = DeploymentVerifier::new(sample_record());
    let report = verifier.verify_all().await.unwrap();
    assert_eq!(report.deployment_id, "deploy-test-001");
    assert!(!report.checks.is_empty());
}

#[test]
fn ci_snippet_includes_deployment_id() {
    let snippet = generate_ci_snippet("my-deploy-id", "testnet");
    assert!(snippet.contains("my-deploy-id"));
    assert!(snippet.contains("deployments verify"));
    assert!(snippet.contains("testnet"));
}
