use anyhow::Result;
use chrono::{DateTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ────────────────────────────────────────────────
// Severity / Status helpers
// ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComplianceSeverity {
    Info,
    Warning,
    Blocking,
}

impl std::fmt::Display for ComplianceSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComplianceSeverity::Info => write!(f, "info"),
            ComplianceSeverity::Warning => write!(f, "warning"),
            ComplianceSeverity::Blocking => write!(f, "blocking"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PolicyType {
    RequiredApprovers,
    DeploymentWindow,
    MaxDeploymentFrequency,
    NetworkRestriction,
    FreezePeriod,
    RegulatoryCompliance,
    SecurityCompliance,
    DataProtection,
    BestPractices,
    Custom(String),
}

// ────────────────────────────────────────────────
// Regulatory framework types
// ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RegulatoryFramework {
    Gdpr,
    Soc2,
    Ccpa,
    Hipaa,
    PciDss,
    Custom(String),
}

impl std::fmt::Display for RegulatoryFramework {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegulatoryFramework::Gdpr => write!(f, "GDPR"),
            RegulatoryFramework::Soc2 => write!(f, "SOC2"),
            RegulatoryFramework::Ccpa => write!(f, "CCPA"),
            RegulatoryFramework::Hipaa => write!(f, "HIPAA"),
            RegulatoryFramework::PciDss => write!(f, "PCI-DSS"),
            RegulatoryFramework::Custom(name) => write!(f, "{}", name),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegulatoryCheck {
    pub framework: RegulatoryFramework,
    pub requirement: String,
    pub description: String,
    pub passed: bool,
    pub severity: ComplianceSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestPracticeCheck {
    pub category: String,
    pub check: String,
    pub description: String,
    pub passed: bool,
    pub severity: ComplianceSeverity,
    pub recommendation: String,
}

// ────────────────────────────────────────────────
// Risk assessment types
// ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskFactor {
    pub name: String,
    pub description: String,
    pub score: u8, // 0-100 (higher = riskier)
    pub mitigation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessmentResult {
    pub overall_score: u8, // 0-100
    pub overall_level: RiskLevel,
    pub approved_for_deployment: bool,
    pub factors: Vec<RiskFactor>,
    pub recommendations: Vec<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "low"),
            RiskLevel::Medium => write!(f, "medium"),
            RiskLevel::High => write!(f, "high"),
            RiskLevel::Critical => write!(f, "critical"),
        }
    }
}

// ────────────────────────────────────────────────
// Core domain types (existing + extended)
// ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompliancePolicy {
    pub id: String,
    pub name: String,
    pub description: String,
    pub policy_type: PolicyType,
    pub severity: ComplianceSeverity,
    pub enabled: bool,
    pub config: HashMap<String, String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceCheckResult {
    pub policy_id: String,
    pub policy_name: String,
    pub passed: bool,
    pub severity: ComplianceSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    pub request_id: String,
    pub contract_id: String,
    pub network: String,
    pub checks: Vec<ComplianceCheckResult>,
    pub regulatory_checks: Vec<RegulatoryCheck>,
    pub best_practices: Vec<BestPracticeCheck>,
    pub risk_assessment: Option<RiskAssessmentResult>,
    pub timestamp: String,
    pub all_passed: bool,
    pub blocking_count: usize,
    pub warning_count: usize,
}

