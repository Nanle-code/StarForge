#![allow(clippy::items_after_test_module)]

use crate::utils::crypto;
use crate::utils::database;
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Validates that a string is a well-formed Stellar Ed25519 public key.
///
/// A valid Stellar public key:
/// - Starts with 'G'
/// - Is exactly 56 characters long
/// - Contains only valid base32 characters (A-Z, 2-7)
///
/// Returns `Ok(())` if the key is valid, or an error with a descriptive message.
pub fn validate_public_key(key: &str) -> Result<()> {
    if !key.starts_with('G') {
        anyhow::bail!(
            "Invalid public key: must start with 'G'.\n  \
             A valid Stellar public key looks like: GABC...XYZ (56 characters, starting with G)."
        );
    }

    if key.len() != 56 {
        anyhow::bail!(
            "Invalid public key: expected 56 characters, got {}.\n  \
             A valid Stellar public key is exactly 56 characters long.",
            key.len()
        );
    }

    // Validate base32 character set (A-Z, 2-7)
    if let Some(bad_char) = key.chars().find(|c| !matches!(c, 'A'..='Z' | '2'..='7')) {
        anyhow::bail!(
            "Invalid public key: contains invalid character '{}'.\n  \
             A valid Stellar public key uses only uppercase letters A-Z and digits 2-7.",
            bad_char
        );
    }
    Ok(())
}

/// Validates a Soroban contract ID.
/// Must start with 'C', be exactly 56 chars long, and use valid base32 chars.
pub fn validate_contract_id(id: &str) -> Result<()> {
    if !id.starts_with('C') {
        anyhow::bail!("Invalid contract ID: must start with 'C'.");
    }
    if id.len() != 56 {
        anyhow::bail!(
            "Invalid contract ID: expected 56 characters, got {}.",
            id.len()
        );
    }
    if let Some(bad_char) = id.chars().find(|c| !matches!(c, 'A'..='Z' | '2'..='7')) {
        anyhow::bail!(
            "Invalid contract ID: contains invalid character '{}'.",
            bad_char
        );
    }
    Ok(())
}

/// Validates a file path exists and optionally matches an extension.
pub fn validate_file_path(path: &std::path::Path, expected_ext: Option<&str>) -> Result<()> {
    if !path.exists() {
        anyhow::bail!("Path does not exist: {}", path.display());
    }
    if !path.is_file() {
        anyhow::bail!("Path is not a file: {}", path.display());
    }
    if let Some(ext) = expected_ext {
        if path.extension().and_then(|e| e.to_str()) != Some(ext) {
            anyhow::bail!("Invalid file type: expected '{}' extension.", ext);
        }
    }
    Ok(())
}

/// Validates network setting.
pub fn validate_network(network: &str) -> Result<()> {
    match network {
        "testnet" | "mainnet" | "docker-testnet" => Ok(()),
        _ => {
            let cfg = load()?;
            if cfg.networks.contains_key(network) {
                Ok(())
            } else {
                anyhow::bail!(
                    "Unsupported network '{}'. Use 'testnet', 'mainnet', 'docker-testnet', or a configured custom network.",
                    network
                )
            }
        }
    }
}

/// Validates a Stellar secret key or encrypted bundle.
pub fn validate_secret_key(secret: &str) -> Result<()> {
    if secret.contains(':') {
        let parts: Vec<&str> = secret.split(':').collect();
        // Accept:
        // - 3-part (legacy: salt:nonce:ciphertext)
        // - 5-part (KDF without p_cost: salt:nonce:ciphertext:mem:iterations)
        // - 6-part (KDF with p_cost: salt:nonce:ciphertext:mem:iterations:parallelism)
        if parts.len() != 3 && parts.len() != 5 && parts.len() != 6 {
            anyhow::bail!(
                "Invalid encrypted secret bundle format: expected 3, 5, or 6 parts, got {}",
                parts.len()
            );
        }

        // Validate base64 parts (first 3 parts are always base64)
        for part in parts.iter().take(3) {
            BASE64
                .decode(part)
                .map_err(|_| anyhow::anyhow!("Invalid base64 in encrypted secret bundle"))?;
        }

        // If 5 or 6-part bundle, validate KDF parameters are valid u32
        if parts.len() >= 5 {
            parts[3]
                .parse::<u32>()
                .map_err(|_| anyhow::anyhow!("Invalid KDF memory cost: must be a valid u32"))?;
            parts[4]
                .parse::<u32>()
                .map_err(|_| anyhow::anyhow!("Invalid KDF iteration count: must be a valid u32"))?;
        }
        if parts.len() == 6 {
            parts[5].parse::<u32>().map_err(|_| {
                anyhow::anyhow!("Invalid KDF parallelism factor: must be a valid u32")
            })?;
        }

        return Ok(());
    }

    if !secret.starts_with('S') {
        anyhow::bail!("Invalid secret key: must start with 'S'.");
    }
    if secret.len() != 56 {
        anyhow::bail!(
            "Invalid secret key: expected 56 characters, got {}.",
            secret.len()
        );
    }
    if let Some(bad_char) = secret.chars().find(|c| !matches!(c, 'A'..='Z' | '2'..='7')) {
        anyhow::bail!(
            "Invalid secret key: contains invalid character '{}'.",
            bad_char
        );
    }
    Ok(())
}

/// Validates that a network exists in the supplied configuration.
///
/// Pure: it only consults `cfg` and the built-in reserved names. It must never
/// fall back to reading the on-disk configuration — validating an in-memory
/// `Config` should not depend on (or mutate) global state, or the result would
/// differ between machines and between test runs.
pub fn validate_network_exists(cfg: &Config, network: &str) -> Result<()> {
    if cfg.networks.contains_key(network) || is_reserved_network(network) {
        return Ok(());
    }
    anyhow::bail!(
        "Unsupported network '{}'. Use 'testnet', 'mainnet', 'docker-testnet', or a network \
         configured in this config.",
        network
    )
}

/// Validates an amount string parses to a positive f64.
pub fn validate_amount(amount: &str) -> Result<f64> {
    let amt: f64 = amount
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid amount format: '{}'", amount))?;
    if amt.is_nan() || amt.is_infinite() {
        anyhow::bail!("Amount must be a finite number, got {}", amt);
    }
    if amt <= 0.0 {
        anyhow::bail!("Amount must be strictly positive, got {}", amt);
    }
    Ok(amt)
}

