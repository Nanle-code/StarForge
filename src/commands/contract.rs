use crate::commands::invoke_script;
use crate::utils::config::WalletEntry;
use crate::utils::hardware_wallet::HardwareWalletKind;
use crate::utils::{bindings, call_graph, config, print as p, soroban, wallet_signer};
use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use colored::*;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Subcommand)]
pub enum ContractCommands {
    /// Invoke a deployed Soroban contract function
    Invoke(InvokeArgs),
    /// Run an ordered YAML or JSON invocation script
    InvokeScript(invoke_script::InvokeScriptArgs),
    /// Inspect a deployed Soroban contract instance or local WASM metadata
    Inspect(InspectArgs),
    /// Build a Soroban contract with StarForge build provenance metadata
    Build(BuildArgs),
    /// Upload a WASM binary to the Stellar network (upload-only step)
    ///
    /// See: https://developers.stellar.org/docs/build/smart-contracts/getting-started/deploy-increment-contract
    Upload(UploadArgs),
    /// Generate typed client bindings from embedded WASM metadata
    GenerateBindings(GenerateBindingsArgs),
    /// Visualize cross-contract call graph from Soroban source
    CallGraph(CallGraphArgs),
    /// Manage contract dependencies
    Deps(DepsArgs),
    /// Track contract versions, resolve conflicts, and manage migrations
    Version(VersionArgs),

    // ── Commands moved under `contract` by ADR 0007 ──────────────────────
    // Each moved command keeps its own argument struct, so no flag definition
    // is duplicated here; `handle` forwards to the owning module.
    /// Deep contract storage inspection (state, key, storage)
    #[command(subcommand)]
    Storage(crate::commands::inspect::InspectCommands),
    /// Debug Soroban contracts with breakpoints, stepping, and inspection
    #[command(subcommand)]
    Debug(crate::commands::debug::DebugCommands),
    /// Interactive REPL for local Soroban contract testing
    Repl {
        #[command(flatten)]
        args: crate::commands::shell::ShellArgs,
    },
    /// Contract testing utilities for Soroban wasm
    Test {
        #[command(flatten)]
        args: crate::commands::test::TestArgs,
    },
    /// Run a comprehensive security audit on a Soroban contract
    Audit {
        #[command(flatten)]
        args: crate::commands::audit::AuditArgs,
    },
    /// Security hardening, validation, and monitoring
    #[command(subcommand)]
    Security(crate::commands::security::SecurityCommands),
    /// Contract upgrade governance (proposals, voting, timelock, audit)
    #[command(subcommand)]
    Governance(crate::commands::governance::GovernanceCommands),
    /// Contract upgrade management (propose, approve, execute, rollback)
    #[command(subcommand)]
    Upgrade(crate::commands::upgrade::UpgradeCommands),
    /// Run formal verification on a contract
    #[command(subcommand)]
    Verify(crate::commands::verify::VerifyCommands),
    /// Contract storage migration tools (transform, validate, rollback)
    #[command(subcommand)]
    Migrate(crate::commands::migrate::MigrateCommands),
    /// Generate smart contracts from natural language prompts
    #[command(subcommand)]
    Generate(crate::commands::generate::GenerateCommands),
    /// Smart contract completion assistant
    #[command(subcommand)]
    Complete(crate::commands::complete::CompleteCommands),
    /// Analyze and explain smart contract code using AI
    #[command(subcommand)]
    Explain(crate::commands::explain::ExplainCommands),
    /// Static analysis and linting for Soroban contracts
    Lint {
        #[command(flatten)]
        args: crate::commands::lint::LintArgs,
    },
    /// Analyse and optimize compiled WASM / Rust contract source for gas and size
    #[command(subcommand)]
    Optimize(crate::commands::optimize::OptimizeCommands),
    /// Gas analysis and optimization helpers
    #[command(subcommand)]
    Gas(crate::commands::gas::GasCommands),
    /// Contract performance monitoring and metrics dashboard
    #[command(subcommand)]
    Metrics(crate::commands::perf::PerfCommands),
    /// Advanced contract performance analysis and profiling tools
    #[command(subcommand)]
    Profile(crate::commands::perf::AdvancedPerfCommands),
    /// Performance benchmarking utilities and industry-standard comparisons
    #[command(subcommand)]
    Benchmark(crate::commands::benchmark::BenchmarkCommands),
    /// Contract documentation portal (generate, view, search)
    #[command(subcommand)]
    Docs(crate::commands::docs::DocsCommands),
    /// AI mutation testing for Soroban contracts
    #[command(subcommand)]
    Mutate(crate::commands::mutate::MutateCommands),
    /// Live monitoring (contract events or wallet threshold)
    Monitor {
        #[command(flatten)]
        args: crate::commands::monitor::MonitorArgs,
    },
    /// Contract health monitoring and alerting
    #[command(subcommand)]
    Health(crate::commands::contract_monitor::ContractMonitorCommands),
    /// Inspect and extend ledger-entry TTLs (instance, code, persistent)
    #[command(subcommand)]
    Ttl(TtlCommands),
}

#[derive(Subcommand)]
pub enum TtlCommands {
    /// List instance/code/persistent entries with live-until ledger and ETA
    Show(TtlShowArgs),
    /// Build (and optionally submit) an ExtendFootprintTTL operation
    Extend(TtlExtendArgs),
}

#[derive(Args)]
pub struct TtlShowArgs {
    /// Contract ID whose ledger entries to inspect
    pub contract_id: String,
    /// Ledger-key selectors (`instance`, `code`, `symbol:NAME`, or base64 LedgerKey XDR).
    /// Repeatable. Defaults to instance + code.
    #[arg(long = "key")]
    pub keys: Vec<String>,
    /// Network to query
    #[arg(long, default_value = "testnet")]
    pub network: String,
    /// Exit non-zero when any entry has fewer than N ledgers remaining (cron/monitor)
    #[arg(long = "warn-below")]
    pub warn_below: Option<u32>,
    /// Emit JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct TtlExtendArgs {
    /// Contract ID whose ledger entries to extend
    pub contract_id: String,
    /// Target TTL floor in ledgers from now (`extendTo`)
    #[arg(long)]
    pub ledgers: u32,
    /// Ledger-key selectors (`instance`, `code`, `symbol:NAME`, or base64 LedgerKey XDR)
    #[arg(long = "key")]
    pub keys: Vec<String>,
    /// Network to use
    #[arg(long, default_value = "testnet")]
    pub network: String,
    /// Wallet used to sign the extend transaction
    #[arg(long)]
    pub wallet: Option<String>,
    /// Submit after showing the cost estimate (default: simulate only)
    #[arg(long)]
    pub submit: bool,
    /// Skip confirmation of the extend cost
    #[arg(long)]
    pub yes: bool,
    /// Emit JSON
    #[arg(long)]
    pub json: bool,
    /// Sign with a hardware wallet instead of a local secret key
    #[arg(long, value_enum)]
    pub hardware: Option<HardwareWalletKind>,
    /// HD derivation path for hardware wallet signing
    #[arg(long, default_value = crate::utils::hardware_wallet::STELLAR_HD_PATH)]
    pub hd_path: String,
}

