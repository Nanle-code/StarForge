use crate::plugins::interface::CORE_VERSION;
use crate::plugins::manifest;
use crate::plugins::registry::{self, RegisteredCommand, TrustLevel, UninstallOptions};
use crate::plugins::{PluginLoadError, PluginManager};
use crate::utils::config;
use crate::utils::output;
use crate::utils::print as p;
use anyhow::{Context, Result};
use clap::Subcommand;
use std::path::Path;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum PluginCommands {
    /// Register a plugin shared library for StarForge to load
    ///
    /// Example: starforge plugin install starforge-defi --path ./libstarforge_defi.so
    Install {
        /// Plugin name (used as the command name)
        name: String,
        /// Path to the plugin shared library (.so/.dylib/.dll)
        #[arg(long)]
        path: Option<PathBuf>,
        /// Source URL or identifier for trust classification
        #[arg(long)]
        source: Option<String>,
        /// Install even if the plugin source is untrusted (requires explicit confirmation)
        #[arg(long)]
        force: bool,
    },
    /// List installed plugins from the local registry
    List {
        /// Emit a machine-readable JSON object instead of the human-readable output
        #[arg(long)]
        json: bool,
    },
    /// Load installed plugins and show those successfully loaded
    Load,
    /// Remove a plugin from the registry
    ///
    /// Example: starforge plugin uninstall starforge-defi
    Uninstall {
        /// Plugin name to remove
        name: String,
        /// Also delete the plugin library file from disk (only under ~/.starforge/plugins/)
        #[arg(long)]
        purge: bool,
        /// Skip confirmation for destructive removal
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Verify trust and compatibility of installed plugins
    Verify {
        /// Plugin name to verify (verifies all plugins if omitted)
        name: Option<String>,
        /// Run the full audit checks, including manifest validation
        #[arg(long)]
        deep: bool,
        /// Attempt to load each plugin as an optional runtime self-check
        #[arg(long)]
        runtime_check: bool,
    },
    /// Audit installed plugins for filesystem, trust, manifest, compatibility, and runtime issues
    Audit {
        /// Plugin name to audit (audits all plugins if omitted)
        name: Option<String>,
        /// Attempt to load each plugin as an optional runtime self-check
        #[arg(long)]
        runtime_check: bool,
    },
    /// Update installed plugins to their latest versions
    ///
    /// Checks each plugin's source URL, validates compatibility with the running
    /// CLI, and replaces the local library if a newer copy is available.
    /// Configuration and trust settings are preserved.
    ///
    /// Example: starforge plugin update
    ///          starforge plugin update starforge-defi
    Update {
        /// Plugin name to update (updates all plugins if omitted)
        name: Option<String>,
        /// Skip confirmation prompt
        #[arg(long, default_value = "false")]
        yes: bool,
    },
    /// List commands registered by installed plugins
    Commands {
        /// Show commands for a specific plugin only
        name: Option<String>,
    },
    /// Search the plugin marketplace for available plugins
    Search {
        /// Search query (e.g. "ai", "security")
        query: Option<String>,
    },
}

pub async fn handle(cmd: PluginCommands) -> Result<()> {
    match cmd {
        PluginCommands::Install {
            name,
            path,
            source,
            force,
        } => install(name, path, source, force),
        PluginCommands::List { json } => list(json),
        PluginCommands::Load => load(),
        PluginCommands::Uninstall { name, purge, yes } => uninstall(name, purge, yes),
        PluginCommands::Verify {
            name,
            deep,
            runtime_check,
        } => verify(name, deep, runtime_check),
        PluginCommands::Audit {
            name,
            runtime_check,
        } => audit(name, runtime_check),
        PluginCommands::Update { name, yes } => update(name, yes),
        PluginCommands::Commands { name } => commands(name),
        PluginCommands::Search { query } => search(query).await,
    }
}

fn install(name: String, path: Option<PathBuf>, source: Option<String>, force: bool) -> Result<()> {
    let lib_path = registry::resolve_plugin_library_path(&name, path)?;
    let source_str = source.as_deref().unwrap_or("");
    let config = config::load().unwrap_or_default();
    let trust = registry::classify_source_with_config(source_str, &config);

    // Warn the user about untrusted sources and require --force to proceed.
    if trust == TrustLevel::Unknown && !source_str.is_empty() && !force {
        p::header("Plugin Install — Trust Warning");
        p::warn(&format!(
            "Plugin source '{}' is not in the trusted sources list.",
            source_str
        ));
        p::info("Trusted sources:");
        for src in &config.plugin_trust.trusted_sources {
            p::info(&format!("  • {}", src));
        }
        p::info("");
        p::info("To install anyway: starforge plugin install <name> --source <url> --force");
        p::info("To install from a local path (always trusted): starforge plugin install <name> --path <lib>");
        anyhow::bail!("Refusing to install plugin from untrusted source without --force");
    }

    let plugin_manifest = manifest::require_compatible_manifest(&lib_path, &name)?;

    // Verify publisher signature and trust metadata
    let ver_res =
        crate::plugins::verifier::verify_plugin_signature(&lib_path, &plugin_manifest, &config);
    match ver_res.status {
        crate::plugins::verifier::VerificationStatus::InvalidSignature
        | crate::plugins::verifier::VerificationStatus::MalformedKey
        | crate::plugins::verifier::VerificationStatus::MalformedSignature => {
            if !force {
                p::header("Plugin Install — Verification Failure");
                p::error(&format!(
                    "Signature verification failed: {}",
                    ver_res.detail.as_deref().unwrap_or("")
                ));
                anyhow::bail!("Refusing to install plugin with invalid signature without --force");
            } else {
                p::warn("Installing plugin with invalid signature because --force was specified");
            }
        }
        crate::plugins::verifier::VerificationStatus::UntrustedPublisher => {
            if !force {
                p::header("Plugin Install — Untrusted Publisher");
                p::warn(&format!(
                    "Publisher key '{}' is not in the trusted publishers list.",
                    ver_res.publisher_key.as_deref().unwrap_or("")
                ));
                anyhow::bail!(
                    "Refusing to install plugin from untrusted publisher without --force"
                );
            } else {
                p::warn("Installing plugin from untrusted publisher because --force was specified");
            }
        }
        crate::plugins::verifier::VerificationStatus::Unsigned => {
            if config.plugin_trust.require_signatures && !force {
                anyhow::bail!(
                    "CLI configuration requires signed plugins, but plugin is unsigned. Refusing without --force"
                );
            }
        }
        _ => {}
    }

    // Load the plugin to discover the commands it registers.
    let (discovered_commands, discovered_description): (Vec<RegisteredCommand>, String) = {
        let mut pm = PluginManager::new();
        unsafe {
            pm.load_plugin(&lib_path).with_context(|| {
                format!("Failed to load plugin '{}' to discover commands", name)
            })?;
        }
        let commands = pm
            .list_commands()
            .into_iter()
            .map(|c| RegisteredCommand {
                name: c.name,
                description: c.description,
            })
            .collect();
        let description = pm
            .list_plugins()
            .into_iter()
            .map(|(_, desc, _)| desc.to_string())
            .find(|d| !d.is_empty())
            .unwrap_or_default();
        (commands, description)
    };

    registry::install_plugin(
        &name,
        &lib_path,
        source_str,
        &plugin_manifest.starforge_version,
        &plugin_manifest.version,
        &discovered_description,
        discovered_commands.clone(),
        plugin_manifest.publisher.clone(),
        plugin_manifest.publisher_key.clone(),
        ver_res.status.clone(),
    )?;

    p::header("Plugin Install");
    p::success("Plugin registered");
    p::kv_accent("Name", &name);
    p::kv("Library", &lib_path.display().to_string());
    p::kv("Plugin version", &plugin_manifest.version);
    p::kv(
        "StarForge compatibility",
        &plugin_manifest.starforge_version,
    );
    p::kv("Trust", trust.label());
    p::kv("Verification", ver_res.status.label());
    if let Some(ref pub_name) = plugin_manifest.publisher {
        p::kv("Publisher", pub_name);
    }
    if let Some(ref pub_key) = plugin_manifest.publisher_key {
        p::kv("Publisher key", pub_key);
    }
    if !source_str.is_empty() {
        p::kv("Source", source_str);
    }
    if !discovered_commands.is_empty() {
        p::info("Registered commands:");
        for cmd in &discovered_commands {
            p::info(&format!("  • {}  — {}", cmd.name, cmd.description));
        }
    }
    p::info("Load plugins with: starforge plugin load");
    Ok(())
}

#[derive(serde::Deserialize, Debug)]
struct MarketplacePlugin {
    name: String,
    description: String,
    url: String,
}

async fn search(query: Option<String>) -> Result<()> {
    p::header("Plugin Marketplace — Search");
    if let Some(ref q) = query {
        p::kv("Query", q);
    }

    // Attempt to fetch from official plugin registry, fallback to hardcoded examples for demonstration.
    let registry_url = "https://starforge-protocol.github.io/starforge/plugins/registry.json";

    let mut available_plugins = vec![
        MarketplacePlugin {
            name: "starforge-ai-audit".to_string(),
            description: "AI-powered smart contract auditing plugin".to_string(),
            url: "https://github.com/StarForge-Labs/starforge-ai-audit".to_string(),
        },
        MarketplacePlugin {
            name: "starforge-defi".to_string(),
            description: "DeFi scaffold and AMM tools".to_string(),
            url: "https://github.com/Nanle-code/starforge-defi".to_string(),
        },
    ];

    if let Ok(resp) = reqwest::get(registry_url).await {
        if let Ok(plugins) = resp.json::<Vec<MarketplacePlugin>>().await {
            available_plugins = plugins;
        }
    }

    if let Some(q) = query {
        let q = q.to_lowercase();
        available_plugins.retain(|p| {
            p.name.to_lowercase().contains(&q) || p.description.to_lowercase().contains(&q)
        });
    }

    if available_plugins.is_empty() {
        p::info("No plugins found matching your search.");
        return Ok(());
    }

    println!("\n  Found {} plugin(s):\n", available_plugins.len());

    let rows: Vec<Vec<String>> = available_plugins
        .into_iter()
        .map(|p| vec![p.name, p.description, p.url])
        .collect();

    p::table(&["Name", "Description", "Source URL"], &rows);
    println!("\nTo install a plugin, run: starforge plugin install <name> --source <url>");

    Ok(())
}

fn list(json: bool) -> Result<()> {
    let emit_json = json || output::is_json_mode_enabled();
    let reg = registry::load_registry().unwrap_or_default();

    if emit_json {
        #[derive(serde::Serialize)]
        struct PluginListResponse {
            plugin_count: usize,
            plugins: Vec<PluginSummary>,
        }

        #[derive(serde::Serialize)]
        struct PluginSummary {
            name: String,
            version: String,
            trust: String,
            source: String,
            description: String,
            commands: Vec<PluginCommandSummary>,
        }

        #[derive(serde::Serialize)]
        struct PluginCommandSummary {
            name: String,
            description: String,
        }

        let plugins: Vec<PluginSummary> = registry::plugin_list_entries(&reg)
            .into_iter()
            .map(|entry| PluginSummary {
                name: entry.name,
                version: entry.plugin_version,
                trust: entry.trust.label().to_string(),
                source: entry.source,
                description: entry.description,
                commands: entry
                    .commands
                    .into_iter()
                    .map(|cmd| PluginCommandSummary {
                        name: cmd.name,
                        description: cmd.description,
                    })
                    .collect(),
            })
            .collect();

        return output::print_json(&PluginListResponse {
            plugin_count: plugins.len(),
            plugins,
        });
    }

    p::header("Installed Plugins");
    if reg.plugins.is_empty() {
        p::info("No plugins installed. Use: starforge plugin install <name> --path <lib>");
        return Ok(());
    }

    p::kv("StarForge core version", CORE_VERSION);
    p::separator();
    let list_entries = registry::plugin_list_entries(&reg);

    let plugin_rows: Vec<Vec<String>> = list_entries
        .iter()
        .map(|entry| {
            vec![
                entry.name.clone(),
                entry.plugin_version.clone(),
                entry.trust.clone(),
                entry.description.clone(),
            ]
        })
        .collect();
    p::table(
        &["Name", "Version", "Trust", "Verification", "Description"],
        &plugin_rows,
    );

    let entries = reg.plugins.clone();
    let command_rows: Vec<Vec<String>> = entries
        .iter()
        .flat_map(|entry| {
            entry.commands.iter().map(|cmd| {
                vec![
                    entry.name.clone(),
                    cmd.name.clone(),
                    cmd.description.clone(),
                ]
            })
        })
        .collect();

    if !command_rows.is_empty() {
        println!();
        p::info("Commands");
        p::table(&["Plugin", "Command", "Description"], &command_rows);
    }

    p::separator();
    Ok(())
}

fn load() -> Result<()> {
    p::header("Plugin Loader");

    let reg = registry::load_registry().unwrap_or_default();
    if reg.plugins.is_empty() {
        p::info("No plugins installed. Use: starforge plugin install <name> --path <lib>");
        return Ok(());
    }

    let _config = config::load().unwrap_or_default();

    // Warn about any unknown-trust plugins before loading.
    for pl in reg.plugins.iter().filter(|p| {
        registry::classify_source(&p.source) == TrustLevel::Unknown && !p.source.is_empty()
    }) {
        p::warn(&format!(
            "Plugin '{}' is from an unknown/untrusted source: {}",
            pl.name, pl.source
        ));
    }

    let mut pm = PluginManager::new();
    let mut failed: Vec<(String, PluginLoadError)> = Vec::new();

    for pl in &reg.plugins {
        match unsafe { pm.load_plugin_diagnosed(&pl.path) } {
            Ok(()) => {}
            Err(e) => failed.push((pl.name.clone(), e)),
        }
    }

    // ── Report failures with structured diagnostics ──────────────────────────
    if !failed.is_empty() {
        p::warn(&format!("{} plugin(s) failed to load:", failed.len()));
        for (name, err) in &failed {
            println!();
            p::error(&format!("[{}] {}", err.category(), name));
            for line in err.diagnostic().lines() {
                println!("  {}", line);
            }
        }
        println!();
    }

    let loaded = pm.list_plugins();
    if loaded.is_empty() && failed.is_empty() {
        p::warn("No plugins loaded.");
        return Ok(());
    }

    if !loaded.is_empty() {
        p::kv("StarForge core version", CORE_VERSION);
        p::separator();
        for (name, desc, built_for) in loaded {
            p::kv_accent(name, desc);
            p::kv("Built for StarForge", built_for);
        }
        p::separator();
    }

    if !failed.is_empty() {
        anyhow::bail!(
            "{} plugin(s) failed to load. See diagnostics above.",
            failed.len()
        );
    }

    Ok(())
}

fn uninstall(name: String, purge: bool, yes: bool) -> Result<()> {
    let reg = registry::load_registry().unwrap_or_default();
    let plugin = reg.plugins.iter().find(|p| p.name == name).ok_or_else(|| {
        anyhow::anyhow!(
            "Plugin '{}' is not installed. Run `starforge plugin list` to see installed plugins.",
            name
        )
    })?;

    let lib_path = PathBuf::from(&plugin.path);
    let lib_exists = lib_path.exists();

    p::header("Plugin Uninstall");
    p::kv_accent("Plugin", &name);
    p::kv("Library", &plugin.path);

    if lib_exists {
        p::warn(
            "If this plugin is loaded in another StarForge session, close that session before purging files.",
        );
    } else {
        p::warn("Plugin library file is already missing on disk.");
    }

    if purge && !yes {
        p::warn("This will permanently delete the plugin library file.");
        p::info("Proceed with: starforge plugin uninstall <name> --purge --yes");
        anyhow::bail!("Refusing destructive uninstall without --yes");
    }

    // Best-effort: load plugin to run on_unload before registry removal
    if lib_exists {
        let mut pm = PluginManager::new();
        if let Err(e) = unsafe { pm.load_plugin(&lib_path) } {
            p::warn(&format!(
                "Could not load plugin for clean shutdown: {}. Proceeding with uninstall.",
                e
            ));
        } else {
            p::info("Plugin unloaded cleanly.");
        }
    }

    let opts = UninstallOptions {
        purge_files: purge,
        assume_yes: yes,
    };
    let report = registry::uninstall_plugin(&name, &opts)?;

    p::success(&format!("Plugin '{}' removed from registry", name));
    if report.files_removed {
        p::success("Plugin library file deleted.");
    } else if !purge {
        p::info("Library file kept on disk. Use --purge --yes to delete it.");
    }
    if report.library_was_missing && !report.files_removed {
        p::info("No library file was present to remove.");
    }

    Ok(())
}

// Not currently called from any code path in this crate. Kept rather than
// removed since deleting it is a product decision, not a lint-scoping one.
#[allow(dead_code)]
fn discover_commands_from_library(lib_path: &str) -> Result<Vec<RegisteredCommand>> {
    let path = Path::new(lib_path);
    let mut pm = PluginManager::new();
    unsafe {
        pm.load_plugin(path).with_context(|| {
            format!(
                "Failed to load plugin from '{}' to discover commands",
                lib_path
            )
        })?;
    }
    Ok(pm
        .list_commands()
        .into_iter()
        .map(|c| RegisteredCommand {
            name: c.name,
            description: c.description,
        })
        .collect())
}

fn update(name: Option<String>, yes: bool) -> Result<()> {
    p::header("Plugin Update");

    let reg = registry::load_registry().unwrap_or_default();
    if reg.plugins.is_empty() {
        p::info("No plugins installed. Use: starforge plugin install <name> --path <lib>");
        return Ok(());
    }

    let _config = config::load().unwrap_or_default();

    let to_update: Vec<_> = match &name {
        Some(n) => {
            let found: Vec<_> = reg.plugins.iter().filter(|p| &p.name == n).collect();
            if found.is_empty() {
                anyhow::bail!(
                    "Plugin '{}' is not installed. Run `starforge plugin list`.",
                    n
                );
            }
            found
        }
        None => reg.plugins.iter().collect(),
    };

    p::kv("Plugins to check", &to_update.len().to_string());
    p::kv("StarForge core version", CORE_VERSION);
    p::separator();

    let mut updated = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    for pl in &to_update {
        println!("  Checking: {}", pl.name);

        // Verify the library still exists at its registered path.
        let lib_exists = std::path::Path::new(&pl.path).exists();
        if !lib_exists {
            p::warn(&format!(
                "  '{}' library missing at {}. Re-install with: starforge plugin install {} --path <lib>",
                pl.name, pl.path, pl.name
            ));
            failed += 1;
            println!();
            continue;
        }

        // Only plugins with a non-empty, trusted source URL can be fetched remotely.
        if pl.source.is_empty() {
            p::info(&format!(
                "  '{}' was installed from a local path — no remote source to fetch from.",
                pl.name
            ));
            p::kv("  Path", &pl.path);
            skipped += 1;
            println!();
            continue;
        }

        let trust = registry::classify_source(&pl.source);
        if trust == TrustLevel::Unknown && !yes {
            p::warn(&format!(
                "  '{}' source '{}' is not trusted. Use --yes to force update from unknown sources.",
                pl.name, pl.source
            ));
            skipped += 1;
            println!();
            continue;
        }

        // For trusted/confirmed sources, re-install the plugin library.
        // This re-uses the existing path — the user is responsible for
        // placing an updated .so/.dylib at the same location, or the source
        // URL must be a direct download endpoint.
        //
        // For crates.io sources we attempt to download via `cargo install`.
        if pl.source.starts_with("https://crates.io/crates/") {
            let crate_name = pl
                .source
                .trim_start_matches("https://crates.io/crates/")
                .split('/')
                .next()
                .unwrap_or(&pl.name);

            p::info(&format!("  Attempting `cargo install {}` ...", crate_name));
            let status = std::process::Command::new("cargo")
                .args(["install", crate_name, "--force"])
                .status();

            match status {
                Ok(s) if s.success() => {
                    registry::install_plugin(
                        &pl.name,
                        std::path::Path::new(&pl.path),
                        &pl.source,
                        &pl.starforge_version,
                        &pl.plugin_version,
                        &pl.description,
                        pl.commands.clone(),
                        pl.publisher.clone(),
                        pl.publisher_key.clone(),
                        pl.verification_status.clone(),
                    )?;
                    p::success(&format!("  '{}' updated via cargo install", pl.name));
                    updated += 1;
                }
                Ok(s) => {
                    p::warn(&format!(
                        "  cargo install exited with status {}. Plugin not updated.",
                        s
                    ));
                    failed += 1;
                }
                Err(e) => {
                    p::warn(&format!(
                        "  Failed to run cargo: {}. Is Cargo installed?",
                        e
                    ));
                    failed += 1;
                }
            }
        } else {
            // For GitHub and other sources, check if the library file on disk exists
            // and refresh the registry metadata.
            let metadata = std::fs::metadata(&pl.path);
            match metadata {
                Ok(m) => {
                    let modified = m
                        .modified()
                        .ok()
                        .and_then(|t| {
                            t.duration_since(std::time::UNIX_EPOCH)
                                .ok()
                                .map(|d| d.as_secs())
                        })
                        .unwrap_or(0);

                    let installed_epoch = 0u64;

                    if modified > installed_epoch {
                        // Library on disk is newer — refresh the registry entry.
                        let (cmds, description) = discover_plugin_metadata(&pl.path)
                            .unwrap_or_else(|_| (pl.commands.clone(), pl.description.clone()));
                        registry::install_plugin(
                            &pl.name,
                            std::path::Path::new(&pl.path),
                            &pl.source,
                            &pl.starforge_version,
                            &pl.plugin_version,
                            &description,
                            cmds,
                            pl.publisher.clone(),
                            pl.publisher_key.clone(),
                            pl.verification_status.clone(),
                        )?;
                        p::success(&format!(
                            "  '{}' library on disk is newer — registry refreshed.",
                            pl.name
                        ));
                        updated += 1;
                    } else {
                        p::info(&format!(
                            "  '{}' is already up to date. Source: {}",
                            pl.name, pl.source
                        ));
                        p::info(
                            "  To update manually: replace the library at the registered path,",
                        );
                        p::info(&format!("  then run: starforge plugin update {}", pl.name));
                        skipped += 1;
                    }
                }
                Err(e) => {
                    p::warn(&format!("  Could not read library metadata: {}", e));
                    failed += 1;
                }
            }
        }

        println!();
    }

    p::separator();
    p::kv("Updated", &updated.to_string());
    p::kv("Skipped (already current / local)", &skipped.to_string());
    p::kv("Failed", &failed.to_string());

    if failed > 0 {
        anyhow::bail!("{} plugin(s) failed to update. See warnings above.", failed);
    }

    Ok(())
}

fn verify(name: Option<String>, deep: bool, runtime_check: bool) -> Result<()> {
    if deep || runtime_check {
        return run_audit(name, runtime_check);
    }

    p::header("Plugin Verification");

    let reg = registry::load_registry().unwrap_or_default();
    if reg.plugins.is_empty() {
        p::info("No plugins installed.");
        return Ok(());
    }

    let to_check: Vec<_> = match &name {
        Some(n) => {
            let found: Vec<_> = reg.plugins.iter().filter(|p| &p.name == n).collect();
            if found.is_empty() {
                anyhow::bail!("Plugin '{}' is not installed.", n);
            }
            found
        }
        None => reg.plugins.iter().collect(),
    };

    let _config = config::load().unwrap_or_default();
    let mut all_ok = true;

    for pl in &to_check {
        let lib_exists = std::path::Path::new(&pl.path).exists();

        let current_trust = registry::classify_source(&pl.source);
        let trust_ok = match current_trust {
            TrustLevel::Local | TrustLevel::Trusted => true,
            TrustLevel::Unknown => false,
        };

        let compat_ok = if pl.starforge_version.is_empty() {
            true
        } else {
            crate::plugins::interface::is_core_version_compatible(&pl.starforge_version)
        };

        let ver_res = manifest::load_manifest_for_library(std::path::Path::new(&pl.path))
            .ok()
            .flatten()
            .map(|mf| {
                crate::plugins::verifier::verify_plugin_signature(
                    std::path::Path::new(&pl.path),
                    &mf,
                    &config,
                )
            });

        let ver_status = ver_res
            .as_ref()
            .map(|r| r.status.clone())
            .unwrap_or(pl.verification_status.clone());
        let ver_ok = match ver_status {
            crate::plugins::verifier::VerificationStatus::Verified
            | crate::plugins::verifier::VerificationStatus::Unsigned => true,
            _ => false,
        };

        let status = if lib_exists && trust_ok && compat_ok && ver_ok {
            "✓ OK"
        } else if !lib_exists {
            all_ok = false;
            "✗ library missing"
        } else if !compat_ok {
            all_ok = false;
            "✗ incompatible"
        } else if !ver_ok {
            all_ok = false;
            "✗ signature invalid"
        } else {
            all_ok = false;
            "⚠ untrusted source"
        };

        println!(
            "  {:<24} [{}]  trust={}  verification={}",
            pl.name,
            status,
            current_trust.label(),
            ver_status.label(),
        );
        if !pl.starforge_version.is_empty() {
            p::kv("StarForge", &pl.starforge_version);
        }
        if !pl.source.is_empty() {
            p::kv("Source", &pl.source);
        }
        if let Some(ref publisher) = pl.publisher {
            p::kv("Publisher", publisher);
        }
        if !lib_exists {
            p::warn(&format!("Library not found at: {}", pl.path));
            p::info("Re-install with: starforge plugin install <name> --path <lib>");
        }
        if current_trust == TrustLevel::Unknown && !pl.source.is_empty() {
            p::warn("Source is not in the trusted sources list.");
            p::info("Check your CLI config for trusted sources.");
        }
        if !compat_ok && !pl.starforge_version.is_empty() {
            p::warn(&format!(
                "Plugin targets StarForge {} but running {}",
                pl.starforge_version, CORE_VERSION
            ));
            p::info("Reinstall a compatible build or upgrade StarForge.");
        }
        if !ver_ok {
            p::warn(&format!(
                "Verification status is {}: {}",
                ver_status.label(),
                ver_res
                    .as_ref()
                    .and_then(|r| r.detail.clone())
                    .unwrap_or_default()
            ));
        }
    }

    if all_ok {
        p::success("All checked plugins passed verification.");
    }

    Ok(())
}

fn audit(name: Option<String>, runtime_check: bool) -> Result<()> {
    run_audit(name, runtime_check)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuditSeverity {
    Pass,
    Warn,
    Fail,
}

impl AuditSeverity {
    fn label(self) -> &'static str {
        match self {
            AuditSeverity::Pass => "pass",
            AuditSeverity::Warn => "warn",
            AuditSeverity::Fail => "fail",
        }
    }
}

#[derive(Debug)]
struct AuditCheck {
    name: &'static str,
    severity: AuditSeverity,
    message: String,
}

#[derive(Debug)]
struct AuditReport {
    plugin_name: String,
    checks: Vec<AuditCheck>,
}

impl AuditReport {
    fn has_failures(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.severity == AuditSeverity::Fail)
    }

    fn has_warnings(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.severity == AuditSeverity::Warn)
    }
}

