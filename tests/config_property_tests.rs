//! Property-based tests for configuration round trips (issue #696).
//!
//! These tests generate hundreds of syntactically diverse configurations and
//! assert two families of properties:
//!
//! 1. **Preservation** — a valid configuration survives every parse /
//!    serialize / merge path unchanged. TOML and JSON round trips are exact,
//!    merging with an empty overlay is the identity, and merging is idempotent.
//! 2. **Rejection** — malformed *combinations* (values that are individually
//!    well-formed but invalid together) are refused, not silently repaired.
//!    An unknown network reference, a duplicate wallet name, a non-HTTP
//!    endpoint, or an unknown overlay key must all fail.
//!
//! Run with:
//!   cargo test --test config_property_tests
//!
//! Deeper coverage:
//!   PROPTEST_CASES=10000 cargo test --test config_property_tests

#![allow(dead_code, unused_imports)]

use proptest::prelude::*;
use starforge::utils::config::{
    self, AiTelemetryConfig, Config, ConfigOverlay, FeatureFlagsConfig, NetworkConfig,
    PluginTrustConfig, WalletEntry, WalletRotationRecord,
};
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// Strategies
// ─────────────────────────────────────────────────────────────────────────────

const STELLAR_CHARSET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn stellar_chars(len: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(proptest::sample::select(STELLAR_CHARSET.as_bytes()), len)
        .prop_map(|v| String::from_utf8(v).unwrap())
}

fn public_key() -> impl Strategy<Value = String> {
    stellar_chars(55).prop_map(|s| format!("G{}", s))
}

fn secret_key() -> impl Strategy<Value = String> {
    stellar_chars(55).prop_map(|s| format!("S{}", s))
}

/// A base64 field of `blocks` four-character groups, so the length is always a
/// multiple of four and the value decodes without padding errors.
fn base64_field(blocks: std::ops::Range<usize>) -> impl Strategy<Value = String> {
    proptest::collection::vec("[A-Za-z0-9+/]{4}", blocks).prop_map(|groups| groups.concat())
}

/// An encrypted secret bundle (`salt:nonce:ciphertext`), the other shape a
/// stored secret is allowed to take.
fn encrypted_bundle() -> impl Strategy<Value = String> {
    (base64_field(1..6), base64_field(1..6), base64_field(1..12))
        .prop_map(|(salt, nonce, ct)| format!("{}:{}:{}", salt, nonce, ct))
}

fn wallet_name() -> impl Strategy<Value = String> {
    "[A-Za-z0-9_-]{1,24}"
}

fn network_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9-]{0,15}"
}

fn http_url() -> impl Strategy<Value = String> {
    (
        proptest::sample::select(vec!["http", "https"]),
        "[a-z][a-z0-9.-]{2,24}",
        proptest::option::of(1024u16..=65535),
    )
        .prop_map(|(scheme, host, port)| match port {
            Some(p) => format!("{}://{}:{}", scheme, host, p),
            None => format!("{}://{}", scheme, host),
        })
}

fn network_config() -> impl Strategy<Value = NetworkConfig> {
    (
        http_url(),
        proptest::option::of(http_url()),
        proptest::option::of(http_url()),
        proptest::option::of("[A-Za-z0-9 ;]{0,40}"),
    )
        .prop_map(
            |(horizon_url, soroban_rpc_url, friendbot_url, passphrase)| NetworkConfig {
                horizon_url,
                soroban_rpc_url,
                friendbot_url,
                passphrase,
            },
        )
}

fn rotation_record() -> impl Strategy<Value = WalletRotationRecord> {
    (
        "[0-9]{4}-[0-1][0-9]-[0-3][0-9]T00:00:00Z",
        public_key(),
        network_name(),
        any::<bool>(),
        proptest::option::of(secret_key()),
    )
        .prop_map(
            |(
                rotated_at,
                previous_public_key,
                previous_network,
                previous_funded,
                previous_secret_key,
            )| {
                WalletRotationRecord {
                    rotated_at,
                    previous_public_key,
                    previous_network,
                    previous_funded,
                    previous_secret_key,
                }
            },
        )
}

fn feature_flags() -> impl Strategy<Value = FeatureFlagsConfig> {
    (
        any::<bool>(),
        0u32..3650,
        proptest::collection::hash_map("[a-z_]{1,12}", "[A-Za-z0-9_-]{0,16}", 0..4),
    )
        .prop_map(
            |(metrics_enabled, metrics_retention_days, default_attributes)| FeatureFlagsConfig {
                metrics_enabled,
                metrics_retention_days,
                default_attributes,
            },
        )
}

