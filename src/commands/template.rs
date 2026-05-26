use crate::utils::{print as p, templates};
use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum TemplateCommands {
    Search {
        query: String,
        #[arg(long)]
        tags: Option<String>,
    },
    List,
    Show {
        name: String,
    },
    Publish {
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        author: Option<String>,
        #[arg(long)]
        tags: Option<String>,
        #[arg(long, default_value = "1.0.0")]
        version: String,
    },
    Remove {
        name: String,
    },
    Init,
}

pub fn handle(cmd: TemplateCommands) -> Result<()> {
    match cmd {
        TemplateCommands::Publish { path, .. } => publish(path),
        TemplateCommands::List => list(),
        TemplateCommands::Search { query, tags } => search(query, tags),
        TemplateCommands::Show { name } => show(name),
        TemplateCommands::Remove { name } => remove(name),
        TemplateCommands::Init => init(),
    }
}

fn publish(path: PathBuf) -> Result<()> {
    let template = templates::publish_template(&path)?;

    p::header("Template Publish");
    p::success("Template registered successfully");
    p::kv_accent("Name", &template.name);
    p::kv("Version", &template.version);
    p::kv("Source", &template.source.to_string());
    if !template.tags.is_empty() {
        p::kv("Tags", &template.tags.join(", "));
    }
    if let Some(path) = template.path.as_ref() {
        p::kv("Path", path);
    }

    Ok(())
}

fn list() -> Result<()> {
    let registry = templates::load_registry()?;
    p::header("Template Registry");
    if registry.templates.is_empty() {
        p::info("No templates found. Publish one with: starforge template publish <path>");
        return Ok(());
    }

    for (i, template) in registry.templates.iter().enumerate() {
        println!("  {:>2}. {}@{}", i + 1, template.name, template.version);
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

fn search(query: String, tags: Option<String>) -> Result<()> {
    let parsed_tags = tags.map(|value| {
        value
            .split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
    });

    let results = templates::search_templates(&query, parsed_tags.as_deref())?;
    p::header(&format!("Template search results for '{}'", query));
    if results.is_empty() {
        p::info("No templates matched that query.");
        return Ok(());
    }

    for (i, template) in results.iter().enumerate() {
        println!("  {:>2}. {}@{}", i + 1, template.name, template.version);
        p::kv("Description", &template.description);
        p::kv("Source", &template.source.to_string());
        if !template.tags.is_empty() {
            p::kv("Tags", &template.tags.join(", "));
        }
        if i + 1 < results.len() {
            println!();
        }
    }

    Ok(())
}

fn show(name: String) -> Result<()> {
    let template = templates::get_template(&name)?;
    p::header(&format!("Template: {}", template.name));
    p::kv("Version", &template.version);
    p::kv("Description", &template.description);
    p::kv("Author", &template.author);
    p::kv("Source", &template.source.to_string());
    p::kv("Verified", if template.verified { "yes" } else { "no" });
    p::kv("Downloads", &template.downloads.to_string());
    if !template.tags.is_empty() {
        p::kv("Tags", &template.tags.join(", "));
    }
    if let Some(path) = template.path.as_ref() {
        p::kv("Path", path);
    }
    Ok(())
}

fn remove(name: String) -> Result<()> {
    templates::remove_template(&name)?;
    p::success(&format!("Removed template '{}'", name));
    Ok(())
}

fn init() -> Result<()> {
    templates::initialize_registry()?;
    p::success("Template registry initialized");
    Ok(())
}
