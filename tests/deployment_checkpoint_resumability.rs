//! Automated test suite for Issue #687: Idempotent and Resumable Deployment Operations.

use starforge::utils::deploy_orchestrator::{
    build_plan, execute_plan, load_manifest, DeployStepStatus,
};
use starforge::utils::deployment_automation::{
    run_automation_pipeline, AutomationLevel, DeploymentAutomationConfig,
};
use starforge::utils::deployment_checkpoint::{
    checkpoints_dir, compute_config_hash, compute_session_key, compute_wasm_content_hash,
    DeploymentCheckpoint, DeploymentCheckpointManager, DeploymentLock, CURRENT_SCHEMA_VERSION,
};
use std::fs;
use tempfile::TempDir;

fn setup_test_env() -> TempDir {
    let dir = TempDir::new().unwrap();
    starforge::utils::config::set_test_config_dir(dir.path().to_path_buf());
    dir
}

fn create_dummy_wasm(dir: &TempDir, name: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    let mut bytes = b"\0asm\x01\0\0\0".to_vec();
    bytes.extend(vec![0u8; 128]);
    fs::write(&path, bytes).unwrap();
    path
}

fn create_invalid_wasm(dir: &TempDir, name: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, b"NOT_A_WASM_HEADER_MAGIC").unwrap();
    path
}

fn create_empty_wasm(dir: &TempDir, name: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, b"").unwrap();
    path
}

