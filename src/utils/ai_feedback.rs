//! AI Feedback and Learning System.
//!
//! Provides:
//! - User feedback collection on AI-generated responses
//! - Response quality tracking and scoring
//! - Preference learning from user corrections
//! - Continuous improvement through feedback loops
//! - Privacy-preserving feedback storage

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::utils::config;

// ── Types ────────────────────────────────────────────────────────────────────

/// A single feedback entry from a user on an AI response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackEntry {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub feature: String,
    pub prompt_summary: String,
    pub response_summary: String,
    pub rating: FeedbackRating,
    pub comment: Option<String>,
    pub corrections: Vec<Correction>,
    pub context: FeedbackContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FeedbackRating {
    Positive,
    Negative,
    Neutral,
    Partial,
}

impl std::fmt::Display for FeedbackRating {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeedbackRating::Positive => write!(f, "Positive"),
            FeedbackRating::Negative => write!(f, "Negative"),
            FeedbackRating::Neutral => write!(f, "Neutral"),
            FeedbackRating::Partial => write!(f, "Partial"),
        }
    }
}

/// A specific correction the user provided for an AI response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Correction {
    pub original_output: String,
    pub corrected_output: String,
    pub reason: String,
    pub category: CorrectionCategory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionCategory {
    Syntax,
    Logic,
    Style,
    Security,
    Performance,
    Documentation,
    TestCoverage,
}

impl std::fmt::Display for CorrectionCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CorrectionCategory::Syntax => write!(f, "Syntax"),
            CorrectionCategory::Logic => write!(f, "Logic"),
            CorrectionCategory::Style => write!(f, "Style"),
            CorrectionCategory::Security => write!(f, "Security"),
            CorrectionCategory::Performance => write!(f, "Performance"),
            CorrectionCategory::Documentation => write!(f, "Documentation"),
            CorrectionCategory::TestCoverage => write!(f, "Test Coverage"),
        }
    }
}

/// Context about the AI interaction that generated the response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackContext {
    pub model: String,
    pub contract_type: Option<String>,
    pub project_size: Option<String>,
    pub language: String,
    pub feature_version: String,
}

/// User preference profile learned from feedback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreference {
    pub preference_type: PreferenceType,
    pub value: String,
    pub confidence: f64,
    pub learned_from: usize,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceType {
    CodeStyle,
    ErrorHandling,
    TestingApproach,
    DocumentationLevel,
    SecurityFocus,
    PerformancePriority,
    OutputFormat,
}

impl std::fmt::Display for PreferenceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreferenceType::CodeStyle => write!(f, "Code Style"),
            PreferenceType::ErrorHandling => write!(f, "Error Handling"),
            PreferenceType::TestingApproach => write!(f, "Testing Approach"),
            PreferenceType::DocumentationLevel => write!(f, "Documentation Level"),
            PreferenceType::SecurityFocus => write!(f, "Security Focus"),
            PreferenceType::PerformancePriority => write!(f, "Performance Priority"),
            PreferenceType::OutputFormat => write!(f, "Output Format"),
        }
    }
}

/// Quality metrics for an AI response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityMetrics {
    pub accuracy_score: f64,
    pub relevance_score: f64,
    pub completeness_score: f64,
    pub clarity_score: f64,
    pub overall_score: f64,
    pub response_time_ms: u64,
    pub token_count: u32,
}

/// Aggregate statistics for a feature's feedback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureStats {
    pub feature: String,
    pub total_feedback: usize,
    pub positive_rate: f64,
    pub negative_rate: f64,
    pub avg_quality_score: f64,
    pub top_corrections: Vec<(CorrectionCategory, usize)>,
    pub improvement_trend: f64,
}

/// The complete feedback store for a project.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeedbackStore {
    pub entries: Vec<FeedbackEntry>,
    pub preferences: Vec<UserPreference>,
    pub version: u32,
}

// ── Storage ──────────────────────────────────────────────────────────────────

