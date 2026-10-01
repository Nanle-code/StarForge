//! CLI Migration Notes Generator
//!
//! Generates markdown migration notes from a version range or a pre-computed
//! diff summary. The generator identifies renamed CLI commands/flags and config
//! key changes by parsing the structured before/after snapshots that a caller
//! provides (captured from real source), then formats them as a ready-to-review
//! markdown document.
//!
//! # Human-review banner
//!
//! **Every** document produced by this module begins with a clearly visible
//! human-review banner. This is unconditional — there is no flag or option
//! that can suppress it. See `HUMAN_REVIEW_BANNER`.
//!
//! # Integration points
//!
//! - CLI subcommand: `starforge migrate-notes`  (`src/commands/migrate_notes.rs`)
//! - Fixture tests:  `tests/cli_migration_notes.rs`
//! - Maintainer docs: `docs/MIGRATION_NOTES_WORKFLOW.md`

use serde::{Deserialize, Serialize};

// ── Human-review banner ───────────────────────────────────────────────────────

/// The mandatory banner prepended to every AI-generated migration notes
/// document.  It is a compile-time constant so no code path can omit it.
pub const HUMAN_REVIEW_BANNER: &str = "\
> ⚠️  **AI-generated migration notes — review for accuracy before publishing.**
> This document was produced automatically from a diff and may contain
> errors or miss context. A human maintainer **must** review, edit, and
> approve this document before it is published in a CHANGELOG, GitHub
> release, or docs site.
";

// ── Input types ───────────────────────────────────────────────────────────────

/// A CLI command or subcommand definition captured from a specific version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CliCommandSnapshot {
    /// The full command path as the user types it (e.g. `"wallet create"`).
    pub command: String,
    /// Short description shown in `--help` output.
    pub description: String,
    /// All flags/arguments this command accepts.
    pub flags: Vec<CliFlag>,
}

/// A single CLI flag or argument.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CliFlag {
    /// Flag name without leading dashes (e.g. `"network"` → `--network`).
    pub name: String,
    /// Short alias without the dash, if any (e.g. `"n"` → `-n`).
    pub short: Option<String>,
    /// Whether this flag is required.
    pub required: bool,
    /// Default value, if any.
    pub default: Option<String>,
    /// Human-readable description.
    pub description: String,
}

/// A configuration key captured from a specific version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfigKeySnapshot {
    /// Dot-separated key path (e.g. `"network.default"`).
    pub key: String,
    /// Value type hint (e.g. `"string"`, `"bool"`, `"u16"`).
    pub value_type: String,
    /// Default value as a string.
    pub default: Option<String>,
    /// Human-readable description.
    pub description: String,
}

/// A full snapshot of the CLI surface and config schema at one point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliSnapshot {
    /// Version label (git tag, semver, or ref name).
    pub version: String,
    /// All top-level and subcommands in this version.
    pub commands: Vec<CliCommandSnapshot>,
    /// All configuration keys in this version.
    pub config_keys: Vec<ConfigKeySnapshot>,
}

// ── Diff types ────────────────────────────────────────────────────────────────

/// A single change detected between two CLI snapshots.
#[derive(Debug, Clone, PartialEq)]
pub enum CliChange {
    /// A command was renamed from `old` to `new`.
    CommandRenamed { old: String, new: String },
    /// A command was removed with no replacement.
    CommandRemoved { command: String, description: String },
    /// A command was added.
    CommandAdded { command: String, description: String },
    /// A required flag was renamed on a specific command.
    FlagRenamed {
        command: String,
        old_flag: String,
        new_flag: String,
    },
    /// A required flag was removed from a command.
    FlagRemoved {
        command: String,
        flag: String,
        description: String,
    },
    /// A previously optional flag became required.
    FlagBecameRequired {
        command: String,
        flag: String,
        description: String,
    },
    /// A config key was renamed.
    ConfigKeyRenamed { old: String, new: String },
    /// A config key was removed.
    ConfigKeyRemoved { key: String, description: String },
    /// A config key was added.
    ConfigKeyAdded {
        key: String,
        value_type: String,
        default: Option<String>,
        description: String,
    },
    /// A config key's type changed.
    ConfigKeyTypeChanged {
        key: String,
        old_type: String,
        new_type: String,
    },
}