/// Validates a wallet name.
/// Must not be empty and must contain only alphanumeric chars, dashes, or underscores.
pub fn validate_wallet_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Wallet name cannot be empty.");
    }
    if let Some(bad_char) = name
        .chars()
        .find(|c| !c.is_alphanumeric() && *c != '-' && *c != '_')
    {
        anyhow::bail!("Invalid wallet name '{}': contains invalid character '{}'. Use alphanumeric, dash, or underscore.", name, bad_char);
    }
    Ok(())
}

/// Validates the full configuration schema and wallet entries.
pub fn validate_config(cfg: &Config) -> Result<()> {
    if cfg.version.is_empty() {
        anyhow::bail!("Config version is missing");
    }

    if cfg.network.trim().is_empty() {
        anyhow::bail!("Active network is not set");
    }

    validate_network_exists(cfg, &cfg.network)?;

    if cfg.networks.is_empty() {
        anyhow::bail!("No networks configured");
    }

    for (name, net_cfg) in &cfg.networks {
        validate_endpoint_url(
            &net_cfg.horizon_url,
            &format!("network '{}'.horizon_url", name),
        )?;
        if let Some(ref soroban_url) = net_cfg.soroban_rpc_url {
            validate_endpoint_url(soroban_url, &format!("network '{}'.soroban_rpc_url", name))?;
        }
        if let Some(ref friendbot_url) = net_cfg.friendbot_url {
            validate_endpoint_url(friendbot_url, &format!("network '{}'.friendbot_url", name))?;
        }
    }

    let mut seen_wallets = std::collections::HashSet::new();
    for wallet in &cfg.wallets {
        validate_wallet_name(&wallet.name)?;
        validate_public_key(&wallet.public_key)?;
        if let Some(ref secret) = wallet.secret_key {
            validate_secret_key(secret)?;
        }
        validate_network_exists(cfg, &wallet.network)?;
        if !seen_wallets.insert(wallet.name.as_str()) {
            anyhow::bail!(
                "Duplicate wallet name '{}': wallet names must be unique",
                wallet.name
            );
        }
    }

    for source in &cfg.plugin_trust.trusted_sources {
        validate_plugin_trust_source(source)?;
    }

    Ok(())
}

// ── Pure parsing / serialization / merging ───────────────────────────────────
//
// These functions never touch the filesystem or the database. Keeping them
// pure is what makes the configuration round trip testable across generated
// inputs (see `tests/config_property_tests.rs`).

/// Parse a configuration from a TOML document.
///
/// Unknown keys are ignored so a config written by a newer StarForge still
/// loads; missing keys fall back to their `#[serde(default)]`. The result is
/// **not** validated — call [`validate_config`] when the values must be sane.
pub fn parse_config_str(contents: &str) -> Result<Config> {
    toml::from_str(contents).context("Failed to parse configuration TOML")
}

/// Parse a configuration from a JSON document (the format used by the local
/// database and by `starforge config export`).
pub fn parse_config_json(contents: &str) -> Result<Config> {
    serde_json::from_str(contents).context("Failed to parse configuration JSON")
}

/// Serialize a configuration to a TOML document.
///
/// Fails if [`Config`]'s field order is ever changed so that a scalar follows a
/// table — see the note on the struct.
pub fn to_toml_string(config: &Config) -> Result<String> {
    toml::to_string_pretty(config).context("Failed to serialize configuration to TOML")
}

/// Serialize a configuration to a JSON document.
pub fn to_json_string(config: &Config) -> Result<String> {
    serde_json::to_string_pretty(config).context("Failed to serialize configuration to JSON")
}

/// A partial configuration layered on top of a base [`Config`].
///
/// Used for profile/environment overlays: `~/.config/starforge/config.toml`
/// provides the base, and an overlay (project-local file, CI environment)
/// supplies only what it wants to change.
///
/// `deny_unknown_fields` is deliberate: an overlay is hand-written, and a typo
/// like `netwrok = "mainnet"` silently deploying to the wrong network is worse
/// than a hard parse error.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConfigOverlay {
    /// Replaces the active network.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    /// Replaces the telemetry opt-in flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_enabled: Option<bool>,
    /// Replaces the wallet encryption KDF settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallet_encryption: Option<crypto::KdfOptions>,
    /// Replaces the feature-flag settings wholesale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_flags: Option<FeatureFlagsConfig>,
    /// Replaces the AI telemetry settings wholesale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_telemetry: Option<AiTelemetryConfig>,
    /// Replaces the plugin trust allowlist wholesale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_trust: Option<PluginTrustConfig>,
    /// Networks to add, or to replace by name.
    #[serde(default)]
    pub networks: HashMap<String, NetworkConfig>,
    /// Wallets to append. A name that already exists in the base is an error
    /// rather than a silent overwrite — wallets hold key material.
    #[serde(default)]
    pub wallets: Vec<WalletEntry>,
}

impl ConfigOverlay {
    /// True when the overlay would not change anything.
    pub fn is_empty(&self) -> bool {
        self == &ConfigOverlay::default()
    }
}

/// Parse a [`ConfigOverlay`] from TOML, rejecting unknown keys.
pub fn parse_overlay_str(contents: &str) -> Result<ConfigOverlay> {
    toml::from_str(contents).context("Failed to parse configuration overlay TOML")
}

/// Layer `overlay` on top of `base` and validate the result.
///
/// Precedence rules:
/// - scalars (`network`, `telemetry_enabled`, `wallet_encryption`) — overlay
///   wins when it sets them, base is kept otherwise;
/// - `feature_flags` / `plugin_trust` / `ai_telemetry` — replaced wholesale when
///   present, so a partially-specified table can never produce a half-merged
///   policy;
/// - `networks` — merged by key; an overlay entry replaces the base entry of
///   the same name;
/// - `wallets` — appended; a duplicate name is rejected;
/// - `version` and `install_id` — always taken from the base. They identify the
///   installation and its schema, and an overlay must not forge either.
///
/// The merged config is validated before it is returned, so a merge can never
/// produce a config that [`save`] would reject.
pub fn merge_configs(base: Config, overlay: ConfigOverlay) -> Result<Config> {
    let mut merged = base;

    for wallet in &overlay.wallets {
        if merged.wallets.iter().any(|w| w.name == wallet.name) {
            anyhow::bail!(
                "Overlay wallet '{}' already exists in the base configuration; \
                 rename it or remove it from the overlay",
                wallet.name
            );
        }
    }

    if let Some(network) = overlay.network {
        merged.network = network;
    }
    if let Some(telemetry) = overlay.telemetry_enabled {
        merged.telemetry_enabled = Some(telemetry);
    }
    if let Some(kdf) = overlay.wallet_encryption {
        merged.wallet_encryption = Some(kdf);
    }
    if let Some(flags) = overlay.feature_flags {
        merged.feature_flags = flags;
    }
    if let Some(ai_telemetry) = overlay.ai_telemetry {
        merged.ai_telemetry = ai_telemetry;
    }
    if let Some(trust) = overlay.plugin_trust {
        merged.plugin_trust = trust;
    }
    for (name, net) in overlay.networks {
        merged.networks.insert(name, net);
    }
    merged.wallets.extend(overlay.wallets);

    validate_config(&merged)?;
    Ok(merged)
}

