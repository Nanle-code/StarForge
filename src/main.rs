pub use starforge::commands;
pub mod curation;
pub use starforge::plugins;
pub use starforge::utils;

use clap::{Parser, Subcommand};
use colored::*;
use std::sync::Once;

#[derive(Parser)]
#[command(
    name = "starforge",
    about = "⚡ Stellar & Soroban developer productivity CLI",
    long_about = "starforge is an open-source CLI toolkit for developers building on the Stellar network.\nManage wallets, deploy Soroban contracts, and scaffold new projects — all from your terminal.",
    version = "0.1.0",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable machine-readable JSON output for supported commands
    #[arg(long, global = true)]
    json: bool,

    /// Simulate every state-changing command: print the full plan of operations
    /// (and costs) it would perform, then exit without writing any file or
    /// submitting any transaction. Combine with `--json` for a machine-readable
    /// plan. See docs/DRY_RUN_SEMANTICS.md.
    #[arg(long, global = true)]
    dry_run: bool,

    /// Suppress the ASCII banner and decorative output
    #[arg(long, short = 'q', global = true)]
    quiet: bool,

    /// Disable color and decorative Unicode symbols (✓/✗/⚠/→) in favor of
    /// ASCII labels ([OK]/[ERROR]/[WARN]/[INFO]), for screen readers,
    /// braille displays, and log files. Auto-detected from $NO_COLOR
    /// (https://no-color.org) or $STARFORGE_NO_COLOR when this flag is
    /// absent.
    #[arg(long, global = true)]
    plain: bool,

    /// Log output format: human (default) or json
    #[arg(long, global = true, default_value = "human", value_parser = ["human", "json"])]
    log_format: String,

    /// Directory to write rotating log files into (optional)
    #[arg(long, global = true)]
    log_dir: Option<std::path::PathBuf>,

    /// Correlation ID tying every log line of this invocation together.
    /// Defaults to $STARFORGE_CORRELATION_ID, or a freshly generated value.
    /// Must be 8–64 characters of [A-Za-z0-9_-].
    #[arg(long, global = true)]
    correlation_id: Option<String>,

    /// Never block on an interactive prompt: fail with a clear error
    /// instead, pointing to the env var or flag that supplies the value
    /// headlessly. Auto-detected when $CI is set or stdin isn't a terminal
    /// (also settable via $STARFORGE_NON_INTERACTIVE).
    #[arg(long, global = true)]
    non_interactive: bool,

    /// Allow signing when the configured passphrase differs from the connected endpoint.
    /// This is unsafe and should only be used with a deliberately trusted endpoint.
    #[arg(long, global = true, hide = true)]
    allow_network_passphrase_mismatch: bool,

    /// Allow mainnet signing with an unencrypted plaintext secret key.
    /// This is unsafe and should only be used for deliberate legacy operations.
    #[arg(long, global = true)]
    allow_plaintext_mainnet: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage test wallets (create, list, fund, sign), transactions, and devices
    #[command(subcommand)]
    Wallet(commands::wallet::WalletCommands),

    /// Contract operations (invoke, build, test, audit, upgrade, inspect, monitor)
    #[command(subcommand)]
    Contract(commands::contract::ContractCommands),

    /// Deploy a compiled Soroban contract and manage the deployment lifecycle
    #[command(subcommand)]
    Deploy(commands::tree::DeployTree),

    /// View or switch the active network, run a local node, simulate, snapshot
    #[command(subcommand)]
    Network(commands::network::NetworkCommands),

    /// Manage community contract templates, versions, and the registry
    #[command(subcommand)]
    Wallet(commands::wallet::WalletCommands),
    /// On-chain account lifecycle with sponsored reserves (CAP-33)
    #[command(subcommand)]
    Account(commands::account::AccountCommands),
    /// Natural language command interface
    Nl(commands::nl::NlArgs),

    /// Manage third-party plugins
    #[command(subcommand)]
    Plugin(commands::plugin::PluginCommands),

    /// Contract operations (invoke, inspect, etc.)
    #[command(subcommand)]
    Contract(commands::contract::ContractCommands),
    /// Generate smart contracts from natural language prompts
    #[command(subcommand)]
    Generate(commands::generate::GenerateCommands),
    /// Smart contract completion assistant
    #[command(subcommand)]
    Complete(commands::complete::CompleteCommands),
    /// External plugins
    #[command(external_subcommand)]
    External(Vec<String>),
    /// Debug Soroban contracts with breakpoints, stepping, and inspection
    #[command(subcommand)]
    Debug(commands::debug::DebugCommands),
    /// Deep contract storage inspection (state, key, storage)
    #[command(subcommand)]
    Inspect(commands::inspect::InspectCommands),
    /// Deploy a compiled Soroban contract (.wasm)
    Deploy(commands::deploy::DeployArgs),
    /// Watch contract sources and rebuild/redeploy on save
    Dev(commands::dev::DevArgs),
    /// Deployment history, rollback, verification, and dashboard
    #[command(subcommand)]
    Deployments(commands::deployments::DeploymentsCommands),
    /// Manage deployment environments (dev/staging/production): configuration, promotion, isolation, and a dashboard
    #[command(subcommand)]
    Environment(commands::environment::EnvironmentCommands),
    /// Show starforge config and environment info
    Info,
    /// Manage AI prompt templates and versioning
    #[command(subcommand)]
    Prompts(commands::prompts::PromptsCommands),
    /// Analyze and explain smart contract code using AI
    #[command(subcommand)]
    Ai(commands::tree::AiTree),

    /// Manage starforge configuration, telemetry, feature flags, and privacy
    #[command(subcommand)]
    Config(commands::config::ConfigCommands),

    /// Project scaffolding and AI-driven project management
    #[command(subcommand)]
    Project(commands::project::ProjectCommands),

    /// Developer-environment utilities: tutorials, natural language, PR checks
    #[command(subcommand)]
    Tool(commands::tree::ToolTree),

    /// Generate shell completions for bash, zsh, fish, and powershell
    #[command(subcommand)]
    Completions(commands::completions::CompletionShell),

    /// Generate or install man pages
    #[command(subcommand)]
    Man(commands::man::ManCommand),

    /// Smart autocomplete — suggest and record commands
    #[command(hide = true)]
    Autocomplete {
        /// Show suggestions for this partial command
        #[arg(long)]
        suggest: Option<String>,

        /// Record this command in history
        #[arg(long)]
        record: Option<String>,

        /// Interactive autocomplete mode
        #[arg(long, short)]
        interactive: bool,

        /// Clear command history
        #[arg(long)]
        clear_history: bool,

        /// Show usage statistics
        #[arg(long)]
        stats: bool,
    },

    /// External plugins
    #[command(external_subcommand)]
    External(Vec<String>),

    /// Manage per-network contract and account aliases
    #[command(subcommand)]
    Alias(commands::alias::AliasCommands),

    /// Terminal User Interface for wallets, contracts, and transactions
    #[cfg(feature = "ui")]
    Ui(commands::ui::UiArgs),
}

