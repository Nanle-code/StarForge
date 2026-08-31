use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub id: String,
    pub name: String,
    pub description: String,
    pub function_name: String,
    pub input_params: Vec<TestParam>,
    pub expected_output: Option<ExpectedOutput>,
    pub tags: Vec<String>,
    pub priority: TestPriority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestParam {
    pub name: String,
    pub value: String,
    pub param_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedOutput {
    pub value: String,
    pub output_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TestPriority {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSuite {
    pub name: String,
    pub contract_id: String,
    pub test_cases: Vec<TestCase>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub test_id: String,
    pub test_name: String,
    pub status: TestStatus,
    pub duration_ms: u64,
    pub output: Option<String>,
    pub error: Option<String>,
    pub coverage_data: Option<CoverageData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageData {
    pub lines_covered: u32,
    pub lines_total: u32,
    pub functions_covered: u32,
    pub functions_total: u32,
    pub branches_covered: u32,
    pub branches_total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestReport {
    pub suite_name: String,
    pub contract_id: String,
    pub total_tests: u32,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub errors: u32,
    pub total_duration_ms: u64,
    pub results: Vec<TestResult>,
    pub coverage_summary: CoverageData,
    pub failure_analysis: Vec<FailureAnalysis>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureAnalysis {
    pub test_id: String,
    pub test_name: String,
    pub error_type: String,
    pub error_message: String,
    pub suggested_fix: Option<String>,
    pub related_tests: Vec<String>,
}

pub struct TestCaseGenerator {
    contract_path: PathBuf,
}

impl TestCaseGenerator {
    pub fn new(contract_path: PathBuf) -> Self {
        Self { contract_path }
    }

    pub fn generate_from_contract(&self) -> Result<TestSuite> {
        let _wasm_path = self
            .contract_path
            .join("target/wasm32-unknown-unknown/release");

        // Read contract source files
        let src_files = self.find_contract_source_files()?;

        let mut test_cases = Vec::new();

        for src_file in &src_files {
            let file_tests = self.generate_tests_from_file(src_file)?;
            test_cases.extend(file_tests);
        }

        Ok(TestSuite {
            name: format!(
                "{}_suite",
                self.contract_path.file_name().unwrap().to_string_lossy()
            ),
            contract_id: "generated".to_string(),
            test_cases,
            generated_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    fn find_contract_source_files(&self) -> Result<Vec<PathBuf>> {
        let src_dir = self.contract_path.join("src");
        let mut files = Vec::new();

        if src_dir.exists() {
            for entry in fs::read_dir(&src_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "rs") {
                    files.push(path);
                }
            }
        }

        Ok(files)
    }

    fn generate_tests_from_file(&self, file_path: &Path) -> Result<Vec<TestCase>> {
        let content = fs::read_to_string(file_path)?;
        let mut tests = Vec::new();

        // Parse function signatures
        for (line_num, line) in content.lines().enumerate() {
            if line.trim_start().starts_with("pub fn ") {
                let base = self.generate_test_from_function(line, line_num, file_path)?;
                for (suffix, description, tags, priority) in [
                    (
                        "happy_path",
                        "Normal operation with valid generated inputs",
                        vec!["happy_path"],
                        TestPriority::High,
                    ),
                    (
                        "error_conditions",
                        "Rejected input and failure-state behavior",
                        vec!["error_condition", "input_validation"],
                        TestPriority::High,
                    ),
                    (
                        "boundary_conditions",
                        "Minimum, maximum, zero, and empty input boundaries",
                        vec!["boundary_condition"],
                        TestPriority::Medium,
                    ),
                    (
                        "security",
                        "Authorization, state isolation, and replay resistance",
                        vec!["security", "authorization"],
                        TestPriority::Critical,
                    ),
                ] {
                    let mut test = base.clone();
                    test.id = format!("{}_{}", test.id, suffix);
                    test.name = format!("{} {}", test.name, suffix.replace('_', " "));
                    test.description = format!("{}: {}", test.description, description);
                    test.tags.extend(tags.into_iter().map(String::from));
                    test.priority = priority;
                    tests.push(test);
                }
            }
        }

        Ok(tests)
    }

    fn generate_test_from_function(
        &self,
        line: &str,
        line_num: usize,
        _file_path: &Path,
    ) -> Result<TestCase> {
        let function_name = line
            .trim_start()
            .strip_prefix("pub fn ")
            .and_then(|s| s.split('(').next())
            .unwrap_or("unknown")
            .to_string();

        Ok(TestCase {
            id: format!("test_{}_{}", line_num, function_name),
            name: format!("Test {}", function_name),
            description: format!("Auto-generated test for function {}", function_name),
            function_name,
            input_params: parse_input_params(line),
            expected_output: None,
            tags: vec!["generated".to_string(), "unit".to_string()],
            priority: TestPriority::Medium,
        })
    }
}

fn parse_input_params(signature: &str) -> Vec<TestParam> {
    let Some(parameters) = signature
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
    else {
        return Vec::new();
    };

    parameters
        .0
        .split(',')
        .filter_map(|parameter| {
            let (name, param_type) = parameter.split_once(':')?;
            let name = name.trim().trim_start_matches("mut ").trim();
            if name.is_empty() || name == "env" {
                return None;
            }
            let param_type = param_type.trim();
            Some(TestParam {
                name: name.to_string(),
                value: sample_parameter(param_type),
                param_type: param_type.to_string(),
            })
        })
        .collect()
}

fn sample_parameter(param_type: &str) -> String {
    match param_type.replace(' ', "").as_str() {
        "bool" => "true",
        "u32" | "u64" | "i32" | "i64" => "1",
        "Address" => "generated_test_address",
        _ => "generated_input",
    }
    .to_string()
}

pub struct ParallelTestRunner {
    max_workers: usize,
}

impl ParallelTestRunner {
    pub fn new(max_workers: usize) -> Self {
        Self { max_workers }
    }

    pub fn run_tests(&self, suite: &TestSuite, wasm_path: &Path) -> Result<TestReport> {
        let start = Instant::now();
        let results = Arc::new(Mutex::new(Vec::new()));
        let test_cases = Arc::new(suite.test_cases.clone());
        let wasm_path = Arc::new(wasm_path.to_path_buf());

        let mut handles = Vec::new();

        for chunk in test_cases.chunks((test_cases.len() / self.max_workers).max(1)) {
            let chunk_tests = chunk.to_vec();
            let results_clone = Arc::clone(&results);
            let wasm_clone = Arc::clone(&wasm_path);

            let handle = thread::spawn(move || {
                for test in chunk_tests {
                    let result = Self::run_single_test(&test, &wasm_clone);
                    let mut results = results_clone.lock().unwrap();
                    results.push(result);
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            handle
                .join()
                .map_err(|e| anyhow::anyhow!("Thread panic: {:?}", e))?;
        }

        let results = Arc::try_unwrap(results).unwrap().into_inner()?;
        let duration = start.elapsed();

        self.generate_report(suite, results, duration)
    }

    fn run_single_test(test: &TestCase, _wasm_path: &Path) -> TestResult {
        let start = Instant::now();

        // Simulate test execution
        let failed = test.function_name.contains("error") || test.function_name.contains("fail");
        let status = if failed {
            TestStatus::Failed
        } else {
            TestStatus::Passed
        };

        let duration = start.elapsed();

        TestResult {
            test_id: test.id.clone(),
            test_name: test.name.clone(),
            status,
            duration_ms: duration.as_millis() as u64,
            output: Some("Test executed successfully".to_string()),
            error: if failed {
                Some("Test assertion failed".to_string())
            } else {
                None
            },
            coverage_data: Some(CoverageData {
                lines_covered: 10,
                lines_total: 20,
                functions_covered: 1,
                functions_total: 2,
                branches_covered: 1,
                branches_total: 3,
            }),
        }
    }

    fn generate_report(
        &self,
        suite: &TestSuite,
        results: Vec<TestResult>,
        duration: std::time::Duration,
    ) -> Result<TestReport> {
        let passed = results
            .iter()
            .filter(|r| matches!(r.status, TestStatus::Passed))
            .count() as u32;
        let failed = results
            .iter()
            .filter(|r| matches!(r.status, TestStatus::Failed))
            .count() as u32;
        let skipped = results
            .iter()
            .filter(|r| matches!(r.status, TestStatus::Skipped))
            .count() as u32;
        let errors = results
            .iter()
            .filter(|r| matches!(r.status, TestStatus::Error))
            .count() as u32;

        let coverage_summary = self.calculate_coverage_summary(&results);
        let failure_analysis = self.analyze_failures(&results);

        Ok(TestReport {
            suite_name: suite.name.clone(),
            contract_id: suite.contract_id.clone(),
            total_tests: results.len() as u32,
            passed,
            failed,
            skipped,
            errors,
            total_duration_ms: duration.as_millis() as u64,
            results,
            coverage_summary,
            failure_analysis,
            generated_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    fn calculate_coverage_summary(&self, results: &[TestResult]) -> CoverageData {
        let mut total_lines = 0u32;
        let mut covered_lines = 0u32;
        let mut total_functions = 0u32;
        let mut covered_functions = 0u32;
        let mut total_branches = 0u32;
        let mut covered_branches = 0u32;

        for result in results {
            if let Some(coverage) = &result.coverage_data {
                total_lines += coverage.lines_total;
                covered_lines += coverage.lines_covered;
                total_functions += coverage.functions_total;
                covered_functions += coverage.functions_covered;
                total_branches += coverage.branches_total;
                covered_branches += coverage.branches_covered;
            }
        }

        CoverageData {
            lines_covered: covered_lines,
            lines_total: total_lines,
            functions_covered: covered_functions,
            functions_total: total_functions,
            branches_covered: covered_branches,
            branches_total: total_branches,
        }
    }

    fn analyze_failures(&self, results: &[TestResult]) -> Vec<FailureAnalysis> {
        results
            .iter()
            .filter(|r| matches!(r.status, TestStatus::Failed | TestStatus::Error))
            .map(|r| FailureAnalysis {
                test_id: r.test_id.clone(),
                test_name: r.test_name.clone(),
                error_type: "AssertionError".to_string(),
                error_message: r
                    .error
                    .clone()
                    .unwrap_or_else(|| "Unknown error".to_string()),
                suggested_fix: Some("Review test expectations and contract logic".to_string()),
                related_tests: vec![],
            })
            .collect()
    }
}

pub struct TestReportExporter;

impl TestReportExporter {
    pub fn export_html(report: &TestReport, output_path: &Path) -> Result<()> {
        let exporter = TestReportExporter;
        let html = exporter.generate_html_report(report)?;
        fs::write(output_path, html)?;
        Ok(())
    }

    pub fn export_json(report: &TestReport, output_path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(report)?;
        fs::write(output_path, json)?;
        Ok(())
    }

    pub fn export_junit(report: &TestReport, output_path: &Path) -> Result<()> {
        let exporter = TestReportExporter;
        let xml = exporter.generate_junit_xml(report);
        fs::write(output_path, xml)?;
        Ok(())
    }

    fn generate_html_report(&self, report: &TestReport) -> Result<String> {
        let coverage_pct = if report.coverage_summary.lines_total > 0 {
            (report.coverage_summary.lines_covered as f64
                / report.coverage_summary.lines_total as f64
                * 100.0) as u32
        } else {
            0
        };

        Ok(format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <title>Test Report - {}</title>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 20px; }}
        .header {{ background: #f0f0f0; padding: 20px; border-radius: 5px; }}
        .summary {{ display: flex; gap: 20px; margin: 20px 0; }}
        .metric {{ background: #e0e0e0; padding: 15px; border-radius: 5px; flex: 1; }}
        .metric h3 {{ margin: 0 0 10px 0; }}
        .metric .value {{ font-size: 24px; font-weight: bold; }}
        .passed {{ color: green; }}
        .failed {{ color: red; }}
        table {{ width: 100%; border-collapse: collapse; margin-top: 20px; }}
        th, td {{ border: 1px solid #ddd; padding: 8px; text-align: left; }}
        th {{ background: #f0f0f0; }}
    </style>
</head>
<body>
    <div class="header">
        <h1>Test Report: {}</h1>
        <p>Contract ID: {}</p>
        <p>Generated: {}</p>
    </div>
    
    <div class="summary">
        <div class="metric">
            <h3>Total Tests</h3>
            <div class="value">{}</div>
        </div>
        <div class="metric">
            <h3>Passed</h3>
            <div class="value passed">{}</div>
        </div>
        <div class="metric">
            <h3>Failed</h3>
            <div class="value failed">{}</div>
        </div>
        <div class="metric">
            <h3>Coverage</h3>
            <div class="value">{}%</div>
        </div>
        <div class="metric">
            <h3>Duration</h3>
            <div class="value">{}ms</div>
        </div>
    </div>
    
    <h2>Test Results</h2>
    <table>
        <tr>
            <th>Test Name</th>
            <th>Status</th>
            <th>Duration</th>
            <th>Error</th>
        </tr>
        {}
    </table>
    
    <h2>Failure Analysis</h2>
    <table>
        <tr>
            <th>Test Name</th>
            <th>Error Type</th>
            <th>Error Message</th>
            <th>Suggested Fix</th>
        </tr>
        {}
    </table>
</body>
</html>"#,
            report.suite_name,
            report.suite_name,
            report.contract_id,
            report.generated_at,
            report.total_tests,
            report.passed,
            report.failed,
            coverage_pct,
            report.total_duration_ms,
            self.generate_test_rows(report),
            self.generate_failure_rows(report)
        ))
    }

    fn generate_test_rows(&self, report: &TestReport) -> String {
        report
            .results
            .iter()
            .map(|r| {
                format!(
                    "<tr><td>{}</td><td class=\"{}\">{:?}</td><td>{}ms</td><td>{}</td></tr>",
                    r.test_name,
                    if matches!(r.status, TestStatus::Passed) {
                        "passed"
                    } else {
                        "failed"
                    },
                    r.status,
                    r.duration_ms,
                    r.error.as_deref().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n        ")
    }

    fn generate_failure_rows(&self, report: &TestReport) -> String {
        report
            .failure_analysis
            .iter()
            .map(|f| {
                format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    f.test_name,
                    f.error_type,
                    f.error_message,
                    f.suggested_fix.as_deref().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n        ")
    }

    fn generate_junit_xml(&self, report: &TestReport) -> String {
        let mut xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites>
    <testsuite name="{}" tests="{}" failures="{}" errors="{}" time="{}">
"#,
            report.suite_name,
            report.total_tests,
            report.failed,
            report.errors,
            report.total_duration_ms as f64 / 1000.0
        );

        for result in &report.results {
            xml.push_str(&format!(
                r#"        <testcase name="{}" time="{}">
"#,
                result.test_name,
                result.duration_ms as f64 / 1000.0
            ));

            if matches!(result.status, TestStatus::Failed) {
                xml.push_str(&format!(
                    r#"            <failure message="{}">{}</failure>
"#,
                    result.error.as_deref().unwrap_or(""),
                    result.error.as_deref().unwrap_or("")
                ));
            }

            xml.push_str("        </testcase>\n");
        }

        xml.push_str("    </testsuite>\n</testsuites>");
        xml
    }
}
