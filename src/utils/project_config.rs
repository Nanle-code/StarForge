//! Shared project lockfiles (#805).
//!
//! A project lockfile is a small, **non-secret** TOML file (default name
//! `starforge-project.toml`) that a team commits to their repository so every
//! member runs the CLI against the same networks, feature flags, and quality
//! gates. It complements — never replaces — the per-user config stored under
//! `~/.starforge`.
//!
//! Security model:
//! - The [`ProjectLockfile`] schema structurally cannot carry secrets: there
//!   are no wallet fields and no key-material fields, and unknown keys are
//!   rejected on load. A lockfile that (mis)declares `wallets` or
//!   `wallet_encryption` fails to parse instead of silently applying.
//! - Precedence is *project wins*: values in the lockfile override the same
//!   values in the user config, so "works on my machine" defaults lose to the
//!   team's committed settings. Everything the lockfile does not set keeps
//!   coming from the user config.
//! - The merged result is validated with the same rules as a saved config, so
//!   a lockfile can never produce a config the CLI itself would reject.

use crate::utils::config::{
    merge_configs, AiTelemetryConfig, Config, ConfigOverlay, FeatureFlagsConfig, NetworkConfig,
    PluginTrustConfig,
};
use crate::utils::smoke_tests::{self, SmokeTest};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Canonical project lockfile filename.
pub const PROJECT_LOCKFILE_FILENAME: &str = "starforge-project.toml";

/// The shareable, non-secret subset of StarForge configuration.
///
/// Mirrors the corresponding [`ConfigOverlay`] fields: each `Option` here
/// overrides the same field of the user config when present, and composite
/// settings are replaced wholesale so a partially-specified table can never
/// produce a half-merged policy (see [`crate::utils::config::merge_configs`]).
///
/// Deliberately absent: `wallets` and `wallet_encryption` (key material),
/// `install_id`/`version` (per-install identity). `deny_unknown_fields`
/// turns any attempt to sneak them in into a load error.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectLockfile {
    /// Overrides the user's active network for this project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    /// Overrides the telemetry opt-in flag for this project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_enabled: Option<bool>,
    /// Replaces the user's feature-flag settings wholesale when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_flags: Option<FeatureFlagsConfig>,
    /// Replaces the user's AI telemetry settings wholesale when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_telemetry: Option<AiTelemetryConfig>,
    /// Replaces the user's plugin trust allowlist wholesale when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_trust: Option<PluginTrustConfig>,
    /// Networks to add, or to replace by name.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub networks: HashMap<String, NetworkConfig>,
    /// Post-deploy smoke tests run by `starforge deploy --execute` (#753).
    /// Not a config override: it never reaches the merged [`Config`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub smoke_tests: Vec<SmokeTest>,
}

impl ProjectLockfile {
    /// True when the lockfile would not change anything.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Convert to a [`ConfigOverlay`] so the merge/precedence machinery stays
    /// in exactly one place. Wallets are structurally impossible here, so the
    /// overlay's wallet list is always empty.
    pub fn to_overlay(&self) -> ConfigOverlay {
        ConfigOverlay {
            network: self.network.clone(),
            telemetry_enabled: self.telemetry_enabled,
            // Wallet key material must never come from a committed file.
            wallet_encryption: None,
            feature_flags: self.feature_flags.clone(),
            ai_telemetry: self.ai_telemetry.clone(),
            plugin_trust: self.plugin_trust.clone(),
            networks: self.networks.clone(),
            wallets: Vec::new(),
        }
    }
}

/// Parse a [`ProjectLockfile`] from TOML, rejecting unknown keys.
///
/// Unknown-key rejection is the schema-validation guarantee: a typo
/// (`feautre_flags`) or a secret-bearing section (`wallets`) fails here
/// instead of being ignored at load time.
pub fn parse_project_lockfile_str(contents: &str) -> Result<ProjectLockfile> {
    let lockfile: ProjectLockfile =
        toml::from_str(contents).context("Invalid project lockfile TOML")?;
    smoke_tests::validate(&lockfile.smoke_tests).context("Invalid project lockfile smoke_tests")?;
    Ok(lockfile)
}

/// Load a [`ProjectLockfile`] from a TOML file.
pub fn load_project_lockfile(path: &Path) -> Result<ProjectLockfile> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read project lockfile {}", path.display()))?;
    parse_project_lockfile_str(&contents)
        .with_context(|| format!("Invalid project lockfile {}", path.display()))
}

/// Search `start` and its ancestors for a project lockfile.
///
/// Walking upwards (unlike the single-directory deploy-policy discovery)
/// matches how developers actually work: any command run anywhere inside the
/// repository picks up the lockfile committed at its root.
pub fn discover_project_lockfile(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        let candidate = current.join(PROJECT_LOCKFILE_FILENAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = current.parent();
    }
    None
}

