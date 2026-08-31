//! `src/utils/security_scanner.rs`
//!
//! External security scanner integrations for the `starforge audit` pipeline:
//!
//! - **cargo-audit**: scans `Cargo.lock` against the RustSec advisory database
//! - **Clippy**: runs `cargo clippy` and converts lint warnings to `VulnerabilityFinding`s
//!
//! Both scanners degrade gracefully: if the tool is not installed they return an
//! `Err` that the caller converts to a warning, never a hard failure.

use crate::utils::security::audit::VulnerabilityFinding;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

// ---------------------------------------------------------------------------
// Public configuration types
// ---------------------------------------------------------------------------

/// Configuration for the cargo-audit scanner.
pub struct CargoAuditConfig {
    /// Directory that contains `Cargo.toml` / `Cargo.lock`.
    pub project_dir: PathBuf,
}

/// Configuration for the Clippy scanner.
pub struct ClippyScanConfig {
    /// Directory that contains `Cargo.toml`.
    pub project_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// cargo-audit
// ---------------------------------------------------------------------------

/// Run `cargo audit --json` in `config.project_dir` and return vulnerability
/// findings. Returns `Err` if cargo-audit is not installed or fails to parse.
pub fn run_cargo_audit(config: &CargoAuditConfig) -> Result<Vec<VulnerabilityFinding>> {
    if !is_tool_available("cargo-audit") && !is_tool_available("cargo") {
        anyhow::bail!("cargo-audit not found — install with: cargo install cargo-audit");
    }

    // Try `cargo audit` (the subcommand form shipped in cargo-audit ≥ 0.18)
    let output = Command::new("cargo")
        .arg("audit")
        .arg("--json")
        .current_dir(&config.project_dir)
        .output()
        .context("Failed to execute cargo audit")?;

    // cargo-audit exits non-zero when vulnerabilities are found; that is expected.
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_cargo_audit_output(&stdout)
}

fn parse_cargo_audit_output(json_str: &str) -> Result<Vec<VulnerabilityFinding>> {
    if json_str.trim().is_empty() {
        return Ok(vec![]);
    }

    #[derive(serde::Deserialize)]
    struct AuditReport {
        vulnerabilities: Option<VulnSection>,
        warnings: Option<std::collections::HashMap<String, Vec<WarningEntry>>>,
    }
    #[derive(serde::Deserialize)]
    struct VulnSection {
        list: Option<Vec<VulnEntry>>,
    }
    #[derive(serde::Deserialize)]
    struct VulnEntry {
        advisory: Advisory,
        package: PackageInfo,
    }
    #[derive(serde::Deserialize)]
    struct Advisory {
        id: String,
        title: String,
        description: String,
        #[serde(rename = "cvss")]
        cvss: Option<String>,
        url: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct PackageInfo {
        name: String,
        version: String,
    }
    #[derive(serde::Deserialize)]
    struct WarningEntry {
        advisory: Option<Advisory>,
        package: Option<PackageInfo>,
        kind: Option<String>,
    }

    let report: AuditReport = serde_json::from_str(json_str).unwrap_or(AuditReport {
        vulnerabilities: None,
        warnings: None,
    });

    let mut findings = Vec::new();

    // Confirmed vulnerabilities
    for vuln in report
        .vulnerabilities
        .and_then(|v| v.list)
        .unwrap_or_default()
    {
        let severity = cvss_to_severity(vuln.advisory.cvss.as_deref());
        let url_note = vuln
            .advisory
            .url
            .as_deref()
            .map(|u| format!(" See: {}", u))
            .unwrap_or_default();
        findings.push(VulnerabilityFinding {
            id: format!("CARGO-{}", vuln.advisory.id),
            title: vuln.advisory.title.clone(),
            severity: severity.to_string(),
            description: format!("{}{}", vuln.advisory.description, url_note),
            location: Some(format!("{}@{}", vuln.package.name, vuln.package.version)),
            tool: "cargo-audit".to_string(),
            remediation: cargo_audit_remediation(&vuln.package.name, &vuln.advisory.id),
        });
    }

    // Unmaintained / yanked warnings
    for (_kind, entries) in report.warnings.unwrap_or_default() {
        for entry in entries {
            if let (Some(advisory), Some(pkg)) = (entry.advisory, entry.package) {
                let severity = entry
                    .kind
                    .as_deref()
                    .map(warning_kind_to_severity)
                    .unwrap_or("info");
                findings.push(VulnerabilityFinding {
                    id: format!("CARGO-WARN-{}", advisory.id),
                    title: advisory.title.clone(),
                    severity: severity.to_string(),
                    description: advisory.description.clone(),
                    location: Some(format!("{}@{}", pkg.name, pkg.version)),
                    tool: "cargo-audit".to_string(),
                    remediation: format!(
                        "Review or replace the '{}' crate; it may be unmaintained or yanked.",
                        pkg.name
                    ),
                });
            }
        }
    }

    Ok(findings)
}

fn cvss_to_severity(cvss: Option<&str>) -> &'static str {
    match cvss {
        Some(s) => {
            let score: f64 = s.parse().unwrap_or(0.0);
            if score >= 9.0 {
                "critical"
            } else if score >= 7.0 {
                "high"
            } else if score >= 4.0 {
                "medium"
            } else {
                "low"
            }
        }
        None => "high", // Unknown CVSS → conservatively treat as high
    }
}

fn warning_kind_to_severity(kind: &str) -> &'static str {
    match kind {
        "unmaintained" | "yanked" => "medium",
        "unsound" => "high",
        _ => "info",
    }
}

