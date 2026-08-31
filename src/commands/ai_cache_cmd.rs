//! `starforge ai cache` — AI request cache management.
//!
//! Sub-commands
//! ────────────
//! - `stats`      – show cache statistics and hit rates
//! - `list`       – list cached entries with filtering
//! - `clear`      – clear all cache entries
//! - `invalidate` – invalidate cache entries by tags or model
//! - `export`     – export cache to file
//! - `import`     – import cache from file
//! - `warm`       – pre-warm cache with common operations

use crate::utils::{ai_cache, print as p};
use anyhow::Result;
use clap::Subcommand;
use std::path::{Path, PathBuf};

// ─── Sub-command enum ─────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum AiCacheCommands {
    /// Show cache statistics and hit rates
    Stats,

    /// List cached entries with filtering
    List {
        /// Search query (matches prompt or response)
        #[arg(short, long)]
        query: Option<String>,

        /// Filter by model name
        #[arg(short, long)]
        model: Option<String>,

        /// Filter by tags
        #[arg(short, long)]
        tags: Option<String>,

        /// Maximum number of entries to show
        #[arg(short, long, default_value_t = 20)]
        limit: usize,

        /// Starting offset
        #[arg(short, long, default_value_t = 0)]
        offset: usize,
    },

    /// Clear all cache entries
    Clear {
        /// Confirm clearing without prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Invalidate cache entries by tags or model
    Invalidate {
        /// Invalidate entries with these tags
        #[arg(short, long, conflicts_with = "model")]
        tags: Option<String>,

        /// Invalidate entries for this model
        #[arg(short, long, conflicts_with = "tags")]
        model: Option<String>,
    },

    /// Export cache to file
    Export {
        /// Path to export file (JSON format)
        #[arg(value_name = "FILE")]
        path: PathBuf,
    },

    /// Import cache from file
    Import {
        /// Path to import file (JSON format)
        #[arg(value_name = "FILE")]
        path: PathBuf,
    },

    /// Pre-warm cache with common operations
    Warm,
}

// ─── Entry point ──────────────────────────────────────────────────────────────

pub async fn handle(cmd: AiCacheCommands) -> Result<()> {
    match cmd {
        AiCacheCommands::Stats => handle_stats().await,
        AiCacheCommands::List {
            query,
            model,
            tags,
            limit,
            offset,
        } => {
            handle_list(
                query.as_deref(),
                model.as_deref(),
                tags.as_deref(),
                limit,
                offset,
            )
            .await
        }
        AiCacheCommands::Clear { force } => handle_clear(force).await,
        AiCacheCommands::Invalidate { tags, model } => {
            handle_invalidate(tags.as_deref(), model.as_deref()).await
        }
        AiCacheCommands::Export { path } => handle_export(&path).await,
        AiCacheCommands::Import { path } => handle_import(&path).await,
        AiCacheCommands::Warm => handle_warm().await,
    }
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_stats() -> Result<()> {
    let mut cache = ai_cache::AiCache::open()?;
    let stats = cache.get_stats()?;

    p::header("AI Request Cache Statistics");
    p::separator();

    p::kv("Total entries", &stats.total_entries.to_string());
    p::kv("Active entries", &stats.active_entries.to_string());
    p::kv("Expired entries", &stats.expired_entries.to_string());
    p::kv("Total size", &format!("{} bytes", stats.total_size_bytes));
    p::kv(
        "Max size",
        &format!("{} bytes", ai_cache::MAX_CACHE_SIZE_BYTES),
    );
    p::kv("Cache hits", &stats.hits.to_string());
    p::kv("Cache misses", &stats.misses.to_string());
    p::kv("Hit rate", &format!("{:.1}%", stats.hit_rate));
    p::kv(
        "Average age",
        &format!("{:.1} hours", stats.avg_age_seconds / 3600.0),
    );
    p::kv("Max access count", &stats.max_access_count.to_string());

    // Show most accessed entries
    if stats.max_access_count > 0 {
        println!();
        p::info("Most Accessed Entries:");

        let entries = cache.search(None, None, None, 5, 0)?;
        for (i, entry) in entries.iter().enumerate() {
            println!(
                "  {}. {} ({} accesses)",
                i + 1,
                truncate(&entry.prompt, 50),
                entry.access_count
            );
        }
    }

    // Show cache health
    println!();
    if stats.hit_rate > 30.0 {
        p::success(&format!("Good hit rate: {:.1}%", stats.hit_rate));
    } else if stats.hit_rate > 10.0 {
        p::warn(&format!(
            "Low hit rate: {:.1}% - consider cache warming",
            stats.hit_rate
        ));
    } else {
        p::error(&format!(
            "Very low hit rate: {:.1}% - cache not effective",
            stats.hit_rate
        ));
    }

    if stats.total_size_bytes > ai_cache::MAX_CACHE_SIZE_BYTES * 3 / 4 {
        p::warn("Cache is approaching size limit - consider cleaning up");
    }

    p::separator();
    Ok(())
}

async fn handle_list(
    query: Option<&str>,
    model: Option<&str>,
    tags: Option<&str>,
    limit: usize,
    offset: usize,
) -> Result<()> {
    let cache = ai_cache::AiCache::open()?;
    let entries = cache.search(query, model, tags, limit, offset)?;

    p::header("AI Cache Entries");
    p::separator();

    if entries.is_empty() {
        p::info("No cache entries found");
        if query.is_some() || model.is_some() || tags.is_some() {
            p::info("Try removing filters to see all entries");
        }
    } else {
        let headers = &["#", "Model", "Prompt", "Accesses", "Age", "Size", "Tags"];
        let rows: Vec<Vec<String>> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let age_hours =
                    (ai_cache::AiCache::current_timestamp() - entry.created_at) as f64 / 3600.0;
                let size_kb = entry.size_bytes as f64 / 1024.0;

                vec![
                    (offset + i + 1).to_string(),
                    truncate(&entry.model, 15),
                    truncate(&entry.prompt, 40),
                    entry.access_count.to_string(),
                    format!("{:.1}h", age_hours),
                    format!("{:.1}KB", size_kb),
                    truncate(&entry.tags, 20),
                ]
            })
            .collect();

        p::table(headers, &rows);

        println!();
        p::info(&format!(
            "Showing {} entries (offset: {})",
            entries.len(),
            offset
        ));

        if entries.len() == limit {
            p::info("More entries available - increase limit or use offset");
        }
    }

    p::separator();
    Ok(())
}