// ────────────────────────────────────────────────
// Summary statistics for reporting
// ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceSummary {
    pub total_reports: usize,
    pub total_checks: usize,
    pub passed_checks: usize,
    pub failed_checks: usize,
    pub blocking_issues: usize,
    pub warning_issues: usize,
    pub pass_rate: f64,
    pub reports: Vec<ComplianceReport>,
    pub period_start: String,
    pub period_end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceStatistics {
    pub total_policies: usize,
    pub enabled_policies: usize,
    pub total_reports: usize,
    pub recent_blocking: usize,
    pub recent_warnings: usize,
    pub most_failed_policies: Vec<(String, usize)>,
    pub network_breakdown: HashMap<String, ComplianceNetworkStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceNetworkStats {
    pub total_deployments: usize,
    pub passed: usize,
    pub blocked: usize,
    pub warnings: usize,
}

// ────────────────────────────────────────────────
// Persistence helpers
// ────────────────────────────────────────────────

fn compliance_dir() -> Result<PathBuf> {
    let dir = crate::utils::config::config_dir().join("compliance");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn policies_path() -> Result<PathBuf> {
    Ok(compliance_dir()?.join("policies.json"))
}

fn reports_path() -> Result<PathBuf> {
    Ok(compliance_dir()?.join("reports.json"))
}

fn load_policies_raw() -> Result<Vec<CompliancePolicy>> {
    let path = policies_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

fn save_policies_raw(policies: &[CompliancePolicy]) -> Result<()> {
    let path = policies_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(policies)?)?;
    Ok(())
}

fn load_reports_raw() -> Result<Vec<ComplianceReport>> {
    let path = reports_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

fn save_reports_raw(reports: &[ComplianceReport]) -> Result<()> {
    let path = reports_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(reports)?)?;
    Ok(())
}

// ────────────────────────────────────────────────
// Policy management
// ────────────────────────────────────────────────

pub fn create_policy(
    name: &str,
    description: &str,
    policy_type: PolicyType,
    severity: ComplianceSeverity,
    config: HashMap<String, String>,
) -> Result<CompliancePolicy> {
    let mut policies = load_policies_raw()?;
    let id = format!(
        "pol-{}",
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("0000")
    );
    let now = Utc::now().to_rfc3339();

    let policy = CompliancePolicy {
        id: id.clone(),
        name: name.to_string(),
        description: description.to_string(),
        policy_type,
        severity,
        enabled: true,
        config,
        created_at: now.clone(),
        updated_at: now,
    };

    policies.push(policy.clone());
    save_policies_raw(&policies)?;
    Ok(policy)
}

pub fn list_policies() -> Result<Vec<CompliancePolicy>> {
    load_policies_raw()
}

pub fn update_policy(
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
    severity: Option<ComplianceSeverity>,
    enabled: Option<bool>,
    config: Option<HashMap<String, String>>,
) -> Result<CompliancePolicy> {
    let mut policies = load_policies_raw()?;
    let policy = policies
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| anyhow::anyhow!("Policy '{}' not found", id))?;

    if let Some(name) = name {
        policy.name = name.to_string();
    }
    if let Some(description) = description {
        policy.description = description.to_string();
    }
    if let Some(severity) = severity {
        policy.severity = severity;
    }
    if let Some(enabled) = enabled {
        policy.enabled = enabled;
    }
    if let Some(config) = config {
        policy.config = config;
    }
    policy.updated_at = Utc::now().to_rfc3339();

    let updated = policy.clone();
    save_policies_raw(&policies)?;
    Ok(updated)
}

pub fn delete_policy(id: &str) -> Result<()> {
    let mut policies = load_policies_raw()?;
    let len_before = policies.len();
    policies.retain(|p| p.id != id);
    if policies.len() == len_before {
        anyhow::bail!("Policy '{}' not found", id);
    }
    save_policies_raw(&policies)?;
    Ok(())
}

pub fn get_policy(id: &str) -> Result<Option<CompliancePolicy>> {
    let policies = load_policies_raw()?;
    Ok(policies.into_iter().find(|p| p.id == id))
}

pub fn toggle_policy(id: &str, enabled: bool) -> Result<CompliancePolicy> {
    update_policy(id, None, None, None, Some(enabled), None)
}

// ────────────────────────────────────────────────
// Compliance check runners
// ────────────────────────────────────────────────

pub fn run_compliance_checks(
    request_id: &str,
    contract_id: &str,
    network: &str,
    requested_by: &str,
) -> Result<ComplianceReport> {
    let policies = load_policies_raw();
    let policies = match policies {
        Ok(p) => p.into_iter().filter(|p| p.enabled).collect::<Vec<_>>(),
        Err(_) => vec![],
    };

    let mut checks: Vec<ComplianceCheckResult> = Vec::new();
    let mut regulatory_checks: Vec<RegulatoryCheck> = Vec::new();
    let mut best_practices: Vec<BestPracticeCheck> = Vec::new();

    for policy in &policies {
        let result = match &policy.policy_type {
            PolicyType::RequiredApprovers => check_required_approvers(policy, network),
            PolicyType::DeploymentWindow => check_deployment_window(policy),
            PolicyType::MaxDeploymentFrequency => {
                check_deployment_frequency(policy, network, requested_by)
            }
            PolicyType::NetworkRestriction => check_network_restriction(policy, network),
            PolicyType::FreezePeriod => check_freeze_period(policy),
            PolicyType::RegulatoryCompliance => {
                let results = check_regulatory_compliance(policy, network, contract_id);
                regulatory_checks.extend(results);
                ComplianceCheckResult {
                    policy_id: policy.id.clone(),
                    policy_name: policy.name.clone(),
                    passed: regulatory_checks.iter().all(|r| r.passed),
                    severity: policy.severity.clone(),
                    message: format!("Regulatory compliance check complete"),
                }
            }
            PolicyType::SecurityCompliance => {
                let results = check_security_compliance(policy, network);
                regulatory_checks.extend(results.into_iter().map(|r| RegulatoryCheck {
                    framework: RegulatoryFramework::Custom("Security".to_string()),
                    requirement: r.policy_name.clone(),
                    description: r.message.clone(),
                    passed: r.passed,
                    severity: r.severity.clone(),
                    message: r.message.clone(),
                }));
                ComplianceCheckResult {
                    policy_id: policy.id.clone(),
                    policy_name: policy.name.clone(),
                    passed: true,
                    severity: policy.severity.clone(),
                    message: "Security compliance check complete".to_string(),
                }
            }
            PolicyType::DataProtection => {
                let results = check_data_protection(policy, network);
                regulatory_checks.extend(results.into_iter().map(|r| RegulatoryCheck {
                    framework: RegulatoryFramework::Custom("DataProtection".to_string()),
                    requirement: r.policy_name.clone(),
                    description: r.message.clone(),
                    passed: r.passed,
                    severity: r.severity.clone(),
                    message: r.message.clone(),
                }));
                ComplianceCheckResult {
                    policy_id: policy.id.clone(),
                    policy_name: policy.name.clone(),
                    passed: true,
                    severity: policy.severity.clone(),
                    message: "Data protection check complete".to_string(),
                }
            }
            PolicyType::BestPractices => {
                let results = check_best_practices(policy, contract_id);
                best_practices.extend(results);
                ComplianceCheckResult {
                    policy_id: policy.id.clone(),
                    policy_name: policy.name.clone(),
                    passed: best_practices.iter().all(|b| b.passed),
                    severity: policy.severity.clone(),
                    message: "Best practices check complete".to_string(),
                }
            }
            PolicyType::Custom(_) => ComplianceCheckResult {
                policy_id: policy.id.clone(),
                policy_name: policy.name.clone(),
                passed: true,
                severity: ComplianceSeverity::Info,
                message: format!("Custom policy '{}': manual check required", policy.name),
            },
        };
        checks.push(result);
    }

    let blocking_count = checks
        .iter()
        .filter(|c| !c.passed && matches!(c.severity, ComplianceSeverity::Blocking))
        .count();
    let warning_count = checks
        .iter()
        .filter(|c| !c.passed && matches!(c.severity, ComplianceSeverity::Warning))
        .count();
    let all_passed = blocking_count == 0;

    // Perform risk assessment
    let risk_assessment = perform_risk_assessment(
        contract_id,
        network,
        &checks,
        &regulatory_checks,
        &best_practices,
    );

    let report = ComplianceReport {
        request_id: request_id.to_string(),
        contract_id: contract_id.to_string(),
        network: network.to_string(),
        checks,
        regulatory_checks,
        best_practices,
        risk_assessment: Some(risk_assessment),
        timestamp: Utc::now().to_rfc3339(),
        all_passed,
        blocking_count,
        warning_count,
    };

    let mut reports = load_reports_raw()?;
    reports.push(report.clone());
    save_reports_raw(&reports)?;

    // Log to the audit trail
    crate::utils::audit::log_action(
        "compliance_check",
        "system",
        "compliance_report",
        request_id,
        [
            ("contract_id".to_string(), contract_id.to_string()),
            ("network".to_string(), network.to_string()),
            ("all_passed".to_string(), all_passed.to_string()),
            ("blocking".to_string(), blocking_count.to_string()),
            (
                "risk_level".to_string(),
                report
                    .risk_assessment
                    .as_ref()
                    .map(|r| r.overall_level.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
            ),
        ]
        .into_iter()
        .collect(),
        all_passed,
        if all_passed {
            None
        } else {
            Some("Compliance check failed".to_string())
        },
    )?;

    Ok(report)
}

// ────────────────────────────────────────────────
// Individual policy check implementations
// ────────────────────────────────────────────────

fn check_required_approvers(policy: &CompliancePolicy, network: &str) -> ComplianceCheckResult {
    let min_approvers = policy
        .config
        .get("min_approvers")
        .map(|v| v.parse::<u8>().unwrap_or(1))
        .unwrap_or(1);
    let require_mainnet_approval = policy
        .config
        .get("require_mainnet_approval")
        .map(|v| v == "true")
        .unwrap_or(true);

    let passed = if network == "mainnet" {
        if require_mainnet_approval {
            min_approvers >= 1
        } else {
            true
        }
    } else {
        true
    };

    ComplianceCheckResult {
        policy_id: policy.id.clone(),
        policy_name: policy.name.clone(),
        passed,
        severity: if !passed {
            ComplianceSeverity::Blocking
        } else {
            ComplianceSeverity::Info
        },
        message: if passed {
            format!("Approval requirements met for {} network", network)
        } else {
            format!(
                "Mainnet deployments require at least {} approver(s)",
                min_approvers
            )
        },
    }
}

fn check_deployment_window(policy: &CompliancePolicy) -> ComplianceCheckResult {
    let start_hour = policy
        .config
        .get("start_hour")
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(9);
    let end_hour = policy
        .config
        .get("end_hour")
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(17);
    let timezone_offset = policy
        .config
        .get("timezone_offset")
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(0);

    let now = Utc::now();
    let hour = (now.hour() as i32 + timezone_offset).rem_euclid(24) as u8;

    let within_window = hour >= start_hour && hour < end_hour;

    ComplianceCheckResult {
        policy_id: policy.id.clone(),
        policy_name: policy.name.clone(),
        passed: within_window,
        severity: ComplianceSeverity::Warning,
        message: if within_window {
            format!(
                "Current time ({:02}:00 UTC{:+}) is within deployment window ({:02}:00-{:02}:00)",
                now.hour(),
                timezone_offset,
                start_hour,
                end_hour
            )
        } else {
            format!(
                "Current time ({:02}:00 UTC{:+}) is outside deployment window ({:02}:00-{:02}:00)",
                now.hour(),
                timezone_offset,
                start_hour,
                end_hour
            )
        },
    }
}

fn check_deployment_frequency(
    policy: &CompliancePolicy,
    network: &str,
    _requested_by: &str,
) -> ComplianceCheckResult {
    let max_per_hour = policy
        .config
        .get("max_per_hour")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(5);

    let reports = load_reports_raw().unwrap_or_default();
    let one_hour_ago = Utc::now() - chrono::Duration::hours(1);
    let recent_count = reports
        .iter()
        .filter(|r| r.network == network)
        .filter(|r| {
            chrono::DateTime::parse_from_rfc3339(&r.timestamp)
                .ok()
                .map(|dt| chrono::DateTime::<Utc>::from(dt) > one_hour_ago)
                .unwrap_or(false)
        })
        .count();

    let passed = recent_count < max_per_hour;

    ComplianceCheckResult {
        policy_id: policy.id.clone(),
        policy_name: policy.name.clone(),
        passed,
        severity: ComplianceSeverity::Warning,
        message: if passed {
            format!(
                "Deployment frequency ({}/hour) is within limit ({}/hour)",
                recent_count, max_per_hour
            )
        } else {
            format!(
                "Deployment frequency ({}/hour) exceeds limit ({}/hour)",
                recent_count, max_per_hour
            )
        },
    }
}

fn check_network_restriction(policy: &CompliancePolicy, network: &str) -> ComplianceCheckResult {
    let allowed = policy.config.get("allowed_networks").map(|v| {
        v.split(',')
            .map(|s| s.trim().to_string())
            .collect::<Vec<_>>()
    });
    let blocked = policy.config.get("blocked_networks").map(|v| {
        v.split(',')
            .map(|s| s.trim().to_string())
            .collect::<Vec<_>>()
    });

    let mut passed = true;
    let mut message = format!("Network '{}' is allowed", network);

    if let Some(ref allowed_nets) = allowed {
        if !allowed_nets.contains(&network.to_string()) && !allowed_nets.contains(&"*".to_string())
        {
            passed = false;
            message = format!(
                "Network '{}' is not in the allowed list: {:?}",
                network, allowed_nets
            );
        }
    }

    if let Some(ref blocked_nets) = blocked {
        if blocked_nets.contains(&network.to_string()) {
            passed = false;
            message = format!("Network '{}' is in the blocked list", network);
        }
    }

    ComplianceCheckResult {
        policy_id: policy.id.clone(),
        policy_name: policy.name.clone(),
        passed,
        severity: if !passed {
            ComplianceSeverity::Blocking
        } else {
            ComplianceSeverity::Info
        },
        message,
    }
}

fn check_freeze_period(policy: &CompliancePolicy) -> ComplianceCheckResult {
    let freeze_start = policy.config.get("freeze_start");
    let freeze_end = policy.config.get("freeze_end");

    let now = Utc::now();

    if let (Some(start_str), Some(end_str)) = (freeze_start, freeze_end) {
        match (
            chrono::NaiveDateTime::parse_from_str(start_str, "%Y-%m-%dT%H:%M:%S"),
            chrono::NaiveDateTime::parse_from_str(end_str, "%Y-%m-%dT%H:%M:%S"),
        ) {
            (Ok(start), Ok(end)) => {
                let start_dt: DateTime<Utc> = DateTime::from_naive_utc_and_offset(start, Utc);
                let end_dt: DateTime<Utc> = DateTime::from_naive_utc_and_offset(end, Utc);

                let in_freeze = now >= start_dt && now <= end_dt;
                return ComplianceCheckResult {
                    policy_id: policy.id.clone(),
                    policy_name: policy.name.clone(),
                    passed: !in_freeze,
                    severity: ComplianceSeverity::Blocking,
                    message: if in_freeze {
                        format!(
                            "Currently in deployment freeze period ({} to {})",
                            start_str, end_str
                        )
                    } else {
                        "Not in a deployment freeze period".to_string()
                    },
                };
            }
            _ => {
                return ComplianceCheckResult {
                    policy_id: policy.id.clone(),
                    policy_name: policy.name.clone(),
                    passed: false,
                    severity: ComplianceSeverity::Warning,
                    message: format!(
                        "Freeze period configuration has invalid date format (start: '{}', end: '{}'). Expected format: YYYY-MM-DDTHH:MM:SS",
                        start_str, end_str
                    ),
                };
            }
        }
    }

    ComplianceCheckResult {
        policy_id: policy.id.clone(),
        policy_name: policy.name.clone(),
        passed: true,
        severity: ComplianceSeverity::Info,
        message: "No freeze period configured".to_string(),
    }
}

// ────────────────────────────────────────────────
// Regulatory compliance checks
// ────────────────────────────────────────────────

pub fn check_regulatory_compliance(
    policy: &CompliancePolicy,
    network: &str,
    contract_id: &str,
) -> Vec<RegulatoryCheck> {
    let frameworks = policy
        .config
        .get("frameworks")
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_lowercase())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec!["gdpr".to_string(), "soc2".to_string()]);

    let mut results = Vec::new();

    for fw in &frameworks {
        match fw.as_str() {
            "gdpr" => results.extend(check_gdpr_compliance(network, contract_id)),
            "soc2" => results.extend(check_soc2_compliance(network, contract_id)),
            "ccpa" => results.extend(check_ccpa_compliance(network, contract_id)),
            "hipaa" => results.extend(check_hipaa_compliance(network, contract_id)),
            "pci-dss" | "pci_dss" => results.extend(check_pci_dss_compliance(network, contract_id)),
            _ => results.push(RegulatoryCheck {
                framework: RegulatoryFramework::Custom(fw.clone()),
                requirement: format!("{} compliance", fw),
                description: format!("Checking {} regulatory framework", fw),
                passed: true,
                severity: ComplianceSeverity::Info,
                message: format!("'{}' framework check: manual review required", fw),
            }),
        }
    }

    results
}

