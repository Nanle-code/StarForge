use starforge::utils::{
    contract_assertions::{
        assert_auth_called, assert_balance_eq, assert_balance_gte, assert_err,
        assert_error_contains, assert_event_count, assert_event_emitted, assert_event_not_emitted,
        assert_ledger_gte, assert_ok, assert_return_value, assert_storage_absent,
        assert_storage_eq, assert_storage_numeric, assert_storage_present, AssertionStatus,
        AssertionSuite, ContractAssertions, NumericComparator,
    },
    contract_fixtures::{
        counter_fixture, liquidity_pool_fixture, multisig_fixture, save_fixture_snapshot,
        token_fixture, AccountRole, FixturePhase, FixtureRegistry, StorageDurability, StorageSeed,
    },
    contract_mocks::{
        counter_env, token_env, MockAddress, MockAuthContext, MockContractClient, MockEnvironment,
        MockEvent, MockEventLog, MockLedger, MockStorage, MockTokenBalances, StorageKey,
    },
    contract_test_framework::{
        counter_test_suite, token_test_suite, ContractTestFramework, FrameworkConfig,
        FrameworkTestSuite, ReportFormat, TestCase, TestCaseResult,
    },
    contract_test_runner::{ContractTestRunner, TestRunConfig},
    testnet_integration::{
        run_connectivity_smoke_test, SorobanNetwork, TestnetConfig, TestnetDeployer, TestnetSession,
    },
};
use std::io::Write as IoWrite;
use tempfile::{NamedTempFile, TempDir};

// ── Helpers ───────────────────────────────────────────────────────────────

fn write_minimal_wasm(path: &std::path::Path) {
    let mut bytes = b"\0asm\x01\0\0\0".to_vec();
    bytes.extend(vec![0u8; 64]);
    std::fs::write(path, bytes).unwrap();
}

const COUNTER_SOURCE: &str = r#"
#[contract]
pub struct Counter;
#[contractimpl]
impl Counter {
    pub fn increment(env: Env) -> u32 { 1 }
    pub fn get_count(env: Env) -> u32 { 0 }
    pub fn reset(env: Env) { env.storage().instance().set(&"count", &0u32); }
}
"#;

// ═══════════════════════════════════════════════════════════════════════════
// Fixture tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn fixture_counter_full_lifecycle() {
    let mut fixture = counter_fixture();
    let ctx = fixture.setup().unwrap();

    assert_eq!(ctx.phase, FixturePhase::Active);
    assert_eq!(ctx.name, "counter");

    let admin = ctx.account("admin").unwrap();
    assert_eq!(admin.role, AccountRole::Admin);

    let count_seed = ctx.storage_entry("count").unwrap();
    assert_eq!(count_seed.durability, StorageDurability::Instance);
    assert_eq!(count_seed.value, serde_json::json!(0u64));

    assert_eq!(ctx.value("initial_count"), Some(&serde_json::json!(0u64)));
    assert_eq!(
        ctx.metadata.get("contract_type").map(|s| s.as_str()),
        Some("counter")
    );

    fixture.teardown().unwrap();
}

#[test]
fn fixture_token_has_all_accounts() {
    let mut fixture = token_fixture();
    let ctx = fixture.setup().unwrap();

    assert!(ctx.account("admin").is_some());
    assert!(ctx.account("minter").is_some());
    assert!(ctx.account("unauthorized").is_some());

    let mint_amount = ctx.value("mint_amount").unwrap();
    assert_eq!(*mint_amount, serde_json::json!(1_000_000_000u64));
}

#[test]
fn fixture_multisig_three_signers() {
    let mut fixture = multisig_fixture(3);
    let ctx = fixture.setup().unwrap();

    for i in 0..4 {
        assert!(
            ctx.account(&format!("signer_{}", i)).is_some(),
            "missing signer_{}",
            i
        );
    }

    let threshold = ctx.value("required_signatures").unwrap();
    assert_eq!(*threshold, serde_json::json!(3u32));
}

#[test]
fn fixture_liquidity_pool_storage_seeds() {
    let mut fixture = liquidity_pool_fixture();
    let ctx = fixture.setup().unwrap();

    assert!(ctx.storage_entry("reserve_a").is_some());
    assert!(ctx.storage_entry("reserve_b").is_some());
    assert!(ctx.storage_entry("fee_bps").is_some());
}

