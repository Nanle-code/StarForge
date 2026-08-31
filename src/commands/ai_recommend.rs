use crate::utils::ai_recommendations as air;
use crate::utils::ollama;
use crate::utils::print as p;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::*;
use std::fs;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum AiRecommendCommands {
    /// Analyze a contract for best practice recommendations
    Analyze(AnalyzeArgs),

    /// Scan a project and rank all recommendations by priority
    Scan(ScanArgs),

    /// Get recommendations for a specific category
    Category(CategoryArgs),

    /// Generate an improvement plan from recommendations
    Plan(PlanArgs),
}

#[derive(Args)]
pub struct AnalyzeArgs {
    /// Path to the contract source file
    pub source: PathBuf,

    /// Focus on specific categories (comma-separated): security, gas_optimization, code_organization, testing, deployment, error_handling, storage, access_control
    #[arg(long, value_delimiter = ',')]
    pub categories: Vec<String>,

    /// Minimum severity to report: critical, high, medium, low, info
    #[arg(long, default_value = "low")]
    pub min_severity: String,

    /// Maximum recommendations to show
    #[arg(long, default_value = "50")]
    pub max_results: usize,

    /// Include code examples in output
    #[arg(long, default_value = "true")]
    pub include_examples: bool,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Write output to file
    #[arg(long, short)]
    pub out: Option<PathBuf>,

    /// Use Ollama for AI-enhanced analysis
    #[arg(long, default_value = "false")]
    pub use_ai: bool,

    /// Ollama model to use
    #[arg(long, default_value = "codellama:7b")]
    pub model: String,
}

#[derive(Args)]
pub struct ScanArgs {
    /// Path to the project directory
    pub dir: PathBuf,

    /// Minimum severity to report
    #[arg(long, default_value = "low")]
    pub min_severity: String,

    /// Maximum recommendations to show
    #[arg(long, default_value = "50")]
    pub max_results: usize,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Write output to file
    #[arg(long, short)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct CategoryArgs {
    /// Category name: security, gas_optimization, code_organization, testing, deployment, error_handling, storage, access_control
    pub category: String,

    /// Path to the contract source file
    pub source: PathBuf,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args)]
pub struct PlanArgs {
    /// Path to the contract source file
    pub source: PathBuf,

    /// Use Ollama for AI-enhanced plan
    #[arg(long, default_value = "false")]
    pub use_ai: bool,

    /// Ollama model to use
    #[arg(long, default_value = "codellama:7b")]
    pub model: String,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Write output to file
    #[arg(long, short)]
    pub out: Option<PathBuf>,
}

pub async fn handle(cmd: AiRecommendCommands) -> Result<()> {
    match cmd {
        AiRecommendCommands::Analyze(args) => handle_analyze(args).await,
        AiRecommendCommands::Scan(args) => handle_scan(args),
        AiRecommendCommands::Category(args) => handle_category(args),
        AiRecommendCommands::Plan(args) => handle_plan(args).await,
    }
}