// ── Diff engine ───────────────────────────────────────────────────────────────

/// Diff two CLI snapshots and return an ordered list of [`CliChange`]s.
///
/// Rename detection heuristic: if a command present in `before` is absent in
/// `after`, and a command present in `after` is absent in `before`, and their
/// `description` values are identical, the pair is treated as a rename rather
/// than a remove+add. This avoids false positives for genuinely new/removed
/// commands.
pub fn diff_snapshots(before: &CliSnapshot, after: &CliSnapshot) -> Vec<CliChange> {
    let mut changes = Vec::new();

    // ── Command-level diff ────────────────────────────────────────────────────

    let before_cmds: std::collections::HashMap<&str, &CliCommandSnapshot> =
        before.commands.iter().map(|c| (c.command.as_str(), c)).collect();
    let after_cmds: std::collections::HashMap<&str, &CliCommandSnapshot> =
        after.commands.iter().map(|c| (c.command.as_str(), c)).collect();

    // Find candidates: commands removed in before and added in after.
    let removed_cmds: Vec<&CliCommandSnapshot> = before
        .commands
        .iter()
        .filter(|c| !after_cmds.contains_key(c.command.as_str()))
        .collect();

    let added_cmds: Vec<&CliCommandSnapshot> = after
        .commands
        .iter()
        .filter(|c| !before_cmds.contains_key(c.command.as_str()))
        .collect();

    // Pair by identical description (rename heuristic).
    let mut rename_matched_old: std::collections::HashSet<&str> =
        std::collections::HashSet::new();
    let mut rename_matched_new: std::collections::HashSet<&str> =
        std::collections::HashSet::new();

    for old_cmd in &removed_cmds {
        if let Some(new_cmd) = added_cmds
            .iter()
            .find(|a| a.description == old_cmd.description && !rename_matched_new.contains(a.command.as_str()))
        {
            changes.push(CliChange::CommandRenamed {
                old: old_cmd.command.clone(),
                new: new_cmd.command.clone(),
            });
            rename_matched_old.insert(&old_cmd.command);
            rename_matched_new.insert(&new_cmd.command);
        }
    }

    // Remaining removes/adds.
    for cmd in &removed_cmds {
        if !rename_matched_old.contains(cmd.command.as_str()) {
            changes.push(CliChange::CommandRemoved {
                command: cmd.command.clone(),
                description: cmd.description.clone(),
            });
        }
    }
    for cmd in &added_cmds {
        if !rename_matched_new.contains(cmd.command.as_str()) {
            changes.push(CliChange::CommandAdded {
                command: cmd.command.clone(),
                description: cmd.description.clone(),
            });
        }
    }

    // ── Flag-level diff (for commands present in both versions) ───────────────

    for (cmd_name, before_cmd) in &before_cmds {
        if let Some(after_cmd) = after_cmds.get(cmd_name) {
            diff_flags(cmd_name, before_cmd, after_cmd, &mut changes);
        }
    }

    // ── Config key diff ───────────────────────────────────────────────────────

    let before_keys: std::collections::HashMap<&str, &ConfigKeySnapshot> =
        before.config_keys.iter().map(|k| (k.key.as_str(), k)).collect();
    let after_keys: std::collections::HashMap<&str, &ConfigKeySnapshot> =
        after.config_keys.iter().map(|k| (k.key.as_str(), k)).collect();

    let removed_keys: Vec<&ConfigKeySnapshot> = before
        .config_keys
        .iter()
        .filter(|k| !after_keys.contains_key(k.key.as_str()))
        .collect();

    let added_keys: Vec<&ConfigKeySnapshot> = after
        .config_keys
        .iter()
        .filter(|k| !before_keys.contains_key(k.key.as_str()))
        .collect();

    // Rename detection for config keys (same type + description heuristic).
    let mut key_rename_old: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut key_rename_new: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for old_key in &removed_keys {
        if let Some(new_key) = added_keys.iter().find(|a| {
            a.value_type == old_key.value_type
                && a.description == old_key.description
                && !key_rename_new.contains(a.key.as_str())
        }) {
            changes.push(CliChange::ConfigKeyRenamed {
                old: old_key.key.clone(),
                new: new_key.key.clone(),
            });
            key_rename_old.insert(&old_key.key);
            key_rename_new.insert(&new_key.key);
        }
    }

    for key in &removed_keys {
        if !key_rename_old.contains(key.key.as_str()) {
            changes.push(CliChange::ConfigKeyRemoved {
                key: key.key.clone(),
                description: key.description.clone(),
            });
        }
    }
    for key in &added_keys {
        if !key_rename_new.contains(key.key.as_str()) {
            changes.push(CliChange::ConfigKeyAdded {
                key: key.key.clone(),
                value_type: key.value_type.clone(),
                default: key.default.clone(),
                description: key.description.clone(),
            });
        }
    }

    // Type changes for keys present in both versions.
    for (key_name, before_key) in &before_keys {
        if let Some(after_key) = after_keys.get(key_name) {
            if before_key.value_type != after_key.value_type {
                changes.push(CliChange::ConfigKeyTypeChanged {
                    key: (*key_name).to_string(),
                    old_type: before_key.value_type.clone(),
                    new_type: after_key.value_type.clone(),
                });
            }
        }
    }

    changes
}

