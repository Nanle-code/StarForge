//! AI Project Planning Assistant (issue #517).
//!
//! Helps plan Soroban projects with requirement analysis, architecture
//! suggestions, task breakdown, timeline estimation, resource planning,
//! and risk identification.

use crate::utils::config;
use crate::utils::ollama::{self, GenerateOptions};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ─── Core plan structures ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRequirements {
    pub summary: String,
    pub functional_requirements: Vec<String>,
    pub non_functional_requirements: Vec<String>,
    pub constraints: Vec<String>,
    pub stakeholders: Vec<String>,
    pub success_criteria: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureSuggestion {
    pub name: String,
    pub description: String,
    pub contract_modules: Vec<ContractModule>,
    pub storage_strategy: String,
    pub auth_model: String,
    pub upgrade_strategy: String,
    pub pros: Vec<String>,
    pub cons: Vec<String>,
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractModule {
    pub name: String,
    pub responsibility: String,
    pub interfaces: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevelopmentPhase {
    pub name: String,
    pub order: u32,
    pub description: String,
    pub deliverables: Vec<String>,
    pub dependencies: Vec<String>,
    pub estimated_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskItem {
    pub id: String,
    pub title: String,
    pub description: String,
    pub phase: String,
    pub priority: TaskPriority,
    pub effort_points: u32,
    pub assignee_role: String,
    pub dependencies: Vec<String>,
    pub acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEstimate {
    pub total_days: u32,
    pub start_date: DateTime<Utc>,
    pub target_completion: DateTime<Utc>,
    pub milestones: Vec<Milestone>,
    pub critical_path: Vec<String>,
    pub buffer_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub name: String,
    pub date: DateTime<Utc>,
    pub deliverables: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePlan {
    pub team_size: u32,
    pub roles: Vec<TeamRole>,
    pub allocation: Vec<ResourceAllocation>,
    pub skills_required: Vec<String>,
    pub tooling: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamRole {
    pub role: String,
    pub count: u32,
    pub responsibilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAllocation {
    pub role: String,
    pub phase: String,
    pub allocation_pct: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRisk {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: RiskCategory,
    pub severity: RiskSeverity,
    pub likelihood: RiskLikelihood,
    pub mitigation: String,
    pub contingency: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskCategory {
    Technical,
    Security,
    Schedule,
    Resource,
    Compliance,
    Operational,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLikelihood {
    Unlikely,
    Possible,
    Likely,
    AlmostCertain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestingStrategy {
    pub unit_tests: String,
    pub integration_tests: String,
    pub property_tests: String,
    pub security_tests: String,
    pub testnet_validation: String,
    pub coverage_target_pct: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentPlan {
    pub environments: Vec<String>,
    pub pre_deploy_checks: Vec<String>,
    pub deployment_steps: Vec<String>,
    pub rollback_procedure: String,
    pub monitoring_setup: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPlan {
    pub project_name: String,
    pub generated_at: DateTime<Utc>,
    pub requirements: ProjectRequirements,
    pub architectures: Vec<ArchitectureSuggestion>,
    pub phases: Vec<DevelopmentPhase>,
    pub tasks: Vec<TaskItem>,
    pub timeline: TimelineEstimate,
    pub resources: ResourcePlan,
    pub risks: Vec<ProjectRisk>,
    pub testing_strategy: TestingStrategy,
    pub deployment_plan: DeploymentPlan,
    pub ai_summary: Option<String>,
}

// ─── Static analysis (no LLM) ────────────────────────────────────────────────

pub fn analyze_requirements(description: &str) -> ProjectRequirements {
    let lower = description.to_lowercase();
    let mut functional = Vec::new();
    let mut non_functional = Vec::new();
    let mut constraints = Vec::new();

    if lower.contains("token") || lower.contains("mint") || lower.contains("transfer") {
        functional.push("Token lifecycle management (mint, transfer, burn)".into());
        functional.push("Balance tracking and authorization".into());
    }
    if lower.contains("nft") || lower.contains("metadata") {
        functional.push("NFT minting with metadata storage".into());
        functional.push("Ownership transfer and enumeration".into());
    }
    if lower.contains("governance") || lower.contains("vote") || lower.contains("dao") {
        functional.push("Proposal creation and voting mechanism".into());
        functional.push("Quorum and execution thresholds".into());
    }
    if lower.contains("escrow") || lower.contains("payment") {
        functional.push("Conditional fund release".into());
        functional.push("Dispute resolution hooks".into());
    }
    if lower.contains("staking") || lower.contains("reward") {
        functional.push("Stake/unstake with reward accrual".into());
        functional.push("Reward distribution logic".into());
    }
    if lower.contains("allowlist") || lower.contains("access control") {
        functional.push("Role-based or allowlist access control".into());
    }

    if functional.is_empty() {
        functional.push("Core contract business logic as described".into());
        functional.push("State initialization and admin functions".into());
    }

    if lower.contains("gas") || lower.contains("optim") {
        non_functional.push("Gas-efficient storage and computation".into());
    }
    if lower.contains("upgrade") {
        non_functional.push("Upgradeable contract architecture".into());
    }
    non_functional.push("Comprehensive test coverage (>80%)".into());
    non_functional.push("Security audit before mainnet deployment".into());

    if lower.contains("testnet") {
        constraints.push("Initial deployment on Stellar testnet".into());
    }
    if lower.contains("mainnet") {
        constraints.push("Production mainnet deployment required".into());
    }
    constraints.push("Soroban SDK compatibility".into());
    constraints.push("Stellar network fee budget constraints".into());

    ProjectRequirements {
        summary: summarize_description(description),
        functional_requirements: functional,
        non_functional_requirements: non_functional,
        constraints,
        stakeholders: vec![
            "Smart contract developers".into(),
            "Product owner".into(),
            "Security reviewer".into(),
        ],
        success_criteria: vec![
            "All acceptance tests pass on testnet".into(),
            "Security audit findings resolved".into(),
            "Gas costs within budget".into(),
            "Documentation complete".into(),
        ],
    }
}

fn summarize_description(description: &str) -> String {
    let trimmed = description.trim();
    if trimmed.len() <= 200 {
        trimmed.to_string()
    } else {
        format!("{}…", &trimmed[..197])
    }
}

pub fn suggest_architectures(description: &str) -> Vec<ArchitectureSuggestion> {
    let lower = description.to_lowercase();
    let mut architectures = Vec::new();

    architectures.push(ArchitectureSuggestion {
        name: "Monolithic Contract".into(),
        description: "Single Soroban contract containing all business logic.".into(),
        contract_modules: vec![ContractModule {
            name: "main".into(),
            responsibility: "All contract logic and storage".into(),
            interfaces: vec!["initialize".into(), "execute".into(), "admin".into()],
        }],
        storage_strategy: "Persistent instance storage with typed keys".into(),
        auth_model: "require_auth on privileged functions".into(),
        upgrade_strategy: "Immutable or admin-controlled migration".into(),
        pros: vec![
            "Simple deployment and testing".into(),
            "Lower cross-contract call overhead".into(),
        ],
        cons: vec![
            "Harder to upgrade individual components".into(),
            "Contract size limits may apply".into(),
        ],
        recommended: !lower.contains("multi") && !lower.contains("modular"),
    });

    if lower.contains("token")
        || lower.contains("governance")
        || lower.contains("multi")
        || lower.contains("modular")
    {
        architectures.push(ArchitectureSuggestion {
            name: "Modular Multi-Contract".into(),
            description: "Separate contracts for distinct domains with cross-contract calls."
                .into(),
            contract_modules: vec![
                ContractModule {
                    name: "core".into(),
                    responsibility: "Shared types and registry".into(),
                    interfaces: vec!["register".into(), "lookup".into()],
                },
                ContractModule {
                    name: "logic".into(),
                    responsibility: "Primary business logic".into(),
                    interfaces: vec!["execute".into(), "query".into()],
                },
                ContractModule {
                    name: "admin".into(),
                    responsibility: "Governance and upgrades".into(),
                    interfaces: vec!["propose".into(), "execute_upgrade".into()],
                },
            ],
            storage_strategy: "Domain-specific storage per contract".into(),
            auth_model: "Cross-contract auth with admin proxy".into(),
            upgrade_strategy: "Per-module upgrade with governance timelock".into(),
            pros: vec![
                "Independent module upgrades".into(),
                "Clear separation of concerns".into(),
                "Reusable components".into(),
            ],
            cons: vec![
                "Higher deployment complexity".into(),
                "Cross-contract call gas costs".into(),
            ],
            recommended: true,
        });
    }

    architectures
}

pub fn breakdown_tasks(description: &str, phases: &[DevelopmentPhase]) -> Vec<TaskItem> {
    let reqs = analyze_requirements(description);
    let mut tasks = Vec::new();
    let mut id = 1;

    let phase_names: Vec<String> = if phases.is_empty() {
        default_phases(description)
            .iter()
            .map(|p| p.name.clone())
            .collect()
    } else {
        phases.iter().map(|p| p.name.clone()).collect()
    };

    let scaffold_phase = phase_names
        .first()
        .cloned()
        .unwrap_or_else(|| "Setup".into());

    tasks.push(TaskItem {
        id: format!("T-{id:03}"),
        title: "Initialize Soroban project scaffold".into(),
        description: "Create project with starforge new, configure Cargo.toml".into(),
        phase: scaffold_phase.clone(),
        priority: TaskPriority::High,
        effort_points: 2,
        assignee_role: "Developer".into(),
        dependencies: vec![],
        acceptance_criteria: vec!["Project compiles".into(), "CI configured".into()],
    });
    id += 1;

    for req in &reqs.functional_requirements {
        tasks.push(TaskItem {
            id: format!("T-{id:03}"),
            title: format!("Implement: {}", req),
            description: format!("Build and test functionality for: {}", req),
            phase: phase_names
                .get(1)
                .cloned()
                .unwrap_or_else(|| "Development".into()),
            priority: TaskPriority::High,
            effort_points: 5,
            assignee_role: "Developer".into(),
            dependencies: vec!["T-001".into()],
            acceptance_criteria: vec!["Unit tests pass".into(), "Function documented".into()],
        });
        id += 1;
    }

    tasks.push(TaskItem {
        id: format!("T-{id:03}"),
        title: "Write integration tests".into(),
        description: "End-to-end tests on local/testnet environment".into(),
        phase: phase_names
            .get(2)
            .cloned()
            .unwrap_or_else(|| "Testing".into()),
        priority: TaskPriority::High,
        effort_points: 5,
        assignee_role: "QA Engineer".into(),
        dependencies: vec!["T-002".into()],
        acceptance_criteria: vec![">80% coverage".into(), "All scenarios covered".into()],
    });
    id += 1;

    let audit_deps = format!("T-{:03}", id - 1);
    tasks.push(TaskItem {
        id: format!("T-{id:03}"),
        title: "Security audit".into(),
        description: "Run starforge ai audit and resolve findings".into(),
        phase: phase_names
            .get(2)
            .cloned()
            .unwrap_or_else(|| "Testing".into()),
        priority: TaskPriority::Critical,
        effort_points: 3,
        assignee_role: "Security Reviewer".into(),
        dependencies: vec![audit_deps],
        acceptance_criteria: vec![
            "No critical findings".into(),
            "High findings mitigated".into(),
        ],
    });
    id += 1;

    let deploy_deps = format!("T-{:03}", id - 1);
    tasks.push(TaskItem {
        id: format!("T-{id:03}"),
        title: "Testnet deployment".into(),
        description: "Deploy to Stellar testnet and validate".into(),
        phase: phase_names
            .last()
            .cloned()
            .unwrap_or_else(|| "Deployment".into()),
        priority: TaskPriority::High,
        effort_points: 3,
        assignee_role: "DevOps".into(),
        dependencies: vec![deploy_deps],
        acceptance_criteria: vec!["Contract deployed".into(), "Smoke tests pass".into()],
    });

    tasks
}

pub fn default_phases(description: &str) -> Vec<DevelopmentPhase> {
    let lower = description.to_lowercase();
    let complexity_days = if lower.contains("complex") || lower.contains("multi") {
        14
    } else {
        7
    };

    vec![
        DevelopmentPhase {
            name: "Discovery & Design".into(),
            order: 1,
            description: "Requirements refinement, architecture design, spike work".into(),
            deliverables: vec![
                "Requirements document".into(),
                "Architecture decision record".into(),
            ],
            dependencies: vec![],
            estimated_days: 3,
        },
        DevelopmentPhase {
            name: "Core Development".into(),
            order: 2,
            description: "Contract implementation, unit tests, local validation".into(),
            deliverables: vec!["Working contract".into(), "Unit test suite".into()],
            dependencies: vec!["Discovery & Design".into()],
            estimated_days: complexity_days,
        },
        DevelopmentPhase {
            name: "Testing & Audit".into(),
            order: 3,
            description: "Integration testing, security audit, gas optimization".into(),
            deliverables: vec![
                "Test report".into(),
                "Audit report".into(),
                "Optimization report".into(),
            ],
            dependencies: vec!["Core Development".into()],
            estimated_days: 5,
        },
        DevelopmentPhase {
            name: "Deployment & Launch".into(),
            order: 4,
            description: "Testnet deployment, monitoring setup, mainnet launch".into(),
            deliverables: vec![
                "Deployed contract".into(),
                "Runbook".into(),
                "Monitoring dashboard".into(),
            ],
            dependencies: vec!["Testing & Audit".into()],
            estimated_days: 3,
        },
    ]
}

pub fn estimate_timeline(tasks: &[TaskItem], phases: &[DevelopmentPhase]) -> TimelineEstimate {
    let total_effort: u32 = tasks.iter().map(|t| t.effort_points).sum();
    let phase_days: u32 = phases.iter().map(|p| p.estimated_days).sum();
    let total_days = total_effort.max(phase_days).max(14);
    let buffer = (total_days as f64 * 0.2).ceil() as u32;
    let start = Utc::now();
    let end = start + Duration::days((total_days + buffer) as i64);

    let milestones = phases
        .iter()
        .scan(start, |cursor, phase| {
            *cursor = *cursor + Duration::days(phase.estimated_days as i64);
            Some(Milestone {
                name: phase.name.clone(),
                date: *cursor,
                deliverables: phase.deliverables.clone(),
            })
        })
        .collect();

    let critical_path: Vec<String> = tasks
        .iter()
        .filter(|t| matches!(t.priority, TaskPriority::Critical | TaskPriority::High))
        .map(|t| t.id.clone())
        .collect();

    TimelineEstimate {
        total_days: total_days + buffer,
        start_date: start,
        target_completion: end,
        milestones,
        critical_path,
        buffer_days: buffer,
    }
}

pub fn plan_resources(tasks: &[TaskItem], team_size: Option<u32>) -> ResourcePlan {
    let roles_needed: Vec<String> = tasks
        .iter()
        .map(|t| t.assignee_role.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let team_size = team_size.unwrap_or(roles_needed.len().max(2) as u32);

    let roles: Vec<TeamRole> = roles_needed
        .iter()
        .map(|r| TeamRole {
            role: r.clone(),
            count: 1,
            responsibilities: tasks
                .iter()
                .filter(|t| &t.assignee_role == r)
                .map(|t| t.title.clone())
                .take(5)
                .collect(),
        })
        .collect();

    let phases: Vec<String> = tasks
        .iter()
        .map(|t| t.phase.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let allocation: Vec<ResourceAllocation> = roles
        .iter()
        .flat_map(|role| {
            phases.iter().map(|phase| ResourceAllocation {
                role: role.role.clone(),
                phase: phase.clone(),
                allocation_pct: 50,
            })
        })
        .collect();

    ResourcePlan {
        team_size,
        roles,
        allocation,
        skills_required: vec![
            "Rust".into(),
            "Soroban SDK".into(),
            "Smart contract security".into(),
            "Stellar ecosystem".into(),
        ],
        tooling: vec![
            "starforge CLI".into(),
            "cargo test".into(),
            "Ollama (local AI)".into(),
            "Stellar testnet".into(),
        ],
    }
}

pub fn identify_risks(description: &str) -> Vec<ProjectRisk> {
    let lower = description.to_lowercase();
    let mut risks = vec![
        ProjectRisk {
            id: "R-001".into(),
            title: "Reentrancy and authorization bugs".into(),
            description: "Missing auth checks or reentrancy guards in contract logic".into(),
            category: RiskCategory::Security,
            severity: RiskSeverity::Critical,
            likelihood: RiskLikelihood::Possible,
            mitigation: "Use require_auth consistently; follow checks-effects-interactions".into(),
            contingency: "Pause contract and deploy patched version".into(),
        },
        ProjectRisk {
            id: "R-002".into(),
            title: "Gas cost overrun".into(),
            description: "Contract exceeds gas budget on mainnet".into(),
            category: RiskCategory::Technical,
            severity: RiskSeverity::High,
            likelihood: RiskLikelihood::Possible,
            mitigation: "Profile with starforge ai profile; optimize storage access patterns"
                .into(),
            contingency: "Refactor hot paths and redeploy".into(),
        },
        ProjectRisk {
            id: "R-003".into(),
            title: "Schedule slip".into(),
            description: "Development takes longer than estimated".into(),
            category: RiskCategory::Schedule,
            severity: RiskSeverity::Medium,
            likelihood: RiskLikelihood::Likely,
            mitigation: "20% timeline buffer; weekly progress reviews".into(),
            contingency: "Descope non-critical features for v1".into(),
        },
    ];

    if lower.contains("upgrade") {
        risks.push(ProjectRisk {
            id: "R-004".into(),
            title: "Upgrade migration failure".into(),
            description: "State migration during upgrade corrupts data".into(),
            category: RiskCategory::Technical,
            severity: RiskSeverity::Critical,
            likelihood: RiskLikelihood::Possible,
            mitigation: "Test migration on forked testnet state; timelock upgrades".into(),
            contingency: "Rollback to previous contract version".into(),
        });
    }

    if lower.contains("token") || lower.contains("payment") {
        risks.push(ProjectRisk {
            id: "R-005".into(),
            title: "Fund loss vulnerability".into(),
            description: "Logic error allows unauthorized fund extraction".into(),
            category: RiskCategory::Security,
            severity: RiskSeverity::Critical,
            likelihood: RiskLikelihood::Unlikely,
            mitigation: "Formal verification; multi-sig admin; audit before launch".into(),
            contingency: "Emergency pause and user notification".into(),
        });
    }

    if lower.contains("compliance") || lower.contains("kyc") {
        risks.push(ProjectRisk {
            id: "R-006".into(),
            title: "Regulatory compliance gap".into(),
            description: "Contract may not meet jurisdictional requirements".into(),
            category: RiskCategory::Compliance,
            severity: RiskSeverity::High,
            likelihood: RiskLikelihood::Possible,
            mitigation: "Legal review; implement allowlist/KYC hooks".into(),
            contingency: "Geo-restrict via off-chain compliance layer".into(),
        });
    }

    risks
}

pub fn default_testing_strategy() -> TestingStrategy {
    TestingStrategy {
        unit_tests: "soroban-sdk testutils for all public functions".into(),
        integration_tests: "Multi-contract scenarios on local sandbox".into(),
        property_tests: "AI property testing with starforge ai-property-test".into(),
        security_tests: "starforge ai audit + static analysis".into(),
        testnet_validation: "Deploy to testnet; run acceptance test suite".into(),
        coverage_target_pct: 85,
    }
}

pub fn default_deployment_plan() -> DeploymentPlan {
    DeploymentPlan {
        environments: vec!["local".into(), "testnet".into(), "mainnet".into()],
        pre_deploy_checks: vec![
            "All tests pass".into(),
            "Security audit complete".into(),
            "Gas profile within budget".into(),
            "WASM hash verified".into(),
        ],
        deployment_steps: vec![
            "Build optimized WASM".into(),
            "Simulate deployment transaction".into(),
            "Submit deploy transaction".into(),
            "Initialize contract state".into(),
            "Verify on-chain deployment".into(),
        ],
        rollback_procedure:
            "Keep previous contract ID; redirect clients; migrate state if upgradeable".into(),
        monitoring_setup: vec![
            "Contract event monitoring".into(),
            "Gas usage alerts".into(),
            "Error rate tracking".into(),
        ],
    }
}

/// Generate a complete project plan from a natural-language description.
pub fn generate_plan(project_name: &str, description: &str) -> ProjectPlan {
    let requirements = analyze_requirements(description);
    let architectures = suggest_architectures(description);
    let phases = default_phases(description);
    let tasks = breakdown_tasks(description, &phases);
    let timeline = estimate_timeline(&tasks, &phases);
    let resources = plan_resources(&tasks, None);
    let risks = identify_risks(description);

    ProjectPlan {
        project_name: project_name.to_string(),
        generated_at: Utc::now(),
        requirements,
        architectures,
        phases,
        tasks,
        timeline,
        resources,
        risks,
        testing_strategy: default_testing_strategy(),
        deployment_plan: default_deployment_plan(),
        ai_summary: None,
    }
}

/// Enhance a plan with AI-generated insights via Ollama.
pub async fn enhance_plan_with_ai(plan: &mut ProjectPlan, model: &str) -> Result<()> {
    let plan_json = serde_json::to_string_pretty(plan)?;
    let prompt = ollama::prompts::project_planning_prompt(&plan_json);
    let opts = GenerateOptions {
        temperature: Some(0.3),
        num_predict: Some(2048),
        num_ctx: Some(8192),
    };

    let response = ollama::generate(model, &prompt, Some(opts))
        .await
        .context("AI plan enhancement failed")?;

    plan.ai_summary = Some(response.response.trim().to_string());
    Ok(())
}

fn plans_dir() -> Result<PathBuf> {
    Ok(config::get_data_dir()?.join("project_plans"))
}

pub fn save_plan(plan: &ProjectPlan) -> Result<PathBuf> {
    let dir = plans_dir()?;
    fs::create_dir_all(&dir)?;
    let filename = format!(
        "{}_{}.json",
        plan.project_name.replace(' ', "_").to_lowercase(),
        plan.generated_at.format("%Y%m%d_%H%M%S")
    );
    let path = dir.join(filename);
    fs::write(&path, serde_json::to_string_pretty(plan)?)?;
    Ok(path)
}

pub fn load_plan(path: &PathBuf) -> Result<ProjectPlan> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn list_saved_plans() -> Result<Vec<PathBuf>> {
    let dir = plans_dir()?;
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut plans: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    plans.sort_by(|a, b| b.cmp(a));
    Ok(plans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyze_token_requirements() {
        let reqs = analyze_requirements("Build a SEP-41 token with mint and transfer");
        assert!(reqs
            .functional_requirements
            .iter()
            .any(|r| r.contains("Token")));
    }

    #[test]
    fn generate_plan_has_tasks() {
        let plan = generate_plan("test-token", "Simple fungible token contract");
        assert!(!plan.tasks.is_empty());
        assert!(!plan.risks.is_empty());
    }

    #[test]
    fn timeline_includes_buffer() {
        let plan = generate_plan("test", "NFT marketplace");
        assert!(plan.timeline.buffer_days > 0);
    }

    #[test]
    fn modular_architecture_for_governance() {
        let archs = suggest_architectures("DAO governance with voting");
        assert!(archs.iter().any(|a| a.name.contains("Modular")));
    }
}
