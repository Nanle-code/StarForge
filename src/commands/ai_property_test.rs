use crate::utils::ai_property_testing as apt;
use crate::utils::ollama;
use crate::utils::print as p;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::*;
use std::fs;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum AiPropertyTestCommands {
    /// Discover properties and invariants in a Soroban contract
    Discover(DiscoverArgs),

    /// Generate property-based test cases
    Generate(GenerateArgs),

    /// Validate that contract invariants hold
    Validate(ValidateArgs),

    /// Show discovered edge cases for a contract
    EdgeCases(EdgeCasesArgs),

    /// Generate shrinkage strategies for counterexample minimization
    Shrink(ShrinkArgs),
}

#[derive(Args)]
pub struct DiscoverArgs {
    /// Path to the contract source file
    pub source: PathBuf,

    /// Contract name (defaults to file stem)
    #[arg(long)]
    pub name: Option<String>,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Write output to file
    #[arg(long, short)]
    pub out: Option<PathBuf>,

    /// Use Ollama for AI-enhanced discovery
    #[arg(long, default_value = "false")]
    pub use_ai: bool,

    /// Ollama model to use
    #[arg(long, default_value = "codellama:7b")]
    pub model: String,
}

#[derive(Args)]
pub struct GenerateArgs {
    /// Path to the contract source file
    pub source: PathBuf,

    /// Focus on specific functions (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub functions: Vec<String>,

    /// Maximum tests per property
    #[arg(long, default_value = "5")]
    pub max_tests: usize,

    /// Include shrinkage strategies
    #[arg(long, default_value = "true")]
    pub shrink: bool,

    /// Output file for generated tests
    #[arg(long, short)]
    pub out: Option<PathBuf>,

    /// Output format: text, json, code
    #[arg(long, default_value = "code")]
    pub format: String,

    /// Use Ollama for AI generation
    #[arg(long, default_value = "false")]
    pub use_ai: bool,

    /// Ollama model to use
    #[arg(long, default_value = "codellama:7b")]
    pub model: String,
}

#[derive(Args)]
pub struct ValidateArgs {
    /// Path to the contract source file
    pub source: PathBuf,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Write output to file
    #[arg(long, short)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct EdgeCasesArgs {
    /// Path to the contract source file
    pub source: PathBuf,

    /// Focus on specific functions (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub functions: Vec<String>,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,
}

#[derive(Args)]
pub struct ShrinkArgs {
    /// The failing test code to generate shrink strategy for
    #[arg(long)]
    pub test_code: String,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Use Ollama for AI-enhanced shrinkage
    #[arg(long, default_value = "false")]
    pub use_ai: bool,

    /// Ollama model to use
    #[arg(long, default_value = "codellama:7b")]
    pub model: String,
}

pub async fn handle(cmd: AiPropertyTestCommands) -> Result<()> {
    match cmd {
        AiPropertyTestCommands::Discover(args) => handle_discover(args).await,
        AiPropertyTestCommands::Generate(args) => handle_generate(args).await,
        AiPropertyTestCommands::Validate(args) => handle_validate(args),
        AiPropertyTestCommands::EdgeCases(args) => handle_edge_cases(args),
        AiPropertyTestCommands::Shrink(args) => handle_shrink(args).await,
    }
}

async fn handle_discover(args: DiscoverArgs) -> Result<()> {
    p::header("AI Property Discovery");

    let source_code = fs::read_to_string(&args.source)
        .with_context(|| format!("Failed to read source: {}", args.source.display()))?;

    if args.use_ai {
        if ollama::is_ollama_running().await {
            let prompt = apt::build_property_discovery_prompt(&source_code);
            p::info(&format!(
                "Sending to {} for AI-enhanced discovery...",
                args.model
            ));
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
            .context("AI discovery failed")?;

            match args.format.as_str() {
                "json" => println!("{}", response.response),
                _ => {
                    println!();
                    println!("{}", "AI-Discovered Properties:".bold());
                    println!("{}", response.response);
                }
            }
        } else {
            p::warn("Ollama not running. Falling back to local discovery.");
            let properties = apt::discover_properties(&source_code)?;
            print_properties(&properties, &args.format);
        }
    } else {
        let properties = apt::discover_properties(&source_code)?;
        print_properties(&properties, &args.format);
    }

    if let Some(out_path) = &args.out {
        let properties = apt::discover_properties(&source_code)?;
        let json = serde_json::to_string_pretty(&properties)?;
        fs::write(out_path, json)?;
        p::success(&format!("Saved to {}", out_path.display()));
    }

    Ok(())
}

