use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentPlan {
    pub id: String,
    pub name: String,
    pub description: String,
    pub contracts: Vec<ContractDeployment>,
    pub dependencies: HashMap<String, Vec<String>>,
    pub deployment_order: Vec<String>,
    pub rollback_plan: RollbackPlan,
    pub state: DeploymentState,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDeployment {
    pub id: String,
    pub name: String,
    pub wasm_path: PathBuf,
    pub contract_id: Option<String>,
    pub network: String,
    pub wallet: String,
    pub constructor_args: Vec<ConstructorArg>,
    pub dependencies: Vec<String>,
    pub status: ContractDeploymentStatus,
    pub deployed_at: Option<String>,
    pub deployment_tx: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstructorArg {
    pub name: String,
    pub value: String,
    pub arg_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContractDeploymentStatus {
    Pending,
    InProgress,
    Deployed,
    Failed,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackPlan {
    pub enabled: bool,
    pub rollback_order: Vec<String>,
    pub rollback_points: HashMap<String, RollbackPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackPoint {
    pub contract_id: String,
    pub previous_contract_id: Option<String>,
    pub state_hash: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeploymentState {
    Draft,
    Ready,
    InProgress,
    Completed,
    Failed,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentExecution {
    pub plan_id: String,
    pub execution_id: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub status: DeploymentState,
    pub results: Vec<DeploymentResult>,
    pub logs: Vec<DeploymentLog>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentResult {
    pub contract_id: String,
    pub success: bool,
    pub contract_address: Option<String>,
    pub transaction_hash: Option<String>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentLog {
    pub timestamp: String,
    pub level: LogLevel,
    pub contract_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
    Success,
}

pub struct OrchestrationEngine {
    plans_dir: PathBuf,
}

impl OrchestrationEngine {
    pub fn new() -> Result<Self> {
        let plans_dir = crate::utils::config::config_dir()
            .join("orchestration")
            .join("plans");

        if !plans_dir.exists() {
            fs::create_dir_all(&plans_dir)?;
        }

        Ok(Self { plans_dir })
    }

    pub fn create_plan(&self, name: &str, description: &str) -> Result<DeploymentPlan> {
        let plan = DeploymentPlan {
            id: format!("plan_{}", uuid::Uuid::new_v4()),
            name: name.to_string(),
            description: description.to_string(),
            contracts: vec![],
            dependencies: HashMap::new(),
            deployment_order: vec![],
            rollback_plan: RollbackPlan {
                enabled: true,
                rollback_order: vec![],
                rollback_points: HashMap::new(),
            },
            state: DeploymentState::Draft,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        self.save_plan(&plan)?;
        Ok(plan)
    }

    pub fn add_contract(&self, plan_id: &str, contract: ContractDeployment) -> Result<()> {
        let mut plan = self.load_plan(plan_id)?;

        plan.contracts.push(contract);
        plan.updated_at = chrono::Utc::now().to_rfc3339();

        self.save_plan(&plan)
    }

    pub fn add_dependency(&self, plan_id: &str, contract_id: &str, depends_on: &str) -> Result<()> {
        let mut plan = self.load_plan(plan_id)?;

        plan.dependencies
            .entry(contract_id.to_string())
            .or_insert_with(Vec::new)
            .push(depends_on.to_string());

        plan.updated_at = chrono::Utc::now().to_rfc3339();

        self.save_plan(&plan)
    }

    pub fn resolve_dependencies(&self, plan_id: &str) -> Result<Vec<String>> {
        let plan = self.load_plan(plan_id)?;

        let contract_ids: HashSet<String> = plan.contracts.iter().map(|c| c.id.clone()).collect();

        // Validate all dependencies exist
        for (contract_id, deps) in &plan.dependencies {
            if !contract_ids.contains(contract_id) {
                anyhow::bail!("Contract {} not found in plan", contract_id);
            }
            for dep in deps {
                if !contract_ids.contains(dep) {
                    anyhow::bail!("Dependency {} not found in plan", dep);
                }
            }
        }

        // Topological sort
        let mut sorted = Vec::new();
        let mut visited = HashSet::new();
        let mut temp_visited = HashSet::new();

        for contract_id in &contract_ids {
            self.topological_sort(
                contract_id,
                &plan.dependencies,
                &mut visited,
                &mut temp_visited,
                &mut sorted,
            )?;
        }

        Ok(sorted)
    }

    fn topological_sort(
        &self,
        contract_id: &str,
        dependencies: &HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
        temp_visited: &mut HashSet<String>,
        sorted: &mut Vec<String>,
    ) -> Result<()> {
        if visited.contains(contract_id) {
            return Ok(());
        }

        if temp_visited.contains(contract_id) {
            anyhow::bail!("Circular dependency detected involving {}", contract_id);
        }

        temp_visited.insert(contract_id.to_string());

        if let Some(deps) = dependencies.get(contract_id) {
            for dep in deps {
                self.topological_sort(dep, dependencies, visited, temp_visited, sorted)?;
            }
        }

        temp_visited.remove(contract_id);
        visited.insert(contract_id.to_string());
        sorted.push(contract_id.to_string());

        Ok(())
    }

    pub fn finalize_plan(&self, plan_id: &str) -> Result<()> {
        let mut plan = self.load_plan(plan_id)?;

        // Resolve deployment order
        plan.deployment_order = self.resolve_dependencies(plan_id)?;

        // Calculate rollback order (reverse of deployment order)
        plan.rollback_plan.rollback_order = plan.deployment_order.iter().rev().cloned().collect();

        plan.state = DeploymentState::Ready;
        plan.updated_at = chrono::Utc::now().to_rfc3339();

        self.save_plan(&plan)
    }

    pub fn execute_plan(&self, plan_id: &str) -> Result<DeploymentExecution> {
        let plan = self.load_plan(plan_id)?;

        if !matches!(plan.state, DeploymentState::Ready) {
            anyhow::bail!(
                "Plan is not ready for execution. Current state: {:?}",
                plan.state
            );
        }

        let execution_id = format!("exec_{}", uuid::Uuid::new_v4());
        let execution = Arc::new(Mutex::new(DeploymentExecution {
            plan_id: plan_id.to_string(),
            execution_id: execution_id.clone(),
            started_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            status: DeploymentState::InProgress,
            results: vec![],
            logs: vec![],
        }));

        let plan_clone = plan.clone();
        let execution_clone = Arc::clone(&execution);
        let execution_clone2 = Arc::clone(&execution);

        thread::spawn(move || {
            let result = Self::execute_deployment(&plan_clone, execution_clone2);

            if let Err(e) = result {
                let mut exec = execution_clone.lock().unwrap();
                exec.status = DeploymentState::Failed;
                exec.completed_at = Some(chrono::Utc::now().to_rfc3339());
                exec.logs.push(DeploymentLog {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    level: LogLevel::Error,
                    contract_id: "system".to_string(),
                    message: format!("Deployment failed: {}", e),
                });
            }
        });

        // Wait for execution to complete (simplified - in production would use async)
        thread::sleep(std::time::Duration::from_secs(1));

        let execution = execution.lock().unwrap().clone();
        Ok(execution)
    }

    fn execute_deployment(
        plan: &DeploymentPlan,
        execution: Arc<Mutex<DeploymentExecution>>,
    ) -> Result<()> {
        for contract_id in &plan.deployment_order {
            let contract = plan
                .contracts
                .iter()
                .find(|c| &c.id == contract_id)
                .ok_or_else(|| anyhow::anyhow!("Contract {} not found", contract_id))?;

            Self::deploy_contract(contract, &execution)?;
        }

        let mut exec = execution.lock().unwrap();
        exec.status = DeploymentState::Completed;
        exec.completed_at = Some(chrono::Utc::now().to_rfc3339());

        Ok(())
    }

    fn deploy_contract(
        contract: &ContractDeployment,
        execution: &Arc<Mutex<DeploymentExecution>>,
    ) -> Result<()> {
        let start = std::time::Instant::now();

        let mut exec = execution.lock().unwrap();
        exec.logs.push(DeploymentLog {
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: LogLevel::Info,
            contract_id: contract.id.clone(),
            message: format!("Starting deployment of {}", contract.name),
        });
        drop(exec);

        // Simulate deployment (in production would actually deploy)
        std::thread::sleep(std::time::Duration::from_millis(100));

        let duration = start.elapsed();

        let mut exec = execution.lock().unwrap();
        exec.results.push(DeploymentResult {
            contract_id: contract.id.clone(),
            success: true,
            contract_address: Some(format!("C{}", uuid::Uuid::new_v4())),
            transaction_hash: Some(format!("tx_{}", uuid::Uuid::new_v4())),
            error: None,
            duration_ms: duration.as_millis() as u64,
        });

        exec.logs.push(DeploymentLog {
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: LogLevel::Success,
            contract_id: contract.id.clone(),
            message: format!(
                "Successfully deployed {} in {}ms",
                contract.name,
                duration.as_millis()
            ),
        });

        Ok(())
    }

    pub fn rollback(&self, execution_id: &str) -> Result<DeploymentExecution> {
        let plan = self.load_plan_by_execution(execution_id)?;

        let execution = Arc::new(Mutex::new(DeploymentExecution {
            plan_id: plan.id.clone(),
            execution_id: format!("rollback_{}", uuid::Uuid::new_v4()),
            started_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            status: DeploymentState::InProgress,
            results: vec![],
            logs: vec![],
        }));

        let plan_clone = plan.clone();
        let execution_clone = Arc::clone(&execution);
        let execution_clone2 = Arc::clone(&execution);

        thread::spawn(move || {
            let result = Self::execute_rollback(&plan_clone, execution_clone2);

            if let Err(e) = result {
                let mut exec = execution_clone.lock().unwrap();
                exec.status = DeploymentState::Failed;
                exec.completed_at = Some(chrono::Utc::now().to_rfc3339());
                exec.logs.push(DeploymentLog {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    level: LogLevel::Error,
                    contract_id: "system".to_string(),
                    message: format!("Rollback failed: {}", e),
                });
            }
        });

        thread::sleep(std::time::Duration::from_secs(1));

        let execution = execution.lock().unwrap().clone();
        Ok(execution)
    }

    fn execute_rollback(
        plan: &DeploymentPlan,
        execution: Arc<Mutex<DeploymentExecution>>,
    ) -> Result<()> {
        for contract_id in &plan.rollback_plan.rollback_order {
            let contract = plan
                .contracts
                .iter()
                .find(|c| &c.id == contract_id)
                .ok_or_else(|| anyhow::anyhow!("Contract {} not found", contract_id))?;

            Self::rollback_contract(contract, &execution)?;
        }

        let mut exec = execution.lock().unwrap();
        exec.status = DeploymentState::RolledBack;
        exec.completed_at = Some(chrono::Utc::now().to_rfc3339());

        Ok(())
    }

    fn rollback_contract(
        contract: &ContractDeployment,
        execution: &Arc<Mutex<DeploymentExecution>>,
    ) -> Result<()> {
        let mut exec = execution.lock().unwrap();
        exec.logs.push(DeploymentLog {
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: LogLevel::Info,
            contract_id: contract.id.clone(),
            message: format!("Rolling back {}", contract.name),
        });
        drop(exec);

        // Simulate rollback
        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut exec = execution.lock().unwrap();
        exec.results.push(DeploymentResult {
            contract_id: contract.id.clone(),
            success: true,
            contract_address: None,
            transaction_hash: None,
            error: None,
            duration_ms: 50,
        });

        exec.logs.push(DeploymentLog {
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: LogLevel::Success,
            contract_id: contract.id.clone(),
            message: format!("Successfully rolled back {}", contract.name),
        });

        Ok(())
    }

    pub fn get_plan(&self, plan_id: &str) -> Result<DeploymentPlan> {
        self.load_plan(plan_id)
    }

    pub fn list_plans(&self) -> Result<Vec<DeploymentPlan>> {
        let mut plans = Vec::new();

        for entry in fs::read_dir(&self.plans_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map_or(false, |ext| ext == "json") {
                let content = fs::read_to_string(&path)?;
                let plan: DeploymentPlan = serde_json::from_str(&content)?;
                plans.push(plan);
            }
        }

        Ok(plans)
    }

    fn save_plan(&self, plan: &DeploymentPlan) -> Result<()> {
        let plan_path = self.plans_dir.join(format!("{}.json", plan.id));
        let json = serde_json::to_string_pretty(plan)?;
        fs::write(plan_path, json)?;
        Ok(())
    }

    fn load_plan(&self, plan_id: &str) -> Result<DeploymentPlan> {
        let plan_path = self.plans_dir.join(format!("{}.json", plan_id));
        let content = fs::read_to_string(&plan_path)?;
        let plan: DeploymentPlan = serde_json::from_str(&content)?;
        Ok(plan)
    }

    fn load_plan_by_execution(&self, execution_id: &str) -> Result<DeploymentPlan> {
        // In production, would store execution-to-plan mapping
        // For now, load the first plan
        let plans = self.list_plans()?;
        plans
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No plans found"))
    }
}

pub struct OrchestrationVisualizer;

impl OrchestrationVisualizer {
    pub fn generate_dependency_graph(plan: &DeploymentPlan) -> Result<String> {
        let mut dot = String::from("digraph DeploymentPlan {\n");
        dot.push_str("    rankdir=TB;\n");
        dot.push_str("    node [shape=box, style=rounded];\n");
        dot.push_str("    edge [dir=back];\n\n");

        // Add nodes
        for contract in &plan.contracts {
            let color = match contract.status {
                ContractDeploymentStatus::Pending => "lightgray",
                ContractDeploymentStatus::InProgress => "yellow",
                ContractDeploymentStatus::Deployed => "lightgreen",
                ContractDeploymentStatus::Failed => "lightcoral",
                ContractDeploymentStatus::RolledBack => "lightblue",
            };

            dot.push_str(&format!(
                "    \"{}\" [label=\"{}\\n{}\", fillcolor={}, style=filled];\n",
                contract.id, contract.name, contract.id, color
            ));
        }

        // Add edges
        for (contract_id, deps) in &plan.dependencies {
            for dep in deps {
                dot.push_str(&format!("    \"{}\" -> \"{}\";\n", contract_id, dep));
            }
        }

        dot.push_str("}\n");
        Ok(dot)
    }

    pub fn generate_execution_timeline(execution: &DeploymentExecution) -> Result<String> {
        let mut timeline = String::from("Deployment Execution Timeline\n");
        timeline.push_str(&format!("Execution ID: {}\n", execution.execution_id));
        timeline.push_str(&format!("Started: {}\n", execution.started_at));
        timeline.push_str(&format!("Status: {:?}\n\n", execution.status));

        timeline.push_str("Logs:\n");
        for log in &execution.logs {
            timeline.push_str(&format!(
                "  [{}] {:?} - {}: {}\n",
                log.timestamp, log.level, log.contract_id, log.message
            ));
        }

        timeline.push_str("\nResults:\n");
        for result in &execution.results {
            timeline.push_str(&format!(
                "  {} - Success: {}, Duration: {}ms\n",
                result.contract_id, result.success, result.duration_ms
            ));
            if let Some(address) = &result.contract_address {
                timeline.push_str(&format!("    Address: {}\n", address));
            }
            if let Some(tx) = &result.transaction_hash {
                timeline.push_str(&format!("    Transaction: {}\n", tx));
            }
            if let Some(error) = &result.error {
                timeline.push_str(&format!("    Error: {}\n", error));
            }
        }

        Ok(timeline)
    }
}