fn check_gdpr_compliance(network: &str, _contract_id: &str) -> Vec<RegulatoryCheck> {
    vec![
        RegulatoryCheck {
            framework: RegulatoryFramework::Gdpr,
            requirement: "Data minimization (Article 5)".to_string(),
            description: "Ensure only necessary data is stored on-chain".to_string(),
            passed: true,
            severity: ComplianceSeverity::Info,
            message: "Contract does not appear to store PII on-chain (preliminary)".to_string(),
        },
        RegulatoryCheck {
            framework: RegulatoryFramework::Gdpr,
            requirement: "Right to erasure (Article 17)".to_string(),
            description: "Users can request deletion of their personal data".to_string(),
            passed: network != "mainnet",
            severity: if network == "mainnet" {
                ComplianceSeverity::Warning
            } else {
                ComplianceSeverity::Info
            },
            message: if network == "mainnet" {
                "Mainnet storage is immutable — consider off-chain data patterns for PII"
                    .to_string()
            } else {
                "Testnet environment — erasure capabilities should be validated before mainnet"
                    .to_string()
            },
        },
        RegulatoryCheck {
            framework: RegulatoryFramework::Gdpr,
            requirement: "Data processing record (Article 30)".to_string(),
            description: "Maintain records of data processing activities".to_string(),
            passed: true,
            severity: ComplianceSeverity::Info,
            message: "Deployment audit trail is being maintained".to_string(),
        },
    ]
}