async fn handle_generate(args: GenerateArgs) -> Result<()> {
    p::header("Property-Based Test Generation");

    let source_code = fs::read_to_string(&args.source)
        .with_context(|| format!("Failed to read source: {}", args.source.display()))?;

    let config = apt::PropertyTestConfig {
        max_tests_per_property: args.max_tests,
        include_shrinkage: args.shrink,
        target_functions: args.functions,
        timeout_ms: 10_000,
    };

    if args.use_ai {
        if ollama::is_ollama_running().await {
            let prompt = apt::build_property_discovery_prompt(&source_code);
            p::info(&format!("Generating property tests with {}...", args.model));
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
            .context("AI generation failed")?;

            match args.format.as_str() {
                "json" => println!("{}", response.response),
                "text" => {
                    println!();
                    println!("{}", "AI-Generated Property Tests:".bold());
                    println!("{}", response.response);
                }
                _ => {
                    println!("{}", response.response);
                }
            }
        } else {
            p::warn("Ollama not running. Falling back to local generation.");
            let result = apt::run_pipeline(&source_code, &config)?;
            print_generation_result(&result, &args.format);
        }
    } else {
        let result = apt::run_pipeline(&source_code, &config)?;
        print_generation_result(&result, &args.format);
    }

    if let Some(out_path) = &args.out {
        let result = apt::run_pipeline(&source_code, &config)?;
        let json = serde_json::to_string_pretty(&result)?;
        fs::write(out_path, json)?;
        p::success(&format!("Saved to {}", out_path.display()));
    }

    Ok(())
}

fn handle_validate(args: ValidateArgs) -> Result<()> {
    p::header("Invariant Validation");

    let source_code = fs::read_to_string(&args.source)
        .with_context(|| format!("Failed to read source: {}", args.source.display()))?;

    let invariants = apt::extract_invariants(&source_code)?;
    let test_code = apt::generate_invariant_tests(&invariants);

    match args.format.as_str() {
        "json" => {
            let output = serde_json::json!({
                "invariants": invariants,
                "test_code": test_code,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        _ => {
            println!();
            p::kv("Invariants found", &invariants.len().to_string());
            println!();
            for inv in &invariants {
                println!(
                    "  {} {} — {}",
                    "●".cyan(),
                    inv.name.bright_white().bold(),
                    inv.description
                );
                println!("    Expression: {}", inv.expression.dimmed());
                println!("    Type: {:?}", inv.check_type);
                println!(
                    "    Affected functions: {}",
                    inv.functions_affected.join(", ")
                );
                println!();
            }
            p::separator();
            println!("{}", "Generated Invariant Tests:".bold());
            println!("{}", test_code);
        }
    }

    if let Some(out_path) = &args.out {
        fs::write(out_path, &test_code)?;
        p::success(&format!("Saved to {}", out_path.display()));
    }

    Ok(())
}

fn handle_edge_cases(args: EdgeCasesArgs) -> Result<()> {
    p::header("Edge Case Discovery");

    let source_code = fs::read_to_string(&args.source)
        .with_context(|| format!("Failed to read source: {}", args.source.display()))?;

    let properties = apt::discover_properties(&source_code)?;

    let filtered: Vec<&apt::DiscoveredProperty> = if args.functions.is_empty() {
        properties.iter().collect()
    } else {
        properties
            .iter()
            .filter(|p| {
                p.target_function
                    .as_ref()
                    .is_some_and(|f| args.functions.contains(f))
            })
            .collect()
    };

    match args.format.as_str() {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&filtered)?);
        }
        _ => {
            println!();
            p::kv("Edge cases found", &filtered.len().to_string());
            println!();
            for prop in &filtered {
                println!(
                    "  {} {} ({})",
                    "●".cyan(),
                    prop.name.bright_white().bold(),
                    prop.property_type.to_string().yellow()
                );
                println!("    {}", prop.description);
                for invariant in &prop.invariants {
                    println!("      → {}", invariant);
                }
                println!("    Confidence: {:.0}%", prop.confidence * 100.0);
                println!();
            }
        }
    }

    Ok(())
}

