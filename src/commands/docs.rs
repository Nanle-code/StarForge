use crate::utils::{ai_docs, doc_generator, docs, print as p};
use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum DocsCommands {
    /// Review project documentation completeness and stale references
    Review {
        #[arg(default_value = ".")]
        dir: PathBuf,
        #[arg(long)]
        json: bool,
    },

    /// Maintain API docs, tutorials, examples, architecture docs, and rustdoc suggestions
    Maintain {
        #[arg(default_value = ".")]
        dir: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "docs/generated")]
        output: PathBuf,
        /// Fail if documentation completeness is below this percentage
        #[arg(long, default_value_t = 80.0)]
        min_completeness: f64,
    },

    /// Generate comprehensive documentation (AI-assisted from source or metadata)
    Generate {
        /// On-chain contract ID (or local identifier)
        contract: String,
        /// Human-friendly contract name
        #[arg(long)]
        name: Option<String>,
        /// Short description of the contract
        #[arg(long)]
        description: Option<String>,
        /// Path to contract source (`.rs`) for rustdoc-based AI documentation
        #[arg(long)]
        source: Option<PathBuf>,
        /// Network (testnet / mainnet)
        #[arg(long, default_value = "testnet")]
        network: String,
        /// Documentation version
        #[arg(long, default_value = "1.0.0")]
        version: String,
        /// Disable AI enrichment (still extracts rustdoc; skips heuristic/LLM prose)
        #[arg(long, default_value_t = false)]
        no_ai: bool,
        /// Languages for usage examples: rust,ts,python,go (comma-separated)
        #[arg(long, default_value = "rust,ts,python")]
        lang: String,
        /// Write Markdown documentation to this path
        #[arg(long)]
        output: Option<PathBuf>,
        /// Write rustdoc stub comments for undocumented items
        #[arg(long)]
        rustdoc_out: Option<PathBuf>,
    },

    /// Extract doc comments from a Soroban contract source file
    Extract {
        /// Path to the contract source file (.rs) or directory
        source: String,
        /// Output file path for extracted JSON (stdout if omitted)
        #[arg(long)]
        output: Option<String>,
        /// Output format: json or markdown (default: json)
        #[arg(long, default_value = "json")]
        format: String,
    },

    /// Show stored documentation for a contract
    Show {
        /// Contract ID
        contract: String,
        /// Specific version (latest if omitted)
        #[arg(long)]
        version: Option<String>,
    },

    /// List all documented contracts
    List,

    /// Full-text search across all documentation
    Search {
        /// Search query
        query: String,
    },

    /// Show documentation versions for a contract
    Versions {
        /// Contract ID
        contract: String,
    },

    /// Export stored documentation as Markdown (printed to stdout)
    Export {
        /// Contract ID
        contract: String,
        /// Version to export (latest if omitted)
        #[arg(long)]
        version: Option<String>,
    },

    /// Generate HTML documentation site from a contract source file
    Html {
        /// Contract ID (used for output directory naming)
        contract: String,
        /// Contract display name
        #[arg(long)]
        name: String,
        /// Path to the contract source file (.rs)
        #[arg(long)]
        source: String,
        /// Output directory for the generated HTML site
        #[arg(long)]
        output_dir: Option<String>,
        /// Custom template directory (overrides built-in templates)
        #[arg(long)]
        template_dir: Option<String>,
    },

    /// Generate an API reference (JSON + Markdown) from a contract source file
    ApiRef {
        /// Contract ID
        contract: String,
        /// Contract display name
        #[arg(long)]
        name: String,
        /// Path to the contract source file (.rs)
        #[arg(long)]
        source: String,
        /// Documentation version
        #[arg(long, default_value = "1.0.0")]
        version: String,
        /// Output directory (defaults to ~/.starforge/docs/<contract>/)
        #[arg(long)]
        output_dir: Option<String>,
    },

    /// Publish generated HTML documentation to a destination
    Publish {
        /// Contract ID
        contract: String,
        /// Source directory containing the generated HTML site
        #[arg(long)]
        source_dir: Option<String>,
        /// Destination path or remote rsync target (e.g. user@host:/var/www/docs)
        #[arg(long)]
        dest: Option<String>,
        /// Also write a deploy.sh script for manual deployment
        #[arg(long)]
        generate_script: bool,
    },
}

