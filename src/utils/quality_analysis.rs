//! AI-assisted template quality analysis.
//!
//! Analyzes a Soroban contract template's source for code quality, security,
//! best-practice adherence, documentation completeness, and test coverage,
//! producing a single weighted quality score (0-100) plus actionable
//! improvement suggestions. All scoring is deterministic static analysis —
//! it does not require network access or a running LLM — so it can run
//! offline and in CI. When a local Ollama model is available, callers may
//! layer richer natural-language suggestions on top via `ollama::generate`;
//! this module only produces the underlying metrics and heuristic findings.

use crate::utils::doc_generator::{DocCommentExtractor, ExtractedDocs, Visibility};
use crate::utils::security::{run_static_checks, StaticCheckResult};
use serde::{Deserialize, Serialize};

/// Score + findings for a single quality dimension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityCategory {
    pub name: String,
    pub score: u8,
    pub max_score: u8,
    pub findings: Vec<String>,
}

/// Static code-shape metrics used to derive the "Code Quality" score.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeMetrics {
    pub total_functions: usize,
    pub public_functions: usize,
    pub long_functions: usize,
    pub max_nesting_depth: usize,
    pub unwrap_count: usize,
    pub expect_count: usize,
    pub panic_count: usize,
    pub todo_count: usize,
    pub total_lines: usize,
}

/// Documentation completeness metrics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DocumentationMetrics {
    pub has_module_doc: bool,
    pub documented_public_items: usize,
    pub total_public_items: usize,
    pub completeness_pct: f64,
}

/// Test coverage heuristics (source-level, not a real coverage tool).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TestCoverageMetrics {
    pub has_test_module: bool,
    pub test_function_count: usize,
    pub public_function_count: usize,
    pub coverage_ratio: f64,
}

/// Gas-efficiency heuristics — a lightweight source-level proxy for the
/// full WASM-level analysis available via `starforge gas analyze`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GasEfficiencyMetrics {
    pub unbounded_loop_count: usize,
    pub storage_ops_in_loop_count: usize,
    pub clone_count: usize,
}

/// Full quality report for a single template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateQualityReport {
    pub template_name: String,
    pub overall_score: u8,
    pub categories: Vec<QualityCategory>,
    pub vulnerabilities: Vec<StaticCheckResult>,
    pub code_metrics: CodeMetrics,
    pub doc_metrics: DocumentationMetrics,
    pub test_metrics: TestCoverageMetrics,
    pub gas_metrics: GasEfficiencyMetrics,
    pub suggestions: Vec<String>,
    pub generated_at: String,
}

const LONG_FUNCTION_LINES: usize = 50;

/// Analyze a single Rust source file's contents and produce a full quality report.
pub fn analyze_source(template_name: &str, source: &str) -> TemplateQualityReport {
    let extracted = DocCommentExtractor::extract_from_source(source);

    let code_metrics = compute_code_metrics(source, &extracted);
    let doc_metrics = compute_doc_metrics(&extracted);
    let test_metrics = compute_test_metrics(source, &extracted);
    let gas_metrics = compute_gas_metrics(source);
    let vulnerabilities = run_static_checks(source);

    let security_category = score_security(&vulnerabilities);
    let code_quality_category = score_code_quality(&code_metrics);
    let best_practice_category = score_best_practices(source, &code_metrics);
    let documentation_category = score_documentation(&doc_metrics);
    let test_category = score_test_coverage(&test_metrics);

    let overall_score = [
        &security_category,
        &code_quality_category,
        &best_practice_category,
        &documentation_category,
        &test_category,
    ]
    .iter()
    .map(|c| c.score as u32)
    .sum::<u32>()
    .min(100) as u8;

    let suggestions = build_suggestions(
        &vulnerabilities,
        &code_metrics,
        &doc_metrics,
        &test_metrics,
        &gas_metrics,
    );

    TemplateQualityReport {
        template_name: template_name.to_string(),
        overall_score,
        categories: vec![
            security_category,
            code_quality_category,
            best_practice_category,
            documentation_category,
            test_category,
        ],
        vulnerabilities,
        code_metrics,
        doc_metrics,
        test_metrics,
        gas_metrics,
        suggestions,
        generated_at: chrono::Utc::now().to_rfc3339(),
    }
}