#[derive(Args)]
pub struct DepsArgs {
    #[command(subcommand)]
    pub cmd: DepsCommands,
}

#[derive(Subcommand)]
pub enum DepsCommands {
    /// Initialize contract-dependencies.toml
    Init,
    /// Add a contract dependency
    Add(DepsAddArgs),
    /// Update a contract dependency
    Update(DepsUpdateArgs),
    /// Resolve and show deployment order
    Resolve,
    /// Visualize the dependency graph
    Graph(DepsGraphArgs),
}

#[derive(Args)]
pub struct DepsAddArgs {
    /// Name of the dependency
    pub name: String,
    /// Version constraint
    #[arg(long)]
    pub version: Option<String>,
    /// Local path
    #[arg(long)]
    pub path: Option<String>,
    /// Git repository URL
    #[arg(long)]
    pub git: Option<String>,
    /// Git branch
    #[arg(long)]
    pub branch: Option<String>,
}

#[derive(Args)]
pub struct DepsUpdateArgs {
    /// Name of the dependency to update
    pub name: String,
    /// New version constraint
    pub version: String,
}

#[derive(Args)]
pub struct DepsGraphArgs {
    /// Format: ascii or dot
    #[arg(long, default_value = "ascii")]
    pub format: String,
}

#[derive(Args)]
pub struct VersionArgs {
    #[command(subcommand)]
    pub cmd: VersionCommands,
}

#[derive(Subcommand)]
pub enum VersionCommands {
    /// Initialize contract-versions.toml
    Init(VersionInitArgs),
    /// Record an explicit semantic version
    Tag(VersionTagArgs),
    /// Bump the current version (major, minor, patch, or prerelease)
    Bump(VersionBumpArgs),
    /// List tracked versions
    List,
    /// Show details for a specific version
    Show(VersionShowArgs),
    /// Mark a version as yanked (deprecated, should not be depended on)
    Yank(VersionShowArgs),
    /// Detect version conflicts across the dependency graph
    Conflicts,
    /// Show a compatibility matrix for a dependency's tracked versions
    Matrix(VersionMatrixArgs),
    /// Resolve the migration chain needed to go from one version to another
    MigrationPath(MigrationPathArgs),
}

#[derive(Args)]
pub struct VersionInitArgs {
    /// Name of the contract being versioned
    #[arg(long)]
    pub name: String,
}

#[derive(Args)]
pub struct VersionTagArgs {
    /// Semantic version to record (e.g. 1.2.0)
    pub version: String,
    /// Optional release notes
    #[arg(long)]
    pub notes: Option<String>,
    /// Optional wasm hash to associate with this version
    #[arg(long)]
    pub wasm_hash: Option<String>,
    /// Allow tagging a version that is not greater than the current highest
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

#[derive(Args)]
pub struct VersionBumpArgs {
    /// Which part to bump
    #[arg(value_parser = ["major", "minor", "patch", "prerelease"])]
    pub part: String,
    /// Optional release notes
    #[arg(long)]
    pub notes: Option<String>,
}

#[derive(Args)]
pub struct VersionShowArgs {
    /// Version to show or yank
    pub version: String,
}

#[derive(Args)]
pub struct VersionMatrixArgs {
    /// Name of the dependency to build a compatibility matrix for
    pub dependency: String,
}

#[derive(Args)]
pub struct MigrationPathArgs {
    /// Directory containing migration rule files (from `starforge migrate init`)
    #[arg(long, default_value = "migrations")]
    pub dir: PathBuf,
    /// Version to migrate from
    #[arg(long)]
    pub from: String,
    /// Version to migrate to
    #[arg(long)]
    pub to: String,
}

#[derive(Args)]
pub struct CallGraphArgs {
    /// Path to Soroban contract source file (.rs)
    pub path: PathBuf,
    /// Output format: ascii (default), dot, json
    #[arg(long, default_value = "ascii")]
    pub format: String,
    /// Save output to file instead of stdout
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Show pattern analysis warnings
    #[arg(long, default_value = "true")]
    pub patterns: bool,
    /// Show concrete structural / gas optimization suggestions
    #[arg(long, default_value = "false")]
    pub optimize: bool,
    /// Launch the interactive call explorere (stdin menu) after extraction
    #[arg(long, default_value = "false", conflicts_with = "out")]
    pub explore: bool,
    /// Filter displayed patterns by minimum severity (low|medium|high)
    #[arg(long, value_parser = ["low", "medium", "high"])]
    pub severity: Option<String>,
    /// Show a one-shot statistics summary at the end
    #[arg(long, default_value = "false")]
    pub stats: bool,
}

#[derive(Args)]
#[command(disable_help_flag = true)]
pub struct InvokeArgs {
    /// Contract ID to invoke
    #[arg(allow_hyphen_values = true)]
    pub contract_id: String,
    /// Function name to call
    #[arg(allow_hyphen_values = true)]
    pub function: Option<String>,
    /// Function arguments (use multiple --arg flags)
    #[arg(long = "arg", action = clap::ArgAction::Append)]
    pub args: Vec<String>,
    /// Argument types (use multiple --type flags, must match --arg count)
    #[arg(long = "type", action = clap::ArgAction::Append)]
    pub types: Vec<String>,
    /// Network to use (testnet, mainnet, docker-testnet, or a configured custom network)
    #[arg(long, default_value = "testnet")]
    pub network: String,
    /// Wallet name to use for signing (required with --submit or restore)
    #[arg(long)]
    pub wallet: Option<String>,
    /// Submit the transaction after simulation
    #[arg(long, default_value = "false")]
    pub submit: bool,
    /// Automatically restore archived ledger entries when simulation returns a restorePreamble
    #[arg(long, default_value = "false")]
    pub auto_restore: bool,
    /// Skip interactive confirmation prompts (scripted mode; also skips restore prompts)
    #[arg(long, default_value = "false")]
    pub yes: bool,
    /// Emit machine-readable JSON (includes `restored` when a restore ran)
    #[arg(long, default_value = "false")]
    pub json: bool,
    /// Sign with a hardware wallet instead of a local secret key
    #[arg(long, value_enum)]
    pub hardware: Option<HardwareWalletKind>,
    /// HD derivation path for hardware wallet signing
    #[arg(long, default_value = crate::utils::hardware_wallet::STELLAR_HD_PATH)]
    pub hd_path: String,
    /// Dynamic typed arguments
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub slop: Vec<String>,
}

#[derive(Args)]
pub struct BuildArgs {
    /// Path to Cargo.toml
    #[arg(long)]
    pub manifest_path: Option<String>,

    /// Do not embed StarForge/source provenance metadata
    #[arg(long)]
    pub no_provenance: bool,
}

#[derive(Args)]
pub struct InspectArgs {
    /// Contract ID to inspect; omit when using --wasm
    #[arg(required_unless_present = "wasm")]
    pub contract_id: Option<String>,