pub async fn handle(cmd: DocsCommands) -> Result<()> {
    match cmd {
        DocsCommands::Review { dir, json } => review(dir, json),
        DocsCommands::Maintain {
            dir,
            name,
            output,
            min_completeness,
        } => maintain(dir, name, output, min_completeness),
        DocsCommands::Generate {
            contract,
            name,
            description,
            source,
            network,
            version,
            no_ai,
            lang,
            output,
            rustdoc_out,
        } => generate(
            contract,
            name,
            description,
            source,
            network,
            version,
            !no_ai,
            lang,
            output,
            rustdoc_out,
        ),

        DocsCommands::Extract {
            source,
            output,
            format,
        } => extract(source, output, format),

        DocsCommands::Show { contract, version } => show(contract, version),
        DocsCommands::List => list(),
        DocsCommands::Search { query } => search(query),
        DocsCommands::Versions { contract } => versions(contract),
        DocsCommands::Export { contract, version } => export(contract, version),
        DocsCommands::Html {
            contract,
            name,
            source,
            output_dir,
            template_dir,
        } => generate_html(contract, name, source, output_dir, template_dir),
        DocsCommands::ApiRef {
            contract,
            name,
            source,
            version,
            output_dir,
        } => generate_api_ref(contract, name, source, version, output_dir),
        DocsCommands::Publish {
            contract,
            source_dir,
            dest,
            generate_script,
        } => publish(contract, source_dir, dest, generate_script),
    }
}

fn review(dir: PathBuf, json: bool) -> Result<()> {
    let review = crate::utils::ai_documentation_assistant::review_project(&dir)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&review)?);
        return Ok(());
    }
    p::header("AI Documentation Review");
    p::kv("Public items", &review.public_items.to_string());
    p::kv("Documented", &review.documented_items.to_string());
    p::kv(
        "Completeness",
        &format!("{:.1}%", review.completeness_percent),
    );
    for issue in review.issues {
        println!(
            "{}:{} [{}] {} — {}",
            issue.file.display(),
            issue.line,
            issue.severity,
            issue.symbol,
            issue.message
        );
        println!("  suggestion: {}", issue.suggestion);
    }
    for stale in review.stale_references {
        p::warn(&stale);
    }
    Ok(())
}

