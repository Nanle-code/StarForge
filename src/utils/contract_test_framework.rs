use crate::utils::{
    contract_assertions::{
        assert_error_contains, assert_return_value, AssertionSuite, ContractAssertions,
    },
    contract_fixtures::{ContractFixture, FixtureContext},
    contract_mocks::{MockAddress, MockContractClient, MockEnvironment, StorageKey},
    contract_test_runner::{ContractTestRunner, TestRunConfig, TestRunSummary},
    testnet_integration::{
        run_connectivity_smoke_test, TestnetConfig, TestnetSession, TestnetTestReport,
    },
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

// ── Framework configuration ────────────────────────────────────────────────

/// Top-level configuration for the contract testing framework.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameworkConfig {
    /// Human-readable suite name shown in reports.
    pub suite_name: String,
    /// Path to the compiled contract WASM under test.
    pub wasm_path: Option<PathBuf>,
    /// Rust source path used for coverage analysis and test generation.
    pub source_path: Option<PathBuf>,
    /// Whether to run tests on the testnet after local tests pass.
    pub testnet_enabled: bool,
    /// Testnet configuration to use when `testnet_enabled` is true.
    pub testnet_config: TestnetConfig,
    /// Number of parallel workers for the test runner.
    pub workers: usize,
    /// Report output directory.
    pub report_dir: Option<PathBuf>,
    /// Report format: "json", "html", or "junit".
    pub report_format: ReportFormat,
}

impl Default for FrameworkConfig {
    fn default() -> Self {
        Self {
            suite_name: "soroban-contract-suite".into(),
            wasm_path: None,
            source_path: None,
            testnet_enabled: false,
            testnet_config: TestnetConfig::default(),
            workers: 4,
            report_dir: None,
            report_format: ReportFormat::Json,
        }
    }
}

impl FrameworkConfig {
    pub fn new(suite_name: impl Into<String>) -> Self {
        Self {
            suite_name: suite_name.into(),
            ..Default::default()
        }
    }

    pub fn with_wasm(mut self, path: impl AsRef<Path>) -> Self {
        self.wasm_path = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn with_source(mut self, path: impl AsRef<Path>) -> Self {
        self.source_path = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn with_testnet(mut self, config: TestnetConfig) -> Self {
        self.testnet_enabled = true;
        self.testnet_config = config;
        self
    }

    pub fn with_workers(mut self, workers: usize) -> Self {
        self.workers = workers;
        self
    }

    pub fn with_report_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.report_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    pub fn with_report_format(mut self, format: ReportFormat) -> Self {
        self.report_format = format;
        self
    }
}

/// Output format for test reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReportFormat {
    Json,
    Html,
    JUnit,
}

impl std::fmt::Display for ReportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReportFormat::Json => write!(f, "json"),
            ReportFormat::Html => write!(f, "html"),
            ReportFormat::JUnit => write!(f, "junit"),
        }
    }
}

// ── Test case definition ───────────────────────────────────────────────────

/// A single named test case within the framework.
pub struct TestCase {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    func: Box<dyn Fn(&mut MockEnvironment) -> TestCaseResult + Send + Sync>,
}

impl TestCase {
    pub fn new<F>(name: impl Into<String>, description: impl Into<String>, func: F) -> Self
    where
        F: Fn(&mut MockEnvironment) -> TestCaseResult + Send + Sync + 'static,
    {
        Self {
            name: name.into(),
            description: description.into(),
            tags: Vec::new(),
            func: Box::new(func),
        }
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn run(&self, env: &mut MockEnvironment) -> TestCaseResult {
        (self.func)(env)
    }
}

/// Outcome of a single test case execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCaseResult {
    pub name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub assertions: AssertionSuite,
    pub error: Option<String>,
}

impl TestCaseResult {
    pub fn pass(name: impl Into<String>, assertions: AssertionSuite, duration_ms: u64) -> Self {
        Self {
            name: name.into(),
            passed: true,
            duration_ms,
            assertions,
            error: None,
        }
    }

    pub fn fail(
        name: impl Into<String>,
        assertions: AssertionSuite,
        duration_ms: u64,
        error: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            passed: false,
            duration_ms,
            assertions,
            error: Some(error.into()),
        }
    }
}

// ── Framework test suite ───────────────────────────────────────────────────

/// A collection of test cases with shared fixture setup.
pub struct FrameworkTestSuite {
    pub name: String,
    cases: Vec<TestCase>,
    fixture: Option<ContractFixture>,
}

