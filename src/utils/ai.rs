use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{Duration, Instant};

use crate::utils::http_client::get_client;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum AIProvider {
    OpenAI,
    Anthropic,
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIServiceConfig {
    pub default_provider: AIProvider,
    pub providers: HashMap<AIProvider, ProviderConfig>,
    pub fallback_order: Vec<AIProvider>,
    pub circuit_breaker_threshold: u32,
    pub circuit_breaker_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone)]
pub struct AIRequest {
    pub prompt: String,
    pub context: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIResponse {
    pub content: String,
    pub provider: AIProvider,
    pub tokens_used: Option<u32>,
    pub latency_ms: u64,
}

#[derive(Debug, Clone)]
pub enum CircuitState {
    Closed,
    Open { opened_at: Instant },
    HalfOpen,
}

#[derive(Debug)]
pub struct CircuitBreaker {
    failure_count: u32,
    threshold: u32,
    timeout: Duration,
    state: CircuitState,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, timeout_secs: u64) -> Self {
        Self {
            failure_count: 0,
            threshold,
            timeout: Duration::from_secs(timeout_secs),
            state: CircuitState::Closed,
        }
    }

    pub fn record_success(&mut self) {
        self.failure_count = 0;
        self.state = CircuitState::Closed;
    }

    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        if self.failure_count >= self.threshold {
            self.state = CircuitState::Open {
                opened_at: Instant::now(),
            };
        }
    }

    pub fn is_available(&mut self) -> bool {
        match &self.state {
            CircuitState::Closed => true,
            CircuitState::Open { opened_at } => {
                if opened_at.elapsed() >= self.timeout {
                    self.state = CircuitState::HalfOpen;
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }
}

#[async_trait]
pub trait AIService: Send + Sync {
    async fn generate_text(&self, request: &AIRequest) -> Result<AIResponse>;
    async fn analyze_code(&self, code: &str, language: &str) -> Result<AIResponse>;
    async fn suggest_improvements(&self, code: &str) -> Result<AIResponse>;
    fn provider_name(&self) -> AIProvider;
}

pub struct OpenAIAdapter {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl OpenAIAdapter {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            config,
            client: get_client().clone(),
        }
    }
}

#[async_trait]
impl AIService for OpenAIAdapter {
    async fn generate_text(&self, request: &AIRequest) -> Result<AIResponse> {
        let start = Instant::now();
        let api_key = self
            .config
            .api_key
            .as_ref()
            .context("OpenAI API key not configured")?;

        let mut messages = vec![serde_json::json!({
            "role": "system",
            "content": request.context.as_deref().unwrap_or("You are a Soroban smart contract expert.")
        })];
        messages.push(serde_json::json!({
            "role": "user",
            "content": &request.prompt
        }));

        let body = serde_json::json!({
            "model": &self.config.model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(self.config.max_tokens),
            "temperature": request.temperature.unwrap_or(0.7),
        });

        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.config.base_url))
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .timeout(Duration::from_secs(self.config.timeout_secs))
            .json(&body)
            .send()
            .await
            .context("Failed to send OpenAI request")?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .context("Failed to read OpenAI response")?;

        if !status.is_success() {
            anyhow::bail!("OpenAI API error ({}): {}", status, text);
        }

        let parsed: serde_json::Value =
            serde_json::from_str(&text).context("Failed to parse OpenAI response")?;

        let content = parsed["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let tokens = parsed["usage"]["total_tokens"].as_u64().map(|v| v as u32);

        Ok(AIResponse {
            content,
            provider: AIProvider::OpenAI,
            tokens_used: tokens,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn analyze_code(&self, code: &str, language: &str) -> Result<AIResponse> {
        let prompt = format!(
            "Analyze the following {} code for correctness, gas optimization, and Soroban compatibility:\n\n```{}\n{}\n```",
            language, language, code
        );
        self.generate_text(&AIRequest {
            prompt,
            context: Some("You are a Soroban smart contract auditor. Focus on security, gas efficiency, and correct use of the Soroban SDK.".into()),
            max_tokens: Some(2048),
            temperature: Some(0.3),
        })
        .await
    }

    async fn suggest_improvements(&self, code: &str) -> Result<AIResponse> {
        let prompt = format!(
            "Suggest improvements for this Soroban contract code:\n\n```rust\n{}\n```",
            code
        );
        self.generate_text(&AIRequest {
            prompt,
            context: Some(
                "You are a Rust and Soroban expert. Suggest concrete, actionable improvements."
                    .into(),
            ),
            max_tokens: Some(1024),
            temperature: Some(0.5),
        })
        .await
    }

    fn provider_name(&self) -> AIProvider {
        AIProvider::OpenAI
    }
}

pub struct AnthropicAdapter {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl AnthropicAdapter {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            config,
            client: get_client().clone(),
        }
    }
}

#[async_trait]
impl AIService for AnthropicAdapter {
    async fn generate_text(&self, request: &AIRequest) -> Result<AIResponse> {
        let start = Instant::now();
        let api_key = self
            .config
            .api_key
            .as_ref()
            .context("Anthropic API key not configured")?;

        let system = request
            .context
            .as_deref()
            .unwrap_or("You are a Soroban smart contract expert.");

        let body = serde_json::json!({
            "model": &self.config.model,
            "max_tokens": request.max_tokens.unwrap_or(self.config.max_tokens),
            "system": system,
            "messages": [{
                "role": "user",
                "content": &request.prompt
            }],
            "temperature": request.temperature.unwrap_or(0.7),
        });

        let resp = self
            .client
            .post(format!("{}/v1/messages", self.config.base_url))
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .timeout(Duration::from_secs(self.config.timeout_secs))
            .json(&body)
            .send()
            .await
            .context("Failed to send Anthropic request")?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .context("Failed to read Anthropic response")?;

        if !status.is_success() {
            anyhow::bail!("Anthropic API error ({}): {}", status, text);
        }

        let parsed: serde_json::Value =
            serde_json::from_str(&text).context("Failed to parse Anthropic response")?;

        let content = parsed["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let tokens = parsed["usage"]["output_tokens"].as_u64().map(|v| v as u32);

        Ok(AIResponse {
            content,
            provider: AIProvider::Anthropic,
            tokens_used: tokens,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn analyze_code(&self, code: &str, language: &str) -> Result<AIResponse> {
        let prompt = format!(
            "Analyze the following {} code for correctness, gas optimization, and Soroban compatibility:\n\n```{}\n{}\n```",
            language, language, code
        );
        self.generate_text(&AIRequest {
            prompt,
            context: Some("You are a Soroban smart contract auditor. Focus on security, gas efficiency, and correct use of the Soroban SDK.".into()),
            max_tokens: Some(2048),
            temperature: Some(0.3),
        })
        .await
    }

    async fn suggest_improvements(&self, code: &str) -> Result<AIResponse> {
        let prompt = format!(
            "Suggest improvements for this Soroban contract code:\n\n```rust\n{}\n```",
            code
        );
        self.generate_text(&AIRequest {
            prompt,
            context: Some(
                "You are a Rust and Soroban expert. Suggest concrete, actionable improvements."
                    .into(),
            ),
            max_tokens: Some(1024),
            temperature: Some(0.5),
        })
        .await
    }

    fn provider_name(&self) -> AIProvider {
        AIProvider::Anthropic
    }
}

pub struct OllamaAdapter {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl OllamaAdapter {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            config,
            client: get_client().clone(),
        }
    }
}

#[async_trait]
impl AIService for OllamaAdapter {
    async fn generate_text(&self, request: &AIRequest) -> Result<AIResponse> {
        let start = Instant::now();

        let body = serde_json::json!({
            "model": &self.config.model,
            "prompt": &request.prompt,
            "stream": false,
            "options": {
                "temperature": request.temperature.unwrap_or(0.7),
                "num_predict": request.max_tokens.unwrap_or(self.config.max_tokens),
            }
        });

        let resp = self
            .client
            .post(format!("{}/api/generate", self.config.base_url))
            .header("Content-Type", "application/json")
            .timeout(Duration::from_secs(self.config.timeout_secs))
            .json(&body)
            .send()
            .await
            .context("Failed to send Ollama request")?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .context("Failed to read Ollama response")?;

        if !status.is_success() {
            anyhow::bail!("Ollama API error ({}): {}", status, text);
        }

        let parsed: serde_json::Value =
            serde_json::from_str(&text).context("Failed to parse Ollama response")?;

        let content = parsed["response"].as_str().unwrap_or("").to_string();

        Ok(AIResponse {
            content,
            provider: AIProvider::Ollama,
            tokens_used: None,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn analyze_code(&self, code: &str, language: &str) -> Result<AIResponse> {
        let prompt = format!(
            "Analyze this {} code for correctness, gas optimization, and Soroban compatibility:\n\n```{}\n{}\n```",
            language, language, code
        );
        self.generate_text(&AIRequest {
            prompt,
            context: Some("You are a Soroban smart contract auditor.".into()),
            max_tokens: Some(2048),
            temperature: Some(0.3),
        })
        .await
    }

    async fn suggest_improvements(&self, code: &str) -> Result<AIResponse> {
        let prompt = format!(
            "Suggest improvements for this Soroban contract code:\n\n```rust\n{}\n```",
            code
        );
        self.generate_text(&AIRequest {
            prompt,
            context: Some("You are a Rust and Soroban expert.".into()),
            max_tokens: Some(1024),
            temperature: Some(0.5),
        })
        .await
    }

    fn provider_name(&self) -> AIProvider {
        AIProvider::Ollama
    }
}

pub struct AIServiceManager {
    providers: RwLock<HashMap<AIProvider, Arc<Mutex<Box<dyn AIService>>>>>,
    circuit_breakers: RwLock<HashMap<AIProvider, Arc<Mutex<CircuitBreaker>>>>,
    fallback_order: Vec<AIProvider>,
    provider_models: HashMap<AIProvider, String>,
}

fn provider_telemetry_name(provider: &AIProvider) -> &'static str {
    match provider {
        AIProvider::OpenAI => "openai",
        AIProvider::Anthropic => "anthropic",
        AIProvider::Ollama => "ollama",
    }
}

/// Coarse error classification for telemetry, derived from an `anyhow`
/// error's display string (the provider layer has no typed error enum).
fn classify_error_kind(err: &anyhow::Error) -> &'static str {
    let msg = err.to_string().to_lowercase();
    if msg.contains("timeout") || msg.contains("timed out") {
        "timeout"
    } else if msg.contains("429") || msg.contains("rate limit") {
        "rate_limit"
    } else if msg.contains("401") || msg.contains("403") || msg.contains("unauthorized") {
        "auth"
    } else if msg.contains("network") || msg.contains("connection") || msg.contains("dns") {
        "network"
    } else if msg.contains("parse") || msg.contains("invalid response") || msg.contains("json") {
        "invalid_response"
    } else {
        "unknown"
    }
}

impl AIServiceManager {
    pub fn new(config: &AIServiceConfig) -> Self {
        let mut providers = HashMap::new();
        let mut circuit_breakers = HashMap::new();
        let mut provider_models = HashMap::new();

        for (provider_type, provider_config) in &config.providers {
            let adapter: Box<dyn AIService> = match provider_type {
                AIProvider::OpenAI => Box::new(OpenAIAdapter::new(provider_config.clone())),
                AIProvider::Anthropic => Box::new(AnthropicAdapter::new(provider_config.clone())),
                AIProvider::Ollama => Box::new(OllamaAdapter::new(provider_config.clone())),
            };
            providers.insert(provider_type.clone(), Arc::new(Mutex::new(adapter)));
            provider_models.insert(provider_type.clone(), provider_config.model.clone());
            circuit_breakers.insert(
                provider_type.clone(),
                Arc::new(Mutex::new(CircuitBreaker::new(
                    config.circuit_breaker_threshold,
                    config.circuit_breaker_timeout_secs,
                ))),
            );
        }

        Self {
            providers: RwLock::new(providers),
            circuit_breakers: RwLock::new(circuit_breakers),
            fallback_order: config.fallback_order.clone(),
            provider_models,
        }
    }

    pub async fn generate_text(&self, request: &AIRequest) -> Result<AIResponse> {
        self.generate_text_for_feature(request, "generate_text")
            .await
    }

    pub async fn generate_text_for_feature(
        &self,
        request: &AIRequest,
        feature: &str,
    ) -> Result<AIResponse> {
        let providers = self.providers.read().await;
        let breakers = self.circuit_breakers.read().await;

        for provider_type in &self.fallback_order {
            let adapter = match providers.get(provider_type) {
                Some(a) => a,
                None => continue,
            };

            let mut breaker = match breakers.get(provider_type) {
                Some(b) => b.lock().await,
                None => continue,
            };

            if !breaker.is_available() {
                continue;
            }
            drop(breaker);

            let start = Instant::now();
            let adapter_lock = adapter.lock().await;
            let result = adapter_lock.generate_text(request).await;
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let model = self
                .provider_models
                .get(provider_type)
                .cloned()
                .unwrap_or_default();

            match result {
                Ok(response) => {
                    let mut breaker = breakers.get(provider_type).unwrap().lock().await;
                    breaker.record_success();
                    crate::utils::ai_telemetry::record_call(
                        provider_telemetry_name(provider_type),
                        &model,
                        feature,
                        None,
                        response.tokens_used.map(|t| t as u64),
                        elapsed_ms,
                        true,
                        None,
                    );
                    return Ok(response);
                }
                Err(e) => {
                    let mut breaker = breakers.get(provider_type).unwrap().lock().await;
                    breaker.record_failure();
                    crate::utils::ai_telemetry::record_call(
                        provider_telemetry_name(provider_type),
                        &model,
                        feature,
                        None,
                        None,
                        elapsed_ms,
                        false,
                        Some(classify_error_kind(&e)),
                    );
                    eprintln!("Provider {:?} failed: {}. Trying next...", provider_type, e);
                    continue;
                }
            }
        }

        anyhow::bail!("All AI providers failed or unavailable")
    }

    pub async fn analyze_code(&self, code: &str, language: &str) -> Result<AIResponse> {
        self.analyze_code_for_feature(code, language, "analyze_code")
            .await
    }

    pub async fn analyze_code_for_feature(
        &self,
        code: &str,
        language: &str,
        feature: &str,
    ) -> Result<AIResponse> {
        let providers = self.providers.read().await;
        let breakers = self.circuit_breakers.read().await;

        for provider_type in &self.fallback_order {
            let adapter = match providers.get(provider_type) {
                Some(a) => a,
                None => continue,
            };

            let mut breaker = match breakers.get(provider_type) {
                Some(b) => b.lock().await,
                None => continue,
            };

            if !breaker.is_available() {
                continue;
            }
            drop(breaker);

            let start = Instant::now();
            let adapter_lock = adapter.lock().await;
            let result = adapter_lock.analyze_code(code, language).await;
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let model = self
                .provider_models
                .get(provider_type)
                .cloned()
                .unwrap_or_default();

            match result {
                Ok(response) => {
                    let mut breaker = breakers.get(provider_type).unwrap().lock().await;
                    breaker.record_success();
                    crate::utils::ai_telemetry::record_call(
                        provider_telemetry_name(provider_type),
                        &model,
                        feature,
                        None,
                        response.tokens_used.map(|t| t as u64),
                        elapsed_ms,
                        true,
                        None,
                    );
                    return Ok(response);
                }
                Err(e) => {
                    let mut breaker = breakers.get(provider_type).unwrap().lock().await;
                    breaker.record_failure();
                    crate::utils::ai_telemetry::record_call(
                        provider_telemetry_name(provider_type),
                        &model,
                        feature,
                        None,
                        None,
                        elapsed_ms,
                        false,
                        Some(classify_error_kind(&e)),
                    );
                    eprintln!("Provider {:?} failed: {}. Trying next...", provider_type, e);
                    continue;
                }
            }
        }

        anyhow::bail!("All AI providers failed or unavailable")
    }

    pub async fn suggest_improvements(&self, code: &str) -> Result<AIResponse> {
        self.suggest_improvements_for_feature(code, "suggest_improvements")
            .await
    }

    pub async fn suggest_improvements_for_feature(
        &self,
        code: &str,
        feature: &str,
    ) -> Result<AIResponse> {
        let providers = self.providers.read().await;
        let breakers = self.circuit_breakers.read().await;

        for provider_type in &self.fallback_order {
            let adapter = match providers.get(provider_type) {
                Some(a) => a,
                None => continue,
            };

            let mut breaker = match breakers.get(provider_type) {
                Some(b) => b.lock().await,
                None => continue,
            };

            if !breaker.is_available() {
                continue;
            }
            drop(breaker);

            let start = Instant::now();
            let adapter_lock = adapter.lock().await;
            let result = adapter_lock.suggest_improvements(code).await;
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let model = self
                .provider_models
                .get(provider_type)
                .cloned()
                .unwrap_or_default();

            match result {
                Ok(response) => {
                    let mut breaker = breakers.get(provider_type).unwrap().lock().await;
                    breaker.record_success();
                    crate::utils::ai_telemetry::record_call(
                        provider_telemetry_name(provider_type),
                        &model,
                        feature,
                        None,
                        response.tokens_used.map(|t| t as u64),
                        elapsed_ms,
                        true,
                        None,
                    );
                    return Ok(response);
                }
                Err(e) => {
                    let mut breaker = breakers.get(provider_type).unwrap().lock().await;
                    breaker.record_failure();
                    crate::utils::ai_telemetry::record_call(
                        provider_telemetry_name(provider_type),
                        &model,
                        feature,
                        None,
                        None,
                        elapsed_ms,
                        false,
                        Some(classify_error_kind(&e)),
                    );
                    eprintln!("Provider {:?} failed: {}. Trying next...", provider_type, e);
                    continue;
                }
            }
        }

        anyhow::bail!("All AI providers failed or unavailable")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_starts_closed() {
        let mut cb = CircuitBreaker::new(3, 60);
        assert!(cb.is_available());
    }

    #[test]
    fn test_circuit_breaker_opens_after_threshold() {
        let mut cb = CircuitBreaker::new(3, 60);
        cb.record_failure();
        cb.record_failure();
        assert!(cb.is_available());
        cb.record_failure();
        assert!(!cb.is_available());
    }

    #[test]
    fn test_circuit_breaker_resets_on_success() {
        let mut cb = CircuitBreaker::new(3, 60);
        cb.record_failure();
        cb.record_failure();
        cb.record_success();
        assert!(cb.is_available());
        assert_eq!(cb.failure_count, 0);
    }

    #[test]
    fn test_provider_config_serialization() {
        let config = ProviderConfig {
            api_key: Some("test-key".into()),
            base_url: "https://api.openai.com".into(),
            model: "gpt-4".into(),
            max_tokens: 4096,
            timeout_secs: 30,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ProviderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "gpt-4");
    }
}