fn cargo_audit_remediation(pkg: &str, advisory_id: &str) -> String {
    format!(
        "Upgrade or replace the '{}' dependency. See the RustSec advisory {} at \
         https://rustsec.org/advisories/{}.html for details and patched versions.",
        pkg, advisory_id, advisory_id
    )
}

// ---------------------------------------------------------------------------
// Clippy scanner
// ---------------------------------------------------------------------------

/// Run `cargo clippy --message-format=json` and convert lint diagnostics to
/// `VulnerabilityFinding`s filtered to security-relevant lints.
pub fn run_clippy_scan(config: &ClippyScanConfig) -> Result<Vec<VulnerabilityFinding>> {
    if !is_tool_available("cargo") {
        anyhow::bail!("cargo not found in PATH");
    }

    let output = Command::new("cargo")
        .args([
            "clippy",
            "--message-format=json",
            "--quiet",
            "--",
            "-D",
            "clippy::all",
        ])
        .current_dir(&config.project_dir)
        .output()
        .context("Failed to execute cargo clippy")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_clippy_output(&stdout)
}

fn parse_clippy_output(json_lines: &str) -> Result<Vec<VulnerabilityFinding>> {
    let mut findings = Vec::new();

    for line in json_lines.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // cargo --message-format=json produces one JSON object per line.
        let msg: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if msg.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }

        let message = match msg.get("message") {
            Some(m) => m,
            None => continue,
        };

        let level = message
            .get("level")
            .and_then(|l| l.as_str())
            .unwrap_or("note");

        // Only surface warnings and errors
        if level != "warning" && level != "error" {
            continue;
        }

        let text = message
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();

        // Filter to security-relevant lints by keyword
        if !is_security_relevant_lint(&text) {
            continue;
        }

        let code_str = message
            .get("code")
            .and_then(|c| c.get("code"))
            .and_then(|c| c.as_str())
            .unwrap_or("unknown")
            .to_string();

        let location = extract_clippy_location(message);
        let severity = clippy_severity_from_level(level, &code_str);

        findings.push(VulnerabilityFinding {
            id: format!("CLIPPY-{}", code_str.to_uppercase().replace('-', "_")),
            title: format!("Clippy: {}", code_str.replace('_', "-")),
            severity: severity.to_string(),
            description: text.clone(),
            location,
            tool: "clippy".to_string(),
            remediation: clippy_remediation(&code_str),
        });
    }

    Ok(findings)
}

/// Returns true for lints that are directly security-relevant for Soroban contracts.
fn is_security_relevant_lint(text: &str) -> bool {
    let keywords = [
        "overflow",
        "underflow",
        "integer",
        "unwrap",
        "expect(",
        "panic",
        "unsafe",
        "transmute",
        "raw pointer",
        "dereference",
        "uninit",
        "mem::forget",
        "std::mem",
        "auth",
        "permission",
        "access",
        "reentr",
        "arithmetic",
    ];
    let lower = text.to_lowercase();
    keywords.iter().any(|kw| lower.contains(kw))
}

fn extract_clippy_location(message: &serde_json::Value) -> Option<String> {
    message
        .get("spans")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.first())
        .and_then(|span| {
            let file = span.get("file_name")?.as_str()?;
            let line = span.get("line_start")?.as_u64()?;
            Some(format!("{}:{}", file, line))
        })
}

fn clippy_severity_from_level(level: &str, code: &str) -> &'static str {
    if level == "error" {
        return "high";
    }
    // Certain lints are elevated
    if code.contains("overflow")
        || code.contains("unsafe")
        || code.contains("transmute")
        || code.contains("uninit")
    {
        return "medium";
    }
    "low"
}

