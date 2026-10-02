//! Integration tests for the CLI migration notes generator.
//!
//! Tests the diff engine and markdown renderer against the canonical fixture
//! breaking-change set stored in `tests/fixtures/migrate_notes/`.
//!
//! Acceptance criteria verified here:
//!   - Generator produces correct migration notes for the fixture change set.
//!   - Human-review banner is present in every output (unconditional).
//!   - Specific changes are detected: command rename, flag rename, flag
//!     becoming required, config key rename, config key type change.

use starforge::utils::cli_migration_notes::{
    diff_snapshots, load_snapshot, render_migration_notes, HUMAN_REVIEW_BANNER,
};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/migrate_notes")
        .join(name)
}

// ── Fixture loading ───────────────────────────────────────────────────────────

#[test]
fn fixture_snapshots_load_without_error() {
    load_snapshot(&fixture("before.json")).expect("before.json should parse");
    load_snapshot(&fixture("after.json")).expect("after.json should parse");
}

// ── Diff engine ───────────────────────────────────────────────────────────────

#[test]
fn diff_detects_command_rename() {
    let before = load_snapshot(&fixture("before.json")).unwrap();
    let after = load_snapshot(&fixture("after.json")).unwrap();
    let changes = diff_snapshots(&before, &after);

    let found = changes.iter().any(|c| {
        matches!(c,
            starforge::utils::cli_migration_notes::CliChange::CommandRenamed { old, new }
            if old == "network-info" && new == "network show"
        )
    });
    assert!(
        found,
        "Expected CommandRenamed {{ old: 'network-info', new: 'network show' }} in changes.\nGot: {:?}",
        changes
    );
}

#[test]
fn diff_detects_flag_rename() {
    let before = load_snapshot(&fixture("before.json")).unwrap();
    let after = load_snapshot(&fixture("after.json")).unwrap();
    let changes = diff_snapshots(&before, &after);

    let found = changes.iter().any(|c| {
        matches!(c,
            starforge::utils::cli_migration_notes::CliChange::FlagRenamed { command, old_flag, new_flag }
            if command == "wallet create" && old_flag == "wallet-name" && new_flag == "name"
        )
    });
    assert!(
        found,
        "Expected FlagRenamed {{ command: 'wallet create', old_flag: 'wallet-name', new_flag: 'name' }}.\nGot: {:?}",
        changes
    );
}

#[test]
fn diff_detects_flag_became_required() {
    let before = load_snapshot(&fixture("before.json")).unwrap();
    let after = load_snapshot(&fixture("after.json")).unwrap();
    let changes = diff_snapshots(&before, &after);

    let found = changes.iter().any(|c| {
        matches!(c,
            starforge::utils::cli_migration_notes::CliChange::FlagBecameRequired { command, flag, .. }
            if command == "deploy" && flag == "network"
        )
    });
    assert!(
        found,
        "Expected FlagBecameRequired {{ command: 'deploy', flag: 'network' }}.\nGot: {:?}",
        changes
    );
}

#[test]
fn diff_detects_flag_rename_dry_run_to_simulate() {
    let before = load_snapshot(&fixture("before.json")).unwrap();
    let after = load_snapshot(&fixture("after.json")).unwrap();
    let changes = diff_snapshots(&before, &after);

    let found = changes.iter().any(|c| {
        matches!(c,
            starforge::utils::cli_migration_notes::CliChange::FlagRenamed { command, old_flag, new_flag }
            if command == "deploy" && old_flag == "dry-run" && new_flag == "simulate"
        )
    });
    assert!(
        found,
        "Expected FlagRenamed {{ command: 'deploy', old_flag: 'dry-run', new_flag: 'simulate' }}.\nGot: {:?}",
        changes
    );
}

#[test]
fn diff_detects_config_key_rename() {
    let before = load_snapshot(&fixture("before.json")).unwrap();
    let after = load_snapshot(&fixture("after.json")).unwrap();
    let changes = diff_snapshots(&before, &after);

    let found = changes.iter().any(|c| {
        matches!(c,
            starforge::utils::cli_migration_notes::CliChange::ConfigKeyRenamed { old, new }
            if old == "network.default_network" && new == "network.default"
        )
    });
    assert!(
        found,
        "Expected ConfigKeyRenamed {{ old: 'network.default_network', new: 'network.default' }}.\nGot: {:?}",
        changes
    );
}

#[test]
fn diff_detects_config_key_type_change() {
    let before = load_snapshot(&fixture("before.json")).unwrap();
    let after = load_snapshot(&fixture("after.json")).unwrap();
    let changes = diff_snapshots(&before, &after);

    let found = changes.iter().any(|c| {
        matches!(c,
            starforge::utils::cli_migration_notes::CliChange::ConfigKeyTypeChanged { key, old_type, new_type }
            if key == "deploy.timeout" && old_type == "u32" && new_type == "u64"
        )
    });
    assert!(
        found,
        "Expected ConfigKeyTypeChanged {{ key: 'deploy.timeout', old_type: 'u32', new_type: 'u64' }}.\nGot: {:?}",
        changes
    );
}

