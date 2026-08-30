//! Integration tests for the AI-driven test optimization pipeline.
//!
//! These tests exercise the full optimizer: ordering, flaky detection,
//! deduplication, caching, performance analysis, resource scheduling,
//! failure pattern analysis, and report generation.

use std::path::PathBuf;

use starforge::utils::test_generator::GeneratedTestCase;
use starforge::utils::test_optimizer::*;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_optimizer() -> TestOptimizer {
    let dir = tempfile::tempdir().unwrap().keep();
    TestOptimizer::with_config_dir(dir).unwrap()
}

fn make_history(
    name: &str,
    total: u32,
    failures: u32,
    passes: u32,
    flaky: u32,
    avg_ms: f64,
    last_status: &str,
) -> (String, TestHistory) {
    (
        name.to_string(),
        TestHistory {
            name: name.to_string(),
            total_runs: total,
            failures,
            passes,
            flaky_count: flaky,
            avg_duration_ms: avg_ms,
            max_duration_ms: avg_ms as u64 * 2,
            min_duration_ms: (avg_ms / 2.0) as u64,
            last_run_duration_ms: avg_ms as u64,
            last_status: last_status.to_string(),
            last_run: "2026-01-01T00:00:00Z".to_string(),
            consecutive_failures: 0,
            consecutive_passes: passes,
        },
    )
}

fn make_timing(name: &str, ms: u64, passed: bool) -> TestCaseTiming {
    TestCaseTiming {
        name: name.to_string(),
        duration_ms: ms,
        passed,
    }
}

fn make_generated(name: &str, func: &str, test_type: &str) -> GeneratedTestCase {
    GeneratedTestCase {
        name: name.to_string(),
        description: format!("Test {} for {}", name, func),
        function: func.to_string(),
        test_type: test_type.to_string(),
        input_data: vec![],
        expected_behavior: "works".to_string(),
        security_checks: vec![],
    }
}

// ── Integration: Full Optimization Pipeline ──────────────────────────────────

#[test]
fn test_full_optimization_pipeline_with_history() {
    let mut opt = make_optimizer();

    // Simulate 10 test runs with varying history
    let test_names: Vec<String> = vec![
        "test_security_auth".into(),
        "test_wallet_e2e".into(),
        "test_smoke_connectivity".into(),
        "test_perf_benchmark".into(),
        "test_property_invariant".into(),
        "test_integration_rollback".into(),
    ];

    // Populate history with realistic patterns
    opt.history.extend([
        make_history("test_security_auth", 20, 5, 15, 3, 300.0, "pass"),
        make_history("test_wallet_e2e", 15, 8, 7, 6, 1200.0, "fail"),
        make_history("test_smoke_connectivity", 25, 1, 24, 1, 50.0, "pass"),
        make_history("test_perf_benchmark", 10, 2, 8, 2, 5000.0, "pass"),
        make_history("test_property_invariant", 30, 0, 30, 0, 200.0, "pass"),
        make_history("test_integration_rollback", 8, 4, 4, 4, 800.0, "fail"),
    ]);

    // Check ordering: flaky/failing tests should come first
    let ordered = opt.optimize_order(&test_names);
    assert_eq!(ordered.len(), test_names.len());
    assert!(
        ordered.iter().position(|t| t == "test_wallet_e2e")
            < ordered.iter().position(|t| t == "test_property_invariant"),
        "Failing tests should be prioritized over stable tests"
    );

    // Check that security tests are highly prioritized
    assert!(
        ordered.iter().position(|t| t == "test_security_auth")
            < ordered.iter().position(|t| t == "test_perf_benchmark"),
        "Security tests should be prioritized over performance tests"
    );

    // Flaky detection
    let flaky = opt.detect_flaky_tests(30.0);
    assert!(!flaky.is_empty(), "Should detect flaky tests");
    let wallet_flaky = flaky.iter().find(|f| f.test_name == "test_wallet_e2e");
    assert!(
        wallet_flaky.is_some(),
        "wallet_e2e should be flagged as flaky"
    );
    assert!(wallet_flaky.unwrap().flakiness_score >= 30.0);

    // Performance analysis
    let timings = vec![
        make_timing("test_security_auth", 300, true),
        make_timing("test_wallet_e2e", 1200, false),
        make_timing("test_smoke_connectivity", 50, true),
        make_timing("test_perf_benchmark", 5000, true),
        make_timing("test_property_invariant", 200, true),
        make_timing("test_integration_rollback", 800, false),
    ];
    let perf = opt.analyze_performance(&timings);
    assert_eq!(perf.total_tests, 6);
    assert_eq!(perf.total_duration_ms, 7550);
    assert_eq!(perf.slowest_tests[0], "test_perf_benchmark");
    assert!((perf.avg_duration_ms - 1258.3).abs() < 1.0);
    assert!(perf.parallel_efficiency > 0.0);

    // Failure pattern analysis
    let patterns = opt.analyze_failure_patterns();
    assert_eq!(patterns.total_failing_tests, 5);
    assert!(!patterns.categories.is_empty());

    // Full report
    let generated = vec![
        make_generated("test_security_auth", "authenticate", "security"),
        make_generated("test_wallet_e2e", "transfer", "integration"),
    ];
    let report = opt.optimize_all("wasm_hash_123", &test_names, &generated, &timings);
    assert_eq!(report.wasm_hash, "wasm_hash_123");
    assert!(report.estimated_improvement_pct > 0.0);
    assert!(!report.flaky_tests.is_empty());
    assert!(!report.performance.slowest_tests.is_empty());
    assert!(report.failure_patterns.total_failing_tests > 0);
}

