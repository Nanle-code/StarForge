//! Contract deployment environment management (#381 / D-44).
//!
//! Layers a small, explicit "environment" concept — dev / staging /
//! production — on top of the network/wallet configuration StarForge
//! already manages (`crate::utils::config`). An environment is a named
//! binding of `{tier, network, wallet}`, plus the settings specific to
//! that stage of the pipeline.
//!
//! ## Why not just use `networks` directly?
//!
//! `Config.networks` already answers "where do I connect" (Horizon/Soroban
//! RPC endpoints) — see `docs/CONFIGURATION.md`. It deliberately says
//! nothing about deployment *process*: which network+wallet pair is "dev"
//! versus "production" for this project, whether a change may skip
//! straight from dev to production, or whether staging and production are
//! actually isolated from each other (the same network+wallet pair would
//! defeat the entire point of having separate stages). Environments are a
//! thin, opinionated layer on top of networks and deployment history that
//! answers those questions, without duplicating anything either already
//! owns.
//!
//! ## Storage
//!
//! Environments are persisted to `~/.starforge/environments.json`
//! (`environments_path()`), the same tier and JSON-array-of-records shape
//! `deploy_history.rs` already uses for `deploy_history.json`.
//!
//! ## Promotion
//!
//! "Promoting" a deployment from one environment to the next does not
//! rebuild or re-derive the artifact — it registers the *exact same*
//! `wasm_hash` that already succeeded in the source environment as a new,
//! pending deployment against the target environment's network, linked
//! back via `previous_id`. That is what makes a promotion meaningful:
//! what was validated in staging is provably the same bytes that reach
//! production, not a rebuild that merely looks the same. Promotion only
//! *records* intent, mirroring how `deploy_history::record_rollback` and
//! the existing `deployments rollback` command work — the actual
//! `stellar contract deploy`/`upgrade` still runs through the normal
//! `starforge deploy --execute` path against the target network.

use crate::utils::config::{self, Config};
use crate::utils::deploy_history::{self, DeployRecord};
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Tiers
// ---------------------------------------------------------------------------

/// Deployment pipeline stage. Fixed to the three tiers the issue asks for —
/// deliberately not an open string, so a typo'd tier name is a
/// compile-time impossibility rather than a silently-accepted config value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentTier {
    Dev,
    Staging,
    Production,
}

impl EnvironmentTier {
    /// The tier immediately before this one in the promotion pipeline, if
    /// any. `Dev` has no predecessor — it's where deployments originate.
    pub fn previous(self) -> Option<Self> {
        match self {
            EnvironmentTier::Dev => None,
            EnvironmentTier::Staging => Some(EnvironmentTier::Dev),
            EnvironmentTier::Production => Some(EnvironmentTier::Staging),
        }
    }
}

impl std::fmt::Display for EnvironmentTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvironmentTier::Dev => write!(f, "dev"),
            EnvironmentTier::Staging => write!(f, "staging"),
            EnvironmentTier::Production => write!(f, "production"),
        }
    }
}