impl FrameworkTestSuite {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            cases: Vec::new(),
            fixture: None,
        }
    }

    /// Attach a fixture to this suite. The fixture is set up before the first
    /// test case runs and torn down after the last.
    pub fn with_fixture(mut self, fixture: ContractFixture) -> Self {
        self.fixture = Some(fixture);
        self
    }

    /// Register a test case.
    pub fn add_case(&mut self, case: TestCase) {
        self.cases.push(case);
    }

    /// Run all cases and return a suite result.
    pub fn run(&mut self) -> FrameworkSuiteResult {
        let suite_start = Instant::now();

        // Setup fixture
        let fixture_ctx: Option<FixtureContext> =
            self.fixture.as_mut().and_then(|f| f.setup().ok().cloned());

        let mut results = Vec::new();
        for case in &self.cases {
            let mut env = MockEnvironment::new();

            // Seed environment from fixture context
            if let Some(ref ctx) = fixture_ctx {
                for seed in ctx.storage.values() {
                    env.storage.set(
                        StorageKey {
                            scope: format!("{:?}", seed.durability).to_lowercase(),
                            key: seed.key.clone(),
                        },
                        seed.value.clone(),
                    );
                }
                for account in ctx.accounts.values() {
                    env.auth
                        .auto_approve(MockAddress::new(account.address.clone()));
                }
            }

            let start = Instant::now();
            let result = case.run(&mut env);
            let _ = start.elapsed(); // duration tracked inside result

            results.push(result);
        }

        // Teardown fixture
        if let Some(ref mut fixture) = self.fixture {
            let _ = fixture.teardown();
        }

        let total_ms = suite_start.elapsed().as_millis() as u64;
        let passed = results.iter().filter(|r| r.passed).count() as u32;
        let failed = results.iter().filter(|r| !r.passed).count() as u32;

        FrameworkSuiteResult {
            suite_name: self.name.clone(),
            total: results.len() as u32,
            passed,
            failed,
            total_duration_ms: total_ms,
            results,
        }
    }
}

/// Aggregated results from a [`FrameworkTestSuite`] run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameworkSuiteResult {
    pub suite_name: String,
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub total_duration_ms: u64,
    pub results: Vec<TestCaseResult>,
}

impl FrameworkSuiteResult {
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }
}

// ── Main framework ─────────────────────────────────────────────────────────

/// The main entry point for the StarForge contract testing framework.
///
/// Usage:
/// ```rust,ignore
/// let result = ContractTestFramework::new(config)
///     .add_suite(my_suite)
///     .run()?;
/// ```
pub struct ContractTestFramework {
    config: FrameworkConfig,
    suites: Vec<FrameworkTestSuite>,
}

impl ContractTestFramework {
    pub fn new(config: FrameworkConfig) -> Self {
        Self {
            config,
            suites: Vec::new(),
        }
    }

    /// Add a test suite to the framework.
    pub fn add_suite(mut self, suite: FrameworkTestSuite) -> Self {
        self.suites.push(suite);
        self
    }

    /// Run all registered suites and produce a [`FrameworkRunResult`].
    pub fn run(mut self) -> Result<FrameworkRunResult> {
        let framework_start = Instant::now();
        let mut suite_results = Vec::new();

        for suite in &mut self.suites {
            let result = suite.run();
            suite_results.push(result);
        }

        // Optionally run WASM-based test runner
        let wasm_summary = if let Some(ref wasm_path) = self.config.wasm_path {
            let runner = ContractTestRunner::new(TestRunConfig {
                wasm_path: wasm_path.clone(),
                source_path: self.config.source_path.clone(),
                workers: self.config.workers,
                parallel: self.config.workers > 1,
                generate: self.config.source_path.is_some(),
                coverage: self.config.source_path.is_some(),
            });
            Some(runner.run()?)
        } else {
            None
        };

        // Optionally run testnet tests
        let testnet_report = if self.config.testnet_enabled {
            Some(run_testnet_tests(&self.config.testnet_config)?)
        } else {
            None
        };

        let total_ms = framework_start.elapsed().as_millis() as u64;

        let total = suite_results.iter().map(|s| s.total).sum::<u32>()
            + wasm_summary.as_ref().map(|s| s.cases_executed).unwrap_or(0)
            + testnet_report.as_ref().map(|r| r.total_tests).unwrap_or(0);

        let passed = suite_results.iter().map(|s| s.passed).sum::<u32>()
            + wasm_summary
                .as_ref()
                .map(|s| s.cases_executed - s.failures)
                .unwrap_or(0)
            + testnet_report.as_ref().map(|r| r.passed).unwrap_or(0);

        let failed = suite_results.iter().map(|s| s.failed).sum::<u32>()
            + wasm_summary.as_ref().map(|s| s.failures).unwrap_or(0)
            + testnet_report.as_ref().map(|r| r.failed).unwrap_or(0);

        let result = FrameworkRunResult {
            config_name: self.config.suite_name.clone(),
            suite_results,
            wasm_summary,
            testnet_report,
            total,
            passed,
            failed,
            total_duration_ms: total_ms,
            generated_at: chrono::Utc::now().to_rfc3339(),
        };

        // Write reports
        if let Some(ref dir) = self.config.report_dir {
            fs::create_dir_all(dir)
                .with_context(|| format!("Cannot create report dir {}", dir.display()))?;
            write_framework_report(&result, dir, &self.config.report_format)?;
        }

        Ok(result)
    }
}

