//! Offline-first AI mode support and the cloud provider feature-parity matrix.
//!
//! Many contributors cannot use cloud AI (no API keys, air-gapped networks, or
//! budget constraints), so StarForge treats a local Ollama instance as a
//! first-class provider. This module is the single source of truth for:
//!
//! * **AI mode** – whether StarForge should run offline (Ollama-only), online
//!   (cloud providers allowed), or automatically detect what is available.
//! * **Parity matrix** – for every AI command / feature, whether it works
//!   offline with a local model, is cloud-only, or is hybrid (local when
//!   available, cloud otherwise).
//! * **Clear failures** – when an offline user requests a cloud-only feature,
//!   or a model that is not present locally, they get an actionable error
//!   instead of a confusing provider timeout.
//!
//! The parity matrix published here is also rendered by the
//! `starforge ai offline` command and documented in `docs/OFFLINE_AI.md`.

use crate::utils::ollama::{self, OllamaModel};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// The running AI mode, controlling whether cloud providers may be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiMode {
    /// Prefer local models; never call a cloud provider.
    Offline,
    /// Allow cloud providers (OpenAI / Anthropic).
    Online,
    /// Automatically detect: offline when Ollama is available, online otherwise.
    Auto,
}

impl Default for AiMode {
    fn default() -> Self {
        AiMode::Auto
    }
}

impl AiMode {
    /// True when cloud providers must never be contacted.
    pub fn is_offline(self) -> bool {
        matches!(self, AiMode::Offline)
    }

    /// True when cloud providers are permitted.
    pub fn allows_cloud(self) -> bool {
        !matches!(self, AiMode::Offline)
    }
}

impl std::fmt::Display for AiMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiMode::Offline => write!(f, "offline"),
            AiMode::Online => write!(f, "online"),
            AiMode::Auto => write!(f, "auto"),
        }
    }
}

/// Parse an AI mode from a user-supplied string.
///
/// Acceptance is case-insensitive and tolerant of common synonyms
/// (`cloud`, `local`, `on`, `off`).
pub fn parse_ai_mode(s: &str) -> Result<AiMode> {
    match s.to_ascii_lowercase().as_str() {
        "offline" | "local" | "off" => Ok(AiMode::Offline),
        "online" | "cloud" | "on" => Ok(AiMode::Online),
        "auto" | "automatic" | "" => Ok(AiMode::Auto),
        other => bail!(
            "Unknown AI mode '{}'. Use one of: offline, online, auto.",
            other
        ),
    }
}

/// Environment variable that forces a specific AI mode.
pub const AI_MODE_ENV: &str = "STARFORGE_AI_MODE";

/// Read the configured AI mode, honouring `$STARFORGE_AI_MODE` first.
///
/// Falls back to [`AiMode::Auto`] when the variable is unset or invalid.
pub fn configured_mode() -> AiMode {
    match std::env::var(AI_MODE_ENV) {
        Ok(raw) => parse_ai_mode(&raw).unwrap_or_default(),
        Err(_) => AiMode::Auto,
    }
}

/// Resolution of a preference against actual runtime availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedMode {
    /// Running against a local model; no cloud access.
    Offline,
    /// Running with cloud providers available.
    Online,
    /// Offline mode requested but no local model is available.
    OfflineDegraded,
}

impl ResolvedMode {
    /// True when cloud providers must never be contacted.
    pub fn is_offline(self) -> bool {
        matches!(self, ResolvedMode::Offline | ResolvedMode::OfflineDegraded)
    }

    /// True when cloud providers are permitted.
    pub fn allows_cloud(self) -> bool {
        !self.is_offline()
    }
}

impl std::fmt::Display for ResolvedMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolvedMode::Offline => write!(f, "offline"),
            ResolvedMode::Online => write!(f, "online"),
            ResolvedMode::OfflineDegraded => write!(f, "offline-degraded"),
        }
    }
}

