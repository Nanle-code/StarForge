//! AI Error Handling and Recovery System
//!
//! Provides robust error handling for AI operations with automatic recovery,
//! fallback mechanisms, and user-friendly error messages.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Error categories for AI operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AiErrorCategory {
    /// API errors: rate limits, auth failures, service unavailable
    Api,
    /// Network errors: timeouts, connection failures
    Network,
    /// Validation errors: invalid responses, parsing failures
    Validation,
    /// Content errors: safety filters, policy violations
    Content,
    /// Unknown error type
    Unknown,
}

impl AiErrorCategory {
    pub fn from_error_code(code: &str) -> Self {
        match code {
            "rate_limit_exceeded" | "auth_failed" | "service_unavailable" => AiErrorCategory::Api,
            "timeout" | "connection_failed" | "dns_error" => AiErrorCategory::Network,
            "invalid_response" | "parse_error" | "schema_mismatch" => AiErrorCategory::Validation,
            "content_filtered" | "policy_violation" | "safety_rejected" => AiErrorCategory::Content,
            _ => AiErrorCategory::Unknown,
        }
    }

    pub fn user_friendly_name(&self) -> &'static str {
        match self {
            AiErrorCategory::Api => "API Error",
            AiErrorCategory::Network => "Network Error",
            AiErrorCategory::Validation => "Validation Error",
            AiErrorCategory::Content => "Content Error",
            AiErrorCategory::Unknown => "Unknown Error",
        }
    }

    pub fn can_retry(&self) -> bool {
        matches!(self, AiErrorCategory::Api | AiErrorCategory::Network)
    }

    pub fn should_fallback(&self) -> bool {
        matches!(self, AiErrorCategory::Api | AiErrorCategory::Content)
    }
}

/// Detailed error information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiError {
    pub category: AiErrorCategory,
    pub code: String,
    pub message: String,
    pub user_message: String,
    pub timestamp: DateTime<Utc>,
    pub retry_count: u32,
    pub provider: String,
}

impl AiError {
    pub fn new(category: AiErrorCategory, code: String, message: String, provider: String) -> Self {
        let sanitized_msg = crate::utils::redaction::redact_secrets(&message);
        let user_message = Self::generate_user_message(&category, &sanitized_msg);
        AiError {
            category,
            code,
            message: sanitized_msg,
            user_message,
            timestamp: Utc::now(),
            retry_count: 0,
            provider,
        }
    }

    fn generate_user_message(category: &AiErrorCategory, technical_message: &str) -> String {
        match category {
            AiErrorCategory::Api => {
                format!(
                    "The AI service encountered an API error. This might be due to rate limiting or temporary service issues. {}",
                    if technical_message.contains("rate_limit") {
                        "Please wait a moment and try again."
                    } else {
                        "The system will attempt to recover automatically."
                    }
                )
            }
            AiErrorCategory::Network => {
                "Unable to connect to the AI service. Please check your internet connection. The system will retry automatically.".to_string()
            }
            AiErrorCategory::Validation => {
                "The AI response could not be processed. This might be a temporary issue. The system will attempt to use a fallback provider.".to_string()
            }
            AiErrorCategory::Content => {
                "The content was filtered by safety policies. Please rephrase your request and try again.".to_string()
            }
            AiErrorCategory::Unknown => {
                "An unexpected error occurred. The system will attempt to recover.".to_string()
            }
        }
    }

    pub fn with_retry(mut self, count: u32) -> Self {
        self.retry_count = count;
        self
    }
}

/// Retry configuration with exponential backoff
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        RetryConfig {
            max_retries: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryConfig {
    pub fn calculate_delay(&self, retry_count: u32) -> u64 {
        let delay = self.initial_delay_ms as f64 * self.backoff_multiplier.powi(retry_count as i32);
        (delay as u64).min(self.max_delay_ms)
    }
}

/// Provider fallback configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub priority: u32,
    pub enabled: bool,
}

