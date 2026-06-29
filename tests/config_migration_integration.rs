//! Integration tests for the versioned config migration system.
//!
//! These load `tests/fixtures/config_v1.toml`, run the migration pipeline, and
//! assert the result matches the expected current `Config` — proving that a
//! legacy config upgrades cleanly *without silently dropping wallet entries*.

use starforge::utils::config::{self, migrations, Config};

fn load_fixture(name: &str) -> serde_json::Value {
    let path = format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name);
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"));
    toml::from_str::<serde_json::Value>(&contents)
        .unwrap_or_else(|e| panic!("failed to parse fixture {path}: {e}"))
}

#[test]
fn v1_fixture_migrates_to_current_config() {
    let raw = load_fixture("config_v1.toml");
    assert_eq!(
        migrations::read_version(&raw),
        1,
        "fixture should start at v1"
    );

    let outcome = config::migrate_json_value(&raw).expect("migration should succeed");
    assert!(outcome.migrated(), "v1 fixture must trigger a migration");
    assert_eq!(outcome.steps, vec![(1, 2)]);

    // The migrated value must deserialize into the current Config struct.
    let cfg: Config =
        serde_json::from_value(outcome.value).expect("migrated value must deserialize into Config");

    // Version bumped to current.
    assert_eq!(cfg.version, config::CURRENT_CONFIG_VERSION);
    assert_eq!(cfg.network, "testnet");

    // Critically: NO wallet was dropped, and the legacy `secret` field was
    // preserved by renaming it to `secret_key`.
    assert_eq!(cfg.wallets.len(), 2, "no wallet entries may be lost");

    let legacy = cfg
        .wallets
        .iter()
        .find(|w| w.name == "legacy-wallet")
        .expect("legacy-wallet must survive migration");
    assert_eq!(
        legacy.secret_key.as_deref(),
        Some("SAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWNT"),
        "legacy `secret` field must be migrated to `secret_key`, not dropped"
    );

    let modern = cfg
        .wallets
        .iter()
        .find(|w| w.name == "modern-wallet")
        .expect("modern-wallet must survive migration");
    assert_eq!(
        modern.secret_key.as_deref(),
        Some("SBBZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWNT"),
        "existing `secret_key` must be left untouched"
    );

    // Flat KDF keys were folded into the structured wallet_encryption table.
    let kdf = cfg
        .wallet_encryption
        .expect("legacy flat KDF keys must become wallet_encryption");
    assert_eq!(kdf.mem, Some(65536));
    assert_eq!(kdf.iterations, Some(4));
    assert_eq!(kdf.parallelism, Some(2));

    // Legacy `telemetry` flag became `telemetry_enabled`.
    assert_eq!(cfg.telemetry_enabled, Some(false));
}

#[test]
fn migration_is_idempotent() {
    let raw = load_fixture("config_v1.toml");
    let first = config::migrate_json_value(&raw).expect("first migration");

    // Re-running migrations on an already-migrated value is a no-op.
    let second = config::migrate_json_value(&first.value).expect("second migration");
    assert!(
        !second.migrated(),
        "migrating an already-current config must do nothing"
    );
    assert!(second.changes.is_empty());
}

#[test]
fn dry_run_diff_is_non_empty_for_legacy_config() {
    let raw = load_fixture("config_v1.toml");
    let outcome = config::migrate_json_value(&raw).expect("migration");

    // The diff that `config migrate --dry-run` shows must mention the key shape
    // changes a user cares about.
    let rendered: Vec<String> = outcome.changes.iter().map(|c| c.to_string()).collect();
    let joined = rendered.join("\n");

    assert!(joined.contains("version"), "diff must mention version bump");
    assert!(
        joined.contains("secret_key"),
        "diff must mention the renamed wallet secret field"
    );
    assert!(
        joined.contains("telemetry_enabled"),
        "diff must mention telemetry rename"
    );
    assert!(
        joined.contains("wallet_encryption"),
        "diff must mention KDF restructuring"
    );
}

#[test]
fn current_default_config_needs_no_migration() {
    let cfg = Config::default();
    let value = serde_json::to_value(&cfg).expect("serialize default config");
    let outcome = config::migrate_json_value(&value).expect("migration");
    assert!(
        !outcome.migrated(),
        "a freshly created default config is already current"
    );
}