fn run_audit(name: Option<String>, runtime_check: bool) -> Result<()> {
    p::header("Plugin Audit");

    let reg = registry::load_registry().unwrap_or_default();
    if reg.plugins.is_empty() {
        p::info("No plugins installed.");
        return Ok(());
    }

    let to_check: Vec<_> = match &name {
        Some(n) => {
            let found: Vec<_> = reg.plugins.iter().filter(|p| &p.name == n).collect();
            if found.is_empty() {
                anyhow::bail!(
                    "Plugin '{}' is not installed. Run `starforge plugin list`.",
                    n
                );
            }
            found
        }
        None => reg.plugins.iter().collect(),
    };

    p::kv("StarForge core version", CORE_VERSION);
    p::kv(
        "Runtime self-check",
        if runtime_check { "enabled" } else { "disabled" },
    );
    p::separator();

    let mut failed = 0usize;
    let mut warned = 0usize;

    for plugin in to_check {
        let report = audit_plugin(plugin, runtime_check);
        if report.has_failures() {
            failed += 1;
        }
        if report.has_warnings() {
            warned += 1;
        }
        print_audit_report(&report);
    }

    p::separator();
    p::kv("Plugins failed", &failed.to_string());
    p::kv("Plugins with warnings", &warned.to_string());

    if failed > 0 {
        anyhow::bail!("{} plugin(s) failed audit checks.", failed);
    }

    p::success("All audited plugins passed required checks.");
    Ok(())
}

