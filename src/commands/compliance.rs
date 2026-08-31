use crate::utils::compliance::{
    self, build_default_policies, delete_policy, generate_compliance_statistics,
    generate_compliance_summary, get_policy, get_recent_reports, get_reports_by_contract,
    get_reports_by_network, list_policies, run_compliance_checks, toggle_policy,
    ComplianceSeverity, RiskLevel,
};
use crate::utils::print as p;
use anyhow::Result;
use clap::{Args, Subcommand};
use colored::Colorize;

#[derive(Subcommand)]
pub enum ComplianceCommands {
    /// Initialize default compliance policies
    Init,
    /// Run compliance checks on a deployment
    Check(CheckArgs),
    /// Manage compliance policies
    #[command(subcommand)]
    Policy(PolicyCommands),
    /// Generate compliance reports
    #[command(subcommand)]
    Report(ReportCommands),
    /// Perform risk assessment
    Risk(RiskArgs),
    /// Show compliance dashboard with statistics
    Dashboard,
    /// Get compliance statistics
    Stats,
}

#[derive(Subcommand)]
pub enum PolicyCommands {
    /// List all compliance policies
    List,
    /// Show policy details
    Show(ShowPolicyArgs),
    /// Enable a policy
    Enable(EnablePolicyArgs),
    /// Disable a policy
    Disable(DisablePolicyArgs),
    /// Delete a policy
    Delete(DeletePolicyArgs),
}

#[derive(Subcommand)]
pub enum ReportCommands {
    /// List recent compliance reports
    List(ListReportsArgs),
    /// Show a specific compliance report
    Show(ShowReportArgs),
    /// Generate a compliance summary for a time period
    Summary(SummaryArgs),
}

#[derive(Args)]
pub struct CheckArgs {
    /// Contract ID to check
    #[arg(long)]
    pub contract_id: String,
    /// Network to check against
    #[arg(long, default_value = "testnet")]
    pub network: String,
    /// Requested by (optional)
    #[arg(long, default_value = "cli-user")]
    pub requested_by: String,
    /// Output as JSON
    #[arg(long, default_value = "false")]
    pub json: bool,
}

#[derive(Args)]
pub struct ShowPolicyArgs {
    /// Policy ID
    pub id: String,
}

#[derive(Args)]
pub struct EnablePolicyArgs {
    /// Policy ID
    pub id: String,
}

#[derive(Args)]
pub struct DisablePolicyArgs {
    /// Policy ID
    pub id: String,
}

#[derive(Args)]
pub struct DeletePolicyArgs {
    /// Policy ID
    pub id: String,
}

#[derive(Args)]
pub struct ListReportsArgs {
    /// Maximum number of reports
    #[arg(long, default_value = "10")]
    pub limit: usize,
    /// Filter by contract ID
    #[arg(long)]
    pub contract_id: Option<String>,
    /// Filter by network
    #[arg(long)]
    pub network: Option<String>,
    /// Output as JSON
    #[arg(long, default_value = "false")]
    pub json: bool,
}

#[derive(Args)]
pub struct ShowReportArgs {
    /// Request ID
    pub request_id: String,
    /// Output as JSON
    #[arg(long, default_value = "false")]
    pub json: bool,
    /// Export as CSV
    #[arg(long, default_value = "false")]
    pub csv: bool,
}

#[derive(Args)]
pub struct SummaryArgs {
    /// Start of period (RFC3339 format)
    #[arg(long)]
    pub start: Option<String>,
    /// End of period (RFC3339 format)
    #[arg(long)]
    pub end: Option<String>,
    /// Output as JSON
    #[arg(long, default_value = "false")]
    pub json: bool,
}

#[derive(Args)]
pub struct RiskArgs {
    /// Contract ID to assess
    #[arg(long)]
    pub contract_id: String,
    /// Network
    #[arg(long, default_value = "testnet")]
    pub network: String,
    /// Output as JSON
    #[arg(long, default_value = "false")]
    pub json: bool,
}