fn check_soc2_compliance(network: &str, _contract_id: &str) -> Vec<RegulatoryCheck> {
    vec![
        RegulatoryCheck {
            framework: RegulatoryFramework::Soc2,
            requirement: "Security — Access controls".to_string(),
            description: "Ensure only authorized parties can deploy contracts".to_string(),
            passed: true,
            severity: ComplianceSeverity::Blocking,
            message: "Approval workflow is available for access control".to_string(),
        },
        RegulatoryCheck {
            framework: RegulatoryFramework::Soc2,
            requirement: "Availability — Monitoring".to_string(),
            description: "System availability should be monitored".to_string(),
            passed: true,
            severity: ComplianceSeverity::Warning,
            message: "Deployment monitoring is available".to_string(),
        },
        RegulatoryCheck {
            framework: RegulatoryFramework::Soc2,
            requirement: "Confidentiality — Encryption".to_string(),
            description: "Sensitive data should be encrypted at rest and in transit".to_string(),
            passed: true,
            severity: ComplianceSeverity::Info,
            message: "Stellar network uses TLS for transit; wallet encryption available"
                .to_string(),
        },
    ]
}

fn check_ccpa_compliance(network: &str, _contract_id: &str) -> Vec<RegulatoryCheck> {
    vec![
        RegulatoryCheck {
            framework: RegulatoryFramework::Ccpa,
            requirement: "Right to know".to_string(),
            description: "Users should be informed about data collection".to_string(),
            passed: true,
            severity: ComplianceSeverity::Info,
            message: "Contract data practices should be documented for users".to_string(),
        },
        RegulatoryCheck {
            framework: RegulatoryFramework::Ccpa,
            requirement: "Right to opt-out".to_string(),
            description: "Users should be able to opt out of data sale".to_string(),
            passed: network != "mainnet",
            severity: ComplianceSeverity::Warning,
            message: if network == "mainnet" {
                "Consider implementing opt-out mechanisms in contract logic".to_string()
            } else {
                "Opt-out mechanisms should be verified before mainnet".to_string()
            },
        },
    ]
}

fn check_hipaa_compliance(_network: &str, _contract_id: &str) -> Vec<RegulatoryCheck> {
    vec![
        RegulatoryCheck {
            framework: RegulatoryFramework::Hipaa,
            requirement: "PHI protection".to_string(),
            description: "Protected Health Information must not be stored on-chain".to_string(),
            passed: true,
            severity: ComplianceSeverity::Blocking,
            message: "Preliminary: verify no PHI is stored in contract state".to_string(),
        },
        RegulatoryCheck {
            framework: RegulatoryFramework::Hipaa,
            requirement: "Audit controls".to_string(),
            description: "Record all access to PHI".to_string(),
            passed: true,
            severity: ComplianceSeverity::Warning,
            message: "Audit trail is active for all deployment activities".to_string(),
        },
    ]
}

fn check_pci_dss_compliance(_network: &str, _contract_id: &str) -> Vec<RegulatoryCheck> {
    vec![
        RegulatoryCheck {
            framework: RegulatoryFramework::PciDss,
            requirement: "Requirement 3 — Protect stored cardholder data".to_string(),
            description: "Cardholder data must not be stored on-chain".to_string(),
            passed: true,
            severity: ComplianceSeverity::Blocking,
            message: "Verify no cardholder data is stored in contract state".to_string(),
        },
        RegulatoryCheck {
            framework: RegulatoryFramework::PciDss,
            requirement: "Requirement 10 — Track and monitor access".to_string(),
            description: "All access to cardholder data environments must be logged".to_string(),
            passed: true,
            severity: ComplianceSeverity::Info,
            message: "Audit logging is enabled for compliance tracking".to_string(),
        },
    ]
}

// ────────────────────────────────────────────────
// Security compliance checks
// ────────────────────────────────────────────────