impl std::str::FromStr for EnvironmentTier {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "dev" | "development" => Ok(EnvironmentTier::Dev),
            "staging" | "stage" => Ok(EnvironmentTier::Staging),
            "production" | "prod" => Ok(EnvironmentTier::Production),
            other => anyhow::bail!(
                "Unknown environment tier '{}'. Use 'dev', 'staging', or 'production'.",
                other
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Environment configuration
// ---------------------------------------------------------------------------

/// One configured deployment environment: a named tier bound to a network
/// and (optionally) a specific wallet, plus environment-specific settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvironmentConfig {
    /// Project-local name, e.g. "dev", "staging", "staging-eu". Distinct
    /// from `tier` so a project can have more than one environment at the
    /// same tier (two `Staging` environments, differently named).
    pub name: String,
    pub tier: EnvironmentTier,
    pub network: String,
    pub wallet: Option<String>,
    /// Environment-specific setting: whether promoting *into* this
    /// environment always requires interactive confirmation, regardless of
    /// a caller-supplied `--yes`. Defaults to `true` for `Production`.
    pub require_confirmation: bool,
    pub description: Option<String>,
    pub created_at: String,
}

impl EnvironmentConfig {
    pub fn new(name: &str, tier: EnvironmentTier, network: &str, wallet: Option<String>) -> Self {
        Self {
            name: name.to_string(),
            tier,
            network: network.to_string(),
            wallet,
            require_confirmation: tier == EnvironmentTier::Production,
            description: None,
            created_at: Utc::now().to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// Configuration validation (pure)
// ---------------------------------------------------------------------------

/// Validates one environment's configuration against a loaded `Config`:
///
/// - the name must not be empty,
/// - the target network must exist (built-in or configured — reuses
///   `config::validate_network_exists`, the pure variant `docs/
///   CONFIGURATION.md` documents specifically so validation never touches
///   disk or depends on load order),
/// - if a wallet is named, it must exist *and* be provisioned on the same
///   network the environment targets. A "production" environment silently
///   signing with a testnet-only wallet would fail on-chain in a
///   confusing way; catching the mismatch here is cheaper than debugging
///   a failed mainnet transaction.
pub fn validate_environment(cfg: &Config, env: &EnvironmentConfig) -> Result<()> {
    if env.name.trim().is_empty() {
        anyhow::bail!("Environment name cannot be empty.");
    }
    config::validate_network_exists(cfg, &env.network)
        .with_context(|| format!("environment '{}'", env.name))?;

    if let Some(wallet_name) = &env.wallet {
        let wallet = cfg
            .wallets
            .iter()
            .find(|w| &w.name == wallet_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Environment '{}' references unknown wallet '{}'.",
                    env.name,
                    wallet_name
                )
            })?;
        if wallet.network != env.network {
            anyhow::bail!(
                "Environment '{}' targets network '{}', but its wallet '{}' is provisioned on \
                 '{}'. A deploy would sign with a key from the wrong network.",
                env.name,
                env.network,
                wallet_name,
                wallet.network
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Environment isolation (pure)
// ---------------------------------------------------------------------------

/// One way `env` fails to be isolated from another registered environment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IsolationViolation {
    pub other_environment: String,
    pub reason: String,
}

/// Checks whether `env` is isolated from every other registered
/// environment: no two environments may share both the same network *and*
/// the same named wallet, since that would let a deployment meant for one
/// silently affect the other. Sharing only a network (different wallets)
/// or only a wallet across networks is fine — plenty of real setups
/// deliberately reuse one deployer key across networks.
pub fn check_isolation(
    env: &EnvironmentConfig,
    others: &[EnvironmentConfig],
) -> Vec<IsolationViolation> {
    others
        .iter()
        .filter(|other| other.name != env.name)
        .filter(|other| {
            other.network == env.network && env.wallet.is_some() && other.wallet == env.wallet
        })
        .map(|other| IsolationViolation {
            other_environment: other.name.clone(),
            reason: format!(
                "shares network '{}' and wallet '{}' with environment '{}' — a deployment to \
                 either would affect the same on-chain account",
                env.network,
                env.wallet.as_deref().unwrap_or(""),
                other.name
            ),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

fn environments_path() -> PathBuf {
    config::config_dir().join("environments.json")
}

pub fn load_environments() -> Result<Vec<EnvironmentConfig>> {
    let path = environments_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

fn save_environments(envs: &[EnvironmentConfig]) -> Result<()> {
    let path = environments_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(envs)?)?;
    Ok(())
}

/// Registers a new environment. Rejects a duplicate name outright —
/// silently redefining an existing environment's network/wallet out from
/// under whoever is already deploying to it by name is exactly the kind
/// of surprise this feature exists to prevent.
pub fn register_environment(env: EnvironmentConfig) -> Result<()> {
    let mut envs = load_environments()?;
    if envs.iter().any(|e| e.name == env.name) {
        anyhow::bail!(
            "Environment '{}' already exists. Remove it first (`starforge environment remove \
             {}`) or choose a different name.",
            env.name,
            env.name
        );
    }
    envs.push(env);
    save_environments(&envs)
}

pub fn get_environment(name: &str) -> Result<Option<EnvironmentConfig>> {
    Ok(load_environments()?.into_iter().find(|e| e.name == name))
}

/// Removes an environment by name. Returns `false` (not an error) when no
/// such environment exists, rather than treating a no-op removal as a
/// failure.
pub fn remove_environment(name: &str) -> Result<bool> {
    let mut envs = load_environments()?;
    let before = envs.len();
    envs.retain(|e| e.name != name);
    let removed = envs.len() != before;
    if removed {
        save_environments(&envs)?;
    }
    Ok(removed)
}

// ---------------------------------------------------------------------------
// Deployment promotion
// ---------------------------------------------------------------------------

/// Pure gate: is promoting from `from` to `to` a valid one-step move?
/// Only the immediate next tier is allowed (dev → staging → production);
/// skipping a stage (dev → production directly) is rejected, matching the
/// promotion pipeline the issue describes.
pub fn is_valid_promotion_order(from: EnvironmentTier, to: EnvironmentTier) -> bool {
    to.previous() == Some(from)
}

/// Record of a successful promotion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionRecord {
    pub from_environment: String,
    pub to_environment: String,
    pub promoted_deployment_id: String,
    pub source_deployment_id: String,
    pub wasm_hash: String,
    pub timestamp: String,
}

/// Promotes the last successful deployment recorded against `from_env`'s
/// network to `to_env`: see the module docs above for what "promotion"
/// means here and why it only records intent rather than executing an
/// on-chain transaction.
///
/// # Errors
/// - Either environment name doesn't resolve to a registered environment.
/// - The move isn't exactly one tier forward (see
///   [`is_valid_promotion_order`]).
/// - `from_env` and `to_env` aren't isolated from each other (see
///   [`check_isolation`]) — promoting into an environment that shares a
///   network+wallet with the source wouldn't actually change anything.
/// - There is no successful deployment on `from_env`'s network to promote.
pub fn promote(from_env_name: &str, to_env_name: &str) -> Result<PromotionRecord> {
    let envs = load_environments()?;
    let from_env = envs
        .iter()
        .find(|e| e.name == from_env_name)
        .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found.", from_env_name))?;
    let to_env = envs
        .iter()
        .find(|e| e.name == to_env_name)
        .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found.", to_env_name))?;

    if !is_valid_promotion_order(from_env.tier, to_env.tier) {
        anyhow::bail!(
            "Cannot promote from '{}' ({}) to '{}' ({}): promotion must move exactly one tier \
             forward (dev -> staging -> production).",
            from_env.name,
            from_env.tier,
            to_env.name,
            to_env.tier
        );
    }

    let violations = check_isolation(to_env, std::slice::from_ref(from_env));
    if !violations.is_empty() {
        anyhow::bail!(
            "Refusing to promote '{}' into '{}': {}",
            from_env.name,
            to_env.name,
            violations[0].reason
        );
    }

    let source = deploy_history::last_successful(&from_env.network)?.ok_or_else(|| {
        anyhow::anyhow!(
            "No successful deployment found on environment '{}' (network '{}') to promote.",
            from_env.name,
            from_env.network
        )
    })?;

    let wallet_name = to_env
        .wallet
        .clone()
        .unwrap_or_else(|| source.wallet.clone());
    let promoted_record = DeployRecord::new(
        &source.wasm_path,
        &source.wasm_hash,
        &to_env.network,
        &wallet_name,
        Some(source.id.clone()),
    );
    let promoted_id = deploy_history::record_deployment(promoted_record)?;

    Ok(PromotionRecord {
        from_environment: from_env.name.clone(),
        to_environment: to_env.name.clone(),
        promoted_deployment_id: promoted_id,
        source_deployment_id: source.id.clone(),
        wasm_hash: source.wasm_hash.clone(),
        timestamp: Utc::now().to_rfc3339(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn env(name: &str, tier: EnvironmentTier, network: &str, wallet: Option<&str>) -> EnvironmentConfig {
        EnvironmentConfig::new(name, tier, network, wallet.map(str::to_string))
    }

    // -- tier ordering -------------------------------------------------

    #[test]
    fn dev_has_no_previous_tier() {
        assert_eq!(EnvironmentTier::Dev.previous(), None);
    }

    #[test]
    fn staging_previous_is_dev_and_production_previous_is_staging() {
        assert_eq!(EnvironmentTier::Staging.previous(), Some(EnvironmentTier::Dev));
        assert_eq!(
            EnvironmentTier::Production.previous(),
            Some(EnvironmentTier::Staging)
        );
    }

    #[test]
    fn tier_from_str_accepts_aliases_case_insensitively() {
        assert_eq!(
            "PROD".parse::<EnvironmentTier>().unwrap(),
            EnvironmentTier::Production
        );
        assert_eq!(
            "Staging".parse::<EnvironmentTier>().unwrap(),
            EnvironmentTier::Staging
        );
        assert!("nonexistent".parse::<EnvironmentTier>().is_err());
    }

    #[test]
    fn tier_display_matches_serde_rename() {
        assert_eq!(EnvironmentTier::Dev.to_string(), "dev");
        assert_eq!(EnvironmentTier::Production.to_string(), "production");
    }

    // -- promotion order (pure) -----------------------------------------

    #[test]
    fn one_step_forward_promotions_are_valid() {
        assert!(is_valid_promotion_order(
            EnvironmentTier::Dev,
            EnvironmentTier::Staging
        ));
        assert!(is_valid_promotion_order(
            EnvironmentTier::Staging,
            EnvironmentTier::Production
        ));
    }

    #[test]
    fn skipping_a_tier_is_rejected() {
        assert!(!is_valid_promotion_order(
            EnvironmentTier::Dev,
            EnvironmentTier::Production
        ));
    }

    #[test]
    fn same_tier_and_backward_promotions_are_rejected() {
        assert!(!is_valid_promotion_order(
            EnvironmentTier::Dev,
            EnvironmentTier::Dev
        ));
        assert!(!is_valid_promotion_order(
            EnvironmentTier::Production,
            EnvironmentTier::Staging
        ));
    }

    // -- isolation (pure) -------------------------------------------------

    #[test]
    fn distinct_network_and_wallet_pairs_are_isolated() {
        let staging = env("staging", EnvironmentTier::Staging, "testnet", Some("stage-key"));
        let production = env(
            "production",
            EnvironmentTier::Production,
            "mainnet",
            Some("prod-key"),
        );
        assert!(check_isolation(&production, &[staging]).is_empty());
    }

    #[test]
    fn same_network_different_wallet_is_isolated() {
        let a = env("staging-a", EnvironmentTier::Staging, "testnet", Some("key-a"));
        let b = env("staging-b", EnvironmentTier::Staging, "testnet", Some("key-b"));
        assert!(check_isolation(&a, &[b]).is_empty());
    }

    #[test]
    fn same_network_and_wallet_is_an_isolation_violation() {
        let staging = env("staging", EnvironmentTier::Staging, "testnet", Some("shared-key"));
        let production = env(
            "production",
            EnvironmentTier::Production,
            "testnet",
            Some("shared-key"),
        );
        let violations = check_isolation(&production, &[staging.clone()]);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].other_environment, "staging");
        assert!(violations[0].reason.contains("testnet"));
        assert!(violations[0].reason.contains("shared-key"));
    }

    #[test]
    fn no_wallet_configured_is_never_a_violation() {
        // Boundary: two environments on the same network with *no* wallet
        // set on the environment being checked can't be flagged — there's
        // nothing to compare.
        let a = env("dev", EnvironmentTier::Dev, "testnet", None);
        let b = env("staging", EnvironmentTier::Staging, "testnet", Some("key"));
        assert!(check_isolation(&a, &[b]).is_empty());
    }

    #[test]
    fn an_environment_never_violates_isolation_against_itself() {
        let staging = env("staging", EnvironmentTier::Staging, "testnet", Some("key"));
        assert!(check_isolation(&staging, std::slice::from_ref(&staging)).is_empty());
    }

    // -- EnvironmentConfig defaults --------------------------------------

    #[test]
    fn production_defaults_to_requiring_confirmation() {
        let e = EnvironmentConfig::new("prod", EnvironmentTier::Production, "mainnet", None);
        assert!(e.require_confirmation);
    }

    #[test]
    fn dev_does_not_default_to_requiring_confirmation() {
        let e = EnvironmentConfig::new("dev", EnvironmentTier::Dev, "testnet", None);
        assert!(!e.require_confirmation);
    }
}
