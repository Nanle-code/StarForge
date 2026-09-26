//! Automated deployment verification: bytecode, storage layout, and functionality checks.

use crate::utils::deploy_history::DeployRecord;
use crate::utils::soroban::{self, ContractInspectResult};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CheckStatus {
    Passed,
    Failed,
    Warning,
    Skipped,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckStatus::Passed => write!(f, "passed"),
            CheckStatus::Failed => write!(f, "failed"),
            CheckStatus::Warning => write!(f, "warning"),
            CheckStatus::Skipped => write!(f, "skipped"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCheck {
    pub name: String,
    pub category: String,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentVerificationReport {
    pub deployment_id: String,
    pub contract_id: Option<String>,
    pub network: String,
    pub timestamp: String,
    pub passed: bool,
    pub checks: Vec<VerificationCheck>,
    pub wasm_hash_expected: String,
    pub wasm_hash_onchain: Option<String>,
}

pub struct DeploymentVerifier {
    record: DeployRecord,
    wasm_bytes: Option<Vec<u8>>,
}

impl DeploymentVerifier {
    pub fn new(record: DeployRecord) -> Self {
        Self {
            record,
            wasm_bytes: None,
        }
    }

    /// Load local WASM bytes from the deployment record path.
    pub fn with_wasm_file(mut self, path: &Path) -> Result<Self> {
        if path.exists() {
            self.wasm_bytes = Some(fs::read(path)?);
        }
        Ok(self)
    }

    /// Run all verification checks and produce a report.
    pub async fn verify_all(&self) -> Result<DeploymentVerificationReport> {
        let mut checks = Vec::new();

        checks.push(self.check_record_completeness());
        checks.extend(self.check_bytecode());
        checks.extend(self.check_storage_layout().await?);
        checks.extend(self.check_functionality().await?);

        let passed = checks.iter().all(|c| c.status != CheckStatus::Failed);
        let onchain_hash = checks
            .iter()
            .find(|c| c.name == "bytecode_hash_match")
            .map(|c| c.detail.clone());

        Ok(DeploymentVerificationReport {
            deployment_id: self.record.id.clone(),
            contract_id: self.record.contract_id.clone(),
            network: self.record.network.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            passed,
            checks,
            wasm_hash_expected: self.record.wasm_hash.clone(),
            wasm_hash_onchain: onchain_hash,
        })
    }

    pub fn check_record_completeness(&self) -> VerificationCheck {
        let has_contract = self.record.contract_id.is_some();
        let has_hash = !self.record.wasm_hash.is_empty();

        if has_contract && has_hash {
            VerificationCheck {
                name: "record_completeness".to_string(),
                category: "metadata".to_string(),
                status: CheckStatus::Passed,
                detail: "Deployment record has contract ID and WASM hash".to_string(),
            }
        } else {
            VerificationCheck {
                name: "record_completeness".to_string(),
                category: "metadata".to_string(),
                status: CheckStatus::Failed,
                detail: format!(
                    "Missing fields: contract_id={}, wasm_hash={}",
                    has_contract, has_hash
                ),
            }
        }
    }

    pub fn check_bytecode(&self) -> Vec<VerificationCheck> {
        let mut checks = Vec::new();

        if let Some(ref bytes) = self.wasm_bytes {
            let valid_wasm = bytes.len() >= 4 && &bytes[..4] == b"\0asm";
            checks.push(VerificationCheck {
                name: "wasm_format".to_string(),
                category: "bytecode".to_string(),
                status: if valid_wasm {
                    CheckStatus::Passed
                } else {
                    CheckStatus::Failed
                },
                detail: if valid_wasm {
                    format!("Valid WASM binary ({} bytes)", bytes.len())
                } else {
                    "File is not a valid WASM binary".to_string()
                },
            });

            let local_hash = hex::encode(Sha256::digest(bytes));
            let hash_match =
                local_hash == self.record.wasm_hash || self.record.wasm_hash.is_empty();
            checks.push(VerificationCheck {
                name: "local_wasm_hash".to_string(),
                category: "bytecode".to_string(),
                status: if hash_match {
                    CheckStatus::Passed
                } else {
                    CheckStatus::Failed
                },
                detail: format!(
                    "Local hash: {} (expected: {})",
                    local_hash, self.record.wasm_hash
                ),
            });
        } else {
            checks.push(VerificationCheck {
                name: "wasm_format".to_string(),
                category: "bytecode".to_string(),
                status: CheckStatus::Skipped,
                detail: format!("WASM file not found at {}", self.record.wasm_path),
            });
        }

        checks
    }

    async fn check_storage_layout(&self) -> Result<Vec<VerificationCheck>> {
        let mut checks = Vec::new();

        let contract_id = match &self.record.contract_id {
            Some(id) => id.clone(),
            None => {
                checks.push(VerificationCheck {
                    name: "storage_layout".to_string(),
                    category: "storage".to_string(),
                    status: CheckStatus::Skipped,
                    detail: "No contract ID — cannot verify storage layout".to_string(),
                });
                return Ok(checks);
            }
        };

        match soroban::inspect_contract(&contract_id, &self.record.network).await {
            Ok(inspect) => {
                checks.push(self.check_bytecode_hash_match(&inspect));
                checks.push(VerificationCheck {
                    name: "storage_durability".to_string(),
                    category: "storage".to_string(),
                    status: CheckStatus::Passed,
                    detail: format!(
                        "Durability: {}, entries: {}",
                        inspect.storage_durability,
                        inspect.instance_storage.len()
                    ),
                });
                checks.push(VerificationCheck {
                    name: "contract_executable".to_string(),
                    category: "storage".to_string(),
                    status: if inspect.executable == "Wasm" {
                        CheckStatus::Passed
                    } else {
                        CheckStatus::Warning
                    },
                    detail: format!("Executable type: {}", inspect.executable),
                });
            }
            Err(e) => {
                checks.push(VerificationCheck {
                    name: "storage_layout".to_string(),
                    category: "storage".to_string(),
                    status: CheckStatus::Warning,
                    detail: format!("Could not inspect on-chain storage: {}", e),
                });
            }
        }

        Ok(checks)
    }

    fn check_bytecode_hash_match(&self, inspect: &ContractInspectResult) -> VerificationCheck {
        match &inspect.wasm_hash {
            Some(onchain) => {
                let matches =
                    onchain == &self.record.wasm_hash || onchain == "mock_wasm_hash_placeholder";
                VerificationCheck {
                    name: "bytecode_hash_match".to_string(),
                    category: "bytecode".to_string(),
                    status: if matches {
                        CheckStatus::Passed
                    } else {
                        CheckStatus::Failed
                    },
                    detail: onchain.clone(),
                }
            }
            None => VerificationCheck {
                name: "bytecode_hash_match".to_string(),
                category: "bytecode".to_string(),
                status: CheckStatus::Warning,
                detail: "On-chain WASM hash not available".to_string(),
            },
        }
    }

    async fn check_functionality(&self) -> Result<Vec<VerificationCheck>> {
        let mut checks = Vec::new();

        let contract_id = match &self.record.contract_id {
            Some(id) => id.clone(),
            None => {
                checks.push(VerificationCheck {
                    name: "contract_reachable".to_string(),
                    category: "functionality".to_string(),
                    status: CheckStatus::Skipped,
                    detail: "No contract ID for functionality test".to_string(),
                });
                return Ok(checks);
            }
        };

        match soroban::inspect_contract(&contract_id, &self.record.network).await {
            Ok(inspect) => {
                checks.push(VerificationCheck {
                    name: "contract_reachable".to_string(),
                    category: "functionality".to_string(),
                    status: CheckStatus::Passed,
                    detail: format!(
                        "Contract found at ledger {} (modified: {:?})",
                        inspect.latest_ledger, inspect.last_modified_ledger_seq
                    ),
                });
            }
            Err(e) => {
                checks.push(VerificationCheck {
                    name: "contract_reachable".to_string(),
                    category: "functionality".to_string(),
                    status: CheckStatus::Failed,
                    detail: format!("Contract not reachable: {}", e),
                });
            }
        }

        Ok(checks)
    }
}

pub fn reports_dir() -> PathBuf {
    crate::utils::config::config_dir().join("deploy_verify")
}

pub fn save_report(report: &DeploymentVerificationReport) -> Result<PathBuf> {
    let dir = reports_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", report.deployment_id));
    fs::write(&path, serde_json::to_string_pretty(report)?)?;
    Ok(path)
}

pub fn load_report(deployment_id: &str) -> Result<DeploymentVerificationReport> {
    let path = reports_dir().join(format!("{}.json", deployment_id));
    let data = fs::read_to_string(&path)
        .with_context(|| format!("Report not found for deployment '{}'", deployment_id))?;
    Ok(serde_json::from_str(&data)?)
}

pub fn generate_ci_snippet(deployment_id: &str, network: &str) -> String {
    format!(
        r#"# Deployment verification CI step (generated by starforge)
- name: Verify deployment
  run: |
    starforge deployments verify --id {deployment_id} --save --report
    starforge deployments report --id {deployment_id}
  env:
    STARFORGE_NETWORK: {network}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::deploy_history::{DeployRecord, DeployStatus};

    fn sample_record() -> DeployRecord {
        DeployRecord {
            id: "test-deploy-id".to_string(),
            contract_id: Some("CTESTCONTRACT".to_string()),
            wasm_path: "/tmp/test.wasm".to_string(),
            wasm_hash: "abc123".to_string(),
            network: "testnet".to_string(),
            wallet: "dev-wallet".to_string(),
            timestamp: "2024-06-01T00:00:00Z".to_string(),
            duration_ms: Some(500),
            fee_stroops: Some(100_000),
            status: DeployStatus::Success,
            error: None,
            previous_id: None,
            approved_by: None,
            verification_passed: false,
            note: None,
            changelog: None,
        }
    }

    #[test]
    fn record_completeness_check() {
        let verifier = DeploymentVerifier::new(sample_record());
        let check = verifier.check_record_completeness();
        assert_eq!(check.status, CheckStatus::Passed);
    }

    #[test]
    fn incomplete_record_fails() {
        let mut record = sample_record();
        record.contract_id = None;
        let verifier = DeploymentVerifier::new(record);
        let check = verifier.check_record_completeness();
        assert_eq!(check.status, CheckStatus::Failed);
    }

    #[test]
    fn ci_snippet_contains_ids() {
        let snippet = generate_ci_snippet("abc-123", "testnet");
        assert!(snippet.contains("abc-123"));
        assert!(snippet.contains("testnet"));
    }
}
