use crate::utils::{
    config, contract_testing, print as p, rollback_testing, test_automation, test_coverage,
    test_runner,
};
use anyhow::Result;
use clap::Args;
use sha2::Digest;
use std::path::PathBuf;

#[derive(Args)]
pub struct TestArgs {
    /// Path to the compiled wasm
    #[arg(long)]
    pub wasm: PathBuf,

    /// JSON/TOML contract testing fixture with mocks, scenarios, and assertions
    #[arg(long)]
    pub fixture: Option<PathBuf>,

    /// Path to contract source for generation/coverage
    #[arg(long)]
    pub source: Option<PathBuf>,

    /// Run the rollback safety test harness for a previous/upgraded contract pair
    #[arg(long, default_value = "false")]
    pub rollback: bool,

    /// Path to the previous compiled wasm used as the rollback target
    #[arg(long = "previous-wasm")]
    pub previous_wasm: Option<PathBuf>,

    /// Rollback scenario JSON file. Can be passed multiple times.
    #[arg(long = "rollback-scenario")]
    pub rollback_scenario: Vec<PathBuf>,

    /// Maximum allowed rollback scenario duration in milliseconds
    #[arg(long = "rollback-performance-budget-ms", default_value = "1000")]
    pub rollback_performance_budget_ms: u64,

    /// Collect coverage analysis (requires --source)
    #[arg(long, default_value = "false")]
    pub coverage: bool,

    /// Write a dedicated coverage report to this path
    #[arg(long)]
    pub coverage_out: Option<PathBuf>,

    /// Dedicated coverage report format (html, json, markdown, text)
    #[arg(long, default_value = "html")]
    pub coverage_format: String,

    /// Minimum overall coverage percentage required by --coverage-ci
    #[arg(long)]
    pub coverage_goal: Option<f64>,

    /// Minimum function coverage percentage required by --coverage-ci
    #[arg(long)]
    pub function_coverage_goal: Option<f64>,

    /// Minimum line coverage percentage required by --coverage-ci
    #[arg(long)]
    pub line_coverage_goal: Option<f64>,

    /// Minimum branch coverage percentage required by --coverage-ci
    #[arg(long)]
    pub branch_coverage_goal: Option<f64>,

    /// Fail the command when configured coverage goals are not met
    #[arg(long, default_value = "false")]
    pub coverage_ci: bool,

    /// Generate a GitHub Actions workflow for contract coverage checks
    #[arg(long)]
    pub coverage_ci_workflow_out: Option<PathBuf>,

    /// Auto-generate test cases from source
    #[arg(long, default_value = "false")]
    pub generate: bool,

    /// Run tests in parallel
    #[arg(long, default_value = "false")]
    pub parallel: bool,

    /// Number of parallel workers
    #[arg(long, default_value = "4")]
    pub workers: usize,

    /// Output report format (html, json) — also generates dashboard
    #[arg(long)]
    pub report: Option<String>,

    /// Path to contract source directory for test generation
    #[arg(long)]
    pub contract_path: Option<PathBuf>,

    /// Verify Soroban testnet integration for this run
    #[arg(long, default_value = "false")]
    pub testnet: bool,

    /// Network used by --testnet
    #[arg(long, default_value = "testnet")]
    pub network: String,

    /// Deployed contract ID used by --testnet checks
    #[arg(long)]
    pub contract_id: Option<String>,

    /// Skip the live RPC health probe while still validating testnet wiring
    #[arg(long, default_value = "false")]
    pub testnet_dry_run: bool,

    /// Disable the Soroban RPC health probe during --testnet
    #[arg(long, default_value = "false")]
    pub skip_rpc_health: bool,

    /// Enable AI-driven test optimization (smart ordering, flaky detection, caching)
    #[arg(long, default_value = "false")]
    pub optimize: bool,

    /// Path to write the AI optimization report
    #[arg(long)]
    pub optimize_out: Option<PathBuf>,

    /// Flaky test detection threshold (0-100)
    #[arg(long, default_value = "30.0")]
    pub flaky_threshold: f64,

