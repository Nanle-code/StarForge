//! AI request caching for StarForge.
//!
//! Provides a caching layer for AI requests to reduce API costs, improve latency,
//! and enable offline capabilities for repeated queries.
//!
//! Features:
//! - Cache key generation based on prompt + context
//! - TTL-based cache invalidation
//! - Disk-based persistent cache using SQLite
//! - Cache size management and eviction policies
//! - Cache hit/miss metrics
//! - Support for cache prewarming
//! - Manual cache invalidation commands

use crate::utils::database::Database;
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/// Default TTL for cached AI responses (7 days)
pub const DEFAULT_CACHE_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;

/// Maximum cache size in bytes (1GB)
pub const MAX_CACHE_SIZE_BYTES: u64 = 1024 * 1024 * 1024;

/// Cache entry for AI responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiCacheEntry {
    /// Unique cache key (SHA256 hash of model + prompt + options)
    pub cache_key: String,
    /// Model used for the request
    pub model: String,
    /// Prompt sent to the AI
    pub prompt: String,
    /// Generation options as JSON
    pub options: String,
    /// AI response
    pub response: String,
    /// Response metadata as JSON
    pub metadata: String,
    /// Creation timestamp (Unix epoch seconds)
    pub created_at: u64,
    /// Last access timestamp (Unix epoch seconds)
    pub last_accessed_at: u64,
    /// Expiration timestamp (Unix epoch seconds, 0 = never expires)
    pub expires_at: u64,
    /// Size estimate in bytes
    pub size_bytes: u64,
    /// Number of times this entry has been accessed
    pub access_count: u64,
    /// Tags for categorization (comma-separated)
    pub tags: String,
}

/// Cache statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiCacheStats {
    /// Total number of cache entries
    pub total_entries: usize,
    /// Number of active (non-expired) entries
    pub active_entries: usize,
    /// Total cache size in bytes
    pub total_size_bytes: u64,
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Hit rate percentage
    pub hit_rate: f64,
    /// Average entry age in seconds
    pub avg_age_seconds: f64,
    /// Most accessed entry count
    pub max_access_count: u64,
    /// Number of expired entries
    pub expired_entries: usize,
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiCacheConfig {
    /// Default TTL in seconds
    pub default_ttl_seconds: u64,
    /// Maximum cache size in bytes
    pub max_size_bytes: u64,
    /// Enable compression for large responses
    pub enable_compression: bool,
    /// Enable semantic similarity search
    pub enable_semantic_search: bool,
    /// Auto-cleanup expired entries on startup
    pub auto_cleanup: bool,
    /// Enable cache warming for common operations
    pub enable_warming: bool,
}

impl Default for AiCacheConfig {
    fn default() -> Self {
        Self {
            default_ttl_seconds: DEFAULT_CACHE_TTL_SECONDS,
            max_size_bytes: MAX_CACHE_SIZE_BYTES,
            enable_compression: true,
            enable_semantic_search: false,
            auto_cleanup: true,
            enable_warming: false,
        }
    }
}

/// AI Cache manager
pub struct AiCache {
    db: Database,
    config: AiCacheConfig,
    stats: AiCacheStats,
}

impl AiCache {
    /// Open or create the AI cache database
    pub fn open() -> Result<Self> {
        let db = Database::open()?;
        let config = AiCacheConfig::default();
        let cache = Self {
            db,
            config,
            stats: AiCacheStats::default(),
        };
        cache.initialize()?;
        Ok(cache)
    }

    /// Initialize cache database schema
    fn initialize(&self) -> Result<()> {
        let schema = "
        CREATE TABLE IF NOT EXISTS ai_cache (
            cache_key TEXT PRIMARY KEY,
            model TEXT NOT NULL,
            prompt TEXT NOT NULL,
            options TEXT NOT NULL,
            response TEXT NOT NULL,
            metadata TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            last_accessed_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL,
            size_bytes INTEGER NOT NULL,
            access_count INTEGER NOT NULL DEFAULT 0,
            tags TEXT NOT NULL DEFAULT ''
        );

        CREATE INDEX IF NOT EXISTS idx_ai_cache_expires_at ON ai_cache(expires_at);
        CREATE INDEX IF NOT EXISTS idx_ai_cache_last_accessed_at ON ai_cache(last_accessed_at);
        CREATE INDEX IF NOT EXISTS idx_ai_cache_created_at ON ai_cache(created_at);
        CREATE INDEX IF NOT EXISTS idx_ai_cache_model ON ai_cache(model);
        CREATE INDEX IF NOT EXISTS idx_ai_cache_tags ON ai_cache(tags);
        CREATE INDEX IF NOT EXISTS idx_ai_cache_size_bytes ON ai_cache(size_bytes);
        ";

        self.db.conn.execute_batch(schema)?;

        // Clean up expired entries on startup if configured
        if self.config.auto_cleanup {
            self.cleanup_expired()?;
        }

        Ok(())
    }