/// Full result of a [`ContractTestFramework`] run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameworkRunResult {
    pub config_name: String,
    pub suite_results: Vec<FrameworkSuiteResult>,
    pub wasm_summary: Option<TestRunSummary>,
    pub testnet_report: Option<TestnetTestReport>,
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub total_duration_ms: u64,
    pub generated_at: String,
}

impl FrameworkRunResult {
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }

    pub fn coverage_percent(&self) -> f64 {
        self.wasm_summary
            .as_ref()
            .and_then(|s| s.coverage.as_ref())
            .map(|c| c.coverage_percent)
            .unwrap_or(0.0)
    }

    pub fn print_summary(&self) {
        println!("\n=== {} ===", self.config_name);
        println!(
            "Tests: {}/{} passed ({} failed) in {}ms",
            self.passed, self.total, self.failed, self.total_duration_ms
        );
        if let Some(ref wasm) = self.wasm_summary {
            println!(
                "Coverage: {:.1}%",
                wasm.coverage
                    .as_ref()
                    .map(|c| c.coverage_percent)
                    .unwrap_or(0.0)
            );
        }
        if self.all_passed() {
            println!("All tests passed.");
        } else {
            println!("FAILED. {} test(s) did not pass.", self.failed);
        }
    }
}

// ── Testnet runner ─────────────────────────────────────────────────────────

fn run_testnet_tests(config: &TestnetConfig) -> Result<TestnetTestReport> {
    let session = TestnetSession::new(config.clone());
    let start = Instant::now();

    let connectivity = run_connectivity_smoke_test(&session);
    let health = session.client.health_check();

    let passed = if connectivity.passed { 1 } else { 0 };
    let failed = if connectivity.passed { 0 } else { 1 };

    Ok(TestnetTestReport {
        network: config.network.to_string(),
        rpc_url: config.network.rpc_url().into(),
        health,
        total_tests: 1,
        passed,
        failed,
        total_duration_ms: start.elapsed().as_millis() as u64,
        contracts_deployed: session.contract_count() as u32,
        accounts_funded: session.funded_accounts.len() as u32,
        results: vec![connectivity],
        generated_at: chrono::Utc::now().to_rfc3339(),
    })
}

// ── Report writers ─────────────────────────────────────────────────────────

fn write_framework_report(
    result: &FrameworkRunResult,
    dir: &Path,
    format: &ReportFormat,
) -> Result<()> {
    match format {
        ReportFormat::Json => {
            let path = dir.join("framework-report.json");
            fs::write(&path, serde_json::to_string_pretty(result)?)
                .with_context(|| format!("Failed to write {}", path.display()))?;
        }
        ReportFormat::Html => {
            let path = dir.join("framework-report.html");
            fs::write(&path, render_html_report(result))
                .with_context(|| format!("Failed to write {}", path.display()))?;
        }
        ReportFormat::JUnit => {
            let path = dir.join("framework-report.xml");
            fs::write(&path, render_junit_report(result))
                .with_context(|| format!("Failed to write {}", path.display()))?;
        }
    }
    Ok(())
}

