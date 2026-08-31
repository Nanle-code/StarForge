use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate_to, Shell};
use std::env;
use std::fs;
use std::path::Path;

#[derive(Parser)]
#[command(
    name = "starforge",
    version = "0.1.0",
    about = "⚡ Stellar & Soroban developer productivity CLI"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Top-level command model used to render the cheat sheet from clap metadata.
///
/// This enumerates every *top-level* subcommand so the generated
/// `docs/COMMAND_CHEATSHEET.md` is derived from actual clap metadata rather than
/// a hand-written list (preventing doc drift). Per-subcommand detail lives in
/// [`MAJOR_SUBCOMMANDS`]; keep the two in sync when commands change.
#[derive(Subcommand)]
enum Commands {
    #[command(
        about = "AI-powered contract debugging assistant (error analysis, bug identification, fix suggestions)"
    )]
    AiDebug,
    #[command(
        about = "AI-driven definitions, references, code graphs, dependencies, and contextual search"
    )]
    AiNavigate,
    #[command(
        about = "Configurable code quality, security, performance, coverage, docs, and license gates"
    )]
    AiQualityGate,
    #[command(
        about = "Local LLM assistant for Soroban contracts (audit, explain, test, optimise, profile)"
    )]
    Ai,
    #[command(about = "AI-driven performance profiling commands")]
    AiProfile,
    #[command(about = "AI-powered IDE integration commands")]
    AiIde,
    #[command(about = "AI-driven test maintenance commands")]
    AiTestMaintain,
    #[command(about = "AI-driven deployment testing commands")]
    AiDeploymentTest,
    #[command(about = "Manage test wallets (create, list, fund, show, remove)")]
    Wallet,
    #[command(about = "Natural language command interface")]
    Nl,
    #[command(about = "Generate Soroban project boilerplate")]
    New,
    #[command(about = "Contract operations (invoke, inspect, etc.)")]
    Contract,
    #[command(about = "Generate smart contracts from natural language prompts")]
    Generate,
    #[command(about = "Smart contract completion assistant")]
    Complete,
    #[command(about = "External plugins", hide = true)]
    External(Vec<String>),
    #[command(about = "Debug Soroban contracts with breakpoints, stepping, and inspection")]
    Debug,
    #[command(about = "Deep contract storage inspection (state, key, storage)")]
    Inspect,
    #[command(about = "Deploy a compiled Soroban contract (.wasm)")]
    Deploy,
    #[command(about = "Deployment history, rollback, verification, and dashboard")]
    Deployments,
    #[command(about = "Show starforge config and environment info")]
    Info,
    #[command(about = "Manage AI prompt templates and versioning")]
    Prompts,
    #[command(about = "Analyze and explain smart contract code using AI")]
    Explain,
    #[command(about = "Manage starforge configuration (telemetry, network)")]
    Config,
    #[command(about = "Manage telemetry settings directly")]
    Telemetry,
    #[command(about = "Fetch a transaction for the account")]
    Tx,
    #[command(about = "View or switch the active network (testnet/mainnet)")]
    Network,
    #[command(about = "Local Soroban devnet (Docker quickstart)")]
    Node,
    #[command(about = "Generate shell completions for bash, zsh, and fish")]
    Completions,
    #[command(
        about = "Smart autocomplete — suggest and record commands",
        hide = true
    )]
    Autocomplete,
    #[command(about = "Interactive REPL for local Soroban contract testing")]
    Shell,
    #[command(about = "Live monitoring (contract events or wallet threshold)")]
    Monitor,
    #[command(about = "Interactive CLI tutorials")]
    Tutorial,
    #[command(about = "Performance benchmarking utilities and industry-standard comparisons")]
    Benchmark,
    #[command(about = "Contract testing utilities for Soroban wasm")]
    Test,
    #[command(about = "Gas analysis and optimization helpers")]
    Gas,
    #[command(
        about = "AI-assisted deployment cost management: budgets, forecasting, cross-network comparison, and reporting"
    )]
    Cost,
    #[command(about = "Manage third-party plugins")]
    Plugin,
    #[command(about = "AI mutation testing for Soroban contracts")]
    Mutate,
    #[command(about = "Privacy protection, anonymization, consent, and reporting")]
    Privacy,
    #[command(
        about = "AI-driven project management for task tracking, sprints, resources, risks, and timelines"
    )]
    Project,
    #[command(about = "Manage community contract templates from the marketplace")]
    Template,
    #[command(about = "Interact with the remote template registry")]
    Registry,
    #[command(about = "Manage multi-signature transactions")]
    Multisig,
    #[command(about = "Contract upgrade management (propose, approve, execute, rollback)")]
    Upgrade,
    #[command(about = "Contract upgrade governance (proposals, voting, timelock, audit)")]
    Governance,
    #[command(about = "Multi-contract deployment orchestration")]
    Orchestrate,
    #[command(about = "Visual pipeline builder for contract deployment workflows")]
    Pipeline,
    #[command(about = "Security hardening, validation, and monitoring")]
    Security,
    #[command(about = "Run a comprehensive security audit on a Soroban contract")]
    Audit,
    #[command(about = "AI-powered security audit for Soroban contracts using Claude")]
    AiAudit,
    #[command(
        about = "AI-driven testing assistance (generate, optimize, analyze, maintain tests)"
    )]
    AiTest,
    #[command(
        about = "AI property-based testing (discover properties, generate tests, validate invariants)"
    )]
    AiPropertyTest,
    #[command(
        about = "AI feedback and learning system (record feedback, track quality, learn preferences)"
    )]
    AiFeedback,
    #[command(about = "AI code search and discovery (search code, find patterns, similar code)")]
    AiSearch,
    #[command(
        about = "AI best practice recommendations (analyze contracts, scan projects, improvement plans)"
    )]
    AiRecommend,
    #[command(
        about = "Intelligent AI model selection and routing based on task complexity and preferences"
    )]
    AiRoute,
    #[command(
        about = "AI project planning assistant — requirements, architecture, timeline, risks"
    )]
    AiPlan,
    #[command(
        about = "AI accessibility features — screen reader, voice commands, text simplification"
    )]
    AiAccessibility,
    #[command(
        about = "AI contract function suggestions (context-aware suggestions based on contract type)"
    )]
    AiContractSuggest,
    #[command(
        about = "AI documentation Q&A (answer questions about StarForge, Stellar, and Soroban docs with citations)"
    )]
    AiDocQa,
    #[command(about = "Schedule deployments for future execution with approval workflows")]
    Schedule,
    #[command(about = "Local network simulation and testing environment")]
    Simulate,
    #[command(about = "Backup and disaster recovery for contract state and code")]
    Backup,
    #[command(about = "Static analysis and linting for Soroban contracts")]
    Lint,
    #[command(about = "Generate or install man pages", hide = true)]
    Man,
    #[command(about = "Run connectivity diagnostics for attached Ledger/Trezor devices")]
    Diagnostics,
    #[command(about = "Template version control (versioning, branching, changelog)")]
    TemplateVcs,
    #[command(about = "Contract performance monitoring and metrics dashboard")]
    Perf,
    #[command(about = "Advanced contract performance analysis and profiling tools")]
    AdvancedPerf,
    #[command(about = "Contract documentation portal (generate, view, search)")]
    Docs,
    #[command(about = "Contract deployment analytics, dashboards, and reporting")]
    Analytics,
    #[command(
        about = "Approval workflow for contract deployments (multi-level approvals, audit, compliance)"
    )]
    Approval,
    #[command(
        about = "Manage feature flags for AI features (rollouts, A/B tests, rollback)",
        hide = true
    )]
    FeatureFlags,
    #[command(about = "Contract storage migration tools (transform, validate, rollback)")]
    Migrate,
    #[command(
        about = "AI-driven collaboration tools: code review, conflict resolution, knowledge sharing, contribution tracking"
    )]
    Collab,
    #[command(about = "Run formal verification on a contract")]
    Verify,
    #[command(
        about = "AI Contextual Help: command, workflow, error, and best-practice guidance",
        hide = true
    )]
    Help,
    #[command(about = "AI usage telemetry and analytics: calls, tokens, latency, cost, opt-out")]
    AiTelemetry,
    #[command(
        about = "Analyse and optimize compiled WASM / Rust contract source for gas and size"
    )]
    Optimize,
    #[command(about = "AI-driven security training: lessons, exercises, progress tracking")]
    AiSecurityTraining,
    #[command(
        about = "Contract health monitoring, performance tracking, security events, alerting, and dashboard"
    )]
    ContractMonitor,
}

