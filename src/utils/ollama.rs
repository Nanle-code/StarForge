//! Ollama local LLM provider for StarForge.
//!
//! Provides an HTTP client for communicating with a locally running Ollama
//! instance at `http://localhost:11434`, plus helpers for:
//!
//! - Auto-detecting whether Ollama is installed and running
//! - Listing available models
//! - Pulling (downloading) models
//! - Sending chat / generate requests with Soroban-optimised prompts
//! - Falling back to a cloud-provider suggestion when Ollama is unavailable

use crate::utils::{ai_cache, http_client::get_client};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default base URL for a locally running Ollama daemon.
pub const OLLAMA_BASE_URL: &str = "http://localhost:11434";

/// Recommended default model for Soroban contract analysis.
pub const DEFAULT_MODEL: &str = "codellama:7b";

// ─── Ollama API types ────────────────────────────────────────────────────────

/// A model reported by `GET /api/tags`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaModel {
    /// Full model name, e.g. `"codellama:7b"`.
    pub name: String,
    /// Size in bytes (optional – older Ollama builds may omit this).
    #[serde(default)]
    pub size: u64,
    /// Human-readable modification timestamp.
    #[serde(default)]
    pub modified_at: String,
    /// Digest / hash of the model blob.
    #[serde(default)]
    pub digest: String,
}

/// Response payload from `GET /api/tags`.
#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Vec<OllamaModel>,
}

/// Request body for `POST /api/generate`.
#[derive(Debug, Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<GenerateOptions>,
}

/// Tunable generation hyper-parameters.
#[derive(Debug, Serialize)]
pub struct GenerateOptions {
    /// Sampling temperature (0.0 – 1.0). Lower = more deterministic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Maximum tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_predict: Option<u32>,
    /// Context window size (tokens).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_ctx: Option<u32>,
}

/// Successful response from `POST /api/generate` (non-streaming).
#[derive(Debug, Serialize, Deserialize)]
pub struct GenerateResponse {
    /// The generated text.
    pub response: String,
    /// Whether the generation finished naturally.
    #[serde(default)]
    pub done: bool,
    /// Total duration in nanoseconds (if reported).
    #[serde(default)]
    pub total_duration: u64,
}

/// Request body for `POST /api/pull`.
#[derive(Debug, Serialize)]
struct PullRequest<'a> {
    name: &'a str,
    stream: bool,
}

/// A single status line returned by `POST /api/pull` (streaming).
#[derive(Debug, Deserialize)]
pub struct PullStatus {
    pub status: String,
    #[serde(default)]
    pub completed: u64,
    #[serde(default)]
    pub total: u64,
}

// ─── Detection helpers ────────────────────────────────────────────────────────

/// Returns `true` if the Ollama binary exists anywhere on `PATH`.
pub fn is_ollama_installed() -> bool {
    which_ollama().is_some()
}

/// Returns the path to the `ollama` binary if it is on `PATH`.
pub fn which_ollama() -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join("ollama");
            if candidate.is_file() {
                Some(candidate)
            } else {
                // On Windows the binary may have a `.exe` suffix.
                let candidate_exe = dir.join("ollama.exe");
                if candidate_exe.is_file() {
                    Some(candidate_exe)
                } else {
                    None
                }
            }
        })
    })
}