async fn handle_clear(force: bool) -> Result<()> {
    if !force {
        p::warn("This will clear ALL AI cache entries.");
        p::warn("This action cannot be undone.");

        let confirmed = dialoguer::Confirm::new()
            .with_prompt("Are you sure you want to clear the cache?")
            .default(false)
            .interact()?;

        if !confirmed {
            p::info("Cache clear cancelled");
            return Ok(());
        }
    }

    let mut cache = ai_cache::AiCache::open()?;
    let cleared = cache.clear()?;

    p::success(&format!("Cleared {} cache entries", cleared));
    p::info("Future AI requests will start with a fresh cache");

    p::separator();
    Ok(())
}

async fn handle_invalidate(tags: Option<&str>, model: Option<&str>) -> Result<()> {
    let mut cache = ai_cache::AiCache::open()?;
    let invalidated = match (tags, model) {
        (Some(tags), None) => {
            p::info(&format!("Invalidating cache entries with tags: {}", tags));
            cache.invalidate_by_tags(tags)?
        }
        (None, Some(model)) => {
            p::info(&format!("Invalidating cache entries for model: {}", model));
            cache.invalidate_by_model(model)?
        }
        _ => {
            anyhow::bail!("Must specify either --tags or --model to invalidate");
        }
    };

    if invalidated > 0 {
        p::success(&format!("Invalidated {} cache entries", invalidated));
    } else {
        p::info("No matching cache entries found to invalidate");
    }

    p::separator();
    Ok(())
}

async fn handle_export(path: &Path) -> Result<()> {
    let cache = ai_cache::AiCache::open()?;

    p::header("Exporting AI Cache");
    p::separator();

    cache.export_to_file(path)?;

    p::success(&format!("Cache exported to {}", path.display()));
    p::info("File contains all cache entries in JSON format");

    p::separator();
    Ok(())
}

async fn handle_import(path: &Path) -> Result<()> {
    let mut cache = ai_cache::AiCache::open()?;

    p::header("Importing AI Cache");
    p::separator();

    if !path.exists() {
        anyhow::bail!("Import file does not exist: {}", path.display());
    }

    let imported = cache.import_from_file(path)?;

    p::success(&format!("Imported {} cache entries", imported));
    p::info("Imported entries are now available for cache hits");

    p::separator();
    Ok(())
}

async fn handle_warm() -> Result<()> {
    let mut cache = ai_cache::AiCache::open()?;

    p::header("Warming AI Cache");
    p::separator();

    p::info("Adding common Soroban patterns and questions to cache...");

    cache.warm_cache()?;

    p::success("Cache warmed with common operations");
    p::info("Common queries will now return cached responses instantly");

    let stats = cache.get_stats()?;
    p::kv("Total entries", &stats.total_entries.to_string());
    p::kv("Active entries", &stats.active_entries.to_string());

    p::separator();
    Ok(())
}

// ─── Utilities ────────────────────────────────────────────────────────────────

fn truncate(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}