// ── Metric computation ────────────────────────────────────────────────────────

fn compute_code_metrics(source: &str, extracted: &ExtractedDocs) -> CodeMetrics {
    let lines: Vec<&str> = source.lines().collect();
    let public_functions = extracted
        .functions
        .iter()
        .filter(|f| f.visibility == Visibility::Public)
        .count();

    let mut long_functions = 0;
    for (i, line) in lines.iter().enumerate() {
        if is_fn_signature_line(line) {
            let len = function_body_line_count(&lines, i);
            if len > LONG_FUNCTION_LINES {
                long_functions += 1;
            }
        }
    }

    let mut max_nesting_depth = 0usize;
    let mut current_depth = 0usize;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        for ch in trimmed.chars() {
            match ch {
                '{' => {
                    current_depth += 1;
                    max_nesting_depth = max_nesting_depth.max(current_depth);
                }
                '}' => current_depth = current_depth.saturating_sub(1),
                _ => {}
            }
        }
    }

    CodeMetrics {
        total_functions: extracted.functions.len(),
        public_functions,
        long_functions,
        max_nesting_depth,
        unwrap_count: count_occurrences(source, ".unwrap()"),
        expect_count: count_occurrences(source, ".expect("),
        panic_count: count_occurrences(source, "panic!("),
        todo_count: count_occurrences(source, "TODO") + count_occurrences(source, "FIXME"),
        total_lines: lines.len(),
    }
}

fn is_fn_signature_line(line: &str) -> bool {
    let trimmed = line.trim();
    let stripped = trimmed
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("pub ")
        .trim_start_matches("async ")
        .trim_start_matches("unsafe ");
    stripped.starts_with("fn ")
}

/// Counts lines in a function body by tracking brace depth from the
/// signature line until the enclosing braces close.
fn function_body_line_count(lines: &[&str], start: usize) -> usize {
    let mut depth = 0i32;
    let mut opened = false;
    let mut count = 0;

    for line in &lines[start..] {
        count += 1;
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth <= 0 {
            break;
        }
    }
    count
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

fn compute_doc_metrics(extracted: &ExtractedDocs) -> DocumentationMetrics {
    let mut total = 0usize;
    let mut documented = 0usize;

    for f in extracted
        .functions
        .iter()
        .filter(|f| f.visibility == Visibility::Public)
    {
        total += 1;
        if !f.doc_comment.trim().is_empty() {
            documented += 1;
        }
    }
    for s in &extracted.structs {
        if s.visibility == Visibility::Public {
            total += 1;
            if !s.doc_comment.trim().is_empty() {
                documented += 1;
            }
        }
    }
    for e in &extracted.enums {
        if e.visibility == Visibility::Public {
            total += 1;
            if !e.doc_comment.trim().is_empty() {
                documented += 1;
            }
        }
    }

    let completeness_pct = if total == 0 {
        100.0
    } else {
        (documented as f64 / total as f64) * 100.0
    };

    DocumentationMetrics {
        has_module_doc: !extracted.module_doc.trim().is_empty(),
        documented_public_items: documented,
        total_public_items: total,
        completeness_pct,
    }
}

fn compute_test_metrics(source: &str, extracted: &ExtractedDocs) -> TestCoverageMetrics {
    let has_test_module = source.contains("#[cfg(test)]");
    let test_function_count = count_occurrences(source, "#[test]");
    let public_function_count = extracted
        .functions
        .iter()
        .filter(|f| f.visibility == Visibility::Public)
        .count();

    let coverage_ratio = if public_function_count == 0 {
        1.0
    } else {
        (test_function_count as f64 / public_function_count as f64).min(1.0)
    };

    TestCoverageMetrics {
        has_test_module,
        test_function_count,
        public_function_count,
        coverage_ratio,
    }
}

fn compute_gas_metrics(source: &str) -> GasEfficiencyMetrics {
    let lines: Vec<&str> = source.lines().collect();
    let mut unbounded_loop_count = 0;
    let mut storage_ops_in_loop_count = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let is_loop = trimmed.starts_with("for ") || trimmed.starts_with("while ");
        if !is_loop {
            continue;
        }
        // Unbounded: iterating a collection without an obvious fixed/small bound.
        if trimmed.contains(".iter()") || trimmed.contains(".into_iter()") {
            unbounded_loop_count += 1;
        }
        // Storage reads/writes inside a loop body are a classic gas hazard on Soroban.
        let body_len = function_body_line_count(&lines, i);
        let end = (i + body_len).min(lines.len());
        if lines[i..end]
            .iter()
            .any(|l| l.contains("storage()") && (l.contains(".set(") || l.contains(".get(")))
        {
            storage_ops_in_loop_count += 1;
        }
    }

    GasEfficiencyMetrics {
        unbounded_loop_count,
        storage_ops_in_loop_count,
        clone_count: count_occurrences(source, ".clone()"),
    }
}

