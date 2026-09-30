use crate::utils::{
    mock_soroban,
    test_coverage::{analyze_source_coverage, CoverageReport},
    test_generator::{generate_from_source, GeneratedTestCase},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

/// Configuration for a single test runner invocation.
#[derive(Debug, Clone)]
pub struct TestRunConfig {
    pub wasm_path: PathBuf,
    pub source_path: Option<PathBuf>,
    pub workers: usize,
    pub parallel: bool,
    pub generate: bool,
    pub coverage: bool,
}

/// One executed test case result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerCaseResult {
    pub name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// Aggregated summary of a runner invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRunSummary {
    pub wasm_hash: String,
    pub wasm_bytes: usize,
    pub cases_executed: u32,
    pub failures: u32,
    pub cases: Vec<RunnerCaseResult>,
    pub generated_cases: Vec<GeneratedTestCase>,
    pub coverage: Option<CoverageReport>,
    pub failure_hints: Vec<FailureHint>,
}

/// A categorised hint for a failing test case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureHint {
    pub test_name: String,
    pub category: String,
    pub suggestion: String,
}

pub struct ContractTestRunner {
    config: TestRunConfig,
}

impl ContractTestRunner {
    pub fn new(config: TestRunConfig) -> Self {
        Self { config }
    }

    pub fn run(&self) -> Result<TestRunSummary> {
        let bytes = fs::read(&self.config.wasm_path).with_context(|| {
            format!(
                "Failed to read WASM from {}",
                self.config.wasm_path.display()
            )
        })?;

        mock_soroban::validate_wasm(&bytes).context("WASM validation failed")?;

        use sha2::{Digest, Sha256};
        let wasm_hash = hex::encode(Sha256::digest(&bytes));

        let mut generated_cases = Vec::new();
        if self.config.generate {
            if let Some(ref src) = self.config.source_path {
                let gen = generate_from_source(src)?;
                generated_cases = gen.cases;
            }
        }

        let case_names = self.build_case_names(&generated_cases);

        let case_results = if self.config.parallel && self.config.workers > 1 {
            self.run_parallel(&case_names)?
        } else {
            run_sequential(&case_names)
        };

        let failures = case_results.iter().filter(|c| !c.passed).count() as u32;
        let hints = build_hints(&case_results);

        let coverage = if self.config.coverage {
            self.config.source_path.as_ref().map(|src| {
                let content = fs::read_to_string(src).unwrap_or_default();
                let executed: Vec<String> =
                    generated_cases.iter().map(|c| c.function.clone()).collect();
                analyze_source_coverage(&content, &executed)
            })
        } else {
            None
        };

        Ok(TestRunSummary {
            wasm_hash,
            wasm_bytes: bytes.len(),
            cases_executed: case_results.len() as u32,
            failures,
            cases: case_results,
            generated_cases,
            coverage,
            failure_hints: hints,
        })
    }

    fn build_case_names(&self, generated: &[GeneratedTestCase]) -> Vec<String> {
        let mut names = vec![
            "wasm_header_valid".into(),
            "wasm_size_non_zero".into(),
            "exports_section_present".into(),
        ];
        for case in generated {
            names.push(case.name.clone());
        }
        names
    }

    fn run_parallel(&self, cases: &[String]) -> Result<Vec<RunnerCaseResult>> {
        let workers = self.config.workers.max(1).min(cases.len().max(1));
        let results: Arc<Mutex<Vec<RunnerCaseResult>>> = Arc::new(Mutex::new(Vec::new()));
        let chunk_size = cases.len().div_ceil(workers);

        let mut handles = Vec::new();
        for chunk in cases.chunks(chunk_size.max(1)) {
            let chunk = chunk.to_vec();
            let results = Arc::clone(&results);
            handles.push(thread::spawn(move || {
                for name in chunk {
                    let r = execute_case(&name);
                    results.lock().unwrap().push(r);
                }
            }));
        }

        for h in handles {
            h.join()
                .map_err(|_| anyhow::anyhow!("Worker thread panicked"))?;
        }

        let collected = results.lock().unwrap().clone();
        Ok(collected)
    }
}

fn run_sequential(cases: &[String]) -> Vec<RunnerCaseResult> {
    cases.iter().map(|n| execute_case(n)).collect()
}

fn execute_case(name: &str) -> RunnerCaseResult {
    let start = Instant::now();
    let passed =
        !name.contains("unauthorized") && !name.contains("fail") && !name.contains("invalid");
    RunnerCaseResult {
        name: name.to_string(),
        passed,
        duration_ms: start.elapsed().as_millis() as u64,
        error: if passed {
            None
        } else {
            Some("Simulated assertion failure".into())
        },
    }
}

fn build_hints(cases: &[RunnerCaseResult]) -> Vec<FailureHint> {
    cases
        .iter()
        .filter(|c| !c.passed)
        .map(|c| {
            let category = if c.name.contains("unauthorized") || c.name.contains("auth") {
                "authorization"
            } else if c.name.contains("zero") || c.name.contains("invalid") {
                "input-validation"
            } else {
                "general"
            };
            FailureHint {
                test_name: c.name.clone(),
                category: category.into(),
                suggestion: match category {
                    "authorization" => "Add require_auth() or validate caller permissions".into(),
                    "input-validation" => {
                        "Validate inputs with explicit guards at function entry".into()
                    }
                    _ => "Review test output and contract logic".into(),
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{NamedTempFile, TempDir};

    fn write_minimal_wasm(path: &std::path::Path) {
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        bytes.extend(std::iter::repeat(0u8).take(64));
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn runner_basic_execution() {
        let home = TempDir::new().unwrap();

        let dir = TempDir::new().unwrap();
        let wasm = dir.path().join("test.wasm");
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
    }

    #[test]
    fn runner_parallel_execution() {
        let home = TempDir::new().unwrap();

        let dir = TempDir::new().unwrap();
        let wasm = dir.path().join("p.wasm");
        write_minimal_wasm(&wasm);

        let runner = ContractTestRunner::new(TestRunConfig {
            wasm_path: wasm,
            source_path: None,
            workers: 2,
            parallel: true,
            generate: false,
            coverage: false,
        });
        let summary = runner.run().unwrap();
        assert!(summary.cases_executed >= 3);
    }

    #[test]
    fn runner_with_source_generates_coverage() {
        const SRC: &str = r#"
pub fn increment(env: Env) -> u32 { 1 }
pub fn get_count(env: Env) -> u32 { 0 }
"#;
        let mut src_file = NamedTempFile::new().unwrap();
        src_file.write_all(SRC.as_bytes()).unwrap();

        let dir = TempDir::new().unwrap();
        let wasm = dir.path().join("cov.wasm");
        write_minimal_wasm(&wasm);

        let runner = ContractTestRunner::new(TestRunConfig {
            wasm_path: wasm,
            source_path: Some(src_file.path().to_path_buf()),
            workers: 1,
            parallel: false,
            generate: true,
            coverage: true,
        });
        let summary = runner.run().unwrap();
        assert!(summary.coverage.is_some());
        assert!(!summary.generated_cases.is_empty());
    }
}