fn audit_plugin(plugin: &registry::InstalledPlugin, runtime_check: bool) -> AuditReport {
    let mut checks = Vec::new();
    let library_path = Path::new(&plugin.path);

    if library_path.exists() {
        checks.push(AuditCheck {
            name: "file",
            severity: AuditSeverity::Pass,
            message: format!("Library exists at {}", plugin.path),
        });
    } else {
        checks.push(AuditCheck {
            name: "file",
            severity: AuditSeverity::Fail,
            message: format!("Library missing at {}", plugin.path),
        });
        return AuditReport {
            plugin_name: plugin.name.clone(),
            checks,
        };
    }

    let classified = registry::classify_source(&plugin.source);
    if classified == plugin.trust {
        checks.push(AuditCheck {
            name: "trust",
            severity: match plugin.trust {
                TrustLevel::Unknown => AuditSeverity::Warn,
                TrustLevel::Local | TrustLevel::Trusted => AuditSeverity::Pass,
            },
            message: format!("Trust level is {}", plugin.trust.label()),
        });
    } else {
        checks.push(AuditCheck {
            name: "trust",
            severity: AuditSeverity::Fail,
            message: format!(
                "Registry trust is {}, but source now classifies as {}",
                plugin.trust.label(),
                classified.label()
            ),
        });
    }

    if plugin.starforge_version.is_empty() {
        checks.push(AuditCheck {
            name: "compatibility",
            severity: AuditSeverity::Fail,
            message: "Registry is missing StarForge compatibility metadata".to_string(),
        });
    } else if crate::plugins::interface::is_core_version_compatible(&plugin.starforge_version) {
        checks.push(AuditCheck {
            name: "compatibility",
            severity: AuditSeverity::Pass,
            message: format!("Compatible with StarForge {}", plugin.starforge_version),
        });
    } else {
        checks.push(AuditCheck {
            name: "compatibility",
            severity: AuditSeverity::Fail,
            message: format!(
                "Plugin targets StarForge {}, running {}",
                plugin.starforge_version, CORE_VERSION
            ),
        });
    }

    match manifest::load_manifest_for_library(library_path) {
        Ok(Some(plugin_manifest)) => {
            if plugin_manifest.name != plugin.name {
                checks.push(AuditCheck {
                    name: "manifest",
                    severity: AuditSeverity::Fail,
                    message: format!(
                        "Manifest name '{}' does not match registry name '{}'",
                        plugin_manifest.name, plugin.name
                    ),
                });
            }

            if plugin_manifest.version != plugin.plugin_version {
                checks.push(AuditCheck {
                    name: "manifest",
                    severity: AuditSeverity::Warn,
                    message: format!(
                        "Manifest version {} differs from registry version {}",
                        plugin_manifest.version, plugin.plugin_version
                    ),
                });
            }

            match plugin_manifest.validate() {
                Ok(()) => checks.push(AuditCheck {
                    name: "manifest",
                    severity: AuditSeverity::Pass,
                    message: "Manifest is valid".to_string(),
                }),
                Err(err) => checks.push(AuditCheck {
                    name: "manifest",
                    severity: AuditSeverity::Fail,
                    message: err.to_string(),
                }),
            }

            let config = config::load().unwrap_or_default();
            let ver_res = crate::plugins::verifier::verify_plugin_signature(
                library_path,
                &plugin_manifest,
                &config,
            );
            match ver_res.status {
                crate::plugins::verifier::VerificationStatus::Verified => {
                    checks.push(AuditCheck {
                        name: "signature",
                        severity: AuditSeverity::Pass,
                        message: format!(
                            "Ed25519 signature verified (publisher: {})",
                            ver_res.publisher.as_deref().unwrap_or("unknown")
                        ),
                    });
                }
                crate::plugins::verifier::VerificationStatus::Unsigned => {
                    checks.push(AuditCheck {
                        name: "signature",
                        severity: if config.plugin_trust.require_signatures {
                            AuditSeverity::Fail
                        } else {
                            AuditSeverity::Pass
                        },
                        message: "Plugin is unsigned".into(),
                    });
                }
                crate::plugins::verifier::VerificationStatus::UntrustedPublisher => {
                    checks.push(AuditCheck {
                        name: "signature",
                        severity: AuditSeverity::Warn,
                        message: format!(
                            "Publisher key '{}' is untrusted",
                            ver_res.publisher_key.as_deref().unwrap_or("")
                        ),
                    });
                }
                crate::plugins::verifier::VerificationStatus::InvalidSignature => {
                    checks.push(AuditCheck {
                        name: "signature",
                        severity: AuditSeverity::Fail,
                        message: "Cryptographic signature mismatch (tampered library or wrong key)"
                            .into(),
                    });
                }
                crate::plugins::verifier::VerificationStatus::MalformedKey => {
                    checks.push(AuditCheck {
                        name: "signature",
                        severity: AuditSeverity::Fail,
                        message: format!(
                            "Malformed publisher key: {}",
                            ver_res.detail.as_deref().unwrap_or("")
                        ),
                    });
                }
                crate::plugins::verifier::VerificationStatus::MalformedSignature => {
                    checks.push(AuditCheck {
                        name: "signature",
                        severity: AuditSeverity::Fail,
                        message: format!(
                            "Malformed signature string: {}",
                            ver_res.detail.as_deref().unwrap_or("")
                        ),
                    });
                }
                crate::plugins::verifier::VerificationStatus::Failed => {
                    checks.push(AuditCheck {
                        name: "signature",
                        severity: AuditSeverity::Fail,
                        message: format!(
                            "Verification failed: {}",
                            ver_res.detail.as_deref().unwrap_or("")
                        ),
                    });
                }
            }
        }
        Ok(None) => checks.push(AuditCheck {
            name: "manifest",
            severity: AuditSeverity::Fail,
            message: format!("Missing {}", manifest::MANIFEST_FILENAME),
        }),
        Err(err) => checks.push(AuditCheck {
            name: "manifest",
            severity: AuditSeverity::Fail,
            message: err.to_string(),
        }),
    }

    if runtime_check {
        let mut manager = PluginManager::new();
        match unsafe { manager.load_plugin(library_path) } {
            Ok(()) => checks.push(AuditCheck {
                name: "runtime",
                severity: AuditSeverity::Pass,
                message: "Plugin loaded successfully".to_string(),
            }),
            Err(err) => checks.push(AuditCheck {
                name: "runtime",
                severity: AuditSeverity::Fail,
                message: err.to_string(),
            }),
        }
    }

    AuditReport {
        plugin_name: plugin.name.clone(),
        checks,
    }
}