fn plugin_trust() -> impl Strategy<Value = PluginTrustConfig> {
    proptest::collection::vec(
        proptest::sample::select(vec![
            "https://github.com/Nanle-code/starforge-*".to_string(),
            "https://plugins.example.com/releases/".to_string(),
            "plugins.example.org".to_string(),
        ]),
        0..3,
    )
    .prop_map(|trusted_sources| PluginTrustConfig {
        trusted_sources,
        trusted_publishers: Vec::new(),
        require_signatures: false,
    })
}

/// A configuration that is internally consistent: every referenced network
/// exists, wallet names are unique, and every URL is an HTTP(S) endpoint.
fn valid_config() -> impl Strategy<Value = Config> {
    (
        proptest::collection::hash_map(network_name(), network_config(), 1..4),
        proptest::collection::vec(
            (
                wallet_name(),
                public_key(),
                proptest::option::of(prop_oneof![secret_key(), encrypted_bundle()]),
                any::<bool>(),
                proptest::collection::vec(rotation_record(), 0..2),
            ),
            0..4,
        ),
        proptest::option::of(any::<bool>()),
        proptest::option::of("[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}"),
        feature_flags(),
        plugin_trust(),
    )
        .prop_map(
            |(
                networks,
                wallet_specs,
                telemetry_enabled,
                install_id,
                feature_flags,
                plugin_trust,
            )| {
                let network_names: Vec<String> = networks.keys().cloned().collect();
                let active = network_names[0].clone();

                let mut used_names = std::collections::HashSet::new();
                let mut wallets = Vec::new();
                for (i, (name, public_key, secret_key, funded, rotation_history)) in
                    wallet_specs.into_iter().enumerate()
                {
                    // Force uniqueness rather than discarding the case: the
                    // duplicate-name path has its own dedicated test below.
                    let mut name = name;
                    while !used_names.insert(name.clone()) {
                        name = format!("{}-{}", name, i);
                    }
                    wallets.push(WalletEntry {
                        name,
                        public_key,
                        secret_key,
                        network: network_names[i % network_names.len()].clone(),
                        created_at: "2026-01-01T00:00:00Z".to_string(),
                        funded,
                        rotation_history,
                    });
                }

                Config {
                    version: "1".to_string(),
                    network: active,
                    telemetry_enabled,
                    install_id,
                    wallet_encryption: None,
                    networks,
                    plugin_trust,
                    feature_flags,
                    ai_telemetry: AiTelemetryConfig::default(),
                    wallets,
                }
            },
        )
}

/// An overlay whose networks and wallets never collide with `base`.
fn overlay_for(base: &Config) -> impl Strategy<Value = ConfigOverlay> {
    let existing_wallets: std::collections::HashSet<String> =
        base.wallets.iter().map(|w| w.name.clone()).collect();
    let network_names: Vec<String> = base.networks.keys().cloned().collect();

    (
        proptest::option::of(proptest::sample::select(network_names)),
        proptest::option::of(any::<bool>()),
        proptest::collection::hash_map(network_name(), network_config(), 0..3),
        proptest::collection::vec((wallet_name(), public_key()), 0..3),
        proptest::option::of(feature_flags()),
    )
        .prop_map(move |(network, telemetry, networks, wallet_specs, flags)| {
            let mut seen = existing_wallets.clone();
            let mut wallets = Vec::new();
            for (i, (name, public_key)) in wallet_specs.into_iter().enumerate() {
                let mut name = format!("overlay-{}", name);
                while !seen.insert(name.clone()) {
                    name = format!("{}-{}", name, i);
                }
                wallets.push(WalletEntry {
                    name,
                    public_key,
                    secret_key: None,
                    // Reserved network: always resolvable regardless of which
                    // networks the overlay happens to add.
                    network: "testnet".to_string(),
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                    funded: false,
                    rotation_history: Vec::new(),
                });
            }
            ConfigOverlay {
                network,
                telemetry_enabled: telemetry,
                wallet_encryption: None,
                feature_flags: flags,
                ai_telemetry: None,
                plugin_trust: None,
                networks,
                wallets,
            }
        })
}

