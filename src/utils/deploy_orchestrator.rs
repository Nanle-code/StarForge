use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::config;
use crate::utils::progress::ProgressReporter;

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
    /// Contracts that must be deployed before this one (from the manifest).
    #[serde(default)]
    pub depends_on: Vec<String>,
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
            depends_on: contract.depends_on.clone(),
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

/// Default worker count when `--concurrency auto` is requested.
///
/// Bounded to 8 so a beefy dev machine cannot oversubscribe an RPC rate limit;
/// falls back to a single worker when the platform cannot report parallelism.
pub fn default_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().min(8))
        .unwrap_or(1)
}

/// Partition the plan into execution *waves*: step indices whose dependencies
/// resolve at the same depth. Steps in one wave never depend on each other and
/// can safely be deployed concurrently; waves themselves are strictly ordered.
///
/// Returns waves of step-index vectors ordered by dependency depth. A step with
/// no dependencies lands in wave 0.
pub fn compute_execution_levels(state: &DeploymentState) -> Result<Vec<Vec<usize>>> {
    let n = state.steps.len();
    let by_id: HashMap<&str, usize> = state
        .steps
        .iter()
        .enumerate()
        .map(|(idx, step)| (step.contract_id.as_str(), idx))
        .collect();

    // Validated dependency graph + depth per step (DFS with cycle detection).
    fn depth(
        idx: usize,
        by_id: &HashMap<&str, usize>,
        steps: &[DeployStep],
        memo: &mut HashMap<usize, usize>,
        stack: &mut HashSet<usize>,
    ) -> Result<usize> {
        if let Some(d) = memo.get(&idx) {
            return Ok(*d);
        }
        if !stack.insert(idx) {
            anyhow::bail!("Circular dependency detected in deployment plan");
        }
        let step = &steps[idx];
        let mut max_dep = 0usize;
        for dep in &step.depends_on {
            let dep_idx = by_id.get(dep.as_str()).ok_or_else(|| {
                anyhow::anyhow!(
                    "Contract '{}' depends on unknown contract '{}'",
                    step.contract_id,
                    dep
                )
            })?;
            max_dep = max_dep.max(depth(*dep_idx, by_id, steps, memo, stack)?);
        }
        stack.remove(&idx);
        memo.insert(idx, max_dep + 1);
        Ok(max_dep + 1)
    }

    let mut memo = HashMap::new();
    let mut stack = HashSet::new();
    let mut levels: Vec<Vec<usize>> = Vec::new();
    for idx in 0..n {
        let d = depth(idx, &by_id, &state.steps, &mut memo, &mut stack)?;
        let slot = d - 1;
        while levels.len() <= slot {
            levels.push(Vec::new());
        }
        levels[slot].push(idx);
    }

    for wave in &mut levels {
        wave.sort_unstable();
    }
    Ok(levels)
}