pub fn handle(cmd: ComplianceCommands) -> Result<()> {
    match cmd {
        ComplianceCommands::Init => handle_init(),
        ComplianceCommands::Check(args) => handle_check(args),
        ComplianceCommands::Policy(sub) => match sub {
            PolicyCommands::List => handle_list_policies(),
            PolicyCommands::Show(args) => handle_show_policy(args),
            PolicyCommands::Enable(args) => handle_enable_policy(args),
            PolicyCommands::Disable(args) => handle_disable_policy(args),
            PolicyCommands::Delete(args) => handle_delete_policy(args),
        },
        ComplianceCommands::Report(sub) => match sub {
            ReportCommands::List(args) => handle_list_reports(args),
            ReportCommands::Show(args) => handle_show_report(args),
            ReportCommands::Summary(args) => handle_summary(args),
        },
        ComplianceCommands::Risk(args) => handle_risk(args),
        ComplianceCommands::Dashboard => handle_dashboard(),
        ComplianceCommands::Stats => handle_stats(),
    }
}

// ────────────────────────────────────────────────
// Handlers
// ────────────────────────────────────────────────

fn handle_init() -> Result<()> {
    p::header("Compliance Initialization");

    let policies = build_default_policies()?;

    println!();
    p::success(&format!(
        "Created {} default compliance policies",
        policies.len()
    ));

    for policy in &policies {
        let severity_color = match policy.severity {
            ComplianceSeverity::Blocking => "blocking".red().to_string(),
            ComplianceSeverity::Warning => "warning".yellow().to_string(),
            ComplianceSeverity::Info => "info".dimmed().to_string(),
        };
        let enabled_mark = if policy.enabled {
            "✓".green()
        } else {
            "✗".red()
        };
        println!(
            "  {} {} ({}) [{}]",
            enabled_mark,
            policy.name.white(),
            severity_color,
            policy.id[..12].cyan()
        );
    }

    println!();
    p::info("Run `starforge compliance policy list` to see all policies.");
    p::info("Run `starforge compliance check --contract-id <id>` to run compliance checks.");
    p::info("Run `starforge compliance risk --contract-id <id>` for a risk assessment.");

    Ok(())
}

fn handle_check(args: CheckArgs) -> Result<()> {
    p::header("Compliance Check");

    let request_id = format!(
        "req-{}",
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("0000")
    );

    let report = run_compliance_checks(
        &request_id,
        &args.contract_id,
        &args.network,
        &args.requested_by,
    )?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!();
    p::kv_accent("Request ID", &report.request_id);
    p::kv("Contract ID", &report.contract_id);
    p::kv("Network", &report.network);
    p::kv(
        "Timestamp",
        report.timestamp.get(..19).unwrap_or(&report.timestamp),
    );

    println!();
    p::separator();
    println!(
        "  {} {}\n",
        "Policy Checks".bright_white(),
        format!("({})", report.checks.len()).dimmed()
    );
    p::separator();

    for check in &report.checks {
        let status = if check.passed {
            "✓".green()
        } else {
            "✗".red()
        };
        let sev = match check.severity {
            ComplianceSeverity::Blocking => "BLOCKING".red().to_string(),
            ComplianceSeverity::Warning => "WARNING".yellow().to_string(),
            ComplianceSeverity::Info => "INFO".dimmed().to_string(),
        };
        println!("  {} {} [{}]", status, check.policy_name.white(), sev);
        println!("    {}", check.message.dimmed());
    }

    if !report.regulatory_checks.is_empty() {
        println!();
        p::separator();
        println!(
            "  {} {}\n",
            "Regulatory Checks".bright_white(),
            format!("({})", report.regulatory_checks.len()).dimmed()
        );
        p::separator();

        for check in &report.regulatory_checks {
            let status = if check.passed {
                "✓".green()
            } else {
                "✗".red()
            };
            let sev = match check.severity {
                ComplianceSeverity::Blocking => "BLOCKING".red().to_string(),
                ComplianceSeverity::Warning => "WARNING".yellow().to_string(),
                ComplianceSeverity::Info => "INFO".dimmed().to_string(),
            };
            println!(
                "  {} [{}] {} — {}",
                status,
                sev,
                check.framework.to_string().cyan(),
                check.requirement.dimmed()
            );
            println!("    {}", check.message);
        }
    }

    if !report.best_practices.is_empty() {
        println!();
        p::separator();
        println!(
            "  {} {}\n",
            "Best Practices".bright_white(),
            format!("({})", report.best_practices.len()).dimmed()
        );
        p::separator();

        for practice in &report.best_practices {
            let status = if practice.passed {
                "✓".green()
            } else {
                "⚠".yellow()
            };
            println!(
                "  {} [{}] {}",
                status,
                practice.category.cyan(),
                practice.check
            );
            if !practice.passed {
                println!("    {}", "Recommendation:".yellow());
                println!("    {}", practice.recommendation);
            }
        }
    }

    if let Some(ref risk) = report.risk_assessment {
        println!();
        p::separator();
        println!("  {} {}\n", "Risk Assessment".bright_white(), "—".dimmed());
        p::separator();

        let risk_color = match risk.overall_level {
            RiskLevel::Low => risk.overall_level.to_string().green().to_string(),
            RiskLevel::Medium => risk.overall_level.to_string().yellow().to_string(),
            RiskLevel::High => risk.overall_level.to_string().red().to_string(),
            RiskLevel::Critical => risk.overall_level.to_string().red().bold().to_string(),
        };
        p::kv("Overall Risk Level", &risk_color);
        p::kv("Risk Score", &format!("{}/100", risk.overall_score));
        let approved_str = if risk.approved_for_deployment {
            format!("{}", "yes".green())
        } else {
            format!("{}", "no".red())
        };
        p::kv("Approved for Deployment", &approved_str);

        if !risk.factors.is_empty() {
            println!();
            println!("  {}:", "Risk Factors".bright_white());
            for factor in &risk.factors {
                let score_color = match factor.score {
                    0..=24 => factor.score.to_string().green(),
                    25..=49 => factor.score.to_string().yellow(),
                    50..=74 => factor.score.to_string().red(),
                    _ => factor.score.to_string().red().bold(),
                };
                println!("    • {} (score: {}/100)", factor.name.white(), score_color);
                println!("      {}", factor.description.dimmed());
                if let Some(ref mitigation) = factor.mitigation {
                    println!("      {} {}", "→".cyan(), mitigation);
                }
            }
        }

        if !risk.recommendations.is_empty() {
            println!();
            println!("  {}:", "Recommendations".bright_white().yellow());
            for rec in &risk.recommendations {
                println!("    • {}", rec);
            }
        }
    }

    println!();
    p::separator();
    println!();
    if report.all_passed {
        p::success("All compliance checks passed");
    } else {
        p::warn(&format!(
            "Compliance: {} blocking, {} warning(s)",
            report.blocking_count, report.warning_count
        ));
    }

    Ok(())
}