/// Performs a lightweight health-check against `GET /api/tags`.
///
/// Returns `true` when Ollama is responding, `false` otherwise.
pub async fn is_ollama_running() -> bool {
    // #902: shared client (proxy, custom CA bundle, user agent).
    let client = crate::utils::http_client::client_with_timeout(Duration::from_secs(3));

    client
        .get(format!("{}/api/tags", OLLAMA_BASE_URL))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Comprehensive status snapshot for the Ollama service.
#[derive(Debug)]
pub struct OllamaStatus {
    pub installed: bool,
    pub running: bool,
    pub binary_path: Option<std::path::PathBuf>,
    pub models: Vec<OllamaModel>,
}

/// Collects installation + runtime status in one call.
pub async fn get_status() -> OllamaStatus {
    let binary_path = which_ollama();
    let installed = binary_path.is_some();
    let running = is_ollama_running().await;
    let models = if running {
        list_models().await.unwrap_or_default()
    } else {
        vec![]
    };

    OllamaStatus {
        installed,
        running,
        binary_path,
        models,
    }
}

// ─── Model management ─────────────────────────────────────────────────────────

/// Returns the list of models currently available in the local Ollama store.
pub async fn list_models() -> Result<Vec<OllamaModel>> {
    let url = format!("{}/api/tags", OLLAMA_BASE_URL);
    let resp: TagsResponse = get_client()
        .get(&url)
        .send()
        .await
        .context("Failed to reach Ollama – is `ollama serve` running?")?
        .error_for_status()
        .context("Ollama returned an error response for /api/tags")?
        .json()
        .await
        .context("Failed to parse Ollama /api/tags response")?;
    Ok(resp.models)
}

/// Pulls a model by name, streaming progress lines back to a callback.
///
/// # Arguments
/// * `model_name` – e.g. `"codellama:7b"` or `"llama3"`.
/// * `on_progress` – called for each status line emitted during the pull.
pub async fn pull_model<F>(model_name: &str, mut on_progress: F) -> Result<()>
where
    F: FnMut(PullStatus),
{
    let url = format!("{}/api/pull", OLLAMA_BASE_URL);
    let body = PullRequest {
        name: model_name,
        stream: true,
    };

    let mut response = get_client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to reach Ollama for model pull")?
        .error_for_status()
        .context("Ollama returned an error while initiating model pull")?;

    // Stream NDJSON lines.
    let mut buf = String::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("Error reading Ollama pull stream")?
    {
        let text = String::from_utf8_lossy(&chunk);
        buf.push_str(&text);
        // Process complete lines.
        while let Some(nl) = buf.find('\n') {
            let line = buf[..nl].trim().to_string();
            buf = buf[nl + 1..].to_string();
            if line.is_empty() {
                continue;
            }
            if let Ok(status) = serde_json::from_str::<PullStatus>(&line) {
                on_progress(status);
            }
        }
    }
    let line = buf.trim();
    if !line.is_empty() {
        if let Ok(status) = serde_json::from_str::<PullStatus>(line) {
            on_progress(status);
        }
    }
    Ok(())
}

// ─── Generation / inference ───────────────────────────────────────────────────

/// Sends a plain text prompt to Ollama and returns the full generated response.
///
/// Uses non-streaming mode for simplicity; for long contracts consider
/// `generate_streaming` instead.
pub async fn generate(
    model: &str,
    prompt: &str,
    options: Option<GenerateOptions>,
) -> Result<GenerateResponse> {
    let url = format!("{}/api/generate", OLLAMA_BASE_URL);
    let body = GenerateRequest {
        model,
        prompt,
        stream: false,
        options,
    };

    // Use a longer timeout for LLM inference (default 30s may be too short).
    // #902: shared client (proxy, custom CA bundle, user agent).
    let client = crate::utils::http_client::client_with_timeout(Duration::from_secs(300));

    let resp: GenerateResponse = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to reach Ollama generate endpoint")?
        .error_for_status()
        .context("Ollama returned an error during generation")?
        .json()
        .await
        .context("Failed to parse Ollama generate response")?;

    Ok(resp)
}

/// Sends a prompt to Ollama with caching support.
///
/// First checks the cache for a matching request. If found and not expired,
/// returns the cached response. Otherwise, makes the request to Ollama,
/// stores the response in cache, and returns it.
pub async fn generate_cached(
    model: &str,
    prompt: &str,
    options: Option<GenerateOptions>,
    ttl_seconds: Option<u64>,
    tags: &str,
) -> Result<GenerateResponse> {
    use serde_json;

    // Try to open cache (may fail if database is locked, etc.)
    let mut cache = match ai_cache::AiCache::open() {
        Ok(cache) => cache,
        Err(e) => {
            tracing::warn!(
                "Failed to open AI cache, falling back to direct call: {}",
                e
            );
            return generate(model, prompt, options).await;
        }
    };

    // Generate cache key
    let options_json = options
        .as_ref()
        .map(|opts| serde_json::to_string(opts).unwrap_or_default())
        .unwrap_or_default();

    let cache_key = ai_cache::AiCache::generate_cache_key(model, prompt, &options_json);

    // Try to get from cache
    if let Some(entry) = cache.get(&cache_key)? {
        tracing::debug!("Cache hit for key: {}", cache_key);

        // Parse response from cache
        let response: GenerateResponse =
            serde_json::from_str(&entry.response).context("Failed to parse cached response")?;

        return Ok(response);
    }

    tracing::debug!(
        "Cache miss for key: {}, making request to Ollama",
        cache_key
    );

    // Make actual request
    let response = generate(model, prompt, options).await?;

    // Store in cache
    let response_json =
        serde_json::to_string(&response).context("Failed to serialize response for caching")?;

    let metadata = serde_json::json!({
        "total_duration": response.total_duration,
        "done": response.done,
        "cached_at": chrono::Utc::now().to_rfc3339(),
        "source": "ollama"
    })
    .to_string();

    let entry = ai_cache::AiCache::create_entry(
        model,
        prompt,
        &options_json,
        &response_json,
        &metadata,
        ttl_seconds,
        tags,
    );

    if let Err(e) = cache.put(entry) {
        tracing::warn!("Failed to store response in cache: {}", e);
    }

    Ok(response)
}

// ─── Soroban prompt engineering ───────────────────────────────────────────────

/// Pre-built system / task prompt prefixes tuned for Soroban smart-contract work.
pub mod prompts {
    /// System context injected before every user prompt.
    pub const SYSTEM_CONTEXT: &str = "\
You are an expert Stellar and Soroban smart-contract developer assistant integrated \
into the StarForge CLI. You help developers write, review, audit, and optimise \
Soroban contracts written in Rust. Always produce idiomatic Rust that compiles with \
soroban-sdk. Keep answers concise and actionable.\n\n";

    /// Wraps a user question with the Soroban system context.
    pub fn wrap_soroban(user_prompt: &str) -> String {
        format!("{}{}", SYSTEM_CONTEXT, user_prompt)
    }

    /// Prompt for auditing a Soroban contract for security issues.
    pub fn audit_prompt(contract_code: &str) -> String {
        format!(
            "{}Audit the following Soroban contract for security vulnerabilities, \
storage inefficiencies, and potential exploits. List each issue with its \
severity (Critical / High / Medium / Low) and a recommended fix.\n\n\
```rust\n{}\n```",
            SYSTEM_CONTEXT, contract_code
        )
    }

    /// Prompt for explaining what a Soroban contract does.
    pub fn explain_prompt(contract_code: &str) -> String {
        format!(
            "{}Explain what the following Soroban smart contract does in plain English. \
Include a summary of its storage model, entry-point functions, and any notable design \
patterns.\n\n```rust\n{}\n```",
            SYSTEM_CONTEXT, contract_code
        )
    }

    /// Prompt for translating text.
    pub fn translation_prompt(text: &str, target_lang: &str) -> String {
        format!(
            "{}Translate the following text into {}. Provide only the translation, no extra commentary.\n\n{}",
            SYSTEM_CONTEXT, target_lang, text
        )
    }

    /// Prompt for generating a test suite for a contract.
    pub fn test_prompt(contract_code: &str) -> String {
        format!(
            "{}Generate a comprehensive test suite for the following Soroban contract \
using the soroban-sdk testing harness. Cover happy paths, edge cases, and failure \
conditions.\n\n```rust\n{}\n```",
            SYSTEM_CONTEXT, contract_code
        )
    }

    /// Prompt for optimising a contract's gas usage.
    pub fn optimise_prompt(contract_code: &str) -> String {
        format!(
            "{}Identify gas optimisation opportunities in the following Soroban contract \
and rewrite it to minimise resource consumption while preserving behaviour.\n\n\
```rust\n{}\n```",
            SYSTEM_CONTEXT, contract_code
        )
    }

    /// Prompt for AI-driven performance profiling of a compiled Soroban contract.
    ///
    /// `profile_summary` is a JSON-serialised snapshot of the static analysis
    /// produced by `contract_profiler::profile_contract_wasm`, giving the model
    /// concrete numbers to reason about instead of raw byte code.
    pub fn performance_profile_prompt(profile_summary: &str) -> String {
        format!(
            "{}\
You have been given the following static performance profile of a compiled Soroban \
smart contract (in JSON). The metrics were produced by StarForge's WASM analyser and \
include estimated gas costs, instruction counts, memory usage, identified bottlenecks, \
and a regression comparison against a previous version where available.\n\n\
```json\n{}\n```\n\n\
Please provide a detailed AI-driven performance analysis that covers:\n\
1. **Bottleneck Identification** – explain each detected bottleneck in plain English, \
   ranking them by impact on on-chain cost and latency.\n\
2. **Optimization Suggestions** – give concrete, actionable Soroban/Rust code-level \
   changes the developer should make to reduce gas usage, lower memory pressure, and \
   improve execution time.\n\
3. **Comparative Analysis** – if a regression comparison is present, explain what \
   changed between the versions and whether the change is acceptable.\n\
4. **Historical Performance Advice** – recommend how the developer should track \
   performance over time (e.g. CI thresholds, baseline update cadence, key metrics \
   to watch).\n\
5. **Visual Report Guidance** – suggest the most important metrics and charts to \
   include in a performance dashboard for this contract type.\n\n\
Be specific and reference exact metric values from the profile. Keep the tone \
practical — developers should be able to act on your advice immediately.",
            SYSTEM_CONTEXT, profile_summary
        )
    }

    /// Prompt for AI contract pattern recognition and anti-pattern detection.
    ///
    /// `contract_code` is the raw Rust source.
    /// `pre_scan_json` is a JSON-serialised `PreScanResult` from the static
    /// indicator scan, giving the model a head-start before full analysis.
    /// `feedback_context` is an optional string injected from the user feedback
    /// store to calibrate confidence on patterns the user has already rated.
    pub fn pattern_recognition_prompt(
        contract_code: &str,
        pre_scan_json: &str,
        feedback_context: &str,
    ) -> String {
        format!(
            "{}\
Analyse the following Soroban smart contract for design patterns and anti-patterns.\n\n\
A static indicator scan has already been run and produced the following preliminary \
matches (JSON). Use these as hints but do not treat them as definitive — the LLM \
analysis should confirm, refine, or reject each match:\n\n\
```json\n{}\n```\n{}\n\n\
**Contract source:**\n```rust\n{}\n```\n\n\
Provide a structured analysis with the following sections:\n\n\
## Recognised Patterns\n\
List every design pattern present. For each include:\n\
- Pattern name and category (Token / Governance / DeFi / Access Control / Storage / General)\n\
- Confidence level (High / Medium / Low) with a one-sentence justification\n\
- Specific improvement suggestions tailored to this contract's implementation\n\n\
## Anti-Patterns Detected\n\
List every anti-pattern found. For each include:\n\
- Anti-pattern name, severity (Critical / High / Medium / Low), and category\n\
- Where in the code it appears (function name or line description)\n\
- Concrete remediation steps with example code where helpful\n\n\
## Pattern Documentation\n\
For the two highest-confidence pattern matches, provide a short documentation \
paragraph a developer could paste into the contract's README.\n\n\
## Actionable Improvements\n\
Rank the top 5 actionable improvements across all findings, ordered by impact.",
            SYSTEM_CONTEXT, pre_scan_json, feedback_context, contract_code
        )
    }

    /// Prompt asking the LLM to classify a contract into its primary pattern
    /// category without full analysis — useful for the `library` subcommand
    /// when browsing which patterns apply.
    pub fn pattern_classify_prompt(contract_code: &str) -> String {
        format!(
            "{}Given the following Soroban contract, classify it into one or more of these \
pattern categories: Token, Governance, DeFi, AccessControl, Storage, General.\n\n\
For each matching category, give a one-line reason.\n\n\
```rust\n{}\n```",
            SYSTEM_CONTEXT, contract_code
        )
    }

    /// Prompt for comparing two performance profiles and summarising the delta.
    ///
    /// `baseline_json` and `candidate_json` are both JSON-serialised
    /// `ContractProfileReport` snapshots.
    pub fn profile_comparison_prompt(baseline_json: &str, candidate_json: &str) -> String {
        format!(
            "{}\
Compare the following two Soroban contract performance profiles and explain the \
differences in plain English.\n\n\
**Baseline profile:**\n```json\n{}\n```\n\n\
**Candidate profile:**\n```json\n{}\n```\n\n\
Cover:\n\
1. Which metrics improved or regressed, and by how much.\n\
2. Whether the overall change is acceptable for production deployment.\n\
3. Root-cause hypotheses for any regressions (e.g. new storage writes, larger \
   data structures, additional host-function calls).\n\
4. Specific steps the developer should take before merging if regressions are found.",
            SYSTEM_CONTEXT, baseline_json, candidate_json
        )
    }

    /// Prompt for AI project planning enhancement.
    pub fn project_planning_prompt(plan_json: &str) -> String {
        format!(
            "{}\
Review the following Soroban project plan (JSON) and provide actionable \
recommendations. Cover:\n\
1. **Requirement gaps** — missing functional or non-functional requirements.\n\
2. **Architecture review** — validate the recommended architecture for this use case.\n\
3. **Timeline realism** — flag optimistic estimates and suggest adjustments.\n\
4. **Risk coverage** — identify risks not yet captured.\n\
5. **Testing & deployment** — suggest improvements to the testing and deployment strategy.\n\n\
```json\n{}\n```",
            SYSTEM_CONTEXT, plan_json
        )
    }

    /// Prompt for plain-language text simplification (accessibility).
    pub fn text_simplification_prompt(text: &str) -> String {
        format!(
            "{}\
Rewrite the following text in plain, simple language suitable for developers \
with cognitive disabilities or screen reader users. Use short sentences, common \
words, and clear structure. Preserve all technical meaning.\n\n\
{}",
            SYSTEM_CONTEXT, text
        )
    }
}

// ─── Cloud fallback ────────────────────────────────────────────────────────────

/// Suggestion message shown when Ollama is unavailable.
pub fn cloud_fallback_message() -> &'static str {
    "\
Ollama is not available on this machine. To use local AI features:\n\
  1. Install Ollama: https://ollama.ai/download\n\
  2. Start the daemon: ollama serve\n\
  3. Pull a model:    ollama pull codellama:7b\n\n\
Alternatively, consider cloud providers:\n\
  • OpenAI  – https://platform.openai.com\n\
  • Anthropic Claude – https://www.anthropic.com\n\
  • Google Gemini    – https://ai.google.dev"
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_is_non_empty() {
        assert!(!DEFAULT_MODEL.is_empty());
    }

    #[test]
    fn base_url_points_to_localhost() {
        assert!(OLLAMA_BASE_URL.contains("localhost"));
    }

    #[test]
    fn prompts_wrap_system_context() {
        let wrapped = prompts::wrap_soroban("What does storage_get do?");
        assert!(wrapped.contains(prompts::SYSTEM_CONTEXT));
        assert!(wrapped.contains("What does storage_get do?"));
    }

    #[test]
    fn audit_prompt_includes_contract_code() {
        let code = "#[contract] pub struct Token;";
        let prompt = prompts::audit_prompt(code);
        assert!(prompt.contains(code));
        assert!(prompt.to_lowercase().contains("audit"));
    }

    #[test]
    fn explain_prompt_includes_contract_code() {
        let code = "pub fn mint(env: Env) {}";
        let prompt = prompts::explain_prompt(code);
        assert!(prompt.contains(code));
    }

    #[test]
    fn test_prompt_includes_contract_code() {
        let code = "pub fn transfer(env: Env) {}";
        let prompt = prompts::test_prompt(code);
        assert!(prompt.contains(code));
    }

    #[test]
    fn optimise_prompt_includes_contract_code() {
        let code = "pub fn heavy(env: Env) {}";
        let prompt = prompts::optimise_prompt(code);
        assert!(prompt.contains(code));
    }

    #[test]
    fn performance_profile_prompt_includes_json_summary() {
        let summary = r#"{"id":"profile-abc","optimization_score":72}"#;
        let prompt = prompts::performance_profile_prompt(summary);
        assert!(prompt.contains(summary));
        assert!(prompt.to_lowercase().contains("bottleneck"));
        assert!(prompt.to_lowercase().contains("optimization"));
        assert!(prompt.contains(prompts::SYSTEM_CONTEXT));
    }

    #[test]
    fn profile_comparison_prompt_includes_both_profiles() {
        let baseline = r#"{"id":"profile-base"}"#;
        let candidate = r#"{"id":"profile-cand"}"#;
        let prompt = prompts::profile_comparison_prompt(baseline, candidate);
        assert!(prompt.contains(baseline));
        assert!(prompt.contains(candidate));
        assert!(prompt.to_lowercase().contains("compare"));
    }

    #[test]
    fn pattern_recognition_prompt_includes_all_inputs() {
        let code = "fn transfer(env: Env) {}";
        let scan = r#"{"matched_patterns":[]}"#;
        let feedback = "pattern 'sep41' confirmed correct 3 times";
        let prompt = prompts::pattern_recognition_prompt(code, scan, feedback);
        assert!(prompt.contains(code));
        assert!(prompt.contains(scan));
        assert!(prompt.contains(feedback));
        assert!(prompt.to_lowercase().contains("anti-pattern"));
        assert!(prompt.contains(prompts::SYSTEM_CONTEXT));
    }

    #[test]
    fn pattern_classify_prompt_includes_code() {
        let code = "fn mint(env: Env) {}";
        let prompt = prompts::pattern_classify_prompt(code);
        assert!(prompt.contains(code));
        assert!(prompt.to_lowercase().contains("categor"));
    }

    #[test]
    fn cloud_fallback_message_mentions_ollama() {
        let msg = cloud_fallback_message();
        assert!(msg.contains("ollama") || msg.contains("Ollama"));
    }

    #[test]
    fn generate_options_serialises_cleanly() {
        let opts = GenerateOptions {
            temperature: Some(0.1),
            num_predict: Some(512),
            num_ctx: Some(4096),
        };
        let json = serde_json::to_string(&opts).unwrap();
        assert!(json.contains("temperature"));
        assert!(json.contains("num_predict"));
        assert!(json.contains("num_ctx"));
    }

    #[test]
    fn ollama_model_deserialises_from_json() {
        let json = r#"{"name":"codellama:7b","size":3826793472,"modified_at":"2024-01-01T00:00:00Z","digest":"abc123"}"#;
        let model: OllamaModel = serde_json::from_str(json).unwrap();
        assert_eq!(model.name, "codellama:7b");
        assert_eq!(model.size, 3826793472);
    }

    #[test]
    fn ollama_model_deserialises_with_missing_optional_fields() {
        let json = r#"{"name":"llama3"}"#;
        let model: OllamaModel = serde_json::from_str(json).unwrap();
        assert_eq!(model.name, "llama3");
        assert_eq!(model.size, 0);
    }
}
