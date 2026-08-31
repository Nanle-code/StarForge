use starforge::utils::pipeline_builder::{self, PipelineStatus, StageType};
use tempfile::TempDir;

fn temp_home() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
    let guard = home_lock();
    let home = TempDir::new().unwrap();
    std::env::set_var("HOME", home.path());
    (home, guard)
}

fn write_minimal_wasm(path: &std::path::Path) {
    let mut bytes = b"\0asm\x01\0\0\0".to_vec();
    bytes.extend(vec![0u8; 64]);
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn pipeline_template_and_execution_flow() {
    let _home = temp_home();
    let mut pipeline =
        pipeline_builder::from_template("approved-deploy", "release", "testnet").unwrap();
    assert_eq!(pipeline.stages.len(), 4);

    let dir = TempDir::new().unwrap();
    let wasm = dir.path().join("token.wasm");
    write_minimal_wasm(&wasm);
    for stage in pipeline.stages.iter_mut() {
        if stage.stage_type == StageType::Test || stage.stage_type == StageType::Deploy {
            stage.config.wasm_path = Some(wasm.clone());
        }
    }
    pipeline_builder::save_pipeline(&pipeline).unwrap();

    pipeline_builder::execute_pipeline(&mut pipeline, true).unwrap();
    assert_eq!(pipeline.status, PipelineStatus::PendingApproval);

    let approval_id = pipeline
        .stages
        .iter()
        .find(|s| s.stage_type == StageType::Approval)
        .unwrap()
        .id
        .clone();
    pipeline_builder::approve_stage(&mut pipeline, &approval_id, "lead").unwrap();
    pipeline_builder::approve_stage(&mut pipeline, &approval_id, "security").unwrap();

    let result = pipeline_builder::execute_pipeline(&mut pipeline, true).unwrap();
    assert_eq!(result.stages_failed, 0);
    assert_eq!(pipeline.status, PipelineStatus::Completed);
}

#[test]
fn pipeline_testing_integration_runs_contract_tests() {
    let _home = temp_home();
    let mut pipeline = pipeline_builder::create_pipeline("tests", "with tests", "testnet").unwrap();
    let dir = TempDir::new().unwrap();
    let wasm = dir.path().join("c.wasm");
    write_minimal_wasm(&wasm);

    pipeline_builder::add_stage(
        &mut pipeline,
        "Contract tests",
        StageType::Test,
        pipeline_builder::StageConfig {
            wasm_path: Some(wasm),
            test_parallel: true,
            test_coverage: true,
            ..Default::default()
        },
    )
    .unwrap();

    let result = pipeline_builder::execute_pipeline(&mut pipeline, false).unwrap();
    assert_eq!(result.stages_failed, 0);
    assert_eq!(pipeline.status, PipelineStatus::Completed);
    assert!(pipeline.stages[0].test_result.is_some());
}

#[test]
fn pipeline_rollback_stage_template() {
    let _home = temp_home();
    let pipeline = pipeline_builder::from_template("ci-gate", "ci", "testnet").unwrap();
    assert!(pipeline
        .stages
        .iter()
        .any(|s| s.stage_type == StageType::Rollback));
}

#[test]
fn pipeline_html_ui_export() {
    let _home = temp_home();
    let pipeline = pipeline_builder::from_template("multi-contract", "stack", "testnet").unwrap();
    let html = pipeline_builder::render_html_ui(&pipeline);
    assert!(html.contains("Deploy token"));
    assert!(html.contains("Treasury approval"));
}

#[test]
fn pipeline_import_export_roundtrip() {
    let _home = temp_home();
    let pipeline = pipeline_builder::from_template("basic", "roundtrip", "testnet").unwrap();
    pipeline_builder::save_pipeline(&pipeline).unwrap();

    let dir = TempDir::new().unwrap();
    let export_path = dir.path().join("pipeline.json");
    pipeline_builder::export_pipeline(&pipeline, &export_path).unwrap();
    let imported = pipeline_builder::import_pipeline(&export_path).unwrap();
    assert_eq!(imported.name, pipeline.name);
    assert_eq!(imported.stages.len(), pipeline.stages.len());
}

#[test]
fn configure_stage_updates_wasm_path() {
    let _home = temp_home();
    let mut pipeline = pipeline_builder::from_template("basic", "cfg", "testnet").unwrap();
    let test_stage = pipeline
        .stages
        .iter()
        .find(|s| s.stage_type == StageType::Test)
        .unwrap()
        .id
        .clone();

    let dir = TempDir::new().unwrap();
    let wasm = dir.path().join("new.wasm");
    write_minimal_wasm(&wasm);

    pipeline_builder::configure_stage(
        &mut pipeline,
        &test_stage,
        pipeline_builder::StageConfig {
            wasm_path: Some(wasm.clone()),
            test_parallel: true,
            ..Default::default()
        },
    )
    .unwrap();

    let stage = pipeline.stages.iter().find(|s| s.id == test_stage).unwrap();
    assert_eq!(stage.config.wasm_path.as_ref(), Some(&wasm));
}

/// Serialises tests that replace the process-wide `HOME`.
///
/// `std::env::set_var` affects every thread in the binary while libtest runs
/// these tests in parallel, so without this two tests race and one reads back
/// paths under the other's temp home.
fn home_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