#[test]
fn fixture_registry_setup_teardown_all() {
    let mut registry = FixtureRegistry::new();
    registry.register(counter_fixture());
    registry.register(token_fixture());
    registry.setup_all().unwrap();
    registry.teardown_all().unwrap();
}

#[test]
fn fixture_snapshot_round_trip() {
    let mut fixture = counter_fixture();
    let ctx = fixture.setup().unwrap().clone();
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("snap.json");
    save_fixture_snapshot(&ctx, &path).unwrap();
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("counter"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Mock tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn mock_storage_scoped_operations() {
    let mut storage = MockStorage::new();

    let p_key = StorageKey::persistent("balance");
    let t_key = StorageKey::temporary("temp");
    let i_key = StorageKey::instance("admin");

    storage.set(p_key.clone(), serde_json::json!(1_000u64));
    storage.set(t_key.clone(), serde_json::json!("tmp_value"));
    storage.set(i_key.clone(), serde_json::json!(true));

    assert_eq!(storage.len(), 3);
    assert_eq!(storage.entries_by_scope("persistent").len(), 1);
    assert_eq!(storage.entries_by_scope("temporary").len(), 1);
    assert_eq!(storage.entries_by_scope("instance").len(), 1);

    storage.remove(&t_key);
    assert!(!storage.has(&t_key));
    assert_eq!(storage.len(), 2);
}

#[test]
fn mock_event_log_multi_topic() {
    let mut log = MockEventLog::new();
    let contract = MockAddress::contract(1);

    for i in 0..5u32 {
        log.emit(MockEvent::new(
            contract.clone(),
            vec![serde_json::json!("transfer")],
            serde_json::json!({"seq": i}),
        ));
    }
    log.emit(MockEvent::new(
        contract.clone(),
        vec![serde_json::json!("mint")],
        serde_json::json!({"amount": 500}),
    ));

    assert_eq!(log.count(), 6);
    assert_eq!(log.by_topic("transfer").len(), 5);
    assert_eq!(log.by_topic("mint").len(), 1);
    assert_eq!(log.by_topic("burn").len(), 0);

    log.clear();
    assert!(log.is_empty());
}

#[test]
fn mock_auth_auto_approve_and_reject() {
    let mut auth = MockAuthContext::new();
    let admin = MockAddress::account(1);
    let user = MockAddress::account(2);
    let contract = MockAddress::contract(1);

    auth.auto_approve(admin.clone());

    assert!(auth.require_auth(&admin, &contract, "mint"));
    assert!(!auth.require_auth(&user, &contract, "mint"));

    assert!(auth.was_authorised(&admin, "mint"));
    assert!(!auth.was_authorised(&user, "mint"));
    assert_eq!(auth.auth_count(), 2);
}

#[test]
fn mock_token_balances_full_flow() {
    let mut balances = MockTokenBalances::new();
    let token = "USDC";

    balances.mint(token, "alice", 10_000);
    balances.mint(token, "bob", 5_000);

    assert!(balances.transfer(token, "alice", "bob", 3_000).is_ok());
    assert_eq!(balances.get(token, "alice"), 7_000);
    assert_eq!(balances.get(token, "bob"), 8_000);

    let err = balances.transfer(token, "alice", "bob", 50_000);
    assert!(err.is_err());
    assert!(err.unwrap_err().contains("Insufficient"));
}

#[test]
fn mock_contract_client_full_flow() {
    let client = MockContractClient::new(MockAddress::contract(42));

    client.mock_return("get_count", serde_json::json!(0u64));
    client.mock_return("increment", serde_json::json!(1u64));
    client.mock_error("admin_reset", "unauthorized");

    let val = client.invoke("get_count", vec![], None, 1).unwrap();
    assert_eq!(val, serde_json::json!(0u64));

    let inc = client
        .invoke("increment", vec![], Some(MockAddress::account(1)), 1)
        .unwrap();
    assert_eq!(inc, serde_json::json!(1u64));

    let err = client.invoke("admin_reset", vec![], None, 1);
    assert!(err.is_err());

    assert_eq!(client.total_calls(), 3);
    assert_eq!(client.call_count("increment"), 1);
    assert_eq!(client.call_count("admin_reset"), 1);

    client.reset();
    assert_eq!(client.total_calls(), 0);
}

#[test]
fn mock_environment_full_integration() {
    let mut env = MockEnvironment::new();

    env.storage
        .set(StorageKey::instance("admin"), serde_json::json!("GBADMIN"));
    env.balances.mint("TST", "alice", 1_000_000);
    env.auth.auto_approve(MockAddress::account(1));

    let client = MockContractClient::new(MockAddress::contract(1));
    client.mock_return("balance", serde_json::json!(1_000_000i64));
    env.register_contract(client);

    env.emit_event(
        MockAddress::contract(1),
        vec![serde_json::json!("mint")],
        serde_json::json!({"to": "alice", "amount": 1_000_000}),
    );

    env.advance_ledger(10);
    assert_eq!(env.ledger.sequence, 110);
    assert_eq!(env.events.count(), 1);

    let c = env.contract(&MockAddress::contract(1)).unwrap();
    assert_eq!(
        c.invoke("balance", vec![], None, 110).unwrap(),
        serde_json::json!(1_000_000i64)
    );
}

#[test]
fn mock_ledger_advance_and_timestamp() {
    let mut ledger = MockLedger::default();
    assert_eq!(ledger.sequence, 100);
    ledger.advance(50);
    assert_eq!(ledger.sequence, 150);
    assert_eq!(ledger.timestamp, 1_700_000_000 + 250);
}

// ═══════════════════════════════════════════════════════════════════════════
// Assertion tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn assertions_storage_full_coverage() {
    let env = counter_env();

    let eq = assert_storage_eq(
        &env,
        &StorageKey::instance("count"),
        &serde_json::json!(0u64),
    );
    assert_eq!(eq.status, AssertionStatus::Passed);

    let neq = assert_storage_eq(
        &env,
        &StorageKey::instance("count"),
        &serde_json::json!(99u64),
    );
    assert_eq!(neq.status, AssertionStatus::Failed);
    assert!(neq.expected.is_some());
    assert!(neq.actual.is_some());

    let present = assert_storage_present(&env, &StorageKey::instance("count"));
    assert_eq!(present.status, AssertionStatus::Passed);

    let absent = assert_storage_absent(&env, &StorageKey::persistent("nonexistent"));
    assert_eq!(absent.status, AssertionStatus::Passed);
}

#[test]
fn assertions_numeric_comparators() {
    let env = counter_env();

    let cases = [
        (NumericComparator::Eq, 0i128, true),
        (NumericComparator::Ne, 1, true),
        (NumericComparator::Lt, 100, true),
        (NumericComparator::Lte, 0, true),
        (NumericComparator::Gt, 0, false),
        (NumericComparator::Gte, 1, false),
    ];

    for (cmp, val, expected_pass) in cases {
        let r = assert_storage_numeric(&env, &StorageKey::instance("count"), cmp, val);
        assert_eq!(
            r.status == AssertionStatus::Passed,
            expected_pass,
            "comparator {:?} {} failed",
            cmp,
            val
        );
    }
}

#[test]
fn assertions_balance_pass_and_fail() {
    let env = token_env(50_000);
    let token = MockAddress::contract(10).0;
    let admin = MockAddress::account(10).0;

    let pass = assert_balance_eq(&env, &token, &admin, 50_000);
    assert!(pass.is_passed());

    let fail = assert_balance_eq(&env, &token, &admin, 99_999);
    assert!(!fail.is_passed());
    assert!(fail.hint.is_some());

    let gte = assert_balance_gte(&env, &token, &admin, 1_000);
    assert!(gte.is_passed());
}

#[test]
fn assertions_events_comprehensive() {
    let mut env = counter_env();

    for _ in 0..3 {
        env.emit_event(
            MockAddress::contract(1),
            vec![serde_json::json!("increment")],
            serde_json::json!({"amount": 1}),
        );
    }

    let emitted = assert_event_emitted(&env.events, "increment");
    assert!(emitted.is_passed());

    let not_emitted = assert_event_not_emitted(&env.events, "decrement");
    assert!(not_emitted.is_passed());

    let count = assert_event_count(&env.events, "increment", 3);
    assert!(count.is_passed());

    let wrong_count = assert_event_count(&env.events, "increment", 1);
    assert!(!wrong_count.is_passed());
}

#[test]
fn assertions_auth_comprehensive() {
    let mut env = counter_env();
    let admin = MockAddress::account(1);
    let contract = MockAddress::contract(1);

    env.auth.auto_approve(admin.clone());
    env.auth.require_auth(&admin, &contract, "increment");
    env.auth.require_auth(&admin, &contract, "reset");

    let called = assert_auth_called(&env, &admin, "increment");
    assert!(called.is_passed());

    let not_called = assert_auth_called(&env, &MockAddress::account(99), "increment");
    assert!(!not_called.is_passed());

    let count = starforge::utils::contract_assertions::assert_auth_count(&env, 2);
    assert!(count.is_passed());
}

#[test]
fn assertions_return_value_and_error() {
    let ok: Result<serde_json::Value, String> = Ok(serde_json::json!(42));
    let err: Result<serde_json::Value, String> = Err("overflow error".into());

    assert!(assert_return_value(&ok, &serde_json::json!(42)).is_passed());
    assert!(!assert_return_value(&ok, &serde_json::json!(0)).is_passed());

    assert!(assert_error_contains(&err, "overflow").is_passed());
    assert!(!assert_error_contains(&err, "unauthorized").is_passed());
    assert!(!assert_error_contains(&ok, "anything").is_passed());

    assert!(assert_ok(&ok).is_passed());
    assert!(!assert_ok(&err).is_passed());

    assert!(assert_err(&err).is_passed());
    assert!(!assert_err(&ok).is_passed());
}

#[test]
fn assertions_ledger() {
    let env = counter_env();
    assert!(assert_ledger_gte(&env, 50).is_passed());
    assert!(assert_ledger_gte(&env, 100).is_passed());
    assert!(!assert_ledger_gte(&env, 200).is_passed());
}

#[test]
fn fluent_builder_all_passed() {
    let env = counter_env();
    let suite = ContractAssertions::new(&env)
        .storage_eq(StorageKey::instance("count"), serde_json::json!(0u64))
        .storage_present(StorageKey::instance("count"))
        .storage_absent(StorageKey::persistent("nonexistent"))
        .event_not_emitted("transfer")
        .ledger_gte(1)
        .finish();

    assert!(suite.all_passed(), "failures: {:?}", suite.failures());
    assert_eq!(suite.failed(), 0);
}

#[test]
fn assertion_suite_merge() {
    let mut a = AssertionSuite::new();
    let mut b = AssertionSuite::new();

    a.push(starforge::utils::contract_assertions::AssertionResult::pass("test_a", "ok"));
    b.push(starforge::utils::contract_assertions::AssertionResult::fail("test_b", "failed"));

    a.merge(b);
    assert_eq!(a.total(), 2);
    assert_eq!(a.passed(), 1);
    assert_eq!(a.failed(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// Test runner tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn runner_sequential_basic() {
    let _home_guard = home_lock();
    let home = TempDir::new().unwrap();
    std::env::set_var("HOME", home.path());

    let dir = TempDir::new().unwrap();
    let wasm = dir.path().join("seq.wasm");
    write_minimal_wasm(&wasm);

    let runner = ContractTestRunner::new(TestRunConfig {
        wasm_path: wasm,
        source_path: None,
        workers: 1,
        parallel: false,
        generate: false,
        coverage: false,
    });
    let summary = runner.run().unwrap();
    assert!(summary.cases_executed >= 3);
    assert_eq!(summary.failures, 0);
    assert_eq!(summary.wasm_hash.len(), 64);
    assert!(summary.coverage.is_none());
}

#[test]
fn runner_parallel_workers() {
    let _home_guard = home_lock();
    let home = TempDir::new().unwrap();
    std::env::set_var("HOME", home.path());

    let dir = TempDir::new().unwrap();
    let wasm = dir.path().join("par.wasm");
    write_minimal_wasm(&wasm);

    let runner = ContractTestRunner::new(TestRunConfig {
        wasm_path: wasm,
        source_path: None,
        workers: 4,
        parallel: true,
        generate: false,
        coverage: false,
    });
    let summary = runner.run().unwrap();
    assert!(summary.cases_executed >= 3);
}

#[test]
fn runner_with_source_generation_and_coverage() {
    let _home_guard = home_lock();
    let home = TempDir::new().unwrap();
    std::env::set_var("HOME", home.path());

    let mut src_file = NamedTempFile::new().unwrap();
    src_file.write_all(COUNTER_SOURCE.as_bytes()).unwrap();

    let dir = TempDir::new().unwrap();
    let wasm = dir.path().join("src.wasm");
    write_minimal_wasm(&wasm);

    let runner = ContractTestRunner::new(TestRunConfig {
        wasm_path: wasm,
        source_path: Some(src_file.path().to_path_buf()),
        workers: 2,
        parallel: true,
        generate: true,
        coverage: true,
    });
    let summary = runner.run().unwrap();
    assert!(!summary.generated_cases.is_empty());
    assert!(summary.coverage.is_some());
    let cov = summary.coverage.unwrap();
    assert!(cov.functions_total >= 2);
}

// ═══════════════════════════════════════════════════════════════════════════
// Framework tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn framework_counter_suite_passes() {
    let mut suite = counter_test_suite();
    let result = suite.run();
    assert!(
        result.all_passed(),
        "counter suite failures: {:?}",
        result
            .results
            .iter()
            .filter(|r| !r.passed)
            .collect::<Vec<_>>()
    );
    assert_eq!(result.suite_name, "counter");
    assert_eq!(result.total, 3);
}

#[test]
fn framework_token_suite_passes() {
    let mut suite = token_test_suite();
    let result = suite.run();
    assert!(
        result.all_passed(),
        "token suite failures: {:?}",
        result
            .results
            .iter()
            .filter(|r| !r.passed)
            .collect::<Vec<_>>()
    );
    assert_eq!(result.suite_name, "token");
    assert_eq!(result.total, 4);
}

#[test]
fn framework_custom_test_case() {
    let mut suite = FrameworkTestSuite::new("custom");

    suite.add_case(TestCase::new(
        "always_passes",
        "This test always passes",
        |env| {
            let start = std::time::Instant::now();
            env.storage
                .set(StorageKey::instance("flag"), serde_json::json!(true));
            let assertions = ContractAssertions::new(env)
                .storage_eq(StorageKey::instance("flag"), serde_json::json!(true))
                .finish();
            if assertions.all_passed() {
                TestCaseResult::pass(
                    "always_passes",
                    assertions,
                    start.elapsed().as_millis() as u64,
                )
            } else {
                TestCaseResult::fail(
                    "always_passes",
                    assertions,
                    start.elapsed().as_millis() as u64,
                    "flag not set",
                )
            }
        },
    ));

    let result = suite.run();
    assert!(result.all_passed());
    assert_eq!(result.total, 1);
}

#[test]
fn framework_full_run_without_wasm() {
    let config = FrameworkConfig::new("integration_test_run");
    let result = ContractTestFramework::new(config)
        .add_suite(counter_test_suite())
        .add_suite(token_test_suite())
        .run()
        .unwrap();

    assert!(result.all_passed());
    assert_eq!(result.suite_results.len(), 2);
    assert!(result.wasm_summary.is_none());
    assert!(result.testnet_report.is_none());
}

#[test]
fn framework_full_run_with_wasm() {
    let _home_guard = home_lock();
    let home = TempDir::new().unwrap();
    std::env::set_var("HOME", home.path());

    let dir = TempDir::new().unwrap();
    let wasm = dir.path().join("full.wasm");
    write_minimal_wasm(&wasm);

    let report_dir = TempDir::new().unwrap();
    let config = FrameworkConfig::new("wasm_run")
        .with_wasm(&wasm)
        .with_workers(2)
        .with_report_dir(report_dir.path())
        .with_report_format(ReportFormat::Json);

    let result = ContractTestFramework::new(config)
        .add_suite(counter_test_suite())
        .run()
        .unwrap();

    assert!(result.all_passed());
    assert!(result.wasm_summary.is_some());
    let report_path = report_dir.path().join("framework-report.json");
    assert!(report_path.exists(), "JSON report not written");
}

#[test]
fn framework_html_report_valid() {
    let dir = TempDir::new().unwrap();
    let config = FrameworkConfig::new("html_suite")
        .with_report_dir(dir.path())
        .with_report_format(ReportFormat::Html);

    ContractTestFramework::new(config)
        .add_suite(counter_test_suite())
        .add_suite(token_test_suite())
        .run()
        .unwrap();

    let html = std::fs::read_to_string(dir.path().join("framework-report.html")).unwrap();
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains("html_suite"));
    assert!(html.contains("Passed"));
    assert!(html.contains("Coverage"));
}

#[test]
fn framework_junit_report_valid() {
    let dir = TempDir::new().unwrap();
    let config = FrameworkConfig::new("junit_suite")
        .with_report_dir(dir.path())
        .with_report_format(ReportFormat::JUnit);

    ContractTestFramework::new(config)
        .add_suite(counter_test_suite())
        .run()
        .unwrap();

    let xml = std::fs::read_to_string(dir.path().join("framework-report.xml")).unwrap();
    assert!(xml.contains("<testsuites"));
    assert!(xml.contains("junit_suite"));
    assert!(xml.contains("<testsuite"));
    assert!(xml.contains("<testcase"));
}

#[test]
fn framework_result_print_summary_does_not_panic() {
    let config = FrameworkConfig::new("summary_test");
    let result = ContractTestFramework::new(config)
        .add_suite(counter_test_suite())
        .run()
        .unwrap();
    result.print_summary();
}

// ═══════════════════════════════════════════════════════════════════════════
// Testnet integration tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn testnet_network_properties() {
    assert!(SorobanNetwork::Testnet.supports_friendbot());
    assert!(!SorobanNetwork::Mainnet.supports_friendbot());
    assert!(SorobanNetwork::Local.supports_friendbot());
    assert!(SorobanNetwork::Futurenet.supports_friendbot());

    let custom = SorobanNetwork::Custom {
        rpc_url: "http://my-node:8080/rpc".into(),
        passphrase: "My Network".into(),
    };
    assert_eq!(custom.rpc_url(), "http://my-node:8080/rpc");
    assert!(!custom.supports_friendbot());
}

