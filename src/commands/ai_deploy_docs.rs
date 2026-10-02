//! `starforge ai-deploy-docs` — AI-powered deployment documentation generator.
//!
//! Automatically generates comprehensive deployment guides, runbooks,
//! troubleshooting docs, API references, and architecture documentation
//! for Soroban smart contracts — all locally without an external AI API.
//!
//! ## Sub-commands
//! - `guide`         — Generate a deployment guide for a contract
//! - `runbook`       — Generate an operational runbook
//! - `troubleshoot`  — Generate a troubleshooting guide
//! - `api`           — Generate API reference documentation
//! - `architecture`  — Generate architecture documentation
//! - `all`           — Generate all documentation types at once

use crate::utils::print as p;
use anyhow::Result;
use clap::{Args, Subcommand};
use colored::*;
use std::fs;
use std::path::PathBuf;

// ── Sub-command enum ──────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum AiDeployDocsCommands {
    /// Generate a step-by-step deployment guide for a Soroban contract
    Guide(GuideArgs),
    /// Generate an operational runbook with procedures and checklists
    Runbook(RunbookArgs),
    /// Generate a troubleshooting guide with common issues and fixes
    Troubleshoot(TroubleshootArgs),
    /// Generate API reference documentation from contract source
    Api(ApiArgs),
    /// Generate architecture documentation describing the system design
    Architecture(ArchitectureArgs),
    /// Generate all documentation types and write to an output directory
    All(AllArgs),
}

// ── Args structs ──────────────────────────────────────────────────────────────

#[derive(Args)]
pub struct GuideArgs {
    /// Contract name (e.g. my_token)
    pub contract: String,

    /// Path to the compiled .wasm file
    #[arg(long)]
    pub wasm: Option<PathBuf>,

    /// Path to the contract source file (.rs) for richer output
    #[arg(long)]
    pub source: Option<PathBuf>,

    /// Target network: testnet | mainnet
    #[arg(long, default_value = "testnet")]
    pub network: String,

    /// Output format: text | markdown
    #[arg(long, default_value = "text", value_parser = ["text", "markdown"])]
    pub format: String,

    /// Write output to this file instead of stdout
    #[arg(long, short)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct RunbookArgs {
    /// Contract name
    pub contract: String,

    /// Path to the contract source file (.rs)
    #[arg(long)]
    pub source: Option<PathBuf>,

    /// Target network: testnet | mainnet
    #[arg(long, default_value = "testnet")]
    pub network: String,

    /// Output format: text | markdown
    #[arg(long, default_value = "text", value_parser = ["text", "markdown"])]
    pub format: String,

    /// Write output to this file instead of stdout
    #[arg(long, short)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct TroubleshootArgs {
    /// Contract name
    pub contract: String,

    /// Path to the contract source file (.rs)
    #[arg(long)]
    pub source: Option<PathBuf>,

    /// Output format: text | markdown
    #[arg(long, default_value = "text", value_parser = ["text", "markdown"])]
    pub format: String,

    /// Write output to this file instead of stdout
    #[arg(long, short)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct ApiArgs {
    /// Contract name
    pub contract: String,

    /// Path to the contract source file (.rs) — required for API extraction
    #[arg(long)]
    pub source: Option<PathBuf>,

    /// Output format: text | markdown
    #[arg(long, default_value = "text", value_parser = ["text", "markdown"])]
    pub format: String,

    /// Write output to this file instead of stdout
    #[arg(long, short)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct ArchitectureArgs {
    /// Contract name
    pub contract: String,

    /// Path to the contract source file (.rs)
    #[arg(long)]
    pub source: Option<PathBuf>,

    /// Target network: testnet | mainnet
    #[arg(long, default_value = "testnet")]
    pub network: String,

    /// Output format: text | markdown
    #[arg(long, default_value = "text", value_parser = ["text", "markdown"])]
    pub format: String,

    /// Write output to this file instead of stdout
    #[arg(long, short)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct AllArgs {
    /// Contract name
    pub contract: String,

    /// Path to the contract source file (.rs)
    #[arg(long)]
    pub source: Option<PathBuf>,

    /// Path to the compiled .wasm file
    #[arg(long)]
    pub wasm: Option<PathBuf>,

    /// Target network: testnet | mainnet
    #[arg(long, default_value = "testnet")]
    pub network: String,

