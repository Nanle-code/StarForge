use crate::utils::ai_test_assistant as ata;
use crate::utils::ollama;
use crate::utils::print as p;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::*;
use std::fs;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum AiTestCommands {
    /// Generate comprehensive tests for a Soroban contract using AI
    Generate(GenerateArgs),

    /// Analyze contract and suggest test priorities
    Analyze(AnalyzeArgs),

    /// Optimize existing test suite using AI
    Optimize(OptimizeArgs),

    /// Analyze coverage gaps and suggest improvements
    Coverage(CoverageArgs),

    /// Detect outdated, broken, or missing tests
    Maintain(MaintainArgs),

    /// Generate mock objects for contract testing
    Mocks(MocksArgs),

    /// Generate test data generators and edge cases
    TestData(TestDataArgs),

    /// Calculate test quality score for existing tests
    Quality(QualityArgs),
}

#[derive(Args)]
pub struct GenerateArgs {
    /// Path to the contract source file or project directory
    pub path: PathBuf,

    /// Test type to generate: unit, integration, edge_case, security, all
    #[arg(long, default_value = "all")]
    pub test_type: String,

    /// Contract name (defaults to file stem)
    #[arg(long)]
    pub name: Option<String>,

    /// Path to existing test file to avoid duplication
    #[arg(long)]
    pub existing_tests: Option<PathBuf>,

    /// Path to coverage report (JSON) for targeted generation
    #[arg(long)]
    pub coverage_report: Option<PathBuf>,

    /// Focus on specific functions (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub functions: Vec<String>,

    /// Output file for generated tests
    #[arg(long, short)]
    pub out: Option<PathBuf>,

    /// Use Ollama for AI generation (requires local Ollama instance)
    #[arg(long, default_value = "false")]
    pub use_ai: bool,

    /// Ollama model to use for generation
    #[arg(long, default_value = "codellama:7b")]
    pub model: String,

    /// Output format: text, json, code
    #[arg(long, default_value = "code")]
    pub format: String,
}

#[derive(Args)]
pub struct AnalyzeArgs {
    /// Path to the contract source file or project directory
    pub path: PathBuf,

    /// Contract name (defaults to file stem)
    #[arg(long)]
    pub name: Option<String>,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Write output to file
    #[arg(long, short)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct OptimizeArgs {
    /// Path to the contract source file
    pub source: PathBuf,

    /// Path to the test file to optimize
    pub tests: PathBuf,

    /// Optimization goals (comma-separated): duplication, performance, coverage, assertions, setup, all
    #[arg(long, value_delimiter = ',', default_value = "all")]
    pub goals: Vec<String>,

    /// Use Ollama for AI optimization
    #[arg(long, default_value = "false")]
    pub use_ai: bool,

    /// Ollama model to use
    #[arg(long, default_value = "codellama:7b")]
    pub model: String,

    /// Output file for optimized tests
    #[arg(long, short)]
    pub out: Option<PathBuf>,

    /// Output format: text, json, code
    #[arg(long, default_value = "code")]
    pub format: String,
}

#[derive(Args)]
pub struct CoverageArgs {
    /// Path to the contract source file
    pub source: PathBuf,

    /// Path to the test file
    pub tests: PathBuf,

    /// Path to coverage report (JSON format)
    #[arg(long)]
    pub coverage_report: Option<PathBuf>,

    /// Target coverage percentage
    #[arg(long, default_value = "80.0")]
    pub target: f64,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Write output to file
    #[arg(long, short)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct MaintainArgs {
    /// Path to the contract source file or project directory
    pub path: PathBuf,

    /// Path to the test file or directory
    #[arg(long)]
    pub tests: Option<PathBuf>,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,

    /// Write output to file
    #[arg(long, short)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct MocksArgs {
    /// Path to the contract source file
    pub source: PathBuf,

    /// Mock types to generate (comma-separated): address, storage, contract, env, events, all
    #[arg(long, value_delimiter = ',', default_value = "all")]
    pub types: Vec<String>,

    /// Output file for generated mocks
    #[arg(long, short)]
    pub out: Option<PathBuf>,

    /// Use Ollama for AI generation
    #[arg(long, default_value = "false")]
    pub use_ai: bool,

    /// Ollama model to use
    #[arg(long, default_value = "codellama:7b")]
    pub model: String,

    /// Output format: text, json, code
    #[arg(long, default_value = "code")]
    pub format: String,
}

#[derive(Args)]
pub struct TestDataArgs {
    /// Path to the contract source file
    pub source: PathBuf,

    /// Data types to generate (comma-separated): address, amount, string, bytes, timestamp, boolean, all
    #[arg(long, value_delimiter = ',', default_value = "all")]
    pub types: Vec<String>,

    /// Number of test data items per type
    #[arg(long, default_value = "5")]
    pub count: u32,

    /// Output file for generated test data
    #[arg(long, short)]
    pub out: Option<PathBuf>,

    /// Use Ollama for AI generation
    #[arg(long, default_value = "false")]
    pub use_ai: bool,

    /// Ollama model to use
    #[arg(long, default_value = "codellama:7b")]
    pub model: String,

    /// Output format: text, json, code
    #[arg(long, default_value = "code")]
    pub format: String,
}

#[derive(Args)]
pub struct QualityArgs {
    /// Path to the contract source file
    pub source: PathBuf,

    /// Path to the test file
    pub tests: PathBuf,

    /// Output format: text, json
    #[arg(long, default_value = "text")]
    pub format: String,
}

pub async fn handle(cmd: AiTestCommands) -> Result<()> {
    match cmd {
        AiTestCommands::Generate(args) => handle_generate(args).await,
        AiTestCommands::Analyze(args) => handle_analyze(args),
        AiTestCommands::Optimize(args) => handle_optimize(args).await,
        AiTestCommands::Coverage(args) => handle_coverage(args),
        AiTestCommands::Maintain(args) => handle_maintain(args),
        AiTestCommands::Mocks(args) => handle_mocks(args).await,
        AiTestCommands::TestData(args) => handle_test_data(args).await,
        AiTestCommands::Quality(args) => handle_quality(args),
    }
}

