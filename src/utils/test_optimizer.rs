use crate::utils::test_generator::GeneratedTestCase;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// ── Core Data Structures ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestHistory {
    pub name: String,
    pub total_runs: u32,
    pub failures: u32,
    pub passes: u32,
    pub flaky_count: u32,
    pub avg_duration_ms: f64,
    pub max_duration_ms: u64,
    pub min_duration_ms: u64,
    pub last_run_duration_ms: u64,
    pub last_status: String,
    pub last_run: String,
    pub consecutive_failures: u32,
    pub consecutive_passes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlakyTestInfo {
    pub test_name: String,
    pub flakiness_score: f64,
    pub failure_rate: f64,
    pub total_runs: u32,
    pub transitions: u32,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum TestPriority {
    Critical,
    High,
    Medium,
    Low,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TestCategory {
    Security,
    Integration,
    Smoke,
    Performance,
    Property,
    Unit,
    General,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizedTestCase {
    pub name: String,
    pub priority: TestPriority,
    pub estimated_duration_ms: u64,
    pub failure_probability: f64,
    pub coverage_impact: f64,
    pub dependencies: Vec<String>,
    pub resource_profile: ResourceProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceProfile {
    pub memory_mb: u64,
    pub cpu_intensity: f64,
    pub io_intensity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateTestInfo {
    pub test_a: String,
    pub test_b: String,
    pub similarity_score: f64,
    pub shared_functions: Vec<String>,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCacheEntry {
    pub wasm_hash: String,
    pub test_name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub cached_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestPerformanceReport {
    pub total_tests: u32,
    pub total_duration_ms: u64,
    pub avg_duration_ms: f64,
    pub median_duration_ms: f64,
    pub p95_duration_ms: f64,
    pub p99_duration_ms: f64,
    pub slowest_tests: Vec<String>,
    pub fastest_tests: Vec<String>,
    pub duration_distribution: Vec<DurationBucket>,
    pub parallel_efficiency: f64,
    pub cache_hit_rate: f64,
    pub optimization_savings_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DurationBucket {
    pub range: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestOptimizationReport {
    pub wasm_hash: String,
    pub timestamp: String,
    pub original_order: Vec<String>,
    pub optimized_order: Vec<String>,
    pub estimated_improvement_pct: f64,
    pub flaky_tests: Vec<FlakyTestInfo>,
    pub duplicate_tests: Vec<DuplicateTestInfo>,
    pub performance: TestPerformanceReport,
    pub cache_stats: CacheStats,
    pub prioritization: PrioritizationReport,
    pub failure_patterns: FailurePatternReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_requests: u32,
    pub cache_hits: u32,
    pub cache_misses: u32,
    pub cache_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrioritizationReport {
    pub critical_tests: u32,
    pub high_priority: u32,
    pub medium_priority: u32,
    pub low_priority: u32,
    pub estimated_risk_reduction: f64,
}

#[derive(Debug, Clone)]
pub struct TestCaseTiming {
    pub name: String,
    pub duration_ms: u64,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureCategory {
    pub category: String,
    pub test_count: u32,
    pub total_failures: u32,
    pub avg_failure_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailurePatternReport {
    pub total_failing_tests: u32,
    pub categories: Vec<FailureCategory>,
    pub recurrence_ratio: f64,
}

// ── Test Optimizer ──────────────────────────────────────────────────────────

pub struct TestOptimizer {
    config_dir: PathBuf,
    pub history: HashMap<String, TestHistory>,
    cache: HashMap<String, TestCacheEntry>,
}

impl TestOptimizer {
    pub fn new() -> Result<Self> {
        let config_dir = crate::utils::config::config_dir().join("test_optimizer");
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)
                .with_context(|| format!("Failed to create {}", config_dir.display()))?;
        }
        let history = Self::load_history(&config_dir);
        let cache = Self::load_cache(&config_dir);
        Ok(Self {
            config_dir,
            history,
            cache,
        })
    }

    fn load_history(dir: &Path) -> HashMap<String, TestHistory> {
        let path = dir.join("history.json");
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(h) = serde_json::from_str(&content) {
                    return h;
                }
            }
        }
        HashMap::new()
    }

    fn load_cache(dir: &Path) -> HashMap<String, TestCacheEntry> {
        let path = dir.join("cache.json");
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(c) = serde_json::from_str(&content) {
                    return c;
                }
            }
        }
        HashMap::new()
    }

    fn save_state(&self) -> Result<()> {
        if !self.config_dir.exists() {
            fs::create_dir_all(&self.config_dir)
                .with_context(|| format!("Failed to create {}", self.config_dir.display()))?;
        }
        let history_path = self.config_dir.join("history.json");
        fs::write(&history_path, serde_json::to_string_pretty(&self.history)?)
            .with_context(|| format!("Failed to write {}", history_path.display()))?;
        let cache_path = self.config_dir.join("cache.json");
        fs::write(&cache_path, serde_json::to_string_pretty(&self.cache)?)
            .with_context(|| format!("Failed to write {}", cache_path.display()))?;
        Ok(())
    }

    // ── Smart Test Ordering ──────────────────────────────────────────────

    pub fn optimize_order(&self, test_names: &[String]) -> Vec<String> {
        let mut scored: Vec<(String, f64)> = test_names
            .iter()
            .map(|name| {
                let score = self.compute_priority_score(name);
                (name.clone(), score)
            })
            .collect();

        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });

        scored.into_iter().map(|(name, _)| name).collect()
    }

    fn compute_priority_score(&self, test_name: &str) -> f64 {
        let mut score = 0.0;

        if let Some(history) = self.history.get(test_name) {
            if history.total_runs > 0 {
                let fail_rate = history.failures as f64 / history.total_runs as f64;
                score += fail_rate * 100.0;
            }

            if history.flaky_count > 0 {
                score += history.flaky_count as f64 * 10.0;
            }

            if history.consecutive_failures > 0 {
                score += (history.consecutive_failures as f64).min(50.0);
            }

            if history.avg_duration_ms > 0.0 {
                let duration_penalty = (history.avg_duration_ms / 1000.0).min(20.0);
                score -= duration_penalty;
            }
        }

        let lower = test_name.to_lowercase();
        if lower.contains("security") || lower.contains("critical") {
            score += 80.0;
        }
        if lower.contains("auth") || lower.contains("unauthorized") {
            score += 50.0;
        }
        if lower.contains("boundary") || lower.contains("edge") {
            score += 30.0;
        }
        if lower.contains("integration") || lower.contains("e2e") {
            score += 40.0;
        }
        if lower.contains("smoke") || lower.contains("sanity") {
            score += 20.0;
        }
        if lower.contains("perf") || lower.contains("benchmark") || lower.contains("load") {
            score -= 15.0;
        }
        if lower.contains("prop") || lower.contains("property") {
            score += 25.0;
        }

        score.max(0.0)
    }

    pub fn classify_test(test_name: &str) -> TestCategory {
        let lower = test_name.to_lowercase();
        if lower.contains("security") || lower.contains("audit") || lower.contains("critical") {
            TestCategory::Security
        } else if lower.contains("e2e")
            || lower.contains("integration")
            || lower.contains("lifecycle")
        {
            TestCategory::Integration
        } else if lower.contains("smoke") || lower.contains("sanity") {
            TestCategory::Smoke
        } else if lower.contains("perf") || lower.contains("benchmark") || lower.contains("load") {
            TestCategory::Performance
        } else if lower.contains("prop") || lower.contains("property") || lower.contains("fuzz") {
            TestCategory::Property
        } else if lower.contains("unit") || lower.contains("test_") {
            TestCategory::Unit
        } else {
            TestCategory::General
        }
    }

    pub fn batch_tests_by_profile(
        &self,
        tests: &[OptimizedTestCase],
    ) -> Vec<Vec<OptimizedTestCase>> {
        let mut batches: Vec<Vec<OptimizedTestCase>> = Vec::new();

        let (io_bound, other): (Vec<OptimizedTestCase>, Vec<OptimizedTestCase>) = tests
            .iter()
            .cloned()
            .partition(|t| t.resource_profile.io_intensity > 0.6);
        let (cpu_only, other): (Vec<_>, Vec<_>) = other
            .into_iter()
            .partition(|t| t.resource_profile.cpu_intensity > 0.6);
        let (mem_only, general): (Vec<_>, Vec<_>) = other
            .into_iter()
            .partition(|t| t.resource_profile.memory_mb > 256);

        for chunk in io_bound.chunks(2) {
            batches.push(chunk.to_vec());
        }
        for chunk in cpu_only.chunks(4) {
            batches.push(chunk.to_vec());
        }
        for chunk in mem_only.chunks(1) {
            batches.push(chunk.to_vec());
        }
        for chunk in general.chunks(6) {
            batches.push(chunk.to_vec());
        }

        batches
    }

    pub fn suggest_parallel_batches(
        &self,
        test_names: &[String],
        max_workers: usize,
    ) -> Vec<Vec<String>> {
        let optimized = self.optimize_order(test_names);
        if optimized.is_empty() {
            return vec![];
        }

        let mut batches: Vec<Vec<String>> = Vec::new();
        let batch_size = (optimized.len() as f64 / max_workers as f64).ceil() as usize;
        for chunk in optimized.chunks(batch_size.max(1)) {
            batches.push(chunk.to_vec());
        }
        batches
    }

    // ── Flaky Test Detection ─────────────────────────────────────────────

    pub fn detect_flaky_tests(&self, threshold: f64) -> Vec<FlakyTestInfo> {
        let mut flaky = Vec::new();
        for (name, history) in &self.history {
            if history.total_runs < 3 {
                continue;
            }
            let failure_rate = history.failures as f64 / history.total_runs as f64;
            let flakiness_score = self.calculate_flakiness_score(history);

            if flakiness_score >= threshold {
                flaky.push(FlakyTestInfo {
                    test_name: name.clone(),
                    flakiness_score,
                    failure_rate,
                    total_runs: history.total_runs,
                    transitions: history.flaky_count,
                    recommendation: if flakiness_score > 80.0 {
                        "Flagged for investigation: highly flaky test".into()
                    } else if flakiness_score > 50.0 {
                        "Monitor: test shows moderate flakiness".into()
                    } else {
                        "Review: test has some instability".into()
                    },
                });
            }
        }
        flaky.sort_by(|a, b| {
            b.flakiness_score
                .partial_cmp(&a.flakiness_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        flaky
    }

    fn calculate_flakiness_score(&self, history: &TestHistory) -> f64 {
        if history.total_runs < 3 {
            return 0.0;
        }

        let failure_rate = history.failures as f64 / history.total_runs as f64;
        let stability = 1.0 - (failure_rate - 0.5).abs() * 2.0;
        let transition_ratio = history.flaky_count as f64 / history.total_runs as f64;

        let mut score = stability * 60.0 + transition_ratio * 40.0;

        if failure_rate < 0.1 || failure_rate > 0.9 {
            score *= 0.3;
        }

        score.clamp(0.0, 100.0)
    }

    // ── Test Deduplication ───────────────────────────────────────────────

    pub fn find_duplicate_tests(&self, cases: &[GeneratedTestCase]) -> Vec<DuplicateTestInfo> {
        let mut duplicates = Vec::new();
        if cases.len() < 2 {
            return duplicates;
        }

        for i in 0..cases.len() - 1 {
            for j in (i + 1)..cases.len() {
                let a = &cases[i];
                let b = &cases[j];
                let similarity = self.compute_similarity(a, b);
                if similarity > 0.7 {
                    let shared = if a.function == b.function {
                        vec![a.function.clone()]
                    } else {
                        Vec::new()
                    };
                    duplicates.push(DuplicateTestInfo {
                        test_a: a.name.clone(),
                        test_b: b.name.clone(),
                        similarity_score: similarity,
                        shared_functions: shared,
                        recommendation: if similarity > 0.9 {
                            "High duplication: consider merging these tests".into()
                        } else {
                            "Some overlap: review for consolidation opportunities".into()
                        },
                    });
                }
            }
        }

        duplicates.sort_by(|a, b| {
            b.similarity_score
                .partial_cmp(&a.similarity_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        duplicates
    }

    fn compute_similarity(&self, a: &GeneratedTestCase, b: &GeneratedTestCase) -> f64 {
        let mut matching = 0.0;
        let mut total = 0.0;

        total += 1.0;
        if a.function == b.function {
            matching += 1.0;
        }

        total += 1.0;
        if a.test_type == b.test_type {
            matching += 1.0;
        }

        total += 1.0;
        if a.expected_behavior == b.expected_behavior {
            matching += 1.0;
        }

        let sec_a: HashSet<&str> = a.security_checks.iter().map(|s| s.as_str()).collect();
        let sec_b: HashSet<&str> = b.security_checks.iter().map(|s| s.as_str()).collect();
        if !sec_a.is_empty() || !sec_b.is_empty() {
            total += 1.0;
            let intersection = sec_a.intersection(&sec_b).count();
            let union = sec_a.union(&sec_b).count();
            if union > 0 {
                matching += intersection as f64 / union as f64;
            }
        }

        if total == 0.0 {
            0.0
        } else {
            matching / total
        }
    }

    // ── Test Result Recording ────────────────────────────────────────────

    pub fn record_result(&mut self, test_name: &str, passed: bool, duration_ms: u64) -> Result<()> {
        let entry = self
            .history
            .entry(test_name.to_string())
            .or_insert(TestHistory {
                name: test_name.to_string(),
                total_runs: 0,
                failures: 0,
                passes: 0,
                flaky_count: 0,
                avg_duration_ms: 0.0,
                max_duration_ms: 0,
                min_duration_ms: u64::MAX,
                last_run_duration_ms: duration_ms,
                last_status: if passed { "pass".into() } else { "fail".into() },
                last_run: chrono::Utc::now().to_rfc3339(),
                consecutive_failures: 0,
                consecutive_passes: 0,
            });

        let previous_status = entry.last_status.clone();
        let current_status = if passed { "pass" } else { "fail" };
        if previous_status != current_status && entry.total_runs > 0 {
            entry.flaky_count += 1;
        }

        entry.total_runs += 1;
        if passed {
            entry.passes += 1;
            entry.consecutive_passes += 1;
            entry.consecutive_failures = 0;
        } else {
            entry.failures += 1;
            entry.consecutive_failures += 1;
            entry.consecutive_passes = 0;
        }

        entry.last_status = current_status.to_string();
        entry.last_run_duration_ms = duration_ms;
        entry.max_duration_ms = entry.max_duration_ms.max(duration_ms);
        entry.min_duration_ms = entry.min_duration_ms.min(duration_ms);

        let prev_total = (entry.total_runs - 1) as f64;
        entry.avg_duration_ms =
            (entry.avg_duration_ms * prev_total + duration_ms as f64) / entry.total_runs as f64;
        entry.last_run = chrono::Utc::now().to_rfc3339();

        self.save_state()
    }

    // ── Cache Management ─────────────────────────────────────────────────

    pub fn check_cache(&self, wasm_hash: &str, test_name: &str) -> Option<bool> {
        let key = format!("{}:{}", wasm_hash, test_name);
        self.cache.get(&key).map(|entry| entry.passed)
    }

    pub fn update_cache(
        &mut self,
        wasm_hash: &str,
        test_name: &str,
        passed: bool,
        duration_ms: u64,
    ) -> Result<()> {
        let key = format!("{}:{}", wasm_hash, test_name);
        self.cache.insert(
            key,
            TestCacheEntry {
                wasm_hash: wasm_hash.to_string(),
                test_name: test_name.to_string(),
                passed,
                duration_ms,
                cached_at: chrono::Utc::now().to_rfc3339(),
            },
        );

        if self.cache.len() > 10000 {
            self.prune_cache();
        }

        self.save_state()
    }

    fn prune_cache(&mut self) {
        let keys_to_remove: Vec<String> = {
            let mut entries: Vec<(String, &TestCacheEntry)> =
                self.cache.iter().map(|(k, v)| (k.clone(), v)).collect();
            entries.sort_by(|a, b| a.1.cached_at.cmp(&b.1.cached_at));

            let remove_count = (entries.len() as f64 * 0.2) as usize;
            entries
                .into_iter()
                .take(remove_count)
                .map(|(k, _)| k)
                .collect()
        };
        for key in keys_to_remove {
            self.cache.remove(&key);
        }
    }

    pub fn get_cache_stats(&self) -> CacheStats {
        let unique_wasms: HashSet<&str> =
            self.cache.values().map(|e| e.wasm_hash.as_str()).collect();
        CacheStats {
            total_requests: 0,
            cache_hits: 0,
            cache_misses: 0,
            cache_size: unique_wasms.len() as u32,
        }
    }

    // ── Performance Analysis ─────────────────────────────────────────────

    pub fn analyze_performance(&self, results: &[TestCaseTiming]) -> TestPerformanceReport {
        if results.is_empty() {
            return TestPerformanceReport {
                total_tests: 0,
                total_duration_ms: 0,
                avg_duration_ms: 0.0,
                median_duration_ms: 0.0,
                p95_duration_ms: 0.0,
                p99_duration_ms: 0.0,
                slowest_tests: vec![],
                fastest_tests: vec![],
                duration_distribution: vec![],
                parallel_efficiency: 0.0,
                cache_hit_rate: 0.0,
                optimization_savings_ms: 0,
            };
        }

        let total_duration: u64 = results.iter().map(|r| r.duration_ms).sum();
        let avg = total_duration as f64 / results.len() as f64;

        let mut sorted = results.to_vec();
        sorted.sort_by(|a, b| a.duration_ms.cmp(&b.duration_ms));

        let median = sorted[sorted.len() / 2].duration_ms as f64;
        let p95_idx = ((sorted.len() as f64 * 0.95) as usize).min(sorted.len() - 1);
        let p95 = sorted[p95_idx].duration_ms as f64;
        let p99_idx = ((sorted.len() as f64 * 0.99) as usize).min(sorted.len() - 1);
        let p99 = sorted[p99_idx].duration_ms as f64;

        let slowest: Vec<String> = sorted
            .iter()
            .rev()
            .take(5)
            .map(|r| r.name.clone())
            .collect();
        let fastest: Vec<String> = sorted.iter().take(5).map(|r| r.name.clone()).collect();

        let buckets = self.compute_duration_buckets(&sorted);
        let parallel_efficiency = self.estimate_parallel_efficiency(results);
        let cache_stats = self.get_cache_stats();

        TestPerformanceReport {
            total_tests: results.len() as u32,
            total_duration_ms: total_duration,
            avg_duration_ms: avg,
            median_duration_ms: median,
            p95_duration_ms: p95,
            p99_duration_ms: p99,
            slowest_tests: slowest,
            fastest_tests: fastest,
            duration_distribution: buckets,
            parallel_efficiency,
            cache_hit_rate: if cache_stats.total_requests > 0 {
                cache_stats.cache_hits as f64 / cache_stats.total_requests as f64 * 100.0
            } else {
                0.0
            },
            optimization_savings_ms: self.estimate_optimization_savings(results),
        }
    }

    fn compute_duration_buckets(&self, sorted: &[TestCaseTiming]) -> Vec<DurationBucket> {
        let ranges: [(&str, u64); 7] = [
            ("<10ms", 10),
            ("10-50ms", 50),
            ("50-100ms", 100),
            ("100-500ms", 500),
            ("500ms-1s", 1000),
            ("1-5s", 5000),
            (">5s", u64::MAX),
        ];

        let mut buckets = Vec::new();
        let mut idx = 0;
        for (label, max) in ranges {
            let count = sorted[idx..]
                .iter()
                .take_while(|r| r.duration_ms <= max)
                .count();
            idx += count;
            buckets.push(DurationBucket {
                range: label.to_string(),
                count: count as u32,
            });
        }
        buckets
    }

    fn estimate_parallel_efficiency(&self, results: &[TestCaseTiming]) -> f64 {
        if results.is_empty() {
            return 0.0;
        }
        let serial_time: u64 = results.iter().map(|r| r.duration_ms).sum();
        let max_time = results.iter().map(|r| r.duration_ms).max().unwrap_or(1);
        if serial_time == 0 || max_time == 0 {
            return 0.0;
        }
        let speedup = serial_time as f64 / max_time as f64;
        let ideal = results.len() as f64;
        (speedup / ideal * 100.0).min(100.0)
    }

    fn estimate_optimization_savings(&self, results: &[TestCaseTiming]) -> u64 {
        if results.is_empty() {
            return 0;
        }
        let serial: u64 = results.iter().map(|r| r.duration_ms).sum();
        if serial == 0 {
            return 0;
        }
        let num_workers: usize = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let max_single: u64 = results.iter().map(|r| r.duration_ms).max().unwrap_or(0);

        let estimated_parallel = max_single + (serial / num_workers as u64).min(max_single / 2);
        let estimated_optimized = (estimated_parallel as f64 * 0.7) as u64;

        serial.saturating_sub(estimated_optimized)
    }

    // ── Failure Pattern Analysis ────────────────────────────────────────

    pub fn analyze_failure_patterns(&self) -> FailurePatternReport {
        let mut by_category: HashMap<String, Vec<&TestHistory>> = HashMap::new();
        let mut all_failing: Vec<&TestHistory> = Vec::new();

        for history in self.history.values() {
            if history.failures > 0 {
                let category = Self::classify_test(&history.name);
                let key = format!("{:?}", category);
                by_category.entry(key).or_default().push(history);
                all_failing.push(history);
            }
        }

        let total_failing = all_failing.len();
        let mut category_summary: Vec<FailureCategory> = by_category
            .into_iter()
            .map(|(cat, tests)| {
                let total_failures: u32 = tests.iter().map(|t| t.failures).sum();
                let avg_fail_rate = if !tests.is_empty() {
                    tests
                        .iter()
                        .map(|t| t.failures as f64 / t.total_runs.max(1) as f64)
                        .sum::<f64>()
                        / tests.len() as f64
                } else {
                    0.0
                };
                FailureCategory {
                    category: cat,
                    test_count: tests.len() as u32,
                    total_failures,
                    avg_failure_rate: avg_fail_rate,
                }
            })
            .collect();
        category_summary.sort_by(|a, b| b.total_failures.cmp(&a.total_failures));

        let recurrence_ratio = if total_failing > 0 {
            all_failing
                .iter()
                .filter(|t| t.consecutive_failures > 1)
                .count() as f64
                / total_failing as f64
        } else {
            0.0
        };

        FailurePatternReport {
            total_failing_tests: total_failing as u32,
            categories: category_summary,
            recurrence_ratio,
        }
    }

    // ── Resource Optimization ────────────────────────────────────────────

    pub fn schedule_with_resources(
        &self,
        tests: &[OptimizedTestCase],
        max_concurrency: usize,
        memory_limit_mb: u64,
    ) -> Vec<Vec<OptimizedTestCase>> {
        let mut sorted_tests = tests.to_vec();
        sorted_tests.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.estimated_duration_ms.cmp(&b.estimated_duration_ms))
        });

        let mut batches: Vec<Vec<OptimizedTestCase>> = Vec::new();
        let mut current_batch: Vec<OptimizedTestCase> = Vec::new();
        let mut current_memory: u64 = 0;

        for test in sorted_tests {
            let test_memory = test.resource_profile.memory_mb;
            if current_batch.len() >= max_concurrency
                || (current_memory + test_memory) > memory_limit_mb
            {
                batches.push(std::mem::take(&mut current_batch));
                current_memory = 0;
            }
            current_memory += test_memory;
            current_batch.push(test);
        }

        if !current_batch.is_empty() {
            batches.push(current_batch);
        }

        batches
    }

    // ── Full Optimization Pipeline ───────────────────────────────────────

    pub fn optimize_all(
        &mut self,
        wasm_hash: &str,
        test_names: &[String],
        generated_cases: &[GeneratedTestCase],
        results: &[TestCaseTiming],
    ) -> TestOptimizationReport {
        let original_order = test_names.to_vec();
        let optimized_order = self.optimize_order(test_names);

        let estimated_improvement = if results.is_empty() {
            0.0
        } else {
            let total_serial: u64 = results.iter().map(|r| r.duration_ms).sum();
            let savings = self.estimate_optimization_savings(results);
            if total_serial > 0 {
                (savings as f64 / total_serial as f64) * 100.0
            } else {
                0.0
            }
        };

        let flaky_tests = self.detect_flaky_tests(30.0);
        let duplicate_tests = self.find_duplicate_tests(generated_cases);
        let performance = self.analyze_performance(results);
        let cache_stats = self.get_cache_stats();
        let failure_patterns = self.analyze_failure_patterns();

        let total_tests = test_names.len() as u32;
        let critical = test_names
            .iter()
            .filter(|n| {
                let l = n.to_lowercase();
                l.contains("security") || l.contains("critical")
            })
            .count() as u32;
        let high = test_names
            .iter()
            .filter(|n| {
                let l = n.to_lowercase();
                l.contains("auth") || l.contains("fail")
            })
            .count() as u32;
        let medium = test_names
            .iter()
            .filter(|n| {
                let l = n.to_lowercase();
                l.contains("boundary") || l.contains("edge")
            })
            .count() as u32;
        let low = total_tests.saturating_sub(critical + high + medium);

        TestOptimizationReport {
            wasm_hash: wasm_hash.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            original_order,
            optimized_order,
            estimated_improvement_pct: estimated_improvement.min(100.0),
            flaky_tests,
            duplicate_tests,
            performance,
            cache_stats,
            prioritization: PrioritizationReport {
                critical_tests: critical,
                high_priority: high,
                medium_priority: medium,
                low_priority: low,
                estimated_risk_reduction: (critical as f64 / total_tests.max(1) as f64) * 100.0,
            },
            failure_patterns,
        }
    }
}

// ── Report Export ───────────────────────────────────────────────────────────

pub fn export_optimization_report(report: &TestOptimizationReport, path: &Path) -> Result<PathBuf> {
    let parent = path.parent().unwrap_or(Path::new("."));
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_string_pretty(report)?)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(path.to_path_buf())
}

pub fn render_optimization_html_report(report: &TestOptimizationReport) -> String {
    let improvement_class = if report.estimated_improvement_pct > 50.0 {
        "good"
    } else {
        "warn"
    };

    let flaky_class = if report.flaky_tests.is_empty() {
        "good"
    } else {
        "warn"
    };

    let duplicate_class = if report.duplicate_tests.is_empty() {
        "good"
    } else {
        "bad"
    };

    let flaky_rows: String = report
        .flaky_tests
        .iter()
        .map(|f| {
            format!(
                "<tr class=\"flaky\"><td>{}</td><td>{:.1}</td><td>{:.1}%</td><td>{}</td><td>{}</td></tr>",
                html_escape(&f.test_name),
                f.flakiness_score,
                f.failure_rate * 100.0,
                f.total_runs,
                html_escape(&f.recommendation)
            )
        })
        .collect::<Vec<_>>()
        .join("\n        ");

    let duplicate_rows: String = report
        .duplicate_tests
        .iter()
        .map(|d| {
            format!(
                "<tr class=\"duplicate\"><td>{}</td><td>{}</td><td>{:.0}%</td><td>{}</td><td>{}</td></tr>",
                html_escape(&d.test_a),
                html_escape(&d.test_b),
                d.similarity_score * 100.0,
                html_escape(&d.shared_functions.join(", ")),
                html_escape(&d.recommendation)
            )
        })
        .collect::<Vec<_>>()
        .join("\n        ");

    let duration_bucket_rows: String = report
        .performance
        .duration_distribution
        .iter()
        .map(|b| format!("<tr><td>{}</td><td>{}</td></tr>", b.range, b.count))
        .collect::<Vec<_>>()
        .join("\n        ");

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>AI Test Optimization Report</title>
    <style>
        body {{ font-family: system-ui, -apple-system, sans-serif; margin: 2rem; background: #0d1117; color: #e6edf3; }}
        .card {{ background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 1.5rem; margin: 1rem 0; }}
        h1, h2 {{ color: #f0f6fc; border-bottom: 1px solid #30363d; padding-bottom: 0.5rem; }}
        h3 {{ color: #f0f6fc; }}
        .grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 1rem; }}
        .metric {{ font-size: 1.8rem; font-weight: bold; }}
        .good {{ color: #3fb950; }}
        .warn {{ color: #d29922; }}
        .bad {{ color: #f85149; }}
        table {{ width: 100%; border-collapse: collapse; margin-top: 1rem; }}
        th, td {{ border: 1px solid #30363d; padding: 0.5rem; text-align: left; }}
        th {{ background: #21262d; }}
        .flaky {{ background: rgba(210, 153, 34, 0.1); }}
        .duplicate {{ background: rgba(248, 81, 73, 0.1); }}
        .order-list {{ display: flex; flex-wrap: wrap; gap: 0.5rem; }}
        .order-item {{ background: #21262d; border: 1px solid #30363d; border-radius: 4px; padding: 0.25rem 0.5rem; font-size: 0.85rem; }}
        code {{ background: #21262d; padding: 0.1rem 0.3rem; border-radius: 3px; }}
    </style>
</head>
<body>
    <h1>AI Test Optimization Report</h1>
    <p>WASM: <code>{}</code> | Generated: {}</p>

    <div class="grid">
        <div class="card">
            <div class="metric {}">{:.1}%</div>
            <div>Estimated Execution Improvement</div>
        </div>
        <div class="card">
            <div class="metric good">{} / {}</div>
            <div>Tests Prioritized / Total</div>
        </div>
        <div class="card">
            <div class="metric {}">{}</div>
            <div>Flaky Tests Detected</div>
        </div>
        <div class="card">
            <div class="metric {}">{}</div>
            <div>Duplicate Groups Found</div>
        </div>
    </div>

    <h2>Test Order</h2>
    <div class="card">
        <h3>Optimized Execution Order (top 20)</h3>
        <div class="order-list">
            {}
        </div>
    </div>

    <h2>Test Prioritization</h2>
    <div class="card">
        <p>Critical: <strong>{}</strong> | High: <strong>{}</strong> | Medium: <strong>{}</strong> | Low: <strong>{}</strong></p>
        <p>Estimated Risk Reduction: <strong>{:.1}%</strong></p>
    </div>

    <h2>Flaky Tests</h2>
    <table>
        <tr><th>Test Name</th><th>Flakiness Score</th><th>Failure Rate</th><th>Total Runs</th><th>Recommendation</th></tr>
        {}
    </table>

    <h2>Duplicate Tests</h2>
    <table>
        <tr><th>Test A</th><th>Test B</th><th>Similarity</th><th>Shared Functions</th><th>Recommendation</th></tr>
        {}
    </table>

    <h2>Performance Summary</h2>
    <div class="card">
        <p>Total Tests: <strong>{}</strong> | Total Duration: <strong>{}ms</strong></p>
        <p>Average: <strong>{:.1}ms</strong> | Median: <strong>{:.1}ms</strong> | P95: <strong>{:.1}ms</strong> | P99: <strong>{:.1}ms</strong></p>
        <p>Parallel Efficiency: <strong>{:.1}%</strong> | Cache Hit Rate: <strong>{:.1}%</strong></p>
        <p>Optimization Savings: <strong>{}ms</strong></p>
    </div>

    <h3>Duration Distribution</h3>
    <table>
        <tr><th>Range</th><th>Count</th></tr>
        {}
    </table>

    <h2>Slowest Tests</h2>
    <div class="card">
        <ol>
            {}
        </ol>
    </div>

    <h2>Failure Patterns</h2>
    <div class="card">
        <p>Failing Tests: <strong>{}</strong> | Recurrence Ratio: <strong>{:.1}%</strong></p>
    </div>
    <table>
        <tr><th>Category</th><th>Test Count</th><th>Total Failures</th><th>Avg Failure Rate</th></tr>
        {}
    </table>
</body>
</html>"#,
        report.wasm_hash,
        report.timestamp,
        improvement_class,
        report.estimated_improvement_pct,
        report.prioritization.critical_tests + report.prioritization.high_priority,
        report.performance.total_tests,
        flaky_class,
        report.flaky_tests.len(),
        duplicate_class,
        report.duplicate_tests.len(),
        report
            .optimized_order
            .iter()
            .take(20)
            .map(|name| format!("<span class=\"order-item\">{}</span>", html_escape(name)))
            .collect::<Vec<_>>()
            .join("\n            "),
        report.prioritization.critical_tests,
        report.prioritization.high_priority,
        report.prioritization.medium_priority,
        report.prioritization.low_priority,
        report.prioritization.estimated_risk_reduction,
        flaky_rows,
        duplicate_rows,
        report.performance.total_tests,
        report.performance.total_duration_ms,
        report.performance.avg_duration_ms,
        report.performance.median_duration_ms,
        report.performance.p95_duration_ms,
        report.performance.p99_duration_ms,
        report.performance.parallel_efficiency,
        report.performance.cache_hit_rate,
        report.performance.optimization_savings_ms,
        duration_bucket_rows,
        report
            .performance
            .slowest_tests
            .iter()
            .map(|name| format!("<li>{}</li>", html_escape(name)))
            .collect::<Vec<_>>()
            .join("\n            "),
        report.failure_patterns.total_failing_tests,
        report.failure_patterns.recurrence_ratio * 100.0,
        report
            .failure_patterns
            .categories
            .iter()
            .map(|c| format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{:.1}%</td></tr>",
                html_escape(&c.category),
                c.test_count,
                c.total_failures,
                c.avg_failure_rate * 100.0
            ))
            .collect::<Vec<_>>()
            .join("\n        "),
    )
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_optimizer() -> TestOptimizer {
        TestOptimizer {
            config_dir: PathBuf::from("/tmp/test_optimizer"),
            history: HashMap::new(),
            cache: HashMap::new(),
        }
    }

    #[test]
    fn test_optimize_order_empty() {
        let optimizer = create_test_optimizer();
        let result = optimizer.optimize_order(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_optimize_order_prioritizes_security() {
        let optimizer = create_test_optimizer();
        let tests = vec![
            "test_normal".into(),
            "test_security_check".into(),
            "test_basic".into(),
        ];
        let result = optimizer.optimize_order(&tests);
        assert_eq!(result[0], "test_security_check");
    }

    #[test]
    fn test_flaky_detection_insufficient_data() {
        let optimizer = create_test_optimizer();
        let result = optimizer.detect_flaky_tests(30.0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_flaky_detection_with_data() {
        let mut optimizer = create_test_optimizer();
        optimizer.history.insert(
            "test_flaky".into(),
            TestHistory {
                name: "test_flaky".into(),
                total_runs: 10,
                failures: 5,
                passes: 5,
                flaky_count: 8,
                avg_duration_ms: 100.0,
                max_duration_ms: 150,
                min_duration_ms: 50,
                last_run_duration_ms: 100,
                last_status: "pass".into(),
                last_run: chrono::Utc::now().to_rfc3339(),
                consecutive_failures: 0,
                consecutive_passes: 3,
            },
        );
        optimizer.history.insert(
            "test_stable".into(),
            TestHistory {
                name: "test_stable".into(),
                total_runs: 10,
                failures: 0,
                passes: 10,
                flaky_count: 0,
                avg_duration_ms: 50.0,
                max_duration_ms: 60,
                min_duration_ms: 40,
                last_run_duration_ms: 50,
                last_status: "pass".into(),
                last_run: chrono::Utc::now().to_rfc3339(),
                consecutive_failures: 0,
                consecutive_passes: 10,
            },
        );
        let result = optimizer.detect_flaky_tests(30.0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].test_name, "test_flaky");
    }

    #[test]
    fn test_find_duplicates() {
        let optimizer = create_test_optimizer();
        let cases = vec![
            GeneratedTestCase {
                name: "test_init_happy".into(),
                description: "test".into(),
                function: "initialize".into(),
                test_type: "happy_path".into(),
                input_data: vec![],
                expected_behavior: "works".into(),
                security_checks: vec![],
            },
            GeneratedTestCase {
                name: "test_init_happy_dupe".into(),
                description: "test".into(),
                function: "initialize".into(),
                test_type: "happy_path".into(),
                input_data: vec![],
                expected_behavior: "works".into(),
                security_checks: vec![],
            },
            GeneratedTestCase {
                name: "test_different".into(),
                description: "test".into(),
                function: "other".into(),
                test_type: "error".into(),
                input_data: vec![],
                expected_behavior: "fails".into(),
                security_checks: vec![],
            },
        ];
        let result = optimizer.find_duplicate_tests(&cases);
        assert_eq!(result.len(), 1);
        assert!(result[0].similarity_score > 0.7);
    }

    #[test]
    fn test_record_result() {
        let mut optimizer = create_test_optimizer();
        optimizer
            .record_result("test_a", true, 100)
            .expect("record_result failed");
        assert_eq!(optimizer.history.len(), 1);
        let entry = &optimizer.history["test_a"];
        assert_eq!(entry.total_runs, 1);
        assert_eq!(entry.passes, 1);
        assert_eq!(entry.avg_duration_ms, 100.0);
    }

    #[test]
    fn test_flaky_transition_detection() {
        let mut optimizer = create_test_optimizer();
        optimizer
            .record_result("test_flaky", true, 100)
            .expect("record_result failed");
        optimizer
            .record_result("test_flaky", false, 200)
            .expect("record_result failed");
        optimizer
            .record_result("test_flaky", true, 150)
            .expect("record_result failed");
        let entry = &optimizer.history["test_flaky"];
        assert_eq!(entry.total_runs, 3);
        assert_eq!(entry.flaky_count, 2);
    }

    #[test]
    fn test_cache_operations() {
        let mut optimizer = create_test_optimizer();
        assert!(optimizer.check_cache("hash1", "test_a").is_none());
        optimizer
            .update_cache("hash1", "test_a", true, 100)
            .expect("update_cache failed");
        let result = optimizer.check_cache("hash1", "test_a");
        assert_eq!(result, Some(true));
    }

    #[test]
    fn test_analyze_performance_empty() {
        let optimizer = create_test_optimizer();
        let report = optimizer.analyze_performance(&[]);
        assert_eq!(report.total_tests, 0);
    }

    #[test]
    fn test_analyze_performance() {
        let optimizer = create_test_optimizer();
        let timings = vec![
            TestCaseTiming {
                name: "fast".into(),
                duration_ms: 10,
                passed: true,
            },
            TestCaseTiming {
                name: "medium".into(),
                duration_ms: 100,
                passed: true,
            },
            TestCaseTiming {
                name: "slow".into(),
                duration_ms: 500,
                passed: true,
            },
        ];
        let report = optimizer.analyze_performance(&timings);
        assert_eq!(report.total_tests, 3);
        assert_eq!(report.total_duration_ms, 610);
        assert!((report.avg_duration_ms - 203.3).abs() < 1.0);
        assert_eq!(report.slowest_tests[0], "slow");
    }

    #[test]
    fn test_schedule_with_resources() {
        let optimizer = create_test_optimizer();
        let tests = vec![
            OptimizedTestCase {
                name: "heavy1".into(),
                priority: TestPriority::High,
                estimated_duration_ms: 1000,
                failure_probability: 0.1,
                coverage_impact: 0.8,
                dependencies: vec![],
                resource_profile: ResourceProfile {
                    memory_mb: 512,
                    cpu_intensity: 0.9,
                    io_intensity: 0.1,
                },
            },
            OptimizedTestCase {
                name: "light1".into(),
                priority: TestPriority::Low,
                estimated_duration_ms: 50,
                failure_probability: 0.0,
                coverage_impact: 0.2,
                dependencies: vec![],
                resource_profile: ResourceProfile {
                    memory_mb: 64,
                    cpu_intensity: 0.1,
                    io_intensity: 0.8,
                },
            },
            OptimizedTestCase {
                name: "heavy2".into(),
                priority: TestPriority::Critical,
                estimated_duration_ms: 2000,
                failure_probability: 0.3,
                coverage_impact: 0.9,
                dependencies: vec![],
                resource_profile: ResourceProfile {
                    memory_mb: 1024,
                    cpu_intensity: 1.0,
                    io_intensity: 0.0,
                },
            },
        ];
        let batches = optimizer.schedule_with_resources(&tests, 2, 2048);
        assert_eq!(batches.len(), 2);
    }

    #[test]
    fn test_optimize_all_pipeline() {
        let mut optimizer = create_test_optimizer();
        let test_names = vec!["test_a".into(), "test_security_b".into(), "test_c".into()];
        let generated = vec![GeneratedTestCase {
            name: "test_a".into(),
            description: "desc".into(),
            function: "func".into(),
            test_type: "happy_path".into(),
            input_data: vec![],
            expected_behavior: "works".into(),
            security_checks: vec![],
        }];
        let results = vec![
            TestCaseTiming {
                name: "test_a".into(),
                duration_ms: 100,
                passed: true,
            },
            TestCaseTiming {
                name: "test_security_b".into(),
                duration_ms: 200,
                passed: true,
            },
            TestCaseTiming {
                name: "test_c".into(),
                duration_ms: 50,
                passed: false,
            },
        ];
        let report = optimizer.optimize_all("hash123", &test_names, &generated, &results);
        assert_eq!(report.optimized_order[0], "test_security_b");
        assert!(!report.estimated_improvement_pct.is_nan());
        assert_eq!(report.wasm_hash, "hash123");
    }

    #[test]
    fn test_priority_scoring() {
        let optimizer = create_test_optimizer();
        let score = optimizer.compute_priority_score("test_security_auth");
        assert!(score > 100.0);
        let low_score = optimizer.compute_priority_score("test_simple_case");
        assert!(low_score < score);
    }

    #[test]
    fn test_classify_test_security() {
        assert_eq!(
            TestOptimizer::classify_test("test_security_audit"),
            TestCategory::Security
        );
        assert_eq!(
            TestOptimizer::classify_test("test_critical_path"),
            TestCategory::Security
        );
    }

    #[test]
    fn test_classify_test_integration() {
        assert_eq!(
            TestOptimizer::classify_test("test_e2e_workflow"),
            TestCategory::Integration
        );
        assert_eq!(
            TestOptimizer::classify_test("test_integration_check"),
            TestCategory::Integration
        );
        assert_eq!(
            TestOptimizer::classify_test("test_wallet_lifecycle"),
            TestCategory::Integration
        );
    }

    #[test]
    fn test_classify_test_smoke() {
        assert_eq!(
            TestOptimizer::classify_test("test_smoke_check"),
            TestCategory::Smoke
        );
        assert_eq!(
            TestOptimizer::classify_test("test_sanity_test"),
            TestCategory::Smoke
        );
    }

    #[test]
    fn test_classify_test_performance() {
        assert_eq!(
            TestOptimizer::classify_test("test_perf_benchmark"),
            TestCategory::Performance
        );
        assert_eq!(
            TestOptimizer::classify_test("test_load_testing"),
            TestCategory::Performance
        );
    }

    #[test]
    fn test_classify_test_property() {
        assert_eq!(
            TestOptimizer::classify_test("test_property_invariant"),
            TestCategory::Property
        );
        assert_eq!(
            TestOptimizer::classify_test("test_fuzz_input"),
            TestCategory::Property
        );
    }

    #[test]
    fn test_batch_tests_by_profile() {
        let optimizer = create_test_optimizer();
        let tests = vec![
            OptimizedTestCase {
                name: "io_test".into(),
                priority: TestPriority::High,
                estimated_duration_ms: 100,
                failure_probability: 0.1,
                coverage_impact: 0.5,
                dependencies: vec![],
                resource_profile: ResourceProfile {
                    memory_mb: 128,
                    cpu_intensity: 0.3,
                    io_intensity: 0.9,
                },
            },
            OptimizedTestCase {
                name: "cpu_test".into(),
                priority: TestPriority::Medium,
                estimated_duration_ms: 200,
                failure_probability: 0.0,
                coverage_impact: 0.3,
                dependencies: vec![],
                resource_profile: ResourceProfile {
                    memory_mb: 64,
                    cpu_intensity: 0.8,
                    io_intensity: 0.2,
                },
            },
            OptimizedTestCase {
                name: "memory_test".into(),
                priority: TestPriority::Critical,
                estimated_duration_ms: 500,
                failure_probability: 0.2,
                coverage_impact: 0.9,
                dependencies: vec![],
                resource_profile: ResourceProfile {
                    memory_mb: 512,
                    cpu_intensity: 0.5,
                    io_intensity: 0.1,
                },
            },
            OptimizedTestCase {
                name: "general_test".into(),
                priority: TestPriority::Low,
                estimated_duration_ms: 50,
                failure_probability: 0.0,
                coverage_impact: 0.1,
                dependencies: vec![],
                resource_profile: ResourceProfile {
                    memory_mb: 32,
                    cpu_intensity: 0.2,
                    io_intensity: 0.1,
                },
            },
        ];
        let batches = optimizer.batch_tests_by_profile(&tests);
        assert!(!batches.is_empty());
        let total_in_batches: usize = batches.iter().map(|b| b.len()).sum();
        assert_eq!(total_in_batches, tests.len());
    }

    #[test]
    fn test_suggest_parallel_batches() {
        let optimizer = create_test_optimizer();
        let tests = vec![
            "test_a".into(),
            "test_b".into(),
            "test_c".into(),
            "test_d".into(),
            "test_e".into(),
            "test_f".into(),
        ];
        let batches = optimizer.suggest_parallel_batches(&tests, 3);
        assert_eq!(batches.len(), 3);
        let total: usize = batches.iter().map(|b| b.len()).sum();
        assert_eq!(total, tests.len());
    }

    #[test]
    fn test_suggest_parallel_batches_empty() {
        let optimizer = create_test_optimizer();
        let batches = optimizer.suggest_parallel_batches(&[], 4);
        assert!(batches.is_empty());
    }

    #[test]
    fn test_analyze_failure_patterns_empty() {
        let optimizer = create_test_optimizer();
        let report = optimizer.analyze_failure_patterns();
        assert_eq!(report.total_failing_tests, 0);
        assert!(report.categories.is_empty());
    }

    #[test]
    fn test_analyze_failure_patterns_with_data() {
        let mut optimizer = create_test_optimizer();
        optimizer.history.insert(
            "test_security_check".into(),
            TestHistory {
                name: "test_security_check".into(),
                total_runs: 10,
                failures: 3,
                passes: 7,
                flaky_count: 2,
                avg_duration_ms: 100.0,
                max_duration_ms: 150,
                min_duration_ms: 50,
                last_run_duration_ms: 100,
                last_status: "pass".into(),
                last_run: chrono::Utc::now().to_rfc3339(),
                consecutive_failures: 0,
                consecutive_passes: 5,
            },
        );
        optimizer.history.insert(
            "test_e2e_flow".into(),
            TestHistory {
                name: "test_e2e_flow".into(),
                total_runs: 5,
                failures: 1,
                passes: 4,
                flaky_count: 1,
                avg_duration_ms: 500.0,
                max_duration_ms: 600,
                min_duration_ms: 400,
                last_run_duration_ms: 450,
                last_status: "pass".into(),
                last_run: chrono::Utc::now().to_rfc3339(),
                consecutive_failures: 0,
                consecutive_passes: 4,
            },
        );
        let report = optimizer.analyze_failure_patterns();
        assert_eq!(report.total_failing_tests, 2);
        assert!(!report.categories.is_empty());
    }

    #[test]
    fn test_optimize_order_prioritizes_integration() {
        let optimizer = create_test_optimizer();
        let tests = vec![
            "test_unit".into(),
            "test_integration_flow".into(),
            "test_e2e_workflow".into(),
        ];
        let result = optimizer.optimize_order(&tests);
        assert!(
            result.iter().position(|t| t == "test_integration_flow")
                < result.iter().position(|t| t == "test_unit")
        );
    }

    #[test]
    fn test_priority_scoring_with_history() {
        let mut optimizer = create_test_optimizer();
        optimizer.history.insert(
            "test_failing".into(),
            TestHistory {
                name: "test_failing".into(),
                total_runs: 10,
                failures: 8,
                passes: 2,
                flaky_count: 6,
                avg_duration_ms: 50.0,
                max_duration_ms: 100,
                min_duration_ms: 30,
                last_run_duration_ms: 50,
                last_status: "fail".into(),
                last_run: chrono::Utc::now().to_rfc3339(),
                consecutive_failures: 3,
                consecutive_passes: 0,
            },
        );
        let score = optimizer.compute_priority_score("test_failing");
        assert!(score > 0.0);
        let low_score = optimizer.compute_priority_score("test_stable");
        assert!(low_score < score);
    }

    #[test]
    fn test_export_report_roundtrip() {
        let report = TestOptimizationReport {
            wasm_hash: "abc123".into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            original_order: vec!["a".into(), "b".into()],
            optimized_order: vec!["b".into(), "a".into()],
            estimated_improvement_pct: 50.0,
            flaky_tests: vec![],
            duplicate_tests: vec![],
            performance: TestPerformanceReport {
                total_tests: 2,
                total_duration_ms: 100,
                avg_duration_ms: 50.0,
                median_duration_ms: 50.0,
                p95_duration_ms: 100.0,
                p99_duration_ms: 100.0,
                slowest_tests: vec!["a".into()],
                fastest_tests: vec!["b".into()],
                duration_distribution: vec![],
                parallel_efficiency: 100.0,
                cache_hit_rate: 0.0,
                optimization_savings_ms: 0,
            },
            cache_stats: CacheStats {
                total_requests: 0,
                cache_hits: 0,
                cache_misses: 0,
                cache_size: 0,
            },
            prioritization: PrioritizationReport {
                critical_tests: 1,
                high_priority: 0,
                medium_priority: 0,
                low_priority: 1,
                estimated_risk_reduction: 50.0,
            },
            failure_patterns: FailurePatternReport {
                total_failing_tests: 0,
                categories: vec![],
                recurrence_ratio: 0.0,
            },
        };
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("report.json");
        let result = export_optimization_report(&report, &path);
        assert!(result.is_ok());
        assert!(path.exists());
    }
}