async fn handle_analyze(args: AnalyzeArgs) -> Result<()> {
    p::header("Best Practice Analysis");

    let source_code = fs::read_to_string(&args.source)
        .with_context(|| format!("Failed to read source: {}", args.source.display()))?;

    let min_severity = parse_severity(&args.min_severity);

    let categories = if args.categories.is_empty() {
        vec![
            air::BestPracticeCategory::Security,
            air::BestPracticeCategory::GasOptimization,
            air::BestPracticeCategory::CodeOrganization,
            air::BestPracticeCategory::Testing,
            air::BestPracticeCategory::Deployment,
            air::BestPracticeCategory::ErrorHandling,
            air::BestPracticeCategory::Storage,
            air::BestPracticeCategory::AccessControl,
        ]
    } else {
        args.categories
            .iter()
            .filter_map(|c| parse_category(c))
            .collect()
    };

    if args.use_ai && ollama::is_ollama_running().await {
        let prompt = air::build_analysis_prompt(&source_code);
        p::info(&format!("Running AI analysis with {}...", args.model));
        let response = ollama::generate(
            &args.model,
            &prompt,
            Some(ollama::GenerateOptions {
                temperature: Some(0.3),
                num_predict: Some(4096),
                num_ctx: Some(8192),
            }),
        )
        .await
        .context("AI analysis failed")?;

        match args.format.as_str() {
            "json" => println!("{}", response.response),
            _ => {
                println!();
                println!("{}", "AI Best Practice Analysis:".bold());
                println!("{}", response.response);
            }
        }

        if let Some(out_path) = &args.out {
            fs::write(out_path, &response.response)?;
            p::success(&format!("Saved to {}", out_path.display()));
        }
    } else {
        let config = air::AnalysisConfig {
            categories,
            min_severity,
            max_recommendations: args.max_results,
            include_code_examples: args.include_examples,
        };

        let result = air::analyze_best_practices(&source_code, &config)?;
        print_analysis_result(&result, &args.format);

        if let Some(out_path) = &args.out {
            let json = serde_json::to_string_pretty(&result)?;
            fs::write(out_path, json)?;
            p::success(&format!("Saved to {}", out_path.display()));
        }
    }

    Ok(())
}

