//! Integration tests for the noun-verb command tree (ADR 0007, issue #936).
//!
//! The tree assertions spawn the built binary so they check exactly what a user
//! sees in `starforge --help`. The deprecation shim is exercised in-process
//! because rewriting argv is the whole compatibility contract.

use std::ffi::OsString;
use std::process::Command;

use starforge::commands::deprecations::{rewrite_argv, AI_LOCAL_VERBS, DEPRECATED_COMMANDS};

/// Command names listed under `Commands:` in `starforge --help`.
fn top_level_commands() -> Vec<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_starforge"))
        .arg("--help")
        .output()
        .expect("run starforge --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Commands:"),
        "--help must list commands, got:\n{stdout}"
    );

    stdout
        .lines()
        .skip_while(|line| !line.trim_end().ends_with("Commands:"))
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let token = line.split_whitespace().next()?;
            if token.starts_with('-') || token == "help" {
                None
            } else {
                Some(token.to_string())
            }
        })
        .collect()
}

/// Help text for one noun, used to assert its verbs are registered.
fn noun_help(noun: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_starforge"))
        .args([noun, "--help"])
        .output()
        .expect("run starforge <noun> --help");
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// The nouns `starforge --help` is expected to show, in help order.
const NOUN_COMMANDS: [&str; 12] = [
    "wallet",
    "contract",
    "deploy",
    "network",
    "template",
    "plugin",
    "ai",
    "config",
    "project",
    "tool",
    "completions",
    "man",
];

/// Issue #936: `--help` must stay within the documented top-level budget, and
/// the surface must be exactly the noun tree (plus the optional `ui` feature).
#[test]
fn top_level_help_is_exactly_the_noun_tree() {
    let commands = top_level_commands();
    assert!(
        commands.len() <= 20,
        "top-level --help shows {} commands ({}); budget is 20",
        commands.len(),
        commands.join(", ")
    );

    let mut expected: Vec<String> = NOUN_COMMANDS.iter().map(|n| n.to_string()).collect();
    if cfg!(feature = "ui") {
        expected.push("ui".to_string());
    }
    assert_eq!(
        commands, expected,
        "unexpected top-level surface; add new commands as a verb of a noun"
    );
}

/// The nouns users are expected to learn, and nothing hidden among them.
#[test]
fn expected_nouns_are_visible() {
    let commands = top_level_commands();
    for noun in NOUN_COMMANDS {
        assert!(
            commands.contains(&noun.to_string()),
            "`{noun}` must be in --help"
        );
    }
}

/// Internal tooling stays registered but out of the visible surface.
#[test]
fn internal_commands_stay_hidden() {
    let commands = top_level_commands();
    for hidden in ["autocomplete", "external", "help"] {
        assert!(
            !commands.contains(&hidden.to_string()),
            "`{hidden}` must not appear in --help"
        );
    }

    // `autocomplete` still works, it is only hidden. Whether it then succeeds
    // depends on the local config dir, so only the parser is asserted here.
    let output = Command::new(env!("CARGO_BIN_EXE_starforge"))
        .args(["autocomplete", "--stats"])
        .output()
        .expect("run starforge autocomplete --stats");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unrecognized subcommand") && !stderr.contains("unexpected argument"),
        "autocomplete must stay a registered subcommand, got: {stderr}"
    );
}

/// Every moved command resolves to a verb under the noun it moved to.
#[test]
fn moved_commands_are_registered_under_their_noun() {
    // One `--help` per noun, then assert all of that noun's verbs appear.
    let mut per_noun: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
    for entry in DEPRECATED_COMMANDS {
        let path: Vec<&str> = entry.new.split(' ').collect();
        if path.len() < 2 {
            // The old command was a leaf and the noun absorbed it, e.g. the
            // former `starforge deploy --wasm …` is now `deploy run`.
            per_noun.entry(path[0]).or_default().push("run");
            continue;
        }
        per_noun.entry(path[0]).or_default().push(path[1]);
    }

    for (noun, verbs) in per_noun {
        let help = noun_help(noun);
        for verb in verbs {
            assert!(
                help.contains(&format!(" {verb}")),
                "`starforge {noun} {verb}` is not listed in `{noun} --help`"
            );
        }
    }
}

