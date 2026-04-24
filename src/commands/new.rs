use crate::utils::print as p;
use anyhow::Result;
use clap::Subcommand;
use colored::*;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use std::fs;
use std::path::Path;

#[derive(Subcommand)]
pub enum NewCommands {
    /// Scaffold a new Soroban smart contract project
    Contract {
        /// Project name
        name: String,
        /// Contract template
        #[arg(long, default_value = "hello-world",
              value_parser = ["hello-world", "token", "nft", "voting"])]
        template: String,
        /// Interactively customize the generated contract
        #[arg(long)]
        interactive: bool,
    },
    /// Scaffold a new Stellar dApp (Vite + React)
    Dapp {
        /// Project name
        name: String,
    },
}

pub fn handle(cmd: NewCommands) -> Result<()> {
    match cmd {
        NewCommands::Contract {
            name,
            template,
            interactive,
        } => {
            if interactive {
                scaffold_contract_interactive(name)
            } else {
                scaffold_contract(name, template, "MIT", "", "none", true)
            }
        }
        NewCommands::Dapp { name } => scaffold_dapp(name),
    }
}

// ── Interactive mode ──────────────────────────────────────────────────────────

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
    println!("  {} Let's set up your contract.\n", "✦".cyan());

    // 1. Contract name
    let name: String = Input::with_theme(&theme)
        .with_prompt("Contract name")
        .default(default_name)
        .interact_text()?;

    // 2. Author
    let author: String = Input::with_theme(&theme)
        .with_prompt("Author name")
        .default(String::from("Your Name"))
        .interact_text()?;

    // 3. License
    let licenses = &["MIT", "Apache-2.0", "None"];
    let license_idx = Select::with_theme(&theme)
        .with_prompt("License")
        .items(licenses)
        .default(0)
        .interact()?;
    let license = licenses[license_idx].to_string();

    // 4. Storage type
    let storage_opts = &["persistent", "temporary", "none"];
    let storage_idx = Select::with_theme(&theme)
        .with_prompt("Storage type")
        .items(storage_opts)
        .default(0)
        .interact()?;
    let storage = storage_opts[storage_idx].to_string();

    // 5. Test suite
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

    // Summary + confirm
    println!();
    println!("  {} Summary:", "◆".bright_white());
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
        println!("\n  {} Aborted — no files written.\n", "✗".red());
        return Ok(());
    }

    scaffold_contract(
        opts.name,
        "hello-world".to_string(), // template base; content is overridden by opts
        &opts.license,
        &opts.author,
        &opts.storage,
        opts.include_tests,
    )
}

fn scaffold_contract(
    name: String,
    template: String,
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

    p::step(1, 4, "Creating directory structure…");
    fs::create_dir_all(dir.join("src"))?;
    fs::create_dir_all(dir.join(".cargo"))?;

    p::step(2, 4, "Writing Cargo.toml…");
    fs::write(dir.join("Cargo.toml"), cargo_toml(&name, license, author))?;
    fs::write(dir.join(".cargo/config.toml"), cargo_config())?;
    fs::write(dir.join(".gitignore"), "target/\n.soroban/\n")?;

    p::step(3, 4, &format!("Generating '{}' contract source…", template));
    let src = match template.as_str() {
        "token" => token_template(&name),
        "voting" => voting_template(&name),
        "nft" => nft_template(&name),
        _ => hello_world_template(&name, storage, include_tests),
    };
    fs::write(dir.join("src/lib.rs"), src)?;

    p::step(4, 4, "Writing README.md…");
    fs::write(dir.join("README.md"), readme(&name, &template))?;

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

    p::step(1, 3, "Creating project structure…");
    fs::create_dir_all(dir.join("src/components"))?;
    fs::create_dir_all(dir.join("public"))?;

    p::step(2, 3, "Writing package.json…");
    fs::write(dir.join("package.json"), dapp_package(&name))?;

    p::step(3, 3, "Writing app scaffold…");
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

// ── Helpers ──────────────────────────────────────────────────────────────────

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

// ── Cargo files ──────────────────────────────────────────────────────────────

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

// ── Contract templates ────────────────────────────────────────────────────────

fn hello_world_template(name: &str, storage: &str, include_tests: bool) -> String {
    let pascal = to_pascal(name);

    let storage_import = match storage {
        "persistent" | "temporary" => "\nuse soroban_sdk::storage::Storage;",
        _ => "",
    };

    let storage_method = match storage {
        "persistent" => format!(
            r#"
    pub fn set_value(env: Env, key: Symbol, value: u64) {{
        env.storage().persistent().set(&key, &value);
    }}

    pub fn get_value(env: Env, key: Symbol) -> Option<u64> {{
        env.storage().persistent().get(&key)
    }}"#
        ),
        "temporary" => format!(
            r#"
    pub fn set_value(env: Env, key: Symbol, value: u64) {{
        env.storage().temporary().set(&key, &value);
    }}

    pub fn get_value(env: Env, key: Symbol) -> Option<u64> {{
        env.storage().temporary().get(&key)
    }}"#
        ),
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
        let id  = env.register_contract(None, {pascal});
        let client = {pascal}Client::new(&env, &id);
        let words = client.hello(&symbol_short!("Dev"));
        assert_eq!(words, vec![&env, symbol_short!("Hello"), symbol_short!("Dev")]);
    }}
}}"#,
            pascal = pascal
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
"#,
        pascal = pascal,
        storage_import = storage_import,
        storage_method = storage_method,
        test_module = test_module,
    )
}

