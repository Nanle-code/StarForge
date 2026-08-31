use crate::utils::print as p;
use crate::utils::security::{
    apply_hardening, default_rules, evaluate_event, format_compliance_report,
    format_data_protection_report, format_report, generate_hardening_report, run_audit,
    run_checklist, validate_security, write_report, AnomalyDetector, AuditConfig, ComplianceEngine,
    ComplianceStandard, DataProtectionEngine, HardeningOptions, IncidentResponse, IncidentStore,
    ThreatDetectionEngine, ThreatFeed,
};
use crate::utils::stream::{EventStreamFilters, SorobanEventStream};
use crate::utils::{config, notifications, soroban};
use anyhow::Result;
use clap::{Args, Subcommand};
use std::fs;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Subcommand)]
pub enum SecurityCommands {
    /// Apply automated security hardening transforms
    Harden(HardenArgs),
    /// Run security checklist against contract source
    Checklist(ChecklistArgs),
    /// Validate contract against security patterns
    Validate(ValidateArgs),
    /// Generate hardening report (json or html)
    Report(ReportArgs),
    /// Continuous security monitoring for deployed contracts
    Monitor(SecurityMonitorArgs),
    /// Manage security incidents
    Incident(IncidentArgs),
    /// Run full security audit with external tools (Slither, Mythril) and built-in analysis
    Audit(AuditArgs),
    /// AI-powered threat detection and analysis
    ThreatDetect(ThreatDetectArgs),
    /// AI-powered compliance monitoring and reporting
    Compliance(ComplianceArgs),
    /// AI-powered data protection and encryption checks
    DataProtection(DataProtectionArgs),
}

#[derive(Args)]
pub struct AuditArgs {
    /// Path to Soroban contract source (.rs)
    pub path: PathBuf,
    /// Run Slither if installed
    #[arg(long, default_value = "true")]
    pub slither: bool,
    /// Run Mythril if installed
    #[arg(long, default_value = "true")]
    pub mythril: bool,
    /// Scan with built-in static analysis only; skip all external tools
    #[arg(long)]
    pub offline: bool,
    /// Output format: text or json
    #[arg(long, default_value = "text")]
    pub format: String,
    /// Save report to file instead of stdout
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// CI mode: exit non-zero if score is below threshold (0-100)
    #[arg(long)]
    pub min_score: Option<f64>,
    /// CI mode: default minimum score to 80 and fail on unmet threshold
    #[arg(long, default_value_t = false)]
    pub ci: bool,
    /// Write a GitHub Actions workflow that runs this audit
    #[arg(long)]
    pub ci_workflow_out: Option<PathBuf>,
    /// Track findings in the remediation tracker
    #[arg(long, default_value_t = false)]
    pub track: bool,
}

#[derive(Args)]
pub struct HardenArgs {
    /// Path to Soroban contract source (.rs)
    pub path: PathBuf,
    /// Apply auto-fix transforms (writes .hardened.rs)
    #[arg(long, default_value = "false")]
    pub apply: bool,
    /// Preview changes without writing files
    #[arg(long, default_value = "false")]
    pub dry_run: bool,
}

#[derive(Args)]
pub struct ChecklistArgs {
    pub path: PathBuf,
}

#[derive(Args)]
pub struct ValidateArgs {
    pub path: PathBuf,
}

#[derive(Args)]
pub struct ReportArgs {
    pub path: PathBuf,
    #[arg(long, default_value = "json")]
    pub format: String,
}

#[derive(Args)]
pub struct SecurityMonitorArgs {
    #[arg(long)]
    pub contract: String,
    #[arg(long, default_value = "testnet")]
    pub network: String,
    #[arg(long, default_value = "2")]
    pub interval: u64,
    #[arg(long, default_value = "true")]
    pub follow: bool,
    #[arg(long, default_value = "false")]
    pub auto_incident: bool,
}

#[derive(Subcommand)]
pub enum IncidentCommands {
    List,
    Ack {
        #[arg(long)]
        id: String,
    },
    Show {
        #[arg(long)]
        id: String,
    },
    CollectEvidence {
        #[arg(long)]
        id: String,
        #[arg(long)]
        evidence_type: String,
        #[arg(long)]
        description: String,
        #[arg(long)]
        data: String,
    },
    Notify {
        #[arg(long)]
        id: String,
        #[arg(long)]
        recipient: String,
        #[arg(long)]
        channel: String,
        #[arg(long)]
        message: String,
    },
    Analyze {
        #[arg(long)]
        id: String,
        #[arg(long)]
        root_cause: String,
        #[arg(long)]
        lessons: Vec<String>,
    },
    Summary {
        #[arg(long)]
        id: String,
    },
}

