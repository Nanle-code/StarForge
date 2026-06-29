use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub action: String,
    pub actor: String,
    pub resource_type: String,
    pub resource_id: String,
    pub timestamp: String,
    pub details: std::collections::HashMap<String, String>,
    pub success: bool,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    pub period_start: String,
    pub period_end: String,
    pub total_actions: usize,
    pub successful_actions: usize,
    pub failed_actions: usize,
    pub unique_actors: usize,
    pub unique_resources: usize,
    pub entries: Vec<AuditEntry>,
}

fn audit_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let dir = home.join(".starforge").join("audit");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn audit_log_file() -> Result<PathBuf> {
    Ok(audit_dir()?.join("audit.json"))
}

pub fn load_audit_log() -> Result<Vec<AuditEntry>> {
    let path = audit_log_file()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

pub fn save_audit_log(entries: &[AuditEntry]) -> Result<()> {
    fs::write(audit_log_file()?, serde_json::to_string_pretty(entries)?)?;
    Ok(())
}

pub fn log_action(
    action: &str,
    actor: &str,
    resource_type: &str,
    resource_id: &str,
    details: std::collections::HashMap<String, String>,
    success: bool,
    error_message: Option<String>,
) -> Result<()> {
    let mut entries = load_audit_log()?;

    let entry = AuditEntry {
        id: format!("audit-{}", Utc::now().timestamp_millis()),
        action: action.to_string(),
        actor: actor.to_string(),
        resource_type: resource_type.to_string(),
        resource_id: resource_id.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        details,
        success,
        error_message,
    };

    entries.push(entry);
    save_audit_log(&entries)?;
    Ok(())
}

pub fn log_deployment(
    actor: &str,
    contract_id: &str,
    network: &str,
    wasm_hash: &str,
    fee: u64,
    success: bool,
    tx_hash: Option<&str>,
) -> Result<()> {
    let mut details = std::collections::HashMap::new();
    details.insert("network".to_string(), network.to_string());
    details.insert("wasm_hash".to_string(), wasm_hash.to_string());
    details.insert("fee_stroops".to_string(), fee.to_string());
    if let Some(hash) = tx_hash {
        details.insert("tx_hash".to_string(), hash.to_string());
    }

    log_action(
        "deploy_contract",
        actor,
        "contract",
        contract_id,
        details,
        success,
        None,
    )
}

pub fn get_deployment_history(contract_id: Option<&str>, limit: usize) -> Result<Vec<AuditEntry>> {
    let entries = load_audit_log()?;
    let filtered: Vec<_> = entries
        .iter()
        .filter(|e| e.action == "deploy_contract")
        .filter(|e| contract_id.is_none_or(|c| e.resource_id == c))
        .cloned()
        .collect();

    let mut result: Vec<_> = filtered.iter().rev().take(limit).cloned().collect();
    result.reverse();
    Ok(result)
}

pub fn get_audit_report(start_time: Option<&str>, end_time: Option<&str>) -> Result<AuditReport> {
    let entries = load_audit_log()?;

    let filtered: Vec<_> = entries
        .iter()
        .filter(|e| {
            if let Some(start) = start_time {
                if e.timestamp.as_str() < start {
                    return false;
                }
            }
            if let Some(end) = end_time {
                if e.timestamp.as_str() > end {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect();

    let total = filtered.len();
    let successful = filtered.iter().filter(|e| e.success).count();
    let failed = total - successful;

    let mut actors = std::collections::HashSet::new();
    let mut resources = std::collections::HashSet::new();
    for e in &filtered {
        actors.insert(e.actor.clone());
        resources.insert(format!("{}:{}", e.resource_type, e.resource_id));
    }

    Ok(AuditReport {
        period_start: start_time.unwrap_or("").to_string(),
        period_end: end_time.unwrap_or("").to_string(),
        total_actions: total,
        successful_actions: successful,
        failed_actions: failed,
        unique_actors: actors.len(),
        unique_resources: resources.len(),
        entries: filtered,
    })
}

pub fn export_audit_log_csv(entries: &[AuditEntry]) -> String {
    let mut csv =
        String::from("id,action,actor,resource_type,resource_id,timestamp,success,error_message\n");
    for entry in entries {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            entry.id,
            entry.action,
            entry.actor,
            entry.resource_type,
            entry.resource_id,
            entry.timestamp,
            entry.success,
            entry.error_message.as_deref().unwrap_or("")
        ));
    }
    csv
}

pub fn check_compliance_violations() -> Result<Vec<String>> {
    let entries = load_audit_log()?;
    let mut violations = Vec::new();

    let recent_cutoff = Utc::now() - chrono::Duration::days(30);

    for entry in entries.iter().filter(|e| {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&e.timestamp) {
            DateTime::<Utc>::from(dt) > recent_cutoff
        } else {
            false
        }
    }) {
        if !entry.success {
            violations.push(format!(
                "Failed deployment by {} on contract {}: {}",
                entry.actor,
                entry.resource_id,
                entry.error_message.as_deref().unwrap_or("unknown error")
            ));
        }
    }

    Ok(violations)
}
