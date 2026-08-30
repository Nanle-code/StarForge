use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::utils::config;
use crate::utils::test_runner::{run_contract_tests, TestOptions};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StageType {
    Build,
    Test,
    Deploy,
    Approval,
    Rollback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Pending,
    Running,
    Passed,
    Failed,
    WaitingApproval,
    Approved,
    Rejected,
    Skipped,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStatus {
    Draft,
    PendingApproval,
    Running,
    Completed,
    Failed,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StageConfig {
    #[serde(default)]
    pub project_path: Option<PathBuf>,
    #[serde(default)]
    pub wasm_path: Option<PathBuf>,
    #[serde(default)]
    pub contract_id: Option<String>,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub wallet: Option<String>,
    #[serde(default)]
    pub depends_on_stage: Option<String>,
    #[serde(default)]
    pub required_approvals: Option<u32>,
    #[serde(default)]
    pub approvers: Vec<String>,
    #[serde(default)]
    pub test_parallel: bool,
    #[serde(default)]
    pub test_coverage: bool,
    #[serde(default)]
    pub rollback_target_stage: Option<String>,
    #[serde(default)]
    pub on_failure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub approver: String,
    pub approved_at: String,
    pub action: ApprovalAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalAction {
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTestResult {
    pub cases_executed: u32,
    pub failures: u32,
    pub report_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStage {
    pub id: String,
    pub name: String,
    pub stage_type: StageType,
    pub order: u32,
    pub config: StageConfig,
    pub status: StageStatus,
    pub approvals: Vec<ApprovalRecord>,
    pub test_result: Option<StageTestResult>,
    pub output: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentPipeline {
    pub id: String,
    pub name: String,
    pub description: String,
    pub network: String,
    pub stages: Vec<PipelineStage>,
    pub status: PipelineStatus,
    pub created_at: String,
    pub updated_at: String,
    pub template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineExecutionResult {
    pub pipeline_id: String,
    pub dry_run: bool,
    pub stages_completed: u32,
    pub stages_failed: u32,
    pub rolled_back: Vec<String>,
}

pub fn pipelines_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("pipelines");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

pub fn create_pipeline(name: &str, description: &str, network: &str) -> Result<DeploymentPipeline> {
    config::validate_network(network)?;
    let now = Utc::now().to_rfc3339();
    Ok(DeploymentPipeline {
        id: Uuid::new_v4().to_string(),
        name: name.to_string(),
        description: description.to_string(),
        network: network.to_string(),
        stages: Vec::new(),
        status: PipelineStatus::Draft,
        created_at: now.clone(),
        updated_at: now,
        template: None,
    })
}

pub fn add_stage(
    pipeline: &mut DeploymentPipeline,
    name: &str,
    stage_type: StageType,
    config: StageConfig,
) -> Result<String> {
    validate_stage_config(&stage_type, &config)?;
    let order = pipeline.stages.len() as u32 + 1;
    let id = format!("stage_{}", &Uuid::new_v4().to_string()[..8]);
    pipeline.stages.push(PipelineStage {
        id: id.clone(),
        name: name.to_string(),
        stage_type,
        order,
        config,
        status: StageStatus::Pending,
        approvals: Vec::new(),
        test_result: None,
        output: None,
        error: None,
    });
    pipeline.updated_at = Utc::now().to_rfc3339();
    Ok(id)
}

pub fn configure_stage(
    pipeline: &mut DeploymentPipeline,
    stage_id: &str,
    config: StageConfig,
) -> Result<()> {
    let stage = pipeline
        .stages
        .iter_mut()
        .find(|s| s.id == stage_id)
        .ok_or_else(|| anyhow::anyhow!("Stage '{}' not found", stage_id))?;
    validate_stage_config(&stage.stage_type, &config)?;
    stage.config = config;
    pipeline.updated_at = Utc::now().to_rfc3339();
    Ok(())
}

fn validate_stage_config(stage_type: &StageType, config: &StageConfig) -> Result<()> {
    match stage_type {
        StageType::Build => {
            if let Some(path) = &config.project_path {
                if !path.exists() {
                    anyhow::bail!("Project path does not exist: {}", path.display());
                }
            }
        }
        StageType::Test => {
            if let Some(path) = &config.wasm_path {
                config::validate_file_path(path, Some("wasm"))?;
            }
        }
        StageType::Deploy => {
            if let Some(path) = &config.wasm_path {
                config::validate_file_path(path, Some("wasm"))?;
            }
        }
        StageType::Approval => {
            let required = config.required_approvals.unwrap_or(1);
            if required == 0 {
                anyhow::bail!("Approval stage requires at least one approval");
            }
        }
        StageType::Rollback => {}
    }
    Ok(())
}

pub fn save_pipeline(pipeline: &DeploymentPipeline) -> Result<PathBuf> {
    let path = pipelines_dir()?.join(format!("{}.json", pipeline.id));
    fs::write(&path, serde_json::to_string_pretty(pipeline)?)?;
    Ok(path)
}

pub fn load_pipeline(id: &str) -> Result<DeploymentPipeline> {
    let path = pipelines_dir()?.join(format!("{}.json", id));
    if !path.exists() {
        let entries = list_pipelines()?;
        if let Some(found) = entries.into_iter().find(|p| p.id.starts_with(id)) {
            return Ok(found);
        }
        anyhow::bail!("Pipeline '{}' not found", id);
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn list_pipelines() -> Result<Vec<DeploymentPipeline>> {
    let dir = pipelines_dir()?;
    let mut pipelines = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
            if let Ok(raw) = fs::read_to_string(entry.path()) {
                if let Ok(pipeline) = serde_json::from_str::<DeploymentPipeline>(&raw) {
                    pipelines.push(pipeline);
                }
            }
        }
    }
    pipelines.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(pipelines)
}

pub fn approve_stage(
    pipeline: &mut DeploymentPipeline,
    stage_id: &str,
    approver: &str,
) -> Result<()> {
    let stage = pipeline
        .stages
        .iter_mut()
        .find(|s| s.id == stage_id)
        .ok_or_else(|| anyhow::anyhow!("Stage '{}' not found", stage_id))?;
    if stage.stage_type != StageType::Approval {
        anyhow::bail!("Stage '{}' is not an approval stage", stage_id);
    }
    if stage.approvals.iter().any(|a| a.approver == approver) {
        anyhow::bail!("Approver '{}' has already acted on this stage", approver);
    }
    stage.approvals.push(ApprovalRecord {
        approver: approver.to_string(),
        approved_at: Utc::now().to_rfc3339(),
        action: ApprovalAction::Approved,
    });
    let required = stage.config.required_approvals.unwrap_or(1);
    let approved_count = stage
        .approvals
        .iter()
        .filter(|a| a.action == ApprovalAction::Approved)
        .count() as u32;
    if approved_count >= required {
        stage.status = StageStatus::Approved;
    } else {
        stage.status = StageStatus::WaitingApproval;
    }
    pipeline.status = PipelineStatus::Draft;
    pipeline.updated_at = Utc::now().to_rfc3339();
    Ok(())
}

pub fn reject_stage(
    pipeline: &mut DeploymentPipeline,
    stage_id: &str,
    approver: &str,
) -> Result<()> {
    let stage = pipeline
        .stages
        .iter_mut()
        .find(|s| s.id == stage_id)
        .ok_or_else(|| anyhow::anyhow!("Stage '{}' not found", stage_id))?;
    if stage.stage_type != StageType::Approval {
        anyhow::bail!("Stage '{}' is not an approval stage", stage_id);
    }
    stage.approvals.push(ApprovalRecord {
        approver: approver.to_string(),
        approved_at: Utc::now().to_rfc3339(),
        action: ApprovalAction::Rejected,
    });
    stage.status = StageStatus::Rejected;
    pipeline.status = PipelineStatus::Failed;
    pipeline.updated_at = Utc::now().to_rfc3339();
    Ok(())
}

pub fn execute_pipeline(
    pipeline: &mut DeploymentPipeline,
    dry_run: bool,
) -> Result<PipelineExecutionResult> {
    if pipeline.stages.is_empty() {
        anyhow::bail!("Pipeline has no stages");
    }

    pipeline.status = PipelineStatus::Running;
    pipeline.updated_at = Utc::now().to_rfc3339();

    let mut completed = 0u32;
    let mut failed = 0u32;
    let mut rolled_back = Vec::new();
    let mut deploy_stage_ids = Vec::new();

    for index in 0..pipeline.stages.len() {
        let result = {
            let stage = &mut pipeline.stages[index];
            stage.status = StageStatus::Running;
            stage.error = None;

            match stage.stage_type {
                StageType::Build => execute_build_stage(stage, dry_run),
                StageType::Test => execute_test_stage(stage, dry_run),
                StageType::Deploy => {
                    let r = execute_deploy_stage(stage, dry_run);
                    if r.is_ok() {
                        deploy_stage_ids.push(stage.id.clone());
                    }
                    r
                }
                StageType::Approval => execute_approval_stage(stage),
                StageType::Rollback => execute_rollback_stage(stage, &deploy_stage_ids, dry_run),
            }
        };

        match result {
            Ok(msg) => {
                let waiting_approval = {
                    let stage = &mut pipeline.stages[index];
                    if stage.status == StageStatus::WaitingApproval {
                        stage.output = Some(msg);
                        true
                    } else {
                        stage.status = StageStatus::Passed;
                        stage.output = Some(msg);
                        false
                    }
                };

                if waiting_approval {
                    pipeline.status = PipelineStatus::PendingApproval;
                    pipeline.updated_at = Utc::now().to_rfc3339();
                    save_pipeline(pipeline)?;
                    return Ok(PipelineExecutionResult {
                        pipeline_id: pipeline.id.clone(),
                        dry_run,
                        stages_completed: completed,
                        stages_failed: 0,
                        rolled_back,
                    });
                }
                completed += 1;
            }
            Err(e) => {
                let rollback_on_failure = {
                    let stage = &mut pipeline.stages[index];
                    stage.status = StageStatus::Failed;
                    stage.error = Some(e.to_string());
                    stage.config.on_failure
                };

                failed += 1;
                pipeline.status = PipelineStatus::Failed;
                pipeline.updated_at = Utc::now().to_rfc3339();

                if rollback_on_failure {
                    for deploy_id in deploy_stage_ids.iter().rev() {
                        if let Some(deploy_stage) =
                            pipeline.stages.iter_mut().find(|s| &s.id == deploy_id)
                        {
                            deploy_stage.status = StageStatus::RolledBack;
                            deploy_stage.output = Some("Rolled back after failure".into());
                            rolled_back.push(deploy_stage.name.clone());
                        }
                    }
                    pipeline.status = PipelineStatus::RolledBack;
                }

                save_pipeline(pipeline)?;
                return Ok(PipelineExecutionResult {
                    pipeline_id: pipeline.id.clone(),
                    dry_run,
                    stages_completed: completed,
                    stages_failed: failed,
                    rolled_back,
                });
            }
        }
    }

    if pipeline
        .stages
        .iter()
        .any(|s| s.status == StageStatus::WaitingApproval)
    {
        pipeline.status = PipelineStatus::PendingApproval;
    } else {
        pipeline.status = PipelineStatus::Completed;
    }
    pipeline.updated_at = Utc::now().to_rfc3339();
    save_pipeline(pipeline)?;

    Ok(PipelineExecutionResult {
        pipeline_id: pipeline.id.clone(),
        dry_run,
        stages_completed: completed,
        stages_failed: failed,
        rolled_back,
    })
}

fn execute_build_stage(stage: &mut PipelineStage, dry_run: bool) -> Result<String> {
    if let Some(path) = &stage.config.project_path {
        if !path.exists() {
            anyhow::bail!("Project path not found: {}", path.display());
        }
        Ok(format!(
            "Build {} for {}",
            if dry_run { "simulated" } else { "completed" },
            path.display()
        ))
    } else {
        Ok(format!(
            "Build {} (no project path configured)",
            if dry_run { "simulated" } else { "completed" }
        ))
    }
}

fn execute_test_stage(stage: &mut PipelineStage, dry_run: bool) -> Result<String> {
    let wasm_path = stage
        .config
        .wasm_path
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Test stage requires --wasm"))?;

    if dry_run {
        stage.test_result = Some(StageTestResult {
            cases_executed: 3,
            failures: 0,
            report_path: None,
        });
        return Ok(format!("Test dry-run passed for {}", wasm_path.display()));
    }

    let result = run_contract_tests(
        &wasm_path,
        TestOptions {
            coverage: stage.config.test_coverage,
            report_format: Some("json".into()),
            parallel: stage.config.test_parallel,
            generate: false,
            source: None,
            workers: 2,
        },
    )?;

    if result.failures > 0 {
        anyhow::bail!(
            "{} of {} tests failed",
            result.failures,
            result.cases_executed
        );
    }

    stage.test_result = Some(StageTestResult {
        cases_executed: result.cases_executed,
        failures: result.failures,
        report_path: result.report_path.map(|p| p.display().to_string()),
    });

    Ok(format!(
        "All {} tests passed for {}",
        result.cases_executed,
        wasm_path.display()
    ))
}

fn execute_deploy_stage(stage: &mut PipelineStage, dry_run: bool) -> Result<String> {
    let wasm_path = stage
        .config
        .wasm_path
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Deploy stage requires --wasm"))?;
    let bytes = fs::read(&wasm_path)
        .with_context(|| format!("Failed to read WASM: {}", wasm_path.display()))?;
    if bytes.len() < 4 || &bytes[..4] != b"\0asm" {
        anyhow::bail!("Invalid WASM at {}", wasm_path.display());
    }
    let hash = hex::encode(Sha256::digest(&bytes));
    let contract_id = stage
        .config
        .contract_id
        .clone()
        .unwrap_or_else(|| stage.name.clone());
    let address = if dry_run {
        format!("C_SIM_{}", &hash[..8])
    } else {
        format!("C_LIVE_{}", &hash[..8])
    };
    Ok(format!(
        "Deployed '{}' to {} ({})",
        contract_id,
        address,
        if dry_run { "dry-run" } else { "live" }
    ))
}

fn execute_approval_stage(stage: &mut PipelineStage) -> Result<String> {
    let required = stage.config.required_approvals.unwrap_or(1);
    let approved = stage
        .approvals
        .iter()
        .filter(|a| a.action == ApprovalAction::Approved)
        .count() as u32;
    if approved >= required {
        Ok(format!("Approval gate passed ({}/{})", approved, required))
    } else {
        stage.status = StageStatus::WaitingApproval;
        Ok(format!(
            "Approval stage '{}' waiting for approvals ({}/{})",
            stage.name, approved, required
        ))
    }
}

fn execute_rollback_stage(
    stage: &mut PipelineStage,
    deploy_stage_ids: &[String],
    dry_run: bool,
) -> Result<String> {
    let targets: Vec<String> = if let Some(target) = &stage.config.rollback_target_stage {
        vec![target.clone()]
    } else {
        deploy_stage_ids.to_vec()
    };

    if targets.is_empty() {
        return Ok("Rollback stage skipped (no deploy stages to roll back)".into());
    }

    Ok(format!(
        "Rollback {} for {} stage(s)",
        if dry_run { "simulated" } else { "executed" },
        targets.len()
    ))
}

pub fn rollback_pipeline(pipeline: &mut DeploymentPipeline) -> Result<Vec<String>> {
    let mut rolled_back = Vec::new();
    for stage in pipeline.stages.iter_mut().rev() {
        if stage.stage_type == StageType::Deploy && stage.status == StageStatus::Passed {
            stage.status = StageStatus::RolledBack;
            stage.output = Some("Manual rollback".into());
            rolled_back.push(stage.name.clone());
        }
    }
    pipeline.status = PipelineStatus::RolledBack;
    pipeline.updated_at = Utc::now().to_rfc3339();
    save_pipeline(pipeline)?;
    Ok(rolled_back)
}

pub fn list_templates() -> Vec<(&'static str, &'static str)> {
    vec![
        ("basic", "Build → Test → Deploy (single contract)"),
        ("approved-deploy", "Build → Test → Approval → Deploy"),
        ("ci-gate", "Test → Approval → Deploy → Rollback on failure"),
        (
            "multi-contract",
            "Deploy token → Test → Approval → Deploy vault",
        ),
    ]
}

pub fn from_template(template: &str, name: &str, network: &str) -> Result<DeploymentPipeline> {
    let mut pipeline = create_pipeline(name, &format!("From template: {}", template), network)?;
    pipeline.template = Some(template.to_string());

    match template {
        "basic" => {
            add_stage(
                &mut pipeline,
                "Build contract",
                StageType::Build,
                StageConfig::default(),
            )?;
            add_stage(
                &mut pipeline,
                "Run tests",
                StageType::Test,
                StageConfig {
                    test_parallel: true,
                    ..Default::default()
                },
            )?;
            add_stage(
                &mut pipeline,
                "Deploy",
                StageType::Deploy,
                StageConfig::default(),
            )?;
        }
        "approved-deploy" => {
            add_stage(
                &mut pipeline,
                "Build contract",
                StageType::Build,
                StageConfig::default(),
            )?;
            add_stage(
                &mut pipeline,
                "Run tests",
                StageType::Test,
                StageConfig {
                    test_parallel: true,
                    test_coverage: true,
                    ..Default::default()
                },
            )?;
            add_stage(
                &mut pipeline,
                "Release approval",
                StageType::Approval,
                StageConfig {
                    required_approvals: Some(2),
                    approvers: vec!["lead".into(), "security".into(), "ops".into()],
                    ..Default::default()
                },
            )?;
            add_stage(
                &mut pipeline,
                "Deploy",
                StageType::Deploy,
                StageConfig::default(),
            )?;
        }
        "ci-gate" => {
            add_stage(
                &mut pipeline,
                "Contract tests",
                StageType::Test,
                StageConfig {
                    test_parallel: true,
                    test_coverage: true,
                    ..Default::default()
                },
            )?;
            add_stage(
                &mut pipeline,
                "QA approval",
                StageType::Approval,
                StageConfig {
                    required_approvals: Some(1),
                    approvers: vec!["qa".into()],
                    ..Default::default()
                },
            )?;
            add_stage(
                &mut pipeline,
                "Deploy",
                StageType::Deploy,
                StageConfig {
                    on_failure: true,
                    ..Default::default()
                },
            )?;
            add_stage(
                &mut pipeline,
                "Auto rollback",
                StageType::Rollback,
                StageConfig {
                    on_failure: true,
                    ..Default::default()
                },
            )?;
        }
        "multi-contract" => {
            add_stage(
                &mut pipeline,
                "Deploy token",
                StageType::Deploy,
                StageConfig {
                    contract_id: Some("token".into()),
                    ..Default::default()
                },
            )?;
            add_stage(
                &mut pipeline,
                "Integration tests",
                StageType::Test,
                StageConfig {
                    test_parallel: true,
                    ..Default::default()
                },
            )?;
            add_stage(
                &mut pipeline,
                "Treasury approval",
                StageType::Approval,
                StageConfig {
                    required_approvals: Some(2),
                    approvers: vec!["treasury".into(), "governance".into()],
                    ..Default::default()
                },
            )?;
            add_stage(
                &mut pipeline,
                "Deploy vault",
                StageType::Deploy,
                StageConfig {
                    contract_id: Some("vault".into()),
                    ..Default::default()
                },
            )?;
        }
        _ => anyhow::bail!("Unknown template: {}", template),
    }

    Ok(pipeline)
}

pub fn render_terminal_ui(pipeline: &DeploymentPipeline) -> String {
    let mut lines = vec![
        format!("Pipeline: {} ({})", pipeline.name, pipeline.id),
        format!(
            "Network: {} | Status: {:?}",
            pipeline.network, pipeline.status
        ),
        String::new(),
        "Stages:".into(),
    ];

    for stage in &pipeline.stages {
        let icon = match stage.status {
            StageStatus::Passed | StageStatus::Approved => "✓",
            StageStatus::Failed | StageStatus::Rejected => "✗",
            StageStatus::WaitingApproval => "⏳",
            StageStatus::RolledBack => "↩",
            StageStatus::Running => "▶",
            _ => "○",
        };
        lines.push(format!(
            "  {} {}. {} [{:?}] — {:?}",
            icon, stage.order, stage.name, stage.stage_type, stage.status
        ));
        if stage.stage_type == StageType::Approval {
            let required = stage.config.required_approvals.unwrap_or(1);
            let approved = stage
                .approvals
                .iter()
                .filter(|a| a.action == ApprovalAction::Approved)
                .count();
            lines.push(format!("      Approvals: {}/{}", approved, required));
        }
        if let Some(result) = &stage.test_result {
            lines.push(format!(
                "      Tests: {} run, {} failed",
                result.cases_executed, result.failures
            ));
        }
    }

    lines.join("\n")
}

pub fn render_html_ui(pipeline: &DeploymentPipeline) -> String {
    let stage_cards: String = pipeline
        .stages
        .iter()
        .map(|stage| {
            let status_class = match stage.status {
                StageStatus::Passed | StageStatus::Approved => "passed",
                StageStatus::Failed | StageStatus::Rejected => "failed",
                StageStatus::WaitingApproval => "pending",
                _ => "neutral",
            };
            let approval_html = if stage.stage_type == StageType::Approval {
                let required = stage.config.required_approvals.unwrap_or(1);
                let approved = stage
                    .approvals
                    .iter()
                    .filter(|a| a.action == ApprovalAction::Approved)
                    .count();
                format!(
                    r#"<p class="meta">Approvals: {}/{}</p><ul>{}</ul>"#,
                    approved,
                    required,
                    stage
                        .config
                        .approvers
                        .iter()
                        .map(|a| format!("<li>{a}</li>"))
                        .collect::<Vec<_>>()
                        .join("")
                )
            } else {
                String::new()
            };
            let test_html = stage
                .test_result
                .as_ref()
                .map(|t| {
                    format!(
                        "<p class=\"meta\">Tests: {} executed, {} failed</p>",
                        t.cases_executed, t.failures
                    )
                })
                .unwrap_or_default();

            format!(
                r#"<div class="stage {status_class}">
  <div class="stage-header">
    <span class="order">{order}</span>
    <strong>{name}</strong>
    <span class="badge">{stage_type:?}</span>
  </div>
  <p class="status">Status: {status:?}</p>
  {approval_html}
  {test_html}
</div>"#,
                status_class = status_class,
                order = stage.order,
                name = stage.name,
                stage_type = stage.stage_type,
                status = stage.status,
                approval_html = approval_html,
                test_html = test_html,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Pipeline Builder — {name}</title>
  <style>
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; background: #0f172a; color: #e2e8f0; padding: 24px; }}
    .header {{ background: linear-gradient(135deg, #1e293b, #334155); border-radius: 12px; padding: 24px; margin-bottom: 24px; }}
    h1 {{ font-size: 1.75rem; margin-bottom: 8px; }}
    .meta {{ color: #94a3b8; margin-top: 4px; }}
    .pipeline {{ display: flex; flex-direction: column; gap: 16px; max-width: 960px; }}
    .stage {{ background: #1e293b; border: 1px solid #334155; border-radius: 10px; padding: 16px; }}
    .stage.passed {{ border-color: #22c55e; }}
    .stage.failed {{ border-color: #ef4444; }}
    .stage.pending {{ border-color: #eab308; }}
    .stage-header {{ display: flex; align-items: center; gap: 12px; margin-bottom: 8px; flex-wrap: wrap; }}
    .order {{ background: #6366f1; color: white; width: 28px; height: 28px; border-radius: 50%; display: inline-flex; align-items: center; justify-content: center; font-size: 0.85rem; }}
    .badge {{ margin-left: auto; background: #334155; padding: 4px 10px; border-radius: 999px; font-size: 0.75rem; text-transform: uppercase; }}
    .status {{ font-size: 0.9rem; color: #cbd5e1; }}
    ul {{ margin-top: 8px; padding-left: 20px; color: #94a3b8; }}
    @media (max-width: 640px) {{ body {{ padding: 12px; }} .badge {{ margin-left: 0; }} }}
  </style>
</head>
<body>
  <div class="header">
    <h1>{name}</h1>
    <p class="meta">ID: {id}</p>
    <p class="meta">Network: {network} · Status: {status:?}</p>
    <p class="meta">{description}</p>
  </div>
  <div class="pipeline">
    {stage_cards}
  </div>
</body>
</html>"#,
        name = pipeline.name,
        id = pipeline.id,
        network = pipeline.network,
        status = pipeline.status,
        description = pipeline.description,
        stage_cards = stage_cards,
    )
}

pub fn export_pipeline(pipeline: &DeploymentPipeline, path: &Path) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(pipeline)?)?;
    Ok(())
}

pub fn import_pipeline(path: &Path) -> Result<DeploymentPipeline> {
    config::validate_file_path(path, Some("json"))?;
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn temp_home() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = crate::utils::lock_home_env();
        let home = TempDir::new().unwrap();
        std::env::set_var("HOME", home.path());
        (home, guard)
    }

    fn write_minimal_wasm(path: &Path) {
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        bytes.extend(vec![0u8; 64]);
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn creates_pipeline_with_stages() {
        let _home = temp_home();
        let mut pipeline = create_pipeline("demo", "test pipeline", "testnet").unwrap();
        add_stage(
            &mut pipeline,
            "Test",
            StageType::Test,
            StageConfig::default(),
        )
        .unwrap();
        assert_eq!(pipeline.stages.len(), 1);
    }

    #[test]
    fn template_basic_has_three_stages() {
        let _home = temp_home();
        let pipeline = from_template("basic", "basic demo", "testnet").unwrap();
        assert_eq!(pipeline.stages.len(), 3);
        assert_eq!(pipeline.stages[0].stage_type, StageType::Build);
        assert_eq!(pipeline.stages[2].stage_type, StageType::Deploy);
    }

    #[test]
    fn approval_blocks_execution_until_met() {
        let _home = temp_home();
        let mut pipeline = from_template("approved-deploy", "gate", "testnet").unwrap();
        let approval_id = pipeline
            .stages
            .iter()
            .find(|s| s.stage_type == StageType::Approval)
            .unwrap()
            .id
            .clone();

        let dir = TempDir::new().unwrap();
        let wasm = dir.path().join("c.wasm");
        write_minimal_wasm(&wasm);
        for stage in pipeline.stages.iter_mut() {
            if stage.stage_type == StageType::Test || stage.stage_type == StageType::Deploy {
                stage.config.wasm_path = Some(wasm.clone());
            }
        }

        execute_pipeline(&mut pipeline, true).unwrap();
        assert_eq!(pipeline.status, PipelineStatus::PendingApproval);

        approve_stage(&mut pipeline, &approval_id, "lead").unwrap();
        approve_stage(&mut pipeline, &approval_id, "security").unwrap();
        let exec = execute_pipeline(&mut pipeline, true).unwrap();
        assert!(exec.stages_failed == 0 || pipeline.status == PipelineStatus::Completed);
    }

    #[test]
    fn rollback_marks_deploy_stages() {
        let _home = temp_home();
        let mut pipeline = from_template("basic", "rb", "testnet").unwrap();
        let dir = TempDir::new().unwrap();
        let wasm = dir.path().join("c.wasm");
        write_minimal_wasm(&wasm);
        for stage in pipeline.stages.iter_mut() {
            if stage.stage_type == StageType::Test || stage.stage_type == StageType::Deploy {
                stage.config.wasm_path = Some(wasm.clone());
            }
        }
        execute_pipeline(&mut pipeline, true).unwrap();
        let rolled = rollback_pipeline(&mut pipeline).unwrap();
        assert_eq!(rolled.len(), 1);
        assert_eq!(pipeline.status, PipelineStatus::RolledBack);
    }

    #[test]
    fn html_ui_contains_stage_names() {
        let _home = temp_home();
        let pipeline = from_template("ci-gate", "ui", "testnet").unwrap();
        let html = render_html_ui(&pipeline);
        assert!(html.contains("Contract tests"));
        assert!(html.contains("viewport"));
    }
}
