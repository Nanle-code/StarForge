use crate::utils::{interactive, print as p, registry, templates};
use anyhow::Result;
use base64::Engine as _;
use clap::Subcommand;
use std::path::PathBuf;

/// Env var checked before prompting for a registry email, so CI can log in
/// or sign up headlessly. `--email` takes precedence when both are set.
const ENV_REGISTRY_EMAIL: &str = "STARFORGE_REGISTRY_EMAIL";
/// Env var checked before prompting for a registry username during signup.
const ENV_REGISTRY_USERNAME: &str = "STARFORGE_REGISTRY_USERNAME";
/// Env var checked before prompting for a registry password (login/signup).
const ENV_REGISTRY_PASSWORD: &str = "STARFORGE_REGISTRY_PASSWORD";

/// Resolve a text field from an explicit CLI value, then an env var, then
/// (only when interactive) a prompt — failing clearly instead of blocking
/// when none of those are available in a non-interactive environment.
fn resolve_text_input(
    explicit: Option<String>,
    env_var: &str,
    prompt_label: &str,
    what: &str,
    alternative: &str,
) -> Result<String> {
    if let Some(value) = explicit {
        return Ok(value);
    }
    if let Ok(value) = std::env::var(env_var) {
        if !value.trim().is_empty() {
            return Ok(value);
        }
    }

    interactive::ensure_interactive(what, alternative)?;

    dialoguer::Input::new()
        .with_prompt(prompt_label)
        .interact()
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", prompt_label.to_lowercase(), e))
}

/// Resolve an existing password (login) from [`ENV_REGISTRY_PASSWORD`] or a
/// prompt, failing clearly when neither is available non-interactively.
fn resolve_login_password() -> Result<String> {
    if let Ok(pwd) = std::env::var(ENV_REGISTRY_PASSWORD) {
        return Ok(pwd);
    }

    interactive::ensure_interactive(
        "a registry password",
        &format!("Set {ENV_REGISTRY_PASSWORD} to supply one headlessly."),
    )?;

    Ok(dialoguer::Password::new()
        .with_prompt("Password")
        .interact()?)
}

/// Resolve a new password (signup) from [`ENV_REGISTRY_PASSWORD`] or a pair
/// of prompts, enforcing the same length rule either way.
fn resolve_new_password() -> Result<String> {
    if let Ok(pwd) = std::env::var(ENV_REGISTRY_PASSWORD) {
        if pwd.len() < 8 {
            anyhow::bail!("Password must be at least 8 characters");
        }
        return Ok(pwd);
    }

    interactive::ensure_interactive(
        "a new registry password",
        &format!("Set {ENV_REGISTRY_PASSWORD} to supply one headlessly."),
    )?;

    let password = dialoguer::Password::new()
        .with_prompt("Password")
        .interact()?;
    let password_confirm = dialoguer::Password::new()
        .with_prompt("Confirm password")
        .interact()?;

    if password != password_confirm {
        anyhow::bail!("Passwords do not match");
    }
    if password.len() < 8 {
        anyhow::bail!("Password must be at least 8 characters");
    }

    Ok(password)
}