fn validate_endpoint_url(url: &str, label: &str) -> Result<()> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        anyhow::bail!(
            "Invalid {}: must start with http:// or https:// (got '{}')",
            label,
            url
        )
    }
}

/// The persisted StarForge configuration.
///
/// **Field order matters.** TOML requires every scalar value of a table to be
/// emitted before any sub-table, so all scalars are declared first, then
/// tables, then arrays of tables. Reordering scalars below a table makes
/// [`to_toml_string`] fail at runtime. Deserialization is by key, so the order
/// is free to change without breaking existing config files.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: String,
    pub network: String,
    pub telemetry_enabled: Option<bool>,
    /// Optional per-install UUIDv4. Lazily created on first load.
    /// Stable identifier used for deterministic feature-flag bucketing.
    #[serde(default)]
    pub install_id: Option<String>,
    pub wallet_encryption: Option<crypto::KdfOptions>,
    #[serde(default)]
    pub networks: std::collections::HashMap<String, NetworkConfig>,
    #[serde(default)]
    pub plugin_trust: PluginTrustConfig,
    /// Feature flag system configuration.
    #[serde(default)]
    pub feature_flags: FeatureFlagsConfig,
    /// AI telemetry (usage analytics) configuration.
    #[serde(default)]
    pub ai_telemetry: AiTelemetryConfig,
    pub wallets: Vec<WalletEntry>,
}

/// Local knobs for the AI usage-telemetry system (issue #482).
///
/// This is separate from the generic CLI `telemetry_enabled` flag: disabling
/// generic telemetry also disables AI telemetry, but AI telemetry can be
/// opted out of independently while generic command telemetry stays on.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AiTelemetryConfig {
    /// Whether AI call metrics (provider/model/tokens/latency/cost) are
    /// recorded locally. Defaults to `true`.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Whether local AI telemetry may additionally be aggregated to a
    /// remote endpoint. Always opt-in, defaults to `false`.
    #[serde(default)]
    pub cloud_aggregation_enabled: bool,
    /// Optional endpoint used when `cloud_aggregation_enabled` is true.
    #[serde(default)]
    pub cloud_endpoint: Option<String>,
    /// How many days of local AI telemetry records are kept before pruning.
    #[serde(default = "default_ai_telemetry_retention_days")]
    pub retention_days: u32,
}

impl Default for AiTelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cloud_aggregation_enabled: false,
            cloud_endpoint: None,
            retention_days: 90,
        }
    }
}

fn default_ai_telemetry_retention_days() -> u32 {
    90
}

/// Top-level knobs for the local feature-flag system.
///
/// Settings here affect **how** the system behaves (metrics retention, whether
/// in-process telemetry is recorded at all). They do **not** override flag
/// states — those live in the SQLite database.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct FeatureFlagsConfig {
    /// Whether the CLI should record `exposure` (and `conversion` /
    /// `rejection`) metric events locally. Defaults to `true`.
    #[serde(default = "default_true")]
    pub metrics_enabled: bool,
    /// How many days of local metric rows are kept before pruning.
    /// Defaults to 30.
    #[serde(default = "default_metrics_retention_days")]
    pub metrics_retention_days: u32,
    /// Default user attribute values that should be present when evaluating
    /// flags without a richer context (e.g. during `info` rendering).
    #[serde(default)]
    pub default_attributes: std::collections::HashMap<String, String>,
}

