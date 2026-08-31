//! `starforge feature-flags` — manage AI feature flags, rollouts, segments,
//! A/B variants, overrides, and rollback.
//!
//! Subcommand summary:
//!
//! - `list`                       – show every flag with current state + evaluation
//! - `show <name>`                – show detailed state for a flag
//! - `enable <name>` / `disable <name>`
//! - `rollout <name> --percent N` – set the global rollout percentage
//! - `segment add|remove|list`    – manage segment rules (allow-list, %,
//!   attribute predicate)
//! - `variant add|remove|list`    – manage A/B variants
//! - `override set|clear|list`    – per-user overrides
//! - `metrics show|prune [--days]`
//! - `history <name>`             – full state-version history for one flag
//! - `rollback <name> --to V`     – restore a prior state as a new version
//! - `reset <name>`               – drop history and start from category default
//!
//! All mutations bump the flag's `version`, so rollback targets always remain
//! in the history table for auditing.

use crate::utils::config;
use crate::utils::database::Database;
use crate::utils::feature_flags::{FlagCategory, FlagManager, SegmentRule, UserContext, Variant};
use crate::utils::print as p;
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use colored::*;
use comfy_table::{presets::UTF8_FULL, ContentArrangement, Table};

#[derive(Args, Debug)]
pub struct FeatureFlagsArgs {
    #[command(subcommand)]
    pub command: FeatureFlagsCommands,
}

#[derive(Subcommand, Debug)]
pub enum FeatureFlagsCommands {
    /// List every feature flag with its category, latest state, and
    /// evaluation outcome for *this* install.
    List {
        /// Emit machine-readable JSON instead of a table.
        #[arg(long)]
        json: bool,
        /// Show only flags whose category matches this slug.
        #[arg(long, value_parser = ["alpha", "beta", "stable", "experimental"])]
        category: Option<String>,
        /// Show only flags currently enabled for this install.
        #[arg(long)]
        enabled_only: bool,
    },

    /// Show deep details for a single flag (state, segments, variants,
    /// history, overrides, metrics).
    Show {
        flag_name: String,
        #[arg(long)]
        json: bool,
    },

    /// Enable a flag (sets `enabled = true`; keeps existing rollout / segments).
    Enable { flag_name: String },
    /// Disable a flag (sets `enabled = false`).
    Disable { flag_name: String },

    /// Set the gradual rollout percentage (0–100).
    Rollout {
        flag_name: String,
        #[arg(long, value_parser = 0..=100)]
        percent: u8,
    },

    /// Manage user-segment rules attached to a flag.
    #[command(subcommand)]
    Segment(SegmentCommands),

    /// Manage A/B variants on a flag.
    #[command(subcommand)]
    Variant(VariantCommands),

    /// Manage per-user overrides (alpha testers, forced kills).
    #[command(subcommand)]
    Override(OverrideCommands),

    /// Inspect or prune locally-stored flag metrics.
    #[command(subcommand)]
    Metrics(MetricsCommands),

    /// Show every recorded state version for a flag (oldest first).
    History { flag_name: String },

    /// Roll a flag back to a prior state; a new version is created so the
    /// audit trail is not destroyed.
    Rollback {
        flag_name: String,
        #[arg(long)]
        to: u32,
    },

    /// Reset a flag to its category default — wipes all state versions.
    Reset {
        flag_name: String,
        /// Skip the confirmation prompt.
        #[arg(long, short)]
        yes: bool,
    },