fn handle_scan(args: ScanArgs) -> Result<()> {
    p::header("Project Best Practice Scan");

    let source_files = find_rust_sources(&args.dir)?;

    if source_files.is_empty() {
        p::warn("No Rust source files found in the directory");
        return Ok(());
    }

    let min_severity = parse_severity(&args.min_severity);
    let mut all_recommendations = Vec::new();
    let mut all_scores = Vec::new();

    for source_file in &source_files {
        if let Ok(source_code) = fs::read_to_string(source_file) {
            let config = air::AnalysisConfig {
                categories: vec![
                    air::BestPracticeCategory::Security,
                    air::BestPracticeCategory::GasOptimization,
                    air::BestPracticeCategory::CodeOrganization,
                    air::BestPracticeCategory::Testing,
                    air::BestPracticeCategory::Deployment,
                    air::BestPracticeCategory::ErrorHandling,
                    air::BestPracticeCategory::Storage,
                    air::BestPracticeCategory::AccessControl,
                ],
                min_severity: min_severity.clone(),
                max_recommendations: 20,
                include_code_examples: false,
            };

            if let Ok(result) = air::analyze_best_practices(&source_code, &config) {
                for mut rec in result.recommendations {
                    rec.current_issue = Some(format!(
                        "{} (in {})",
                        rec.current_issue.unwrap_or_default(),
                        source_file.display()
                    ));
                    all_recommendations.push(rec);
                }
                all_scores.push(result.score);
            }
        }
    }

    all_recommendations.sort_by(|a, b| {
        b.priority_score
            .partial_cmp(&a.priority_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    all_recommendations.truncate(args.max_results);

    let avg_score = if all_scores.is_empty() {
        air::BestPracticeScore {
            overall: 0.0,
            security: 0.0,
            gas_optimization: 0.0,
            code_organization: 0.0,
            testing: 0.0,
            deployment: 0.0,
        }
    } else {
        let n = all_scores.len() as f64;
        air::BestPracticeScore {
            overall: all_scores.iter().map(|s| s.overall).sum::<f64>() / n,
            security: all_scores.iter().map(|s| s.security).sum::<f64>() / n,
            gas_optimization: all_scores.iter().map(|s| s.gas_optimization).sum::<f64>() / n,
            code_organization: all_scores.iter().map(|s| s.code_organization).sum::<f64>() / n,
            testing: all_scores.iter().map(|s| s.testing).sum::<f64>() / n,
            deployment: all_scores.iter().map(|s| s.deployment).sum::<f64>() / n,
        }
    };

    let result = air::BestPracticeResult {
        recommendations: all_recommendations,
        score: avg_score,
        summary: format!("Scan complete across {} files", source_files.len()),
        category_breakdown: std::collections::HashMap::new(),
    };

    print_analysis_result(&result, &args.format);

    if let Some(out_path) = &args.out {
        let json = serde_json::to_string_pretty(&result)?;
        fs::write(out_path, json)?;
        p::success(&format!("Saved to {}", out_path.display()));
    }

    Ok(())
}

fn handle_category(args: CategoryArgs) -> Result<()> {
    p::header(&format!("{} Recommendations", args.category));

    let source_code = fs::read_to_string(&args.source)
        .with_context(|| format!("Failed to read source: {}", args.source.display()))?;

    let category = parse_category(&args.category)
        .ok_or_else(|| anyhow::anyhow!("Unknown category: {}", args.category))?;

    let config = air::AnalysisConfig {
        categories: vec![category],
        min_severity: air::RecommendationSeverity::Low,
        max_recommendations: 50,
        include_code_examples: true,
    };

    let result = air::analyze_best_practices(&source_code, &config)?;

    match args.format.as_str() {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        _ => {
            println!();
            p::kv("Recommendations", &result.recommendations.len().to_string());
            println!();

            for rec in &result.recommendations {
                let severity_color = match rec.severity {
                    air::RecommendationSeverity::Critical => "red",
                    air::RecommendationSeverity::High => "yellow",
                    air::RecommendationSeverity::Medium => "cyan",
                    _ => "white",
                };

                println!(
                    "  {} {} [{}]",
                    "●".cyan(),
                    rec.title.bright_white().bold(),
                    rec.severity.to_string().color(severity_color).bold()
                );
                println!("    {}", rec.description);
                if let Some(ref issue) = rec.current_issue {
                    println!("    Issue: {}", issue);
                }
                println!("    Fix: {}", rec.suggested_fix);
                println!("    Effort: {}", rec.estimated_effort);
                if let Some(ref example) = rec.code_example {
                    println!("    Example:");
                    for line in example.lines() {
                        println!("      {}", line.dimmed());
                    }
                }
                println!();
            }
        }
    }

    Ok(())
}

async fn handle_plan(args: PlanArgs) -> Result<()> {
    p::header("Improvement Plan");

    let source_code = fs::read_to_string(&args.source)
        .with_context(|| format!("Failed to read source: {}", args.source.display()))?;

    let config = air::AnalysisConfig::default();
    let result = air::analyze_best_practices(&source_code, &config)?;

    if args.use_ai && ollama::is_ollama_running().await {
        let prompt = air::build_analysis_prompt(&source_code);
        p::info(&format!(
            "Generating AI improvement plan with {}...",
            args.model
        ));
        let response = ollama::generate(
            &args.model,
            &prompt,
            Some(ollama::GenerateOptions {
                temperature: Some(0.3),
                num_predict: Some(4096),
                num_ctx: Some(8192),
            }),
        )
        .await
        .context("AI plan generation failed")?;

        match args.format.as_str() {
            "json" => println!("{}", response.response),
            _ => {
                println!();
                println!("{}", response.response);
            }
        }
    } else {
        print_improvement_plan(&result);
    }

    if let Some(out_path) = &args.out {
        let json = serde_json::to_string_pretty(&result)?;
        fs::write(out_path, json)?;
        p::success(&format!("Saved to {}", out_path.display()));
    }

    Ok(())
}

fn print_analysis_result(result: &air::BestPracticeResult, format: &str) {
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(result).unwrap());
        }
        _ => {
            println!();
            println!("{}", result.summary.bold());
            println!();
            println!("{}", "Best Practice Scores:".bold());
            p::kv("Overall", &format!("{:.0}/100", result.score.overall));
            p::kv("Security", &format!("{:.0}/100", result.score.security));
            p::kv(
                "Gas Optimization",
                &format!("{:.0}/100", result.score.gas_optimization),
            );
            p::kv(
                "Code Organization",
                &format!("{:.0}/100", result.score.code_organization),
            );
            p::kv("Testing", &format!("{:.0}/100", result.score.testing));
            p::kv("Deployment", &format!("{:.0}/100", result.score.deployment));
            println!();
            p::separator();
            println!(
                "{} ({} total)",
                "Recommendations:".bold(),
                result.recommendations.len()
            );
            println!();

            for rec in &result.recommendations {
                let severity_color = match rec.severity {
                    air::RecommendationSeverity::Critical => "red",
                    air::RecommendationSeverity::High => "yellow",
                    air::RecommendationSeverity::Medium => "cyan",
                    _ => "white",
                };

                println!(
                    "  {} [{}] {} — {}",
                    rec.id.dimmed(),
                    rec.severity.to_string().color(severity_color).bold(),
                    rec.title.bright_white().bold(),
                    rec.category.to_string().dimmed()
                );
                println!("    {}", rec.description);
                println!("    Fix: {}", rec.suggested_fix);
                println!("    Effort: {}", rec.estimated_effort);
                println!();
            }

            if !result.category_breakdown.is_empty() {
                p::separator();
                println!("{}", "By Category:".bold());
                for (cat, count) in &result.category_breakdown {
                    println!("  {} — {}", cat, count);
                }
            }
        }
    }
}