#[derive(Args)]
pub struct IncidentArgs {
    #[command(subcommand)]
    pub command: IncidentCommands,
}

#[derive(Args)]
pub struct ThreatDetectArgs {
    /// Path to Soroban contract source (.rs)
    pub path: PathBuf,
    /// Contract ID to monitor
    #[arg(long)]
    pub contract: String,
    /// Event type to analyze
    #[arg(long)]
    pub event_type: String,
    /// Event value/data to analyze
    #[arg(long)]
    pub event_value: String,
    /// Caller address
    #[arg(long, default_value = "unknown")]
    pub caller: String,
    /// Numeric value associated with event (optional)
    #[arg(long)]
    pub value: Option<f64>,
    /// Output format: text or json
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args)]
pub struct ComplianceArgs {
    /// Path to Soroban contract source (.rs)
    pub path: PathBuf,
    /// Compliance standards to check (gdpr, soc2, hipaa, iso27001)
    #[arg(long)]
    pub standards: Vec<String>,
    /// Save report to file
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Output format: text or json
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args)]
pub struct DataProtectionArgs {
    /// Path to Soroban contract source (.rs)
    pub path: PathBuf,
    /// Save report to file
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Output format: text or json
    #[arg(long, default_value = "text")]
    pub format: String,
}

pub async fn handle(cmd: SecurityCommands) -> Result<()> {
    match cmd {
        SecurityCommands::Harden(args) => handle_harden(args),
        SecurityCommands::Checklist(args) => handle_checklist(args),
        SecurityCommands::Validate(args) => handle_validate(args),
        SecurityCommands::Report(args) => handle_report(args),
        SecurityCommands::Monitor(args) => handle_monitor(args).await,
        SecurityCommands::Incident(args) => handle_incident(args),
        SecurityCommands::Audit(args) => handle_audit(args),
        SecurityCommands::ThreatDetect(args) => handle_threat_detect(args),
        SecurityCommands::Compliance(args) => handle_compliance(args),
        SecurityCommands::DataProtection(args) => handle_data_protection(args),
    }
}

fn handle_harden(args: HardenArgs) -> Result<()> {
    config::validate_file_path(&args.path, Some("rs"))?;
    p::header("Security Hardening");

    let result = apply_hardening(
        &args.path,
        &HardeningOptions {
            apply_fixes: args.apply,
            dry_run: args.dry_run || !args.apply,
            pattern_ids: None,
        },
    )?;

    p::kv("File", &result.file);
    p::kv("Findings", &result.findings.len().to_string());
    p::kv("Transforms applied", &result.transforms_applied.to_string());
    if let Some(out) = &result.output_path {
        p::kv("Output", &out.display().to_string());
    }

    for finding in &result.findings {
        println!(
            "  [{}] line {}: {} ({})",
            finding.severity, finding.line, finding.pattern_name, finding.pattern_id
        );
    }

    p::success("Hardening scan complete");
    Ok(())
}

fn handle_checklist(args: ChecklistArgs) -> Result<()> {
    config::validate_file_path(&args.path, Some("rs"))?;
    p::header("Security Checklist");

    let result = run_checklist(&args.path)?;
    p::kv("Score", &format!("{:.1}%", result.score_percent));
    p::kv("Passed", &result.passed.to_string());
    p::kv("Failed", &result.failed.to_string());

    for item in &result.items {
        let icon = if item.passed { "✓" } else { "✗" };
        println!(
            "  {} [{}] {} — {}",
            icon, item.severity, item.title, item.category
        );
    }

    Ok(())
}

fn handle_validate(args: ValidateArgs) -> Result<()> {
    config::validate_file_path(&args.path, Some("rs"))?;
    p::header("Security Validation");

    let result = validate_security(&args.path)?;
    p::kv("Valid", if result.valid { "yes" } else { "no" });
    p::kv("Critical", &result.critical.to_string());
    p::kv("High", &result.high.to_string());
    p::kv("Medium", &result.medium.to_string());
    p::kv("Low", &result.low.to_string());

    if !result.valid {
        anyhow::bail!("Security validation failed");
    }
    p::success("Security validation passed");
    Ok(())
}