// ── Integration: Flaky Detection Lifecycle ────────────────────────────────────

#[test]
fn test_flaky_detection_lifecycle() {
    let mut opt = make_optimizer();

    // Simulate a test that alternates pass/fail over many runs
    let patterns = [
        (true, 100),
        (false, 110),
        (true, 95),
        (false, 105),
        (true, 100),
        (false, 115),
        (true, 90),
        (false, 108),
        (true, 102),
        (false, 112),
        (true, 98),
        (false, 107),
        (true, 101),
        (false, 109),
        (true, 99),
        (false, 106),
        (true, 100),
        (false, 111),
        (true, 97),
        (false, 104),
    ];

    for (i, &(passed, ms)) in patterns.iter().enumerate() {
        opt.record_result("test_alternating", passed, ms)
            .expect("record_result should succeed");

        // After first run, history should have 1 entry
        if i == 0 {
            let h = opt.history.get("test_alternating").unwrap();
            assert_eq!(h.total_runs, 1);
        }
    }

    let h = opt.history.get("test_alternating").unwrap();
    assert_eq!(h.total_runs, 20);
    assert_eq!(h.passes, 10);
    assert_eq!(h.failures, 10);
    assert!(
        h.flaky_count > 0,
        "Alternating test should have flaky transitions"
    );

    // Detect flaky tests
    let flaky = opt.detect_flaky_tests(30.0);
    let alt = flaky.iter().find(|f| f.test_name == "test_alternating");
    assert!(
        alt.is_some(),
        "Alternating test should be detected as flaky"
    );
    assert!(alt.unwrap().flakiness_score > 50.0);
}

// ── Integration: Cache System ────────────────────────────────────────────────

#[test]
fn test_cache_system_integration() {
    let mut opt = make_optimizer();

    // Cache lookups before any data
    assert!(opt.check_cache("wasm_a", "test_A").is_none());
    assert!(opt.check_cache("wasm_a", "test_B").is_none());

    // Update cache
    opt.update_cache("wasm_a", "test_A", true, 100)
        .expect("update_cache failed");
    opt.update_cache("wasm_a", "test_B", false, 200)
        .expect("update_cache failed");
    opt.update_cache("wasm_b", "test_A", true, 150)
        .expect("update_cache failed");

    // Verify cached values
    assert_eq!(opt.check_cache("wasm_a", "test_A"), Some(true));
    assert_eq!(opt.check_cache("wasm_a", "test_B"), Some(false));
    assert_eq!(opt.check_cache("wasm_b", "test_A"), Some(true));
    assert!(opt.check_cache("wasm_b", "test_B").is_none());

    // Cache stats
    let stats = opt.get_cache_stats();
    assert_eq!(stats.cache_size, 2);
}

// ── Integration: Resource-Aware Scheduling ───────────────────────────────────