async fn handle_generate(args: GenerateArgs) -> Result<()> {
    p::header("AI Test Generation");

    let source_code = ata::read_source_file(&args.path)?;
    let contract_name = args.name.clone().unwrap_or_else(|| {
        args.path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Contract".to_string())
    });

    let test_type = parse_test_type(&args.test_type)?;

    let existing_tests =
        if let Some(test_path) = &args.existing_tests {
            Some(fs::read_to_string(test_path).with_context(|| {
                format!("Failed to read existing tests: {}", test_path.display())
            })?)
        } else {
            None
        };

    let coverage_data = if let Some(coverage_path) = &args.coverage_report {
        let coverage_json = fs::read_to_string(coverage_path).with_context(|| {
            format!(
                "Failed to read coverage report: {}",
                coverage_path.display()
            )
        })?;
        let coverage: ata::CoverageInput =
            serde_json::from_str(&coverage_json).context("Failed to parse coverage report JSON")?;
        Some(coverage)
    } else {
        None
    };

    let analysis = ata::analyze_contract_for_testing(&source_code)?;

    p::kv("Contract", &contract_name);
    p::kv("Functions", &analysis.total_functions.to_string());
    p::kv("Public functions", &analysis.public_functions.to_string());
    p::kv("Entry points", &analysis.entry_points.to_string());
    p::kv("Test type", &args.test_type);
    println!();

    let request = ata::TestGenerationRequest {
        source_path: args.path.clone(),
        test_type: test_type.clone(),
        contract_name: contract_name.clone(),
        contract_code: source_code.clone(),
        existing_tests,
        coverage_data,
        focus_functions: args.functions.clone(),
    };

    if args.use_ai {
        let ai_response = generate_with_ai(&request, &args.model).await?;
        handle_generate_output(&ai_response, &args, &contract_name)?;
    } else {
        let response = generate_locally(&request, &analysis)?;
        handle_generate_output(&response, &args, &contract_name)?;
    }

    Ok(())
}

async fn generate_with_ai(
    request: &ata::TestGenerationRequest,
    model: &str,
) -> Result<ata::TestGenerationResponse> {
    if !ollama::is_ollama_running().await {
        p::warn("Ollama is not running. Falling back to local generation.");
        p::info(&ollama::cloud_fallback_message());
        let analysis = ata::analyze_contract_for_testing(&request.contract_code)?;
        return generate_locally(request, &analysis);
    }

    let prompt = ata::build_generation_prompt(request);
    p::info(&format!("Sending to {} for AI generation...", model));

    let response = ollama::generate(
        model,
        &prompt,
        Some(ollama::GenerateOptions {
            temperature: Some(0.2),
            num_predict: Some(4096),
            num_ctx: Some(8192),
        }),
    )
    .await
    .context("AI generation failed")?;

    let tests = vec![ata::GeneratedTest {
        name: format!("ai_generated_{}", request.contract_name.to_lowercase()),
        test_type: request.test_type.clone(),
        function_under_test: "all".to_string(),
        description: "AI-generated test suite".to_string(),
        code: response.response,
        priority: ata::TestPriority::High,
        edge_cases_covered: vec!["AI-determined edge cases".to_string()],
        security_checks: vec!["AI-determined security checks".to_string()],
    }];

    Ok(ata::TestGenerationResponse {
        tests,
        summary: format!("AI-generated tests using model {}", model),
        estimated_coverage_improvement: 25.0,
        warnings: vec![],
    })
}

fn generate_locally(
    request: &ata::TestGenerationRequest,
    analysis: &ata::ContractAnalysis,
) -> Result<ata::TestGenerationResponse> {
    let mut tests = Vec::new();
    let priorities = ata::generate_test_priorities(analysis);

    for priority_suggestion in &priorities {
        if !request.focus_functions.is_empty()
            && !request
                .focus_functions
                .contains(&priority_suggestion.function_name)
        {
            continue;
        }

        let func = analysis
            .functions
            .iter()
            .find(|f| f.name == priority_suggestion.function_name)
            .unwrap();

        for test_type_str in &priority_suggestion.test_types {
            let test_type = match test_type_str.as_str() {
                "unit" => ata::TestType::Unit,
                "integration" => ata::TestType::Integration,
                "edge_case" => ata::TestType::EdgeCase,
                "security" => ata::TestType::Security,
                _ => continue,
            };

            if request.test_type != ata::TestType::All && request.test_type != test_type {
                continue;
            }

            let code = generate_test_code(func, &test_type, &request.contract_name);
            let test_name = format!("test_{}_{}", func.name, test_type_str.replace('_', ""));

            tests.push(ata::GeneratedTest {
                name: test_name,
                test_type,
                function_under_test: func.name.clone(),
                description: format!("{} test for {}", test_type_str.replace('_', " "), func.name),
                code,
                priority: priority_suggestion.priority.clone(),
                edge_cases_covered: ata::generate_edge_case_descriptions(func),
                security_checks: ata::generate_security_checks(func),
            });
        }
    }

    let estimated_improvement = calculate_estimated_improvement(&tests, analysis);

    Ok(ata::TestGenerationResponse {
        tests: tests.clone(),
        summary: format!(
            "Generated {} test cases covering {} functions",
            tests.len(),
            analysis.public_functions
        ),
        estimated_coverage_improvement: estimated_improvement,
        warnings: ata::generate_warnings(analysis),
    })
}

fn generate_test_code(
    func: &ata::FunctionInfo,
    test_type: &ata::TestType,
    contract_name: &str,
) -> String {
    let test_suffix = match test_type {
        ata::TestType::Unit => "unit",
        ata::TestType::Integration => "integration",
        ata::TestType::EdgeCase => "edge_case",
        ata::TestType::Security => "security",
        ata::TestType::All => "all",
    };

    let setup = generate_setup_code(func, contract_name);
    let assertions = generate_assertions(func, test_type);

    format!(
        "/// {} test for `{}`
#[test]
fn test_{}_{}() {{
    let env = Env::default();
    {}
    {}
}}",
        test_suffix, func.name, func.name, test_suffix, setup, assertions
    )
}

fn generate_setup_code(func: &ata::FunctionInfo, contract_name: &str) -> String {
    let mut lines = Vec::new();

    lines.push(format!("let contract_address = Address::random(&env);"));

    for param in &func.params {
        match param.param_type.as_str() {
            t if t.contains("Address") => {
                lines.push(format!("let {} = Address::random(&env);", param.name));
            }
            t if t.contains("u64")
                || t.contains("i64")
                || t.contains("u32")
                || t.contains("i32") =>
            {
                lines.push(format!("let {}: {} = 100;", param.name, param.param_type));
            }
            t if t.contains("String") => {
                lines.push(format!(
                    "let {}: soroban_sdk::String = \"test\".into();",
                    param.name
                ));
            }
            _ => {
                lines.push(format!(
                    "// TODO: set up {} ({})",
                    param.name, param.param_type
                ));
            }
        }
    }

    lines.join("\n    ")
}