thread_local! {
    static TEST_DIR_OVERRIDE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub fn set_test_dir(path: PathBuf) {
    TEST_DIR_OVERRIDE.with(|cell| {
        *cell.borrow_mut() = Some(path);
    });
}

fn feedback_dir() -> PathBuf {
    #[cfg(test)]
    {
        if let Some(path) = TEST_DIR_OVERRIDE.with(|cell| cell.borrow().clone()) {
            return path;
        }
    }
    config::config_dir().join("feedback")
}

fn feedback_file() -> PathBuf {
    feedback_dir().join("feedback.json")
}

pub fn load_store() -> Result<FeedbackStore> {
    let path = feedback_file();
    if !path.exists() {
        return Ok(FeedbackStore::default());
    }
    let data = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read feedback store: {}", path.display()))?;
    let store: FeedbackStore =
        serde_json::from_str(&data).context("Failed to parse feedback store")?;
    Ok(store)
}

pub fn save_store(store: &FeedbackStore) -> Result<()> {
    let dir = feedback_dir();
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create feedback dir: {}", dir.display()))?;
    let data = serde_json::to_string_pretty(store)?;
    fs::write(feedback_file(), data)?;
    Ok(())
}

// ── Feedback Collection ──────────────────────────────────────────────────────

/// Record a new feedback entry.
pub fn record_feedback(
    feature: &str,
    prompt_summary: &str,
    response_summary: &str,
    rating: FeedbackRating,
    comment: Option<String>,
    corrections: Vec<Correction>,
) -> Result<FeedbackEntry> {
    let mut store = load_store()?;

    let entry = FeedbackEntry {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        feature: feature.to_string(),
        prompt_summary: prompt_summary.to_string(),
        response_summary: response_summary.to_string(),
        rating,
        comment,
        corrections,
        context: FeedbackContext {
            model: "default".to_string(),
            contract_type: None,
            project_size: None,
            language: "rust".to_string(),
            feature_version: "1.0".to_string(),
        },
    };

    store.entries.push(entry.clone());
    store.version += 1;
    save_store(&store)?;

    Ok(entry)
}

// ── Preference Learning ──────────────────────────────────────────────────────

/// Analyze corrections and update user preferences.
pub fn learn_preferences(store: &mut FeedbackStore) {
    let mut pref_counts: HashMap<PreferenceType, HashMap<String, usize>> = HashMap::new();

    for entry in &store.entries {
        for correction in &entry.corrections {
            let pref_type = match correction.category {
                CorrectionCategory::Style => PreferenceType::CodeStyle,
                CorrectionCategory::Logic => PreferenceType::TestingApproach,
                CorrectionCategory::Documentation => PreferenceType::DocumentationLevel,
                CorrectionCategory::Security => PreferenceType::SecurityFocus,
                CorrectionCategory::Performance => PreferenceType::PerformancePriority,
                _ => continue,
            };
            let map = pref_counts.entry(pref_type).or_default();
            *map.entry(correction.corrected_output.clone()).or_insert(0) += 1;
        }
    }

    for (pref_type, values) in &pref_counts {
        if let Some((best_value, count)) = values.iter().max_by_key(|(_, c)| *c) {
            let total: usize = values.values().sum();
            let confidence = *count as f64 / total as f64;

            if let Some(existing) = store
                .preferences
                .iter_mut()
                .find(|p| p.preference_type == *pref_type)
            {
                existing.confidence = confidence;
                existing.learned_from += 1;
                existing.last_updated = Utc::now();
                if confidence > existing.confidence {
                    existing.value = best_value.clone();
                }
            } else {
                store.preferences.push(UserPreference {
                    preference_type: pref_type.clone(),
                    value: best_value.clone(),
                    confidence,
                    learned_from: *count,
                    last_updated: Utc::now(),
                });
            }
        }
    }
}

/// Get the current preference for a given type.
pub fn get_preference<'a>(
    store: &'a FeedbackStore,
    pref_type: &PreferenceType,
) -> Option<&'a UserPreference> {
    store
        .preferences
        .iter()
        .find(|p| p.preference_type == *pref_type)
}

