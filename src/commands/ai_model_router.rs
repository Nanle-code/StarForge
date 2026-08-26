//! CLI for intelligent AI model selection and routing (issue #491).

use crate::utils::ai_model_router as router;
use crate::utils::print as p;
use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub enum AiModelRouterCommands {
    /// Classify a task by complexity and category
    Classify(ClassifyArgs),

    /// Select the optimal model for a task
    Select(SelectArgs),

    /// Show or update routing preferences
    #[command(subcommand)]
    Preferences(PreferencesCommands),

    /// Show model performance metrics from telemetry
    Stats(StatsArgs),

    /// Record a learned preference for a task category
    Learn(LearnArgs),
}

#[derive(Args)]
pub struct ClassifyArgs {
    /// Task prompt or description to classify
    pub prompt: String,

    /// Optional explicit category hint
    #[arg(long)]
    pub category: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct SelectArgs {
    /// Task prompt or description
    pub prompt: String,

    /// Optional explicit category hint
    #[arg(long)]
    pub category: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum PreferencesCommands {
    /// Show current routing preferences
    Show {
        #[arg(long)]
        json: bool,
    },
    /// Update routing preferences
    Set(SetPreferencesArgs),
}

#[derive(Args)]
pub struct SetPreferencesArgs {
    /// Optimize for cost (prefer cheaper models)
    #[arg(long)]
    pub cost_sensitive: Option<bool>,

    /// Prefer local Ollama when available
    #[arg(long)]
    pub prefer_local: Option<bool>,

    /// Prefer faster models over higher quality
    #[arg(long)]
    pub prefer_speed: Option<bool>,

    /// Preferred provider: openai, anthropic, ollama
    #[arg(long)]
    pub provider: Option<String>,

    /// Maximum cost tier (0=free/local, 1=cheap, 2=mid, 3=premium)
    #[arg(long)]
    pub max_cost_tier: Option<u8>,
}

#[derive(Args)]
pub struct StatsArgs {
    /// Only include records from the last N days
    #[arg(long)]
    pub days: Option<u32>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct LearnArgs {
    /// Task category to remember preference for
    pub category: String,

    /// Preferred model name
    pub model: String,
}

pub async fn handle(cmd: AiModelRouterCommands) -> Result<()> {
    match cmd {
        AiModelRouterCommands::Classify(args) => handle_classify(args),
        AiModelRouterCommands::Select(args) => handle_select(args).await,
        AiModelRouterCommands::Preferences(prefs) => handle_preferences(prefs),
        AiModelRouterCommands::Stats(args) => handle_stats(args),
        AiModelRouterCommands::Learn(args) => handle_learn(args),
    }
}

fn handle_classify(args: ClassifyArgs) -> Result<()> {
    let category = args
        .category
        .as_deref()
        .map(router::parse_category)
        .transpose()?;

    let classification = router::classify_task(&args.prompt, category);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&classification)?);
        return Ok(());
    }

    p::header("Task Classification");
    p::separator();
    p::kv("Complexity", &classification.complexity.to_string());
    p::kv("Category", &classification.category.to_string());
    p::kv("Est. tokens", &classification.estimated_tokens.to_string());
    p::kv(
        "Requires reasoning",
        &classification.requires_reasoning.to_string(),
    );
    p::kv("Requires code", &classification.requires_code.to_string());
    p::kv(
        "Confidence",
        &format!("{:.0}%", classification.confidence * 100.0),
    );
    if !classification.signals.is_empty() {
        p::kv("Signals", &classification.signals.join(", "));
    }
    p::separator();
    Ok(())
}

