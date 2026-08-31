use crate::utils::ai_search as ais;
use crate::utils::ollama;
use crate::utils::print as p;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::*;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum AiSearchCommands {
    /// Search code by natural language query
    Search(SearchArgs),

    /// Find similar code blocks to a given snippet
    Similar(SimilarArgs),

    /// Discover patterns in the codebase
    Patterns(PatternsArgs),

    /// Find usage examples for a function
    Usage(UsageArgs),

    /// Run a comprehensive code discovery pipeline
    Discover(DiscoverArgs),
}

#[derive(Args)]
pub struct SearchArgs {
    /// Natural language search query
    pub query: String,

    /// Project directory (defaults to current directory)
    #[arg(long, short)]
    pub dir: Option<PathBuf>,

    /// Maximum number of results
    #[arg(long, default_value = "20")]
    pub max_results: usize,

    /// Minimum relevance score (0.0 - 1.0)
    #[arg(long, default_value = "0.3")]
    pub min_relevance: f64,

    /// Include test files in search
    #[arg(long, default_value = "true")]
    pub include_tests: bool,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Use Ollama for AI-enhanced search
    #[arg(long, default_value = "false")]
    pub use_ai: bool,

    /// Ollama model to use
    #[arg(long, default_value = "codellama:7b")]
    pub model: String,
}

#[derive(Args)]
pub struct SimilarArgs {
    /// Code snippet to find similar code for
    #[arg(long)]
    pub snippet: String,

    /// Project directory (defaults to current directory)
    #[arg(long, short)]
    pub dir: Option<PathBuf>,

    /// Maximum number of results
    #[arg(long, default_value = "10")]
    pub max_results: usize,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args)]
pub struct PatternsArgs {
    /// Project directory (defaults to current directory)
    #[arg(long, short)]
    pub dir: Option<PathBuf>,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Use Ollama for AI-enhanced pattern discovery
    #[arg(long, default_value = "false")]
    pub use_ai: bool,

    /// Ollama model to use
    #[arg(long, default_value = "codellama:7b")]
    pub model: String,
}

#[derive(Args)]
pub struct UsageArgs {
    /// Function name to find usage examples for
    pub function_name: String,

    /// Project directory (defaults to current directory)
    #[arg(long, short)]
    pub dir: Option<PathBuf>,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args)]
pub struct DiscoverArgs {
    /// Search query for code discovery
    pub query: String,

    /// Project directory (defaults to current directory)
    #[arg(long, short)]
    pub dir: Option<PathBuf>,

    /// Maximum number of results
    #[arg(long, default_value = "20")]
    pub max_results: usize,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Use Ollama for AI-enhanced discovery
    #[arg(long, default_value = "false")]
    pub use_ai: bool,

    /// Ollama model to use
    #[arg(long, default_value = "codellama:7b")]
    pub model: String,
}

pub async fn handle(cmd: AiSearchCommands) -> Result<()> {
    match cmd {
        AiSearchCommands::Search(args) => handle_search(args).await,
        AiSearchCommands::Similar(args) => handle_similar(args),
        AiSearchCommands::Patterns(args) => handle_patterns(args).await,
        AiSearchCommands::Usage(args) => handle_usage(args),
        AiSearchCommands::Discover(args) => handle_discover(args).await,
    }
}

