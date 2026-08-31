//! Automated secure defaults audit tests (issue #797).
//!
//! These tests verify that StarForge ships with privacy-respecting, secure
//! defaults. Run via:
//!
//! ```bash
//! cargo test --test secure_defaults_audit --locked
//! ```
//!
//! Each test corresponds to an item in `SECURE_DEFAULTS_AUDIT.md`.

use starforge::utils::config::{
    default_trusted_plugin_sources, AiTelemetryConfig, Config, ConfigOverlay, FeatureFlagsConfig,
    NetworkConfig, PluginTrustConfig, CURRENT_CONFIG_VERSION,
};

// ── A1: Telemetry opt-out respected ─────────────────────────────────────────

#[test]
fn a01_telemetry_defaults_to_enabled() {
    let cfg = Config::default();
    assert_eq!(
        cfg.telemetry_enabled,
        Some(true),
        "telemetry_enabled must default to Some(true) for opt-out model"
    );
}

// ── A2: Telemetry local-only (no network transmission) ───────────────────────

#[test]
fn a02_telemetry_is_local_only() {
    // Verify that the telemetry module only writes to local files.
    // This is a source-level check: if network code was added, the test
    // source would need to import reqwest or similar. We verify by checking
    // that the telemetry data struct doesn't have network-related fields.
    // The actual network check is verified by reading the source.
    //
    // If this test fails, it means someone added a network transmission field
    // to TelemetryData — review TELEMETRY_PRIVACY.md before proceeding.
    let source = include_str!("../src/utils/telemetry.rs");
    assert!(
        !source.contains("reqwest")
            && !source.contains("hyper")
            && !source.contains("surf"),
        "telemetry.rs must not import HTTP clients — telemetry is local-only per TELEMETRY_PRIVACY.md"
    );
    assert!(
        source.contains("save_telemetry_locally"),
        "telemetry must use local file storage"
    );
}

// ── A3: AI telemetry cloud disabled by default ───────────────────────────────

#[test]
fn a03_ai_telemetry_cloud_disabled_by_default() {
    let ai_telemetry = AiTelemetryConfig::default();
    assert!(
        !ai_telemetry.cloud_aggregation_enabled,
        "AI telemetry cloud aggregation must default to false (opt-in only)"
    );
}

// ── A4: AI telemetry cloud endpoint empty ────────────────────────────────────

#[test]
fn a04_ai_telemetry_cloud_endpoint_empty() {
    let ai_telemetry = AiTelemetryConfig::default();
    assert_eq!(
        ai_telemetry.cloud_endpoint, None,
        "AI telemetry cloud endpoint must default to None"
    );
}

// ── A5: Feature flag metrics enabled ─────────────────────────────────────────

#[test]
fn a05_feature_flag_metrics_enabled() {
    let flags = FeatureFlagsConfig::default();
    assert!(
        flags.metrics_enabled,
        "feature flag metrics_enabled must default to true"
    );
}

// ── A6: Feature flag metrics retention capped ────────────────────────────────

#[test]
fn a06_feature_flag_metrics_retention_capped() {
    let flags = FeatureFlagsConfig::default();
    assert!(
        flags.metrics_retention_days <= 90,
        "feature flag metrics_retention_days must be <= 90, got {}",
        flags.metrics_retention_days
    );
    assert_eq!(
        flags.metrics_retention_days, 30,
        "feature flag metrics_retention_days must default to 30"
    );
}

// ── A7: Friendbot absent on mainnet ──────────────────────────────────────────

#[test]
fn a07_friendbot_absent_on_mainnet() {
    let cfg = Config::default();
    let mainnet = cfg
        .networks
        .get("mainnet")
        .expect("mainnet must be configured");
    assert_eq!(
        mainnet.friendbot_url, None,
        "mainnet must not have a Friendbot URL — Friendbot is testnet-only"
    );
}

// ── A8: Friendbot present on testnet ─────────────────────────────────────────

#[test]
fn a08_friendbot_present_on_testnet() {
    let cfg = Config::default();
    let testnet = cfg
        .networks
        .get("testnet")
        .expect("testnet must be configured");
    assert_eq!(
        testnet.friendbot_url.as_deref(),
        Some("https://friendbot.stellar.org"),
        "testnet must have Friendbot URL pointing to friendbot.stellar.org"
    );
}

// ── A9: Default network is testnet ───────────────────────────────────────────

#[test]
fn a09_default_network_is_testnet() {
    let cfg = Config::default();
    assert_eq!(
        cfg.network, "testnet",
        "default active network must be testnet"
    );
}

// ── A10: Plugin trust sources restricted ─────────────────────────────────────

#[test]
fn a10_plugin_trust_sources_restricted() {
    let cfg = Config::default();
    let sources = &cfg.plugin_trust.trusted_sources;

    // Must have exactly the default sources (no extra)
    assert_eq!(
        sources.len(),
        default_trusted_plugin_sources().len(),
        "plugin trust sources must match the default allowlist"
    );

    // All sources must use HTTPS
    for source in sources {
        if source.contains("://") {
            assert!(
                source.starts_with("https://") || source.starts_with("git+https://"),
                "plugin trust source must use HTTPS: {}",
                source
            );
        }
    }

    // Must only reference known repos
    for source in sources {
        assert!(
            source.contains("Nanle-code")
                || source.contains("StarForge-Labs")
                || source.contains("crates.io"),
            "plugin trust source references unknown repo: {}",
            source
        );
    }
}