/// Diff flags for a single command that exists in both versions.
fn diff_flags(
    cmd_name: &str,
    before_cmd: &CliCommandSnapshot,
    after_cmd: &CliCommandSnapshot,
    changes: &mut Vec<CliChange>,
) {
    let before_flags: std::collections::HashMap<&str, &CliFlag> =
        before_cmd.flags.iter().map(|f| (f.name.as_str(), f)).collect();
    let after_flags: std::collections::HashMap<&str, &CliFlag> =
        after_cmd.flags.iter().map(|f| (f.name.as_str(), f)).collect();

    let removed_flags: Vec<&CliFlag> = before_cmd
        .flags
        .iter()
        .filter(|f| !after_flags.contains_key(f.name.as_str()))
        .collect();
    let added_flags: Vec<&CliFlag> = after_cmd
        .flags
        .iter()
        .filter(|f| !before_flags.contains_key(f.name.as_str()))
        .collect();

    // Rename: same description, different name.
    let mut flag_rename_old: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut flag_rename_new: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for old_flag in &removed_flags {
        if let Some(new_flag) = added_flags.iter().find(|a| {
            a.description == old_flag.description && !flag_rename_new.contains(a.name.as_str())
        }) {
            changes.push(CliChange::FlagRenamed {
                command: cmd_name.to_string(),
                old_flag: old_flag.name.clone(),
                new_flag: new_flag.name.clone(),
            });
            flag_rename_old.insert(&old_flag.name);
            flag_rename_new.insert(&new_flag.name);
        }
    }

    for flag in &removed_flags {
        if !flag_rename_old.contains(flag.name.as_str()) {
            changes.push(CliChange::FlagRemoved {
                command: cmd_name.to_string(),
                flag: flag.name.clone(),
                description: flag.description.clone(),
            });
        }
    }

    // Required-ness changes for flags present in both versions.
    for (flag_name, before_flag) in &before_flags {
        if let Some(after_flag) = after_flags.get(flag_name) {
            if !before_flag.required && after_flag.required {
                changes.push(CliChange::FlagBecameRequired {
                    command: cmd_name.to_string(),
                    flag: (*flag_name).to_string(),
                    description: after_flag.description.clone(),
                });
            }
        }
    }
}