// ── Markdown renderer ─────────────────────────────────────────────────────────

#[test]
fn rendered_notes_always_contain_human_review_banner() {
    let before = load_snapshot(&fixture("before.json")).unwrap();
    let after = load_snapshot(&fixture("after.json")).unwrap();
    let changes = diff_snapshots(&before, &after);

    let notes = render_migration_notes(&before.version, &after.version, &changes);

    assert!(
        notes.contains(HUMAN_REVIEW_BANNER),
        "Migration notes MUST contain the human-review banner but it was absent.\n\
         First 500 chars of notes:\n{}",
        &notes[..notes.len().min(500)]
    );
}

#[test]
fn banner_present_even_when_no_changes() {
    // Diff an identical snapshot against itself — no changes, but banner must still appear.
    let before = load_snapshot(&fixture("before.json")).unwrap();
    let changes: Vec<starforge::utils::cli_migration_notes::CliChange> = vec![];
    let notes = render_migration_notes(&before.version, &before.version, &changes);

    assert!(
        notes.contains(HUMAN_REVIEW_BANNER),
        "Banner must be present even when there are zero changes."
    );
}

#[test]
fn rendered_notes_contain_version_header() {
    let before = load_snapshot(&fixture("before.json")).unwrap();
    let after = load_snapshot(&fixture("after.json")).unwrap();
    let changes = diff_snapshots(&before, &after);

    let notes = render_migration_notes(&before.version, &after.version, &changes);

    assert!(
        notes.contains("v0.1.0") && notes.contains("v0.2.0"),
        "Notes should reference both version strings.\nNotes snippet:\n{}",
        &notes[..notes.len().min(400)]
    );
}

#[test]
fn rendered_notes_contain_renamed_command() {
    let before = load_snapshot(&fixture("before.json")).unwrap();
    let after = load_snapshot(&fixture("after.json")).unwrap();
    let changes = diff_snapshots(&before, &after);

    let notes = render_migration_notes(&before.version, &after.version, &changes);

    assert!(
        notes.contains("network-info") && notes.contains("network show"),
        "Notes should mention both old and new names for the renamed command."
    );
}

#[test]
fn rendered_notes_contain_renamed_flag() {
    let before = load_snapshot(&fixture("before.json")).unwrap();
    let after = load_snapshot(&fixture("after.json")).unwrap();
    let changes = diff_snapshots(&before, &after);

    let notes = render_migration_notes(&before.version, &after.version, &changes);

    assert!(
        notes.contains("wallet-name") && notes.contains("--name"),
        "Notes should mention both old and new flag names for wallet create."
    );
}

#[test]
fn rendered_notes_contain_config_key_rename() {
    let before = load_snapshot(&fixture("before.json")).unwrap();
    let after = load_snapshot(&fixture("after.json")).unwrap();
    let changes = diff_snapshots(&before, &after);

    let notes = render_migration_notes(&before.version, &after.version, &changes);

    assert!(
        notes.contains("network.default_network") && notes.contains("network.default"),
        "Notes should mention both old and new config key names."
    );
}

// ── Full fixture test (comprehensive) ────────────────────────────────────────

/// This is the primary acceptance test for issue #780:
/// "Generator produces notes for a fixture breaking change set."
#[test]
fn full_fixture_produces_expected_note_sections() {
    let before = load_snapshot(&fixture("before.json")).unwrap();
    let after = load_snapshot(&fixture("after.json")).unwrap();
    let changes = diff_snapshots(&before, &after);

    // Must have detected some changes.
    assert!(
        !changes.is_empty(),
        "Expected changes from the fixture pair but found none."
    );

    let notes = render_migration_notes(&before.version, &after.version, &changes);

    // Banner is present (unconditional).
    assert!(notes.contains(HUMAN_REVIEW_BANNER), "Banner missing.");

    // Version header present.
    assert!(notes.contains("v0.1.0") && notes.contains("v0.2.0"));

    // Renamed commands section.
    assert!(
        notes.contains("Renamed Commands") || notes.contains("network-info"),
        "Expected a Renamed Commands section or reference to 'network-info'."
    );

    // Renamed flags section.
    assert!(
        notes.contains("Renamed Flags") || notes.contains("wallet-name"),
        "Expected a Renamed Flags section."
    );

    // Config key rename section.
    assert!(
        notes.contains("Config Key Renames") || notes.contains("network.default_network"),
        "Expected a Config Key Renames section."
    );

    // Footer present.
    assert!(
        notes.contains("AI-generated migration notes") || notes.contains("review"),
        "Expected footer or review reminder."
    );
}