#[tokio::test]
async fn test_primary_deployment_flow_creates_checkpoint() {
    let dir = setup_test_env();
    let wasm = create_dummy_wasm(&dir, "contract.wasm");

    let config = DeploymentAutomationConfig {
        wasm_path: wasm.to_string_lossy().to_string(),
        network: "testnet".to_string(),
        wallet: Some("alice".to_string()),
        enable_pre_deployment_checks: true,
        enable_automated_testing: true,
        enable_post_deployment_verification: true,
        enable_rollback_automation: true,
        enable_monitoring_setup: true,
        automation_level: AutomationLevel::Standard,
        fresh: false,
    };

    let result = run_automation_pipeline(&config).await.unwrap();
    assert!(result.overall_success);

    // Verify checkpoint file was created with correct schema version and completed steps
    let wasm_bytes = fs::read(&wasm).unwrap();
    let wasm_hash = compute_wasm_content_hash(&wasm_bytes);
    let session_key = compute_session_key(&wasm_hash, "testnet", Some("alice"));
    let cp_file = checkpoints_dir()
        .unwrap()
        .join(format!("{}.json", session_key));

    assert!(cp_file.exists());
    let raw = fs::read_to_string(cp_file).unwrap();
    let cp: DeploymentCheckpoint = serde_json::from_str(&raw).unwrap();

    assert_eq!(cp.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(
        cp.status,
        starforge::utils::deployment_checkpoint::CheckpointStatus::Completed
    );
    assert!(cp.is_step_completed("pre_deployment_validation"));
    assert!(cp.is_step_completed("automated_testing"));
    assert!(cp.is_step_completed("deployment_execution"));
    assert!(cp.is_step_completed("post_deployment_verification"));
    assert!(cp.is_step_completed("monitoring_setup"));
}

#[tokio::test]
async fn test_interrupted_deployment_resumes_from_checkpoint() {
    let dir = setup_test_env();
    let wasm = create_dummy_wasm(&dir, "contract.wasm");
    let wasm_bytes = fs::read(&wasm).unwrap();
    let wasm_hash = compute_wasm_content_hash(&wasm_bytes);

    let flags = [
        ("pre", true),
        ("test", true),
        ("verify", true),
        ("rollback", true),
        ("monitor", true),
    ];
    let config_hash = compute_config_hash("testnet", Some("alice"), &flags);
    let session_key = compute_session_key(&wasm_hash, "testnet", Some("alice"));

    // Manually create a partially completed checkpoint (steps 1 & 2 completed)
    let mut cp = DeploymentCheckpoint::new(
        &session_key,
        &wasm_hash,
        &wasm.to_string_lossy(),
        "testnet",
        Some("alice"),
        &config_hash,
    );
    cp.record_step_completion(
        "pre_deployment_validation",
        &serde_json::json!({
            "validation_id": "val-123",
            "timestamp": "2026-08-25T12:00:00Z",
            "overall_status": "pass",
            "checks": [],
            "gas_estimation": { "estimated_gas_stroops": 10000, "estimated_cost_usd": 0.01, "confidence_level": "high", "optimization_suggestions": [] },
            "network_connectivity": { "network_name": "testnet", "horizon_reachable": true, "soroban_rpc_reachable": true, "latency_ms": 100, "status": "pass" },
            "wallet_balance": { "wallet_name": "alice", "public_key": "GABC", "balance_xlm": 100.0, "sufficient_for_deployment": true, "required_xlm": 1.0, "status": "pass" },
            "approved_for_deployment": true
        }),
    ).unwrap();

    cp.record_step_completion(
        "automated_testing",
        &serde_json::json!({
            "test_run_id": "test-123",
            "timestamp": "2026-08-25T12:01:00Z",
            "overall_status": "pass",
            "test_results": [],
            "coverage_percentage": 90.0,
            "passed_tests": 5,
            "failed_tests": 0,
            "skipped_tests": 0
        }),
    )
    .unwrap();

    DeploymentCheckpointManager::save(&cp).unwrap();

    let config = DeploymentAutomationConfig {
        wasm_path: wasm.to_string_lossy().to_string(),
        network: "testnet".to_string(),
        wallet: Some("alice".to_string()),
        enable_pre_deployment_checks: true,
        enable_automated_testing: true,
        enable_post_deployment_verification: true,
        enable_rollback_automation: true,
        enable_monitoring_setup: true,
        automation_level: AutomationLevel::Standard,
        fresh: false,
    };

    // Run pipeline — it should detect steps 1 & 2 completed, resume at step 3, and finish cleanly
    let result = run_automation_pipeline(&config).await.unwrap();
    assert!(result.overall_success);
    assert!(result.deployment_execution.is_some());

    let loaded_cp = DeploymentCheckpointManager::load_or_create(
        &session_key,
        &wasm_hash,
        &wasm.to_string_lossy(),
        "testnet",
        Some("alice"),
        &config_hash,
        false,
    )
    .unwrap()
    .0;

    assert_eq!(
        loaded_cp.status,
        starforge::utils::deployment_checkpoint::CheckpointStatus::Completed
    );
}

#[tokio::test]
async fn test_idempotent_reexecution() {
    let dir = setup_test_env();
    let wasm = create_dummy_wasm(&dir, "contract.wasm");

    let config = DeploymentAutomationConfig {
        wasm_path: wasm.to_string_lossy().to_string(),
        network: "testnet".to_string(),
        wallet: Some("alice".to_string()),
        enable_pre_deployment_checks: true,
        enable_automated_testing: true,
        enable_post_deployment_verification: true,
        enable_rollback_automation: true,
        enable_monitoring_setup: true,
        automation_level: AutomationLevel::Standard,
        fresh: false,
    };

    // First execution
    let res1 = run_automation_pipeline(&config).await.unwrap();
    assert!(res1.overall_success);

    // Second execution (re-run completed pipeline) — safe no-op returning identical completed result
    let res2 = run_automation_pipeline(&config).await.unwrap();
    assert!(res2.overall_success);
    assert_eq!(res1.automation_id, res2.automation_id);
}

#[tokio::test]
async fn test_wasm_content_change_triggers_staleness_reset() {
    let dir = setup_test_env();
    let wasm = create_dummy_wasm(&dir, "contract.wasm");

    let config = DeploymentAutomationConfig {
        wasm_path: wasm.to_string_lossy().to_string(),
        network: "testnet".to_string(),
        wallet: Some("alice".to_string()),
        enable_pre_deployment_checks: true,
        enable_automated_testing: true,
        enable_post_deployment_verification: true,
        enable_rollback_automation: true,
        enable_monitoring_setup: true,
        automation_level: AutomationLevel::Standard,
        fresh: false,
    };

    // Run initial deployment
    let res1 = run_automation_pipeline(&config).await.unwrap();

    // Modify WASM file content under the SAME file path
    let mut bytes = fs::read(&wasm).unwrap();
    bytes.extend_from_slice(b"MODIFIED_BYTECODE_CONTENT");
    fs::write(&wasm, bytes).unwrap();

    // Re-run deployment — should detect WASM hash changed, discard stale checkpoint, and start fresh
    let res2 = run_automation_pipeline(&config).await.unwrap();
    assert!(res2.overall_success);
    assert_ne!(res1.automation_id, res2.automation_id);
}

#[tokio::test]
async fn test_corrupted_checkpoint_recovery() {
    let dir = setup_test_env();
    let wasm = create_dummy_wasm(&dir, "contract.wasm");
    let wasm_bytes = fs::read(&wasm).unwrap();
    let wasm_hash = compute_wasm_content_hash(&wasm_bytes);
    let session_key = compute_session_key(&wasm_hash, "testnet", Some("alice"));

    // Write corrupted invalid JSON to checkpoint file location
    let cp_file = checkpoints_dir()
        .unwrap()
        .join(format!("{}.json", session_key));
    fs::write(&cp_file, "{ corrupted_json_incomplete...").unwrap();

    let config = DeploymentAutomationConfig {
        wasm_path: wasm.to_string_lossy().to_string(),
        network: "testnet".to_string(),
        wallet: Some("alice".to_string()),
        enable_pre_deployment_checks: true,
        enable_automated_testing: true,
        enable_post_deployment_verification: true,
        enable_rollback_automation: true,
        enable_monitoring_setup: true,
        automation_level: AutomationLevel::Standard,
        fresh: false,
    };

    // Should warn, discard corrupt file, and execute fresh deployment cleanly
    let result = run_automation_pipeline(&config).await.unwrap();
    assert!(result.overall_success);
}

#[tokio::test]
async fn test_schema_version_mismatch_resets() {
    let dir = setup_test_env();
    let wasm = create_dummy_wasm(&dir, "contract.wasm");
    let wasm_bytes = fs::read(&wasm).unwrap();
    let wasm_hash = compute_wasm_content_hash(&wasm_bytes);
    let session_key = compute_session_key(&wasm_hash, "testnet", Some("alice"));

    // Write a checkpoint file with schema_version = 99
    let cp_file = checkpoints_dir()
        .unwrap()
        .join(format!("{}.json", session_key));
    let obsolete_json = serde_json::json!({
        "schema_version": 99,
        "id": "old-id",
        "session_key": session_key,
        "wasm_hash": wasm_hash,
        "wasm_path": wasm.to_string_lossy(),
        "network": "testnet",
        "wallet": "alice",
        "status": "in_progress",
        "completed_steps": [],
        "failed_step": null,
        "config_hash": "cfg",
        "created_at": "2026-08-25T10:00:00Z",
        "updated_at": "2026-08-25T10:00:00Z"
    });
    fs::write(
        &cp_file,
        serde_json::to_string_pretty(&obsolete_json).unwrap(),
    )
    .unwrap();

    let config = DeploymentAutomationConfig {
        wasm_path: wasm.to_string_lossy().to_string(),
        network: "testnet".to_string(),
        wallet: Some("alice".to_string()),
        enable_pre_deployment_checks: true,
        enable_automated_testing: true,
        enable_post_deployment_verification: true,
        enable_rollback_automation: true,
        enable_monitoring_setup: true,
        automation_level: AutomationLevel::Standard,
        fresh: false,
    };

    // Run pipeline — detects schema mismatch (v99 vs CURRENT), warns, resets, and succeeds
    let result = run_automation_pipeline(&config).await.unwrap();
    assert!(result.overall_success);
}

#[tokio::test]
async fn test_concurrency_lock_prevents_duplicate_run() {
    let dir = setup_test_env();
    let wasm = create_dummy_wasm(&dir, "contract.wasm");
    let wasm_bytes = fs::read(&wasm).unwrap();
    let wasm_hash = compute_wasm_content_hash(&wasm_bytes);
    let session_key = compute_session_key(&wasm_hash, "testnet", Some("alice"));

    // Acquire lock manually
    let _lock = DeploymentLock::acquire(&session_key).unwrap();

    let config = DeploymentAutomationConfig {
        wasm_path: wasm.to_string_lossy().to_string(),
        network: "testnet".to_string(),
        wallet: Some("alice".to_string()),
        enable_pre_deployment_checks: true,
        enable_automated_testing: true,
        enable_post_deployment_verification: true,
        enable_rollback_automation: true,
        enable_monitoring_setup: true,
        automation_level: AutomationLevel::Standard,
        fresh: false,
    };

    // Second run should fail fast with lock error
    let err = run_automation_pipeline(&config).await.unwrap_err();
    assert!(err.to_string().contains("already in progress"));
}

#[tokio::test]
async fn test_fast_fail_invalid_inputs_and_unsupported_network() {
    let dir = setup_test_env();

    // Case 1: WASM file does not exist
    let cfg1 = DeploymentAutomationConfig {
        wasm_path: dir
            .path()
            .join("missing.wasm")
            .to_string_lossy()
            .to_string(),
        network: "testnet".to_string(),
        wallet: None,
        enable_pre_deployment_checks: true,
        enable_automated_testing: false,
        enable_post_deployment_verification: false,
        enable_rollback_automation: false,
        enable_monitoring_setup: false,
        automation_level: AutomationLevel::Basic,
        fresh: false,
    };
    let err1 = run_automation_pipeline(&cfg1).await.unwrap_err();
    assert!(err1.to_string().contains("not found"));

    // Case 2: Empty WASM file
    let empty_wasm = create_empty_wasm(&dir, "empty.wasm");
    let cfg2 = DeploymentAutomationConfig {
        wasm_path: empty_wasm.to_string_lossy().to_string(),
        network: "testnet".to_string(),
        wallet: None,
        enable_pre_deployment_checks: true,
        enable_automated_testing: false,
        enable_post_deployment_verification: false,
        enable_rollback_automation: false,
        enable_monitoring_setup: false,
        automation_level: AutomationLevel::Basic,
        fresh: false,
    };
    let err2 = run_automation_pipeline(&cfg2).await.unwrap_err();
    assert!(err2.to_string().contains("0 bytes"));

    // Case 3: Invalid magic bytes
    let invalid_wasm = create_invalid_wasm(&dir, "invalid.wasm");
    let cfg3 = DeploymentAutomationConfig {
        wasm_path: invalid_wasm.to_string_lossy().to_string(),
        network: "testnet".to_string(),
        wallet: None,
        enable_pre_deployment_checks: true,
        enable_automated_testing: false,
        enable_post_deployment_verification: false,
        enable_rollback_automation: false,
        enable_monitoring_setup: false,
        automation_level: AutomationLevel::Basic,
        fresh: false,
    };
    let err3 = run_automation_pipeline(&cfg3).await.unwrap_err();
    assert!(err3.to_string().contains("invalid magic header"));

    // Case 4: Unsupported network
    let dummy_wasm = create_dummy_wasm(&dir, "valid.wasm");
    let cfg4 = DeploymentAutomationConfig {
        wasm_path: dummy_wasm.to_string_lossy().to_string(),
        network: "unsupported_solana_devnet".to_string(),
        wallet: None,
        enable_pre_deployment_checks: true,
        enable_automated_testing: false,
        enable_post_deployment_verification: false,
        enable_rollback_automation: false,
        enable_monitoring_setup: false,
        automation_level: AutomationLevel::Basic,
        fresh: false,
    };
    let err4 = run_automation_pipeline(&cfg4).await.unwrap_err();
    assert!(err4.to_string().contains("Unsupported target network"));
}

#[test]
fn test_orchestrate_resumability_and_idempotency() {
    let dir = setup_test_env();
    let wasm_a = create_dummy_wasm(&dir, "a.wasm");
    let wasm_b = create_dummy_wasm(&dir, "b.wasm");

    let manifest_path = dir.path().join("manifest.json");
    fs::write(
        &manifest_path,
        format!(
            r#"{{
                "name": "stack-test",
                "network": "testnet",
                "contracts": [
                    {{ "id": "a", "wasm": "{}", "depends_on": [] }},
                    {{ "id": "b", "wasm": "{}", "depends_on": ["a"] }}
                ]
            }}"#,
            wasm_a.display().to_string().replace('\\', "/"),
            wasm_b.display().to_string().replace('\\', "/")
        ),
    )
    .unwrap();

    let manifest = load_manifest(&manifest_path).unwrap();
    let mut state = build_plan(&manifest).unwrap();

    // Mark step 'a' as already Deployed
    state.steps[0].status = DeployStepStatus::Deployed;
    state.steps[0].deployed_address = Some("C_ALREADY_DEPLOYED_A".to_string());

    // Execute plan — should skip step 'a' and deploy step 'b'
    execute_plan(&mut state, true).unwrap();
    assert_eq!(
        state.steps[0].deployed_address.as_deref(),
        Some("C_ALREADY_DEPLOYED_A")
    );
    assert_eq!(state.steps[1].status, DeployStepStatus::Deployed);
    assert_eq!(state.status, "simulated-complete");

    // Re-running execute_plan on fully deployed stack is safe no-op
    execute_plan(&mut state, true).unwrap();
    assert_eq!(state.status, "simulated-complete");
}
