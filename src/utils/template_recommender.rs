//! AI Template Recommendation Engine
//!
//! Provides intelligent template recommendations based on:
//! - Project requirements (keyword/tag analysis)
//! - User skill level (beginner, intermediate, advanced)
//! - Past usage history stored in the local data directory
//! - Community popularity (download counts, quality scores)
//! - Best-practice signals (verified, documented, audited)
//!
//! All data is stored and processed locally — no network calls are made.

use crate::utils::{config, templates};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ──────────────────────────────────────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────────────────────────────────────

/// User's self-reported or inferred skill level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SkillLevel {
    /// Just getting started with Soroban/Stellar.
    Beginner,
    /// Comfortable with the SDK; building real-world contracts.
    #[default]
    Intermediate,
    /// Deep Stellar expertise; values advanced patterns and optimisation.
    Advanced,
}

impl SkillLevel {
    /// Parse from a case-insensitive string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "beginner" | "b" | "novice" => Some(Self::Beginner),
            "intermediate" | "i" | "mid" | "medium" => Some(Self::Intermediate),
            "advanced" | "a" | "expert" | "senior" => Some(Self::Advanced),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            SkillLevel::Beginner => "Beginner",
            SkillLevel::Intermediate => "Intermediate",
            SkillLevel::Advanced => "Advanced",
        }
    }
}

/// Parameters that drive the recommendation algorithm.
#[derive(Debug, Clone, Default)]
pub struct RecommendationRequest {
    /// Free-form description of what the user wants to build.
    pub query: String,
    /// Explicit tags the project should match (e.g. `["defi", "token"]`).
    pub tags: Vec<String>,
    /// Self-reported or inferred skill level of the user.
    pub skill_level: SkillLevel,
    /// Maximum number of recommendations to return (default 5).
    pub limit: usize,
    /// Whether to boost templates the user has used before.
    pub personalise: bool,
    /// Whether to boost templates popular in the community.
    pub community_boost: bool,
}

impl RecommendationRequest {
    /// Build with sensible defaults.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            limit: 5,
            personalise: true,
            community_boost: true,
            ..Default::default()
        }
    }
}

/// A single recommended template with its score and human-readable explanation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    /// Template name.
    pub name: String,
    /// Template description.
    pub description: String,
    /// Tags from the registry.
    pub tags: Vec<String>,
    /// Composite recommendation score in \[0, 100\].
    pub score: f64,
    /// Ordered list of human-readable reasons why this template was suggested.
    pub reasons: Vec<String>,
    /// How well the template matches the requested tags/query (0–100).
    pub relevance: u8,
    /// Community popularity score (0–100).
    pub popularity: u8,
    /// Whether the user has used this template before.
    pub previously_used: bool,
    /// Skill-level suitability label.
    pub skill_fit: String,
}

// ──────────────────────────────────────────────────────────────────────────────
// Usage history (local persistence)
// ──────────────────────────────────────────────────────────────────────────────

/// A single recorded use of a template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateUsageEvent {
    /// Template name that was used.
    pub template_name: String,
    /// ISO-8601 timestamp.
    pub timestamp: String,
    /// How the template was accessed: "install", "new", "recommend".
    pub action: String,
}

/// Read all locally persisted usage events.
pub fn load_usage_history() -> Result<Vec<TemplateUsageEvent>> {
    let path = usage_log_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path)?;
    let events: Vec<TemplateUsageEvent> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    Ok(events)
}

/// Append a usage event to the local log.
pub fn record_usage(template_name: &str, action: &str) -> Result<()> {
    let path = usage_log_path()?;
    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let event = TemplateUsageEvent {
        template_name: template_name.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        action: action.to_string(),
    };
    let json = serde_json::to_string(&event)?;
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{}", json)?;
    Ok(())
}

/// Path of the local template usage log file.
fn usage_log_path() -> Result<PathBuf> {
    let data_dir = config::get_data_dir()?;
    Ok(data_dir.join("template_usage.log"))
}