    /// Local WASM file to inspect
    #[arg(long, conflicts_with = "contract_id")]
    pub wasm: Option<PathBuf>,
    /// Network to use; defaults to the global config network
    #[arg(long, value_parser = ["testnet", "mainnet"])]
    pub network: Option<String>,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct UploadArgs {
    /// Path to the compiled WASM file
    #[arg(long)]
    pub wasm: String,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Wallet name to use for signing
    #[arg(long)]
    pub wallet: Option<String>,
    /// Sign with a hardware wallet instead of a local secret key
    #[arg(long, value_enum)]
    pub hardware: Option<HardwareWalletKind>,
    /// HD derivation path for hardware wallet signing
    #[arg(long, default_value = crate::utils::hardware_wallet::STELLAR_HD_PATH)]
    pub hd_path: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BindingLang {
    Rust,
    Ts,
    Python,
    Go,
}

/// JavaScript module layout for a generated TypeScript package.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TsModuleArg {
    Esm,
    Cjs,
    Dual,
}

#[derive(Args, Debug, Clone)]
pub struct GenerateBindingsArgs {
    /// Path to the compiled WASM file, or a contract spec as raw or base64 XDR
    pub wasm_file: PathBuf,
    /// Binding target language
    #[arg(long, value_enum)]
    pub lang: BindingLang,
    /// Write a complete npm package into this directory instead of printing a
    /// single file (TypeScript only). Only changed files are rewritten.
    #[arg(long, value_name = "DIR")]
    pub out_dir: Option<PathBuf>,
    /// Module layout of the generated package (with --out-dir)
    #[arg(long = "module", value_enum, default_value = "dual")]
    pub module_format: TsModuleArg,
    /// npm package name (with --out-dir; default: <wasm-stem>-client)
    #[arg(long)]
    pub package_name: Option<String>,
    /// Do not write anything; fail if the package in --out-dir is out of date
    #[arg(long, requires = "out_dir")]
    pub check: bool,
}

pub async fn handle(cmd: ContractCommands) -> Result<()> {
    match cmd {
        ContractCommands::Invoke(args) => handle_invoke(args).await,
        ContractCommands::InvokeScript(args) => invoke_script::handle(args).await,
        ContractCommands::Inspect(args) => handle_inspect(args).await,
        ContractCommands::Build(args) => handle_build(args),
        ContractCommands::Upload(args) => handle_upload(args),
        ContractCommands::GenerateBindings(args) => handle_generate_bindings(&args),
        ContractCommands::CallGraph(args) => handle_call_graph(args),
        ContractCommands::Deps(args) => handle_deps(args),
        ContractCommands::Version(args) => handle_version(args).await,

        // ADR 0007: forward the commands that moved under `contract`.
        ContractCommands::Storage(cmd) => crate::commands::inspect::handle(cmd).await,
        ContractCommands::Debug(cmd) => crate::commands::debug::handle(cmd).await,
        ContractCommands::Repl { args } => crate::commands::shell::handle(args).await,
        ContractCommands::Test { args } => crate::commands::test::handle(args).await,
        ContractCommands::Audit { args } => crate::commands::audit::handle(args).await,
        ContractCommands::Security(cmd) => crate::commands::security::handle(cmd).await,
        ContractCommands::Governance(cmd) => crate::commands::governance::handle(cmd).await,
        ContractCommands::Upgrade(cmd) => crate::commands::upgrade::handle(cmd).await,
        ContractCommands::Verify(cmd) => crate::commands::verify::handle(cmd).await,
        ContractCommands::Migrate(cmd) => crate::commands::migrate::handle(cmd),
        ContractCommands::Generate(cmd) => crate::commands::generate::handle(&cmd).await,
        ContractCommands::Complete(cmd) => crate::commands::complete::handle(cmd).await,
        ContractCommands::Explain(cmd) => crate::commands::explain::handle(&cmd).await,
        ContractCommands::Lint { args } => crate::commands::lint::handle(args).await,
        ContractCommands::Optimize(cmd) => crate::commands::optimize::handle(cmd).await,
        ContractCommands::Gas(cmd) => crate::commands::gas::handle(cmd).await,
        ContractCommands::Metrics(cmd) => crate::commands::perf::handle(cmd).await,
        ContractCommands::Profile(cmd) => crate::commands::perf::handle_advanced(cmd).await,
        ContractCommands::Benchmark(cmd) => crate::commands::benchmark::handle(cmd).await,
        ContractCommands::Docs(cmd) => crate::commands::docs::handle(cmd).await,
        ContractCommands::Mutate(cmd) => crate::commands::mutate::handle(cmd).await,
        ContractCommands::Monitor { args } => crate::commands::monitor::handle(args).await,
        ContractCommands::Health(cmd) => crate::commands::contract_monitor::handle(cmd).await,
        ContractCommands::Ttl(cmd) => handle_ttl(cmd).await,
    }
}

pub fn handle_generate_bindings(args: &GenerateBindingsArgs) -> Result<()> {
    config::validate_file_path(&args.wasm_file, None)?;

    let lang = match args.lang {
        BindingLang::Rust => bindings::BindingLanguage::Rust,
        BindingLang::Ts => bindings::BindingLanguage::TypeScript,
        BindingLang::Python => bindings::BindingLanguage::Python,
        BindingLang::Go => bindings::BindingLanguage::Go,
    };

    let Some(out_dir) = &args.out_dir else {
        let generated = bindings::generate_bindings(&args.wasm_file, lang)?;
        println!("{}", generated);
        return Ok(());
    };

    if lang != bindings::BindingLanguage::TypeScript {
        anyhow::bail!("--out-dir is currently supported only with --lang ts");
    }
    let module = match args.module_format {
        TsModuleArg::Esm => bindings::TsModuleFormat::Esm,
        TsModuleArg::Cjs => bindings::TsModuleFormat::Cjs,
        TsModuleArg::Dual => bindings::TsModuleFormat::Dual,
    };
    let package_name = match &args.package_name {
        Some(name) => name.clone(),
        None => bindings::typescript::default_package_name(
            &args
                .wasm_file
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
    };

    let metadata = bindings::load_metadata(&args.wasm_file)?;
    let files = bindings::generate_typescript_package(
        &metadata,
        &bindings::TsPackageOptions::new(package_name.clone(), module),
    )?;
    let report = bindings::write_generated_files(out_dir, &files, args.check)?;

    let verb = if args.check { "would be " } else { "" };
    for path in &report.created {
        p::info(&format!("{verb}created   {path}"));
    }
    for path in &report.updated {
        p::info(&format!("{verb}updated   {path}"));
    }
    for path in &report.removed {
        p::info(&format!("{verb}removed   {path}"));
    }

    if args.check {
        if !report.is_clean() {
            anyhow::bail!(
                "TypeScript bindings in {} are out of date; rerun without --check",
                out_dir.display()
            );
        }
        p::success(&format!(
            "TypeScript bindings in {} are up to date",
            out_dir.display()
        ));
    } else {
        p::success(&format!(
            "Generated {} ({} layout) in {}: {} created, {} updated, {} unchanged, {} removed",
            package_name,
            module.as_str(),
            out_dir.display(),
            report.created.len(),
            report.updated.len(),
            report.unchanged.len(),
            report.removed.len()
        ));
    }
    Ok(())
}

async fn handle_inspect(args: InspectArgs) -> Result<()> {
    if let Some(wasm) = args.wasm {
        return handle_inspect_wasm(&wasm, args.json);
    }

    let contract_id = args
        .contract_id
        .ok_or_else(|| anyhow::anyhow!("A contract ID is required unless --wasm is supplied"))?;

    config::validate_contract_id(&contract_id)?;

    if let Some(ref net) = args.network {
        config::validate_network(net)?;
    }

    let network = resolve_network(args.network)?;

    p::header("Inspect Soroban Contract");
    p::separator();
    p::kv("Contract ID", &contract_id);
    p::kv("Network", &network);
    p::separator();

    println!();
    p::step(1, 2, "Querying contract instance from Soroban RPC…");
    let inspect = soroban::inspect_contract(&contract_id, &network).await?;

    let metadata = get_contract_metadata(None, Some(&contract_id), Some(&network));

    if args.json {
        let mut output = serde_json::to_value(&inspect)?;

        if let Some(obj) = output.as_object_mut() {
            if let Some(metadata) = metadata {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&metadata) {
                    obj.insert("metadata".to_string(), value);
                } else {
                    obj.insert("metadata".to_string(), serde_json::Value::String(metadata));
                }
            } else {
                obj.insert("metadata".to_string(), serde_json::Value::Null);
            }
        }

        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!();
    p::kv_accent("Contract ID", &inspect.contract_id);
    p::kv("Executable", &inspect.executable);
    p::kv(
        "WASM Hash",
        inspect
            .wasm_hash
            .as_deref()
            .unwrap_or("n/a (stellar asset contract)"),
    );
    p::kv("Storage Durability", &inspect.storage_durability);
    p::kv("Ledger Sequence", &inspect.latest_ledger.to_string());

    if let Some(last_modified) = inspect.last_modified_ledger_seq {
        p::kv("Last Modified", &last_modified.to_string());
    }

    if let Some(live_until) = inspect.live_until_ledger_seq {
        p::kv("Live Until", &live_until.to_string());
    }

    p::kv(
        "Instance Storage",
        &format!(
            "{} entr{}",
            inspect.instance_storage.len(),
            if inspect.instance_storage.len() == 1 {
                "y"
            } else {
                "ies"
            }
        ),
    );
    p::separator();

    if inspect.instance_storage.is_empty() {
        p::info("No instance storage entries found.");
    } else {
        p::info("Instance storage:");
        for (index, entry) in inspect.instance_storage.iter().enumerate() {
            p::kv(&format!("  Key {}", index + 1), &entry.key);
            p::kv(&format!("  Val {}", index + 1), &entry.value);
        }
    }

    p::separator();
    p::step(2, 2, "Reading contract build metadata…");

    if let Some(metadata) = metadata {
        p::header("Build Provenance");
        println!("{}", metadata);
    } else {
        p::info("No contract build metadata found.");
    }

    p::separator();
    Ok(())
}

fn handle_inspect_wasm(wasm: &Path, json: bool) -> Result<()> {
    if !wasm.exists() {
        anyhow::bail!("WASM file does not exist: {}", wasm.display());
    }

    if !wasm.is_file() {
        anyhow::bail!("WASM path is not a file: {}", wasm.display());
    }

    let metadata = get_contract_metadata(Some(wasm), None, None).ok_or_else(|| {
        anyhow::anyhow!(
            "Could not read contract metadata from WASM: {}",
            wasm.display()
        )
    })?;

    if json {
        let metadata_value = serde_json::from_str::<serde_json::Value>(&metadata)
            .unwrap_or_else(|_| serde_json::Value::String(metadata.clone()));

        let output = serde_json::json!({
            "wasm": wasm.display().to_string(),
            "metadata": metadata_value
        });

        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    p::header("Inspect Contract WASM");
    p::separator();
    p::kv("WASM", &wasm.display().to_string());
    p::separator();

    p::header("Build Provenance");
    println!("{}", metadata);

    p::separator();
    Ok(())
}

fn get_contract_metadata(
    wasm: Option<&Path>,
    contract_id: Option<&str>,
    network: Option<&str>,
) -> Option<String> {
    let mut command = Command::new("stellar");
    command.args(["contract", "info", "meta"]);

    if let Some(wasm) = wasm {
        command.arg("--wasm").arg(wasm);
    } else if let Some(contract_id) = contract_id {
        command.arg("--contract-id").arg(contract_id);

        if let Some(network) = network {
            command.arg("--network").arg(network);
        }
    } else {
        return None;
    }

    command.arg("--output").arg("json-formatted");

    let output = command.output().ok()?;

    if !output.status.success() {
        return None;
    }

    let metadata = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if metadata.is_empty() {
        None
    } else {
        Some(metadata)
    }
}

fn git_output(args: &[&str], directory: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn normalize_repository_url(repository: &str) -> String {
    if let Some(path) = repository.strip_prefix("git@github.com:") {
        return format!("https://github.com/{}", path.trim_end_matches(".git"));
    }

    repository.trim_end_matches(".git").to_string()
}

fn handle_build(args: BuildArgs) -> Result<()> {
    let current_dir = std::env::current_dir()?;

    let project_dir = args
        .manifest_path
        .as_ref()
        .and_then(|manifest| Path::new(manifest).parent())
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| current_dir.clone());

    let mut command = Command::new("stellar");
    command.args(["contract", "build"]);

    if let Some(manifest_path) = &args.manifest_path {
        command.args(["--manifest-path", manifest_path]);
    }

    if !args.no_provenance {
        if let Some(repository) =
            git_output(&["config", "--get", "remote.origin.url"], &project_dir)
        {
            command.arg("--meta").arg(format!(
                "source_repo={}",
                normalize_repository_url(&repository)
            ));
        } else {
            p::warn(
                "Could not determine the Git repository URL. \
                 Build will continue without source_repo metadata.",
            );
        }

        if let Some(commit) = git_output(&["rev-parse", "HEAD"], &project_dir) {
            command.arg("--meta").arg(format!("commit_sha={commit}"));
        } else {
            p::warn(
                "Could not determine the Git commit SHA. \
                 Build will continue without commit_sha metadata.",
            );
        }

        command
            .arg("--meta")
            .arg(format!("starforge_version={}", env!("CARGO_PKG_VERSION")));
    }

    p::header("Build Soroban Contract");
    p::separator();

    if args.no_provenance {
        p::info("StarForge provenance metadata disabled.");
    } else {
        p::info("Embedding StarForge build provenance metadata.");
    }

    p::separator();

    let status = command.status().map_err(|error| {
        anyhow::anyhow!(
            "Failed to execute `stellar contract build`: {error}. \
                 Make sure the Stellar CLI is installed and available on PATH."
        )
    })?;

    if !status.success() {
        anyhow::bail!("`stellar contract build` failed with status {status}");
    }

    p::separator();
    p::success("Contract build completed successfully.");

    if !args.no_provenance {
        p::info(
            "Provenance includes the repository, commit SHA, and StarForge version \
             when Git information is available.",
        );
    }

    Ok(())
}

fn fetch_contract_spec(
    contract_id: &str,
    network: &str,
) -> Result<crate::utils::bindings::ContractMetadata> {
    let output = std::process::Command::new("stellar")
        .args([
            "contract",
            "fetch",
            "--id",
            contract_id,
            "--network",
            network,
        ])
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "Failed to fetch contract WASM for {}: {}",
            contract_id,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let entries = crate::utils::bindings::read_spec_entries(&output.stdout)?;
    Ok(crate::utils::bindings::parse_spec_entries(&entries))
}

async fn handle_invoke(args: InvokeArgs) -> Result<()> {
    if args.contract_id == "--help" || args.contract_id == "-h" {
        use clap::CommandFactory;
        let mut cmd = InvokeArgs::command();
        cmd.print_help()?;
        return Ok(());
    }

    p::header("Invoke Soroban Contract");

    config::validate_contract_id(&args.contract_id)?;
    config::validate_network(&args.network)?;

    let function_name = args.function.clone().unwrap_or_default();
    let wants_contract_help =
        function_name.is_empty() || function_name == "--help" || function_name == "-h";
    let wants_func_help =
        args.slop.contains(&"--help".to_string()) || args.slop.contains(&"-h".to_string());

    let metadata = fetch_contract_spec(&args.contract_id, &args.network)?;

    if wants_contract_help {
        println!("Available functions for contract {}:\n", args.contract_id);
        for f in &metadata.functions {
            let inputs = f
                .inputs
                .iter()
                .map(|i| format!("{}: {}", i.name, i.type_name))
                .collect::<Vec<_>>()
                .join(", ");
            println!("  - {} ({})", f.name, inputs);
        }
        return Ok(());
    }

    let func_spec = metadata
        .functions
        .iter()
        .find(|f| f.name == function_name)
        .ok_or_else(|| anyhow::anyhow!("Function '{}' not found in contract", function_name))?;

    if wants_func_help {
        println!(
            "Usage: starforge contract invoke {} {} [OPTIONS]\n",
            args.contract_id, function_name
        );
        println!("Arguments:");
        for i in &func_spec.inputs {
            println!("  --{} <{}>", i.name, i.type_name);
        }
        return Ok(());
    }

    let mut parsed_args = args.args.clone();
    let mut parsed_types = args.types.clone();

    if !parsed_types.is_empty() || !parsed_args.is_empty() {
        p::warn("The --type flag is deprecated. Arguments are now typed automatically from the contract spec.");
        if parsed_args.len() != parsed_types.len() && !parsed_types.is_empty() {
            anyhow::bail!(
                "Argument count mismatch: {} args but {} types specified",
                parsed_args.len(),
                parsed_types.len()
            );
        }
        if parsed_types.is_empty() {
            parsed_types = vec!["string".to_string(); parsed_args.len()];
        }
    } else if !func_spec.inputs.is_empty() {
        let mut cmd = clap::Command::new(&func_spec.name)
            .no_binary_name(true)
            .ignore_errors(false);

        for input in &func_spec.inputs {
            cmd = cmd.arg(
                clap::Arg::new(&input.name)
                    .long(&input.name)
                    .required(true)
                    .help(input.type_name.clone()),
            );
        }

        let matches = cmd
            .try_get_matches_from(&args.slop)
            .map_err(|e| anyhow::anyhow!("Invalid arguments for '{}':\n{}", function_name, e))?;

        for input in &func_spec.inputs {
            let val: String = matches.get_one::<String>(&input.name).unwrap().clone();
            parsed_args.push(val);
            parsed_types.push(input.type_name.clone());
        }
    }

    p::separator();
    p::kv("Contract ID", &args.contract_id);
    p::kv("Function", &function_name);
    p::kv("Network", &args.network);

    if !parsed_args.is_empty() {
        p::kv("Arguments", &format!("{} args", parsed_args.len()));
        for (i, (arg, arg_type)) in parsed_args.iter().zip(parsed_types.iter()).enumerate() {
            p::kv(
                &format!("  Arg {}", i + 1),
                &format!("{} ({})", arg, arg_type),
            );
        }
    } else {
        p::kv("Arguments", "none");
    }

    if args.network == "mainnet" && !args.json {
        p::warn("You are invoking on MAINNET. This may cost real XLM if submitted.");
    }

    // Prefer loading a wallet whenever restore or submit may need one. For
    // plain simulation we still try the default wallet so a restorePreamble
    // can be handled interactively when keys are available.
    let (submit_wallet, signing_request) = {
        let cfg = config::load()?;
        let maybe = if let Some(ref wallet_name) = args.wallet {
            Some(
                cfg.wallets
                    .iter()
                    .find(|w| &w.name == wallet_name)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Wallet '{}' not found. Run `starforge wallet list`",
                            wallet_name
                        )
                    })?
                    .clone(),
            )
        } else if args.submit || args.auto_restore {
            if cfg.wallets.is_empty() {
                anyhow::bail!(
                    "No wallets found for submission. Create one first:\n  starforge wallet create deployer --fund"
                );
            }
            if !args.json {
                p::info(&format!(
                    "No --wallet specified. Using: {}",
                    cfg.wallets[0].name.cyan()
                ));
            }
            Some(cfg.wallets[0].clone())
        } else {
            cfg.wallets.first().cloned()
        };

        match maybe {
            Some(wallet) => {
                if !args.json && (args.submit || args.auto_restore) {
                    p::kv("Wallet", &wallet.name);
                }
                if (args.submit || args.auto_restore)
                    && wallet.secret_key.is_none()
                    && args.hardware.is_none()
                {
                    anyhow::bail!(
                        "Wallet '{}' has no local secret key. Use --hardware ledger or --hardware trezor.",
                        wallet.name
                    );
                }
                let signing = wallet_signer::SigningRequest::from_options(
                    Some(&wallet),
                    args.hardware,
                    Some(&args.hd_path),
                    &args.network,
                    true,
                    "contract invocation",
                )?;
                (Some(wallet), Some(signing))
            }
            None => (None, None),
        }
    };

