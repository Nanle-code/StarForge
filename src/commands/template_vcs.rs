use crate::utils::{print as p, template_vcs, template_version_ai};
use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum TemplateVcsCommands {
    /// Initialize version control for a template directory
    Init {
        /// Path to the template directory
        path: PathBuf,
        /// Template name
        #[arg(long)]
        name: String,
    },
    /// Commit a new version of the template
    Commit {
        /// Path to the template directory
        path: PathBuf,
        /// Version number (semver, e.g. "1.0.0")
        version: String,
        /// Commit message describing changes
        message: String,
        /// Author name
        #[arg(long)]
        author: Option<String>,
    },
    /// Create a new branch for template development
    Branch {
        /// Path to the template directory
        path: PathBuf,
        /// Branch name to create
        name: Option<String>,
        /// Switch to this branch after creation
        #[arg(long)]
        checkout: Option<String>,
    },
    /// Show version history
    Log {
        /// Path to the template directory
        path: PathBuf,
        /// Number of versions to show
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// Show uncommitted changes
    Diff {
        /// Path to the template directory
        path: PathBuf,
    },
    /// Create a release (tag + changelog)
    Release {
        /// Path to the template directory
        path: PathBuf,
        /// Version number (semver)
        version: String,
        /// Release notes
        message: String,
        /// Author name
        #[arg(long)]
        author: Option<String>,
    },
    /// Generate or view the changelog
    Changelog {
        /// Path to the template directory
        path: PathBuf,
    },
    /// Show VCS status
    Status {
        /// Path to the template directory
        path: PathBuf,
    },
    /// Analyze changes using AI
    Analyze {
        /// Path to the template directory
        path: PathBuf,
    },
    /// Check compatibility between versions using AI
    Compatibility {
        /// Path to the template directory
        path: PathBuf,
        /// From version
        from: String,
        /// To version
        to: String,
    },
    /// Suggest next version using AI
    Suggest {
        /// Path to the template directory
        path: PathBuf,
    },
    /// Generate migration guide between versions using AI
    Migrate {
        /// Path to the template directory
        path: PathBuf,
        /// From version
        from: String,
        /// To version
        to: String,
    },
    /// Generate a read-only upgrade checklist for a scaffolded project
    Upgrade {
        /// Path to the scaffolded project
        path: PathBuf,
        /// Target template version (semver)
        #[arg(long)]
        to: String,
        /// Include optional manual patch suggestions
        #[arg(long)]
        patch_hints: bool,
    },
    /// Rollback to a specific version
    Rollback {
        /// Path to the template directory
        path: PathBuf,
        /// Version to rollback to
        version: String,
    },
}

pub async fn handle(cmd: TemplateVcsCommands) -> Result<()> {
    match cmd {
        TemplateVcsCommands::Init { path, name } => init(path, name),
        TemplateVcsCommands::Commit {
            path,
            version,
            message,
            author,
        } => commit(path, version, message, author),
        TemplateVcsCommands::Branch {
            path,
            name,
            checkout,
        } => branch(path, name, checkout),
        TemplateVcsCommands::Log { path, limit } => log(path, limit),
        TemplateVcsCommands::Diff { path } => diff(path),
        TemplateVcsCommands::Release {
            path,
            version,
            message,
            author,
        } => release(path, version, message, author),
        TemplateVcsCommands::Changelog { path } => changelog(path),
        TemplateVcsCommands::Status { path } => status(path),
        TemplateVcsCommands::Analyze { path } => analyze(path).await,
        TemplateVcsCommands::Compatibility { path, from, to } => {
            compatibility(path, from, to).await
        }
        TemplateVcsCommands::Suggest { path } => suggest(path).await,
        TemplateVcsCommands::Migrate { path, from, to } => migrate(path, from, to).await,
        TemplateVcsCommands::Upgrade {
            path,
            to,
            patch_hints,
        } => upgrade(path, to, patch_hints),
        TemplateVcsCommands::Rollback { path, version } => rollback(path, version).await,
    }
}

fn init(path: PathBuf, name: String) -> Result<()> {
    p::header("Template Version Control — Init");
    p::step(1, 2, "Initializing git repository...");
    template_vcs::init_vcs(&path, &name)?;

    p::step(2, 2, "Creating version tracking...");
    println!();
    p::success(&format!("Version control initialized for '{}'", name));
    p::kv("Path", &path.display().to_string());
    p::info("Use `starforge template-vcs commit` to record versions.");
    Ok(())
}

fn commit(path: PathBuf, version: String, message: String, author: Option<String>) -> Result<()> {
    p::header("Template Version Control — Commit");
    let author_name = author.unwrap_or_else(|| "Anonymous".to_string());

    p::step(1, 2, &format!("Recording version {}...", version));
    let entry = template_vcs::commit_version(&path, &version, &message, &author_name)?;

    p::step(2, 2, "Updating changelog...");
    println!();
    p::success(&format!("Version {} committed", entry.tag));
    p::kv("Version", &entry.version);
    p::kv("Tag", &entry.tag);
    p::kv("Author", &entry.author);
    p::kv("Changes", &entry.message);
    Ok(())
}

fn branch(path: PathBuf, name: Option<String>, checkout: Option<String>) -> Result<()> {
    if let Some(branch_name) = checkout {
        p::header("Template Version Control — Switch Branch");
        template_vcs::switch_branch(&path, &branch_name)?;
        p::success(&format!("Switched to branch '{}'", branch_name));
        return Ok(());
    }

    if let Some(branch_name) = name {
        p::header("Template Version Control — Create Branch");
        template_vcs::create_branch(&path, &branch_name)?;
        p::success(&format!("Branch '{}' created", branch_name));
        return Ok(());
    }

    p::header("Template Version Control — Branches");
    let branches = template_vcs::list_branches(&path)?;

    if branches.is_empty() {
        p::info("No branches found. Initialize VCS first.");
        return Ok(());
    }

    for branch in &branches {
        let marker = if branch.current { "* " } else { "  " };
        println!(
            "{}{} {} {}",
            marker, branch.name, branch.last_commit, branch.last_message
        );
    }
    Ok(())
}

fn log(path: PathBuf, limit: usize) -> Result<()> {
    p::header("Template Version Control — Log");
    let versions = template_vcs::view_log(&path, limit)?;

    if versions.is_empty() {
        p::info("No version history found. Commit a version first.");
        return Ok(());
    }

    for version in &versions {
        println!(
            "  {} ({}) — {}",
            version.tag,
            &version.timestamp[..10],
            version.message.lines().next().unwrap_or("")
        );
        p::kv("Author", &version.author);
        if version.changes.len() > 1 {
            for change in &version.changes {
                println!("    - {}", change);
            }
        }
        println!();
    }

    p::kv("Showing", &format!("{} versions", versions.len()));
    Ok(())
}

fn diff(path: PathBuf) -> Result<()> {
    p::header("Template Version Control — Diff");
    let diff_output = template_vcs::show_diff(&path)?;

    if diff_output.trim().is_empty() {
        p::info("No changes detected.");
    } else {
        println!("{}", diff_output);
    }
    Ok(())
}

fn release(path: PathBuf, version: String, message: String, author: Option<String>) -> Result<()> {
    p::header("Template Version Control — Release");
    let author_name = author.unwrap_or_else(|| "Anonymous".to_string());

    p::step(1, 2, &format!("Creating release {}...", version));
    let entry = template_vcs::create_release(&path, &version, &message, &author_name)?;

    p::step(2, 2, "Generating changelog...");
    template_vcs::generate_changelog(&path)?;

    println!();
    p::success(&format!("Release {} created", entry.tag));
    p::kv("Version", &entry.version);
    p::kv("Tag", &entry.tag);
    p::kv("Author", &entry.author);
    Ok(())
}

fn changelog(path: PathBuf) -> Result<()> {
    p::header("Template Version Control — Changelog");
    let content = template_vcs::generate_changelog(&path)?;
    println!("{}", content);
    Ok(())
}

fn status(path: PathBuf) -> Result<()> {
    p::header("Template Version Control — Status");
    p::kv("Path", &path.display().to_string());

    let versions = template_vcs::get_version_history(&path)?;
    p::kv("Versions", &versions.versions.len().to_string());

    if !versions.versions.is_empty() {
        if let Some(latest) = versions
            .versions
            .iter()
            .max_by(|a, b| a.version.cmp(&b.version))
        {
            p::kv("Latest", &latest.version);
        }
    }

    let branches = template_vcs::list_branches(&path).unwrap_or_default();
    p::kv("Branches", &branches.len().to_string());

    let diff_output = template_vcs::show_diff(&path).unwrap_or_default();
    let has_changes = !diff_output.trim().is_empty();
    p::kv("Uncommitted", if has_changes { "Yes" } else { "No" });

    println!();
    p::info("Use `starforge template-vcs commit` to record changes.");
    Ok(())
}

async fn analyze(path: PathBuf) -> Result<()> {
    p::header("Template Version Control — AI Change Analysis");
    let analysis = template_version_ai::analyze_changes(&path).await?;

    p::kv("Impact Level", &analysis.impact_level);
    println!("\nChanges:");
    for change in &analysis.changes {
        println!("  - {}", change);
    }

    println!("\nBreaking Changes:");
    if analysis.breaking_changes.is_empty() {
        println!("  None");
    } else {
        for bc in &analysis.breaking_changes {
            println!("  - {}", bc);
        }
    }

    println!("\nSummary: {}", analysis.summary);
    Ok(())
}

async fn compatibility(path: PathBuf, from: String, to: String) -> Result<()> {
    p::header("Template Version Control — AI Compatibility Check");
    let check = template_version_ai::check_compatibility(&path, &from, &to).await?;

    p::kv("Compatible", if check.compatible { "Yes" } else { "No" });
    println!("\nIssues:");
    if check.issues.is_empty() {
        println!("  None");
    } else {
        for issue in &check.issues {
            println!("  - {}", issue);
        }
    }

    println!("\nRecommendations:");
    for rec in &check.recommendations {
        println!("  - {}", rec);
    }
    Ok(())
}

async fn suggest(path: PathBuf) -> Result<()> {
    p::header("Template Version Control — AI Update Suggestion");
    let suggestion = template_version_ai::suggest_update(&path).await?;

    p::kv("Suggested Version", &suggestion.suggested_version);
    p::kv("Reason", &suggestion.reason);
    println!("\nBenefits:");
    for benefit in &suggestion.benefits {
        println!("  - {}", benefit);
    }
    Ok(())
}

async fn migrate(path: PathBuf, from: String, to: String) -> Result<()> {
    p::header("Template Version Control — AI Migration Guide");
    let guide = template_version_ai::generate_migration_guide(&path, &from, &to).await?;

    p::kv("From Version", &guide.from_version);
    p::kv("To Version", &guide.to_version);
    println!("\nSteps:");
    for (i, step) in guide.steps.iter().enumerate() {
        println!("  {}. {}", i + 1, step);
    }

    println!("\nWarnings:");
    if guide.warnings.is_empty() {
        println!("  None");
    } else {
        for warning in &guide.warnings {
            println!("  - {}", warning);
        }
    }
    Ok(())
}

fn upgrade(path: PathBuf, to: String, patch_hints: bool) -> Result<()> {
    p::header("Template Upgrade Guide");
    let Some(plan) = template_vcs::plan_project_upgrade(&path, &to, patch_hints)? else {
        p::warn(
            "No .starforge-template.json metadata found; the source template version is unknown.",
        );
        p::info("Add {\"template\": \"<name>\", \"version\": \"<semver>\"} metadata and rerun this command.");
        return Ok(());
    };

    p::kv("Template", &plan.template);
    p::kv("From version", &plan.from_version);
    p::kv("To version", &plan.to_version);
    println!("\nMajor version transitions:");
    if plan.major_steps.is_empty() {
        println!("  No major-version transitions.");
    } else {
        for step in &plan.major_steps {
            println!("  - {}", step);
        }
    }

    println!("\nMigration checklist:");
    for (index, item) in plan.checklist.iter().enumerate() {
        println!("  {}. {}", index + 1, item);
    }

    if !plan.patch_hints.is_empty() {
        println!("\nOptional patch suggestions:");
        for hint in &plan.patch_hints {
            println!("  - {}", hint);
        }
    }
    p::warn("No project files were changed. Review and apply any edits yourself.");
    Ok(())
}

async fn rollback(path: PathBuf, version: String) -> Result<()> {
    p::header("Template Version Control — Rollback");
    p::step(1, 1, &format!("Rolling back to version {}...", version));
    template_version_ai::rollback_to_version(&path, &version).await?;
    p::success(&format!("Successfully rolled back to version {}", version));
    Ok(())
}