/// Deterministic mock deployment address for a WASM hash (shared by the
/// sequential and parallel executors so both paths produce identical output).
fn simulate_deploy_address(wasm_hash: &str, dry_run: bool) -> String {
    let prefix_len = 8.min(wasm_hash.len());
    let hash_prefix = &wasm_hash[..prefix_len];
    if dry_run {
        format!("C_SIMULATED_{}", hash_prefix)
    } else {
        format!("C_LIVE_{}", hash_prefix)
    }
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

    let reporter = ProgressReporter::new(state.steps.len());
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

        reporter.started(i + 1, state.steps[i].contract_id.clone());
        state.steps[i].status = DeployStepStatus::Running;
        save_state(state)?;

        let wasm_hash = state.steps[i].wasm_hash.clone();

        state.steps[i].deployed_address = Some(simulate_deploy_address(&wasm_hash, dry_run));
        state.steps[i].status = DeployStepStatus::Deployed;
        state.updated_at = Utc::now().to_rfc3339();
        save_state(state)?;
        reporter.completed(
            i + 1,
            state.steps[i].contract_id.clone(),
            if dry_run { "simulated" } else { "deployed" },
        );
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

/// Bounded parallel deployment execution.
///
/// Deploys independent contracts concurrently (up to `concurrency` workers),
/// while keeping dependency ordering exact. Wave boundaries are computed from
/// `depends_on`; a worker pool drains each wave, and state is persisted
/// sequentially after every step so the checkpoint guarantees match the
/// sequential [`execute_plan`] path.
///
/// `concurrency` is clamped to `[1, max(1, steps.len())]`; a value of `1`
/// behaves identically to the sequential path.
pub fn execute_plan_parallel(
    state: &mut DeploymentState,
    dry_run: bool,
    concurrency: usize,
) -> Result<()> {
    let _lock = crate::utils::deployment_checkpoint::DeploymentLock::acquire(&format!(
        "orchestrate_{}",
        state.id
    ))?;

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

    let waves = compute_execution_levels(state)?;
    let workers = concurrency.clamp(1, state.steps.len().max(1));

    let started = std::time::Instant::now();
    let reporter = ProgressReporter::new(state.steps.len());
    let mut deployed_count = 0usize;

    for (wave_idx, wave) in waves.iter().enumerate() {
        // Vec of (step_index, contract_name) for address simulation.
        let tasks: Vec<(usize, String)> = wave
            .iter()
            .filter(|&&idx| state.steps[idx].status != DeployStepStatus::Deployed)
            .map(|&idx| (idx, state.steps[idx].wasm_hash.clone()))
            .collect();

        if tasks.is_empty() {
            continue;
        }

        let cursor = std::sync::atomic::AtomicUsize::new(0);
        let next_index = || cursor.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let thread_results: Vec<Vec<(usize, String, Option<String>)>> =
            std::thread::scope(|scope| {
                let mut handles = Vec::new();
                for _ in 0..workers {
                    let tasks = tasks.clone();
                    handles.push(scope.spawn(move || {
                        let mut results = Vec::new();
                        while let Some(task) = tasks.get(next_index()) {
                            let address = simulate_deploy_address(&task.1, dry_run);
                            results.push((task.0, address, None::<String>));
                        }
                        results
                    }));
                }
                handles
                    .into_iter()
                    .map(|handle| {
                        handle.join().map_err(|_| {
                            anyhow::anyhow!(
                                "Deployment worker thread panicked while planning wave {}",
                                wave_idx + 1
                            )
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            })?;

        for (idx, address, error) in thread_results.into_iter().flatten() {
            reporter.started(idx + 1, state.steps[idx].contract_id.clone());
            if error.is_some() {
                state.steps[idx].status = DeployStepStatus::Failed;
                state.steps[idx].error = error;
                reporter.failed(
                    idx + 1,
                    state.steps[idx].contract_id.clone(),
                    state.steps[idx]
                        .error
                        .clone()
                        .unwrap_or_else(|| "worker failed".into()),
                );
            } else {
                state.steps[idx].status = DeployStepStatus::Running;
                // Apply the (deterministic) simulated result back on the
                // orchestrator thread so state writes stay serialized.
                state.steps[idx].deployed_address = Some(address);
                state.steps[idx].status = DeployStepStatus::Deployed;
                deployed_count += 1;
                reporter.completed(
                    idx + 1,
                    state.steps[idx].contract_id.clone(),
                    if dry_run { "simulated" } else { "deployed" },
                );
            }
            state.updated_at = Utc::now().to_rfc3339();
            save_state(state)?;
        }
    }

    let elapsed = started.elapsed();

    state.status = if dry_run {
        "simulated-complete".into()
    } else {
        "complete".into()
    };
    state.updated_at = Utc::now().to_rfc3339();
    save_state(state)?;

    crate::utils::print::success(&format!(
        "Deployment '{id}' completed: {deployed_count} contract(s) across {levels} parallel wave(s) with {workers} worker(s) in {elapsed:.2?}",
        id = state.id,
        levels = waves.len(),
    ));

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

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, depends_on: &[&str]) -> DeployStep {
        DeployStep {
            contract_id: id.to_string(),
            wasm: PathBuf::from(format!("{id}.wasm")),
            wasm_hash: format!("hash_{id}"),
            status: DeployStepStatus::Pending,
            deployed_address: None,
            error: None,
            order: 0,
            depends_on: depends_on.iter().map(|d| d.to_string()).collect(),
        }
    }

    fn state(steps: Vec<DeployStep>) -> DeploymentState {
        DeploymentState {
            id: "test".into(),
            manifest_name: "test".into(),
            network: "testnet".into(),
            created_at: String::new(),
            updated_at: String::new(),
            status: "planned".into(),
            steps,
        }
    }

    #[test]
    fn execution_levels_independent_contracts_share_a_wave() {
        let s = state(vec![step("a", &[]), step("b", &[]), step("c", &[])]);
        let waves = compute_execution_levels(&s).unwrap();
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0], vec![0, 1, 2]);
    }

    #[test]
    fn execution_levels_respect_dependency_chain() {
        let s = state(vec![step("a", &[]), step("b", &["a"]), step("c", &["b"])]);
        let waves = compute_execution_levels(&s).unwrap();
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0], vec![0]);
        assert_eq!(waves[1], vec![1]);
        assert_eq!(waves[2], vec![2]);
    }

    #[test]
    fn execution_levels_handles_diamond_and_siblings() {
        // a -> b, a -> c, b -> d, c -> d
        let s = state(vec![
            step("a", &[]),
            step("b", &["a"]),
            step("c", &["a"]),
            step("d", &["b", "c"]),
        ]);
        let waves = compute_execution_levels(&s).unwrap();
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0], vec![0]); // a
        assert_eq!(waves[1], vec![1, 2]); // b + c (concurrent)
        assert_eq!(waves[2], vec![3]); // d
    }

    #[test]
    fn execution_levels_orders_tasks_sorted_within_level() {
        // Two independent contracts listed last in the manifest still land in wave 0.
        let s = state(vec![step("z_dep", &["a"]), step("a", &[]), step("b", &[])]);
        let waves = compute_execution_levels(&s).unwrap();
        assert_eq!(waves[0], vec![1, 2]);
        assert_eq!(waves[1], vec![0]);
    }

    #[test]
    fn execution_levels_detects_cycles() {
        let s = state(vec![step("a", &["b"]), step("b", &["a"])]);
        assert!(compute_execution_levels(&s).is_err());
    }

    #[test]
    fn execution_levels_rejects_unknown_dependency() {
        let s = state(vec![step("a", &[]), step("b", &["ghost"])]);
        assert!(compute_execution_levels(&s).is_err());
    }

    #[test]
    fn simulated_addresses_match_executors() {
        let hash = "0123456789abcdef";
        assert_eq!(
            simulate_deploy_address(hash, true),
            format!("C_SIMULATED_{}", &hash[..8])
        );
        assert_eq!(
            simulate_deploy_address(hash, false),
            format!("C_LIVE_{}", &hash[..8])
        );
    }

    #[test]
    fn default_concurrency_is_at_least_one() {
        assert!(default_concurrency() >= 1);
    }
}