fn handle_report(args: ReportArgs) -> Result<()> {
    config::validate_file_path(&args.path, Some("rs"))?;
    p::header("Security Hardening Report");

    let hardening = apply_hardening(
        &args.path,
        &HardeningOptions {
            apply_fixes: false,
            dry_run: true,
            pattern_ids: None,
        },
    )?;
    let checklist = run_checklist(&args.path)?;
    let validation = validate_security(&args.path)?;
    let report = generate_hardening_report(&args.path, hardening, checklist, validation)?;
    let path = write_report(&report, &args.format)?;

    p::kv("Report", &path.display().to_string());
    p::kv(
        "Security score",
        &format!("{:.1}%", report.summary.security_score),
    );
    p::success("Hardening report generated");
    Ok(())
}

async fn handle_monitor(args: SecurityMonitorArgs) -> Result<()> {
    config::validate_contract_id(&args.contract)?;
    config::validate_network(&args.network)?;

    p::header("Security Monitoring");
    p::kv("Contract", &args.contract);
    p::kv("Network", &args.network);

    let rpc_url = soroban::rpc_url(&args.network)?;
    let rules = default_rules();
    let threat_feed = ThreatFeed::default_feed();
    let mut anomaly = AnomalyDetector::new(&args.contract);

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || running.store(false, Ordering::SeqCst))?;
    }

    let mut stream = SorobanEventStream::new(rpc_url, args.contract.clone())
        .with_poll_interval(args.interval)
        .with_filters(EventStreamFilters::default());

    let report_dir = config::config_dir().join("security").join("reports");
    fs::create_dir_all(&report_dir)?;

    while running.load(Ordering::SeqCst) {
        match stream.next_batch().await {
            Ok(batch) => {
                for event in batch {
                    let security_events = evaluate_event(
                        &rules,
                        &args.contract,
                        event.ledger,
                        &event.id,
                        &event.topic,
                        &event.value,
                    );

                    for se in &security_events {
                        notifications::alert(&format!("[{}] {}", se.severity, se.message));

                        if args.auto_incident {
                            IncidentResponse::auto_respond(
                                &args.contract,
                                &se.severity,
                                &se.rule_name,
                                &se.message,
                            )?;
                        }
                    }

                    let threats = threat_feed.match_event(&event.value.to_string());
                    for threat in threats {
                        notifications::alert(&format!(
                            "Threat intel match [{}]: {}",
                            threat.severity, threat.description
                        ));
                    }

                    if let Some(anomaly_finding) = anomaly.record_event(None) {
                        notifications::warn(&anomaly_finding.message);
                    }
                }

                if !args.follow {
                    break;
                }
                stream.sleep().await;
            }
            Err(err) => {
                notifications::warn(&format!("Stream error: {}. Retrying…", err));
                stream.sleep_backoff().await;
            }
        }
    }

    p::success("Security monitoring session ended");
    Ok(())
}

fn handle_incident(args: IncidentArgs) -> Result<()> {
    match args.command {
        IncidentCommands::List => {
            p::header("Security Incidents");
            let incidents = IncidentStore::load_all()?;
            if incidents.is_empty() {
                p::info("No incidents recorded");
                return Ok(());
            }
            for inc in &incidents {
                println!(
                    "  {} [{}] {} — {:?} ({})",
                    inc.id, inc.severity, inc.title, inc.status, inc.created_at
                );
                if inc.playbook.is_some() {
                    println!("    Playbook: assigned");
                }
                if !inc.evidence.is_empty() {
                    println!("    Evidence: {} items", inc.evidence.len());
                }
            }
            Ok(())
        }
        IncidentCommands::Ack { id } => {
            let updated = IncidentStore::update_status(
                &id,
                crate::utils::security::IncidentStatus::Acknowledged,
            )?;
            p::success(&format!("Incident {} acknowledged", updated.id));
            Ok(())
        }
        IncidentCommands::Show { id } => {
            let summary = IncidentResponse::generate_incident_summary(&id)?;
            println!("{}", summary);
            Ok(())
        }
        IncidentCommands::CollectEvidence {
            id,
            evidence_type,
            description,
            data,
        } => {
            let item = IncidentStore::add_evidence(&id, &evidence_type, &description, &data)?;
            p::success(&format!(
                "Evidence {} collected for incident {}",
                item.id, id
            ));
            Ok(())
        }
        IncidentCommands::Notify {
            id,
            recipient,
            channel,
            message,
        } => {
            let notification =
                IncidentStore::notify_stakeholder(&id, &recipient, &channel, &message)?;
            p::success(&format!(
                "Notification {} sent to {} via {}",
                notification.id, recipient, channel
            ));
            Ok(())
        }
        IncidentCommands::Analyze {
            id,
            root_cause,
            lessons,
        } => {
            let analysis = IncidentResponse::complete_post_analysis(
                &id,
                &root_cause,
                lessons,
                vec![
                    "Review access controls".into(),
                    "Add monitoring rules".into(),
                ],
            )?;
            p::success(&format!("Post-incident analysis completed for {}", id));
            p::kv("Root cause", &analysis.root_cause);
            Ok(())
        }
        IncidentCommands::Summary { id } => {
            let summary = IncidentResponse::generate_incident_summary(&id)?;
            println!("{}", summary);
            Ok(())
        }
    }
}