fn generate_assertions(func: &ata::FunctionInfo, test_type: &ata::TestType) -> String {
    match test_type {
        ata::TestType::Unit => {
            if func.return_type.is_some() {
                format!("let result = contract.{}(&{});\n    // Assert expected behavior\n    assert!(result.is_ok() || result.is_some());",
                    func.name,
                    func.params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", &"))
            } else {
                format!(
                    "contract.{}(&{});\n    // Assert state changes or event emission",
                    func.name,
                    func.params
                        .iter()
                        .map(|p| p.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", &")
                )
            }
        }
        ata::TestType::EdgeCase => {
            let mut assertions = Vec::new();
            assertions.push("// Test with zero/empty values".to_string());
            assertions.push("let zero_env = Env::default();".to_string());
            assertions.push(format!("// Test {} with boundary values", func.name));
            assertions
                .push("assert!(true); // Replace with specific boundary assertions".to_string());
            assertions.join("\n    ")
        }
        ata::TestType::Security => {
            let mut assertions = Vec::new();
            if func.is_mutating {
                assertions.push("// Test unauthorized access".to_string());
                assertions.push("let unauthorized = Address::random(&env);".to_string());
                assertions.push(format!(
                    "// {} should require_auth - verify unauthorized calls fail",
                    func.name
                ));
            }
            assertions.push("// Test replay protection".to_string());
            assertions.push("// Test state isolation".to_string());
            assertions.join("\n    ")
        }
        ata::TestType::Integration => {
            format!("// Test full workflow with {} \n    // Verify state transitions\n    // Check event emission",
                func.name)
        }
        ata::TestType::All => {
            format!("// Comprehensive test for {} \n    // Happy path\n    // Edge cases\n    // Error conditions\n    // Security checks",
                func.name)
        }
    }
}

fn calculate_estimated_improvement(
    tests: &[ata::GeneratedTest],
    analysis: &ata::ContractAnalysis,
) -> f64 {
    let test_count = tests.len() as f64;
    let func_count = analysis.total_functions as f64;
    if func_count == 0.0 {
        return 0.0;
    }
    let base_improvement = (test_count / func_count) * 15.0;
    base_improvement.min(50.0)
}

fn handle_generate_output(
    response: &ata::TestGenerationResponse,
    args: &GenerateArgs,
    contract_name: &str,
) -> Result<()> {
    match args.format.as_str() {
        "json" => {
            let json = serde_json::to_string_pretty(response)?;
            if let Some(out_path) = &args.out {
                fs::write(out_path, &json)?;
                p::success(&format!("JSON output saved to {}", out_path.display()));
            } else {
                println!("{}", json);
            }
        }
        "text" => {
            println!();
            p::separator();
            println!("{}", response.summary.bold());
            println!();

            for test in &response.tests {
                let priority_color = match test.priority {
                    ata::TestPriority::Critical => "CRITICAL".red().bold(),
                    ata::TestPriority::High => "HIGH".yellow().bold(),
                    ata::TestPriority::Medium => "MEDIUM".cyan(),
                    ata::TestPriority::Low => "LOW".white(),
                };

                println!(
                    "  [{}] {} — {}",
                    priority_color,
                    test.name.bright_white().bold(),
                    test.description
                );
                println!("    Function: {}", test.function_under_test);
                println!("    Edge cases: {}", test.edge_cases_covered.join(", "));
                if !test.security_checks.is_empty() {
                    println!("    Security: {}", test.security_checks.join(", "));
                }
                println!();
            }

            p::kv(
                "Estimated coverage improvement",
                &format!("{:.1}%", response.estimated_coverage_improvement),
            );

            if !response.warnings.is_empty() {
                println!();
                p::warn("Warnings:");
                for warning in &response.warnings {
                    println!("  • {}", warning);
                }
            }
        }
        _ => {
            // code format
            let mut code = String::from(
                "// Generated by StarForge AI Test Assistant\n// Review and customize before committing\n\n",
            );
            code.push_str("#[cfg(test)]\nmod tests {\n    use super::*;\n    use soroban_sdk::tests::Env;\n\n");

            for test in &response.tests {
                code.push_str(&test.code);
                code.push_str("\n\n");
            }

            code.push_str("}\n");

            if let Some(out_path) = &args.out {
                fs::write(out_path, &code)?;
                p::success(&format!("Tests saved to {}", out_path.display()));
            } else {
                println!("{}", code);
            }
        }
    }

    p::success(&format!("Generated {} test cases", response.tests.len()));

    Ok(())
}

fn handle_analyze(args: AnalyzeArgs) -> Result<()> {
    p::header("Contract Analysis for Testing");

    let source_code = ata::read_source_file(&args.path)?;
    let contract_name = args.name.clone().unwrap_or_else(|| {
        args.path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Contract".to_string())
    });

    let analysis = ata::analyze_contract_for_testing(&source_code)?;
    let priorities = ata::generate_test_priorities(&analysis);

    match args.format.as_str() {
        "json" => {
            let output = serde_json::json!({
                "contract_name": contract_name,
                "analysis": analysis,
                "priorities": priorities,
            });
            let json = serde_json::to_string_pretty(&output)?;
            if let Some(out_path) = &args.out {
                fs::write(out_path, &json)?;
                p::success(&format!("JSON saved to {}", out_path.display()));
            } else {
                println!("{}", json);
            }
        }
        _ => {
            println!();
            p::kv("Contract", &contract_name);
            p::kv("Total functions", &analysis.total_functions.to_string());
            p::kv("Public functions", &analysis.public_functions.to_string());
            p::kv("Entry points", &analysis.entry_points.to_string());
            p::kv(
                "Mutating functions",
                &analysis.mutating_functions.to_string(),
            );
            p::kv(
                "Read-only functions",
                &analysis.read_only_functions.to_string(),
            );
            p::kv("Complex functions", &analysis.complex_functions.to_string());

            if !analysis.storage_accesses.is_empty() {
                p::kv(
                    "Storage accesses",
                    &analysis.storage_accesses.len().to_string(),
                );
            }
            if !analysis.external_calls.is_empty() {
                p::kv("External calls", &analysis.external_calls.len().to_string());
            }

            println!();
            p::separator();
            println!("{}", "Test Priority Recommendations".bold());
            println!();

            for suggestion in &priorities {
                let priority_badge = match suggestion.priority {
                    ata::TestPriority::Critical => format!("[{}]", "CRITICAL".red().bold()),
                    ata::TestPriority::High => format!("[{}]", "HIGH".yellow().bold()),
                    ata::TestPriority::Medium => format!("[{}]", "MEDIUM".cyan()),
                    ata::TestPriority::Low => format!("[{}]", "LOW".white()),
                };

                println!(
                    "  {} {} — {}",
                    priority_badge,
                    suggestion.function_name.bright_white().bold(),
                    suggestion.test_types.join(", ")
                );
                println!("    {}", suggestion.rationale.dimmed());
                println!(
                    "    Estimated tests needed: {}",
                    suggestion.estimated_tests_needed
                );
                println!();
            }

            let total_estimated: u32 = priorities.iter().map(|p| p.estimated_tests_needed).sum();
            p::separator();
            p::kv("Total estimated tests needed", &total_estimated.to_string());
        }
    }

    Ok(())
}