async fn handle_search(args: SearchArgs) -> Result<()> {
    p::header("AI Code Search");

    let project_dir = args
        .dir
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let config = ais::SearchConfig {
        max_results: args.max_results,
        min_relevance: args.min_relevance,
        search_tests: args.include_tests,
        search_examples: true,
        include_context_lines: 3,
    };

    if args.use_ai {
        if ollama::is_ollama_running().await {
            let context = format!("Project directory: {}", project_dir.display());
            let prompt = ais::build_search_prompt(&args.query, &context);
            p::info(&format!("Searching with AI ({})...", args.model));
            let response = ollama::generate(
                &args.model,
                &prompt,
                Some(ollama::GenerateOptions {
                    temperature: Some(0.2),
                    num_predict: Some(4096),
                    num_ctx: Some(8192),
                }),
            )
            .await
            .context("AI search failed")?;

            match args.format.as_str() {
                "json" => println!("{}", response.response),
                _ => {
                    println!();
                    println!("{}", "AI Search Results:".bold());
                    println!("{}", response.response);
                }
            }
        } else {
            p::warn("Ollama not running. Falling back to local search.");
            let result = ais::search_code(&args.query, &project_dir, &config)?;
            print_search_result(&result, &args.format);
        }
    } else {
        let result = ais::search_code(&args.query, &project_dir, &config)?;
        print_search_result(&result, &args.format);
    }

    Ok(())
}

fn handle_similar(args: SimilarArgs) -> Result<()> {
    p::header("Find Similar Code");

    let project_dir = args
        .dir
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let results = ais::find_similar_code(&args.snippet, &project_dir, args.max_results)?;

    match args.format.as_str() {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&results)?);
        }
        _ => {
            println!();
            p::kv("Similar code blocks found", &results.len().to_string());
            println!();

            for (i, result) in results.iter().enumerate() {
                println!(
                    "  {} {} (similarity: {:.0}%)",
                    format!("{}.", i + 1).cyan(),
                    result.file_path.bright_white().bold(),
                    result.similarity_score * 100.0
                );
                println!("    Line {}-{}", result.line_start, result.line_end);
                println!("    {}", result.snippet.dimmed());
                if !result.shared_patterns.is_empty() {
                    println!("    Shared patterns: {}", result.shared_patterns.join(", "));
                }
                println!();
            }
        }
    }

    Ok(())
}

async fn handle_patterns(args: PatternsArgs) -> Result<()> {
    p::header("Pattern Discovery");

    let project_dir = args
        .dir
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    if args.use_ai {
        if ollama::is_ollama_running().await {
            let context = format!("Project directory: {}", project_dir.display());
            let prompt = ais::build_pattern_discovery_prompt(&context);
            p::info(&format!("Discovering patterns with AI ({})...", args.model));
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
            .context("AI pattern discovery failed")?;

            match args.format.as_str() {
                "json" => println!("{}", response.response),
                _ => {
                    println!();
                    println!("{}", "AI-Discovered Patterns:".bold());
                    println!("{}", response.response);
                }
            }
        } else {
            p::warn("Ollama not running. Falling back to local pattern discovery.");
            let patterns = ais::discover_patterns(&project_dir)?;
            print_patterns(&patterns, &args.format);
        }
    } else {
        let patterns = ais::discover_patterns(&project_dir)?;
        print_patterns(&patterns, &args.format);
    }

    Ok(())
}

fn handle_usage(args: UsageArgs) -> Result<()> {
    p::header("Usage Examples");

    let project_dir = args
        .dir
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let examples = ais::generate_usage_examples(&args.function_name, &project_dir)?;

    match args.format.as_str() {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&examples)?);
        }
        _ => {
            println!();
            p::kv("Function", &args.function_name);
            p::kv("Examples found", &examples.len().to_string());
            println!();

            for example in &examples {
                println!(
                    "  {} {} ({})",
                    "●".cyan(),
                    example.function_name.bright_white().bold(),
                    format!("{:?}", example.example_type).yellow()
                );
                println!("    {}", example.description);
                println!("    {}", example.code.dimmed());
                if let Some(ref source) = example.source_file {
                    println!("    Source: {}", source);
                }
                println!();
            }
        }
    }

    Ok(())
}

