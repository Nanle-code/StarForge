use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::utils::config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataClassification {
    pub level: DataSensitivity,
    pub description: String,
    pub encryption_required: bool,
    pub access_control_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DataSensitivity {
    Public,
    Internal,
    Confidential,
    Restricted,
}

impl DataSensitivity {
    pub fn as_str(&self) -> &'static str {
        match self {
            DataSensitivity::Public => "public",
            DataSensitivity::Internal => "internal",
            DataSensitivity::Confidential => "confidential",
            DataSensitivity::Restricted => "restricted",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionPolicy {
    pub algorithm: String,
    pub key_size: u32,
    pub rotation_days: u32,
    pub at_rest: bool,
    pub in_transit: bool,
}

impl Default for EncryptionPolicy {
    fn default() -> Self {
        Self {
            algorithm: "AES-256-GCM".into(),
            key_size: 256,
            rotation_days: 90,
            at_rest: true,
            in_transit: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRule {
    pub principal: String,
    pub resource: String,
    pub permissions: Vec<String>,
    pub conditions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessPolicy {
    pub rules: Vec<AccessRule>,
    pub default_deny: bool,
}

impl Default for AccessPolicy {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            default_deny: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRecord {
    pub id: String,
    pub algorithm: String,
    pub created_at: String,
    pub rotated_at: Option<String>,
    pub expires_at: Option<String>,
    pub status: KeyStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum KeyStatus {
    Active,
    Rotated,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProtectionResult {
    pub contract_id: String,
    pub timestamp: String,
    pub checks: Vec<DataProtectionCheck>,
    pub score: f64,
    pub summary: DataProtectionSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProtectionCheck {
    pub id: String,
    pub category: String,
    pub title: String,
    pub passed: bool,
    pub severity: String,
    pub details: String,
    pub remediation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProtectionSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub encryption_score: f64,
    pub access_control_score: f64,
    pub key_management_score: f64,
    pub integrity_score: f64,
}

// Fields not currently read from any code path in this crate. Kept rather
// than removed since deleting them is a product decision, not a
// lint-scoping one.
#[allow(dead_code)]
pub struct DataProtectionEngine {
    encryption_policy: EncryptionPolicy,
    access_policy: AccessPolicy,
    keys: Vec<KeyRecord>,
    classifications: HashMap<String, DataClassification>,
}

impl Default for DataProtectionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DataProtectionEngine {
    pub fn new() -> Self {
        let mut classifications = HashMap::new();
        classifications.insert(
            "wallet_key".into(),
            DataClassification {
                level: DataSensitivity::Restricted,
                description: "Wallet private keys and mnemonics".into(),
                encryption_required: true,
                access_control_required: true,
            },
        );
        classifications.insert(
            "user_data".into(),
            DataClassification {
                level: DataSensitivity::Confidential,
                description: "User personal information".into(),
                encryption_required: true,
                access_control_required: true,
            },
        );
        classifications.insert(
            "config".into(),
            DataClassification {
                level: DataSensitivity::Internal,
                description: "Application configuration".into(),
                encryption_required: false,
                access_control_required: true,
            },
        );
        classifications.insert(
            "logs".into(),
            DataClassification {
                level: DataSensitivity::Internal,
                description: "Application logs".into(),
                encryption_required: false,
                access_control_required: false,
            },
        );

        Self {
            encryption_policy: EncryptionPolicy::default(),
            access_policy: AccessPolicy::default(),
            keys: Vec::new(),
            classifications,
        }
    }

    pub fn check_protection(&self, contract_path: &PathBuf) -> Result<DataProtectionResult> {
        let source = fs::read_to_string(contract_path)
            .context("Failed to read contract source for data protection check")?;

        let contract_id = contract_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut checks = Vec::new();
        let source_lower = source.to_lowercase();

        checks.push(self.check_encryption_at_rest(&source_lower));
        checks.push(self.check_encryption_in_transit(&source_lower));
        checks.push(self.check_access_control(&source_lower));
        checks.push(self.check_key_management(&source_lower));
        checks.push(self.check_data_validation(&source_lower));
        checks.push(self.check_data_integrity(&source_lower));
        checks.push(self.check_secure_storage(&source_lower));
        checks.push(self.check_data_loss_prevention(&source_lower));

        let total = checks.len();
        let passed = checks.iter().filter(|c| c.passed).count();
        let failed = total - passed;
        let score = if total > 0 {
            (passed as f64 / total as f64) * 100.0
        } else {
            100.0
        };

        let encryption_checks: Vec<&DataProtectionCheck> = checks
            .iter()
            .filter(|c| c.category == "Encryption")
            .collect();
        let encryption_score = if encryption_checks.is_empty() {
            100.0
        } else {
            encryption_checks.iter().filter(|c| c.passed).count() as f64
                / encryption_checks.len() as f64
                * 100.0
        };

        let access_checks: Vec<&DataProtectionCheck> = checks
            .iter()
            .filter(|c| c.category == "Access Control")
            .collect();
        let access_control_score = if access_checks.is_empty() {
            100.0
        } else {
            access_checks.iter().filter(|c| c.passed).count() as f64 / access_checks.len() as f64
                * 100.0
        };

        let key_checks: Vec<&DataProtectionCheck> = checks
            .iter()
            .filter(|c| c.category == "Key Management")
            .collect();
        let key_management_score = if key_checks.is_empty() {
            100.0
        } else {
            key_checks.iter().filter(|c| c.passed).count() as f64 / key_checks.len() as f64 * 100.0
        };

        let integrity_checks: Vec<&DataProtectionCheck> = checks
            .iter()
            .filter(|c| c.category == "Data Integrity")
            .collect();
        let integrity_score = if integrity_checks.is_empty() {
            100.0
        } else {
            integrity_checks.iter().filter(|c| c.passed).count() as f64
                / integrity_checks.len() as f64
                * 100.0
        };

        Ok(DataProtectionResult {
            contract_id,
            timestamp: Utc::now().to_rfc3339(),
            checks,
            score,
            summary: DataProtectionSummary {
                total,
                passed,
                failed,
                encryption_score,
                access_control_score,
                key_management_score,
                integrity_score,
            },
        })
    }

    fn check_encryption_at_rest(&self, source: &str) -> DataProtectionCheck {
        let has_encryption = source.contains("encrypt")
            || source.contains("cipher")
            || source.contains("aes")
            || source.contains("aes_gcm");
        let has_sensitive_storage =
            source.contains("store") || source.contains("save") || source.contains("write");

        let passed = has_encryption || !has_sensitive_storage;
        DataProtectionCheck {
            id: "encryption-at-rest".into(),
            category: "Encryption".into(),
            title: "Encryption at rest".into(),
            passed,
            severity: if has_sensitive_storage && !has_encryption {
                "critical"
            } else {
                "high"
            }
            .into(),
            details: if passed {
                "Contract implements encryption for stored data".into()
            } else {
                "Contract stores data without encryption".into()
            },
            remediation: "Use AES-256-GCM encryption for sensitive stored data".into(),
        }
    }

    fn check_encryption_in_transit(&self, source: &str) -> DataProtectionCheck {
        let has_tls = source.contains("https") || source.contains("tls") || source.contains("ssl");
        let has_http = source.contains("http://") && !source.contains("https");

        let passed = has_tls || !has_http;
        DataProtectionCheck {
            id: "encryption-in-transit".into(),
            category: "Encryption".into(),
            title: "Encryption in transit".into(),
            passed,
            severity: if has_http { "high" } else { "medium" }.into(),
            details: if passed {
                "All network communications use encryption".into()
            } else {
                "Unencrypted HTTP communication detected".into()
            },
            remediation: "Use HTTPS/TLS for all network communications".into(),
        }
    }

    fn check_access_control(&self, source: &str) -> DataProtectionCheck {
        let has_auth = source.contains("require_auth")
            || source.contains("check_auth")
            || source.contains("has_role")
            || source.contains("is_admin")
            || source.contains("authorize");

        DataProtectionCheck {
            id: "access-control".into(),
            category: "Access Control".into(),
            title: "Granular access control".into(),
            passed: has_auth,
            severity: "critical".into(),
            details: if has_auth {
                "Contract implements access control checks".into()
            } else {
                "No access control mechanisms detected".into()
            },
            remediation: "Add require_auth or role-based access checks to sensitive functions"
                .into(),
        }
    }

    fn check_key_management(&self, source: &str) -> DataProtectionCheck {
        let _has_key_ops =
            source.contains("key") || source.contains("secret") || source.contains("private");
        let has_hardcoded = source.contains("\"sk1\"")
            || source.contains("\"secret_key\"")
            || source.contains("hardcoded");

        let passed = !has_hardcoded;
        DataProtectionCheck {
            id: "key-management".into(),
            category: "Key Management".into(),
            title: "Secure key management".into(),
            passed,
            severity: if has_hardcoded { "critical" } else { "medium" }.into(),
            details: if passed {
                "No hardcoded secrets detected".into()
            } else {
                "Hardcoded secret keys found in source".into()
            },
            remediation:
                "Use environment variables or secure key storage instead of hardcoded values".into(),
        }
    }

    fn check_data_validation(&self, source: &str) -> DataProtectionCheck {
        let has_validation = source.contains("validate")
            || source.contains("check")
            || source.contains("verify")
            || source.contains("assert");

        DataProtectionCheck {
            id: "data-validation".into(),
            category: "Data Integrity".into(),
            title: "Input data validation".into(),
            passed: has_validation,
            severity: "high".into(),
            details: if has_validation {
                "Contract validates input data".into()
            } else {
                "No input validation detected".into()
            },
            remediation: "Add input validation for all user-provided data".into(),
        }
    }

    fn check_data_integrity(&self, source: &str) -> DataProtectionCheck {
        let has_integrity = source.contains("hash")
            || source.contains("checksum")
            || source.contains("verify")
            || source.contains("integrity");

        DataProtectionCheck {
            id: "data-integrity".into(),
            category: "Data Integrity".into(),
            title: "Data integrity verification".into(),
            passed: has_integrity,
            severity: "medium".into(),
            details: if has_integrity {
                "Contract includes integrity checks".into()
            } else {
                "No data integrity verification detected".into()
            },
            remediation: "Add hash-based integrity checks for critical data".into(),
        }
    }

    fn check_secure_storage(&self, source: &str) -> DataProtectionCheck {
        let has_storage =
            source.contains("storage") || source.contains("persist") || source.contains("save");
        let has_secure =
            source.contains("encrypt") || source.contains("secure") || source.contains("protected");

        let passed = !has_storage || has_secure;
        DataProtectionCheck {
            id: "secure-storage".into(),
            category: "Encryption".into(),
            title: "Secure storage implementation".into(),
            passed,
            severity: "high".into(),
            details: if passed {
                "Storage implementation uses security measures".into()
            } else {
                "Storage without security measures detected".into()
            },
            remediation: "Encrypt sensitive data before storage".into(),
        }
    }

    fn check_data_loss_prevention(&self, source: &str) -> DataProtectionCheck {
        let has_dlp = source.contains("mask")
            || source.contains("redact")
            || source.contains("sanitize")
            || source.contains("safe_log");

        DataProtectionCheck {
            id: "data-loss-prevention".into(),
            category: "Data Integrity".into(),
            title: "Data loss prevention".into(),
            passed: has_dlp,
            severity: "medium".into(),
            details: if has_dlp {
                "Contract implements DLP measures".into()
            } else {
                "No DLP measures detected".into()
            },
            remediation: "Add data masking and sanitization for sensitive outputs".into(),
        }
    }

    pub fn save_result(&self, result: &DataProtectionResult) -> Result<PathBuf> {
        let dir = config::config_dir()
            .join("security")
            .join("data-protection");
        fs::create_dir_all(&dir)?;

        let filename = format!(
            "protection-{}-{}.json",
            result.contract_id,
            Utc::now().format("%Y%m%d-%H%M%S")
        );
        let path = dir.join(&filename);
        let json = serde_json::to_string_pretty(result)?;
        fs::write(&path, &json).context("Failed to save data protection result")?;
        Ok(path)
    }
}

pub fn format_data_protection_report(result: &DataProtectionResult) -> String {
    let mut out = String::new();

    out.push_str(&format!("Data Protection Report: {}\n", result.contract_id));
    out.push_str(&format!("Timestamp: {}\n", result.timestamp));
    out.push_str(&format!("Score: {:.1}%\n\n", result.score));

    out.push_str("Category Scores:\n");
    out.push_str(&format!(
        "  Encryption:         {:.1}%\n",
        result.summary.encryption_score
    ));
    out.push_str(&format!(
        "  Access Control:     {:.1}%\n",
        result.summary.access_control_score
    ));
    out.push_str(&format!(
        "  Key Management:     {:.1}%\n",
        result.summary.key_management_score
    ));
    out.push_str(&format!(
        "  Data Integrity:     {:.1}%\n\n",
        result.summary.integrity_score
    ));

    out.push_str(&format!(
        "Results: {}/{} passed\n\n",
        result.summary.passed, result.summary.total
    ));

    for check in &result.checks {
        let status = if check.passed { "PASS" } else { "FAIL" };
        out.push_str(&format!(
            "  [{}] {} — {}\n",
            status, check.title, check.severity
        ));
        if !check.passed {
            out.push_str(&format!("    {}\n", check.details));
            out.push_str(&format!("    Remediation: {}\n", check.remediation));
        }
    }

    out
}