fn render_html_report(result: &FrameworkRunResult) -> String {
    let pass_color = if result.all_passed() {
        "#3fb950"
    } else {
        "#f85149"
    };
    let coverage = result.coverage_percent();

    let suite_rows: String = result
        .suite_results
        .iter()
        .flat_map(|s| {
            s.results.iter().map(|r| {
                format!(
                    "<tr>\
                     <td>{}</td><td>{}</td>\
                     <td style=\"color:{};\">{}</td>\
                     <td>{}ms</td>\
                     <td>{}</td></tr>",
                    s.suite_name,
                    r.name,
                    if r.passed { "#3fb950" } else { "#f85149" },
                    if r.passed { "PASS" } else { "FAIL" },
                    r.duration_ms,
                    r.error.as_deref().unwrap_or("")
                )
            })
        })
        .collect();

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>StarForge Contract Test Report — {name}</title>
  <style>
    body {{font-family:system-ui;background:#0d1117;color:#e6edf3;padding:2rem;}}
    h1,h2 {{color:#e6edf3;}}
    .grid {{display:grid;grid-template-columns:repeat(4,1fr);gap:1rem;margin:1.5rem 0;}}
    .card {{background:#161b22;border:1px solid #30363d;border-radius:8px;padding:1.25rem;}}
    .metric {{font-size:2rem;font-weight:700;}}
    table {{width:100%;border-collapse:collapse;margin-top:1rem;}}
    th,td {{border:1px solid #30363d;padding:.5rem .75rem;text-align:left;}}
    th {{background:#161b22;}}
    .badge-pass {{color:#3fb950;}} .badge-fail {{color:#f85149;}}
  </style>
</head>
<body>
<h1>Contract Test Report</h1>
<p>Suite: <strong>{name}</strong> &mdash; Generated: {ts}</p>
<div class="grid">
  <div class="card"><div class="metric" style="color:{pc}">{total}</div><div>Total Tests</div></div>
  <div class="card"><div class="metric badge-pass">{passed}</div><div>Passed</div></div>
  <div class="card"><div class="metric badge-fail">{failed}</div><div>Failed</div></div>
  <div class="card"><div class="metric">{cov:.1}%</div><div>Coverage</div></div>
</div>
<h2>Test Results</h2>
<table>
<thead><tr><th>Suite</th><th>Test</th><th>Status</th><th>Duration</th><th>Error</th></tr></thead>
<tbody>{rows}</tbody>
</table>
</body>
</html>"#,
        name = result.config_name,
        ts = result.generated_at,
        pc = pass_color,
        total = result.total,
        passed = result.passed,
        failed = result.failed,
        cov = coverage,
        rows = suite_rows,
    )
}

fn render_junit_report(result: &FrameworkRunResult) -> String {
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <testsuites name=\"{}\" tests=\"{}\" failures=\"{}\" time=\"{}\">\n",
        result.config_name,
        result.total,
        result.failed,
        result.total_duration_ms as f64 / 1000.0
    );

    for suite in &result.suite_results {
        xml.push_str(&format!(
            "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\">\n",
            suite.suite_name, suite.total, suite.failed
        ));
        for case in &suite.results {
            xml.push_str(&format!(
                "    <testcase name=\"{}\" time=\"{}\"",
                case.name,
                case.duration_ms as f64 / 1000.0
            ));
            if case.passed {
                xml.push_str("/>\n");
            } else {
                let msg = case.error.as_deref().unwrap_or("assertion failed");
                xml.push_str(&format!(
                    ">\n      <failure message=\"{}\">{}</failure>\n    </testcase>\n",
                    msg, msg
                ));
            }
        }
        xml.push_str("  </testsuite>\n");
    }

    xml.push_str("</testsuites>\n");
    xml
}

// ── Built-in standard test suites ─────────────────────────────────────────

/// Standard test cases for any counter-style contract.
pub fn counter_test_suite() -> FrameworkTestSuite {
    use crate::utils::contract_fixtures::counter_fixture;
    use crate::utils::contract_mocks::MockAddress;

    let mut suite = FrameworkTestSuite::new("counter").with_fixture(counter_fixture());

    suite.add_case(TestCase::new(
        "initial_count_is_zero",
        "Counter must start at zero",
        |env| {
            let start = Instant::now();
            let suite = ContractAssertions::new(env)
                .storage_eq(StorageKey::instance("count"), serde_json::json!(0u64))
                .finish();
            let passed = suite.all_passed();
            if passed {
                TestCaseResult::pass(
                    "initial_count_is_zero",
                    suite,
                    start.elapsed().as_millis() as u64,
                )
            } else {
                TestCaseResult::fail(
                    "initial_count_is_zero",
                    suite,
                    start.elapsed().as_millis() as u64,
                    "count storage not initialised to zero",
                )
            }
        },
    ));

    suite.add_case(TestCase::new(
        "increment_returns_new_count",
        "increment() returns the updated counter value",
        |env| {
            let start = Instant::now();
            let client = MockContractClient::new(MockAddress::contract(1));
            client.mock_return("increment", serde_json::json!(1u64));
            let result = client.invoke(
                "increment",
                vec![],
                Some(MockAddress::account(1)),
                env.ledger.sequence,
            );
            let mut suite = AssertionSuite::new();
            suite.push(assert_return_value(&result, &serde_json::json!(1u64)));
            let passed = suite.all_passed();
            if passed {
                TestCaseResult::pass(
                    "increment_returns_new_count",
                    suite,
                    start.elapsed().as_millis() as u64,
                )
            } else {
                TestCaseResult::fail(
                    "increment_returns_new_count",
                    suite,
                    start.elapsed().as_millis() as u64,
                    "unexpected return value from increment",
                )
            }
        },
    ));

    suite.add_case(TestCase::new(
        "unauthorised_increment_fails",
        "increment() must reject callers without auth",
        |env| {
            let start = Instant::now();
            let client = MockContractClient::new(MockAddress::contract(1));
            client.mock_error("increment", "unauthorized");
            let result = client.invoke("increment", vec![], None, env.ledger.sequence);
            let mut suite = AssertionSuite::new();
            suite.push(assert_error_contains(&result, "unauthorized"));
            let passed = suite.all_passed();
            if passed {
                TestCaseResult::pass(
                    "unauthorised_increment_fails",
                    suite,
                    start.elapsed().as_millis() as u64,
                )
            } else {
                TestCaseResult::fail(
                    "unauthorised_increment_fails",
                    suite,
                    start.elapsed().as_millis() as u64,
                    "expected authorization error not returned",
                )
            }
        },
    ));

    suite
}

/// Standard test cases for a SEP-41 token contract.
pub fn token_test_suite() -> FrameworkTestSuite {
    use crate::utils::contract_fixtures::token_fixture;
    use crate::utils::contract_mocks::{MockAddress, MockContractClient};

    let mut suite = FrameworkTestSuite::new("token").with_fixture(token_fixture());

    suite.add_case(TestCase::new(
        "initial_supply_zero",
        "Total supply starts at zero before any minting",
        |env| {
            let start = Instant::now();
            let suite = ContractAssertions::new(env)
                .storage_eq(
                    StorageKey::persistent("total_supply"),
                    serde_json::json!(0u64),
                )
                .finish();
            let passed = suite.all_passed();
            if passed {
                TestCaseResult::pass(
                    "initial_supply_zero",
                    suite,
                    start.elapsed().as_millis() as u64,
                )
            } else {
                TestCaseResult::fail(
                    "initial_supply_zero",
                    suite,
                    start.elapsed().as_millis() as u64,
                    "total_supply not zero at init",
                )
            }
        },
    ));

    suite.add_case(TestCase::new(
        "mint_emits_event",
        "mint() must emit a 'mint' event",
        |env| {
            let start = Instant::now();
            env.emit_event(
                MockAddress::contract(10),
                vec![serde_json::json!("mint")],
                serde_json::json!({"to": "GBTEST", "amount": 1_000_000_000u64}),
            );
            let suite = ContractAssertions::new(env).event_emitted("mint").finish();
            let passed = suite.all_passed();
            if passed {
                TestCaseResult::pass(
                    "mint_emits_event",
                    suite,
                    start.elapsed().as_millis() as u64,
                )
            } else {
                TestCaseResult::fail(
                    "mint_emits_event",
                    suite,
                    start.elapsed().as_millis() as u64,
                    "mint event not emitted",
                )
            }
        },
    ));

    suite.add_case(TestCase::new(
        "unauthorised_mint_fails",
        "mint() must reject callers without minter role",
        |env| {
            let start = Instant::now();
            let client = MockContractClient::new(MockAddress::contract(10));
            client.mock_error("mint", "not authorized: minter role required");
            let result = client.invoke("mint", vec![], None, env.ledger.sequence);
            let mut suite = AssertionSuite::new();
            suite.push(assert_error_contains(&result, "not authorized"));
            let passed = suite.all_passed();
            if passed {
                TestCaseResult::pass(
                    "unauthorised_mint_fails",
                    suite,
                    start.elapsed().as_millis() as u64,
                )
            } else {
                TestCaseResult::fail(
                    "unauthorised_mint_fails",
                    suite,
                    start.elapsed().as_millis() as u64,
                    "expected authorization error not returned",
                )
            }
        },
    ));

    suite.add_case(TestCase::new(
        "transfer_no_extra_events",
        "A simple transfer must not emit unexpected events",
        |env| {
            let start = Instant::now();
            env.emit_event(
                MockAddress::contract(10),
                vec![serde_json::json!("transfer")],
                serde_json::json!({"from":"A","to":"B","amount":100}),
            );
            let suite = ContractAssertions::new(env)
                .event_emitted("transfer")
                .event_not_emitted("mint")
                .event_not_emitted("burn")
                .finish();
            let passed = suite.all_passed();
            if passed {
                TestCaseResult::pass(
                    "transfer_no_extra_events",
                    suite,
                    start.elapsed().as_millis() as u64,
                )
            } else {
                TestCaseResult::fail(
                    "transfer_no_extra_events",
                    suite,
                    start.elapsed().as_millis() as u64,
                    "unexpected events emitted during transfer",
                )
            }
        },
    ));

    suite
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn counter_suite_all_pass() {
        let mut suite = counter_test_suite();
        let result = suite.run();
        assert!(
            result.all_passed(),
            "failures: {:?}",
            result
                .results
                .iter()
                .filter(|r| !r.passed)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn token_suite_all_pass() {
        let mut suite = token_test_suite();
        let result = suite.run();
        assert!(
            result.all_passed(),
            "failures: {:?}",
            result
                .results
                .iter()
                .filter(|r| !r.passed)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn framework_runs_without_wasm() {
        let config = FrameworkConfig::new("framework_test");
        let result = ContractTestFramework::new(config)
            .add_suite(counter_test_suite())
            .add_suite(token_test_suite())
            .run()
            .unwrap();

        assert!(result.all_passed());
        assert_eq!(result.suite_results.len(), 2);
    }

    #[test]
    fn framework_writes_json_report() {
        let dir = TempDir::new().unwrap();
        let config = FrameworkConfig::new("report_test")
            .with_report_dir(dir.path())
            .with_report_format(ReportFormat::Json);

        ContractTestFramework::new(config)
            .add_suite(counter_test_suite())
            .run()
            .unwrap();

        let report_path = dir.path().join("framework-report.json");
        assert!(report_path.exists(), "JSON report not written");
        let content = fs::read_to_string(&report_path).unwrap();
        assert!(content.contains("framework_report_test") || content.contains("report_test"));
    }

    #[test]
    fn framework_writes_html_report() {
        let dir = TempDir::new().unwrap();
        let config = FrameworkConfig::new("html_report_test")
            .with_report_dir(dir.path())
            .with_report_format(ReportFormat::Html);

        ContractTestFramework::new(config)
            .add_suite(counter_test_suite())
            .run()
            .unwrap();

        let html_path = dir.path().join("framework-report.html");
        assert!(html_path.exists());
        let html = fs::read_to_string(&html_path).unwrap();
        assert!(html.contains("<!doctype html>"));
    }

    #[test]
    fn framework_writes_junit_report() {
        let dir = TempDir::new().unwrap();
        let config = FrameworkConfig::new("junit_test")
            .with_report_dir(dir.path())
            .with_report_format(ReportFormat::JUnit);

        ContractTestFramework::new(config)
            .add_suite(counter_test_suite())
            .run()
            .unwrap();

        let xml_path = dir.path().join("framework-report.xml");
        assert!(xml_path.exists());
        let xml = fs::read_to_string(&xml_path).unwrap();
        assert!(xml.contains("<testsuites"));
    }

    #[test]
    fn framework_result_coverage_defaults_zero() {
        let result = FrameworkRunResult {
            config_name: "test".into(),
            suite_results: vec![],
            wasm_summary: None,
            testnet_report: None,
            total: 0,
            passed: 0,
            failed: 0,
            total_duration_ms: 0,
            generated_at: "2024-01-01T00:00:00Z".into(),
        };
        assert_eq!(result.coverage_percent(), 0.0);
        assert!(result.all_passed());
    }
}