/// Internal / developer-only top-level commands that are excluded from the
/// generated cheat sheet. Hidden commands (`#[command(hide)]`) are excluded
/// automatically by the renderer; this set covers the ones that are declared
/// but considered internal tooling rather than user-facing CLI surface.
///
/// Keep this list consistent: it is the single source of truth for "what does
/// not appear in the cheat sheet" and is also applied when the man-page names
/// are filtered below.
const INTERNAL_COMMANDS: &[&str] = &["external", "autocomplete", "man", "feature-flags", "help"];

/// Named subcommand detail for the "major subcommands" section of the cheat
/// sheet. The top-level command list is derived from clap metadata; this table
/// records the most important subcommands under each group so the cheat sheet
/// remains useful as a quick reference.
///
/// The renderer asserts that every parent listed here is a real top-level
/// command, so a rename/removal fails the build instead of silently drifting.
const MAJOR_SUBCOMMANDS: &[(&str, &[(&str, &str)])] = &[
    (
        "wallet",
        &[
            (
                "create <NAME>",
                "Create and store a keypair (--fund, --encrypt, --mnemonic)",
            ),
            ("list", "List saved wallets"),
            ("show <NAME>", "Show wallet metadata and balance (--reveal)"),
            ("fund <NAME>", "Fund via Friendbot when configured"),
            ("remove <NAME>", "Delete a saved wallet"),
            ("rename <OLD> <NEW>", "Rename a wallet entry"),
            ("merge", "Account merge (--from, --to, --yes)"),
            ("rotate <NAME>", "Rotate keys in place"),
            ("export <NAME>", "Export backup JSON"),
            ("import", "Import from file or --mnemonic"),
            ("sign", "Sign a payload with a saved wallet"),
            ("multisig", "Multisig helpers"),
        ],
    ),
    (
        "contract",
        &[
            ("invoke", "Invoke a deployed Soroban contract function"),
            (
                "invoke-script",
                "Run an ordered YAML or JSON invocation script (--dry-run)",
            ),
            ("inspect", "Inspect a deployed Soroban contract instance"),
            ("upload", "Upload a WASM binary to the Stellar network"),
            (
                "generate-bindings <WASM>",
                "Generate typed client bindings (--lang rust|ts|python|go)",
            ),
            (
                "call-graph",
                "Visualize cross-contract call graph from Soroban source",
            ),
            ("deps", "Manage contract dependencies"),
            (
                "version",
                "Track contract versions, resolve conflicts, migrations",
            ),
        ],
    ),
    (
        "deploy",
        &[(
            "deploy --wasm <FILE>",
            "Prepare a Soroban deployment (--simulate, --execute)",
        )],
    ),
    (
        "inspect",
        &[(
            "inspect storage",
            "Deep storage inspection (state, key, storage)",
        )],
    ),
    (
        "network",
        &[
            ("show", "Show current active network"),
            (
                "switch <NAME>",
                "Switch the active network (testnet, mainnet, custom)",
            ),
            ("add", "Add a custom network endpoint"),
            ("test", "Test connectivity to a network"),
        ],
    ),
    (
        "config",
        &[
            ("show", "Show current global configuration"),
            ("set <KEY> <VALUE>", "Set a configuration key/value pair"),
            (
                "set-encryption",
                "Set global wallet encryption parameters (Argon2id)",
            ),
            (
                "doctor",
                "Validate configuration and check network connectivity",
            ),
            ("db", "SQLite database management"),
        ],
    ),
    (
        "template",
        &[
            ("list", "List marketplace templates"),
            ("search <QUERY>", "Search templates"),
            ("show <ID>", "Template details"),
            ("init <ID> <DIR>", "Scaffold from template"),
            ("publish", "Publish template metadata"),
            ("remove <ID>", "Remove local template entry"),
        ],
    ),
    (
        "plugin",
        &[
            ("install", "Install a third-party plugin"),
            ("list", "List installed plugins"),
            ("run", "Run a plugin command"),
        ],
    ),
    (
        "test",
        &[(
            "test --wasm <FILE>",
            "Run Soroban contract tests (--coverage, --fixture, --report)",
        )],
    ),
    (
        "gas",
        &[
            ("analyse <WASM>", "Heuristic gas/cpu report"),
            ("optimize", "Lightweight WASM shrink pass"),
            ("diff <OLD> <NEW>", "Compare estimated costs"),
        ],
    ),
    (
        "security",
        &[
            (
                "audit <PATH>",
                "Run built-in Soroban analysis (--format, --ci, --track)",
            ),
            (
                "remediation list",
                "Review tracked audit and pentest remediation items",
            ),
        ],
    ),
    (
        "governance",
        &[
            (
                "propose",
                "Create upgrade proposal (--contract-id, --wasm, --threshold)",
            ),
            ("list", "List proposals"),
            ("show", "Show proposal details and votes"),
            ("vote", "Cast a vote (--for / --against)"),
            ("reject", "Reject a proposal"),
            ("execute", "Execute after timelock and threshold met"),
            ("emergency", "Emergency upgrade (bypasses timelock)"),
            ("audit", "Show governance audit trail"),
        ],
    ),
    (
        "upgrade",
        &[
            ("prepare", "Validate upgrade WASM"),
            ("auto compat", "Compare old/new WASM ABI and storage layout"),
            ("auto plan", "Generate compatibility-aware upgrade plan"),
            ("propose", "Create governance proposal"),
            ("list / status", "List pending proposals"),
            ("approve", "Approve proposal"),
            ("execute", "Execute approved upgrade"),
            ("rollback", "Roll back contract version"),
            ("history", "Show upgrade history"),
        ],
    ),
    (
        "multisig",
        &[
            ("wizard", "Interactive transaction proposal builder"),
            (
                "create",
                "Create a proposal with threshold, signers, metadata",
            ),
            ("status <FILE>", "Show signature collection progress"),
            (
                "verify <FILE>",
                "Validate signatures and threshold readiness",
            ),
            ("notify <FILE>", "Queue signature request notifications"),
            ("export / import", "Share proposal JSON between signers"),
        ],
    ),
    (
        "tutorial",
        &[
            ("list", "List tutorials"),
            ("start <SLUG>", "Begin a guided flow"),
            ("next", "Mark step complete and show next milestone"),
            ("status", "Show active tutorial and current step"),
        ],
    ),
    (
        "simulate",
        &[(
            "resources",
            "Report CPU, memory, footprint, and minimum resource fee",
        )],
    ),
    (
        "cost",
        &[(
            "resources",
            "Price a simulation and check against budgets (--enforce)",
        )],
    ),
    (
        "advanced-perf",
        &[
            (
                "profile <WASM>",
                "Profile a compiled Soroban contract artifact",
            ),
            ("analyze <CONTRACT>", "Analyze recorded runtime metrics"),
            ("compare <CONTRACT>", "Compare profiles across time windows"),
            (
                "generate-dashboard <CONTRACT>",
                "Show the recorded-metrics dashboard",
            ),
        ],
    ),
    (
        "docs",
        &[
            (
                "generate <CONTRACT>",
                "Generate documentation (--source, --lang)",
            ),
            ("extract <PATH>", "Extract rustdoc comments"),
            ("show / list / search", "Browse the local docs store"),
            (
                "html / api-ref / publish",
                "HTML site and publishing helpers",
            ),
        ],
    ),
];