async fn handle_optimize(args: OptimizeArgs) -> Result<()> {
    p::header("Test Optimization");

    let source_code = ata::read_source_file(&args.source)?;
    let test_code = fs::read_to_string(&args.tests)
        .with_context(|| format!("Failed to read tests: {}", args.tests.display()))?;

    let goals: Vec<ata::OptimizationGoal> = args
        .goals
        .iter()
        .map(|g| match g.as_str() {
            "duplication" | "reduce_duplication" => ata::OptimizationGoal::ReduceDuplication,
            "performance" | "improve_performance" => ata::OptimizationGoal::ImprovePerformance,
            "coverage" | "increase_coverage" => ata::OptimizationGoal::IncreaseCoverage,
            "assertions" | "better_assertions" => ata::OptimizationGoal::BetterAssertions,
            "setup" | "simplify_setup" => ata::OptimizationGoal::SimplifySetup,
            _ => ata::OptimizationGoal::All,
        })
        .collect();

    let quality_before = ata::calculate_test_quality_score(&test_code, &source_code);

    p::kv("Source", &args.source.display().to_string());
    p::kv("Tests", &args.tests.display().to_string());
    p::kv(
        "Current quality score",
        &format!("{:.1}/100", quality_before.overall),
    );
    p::kv("Goals", &args.goals.join(", "));
    println!();

    if args.use_ai {
        let prompt = ata::build_optimization_prompt(&ata::TestOptimizationRequest {
            test_code: test_code.clone(),
            contract_code: source_code.clone(),
            optimization_goals: goals.clone(),
        });

        if ollama::is_ollama_running().await {
            p::info(&format!("Sending to {} for optimization...", args.model));
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
            .context("AI optimization failed")?;

            let optimized_code = response.response;
            let quality_after = ata::calculate_test_quality_score(&optimized_code, &source_code);

            print_optimization_result(
                &test_code,
                &optimized_code,
                &quality_before,
                &quality_after,
                &args,
            )?;
        } else {
            p::warn("Ollama is not running. Using local optimization.");
            let result = optimize_locally(&test_code, &source_code, &goals);
            print_optimization_result(
                &test_code,
                &result,
                &quality_before,
                &ata::calculate_test_quality_score(&result, &source_code),
                &args,
            )?;
        }
    } else {
        let result = optimize_locally(&test_code, &source_code, &goals);
        let quality_after = ata::calculate_test_quality_score(&result, &source_code);
        print_optimization_result(&test_code, &result, &quality_before, &quality_after, &args)?;
    }

    Ok(())
}

fn optimize_locally(
    test_code: &str,
    _source_code: &str,
    goals: &[ata::OptimizationGoal],
) -> String {
    let mut optimized = test_code.to_string();

    if goals.contains(&ata::OptimizationGoal::ReduceDuplication)
        || goals.contains(&ata::OptimizationGoal::All)
    {
        let setup_pattern = "let env = Env::default();";
        if optimized.matches(setup_pattern).count() > 2 {
            let helper =
                "fn setup_test_env() -> soroban_sdk::tests::Env {\n    Env::default()\n}\n\n";
            optimized = format!("{}\n{}", helper, optimized);
        }
    }

    if goals.contains(&ata::OptimizationGoal::BetterAssertions)
        || goals.contains(&ata::OptimizationGoal::All)
    {
        optimized = optimized.replace(
            "assert!(true);",
            "assert!(result.is_ok(), \"Operation should succeed\");",
        );
        optimized = optimized.replace(
            "assert_eq!(1, 1);",
            "assert_eq!(actual, expected, \"Values should match\");",
        );
    }

    optimized
}

fn print_optimization_result(
    original: &str,
    optimized: &str,
    before: &ata::TestQualityScore,
    after: &ata::TestQualityScore,
    args: &OptimizeArgs,
) -> Result<()> {
    println!();
    p::separator();
    p::kv("Quality before", &format!("{:.1}/100", before.overall));
    p::kv("Quality after", &format!("{:.1}/100", after.overall));
    p::kv(
        "Improvement",
        &format!("{:.1}%", after.overall - before.overall),
    );

    println!();
    println!("{}", "Improvements:".bold());

    if before.test_count < after.test_count {
        println!(
            "  ✓ Added {} new test cases",
            after.test_count - before.test_count
        );
    }
    if before.assertion_count < after.assertion_count {
        println!(
            "  ✓ Added {} new assertions",
            after.assertion_count - before.assertion_count
        );
    }
    if !before.has_edge_cases && after.has_edge_cases {
        println!("  ✓ Added edge case coverage");
    }
    if !before.has_security && after.has_security {
        println!("  ✓ Added security test coverage");
    }
    if !before.has_error_handling && after.has_error_handling {
        println!("  ✓ Added error handling tests");
    }

    match args.format.as_str() {
        "json" => {
            let output = serde_json::json!({
                "original": original,
                "optimized": optimized,
                "quality_before": before,
                "quality_after": after,
            });
            let json = serde_json::to_string_pretty(&output)?;
            if let Some(out_path) = &args.out {
                fs::write(out_path, &json)?;
                p::success(&format!("JSON saved to {}", out_path.display()));
            } else {
                println!("{}", json);
            }
        }
        "code" => {
            if let Some(out_path) = &args.out {
                fs::write(out_path, optimized)?;
                p::success(&format!("Optimized tests saved to {}", out_path.display()));
            } else {
                println!();
                println!("{}", optimized);
            }
        }
        _ => {
            println!();
            println!("Optimized code:");
            println!("{}", optimized);
        }
    }

    Ok(())
}