// ── Markdown renderer ─────────────────────────────────────────────────────────

/// Render migration notes as a markdown string.
///
/// The output **always** starts with `HUMAN_REVIEW_BANNER`.  This function is
/// the single choke-point — all callers (CLI command, tests, release scripts)
/// go through here, ensuring the banner cannot be accidentally dropped.
pub fn render_migration_notes(
    from_version: &str,
    to_version: &str,
    changes: &[CliChange],
) -> String {
    let mut doc = String::new();

    // ── Unconditional human-review banner ─────────────────────────────────────
    doc.push_str(HUMAN_REVIEW_BANNER);
    doc.push('\n');

    // ── Document header ───────────────────────────────────────────────────────
    doc.push_str(&format!(
        "# Migration Notes: {} → {}\n\n",
        from_version, to_version
    ));
    doc.push_str(&format!(
        "_Generated: {}_\n\n",
        chrono::Utc::now().format("%Y-%m-%d")
    ));

    if changes.is_empty() {
        doc.push_str("No breaking changes detected between these versions.\n");
        return doc;
    }

    // ── Separate changes into categories ─────────────────────────────────────
    let cmd_renames: Vec<_> = changes
        .iter()
        .filter_map(|c| {
            if let CliChange::CommandRenamed { old, new } = c {
                Some((old, new))
            } else {
                None
            }
        })
        .collect();

    let cmd_removed: Vec<_> = changes
        .iter()
        .filter_map(|c| {
            if let CliChange::CommandRemoved { command, description } = c {
                Some((command, description))
            } else {
                None
            }
        })
        .collect();

    let flag_renames: Vec<_> = changes
        .iter()
        .filter_map(|c| {
            if let CliChange::FlagRenamed { command, old_flag, new_flag } = c {
                Some((command, old_flag, new_flag))
            } else {
                None
            }
        })
        .collect();

    let flag_removed: Vec<_> = changes
        .iter()
        .filter_map(|c| {
            if let CliChange::FlagRemoved { command, flag, description } = c {
                Some((command, flag, description))
            } else {
                None
            }
        })
        .collect();

    let flag_required: Vec<_> = changes
        .iter()
        .filter_map(|c| {
            if let CliChange::FlagBecameRequired { command, flag, description } = c {
                Some((command, flag, description))
            } else {
                None
            }
        })
        .collect();

    let key_renames: Vec<_> = changes
        .iter()
        .filter_map(|c| {
            if let CliChange::ConfigKeyRenamed { old, new } = c {
                Some((old, new))
            } else {
                None
            }
        })
        .collect();

    let key_removed: Vec<_> = changes
        .iter()
        .filter_map(|c| {
            if let CliChange::ConfigKeyRemoved { key, description } = c {
                Some((key, description))
            } else {
                None
            }
        })
        .collect();

    let key_type_changes: Vec<_> = changes
        .iter()
        .filter_map(|c| {
            if let CliChange::ConfigKeyTypeChanged { key, old_type, new_type } = c {
                Some((key, old_type, new_type))
            } else {
                None
            }
        })
        .collect();

    // ── Section: Renamed Commands ─────────────────────────────────────────────
    if !cmd_renames.is_empty() {
        doc.push_str("## Renamed Commands\n\n");
        doc.push_str("Update any scripts, aliases, or CI pipelines that reference these commands.\n\n");
        doc.push_str("| Before | After |\n");
        doc.push_str("|--------|-------|\n");
        for (old, new) in &cmd_renames {
            doc.push_str(&format!("| `starforge {}` | `starforge {}` |\n", old, new));
        }
        doc.push('\n');
    }

    // ── Section: Removed Commands ─────────────────────────────────────────────
    if !cmd_removed.is_empty() {
        doc.push_str("## Removed Commands\n\n");
        doc.push_str("These commands have been removed with no direct replacement.\n\n");
        for (cmd, desc) in &cmd_removed {
            doc.push_str(&format!("- `starforge {}` — {}\n", cmd, desc));
        }
        doc.push('\n');
    }

    // ── Section: Renamed Flags ────────────────────────────────────────────────
    if !flag_renames.is_empty() {
        doc.push_str("## Renamed Flags\n\n");
        doc.push_str("| Command | Old Flag | New Flag |\n");
        doc.push_str("|---------|----------|----------|\n");
        for (cmd, old_flag, new_flag) in &flag_renames {
            doc.push_str(&format!(
                "| `starforge {}` | `--{}` | `--{}` |\n",
                cmd, old_flag, new_flag
            ));
        }
        doc.push('\n');
    }

    // ── Section: Removed Flags ────────────────────────────────────────────────
    if !flag_removed.is_empty() {
        doc.push_str("## Removed Flags\n\n");
        for (cmd, flag, desc) in &flag_removed {
            doc.push_str(&format!(
                "- `starforge {} --{}` — {} (removed; no replacement)\n",
                cmd, flag, desc
            ));
        }
        doc.push('\n');
    }

    // ── Section: Flags That Became Required ───────────────────────────────────
    if !flag_required.is_empty() {
        doc.push_str("## Flags That Became Required\n\n");
        doc.push_str("These flags were optional before and are now required.\n\n");
        for (cmd, flag, desc) in &flag_required {
            doc.push_str(&format!(
                "- `starforge {} --{}` — {} (now required)\n",
                cmd, flag, desc
            ));
        }
        doc.push('\n');
    }

    // ── Section: Config Key Renames ───────────────────────────────────────────
    if !key_renames.is_empty() {
        doc.push_str("## Config Key Renames\n\n");
        doc.push_str(
            "Update your `starforge.toml` (or equivalent config file) accordingly.\n\n",
        );
        doc.push_str("| Before | After |\n");
        doc.push_str("|--------|-------|\n");
        for (old, new) in &key_renames {
            doc.push_str(&format!("| `{}` | `{}` |\n", old, new));
        }
        doc.push('\n');
    }

    // ── Section: Removed Config Keys ─────────────────────────────────────────
    if !key_removed.is_empty() {
        doc.push_str("## Removed Config Keys\n\n");
        for (key, desc) in &key_removed {
            doc.push_str(&format!("- `{}` — {} (removed)\n", key, desc));
        }
        doc.push('\n');
    }

    // ── Section: Config Key Type Changes ─────────────────────────────────────
    if !key_type_changes.is_empty() {
        doc.push_str("## Config Key Type Changes\n\n");
        doc.push_str("These keys exist in both versions but their value type changed.\n\n");
        doc.push_str("| Key | Old Type | New Type |\n");
        doc.push_str("|-----|----------|----------|\n");
        for (key, old_type, new_type) in &key_type_changes {
            doc.push_str(&format!("| `{}` | `{}` | `{}` |\n", key, old_type, new_type));
        }
        doc.push('\n');
    }

    // ── Footer ────────────────────────────────────────────────────────────────
    doc.push_str("---\n\n");
    doc.push_str(
        "_These notes were generated automatically. Edit as needed, then remove \
        this line before publishing._\n",
    );

    doc
}

// ── Snapshot I/O ──────────────────────────────────────────────────────────────

/// Load a `CliSnapshot` from a JSON file.
pub fn load_snapshot(path: &std::path::Path) -> anyhow::Result<CliSnapshot> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read snapshot {}: {}", path.display(), e))?;
    serde_json::from_str(&data)
        .map_err(|e| anyhow::anyhow!("Failed to parse snapshot {}: {}", path.display(), e))
}

/// Save a `CliSnapshot` to a JSON file.
pub fn save_snapshot(snapshot: &CliSnapshot, path: &std::path::Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, serde_json::to_string_pretty(snapshot)?)
        .map_err(|e| anyhow::anyhow!("Failed to write snapshot {}: {}", path.display(), e))
}
