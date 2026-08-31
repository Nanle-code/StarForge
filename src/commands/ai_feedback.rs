use crate::utils::ai_feedback as af;
use crate::utils::ollama;
use crate::utils::print as p;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::*;

#[derive(Subcommand)]
pub enum AiFeedbackCommands {
    /// Record feedback on an AI-generated response
    Record(RecordArgs),

    /// View feedback history and statistics
    Stats(StatsArgs),

    /// View learned user preferences
    Preferences,

    /// View quality metrics for a feature
    Quality(QualityArgs),

    /// Generate an improvement summary for a feature
    Improve(ImproveArgs),

    /// Build a prompt incorporating learned preferences
    Prompt(PromptArgs),
}

#[derive(Args)]
pub struct RecordArgs {
    /// Feature name (e.g., ai_test, ai_audit, ai_debug)
    pub feature: String,

    /// Brief summary of the prompt
    #[arg(long)]
    pub prompt_summary: String,

    /// Brief summary of the AI response
    #[arg(long)]
    pub response_summary: String,

    /// Rating: positive, negative, neutral, partial
    #[arg(long, default_value = "positive")]
    pub rating: String,

    /// Optional comment
    #[arg(long)]
    pub comment: Option<String>,

    /// Correction category (syntax, logic, style, security, performance, documentation, test_coverage)
    #[arg(long)]
    pub correction_category: Option<String>,

    /// Original output that was incorrect
    #[arg(long)]
    pub original_output: Option<String>,

    /// Corrected output
    #[arg(long)]
    pub corrected_output: Option<String>,
}

#[derive(Args)]
pub struct StatsArgs {
    /// Feature name to filter by (optional)
    #[arg(long)]
    pub feature: Option<String>,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args)]
pub struct QualityArgs {
    /// Feature name to analyze
    pub feature: String,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args)]
pub struct ImproveArgs {
    /// Feature name to generate improvement plan for
    pub feature: String,

    /// Use Ollama for AI-enhanced improvement plan
    #[arg(long, default_value = "false")]
    pub use_ai: bool,

    /// Ollama model to use
    #[arg(long, default_value = "codellama:7b")]
    pub model: String,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args)]
pub struct PromptArgs {
    /// Feature name to build prompt for
    pub feature: String,

    /// Base prompt to enhance
    #[arg(long)]
    pub base_prompt: String,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,
}

pub async fn handle(cmd: AiFeedbackCommands) -> Result<()> {
    match cmd {
        AiFeedbackCommands::Record(args) => handle_record(args),
        AiFeedbackCommands::Stats(args) => handle_stats(args),
        AiFeedbackCommands::Preferences => handle_preferences(),
        AiFeedbackCommands::Quality(args) => handle_quality(args),
        AiFeedbackCommands::Improve(args) => handle_improve(args).await,
        AiFeedbackCommands::Prompt(args) => handle_prompt(args),
    }
}

fn handle_record(args: RecordArgs) -> Result<()> {
    p::header("Record Feedback");

    let rating = match args.rating.to_lowercase().as_str() {
        "positive" => af::FeedbackRating::Positive,
        "negative" => af::FeedbackRating::Negative,
        "neutral" => af::FeedbackRating::Neutral,
        "partial" => af::FeedbackRating::Partial,
        _ => {
            p::warn(&format!(
                "Unknown rating '{}', defaulting to Neutral",
                args.rating
            ));
            af::FeedbackRating::Neutral
        }
    };

    let corrections = if let (Some(cat), Some(orig), Some(corrected)) = (
        &args.correction_category,
        &args.original_output,
        &args.corrected_output,
    ) {
        let category = match cat.to_lowercase().as_str() {
            "syntax" => af::CorrectionCategory::Syntax,
            "logic" => af::CorrectionCategory::Logic,
            "style" => af::CorrectionCategory::Style,
            "security" => af::CorrectionCategory::Security,
            "performance" => af::CorrectionCategory::Performance,
            "documentation" => af::CorrectionCategory::Documentation,
            "test_coverage" => af::CorrectionCategory::TestCoverage,
            _ => af::CorrectionCategory::Logic,
        };
        vec![af::Correction {
            original_output: orig.clone(),
            corrected_output: corrected.clone(),
            reason: args.comment.clone().unwrap_or_default(),
            category,
        }]
    } else {
        vec![]
    };

    let entry = af::record_feedback(
        &args.feature,
        &args.prompt_summary,
        &args.response_summary,
        rating.clone(),
        args.comment,
        corrections,
    )?;

    p::success(&format!("Feedback recorded (ID: {})", &entry.id[..8]));
    p::kv("Feature", &args.feature);
    p::kv("Rating", &rating.to_string());

    if !entry.corrections.is_empty() {
        p::kv("Corrections", &entry.corrections.len().to_string());
    }

    // Update preferences from new feedback
    let mut store = af::load_store()?;
    af::learn_preferences(&mut store);
    af::save_store(&store)?;

    p::success("Preferences updated from feedback");

    Ok(())
}