async fn handle_shrink(args: ShrinkArgs) -> Result<()> {
    p::header("Shrinkage Strategy");

    if args.use_ai && ollama::is_ollama_running().await {
        let prompt = apt::build_shrink_prompt(&args.test_code);
        p::info(&format!(
            "Generating shrink strategy with {}...",
            args.model
        ));
        let response = ollama::generate(
            &args.model,
            &prompt,
            Some(ollama::GenerateOptions {
                temperature: Some(0.2),
                num_predict: Some(2048),
                num_ctx: Some(4096),
            }),
        )
        .await
        .context("AI shrink generation failed")?;

        match args.format.as_str() {
            "json" => println!("{}", response.response),
            _ => {
                println!();
                println!("{}", "AI-Generated Shrink Strategy:".bold());
                println!("{}", response.response);
            }
        }
    } else {
        let properties = apt::discover_properties(&args.test_code).unwrap_or_default();
        let default_prop = apt::DiscoveredProperty {
            name: "custom_test".to_string(),
            description: args.test_code.clone(),
            property_type: apt::PropertyType::Postcondition,
            target_function: None,
            invariants: vec!["test passes".to_string()],
            confidence: 0.5,
        };
        let prop = properties.first().unwrap_or(&default_prop);
        let shrink = apt::generate_test_cases(
            std::slice::from_ref(prop),
            &[],
            &apt::PropertyTestConfig::default(),
        )
        .first()
        .and_then(|tc| tc.shrink_strategy.clone())
        .unwrap_or_else(|| "shrink numerics toward 0, strings toward empty".to_string());

        match args.format.as_str() {
            "json" => {
                println!(
                    "{}",
                    serde_json::json!({
                        "shrink_strategy": shrink,
                        "converges": true,
                    })
                );
            }
            _ => {
                println!();
                println!("Shrink strategy: {}", shrink);
            }
        }
    }

    Ok(())
}

fn print_properties(properties: &[apt::DiscoveredProperty], format: &str) {
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(properties).unwrap());
        }
        _ => {
            println!();
            p::kv("Properties discovered", &properties.len().to_string());
            println!();
            for prop in properties {
                let severity_color = match prop.property_type {
                    apt::PropertyType::AccessControl => "red".to_string(),
                    apt::PropertyType::Overflow => "yellow".to_string(),
                    _ => "cyan".to_string(),
                };
                let prop_type_str = prop.property_type.to_string();
                let colored_type = match severity_color.as_str() {
                    "red" => prop_type_str.red(),
                    "yellow" => prop_type_str.yellow(),
                    _ => prop_type_str.cyan(),
                };
                println!(
                    "  {} {} [{}]",
                    "●".cyan(),
                    prop.name.bright_white().bold(),
                    colored_type
                );
                println!("    {}", prop.description);
                for inv in &prop.invariants {
                    println!("      → {}", inv);
                }
                println!("    Confidence: {:.0}%", prop.confidence * 100.0);
                println!();
            }
        }
    }
}

fn print_generation_result(result: &apt::PropertyTestResult, format: &str) {
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(result).unwrap());
        }
        "code" => {
            println!(
                "// Generated by StarForge AI Property-Based Testing\n// Review and customize before committing\n\n"
            );
            println!("#[cfg(test)]");
            println!("mod property_tests {{");
            println!("    use proptest::prelude::*;");
            println!("    use soroban_sdk::tests::Env;\n");

            for test in &result.test_cases {
                println!("{}", test.test_code);
                println!();
            }
            println!("}}");
        }
        _ => {
            println!();
            println!("{}", result.summary.bold());
            println!();
            p::kv("Properties", &result.properties.len().to_string());
            p::kv("Strategies", &result.strategies.len().to_string());
            p::kv("Test cases", &result.total_tests.to_string());
            p::kv("Invariants", &result.invariants.len().to_string());
            println!();
            p::separator();
            println!("{}", "Generated Test Cases:".bold());
            println!();
            for test in &result.test_cases {
                println!("  {} {}", "●".cyan(), test.name.bright_white().bold());
                println!("    Property: {}", test.property);
                println!("    Outcome: {:?}", test.expected_outcome);
                if let Some(ref shrink) = test.shrink_strategy {
                    println!("    Shrink: {}", shrink);
                }
                println!();
            }
        }
    }
}