fn handle_list_policies() -> Result<()> {
    p::header("Compliance Policies");

    let policies = list_policies()?;

    if policies.is_empty() {
        p::info("No policies configured. Run `starforge compliance init` to create defaults.");
        return Ok(());
    }

    p::separator();
    println!(
        "  {:<14} {:<36} {:<12} {:<10} {:<8}",
        "ID".dimmed(),
        "Name".dimmed(),
        "Type".dimmed(),
        "Severity".dimmed(),
        "Enabled".dimmed(),
    );
    println!("  {}", "─".repeat(90).dimmed());

    for policy in &policies {
        let sev = match policy.severity {
            ComplianceSeverity::Blocking => policy.severity.to_string().red().to_string(),
            ComplianceSeverity::Warning => policy.severity.to_string().yellow().to_string(),
            ComplianceSeverity::Info => policy.severity.to_string().dimmed().to_string(),
        };
        let enabled = if policy.enabled {
            "✓".green().to_string()
        } else {
            "✗".red().to_string()
        };
        let type_str = format!("{:?}", policy.policy_type);
        println!(
            "  {:<14} {:<36} {:<12} {:<10} {:<8}",
            policy.id[..12].cyan(),
            policy.name.truncate_or_pad(34),
            type_str,
            sev,
            enabled,
        );
    }
    p::separator();
    println!(
        "\n  {} {}",
        "Total:".dimmed(),
        policies.len().to_string().white()
    );

    Ok(())
}

fn handle_show_policy(args: ShowPolicyArgs) -> Result<()> {
    let policy =
        get_policy(&args.id)?.ok_or_else(|| anyhow::anyhow!("Policy '{}' not found", args.id))?;

    p::header("Policy Details");
    println!();
    p::kv_accent("ID", &policy.id);
    p::kv("Name", &policy.name);
    p::kv("Description", &policy.description);
    p::kv("Type", &format!("{:?}", policy.policy_type));
    p::kv("Severity", &policy.severity.to_string());
    p::kv("Enabled", if policy.enabled { "yes" } else { "no" });
    p::kv(
        "Created",
        policy.created_at.get(..19).unwrap_or(&policy.created_at),
    );
    p::kv(
        "Updated",
        policy.updated_at.get(..19).unwrap_or(&policy.updated_at),
    );

    if !policy.config.is_empty() {
        println!();
        p::separator();
        println!("  {}:", "Configuration".bright_white());
        for (key, value) in &policy.config {
            println!("    {} = {}", key.cyan(), value.white());
        }
    }

    Ok(())
}