static OUTPUT_MODE_INIT: Once = Once::new();

/// Stack reserved for the thread that actually runs the CLI.
///
/// Windows gives the process main thread a 1 MiB stack by default, where Linux
/// and macOS give 8 MiB. Building this crate's clap command tree needs more
/// than 1 MiB in a debug build, so on Windows *every* invocation -- including
/// `--version` -- died in `Cli::parse()` with STATUS_STACK_OVERFLOW
/// (0xC00000FD) before reaching any command. Measured floor is between 1 and
/// 2 MiB; 8 MiB matches the Unix default and leaves room for the tree to grow.
const MAIN_STACK_SIZE: usize = 8 * 1024 * 1024;

/// Exit code Rust uses when the main thread panics.
const PANIC_EXIT_CODE: i32 = 101;

fn main() {
    // Run on an explicitly sized thread rather than the process main thread so
    // the stack does not depend on the platform default. rustc does the same
    // thing for the same reason.
    let worker = std::thread::Builder::new()
        .name("starforge-main".to_string())
        .stack_size(MAIN_STACK_SIZE)
        .spawn(run)
        .expect("failed to spawn the starforge main thread");

    if worker.join().is_err() {
        // The panic hook has already reported the payload; mirror the exit code
        // the runtime would have produced had this panicked on the main thread.
        std::process::exit(PANIC_EXIT_CODE);
    }
}

