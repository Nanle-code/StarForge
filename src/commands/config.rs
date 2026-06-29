use crate::utils::{config, database, print as p};
use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Show current global configuration
    Show,
    /// Set a scalar configuration value
    Set {
        /// Configuration key, e.g. telemetry.enabled or network
        key: String,
        /// New value
        value: String,
    },
    /// Manage trusted plugin source allowlist
    #[command(subcommand)]
    PluginTrust(PluginTrustCommands),
    /// Migrate the config file to the latest schema version
    Migrate {
        /// Show what changes would be applied without writing anything
        #[arg(long, default_value = "false")]
        dry_run: bool,
    },
    /// Set global wallet encryption parameters (Argon2id)
    SetEncryption {
        /// Argon2 memory cost in KiB (e.g. 65536)
        #[arg(long)]
        mem: Option<u32>,
        /// Argon2 iteration count (e.g. 3)
        #[arg(long)]
        iterations: Option<u32>,
        /// Argon2 parallelism factor (e.g. 4)
        #[arg(long)]
        parallelism: Option<u32>,
        /// Reset to library defaults
        #[arg(long, default_value = "false")]
        reset: bool,
    },
    /// Validate configuration and check network connectivity
    Doctor,
    /// SQLite database management (init, migrate, query, backup, export)
    #[command(subcommand)]
    Db(DbCommands),
}

#[derive(Subcommand)]
pub enum DbCommands {
    /// Initialize the SQLite database schema
    Init,
    /// Migrate existing TOML configuration into SQLite
    Migrate,
    /// Run a raw SQL SELECT query against the database
    Query {
        /// SQL query to execute (SELECT only)
        sql: String,
    },
    /// Backup the database to a file
    Backup {
        /// Destination file path
        dest: String,
    },
    /// Export database contents back to TOML format
    Export {
        /// Output file path (default: stdout)
        #[arg(long)]
        out: Option<String>,
    },
    /// Show database status and statistics
    Status,
    /// Run integrity check on the database
    Check,
}

#[derive(Subcommand)]
pub enum PluginTrustCommands {
    /// List trusted plugin sources
    List,
    /// Add a trusted plugin domain or URL prefix
    Add {
        /// Domain or URL prefix to trust
        source: String,
    },
    /// Remove a trusted plugin source
    Remove {
        /// Domain or URL prefix to remove
        source: String,
    },
    /// Reset trusted plugin sources to StarForge defaults
    Reset,
}

pub async fn handle(cmd: ConfigCommands) -> Result<()> {
    match cmd {
        ConfigCommands::Show => show(),
        ConfigCommands::Set { key, value } => set_value(&key, &value),
        ConfigCommands::PluginTrust(cmd) => plugin_trust(cmd),
        ConfigCommands::Migrate { dry_run } => migrate(dry_run),
        ConfigCommands::SetEncryption {
            mem,
            iterations,
            parallelism,
            reset,
        } => set_encryption(mem, iterations, parallelism, reset),
        ConfigCommands::Doctor => crate::commands::doctor::run().await,
        ConfigCommands::Db(cmd) => handle_db(cmd),
    }
}

fn handle_db(cmd: DbCommands) -> Result<()> {
    match cmd {
        DbCommands::Init => db_init(),
        DbCommands::Migrate => db_migrate(),
        DbCommands::Query { sql } => db_query(&sql),
        DbCommands::Backup { dest } => db_backup(&dest),
        DbCommands::Export { out } => db_export(out.as_deref()),
        DbCommands::Status => db_status(),
        DbCommands::Check => db_check(),
    }
}

fn db_init() -> Result<()> {
    p::header("Database Initialization");
    let path = database::db_path();
    p::kv("Database path", &path.display().to_string());

    let db = database::Database::open()?;
    db.initialize()?;

    p::success("SQLite database initialized successfully.");
    p::info("Schema created: wallets, networks, config_kv, plugins, templates, meta");
    p::info("Run `starforge config db migrate` to import your TOML configuration.");
    Ok(())
}

fn db_migrate() -> Result<()> {
    p::header("TOML → SQLite Migration");

    let db = database::Database::open()?;
    db.initialize()?;

    let report = database::migrate_from_toml(&db)?;

    p::separator();
    p::kv("Wallets migrated", &report.wallets_migrated.to_string());
    p::kv("Networks migrated", &report.networks_migrated.to_string());
    p::kv(
        "Config keys migrated",
        &report.config_keys_migrated.to_string(),
    );
    p::success("Migration complete. TOML config still available for read/write.");
    p::info("Run `starforge config db status` to verify the database contents.");
    Ok(())
}

