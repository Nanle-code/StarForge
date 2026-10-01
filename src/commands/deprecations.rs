//! Deprecated top-level command names and their noun-verb replacements.
//!
//! ADR 0007 consolidated 86 top-level commands into 12 nouns. Every command
//! that moved keeps working for one minor release: before clap parses argv,
//! [`DEPRECATED_COMMANDS`] is consulted and a leading deprecated token is
//! rewritten to its new path, with a deprecation warning on stderr.
//!
//! This table is the single source of truth for the migration. The rendered
//! migration table in `docs/CLI_COMMAND_TREE.md` mirrors it, and
//! `tests/cli_command_tree.rs` asserts both that the top-level help stays
//! within budget and that a legacy invocation still succeeds with a warning.
//!
//! Removal: delete this module and the call to [`rewrite_argv`] in
//! `src/main.rs` when the aliases are dropped.
//!
//! [`rewrite_argv`]: crate::commands::deprecations::rewrite_argv

/// A top-level command that moved under a noun, and the path that replaces it.
pub struct DeprecatedCommand {
    /// The old top-level token, e.g. `ai-debug`.
    pub old: &'static str,
    /// The replacement path, space separated, e.g. `ai debug`.
    pub new: &'static str,
    /// The minor release in which the old spelling stops working.
    pub removed_in: &'static str,
}

/// Global options that consume the following argv token as their value.
///
/// Used to find the position of the subcommand token: everything before it is
/// a flag, and these three take a separate value token.
const VALUE_GLOBAL_FLAGS: &[&str] = &["--log-format", "--log-dir", "--correlation-id"];

/// Legacy `starforge ai <verb>` invocations that now live under `ai local`.
///
/// The top-level `ai` name itself is unchanged, so the shim has to look one
/// token further to preserve the old flat form.
pub const AI_LOCAL_VERBS: &[&str] = &[
    "status",
    "models",
    "pull",
    "ask",
    "audit",
    "explain",
    "test",
    "optimise",
    "profile",
    "compare-profiles",
    "patterns",
    "library",
    "pattern-feedback",
    "cache",
    "analytics",
    "offline",
];

/// Every moved top-level command, sorted by the old name.
pub const DEPRECATED_COMMANDS: &[DeprecatedCommand] = &[
    DeprecatedCommand {
        old: "advanced-perf",
        new: "contract profile",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai",
        new: "ai local",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-accessibility",
        new: "ai accessibility",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-audit",
        new: "ai security-audit",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-contract-suggest",
        new: "ai suggest",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-debug",
        new: "ai debug",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-deployment-test",
        new: "ai deploy-test",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-doc-qa",
        new: "ai docs",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-feedback",
        new: "ai feedback",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-ide",
        new: "ai ide",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-navigate",
        new: "ai navigate",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-plan",
        new: "ai plan",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-profile",
        new: "ai profiling",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-property-test",
        new: "ai property-test",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-quality-gate",
        new: "ai gate",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-recommend",
        new: "ai recommend",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-route",
        new: "ai route",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-search",
        new: "ai search",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-security-training",
        new: "ai training",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-telemetry",
        new: "ai telemetry",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-test",
        new: "ai tests",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "ai-test-maintain",
        new: "ai test-maintain",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "analytics",
        new: "deploy analytics",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "approval",
        new: "deploy approval",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "audit",
        new: "contract audit",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "backup",
        new: "deploy backup",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "benchmark",
        new: "contract benchmark",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "bug-report",
        new: "tool bug-report",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "collab",
        new: "project collab",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "complete",
        new: "contract complete",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "contract-monitor",
        new: "contract health",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "cost",
        new: "deploy cost",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "debug",
        new: "contract debug",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "deploy",
        new: "deploy run",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "deployments",
        new: "deploy history",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "diagnostics",
        new: "wallet diagnostics",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "docs",
        new: "contract docs",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "environment",
        new: "deploy env",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "explain",
        new: "contract explain",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "feature-flags",
        new: "config flags",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "gas",
        new: "contract gas",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "generate",
        new: "contract generate",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "governance",
        new: "contract governance",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "help",
        new: "ai help",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "info",
        new: "config info",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "inspect",
        new: "contract storage",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "lint",
        new: "contract lint",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "migrate",
        new: "contract migrate",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "monitor",
        new: "contract monitor",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "multisig",
        new: "wallet multisig",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "mutate",
        new: "contract mutate",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "new",
        new: "project new",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "nl",
        new: "tool nl",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "node",
        new: "network node",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "optimize",
        new: "contract optimize",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "orchestrate",
        new: "deploy orchestrate",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "perf",
        new: "contract metrics",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "pipeline",
        new: "deploy pipeline",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "pr",
        new: "tool pr",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "privacy",
        new: "config privacy",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "prompts",
        new: "ai prompts",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "registry",
        new: "template registry",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "schedule",
        new: "deploy schedule",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "security",
        new: "contract security",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "sep10",
        new: "wallet auth",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "shell",
        new: "contract repl",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "simulate",
        new: "network simulate",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "snapshot",
        new: "network snapshot",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "telemetry",
        new: "config telemetry",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "template-vcs",
        new: "template vcs",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "test",
        new: "contract test",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "tutorial",
        new: "tool tutorial",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "tx",
        new: "wallet tx",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "upgrade",
        new: "contract upgrade",
        removed_in: "0.2.0",
    },
    DeprecatedCommand {
        old: "verify",
        new: "contract verify",
        removed_in: "0.2.0",
    },
];

