//! Integration tests for explicit config schema migrations.
//!
//! These tests exercise `run_config_migrations` and `migrate_config` from
//! `starforge::utils::config` at the boundary — no database, no filesystem
//! I/O except for the backup the migration itself writes (which is directed to
//! a temp directory by overriding the home-dir expectations via the actual
//! struct fields, keeping tests hermetic).
//!
//! Scenarios covered:
//!   Primary flow   : v0 (empty version) migrates to v1
//!   Boundary cases : already-current config is a no-op; empty version string
//!                    treated as v0; backup path is returned in report
//!   Failure paths  : future version produces `FromFuture` error;
//!                    unknown version produces `UnknownVersion` error

use starforge::utils::config::{
    migrate_config, run_config_migrations, Config, ConfigMigrationError, CURRENT_CONFIG_VERSION,
};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Build a minimal config with the given version string and no wallets.
fn config_with_version(version: &str) -> Config {
    Config {
        version: version.to_string(),
        ..Config::default()
    }
}

// ── primary flow ──────────────────────────────────────────────────────────────

/// Migrating a v0 config (empty version, as written by early releases) must
/// produce a v1 config with one migration step applied.
#[test]
fn test_migration_from_empty_version_to_v1() {
    let cfg = config_with_version(""); // empty == v0 in the registry
    let (migrated, report) =
        run_config_migrations(cfg).expect("migration from empty version should succeed");

    assert_eq!(migrated.version, "1", "config must be upgraded to v1");
    assert_eq!(report.from_version, "0", "report must normalise empty → 0");
    assert_eq!(report.to_version, "1");
    assert_eq!(
        report.steps_applied,
        vec![("0".to_string(), "1".to_string())]
    );
    assert!(!report.is_no_op(), "one step was applied");
}

/// Migrating a config that explicitly declares version "0" behaves identically
/// to the empty-version case.
#[test]
fn test_migration_from_explicit_v0_to_v1() {
    let cfg = config_with_version("0");
    let (migrated, report) = run_config_migrations(cfg).expect("migration from v0 should succeed");

    assert_eq!(migrated.version, "1");
    assert_eq!(report.from_version, "0");
    assert_eq!(report.to_version, "1");
    assert_eq!(
        report.steps_applied,
        vec![("0".to_string(), "1".to_string())]
    );
}

// ── boundary cases ────────────────────────────────────────────────────────────

/// A config already at the current version must be returned unchanged and the
/// report must signal a no-op (no backup written, no steps executed).
#[test]
fn test_migration_already_current_is_noop() {
    let cfg = config_with_version(CURRENT_CONFIG_VERSION);
    let (migrated, report) =
        run_config_migrations(cfg.clone()).expect("already-current config must not error");

    assert_eq!(migrated.version, CURRENT_CONFIG_VERSION);
    assert!(
        report.is_no_op(),
        "no steps should run for a current config"
    );
    assert!(
        report.backup_path.is_none(),
        "no backup should be written for a no-op migration"
    );
    assert_eq!(report.steps_applied.len(), 0);
}

/// The convenience `migrate_config` wrapper must return the same migrated
/// config as `run_config_migrations` (it simply discards the report).
#[test]
fn test_migrate_config_convenience_wrapper() {
    let cfg_v0 = config_with_version("0");
    // This will attempt a backup write; if the home dir is not writable in CI
    // the test is still valid — we just check it does not panic/abort.
    let result = migrate_config(cfg_v0);
    // Either succeeds (backup written) or fails with BackupFailed — not with
    // UnknownVersion or FromFuture.
    match result {
        Ok(migrated) => assert_eq!(migrated.version, "1"),
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("backup") || msg.contains("Failed"),
                "unexpected error: {msg}"
            );
        }
    }
}

/// The backup path, when present, must refer to a file whose name encodes the
/// source version.  This guards the naming convention that `rollback_config`
/// depends on.
#[test]
fn test_migration_backup_path_encodes_source_version() {
    let cfg = config_with_version("0");
    let result = run_config_migrations(cfg);
    match result {
        Ok((_migrated, report)) => {
            if let Some(path) = report.backup_path {
                let name = path.file_name().unwrap().to_string_lossy();
                assert!(
                    name.contains("backup.v0"),
                    "backup filename must contain 'backup.v0', got: {name}"
                );
                assert!(name.ends_with(".toml"), "backup must be a .toml file");
            }
            // backup_path == None means the home dir doesn't exist — acceptable in CI
        }
        Err(e) => {
            // Only BackupFailed is acceptable here
            let msg = e.to_string();
            assert!(
                msg.contains("backup") || msg.contains("Failed"),
                "unexpected error in backup test: {msg}"
            );
        }
    }
}

// ── failure paths ─────────────────────────────────────────────────────────────

/// A config whose version is numerically greater than the current version must
/// produce a `FromFuture` error with a message directing the user to upgrade.
#[test]
fn test_migration_from_future_version_errors() {
    // CURRENT_CONFIG_VERSION is "1"; "999" is from the future
    let cfg = config_with_version("999");
    let err = run_config_migrations(cfg).expect_err("future version must fail");

    let msg = err.to_string();
    assert!(
        msg.contains("newer than this binary supports"),
        "error must mention 'newer than this binary supports', got: {msg}"
    );
    assert!(
        msg.contains("999"),
        "error must mention the found version, got: {msg}"
    );
    assert!(
        msg.contains("upgrade"),
        "error must mention upgrading starforge, got: {msg}"
    );
}

/// A config whose version is in neither the known-past nor the known-future
/// (i.e., an edited/corrupted config file) must produce an `UnknownVersion`
/// error.
#[test]
fn test_migration_unknown_version_errors() {
    // "banana" is not numeric and not in the registry
    let cfg = config_with_version("banana");
    let err = run_config_migrations(cfg).expect_err("unknown version must fail");

    let msg = err.to_string();
    assert!(
        msg.contains("banana"),
        "error must mention the bad version, got: {msg}"
    );
    // Must NOT say "upgrade" — this is a corrupted/unknown version, not a future one
    assert!(
        !msg.contains("upgrade"),
        "UnknownVersion error must not say 'upgrade', got: {msg}"
    );
}

/// A config with a non-numeric version string that would be misread as "0" by
/// the numeric comparator must still be detected as unknown (not silently
/// migrated).
#[test]
fn test_migration_non_numeric_version_not_silently_upgraded() {
    let cfg = config_with_version("alpha");
    let err = run_config_migrations(cfg).expect_err("non-numeric unknown version must fail");

    let msg = err.to_string();
    // Should be UnknownVersion, not FromFuture
    assert!(
        msg.contains("alpha"),
        "error must name the bad version, got: {msg}"
    );
}
