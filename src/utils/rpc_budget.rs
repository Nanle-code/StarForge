//! RPC request budgeting and backpressure.
//!
//! Provides rate limiting and concurrency control for Soroban RPC requests
//! to prevent overwhelming RPC providers and the CLI itself during orchestration
//! and monitoring operations.

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// RPC budget configuration with configurable limits.
#[derive(Debug, Clone)]
pub struct RpcBudgetConfig {
    /// Maximum requests per second (QPS) per RPC endpoint
    pub max_qps: u32,
    /// Maximum concurrent requests per RPC endpoint
    pub max_concurrent: u32,
    /// Whether budget enforcement is enabled
    pub enabled: bool,
}

impl Default for RpcBudgetConfig {
    fn default() -> Self {
        Self {
            // Conservative defaults for public RPC endpoints
            max_qps: 10,
            max_concurrent: 5,
            enabled: true,
        }
    }
}

impl RpcBudgetConfig {
    /// Create a custom budget configuration.
    pub fn new(max_qps: u32, max_concurrent: u32, enabled: bool) -> Self {
        Self {
            max_qps,
            max_concurrent,
            enabled,
        }
    }

    /// Load configuration from environment variables.
    ///
    /// Environment variables:
    /// - STARFORGE_RPC_MAX_QPS: Maximum QPS (default: 10)
    /// - STARFORGE_RPC_MAX_CONCURRENT: Maximum concurrent requests (default: 5)
    /// - STARFORGE_RPC_BUDGET_ENABLED: Enable/disable budgeting (default: true)
    pub fn from_env() -> Self {
        let max_qps = std::env::var("STARFORGE_RPC_MAX_QPS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);

        let max_concurrent = std::env::var("STARFORGE_RPC_MAX_CONCURRENT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);

        let enabled = std::env::var("STARFORGE_RPC_BUDGET_ENABLED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(true);

        Self {
            max_qps,
            max_concurrent,
            enabled,
        }
    }

    /// Create budget configuration optimized for CI environments.
    pub fn for_ci() -> Self {
        Self {
            max_qps: 5,      // More conservative for CI
            max_concurrent: 3,
            enabled: true,
        }
    }

    /// Create budget configuration optimized for interactive use.
    pub fn for_interactive() -> Self {
        Self {
            max_qps: 15,     // More permissive for interactive use
            max_concurrent: 8,
            enabled: true,
        }
    }
}

/// Per-endpoint RPC budget tracker.
#[derive(Debug)]
pub struct RpcBudget {
    config: RpcBudgetConfig,
    semaphore: Arc<Semaphore>,
    request_count: Arc<AtomicU64>,
    window_start: Arc<AtomicU64>, // Unix timestamp in seconds
    telemetry_enabled: bool,
}

impl RpcBudget {
    /// Create a new RPC budget with the given configuration.
    pub fn new(config: RpcBudgetConfig) -> Self {
        let permits = config.max_concurrent as usize;
        Self {
            semaphore: Arc::new(Semaphore::new(permits)),
            config,
            request_count: Arc::new(AtomicU64::new(0)),
            window_start: Arc::new(AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )),
            telemetry_enabled: std::env::var("STARFORGE_RPC_TELEMETRY").is_ok(),
        }
    }

    /// Create a budget with default configuration.
    pub fn default_config() -> Self {
        Self::new(RpcBudgetConfig::default())
    }

    /// Create a budget from environment variables.
    pub fn from_env() -> Self {
        Self::new(RpcBudgetConfig::from_env())
    }

    /// Acquire a permit for making an RPC request.
    /// 
    /// This enforces both concurrency limits (via semaphore) and QPS limits
    /// (via rate limiting). Returns an error if budgets are exhausted.
    pub async fn acquire_permit(&self) -> Result<RpcPermit> {
        if !self.config.enabled {
            return Ok(RpcPermit::new(None, self.telemetry_enabled));
        }

        // Check QPS limit
        self.check_qps_limit()?;

        // Acquire concurrency permit
        let permit = self.semaphore
            .acquire()
            .await
            .context("Failed to acquire RPC concurrency permit")?;

        Ok(RpcPermit::new(Some(permit), self.telemetry_enabled))
    }

    /// Check if the QPS limit would be exceeded.
    fn check_qps_limit(&self) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let window_start = self.window_start.load(Ordering::Relaxed);
        let window_duration = now - window_start;

        // Reset window if more than 1 second has passed
        if window_duration >= 1 {
            self.window_start.store(now, Ordering::Relaxed);
            self.request_count.store(0, Ordering::Relaxed);
        }

        let current_count = self.request_count.load(Ordering::Relaxed);
        if current_count >= self.config.max_qps as u64 {
            return Err(anyhow::anyhow!(
                "RPC QPS budget exhausted: {} requests/sec limit reached. \
                 Wait 1 second or increase STARFORGE_RPC_MAX_QPS.",
                self.config.max_qps
            ));
        }

        // Increment counter
        self.request_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Get current budget statistics.
    pub fn stats(&self) -> RpcBudgetStats {
        let current_concurrent = self.semaphore.available_permits();
        let used_concurrent = self.config.max_concurrent as usize - current_concurrent;
        
        RpcBudgetStats {
            max_qps: self.config.max_qps,
            max_concurrent: self.config.max_concurrent,
            current_qps: self.request_count.load(Ordering::Relaxed) as u32,
            current_concurrent: used_concurrent as u32,
            enabled: self.config.enabled,
        }
    }
}

/// A permit that represents permission to make an RPC request.
/// When dropped, it releases the concurrency permit.
pub struct RpcPermit {
    _permit: Option<tokio::sync::SemaphorePermit<'static>>,
    telemetry_enabled: bool,
    start_time: Option<Instant>,
}

impl RpcPermit {
    fn new(permit: Option<tokio::sync::SemaphorePermit<'static>>, telemetry_enabled: bool) -> Self {
        let start_time = if telemetry_enabled {
            Some(Instant::now())
        } else {
            None
        };
        
        Self {
            _permit: permit,
            telemetry_enabled,
            start_time,
        }
    }
}

impl Drop for RpcPermit {
    fn drop(&mut self) {
        if self.telemetry_enabled {
            if let Some(start) = self.start_time {
                let duration = start.elapsed();
                tracing::debug!(
                    "RPC request completed in {:?}",
                    duration
                );
            }
        }
    }
}

/// Statistics about current RPC budget usage.
#[derive(Debug, Clone)]
pub struct RpcBudgetStats {
    pub max_qps: u32,
    pub max_concurrent: u32,
    pub current_qps: u32,
    pub current_concurrent: u32,
    pub enabled: bool,
}

impl RpcBudgetStats {
    /// Check if budgets are near saturation (above 80% capacity).
    pub fn is_near_saturation(&self) -> bool {
        if !self.enabled {
            return false;
        }
        
        let qps_utilization = self.current_qps as f64 / self.max_qps as f64;
        let concurrent_utilization = self.current_concurrent as f64 / self.max_concurrent as f64;
        
        qps_utilization > 0.8 || concurrent_utilization > 0.8
    }

    /// Get a formatted summary of budget status.
    pub fn summary(&self) -> String {
        if !self.enabled {
            return "RPC budgeting disabled".to_string();
        }

        let qps_pct = (self.current_qps as f64 / self.max_qps as f64 * 100.0) as u32;
        let concurrent_pct = (self.current_concurrent as f64 / self.max_concurrent as f64 * 100.0) as u32;

        format!(
            "RPC Budget: {}/{} QPS ({}%), {}/{} concurrent ({}%)",
            self.current_qps, self.max_qps, qps_pct,
            self.current_concurrent, self.max_concurrent, concurrent_pct
        )
    }
}

/// Global RPC budget manager (singleton per RPC endpoint).
pub struct RpcBudgetManager {
    budgets: std::collections::HashMap<String, Arc<RpcBudget>>,
}

impl RpcBudgetManager {
    /// Create a new RPC budget manager.
    pub fn new() -> Self {
        Self {
            budgets: std::collections::HashMap::new(),
        }
    }

    /// Get or create a budget for the given RPC endpoint.
    pub fn get_budget(&mut self, endpoint: &str) -> Arc<RpcBudget> {
        if let Some(budget) = self.budgets.get(endpoint) {
            return Arc::clone(budget);
        }

        let budget = Arc::new(RpcBudget::from_env());
        self.budgets.insert(endpoint.to_string(), Arc::clone(&budget));
        budget
    }

    /// Check if any budget is near saturation and log a warning.
    pub fn check_saturation(&self) {
        for (endpoint, budget) in &self.budgets {
            let stats = budget.stats();
            if stats.is_near_saturation() {
                tracing::warn!(
                    "RPC budget near saturation for {}: {}",
                    endpoint,
                    stats.summary()
                );
            }
        }
    }
}

impl Default for RpcBudgetManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RpcBudgetConfig::default();
        assert_eq!(config.max_qps, 10);
        assert_eq!(config.max_concurrent, 5);
        assert!(config.enabled);
    }

