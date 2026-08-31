//! `starforge ai-audit` — AI-powered security audit for Soroban contracts
//!
//! Uses Claude AI combined with static analysis to detect vulnerabilities
//! with comprehensive coverage and minimal false positives.

use crate::utils::print as p;
use crate::utils::security::{AuditLevel, AuditRequest};
use anyhow::Result;
use clap::Args;
use colored::*;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Args, Debug)]
pub struct AiAuditArgs {
    /// Path to the Soroban contract source file (.rs) or directory
    pub path: PathBuf,

    /// Contract name (defaults to file stem if not provided)
    #[arg(long)]
    pub name: Option<String>,

    /// Security level: basic, standard, comprehensive
    #[arg(long, default_value = "comprehensive")]
    pub level: String,

    /// Include attack scenario simulation
    #[arg(long, default_value = "true")]
    pub attack_simulation: bool,

    /// Output format: text, json, html
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Write output to file instead of stdout
    #[arg(long, short)]
    pub out: Option<PathBuf>,

    /// Quiet mode: only output score
    #[arg(long, short)]
    pub quiet: bool,
}

/// Parse audit level string to enum.
fn parse_audit_level(level: &str) -> AuditLevel {
    match level.to_lowercase().as_str() {
        "basic" => AuditLevel::Basic,
        "comprehensive" => AuditLevel::Comprehensive,
        _ => AuditLevel::Standard,
    }
}

/// Read contract source code from path.
fn read_contract_code(path: &PathBuf) -> Result<String> {
    if path.is_dir() {
        // Look for lib.rs or main.rs
        let lib_path = path.join("src/lib.rs");
        let main_path = path.join("src/main.rs");

        if lib_path.exists() {
            return fs::read_to_string(&lib_path)
                .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", lib_path.display(), e));
        }
        if main_path.exists() {
            return fs::read_to_string(&main_path)
                .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", main_path.display(), e));
        }

        Err(anyhow::anyhow!(
            "No src/lib.rs or src/main.rs found in {}",
            path.display()
        ))
    } else if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
        fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))
    } else {
        Err(anyhow::anyhow!(
            "Path must be a .rs file or directory: {}",
            path.display()
        ))
    }
}

/// Get contract name from path or use provided name.
fn get_contract_name(path: &Path, provided_name: Option<String>) -> String {
    if let Some(name) = provided_name {
        return name;
    }

    if let Some(file_name) = path.file_name() {
        if let Some(name_str) = file_name.to_str() {
            return name_str.replace(".rs", "");
        }
    }

    "UnnamedContract".to_string()
}

/// Determine overall risk color.
fn risk_color(risk: &str) -> colored::ColoredString {
    match risk.to_lowercase().as_str() {
        "critical" => risk.red().bold(),
        "high" => risk.red(),
        "medium" => risk.yellow(),
        "low" => risk.yellow(),
        "safe" => risk.green().bold(),
        _ => risk.white(),
    }
}

/// Format security score with color.
fn score_color(score: f64) -> colored::ColoredString {
    let score_str = format!("{:.1}/100", score);
    match score as u32 {
        90..=100 => score_str.green().bold(),
        70..=89 => score_str.green(),
        50..=69 => score_str.yellow(),
        20..=49 => score_str.red(),
        _ => score_str.red().bold(),
    }
}

