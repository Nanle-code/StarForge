use crate::utils::print as p;
use crate::utils::templates;
use anyhow::{Context, Result};
use clap::Subcommand;
use colored::*;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum NewCommands {
    Contract {
        #[arg(required_unless_present = "search")]
        name: Option<String>,
        #[arg(long, default_value = "hello-world")]
        template: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        search: Option<String>,
        #[arg(long)]
        interactive: bool,
        #[arg(long)]
        tags: Option<String>,
    },
    Dapp {
        name: String,
    },
}

pub fn handle(cmd: NewCommands) -> Result<()> {
    match cmd {
        NewCommands::Contract {
            name,
            template,
            from,
            search,
            interactive,
            tags,
        } => {
            if let Some(query) = search {
                return handle_template_search(&query, tags.as_deref());
            }

            let name = name.ok_or_else(|| {
                anyhow::anyhow!("A contract name is required unless --search is used")
            })?;

            if matches!(from.as_deref(), Some("marketplace")) {
                scaffold_from_marketplace(name, template)
            } else if interactive {
                scaffold_contract_interactive(name)
            } else {
                scaffold_contract(
                    name,
                    template,
                    from.as_deref().unwrap_or("official"),
                    "MIT",
                    "",
                    "none",
                    true,
                )
            }
        }
        NewCommands::Dapp { name } => scaffold_dapp(name),
    }
}

fn search_templates(query: &str) -> Result<()> {
    let results = templates::search_templates(query, None)?;
    p::header(&format!("Template search results for '{}'", query));
    if results.is_empty() {
        p::info("No templates matched that query.");
        return Ok(());
    }

    for (i, entry) in results.iter().enumerate() {
        println!("  {:>2}. {}@{}", i + 1, entry.name, entry.version);
        p::kv("Description", &entry.description);
        p::kv("Source", &entry.source.to_string());
        if !entry.tags.is_empty() {
            p::kv("Tags", &entry.tags.join(", "));
        }
        if i + 1 < results.len() {
            println!();
        }
    }

    Ok(())
}

struct ContractOptions {
    name: String,
    author: String,
    license: String,
    storage: String,
    include_tests: bool,
}

fn scaffold_contract_interactive(default_name: String) -> Result<()> {
    let theme = ColorfulTheme::default();

    println!();
    println!("  {} Let's set up your contract.\n", "âœ¦".cyan());

    let name: String = Input::with_theme(&theme)
        .with_prompt("Contract name")
        .default(default_name)
        .interact_text()?;

    let author: String = Input::with_theme(&theme)
        .with_prompt("Author name")
        .default(String::from("Your Name"))
        .interact_text()?;

    let licenses = &["MIT", "Apache-2.0", "None"];
    let license_idx = Select::with_theme(&theme)
        .with_prompt("License")
        .items(licenses)
        .default(0)
        .interact()?;
    let license = licenses[license_idx].to_string();

    let storage_opts = &["persistent", "temporary", "none"];
    let storage_idx = Select::with_theme(&theme)
        .with_prompt("Storage type")
        .items(storage_opts)
        .default(0)
        .interact()?;
    let storage = storage_opts[storage_idx].to_string();

    let include_tests = Confirm::with_theme(&theme)
        .with_prompt("Include a test module?")
        .default(true)
        .interact()?;

    let opts = ContractOptions {
        name,
        author,
        license,
        storage,
        include_tests,
    };

    println!();
    println!("  {} Summary:", "â—†".bright_white());
    println!("    Contract name : {}", opts.name.cyan());
    println!("    Author        : {}", opts.author.cyan());
    println!("    License       : {}", opts.license.cyan());
    println!("    Storage       : {}", opts.storage.cyan());
    println!(
        "    Tests         : {}",
        if opts.include_tests {
            "yes".green()
        } else {
            "no".yellow()
        }
    );
    println!();

    let confirmed = Confirm::with_theme(&theme)
        .with_prompt("Write files?")
        .default(true)
        .interact()?;

    if !confirmed {
        println!("\n  {} Aborted - no files written.\n", "âœ—".red());
        return Ok(());
    }

    scaffold_contract(
        opts.name,
        "hello-world".to_string(),
        "official",
        &opts.license,
        &opts.author,
        &opts.storage,
        opts.include_tests,
    )
}