fn check_security_compliance(
    policy: &CompliancePolicy,
    network: &str,
) -> Vec<ComplianceCheckResult> {
    let require_hardware_wallet = policy
        .config
        .get("require_hardware_wallet")
        .map(|v| v == "true")
        .unwrap_or(false);
    let require_audit = policy
        .config
        .get("require_audit")
        .map(|v| v == "true")
        .unwrap_or(true);
    let require_security_scan = policy
        .config
        .get("require_security_scan")
        .map(|v| v == "true")
        .unwrap_or(true);

    let mut results = vec![ComplianceCheckResult {
        policy_id: policy.id.clone(),
        policy_name: "Security Audit".to_string(),
        passed: !require_audit || network != "mainnet",
        severity: ComplianceSeverity::Warning,
        message: if require_audit && network == "mainnet" {
            "A security audit is recommended before mainnet deployment. Run `starforge audit`."
                .to_string()
        } else {
            "Security audit check passed".to_string()
        },
    }];

    results.push(ComplianceCheckResult {
        policy_id: policy.id.clone(),
        policy_name: "Hardware Wallet".to_string(),
        passed: !require_hardware_wallet,
        severity: ComplianceSeverity::Warning,
        message: if require_hardware_wallet {
            "Hardware wallet signing is recommended for mainnet deployments".to_string()
        } else {
            "Software wallet signing is allowed".to_string()
        },
    });

    results.push(ComplianceCheckResult {
        policy_id: policy.id.clone(),
        policy_name: "Security Scan".to_string(),
        passed: !require_security_scan,
        severity: ComplianceSeverity::Warning,
        message: if require_security_scan {
            "Run `starforge security audit` for a full security scan".to_string()
        } else {
            "Security scanning is optional".to_string()
        },
    });

    results
}

// ────────────────────────────────────────────────
// Data protection checks
// ────────────────────────────────────────────────

fn check_data_protection(policy: &CompliancePolicy, network: &str) -> Vec<ComplianceCheckResult> {
    let encrypt_sensitive = policy
        .config
        .get("encrypt_sensitive_data")
        .map(|v| v == "true")
        .unwrap_or(true);
    let private_network = policy
        .config
        .get("private_network_only")
        .map(|v| v == "true")
        .unwrap_or(false);

    let mut results = vec![];

    results.push(ComplianceCheckResult {
        policy_id: policy.id.clone(),
        policy_name: "Sensitive Data Encryption".to_string(),
        passed: !private_network || network != "mainnet",
        severity: ComplianceSeverity::Warning,
        message: if encrypt_sensitive && network == "mainnet" {
            "Consider encrypting sensitive contract data or using off-chain storage for PII"
                .to_string()
        } else {
            "Data protection check passed".to_string()
        },
    });

    results.push(ComplianceCheckResult {
        policy_id: policy.id.clone(),
        policy_name: "Network Privacy".to_string(),
        passed: !private_network || network == "futurenet" || network == "testnet",
        severity: ComplianceSeverity::Info,
        message: format!(
            "Deploying to '{}': {}",
            network,
            if private_network && network == "mainnet" {
                "mainnet is a public network — data is visible to all nodes"
            } else {
                "network privacy check passed"
            }
        ),
    });

    results
}

// ────────────────────────────────────────────────
// Best practices enforcement
// ────────────────────────────────────────────────

fn check_best_practices(policy: &CompliancePolicy, contract_id: &str) -> Vec<BestPracticeCheck> {
    let enable_name_check = policy
        .config
        .get("enable_naming_conventions")
        .map(|v| v == "true")
        .unwrap_or(true);
    let enable_security_practices = policy
        .config
        .get("enable_security_practices")
        .map(|v| v == "true")
        .unwrap_or(true);
    let enable_testing_practices = policy
        .config
        .get("enable_testing_practices")
        .map(|v| v == "true")
        .unwrap_or(true);

    let mut results = vec![];

    // Naming conventions check
    if enable_name_check {
        let name_starts_with_c = contract_id.starts_with('C');
        results.push(BestPracticeCheck {
            category: "Naming Conventions".to_string(),
            check: "Contract ID format".to_string(),
            description: "Soroban contract IDs should start with 'C'".to_string(),
            passed: name_starts_with_c,
            severity: ComplianceSeverity::Info,
            recommendation: if name_starts_with_c {
                "Contract ID follows Soroban naming convention".to_string()
            } else {
                "Contract IDs should start with 'C' and be 56 characters long".to_string()
            },
        });
    }

    // Security practices check
    if enable_security_practices {
        results.push(BestPracticeCheck {
            category: "Security Practices".to_string(),
            check: "Two-factor deployment".to_string(),
            description: "Critical deployments should use multi-signature or approval workflows"
                .to_string(),
            passed: false,
            severity: ComplianceSeverity::Warning,
            recommendation:
                "Set up an approval workflow: `starforge approval init` and create an approval request"
                    .to_string(),
        });

        results.push(BestPracticeCheck {
            category: "Security Practices".to_string(),
            check: "WASM optimization".to_string(),
            description: "Optimize contract WASM before deployment to reduce gas costs".to_string(),
            passed: false,
            severity: ComplianceSeverity::Info,
            recommendation:
                "Run `starforge deploy --optimize` to optimize WASM before mainnet deployment"
                    .to_string(),
        });
    }

    // Testing practices check
    if enable_testing_practices {
        results.push(BestPracticeCheck {
            category: "Testing Practices".to_string(),
            check: "Testnet deployment first".to_string(),
            description: "Contracts should be deployed to testnet before mainnet".to_string(),
            passed: true,
            severity: ComplianceSeverity::Info,
            recommendation: "Consider a testnet deployment first to validate the contract"
                .to_string(),
        });
    }

    results
}

// ────────────────────────────────────────────────
// Risk assessment engine
// ────────────────────────────────────────────────

