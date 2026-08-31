use crate::utils::{print as p, template_analytics, templates};
use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Subcommand)]
pub enum TemplateCommands {
    /// Search for templates in the marketplace
    Search {
        /// Search query (matches name, description, or tags). Use "" to list all.
        #[arg(default_value = "")]
        query: String,
        /// Filter by tags (comma-separated); a template must have all of them
        #[arg(long)]
        tags: Option<String>,
        /// Only show verified templates
        #[arg(long)]
        verified: bool,
        /// Only show templates with at least this quality score (0-100)
        #[arg(long, default_value_t = 0)]
        min_quality: u8,
        /// Force refresh of remote registry, ignoring cached copy
        #[arg(long)]
        refresh: bool,
    },
    /// List all available templates
    List,
    /// Show details of a specific template
    Show {
        /// Template name
        name: String,
    },
    /// Import a template from a directory or .zip archive into the local registry
    Import {
        /// Path to template directory or .zip package
        path: PathBuf,
        /// Template name (defaults to directory/archive stem)
        #[arg(long)]
        name: Option<String>,
        /// Template description
        #[arg(long)]
        description: Option<String>,
        /// Author name
        #[arg(long)]
        author: Option<String>,
        /// Tags (comma-separated)
        #[arg(long)]
        tags: Option<String>,
        /// Version
        #[arg(long, default_value = "1.0.0")]
        version: String,
        /// Minimum StarForge CLI version required
        #[arg(long)]
        cli_version_min: Option<String>,
        /// Maximum StarForge CLI version supported
        #[arg(long)]
        cli_version_max: Option<String>,
    },
    /// Publish a template to the local marketplace
    Publish {
        /// Path to the template directory
        path: PathBuf,
        /// Template name
        #[arg(long)]
        name: Option<String>,
        /// Template description
        #[arg(long)]
        description: Option<String>,
        /// Author name
        #[arg(long)]
        author: Option<String>,
        /// Tags (comma-separated)
        #[arg(long)]
        tags: Option<String>,
        /// Version
        #[arg(long, default_value = "1.0.0")]
        version: String,
        /// Minimum StarForge CLI version required (semver, e.g. "0.1.0")
        #[arg(long)]
        cli_version_min: Option<String>,
        /// Maximum StarForge CLI version supported (semver, e.g. "1.99.99")
        #[arg(long)]
        cli_version_max: Option<String>,
        /// SPDX license identifier (e.g. "MIT", "Apache-2.0")
        #[arg(long)]
        license: Option<String>,
        /// Source repository URL
        #[arg(long)]
        repository: Option<String>,
        /// Project homepage URL
        #[arg(long)]
        homepage: Option<String>,
        /// Extended documentation URL
        #[arg(long)]
        documentation: Option<String>,
    },
    /// Remove a template from the local marketplace
    Remove {
        /// Template name
        name: String,

        /// Also delete cached files and downloaded assets
        #[arg(long)]
        purge: bool,
    },
    /// Initialize the template registry with example templates
    Init,
    /// Show full metadata for a template: author, version, license, repository, trust badges
    Info {
        /// Template name
        name: String,
    },
    /// Install a template from a Git URL, local path, or marketplace registry name
    Install {
        /// Source: git URL (https://...), local filesystem path, or registry template name
        source: String,
        /// Override the installed template name (defaults to the template name or URL basename)
        #[arg(long)]
        name: Option<String>,
        /// Pin to a specific version when installing from the marketplace registry
        #[arg(long)]
        version: Option<String>,
        /// Overwrite the template if it is already installed
        #[arg(long, default_value = "false")]
        force: bool,
    },
    /// Update installed templates to their latest versions
    Update {
        /// Name of the template to update (omit when using --all)
        #[arg(long, conflicts_with = "all")]
        name: Option<String>,
        /// Update all installed git-sourced templates
        #[arg(long, short, conflicts_with = "name")]
        all: bool,
    },
    /// Run the built-in test suite for a template
    Test {
        /// Template name or path to a template directory
        name: String,
        /// Show verbose cargo output
        #[arg(long)]
        verbose: bool,
    },
    /// Generate Markdown documentation for a template from its registry metadata
    Docs {
        /// Template name to generate docs for
        name: String,
        /// Write the generated docs to this file (defaults to stdout)
        #[arg(long)]
        output: Option<std::path::PathBuf>,
    },
    /// Show security review status for a template (or all templates)
    Audit {
        /// Template name (omit to list the security status of all templates)
        name: Option<String>,
    },
    /// Generate an AI-assisted community analysis report (usage, feedback, trends, issues)
    Analyze {
        /// Template name (omit to analyze the whole marketplace)
        name: Option<String>,
        /// Output as JSON instead of a human-readable summary
        #[arg(long)]
        json: bool,
        /// Write the report to this file instead of stdout
        #[arg(long, short)]
        out: Option<std::path::PathBuf>,
        /// Also generate a natural-language narrative via a locally running Ollama model
        #[arg(long)]
        ai: bool,
    },
    /// Submit community feedback (rating and/or comment) for a template
    Feedback {
        /// Template name
        name: String,
        /// Free-text feedback comment
        #[arg(long)]
        comment: String,
        /// Star rating from 1 (worst) to 5 (best)
        #[arg(long)]
        rating: Option<u8>,
        /// Feedback category: bug, feature-request, praise, question, or other.
        /// Auto-detected from the comment/rating when omitted.
        #[arg(long)]
        category: Option<String>,
    },
}