async fn handle_select(args: SelectArgs) -> Result<()> {
    let category = args
        .category
        .as_deref()
        .map(router::parse_category)
        .transpose()?;

    let decision = router::route_task(&args.prompt, category, None).await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&decision)?);
        return Ok(());
    }

    p::header("Model Routing Decision");
    p::separator();
    p::kv("Provider", &format!("{:?}", decision.provider));
    p::kv("Model", &decision.model);
    p::kv("Complexity", &decision.complexity.to_string());
    p::kv("Category", &decision.category.to_string());
    p::kv("Reason", &decision.reason);
    if let Some(cost) = decision.estimated_cost_usd {
        p::kv("Est. cost", &format!("${:.4}", cost));
    }
    if !decision.fallback_chain.is_empty() {
        println!();
        p::info("Fallback chain:");
        for (i, (provider, model)) in decision.fallback_chain.iter().enumerate() {
            println!("  {}. {:?} / {}", i + 1, provider, model);
        }
    }
    p::separator();
    Ok(())
}

fn handle_preferences(cmd: PreferencesCommands) -> Result<()> {
    match cmd {
        PreferencesCommands::Show { json } => {
            let prefs = router::load_preferences()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&prefs)?);
            } else {
                p::header("Model Routing Preferences");
                p::separator();
                p::kv("Cost sensitive", &prefs.cost_sensitive.to_string());
                p::kv("Prefer local", &prefs.prefer_local.to_string());
                p::kv("Prefer speed", &prefs.prefer_speed.to_string());
                p::kv(
                    "Preferred provider",
                    &prefs
                        .preferred_provider
                        .as_ref()
                        .map(|p| format!("{:?}", p))
                        .unwrap_or_else(|| "none".into()),
                );
                p::kv("Max cost tier", &prefs.max_cost_tier.to_string());
                p::separator();
            }
            Ok(())
        }
        PreferencesCommands::Set(args) => {
            let mut prefs = router::load_preferences()?;
            if let Some(v) = args.cost_sensitive {
                prefs.cost_sensitive = v;
            }
            if let Some(v) = args.prefer_local {
                prefs.prefer_local = v;
            }
            if let Some(v) = args.prefer_speed {
                prefs.prefer_speed = v;
            }
            if let Some(ref provider) = args.provider {
                prefs.preferred_provider = Some(parse_provider(provider)?);
            }
            if let Some(tier) = args.max_cost_tier {
                prefs.max_cost_tier = tier;
            }
            router::save_preferences(&prefs)?;
            p::success("Routing preferences updated.");
            Ok(())
        }
    }
}

fn parse_provider(s: &str) -> Result<crate::utils::ai::AIProvider> {
    use crate::utils::ai::AIProvider;
    match s.to_lowercase().as_str() {
        "openai" => Ok(AIProvider::OpenAI),
        "anthropic" | "claude" => Ok(AIProvider::Anthropic),
        "ollama" | "local" => Ok(AIProvider::Ollama),
        other => anyhow::bail!(
            "Unknown provider '{}'. Use: openai, anthropic, ollama",
            other
        ),
    }
}

fn handle_stats(args: StatsArgs) -> Result<()> {
    let stats = router::model_performance_stats(args.days)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
        return Ok(());
    }

    p::header("Model Performance Metrics");
    p::separator();

    if stats.is_empty() {
        p::info("No model performance data recorded yet.");
        p::info("Enable AI telemetry with: starforge ai-telemetry enable");
    } else {
        let headers = &[
            "Provider",
            "Model",
            "Feature",
            "Calls",
            "Success %",
            "Avg ms",
            "Avg tokens",
        ];
        let rows: Vec<Vec<String>> = stats
            .iter()
            .take(20)
            .map(|s| {
                vec![
                    s.provider.clone(),
                    s.model.clone(),
                    s.feature.clone(),
                    s.total_calls.to_string(),
                    format!("{:.1}", s.success_rate * 100.0),
                    s.avg_latency_ms.to_string(),
                    s.avg_tokens.to_string(),
                ]
            })
            .collect();
        p::table(headers, &rows);
    }

    p::separator();
    Ok(())
}

fn handle_learn(args: LearnArgs) -> Result<()> {
    router::parse_category(&args.category)?;
    router::record_category_preference(&args.category, &args.model)?;
    p::success(&format!(
        "Learned preference: {} → {}",
        args.category, args.model
    ));
    Ok(())
}