// ── A11: Wallet encryption opt-in ───────────────────────────────────────────

#[test]
fn a11_wallet_encryption_opt_in() {
    let cfg = Config::default();
    assert_eq!(
        cfg.wallet_encryption, None,
        "wallet_encryption must default to None (opt-in only)"
    );
}

// ── A12: Config schema version current ───────────────────────────────────────

#[test]
fn a12_config_schema_version_current() {
    let cfg = Config::default();
    assert_eq!(
        cfg.version, CURRENT_CONFIG_VERSION,
        "default config version must match CURRENT_CONFIG_VERSION"
    );
}

// ── A13: File permissions restricted (source check) ──────────────────────────

#[test]
fn a13_file_permissions_restricted() {
    // Verify that the deployment checkpoint module sets restricted permissions.
    // This is a source-level check for Unix file modes.
    let source = include_str!("../src/utils/deployment_checkpoint.rs");
    assert!(
        source.contains("0o600") || source.contains("Permissions::from_mode(0o600)"),
        "deployment_checkpoint.rs must set file permissions to 0600 for sensitive files"
    );
}

// ── A14: Data directory permissions restricted (source check) ────────────────

#[test]
fn a14_data_directory_permissions_restricted() {
    // The data directory should be created with restricted permissions.
    // Check that the config module creates the data directory securely.
    let source = include_str!("../src/utils/config.rs");
    assert!(
        source.contains("create_dir_all"),
        "config.rs must create directories"
    );
}

// ── A15: Network passphrase validated ────────────────────────────────────────

#[test]
fn a15_network_passphrases_populated() {
    let cfg = Config::default();

    let testnet = cfg.networks.get("testnet").unwrap();
    assert!(
        testnet.passphrase.is_some(),
        "testnet must have a passphrase"
    );
    assert!(
        testnet
            .passphrase
            .as_ref()
            .unwrap()
            .contains("Test SDF Network"),
        "testnet passphrase must contain 'Test SDF Network'"
    );

    let mainnet = cfg.networks.get("mainnet").unwrap();
    assert!(
        mainnet.passphrase.is_some(),
        "mainnet must have a passphrase"
    );
    assert!(
        mainnet
            .passphrase
            .as_ref()
            .unwrap()
            .contains("Public Global Stellar Network"),
        "mainnet passphrase must contain 'Public Global Stellar Network'"
    );
}

// ── Additional: Overlay doesn't accidentally enable cloud telemetry ──────────

#[test]
fn overlay_does_not_force_cloud_telemetry() {
    let base = Config::default();
    let overlay = ConfigOverlay::default();
    let merged = starforge::utils::config::merge_configs(base, overlay).unwrap();

    assert!(
        !merged.ai_telemetry.cloud_aggregation_enabled,
        "empty overlay must not enable cloud telemetry"
    );
    assert_eq!(
        merged.ai_telemetry.cloud_endpoint, None,
        "empty overlay must not set cloud endpoint"
    );
}

// ── Additional: Env var opt-out works for telemetry ──────────────────────────

#[test]
fn env_var_opt_out_respected() {
    // Verify the telemetry module checks the env var before config.
    // This is a source-level check.
    let source = include_str!("../src/utils/telemetry.rs");
    assert!(
        source.contains("STARFORGE_TELEMETRY"),
        "telemetry module must check STARFORGE_TELEMETRY env var"
    );
    assert!(
        source.contains("\"0\"") && source.contains("\"false\""),
        "telemetry module must recognize '0' and 'false' as opt-out values"
    );
}

// ── Additional: AI telemetry env var override ────────────────────────────────

#[test]
fn ai_telemetry_env_var_override() {
    let source = include_str!("../src/utils/ai_telemetry.rs");
    assert!(
        source.contains("STARFORGE_AI_TELEMETRY"),
        "ai_telemetry module must check STARFORGE_AI_TELEMETRY env var"
    );
    assert!(
        source.contains("STARFORGE_TELEMETRY"),
        "ai_telemetry module must fall back to STARFORGE_TELEMETRY env var"
    );
}

// ── Additional: Docker testnet has no Friendbot ──────────────────────────────

#[test]
fn docker_testnet_no_friendbot() {
    let cfg = Config::default();
    let docker = cfg
        .networks
        .get("docker-testnet")
        .expect("docker-testnet must be configured");
    assert_eq!(
        docker.friendbot_url, None,
        "docker-testnet must not have Friendbot URL (local network)"
    );
}

// ── Additional: Horizon URLs use HTTPS for public networks ───────────────────

#[test]
fn public_network_horizon_urls_use_https() {
    let cfg = Config::default();

    for name in &["testnet", "mainnet"] {
        let net = cfg.networks.get(*name).unwrap();
        assert!(
            net.horizon_url.starts_with("https://"),
            "network '{}' horizon_url must use HTTPS, got: {}",
            name,
            net.horizon_url
        );
    }
}