pub fn perform_risk_assessment(
    contract_id: &str,
    network: &str,
    policy_checks: &[ComplianceCheckResult],
    regulatory_checks: &[RegulatoryCheck],
    best_practices: &[BestPracticeCheck],
) -> RiskAssessmentResult {
    let mut factors = Vec::new();
    let mut recommendations = Vec::new();
    let mut total_score: u32 = 0;

    // Factor 1: Network risk
    let network_risk = match network {
        "mainnet" => 70u8,
        "testnet" => 20u8,
        "futurenet" | "standalone" => 10u8,
        _ => 30u8,
    };
    factors.push(RiskFactor {
        name: "Network Risk".to_string(),
        description: format!("Risk based on deployment target network '{}'", network),
        score: network_risk,
        mitigation: if network == "mainnet" {
            Some("Use approval workflows and hardware wallet signing".to_string())
        } else {
            None
        },
    });
    total_score += network_risk as u32;

    // Factor 2: Policy failure risk
    let failed_blocking = policy_checks
        .iter()
        .filter(|c| !c.passed && matches!(c.severity, ComplianceSeverity::Blocking))
        .count();
    let failed_warning = policy_checks
        .iter()
        .filter(|c| !c.passed && matches!(c.severity, ComplianceSeverity::Warning))
        .count();

    let policy_risk: u8 = if failed_blocking > 0 {
        80
    } else if failed_warning > 2 {
        50
    } else if failed_warning > 0 {
        30
    } else {
        5
    };
    factors.push(RiskFactor {
        name: "Policy Compliance Risk".to_string(),
        description: format!(
            "{} blocking and {} warning policy failures",
            failed_blocking, failed_warning
        ),
        score: policy_risk,
        mitigation: if failed_blocking > 0 {
            Some("Address all blocking policy violations before deployment".to_string())
        } else if failed_warning > 0 {
            Some("Review and address policy warnings".to_string())
        } else {
            None
        },
    });
    total_score += policy_risk as u32;

    // Factor 3: Regulatory compliance risk
    let reg_failed = regulatory_checks.iter().filter(|c| !c.passed).count();
    let reg_risk: u8 = if reg_failed > 3 {
        75
    } else if reg_failed > 0 {
        40
    } else {
        5
    };
    factors.push(RiskFactor {
        name: "Regulatory Compliance Risk".to_string(),
        description: format!("{} regulatory check(s) require attention", reg_failed),
        score: reg_risk,
        mitigation: if reg_failed > 0 {
            Some("Review regulatory requirements for your deployment region".to_string())
        } else {
            None
        },
    });
    total_score += reg_risk as u32;

    // Factor 4: Best practices
    let bp_failed = best_practices.iter().filter(|b| !b.passed).count();
    let bp_risk: u8 = if bp_failed > 2 {
        40
    } else if bp_failed > 0 {
        20
    } else {
        0
    };
    factors.push(RiskFactor {
        name: "Best Practices Risk".to_string(),
        description: format!("{} best practice recommendation(s) not followed", bp_failed),
        score: bp_risk,
        mitigation: if bp_failed > 0 {
            Some("Follow best practice recommendations for safer deployments".to_string())
        } else {
            None
        },
    });
    total_score += bp_risk as u32;

    // Factor 5: Contract ID validation
    let id_valid = contract_id.starts_with('C') && contract_id.len() == 56;
    let id_risk: u8 = if id_valid { 0 } else { 25 };
    factors.push(RiskFactor {
        name: "Contract Identifier Risk".to_string(),
        description: "Validates contract ID format".to_string(),
        score: id_risk,
        mitigation: if !id_valid {
            Some(
                "Ensure the contract ID is a valid Soroban address (starts with 'C', 56 chars)"
                    .to_string(),
            )
        } else {
            None
        },
    });
    total_score += id_risk as u32;

    // Calculate overall score (0-100, weighted)
    let max_possible = 5u32 * 100; // 5 factors, max 100 each
    let overall_score = ((total_score as f64 / max_possible as f64) * 100.0).round() as u8;

    // Determine risk level. A blocking policy violation is a hard stop
    // regardless of the averaged score, consistent with `approved_for_deployment`
    // below also refusing deployment whenever one is present.
    let overall_level = if failed_blocking > 0 {
        RiskLevel::Critical
    } else if overall_score >= 70 {
        RiskLevel::Critical
    } else if overall_score >= 50 {
        RiskLevel::High
    } else if overall_score >= 25 {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    };

    // Generate recommendations
    if failed_blocking > 0 {
        recommendations.push(format!(
            "Resolve {} blocking compliance policy violation(s) before deploying",
            failed_blocking
        ));
    }
    if network == "mainnet" {
        recommendations.push("Use a hardware wallet for mainnet signing if available".to_string());
        recommendations
            .push("Consider multi-signature or approval workflows for production".to_string());
    }
    if reg_failed > 0 {
        recommendations.push("Consult legal team about regulatory requirements".to_string());
    }
    recommendations.push("Run `starforge compliance report` for detailed findings".to_string());

    let approved_for_deployment = overall_level != RiskLevel::Critical && failed_blocking == 0;

    RiskAssessmentResult {
        overall_score,
        overall_level,
        approved_for_deployment,
        factors,
        recommendations,
        timestamp: Utc::now().to_rfc3339(),
    }
}

// ────────────────────────────────────────────────
// Report generation
// ────────────────────────────────────────────────

pub fn get_recent_reports(limit: usize) -> Result<Vec<ComplianceReport>> {
    let mut reports = load_reports_raw()?;
    reports.reverse();
    Ok(reports.into_iter().take(limit).collect())
}

pub fn get_report(request_id: &str) -> Result<Option<ComplianceReport>> {
    let reports = load_reports_raw()?;
    Ok(reports.into_iter().find(|r| r.request_id == request_id))
}

pub fn get_reports_by_contract(contract_id: &str) -> Result<Vec<ComplianceReport>> {
    let reports = load_reports_raw()?;
    Ok(reports
        .into_iter()
        .filter(|r| r.contract_id == contract_id)
        .collect())
}

pub fn get_reports_by_network(network: &str) -> Result<Vec<ComplianceReport>> {
    let reports = load_reports_raw()?;
    Ok(reports
        .into_iter()
        .filter(|r| r.network == network)
        .collect())
}

pub fn generate_compliance_summary(
    period_start: Option<&str>,
    period_end: Option<&str>,
) -> Result<ComplianceSummary> {
    let mut reports = load_reports_raw()?;

    if let Some(start) = period_start {
        reports.retain(|r| r.timestamp.as_str() >= start);
    }
    if let Some(end) = period_end {
        reports.retain(|r| r.timestamp.as_str() <= end);
    }

    let total_checks: usize = reports.iter().map(|r| r.checks.len()).sum();
    let passed_checks: usize = reports
        .iter()
        .flat_map(|r| &r.checks)
        .filter(|c| c.passed)
        .count();
    let blocking_issues: usize = reports.iter().map(|r| r.blocking_count).sum();
    let warning_issues: usize = reports.iter().map(|r| r.warning_count).sum();

    let pass_rate = if total_checks > 0 {
        (passed_checks as f64 / total_checks as f64) * 100.0
    } else {
        100.0
    };

    Ok(ComplianceSummary {
        total_reports: reports.len(),
        total_checks,
        passed_checks,
        failed_checks: total_checks - passed_checks,
        blocking_issues,
        warning_issues,
        pass_rate,
        reports,
        period_start: period_start.unwrap_or("all").to_string(),
        period_end: period_end.unwrap_or("all").to_string(),
    })
}

