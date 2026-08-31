//! Static, offline knowledge base powering the AI Contextual Help System.
//!
//! This module is deliberately dependency-free and provides three things:
//!
//!   * [`HELP_REGISTRY`] — a compact, per-command metadata table describing
//!     the most useful StarForge commands. Fields include flags, workflows,
//!     tips, related commands, and prerequisites. New commands can be added
//!     to `Commands` (in `src/main.rs`) without touching this file; this
//!     registry intentionally only covers the commands where rich guidance
//!     adds clear value (≈ top commands the user is most likely to need
//!     help with).
//!
//!   * [`WORKFLOWS`] — multi-command sequences for common end-to-end jobs
//!     (first contract deployments, gas debugging, security audit, …). The
//!     `starforge help --workflow` command lists these and the user can
//!     filter by command name.
//!
//!   * [`ERROR_QUICK_FIXES`] and [`PREREQUISITES`] — tiny tables used by
//!     [`crate::utils::context_help`] for issue prediction and
//!     error-specific troubleshooting. They are kept here so the metadata
//!     can be reviewed and extended without touching the inference engine.
//!
//! All data is `&'static` so it can be embedded as consts and indexed in
//! O(log n) when the engine needs to look things up by command name.
//!
//! [`HELP_REGISTRY`]: crate::utils::help_metadata::HELP_REGISTRY
//! [`WORKFLOWS`]: crate::utils::help_metadata::WORKFLOWS
//! [`ERROR_QUICK_FIXES`]: crate::utils::help_metadata::ERROR_QUICK_FIXES
//! [`PREREQUISITES`]: crate::utils::help_metadata::PREREQUISITES

// ── Command metadata ──────────────────────────────────────────────────────────

/// One flag/option attached to a command, used for the `help <cmd>` summary.
#[derive(Debug, Clone, Copy)]
pub struct FlagHelp {
    /// Short flag, e.g. `"--wasm"` or `"-q"`.
    pub flag: &'static str,
    /// One-line purpose, e.g. `"Path to compiled WASM binary"`.
    pub purpose: &'static str,
}

/// One short example invocation shown alongside command help.
#[derive(Debug, Clone, Copy)]
pub struct ExampleHelp {
    /// The user-facing command line.
    pub command: &'static str,
    /// What this example demonstrates.
    pub description: &'static str,
}

/// Static metadata for a command that the contextual help engine renders.
#[derive(Debug, Clone, Copy)]
pub struct CommandHelpInfo {
    /// Stable command identifier (matching `Commands` enum in `main.rs`),
    /// e.g. `"deploy"`. Used as the lookup key.
    pub name: &'static str,
    /// One-line human description (shown in listings and `help <cmd>` header).
    pub summary: &'static str,
    /// Definition list of the most useful flags. Kept short on purpose —
    /// exhaustive flag docs live in `starforge <cmd> --help`.
    pub flags: &'static [FlagHelp],
    /// Concrete examples a beginner can copy-paste.
    pub examples: &'static [ExampleHelp],
    /// Workflow names (see [`WORKFLOWS`]) the user can follow for this
    /// command; they will be expanded when `starforge help <cmd> --workflow`
    /// is run.
    pub workflows: &'static [&'static str],
    /// Best-practice tips the engine surfaces when relevant (security, gas,
    /// TTL, etc.).
    pub tips: &'static [&'static str],
    /// Other commands the user is likely to chain with this one, used by
    /// "related commands" suggestions.
    pub related: &'static [&'static str],
}

