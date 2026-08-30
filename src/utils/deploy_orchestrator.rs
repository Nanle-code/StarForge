use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::config;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeployManifest {
    pub name: String,
    pub network: String,
    #[serde(default)]
    pub wallet: Option<String>,
    pub contracts: Vec<ManifestContract>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManifestContract {
    pub id: String,
    pub wasm: PathBuf,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub init_args: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeployStepStatus {
    Pending,
    Running,
    Deployed,
    Failed,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployStep {
    pub contract_id: String,
    pub wasm: PathBuf,
    pub wasm_hash: String,
    pub status: DeployStepStatus,
    pub deployed_address: Option<String>,
    pub error: Option<String>,
    pub order: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentState {
    pub id: String,
    pub manifest_name: String,
    pub network: String,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
    pub steps: Vec<DeployStep>,
}

pub fn load_manifest(path: &Path) -> Result<DeployManifest> {
    config::validate_file_path(path, Some("json"))?;
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read manifest: {}", path.display()))?;
    let manifest: DeployManifest =
        serde_json::from_str(&raw).context("Invalid deploy manifest JSON")?;
    if manifest.contracts.is_empty() {
        anyhow::bail!("Manifest must contain at least one contract");
    }
    Ok(manifest)
}

pub fn resolve_order(manifest: &DeployManifest) -> Result<Vec<String>> {
    let ids: HashSet<_> = manifest.contracts.iter().map(|c| c.id.clone()).collect();
    for contract in &manifest.contracts {
        for dep in &contract.depends_on {
            if !ids.contains(dep) {
                anyhow::bail!(
                    "Contract '{}' depends on unknown contract '{}'",
                    contract.id,
                    dep
                );
            }
            if dep == &contract.id {
                anyhow::bail!("Contract '{}' cannot depend on itself", contract.id);
            }
        }
    }

    let mut in_degree: HashMap<String, usize> = ids.iter().map(|id| (id.clone(), 0)).collect();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();

    for contract in &manifest.contracts {
        for dep in &contract.depends_on {
            adj.entry(dep.clone())
                .or_default()
                .push(contract.id.clone());
            *in_degree.get_mut(&contract.id).unwrap() += 1;
        }
    }

    let mut queue: VecDeque<String> = in_degree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(id, _)| id.clone())
        .collect();
    queue.make_contiguous().sort();

    let mut order = Vec::new();
    while let Some(node) = queue.pop_front() {
        order.push(node.clone());
        if let Some(neighbors) = adj.get(&node) {
            for next in neighbors {
                let deg = in_degree.get_mut(next).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(next.clone());
                }
            }
        }
    }

    if order.len() != manifest.contracts.len() {
        anyhow::bail!("Circular dependency detected in deployment manifest");
    }

    Ok(order)
}

pub fn build_plan(manifest: &DeployManifest) -> Result<DeploymentState> {
    let order = resolve_order(manifest)?;
    let mut steps = Vec::new();

    for (idx, contract_id) in order.iter().enumerate() {
        let contract = manifest
            .contracts
            .iter()
            .find(|c| &c.id == contract_id)
            .unwrap();
        let bytes = fs::read(&contract.wasm)
            .with_context(|| format!("Failed to read WASM: {}", contract.wasm.display()))?;
        if bytes.len() < 4 || &bytes[..4] != b"\0asm" {
            anyhow::bail!(
                "Contract '{}': invalid WASM at {}",
                contract.id,
                contract.wasm.display()
            );
        }
        let hash = hex::encode(Sha256::digest(&bytes));
        steps.push(DeployStep {
            contract_id: contract.id.clone(),
            wasm: contract.wasm.clone(),
            wasm_hash: hash,
            status: DeployStepStatus::Pending,
            deployed_address: None,
            error: None,
            order: idx as u32 + 1,
        });
    }

    let now = Utc::now().to_rfc3339();
    Ok(DeploymentState {
        id: uuid::Uuid::new_v4().to_string(),
        manifest_name: manifest.name.clone(),
        network: manifest.network.clone(),
        created_at: now.clone(),
        updated_at: now,
        status: "planned".into(),
        steps,
    })
}

pub fn deployments_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("deployments");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

pub fn save_state(state: &DeploymentState) -> Result<PathBuf> {
    let path = deployments_dir()?.join(format!("{}.json", state.id));
    fs::write(&path, serde_json::to_string_pretty(state)?)?;
    Ok(path)
}

pub fn load_state(id: &str) -> Result<DeploymentState> {
    let path = deployments_dir()?.join(format!("{}.json", id));
    if !path.exists() {
        anyhow::bail!("Deployment state '{}' not found", id);
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn list_states() -> Result<Vec<DeploymentState>> {
    let dir = deployments_dir()?;
    let mut states = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
            if let Ok(raw) = fs::read_to_string(entry.path()) {
                if let Ok(state) = serde_json::from_str::<DeploymentState>(&raw) {
                    states.push(state);
                }
            }
        }
    }
    states.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(states)
}

/// Simulate deployment execution (dry-run). Marks steps as deployed with mock addresses.
/// Resumes from the last uncompleted step if previous steps were already deployed.
pub fn execute_plan(state: &mut DeploymentState, dry_run: bool) -> Result<()> {
    let _lock = crate::utils::deployment_checkpoint::DeploymentLock::acquire(&format!(
        "orchestrate_{}",
        state.id
    ))?;

    // Idempotency check: if all steps are already Deployed
    let all_deployed = !state.steps.is_empty()
        && state
            .steps
            .iter()
            .all(|s| s.status == DeployStepStatus::Deployed);
    if all_deployed {
        crate::utils::print::info(&format!(
            "[checkpoint] All contracts in deployment plan '{}' are already deployed.",
            state.id
        ));
        state.status = if dry_run {
            "simulated-complete".into()
        } else {
            "complete".into()
        };
        state.updated_at = Utc::now().to_rfc3339();
        save_state(state)?;
        return Ok(());
    }

    state.status = if dry_run {
        "simulated".into()
    } else {
        "executing".into()
    };
    state.updated_at = Utc::now().to_rfc3339();
    save_state(state)?;

    for i in 0..state.steps.len() {
        if state.steps[i].status == DeployStepStatus::Deployed {
            crate::utils::print::info(&format!(
                "[checkpoint] Contract '{}' already deployed ({}), skipping.",
                state.steps[i].contract_id,
                state.steps[i]
                    .deployed_address
                    .as_deref()
                    .unwrap_or("active")
            ));
            continue;
        }

        state.steps[i].status = DeployStepStatus::Running;
        save_state(state)?;

        let wasm_hash = state.steps[i].wasm_hash.clone();
        let prefix_len = 8.min(wasm_hash.len());
        let hash_prefix = &wasm_hash[..prefix_len];

        if dry_run {
            state.steps[i].deployed_address = Some(format!("C_SIMULATED_{}", hash_prefix));
            state.steps[i].status = DeployStepStatus::Deployed;
        } else {
            state.steps[i].deployed_address = Some(format!("C_LIVE_{}", hash_prefix));
            state.steps[i].status = DeployStepStatus::Deployed;
        }
        state.updated_at = Utc::now().to_rfc3339();
        save_state(state)?;
    }

    state.status = if dry_run {
        "simulated-complete".into()
    } else {
        "complete".into()
    };
    state.updated_at = Utc::now().to_rfc3339();
    save_state(state)?;
    Ok(())
}

/// Roll back deployed steps in reverse order.
pub fn rollback(state: &mut DeploymentState) -> Result<Vec<String>> {
    let mut rolled_back = Vec::new();
    for step in state.steps.iter_mut().rev() {
        if step.status == DeployStepStatus::Deployed {
            step.status = DeployStepStatus::RolledBack;
            step.deployed_address = None;
            rolled_back.push(step.contract_id.clone());
        }
    }
    state.status = "rolled-back".into();
    state.updated_at = Utc::now().to_rfc3339();
    save_state(state)?;
    Ok(rolled_back)
}

pub fn render_dag(manifest: &DeployManifest) -> Result<String> {
    let order = resolve_order(manifest)?;
    let mut lines = vec![
        format!("Deployment: {}", manifest.name),
        format!("Network: {}", manifest.network),
        String::new(),
        "Dependency Graph (execution order):".into(),
    ];

    for (idx, id) in order.iter().enumerate() {
        let contract = manifest.contracts.iter().find(|c| &c.id == id).unwrap();
        let deps = if contract.depends_on.is_empty() {
            "none".to_string()
        } else {
            contract.depends_on.join(", ")
        };
        lines.push(format!("  {}. {} ← depends on [{}]", idx + 1, id, deps));
    }

    lines.push(String::new());
    lines.push("Mermaid diagram:".into());
    lines.push("```mermaid".into());
    lines.push("graph TD".into());
    for contract in &manifest.contracts {
        for dep in &contract.depends_on {
            lines.push(format!("    {} --> {}", dep, contract.id));
        }
        if contract.depends_on.is_empty() {
            lines.push(format!("    START --> {}", contract.id));
        }
    }
    lines.push("```".into());

    Ok(lines.join("\n"))
}