#[derive(Subcommand)]
pub enum RegistryCommands {
    /// Search the remote template registry
    Search {
        /// Search query
        #[arg(default_value = "")]
        query: String,
        /// Filter by tags (comma-separated)
        #[arg(long)]
        tags: Option<String>,
        /// Only verified templates
        #[arg(long)]
        verified: bool,
        /// Minimum quality score (0-100)
        #[arg(long)]
        min_quality: Option<u8>,
        /// Number of results to show
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Get details of a remote template
    Info {
        /// Template name
        name: String,
        /// Template version (defaults to latest)
        #[arg(long)]
        version: Option<String>,
    },
    /// Log in to the remote registry to publish templates
    Login {
        /// Email address
        #[arg(long)]
        email: Option<String>,
    },
    /// Sign up for a new registry account
    Signup {
        /// Email address
        #[arg(long)]
        email: Option<String>,
        /// Username
        #[arg(long)]
        username: Option<String>,
    },
    /// Log out from the remote registry
    Logout,
    /// Publish a template to the remote registry
    Publish {
        /// Path to the template directory
        path: PathBuf,
        /// Template name (defaults to directory name)
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
        /// License identifier
        #[arg(long)]
        license: Option<String>,
        /// Repository URL
        #[arg(long)]
        repository: Option<String>,
        /// Homepage URL
        #[arg(long)]
        homepage: Option<String>,
    },
    /// Download and install a template from remote registry
    Install {
        /// Template name
        name: String,
        /// Version (defaults to latest)
        #[arg(long)]
        version: Option<String>,
    },
    /// Rate or review a template
    Review {
        /// Template name
        name: String,
        /// Rating (1-5 stars)
        #[arg(long)]
        rating: u8,
        /// Review comment
        #[arg(long)]
        comment: Option<String>,
    },
    /// Show authentication status
    Status,
    /// Configure remote registry settings
    Config {
        /// Registry URL
        #[arg(long)]
        url: Option<String>,
    },
}

pub async fn handle(cmd: RegistryCommands) -> Result<()> {
    match cmd {
        RegistryCommands::Search {
            query,
            tags,
            verified,
            min_quality,
            limit,
        } => search(query, tags, verified, min_quality, limit).await,
        RegistryCommands::Info { name, version } => info(name, version).await,
        RegistryCommands::Login { email } => login(email).await,
        RegistryCommands::Signup { email, username } => signup(email, username).await,
        RegistryCommands::Logout => logout(),
        RegistryCommands::Publish {
            path,
            name,
            description,
            author,
            tags,
            version,
            license,
            repository,
            homepage,
        } => {
            publish(
                path,
                name,
                description,
                author,
                tags,
                version,
                license,
                repository,
                homepage,
            )
            .await
        }
        RegistryCommands::Install { name, version } => install(name, version).await,
        RegistryCommands::Review {
            name,
            rating,
            comment,
        } => review(name, rating, comment).await,
        RegistryCommands::Status => status(),
        RegistryCommands::Config { url } => config(url),
    }
}

async fn search(
    query: String,
    tags: Option<String>,
    verified: bool,
    min_quality: Option<u8>,
    limit: u32,
) -> Result<()> {
    p::info("Searching remote registry...");

    let config = registry::load_registry_config()?;
    let client = registry::RegistryClient::new(config.url, config.token);

    let tag_list = tags.as_ref().map(|t| {
        t.split(',')
            .map(|s| s.trim().to_string())
            .collect::<Vec<_>>()
    });

    let req = registry::SearchRequest {
        query: query.clone(),
        tags: tag_list,
        verified: if verified { Some(true) } else { None },
        min_quality,
        limit: Some(limit),
        offset: Some(0),
    };

    let resp = client.search(&req).await?;

    if resp.results.is_empty() {
        p::info(&format!("No templates found matching '{}'", query));
        return Ok(());
    }

    p::success(&format!("Found {} templates", resp.total));
    println!();

    for (idx, tpl) in resp.results.iter().enumerate() {
        let badges = if tpl.verified {
            " [VERIFIED]".to_string()
        } else {
            String::new()
        };

        println!(
            "  {}. {} v{}{}",
            idx + 1,
            colored::Colorize::cyan(tpl.name.as_str()),
            tpl.version,
            badges
        );
        println!("     {}", tpl.description);
        println!(
            "     by {} | ⭐ {:.1} ({} reviews) | ↓ {} | {}",
            tpl.author,
            tpl.ratings.average_rating,
            tpl.ratings.review_count,
            tpl.downloads,
            tpl.tags.join(", ")
        );
        println!();
    }

    Ok(())
}

async fn info(name: String, version: Option<String>) -> Result<()> {
    p::info(&format!(
        "Fetching template info for '{}'{}",
        name,
        version
            .as_ref()
            .map(|v| format!(" v{}", v))
            .unwrap_or_default()
    ));

    let config = registry::load_registry_config()?;
    let client = registry::RegistryClient::new(config.url, config.token);

    let tpl = client.get_template(&name, version.as_deref()).await?;

    println!();
    println!(
        "{} v{}",
        colored::Colorize::cyan(tpl.name.as_str()),
        tpl.version
    );
    println!("{}", tpl.description);
    println!();
    println!("Author:       {}", tpl.author);
    if let Some(license) = &tpl.license {
        println!("License:      {}", license);
    }
    if let Some(repo) = &tpl.repository {
        println!("Repository:   {}", repo);
    }
    if let Some(docs) = &tpl.documentation {
        println!("Documentation: {}", docs);
    }
    println!("Downloads:    {}", tpl.downloads);
    println!(
        "Rating:       ⭐ {:.1} ({} reviews)",
        tpl.ratings.average_rating, tpl.ratings.review_count
    );
    println!("Tags:         {}", tpl.tags.join(", "));
    println!();

    Ok(())
}

async fn login(email: Option<String>) -> Result<()> {
    let email = resolve_text_input(
        email,
        ENV_REGISTRY_EMAIL,
        "Email",
        "a registry email",
        &format!("Pass --email or set {ENV_REGISTRY_EMAIL}."),
    )?;

    if email.is_empty() {
        anyhow::bail!("Email is required");
    }

    let password = resolve_login_password()?;

    p::info("Authenticating with remote registry...");

    let config = registry::load_registry_config()?;
    let client = registry::RegistryClient::new(config.url.clone(), None);

    let resp = client.authenticate(&email, &password).await?;

    if !resp.success {
        anyhow::bail!("Authentication failed: {}", resp.message);
    }

    let token = resp
        .token
        .ok_or_else(|| anyhow::anyhow!("No token received"))?;
    let username = resp
        .username
        .ok_or_else(|| anyhow::anyhow!("No username received"))?;

    // Save credentials
    let mut new_config = config;
    new_config.token = Some(token);
    new_config.username = Some(username.clone());
    new_config.email = Some(email.clone());
    registry::save_registry_config(&new_config)?;

    p::success(&format!("Logged in as '{}'", username));

    Ok(())
}

async fn signup(email: Option<String>, username: Option<String>) -> Result<()> {
    let email = resolve_text_input(
        email,
        ENV_REGISTRY_EMAIL,
        "Email",
        "a registry email",
        &format!("Pass --email or set {ENV_REGISTRY_EMAIL}."),
    )?;

    let username = resolve_text_input(
        username,
        ENV_REGISTRY_USERNAME,
        "Username",
        "a registry username",
        &format!("Pass --username or set {ENV_REGISTRY_USERNAME}."),
    )?;

    let password = resolve_new_password()?;

    p::info("Creating account...");

    let config = registry::load_registry_config()?;
    let client = registry::RegistryClient::new(config.url.clone(), None);

    let resp = client.signup(&email, &username, &password).await?;

    if !resp.success {
        anyhow::bail!("Signup failed: {}", resp.message);
    }

    let token = resp
        .token
        .ok_or_else(|| anyhow::anyhow!("No token received"))?;

    // Save credentials
    let mut new_config = config;
    new_config.token = Some(token);
    new_config.username = Some(username.clone());
    new_config.email = Some(email.clone());
    registry::save_registry_config(&new_config)?;

    p::success(&format!("Account created and logged in as '{}'", username));

    Ok(())
}

fn logout() -> Result<()> {
    let mut config = registry::load_registry_config()?;
    config.token = None;
    config.username = None;
    config.email = None;
    registry::save_registry_config(&config)?;

    p::success("Logged out from remote registry");

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
    license: Option<String>,
    repository: Option<String>,
    homepage: Option<String>,
) -> Result<()> {
    let config = registry::load_registry_config()?;
    if config.token.is_none() {
        anyhow::bail!("Not logged in. Use 'starforge registry login' first.");
    }

    p::info("Preparing template for publication...");

    // Validate template structure
    let template_name = name.clone().unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("template")
            .to_string()
    });

    let description = description.ok_or_else(|| anyhow::anyhow!("Description is required"))?;
    let author = author.ok_or_else(|| anyhow::anyhow!("Author is required"))?;

    templates::validate_template_structure(&path, &template_name, &description, &author, &version)?;

    // Create zip archive
    p::info("Creating archive...");
    let temp_dir = std::env::temp_dir().join(format!("starforge-pub-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir)?;
    let zip_path = temp_dir.join("template.zip");

    create_zip_archive(&path, &zip_path)?;

    // Read and encode as base64
    let archive_bytes = std::fs::read(&zip_path)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&archive_bytes);

    let tag_list = tags
        .map(|t| {
            t.split(',')
                .map(|s| s.trim().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let publish_req = registry::PublishTemplateRequest {
        name: template_name,
        version,
        description,
        author,
        tags: tag_list,
        license,
        repository,
        homepage,
        documentation: None,
        cli_version_min: None,
        cli_version_max: None,
        content: encoded,
    };

    p::info("Publishing to remote registry...");

    let client = registry::RegistryClient::new(config.url, config.token);
    let resp = client.publish(&publish_req).await?;

    if !resp.success {
        anyhow::bail!("Publish failed: {}", resp.message);
    }

    if let Some(url) = resp.url {
        p::success(&format!("Template published! View it at: {}", url));
    } else {
        p::success(&resp.message);
    }

    // Cleanup
    std::fs::remove_dir_all(&temp_dir).ok();

    Ok(())
}

async fn install(name: String, version: Option<String>) -> Result<()> {
    p::info(&format!(
        "Downloading template '{}'{}",
        name,
        version
            .as_ref()
            .map(|v| format!(" v{}", v))
            .unwrap_or_default()
    ));

    let config = registry::load_registry_config()?;
    let client = registry::RegistryClient::new(config.url, config.token);

    let tpl = client.get_template(&name, version.as_deref()).await?;

    // Download archive
    let archive_bytes = client
        .download_template(&tpl.download_url, tpl.expected_sha256.as_deref())
        .await?;

    // Save to temp and extract
    let temp_dir = std::env::temp_dir().join(format!("starforge-dl-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&temp_dir)?;
    let zip_path = temp_dir.join("template.zip");
    std::fs::write(&zip_path, archive_bytes)?;

    p::info("Extracting and installing...");

    let extract_dir = temp_dir.join("extracted");
    templates::extract_zip_archive(&zip_path, &extract_dir)?;
    let root = templates::normalize_template_root(&extract_dir)?;

    // Install to local registry
    templates::install_template_package(
        &root,
        name.clone(),
        tpl.description,
        tpl.author,
        tpl.tags,
        tpl.version,
        None,
        None,
    )
    .await?;

    p::success(&format!("Template '{}' installed successfully", name));

    // Cleanup
    std::fs::remove_dir_all(&temp_dir).ok();

    Ok(())
}

async fn review(name: String, rating: u8, comment: Option<String>) -> Result<()> {
    if !(1..=5).contains(&rating) {
        anyhow::bail!("Rating must be between 1 and 5");
    }

    let config = registry::load_registry_config()?;
    if config.token.is_none() {
        anyhow::bail!("Not logged in. Use 'starforge registry login' first.");
    }

    p::info("Posting review...");

    let client = registry::RegistryClient::new(config.url, config.token);
    let tpl = client.get_template(&name, None).await?;
    let resp = client
        .post_review(&tpl.id, rating, comment.as_deref())
        .await?;

    if !resp.success {
        anyhow::bail!("Failed to post review: {}", resp.message);
    }

    p::success(&format!("Review posted: ⭐ {} for '{}'", rating, name));

    Ok(())
}

fn status() -> Result<()> {
    let config = registry::load_registry_config()?;

    println!();
    println!("Registry: {}", config.url);

    if let Some(username) = config.username {
        println!(
            "Status:   {} (logged in as '{}')",
            colored::Colorize::green("✓"),
            username
        );
        if let Some(email) = config.email {
            println!("Email:    {}", email);
        }
    } else {
        println!("Status:   {} (not logged in)", colored::Colorize::red("✗"));
    }

    println!();

    Ok(())
}

fn config(url: Option<String>) -> Result<()> {
    let mut config = registry::load_registry_config()?;

    if let Some(new_url) = url {
        config.url = new_url;
        registry::save_registry_config(&config)?;
        p::success("Registry URL updated");
    }

    println!();
    println!("Registry Configuration:");
    println!("  URL: {}", config.url);
    println!();

    Ok(())
}

/// Create a zip archive from a template directory.
fn create_zip_archive(source: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    use zip::ZipWriter;

    let file = std::fs::File::create(dest)?;
    let mut zip = ZipWriter::new(file);
    let options = zip::write::FileOptions::default();

    let mut entries = Vec::new();
    collect_files(source, &mut entries, &mut vec![".git", ".DS_Store"])?;

    for entry in entries {
        let rel = entry.strip_prefix(source)?;
        let name = rel.to_string_lossy().replace('\\', "/");

        if entry.is_dir() {
            zip.add_directory(format!("{}/", name), options)?;
        } else {
            zip.start_file(name, options)?;
            let mut f = std::fs::File::open(&entry)?;
            std::io::copy(&mut f, &mut zip)?;
        }
    }

    zip.finish()?;
    Ok(())
}

fn collect_files(
    dir: &std::path::Path,
    out: &mut Vec<std::path::PathBuf>,
    skip_names: &mut Vec<&str>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();

        if skip_names.contains(&file_name.to_string_lossy().as_ref()) {
            continue;
        }

        if path.is_dir() {
            out.push(path.clone());
            collect_files(&path, out, skip_names)?;
        } else {
            out.push(path);
        }
    }

    Ok(())
}

#[cfg(test)]
mod ci_prompt_tests {
    use super::*;

    fn clear_env() {
        std::env::remove_var(ENV_REGISTRY_EMAIL);
        std::env::remove_var(ENV_REGISTRY_USERNAME);
        std::env::remove_var(ENV_REGISTRY_PASSWORD);
        std::env::remove_var(interactive::ENV_NON_INTERACTIVE);
        std::env::remove_var("CI");
    }

    #[test]
    fn resolve_text_input_prefers_explicit_value_over_env_in_ci() {
        let _guard = interactive::env_test_lock().lock().unwrap();
        clear_env();
        std::env::set_var("CI", "1");
        std::env::set_var(ENV_REGISTRY_EMAIL, "env@example.com");

        let email = resolve_text_input(
            Some("flag@example.com".to_string()),
            ENV_REGISTRY_EMAIL,
            "Email",
            "a registry email",
            "Pass --email.",
        )
        .unwrap();
        assert_eq!(email, "flag@example.com");

        clear_env();
    }

    #[test]
    fn resolve_text_input_falls_back_to_env_var_in_ci() {
        let _guard = interactive::env_test_lock().lock().unwrap();
        clear_env();
        std::env::set_var("CI", "1");
        std::env::set_var(ENV_REGISTRY_EMAIL, "env@example.com");

        let email = resolve_text_input(
            None,
            ENV_REGISTRY_EMAIL,
            "Email",
            "a registry email",
            "Pass --email.",
        )
        .unwrap();
        assert_eq!(email, "env@example.com");

        clear_env();
    }

    #[test]
    fn resolve_text_input_fails_fast_in_ci_without_fallback() {
        let _guard = interactive::env_test_lock().lock().unwrap();
        clear_env();
        std::env::set_var("CI", "1");

        let err = resolve_text_input(
            None,
            ENV_REGISTRY_EMAIL,
            "Email",
            "a registry email",
            &format!("Pass --email or set {ENV_REGISTRY_EMAIL}."),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains(ENV_REGISTRY_EMAIL), "got: {}", err);

        clear_env();
    }

    #[test]
    fn resolve_login_password_uses_env_fallback_in_ci() {
        let _guard = interactive::env_test_lock().lock().unwrap();
        clear_env();
        std::env::set_var("CI", "1");
        std::env::set_var(ENV_REGISTRY_PASSWORD, "hunter2-but-longer");

        let pwd = resolve_login_password().unwrap();
        assert_eq!(pwd, "hunter2-but-longer");

        clear_env();
    }

    #[test]
    fn resolve_login_password_fails_fast_in_ci_without_fallback() {
        let _guard = interactive::env_test_lock().lock().unwrap();
        clear_env();
        std::env::set_var("CI", "1");

        let err = resolve_login_password().unwrap_err().to_string();
        assert!(err.contains(ENV_REGISTRY_PASSWORD), "got: {}", err);

        clear_env();
    }

    #[test]
    fn resolve_new_password_env_fallback_still_enforces_min_length() {
        let _guard = interactive::env_test_lock().lock().unwrap();
        clear_env();
        std::env::set_var("CI", "1");
        std::env::set_var(ENV_REGISTRY_PASSWORD, "short");

        let err = resolve_new_password().unwrap_err().to_string();
        assert!(err.contains("8 characters"), "got: {}", err);

        clear_env();
    }
}