fn handle_audit(args: AuditArgs) -> Result<()> {
    config::validate_file_path(&args.path, Some("rs"))?;
    p::header("Contract Security Audit");
    p::kv("Contract", &args.path.display().to_string());

    let cfg = AuditConfig {
        run_slither: args.slither && !args.offline,
        run_mythril: args.mythril && !args.offline,
    };

    let result = run_audit(&args.path, &cfg)?;
    let min_score = args.min_score.or_else(|| args.ci.then_some(80.0));

    let score_label = match result.score as u32 {
        90..=100 => "Excellent",
        70..=89 => "Good",
        50..=69 => "Fair",
        _ => "Poor",
    };

    p::separator();
    p::kv("Tools used", &result.tools_used.join(", "));
    p::kv("CI ready", if result.ci_passed { "yes" } else { "no" });
    p::kv(
        "Security score",
        &format!("{:.1}/100  ({})", result.score, score_label),
    );
    p::kv("Critical", &result.summary.critical.to_string());
    p::kv("High    ", &result.summary.high.to_string());
    p::kv("Medium  ", &result.summary.medium.to_string());
    p::kv("Low     ", &result.summary.low.to_string());
    p::kv("Info    ", &result.summary.info.to_string());

    println!();
    p::header("Tool Status");
    for tool in &result.tool_statuses {
        let detail = tool
            .message
            .as_deref()
            .map(|message| format!(" - {}", message))
            .unwrap_or_default();
        println!(
            "  {}: {} (findings: {}){}",
            tool.tool, tool.status, tool.findings, detail
        );
    }

    if !result.findings.is_empty() {
        println!();
        p::header("Findings");
        for (i, f) in result.findings.iter().enumerate() {
            println!(
                "  {}. [{}] {}  ({})",
                i + 1,
                f.severity.to_uppercase(),
                f.title,
                f.tool
            );
            println!("     {}", f.description);
            println!("     Remediation: {}", f.remediation);
            if let Some(loc) = &f.location {
                println!("     Location: {}", loc);
            }
            println!();
        }
    } else {
        println!();
        p::success("No security issues found.");
    }

    if args.track {
        let tracking_findings: Vec<_> = result
            .findings
            .iter()
            .map(|finding| {
                (
                    finding.title.clone(),
                    finding.severity.clone(),
                    finding.description.clone(),
                    finding.remediation.clone(),
                )
            })
            .collect();
        let created = crate::utils::security::track_findings("audit", &tracking_findings)?;
        if !created.is_empty() {
            p::info(&format!(
                "Created {} remediation tracking item(s)",
                created.len()
            ));
        }
    }

    if let Some(path) = &args.ci_workflow_out {
        let workflow = crate::utils::security::generate_github_actions_workflow(
            &args.path,
            min_score.unwrap_or(80.0),
        );
        fs::write(path, workflow)?;
        p::kv("CI workflow", &path.display().to_string());
    }

    match args.format.as_str() {
        "json" => {
            let json = serde_json::to_string_pretty(&result)?;
            if let Some(out) = &args.out {
                fs::write(out, &json)?;
                p::kv("Report saved", &out.display().to_string());
            } else {
                println!("{}", json);
            }
        }
        "html" => {
            let html = crate::utils::security::format_html_report(&result);
            if let Some(out) = &args.out {
                fs::write(out, &html)?;
                p::kv("Report saved", &out.display().to_string());
            } else {
                println!("{}", html);
            }
        }
        "text" => {
            if let Some(out) = &args.out {
                let text = format_report(&result);
                fs::write(out, &text)?;
                p::kv("Report saved", &out.display().to_string());
            }
        }
        _ => {
            anyhow::bail!(
                "Unsupported audit format '{}'. Use text, json, or html.",
                args.format
            );
        }
    }

    if let Some(min) = min_score {
        if result.score < min {
            anyhow::bail!(
                "Security score {:.1} is below required minimum {:.1}",
                result.score,
                min
            );
        }
    }

    p::success("Security audit complete");
    Ok(())
}