fn handle_coverage(args: CoverageArgs) -> Result<()> {
    p::header("Coverage Analysis & Improvement");

    let source_code = ata::read_source_file(&args.source)?;
    let test_code = fs::read_to_string(&args.tests)
        .with_context(|| format!("Failed to read tests: {}", args.tests.display()))?;

    let coverage_data = if let Some(coverage_path) = &args.coverage_report {
        let coverage_json = fs::read_to_string(coverage_path)?;
        serde_json::from_str(&coverage_json)?
    } else {
        let analysis = ata::analyze_contract_for_testing(&source_code)?;
        let test_analysis = ata::analyze_contract_for_testing(&test_code).unwrap_or_else(|_| {
            ata::ContractAnalysis {
                total_functions: 0,
                public_functions: 0,
                entry_points: 0,
                mutating_functions: 0,
                read_only_functions: 0,
                complex_functions: 0,
                functions: vec![],
                storage_accesses: vec![],
                external_calls: vec![],
            }
        });

        let covered = test_analysis.total_functions.min(analysis.total_functions);
        ata::CoverageInput {
            functions_total: analysis.total_functions,
            functions_covered: covered,
            lines_total: analysis.total_functions * 10,
            lines_covered: covered * 8,
            branches_total: analysis.total_functions * 3,
            branches_covered: covered * 2,
            uncovered_functions: analysis
                .functions
                .iter()
                .skip(covered as usize)
                .map(|f| f.name.clone())
                .collect(),
        }
    };

    let prompt = ata::build_coverage_improvement_prompt(&ata::CoverageAnalysisRequest {
        source_code: source_code.clone(),
        test_code: test_code.clone(),
        coverage_data: coverage_data.clone(),
    });

    let current_score = if coverage_data.functions_total > 0 {
        coverage_data.functions_covered as f64 / coverage_data.functions_total as f64 * 100.0
    } else {
        0.0
    };

    p::kv("Source", &args.source.display().to_string());
    p::kv("Tests", &args.tests.display().to_string());
    p::kv("Current coverage", &format!("{:.1}%", current_score));
    p::kv("Target coverage", &format!("{:.1}%", args.target));
    p::kv(
        "Gap",
        &format!("{:.1}%", (args.target - current_score).max(0.0)),
    );
    println!();

    let suggestions = analyze_coverage_gaps(&source_code, &test_code, &coverage_data);

    match args.format.as_str() {
        "json" => {
            let output = serde_json::json!({
                "coverage_data": coverage_data,
                "current_score": current_score,
                "target_score": args.target,
                "suggestions": suggestions,
            });
            let json = serde_json::to_string_pretty(&output)?;
            if let Some(out_path) = &args.out {
                fs::write(out_path, &json)?;
                p::success(&format!("JSON saved to {}", out_path.display()));
            } else {
                println!("{}", json);
            }
        }
        _ => {
            println!("{}", "Coverage Improvement Suggestions:".bold());
            println!();

            for suggestion in &suggestions {
                println!(
                    "  {} — {}",
                    suggestion.function.bright_white().bold(),
                    suggestion.description
                );
                println!("    Type: {:?}", suggestion.suggestion_type);
                println!(
                    "    Estimated lines covered: {}",
                    suggestion.estimated_lines_covered
                );
                println!("    Difficulty: {}", suggestion.difficulty);
                println!();
            }

            let total_improvement: u32 =
                suggestions.iter().map(|s| s.estimated_lines_covered).sum();
            p::separator();
            p::kv(
                "Potential improvement",
                &format!("+{} lines", total_improvement),
            );
        }
    }

    Ok(())
}

fn analyze_coverage_gaps(
    source_code: &str,
    _test_code: &str,
    coverage: &ata::CoverageInput,
) -> Vec<ata::CoverageSuggestion> {
    let mut suggestions = Vec::new();

    for uncovered_func in &coverage.uncovered_functions {
        suggestions.push(ata::CoverageSuggestion {
            function: uncovered_func.clone(),
            suggestion_type: ata::SuggestionType::AddUnitTest,
            description: format!("Add unit test for uncovered function '{}'", uncovered_func),
            estimated_lines_covered: 10,
            difficulty: "easy".to_string(),
        });

        suggestions.push(ata::CoverageSuggestion {
            function: uncovered_func.clone(),
            suggestion_type: ata::SuggestionType::AddEdgeCaseTest,
            description: format!("Add edge case tests for '{}'", uncovered_func),
            estimated_lines_covered: 15,
            difficulty: "medium".to_string(),
        });
    }

    let total_lines_gap = coverage.lines_total.saturating_sub(coverage.lines_covered);
    if total_lines_gap > 50 {
        suggestions.push(ata::CoverageSuggestion {
            function: "overall".to_string(),
            suggestion_type: ata::SuggestionType::AddIntegrationTest,
            description: "Add integration tests for contract workflows".to_string(),
            estimated_lines_covered: 30,
            difficulty: "hard".to_string(),
        });
    }

    suggestions
}