    /// Register a brand-new custom flag with a chosen category.
    Define {
        flag_name: String,
        #[arg(long, value_parser = ["alpha", "beta", "stable", "experimental"])]
        category: String,
        #[arg(long)]
        description: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum SegmentCommands {
    /// List the segment rules currently attached to a flag.
    List { flag_name: String },
    /// Append an allow-list rule (`--user id` can be repeated).
    AddAllowlist {
        flag_name: String,
        #[arg(long = "user")]
        users: Vec<String>,
    },
    /// Append a percentage rule.
    AddPercent {
        flag_name: String,
        #[arg(long)]
        percent: u8,
    },
    /// Append an attribute predicate (`--key X --any-of a,b`).
    AddAttribute {
        flag_name: String,
        #[arg(long)]
        key: String,
        #[arg(long, value_delimiter = ',')]
        any_of: Vec<String>,
    },
    /// Replace all segment rules with a single `always` rule (open rollout).
    Clear { flag_name: String },
}

#[derive(Subcommand, Debug)]
pub enum VariantCommands {
    /// List the A/B variants currently attached to a flag.
    List { flag_name: String },
    /// Add or replace a variant.
    Set {
        flag_name: String,
        /// Variant name (e.g. `control`, `treatment-a`).
        name: String,
        /// Integer weight; weights are normalised at evaluation.
        #[arg(long, default_value_t = 1)]
        weight: u32,
        /// Optional opaque payload (model id, prompt template, etc.).
        #[arg(long)]
        payload: Option<String>,
    },
    /// Remove a variant by name.
    Remove { flag_name: String, name: String },
    /// Drop every variant, falling back to boolean on/off.
    Clear { flag_name: String },
}

#[derive(Subcommand, Debug)]
pub enum OverrideCommands {
    /// List every override attached to a flag.
    List { flag_name: String },
    /// Force-enable or force-disable a flag for a specific user.
    Set {
        flag_name: String,
        /// Override the install UUID (defaults to the current install).
        #[arg(long)]
        user: Option<String>,
        /// Pass `--enable` to force on, `--disable` to force off.
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
        /// Optional pinned variant.
        #[arg(long)]
        variant: Option<String>,
    },
    /// Drop the override for a user, falling back to the global state.
    Clear {
        flag_name: String,
        #[arg(long)]
        user: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum MetricsCommands {
    /// Show aggregated counts (and the last N events) for a flag.
    Show {
        flag_name: String,
        #[arg(long, default_value_t = 10)]
        limit: u32,
        #[arg(long)]
        json: bool,
    },
    /// Delete metric rows older than `--days` (default configured value).
    Prune {
        #[arg(long)]
        days: Option<u32>,
        /// Skip confirmation.
        #[arg(long, short)]
        yes: bool,
    },
}

// ── Top-level handler ─────────────────────────────────────────────────────────

pub async fn handle(args: FeatureFlagsArgs) -> Result<()> {
    let db = Database::open().context("Failed to open starforge database")?;
    db.initialize().context("Failed to initialize database")?;
    let cfg = config::load().context("Failed to load starforge config")?;
    let install_id = crate::utils::feature_flags::load_or_create_install_id(&db)
        .context("Failed to resolve install_id")?;
    let user_ctx = if cfg.feature_flags.default_attributes.is_empty() {
        UserContext::new(install_id.clone())
    } else {
        UserContext {
            user_id: install_id.clone(),
            attributes: cfg.feature_flags.default_attributes.clone(),
        }
    };
    let mgr =
        FlagManager::new(&db, user_ctx).with_exposure_recording(cfg.feature_flags.metrics_enabled);

    match args.command {
        FeatureFlagsCommands::List {
            json,
            category,
            enabled_only,
        } => handle_list(&mgr, &db, category, enabled_only, json),
        FeatureFlagsCommands::Show { flag_name, json } => handle_show(&mgr, &db, &flag_name, json),
        FeatureFlagsCommands::Enable { flag_name } => {
            let s = mgr.set_enabled(&flag_name, true)?;
            p::success(&format!("Enabled '{}' (v{})", flag_name, s.version));
            Ok(())
        }
        FeatureFlagsCommands::Disable { flag_name } => {
            let s = mgr.set_enabled(&flag_name, false)?;
            p::success(&format!("Disabled '{}' (v{})", flag_name, s.version));
            Ok(())
        }
        FeatureFlagsCommands::Rollout { flag_name, percent } => {
            let s = mgr.set_rollout(&flag_name, percent)?;
            p::success(&format!(
                "'{}' rollout set to {}% (v{})",
                flag_name, s.rollout_percent, s.version
            ));
            Ok(())
        }
        FeatureFlagsCommands::Segment(cmd) => handle_segment(&mgr, cmd),
        FeatureFlagsCommands::Variant(cmd) => handle_variant(&mgr, cmd),
        FeatureFlagsCommands::Override(cmd) => handle_override(&mgr, &db, &install_id, cmd),
        FeatureFlagsCommands::Metrics(cmd) => handle_metrics(&mgr, &db, &cfg, cmd),
        FeatureFlagsCommands::History { flag_name } => handle_history(&db, &flag_name),
        FeatureFlagsCommands::Rollback { flag_name, to } => {
            let restored = mgr.rollback(&flag_name, to)?;
            p::success(&format!(
                "'{}' rolled back to v{} (created new v{})",
                flag_name, to, restored.version
            ));
            Ok(())
        }
        FeatureFlagsCommands::Reset { flag_name, yes } => {
            if !yes {
                bail!("Refusing to reset without --yes; add --yes to confirm.");
            }
            let state = mgr.reset(&flag_name)?;
            p::warn(&format!(
                "'{}' reset; current version is v{} (enabled={}, rollout={}%)",
                flag_name, state.version, state.enabled, state.rollout_percent
            ));
            Ok(())
        }
        FeatureFlagsCommands::Define {
            flag_name,
            category,
            description,
        } => {
            let cat = FlagCategory::parse(&category)
                .ok_or_else(|| anyhow::anyhow!("invalid category '{}'", category))?;
            let def = crate::utils::feature_flags::FlagDefinition {
                name: flag_name.clone(),
                category: cat,
                description,
                owner: None,
                user_manageable: true,
            };
            if mgr.register_flag(def.clone())? {
                p::success(&format!(
                    "Registered new flag '{}' in category {}",
                    flag_name,
                    cat.slug()
                ));
            } else {
                bail!("Flag '{}' already exists", flag_name);
            }
            Ok(())
        }
    }
}

// ── Helpers shared by subcommands ─────────────────────────────────────────────

// Not currently called from any code path in this crate. Kept rather than
// removed since deleting it is a product decision, not a lint-scoping one.
#[allow(dead_code)]
fn hydrate_state(mgr: &FlagManager, db: &Database, flag_name: &str) -> Result<()> {
    if db.get_definition(flag_name)?.is_none() {
        bail!(
            "Flag '{}' is not defined. Use `starforge feature-flags define <name> --category <cat>` first.",
            flag_name
        );
    }
    if db.latest_state(flag_name)?.is_none() {
        let _ = mgr.set_enabled(flag_name, false)?;
    }
    Ok(())
}

fn handle_list(
    mgr: &FlagManager,
    _db: &Database,
    category: Option<String>,
    enabled_only: bool,
    json: bool,
) -> Result<()> {
    let entries = mgr.list_all()?;
    let filtered: Vec<_> = entries
        .into_iter()
        .filter(|e| match &category {
            Some(c) => e.definition.category.slug() == c.as_str(),
            None => true,
        })
        .filter(|e| !enabled_only || e.evaluation.enabled)
        .collect();
    if json {
        let out = serde_json::to_string_pretty(&filtered)?;
        println!("{out}");
        return Ok(());
    }
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "Flag".to_string(),
            "Category".to_string(),
            "Rollout".to_string(),
            "State V".to_string(),
            "Enabled For You".to_string(),
            "Variant".to_string(),
            "Reason".to_string(),
        ]);
    for e in &filtered {
        let rollout = e
            .state
            .as_ref()
            .map(|s| format!("{}%", s.rollout_percent))
            .unwrap_or_else(|| "—".into());
        let version = e
            .state
            .as_ref()
            .map(|s| s.version.to_string())
            .unwrap_or_else(|| "—".into());
        let enabled_cell = if e.evaluation.enabled {
            "yes".green().bold().to_string()
        } else if e.evaluation.from_override {
            "override".yellow().to_string()
        } else {
            "no".dimmed().to_string()
        };
        let variant = e
            .evaluation
            .variant
            .clone()
            .unwrap_or_else(|| "—".to_string());
        table.add_row(vec![
            e.definition.name.clone(),
            e.definition.category.slug().to_string(),
            rollout,
            version,
            enabled_cell,
            variant,
            e.evaluation.reason.clone(),
        ]);
    }
    println!("{table}");
    Ok(())
}

fn handle_show(mgr: &FlagManager, db: &Database, flag_name: &str, json: bool) -> Result<()> {
    let def = db
        .get_definition(flag_name)?
        .ok_or_else(|| anyhow::anyhow!("flag '{}' is not defined", flag_name))?;
    let state = db.latest_state(flag_name)?;
    let eval = mgr.evaluate_dry(flag_name)?;
    let history = db.state_history(flag_name)?;
    let overrides = db.list_overrides(flag_name)?;
    let metrics = db.metrics_summary(flag_name)?;
    if json {
        let out = serde_json::json!({
            "definition": def,
            "state": state,
            "evaluation": eval,
            "history": history,
            "overrides": overrides,
            "metrics": metrics,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    p::header(&format!("Flag {}", def.name));
    p::kv("Category", def.category.slug());
    p::kv("Description", &def.description);
    if let Some(o) = &def.owner {
        p::kv("Owner", o);
    }
    p::kv(
        "User-manageable",
        if def.user_manageable { "yes" } else { "no" },
    );
    p::separator();

    if let Some(s) = &state {
        p::kv(
            "Latest State (v)",
            &format!(
                "enabled={}, rollout={}%, note=\"{}\"",
                s.enabled, s.rollout_percent, s.note
            ),
        );
        p::kv("Created at", &s.created_at);
        if s.segments.is_empty() {
            p::info("Segments: (none — open rollout)");
        } else {
            println!();
            println!("{}", "Segments:".bold());
            for r in &s.segments {
                println!("  • {}", format_rule(r));
            }
        }
        if s.variants.is_empty() {
            p::info("Variants: (none)");
        } else {
            println!();
            println!("{}", "Variants:".bold());
            for v in &s.variants {
                println!(
                    "  • {} (weight {}){}",
                    v.name,
                    v.weight,
                    v.payload
                        .as_ref()
                        .map(|p| format!(" [payload: {p}]"))
                        .unwrap_or_default()
                );
            }
        }
    } else {
        p::warn("No state committed yet — using category default.");
    }
    p::separator();
    p::kv(
        "Your evaluation",
        &format!("enabled={}, reason={}", eval.enabled, eval.reason),
    );
    if let Some(v) = &eval.variant {
        p::kv("Your variant", v);
    }
    p::separator();

    if !overrides.is_empty() {
        println!("{}", "Overrides:".bold());
        for ov in &overrides {
            println!(
                "  • {} -> {} (variant: {})",
                ov.user_id,
                if ov.enabled {
                    "on".green()
                } else {
                    "off".red()
                },
                ov.variant.as_deref().unwrap_or("—")
            );
        }
    } else {
        p::info("Overrides: (none)");
    }
    p::separator();
    println!("{}", "Metrics summary:".bold());
    if metrics.is_empty() {
        p::info("  (none recorded)");
    } else {
        for (k, v) in &metrics {
            println!("  • {} = {}", k, v);
        }
    }
    p::separator();

    println!(
        "{} ({} prior versions)",
        "History".bold(),
        history.len().saturating_sub(1)
    );
    for h in &history {
        println!(
            "  v{} · {} · enabled={} rollout={}% \u{2014} {}",
            h.version, h.created_at, h.enabled, h.rollout_percent, h.note
        );
    }
    Ok(())
}

/// Ensures `flag_name` has a persisted definition and an initial state row.
///
/// Built-in flags are seeded lazily — the definition only reaches the database
/// the first time a command touches the flag — so the segment, variant, and
/// override subcommands hydrate it before reading `latest_state`.
fn hydrate_definition(db: &Database, flag_name: &str) -> Result<()> {
    let def = match db.get_definition(flag_name)? {
        Some(def) => def,
        None => crate::utils::feature_flags::builtin_definitions()
            .into_iter()
            .find(|d| d.name == flag_name)
            .ok_or_else(|| anyhow::anyhow!("unknown feature flag '{}'", flag_name))?,
    };
    // `upsert_definition` also seeds the initial state row when it is missing.
    db.upsert_definition(&def)?;
    Ok(())
}

fn handle_segment(mgr: &FlagManager, cmd: SegmentCommands) -> Result<()> {
    match cmd {
        SegmentCommands::List { flag_name } => {
            hydrate_definition(mgr.db(), &flag_name)?;
            let state = mgr
                .db()
                .latest_state(&flag_name)?
                .ok_or_else(|| anyhow::anyhow!("flag has no state"))?;
            if state.segments.is_empty() {
                p::info("(no segments — open rollout)");
            } else {
                for r in &state.segments {
                    println!("  • {}", format_rule(r));
                }
            }
            Ok(())
        }
        SegmentCommands::AddAllowlist { flag_name, users } => {
            hydrate_definition(mgr.db(), &flag_name)?;
            let mut current: Vec<SegmentRule> = mgr
                .db()
                .latest_state(&flag_name)?
                .map(|s| s.segments)
                .unwrap_or_default();
            let rule = SegmentRule::UserInList { user_ids: users };
            current.push(rule);
            let s = mgr.replace_segments(&flag_name, current)?;
            p::success(&format!(
                "Added allowlist rule to '{}' (now v{})",
                flag_name, s.version
            ));
            Ok(())
        }
        SegmentCommands::AddPercent { flag_name, percent } => {
            hydrate_definition(mgr.db(), &flag_name)?;
            let mut current: Vec<SegmentRule> = mgr
                .db()
                .latest_state(&flag_name)?
                .map(|s| s.segments)
                .unwrap_or_default();
            current.push(SegmentRule::PercentOfUsers {
                percent: percent.min(100),
            });
            let s = mgr.replace_segments(&flag_name, current)?;
            p::success(&format!(
                "Added {percent}% segment to '{}' (now v{})",
                flag_name, s.version
            ));
            Ok(())
        }
        SegmentCommands::AddAttribute {
            flag_name,
            key,
            any_of,
        } => {
            hydrate_definition(mgr.db(), &flag_name)?;
            let mut current: Vec<SegmentRule> = mgr
                .db()
                .latest_state(&flag_name)?
                .map(|s| s.segments)
                .unwrap_or_default();
            current.push(SegmentRule::HasAttribute { key, any_of });
            let s = mgr.replace_segments(&flag_name, current)?;
            p::success(&format!(
                "Added attribute rule to '{}' (now v{})",
                flag_name, s.version
            ));
            Ok(())
        }
        SegmentCommands::Clear { flag_name } => {
            hydrate_definition(mgr.db(), &flag_name)?;
            let s = mgr.replace_segments(&flag_name, Vec::new())?;
            p::success(&format!(
                "Cleared segments on '{}' (now v{})",
                flag_name, s.version
            ));
            Ok(())
        }
    }
}

fn handle_variant(mgr: &FlagManager, cmd: VariantCommands) -> Result<()> {
    match cmd {
        VariantCommands::List { flag_name } => {
            hydrate_definition(mgr.db(), &flag_name)?;
            let state = mgr
                .db()
                .latest_state(&flag_name)?
                .ok_or_else(|| anyhow::anyhow!("flag has no state"))?;
            if state.variants.is_empty() {
                p::info("(no variants)");
            } else {
                for v in &state.variants {
                    println!(
                        "  • {} (weight {})\u{2003}{}",
                        v.name,
                        v.weight,
                        v.payload
                            .as_ref()
                            .map(|p| format!("[payload: {p}]"))
                            .unwrap_or_default()
                    );
                }
            }
            Ok(())
        }
        VariantCommands::Set {
            flag_name,
            name,
            weight,
            payload,
        } => {
            hydrate_definition(mgr.db(), &flag_name)?;
            let mut current: Vec<Variant> = mgr
                .db()
                .latest_state(&flag_name)?
                .map(|s| s.variants)
                .unwrap_or_default();
            let mut found = false;
            for v in current.iter_mut() {
                if v.name == name {
                    v.weight = weight;
                    v.payload = payload.clone();
                    found = true;
                }
            }
            if !found {
                current.push(Variant {
                    name,
                    weight,
                    payload,
                });
            }
            let s = mgr.replace_variants(&flag_name, current)?;
            p::success(&format!(
                "Updated variants for '{}' (now v{})",
                flag_name, s.version
            ));
            Ok(())
        }
        VariantCommands::Remove { flag_name, name } => {
            hydrate_definition(mgr.db(), &flag_name)?;
            let mut current: Vec<Variant> = mgr
                .db()
                .latest_state(&flag_name)?
                .map(|s| s.variants)
                .unwrap_or_default();
            let before = current.len();
            current.retain(|v| v.name != name);
            if current.len() == before {
                bail!("variant '{}' not found on flag '{}'", name, flag_name);
            }
            let s = mgr.replace_variants(&flag_name, current)?;
            p::success(&format!(
                "Removed variant '{}' from '{}' (now v{})",
                name, flag_name, s.version
            ));
            Ok(())
        }
        VariantCommands::Clear { flag_name } => {
            hydrate_definition(mgr.db(), &flag_name)?;
            let s = mgr.replace_variants(&flag_name, Vec::new())?;
            p::success(&format!(
                "Cleared variants on '{}' (now v{})",
                flag_name, s.version
            ));
            Ok(())
        }
    }
}

fn handle_override(
    mgr: &FlagManager,
    db: &Database,
    install_id: &str,
    cmd: OverrideCommands,
) -> Result<()> {
    match cmd {
        OverrideCommands::List { flag_name } => {
            hydrate_definition(db, &flag_name)?;
            let overrides = db.list_overrides(&flag_name)?;
            if overrides.is_empty() {
                p::info("(no overrides)");
            } else {
                for ov in &overrides {
                    println!(
                        "  • {} -> {} (variant: {}) [{}]",
                        ov.user_id,
                        if ov.enabled {
                            "on".green()
                        } else {
                            "off".red()
                        },
                        ov.variant.as_deref().unwrap_or("\u{2014}"),
                        ov.created_at
                    );
                }
            }
            Ok(())
        }
        OverrideCommands::Set {
            flag_name,
            user,
            enable,
            disable,
            variant,
        } => {
            hydrate_definition(db, &flag_name)?;
            if !enable && !disable {
                bail!("specify either --enable or --disable");
            }
            let user_id = user.unwrap_or_else(|| install_id.to_string());
            let enabled = enable;
            let ov = mgr.set_override(&flag_name, &user_id, enabled, variant.clone())?;
            p::success(&format!(
                "Override set for '{}': user={} enabled={} variant={}",
                ov.flag_name,
                ov.user_id,
                ov.enabled,
                ov.variant.as_deref().unwrap_or("\u{2014}")
            ));
            Ok(())
        }
        OverrideCommands::Clear { flag_name, user } => {
            hydrate_definition(db, &flag_name)?;
            let user_id = user.unwrap_or_else(|| install_id.to_string());
            if mgr.clear_override(&flag_name, &user_id)? {
                p::success(&format!(
                    "Override cleared for '{}' on user {}",
                    flag_name, user_id
                ));
            } else {
                p::info("(no override to clear)");
            }
            Ok(())
        }
    }
}

fn handle_metrics(
    _mgr: &FlagManager,
    db: &Database,
    cfg: &config::Config,
    cmd: MetricsCommands,
) -> Result<()> {
    match cmd {
        MetricsCommands::Show {
            flag_name,
            limit,
            json,
        } => {
            let summary = db.metrics_summary(&flag_name)?;
            let recent = db.metrics_recent(&flag_name, limit)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "flag": flag_name,
                        "summary": summary,
                        "recent": recent,
                    }))?
                );
                return Ok(());
            }
            p::header(&format!("Metrics for '{}'", flag_name));
            if summary.is_empty() {
                p::info("(no metrics)");
                return Ok(());
            }
            for (k, v) in &summary {
                println!("  {} = {}", k, v);
            }
            println!();
            println!("Recent events:");
            for e in &recent {
                let ctx_str = if e.context.is_empty() {
                    String::new()
                } else {
                    format!(" ctx={:?}", e.context)
                };
                println!(
                    "  [{}] {} variant={}{}",
                    e.timestamp,
                    e.event_type.slug(),
                    e.variant.as_deref().unwrap_or("\u{2014}"),
                    ctx_str
                );
            }
            Ok(())
        }
        MetricsCommands::Prune { days, yes } => {
            let cut = days.unwrap_or(cfg.feature_flags.metrics_retention_days.max(1));
            if !yes {
                bail!(
                    "Refusing to prune without --yes. Add --yes to drop metrics older than {} day(s).",
                    cut
                );
            }
            let n = db.prune_metrics(cut)?;
            p::success(&format!(
                "Pruned {} metric row(s) older than {} day(s)",
                n, cut
            ));
            Ok(())
        }
    }
}