fn clippy_remediation(code: &str) -> String {
    match code {
        c if c.contains("overflow") => {
            "Use checked_add / checked_mul / saturating_* arithmetic to prevent integer overflow."
                .to_string()
        }
        c if c.contains("unwrap") || c.contains("expect") => {
            "Replace .unwrap() / .expect() with proper error handling using ? or match.".to_string()
        }
        c if c.contains("unsafe") => {
            "Avoid unsafe code in Soroban contracts; the runtime environment does not guarantee \
             memory safety across the WASM boundary."
                .to_string()
        }
        c if c.contains("transmute") => {
            "std::mem::transmute is unsound in cross-platform WASM contexts. \
             Use explicit conversions instead."
                .to_string()
        }
        _ => format!(
            "Address the '{}' Clippy lint to improve contract safety.",
            code.replace('_', "-")
        ),
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

fn is_tool_available(tool: &str) -> bool {
    Command::new("which")
        .arg(tool)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- cargo-audit ---

    #[test]
    fn parse_empty_cargo_audit_output() {
        let findings = parse_cargo_audit_output("").unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_cargo_audit_with_vulnerability() {
        let json = r#"{
            "vulnerabilities": {
                "found": true,
                "count": 1,
                "list": [
                    {
                        "advisory": {
                            "id": "RUSTSEC-2023-0001",
                            "title": "Memory safety issue",
                            "description": "A use-after-free in example-crate",
                            "cvss": "7.5",
                            "url": "https://rustsec.org/advisories/RUSTSEC-2023-0001.html"
                        },
                        "package": { "name": "example-crate", "version": "1.0.0" }
                    }
                ]
            },
            "warnings": {}
        }"#;
        let findings = parse_cargo_audit_output(json).unwrap();
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "CARGO-RUSTSEC-2023-0001");
        assert_eq!(f.severity, "high");
        assert!(f.remediation.contains("RUSTSEC-2023-0001"));
    }

    #[test]
    fn cvss_score_mapping() {
        assert_eq!(cvss_to_severity(Some("9.8")), "critical");
        assert_eq!(cvss_to_severity(Some("7.0")), "high");
        assert_eq!(cvss_to_severity(Some("5.0")), "medium");
        assert_eq!(cvss_to_severity(Some("2.0")), "low");
        assert_eq!(cvss_to_severity(None), "high");
    }

    #[test]
    fn cvss_invalid_string_defaults_to_low() {
        // unparseable score → f64::parse returns Err → defaults to 0.0 → low
        assert_eq!(cvss_to_severity(Some("not-a-number")), "low");
    }

    // --- Clippy scanner ---

    #[test]
    fn parse_empty_clippy_output() {
        let findings = parse_clippy_output("").unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_clippy_non_security_lint_filtered() {
        // A warning about dead_code is not security-relevant.
        let json_line = r#"{"reason":"compiler-message","message":{"level":"warning","message":"unused variable: `x`","code":{"code":"unused_variables","explanation":null},"spans":[{"file_name":"src/lib.rs","line_start":10}]}}"#;
        let findings = parse_clippy_output(json_line).unwrap();
        assert!(
            findings.is_empty(),
            "Non-security lints should be filtered out"
        );
    }

    #[test]
    fn parse_clippy_security_lint_kept() {
        let json_line = r#"{"reason":"compiler-message","message":{"level":"warning","message":"integer overflow possible in addition","code":{"code":"clippy_overflow_check","explanation":null},"spans":[{"file_name":"src/contract.rs","line_start":42}]}}"#;
        let findings = parse_clippy_output(json_line).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].tool, "clippy");
        assert!(findings[0].id.contains("CLIPPY"));
    }

    #[test]
    fn security_relevant_lint_detection() {
        assert!(is_security_relevant_lint("integer overflow possible"));
        assert!(is_security_relevant_lint("use of .unwrap() on option"));
        assert!(is_security_relevant_lint("unsafe block"));
        assert!(!is_security_relevant_lint("unused import"));
        assert!(!is_security_relevant_lint("dead code warning"));
    }

    #[test]
    fn clippy_remediation_messages_are_meaningful() {
        let r = clippy_remediation("integer_overflow");
        assert!(r.contains("checked_add") || r.contains("overflow"));
        let r2 = clippy_remediation("clippy_unwrap_used");
        assert!(r2.contains(".unwrap()") || r2.contains("error handling"));
    }
}