/// Resolve a preferred [`AiMode`] into a concrete runtime mode.
///
/// * `Offline` is always honoured (it is the safe default for air-gapped use),
///   but is [`ResolvedMode::OfflineDegraded`] when no local model is present.
/// * `Online` resolves to [`ResolvedMode::Online`].
/// * `Auto` resolves offline when Ollama is available, otherwise online.
pub fn resolve_mode(preferred: AiMode, ollama_available: bool) -> ResolvedMode {
    match preferred {
        AiMode::Offline => {
            if ollama_available {
                ResolvedMode::Offline
            } else {
                ResolvedMode::OfflineDegraded
            }
        }
        AiMode::Online => ResolvedMode::Online,
        AiMode::Auto => {
            if ollama_available {
                ResolvedMode::Offline
            } else {
                ResolvedMode::Online
            }
        }
    }
}

/// Detect the available local backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalBackend {
    /// `true` when an Ollama daemon is responding on `localhost:11434`.
    pub ollama_running: bool,
}

/// Probe the environment for a local backend.
pub async fn detect_local_backend() -> LocalBackend {
    LocalBackend {
        ollama_running: ollama::is_ollama_running().await,
    }
}

/// Resolve the effective mode, probing the environment when in `Auto`.
pub async fn effective_mode() -> ResolvedMode {
    let preferred = configured_mode();
    let backend = detect_local_backend().await;
    resolve_mode(preferred, backend.ollama_running)
}

/// Resolve the configured mode to a concrete [`ResolvedMode`] without probing
/// the network.
///
/// Used by cloud-only command guards that must decide *before* any I/O whether
/// offline mode is active. `Auto` is conservatively treated as online here
/// because, absent a probe, we cannot prove a local backend exists — offline
/// mode (the explicit opt-in) is the only setting that guarantees no cloud
/// contact.
pub fn resolve_configured_mode_sync() -> ResolvedMode {
    match configured_mode() {
        AiMode::Offline => ResolvedMode::Offline,
        AiMode::Online | AiMode::Auto => ResolvedMode::Online,
    }
}

/// How a command is served in relation to cloud providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfflineSupport {
    /// Works fully offline with a local Ollama model.
    Local,
    /// Only works with a cloud provider (no offline fallback).
    CloudOnly,
    /// Works offline when a local model is present, cloud otherwise.
    Hybrid,
}

impl OfflineSupport {
    /// Whether the command can be satisfied without any cloud provider.
    pub fn works_offline(self) -> bool {
        matches!(self, OfflineSupport::Local | OfflineSupport::Hybrid)
    }
}

/// One row of the AI provider parity matrix.
#[derive(Debug, Clone)]
pub struct ParityEntry {
    /// The command path, e.g. `ai audit`.
    pub command: &'static str,
    /// Short human description of what the command does.
    pub description: &'static str,
    /// Offline / cloud classification.
    pub support: OfflineSupport,
    /// Providers that can service this command.
    pub providers: &'static [&'static str],
}

impl ParityEntry {
    /// True when the entry runs entirely on a local model.
    pub fn is_local(&self) -> bool {
        self.support == OfflineSupport::Local
    }

    /// True when the entry has no offline path.
    pub fn is_cloud_only(&self) -> bool {
        self.support == OfflineSupport::CloudOnly
    }
}