pub const HELP_REGISTRY: &[CommandHelpInfo] = &[
    CommandHelpInfo {
        name: "deploy",
        summary: "Deploy a compiled Soroban contract (.wasm) to a network",
        flags: &[
            FlagHelp { flag: "--wasm <path>", purpose: "Path to the compiled WASM binary" },
            FlagHelp { flag: "--network <name>", purpose: "Target network (testnet/mainnet/...)" },
            FlagHelp { flag: "--wallet <name>", purpose: "Source wallet used to fund deployment" },
            FlagHelp { flag: "--salt <hex>", purpose: "Deterministic deployment salt (optional)" },
            FlagHelp { flag: "--optimize", purpose: "Optimize WASM before deployment" },
        ],
        examples: &[
            ExampleHelp {
                command: "starforge deploy --wasm target/wasm32-unknown-unknown/release/hello.wasm",
                description: "Deploy a freshly built contract to the active network",
            },
            ExampleHelp {
                command: "starforge deploy --wasm ./build/c.wasm --network testnet --wallet deployer",
                description: "Target testnet with a specific deployer wallet",
            },
            ExampleHelp {
                command: "cargo build --target wasm32-unknown-unknown --release && starforge deploy --wasm target/wasm32-unknown-unknown/release/hello.wasm",
                description: "Rebuild first, then deploy the rebuilt wasm — avoids shipping a stale binary",
            },
        ],
        workflows: &["first-contract", "upgrade-existing-contract"],
        tips: &[
            "Always run `starforge test <wasm>` before deploying to mainnet — mainnet deployments are irreversible.",
            "Use `--optimize` for production deployments to reduce WASM size and gas cost.",
            "Capture the contract ID printed on success — you'll need it for `contract invoke` and `inspect`.",
        ],
        related: &["contract", "wallet", "network", "test", "deployments"],
    },
    CommandHelpInfo {
        name: "wallet",
        summary: "Manage test wallets (create, list, show, fund, remove)",
        flags: &[
            FlagHelp { flag: "--fund", purpose: "Fund a new wallet via Friendbot (testnet only)" },
            FlagHelp { flag: "--encrypt", purpose: "Encrypt the saved secret key with a password" },
            FlagHelp { flag: "--network <name>", purpose: "Network the wallet lives on" },
        ],
        examples: &[
            ExampleHelp {
                command: "starforge wallet create deployer --fund",
                description: "Create and fund a testnet deployer wallet in one step",
            },
            ExampleHelp {
                command: "starforge wallet show deployer",
                description: "Show address, balance, and network of a saved wallet",
            },
        ],
        workflows: &["first-contract"],
        tips: &[
            "Never reuse real mainnet secret keys with this CLI — these wallets are intended for development.",
            "Use `--encrypt` if your machine is shared, otherwise the secret key is stored in plaintext.",
            "Funded testnet wallets can be re-funded any time via `wallet fund <name>`.",
        ],
        related: &["network", "deploy", "audit"],
    },
    CommandHelpInfo {
        name: "contract",
        summary: "Inspect or invoke an already-deployed contract",
        flags: &[
            FlagHelp { flag: "--id <contract-id>", purpose: "Contract ID to target (starts with C)" },
            FlagHelp { flag: "--function <name>", purpose: "Method name to invoke" },
            FlagHelp { flag: "--args <json>", purpose: "Positional/named args as a JSON array" },
            FlagHelp { flag: "--wallet <name>", purpose: "Signing wallet for the invocation" },
            FlagHelp { flag: "--network <name>", purpose: "Network to use for RPC calls" },
        ],
        examples: &[
            ExampleHelp {
                command: "starforge contract inspect --id CABC...XYZ",
                description: "List the public methods and types of a deployed contract",
            },
            ExampleHelp {
                command: "starforge contract invoke --id C... --function balance --args '[\"G...\"]'",
                description: "Call a read-only method on the contract",
            },
        ],
        workflows: &["first-contract"],
        tips: &[
            "Inspect before invoking: the ABI shows the exact argument order/types so invocation succeeds on the first try.",
            "If you get a type mismatch, regenerate bindings: `starforge contract generate-bindings`.",
            "For write calls, the signing wallet must match the `require_auth` check in the contract.",
        ],
        related: &["deploy", "wallet", "debug", "inspect", "audit"],
    },
    CommandHelpInfo {
        name: "network",
        summary: "Show, switch, or add a Stellar/Soroban network",
        flags: &[
            FlagHelp { flag: "--switch <name>", purpose: "Set the active network for subsequent commands" },
            FlagHelp { flag: "--add <name> --horizon-url <url>", purpose: "Add a custom network entry" },
            FlagHelp { flag: "--remove <name>", purpose: "Remove a custom network (reserved names are protected)" },
        ],
        examples: &[
            ExampleHelp {
                command: "starforge network show",
                description: "Display the active network and its endpoints",
            },
            ExampleHelp {
                command: "starforge network switch testnet",
                description: "Switch to the testnet for free, dev-friendly deployments",
            },
        ],
        workflows: &["first-contract"],
        tips: &[
            "Switch to testnet when experimenting — Friendbot is free there but unavailable on mainnet.",
            "Built-in networks (testnet, mainnet, docker-testnet) cannot be removed.",
            "`docker-testnet` points at a local container; `starforge node start` brings it up.",
        ],
        related: &["node", "deploy", "wallet"],
    },
    CommandHelpInfo {
        name: "test",
        summary: "Run contract unit tests with Soroban test utilities",
        flags: &[
            FlagHelp { flag: "--wasm <path>", purpose: "WASM file to test" },
            FlagHelp { flag: "--name <filter>", purpose: "Run only tests whose name matches" },
        ],
        examples: &[
            ExampleHelp { command: "starforge test --wasm target/wasm32-unknown-unknown/release/hello.wasm",
                description: "Run all unit tests embedded in the WASM" },
        ],
        workflows: &["first-contract"],
        tips: &[
            "Run `cargo test` in the contract crate before invoking `starforge test` for fast feedback.",
            "Use `--name` to scope a run to a single failing test while debugging.",
        ],
        related: &["contract", "audit", "gas"],
    },
    CommandHelpInfo {
        name: "gas",
        summary: "Estimate, analyse, and optimise gas usage for a contract invocation",
        flags: &[
            FlagHelp { flag: "--wasm <path>", purpose: "WASM to measure" },
            FlagHelp { flag: "--function <name>", purpose: "Target method for the estimate" },
            FlagHelp { flag: "--args <json>", purpose: "Arguments to the method" },
            FlagHelp { flag: "--report", purpose: "Produce a human-readable gas usage report" },
        ],
        examples: &[
            ExampleHelp { command: "starforge gas estimate --wasm app.wasm --function transfer",
                description: "Estimate the gas cost of a single call" },
        ],
        workflows: &["gas-debugging"],
        tips: &[
            "Cold storage reads cost significantly more than warm reads — repeated access within one call is cheaper.",
            "`require_auth` adds a fixed cost; batch operations behind one auth call save gas.",
            "Persistent storage must have its TTL extended periodically; forgotten TTL is a common gas cliff.",
        ],
        related: &["contract", "audit", "test"],
    },
    CommandHelpInfo {
        name: "audit",
        summary: "Static security analysis for a Soroban contract",
        flags: &[
            FlagHelp { flag: "--path <wasm-or-src>", purpose: "Path to the WASM or contract source" },
            FlagHelp { flag: "--deep", purpose: "Run additional deep checks (slower, more findings)" },
        ],
        examples: &[
            ExampleHelp { command: "starforge audit ./hello",
                description: "Audit the contract source directory" },
        ],
        workflows: &["security-review"],
        tips: &[
            "Run audits before every mainnet deploy — even a small change can introduce regressions.",
            "Pair `audit` with `ai-audit` for an LLM-assisted explanation of findings.",
        ],
        related: &["ai-audit", "deploy", "test"],
    },
    CommandHelpInfo {
        name: "ai-debug",
        summary: "AI-assisted error analysis with root-cause hints",
        flags: &[
            FlagHelp { flag: "--analyse <error>", purpose: "Analyse an error message and return findings" },
            FlagHelp { flag: "--explain <category>", purpose: "Explain a known error category in detail" },
        ],
        examples: &[
            ExampleHelp { command: "starforge ai-debug analyse \"require_auth failed for address\"",
                description: "Identify why an auth check failed" },
        ],
        workflows: &["troubleshoot-error"],
        tips: &[
            "Quote the error message so the analyser sees the exact text — backticks and special characters matter.",
            "Pair with `starforge debug start` to reproduce the failing call interactively.",
        ],
        related: &["debug", "audit", "contract"],
    },
    CommandHelpInfo {
        name: "tutorial",
        summary: "Interactive, step-by-step CLI tutorials",
        flags: &[
            FlagHelp { flag: "--list", purpose: "Show every installed tutorial" },
            FlagHelp { flag: "--start <slug>", purpose: "Start a tutorial by slug (e.g. hello-world)" },
            FlagHelp { flag: "--next", purpose: "Mark the current step done and advance" },
            FlagHelp { flag: "--status", purpose: "Show overall tutorial progress" },
        ],
        examples: &[
            ExampleHelp { command: "starforge tutorial start hello-world",
                description: "Walk through your first end-to-end flow" },
        ],
        workflows: &["first-contract"],
        tips: &[
            "Run `starforge tutorial list` to see every available tutorial — they're the fastest way to learn.",
            "`starforge tutorial status` shows where you paused; `next` resumes.",
        ],
        related: &["new", "deploy", "wallet"],
    },
    CommandHelpInfo {
        name: "template",
        summary: "Search, install, and publish community Soroban templates",
        flags: &[
            FlagHelp { flag: "--search <query>", purpose: "Search the marketplace by name/tag" },
            FlagHelp { flag: "--install <slug>", purpose: "Fetch a template into your project" },
            FlagHelp { flag: "--publish", purpose: "Publish a local template to the marketplace" },
        ],
        examples: &[
            ExampleHelp { command: "starforge template search token",
                description: "Find templates related to token contracts" },
        ],
        workflows: &["first-contract"],
        tips: &[
            "Skim `info <slug>` before installing to see the trust badges and license.",
            "Always review the README of an installed template before building on top of it.",
        ],
        related: &["new", "registry", "audit"],
    },
];