fn handle_maintain(args: MaintainArgs) -> Result<()> {
    p::header("Test Maintenance Analysis");

    let source_code = ata::read_source_file(&args.path)?;

    let test_files = if let Some(test_path) = &args.tests {
        if test_path.is_dir() {
            ata::find_test_files(test_path)
        } else {
            vec![test_path.clone()]
        }
    } else {
        let project_path = args.path.parent().unwrap_or(&args.path).to_path_buf();
        ata::find_test_files(&project_path)
    };

    if test_files.is_empty() {
        p::warn("No test files found");
        return Ok(());
    }

    let mut all_outdated: Vec<ata::OutdatedTest> = Vec::new();
    let all_broken: Vec<ata::BrokenTest> = Vec::new();
    let mut all_missing: Vec<ata::MissingTest> = Vec::new();
    let all_recommendations: Vec<String> = Vec::new();

    for test_file in &test_files {
        let test_code = fs::read_to_string(test_file)
            .with_context(|| format!("Failed to read test file: {}", test_file.display()))?;

        let source_analysis = ata::analyze_contract_for_testing(&source_code)?;
        let test_analysis = ata::analyze_contract_for_testing(&test_code).unwrap_or_else(|_| {
            ata::ContractAnalysis {
                total_functions: 0,
                public_functions: 0,
                entry_points: 0,
                mutating_functions: 0,
                read_only_functions: 0,
                complex_functions: 0,
                functions: vec![],
                storage_accesses: vec![],
                external_calls: vec![],
            }
        });

        let source_func_names: Vec<String> = source_analysis
            .functions
            .iter()
            .map(|f| f.name.clone())
            .collect();

        let test_func_names: Vec<String> = test_analysis
            .functions
            .iter()
            .filter(|f| f.name.starts_with("test_"))
            .map(|f| f.name.clone())
            .collect();

        for source_func in &source_func_names {
            let has_test = test_func_names.iter().any(|t| {
                t.contains(source_func)
                    || source_func
                        .strip_prefix("test_")
                        .map_or(false, |stripped| t.contains(stripped))
            });

            if !has_test {
                all_missing.push(ata::MissingTest {
                    function_name: source_func.clone(),
                    test_type: "unit".to_string(),
                    reason: format!("No test found for function '{}'", source_func),
                    priority: ata::TestPriority::High,
                });
            }
        }

        if test_code.contains("todo!()") || test_code.contains("unimplemented!()") {
            all_outdated.push(ata::OutdatedTest {
                test_name: "contains_unimplemented".to_string(),
                reason: "Test contains todo!() or unimplemented!() macros".to_string(),
                severity: "high".to_string(),
                suggested_fix: "Implement the test or remove it".to_string(),
            });
        }

        if test_code
            .lines()
            .any(|l| l.contains("unwrap()") && l.contains("#[test]"))
        {
            all_outdated.push(ata::OutdatedTest {
                test_name: "uses_unwrap_in_test".to_string(),
                reason: "Test uses unwrap() which may panic silently".to_string(),
                severity: "medium".to_string(),
                suggested_fix: "Use assert! or expect() for better error messages".to_string(),
            });
        }
    }

    let total_tests = all_outdated.len() + all_broken.len() + all_missing.len();
    let maintenance_score = if total_tests == 0 {
        100.0
    } else {
        (100.0 - (total_tests as f64 * 5.0)).max(0.0)
    };

    match args.format.as_str() {
        "json" => {
            let output = serde_json::json!({
                "outdated_tests": all_outdated,
                "broken_tests": all_broken,
                "missing_tests": all_missing,
                "maintenance_score": maintenance_score,
                "recommendations": all_recommendations,
            });
            let json = serde_json::to_string_pretty(&output)?;
            if let Some(out_path) = &args.out {
                fs::write(out_path, &json)?;
                p::success(&format!("JSON saved to {}", out_path.display()));
            } else {
                println!("{}", json);
            }
        }
        _ => {
            p::kv(
                "Maintenance score",
                &format!("{:.1}/100", maintenance_score),
            );
            println!();

            if !all_outdated.is_empty() {
                println!(
                    "{} ({})",
                    "Outdated Tests:".yellow().bold(),
                    all_outdated.len()
                );
                for test in &all_outdated {
                    println!("  [{}] {}", test.severity.to_uppercase(), test.test_name);
                    println!("    Reason: {}", test.reason);
                    println!("    Fix: {}", test.suggested_fix);
                }
                println!();
            }

            if !all_broken.is_empty() {
                println!("{} ({})", "Broken Tests:".red().bold(), all_broken.len());
                for test in &all_broken {
                    println!("  {} — {}", test.test_name, test.error_message);
                    println!("    Fix: {}", test.suggested_fix);
                }
                println!();
            }

            if !all_missing.is_empty() {
                println!("{} ({})", "Missing Tests:".cyan().bold(), all_missing.len());
                for test in &all_missing {
                    println!("  {} — {}", test.function_name, test.reason);
                }
                println!();
            }

            p::separator();
            p::kv("Test files analyzed", &test_files.len().to_string());
            p::kv("Issues found", &total_tests.to_string());
        }
    }

    Ok(())
}

async fn handle_mocks(args: MocksArgs) -> Result<()> {
    p::header("Mock Generation");

    let source_code = ata::read_source_file(&args.source)?;

    let mock_types: Vec<ata::MockType> = args
        .types
        .iter()
        .map(|t| match t.as_str() {
            "address" => ata::MockType::Address,
            "storage" => ata::MockType::Storage,
            "contract" => ata::MockType::Contract,
            "env" => ata::MockType::Env,
            "events" => ata::MockType::Events,
            _ => ata::MockType::All,
        })
        .collect();

    let suggestions = ata::generate_mock_suggestions(&source_code);

    p::kv("Source", &args.source.display().to_string());
    p::kv("Mock types", &args.types.join(", "));
    println!();

    if args.use_ai {
        let prompt = ata::build_mock_generation_prompt(&ata::MockGenerationRequest {
            contract_code: source_code.clone(),
            mock_types: mock_types.clone(),
        });

        if ollama::is_ollama_running().await {
            p::info(&format!("Generating mocks with {}...", args.model));
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
            .context("AI mock generation failed")?;

            print_mock_output(&response.response, &args)?;
        } else {
            p::warn("Ollama is not running. Using local mock generation.");
            let mocks_code = generate_local_mocks(&suggestions);
            print_mock_output(&mocks_code, &args)?;
        }
    } else {
        let mocks_code = generate_local_mocks(&suggestions);
        print_mock_output(&mocks_code, &args)?;
    }

    Ok(())
}

