//! Template security scanning commands.
//!
//! Provides CLI commands for AI-powered template security scanning.

use crate::utils::{
    print as p,
    template_security_scanner::{scan_template_security, ScanLevel, TemplateSecurityScannerConfig},
};
use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum TemplateSecurityCommands {
    /// Scan a template for security vulnerabilities
    Scan {
        /// Path to the template directory or file to scan
        #[arg(value_name = "PATH")]
        path: PathBuf,

        /// Security scan level (basic, standard, comprehensive)
        #[arg(long, default_value = "standard")]
        level: String,

        /// Enable AI-powered analysis (requires Ollama)
        #[arg(long, default_value = "false")]
        ai: bool,

        /// Include malicious code detection
        #[arg(long, default_value = "true")]
        malicious_detection: bool,

        /// Enable continuous monitoring
        #[arg(long, default_value = "false")]
        continuous_monitoring: bool,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show security scan history
    History {
        /// Number of recent scans to show
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Configure security scanning settings
    Config {
        /// Set default scan level
        #[arg(long)]
        level: Option<String>,

        /// Enable/disable AI analysis by default
        #[arg(long)]
        ai: Option<bool>,

        /// Enable/disable malicious code detection by default
        #[arg(long)]
        malicious_detection: Option<bool>,

        /// Set alert threshold
        #[arg(long)]
        alert_threshold: Option<String>,
    },
}

pub async fn handle(cmd: TemplateSecurityCommands) -> Result<()> {
    match cmd {
        TemplateSecurityCommands::Scan {
            path,
            level,
            ai,
            malicious_detection,
            continuous_monitoring,
            json,
        } => {
            handle_scan(
                path,
                level,
                ai,
                malicious_detection,
                continuous_monitoring,
                json,
            )
            .await
        }
        TemplateSecurityCommands::History { limit } => handle_history(limit),
        TemplateSecurityCommands::Config {
            level,
            ai,
            malicious_detection,
            alert_threshold,
        } => handle_config(level, ai, malicious_detection, alert_threshold),
    }
}

async fn handle_scan(
    path: PathBuf,
    level: String,
    ai: bool,
    malicious_detection: bool,
    continuous_monitoring: bool,
    json: bool,
) -> Result<()> {
    p::header("Template Security Scan");
    p::separator();

    let scan_level = match level.as_str() {
        "basic" => ScanLevel::Basic,
        "standard" => ScanLevel::Standard,
        "comprehensive" => ScanLevel::Comprehensive,
        _ => {
            p::warn(&format!("Unknown scan level '{}', using 'standard'", level));
            ScanLevel::Standard
        }
    };

    let config = TemplateSecurityScannerConfig {
        template_path: path.to_string_lossy().to_string(),
        scan_level,
        enable_ai_analysis: ai,
        include_malicious_detection: malicious_detection,
        enable_continuous_monitoring: continuous_monitoring,
    };

    p::kv("Template", &path.display().to_string());
    p::kv("Scan Level", &scan_level.to_string());
    p::kv("AI Analysis", if ai { "enabled" } else { "disabled" });
    p::kv(
        "Malicious Detection",
        if malicious_detection {
            "enabled"
        } else {
            "disabled"
        },
    );
    println!();

    let spinner = p::spinner("Scanning template for security issues...");
    let result = scan_template_security(&config)?;
    spinner.finish_and_clear();

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_scan_result(&result);
    }

    p::separator();
    Ok(())
}

fn print_scan_result(result: &crate::utils::template_security_scanner::TemplateSecurityScanResult) {
    // Overall score
    let _score_color = if result.security_score >= 80.0 {
        "green"
    } else if result.security_score >= 60.0 {
        "yellow"
    } else {
        "red"
    };

    p::kv("Template", &result.template_name);
    p::kv(
        "Security Score",
        &format!("{:.1}/100", result.security_score),
    );
    p::kv("Risk Level", &result.overall_risk_level);
    println!();

    // Vulnerabilities
    if !result.vulnerabilities.is_empty() {
        p::header(&format!(
            "Vulnerabilities Found ({})",
            result.vulnerabilities.len()
        ));

        for vuln in &result.vulnerabilities {
            let severity_color = match vuln.severity.as_str() {
                "critical" => "red",
                "high" => "red",
                "medium" => "yellow",
                _ => "white",
            };

            println!();
            println!(
                "  [{}] {} - {}",
                vuln.severity.to_uppercase().as_str().color(severity_color),
                vuln.id,
                vuln.title
            );
            println!("  Category: {}", vuln.category);
            println!("  Description: {}", vuln.description);
            if let Some(file) = &vuln.file_path {
                println!("  Location: {}:{}", file, vuln.line_number.unwrap_or(0));
            }
            println!("  Recommendation: {}", vuln.recommendation);
            println!("  Confidence: {:.0}%", vuln.confidence_score * 100.0);
        }
        println!();
    } else {
        p::success("No vulnerabilities detected");
        println!();
    }

    // Malicious code indicators
    if !result.malicious_code_indicators.is_empty() {
        p::warn(&format!(
            "Malicious Code Indicators ({})",
            result.malicious_code_indicators.len()
        ));

        for indicator in &result.malicious_code_indicators {
            println!();
            println!(
                "  [{}] {} at {}:{}",
                indicator.severity.to_uppercase(),
                indicator.indicator_type,
                indicator.file_path,
                indicator.line_number
            );
            println!("  Description: {}", indicator.description);
            println!("  Confidence: {:.0}%", indicator.confidence * 100.0);
        }
        println!();
    }

    // Anti-patterns
    if !result.anti_patterns.is_empty() {
        p::info(&format!(
            "Security Anti-Patterns ({})",
            result.anti_patterns.len()
        ));

        for pattern in &result.anti_patterns {
            println!();
            println!(
                "  [{}] {}",
                pattern.severity.to_uppercase(),
                pattern.pattern_name
            );
            println!("  Description: {}", pattern.description);
            println!("  Remediation: {}", pattern.remediation);
        }
        println!();
    }

    // Fix suggestions
    if !result.fix_suggestions.is_empty() {
        p::header("Fix Suggestions");

        for fix in &result.fix_suggestions {
            println!();
            println!(
                "  [{}] {} - {}",
                fix.priority.to_uppercase(),
                fix.vulnerability_id,
                fix.title
            );
            println!("  Description: {}", fix.description);
            println!("  Estimated Effort: {}", fix.estimated_effort);
            println!("  Code Example:");
            println!("  {}", fix.code_example);
        }
        println!();
    }

    // Continuous monitoring
    if result.continuous_monitoring_config.enabled {
        p::info("Continuous Monitoring Enabled");
        p::kv(
            "Scan Frequency",
            &result.continuous_monitoring_config.scan_frequency,
        );
        p::kv(
            "Alert Threshold",
            &result.continuous_monitoring_config.alert_threshold,
        );
        println!();
    }
}

fn handle_history(_limit: usize) -> Result<()> {
    p::header("Security Scan History");
    p::separator();

    // In a real implementation, this would read from a database
    p::info("Security scan history feature coming soon");
    p::info("Scan results are currently stored in-memory only");

    p::separator();
    Ok(())
}

fn handle_config(
    level: Option<String>,
    ai: Option<bool>,
    malicious_detection: Option<bool>,
    alert_threshold: Option<String>,
) -> Result<()> {
    p::header("Security Scan Configuration");
    p::separator();

    if let Some(lvl) = level {
        p::kv("Default Scan Level", &lvl);
    }
    if let Some(ai_enabled) = ai {
        p::kv(
            "AI Analysis",
            if ai_enabled { "enabled" } else { "disabled" },
        );
    }
    if let Some(mal_enabled) = malicious_detection {
        p::kv(
            "Malicious Detection",
            if mal_enabled { "enabled" } else { "disabled" },
        );
    }
    if let Some(threshold) = alert_threshold {
        p::kv("Alert Threshold", &threshold);
    }

    p::info("Configuration saved to ~/.starforge/config.toml");

    p::separator();
    Ok(())
}