impl Default for FeatureFlagsConfig {
    fn default() -> Self {
        Self {
            metrics_enabled: true,
            metrics_retention_days: 30,
            default_attributes: std::collections::HashMap::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_metrics_retention_days() -> u32 {
    30
}

fn default_version() -> String {
    "1".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct NetworkConfig {
    pub horizon_url: String,
    pub soroban_rpc_url: Option<String>,
    pub friendbot_url: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct PluginTrustConfig {
    /// Trusted plugin source allowlist entries. Entries may be domains
    /// (`plugins.example.com`) or URL prefixes (`https://plugins.example.com/releases/`).
    #[serde(default = "default_trusted_plugin_sources")]
    pub trusted_sources: Vec<String>,
}

impl Default for PluginTrustConfig {
    fn default() -> Self {
        Self {
            trusted_sources: default_trusted_plugin_sources(),
        }
    }
}

pub fn default_trusted_plugin_sources() -> Vec<String> {
    vec![
        "https://github.com/Nanle-code/starforge-*".to_string(),
        "https://github.com/StarForge-Labs/*".to_string(),
        "https://crates.io/crates/starforge-plugin-*".to_string(),
    ]
}

pub fn validate_plugin_trust_source(source: &str) -> Result<()> {
    let source = source.trim();
    if source.is_empty() {
        anyhow::bail!("Trusted plugin source cannot be empty");
    }
    if source.chars().any(char::is_whitespace) {
        anyhow::bail!("Trusted plugin source cannot contain whitespace");
    }

    let wildcard_count = source.matches('*').count();
    if wildcard_count > 1 || (wildcard_count == 1 && !source.ends_with('*')) {
        anyhow::bail!("Trusted plugin source may only use '*' as a trailing wildcard");
    }

    let without_wildcard = source.strip_suffix('*').unwrap_or(source);
    if without_wildcard.contains("://") {
        let scheme = without_wildcard
            .split_once("://")
            .map(|(scheme, _)| scheme.to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(scheme.as_str(), "http" | "https" | "git+https") {
            anyhow::bail!("Trusted plugin source URL must use http, https, or git+https scheme");
        }
        let after_scheme = without_wildcard
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or("");
        let host = after_scheme
            .split(['/', '?', '#'])
            .next()
            .unwrap_or("")
            .rsplit('@')
            .next()
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("");
        if host.is_empty() || host.starts_with('.') || host.ends_with('.') {
            anyhow::bail!("Trusted plugin source URL must include a valid host");
        }
        return Ok(());
    }

    let domain = without_wildcard.trim_start_matches("*.");
    if domain.contains('/')
        || domain.contains(':')
        || domain.starts_with('.')
        || domain.ends_with('.')
    {
        anyhow::bail!("Trusted plugin domain must be a domain name, not a path or URL fragment");
    }
    if domain.is_empty() || !domain.contains('.') {
        anyhow::bail!("Trusted plugin domain must include a dot, such as plugins.example.com");
    }
    if !domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    {
        anyhow::bail!("Trusted plugin domain contains invalid characters");
    }

    Ok(())
}

pub fn add_trusted_plugin_source(config: &mut Config, source: String) -> Result<bool> {
    validate_plugin_trust_source(&source)?;
    let source = source.trim().to_string();
    if config
        .plugin_trust
        .trusted_sources
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&source))
    {
        return Ok(false);
    }
    config.plugin_trust.trusted_sources.push(source);
    config
        .plugin_trust
        .trusted_sources
        .sort_by_key(|entry| entry.to_ascii_lowercase());
    Ok(true)
}

pub fn remove_trusted_plugin_source(config: &mut Config, source: &str) -> bool {
    let before = config.plugin_trust.trusted_sources.len();
    config
        .plugin_trust
        .trusted_sources
        .retain(|existing| !existing.eq_ignore_ascii_case(source.trim()));
    before != config.plugin_trust.trusted_sources.len()
}

pub fn reset_trusted_plugin_sources(config: &mut Config) {
    config.plugin_trust.trusted_sources = default_trusted_plugin_sources();
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct WalletEntry {
    pub name: String,
    pub public_key: String,
    pub secret_key: Option<String>,
    pub network: String,
    pub created_at: String,
    pub funded: bool,
    #[serde(default)]
    pub rotation_history: Vec<WalletRotationRecord>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct WalletRotationRecord {
    pub rotated_at: String,
    pub previous_public_key: String,
    pub previous_network: String,
    pub previous_funded: bool,
    /// The previous secret key (plaintext or encrypted bundle), preserved when
    /// `--backup` is passed to `wallet rotate`.  `None` when not requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_secret_key: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        let mut networks = HashMap::new();
        networks.insert(
            "testnet".to_string(),
            NetworkConfig {
                horizon_url: "https://horizon-testnet.stellar.org".to_string(),
                soroban_rpc_url: Some("https://soroban-testnet.stellar.org".to_string()),
                friendbot_url: Some("https://friendbot.stellar.org".to_string()),
                passphrase: Some("Test SDF Network ; September 2015".to_string()),
            },
        );
        networks.insert(
            "mainnet".to_string(),
            NetworkConfig {
                horizon_url: "https://horizon.stellar.org".to_string(),
                soroban_rpc_url: Some("https://mainnet.sorobanrpc.com".to_string()),
                friendbot_url: None,
                passphrase: Some("Public Global Stellar Network ; September 2015".to_string()),
            },
        );
        networks.insert(
            "docker-testnet".to_string(),
            NetworkConfig {
                horizon_url: "http://localhost:8000".to_string(),
                soroban_rpc_url: Some("http://localhost:8000/rpc".to_string()),
                friendbot_url: None,
                passphrase: Some("Test SDF Network ; September 2015".to_string()),
            },
        );

        Self {
            version: "1".to_string(),
            network: "testnet".to_string(),
            wallets: vec![],
            networks,
            plugin_trust: PluginTrustConfig::default(),
            telemetry_enabled: Some(true),
            wallet_encryption: None,
            install_id: None,
            feature_flags: FeatureFlagsConfig::default(),
            ai_telemetry: AiTelemetryConfig::default(),
        }
    }
}

/// The current (highest supported) config schema version.
pub const CURRENT_CONFIG_VERSION: &str = "1";

// ── Schema migration types ────────────────────────────────────────────────────

/// A structured error produced during config schema migration.
///
/// Separate from the generic `anyhow::Error` so callers can branch on the
/// reason and present user-friendly guidance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigMigrationError {
    /// The config declares a version that is newer than this binary supports.
    FromFuture { found: String, latest: &'static str },
    /// The config declares a version not in the migration registry.
    UnknownVersion { found: String },
    /// A migration step failed.
    StepFailed {
        from: String,
        to: String,
        reason: String,
    },
    /// Creating the pre-migration backup failed.
    BackupFailed { version: String, reason: String },
}

impl std::fmt::Display for ConfigMigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FromFuture { found, latest } => write!(
                f,
                "Config schema version '{found}' is newer than this binary supports (max \
                 '{latest}'). Please upgrade starforge: \
                 https://github.com/Nanle-code/StarForge/releases"
            ),
            Self::UnknownVersion { found } => write!(
                f,
                "Unrecognised config schema version '{found}'. Check that the config file has \
                 not been manually edited."
            ),
            Self::StepFailed { from, to, reason } => {
                write!(f, "Migration from config v{from} to v{to} failed: {reason}")
            }
            Self::BackupFailed { version, reason } => write!(
                f,
                "Failed to create backup of config v{version} before migration: {reason}. \
                 Migration aborted — your original config is unchanged."
            ),
        }
    }
}

impl std::error::Error for ConfigMigrationError {}

/// Summary returned by `run_config_migrations`, useful for tests and logging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// Version of the config before any migration was applied.
    pub from_version: String,
    /// Version of the config after all applicable steps ran.
    pub to_version: String,
    /// Ordered list of `(from, to)` step pairs that were applied.
    pub steps_applied: Vec<(String, String)>,
    /// Path to the pre-migration backup file, if one was written.
    pub backup_path: Option<std::path::PathBuf>,
}

impl MigrationReport {
    /// True when no migration steps were needed.
    pub fn is_no_op(&self) -> bool {
        self.steps_applied.is_empty()
    }
}

// ── Internal migration step registry ─────────────────────────────────────────
//
// To add a new schema version:
//   1. Bump `CURRENT_CONFIG_VERSION`.
//   2. Add a `fn migrate_vN_to_vM(config: &mut Config)` function below.
//   3. Push a `ConfigMigrationStep` into `MIGRATION_STEPS` in ascending order.

struct ConfigMigrationStep {
    from_version: &'static str,
    to_version: &'static str,
    apply: fn(&mut Config),
}

/// v0 → v1: populate the `version` field absent in early releases.
///
/// Pre-v1 configs were written without a `version` key; `serde(default)` fills
/// it with `""` on deserialization.
fn migrate_v0_to_v1(config: &mut Config) {
    config.version = "1".to_string();
}

const MIGRATION_STEPS: &[ConfigMigrationStep] = &[
    ConfigMigrationStep {
        from_version: "0",
        to_version: "1",
        apply: migrate_v0_to_v1,
    },
    // Future steps go here:
    // ConfigMigrationStep { from_version: "1", to_version: "2", apply: migrate_v1_to_v2 },
];

// ── Public migration entry point ──────────────────────────────────────────────

/// Migrate `config` from its declared version to [`CURRENT_CONFIG_VERSION`].
///
/// - Already-current configs are returned immediately (no backup, no I/O).
/// - A timestamped backup is written *before* the first step runs.
/// - Steps execute in ascending version order so multi-version gaps are handled.
///
/// # Errors
///
/// Returns a [`ConfigMigrationError`] (wrapped in `anyhow::Error`) when:
/// - The declared version is newer than `CURRENT_CONFIG_VERSION`.
/// - The declared version is not in the step registry.
/// - The backup write fails.
pub fn run_config_migrations(mut config: Config) -> Result<(Config, MigrationReport)> {
    let raw_version = if config.version.is_empty() {
        "0".to_string()
    } else {
        config.version.clone()
    };

    let report_from = raw_version.clone();

    if raw_version == CURRENT_CONFIG_VERSION {
        let report = MigrationReport {
            from_version: report_from.clone(),
            to_version: report_from,
            steps_applied: vec![],
            backup_path: None,
        };
        return Ok((config, report));
    }

    if is_version_newer(&raw_version, CURRENT_CONFIG_VERSION) {
        return Err(ConfigMigrationError::FromFuture {
            found: raw_version,
            latest: CURRENT_CONFIG_VERSION,
        }
        .into());
    }

    let first_step_idx = MIGRATION_STEPS
        .iter()
        .position(|s| s.from_version == raw_version.as_str())
        .ok_or_else(|| ConfigMigrationError::UnknownVersion {
            found: raw_version.clone(),
        })?;

    let backup_path =
        write_config_backup(&config).map_err(|e| ConfigMigrationError::BackupFailed {
            version: raw_version.clone(),
            reason: e.to_string(),
        })?;

    let mut steps_applied: Vec<(String, String)> = Vec::new();
    let mut current_version = raw_version.clone();

    for step in &MIGRATION_STEPS[first_step_idx..] {
        if current_version != step.from_version {
            break;
        }
        (step.apply)(&mut config);
        steps_applied.push((step.from_version.to_string(), step.to_version.to_string()));
        current_version = step.to_version.to_string();
        if current_version == CURRENT_CONFIG_VERSION {
            break;
        }
    }

    let report = MigrationReport {
        from_version: report_from,
        to_version: current_version,
        steps_applied,
        backup_path: Some(backup_path),
    };

    Ok((config, report))
}

/// Convenience wrapper that drops the [`MigrationReport`].
///
/// This preserves the signature expected by existing `load()` callers.
pub fn migrate_config(config: Config) -> Result<Config> {
    let (migrated, _report) = run_config_migrations(config)?;
    Ok(migrated)
}

/// Returns `true` when version string `a` is strictly greater than `b`.
///
/// Versions are compared component-by-component after splitting on `'.'`.
/// Non-numeric components are treated as `0`.
fn is_version_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> { s.split('.').map(|c| c.parse().unwrap_or(0)).collect() };
    let a_parts = parse(a);
    let b_parts = parse(b);
    let max_len = a_parts.len().max(b_parts.len());
    for i in 0..max_len {
        let av = a_parts.get(i).copied().unwrap_or(0);
        let bv = b_parts.get(i).copied().unwrap_or(0);
        if av > bv {
            return true;
        }
        if av < bv {
            return false;
        }
    }
    false
}

