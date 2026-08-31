use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use std::io::{self, Write};

/// Shell to generate completions for.
///
/// Kept in sync with the shells clap_complete supports for this CLI: Bash,
/// Zsh, and Fish (the original three), plus PowerShell for Windows users.
#[derive(Subcommand, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionShell {
    /// Generate bash completions
    Bash,
    /// Generate zsh completions
    Zsh,
    /// Generate fish completions
    Fish,
    /// Generate PowerShell completions
    #[command(name = "powershell")]
    PowerShell,
}

impl CompletionShell {
    fn to_clap_shell(self) -> Shell {
        match self {
            CompletionShell::Bash => Shell::Bash,
            CompletionShell::Zsh => Shell::Zsh,
            CompletionShell::Fish => Shell::Fish,
            CompletionShell::PowerShell => Shell::PowerShell,
        }
    }
}

pub async fn handle(shell: CompletionShell) -> Result<()> {
    let shell = shell.to_clap_shell();
    let mut buf = Vec::new();
    generate_completion(shell, &mut buf);

    if buf.is_empty() {
        // clap_complete always writes a non-empty script for a command with
        // a name and at least one subcommand; an empty buffer means the
        // generator failed silently rather than that there's nothing to do.
        bail!("completion generation for {:?} produced no output", shell);
    }

    // Append plugin command completions so they are visible in tab completion.
    // Plugin names/descriptions come from a registry file on disk that may
    // have been hand-edited or written by a third-party plugin installer,
    // and this script is meant to be `source`d directly into the user's
    // shell — so untrusted-looking entries are dropped rather than escaped
    // and embedded, to avoid the generated script executing attacker
    // controlled shell code.
    let plugin_cmds = crate::plugins::registry::load_all_registered_commands();
    if !plugin_cmds.is_empty() {
        append_plugin_completions(shell, &plugin_cmds, &mut buf);
    }

    io::stdout()
        .write_all(&buf)
        .context("failed to write completion script to stdout")?;
    Ok(())
}

/// Plugin command names are only trusted for interpolation into a generated
/// completion script if they look like a plain identifier. This rejects
/// names carrying quotes, whitespace, or other shell metacharacters that
/// could otherwise break out of the single-quoted contexts the generators
/// below build.
fn is_safe_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':')
}

/// Strip control characters and escape single quotes so a (freeform) plugin
/// description can be safely embedded in a single-quoted shell string.
fn sanitize_plugin_description(description: &str) -> String {
    description
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .replace('\'', "\\'")
}