fn handle_enable_policy(args: EnablePolicyArgs) -> Result<()> {
    let policy = toggle_policy(&args.id, true)?;
    p::success(&format!("Policy '{}' is now enabled", policy.name));
    Ok(())
}

fn handle_disable_policy(args: DisablePolicyArgs) -> Result<()> {
    let policy = toggle_policy(&args.id, false)?;
    p::success(&format!("Policy '{}' is now disabled", policy.name));
    Ok(())
}

fn handle_delete_policy(args: DeletePolicyArgs) -> Result<()> {
    delete_policy(&args.id)?;
    p::success(&format!("Policy '{}' deleted", args.id));
    Ok(())
}

fn handle_list_reports(args: ListReportsArgs) -> Result<()> {
    p::header("Compliance Reports");

    let reports = if let Some(ref contract_id) = args.contract_id {
        get_reports_by_contract(contract_id)?
    } else if let Some(ref network) = args.network {
        get_reports_by_network(network)?
    } else {
        get_recent_reports(args.limit)?
    };

    if reports.is_empty() {
        p::info("No compliance reports found.");
        return Ok(());
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&reports)?);
        return Ok(());
    }

    p::separator();
    println!(
        "  {:<14} {:<20} {:<12} {:<8} {:<8} {:<12}",
        "ID".dimmed(),
        "Contract".dimmed(),
        "Network".dimmed(),
        "Status".dimmed(),
        "Blocking".dimmed(),
        "Timestamp".dimmed(),
    );
    println!("  {}", "─".repeat(80).dimmed());

    for report in &reports {
        let status = if report.all_passed {
            "✓ PASS".green().to_string()
        } else {
            "✗ FAIL".red().to_string()
        };
        let ts = report.timestamp.get(..16).unwrap_or(&report.timestamp);
        println!(
            "  {:<14} {:<20} {:<12} {:<8} {:<8} {:<12}",
            report.request_id[..12].cyan(),
            report.contract_id.chars().take(18).collect::<String>(),
            report.network,
            status,
            report.blocking_count.to_string().red(),
            ts.dimmed(),
        );
    }
    p::separator();
    println!(
        "\n  {} {} reports found",
        "Total:".dimmed(),
        reports.len().to_string().white()
    );

    Ok(())
}

fn handle_show_report(args: ShowReportArgs) -> Result<()> {
    let report = compliance::get_report(&args.request_id)?
        .ok_or_else(|| anyhow::anyhow!("Report '{}' not found", args.request_id))?;

    if args.csv {
        println!("{}", compliance::export_report_csv(&report));
        return Ok(());
    }

    if args.json {
        println!("{}", compliance::export_report_json(&report)?);
        return Ok(());
    }

    p::header("Compliance Report Details");
    println!();
    p::kv_accent("Request ID", &report.request_id);
    p::kv("Contract ID", &report.contract_id);
    p::kv("Network", &report.network);
    p::kv(
        "Timestamp",
        report.timestamp.get(..19).unwrap_or(&report.timestamp),
    );
    let status_str = if report.all_passed {
        format!("{}", "PASSED".green())
    } else {
        format!("{}", "FAILED".red())
    };
    p::kv("Status", &status_str);
    p::kv("Blocking issues", &report.blocking_count.to_string());
    p::kv("Warnings", &report.warning_count.to_string());

    println!();
    p::separator();
    println!(
        "  {} {}\n",
        "Check Results".bright_white(),
        format!("({})", report.checks.len()).dimmed()
    );
    p::separator();

    for check in &report.checks {
        let status = if check.passed {
            "✓".green()
        } else {
            "✗".red()
        };
        println!(
            "  {} {} — {}",
            status,
            check.policy_name.white(),
            check.message.dimmed()
        );
    }

    if !report.regulatory_checks.is_empty() {
        println!();
        p::separator();
        println!(
            "  {} {}\n",
            "Regulatory Checks".bright_white(),
            format!("({})", report.regulatory_checks.len()).dimmed()
        );
        p::separator();

        for check in &report.regulatory_checks {
            let status = if check.passed {
                "✓".green()
            } else {
                "✗".red()
            };
            println!(
                "  {} [{}] {} — {}",
                status,
                check.framework,
                check.requirement.dimmed(),
                check.message
            );
        }
    }

    if !report.best_practices.is_empty() {
        println!();
        p::separator();
        println!(
            "  {} {}\n",
            "Best Practices".bright_white(),
            format!("({})", report.best_practices.len()).dimmed()
        );
        p::separator();

        for practice in &report.best_practices {
            let status = if practice.passed {
                "✓".green()
            } else {
                "⚠".yellow()
            };
            println!("  {} [{}] {}", status, practice.category, practice.check);
            println!("    {}", practice.recommendation.dimmed());
        }
    }

    if let Some(ref risk) = report.risk_assessment {
        println!();
        p::separator();
        println!("  {}:", "Risk Assessment".bright_white());
        println!(
            "    {} {} (score: {}/100)",
            "Level:".dimmed(),
            risk.overall_level,
            risk.overall_score
        );
        println!(
            "    {} {}",
            "Approved:".dimmed(),
            if risk.approved_for_deployment {
                "yes".green()
            } else {
                "no".red()
            }
        );
    }

    Ok(())
}