fn handle_stats(args: StatsArgs) -> Result<()> {
    p::header("Feedback Statistics");

    let store = af::load_store()?;
    let entries = if let Some(ref feature) = args.feature {
        store
            .entries
            .iter()
            .filter(|e| e.feature == *feature)
            .collect::<Vec<_>>()
    } else {
        store.entries.iter().collect::<Vec<_>>()
    };

    let total = entries.len();
    let positive = entries
        .iter()
        .filter(|e| matches!(e.rating, af::FeedbackRating::Positive))
        .count();
    let negative = entries
        .iter()
        .filter(|e| matches!(e.rating, af::FeedbackRating::Negative))
        .count();

    let mut feature_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for entry in &store.entries {
        *feature_counts.entry(entry.feature.clone()).or_insert(0) += 1;
    }

    match args.format.as_str() {
        "json" => {
            println!(
                "{}",
                serde_json::json!({
                    "total": total,
                    "positive": positive,
                    "negative": negative,
                    "features": feature_counts,
                    "preferences": store.preferences,
                })
            );
        }
        _ => {
            p::kv("Total feedback entries", &total.to_string());
            p::kv("Positive", &positive.to_string());
            p::kv("Negative", &negative.to_string());
            if total > 0 {
                p::kv(
                    "Positive rate",
                    &format!("{:.1}%", positive as f64 / total as f64 * 100.0),
                );
            }
            println!();
            println!("{}", "Feedback by Feature:".bold());
            for (feature, count) in &feature_counts {
                println!("  {} — {} entries", feature.bright_white(), count);
            }
        }
    }

    Ok(())
}

fn handle_preferences() -> Result<()> {
    p::header("Learned Preferences");

    let store = af::load_store()?;

    if store.preferences.is_empty() {
        p::info("No preferences learned yet. Record some feedback to start learning.");
        return Ok(());
    }

    println!();
    for pref in &store.preferences {
        println!(
            "  {} {}",
            "●".cyan(),
            format!("{:?}", pref.preference_type).bright_white().bold()
        );
        println!("    Value: {}", pref.value);
        println!(
            "    Confidence: {:.0}% (learned from {} corrections)",
            pref.confidence * 100.0,
            pref.learned_from
        );
        println!(
            "    Last updated: {}",
            pref.last_updated.format("%Y-%m-%d %H:%M")
        );
        println!();
    }

    Ok(())
}

fn handle_quality(args: QualityArgs) -> Result<()> {
    p::header("Quality Metrics");

    let metrics = af::calculate_quality_metrics(&args.feature)?;

    match args.format.as_str() {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&metrics)?);
        }
        _ => {
            p::kv("Feature", &args.feature);
            println!();
            p::kv(
                "Accuracy",
                &format!("{:.1}%", metrics.accuracy_score * 100.0),
            );
            p::kv(
                "Relevance",
                &format!("{:.1}%", metrics.relevance_score * 100.0),
            );
            p::kv(
                "Completeness",
                &format!("{:.1}%", metrics.completeness_score * 100.0),
            );
            p::kv("Clarity", &format!("{:.1}%", metrics.clarity_score * 100.0));
            p::separator();
            p::kv(
                "Overall",
                &format!("{:.1}/100", metrics.overall_score * 100.0),
            );
        }
    }

    Ok(())
}

async fn handle_improve(args: ImproveArgs) -> Result<()> {
    p::header("Improvement Plan");

    let prompt = af::build_improvement_summary_prompt(&args.feature)?;

    if args.use_ai {
        if ollama::is_ollama_running().await {
            p::info(&format!(
                "Generating improvement plan with {}...",
                args.model
            ));
            let response = ollama::generate(
                &args.model,
                &prompt,
                Some(ollama::GenerateOptions {
                    temperature: Some(0.3),
                    num_predict: Some(2048),
                    num_ctx: Some(4096),
                }),
            )
            .await
            .context("AI improvement plan failed")?;

            match args.format.as_str() {
                "json" => println!("{}", response.response),
                _ => {
                    println!();
                    println!("{}", response.response);
                }
            }
        } else {
            p::warn("Ollama not running. Using local improvement plan.");
            print_local_improvement(&args.feature)?;
        }
    } else {
        print_local_improvement(&args.feature)?;
    }

    Ok(())
}

fn print_local_improvement(feature: &str) -> Result<()> {
    let stats = af::get_feature_stats(feature)?;

    println!();
    println!("Improvement Plan for '{}':", feature.bold());
    println!();

    if stats.total_feedback == 0 {
        p::info("No feedback recorded yet. Start by recording feedback:");
        println!("  starforge ai feedback record {} --prompt-summary \"...\" --response-summary \"...\" --rating positive", feature);
        return Ok(());
    }

    p::kv("Total feedback", &stats.total_feedback.to_string());
    p::kv(
        "Positive rate",
        &format!("{:.1}%", stats.positive_rate * 100.0),
    );
    p::kv(
        "Quality score",
        &format!("{:.1}/100", stats.avg_quality_score * 100.0),
    );
    println!();

    if !stats.top_corrections.is_empty() {
        println!("{}", "Top Correction Categories:".bold());
        for (cat, count) in &stats.top_corrections {
            println!("  {} — {} occurrences", cat.to_string().yellow(), count);
        }
        println!();
    }

    println!("{}", "Recommendations:".bold());
    if stats.avg_quality_score < 0.5 {
        println!("  → Focus on accuracy: review AI responses before accepting");
    }
    if stats.negative_rate > 0.3 {
        println!("  → High negative feedback: investigate common failure modes");
    }
    if stats
        .top_corrections
        .iter()
        .any(|(c, _)| *c == af::CorrectionCategory::Security)
    {
        println!("  → Security corrections detected: add security-focused prompts");
    }

    Ok(())
}

fn handle_prompt(args: PromptArgs) -> Result<()> {
    let prompt = af::build_preference_aware_prompt(&args.base_prompt, &args.feature)?;

    match args.format.as_str() {
        "json" => {
            println!("{}", serde_json::json!({ "prompt": prompt }));
        }
        _ => {
            println!("{}", prompt);
        }
    }

    Ok(())
}