/// Index of the first token in `args` that is not a global flag or flag value.
///
/// `args` is a full argv vector, so `args[0]` is the executable path. Any
/// global option that takes a value is skipped together with its value, so the
/// token returned is always the subcommand clap is about to see.
fn subcommand_index(args: &[std::ffi::OsString]) -> Option<usize> {
    let mut idx = 1;
    while idx < args.len() {
        let token = args[idx].to_str().unwrap_or_default();
        if !token.starts_with('-') {
            return Some(idx);
        }
        if VALUE_GLOBAL_FLAGS.contains(&token) {
            idx += 2;
        } else {
            idx += 1;
        }
    }
    None
}

/// Rewrite deprecated invocations in place, printing a warning for each.
///
/// Returns `true` when argv was modified. Warnings go to stderr so that
/// `--json` output, completion scripts and pipes are unaffected.
pub fn rewrite_argv(args: &mut Vec<std::ffi::OsString>) -> bool {
    let Some(idx) = subcommand_index(args) else {
        return false;
    };
    let Some(token) = args[idx].to_str().map(str::to_owned) else {
        return false;
    };

    if let Some(entry) = DEPRECATED_COMMANDS.iter().find(|entry| entry.old == token) {
        let replacement: Vec<std::ffi::OsString> =
            entry.new.split(' ').map(std::ffi::OsString::from).collect();
        warn(entry.old, entry.new, entry.removed_in);
        args.splice(idx..=idx, replacement);
        return true;
    }

    // `ai` itself keeps its name, so the legacy flat verbs are handled here:
    // `starforge ai pull MODEL` becomes `starforge ai local pull MODEL`.
    if token == "ai" {
        let verb = args.get(idx + 1).and_then(|v| v.to_str());
        if let Some(verb) = verb.filter(|v| AI_LOCAL_VERBS.contains(v)) {
            warn(
                &format!("ai {verb}"),
                &format!("ai local {verb}"),
                DEPRECATED_COMMANDS
                    .iter()
                    .find(|entry| entry.old == "ai")
                    .map(|entry| entry.removed_in)
                    .unwrap_or("0.2.0"),
            );
            args.insert(idx + 1, std::ffi::OsString::from("local"));
            return true;
        }
    }

    false
}

/// Emit the deprecation warning for a single rewritten invocation.
fn warn(old: &str, new: &str, removed_in: &str) {
    eprintln!(
        "warning: `starforge {old}` is deprecated and will stop working in starforge {removed_in}; \
         use `starforge {new}` instead"
    );
}