fn handle_summary(args: SummaryArgs) -> Result<()> {
    p::header("Compliance Summary");

    let summary = generate_compliance_summary(args.start.as_deref(), args.end.as_deref())?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    println!();
    p::kv(
        "Period",
        &format!("{} – {}", summary.period_start, summary.period_end),
    );
    p::kv("Total reports", &summary.total_reports.to_string());
    p::kv("Total checks", &summary.total_checks.to_string());
    println!();
    p::kv("Passed checks", &summary.passed_checks.to_string());
    p::kv("Failed checks", &summary.failed_checks.to_string());
    p::kv("Blocking issues", &summary.blocking_issues.to_string());
    p::kv("Warnings", &summary.warning_issues.to_string());
    println!();
    p::kv("Pass rate", &format!("{:.1}%", summary.pass_rate));

    Ok(())
}

fn handle_risk(args: RiskArgs) -> Result<()> {
    p::header("Risk Assessment");

    let request_id = format!(
        "risk-{}",
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("0000")
    );
    let report = run_compliance_checks(
        &request_id,
        &args.contract_id,
        &args.network,
        "risk-assessment",
    )?;

    if args.json {
        if let Some(ref risk) = report.risk_assessment {
            println!("{}", serde_json::to_string_pretty(risk)?);
        } else {
            println!("{{}}");
        }
        return Ok(());
    }

    if let Some(ref risk) = report.risk_assessment {
        println!();
        let risk_color = match risk.overall_level {
            RiskLevel::Low => risk.overall_level.to_string().green().to_string(),
            RiskLevel::Medium => risk.overall_level.to_string().yellow().to_string(),
            RiskLevel::High => risk.overall_level.to_string().red().to_string(),
            RiskLevel::Critical => risk.overall_level.to_string().red().bold().to_string(),
        };
        p::kv_accent("Overall Risk Level", &risk_color);
        p::kv("Risk Score", &format!("{}/100", risk.overall_score));
        let approved_str = if risk.approved_for_deployment {
            format!("{}", "yes".green())
        } else {
            format!("{}", "no".red())
        };
        p::kv("Approved for Deployment", &approved_str);

        println!();
        p::separator();
        println!("  {}:", "Risk Factors".bright_white());
        p::separator();

        for factor in &risk.factors {
            let score_color = match factor.score {
                0..=24 => factor.score.to_string().green(),
                25..=49 => factor.score.to_string().yellow(),
                50..=74 => factor.score.to_string().red(),
                _ => factor.score.to_string().red().bold(),
            };
            println!();
            println!(
                "  {} (score: {}/100)",
                factor.name.white().bold(),
                score_color
            );
            println!("    {}", factor.description.dimmed());
            if let Some(ref mitigation) = factor.mitigation {
                println!("    {} {}", "→".cyan(), mitigation);
            }
        }

        if !risk.recommendations.is_empty() {
            println!();
            println!();
            p::separator();
            println!("  {}:", "Recommendations".bright_white().yellow());
            p::separator();
            for rec in &risk.recommendations {
                println!("  • {}", rec);
            }
        }
    }

    println!();
    p::separator();
    Ok(())
}