async fn handle_discover(args: DiscoverArgs) -> Result<()> {
    p::header("Code Discovery");

    let project_dir = args
        .dir
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let config = ais::SearchConfig {
        max_results: args.max_results,
        min_relevance: 0.3,
        search_tests: true,
        search_examples: true,
        include_context_lines: 3,
    };

    if args.use_ai && ollama::is_ollama_running().await {
        let context = format!("Project directory: {}", project_dir.display());
        let prompt = ais::build_search_prompt(&args.query, &context);
        p::info(&format!("Running AI discovery with {}...", args.model));
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
        .context("AI discovery failed")?;

        match args.format.as_str() {
            "json" => println!("{}", response.response),
            _ => {
                println!();
                println!("{}", "AI Discovery Results:".bold());
                println!("{}", response.response);
            }
        }
    } else {
        let result = ais::run_discovery(&args.query, &project_dir, &config)?;

        match args.format.as_str() {
            "json" => {
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            _ => {
                println!();
                println!("{}", result.summary.bold());
                println!();

                if !result.results.is_empty() {
                    println!("{}", "Code Matches:".bold());
                    for (i, r) in result.results.iter().take(5).enumerate() {
                        println!(
                            "  {} {} (relevance: {:.0}%)",
                            format!("{}.", i + 1).cyan(),
                            r.file_path.bright_white().bold(),
                            r.relevance_score * 100.0
                        );
                        println!("    {}", r.snippet.dimmed());
                    }
                    println!();
                }

                if !result.patterns.is_empty() {
                    println!("{}", "Discovered Patterns:".bold());
                    for pattern in &result.patterns {
                        println!(
                            "  {} {} ({} occurrences)",
                            "●".cyan(),
                            pattern.name.bright_white().bold(),
                            pattern.occurrences.len()
                        );
                    }
                    println!();
                }

                if !result.usage_examples.is_empty() {
                    println!("{}", "Usage Examples:".bold());
                    for example in &result.usage_examples {
                        println!(
                            "  {} {}",
                            "●".cyan(),
                            example.function_name.bright_white().bold()
                        );
                        println!("    {}", example.code.dimmed());
                    }
                }
            }
        }
    }

    Ok(())
}

fn print_search_result(result: &ais::DiscoveryResult, format: &str) {
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(result).unwrap());
        }
        _ => {
            println!();
            println!("{}", result.summary.bold());
            println!();

            for (i, r) in result.results.iter().enumerate() {
                let match_type_color = match r.match_type {
                    ais::MatchType::ExactMatch => "green".to_string(),
                    ais::MatchType::SemanticMatch => "cyan".to_string(),
                    ais::MatchType::PatternMatch => "yellow".to_string(),
                    ais::MatchType::StructuralMatch => "white".to_string(),
                };

                let match_type_str = format!("{:?}", r.match_type);
                let colored_match = match match_type_color.as_str() {
                    "green" => match_type_str.green(),
                    "cyan" => match_type_str.cyan(),
                    "yellow" => match_type_str.yellow(),
                    _ => match_type_str.white(),
                };

                println!(
                    "  {} {} [{}] (relevance: {:.0}%)",
                    format!("{}.", i + 1).cyan(),
                    r.file_path.bright_white().bold(),
                    colored_match,
                    r.relevance_score * 100.0
                );
                println!(
                    "    {} (lines {}-{})",
                    r.context.dimmed(),
                    r.line_start,
                    r.line_end
                );
                println!("    {}", r.snippet.dimmed());
                println!();
            }
        }
    }
}

fn print_patterns(patterns: &[ais::DiscoveredPattern], format: &str) {
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(patterns).unwrap());
        }
        _ => {
            println!();
            p::kv("Patterns discovered", &patterns.len().to_string());
            println!();

            for pattern in patterns {
                println!(
                    "  {} {} ({} occurrences)",
                    "●".cyan(),
                    pattern.name.bright_white().bold(),
                    pattern.occurrences.len()
                );
                println!("    {}", pattern.description);
                println!("    Suggestion: {}", pattern.suggestion);
                for occ in pattern.occurrences.iter().take(3) {
                    println!(
                        "      → {}:{} — {}",
                        occ.file_path,
                        occ.line,
                        occ.context.dimmed()
                    );
                }
                println!();
            }
        }
    }
}