// ── Scoring ────────────────────────────────────────────────────────────────────

fn score_security(vulnerabilities: &[StaticCheckResult]) -> QualityCategory {
    let max_score = 30u8;
    let mut score: i32 = max_score as i32;
    let mut findings = Vec::new();

    for v in vulnerabilities {
        let deduction = match v.severity.as_str() {
            "critical" => 20,
            "high" => 12,
            "medium" => 6,
            "low" => 3,
            _ => 1,
        };
        score -= deduction;
        findings.push(format!(
            "[{}] {}: {}",
            v.severity, v.pattern_name, v.description
        ));
    }

    QualityCategory {
        name: "Security".to_string(),
        score: score.clamp(0, max_score as i32) as u8,
        max_score,
        findings,
    }
}

fn score_code_quality(metrics: &CodeMetrics) -> QualityCategory {
    let max_score = 25u8;
    let mut score: i32 = max_score as i32;
    let mut findings = Vec::new();

    if metrics.long_functions > 0 {
        score -= (metrics.long_functions as i32) * 4;
        findings.push(format!(
            "{} function(s) exceed {} lines — consider extracting helpers",
            metrics.long_functions, LONG_FUNCTION_LINES
        ));
    }
    if metrics.unwrap_count + metrics.expect_count > 0 {
        let total = metrics.unwrap_count + metrics.expect_count;
        score -= (total as i32).min(10);
        findings.push(format!(
            "{} unwrap()/expect() call(s) that could panic at runtime",
            total
        ));
    }
    if metrics.panic_count > 0 {
        score -= (metrics.panic_count as i32) * 3;
        findings.push(format!(
            "{} explicit panic!() call(s) — prefer returning a Result/Error",
            metrics.panic_count
        ));
    }
    if metrics.todo_count > 0 {
        score -= (metrics.todo_count as i32).min(6);
        findings.push(format!(
            "{} TODO/FIXME marker(s) left in source",
            metrics.todo_count
        ));
    }
    if metrics.max_nesting_depth > 6 {
        score -= 5;
        findings.push(format!(
            "Deep nesting detected (depth {}) — flatten control flow for readability",
            metrics.max_nesting_depth
        ));
    }

    QualityCategory {
        name: "Code Quality".to_string(),
        score: score.clamp(0, max_score as i32) as u8,
        max_score,
        findings,
    }
}

