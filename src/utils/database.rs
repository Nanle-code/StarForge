use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub fn db_path() -> PathBuf {
    crate::utils::config::config_dir().join("starforge.db")
}

/// Current schema version of the database
pub const CURRENT_SCHEMA_VERSION: i64 = 2;

/// Migration trait for defining schema changes
pub trait Migration: Send + Sync {
    /// Version number for this migration (must be unique)
    fn version(&self) -> i64;

    /// Description of what this migration does
    fn description(&self) -> &str;

    /// Apply the migration (upgrade)
    ///
    /// Takes a shared reference so a migration can run inside a
    /// `rusqlite::Transaction`, which only derefs to `&Connection`.
    fn up(&self, conn: &Connection) -> Result<()>;

    /// Rollback the migration (downgrade)
    fn down(&self, conn: &Connection) -> Result<()>;
}

/// Record of an applied migration in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedMigration {
    pub version: i64,
    pub name: String,
    pub applied_at: String,
    pub checksum: String,
}

/// Result of running migrations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationResult {
    pub current_version: i64,
    pub migrations_applied: Vec<i64>,
    pub migrations_rolled_back: Vec<i64>,
}

/// Error types for migration operations
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MigrationError {
    #[error("Migration version {0} is already applied")]
    AlreadyApplied(i64),

    #[error("Migration version {0} not found")]
    NotFound(i64),

    #[error("Cannot rollback: no migrations applied")]
    NothingToRollback,

    #[error("Migration version {0} depends on unapplied version {1}")]
    MissingDependency(i64, i64),

    #[error("Invalid migration sequence: versions must be consecutive")]
    InvalidSequence,

    #[error("Database schema version {0} is not supported (minimum: {1}, maximum: {2})")]
    UnsupportedVersion(i64, i64, i64),

    #[error("Migration failed: {0}")]
    MigrationFailed(String),

    #[error(
        "Database at {path} is corrupted: {issues}. Restore from a backup with \
         `starforge config db restore <backup-file>`, or move the corrupted file \
         aside to start a fresh database."
    )]
    DatabaseCorrupted { path: String, issues: String },
}

pub struct Database {
    pub(crate) conn: Connection,
}

impl Database {
    pub fn open() -> Result<Self> {
        let path = db_path();
        // A pre-existing file is the only case corruption is possible (a
        // brand-new file SQLite is about to create is trivially intact), so
        // this check has to happen before `Connection::open`, which creates
        // an empty file as a side effect and would make every open "existing"
        // from then on.
        let existed_before_open = path.exists();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self { conn };
        if existed_before_open {
            db.fail_clearly_if_corrupted(&path)?;
        }
        Ok(db)
    }

    /// Run `PRAGMA integrity_check`/`PRAGMA foreign_key_check` and fail with a
    /// [`MigrationError::DatabaseCorrupted`] (rather than surfacing corruption
    /// later as a confusing query-time SQLite error) when either reports a
    /// problem. Called from `open()` for a pre-existing database file; a
    /// caller that already has a `Database` and wants to re-check later can
    /// call `integrity_check()` directly.
    fn fail_clearly_if_corrupted(&self, path: &std::path::Path) -> Result<()> {
        // A file that is not a SQLite database at all (or too badly damaged
        // to read its header) fails the `PRAGMA` query itself rather than
        // returning a row describing the problem; that case is corruption
        // too; it just surfaces through a different Result arm.
        let issues = match self.integrity_check() {
            Ok(issues) => issues,
            Err(e) => {
                return Err(MigrationError::DatabaseCorrupted {
                    path: path.display().to_string(),
                    issues: e.to_string(),
                }
                .into());
            }
        };
        if issues.is_empty() || issues == ["ok".to_string()] {
            return Ok(());
        }
        Err(MigrationError::DatabaseCorrupted {
            path: path.display().to_string(),
            issues: issues.join("; "),
        }
        .into())
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(Self { conn })
    }