#[tokio::main]
async fn run() {
    let cli = Cli::parse();

    // Handle --help-all: show information about progressive disclosure
    if cli.help_all {
        eprintln!("StarForge Progressive Disclosure");
        eprintln!("===============================");
        eprintln!("");
        eprintln!("StarForge uses progressive disclosure to reduce help noise by hiding");
        eprintln!("advanced/power-user flags by default. These flags are typically used");
        eprintln!("by experienced users or for specialized workflows.");
        eprintln!("");
        eprintln!("To see all flags including hidden ones, you can:");
        eprintln!("  1. Use --help-all to see this message");
        eprintln!("  2. Set STARFORGE_SHOW_ALL_HELP=1 environment variable");
        eprintln!("");
        eprintln!("Common hidden flags include:");
        eprintln!("  --allow-network-passphrase-mismatch : Allow signing with mismatched passphrase (unsafe)");
        eprintln!("  --hardware <ledger|trezor>           : Use hardware wallet for signing");
        eprintln!("  --compliance                        : Run AI-driven compliance checks");
        eprintln!("");
        std::process::exit(0);
    }

    OUTPUT_MODE_INIT.call_once(|| {});
    utils::output::set_json_mode(cli.json);
    utils::output::set_plain_mode(cli.plain);
    // `--dry-run` is process-global (it may be passed before or after the
    // subcommand): record it so every command handler can short-circuit its
    // mutations behind a shared plan. Detection also scans the raw arguments
    // so it does not depend on where clap attached the global value.
    let dry_run_requested = cli.dry_run
        || std::env::args_os().any(|arg| arg.to_str() == Some("--dry-run"));
    utils::dry_run::set_enabled(dry_run_requested);
    if utils::output::is_plain_mode_enabled() {
        // Global override: neutralizes every `colored` call in the codebase,
        // not only the ones in utils::print that also swap their Unicode
        // symbols for ASCII labels, so plain mode is not a partial effort
        // that still leaves ANSI escapes in less-visited output paths.
        colored::control::set_override(false);
    }
    utils::interactive::set_non_interactive(cli.non_interactive);
    utils::network_guard::set_allow_mismatch(cli.allow_network_passphrase_mismatch);
    utils::network_guard::set_allow_plaintext_mainnet(cli.allow_plaintext_mainnet);

    // Initialise structured logging before anything else runs.
    let log_cfg =
        utils::logging::config_from_env(Some(cli.log_format.as_str()), cli.log_dir.clone());
    if let Err(e) = utils::logging::init(log_cfg) {
        eprintln!("Warning: failed to initialise logger: {}", e);
    }

    // Resolve the correlation ID before any command runs so every span, retry,
    // network request, plugin call, and deployment step shares it. An invalid
    // explicit value is fatal: silently generating a different ID would break
    // the log join the caller asked for.
    let correlation_id = match utils::correlation::resolve(cli.correlation_id.as_deref()) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Invalid correlation ID: {}", e);
            utils::exit_codes::ExitCode::Usage.exit();
        }
    };
    utils::correlation::init(correlation_id);

    // Completion scripts are sourced by the shell, so stdout must be pure script.
    // The same holds whenever stdout is redirected or piped: the `tx` XDR toolbox
    // emits envelopes and JSON there (`tx encode | tx sign | tx submit`), and the
    // banner would corrupt the payload.
    use std::io::IsTerminal;
    if !cli.quiet
        && !matches!(cli.command, Commands::Completions(_))
        && std::io::stdout().is_terminal()
    {
        print_banner();
    }

    // One stable name per noun, used for telemetry, logging, and recovery hints.
    // The leaf verb is not included so the metric cardinality stays flat and a
    // new verb never needs a telemetry-schema change.
    let command_name = match &cli.command {
        Commands::Wallet(_) => "wallet",
        Commands::Contract(_) => "contract",
        Commands::Deploy(_) => "deploy",
        Commands::Dev(_) => "dev",
        Commands::Deployments(_) => "deployments",
        Commands::Environment(_) => "environment",
        Commands::Info => "info",
        Commands::BugReport(_) => "bug-report",
        Commands::Prompts(_) => "prompts",
        Commands::Explain(_) => "explain",
        Commands::Config(_) => "config",
        Commands::Telemetry(_) => "telemetry",
        Commands::Tx(_) => "tx",
        Commands::Sep10(_) => "sep10",
        Commands::Network(_) => "network",
        Commands::Template(_) => "template",
        Commands::Plugin(_) => "plugin",
        Commands::Ai(_) => "ai",
        Commands::Config(_) => "config",
        Commands::Project(_) => "project",
        Commands::Tool(_) => "tool",
        Commands::Completions(_) => "completions",
        Commands::Man(_) => "man",
        Commands::Autocomplete { .. } => "autocomplete",
        Commands::External(_) => "external",
        Commands::Verify(_) => "verify",
        Commands::Help(_) => "help",
        Commands::AiTelemetry(_) => "ai-telemetry",
        Commands::Optimize(_) => "optimize",
        Commands::AiSecurityTraining(_) => "ai-security-training",
        Commands::ContractMonitor(_) => "contract-monitor",
        Commands::Alias(_) => "alias",
        #[cfg(feature = "ui")]
        Commands::Ui(_) => "ui",
    }
    .to_string();

    // Root span: everything below inherits `correlation_id` through the span
    // stack, including work done inside spawned command handlers.
    let command_span = utils::correlation::command_span(&command_name);
    let _command_guard = command_span.enter();
    tracing::info!(
        correlation_id = %utils::correlation::current_str(),
        command = %command_name,
        "command started"
    );

    let start = std::time::Instant::now();
    let result = match cli.command {
        // Every handler lives in the module that owns the command; the noun
        // enums added by ADR 0007 forward to those modules unchanged.
        Commands::Wallet(cmd) => commands::wallet::handle(cmd).await,
        Commands::Account(cmd) => commands::account::handle(cmd).await,
        Commands::Nl(args) => commands::nl::handle(args).await,
        Commands::New(cmd) => commands::new::handle(cmd).await,
        Commands::Generate(cmd) => commands::generate::handle(&cmd).await,
        Commands::Contract(cmd) => commands::contract::handle(cmd).await,
        Commands::Inspect(cmd) => commands::inspect::handle(cmd).await,
        Commands::Debug(cmd) => commands::debug::handle(cmd).await,
        Commands::Deploy(args) => commands::deploy::handle(args).await,
        Commands::Dev(args) => commands::dev::handle(args).await,
        Commands::Deployments(cmd) => commands::deployments::handle(cmd).await,
        Commands::Environment(cmd) => commands::environment::handle(cmd),
        Commands::Info => commands::info::handle().await,
        Commands::BugReport(args) => commands::bug_report::handle(args).await,
        Commands::Prompts(cmd) => commands::prompts::handle(&cmd).await,
        Commands::Explain(ref cmd) => commands::explain::handle(cmd).await,
        Commands::Config(cmd) => commands::config::handle(cmd).await,
        Commands::Telemetry(cmd) => commands::telemetry::handle(cmd).await,
        Commands::Tx(args) => commands::tx::handle(args).await,
        Commands::Sep10(args) => commands::sep::handle(args).await,
        Commands::Network(cmd) => commands::network::handle(cmd).await,
        Commands::Template(cmd) => commands::template::handle(cmd).await,
        Commands::Plugin(cmd) => commands::plugin::handle(cmd).await,
        Commands::Ai(cmd) => commands::tree::handle_ai(cmd).await,
        Commands::Config(cmd) => commands::config::handle(cmd).await,
        Commands::Project(cmd) => commands::project::handle(cmd).await,
        Commands::Tool(cmd) => commands::tree::handle_tool(cmd).await,
        Commands::Completions(shell) => commands::completions::handle(shell).await,
        Commands::Man(cmd) => commands::man::handle(cmd).await,
        Commands::Autocomplete {
            suggest,
            record,
            interactive,
            clear_history,
            stats,
        } => {
            commands::autocomplete::handle_autocomplete(
                suggest,
                record,
                interactive,
                clear_history,
                stats,
            )
            .await
        }
        Commands::External(args) => handle_external_plugin(args),
        Commands::Help(args) => commands::help::handle(args).await,
        Commands::AiTelemetry(cmd) => commands::ai_telemetry::handle(cmd).await,
        Commands::Optimize(cmd) => commands::optimize::handle(cmd).await,
        Commands::AiSecurityTraining(cmd) => commands::ai_security_training::handle(cmd).await,
        Commands::ContractMonitor(cmd) => commands::contract_monitor::handle(cmd).await,
        Commands::Alias(cmd) => commands::alias::handle(cmd).await,
        #[cfg(feature = "ui")]
        Commands::Ui(args) => commands::ui::handle(args).await,
    };
    let duration = start.elapsed();

    tracing::info!(
        correlation_id = %utils::correlation::current_str(),
        command = %command_name,
        success = result.is_ok(),
        duration_ms = duration.as_millis() as u64,
        "command finished"
    );

    let _ = utils::telemetry::track_event(
        &command_name,
        serde_json::json!({
            "success": result.is_ok(),
            "duration_ms": duration.as_millis(),
        }),
    );

    if let Err(e) = result {
        let error_code = utils::errors::ErrorCode::classify(&command_name, &e);
        if utils::output::is_json_mode_enabled() {
            let _ = utils::output::print_error_json(error_code, &e.to_string());
            error_code.exit_code().exit();
        }

        let mut hints = recovery_hints(&command_name, &e);
        // Augment the static command-specific hints with the AI Contextual
        // Help engine. Patterns that did not match the static rule table
        // still produce a useful, command-agnostic one-liner.
        utils::context_help::troubleshoot_merging(&e.to_string(), &mut hints);
        utils::print::cli_error(&e, &hints.iter().map(String::as_str).collect::<Vec<_>>());
        let exit_code = error_code.exit_code();
        eprintln!("Error code: {}", error_code.id());
        eprintln!("Cause: {}", error_code.cause());
        eprintln!("Fix: {}", error_code.fix().trim());
        eprintln!("Exit: {} ({})", exit_code.code(), exit_code.name());
        eprintln!("Docs: {}", error_code.docs_url());
        exit_code.exit();
    }

    // On a successful run, optionally surface a single proactive tip.
    // Gated so the happy path stays cheap:
    //   * STARFORGE_HELP_TIPS=0 explicitly opts out;
    //   * telemetry must be enabled (it already touches the disk/network);
    //   * `proactive_tip` further ignores commands on its blocklist.
    // Truthy semantics: only the listed false-strings opt out. Any other
    // value ("1", "yes", " true", "", unset) keeps tips enabled; tighten
    // with care so we never regress "1" → disable.
    let tips_allowed = !cli.quiet
        && !utils::output::is_json_mode_enabled()
        && std::env::var("STARFORGE_HELP_TIPS")
            .map(|v| !matches!(v.to_lowercase().as_str(), "0" | "false" | "off" | "no"))
            .unwrap_or(true);
    if tips_allowed {
        let cfg = utils::config::load().ok();
        let tips_enabled = cfg.and_then(|c| c.telemetry_enabled).unwrap_or(false);
        if tips_enabled {
            let history_path = utils::config::config_dir();
            if let Ok(history_entries) = utils::history::load_history(&history_path) {
                if let Some(tip) =
                    utils::context_help::proactive_tip(&command_name, &history_entries)
                {
                    eprintln!("{}", tip);
                }
            }
        }
    }
}