#[test]
fn test_resource_aware_scheduling() {
    let opt = make_optimizer();

    let tests: Vec<OptimizedTestCase> = vec![
        OptimizedTestCase {
            name: "high_mem_test".into(),
            priority: TestPriority::Critical,
            estimated_duration_ms: 1000,
            failure_probability: 0.1,
            coverage_impact: 0.9,
            dependencies: vec![],
            resource_profile: ResourceProfile {
                memory_mb: 1024,
                cpu_intensity: 0.8,
                io_intensity: 0.1,
            },
        },
        OptimizedTestCase {
            name: "low_mem_test".into(),
            priority: TestPriority::Low,
            estimated_duration_ms: 50,
            failure_probability: 0.0,
            coverage_impact: 0.1,
            dependencies: vec![],
            resource_profile: ResourceProfile {
                memory_mb: 64,
                cpu_intensity: 0.1,
                io_intensity: 0.1,
            },
        },
        OptimizedTestCase {
            name: "medium_mem_test".into(),
            priority: TestPriority::High,
            estimated_duration_ms: 500,
            failure_probability: 0.05,
            coverage_impact: 0.7,
            dependencies: vec![],
            resource_profile: ResourceProfile {
                memory_mb: 256,
                cpu_intensity: 0.5,
                io_intensity: 0.3,
            },
        },
        OptimizedTestCase {
            name: "io_bound_test".into(),
            priority: TestPriority::Medium,
            estimated_duration_ms: 300,
            failure_probability: 0.02,
            coverage_impact: 0.5,
            dependencies: vec![],
            resource_profile: ResourceProfile {
                memory_mb: 128,
                cpu_intensity: 0.2,
                io_intensity: 0.9,
            },
        },
    ];

    // Schedule with tight constraints
    let batches_tight = opt.schedule_with_resources(&tests, 2, 512);
    assert_eq!(batches_tight.len(), 3);
    let total: usize = batches_tight.iter().map(|b| b.len()).sum();
    assert_eq!(total, tests.len());

    // Schedule with loose constraints
    let batches_loose = opt.schedule_with_resources(&tests, 8, 4096);
    assert_eq!(batches_loose.len(), 1);

    // Profile-based batching
    let profile_batches = opt.batch_tests_by_profile(&tests);
    let profile_total: usize = profile_batches.iter().map(|b| b.len()).sum();
    assert_eq!(profile_total, tests.len());
}

// ── Integration: Deduplication ───────────────────────────────────────────────

#[test]
fn test_dedup_with_similar_tests() {
    let opt = make_optimizer();

    let cases = vec![
        make_generated("test_init_basic", "initialize", "happy_path"),
        make_generated("test_init_duplicate", "initialize", "happy_path"),
        make_generated("test_init_edge", "initialize", "edge_case"),
        make_generated("test_transfer_basic", "transfer", "happy_path"),
        make_generated("test_transfer_duplicate", "transfer", "happy_path"),
        make_generated("test_balance_check", "check_balance", "happy_path"),
    ];

    let duplicates = opt.find_duplicate_tests(&cases);
    assert!(!duplicates.is_empty(), "Should find duplicate tests");

    // The init and transfer pairs should be detected
    let init_dupes: Vec<_> = duplicates
        .iter()
        .filter(|d| d.test_a.contains("init") && d.test_b.contains("init"))
        .collect();
    assert!(
        !init_dupes.is_empty(),
        "Init tests should be flagged as duplicates"
    );

    let transfer_dupes: Vec<_> = duplicates
        .iter()
        .filter(|d| d.test_a.contains("transfer") && d.test_b.contains("transfer"))
        .collect();
    assert!(
        !transfer_dupes.is_empty(),
        "Transfer tests should be flagged as duplicates"
    );

    // Unique tests should not be in duplicate pairs
    let balance_dupes: Vec<_> = duplicates
        .iter()
        .filter(|d| d.test_a == "test_balance_check" || d.test_b == "test_balance_check")
        .collect();
    assert!(
        balance_dupes.is_empty(),
        "Unique test should not be duplicated"
    );
}

// ── Integration: Parallel Batch Suggestion ───────────────────────────────────

#[test]
fn test_parallel_batch_suggestion_with_ordering() {
    let opt = make_optimizer();

    let test_names: Vec<String> = (0..12).map(|i| format!("test_case_{}", i)).collect();

    // Suggest batches for 4 workers
    let batches = opt.suggest_parallel_batches(&test_names, 4);
    assert_eq!(batches.len(), 4);
    let total: usize = batches.iter().map(|b| b.len()).sum();
    assert_eq!(total, test_names.len());

    // Each batch should be roughly equal in size
    let sizes: Vec<usize> = batches.iter().map(|b| b.len()).collect();
    let max_size = sizes.iter().max().unwrap();
    let min_size = sizes.iter().min().unwrap();
    assert!(
        max_size - min_size <= 1,
        "Batch sizes should be roughly equal, got {:?}",
        sizes
    );
}

// ── Integration: Report Generation ───────────────────────────────────────────

