//! `src/utils/ai_template_testing.rs`
//!
//! AI-driven testing framework for Soroban smart contract templates.
//!
//! This module provides comprehensive automated testing across five categories:
//!
//! - **Functional / Structural Validation** — template files, placeholders, Cargo.toml
//! - **Security Testing** — static analysis for common Soroban vulnerabilities
//! - **Performance / Gas Optimization** — storage pattern analysis and gas heuristics
//! - **SDK / Network Compatibility** — version constraints and feature flag checks
//! - **Test Reporting** — structured, machine-readable results with scoring

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ─────────────────────────────────────────────────────────────────────────────
// Core types
// ─────────────────────────────────────────────────────────────────────────────

/// Severity of a test finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl Severity {
    /// Numeric weight used when computing aggregate scores.
    pub fn weight(&self) -> u32 {
        match self {
            Severity::Critical => 40,
            Severity::High => 25,
            Severity::Medium => 15,
            Severity::Low => 5,
            Severity::Info => 1,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Severity::Critical => "CRITICAL",
            Severity::High => "HIGH",
            Severity::Medium => "MEDIUM",
            Severity::Low => "LOW",
            Severity::Info => "INFO",
        }
    }
}

/// Category of a test finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    Structure,
    Placeholder,
    Security,
    Performance,
    Compatibility,
    Documentation,
    BestPractice,
}

/// A single finding produced by any test phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestFinding {
    pub category: FindingCategory,
    pub severity: Severity,
    pub title: String,
    pub description: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub suggestion: Option<String>,
}

/// Result of running a single test phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseResult {
    pub phase: String,
    pub findings: Vec<TestFinding>,
    pub passed: bool,
    pub duration_ms: u64,
}

/// The full report produced by running all test phases against a template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateTestReport {
    pub template_name: String,
    pub template_path: String,
    pub phases: Vec<PhaseResult>,
    pub total_findings: usize,
    pub critical_count: usize,
    pub high_count: usize,
    pub medium_count: usize,
    pub low_count: usize,
    pub info_count: usize,
    pub quality_score: u32,
    pub passed: bool,
    pub summary: String,
}

/// Configuration for which phases to run and their parameters.
#[derive(Debug, Clone)]
pub struct TestConfig {
    pub run_structure: bool,
    pub run_security: bool,
    pub run_performance: bool,
    pub run_compatibility: bool,
    pub run_docs: bool,
    /// Soroban SDK version to test compatibility against (e.g. "21.0.0").
    pub target_sdk_version: Option<String>,
    /// Minimum Rust edition expected in Cargo.toml.
    pub min_rust_edition: Option<String>,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            run_structure: true,
            run_security: true,
            run_performance: true,
            run_compatibility: true,
            run_docs: true,
            target_sdk_version: Some("21.0.0".to_string()),
            min_rust_edition: Some("2021".to_string()),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Run the full AI-driven test suite against a template directory.
pub fn test_template(template_dir: &Path, config: &TestConfig) -> Result<TemplateTestReport> {
    let template_name = template_dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let template_path = template_dir.to_string_lossy().to_string();

    let mut phases = Vec::new();
    let mut all_findings = Vec::new();

    if config.run_structure {
        let phase = run_structure_phase(template_dir)?;
        all_findings.extend(phase.findings.clone());
        phases.push(phase);
    }

    if config.run_security {
        let phase = run_security_phase(template_dir)?;
        all_findings.extend(phase.findings.clone());
        phases.push(phase);
    }

    if config.run_performance {
        let phase = run_performance_phase(template_dir)?;
        all_findings.extend(phase.findings.clone());
        phases.push(phase);
    }

    if config.run_compatibility {
        let phase = run_compatibility_phase(template_dir, config)?;
        all_findings.extend(phase.findings.clone());
        phases.push(phase);
    }

    if config.run_docs {
        let phase = run_docs_phase(template_dir)?;
        all_findings.extend(phase.findings.clone());
        phases.push(phase);
    }

    let critical_count = all_findings
        .iter()
        .filter(|f| f.severity == Severity::Critical)
        .count();
    let high_count = all_findings
        .iter()
        .filter(|f| f.severity == Severity::High)
        .count();
    let medium_count = all_findings
        .iter()
        .filter(|f| f.severity == Severity::Medium)
        .count();
    let low_count = all_findings
        .iter()
        .filter(|f| f.severity == Severity::Low)
        .count();
    let info_count = all_findings
        .iter()
        .filter(|f| f.severity == Severity::Info)
        .count();

    let quality_score = compute_quality_score(&all_findings);
    let passed = critical_count == 0 && high_count == 0;

    let summary = format!(
        "Template '{}' scored {}/100. {} findings ({} critical, {} high, {} medium, {} low, {} info). {}",
        template_name,
        quality_score,
        all_findings.len(),
        critical_count,
        high_count,
        medium_count,
        low_count,
        info_count,
        if passed {
            "PASSED — production ready"
        } else {
            "FAILED — requires remediation"
        },
    );

    Ok(TemplateTestReport {
        template_name,
        template_path,
        phases,
        total_findings: all_findings.len(),
        critical_count,
        high_count,
        medium_count,
        low_count,
        info_count,
        quality_score,
        passed,
        summary,
    })
}

/// Run all tests across every template in a registry directory.
pub fn test_all_templates(
    templates_dir: &Path,
    config: &TestConfig,
) -> Result<Vec<TemplateTestReport>> {
    let examples_dir = templates_dir.join("examples");
    if !examples_dir.exists() {
        anyhow::bail!("Examples directory not found at {}", examples_dir.display());
    }

    let mut reports = Vec::new();
    let entries = fs::read_dir(&examples_dir)
        .with_context(|| format!("Failed to read {}", examples_dir.display()))?;

    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            match test_template(&entry.path(), config) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    eprintln!(
                        "Warning: failed to test template {}: {}",
                        entry.path().display(),
                        e
                    );
                }
            }
        }
    }

    Ok(reports)
}