    /// Enable test deduplication analysis
    #[arg(long, default_value = "false")]
    pub dedup: bool,

    /// Enable test result caching
    #[arg(long, default_value = "false")]
    pub cache: bool,

    /// Enable resource-aware scheduling
    #[arg(long, default_value = "false")]
    pub resource_aware: bool,

    /// Memory limit per test worker (MB)
    #[arg(long, default_value = "512")]
    pub memory_limit_mb: u64,

    /// Maximum concurrency for resource-aware scheduling
    #[arg(long, default_value = "8")]
    pub max_concurrency: usize,

    /// Enable performance analysis and reporting
    #[arg(long, default_value = "false")]
    pub perf_analysis: bool,

    /// Generate HTML optimization report
    #[arg(long, default_value = "false")]
    pub optimize_html: bool,
}

pub async fn handle(args: TestArgs) -> Result<()> {
    let coverage_goals = build_coverage_goals(&args)?;
    let coverage_requested = args.coverage
        || args.coverage_out.is_some()
        || args.coverage_ci
        || coverage_goals.has_goals();

    config::validate_file_path(&args.wasm, Some("wasm"))?;
    if let Some(fixture) = &args.fixture {
        config::validate_file_path(fixture, None)?;
    }
    if let Some(contract_id) = &args.contract_id {
        config::validate_contract_id(contract_id)?;
    }
    if coverage_requested && args.source.is_none() {
        anyhow::bail!("coverage analysis requires --source");
    }
    if args.generate && args.source.is_none() {
        anyhow::bail!("--generate requires --source");
    }
    if args.coverage_ci_workflow_out.is_some() && args.source.is_none() {
        anyhow::bail!("--coverage-ci-workflow-out requires --source");
    }

    if let Some(workflow_out) = &args.coverage_ci_workflow_out {
        let source = args.source.as_ref().expect("source checked above");
        let path = test_coverage::write_coverage_ci_workflow(
            workflow_out,
            &args.wasm,
            source,
            &coverage_goals,
        )?;
        p::kv("Coverage CI workflow", &path.display().to_string());
    }

    p::header("Contract Test Runner");
    p::kv("Wasm", &args.wasm.display().to_string());
    p::kv("Coverage", if coverage_requested { "yes" } else { "no" });
    p::kv("Generate", if args.generate { "yes" } else { "no" });
    p::kv("Parallel", if args.parallel { "yes" } else { "no" });
    if let Some(fixture) = &args.fixture {
        p::kv("Fixture", &fixture.display().to_string());
    }
    if args.testnet {
        p::kv("Testnet integration", "yes");
        p::kv("Network", &args.network);
    }
    if let Some(r) = &args.report {
        p::kv("Report", r);
    }
    p::kv("Generate tests", if args.generate { "yes" } else { "no" });
    p::kv(
        "Parallel execution",
        if args.parallel { "yes" } else { "no" },
    );
    if args.parallel {
        p::kv("Workers", &args.workers.to_string());
    }
    if args.rollback {
        p::kv("Rollback harness", "enabled");
        p::kv(
            "Rollback scenarios",
            if args.rollback_scenario.is_empty() {
                "default"
            } else {
                "custom"
            },
        );
    }

    if args.rollback {
        let previous_wasm = args.previous_wasm.clone().ok_or_else(|| {
            anyhow::anyhow!("--rollback requires --previous-wasm <path-to-previous.wasm>")
        })?;
        config::validate_file_path(&previous_wasm, Some("wasm"))?;
        for scenario in &args.rollback_scenario {
            config::validate_file_path(scenario, Some("json"))?;
        }

        p::info("Running contract rollback safety harness...");
        let report = rollback_testing::run_rollback_tests(rollback_testing::RollbackTestOptions {
            previous_wasm,
            upgraded_wasm: args.wasm.clone(),
            scenario_paths: args.rollback_scenario.clone(),
            performance_budget_ms: args.rollback_performance_budget_ms,
            report_format: args.report.clone(),
        })?;

        println!();
        p::separator();
        p::kv_accent("Previous SHA256", &report.previous_wasm_hash);
        p::kv_accent("Upgraded SHA256", &report.upgraded_wasm_hash);
        p::kv("Rollback scenarios", &report.total_scenarios.to_string());
        p::kv("Passed", &report.passed.to_string());
        p::kv("Failed", &report.failed.to_string());
        p::kv("Duration", &format!("{}ms", report.total_duration_ms));
        if let Some(path) = &report.report_path {
            p::kv("Rollback report", &path.display().to_string());
        }

        for scenario in &report.scenario_results {
            println!();
            p::kv(
                &format!("Scenario {}", scenario.scenario_name),
                if scenario.passed { "pass" } else { "fail" },
            );
            for check in &scenario.checks {
                let marker = if check.passed { "✓" } else { "✗" };
                println!("  {} {:?}: {}", marker, check.category, check.message);
            }
        }
        p::separator();

        if report.failed > 0 {
            anyhow::bail!("Rollback safety checks failed");
        }

        p::success("Rollback safety checks passed");
        return Ok(());
    }

    if let Some(fixture) = &args.fixture {
        let mut report = contract_testing::run_contract_framework(
            &args.wasm,
            fixture,
            contract_testing::FrameworkRunOptions {
                coverage: coverage_requested,
                report_format: args.report.clone(),
                source: args.source.clone(),
                testnet: build_testnet_config(&args),
            },
        )
        .await?;

        let coverage_goals_passed =
            handle_coverage_outputs(report.coverage.as_mut(), &args, &coverage_goals)?;
        print_framework_report(&report);

        if report.failures > 0 {
            anyhow::bail!("Some contract framework tests failed");
        }
        if args.coverage_ci && !coverage_goals_passed {
            anyhow::bail!("Coverage goals were not met");
        }

        p::success("All contract framework tests passed");
        return Ok(());
    }

    // Handle automated test generation
    if args.generate {
        let source = args.source.as_ref().expect("source checked above");
        p::info("Generating comprehensive contract test cases...");
        let generated = crate::utils::test_generator::generate_from_source(source)?;

        let project_path = args
            .contract_path
            .clone()
            .or_else(|| {
                source
                    .parent()
                    .and_then(|path| path.parent())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from("."));
        let tests_dir = project_path.join("tests");
        std::fs::create_dir_all(&tests_dir)?;
        let generated_path = tests_dir.join("starforge_generated.rs");
        crate::utils::test_generator::write_generated_tests(&generated, &generated_path)?;
        p::kv("Rust tests saved", &generated_path.display().to_string());

        if let Some(contract_path) = &args.contract_path {
            let generator = test_automation::TestCaseGenerator::new(contract_path.clone());
            let suite = generator.generate_from_contract()?;

            p::success(&format!("Generated {} test cases", suite.test_cases.len()));

            // Save test suite
            let suite_path = contract_path.join("test_suite.json");
            let json = serde_json::to_string_pretty(&suite)?;
            std::fs::write(&suite_path, json)?;
            p::kv("Test suite saved", &suite_path.display().to_string());
        } else {
            p::success(&format!("Generated {} test cases", generated.cases.len()));
        }
    }

    // Run tests with automation if parallel is enabled
    if args.parallel {
        if let Some(contract_path) = &args.contract_path {
            let suite_path = contract_path.join("test_suite.json");
            if suite_path.exists() {
                let suite_content = std::fs::read_to_string(&suite_path)?;
                let suite: test_automation::TestSuite = serde_json::from_str(&suite_content)?;

                p::info("Running tests in parallel...");
                let runner = test_automation::ParallelTestRunner::new(args.workers);
                let report = runner.run_tests(&suite, &args.wasm)?;

                // Export report
                if let Some(report_format) = &args.report {
                    let report_path = match report_format.as_str() {
                        "html" => PathBuf::from("test_report.html"),
                        "json" => PathBuf::from("test_report.json"),
                        "junit" => PathBuf::from("test_report.xml"),
                        _ => PathBuf::from("test_report.html"),
                    };

                    match report_format.as_str() {
                        "html" => {
                            test_automation::TestReportExporter::export_html(&report, &report_path)?
                        }
                        "json" => {
                            test_automation::TestReportExporter::export_json(&report, &report_path)?
                        }
                        "junit" => test_automation::TestReportExporter::export_junit(
                            &report,
                            &report_path,
                        )?,
                        _ => {
                            test_automation::TestReportExporter::export_html(&report, &report_path)?
                        }
                    }

                    p::kv("Report saved", &report_path.display().to_string());
                }

                println!();
                p::separator();
                p::kv("Total tests", &report.total_tests.to_string());
                p::kv("Passed", &report.passed.to_string());
                p::kv("Failed", &report.failed.to_string());
                p::kv(
                    "Coverage",
                    &format!(
                        "{}%",
                        if report.coverage_summary.lines_total > 0 {
                            (report.coverage_summary.lines_covered as f64
                                / report.coverage_summary.lines_total as f64
                                * 100.0) as u32
                        } else {
                            0
                        }
                    ),
                );
                p::kv("Duration", &format!("{}ms", report.total_duration_ms));
                p::separator();

                if report.failed > 0 {
                    anyhow::bail!("Some contract tests failed");
                }

                p::success("All contract tests passed");
                return Ok(());
            }
        }
    }

    // ── AI-Driven Test Optimization ─────────────────────────────────────────
    let optimization_requested = args.optimize
        || args.optimize_out.is_some()
        || args.dedup
        || args.cache
        || args.resource_aware
        || args.perf_analysis
        || args.optimize_html;

    if optimization_requested || args.parallel {
        let wasm_bytes = std::fs::read(&args.wasm)?;
        let wasm_hash = hex::encode(sha2::Sha256::digest(&wasm_bytes));
        let mut optimizer = crate::utils::test_optimizer::TestOptimizer::new()?;

        // Build test name list
        let source_tests = if let Some(source) = &args.source {
            if args.generate {
                let gen = crate::utils::test_generator::generate_from_source(source)?;
                gen.cases.iter().map(|c| c.name.clone()).collect::<Vec<_>>()
            } else {
                vec![
                    "wasm_header_valid".into(),
                    "wasm_size_reasonable".into(),
                    "exports_present".into(),
                ]
            }
        } else {
            vec![
                "wasm_header_valid".into(),
                "wasm_size_reasonable".into(),
                "exports_present".into(),
            ]
        };

        // Apply smart ordering
        let ordered_tests = if args.optimize {
            let optimized = optimizer.optimize_order(&source_tests);
            p::info(&format!(
                "AI optimizer: reordered {} tests (security-critical tests first)",
                optimized.len()
            ));
            optimized
        } else {
            source_tests.clone()
        };

        // Check cache
        let mut cached_count = 0u32;
        if args.cache {
            for test_name in &ordered_tests {
                if let Some(_cached) = optimizer.check_cache(&wasm_hash, test_name) {
                    cached_count += 1;
                }
            }
            if cached_count > 0 {
                p::kv("Cache hits", &cached_count.to_string());
            }
        }

        // Run tests with optimization tracking
        let timing_results: Vec<_> = if args.parallel {
            p::info("Running optimized tests in parallel...");
            let runner = test_automation::ParallelTestRunner::new(args.workers);
            if let Some(contract_path) = &args.contract_path {
                let suite_path = contract_path.join("test_suite.json");
                if suite_path.exists() {
                    let suite_content = std::fs::read_to_string(&suite_path)?;
                    let suite: test_automation::TestSuite = serde_json::from_str(&suite_content)?;
                    let report = runner.run_tests(&suite, &args.wasm)?;

                    // Record results for flaky detection
                    for result in &report.results {
                        let passed = matches!(result.status, test_automation::TestStatus::Passed);
                        optimizer.record_result(&result.test_name, passed, result.duration_ms)?;
                        if args.cache {
                            optimizer.update_cache(
                                &wasm_hash,
                                &result.test_name,
                                passed,
                                result.duration_ms,
                            )?;
                        }
                    }

                    let _timings: Vec<crate::utils::test_optimizer::TestCaseTiming> = report
                        .results
                        .iter()
                        .map(|r| crate::utils::test_optimizer::TestCaseTiming {
                            name: r.test_name.clone(),
                            duration_ms: r.duration_ms,
                            passed: matches!(r.status, test_automation::TestStatus::Passed),
                        })
                        .collect::<Vec<_>>();

                    // Export report
                    if let Some(report_format) = &args.report {
                        let report_path = match report_format.as_str() {
                            "html" => PathBuf::from("test_report.html"),
                            "json" => PathBuf::from("test_report.json"),
                            "junit" => PathBuf::from("test_report.xml"),
                            _ => PathBuf::from("test_report.html"),
                        };
                        match report_format.as_str() {
                            "html" => test_automation::TestReportExporter::export_html(
                                &report,
                                &report_path,
                            )?,
                            "json" => test_automation::TestReportExporter::export_json(
                                &report,
                                &report_path,
                            )?,
                            "junit" => test_automation::TestReportExporter::export_junit(
                                &report,
                                &report_path,
                            )?,
                            _ => test_automation::TestReportExporter::export_html(
                                &report,
                                &report_path,
                            )?,
                        }
                        p::kv("Report saved", &report_path.display().to_string());
                    }

                    println!();
                    p::separator();
                    p::kv("Total tests", &report.total_tests.to_string());
                    p::kv("Passed", &report.passed.to_string());
                    p::kv("Failed", &report.failed.to_string());
                    p::kv(
                        "Coverage",
                        &format!(
                            "{}%",
                            if report.coverage_summary.lines_total > 0 {
                                (report.coverage_summary.lines_covered as f64
                                    / report.coverage_summary.lines_total as f64
                                    * 100.0) as u32
                            } else {
                                0
                            }
                        ),
                    );
                    p::kv("Duration", &format!("{}ms", report.total_duration_ms));
                    p::separator();

                    if report.failed > 0 {
                        anyhow::bail!("Some contract tests failed");
                    }

                    p::success("All contract tests passed");
                    return Ok(());
                }
            }
            // Fallback parallel execution
            let cases: Vec<String> = ordered_tests.clone();
            let results = test_runner::run_parallel(&cases, args.workers)?;
            for r in &results {
                optimizer.record_result(&r.name, r.passed, r.duration_ms)?;
                if args.cache {
                    optimizer.update_cache(&wasm_hash, &r.name, r.passed, r.duration_ms)?;
                }
            }
            results
                .iter()
                .map(|r| crate::utils::test_optimizer::TestCaseTiming {
                    name: r.name.clone(),
                    duration_ms: r.duration_ms,
                    passed: r.passed,
                })
                .collect::<Vec<_>>()
        } else {
            // Sequential run with tracking
            let cases: Vec<String> = ordered_tests.clone();
            let results = test_runner::run_sequential(&cases)?;
            for r in &results {
                optimizer.record_result(&r.name, r.passed, r.duration_ms)?;
                if args.cache {
                    optimizer.update_cache(&wasm_hash, &r.name, r.passed, r.duration_ms)?;
                }
            }
            results
                .iter()
                .map(|r| crate::utils::test_optimizer::TestCaseTiming {
                    name: r.name.clone(),
                    duration_ms: r.duration_ms,
                    passed: r.passed,
                })
                .collect::<Vec<_>>()
        };

        // Generate optimization report
        if args.optimize || args.perf_analysis || args.optimize_out.is_some() || args.optimize_html
        {
            let generated_cases = if let Some(source) = &args.source {
                if args.generate {
                    crate::utils::test_generator::generate_from_source(source)
                        .map(|g| g.cases)
                        .unwrap_or_default()
                } else {
                    vec![]
                }
            } else {
                vec![]
            };

            let opt_report = optimizer.optimize_all(
                &wasm_hash,
                &source_tests,
                &generated_cases,
                &timing_results,
            );

            // Log optimization insights
            println!();
            p::header("AI Test Optimization Results");

            for flaky in &opt_report.flaky_tests {
                p::warn(&format!(
                    "Flaky test '{}' (score: {:.1}, failure rate: {:.1}%)",
                    flaky.test_name,
                    flaky.flakiness_score,
                    flaky.failure_rate * 100.0
                ));
            }

            if !opt_report.duplicate_tests.is_empty() {
                for dup in &opt_report.duplicate_tests {
                    p::warn(&format!(
                        "Duplicate pair: '{}' <-> '{}' (similarity: {:.0}%)",
                        dup.test_a,
                        dup.test_b,
                        dup.similarity_score * 100.0
                    ));
                }
            }

            p::kv(
                "Estimated improvement",
                &format!("{:.1}%", opt_report.estimated_improvement_pct),
            );
            p::kv(
                "Slowest test",
                &opt_report
                    .performance
                    .slowest_tests
                    .first()
                    .cloned()
                    .unwrap_or_default(),
            );
            p::kv(
                "Parallel efficiency",
                &format!("{:.1}%", opt_report.performance.parallel_efficiency),
            );

            // Export report
            if let Some(out_path) = &args.optimize_out {
                crate::utils::test_optimizer::export_optimization_report(&opt_report, out_path)?;
                p::kv("Optimization report", &out_path.display().to_string());
            }

            if args.optimize_html {
                let html_path = PathBuf::from("ai_test_optimization_report.html");
                let html =
                    crate::utils::test_optimizer::render_optimization_html_report(&opt_report);
                std::fs::write(&html_path, html)?;
                p::kv("HTML report", &html_path.display().to_string());
            }

            p::separator();
        }

        if !timing_results.iter().all(|r| r.passed) {
            anyhow::bail!("Some contract tests failed");
        }

        p::success("All contract tests passed (optimized)");
        return Ok(());
    }

    // Fall back to original test runner
    let mut result = test_runner::run_contract_tests(
        &args.wasm,
        test_runner::TestOptions {
            coverage: coverage_requested,
            report_format: args.report.clone(),
            parallel: args.parallel,
            generate: args.generate,
            source: args.source.clone(),
            workers: args.workers,
        },
    )?;
    let coverage_goals_passed =
        handle_coverage_outputs(result.coverage.as_mut(), &args, &coverage_goals)?;

    println!();
    p::separator();
    p::kv_accent("SHA256", &result.sha256);
    p::kv("Wasm bytes", &result.size_bytes.to_string());
    p::kv("Cases executed", &result.cases_executed.to_string());
    p::kv("Failures", &result.failures.to_string());
    p::kv("Generated cases", &result.generated_cases.len().to_string());

    if let Some(cov) = &result.coverage {
        print_coverage_summary(cov);
    }
    if let Some(path) = &result.report_path {
        p::kv("Report path", &path.display().to_string());
    }
    if let Some(path) = &result.dashboard_path {
        p::kv("Dashboard", &path.display().to_string());
    }

    if args.testnet {
        let status = contract_testing::verify_testnet_integration(
            build_testnet_config(&args).expect("testnet config available"),
        )
        .await?;
        println!();
        p::header("Testnet Integration");
        p::kv("Network", &status.network);
        p::kv("RPC", &status.rpc_url);
        p::kv("RPC healthy", if status.rpc_healthy { "yes" } else { "no" });
        for check in &status.checks {
            p::info(check);
        }
    }

    if !result.failure_analysis.is_empty() {
        println!();
        p::header("Failure Analysis");
        for fa in &result.failure_analysis {
            println!("  {} [{}]: {}", fa.test_name, fa.category, fa.suggestion);
        }
    }

    p::separator();

    if result.failures > 0 {
        anyhow::bail!("Some contract tests failed");
    }
    if args.coverage_ci && !coverage_goals_passed {
        anyhow::bail!("Coverage goals were not met");
    }

    p::success("All contract tests passed");
    Ok(())
}

fn build_coverage_goals(args: &TestArgs) -> Result<test_coverage::CoverageGoals> {
    Ok(test_coverage::CoverageGoals {
        min_overall: validate_coverage_goal("--coverage-goal", args.coverage_goal)?,
        min_functions: validate_coverage_goal(
            "--function-coverage-goal",
            args.function_coverage_goal,
        )?,
        min_lines: validate_coverage_goal("--line-coverage-goal", args.line_coverage_goal)?,
        min_branches: validate_coverage_goal("--branch-coverage-goal", args.branch_coverage_goal)?,
    })
}

fn validate_coverage_goal(name: &str, value: Option<f64>) -> Result<Option<f64>> {
    if let Some(value) = value {
        if !(0.0..=100.0).contains(&value) {
            anyhow::bail!("{} must be between 0 and 100", name);
        }
    }
    Ok(value)
}

fn handle_coverage_outputs(
    coverage: Option<&mut test_coverage::CoverageReport>,
    args: &TestArgs,
    goals: &test_coverage::CoverageGoals,
) -> Result<bool> {
    let Some(coverage) = coverage else {
        if args.coverage_out.is_some() || args.coverage_ci || goals.has_goals() {
            anyhow::bail!("Coverage was requested, but no coverage data was produced");
        }
        return Ok(true);
    };

    let mut goals_passed = true;
    if goals.has_goals() {
        let goal_result = test_coverage::apply_coverage_goals(coverage, goals.clone());
        goals_passed = goal_result.passed;
        p::kv(
            "Coverage goals",
            if goal_result.passed {
                "passed"
            } else {
                "failed"
            },
        );
        for violation in &goal_result.violations {
            p::warn(violation);
        }
    }

    if let Some(output) = &args.coverage_out {
        let path = test_coverage::write_coverage_report(coverage, &args.coverage_format, output)?;
        p::kv("Coverage report", &path.display().to_string());
    }

    Ok(goals_passed)
}

fn print_coverage_summary(cov: &test_coverage::CoverageReport) {
    p::kv("Coverage", &format!("{:.1}%", cov.coverage_percent));
    p::kv(
        "Function coverage",
        &format!(
            "{:.1}% ({}/{})",
            cov.function_coverage_percent, cov.functions_covered, cov.functions_total
        ),
    );
    p::kv(
        "Line coverage",
        &format!(
            "{:.1}% ({}/{})",
            cov.line_coverage_percent, cov.lines_covered, cov.lines_total
        ),
    );
    p::kv(
        "Branch coverage",
        &format!(
            "{:.1}% ({}/{})",
            cov.branch_coverage_percent, cov.branches_covered, cov.branches_total
        ),
    );
    if !cov.uncovered_functions.is_empty() {
        p::warn(&format!(
            "Uncovered functions: {}",
            cov.uncovered_functions.join(", ")
        ));
    }
}

fn build_testnet_config(args: &TestArgs) -> Option<contract_testing::TestnetIntegrationConfig> {
    args.testnet
        .then(|| contract_testing::TestnetIntegrationConfig {
            network: args.network.clone(),
            contract_id: args.contract_id.clone(),
            verify_rpc_health: !args.skip_rpc_health,
            dry_run: args.testnet_dry_run,
        })
}

fn print_framework_report(report: &contract_testing::ContractFrameworkReport) {
    println!();
    p::separator();
    p::kv_accent("SHA256", &report.wasm_sha256);
    p::kv("Suite", &report.suite_name);
    p::kv("Wasm bytes", &report.size_bytes.to_string());
    p::kv("Fixtures", &report.fixtures_loaded.to_string());
    p::kv("Mocks", &report.mocks_available.to_string());
    p::kv("Cases executed", &report.cases_executed.to_string());
    p::kv("Failures", &report.failures.to_string());

    if let Some(coverage) = &report.coverage {
        print_coverage_summary(coverage);
    }
    if let Some(path) = &report.report_path {
        p::kv("Report path", &path.display().to_string());
    }
    if let Some(status) = &report.testnet {
        p::kv("Testnet RPC", &status.rpc_url);
        p::kv("RPC healthy", if status.rpc_healthy { "yes" } else { "no" });
    }

    for case in &report.cases {
        let status = if case.passed { "PASS" } else { "FAIL" };
        p::kv(&format!("Case {}", case.name), status);
        for error in &case.errors {
            p::warn(error);
        }
    }

    p::separator();
}
