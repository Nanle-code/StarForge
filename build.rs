use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate_to, Shell};
use std::env;
use std::fs;
use std::path::Path;

#[derive(Parser)]
#[command(
    name = "starforge",
    version = "0.1.0",
    about = "⚡ Stellar & Soroban developer productivity CLI",
    long_about = "Stellar & Soroban developer productivity CLI.\n\n\
                  StarForge wraps the Stellar CLI with wallet management, contract \
                  testing and audits, deployment lifecycle management, AI-assisted \
                  development, and community contract templates. Commands are grouped \
                  by noun: wallet, contract, deploy, network, template, plugin, ai, \
                  config, project, tool. See docs/CLI_COMMAND_TREE.md for the full tree.",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// Top-level command model, mirrored from `src/main.rs` (ADR 0007).
///
/// Only the names and `about` text matter here: this copy drives the generated
/// `docs/COMMAND_CHEATSHEET.md`, the shell completions, and the man pages, so
/// they cannot drift from a hand-written list. Per-subcommand detail lives in
/// [`MAJOR_SUBCOMMANDS`]; keep the two in sync when commands change. The
/// top-level budget asserted in `main` below is the regression guard for the
/// 20-command limit from issue #936.
#[derive(Subcommand)]
enum Commands {
    // Mirrors the noun-verb tree from `src/main.rs` (ADR 0007). Only names and
    // `about` text are needed here: this copy drives the generated cheat sheet,
    // the shell completions, and the man pages. The top-level budget check in
    // `main()` below keeps it honest about the command count.
    #[command(about = "Manage test wallets (create, list, fund, sign), transactions, and devices")]
    Wallet,
    #[command(
        about = "Contract operations (invoke, build, test, audit, upgrade, inspect, monitor)"
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
    #[command(about = "On-chain account lifecycle with sponsored reserves (CAP-33)")]
    Account,
    #[command(about = "Natural language command interface")]
    Nl,
    #[command(about = "Generate Soroban project boilerplate")]
    New,
    #[command(about = "Contract operations (invoke, inspect, etc.)")]
    Contract,
    #[command(about = "Deploy a compiled Soroban contract and manage the deployment lifecycle")]
    Deploy,
    #[command(about = "View or switch the active network, run a local node, simulate, snapshot")]
    Network,
    #[command(about = "Manage community contract templates, versions, and the registry")]
    Template,
    #[command(about = "Manage third-party plugins")]
    Plugin,
    #[command(about = "AI-assisted development: local assistant, audits, tests, search, planning")]
    Ai,
    #[command(about = "Manage starforge configuration, telemetry, feature flags, and privacy")]
    Config,
    #[command(about = "Project scaffolding and AI-driven project management")]
    Project,
    #[command(about = "Developer-environment utilities: tutorials, natural language, PR checks")]
    Tool,
    #[command(about = "Generate shell completions for bash, zsh, fish, and powershell")]
    Completions,
    #[command(about = "Generate or install man pages")]
    Man,
    #[command(
        about = "Smart autocomplete — suggest and record commands",
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
    #[command(about = "Manage per-network contract and account aliases")]
    Alias,
}

/// The acceptance criterion from issue #936: top-level `--help` shows at most
/// this many commands. Keeping it here means a future top-level addition fails
/// the build instead of quietly regressing discoverability.
const MAX_TOP_LEVEL_COMMANDS: usize = 20;

/// Internal / developer-only top-level commands that are excluded from the
/// generated cheat sheet. Hidden commands (`#[command(hide)]`) are excluded
/// automatically by the renderer; this set covers the ones that are declared
/// but considered internal tooling rather than user-facing CLI surface.
///
/// Keep this list consistent: it is the single source of truth for "what does
/// not appear in the cheat sheet" and is also applied when the man-page names
/// are filtered below.
const INTERNAL_COMMANDS: &[&str] = &["external", "autocomplete", "man"];