/// Returns command-specific recovery hints for the error sink.
///
/// Hints are chosen based on the command that failed and the error message text
/// so users get actionable next steps instead of a raw error dump.
fn recovery_hints(command: &str, err: &anyhow::Error) -> Vec<String> {
    let msg = err.to_string().to_lowercase();
    let mut hints: Vec<String> = Vec::new();

    match command {
        "ai" => {
            if msg.contains("not running") || msg.contains("ollama") {
                hints.push("Install Ollama from https://ollama.ai/download".into());
                hints.push("Start the daemon: ollama serve".into());
                hints.push("Pull a model: starforge ai local pull codellama:7b".into());
            } else if msg.contains("model") || msg.contains("not found") {
                hints.push("List available models: starforge ai local models".into());
                hints.push("Download a model: starforge ai local pull codellama:7b".into());
            } else if msg.contains("wasm") || msg.contains("profile") {
                hints.push("Build your contract first: stellar contract build".into());
                hints.push(
                    "Pass the compiled WASM: starforge ai profiling run <path/to/contract.wasm>"
                        .into(),
                );
                hints.push(
                    "Save a baseline first: starforge ai profiling run <wasm> --output baseline.json"
                        .into(),
                );
            } else if msg.contains("analyse")
                || msg.contains("analyze")
                || msg.contains("root cause")
            {
                hints.push(
                    "Provide the full error message in quotes: starforge ai debug analyse \"<error>\""
                        .into(),
                );
                hints
                    .push("Explain a known error category: starforge ai debug explain auth".into());
            } else if msg.contains("source") || msg.contains("src/lib.rs") {
                hints.push("Point the command at a contract file, e.g. src/lib.rs.".into());
                hints.push(
                    "Discover properties: starforge ai property-test discover src/lib.rs".into(),
                );
            } else if msg.contains("search") || msg.contains("pattern") {
                hints.push("Search code: starforge ai search search \"token transfer\"".into());
                hints.push("Discover patterns: starforge ai search patterns".into());
            }
        }
        "wallet" => {
            if msg.contains("not found") || msg.contains("no wallet") {
                hints.push("Create a wallet first: starforge wallet create <name>".into());
                hints.push("List existing wallets: starforge wallet list".into());
            } else if msg.contains("password") || msg.contains("decrypt") {
                hints.push("Re-enter the password you used when creating the wallet.".into());
                hints.push("If you forgot it, remove the wallet and create a new one: starforge wallet remove <name>".into());
            } else if msg.contains("fund") || msg.contains("friendbot") {
                hints.push("Fund a testnet wallet: starforge wallet fund <name>".into());
                hints.push("Friendbot is only available on testnet — switch networks: starforge network switch testnet".into());
            } else if msg.contains("already exists") {
                hints.push("Use a different wallet name, or remove the existing one first.".into());
                hints.push("List wallets: starforge wallet list".into());
            }
        }
        "deploy" => {
            if msg.contains("smoke tests failed") {
                hints.push("The contract is deployed; only the post-deploy checks failed.".into());
                hints.push(
                    "Fix the contract or the [[smoke_tests]] entry in starforge-project.toml."
                        .into(),
                );
                hints.push("Deploy without running smoke tests: --skip-smoke".into());
            } else if msg.contains("wasm")
                || msg.contains("not found")
                || msg.contains("no such file")
            {
                hints.push("Build your contract first: stellar contract build".into());
                hints.push("Make sure you pass the correct --wasm path to deploy.".into());
            } else if msg.contains("account") || msg.contains("not found on") {
                hints.push(
                    "Fund your account before deploying: starforge wallet fund <name>".into(),
                );
                hints.push("Check the active network: starforge network show".into());
            } else if msg.contains("network") {
                hints.push("Check available networks: starforge network show".into());
                hints.push(
                    "Switch to testnet for free deployments: starforge network switch testnet"
                        .into(),
                );
            }
        }
        "contract" => {
            if msg.contains("no wallet") || msg.contains("wallet not found") {
                hints.push("Create a wallet first: starforge wallet create deployer --fund".into());
            } else if msg.contains("contract id") || msg.contains("invalid contract") {
                hints
                    .push("Contract IDs start with 'C' and are exactly 56 characters long.".into());
                hints.push(
                    "Find your contract ID in the deploy output or: starforge contract list".into(),
                );
            } else if msg.contains("invoke") || msg.contains("simulate") {
                hints.push(
                    "Run `stellar contract build` to ensure the contract is up to date.".into(),
                );
                hints.push("Check function name and argument types match the contract ABI.".into());
            } else if msg.contains("wasm") || msg.contains("no such file") {
                // `contract test`, `contract benchmark`, `contract lint` and
                // `contract profile` all take a compiled artifact.
                hints.push("Build your contract first: stellar contract build".into());
                hints.push("Pass the correct --wasm path to the command.".into());
            }
        }
        "network" => {
            if msg.contains("unsupported") || msg.contains("not found") {
                hints.push("List configured networks: starforge network show".into());
                hints.push(
                    "Add a custom network: starforge network add <name> --horizon <url>".into(),
                );
                hints.push("Valid built-in networks: testnet, mainnet, docker-testnet".into());
            } else if msg.contains("docker") {
                // `network node` runs the Docker devnet.
                hints.push(
                    "Install Docker Desktop from https://www.docker.com/products/docker-desktop"
                        .into(),
                );
                hints.push("Ensure the Docker daemon is running before retrying.".into());
            }
        }
        "config" => {
            if msg.contains("parse") || msg.contains("toml") || msg.contains("json") {
                hints.push("Your config file may be corrupted. Inspect it at: ~/.config/starforge/config.toml".into());
                hints
                    .push("Run `starforge config doctor` to diagnose configuration issues.".into());
            }
        }
        "plugin" => {
            if msg.contains("not found") || msg.contains("load") {
                hints.push(
                    "Re-install the plugin: starforge plugin install <name> --path <lib>".into(),
                );
                hints.push("List installed plugins: starforge plugin list".into());
            } else if msg.contains("untrusted") || msg.contains("trust") {
                hints.push(
                    "Review the plugin source and mark it trusted: starforge plugin trust <name>"
                        .into(),
                );
            }
        }
        "template" => {
            if msg.contains("not found") || msg.contains("fetch") {
                hints.push("List available templates: starforge template search".into());
                hints.push("Check your internet connection and retry.".into());
            }
        }
        _ => {}
    }

    // Generic fallbacks always appended when nothing command-specific matched
    if hints.is_empty() {
        if msg.contains("permission denied") || msg.contains("access denied") {
            hints.push("Check file and directory permissions.".into());
        } else if msg.contains("connection") || msg.contains("network") || msg.contains("timeout") {
            hints.push("Check your internet connection and try again.".into());
            hints.push("If behind a proxy, set the HTTPS_PROXY environment variable.".into());
        } else if msg.contains("config") {
            hints.push("Run `starforge config doctor` to diagnose configuration issues.".into());
        }
        // If still nothing, the cli_error fn will print the generic fallback.
    }

    hints
}