    if !args.json {
        p::separator();
        println!();
        p::step(
            1,
            if args.submit { 2 } else { 1 },
            "Simulating contract invocation…",
        );
    }

    let restore_mode = if args.auto_restore {
        soroban::RestoreMode::Auto
    } else if submit_wallet.is_some() {
        soroban::RestoreMode::Prompt
    } else {
        soroban::RestoreMode::Never
    };

    let outcome = soroban::invoke_contract_with_options(
        &args.contract_id,
        &function_name,
        &parsed_args,
        &parsed_types,
        &args.network,
        submit_wallet.as_ref(),
        signing_request.as_ref(),
        soroban::InvokeOptions {
            restore: restore_mode,
            yes: args.yes || args.auto_restore || args.json,
            submit: false,
        },
    )
    .await?;

    finalize_invoke_after_sim(
        args,
        &function_name,
        &parsed_args,
        &parsed_types,
        outcome,
        submit_wallet,
        signing_request,
    )
    .await
}

async fn finalize_invoke_after_sim(
    args: InvokeArgs,
    function_name: &str,
    parsed_args: &[String],
    parsed_types: &[String],
    outcome: soroban::InvokeOutcome,
    submit_wallet: Option<WalletEntry>,
    signing_request: Option<wallet_signer::SigningRequest>,
) -> Result<()> {
    let simulation_result = &outcome.simulation;

    if args.json {
        let payload = serde_json::json!({
            "contract_id": args.contract_id,
            "function": function_name,
            "network": args.network,
            "return_value": simulation_result.return_value,
            "fee_stroops": simulation_result.fee,
            "restored": outcome.restored,
            "restore_fee_stroops": outcome.restore_fee_stroops,
            "restore_tx_hash": outcome.restore_tx_hash,
            "events": simulation_result.events,
            "errors": simulation_result.errors,
            "submitted": false,
            "tx_hash": serde_json::Value::Null,
        });
        if !args.submit {
            println!("{}", serde_json::to_string_pretty(&payload)?);
            return Ok(());
        }
        // Fall through to submit, then emit final JSON.
    } else {
        if outcome.restored {
            p::success("Restored archived ledger entries before invoke");
            if let Some(fee) = outcome.restore_fee_stroops {
                p::kv("Restore fee (stroops)", &fee.to_string());
            }
            if let Some(hash) = &outcome.restore_tx_hash {
                p::kv("Restore TX", hash);
            }
        }
        p::kv_accent("Simulation", "✓ Success");
        p::kv("Return Value", &simulation_result.return_value);
        p::kv("Fee (stroops)", &simulation_result.fee.to_string());
        p::kv(
            "Fee (XLM)",
            &format!("{:.7}", simulation_result.fee as f64 / 10_000_000.0),
        );

        if let Some(resources) = simulation_result.resources.as_ref() {
            if resources.requires_restore() && !outcome.restored {
                p::warn(
                    "Simulation reported archived entries (restorePreamble). \
                     Re-run with --auto-restore or --submit to restore them first.",
                );
            }
        }

        if !simulation_result.events.is_empty() {
            p::kv(
                "Events",
                &format!("{} emitted", simulation_result.events.len()),
            );
            for (i, event) in simulation_result.events.iter().enumerate() {
                p::kv(&format!("  Event {}", i + 1), event);
            }
        }
    }

    if args.submit {
        let submit_wallet_ref = submit_wallet
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("--submit requires a wallet; pass --wallet <name>"))?;

        let risk_level = if args.network == "mainnet" {
            crate::utils::confirmation::RiskLevel::High
        } else {
            crate::utils::confirmation::RiskLevel::Medium
        };

        if !args.json {
            let mut summary = crate::utils::confirmation::OperationSummary::new(
                if args.hardware.is_some() {
                    "Hardware Wallet — Invoke Contract".to_string()
                } else {
                    "Invoke Contract Function".to_string()
                },
                args.network.clone(),
                risk_level,
            )
            .add("Contract ID", &args.contract_id)
            .add("Function", function_name)
            .add(
                "Wallet",
                if args.hardware.is_some() {
                    format!("{} (Hardware)", submit_wallet_ref.name)
                } else {
                    submit_wallet_ref.name.clone()
                },
            )
            .add(
                "Estimated Fee",
                format!("{} stroops", simulation_result.fee),
            )
            .add("Return Value", &simulation_result.return_value)
            .with_auth_trees(simulation_result.auth.clone());

            if args.hardware.is_some() {
                summary = summary.add("Next step", "Review and approve on your device screen");
            }

            let confirm_config = crate::utils::confirmation::ConfirmationConfig {
                risk_level,
                network: args.network.clone(),
                skip_confirm: args.yes,
                dry_run: false,
                prompt: if args.hardware.is_some() {
                    Some("Proceed with hardware wallet signing?".to_string())
                } else {
                    Some("Submit this transaction?".to_string())
                },
                require_type_confirmation: args.network == "mainnet" && !args.yes,
                ..Default::default()
            };

            if !crate::utils::confirmation::confirm_operation(&summary, &confirm_config)? {
                anyhow::bail!("Transaction submission cancelled.");
            }

            if let Some(kind) = args.hardware {
                p::info(&format!(
                    "Connect your {} and approve the invocation on the device screen.",
                    kind
                ));
            }

            println!();
            p::step(2, 2, "Submitting transaction…");
        }

        let tx_result = soroban::submit_transaction(
            &args.contract_id,
            function_name,
            parsed_args,
            parsed_types,
            &args.network,
            submit_wallet_ref,
            signing_request.as_ref(),
            simulation_result.fee,
        )
        .await?;

        if args.json {
            let payload = serde_json::json!({
                "contract_id": args.contract_id,
                "function": function_name,
                "network": args.network,
                "return_value": tx_result.return_value,
                "fee_stroops": simulation_result.fee,
                "restored": outcome.restored,
                "restore_fee_stroops": outcome.restore_fee_stroops,
                "restore_tx_hash": outcome.restore_tx_hash,
                "events": simulation_result.events,
                "errors": simulation_result.errors,
                "submitted": true,
                "tx_hash": tx_result.hash,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        } else {
            p::kv_accent("Transaction", "✓ Submitted");
            p::kv("TX Hash", &tx_result.hash);
            p::kv("Return Value", &tx_result.return_value);
            p::separator();
        }
    } else if !args.json {
        println!();
        p::info("Simulation complete. Add --submit to execute the transaction.");
        p::separator();
    }

    Ok(())
}

async fn handle_ttl(cmd: TtlCommands) -> Result<()> {
    match cmd {
        TtlCommands::Show(args) => handle_ttl_show(args).await,
        TtlCommands::Extend(args) => handle_ttl_extend(args).await,
    }
}

async fn handle_ttl_show(args: TtlShowArgs) -> Result<()> {
    let report = crate::utils::contract_ttl::show_ttl(
        &args.contract_id,
        &args.network,
        &args.keys,
        args.warn_below,
    )
    .await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        p::header("Contract TTL");
        p::kv("Contract", &report.contract_id);
        p::kv("Network", &report.network);
        p::kv("Latest ledger", &report.latest_ledger.to_string());
        p::separator();
        for entry in &report.entries {
            p::info(&format!("{} ({})", entry.key, entry.kind));
            match entry.live_until_ledger {
                Some(lu) => p::kv("  Live until", &lu.to_string()),
                None => p::kv("  Live until", "unknown / missing"),
            }
            match entry.remaining_ledgers {
                Some(r) => p::kv("  Remaining", &format!("{r} ledgers")),
                None => p::kv("  Remaining", "unknown"),
            }
            if let Some(eta) = &entry.eta {
                p::kv("  ETA", eta);
            }
        }
        if let Some(threshold) = args.warn_below {
            if !report.low_ttl.is_empty() {
                p::warn(&format!(
                    "{} entr{} below --warn-below {threshold}",
                    report.low_ttl.len(),
                    if report.low_ttl.len() == 1 {
                        "y"
                    } else {
                        "ies"
                    }
                ));
            } else {
                p::success(&format!(
                    "All entries have at least {threshold} ledgers of TTL remaining"
                ));
            }
        }
        p::separator();
    }

    if args.warn_below.is_some() && !report.low_ttl.is_empty() {
        anyhow::bail!(
            "TTL warning: {} entr{} below threshold",
            report.low_ttl.len(),
            if report.low_ttl.len() == 1 {
                "y"
            } else {
                "ies"
            }
        );
    }
    Ok(())
}

async fn handle_ttl_extend(args: TtlExtendArgs) -> Result<()> {
    let cfg = config::load()?;
    let wallet = if let Some(ref name) = args.wallet {
        cfg.wallets
            .iter()
            .find(|w| &w.name == name)
            .ok_or_else(|| anyhow::anyhow!("Wallet '{name}' not found"))?
            .clone()
    } else if !cfg.wallets.is_empty() {
        if !args.json {
            p::info(&format!(
                "No --wallet specified. Using: {}",
                cfg.wallets[0].name.cyan()
            ));
        }
        cfg.wallets[0].clone()
    } else {
        anyhow::bail!(
            "No wallets found. Create one with `starforge wallet create deployer --fund`"
        );
    };

    let signing = wallet_signer::SigningRequest::from_options(
        Some(&wallet),
        args.hardware,
        Some(&args.hd_path),
        &args.network,
        true,
        "extend TTL",
    )?;

    // Always simulate first to show cost.
    let estimate = crate::utils::contract_ttl::extend_ttl(
        &args.contract_id,
        &args.network,
        &args.keys,
        args.ledgers,
        &wallet,
        &signing,
        false,
    )
    .await?;

    if args.json && !args.submit {
        println!("{}", serde_json::to_string_pretty(&estimate)?);
        return Ok(());
    }

    if !args.json {
        p::header("Extend Contract TTL");
        p::kv("Contract", &args.contract_id);
        p::kv("Network", &args.network);
        p::kv("Extend to", &format!("{} ledgers from now", args.ledgers));
        p::kv("Keys", &estimate.keys_extended.to_string());
        p::kv(
            "Estimated fee",
            &format!(
                "{} stroops ({:.7} XLM)",
                estimate.estimated_fee_stroops, estimate.estimated_fee_xlm
            ),
        );
        p::separator();
    }

    if !args.submit {
        if !args.json {
            p::info("Simulation only. Re-run with --submit to broadcast the extend transaction.");
        }
        return Ok(());
    }

    if !args.yes && !args.json {
        let risk = if args.network == "mainnet" {
            crate::utils::confirmation::RiskLevel::High
        } else {
            crate::utils::confirmation::RiskLevel::Medium
        };
        let summary = crate::utils::confirmation::OperationSummary::new(
            "Extend Footprint TTL".to_string(),
            args.network.clone(),
            risk,
        )
        .add("Contract", &args.contract_id)
        .add("Ledgers", args.ledgers.to_string())
        .add(
            "Estimated fee",
            format!("{} stroops", estimate.estimated_fee_stroops),
        );
        let confirm = crate::utils::confirmation::ConfirmationConfig {
            risk_level: risk,
            network: args.network.clone(),
            skip_confirm: false,
            dry_run: false,
            prompt: Some("Submit ExtendFootprintTTL?".to_string()),
            require_type_confirmation: args.network == "mainnet",
            ..Default::default()
        };
        if !crate::utils::confirmation::confirm_operation(&summary, &confirm)? {
            anyhow::bail!("TTL extend cancelled.");
        }
    }

    let result = crate::utils::contract_ttl::extend_ttl(
        &args.contract_id,
        &args.network,
        &args.keys,
        args.ledgers,
        &wallet,
        &signing,
        true,
    )
    .await?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        p::success("TTL extended");
        if let Some(hash) = &result.tx_hash {
            p::kv("TX Hash", hash);
        }
        p::kv("Fee", &format!("{} stroops", result.estimated_fee_stroops));
        p::separator();
    }
    Ok(())
}