// ── Workflows ─────────────────────────────────────────────────────────────────

/// A reusable, multi-command sequence that solves a common end-to-end job.
#[derive(Debug, Clone, Copy)]
pub struct Workflow {
    /// Short slug used by `--workflow <slug>` and in suggestions.
    pub name: &'static str,
    /// Human description of what this flow achieves.
    pub description: &'static str,
    /// Ordered commands the user can run, top to bottom.
    pub steps: &'static [&'static str],
    /// How long this typically takes (rough order: minute / tens of minutes / hours).
    pub approx_duration: &'static str,
}

pub const WORKFLOWS: &[Workflow] = &[
    Workflow {
        name: "first-contract",
        description: "Build, fund, deploy, and invoke your first Soroban contract end-to-end",
        approx_duration: "5–10 minutes",
        steps: &[
            "starforge tutorial start hello-world          # optional guided intro",
            "starforge wallet create deployer --fund       # create + fund a testnet wallet",
            "starforge network show                        # confirm active network is testnet",
            "starforge new contract hello                  # scaffold a contract (or use your own)",
            "cd hello && stellar contract build            # build the WASM",
            "starforge deploy --wasm target/wasm32-unknown-unknown/release/hello.wasm --wallet deployer",
            "starforge contract invoke --id <printed-id> --function hello --args '[]'",
        ],
    },
    Workflow {
        name: "gas-debugging",
        description: "Profile and reduce the gas cost of a specific contract invocation",
        approx_duration: "10–20 minutes",
        steps: &[
            "starforge gas estimate --wasm app.wasm --function <fn> --args '[...]'   # baseline",
            "starforge gas report --wasm app.wasm                                   # full breakdown",
            "starforge audit --deep <path>                                          # look for TTL/auth issues",
            "# Edit contract to reduce storage ops, then rebuild",
            "stellar contract build && starforge gas estimate --wasm <new>.wasm --function <fn> --args '[...]'",
        ],
    },
    Workflow {
        name: "security-review",
        description: "Run static and AI-assisted security checks before a mainnet deployment",
        approx_duration: "5–15 minutes",
        steps: &[
            "starforge test <path>              # all unit tests must pass first",
            "starforge audit <path>             # static analysis (fast, local)",
            "starforge ai-audit <path>          # LLM-assisted deeper review",
            "starforge ai-debug analyse \"<if applicable>\"  # triage any leftover findings",
            "starforge deploy --optimize --wasm <path> --network mainnet",
        ],
    },
    Workflow {
        name: "troubleshoot-error",
        description: "Track down an error from a failed command and form a fix plan",
        approx_duration: "5 minutes",
        steps: &[
            "starforge help --why                       # explain your last error",
            "starforge ai-debug analyse \"<error>\"      # pattern-match the message",
            "starforge debug start --wasm <path>        # step through the failure",
            "starforge audit <path>                     # look for static issues",
            "# Apply the fix suggested in the ai-debug report and retry",
        ],
    },
    Workflow {
        name: "upgrade-existing-contract",
        description: "Propose, approve, and execute a contract upgrade end-to-end",
        approx_duration: "20–40 minutes",
        steps: &[
            "starforge audit <new.wasm>                           # sanity check the new build",
            "starforge upgrade propose --id <id> --new-wasm <new.wasm>",
            "starforge upgrade approve --id <id> --wallet <name>  # collect approvals",
            "starforge upgrade execute --id <id> --wallet <name>",
            "starforge deployments list                             # confirm the change",
            "starforge upgrade rollback --id <id> --to <prev-hash>  # if anything went wrong",
        ],
    },
];

