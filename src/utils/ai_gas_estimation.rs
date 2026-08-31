use crate::utils::config;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiGasEstimate {
    pub network: String,
    pub contract_name: String,
    pub total_estimated_cost: u64,
    pub accuracy_confidence: f64,
    pub function_costs: HashMap<String, u64>,
    pub expensive_operations: Vec<ExpensiveOperation>,
    pub optimization_suggestions: Vec<String>,
    pub estimated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpensiveOperation {
    pub operation: String,
    pub location: String,
    pub cost_impact: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiGasHistoryEntry {
    pub id: String,
    pub estimate: AiGasEstimate,
}

pub struct AiGasEstimator {
    // Not currently called from any code path in this crate. Kept rather than
    // removed since deleting it is a product decision, not a lint-scoping one.
    #[allow(dead_code)]
    model_version: String,
}

impl Default for AiGasEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl AiGasEstimator {
    pub fn new() -> Self {
        Self {
            model_version: "v1.0.0-ai-gas".to_string(),
        }
    }

    pub fn estimate(&self, wasm_path: &Path, network: &str) -> anyhow::Result<AiGasEstimate> {
        // Mocking an AI estimation
        let mut function_costs = HashMap::new();
        function_costs.insert("init".to_string(), 1500);
        function_costs.insert("execute".to_string(), 5000);
        function_costs.insert("update_state".to_string(), 3500);

        let expensive_operations = vec![
            ExpensiveOperation {
                operation: "Storage Write".to_string(),
                location: "update_state (line 42)".to_string(),
                cost_impact: 2000,
            },
            ExpensiveOperation {
                operation: "Complex Loop".to_string(),
                location: "execute (line 112)".to_string(),
                cost_impact: 1800,
            },
        ];

        let suggestions = vec![
            "Batch storage writes in `update_state` to reduce I/O costs.".to_string(),
            "Optimize the loop in `execute` to O(1) mathematical calculation if possible."
                .to_string(),
            "Consider using a smaller data type for the struct fields in `init`.".to_string(),
        ];

        let estimate = AiGasEstimate {
            network: network.to_string(),
            contract_name: wasm_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            total_estimated_cost: 10000,
            accuracy_confidence: 0.94,
            function_costs,
            expensive_operations,
            optimization_suggestions: suggestions,
            estimated_at: Utc::now().to_rfc3339(),
        };

        // Track historical cost
        self.save_history(&estimate)?;

        Ok(estimate)
    }

    fn history_path(&self) -> PathBuf {
        config::config_dir().join("ai_gas_history.json")
    }

    pub fn load_history(&self) -> anyhow::Result<Vec<AiGasHistoryEntry>> {
        let path = self.history_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&data).unwrap_or_default())
    }

    fn save_history(&self, estimate: &AiGasEstimate) -> anyhow::Result<()> {
        let mut history = self.load_history().unwrap_or_default();
        history.push(AiGasHistoryEntry {
            id: Uuid::new_v4().to_string(),
            estimate: estimate.clone(),
        });

        let path = self.history_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, serde_json::to_string_pretty(&history)?)?;
        Ok(())
    }
}