const SUBCOMMAND_INFO: &[(&str, &str)] = &[
    ("ai-debug", "AI-powered contract debugging assistant"),
    ("ai-navigate", "AI-driven code navigation and search"),
    ("ai-quality-gate", "Configurable code quality gates"),
    ("ai", "Local LLM assistant for Soroban contracts"),
    (
        "wallet",
        "Manage test wallets (create, list, fund, show, remove)",
    ),
    ("nl", "Natural language command interface"),
    ("new", "Generate Soroban project boilerplate"),
    ("contract", "Contract operations (invoke, inspect, etc.)"),
    (
        "generate",
        "Generate smart contracts from natural language prompts",
    ),
    ("complete", "Smart contract completion assistant"),
    (
        "debug",
        "Debug Soroban contracts with breakpoints, stepping, and inspection",
    ),
    (
        "inspect",
        "Deep contract storage inspection (state, key, storage)",
    ),
    ("deploy", "Deploy a compiled Soroban contract (.wasm)"),
    (
        "deployments",
        "Deployment history, rollback, verification, and dashboard",
    ),
    ("info", "Show starforge config and environment info"),
    ("prompts", "Manage AI prompt templates and versioning"),
    (
        "explain",
        "Analyze and explain smart contract code using AI",
    ),
    (
        "config",
        "Manage starforge configuration (telemetry, network)",
    ),
    ("telemetry", "Manage telemetry settings directly"),
    ("tx", "Fetch transaction for the account"),
    (
        "network",
        "View or switch the active network (testnet/mainnet)",
    ),
    ("node", "Local Soroban devnet (Docker quickstart)"),
    (
        "completions",
        "Generate shell completions for bash, zsh, and fish",
    ),
    (
        "autocomplete",
        "Smart autocomplete — suggest and record commands",
    ),
    (
        "shell",
        "Interactive REPL for local Soroban contract testing",
    ),
    (
        "monitor",
        "Live monitoring (contract events or wallet threshold)",
    ),
    ("tutorial", "Interactive CLI tutorials"),
    ("benchmark", "Performance benchmarking utilities"),
    ("test", "Contract testing utilities for Soroban wasm"),
    ("gas", "Gas analysis and optimization helpers"),
    ("cost", "AI-assisted deployment cost management"),
    ("plugin", "Manage third-party plugins"),
    (
        "privacy",
        "Privacy protection, anonymization, consent, and reporting",
    ),
    ("project", "AI-driven project management"),
    (
        "template",
        "Manage community contract templates from the marketplace",
    ),
    ("registry", "Interact with the remote template registry"),
    ("multisig", "Manage multi-signature transactions"),
    ("upgrade", "Contract upgrade management"),
    ("governance", "Contract upgrade governance"),
    ("orchestrate", "Multi-contract deployment orchestration"),
    (
        "pipeline",
        "Visual pipeline builder for contract deployment workflows",
    ),
    ("security", "Security hardening, validation, and monitoring"),
    (
        "audit",
        "Run a comprehensive security audit on a Soroban contract",
    ),
    ("ai-audit", "AI-powered security audit"),
    ("ai-test", "AI-driven testing assistance"),
    ("ai-property-test", "AI property-based testing"),
    ("ai-feedback", "AI feedback and learning system"),
    ("ai-search", "AI code search and discovery"),
    ("ai-recommend", "AI best practice recommendations"),
    ("ai-route", "Intelligent AI model selection and routing"),
    ("ai-plan", "AI project planning assistant"),
    ("ai-accessibility", "AI accessibility features"),
    ("ai-contract-suggest", "AI contract function suggestions"),
    ("ai-doc-qa", "AI documentation Q&A"),
    ("schedule", "Schedule deployments for future execution"),
    (
        "simulate",
        "Local network simulation and testing environment",
    ),
    ("backup", "Backup and disaster recovery"),
    ("lint", "Static analysis and linting for Soroban contracts"),
    (
        "diagnostics",
        "Run connectivity diagnostics for Ledger/Trezor devices",
    ),
    ("template-vcs", "Template version control"),
    (
        "perf",
        "Contract performance monitoring and metrics dashboard",
    ),
    ("advanced-perf", "Advanced contract performance analysis"),
    ("docs", "Contract documentation portal"),
    ("analytics", "Contract deployment analytics and reporting"),
    ("approval", "Approval workflow for contract deployments"),
    ("feature-flags", "Manage feature flags for AI features"),
    ("migrate", "Contract storage migration tools"),
    ("collab", "AI-driven collaboration tools"),
    ("verify", "Run formal verification on a contract"),
    ("help", "AI contextual help"),
    ("ai-telemetry", "AI usage telemetry and analytics"),
    (
        "optimize",
        "Analyse and optimize compiled WASM / Rust contract source",
    ),
    ("ai-security-training", "AI-driven security training"),
    (
        "contract-monitor",
        "Contract health monitoring and alerting",
    ),
    ("man", "Generate or install man pages"),
];