// ── Issue prediction ──────────────────────────────────────────────────────────

/// Tracks a pattern that should appear in the user's recent command history
/// before they run `command`. Used to predict likely problems.
#[derive(Debug, Clone, Copy)]
pub struct Prerequisite {
    /// Substring that must appear in at least one recent history entry.
    pub pattern: &'static str,
    /// Short user-facing warning if the pattern is missing.
    pub warning: &'static str,
    /// Optional tip command (e.g. `"wallet fund <name>"`).
    pub remedy: &'static str,
}

/// Set of prerequisites grouped by the command they apply to.
#[derive(Debug, Clone, Copy)]
pub struct PrerequisiteSet {
    /// Target command being checked (e.g. `"deploy"`).
    pub command: &'static str,
    /// What the user must have done before this command usually works.
    pub needs: &'static [Prerequisite],
}

pub const PREREQUISITES: &[PrerequisiteSet] = &[
    PrerequisiteSet {
        command: "deploy",
        needs: &[
            Prerequisite {
                pattern: "wallet create",
                warning: "No wallets saved yet — create one first or `--wallet` will fail.",
                remedy: "starforge wallet create deployer",
            },
            Prerequisite {
                pattern: "wallet fund",
                warning: "Your wallet may not be funded on the active network — deployment can fail with insufficient balance.",
                remedy: "starforge wallet fund <name>",
            },
        ],
    },
    PrerequisiteSet {
        command: "contract",
        needs: &[
            Prerequisite {
                pattern: "deploy",
                warning: "If you're invoking a contract, it usually needs to be deployed first.",
                remedy: "starforge deploy --wasm <path>",
            },
        ],
    },
    PrerequisiteSet {
        command: "tutorial",
        needs: &[
            Prerequisite {
                pattern: "tutorial list",
                warning: "If `tutorial list` is empty, no tutorials are installed; grab one from the registry first.",
                remedy: "starforge tutorial list",
            },
        ],
    },
];

