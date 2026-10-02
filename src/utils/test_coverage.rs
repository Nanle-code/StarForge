use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoverageReport {
    pub functions_total: u32,
    pub functions_covered: u32,
    pub lines_total: u32,
    pub lines_covered: u32,
    pub branches_total: u32,
    pub branches_covered: u32,
    pub uncovered_functions: Vec<String>,
    pub coverage_percent: f64,
    pub function_coverage_percent: f64,
    pub line_coverage_percent: f64,
    pub branch_coverage_percent: f64,
    pub functions: Vec<FunctionCoverage>,
    pub branches: Vec<BranchCoverage>,
    pub goals: Option<CoverageGoalResult>,
    pub visualization: CoverageVisualization,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FunctionCoverage {
    pub name: String,
    pub signature: String,
    pub start_line: u32,
    pub end_line: u32,
    pub lines_total: u32,
    pub lines_covered: u32,
    pub branches_total: u32,
    pub branches_covered: u32,
    pub covered: bool,
    pub test_cases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchCoverage {
    pub id: String,
    pub function: String,
    pub line: u32,
    pub kind: BranchKind,
    pub condition: String,
    pub paths_total: u32,
    pub paths_covered: u32,
    pub covered: bool,
    pub test_cases: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchKind {
    If,
    ElseIf,
    Match,
    RequireAuth,
    ResultPropagation,
    AssertionGuard,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoverageVisualization {
    pub summary_bars: Vec<CoverageBar>,
    pub heatmap: Vec<CoverageHeatmapEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageBar {
    pub label: String,
    pub percent: f64,
    pub covered: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageHeatmapEntry {
    pub function: String,
    pub line_start: u32,
    pub line_end: u32,
    pub intensity: f64,
    pub covered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageTestExecution {
    pub test_name: String,
    pub function: String,
    #[serde(default = "default_passed")]
    pub passed: bool,
}

impl CoverageTestExecution {
    pub fn new(test_name: impl Into<String>, function: impl Into<String>) -> Self {
        Self {
            test_name: test_name.into(),
            function: function.into(),
            passed: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoverageGoals {
    pub min_overall: Option<f64>,
    pub min_functions: Option<f64>,
    pub min_lines: Option<f64>,
    pub min_branches: Option<f64>,
}

impl CoverageGoals {
    pub fn has_goals(&self) -> bool {
        self.min_overall.is_some()
            || self.min_functions.is_some()
            || self.min_lines.is_some()
            || self.min_branches.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageGoalResult {
    pub passed: bool,
    pub actual_overall: f64,
    pub min_overall: Option<f64>,
    pub actual_functions: f64,
    pub min_functions: Option<f64>,
    pub actual_lines: f64,
    pub min_lines: Option<f64>,
    pub actual_branches: f64,
    pub min_branches: Option<f64>,
    pub violations: Vec<String>,
}

#[derive(Debug, Clone)]
struct SourceFunction {
    name: String,
    signature: String,
    start_line: usize,
    end_line: usize,
}

pub fn analyze_source_coverage(source: &str, executed_functions: &[String]) -> CoverageReport {
    let executions = executed_functions
        .iter()
        .map(|function| CoverageTestExecution::new(function.clone(), function.clone()))
        .collect::<Vec<_>>();
    analyze_source_coverage_with_executions(source, &executions)
}

pub fn analyze_source_coverage_with_executions(
    source: &str,
    executions: &[CoverageTestExecution],
) -> CoverageReport {
    let discovered = discover_functions(source);
    let mut tests_by_function: HashMap<String, Vec<String>> = HashMap::new();

    for execution in executions {
        tests_by_function
            .entry(execution.function.clone())
            .or_default()
            .push(execution.test_name.clone());
    }

    let mut branches = Vec::new();
    for function in &discovered {
        branches.extend(discover_branches(source, function, &tests_by_function));
    }

    let mut functions = Vec::new();
    for function in discovered {
        let test_cases = tests_by_function
            .get(&function.name)
            .cloned()
            .unwrap_or_default();
        let covered = !test_cases.is_empty();
        let lines_total = count_function_lines(source, function.start_line, function.end_line);
        let function_branches = branches
            .iter()
            .filter(|branch| branch.function == function.name)
            .collect::<Vec<_>>();
        let branches_total = function_branches
            .iter()
            .map(|branch| branch.paths_total)
            .sum::<u32>();
        let branches_covered = function_branches
            .iter()
            .map(|branch| branch.paths_covered)
            .sum::<u32>();

        functions.push(FunctionCoverage {
            name: function.name,
            signature: function.signature,
            start_line: function.start_line as u32,
            end_line: function.end_line as u32,
            lines_total,
            lines_covered: if covered { lines_total } else { 0 },
            branches_total,
            branches_covered,
            covered,
            test_cases,
        });
    }

    let functions_total = functions.len() as u32;
    let functions_covered = functions.iter().filter(|function| function.covered).count() as u32;
    let lines_total = functions
        .iter()
        .map(|function| function.lines_total)
        .sum::<u32>();
    let lines_covered = functions
        .iter()
        .map(|function| function.lines_covered)
        .sum::<u32>();
    let branches_total = branches
        .iter()
        .map(|branch| branch.paths_total)
        .sum::<u32>();
    let branches_covered = branches
        .iter()
        .map(|branch| branch.paths_covered)
        .sum::<u32>();
    let uncovered_functions = functions
        .iter()
        .filter(|function| !function.covered)
        .map(|function| function.name.clone())
        .collect::<Vec<_>>();

    let function_coverage_percent = percent(functions_covered, functions_total);
    let line_coverage_percent = percent(lines_covered, lines_total);
    let branch_coverage_percent = percent(branches_covered, branches_total);
    let coverage_percent = weighted_percent(
        functions_covered + lines_covered + branches_covered,
        functions_total + lines_total + branches_total,
    );
    let visualization = build_visualization(
        &functions,
        function_coverage_percent,
        line_coverage_percent,
        branch_coverage_percent,
    );

    CoverageReport {
        functions_total,
        functions_covered,
        lines_total,
        lines_covered,
        branches_total,
        branches_covered,
        uncovered_functions,
        coverage_percent,
        function_coverage_percent,
        line_coverage_percent,
        branch_coverage_percent,
        functions,
        branches,
        goals: None,
        visualization,
        generated_at: chrono::Utc::now().to_rfc3339(),
    }
}

pub fn apply_coverage_goals(
    report: &mut CoverageReport,
    goals: CoverageGoals,
) -> CoverageGoalResult {
    let result = evaluate_coverage_goals(report, &goals);
    report.goals = Some(result.clone());
    result
}

pub fn evaluate_coverage_goals(
    report: &CoverageReport,
    goals: &CoverageGoals,
) -> CoverageGoalResult {
    let mut violations = Vec::new();

    check_goal(
        "overall coverage",
        report.coverage_percent,
        goals.min_overall,
        &mut violations,
    );
    check_goal(
        "function coverage",
        report.function_coverage_percent,
        goals.min_functions,
        &mut violations,
    );
    check_goal(
        "line coverage",
        report.line_coverage_percent,
        goals.min_lines,
        &mut violations,
    );
    check_goal(
        "branch coverage",
        report.branch_coverage_percent,
        goals.min_branches,
        &mut violations,
    );

    CoverageGoalResult {
        passed: violations.is_empty(),
        actual_overall: report.coverage_percent,
        min_overall: goals.min_overall,
        actual_functions: report.function_coverage_percent,
        min_functions: goals.min_functions,
        actual_lines: report.line_coverage_percent,
        min_lines: goals.min_lines,
        actual_branches: report.branch_coverage_percent,
        min_branches: goals.min_branches,
        violations,
    }
}

pub fn write_coverage_report(
    report: &CoverageReport,
    format: &str,
    output_path: &Path,
) -> Result<PathBuf> {
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
    }

    let rendered = render_coverage_report(report, format)?;
    fs::write(output_path, rendered)
        .with_context(|| format!("Failed to write {}", output_path.display()))?;
    Ok(output_path.to_path_buf())
}

pub fn render_coverage_report(report: &CoverageReport, format: &str) -> Result<String> {
    match format {
        "json" => Ok(serde_json::to_string_pretty(report)?),
        "html" => Ok(render_html_report(report)),
        "markdown" | "md" => Ok(render_markdown_report(report)),
        "text" | "txt" => Ok(render_text_report(report)),
        other => anyhow::bail!(
            "Unsupported coverage report format '{}'. Use html, json, markdown, or text.",
            other
        ),
    }
}

pub fn write_coverage_ci_workflow(
    output_path: &Path,
    wasm: &Path,
    source: &Path,
    goals: &CoverageGoals,
) -> Result<PathBuf> {
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
    }

    let mut command = format!(
        "starforge test --wasm {} --source {} --coverage --coverage-ci",
        shell_quote(&portable_path(wasm)),
        shell_quote(&portable_path(source))
    );

    if let Some(value) = goals.min_overall {
        command.push_str(&format!(" --coverage-goal {:.1}", value));
    }
    if let Some(value) = goals.min_functions {
        command.push_str(&format!(" --function-coverage-goal {:.1}", value));
    }
    if let Some(value) = goals.min_lines {
        command.push_str(&format!(" --line-coverage-goal {:.1}", value));
    }
    if let Some(value) = goals.min_branches {
        command.push_str(&format!(" --branch-coverage-goal {:.1}", value));
    }
    command.push_str(" --coverage-format html --coverage-out coverage/contract-coverage.html");

    let yaml = format!(
        r#"name: StarForge Contract Coverage

on:
  pull_request:
  push:
    branches: [ master, main ]

jobs:
  contract-coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Install StarForge
        run: cargo install --path .
            - name: Build Soroban WASM
                run: starforge contract build
      - name: Run Soroban contract coverage
        run: {}
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: contract-coverage
          path: coverage/contract-coverage.html
"#,
        command
    );

    fs::write(output_path, yaml)
        .with_context(|| format!("Failed to write {}", output_path.display()))?;
    Ok(output_path.to_path_buf())
}

fn discover_functions(source: &str) -> Vec<SourceFunction> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut functions = Vec::new();
    let mut line_index = 0usize;

    while line_index < lines.len() {
        let line = strip_line_comment(lines[line_index]);
        if let Some(name) = extract_function_name(line.trim()) {
            let (signature, end_line) = collect_signature_and_end(&lines, line_index);
            functions.push(SourceFunction {
                name,
                signature,
                start_line: line_index + 1,
                end_line,
            });
            line_index = end_line;
        } else {
            line_index += 1;
        }
    }

    functions
}

fn collect_signature_and_end(lines: &[&str], start_index: usize) -> (String, usize) {
    let mut signature_parts = Vec::new();
    let mut brace_depth = 0i32;
    let mut body_started = false;
    let mut end_line = start_index + 1;

    for (offset, line) in lines[start_index..].iter().enumerate() {
        let cleaned = strip_line_comment(line);
        let trimmed = cleaned.trim();
        if !trimmed.is_empty() {
            signature_parts.push(trimmed.to_string());
        }

        for ch in cleaned.chars() {
            match ch {
                '{' => {
                    body_started = true;
                    brace_depth += 1;
                }
                '}' => {
                    brace_depth -= 1;
                }
                _ => {}
            }
        }

        end_line = start_index + offset + 1;
        if body_started && brace_depth <= 0 {
            break;
        }
        if !body_started && offset > 0 && extract_function_name(trimmed).is_some() {
            end_line = start_index + offset;
            break;
        }
    }

    (signature_parts.join(" "), end_line)
}

fn discover_branches(
    source: &str,
    function: &SourceFunction,
    tests_by_function: &HashMap<String, Vec<String>>,
) -> Vec<BranchCoverage> {
    let lines = source.lines().collect::<Vec<_>>();
    let test_cases = tests_by_function
        .get(&function.name)
        .cloned()
        .unwrap_or_default();
    let mut branches = Vec::new();

    for line_number in function.start_line..=function.end_line {
        let Some(raw) = lines.get(line_number - 1) else {
            continue;
        };
        let cleaned = strip_line_comment(raw);
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            continue;
        }

        for (kind, condition) in branch_points(trimmed) {
            let paths_total = paths_for_branch(kind, &condition);
            let paths_covered = estimate_branch_paths(kind, paths_total, &test_cases);
            branches.push(BranchCoverage {
                id: format!("{}:{}:{:?}", function.name, line_number, kind),
                function: function.name.clone(),
                line: line_number as u32,
                kind,
                condition,
                paths_total,
                paths_covered,
                covered: paths_total > 0 && paths_covered >= paths_total,
                test_cases: test_cases.clone(),
            });
        }
    }

    branches
}

fn branch_points(line: &str) -> Vec<(BranchKind, String)> {
    let mut points = Vec::new();
    let normalized = line.trim();

    if normalized.starts_with("else if ") {
        points.push((
            BranchKind::ElseIf,
            condition_after_keyword(normalized, "else if"),
        ));
    } else if normalized.starts_with("if ") || normalized.contains(" if ") {
        points.push((BranchKind::If, condition_after_keyword(normalized, "if")));
    }

    if normalized.starts_with("match ") || normalized.contains(" match ") {
        points.push((
            BranchKind::Match,
            condition_after_keyword(normalized, "match"),
        ));
    }

    if normalized.contains("require_auth(") || normalized.contains(".require_auth()") {
        points.push((BranchKind::RequireAuth, "require_auth".to_string()));
    }

    if normalized.contains("assert!")
        || normalized.contains("assert_eq!")
        || normalized.contains("assert_ne!")
        || normalized.contains("panic!")
        || normalized.contains("ensure!")
    {
        points.push((BranchKind::AssertionGuard, normalized.to_string()));
    }

    if normalized.contains('?') {
        points.push((BranchKind::ResultPropagation, normalized.to_string()));
    }

    points
}

fn condition_after_keyword(line: &str, keyword: &str) -> String {
    let Some(index) = line.find(keyword) else {
        return line.to_string();
    };
    let rest = line[index + keyword.len()..].trim();
    rest.split('{')
        .next()
        .unwrap_or(rest)
        .trim()
        .trim_end_matches("=>")
        .trim()
        .to_string()
}

fn paths_for_branch(kind: BranchKind, condition: &str) -> u32 {
    match kind {
        BranchKind::Match => condition.matches("=>").count().max(2) as u32,
        _ => 2,
    }
}

fn estimate_branch_paths(kind: BranchKind, paths_total: u32, test_cases: &[String]) -> u32 {
    if test_cases.is_empty() || paths_total == 0 {
        return 0;
    }

    let mut signals = HashSet::new();
    for test_name in test_cases {
        let lower = test_name.to_ascii_lowercase();
        if has_negative_signal(kind, &lower) {
            signals.insert("negative");
        }
        if has_positive_signal(&lower) {
            signals.insert("positive");
        }
    }

    if signals.is_empty() {
        signals.insert("positive");
    }

    (signals.len() as u32).min(paths_total)
}

fn has_positive_signal(value: &str) -> bool {
    value.contains("happy")
        || value.contains("success")
        || value.contains("valid")
        || value.contains("authorized")
        || value.contains("owner")
        || value.contains("admin")
        || value.contains("positive")
        || value.contains("pass")
}

fn has_negative_signal(kind: BranchKind, value: &str) -> bool {
    value.contains("fail")
        || value.contains("error")
        || value.contains("invalid")
        || value.contains("unauthorized")
        || value.contains("forbidden")
        || value.contains("reject")
        || value.contains("revert")
        || value.contains("missing")
        || value.contains("none")
        || value.contains("zero")
        || value.contains("overflow")
        || matches!(kind, BranchKind::ResultPropagation) && value.contains("not_found")
}

fn extract_function_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let fn_index = trimmed.find("fn ")?;
    let prefix = trimmed[..fn_index].trim();

    let allowed_prefix = prefix.is_empty()
        || prefix == "pub"
        || prefix == "async"
        || prefix == "pub async"
        || prefix.starts_with("pub(")
        || prefix.ends_with(" pub")
        || prefix.ends_with(" async");

    if !allowed_prefix {
        return None;
    }

    let rest = &trimmed[fn_index + 3..];
    let name = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>();

    (!name.is_empty()).then_some(name)
}

fn count_function_lines(source: &str, start_line: usize, end_line: usize) -> u32 {
    source
        .lines()
        .enumerate()
        .filter(|(index, line)| {
            let line_number = index + 1;
            line_number >= start_line && line_number <= end_line && is_code_line(line)
        })
        .count() as u32
}

fn is_code_line(line: &str) -> bool {
    let trimmed = strip_line_comment(line).trim();
    !trimmed.is_empty()
        && !trimmed.starts_with("#[")
        && trimmed != "{"
        && trimmed != "}"
        && trimmed != "};"
}

fn strip_line_comment(line: &str) -> &str {
    line.split("//").next().unwrap_or(line)
}

fn build_visualization(
    functions: &[FunctionCoverage],
    function_percent: f64,
    line_percent: f64,
    branch_percent: f64,
) -> CoverageVisualization {
    let summary_bars = vec![
        CoverageBar {
            label: "Functions".to_string(),
            percent: function_percent,
            covered: functions.iter().filter(|function| function.covered).count() as u32,
            total: functions.len() as u32,
        },
        CoverageBar {
            label: "Lines".to_string(),
            percent: line_percent,
            covered: functions
                .iter()
                .map(|function| function.lines_covered)
                .sum::<u32>(),
            total: functions
                .iter()
                .map(|function| function.lines_total)
                .sum::<u32>(),
        },
        CoverageBar {
            label: "Branches".to_string(),
            percent: branch_percent,
            covered: functions
                .iter()
                .map(|function| function.branches_covered)
                .sum::<u32>(),
            total: functions
                .iter()
                .map(|function| function.branches_total)
                .sum::<u32>(),
        },
    ];

    let heatmap = functions
        .iter()
        .map(|function| CoverageHeatmapEntry {
            function: function.name.clone(),
            line_start: function.start_line,
            line_end: function.end_line,
            intensity: if function.lines_total == 0 {
                0.0
            } else {
                function.lines_covered as f64 / function.lines_total as f64
            },
            covered: function.covered,
        })
        .collect();

    CoverageVisualization {
        summary_bars,
        heatmap,
    }
}

fn check_goal(label: &str, actual: f64, expected: Option<f64>, violations: &mut Vec<String>) {
    if let Some(expected) = expected {
        if actual + f64::EPSILON < expected {
            violations.push(format!(
                "{} {:.1}% is below required {:.1}%",
                label, actual, expected
            ));
        }
    }
}

fn percent(covered: u32, total: u32) -> f64 {
    if total == 0 {
        100.0
    } else {
        round_one((covered as f64 / total as f64) * 100.0)
    }
}

fn weighted_percent(covered: u32, total: u32) -> f64 {
    percent(covered, total)
}

fn round_one(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn render_html_report(report: &CoverageReport) -> String {
    let bars = report
        .visualization
        .summary_bars
        .iter()
        .map(|bar| {
            format!(
                r#"<section class="metric"><strong>{}</strong><span>{:.1}%</span><div class="bar"><i style="width:{:.1}%"></i></div><small>{}/{}</small></section>"#,
                html_escape(&bar.label),
                bar.percent,
                bar.percent.clamp(0.0, 100.0),
                bar.covered,
                bar.total
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let function_rows = report
        .functions
        .iter()
        .map(|function| {
            format!(
                "<tr><td>{}</td><td>{}-{} </td><td>{}</td><td>{}/{}</td><td>{}/{}</td><td>{}</td></tr>",
                html_escape(&function.name),
                function.start_line,
                function.end_line,
                if function.covered { "covered" } else { "missing" },
                function.lines_covered,
                function.lines_total,
                function.branches_covered,
                function.branches_total,
                html_escape(&function.test_cases.join(", "))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let branch_rows = report
        .branches
        .iter()
        .map(|branch| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{:?}</td><td>{}</td><td>{}/{}</td><td>{}</td></tr>",
                html_escape(&branch.function),
                branch.line,
                branch.kind,
                html_escape(&branch.condition),
                branch.paths_covered,
                branch.paths_total,
                html_escape(&branch.test_cases.join(", "))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let goals = report
        .goals
        .as_ref()
        .map(|goals| {
            let status = if goals.passed { "passed" } else { "failed" };
            let violations = if goals.violations.is_empty() {
                "No goal violations".to_string()
            } else {
                goals.violations.join("; ")
            };
            format!(
                "<p><strong>Coverage goals:</strong> {} - {}</p>",
                status,
                html_escape(&violations)
            )
        })
        .unwrap_or_default();

    format!(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>StarForge Contract Coverage</title>
  <style>
    body {{ font-family: system-ui, sans-serif; margin: 2rem; color: #17202a; }}
    .grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 1rem; }}
    .metric {{ border: 1px solid #d6dde6; border-radius: 8px; padding: 1rem; }}
    .metric strong {{ display: block; margin-bottom: .5rem; }}
    .metric span {{ font-size: 1.8rem; font-weight: 700; }}
    .bar {{ height: .65rem; background: #edf2f7; border-radius: 999px; overflow: hidden; margin: .75rem 0; }}
    .bar i {{ display: block; height: 100%; background: #2f855a; }}
    table {{ border-collapse: collapse; width: 100%; margin-top: 1rem; }}
    th, td {{ border: 1px solid #d6dde6; padding: .55rem; text-align: left; }}
    th {{ background: #f6f8fa; }}
  </style>
</head>
<body>
  <h1>Contract Coverage</h1>
  <p>Overall coverage: <strong>{:.1}%</strong></p>
  {}
  <div class="grid">{}</div>
  <h2>Function Coverage</h2>
  <table>
    <tr><th>Function</th><th>Lines</th><th>Status</th><th>Line Coverage</th><th>Branch Coverage</th><th>Tests</th></tr>
    {}
  </table>
  <h2>Branch Coverage</h2>
  <table>
    <tr><th>Function</th><th>Line</th><th>Kind</th><th>Condition</th><th>Paths</th><th>Tests</th></tr>
    {}
  </table>
</body>
</html>"#,
        report.coverage_percent, goals, bars, function_rows, branch_rows
    )
}

fn render_markdown_report(report: &CoverageReport) -> String {
    let mut output = String::new();
    output.push_str("# StarForge Contract Coverage\n\n");
    output.push_str(&format!(
        "- Overall: {:.1}%\n- Functions: {:.1}% ({}/{})\n- Lines: {:.1}% ({}/{})\n- Branches: {:.1}% ({}/{})\n\n",
        report.coverage_percent,
        report.function_coverage_percent,
        report.functions_covered,
        report.functions_total,
        report.line_coverage_percent,
        report.lines_covered,
        report.lines_total,
        report.branch_coverage_percent,
        report.branches_covered,
        report.branches_total
    ));

    if let Some(goals) = &report.goals {
        output.push_str(&format!(
            "Coverage goals: **{}**\n\n",
            if goals.passed { "passed" } else { "failed" }
        ));
        for violation in &goals.violations {
            output.push_str(&format!("- {}\n", violation));
        }
        output.push('\n');
    }

    output.push_str("| Function | Lines | Covered | Branch paths | Tests |\n");
    output.push_str("|---|---:|---|---:|---|\n");
    for function in &report.functions {
        output.push_str(&format!(
            "| {} | {}-{} | {} | {}/{} | {} |\n",
            function.name,
            function.start_line,
            function.end_line,
            if function.covered { "yes" } else { "no" },
            function.branches_covered,
            function.branches_total,
            function.test_cases.join(", ")
        ));
    }

    output
}

fn render_text_report(report: &CoverageReport) -> String {
    let mut output = String::new();
    output.push_str("StarForge Contract Coverage\n");
    output.push_str(&format!("Overall: {:.1}%\n", report.coverage_percent));
    output.push_str(&format!(
        "Functions: {:.1}% ({}/{})\n",
        report.function_coverage_percent, report.functions_covered, report.functions_total
    ));
    output.push_str(&format!(
        "Lines: {:.1}% ({}/{})\n",
        report.line_coverage_percent, report.lines_covered, report.lines_total
    ));
    output.push_str(&format!(
        "Branches: {:.1}% ({}/{})\n",
        report.branch_coverage_percent, report.branches_covered, report.branches_total
    ));

    if !report.uncovered_functions.is_empty() {
        output.push_str(&format!(
            "Uncovered functions: {}\n",
            report.uncovered_functions.join(", ")
        ));
    }

    if let Some(goals) = &report.goals {
        output.push_str(&format!(
            "Coverage goals: {}\n",
            if goals.passed { "passed" } else { "failed" }
        ));
        for violation in &goals.violations {
            output.push_str(&format!("  - {}\n", violation));
        }
    }

    output
}

fn portable_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn default_passed() -> bool {
    true
}