async fn handle_version(_args: crate::commands::contract::VersionArgs) -> Result<()> {
    Ok(())
}

fn handle_upload(args: UploadArgs) -> Result<()> {
    config::validate_network(&args.network)?;

    p::header("Upload WASM to Stellar Network");
    p::separator();
    p::kv("WASM", &args.wasm);
    p::kv("Network", &args.network);

    if args.network == "mainnet" {
        p::warn("You are uploading on MAINNET. This will cost real XLM.");
    }

    let cfg = config::load()?;
    let wallet = if let Some(ref name) = args.wallet {
        cfg.wallets
            .iter()
            .find(|w| &w.name == name)
            .ok_or_else(|| {
                anyhow::anyhow!("Wallet '{}' not found. Run `starforge wallet list`", name)
            })?
            .clone()
    } else if !cfg.wallets.is_empty() {
        p::info(&format!(
            "No --wallet specified. Using: {}",
            cfg.wallets[0].name.cyan()
        ));
        cfg.wallets[0].clone()
    } else {
        anyhow::bail!(
            "No wallets found. Create one first:\n  starforge wallet create deployer --fund"
        );
    };

    p::kv("Wallet", &wallet.name);
    p::separator();

    println!();
    p::step(1, 1, "Uploading WASM binary…");

    let wasm_hash = soroban::upload_wasm(&args.wasm, &args.network, &wallet)?;

    println!();
    p::kv_accent("WASM Hash", &wasm_hash);
    p::success("WASM uploaded successfully.");
    println!();
    p::info("Next step — create the contract instance:");
    p::info(&format!(
        "  stellar contract deploy --wasm-hash {} --network {} --source {}",
        wasm_hash, args.network, wallet.name
    ));
    println!();
    Ok(())
}