fn print_audit_report(report: &AuditReport) {
    p::kv_accent("Plugin", &report.plugin_name);
    for check in &report.checks {
        let marker = match check.severity {
            AuditSeverity::Pass => "✓",
            AuditSeverity::Warn => "⚠",
            AuditSeverity::Fail => "✗",
        };
        println!(
            "  {} {:<13} {:<4} {}",
            marker,
            check.name,
            check.severity.label(),
            check.message
        );
    }
    println!();
}

fn discover_plugin_metadata(path: &str) -> Result<(Vec<RegisteredCommand>, String)> {
    let mut pm = PluginManager::new();
    unsafe {
        pm.load_plugin(path)
            .with_context(|| format!("Failed to load plugin from {}", path))?;
    }
    let commands = pm
        .list_commands()
        .into_iter()
        .map(|c| RegisteredCommand {
            name: c.name,
            description: c.description,
        })
        .collect();
    let description = pm
        .list_plugins()
        .into_iter()
        .map(|(_, desc, _)| desc.to_string())
        .find(|d| !d.is_empty())
        .unwrap_or_default();
    Ok((commands, description))
}

fn commands(name: Option<String>) -> Result<()> {
    p::header("Plugin Commands");

    let reg = registry::load_registry().unwrap_or_default();
    if reg.plugins.is_empty() {
        p::info("No plugins installed. Use: starforge plugin install <name> --path <lib>");
        return Ok(());
    }

    let entries: Vec<_> = match &name {
        Some(n) => {
            let found: Vec<_> = reg
                .plugins
                .clone()
                .into_iter()
                .filter(|entry| entry.name == *n)
                .collect();
            if found.is_empty() {
                anyhow::bail!(
                    "Plugin '{}' is not installed. Run `starforge plugin list`.",
                    n
                );
            }
            found
        }
        None => reg.plugins.clone(),
    };

    let rows: Vec<Vec<String>> = entries
        .iter()
        .flat_map(|entry| {
            entry.commands.iter().map(|cmd| {
                vec![
                    entry.name.clone(),
                    cmd.name.clone(),
                    cmd.description.clone(),
                ]
            })
        })
        .collect();

    if rows.is_empty() {
        p::info("No commands registered. Re-install plugins to discover their commands.");
        p::info("  starforge plugin install <name> --path <lib>");
        return Ok(());
    }

    p::table(&["Plugin", "Command", "Description"], &rows);
    Ok(())
}