fn handle_dashboard() -> Result<()> {
    p::header("Compliance Dashboard");

    let stats = generate_compliance_statistics()?;
    let reports = get_recent_reports(5)?;

    p::separator();
    p::kv("Total policies", &stats.total_policies.to_string());
    p::kv("Enabled policies", &stats.enabled_policies.to_string());
    p::kv("Total compliance reports", &stats.total_reports.to_string());
    println!();
    p::kv("Blocking issues (30d)", &stats.recent_blocking.to_string());
    p::kv("Warnings (30d)", &stats.recent_warnings.to_string());

    if !stats.most_failed_policies.is_empty() {
        println!();
        p::separator();
        println!("  {}:", "Most Failed Policies".bright_white().red());
        for (policy, count) in &stats.most_failed_policies {
            println!(
                "  {} {} times",
                format!("  • {}:", policy).dimmed(),
                count.to_string().red()
            );
        }
    }

    if !stats.network_breakdown.is_empty() {
        println!();
        p::separator();
        println!("  {}:", "Network Breakdown".bright_white());
        p::separator();
        println!(
            "  {:<12} {:>8} {:>8} {:>8} {:>8}",
            "Network".dimmed(),
            "Total".dimmed(),
            "Passed".dimmed(),
            "Blocked".dimmed(),
            "Warnings".dimmed(),
        );
        for (net, net_stats) in &stats.network_breakdown {
            println!(
                "  {:<12} {:>8} {:>8} {:>8} {:>8}",
                net,
                net_stats.total_deployments,
                net_stats.passed.to_string().green(),
                net_stats.blocked.to_string().red(),
                net_stats.warnings.to_string().yellow(),
            );
        }
    }

    if !reports.is_empty() {
        println!();
        p::separator();
        println!("  {}:", "Recent Reports".bright_white());
        p::separator();
        for report in &reports {
            let status = if report.all_passed {
                "✓".green()
            } else {
                "✗".red()
            };
            let ts = report.timestamp.get(..19).unwrap_or(&report.timestamp);
            println!(
                "  {} {} | {} | {} | blocking: {}",
                status,
                report.request_id[..12].cyan(),
                report.network,
                ts.dimmed(),
                report.blocking_count.to_string().red(),
            );
        }
    }

    println!();
    p::separator();
    println!();
    p::info("Run `starforge compliance stats` for detailed statistics.");
    p::info("Run `starforge compliance check --help` to check compliance for a deployment.");

    Ok(())
}

fn handle_stats() -> Result<()> {
    p::header("Compliance Statistics");

    let stats = generate_compliance_statistics()?;

    println!();
    println!("  {}:", "Overview".bright_white());
    p::kv("  Total policies", &stats.total_policies.to_string());
    p::kv("  Enabled policies", &stats.enabled_policies.to_string());
    p::kv(
        "  Total compliance reports",
        &stats.total_reports.to_string(),
    );
    p::kv(
        "  Blocking issues (30d)",
        &stats.recent_blocking.to_string(),
    );
    p::kv("  Warnings (30d)", &stats.recent_warnings.to_string());

    if !stats.most_failed_policies.is_empty() {
        println!();
        p::separator();
        println!("  {}:", "Most Frequently Failed Policies".bright_white());
        for (policy, count) in &stats.most_failed_policies {
            println!(
                "  {} {} failures",
                format!("  • {}:", policy).dimmed(),
                count.to_string().red()
            );
        }
    }

    if !stats.network_breakdown.is_empty() {
        println!();
        p::separator();
        println!("  {}:", "Compliance by Network".bright_white());
        for (net, net_stats) in &stats.network_breakdown {
            let pass_rate = if net_stats.total_deployments > 0 {
                (net_stats.passed as f64 / net_stats.total_deployments as f64) * 100.0
            } else {
                0.0
            };
            println!(
                "  {}: {} deployments ({} passed, {} blocked, {} warnings) — {:.1}% pass rate",
                net.cyan(),
                net_stats.total_deployments,
                net_stats.passed.to_string().green(),
                net_stats.blocked.to_string().red(),
                net_stats.warnings.to_string().yellow(),
                pass_rate,
            );
        }
    }

    Ok(())
}

// ────────────────────────────────────────────────
// Helper trait extension
// ────────────────────────────────────────────────

trait StringTruncateExt {
    fn truncate_or_pad(&self, max_len: usize) -> String;
}

impl StringTruncateExt for String {
    fn truncate_or_pad(&self, max_len: usize) -> String {
        if self.len() > max_len {
            format!("{}…", &self[..max_len - 1])
        } else {
            format!("{:width$}", self, width = max_len)
        }
    }
}