fn append_plugin_completions(
    shell: Shell,
    cmds: &[crate::plugins::registry::RegisteredCommand],
    buf: &mut Vec<u8>,
) {
    use std::io::Write;

    let safe_cmds: Vec<&crate::plugins::registry::RegisteredCommand> = cmds
        .iter()
        .filter(|c| is_safe_plugin_name(&c.name))
        .collect();
    if safe_cmds.is_empty() {
        return;
    }

    match shell {
        Shell::Fish => {
            for cmd in &safe_cmds {
                let _ = writeln!(
                    buf,
                    "complete -c starforge -n '__fish_use_subcommand starforge' -f -a '{}' -d '{}'",
                    cmd.name,
                    sanitize_plugin_description(&cmd.description)
                );
            }
        }
        Shell::Bash => {
            // Inject plugin names into the top-level subcommand list.
            let names: Vec<&str> = safe_cmds.iter().map(|c| c.name.as_str()).collect();
            let _ = writeln!(
                buf,
                "\n# Plugin commands\n_starforge_plugin_cmds='{}'\n",
                names.join(" ")
            );
        }
        Shell::Zsh => {
            let _ = writeln!(buf, "\n# Plugin commands");
            for cmd in &safe_cmds {
                let _ = writeln!(
                    buf,
                    "# plugin: {} -- {}",
                    cmd.name,
                    sanitize_plugin_description(&cmd.description)
                );
            }
        }
        Shell::PowerShell => {
            // PowerShell's generated script is a single Register-ArgumentCompleter
            // ScriptBlock; rather than splice entries into it, document plugin
            // commands as comments (safe: `#`-comments can't be broken out of).
            let _ = writeln!(buf, "\n# Plugin commands");
            for cmd in &safe_cmds {
                let _ = writeln!(
                    buf,
                    "# plugin: {} -- {}",
                    cmd.name,
                    sanitize_plugin_description(&cmd.description)
                );
            }
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
    /// Generate shell completions for bash, zsh, fish, and powershell
    #[command(subcommand)]
    Completions(CompletionShell),
    /// Interactive REPL for local Soroban contract testing
    Shell(crate::commands::shell::ShellArgs),
    /// Live monitoring (contract events or wallet threshold)
    Monitor(crate::commands::monitor::MonitorArgs),
    /// Interactive CLI tutorials
    #[command(subcommand)]
    Tutorial(crate::commands::tutorial::TutorialCommands),
    /// Performance benchmarking utilities
    #[command(subcommand)]
    Benchmark(crate::commands::benchmark::BenchmarkCommands),
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

    // ── powershell ────────────────────────────────────────────────────────────

    #[test]
    fn powershell_completion_generates_non_empty_output() {
        let out = completion_output(Shell::PowerShell);
        assert!(
            !out.is_empty(),
            "powershell completion output must not be empty"
        );
    }

    #[test]
    fn powershell_completion_registers_argument_completer() {
        let out = completion_output(Shell::PowerShell);
        assert!(
            out.contains("Register-ArgumentCompleter") && out.contains("'starforge'"),
            "powershell completion must register an argument completer for starforge"
        );
    }

    #[test]
    fn powershell_completion_lists_all_subcommands() {
        let out = completion_output(Shell::PowerShell);
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
                "powershell completion missing subcommand '{}'",
                cmd
            );
        }
    }

    // ── regression coverage ───────────────────────────────────────────────────

    const ALL_SHELLS: [Shell; 4] = [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell];

    #[test]
    fn all_shells_include_global_flags() {
        for shell in ALL_SHELLS {
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
        for shell in ALL_SHELLS {
            let out = completion_output(shell);
            assert!(
                out.contains("completions"),
                "{:?} completion must list the completions subcommand",
                shell
            );
        }
    }

    #[test]
    fn completion_shell_maps_to_expected_clap_shell() {
        assert_eq!(CompletionShell::Bash.to_clap_shell(), Shell::Bash);
        assert_eq!(CompletionShell::Zsh.to_clap_shell(), Shell::Zsh);
        assert_eq!(CompletionShell::Fish.to_clap_shell(), Shell::Fish);
        assert_eq!(
            CompletionShell::PowerShell.to_clap_shell(),
            Shell::PowerShell
        );
    }

    // ── nested subcommands / edge-case flags (boundary coverage) ───────────────

    #[test]
    fn deeply_nested_subcommands_are_present() {
        // `wallet create` and `contract deps` exercise subcommand nesting
        // beyond the top level, and `--strict`/`--mnemonic` are flags nested
        // inside a struct-variant subcommand with `requires =` relations.
        for shell in ALL_SHELLS {
            let out = completion_output(shell);
            for needle in ["create", "deps", "strict", "mnemonic"] {
                assert!(
                    out.contains(needle),
                    "{:?} completion missing nested item '{}'",
                    shell,
                    needle
                );
            }
        }
    }

    #[test]
    fn generation_handles_command_with_no_subcommands() {
        // Boundary case: a command struct with no subcommands and no args at
        // all must still produce a script rather than panicking or emitting
        // empty/garbled output.
        for shell in ALL_SHELLS {
            let mut cmd = clap::Command::new("empty");
            let mut buf = Vec::new();
            generate(shell, &mut cmd, "empty", &mut buf);
            assert!(
                !buf.is_empty(),
                "{:?} completion for a subcommand-less command must not be empty",
                shell
            );
        }
    }

    // ── plugin command hardening (security / failure paths) ────────────────────

    #[test]
    fn safe_plugin_names_accept_plain_identifiers() {
        for name in ["deploy-helper", "gas_report", "ns:tool", "a"] {
            assert!(is_safe_plugin_name(name), "'{}' should be accepted", name);
        }
    }

    #[test]
    fn unsafe_plugin_names_are_rejected() {
        for name in [
            "",
            "-leading-dash",
            "has space",
            "quote'here",
            "newline\nhere",
            "semi;colon",
            "$(cmd)",
            "`backtick`",
            &"x".repeat(65),
        ] {
            assert!(
                !is_safe_plugin_name(name),
                "'{}' should be rejected as an unsafe plugin name",
                name
            );
        }
    }

    #[test]
    fn sanitize_plugin_description_strips_control_chars_and_escapes_quotes() {
        let sanitized = sanitize_plugin_description("it's a \x07bell and \nnewline");
        assert!(!sanitized.contains('\x07'));
        assert!(!sanitized.contains('\n'));
        assert!(sanitized.contains("it\\'s"));
    }

    #[test]
    fn malformed_plugin_command_is_dropped_from_generated_scripts() {
        use crate::plugins::registry::RegisteredCommand;

        let cmds = vec![
            RegisteredCommand {
                name: "good-plugin".to_string(),
                description: "a well-behaved plugin".to_string(),
            },
            RegisteredCommand {
                name: "evil'; rm -rf ~ #".to_string(),
                description: "malformed name attempting shell injection".to_string(),
            },
        ];

        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
            let mut buf = Vec::new();
            generate_completion(shell, &mut buf);
            append_plugin_completions(shell, &cmds, &mut buf);
            let out = String::from_utf8(buf).expect("valid utf8");

            assert!(
                out.contains("good-plugin"),
                "{:?} output should still include the well-formed plugin command",
                shell
            );
            assert!(
                !out.contains("rm -rf"),
                "{:?} output must not embed the malformed plugin command verbatim",
                shell
            );
        }
    }

    #[test]
    fn no_plugin_commands_leaves_output_unchanged() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
            let mut buf = Vec::new();
            generate_completion(shell, &mut buf);
            let before = buf.clone();
            append_plugin_completions(shell, &[], &mut buf);
            assert_eq!(
                buf, before,
                "{:?} output should be untouched when there are no plugin commands",
                shell
            );
        }
    }
}
