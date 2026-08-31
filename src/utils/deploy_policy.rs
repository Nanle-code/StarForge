//! Organization deploy policy-as-code: networks, reviewers, and checklists.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_POLICY_FILENAMES: &[&str] = &[
    "starforge-deploy-policy.toml",
    "starforge-deploy-policy.yaml",
    "starforge-deploy-policy.yml",
];

/// Comma-separated approver identities recorded at deploy time.
pub const ENV_DEPLOY_APPROVERS: &str = "STARFORGE_DEPLOY_APPROVERS";

/// Comma-separated checklist item ids satisfied for this deploy.
pub const ENV_DEPLOY_CHECKLIST: &str = "STARFORGE_DEPLOY_CHECKLIST";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequiredReviewer {
    pub username: String,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChecklistItem {
    pub id: String,
    pub description: String,
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DeployPolicy {
    pub organization: Option<String>,
    pub allowed_networks: Vec<String>,
    pub required_reviewers: Vec<RequiredReviewer>,
    pub checklist: Vec<ChecklistItem>,
    /// When true, `deploy --execute` is required before a real deploy proceeds.
    pub require_execute_flag: bool,
}

impl Default for DeployPolicy {
    fn default() -> Self {
        Self {
            organization: None,
            allowed_networks: vec!["testnet".into(), "mainnet".into()],
            required_reviewers: Vec::new(),
            checklist: Vec::new(),
            require_execute_flag: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyViolation {
    pub rule: String,
    pub message: String,
    pub remediation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyReport {
    pub passed: bool,
    pub organization: Option<String>,
    pub policy_path: PathBuf,
    pub network: String,
    pub violations: Vec<PolicyViolation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployContext {
    pub network: String,
    pub execute: bool,
    pub approvers: Vec<String>,
    pub completed_checklist: Vec<String>,
}

impl DeployContext {
    pub fn from_env(network: &str, execute: bool) -> Self {
        Self {
            network: network.to_string(),
            execute,
            approvers: parse_csv_env(ENV_DEPLOY_APPROVERS),
            completed_checklist: parse_csv_env(ENV_DEPLOY_CHECKLIST),
        }
    }

    pub fn with_overrides(
        mut self,
        approvers: Option<Vec<String>>,
        checklist: Option<Vec<String>>,
    ) -> Self {
        if let Some(values) = approvers {
            self.approvers = values;
        }
        if let Some(values) = checklist {
            self.completed_checklist = values;
        }
        self
    }
}

fn parse_csv_env(key: &str) -> Vec<String> {
    std::env::var(key)
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn discover_policy_file(start: &Path) -> Option<PathBuf> {
    for name in DEFAULT_POLICY_FILENAMES {
        let candidate = start.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn load_policy(path: &Path) -> Result<DeployPolicy> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read deploy policy {}", path.display()))?;
    parse_policy_str(&content, path)
}

pub fn parse_policy_str(content: &str, source: &Path) -> Result<DeployPolicy> {
    let ext = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let policy = match ext.as_str() {
        "toml" => toml::from_str(content)
            .with_context(|| format!("Invalid deploy policy TOML in {}", source.display()))?,
        "yaml" | "yml" => serde_yaml::from_str(content)
            .with_context(|| format!("Invalid deploy policy YAML in {}", source.display()))?,
        _ => toml::from_str(content).or_else(|_| {
            serde_yaml::from_str(content).context("Deploy policy must be valid TOML or YAML")
        })?,
    };
    Ok(policy)
}

pub fn write_default_policy(path: &Path) -> Result<()> {
    if path.exists() {
        anyhow::bail!(
            "Deploy policy configuration already exists: {}",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let policy = DeployPolicy {
        organization: Some("example-org".into()),
        allowed_networks: vec!["testnet".into()],
        required_reviewers: vec![RequiredReviewer {
            username: "security-lead".into(),
            role: Some("security".into()),
        }],
        checklist: vec![
            ChecklistItem {
                id: "audit-passed".into(),
                description: "Run `starforge audit` with no critical findings".into(),
                required: true,
            },
            ChecklistItem {
                id: "changelog-updated".into(),
                description: "Update CHANGELOG for this release".into(),
                required: true,
            },
        ],
        require_execute_flag: true,
    };

    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("toml")
        .to_ascii_lowercase();

    let serialized = match ext.as_str() {
        "yaml" | "yml" => serde_yaml::to_string(&policy)?,
        _ => toml::to_string_pretty(&policy)?,
    };
    fs::write(path, serialized)?;
    Ok(())
}

pub fn evaluate(policy_path: &Path, policy: &DeployPolicy, context: &DeployContext) -> PolicyReport {
    let mut violations = Vec::new();

    if !policy.allowed_networks.is_empty()
        && !policy
            .allowed_networks
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(&context.network))
    {
        violations.push(PolicyViolation {
            rule: "allowed_networks".into(),
            message: format!(
                "Network '{}' is not allowed by organization policy",
                context.network
            ),
            remediation: format!(
                "Deploy to one of: {} or update the policy file",
                policy.allowed_networks.join(", ")
            ),
        });
    }

    if policy.require_execute_flag && !context.execute {
        violations.push(PolicyViolation {
            rule: "require_execute_flag".into(),
            message: "Policy requires `--execute` for real deployments".into(),
            remediation: "Re-run with --execute after completing the checklist".into(),
        });
    }

    let approver_set: HashSet<String> = context
        .approvers
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect();

    for reviewer in &policy.required_reviewers {
        if !approver_set.contains(&reviewer.username.to_ascii_lowercase()) {
            let role = reviewer
                .role
                .as_deref()
                .map(|value| format!(" ({value})"))
                .unwrap_or_default();
            violations.push(PolicyViolation {
                rule: "required_reviewers".into(),
                message: format!(
                    "Missing required reviewer '{}{}'",
                    reviewer.username, role
                ),
                remediation: format!(
                    "Set {} to a comma-separated list including '{}'",
                    ENV_DEPLOY_APPROVERS, reviewer.username
                ),
            });
        }
    }

    let completed: HashSet<String> = context
        .completed_checklist
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect();

    for item in policy.checklist.iter().filter(|item| item.required) {
        if !completed.contains(&item.id.to_ascii_lowercase()) {
            violations.push(PolicyViolation {
                rule: "checklist".into(),
                message: format!("Required checklist item '{}' not satisfied", item.id),
                remediation: format!(
                    "Complete '{}' ({}) and record it via {} or deploy --checklist",
                    item.id, item.description, ENV_DEPLOY_CHECKLIST
                ),
            });
        }
    }

    PolicyReport {
        passed: violations.is_empty(),
        organization: policy.organization.clone(),
        policy_path: policy_path.to_path_buf(),
        network: context.network.clone(),
        violations,
    }
}

pub fn enforce(policy_path: &Path, policy: &DeployPolicy, context: &DeployContext) -> Result<()> {
    let report = evaluate(policy_path, policy, context);
    if report.passed {
        tracing::info!(
            deploy_policy = %policy_path.display(),
            deploy_policy_org = report.organization.as_deref().unwrap_or("unknown"),
            deploy_policy_network = %report.network,
            "deploy policy satisfied"
        );
        return Ok(());
    }

    for violation in &report.violations {
        tracing::warn!(
            deploy_policy_rule = %violation.rule,
            deploy_policy_network = %report.network,
            "deploy policy violation"
        );
    }

    let details = report
        .violations
        .iter()
        .map(|v| format!("- [{}] {} — {}", v.rule, v.message, v.remediation))
        .collect::<Vec<_>>()
        .join("\n");

    anyhow::bail!(
        "Deploy blocked by policy ({}):\n{}",
        policy_path.display(),
        details
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_violations_block_deploy() {
        let policy = DeployPolicy {
            organization: Some("acme".into()),
            allowed_networks: vec!["testnet".into()],
            required_reviewers: vec![RequiredReviewer {
                username: "alice".into(),
                role: Some("security".into()),
            }],
            checklist: vec![ChecklistItem {
                id: "audit-passed".into(),
                description: "Audit complete".into(),
                required: true,
            }],
            require_execute_flag: true,
        };
        let context = DeployContext {
            network: "mainnet".into(),
            execute: false,
            approvers: vec![],
            completed_checklist: vec![],
        };
        let report = evaluate(Path::new("policy.toml"), &policy, &context);
        assert!(!report.passed);
        assert!(report.violations.iter().any(|v| v.rule == "allowed_networks"));
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule == "require_execute_flag"));
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule == "required_reviewers"));
        assert!(report.violations.iter().any(|v| v.rule == "checklist"));
    }

    #[test]
    fn satisfied_policy_passes() {
        let policy = DeployPolicy {
            allowed_networks: vec!["testnet".into()],
            required_reviewers: vec![RequiredReviewer {
                username: "alice".into(),
                role: None,
            }],
            checklist: vec![ChecklistItem {
                id: "audit-passed".into(),
                description: "Audit complete".into(),
                required: true,
            }],
            require_execute_flag: true,
            ..DeployPolicy::default()
        };
        let context = DeployContext {
            network: "testnet".into(),
            execute: true,
            approvers: vec!["alice".into()],
            completed_checklist: vec!["audit-passed".into()],
        };
        let report = evaluate(Path::new("policy.toml"), &policy, &context);
        assert!(report.passed);
    }

    #[test]
    fn toml_and_yaml_parse_equivalent_policy() {
        let toml = r#"
organization = "acme"
allowed_networks = ["testnet"]
require_execute_flag = true

[[required_reviewers]]
username = "alice"
role = "security"

[[checklist]]
id = "audit-passed"
description = "Audit complete"
required = true
"#;
        let yaml = r#"
organization: acme
allowed_networks:
  - testnet
require_execute_flag: true
required_reviewers:
  - username: alice
    role: security
checklist:
  - id: audit-passed
    description: Audit complete
    required: true
"#;
        let from_toml = parse_policy_str(toml, Path::new("policy.toml")).unwrap();
        let from_yaml = parse_policy_str(yaml, Path::new("policy.yaml")).unwrap();
        assert_eq!(from_toml, from_yaml);
    }

    #[test]
    fn default_policy_round_trips() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("starforge-deploy-policy.toml");
        write_default_policy(&path).unwrap();
        let loaded = load_policy(&path).unwrap();
        assert_eq!(loaded.organization.as_deref(), Some("example-org"));
        assert!(loaded.require_execute_flag);
    }

    #[test]
    fn enforce_returns_error_on_violation() {
        let policy = DeployPolicy {
            allowed_networks: vec!["testnet".into()],
            ..DeployPolicy::default()
        };
        let context = DeployContext {
            network: "mainnet".into(),
            execute: true,
            approvers: vec![],
            completed_checklist: vec![],
        };
        let err = enforce(Path::new("policy.toml"), &policy, &context).unwrap_err();
        assert!(err.to_string().contains("allowed_networks"));
    }
}