fn token_template(name: &str) -> String {
    let pascal = to_pascal(name);
    format!(
        r#"#![no_std]
use soroban_sdk::{{contract, contractimpl, Address, Env, String}};

#[contract]
pub struct {pascal};

#[contractimpl]
impl {pascal} {{
    pub fn initialize(env: Env, admin: Address, decimal: u32, name: String, symbol: String) {{
        admin.require_auth();
        // TODO: store token metadata in env.storage()
        let _ = (decimal, name, symbol);
    }}

    pub fn mint(env: Env, to: Address, amount: i128) {{
        // TODO: implement minting logic
        let _ = (to, amount);
    }}

    pub fn balance(_env: Env, _id: Address) -> i128 {{
        // TODO: return balance from storage
        0
    }}

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {{
        from.require_auth();
        // TODO: implement transfer
        let _ = (to, amount);
    }}
}}
"#,
        pascal = pascal
    )
}

fn voting_template(name: &str) -> String {
    let pascal = to_pascal(name);
    format!(
        r#"#![no_std]
use soroban_sdk::{{contract, contractimpl, Address, Env, Symbol}};

#[contract]
pub struct {pascal};

#[contractimpl]
impl {pascal} {{
    pub fn create_proposal(env: Env, creator: Address, title: Symbol) -> u32 {{
        creator.require_auth();
        // TODO: store proposal, return ID
        let _ = title;
        0
    }}

    pub fn vote(env: Env, voter: Address, proposal_id: u32, approve: bool) {{
        voter.require_auth();
        // TODO: record vote in storage
        let _ = (proposal_id, approve);
    }}

    pub fn results(_env: Env, proposal_id: u32) -> (u32, u32) {{
        // TODO: return (yes_votes, no_votes)
        let _ = proposal_id;
        (0, 0)
    }}
}}
"#,
        pascal = pascal
    )
}

fn nft_template(name: &str) -> String {
    let pascal = to_pascal(name);
    format!(
        r#"#![no_std]
use soroban_sdk::{{contract, contractimpl, Address, Env, String}};

#[contract]
pub struct {pascal};

#[contractimpl]
impl {pascal} {{
    pub fn mint(env: Env, to: Address, token_id: u64, uri: String) {{
        // TODO: mint NFT and store metadata
        let _ = (to, token_id, uri);
    }}

    pub fn owner_of(_env: Env, token_id: u64) -> Address {{
        // TODO: return owner from storage
        let _ = token_id;
        panic!("not implemented")
    }}

    pub fn transfer(env: Env, from: Address, to: Address, token_id: u64) {{
        from.require_auth();
        // TODO: transfer ownership
        let _ = (to, token_id);
    }}
}}
"#,
        pascal = pascal
    )
}

// ── dApp scaffold files ───────────────────────────────────────────────────────

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
      <h1>⚡ {name}</h1>
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

A Stellar dApp scaffolded with [starforge](https://github.com/YOUR_USERNAME/starforge).

## Getting Started

```bash
npm install
npm run dev
```
"#
    )
}

fn readme(name: &str, template: &str) -> String {
    format!(
        r#"# {name}

A Soroban smart contract scaffolded with [starforge](https://github.com/YOUR_USERNAME/starforge).

## Build

```bash
stellar contract build
```

## Test

```bash
cargo test
```

## Deploy

```bash
starforge deploy \
  --wasm target/wasm32-unknown-unknown/release/{snake}.wasm \
  --network testnet
```

Template: `{template}`
"#,
        name = name,
        snake = name.replace('-', "_"),
        template = template
    )
}
