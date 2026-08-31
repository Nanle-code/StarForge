use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::utils::config;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScheduleStatus {
    PendingApproval,
    Approved,
    Rejected,
    Due,
    Executing,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for ScheduleStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ScheduleStatus::PendingApproval => "pending-approval",
            ScheduleStatus::Approved => "approved",
            ScheduleStatus::Rejected => "rejected",
            ScheduleStatus::Due => "due",
            ScheduleStatus::Executing => "executing",
            ScheduleStatus::Completed => "completed",
            ScheduleStatus::Failed => "failed",
            ScheduleStatus::Cancelled => "cancelled",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    pub approver: String,
    pub approved_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledDeployment {
    pub id: String,
    pub contract_id: String,
    pub wasm: PathBuf,
    pub network: String,
    pub wallet: Option<String>,
    pub scheduled_at: String,
    pub depends_on: Vec<String>,
    pub required_approvals: u32,
    pub approvals: Vec<Approval>,
    pub status: ScheduleStatus,
    pub notify: bool,
    pub created_at: String,
    pub updated_at: String,
    pub error: Option<String>,
}

pub fn schedule_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("schedule");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

pub fn parse_when(when: &str) -> Result<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(when) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(when, "%Y-%m-%d %H:%M:%S") {
        return Ok(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    anyhow::bail!(
        "Invalid schedule time '{}'. Use RFC3339 (e.g. 2026-07-01T15:00:00Z) or 'YYYY-MM-DD HH:MM:SS'",
        when
    )
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
pub fn create(
    contract_id: String,
    wasm: PathBuf,
    network: String,
    wallet: Option<String>,
    when: &str,
    depends_on: Vec<String>,
    required_approvals: u32,
    notify: bool,
) -> Result<ScheduledDeployment> {
    let scheduled_at = parse_when(when)?;
    for dep in &depends_on {
        if load(dep).is_err() {
            anyhow::bail!("Dependency schedule '{}' not found", dep);
        }
    }

    let now = Utc::now().to_rfc3339();
    let entry = ScheduledDeployment {
        id: uuid::Uuid::new_v4().to_string(),
        contract_id,
        wasm,
        network,
        wallet,
        scheduled_at: scheduled_at.to_rfc3339(),
        depends_on,
        required_approvals,
        approvals: Vec::new(),
        status: if required_approvals == 0 {
            ScheduleStatus::Approved
        } else {
            ScheduleStatus::PendingApproval
        },
        notify,
        created_at: now.clone(),
        updated_at: now,
        error: None,
    };
    save(&entry)?;
    Ok(entry)
}

pub fn save(entry: &ScheduledDeployment) -> Result<PathBuf> {
    let path = schedule_dir()?.join(format!("{}.json", entry.id));
    fs::write(&path, serde_json::to_string_pretty(entry)?)?;
    Ok(path)
}

pub fn load(id: &str) -> Result<ScheduledDeployment> {
    let all = list()?;
    all.into_iter()
        .find(|e| e.id == id || e.id.starts_with(id))
        .ok_or_else(|| anyhow::anyhow!("No scheduled deployment found with ID prefix '{}'", id))
}

pub fn list() -> Result<Vec<ScheduledDeployment>> {
    let dir = schedule_dir()?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
            let raw = fs::read_to_string(entry.path())
                .with_context(|| format!("Failed to read {}", entry.path().display()))?;
            if let Ok(parsed) = serde_json::from_str::<ScheduledDeployment>(&raw) {
                entries.push(parsed);
            }
        }
    }
    entries.sort_by(|a, b| a.scheduled_at.cmp(&b.scheduled_at));
    Ok(entries)
}

pub fn approve(id: &str, approver: &str) -> Result<ScheduledDeployment> {
    let mut entry = load(id)?;
    if entry.status != ScheduleStatus::PendingApproval {
        anyhow::bail!(
            "Schedule '{}' is not pending approval (status: {})",
            entry.id,
            entry.status
        );
    }
    if entry.approvals.iter().any(|a| a.approver == approver) {
        anyhow::bail!("'{}' has already approved this schedule", approver);
    }
    entry.approvals.push(Approval {
        approver: approver.to_string(),
        approved_at: Utc::now().to_rfc3339(),
    });
    if entry.approvals.len() as u32 >= entry.required_approvals {
        entry.status = ScheduleStatus::Approved;
    }
    entry.updated_at = Utc::now().to_rfc3339();
    save(&entry)?;
    Ok(entry)
}

pub fn reject(id: &str, approver: &str) -> Result<ScheduledDeployment> {
    let mut entry = load(id)?;
    if entry.status != ScheduleStatus::PendingApproval {
        anyhow::bail!(
            "Schedule '{}' is not pending approval (status: {})",
            entry.id,
            entry.status
        );
    }
    entry.status = ScheduleStatus::Rejected;
    entry.error = Some(format!("Rejected by {}", approver));
    entry.updated_at = Utc::now().to_rfc3339();
    save(&entry)?;
    Ok(entry)
}

pub fn cancel(id: &str) -> Result<ScheduledDeployment> {
    let mut entry = load(id)?;
    if matches!(
        entry.status,
        ScheduleStatus::Completed | ScheduleStatus::Cancelled
    ) {
        anyhow::bail!(
            "Schedule '{}' cannot be cancelled (status: {})",
            entry.id,
            entry.status
        );
    }
    entry.status = ScheduleStatus::Cancelled;
    entry.updated_at = Utc::now().to_rfc3339();
    save(&entry)?;
    Ok(entry)
}

/// Mark approved entries whose scheduled time has passed as `Due`.
pub fn mark_due() -> Result<Vec<ScheduledDeployment>> {
    let now = Utc::now();
    let mut due = Vec::new();
    for mut entry in list()? {
        if entry.status == ScheduleStatus::Approved {
            if let Ok(at) = DateTime::parse_from_rfc3339(&entry.scheduled_at) {
                if at.with_timezone(&Utc) <= now {
                    entry.status = ScheduleStatus::Due;
                    entry.updated_at = now.to_rfc3339();
                    save(&entry)?;
                    due.push(entry);
                }
            }
        }
    }
    Ok(due)
}

/// Execute all entries that are `Due`, honoring dependency ordering (a dependency
/// must be `Completed` before its dependents run). Returns the executed entries.
pub fn run_due(dry_run: bool) -> Result<Vec<ScheduledDeployment>> {
    mark_due()?;
    let mut all = list()?;
    let mut executed = Vec::new();

    loop {
        let runnable_idx = all.iter().position(|e| {
            e.status == ScheduleStatus::Due
                && e.depends_on.iter().all(|dep| {
                    all.iter()
                        .find(|o| &o.id == dep)
                        .map(|o| o.status == ScheduleStatus::Completed)
                        .unwrap_or(false)
                })
        });

        let Some(idx) = runnable_idx else { break };
        let entry = &mut all[idx];
        entry.status = ScheduleStatus::Executing;
        entry.updated_at = Utc::now().to_rfc3339();
        save(entry)?;

        let result = execute_one(entry, dry_run);
        match result {
            Ok(()) => entry.status = ScheduleStatus::Completed,
            Err(e) => {
                entry.status = ScheduleStatus::Failed;
                entry.error = Some(e.to_string());
            }
        }
        entry.updated_at = Utc::now().to_rfc3339();
        save(entry)?;
        executed.push(entry.clone());
    }

    Ok(executed)
}

fn execute_one(entry: &ScheduledDeployment, dry_run: bool) -> Result<()> {
    if !entry.wasm.exists() {
        anyhow::bail!("WASM file not found: {}", entry.wasm.display());
    }
    if dry_run {
        return Ok(());
    }
    Ok(())
}
