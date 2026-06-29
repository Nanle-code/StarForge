use anyhow::Result;
use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use std::io::{self, Write};

/// Shell completion generation options
#[derive(Args)]
pub struct CompletionArgs {
    /// Include dynamic plugin command completions resolved at shell-completion time
    #[arg(long)]
    pub dynamic: bool,

    #[command(subcommand)]
    pub shell: CompletionShell,
}

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

pub async fn handle(args: CompletionArgs) -> Result<()> {
    let shell = match args.shell {
        CompletionShell::Bash => Shell::Bash,
        CompletionShell::Zsh => Shell::Zsh,
        CompletionShell::Fish => Shell::Fish,
    };
    let mut buf = Vec::new();
    generate_completion(shell, &mut buf);

    if args.dynamic {
        append_dynamic_plugin_completions(shell, &mut buf);
    } else {
        // Keep existing generation-time plugin names for users who prefer static scripts.
        let plugin_cmds = crate::plugins::registry::load_all_registered_commands();
        if !plugin_cmds.is_empty() {
            append_static_plugin_completions(shell, &plugin_cmds, &mut buf);
        }
    }

    io::stdout().write_all(&buf)?;
    Ok(())
}

fn append_static_plugin_completions(
    shell: Shell,
    cmds: &[crate::plugins::registry::RegisteredCommand],
    buf: &mut Vec<u8>,
) {
    use std::io::Write;
    match shell {
        Shell::Fish => {
            for cmd in cmds {
                let _ = writeln!(
                    buf,
                    "complete -c starforge -n '__fish_use_subcommand starforge' -f -a '{}' -d '{}'",
                    cmd.name,
                    cmd.description.replace('\'', "\\'")
                );
            }
        }
        Shell::Bash => {
            // Inject plugin names into the top-level subcommand list.
            let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
            let _ = writeln!(
                buf,
                "\n# Plugin commands\n_starforge_plugin_cmds='{}'\n",
                names.join(" ")
            );
        }
        Shell::Zsh => {
            let _ = writeln!(buf, "\n# Plugin commands");
            for cmd in cmds {
                let _ = writeln!(
                    buf,
                    "# plugin: {} -- {}",
                    cmd.name,
                    cmd.description.replace('\'', "\\'")
                );
            }
        }
        _ => {}
    }
}