fn handle_external_plugin(args: Vec<String>) -> anyhow::Result<()> {
    use anyhow::Context;
    use plugins::registry::TrustLevel;

    if args.is_empty() {
        anyhow::bail!("No plugin command provided");
    }

    let plugin_name = &args[0];
    let plugin_args = &args[1..];

    let _cfg = starforge::utils::config::load()?;
    let reg = plugins::registry::load_registry().unwrap_or_default();
    if reg.plugins.is_empty() {
        anyhow::bail!(
            "Unknown command '{}'. No plugins installed.\n\nTry: starforge plugin install <name> --path <lib>",
            plugin_name
        );
    }

    // Check if the command matches any registered plugin command before loading .so files.
    let all_commands = plugins::registry::load_all_registered_commands();
    let known = all_commands.iter().any(|c| c.name == *plugin_name);
    if !known {
        let available: Vec<String> = all_commands
            .iter()
            .map(|c| format!("  • {}", c.name))
            .collect();
        let hint = if available.is_empty() {
            "No plugin commands registered. Re-install plugins to discover their commands."
                .to_string()
        } else {
            format!("Available plugin commands:\n{}", available.join("\n"))
        };
        anyhow::bail!("Unknown command '{}'.\n\n{}", plugin_name, hint);
    }

    // Warn about unknown-trust plugins before loading.
    for pl in reg.plugins.iter().filter(|p| {
        plugins::registry::classify_source(&p.source) == TrustLevel::Unknown && !p.source.is_empty()
    }) {
        eprintln!(
            "  ⚠  Warning: plugin '{}' is from an untrusted source: {}",
            pl.name, pl.source
        );
    }

    let mut pm = plugins::PluginManager::new();
    for pl in &reg.plugins {
        unsafe {
            pm.load_plugin(&pl.path)
                .with_context(|| format!("Failed to load plugin '{}' from {}", pl.name, pl.path))?;
        }
    }

    pm.execute(plugin_name, plugin_args)
        .map_err(|e| anyhow::anyhow!(e))
}

fn print_banner() {
    println!(
        "{}",
        "\n  ███████╗████████╗ █████╗ ██████╗ ███████╗ ██████╗ ██████╗  ██████╗ ███████╗\n  ██╔════╝╚══██╔══╝██╔══██╗██╔══██╗██╔════╝██╔═══██╗██╔══██╗██╔════╝ ██╔════╝\n  ███████╗   ██║   ███████║██████╔╝█████╗  ██║   ██║██████╔╝██║  ███╗█████╗  \n  ╚════██║   ██║   ██╔══██║██╔══██╗██╔══╝  ██║   ██║██╔══██╗██║   ██║██╔══╝  \n  ███████║   ██║   ██║  ██║██║  ██║██║     ╚██████╔╝██║  ██║╚██████╔╝███████╗\n  ╚══════╝   ╚═╝   ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝      ╚═════╝ ╚═╝  ╚═╝ ╚═════╝ ╚══════╝\n"
        .cyan().bold()
    );
    println!(
        "  {} {}\n",
        "⚡ Stellar & Soroban Developer CLI".bright_white(),
        "v0.1.0".dimmed()
    );
}