/// Render `docs/COMMAND_CHEATSHEET.md` from the clap `Command` tree so the
/// generated sheet can never drift from the actual CLI surface.
fn render_cheatsheet(cmd: &clap::Command) -> String {
    let mut out = String::new();
    out.push_str("# StarForge Command Cheat Sheet\n\n");
    out.push_str(
        "> **Auto-generated** from clap command metadata by `build.rs`. Do not edit by hand.\n",
    );
    out.push_str(
        "> Regenerate with `cargo build` (build.rs rewrites this file), then commit the result.\n",
    );
    out.push_str("> See `DEVELOPER_GUIDE.md` → “Command cheat sheet” for details.\n\n");

    out.push_str(&format!(
        "`{}` — {}\n\n",
        cmd.get_name(),
        cmd.get_about().map(|a| a.to_string()).unwrap_or_default()
    ));

    out.push_str("## Usage\n\n```\nstarforge <command> [options]\n```\n\n");
    out.push_str(
        "Global options: `--json`, `--quiet`/`-q`, `--log-format`, `--log-dir`, \
         `--correlation-id`, `--non-interactive`, `-h`/`--help`, `-V`/`--version`.\n\n",
    );

    out.push_str("## Top-level commands\n\n| Command | Description |\n|---|---|\n");

    let mut subs: Vec<&clap::Command> = cmd.get_subcommands().collect();
    subs.sort_by(|a, b| a.get_name().cmp(b.get_name()));
    for sub in &subs {
        if is_internal(sub) {
            continue;
        }
        let about = sub.get_about().map(|a| a.to_string()).unwrap_or_default();
        out.push_str(&format!(
            "| `{}` | {} |\n",
            sub.get_name(),
            escape_md(&about)
        ));
    }
    out.push_str("\n");

    // Major subcommand groups (excludes any that were removed/renamed).
    for (parent, children) in MAJOR_SUBCOMMANDS {
        if !subs.iter().any(|s| s.get_name() == *parent) || is_internal_name(parent) {
            continue;
        }
        out.push_str(&format!("## `{}` subcommands\n\n", parent));
        out.push_str("| Subcommand | Description |\n|---|---|\n");
        for (name, desc) in *children {
            out.push_str(&format!("| `{}` | {} |\n", name, escape_md(desc)));
        }
        out.push_str("\n");
    }

    out
}