fn generate_local_mocks(suggestions: &[ata::MockSuggestion]) -> String {
    let mut code = String::from(
        "// Generated by StarForge AI Test Assistant\n// Mock objects for contract testing\n\n",
    );

    for suggestion in suggestions {
        match suggestion.mock_type.as_str() {
            "address" => {
                code.push_str(
                    "#[cfg(test)]\n\
                     pub mod mock_address {\n\
                     \x20   use soroban_sdk::Address;\n\
                     \x20   use soroban_sdk::tests::Env;\n\
                     \n\
                     \x20   pub fn random(env: &Env) -> Address {\n\
                     \x20       Address::random(env)\n\
                     \x20   }\n\
                     \n\
                     \x20   pub fn generate(id: u32) -> Address {\n\
                     \x20       // Generate deterministic address for testing\n\
                     \x20       Address::from_string(\n\
                     \x20           env,\n\
                     \x20           &format!(\"GA{:055}\", id)\n\
                     \x20       )\n\
                     \x20   }\n\
                     }\n\n",
                );
            }
            "storage" => {
                code.push_str(
                    "#[cfg(test)]\n\
                     pub mod mock_storage {\n\
                     \x20   use std::collections::HashMap;\n\
                     \n\
                     \x20   #[derive(Default, Clone)]\n\
                     \x20   pub struct InMemoryStorage {\n\
                     \x20       entries: HashMap<Vec<u8>, Vec<u8>>,\n\
                     \x20   }\n\
                     \n\
                     \x20   impl InMemoryStorage {\n\
                     \x20       pub fn new() -> Self {\n\
                     \x20           Self::default()\n\
                     \x20       }\n\
                     \n\
                     \x20       pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {\n\
                     \x20           self.entries.get(key).cloned()\n\
                     \x20       }\n\
                     \n\
                     \x20       pub fn set(&mut self, key: Vec<u8>, value: Vec<u8>) {\n\
                     \x20           self.entries.insert(key, value);\n\
                     \x20       }\n\
                     \n\
                     \x20       pub fn has(&self, key: &[u8]) -> bool {\n\
                     \x20           self.entries.contains_key(key)\n\
                     \x20       }\n\
                     \n\
                     \x20       pub fn clear(&mut self) {\n\
                     \x20           self.entries.clear();\n\
                     \x20       }\n\
                     \x20   }\n\
                     }\n\n",
                );
            }
            "env" => {
                code.push_str(
                    "#[cfg(test)]\n\
                     pub mod mock_env {\n\
                     \x20   use soroban_sdk::tests::Env;\n\
                     \n\
                     \x20   pub fn setup() -> Env {\n\
                     \x20       Env::default()\n\
                     \x20   }\n\
                     \n\
                     \x20   pub fn setup_with_account(id: u32) -> (Env, soroban_sdk::Address) {\n\
                     \x20       let env = Env::default();\n\
                     \x20       let address = soroban_sdk::Address::random(&env);\n\
                     \x20       (env, address)\n\
                     \x20   }\n\
                     }\n\n",
                );
            }
            "contract_client" => {
                code.push_str(
                    "#[cfg(test)]\n\
                     pub mod mock_contract {\n\
                     \x20   use soroban_sdk::{Address, Env};\n\
                     \n\
                     \x20   pub struct MockContractClient {\n\
                     \x20       env: Env,\n\
                     \x20       address: Address,\n\
                     \x20   }\n\
                     \n\
                     \x20   impl MockContractClient {\n\
                     \x20       pub fn new(env: &Env) -> Self {\n\
                     \x20           Self {\n\
                     \x20               env: env.clone(),\n\
                     \x20               address: Address::random(env),\n\
                     \x20           }\n\
                     \x20       }\n\
                     \n\
                     \x20       pub fn address(&self) -> &Address {\n\
                     \x20           &self.address\n\
                     \x20       }\n\
                     \x20   }\n\
                     }\n\n",
                );
            }
            "events" => {
                code.push_str(
                    "#[cfg(test)]\n\
                     pub mod mock_events {\n\
                     \x20   use soroban_sdk::Env;\n\
                     \n\
                     \x20   pub struct EventTracker {\n\
                     \x20       events: Vec<(String, String)>,\n\
                     \x20   }\n\
                     \n\
                     \x20   impl EventTracker {\n\
                     \x20       pub fn new() -> Self {\n\
                     \x20           Self { events: vec![] }\n\
                     \x20       }\n\
                     \n\
                     \x20       pub fn track(&mut self, topic: &str, data: &str) {\n\
                     \x20           self.events.push((topic.to_string(), data.to_string()));\n\
                     \x20       }\n\
                     \n\
                     \x20       pub fn events(&self) -> &[(String, String)] {\n\
                     \x20           &self.events\n\
                     \x20       }\n\
                     \n\
                     \x20       pub fn contains(&self, topic: &str) -> bool {\n\
                     \x20           self.events.iter().any(|(t, _)| t == topic)\n\
                     \x20       }\n\
                     \x20   }\n\
                     }\n\n",
                );
            }
            _ => {}
        }
    }

    code
}

fn print_mock_output(code: &str, args: &MocksArgs) -> Result<()> {
    match args.format.as_str() {
        "json" => {
            let output = serde_json::json!({
                "mocks_code": code,
                "mock_types": args.types,
            });
            let json = serde_json::to_string_pretty(&output)?;
            if let Some(out_path) = &args.out {
                fs::write(out_path, &json)?;
                p::success(&format!("JSON saved to {}", out_path.display()));
            } else {
                println!("{}", json);
            }
        }
        _ => {
            if let Some(out_path) = &args.out {
                fs::write(out_path, code)?;
                p::success(&format!("Mocks saved to {}", out_path.display()));
            } else {
                println!("{}", code);
            }
        }
    }

    Ok(())
}

async fn handle_test_data(args: TestDataArgs) -> Result<()> {
    p::header("Test Data Generation");

    let source_code = ata::read_source_file(&args.source)?;

    let data_types: Vec<ata::DataType> = args
        .types
        .iter()
        .map(|t| match t.as_str() {
            "address" => ata::DataType::Address,
            "amount" => ata::DataType::Amount,
            "string" => ata::DataType::String,
            "bytes" => ata::DataType::Bytes,
            "timestamp" => ata::DataType::Timestamp,
            "boolean" => ata::DataType::Boolean,
            _ => ata::DataType::All,
        })
        .collect();

    let suggestions = ata::generate_test_data_suggestions(&source_code);

    p::kv("Source", &args.source.display().to_string());
    p::kv("Data types", &args.types.join(", "));
    p::kv("Count per type", &args.count.to_string());
    println!();

    if args.use_ai {
        let prompt = ata::build_test_data_prompt(&ata::TestDataGenerationRequest {
            contract_code: source_code.clone(),
            data_types: data_types.clone(),
            count_per_type: args.count,
            constraints: vec![],
        });

        if ollama::is_ollama_running().await {
            p::info(&format!("Generating test data with {}...", args.model));
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
            .context("AI test data generation failed")?;

            print_test_data_output(&response.response, &args)?;
        } else {
            p::warn("Ollama is not running. Using local test data generation.");
            let data_code = generate_local_test_data(&suggestions, args.count);
            print_test_data_output(&data_code, &args)?;
        }
    } else {
        let data_code = generate_local_test_data(&suggestions, args.count);
        print_test_data_output(&data_code, &args)?;
    }

    Ok(())
}

