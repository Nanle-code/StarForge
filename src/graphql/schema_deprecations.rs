//! GraphQL Schema Deprecation Registry
//!
//! Every field that has been deprecated (marked with `#[graphql(deprecation)]`)
//! MUST have a corresponding entry in `DEPRECATED_FIELDS` below.
//!
//! Rules (see `GRAPHQL_SCHEMA_VERSIONING.md` for the full policy):
//!
//! - `deprecated_since`: ISO 8601 date the deprecation PR merged to `master`.
//! - `earliest_removal`: must be >= `deprecated_since` + 90 days.
//! - `replacement`: name the successor field/type so clients know what to use.
//!
//! The CI job `GraphQL Schema Lint` (`.github/workflows/graphql-schema-lint.yml`)
//! reads this file via `scripts/check-graphql-deprecations.sh` and fails any PR
//! that removes a non-deprecated field or removes a deprecated field before its
//! `earliest_removal` date.

/// A single deprecated GraphQL field.
#[derive(Debug, Clone)]
pub struct DeprecatedField {
    /// The GraphQL type that owns this field (e.g. `"Query"`, `"Wallet"`).
    pub type_name: &'static str,
    /// The field name as it appears in the GraphQL schema.
    pub field_name: &'static str,
    /// ISO 8601 date (`YYYY-MM-DD`) when the deprecation was merged to master.
    pub deprecated_since: &'static str,
    /// ISO 8601 date (`YYYY-MM-DD`) on or after which the field may be removed
    /// (must be at least 90 days after `deprecated_since`).
    pub earliest_removal: &'static str,
    /// Name of the replacement field/type, if one exists.
    pub replacement: Option<&'static str>,
    /// Human-readable reason surfaced in `@deprecated(reason: "...")`.
    pub reason: &'static str,
}

/// The canonical registry of all currently deprecated GraphQL fields.
///
/// Add an entry here whenever you add `#[graphql(deprecation = "...")]` to a
/// resolver method or type field.  Remove the entry only when the field itself
/// is removed from the schema (after `earliest_removal` has elapsed).
///
/// Currently no fields are deprecated — this slice is intentionally empty until
/// the first deprecation is introduced.  The CI lint treats an empty registry
/// as clean and passes immediately.
pub const DEPRECATED_FIELDS: &[DeprecatedField] = &[
    // ── Example (commented out) ────────────────────────────────────────────
    // DeprecatedField {
    //     type_name: "Query",
    //     field_name: "wallets",
    //     deprecated_since: "2026-10-01",
    //     earliest_removal: "2026-12-31",
    //     replacement: Some("walletsPaginated"),
    //     reason: "Use `walletsPaginated(first, after)` for cursor-based pagination.",
    // },
    // ──────────────────────────────────────────────────────────────────────
];

/// Returns all deprecated field entries for a given GraphQL type name.
///
/// Useful in tests and the schema-lint script stub to query the registry
/// programmatically.
pub fn fields_for_type(type_name: &str) -> Vec<&'static DeprecatedField> {
    DEPRECATED_FIELDS
        .iter()
        .filter(|f| f.type_name == type_name)
        .collect()
}

/// Returns `true` if a field is currently registered as deprecated.
pub fn is_deprecated(type_name: &str, field_name: &str) -> bool {
    DEPRECATED_FIELDS
        .iter()
        .any(|f| f.type_name == type_name && f.field_name == field_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every deprecated field must have earliest_removal >= deprecated_since + 90 days.
    #[test]
    fn deprecation_windows_are_at_least_90_days() {
        for field in DEPRECATED_FIELDS {
            let since = parse_date(field.deprecated_since).unwrap_or_else(|| {
                panic!(
                    "Invalid deprecated_since date for {}.{}: {}",
                    field.type_name, field.field_name, field.deprecated_since
                )
            });
            let earliest = parse_date(field.earliest_removal).unwrap_or_else(|| {
                panic!(
                    "Invalid earliest_removal date for {}.{}: {}",
                    field.type_name, field.field_name, field.earliest_removal
                )
            });
            let window_days = (earliest - since).num_days();
            assert!(
                window_days >= 90,
                "Deprecation window for {}.{} is only {} days (minimum 90). \
                 earliest_removal must be at least {} days after deprecated_since.",
                field.type_name,
                field.field_name,
                window_days,
                90
            );
        }
    }

    /// Every deprecated field must supply a non-empty reason string.
    #[test]
    fn deprecation_reasons_are_non_empty() {
        for field in DEPRECATED_FIELDS {
            assert!(
                !field.reason.trim().is_empty(),
                "Deprecated field {}.{} has an empty reason string.",
                field.type_name,
                field.field_name
            );
        }
    }

    /// Field names must be unique within a type.
    #[test]
    fn no_duplicate_deprecations() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for field in DEPRECATED_FIELDS {
            let key = (field.type_name, field.field_name);
            assert!(
                seen.insert(key),
                "Duplicate deprecation entry for {}.{}",
                field.type_name,
                field.field_name
            );
        }
    }

    // ── helpers ──────────────────────────────────────────────────────────────

    /// Parse a `YYYY-MM-DD` string into a naive date (chrono NaiveDate).
    fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
    }
}
