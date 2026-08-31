use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulnerabilityFinding {
    pub id: String,
    pub title: String,
    pub severity: String,
    pub description: String,
    pub location: Option<String>,
    pub tool: String,
    pub remediation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSummary {
    pub critical: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
    pub info: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditResult {
    pub timestamp: String,
    pub contract_path: String,
    pub score: f64,
    pub findings: Vec<VulnerabilityFinding>,
    pub tools_used: Vec<String>,
    #[serde(default)]
    pub tool_statuses: Vec<AuditToolStatus>,
    pub summary: AuditSummary,
    #[serde(default)]
    pub ci_passed: bool,
}

pub struct AuditConfig {
    pub run_slither: bool,
    pub run_mythril: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditToolStatus {
    pub tool: String,
    pub available: bool,
    pub executed: bool,
    pub status: String,
    pub command: Option<String>,
    pub findings: u32,
    pub message: Option<String>,
}

pub fn run_audit(path: &Path, config: &AuditConfig) -> Result<AuditResult> {
    let mut findings = Vec::new();
    let mut tools_used = Vec::new();
    let mut tool_statuses = Vec::new();

    let builtin = run_builtin_analysis(path)?;
    let builtin_count = builtin.len() as u32;
    findings.extend(builtin);
    tools_used.push("starforge-builtin".to_string());
    tool_statuses.push(AuditToolStatus {
        tool: "starforge-builtin".to_string(),
        available: true,
        executed: true,
        status: "completed".to_string(),
        command: None,
        findings: builtin_count,
        message: Some("Built-in Soroban heuristics completed".to_string()),
    });

    collect_external_tool(
        "slither",
        &["STARFORGE_SLITHER_CMD"],
        config.run_slither,
        path,
        run_slither,
        &mut findings,
        &mut tools_used,
        &mut tool_statuses,
    );

    collect_external_tool(
        "mythril",
        &["STARFORGE_MYTHRIL_CMD", "STARFORGE_MYTH_CMD"],
        config.run_mythril,
        path,
        run_mythril,
        &mut findings,
        &mut tools_used,
        &mut tool_statuses,
    );

    let summary = compute_summary(&findings);
    let score = compute_score(&summary);
    let ci_passed = score >= 80.0 && summary.critical == 0;

    Ok(AuditResult {
        timestamp: Utc::now().to_rfc3339(),
        contract_path: path.to_string_lossy().to_string(),
        score,
        findings,
        tools_used,
        tool_statuses,
        summary,
        ci_passed,
    })
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
fn collect_external_tool(
    tool: &str,
    env_vars: &[&str],
    enabled: bool,
    path: &Path,
    runner: fn(&Path, &str) -> Result<Vec<VulnerabilityFinding>>,
    findings: &mut Vec<VulnerabilityFinding>,
    tools_used: &mut Vec<String>,
    tool_statuses: &mut Vec<AuditToolStatus>,
) {
    if !enabled {
        tool_statuses.push(skipped_tool_status(tool, "Disabled by audit configuration"));
        return;
    }

    let Some(command) = resolve_tool(default_command_for(tool), env_vars) else {
        tool_statuses.push(skipped_tool_status(
            tool,
            &format!(
                "{} not found. Install it or configure {}.",
                tool,
                env_vars.join("/")
            ),
        ));
        return;
    };

    match runner(path, &command) {
        Ok(mut tool_findings) => {
            let count = tool_findings.len() as u32;
            findings.append(&mut tool_findings);
            tools_used.push(tool.to_string());
            tool_statuses.push(AuditToolStatus {
                tool: tool.to_string(),
                available: true,
                executed: true,
                status: "completed".to_string(),
                command: Some(command),
                findings: count,
                message: None,
            });
        }
        Err(err) => {
            tool_statuses.push(AuditToolStatus {
                tool: tool.to_string(),
                available: true,
                executed: true,
                status: "failed".to_string(),
                command: Some(command),
                findings: 0,
                message: Some(err.to_string()),
            });
        }
    }
}

fn default_command_for(tool: &str) -> &str {
    match tool {
        "mythril" => "myth",
        other => other,
    }
}

fn skipped_tool_status(tool: &str, message: &str) -> AuditToolStatus {
    AuditToolStatus {
        tool: tool.to_string(),
        available: false,
        executed: false,
        status: "skipped".to_string(),
        command: None,
        findings: 0,
        message: Some(message.to_string()),
    }
}

fn resolve_tool(default_command: &str, env_vars: &[&str]) -> Option<String> {
    for env_var in env_vars {
        if let Ok(command) = env::var(env_var) {
            let command = command.trim();
            if !command.is_empty() {
                return Some(command.to_string());
            }
        }
    }

    if is_tool_available(default_command) {
        Some(default_command.to_string())
    } else {
        None
    }
}

fn is_tool_available(tool: &str) -> bool {
    let probe = if cfg!(windows) { "where" } else { "which" };
    Command::new(probe)
        .arg(tool)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_slither(path: &Path, command: &str) -> Result<Vec<VulnerabilityFinding>> {
    let output = Command::new(command)
        .arg(path)
        .arg("--json")
        .arg("-")
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "Slither exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    parse_slither_output(&String::from_utf8_lossy(&output.stdout))
}

fn parse_slither_output(json_str: &str) -> Result<Vec<VulnerabilityFinding>> {
    #[derive(Deserialize)]
    struct SlitherOut {
        results: Option<SlitherDetectors>,
    }
    #[derive(Deserialize)]
    struct SlitherDetectors {
        detectors: Option<Vec<SlitherDet>>,
    }
    #[derive(Deserialize)]
    struct SlitherDet {
        check: String,
        impact: String,
        description: String,
        elements: Option<Vec<SlitherElem>>,
    }
    #[derive(Deserialize)]
    struct SlitherElem {
        source_mapping: Option<SlitherSrc>,
    }
    #[derive(Deserialize)]
    struct SlitherSrc {
        filename_used: Option<String>,
        lines: Option<Vec<u32>>,
    }

    let result: SlitherOut = serde_json::from_str(json_str).unwrap_or(SlitherOut { results: None });
    let mut findings = Vec::new();

    if let Some(detectors) = result.results.and_then(|r| r.detectors) {
        for det in detectors {
            let severity = match det.impact.as_str() {
                "High" => "high",
                "Medium" => "medium",
                "Low" => "low",
                _ => "info",
            };
            let location = det
                .elements
                .as_ref()
                .and_then(|els| els.first())
                .and_then(|el| el.source_mapping.as_ref())
                .map(|sm| {
                    let file = sm.filename_used.as_deref().unwrap_or("unknown");
                    let lines = sm.lines.as_deref().unwrap_or(&[]);
                    match (lines.first(), lines.last()) {
                        (Some(f), Some(l)) => format!("{}:{}-{}", file, f, l),
                        _ => file.to_string(),
                    }
                });
            findings.push(VulnerabilityFinding {
                id: format!("SLITHER-{}", det.check.to_uppercase().replace('-', "_")),
                title: det.check.clone(),
                severity: severity.to_string(),
                description: det.description.clone(),
                location,
                tool: "slither".to_string(),
                remediation: slither_remediation(&det.check),
            });
        }
    }
    Ok(findings)
}

fn slither_remediation(check: &str) -> String {
    match check {
        "reentrancy-eth" | "reentrancy-no-eth" => {
            "Use checks-effects-interactions or add a reentrancy guard.".to_string()
        }
        "uninitialized-state" | "uninitialized-storage" => {
            "Initialize all state before it is read.".to_string()
        }
        "integer-overflow" | "integer-underflow" => {
            "Use checked arithmetic operations for amount and counter math.".to_string()
        }
        "arbitrary-send-eth" => "Validate the recipient before transferring funds.".to_string(),
        "suicidal" => "Remove or strictly restrict destructive operations.".to_string(),
        _ => format!("Review and fix the '{}' vulnerability pattern.", check),
    }
}

fn run_mythril(path: &Path, command: &str) -> Result<Vec<VulnerabilityFinding>> {
    let output = Command::new(command)
        .arg("analyze")
        .arg(path)
        .arg("--output")
        .arg("json")
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "Mythril exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    parse_mythril_output(&String::from_utf8_lossy(&output.stdout))
}

fn parse_mythril_output(json_str: &str) -> Result<Vec<VulnerabilityFinding>> {
    #[derive(Deserialize)]
    struct MythReport {
        issues: Option<Vec<MythIssue>>,
    }
    #[derive(Deserialize)]
    struct MythIssue {
        title: String,
        severity: String,
        description: Option<MythDesc>,
        filename: Option<String>,
        lineno: Option<u32>,
    }
    #[derive(Deserialize)]
    struct MythDesc {
        head: String,
        tail: Option<String>,
    }

    let report: MythReport = serde_json::from_str(json_str).unwrap_or(MythReport { issues: None });
    let mut findings = Vec::new();

    for issue in report.issues.unwrap_or_default() {
        let description = issue
            .description
            .as_ref()
            .map(|d| format!("{} {}", d.head, d.tail.as_deref().unwrap_or("")))
            .unwrap_or_else(|| issue.title.clone());

        let location = match (&issue.filename, issue.lineno) {
            (Some(f), Some(l)) => Some(format!("{}:{}", f, l)),
            (Some(f), None) => Some(f.clone()),
            _ => None,
        };

        let severity = match issue.severity.as_str() {
            "High" => "high",
            "Medium" => "medium",
            "Low" => "low",
            _ => "info",
        };

        findings.push(VulnerabilityFinding {
            id: format!("MYTHRIL-{}", issue.title.to_uppercase().replace(' ', "_")),
            title: issue.title.clone(),
            severity: severity.to_string(),
            description,
            location,
            tool: "mythril".to_string(),
            remediation: mythril_remediation(&issue.title),
        });
    }
    Ok(findings)
}

fn mythril_remediation(title: &str) -> String {
    let lower = title.to_ascii_lowercase();
    if lower.contains("reentrancy") {
        "Move state updates before external calls and add explicit authorization.".to_string()
    } else if lower.contains("overflow") || lower.contains("underflow") {
        "Use checked or saturating arithmetic for amount and counter updates.".to_string()
    } else if lower.contains("access") || lower.contains("authorization") {
        "Require the expected signer or admin before privileged logic.".to_string()
    } else {
        "Review the Mythril finding and apply the recommended fix.".to_string()
    }
}

fn run_builtin_analysis(path: &Path) -> Result<Vec<VulnerabilityFinding>> {
    let mut findings = Vec::new();

    // File-level checklist heuristics.
    let result = super::checklist::run_checklist(path)?;
    for item in result.items {
        if !item.passed {
            findings.push(VulnerabilityFinding {
                id: format!("SF-{}", item.id.to_uppercase()),
                title: item.title.clone(),
                severity: normalize_severity(&item.severity).to_string(),
                description: item.description.clone(),
                location: Some(path.to_string_lossy().to_string()),
                tool: "starforge-builtin".to_string(),
                remediation: builtin_remediation(&item.id),
            });
        }
    }

    // Line-level static analysis against the built-in pattern library. This is
    // what gives the offline scanner real coverage of reentrancy, unchecked
    // arithmetic, unsafe unwraps and missing authorization without needing
    // Slither or Mythril.
    findings.extend(run_pattern_analysis(path)?);

    Ok(findings)
}

fn run_pattern_analysis(path: &Path) -> Result<Vec<VulnerabilityFinding>> {
    use super::hardening::{apply_hardening, HardeningOptions};

    let hardening = apply_hardening(
        path,
        &HardeningOptions {
            apply_fixes: false,
            dry_run: true,
            pattern_ids: None,
        },
    )?;

    let file = hardening.file.clone();
    let findings = hardening
        .findings
        .into_iter()
        .map(|f| VulnerabilityFinding {
            id: format!("SF-PATTERN-{}", f.pattern_id.to_uppercase()),
            title: f.pattern_name,
            severity: f.severity,
            description: f.message,
            location: Some(format!("{}:{}", file, f.line)),
            remediation: pattern_remediation(&f.pattern_id),
            tool: "starforge-builtin".to_string(),
        })
        .collect();

    Ok(findings)
}

fn pattern_remediation(pattern_id: &str) -> String {
    super::patterns::SecurityPatternLibrary::by_id(pattern_id)
        .and_then(|p| p.fix.map(|fix| fix.description))
        .unwrap_or_else(|| format!("Review and remediate the '{}' pattern.", pattern_id))
}

fn builtin_remediation(id: &str) -> String {
    match id {
        "auth_check" | "auth-missing" => {
            "Add require_auth() before sensitive state changes.".to_string()
        }
        "overflow" | "unchecked-arithmetic" => {
            "Use checked_add, checked_sub, checked_mul, or saturating operations.".to_string()
        }
        "panic" | "unsafe-unwrap" => {
            "Replace unwrap/expect with explicit Result or fallback handling.".to_string()
        }
        "reentrancy" | "reentrancy-risk" => {
            "Avoid external calls before state changes and emit events after mutations.".to_string()
        }
        "no-upgrade-guard" => {
            "Require admin or governance authorization before upgrade operations.".to_string()
        }
        _ => format!("Review and fix the '{}' security pattern.", id),
    }
}

fn normalize_severity(severity: &str) -> &'static str {
    match severity.to_ascii_lowercase().as_str() {
        "critical" => "critical",
        "high" => "high",
        "medium" | "warning" => "medium",
        "low" => "low",
        _ => "info",
    }
}

fn compute_summary(findings: &[VulnerabilityFinding]) -> AuditSummary {
    let mut s = AuditSummary {
        critical: 0,
        high: 0,
        medium: 0,
        low: 0,
        info: 0,
    };
    for f in findings {
        match normalize_severity(&f.severity) {
            "critical" => s.critical += 1,
            "high" => s.high += 1,
            "medium" => s.medium += 1,
            "low" => s.low += 1,
            _ => s.info += 1,
        }
    }
    s
}

fn compute_score(s: &AuditSummary) -> f64 {
    let penalty = (s.critical as f64 * 30.0)
        + (s.high as f64 * 15.0)
        + (s.medium as f64 * 7.5)
        + (s.low as f64 * 2.5)
        + (s.info as f64 * 0.5);
    (100.0 - penalty).max(0.0)
}

pub fn format_report(result: &AuditResult) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Security Audit Report\n\
         =====================\n\
         Contract : {}\n\
         Timestamp: {}\n\
         Tools    : {}\n\
         Score    : {:.1}/100\n\
         CI ready : {}\n\n",
        result.contract_path,
        result.timestamp,
        result.tools_used.join(", "),
        result.score,
        if result.ci_passed { "yes" } else { "no" },
    ));
    out.push_str(&format!(
        "Summary\n\
         -------\n\
         Critical : {}\n\
         High     : {}\n\
         Medium   : {}\n\
         Low      : {}\n\
         Info     : {}\n\n",
        result.summary.critical,
        result.summary.high,
        result.summary.medium,
        result.summary.low,
        result.summary.info,
    ));
    out.push_str("Tool Status\n-----------\n");
    for tool in &result.tool_statuses {
        out.push_str(&format!(
            "- {}: {} (available: {}, findings: {})",
            tool.tool, tool.status, tool.available, tool.findings
        ));
        if let Some(message) = &tool.message {
            out.push_str(&format!(" - {}", message));
        }
        out.push('\n');
    }
    out.push('\n');

    if result.findings.is_empty() {
        out.push_str("No issues found.\n");
    } else {
        out.push_str("Findings\n--------\n");
        for (i, f) in result.findings.iter().enumerate() {
            out.push_str(&format!(
                "{}. [{}] {} ({})\n   {}\n   Remediation: {}\n",
                i + 1,
                f.severity.to_uppercase(),
                f.title,
                f.tool,
                f.description,
                f.remediation,
            ));
            if let Some(loc) = &f.location {
                out.push_str(&format!("   Location: {}\n", loc));
            }
            out.push('\n');
        }
    }
    out
}

pub fn format_html_report(result: &AuditResult) -> String {
    let rows = result
        .findings
        .iter()
        .map(|finding| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&finding.severity),
                html_escape(&finding.tool),
                html_escape(&finding.title),
                html_escape(finding.location.as_deref().unwrap_or("")),
                html_escape(&finding.remediation)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let tool_rows = result
        .tool_statuses
        .iter()
        .map(|tool| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&tool.tool),
                html_escape(&tool.status),
                tool.findings,
                html_escape(tool.message.as_deref().unwrap_or(""))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!doctype html>
<html>
<head><meta charset="utf-8"><title>Security Audit Report</title></head>
<body>
<h1>Security Audit Report</h1>
<p><strong>Contract:</strong> {}</p>
<p><strong>Score:</strong> {:.1}/100</p>
<p><strong>Generated:</strong> {}</p>
<h2>Tool Status</h2>
<table border="1"><tr><th>Tool</th><th>Status</th><th>Findings</th><th>Message</th></tr>{}</table>
<h2>Findings</h2>
<table border="1"><tr><th>Severity</th><th>Tool</th><th>Title</th><th>Location</th><th>Remediation</th></tr>{}</table>
</body>
</html>"#,
        html_escape(&result.contract_path),
        result.score,
        html_escape(&result.timestamp),
        tool_rows,
        rows
    )
}