fn handle_history(db: &Database, flag_name: &str) -> Result<()> {
    if db.get_definition(flag_name)?.is_none() {
        bail!("flag '{}' is not defined", flag_name);
    }
    let history = db.state_history(flag_name)?;
    if history.is_empty() {
        p::info("(no history)");
        return Ok(());
    }
    let mut table = Table::new();
    table.load_preset(UTF8_FULL).set_header(vec![
        "Version".to_string(),
        "Enabled".to_string(),
        "Rollout %".to_string(),
        "Note".to_string(),
        "Created at".to_string(),
    ]);
    for h in &history {
        table.add_row(vec![
            format!("v{}", h.version),
            if h.enabled {
                "yes".green().to_string()
            } else {
                "no".red().to_string()
            },
            format!("{}%", h.rollout_percent),
            h.note.clone(),
            h.created_at.clone(),
        ]);
    }
    println!("{table}");
    Ok(())
}

/// Convenience helper called by other AI commands at startup. Returns
/// `Ok(())` if the feature is enabled for this install, otherwise returns
/// `Err` with a user-friendly message explaining how to roll it out.
///
/// Operators may set `STARFORGE_DISABLE_FLAGS=1` to disable the gate
/// entirely; this is the documented escape hatch for users on a broken DB
/// or who want hard-on access without DB writes.
///
/// Uses a single SQLite connection to avoid the concurrent-writer race
/// between this DB and the one opened by `config::load()`.
pub fn require_feature(flag_name: &str) -> Result<()> {
    if std::env::var("STARFORGE_DISABLE_FLAGS").ok().as_deref() == Some("1") {
        return Ok(());
    }
    let db = Database::open().context("Failed to open starforge database")?;
    db.initialize()
        .context("Failed to initialize starforge database")?;
    // Read both install_id and the metrics setting from the same connection
    // so we never see a stale or empty install_id during a concurrent
    // writer's insert window.
    let install_id = db
        .get_config_kv("install_id")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "anonymous".to_string());
    let metrics_on = db
        .get_config_kv("feature_flags")
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<config::FeatureFlagsConfig>(&s).ok())
        .map(|c| c.metrics_enabled)
        .unwrap_or(true);
    let default_attributes = db
        .get_config_kv("feature_flags")
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<config::FeatureFlagsConfig>(&s).ok())
        .map(|c| c.default_attributes)
        .unwrap_or_default();
    let ctx = UserContext {
        user_id: install_id,
        attributes: default_attributes,
    };
    let mgr = FlagManager::new(&db, ctx).with_exposure_recording(metrics_on);
    match mgr.evaluate(flag_name) {
        Ok(res) if res.enabled => Ok(()),
        Ok(res) => bail!(
            "Feature '{}' is not enabled for this install.\n  Reason: {}\n  \
             Enable it with: `starforge feature-flags enable {}`\n  \
             Or set a rollout: `starforge feature-flags rollout {} --percent 25`\n  \
             Hard-bypass (DB-free): STARFORGE_DISABLE_FLAGS=1",
            flag_name,
            res.reason,
            flag_name,
            flag_name
        ),
        Err(e) => Err(e),
    }
}

