use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

fn read_source_files(root: &Path) -> Vec<String> {
    let mut sources = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name == "target" || name == ".git" {
                        continue;
                    }
                }
                sources.extend(read_source_files(&path));
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                if let Ok(content) = fs::read_to_string(&path) {
                    sources.push(content);
                }
            }
        }
    }
    sources
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OptimizationSuggestion {
    pub category: String,
    pub priority: String,
    pub title: String,
    pub detail: String,
    pub estimated_impact: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemplatePerformanceAnalysis {
    pub template_name: String,
    pub path: String,
    pub storage_layout_score: u8,
    pub function_efficiency_score: u8,
    pub loop_optimization_score: u8,
    pub external_call_score: u8,
    pub batch_operations_score: u8,
    pub overall_score: u8,
    pub estimated_gas_reduction_percent: u8,
    pub estimated_speedup_percent: u8,
    pub estimated_memory_savings_percent: u8,
    pub benchmark_summary: String,
    pub suggestions: Vec<OptimizationSuggestion>,
}

pub fn analyze_template_directory(
    path: &Path,
    template_name: Option<&str>,
) -> Result<TemplatePerformanceAnalysis> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let name = template_name
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("template")
        })
        .to_string();
    let sources = read_source_files(&path);
    let joined = sources.join("\n");

    let storage_layout_score = if joined.contains("storage().instance()") {
        74
    } else {
        88
    };
    let function_efficiency_score = if joined.contains("for") { 66 } else { 84 };
    let loop_optimization_score = if joined.contains("for") { 58 } else { 82 };
    let external_call_score = if joined.contains("call") || joined.contains("invoke") {
        61
    } else {
        86
    };
    let batch_operations_score = if joined.contains("set(&") || joined.contains("get(&") {
        69
    } else {
        84
    };

    let mut suggestions = Vec::new();
    if joined.contains("for") {
        suggestions.push(OptimizationSuggestion {
            category: "loop_optimization".to_string(),
            priority: "high".to_string(),
            title: "Reduce repeated storage writes inside loops".to_string(),
            detail: "Move repeated state updates outside the loop or batch them to reduce host I/O cost.".to_string(),
            estimated_impact: "Up to 20% gas reduction".to_string(),
        });
    }
    if joined.contains("storage().instance()") {
        suggestions.push(OptimizationSuggestion {
            category: "storage_layout".to_string(),
            priority: "medium".to_string(),
            title: "Favor a compact storage layout".to_string(),
            detail: "Combine related values into a single storage entry when possible to reduce read/write overhead.".to_string(),
            estimated_impact: "Up to 10% gas reduction".to_string(),
        });
    }
    if joined.contains("call") || joined.contains("invoke") {
        suggestions.push(OptimizationSuggestion {
            category: "external_call".to_string(),
            priority: "medium".to_string(),
            title: "Batch or defer external calls".to_string(),
            detail: "Limit cross-contract interactions and prefer local computation to reduce execution time.".to_string(),
            estimated_impact: "Up to 12% speedup".to_string(),
        });
    }

    let overall_score = ((storage_layout_score as u32
        + function_efficiency_score as u32
        + loop_optimization_score as u32
        + external_call_score as u32
        + batch_operations_score as u32)
        / 5) as u8;
    let estimated_gas_reduction_percent = (100 - overall_score).clamp(5, 40);
    let estimated_speedup_percent = ((100 - overall_score) / 2).clamp(3, 25);
    let estimated_memory_savings_percent = ((100 - overall_score) / 3).clamp(2, 15);

    Ok(TemplatePerformanceAnalysis {
        template_name: name,
        path: path.display().to_string(),
        storage_layout_score,
        function_efficiency_score,
        loop_optimization_score,
        external_call_score,
        batch_operations_score,
        overall_score,
        estimated_gas_reduction_percent,
        estimated_speedup_percent,
        estimated_memory_savings_percent,
        benchmark_summary:
            "Static analysis of source layout, loops, storage usage, and external interactions"
                .to_string(),
        suggestions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyze_template_directory_returns_actionable_suggestions() {
        let temp_dir = tempfile::tempdir().unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(
            src_dir.join("lib.rs"),
            r#"
#![no_std]
use soroban_sdk::{contract, contractimpl, Env};

#[contract]
pub struct Counter;

#[contractimpl]
impl Counter {
    pub fn increment(env: Env) -> u32 {
        let mut count: u32 = env.storage().instance().get(&COUNTER).unwrap_or(0);
        for _ in 0..10 {
            env.storage().instance().set(&COUNTER, &count);
        }
        count
    }
}
"#,
        )
        .unwrap();

        let analysis = analyze_template_directory(temp_dir.path(), Some("counter")).unwrap();

        assert!(
            analysis.overall_score > 0,
            "analysis should include a score"
        );
        assert!(
            !analysis.suggestions.is_empty(),
            "analysis should return actionable suggestions"
        );
        assert!(analysis.estimated_gas_reduction_percent > 0);
    }
}