fn resolve_network(network_override: Option<String>) -> Result<String> {
    let network = network_override.unwrap_or(config::load()?.network);
    match network.as_str() {
        "testnet" | "mainnet" => Ok(network),
        _ => anyhow::bail!(
            "Unsupported network '{}'. Use 'testnet' or 'mainnet'.",
            network
        ),
    }
}

fn handle_call_graph(args: CallGraphArgs) -> Result<()> {
    config::validate_file_path(&args.path, Some("rs"))?;
    p::header("Cross-Contract Call Graph");
    p::kv("Source", &args.path.display().to_string());

    let graph = call_graph::extract_call_graph(&args.path)?;

    // Filter patterns by minimum severity, if requested.
    let effective_patterns: Vec<call_graph::CallPattern> = if let Some(min) = &args.severity {
        let rank = |s: &str| match s {
            "high" => 3,
            "medium" => 2,
            "low" => 1,
            _ => 0,
        };
        let threshold = rank(min);
        graph
            .patterns
            .iter()
            .filter(|p| rank(&p.severity) >= threshold)
            .cloned()
            .collect()
    } else {
        graph.patterns.clone()
    };

    let output = match args.format.as_str() {
        "dot" => call_graph::render_dot(&graph),
        "json" => {
            // Backwards-compatible JSON: keep the full `CallGraph` shape at the
            // top level (so existing consumers still work) and *additionally*
            // include `_stats` and `_filtered_patterns` next to it.
            let mut view = serde_json::to_value(&graph)?;
            if let Some(obj) = view.as_object_mut() {
                obj.insert(
                    "_stats".to_string(),
                    serde_json::to_value(call_graph::compute_stats(&graph))?,
                );
                obj.insert(
                    "_filtered_patterns".to_string(),
                    serde_json::to_value(&effective_patterns)?,
                );
            }
            serde_json::to_string_pretty(&view)?
        }
        _ => call_graph::render_ascii(&graph),
    };

    if let Some(out_path) = &args.out {
        std::fs::write(out_path, &output)?;
        p::kv("Output saved", &out_path.display().to_string());
    } else {
        println!("{}", output);
    }

    p::separator();
    p::kv("Nodes", &graph.nodes.len().to_string());
    p::kv("Edges", &graph.edges.len().to_string());
    p::kv("Dependencies", &graph.dependencies.len().to_string());

    if args.patterns && !effective_patterns.is_empty() {
        println!();
        p::header("Pattern Analysis");
        for pat in &effective_patterns {
            let icon = match pat.severity.as_str() {
                "high" => "⚠",
                "medium" => "⚡",
                _ => "ℹ",
            };
            println!("  {} [{}] {}", icon, pat.severity.to_uppercase(), pat.name);
            println!("     {}", pat.description);
        }
        println!();
        p::info("Use `starforge security audit <path>` for a full security report.");
    }

    if args.stats {
        let stats = call_graph::compute_stats(&graph);
        println!();
        p::header("Graph Statistics");
        println!(
            "  {:<24} {}",
            "Total nodes".dimmed(),
            stats.total_nodes.to_string().bright_white()
        );
        println!(
            "  {:<24} {}",
            "Total edges".dimmed(),
            stats.total_edges.to_string().bright_white()
        );
        println!(
            "  {:<24} {}",
            "  external".dimmed(),
            stats.external_edges.to_string().bright_white()
        );
        println!(
            "  {:<24} {}",
            "  internal".dimmed(),
            stats.internal_edges.to_string().bright_white()
        );
        println!(
            "  {:<24} {}",
            "Direct invokes".dimmed(),
            stats.direct_invokes.to_string().bright_white()
        );
        println!(
            "  {:<24} {}",
            "Client constructions".dimmed(),
            stats.client_constructions.to_string().bright_white()
        );
        println!(
            "  {:<24} {}",
            "Dependencies".dimmed(),
            stats.dependencies.to_string().bright_white()
        );
        println!(
            "  {:<24} {}",
            "Patterns (h/m/l)".dimmed(),
            format!(
                "{} / {} / {}",
                stats.patterns_high, stats.patterns_medium, stats.patterns_low
            )
            .bright_white()
        );
        println!(
            "  {:<24} {}",
            "Max out-degree".dimmed(),
            stats.fan_out_max.to_string().bright_white()
        );
        println!(
            "  {:<24} {}",
            "Max in-degree".dimmed(),
            stats.fan_in_max.to_string().bright_white()
        );
    }

    if args.optimize {
        // Compute suggestions on the unfiltered graph (most suggestions are
        // derived from edges / dependencies, not patterns) and then post-filter
        // by priority so `--severity=high` hides low / medium hints without
        // paying for a full graph clone.
        let all = call_graph::generate_suggestions(&graph);
        let suggestions: Vec<_> = if let Some(min) = &args.severity {
            let rank = |p: &str| match p {
                "high" => 3,
                "medium" => 2,
                _ => 1,
            };
            let threshold = rank(min);
            all.into_iter()
                .filter(|s| rank(&s.priority) >= threshold)
                .collect()
        } else {
            all
        };
        println!();
        p::header("Optimization Suggestions");
        if suggestions.is_empty() {
            p::info("No optimization opportunities detected.");
        } else {
            for sug in &suggestions {
                let icon = match sug.priority.as_str() {
                    "high" => "▲".red(),
                    "medium" => "●".yellow(),
                    _ => "·".cyan(),
                };
                println!(
                    "  {} [{}] {} → {}",
                    icon,
                    sug.priority.to_uppercase().dimmed(),
                    sug.title.bright_white(),
                    sug.target.bright_green()
                );
                println!("      {}", sug.detail.dimmed());
                if let Some(s) = &sug.estimated_savings {
                    println!("      est. savings: {}", s.cyan());
                }
            }
        }
    }

    p::success("Call graph extraction complete");

    if args.explore {
        call_graph::explore_graph(&graph)?;
    }

    Ok(())
}

