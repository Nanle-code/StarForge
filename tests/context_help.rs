//! Integration tests for the AI Contextual Help System.
//!
//! These exercise the public surface assembled across `context_help`,
//! `help_metadata` and `history` to make sure the components
//! interlock correctly. They are kept dependency-light (no network, no
//! filesystem fixtures beyond `TempDir`).

use chrono::{Duration, Utc};
use starforge::utils::{
    context_help, help_metadata,
    history::{load_history, save_history, HistoryEntry},
};
use tempfile::TempDir;

fn entry(command: &str, count: u32, days_ago: i64) -> HistoryEntry {
    let last_used = Utc::now() - Duration::days(days_ago);
    HistoryEntry {
        command: command.to_string(),
        timestamp: last_used,
        count: count as usize,
        last_used,
    }
}

#[test]
fn help_engine_handles_known_command() {
    let ctx = context_help::HelpContext {
        command: "deploy",
        ..Default::default()
    };
    let help = context_help::generate_help(&ctx);
    assert!(help.description.contains("Deploy"));
    assert!(!help.flags_and_examples.is_empty());
    assert!(help.flags_and_examples.iter().any(|s| s.contains("--wasm")));
    assert!(help
        .workflow_suggestions
        .iter()
        .any(|s| s.contains("first-contract")));
    assert!(!help.best_practice_tips.is_empty());
    assert!(help.related_commands.contains(&"contract".to_string()));
}

#[test]
fn help_engine_handles_unknown_command_with_fallback() {
    let ctx = context_help::HelpContext {
        command: "totally-not-a-command",
        ..Default::default()
    };
    let help = context_help::generate_help(&ctx);
    assert!(help.description.contains("No dedicated help"));
    assert!(help.workflow_suggestions.is_empty());
    assert!(help.flags_and_examples.is_empty());
    let _total = help.total_items();
}

#[test]
fn predicted_issues_block_first_deploy_without_wallet_history() {
    let warnings = context_help::predict_issues("deploy", &[]);
    assert_eq!(
        warnings.len(),
        2,
        "expected wallet create + fund warnings, got {:?}",
        warnings
    );
    assert!(warnings.iter().any(|w| w.contains("wallet create")));
    assert!(warnings.iter().any(|w| w.contains("funded")));
}

#[test]
fn predicted_issues_satisfied_with_history() {
    let hist = vec![
        entry("wallet create deployer", 1, 0),
        entry("wallet fund deployer", 1, 0),
    ];
    let warnings = context_help::predict_issues("deploy", &hist);
    assert!(
        warnings.is_empty(),
        "got unexpected warnings: {:?}",
        warnings
    );
}

#[test]
fn expertise_level_adapts_to_recent_run_count() {
    let beginner = context_help::expertise_level("deploy", &[]);
    assert_eq!(beginner, context_help::Expertise::Beginner);

    let medium = context_help::expertise_level(
        "deploy",
        &[
            entry("deploy --wasm a.wasm", 3, 0),
            entry("deploy --wasm b.wasm", 3, 0),
        ],
    );
    assert_eq!(medium, context_help::Expertise::Intermediate);

    let advanced = context_help::expertise_level(
        "deploy",
        &[
            entry("deploy --wasm a.wasm", 6, 0),
            entry("deploy --wasm b.wasm", 6, 0),
            entry("deploy --wasm c.wasm", 5, 1),
            entry("deploy --wasm d.wasm", 5, 2),
            entry("deploy --wasm e.wasm", 4, 3),
        ],
    );
    assert_eq!(advanced, context_help::Expertise::Advanced);
}

#[test]
fn troubleshoot_returns_actionable_step_for_common_errors() {
    let auth_steps = context_help::troubleshoot("require_auth failed for caller");
    assert!(auth_steps.iter().any(|s| s.contains("Authorization")));

    let overflow = context_help::troubleshoot("attempt to multiply with overflow");
    assert!(overflow
        .iter()
        .any(|s| s.to_lowercase().contains("arithmetic")));

    let wasm = context_help::troubleshoot("invalid wasm magic header");
    assert!(wasm.iter().any(|s| s.to_lowercase().contains("wasm")));

    let empty = context_help::troubleshoot("kjsdfhkjsdf");
    assert_eq!(empty.len(), 1);
    assert!(empty[0].contains("No specific pattern"));
}

