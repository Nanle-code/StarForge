use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub providers: HashMap<String, ProviderRateLimit>,
    pub default_limits: ProviderRateLimit,
    pub enable_priority_queue: bool,
    pub max_queue_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRateLimit {
    pub requests_per_minute: u32,
    pub tokens_per_minute: u32,
    pub burst_size: u32,
    pub backoff_base_ms: u64,
    pub backoff_max_ms: u64,
    pub max_retries: u32,
}

impl Default for ProviderRateLimit {
    fn default() -> Self {
        Self {
            requests_per_minute: 60,
            tokens_per_minute: 90000,
            burst_size: 10,
            backoff_base_ms: 1000,
            backoff_max_ms: 60000,
            max_retries: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RequestPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

#[derive(Debug, Clone)]
pub struct RateLimitedRequest {
    pub id: String,
    pub provider: String,
    pub priority: RequestPriority,
    pub estimated_tokens: u32,
    pub created_at: Instant,
}

#[derive(Debug, Clone)]
struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: f64, refill_rate_per_sec: f64) -> Self {
        Self {
            tokens: max_tokens,
            max_tokens,
            refill_rate: refill_rate_per_sec,
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;
    }

    fn try_consume(&mut self, tokens: f64) -> bool {
        self.refill();
        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }

    fn available_tokens(&mut self) -> f64 {
        self.refill();
        self.tokens
    }
}

#[derive(Debug)]
struct ProviderRateLimiter {
    request_bucket: TokenBucket,
    token_bucket: TokenBucket,
    config: ProviderRateLimit,
    consecutive_failures: u32,
    last_request: Option<Instant>,
}

impl ProviderRateLimiter {
    fn new(config: ProviderRateLimit) -> Self {
        let refill_rate = config.requests_per_minute as f64 / 60.0;
        let token_refill = config.tokens_per_minute as f64 / 60.0;

        Self {
            request_bucket: TokenBucket::new(config.burst_size as f64, refill_rate),
            token_bucket: TokenBucket::new(config.tokens_per_minute as f64, token_refill),
            config,
            consecutive_failures: 0,
            last_request: None,
        }
    }

    fn can_proceed(&mut self, estimated_tokens: u32) -> bool {
        self.request_bucket.try_consume(1.0)
            && self.token_bucket.try_consume(estimated_tokens as f64)
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.last_request = Some(Instant::now());
    }

    fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        self.last_request = Some(Instant::now());
    }

    fn backoff_duration(&self) -> Duration {
        if self.consecutive_failures == 0 {
            return Duration::from_millis(0);
        }

        let base = self.config.backoff_base_ms as f64;
        let max = self.config.backoff_max_ms as f64;
        let backoff = (base * 2.0_f64.powi(self.consecutive_failures as i32 - 1)).min(max);

        let jitter = backoff * 0.1 * rand::random::<f64>();
        Duration::from_millis((backoff + jitter) as u64)
    }

    fn is_circuit_open(&self) -> bool {
        self.consecutive_failures >= self.config.max_retries
    }
}

pub struct AIRateLimiter {
    limiters: RwLock<HashMap<String, Arc<Mutex<ProviderRateLimiter>>>>,
    config: RateLimitConfig,
    queue: Mutex<Vec<RateLimitedRequest>>,
    metrics: RwLock<RateLimitMetrics>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateLimitMetrics {
    pub total_requests: u64,
    pub rejected_requests: u64,
    pub queued_requests: u64,
    pub completed_requests: u64,
    pub failed_requests: u64,
    pub average_wait_time_ms: f64,
    pub provider_metrics: HashMap<String, ProviderMetrics>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderMetrics {
    pub requests: u64,
    pub successes: u64,
    pub failures: u64,
    pub tokens_used: u64,
    pub average_latency_ms: f64,
}

impl AIRateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        let mut limiters = HashMap::new();

        for (provider, provider_config) in &config.providers {
            limiters.insert(
                provider.clone(),
                Arc::new(Mutex::new(ProviderRateLimiter::new(
                    provider_config.clone(),
                ))),
            );
        }

        Self {
            limiters: RwLock::new(limiters),
            config,
            queue: Mutex::new(Vec::new()),
            metrics: RwLock::new(RateLimitMetrics::default()),
        }
    }

    pub async fn check_rate_limit(&self, provider: &str, estimated_tokens: u32) -> Result<bool> {
        let limiters = self.limiters.read().await;

        if let Some(limiter) = limiters.get(provider) {
            let mut limiter = limiter.lock().await;
            Ok(limiter.can_proceed(estimated_tokens))
        } else {
            Ok(true)
        }
    }

    pub async fn acquire_permit(
        &self,
        provider: &str,
        estimated_tokens: u32,
        priority: RequestPriority,
    ) -> Result<RateLimitPermit> {
        let mut metrics = self.metrics.write().await;
        metrics.total_requests += 1;

        let limiters = self.limiters.read().await;
        let limiter = limiters
            .get(provider)
            .context(format!("Unknown provider: {}", provider))?;

        {
            let mut limiter = limiter.lock().await;

            if limiter.is_circuit_open() {
                metrics.rejected_requests += 1;
                let backoff = limiter.backoff_duration();
                return Err(anyhow::anyhow!(
                    "Circuit breaker open for provider {}. Retry after {:?}",
                    provider,
                    backoff
                ));
            }

            if limiter.can_proceed(estimated_tokens) {
                limiter.record_success();
                return Ok(RateLimitPermit {
                    provider: provider.to_string(),
                    tokens: estimated_tokens,
                    granted_at: Instant::now(),
                });
            }
        }

        if self.config.enable_priority_queue {
            let request = RateLimitedRequest {
                id: uuid::Uuid::new_v4().to_string(),
                provider: provider.to_string(),
                priority,
                estimated_tokens,
                created_at: Instant::now(),
            };

            let mut queue = self.queue.lock().await;
            if queue.len() >= self.config.max_queue_size {
                metrics.rejected_requests += 1;
                return Err(anyhow::anyhow!("Rate limit queue full"));
            }

            queue.push(request);
            queue.sort_by_key(|a| std::cmp::Reverse(a.priority));
            metrics.queued_requests += 1;

            drop(queue);
            drop(limiters);

            self.wait_for_permit(provider, estimated_tokens).await
        } else {
            metrics.rejected_requests += 1;
            Err(anyhow::anyhow!(
                "Rate limit exceeded for provider {}",
                provider
            ))
        }
    }