fn maintain(
    dir: PathBuf,
    name: Option<String>,
    output: PathBuf,
    min_completeness: f64,
) -> Result<()> {
    let project_name = name.or_else(|| {
        dir.file_name()
            .and_then(|part| part.to_str())
            .map(str::to_string)
    });
    let bundle = crate::utils::ai_documentation_assistant::generate_bundle(
        &dir,
        project_name.as_deref().unwrap_or("StarForge Project"),
    )?;
    let files = crate::utils::ai_documentation_assistant::write_bundle(&bundle, &output)?;
    let suggestions =
        crate::utils::ai_documentation_assistant::render_rustdoc_suggestions(&bundle.review);
    let suggestion_path = output.join("RUSTDOC_SUGGESTIONS.rs");
    std::fs::write(&suggestion_path, suggestions)?;

    p::header("AI Documentation Maintenance");
    for file in files.iter().chain(std::iter::once(&suggestion_path)) {
        p::success(&format!("Wrote {}", file.display()));
    }
    p::kv(
        "Completeness",
        &format!("{:.1}%", bundle.review.completeness_percent),
    );
    if bundle.review.completeness_percent < min_completeness {
        anyhow::bail!(
            "Documentation completeness {:.1}% is below the required {:.1}%",
            bundle.review.completeness_percent,
            min_completeness
        );
    }
    Ok(())
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
fn generate(
    contract: String,
    name: Option<String>,
    description: Option<String>,
    source: Option<PathBuf>,
    network: String,
    version: String,
    use_ai: bool,
    lang: String,
    output: Option<PathBuf>,
    rustdoc_out: Option<PathBuf>,
) -> Result<()> {
    p::header("AI Documentation Generation");

    let display_name = name.unwrap_or_else(|| contract.clone());

    if let Some(source_path) = source {
        p::step(1, 4, &format!("Analyzing source {}", source_path.display()));
        if !source_path.exists() {
            anyhow::bail!("Source file not found: {}", source_path.display());
        }

        // Feature-flag gate: only fire the gate when AI is actually being
        // used. When the user passes `--no-ai` we still walk the source path
        // (for rustdoc extraction) but skip the AI enrichment, so the gate
        // would be a false blocker.
        if use_ai {
            crate::commands::feature_flags_cmd::require_feature("ai.docs")?;
        }

        let options = ai_docs::AiDocsOptions {
            contract_id: contract.clone(),
            name: display_name.clone(),
            description: description.clone(),
            network: network.clone(),
            version: version.clone(),
            languages: {
                let parsed = ai_docs::DocLanguage::parse_list(&lang);
                if parsed.is_empty() {
                    vec![
                        ai_docs::DocLanguage::Rust,
                        ai_docs::DocLanguage::TypeScript,
                        ai_docs::DocLanguage::Python,
                    ]
                } else {
                    parsed
                }
            },
            use_llm: use_ai,
        };

        p::step(
            2,
            4,
            if use_ai {
                "Generating AI-enriched documentation (functions, architecture, storage, security, usage)..."
            } else {
                "Extracting rustdoc comments into documentation..."
            },
        );

        let generated = ai_docs::generate_from_source(&source_path, &options)?;

        p::step(3, 4, "Persisting documentation to the docs store...");
        let entry =
            ai_docs::persist_generated(&generated, output.as_deref(), rustdoc_out.as_deref())?;

        p::step(4, 4, "Done.");
        println!();
        p::success(&format!(
            "Documentation generated for '{}' ({})",
            entry.name, generated.enrichment_mode
        ));
        p::kv("Contract", &entry.contract_id);
        p::kv("Version", &entry.version);
        p::kv("Network", &entry.network);
        p::kv("Functions", &entry.api.functions.len().to_string());
        p::kv("Storage keys", &entry.api.storage.len().to_string());
        p::kv("Sections", &entry.sections.len().to_string());
        p::kv("Enrichment", &generated.enrichment_mode);
        if let Some(path) = output {
            p::kv("Markdown", &path.display().to_string());
        }
        if let Some(path) = rustdoc_out {
            p::kv("Rustdoc stubs", &path.display().to_string());
        }
        p::info("Use `starforge docs show <contract>` to view.");
        p::info("Use `starforge docs export <contract>` for Markdown on stdout.");
        return Ok(());
    }

    // Metadata-only fallback (no source provided).
    p::step(1, 3, "Building documentation structure (metadata mode)...");
    let desc = description.unwrap_or_else(|| format!("{} Soroban contract", display_name));
    let functions = vec![
        docs::FunctionDoc {
            name: "initialize".to_string(),
            description: "Initialize the contract with an admin address.".to_string(),
            parameters: vec![docs::ParamDoc {
                name: "admin".to_string(),
                ty: "Address".to_string(),
                description: "The administrator address.".to_string(),
                required: true,
            }],
            returns: Some("bool".to_string()),
            examples: vec!["contract.initialize(&admin)".to_string()],
        },
        docs::FunctionDoc {
            name: "transfer".to_string(),
            description: "Transfer tokens between accounts.".to_string(),
            parameters: vec![
                docs::ParamDoc {
                    name: "from".to_string(),
                    ty: "Address".to_string(),
                    description: "Source address.".to_string(),
                    required: true,
                },
                docs::ParamDoc {
                    name: "to".to_string(),
                    ty: "Address".to_string(),
                    description: "Destination address.".to_string(),
                    required: true,
                },
                docs::ParamDoc {
                    name: "amount".to_string(),
                    ty: "i128".to_string(),
                    description: "Amount of tokens to transfer.".to_string(),
                    required: true,
                },
            ],
            returns: Some("bool".to_string()),
            examples: vec!["contract.transfer(&from, &to, 1_000)".to_string()],
        },
    ];

    let events = vec![docs::EventDoc {
        name: "Transfer".to_string(),
        description: "Emitted on every successful token transfer.".to_string(),
        topics: vec![
            docs::TopicDoc {
                name: "from".to_string(),
                ty: "Address".to_string(),
                description: "Source address.".to_string(),
            },
            docs::TopicDoc {
                name: "to".to_string(),
                ty: "Address".to_string(),
                description: "Destination address.".to_string(),
            },
        ],
    }];

    let storage = vec![
        docs::StorageDoc {
            key: "admin".to_string(),
            ty: "Address".to_string(),
            description: "Contract administrator.".to_string(),
        },
        docs::StorageDoc {
            key: "balances".to_string(),
            ty: "Map<Address, i128>".to_string(),
            description: "Token balances for all accounts.".to_string(),
        },
    ];

    let sections = vec![
        docs::DocSection {
            title: "Overview".to_string(),
            content: format!(
                "{} is a Soroban smart contract on {}. {}",
                display_name, network, desc
            ),
            order: 0,
        },
        docs::DocSection {
            title: "Getting Started".to_string(),
            content: format!(
                "Provide `--source path/to/lib.rs` for AI documentation generation from rustdoc.\n\
                 Deploy {} to {} and interact via the Soroban RPC.",
                display_name, network
            ),
            order: 1,
        },
        docs::DocSection {
            title: "Security".to_string(),
            content: "All state-changing operations require address-based authorization."
                .to_string(),
            order: 2,
        },
    ];

    p::step(2, 3, "Saving documentation...");
    let entry = docs::generate_documentation(
        &contract,
        &display_name,
        &desc,
        &network,
        &version,
        functions,
        events,
        storage,
        sections,
    )?;

    p::step(3, 3, "Updating index...");
    println!();
    p::success(&format!("Documentation generated for '{}'", display_name));
    p::kv("Contract", &entry.contract_id);
    p::kv("Version", &entry.version);
    p::kv("Network", &entry.network);
    p::kv("Generated", &entry.generated_at[..10]);
    p::warn("Metadata mode used sample API stubs. Re-run with `--source` for accurate docs.");
    p::info("Use `starforge docs show <contract>` to view.");
    Ok(())
}

fn extract(source: String, output: Option<String>, format: String) -> Result<()> {
    p::header("Documentation — Extract Doc Comments");

    let source_path = PathBuf::from(&source);
    p::step(1, 3, &format!("Reading source: {}", source));

    let extracted = if source_path.is_dir() {
        let mut merged = doc_generator::ExtractedDocs {
            module_doc: String::new(),
            functions: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            constants: Vec::new(),
            source_path: source.clone(),
        };
        for entry in std::fs::read_dir(&source_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let docs = doc_generator::DocCommentExtractor::extract_from_file(&path)?;
                merged.functions.extend(docs.functions);
                merged.structs.extend(docs.structs);
                merged.enums.extend(docs.enums);
                merged.constants.extend(docs.constants);
                if merged.module_doc.is_empty() {
                    merged.module_doc = docs.module_doc;
                }
            }
        }
        merged
    } else {
        doc_generator::DocCommentExtractor::extract_from_file(&source_path)?
    };

    p::step(2, 3, "Formatting output...");

    let content = match format.as_str() {
        "markdown" | "md" => {
            let api = doc_generator::ApiReferenceGenerator::from_extracted(
                &extracted,
                &source,
                &source,
                "extracted",
            );
            doc_generator::ApiReferenceGenerator::render_markdown(&api)
        }
        _ => serde_json::to_string_pretty(&extracted)?,
    };

    p::step(3, 3, "Writing output...");

    if let Some(out_path) = output {
        std::fs::write(&out_path, &content)?;
        p::success(&format!("Extracted docs written to '{}'", out_path));
    } else {
        println!("{}", content);
    }

    println!();
    p::kv("Functions found", &extracted.functions.len().to_string());
    p::kv("Structs found", &extracted.structs.len().to_string());
    p::kv("Enums found", &extracted.enums.len().to_string());
    p::kv(
        "Public functions",
        &extracted
            .functions
            .iter()
            .filter(|f| f.visibility == doc_generator::Visibility::Public)
            .count()
            .to_string(),
    );

    Ok(())
}

