//! `starforge audit` — Dedicated security audit pipeline for Soroban contracts.
//!
//! Provides a top-level `audit` command with:
//! - External tool integration (Slither, Mythril, cargo-audit)
//! - Soroban-specific static analysis (built-in pattern library)
//! - Cargo dependency vulnerability scanning
//! - Security scoring (0–100) with severity-weighted penalties
//! - Remediation suggestions mapped to each finding
//! - CI/CD mode (`--ci`) that exits non-zero when score is below `--min-score`
//! - Output as human-readable text, JSON, or HTML
//! - Optional file output (`--out`)

use crate::utils::print as p;
use crate::utils::security::audit::{format_report, run_audit, AuditConfig};
use crate::utils::security_scanner::{
    run_cargo_audit, run_clippy_scan, CargoAuditConfig, ClippyScanConfig,
};
use anyhow::Result;
use clap::Args;
use colored::Colorize;
use std::fs;
use std::path::PathBuf;

/// Arguments for `starforge audit`
#[derive(Args, Debug)]
pub struct AuditArgs {
    /// Path to the Soroban contract source file (.rs) or directory
    pub path: PathBuf,

    /// Enable/disable Slither external scanner
    #[arg(long, default_value = "true")]
    pub slither: bool,

    /// Enable/disable Mythril external scanner
    #[arg(long, default_value = "true")]
    pub mythril: bool,

    /// Enable/disable cargo-audit dependency vulnerability scan
    #[arg(long, default_value = "true")]
    pub cargo_audit: bool,

    /// Enable/disable Clippy-based static analysis
    #[arg(long, default_value = "true")]
    pub clippy: bool,

    /// Output format: text | json | html
    #[arg(long, default_value = "text", value_parser = ["text", "json", "html"])]
    pub format: String,

    /// Write the report to this file instead of stdout
    #[arg(long)]
    pub out: Option<PathBuf>,

    /// CI mode: exit with code 1 when the security score is below this threshold (0–100)
    #[arg(long, default_value = "0")]
    pub min_score: f64,

    /// CI mode shorthand — implies --min-score 60 unless overridden
    #[arg(long, default_value = "false")]
    pub ci: bool,

    /// Suppress all output except errors and the final score line
    #[arg(long, default_value = "false")]
    pub quiet: bool,
}

