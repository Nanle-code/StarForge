//! AI usage telemetry and analytics (issue #482).
//!
//! Tracks AI API calls (provider/model/feature, token usage, latency, cost,
//! success/error) so users and maintainers can understand adoption,
//! performance and cost of AI features. Local-first: records are appended as
//! JSON lines under `~/.starforge/data/ai_telemetry.jsonl` and are never sent
//! anywhere unless the user explicitly opts in to cloud aggregation
//! (`ai_telemetry.cloud_aggregation_enabled` in config.toml, off by default).
//!
//! This module never stores prompt/response content — only call metadata —
//! so it stays consistent with StarForge's existing telemetry privacy stance
//! (see TELEMETRY_PRIVACY.md).

use crate::utils::config;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

/// A single recorded AI provider call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiCallRecord {
    pub timestamp: DateTime<Utc>,
    /// Provider name, e.g. "openai", "anthropic", "ollama".
    pub provider: String,
    /// Model name, e.g. "gpt-4o-mini", "claude-opus-4-1".
    pub model: String,
    /// Logical feature that triggered the call, e.g. "docs-generate",
    /// "security-audit", "error-explain", "contract-optimize".
    pub feature: String,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    pub latency_ms: u64,
    pub success: bool,
    /// Coarse error classification when `success` is false (e.g. "timeout",
    /// "rate_limit", "auth", "network", "invalid_response", "unknown").
    pub error_kind: Option<String>,
    pub cost_usd: Option<f64>,
}

/// Returns whether AI telemetry recording is currently enabled, honoring
/// (in priority order): the `STARFORGE_TELEMETRY` / `STARFORGE_AI_TELEMETRY`
/// env vars, then the per-feature `ai_telemetry.enabled` config flag, then
/// the global `telemetry_enabled` flag.
pub fn is_enabled() -> bool {
    for var in ["STARFORGE_AI_TELEMETRY", "STARFORGE_TELEMETRY"] {
        if let Ok(val) = std::env::var(var) {
            let disabled = matches!(
                val.to_lowercase().as_str(),
                "0" | "false" | "off" | "disabled" | "no"
            );
            if disabled {
                return false;
            }
        }
    }

    let cfg = match config::load() {
        Ok(c) => c,
        Err(_) => return true,
    };

    if !cfg.telemetry_enabled.unwrap_or(true) {
        return false;
    }
    cfg.ai_telemetry.enabled
}

fn telemetry_log_path() -> Result<PathBuf> {
    Ok(config::get_data_dir()?.join("ai_telemetry.jsonl"))
}

/// Static per-1K-token USD pricing table for known models. Returns `None`
/// for unknown/local models (e.g. Ollama, which is free to run locally).
fn price_per_1k_tokens(provider: &str, model: &str) -> Option<(f64, f64)> {
    let provider = provider.to_lowercase();
    let model = model.to_lowercase();

    // (input $/1K, output $/1K) — approximate published list prices.
    // More specific model names must precede substrings of themselves (e.g.
    // "gpt-4o-mini" before "gpt-4o") since lookup is first-match substring.
    let table: &[(&str, f64, f64)] = &[
        ("gpt-4o-mini", 0.00015, 0.0006),
        ("gpt-4o", 0.0025, 0.010),
        ("gpt-4-turbo", 0.010, 0.030),
        ("gpt-4", 0.030, 0.060),
        ("gpt-3.5-turbo", 0.0005, 0.0015),
        ("claude-opus-4-1", 0.015, 0.075),
        ("claude-3-5-sonnet", 0.003, 0.015),
        ("claude-3-5-haiku", 0.0008, 0.004),
        ("claude-3-opus", 0.015, 0.075),
        ("claude-3-haiku", 0.00025, 0.00125),
    ];

    if provider == "ollama" {
        return None; // local inference — no per-token API cost.
    }

    // Match the most specific model name, not the first one that happens to be
    // a substring: "gpt-4o-mini" contains "gpt-4o", and picking the shorter
    // entry would bill a mini call at full rates.
    table
        .iter()
        .filter(|(name, _, _)| model.contains(name))
        .max_by_key(|(name, _, _)| name.len())
        .map(|(_, input, output)| (*input, *output))
}