impl ProviderConfig {
    pub fn new(name: String, priority: u32) -> Self {
        ProviderConfig {
            name,
            priority,
            enabled: true,
        }
    }
}

/// Error analytics tracker
#[derive(Debug, Clone)]
pub struct ErrorAnalytics {
    pub total_errors: u64,
    pub errors_by_category: HashMap<AiErrorCategory, u64>,
    pub errors_by_provider: HashMap<String, u64>,
    pub successful_recoveries: u64,
    pub failed_recoveries: u64,
}

impl Default for ErrorAnalytics {
    fn default() -> Self {
        ErrorAnalytics {
            total_errors: 0,
            errors_by_category: HashMap::new(),
            errors_by_provider: HashMap::new(),
            successful_recoveries: 0,
            failed_recoveries: 0,
        }
    }
}

impl ErrorAnalytics {
    pub fn record_error(&mut self, error: &AiError) {
        self.total_errors += 1;
        *self
            .errors_by_category
            .entry(error.category.clone())
            .or_insert(0) += 1;
        *self
            .errors_by_provider
            .entry(error.provider.clone())
            .or_insert(0) += 1;
    }

    pub fn record_recovery(&mut self, success: bool) {
        if success {
            self.successful_recoveries += 1;
        } else {
            self.failed_recoveries += 1;
        }
    }

    pub fn recovery_rate(&self) -> f64 {
        let total = self.successful_recoveries + self.failed_recoveries;
        if total == 0 {
            0.0
        } else {
            self.successful_recoveries as f64 / total as f64
        }
    }
}

/// Main AI error handler
pub struct AiErrorHandler {
    retry_config: RetryConfig,
    providers: Vec<ProviderConfig>,
    analytics: Arc<RwLock<ErrorAnalytics>>,
}

impl AiErrorHandler {
    pub fn new() -> Self {
        let providers = vec![
            ProviderConfig::new("ollama".to_string(), 1),
            ProviderConfig::new("openai".to_string(), 2),
            ProviderConfig::new("anthropic".to_string(), 3),
        ];

        AiErrorHandler {
            retry_config: RetryConfig::default(),
            providers,
            analytics: Arc::new(RwLock::new(ErrorAnalytics::default())),
        }
    }

    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    pub fn with_providers(mut self, providers: Vec<ProviderConfig>) -> Self {
        self.providers = providers;
        self
    }

    /// Execute an AI operation with automatic retry and fallback
    pub async fn execute_with_recovery<F, Fut, T>(
        &self,
        operation: F,
        current_provider: &str,
    ) -> Result<T>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut last_error = None;
        let mut provider_index = self
            .providers
            .iter()
            .position(|p| p.name == current_provider)
            .unwrap_or(0);

        for retry_count in 0..=self.retry_config.max_retries {
            let provider = &self.providers[provider_index];

            if !provider.enabled {
                // Try next provider
                provider_index = (provider_index + 1) % self.providers.len();
                continue;
            }

            match operation(provider.name.clone()).await {
                Ok(result) => {
                    // Record successful recovery if this was a retry
                    if retry_count > 0 || provider_index > 0 {
                        let mut analytics = self.analytics.write().await;
                        analytics.record_recovery(true);
                    }
                    return Ok(result);
                }
                Err(e) => {
                    let error = self.classify_error(&e, &provider.name);
                    let mut analytics = self.analytics.write().await;
                    analytics.record_error(&error);

                    last_error = Some(error.clone());

                    // Check if we can retry
                    if !error.category.can_retry() || retry_count >= self.retry_config.max_retries {
                        // Try fallback provider
                        if error.category.should_fallback() {
                            provider_index = (provider_index + 1) % self.providers.len();
                            continue;
                        }
                        break;
                    }

                    // Exponential backoff
                    let delay = self.retry_config.calculate_delay(retry_count);
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
            }
        }

        // All retries and fallbacks failed
        let mut analytics = self.analytics.write().await;
        analytics.record_recovery(false);

        if let Some(error) = last_error {
            anyhow::bail!("AI operation failed after retries: {}", error.user_message);
        } else {
            anyhow::bail!("AI operation failed with unknown error");
        }
    }