/// The authoritative AI feature-parity registry.
///
/// This is the single source of truth for the offline capability contract
/// rendered by `starforge ai offline` and published in `docs/OFFLINE_AI.md`.
/// Keep `docs/OFFLINE_AI.md` in sync when rows are added or removed.
pub const AI_PARITY: &[ParityEntry] = &[
    // ── Local assistant (`starforge ai`), fully offline ────────────────────
    ParityEntry {
        command: "ai status",
        description: "Show Ollama installation and runtime status",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai models",
        description: "List locally available Ollama models",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai pull",
        description: "Download a model into the local Ollama store",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai ask",
        description: "Ask the local LLM a free-form Soroban question",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai audit",
        description: "AI security audit of a Soroban contract source file",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai explain",
        description: "Plain-English explanation of a Soroban contract",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai test",
        description: "Generate a test suite for a Soroban contract",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai optimise",
        description: "Suggest gas optimisations for a Soroban contract",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai profile",
        description: "AI-driven performance profiling of a compiled WASM",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai compare-profiles",
        description: "AI comparative analysis between two profile snapshots",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai patterns",
        description: "Contract pattern recognition and anti-pattern detection",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai library",
        description: "Browse the built-in pattern / anti-pattern library",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai pattern-feedback",
        description: "Record feedback on a pattern recognition result",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai cache",
        description: "Manage the AI request cache",
        support: OfflineSupport::Local,
        providers: &["ollama"],
    },
    ParityEntry {
        command: "ai analytics",
        description: "Show AI test analytics",
        support: OfflineSupport::Local,
        providers: &["database"],
    },
    // ── Assistant command groups, offline when a local model is present ─────
    ParityEntry {
        command: "ai chat",
        description: "Interactive chat with the AI assistant",
        support: OfflineSupport::Hybrid,
        providers: &["ollama", "openai", "anthropic"],
    },
    ParityEntry {
        command: "ai test-gen",
        description: "Generate tests with AI",
        support: OfflineSupport::Hybrid,
        providers: &["ollama", "openai", "anthropic"],
    },
    ParityEntry {
        command: "ai property-test",
        description: "AI-assisted property testing",
        support: OfflineSupport::Hybrid,
        providers: &["ollama", "openai", "anthropic"],
    },
    ParityEntry {
        command: "ai recommend",
        description: "AI contract recommendations",
        support: OfflineSupport::Hybrid,
        providers: &["ollama", "openai", "anthropic"],
    },
    ParityEntry {
        command: "ai search",
        description: "AI-assisted code search",
        support: OfflineSupport::Hybrid,
        providers: &["ollama", "openai", "anthropic"],
    },
    ParityEntry {
        command: "ai plan",
        description: "AI project planning",
        support: OfflineSupport::Hybrid,
        providers: &["ollama", "openai", "anthropic"],
    },
    ParityEntry {
        command: "ai feedback",
        description: "AI feedback collection and analysis",
        support: OfflineSupport::Hybrid,
        providers: &["ollama", "openai", "anthropic"],
    },
    ParityEntry {
        command: "ai debug",
        description: "AI debugging assistant",
        support: OfflineSupport::Hybrid,
        providers: &["ollama", "openai", "anthropic"],
    },
    ParityEntry {
        command: "ai accessibility",
        description: "AI accessibility features",
        support: OfflineSupport::Hybrid,
        providers: &["ollama", "openai", "anthropic"],
    },
    // ── Multi-provider / model routing, cloud-capable ──────────────────────
    ParityEntry {
        command: "ai-model route",
        description: "Route a task to the optimal provider and model",
        support: OfflineSupport::Hybrid,
        providers: &["ollama", "openai", "anthropic"],
    },
    ParityEntry {
        command: "ai telemetry",
        description: "AI usage telemetry",
        support: OfflineSupport::Local,
        providers: &["database"],
    },
    // ── Security audit service with a static offline fallback ───────────────
    ParityEntry {
        command: "ai audit-service",
        description: "AI security audit service with static offline analyses",
        support: OfflineSupport::Hybrid,
        providers: &["ollama", "openai", "anthropic"],
    },
    // ── Cloud-only: no offline path (OpenAI, hard-coded endpoint) ───────────
    //
    // `generate` and `explain` call the OpenAI chat-completions endpoint
    // directly with an API key and have no local fallback. In offline mode
    // these must fail clearly instead of attempting a cloud round-trip.
    ParityEntry {
        command: "generate",
        description: "Generate a Soroban contract from a natural-language prompt",
        support: OfflineSupport::CloudOnly,
        providers: &["openai"],
    },
    ParityEntry {
        command: "explain",
        description: "Explain a Soroban contract using AI",
        support: OfflineSupport::CloudOnly,
        providers: &["openai"],
    },
];

/// Find the parity entry for a command path, if it is known.
pub fn parity_entry(command: &str) -> Option<&'static ParityEntry> {
    AI_PARITY.iter().find(|e| e.command == command)
}

/// True when a known command can run without any cloud provider.
pub fn works_offline(command: &str) -> bool {
    parity_entry(command)
        .map(|e| e.support.works_offline())
        .unwrap_or(false)
}

/// True when a known command has no offline path and requires a cloud provider.
pub fn is_cloud_only(command: &str) -> bool {
    parity_entry(command)
        .map(|e| e.support == OfflineSupport::CloudOnly)
        .unwrap_or(false)
}

/// All entries that work offline (for rendering the "supported offline" subset).
pub fn offline_commands() -> impl Iterator<Item = &'static ParityEntry> {
    AI_PARITY.iter().filter(|e| e.support.works_offline())
}