pub fn generate_github_actions_workflow(contract_path: &Path, min_score: f64) -> String {
    format!(
        r#"name: StarForge Security Audit

on:
  pull_request:
  push:
    branches: [ master, main ]

jobs:
  security-audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Install optional audit tools
        run: |
          python -m pip install --upgrade pip
          pip install slither-analyzer mythril || true
      - name: Build StarForge
        run: cargo build --locked
      - name: Run contract security audit
        run: cargo run -- security audit {} --format json --min-score {:.1} --ci
"#,
        contract_path.display(),
        min_score
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_full_for_no_findings() {
        let s = AuditSummary {
            critical: 0,
            high: 0,
            medium: 0,
            low: 0,
            info: 0,
        };
        assert_eq!(compute_score(&s), 100.0);
    }

    #[test]
    fn score_floored_at_zero() {
        let s = AuditSummary {
            critical: 10,
            high: 10,
            medium: 10,
            low: 10,
            info: 10,
        };
        assert_eq!(compute_score(&s), 0.0);
    }

    #[test]
    fn summary_counts_correctly() {
        let findings = vec![
            VulnerabilityFinding {
                id: "x".to_string(),
                title: "t".to_string(),
                severity: "high".to_string(),
                description: "d".to_string(),
                location: None,
                tool: "builtin".to_string(),
                remediation: "r".to_string(),
            },
            VulnerabilityFinding {
                id: "y".to_string(),
                title: "t2".to_string(),
                severity: "warning".to_string(),
                description: "d2".to_string(),
                location: None,
                tool: "builtin".to_string(),
                remediation: "r2".to_string(),
            },
        ];
        let s = compute_summary(&findings);
        assert_eq!(s.high, 1);
        assert_eq!(s.medium, 1);
        assert_eq!(s.critical + s.low + s.info, 0);
    }

    #[test]
    fn parses_slither_json() {
        let raw = r#"{
          "results": {
            "detectors": [{
              "check": "reentrancy-no-eth",
              "impact": "High",
              "description": "External call before state update",
              "elements": [{
                "source_mapping": {
                  "filename_used": "src/lib.rs",
                  "lines": [10, 11]
                }
              }]
            }]
          }
        }"#;

        let findings = parse_slither_output(raw).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, "high");
        assert!(findings[0].remediation.contains("reentrancy"));
    }

    #[test]
    fn parses_mythril_json() {
        let raw = r#"{
          "issues": [{
            "title": "Integer Overflow",
            "severity": "Medium",
            "description": { "head": "Overflow possible", "tail": "on addition" },
            "filename": "src/lib.rs",
            "lineno": 42
          }]
        }"#;

        let findings = parse_mythril_output(raw).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].tool, "mythril");
        assert_eq!(findings[0].location.as_deref(), Some("src/lib.rs:42"));
    }

    #[test]
    fn generated_ci_workflow_runs_security_audit() {
        let workflow =
            generate_github_actions_workflow(Path::new("contracts/token/src/lib.rs"), 85.0);
        assert!(workflow.contains("security audit contracts/token/src/lib.rs"));
        assert!(workflow.contains("--min-score 85.0"));
    }

    fn write_temp_contract(src: &str) -> std::path::PathBuf {
        use std::io::Write;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "sf_audit_test_{}_{}.rs",
            std::process::id(),
            unique
        ));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(src.as_bytes()).unwrap();
        path
    }

    #[test]
    fn builtin_analysis_detects_line_patterns_without_external_tools() {
        let src = "pub fn withdraw(amount: i128) {\n\
                   \x20   let total = balance + amount;\n\
                   \x20   client.invoke_contract(&addr); storage.set(&key, &total);\n\
                   \x20   let v = data.unwrap();\n\
                   }\n";
        let path = write_temp_contract(src);
        let findings = run_builtin_analysis(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert!(
            findings
                .iter()
                .any(|f| f.id == "SF-PATTERN-REENTRANCY-RISK"),
            "expected reentrancy to be detected offline, got: {:?}",
            findings.iter().map(|f| f.id.clone()).collect::<Vec<_>>()
        );

        // Every line-level pattern finding must carry a line-qualified location,
        // a non-empty remediation, and be attributed to the built-in scanner.
        for f in findings.iter().filter(|f| f.id.starts_with("SF-PATTERN-")) {
            assert!(
                f.location.as_deref().unwrap_or_default().contains(':'),
                "missing line in location for {}",
                f.id
            );
            assert!(
                !f.remediation.is_empty(),
                "missing remediation for {}",
                f.id
            );
            assert_eq!(f.tool, "starforge-builtin");
        }
    }

    #[test]
    fn pattern_remediation_falls_back_for_unknown_pattern() {
        let remediation = pattern_remediation("does-not-exist");
        assert!(remediation.contains("does-not-exist"));
    }
}