// ── Quality Tracking ─────────────────────────────────────────────────────────

/// Calculate quality metrics from feedback for a feature.
pub fn calculate_quality_metrics(feature: &str) -> Result<QualityMetrics> {
    let store = load_store()?;
    let feature_entries: Vec<&FeedbackEntry> = store
        .entries
        .iter()
        .filter(|e| e.feature == feature)
        .collect();

    if feature_entries.is_empty() {
        return Ok(QualityMetrics {
            accuracy_score: 0.5,
            relevance_score: 0.5,
            completeness_score: 0.5,
            clarity_score: 0.5,
            overall_score: 0.5,
            response_time_ms: 0,
            token_count: 0,
        });
    }

    let positive_count = feature_entries
        .iter()
        .filter(|e| matches!(e.rating, FeedbackRating::Positive))
        .count();
    let partial_count = feature_entries
        .iter()
        .filter(|e| matches!(e.rating, FeedbackRating::Partial))
        .count();
    let total = feature_entries.len() as f64;

    let accuracy_score = (positive_count as f64 + partial_count as f64 * 0.5) / total;
    let relevance_score = accuracy_score * 0.9;
    let completeness_score = (total
        - feature_entries
            .iter()
            .filter(|e| e.corrections.len() > 2)
            .count() as f64)
        / total;
    let clarity_score = (total
        - feature_entries
            .iter()
            .filter(|e| {
                e.corrections
                    .iter()
                    .any(|c| c.category == CorrectionCategory::Syntax)
            })
            .count() as f64)
        / total;

    let overall_score = accuracy_score * 0.3
        + relevance_score * 0.3
        + completeness_score * 0.2
        + clarity_score * 0.2;

    Ok(QualityMetrics {
        accuracy_score,
        relevance_score,
        completeness_score,
        clarity_score,
        overall_score,
        response_time_ms: 0,
        token_count: 0,
    })
}

// ── Feature Statistics ───────────────────────────────────────────────────────

/// Get aggregate statistics for a feature.
pub fn get_feature_stats(feature: &str) -> Result<FeatureStats> {
    let store = load_store()?;
    let entries: Vec<&FeedbackEntry> = store
        .entries
        .iter()
        .filter(|e| e.feature == feature)
        .collect();

    let total = entries.len();
    if total == 0 {
        return Ok(FeatureStats {
            feature: feature.to_string(),
            total_feedback: 0,
            positive_rate: 0.0,
            negative_rate: 0.0,
            avg_quality_score: 0.0,
            top_corrections: vec![],
            improvement_trend: 0.0,
        });
    }

    let positive = entries
        .iter()
        .filter(|e| matches!(e.rating, FeedbackRating::Positive))
        .count();
    let negative = entries
        .iter()
        .filter(|e| matches!(e.rating, FeedbackRating::Negative))
        .count();

    let mut correction_counts: HashMap<CorrectionCategory, usize> = HashMap::new();
    for entry in &entries {
        for correction in &entry.corrections {
            *correction_counts
                .entry(correction.category.clone())
                .or_insert(0) += 1;
        }
    }

    let mut top_corrections: Vec<(CorrectionCategory, usize)> =
        correction_counts.into_iter().collect();
    top_corrections.sort_by_key(|a| std::cmp::Reverse(a.1));
    top_corrections.truncate(5);

    let metrics = calculate_quality_metrics(feature)?;

    Ok(FeatureStats {
        feature: feature.to_string(),
        total_feedback: total,
        positive_rate: positive as f64 / total as f64,
        negative_rate: negative as f64 / total as f64,
        avg_quality_score: metrics.overall_score,
        top_corrections,
        improvement_trend: 0.0,
    })
}

// ── Prompt Building ──────────────────────────────────────────────────────────