fn handle_threat_detect(args: ThreatDetectArgs) -> Result<()> {
    config::validate_file_path(&args.path, Some("rs"))?;
    p::header("AI Threat Detection");
    p::kv("Contract", &args.contract);
    p::kv("Event type", &args.event_type);

    let mut engine = ThreatDetectionEngine::new(&args.contract);

    let event = engine.analyze_event(
        &args.event_type,
        &args.event_value,
        &args.caller,
        args.value,
    )?;

    let summary = engine.threat_summary();

    p::separator();
    p::kv("Threat score", &format!("{:.2}", event.score));
    p::kv("Classification", event.classification.as_str());
    p::kv("Severity", &event.severity);

    if !event.indicators.is_empty() {
        println!();
        p::header("Indicators");
        for indicator in &event.indicators {
            println!("  - {}", indicator);
        }
    }

    if !event.recommended_actions.is_empty() {
        println!();
        p::header("Recommended Actions");
        for action in &event.recommended_actions {
            println!("  - {}", action);
        }
    }

    println!();
    p::kv("Total events analyzed", &summary.total_events.to_string());
    p::kv("Malicious", &summary.malicious.to_string());
    p::kv("Suspicious", &summary.suspicious.to_string());

    if args.format.as_str() == "json" {
        let json = serde_json::to_string_pretty(&event)?;
        println!("{}", json);
    }

    if event.classification == crate::utils::security::ThreatClassification::Malicious {
        notifications::alert(&format!(
            "CRITICAL THREAT DETECTED [{}]: score {:.2}",
            event.contract_id, event.score
        ));
    }

    p::success("Threat analysis complete");
    Ok(())
}