    /// Directory to write all generated documentation files
    #[arg(long, default_value = "docs")]
    pub out_dir: PathBuf,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Try to read contract source code; returns an empty string if no path given.
fn read_source(source: &Option<PathBuf>) -> String {
    match source {
        Some(path) if path.exists() => fs::read_to_string(path).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Extract public function signatures from Rust source using simple heuristics.
fn extract_pub_fns(source: &str) -> Vec<String> {
    source
        .lines()
        .filter(|l| {
            l.trim_start().starts_with("pub fn ") || l.trim_start().starts_with("pub async fn ")
        })
        .map(|l| l.trim().trim_end_matches('{').trim().to_string())
        .collect()
}

/// Write content to a file, creating parent directories as needed.
fn write_file(path: &PathBuf, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)
        .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", path.display(), e))
}

/// Emit output: write to file or print to stdout.
fn emit(content: &str, out: &Option<PathBuf>) -> Result<()> {
    match out {
        Some(path) => {
            write_file(path, content)?;
            p::success(&format!("Written to {}", path.display()));
        }
        None => println!("{}", content),
    }
    Ok(())
}

// ── Document generators ───────────────────────────────────────────────────────

fn generate_deployment_guide(
    contract: &str,
    network: &str,
    source: &str,
    markdown: bool,
) -> String {
    let pub_fns = extract_pub_fns(source);
    let fn_list = if pub_fns.is_empty() {
        "  (no public functions detected — provide --source for richer output)".to_string()
    } else {
        pub_fns
            .iter()
            .map(|f| format!("  - `{}`", f))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let h1 = if markdown { "#" } else { "" };
    let h2 = if markdown { "##" } else { "──" };
    let h3 = if markdown { "###" } else { "  >" };
    let wasm_target = crate::utils::wasm_target::resolve_target(None)
        .unwrap_or_else(|_| "<selected-target>".to_string());

    format!(
        r#"{h1} Deployment Guide — {contract}

Generated by starforge ai-deploy-docs · Network: {network}

{h2} Prerequisites

- Rust toolchain with the selected Soroban WASM target installed
- Stellar CLI (`stellar`) ≥ 21.0 on your PATH
- A funded {network} account (create with `starforge wallet create deployer --fund`)
- The compiled contract WASM: `target/{wasm_target}/release/{contract_snake}.wasm`

{h2} Build

```bash
stellar contract build
# Output: target/{wasm_target}/release/{contract_snake}.wasm
```

{h2} Pre-deployment Checks

1. Verify the WASM size is within Soroban limits (< 64 KB recommended):
   ```bash
    wc -c target/{wasm_target}/release/{contract_snake}.wasm
   ```
2. Check your deployer account balance:
   ```bash
   starforge wallet show deployer
   ```
3. Confirm the target network:
   ```bash
   starforge network show
   starforge network switch {network}
   ```

{h2} Deploy

```bash
starforge deploy \
    --wasm target/{wasm_target}/release/{contract_snake}.wasm \
  --network {network} \
  --wallet deployer
```

For CI/CD environments add `--yes` to skip the confirmation prompt.

{h2} Post-deployment Verification

After deployment you will receive a **Contract ID** (starts with `C`).

```bash
# Inspect the deployed contract
starforge contract inspect <CONTRACT_ID> --network {network}
```

{h2} Public Entry Points

{fn_list}

{h2} Rollback

If you need to revert to a previous version:
```bash
starforge deployments list
starforge deployments rollback --contract <CONTRACT_ID> --to <PREVIOUS_WASM_HASH>
```

{h3} Security reminder
Never commit secret keys to source control. Use `starforge wallet create --encrypt` for
production deployer accounts and store the passphrase in a secrets manager.
"#,
        h1 = h1,
        h2 = h2,
        h3 = h3,
        contract = contract,
        contract_snake = contract.replace('-', "_"),
        network = network,
        fn_list = fn_list,
        wasm_target = wasm_target,
    )
}

fn generate_runbook(contract: &str, network: &str, source: &str, markdown: bool) -> String {
    let pub_fns = extract_pub_fns(source);
    let fn_checks = if pub_fns.is_empty() {
        "  - Verify contract is responding (no public functions detected)".to_string()
    } else {
        pub_fns
            .iter()
            .map(|f| format!("  - [ ] Smoke-test `{}`", f))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let h1 = if markdown { "#" } else { "" };
    let h2 = if markdown { "##" } else { "──" };
    let wasm_target = crate::utils::wasm_target::resolve_target(None)
        .unwrap_or_else(|_| "<selected-target>".to_string());

    format!(
        r#"{h1} Operational Runbook — {contract}

Generated by starforge ai-deploy-docs · Network: {network}

{h2} Deployment Checklist

- [ ] Contract WASM built and size-checked
- [ ] Deployer wallet funded (minimum 2 XLM recommended)
- [ ] Network confirmed: {network}
- [ ] Previous contract version noted for rollback reference
- [ ] Deployment command reviewed and approved
{fn_checks}

{h2} Deployment Procedure

1. **Switch to the target network**
   ```bash
   starforge network switch {network}
   ```
2. **Deploy the contract**
   ```bash
   starforge deploy \
    --wasm target/{wasm_target}/release/{contract_snake}.wasm \
     --network {network} \
     --wallet deployer
   ```
3. **Record the Contract ID** returned in the output.
4. **Verify deployment**
   ```bash
   starforge contract inspect <CONTRACT_ID>
   ```
5. **Update your application config** with the new Contract ID.

{h2} Rollback Procedure

1. Identify the previous deployment hash:
   ```bash
   starforge deployments list --contract <CONTRACT_ID>
   ```
2. Execute rollback:
   ```bash
   starforge deployments rollback --contract <CONTRACT_ID> --to <PREV_HASH>
   ```
3. Confirm the contract is operational and smoke-test all entry points.

{h2} Monitoring

```bash
# Watch for contract events
starforge monitor --contract <CONTRACT_ID>

# Check deployment analytics
starforge analytics show --contract <CONTRACT_ID>
```

{h2} Escalation Path

| Severity | Action |
|----------|--------|
| Low      | Review logs; retry deployment |
| Medium   | Rollback to previous version; investigate |
| High     | Rollback immediately; page on-call; open incident |

{h2} Contacts & Resources

- Stellar documentation: https://developers.stellar.org
- Soroban docs: https://soroban.stellar.org
- starforge help: `starforge --help`
"#,
        h1 = h1,
        h2 = h2,
        contract = contract,
        contract_snake = contract.replace('-', "_"),
        network = network,
        fn_checks = fn_checks,
        wasm_target = wasm_target,
    )
}

fn generate_troubleshooting(contract: &str, source: &str, markdown: bool) -> String {
    let pub_fns = extract_pub_fns(source);
    let fn_section = if !pub_fns.is_empty() {
        let lines = pub_fns
            .iter()
            .map(|f| format!("  - `{}` — verify argument types and authorization", f))
            .collect::<Vec<_>>()
            .join("\n");
        format!("\n## Function-level Notes\n\n{}\n", lines)
    } else {
        String::new()
    };

    let h1 = if markdown { "#" } else { "" };
    let h2 = if markdown { "##" } else { "──" };

    format!(
        r#"{h1} Troubleshooting Guide — {contract}

Generated by starforge ai-deploy-docs

{h2} Deployment Failures

**Error: `account not found` / `account not active`**
- Cause: Deployer account not funded or does not exist on this network.
- Fix:
  ```bash
  starforge wallet fund deployer
  starforge wallet show deployer   # confirm balance > 0
  ```

**Error: `insufficient balance`**
- Cause: Not enough XLM to cover fees and base reserve.
- Fix: Fund the wallet via Friendbot (testnet) or transfer XLM (mainnet).

**Error: `No such file or directory` (WASM path)**
- Cause: Contract has not been compiled yet.
- Fix:
  ```bash
  stellar contract build
  ```

**Error: `wasm file too large`**
- Cause: WASM exceeds Soroban upload limits.
- Fix: Compile with optimisation:
  ```bash
  starforge deploy --wasm ./contract.wasm --optimize
  ```

{h2} Runtime / Invocation Errors

**`Error(Auth, InvalidAction)`**
- Cause: The caller is not authorised to invoke this function.
- Fix: Ensure the transaction is signed by the required account or contract.

**`Error(Value, InvalidInput)`**
- Cause: An argument has the wrong type or is out of the accepted range.
- Fix: Check the function signature and verify argument encoding.

**`Error(Storage, MissingValue)`**
- Cause: Reading a storage key that has not been initialised or has expired.
- Fix: Call the contract initialisation function before reading state.
  Check TTL extensions for long-lived data:
  ```bash
  starforge contract inspect <CONTRACT_ID>
  ```

**`Error(Contract, ArithmeticDomain)`**
- Cause: Integer overflow or division by zero.
- Fix: Review arithmetic operations; use checked arithmetic in contract code.

{h2} Network / RPC Issues

**`connection refused` / `timeout`**
- Fix:
  ```bash
  starforge network test
  starforge network switch testnet   # fall back to default endpoints
  ```

**`sequence number too old`**
- Cause: Stale transaction sequence number (common in CI retry loops).
- Fix: Refresh account sequence before re-submitting.

{h2} Useful Diagnostic Commands

```bash
# Inspect on-chain contract state
starforge contract inspect <CONTRACT_ID>

# View deployment history
starforge deployments list

# Run AI error analysis
starforge ai-debug analyse "<paste error here>"

# Security audit
starforge ai-audit <source_file>
```
{fn_section}"#,
        h1 = h1,
        h2 = h2,
        contract = contract,
        fn_section = fn_section,
    )
}

fn generate_api_reference(contract: &str, source: &str, markdown: bool) -> String {
    let pub_fns = extract_pub_fns(source);

    let h1 = if markdown { "#" } else { "" };
    let h2 = if markdown { "##" } else { "──" };
    let h3 = if markdown { "###" } else { "  >" };

    let fn_docs = if pub_fns.is_empty() {
        format!(
            "{}  No public functions detected. Provide --source <file.rs> for full API extraction.",
            h3
        )
    } else {
        pub_fns
            .iter()
            .enumerate()
            .map(|(i, sig)| {
                format!(
                    "{h3} `{name}`\n\n```rust\n{sig}\n```\n\n- **Index**: {i}\n- **Authorization**: verify caller requirements in source\n- **Returns**: see function signature\n",
                    h3 = h3,
                    name = sig
                        .split('(')
                        .next()
                        .unwrap_or(sig)
                        .trim_start_matches("pub fn ")
                        .trim_start_matches("pub async fn ")
                        .trim(),
                    sig = sig,
                    i = i,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"{h1} API Reference — {contract}

Generated by starforge ai-deploy-docs

{h2} Overview

This document describes the public interface of the `{contract}` Soroban smart contract.
All functions are invokable on-chain via the Stellar network using the Soroban RPC.

{h2} Invoking Functions

```bash
# Using Stellar CLI
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network <testnet|mainnet> \
  --source <WALLET_NAME> \
  -- <function_name> <args...>
```

{h2} Public Functions

{fn_docs}

{h2} Error Codes

| Code | Meaning |
|------|---------|
| `Error(Auth, InvalidAction)` | Caller not authorised |
| `Error(Value, InvalidInput)` | Bad argument type or range |
| `Error(Storage, MissingValue)` | State not initialised |
| `Error(Contract, ArithmeticDomain)` | Overflow or division by zero |

{h2} Generating Typed Bindings

```bash
# Rust bindings
starforge contract generate-bindings ./contract.wasm --lang rust

# TypeScript bindings
starforge contract generate-bindings ./contract.wasm --lang ts
```
"#,
        h1 = h1,
        h2 = h2,
        contract = contract,
        fn_docs = fn_docs,
    )
}

fn generate_architecture(contract: &str, network: &str, source: &str, markdown: bool) -> String {
    let pub_fns = extract_pub_fns(source);
    let entry_points = if pub_fns.is_empty() {
        "  (provide --source for entry-point list)".to_string()
    } else {
        pub_fns
            .iter()
            .map(|f| format!("  - `{}`", f))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let h1 = if markdown { "#" } else { "" };
    let h2 = if markdown { "##" } else { "──" };

    format!(
        r#"{h1} Architecture Documentation — {contract}

Generated by starforge ai-deploy-docs · Network: {network}

{h2} System Overview

`{contract}` is a Soroban smart contract deployed on the Stellar {network}.
Soroban contracts execute inside a WASM sandbox on Stellar validators and interact
with the ledger via a strictly capability-controlled host API.

{h2} Deployment Architecture

```
  Developer / CI
       │
       ▼
  starforge deploy ──► Stellar {network} RPC
                            │
                            ▼
                      Soroban Host (WASM Sandbox)
                            │
                   ┌────────┴────────┐
                   │                 │
             Contract Storage   Stellar Ledger
             (Instance / Temp)   (Accounts, XLM)
```

{h2} Contract Entry Points

{entry_points}

{h2} Storage Model

Soroban supports three storage tiers:

| Tier | Scope | TTL |
|------|-------|-----|
| Instance | Per contract instance | Tied to contract TTL |
| Persistent | Key-value, survives restores | Must be extended explicitly |
| Temporary | Key-value, auto-expires | Short-lived; not recoverable |

{h2} Authorization Model

- Functions may require `Address::require_auth()` from specific signers.
- Multi-party workflows use Stellar's built-in multi-sig or `starforge multisig`.
- Contract-to-contract calls propagate auth context automatically.

{h2} Upgrade Path

Soroban contracts can be upgraded in-place via `stellar contract upload` +
`stellar contract install`. Use `starforge upgrade` to manage upgrade proposals
and `starforge deployments rollback` to revert if needed.

{h2} Monitoring & Observability

```bash
# Live event stream
starforge monitor --contract <CONTRACT_ID>

# Deployment analytics
starforge analytics show --contract <CONTRACT_ID>

# Performance metrics
starforge perf show --contract <CONTRACT_ID>
```

{h2} Security Considerations

- Run `starforge ai-audit <source>` before each production deployment.
- Store deployer keys encrypted: `starforge wallet create deployer --encrypt`.
- Enable approval workflows for mainnet: `starforge approval create`.
- Review `starforge security harden` recommendations.
"#,
        h1 = h1,
        h2 = h2,
        contract = contract,
        network = network,
        entry_points = entry_points,
    )
}

// ── Public handle entry point ─────────────────────────────────────────────────

/// Dispatch `starforge ai-deploy-docs <subcommand>`.
pub async fn handle(cmd: AiDeployDocsCommands) -> Result<()> {
    match cmd {
        AiDeployDocsCommands::Guide(args) => handle_guide(args),
        AiDeployDocsCommands::Runbook(args) => handle_runbook(args),
        AiDeployDocsCommands::Troubleshoot(args) => handle_troubleshoot(args),
        AiDeployDocsCommands::Api(args) => handle_api(args),
        AiDeployDocsCommands::Architecture(args) => handle_architecture(args),
        AiDeployDocsCommands::All(args) => handle_all(args),
    }
}

fn handle_guide(args: GuideArgs) -> Result<()> {
    p::header("AI Deployment Documentation — Deployment Guide");
    p::kv("Contract", &args.contract);
    p::kv("Network", &args.network);
    println!();

    let source = read_source(&args.source);
    let markdown = args.format == "markdown";
    let content = generate_deployment_guide(&args.contract, &args.network, &source, markdown);
    emit(&content, &args.out)
}

fn handle_runbook(args: RunbookArgs) -> Result<()> {
    p::header("AI Deployment Documentation — Runbook");
    p::kv("Contract", &args.contract);
    p::kv("Network", &args.network);
    println!();

    let source = read_source(&args.source);
    let markdown = args.format == "markdown";
    let content = generate_runbook(&args.contract, &args.network, &source, markdown);
    emit(&content, &args.out)
}

fn handle_troubleshoot(args: TroubleshootArgs) -> Result<()> {
    p::header("AI Deployment Documentation — Troubleshooting Guide");
    p::kv("Contract", &args.contract);
    println!();

    let source = read_source(&args.source);
    let markdown = args.format == "markdown";
    let content = generate_troubleshooting(&args.contract, &source, markdown);
    emit(&content, &args.out)
}

fn handle_api(args: ApiArgs) -> Result<()> {
    p::header("AI Deployment Documentation — API Reference");
    p::kv("Contract", &args.contract);
    println!();

    let source = read_source(&args.source);
    let markdown = args.format == "markdown";
    let content = generate_api_reference(&args.contract, &source, markdown);
    emit(&content, &args.out)
}

fn handle_architecture(args: ArchitectureArgs) -> Result<()> {
    p::header("AI Deployment Documentation — Architecture");
    p::kv("Contract", &args.contract);
    p::kv("Network", &args.network);
    println!();

    let source = read_source(&args.source);
    let markdown = args.format == "markdown";
    let content = generate_architecture(&args.contract, &args.network, &source, markdown);
    emit(&content, &args.out)
}

fn handle_all(args: AllArgs) -> Result<()> {
    p::header("AI Deployment Documentation — Generating All Docs");
    p::kv("Contract", &args.contract);
    p::kv("Network", &args.network);
    p::kv("Output directory", &args.out_dir.display().to_string());
    println!();

    let source = read_source(&args.source);

    let files: Vec<(&str, String)> = vec![
        (
            "deployment-guide.md",
            generate_deployment_guide(&args.contract, &args.network, &source, true),
        ),
        (
            "runbook.md",
            generate_runbook(&args.contract, &args.network, &source, true),
        ),
        (
            "troubleshooting.md",
            generate_troubleshooting(&args.contract, &source, true),
        ),
        (
            "api-reference.md",
            generate_api_reference(&args.contract, &source, true),
        ),
        (
            "architecture.md",
            generate_architecture(&args.contract, &args.network, &source, true),
        ),
    ];

    fs::create_dir_all(&args.out_dir)
        .map_err(|e| anyhow::anyhow!("Failed to create output dir: {}", e))?;

    for (filename, content) in files {
        let path = args.out_dir.join(filename);
        write_file(&path, &content)?;
        println!("  {} {}", "✓".green(), path.display());
    }

    println!();
    p::success(&format!(
        "All documentation written to {}",
        args.out_dir.display()
    ));
    Ok(())
}