fn print_improvement_plan(result: &air::BestPracticeResult) {
    println!();
    println!("{}", "Improvement Plan:".bold());
    println!();

    let critical: Vec<_> = result
        .recommendations
        .iter()
        .filter(|r| r.severity == air::RecommendationSeverity::Critical)
        .collect();
    let high: Vec<_> = result
        .recommendations
        .iter()
        .filter(|r| r.severity == air::RecommendationSeverity::High)
        .collect();
    let medium: Vec<_> = result
        .recommendations
        .iter()
        .filter(|r| r.severity == air::RecommendationSeverity::Medium)
        .collect();

    if !critical.is_empty() {
        println!(
            "{} ({} items)",
            "IMMEDIATE — Critical Issues:".red().bold(),
            critical.len()
        );
        for rec in &critical {
            println!("  → {}: {}", rec.title, rec.suggested_fix);
        }
        println!();
    }

    if !high.is_empty() {
        println!(
            "{} ({} items)",
            "SHORT-TERM — High Priority:".yellow().bold(),
            high.len()
        );
        for rec in &high {
            println!("  → {}: {}", rec.title, rec.suggested_fix);
        }
        println!();
    }

    if !medium.is_empty() {
        println!(
            "{} ({} items)",
            "MEDIUM-TERM — Medium Priority:".cyan().bold(),
            medium.len()
        );
        for rec in &medium {
            println!("  → {}: {}", rec.title, rec.suggested_fix);
        }
        println!();
    }

    p::kv("Overall score", &format!("{:.0}/100", result.score.overall));
}

fn parse_category(s: &str) -> Option<air::BestPracticeCategory> {
    match s.to_lowercase().as_str() {
        "security" => Some(air::BestPracticeCategory::Security),
        "gas_optimization" | "gas" => Some(air::BestPracticeCategory::GasOptimization),
        "code_organization" | "organization" => Some(air::BestPracticeCategory::CodeOrganization),
        "testing" | "tests" => Some(air::BestPracticeCategory::Testing),
        "deployment" | "deploy" => Some(air::BestPracticeCategory::Deployment),
        "error_handling" | "errors" => Some(air::BestPracticeCategory::ErrorHandling),
        "storage" => Some(air::BestPracticeCategory::Storage),
        "access_control" | "auth" => Some(air::BestPracticeCategory::AccessControl),
        _ => None,
    }
}

fn parse_severity(s: &str) -> air::RecommendationSeverity {
    match s.to_lowercase().as_str() {
        "critical" => air::RecommendationSeverity::Critical,
        "high" => air::RecommendationSeverity::High,
        "medium" => air::RecommendationSeverity::Medium,
        "low" => air::RecommendationSeverity::Low,
        "info" => air::RecommendationSeverity::Info,
        _ => air::RecommendationSeverity::Low,
    }
}

fn find_rust_sources(dir: &PathBuf) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if !name.starts_with('.')
                    && name != "target"
                    && name != "node_modules"
                    && name != "wasm"
                {
                    files.extend(find_rust_sources(&path)?);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    Ok(files)
}