fn show(contract: String, version: Option<String>) -> Result<()> {
    p::header("Documentation Portal — View");

    let entry = docs::get_documentation(&contract, version.as_deref())?;

    p::separator();
    p::kv_accent("Contract", &entry.name);
    p::kv("ID", &entry.contract_id);
    p::kv("Version", &entry.version);
    p::kv("Network", &entry.network);
    p::kv("Generated", &entry.generated_at[..10]);
    p::separator();
    println!();

    for section in &entry.sections {
        println!("  {} {}", "##".dimmed(), section.title.bright_white());
        println!("  {}", section.content.dimmed());
        println!();
    }

    if !entry.api.functions.is_empty() {
        p::info("API Reference — Functions");
        for func in &entry.api.functions {
            println!("  {} `{}`", "→".cyan(), func.name.bright_white());
            println!("    {}", func.description);
            for param in &func.parameters {
                let req = if param.required {
                    "required"
                } else {
                    "optional"
                };
                println!(
                    "    • {} ({}): {} [{}]",
                    param.name, param.ty, param.description, req
                );
            }
            if let Some(ref ret) = func.returns {
                println!("    Returns: {}", ret);
            }
            println!();
        }
    }

    if !entry.api.events.is_empty() {
        p::info("API Reference — Events");
        for event in &entry.api.events {
            println!("  {} `{}`", "→".cyan(), event.name.bright_white());
            println!("    {}", event.description);
            for topic in &event.topics {
                println!("    • {} ({}): {}", topic.name, topic.ty, topic.description);
            }
            println!();
        }
    }

    if !entry.api.storage.is_empty() {
        p::info("Storage Layout");
        for s in &entry.api.storage {
            println!("  • {} ({}): {}", s.key, s.ty, s.description);
        }
    }

    println!();
    p::separator();
    Ok(())
}

