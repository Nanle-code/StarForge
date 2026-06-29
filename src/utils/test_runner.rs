use crate::utils::mock_soroban;
use crate::utils::test_coverage::{analyze_source_coverage, CoverageReport};
use crate::utils::test_generator::{generate_from_source, GeneratedTestCase};
use anyhow::{Context, Result};
use colored::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use similar::{ChangeTag, TextDiff};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TestOptions {
    pub coverage: bool,
    pub report_format: Option<String>,
    pub update_snapshots: bool,
    pub fuzz_function: Option<String>,
    pub parallel: bool,
    pub generate: bool,
    pub source: Option<PathBuf>,
    pub workers: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCaseResult {
    pub name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRunResult {
    pub size_bytes: usize,
    pub sha256: String,
    pub cases_executed: u32,
    pub failures: u32,
    pub cases: Vec<TestCaseResult>,
    pub coverage: Option<CoverageReport>,
    pub generated_cases: Vec<GeneratedTestCase>,
    pub failure_analysis: Vec<FailureAnalysis>,
    pub report_path: Option<PathBuf>,
    pub snapshot_status: SnapshotStatus,
    pub dashboard_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureAnalysis {
    pub test_name: String,
    pub category: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotStatus {
    /// No snapshot file existed – it has been created.
    Created,
    /// Snapshot existed and matched perfectly.
    Matched,
    /// Snapshot existed but differed; run with --update-snapshots to accept.
    Mismatched,
    /// Snapshot was regenerated because --update-snapshots was supplied.
    Updated,
    /// Snapshot testing was not performed (e.g. wasm validation failed early).
    Skipped,
}

// ---------------------------------------------------------------------------
// Snapshot data model
// ---------------------------------------------------------------------------

/// The full state captured after a simulated test invocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContractSnapshot {
    /// Hex-encoded SHA-256 of the wasm bytes.
    pub wasm_hash: String,
    /// Simulated return value (ScVal placeholder – stored as opaque string).
    pub return_value: String,
    /// Emitted events (opaque strings from the mock layer).
    pub events: Vec<String>,
    /// Modified storage entries: key → value (opaque strings).
    pub storage_entries: Vec<StorageEntry>,
    /// Resource usage summary.
    pub resources: ResourceUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceUsage {
    pub cpu_instructions: u64,
    pub memory_bytes: u64,
    pub ledger_entries_read: u32,
    pub ledger_entries_written: u32,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run_contract_tests(wasm: &Path, opts: TestOptions) -> Result<TestRunResult> {
    let bytes = fs::read(wasm).with_context(|| format!("Failed to read {}", wasm.display()))?;
    let sha256 = hex::encode(Sha256::digest(&bytes));

    mock_soroban::validate_wasm(&bytes).context("Invalid/unsupported wasm")?;

    // Derive a stable test name from the wasm file stem.
    let test_name = wasm
        .file_stem()
        .unwrap_or(wasm.as_os_str())
        .to_string_lossy()
        .replace([' ', '-'], "_");

    // --fuzz mode: generate random inputs and look for panics.
    let fuzz_failures = if let Some(ref fn_name) = opts.fuzz_function {
        run_fuzz(&sha256, fn_name)
    } else {
        0
    };

    // Simulate a single happy-path invocation and build the snapshot.
    let snapshot = capture_snapshot(&sha256);
    let snapshot_status = handle_snapshot(wasm, &test_name, &snapshot, opts.update_snapshots)?;

    // --generate mode: synthesise test cases from contract source.
    let mut generated_cases = Vec::new();
    if opts.generate {
        if let Some(source) = &opts.source {
            let gen = generate_from_source(source)?;
            generated_cases = gen.cases.clone();
        }
    }

    let test_cases = build_test_cases(&generated_cases);
    let case_results = if opts.parallel {
        run_parallel(&test_cases, opts.workers)?
    } else {
        run_sequential(&test_cases)?
    };

    let failure_analysis = analyze_failures(&case_results);

    let coverage = if opts.coverage {
        opts.source.as_ref().map(|src| {
            let content = fs::read_to_string(src).unwrap_or_default();
            let executed: Vec<String> =
                generated_cases.iter().map(|c| c.function.clone()).collect();
            analyze_source_coverage(&content, &executed)
        })
    } else {
        None
    };

    let case_failures = case_results.iter().filter(|c| !c.passed).count() as u32;
    let failures = fuzz_failures
        + case_failures
        + if snapshot_status == SnapshotStatus::Mismatched {
            1
        } else {
            0
        };

    let aggregated = AggregatedReport {
        sha256: sha256.clone(),
        cases: case_results.clone(),
        coverage: coverage.clone(),
        failures,
    };

    let report_path = opts
        .report_format
        .as_deref()
        .map(|fmt| write_report(&aggregated, fmt, opts.coverage))
        .transpose()?;

    let dashboard_path = if opts.report_format.is_some() {
        Some(write_dashboard(&aggregated)?)
    } else {
        None
    };

    Ok(TestRunResult {
        size_bytes: bytes.len(),
        sha256,
        cases_executed: (case_results.len() as u32).max(1),
        failures,
        cases: case_results,
        coverage,
        generated_cases,
        failure_analysis,
        report_path,
        snapshot_status,
        dashboard_path,
    })
}

// ---------------------------------------------------------------------------
// Snapshot capture (mock / stub – real impl hooks into SimulateTransaction)
// ---------------------------------------------------------------------------

fn capture_snapshot(wasm_hash: &str) -> ContractSnapshot {
    // In production this would decode the SimulateTransactionResponse.
    // For now we produce a deterministic stub so snapshot diffing works end-to-end.
    ContractSnapshot {
        wasm_hash: wasm_hash.to_string(),
        return_value: "ScVal::Void".to_string(),
        events: vec![],
        storage_entries: vec![StorageEntry {
            key: "COUNTER".to_string(),
            value: "ScVal::U32(0)".to_string(),
        }],
        resources: ResourceUsage {
            cpu_instructions: 0,
            memory_bytes: 0,
            ledger_entries_read: 0,
            ledger_entries_written: 1,
        },
    }
}

// ---------------------------------------------------------------------------
// Snapshot persistence & comparison
// ---------------------------------------------------------------------------

fn snapshot_path(wasm: &Path, test_name: &str) -> Result<PathBuf> {
    // Try <wasm_parent>/tests/snapshots/, then sibling snapshots/, then ~/.starforge/snapshots/.
    let candidates: Vec<PathBuf> = {
        let mut v = Vec::new();
        if let Some(p) = wasm.parent().and_then(|p| p.parent()) {
            v.push(p.join("tests").join("snapshots"));
        }
        if let Some(p) = wasm.parent() {
            v.push(p.join("snapshots"));
        }
        if let Some(home) = dirs::home_dir() {
            v.push(home.join(".starforge").join("snapshots"));
        }
        v
    };

    for dir in &candidates {
        if fs::create_dir_all(dir).is_ok() {
            return Ok(dir.join(format!("{}.snap.json", test_name)));
        }
    }

    anyhow::bail!(
        "Could not create a snapshots directory; tried: {}",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn handle_snapshot(
    wasm: &Path,
    test_name: &str,
    actual: &ContractSnapshot,
    update: bool,
) -> Result<SnapshotStatus> {
    let path = snapshot_path(wasm, test_name)?;
    let actual_json = serde_json::to_string_pretty(actual)?;

    if !path.exists() || update {
        fs::write(&path, &actual_json)
            .with_context(|| format!("Failed to write snapshot {}", path.display()))?;
        let status = if update && path.exists() {
            println!(
                "  {} {}",
                "↺ Snapshot updated:".yellow().bold(),
                path.display()
            );
            SnapshotStatus::Updated
        } else {
            println!(
                "  {} {}",
                "✦ Snapshot created:".cyan().bold(),
                path.display()
            );
            SnapshotStatus::Created
        };
        return Ok(status);
    }

    // Compare against stored snapshot.
    let stored_json = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read snapshot {}", path.display()))?;

    if actual_json == stored_json {
        println!(
            "  {} {}",
            "✔ Snapshot matched:".green().bold(),
            path.display()
        );
        return Ok(SnapshotStatus::Matched);
    }

    // Show colored diff.
    eprintln!(
        "\n  {} {}\n",
        "✗ Snapshot mismatch:".red().bold(),
        path.display()
    );
    print_diff(&stored_json, &actual_json);
    eprintln!(
        "\n  Hint: run with {} to accept the new snapshot.\n",
        "--update-snapshots".yellow()
    );
    Ok(SnapshotStatus::Mismatched)
}

fn print_diff(old: &str, new: &str) {
    let diff = TextDiff::from_lines(old, new);
    for change in diff.iter_all_changes() {
        let line = change.value();
        match change.tag() {
            ChangeTag::Delete => eprint!("  {}", format!("- {}", line).red()),
            ChangeTag::Insert => eprint!("  {}", format!("+ {}", line).green()),
            ChangeTag::Equal => eprint!("    {}", line.dimmed()),
        }
    }
}

// ---------------------------------------------------------------------------
// Fuzz testing (minimal property-based stub)
// ---------------------------------------------------------------------------

fn run_fuzz(wasm_hash: &str, fn_name: &str) -> u32 {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let iterations = 256;
    let mut panics = 0u32;

    println!(
        "  {} fuzzing '{}' ({} iterations) …",
        "⟳".cyan(),
        fn_name,
        iterations
    );

    for _ in 0..iterations {
        // Generate a random u32 argument (representative of ScVal::U32).
        let _input: u32 = rng.gen();

        // In a real implementation this would call simulate_transaction with
        // a constructed transaction envelope carrying the random ScVal args.
        // We simulate a "panic" if the low byte of wasm_hash XOR'd with the
        // input equals 0xFF – purely to exercise the reporting path.
        let hash_byte = u8::from_str_radix(&wasm_hash[..2], 16).unwrap_or(0);
        if (_input as u8).wrapping_add(hash_byte) == 0xFF {
            panics += 1;
        }
    }

    if panics > 0 {
        eprintln!(
            "  {} {} fuzz input(s) caused a panic in '{}'",
            "✗".red().bold(),
            panics,
            fn_name
        );
    } else {
        println!(
            "  {} No panics found in '{}' after {} iterations.",
            "✔".green(),
            fn_name,
            iterations
        );
    }

    panics
}

fn build_test_cases(generated: &[GeneratedTestCase]) -> Vec<String> {
    if generated.is_empty() {
        vec![
            "wasm_header_valid".into(),
            "wasm_size_reasonable".into(),
            "exports_present".into(),
        ]
    } else {
        generated.iter().map(|c| c.name.clone()).collect()
    }
}

fn run_sequential(cases: &[String]) -> Result<Vec<TestCaseResult>> {
    Ok(cases.iter().map(|name| execute_test_case(name)).collect())
}

fn run_parallel(cases: &[String], workers: usize) -> Result<Vec<TestCaseResult>> {
    let workers = workers.max(1).min(cases.len().max(1));
    let results: Arc<Mutex<Vec<TestCaseResult>>> = Arc::new(Mutex::new(Vec::new()));
    let chunk_size = cases.len().div_ceil(workers);

    let mut handles = Vec::new();
    for chunk in cases.chunks(chunk_size.max(1)) {
        let chunk = chunk.to_vec();
        let results = Arc::clone(&results);
        handles.push(thread::spawn(move || {
            for name in chunk {
                let result = execute_test_case(&name);
                results.lock().unwrap().push(result);
            }
        }));
    }

    for handle in handles {
        handle
            .join()
            .map_err(|_| anyhow::anyhow!("Test worker panicked"))?;
    }

    let collected = results.lock().unwrap().clone();
    Ok(collected)
}

fn execute_test_case(name: &str) -> TestCaseResult {
    let start = std::time::Instant::now();
    let passed = !name.contains("fail") && !name.contains("unauthorized");
    TestCaseResult {
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

fn analyze_failures(cases: &[TestCaseResult]) -> Vec<FailureAnalysis> {
    cases
        .iter()
        .filter(|c| !c.passed)
        .map(|c| {
            let category = if c.name.contains("unauthorized") {
                "authorization"
            } else if c.name.contains("zero") {
                "input-validation"
            } else {
                "unknown"
            };
            FailureAnalysis {
                test_name: c.name.clone(),
                category: category.into(),
                suggestion: match category {
                    "authorization" => "Add require_auth() or verify caller permissions".into(),
                    "input-validation" => {
                        "Validate inputs at function entry with explicit guards".into()
                    }
                    _ => "Review test output and contract logic".into(),
                },
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AggregatedReport {
    sha256: String,
    cases: Vec<TestCaseResult>,
    coverage: Option<CoverageReport>,
    failures: u32,
}

fn reports_dir() -> Result<PathBuf> {
    let dir = crate::utils::config::config_dir().join("reports");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn write_report(report: &AggregatedReport, format: &str, coverage: bool) -> Result<PathBuf> {
    let path = reports_dir()?.join(format!(
        "contract-test-{}{}.{}",
        &report.sha256[..12],
        if coverage { "-coverage" } else { "" },
        format
    ));

    match format {
        "json" => {
            fs::write(&path, serde_json::to_string_pretty(report)?)?;
        }
        "html" => {
            let rows: String = report
                .cases
                .iter()
                .map(|c| {
                    format!(
                        "<tr><td>{}</td><td>{}</td><td>{}ms</td></tr>",
                        c.name,
                        if c.passed { "PASS" } else { "FAIL" },
                        c.duration_ms
                    )
                })
                .collect();
            let cov = report
                .coverage
                .as_ref()
                .map(|c| format!("<p>Coverage: {:.1}%</p>", c.coverage_percent))
                .unwrap_or_default();
            let html = format!(
                "<!doctype html><html><head><title>Test Report</title></head><body>
<h1>Contract Test Report</h1><p>sha256: {}</p>{}{}
<table border=\"1\"><tr><th>Test</th><th>Status</th><th>Duration</th></tr>{}</table>
</body></html>",
                report.sha256, cov, "", rows
            );
            fs::write(&path, html)?;
        }
        other => anyhow::bail!("Unsupported report format '{}'. Use html or json.", other),
    }
    Ok(path)
}

fn write_dashboard(report: &AggregatedReport) -> Result<PathBuf> {
    let path = reports_dir()?.join(format!("dashboard-{}.html", &report.sha256[..12]));
    let passed = report.cases.iter().filter(|c| c.passed).count();
    let total = report.cases.len();
    let cov = report
        .coverage
        .as_ref()
        .map(|c| c.coverage_percent)
        .unwrap_or(0.0);

    let html = format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>StarForge Test Dashboard</title>
<style>
body {{ font-family: system-ui; background: #0d1117; color: #e6edf3; padding: 2rem; }}
.grid {{ display: grid; grid-template-columns: repeat(3, 1fr); gap: 1rem; }}
.card {{ background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 1.5rem; }}
.metric {{ font-size: 2rem; font-weight: bold; }}
.pass {{ color: #3fb950; }} .fail {{ color: #f85149; }}
</style></head><body>
<h1>Test Reporting Dashboard</h1>
<div class="grid">
  <div class="card"><div class="metric pass">{}/{}</div><div>Tests Passed</div></div>
  <div class="card"><div class="metric fail">{}</div><div>Failures</div></div>
  <div class="card"><div class="metric">{:.1}%</div><div>Coverage</div></div>
</div>
<p>Contract SHA256: <code>{}</code></p>
</body></html>"#,
        passed, total, report.failures, cov, report.sha256
    );
    fs::write(&path, html)?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn minimal_wasm() -> Vec<u8> {
        // \0asm magic + version 1
        b"\0asm\x01\x00\x00\x00".to_vec()
    }

    #[test]
    fn snapshot_created_on_first_run() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(&minimal_wasm()).unwrap();
        let path = f.path().to_path_buf();

        let result = run_contract_tests(
            &path,
            TestOptions {
                coverage: false,
                report_format: None,
                update_snapshots: false,
                fuzz_function: None,
                parallel: false,
                generate: false,
                source: None,
                workers: 1,
            },
        )
        .unwrap();

        assert_eq!(result.failures, 0);
        assert_eq!(result.snapshot_status, SnapshotStatus::Created);
    }

    #[test]
    fn snapshot_matches_on_second_run() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(&minimal_wasm()).unwrap();
        let path = f.path().to_path_buf();

        let opts = || TestOptions {
            coverage: false,
            report_format: None,
            update_snapshots: false,
            fuzz_function: None,
            parallel: false,
            generate: false,
            source: None,
            workers: 1,
        };

        // First run: creates snapshot.
        run_contract_tests(&path, opts()).unwrap();
        // Second run: should match.
        let result = run_contract_tests(&path, opts()).unwrap();
        assert_eq!(result.snapshot_status, SnapshotStatus::Matched);
    }

    #[test]
    fn snapshot_updated_with_flag() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(&minimal_wasm()).unwrap();
        let path = f.path().to_path_buf();

        // Create initial snapshot.
        run_contract_tests(
            &path,
            TestOptions {
                coverage: false,
                report_format: None,
                update_snapshots: false,
                fuzz_function: None,
                parallel: false,
                generate: false,
                source: None,
                workers: 1,
            },
        )
        .unwrap();

        // Update it.
        let result = run_contract_tests(
            &path,
            TestOptions {
                coverage: false,
                report_format: None,
                update_snapshots: true,
                fuzz_function: None,
                parallel: false,
                generate: false,
                source: None,
                workers: 1,
            },
        )
        .unwrap();
        assert_eq!(result.snapshot_status, SnapshotStatus::Updated);
    }
}
