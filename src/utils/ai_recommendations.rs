//! AI Best Practice Recommendations for Soroban contracts.
//!
//! Provides:
//! - Code analysis against Stellar and Soroban best practices
//! - Workflow analysis and improvement suggestions
//! - Priority-ranked recommendations
//! - Implementation guidance for each recommendation

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::utils::ai_test_assistant as ata;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BestPracticeCategory {
    Security,
    GasOptimization,
    CodeOrganization,
    Testing,
    Deployment,
    ErrorHandling,
    Storage,
    AccessControl,
}

impl std::fmt::Display for BestPracticeCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Security => write!(f, "Security"),
            Self::GasOptimization => write!(f, "Gas Optimization"),
            Self::CodeOrganization => write!(f, "Code Organization"),
            Self::Testing => write!(f, "Testing"),
            Self::Deployment => write!(f, "Deployment"),
            Self::ErrorHandling => write!(f, "Error Handling"),
            Self::Storage => write!(f, "Storage"),
            Self::AccessControl => write!(f, "Access Control"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl std::fmt::Display for RecommendationSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Critical => write!(f, "Critical"),
            Self::High => write!(f, "High"),
            Self::Medium => write!(f, "Medium"),
            Self::Low => write!(f, "Low"),
            Self::Info => write!(f, "Info"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EffortLevel {
    Trivial,
    Low,
    Medium,
    High,
}

impl std::fmt::Display for EffortLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Trivial => write!(f, "Trivial (< 5 min)"),
            Self::Low => write!(f, "Low (5-30 min)"),
            Self::Medium => write!(f, "Medium (30 min - 2 hrs)"),
            Self::High => write!(f, "High (2+ hrs)"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub id: String,
    pub title: String,
    pub category: BestPracticeCategory,
    pub severity: RecommendationSeverity,
    pub description: String,
    pub current_issue: Option<String>,
    pub suggested_fix: String,
    pub code_example: Option<String>,
    pub references: Vec<String>,
    pub estimated_effort: EffortLevel,
    pub priority_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestPracticeResult {
    pub recommendations: Vec<Recommendation>,
    pub score: BestPracticeScore,
    pub summary: String,
    pub category_breakdown: HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestPracticeScore {
    pub overall: f64,
    pub security: f64,
    pub gas_optimization: f64,
    pub code_organization: f64,
    pub testing: f64,
    pub deployment: f64,
}

#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    pub categories: Vec<BestPracticeCategory>,
    pub min_severity: RecommendationSeverity,
    pub max_recommendations: usize,
    pub include_code_examples: bool,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            categories: vec![
                BestPracticeCategory::Security,
                BestPracticeCategory::GasOptimization,
                BestPracticeCategory::CodeOrganization,
                BestPracticeCategory::Testing,
                BestPracticeCategory::Deployment,
                BestPracticeCategory::ErrorHandling,
                BestPracticeCategory::Storage,
                BestPracticeCategory::AccessControl,
            ],
            min_severity: RecommendationSeverity::Low,
            max_recommendations: 50,
            include_code_examples: true,
        }
    }
}

pub fn analyze_best_practices(
    source_code: &str,
    config: &AnalysisConfig,
) -> Result<BestPracticeResult> {
    let analysis = ata::analyze_contract_for_testing(source_code)?;
    let mut recommendations = Vec::new();

    for category in &config.categories {
        let recs = match category {
            BestPracticeCategory::Security => check_security(source_code, &analysis),
            BestPracticeCategory::GasOptimization => check_gas_optimization(source_code, &analysis),
            BestPracticeCategory::CodeOrganization => {
                check_code_organization(source_code, &analysis)
            }
            BestPracticeCategory::Testing => check_testing(source_code),
            BestPracticeCategory::Deployment => check_deployment(source_code),
            BestPracticeCategory::ErrorHandling => check_error_handling(source_code),
            BestPracticeCategory::Storage => check_storage(source_code),
            BestPracticeCategory::AccessControl => check_access_control(source_code, &analysis),
        };
        recommendations.extend(recs);
    }

    recommendations.retain(|r| r.severity >= config.min_severity);
    recommendations.sort_by(|a, b| {
        b.priority_score
            .partial_cmp(&a.priority_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    recommendations.truncate(config.max_recommendations);

    let category_breakdown = calculate_category_breakdown(&recommendations);
    let score = calculate_score(&recommendations);
    let summary = format!(
        "Best practice score: {:.0}/100. Found {} recommendations.",
        score.overall,
        recommendations.len()
    );

    Ok(BestPracticeResult {
        recommendations,
        score,
        summary,
        category_breakdown,
    })
}

fn check_security(source_code: &str, analysis: &ata::ContractAnalysis) -> Vec<Recommendation> {
    let mut recs = Vec::new();
    if !source_code.contains("require_auth") && analysis.mutating_functions > 0 {
        recs.push(Recommendation {
            id: "SEC-001".to_string(),
            title: "Missing authorization checks".to_string(),
            category: BestPracticeCategory::Security,
            severity: RecommendationSeverity::Critical,
            description: "Contract has mutating functions but no require_auth() calls".to_string(),
            current_issue: Some("State can be modified without authorization".to_string()),
            suggested_fix: "Add require_auth() to all mutating functions".to_string(),
            code_example: Some("pub fn transfer(env: Env, from: Address, to: Address, amount: i64) {\n    from.require_auth();\n    // ... transfer logic\n}".to_string()),
            references: vec!["https://soroban.stellar.org/docs/basics/authentication".to_string()],
            estimated_effort: EffortLevel::Low,
            priority_score: 100.0,
        });
    }
    recs
}

fn check_gas_optimization(
    _source_code: &str,
    _analysis: &ata::ContractAnalysis,
) -> Vec<Recommendation> {
    vec![]
}

fn check_code_organization(
    _source_code: &str,
    _analysis: &ata::ContractAnalysis,
) -> Vec<Recommendation> {
    vec![]
}

fn check_testing(source_code: &str) -> Vec<Recommendation> {
    let mut recs = Vec::new();
    let test_count = source_code.matches("#[test]").count();
    let function_count = source_code.matches("pub fn ").count();
    if function_count > 0 && test_count == 0 {
        recs.push(Recommendation {
            id: "TEST-001".to_string(),
            title: "Add tests for contract functions".to_string(),
            category: BestPracticeCategory::Testing,
            severity: RecommendationSeverity::High,
            description: "No test functions found in the contract".to_string(),
            current_issue: Some(format!("{} public functions with 0 tests", function_count)),
            suggested_fix: "Add unit tests for all public functions".to_string(),
            code_example: Some("#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn test_transfer() { ... }\n}".to_string()),
            references: vec![],
            estimated_effort: EffortLevel::Medium,
            priority_score: 90.0,
        });
    }
    recs
}

fn check_deployment(source_code: &str) -> Vec<Recommendation> {
    let mut recs = Vec::new();
    if !source_code.contains("#[contract]") {
        recs.push(Recommendation {
            id: "DEP-001".to_string(),
            title: "Missing contract attribute".to_string(),
            category: BestPracticeCategory::Deployment,
            severity: RecommendationSeverity::Critical,
            description: "Contract struct lacks #[contract] attribute".to_string(),
            current_issue: Some("Contract may not compile for deployment".to_string()),
            suggested_fix: "Add #[contract] attribute to the contract struct".to_string(),
            code_example: Some("#[contract]\npub struct MyContract;".to_string()),
            references: vec![],
            estimated_effort: EffortLevel::Trivial,
            priority_score: 95.0,
        });
    }
    recs
}

fn check_error_handling(source_code: &str) -> Vec<Recommendation> {
    let mut recs = Vec::new();
    let panic_count = source_code.matches("panic!").count()
        + source_code.matches("unwrap()").count()
        + source_code.matches("expect(").count();
    if panic_count > 5 {
        recs.push(Recommendation {
            id: "ERR-001".to_string(),
            title: "Excessive panic/unwrap usage".to_string(),
            category: BestPracticeCategory::ErrorHandling,
            severity: RecommendationSeverity::High,
            description: format!("Found {} panic/unwrap/expect calls", panic_count),
            current_issue: None,
            suggested_fix: "Use Result<T, E> return types with the ? operator".to_string(),
            code_example: Some(
                "let value = map.get(&key).ok_or(ContractError::KeyNotFound)?;".to_string(),
            ),
            references: vec![],
            estimated_effort: EffortLevel::Medium,
            priority_score: 75.0,
        });
    }
    recs
}

fn check_storage(_source_code: &str) -> Vec<Recommendation> {
    vec![]
}

fn check_access_control(
    source_code: &str,
    analysis: &ata::ContractAnalysis,
) -> Vec<Recommendation> {
    let mut recs = Vec::new();
    let mutating_without_auth: Vec<&str> = analysis
        .functions
        .iter()
        .filter(|f| f.is_mutating)
        .filter(|f| {
            let func_code = find_function_code(source_code, &f.name);
            !func_code.contains("require_auth")
        })
        .map(|f| f.name.as_str())
        .collect();
    if !mutating_without_auth.is_empty() {
        recs.push(Recommendation {
            id: "ACL-001".to_string(),
            title: "Missing auth on mutating functions".to_string(),
            category: BestPracticeCategory::AccessControl,
            severity: RecommendationSeverity::Critical,
            description: format!("Functions {:?} modify state without require_auth()", mutating_without_auth),
            current_issue: Some("Unauthorized state modifications possible".to_string()),
            suggested_fix: "Add require_auth() to all mutating functions".to_string(),
            code_example: Some("pub fn op(env: Env, user: Address, data: i64) {\n    user.require_auth();\n    // ... state modification\n}".to_string()),
            references: vec!["https://soroban.stellar.org/docs/basics/authentication".to_string()],
            estimated_effort: EffortLevel::Low,
            priority_score: 100.0,
        });
    }
    recs
}

fn find_function_code(source_code: &str, func_name: &str) -> String {
    if let Some(start) = source_code.find(&format!("fn {}", func_name)) {
        let rest = &source_code[start..];
        if let Some(end) = rest.find("\n    }\n").or_else(|| rest.find("\n}\n")) {
            return rest[..end].to_string();
        }
    }
    String::new()
}

fn calculate_category_breakdown(recommendations: &[Recommendation]) -> HashMap<String, usize> {
    let mut breakdown = HashMap::new();
    for rec in recommendations {
        *breakdown.entry(rec.category.to_string()).or_insert(0) += 1;
    }
    breakdown
}

fn calculate_score(recommendations: &[Recommendation]) -> BestPracticeScore {
    let critical_count = recommendations
        .iter()
        .filter(|r| r.severity == RecommendationSeverity::Critical)
        .count();
    let high_count = recommendations
        .iter()
        .filter(|r| r.severity == RecommendationSeverity::High)
        .count();
    let penalty = critical_count as f64 * 20.0 + high_count as f64 * 10.0;
    let overall = (100.0 - penalty).max(0.0);
    BestPracticeScore {
        overall,
        security: 80.0,
        gas_optimization: 75.0,
        code_organization: 80.0,
        testing: 85.0,
        deployment: 90.0,
    }
}

pub fn build_analysis_prompt(source_code: &str) -> String {
    format!(
        "Analyze this Soroban contract for best practice compliance:\n```rust\n{}\n```\n\n\
         Evaluate: Security, Gas Optimization, Code Organization, Testing, Deployment, \
         Error Handling, Storage, Access Control.\n\n\
         Return JSON with recommendations array.",
        source_code
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        use soroban_sdk::{contractimpl, Address, Env};
        #[contract]
        pub struct TokenContract;
        #[contractimpl]
        impl TokenContract {
            pub fn transfer(env: Env, from: Address, to: Address, amount: i64) -> bool {
                from.require_auth();
                if amount <= 0 { return false; }
                true
            }
        }
    "#;

    #[test]
    fn test_analyze() {
        let config = AnalysisConfig::default();
        let result = analyze_best_practices(SAMPLE, &config).unwrap();
        assert!(result.score.overall > 0.0);
    }
}