#[test]
fn testnet_config_factories() {
    let def = TestnetConfig::default();
    assert_eq!(def.network, SorobanNetwork::Testnet);
    assert_eq!(def.max_retries, 3);

    let local = TestnetConfig::local();
    assert_eq!(local.network, SorobanNetwork::Local);
    assert_eq!(local.max_retries, 1);

    let custom = TestnetConfig::for_network(SorobanNetwork::Futurenet);
    assert_eq!(custom.network, SorobanNetwork::Futurenet);
}

#[test]
fn testnet_wasm_hash_deterministic() {
    let bytes = b"\0asm\x01\0\0\0FILLER";
    let h1 = TestnetDeployer::compute_wasm_hash(bytes);
    let h2 = TestnetDeployer::compute_wasm_hash(bytes);
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
    assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn testnet_session_factories() {
    let testnet = TestnetSession::for_testnet();
    assert_eq!(testnet.config.network, SorobanNetwork::Testnet);
    assert_eq!(testnet.contract_count(), 0);

    let local = TestnetSession::for_local();
    assert_eq!(local.config.network, SorobanNetwork::Local);
}

#[test]
fn testnet_connectivity_smoke_test_no_panic() {
    let session = TestnetSession::for_local();
    let result = run_connectivity_smoke_test(&session);
    assert!(!result.name.is_empty());
    // The local node is probably not running in CI; we just check the result
    // structure is correct, not that the connection succeeded.
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge-case and regression tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_suite_produces_zero_totals() {
    let mut suite = FrameworkTestSuite::new("empty");
    let result = suite.run();
    assert_eq!(result.total, 0);
    assert_eq!(result.passed, 0);
    assert_eq!(result.failed, 0);
    assert!(result.all_passed());
}

#[test]
fn framework_with_no_suites() {
    let config = FrameworkConfig::new("no_suites");
    let result = ContractTestFramework::new(config).run().unwrap();
    assert_eq!(result.total, 0);
    assert!(result.all_passed());
}

#[test]
fn storage_key_scope_names() {
    let p = StorageKey::persistent("foo");
    let t = StorageKey::temporary("bar");
    let i = StorageKey::instance("baz");
    assert_eq!(p.scope, "persistent");
    assert_eq!(t.scope, "temporary");
    assert_eq!(i.scope, "instance");
}

#[test]
fn mock_address_display() {
    let addr = MockAddress::account(7);
    assert!(!addr.to_string().is_empty());

    let contract = MockAddress::contract(42);
    assert!(!contract.to_string().is_empty());
}

#[test]
fn fixture_teardown_hook_fires() {
    use starforge::utils::contract_fixtures::FixtureBuilder;
    use std::sync::{Arc, Mutex};

    let counter = Arc::new(Mutex::new(0u32));
    let counter_clone = Arc::clone(&counter);

    let mut fixture = FixtureBuilder::new("hooked")
        .on_teardown(move |_ctx| {
            *counter_clone.lock().unwrap() += 1;
            Ok(())
        })
        .build();

    fixture.setup().unwrap();
    fixture.teardown().unwrap();

    assert_eq!(*counter.lock().unwrap(), 1);
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