fn is_internal(c: &clap::Command) -> bool {
    c.is_hide_set() || is_internal_name(c.get_name())
}

fn is_internal_name(name: &str) -> bool {
    INTERNAL_COMMANDS.contains(&name)
}

/// Escape `|` (table delimiter) and newlines inside clap metadata so values
/// render as a single clean table cell.
fn escape_md(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn main() {
    let _outdir = match env::var_os("OUT_DIR") {
        None => return,
        Some(_outdir) => _outdir,
    };

    let mut cmd = Cli::command();

    let project_root = env::var("CARGO_MANIFEST_DIR").unwrap();

    // ── Shell completions ─────────────────────────────────────────────────
    let completions_dir = Path::new(&project_root).join("completions");
    fs::create_dir_all(&completions_dir).unwrap();

    for &shell in &[Shell::Bash, Shell::Zsh, Shell::Fish] {
        generate_to(shell, &mut cmd, "starforge", &completions_dir)
            .expect("Failed to generate completions");
    }

    // ── Cheat sheet ───────────────────────────────────────────────────────
    let docs_dir = Path::new(&project_root).join("docs");
    fs::create_dir_all(&docs_dir).unwrap();
    let cheatsheet_path = docs_dir.join("COMMAND_CHEATSHEET.md");
    fs::write(&cheatsheet_path, render_cheatsheet(&cmd))
        .expect("Failed to generate docs/COMMAND_CHEATSHEET.md");
    println!("cargo:rerun-if-changed=build.rs");

    // ── Man pages ─────────────────────────────────────────────────────────
    let man_dir = Path::new(&project_root).join("man");
    fs::create_dir_all(&man_dir).unwrap();

    // Main starforge(1) page
    let main_cmd = Cli::command();
    let man = clap_mangen::Man::new(main_cmd)
        .title("starforge".to_string())
        .section("1".to_string())
        .source("StarForge".to_string())
        .manual("User Manual".to_string());
    man.generate_to(&man_dir)
        .expect("Failed to generate starforge.1 man page");

    // Per-subcommand pages
    for &(name, about) in SUBCOMMAND_INFO {
        if is_internal_name(name) {
            continue;
        }
        let full_name = format!("starforge-{}", name);
        let full_name_static: &'static str = Box::leak(full_name.into_boxed_str());
        let sub_cmd = clap::Command::new(full_name_static)
            .about(about)
            .version("0.1.0");

        let man = clap_mangen::Man::new(sub_cmd)
            .title(full_name_static)
            .section("1".to_string())
            .source("StarForge".to_string())
            .manual("User Manual".to_string());
        man.generate_to(&man_dir)
            .unwrap_or_else(|e| panic!("Failed to generate {} man page: {}", full_name_static, e));
    }

    // ── rustc version capture ─────────────────────────────────────────────
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .expect("Failed to get rustc version");
    let version = String::from_utf8(output.stdout).unwrap();
    println!("cargo:rustc-env=RUSTC_VERSION={}", version.trim());
}