/// Build a prompt that incorporates learned preferences into AI responses.
pub fn build_preference_aware_prompt(base_prompt: &str, feature: &str) -> Result<String> {
    let store = load_store()?;
    let metrics = calculate_quality_metrics(feature)?;

    let mut context_parts = vec![format!(
        "Base prompt:\n{}\n\nQuality context for feature '{}':",
        base_prompt, feature,
    )];

    if metrics.overall_score < 0.5 {
        context_parts
            .push("Note: Previous responses for this feature have low quality. Focus on accuracy and completeness.".to_string());
    }

    for pref in &store.preferences {
        context_parts.push(format!(
            "User preference: {} = {} (confidence: {:.0}%)",
            pref.preference_type,
            pref.value,
            pref.confidence * 100.0,
        ));
    }

    Ok(context_parts.join("\n"))
}

/// Build a summary prompt to track improvement over time.
pub fn build_improvement_summary_prompt(feature: &str) -> Result<String> {
    let stats = get_feature_stats(feature)?;

    Ok(format!(
        r#"Generate an improvement summary for AI feature "{feature}":

Total feedback entries: {total}
Positive rate: {positive:.0}%
Negative rate: {negative:.0}%
Average quality score: {quality:.1}/1.0

Top correction categories:
{corrections}

Based on this feedback, what are the key areas for improvement? Provide actionable recommendations."#,
        feature = stats.feature,
        total = stats.total_feedback,
        positive = stats.positive_rate * 100.0,
        negative = stats.negative_rate * 100.0,
        quality = stats.avg_quality_score,
        corrections = stats
            .top_corrections
            .iter()
            .map(|(cat, count)| format!("  - {}: {} occurrences", cat, count))
            .collect::<Vec<_>>()
            .join("\n"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_test_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        set_test_dir(dir.path().to_path_buf());
        dir
    }

    #[test]
    fn test_record_feedback() {
        let _dir = init_test_dir();
        let entry = record_feedback(
            "ai_test",
            "Generate tests for token contract",
            "Generated 5 test cases",
            FeedbackRating::Positive,
            Some("Good coverage".to_string()),
            vec![],
        )
        .unwrap();

        assert_eq!(entry.feature, "ai_test");
        assert!(matches!(entry.rating, FeedbackRating::Positive));
    }

    #[test]
    fn test_quality_metrics() {
        let _dir = init_test_dir();
        let metrics = calculate_quality_metrics("nonexistent_feature").unwrap();
        assert_eq!(metrics.overall_score, 0.5);
    }

    #[test]
    fn test_feature_stats() {
        let _dir = init_test_dir();
        let stats = get_feature_stats("nonexistent_feature").unwrap();
        assert_eq!(stats.total_feedback, 0);
    }

    #[test]
    fn test_build_prompts() {
        let _dir = init_test_dir();
        let prompt = build_preference_aware_prompt("test prompt", "ai_test").unwrap();
        assert!(prompt.contains("test prompt"));

        let summary = build_improvement_summary_prompt("ai_test").unwrap();
        assert!(summary.contains("improvement"));
    }

    #[test]
    fn test_correction_categories() {
        assert_eq!(CorrectionCategory::Syntax.to_string(), "Syntax");
        assert_eq!(CorrectionCategory::Security.to_string(), "Security");
    }

    #[test]
    fn test_preference_learning() {
        let mut store = FeedbackStore::default();
        store.entries.push(FeedbackEntry {
            id: "1".to_string(),
            timestamp: Utc::now(),
            feature: "test".to_string(),
            prompt_summary: "test".to_string(),
            response_summary: "test".to_string(),
            rating: FeedbackRating::Negative,
            comment: None,
            corrections: vec![Correction {
                original_output: "old".to_string(),
                corrected_output: "new style".to_string(),
                reason: "Style preference".to_string(),
                category: CorrectionCategory::Style,
            }],
            context: FeedbackContext {
                model: "test".to_string(),
                contract_type: None,
                project_size: None,
                language: "rust".to_string(),
                feature_version: "1.0".to_string(),
            },
        });

        learn_preferences(&mut store);
        assert!(!store.preferences.is_empty());
    }
}