pub async fn handle(args: AuditArgs) -> Result<()> {
    // Resolve effective minimum score.
    let effective_min = if args.ci && args.min_score == 0.0 {
        60.0
    } else {
        args.min_score
    };

    if !args.quiet {
        p::header("Soroban Contract Security Audit");
        p::kv("Target", &args.path.display().to_string());
        if args.ci || effective_min > 0.0 {
            p::kv("Min score (CI threshold)", &format!("{:.0}", effective_min));
        }
    }

    // --- 1. Core built-in + external tool audit ---
    let audit_cfg = AuditConfig {
        run_slither: args.slither,
        run_mythril: args.mythril,
    };

    let mut audit_result = run_audit(&args.path, &audit_cfg)?;

    // --- 2. cargo-audit: dependency vulnerability scan ---
    if args.cargo_audit {
        if !args.quiet {
            eprintln!("  → Running cargo-audit …");
        }
        let cargo_cfg = CargoAuditConfig {
            project_dir: project_dir_for(&args.path),
        };
        match run_cargo_audit(&cargo_cfg) {
            Ok(mut dep_findings) => {
                if !dep_findings.is_empty() {
                    if !args.quiet {
                        eprintln!(
                            "  {} cargo-audit: {} finding(s)",
                            "→".cyan(),
                            dep_findings.len()
                        );
                    }
                    audit_result.findings.append(&mut dep_findings);
                    audit_result.tools_used.push("cargo-audit".to_string());
                }
            }
            Err(e) => {
                if !args.quiet {
                    eprintln!("  {} cargo-audit skipped: {}", "⚠".yellow(), e);
                }
            }
        }
    }

    // --- 3. Clippy static analysis ---
    if args.clippy {
        if !args.quiet {
            eprintln!("  → Running Clippy static analysis …");
        }
        let clippy_cfg = ClippyScanConfig {
            project_dir: project_dir_for(&args.path),
        };
        match run_clippy_scan(&clippy_cfg) {
            Ok(mut clippy_findings) => {
                if !clippy_findings.is_empty() {
                    if !args.quiet {
                        eprintln!(
                            "  {} clippy: {} finding(s)",
                            "→".cyan(),
                            clippy_findings.len()
                        );
                    }
                    audit_result.findings.append(&mut clippy_findings);
                    audit_result.tools_used.push("clippy".to_string());
                }
            }
            Err(e) => {
                if !args.quiet {
                    eprintln!("  {} clippy skipped: {}", "⚠".yellow(), e);
                }
            }
        }
    }

    // Recompute score after merging all findings.
    let summary = recompute_summary(&audit_result.findings);
    let score = compute_final_score(&summary);
    audit_result.summary = summary;
    audit_result.score = score;

    // --- Print human-readable report ---
    if !args.quiet && args.format == "text" {
        print_text_report(&audit_result);
    }

    // --- Produce structured output ---
    match args.format.as_str() {
        "json" => {
            let json = serde_json::to_string_pretty(&audit_result)?;
            if let Some(out) = &args.out {
                fs::write(out, &json)?;
                if !args.quiet {
                    p::kv("Report saved (JSON)", &out.display().to_string());
                }
            } else {
                println!("{}", json);
            }
        }
        "html" => {
            let html = render_html_report(&audit_result);
            if let Some(out) = &args.out {
                fs::write(out, &html)?;
                if !args.quiet {
                    p::kv("Report saved (HTML)", &out.display().to_string());
                }
            } else {
                println!("{}", html);
            }
        }
        _ => {
            // text format: file output if requested
            if let Some(out) = &args.out {
                let text = format_report(&audit_result);
                fs::write(out, &text)?;
                if !args.quiet {
                    p::kv("Report saved (text)", &out.display().to_string());
                }
            }
        }
    }

    // --- CI gate ---
    if effective_min > 0.0 && audit_result.score < effective_min {
        anyhow::bail!(
            "Security score {:.1}/100 is below the required minimum of {:.1}. \
             Fix the findings above and re-run.",
            audit_result.score,
            effective_min
        );
    }

    if !args.quiet {
        p::success(&format!(
            "Audit complete — score {:.1}/100",
            audit_result.score
        ));
    } else {
        // In quiet CI mode, only emit the final score line.
        println!("{:.1}", audit_result.score);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Walk up from the given path to find the nearest Cargo.toml directory.
fn project_dir_for(path: &std::path::Path) -> PathBuf {
    let mut dir = if path.is_file() {
        path.parent().unwrap_or(path).to_path_buf()
    } else {
        path.to_path_buf()
    };

    loop {
        if dir.join("Cargo.toml").exists() {
            return dir;
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => return path.parent().unwrap_or(path).to_path_buf(),
        }
    }
}

/// Recompute the AuditSummary after merging findings from multiple tools.
fn recompute_summary(
    findings: &[crate::utils::security::audit::VulnerabilityFinding],
) -> crate::utils::security::audit::AuditSummary {
    use crate::utils::security::audit::AuditSummary;
    let mut s = AuditSummary {
        critical: 0,
        high: 0,
        medium: 0,
        low: 0,
        info: 0,
    };
    for f in findings {
        match f.severity.as_str() {
            "critical" => s.critical += 1,
            "high" => s.high += 1,
            "medium" => s.medium += 1,
            "low" => s.low += 1,
            _ => s.info += 1,
        }
    }
    s
}

/// Score = 100 minus severity-weighted penalties, floored at 0.
fn compute_final_score(s: &crate::utils::security::audit::AuditSummary) -> f64 {
    let penalty = (s.critical as f64 * 30.0)
        + (s.high as f64 * 15.0)
        + (s.medium as f64 * 7.5)
        + (s.low as f64 * 2.5)
        + (s.info as f64 * 0.5);
    (100.0_f64 - penalty).max(0.0)
}

/// Human-readable colourised report printed to stdout.
fn print_text_report(result: &crate::utils::security::audit::AuditResult) {
    let score_label = match result.score as u32 {
        90..=100 => "Excellent".green().to_string(),
        70..=89 => "Good".cyan().to_string(),
        50..=69 => "Fair".yellow().to_string(),
        _ => "Poor".red().to_string(),
    };

    p::separator();
    p::kv("Tools used", &result.tools_used.join(", "));
    p::kv(
        "Security score",
        &format!("{:.1}/100  ({})", result.score, score_label),
    );
    p::kv("Critical", &result.summary.critical.to_string());
    p::kv("High    ", &result.summary.high.to_string());
    p::kv("Medium  ", &result.summary.medium.to_string());
    p::kv("Low     ", &result.summary.low.to_string());
    p::kv("Info    ", &result.summary.info.to_string());

    if result.findings.is_empty() {
        println!();
        p::success("No security issues found.");
        return;
    }

    println!();
    println!("{}", "  Findings".bold());
    println!("  {}", "─".repeat(60));

    for (i, f) in result.findings.iter().enumerate() {
        let sev = match f.severity.as_str() {
            "critical" => f.severity.to_uppercase().red().bold().to_string(),
            "high" => f.severity.to_uppercase().red().to_string(),
            "medium" => f.severity.to_uppercase().yellow().to_string(),
            "low" => f.severity.to_uppercase().cyan().to_string(),
            _ => f.severity.to_uppercase().normal().to_string(),
        };
        println!();
        println!(
            "  {}. [{}] {}  ({})",
            i + 1,
            sev,
            f.title.bold(),
            f.tool.dimmed()
        );
        println!("     {}", f.description);
        println!("     {}: {}", "Remediation".green(), f.remediation);
        if let Some(loc) = &f.location {
            println!("     {}: {}", "Location".dimmed(), loc);
        }
    }
    println!();
}

/// Minimal self-contained HTML report.
fn render_html_report(result: &crate::utils::security::audit::AuditResult) -> String {
    let score_class = match result.score as u32 {
        90..=100 => "excellent",
        70..=89 => "good",
        50..=69 => "fair",
        _ => "poor",
    };

    let findings_html = result
        .findings
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let loc = f
                .location
                .as_deref()
                .map(|l| format!("<p><strong>Location:</strong> {}</p>", l))
                .unwrap_or_default();
            format!(
                r#"<div class="finding sev-{sev}">
  <h3>{n}. [{sev_up}] {title} <span class="tool">{tool}</span></h3>
  <p>{desc}</p>
  <p class="remediation"><strong>Remediation:</strong> {rem}</p>
  {loc}
</div>"#,
                sev = f.severity,
                sev_up = f.severity.to_uppercase(),
                n = i + 1,
                title = html_escape(&f.title),
                tool = html_escape(&f.tool),
                desc = html_escape(&f.description),
                rem = html_escape(&f.remediation),
                loc = loc,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>StarForge Security Audit Report</title>
<style>
  body {{ font-family: sans-serif; max-width: 900px; margin: 2rem auto; color: #222; }}
  h1 {{ color: #1a1a2e; }}
  .score {{ font-size: 3rem; font-weight: bold; }}
  .excellent {{ color: #16a34a; }}
  .good {{ color: #0891b2; }}
  .fair {{ color: #d97706; }}
  .poor {{ color: #dc2626; }}
  .summary-grid {{ display: grid; grid-template-columns: repeat(5,1fr); gap: 1rem; margin: 1rem 0; }}
  .summary-cell {{ text-align: center; padding: 0.75rem; border-radius: 8px; background: #f1f5f9; }}
  .finding {{ border-left: 4px solid #ccc; padding: 0.75rem 1rem; margin: 0.75rem 0; background: #f8fafc; }}
  .sev-critical {{ border-color: #7f1d1d; background: #fef2f2; }}
  .sev-high {{ border-color: #dc2626; background: #fff5f5; }}
  .sev-medium {{ border-color: #d97706; background: #fffbeb; }}
  .sev-low {{ border-color: #0891b2; background: #f0f9ff; }}
  .sev-info {{ border-color: #6b7280; background: #f9fafb; }}
  .tool {{ font-size: 0.8rem; background: #e2e8f0; padding: 2px 6px; border-radius: 4px; margin-left: 8px; }}
  .remediation {{ color: #16a34a; }}
  footer {{ margin-top: 2rem; color: #6b7280; font-size: 0.85rem; }}
</style>
</head>
<body>
<h1>🔒 StarForge Security Audit Report</h1>
<p><strong>Contract:</strong> {contract}</p>
<p><strong>Timestamp:</strong> {ts}</p>
<p><strong>Tools used:</strong> {tools}</p>

<div class="score {score_class}">{score:.1}/100</div>

<div class="summary-grid">
  <div class="summary-cell"><strong>{critical}</strong><br>Critical</div>
  <div class="summary-cell"><strong>{high}</strong><br>High</div>
  <div class="summary-cell"><strong>{medium}</strong><br>Medium</div>
  <div class="summary-cell"><strong>{low}</strong><br>Low</div>
  <div class="summary-cell"><strong>{info}</strong><br>Info</div>
</div>

<h2>Findings ({total})</h2>
{findings_html}

<footer>Generated by <strong>starforge audit</strong> — {ts}</footer>
</body>
</html>"#,
        contract = html_escape(&result.contract_path),
        ts = html_escape(&result.timestamp),
        tools = html_escape(&result.tools_used.join(", ")),
        score = result.score,
        score_class = score_class,
        critical = result.summary.critical,
        high = result.summary.high,
        medium = result.summary.medium,
        low = result.summary.low,
        info = result.summary.info,
        total = result.findings.len(),
        findings_html = findings_html,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_final_score_perfect() {
        use crate::utils::security::audit::AuditSummary;
        let s = AuditSummary {
            critical: 0,
            high: 0,
            medium: 0,
            low: 0,
            info: 0,
        };
        assert_eq!(compute_final_score(&s), 100.0);
    }

    #[test]
    fn compute_final_score_floored_at_zero() {
        use crate::utils::security::audit::AuditSummary;
        let s = AuditSummary {
            critical: 10,
            high: 10,
            medium: 10,
            low: 10,
            info: 10,
        };
        assert_eq!(compute_final_score(&s), 0.0);
    }

    #[test]
    fn compute_final_score_single_high() {
        use crate::utils::security::audit::AuditSummary;
        let s = AuditSummary {
            critical: 0,
            high: 1,
            medium: 0,
            low: 0,
            info: 0,
        };
        assert!((compute_final_score(&s) - 85.0).abs() < f64::EPSILON);
    }

    #[test]
    fn html_report_contains_score() {
        use crate::utils::security::audit::{AuditResult, AuditSummary};
        let result = AuditResult {
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            contract_path: "/tmp/test.rs".to_string(),
            score: 75.0,
            findings: vec![],
            tools_used: vec!["builtin".to_string()],
            tool_statuses: Vec::new(),
            ci_passed: true,
            summary: AuditSummary {
                critical: 0,
                high: 0,
                medium: 0,
                low: 0,
                info: 0,
            },
        };
        let html = render_html_report(&result);
        assert!(html.contains("75.0/100"));
        assert!(html.contains("StarForge Security Audit Report"));
    }

    #[test]
    fn html_escape_replaces_special_chars() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
    }

    #[test]
    fn project_dir_returns_cargo_toml_dir() {
        // Provide a path to a real file in this workspace — Cargo.toml should be found.
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
        let dir = project_dir_for(&path);
        assert!(dir.join("Cargo.toml").exists());
    }
}
