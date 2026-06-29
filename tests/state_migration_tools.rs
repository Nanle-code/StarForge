//! Integration tests for the contract state diffing & migration tooling (D-9).

use starforge::utils::state_migration as sm;

fn entry(key: &str, value: &str, dur: &str) -> sm::StateEntry {
    sm::StateEntry {
        key: key.to_string(),
        value: value.to_string(),
        durability: dur.to_string(),
    }
}

fn snapshot(contract: &str, label: &str, entries: Vec<sm::StateEntry>) -> sm::StateSnapshot {
    sm::StateSnapshot::new(
        contract,
        "testnet",
        Some("wasmhash".into()),
        42,
        Some(label.to_string()),
        entries,
    )
}

#[test]
fn end_to_end_snapshot_diff_migrate_test_rollback() {
    // v1 state
    let v1 = snapshot(
        "CCONTRACT",
        "v1",
        vec![
            entry("admin", "GADMIN", "instance"),
            entry("total_supply", "1000", "persistent"),
            entry("paused", "false", "instance"),
        ],
    );

    // v2 state: total_supply changed, paused removed, decimals added
    let v2 = snapshot(
        "CCONTRACT",
        "v2",
        vec![
            entry("admin", "GADMIN", "instance"),
            entry("total_supply", "2000", "persistent"),
            entry("decimals", "7", "instance"),
        ],
    );

    // 1. Diff detects exactly the expected changes.
    let diff = sm::diff_snapshots(&v1, &v2);
    assert_eq!(diff.modified, 1, "total_supply modified");
    assert_eq!(diff.removed, 1, "paused removed");
    assert_eq!(diff.added, 1, "decimals added");
    assert_eq!(diff.unchanged, 1, "admin unchanged");

    // 2. Migration plan + script generation.
    let plan = sm::generate_migration_plan(&diff);
    assert_eq!(plan.operations.len(), 3);
    let script = sm::generate_migration_script(&plan);
    assert!(script.contains("fn migrate"));
    assert!(script.contains("require_auth"));

    // 3. Migration testing framework: applying the plan to v1 reproduces v2.
    let result = sm::test_migration(&v1, &plan.operations, &v2);
    assert!(result.passed, "mismatches: {:?}", result.mismatches);
    assert_eq!(result.actual_hash, v2.state_hash);

    // 4. Rollback: derive inverse ops and confirm they restore v1.
    let rollback = sm::rollback_operations(&v2, &v1);
    let restored = sm::apply_operations(&v2, &rollback);
    assert_eq!(restored.state_hash, v1.state_hash);
}

#[test]
fn validation_blocks_persistent_data_loss() {
    let before = snapshot(
        "C",
        "before",
        vec![entry("vault_balance", "5000", "persistent")],
    );
    let after = snapshot("C", "after", vec![]);
    let diff = sm::diff_snapshots(&before, &after);

    // Default policy forbids persistent removal -> blocking.
    let strict = sm::validate_transition(&diff, &sm::MigrationPolicy::default());
    assert!(sm::has_blocking_findings(&strict));

    // Explicitly allowing it clears the block.
    let lenient = sm::MigrationPolicy {
        allow_persistent_removal: true,
        ..sm::MigrationPolicy::default()
    };
    let relaxed = sm::validate_transition(&diff, &lenient);
    assert!(!sm::has_blocking_findings(&relaxed));
}

#[test]
fn snapshot_persistence_roundtrips_via_json() {
    let snap = snapshot("CPERSIST", "v1", vec![entry("k", "v", "instance")]);
    let json = serde_json::to_string_pretty(&snap).unwrap();
    let restored: sm::StateSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.state_hash, snap.state_hash);
    assert_eq!(restored.entries.len(), 1);
}

#[test]
fn detects_wrong_migration_plan() {
    let base = snapshot("C", "base", vec![entry("a", "1", "instance")]);
    let expected = snapshot("C", "expected", vec![entry("a", "2", "instance")]);
    let wrong_ops = vec![sm::MigrationOperation::SetKey {
        durability: "instance".into(),
        key: "a".into(),
        value: "3".into(),
    }];
    let result = sm::test_migration(&base, &wrong_ops, &expected);
    assert!(!result.passed);
    assert!(!result.mismatches.is_empty());
}
