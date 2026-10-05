use crate::graphql::resolvers::{Mutation, Query};
use crate::graphql::subscription::Subscription;
use async_graphql::Schema;

/// Current schema version string (format: `YYYY-MM`).
///
/// Increment this whenever a breaking change ships:
///   - a deprecated field is removed, OR
///   - a field's type signature changes, OR
///   - a non-null argument is added to an existing field.
///
/// Non-breaking additions (new fields/types) do NOT require a bump.
/// See `GRAPHQL_SCHEMA_VERSIONING.md` for the full policy.
pub const SCHEMA_VERSION: &str = "2026-10";

pub type StarforgeSchema = Schema<Query, Mutation, Subscription>;

pub fn build_schema() -> StarforgeSchema {
    Schema::build(Query, Mutation, Subscription).finish()
}
