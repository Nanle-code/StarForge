//! Integration tests for registry schema validation (issue #686).
//!
//! These exercise the boundary the CLI actually crosses: a registry document
//! arriving from disk, from the marketplace, or bundled in the binary is
//! checked against `templates/registry.schema.json` before anything uses it.

use serde_json::json;
use starforge::utils::template_schema::{
    check_template_name, registry_schema, validate_registry, validate_template_entry,
};
use starforge::utils::templates::{
    parse_registry_checked, validate_bundled_registry, validate_registry_file, TemplateRegistry,
};
use std::fs;
use tempfile::tempdir;

fn valid_registry_json() -> String {
    json!({
        "version": "1",
        "templates": [{
            "name": "escrow",
            "version": "1.0.0",
            "description": "Token escrow",
            "author": "StarForge",
            "tags": ["defi"],
            "source": { "type": "builtin", "id": "escrow" }
        }]
    })
    .to_string()
}

// ── the shipped registry and schema agree ────────────────────────────────────

#[test]
fn bundled_registry_satisfies_the_schema() {
    let report = validate_bundled_registry().expect("bundled registry parses");
    assert!(
        report.is_valid(),
        "templates/registry.json violates its own schema: {:?}",
        report.errors
    );
    assert!(
        report.warnings.is_empty(),
        "templates/registry.json has unknown fields: {:?}",
        report.warnings
    );
}

/// The registry bundled with the binary is the offline fallback, so it must
/// also deserialize into the type the loaders return. This is the regression
/// test for a schema/type drift that made that fallback fail at runtime.
#[test]
fn bundled_registry_loads_through_the_checked_loader() {
    let raw = fs::read_to_string("templates/registry.json").expect("read bundled registry");
    let registry: TemplateRegistry =
        parse_registry_checked(&raw, "templates/registry.json").expect("bundled registry loads");
    assert!(
        !registry.templates.is_empty(),
        "bundled registry should ship templates"
    );

    let audited = registry
        .templates
        .iter()
        .filter_map(|t| t.security_review.as_ref())
        .filter_map(|review| review.findings)
        .count();
    assert!(
        audited > 0,
        "audited templates should carry a numeric findings count"
    );
}

#[test]
fn schema_definitions_all_resolve() {
    let schema = registry_schema();
    for name in [
        "templateEntry",
        "source",
        "securityReview",
        "changelogEntry",
    ] {
        assert!(
            schema.pointer(&format!("/$defs/{}", name)).is_some(),
            "schema is missing $defs/{}",
            name
        );
    }
}

// ── loader: primary flow ─────────────────────────────────────────────────────

#[test]
fn a_valid_registry_document_loads() {
    let registry = parse_registry_checked(&valid_registry_json(), "test").expect("valid registry");
    assert_eq!(registry.templates.len(), 1);
    assert_eq!(registry.templates[0].name, "escrow");
}

#[test]
fn a_registry_file_on_disk_is_validated() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("registry.json");
    fs::write(&path, valid_registry_json()).unwrap();

    let report = validate_registry_file(&path).expect("file is readable");
    assert!(report.is_valid(), "{:?}", report.errors);
    assert!(report.origin.contains("registry.json"));
}

#[test]
fn a_file_holding_one_entry_is_validated_as_an_entry() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("my-template.json");
    fs::write(
        &path,
        json!({
            "name": "my-template",
            "version": "0.2.0",
            "description": "Mine",
            "author": "Me",
            "tags": [],
            "source": { "type": "local", "path": "./my-template" }
        })
        .to_string(),
    )
    .unwrap();

    let report = validate_registry_file(&path).expect("file is readable");
    assert!(report.is_valid(), "{:?}", report.errors);
}

// ── loader: boundary cases ───────────────────────────────────────────────────

#[test]
fn a_registry_with_no_templates_loads() {
    let registry = parse_registry_checked(r#"{"templates": []}"#, "test").expect("empty registry");
    assert!(registry.templates.is_empty());
}

#[test]
fn unknown_fields_do_not_block_loading() {
    let raw = json!({
        "templates": [{
            "name": "escrow",
            "version": "1.0.0",
            "description": "Token escrow",
            "author": "StarForge",
            "tags": ["defi"],
            "source": { "type": "builtin", "id": "escrow" },
            "future_field": "from a newer CLI"
        }]
    })
    .to_string();

    let registry = parse_registry_checked(&raw, "test").expect("forward compatible");
    assert_eq!(registry.templates.len(), 1);

    // It is still surfaced as a warning for template authors.
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let report = validate_registry(&value, "test");
    assert!(report.is_valid());
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(report.warnings[0].field, "templates[0].future_field");
}

// ── loader: failure cases ────────────────────────────────────────────────────

#[test]
fn a_malformed_entry_fails_with_the_offending_field() {
    let raw = json!({
        "templates": [
            {
                "name": "ok",
                "version": "1.0.0",
                "description": "fine",
                "author": "StarForge",
                "tags": [],
                "source": { "type": "builtin", "id": "ok" }
            },
            {
                "name": "broken",
                "version": "v2",
                "description": "bad version",
                "author": "StarForge",
                "tags": [],
                "source": { "type": "builtin", "id": "broken" }
            }
        ]
    })
    .to_string();

    let err = parse_registry_checked(&raw, "the marketplace registry").unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("templates[1].version"),
        "error should name the field: {}",
        message
    );
    assert!(
        message.contains("the marketplace registry"),
        "error should name the registry: {}",
        message
    );
}

#[test]
fn a_missing_required_field_fails_before_deserialization() {
    let raw = json!({
        "templates": [{
            "name": "no-source",
            "version": "1.0.0",
            "description": "missing its source",
            "author": "StarForge",
            "tags": []
        }]
    })
    .to_string();

    let err = parse_registry_checked(&raw, "test").unwrap_err();
    assert!(
        err.to_string()
            .contains("templates[0].source: required field is missing"),
        "{}",
        err
    );
}

#[test]
fn invalid_json_reports_where_it_broke() {
    let err = parse_registry_checked("{\"templates\": [", "test").unwrap_err();
    let message = err.to_string();
    assert!(message.contains("is not valid JSON"), "{}", message);
    assert!(message.contains("line"), "{}", message);
}

#[test]
fn every_bad_field_in_an_entry_is_reported_at_once() {
    let entry = json!({
        "name": "bad name",
        "version": "1",
        "description": "several problems",
        "author": "StarForge",
        "tags": ["ok", ""],
        "source": { "type": "git" },
        "maintenance": "archived"
    });

    let report = validate_template_entry(&entry, "entry");
    let fields: Vec<&str> = report.errors.iter().map(|e| e.field.as_str()).collect();
    for expected in ["name", "version", "tags[1]", "source.url", "maintenance"] {
        assert!(
            fields.contains(&expected),
            "expected a problem on {}, got {:?}",
            expected,
            fields
        );
    }
}

#[test]
fn a_name_that_would_escape_the_template_store_is_refused() {
    for name in ["../evil", "a/b", "a\\b", "with space", ".."] {
        assert!(
            check_template_name(name).is_err(),
            "'{}' should be rejected as a template name",
            name
        );
    }
    for name in ["escrow", "sep-41-token", "my_template.v2"] {
        assert!(
            check_template_name(name).is_ok(),
            "'{}' should be accepted as a template name",
            name
        );
    }
}