#[test]
fn troubleshoot_merging_does_not_duplicate_existing_hints() {
    let mut existing = vec!["Already-known hint".into()];
    context_help::troubleshoot_merging("require_auth failed", &mut existing);
    context_help::troubleshoot_merging("require_auth failed", &mut existing);
    assert_eq!(existing.len(), 3, "result was {:?}", existing);
}

#[test]
fn category_filtering_works_via_help_context() {
    let ctx = context_help::HelpContext {
        command: "deploy",
        enabled_categories: &["tip"],
        ..Default::default()
    };
    let help = context_help::generate_help(&ctx);
    assert!(!help.best_practice_tips.is_empty());
    assert!(
        help.workflow_suggestions.is_empty(),
        "got {:?}",
        help.workflow_suggestions
    );
    assert!(
        help.flags_and_examples.is_empty(),
        "got {:?}",
        help.flags_and_examples
    );
}

#[test]
fn workflow_lookups_are_consistent() {
    let _ = context_help::workflow_steps("first-contract");
    assert!(context_help::workflow_steps("first-contract").is_some());
    assert!(context_help::workflow_description("first-contract").is_some());
    assert!(context_help::workflow_duration("first-contract").is_some());
    assert!(context_help::workflow_steps("nope").is_none());

    assert_eq!(
        context_help::workflow_count(),
        help_metadata::WORKFLOWS.len()
    );
    assert_eq!(
        context_help::commands_with_help(),
        help_metadata::HELP_REGISTRY.len()
    );
}

#[test]
fn all_command_names_includes_well_known_entries() {
    let names = context_help::all_command_names();
    assert!(names.contains(&"deploy"));
    assert!(names.contains(&"wallet"));
    assert!(names.contains(&"contract"));
}

#[test]
fn command_summary_lookup() {
    assert!(context_help::command_summary("deploy").is_some());
    assert!(context_help::command_summary("no-such-cmd").is_none());
}

#[test]
fn proactive_tip_nudges_when_history_is_empty() {
    assert!(context_help::proactive_tip("deploy", &[]).is_some());
}

#[test]
fn proactive_tip_recommends_audit_after_first_deploy() {
    let hist = vec![entry("tutorial list", 1, 0)];
    let tip = context_help::proactive_tip("deploy", &hist);
    assert!(tip.unwrap_or_default().contains("audit"));
}

#[test]
fn help_engine_history_round_trip_via_disk() {
    // Make sure the same code paths used by the CLI (read history from
    // disk) return populated data when a quarantine file exists.
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let entries = vec![
        entry("wallet create deployer", 1, 0),
        entry("wallet fund deployer", 1, 0),
        entry("deployment", 1, 1),
    ];
    save_history(&entries, &dir).unwrap();
    let loaded = load_history(&dir).unwrap();
    assert_eq!(loaded.len(), 3);

    let ctx = context_help::HelpContext {
        command: "deploy",
        history: &loaded,
        ..Default::default()
    };
    let preds = context_help::generate_help(&ctx).predicted_issues;
    // No predictions when the history satisfies the prerequisites.
    assert!(preds.is_empty(), "unexpected predictions: {:?}", preds);

    // Bonus: empty history via disk is treated as empty.
    let empty_dir = tempfile::TempDir::new().unwrap();
    let empty = load_history(empty_dir.path()).unwrap();
    assert!(empty.is_empty());
}

#[test]
fn help_context_category_logic() {
    let ctx = context_help::HelpContext {
        command: "deploy",
        disabled_categories: &["workflow", "tip"],
        enabled_categories: &["tip", "workflow", "related"],
        ..Default::default()
    };
    // Disabled always wins.
    assert!(!ctx.category_enabled("workflow"));
    assert!(!ctx.category_enabled("tip"));
    assert!(ctx.category_enabled("related"));

    let empty = context_help::HelpContext::default();
    for c in context_help::CATEGORIES {
        assert!(empty.category_enabled(c));
    }
}

#[test]
fn error_quick_fixes_table_contains_required_categories() {
    let cats: Vec<&str> = help_metadata::ERROR_QUICK_FIXES
        .iter()
        .map(|f| f.category)
        .collect();
    for required in ["auth", "arithmetic", "wasm", "storage-ttl", "balance"] {
        assert!(
            cats.contains(&required),
            "missing category {required} in ERROR_QUICK_FIXES"
        );
    }
}
