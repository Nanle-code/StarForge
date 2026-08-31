#![allow(dead_code, unused, clippy::all)]

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

    /// Suppress the ASCII banner and decorative output
    #[arg(long, short = 'q', global = true)]
    quiet: bool,

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
    #[arg(long, global = true)]
    allow_network_passphrase_mismatch: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// AI-powered contract debugging assistant (error analysis, bug identification, fix suggestions)
    #[command(subcommand)]
    AiDebug(commands::ai_debug::AiDebugCommands),

    /// AI-driven definitions, references, code graphs, dependencies, and contextual search
    #[command(subcommand)]
    AiNavigate(commands::ai_navigate::AiNavigateCommands),

    /// Configurable code quality, security, performance, coverage, docs, and license gates
    #[command(subcommand)]
    AiQualityGate(commands::ai_quality_gate::AiQualityGateCommands),

    /// Local LLM assistant for Soroban contracts (audit, explain, test, optimise, profile)
    #[command(subcommand)]
    Ai(commands::ai::AiCommands),

    /// AI-driven performance profiling commands
    #[command(subcommand, name = "ai-profile")]
    AiProfile(commands::ai_profile::AiProfileCommands),

    /// AI-powered IDE integration commands
    #[command(subcommand, name = "ai-ide")]
    AiIde(commands::ai_ide::AiIdeCommands),

    /// AI-driven test maintenance commands
    #[command(subcommand, name = "ai-test-maintain")]
    AiTestMaintain(commands::ai_test_maintain::AiTestMaintainCommands),

    /// AI-driven deployment testing commands
    #[command(subcommand, name = "ai-deployment-test")]
    AiDeploymentTest(commands::ai_deployment_test::AiDeploymentTestCommands),

    /// Manage test wallets (create, list, fund, show, remove)
    #[command(subcommand)]
    Wallet(commands::wallet::WalletCommands),
    /// Natural language command interface
    Nl(commands::nl::NlArgs),

    /// Generate Soroban project boilerplate
    #[command(subcommand)]
    New(commands::new::NewCommands),

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
    /// Deployment history, rollback, verification, and dashboard
    #[command(subcommand)]
    Deployments(commands::deployments::DeploymentsCommands),
    /// Generate CI/CD configuration templates (GitHub Actions, GitLab CI, Jenkins)
    #[command(subcommand)]
    Cicd(commands::cicd::CicdCommands),
    /// Show starforge config and environment info
    Info,
    /// Manage AI prompt templates and versioning
    #[command(subcommand)]
    Prompts(commands::prompts::PromptsCommands),
    /// Analyze and explain smart contract code using AI
    #[command(subcommand)]
    Explain(commands::explain::ExplainCommands),
    /// Manage starforge configuration (telemetry, network)
    #[command(subcommand)]
    Config(commands::config::ConfigCommands),

    /// Manage telemetry settings directly
    #[command(subcommand)]
    Telemetry(commands::telemetry::TelemetryCommands),

    Tx(commands::tx::TxArgs), // fetch transaction for the account

    /// View or switch the active network (testnet/mainnet)
    #[command(subcommand)]
    Network(commands::network::NetworkCommands),
    /// Local Soroban devnet (Docker quickstart)
    #[command(subcommand)]
    Node(commands::node::NodeCommands),
    /// Generate shell completions for bash, zsh, and fish
    #[command(subcommand)]
    Completions(commands::completions::CompletionShell),

    /// Smart autocomplete — suggest and record commands
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

    /// Interactive REPL for local Soroban contract testing
    Shell(commands::shell::ShellArgs),

    /// Live monitoring (contract events or wallet threshold)
    Monitor(commands::monitor::MonitorArgs),

    /// Interactive CLI tutorials
    #[command(subcommand)]
    Tutorial(commands::tutorial::TutorialCommands),

    /// Performance benchmarking utilities and industry-standard comparisons
    #[command(subcommand)]
    Benchmark(commands::benchmark::BenchmarkCommands),

    /// Contract testing utilities for Soroban wasm
    Test(commands::test::TestArgs),

    /// Gas analysis and optimization helpers
    #[command(subcommand)]
    Gas(commands::gas::GasCommands),

    /// AI-assisted deployment cost management: budgets, forecasting,
    /// cross-network comparison, and reporting
    #[command(subcommand)]
    Cost(commands::cost::CostCommands),

    /// Manage third-party plugins
    #[command(subcommand)]
    Plugin(commands::plugin::PluginCommands),

    /// Check PR readiness (CI status and merge conflicts)
    #[command(subcommand)]
    Pr(commands::pr::PrCommands),

    /// AI mutation testing for Soroban contracts
    #[command(subcommand)]
    Mutate(commands::mutate::MutateCommands),
    /// Privacy protection, anonymization, consent, and reporting
    #[command(subcommand)]
    Privacy(commands::privacy::PrivacyCommands),
    /// AI-driven project management for task tracking, sprints, resources, risks, and timelines
    #[command(subcommand)]
    Project(commands::project::ProjectCommands),
    /// Manage community contract templates from the marketplace
    #[command(subcommand)]
    Template(commands::template::TemplateCommands),

    /// Interact with the remote template registry
    #[command(subcommand)]
    Registry(commands::registry::RegistryCommands),

    /// Manage multi-signature transactions
    #[command(subcommand)]
    Multisig(commands::multisig_builder::MultisigCommands),

    /// Contract upgrade management (propose, approve, execute, rollback)
    #[command(subcommand)]
    Upgrade(commands::upgrade::UpgradeCommands),

    /// Contract upgrade governance (proposals, voting, timelock, audit)
    #[command(subcommand)]
    Governance(commands::governance::GovernanceCommands),

    /// Multi-contract deployment orchestration
    #[command(subcommand)]
    Orchestrate(commands::orchestrate::OrchestrateCommands),

    /// Visual pipeline builder for contract deployment workflows
    #[command(subcommand)]
    Pipeline(commands::pipeline_builder::PipelineCommands),

    /// Security hardening, validation, and monitoring
    #[command(subcommand)]
    Security(commands::security::SecurityCommands),

    /// Run a comprehensive security audit on a Soroban contract
    Audit(commands::audit::AuditArgs),

    /// AI-powered security audit for Soroban contracts using Claude
    AiAudit(commands::ai_audit::AiAuditArgs),

    /// AI-driven testing assistance (generate, optimize, analyze, maintain tests)
    #[command(subcommand)]
    AiTest(commands::ai_test::AiTestCommands),

    /// AI property-based testing (discover properties, generate tests, validate invariants)
    #[command(subcommand)]
    AiPropertyTest(commands::ai_property_test::AiPropertyTestCommands),

    /// AI feedback and learning system (record feedback, track quality, learn preferences)
    #[command(subcommand)]
    AiFeedback(commands::ai_feedback::AiFeedbackCommands),

    /// AI code search and discovery (search code, find patterns, similar code)
    #[command(subcommand)]
    AiSearch(commands::ai_search::AiSearchCommands),

    /// AI best practice recommendations (analyze contracts, scan projects, improvement plans)
    #[command(subcommand)]
    AiRecommend(commands::ai_recommend::AiRecommendCommands),

    /// Intelligent AI model selection and routing based on task complexity and preferences
    #[command(subcommand, name = "ai-route")]
    AiRoute(commands::ai_model_router::AiModelRouterCommands),

    /// AI project planning assistant — requirements, architecture, timeline, risks
    #[command(subcommand, name = "ai-plan")]
    AiPlan(commands::ai_plan::AiPlanCommands),

    /// AI accessibility features — screen reader, voice commands, text simplification
    #[command(subcommand, name = "ai-accessibility")]
    AiAccessibility(commands::ai_accessibility::AiAccessibilityCommands),

    /// AI contract function suggestions (context-aware function suggestions based on contract type)
    #[command(subcommand)]
    AiContractSuggest(commands::ai_contract_suggest::AiContractSuggestCommands),

    /// AI documentation Q&A (answer questions about StarForge, Stellar, and Soroban docs with citations)
    #[command(subcommand)]
    AiDocQa(commands::ai_doc_qa::AiDocQaCommands),

    /// Schedule deployments for future execution with approval workflows
    #[command(subcommand)]
    Schedule(commands::schedule::ScheduleCommands),

    /// Local network simulation and testing environment
    #[command(subcommand)]
    Simulate(commands::simulate::SimulateCommands),

    /// Backup and disaster recovery for contract state and code
    #[command(subcommand)]
    Backup(commands::backup::BackupCommands),

    /// Static analysis and linting for Soroban contracts
    Lint(commands::lint::LintArgs),

    /// Generate or install man pages
    #[command(subcommand)]
    Man(commands::man::ManCommand),

    /// Run connectivity diagnostics for attached Ledger/Trezor devices
    Diagnostics(commands::diagnostics::DiagnosticsArgs),

    /// Template version control (versioning, branching, changelog)
    #[command(subcommand)]
    TemplateVcs(commands::template_vcs::TemplateVcsCommands),

    /// Contract performance monitoring and metrics dashboard
    #[command(subcommand)]
    Perf(commands::perf::PerfCommands),

    /// Advanced contract performance analysis and profiling tools
    #[command(subcommand)]
    AdvancedPerf(commands::perf::AdvancedPerfCommands),

    /// Contract documentation portal (generate, view, search)
    #[command(subcommand)]
    Docs(commands::docs::DocsCommands),

    /// Contract deployment analytics, dashboards, and reporting
    #[command(subcommand)]
    Analytics(commands::analytics::AnalyticsCommands),

    /// Approval workflow for contract deployments (multi-level approvals, audit, compliance)
    #[command(subcommand)]
    Approval(commands::approval::ApprovalCommands),

    /// Manage feature flags for AI features (rollouts, A/B tests, rollback)
    FeatureFlags(commands::feature_flags_cmd::FeatureFlagsArgs),

    /// Contract storage migration tools (transform, validate, rollback)
    #[command(subcommand)]
    Migrate(commands::migrate::MigrateCommands),
    /// AI-driven collaboration tools: code review, conflict resolution, knowledge sharing, contribution tracking
    #[command(subcommand)]
    Collab(commands::collab::CollabCommands),

    /// Run formal verification on a contract
    #[command(subcommand)]
    Verify(commands::verify::VerifyCommands),
    /// AI Contextual Help: command, workflow, error, and best-practice guidance
    Help(commands::help::HelpArgs),

    /// AI usage telemetry and analytics: calls, tokens, latency, cost, opt-out
    #[command(subcommand)]
    AiTelemetry(commands::ai_telemetry::AiTelemetryCommands),

    /// Analyse and optimize compiled WASM / Rust contract source for gas and size
    #[command(subcommand)]
    Optimize(commands::optimize::OptimizeCommands),

    /// AI-driven security training: lessons, exercises, progress tracking
    #[command(subcommand)]
    AiSecurityTraining(commands::ai_security_training::AiSecurityTrainingCommands),

    /// Contract health monitoring, performance tracking, security events, alerting, and dashboard
    #[command(subcommand)]
    ContractMonitor(commands::contract_monitor::ContractMonitorCommands),
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
    OUTPUT_MODE_INIT.call_once(|| {});
    utils::output::set_json_mode(cli.json);
    utils::interactive::set_non_interactive(cli.non_interactive);
    utils::network_guard::set_allow_mismatch(cli.allow_network_passphrase_mismatch);

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

    if !cli.quiet {
        print_banner();
    }

    let command_name = match &cli.command {
        Commands::AiDebug(_) => "ai-debug",
        Commands::AiNavigate(_) => "ai-navigate",
        Commands::AiQualityGate(_) => "ai-quality-gate",
        Commands::Ai(_) => "ai",
        Commands::AiProfile(_) => "ai-profile",
        Commands::AiIde(_) => "ai-ide",
        Commands::AiTestMaintain(_) => "ai-test-maintain",
        Commands::AiDeploymentTest(_) => "ai-deployment-test",
        Commands::Wallet(_) => "wallet",
        Commands::Nl(_) => "nl",
        Commands::New(_) => "new",
        Commands::Generate(_) => "generate",
        Commands::Contract(_) => "contract",
        Commands::Complete(_) => "complete",
        Commands::FeatureFlags(_) => "feature-flags",
        Commands::Debug(_) => "debug",
        Commands::Inspect(_) => "inspect",
        Commands::Deploy(_) => "deploy",
        Commands::Deployments(_) => "deployments",
        Commands::Cicd(_) => "cicd",
        Commands::Info => "info",
        Commands::Prompts(_) => "prompts",
        Commands::Explain(_) => "explain",
        Commands::Config(_) => "config",
        Commands::Telemetry(_) => "telemetry",
        Commands::Tx(_) => "tx",
        Commands::Network(_) => "network",
        Commands::Node(_) => "node",
        Commands::Completions(_) => "completions",
        Commands::Autocomplete { .. } => "autocomplete",
        Commands::Shell(_) => "shell",
        Commands::Monitor(_) => "monitor",
        Commands::Multisig(_) => "multisig",
        Commands::Tutorial(_) => "tutorial",
        Commands::Benchmark(_) => "benchmark",
        Commands::Test(_) => "test",
        Commands::Gas(_) => "gas",
        Commands::Cost(_) => "cost",
        Commands::Plugin(_) => "plugin",
        Commands::Pr(_) => "pr",
        Commands::Mutate(_) => "mutate",
        Commands::Privacy(_) => "privacy",
        Commands::Project(_) => "project",
        Commands::Template(_) => "template",
        Commands::Registry(_) => "registry",
        Commands::Upgrade(_) => "upgrade",
        Commands::Governance(_) => "governance",
        Commands::Orchestrate(_) => "orchestrate",
        Commands::Pipeline(_) => "pipeline",
        Commands::Security(_) => "security",
        Commands::Audit(_) => "audit",
        Commands::AiAudit(_) => "ai-audit",
        Commands::AiTest(_) => "ai-test",
        Commands::AiPropertyTest(_) => "ai-property-test",
        Commands::AiFeedback(_) => "ai-feedback",
        Commands::AiSearch(_) => "ai-search",
        Commands::AiRecommend(_) => "ai-recommend",
        Commands::AiRoute(_) => "ai-route",
        Commands::AiPlan(_) => "ai-plan",
        Commands::AiAccessibility(_) => "ai-accessibility",
        Commands::AiContractSuggest(_) => "ai-contract-suggest",
        Commands::AiDocQa(_) => "ai-doc-qa",
        Commands::Schedule(_) => "schedule",
        Commands::Simulate(_) => "simulate",
        Commands::Backup(_) => "backup",
        Commands::Lint(_) => "lint",
        Commands::Man(_) => "man",
        Commands::Diagnostics(_) => "diagnostics",
        Commands::TemplateVcs(_) => "template-vcs",
        Commands::Perf(_) => "perf",
        Commands::AdvancedPerf(_) => "advanced-perf",
        Commands::Docs(_) => "docs",
        Commands::Analytics(_) => "analytics",
        Commands::Approval(_) => "approval",
        Commands::Migrate(_) => "migrate",
        Commands::Collab(_) => "collab",
        Commands::External(_) => "external",
        Commands::Verify(_) => "verify",
        Commands::Help(_) => "help",
        Commands::AiTelemetry(_) => "ai-telemetry",
        Commands::Optimize(_) => "optimize",
        Commands::AiSecurityTraining(_) => "ai-security-training",
        Commands::ContractMonitor(_) => "contract-monitor",
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
        Commands::AiDebug(cmd) => commands::ai_debug::handle(cmd).await,
        Commands::AiNavigate(cmd) => commands::ai_navigate::handle(cmd),
        Commands::AiQualityGate(cmd) => commands::ai_quality_gate::handle(cmd),
        Commands::Ai(cmd) => commands::ai::handle(cmd).await,
        Commands::AiProfile(cmd) => commands::ai_profile::handle(cmd).await,
        Commands::AiIde(cmd) => commands::ai_ide::handle(cmd).await,
        Commands::AiTestMaintain(cmd) => commands::ai_test_maintain::handle(cmd).await,
        Commands::AiDeploymentTest(cmd) => commands::ai_deployment_test::handle(cmd).await,
        Commands::Wallet(cmd) => commands::wallet::handle(cmd).await,
        Commands::Nl(args) => commands::nl::handle(args).await,
        Commands::New(cmd) => commands::new::handle(cmd).await,
        Commands::Generate(cmd) => commands::generate::handle(&cmd).await,
        Commands::Contract(cmd) => commands::contract::handle(cmd).await,
        Commands::Inspect(cmd) => commands::inspect::handle(cmd).await,
        Commands::Debug(cmd) => commands::debug::handle(cmd).await,
        Commands::Deploy(args) => commands::deploy::handle(args).await,
        Commands::Deployments(cmd) => commands::deployments::handle(cmd).await,
        Commands::Cicd(cmd) => commands::cicd::handle(cmd),
        Commands::Info => commands::info::handle().await,
        Commands::Prompts(cmd) => commands::prompts::handle(&cmd).await,
        Commands::Explain(ref cmd) => commands::explain::handle(cmd).await,
        Commands::Config(cmd) => commands::config::handle(cmd).await,
        Commands::Telemetry(cmd) => commands::telemetry::handle(cmd).await,
        Commands::Tx(args) => commands::tx::handle(args).await,
        Commands::Network(cmd) => commands::network::handle(cmd).await,
        Commands::Node(cmd) => commands::node::handle(cmd).await,
        Commands::Completions(shell) => commands::completions::handle(shell).await,
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
        Commands::Shell(args) => commands::shell::handle(args).await,
        Commands::Monitor(args) => commands::monitor::handle(args).await,
        Commands::Multisig(cmd) => commands::multisig_builder::handle(cmd).await,
        Commands::Tutorial(cmd) => commands::tutorial::handle(cmd).await,
        Commands::Benchmark(args) => commands::benchmark::handle(args).await,
        Commands::Test(args) => commands::test::handle(args).await,
        Commands::Gas(args) => commands::gas::handle(args).await,
        Commands::Plugin(args) => commands::plugin::handle(args).await,
        Commands::Pr(cmd) => commands::pr::handle(cmd).await,
        Commands::Mutate(cmd) => commands::mutate::handle(cmd).await,
        Commands::Privacy(cmd) => commands::privacy::handle(cmd).await,
        Commands::Template(args) => commands::template::handle(args).await,
        Commands::Registry(cmd) => commands::registry::handle(cmd).await,
        Commands::Upgrade(cmd) => commands::upgrade::handle(cmd).await,
        Commands::Governance(cmd) => commands::governance::handle(cmd).await,
        Commands::Orchestrate(cmd) => commands::orchestrate::handle(cmd).await,
        Commands::Pipeline(cmd) => commands::pipeline_builder::handle(cmd).await,
        Commands::Security(cmd) => commands::security::handle(cmd).await,
        Commands::Audit(args) => commands::audit::handle(args).await,
        Commands::AiAudit(args) => commands::ai_audit::handle(args).await,
        Commands::AiTest(cmd) => commands::ai_test::handle(cmd).await,
        Commands::AiPropertyTest(cmd) => commands::ai_property_test::handle(cmd).await,
        Commands::AiFeedback(cmd) => commands::ai_feedback::handle(cmd).await,
        Commands::AiSearch(cmd) => commands::ai_search::handle(cmd).await,
        Commands::AiRecommend(cmd) => commands::ai_recommend::handle(cmd).await,
        Commands::AiRoute(cmd) => commands::ai_model_router::handle(cmd).await,
        Commands::AiPlan(cmd) => commands::ai_plan::handle(cmd).await,
        Commands::AiAccessibility(cmd) => commands::ai_accessibility::handle(cmd).await,
        Commands::AiContractSuggest(cmd) => commands::ai_contract_suggest::handle(cmd).await,
        Commands::AiDocQa(cmd) => commands::ai_doc_qa::handle(cmd).await,
        Commands::Schedule(cmd) => commands::schedule::handle(cmd).await,
        Commands::Simulate(cmd) => commands::simulate::handle(cmd).await,
        Commands::Backup(cmd) => commands::backup::handle(cmd).await,
        Commands::Lint(args) => commands::lint::handle(args).await,
        Commands::Man(cmd) => commands::man::handle(cmd).await,
        Commands::Diagnostics(args) => commands::diagnostics::handle(args),
        Commands::TemplateVcs(cmd) => commands::template_vcs::handle(cmd).await,
        Commands::Perf(cmd) => commands::perf::handle(cmd).await,
        Commands::AdvancedPerf(cmd) => commands::perf::handle_advanced(cmd).await,
        Commands::Docs(cmd) => commands::docs::handle(cmd).await,
        Commands::Analytics(cmd) => commands::analytics::handle(cmd).await,
        Commands::Approval(cmd) => commands::approval::handle(cmd).await,
        Commands::Migrate(cmd) => commands::migrate::handle(cmd),
        Commands::Collab(cmd) => commands::collab::handle(cmd).await,
        Commands::Complete(cmd) => commands::complete::handle(cmd).await,
        Commands::Verify(cmd) => commands::verify::handle(cmd).await,
        Commands::Cost(cmd) => commands::cost::handle(cmd).await,
        Commands::Project(cmd) => commands::project::handle(cmd).await,
        Commands::FeatureFlags(args) => commands::feature_flags_cmd::handle(args).await,
        Commands::External(args) => handle_external_plugin(args),
        Commands::Help(args) => commands::help::handle(args).await,
        Commands::AiTelemetry(cmd) => commands::ai_telemetry::handle(cmd).await,
        Commands::Optimize(cmd) => commands::optimize::handle(cmd).await,
        Commands::AiSecurityTraining(cmd) => commands::ai_security_training::handle(cmd).await,
        Commands::ContractMonitor(cmd) => commands::contract_monitor::handle(cmd).await,
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
        let mut hints = recovery_hints(&command_name, &e);
        // Augment the static command-specific hints with the AI Contextual
        // Help engine. Patterns that did not match the static rule table
        // still produce a useful, command-agnostic one-liner.
        utils::context_help::troubleshoot_merging(&e.to_string(), &mut hints);
        utils::print::cli_error(&e, &hints.iter().map(String::as_str).collect::<Vec<_>>());
        let code = utils::exit_codes::determine_exit_code(&e);
        code.exit();
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
        let tips_enabled = cfg.and_then(|c| c.telemetry_enabled).unwrap_or(true);
        if tips_enabled {
            let history_path = utils::config::config_dir();
            if let Ok(history_entries) = utils::history::load_history(&history_path) {
                if let Some(tip) =
                    utils::context_help::proactive_tip(&command_name, &history_entries)
                {
                    utils::print::info(&tip);
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
                hints.push("Pull a model: starforge ai pull codellama:7b".into());
            } else if msg.contains("model") || msg.contains("not found") {
                hints.push("List available models: starforge ai models".into());
                hints.push("Download a model: starforge ai pull codellama:7b".into());
            } else if msg.contains("wasm") || msg.contains("profile") {
                hints.push("Build your contract first: stellar contract build".into());
                hints.push(
                    "Pass the compiled WASM: starforge ai profile <path/to/contract.wasm>".into(),
                );
                hints.push(
                    "Save a baseline first: starforge ai profile <wasm> --output baseline.json"
                        .into(),
                );
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
            if msg.contains("wasm") || msg.contains("not found") || msg.contains("no such file") {
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
            }
        }
        "tx" => {
            if msg.contains("account not found") || msg.contains("not active") {
                hints.push("Fund your account first: starforge wallet fund <name>".into());
                hints.push("Verify you are on the right network: starforge network show".into());
            } else if msg.contains("insufficient") {
                hints.push("Check your XLM balance: starforge wallet show <name>".into());
                hints.push("Fund the account: starforge wallet fund <name>".into());
            } else if msg.contains("asset") {
                hints.push(
                    "Asset format is CODE:ISSUER (e.g. USDC:GA5ZS...) or XLM for native.".into(),
                );
            }
        }
        "network" => {
            if msg.contains("unsupported") || msg.contains("not found") {
                hints.push("List configured networks: starforge network show".into());
                hints.push(
                    "Add a custom network: starforge network add <name> --horizon <url>".into(),
                );
                hints.push("Valid built-in networks: testnet, mainnet, docker-testnet".into());
            }
        }
        "node" => {
            if msg.contains("docker") || msg.contains("not found") || msg.contains("command") {
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
        "ai-debug" => {
            hints.push(
                "Provide the full error message in quotes: starforge ai-debug analyse \"<error>\""
                    .into(),
            );
            hints.push("Explain a specific category: starforge ai-debug explain auth".into());
            hints.push("Available categories: auth, arithmetic, storage, token, panic, wasm, network, deployment, rollback, security, analytics, ttl, test, type".into());
        }
        "ai-test" => {
            if msg.contains("not found") || msg.contains("no such file") {
                hints.push("Ensure the source file exists: ls src/lib.rs".into());
                hints.push("Build your contract first: stellar contract build".into());
            } else if msg.contains("ollama") || msg.contains("not running") {
                hints.push("Install Ollama: https://ollama.ai/download".into());
                hints.push("Start Ollama: ollama serve".into());
                hints.push("Or run without --use-ai for local generation".into());
            } else if msg.contains("coverage") {
                hints.push(
                    "Generate coverage first: starforge test --coverage --source src/lib.rs".into(),
                );
            }
        }
        "ai-property-test" => {
            hints.push(
                "Provide a contract source file: starforge ai-property-test discover src/lib.rs"
                    .into(),
            );
            hints.push("Run without --use-ai for local property discovery".into());
        }
        "ai-feedback" => {
            hints.push("Record feedback: starforge ai-feedback record <feature> --prompt-summary \"...\" --response-summary \"...\" --rating positive".into());
            hints.push("View stats: starforge ai-feedback stats".into());
        }
        "ai-search" => {
            hints.push("Search code: starforge ai-search search \"token transfer\"".into());
            hints.push("Discover patterns: starforge ai-search patterns".into());
        }
        "ai-recommend" => {
            hints.push("Analyze a contract: starforge ai-recommend analyze src/lib.rs".into());
            hints.push("Scan a project: starforge ai-recommend scan .".into());
        }
        "benchmark" | "test" => {
            if msg.contains("wasm") || msg.contains("not found") {
                hints.push("Build your contract first: stellar contract build".into());
                hints.push("Pass the correct --wasm path to the command.".into());
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

    let cfg = starforge::utils::config::load()?;
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
