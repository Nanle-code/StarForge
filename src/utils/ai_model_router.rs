//! Intelligent AI model selection and routing (issue #491).
//!
//! Classifies tasks by complexity and category, maps them to the optimal
//! provider/model based on cost, speed, and capability requirements, learns
//! user preferences over time, and tracks routing performance via telemetry.

use crate::utils::ai::{AIProvider, AIServiceConfig, ProviderConfig};
use crate::utils::ai_telemetry;
use crate::utils::config;
use crate::utils::ollama;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ─── Task classification ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskComplexity {
    Simple,
    Moderate,
    Complex,
    Expert,
}

impl std::fmt::Display for TaskComplexity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskComplexity::Simple => write!(f, "simple"),
            TaskComplexity::Moderate => write!(f, "moderate"),
            TaskComplexity::Complex => write!(f, "complex"),
            TaskComplexity::Expert => write!(f, "expert"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskCategory {
    General,
    CodeGeneration,
    CodeAnalysis,
    SecurityAudit,
    Planning,
    Accessibility,
    Documentation,
    Testing,
    Optimization,
}

impl std::fmt::Display for TaskCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskCategory::General => write!(f, "general"),
            TaskCategory::CodeGeneration => write!(f, "code_generation"),
            TaskCategory::CodeAnalysis => write!(f, "code_analysis"),
            TaskCategory::SecurityAudit => write!(f, "security_audit"),
            TaskCategory::Planning => write!(f, "planning"),
            TaskCategory::Accessibility => write!(f, "accessibility"),
            TaskCategory::Documentation => write!(f, "documentation"),
            TaskCategory::Testing => write!(f, "testing"),
            TaskCategory::Optimization => write!(f, "optimization"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskClassification {
    pub complexity: TaskComplexity,
    pub category: TaskCategory,
    pub estimated_tokens: u32,
    pub requires_reasoning: bool,
    pub requires_code: bool,
    pub confidence: f32,
    pub signals: Vec<String>,
}

// ─── Model capabilities ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapability {
    pub provider: AIProvider,
    pub model: String,
    pub max_complexity: TaskComplexity,
    pub supports_code: bool,
    pub cost_tier: u8,
    pub speed_tier: u8,
    pub quality_tier: u8,
    pub is_local: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingPreferences {
    pub cost_sensitive: bool,
    pub prefer_local: bool,
    pub prefer_speed: bool,
    pub preferred_provider: Option<AIProvider>,
    pub max_cost_tier: u8,
    pub updated_at: DateTime<Utc>,
}

impl Default for RoutingPreferences {
    fn default() -> Self {
        Self {
            cost_sensitive: false,
            prefer_local: false,
            prefer_speed: false,
            preferred_provider: None,
            max_cost_tier: 3,
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    pub provider: AIProvider,
    pub model: String,
    pub complexity: TaskComplexity,
    pub category: TaskCategory,
    pub reason: String,
    pub estimated_cost_usd: Option<f64>,
    pub fallback_chain: Vec<(AIProvider, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPerformanceRecord {
    pub provider: String,
    pub model: String,
    pub feature: String,
    pub success_rate: f64,
    pub avg_latency_ms: u64,
    pub avg_tokens: u64,
    pub total_calls: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PreferenceStore {
    preferences: RoutingPreferences,
    category_overrides: HashMap<String, String>,
    feature_model_history: HashMap<String, Vec<String>>,
}

fn default_model_catalog() -> Vec<ModelCapability> {
    vec![
        ModelCapability {
            provider: AIProvider::OpenAI,
            model: "gpt-3.5-turbo".into(),
            max_complexity: TaskComplexity::Moderate,
            supports_code: true,
            cost_tier: 1,
            speed_tier: 3,
            quality_tier: 2,
            is_local: false,
        },
        ModelCapability {
            provider: AIProvider::OpenAI,
            model: "gpt-4o-mini".into(),
            max_complexity: TaskComplexity::Complex,
            supports_code: true,
            cost_tier: 2,
            speed_tier: 3,
            quality_tier: 3,
            is_local: false,
        },
        ModelCapability {
            provider: AIProvider::OpenAI,
            model: "gpt-4o".into(),
            max_complexity: TaskComplexity::Expert,
            supports_code: true,
            cost_tier: 3,
            speed_tier: 2,
            quality_tier: 4,
            is_local: false,
        },
        ModelCapability {
            provider: AIProvider::Anthropic,
            model: "claude-3-5-haiku".into(),
            max_complexity: TaskComplexity::Moderate,
            supports_code: true,
            cost_tier: 1,
            speed_tier: 3,
            quality_tier: 2,
            is_local: false,
        },
        ModelCapability {
            provider: AIProvider::Anthropic,
            model: "claude-3-5-sonnet".into(),
            max_complexity: TaskComplexity::Complex,
            supports_code: true,
            cost_tier: 2,
            speed_tier: 2,
            quality_tier: 4,
            is_local: false,
        },
        ModelCapability {
            provider: AIProvider::Anthropic,
            model: "claude-opus-4-1".into(),
            max_complexity: TaskComplexity::Expert,
            supports_code: true,
            cost_tier: 3,
            speed_tier: 1,
            quality_tier: 5,
            is_local: false,
        },
        ModelCapability {
            provider: AIProvider::Ollama,
            model: ollama::DEFAULT_MODEL.into(),
            max_complexity: TaskComplexity::Complex,
            supports_code: true,
            cost_tier: 0,
            speed_tier: 2,
            quality_tier: 3,
            is_local: true,
        },
    ]
}

fn preferences_path() -> Result<PathBuf> {
    Ok(config::get_data_dir()?.join("ai_model_preferences.json"))
}

pub fn load_preferences() -> Result<RoutingPreferences> {
    let path = preferences_path()?;
    if !path.exists() {
        return Ok(RoutingPreferences::default());
    }
    let store: PreferenceStore =
        serde_json::from_str(&fs::read_to_string(&path)?).unwrap_or_default();
    Ok(store.preferences)
}

pub fn save_preferences(prefs: &RoutingPreferences) -> Result<()> {
    let path = preferences_path()?;
    let mut store = if path.exists() {
        serde_json::from_str(&fs::read_to_string(&path)?).unwrap_or_default()
    } else {
        PreferenceStore::default()
    };
    store.preferences = prefs.clone();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(&store)?)?;
    Ok(())
}

pub fn record_category_preference(category: &str, model: &str) -> Result<()> {
    let path = preferences_path()?;
    let mut store: PreferenceStore = if path.exists() {
        serde_json::from_str(&fs::read_to_string(&path)?).unwrap_or_default()
    } else {
        PreferenceStore::default()
    };
    store
        .category_overrides
        .insert(category.to_string(), model.to_string());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(&store)?)?;
    Ok(())
}

/// Classify a task from its prompt text and optional explicit category.
pub fn classify_task(prompt: &str, category_hint: Option<TaskCategory>) -> TaskClassification {
    let lower = prompt.to_lowercase();
    let word_count = prompt.split_whitespace().count();
    let line_count = prompt.lines().count();
    let has_code = prompt.contains("```") || lower.contains("fn ") || lower.contains("pub struct");
    let mut signals = Vec::new();

    let category = category_hint.unwrap_or_else(|| infer_category(&lower, has_code, &mut signals));

    let requires_code = matches!(
        category,
        TaskCategory::CodeGeneration
            | TaskCategory::CodeAnalysis
            | TaskCategory::SecurityAudit
            | TaskCategory::Testing
            | TaskCategory::Optimization
    ) || has_code;

    let requires_reasoning = lower.contains("architect")
        || lower.contains("design")
        || lower.contains("plan")
        || lower.contains("roadmap")
        || lower.contains("why")
        || lower.contains("compare")
        || lower.contains("trade-off")
        || lower.contains("risk")
        || matches!(
            category,
            TaskCategory::Planning | TaskCategory::SecurityAudit
        );

    if requires_reasoning {
        signals.push("reasoning_keywords".into());
    }
    if has_code {
        signals.push("contains_code".into());
    }

    let complexity =
        if word_count > 800 || line_count > 60 || requires_reasoning && word_count > 300 {
            signals.push("high_token_count".into());
            TaskComplexity::Expert
        } else if word_count > 300 || line_count > 25 || requires_reasoning {
            TaskComplexity::Complex
        } else if word_count > 80 || requires_code {
            TaskComplexity::Moderate
        } else {
            TaskComplexity::Simple
        };

    let estimated_tokens = (word_count as u32 * 2).max(256).min(8192);
    let confidence = if category_hint.is_some() {
        0.95
    } else if signals.len() >= 2 {
        0.85
    } else {
        0.7
    };

    TaskClassification {
        complexity,
        category,
        estimated_tokens,
        requires_reasoning,
        requires_code,
        confidence,
        signals,
    }
}

fn infer_category(lower: &str, has_code: bool, signals: &mut Vec<String>) -> TaskCategory {
    if lower.contains("accessibility")
        || lower.contains("screen reader")
        || lower.contains("wcag")
        || lower.contains("voice command")
    {
        signals.push("accessibility_keywords".into());
        TaskCategory::Accessibility
    } else if lower.contains("plan")
        || lower.contains("roadmap")
        || lower.contains("timeline")
        || lower.contains("milestone")
        || lower.contains("sprint")
    {
        signals.push("planning_keywords".into());
        TaskCategory::Planning
    } else if lower.contains("audit")
        || lower.contains("vulnerabilit")
        || lower.contains("exploit")
        || lower.contains("security")
    {
        signals.push("security_keywords".into());
        TaskCategory::SecurityAudit
    } else if lower.contains("test") || lower.contains("coverage") {
        TaskCategory::Testing
    } else if lower.contains("optimiz") || lower.contains("gas") || lower.contains("performance") {
        TaskCategory::Optimization
    } else if lower.contains("document") || lower.contains("readme") || lower.contains("explain") {
        TaskCategory::Documentation
    } else if lower.contains("generate") || lower.contains("implement") || lower.contains("write") {
        if has_code
            || lower.contains("contract")
            || lower.contains("code")
            || lower.contains("function")
            || lower.contains("program")
        {
            signals.push("code_generation".into());
            TaskCategory::CodeGeneration
        } else {
            TaskCategory::General
        }
    } else if has_code || lower.contains("analyze") || lower.contains("review") {
        TaskCategory::CodeAnalysis
    } else {
        TaskCategory::General
    }
}

fn complexity_rank(c: TaskComplexity) -> u8 {
    match c {
        TaskComplexity::Simple => 1,
        TaskComplexity::Moderate => 2,
        TaskComplexity::Complex => 3,
        TaskComplexity::Expert => 4,
    }
}

fn complexity_sufficient(model_max: TaskComplexity, required: TaskComplexity) -> bool {
    complexity_rank(model_max) >= complexity_rank(required)
}

/// Select the optimal model for a classified task.
pub async fn route_task(
    prompt: &str,
    category_hint: Option<TaskCategory>,
    prefs: Option<RoutingPreferences>,
) -> Result<RoutingDecision> {
    let classification = classify_task(prompt, category_hint);
    let prefs = prefs.unwrap_or_else(|| load_preferences().unwrap_or_default());
    let catalog = default_model_catalog();
    let ollama_available = ollama::is_ollama_running().await;

    let path = preferences_path().ok();
    let category_override = path
        .as_ref()
        .and_then(|p| {
            if p.exists() {
                serde_json::from_str::<PreferenceStore>(&fs::read_to_string(p).ok()?).ok()
            } else {
                None
            }
        })
        .and_then(|s| {
            s.category_overrides
                .get(&classification.category.to_string())
                .cloned()
        });

    let mut candidates: Vec<&ModelCapability> = catalog
        .iter()
        .filter(|m| complexity_sufficient(m.max_complexity, classification.complexity))
        .filter(|m| !classification.requires_code || m.supports_code)
        .filter(|m| !m.is_local || ollama_available)
        .filter(|m| m.cost_tier <= prefs.max_cost_tier)
        .collect();

    if let Some(ref provider) = prefs.preferred_provider {
        candidates.retain(|m| &m.provider == provider || m.is_local);
        if candidates.is_empty() {
            candidates = catalog
                .iter()
                .filter(|m| complexity_sufficient(m.max_complexity, classification.complexity))
                .filter(|m| !m.is_local || ollama_available)
                .collect();
        }
    }

    if prefs.prefer_local && ollama_available {
        if let Some(local) = candidates.iter().find(|m| m.is_local) {
            return Ok(build_decision(
                local,
                &classification,
                "Local Ollama preferred by user",
            ));
        }
    }

    if let Some(override_model) = category_override {
        if let Some(m) = catalog.iter().find(|m| m.model == override_model) {
            return Ok(build_decision(
                m,
                &classification,
                "Learned category preference",
            ));
        }
    }

    candidates.sort_by(|a, b| {
        let score_a = model_score(a, &classification, &prefs);
        let score_b = model_score(b, &classification, &prefs);
        score_b.cmp(&score_a)
    });

    let best = candidates
        .first()
        .context("No suitable model found for task")?;

    let reason = match (classification.complexity, classification.category) {
        (TaskComplexity::Simple, _) if prefs.cost_sensitive => "Simple task — optimizing for cost",
        (_, TaskCategory::CodeGeneration) => "Code generation — code-specialized model",
        (_, TaskCategory::SecurityAudit) => "Security audit — high-capability model",
        (TaskComplexity::Expert, _) => "Expert complexity — capable model selected",
        (TaskComplexity::Simple, _) => "Simple task — fast/cheap model",
        _ => "Balanced cost-performance routing",
    };

    Ok(build_decision(best, &classification, reason))
}

fn model_score(m: &ModelCapability, task: &TaskClassification, prefs: &RoutingPreferences) -> i32 {
    let mut score = (m.quality_tier as i32) * 10;

    if prefs.cost_sensitive {
        score += (4 - m.cost_tier as i32) * 15;
    } else {
        score += (m.cost_tier as i32) * 3;
    }

    if prefs.prefer_speed {
        score += (m.speed_tier as i32) * 12;
    }

    if m.is_local && prefs.prefer_local {
        score += 30;
    }

    if task.requires_code && m.supports_code {
        score += 10;
    }

    if task.requires_reasoning && m.quality_tier >= 3 {
        score += 15;
    }

    if m.model.contains("codellama") && task.requires_code {
        score += 20;
    }

    score
}

fn build_decision(
    model: &ModelCapability,
    classification: &TaskClassification,
    reason: &str,
) -> RoutingDecision {
    let catalog = default_model_catalog();
    let fallback_chain: Vec<(AIProvider, String)> = catalog
        .iter()
        .filter(|m| m.provider != model.provider || m.model != model.model)
        .filter(|m| complexity_sufficient(m.max_complexity, classification.complexity))
        .take(3)
        .map(|m| (m.provider.clone(), m.model.clone()))
        .collect();

    let estimated_cost = if model.is_local {
        None
    } else {
        ai_telemetry::estimate_cost(
            &provider_name(&model.provider),
            &model.model,
            classification.estimated_tokens as u64,
            (classification.estimated_tokens / 2) as u64,
        )
    };

    RoutingDecision {
        provider: model.provider.clone(),
        model: model.model.clone(),
        complexity: classification.complexity,
        category: classification.category,
        reason: reason.to_string(),
        estimated_cost_usd: estimated_cost,
        fallback_chain,
    }
}

fn provider_name(p: &AIProvider) -> &'static str {
    match p {
        AIProvider::OpenAI => "openai",
        AIProvider::Anthropic => "anthropic",
        AIProvider::Ollama => "ollama",
    }
}

/// Build an AIServiceConfig from a routing decision for immediate use.
pub fn config_from_decision(decision: &RoutingDecision) -> AIServiceConfig {
    let provider_config = ProviderConfig {
        api_key: std::env::var(match decision.provider {
            AIProvider::OpenAI => "OPENAI_API_KEY",
            AIProvider::Anthropic => "ANTHROPIC_API_KEY",
            AIProvider::Ollama => "OLLAMA_API_KEY",
        })
        .ok(),
        base_url: match decision.provider {
            AIProvider::OpenAI => "https://api.openai.com".into(),
            AIProvider::Anthropic => "https://api.anthropic.com".into(),
            AIProvider::Ollama => ollama::OLLAMA_BASE_URL.into(),
        },
        model: decision.model.clone(),
        max_tokens: 4096,
        timeout_secs: 60,
    };

    let mut providers = HashMap::new();
    providers.insert(decision.provider.clone(), provider_config);

    let fallback_order = std::iter::once(decision.provider.clone())
        .chain(decision.fallback_chain.iter().map(|(p, _)| p.clone()))
        .collect();

    AIServiceConfig {
        default_provider: decision.provider.clone(),
        providers,
        fallback_order,
        circuit_breaker_threshold: 3,
        circuit_breaker_timeout_secs: 60,
    }
}

/// Aggregate model performance from local AI telemetry records.
pub fn model_performance_stats(days: Option<u32>) -> Result<Vec<ModelPerformanceRecord>> {
    let records = ai_telemetry::load_records(days)?;
    let mut by_model: HashMap<(String, String, String), (u64, u64, u64, u64)> = HashMap::new();

    for r in &records {
        let key = (r.provider.clone(), r.model.clone(), r.feature.clone());
        let entry = by_model.entry(key).or_insert((0, 0, 0, 0));
        entry.0 += 1;
        if r.success {
            entry.1 += 1;
        }
        entry.2 += r.latency_ms;
        let tokens = r.tokens_in.unwrap_or(0) + r.tokens_out.unwrap_or(0);
        entry.3 += tokens;
    }

    let mut stats: Vec<ModelPerformanceRecord> = by_model
        .into_iter()
        .map(
            |((provider, model, feature), (total, success, latency, tokens))| {
                ModelPerformanceRecord {
                    provider,
                    model,
                    feature,
                    success_rate: if total > 0 {
                        success as f64 / total as f64
                    } else {
                        0.0
                    },
                    avg_latency_ms: if total > 0 { latency / total } else { 0 },
                    avg_tokens: if total > 0 { tokens / total } else { 0 },
                    total_calls: total,
                }
            },
        )
        .collect();

    stats.sort_by(|a, b| b.total_calls.cmp(&a.total_calls));
    Ok(stats)
}

pub fn parse_category(s: &str) -> Result<TaskCategory> {
    match s.to_lowercase().as_str() {
        "general" => Ok(TaskCategory::General),
        "code_generation" | "code-generation" | "codegen" => Ok(TaskCategory::CodeGeneration),
        "code_analysis" | "code-analysis" | "analysis" => Ok(TaskCategory::CodeAnalysis),
        "security_audit" | "security-audit" | "security" | "audit" => {
            Ok(TaskCategory::SecurityAudit)
        }
        "planning" | "plan" => Ok(TaskCategory::Planning),
        "accessibility" | "a11y" => Ok(TaskCategory::Accessibility),
        "documentation" | "docs" => Ok(TaskCategory::Documentation),
        "testing" | "test" => Ok(TaskCategory::Testing),
        "optimization" | "optimisation" | "optimize" => Ok(TaskCategory::Optimization),
        other => anyhow::bail!(
            "Unknown category '{}'. Use: general, code_generation, code_analysis, security_audit, planning, accessibility, documentation, testing, optimization",
            other
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_simple_question() {
        let c = classify_task("What is Soroban?", None);
        assert_eq!(c.complexity, TaskComplexity::Simple);
    }

    #[test]
    fn classify_code_generation() {
        let c = classify_task("Generate a token contract with mint and burn", None);
        assert_eq!(c.category, TaskCategory::CodeGeneration);
    }

    #[test]
    fn classify_planning_task() {
        let c = classify_task("Create a development roadmap with milestones", None);
        assert_eq!(c.category, TaskCategory::Planning);
    }

    #[test]
    fn classify_security_audit() {
        let c = classify_task("Audit this contract for vulnerabilities", None);
        assert_eq!(c.category, TaskCategory::SecurityAudit);
    }

    #[test]
    fn complexity_sufficient_check() {
        assert!(complexity_sufficient(
            TaskComplexity::Complex,
            TaskComplexity::Moderate
        ));
        assert!(!complexity_sufficient(
            TaskComplexity::Simple,
            TaskComplexity::Expert
        ));
    }
}