fn format_rule(rule: &SegmentRule) -> String {
    match rule {
        SegmentRule::Always => "always".to_string(),
        SegmentRule::UserInList { user_ids } => {
            format!("user_in_list({})", user_ids.join(", "))
        }
        SegmentRule::PercentOfUsers { percent } => {
            format!("percent_of_users({}%)", percent)
        }
        SegmentRule::HasAttribute { key, any_of } => {
            if any_of.is_empty() {
                format!("has_attribute({})", key)
            } else {
                format!("has_attribute({} ∈ {{{}}})", key, any_of.join(", "))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::feature_flags::MetricKind;

    #[test]
    fn format_rule_user_in_list() {
        let r = SegmentRule::UserInList {
            user_ids: vec!["a".into(), "b".into()],
        };
        assert!(format_rule(&r).contains("a, b"));
    }

    #[test]
    fn format_rule_percent_of_users() {
        let r = SegmentRule::PercentOfUsers { percent: 25 };
        assert_eq!(format_rule(&r), "percent_of_users(25%)");
    }

    #[test]
    fn format_rule_attribute_with_values() {
        let r = SegmentRule::HasAttribute {
            key: "team".into(),
            any_of: vec!["x".into(), "y".into()],
        };
        let txt = format_rule(&r);
        assert!(txt.contains("team"));
        assert!(txt.contains("x"));
        assert!(txt.contains("y"));
    }

    #[test]
    fn format_rule_attribute_without_values() {
        let r = SegmentRule::HasAttribute {
            key: "team".into(),
            any_of: vec![],
        };
        assert_eq!(format_rule(&r), "has_attribute(team)");
    }

    #[test]
    fn format_rule_always() {
        assert_eq!(format_rule(&SegmentRule::Always), "always");
    }

    #[test]
    fn metric_kind_roundtrips_parsed_slugs() {
        for k in [
            MetricKind::Exposure,
            MetricKind::Conversion,
            MetricKind::Rejection,
        ] {
            assert!(MetricKind::parse(k.slug()).is_some());
        }
    }
}