    #[test]
    fn test_custom_config() {
        let config = RpcBudgetConfig::new(20, 10, false);
        assert_eq!(config.max_qps, 20);
        assert_eq!(config.max_concurrent, 10);
        assert!(!config.enabled);
    }

    #[test]
    fn test_ci_config() {
        let config = RpcBudgetConfig::for_ci();
        assert_eq!(config.max_qps, 5);
        assert_eq!(config.max_concurrent, 3);
        assert!(config.enabled);
    }

    #[test]
    fn test_interactive_config() {
        let config = RpcBudgetConfig::for_interactive();
        assert_eq!(config.max_qps, 15);
        assert_eq!(config.max_concurrent, 8);
        assert!(config.enabled);
    }

    #[test]
    fn test_budget_stats_saturation() {
        let stats = RpcBudgetStats {
            max_qps: 10,
            max_concurrent: 5,
            current_qps: 9,
            current_concurrent: 4,
            enabled: true,
        };
        assert!(stats.is_near_saturation());

        let stats_low = RpcBudgetStats {
            max_qps: 10,
            max_concurrent: 5,
            current_qps: 5,
            current_concurrent: 2,
            enabled: true,
        };
        assert!(!stats_low.is_near_saturation());
    }

    #[test]
    fn test_budget_stats_disabled() {
        let stats = RpcBudgetStats {
            max_qps: 10,
            max_concurrent: 5,
            current_qps: 10,
            current_concurrent: 5,
            enabled: false,
        };
        assert!(!stats.is_near_saturation());
        assert_eq!(stats.summary(), "RPC budgeting disabled");
    }
}