fn scaffold_contract(
    name: String,
    template: String,
    source: &str,
    license: &str,
    author: &str,
    storage: &str,
    include_tests: bool,
) -> Result<()> {
    let dir = Path::new(&name);
    if dir.exists() {
        anyhow::bail!("Directory '{}' already exists", name);
    }

    p::header(&format!("Scaffolding Soroban contract: {}", name));
    println!("  Template: {}\n", template.cyan());

    p::step(1, 4, "Creating directory structure...");
    fs::create_dir_all(dir.join("src"))?;
    fs::create_dir_all(dir.join(".cargo"))?;

    p::step(2, 4, "Writing Cargo.toml...");
    fs::write(dir.join("Cargo.toml"), cargo_toml(&name, license, author))?;
    fs::write(dir.join(".cargo/config.toml"), cargo_config())?;
    fs::write(dir.join(".gitignore"), "target/\n.soroban/\n")?;

    p::step(3, 4, &format!("Generating '{}' contract source...", template));
    let src = match template.as_str() {
        "token" => token_template(&name),
        "voting" => voting_template(&name),
        "nft" => nft_template(&name),
        _ => {
            if let Some(custom) = templates::template_source_content(&template)? {
                custom
            } else if template == "hello-world" {
                hello_world_template(&name, storage, include_tests)
            } else {
                anyhow::bail!(
                    "Unknown template '{}'. Search available templates with `starforge new contract --search <query>`.",
                    template
                );
            }
        }
    };
    fs::write(dir.join("src/lib.rs"), src)?;

    p::step(4, 4, "Writing README.md...");
    fs::write(dir.join("README.md"), readme(&name, &template, source))?;

    println!();
    p::success(&format!("Contract '{}' scaffolded!", name));
    println!();
    println!("  Next steps:");
    p::info(&format!("  cd {}", name));
    p::info("  stellar contract build");
    p::info(&format!(
        "  starforge deploy --wasm target/wasm32-unknown-unknown/release/{}.wasm",
        name.replace('-', "_")
    ));
    println!();
    Ok(())
}

fn scaffold_dapp(name: String) -> Result<()> {
    let dir = Path::new(&name);
    if dir.exists() {
        anyhow::bail!("Directory '{}' already exists", name);
    }

    p::header(&format!("Scaffolding Stellar dApp: {}", name));

    p::step(1, 3, "Creating project structure...");
    fs::create_dir_all(dir.join("src/components"))?;
    fs::create_dir_all(dir.join("public"))?;

    p::step(2, 3, "Writing package.json...");
    fs::write(dir.join("package.json"), dapp_package(&name))?;

    p::step(3, 3, "Writing app scaffold...");
    fs::write(dir.join("index.html"), dapp_index(&name))?;
    fs::write(dir.join("src/main.jsx"), dapp_main())?;
    fs::write(dir.join("src/App.jsx"), dapp_app(&name))?;
    fs::write(dir.join(".gitignore"), "node_modules/\ndist/\n")?;
    fs::write(dir.join("README.md"), dapp_readme(&name))?;

    println!();
    p::success(&format!("dApp '{}' scaffolded!", name));
    p::info(&format!("cd {} && npm install && npm run dev", name));
    println!();
    Ok(())
}

fn to_pascal(s: &str) -> String {
    s.split(['-', '_', ' '])
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

fn cargo_toml(name: &str, license: &str, author: &str) -> String {
    let license_field = if license == "None" || license.is_empty() {
        String::new()
    } else {
        format!("license = \"{license}\"\n")
    };
    let author_field = if author.is_empty() {
        String::new()
    } else {
        format!("authors = [\"{author}\"]\n")
    };
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
{author_field}{license_field}
[lib]
crate-type = ["cdylib"]

[dependencies]
soroban-sdk = "21.0.0"

[dev-dependencies]
soroban-sdk = {{ version = "21.0.0", features = ["testutils"] }}

[profile.release]
opt-level = "z"
overflow-checks = true
debug = 0
strip = "symbols"
debug-assertions = false
panic = "abort"
codegen-units = 1
lto = true
"#
    )
}

fn cargo_config() -> &'static str {
    r#"[target.wasm32-unknown-unknown]
rustflags = ["-C", "target-feature=+multivalue,+sign-ext"]
"#
}

fn hello_world_template(name: &str, storage: &str, include_tests: bool) -> String {
    let pascal = to_pascal(name);

    let storage_import = match storage {
        "persistent" | "temporary" => "\nuse soroban_sdk::storage::Storage;",
        _ => "",
    };

    let storage_method = match storage {
        "persistent" => {
            r#"
    pub fn set_value(env: Env, key: Symbol, value: u64) {
        env.storage().persistent().set(&key, &value);
    }

    pub fn get_value(env: Env, key: Symbol) -> Option<u64> {
        env.storage().persistent().get(&key)
    }"#
                .to_string()
        }
        "temporary" => {
            r#"
    pub fn set_value(env: Env, key: Symbol, value: u64) {
        env.storage().temporary().set(&key, &value);
    }

    pub fn get_value(env: Env, key: Symbol) -> Option<u64> {
        env.storage().temporary().get(&key)
    }"#
                .to_string()
        }
        _ => String::new(),
    };

    let test_module = if include_tests {
        format!(
            r#"

#[cfg(test)]
mod test {{
    use super::*;
    use soroban_sdk::{{Env, symbol_short}};

    #[test]
    fn test_hello() {{
        let env = Env::default();
        let id = env.register_contract(None, {pascal});
        let client = {pascal}Client::new(&env, &id);
        let words = client.hello(&symbol_short!("Dev"));
        assert_eq!(words, vec![&env, symbol_short!("Hello"), symbol_short!("Dev")]);
    }}
}}"#
        )
    } else {
        String::new()
    };

    format!(
        r#"#![no_std]
use soroban_sdk::{{contract, contractimpl, symbol_short, vec, Env, Symbol, Vec}};{storage_import}

#[contract]
pub struct {pascal};

#[contractimpl]
impl {pascal} {{
    pub fn hello(env: Env, to: Symbol) -> Vec<Symbol> {{
        vec![&env, symbol_short!("Hello"), to]
    }}{storage_method}
}}{test_module}
"#
    )
}