fn write_config_backup(config: &Config) -> Result<std::path::PathBuf> {
    let dir = config_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create config directory {:?}", dir))?;
    }
    let backup_path = dir.join(format!(
        "config.backup.v{}.{}.toml",
        config.version,
        chrono::Utc::now().timestamp(),
    ));
    let contents =
        toml::to_string_pretty(config).with_context(|| "Failed to serialize config for backup")?;
    fs::write(&backup_path, contents)
        .with_context(|| format!("Failed to write backup to {:?}", backup_path))?;
    Ok(backup_path)
}

// Keep the old name so `rollback_config` still compiles.
fn backup_config(config: &Config) -> Result<()> {
    write_config_backup(config).map(|_| ())
}

#[allow(dead_code)]
pub fn rollback_config(version: &str) -> Result<()> {
    let config_dir = config_dir();
    let backup_pattern = format!("config.backup.v{}", version);

    let mut backups: Vec<_> = fs::read_dir(&config_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&backup_pattern)
        })
        .collect();

    if backups.is_empty() {
        anyhow::bail!("No backup found for version '{}'", version);
    }

    // Sort by timestamp (newest first)
    backups.sort_by_key(|b| std::cmp::Reverse(b.file_name()));

    let latest_backup = &backups[0];
    let backup_path = latest_backup.path();

    fs::copy(&backup_path, config_path())
        .with_context(|| format!("Failed to restore backup from {:?}", backup_path))?;

    Ok(())
}

thread_local! {
    static TEST_CONFIG_DIR_OVERRIDE: std::cell::RefCell<Option<PathBuf>> = std::cell::RefCell::new(None);
}

pub fn set_test_config_dir(path: PathBuf) {
    TEST_CONFIG_DIR_OVERRIDE.with(|p| {
        *p.borrow_mut() = Some(path);
    });
}

/// Environment variable that relocates the StarForge config directory.
///
/// `set_test_config_dir` only affects the calling thread, so it cannot isolate
/// a `starforge` binary spawned as a subprocess. Integration tests that shell
/// out need an out-of-process handle, and so do users who keep StarForge state
/// somewhere other than `~/.starforge`.
///
/// This matters most on Windows: `dirs::home_dir()` there resolves through
/// `SHGetKnownFolderPath(FOLDERID_Profile)` and deliberately ignores `HOME`
/// and `USERPROFILE`, so tests that set those env vars still share one real
/// config directory (and one SQLite database) across concurrent processes.
pub const CONFIG_DIR_ENV: &str = "STARFORGE_CONFIG_DIR";