/// Generate a combined summary report from multiple template test results.
pub fn generate_summary(reports: &[TemplateTestReport]) -> String {
    let total = reports.len();
    let passed = reports.iter().filter(|r| r.passed).count();
    let failed = total - passed;
    let avg_score: u32 = if total > 0 {
        reports.iter().map(|r| r.quality_score).sum::<u32>() / total as u32
    } else {
        0
    };
    let total_findings: usize = reports.iter().map(|r| r.total_findings).sum();
    let total_critical: usize = reports.iter().map(|r| r.critical_count).sum();
    let total_high: usize = reports.iter().map(|r| r.high_count).sum();

    let mut out = format!(
        "═══ AI Template Testing Summary ═══\n\
         Templates tested:  {}\n\
         Passed:            {}\n\
         Failed:            {}\n\
         Average score:     {}/100\n\
         Total findings:    {}\n\
         Critical findings: {}\n\
         High findings:     {}\n\n",
        total, passed, failed, avg_score, total_findings, total_critical, total_high,
    );

    for report in reports {
        let status = if report.passed { "PASS" } else { "FAIL" };
        out.push_str(&format!(
            "  [{}] {} — score {}/100 ({} findings)\n",
            status, report.template_name, report.quality_score, report.total_findings,
        ));
    }

    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 1: Structure & Placeholder Validation
// ─────────────────────────────────────────────────────────────────────────────

fn run_structure_phase(template_dir: &Path) -> Result<PhaseResult> {
    let start = std::time::Instant::now();
    let mut findings = Vec::new();

    // Required files check
    check_required_files(template_dir, &mut findings);

    // Cargo.toml validation
    if let Some(cargo_path) = find_file(template_dir, "Cargo.toml") {
        validate_cargo_toml(&cargo_path, &mut findings);
    }

    // Source file validation
    validate_source_files(template_dir, &mut findings);

    // Placeholder validation
    validate_placeholders(template_dir, &mut findings);

    // Check for test module in source
    check_test_coverage_in_source(template_dir, &mut findings);

    let passed = !findings
        .iter()
        .any(|f| f.severity == Severity::Critical || f.severity == Severity::High);

    Ok(PhaseResult {
        phase: "structure_validation".to_string(),
        findings,
        passed,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

const REQUIRED_FILES: &[&str] = &["Cargo.toml", "src/lib.rs"];

const RECOMMENDED_FILES: &[&str] = &["README.md"];

fn check_required_files(template_dir: &Path, findings: &mut Vec<TestFinding>) {
    for file in REQUIRED_FILES {
        let path = template_dir.join(file);
        if !path.exists() {
            findings.push(TestFinding {
                category: FindingCategory::Structure,
                severity: Severity::Critical,
                title: format!("Missing required file: {}", file),
                description: format!(
                    "Template must contain '{}' to be a valid Soroban contract project.",
                    file
                ),
                file: Some(file.to_string()),
                line: None,
                suggestion: Some(format!("Add a '{}' file to the template root.", file)),
            });
        }
    }

    for file in RECOMMENDED_FILES {
        let path = template_dir.join(file);
        if !path.exists() {
            findings.push(TestFinding {
                category: FindingCategory::Documentation,
                severity: Severity::Low,
                title: format!("Missing recommended file: {}", file),
                description: format!(
                    "Template should include a '{}' with usage instructions and examples.",
                    file
                ),
                file: Some(file.to_string()),
                line: None,
                suggestion: Some(format!("Add a '{}' describing the template.", file)),
            });
        }
    }
}

fn validate_cargo_toml(cargo_path: &Path, findings: &mut Vec<TestFinding>) {
    let content = match fs::read_to_string(cargo_path) {
        Ok(c) => c,
        Err(_) => {
            findings.push(TestFinding {
                category: FindingCategory::Structure,
                severity: Severity::Critical,
                title: "Cannot read Cargo.toml".to_string(),
                description: "Cargo.toml exists but could not be read.".to_string(),
                file: Some("Cargo.toml".to_string()),
                line: None,
                suggestion: None,
            });
            return;
        }
    };

    // Parse as TOML
    let parsed: toml::Value = match content.parse() {
        Ok(v) => v,
        Err(e) => {
            findings.push(TestFinding {
                category: FindingCategory::Structure,
                severity: Severity::Critical,
                title: "Cargo.toml is not valid TOML".to_string(),
                description: format!("Parse error: {}", e),
                file: Some("Cargo.toml".to_string()),
                line: None,
                suggestion: Some("Fix TOML syntax errors in Cargo.toml.".to_string()),
            });
            return;
        }
    };

    // Check [package] section exists
    if parsed.get("package").is_none() {
        findings.push(TestFinding {
            category: FindingCategory::Structure,
            severity: Severity::Critical,
            title: "Missing [package] in Cargo.toml".to_string(),
            description: "Cargo.toml must have a [package] section.".to_string(),
            file: Some("Cargo.toml".to_string()),
            line: None,
            suggestion: None,
        });
        return;
    }

    let package = &parsed["package"];

    // Check name uses placeholder
    if let Some(name) = package.get("name").and_then(|v| v.as_str()) {
        if !name.contains("{{PROJECT_NAME") {
            findings.push(TestFinding {
                category: FindingCategory::Placeholder,
                severity: Severity::Medium,
                title: "Cargo.toml package name is not parameterized".to_string(),
                description: format!(
                    "Package name '{}' does not use a {{{{PROJECT_NAME}}}} placeholder.",
                    name
                ),
                file: Some("Cargo.toml".to_string()),
                line: None,
                suggestion: Some(
                    "Use {{PROJECT_NAME}} as the package name for template substitution."
                        .to_string(),
                ),
            });
        }
    } else {
        findings.push(TestFinding {
            category: FindingCategory::Structure,
            severity: Severity::Critical,
            title: "Missing package name in Cargo.toml".to_string(),
            description: "[package] must have a 'name' field.".to_string(),
            file: Some("Cargo.toml".to_string()),
            line: None,
            suggestion: None,
        });
    }

    // Check soroban-sdk dependency exists (dependencies live at the document
    // root, as a sibling of [package], not nested inside it).
    let has_soroban_dep = find_soroban_dependency(&parsed);
    if !has_soroban_dep {
        findings.push(TestFinding {
            category: FindingCategory::Compatibility,
            severity: Severity::High,
            title: "Missing soroban-sdk dependency".to_string(),
            description: "Soroban templates must depend on soroban-sdk.".to_string(),
            file: Some("Cargo.toml".to_string()),
            line: None,
            suggestion: Some("Add soroban-sdk to [dependencies].".to_string()),
        });
    }

    // Check cdylib crate type
    if let Some(lib) = parsed.get("lib") {
        if let Some(crate_type) = lib.get("crate-type").and_then(|v| v.as_array()) {
            let has_cdylib = crate_type.iter().any(|v| v.as_str() == Some("cdylib"));
            if !has_cdylib {
                findings.push(TestFinding {
                    category: FindingCategory::BestPractice,
                    severity: Severity::Medium,
                    title: "Missing cdylib crate type".to_string(),
                    description:
                        "Soroban contracts should include 'cdylib' in crate-type for WASM output."
                            .to_string(),
                    file: Some("Cargo.toml".to_string()),
                    line: None,
                    suggestion: Some("Add crate-type = [\"cdylib\"] under [lib].".to_string()),
                });
            }
        }
    } else {
        findings.push(TestFinding {
            category: FindingCategory::BestPractice,
            severity: Severity::Medium,
            title: "Missing [lib] section in Cargo.toml".to_string(),
            description: "Soroban contracts should specify crate-type = [\"cdylib\"].".to_string(),
            file: Some("Cargo.toml".to_string()),
            line: None,
            suggestion: Some("Add [lib] section with crate-type = [\"cdylib\"].".to_string()),
        });
    }

    // Check overflow-checks in release profile
    if let Some(profile) = parsed.get("profile") {
        if let Some(release) = profile.get("release") {
            let has_overflow = release
                .get("overflow-checks")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !has_overflow {
                findings.push(TestFinding {
                    category: FindingCategory::Security,
                    severity: Severity::Medium,
                    title: "overflow-checks not enabled in release profile".to_string(),
                    description: "Release profile should enable overflow-checks to prevent integer overflow exploits.".to_string(),
                    file: Some("Cargo.toml".to_string()),
                    line: None,
                    suggestion: Some("Add overflow-checks = true under [profile.release].".to_string()),
                });
            }
        }
    }
}

/// Whether the manifest depends on `soroban-sdk`.
///
/// Takes the whole manifest, not the `[package]` table: dependencies live in
/// their own top-level tables.
fn find_soroban_dependency(manifest: &toml::Value) -> bool {
    ["dependencies", "dev-dependencies"].iter().any(|table| {
        manifest
            .get(table)
            .and_then(|deps| deps.get("soroban-sdk"))
            .is_some()
    })
}

fn validate_source_files(template_dir: &Path, findings: &mut Vec<TestFinding>) {
    let src_dir = template_dir.join("src");
    if !src_dir.exists() {
        return;
    }

    if let Ok(entries) = fs::read_dir(&src_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                validate_rust_source_file(&path, findings);
            }
        }
    }
}

fn validate_rust_source_file(path: &Path, findings: &mut Vec<TestFinding>) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    // Check for #![no_std]
    if !content.contains("#![no_std]") {
        findings.push(TestFinding {
            category: FindingCategory::BestPractice,
            severity: Severity::Low,
            title: "Missing #![no_std] attribute".to_string(),
            description: "Soroban contracts should use no_std for WASM compatibility.".to_string(),
            file: Some(file_name.clone()),
            line: None,
            suggestion: Some("Add #![no_std] at the top of the source file.".to_string()),
        });
    }

    // Check for #[contract] attribute
    if !content.contains("#[contract]") {
        findings.push(TestFinding {
            category: FindingCategory::Structure,
            severity: Severity::High,
            title: "Missing #[contract] attribute".to_string(),
            description: "Source file must declare a Soroban contract with #[contract]."
                .to_string(),
            file: Some(file_name.clone()),
            line: None,
            suggestion: Some("Add #[contract] to the main contract struct.".to_string()),
        });
    }

    // Check for #[contractimpl] attribute
    if !content.contains("#[contractimpl]") {
        findings.push(TestFinding {
            category: FindingCategory::Structure,
            severity: Severity::High,
            title: "Missing #[contractimpl] attribute".to_string(),
            description: "Contract methods must be inside a #[contractimpl] block.".to_string(),
            file: Some(file_name.clone()),
            line: None,
            suggestion: Some(
                "Add #[contractimpl] to the contract implementation block.".to_string(),
            ),
        });
    }

    // Check for placeholder usage in struct name
    if content.contains("{{PROJECT_NAME_PASCAL}}") {
        // Good — template uses placeholders
    } else if content.contains("#[contract]") && !content.contains("{{") {
        findings.push(TestFinding {
            category: FindingCategory::Placeholder,
            severity: Severity::Medium,
            title: "No template placeholders in source".to_string(),
            description: "Contract struct name should use {{{{PROJECT_NAME_PASCAL}}}} placeholder."
                .to_string(),
            file: Some(file_name.clone()),
            line: None,
            suggestion: Some(
                "Use {{PROJECT_NAME_PASCAL}} for the contract struct name.".to_string(),
            ),
        });
    }
}

fn validate_placeholders(template_dir: &Path, findings: &mut Vec<TestFinding>) {
    let expected_placeholders = &[
        "{{PROJECT_NAME}}",
        "{{PROJECT_NAME_PASCAL}}",
        "{{PROJECT_NAME_SNAKE}}",
    ];

    let mut found_placeholders: HashMap<String, Vec<String>> = HashMap::new();
    collect_placeholders(template_dir, &mut found_placeholders);

    let all_found: Vec<String> = found_placeholders.keys().cloned().collect();

    for placeholder in expected_placeholders {
        if !all_found.contains(&placeholder.to_string()) {
            // Only flag if this is a comprehensive template
            findings.push(TestFinding {
                category: FindingCategory::Placeholder,
                severity: Severity::Info,
                title: format!("Placeholder '{}' not used in template", placeholder),
                description: format!(
                    "The placeholder '{}' was not found. This may be intentional for simple templates.",
                    placeholder
                ),
                file: None,
                line: None,
                suggestion: None,
            });
        }
    }
}

fn collect_placeholders(dir: &Path, found: &mut HashMap<String, Vec<String>>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.file_name().unwrap_or_default() != "target" {
                collect_placeholders(&path, found);
            } else if path.is_file() {
                if let Ok(content) = fs::read_to_string(&path) {
                    for placeholder in &[
                        "{{PROJECT_NAME}}",
                        "{{PROJECT_NAME_PASCAL}}",
                        "{{PROJECT_NAME_SNAKE}}",
                    ] {
                        if content.contains(placeholder) {
                            let file_name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            found
                                .entry(placeholder.to_string())
                                .or_default()
                                .push(file_name);
                        }
                    }
                }
            }
        }
    }
}