fn token_template(name: &str) -> String {
    let pascal = to_pascal(name);
    format!(
        r#"#![no_std]
use soroban_sdk::{{contract, contractimpl, contracttype, Address, Env, String}};

#[derive(Clone)]
#[contracttype]
pub struct TokenMetadata {{
    pub decimal: u32,
    pub name: String,
    pub symbol: String,
}}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {{
    Admin,
    Metadata,
    Balance(Address),
    TotalSupply,
}}

#[contract]
pub struct {pascal};

#[contractimpl]
impl {pascal} {{
    pub fn initialize(env: Env, admin: Address, decimal: u32, name: String, symbol: String) {{
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Metadata, &TokenMetadata {{ decimal, name, symbol }});
        env.storage().instance().set(&DataKey::TotalSupply, &0i128);
    }}

    pub fn mint(env: Env, to: Address, amount: i128) {{
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();
        let balance = Self::balance(env.clone(), to.clone());
        env.storage().persistent().set(&DataKey::Balance(to), &(balance + amount));
        let total: i128 = env.storage().instance().get(&DataKey::TotalSupply).unwrap();
        env.storage().instance().set(&DataKey::TotalSupply, &(total + amount));
    }}

    pub fn balance(env: Env, id: Address) -> i128 {{
        env.storage().persistent().get(&DataKey::Balance(id)).unwrap_or(0)
    }}
}}
"#
    )
}

fn voting_template(name: &str) -> String {
    let pascal = to_pascal(name);
    format!(
        r#"#![no_std]
use soroban_sdk::{{contract, contractimpl, contracttype, Address, Env, String}};

#[derive(Clone)]
#[contracttype]
pub struct Proposal {{
    pub id: u32,
    pub creator: Address,
    pub title: String,
    pub yes_votes: u32,
    pub no_votes: u32,
    pub active: bool,
}}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {{
    ProposalCount,
    Proposal(u32),
    Vote(u32, Address),
}}

#[contract]
pub struct {pascal};

#[contractimpl]
impl {pascal} {{
    pub fn create_proposal(env: Env, creator: Address, title: String) -> u32 {{
        creator.require_auth();
        let count: u32 = env.storage().instance().get(&DataKey::ProposalCount).unwrap_or(0);
        let proposal_id = count + 1;
        let proposal = Proposal {{
            id: proposal_id,
            creator,
            title,
            yes_votes: 0,
            no_votes: 0,
            active: true,
        }};
        env.storage().persistent().set(&DataKey::Proposal(proposal_id), &proposal);
        env.storage().instance().set(&DataKey::ProposalCount, &proposal_id);
        proposal_id
    }}
}}
"#
    )
}

fn nft_template(name: &str) -> String {
    let pascal = to_pascal(name);
    format!(
        r#"#![no_std]
use soroban_sdk::{{contract, contractimpl, contracttype, Address, Env, String}};

#[derive(Clone)]
#[contracttype]
pub struct NFTMetadata {{
    pub owner: Address,
    pub uri: String,
}}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {{
    Admin,
    Token(u64),
    TotalSupply,
}}

#[contract]
pub struct {pascal};

#[contractimpl]
impl {pascal} {{
    pub fn initialize(env: Env, admin: Address) {{
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TotalSupply, &0u64);
    }}
}}
"#
    )
}

fn dapp_package(name: &str) -> String {
    format!(
        r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "type": "module",
  "scripts": {{
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview"
  }},
  "dependencies": {{
    "@stellar/stellar-sdk": "^12.3.0",
    "react": "^18.3.0",
    "react-dom": "^18.3.0"
  }},
  "devDependencies": {{
    "@vitejs/plugin-react": "^4.3.1",
    "vite": "^5.4.0"
  }}
}}
"#
    )
}

fn dapp_index(name: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>{name}</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.jsx"></script>
  </body>
</html>
"#
    )
}