/// Named subcommand detail for the "major subcommands" section of the cheat
/// sheet. The top-level command list is derived from clap metadata; this table
/// records the most important subcommands under each group so the cheat sheet
/// remains useful as a quick reference.
///
/// The renderer asserts that every parent listed here is a real top-level
/// command, so a rename/removal fails the build instead of silently drifting.
const MAJOR_SUBCOMMANDS: &[(&str, &[(&str, &str)])] = &[
    (
        "account",
        &[
            (
                "create --sponsor <WALLET> --to <G...>",
                "Create an account with sponsored reserves (CAP-33, --fee-payer, --yes)",
            ),
            (
                "end-sponsorship --wallet <WALLET>",
                "Release a sponsor's reserve (--fee-payer, --yes)",
            ),
        ],
    ),
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
            ("multisig", "Multi-signature account management"),
            ("tx", "Fetch a transaction for the account"),
            ("auth", "SEP-10 web authentication against an anchor"),
            ("diagnostics", "Ledger/Trezor connectivity diagnostics"),
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
            (
                "build",
                "Build a contract with StarForge provenance metadata",
            ),
            ("inspect", "Inspect a deployed Soroban contract instance"),
            ("upload", "Upload a WASM binary to the Stellar network"),
            (
                "generate-bindings <WASM>",
                "Generate typed client bindings (--lang rust|ts|python|go)",
            ),
            ("storage <state\\|key\\|storage>", "Deep storage inspection"),
            ("debug", "Breakpoints, stepping, and inspection"),
            ("repl", "Interactive REPL for local contract testing"),
            (
                "test --wasm <FILE>",
                "Run contract tests (--coverage, --fixture, --report)",
            ),
            ("audit", "Run a comprehensive security audit"),
            ("security", "Hardening, validation, monitoring, incidents"),
            ("governance", "Upgrade proposals, voting, timelock, audit"),
            ("upgrade", "Propose, approve, execute, roll back an upgrade"),
            ("verify", "Run formal verification"),
            (
                "migrate",
                "Storage migrations (transform, validate, rollback)",
            ),
            ("generate", "Generate contracts from natural language"),
            ("explain", "Explain contract code with AI"),
            ("lint", "Static analysis and linting"),
            ("optimize", "WASM/source optimisation for gas and size"),
            ("gas", "Gas analysis, diffs, estimates, alerts"),
            (
                "metrics",
                "Performance metrics, dashboards, regression baselines",
            ),
            ("profile", "Advanced performance analysis and profiling"),
            ("benchmark", "Performance benchmarks and comparisons"),
            ("docs", "Contract documentation portal"),
            ("mutate", "AI mutation testing"),
            (
                "monitor",
                "Live contract event or wallet-threshold monitoring",
            ),
            ("health", "Contract health monitoring and alerting"),
        ],
    ),
    (
        "deploy",
        &[
            (
                "run --wasm <FILE>",
                "Prepare a Soroban deployment (--simulate, --execute)",
            ),
            (
                "history",
                "Deployment history, rollback, verification, dashboard",
            ),
            ("env", "Manage dev/staging/production environments"),
            ("schedule", "Schedule deployments with approval workflows"),
            ("orchestrate", "Multi-contract deployment orchestration"),
            (
                "pipeline",
                "Visual pipeline builder for deployment workflows",
            ),
            ("approval", "Multi-level deployment approvals"),
            ("cost", "Budgets, forecasting, and cross-network comparison"),
            ("analytics", "Deployment analytics and reporting"),
            ("backup", "Backup and disaster recovery"),
        ],
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
            ("node", "Local Soroban devnet (Docker quickstart)"),
            ("simulate", "Local network simulation and testing"),
            ("snapshot", "Deterministic live-ledger snapshots"),
        ],
    ),
    (
        "sep10",
        &[(
            "auth <--domain <HOME_DOMAIN>> <--wallet <NAME>>",
            "Validate, sign, and exchange a SEP-10 challenge for a session JWT",
        )],
    ),
    (
        "config",
        &[
            (
                "show",
                "Show effective configuration (user config + project lockfile)",
            ),
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
            ("info", "Show config and environment info"),
            ("telemetry", "Telemetry settings and opt-out"),
            ("flags", "AI feature flags, rollouts, rollback"),
            ("privacy", "Anonymization, consent, and reporting"),
        ],
    ),
    (
        "project",
        &[
            ("new", "Generate Soroban project boilerplate"),
            ("task", "Create, assign, track, and complete tasks"),
            ("progress", "Visualize task progress"),
            ("sprint", "Sprint planning and burndown"),
            ("collab", "Code review, conflict resolution, knowledge base"),
        ],
    ),
    (
        "tool",
        &[
            ("tutorial", "Interactive, step-by-step CLI tutorials"),
            ("nl <INPUT>", "Natural language command interface"),
            ("pr", "Check PR readiness (CI green, no conflicts)"),
            ("bug-report", "Prefilled environment bug report"),
        ],
    ),
];

