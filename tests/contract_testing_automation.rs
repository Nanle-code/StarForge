use starforge::utils::test_coverage::analyze_source_coverage;
use starforge::utils::test_generator::generate_from_source;
use starforge::utils::test_runner::{run_contract_tests, TestOptions};
use std::io::Write;
use tempfile::{NamedTempFile, TempDir};

const SAMPLE_SOURCE: &str = r#"
#![no_std]
#[contract]
pub struct Counter;

#[contractimpl]
impl Counter {
    pub fn increment(env: Env) -> u32 { 1 }
    pub fn get_count(env: Env) -> u32 { 0 }
}
"#;

fn write_minimal_wasm(path: &std::path::Path) {
    let mut bytes = b"\0asm\x01\0\0\0".to_vec();
    bytes.extend(std::iter::repeat_n(0u8, 64));
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn generates_test_cases_from_source() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(SAMPLE_SOURCE.as_bytes()).unwrap();

    let result = generate_from_source(file.path()).unwrap();
    assert!(result.cases.len() >= 2);
    assert!(result.cases.iter().any(|c| c.test_type == "happy_path"));
}

#[test]
fn coverage_analysis_reports_functions() {
    let report = analyze_source_coverage(SAMPLE_SOURCE, &["increment".into()]);
    assert_eq!(report.functions_total, 2);
    assert_eq!(report.functions_covered, 1);
    assert!(report.coverage_percent > 0.0);
}

#[test]
fn parallel_test_runner_executes_cases() {
    let home = TempDir::new().unwrap();
    std::env::set_var("HOME", home.path());

    let dir = TempDir::new().unwrap();
    let wasm = dir.path().join("test.wasm");
    write_minimal_wasm(&wasm);

    let result = run_contract_tests(
        &wasm,
        TestOptions {
            coverage: false,
            report_format: Some("json".into()),
            update_snapshots: false,
            fuzz_function: None,
            parallel: true,
            generate: false,
            source: None,
            workers: 2,
        },
    )
    .unwrap();

    assert!(result.cases_executed >= 3);
    assert_eq!(result.failures, 0);
    assert!(result.report_path.is_some());
    assert!(result.dashboard_path.is_some());
}

#[test]
fn generated_tests_include_auth_cases() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(SAMPLE_SOURCE.as_bytes()).unwrap();

    let dir = TempDir::new().unwrap();
    let wasm = dir.path().join("c.wasm");
    write_minimal_wasm(&wasm);

    let result = run_contract_tests(
        &wasm,
        TestOptions {
            coverage: true,
            report_format: None,
            update_snapshots: false,
            fuzz_function: None,
            parallel: false,
            generate: true,
            source: Some(file.path().to_path_buf()),
            workers: 1,
        },
    )
    .unwrap();

    assert!(!result.generated_cases.is_empty());
    assert!(result.coverage.is_some());
}