fn dapp_main() -> &'static str {
    r#"import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App.jsx'

ReactDOM.createRoot(document.getElementById('root')).render(
  <React.StrictMode><App /></React.StrictMode>
)
"#
}

fn dapp_app(name: &str) -> String {
    format!(
        r#"import React from 'react'

export default function App() {{
  return (
    <div style={{{{ fontFamily: 'monospace', padding: '2rem' }}}}>
      <h1>{name}</h1>
      <p>Your Stellar dApp is ready. Start building!</p>
    </div>
  )
}}
"#
    )
}

fn dapp_readme(name: &str) -> String {
    format!(
        r#"# {name}

A Stellar dApp scaffolded with starforge.
"#
    )
}

fn readme(name: &str, template: &str, source: &str) -> String {
    format!(
        r#"# {name}

A Soroban smart contract scaffolded with starforge.

Template: `{template}`
Source: `{source}`
"#
    )
}

fn handle_template_search(query: &str, tags: Option<&str>) -> Result<()> {
    p::header("Template Marketplace - Search");
    p::kv("Query", query);

    let tag_list = tags.map(|value| {
        value
            .split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
    });

    if let Some(ref tags) = tag_list {
        p::kv("Tags", &tags.join(", "));
    }

    println!();

    let results = templates::search_templates(query, tag_list.as_deref())?;

    if results.is_empty() {
        p::info("No templates found matching your search.");
        p::info("Try: starforge template publish ./my-template");
        return Ok(());
    }

    p::separator();
    println!("  Found {} template(s):\n", results.len());

    for (i, tmpl) in results.iter().enumerate() {
        let verified = if tmpl.verified {
            " âœ“".green().to_string()
        } else {
            String::new()
        };
        println!("  {}. {}{}", i + 1, tmpl.name.cyan().bold(), verified);
        println!("     {}", tmpl.description.dimmed());
        println!(
            "     {} â€¢ {} â€¢ {} downloads",
            tmpl.version.yellow(),
            tmpl.author.dimmed(),
            tmpl.downloads
        );

        if !tmpl.tags.is_empty() {
            println!("     Tags: {}", tmpl.tags.join(", ").bright_black());
        }

        if i < results.len() - 1 {
            println!();
        }
    }

    p::separator();
    println!();
    p::info("Use a template:");
    println!(
        "  {}",
        format!(
            "starforge new contract my-project --template {} --from marketplace",
            results[0].name
        )
        .cyan()
    );

    Ok(())
}

fn scaffold_from_marketplace(name: String, template_name: String) -> Result<()> {
    p::header(&format!("Scaffolding from Marketplace: {}", template_name));

    let template = templates::get_template(&template_name).with_context(|| {
        format!(
            "Template '{}' not found. Try: starforge new contract --search {}",
            template_name, template_name
        )
    })?;

    let dir = Path::new(&name);
    if dir.exists() {
        anyhow::bail!("Directory '{}' already exists", name);
    }

    p::separator();
    p::kv("Template", &template.name);
    p::kv("Version", &template.version);
    p::kv("Author", &template.author);
    p::kv("Description", &template.description);
    p::separator();

    println!();
    p::step(1, 3, "Fetching template...");

    let temp_dir = std::env::temp_dir().join(format!("starforge-template-{}", uuid::Uuid::new_v4()));
    templates::fetch_template(&template, &temp_dir)?;

    p::step(2, 3, "Validating template structure...");
    templates::validate_template_structure(&temp_dir)?;

    p::step(3, 3, "Copying template to project directory...");
    fs::create_dir_all(dir)?;
    copy_template_contents(&temp_dir, dir, &name)?;
    fs::remove_dir_all(&temp_dir).ok();

    let mut registry = templates::load_registry()?;
    if let Some(entry) = registry.templates.iter_mut().find(|item| item.name == template.name) {
        entry.downloads += 1;
        templates::save_registry(&registry)?;
    }

    println!();
    p::success(&format!("Contract '{}' scaffolded from marketplace!", name));
    Ok(())
}

fn copy_template_contents(src: &Path, dst: &Path, project_name: &str) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();

        if file_name == ".git" || file_name == "target" {
            continue;
        }

        let dest_path = dst.join(&file_name);

        if path.is_dir() {
            fs::create_dir_all(&dest_path)?;
            copy_template_contents(&path, &dest_path, project_name)?;
        } else {
            let mut content = fs::read_to_string(&path)?;
            content = content.replace("{{PROJECT_NAME}}", project_name);
            content = content.replace("{{PROJECT_NAME_SNAKE}}", &project_name.replace('-', "_"));
            content = content.replace("{{PROJECT_NAME_PASCAL}}", &to_pascal(project_name));
            fs::write(&dest_path, content)?;
        }
    }

    Ok(())
}