    /// Classify an error into an AiError
    fn classify_error(&self, error: &anyhow::Error, provider: &str) -> AiError {
        let error_msg = error.to_string().to_lowercase();

        let (category, code) = if error_msg.contains("timeout") || error_msg.contains("timed out") {
            (AiErrorCategory::Network, "timeout".to_string())
        } else if error_msg.contains("connection") || error_msg.contains("connect") {
            (AiErrorCategory::Network, "connection_failed".to_string())
        } else if error_msg.contains("rate limit") || error_msg.contains("429") {
            (AiErrorCategory::Api, "rate_limit_exceeded".to_string())
        } else if error_msg.contains("auth")
            || error_msg.contains("401")
            || error_msg.contains("403")
        {
            (AiErrorCategory::Api, "auth_failed".to_string())
        } else if error_msg.contains("503") || error_msg.contains("unavailable") {
            (AiErrorCategory::Api, "service_unavailable".to_string())
        } else if error_msg.contains("parse") || error_msg.contains("invalid") {
            (AiErrorCategory::Validation, "parse_error".to_string())
        } else if error_msg.contains("filter") || error_msg.contains("safety") {
            (AiErrorCategory::Content, "content_filtered".to_string())
        } else {
            (AiErrorCategory::Unknown, "unknown".to_string())
        };

        AiError::new(category, code, error.to_string(), provider.to_string())
    }

    /// Get current analytics
    pub async fn get_analytics(&self) -> ErrorAnalytics {
        self.analytics.read().await.clone()
    }

    /// Reset analytics
    pub async fn reset_analytics(&self) {
        let mut analytics = self.analytics.write().await;
        *analytics = ErrorAnalytics::default();
    }

    /// Enable/disable a provider
    pub async fn set_provider_enabled(&mut self, provider_name: &str, enabled: bool) {
        if let Some(provider) = self.providers.iter_mut().find(|p| p.name == provider_name) {
            provider.enabled = enabled;
        }
    }

    /// Get available providers
    pub fn get_providers(&self) -> &[ProviderConfig] {
        &self.providers
    }
}

impl Default for AiErrorHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_category_from_code() {
        assert_eq!(
            AiErrorCategory::from_error_code("rate_limit_exceeded"),
            AiErrorCategory::Api
        );
        assert_eq!(
            AiErrorCategory::from_error_code("timeout"),
            AiErrorCategory::Network
        );
        assert_eq!(
            AiErrorCategory::from_error_code("parse_error"),
            AiErrorCategory::Validation
        );
        assert_eq!(
            AiErrorCategory::from_error_code("content_filtered"),
            AiErrorCategory::Content
        );
    }

    #[test]
    fn test_retry_config_delay_calculation() {
        let config = RetryConfig::default();
        assert_eq!(config.calculate_delay(0), 1000);
        assert_eq!(config.calculate_delay(1), 2000);
        assert_eq!(config.calculate_delay(2), 4000);
    }

    #[test]
    fn test_error_analytics() {
        let mut analytics = ErrorAnalytics::default();
        let error = AiError::new(
            AiErrorCategory::Network,
            "timeout".to_string(),
            "Connection timeout".to_string(),
            "ollama".to_string(),
        );

        analytics.record_error(&error);
        assert_eq!(analytics.total_errors, 1);
        assert_eq!(
            *analytics
                .errors_by_category
                .get(&AiErrorCategory::Network)
                .unwrap(),
            1
        );

        analytics.record_recovery(true);
        assert_eq!(analytics.successful_recoveries, 1);
        assert_eq!(analytics.recovery_rate(), 1.0);
    }

    #[test]
    fn test_provider_config() {
        let config = ProviderConfig::new("test".to_string(), 1);
        assert_eq!(config.name, "test");
        assert_eq!(config.priority, 1);
        assert!(config.enabled);
    }
}