/// Estimate USD cost for a call given provider, model, and token counts.
pub fn estimate_cost(provider: &str, model: &str, tokens_in: u64, tokens_out: u64) -> Option<f64> {
    estimate_cost_usd(provider, model, Some(tokens_in), Some(tokens_out))
}

fn estimate_cost_usd(
    provider: &str,
    model: &str,
    tokens_in: Option<u64>,
    tokens_out: Option<u64>,
) -> Option<f64> {
    let (price_in, price_out) = price_per_1k_tokens(provider, model)?;
    let tin = tokens_in.unwrap_or(0) as f64;
    let tout = tokens_out.unwrap_or(0) as f64;
    Some((tin / 1000.0) * price_in + (tout / 1000.0) * price_out)
}

/// Records one AI provider call. No-ops silently (never fails the caller)
/// when telemetry is disabled or storage is unavailable, mirroring the
/// existing `telemetry::track_event` behavior.
#[allow(clippy::too_many_arguments)]
pub fn record_call(
    provider: &str,
    model: &str,
    feature: &str,
    tokens_in: Option<u64>,
    tokens_out: Option<u64>,
    latency_ms: u64,
    success: bool,
    error_kind: Option<&str>,
) {
    if !is_enabled() {
        return;
    }

    let cost_usd = estimate_cost_usd(provider, model, tokens_in, tokens_out);
    let record = AiCallRecord {
        timestamp: Utc::now(),
        provider: provider.to_string(),
        model: model.to_string(),
        feature: feature.to_string(),
        tokens_in,
        tokens_out,
        latency_ms,
        success,
        error_kind: error_kind.map(|s| s.to_string()),
        cost_usd,
    };

    let _ = append_record(&record);
}

fn append_record(record: &AiCallRecord) -> Result<()> {
    let path = telemetry_log_path()?;
    let line = serde_json::to_string(record)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", line)?;
    Ok(())
}

/// Loads all locally stored AI call records, optionally filtered to the last
/// `days` days.
pub fn load_records(days: Option<u32>) -> Result<Vec<AiCallRecord>> {
    let path = telemetry_log_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&path)?;
    let cutoff = days.map(|d| Utc::now() - chrono::Duration::days(d as i64));

    let records = content
        .lines()
        .filter_map(|line| serde_json::from_str::<AiCallRecord>(line).ok())
        .filter(|r| cutoff.map_or(true, |c| r.timestamp >= c))
        .collect();
    Ok(records)
}