/// Discover the project lockfile for the given directory and load it.
///
/// Returns `Ok(None)` when no lockfile exists in the tree — the normal case
/// outside a project. A lockfile that exists but fails schema validation is
/// an error: a broken team config should stop the run, not be ignored.
pub fn find_and_load_project_lockfile(start: &Path) -> Result<Option<(PathBuf, ProjectLockfile)>> {
    match discover_project_lockfile(start) {
        Some(path) => {
            let lockfile = load_project_lockfile(&path)?;
            Ok(Some((path, lockfile)))
        }
        None => Ok(None),
    }
}

/// Layer a discovered project lockfile on top of the user's config.
///
/// Delegates to [`merge_configs`], so the merged result is validated and the
/// per-install identity (`version`, `install_id`) always stays with the base.
pub fn apply_project_overrides(base: Config, lockfile: &ProjectLockfile) -> Result<Config> {
    merge_configs(base, lockfile.to_overlay())
}

/// Load the *effective* configuration: the user's config layered with the
/// project lockfile discovered from the current directory, when one exists.
///
/// Commands that only *read* configuration should prefer this over
/// [`crate::utils::config::load`] so team settings apply transparently.
/// Commands that *write* configuration must keep using
/// [`crate::utils::config::load`] — project overrides are an input, not
/// something to persist back into the user's store.
pub fn load_effective() -> Result<Config> {
    let base = crate::utils::config::load()?;
    let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match find_and_load_project_lockfile(&start)? {
        Some((_path, lockfile)) => apply_project_overrides(base, &lockfile),
        None => Ok(base),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Schema validation ─────────────────────────────────────────────────────

    #[test]
    fn empty_lockfile_parses_and_is_noop() {
        let lock = parse_project_lockfile_str("").unwrap();
        assert!(lock.is_empty());
        let base = Config::default();
        let merged = apply_project_overrides(base.clone(), &lock).unwrap();
        assert_eq!(merged, base);
    }

    #[test]
    fn minimal_network_override_parses() {
        let lock = parse_project_lockfile_str("network = \"testnet\"").unwrap();
        assert_eq!(lock.network.as_deref(), Some("testnet"));
        assert!(lock.networks.is_empty());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        // A typo must fail loudly, not silently apply nothing.
        let err = parse_project_lockfile_str("feautre_flags = {}").unwrap_err();
        assert!(err.to_string().contains("Invalid project lockfile"));
    }

    #[test]
    fn wallets_are_rejected_by_schema() {
        // Key material must never travel through a committed lockfile.
        let result =
            parse_project_lockfile_str("[[wallets]]\nname = \"team\"\npublic_key = \"GABC...\"\n");
        assert!(result.is_err(), "wallets must be rejected by the schema");
    }

    #[test]
    fn wallet_encryption_is_rejected_by_schema() {
        let result =
            parse_project_lockfile_str("[wallet_encryption]\nmem = 65536\niterations = 3\n");
        assert!(
            result.is_err(),
            "wallet encryption settings must be rejected by the schema"
        );
    }

    #[test]
    fn install_id_is_rejected_by_schema() {
        let result = parse_project_lockfile_str("install_id = \"123e4567-...\"");
        assert!(result.is_err(), "install identity must stay per-user");
    }

    // ── Discovery ─────────────────────────────────────────────────────────────

    #[test]
    fn discovery_finds_lockfile_in_start_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROJECT_LOCKFILE_FILENAME);
        std::fs::write(&path, "network = \"testnet\"").unwrap();
        let found = discover_project_lockfile(dir.path()).unwrap();
        assert_eq!(found, path);
    }

    #[test]
    fn discovery_walks_up_to_ancestors() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        let path = dir.path().join(PROJECT_LOCKFILE_FILENAME);
        std::fs::write(&path, "network = \"testnet\"").unwrap();
        assert_eq!(discover_project_lockfile(&nested).unwrap(), path);
    }

    #[test]
    fn discovery_returns_none_without_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        assert!(discover_project_lockfile(dir.path()).is_none());
    }

    #[test]
    fn find_and_load_reports_broken_lockfile_as_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PROJECT_LOCKFILE_FILENAME);
        std::fs::write(&path, "bogus_key = 1").unwrap();
        assert!(find_and_load_project_lockfile(dir.path()).is_err());
    }

    // ── Precedence / application ──────────────────────────────────────────────

    #[test]
    fn project_network_overrides_user_network() {
        let lock = parse_project_lockfile_str("network = \"mainnet\"").unwrap();
        let base = Config::default(); // default active network is "testnet"
        let merged = apply_project_overrides(base, &lock).unwrap();
        assert_eq!(merged.network, "mainnet");
        // Per-install identity is untouched by the project layer.
        assert!(merged.install_id.is_none());
    }

    #[test]
    fn project_networks_merge_by_name() {
        let lock = parse_project_lockfile_str(
            "[networks.localnet]\nhorizon_url = \"http://localhost:8000\"\n",
        )
        .unwrap();
        let base = Config::default();
        let merged = apply_project_overrides(base, &lock).unwrap();
        assert!(merged.networks.contains_key("localnet"));
        // Existing user networks survive.
        assert!(merged.networks.contains_key("testnet"));
        assert!(merged.networks.contains_key("mainnet"));
    }

    #[test]
    fn project_telemetry_override_wins() {
        let lock = parse_project_lockfile_str("telemetry_enabled = true").unwrap();
        let base = Config::default(); // telemetry_enabled defaults to Some(false)
        let merged = apply_project_overrides(base, &lock).unwrap();
        assert_eq!(merged.telemetry_enabled, Some(true));
    }

    #[test]
    fn unset_fields_fall_through_to_user_config() {
        let lock = parse_project_lockfile_str("network = \"mainnet\"").unwrap();
        let base = Config::default();
        let merged = apply_project_overrides(base.clone(), &lock).unwrap();
        // Only the overridden field changes; the rest of the user config is kept.
        assert_eq!(merged.feature_flags, base.feature_flags);
        assert_eq!(merged.plugin_trust, base.plugin_trust);
        assert_eq!(merged.ai_telemetry, base.ai_telemetry);
        assert!(merged.wallets.is_empty());
    }

    #[test]
    fn to_overlay_never_carries_wallet_material() {
        let lock = parse_project_lockfile_str("network = \"testnet\"").unwrap();
        let overlay = lock.to_overlay();
        assert!(overlay.wallets.is_empty());
        assert!(overlay.wallet_encryption.is_none());
    }

    #[test]
    fn full_lockfile_parses_every_shareable_section() {
        let lock = parse_project_lockfile_str(
            r#"
network = "mainnet"
telemetry_enabled = false

[feature_flags]
metrics_enabled = false

[ai_telemetry]
enabled = false

[plugin_trust]
trusted_publishers = ["a06f4d3d1d5b5b1c0e0f0d0c0b0a09080706050403020100ffeeddccbbaa9988"]

[networks.customnet]
horizon_url = "https://horizon.example.com"
soroban_rpc_url = "https://rpc.example.com"
"#,
        )
        .unwrap();
        assert_eq!(lock.network.as_deref(), Some("mainnet"));
        assert_eq!(lock.telemetry_enabled, Some(false));
        assert_eq!(lock.feature_flags.as_ref().unwrap().metrics_enabled, false);
        assert_eq!(lock.ai_telemetry.as_ref().unwrap().enabled, false);
        assert_eq!(
            lock.plugin_trust.as_ref().unwrap().trusted_publishers.len(),
            1
        );
        assert!(lock.networks.contains_key("customnet"));

        let merged = apply_project_overrides(Config::default(), &lock).unwrap();
        assert_eq!(merged.network, "mainnet");
        assert_eq!(merged.telemetry_enabled, Some(false));
        assert_eq!(merged.feature_flags.metrics_enabled, false);
        assert_eq!(merged.ai_telemetry.enabled, false);
        assert_eq!(merged.plugin_trust.trusted_publishers.len(), 1);
    }

    // ── Smoke tests (#753) ────────────────────────────────────────────────────

    #[test]
    fn smoke_tests_parse_from_manifest() {
        let lock = parse_project_lockfile_str(
            r#"
[[smoke_tests]]
name = "hello"
invoke = { function = "hello", args = ["--to", "world"] }
expect_contains = "world"

[[smoke_tests]]
name = "health"
command = "echo ok"
expect_output = "ok"
timeout_secs = 10
"#,
        )
        .unwrap();
        assert_eq!(lock.smoke_tests.len(), 2);
        assert_eq!(
            lock.smoke_tests[0].invoke.as_ref().unwrap().function,
            "hello"
        );
        assert_eq!(lock.smoke_tests[1].timeout_secs, Some(10));
        // Smoke tests are not a config override.
        let merged = apply_project_overrides(Config::default(), &lock).unwrap();
        assert_eq!(merged, Config::default());
    }

    #[test]
    fn sample_project_manifests_are_valid() {
        let passing = parse_project_lockfile_str(include_str!(
            "../../examples/smoke-tests/starforge-project.toml"
        ))
        .unwrap();
        assert_eq!(passing.smoke_tests.len(), 3);

        let failing = parse_project_lockfile_str(include_str!(
            "../../examples/smoke-tests/failing/starforge-project.toml"
        ))
        .unwrap();
        assert_eq!(failing.smoke_tests.len(), 2);
    }

    #[test]
    fn manifest_without_smoke_tests_has_none() {
        let lock = parse_project_lockfile_str("network = \"testnet\"").unwrap();
        assert!(lock.smoke_tests.is_empty());
    }

    #[test]
    fn invalid_smoke_tests_fail_manifest_validation() {
        let err = parse_project_lockfile_str("[[smoke_tests]]\nname = \"empty\"\n").unwrap_err();
        assert!(format!("{err:#}").contains("smoke_tests"), "{err:#}");

        let err = parse_project_lockfile_str(
            "[[smoke_tests]]\nname = \"typo\"\ncommand = \"echo\"\nexpect_contain = \"x\"\n",
        );
        assert!(err.is_err(), "unknown smoke test keys must be rejected");
    }
}
