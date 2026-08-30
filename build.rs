use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate_to, Shell};
use std::env;
use std::fs;
use std::path::Path;

#[derive(Parser)]
#[command(name = "starforge", version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Wallet,
    New,
    Contract,
    Inspect,
    Deploy,
    Info,
    Tx,
    Network,
    Completions,
    Shell,
    Monitor,
    Tutorial,
    Benchmark,
    Test,
    Gas,
    Plugin,
    Template,
    Upgrade,
    Man,
    #[command(external_subcommand)]
    #[allow(dead_code)]
    External(Vec<String>),
}

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
        if name == "help" || name == "external" {
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

    println!("cargo:rerun-if-changed=build.rs");
}