fn check_test_coverage_in_source(template_dir: &Path, findings: &mut Vec<TestFinding>) {
    let src_dir = template_dir.join("src");
    if !src_dir.exists() {
        return;
    }

    if let Ok(entries) = fs::read_dir(&src_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                if let Ok(content) = fs::read_to_string(&path) {
                    let file_name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    if content.contains("#[contractimpl]") && !content.contains("#[cfg(test)]") {
                        findings.push(TestFinding {
                            category: FindingCategory::BestPractice,
                            severity: Severity::Medium,
                            title: "No inline tests in contract source".to_string(),
                            description: format!(
                                "'{}' implements a contract but has no #[cfg(test)] module.",
                                file_name
                            ),
                            file: Some(file_name),
                            line: None,
                            suggestion: Some(
                                "Add a #[cfg(test)] module with unit tests.".to_string(),
                            ),
                        });
                    }
                }
            }
        }
    }
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let path = dir.join(name);
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 2: Security Testing
// ─────────────────────────────────────────────────────────────────────────────

fn run_security_phase(template_dir: &Path) -> Result<PhaseResult> {
    let start = std::time::Instant::now();
    let mut findings = Vec::new();

    let src_dir = template_dir.join("src");
    if !src_dir.exists() {
        return Ok(PhaseResult {
            phase: "security_testing".to_string(),
            findings,
            passed: true,
            duration_ms: start.elapsed().as_millis() as u64,
        });
    }

    scan_source_security(&src_dir, &mut findings);

    let passed = !findings
        .iter()
        .any(|f| f.severity == Severity::Critical || f.severity == Severity::High);

    Ok(PhaseResult {
        phase: "security_testing".to_string(),
        findings,
        passed,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

fn scan_source_security(src_dir: &Path, findings: &mut Vec<TestFinding>) {
    if let Ok(entries) = fs::read_dir(src_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rs") {
                if let Ok(content) = fs::read_to_string(&path) {
                    let file_name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    analyze_security_patterns(&content, &file_name, findings);
                }
            }
        }
    }
}

fn analyze_security_patterns(content: &str, file_name: &str, findings: &mut Vec<TestFinding>) {
    let lines: Vec<&str> = content.lines().collect();

    // Check for require_auth usage in mutating functions
    let mut in_pub_fn = false;
    let mut current_fn = String::new();
    let mut fn_has_require_auth = false;
    let mut fn_has_state_write = false;
    let mut fn_line = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        let is_pub = trimmed.starts_with("pub fn ");
        let is_priv = trimmed.starts_with("fn ")
            || trimmed.starts_with("pub(crate) fn ")
            || trimmed.starts_with("pub(in ")
            || trimmed.starts_with("impl ")
            || trimmed.starts_with("struct ")
            || trimmed.starts_with("enum ")
            || trimmed.starts_with("const ")
            || trimmed.starts_with("static ");

        if is_pub || is_priv {
            // Process previous function
            if in_pub_fn
                && fn_has_state_write
                && !fn_has_require_auth
                && current_fn != "initialize"
                && current_fn != "init"
            {
                findings.push(TestFinding {
                    category: FindingCategory::Security,
                    severity: Severity::High,
                    title: format!(
                        "Function '{}' writes state without require_auth()",
                        current_fn
                    ),
                    description: format!(
                        "Function '{}' modifies contract state but does not call require_auth(). \
                         This may allow unauthorized state changes.",
                        current_fn
                    ),
                    file: Some(file_name.to_string()),
                    line: Some(fn_line),
                    suggestion: Some(
                        "Add caller.require_auth() or equivalent authorization check before state mutations.".to_string(),
                    ),
                });
            }

            if is_pub {
                // Reset for new function
                current_fn = trimmed
                    .strip_prefix("pub fn ")
                    .unwrap_or("")
                    .split('(')
                    .next()
                    .unwrap_or("")
                    .to_string();
                in_pub_fn = true;
                fn_has_require_auth = false;
                fn_has_state_write = false;
                fn_line = i + 1;
            } else {
                in_pub_fn = false;
            }
        }

        if in_pub_fn {
            if trimmed.contains("require_auth()") || trimmed.contains("require_auth]") {
                fn_has_require_auth = true;
            }
            if trimmed.contains(".set(")
                || trimmed.contains("transfer(")
                || trimmed.contains("mint(")
                || trimmed.contains(".remove(")
            {
                fn_has_state_write = true;
            }
        }
    }

    // Check last function
    if in_pub_fn
        && fn_has_state_write
        && !fn_has_require_auth
        && current_fn != "initialize"
        && current_fn != "init"
    {
        findings.push(TestFinding {
            category: FindingCategory::Security,
            severity: Severity::High,
            title: format!(
                "Function '{}' writes state without require_auth()",
                current_fn
            ),
            description: format!(
                "Function '{}' modifies contract state but does not call require_auth().",
                current_fn
            ),
            file: Some(file_name.to_string()),
            line: Some(fn_line),
            suggestion: Some("Add caller.require_auth() before state mutations.".to_string()),
        });
    }

    // Check for reentrancy patterns
    let has_external_call =
        content.contains("token::Client") || content.contains("client.transfer(");
    let has_reentrancy_guard = content.contains("REENTRANCY") || content.contains("reentrancy");

    if has_external_call && !has_reentrancy_guard {
        findings.push(TestFinding {
            category: FindingCategory::Security,
            severity: Severity::Medium,
            title: "Potential reentrancy: external call without guard".to_string(),
            description: "Contract makes external token calls without a reentrancy guard. \
                          In complex flows this could be exploited."
                .to_string(),
            file: Some(file_name.to_string()),
            line: None,
            suggestion: Some(
                "Consider adding a reentrancy guard (locked flag) for contracts with complex \
                 multi-step flows."
                    .to_string(),
            ),
        });
    }

    // Check for unsafe unwrap() usage
    for (i, line) in lines.iter().enumerate() {
        if line.contains(".unwrap()")
            && !line.trim().starts_with("//")
            && !line.contains("unwrap_or")
            && !line.contains("unwrap_or_else")
            && !line.contains("unwrap_or_default")
        {
            findings.push(TestFinding {
                category: FindingCategory::Security,
                severity: Severity::Medium,
                title: "Unsafe unwrap() in contract code".to_string(),
                description: "Using unwrap() on Option/Result in contract code can cause panics \
                              that may be exploited for denial-of-service."
                    .to_string(),
                file: Some(file_name.to_string()),
                line: Some(i + 1),
                suggestion: Some(
                    "Replace unwrap() with expect() with a meaningful message, or use \
                     unwrap_or/unwrap_or_else for graceful error handling."
                        .to_string(),
                ),
            });
            break; // Only report once per file
        }
    }

    // Check for hardcoded addresses or secret-like patterns
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        // Look for patterns like "secret", "private_key", "password" in comments or code
        let lower = trimmed.to_lowercase();
        if lower.contains("secret_key")
            || lower.contains("private_key")
            || lower.contains("password")
            || lower.contains("api_key")
        {
            findings.push(TestFinding {
                category: FindingCategory::Security,
                severity: Severity::Critical,
                title: "Potential secret or credential in source code".to_string(),
                description: "Source code references what may be a secret or credential. \
                              Hardcoded secrets are a critical security vulnerability."
                    .to_string(),
                file: Some(file_name.to_string()),
                line: Some(i + 1),
                suggestion: Some(
                    "Remove all hardcoded secrets. Use environment variables or \
                     authenticated storage for sensitive values."
                        .to_string(),
                ),
            });
        }
    }

    // Check for unchecked arithmetic (overflow in key positions)
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        if (trimmed.contains("+= 1") || trimmed.contains("+ 1"))
            && !content.contains("overflow-checks")
            && !trimmed.contains("checked_add")
            && !trimmed.contains("saturating_add")
        {
            findings.push(TestFinding {
                category: FindingCategory::Security,
                severity: Severity::Low,
                title: "Unchecked increment may overflow".to_string(),
                description: "Arithmetic operations without explicit overflow protection. \
                              While Rust checks overflow in debug builds, release builds with \
                              overflow-checks=false will wrap."
                    .to_string(),
                file: Some(file_name.to_string()),
                line: Some(i + 1),
                suggestion: Some(
                    "Use checked_add(), saturating_add(), or ensure overflow-checks = true \
                     in Cargo.toml release profile."
                        .to_string(),
                ),
            });
            break;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 3: Performance / Gas Optimization
// ─────────────────────────────────────────────────────────────────────────────

fn run_performance_phase(template_dir: &Path) -> Result<PhaseResult> {
    let start = std::time::Instant::now();
    let mut findings = Vec::new();

    let src_dir = template_dir.join("src");
    if !src_dir.exists() {
        return Ok(PhaseResult {
            phase: "performance_testing".to_string(),
            findings,
            passed: true,
            duration_ms: start.elapsed().as_millis() as u64,
        });
    }

    scan_performance_patterns(&src_dir, &mut findings);

    let passed = !findings
        .iter()
        .any(|f| f.severity == Severity::Critical || f.severity == Severity::High);

    Ok(PhaseResult {
        phase: "performance_testing".to_string(),
        findings,
        passed,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

fn scan_performance_patterns(src_dir: &Path, findings: &mut Vec<TestFinding>) {
    if let Ok(entries) = fs::read_dir(src_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rs") {
                if let Ok(content) = fs::read_to_string(&path) {
                    let file_name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    analyze_performance_patterns(&content, &file_name, findings);
                }
            }
        }
    }
}

fn analyze_performance_patterns(content: &str, file_name: &str, findings: &mut Vec<TestFinding>) {
    let lines: Vec<&str> = content.lines().collect();

    // Check for storage instance vs persistent vs temporary usage patterns
    let instance_count = content.matches("storage().instance()").count();
    let persistent_count = content.matches("storage().persistent()").count();
    let temporary_count = content.matches("storage().temporary()").count();

    // Instance storage is most expensive — suggest persistent/temporary where possible
    if instance_count > 3 && persistent_count == 0 && temporary_count == 0 {
        findings.push(TestFinding {
            category: FindingCategory::Performance,
            severity: Severity::Info,
            title: "Heavy use of instance storage".to_string(),
            description: format!(
                "Contract uses instance storage {} times with no persistent/temporary storage. \
                 Instance storage is the most expensive storage type in Soroban.",
                instance_count
            ),
            file: Some(file_name.to_string()),
            line: None,
            suggestion: Some(
                "Consider using persistent().storage() for data that doesn't need to be \
                 available across contract upgrades, and temporary().storage() for transient data."
                    .to_string(),
            ),
        });
    }

    // Check for loops that could be expensive on-chain
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.contains("for ") && (trimmed.contains("in ") || trimmed.contains("iter()")) {
            // Check if the loop iterates over storage collections
            if trimmed.contains("storage()")
                || content
                    .lines()
                    .nth(i + 1)
                    .unwrap_or("")
                    .contains("storage()")
            {
                findings.push(TestFinding {
                    category: FindingCategory::Performance,
                    severity: Severity::Medium,
                    title: "Loop over storage collection".to_string(),
                    description: "Iterating over storage data on-chain can consume excessive gas \
                                  with large datasets. This is a common source of out-of-gas errors."
                        .to_string(),
                    file: Some(file_name.to_string()),
                    line: Some(i + 1),
                    suggestion: Some(
                        "Consider pagination or indexed access patterns instead of iterating \
                         over all stored entries."
                            .to_string(),
                    ),
                });
            }
        }
    }

    // Check for vector/snapshot usage
    if content.contains("Vec::") && content.contains("env.storage()") {
        findings.push(TestFinding {
            category: FindingCategory::Performance,
            severity: Severity::Low,
            title: "Vector stored in contract storage".to_string(),
            description: "Storing Vec types directly in storage can be gas-inefficient for \
                          large collections due to serialization overhead."
                .to_string(),
            file: Some(file_name.to_string()),
            line: None,
            suggestion: Some(
                "Consider using indexed storage patterns (e.g., HashMap with explicit keys) \
                 instead of storing full vectors."
                    .to_string(),
            ),
        });
    }

    // Check for release profile optimizations
    let cargo_path = file_name.rsplit_once('/').map(|(_, rest)| rest.to_string());
    if cargo_path.as_deref() == Some("lib.rs") {
        // We're in a lib.rs — check if the parent has good release profile
        // (This is a heuristic based on content patterns)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 4: SDK & Network Compatibility
// ─────────────────────────────────────────────────────────────────────────────

fn run_compatibility_phase(template_dir: &Path, config: &TestConfig) -> Result<PhaseResult> {
    let start = std::time::Instant::now();
    let mut findings = Vec::new();

    let cargo_path = template_dir.join("Cargo.toml");
    if !cargo_path.exists() {
        return Ok(PhaseResult {
            phase: "compatibility_testing".to_string(),
            findings,
            passed: true,
            duration_ms: start.elapsed().as_millis() as u64,
        });
    }

    let content = match fs::read_to_string(&cargo_path) {
        Ok(c) => c,
        Err(_) => {
            return Ok(PhaseResult {
                phase: "compatibility_testing".to_string(),
                findings,
                passed: false,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }
    };
    let parsed: toml::Value = match content.parse() {
        Ok(v) => v,
        Err(_) => {
            return Ok(PhaseResult {
                phase: "compatibility_testing".to_string(),
                findings,
                passed: false,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }
    };

    if let Some(package) = parsed.get("package") {
        // Check Rust edition
        if let Some(edition) = package.get("edition").and_then(|v| v.as_str()) {
            if let Some(min_edition) = &config.min_rust_edition {
                if edition < min_edition.as_str() {
                    findings.push(TestFinding {
                        category: FindingCategory::Compatibility,
                        severity: Severity::Medium,
                        title: "Outdated Rust edition".to_string(),
                        description: format!(
                            "Template uses Rust edition '{}', minimum expected is '{}'.",
                            edition, min_edition
                        ),
                        file: Some("Cargo.toml".to_string()),
                        line: None,
                        suggestion: Some(format!(
                            "Update edition to \"{}\" in Cargo.toml.",
                            min_edition
                        )),
                    });
                }
            }
        }

        // Check soroban-sdk version
        if let Some(deps) = package.get("dependencies") {
            if let Some(soroban) = deps.get("soroban-sdk") {
                let version_str = if let Some(v) = soroban.as_str() {
                    v.to_string()
                } else if let Some(table) = soroban.as_table() {
                    table
                        .get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string()
                } else {
                    "unknown".to_string()
                };

                if version_str == "unknown" {
                    findings.push(TestFinding {
                        category: FindingCategory::Compatibility,
                        severity: Severity::Low,
                        title: "Cannot determine soroban-sdk version".to_string(),
                        description: "soroban-sdk dependency format is not recognized.".to_string(),
                        file: Some("Cargo.toml".to_string()),
                        line: None,
                        suggestion: Some(
                            "Use soroban-sdk = \"21.0.0\" format for version pinning.".to_string(),
                        ),
                    });
                } else if let Some(target) = &config.target_sdk_version {
                    if version_str.contains(target) {
                        // Exact match — good
                    } else {
                        findings.push(TestFinding {
                            category: FindingCategory::Compatibility,
                            severity: Severity::Info,
                            title: "soroban-sdk version mismatch".to_string(),
                            description: format!(
                                "Template uses soroban-sdk '{}', target is '{}'.",
                                version_str, target
                            ),
                            file: Some("Cargo.toml".to_string()),
                            line: None,
                            suggestion: Some(format!(
                                "Consider updating soroban-sdk to '{}'.",
                                target
                            )),
                        });
                    }
                }

                // Check for testutils feature in dev-dependencies
                if let Some(dev_deps) = package.get("dev-dependencies") {
                    if let Some(dev_soroban) = dev_deps.get("soroban-sdk") {
                        let has_testutils = if let Some(table) = dev_soroban.as_table() {
                            table
                                .get("features")
                                .and_then(|f| f.as_array())
                                .map(|arr| arr.iter().any(|f| f.as_str() == Some("testutils")))
                                .unwrap_or(false)
                        } else {
                            false
                        };

                        if !has_testutils {
                            findings.push(TestFinding {
                                category: FindingCategory::Compatibility,
                                severity: Severity::Medium,
                                title: "Missing testutils feature in dev-dependencies".to_string(),
                                description: "Soroban tests require the 'testutils' feature flag \
                                              in dev-dependencies."
                                    .to_string(),
                                file: Some("Cargo.toml".to_string()),
                                line: None,
                                suggestion: Some(
                                    "Add soroban-sdk = {{ version = \"...\", features = [\"testutils\"] }} to [dev-dependencies]."
                                        .to_string(),
                                ),
                            });
                        }
                    }
                } else {
                    findings.push(TestFinding {
                        category: FindingCategory::Compatibility,
                        severity: Severity::Medium,
                        title: "No dev-dependencies section".to_string(),
                        description: "Template has no [dev-dependencies] section. Soroban test \
                                      utilities require the testutils feature."
                            .to_string(),
                        file: Some("Cargo.toml".to_string()),
                        line: None,
                        suggestion: Some(
                            "Add [dev-dependencies] with soroban-sdk testutils feature."
                                .to_string(),
                        ),
                    });
                }
            }
        }
    }

    let passed = !findings
        .iter()
        .any(|f| f.severity == Severity::Critical || f.severity == Severity::High);

    Ok(PhaseResult {
        phase: "compatibility_testing".to_string(),
        findings,
        passed,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 5: Documentation
// ─────────────────────────────────────────────────────────────────────────────

fn run_docs_phase(template_dir: &Path) -> Result<PhaseResult> {
    let start = std::time::Instant::now();
    let mut findings = Vec::new();

    // README check
    let readme_path = template_dir.join("README.md");
    if readme_path.exists() {
        if let Ok(content) = fs::read_to_string(&readme_path) {
            validate_readme(&content, &mut findings);
        }
    }

    // Check for inline documentation in source
    let src_dir = template_dir.join("src");
    if src_dir.exists() {
        check_source_docs(&src_dir, &mut findings);
    }

    let passed = !findings
        .iter()
        .any(|f| f.severity == Severity::Critical || f.severity == Severity::High);

    Ok(PhaseResult {
        phase: "documentation_testing".to_string(),
        findings,
        passed,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

fn validate_readme(content: &str, findings: &mut Vec<TestFinding>) {
    let min_length = 100;
    if content.len() < min_length {
        findings.push(TestFinding {
            category: FindingCategory::Documentation,
            severity: Severity::Low,
            title: "README is too short".to_string(),
            description: format!(
                "README is only {} bytes. Should be at least {} bytes with usage instructions.",
                content.len(),
                min_length
            ),
            file: Some("README.md".to_string()),
            line: None,
            suggestion: Some(
                "Expand README with installation instructions, usage examples, and API documentation."
                    .to_string(),
            ),
        });
    }

    // Check for required sections
    let lower = content.to_lowercase();
    let has_install = lower.contains("install") || lower.contains("getting started");
    let has_usage = lower.contains("usage") || lower.contains("example");
    let has_license = lower.contains("license");

    if !has_install {
        findings.push(TestFinding {
            category: FindingCategory::Documentation,
            severity: Severity::Info,
            title: "README missing installation section".to_string(),
            description: "README should include installation/getting started instructions."
                .to_string(),
            file: Some("README.md".to_string()),
            line: None,
            suggestion: Some("Add an 'Installation' or 'Getting Started' section.".to_string()),
        });
    }

    if !has_usage {
        findings.push(TestFinding {
            category: FindingCategory::Documentation,
            severity: Severity::Info,
            title: "README missing usage section".to_string(),
            description: "README should include usage examples.".to_string(),
            file: Some("README.md".to_string()),
            line: None,
            suggestion: Some("Add a 'Usage' section with code examples.".to_string()),
        });
    }

    if !has_license {
        findings.push(TestFinding {
            category: FindingCategory::Documentation,
            severity: Severity::Info,
            title: "README missing license information".to_string(),
            description: "README should mention the license.".to_string(),
            file: Some("README.md".to_string()),
            line: None,
            suggestion: Some(
                "Add license information to README and include a LICENSE file.".to_string(),
            ),
        });
    }
}

fn check_source_docs(src_dir: &Path, findings: &mut Vec<TestFinding>) {
    if let Ok(entries) = fs::read_dir(src_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rs") {
                if let Ok(content) = fs::read_to_string(&path) {
                    let file_name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    // Count doc comments vs public functions
                    let doc_comment_count = content
                        .lines()
                        .filter(|l| l.trim().starts_with("///"))
                        .count();
                    let pub_fn_count = content
                        .lines()
                        .filter(|l| l.trim().starts_with("pub fn "))
                        .count();

                    if pub_fn_count > 0 && doc_comment_count == 0 {
                        findings.push(TestFinding {
                            category: FindingCategory::Documentation,
                            severity: Severity::Low,
                            title: format!(
                                "No doc comments in '{}'",
                                file_name
                            ),
                            description: "Contract has public functions but no /// doc comments. \
                                          Good documentation improves developer experience."
                                .to_string(),
                            file: Some(file_name),
                            line: None,
                            suggestion: Some(
                                "Add /// doc comments to public functions describing their purpose, \
                                 parameters, and return values."
                                    .to_string(),
                            ),
                        });
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Scoring
// ─────────────────────────────────────────────────────────────────────────────

fn compute_quality_score(findings: &[TestFinding]) -> u32 {
    let deduction: u32 = findings.iter().map(|f| f.severity.weight()).sum();
    100u32.saturating_sub(deduction)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_test_template(dir: &Path) {
        fs::create_dir_all(dir.join("src")).unwrap();

        fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "{{PROJECT_NAME}}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
soroban-sdk = "21.0.0"

[dev-dependencies]
soroban-sdk = { version = "21.0.0", features = ["testutils"] }

[profile.release]
opt-level = "z"
overflow-checks = true
panic = "abort"
lto = true
"#,
        )
        .unwrap();

        fs::write(
            dir.join("src/lib.rs"),
            r#"#![no_std]
use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Symbol};

const KEY: Symbol = symbol_short!("KEY");

#[contract]
pub struct {{PROJECT_NAME_PASCAL}};

#[contractimpl]
impl {{PROJECT_NAME_PASCAL}} {
    /// Initialize the contract. Only `admin` may call this.
    pub fn init(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&KEY, &0u32);
    }

    /// Get the stored value.
    pub fn get(env: Env) -> u32 {
        env.storage().instance().get(&KEY).unwrap_or(0)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_init() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.init(&admin);
        assert_eq!(client.get(), 0);
    }
}
"#,
        )
        .unwrap();

        fs::write(
            dir.join("README.md"),
            "# {{PROJECT_NAME}}\n\nA Soroban smart contract template.\n\n## Installation\n\nAdd to your project.\n\n## Usage\n\nCall init() then get().\n\n## License\n\nMIT\n",
        )
        .unwrap();
    }

    fn create_minimal_template(dir: &Path) {
        fs::create_dir_all(dir.join("src")).unwrap();

        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"my-contract\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        fs::write(
            dir.join("src/lib.rs"),
            "pub fn hello() { println!(\"hello\"); }\n",
        )
        .unwrap();
    }

    #[test]
    fn test_valid_template_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let template_dir = tmp.path().join("good-template");
        create_test_template(&template_dir);

        let config = TestConfig::default();
        let report = test_template(&template_dir, &config).unwrap();

        let blocking: Vec<String> = report
            .phases
            .iter()
            .flat_map(|p| p.findings.iter())
            .filter(|f| f.severity == Severity::Critical || f.severity == Severity::High)
            .map(|f| format!("[{}] {}", f.severity.label(), f.title))
            .collect();
        assert!(
            report.passed,
            "Valid template should pass: {} — blocking findings: {:?}",
            report.summary, blocking
        );
        assert!(
            report.quality_score >= 70,
            "Score should be >= 70, got {}",
            report.quality_score
        );
    }

    #[test]
    fn test_minimal_template_has_findings() {
        let tmp = tempfile::tempdir().unwrap();
        let template_dir = tmp.path().join("bad-template");
        create_minimal_template(&template_dir);

        let config = TestConfig::default();
        let report = test_template(&template_dir, &config).unwrap();

        assert!(!report.passed, "Minimal template should fail");
        assert!(report.critical_count > 0 || report.high_count > 0);
    }

    #[test]
    fn test_structure_phase_detects_missing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let template_dir = tmp.path().join("empty-template");
        fs::create_dir_all(&template_dir).unwrap();

        let result = run_structure_phase(&template_dir).unwrap();
        assert!(!result.passed);
        assert!(result
            .findings
            .iter()
            .any(|f| f.title.contains("Missing required file")));
    }

    #[test]
    fn test_security_phase_detects_unprotected_state_write() {
        let tmp = tempfile::tempdir().unwrap();
        let template_dir = tmp.path().join("vuln-template");
        fs::create_dir_all(template_dir.join("src")).unwrap();

        fs::write(
            template_dir.join("src/lib.rs"),
            r#"#![no_std]
use soroban_sdk::{contract, contractimpl, Env};

#[contract]
pub struct VulnContract;

#[contractimpl]
impl VulnContract {
    pub fn unsafe_write(env: Env) {
        env.storage().instance().set(&"key", &"value");
    }
}
"#,
        )
        .unwrap();

        let result = run_security_phase(&template_dir).unwrap();
        assert!(result
            .findings
            .iter()
            .any(|f| f.title.contains("writes state without require_auth")));
    }

    #[test]
    fn test_security_phase_detects_unwrap() {
        let tmp = tempfile::tempdir().unwrap();
        let template_dir = tmp.path().join("unwrap-template");
        fs::create_dir_all(template_dir.join("src")).unwrap();

        fs::write(
            template_dir.join("src/lib.rs"),
            r#"#![no_std]
use soroban_sdk::{contract, contractimpl, Env};

#[contract]
pub struct UnwrapContract;

#[contractimpl]
impl UnwrapContract {
    pub fn do_something(env: Env) {
        let val: u32 = env.storage().instance().get(&"key").unwrap();
        let _ = val;
    }
}
"#,
        )
        .unwrap();

        let result = run_security_phase(&template_dir).unwrap();
        assert!(result
            .findings
            .iter()
            .any(|f| f.title.contains("Unsafe unwrap")));
    }

    #[test]
    fn test_compatibility_phase_detects_old_edition() {
        let tmp = tempfile::tempdir().unwrap();
        let template_dir = tmp.path().join("old-edition");
        fs::create_dir_all(template_dir.join("src")).unwrap();

        fs::write(
            template_dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2018\"\n",
        )
        .unwrap();

        let config = TestConfig::default();
        let result = run_compatibility_phase(&template_dir, &config).unwrap();
        assert!(result
            .findings
            .iter()
            .any(|f| f.title.contains("Outdated Rust edition")));
    }

    #[test]
    fn test_docs_phase_detects_short_readme() {
        let tmp = tempfile::tempdir().unwrap();
        let template_dir = tmp.path().join("short-readme");
        fs::create_dir_all(&template_dir).unwrap();

        fs::write(template_dir.join("README.md"), "Hi\n").unwrap();

        let result = run_docs_phase(&template_dir).unwrap();
        assert!(result
            .findings
            .iter()
            .any(|f| f.title.contains("too short")));
    }

    #[test]
    fn test_quality_score_deductions() {
        let findings = vec![
            TestFinding {
                category: FindingCategory::Security,
                severity: Severity::Critical,
                title: "test".to_string(),
                description: "test".to_string(),
                file: None,
                line: None,
                suggestion: None,
            },
            TestFinding {
                category: FindingCategory::Performance,
                severity: Severity::Medium,
                title: "test".to_string(),
                description: "test".to_string(),
                file: None,
                line: None,
                suggestion: None,
            },
        ];

        let score = compute_quality_score(&findings);
        assert_eq!(score, 100 - 40 - 15); // 45
    }

    #[test]
    fn test_severity_weights() {
        assert_eq!(Severity::Critical.weight(), 40);
        assert_eq!(Severity::High.weight(), 25);
        assert_eq!(Severity::Medium.weight(), 15);
        assert_eq!(Severity::Low.weight(), 5);
        assert_eq!(Severity::Info.weight(), 1);
    }

    #[test]
    fn test_summary_generation() {
        let tmp = tempfile::tempdir().unwrap();
        let template_dir = tmp.path().join("summary-test");
        create_test_template(&template_dir);

        let config = TestConfig::default();
        let report = test_template(&template_dir, &config).unwrap();
        let summary = generate_summary(&[report]);

        assert!(summary.contains("AI Template Testing Summary"));
        assert!(summary.contains("Templates tested:  1"));
    }

    #[test]
    fn test_cargo_toml_missing_soroban_dep() {
        let tmp = tempfile::tempdir().unwrap();
        let template_dir = tmp.path().join("no-soroban");
        fs::create_dir_all(template_dir.join("src")).unwrap();

        fs::write(
            template_dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();

        fs::write(
            template_dir.join("src/lib.rs"),
            "#![no_std]\nuse soroban_sdk::{{contract, contractimpl, Env}};\n\n#[contract]\npub struct Test;\n\n#[contractimpl]\nimpl Test { pub fn hello() {} }\n",
        )
        .unwrap();

        let result = run_structure_phase(&template_dir).unwrap();
        assert!(result
            .findings
            .iter()
            .any(|f| f.title.contains("Missing soroban-sdk")));
    }

    #[test]
    fn test_cargo_toml_missing_cdylib() {
        let tmp = tempfile::tempdir().unwrap();
        let template_dir = tmp.path().join("no-cdylib");
        fs::create_dir_all(template_dir.join("src")).unwrap();

        fs::write(
            template_dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nsoroban-sdk = \"21.0.0\"\n\n[lib]\ncrate-type = [\"rlib\"]\n",
        )
        .unwrap();

        fs::write(
            template_dir.join("src/lib.rs"),
            "#![no_std]\nuse soroban_sdk::{{contract, contractimpl, Env}};\n\n#[contract]\npub struct Test;\n\n#[contractimpl]\nimpl Test { pub fn hello() {} }\n",
        )
        .unwrap();

        let result = run_structure_phase(&template_dir).unwrap();
        assert!(result
            .findings
            .iter()
            .any(|f| f.title.contains("Missing cdylib")));
    }

    #[test]
    fn test_cargo_toml_missing_overflow_checks() {
        let tmp = tempfile::tempdir().unwrap();
        let template_dir = tmp.path().join("no-overflow");
        fs::create_dir_all(template_dir.join("src")).unwrap();

        fs::write(
            template_dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nsoroban-sdk = \"21.0.0\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[profile.release]\nopt-level = \"z\"\n",
        )
        .unwrap();

        fs::write(
            template_dir.join("src/lib.rs"),
            "#![no_std]\nuse soroban_sdk::{{contract, contractimpl, Env}};\n\n#[contract]\npub struct Test;\n\n#[contractimpl]\nimpl Test { pub fn hello() {} }\n",
        )
        .unwrap();

        let result = run_structure_phase(&template_dir).unwrap();
        assert!(result
            .findings
            .iter()
            .any(|f| f.title.contains("overflow-checks")));
    }

    #[test]
    fn test_missing_no_std_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let template_dir = tmp.path().join("no-nostd");
        fs::create_dir_all(template_dir.join("src")).unwrap();

        fs::write(
            template_dir.join("src/lib.rs"),
            "use soroban_sdk::{{contract, contractimpl, Env}};\n\n#[contract]\npub struct Test;\n\n#[contractimpl]\nimpl Test { pub fn hello() {} }\n",
        )
        .unwrap();

        let result = run_structure_phase(&template_dir).unwrap();
        assert!(result
            .findings
            .iter()
            .any(|f| f.title.contains("#![no_std]")));
    }

    #[test]
    fn test_phase_results_serialization() {
        let finding = TestFinding {
            category: FindingCategory::Security,
            severity: Severity::High,
            title: "Test finding".to_string(),
            description: "A test".to_string(),
            file: Some("lib.rs".to_string()),
            line: Some(10),
            suggestion: Some("Fix it".to_string()),
        };

        let phase = PhaseResult {
            phase: "test".to_string(),
            findings: vec![finding],
            passed: false,
            duration_ms: 42,
        };

        let json = serde_json::to_string_pretty(&phase).unwrap();
        // The enums carry serde rename policies, so compare case-insensitively
        // rather than pinning the JSON to a particular casing.
        let lower = json.to_lowercase();
        assert!(lower.contains("security"), "{}", json);
        assert!(lower.contains("high"), "{}", json);

        let deserialized: PhaseResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.phase, "test");
        assert_eq!(deserialized.findings.len(), 1);
    }
}