fn handle_deps(args: DepsArgs) -> Result<()> {
    use crate::utils::contract_deps;
    let cwd = std::env::current_dir()?;

    match args.cmd {
        DepsCommands::Init => {
            contract_deps::init(&cwd)?;
            p::success("Initialized contract-dependencies.toml");
        }
        DepsCommands::Add(add_args) => {
            let source = if add_args.path.is_some() || add_args.git.is_some() {
                contract_deps::DependencySource::Detailed {
                    version: add_args.version,
                    path: add_args.path,
                    git: add_args.git,
                    branch: add_args.branch,
                }
            } else if let Some(v) = add_args.version {
                contract_deps::DependencySource::Version(v)
            } else {
                anyhow::bail!("Must specify at least --version, --path, or --git");
            };

            contract_deps::add_dependency(&cwd, &add_args.name, source)?;
            p::success(&format!("Added dependency '{}'", add_args.name));
        }
        DepsCommands::Update(update_args) => {
            contract_deps::update_dependency(&cwd, &update_args.name, &update_args.version)?;
            p::success(&format!(
                "Updated dependency '{}' to '{}'",
                update_args.name, update_args.version
            ));
        }
        DepsCommands::Resolve => {
            p::header("Contract Dependency Deployment Order");
            let graph = contract_deps::resolve_graph(&cwd)?;
            let order = contract_deps::resolve_deployment_order(&graph)?;
            for (i, name) in order.iter().enumerate() {
                p::step(i + 1, order.len(), name);
            }
            if order.is_empty() {
                p::info("No dependencies found.");
            }
        }
        DepsCommands::Graph(graph_args) => {
            let graph = contract_deps::resolve_graph(&cwd)?;
            match graph_args.format.as_str() {
                "ascii" => {
                    let out = contract_deps::render_ascii_graph(&graph);
                    println!("{}", out);
                }
                "dot" => {
                    let out = contract_deps::render_dot_graph(&graph);
                    println!("{}", out);
                }
                _ => anyhow::bail!("Unsupported format. Use 'ascii' or 'dot'"),
            }
        }
    }

    Ok(())
}