fn append_dynamic_plugin_completions(shell: Shell, buf: &mut Vec<u8>) {
    use std::io::Write;
    match shell {
        Shell::Bash => {
            let _ = writeln!(
                buf,
                r#"
# Dynamic plugin commands
_starforge_plugin_commands() {{
    starforge --list-plugin-commands 2>/dev/null
}}

_starforge_with_plugins() {{
    _starforge "$@"
    if [[ ${{COMP_CWORD:-0}} -eq 1 ]]; then
        local cur="${{COMP_WORDS[COMP_CWORD]}}"
        local plugin_words
        plugin_words="$(_starforge_plugin_commands)"
        if [[ -n "$plugin_words" ]]; then
            COMPREPLY+=( $(compgen -W "$plugin_words" -- "$cur") )
        fi
    fi
}}

complete -F _starforge_with_plugins starforge
"#
            );
        }
        Shell::Zsh => {
            let _ = writeln!(
                buf,
                r#"
# Dynamic plugin commands
_starforge_plugin_commands() {{
    local -a plugin_commands
    plugin_commands=("${{(@f)$(starforge --list-plugin-commands 2>/dev/null)}}")
    if (( ${{#plugin_commands}} )); then
        compadd -- $plugin_commands
    fi
}}

_starforge_with_plugins() {{
    _starforge "$@"
    if (( CURRENT == 2 )); then
        _starforge_plugin_commands
    fi
}}

compdef _starforge_with_plugins starforge
"#
            );
        }
        Shell::Fish => {
            let _ = writeln!(
                buf,
                r#"
# Dynamic plugin commands
function __starforge_complete_plugins
    starforge --list-plugin-commands 2>/dev/null
end

complete -c starforge -n "__fish_use_subcommand" -f -a "(__starforge_complete_plugins)" -d "Installed plugin command"
"#
            );
        }
        _ => {}
    }
}

/// Generate completion script to a writer instead of stdout (used in tests).
pub fn generate_completion(shell: Shell, writer: &mut impl io::Write) {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "starforge", writer);
}

// ── Mirror of the top-level CLI ───────────────────────────────────────────────
// clap_complete needs the full Command tree at generation time.
// Keep this in sync with main.rs; only structure is needed, not handler logic.

#[derive(Parser)]
#[command(
    name = "starforge",
    version = "0.1.0",
    about = "Stellar & Soroban developer productivity CLI"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Suppress the ASCII banner and decorative output
    #[arg(long, short = 'q', global = true)]
    quiet: bool,

    /// Log output format: human (default) or json
    #[arg(long, global = true, default_value = "human", value_parser = ["human", "json"])]
    log_format: String,

    /// Directory to write rotating log files into (optional)
    #[arg(long, global = true)]
    log_dir: Option<std::path::PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage test wallets (create, list, fund, show, remove)
    #[command(subcommand)]
    Wallet(crate::commands::wallet::WalletCommands),
    /// Generate Soroban project boilerplate
    #[command(subcommand)]
    New(crate::commands::new::NewCommands),
    /// Contract operations (invoke, inspect, etc.)
    #[command(subcommand)]
    Contract(crate::commands::contract::ContractCommands),
    /// Deep contract storage inspection (state, key, storage)
    #[command(subcommand)]
    Inspect(crate::commands::inspect::InspectCommands),
    /// Deploy a compiled Soroban contract (.wasm)
    Deploy(crate::commands::deploy::DeployArgs),
    /// Show starforge config and environment info
    Info,
    /// Fetch transaction for an account
    Tx(crate::commands::tx::TxArgs),
    /// View or switch the active network (testnet/mainnet)
    #[command(subcommand)]
    Network(crate::commands::network::NetworkCommands),
    /// Local Soroban devnet (Docker quickstart)
    #[command(subcommand)]
    Node(crate::commands::node::NodeCommands),
    /// Generate shell completions for bash, zsh, and fish
    Completions(CompletionArgs),
    /// Interactive REPL for local Soroban contract testing
    Shell(crate::commands::shell::ShellArgs),
    /// Live monitoring (contract events or wallet threshold)
    Monitor(crate::commands::monitor::MonitorArgs),
    /// Interactive CLI tutorials
    #[command(subcommand)]
    Tutorial(crate::commands::tutorial::TutorialCommands),
    /// Performance benchmarking utilities
    Benchmark(crate::commands::benchmark::BenchmarkArgs),
    /// Contract testing utilities for Soroban wasm
    Test(crate::commands::test::TestArgs),
    /// Gas analysis and optimization helpers
    #[command(subcommand)]
    Gas(crate::commands::gas::GasCommands),
    /// Manage third-party plugins
    #[command(subcommand)]
    Plugin(crate::commands::plugin::PluginCommands),
    /// Manage community contract templates from the marketplace
    #[command(subcommand)]
    Template(crate::commands::template::TemplateCommands),
    /// Contract upgrade management (propose, approve, execute, rollback)
    #[command(subcommand)]
    Upgrade(crate::commands::upgrade::UpgradeCommands),
    /// Static analysis and linting for Soroban contracts
    Lint(crate::commands::lint::LintArgs),
    /// Execute an installed plugin command
    #[command(external_subcommand)]
    #[allow(dead_code)]
    External(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap_complete::Shell;

    fn completion_output(shell: Shell) -> String {
        let mut buf = Vec::new();
        generate_completion(shell, &mut buf);
        String::from_utf8(buf).expect("completion output is valid UTF-8")
    }

    fn dynamic_completion_output(shell: Shell) -> String {
        let mut buf = Vec::new();
        append_dynamic_plugin_completions(shell, &mut buf);
        String::from_utf8(buf).expect("dynamic completion output is valid UTF-8")
    }

    // ── bash ──────────────────────────────────────────────────────────────────

    #[test]
    fn bash_completion_generates_non_empty_output() {
        let out = completion_output(Shell::Bash);
        assert!(!out.is_empty(), "bash completion output must not be empty");
    }

    #[test]
    fn bash_completion_contains_function_definition() {
        let out = completion_output(Shell::Bash);
        assert!(
            out.contains("_starforge"),
            "bash completion should define a _starforge function"
        );
    }

    #[test]
    fn bash_completion_lists_core_subcommands() {
        let out = completion_output(Shell::Bash);
        for cmd in ["wallet", "deploy", "template", "plugin", "completions"] {
            assert!(
                out.contains(cmd),
                "bash completion must include subcommand '{}'",
                cmd
            );
        }
    }

    #[test]
    fn bash_completion_lists_all_subcommands() {
        let out = completion_output(Shell::Bash);
        for cmd in [
            "wallet",
            "new",
            "contract",
            "inspect",
            "deploy",
            "info",
            "tx",
            "network",
            "node",
            "completions",
            "shell",
            "monitor",
            "tutorial",
            "benchmark",
            "test",
            "gas",
            "plugin",
            "template",
            "upgrade",
            "lint",
        ] {
            assert!(
                out.contains(cmd),
                "bash completion missing subcommand '{}'",
                cmd
            );
        }
    }

    // ── zsh ───────────────────────────────────────────────────────────────────

    #[test]
    fn zsh_completion_generates_non_empty_output() {
        let out = completion_output(Shell::Zsh);
        assert!(!out.is_empty(), "zsh completion output must not be empty");
    }

    #[test]
    fn zsh_completion_has_compdef_header() {
        let out = completion_output(Shell::Zsh);
        assert!(
            out.contains("#compdef starforge"),
            "zsh completion must start with #compdef starforge"
        );
    }

    #[test]
    fn zsh_completion_lists_all_subcommands() {
        let out = completion_output(Shell::Zsh);
        for cmd in [
            "wallet",
            "new",
            "contract",
            "inspect",
            "deploy",
            "info",
            "tx",
            "network",
            "node",
            "completions",
            "shell",
            "monitor",
            "tutorial",
            "benchmark",
            "test",
            "gas",
            "plugin",
            "template",
            "upgrade",
            "lint",
        ] {
            assert!(
                out.contains(cmd),
                "zsh completion missing subcommand '{}'",
                cmd
            );
        }
    }

    // ── fish ──────────────────────────────────────────────────────────────────

    #[test]
    fn fish_completion_generates_non_empty_output() {
        let out = completion_output(Shell::Fish);
        assert!(!out.is_empty(), "fish completion output must not be empty");
    }

    #[test]
    fn fish_completion_uses_complete_command() {
        let out = completion_output(Shell::Fish);
        assert!(
            out.contains("complete -c starforge"),
            "fish completion must use 'complete -c starforge'"
        );
    }

    #[test]
    fn fish_completion_lists_all_subcommands() {
        let out = completion_output(Shell::Fish);
        for cmd in [
            "wallet",
            "new",
            "contract",
            "inspect",
            "deploy",
            "info",
            "tx",
            "network",
            "node",
            "completions",
            "shell",
            "monitor",
            "tutorial",
            "benchmark",
            "test",
            "gas",
            "plugin",
            "template",
            "upgrade",
            "lint",
        ] {
            assert!(
                out.contains(cmd),
                "fish completion missing subcommand '{}'",
                cmd
            );
        }
    }

    // ── regression coverage ───────────────────────────────────────────────────

    #[test]
    fn all_shells_include_global_flags() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let out = completion_output(shell);
            assert!(
                out.contains("quiet") || out.contains("-q"),
                "{:?} completion should include --quiet / -q flag",
                shell
            );
        }
    }

    #[test]
    fn completions_subcommand_itself_is_listed() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let out = completion_output(shell);
            assert!(
                out.contains("completions"),
                "{:?} completion must list the completions subcommand",
                shell
            );
        }
    }

    #[test]
    fn bash_dynamic_completion_queries_plugin_command_list() {
        let out = dynamic_completion_output(Shell::Bash);
        assert!(out.contains("--list-plugin-commands"));
        assert!(out.contains("_starforge_with_plugins"));
        assert!(out.contains("complete -F _starforge_with_plugins starforge"));
    }

    #[test]
    fn zsh_dynamic_completion_queries_plugin_command_list() {
        let out = dynamic_completion_output(Shell::Zsh);
        assert!(out.contains("--list-plugin-commands"));
        assert!(out.contains("_starforge_plugin_commands"));
        assert!(out.contains("compdef _starforge_with_plugins starforge"));
    }

    #[test]
    fn fish_dynamic_completion_queries_plugin_command_list() {
        let out = dynamic_completion_output(Shell::Fish);
        assert!(out.contains("--list-plugin-commands"));
        assert!(out.contains("__starforge_complete_plugins"));
        assert!(out.contains("complete -c starforge"));
    }
}