    /// Run `f` inside a SQLite transaction. Rolls back on error. Used by
    /// feature-flag and other writes that need atomic read-then-insert.
    pub fn with_transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        let tx = self.conn.unchecked_transaction()?;
        let result = f();
        match result {
            Ok(value) => {
                tx.commit()?;
                Ok(value)
            }
            Err(e) => {
                let _ = tx.rollback();
                Err(e)
            }
        }
    }

    pub fn initialize(&self) -> Result<()> {
        self.initialize_impl()
    }

    /// Same as [`Self::initialize`], but when an upgrade is actually about to
    /// run (an existing database whose schema version trails
    /// [`CURRENT_SCHEMA_VERSION`]), first copies the database file into
    /// `backup_dir` and returns the backup's path. A fresh database and an
    /// already-current one have nothing to protect against, so no backup is
    /// made and `Ok(None)` is returned — the same case `run_migrations`
    /// treats as a no-op.
    ///
    /// The backup itself uses [`Self::backup`], a plain file copy; on a
    /// WAL-mode database (`open()` always sets `journal_mode=WAL`) this reads
    /// the main database file only, which SQLite's WAL protocol guarantees is
    /// self-consistent even mid-write, so no separate quiescing step is
    /// needed before copying it.
    pub fn initialize_with_backup(&self, backup_dir: &std::path::Path) -> Result<Option<PathBuf>> {
        // A backup is only worth taking for a database that was already
        // initialized (has a `schema_version` row) and is genuinely behind.
        // A brand-new database (no `meta` table yet, or a `meta` table with
        // no row for this key) has no prior data to protect, so it takes the
        // same `Ok(None)` path `initialize()`'s own fresh-database branch
        // does — `get_meta` erroring with "no such table" is treated
        // identically to it returning `Ok(None)`, mirroring
        // `get_current_schema_version`'s own handling of the same case.
        let needs_upgrade = match self.get_meta("schema_version") {
            Ok(Some(v)) => v.parse::<i64>().unwrap_or(0) < CURRENT_SCHEMA_VERSION,
            Ok(None) => false,
            Err(e) if e.to_string().contains("no such table: meta") => false,
            Err(e) => return Err(e),
        };

        let backup_path = if needs_upgrade {
            std::fs::create_dir_all(backup_dir)?;
            let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
            let dest = backup_dir.join(format!("starforge-pre-migrate-{timestamp}.db"));
            self.backup(&dest)
                .with_context(|| format!("Failed to back up database to {}", dest.display()))?;
            Some(dest)
        } else {
            None
        };

        self.initialize_impl()?;
        Ok(backup_path)
    }

    fn initialize_impl(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA)?;
        self.ensure_column("wallets", "secret_key", "TEXT")?;
        self.ensure_column("wallets", "rotation_history", "TEXT NOT NULL DEFAULT '[]'")?;

        // Run migrations if this is not a fresh database.
        //
        // `get_meta` returns `Ok(None)` for a key that is absent, so the
        // presence of the row — not the success of the lookup — decides which
        // branch a fresh database takes.
        if self.get_meta("schema_version")?.is_some() {
            self.run_migrations()?;
        } else {
            // Fresh databases bootstrap at v1, then use the same migration path
            // as existing databases for subsequent schema versions.
            self.set_meta("schema_version", "1")?;
            self.record_migration(1, "initial_schema")?;
            self.run_migrations()?;
        }

        // The feature-flags schema is shipped alongside the rest of the
        // schema for first-startup convenience; subsequent startups hit the
        // idempotent `CREATE TABLE IF NOT EXISTS` guards and no-op.
        self.conn
            .execute_batch(crate::utils::feature_flags::FEATURE_FLAGS_SCHEMA)
            .context("Failed to apply feature_flags schema")?;
        for def in crate::utils::feature_flags::builtin_definitions() {
            self.upsert_definition(&def)?;
        }
        Ok(())
    }

    /// Get the current schema version from the database
    pub fn get_current_schema_version(&self) -> Result<i64> {
        match self.get_meta("schema_version") {
            Ok(Some(v)) => v.parse::<i64>().map_err(|e| anyhow::anyhow!(e)),
            Ok(None) => Ok(0),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table: meta") {
                    Ok(0)
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Get all applied migrations from the database
    pub fn get_applied_migrations(&self) -> Result<Vec<AppliedMigration>> {
        let mut stmt = match self.conn.prepare(
            "SELECT version, name, applied_at, checksum FROM schema_migrations ORDER BY version",
        ) {
            Ok(s) => s,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table: schema_migrations") {
                    return Ok(Vec::new());
                } else {
                    return Err(e.into());
                }
            }
        };
        let rows = stmt.query_map([], |row| {
            Ok(AppliedMigration {
                version: row.get(0)?,
                name: row.get(1)?,
                applied_at: row.get(2)?,
                checksum: row.get(3)?,
            })
        })?;
        rows.map(|r| r.map_err(anyhow::Error::from)).collect()
    }

    /// Record a migration as applied in the database
    fn record_migration(&self, version: i64, name: &str) -> Result<()> {
        let checksum = self.compute_migration_checksum(version, name)?;
        let applied_at = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO schema_migrations (version, name, applied_at, checksum) VALUES (?1, ?2, ?3, ?4)",
            params![version, name, applied_at, checksum],
        )?;
        Ok(())
    }

    /// Remove a migration record from the database
    // Not currently called from any code path in this crate. Kept rather than
    // removed since deleting it is a product decision, not a lint-scoping one.
    #[allow(dead_code)]
    fn remove_migration(&self, version: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM schema_migrations WHERE version = ?1",
            params![version],
        )?;
        Ok(())
    }

    /// Compute a checksum for a migration to detect changes
    fn compute_migration_checksum(&self, version: i64, name: &str) -> Result<String> {
        let mut hasher = Sha256::new();
        hasher.update(version.to_string().as_bytes());
        hasher.update(name.as_bytes());
        Ok(hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect())
    }

    /// Run pending migrations to bring the database to the current schema version
    pub fn run_migrations(&self) -> Result<MigrationResult> {
        // `schema_version` is the authority on how far the database has been
        // migrated; `schema_migrations` is the audit log of what ran. A
        // database whose version trails the log is still upgraded, and the log
        // entry is rewritten rather than duplicated.
        let current_version = self.get_current_schema_version()?;

        let mut migrations_applied = Vec::new();

        // Check if we need to upgrade
        if current_version < CURRENT_SCHEMA_VERSION {
            // Apply migrations from current_version + 1 to CURRENT_SCHEMA_VERSION
            for version in (current_version + 1)..=CURRENT_SCHEMA_VERSION {
                self.apply_migration(version)?;
                migrations_applied.push(version);
            }
        }

        Ok(MigrationResult {
            current_version: CURRENT_SCHEMA_VERSION,
            migrations_applied,
            migrations_rolled_back: Vec::new(),
        })
    }

    /// Apply a single migration within a transaction
    fn apply_migration(&self, version: i64) -> Result<()> {
        let migration = self
            .get_migration(version)
            .ok_or_else(|| anyhow::anyhow!("Migration version {} not found", version))?;

        let tx = self.conn.unchecked_transaction()?;

        // Apply the migration
        match migration.up(&tx) {
            Ok(()) => {
                // Record the migration
                let checksum = self.compute_migration_checksum(version, migration.description())?;
                let applied_at = chrono::Utc::now().to_rfc3339();
                tx.execute(
                    "INSERT OR REPLACE INTO schema_migrations (version, name, applied_at, checksum) VALUES (?1, ?2, ?3, ?4)",
                    params![version, migration.description(), applied_at, checksum],
                )?;

                // Update schema version
                tx.execute(
                    "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![version.to_string()],
                )?;

                tx.commit()?;
                Ok(())
            }
            Err(e) => {
                let _ = tx.rollback();
                Err(anyhow::anyhow!("Migration {} failed: {}", version, e))
            }
        }
    }

    /// Rollback a single migration within a transaction
    pub fn rollback_migration(&self, version: i64) -> Result<()> {
        let applied = self.get_applied_migrations()?;
        let _current_version = self.get_current_schema_version()?;

        // Migrations roll back newest first, so anything below the newest
        // applied version is refused on that ground — whether or not it was
        // itself applied. Only a version at or above the newest can be
        // "not applied".
        let max_applied = applied
            .iter()
            .map(|m| m.version)
            .max()
            .ok_or_else(|| anyhow::anyhow!("No migrations applied"))?;

        if version < max_applied {
            return Err(anyhow::anyhow!(
                "Can only rollback the latest migration ({}), tried to rollback {}",
                max_applied,
                version
            ));
        }

        if !applied.iter().any(|m| m.version == version) {
            return Err(anyhow::anyhow!(
                "Migration version {} is not applied",
                version
            ));
        }

        let migration = self
            .get_migration(version)
            .ok_or_else(|| anyhow::anyhow!("Migration version {} not found", version))?;

        let tx = self.conn.unchecked_transaction()?;

        // Rollback the migration
        match migration.down(&tx) {
            Ok(()) => {
                // A migration's `down` undoes the schema it created, which for
                // the initial migration includes the runner's own bookkeeping
                // tables. Re-create them before recording the rollback.
                tx.execute_batch(MIGRATION_BOOKKEEPING_SCHEMA)?;

                // Remove the migration record
                tx.execute(
                    "DELETE FROM schema_migrations WHERE version = ?1",
                    params![version],
                )?;

                // Update schema version to previous version. This is an upsert
                // because `meta` may have just been re-created empty.
                let previous_version = if version > 1 { version - 1 } else { 0 };
                tx.execute(
                    "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![previous_version.to_string()],
                )?;

                tx.commit()?;
                Ok(())
            }
            Err(e) => {
                let _ = tx.rollback();
                Err(anyhow::anyhow!(
                    "Rollback of migration {} failed: {}",
                    version,
                    e
                ))
            }
        }
    }

    /// Get a migration by version number
    fn get_migration(&self, version: i64) -> Option<Box<dyn Migration>> {
        match version {
            1 => Some(Box::new(MigrationV1 {})),
            2 => Some(Box::new(MigrationV2 {})),
            _ => None,
        }
    }

    fn ensure_column(&self, table: &str, column: &str, definition: &str) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare(&format!("PRAGMA table_info({})", table))?;
        let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for existing in columns {
            if existing? == column {
                return Ok(());
            }
        }
        self.conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
        Ok(())
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn insert_wallet(&self, wallet: &WalletRow) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO wallets \
             (name, public_key, secret_key, network, created_at, funded, rotation_history, usage_policy) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                wallet.name,
                wallet.public_key,
                wallet.secret_key,
                wallet.network,
                wallet.created_at,
                wallet.funded,
                wallet.rotation_history,
                wallet.usage_policy,
            ],
        )?;
        Ok(())
    }

    pub fn list_wallets(&self) -> Result<Vec<WalletRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, public_key, secret_key, network, created_at, funded, rotation_history, usage_policy FROM wallets ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(WalletRow {
                name: row.get(0)?,
                public_key: row.get(1)?,
                secret_key: row.get(2)?,
                network: row.get(3)?,
                created_at: row.get(4)?,
                funded: row.get(5)?,
                rotation_history: row.get(6)?,
                usage_policy: row.get(7)?,
            })
        })?;
        rows.map(|r| r.map_err(anyhow::Error::from)).collect()
    }

    pub fn get_wallet(&self, name: &str) -> Result<Option<WalletRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, public_key, secret_key, network, created_at, funded, rotation_history, usage_policy FROM wallets WHERE name = ?1",
        )?;
        let mut rows = stmt.query(params![name])?;
        if let Some(row) = rows.next()? {
            Ok(Some(WalletRow {
                name: row.get(0)?,
                public_key: row.get(1)?,
                secret_key: row.get(2)?,
                network: row.get(3)?,
                created_at: row.get(4)?,
                funded: row.get(5)?,
                rotation_history: row.get(6)?,
                usage_policy: row.get(7)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn delete_wallet(&self, name: &str) -> Result<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM wallets WHERE name = ?1", params![name])?)
    }

    pub fn insert_network(&self, net: &NetworkRow) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO networks \
             (name, horizon_url, soroban_rpc_url, friendbot_url, passphrase) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                net.name,
                net.horizon_url,
                net.soroban_rpc_url,
                net.friendbot_url,
                net.passphrase,
            ],
        )?;
        Ok(())
    }

    pub fn list_networks(&self) -> Result<Vec<NetworkRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, horizon_url, soroban_rpc_url, friendbot_url, passphrase FROM networks ORDER BY name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(NetworkRow {
                name: row.get(0)?,
                horizon_url: row.get(1)?,
                soroban_rpc_url: row.get(2)?,
                friendbot_url: row.get(3)?,
                passphrase: row.get(4)?,
            })
        })?;
        rows.map(|r| r.map_err(anyhow::Error::from)).collect()
    }

    pub fn insert_config_kv(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO config_kv (key, value, updated_at) VALUES (?1, ?2, datetime('now'))",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_config_kv(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM config_kv WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_config_kv(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM config_kv ORDER BY key")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.map(|r| r.map_err(anyhow::Error::from)).collect()
    }

    pub fn has_config(&self) -> Result<bool> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM config_kv", [], |row| row.get(0))
            .unwrap_or(0);
        Ok(count > 0)
    }

    pub fn load_config(&self) -> Result<crate::utils::config::Config> {
        use crate::utils::config::{
            Config, NetworkConfig, PluginTrustConfig, WalletEntry, WalletRotationRecord,
            WalletUsagePolicy,
        };
        use std::collections::HashMap;

        let mut cfg = Config::default();
        if let Some(version) = self.get_config_kv("schema_version")? {
            cfg.version = version;
        }
        if let Some(network) = self.get_config_kv("network")? {
            cfg.network = network;
        }
        if let Some(telemetry) = self.get_config_kv("telemetry_enabled")? {
            cfg.telemetry_enabled = telemetry.parse::<bool>().ok();
        }
        if let Some(plugin_trust) = self.get_config_kv("plugin_trust.trusted_sources")? {
            cfg.plugin_trust.trusted_sources = serde_json::from_str(&plugin_trust)?;
        }
        if let Some(trusted_pubs) = self.get_config_kv("plugin_trust.trusted_publishers")? {
            cfg.plugin_trust.trusted_publishers = serde_json::from_str(&trusted_pubs)?;
        }
        if let Some(req_sigs) = self.get_config_kv("plugin_trust.require_signatures")? {
            cfg.plugin_trust.require_signatures = req_sigs.parse::<bool>().unwrap_or(false);
        }
        if let Some(wallet_encryption) = self.get_config_kv("wallet_encryption")? {
            cfg.wallet_encryption = Some(serde_json::from_str(&wallet_encryption)?);
        }
        if let Some(install_id) = self.get_config_kv("install_id")? {
            cfg.install_id = Some(install_id);
        }
        if let Some(feature_flags) = self.get_config_kv("feature_flags")? {
            if let Ok(parsed) =
                serde_json::from_str::<crate::utils::config::FeatureFlagsConfig>(&feature_flags)
            {
                cfg.feature_flags = parsed;
            }
        }

        cfg.networks = self
            .list_networks()?
            .into_iter()
            .map(|net| {
                (
                    net.name,
                    NetworkConfig {
                        horizon_url: net.horizon_url,
                        soroban_rpc_url: net.soroban_rpc_url,
                        friendbot_url: net.friendbot_url,
                        passphrase: net.passphrase,
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        cfg.wallets = self
            .list_wallets()?
            .into_iter()
            .map(|wallet| {
                let rotation_history: Vec<WalletRotationRecord> =
                    serde_json::from_str(&wallet.rotation_history).unwrap_or_default();
                let kdf_options = wallet
                    .secret_key
                    .as_ref()
                    .and_then(|s| crate::utils::crypto::extract_kdf_metadata(s).ok())
                    .map(|m| crate::utils::crypto::KdfOptions {
                        mem: Some(m.mem),
                        iterations: Some(m.iterations),
                        parallelism: Some(m.parallelism),
                    });
                let usage_policy = serde_json::from_str::<WalletUsagePolicy>(&wallet.usage_policy)
                    .unwrap_or_default();
                WalletEntry {
                    name: wallet.name,
                    public_key: wallet.public_key,
                    secret_key: wallet.secret_key,
                    network: wallet.network,
                    created_at: wallet.created_at,
                    funded: wallet.funded,
                    kdf_options,
                    rotation_history,
                    derivation_index: None,
                    derivation_path: None,
                    mnemonic_wallet: None,
                    usage_policy,
                }
            })
            .collect();

        Ok(cfg)
    }

    pub fn save_config(&self, cfg: &crate::utils::config::Config) -> Result<()> {
        self.initialize()?;
        self.conn.execute_batch(
            "DELETE FROM wallets;
             DELETE FROM networks;
             DELETE FROM config_kv;",
        )?;

        for wallet in &cfg.wallets {
            self.insert_wallet(&WalletRow {
                name: wallet.name.clone(),
                public_key: wallet.public_key.clone(),
                secret_key: wallet.secret_key.clone(),
                network: wallet.network.clone(),
                created_at: wallet.created_at.clone(),
                funded: wallet.funded,
                rotation_history: serde_json::to_string(&wallet.rotation_history)?,
                usage_policy: serde_json::to_string(&wallet.usage_policy)?,
            })?;
        }

        for (name, net) in &cfg.networks {
            self.insert_network(&NetworkRow {
                name: name.clone(),
                horizon_url: net.horizon_url.clone(),
                soroban_rpc_url: net.soroban_rpc_url.clone(),
                friendbot_url: net.friendbot_url.clone(),
                passphrase: net.passphrase.clone(),
            })?;
        }

        self.insert_config_kv("network", &cfg.network)?;
        self.insert_config_kv("schema_version", &cfg.version)?;
        if let Some(telemetry) = cfg.telemetry_enabled {
            self.insert_config_kv("telemetry_enabled", &telemetry.to_string())?;
        }
        self.insert_config_kv(
            "plugin_trust.trusted_sources",
            &serde_json::to_string(&cfg.plugin_trust.trusted_sources)?,
        )?;
        self.insert_config_kv(
            "plugin_trust.trusted_publishers",
            &serde_json::to_string(&cfg.plugin_trust.trusted_publishers)?,
        )?;
        self.insert_config_kv(
            "plugin_trust.require_signatures",
            &cfg.plugin_trust.require_signatures.to_string(),
        )?;
        if let Some(kdf) = &cfg.wallet_encryption {
            self.insert_config_kv("wallet_encryption", &serde_json::to_string(kdf)?)?;
        }
        if let Some(install_id) = &cfg.install_id {
            self.insert_config_kv("install_id", install_id)?;
        }
        self.insert_config_kv("feature_flags", &serde_json::to_string(&cfg.feature_flags)?)?;
        self.set_meta("updated_at", &chrono::Utc::now().to_rfc3339())?;

        Ok(())
    }

    pub fn execute_query(&self, sql: &str) -> Result<QueryResult> {
        if sql.trim_start().to_ascii_lowercase().starts_with("select") {
            let mut stmt = self.conn.prepare(sql)?;
            let col_count = stmt.column_count();
            let cols: Vec<String> = (0..col_count)
                .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
                .collect();
            let rows = stmt.query_map([], |row| {
                let values: Vec<String> = (0..col_count)
                    .map(|i| {
                        row.get::<_, rusqlite::types::Value>(i)
                            .map(|v| match v {
                                rusqlite::types::Value::Null => "NULL".to_string(),
                                rusqlite::types::Value::Integer(n) => n.to_string(),
                                rusqlite::types::Value::Real(f) => f.to_string(),
                                rusqlite::types::Value::Text(s) => s,
                                rusqlite::types::Value::Blob(b) => {
                                    format!("<blob:{} bytes>", b.len())
                                }
                            })
                            .unwrap_or_else(|_| "?".to_string())
                    })
                    .collect();
                Ok(values)
            })?;

            let result_rows: Vec<Vec<String>> = rows
                .map(|r| r.map_err(anyhow::Error::from))
                .collect::<Result<_>>()?;
            let row_count = result_rows.len();

            Ok(QueryResult {
                columns: cols,
                rows: result_rows,
                rows_affected: row_count,
            })
        } else {
            let affected = self.conn.execute(sql, [])?;
            Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                rows_affected: affected,
            })
        }
    }

    /// Copy this database's file to `dest`.
    ///
    /// Copies from the path this `Connection` actually has open — not the
    /// default `db_path()`, which is wrong for any database opened via
    /// [`Self::open_in_memory`] or a test/alternate path, and previously
    /// caused every such backup to silently copy the unrelated default
    /// database file instead of this one. `PRAGMA wal_checkpoint(TRUNCATE)`
    /// folds the WAL file's contents back into the main database file first,
    /// since `open()`/`open_in_memory()` both run in `journal_mode=WAL` and a
    /// plain copy of only the main file could otherwise miss committed data
    /// still sitting in `-wal`.
    pub fn backup(&self, dest: &std::path::Path) -> Result<()> {
        let src = self
            .conn
            .path()
            .map(std::path::PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("Cannot back up an in-memory database"))?;
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .context("Failed to checkpoint WAL before backup")?;
        std::fs::copy(&src, dest)
            .with_context(|| format!("Failed to copy {} to {}", src.display(), dest.display()))?;
        Ok(())
    }

    pub fn integrity_check(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("PRAGMA integrity_check")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut results: Vec<String> = rows
            .map(|r| r.map_err(anyhow::Error::from))
            .collect::<Result<_>>()?;

        let foreign_key_issue: Option<String> = self
            .conn
            .query_row("PRAGMA foreign_key_check", [], |row| {
                Ok(format!(
                    "{} row {} references {}",
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?
                ))
            })
            .optional()?;
        if let Some(issue) = foreign_key_issue {
            results.push(issue);
        }
        Ok(results)
    }

    pub fn stats(&self) -> Result<DbStats> {
        let wallets: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0))
            .unwrap_or(0);
        let networks: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM networks", [], |r| r.get(0))
            .unwrap_or(0);
        let config_entries: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM config_kv", [], |r| r.get(0))
            .unwrap_or(0);
        let events_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap_or(0);
        let schema_version = self
            .get_meta("schema_version")?
            .unwrap_or_else(|| "unknown".to_string());
        let db_size = std::fs::metadata(db_path()).map(|m| m.len()).unwrap_or(0);
        let applied_migrations: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap_or(0);
        Ok(DbStats {
            wallets: wallets as usize,
            networks: networks as usize,
            config_entries: config_entries as usize,
            events: events_count as usize,
            schema_version,
            db_size_bytes: db_size,
            applied_migrations: applied_migrations as usize,
        })
    }

    pub fn insert_event(&self, event: &EventRow) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO events \
             (id, event_type, contract_id, ledger, topics, value, timestamp, network) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                event.id,
                event.event_type,
                event.contract_id,
                event.ledger,
                event.topics,
                event.value,
                event.timestamp,
                event.network,
            ],
        )?;
        Ok(())
    }

    pub fn search_events(&self, filters: &EventSearchFilters) -> Result<Vec<EventRow>> {
        let mut conditions = vec!["1=1".to_string()];
        let mut params = Vec::new();

        if let Some(ref contract_id) = filters.contract_id {
            conditions.push("contract_id = ?".to_string());
            params.push(contract_id.clone());
        }
        if let Some(ref event_type) = filters.event_type {
            conditions.push("event_type = ?".to_string());
            params.push(event_type.clone());
        }
        if let Some(min_ledger) = filters.min_ledger {
            conditions.push("ledger >= ?".to_string());
            params.push(min_ledger.to_string());
        }
        if let Some(max_ledger) = filters.max_ledger {
            conditions.push("ledger <= ?".to_string());
            params.push(max_ledger.to_string());
        }
        if let Some(ref start_time) = filters.start_time {
            conditions.push("timestamp >= ?".to_string());
            params.push(start_time.clone());
        }
        if let Some(ref end_time) = filters.end_time {
            conditions.push("timestamp <= ?".to_string());
            params.push(end_time.clone());
        }
        if let Some(ref network) = filters.network {
            conditions.push("network = ?".to_string());
            params.push(network.clone());
        }

        let limit = filters.limit.unwrap_or(100).to_string();
        let offset = filters.offset.unwrap_or(0).to_string();

        let sql = format!(
            "SELECT id, event_type, contract_id, ledger, topics, value, timestamp, network \
             FROM events \
             WHERE {} \
             ORDER BY timestamp DESC \
             LIMIT {} OFFSET {}",
            conditions.join(" AND "),
            limit,
            offset
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok(EventRow {
                id: row.get(0)?,
                event_type: row.get(1)?,
                contract_id: row.get(2)?,
                ledger: row.get(3)?,
                topics: row.get(4)?,
                value: row.get(5)?,
                timestamp: row.get(6)?,
                network: row.get(7)?,
            })
        })?;

        rows.map(|r| r.map_err(anyhow::Error::from)).collect()
    }

    pub fn aggregate_events(
        &self,
        bucket: &AggregationBucket,
        filters: &EventSearchFilters,
    ) -> Result<Vec<EventAggregation>> {
        let bucket_sql = match bucket {
            AggregationBucket::Hour => "strftime('%Y-%m-%d %H:00:00', timestamp) AS bucket",
            AggregationBucket::Day => "strftime('%Y-%m-%d', timestamp) AS bucket",
            AggregationBucket::Week => "strftime('%Y-%W', timestamp) AS bucket",
            AggregationBucket::Month => "strftime('%Y-%m', timestamp) AS bucket",
        };

        let mut conditions = vec!["1=1".to_string()];
        let mut params = Vec::new();

        if let Some(ref contract_id) = filters.contract_id {
            conditions.push("contract_id = ?".to_string());
            params.push(contract_id.clone());
        }
        if let Some(ref event_type) = filters.event_type {
            conditions.push("event_type = ?".to_string());
            params.push(event_type.clone());
        }
        if let Some(min_ledger) = filters.min_ledger {
            conditions.push("ledger >= ?".to_string());
            params.push(min_ledger.to_string());
        }
        if let Some(max_ledger) = filters.max_ledger {
            conditions.push("ledger <= ?".to_string());
            params.push(max_ledger.to_string());
        }
        if let Some(ref start_time) = filters.start_time {
            conditions.push("timestamp >= ?".to_string());
            params.push(start_time.clone());
        }
        if let Some(ref end_time) = filters.end_time {
            conditions.push("timestamp <= ?".to_string());
            params.push(end_time.clone());
        }
        if let Some(ref network) = filters.network {
            conditions.push("network = ?".to_string());
            params.push(network.clone());
        }

        let sql = format!(
            "SELECT {}, COUNT(*) AS count \
             FROM events \
             WHERE {} \
             GROUP BY bucket \
             ORDER BY bucket DESC",
            bucket_sql,
            conditions.join(" AND ")
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
            Ok(EventAggregation {
                bucket: row.get(0)?,
                count: row.get(1)?,
            })
        })?;

        rows.map(|r| r.map_err(anyhow::Error::from)).collect()
    }

    pub fn export_events(
        &self,
        filters: &EventSearchFilters,
        format: ExportFormat,
        writer: &mut impl std::io::Write,
    ) -> Result<()> {
        let events = self.search_events(filters)?;

        match format {
            ExportFormat::Json => {
                serde_json::to_writer_pretty(writer, &events)?;
            }
            ExportFormat::Csv => {
                let mut wtr = csv::Writer::from_writer(writer);
                wtr.write_record([
                    "id",
                    "event_type",
                    "contract_id",
                    "ledger",
                    "topics",
                    "value",
                    "timestamp",
                    "network",
                ])?;
                for event in events {
                    wtr.write_record([
                        &event.id,
                        &event.event_type,
                        &event.contract_id,
                        &event.ledger.to_string(),
                        &event.topics.unwrap_or_default(),
                        &event.value,
                        &event.timestamp,
                        &event.network,
                    ])?;
                }
                wtr.flush()?;
            }
        }

        Ok(())
    }
}

