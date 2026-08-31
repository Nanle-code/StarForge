use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::utils::config;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ComplianceStandard {
    GDPR,
    SOC2,
    HIPAA,
    ISO27001,
    Custom,
}

impl ComplianceStandard {
    pub fn as_str(&self) -> &'static str {
        match self {
            ComplianceStandard::GDPR => "GDPR",
            ComplianceStandard::SOC2 => "SOC2",
            ComplianceStandard::HIPAA => "HIPAA",
            ComplianceStandard::ISO27001 => "ISO27001",
            ComplianceStandard::Custom => "Custom",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            ComplianceStandard::GDPR => "General Data Protection Regulation",
            ComplianceStandard::SOC2 => "Service Organization Control 2",
            ComplianceStandard::HIPAA => "Health Insurance Portability and Accountability Act",
            ComplianceStandard::ISO27001 => "Information Security Management",
            ComplianceStandard::Custom => "Custom compliance standard",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceRule {
    pub id: String,
    pub standard: ComplianceStandard,
    pub category: String,
    pub title: String,
    pub description: String,
    pub severity: String,
    pub check_fn: String,
    pub remediation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceCheckResult {
    pub rule_id: String,
    pub standard: String,
    pub category: String,
    pub title: String,
    pub passed: bool,
    pub severity: String,
    pub details: String,
    pub remediation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    pub contract_id: String,
    pub timestamp: String,
    pub standards_checked: Vec<String>,
    pub total_rules: usize,
    pub passed: usize,
    pub failed: usize,
    pub score: f64,
    pub results: Vec<ComplianceCheckResult>,
    pub risk_assessment: RiskAssessment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub overall_risk: String,
    pub critical_gaps: Vec<String>,
    pub recommendations: Vec<String>,
}

pub struct ComplianceEngine {
    rules: Vec<ComplianceRule>,
}

impl Default for ComplianceEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ComplianceEngine {
    pub fn new() -> Self {
        let rules = vec![
            ComplianceRule {
                id: "gdpr-data-minimization".into(),
                standard: ComplianceStandard::GDPR,
                category: "Data Minimization".into(),
                title: "Data minimization principle".into(),
                description: "Ensure only necessary data is collected and stored".into(),
                severity: "high".into(),
                check_fn: "check_data_minimization".into(),
                remediation: "Review data collection to ensure only required fields are stored"
                    .into(),
            },
            ComplianceRule {
                id: "gdpr-right-to-erasure".into(),
                standard: ComplianceStandard::GDPR,
                category: "Data Subject Rights".into(),
                title: "Right to erasure support".into(),
                description: "Provide mechanism for data deletion requests".into(),
                severity: "high".into(),
                check_fn: "check_deletion_mechanism".into(),
                remediation: "Implement data deletion endpoints and key rotation".into(),
            },
            ComplianceRule {
                id: "gdpr-consent".into(),
                standard: ComplianceStandard::GDPR,
                category: "Consent".into(),
                title: "Consent management".into(),
                description: "Obtain and record user consent for data processing".into(),
                severity: "medium".into(),
                check_fn: "check_consent".into(),
                remediation: "Add consent recording to contract state".into(),
            },
            ComplianceRule {
                id: "soc2-access-control".into(),
                standard: ComplianceStandard::SOC2,
                category: "Access Control".into(),
                title: "Role-based access control".into(),
                description: "Implement role-based access for contract operations".into(),
                severity: "critical".into(),
                check_fn: "check_rbac".into(),
                remediation: "Add role checks to admin functions using require_auth".into(),
            },
            ComplianceRule {
                id: "soc2-audit-logging".into(),
                standard: ComplianceStandard::SOC2,
                category: "Audit Trail".into(),
                title: "Audit logging".into(),
                description: "Record all significant operations for audit purposes".into(),
                severity: "high".into(),
                check_fn: "check_audit_logging".into(),
                remediation: "Emit events for all state-changing operations".into(),
            },
            ComplianceRule {
                id: "soc2-encryption".into(),
                standard: ComplianceStandard::SOC2,
                category: "Encryption".into(),
                title: "Data encryption at rest".into(),
                description: "Ensure sensitive data is encrypted when stored".into(),
                severity: "critical".into(),
                check_fn: "check_encryption".into(),
                remediation: "Use encrypted storage for sensitive contract state".into(),
            },
            ComplianceRule {
                id: "hipaa-phi-protection".into(),
                standard: ComplianceStandard::HIPAA,
                category: "PHI Protection".into(),
                title: "Protected Health Information safeguards".into(),
                description: "Ensure PHI is properly encrypted and access-controlled".into(),
                severity: "critical".into(),
                check_fn: "check_phi_protection".into(),
                remediation: "Implement encryption and access controls for health data".into(),
            },
            ComplianceRule {
                id: "hipaa-audit-trail".into(),
                standard: ComplianceStandard::HIPAA,
                category: "Audit Trail".into(),
                title: "Comprehensive audit trail".into(),
                description: "Maintain detailed logs of all PHI access".into(),
                severity: "high".into(),
                check_fn: "check_hipaa_audit".into(),
                remediation: "Log all access to health-related contract state".into(),
            },
            ComplianceRule {
                id: "iso27001-risk-assessment".into(),
                standard: ComplianceStandard::ISO27001,
                category: "Risk Management".into(),
                title: "Risk assessment documentation".into(),
                description: "Document and assess security risks".into(),
                severity: "medium".into(),
                check_fn: "check_risk_doc".into(),
                remediation: "Create risk assessment document for the contract".into(),
            },
            ComplianceRule {
                id: "iso27001-access-review".into(),
                standard: ComplianceStandard::ISO27001,
                category: "Access Management".into(),
                title: "Regular access review".into(),
                description: "Implement mechanism for periodic access review".into(),
                severity: "medium".into(),
                check_fn: "check_access_review".into(),
                remediation: "Add access review functions and scheduling".into(),
            },
        ];

        Self { rules }
    }

    pub fn check_compliance(
        &self,
        contract_path: &PathBuf,
        standards: &[ComplianceStandard],
    ) -> Result<ComplianceReport> {
        let source = fs::read_to_string(contract_path)
            .context("Failed to read contract source for compliance check")?;

        let contract_id = contract_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut results = Vec::new();

        let filtered_rules: Vec<&ComplianceRule> = self
            .rules
            .iter()
            .filter(|r| standards.is_empty() || standards.contains(&r.standard))
            .collect();

        for rule in &filtered_rules {
            let passed = self.evaluate_rule(rule, &source);
            let details = if passed {
                format!("Check passed: {}", rule.title)
            } else {
                format!("Check failed: {}", rule.description)
            };

            results.push(ComplianceCheckResult {
                rule_id: rule.id.clone(),
                standard: rule.standard.as_str().to_string(),
                category: rule.category.clone(),
                title: rule.title.clone(),
                passed,
                severity: rule.severity.clone(),
                details,
                remediation: rule.remediation.clone(),
            });
        }

        let total = results.len();
        let passed_count = results.iter().filter(|r| r.passed).count();
        let failed_count = total - passed_count;
        let score = if total > 0 {
            (passed_count as f64 / total as f64) * 100.0
        } else {
            100.0
        };

        let standards_checked: Vec<String> =
            standards.iter().map(|s| s.as_str().to_string()).collect();

        let critical_gaps: Vec<String> = results
            .iter()
            .filter(|r| !r.passed && r.severity == "critical")
            .map(|r| format!("[{}] {}", r.standard, r.title))
            .collect();

        let recommendations: Vec<String> = results
            .iter()
            .filter(|r| !r.passed)
            .map(|r| r.remediation.clone())
            .collect();

        let overall_risk = if !critical_gaps.is_empty() {
            "critical"
        } else if failed_count > total / 2 {
            "high"
        } else if failed_count > 0 {
            "medium"
        } else {
            "low"
        };

        let report = ComplianceReport {
            contract_id,
            timestamp: Utc::now().to_rfc3339(),
            standards_checked,
            total_rules: total,
            passed: passed_count,
            failed: failed_count,
            score,
            results,
            risk_assessment: RiskAssessment {
                overall_risk: overall_risk.to_string(),
                critical_gaps,
                recommendations,
            },
        };

        Ok(report)
    }

    fn evaluate_rule(&self, rule: &ComplianceRule, source: &str) -> bool {
        let source_lower = source.to_lowercase();

        match rule.id.as_str() {
            "gdpr-data-minimization" => {
                let has_minimize = source_lower.contains("minimize")
                    || source_lower.contains("necessary")
                    || source_lower.contains("data_length");
                let has_excessive = source_lower.contains("collect_all")
                    || source_lower.contains("store_everything");
                has_minimize && !has_excessive
            }
            "gdpr-right-to-erasure" => {
                source_lower.contains("delete")
                    || source_lower.contains("remove")
                    || source_lower.contains("erase")
                    || source_lower.contains("destroy")
            }
            "gdpr-consent" => {
                source_lower.contains("consent")
                    || source_lower.contains("approve")
                    || source_lower.contains("authorize")
            }
            "soc2-access-control" => {
                source_lower.contains("require_auth")
                    || source_lower.contains("check_auth")
                    || source_lower.contains("has_role")
                    || source_lower.contains("is_admin")
                    || source_lower.contains("authorize")
            }
            "soc2-audit-logging" => {
                source_lower.contains("event")
                    || source_lower.contains("log")
                    || source_lower.contains("emit")
                    || source_lower.contains("audit")
            }
            "soc2-encryption" => {
                source_lower.contains("encrypt")
                    || source_lower.contains("cipher")
                    || source_lower.contains("aes")
                    || source_lower.contains("hash")
            }
            "hipaa-phi-protection" => {
                source_lower.contains("encrypt")
                    || source_lower.contains("protect")
                    || source_lower.contains("secure")
                    || source_lower.contains("safe")
            }
            "hipaa-audit-trail" => {
                source_lower.contains("log")
                    || source_lower.contains("audit")
                    || source_lower.contains("trace")
                    || source_lower.contains("record")
            }
            "iso27001-risk-assessment" => {
                source_lower.contains("risk")
                    || source_lower.contains("assess")
                    || source_lower.contains("threat")
                    || source_lower.contains("vulnerability")
            }
            "iso27001-access-review" => {
                source_lower.contains("review")
                    || source_lower.contains("audit")
                    || source_lower.contains("check_access")
                    || source_lower.contains("validate_access")
            }
            _ => true,
        }
    }

    pub fn save_report(&self, report: &ComplianceReport) -> Result<PathBuf> {
        let dir = config::config_dir().join("security").join("compliance");
        fs::create_dir_all(&dir)?;

        let filename = format!(
            "compliance-{}-{}.json",
            report.contract_id,
            Utc::now().format("%Y%m%d-%H%M%S")
        );
        let path = dir.join(&filename);
        let json = serde_json::to_string_pretty(report)?;
        fs::write(&path, &json).context("Failed to save compliance report")?;
        Ok(path)
    }

    pub fn list_reports() -> Result<Vec<PathBuf>> {
        let dir = config::config_dir().join("security").join("compliance");
        if !dir.exists() {
            return Ok(vec![]);
        }

        let mut reports: Vec<PathBuf> = fs::read_dir(&dir)
            .context("Failed to list compliance reports")?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
            .collect();

        reports.sort();
        reports.reverse();
        Ok(reports)
    }
}

pub fn format_compliance_report(report: &ComplianceReport) -> String {
    let mut out = String::new();

    out.push_str(&format!("Compliance Report: {}\n", report.contract_id));
    out.push_str(&format!("Timestamp: {}\n", report.timestamp));
    out.push_str(&format!(
        "Standards: {}\n",
        report.standards_checked.join(", ")
    ));
    out.push_str(&format!(
        "Score: {:.1}% ({}/{} passed)\n",
        report.score, report.passed, report.total_rules
    ));
    out.push_str(&format!("Risk: {}\n", report.risk_assessment.overall_risk));

    if !report.risk_assessment.critical_gaps.is_empty() {
        out.push_str("\nCritical Gaps:\n");
        for gap in &report.risk_assessment.critical_gaps {
            out.push_str(&format!("  - {}\n", gap));
        }
    }

    out.push_str("\nResults:\n");
    for r in &report.results {
        let status = if r.passed { "PASS" } else { "FAIL" };
        out.push_str(&format!(
            "  [{}] [{}] {} — {}\n",
            status, r.standard, r.title, r.severity
        ));
        if !r.passed {
            out.push_str(&format!("    Remediation: {}\n", r.remediation));
        }
    }

    out
}