/// Build a map of `template_name → use_count` from the persisted history.
pub fn usage_counts() -> Result<HashMap<String, u32>> {
    let events = load_usage_history()?;
    let mut counts: HashMap<String, u32> = HashMap::new();
    for ev in events {
        *counts.entry(ev.template_name).or_insert(0) += 1;
    }
    Ok(counts)
}

// ──────────────────────────────────────────────────────────────────────────────
// Scoring helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Tokenise a string into lowercase words (splits on whitespace and punctuation).
fn tokenise(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

/// Return how many tokens from `query_tokens` appear in `target_tokens`.
fn token_overlap(query_tokens: &[String], target_tokens: &[String]) -> usize {
    query_tokens
        .iter()
        .filter(|q| target_tokens.contains(q))
        .count()
}

/// Compute a relevance score in \[0, 100\] for `entry` against `request`.
///
/// The score is weighted across four sources:
/// 1. Exact tag overlap (highest weight)
/// 2. Query-to-description token match
/// 3. Query-to-name token match
/// 4. Query-to-tag token match
fn relevance_score(entry: &templates::TemplateEntry, request: &RecommendationRequest) -> u8 {
    let mut score: f64 = 0.0;

    let query_tokens = tokenise(&request.query);
    let tag_tokens_in_query: Vec<String> = request.tags.iter().map(|t| t.to_lowercase()).collect();

    // --- Tag exact match (40 pts max) ---
    if !request.tags.is_empty() {
        let entry_tags_lower: Vec<String> = entry.tags.iter().map(|t| t.to_lowercase()).collect();
        let matched = request
            .tags
            .iter()
            .filter(|rt| entry_tags_lower.contains(&rt.to_lowercase()))
            .count();
        score += (matched as f64 / request.tags.len() as f64) * 40.0;
    } else if !query_tokens.is_empty() {
        // No explicit tags — use query tokens against entry tags (20 pts max)
        let entry_tags_lower: Vec<String> = entry.tags.iter().map(|t| t.to_lowercase()).collect();
        let matched = token_overlap(&query_tokens, &entry_tags_lower);
        let max = entry.tags.len().max(1);
        score += (matched as f64 / max as f64).min(1.0) * 20.0;
    }

    // --- Query to description (30 pts max) ---
    if !query_tokens.is_empty() {
        let desc_tokens = tokenise(&entry.description);
        let overlap = token_overlap(&query_tokens, &desc_tokens);
        let max = query_tokens.len().max(1);
        score += (overlap as f64 / max as f64).min(1.0) * 30.0;
    }

    // --- Query to name (20 pts max) ---
    if !query_tokens.is_empty() {
        let name_tokens = tokenise(&entry.name);
        let overlap = token_overlap(&query_tokens, &name_tokens);
        let max = query_tokens.len().max(1);
        score += (overlap as f64 / max as f64).min(1.0) * 20.0;
    }

    // --- Explicit tag-tokens in query against description/name (10 pts max) ---
    if !tag_tokens_in_query.is_empty() {
        let combined = format!("{} {}", entry.name, entry.description);
        let combined_tokens = tokenise(&combined);
        let overlap = token_overlap(&tag_tokens_in_query, &combined_tokens);
        let max = tag_tokens_in_query.len().max(1);
        score += (overlap as f64 / max as f64).min(1.0) * 10.0;
    }

    score.round().clamp(0.0, 100.0) as u8
}

/// Compute a community popularity score in \[0, 100\] for `entry`.
///
/// Uses a log-scale normalisation so very popular templates don't
/// completely dominate less-known but still good ones.
fn popularity_score(entry: &templates::TemplateEntry, max_downloads: u32) -> u8 {
    if max_downloads == 0 {
        // No community data yet — fall back to quality score.
        return entry.quality_score();
    }
    // log2 normalise: (log2(downloads+1) / log2(max+1)) * 60 + quality * 40%
    let log_score = ((entry.downloads as f64 + 1.0).log2()
        / (max_downloads as f64 + 1.0).log2().max(1.0))
        * 60.0;
    let quality = entry.quality_score() as f64 * 0.4;
    (log_score + quality).round().clamp(0.0, 100.0) as u8
}

/// Return a skill-fit label and a point bonus (0–15) for `entry` and `skill_level`.
///
/// - Beginners get a bonus for templates that are verified, well-documented,
///   and have higher download counts (community-proven safe starting points).
/// - Advanced users get a bonus for templates that have been audited and have
///   higher complexity tags (multisig, governance, etc.).
fn skill_fit(entry: &templates::TemplateEntry, skill_level: SkillLevel) -> (&'static str, f64) {
    let advanced_tags = ["multisig", "governance", "dao", "amm", "dex", "lending"];
    let beginner_tags = ["hello-world", "simple", "counter", "token", "nft", "sep-41"];

    let entry_tags_lower: Vec<String> = entry.tags.iter().map(|t| t.to_lowercase()).collect();
    let has_advanced = advanced_tags
        .iter()
        .any(|t| entry_tags_lower.contains(&t.to_string()));
    let has_beginner = beginner_tags
        .iter()
        .any(|t| entry_tags_lower.contains(&t.to_string()));

    match skill_level {
        SkillLevel::Beginner => {
            if has_beginner && entry.documented && entry.verified {
                ("Great for beginners", 15.0)
            } else if entry.documented {
                ("Suitable for beginners", 8.0)
            } else if has_advanced {
                ("Advanced — may be challenging", -5.0)
            } else {
                ("Suitable", 0.0)
            }
        }
        SkillLevel::Intermediate => {
            // Intermediate users benefit from everything.
            ("Good fit", 5.0)
        }
        SkillLevel::Advanced => {
            if has_advanced {
                ("Excellent for advanced use", 15.0)
            } else if entry.security_review.as_ref().map_or(false, |sr| {
                sr.status == "audited" && sr.score.unwrap_or(0.0) >= 90.0
            }) {
                ("Production-grade quality", 10.0)
            } else {
                ("Suitable", 0.0)
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Main recommendation entry point
// ──────────────────────────────────────────────────────────────────────────────

/// Generate template recommendations based on the provided request.
///
/// Loads the local registry (and merges with the remote registry if available),
/// then scores every template and returns the top `request.limit` results.
pub async fn recommend(request: &RecommendationRequest) -> Result<Vec<Recommendation>> {
    let registry = templates::load_registry().await?;
    let all_templates = &registry.templates;

    if all_templates.is_empty() {
        return Ok(Vec::new());
    }

    // Load personal usage history for personalisation.
    let personal_usage = if request.personalise {
        usage_counts().unwrap_or_default()
    } else {
        HashMap::new()
    };

    // Find the maximum download count for normalisation.
    let max_downloads = all_templates.iter().map(|t| t.downloads).max().unwrap_or(1);

    let mut scored: Vec<Recommendation> = all_templates
        .iter()
        .map(|entry| score_template(entry, request, &personal_usage, max_downloads))
        .collect();

    // Sort descending by composite score, then by name for stability.
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });

    let limit = if request.limit == 0 { 5 } else { request.limit };
    scored.truncate(limit);

    Ok(scored)
}

/// Score a single template entry and produce a `Recommendation`.
fn score_template(
    entry: &templates::TemplateEntry,
    request: &RecommendationRequest,
    personal_usage: &HashMap<String, u32>,
    max_downloads: u32,
) -> Recommendation {
    let mut composite: f64 = 0.0;
    let mut reasons: Vec<String> = Vec::new();

    // ── Relevance (max 40 pts) ────────────────────────────────────────────────
    let rel = relevance_score(entry, request);
    composite += rel as f64 * 0.40;
    if rel >= 60 {
        if !request.tags.is_empty() {
            let matched_tags: Vec<&String> = request
                .tags
                .iter()
                .filter(|t| entry.tags.iter().any(|et| et.eq_ignore_ascii_case(t)))
                .collect();
            if !matched_tags.is_empty() {
                reasons.push(format!(
                    "Matches tag{}: {}",
                    if matched_tags.len() > 1 { "s" } else { "" },
                    matched_tags
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        if !request.query.is_empty() {
            reasons.push(format!(
                "Strong match for \"{}\"",
                request.query.chars().take(40).collect::<String>()
            ));
        }
    }

    // ── Community popularity (max 25 pts) ────────────────────────────────────
    let pop = popularity_score(entry, max_downloads);
    if request.community_boost {
        composite += pop as f64 * 0.25;
        if entry.downloads > 500 {
            reasons.push(format!("{} community downloads", entry.downloads));
        }
    }

    // ── Quality / trust signals (max 20 pts) ─────────────────────────────────
    let quality = entry.quality_score() as f64;
    composite += quality * 0.20;
    if entry.verified {
        reasons.push("Verified template".to_string());
    }
    if entry.documented {
        reasons.push("Has documentation".to_string());
    }
    if let Some(ref sr) = entry.security_review {
        if sr.status == "audited" {
            if let Some(score) = sr.score {
                reasons.push(format!("Security-audited (score {:.0}/100)", score));
            } else {
                reasons.push("Security-audited".to_string());
            }
        }
    }

    // ── Skill-level fit (max 15 pts) ─────────────────────────────────────────
    let (skill_label, skill_bonus) = skill_fit(entry, request.skill_level);
    composite += skill_bonus.max(0.0);
    if skill_bonus > 0.0 {
        reasons.push(skill_label.to_string());
    }

    // ── Personalisation (max 10 pts) ─────────────────────────────────────────
    let use_count = personal_usage.get(&entry.name).copied().unwrap_or(0);
    let previously_used = use_count > 0;
    if request.personalise && previously_used {
        // Gentle boost for familiarity, capped at 10.
        let personal_bonus = (use_count as f64).log2().min(10.0);
        composite += personal_bonus;
        reasons.push(format!(
            "Used {} time{} before",
            use_count,
            if use_count > 1 { "s" } else { "" }
        ));
    }

    Recommendation {
        name: entry.name.clone(),
        description: entry.description.clone(),
        tags: entry.tags.clone(),
        score: composite.clamp(0.0, 100.0),
        reasons,
        relevance: rel,
        popularity: pop,
        previously_used,
        skill_fit: skill_label.to_string(),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Explanation helpers (for CLI display)
// ──────────────────────────────────────────────────────────────────────────────

/// Format a recommendation explanation for human-readable output.
pub fn format_explanation(rec: &Recommendation) -> String {
    let mut parts = Vec::new();
    parts.push(format!("Score: {:.0}/100", rec.score));
    parts.push(format!("Relevance: {}/100", rec.relevance));
    parts.push(format!("Popularity: {}/100", rec.popularity));
    parts.push(format!("Skill fit: {}", rec.skill_fit));
    if !rec.reasons.is_empty() {
        parts.push(format!("Why: {}", rec.reasons.join(" · ")));
    }
    parts.join("  |  ")
}

// ──────────────────────────────────────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::templates::{MaintenanceStatus, TemplateEntry, TemplateSource};

    fn make_entry(name: &str, tags: &[&str], downloads: u32, verified: bool) -> TemplateEntry {
        TemplateEntry {
            name: name.to_string(),
            description: format!("A {} contract for testing", name),
            version: "1.0.0".to_string(),
            source: TemplateSource::Builtin {
                id: name.to_string(),
            },
            tags: tags.iter().map(|s| s.to_string()).collect(),
            path: None,
            author: "Test Author".to_string(),
            downloads,
            verified,
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
            cli_version_min: None,
            cli_version_max: None,
            documented: verified,
            maintenance: MaintenanceStatus::Active,
            license: Some("MIT".to_string()),
            repository: None,
            repository_url: None,
            homepage: None,
            documentation: None,
            categories: vec![],
            featured: false,
            security_review: None,
            changelog: None,
        }
    }

    #[test]
    fn test_tokenise() {
        let tokens = tokenise("DeFi AMM dex swap");
        assert!(tokens.contains(&"defi".to_string()));
        assert!(tokens.contains(&"amm".to_string()));
    }

    #[test]
    fn test_relevance_tag_match() {
        let entry = make_entry("uniswap-v2", &["defi", "dex", "amm"], 1200, true);
        let req = RecommendationRequest {
            tags: vec!["defi".to_string(), "dex".to_string()],
            ..Default::default()
        };
        let score = relevance_score(&entry, &req);
        // Two exact tag matches out of two requested → should be high.
        assert!(score >= 40, "Expected relevance >= 40, got {}", score);
    }

    #[test]
    fn test_relevance_query_match() {
        let entry = make_entry("lending-pool", &["defi", "lending"], 874, true);
        let req = RecommendationRequest {
            query: "lending borrowing protocol".to_string(),
            ..Default::default()
        };
        let score = relevance_score(&entry, &req);
        assert!(score > 0, "Expected some relevance for query match");
    }

    #[test]
    fn test_relevance_no_match() {
        let entry = make_entry("multisig-vault", &["wallet", "multisig"], 300, false);
        let req = RecommendationRequest {
            tags: vec!["nft".to_string()],
            query: "non-fungible token collectible".to_string(),
            ..Default::default()
        };
        let score = relevance_score(&entry, &req);
        assert!(
            score < 30,
            "Expected low relevance for non-matching template, got {}",
            score
        );
    }

    #[test]
    fn test_popularity_score_normalisation() {
        let entry = make_entry("popular", &["defi"], 1000, true);
        let pop = popularity_score(&entry, 1000);
        assert!(pop > 50, "Popular template should score > 50");

        let rare = make_entry("rare", &["defi"], 10, true);
        let pop_rare = popularity_score(&rare, 1000);
        assert!(
            pop > pop_rare,
            "Popular template should outscore rare template"
        );
    }

    #[test]
    fn test_skill_fit_beginner() {
        let entry = make_entry("simple-counter", &["simple", "counter"], 50, true);
        let (label, bonus) = skill_fit(&entry, SkillLevel::Beginner);
        // Should recognise a beginner-friendly template.
        assert!(bonus > 0.0, "Beginner entry should have positive bonus");
        assert!(!label.is_empty());
    }

    #[test]
    fn test_skill_fit_advanced_on_governance() {
        let mut entry = make_entry("dao-governance", &["dao", "governance"], 500, true);
        entry.security_review = Some(crate::utils::templates::SecurityReview {
            status: "audited".to_string(),
            audited_at: None,
            auditor: None,
            findings: None,
            score: Some(95.0),
        });
        let (label, bonus) = skill_fit(&entry, SkillLevel::Advanced);
        assert!(
            bonus > 0.0,
            "Governance/advanced template should bonus for advanced users"
        );
        assert!(!label.is_empty());
    }

    #[test]
    fn test_score_template_personalisation() {
        let entry = make_entry("my-token", &["token"], 100, true);
        let req = RecommendationRequest {
            personalise: true,
            community_boost: false,
            ..Default::default()
        };
        let mut personal_usage = HashMap::new();
        personal_usage.insert("my-token".to_string(), 3u32);

        let rec = score_template(&entry, &req, &personal_usage, 1000);
        assert!(
            rec.previously_used,
            "Should detect previously_used when in history"
        );
        assert!(
            rec.reasons
                .iter()
                .any(|r| r.contains("Used") && r.contains("time")),
            "Should mention prior use in reasons"
        );
    }

    #[test]
    fn test_format_explanation_non_empty() {
        let rec = Recommendation {
            name: "test".to_string(),
            description: "desc".to_string(),
            tags: vec![],
            score: 75.0,
            reasons: vec!["Verified template".to_string()],
            relevance: 80,
            popularity: 60,
            previously_used: false,
            skill_fit: "Good fit".to_string(),
        };
        let explanation = format_explanation(&rec);
        assert!(
            explanation.contains("75"),
            "Explanation should include score"
        );
        assert!(
            explanation.contains("Verified template"),
            "Explanation should include reasons"
        );
    }

    #[test]
    fn test_skill_level_from_str() {
        assert_eq!(SkillLevel::from_str("beginner"), Some(SkillLevel::Beginner));
        assert_eq!(
            SkillLevel::from_str("INTERMEDIATE"),
            Some(SkillLevel::Intermediate)
        );
        assert_eq!(SkillLevel::from_str("expert"), Some(SkillLevel::Advanced));
        assert_eq!(SkillLevel::from_str("unknown"), None);
    }
}