    async fn wait_for_permit(
        &self,
        provider: &str,
        estimated_tokens: u32,
    ) -> Result<RateLimitPermit> {
        let max_wait = Duration::from_secs(30);
        let start = Instant::now();
        let mut check_interval = Duration::from_millis(100);

        loop {
            if start.elapsed() >= max_wait {
                return Err(anyhow::anyhow!("Timeout waiting for rate limit permit"));
            }

            tokio::time::sleep(check_interval).await;

            let limiters = self.limiters.read().await;
            if let Some(limiter) = limiters.get(provider) {
                let mut limiter = limiter.lock().await;
                if limiter.can_proceed(estimated_tokens) {
                    limiter.record_success();

                    let mut queue = self.queue.lock().await;
                    queue.retain(|r| r.provider != provider);

                    let mut metrics = self.metrics.write().await;
                    let wait_ms = start.elapsed().as_millis() as f64;
                    let count = metrics.completed_requests as f64;
                    metrics.average_wait_time_ms =
                        (metrics.average_wait_time_ms * count + wait_ms) / (count + 1.0);
                    metrics.completed_requests += 1;

                    return Ok(RateLimitPermit {
                        provider: provider.to_string(),
                        tokens: estimated_tokens,
                        granted_at: start,
                    });
                }

                check_interval = (check_interval * 2).min(Duration::from_secs(2));
            }
        }
    }

    pub async fn record_request_result(
        &self,
        provider: &str,
        success: bool,
        tokens_used: u32,
        latency_ms: u64,
    ) {
        let limiters = self.limiters.read().await;
        if let Some(limiter) = limiters.get(provider) {
            let mut limiter = limiter.lock().await;
            if success {
                limiter.record_success();
            } else {
                limiter.record_failure();
            }
        }

        let mut metrics = self.metrics.write().await;
        let provider_metrics = metrics
            .provider_metrics
            .entry(provider.to_string())
            .or_default();

        provider_metrics.requests += 1;
        if success {
            provider_metrics.successes += 1;
        } else {
            provider_metrics.failures += 1;
        }
        provider_metrics.tokens_used += tokens_used as u64;

        let count = provider_metrics.requests as f64;
        provider_metrics.average_latency_ms =
            (provider_metrics.average_latency_ms * (count - 1.0) + latency_ms as f64) / count;
    }

    pub async fn get_metrics(&self) -> RateLimitMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn get_queue_length(&self) -> usize {
        self.queue.lock().await.len()
    }

    pub async fn get_available_tokens(&self, provider: &str) -> Result<f64> {
        let limiters = self.limiters.read().await;
        let limiter = limiters
            .get(provider)
            .context(format!("Unknown provider: {}", provider))?;
        let mut limiter = limiter.lock().await;
        Ok(limiter.token_bucket.available_tokens())
    }
}

#[derive(Debug)]
pub struct RateLimitPermit {
    pub provider: String,
    pub tokens: u32,
    pub granted_at: Instant,
}

impl RateLimitPermit {
    pub fn elapsed(&self) -> Duration {
        self.granted_at.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket_consume() {
        let mut bucket = TokenBucket::new(10.0, 1.0);
        assert!(bucket.try_consume(5.0));
        assert!(bucket.try_consume(5.0));
        assert!(!bucket.try_consume(1.0));
    }

    #[test]
    fn test_token_bucket_refill() {
        let mut bucket = TokenBucket::new(10.0, 100.0);
        bucket.try_consume(10.0);
        bucket.last_refill = Instant::now() - Duration::from_secs(1);
        assert!(bucket.try_consume(1.0));
    }

    #[test]
    fn test_priority_ordering() {
        let mut priorities = vec![
            RequestPriority::Low,
            RequestPriority::Critical,
            RequestPriority::Normal,
            RequestPriority::High,
        ];
        priorities.sort_by(|a, b| b.cmp(a));
        assert_eq!(priorities[0], RequestPriority::Critical);
        assert_eq!(priorities[3], RequestPriority::Low);
    }

    #[tokio::test]
    async fn test_rate_limiter_check() {
        let config = RateLimitConfig {
            providers: HashMap::new(),
            default_limits: ProviderRateLimit::default(),
            enable_priority_queue: false,
            max_queue_size: 100,
        };
        let limiter = AIRateLimiter::new(config);
        let result = limiter.check_rate_limit("unknown", 100).await.unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn test_rate_limiter_metrics() {
        let config = RateLimitConfig {
            providers: HashMap::new(),
            default_limits: ProviderRateLimit::default(),
            enable_priority_queue: false,
            max_queue_size: 100,
        };
        let limiter = AIRateLimiter::new(config);
        limiter
            .record_request_result("openai", true, 100, 500)
            .await;
        let metrics = limiter.get_metrics().await;
        assert_eq!(metrics.provider_metrics["openai"].requests, 1);
        assert_eq!(metrics.provider_metrics["openai"].successes, 1);
    }
}