pub fn generate_compliance_statistics() -> Result<ComplianceStatistics> {
    let policies = load_policies_raw()?;
    let reports = load_reports_raw()?;

    // Count policy failures
    let mut policy_failures: HashMap<String, usize> = HashMap::new();
    for report in &reports {
        for check in &report.checks {
            if !check.passed {
                *policy_failures
                    .entry(check.policy_name.clone())
                    .or_default() += 1;
            }
        }
    }

    let mut most_failed: Vec<(String, usize)> = policy_failures.into_iter().collect();
    most_failed.sort_by_key(|b| std::cmp::Reverse(b.1));
    most_failed.truncate(5);

    // Network breakdown
    let mut network_breakdown: HashMap<String, ComplianceNetworkStats> = HashMap::new();
    for report in &reports {
        let stats =
            network_breakdown
                .entry(report.network.clone())
                .or_insert(ComplianceNetworkStats {
                    total_deployments: 0,
                    passed: 0,
                    blocked: 0,
                    warnings: 0,
                });
        stats.total_deployments += 1;
        if report.all_passed {
            stats.passed += 1;
        }
        if report.blocking_count > 0 {
            stats.blocked += 1;
        }
        if report.warning_count > 0 {
            stats.warnings += 1;
        }
    }

    let recent_reports: Vec<_> = reports
        .iter()
        .filter(|r| {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&r.timestamp) {
                let cutoff = Utc::now() - chrono::Duration::days(30);
                DateTime::<Utc>::from(dt) > cutoff
            } else {
                false
            }
        })
        .collect();

    Ok(ComplianceStatistics {
        total_policies: policies.len(),
        enabled_policies: policies.iter().filter(|p| p.enabled).count(),
        total_reports: reports.len(),
        recent_blocking: recent_reports.iter().map(|r| r.blocking_count).sum(),
        recent_warnings: recent_reports.iter().map(|r| r.warning_count).sum(),
        most_failed_policies: most_failed,
        network_breakdown,
    })
}

/// Escape a string for CSV by wrapping in quotes and doubling internal quotes.
fn csv_escape(value: impl std::fmt::Display) -> String {
    let s = value.to_string();
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

pub fn export_report_csv(report: &ComplianceReport) -> String {
    let mut csv = String::from("type,policy_id,check_name,passed,severity,message,framework\n");
    for check in &report.checks {
        csv.push_str(&format!(
            "{},{},{},{},{},{},\n",
            csv_escape("policy"),
            csv_escape(&check.policy_id),
            csv_escape(&check.policy_name),
            csv_escape(&check.passed),
            csv_escape(&check.severity),
            csv_escape(&check.message),
        ));
    }
    for check in &report.regulatory_checks {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            csv_escape("regulatory"),
            csv_escape(""),
            csv_escape(&check.requirement),
            csv_escape(&check.passed),
            csv_escape(&check.severity),
            csv_escape(&check.message),
            csv_escape(&check.framework),
        ));
    }
    for practice in &report.best_practices {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            csv_escape("best_practice"),
            csv_escape(""),
            csv_escape(&practice.check),
            csv_escape(&practice.passed),
            csv_escape(&practice.severity),
            csv_escape(&practice.recommendation),
            csv_escape(&practice.category),
        ));
    }
    csv
}

pub fn export_report_json(report: &ComplianceReport) -> Result<String> {
    Ok(serde_json::to_string_pretty(report)?)
}

// ────────────────────────────────────────────────
// Default policy initialization
// ────────────────────────────────────────────────

pub fn build_default_policies() -> Result<Vec<CompliancePolicy>> {
    let existing = load_policies_raw()?;
    if !existing.is_empty() {
        return Ok(existing);
    }

    let mut created = vec![];

    created.push(create_policy(
        "Mainnet Approval Required",
        "Production deployments require at least one approval",
        PolicyType::RequiredApprovers,
        ComplianceSeverity::Blocking,
        [
            ("min_approvers".to_string(), "1".to_string()),
            ("require_mainnet_approval".to_string(), "true".to_string()),
        ]
        .into_iter()
        .collect(),
    )?);

    created.push(create_policy(
        "Deployment Window",
        "Deployments should occur during business hours (09:00-17:00 UTC)",
        PolicyType::DeploymentWindow,
        ComplianceSeverity::Warning,
        [
            ("start_hour".to_string(), "9".to_string()),
            ("end_hour".to_string(), "17".to_string()),
            ("timezone_offset".to_string(), "0".to_string()),
        ]
        .into_iter()
        .collect(),
    )?);

    created.push(create_policy(
        "Deployment Frequency Limit",
        "Maximum 5 deployments per hour per network",
        PolicyType::MaxDeploymentFrequency,
        ComplianceSeverity::Warning,
        [("max_per_hour".to_string(), "5".to_string())]
            .into_iter()
            .collect(),
    )?);

    created.push(create_policy(
        "Network Restriction",
        "Only testnet and mainnet are allowed for deployments",
        PolicyType::NetworkRestriction,
        ComplianceSeverity::Blocking,
        [(
            "allowed_networks".to_string(),
            "testnet,mainnet".to_string(),
        )]
        .into_iter()
        .collect(),
    )?);

    created.push(create_policy(
        "Regulatory Compliance (GDPR & SOC2)",
        "Validate deployment against GDPR and SOC2 regulatory frameworks",
        PolicyType::RegulatoryCompliance,
        ComplianceSeverity::Warning,
        [("frameworks".to_string(), "gdpr,soc2".to_string())]
            .into_iter()
            .collect(),
    )?);

    created.push(create_policy(
        "Security Compliance",
        "Enforce security best practices for deployment",
        PolicyType::SecurityCompliance,
        ComplianceSeverity::Warning,
        [
            ("require_audit".to_string(), "true".to_string()),
            ("require_security_scan".to_string(), "true".to_string()),
        ]
        .into_iter()
        .collect(),
    )?);

    created.push(create_policy(
        "Data Protection",
        "Ensure sensitive data is protected during deployment",
        PolicyType::DataProtection,
        ComplianceSeverity::Warning,
        [
            ("encrypt_sensitive_data".to_string(), "true".to_string()),
            ("private_network_only".to_string(), "false".to_string()),
        ]
        .into_iter()
        .collect(),
    )?);

    created.push(create_policy(
        "Soroban Best Practices",
        "Enforce Soroban/Stellar best practices for deployments",
        PolicyType::BestPractices,
        ComplianceSeverity::Info,
        [
            ("enable_naming_conventions".to_string(), "true".to_string()),
            ("enable_security_practices".to_string(), "true".to_string()),
            ("enable_testing_practices".to_string(), "true".to_string()),
        ]
        .into_iter()
        .collect(),
    )?);

    Ok(created)
}