// ── Error quick fixes ─────────────────────────────────────────────────────────

/// One-line, command-line friendly remediation for a common error.
/// Used by the proactive help hook in `main.rs` and by
/// `context_help::troubleshoot` to extend the existing command hints.
#[derive(Debug, Clone, Copy)]
pub struct ErrorQuickFix {
    /// Lower-case substrings that, when found in an error message, trigger
    /// this fix. The first matching entry wins (entries are checked in order).
    pub keywords: &'static [&'static str],
    /// Category slug used for filtering — matches the help settings names.
    pub category: &'static str,
    /// One-line action the user can take next.
    pub action: &'static str,
    /// Optional follow-up command (printed as a "→ run …" tip).
    pub follow_up: &'static str,
}

/// Ordered list of error quick fixes. Order matters because the first match
/// wins; more specific phrases come first.
pub const ERROR_QUICK_FIXES: &[ErrorQuickFix] = &[
    ErrorQuickFix {
        keywords: &["friendbot", "faucet"],
        category: "funding",
        action: "Friendbot only works on testnet — switch networks or fund manually on mainnet.",
        follow_up: "starforge network switch testnet && starforge wallet fund <name>",
    },
    ErrorQuickFix {
        keywords: &["ttl", "expired", "archived", "state archival"],
        category: "storage-ttl",
        action: "A storage entry's TTL expired. Extend it on the next read/write.",
        follow_up: "env.storage().persistent().extend_ttl(&key, MIN_TTL, MAX_TTL);",
    },
    ErrorQuickFix {
        keywords: &["require_auth", "unauthorized", "auth failed"],
        category: "auth",
        action: "Authorization failed — sign with the wallet the contract expects.",
        follow_up: "starforge contract invoke --wallet <matching-wallet> --id <id>",
    },
    ErrorQuickFix {
        keywords: &["attempt to add with overflow", "attempt to multiply with overflow", "overflow"],
        category: "arithmetic",
        action: "Arithmetic overflow. Use checked_add/checked_mul and return a contract error.",
        follow_up: "amount.checked_add(fee).ok_or(Error::Overflow)?",
    },
    ErrorQuickFix {
        keywords: &["panic", "panicked", "called `result::unwrap`", "called `option::unwrap`"],
        category: "panic",
        action: "Contract panicked (commonly from `.unwrap()` or an assertion). Replace with explicit error handling.",
        follow_up: "starforge debug start --wasm <path>",
    },
    ErrorQuickFix {
        keywords: &["invalid wasm", "wasm"],
        category: "wasm",
        action: "WASM binary is invalid or stale. Rebuild with `stellar contract build`.",
        follow_up: "starforge deploy --wasm <rebuilt-path>",
    },
    ErrorQuickFix {
        keywords: &["contract not found", "ledger entry not found", "does not exist"],
        category: "network-contract",
        action: "Contract ID not found on the active network. Verify you're on the right network.",
        follow_up: "starforge network show && starforge deployments list",
    },
    ErrorQuickFix {
        keywords: &["missing key", "key not found", "no entry", "missing storage"],
        category: "storage-missing",
        action: "Storage entry hasn't been written. Call `initialize()` (or equivalent) first.",
        follow_up: "starforge contract invoke --id <id> --function initialize --args '[...]'",
    },
    ErrorQuickFix {
        keywords: &["insufficient", "balance", "insufficient funds"],
        category: "balance",
        action: "Sender has insufficient balance for the transfer or fee.",
        follow_up: "starforge wallet fund <name>",
    },
    ErrorQuickFix {
        keywords: &["test", "assertion", "left =", "right =", "expected"],
        category: "test-failure",
        action: "Test assertion failed. Run with `--nocapture` to see the diverging values.",
        follow_up: "cargo test -- --nocapture",
    },
    ErrorQuickFix {
        keywords: &["connection", "network", "timeout", "unreachable"],
        category: "connectivity",
        action: "Network/HTTP timeout. Check connectivity or set HTTPS_PROXY.",
        follow_up: "starforge network show",
    },
    ErrorQuickFix {
        keywords: &["permission denied", "access denied"],
        category: "permissions",
        action: "File or directory permission denied. Check the file mode or rerun with appropriate privileges.",
        follow_up: "ls -la <path>",
    },
];

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lookup_finds_well_known_commands() {
        for cmd in [
            "deploy", "wallet", "contract", "network", "gas", "audit", "ai-debug", "test",
            "tutorial", "template",
        ] {
            assert!(
                HELP_REGISTRY.iter().any(|c| c.name == cmd),
                "registry missing '{cmd}'"
            );
        }
    }

    #[test]
    fn registry_commands_have_non_empty_summary() {
        for cmd in HELP_REGISTRY {
            assert!(!cmd.summary.is_empty(), "{} has empty summary", cmd.name);
            assert!(!cmd.summary.trim().is_empty());
        }
    }

    #[test]
    fn registry_flags_have_meaningful_purpose() {
        // `flags` documents both real `--flag` options and subcommand/
        // positional usage forms (e.g. "switch <name>", "list"), so entries
        // are not required to start with '-'; only a non-empty purpose is.
        for cmd in HELP_REGISTRY {
            for flag in cmd.flags {
                assert!(
                    !flag.flag.trim().is_empty(),
                    "{} has an empty flag entry",
                    cmd.name
                );
                assert!(
                    !flag.purpose.is_empty(),
                    "{}.{} has empty purpose",
                    cmd.name,
                    flag.flag
                );
            }
        }
    }

    #[test]
    fn workflows_have_at_least_two_steps() {
        for wf in WORKFLOWS {
            assert!(wf.steps.len() >= 2, "workflow '{}' too short", wf.name);
            assert!(
                !wf.description.is_empty(),
                "workflow '{}' missing description",
                wf.name
            );
        }
    }

    #[test]
    fn workflows_have_unique_names() {
        let mut seen: Vec<&str> = WORKFLOWS.iter().map(|w| w.name).collect();
        seen.sort();
        let original_len = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), original_len, "duplicate workflow names found");
    }

    #[test]
    fn error_fixes_cover_common_patterns() {
        let required_anywhere = ["require_auth", "overflow", "wasm", "ttl"];
        for kw in required_anywhere {
            assert!(
                ERROR_QUICK_FIXES
                    .iter()
                    .any(|f| f.keywords.iter().any(|k| k.contains(kw))),
                "no quick-fix covers keyword '{kw}'"
            );
        }
    }

    #[test]
    fn error_fixes_have_non_empty_fields() {
        for fix in ERROR_QUICK_FIXES {
            assert!(!fix.keywords.is_empty(), "empty keywords");
            assert!(!fix.category.is_empty(), "empty category");
            assert!(!fix.action.is_empty(), "empty action");
        }
    }

    #[test]
    fn prerequisites_are_well_formed() {
        for set in PREREQUISITES {
            assert!(!set.command.is_empty(), "prereq set has empty command");
            assert!(!set.needs.is_empty(), "prereq set {} is empty", set.command);
            for need in set.needs {
                assert!(
                    !need.pattern.is_empty(),
                    "{} has empty pattern",
                    set.command
                );
                assert!(
                    !need.warning.is_empty(),
                    "{} has empty warning",
                    set.command
                );
            }
        }
    }

    #[test]
    fn lookup_helper_returns_command() {
        let cmd = HELP_REGISTRY
            .iter()
            .find(|c| c.name == "deploy")
            .expect("deploy");
        assert!(cmd.examples.len() >= 1);
        assert!(cmd.workflows.contains(&"first-contract"));
    }
}