/// Print human-readable audit report.
fn print_text_report(report: &crate::utils::security::SecurityAuditReport) {
    println!();
    p::header("AI-Powered Soroban Security Audit");
    p::separator();

    p::kv("Contract", &report.contract_name);
    p::kv("Audit Date", &report.audit_date);
    p::kv(
        "Overall Risk",
        &risk_color(&report.overall_risk).to_string(),
    );
    p::kv(
        "Security Score",
        &score_color(report.security_score).to_string(),
    );
    p::kv("Tools Used", &report.tools_used.join(", "));

    println!();
    println!("{}", "Summary".bold());
    println!("{}", report.summary);

    if !report.vulnerabilities.is_empty() {
        println!();
        println!(
            "{} ({})",
            "Vulnerabilities".bold(),
            report.vulnerabilities.len()
        );
        println!("{}", "─".repeat(80));

        let mut by_severity: std::collections::HashMap<&str, Vec<_>> =
            std::collections::HashMap::new();
        for vuln in &report.vulnerabilities {
            by_severity
                .entry(&vuln.severity)
                .or_insert_with(Vec::new)
                .push(vuln);
        }

        for severity in &["critical", "high", "medium", "low", "info"] {
            if let Some(vulns) = by_severity.get(severity) {
                for vuln in vulns {
                    let severity_badge = match vuln.severity.as_str() {
                        "critical" => format!("[{}]", vuln.severity.red().bold()),
                        "high" => format!("[{}]", vuln.severity.red()),
                        "medium" => format!("[{}]", vuln.severity.yellow()),
                        "low" => format!("[{}]", vuln.severity.yellow()),
                        _ => format!("[{}]", vuln.severity.white()),
                    };

                    println!("{} {}", severity_badge, vuln.title.bold());
                    println!("  Category: {}", vuln.category);
                    println!("  Description: {}", vuln.description);

                    if let Some(line) = vuln.line_number {
                        println!("  Line: {}", line);
                    }
                    if let Some(snippet) = &vuln.code_snippet {
                        println!("  Code: {}", snippet);
                    }
                    println!("  Recommendation: {}", vuln.recommendation);
                    println!();
                }
            }
        }
    } else {
        println!();
        p::success("✓ No vulnerabilities detected");
    }

    if !report.attack_scenarios.is_empty() {
        println!();
        println!(
            "{} ({})",
            "Attack Scenarios".bold(),
            report.attack_scenarios.len()
        );
        println!("{}", "─".repeat(80));

        for scenario in &report.attack_scenarios {
            println!("{}", scenario.name.bold());
            println!("  Likelihood: {}", scenario.likelihood);
            println!("  Description: {}", scenario.description);
            println!("  Impact: {}", scenario.impact);
            println!("  Attack Steps:");
            for (i, step) in scenario.steps.iter().enumerate() {
                println!("    {}. {}", i + 1, step);
            }
            println!();
        }
    }

    if !report.best_practice_violations.is_empty() {
        println!();
        println!(
            "{} ({})",
            "Best Practice Violations".bold(),
            report.best_practice_violations.len()
        );
        for violation in &report.best_practice_violations {
            println!("  • {}", violation);
        }
    }

    if !report.fix_suggestions.is_empty() {
        println!();
        println!(
            "{} ({})",
            "Fix Suggestions".bold(),
            report.fix_suggestions.len()
        );
        println!("{}", "─".repeat(80));

        for suggestion in &report.fix_suggestions {
            println!("{}", suggestion.title.bold());
            println!("  Priority: {}", suggestion.priority);
            println!("  Description: {}", suggestion.description);
            println!("  Code Example:");
            for line in suggestion.code_example.lines() {
                println!("    {}", line);
            }
            println!();
        }
    }

    println!();
    println!("{}", "─".repeat(80));
    p::warn(&report.false_positive_warning);
}

/// Format audit report as JSON.
fn format_json_report(report: &crate::utils::security::SecurityAuditReport) -> Result<String> {
    Ok(serde_json::to_string_pretty(report)?)
}

