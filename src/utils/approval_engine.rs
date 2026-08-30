use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    InProgress,
    Approved,
    Rejected,
    Expired,
    Cancelled,
}

impl std::fmt::Display for ApprovalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApprovalStatus::Pending => write!(f, "pending"),
            ApprovalStatus::InProgress => write!(f, "in_progress"),
            ApprovalStatus::Approved => write!(f, "approved"),
            ApprovalStatus::Rejected => write!(f, "rejected"),
            ApprovalStatus::Expired => write!(f, "expired"),
            ApprovalStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Approve,
    Reject,
    Escalate,
}

impl std::fmt::Display for ActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionType::Approve => write!(f, "approve"),
            ActionType::Reject => write!(f, "reject"),
            ActionType::Escalate => write!(f, "escalate"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalLevel {
    pub name: String,
    pub description: String,
    pub required_approvers: u8,
    pub approver_roles: Vec<String>,
    pub timeout_hours: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalWorkflow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub levels: Vec<ApprovalLevel>,
    pub created_at: String,
    pub updated_at: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalAction {
    pub level_index: usize,
    pub level_name: String,
    pub approver: String,
    pub action: ActionType,
    pub comment: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    pub total_levels: usize,
    pub contract_id: String,
    pub wasm_path: String,
    pub wasm_hash: String,
    pub network: String,
    pub description: String,
    pub requested_by: String,
    pub current_level: usize,
    pub status: ApprovalStatus,
    pub actions: Vec<ApprovalAction>,
    pub created_at: String,
    pub updated_at: String,
    pub expires_at: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl ApprovalRequest {
    pub fn level_progress(&self) -> String {
        format!("{}/{}", self.current_level, self.total_levels)
    }

    pub fn is_fully_approved(&self) -> bool {
        self.status == ApprovalStatus::Approved
    }
}

fn approval_dir() -> Result<PathBuf> {
    let dir = crate::utils::config::config_dir().join("approval");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn workflows_path() -> Result<PathBuf> {
    Ok(approval_dir()?.join("workflows.json"))
}

fn requests_path() -> Result<PathBuf> {
    Ok(approval_dir()?.join("requests.json"))
}

fn load_workflows_raw() -> Result<Vec<ApprovalWorkflow>> {
    let path = workflows_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

fn save_workflows_raw(workflows: &[ApprovalWorkflow]) -> Result<()> {
    let path = workflows_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(workflows)?)?;
    Ok(())
}

fn load_requests_raw() -> Result<Vec<ApprovalRequest>> {
    let path = requests_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

fn save_requests_raw(requests: &[ApprovalRequest]) -> Result<()> {
    let path = requests_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(requests)?)?;
    Ok(())
}

pub fn create_workflow(
    name: &str,
    description: &str,
    levels: Vec<ApprovalLevel>,
) -> Result<ApprovalWorkflow> {
    let mut workflows = load_workflows_raw()?;
    let id = format!(
        "wf-{}",
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("0000")
    );

    if levels.is_empty() {
        anyhow::bail!("Workflow must have at least one approval level");
    }

    let now = Utc::now().to_rfc3339();
    let workflow = ApprovalWorkflow {
        id: id.clone(),
        name: name.to_string(),
        description: description.to_string(),
        levels,
        created_at: now.clone(),
        updated_at: now,
        active: true,
    };

    workflows.push(workflow.clone());
    save_workflows_raw(&workflows)?;
    Ok(workflow)
}

pub fn list_workflows(active_only: bool) -> Result<Vec<ApprovalWorkflow>> {
    let workflows = load_workflows_raw()?;
    if active_only {
        Ok(workflows.into_iter().filter(|w| w.active).collect())
    } else {
        Ok(workflows)
    }
}

pub fn get_workflow(id: &str) -> Result<Option<ApprovalWorkflow>> {
    let workflows = load_workflows_raw()?;
    Ok(workflows.into_iter().find(|w| w.id == id))
}

pub fn deactivate_workflow(id: &str) -> Result<()> {
    let mut workflows = load_workflows_raw()?;
    let found = workflows.iter_mut().find(|w| w.id == id);
    match found {
        Some(wf) => {
            wf.active = false;
            wf.updated_at = Utc::now().to_rfc3339();
            save_workflows_raw(&workflows)?;
            Ok(())
        }
        None => anyhow::bail!("Workflow '{}' not found", id),
    }
}

pub fn create_request(
    workflow_id: &str,
    contract_id: &str,
    wasm_path: &str,
    wasm_hash: &str,
    network: &str,
    description: &str,
    requested_by: &str,
    metadata: HashMap<String, String>,
) -> Result<ApprovalRequest> {
    let workflow = get_workflow(workflow_id)?
        .ok_or_else(|| anyhow::anyhow!("Workflow '{}' not found", workflow_id))?;

    if !workflow.active {
        anyhow::bail!("Workflow '{}' is deactivated", workflow_id);
    }

    let mut requests = load_requests_raw()?;
    let id = format!(
        "apr-{}",
        uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("0000")
    );

    let expires_at = workflow
        .levels
        .first()
        .and_then(|l| l.timeout_hours)
        .map(|h| {
            let expiry = Utc::now() + chrono::Duration::hours(h as i64);
            expiry.to_rfc3339()
        });

    let total_levels = workflow.levels.len();
    let now = Utc::now().to_rfc3339();
    let request = ApprovalRequest {
        id: id.clone(),
        workflow_id: workflow_id.to_string(),
        workflow_name: workflow.name.clone(),
        total_levels,
        contract_id: contract_id.to_string(),
        wasm_path: wasm_path.to_string(),
        wasm_hash: wasm_hash.to_string(),
        network: network.to_string(),
        description: description.to_string(),
        requested_by: requested_by.to_string(),
        current_level: 0,
        status: ApprovalStatus::Pending,
        actions: vec![],
        created_at: now.clone(),
        updated_at: now,
        expires_at,
        metadata,
    };

    requests.push(request.clone());
    save_requests_raw(&requests)?;
    Ok(request)
}

pub fn list_requests(
    status_filter: Option<ApprovalStatus>,
    network_filter: Option<&str>,
) -> Result<Vec<ApprovalRequest>> {
    let requests = load_requests_raw()?;
    Ok(requests
        .into_iter()
        .filter(|r| status_filter.as_ref().map_or(true, |s| r.status == *s))
        .filter(|r| network_filter.map_or(true, |n| r.network == n))
        .collect())
}

pub fn get_request(id: &str) -> Result<Option<ApprovalRequest>> {
    let requests = load_requests_raw()?;
    Ok(requests
        .into_iter()
        .find(|r| r.id == id || r.id.starts_with(id)))
}

fn process_expired_requests() -> Result<Vec<ApprovalRequest>> {
    let mut requests = load_requests_raw()?;
    let now = Utc::now();
    let mut changed = false;

    for req in &mut requests {
        if req.status != ApprovalStatus::Pending && req.status != ApprovalStatus::InProgress {
            continue;
        }
        if let Some(ref expiry) = req.expires_at {
            if let Ok(exp_dt) = chrono::DateTime::parse_from_rfc3339(expiry) {
                if now > exp_dt.with_timezone(&Utc) {
                    req.status = ApprovalStatus::Expired;
                    req.updated_at = now.to_rfc3339();
                    changed = true;
                }
            }
        }
    }

    if changed {
        save_requests_raw(&requests)?;
    }
    Ok(requests)
}

pub fn approve_request(
    id: &str,
    approver: &str,
    comment: Option<&str>,
    roles: &[String],
) -> Result<ApprovalRequest> {
    let mut requests = load_requests_raw()?;
    let req = requests
        .iter_mut()
        .find(|r| r.id == id || r.id.starts_with(id))
        .ok_or_else(|| anyhow::anyhow!("Approval request '{}' not found", id))?;

    if req.status == ApprovalStatus::Expired {
        anyhow::bail!("Approval request '{}' has expired", id);
    }
    if req.status == ApprovalStatus::Approved {
        anyhow::bail!("Approval request '{}' is already fully approved", id);
    }
    if req.status == ApprovalStatus::Rejected {
        anyhow::bail!("Approval request '{}' has been rejected", id);
    }
    if req.status == ApprovalStatus::Cancelled {
        anyhow::bail!("Approval request '{}' has been cancelled", id);
    }

    let workflow = get_workflow(&req.workflow_id)?
        .ok_or_else(|| anyhow::anyhow!("Workflow '{}' not found for request", req.workflow_id))?;

    if req.current_level >= workflow.levels.len() {
        anyhow::bail!("Request '{}' has already passed all approval levels", id);
    }

    let level = &workflow.levels[req.current_level];

    let has_role = roles.is_empty() || roles.iter().any(|r| level.approver_roles.contains(r));
    if !has_role {
        anyhow::bail!(
            "Approver does not have a required role for level '{}'. Required: {:?}",
            level.name,
            level.approver_roles
        );
    }

    let already_acted = req.actions.iter().any(|a| {
        a.level_index == req.current_level
            && a.approver == approver
            && a.action == ActionType::Approve
    });
    if already_acted {
        anyhow::bail!(
            "Approver '{}' has already approved at level '{}'",
            approver,
            level.name
        );
    }

    let approved_count = req
        .actions
        .iter()
        .filter(|a| a.level_index == req.current_level && a.action == ActionType::Approve)
        .count() as u8;

    req.actions.push(ApprovalAction {
        level_index: req.current_level,
        level_name: level.name.clone(),
        approver: approver.to_string(),
        action: ActionType::Approve,
        comment: comment.map(|c| c.to_string()),
        timestamp: Utc::now().to_rfc3339(),
    });
    req.updated_at = Utc::now().to_rfc3339();

    let level_index = req.current_level;
    let new_count = approved_count + 1;
    if new_count >= level.required_approvers {
        if req.current_level + 1 >= req.total_levels {
            req.status = ApprovalStatus::Approved;
        } else {
            req.current_level += 1;
            req.status = ApprovalStatus::InProgress;
        }
    } else {
        req.status = ApprovalStatus::InProgress;
    }

    let result = req.clone();
    save_requests_raw(&requests)?;

    crate::utils::audit::log_action(
        "approval_approve",
        approver,
        "approval_request",
        &result.id,
        [
            ("workflow".to_string(), result.workflow_name.clone()),
            ("contract_id".to_string(), result.contract_id.clone()),
            ("level".to_string(), level.name.clone()),
            ("level_index".to_string(), level_index.to_string()),
            ("status".to_string(), result.status.to_string()),
        ]
        .into_iter()
        .collect(),
        true,
        None,
    )?;

    Ok(result)
}

pub fn reject_request(
    id: &str,
    approver: &str,
    reason: &str,
    roles: &[String],
) -> Result<ApprovalRequest> {
    let mut requests = load_requests_raw()?;
    let req = requests
        .iter_mut()
        .find(|r| r.id == id || r.id.starts_with(id))
        .ok_or_else(|| anyhow::anyhow!("Approval request '{}' not found", id))?;

    if req.status == ApprovalStatus::Approved {
        anyhow::bail!("Approval request '{}' is already approved", id);
    }
    if req.status == ApprovalStatus::Rejected {
        anyhow::bail!("Approval request '{}' is already rejected", id);
    }
    if req.status == ApprovalStatus::Cancelled {
        anyhow::bail!("Approval request '{}' is already cancelled", id);
    }
    if req.status == ApprovalStatus::Expired {
        anyhow::bail!("Approval request '{}' has expired", id);
    }

    let workflow = get_workflow(&req.workflow_id)?
        .ok_or_else(|| anyhow::anyhow!("Workflow '{}' not found for request", req.workflow_id))?;

    let level = &workflow.levels[req.current_level];
    let has_role = roles.is_empty() || roles.iter().any(|r| level.approver_roles.contains(r));
    if !has_role {
        anyhow::bail!(
            "Approver does not have required role for level '{}'. Required: {:?}",
            level.name,
            level.approver_roles
        );
    }

    req.actions.push(ApprovalAction {
        level_index: req.current_level,
        level_name: level.name.clone(),
        approver: approver.to_string(),
        action: ActionType::Reject,
        comment: Some(reason.to_string()),
        timestamp: Utc::now().to_rfc3339(),
    });
    req.status = ApprovalStatus::Rejected;
    req.updated_at = Utc::now().to_rfc3339();

    let result = req.clone();
    save_requests_raw(&requests)?;

    crate::utils::audit::log_action(
        "approval_reject",
        approver,
        "approval_request",
        &result.id,
        [
            ("workflow".to_string(), result.workflow_name.clone()),
            ("contract_id".to_string(), result.contract_id.clone()),
            ("level".to_string(), level.name.clone()),
            ("reason".to_string(), reason.to_string()),
        ]
        .into_iter()
        .collect(),
        true,
        None,
    )?;

    Ok(result)
}

pub fn cancel_request(id: &str, cancelled_by: &str, reason: &str) -> Result<ApprovalRequest> {
    let mut requests = load_requests_raw()?;
    let req = requests
        .iter_mut()
        .find(|r| r.id == id || r.id.starts_with(id))
        .ok_or_else(|| anyhow::anyhow!("Approval request '{}' not found", id))?;

    if req.status == ApprovalStatus::Approved {
        anyhow::bail!("Cannot cancel an already approved request");
    }
    if req.status == ApprovalStatus::Rejected {
        anyhow::bail!("Cannot cancel an already rejected request");
    }

    req.status = ApprovalStatus::Cancelled;
    req.updated_at = Utc::now().to_rfc3339();

    let result = req.clone();
    save_requests_raw(&requests)?;

    crate::utils::audit::log_action(
        "approval_cancel",
        cancelled_by,
        "approval_request",
        &result.id,
        [
            ("reason".to_string(), reason.to_string()),
            ("contract_id".to_string(), result.contract_id.clone()),
        ]
        .into_iter()
        .collect(),
        true,
        None,
    )?;

    Ok(result)
}

pub fn get_approval_summary() -> Result<ApprovalDashboardSummary> {
    let _ = process_expired_requests();
    let requests = load_requests_raw()?;

    let total = requests.len();
    let pending = requests
        .iter()
        .filter(|r| r.status == ApprovalStatus::Pending)
        .count();
    let in_progress = requests
        .iter()
        .filter(|r| r.status == ApprovalStatus::InProgress)
        .count();
    let approved = requests
        .iter()
        .filter(|r| r.status == ApprovalStatus::Approved)
        .count();
    let rejected = requests
        .iter()
        .filter(|r| r.status == ApprovalStatus::Rejected)
        .count();
    let expired = requests
        .iter()
        .filter(|r| r.status == ApprovalStatus::Expired)
        .count();
    let cancelled = requests
        .iter()
        .filter(|r| r.status == ApprovalStatus::Cancelled)
        .count();
    let total_workflows = load_workflows_raw()?.len();

    let mut by_network: HashMap<String, usize> = HashMap::new();
    for r in &requests {
        *by_network.entry(r.network.clone()).or_default() += 1;
    }

    let mut by_workflow: HashMap<String, usize> = HashMap::new();
    for r in &requests {
        *by_workflow.entry(r.workflow_name.clone()).or_default() += 1;
    }

    Ok(ApprovalDashboardSummary {
        total_requests: total,
        pending,
        in_progress,
        approved,
        rejected,
        expired,
        cancelled,
        total_workflows,
        by_network,
        by_workflow,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalDashboardSummary {
    pub total_requests: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub approved: usize,
    pub rejected: usize,
    pub expired: usize,
    pub cancelled: usize,
    pub total_workflows: usize,
    pub by_network: HashMap<String, usize>,
    pub by_workflow: HashMap<String, usize>,
}

pub fn build_default_workflows() -> Result<Vec<ApprovalWorkflow>> {
    let existing = load_workflows_raw()?;
    if !existing.is_empty() {
        return Ok(existing);
    }

    let mut created = vec![];

    let dev_workflow = create_workflow(
        "Development Deployment",
        "Simple single-level approval for development/testnet deployments",
        vec![ApprovalLevel {
            name: "team-lead".to_string(),
            description: "Team lead reviews and approves development deployments".to_string(),
            required_approvers: 1,
            approver_roles: vec!["team-lead".to_string(), "developer".to_string()],
            timeout_hours: Some(24),
        }],
    )?;
    created.push(dev_workflow);

    let prod_workflow = create_workflow(
        "Production Deployment",
        "Multi-level approval for production/mainnet deployments requiring team lead and manager approval",
        vec![
            ApprovalLevel {
                name: "team-lead".to_string(),
                description: "Team lead reviews code changes and deployment plan".to_string(),
                required_approvers: 1,
                approver_roles: vec!["team-lead".to_string()],
                timeout_hours: Some(48),
            },
            ApprovalLevel {
                name: "manager".to_string(),
                description: "Manager reviews business impact and compliance".to_string(),
                required_approvers: 1,
                approver_roles: vec!["manager".to_string(), "admin".to_string()],
                timeout_hours: Some(72),
            },
        ],
    )?;
    created.push(prod_workflow);

    let compliance_workflow = create_workflow(
        "Compliance Deployment",
        "Three-level approval for regulated environments requiring team lead, compliance officer, and admin",
        vec![
            ApprovalLevel {
                name: "team-lead".to_string(),
                description: "Team lead reviews technical correctness".to_string(),
                required_approvers: 1,
                approver_roles: vec!["team-lead".to_string()],
                timeout_hours: Some(48),
            },
            ApprovalLevel {
                name: "compliance-officer".to_string(),
                description: "Compliance officer reviews regulatory requirements".to_string(),
                required_approvers: 1,
                approver_roles: vec!["compliance-officer".to_string()],
                timeout_hours: Some(72),
            },
            ApprovalLevel {
                name: "admin".to_string(),
                description: "Admin gives final sign-off for production release".to_string(),
                required_approvers: 1,
                approver_roles: vec!["admin".to_string()],
                timeout_hours: Some(120),
            },
        ],
    )?;
    created.push(compliance_workflow);

    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_approval_status_display() {
        assert_eq!(ApprovalStatus::Pending.to_string(), "pending");
        assert_eq!(ApprovalStatus::Approved.to_string(), "approved");
        assert_eq!(ApprovalStatus::Rejected.to_string(), "rejected");
        assert_eq!(ApprovalStatus::Expired.to_string(), "expired");
    }

    #[test]
    fn test_action_type_display() {
        assert_eq!(ActionType::Approve.to_string(), "approve");
        assert_eq!(ActionType::Reject.to_string(), "reject");
        assert_eq!(ActionType::Escalate.to_string(), "escalate");
    }

    #[test]
    fn test_approval_level_progress() {
        let req = ApprovalRequest {
            id: "test-1".to_string(),
            workflow_id: "wf-1".to_string(),
            workflow_name: "Test".to_string(),
            total_levels: 1,
            contract_id: "CCONTRACT".to_string(),
            wasm_path: "test.wasm".to_string(),
            wasm_hash: "abcd".to_string(),
            network: "testnet".to_string(),
            description: "Test".to_string(),
            requested_by: "alice".to_string(),
            current_level: 0,
            status: ApprovalStatus::Pending,
            actions: vec![],
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
            expires_at: None,
            metadata: HashMap::new(),
        };
        assert_eq!(req.level_progress(), "0/1");
    }
}
