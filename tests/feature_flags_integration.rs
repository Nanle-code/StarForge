//! Integration tests for the feature flag system.
//!
//! These exercise the full FlagManager + Database + Config wiring without
//! going through the CLI parser. They are the closest analogue to what a
//! real `starforge feature-flags …` invocation does.

use starforge::utils::config;
use starforge::utils::config::FeatureFlagsConfig;
use starforge::utils::database::Database;
use starforge::utils::feature_flags::{
    self, FlagCategory, FlagDefinition, FlagManager, SegmentRule, UserContext, Variant,
};

fn fresh_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.initialize().unwrap();
    db
}

fn mgr<'a>(db: &'a Database, which: &str) -> FlagManager<'a> {
    FlagManager::new(db, UserContext::new(which)).with_exposure_recording(false)
}

#[test]
fn builtin_flags_are_seeded_on_first_init() {
    let db = fresh_db();
    let defs = db.list_definitions().unwrap();
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(
        names.contains(&"ai.audit"),
        "ai.audit should be seeded; got {:?}",
        names
    );
    assert!(names.contains(&"ai.docs"));
    assert!(names.contains(&"ai.gas_estimation"));
    assert!(names.contains(&"ai.completion"));
}

#[test]
fn stable_flag_default_is_enabled() {
    let db = fresh_db();
    let m = mgr(&db, "u");
    assert!(
        m.is_enabled("ai.debug"),
        "Stable flag should be enabled by default"
    );
    assert!(
        m.is_enabled("ai.completion"),
        "Stable flag should be enabled by default"
    );
}

#[test]
fn beta_flag_default_is_disabled() {
    let db = fresh_db();
    let m = mgr(&db, "u");
    assert!(!m.is_enabled("ai.audit"));
    assert!(!m.is_enabled("ai.docs"));
    assert!(!m.is_enabled("ai.gas_estimation"));
}

#[test]
fn enable_disable_creates_audit_trail() {
    let db = fresh_db();
    let m = mgr(&db, "u");
    m.set_enabled("ai.audit", true).unwrap();
    assert!(m.is_enabled("ai.audit"));
    m.set_enabled("ai.audit", false).unwrap();
    assert!(!m.is_enabled("ai.audit"));
    let history = db.state_history("ai.audit").unwrap();
    assert!(
        history.len() >= 3,
        "expected ≥ 3 versions (initial + enable + disable)"
    );
}

#[test]
fn percentage_rollout_excludes_consistently() {
    let db = fresh_db();
    let m = mgr(&db, "seed");
    m.set_enabled("ai.audit", true).unwrap();
    m.set_rollout("ai.audit", 25).unwrap();
    // The same user always gets the same bucket.
    assert_eq!(
        feature_flags::stable_bucket("ai.audit", "user-x", 100) < 25,
        m.is_enabled("ai.audit"),
    );
}

#[test]
fn variant_distribution_is_deterministic() {
    let db = fresh_db();
    let m = mgr(&db, "seed");
    m.set_enabled("ai.audit", true).unwrap();
    m.set_rollout("ai.audit", 100).unwrap();
    m.replace_variants(
        "ai.audit",
        vec![
            Variant {
                name: "control".into(),
                weight: 1,
                payload: None,
            },
            Variant {
                name: "treatment".into(),
                weight: 1,
                payload: None,
            },
        ],
    )
    .unwrap();
    // Same (flag, user) → same variant every time.
    for i in 0..100 {
        let id = format!("u-{i}");
        let a = mgr(&db, &id).evaluate_dry("ai.audit").unwrap().variant;
        let b = mgr(&db, &id).evaluate_dry("ai.audit").unwrap().variant;
        assert_eq!(a, b, "variant must be deterministic for {}", id);
    }
}

#[test]
fn override_takes_priority_over_state() {
    let db = fresh_db();
    let m = mgr(&db, "u-disabled");
    m.set_enabled("ai.audit", false).unwrap();
    assert!(!m.is_enabled("ai.audit"));
    m.set_override("ai.audit", "u-disabled", true, None)
        .unwrap();
    assert!(m.is_enabled("ai.audit"));
    let res = m.evaluate_dry("ai.audit").unwrap();
    assert!(res.from_override);
}

#[test]
fn override_with_unknown_variant_is_rejected() {
    let db = fresh_db();
    let m = mgr(&db, "u");
    m.set_enabled("ai.audit", true).unwrap();
    m.replace_variants(
        "ai.audit",
        vec![Variant {
            name: "control".into(),
            weight: 1,
            payload: None,
        }],
    )
    .unwrap();
    let err = m
        .set_override("ai.audit", "u", true, Some("ghost".into()))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("variant 'ghost' is not registered"),
        "got: {err}"
    );
}