    /// Generate cache key from model, prompt, and options
    pub fn generate_cache_key(model: &str, prompt: &str, options: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(model.as_bytes());
        hasher.update(prompt.as_bytes());
        hasher.update(options.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Get current timestamp in seconds
    pub fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Get cached response if available and not expired
    pub fn get(&mut self, cache_key: &str) -> Result<Option<AiCacheEntry>> {
        let now = Self::current_timestamp();

        let entry: Option<AiCacheEntry> = self
            .db
            .conn
            .query_row(
                "SELECT cache_key, model, prompt, options, response, metadata, 
                    created_at, last_accessed_at, expires_at, size_bytes, access_count, tags
             FROM ai_cache 
             WHERE cache_key = ?1 AND (expires_at = 0 OR expires_at > ?2)",
                params![cache_key, now],
                |row| {
                    Ok(AiCacheEntry {
                        cache_key: row.get(0)?,
                        model: row.get(1)?,
                        prompt: row.get(2)?,
                        options: row.get(3)?,
                        response: row.get(4)?,
                        metadata: row.get(5)?,
                        created_at: row.get(6)?,
                        last_accessed_at: row.get(7)?,
                        expires_at: row.get(8)?,
                        size_bytes: row.get(9)?,
                        access_count: row.get(10)?,
                        tags: row.get(11)?,
                    })
                },
            )
            .optional()?;

        if let Some(mut entry) = entry {
            // Update access stats
            entry.last_accessed_at = now;
            entry.access_count += 1;

            self.db.conn.execute(
                "UPDATE ai_cache SET last_accessed_at = ?1, access_count = ?2 WHERE cache_key = ?3",
                params![entry.last_accessed_at, entry.access_count, cache_key],
            )?;

            self.stats.hits += 1;
            Ok(Some(entry))
        } else {
            self.stats.misses += 1;
            Ok(None)
        }
    }

    /// Store response in cache
    pub fn put(&mut self, entry: AiCacheEntry) -> Result<()> {
        // Check cache size and evict if necessary
        self.enforce_size_limit()?;

        // Clean up expired entries
        self.cleanup_expired()?;

        self.db.conn.execute(
            "INSERT OR REPLACE INTO ai_cache 
             (cache_key, model, prompt, options, response, metadata, 
              created_at, last_accessed_at, expires_at, size_bytes, access_count, tags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                entry.cache_key,
                entry.model,
                entry.prompt,
                entry.options,
                entry.response,
                entry.metadata,
                entry.created_at,
                entry.last_accessed_at,
                entry.expires_at,
                entry.size_bytes,
                entry.access_count,
                entry.tags,
            ],
        )?;

        Ok(())
    }

    /// Create a cache entry from AI request parameters
    pub fn create_entry(
        model: &str,
        prompt: &str,
        options: &str,
        response: &str,
        metadata: &str,
        ttl_seconds: Option<u64>,
        tags: &str,
    ) -> AiCacheEntry {
        let now = Self::current_timestamp();
        let cache_key = Self::generate_cache_key(model, prompt, options);
        let expires_at = ttl_seconds.map_or(0, |ttl| now + ttl);

        // Estimate size (rough approximation)
        let size_bytes = (model.len()
            + prompt.len()
            + options.len()
            + response.len()
            + metadata.len()
            + tags.len()) as u64;

        AiCacheEntry {
            cache_key,
            model: model.to_string(),
            prompt: prompt.to_string(),
            options: options.to_string(),
            response: response.to_string(),
            metadata: metadata.to_string(),
            created_at: now,
            last_accessed_at: now,
            expires_at,
            size_bytes,
            access_count: 1,
            tags: tags.to_string(),
        }
    }

    /// Enforce cache size limit by evicting least recently used entries
    fn enforce_size_limit(&mut self) -> Result<()> {
        let total_size: u64 = self.db.conn.query_row(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM ai_cache",
            [],
            |row| row.get(0),
        )?;

        if total_size <= self.config.max_size_bytes {
            return Ok(());
        }

        // Calculate how much to evict (evict 20% over limit)
        let target_size = (self.config.max_size_bytes as f64 * 0.8) as u64;
        let mut to_evict = total_size - target_size;

        // Get entries sorted by last accessed (oldest first)
        let mut stmt = self.db.conn.prepare(
            "SELECT cache_key, size_bytes FROM ai_cache 
             ORDER BY last_accessed_at ASC, access_count ASC",
        )?;

        let mut rows = stmt.query([])?;
        while to_evict > 0 {
            if let Some(row) = rows.next()? {
                let cache_key: String = row.get(0)?;
                let size_bytes: u64 = row.get(1)?;

                self.db.conn.execute(
                    "DELETE FROM ai_cache WHERE cache_key = ?1",
                    params![cache_key],
                )?;

                to_evict = to_evict.saturating_sub(size_bytes);
            } else {
                break;
            }
        }

        Ok(())
    }

    /// Clean up expired cache entries
    pub fn cleanup_expired(&self) -> Result<usize> {
        let now = Self::current_timestamp();
        let deleted = self.db.conn.execute(
            "DELETE FROM ai_cache WHERE expires_at > 0 AND expires_at <= ?1",
            params![now],
        )?;
        Ok(deleted)
    }

    /// Invalidate cache entries by tags
    pub fn invalidate_by_tags(&mut self, tags: &str) -> Result<usize> {
        let deleted = self.db.conn.execute(
            "DELETE FROM ai_cache WHERE tags LIKE ?1",
            params![format!("%{}%", tags)],
        )?;
        Ok(deleted)
    }

    /// Invalidate cache entries by model
    pub fn invalidate_by_model(&mut self, model: &str) -> Result<usize> {
        let deleted = self
            .db
            .conn
            .execute("DELETE FROM ai_cache WHERE model = ?1", params![model])?;
        Ok(deleted)
    }

    /// Clear entire cache
    pub fn clear(&mut self) -> Result<usize> {
        let deleted = self.db.conn.execute("DELETE FROM ai_cache", [])?;
        Ok(deleted)
    }

    /// Get cache statistics
    pub fn get_stats(&mut self) -> Result<AiCacheStats> {
        let now = Self::current_timestamp();

        let total_entries: usize =
            self.db
                .conn
                .query_row("SELECT COUNT(*) FROM ai_cache", [], |row| row.get(0))?;

        let active_entries: usize = self.db.conn.query_row(
            "SELECT COUNT(*) FROM ai_cache WHERE expires_at = 0 OR expires_at > ?1",
            params![now],
            |row| row.get(0),
        )?;

        let total_size_bytes: u64 = self.db.conn.query_row(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM ai_cache",
            [],
            |row| row.get(0),
        )?;

        let expired_entries: usize = self.db.conn.query_row(
            "SELECT COUNT(*) FROM ai_cache WHERE expires_at > 0 AND expires_at <= ?1",
            params![now],
            |row| row.get(0),
        )?;

        let avg_age_seconds: f64 = self
            .db
            .conn
            .query_row(
                "SELECT AVG(?1 - created_at) FROM ai_cache",
                params![now],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        let max_access_count: u64 = self
            .db
            .conn
            .query_row("SELECT MAX(access_count) FROM ai_cache", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);

        let total_accesses = self.stats.hits + self.stats.misses;
        let hit_rate = if total_accesses > 0 {
            (self.stats.hits as f64 / total_accesses as f64) * 100.0
        } else {
            0.0
        };

        self.stats.total_entries = total_entries;
        self.stats.active_entries = active_entries;
        self.stats.total_size_bytes = total_size_bytes;
        self.stats.expired_entries = expired_entries;
        self.stats.avg_age_seconds = avg_age_seconds;
        self.stats.max_access_count = max_access_count;
        self.stats.hit_rate = hit_rate;

        Ok(self.stats.clone())
    }

    /// Search cache entries
    pub fn search(
        &self,
        query: Option<&str>,
        model_filter: Option<&str>,
        tag_filter: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AiCacheEntry>> {
        let mut conditions = vec!["1=1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

        if let Some(query) = query {
            conditions.push("(prompt LIKE ? OR response LIKE ?)".to_string());
            params.push(Box::new(format!("%{}%", query)));
            params.push(Box::new(format!("%{}%", query)));
        }

        if let Some(model) = model_filter {
            conditions.push("model = ?".to_string());
            params.push(Box::new(model.to_string()));
        }

        if let Some(tag) = tag_filter {
            conditions.push("tags LIKE ?".to_string());
            params.push(Box::new(format!("%{}%", tag)));
        }

        let sql = format!(
            "SELECT cache_key, model, prompt, options, response, metadata, 
                    created_at, last_accessed_at, expires_at, size_bytes, access_count, tags
             FROM ai_cache 
             WHERE {}
             ORDER BY last_accessed_at DESC
             LIMIT ? OFFSET ?",
            conditions.join(" AND ")
        );

        let mut stmt = self.db.conn.prepare(&sql)?;

        // Convert params to correct type for query_map
        let params_vec: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| &**p).collect();

        let mut rows = stmt.query(rusqlite::params_from_iter(params_vec.into_iter().chain([
            &limit as &dyn rusqlite::ToSql,
            &offset as &dyn rusqlite::ToSql,
        ])))?;

        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            entries.push(AiCacheEntry {
                cache_key: row.get(0)?,
                model: row.get(1)?,
                prompt: row.get(2)?,
                options: row.get(3)?,
                response: row.get(4)?,
                metadata: row.get(5)?,
                created_at: row.get(6)?,
                last_accessed_at: row.get(7)?,
                expires_at: row.get(8)?,
                size_bytes: row.get(9)?,
                access_count: row.get(10)?,
                tags: row.get(11)?,
            });
        }

        Ok(entries)
    }

    /// Export cache to file
    pub fn export_to_file(&self, path: &std::path::Path) -> Result<()> {
        let entries = self.search(None, None, None, usize::MAX, 0)?;
        let json = serde_json::to_string_pretty(&entries)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Import cache from file
    pub fn import_from_file(&mut self, path: &std::path::Path) -> Result<usize> {
        let json = std::fs::read_to_string(path)?;
        let entries: Vec<AiCacheEntry> = serde_json::from_str(&json)?;

        let mut count = 0;
        for entry in entries {
            self.put(entry)?;
            count += 1;
        }

        Ok(count)
    }

    /// Warm cache with common operations
    pub fn warm_cache(&mut self) -> Result<()> {
        // Common Soroban patterns and questions to pre-cache
        let common_prompts = vec![
            (
                "codellama:7b",
                "What is Soroban?",
                "Soroban is the smart contract platform for the Stellar network...",
            ),
            (
                "codellama:7b",
                "How do I create a token contract in Soroban?",
                "To create a token contract in Soroban...",
            ),
            (
                "codellama:7b",
                "What is the storage interface in Soroban?",
                "The storage interface in Soroban provides...",
            ),
            (
                "codellama:7b",
                "How do I handle errors in Soroban contracts?",
                "Error handling in Soroban contracts...",
            ),
        ];

        for (model, prompt, response) in common_prompts {
            let cache_key = Self::generate_cache_key(model, prompt, "{}");
            let entry = AiCacheEntry {
                cache_key,
                model: model.to_string(),
                prompt: prompt.to_string(),
                options: "{}".to_string(),
                response: response.to_string(),
                metadata: serde_json::json!({
                    "source": "cache_warming",
                    "created_by": "system"
                })
                .to_string(),
                created_at: Self::current_timestamp(),
                last_accessed_at: Self::current_timestamp(),
                expires_at: 0, // Never expire
                size_bytes: (model.len() + prompt.len() + response.len()) as u64,
                access_count: 0,
                tags: "system,warmup,soroban".to_string(),
            };

            self.put(entry)?;
        }

        Ok(())
    }
}
