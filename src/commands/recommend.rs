//! `starforge template recommend` — AI-powered template recommendation engine.
//!
//! Usage examples:
//! ```bash
//! # Ask for a DeFi recommendation
//! starforge template recommend "decentralized exchange"
//!
//! # Filter by tags and skill level
//! starforge template recommend --tags defi,amm --skill intermediate
//!
//! # Show top-3 with full explanation
//! starforge template recommend "token contract" --limit 3 --explain
//!
//! # Disable personalisation (ignore past usage)
//! starforge template recommend --no-personalise
//! ```

use crate::utils::{print as p, template_recommender as rec};
use anyhow::Result;

/// Handle the `recommend` sub-command.
///
/// # Parameters
/// - `query` – free-form text describing the project to build
/// - `tags` – comma-separated tag filters (e.g. `"defi,amm"`)
/// - `skill` – user skill level: beginner | intermediate | advanced
/// - `limit` – max results to display (default 5)
/// - `explain` – if true, print the full scoring breakdown
/// - `no_personalise` – if true, ignore past usage history
/// - `no_community` – if true, ignore community download counts
pub async fn handle(
    query: String,
    tags: Option<String>,
    skill: Option<String>,
    limit: usize,
    explain: bool,
    no_personalise: bool,
    no_community: bool,
) -> Result<()> {
    // ── Parse inputs ─────────────────────────────────────────────────────────
    let tag_list: Vec<String> = tags
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let skill_level = match skill.as_deref() {
        Some(s) => match rec::SkillLevel::from_str(s) {
            Some(level) => level,
            None => {
                p::warn(&format!(
                    "Unknown skill level '{}'. Using 'intermediate'. Valid values: beginner, intermediate, advanced.",
                    s
                ));
                rec::SkillLevel::Intermediate
            }
        },
        None => rec::SkillLevel::default(),
    };

    let actual_limit = if limit == 0 { 5 } else { limit };

    let request = rec::RecommendationRequest {
        query: query.clone(),
        tags: tag_list,
        skill_level,
        limit: actual_limit,
        personalise: !no_personalise,
        community_boost: !no_community,
    };

    // ── Print header ─────────────────────────────────────────────────────────
    p::header("AI Template Recommendations");

    // Show what parameters are being used.
    if !query.is_empty() {
        p::kv("Query", &query);
    }
    if !request.tags.is_empty() {
        p::kv("Tags", &request.tags.join(", "));
    }
    p::kv("Skill level", skill_level.label());
    p::kv(
        "Personalisation",
        if request.personalise { "on" } else { "off" },
    );
    p::kv(
        "Community boost",
        if request.community_boost { "on" } else { "off" },
    );
    println!();

    // ── Run the recommendation engine ────────────────────────────────────────
    let recommendations = rec::recommend(&request).await?;

    if recommendations.is_empty() {
        p::info(
            "No templates found in the registry. Run `starforge template init` to populate it.",
        );
        return Ok(());
    }

    // ── Display results ──────────────────────────────────────────────────────
    p::kv("Recommendations", &recommendations.len().to_string());
    println!();

    for (i, rec_item) in recommendations.iter().enumerate() {
        println!(
            "  {:>2}. {}  [score {:.0}/100]{}",
            i + 1,
            rec_item.name,
            rec_item.score,
            if rec_item.previously_used {
                " ★ previously used"
            } else {
                ""
            },
        );
        p::kv("Description", &rec_item.description);

        if !rec_item.tags.is_empty() {
            p::kv("Tags", &rec_item.tags.join(", "));
        }

        if !rec_item.reasons.is_empty() {
            p::kv("Why recommended", &rec_item.reasons.join("  ·  "));
        }

        if explain {
            let explanation = rec::format_explanation(rec_item);
            p::kv("Scoring breakdown", &explanation);
        }

        if i + 1 < recommendations.len() {
            println!();
        }
    }

    println!();
    p::info("Use a template with: starforge template install <name>");
    p::info("See full details with: starforge template info <name>");

    Ok(())
}