fn list() -> Result<()> {
    p::header("Documentation Portal — Index");

    let index = docs::list_documentation()?;

    if index.contracts.is_empty() {
        p::info("No documentation generated yet. Use `starforge docs generate` first.");
        return Ok(());
    }

    for contract in &index.contracts {
        println!(
            "  {} {} ({} versions)",
            "→".cyan(),
            contract.name.bright_white(),
            contract.versions.len()
        );
        p::kv("Contract ID", &contract.contract_id);
        if let Some(latest) = contract.versions.first() {
            p::kv("Latest", &latest.version);
        }
        println!();
    }

    p::kv("Total", &index.contracts.len().to_string());
    Ok(())
}

fn search(query: String) -> Result<()> {
    p::header(&format!("Documentation Search: '{}'", query));

    let results = docs::search_documentation(&query)?;

    if results.is_empty() {
        p::info("No documentation matched your query.");
        return Ok(());
    }

    p::kv("Matches", &results.len().to_string());
    println!();

    for result in &results {
        println!(
            "  {} {} (score: {})",
            "→".cyan(),
            result.name.bright_white(),
            result.score
        );
        p::kv("Contract", &result.contract_id);
        p::kv("Version", &result.version);
        if !result.matched_sections.is_empty() {
            p::kv("Matched", &result.matched_sections.join(", "));
        }
        println!();
    }

    Ok(())
}

fn versions(contract: String) -> Result<()> {
    p::header("Documentation Portal — Versions");
    p::kv("Contract", &contract);

    let versions = docs::list_versions(&contract)?;

    if versions.is_empty() {
        p::info("No documentation versions found.");
        return Ok(());
    }

    println!();
    for v in &versions {
        println!("  {} v{}", "→".cyan(), v.bright_white());
    }

    println!();
    p::kv("Versions", &versions.len().to_string());
    Ok(())
}

fn export(contract: String, version: Option<String>) -> Result<()> {
    p::header("Documentation Portal — Export Markdown");
    let md = docs::render_markdown(&contract, version.as_deref())?;
    println!("{}", md);
    Ok(())
}