#[test]
fn rollback_restores_previous_version() {
    let db = fresh_db();
    let m = mgr(&db, "u");
    m.set_enabled("ai.audit", true).unwrap();
    m.set_rollout("ai.audit", 30).unwrap();
    let target_version = db.latest_state("ai.audit").unwrap().unwrap().version;
    m.set_rollout("ai.audit", 90).unwrap();
    let restored = m.rollback("ai.audit", target_version).unwrap();
    assert_eq!(restored.rollout_percent, 30);
}

#[test]
fn unknown_flag_returns_disabled() {
    let db = fresh_db();
    let m = mgr(&db, "u");
    let res = m.evaluate_dry("nope.never.defined").unwrap();
    assert!(!res.enabled);
    assert!(res.reason.contains("unknown flag"));
}

#[test]
fn metrics_summary_groups_by_kind() {
    let db = fresh_db();
    let m = FlagManager::new(&db, UserContext::new("u")).with_exposure_recording(true);
    m.set_enabled("ai.audit", true).unwrap();
    m.set_rollout("ai.audit", 100).unwrap();
    let _ = m.is_enabled("ai.audit"); // exposure
    m.record_conversion("ai.audit").unwrap();
    m.record_rejection("ai.audit").unwrap();
    let summary = db.metrics_summary("ai.audit").unwrap();
    assert!(summary.get("exposure").copied().unwrap_or(0) >= 1);
    assert_eq!(summary.get("conversion").copied().unwrap_or(0), 1);
    assert_eq!(summary.get("rejection").copied().unwrap_or(0), 1);
}

#[test]
fn config_feature_flags_section_round_trips() {
    let cfg = config::Config {
        feature_flags: FeatureFlagsConfig {
            metrics_enabled: false,
            metrics_retention_days: 7,
            default_attributes: Default::default(),
        },
        ..Default::default()
    };
    let db = fresh_db();
    db.save_config(&cfg).unwrap();
    let loaded = db.load_config().unwrap();
    assert!(!loaded.feature_flags.metrics_enabled);
    assert_eq!(loaded.feature_flags.metrics_retention_days, 7);
}

#[test]
fn segments_allow_user_allowlists() {
    let db = fresh_db();
    let m = mgr(&db, "u-in");
    m.set_enabled("ai.audit", true).unwrap();
    m.set_rollout("ai.audit", 0).unwrap(); // exclude everyone but segments
    m.replace_segments(
        "ai.audit",
        vec![SegmentRule::UserInList {
            user_ids: vec!["u-in".into()],
        }],
    )
    .unwrap();
    assert!(mgr(&db, "u-in").is_enabled("ai.audit"));
    assert!(!mgr(&db, "u-out").is_enabled("ai.audit"));
}

#[test]
fn register_then_evaluate_round_trip() {
    let db = fresh_db();
    let m = mgr(&db, "u");
    assert!(!m.is_enabled("my.new.flag"));
    m.register_flag(FlagDefinition {
        name: "my.new.flag".into(),
        category: FlagCategory::Stable,
        description: "test".into(),
        owner: None,
        user_manageable: true,
    })
    .unwrap();
    assert!(m.is_enabled("my.new.flag"));
    // register_flag returns false the second time.
    assert!(!m
        .register_flag(FlagDefinition {
            name: "my.new.flag".into(),
            category: FlagCategory::Stable,
            description: "test".into(),
            owner: None,
            user_manageable: true,
        })
        .unwrap());
}

#[test]
fn version_increments_monotonically() {
    let db = fresh_db();
    let m = mgr(&db, "u");
    let v0 = db.current_snapshot_version().unwrap();
    m.set_enabled("ai.audit", true).unwrap();
    let v1 = db.current_snapshot_version().unwrap();
    m.set_rollout("ai.audit", 50).unwrap();
    let v2 = db.current_snapshot_version().unwrap();
    assert!(v1 > v0);
    assert!(v2 > v1);
}

#[test]
fn invalid_flag_name_is_rejected() {
    let err = feature_flags::validate_ident("flag name", "AI.Audit").unwrap_err();
    assert!(err.to_string().contains("invalid character"));
    let err = feature_flags::validate_ident("flag name", "ai audit").unwrap_err();
    assert!(err.to_string().contains("invalid character"));
    let err = feature_flags::validate_ident("flag name", "").unwrap_err();
    assert!(err.to_string().contains("must not be empty"));
}