/// Deletes all locally stored AI telemetry records.
pub fn reset() -> Result<()> {
    let path = telemetry_log_path()?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Persists the `ai_telemetry.enabled` opt-out/opt-in flag.
pub fn set_enabled(enabled: bool) -> Result<()> {
    let mut cfg = config::load()?;
    cfg.ai_telemetry.enabled = enabled;
    config::save(&cfg)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderStats {
    pub calls: u64,
    pub success: u64,
    pub errors: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyPercentiles {
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub p99_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTelemetrySummary {
    pub total_calls: u64,
    pub success_count: u64,
    pub error_count: u64,
    pub error_rate_pct: f64,
    pub total_tokens_in: u64,
    pub total_tokens_out: u64,
    pub total_cost_usd: f64,
    pub latency: LatencyPercentiles,
    pub by_provider: std::collections::BTreeMap<String, ProviderStats>,
    pub by_feature: std::collections::BTreeMap<String, u64>,
    pub by_error_kind: std::collections::BTreeMap<String, u64>,
}

fn percentile(sorted: &[u64], pct: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((pct / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

/// Aggregates a set of records into summary statistics: per-provider/model
/// call counts, token/cost totals, latency p50/p95/p99, error rates by type,
/// and feature usage frequency.
pub fn summarize(records: &[AiCallRecord]) -> AiTelemetrySummary {
    let mut by_provider: std::collections::BTreeMap<String, ProviderStats> =
        std::collections::BTreeMap::new();
    let mut by_feature: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut by_error_kind: std::collections::BTreeMap<String, u64> =
        std::collections::BTreeMap::new();
    let mut latencies: Vec<u64> = Vec::with_capacity(records.len());

    let mut success_count = 0u64;
    let mut error_count = 0u64;
    let mut total_tokens_in = 0u64;
    let mut total_tokens_out = 0u64;
    let mut total_cost_usd = 0.0f64;

    for r in records {
        latencies.push(r.latency_ms);

        let provider_key = format!("{}/{}", r.provider, r.model);
        let stats = by_provider.entry(provider_key).or_default();
        stats.calls += 1;
        stats.tokens_in += r.tokens_in.unwrap_or(0);
        stats.tokens_out += r.tokens_out.unwrap_or(0);
        stats.cost_usd += r.cost_usd.unwrap_or(0.0);

        *by_feature.entry(r.feature.clone()).or_insert(0) += 1;
        total_tokens_in += r.tokens_in.unwrap_or(0);
        total_tokens_out += r.tokens_out.unwrap_or(0);
        total_cost_usd += r.cost_usd.unwrap_or(0.0);

        if r.success {
            success_count += 1;
            stats.success += 1;
        } else {
            error_count += 1;
            stats.errors += 1;
            let kind = r
                .error_kind
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            *by_error_kind.entry(kind).or_insert(0) += 1;
        }
    }

    latencies.sort_unstable();
    let total_calls = records.len() as u64;
    let error_rate_pct = if total_calls > 0 {
        (error_count as f64 / total_calls as f64) * 100.0
    } else {
        0.0
    };

    AiTelemetrySummary {
        total_calls,
        success_count,
        error_count,
        error_rate_pct,
        total_tokens_in,
        total_tokens_out,
        total_cost_usd,
        latency: LatencyPercentiles {
            p50_ms: percentile(&latencies, 50.0),
            p95_ms: percentile(&latencies, 95.0),
            p99_ms: percentile(&latencies, 99.0),
        },
        by_provider,
        by_feature,
        by_error_kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(
        provider: &str,
        model: &str,
        feature: &str,
        latency_ms: u64,
        success: bool,
    ) -> AiCallRecord {
        AiCallRecord {
            timestamp: Utc::now(),
            provider: provider.to_string(),
            model: model.to_string(),
            feature: feature.to_string(),
            tokens_in: Some(100),
            tokens_out: Some(50),
            latency_ms,
            success,
            error_kind: if success {
                None
            } else {
                Some("timeout".to_string())
            },
            cost_usd: estimate_cost_usd(provider, model, Some(100), Some(50)),
        }
    }

    #[test]
    fn cost_estimation_known_model() {
        let cost = estimate_cost_usd("openai", "gpt-4o-mini", Some(1000), Some(1000)).unwrap();
        assert!((cost - (0.00015 + 0.0006)).abs() < 1e-9);
    }

    #[test]
    fn cost_estimation_unknown_provider_is_none() {
        assert!(estimate_cost_usd("ollama", "codellama:7b", Some(100), Some(50)).is_none());
    }

    #[test]
    fn percentile_empty_is_zero() {
        assert_eq!(percentile(&[], 95.0), 0);
    }

    #[test]
    fn percentile_basic() {
        let v = vec![10, 20, 30, 40, 50];
        assert_eq!(percentile(&v, 50.0), 30);
        assert_eq!(percentile(&v, 100.0), 50);
    }

    #[test]
    fn summarize_counts_and_rates() {
        let records = vec![
            rec("openai", "gpt-4o-mini", "docs-generate", 100, true),
            rec("openai", "gpt-4o-mini", "docs-generate", 200, true),
            rec("anthropic", "claude-opus-4-1", "security-audit", 300, false),
        ];
        let summary = summarize(&records);
        assert_eq!(summary.total_calls, 3);
        assert_eq!(summary.success_count, 2);
        assert_eq!(summary.error_count, 1);
        assert!((summary.error_rate_pct - 33.333333333333336).abs() < 1e-9);
        assert_eq!(summary.by_feature.get("docs-generate"), Some(&2));
        assert_eq!(summary.by_error_kind.get("timeout"), Some(&1));
        assert_eq!(summary.by_provider.len(), 2);
    }

    #[test]
    fn summarize_empty_records() {
        let summary = summarize(&[]);
        assert_eq!(summary.total_calls, 0);
        assert_eq!(summary.error_rate_pct, 0.0);
        assert_eq!(summary.latency.p50_ms, 0);
    }
}