pub fn config_dir() -> PathBuf {
    if let Some(path) = TEST_CONFIG_DIR_OVERRIDE.with(|p| p.borrow().clone()) {
        return path;
    }
    if let Some(dir) = std::env::var_os(CONFIG_DIR_ENV) {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let home = dirs::home_dir().expect("Could not find home directory");
    home.join(".starforge")
}

pub fn get_data_dir() -> Result<PathBuf> {
    let dir = config_dir().join("data");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

pub fn get_config_path() -> Result<PathBuf> {
    Ok(config_path())
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn load() -> Result<Config> {
    let db = database::Database::open()?;
    db.initialize()?;

    let mut config = if db.has_config()? {
        db.load_config()?
    } else {
        let path = config_path();
        let cfg = if path.exists() {
            let mut toml_cfg = parse_config_file()?;
            toml_cfg = migrate_config(toml_cfg)?;
            toml_cfg
        } else {
            Config::default()
        };
        db.save_config(&cfg)?;
        cfg
    };

    config = migrate_config(config)?;

    ensure_default_networks(&mut config);

    match config.install_id.as_deref() {
        None => {
            config.install_id = Some(crate::utils::feature_flags::load_or_create_install_id(&db)?);
        }
        Some(install_id) => {
            // Make sure install_id is also persisted to config_kv. If we loaded
            // from a TOML file (the legacy path) the column will be missing.
            let _ = db.insert_config_kv("install_id", install_id);
        }
    }

    if config.version != CURRENT_CONFIG_VERSION {
        save(&config)?;
    } else {
        db.save_config(&config)?;
    }

    Ok(config)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DoctorStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone)]
pub struct DoctorFinding {
    pub category: &'static str,
    pub status: DoctorStatus,
    pub message: String,
}

impl DoctorFinding {
    pub fn pass(category: &'static str, message: impl Into<String>) -> Self {
        Self {
            category,
            status: DoctorStatus::Pass,
            message: message.into(),
        }
    }

    pub fn fail(category: &'static str, message: impl Into<String>) -> Self {
        Self {
            category,
            status: DoctorStatus::Fail,
            message: message.into(),
        }
    }
}

/// Read and parse `config.toml` without migration or default-network injection.
pub fn parse_config_file() -> Result<Config> {
    let path = config_path();
    if !path.exists() {
        return Ok(Config::default());
    }
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config at {}", path.display()))?;
    toml::from_str(&contents).with_context(|| "Failed to parse config.toml")
}

fn validate_service_url(url: &str, label: &str) -> Result<()> {
    if url.trim().is_empty() {
        anyhow::bail!("{label} cannot be empty");
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        anyhow::bail!("{label} must use http or https");
    }
    Ok(())
}

/// Run structural validation checks against a loaded configuration.
pub fn validate_config_integrity(cfg: &Config) -> Vec<DoctorFinding> {
    let mut findings = Vec::new();

    if cfg.version == CURRENT_CONFIG_VERSION {
        findings.push(DoctorFinding::pass(
            "schema",
            format!("config version is {}", cfg.version),
        ));
    } else {
        findings.push(DoctorFinding::fail(
            "schema",
            format!(
                "unsupported config version '{}' (expected {})",
                cfg.version, CURRENT_CONFIG_VERSION
            ),
        ));
    }

    match validate_network_exists(cfg, &cfg.network) {
        Ok(()) => findings.push(DoctorFinding::pass(
            "network",
            format!("active network '{}' is configured", cfg.network),
        )),
        Err(e) => findings.push(DoctorFinding::fail("network", e.to_string())),
    }

    if cfg.wallets.is_empty() {
        findings.push(DoctorFinding::pass("wallet", "no wallets configured"));
    } else {
        let mut wallet_ok = true;
        let mut wallet_errors = Vec::new();
        for wallet in &cfg.wallets {
            let label = format!("wallet '{}'", wallet.name);
            if let Err(e) = validate_wallet_name(&wallet.name) {
                wallet_ok = false;
                wallet_errors.push(format!("{label}: {e}"));
            }
            if let Err(e) = validate_public_key(&wallet.public_key) {
                wallet_ok = false;
                wallet_errors.push(format!("{label} public key: {e}"));
            }
            if let Some(ref secret) = wallet.secret_key {
                if let Err(e) = validate_secret_key(secret) {
                    wallet_ok = false;
                    wallet_errors.push(format!("{label} secret key: {e}"));
                }
            }
            if let Err(e) = validate_network_exists(cfg, &wallet.network) {
                wallet_ok = false;
                wallet_errors.push(format!("{label} network: {e}"));
            }
        }
        if wallet_ok {
            findings.push(DoctorFinding::pass(
                "wallet",
                format!("{} wallet(s) validated", cfg.wallets.len()),
            ));
        } else {
            findings.push(DoctorFinding::fail("wallet", wallet_errors.join("; ")));
        }
    }

    let mut network_ok = true;
    let mut network_errors = Vec::new();
    for (name, net) in &cfg.networks {
        if let Err(e) = validate_service_url(&net.horizon_url, "horizon_url") {
            network_ok = false;
            network_errors.push(format!("network '{name}': {e}"));
        }
        if let Some(ref rpc) = net.soroban_rpc_url {
            if let Err(e) = validate_service_url(rpc, "soroban_rpc_url") {
                network_ok = false;
                network_errors.push(format!("network '{name}' soroban RPC: {e}"));
            }
        }
    }
    if network_ok {
        findings.push(DoctorFinding::pass(
            "network",
            format!("{} network(s) have valid endpoint URLs", cfg.networks.len()),
        ));
    } else {
        findings.push(DoctorFinding::fail("network", network_errors.join("; ")));
    }

    let mut trust_ok = true;
    let mut trust_errors = Vec::new();
    for source in &cfg.plugin_trust.trusted_sources {
        if let Err(e) = validate_plugin_trust_source(source) {
            trust_ok = false;
            trust_errors.push(format!("'{source}': {e}"));
        }
    }
    if trust_ok {
        findings.push(DoctorFinding::pass(
            "plugin_trust",
            format!(
                "{} trusted plugin source(s) validated",
                cfg.plugin_trust.trusted_sources.len()
            ),
        ));
    } else {
        findings.push(DoctorFinding::fail("plugin_trust", trust_errors.join("; ")));
    }

    if let Some(ref kdf) = cfg.wallet_encryption {
        let mut enc_ok = true;
        let mut enc_errors = Vec::new();
        for (field, value) in [
            ("mem", kdf.mem),
            ("iterations", kdf.iterations),
            ("parallelism", kdf.parallelism),
        ] {
            if let Some(v) = value {
                if v == 0 {
                    enc_ok = false;
                    enc_errors.push(format!("{field} must be > 0"));
                }
            }
        }
        if enc_ok {
            findings.push(DoctorFinding::pass(
                "encryption",
                "wallet encryption parameters are valid",
            ));
        } else {
            findings.push(DoctorFinding::fail("encryption", enc_errors.join("; ")));
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_public_key() {
        // Well-formed Stellar public key (56 chars, starts with G, valid base32)
        let key = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWNT";
        assert!(validate_public_key(key).is_ok());
    }

    #[test]
    fn test_rejects_key_not_starting_with_g() {
        let key = "SAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN";
        let err = validate_public_key(key).unwrap_err();
        assert!(err.to_string().contains("must start with 'G'"));
    }

    #[test]
    fn test_rejects_key_wrong_length() {
        let key = "GAAZI4TCR3TY5";
        let err = validate_public_key(key).unwrap_err();
        assert!(err.to_string().contains("expected 56 characters"));
    }

    #[test]
    fn test_rejects_key_invalid_characters() {
        // Lowercase letters are not valid base32
        let key = "Gaazi4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWNT";
        let err = validate_public_key(key).unwrap_err();
        assert!(err.to_string().contains("invalid character"));
    }

    #[test]
    fn test_rejects_empty_key() {
        let err = validate_public_key("").unwrap_err();
        assert!(err.to_string().contains("must start with 'G'"));
    }

    #[test]
    fn test_valid_contract_id() {
        let id = "CAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWNW";
        assert!(validate_contract_id(id).is_ok());
    }

    #[test]
    fn test_rejects_contract_id_not_starting_with_c() {
        // Starts with 'G'
        let id = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWNW";
        let err = validate_contract_id(id).unwrap_err();
        assert!(err.to_string().contains("must start with 'C'"));
    }

    #[test]
    fn test_valid_amount() {
        assert_eq!(validate_amount("10.5").unwrap(), 10.5);
        assert_eq!(validate_amount("1").unwrap(), 1.0);
    }

    #[test]
    fn test_invalid_amount() {
        assert!(validate_amount("-5").is_err());
        assert!(validate_amount("0").is_err());
        assert!(validate_amount("abc").is_err());
    }

    #[test]
    fn test_valid_wallet_name() {
        assert!(validate_wallet_name("alice-123_DEPLOY").is_ok());
    }

    #[test]
    fn test_invalid_wallet_name() {
        assert!(validate_wallet_name("").is_err());
        assert!(validate_wallet_name("alice!").is_err());
        assert!(validate_wallet_name("my wallet").is_err());
    }

    #[test]
    fn test_valid_plain_secret_key() {
        let Ok(secret) = std::env::var("STARFORGE_TEST_SECRET_KEY") else {
            eprintln!("skipping test_valid_plain_secret_key: STARFORGE_TEST_SECRET_KEY is not set");
            return;
        };
        assert!(validate_secret_key(&secret).is_ok());
    }

    #[test]
    fn test_valid_encrypted_secret_bundle() {
        let salt = BASE64.encode([0u8; 16]);
        let nonce = BASE64.encode([1u8; 12]);
        let cipher = BASE64.encode([2u8; 32]);
        let bundle = format!("{}:{}:{}", salt, nonce, cipher);
        assert!(validate_secret_key(&bundle).is_ok());

        // 5-part
        let bundle_5 = format!("{}:{}:{}:32768:4", salt, nonce, cipher);
        assert!(validate_secret_key(&bundle_5).is_ok());

        // 6-part
        let bundle_6 = format!("{}:{}:{}:32768:4:2", salt, nonce, cipher);
        assert!(validate_secret_key(&bundle_6).is_ok());
    }

    #[test]
    fn test_invalid_secret_key() {
        assert!(validate_secret_key("not-a-key").is_err());
        assert!(validate_secret_key("S123").is_err());
        assert!(validate_secret_key("bad:bundle").is_err());
    }

    #[test]
    fn validate_config_accepts_default_config() {
        let cfg = Config::default();
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn validate_config_rejects_missing_active_network() {
        let cfg = Config {
            network: "unknown-net".to_string(),
            ..Default::default()
        };
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("unknown-net"));
    }

    #[test]
    fn validate_config_rejects_invalid_horizon_url() {
        let mut cfg = Config::default();
        cfg.networks.get_mut("testnet").unwrap().horizon_url = "ftp://bad.example.com".to_string();
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("horizon_url"));
    }

    #[test]
    fn default_config_includes_plugin_trust_sources() {
        let cfg = Config::default();
        assert_eq!(
            cfg.plugin_trust.trusted_sources,
            default_trusted_plugin_sources()
        );
    }

    #[test]
    fn config_without_plugin_trust_deserializes_with_defaults() {
        let toml = r#"
version = "1"
network = "testnet"
wallets = []
telemetry_enabled = true
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            cfg.plugin_trust.trusted_sources,
            default_trusted_plugin_sources()
        );
    }

    #[test]
    fn trusted_plugin_source_management_deduplicates_and_resets() {
        let mut cfg = Config::default();
        assert!(add_trusted_plugin_source(&mut cfg, "plugins.example.com".to_string()).unwrap());
        assert!(!add_trusted_plugin_source(&mut cfg, "PLUGINS.EXAMPLE.COM".to_string()).unwrap());
        assert!(cfg
            .plugin_trust
            .trusted_sources
            .contains(&"plugins.example.com".to_string()));

        assert!(remove_trusted_plugin_source(
            &mut cfg,
            "plugins.example.com"
        ));
        assert!(!remove_trusted_plugin_source(
            &mut cfg,
            "plugins.example.com"
        ));

        cfg.plugin_trust.trusted_sources.clear();
        reset_trusted_plugin_sources(&mut cfg);
        assert_eq!(
            cfg.plugin_trust.trusted_sources,
            default_trusted_plugin_sources()
        );
    }

    #[test]
    fn invalid_trusted_plugin_sources_are_rejected() {
        for source in [
            "",
            "plugins example.com",
            "https://",
            "ftp://example.com",
            "example",
            "example.com/path",
            "https://example.com/*/bad",
        ] {
            assert!(
                validate_plugin_trust_source(source).is_err(),
                "{source} should be invalid"
            );
        }
    }

    #[test]
    fn validate_config_integrity_passes_default_config() {
        let cfg = Config::default();
        let findings = validate_config_integrity(&cfg);
        assert!(
            findings.iter().all(|f| f.status == DoctorStatus::Pass),
            "expected all pass, got: {:?}",
            findings
        );
    }

    #[test]
    fn validate_config_integrity_catches_bad_wallet_key() {
        let mut cfg = Config::default();
        cfg.wallets.push(WalletEntry {
            name: "bad".to_string(),
            public_key: "not-a-key".to_string(),
            secret_key: None,
            network: "testnet".to_string(),
            created_at: String::new(),
            funded: false,
            rotation_history: Vec::new(),
        });
        let findings = validate_config_integrity(&cfg);
        assert!(
            findings
                .iter()
                .any(|f| f.category == "wallet" && f.status == DoctorStatus::Fail),
            "expected wallet failure, got: {:?}",
            findings
        );
    }
}

/// Returns the network passphrase for transaction signing.
/// Checks the config for a custom passphrase; falls back to well-known defaults.
pub fn get_network_passphrase(network: &str) -> String {
    if let Ok(cfg) = load() {
        if let Some(net_cfg) = cfg.networks.get(network) {
            if let Some(passphrase) = &net_cfg.passphrase {
                return passphrase.clone();
            }
        }
    }
    match network {
        "mainnet" => "Public Global Stellar Network ; September 2015".to_string(),
        _ => "Test SDF Network ; September 2015".to_string(),
    }
}

/// Ensures the three built-in networks are present in the config's network map.
/// Safe to call on any Config — existing entries are never overwritten.
pub fn ensure_default_networks(cfg: &mut Config) {
    cfg.networks
        .entry("testnet".to_string())
        .or_insert_with(|| NetworkConfig {
            horizon_url: "https://horizon-testnet.stellar.org".to_string(),
            soroban_rpc_url: Some("https://soroban-testnet.stellar.org".to_string()),
            friendbot_url: Some("https://friendbot.stellar.org".to_string()),
            passphrase: Some("Test SDF Network ; September 2015".to_string()),
        });
    cfg.networks
        .entry("mainnet".to_string())
        .or_insert_with(|| NetworkConfig {
            horizon_url: "https://horizon.stellar.org".to_string(),
            soroban_rpc_url: Some("https://mainnet.sorobanrpc.com".to_string()),
            friendbot_url: None,
            passphrase: Some("Public Global Stellar Network ; September 2015".to_string()),
        });
    cfg.networks
        .entry("docker-testnet".to_string())
        .or_insert_with(|| NetworkConfig {
            horizon_url: "http://localhost:8000".to_string(),
            soroban_rpc_url: Some("http://localhost:8000/rpc".to_string()),
            friendbot_url: None,
            passphrase: Some("Test SDF Network ; September 2015".to_string()),
        });
}

pub fn save(config: &Config) -> Result<()> {
    validate_config(config)?;
    let db = database::Database::open()?;
    db.save_config(config)?;
    Ok(())
}

pub fn get_network_config(cfg: &Config, network: &str) -> Result<NetworkConfig> {
    cfg.networks
        .get(network)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Network '{}' not found in configuration", network))
}

pub const RESERVED_NETWORKS: &[&str] = &["testnet", "mainnet", "docker-testnet"];

/// Returns true for built-in networks that cannot be removed or renamed.
pub fn is_reserved_network(name: &str) -> bool {
    RESERVED_NETWORKS.contains(&name)
}

pub fn add_custom_network(
    config: &mut Config,
    name: String,
    horizon_url: String,
    soroban_rpc_url: Option<String>,
    friendbot_url: Option<String>,
    passphrase: Option<String>,
) -> Result<()> {
    if is_reserved_network(&name) {
        anyhow::bail!(
            "'{}' is a reserved network name ('testnet', 'mainnet', 'docker-testnet'). Choose a different name.",
            name
        );
    }
    if config.networks.contains_key(&name) {
        anyhow::bail!("Network '{}' already exists", name);
    }
    config.networks.insert(
        name,
        NetworkConfig {
            horizon_url,
            soroban_rpc_url,
            friendbot_url,
            passphrase,
        },
    );
    Ok(())
}

/// Remove a custom network from config. Built-in networks are protected.
pub fn remove_custom_network(config: &mut Config, name: &str) -> Result<()> {
    if is_reserved_network(name) {
        anyhow::bail!(
            "'{}' is a built-in network and cannot be removed. Only custom networks can be removed.",
            name
        );
    }
    if !config.networks.contains_key(name) {
        anyhow::bail!("Network '{}' not found", name);
    }
    // Only remove if it is not a built-in re-injected entry (custom keys are user-added).
    config.networks.remove(name);

    if config.network == name {
        config.network = "testnet".to_string();
    }

    for wallet in &mut config.wallets {
        if wallet.network == name {
            wallet.network = config.network.clone();
        }
    }

    Ok(())
}

/// Rename a custom network. Built-in networks cannot be renamed.
pub fn rename_custom_network(config: &mut Config, old_name: &str, new_name: &str) -> Result<()> {
    if is_reserved_network(old_name) {
        anyhow::bail!(
            "'{}' is a built-in network and cannot be renamed.",
            old_name
        );
    }
    if is_reserved_network(new_name) {
        anyhow::bail!(
            "'{}' is a reserved network name. Choose a different name.",
            new_name
        );
    }
    if !config.networks.contains_key(old_name) {
        anyhow::bail!("Network '{}' not found", old_name);
    }
    if config.networks.contains_key(new_name) {
        anyhow::bail!("Network '{}' already exists", new_name);
    }
    if old_name == new_name {
        anyhow::bail!("Old and new network names are the same");
    }

    let net_cfg = config.networks.remove(old_name).expect("network exists");
    config.networks.insert(new_name.to_string(), net_cfg);

    if config.network == old_name {
        config.network = new_name.to_string();
    }

    for wallet in &mut config.wallets {
        if wallet.network == old_name {
            wallet.network = new_name.to_string();
        }
    }

    Ok(())
}
