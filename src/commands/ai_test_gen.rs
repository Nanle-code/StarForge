//! AI Test Generation Commands
//!
//! Provides commands for generating comprehensive test suites including
//! unit tests, integration tests, E2E tests, property-based testing, fuzzing, and regression tests.

use crate::utils::{
    ai_test_generator::{AiTestGenerator, TestCategory, TestGenerationConfig, TestType},
    print as p,
};
use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum AiTestGenCommands {
    /// Generate comprehensive test suite for a contract
    Generate {
        /// Path to the Rust source file
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Output directory for generated tests
        #[arg(short, long, default_value = "./tests")]
        output: PathBuf,

        /// Target coverage percentage (0-100)
        #[arg(long, default_value_t = 90)]
        coverage: u32,

        /// Include unit tests
        #[arg(long, default_value_t = true)]
        unit: bool,

        /// Include integration tests
        #[arg(long, default_value_t = true)]
        integration: bool,

        /// Include E2E tests
        #[arg(long)]
        e2e: bool,

        /// Include property-based tests
        #[arg(long, default_value_t = true)]
        property: bool,

        /// Include fuzzing tests
        #[arg(long, default_value_t = true)]
        fuzzing: bool,

        /// Include regression tests
        #[arg(long, default_value_t = true)]
        regression: bool,

        /// Maximum test complexity (1-10)
        #[arg(long, default_value_t = 10)]
        max_complexity: u32,
    },

    /// Show test generation analytics
    Analytics,

    /// Reset test generation analytics
    ResetAnalytics,

    /// Analyze code structure without generating tests
    Analyze {
        /// Path to the Rust source file
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
}

pub async fn handle(cmd: AiTestGenCommands) -> Result<()> {
    match cmd {
        AiTestGenCommands::Generate {
            file,
            output,
            coverage,
            unit,
            integration,
            e2e,
            property,
            fuzzing,
            regression,
            max_complexity,
        } => {
            handle_generate(
                file,
                output,
                coverage,
                unit,
                integration,
                e2e,
                property,
                fuzzing,
                regression,
                max_complexity,
            )
            .await
        }
        AiTestGenCommands::Analytics => handle_analytics().await,
        AiTestGenCommands::ResetAnalytics => handle_reset_analytics().await,
        AiTestGenCommands::Analyze { file } => handle_analyze(file).await,
    }
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
async fn handle_generate(
    file: PathBuf,
    output: PathBuf,
    coverage: u32,
    unit: bool,
    integration: bool,
    e2e: bool,
    property: bool,
    fuzzing: bool,
    regression: bool,
    max_complexity: u32,
) -> Result<()> {
    p::header("AI Test Generation");
    p::separator();

    // Validate input file
    if !file.exists() {
        anyhow::bail!("File not found: {}", file.display());
    }

    let code = std::fs::read_to_string(&file)
        .with_context(|| format!("Failed to read file: {}", file.display()))?;

    p::kv("Source File", &file.display().to_string());
    p::kv("Output Directory", &output.display().to_string());
    p::kv("Target Coverage", &format!("{}%", coverage));
    p::kv("Max Complexity", &max_complexity.to_string());
    println!();

    p::info("Test Types:");
    if unit {
        p::info("  ✓ Unit Tests");
    }
    if integration {
        p::info("  ✓ Integration Tests");
    }
    if e2e {
        p::info("  ✓ E2E Tests");
    }
    if property {
        p::info("  ✓ Property-Based Tests");
    }
    if fuzzing {
        p::info("  ✓ Fuzzing Tests");
    }
    if regression {
        p::info("  ✓ Regression Tests");
    }
    println!();

    let config = TestGenerationConfig {
        include_unit_tests: unit,
        include_integration_tests: integration,
        include_e2e_tests: e2e,
        include_property_based: property,
        include_fuzzing: fuzzing,
        include_regression: regression,
        target_coverage: coverage as f64 / 100.0,
        max_complexity,
    };

    let generator = AiTestGenerator::new().with_config(config);

    p::info("Analyzing code structure...");
    let spinner = p::spinner("Generating test suite...");

    let test_suite = generator.generate_test_suite(&file, &code).await?;

    spinner.finish_and_clear();

    p::success(&format!("Generated {} tests", test_suite.tests.len()));
    p::kv(
        "Estimated Coverage",
        &format!("{:.1}%", test_suite.coverage_estimate * 100.0),
    );

    // Show test breakdown
    println!();
    p::info("Test Breakdown:");
    let mut breakdown: std::collections::HashMap<TestType, usize> =
        std::collections::HashMap::new();
    for test in &test_suite.tests {
        *breakdown.entry(test.test_type.clone()).or_insert(0) += 1;
    }

    for (test_type, count) in breakdown {
        p::kv(&format!("{:?}", test_type), &count.to_string());
    }

    // Write test suite
    let output_file = output.join(format!("{}.rs", test_suite.name));
    generator.write_test_suite(&test_suite, &output_file)?;

    println!();
    p::success(&format!("Test suite written to: {}", output_file.display()));

    // Show coverage by category
    println!();
    p::info("Test Categories:");
    let mut category_breakdown: std::collections::HashMap<TestCategory, usize> =
        std::collections::HashMap::new();
    for test in &test_suite.tests {
        *category_breakdown.entry(test.category.clone()).or_insert(0) += 1;
    }

    for (category, count) in category_breakdown {
        p::kv(&format!("{:?}", category), &count.to_string());
    }

    p::separator();
    p::info("Run tests with: cargo test");
    Ok(())
}

async fn handle_analytics() -> Result<()> {
    p::header("Test Generation Analytics");
    p::separator();

    let generator = AiTestGenerator::new();
    let analytics = generator.get_analytics().await;

    p::kv(
        "Total Tests Generated",
        &analytics.total_tests_generated.to_string(),
    );
    p::kv(
        "Average Coverage",
        &format!("{:.1}%", analytics.average_coverage * 100.0),
    );
    p::kv(
        "Total Generation Time",
        &format!("{} ms", analytics.generation_time_ms),
    );
    println!();

    p::info("Tests by Type:");
    let mut type_breakdown: Vec<_> = analytics.tests_by_type.iter().collect();
    type_breakdown.sort_by(|a, b| b.1.cmp(a.1));

    for (test_type, count) in type_breakdown {
        p::kv(&format!("{:?}", test_type), &count.to_string());
    }

    println!();
    p::info("Tests by Category:");
    let mut category_breakdown: Vec<_> = analytics.tests_by_category.iter().collect();
    category_breakdown.sort_by(|a, b| b.1.cmp(a.1));

    for (category, count) in category_breakdown {
        p::kv(&format!("{:?}", category), &count.to_string());
    }

    p::separator();
    Ok(())
}

async fn handle_reset_analytics() -> Result<()> {
    p::header("Reset Test Generation Analytics");
    p::separator();

    let _generator = AiTestGenerator::new();
    // Note: This would require adding a reset method to AiTestGenerator
    // For now, just inform the user
    p::info("Analytics reset functionality would be implemented here.");

    p::separator();
    Ok(())
}

async fn handle_analyze(file: PathBuf) -> Result<()> {
    p::header("Code Analysis");
    p::separator();

    if !file.exists() {
        anyhow::bail!("File not found: {}", file.display());
    }

    let code = std::fs::read_to_string(&file)
        .with_context(|| format!("Failed to read file: {}", file.display()))?;

    let generator = AiTestGenerator::new();
    let analysis = generator.analyze_code(&code)?;

    p::kv("Functions", &analysis.functions.len().to_string());
    p::kv("Structs", &analysis.structs.len().to_string());
    p::kv("Enums", &analysis.enums.len().to_string());
    p::kv("Entry Points", &analysis.entry_points.len().to_string());
    println!();

    if !analysis.functions.is_empty() {
        p::info("Functions:");
        let headers = &["Name", "Visibility", "Mutating"];
        let rows: Vec<Vec<String>> = analysis
            .functions
            .iter()
            .map(|f| {
                vec![
                    f.name.clone(),
                    f.visibility.clone(),
                    if f.is_mutating { "Yes" } else { "No" }.to_string(),
                ]
            })
            .collect();
        p::table(headers, &rows);
    }

    if !analysis.structs.is_empty() {
        println!();
        p::info("Structs:");
        for struct_info in &analysis.structs {
            p::kv(
                &struct_info.name,
                &format!("{} fields", struct_info.fields.len()),
            );
        }
    }

    if !analysis.entry_points.is_empty() {
        println!();
        p::info("Entry Points:");
        for entry_point in &analysis.entry_points {
            p::info(&format!("  - {}", entry_point));
        }
    }

    p::separator();
    Ok(())
}