/// Man page names, following the noun-verb tree (ADR 0007).
///
/// Each entry becomes `man/starforge-<name>.1`. Nested verbs use their dashed
/// path (`contract test` -> `starforge-contract-test.1`).
const SUBCOMMAND_INFO: &[(&str, &str)] = &[
    (
        "wallet",
        "Manage test wallets, transactions, and hardware devices",
    ),
    ("wallet tx", "Fetch a transaction for the account"),
    (
        "wallet auth",
        "SEP-10 web authentication for Stellar anchors",
    ),
    ("wallet multisig", "Manage multi-signature transactions"),
    (
        "wallet diagnostics",
        "Connectivity diagnostics for Ledger/Trezor devices",
    ),
    (
        "contract",
        "Invoke, build, inspect, and manage a Soroban contract",
    ),
    (
        "contract storage",
        "Deep contract storage inspection (state, key, storage)",
    ),
    (
        "contract debug",
        "Debug Soroban contracts with breakpoints and stepping",
    ),
    (
        "contract repl",
        "Interactive REPL for local Soroban contract testing",
    ),
    (
        "contract test",
        "Contract testing utilities for Soroban wasm",
    ),
    (
        "contract audit",
        "Run a comprehensive security audit on a Soroban contract",
    ),
    (
        "contract security",
        "Security hardening, validation, and monitoring",
    ),
    ("contract governance", "Contract upgrade governance"),
    ("contract upgrade", "Contract upgrade management"),
    ("contract verify", "Run formal verification on a contract"),
    ("contract migrate", "Contract storage migration tools"),
    (
        "contract generate",
        "Generate smart contracts from natural language prompts",
    ),
    ("contract complete", "Smart contract completion assistant"),
    (
        "contract explain",
        "Analyze and explain smart contract code using AI",
    ),
    (
        "contract lint",
        "Static analysis and linting for Soroban contracts",
    ),
    (
        "contract optimize",
        "Analyse and optimize compiled WASM and contract source",
    ),
    ("contract gas", "Gas analysis and optimization helpers"),
    (
        "contract metrics",
        "Contract performance monitoring and metrics dashboard",
    ),
    (
        "contract profile",
        "Advanced contract performance analysis and profiling",
    ),
    ("contract benchmark", "Performance benchmarking utilities"),
    ("contract docs", "Contract documentation portal"),
    (
        "contract mutate",
        "AI mutation testing for Soroban contracts",
    ),
    (
        "contract monitor",
        "Live monitoring (contract events or wallet threshold)",
    ),
    ("contract health", "Contract health monitoring and alerting"),
    (
        "deploy",
        "Deploy a compiled Soroban contract and manage the lifecycle",
    ),
    ("deploy run", "Deploy a compiled Soroban contract (.wasm)"),
    (
        "deploy history",
        "Deployment history, rollback, verification, dashboard",
    ),
    (
        "deploy env",
        "Manage deployment environments (dev/staging/production)",
    ),
    ("telemetry", "Manage telemetry settings directly"),
    ("tx", "Fetch transaction for the account"),
    ("sep10", "SEP-10 web authentication (anchor auth testing)"),
    (
        "deploy orchestrate",
        "Multi-contract deployment orchestration",
    ),
    (
        "deploy pipeline",
        "Visual pipeline builder for deployment workflows",
    ),
    (
        "deploy approval",
        "Approval workflow for contract deployments",
    ),
    ("deploy cost", "AI-assisted deployment cost management"),
    (
        "deploy analytics",
        "Contract deployment analytics and reporting",
    ),
    (
        "deploy backup",
        "Backup and disaster recovery for contract state",
    ),
    ("network", "View or switch the active network"),
    ("network node", "Local Soroban devnet (Docker quickstart)"),
    (
        "network simulate",
        "Local network simulation and testing environment",
    ),
    (
        "network snapshot",
        "Deterministic live-ledger snapshots for local tests",
    ),
    ("template", "Manage community contract templates"),
    ("template vcs", "Template version control"),
    (
        "template registry",
        "Interact with the remote template registry",
    ),
    ("plugin", "Manage third-party plugins"),
    (
        "ai",
        "AI-assisted development: assistant, audits, tests, search",
    ),
    ("ai local", "Local LLM assistant powered by Ollama"),
    ("ai debug", "AI-powered contract debugging assistant"),
    ("ai navigate", "AI-driven code navigation and search"),
    ("ai gate", "Configurable code quality gates"),
    ("ai profiling", "AI-driven performance profiling"),
    ("ai ide", "AI-powered IDE integration"),
    ("ai tests", "AI-driven testing assistance"),
    ("ai test-maintain", "AI-driven test maintenance"),
    ("ai deploy-test", "AI-driven deployment testing"),
    ("ai property-test", "AI property-based testing"),
    ("ai feedback", "AI feedback and learning system"),
    ("ai search", "AI code search and discovery"),
    ("ai recommend", "AI best practice recommendations"),
    ("ai route", "Intelligent AI model selection and routing"),
    ("ai plan", "AI project planning assistant"),
    ("ai accessibility", "AI accessibility features"),
    ("ai suggest", "AI contract function suggestions"),
    ("ai docs", "AI documentation Q&A"),
    ("ai telemetry", "AI usage telemetry and analytics"),
    ("ai training", "AI-driven security training"),
    ("ai security-audit", "AI-powered security audit"),
    ("ai prompts", "Manage AI prompt templates and versioning"),
    ("ai help", "AI contextual help"),
    (
        "config",
        "Manage starforge configuration, telemetry, flags, privacy",
    ),
    ("config info", "Show starforge config and environment info"),
    ("config telemetry", "Manage telemetry settings directly"),
    ("config flags", "Manage feature flags for AI features"),
    (
        "config privacy",
        "Privacy protection, anonymization, consent, reporting",
    ),
    (
        "project",
        "Project scaffolding and AI-driven project management",
    ),
    ("project new", "Generate Soroban project boilerplate"),
    ("project collab", "AI-driven collaboration tools"),
    ("tool", "Developer-environment utilities"),
    ("tool tutorial", "Interactive CLI tutorials"),
    ("tool nl", "Natural language command interface"),
    (
        "tool pr",
        "Check PR readiness (CI status and merge conflicts)",
    ),
    (
        "tool bug-report",
        "Collect environment diagnostics and a prefilled bug report",
    ),
    (
        "completions",
        "Generate shell completions for bash, zsh, fish, powershell",
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

    out.push_str("## Usage\n\n```bash norun\nstarforge <command> [options]\n```\n\n");
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
    out.push('\n');

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
        out.push('\n');
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

    // Issue #936: the top-level surface is a budget, not a suggestion. Fail the
    // build if a future command is added without folding it into a noun.
    let visible: Vec<String> = cmd
        .get_subcommands()
        .filter(|c| !c.is_hide_set())
        .map(|c| c.get_name().to_string())
        .collect();
    assert!(
        visible.len() <= MAX_TOP_LEVEL_COMMANDS,
        "top-level `starforge --help` shows {} commands ({}), budget is {}. \
         Fold the new command under an existing noun (ADR 0007) instead of \
         adding a top-level one.",
        visible.len(),
        visible.join(", "),
        MAX_TOP_LEVEL_COMMANDS
    );

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
        // CLI path (`contract test`) -> page file name (`starforge-contract-test.1`).
        let full_name = format!("starforge-{}", name.replace(' ', "-"));
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