/// Format audit report as HTML.
fn format_html_report(report: &crate::utils::security::SecurityAuditReport) -> String {
    let vuln_html = report
        .vulnerabilities
        .iter()
        .map(|v| {
            format!(
                r#"<tr>
    <td>{}</td>
    <td><span class="severity-{}">{}</span></td>
    <td>{}</td>
    <td>{}</td>
    <td>{}</td>
</tr>"#,
                v.title, v.severity, v.severity, v.category, v.description, v.recommendation
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Soroban Security Audit Report</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
            margin: 2rem;
            color: #333;
            background: #f9f9f9;
        }}
        .container {{ max-width: 1200px; margin: 0 auto; background: white; padding: 2rem; border-radius: 8px; }}
        h1 {{ color: #1a73e8; margin-bottom: 0.5rem; }}
        .metadata {{ display: grid; grid-template-columns: repeat(2, 1fr); gap: 1rem; margin: 1rem 0; }}
        .metadata-item {{ padding: 0.5rem; background: #f0f0f0; border-radius: 4px; }}
        .metric-label {{ font-weight: bold; color: #666; }}
        .metric-value {{ font-size: 1.2em; color: #1a73e8; }}
        .risk-critical {{ color: #d32f2f; font-weight: bold; }}
        .risk-high {{ color: #f57c00; }}
        .risk-medium {{ color: #fbc02d; }}
        .risk-low {{ color: #689f38; }}
        .risk-safe {{ color: #388e3c; font-weight: bold; }}
        .severity-critical {{ background: #ffcdd2; color: #c62828; padding: 0.2rem 0.5rem; border-radius: 3px; }}
        .severity-high {{ background: #ffe0b2; color: #e65100; padding: 0.2rem 0.5rem; border-radius: 3px; }}
        .severity-medium {{ background: #fff9c4; color: #f57f17; padding: 0.2rem 0.5rem; border-radius: 3px; }}
        .severity-low {{ background: #c8e6c9; color: #1b5e20; padding: 0.2rem 0.5rem; border-radius: 3px; }}
        .section {{ margin: 2rem 0; }}
        table {{ width: 100%; border-collapse: collapse; }}
        th, td {{ text-align: left; padding: 0.75rem; border-bottom: 1px solid #ddd; }}
        th {{ background: #f5f5f5; font-weight: bold; }}
        tr:hover {{ background: #f9f9f9; }}
    </style>
</head>
<body>
    <div class="container">
        <h1>🔒 Soroban Security Audit Report</h1>
        
        <div class="metadata">
            <div class="metadata-item">
                <div class="metric-label">Contract</div>
                <div class="metric-value">{}</div>
            </div>
            <div class="metadata-item">
                <div class="metric-label">Overall Risk</div>
                <div class="metric-value risk-{}">{{ override }}</div>
            </div>
            <div class="metadata-item">
                <div class="metric-label">Security Score</div>
                <div class="metric-value">{:.1}/100</div>
            </div>
            <div class="metadata-item">
                <div class="metric-label">Audit Date</div>
                <div class="metric-value">{}</div>
            </div>
        </div>

        <div class="section">
            <h2>Summary</h2>
            <p>{}</p>
        </div>

        <div class="section">
            <h2>Vulnerabilities ({} found)</h2>
            <table>
                <thead>
                    <tr>
                        <th>Title</th>
                        <th>Severity</th>
                        <th>Category</th>
                        <th>Description</th>
                        <th>Recommendation</th>
                    </tr>
                </thead>
                <tbody>
                    {}
                </tbody>
            </table>
        </div>

        <p style="color: #999; font-size: 0.9em; margin-top: 3rem;">
            Generated by StarForge AI Security Audit
        </p>
    </div>
</body>
</html>"#,
        report.contract_name,
        report.overall_risk.to_lowercase(),
        report.security_score,
        report.audit_date,
        report.summary,
        report.vulnerabilities.len(),
        vuln_html
    )
}

/// Handle the `starforge ai-audit` command.
pub async fn handle(args: AiAuditArgs) -> Result<()> {
    // Feature-flag gate: the audit pipeline is gated on `ai.audit` so we can
    // do gradual rollouts and A/B experiments. Admins can flip the flag with
    // `starforge feature-flags enable ai.audit` / `rollout ai.audit --percent 25`.
    crate::commands::feature_flags_cmd::require_feature("ai.audit")?;

    // Get API key
    let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
        anyhow::anyhow!(
            "ANTHROPIC_API_KEY environment variable not set. Set it to your Anthropic API key."
        )
    })?;

    // Read contract code
    let contract_code = read_contract_code(&args.path)?;

    // Get contract name
    let contract_name = get_contract_name(&args.path, args.name.clone());

    // Parse audit level
    let security_level = parse_audit_level(&args.level);

    // Create audit service
    let service = crate::utils::security::AiAuditService::new(api_key)?;

    // Print status
    if !args.quiet {
        p::header("AI-Powered Soroban Security Audit");
        p::kv("Contract", &contract_name);
        p::kv("Level", &args.level);
        p::kv(
            "Attack Simulation",
            if args.attack_simulation { "on" } else { "off" },
        );
        println!();
        eprintln!("{}  Running AI security analysis…", "→".cyan());
    }

    // Prepare audit request
    let request = AuditRequest {
        contract_code,
        contract_name,
        include_attack_simulation: args.attack_simulation,
        security_level,
    };

    // Run audit
    let report = service.audit_contract(request).await?;

    // Generate output
    let output = match args.format.to_lowercase().as_str() {
        "json" => format_json_report(&report)?,
        "html" => format_html_report(&report),
        _ => {
            print_text_report(&report);
            "".to_string() // Already printed
        }
    };

    // Write output
    if !output.is_empty() {
        if let Some(out_path) = args.out {
            fs::write(&out_path, &output).map_err(|e| {
                anyhow::anyhow!("Failed to write output to {}: {}", out_path.display(), e)
            })?;
            if !args.quiet {
                p::success(&format!("Report written to {}", out_path.display()));
            }
        } else {
            println!("{}", output);
        }
    }

    // Print summary if not quiet
    if !args.quiet {
        println!();
        p::separator();
        p::kv(
            "Overall Risk",
            &risk_color(&report.overall_risk).to_string(),
        );
        p::kv(
            "Security Score",
            &score_color(report.security_score).to_string(),
        );
        p::kv("Vulnerabilities", &report.vulnerabilities.len().to_string());
        p::kv(
            "Attack Scenarios",
            &report.attack_scenarios.len().to_string(),
        );
    } else {
        // Quiet mode: just output score
        println!("{:.1}", report.security_score);
    }

    Ok(())
}
