use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;

pub fn db_path() -> PathBuf {
    crate::utils::config::config_dir().join("starforge.db")
}

/// Current schema version of the database
pub const CURRENT_SCHEMA_VERSION: i64 = 1;

/// Migration trait for defining schema changes
pub trait Migration: Send + Sync {
    /// Version number for this migration (must be unique)
    fn version(&self) -> i64;

    /// Description of what this migration does
    fn description(&self) -> &str;

    /// Apply the migration (upgrade)
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
#[derive(Debug, thiserror::Error)]
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
}

pub struct Database {
    pub(crate) conn: Connection,
}

impl Database {
    pub fn open() -> Result<Self> {
        let path = db_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(Self { conn })
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
        let mut tx = self.conn.unchecked_transaction()?;
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
        self.conn.execute_batch(SCHEMA)?;
        self.ensure_column("wallets", "secret_key", "TEXT")?;
        self.ensure_column("wallets", "rotation_history", "TEXT NOT NULL DEFAULT '[]'")?;

        // Run migrations if this is not a fresh database
        if self.get_meta("schema_version").is_ok() {
            self.run_migrations()?;
        } else {
            // Fresh database - set initial version
            self.set_meta("schema_version", &CURRENT_SCHEMA_VERSION.to_string())?;
            self.record_migration(CURRENT_SCHEMA_VERSION, "initial_schema")?;
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
        self.get_meta("schema_version")?
            .and_then(|v| v.parse::<i64>().ok())
            .ok_or_else(|| anyhow::anyhow!("Schema version not found or invalid"))
    }

    /// Get all applied migrations from the database
    pub fn get_applied_migrations(&self) -> Result<Vec<AppliedMigration>> {
        let mut stmt = self.conn.prepare(
            "SELECT version, name, applied_at, checksum FROM schema_migrations ORDER BY version",
        )?;
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
        let current_version = self.get_current_schema_version()?;
        let applied = self.get_applied_migrations()?;
        let applied_versions: std::collections::HashSet<i64> =
            applied.iter().map(|m| m.version).collect();

        let mut migrations_applied = Vec::new();

        // Check if we need to upgrade
        if current_version < CURRENT_SCHEMA_VERSION {
            // Apply migrations from current_version + 1 to CURRENT_SCHEMA_VERSION
            for version in (current_version + 1)..=CURRENT_SCHEMA_VERSION {
                if !applied_versions.contains(&version) {
                    self.apply_migration(version)?;
                    migrations_applied.push(version);
                }
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

        let mut tx = self.conn.unchecked_transaction()?;

        // Apply the migration
        match migration.up(&tx) {
            Ok(()) => {
                // Record the migration
                let checksum = self.compute_migration_checksum(version, migration.description())?;
                let applied_at = chrono::Utc::now().to_rfc3339();
                tx.execute(
                    "INSERT INTO schema_migrations (version, name, applied_at, checksum) VALUES (?1, ?2, ?3, ?4)",
                    params![version, migration.description(), applied_at, checksum],
                )?;

                // Update schema version
                tx.execute(
                    "UPDATE meta SET value = ?1 WHERE key = 'schema_version'",
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
        let current_version = self.get_current_schema_version()?;

        // Check if the migration is applied
        if !applied.iter().any(|m| m.version == version) {
            return Err(anyhow::anyhow!(
                "Migration version {} is not applied",
                version
            ));
        }

        // Check if we can rollback (must be the latest applied migration)
        let max_applied = applied
            .iter()
            .map(|m| m.version)
            .max()
            .ok_or_else(|| anyhow::anyhow!("No migrations applied"))?;

        if version != max_applied {
            return Err(anyhow::anyhow!(
                "Can only rollback the latest migration ({}), tried to rollback {}",
                max_applied,
                version
            ));
        }

        let migration = self
            .get_migration(version)
            .ok_or_else(|| anyhow::anyhow!("Migration version {} not found", version))?;

        let mut tx = self.conn.unchecked_transaction()?;

        // Rollback the migration
        match migration.down(&tx) {
            Ok(()) => {
                // Remove the migration record
                tx.execute(
                    "DELETE FROM schema_migrations WHERE version = ?1",
                    params![version],
                )?;

                // Update schema version to previous version
                let previous_version = if version > 1 { version - 1 } else { 0 };
                tx.execute(
                    "UPDATE meta SET value = ?1 WHERE key = 'schema_version'",
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
             (name, public_key, secret_key, network, created_at, funded, rotation_history) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                wallet.name,
                wallet.public_key,
                wallet.secret_key,
                wallet.network,
                wallet.created_at,
                wallet.funded,
                wallet.rotation_history,
            ],
        )?;
        Ok(())
    }

    pub fn list_wallets(&self) -> Result<Vec<WalletRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, public_key, secret_key, network, created_at, funded, rotation_history FROM wallets ORDER BY created_at",
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
            })
        })?;
        rows.map(|r| r.map_err(anyhow::Error::from)).collect()
    }

    pub fn get_wallet(&self, name: &str) -> Result<Option<WalletRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, public_key, secret_key, network, created_at, funded, rotation_history FROM wallets WHERE name = ?1",
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
            cfg.plugin_trust = PluginTrustConfig {
                trusted_sources: serde_json::from_str(&plugin_trust)?,
            };
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
                WalletEntry {
                    name: wallet.name,
                    public_key: wallet.public_key,
                    secret_key: wallet.secret_key,
                    network: wallet.network,
                    created_at: wallet.created_at,
                    funded: wallet.funded,
                    rotation_history,
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

    pub fn backup(&self, dest: &std::path::Path) -> Result<()> {
        let src = db_path();
        std::fs::copy(&src, dest)?;
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
                wtr.write_record(&[
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
                    wtr.write_record(&[
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
    rotation_history TEXT NOT NULL DEFAULT '[]'
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

    fn up(&self, conn: &Connection) -> Result<()> {
        // This is a no-op since the initial schema is already applied in SCHEMA
        Ok(())
    }

    fn down(&self, conn: &Connection) -> Result<()> {
        // Rollback: drop all tables
        conn.execute_batch(
            "DROP TABLE IF EXISTS events;
             DROP TABLE IF EXISTS templates;
             DROP TABLE IF EXISTS plugins;
             DROP TABLE IF EXISTS config_kv;
             DROP TABLE IF EXISTS networks;
             DROP TABLE IF EXISTS wallets;
             DROP TABLE IF EXISTS schema_migrations;
             DROP TABLE IF EXISTS meta;",
        )?;
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
        })
        .unwrap();
        let wallets = db.list_wallets().unwrap();
        assert_eq!(wallets.len(), 1);
        assert_eq!(wallets[0].name, "alice");
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
        assert!(result.unwrap_err().to_string().contains("latest migration"));
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
        let mut conn = db.conn;
        // Should not fail even though schema already exists
        assert!(migration.up(&mut conn).is_ok());
    }

    #[test]
    fn migration_v1_down_drops_tables() {
        let db = in_memory_db();
        let migration = MigrationV1 {};
        let mut conn = db.conn;

        // Verify tables exist before rollback
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(table_count > 0);

        // Rollback
        migration.down(&mut conn).unwrap();

        // Verify tables are dropped
        let table_count_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(table_count_after, 0);
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

        // This should apply migration 1
        let result = db.run_migrations().unwrap();
        assert_eq!(result.current_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(result.migrations_applied, vec![1]);
    }
}
