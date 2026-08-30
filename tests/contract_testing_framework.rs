use starforge::utils::contract_testing::{
    run_contract_framework, FrameworkRunOptions, TestnetIntegrationConfig,
};
use std::fs;
use std::io::Write;
use tempfile::{NamedTempFile, TempDir};

const SAMPLE_SOURCE: &str = r#"
#[contractimpl]
impl Counter {
    pub fn increment(env: Env) -> u32 { 1 }
    pub fn balance(env: Env) -> u32 { 100 }
}
"#;

fn write_minimal_wasm(path: &std::path::Path) {
    let mut bytes = b"\0asm\x01\0\0\0".to_vec();
    bytes.extend(vec![0u8; 64]);
    fs::write(path, bytes).unwrap();
}

fn isolate_starforge_home() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
    let guard = home_lock();
    let home = TempDir::new().unwrap();
    std::env::set_var("HOME", home.path());
    std::env::set_var("USERPROFILE", home.path());
    (home, guard)
}

#[tokio::test]
async fn framework_runs_fixtures_mocks_assertions_reports_and_testnet_dry_run() {
    let _home = isolate_starforge_home();
    let dir = TempDir::new().unwrap();
    let wasm = dir.path().join("counter.wasm");
    write_minimal_wasm(&wasm);

    let spec = dir.path().join("contract-tests.json");
    fs::write(
        &spec,
        r#"{
  "name": "counter-contract",
  "fixtures": [
    {
      "name": "empty-counter",
      "storage": [{ "key": "COUNTER", "value": 0 }]
    }
  ],
  "mocks": [
    {
      "function": "balance",
      "returns": 100,
      "events": ["balance-read"]
    }
  ],
  "tests": [
    {
      "name": "increment updates counter",
      "fixture": "empty-counter",
      "function": "increment",
      "expected_return": 1,
      "assertions": [
        { "type": "state_equals", "key": "COUNTER", "value": 1 },
        { "type": "event_emitted", "value": "increment:1" },
        { "type": "fee_at_most", "stroops": 120000 }
      ]
    },
    {
      "name": "mocked balance is available",
      "function": "balance",
      "assertions": [
        { "type": "return_equals", "value": 100 },
        { "type": "event_emitted", "value": "balance-read" },
        { "type": "mock_called", "function": "balance", "times": 1 }
      ]
    }
  ]
}"#,
    )
    .unwrap();

    let mut source = NamedTempFile::new().unwrap();
    source.write_all(SAMPLE_SOURCE.as_bytes()).unwrap();

    let report = run_contract_framework(
        &wasm,
        &spec,
        FrameworkRunOptions {
            coverage: true,
            report_format: Some("json".to_string()),
            source: Some(source.path().to_path_buf()),
            testnet: Some(TestnetIntegrationConfig {
                network: "testnet".to_string(),
                contract_id: None,
                verify_rpc_health: false,
                dry_run: true,
            }),
        },
    )
    .await
    .unwrap();

    assert_eq!(report.suite_name, "counter-contract");
    assert_eq!(report.cases_executed, 2);
    assert_eq!(report.failures, 0);
    assert_eq!(report.fixtures_loaded, 1);
    assert_eq!(report.mocks_available, 1);
    assert!(report.coverage.unwrap().coverage_percent > 0.0);
    assert!(report
        .report_path
        .as_ref()
        .is_some_and(|path| path.exists()));
    assert!(report.testnet.unwrap().dry_run);
}

#[tokio::test]
async fn framework_reports_custom_assertion_failures() {
    let _home = isolate_starforge_home();
    let dir = TempDir::new().unwrap();
    let wasm = dir.path().join("counter.wasm");
    write_minimal_wasm(&wasm);

    let spec = dir.path().join("contract-tests.json");
    fs::write(
        &spec,
        r#"{
  "name": "counter-contract",
  "fixtures": [
    {
      "name": "empty-counter",
      "storage": [{ "key": "COUNTER", "value": 0 }]
    }
  ],
  "tests": [
    {
      "name": "increment must update counter twice",
      "fixture": "empty-counter",
      "function": "increment",
      "assertions": [
        { "type": "state_equals", "key": "COUNTER", "value": 2 }
      ]
    }
  ]
}"#,
    )
    .unwrap();

    let report = run_contract_framework(
        &wasm,
        &spec,
        FrameworkRunOptions {
            coverage: false,
            report_format: None,
            source: None,
            testnet: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(report.failures, 1);
    assert!(!report.cases[0].assertions[0].passed);
    assert!(report.cases[0].errors[0].contains("expected 2"));
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