// ─────────────────────────────────────────────────────────────────────────────
// Preservation properties (primary flow)
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    /// A generated configuration is one the validator accepts. If this fails,
    /// every property below is testing the wrong thing.
    #[test]
    fn generated_configs_are_valid(cfg in valid_config()) {
        prop_assert!(config::validate_config(&cfg).is_ok(), "generator produced an invalid config: {:?}", cfg);
    }

    /// TOML serialization round trips exactly.
    #[test]
    fn toml_round_trip_preserves_every_value(cfg in valid_config()) {
        let text = config::to_toml_string(&cfg).expect("serialize to TOML");
        let parsed = config::parse_config_str(&text).expect("parse TOML back");
        prop_assert_eq!(parsed, cfg);
    }

    /// JSON serialization (the database and export format) round trips exactly.
    #[test]
    fn json_round_trip_preserves_every_value(cfg in valid_config()) {
        let text = config::to_json_string(&cfg).expect("serialize to JSON");
        let parsed = config::parse_config_json(&text).expect("parse JSON back");
        prop_assert_eq!(parsed, cfg);
    }

    /// Crossing formats does not lose anything either: TOML → value → JSON →
    /// value must land on the same configuration.
    #[test]
    fn cross_format_round_trip_is_stable(cfg in valid_config()) {
        let via_toml = config::parse_config_str(&config::to_toml_string(&cfg).unwrap()).unwrap();
        let via_json = config::parse_config_json(&config::to_json_string(&via_toml).unwrap()).unwrap();
        prop_assert_eq!(via_json, cfg);
    }

    /// Serializing twice produces byte-identical TOML (no hidden state).
    #[test]
    fn serialization_is_deterministic(cfg in valid_config()) {
        let a = config::to_toml_string(&cfg).unwrap();
        let b = config::to_toml_string(&cfg).unwrap();
        prop_assert_eq!(a, b);
    }

    /// Merging an empty overlay is the identity.
    #[test]
    fn merging_an_empty_overlay_changes_nothing(cfg in valid_config()) {
        let merged = config::merge_configs(cfg.clone(), ConfigOverlay::default())
            .expect("merging an empty overlay must succeed");
        prop_assert_eq!(merged, cfg);
    }

    /// Merging is idempotent: applying the same overlay twice adds nothing the
    /// first application did not already add. (The second application is
    /// expected to fail on duplicate wallets, which is itself the guarantee —
    /// so wallets are dropped before re-applying.)
    #[test]
    fn merging_is_idempotent(
        (cfg, overlay) in valid_config().prop_flat_map(|c| {
            let c2 = c.clone();
            (Just(c), overlay_for(&c2))
        })
    ) {
        let once = config::merge_configs(cfg, overlay.clone()).expect("first merge");
        let without_wallets = ConfigOverlay { wallets: Vec::new(), ..overlay };
        let twice = config::merge_configs(once.clone(), without_wallets).expect("second merge");
        prop_assert_eq!(twice, once);
    }

    /// The overlay wins for every scalar it sets, and the base is preserved for
    /// every scalar it does not.
    #[test]
    fn overlay_takes_precedence_over_the_base(
        (cfg, overlay) in valid_config().prop_flat_map(|c| {
            let c2 = c.clone();
            (Just(c), overlay_for(&c2))
        })
    ) {
        let base = cfg.clone();
        let merged = config::merge_configs(cfg, overlay.clone()).expect("merge");

        match &overlay.network {
            Some(n) => prop_assert_eq!(&merged.network, n),
            None => prop_assert_eq!(&merged.network, &base.network),
        }
        match overlay.telemetry_enabled {
            Some(t) => prop_assert_eq!(merged.telemetry_enabled, Some(t)),
            None => prop_assert_eq!(merged.telemetry_enabled, base.telemetry_enabled),
        }

        // The installation identity is never taken from an overlay.
        prop_assert_eq!(&merged.version, &base.version);
        prop_assert_eq!(&merged.install_id, &base.install_id);

        // Every overlay network is present with the overlay's value.
        for (name, net) in &overlay.networks {
            prop_assert_eq!(merged.networks.get(name), Some(net));
        }
        // Base networks the overlay did not mention survive untouched.
        for (name, net) in &base.networks {
            if !overlay.networks.contains_key(name) {
                prop_assert_eq!(merged.networks.get(name), Some(net));
            }
        }
        // Wallets are appended, never replaced.
        prop_assert_eq!(merged.wallets.len(), base.wallets.len() + overlay.wallets.len());
    }

    /// A merged configuration is still a valid configuration that round trips.
    #[test]
    fn merged_configs_still_round_trip(
        (cfg, overlay) in valid_config().prop_flat_map(|c| {
            let c2 = c.clone();
            (Just(c), overlay_for(&c2))
        })
    ) {
        let merged = config::merge_configs(cfg, overlay).expect("merge");
        prop_assert!(config::validate_config(&merged).is_ok());

        let text = config::to_toml_string(&merged).unwrap();
        prop_assert_eq!(config::parse_config_str(&text).unwrap(), merged);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Rejection properties (malformed combinations)
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    /// An active network that is neither configured nor built in is rejected.
    #[test]
    fn unknown_active_network_is_rejected(
        cfg in valid_config(),
        stray in "[a-z][a-z0-9-]{3,15}",
    ) {
        prop_assume!(!cfg.networks.contains_key(&stray));
        prop_assume!(!config::is_reserved_network(&stray));

        let broken = Config { network: stray.clone(), ..cfg };
        let err = config::validate_config(&broken).unwrap_err();
        prop_assert!(err.to_string().contains(&stray), "error should name the network: {}", err);
    }

    /// Two wallets with the same name is a malformed combination even though
    /// each entry is individually well formed.
    #[test]
    fn duplicate_wallet_names_are_rejected(cfg in valid_config(), key in public_key()) {
        prop_assume!(!cfg.wallets.is_empty());

        let mut broken = cfg.clone();
        let clone_of = broken.wallets[0].clone();
        broken.wallets.push(WalletEntry { public_key: key, ..clone_of });

        let err = config::validate_config(&broken).unwrap_err();
        prop_assert!(err.to_string().contains("Duplicate wallet name"), "got: {}", err);
    }

    /// A wallet pointing at a network that is not configured is rejected.
    #[test]
    fn wallet_on_an_unknown_network_is_rejected(
        cfg in valid_config(),
        key in public_key(),
        stray in "[a-z][a-z0-9-]{3,15}",
    ) {
        prop_assume!(!cfg.networks.contains_key(&stray));
        prop_assume!(!config::is_reserved_network(&stray));

        let mut broken = cfg;
        broken.wallets.push(WalletEntry {
            name: "orphan-wallet".to_string(),
            public_key: key,
            secret_key: None,
            network: stray,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            funded: false,
            rotation_history: Vec::new(),
        });
        prop_assert!(config::validate_config(&broken).is_err());
    }

    /// Endpoints must be HTTP(S); any other scheme is refused.
    #[test]
    fn non_http_endpoints_are_rejected(
        cfg in valid_config(),
        scheme in proptest::sample::select(vec!["ftp", "file", "ws", "javascript"]),
        host in "[a-z]{3,10}\\.example\\.com",
    ) {
        let mut broken = cfg;
        let name = broken.networks.keys().next().unwrap().clone();
        broken.networks.get_mut(&name).unwrap().horizon_url = format!("{}://{}", scheme, host);

        let err = config::validate_config(&broken).unwrap_err();
        prop_assert!(err.to_string().contains("horizon_url"), "got: {}", err);
    }

    /// A malformed public key is rejected wherever it appears.
    #[test]
    fn malformed_public_keys_are_rejected(cfg in valid_config(), bad in "[a-z]{1,60}") {
        let mut broken = cfg;
        broken.wallets.push(WalletEntry {
            name: "bad-key-wallet".to_string(),
            public_key: bad,
            secret_key: None,
            network: broken.network.clone(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            funded: false,
            rotation_history: Vec::new(),
        });
        prop_assert!(config::validate_config(&broken).is_err());
    }

    /// An overlay whose wallet name collides with the base is rejected instead
    /// of silently replacing stored key material.
    #[test]
    fn overlay_wallet_collision_is_rejected(cfg in valid_config()) {
        prop_assume!(!cfg.wallets.is_empty());

        let clash = cfg.wallets[0].clone();
        let overlay = ConfigOverlay { wallets: vec![clash], ..Default::default() };
        let err = config::merge_configs(cfg, overlay).unwrap_err();
        prop_assert!(err.to_string().contains("already exists"), "got: {}", err);
    }

    /// An overlay that switches to an unconfigured network is rejected by the
    /// merge, not deferred to a later save.
    #[test]
    fn overlay_cannot_select_an_unknown_network(
        cfg in valid_config(),
        stray in "[a-z][a-z0-9-]{3,15}",
    ) {
        prop_assume!(!cfg.networks.contains_key(&stray));
        prop_assume!(!config::is_reserved_network(&stray));

        let overlay = ConfigOverlay { network: Some(stray), ..Default::default() };
        prop_assert!(config::merge_configs(cfg, overlay).is_err());
    }

    /// Unknown overlay keys are a hard error — a typo must not silently do
    /// nothing.
    #[test]
    fn unknown_overlay_keys_are_rejected(key in "[a-z_]{3,15}") {
        prop_assume!(![
            "network", "telemetry_enabled", "wallet_encryption",
            "feature_flags", "ai_telemetry", "plugin_trust", "networks", "wallets",
        ].contains(&key.as_str()));

        let doc = format!("{} = \"whatever\"\n", key);
        prop_assert!(config::parse_overlay_str(&doc).is_err(), "accepted unknown key {}", key);
    }

    /// Parsing arbitrary bytes never panics; it either succeeds or errors.
    #[test]
    fn parsing_arbitrary_text_never_panics(text in ".{0,400}") {
        let _ = config::parse_config_str(&text);
        let _ = config::parse_config_json(&text);
        let _ = config::parse_overlay_str(&text);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Boundary cases (fixed inputs at the edges of the accepted range)
// ─────────────────────────────────────────────────────────────────────────────

fn minimal_config() -> Config {
    let mut networks = HashMap::new();
    networks.insert(
        "testnet".to_string(),
        NetworkConfig {
            horizon_url: "https://horizon-testnet.stellar.org".to_string(),
            soroban_rpc_url: None,
            friendbot_url: None,
            passphrase: None,
        },
    );
    Config {
        version: "1".to_string(),
        network: "testnet".to_string(),
        telemetry_enabled: None,
        install_id: None,
        wallet_encryption: None,
        networks,
        plugin_trust: PluginTrustConfig::default(),
        feature_flags: FeatureFlagsConfig::default(),
        ai_telemetry: AiTelemetryConfig::default(),
        wallets: Vec::new(),
    }
}

#[test]
fn boundary_smallest_valid_config_round_trips() {
    let cfg = minimal_config();
    assert!(config::validate_config(&cfg).is_ok());

    let text = config::to_toml_string(&cfg).unwrap();
    assert_eq!(config::parse_config_str(&text).unwrap(), cfg);
}

#[test]
fn boundary_all_optional_fields_absent_survive_json() {
    let cfg = minimal_config();
    let text = config::to_json_string(&cfg).unwrap();
    let parsed = config::parse_config_json(&text).unwrap();

    assert_eq!(parsed.telemetry_enabled, None);
    assert_eq!(parsed.install_id, None);
    assert_eq!(parsed.wallet_encryption, None);
    assert_eq!(parsed, cfg);
}

#[test]
fn boundary_missing_optional_keys_fall_back_to_defaults() {
    // The smallest document the parser is required to accept.
    let doc = r#"
network = "testnet"
wallets = []
"#;
    let cfg = config::parse_config_str(doc).unwrap();
    assert_eq!(cfg.version, "1");
    assert_eq!(cfg.network, "testnet");
    assert!(cfg.networks.is_empty());
    assert_eq!(cfg.feature_flags, FeatureFlagsConfig::default());
    assert_eq!(cfg.plugin_trust, PluginTrustConfig::default());
}

#[test]
fn boundary_empty_version_is_rejected() {
    let cfg = Config {
        version: String::new(),
        ..minimal_config()
    };
    assert!(config::validate_config(&cfg).is_err());
}

#[test]
fn boundary_empty_networks_map_is_rejected() {
    let cfg = Config {
        networks: HashMap::new(),
        ..minimal_config()
    };
    assert!(config::validate_config(&cfg).is_err());
}

#[test]
fn boundary_whitespace_only_active_network_is_rejected() {
    let cfg = Config {
        network: "   ".to_string(),
        ..minimal_config()
    };
    assert!(config::validate_config(&cfg).is_err());
}

// ─────────────────────────────────────────────────────────────────────────────
// Failure cases (explicit, deterministic)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn failure_truncated_toml_is_rejected() {
    assert!(config::parse_config_str("network = \"testnet").is_err());
    assert!(config::parse_config_str("[networks.testnet").is_err());
}

#[test]
fn failure_wrong_type_for_a_known_key_is_rejected() {
    assert!(config::parse_config_str("network = 42\nwallets = []\n").is_err());
    assert!(config::parse_config_json(r#"{"network": true, "wallets": []}"#).is_err());
}

#[test]
fn failure_overlay_typo_is_reported_with_the_key() {
    let err = config::parse_overlay_str("netwrok = \"mainnet\"\n").unwrap_err();
    assert!(
        format!("{:#}", err).contains("netwrok"),
        "error should name the offending key: {:#}",
        err
    );
}

#[test]
fn failure_validation_does_not_depend_on_the_host_machine() {
    // `validate_config` must be pure: no disk, no database, no environment.
    // Running it repeatedly on the same value must give the same answer.
    let cfg = Config {
        network: "not-configured-anywhere".to_string(),
        ..minimal_config()
    };
    let first = config::validate_config(&cfg).is_err();
    for _ in 0..10 {
        assert_eq!(config::validate_config(&cfg).is_err(), first);
    }
    assert!(first);
}