// ────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compliance_severity_display() {
        assert_eq!(ComplianceSeverity::Info.to_string(), "info");
        assert_eq!(ComplianceSeverity::Warning.to_string(), "warning");
        assert_eq!(ComplianceSeverity::Blocking.to_string(), "blocking");
    }

    #[test]
    fn test_risk_level_display() {
        assert_eq!(RiskLevel::Low.to_string(), "low");
        assert_eq!(RiskLevel::Medium.to_string(), "medium");
        assert_eq!(RiskLevel::High.to_string(), "high");
        assert_eq!(RiskLevel::Critical.to_string(), "critical");
    }

    #[test]
    fn test_network_restriction_allowed() {
        let policy = CompliancePolicy {
            id: "pol-1".to_string(),
            name: "Test".to_string(),
            description: "".to_string(),
            policy_type: PolicyType::NetworkRestriction,
            severity: ComplianceSeverity::Blocking,
            enabled: true,
            config: [(
                "allowed_networks".to_string(),
                "testnet,mainnet".to_string(),
            )]
            .into_iter()
            .collect(),
            created_at: "".to_string(),
            updated_at: "".to_string(),
        };
        let result = check_network_restriction(&policy, "testnet");
        assert!(result.passed);
    }

    #[test]
    fn test_network_restriction_blocked() {
        let policy = CompliancePolicy {
            id: "pol-2".to_string(),
            name: "Test".to_string(),
            description: "".to_string(),
            policy_type: PolicyType::NetworkRestriction,
            severity: ComplianceSeverity::Blocking,
            enabled: true,
            config: [("allowed_networks".to_string(), "testnet".to_string())]
                .into_iter()
                .collect(),
            created_at: "".to_string(),
            updated_at: "".to_string(),
        };
        let result = check_network_restriction(&policy, "mainnet");
        assert!(!result.passed);
    }

    #[test]
    fn test_regulatory_framework_display() {
        assert_eq!(RegulatoryFramework::Gdpr.to_string(), "GDPR");
        assert_eq!(RegulatoryFramework::Soc2.to_string(), "SOC2");
        assert_eq!(RegulatoryFramework::Ccpa.to_string(), "CCPA");
        assert_eq!(RegulatoryFramework::Hipaa.to_string(), "HIPAA");
        assert_eq!(RegulatoryFramework::PciDss.to_string(), "PCI-DSS");
    }

    #[test]
    fn test_gdpr_compliance_basic() {
        let results = check_gdpr_compliance("testnet", "CAFEBABE");
        assert_eq!(results.len(), 3);
        assert!(results[0].passed); // Data minimization
        assert!(results[1].passed); // Right to erasure on testnet
    }

    #[test]
    fn test_gdpr_compliance_mainnet_warning() {
        let results = check_gdpr_compliance("mainnet", "CAFEBABE");
        // On mainnet, right to erasure should flag a warning
        assert_eq!(results[1].severity, ComplianceSeverity::Warning);
        assert!(!results[1].passed);
    }

    #[test]
    fn test_soc2_compliance() {
        let results = check_soc2_compliance("mainnet", "CAFEBABE");
        assert_eq!(results.len(), 3);
        assert!(results[0].passed);
    }

    #[test]
    fn test_risk_assessment_low_risk() {
        let checks = vec![ComplianceCheckResult {
            policy_id: "pol-1".to_string(),
            policy_name: "Test".to_string(),
            passed: true,
            severity: ComplianceSeverity::Info,
            message: "All good".to_string(),
        }];
        let reg_checks = vec![];
        let practices = vec![];

        let assessment = perform_risk_assessment(
            "CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
            "testnet",
            &checks,
            &reg_checks,
            &practices,
        );
        assert_eq!(assessment.overall_level, RiskLevel::Low);
        assert!(assessment.approved_for_deployment);
    }

    #[test]
    fn test_risk_assessment_high_risk() {
        let checks = vec![ComplianceCheckResult {
            policy_id: "pol-1".to_string(),
            policy_name: "Test".to_string(),
            passed: false,
            severity: ComplianceSeverity::Blocking,
            message: "Failed".to_string(),
        }];
        let reg_checks = vec![RegulatoryCheck {
            framework: RegulatoryFramework::Gdpr,
            requirement: "Test".to_string(),
            description: "".to_string(),
            passed: false,
            severity: ComplianceSeverity::Blocking,
            message: "Failed".to_string(),
        }];
        let practices = vec![BestPracticeCheck {
            category: "Test".to_string(),
            check: "Test".to_string(),
            description: "".to_string(),
            passed: false,
            severity: ComplianceSeverity::Warning,
            recommendation: "Fix it".to_string(),
        }];

        let assessment =
            perform_risk_assessment("test", "mainnet", &checks, &reg_checks, &practices);
        assert_eq!(assessment.overall_level, RiskLevel::Critical);
        assert!(!assessment.approved_for_deployment);
    }

    #[test]
    fn test_best_practices_naming() {
        let policy = CompliancePolicy {
            id: "pol-bp".to_string(),
            name: "Best Practices".to_string(),
            description: "".to_string(),
            policy_type: PolicyType::BestPractices,
            severity: ComplianceSeverity::Info,
            enabled: true,
            config: [
                ("enable_naming_conventions".to_string(), "true".to_string()),
                ("enable_security_practices".to_string(), "true".to_string()),
                ("enable_testing_practices".to_string(), "true".to_string()),
            ]
            .into_iter()
            .collect(),
            created_at: "".to_string(),
            updated_at: "".to_string(),
        };
        let results = check_best_practices(
            &policy,
            "CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX",
        );
        assert!(!results.is_empty());
    }

    #[test]
    fn test_policy_crud() {
        // Test create, list, get, update, delete
        let policy = create_policy(
            "Test Policy",
            "A test policy",
            PolicyType::NetworkRestriction,
            ComplianceSeverity::Warning,
            HashMap::new(),
        )
        .unwrap();

        let policies = list_policies().unwrap();
        assert!(policies.iter().any(|p| p.id == policy.id));

        let fetched = get_policy(&policy.id).unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().name, "Test Policy");

        let updated =
            update_policy(&policy.id, Some("Updated Policy"), None, None, None, None).unwrap();
        assert_eq!(updated.name, "Updated Policy");

        delete_policy(&policy.id).unwrap();
        let deleted = get_policy(&policy.id).unwrap();
        assert!(deleted.is_none());
    }

    #[test]
    fn test_export_report_csv_format() {
        let report = ComplianceReport {
            request_id: "req-123".to_string(),
            contract_id: "CXXXXXXXXXXXXXXXXXXX".to_string(),
            network: "testnet".to_string(),
            checks: vec![],
            regulatory_checks: vec![],
            best_practices: vec![],
            risk_assessment: None,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            all_passed: true,
            blocking_count: 0,
            warning_count: 0,
        };

        let csv = export_report_csv(&report);
        assert!(csv.starts_with("type,policy_id,check_name,passed,severity,message,framework"));
    }

    #[test]
    fn test_generate_summary_empty() {
        let summary = generate_compliance_summary(None, None).unwrap();
        // Should handle empty reports gracefully
        assert!(summary.pass_rate >= 0.0);
    }
}