#[test]
fn test_report_generation_and_export() {
    let mut opt = make_optimizer();

    // Add some history
    opt.history.extend([
        make_history("test_a", 10, 2, 8, 1, 100.0, "pass"),
        make_history("test_b", 5, 3, 2, 3, 500.0, "fail"),
    ]);

    let test_names = vec!["test_a".into(), "test_b".into()];
    let generated = vec![make_generated("test_a", "func1", "happy_path")];
    let timings = vec![
        make_timing("test_a", 100, true),
        make_timing("test_b", 500, false),
    ];

    let report = opt.optimize_all("hash_xyz", &test_names, &generated, &timings);
    assert!(!report.optimized_order.is_empty());

    // Export to temp file
    let dir = tempfile::tempdir().expect("temp dir");
    let json_path = dir.path().join("optimization_report.json");
    let exported = export_optimization_report(&report, &json_path);
    assert!(
        exported.is_ok(),
        "Export should succeed: {:?}",
        exported.err()
    );
    assert!(json_path.exists(), "Report file should exist");

    // Verify JSON content
    let content = std::fs::read_to_string(&json_path).expect("read report");
    assert!(content.contains("hash_xyz"));
    assert!(content.contains("test_a"));
    assert!(content.contains("test_b"));

    // Verify HTML report generation
    let html = render_optimization_html_report(&report);
    assert!(html.contains("AI Test Optimization Report"));
    assert!(html.contains("hash_xyz"));
    assert!(html.contains("test_a"));
    assert!(html.contains("</html>"));
}

// ── Integration: Result Recording Consistency ────────────────────────────────

#[test]
fn test_result_recording_consistency() {
    let mut opt = make_optimizer();

    // Record multiple results for the same test
    for i in 0..50 {
        let passed = i % 3 != 0;
        let ms = (i as u64 + 1) * 10;
        opt.record_result("test_consistent", passed, ms)
            .expect("record_result should succeed");
    }

    let h = opt.history.get("test_consistent").unwrap();
    assert_eq!(h.total_runs, 50);
    assert_eq!(h.passes, 33);
    assert_eq!(h.failures, 17);
    assert!(h.avg_duration_ms > 0.0);
    assert!(h.max_duration_ms >= h.min_duration_ms);
    assert_eq!(h.last_status, "pass");

    // Consecutive tracking
    assert_eq!(h.consecutive_passes, 1);
    assert_eq!(h.consecutive_failures, 0);
}

// ── Integration: Edge Cases ──────────────────────────────────────────────────

#[test]
fn test_optimizer_edge_cases() {
    let opt = make_optimizer();

    // Empty inputs
    assert!(opt.optimize_order(&[]).is_empty());
    assert!(opt.detect_flaky_tests(30.0).is_empty());
    assert!(opt.find_duplicate_tests(&[]).is_empty());
    assert!(opt.suggest_parallel_batches(&[], 4).is_empty());

    let empty_perf = opt.analyze_performance(&[]);
    assert_eq!(empty_perf.total_tests, 0);

    // Single test
    let single = opt.optimize_order(&["test_only".into()]);
    assert_eq!(single.len(), 1);
    assert_eq!(single[0], "test_only");

    // Single timing
    let single_perf = opt.analyze_performance(&[make_timing("test_only", 100, true)]);
    assert_eq!(single_perf.total_tests, 1);
    assert_eq!(single_perf.total_duration_ms, 100);

    // No history flaky detection
    let no_flaky = opt.detect_flaky_tests(30.0);
    assert!(no_flaky.is_empty());

    // Empty failure patterns
    let empty_patterns = opt.analyze_failure_patterns();
    assert_eq!(empty_patterns.total_failing_tests, 0);
}

// ── Integration: Test Category Classification ────────────────────────────────

#[test]
fn test_test_category_classification() {
    use TestOptimizer;

    let cases = vec![
        ("test_security_audit", TestCategory::Security),
        ("test_critical_path", TestCategory::Security),
        ("test_audit_compliance", TestCategory::Security),
        ("test_integration_flow", TestCategory::Integration),
        ("test_e2e_workflow", TestCategory::Integration),
        ("test_lifecycle_check", TestCategory::Integration),
        ("test_smoke_check", TestCategory::Smoke),
        ("test_sanity_validation", TestCategory::Smoke),
        ("test_perf_benchmark", TestCategory::Performance),
        ("test_load_testing", TestCategory::Performance),
        ("test_property_invariant", TestCategory::Property),
        ("test_fuzz_corpus", TestCategory::Property),
        ("test_unit_math", TestCategory::Unit),
        ("test_something_general", TestCategory::General),
    ];

    for (name, expected) in cases {
        let actual = TestOptimizer::classify_test(name);
        assert_eq!(
            actual, expected,
            "Mismatch for '{}': expected {:?}, got {:?}",
            name, expected, actual
        );
    }
}