fn generate_html(
    contract: String,
    name: String,
    source: String,
    output_dir: Option<String>,
    template_dir: Option<String>,
) -> Result<()> {
    p::header("Documentation — Generate HTML Site");

    p::step(1, 4, &format!("Extracting doc comments from '{}'", source));
    let source_path = PathBuf::from(&source);
    let extracted = doc_generator::DocCommentExtractor::extract_from_file(&source_path)?;

    let out_dir = output_dir.map(PathBuf::from).unwrap_or_else(|| {
        crate::utils::config::config_dir()
            .join("docs")
            .join("html")
            .join(&contract)
    });

    p::step(2, 4, "Initialising template engine...");
    let mut generator = doc_generator::HtmlDocGenerator::new();
    if let Some(tmpl_dir) = template_dir {
        generator = generator.with_template_dir(&PathBuf::from(tmpl_dir))?;
    }

    p::step(
        3,
        4,
        &format!("Generating HTML site in '{}'", out_dir.display()),
    );
    generator.generate_site(&extracted, &name, &contract, &out_dir)?;

    p::step(4, 4, "Writing publish manifest...");
    doc_generator::DocPublisher::write_manifest(&out_dir, &contract, "latest")?;

    println!();
    p::success(&format!("HTML documentation generated for '{}'", name));
    p::kv("Contract", &contract);
    p::kv("Output", &out_dir.display().to_string());
    p::kv(
        "Functions documented",
        &extracted.functions.len().to_string(),
    );
    p::info(&format!(
        "Open '{}' to view the documentation.",
        out_dir.join("index.html").display()
    ));

    Ok(())
}

fn generate_api_ref(
    contract: String,
    name: String,
    source: String,
    version: String,
    output_dir: Option<String>,
) -> Result<()> {
    p::header("Documentation — Generate API Reference");

    p::step(1, 3, &format!("Extracting from '{}'", source));
    let source_path = PathBuf::from(&source);
    let extracted = doc_generator::DocCommentExtractor::extract_from_file(&source_path)?;

    p::step(2, 3, "Building API reference...");
    let api_ref = doc_generator::ApiReferenceGenerator::from_extracted(
        &extracted, &contract, &name, &version,
    );

    let out_dir = output_dir.map(PathBuf::from).unwrap_or_else(|| {
        crate::utils::config::config_dir()
            .join("docs")
            .join(&contract)
    });

    std::fs::create_dir_all(&out_dir)?;

    p::step(3, 3, "Writing JSON and Markdown...");
    let json_path = out_dir.join("api-reference.json");
    let md_path = out_dir.join("api-reference.md");

    doc_generator::ApiReferenceGenerator::save_json(&api_ref, &json_path)?;
    std::fs::write(
        &md_path,
        doc_generator::ApiReferenceGenerator::render_markdown(&api_ref),
    )?;

    println!();
    p::success(&format!(
        "API reference generated for '{}' v{}",
        name, version
    ));
    p::kv("JSON", &json_path.display().to_string());
    p::kv("Markdown", &md_path.display().to_string());
    p::kv("Functions", &api_ref.functions.len().to_string());
    p::kv("Events", &api_ref.events.len().to_string());

    Ok(())
}

fn publish(
    contract: String,
    source_dir: Option<String>,
    dest: Option<String>,
    generate_script: bool,
) -> Result<()> {
    p::header("Documentation — Publish");

    let src = source_dir.map(PathBuf::from).unwrap_or_else(|| {
        crate::utils::config::config_dir()
            .join("docs")
            .join("html")
            .join(&contract)
    });

    if !src.exists() {
        p::error(&format!(
            "Source directory '{}' does not exist. Run `starforge docs html` first.",
            src.display()
        ));
        return Ok(());
    }

    p::step(1, 3, &format!("Source: {}", src.display()));

    if let Some(ref destination) = dest {
        p::step(2, 3, &format!("Copying to '{}'", destination));
        let dest_path = PathBuf::from(destination);
        doc_generator::DocPublisher::publish_to_dir(&src, &dest_path)?;
        p::success(&format!("Documentation published to '{}'", destination));
    } else {
        p::step(2, 3, "No --dest specified; skipping file copy");
    }

    p::step(3, 3, "Finalising...");
    doc_generator::DocPublisher::write_manifest(&src, &contract, "latest")?;

    if generate_script {
        let endpoint = dest.as_deref().unwrap_or("user@host:/var/www/docs");
        let script = doc_generator::DocPublisher::generate_deploy_script(&src, endpoint)?;
        p::kv("Deploy script", &script.display().to_string());
    }

    println!();
    p::kv("Contract", &contract);
    p::kv("Source", &src.display().to_string());
    if let Some(d) = &dest {
        p::kv("Destination", d);
    }
    p::info("Manifest written to manifest.json in the source directory.");

    Ok(())
}