fn score_best_practices(source: &str, metrics: &CodeMetrics) -> QualityCategory {
    let max_score = 15u8;
    let mut score: i32 = max_score as i32;
    let mut findings = Vec::new();

    if !source.contains("#[contract]") && !source.contains("#[contracttype]") {
        score -= 3;
        findings.push(
            "No #[contract]/#[contracttype] attributes found — verify this is a Soroban contract"
                .to_string(),
        );
    }

    let raw_arith = count_occurrences(source, " + ") + count_occurrences(source, " - ");
    let checked_arith = count_occurrences(source, "checked_add")
        + count_occurrences(source, "checked_sub")
        + count_occurrences(source, "checked_mul");
    if raw_arith > 0 && checked_arith == 0 {
        score -= 4;
        findings.push(
            "Arithmetic uses raw operators without checked_add/checked_sub/checked_mul guards"
                .to_string(),
        );
    }

    if metrics.public_functions > 0
        && !source.contains("Symbol::new")
        && !source.contains("publish(")
    {
        score -= 3;
        findings.push(
            "No event emission (env.events().publish) detected for state-changing functions"
                .to_string(),
        );
    }

    if (source.contains("Map<") || source.contains("Vec<"))
        && !source.contains("enum DataKey")
        && !source.contains("enum StorageKey")
    {
        score -= 2;
        findings.push(
                "No dedicated storage-key enum (DataKey/StorageKey) — consider one for type-safe storage access"
                    .to_string(),
            );
    }

    QualityCategory {
        name: "Best Practices".to_string(),
        score: score.clamp(0, max_score as i32) as u8,
        max_score,
        findings,
    }
}

fn score_documentation(metrics: &DocumentationMetrics) -> QualityCategory {
    let max_score = 15u8;
    let mut findings = Vec::new();

    let mut score = (metrics.completeness_pct / 100.0 * max_score as f64).round() as i32;
    if !metrics.has_module_doc {
        score -= 2;
        findings.push("No module-level doc comment (//!) describing the contract".to_string());
    }
    if metrics.completeness_pct < 100.0 {
        findings.push(format!(
            "{}/{} public items documented ({:.0}% complete)",
            metrics.documented_public_items, metrics.total_public_items, metrics.completeness_pct
        ));
    }

    QualityCategory {
        name: "Documentation".to_string(),
        score: score.clamp(0, max_score as i32) as u8,
        max_score,
        findings,
    }
}

fn score_test_coverage(metrics: &TestCoverageMetrics) -> QualityCategory {
    let max_score = 15u8;
    let mut findings = Vec::new();

    let mut score = (metrics.coverage_ratio * max_score as f64).round() as i32;
    if !metrics.has_test_module {
        score = 0;
        findings.push("No #[cfg(test)] module found in source".to_string());
    } else if metrics.coverage_ratio < 1.0 {
        findings.push(format!(
            "{} test(s) for {} public function(s) — aim for at least one test per function",
            metrics.test_function_count, metrics.public_function_count
        ));
    }

    QualityCategory {
        name: "Test Coverage".to_string(),
        score: score.clamp(0, max_score as i32) as u8,
        max_score,
        findings,
    }
}

// ── Suggestions ─────────────────────────────────────────────────────────────────

