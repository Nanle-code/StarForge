use crate::utils::template_integration;
use crate::utils::template_performance;
use crate::utils::{output, print as p, registry, template_customization_ai, templates};
use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use std::path::PathBuf;

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
        /// Maximum number of results to show per page. Omit to show all matches.
        #[arg(long)]
        limit: Option<usize>,
        /// Resume after this pagination cursor (from a previous page's "Next cursor")
        #[arg(long)]
        cursor: Option<String>,
    },
    /// List all available templates
    List {
        /// Emit a machine-readable JSON object instead of the human-readable output
        #[arg(long)]
        json: bool,
        /// Maximum number of templates to show per page. Omit to show all.
        #[arg(long)]
        limit: Option<usize>,
        /// Resume after this pagination cursor (from a previous page's "Next cursor")
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Show details of a specific template
    Show {
        /// Template name
        name: String,
    },
    /// Install a template package into the local registry
    Install {
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
    /// Fetch and install a template from a Git URL, local path, or marketplace registry name
    Fetch {
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
    /// Roll back the last tracked update for a template
    Rollback {
        /// Template name to roll back
        name: String,
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
    /// Validate a template registry (or a single template's metadata) against the registry schema
    Validate {
        /// Registry JSON file, or a JSON file holding one template entry.
        /// Defaults to the local registry, falling back to the bundled one.
        path: Option<PathBuf>,
        /// Emit a machine-readable JSON report instead of human-readable output
        #[arg(long)]
        json: bool,
    },
    /// Show security review status for a template (or all templates)
    Audit {
        /// Template name (omit to list the security status of all templates)
        name: Option<String>,
    },
    /// Customize a template using AI based on requirements
    Customize {
        /// Path to the template directory
        path: PathBuf,
        /// Requirements for customization
        requirements: String,
    },
    /// View customization history for a template
    CustomizeHistory {
        /// Path to the template directory
        path: PathBuf,
    },
    /// Rollback template to a previous customization state
    CustomizeRollback {
        /// Path to the template directory
        path: PathBuf,
        /// Optional index to rollback to (0 is oldest, omit for previous)
        index: Option<usize>,
    },
}

pub async fn handle(cmd: TemplateCommands) -> Result<()> {
    match cmd {
        TemplateCommands::Install {
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
        TemplateCommands::List {
            json,
            limit,
            cursor,
        } => list(json, limit, cursor).await,
        TemplateCommands::Search {
            query,
            tags,
            verified,
            min_quality,
            refresh,
            limit,
            cursor,
        } => search(query, tags, verified, min_quality, refresh, limit, cursor).await,
        TemplateCommands::Show { name } => show(name).await,
        TemplateCommands::Remove { name, purge } => remove(name, purge).await,
        TemplateCommands::Init => init(),
        TemplateCommands::Info { name } => info(name).await,
        TemplateCommands::Fetch {
            source,
            name,
            version,
            force,
        } => crate::utils::template::install(source, name, version, force).await,
        TemplateCommands::Update { name, all } => update(name, all).await,
        TemplateCommands::Rollback { name } => rollback(name).await,
        TemplateCommands::Test { name, verbose } => template_test(name, verbose).await,
        TemplateCommands::Docs { name, output } => template_docs(name, output).await,
        TemplateCommands::Validate { path, json } => template_validate(path, json),
        TemplateCommands::Audit { name } => template_audit(name).await,
        TemplateCommands::Customize { path, requirements } => {
            template_customize(path, requirements).await
        }
        TemplateCommands::CustomizeHistory { path } => template_customize_history(path).await,
        TemplateCommands::CustomizeRollback { path, index } => {
            template_customize_rollback(path, index).await
        }
    }
}

async fn template_assist(
    template: String,
    project: PathBuf,
    run_tests: bool,
    json: bool,
    output: Option<PathBuf>,
) -> Result<()> {
    let direct = PathBuf::from(&template);
    let template_path = if direct.is_dir() {
        direct
    } else {
        let entry = templates::get_template(&template).await.with_context(|| {
            format!(
                "Template '{}' was not found. Pass a directory or run `starforge template list`.",
                template
            )
        })?;
        entry.path.map(PathBuf::from).filter(|path| path.is_dir()).or_else(|| {
            if let templates::TemplateSource::Local { path } = entry.source {
                let path = PathBuf::from(path);
                path.is_dir().then_some(path)
            } else {
                None
            }
        }).ok_or_else(|| anyhow::anyhow!(
            "Template '{}' is not available locally. Install it first with `starforge template install {}`.",
            template,
            template
        ))?
    };
    let mut report = template_integration::analyze(&template_path, &project)?;
    if run_tests {
        report.test_result = Some(template_integration::run_integration_tests(&project));
    }
    let rendered = if json {
        serde_json::to_string_pretty(&report)?
    } else {
        report.to_markdown()
    };
    if let Some(path) = output {
        std::fs::write(&path, rendered)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        p::success(&format!("Integration report written to {}", path.display()));
    } else {
        println!("{rendered}");
    }
    Ok(())
}
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
    if let Some(repo) = template.repository_url.as_ref() {
        p::kv("Repository", repo);
    }
    if let Some(path) = template.path.as_ref() {
        p::kv("Path", path);
    }

    Ok(())
}

/// Default number of results shown per page when pagination is requested via
/// `--cursor` without an explicit `--limit`.
const DEFAULT_PAGE_LIMIT: usize = 20;

/// Print the "Shown X of Y" / "Next cursor" footer for a paginated command,
/// when pagination was requested at all.
fn print_pagination_footer<T>(page: Option<&templates::Page<T>>) {
    if let Some(page) = page {
        println!();
        p::kv("Shown", &format!("{} of {}", page.items.len(), page.total));
        if let Some(next) = &page.next_cursor {
            p::info(&format!(
                "More results available — pass --cursor {} to continue",
                next
            ));
        }
    }
}

async fn list(json: bool, limit: Option<usize>, cursor: Option<String>) -> Result<()> {
    use crate::utils::templates::{check_template_compatibility, CompatibilityStatus};

    let registry = templates::load_registry().await?;
    let emit_json = json || output::is_json_mode_enabled();

    // Pagination only kicks in when the caller opts in via --limit or
    // --cursor, so plain `template list` keeps showing everything.
    let page = match limit.or(cursor.is_some().then_some(DEFAULT_PAGE_LIMIT)) {
        Some(limit) => Some(templates::paginate(
            &registry.templates,
            cursor.as_deref(),
            limit,
            |t| t.name.as_str(),
        )?),
        None => None,
    };
    let shown: &[templates::TemplateEntry] = match &page {
        Some(page) => &page.items,
        None => &registry.templates,
    };

    if emit_json {
        #[derive(serde::Serialize)]
        struct TemplateListResponse {
            template_count: usize,
            shown_count: usize,
            next_cursor: Option<String>,
            templates: Vec<TemplateSummary>,
        }

        #[derive(serde::Serialize)]
        struct TemplateSummary {
            name: String,
            version: String,
            description: String,
            source: String,
            tags: Vec<String>,
            path: Option<String>,
            compatible: bool,
        }

        let template_count = registry.templates.len();
        let next_cursor = page.as_ref().and_then(|p| p.next_cursor.clone());
        let templates: Vec<TemplateSummary> = shown
            .iter()
            .map(|template| {
                let compatible = matches!(
                    check_template_compatibility(template),
                    CompatibilityStatus::Compatible
                );
                TemplateSummary {
                    name: template.name.clone(),
                    version: template.version.clone(),
                    description: template.description.clone(),
                    source: template.source.to_string(),
                    tags: template.tags.clone(),
                    path: template.path.clone(),
                    compatible,
                }
            })
            .collect();

        return output::print_json(&TemplateListResponse {
            template_count,
            shown_count: templates.len(),
            next_cursor,
            templates,
        });
    }
    p::header("Template Registry");
    if registry.templates.is_empty() {
        p::info("No templates found. Publish one with: starforge template publish <path>");
        return Ok(());
    }
    if shown.is_empty() {
        p::info("No more templates on this page.");
        return Ok(());
    }

    for (i, template) in shown.iter().enumerate() {
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
        if i + 1 < shown.len() {
            println!();
        }
    }

    print_pagination_footer(page.as_ref());

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn search(
    query: String,
    tags: Option<String>,
    verified: bool,
    min_quality: u8,
    refresh: bool,
    limit: Option<usize>,
    cursor: Option<String>,
) -> Result<()> {
    use crate::utils::templates::{check_template_compatibility, CompatibilityStatus};
    let tag_list: Vec<String> = tags
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let filters = templates::SearchFilters {
        tags: tag_list,
        categories: Vec::new(),
        verified_only: verified,
        featured_only: false,
        hide_spam: false,
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

    // Pagination only kicks in when the caller opts in via --limit or
    // --cursor, so plain `template search` keeps showing every match.
    let page = match limit.or(cursor.is_some().then_some(DEFAULT_PAGE_LIMIT)) {
        Some(limit) => Some(templates::paginate(
            &results,
            cursor.as_deref(),
            limit,
            |r| r.entry.name.as_str(),
        )?),
        None => None,
    };
    let shown: &[templates::SearchResult] = match &page {
        Some(page) => &page.items,
        None => &results,
    };

    p::kv("Matches", &results.len().to_string());
    println!();

    if shown.is_empty() {
        p::info("No more results on this page.");
        return Ok(());
    }

    for (i, result) in shown.iter().enumerate() {
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
        if i + 1 < shown.len() {
            println!();
        }
    }

    print_pagination_footer(page.as_ref());

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
    if let Some(ref repo) = template.repository_url {
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

async fn optimize(path: PathBuf, name: Option<String>) -> Result<()> {
    let analysis = template_performance::analyze_template_directory(&path, name.as_deref())?;

    p::header(&format!(
        "Template Performance Analysis: {}",
        analysis.template_name
    ));
    p::separator();
    p::kv("Path", &analysis.path);
    p::kv("Overall score", &format!("{}/100", analysis.overall_score));
    p::kv(
        "Estimated gas reduction",
        &format!("{}%", analysis.estimated_gas_reduction_percent),
    );
    p::kv(
        "Estimated speedup",
        &format!("{}%", analysis.estimated_speedup_percent),
    );
    p::kv(
        "Estimated memory savings",
        &format!("{}%", analysis.estimated_memory_savings_percent),
    );
    println!();
    p::info("Optimization focus areas");
    p::kv(
        "Storage layout",
        &format!("{}/100", analysis.storage_layout_score),
    );
    p::kv(
        "Function efficiency",
        &format!("{}/100", analysis.function_efficiency_score),
    );
    p::kv(
        "Loop optimization",
        &format!("{}/100", analysis.loop_optimization_score),
    );
    p::kv(
        "External call optimization",
        &format!("{}/100", analysis.external_call_score),
    );
    p::kv(
        "Batch operations",
        &format!("{}/100", analysis.batch_operations_score),
    );
    println!();
    p::info("Benchmark summary");
    p::kv("Summary", &analysis.benchmark_summary);
    println!();
    if analysis.suggestions.is_empty() {
        p::info("No optimization opportunities detected.");
    } else {
        p::info("Actionable suggestions");
        for suggestion in analysis.suggestions {
            println!(
                "  • [{}] {}",
                suggestion.priority.to_uppercase(),
                suggestion.title
            );
            println!("    {}", suggestion.detail);
            println!("    Impact: {}", suggestion.estimated_impact);
        }
    }

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

async fn fetch(
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
    p::success(&format!("Template '{}' installed", entry.name));
    p::kv_accent("Name", &entry.name);
    p::kv("Version", &entry.version);
    p::kv("Source", &entry.source.to_string());
    if let Some(ref path) = entry.path {
        p::kv("Local path", path);
    }
    // Record this install for community-learning / personalisation.
    let _ = crate::utils::template_recommender::record_usage(&entry.name, "install");
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
                Ok(report) => {
                    p::success(&format!("  {} updated", tpl_name));
                    p::kv("Impact", &report.impact.severity);
                    if report.impact.breaking_changes {
                        p::warn(&format!("  {} has breaking changes", tpl_name));
                    }
                }
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
    let report = templates::update_installed_template(&name).await?;
    println!();
    p::success(&format!("Template '{}' updated", name));
    p::kv("Compatibility", &report.compatibility);
    p::kv("Impact", &report.impact.summary);
    for guidance in &report.migration_guidance {
        p::info(guidance);
    }
    Ok(())
}

async fn rollback(name: String) -> Result<()> {
    p::header(&format!("Template Rollback: {}", name));
    p::step(1, 1, "Restoring the last tracked template state...");
    let report = templates::rollback_installed_template(&name).await?;
    println!();
    p::success(&format!("Template '{}' rolled back", name));
    p::kv("Backup", report.backup_path.as_deref().unwrap_or("n/a"));
    p::kv("Compatibility", &report.compatibility);
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

// ─── template validate ────────────────────────────────────────────────────────

/// Check a registry document against `templates/registry.schema.json` and
/// report every problem, each anchored to the field that caused it.
///
/// With no path it checks the registry the CLI would actually load: the local
/// registry, or the bundled one when no local registry exists yet.
fn template_validate(path: Option<std::path::PathBuf>, json: bool) -> Result<()> {
    let report = match path {
        Some(path) => {
            if !path.exists() {
                anyhow::bail!("No such file: {}", path.display());
            }
            templates::validate_registry_file(&path)?
        }
        None => {
            let local = templates::active_registry_path()?;
            if local.exists() {
                templates::validate_registry_file(&local)?
            } else {
                templates::validate_bundled_registry()?
            }
        }
    };

    if json {
        output::print_json(&serde_json::json!({
            "origin": report.origin,
            "valid": report.is_valid(),
            "errors": report.errors.iter().map(|issue| serde_json::json!({
                "field": issue.field,
                "message": issue.message,
            })).collect::<Vec<_>>(),
            "warnings": report.warnings.iter().map(|issue| serde_json::json!({
                "field": issue.field,
                "message": issue.message,
            })).collect::<Vec<_>>(),
        }))?;
    } else {
        p::header(&format!("Validating {}", report.origin));
        for issue in &report.errors {
            println!(
                "  {} {}",
                "✗".red().bold(),
                issue.to_string().bright_white()
            );
        }
        for issue in &report.warnings {
            println!("  {} {}", "⚠".yellow().bold(), issue);
        }
        if report.is_valid() {
            p::success(&format!(
                "Matches the registry schema{}",
                if report.warnings.is_empty() {
                    String::new()
                } else {
                    format!(" ({} warning(s))", report.warnings.len())
                }
            ));
        }
    }

    if report.is_valid() {
        Ok(())
    } else {
        anyhow::bail!(
            "{} field(s) do not match the template registry schema",
            report.errors.len()
        )
    }
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
                    .clone()
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

async fn template_customize(path: PathBuf, requirements: String) -> Result<()> {
    p::header("Template Customization (AI)");
    p::kv("Template Path", &path.display().to_string());
    p::kv("Requirements", &requirements);
    println!();

    p::step(1, 3, "Analyzing template structure...");
    p::step(2, 3, "Generating AI customizations...");
    let result = template_customization_ai::customize_template(&path, &requirements).await?;
    p::step(3, 3, "Validating changes...");

    println!();
    if result.success {
        p::success("Customization successful!");
    } else {
        p::warn("Customization completed with warnings!");
    }

    println!("\nChanges made:");
    for change in &result.changes {
        println!("  - {}", change);
    }

    println!("\nValidation report:");
    println!("{}", result.validation_report);

    Ok(())
}

async fn template_customize_history(path: PathBuf) -> Result<()> {
    p::header("Template Customization History");
    let history = template_customization_ai::get_customization_history(&path).await?;

    if history.entries.is_empty() {
        p::info("No customization history found for this template");
        return Ok(());
    }

    for (i, entry) in history.entries.iter().enumerate() {
        println!("\n--- Entry {} ---", i);
        p::kv("Timestamp", &entry.timestamp);
        p::kv("Requirements", &entry.requirements);
        println!("Changes made:");
        println!("{}", entry.changes_made);
    }

    Ok(())
}

async fn template_customize_rollback(path: PathBuf, index: Option<usize>) -> Result<()> {
    p::header("Template Customization Rollback");
    template_customization_ai::rollback_customization(&path, index).await?;
    p::success("Rollback successful!");
    Ok(())
}