#[test]
fn legacy_ai_verbs_resolve_under_ai_local() {
    let help = noun_help("ai");
    assert!(help.contains(" local"), "`ai local` must exist");
    // `ai local <verb>` is three tokens, so the legacy verbs are asserted
    // against the `ai local` help text rather than the `ai` one.
    let local_help = noun_help("ai local");
    for verb in AI_LOCAL_VERBS {
        assert!(
            local_help.contains(&format!(" {verb}")),
            "legacy `ai {verb}` is not a verb of `ai local`"
        );
    }
}

/// The shim rewrites the deprecated spelling and reports the replacement.
#[test]
fn rewrite_argv_rewrites_a_moved_command() {
    let mut argv: Vec<OsString> = ["starforge", "test", "--wasm", "hello.wasm"]
        .iter()
        .map(OsString::from)
        .collect();

    assert!(rewrite_argv(&mut argv), "argv should have been rewritten");
    assert_eq!(
        argv,
        vec![
            OsString::from("starforge"),
            OsString::from("contract"),
            OsString::from("test"),
            OsString::from("--wasm"),
            OsString::from("hello.wasm"),
        ]
    );
}

/// `starforge deploy --wasm …` becomes `starforge deploy run --wasm …`, the
/// case a clap alias cannot express.
#[test]
fn rewrite_argv_handles_deploy_becoming_deploy_run() {
    let mut argv: Vec<OsString> = ["starforge", "deploy", "--wasm", "c.wasm"]
        .iter()
        .map(OsString::from)
        .collect();

    assert!(rewrite_argv(&mut argv));
    assert_eq!(
        argv,
        vec![
            OsString::from("starforge"),
            OsString::from("deploy"),
            OsString::from("run"),
            OsString::from("--wasm"),
            OsString::from("c.wasm"),
        ]
    );
}

/// The legacy flat `ai` verbs are rewritten to `ai local <verb>`.
#[test]
fn rewrite_argv_handles_legacy_ai_verbs() {
    let mut argv: Vec<OsString> = ["starforge", "ai", "pull", "llama3"]
        .iter()
        .map(OsString::from)
        .collect();

    assert!(rewrite_argv(&mut argv));
    assert_eq!(
        argv,
        vec![
            OsString::from("starforge"),
            OsString::from("ai"),
            OsString::from("local"),
            OsString::from("pull"),
            OsString::from("llama3"),
        ]
    );
}

/// Global flags that take a value must not be mistaken for the subcommand.
#[test]
fn rewrite_argv_skips_global_flag_values() {
    let mut argv: Vec<OsString> = ["starforge", "--log-format", "json", "--quiet", "audit", "."]
        .iter()
        .map(OsString::from)
        .collect();

    assert!(rewrite_argv(&mut argv));
    assert_eq!(
        argv,
        vec![
            OsString::from("starforge"),
            OsString::from("--log-format"),
            OsString::from("json"),
            OsString::from("--quiet"),
            OsString::from("contract"),
            OsString::from("audit"),
            OsString::from("."),
        ]
    );
}

/// A current spelling is left untouched, so the shim is a no-op for new code.
#[test]
fn rewrite_argv_leaves_current_commands_alone() {
    for current in [
        vec!["starforge", "contract", "audit", "."],
        vec!["starforge", "deploy", "run", "--wasm", "c.wasm"],
        vec!["starforge", "ai", "local", "pull", "llama3"],
        vec!["starforge", "wallet", "list"],
        vec!["starforge", "--version"],
    ] {
        let mut argv: Vec<OsString> = current.iter().map(OsString::from).collect();
        assert!(!rewrite_argv(&mut argv), "rewrote {current:?}");
        assert_eq!(argv, current.iter().map(OsString::from).collect::<Vec<_>>());
    }
}

/// The table is the single source of truth for the migration doc, so keep it
/// sorted, unique, and pointing at a different path.
#[test]
fn deprecation_table_is_sorted_and_unique() {
    let mut names: Vec<&str> = DEPRECATED_COMMANDS.iter().map(|e| e.old).collect();
    let original = names.clone();
    names.sort_unstable();
    names.dedup();
    assert_eq!(
        original, names,
        "DEPRECATED_COMMANDS must be sorted and free of duplicates"
    );

    for entry in DEPRECATED_COMMANDS {
        assert_ne!(entry.old, entry.new, "`{}` maps to itself", entry.old);
        assert!(!entry.new.is_empty(), "{} has no replacement", entry.old);
        assert!(
            !entry.removed_in.is_empty(),
            "{} needs a removal version",
            entry.old
        );
    }
}