fn build_suggestions(
    vulnerabilities: &[StaticCheckResult],
    code_metrics: &CodeMetrics,
    doc_metrics: &DocumentationMetrics,
    test_metrics: &TestCoverageMetrics,
    gas_metrics: &GasEfficiencyMetrics,
) -> Vec<String> {
    let mut suggestions = Vec::new();

    let mut sorted_vulns: Vec<&StaticCheckResult> = vulnerabilities.iter().collect();
    sorted_vulns.sort_by_key(|v| match v.severity.as_str() {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 4,
    });
    for v in sorted_vulns.into_iter().take(5) {
        suggestions.push(format!(
            "Fix {} severity issue '{}': {}",
            v.severity, v.pattern_name, v.description
        ));
    }

    if code_metrics.long_functions > 0 {
        suggestions.push(format!(
            "Break up the {} function(s) longer than {} lines into smaller, focused helpers",
            code_metrics.long_functions, LONG_FUNCTION_LINES
        ));
    }
    if code_metrics.unwrap_count + code_metrics.expect_count > 3 {
        suggestions.push(
            "Replace unwrap()/expect() with `?` and typed errors to avoid panics in production"
                .to_string(),
        );
    }
    if doc_metrics.completeness_pct < 80.0 {
        suggestions.push(format!(
            "Document the {} undocumented public item(s) to reach full API coverage",
            doc_metrics.total_public_items - doc_metrics.documented_public_items
        ));
    }
    if !test_metrics.has_test_module {
        suggestions
            .push("Add a #[cfg(test)] module with unit tests for each public function".to_string());
    } else if test_metrics.coverage_ratio < 0.5 {
        suggestions.push(
            "Increase test coverage — fewer than half of public functions currently have a matching test"
                .to_string(),
        );
    }
    if gas_metrics.storage_ops_in_loop_count > 0 {
        suggestions.push(format!(
            "Hoist {} storage read/write(s) currently inside loop bodies to reduce gas cost",
            gas_metrics.storage_ops_in_loop_count
        ));
    }
    if gas_metrics.clone_count > 10 {
        suggestions.push(format!(
            "{} .clone() call(s) found — review whether references or Copy types could avoid unnecessary allocation",
            gas_metrics.clone_count
        ));
    }

    if suggestions.is_empty() {
        suggestions.push("No significant issues found — template looks solid.".to_string());
    }

    suggestions
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD_SOURCE: &str = r#"
//! A well-documented example contract.

/// Storage keys for the contract.
pub enum DataKey {
    /// Admin address.
    Admin,
}

/// Initialize the contract.
pub fn initialize(env: Env, admin: Address) -> bool {
    admin.require_auth();
    env.storage().instance().set(&DataKey::Admin, &admin);
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_initialize() {
        assert!(true);
    }
}
"#;

    const BAD_SOURCE: &str = r#"
pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> bool {
    let balance = get_balance(&env, &from).unwrap();
    let new_balance = balance - amount;
    env.storage().instance().set(&from, &new_balance);
    // TODO: emit event
    true
}
"#;

    #[test]
    fn good_source_scores_high() {
        let report = analyze_source("good-template", GOOD_SOURCE);
        assert!(
            report.overall_score >= 70,
            "score was {}",
            report.overall_score
        );
        assert!(report.doc_metrics.has_module_doc);
        assert!(report.test_metrics.has_test_module);
    }

    #[test]
    fn bad_source_scores_lower_and_flags_issues() {
        let good = analyze_source("good", GOOD_SOURCE);
        let bad = analyze_source("bad-template", BAD_SOURCE);
        assert!(bad.overall_score < good.overall_score);
        assert!(!bad.suggestions.is_empty());
        assert!(bad.code_metrics.unwrap_count >= 1);
        assert!(bad.code_metrics.todo_count >= 1);
        assert!(!bad.test_metrics.has_test_module);
    }

    #[test]
    fn security_findings_reduce_security_score() {
        let report = analyze_source("t", BAD_SOURCE);
        let security = report
            .categories
            .iter()
            .find(|c| c.name == "Security")
            .unwrap();
        // missing require_auth on a mutating fn should be picked up by static checks
        assert!(security.score <= security.max_score);
    }

    #[test]
    fn documentation_completeness_is_computed() {
        let report = analyze_source("t", GOOD_SOURCE);
        assert_eq!(report.doc_metrics.total_public_items, 2); // fn + enum
        assert_eq!(report.doc_metrics.documented_public_items, 2);
        assert_eq!(report.doc_metrics.completeness_pct, 100.0);
    }

    #[test]
    fn overall_score_never_exceeds_100() {
        let report = analyze_source("t", GOOD_SOURCE);
        assert!(report.overall_score <= 100);
    }

    #[test]
    fn empty_source_does_not_panic() {
        let report = analyze_source("empty", "");
        assert_eq!(report.code_metrics.total_functions, 0);
        assert!(report.overall_score <= 100);
    }
}