fn generate_local_test_data(suggestions: &[ata::TestDataSuggestion], count: u32) -> String {
    let mut code = String::from(
        "// Generated by StarForge AI Test Assistant\n// Test data generators and edge cases\n\n",
    );

    for suggestion in suggestions {
        code.push_str(&format!(
            "// {} — {}\n",
            suggestion.field, suggestion.description
        ));

        match suggestion.data_type.as_str() {
            "address" => {
                code.push_str(&format!(
                    "pub fn generate_{}_addresses(env: &Env, count: u32) -> Vec<Address> {{\n\
                     \x20   (0..count).map(|_| Address::random(env)).collect()\n\
                     }}\n\n",
                    suggestion.field
                ));
                code.push_str(&format!(
                    "pub fn {}_edge_cases(env: &Env) -> Vec<Address> {{\n\
                     \x20   vec![\n\
                     \x20       Address::random(env),  // Random valid address\n\
                     \x20       // TODO: Add zero address, self-referencing, contract address\n\
                     \x20   ]\n\
                     }}\n\n",
                    suggestion.field
                ));
            }
            "amount" => {
                code.push_str(&format!(
                    "pub fn generate_{}_values(count: u32) -> Vec<i64> {{\n\
                     \x20   vec![0, 1, 100, 1_000, 1_000_000, i64::MAX]\n\
                     }}\n\n",
                    suggestion.field
                ));
                code.push_str(&format!(
                    "pub fn {}_edge_cases() -> Vec<i64> {{\n\
                     \x20   vec![\n\
                     \x20       0,           // Zero\n\
                     \x20       1,           // Minimum positive\n\
                     \x20       i64::MAX,    // Maximum value\n\
                     \x20       i64::MIN,    // Minimum value (negative)\n\
                     \x20       -1,          // Negative one\n\
                     \x20       1_000_000,   // Large amount\n\
                     \x20   ]\n\
                     }}\n\n",
                    suggestion.field
                ));
            }
            "string" => {
                code.push_str(&format!(
                    "pub fn generate_{}_values(count: u32) -> Vec<soroban_sdk::String> {{\n\
                     \x20   vec![\"test\".into(), \"hello\".into(), \"a\".repeat(100).into()]\n\
                     }}\n\n",
                    suggestion.field
                ));
                code.push_str(&format!(
                    "pub fn {}_edge_cases() -> Vec<soroban_sdk::String> {{\n\
                     \x20   vec![\n\
                     \x20       \"\".into(),                          // Empty string\n\
                     \x20       \"a\".into(),                         // Single character\n\
                     \x20       \"a\".repeat(1000).into(),             // Long string\n\
                     \x20       \"special!@#$%^&*()\".into(),          // Special characters\n\
                     \x20       \"unicode: 🚀 🔐 💰\".into(),       // Unicode\n\
                     \x20   ]\n\
                     }}\n\n",
                    suggestion.field
                ));
            }
            _ => {
                code.push_str(&format!(
                    "// TODO: Implement generators for type '{}'\n\n",
                    suggestion.data_type
                ));
            }
        }
    }

    code
}

fn print_test_data_output(code: &str, args: &TestDataArgs) -> Result<()> {
    match args.format.as_str() {
        "json" => {
            let output = serde_json::json!({
                "test_data_code": code,
                "data_types": args.types,
                "count": args.count,
            });
            let json = serde_json::to_string_pretty(&output)?;
            if let Some(out_path) = &args.out {
                fs::write(out_path, &json)?;
                p::success(&format!("JSON saved to {}", out_path.display()));
            } else {
                println!("{}", json);
            }
        }
        _ => {
            if let Some(out_path) = &args.out {
                fs::write(out_path, code)?;
                p::success(&format!("Test data saved to {}", out_path.display()));
            } else {
                println!("{}", code);
            }
        }
    }

    Ok(())
}

fn handle_quality(args: QualityArgs) -> Result<()> {
    p::header("Test Quality Analysis");

    let source_code = ata::read_source_file(&args.source)?;
    let test_code = fs::read_to_string(&args.tests)
        .with_context(|| format!("Failed to read tests: {}", args.tests.display()))?;

    let score = ata::calculate_test_quality_score(&test_code, &source_code);

    match args.format.as_str() {
        "json" => {
            let json = serde_json::to_string_pretty(&score)?;
            println!("{}", json);
        }
        _ => {
            println!();
            p::kv("Source", &args.source.display().to_string());
            p::kv("Tests", &args.tests.display().to_string());
            println!();
            p::separator();

            let score_color = match score.overall as u32 {
                90..=100 => format!("{:.1}/100", score.overall).green().bold(),
                70..=89 => format!("{:.1}/100", score.overall).green(),
                50..=69 => format!("{:.1}/100", score.overall).yellow(),
                _ => format!("{:.1}/100", score.overall).red(),
            };

            p::kv("Overall quality", &score_color.to_string());
            println!();
            p::kv("Test count", &score.test_count.to_string());
            p::kv("Assertion count", &score.assertion_count.to_string());
            p::kv(
                "Test-to-function ratio",
                &format!("{:.2}", score.test_to_func_ratio),
            );
            p::kv(
                "Assertion density",
                &format!("{:.2} per test", score.assertion_density),
            );
            println!();
            p::kv("Has setup helpers", if score.has_setup { "✓" } else { "✗" });
            p::kv(
                "Has edge cases",
                if score.has_edge_cases { "✓" } else { "✗" },
            );
            p::kv(
                "Has security tests",
                if score.has_security { "✓" } else { "✗" },
            );
            p::kv(
                "Has error handling",
                if score.has_error_handling {
                    "✓"
                } else {
                    "✗"
                },
            );
            println!();

            println!("{}", "Recommendations:".bold());
            if !score.has_setup {
                println!("  → Add shared test setup helpers to reduce duplication");
            }
            if !score.has_edge_cases {
                println!("  → Add edge case tests for boundary conditions");
            }
            if !score.has_security {
                println!("  → Add security tests for authorization checks");
            }
            if !score.has_error_handling {
                println!("  → Add error handling tests for failure paths");
            }
            if score.test_to_func_ratio < 1.0 {
                println!(
                    "  → Add more tests (current ratio: {:.2} tests per function)",
                    score.test_to_func_ratio
                );
            }
            if score.assertion_density < 2.0 {
                println!(
                    "  → Add more assertions (current: {:.2} per test)",
                    score.assertion_density
                );
            }
        }
    }

    Ok(())
}

// ─── Helper functions ──────────────────────────────────────────────────────────

fn parse_test_type(s: &str) -> Result<ata::TestType> {
    match s.to_lowercase().as_str() {
        "unit" => Ok(ata::TestType::Unit),
        "integration" => Ok(ata::TestType::Integration),
        "edge_case" | "edge-case" | "edgecase" => Ok(ata::TestType::EdgeCase),
        "security" => Ok(ata::TestType::Security),
        "all" => Ok(ata::TestType::All),
        _ => Err(anyhow::anyhow!(
            "Invalid test type '{}'. Valid types: unit, integration, edge_case, security, all",
            s
        )),
    }
}