pub async fn handle(cmd: TemplateCommands) -> Result<()> {
    match cmd {
        TemplateCommands::Import {
            path,
            name,
            description,
            author,
            tags,
            version,
            cli_version_min,
            cli_version_max,
        } => {
            import(
                path,
                name,
                description,
                author,
                tags,
                version,
                cli_version_min,
                cli_version_max,
            )
            .await
        }
        TemplateCommands::Publish {
            path,
            name,
            description,
            author,
            tags,
            version,
            cli_version_min,
            cli_version_max,
            license,
            repository,
            homepage,
            documentation,
        } => {
            publish(
                path,
                name,
                description,
                author,
                tags,
                version,
                cli_version_min,
                cli_version_max,
                license,
                repository,
                homepage,
                documentation,
            )
            .await
        }
        TemplateCommands::List => list().await,
        TemplateCommands::Search {
            query,
            tags,
            verified,
            min_quality,
            refresh,
        } => search(query, tags, verified, min_quality, refresh).await,
        TemplateCommands::Show { name } => show(name).await,
        TemplateCommands::Remove { name, purge } => remove(name, purge).await,
        TemplateCommands::Init => init(),
        TemplateCommands::Info { name } => info(name).await,
        TemplateCommands::Install {
            source,
            name,
            version,
            force,
        } => install(source, name, version, force).await,
        TemplateCommands::Update { name, all } => update(name, all).await,
        TemplateCommands::Test { name, verbose } => template_test(name, verbose).await,
        TemplateCommands::Docs { name, output } => template_docs(name, output).await,
        TemplateCommands::Audit { name } => template_audit(name).await,
        TemplateCommands::Analyze {
            name,
            json,
            out,
            ai,
        } => template_analyze(name, json, out, ai).await,
        TemplateCommands::Feedback {
            name,
            comment,
            rating,
            category,
        } => template_feedback(name, comment, rating, category),
    }
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
async fn import(
    path: PathBuf,
    name: Option<String>,
    description: Option<String>,
    author: Option<String>,
    tags: Option<String>,
    version: String,
    cli_version_min: Option<String>,
    cli_version_max: Option<String>,
) -> Result<()> {
    publish(
        path,
        name,
        description,
        author,
        tags,
        version,
        cli_version_min,
        cli_version_max,
        None,
        None,
        None,
        None,
    )
    .await?;
    p::header("Template Import");
    p::info("Template package imported into the local registry.");
    Ok(())
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
async fn publish(
    path: PathBuf,
    name: Option<String>,
    description: Option<String>,
    author: Option<String>,
    tags: Option<String>,
    version: String,
    cli_version_min: Option<String>,
    cli_version_max: Option<String>,
    license: Option<String>,
    repository: Option<String>,
    homepage: Option<String>,
    documentation: Option<String>,
) -> Result<()> {
    use dialoguer::{theme::ColorfulTheme, Input};
    let name = match name {
        Some(n) => n,
        None => Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Template name")
            .interact_text()?,
    };
    let description = match description {
        Some(d) => d,
        None => Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Description")
            .interact_text()?,
    };
    let author = match author {
        Some(a) => a,
        None => Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Author")
            .interact_text()?,
    };
    let tag_list: Vec<String> = tags
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    templates::publish_template_versioned(
        &path,
        name.clone(),
        description,
        author,
        tag_list,
        version,
        cli_version_min,
        cli_version_max,
        license,
        repository,
        homepage,
        documentation,
    )
    .await?;
    let template = templates::get_template(&name).await?;

    p::header("Template Publish");
    p::success("Template registered successfully");
    p::kv_accent("Name", &template.name);
    p::kv("Version", &template.version);
    p::kv("Source", &template.source.to_string());
    if !template.tags.is_empty() {
        p::kv("Tags", &template.tags.join(", "));
    }
    if let Some(lic) = template.license.as_ref() {
        p::kv("License", lic);
    }
    if let Some(repo) = template.repository.as_ref() {
        p::kv("Repository", repo);
    }
    if let Some(path) = template.path.as_ref() {
        p::kv("Path", path);
    }

    Ok(())
}

async fn list() -> Result<()> {
    use crate::utils::templates::{check_template_compatibility, CompatibilityStatus};

    let registry = templates::load_registry().await?;
    p::header("Template Registry");
    if registry.templates.is_empty() {
        p::info("No templates found. Publish one with: starforge template publish <path>");
        return Ok(());
    }

    for (i, template) in registry.templates.iter().enumerate() {
        let compat_badge = match check_template_compatibility(template) {
            CompatibilityStatus::Compatible => "[COMPATIBLE]",
            CompatibilityStatus::TooOld { .. } | CompatibilityStatus::TooNew { .. } => {
                "[INCOMPATIBLE]"
            }
            CompatibilityStatus::MalformedMetadata { .. } => "[BAD-META]",
        };
        let mut badges = template.trust_indicators();
        badges.push(compat_badge.to_string());
        println!(
            "  {:>2}. {}@{}  [quality {}/100]  {}",
            i + 1,
            template.name,
            template.version,
            template.quality_score(),
            badges.join(" "),
        );
        p::kv("Description", &template.description);
        p::kv("Source", &template.source.to_string());
        if !template.tags.is_empty() {
            p::kv("Tags", &template.tags.join(", "));
        }
        if let Some(path) = template.path.as_ref() {
            p::kv("Path", path);
        }
        if i + 1 < registry.templates.len() {
            println!();
        }
    }

    Ok(())
}

async fn search(
    query: String,
    tags: Option<String>,
    verified: bool,
    min_quality: u8,
    refresh: bool,
) -> Result<()> {
    use crate::utils::templates::{check_template_compatibility, CompatibilityStatus};
    let tag_list: Vec<String> = tags
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let filters = templates::SearchFilters {
        categories: vec![],
        featured_only: false,
        hide_spam: false,
        tags: tag_list,
        verified_only: verified,
        min_quality,
    };

    // Load registry, optionally forcing a refresh of the remote copy.
    let results = if refresh {
        std::env::set_var("STARFORGE_TEMPLATE_REGISTRY_FORCE_REFRESH", "1");
        let res = templates::search_templates_ranked(&query, &filters).await;
        std::env::remove_var("STARFORGE_TEMPLATE_REGISTRY_FORCE_REFRESH");
        res?
    } else {
        templates::search_templates_ranked(&query, &filters).await?
    };

    let heading = if query.trim().is_empty() {
        "Template search results".to_string()
    } else {
        format!("Template search results for '{}'", query)
    };
    p::header(&heading);

    // Summarize the active filters so users understand the result set.
    let mut active_filters = Vec::new();
    if !filters.tags.is_empty() {
        active_filters.push(format!("tags: {}", filters.tags.join(", ")));
    }
    if filters.verified_only {
        active_filters.push("verified only".to_string());
    }
    if filters.min_quality > 0 {
        active_filters.push(format!("min quality: {}", filters.min_quality));
    }
    if !active_filters.is_empty() {
        p::kv("Filters", &active_filters.join("  |  "));
    }

    if results.is_empty() {
        p::info("No templates matched. Try a broader query or relaxing the filters.");
        return Ok(());
    }

    p::kv("Matches", &results.len().to_string());
    println!();

    for (i, result) in results.iter().enumerate() {
        let template = &result.entry;
        let compat_badge = match check_template_compatibility(template) {
            CompatibilityStatus::Compatible => "[COMPATIBLE]",
            CompatibilityStatus::TooOld { .. } | CompatibilityStatus::TooNew { .. } => {
                "[INCOMPATIBLE]"
            }
            CompatibilityStatus::MalformedMetadata { .. } => "[BAD-META]",
        };
        let mut badges = template.trust_indicators();
        badges.push(compat_badge.to_string());
        println!(
            "  {:>2}. {}@{}  [quality {}/100]  {}",
            i + 1,
            template.name,
            template.version,
            template.quality_score(),
            badges.join(" "),
        );
        p::kv("Description", &template.description);
        p::kv("Downloads", &template.downloads.to_string());
        if !template.tags.is_empty() {
            p::kv("Tags", &template.tags.join(", "));
        }
        // Explain why this result matched, helping users scan the list.
        if !result.reasons.is_empty() {
            p::kv(
                "Matched",
                &format!(
                    "{} (relevance {})",
                    result.reasons.join(", "),
                    result.relevance
                ),
            );
        }
        p::kv("Source", &template.source.to_string());
        if i + 1 < results.len() {
            println!();
        }
    }

    Ok(())
}

async fn show(name: String) -> Result<()> {
    use crate::utils::templates::{check_template_compatibility, CompatibilityStatus};

    let template = templates::get_template(&name).await?;
    p::header(&format!("Template: {}", template.name));
    p::kv("Version", &template.version);
    p::kv("Description", &template.description);
    p::kv("Source", &template.source.to_string());
    if !template.author.is_empty() {
        p::kv("Author", &template.author);
    }
    if !template.tags.is_empty() {
        p::kv("Tags", &template.tags.join(", "));
    }
    if let Some(ref license) = template.license {
        p::kv("License", license);
    }
    if let Some(ref repo) = template.repository {
        p::kv("Repository", repo);
    }
    if let Some(ref hp) = template.homepage {
        p::kv("Homepage", hp);
    }
    if let Some(ref doc_url) = template.documentation {
        p::kv("Documentation", doc_url);
    }
    if let Some(ref min) = template.cli_version_min {
        p::kv("Requires StarForge >=", min);
    }
    if let Some(ref max) = template.cli_version_max {
        p::kv("Requires StarForge <=", max);
    }
    match check_template_compatibility(&template) {
        CompatibilityStatus::Compatible => p::success("Compatible with this StarForge version"),
        CompatibilityStatus::TooOld {
            required_min,
            running,
        } => {
            p::warn(&format!(
                "Incompatible: requires >= {} (running {})",
                required_min, running
            ));
        }
        CompatibilityStatus::TooNew {
            required_max,
            running,
        } => {
            p::warn(&format!(
                "Incompatible: requires <= {} (running {})",
                required_max, running
            ));
        }
        CompatibilityStatus::MalformedMetadata { reason } => {
            p::warn(&format!("Malformed version metadata: {}", reason));
        }
    }
    print_quality_signals(&template);
    Ok(())
}

/// Render the quality / trust signals for a template so users can quickly
/// gauge how dependable it is.
fn print_quality_signals(template: &templates::TemplateEntry) {
    p::kv(
        "Quality score",
        &format!("{}/100", template.quality_score()),
    );
    p::kv("Maintenance", template.maintenance.label());
    p::kv(
        "Documentation",
        if template.documented {
            "Available"
        } else {
            "Not provided"
        },
    );
    p::kv("Downloads", &template.downloads.to_string());
    let badges = template.trust_indicators();
    if !badges.is_empty() {
        p::kv("Trust signals", &badges.join("  "));
    }
}

async fn remove(name: String, purge: bool) -> Result<()> {
    templates::remove_template(&name, purge).await?;

    if purge {
        p::success(&format!("Template '{}' and all local assets removed", name));
    } else {
        p::success(&format!(
            "Template '{}' removed from registry (use --purge to also delete cached files)",
            name
        ));
    }
    Ok(())
}

fn init() -> Result<()> {
    p::info("Template registry is ready. Use `starforge template list` to view templates.");
    Ok(())
}

async fn info(name: String) -> Result<()> {
    use crate::utils::templates::{check_template_compatibility, CompatibilityStatus};

    let template = templates::get_template(&name).await?;

    p::header(&format!("Template Info: {}", template.name));
    p::separator();

    p::kv_accent("Name", &template.name);
    p::kv("Version", &template.version);

    if !template.author.is_empty() {
        p::kv("Author", &template.author);
    }
    if !template.description.is_empty() {
        p::kv("Description", &template.description);
    }
    if !template.tags.is_empty() {
        p::kv("Tags", &template.tags.join(", "));
    }

    println!();
    p::info("Source & Repository");
    p::kv("Source", &template.source.to_string());
    if let Some(ref repo) = template.repository {
        p::kv("Repository", repo);
    }

    println!();
    p::info("Licensing & Compatibility");
    if let Some(ref license) = template.license {
        p::kv("License", license);
    } else {
        p::kv("License", "Not declared");
    }
    match (
        template.cli_version_min.as_deref(),
        template.cli_version_max.as_deref(),
    ) {
        (Some(min), Some(max)) => p::kv("CLI Version Range", &format!(">= {}  <=  {}", min, max)),
        (Some(min), None) => p::kv("CLI Version Range", &format!(">= {}", min)),
        (None, Some(max)) => p::kv("CLI Version Range", &format!("<= {}", max)),
        (None, None) => p::kv("CLI Version Range", "Any version"),
    }
    match check_template_compatibility(&template) {
        CompatibilityStatus::Compatible => p::success("Compatible with this StarForge version"),
        CompatibilityStatus::TooOld {
            required_min,
            running,
        } => p::warn(&format!(
            "Incompatible: requires >= {} (running {})",
            required_min, running
        )),
        CompatibilityStatus::TooNew {
            required_max,
            running,
        } => p::warn(&format!(
            "Incompatible: requires <= {} (running {})",
            required_max, running
        )),
        CompatibilityStatus::MalformedMetadata { reason } => {
            p::warn(&format!("Malformed version metadata: {}", reason))
        }
    }

    println!();
    p::info("Quality & Trust");
    p::kv(
        "Quality Score",
        &format!("{}/100", template.quality_score()),
    );
    p::kv("Maintenance", template.maintenance.label());
    p::kv(
        "Documentation",
        if template.documented {
            "Available"
        } else {
            "Not provided"
        },
    );
    p::kv("Downloads", &template.downloads.to_string());

    let badges = template.trust_indicators();
    if !badges.is_empty() {
        p::kv("Trust Badges", &badges.join("  "));
    }

    if !template.created_at.is_empty() {
        println!();
        p::info("Timestamps");
        p::kv("Published", &template.created_at);
        if !template.updated_at.is_empty() {
            p::kv("Last Updated", &template.updated_at);
        }
    }

    p::separator();
    Ok(())
}

pub async fn install(
    source: String,
    name: Option<String>,
    version: Option<String>,
    force: bool,
) -> Result<()> {
    p::header("Template Install");
    p::kv("Source", &source);
    if let Some(ref n) = name {
        p::kv("Name override", n);
    }
    if let Some(ref v) = version {
        p::kv("Version", v);
    }
    println!();

    p::step(1, 2, "Resolving and fetching template...");
    let entry =
        templates::install_template(&source, name.as_deref(), version.as_deref(), force).await?;

    p::step(2, 2, "Registering in local registry...");
    println!();

    // Best-effort usage tracking for community analytics — never fail the
    // install because analytics logging had a problem.
    let _ = template_analytics::record_usage(&entry.name, template_analytics::UsageAction::Install);

    p::success(&format!("Template '{}' installed", entry.name));
    p::kv_accent("Name", &entry.name);
    p::kv("Version", &entry.version);
    p::kv("Source", &entry.source.to_string());
    if let Some(ref path) = entry.path {
        p::kv("Local path", path);
    }
    p::info(&format!(
        "Use it with: starforge template info {}",
        entry.name
    ));
    Ok(())
}

async fn update(name: Option<String>, all: bool) -> Result<()> {
    if all {
        p::header("Template Update — All");
        p::step(1, 1, "Updating all git-sourced templates...");
        let results = templates::update_all_installed_templates().await?;

        if results.is_empty() {
            p::info("No git-sourced templates are installed.");
            return Ok(());
        }

        println!();
        for (tpl_name, result) in &results {
            match result {
                Ok(_report) => p::success(&format!("  {} updated", tpl_name)),
                Err(e) => p::warn(&format!("  {} — {}", tpl_name, e)),
            }
        }

        let ok = results.iter().filter(|(_, r)| r.is_ok()).count();
        println!();
        p::kv("Updated", &format!("{}/{}", ok, results.len()));
        return Ok(());
    }

    let name = name.ok_or_else(|| {
        anyhow::anyhow!("Provide a template name or --all to update all templates")
    })?;

    p::header(&format!("Template Update: {}", name));
    p::step(1, 1, "Re-fetching from source...");
    templates::update_installed_template(&name).await?;
    println!();

    let _ = template_analytics::record_usage(&name, template_analytics::UsageAction::Update);

    p::success(&format!("Template '{}' updated", name));
    Ok(())
}

// ─── template test ────────────────────────────────────────────────────────────

async fn template_test(name: String, verbose: bool) -> Result<()> {
    use std::process::Command;

    p::header(&format!("Template Test: {}", name));

    // Locate the template source directory — prefer builtin examples.
    let builtin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("templates")
        .join("examples")
        .join(&name);

    let template_dir = if builtin.exists() {
        builtin
    } else {
        // Fall back to path stored in the registry.
        let entry = templates::get_template(&name).await?;
        match entry.path {
            Some(ref p) => std::path::PathBuf::from(p),
            None => anyhow::bail!(
                "Template '{}' has no local path. Install it first with: starforge template install {}",
                name, name
            ),
        }
    };

    p::kv("Template directory", &template_dir.display().to_string());
    p::info("Running: cargo test");

    let mut cmd = Command::new("cargo");
    cmd.arg("test");
    if verbose {
        cmd.arg("--verbose");
    }
    cmd.current_dir(&template_dir);

    let status = cmd.status()?;

    if status.success() {
        p::success("All tests passed");
    } else {
        anyhow::bail!("Tests failed for template '{}'", name);
    }
    Ok(())
}

// ─── template docs ────────────────────────────────────────────────────────────

async fn template_docs(name: String, output: Option<std::path::PathBuf>) -> Result<()> {
    let entry = templates::get_template(&name).await?;

    let mut md = String::new();

    // Title
    md.push_str(&format!("# {} `{}`\n\n", entry.name, entry.version));
    md.push_str(&format!("> {}\n\n", entry.description));

    // Badges
    if entry.verified {
        md.push_str("![Verified](https://img.shields.io/badge/verified-✓-brightgreen) ");
    }
    md.push_str(&format!(
        "![Maintenance](https://img.shields.io/badge/maintenance-{}-blue) ",
        entry.maintenance.label().replace(' ', "%20")
    ));
    if let Some(ref lic) = entry.license {
        md.push_str(&format!(
            "![License](https://img.shields.io/badge/license-{}-cyan)\n\n",
            lic
        ));
    } else {
        md.push('\n');
    }

    // Metadata table
    md.push_str("## Metadata\n\n");
    md.push_str("| Field | Value |\n|---|---|\n");
    md.push_str(&format!("| Author | {} |\n", entry.author));
    md.push_str(&format!("| Version | {} |\n", entry.version));
    md.push_str(&format!(
        "| License | {} |\n",
        entry.license.as_deref().unwrap_or("Not declared")
    ));
    md.push_str(&format!(
        "| Tags | {} |\n",
        if entry.tags.is_empty() {
            "—".to_string()
        } else {
            entry.tags.join(", ")
        }
    ));
    md.push_str(&format!("| Source | {} |\n", entry.source));
    if let Some(ref repo) = entry.repository {
        md.push_str(&format!("| Repository | {} |\n", repo));
    }
    if let Some(ref hp) = entry.homepage {
        md.push_str(&format!("| Homepage | {} |\n", hp));
    }
    md.push('\n');

    // Security review
    md.push_str("## Security Review\n\n");
    if let Some(ref sr) = entry.security_review {
        md.push_str(&format!("**Status:** {}\n\n", sr.status));
        if let (Some(ref auditor), Some(ref date)) = (&sr.auditor, &sr.audited_at) {
            md.push_str(&format!("- **Auditor:** {}\n", auditor));
            md.push_str(&format!("- **Audited at:** {}\n", date));
        }
        if let Some(findings) = &sr.findings {
            md.push_str(&format!("- **Findings:** {}\n", findings));
        }
        if let Some(score) = sr.score {
            md.push_str(&format!("- **Score:** {}/100\n", score));
        }
    } else {
        md.push_str("No security review data available.\n");
    }
    md.push('\n');

    // Changelog
    if let Some(changelogs) = &entry.changelog {
        if !changelogs.is_empty() {
            md.push_str("## Changelog\n\n");
            for changelog_entry in changelogs {
                md.push_str(&format!(
                    "### {} — {}\n\n",
                    changelog_entry.version, changelog_entry.date
                ));
            }
        }
    }

    // Usage
    md.push_str("## Usage\n\n");
    md.push_str("```bash\n");
    md.push_str(&format!(
        "starforge new contract my-project --template {}\n",
        name
    ));
    md.push_str("```\n");

    match output {
        Some(path) => {
            std::fs::write(&path, &md)?;
            p::success(&format!("Documentation written to {}", path.display()));
        }
        None => {
            p::header(&format!("Documentation for {}", name));
            println!("{}", md);
        }
    }
    Ok(())
}

// ─── template audit ───────────────────────────────────────────────────────────

async fn template_audit(name: Option<String>) -> Result<()> {
    let registry = templates::load_registry().await?;

    let entries: Vec<&templates::TemplateEntry> = match &name {
        Some(n) => registry.templates.iter().filter(|t| &t.name == n).collect(),
        None => registry.templates.iter().collect(),
    };

    if entries.is_empty() {
        if let Some(n) = &name {
            anyhow::bail!("Template '{}' not found in registry", n);
        } else {
            p::info("No templates in registry.");
            return Ok(());
        }
    }

    p::header("Template Security Review Status");
    println!(
        "  {:<20} {:<10} {:<12} {:<10} {:<8}",
        "NAME", "VERSION", "STATUS", "FINDINGS", "SCORE"
    );
    println!("  {}", "─".repeat(64));

    for entry in entries {
        let (status, findings, score) = match &entry.security_review {
            Some(sr) => (
                sr.status.as_str(),
                sr.findings
                    .map(|f| f.to_string())
                    .unwrap_or_else(|| "—".to_string()),
                sr.score
                    .map(|s| format!("{}/100", s))
                    .unwrap_or_else(|| "—".to_string()),
            ),
            None => ("not-reviewed", "—".to_string(), "—".to_string()),
        };

        let status_icon = match status {
            "audited" => "✓ audited",
            "pending" => "⧖ pending",
            _ => "✗ not-reviewed",
        };

        println!(
            "  {:<20} {:<10} {:<12} {:<10} {:<8}",
            entry.name, entry.version, status_icon, findings, score
        );
    }

    if name.is_none() {
        println!();
        let audited = registry
            .templates
            .iter()
            .filter(|t| {
                t.security_review
                    .as_ref()
                    .map(|sr| sr.status == "audited")
                    .unwrap_or(false)
            })
            .count();
        p::kv(
            "Audited",
            &format!("{}/{}", audited, registry.templates.len()),
        );
    }

    Ok(())
}

// ─── template analyze ──────────────────────────────────────────────────────────

async fn template_analyze(
    name: Option<String>,
    json: bool,
    out: Option<PathBuf>,
    ai: bool,
) -> Result<()> {
    let report = template_analytics::generate_report(name.as_deref()).await?;

    let narrative = if ai {
        p::info("Asking the local Ollama model for a narrative summary (best-effort)...");
        let result = template_analytics::ai_narrative_summary(&report).await;
        if result.is_none() {
            p::warn("Ollama is not running — showing the deterministic report only.");
        }
        result
    } else {
        None
    };

    let rendered = if json {
        #[derive(serde::Serialize)]
        struct ReportWithNarrative<'a> {
            #[serde(flatten)]
            report: &'a template_analytics::CommunityAnalysisReport,
            #[serde(skip_serializing_if = "Option::is_none")]
            ai_narrative: Option<String>,
        }
        serde_json::to_string_pretty(&ReportWithNarrative {
            report: &report,
            ai_narrative: narrative,
        })?
    } else {
        let mut text = report.to_text();
        if let Some(n) = &narrative {
            text.push_str("\nAI Narrative Summary\n");
            text.push_str(n);
            text.push('\n');
        }
        text
    };

    match out {
        Some(path) => {
            std::fs::write(&path, &rendered)?;
            p::success(&format!(
                "Community analysis report written to {}",
                path.display()
            ));
        }
        None => {
            if !json {
                p::header("Template Community Analysis");
            }
            println!("{}", rendered);
        }
    }

    Ok(())
}

// ─── template feedback ─────────────────────────────────────────────────────────

fn template_feedback(
    name: String,
    comment: String,
    rating: Option<u8>,
    category: Option<String>,
) -> Result<()> {
    let category = category
        .map(|c| template_analytics::FeedbackCategory::from_str(&c))
        .transpose()?;

    let entry = template_analytics::submit_feedback(&name, &comment, rating, category)?;

    p::header("Feedback Submitted");
    p::kv("Template", &entry.template);
    p::kv("Category", entry.category.label());
    if let Some(r) = entry.rating {
        p::kv("Rating", &format!("{}/5", r));
    }
    p::info("Thanks — this feeds into `starforge template analyze` reports.");
    Ok(())
}