fn handle_compliance(args: ComplianceArgs) -> Result<()> {
    config::validate_file_path(&args.path, Some("rs"))?;
    p::header("AI Compliance Monitoring");

    let standards: Vec<ComplianceStandard> = args
        .standards
        .iter()
        .map(|s| match s.to_lowercase().as_str() {
            "gdpr" => ComplianceStandard::GDPR,
            "soc2" => ComplianceStandard::SOC2,
            "hipaa" => ComplianceStandard::HIPAA,
            "iso27001" => ComplianceStandard::ISO27001,
            _ => ComplianceStandard::Custom,
        })
        .collect();

    if standards.is_empty() {
        p::info("Checking all compliance standards...");
    } else {
        let names: Vec<&str> = standards.iter().map(|s| s.as_str()).collect();
        p::kv("Standards", &names.join(", "));
    }

    let engine = ComplianceEngine::new();
    let report = engine.check_compliance(&args.path, &standards)?;

    p::separator();
    p::kv("Score", &format!("{:.1}%", report.score));
    p::kv("Passed", &report.passed.to_string());
    p::kv("Failed", &report.failed.to_string());
    p::kv("Risk level", &report.risk_assessment.overall_risk);

    if !report.risk_assessment.critical_gaps.is_empty() {
        println!();
        p::header("Critical Gaps");
        for gap in &report.risk_assessment.critical_gaps {
            println!("  - {}", gap);
        }
    }

    println!();
    p::header("Results");
    for result in &report.results {
        let status = if result.passed { "PASS" } else { "FAIL" };
        println!(
            "  [{}] [{}] {} — {}",
            status, result.standard, result.title, result.severity
        );
        if !result.passed {
            println!("    Remediation: {}", result.remediation);
        }
    }

    let saved_path = engine.save_report(&report)?;
    p::kv("Report saved", &saved_path.display().to_string());

    let incidents = IncidentStore::load_all().unwrap_or_default();

    let critical_open = incidents
        .iter()
        .filter(|i| {
            i.severity.eq_ignore_ascii_case("critical")
                && !matches!(i.status, crate::utils::security::IncidentStatus::Resolved)
        })
        .count();
    let open_incidents = incidents
        .iter()
        .filter(|i| !matches!(i.status, crate::utils::security::IncidentStatus::Resolved))
        .count();

    let remediation_items = crate::utils::security::remediation::load_all().unwrap_or_default();
    let open_remediation = remediation_items
        .iter()
        .filter(|i| {
            let s = i.status.to_string();
            s != "resolved" && s != "verified"
        })
        .count();

    let mut score: i32 = 100;
    score -= (critical_open as i32) * 20;
    score -= ((open_incidents - critical_open) as i32) * 10;
    score -= (open_remediation as i32) * 5;
    let score = score.max(0);

    println!();
    p::kv("Security score", &format!("{}/100", score));
    println!();

    p::header("Risk Heatmap");
    println!("  Critical open incidents : {}", critical_open);
    println!("  Total open incidents    : {}", open_incidents);
    println!("  Open remediation items  : {}", open_remediation);
    println!();
    p::header("Incident Timeline (most recent)");
    if incidents.is_empty() {
        p::info("No incidents recorded");
    } else {
        let mut sorted_incidents = incidents.clone();
        sorted_incidents.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        for inc in sorted_incidents.iter().take(10) {
            println!(
                "  {} [{}] {} — {:?} ({})",
                inc.id, inc.severity, inc.title, inc.status, inc.created_at
            );
        }
    }
    println!();

    p::header("Compliance Status");
    p::kv(
        "No critical open incidents",
        if critical_open == 0 { "PASS" } else { "FAIL" },
    );
    p::kv(
        "Remediation backlog clear",
        if open_remediation == 0 {
            "PASS"
        } else {
            "FAIL"
        },
    );
    println!();

    match args.format.as_str() {
        "json" => {
            let json = serde_json::to_string_pretty(&report)?;
            if let Some(out) = &args.out {
                fs::write(out, &json)?;
                p::kv("JSON saved", &out.display().to_string());
            } else {
                println!("\n{}", json);
            }
        }
        _ => {
            if let Some(out) = &args.out {
                let text = format_compliance_report(&report);
                fs::write(out, &text)?;
                p::kv("Report saved", &out.display().to_string());
            }
        }
    }

    if !report.risk_assessment.critical_gaps.is_empty() {
        anyhow::bail!(
            "Compliance check failed: {} critical gaps found",
            report.risk_assessment.critical_gaps.len()
        );
    }

    p::success("Compliance check complete");
    Ok(())
}

fn handle_data_protection(args: DataProtectionArgs) -> Result<()> {
    config::validate_file_path(&args.path, Some("rs"))?;
    p::header("AI Data Protection");

    let engine = DataProtectionEngine::new();
    let result = engine.check_protection(&args.path)?;

    p::separator();
    p::kv("Score", &format!("{:.1}%", result.score));
    p::kv("Passed", &result.summary.passed.to_string());
    p::kv("Failed", &result.summary.failed.to_string());

    println!();
    p::header("Category Scores");
    p::kv(
        "Encryption",
        &format!("{:.1}%", result.summary.encryption_score),
    );
    p::kv(
        "Access Control",
        &format!("{:.1}%", result.summary.access_control_score),
    );
    p::kv(
        "Key Management",
        &format!("{:.1}%", result.summary.key_management_score),
    );
    p::kv(
        "Data Integrity",
        &format!("{:.1}%", result.summary.integrity_score),
    );

    println!();
    p::header("Check Results");
    for check in &result.checks {
        let status = if check.passed { "PASS" } else { "FAIL" };
        println!("  [{}] {} — {}", status, check.title, check.severity);
        if !check.passed {
            println!("    {}", check.details);
            println!("    Remediation: {}", check.remediation);
        }
    }

    let saved_path = engine.save_result(&result)?;
    p::kv("Report saved", &saved_path.display().to_string());

    match args.format.as_str() {
        "json" => {
            let json = serde_json::to_string_pretty(&result)?;
            if let Some(out) = &args.out {
                fs::write(out, &json)?;
                p::kv("JSON saved", &out.display().to_string());
            } else {
                println!("\n{}", json);
            }
        }
        _ => {
            if let Some(out) = &args.out {
                let text = format_data_protection_report(&result);
                fs::write(out, &text)?;
                p::kv("Report saved", &out.display().to_string());
            }
        }
    }

    if result.summary.failed > 0 {
        anyhow::bail!(
            "Data protection check: {} checks failed",
            result.summary.failed
        );
    }

    p::success("Data protection check complete");
    Ok(())
}