fn db_query(sql: &str) -> Result<()> {
    let sql_lower = sql.trim_start().to_ascii_lowercase();
    if !sql_lower.starts_with("select") {
        anyhow::bail!("Only SELECT queries are allowed via `config db query` for safety.");
    }

    let db = database::Database::open()?;
    let result = db.execute_query(sql)?;

    if result.rows.is_empty() {
        p::info("Query returned no rows.");
        return Ok(());
    }

    let col_widths: Vec<usize> = result
        .columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            result
                .rows
                .iter()
                .map(|r| r.get(i).map(|s| s.len()).unwrap_or(0))
                .max()
                .unwrap_or(0)
                .max(col.len())
        })
        .collect();

    let header: Vec<String> = result
        .columns
        .iter()
        .enumerate()
        .map(|(i, col)| format!("{:<width$}", col, width = col_widths[i]))
        .collect();
    println!("  {}", header.join("  |  "));
    println!(
        "  {}",
        "─".repeat(header.iter().map(|h| h.len()).sum::<usize>() + result.columns.len() * 5)
    );

    for row in &result.rows {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, v)| format!("{:<width$}", v, width = col_widths[i]))
            .collect();
        println!("  {}", cells.join("  |  "));
    }
    println!();
    p::kv("Rows", &result.rows_affected.to_string());
    Ok(())
}

fn db_backup(dest: &str) -> Result<()> {
    p::header("Database Backup");
    let dest_path = std::path::Path::new(dest);
    let db = database::Database::open()?;
    db.backup(dest_path)?;
    p::kv("Backup saved", dest);
    p::success("Database backup complete.");
    Ok(())
}

fn db_export(out: Option<&str>) -> Result<()> {
    p::header("Database → TOML Export");

    let db = database::Database::open()?;
    let toml_str = database::export_to_toml(&db)?;

    if let Some(path) = out {
        std::fs::write(path, &toml_str)?;
        p::kv("Exported to", path);
        p::success("Export complete.");
    } else {
        println!("{}", toml_str);
    }
    Ok(())
}

fn db_status() -> Result<()> {
    p::header("Database Status");

    let path = database::db_path();
    p::kv("Path", &path.display().to_string());
    p::kv(
        "Exists",
        if path.exists() {
            "yes"
        } else {
            "no — run `starforge config db init`"
        },
    );

    if !path.exists() {
        return Ok(());
    }

    let db = database::Database::open()?;
    let stats = db.stats()?;

    p::separator();
    p::kv("Schema version", &stats.schema_version);
    p::kv("Wallets", &stats.wallets.to_string());
    p::kv("Networks", &stats.networks.to_string());
    p::kv("Config entries", &stats.config_entries.to_string());
    p::kv("Database size", &format!("{} bytes", stats.db_size_bytes));
    p::separator();
    Ok(())
}

fn db_check() -> Result<()> {
    p::header("Database Integrity Check");

    let db = database::Database::open()?;
    let results = db.integrity_check()?;

    for line in &results {
        if line == "ok" {
            p::success("Integrity check passed.");
        } else {
            p::warn(&format!("Issue: {}", line));
        }
    }
    Ok(())
}

fn show() -> Result<()> {
    let cfg = config::load()?;
    p::header("StarForge Configuration");
    p::separator();

    p::kv("Config file", &config::config_path().display().to_string());
    p::kv("Active network", &cfg.network);
    p::kv(
        "telemetry.enabled",
        &cfg.telemetry_enabled.unwrap_or(false).to_string(),
    );

    println!();
    p::header("Plugin Trust");
    if cfg.plugin_trust.trusted_sources.is_empty() {
        p::warn("No trusted remote plugin sources configured.");
    } else {
        for source in &cfg.plugin_trust.trusted_sources {
            p::kv("trusted source", source);
        }
    }

    println!();
    p::header("Wallet Encryption (Argon2id)");
    if let Some(kdf) = &cfg.wallet_encryption {
        p::kv("Memory cost", &format!("{} KiB", kdf.mem.unwrap_or(32768)));
        p::kv("Iterations", &kdf.iterations.unwrap_or(3).to_string());
        p::kv("Parallelism", &kdf.parallelism.unwrap_or(1).to_string());
    } else {
        p::info("Using default Argon2id parameters:");
        p::kv("Memory cost", "32768 KiB (default)");
        p::kv("Iterations", "3 (default)");
        p::kv("Parallelism", "1 (default)");
    }

    p::separator();
    Ok(())
}

fn set_value(key: &str, value: &str) -> Result<()> {
    let mut cfg = config::load()?;
    match key {
        "telemetry" | "telemetry.enabled" => {
            cfg.telemetry_enabled = Some(parse_bool(value)?);
        }
        "network" => {
            config::validate_network_exists(&cfg, value)?;
            cfg.network = value.to_string();
        }
        _ => {
            anyhow::bail!(
                "Unsupported config key '{}'. Supported keys: telemetry.enabled, network",
                key
            );
        }
    }
    config::save(&cfg)?;
    p::success(&format!("{} set to '{}'", key, value));
    Ok(())
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" | "enabled" => Ok(true),
        "false" | "0" | "no" | "off" | "disabled" => Ok(false),
        _ => anyhow::bail!(
            "Expected boolean value for telemetry.enabled, got '{}'",
            value
        ),
    }
}