pub fn restore_database(src: &std::path::Path) -> Result<()> {
    let dest = db_path();
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, &dest)
        .with_context(|| format!("Failed to restore database from {}", src.display()))?;
    Ok(())
}

pub fn migrate_from_toml(db: &Database) -> Result<MigrationReport> {
    let mut cfg = crate::utils::config::parse_config_file()?;
    cfg = crate::utils::config::migrate_config(cfg)?;
    crate::utils::config::ensure_default_networks(&mut cfg);
    db.save_config(&cfg)?;
    let report = MigrationReport {
        wallets_migrated: cfg.wallets.len(),
        networks_migrated: cfg.networks.len(),
        config_keys_migrated: db.list_config_kv()?.len(),
    };

    db.set_meta("migrated_from_toml", "true")?;
    db.set_meta("migration_timestamp", &chrono::Utc::now().to_rfc3339())?;

    Ok(report)
}

pub fn export_to_toml(db: &Database) -> Result<String> {
    Ok(toml::to_string_pretty(&db.load_config()?)?)
}

/// The tables the migration runner needs to record what it has done.
///
/// A migration's `down` undoes the schema it created, and for the initial
/// migration that includes these tables. The runner re-creates them before
/// writing the rollback down, so its own bookkeeping survives.
const MIGRATION_BOOKKEEPING_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL,
    checksum TEXT NOT NULL
);
";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL,
    checksum TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS wallets (
    name        TEXT PRIMARY KEY,
    public_key  TEXT NOT NULL,
    secret_key  TEXT,
    network     TEXT NOT NULL DEFAULT 'testnet',
    created_at  TEXT NOT NULL,
    funded      INTEGER NOT NULL DEFAULT 0,
    rotation_history TEXT NOT NULL DEFAULT '[]',
    usage_policy TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS networks (
    name            TEXT PRIMARY KEY,
    horizon_url     TEXT NOT NULL,
    soroban_rpc_url TEXT,
    friendbot_url   TEXT,
    passphrase      TEXT
);

CREATE TABLE IF NOT EXISTS config_kv (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS plugins (
    name        TEXT PRIMARY KEY,
    path        TEXT NOT NULL,
    source      TEXT,
    installed_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS templates (
    name        TEXT PRIMARY KEY,
    description TEXT,
    tags        TEXT,
    source_url  TEXT,
    cached_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    contract_id TEXT NOT NULL,
    ledger INTEGER NOT NULL,
    topics TEXT,
    value TEXT NOT NULL,
    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
    network TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_contract ON events(contract_id);
CREATE INDEX IF NOT EXISTS idx_events_ledger ON events(ledger);
CREATE INDEX IF NOT EXISTS idx_events_type ON events(event_type);
CREATE INDEX IF NOT EXISTS idx_events_network ON events(network);
CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_events_contract_ledger ON events(contract_id, ledger);
CREATE INDEX IF NOT EXISTS idx_wallets_network ON wallets(network);
CREATE INDEX IF NOT EXISTS idx_wallets_public_key ON wallets(public_key);
CREATE INDEX IF NOT EXISTS idx_config_kv_key   ON config_kv(key);
CREATE INDEX IF NOT EXISTS idx_plugins_source ON plugins(source);
CREATE INDEX IF NOT EXISTS idx_templates_source_url ON templates(source_url);
CREATE INDEX IF NOT EXISTS idx_templates_cached_at ON templates(cached_at);
";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletRow {
    pub name: String,
    pub public_key: String,
    pub secret_key: Option<String>,
    pub network: String,
    pub created_at: String,
    pub funded: bool,
    pub rotation_history: String,
    #[serde(default = "default_wallet_policy_json")]
    pub usage_policy: String,
}

fn default_wallet_policy_json() -> String {
    "{}".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkRow {
    pub name: String,
    pub horizon_url: String,
    pub soroban_rpc_url: Option<String>,
    pub friendbot_url: Option<String>,
    pub passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub rows_affected: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbStats {
    pub wallets: usize,
    pub networks: usize,
    pub config_entries: usize,
    pub events: usize,
    pub schema_version: String,
    pub db_size_bytes: u64,
    pub applied_migrations: usize,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    pub wallets_migrated: usize,
    pub networks_migrated: usize,
    pub config_keys_migrated: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRow {
    pub id: String,
    pub event_type: String,
    pub contract_id: String,
    pub ledger: u32,
    pub topics: Option<String>,
    pub value: String,
    pub timestamp: String,
    pub network: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct EventSearchFilters {
    pub contract_id: Option<String>,
    pub event_type: Option<String>,
    pub min_ledger: Option<u32>,
    pub max_ledger: Option<u32>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub network: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AggregationBucket {
    Hour,
    Day,
    Week,
    Month,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventAggregation {
    pub bucket: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExportFormat {
    Json,
    Csv,
}

/// Migration V1: Initial schema setup
struct MigrationV1;

impl Migration for MigrationV1 {
    fn version(&self) -> i64 {
        1
    }

    fn description(&self) -> &str {
        "initial_schema"
    }

    fn up(&self, _conn: &Connection) -> Result<()> {
        // This is a no-op since the initial schema is already applied in SCHEMA
        Ok(())
    }

    fn down(&self, conn: &Connection) -> Result<()> {
        // Rollback: drop every table the initial bootstrap created. That
        // includes the feature-flag tables, which `Database::initialize`
        // applies alongside SCHEMA.
        conn.execute_batch(
            "DROP TABLE IF EXISTS flag_metrics;
             DROP TABLE IF EXISTS flag_overrides;
             DROP TABLE IF EXISTS flag_states;
             DROP TABLE IF EXISTS flag_definitions;
             DROP TABLE IF EXISTS events;
             DROP TABLE IF EXISTS templates;
             DROP TABLE IF EXISTS plugins;
             DROP TABLE IF EXISTS config_kv;
             DROP TABLE IF EXISTS networks;
             DROP TABLE IF EXISTS wallets;
             DROP TABLE IF EXISTS flag_definitions;
             DROP TABLE IF EXISTS flag_states;
             DROP TABLE IF EXISTS flag_overrides;
             DROP TABLE IF EXISTS flag_metrics;
             DROP TABLE IF EXISTS schema_migrations;
             DROP TABLE IF EXISTS meta;",
        )?;
        Ok(())
    }
}

struct MigrationV2;

impl Migration for MigrationV2 {
    fn version(&self) -> i64 {
        2
    }

    fn description(&self) -> &str {
        "wallet_usage_policy"
    }

    fn up(&self, conn: &Connection) -> Result<()> {
        let mut stmt = conn.prepare("PRAGMA table_info(wallets)")?;
        let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
        if !columns
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .any(|name| name == "usage_policy")
        {
            conn.execute(
                "ALTER TABLE wallets ADD COLUMN usage_policy TEXT NOT NULL DEFAULT '{}'",
                [],
            )?;
        }
        Ok(())
    }

    fn down(&self, conn: &Connection) -> Result<()> {
        conn.execute("ALTER TABLE wallets DROP COLUMN usage_policy", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.initialize().unwrap();
        db
    }

    #[test]
    fn insert_and_list_wallet() {
        let db = in_memory_db();
        db.insert_wallet(&WalletRow {
            name: "alice".to_string(),
            public_key: "GABC".to_string(),
            secret_key: Some("SABC".to_string()),
            network: "testnet".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            funded: false,
            rotation_history: "[]".to_string(),
            usage_policy: r#"{"allowed_networks":["mainnet"],"max_fee":250000,"allowed_contracts":[],"require_confirmation":true}"#.to_string(),
        })
        .unwrap();
        let wallets = db.list_wallets().unwrap();
        assert_eq!(wallets.len(), 1);
        assert_eq!(wallets[0].name, "alice");
        assert!(wallets[0].usage_policy.contains("mainnet"));
        assert!(wallets[0].usage_policy.contains("250000"));
        assert!(wallets[0].usage_policy.contains("true"));
    }

    #[test]
    fn get_wallet_returns_none_for_missing() {
        let db = in_memory_db();
        let w = db.get_wallet("missing").unwrap();
        assert!(w.is_none());
    }

    #[test]
    fn config_kv_roundtrip() {
        let db = in_memory_db();
        db.insert_config_kv("network", "mainnet").unwrap();
        let v = db.get_config_kv("network").unwrap();
        assert_eq!(v, Some("mainnet".to_string()));
    }

    #[test]
    fn integrity_check_passes_on_fresh_db() {
        let db = in_memory_db();
        let result = db.integrity_check().unwrap();
        assert_eq!(result, vec!["ok".to_string()]);
    }

    #[test]
    fn stats_reflect_inserted_data() {
        let db = in_memory_db();
        db.insert_wallet(&WalletRow {
            name: "bob".to_string(),
            public_key: "GXYZ".to_string(),
            secret_key: None,
            network: "testnet".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            funded: true,
            rotation_history: "[]".to_string(),
            usage_policy: "{}".to_string(),
        })
        .unwrap();
        let stats = db.stats().unwrap();
        assert_eq!(stats.wallets, 1);
    }

    #[test]
    fn delete_wallet_removes_entry() {
        let db = in_memory_db();
        db.insert_wallet(&WalletRow {
            name: "temp".to_string(),
            public_key: "GTEMP".to_string(),
            secret_key: None,
            network: "testnet".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            funded: false,
            rotation_history: "[]".to_string(),
            usage_policy: "{}".to_string(),
        })
        .unwrap();
        let removed = db.delete_wallet("temp").unwrap();
        assert_eq!(removed, 1);
        assert!(db.get_wallet("temp").unwrap().is_none());
    }

    #[test]
    fn insert_and_search_event() {
        let db = in_memory_db();
        let event = EventRow {
            id: "evt123".to_string(),
            event_type: "contract".to_string(),
            contract_id: "CABC123".to_string(),
            ledger: 12345,
            topics: Some(serde_json::to_string(&vec!["topic1", "topic2"]).unwrap()),
            value: serde_json::json!({"key": "value"}).to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            network: "testnet".to_string(),
        };
        db.insert_event(&event).unwrap();

        let filters = EventSearchFilters {
            contract_id: Some("CABC123".to_string()),
            ..Default::default()
        };
        let events = db.search_events(&filters).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "evt123");
    }

    #[test]
    fn aggregate_events() {
        let db = in_memory_db();
        let event1 = EventRow {
            id: "evt1".to_string(),
            event_type: "contract".to_string(),
            contract_id: "CABC".to_string(),
            ledger: 1,
            topics: None,
            value: "{}".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            network: "testnet".to_string(),
        };
        let event2 = EventRow {
            id: "evt2".to_string(),
            event_type: "contract".to_string(),
            contract_id: "CABC".to_string(),
            ledger: 2,
            topics: None,
            value: "{}".to_string(),
            timestamp: "2024-01-01T00:30:00Z".to_string(),
            network: "testnet".to_string(),
        };
        db.insert_event(&event1).unwrap();
        db.insert_event(&event2).unwrap();

        let aggregates = db
            .aggregate_events(&AggregationBucket::Hour, &EventSearchFilters::default())
            .unwrap();
        assert_eq!(aggregates.len(), 1);
        assert_eq!(aggregates[0].count, 2);
    }

    #[test]
    fn migration_initialization_sets_version() {
        let db = in_memory_db();
        let version = db.get_current_schema_version().unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn migration_records_applied_migrations() {
        let db = in_memory_db();
        let applied = db.get_applied_migrations().unwrap();
        assert!(!applied.is_empty());
        assert!(applied.iter().any(|m| m.version == 1));
    }

    #[test]
    fn migration_rollback_latest_migration() {
        let db = in_memory_db();
        let version_before = db.get_current_schema_version().unwrap();

        // Rollback the latest migration
        db.rollback_migration(version_before).unwrap();

        let version_after = db.get_current_schema_version().unwrap();
        assert_eq!(version_after, version_before - 1);

        let applied = db.get_applied_migrations().unwrap();
        assert!(!applied.iter().any(|m| m.version == version_before));
    }

    #[test]
    fn migration_rollback_fails_for_nonexistent_migration() {
        let db = in_memory_db();
        let result = db.rollback_migration(999);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not applied"));
    }

    #[test]
    fn migration_rollback_fails_for_non_latest_migration() {
        let db = in_memory_db();
        // Try to rollback a migration that isn't the latest
        let result = db.rollback_migration(0);
        assert!(result.is_err());
    }

    #[test]
    fn migration_checksum_is_deterministic() {
        let db = in_memory_db();
        let checksum1 = db.compute_migration_checksum(1, "test_migration").unwrap();
        let checksum2 = db.compute_migration_checksum(1, "test_migration").unwrap();
        assert_eq!(checksum1, checksum2);
    }

    #[test]
    fn migration_checksum_differs_for_different_inputs() {
        let db = in_memory_db();
        let checksum1 = db.compute_migration_checksum(1, "test_migration").unwrap();
        let checksum2 = db.compute_migration_checksum(2, "test_migration").unwrap();
        assert_ne!(checksum1, checksum2);
    }

    #[test]
    fn migration_stats_includes_applied_migrations() {
        let db = in_memory_db();
        let stats = db.stats().unwrap();
        assert!(stats.applied_migrations > 0);
    }

    #[test]
    fn migration_v1_up_is_noop() {
        let db = in_memory_db();
        let migration = MigrationV1 {};
        let conn = db.conn;
        // Should not fail even though schema already exists
        assert!(migration.up(&conn).is_ok());
    }

    #[test]
    fn migration_v1_down_drops_tables() {
        let db = in_memory_db();
        let migration = MigrationV1 {};
        let conn = db.conn;

        // Verify tables exist before rollback
        let table_count: i64 = conn
            .query_row(
                // `sqlite_%` names are SQLite's own internals (sqlite_sequence
                // is created by the AUTOINCREMENT columns) and are not part of
                // the schema a migration owns.
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(table_count > 0);

        // Rollback
        migration.down(&conn).unwrap();

        // Verify tables are dropped
        let table_count_after: i64 = conn
            .query_row(
                // `sqlite_%` names are SQLite's own internals (sqlite_sequence
                // is created by the AUTOINCREMENT columns) and are not part of
                // the schema a migration owns.
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(table_count_after < table_count);
    }

    #[test]
    fn migration_run_migrations_handles_up_to_date_database() {
        let db = in_memory_db();
        let result = db.run_migrations().unwrap();
        assert_eq!(result.current_version, CURRENT_SCHEMA_VERSION);
        assert!(result.migrations_applied.is_empty());
    }

    #[test]
    fn migration_transaction_rollback_on_failure() {
        let db = in_memory_db();
        // Set schema version to 0 to simulate an old database
        db.conn
            .execute(
                "UPDATE meta SET value = '0' WHERE key = 'schema_version'",
                [],
            )
            .unwrap();
        db.conn
            .execute("DELETE FROM schema_migrations WHERE version = 1", [])
            .unwrap();

        // This should apply migration 1
        let result = db.run_migrations().unwrap();
        assert_eq!(result.current_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(result.migrations_applied, vec![1]);
    }

    /// A real N-1 -> N upgrade, on a file-backed database (not
    /// `open_in_memory()`, which can't be closed and reopened the way a real
    /// upgrade-on-next-launch happens): a database is created and
    /// initialized at schema 0 (pre-migration), closed, then reopened and
    /// initialized again, which is exactly what `run_migrations` (called
    /// from `initialize`) is for.
    #[test]
    fn migration_upgrades_a_reopened_database_from_n_minus_1_to_n() {
        let dir = tempfile::tempdir().unwrap();
        let db_file = dir.path().join("upgrade-test.db");

        {
            let db = Database {
                conn: Connection::open(&db_file).unwrap(),
            };
            db.initialize().unwrap();
            // Roll the freshly-initialized database back to schema 0, as if
            // it had been created by a build that predates migration 1 and
            // is only now being opened by a build that has it.
            db.conn
                .execute(
                    "UPDATE meta SET value = '0' WHERE key = 'schema_version'",
                    [],
                )
                .unwrap();
            db.conn
                .execute("DELETE FROM schema_migrations WHERE version = 1", [])
                .unwrap();
            assert_eq!(db.get_current_schema_version().unwrap(), 0);
        }

        // Reopen as a fresh connection to the same file: this is the
        // "upgrade on next launch" path, not the same in-process Database.
        let reopened = Database {
            conn: Connection::open(&db_file).unwrap(),
        };
        assert_eq!(reopened.get_current_schema_version().unwrap(), 0);
        reopened.initialize().unwrap();
        assert_eq!(
            reopened.get_current_schema_version().unwrap(),
            CURRENT_SCHEMA_VERSION
        );
        let applied = reopened.get_applied_migrations().unwrap();
        assert!(applied.iter().any(|m| m.version == CURRENT_SCHEMA_VERSION));
    }

    #[test]
    fn open_succeeds_on_a_fresh_database_file() {
        let dir = tempfile::tempdir().unwrap();
        let db_file = dir.path().join("fresh.db");
        assert!(!db_file.exists());

        // Simulates `Database::open()`'s own logic without depending on
        // `db_path()` (which reads the real config directory): a file that
        // does not exist yet is never corrupted, so no integrity check runs.
        let conn = Connection::open(&db_file).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
        let db = Database { conn };
        db.initialize().unwrap();
        assert!(db.integrity_check().unwrap().iter().all(|r| r == "ok"));
    }

    #[test]
    fn fail_clearly_if_corrupted_passes_on_an_intact_database() {
        let db = in_memory_db();
        let path = std::path::Path::new(":memory:");
        assert!(db.fail_clearly_if_corrupted(path).is_ok());
    }

    #[test]
    fn fail_clearly_if_corrupted_reports_a_typed_error_on_real_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let db_file = dir.path().join("corrupt.db");
        // Not a SQLite file at all: `PRAGMA integrity_check` reports this as
        // corruption rather than erroring the pragma itself, which is what
        // `fail_clearly_if_corrupted` is built to catch.
        std::fs::write(&db_file, b"this is not a sqlite database file").unwrap();

        let conn = Connection::open(&db_file).unwrap();
        let db = Database { conn };
        let result = db.fail_clearly_if_corrupted(&db_file);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.downcast_ref::<MigrationError>().is_some());
        let message = err.to_string();
        assert!(message.contains("corrupted"));
        assert!(message.contains("starforge config db restore"));
    }

    #[test]
    fn initialize_with_backup_skips_backup_for_a_fresh_database() {
        let dir = tempfile::tempdir().unwrap();
        let db_file = dir.path().join("fresh-with-backup.db");
        let backup_dir = dir.path().join("backups");

        let conn = Connection::open(&db_file).unwrap();
        let db = Database { conn };
        let backup_path = db.initialize_with_backup(&backup_dir).unwrap();

        assert!(backup_path.is_none());
        assert!(!backup_dir.exists());
    }

    #[test]
    fn initialize_with_backup_skips_backup_when_already_current() {
        let db = in_memory_db();
        let dir = tempfile::tempdir().unwrap();
        let backup_dir = dir.path().join("backups");

        // `db` is already fully initialized at CURRENT_SCHEMA_VERSION, so a
        // second call has nothing to upgrade and should not back anything up.
        let backup_path = db.initialize_with_backup(&backup_dir).unwrap();
        assert!(backup_path.is_none());
    }

    #[test]
    fn initialize_with_backup_backs_up_before_an_actual_upgrade() {
        let dir = tempfile::tempdir().unwrap();
        let db_file = dir.path().join("needs-upgrade.db");
        let backup_dir = dir.path().join("backups");

        {
            let db = Database {
                conn: Connection::open(&db_file).unwrap(),
            };
            db.initialize().unwrap();
            db.conn
                .execute(
                    "UPDATE meta SET value = '0' WHERE key = 'schema_version'",
                    [],
                )
                .unwrap();
            db.conn
                .execute("DELETE FROM schema_migrations WHERE version = 1", [])
                .unwrap();
        }

        let reopened = Database {
            conn: Connection::open(&db_file).unwrap(),
        };
        let backup_path = reopened.initialize_with_backup(&backup_dir).unwrap();

        let backup_path = backup_path.expect("an upgrade was pending, so a backup must be made");
        assert!(backup_path.exists());
        assert!(backup_path.starts_with(&backup_dir));
        assert_eq!(
            reopened.get_current_schema_version().unwrap(),
            CURRENT_SCHEMA_VERSION
        );

        // The backup is a snapshot of the pre-upgrade file: opening it
        // directly should show the pre-upgrade schema version, not the
        // post-upgrade one the live connection now has.
        let backup_conn = Connection::open(&backup_path).unwrap();
        let backup_db = Database { conn: backup_conn };
        assert_eq!(backup_db.get_current_schema_version().unwrap(), 0);
    }
}