/// All entries that are cloud-only (no offline path).
pub fn cloud_only_commands() -> impl Iterator<Item = &'static ParityEntry> {
    AI_PARITY
        .iter()
        .filter(|e| e.support == OfflineSupport::CloudOnly)
}

/// Build a clear, actionable error for requesting a cloud-only feature offline.
///
/// Offline mode never talks to a cloud provider, so instead of a confusing
/// network timeout we fail fast with context and remediation.
pub fn cloud_only_error(command: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "'{command}' is a cloud-only AI feature and is unavailable in offline mode.\n\
         \n\
         Offline mode only talks to a local Ollama instance and never contacts a \
         cloud provider.\n\
         \n\
         To proceed you can either:\n\
         \x20 1. Run with cloud access:    starforge ai offline --mode online\n\
         \x20 2. Or unset $STARFORGE_AI_MODE when cloud providers are allowed.\n\
         \n\
         Run `starforge ai offline` to see which commands are available offline."
    )
}

/// Enforce that a command is available in the current mode.
///
/// Returns [`cloud_only_error`] when the resolved mode is offline (or
/// offline-degraded) and the command only works with a cloud provider.
pub fn require_offline_compatible(command: &str, mode: ResolvedMode) -> Result<()> {
    if mode.is_offline() && is_cloud_only(command) {
        return Err(cloud_only_error(command));
    }
    Ok(())
}

/// True when `model` is present in the locally installed model set.
///
/// Names are compared case-insensitively. A request that includes a tag
/// (e.g. `codellama:7b`) must match the full name exactly; a bare request
/// (e.g. `codellama`) matches any locally installed model with that name.
pub fn model_present(models: &[OllamaModel], model: &str) -> bool {
    let needle = model.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return false;
    }
    let bare = needle.split(':').next().unwrap_or(&needle);
    // A tag is a specific model — require an exact full-name match.
    if needle.contains(':') {
        return models.iter().any(|m| m.name.to_ascii_lowercase() == needle);
    }
    // No tag — match any installed model with that bare name.
    models
        .iter()
        .any(|m| m.name.to_ascii_lowercase().split(':').next() == Some(bare))
}

/// Build a clear error for a model that is not present locally.
pub fn model_unavailable_error(model: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "Model '{model}' is not available in the local Ollama store.\n\
         \n\
         Pull it first with:\n\
         \x20  starforge ai pull {model}\n\
         \n\
         Or list the installed models with `starforge ai models` and pick one of them."
    )
}

/// Ensure a model is present, else fail clearly (unavailable-model error).
pub fn ensure_model_available(models: &[OllamaModel], model: &str) -> Result<()> {
    if model_present(models, model) {
        Ok(())
    } else {
        Err(model_unavailable_error(model))
    }
}