fn plugin_trust(cmd: PluginTrustCommands) -> Result<()> {
    match cmd {
        PluginTrustCommands::List => {
            let cfg = config::load()?;
            print_plugin_trust_sources(&cfg);
        }
        PluginTrustCommands::Add { source } => {
            let mut cfg = config::load()?;
            let added = config::add_trusted_plugin_source(&mut cfg, source.clone())?;
            config::save(&cfg)?;
            if added {
                p::success(&format!("Trusted plugin source added: {}", source.trim()));
            } else {
                p::info(&format!(
                    "Trusted plugin source already exists: {}",
                    source.trim()
                ));
            }
            print_plugin_trust_sources(&cfg);
        }
        PluginTrustCommands::Remove { source } => {
            let mut cfg = config::load()?;
            if !config::remove_trusted_plugin_source(&mut cfg, &source) {
                anyhow::bail!("Trusted plugin source not found: {}", source.trim());
            }
            config::save(&cfg)?;
            p::success(&format!("Trusted plugin source removed: {}", source.trim()));
            print_plugin_trust_sources(&cfg);
        }
        PluginTrustCommands::Reset => {
            let mut cfg = config::load()?;
            config::reset_trusted_plugin_sources(&mut cfg);
            config::save(&cfg)?;
            p::success("Trusted plugin sources reset to defaults.");
            print_plugin_trust_sources(&cfg);
        }
    }
    Ok(())
}

fn print_plugin_trust_sources(cfg: &config::Config) {
    p::header("Trusted Plugin Sources");
    if cfg.plugin_trust.trusted_sources.is_empty() {
        p::warn("No trusted remote plugin sources configured.");
        return;
    }
    for source in &cfg.plugin_trust.trusted_sources {
        p::info(&format!("- {}", source));
    }
}

fn migrate(dry_run: bool) -> Result<()> {
    p::header("Config Migration");
    p::separator();
    p::kv("Config file", &config::config_path().display().to_string());
    p::kv("Target schema version", config::CURRENT_CONFIG_VERSION);

    if dry_run {
        let Some(outcome) = config::plan_migration()? else {
            p::info("No config file exists yet — nothing to migrate.");
            return Ok(());
        };

        if !outcome.migrated() {
            p::success("Config is already at the latest schema version. No changes needed.");
            return Ok(());
        }

        println!();
        p::info("Migration steps that would be applied:");
        for (from, to) in &outcome.steps {
            p::kv("  step", &format!("v{from} -> v{to}"));
        }

        println!();
        p::info("Changes that would be written:");
        for change in &outcome.changes {
            println!("    {change}");
        }

        println!();
        p::warn("Dry run: no files were modified. Re-run without --dry-run to apply.");
        return Ok(());
    }

    let Some(outcome) = config::apply_migration()? else {
        p::info("No config file exists yet — nothing to migrate.");
        return Ok(());
    };

    if !outcome.migrated() {
        p::success("Config is already at the latest schema version. No changes needed.");
        return Ok(());
    }

    println!();
    p::info("Applied migration steps:");
    for (from, to) in &outcome.steps {
        p::kv("  step", &format!("v{from} -> v{to}"));
    }

    println!();
    p::info("Changes written:");
    for change in &outcome.changes {
        println!("    {change}");
    }

    println!();
    p::info(&format!(
        "A backup of the previous config was saved to {}",
        config::config_dir().join("config.toml.bak").display()
    ));
    p::success("Config migrated successfully.");
    Ok(())
}

fn set_encryption(
    mem: Option<u32>,
    iterations: Option<u32>,
    parallelism: Option<u32>,
    reset: bool,
) -> Result<()> {
    let mut cfg = config::load()?;

    if reset {
        cfg.wallet_encryption = None;
        config::save(&cfg)?;
        p::success("Wallet encryption parameters reset to defaults.");
        return Ok(());
    }

    if mem.is_none() && iterations.is_none() && parallelism.is_none() {
        anyhow::bail!("Provide at least one parameter to set (e.g. --mem 65536) or use --reset");
    }

    let mut kdf = cfg.wallet_encryption.unwrap_or_default();
    if let Some(m) = mem {
        kdf.mem = Some(m);
    }
    if let Some(i) = iterations {
        kdf.iterations = Some(i);
    }
    if let Some(p) = parallelism {
        kdf.parallelism = Some(p);
    }

    cfg.wallet_encryption = Some(kdf);
    config::save(&cfg)?;

    p::success("Global wallet encryption parameters updated.");
    show()?;
    Ok(())
}
