use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use std::io;

/// Shell to generate completions for
#[derive(Subcommand)]
pub enum CompletionShell {
    /// Generate bash completions
    Bash,
    /// Generate zsh completions
    Zsh,
    /// Generate fish completions
    Fish,
}

pub fn handle(shell: CompletionShell) -> Result<()> {
    // Import the top-level Cli so clap_complete can walk the full command tree.
    // We re-derive it here to avoid a circular dependency with main.rs.
    let mut cmd = Cli::command();
    let shell = match shell {
        CompletionShell::Bash => Shell::Bash,
        CompletionShell::Zsh => Shell::Zsh,
        CompletionShell::Fish => Shell::Fish,
    };
    generate(shell, &mut cmd, "starforge", &mut io::stdout());
    Ok(())
}

// ── Mirror of the top-level CLI ───────────────────────────────────────────────
// clap_complete needs the full Command tree at generation time.
// We keep this in sync with main.rs manually; it only needs the structure,
// not the handler logic.

#[derive(Parser)]
#[command(name = "starforge", version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage test wallets (create, list, fund, show, remove)
    #[command(subcommand)]
    Wallet(crate::commands::wallet::WalletCommands),
    /// Generate Soroban project boilerplate
    #[command(subcommand)]
    New(crate::commands::new::NewCommands),
    /// Contract operations (invoke, etc.)
    #[command(subcommand)]
    Contract(crate::commands::contract::ContractCommands),
    /// Deploy a compiled Soroban contract (.wasm)
    Deploy(crate::commands::deploy::DeployArgs),
    /// Show starforge config and environment info
    Info,
    Tx(crate::commands::tx::TxArgs),
    /// View or switch the active network (testnet/mainnet)
    #[command(subcommand)]
    Network(crate::commands::network::NetworkCommands),
    /// Print shell completion scripts
    #[command(subcommand)]
    Completions(CompletionShell),
}