/// Count of AI features by support tier, handy for summaries.
pub fn parity_summary() -> (usize, usize, usize) {
    let mut local = 0;
    let mut hybrid = 0;
    let mut cloud = 0;
    for e in AI_PARITY {
        match e.support {
            OfflineSupport::Local => local += 1,
            OfflineSupport::Hybrid => hybrid += 1,
            OfflineSupport::CloudOnly => cloud += 1,
        }
    }
    (local, hybrid, cloud)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_models() -> Vec<OllamaModel> {
        vec![
            OllamaModel {
                name: "codellama:7b".into(),
                size: 3826793472,
                modified_at: "2024-01-01T00:00:00Z".into(),
                digest: "abc".into(),
            },
            OllamaModel {
                name: "llama3".into(),
                size: 0,
                modified_at: "".into(),
                digest: "".into(),
            },
        ]
    }

    #[test]
    fn registry_has_entry_for_every_command() {
        for e in AI_PARITY {
            assert!(!e.command.is_empty());
            assert!(!e.description.is_empty());
            assert!(!e.providers.is_empty());
        }
    }

    #[test]
    fn registry_ships_some_local_commands() {
        let (local, hybrid, cloud) = parity_summary();
        assert!(local >= 5, "expected local commands, got {local}");
        assert!(local + hybrid + cloud == AI_PARITY.len());
    }

    #[test]
    fn ai_ask_is_offline_compatible() {
        assert!(works_offline("ai ask"));
        assert!(!is_cloud_only("ai ask"));
        assert_eq!(
            parity_entry("ai ask").unwrap().support,
            OfflineSupport::Local
        );
    }

    #[test]
    fn local_commands_reported_in_offline_subset() {
        let names: Vec<&str> = offline_commands().map(|e| e.command).collect();
        assert!(names.contains(&"ai audit"));
        assert!(names.contains(&"ai explain"));
        assert!(names.contains(&"ai test"));
        assert!(names.contains(&"ai optimise"));
    }

    #[test]
    fn unknown_command_is_not_cloud_only() {
        assert!(!is_cloud_only("ai completely-unknown"));
        assert!(!works_offline("ai completely-unknown"));
    }

    #[test]
    fn offline_mode_blocks_cloud_only_command() {
        let entry = AI_PARITY
            .iter()
            .find(|e| e.is_cloud_only())
            .expect("registry should include a cloud-only entry for coverage");
        let err = require_offline_compatible(entry.command, ResolvedMode::Offline);
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("cloud-only"), "unexpected message: {msg}");
        assert!(msg.contains(&entry.command));
    }

    #[test]
    fn cloud_only_command_allowed_in_online_mode() {
        let entry = AI_PARITY
            .iter()
            .find(|e| e.is_cloud_only())
            .expect("registry should include a cloud-only entry for coverage");
        assert!(require_offline_compatible(entry.command, ResolvedMode::Online).is_ok());
    }

    #[test]
    fn offline_local_command_never_blocked() {
        let entry = AI_PARITY
            .iter()
            .find(|e| e.is_local())
            .expect("registry should include a local entry");
        assert!(require_offline_compatible(entry.command, ResolvedMode::Offline).is_ok());
    }

    #[test]
    fn resolve_offline_with_ollama_is_offline() {
        assert_eq!(resolve_mode(AiMode::Offline, true), ResolvedMode::Offline);
        assert_eq!(
            resolve_mode(AiMode::Offline, false),
            ResolvedMode::OfflineDegraded
        );
    }

    #[test]
    fn resolve_auto_picks_offline_when_ollama_available() {
        assert_eq!(resolve_mode(AiMode::Auto, true), ResolvedMode::Offline);
        assert_eq!(resolve_mode(AiMode::Auto, false), ResolvedMode::Online);
    }

    #[test]
    fn resolve_online_is_always_online() {
        assert_eq!(resolve_mode(AiMode::Online, true), ResolvedMode::Online);
        assert_eq!(resolve_mode(AiMode::Online, false), ResolvedMode::Online);
    }

    #[test]
    fn degraded_is_offline_for_cloud_blocking() {
        assert!(ResolvedMode::OfflineDegraded.is_offline());
        assert!(!ResolvedMode::Online.is_offline());
    }

    #[test]
    fn parse_mode_variants() {
        assert_eq!(parse_ai_mode("offline").unwrap(), AiMode::Offline);
        assert_eq!(parse_ai_mode("LOCAL").unwrap(), AiMode::Offline);
        assert_eq!(parse_ai_mode("online").unwrap(), AiMode::Online);
        assert_eq!(parse_ai_mode("cloud").unwrap(), AiMode::Online);
        assert_eq!(parse_ai_mode("auto").unwrap(), AiMode::Auto);
        assert!(parse_ai_mode("bogus").is_err());
    }

    #[test]
    fn model_present_matches_exact_and_bare() {
        let models = sample_models();
        assert!(model_present(&models, "codellama:7b"));
        assert!(
            model_present(&models, "CODELLAMA:7b"),
            "match should be case-insensitive"
        );
        assert!(model_present(&models, "codellama"));
        assert!(model_present(&models, "llama3"));
    }

    #[test]
    fn model_present_rejects_missing() {
        let models = sample_models();
        assert!(!model_present(&models, "codellama:13b"));
        assert!(!model_present(&models, "mixtral"));
        assert!(!model_present(&models, ""));
    }

    #[test]
    fn ensure_model_available_errors_clearly_on_missing() {
        let models = sample_models();
        let err = ensure_model_available(&models, "mistral").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("mistral"));
        assert!(msg.contains("not available"));
        assert!(msg.contains("starforge ai pull mistral"));
    }

    #[test]
    fn ensure_model_available_ok_when_present() {
        let models = sample_models();
        assert!(ensure_model_available(&models, "llama3").is_ok());
    }
}
