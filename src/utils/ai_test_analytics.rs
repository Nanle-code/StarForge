//! AI-powered Test Analytics System
//!
//! Collects, analyzes, and provides predictive insights on test execution data.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Represents a single test result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub duration_ms: u64,
    pub success: bool,
    pub timestamp: DateTime<Utc>,
    pub coverage_percent: Option<f32>,
}

/// Analytics data for tests
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TestAnalyticsData {
    pub total_tests_run: u64,
    pub total_passed: u64,
    pub total_failed: u64,
    pub total_duration_ms: u64,
    pub test_results: Vec<TestResult>,
    pub flaky_test_patterns: HashMap<String, u64>,
}

impl TestAnalyticsData {
    pub fn record_test(&mut self, result: TestResult) {
        self.total_tests_run += 1;
        if result.success {
            self.total_passed += 1;
        } else {
            self.total_failed += 1;
            // Simple flaky detection: if failed after passed
            if self
                .test_results
                .iter()
                .any(|r| r.name == result.name && r.success)
            {
                *self
                    .flaky_test_patterns
                    .entry(result.name.clone())
                    .or_insert(0) += 1;
            }
        }
        self.total_duration_ms += result.duration_ms;
        self.test_results.push(result);
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_tests_run == 0 {
            0.0
        } else {
            (self.total_passed as f64) / (self.total_tests_run as f64)
        }
    }
}

/// Service for managing test analytics
pub struct TestAnalyticsService {
    analytics: Arc<RwLock<TestAnalyticsData>>,
}

impl Default for TestAnalyticsService {
    fn default() -> Self {
        Self::new()
    }
}

impl TestAnalyticsService {
    pub fn new() -> Self {
        TestAnalyticsService {
            analytics: Arc::new(RwLock::new(TestAnalyticsData::default())),
        }
    }

    pub async fn record_test_result(&self, result: TestResult) {
        let mut data = self.analytics.write().await;
        data.record_test(result);
    }

    pub async fn get_analytics(&self) -> TestAnalyticsData {
        self.analytics.read().await.clone()
    }

    // Predictive insight placeholder
    pub async fn get_predictive_insights(&self) -> String {
        let data = self.analytics.read().await;
        if data.flaky_test_patterns.len() > 2 {
            format!("Warning: Detected {} potentially flaky tests. Suggest prioritizing these for investigation.", data.flaky_test_patterns.len())
        } else {
            "No significant flaky test patterns detected.".to_string()
        }
    }
}
