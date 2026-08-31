//! Schema validation for StarForge template registries.
//!
//! Every registry document StarForge reads — the registry bundled with the
//! binary, a remote marketplace index, or the local cache written by
//! `starforge template install/publish` — is checked against
//! `templates/registry.schema.json` *before* it is deserialized and used.
//!
//! The point is to fail early and precisely. Without this pass a malformed
//! template surfaces as a late, opaque failure (a serde type error naming no
//! field, or a scaffolding crash long after the template was accepted). With
//! it, the user gets the offending field and what to do about it:
//!
//! ```text
//! templates[3].version: 'v1.2' is not valid semver (expected major.minor.patch, e.g. "1.2.0")
//! templates[3].source.url: required field is missing
//! ```
//!
//! ## Supported schema vocabulary
//!
//! The validator implements the subset of JSON Schema the registry schema
//! actually uses — `$ref` (local pointers), `type`, `enum`, `const`,
//! `required`, `properties`, `items`, `oneOf`, `minLength`, `maxLength`,
//! `minItems`, `minimum` and `maximum` — plus two StarForge extensions:
//!
//! * `x-format` — a semantic check that plain JSON Schema cannot express
//!   without regular expressions: `semver`, `rfc3339`, `date`, `url`,
//!   `git-url` and `template-name`.
//! * `x-unknown-properties: "warn"` — properties not named by the schema are
//!   reported as warnings rather than errors. Unknown fields stay forward
//!   compatible (an older CLI can read a newer registry) while still catching
//!   misspelled field names when an author runs `starforge template validate`.

use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashSet;
use std::fmt;

/// The registry schema, embedded at build time so validation works offline.
pub const REGISTRY_SCHEMA_JSON: &str = include_str!("../../templates/registry.schema.json");

static SCHEMA: Lazy<Value> = Lazy::new(|| {
    serde_json::from_str(REGISTRY_SCHEMA_JSON)
        .expect("embedded templates/registry.schema.json must be valid JSON")
});

/// The parsed registry schema.
pub fn registry_schema() -> &'static Value {
    &SCHEMA
}

/// A single problem, anchored to the field that caused it.
///
/// `field` is a human-readable path such as `templates[3].source.url`. It is
/// empty when the problem concerns the document as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldIssue {
    pub field: String,
    pub message: String,
}

impl fmt::Display for FieldIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.field.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.field, self.message)
        }
    }
}

/// The outcome of validating one document.
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    /// What was validated, e.g. a file path or a registry URL.
    pub origin: String,
    /// Problems that make the document unusable.
    pub errors: Vec<FieldIssue>,
    /// Problems worth flagging that do not block use.
    pub warnings: Vec<FieldIssue>,
}

impl ValidationReport {
    /// Whether the document can be used.
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Turn a failing report into an `anyhow` error, or return `Ok(())`.
    pub fn into_result(self) -> anyhow::Result<()> {
        if self.is_valid() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("{}", self))
        }
    }
}

impl fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.errors.len();
        write!(
            f,
            "{} does not match the template registry schema ({} {})",
            self.origin,
            count,
            if count == 1 { "problem" } else { "problems" }
        )?;
        for issue in &self.errors {
            write!(f, "\n  - {}", issue)?;
        }
        for issue in &self.warnings {
            write!(f, "\n  ! {}", issue)?;
        }
        Ok(())
    }
}

/// Parse a registry document, reporting JSON syntax errors with line and column.
pub fn parse_json(raw: &str, origin: &str) -> anyhow::Result<Value> {
    serde_json::from_str(raw).map_err(|e| {
        anyhow::anyhow!(
            "{} is not valid JSON: {} (line {}, column {})",
            origin,
            e,
            e.line(),
            e.column()
        )
    })
}

/// Validate a whole registry document (`{"templates": [...]}`).
pub fn validate_registry(value: &Value, origin: &str) -> ValidationReport {
    let mut ctx = Ctx::new(origin);
    ctx.validate(registry_schema(), value, "");
    ctx.registry_semantics(value);
    ctx.finish()
}

/// Validate a single template entry against `#/$defs/templateEntry`.
pub fn validate_template_entry(value: &Value, origin: &str) -> ValidationReport {
    let mut ctx = Ctx::new(origin);
    let schema = entry_schema();
    ctx.validate(schema, value, "");
    ctx.entry_semantics(value, "");
    ctx.finish()
}

/// Check a template name on its own, using the same rule the schema applies.
///
/// Callers derive a name from user input (an archive stem, a directory name, a
/// git URL) and then use it as a directory name under the template store, so
/// this runs before anything is written to disk.
pub fn check_template_name(name: &str) -> Result<(), FieldIssue> {
    if name.is_empty() {
        return Err(FieldIssue {
            field: "name".to_string(),
            message: "must not be empty".to_string(),
        });
    }
    match invalid_template_name(name) {
        Some(message) => Err(FieldIssue {
            field: "name".to_string(),
            message,
        }),
        None => Ok(()),
    }
}

fn entry_schema() -> &'static Value {
    registry_schema()
        .pointer("/$defs/templateEntry")
        .expect("registry schema always defines $defs/templateEntry")
}

// ─── validator ───────────────────────────────────────────────────────────────

struct Ctx<'a> {
    origin: String,
    root: &'a Value,
    errors: Vec<FieldIssue>,
    warnings: Vec<FieldIssue>,
}

impl<'a> Ctx<'a> {
    fn new(origin: &str) -> Self {
        Self {
            origin: origin.to_string(),
            root: registry_schema(),
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn error(&mut self, field: &str, message: impl Into<String>) {
        self.errors.push(FieldIssue {
            field: field.to_string(),
            message: message.into(),
        });
    }

    fn warn(&mut self, field: &str, message: impl Into<String>) {
        self.warnings.push(FieldIssue {
            field: field.to_string(),
            message: message.into(),
        });
    }

    /// Drop duplicate issues (a `oneOf` branch can restate a sibling error)
    /// while preserving order.
    fn finish(self) -> ValidationReport {
        ValidationReport {
            origin: self.origin,
            errors: dedupe(self.errors),
            warnings: dedupe(self.warnings),
        }
    }

    /// Resolve a `{"$ref": "#/..."}` wrapper to the schema it points at.
    fn resolve<'s>(&self, schema: &'s Value) -> &'s Value
    where
        'a: 's,
    {
        match schema.get("$ref").and_then(Value::as_str) {
            Some(reference) => reference
                .strip_prefix('#')
                .and_then(|p| self.root.pointer(p))
                .unwrap_or(schema),
            None => schema,
        }
    }

    fn validate(&mut self, schema: &Value, instance: &Value, field: &str) {
        let schema = self.resolve(schema);

        // --- type ---
        if let Some(expected) = schema.get("type") {
            if !type_matches(expected, instance) {
                self.error(
                    field,
                    format!(
                        "expected {}, found {}",
                        describe_type(expected),
                        article(type_name(instance))
                    ),
                );
                // Every other keyword is meaningless once the type is wrong.
                return;
            }
        }

        // --- const / enum ---
        if let Some(expected) = schema.get("const") {
            if instance != expected {
                self.error(field, format!("must be {}", render(expected)));
                return;
            }
        }
        if let Some(Value::Array(allowed)) = schema.get("enum") {
            if !allowed.contains(instance) {
                self.error(
                    field,
                    format!(
                        "{} is not one of: {}",
                        render(instance),
                        allowed
                            .iter()
                            .map(render_bare)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
            }
        }

        match instance {
            Value::Object(map) => {
                if let Some(Value::Array(required)) = schema.get("required") {
                    for name in required.iter().filter_map(Value::as_str) {
                        if !map.contains_key(name) {
                            self.error(&join(field, name), "required field is missing");
                        }
                    }
                }

                let properties = schema.get("properties").and_then(Value::as_object);
                if let Some(properties) = properties {
                    for (name, subschema) in properties {
                        if let Some(child) = map.get(name) {
                            self.validate(subschema, child, &join(field, name));
                        }
                    }
                }

                if schema.get("x-unknown-properties").and_then(Value::as_str) == Some("warn") {
                    let known: HashSet<&str> = properties
                        .map(|p| p.keys().map(String::as_str).collect())
                        .unwrap_or_default();
                    // A `oneOf` branch names the rest of the valid properties.
                    let branch_known = self.one_of_property_names(schema, map);
                    for name in map.keys() {
                        if !known.contains(name.as_str()) && !branch_known.contains(name.as_str()) {
                            self.warn(
                                &join(field, name),
                                "unknown field; it will be ignored (check the spelling)",
                            );
                        }
                    }
                }

                if let Some(Value::Array(branches)) = schema.get("oneOf") {
                    self.validate_one_of(branches, instance, field);
                }
            }
            Value::Array(items) => {
                if let Some(subschema) = schema.get("items") {
                    for (i, item) in items.iter().enumerate() {
                        self.validate(subschema, item, &format!("{}[{}]", field, i));
                    }
                }
                if let Some(min) = schema.get("minItems").and_then(Value::as_u64) {
                    if (items.len() as u64) < min {
                        self.error(field, format!("must have at least {} item(s)", min));
                    }
                }
            }
            Value::String(s) => {
                if let Some(min) = schema.get("minLength").and_then(Value::as_u64) {
                    if (s.chars().count() as u64) < min {
                        if min == 1 {
                            self.error(field, "must not be empty");
                        } else {
                            self.error(field, format!("must be at least {} characters", min));
                        }
                    }
                }
                if let Some(max) = schema.get("maxLength").and_then(Value::as_u64) {
                    if (s.chars().count() as u64) > max {
                        self.error(
                            field,
                            format!(
                                "must be at most {} characters (found {})",
                                max,
                                s.chars().count()
                            ),
                        );
                    }
                }
                if let Some(format) = schema.get("x-format").and_then(Value::as_str) {
                    if let Some(message) = check_format(format, s) {
                        self.error(field, message);
                    }
                }
            }
            Value::Number(_) => {
                let n = instance.as_f64().unwrap_or_default();
                if let Some(min) = schema.get("minimum").and_then(Value::as_f64) {
                    if n < min {
                        self.error(
                            field,
                            format!("must be >= {} (found {})", min, render(instance)),
                        );
                    }
                }
                if let Some(max) = schema.get("maximum").and_then(Value::as_f64) {
                    if n > max {
                        self.error(
                            field,
                            format!("must be <= {} (found {})", max, render(instance)),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// Property names contributed by whichever `oneOf` branch the instance
    /// selects, so they are not mistaken for unknown fields.
    fn one_of_property_names(
        &self,
        schema: &Value,
        map: &serde_json::Map<String, Value>,
    ) -> HashSet<String> {
        let mut names = HashSet::new();
        let Some(Value::Array(branches)) = schema.get("oneOf") else {
            return names;
        };
        for branch in branches {
            let branch = self.resolve(branch);
            if let Some(properties) = branch.get("properties").and_then(Value::as_object) {
                if discriminator_matches(branch, map) {
                    names.extend(properties.keys().cloned());
                }
            }
        }
        names
    }

    /// Validate against a `oneOf`, using the `type` discriminator to report
    /// errors from the branch the author clearly meant.
    fn validate_one_of(&mut self, branches: &[Value], instance: &Value, field: &str) {
        let map = match instance.as_object() {
            Some(map) => map,
            None => return,
        };

        let allowed: Vec<String> = branches
            .iter()
            .filter_map(|b| {
                self.resolve(b)
                    .pointer("/properties/type/const")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();

        let selected = branches
            .iter()
            .find(|branch| discriminator_matches(self.resolve(branch), map));

        match selected {
            Some(branch) => {
                let branch = self.resolve(branch).clone();
                self.validate(&branch, instance, field);
            }
            None => {
                // No branch claims this `type`; say so against the field that
                // decides it. Deduplication folds this into the sibling
                // `enum`/`required` error when they overlap.
                let type_field = join(field, "type");
                match map.get("type") {
                    Some(value) => self.error(
                        &type_field,
                        format!("{} is not one of: {}", render(value), allowed.join(", ")),
                    ),
                    None => self.error(&type_field, "required field is missing"),
                }
            }
        }
    }

    /// Registry-wide rules the schema itself cannot express.
    fn registry_semantics(&mut self, value: &Value) {
        let Some(templates) = value.get("templates").and_then(Value::as_array) else {
            return;
        };

        let mut seen: Vec<(&str, &str, usize)> = Vec::new();
        for (i, entry) in templates.iter().enumerate() {
            let field = format!("templates[{}]", i);
            self.entry_semantics(entry, &field);

            let (Some(name), Some(version)) = (
                entry.get("name").and_then(Value::as_str),
                entry.get("version").and_then(Value::as_str),
            ) else {
                continue;
            };
            if let Some((_, _, first)) = seen
                .iter()
                .find(|(n, v, _)| *n == name && *v == version)
                .copied()
            {
                self.error(
                    &join(&field, "name"),
                    format!(
                        "duplicate entry: '{}' version {} is already defined at templates[{}]",
                        name, version, first
                    ),
                );
            } else {
                seen.push((name, version, i));
            }
        }
    }

    /// Per-entry rules the schema itself cannot express.
    fn entry_semantics(&mut self, entry: &Value, field: &str) {
        let min = entry.get("cli_version_min").and_then(Value::as_str);
        let max = entry.get("cli_version_max").and_then(Value::as_str);
        if let (Some(min), Some(max)) = (min, max) {
            if let (Some(min_v), Some(max_v)) = (parse_semver(min), parse_semver(max)) {
                if min_v > max_v {
                    self.error(
                        &join(field, "cli_version_max"),
                        format!(
                            "'{}' is lower than cli_version_min '{}'; the supported range is empty",
                            max, min
                        ),
                    );
                }
            }
        }
    }
}

fn discriminator_matches(branch: &Value, map: &serde_json::Map<String, Value>) -> bool {
    match branch.pointer("/properties/type/const") {
        Some(expected) => map.get("type") == Some(expected),
        None => false,
    }
}

fn dedupe(issues: Vec<FieldIssue>) -> Vec<FieldIssue> {
    let mut seen = HashSet::new();
    issues
        .into_iter()
        .filter(|issue| seen.insert((issue.field.clone(), issue.message.clone())))
        .collect()
}

fn join(field: &str, name: &str) -> String {
    if field.is_empty() {
        name.to_string()
    } else {
        format!("{}.{}", field, name)
    }
}

// ─── type helpers ────────────────────────────────────────────────────────────

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn article(name: &str) -> String {
    match name {
        "array" | "object" => format!("an {}", name),
        "null" => "null".to_string(),
        other => format!("a {}", other),
    }
}

fn type_matches(expected: &Value, instance: &Value) -> bool {
    match expected {
        Value::String(name) => matches_single_type(name, instance),
        Value::Array(names) => names
            .iter()
            .filter_map(Value::as_str)
            .any(|name| matches_single_type(name, instance)),
        _ => true,
    }
}

fn matches_single_type(name: &str, instance: &Value) -> bool {
    match name {
        "null" => instance.is_null(),
        "boolean" => instance.is_boolean(),
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "number" => instance.is_number(),
        "integer" => {
            instance.is_i64()
                || instance.is_u64()
                || instance.as_f64().map(|f| f.fract() == 0.0).unwrap_or(false)
        }
        _ => true,
    }
}

fn describe_type(expected: &Value) -> String {
    match expected {
        Value::String(name) => article(name),
        Value::Array(names) => {
            let rendered: Vec<String> = names
                .iter()
                .filter_map(Value::as_str)
                .map(article)
                .collect();
            match rendered.len() {
                0 => "a value".to_string(),
                1 => rendered[0].clone(),
                _ => format!(
                    "{} or {}",
                    rendered[..rendered.len() - 1].join(", "),
                    rendered[rendered.len() - 1]
                ),
            }
        }
        _ => "a value".to_string(),
    }
}

/// Render a JSON value for an error message, quoting strings.
fn render(value: &Value) -> String {
    match value {
        Value::String(s) => format!("'{}'", s),
        other => other.to_string(),
    }
}

/// Render a value for a comma-separated list, without quotes.
fn render_bare(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// ─── x-format checks ─────────────────────────────────────────────────────────

/// Run an `x-format` check, returning the error message when it fails.
fn check_format(format: &str, value: &str) -> Option<String> {
    match format {
        "semver" => (parse_semver(value).is_none()).then(|| {
            format!(
                "'{}' is not valid semver (expected major.minor.patch, e.g. \"1.2.0\")",
                value
            )
        }),
        // An empty timestamp means "unset" for templates installed from a
        // local path or a git URL, which have no publication date.
        "rfc3339" => (!value.is_empty()
            && chrono::DateTime::parse_from_rfc3339(value).is_err())
        .then(|| {
            format!(
                "'{}' is not an RFC 3339 timestamp (e.g. \"2025-01-01T00:00:00Z\")",
                value
            )
        }),
        "date" => (chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_err())
            .then(|| format!("'{}' is not a date in YYYY-MM-DD form", value)),
        "url" => (!is_http_url(value)).then(|| {
            format!(
                "'{}' is not an absolute URL (expected it to start with http:// or https://)",
                value
            )
        }),
        "git-url" => (!is_git_url(value)).then(|| {
            format!(
                "'{}' is not a git remote (expected https://, http://, git://, ssh:// or git@host:path)",
                value
            )
        }),
        "template-name" => invalid_template_name(value),
        _ => None,
    }
}

/// Parse `major.minor.patch` into a comparable tuple.
///
/// Deliberately as strict as the CLI's own compatibility check: exactly three
/// numeric components, so a version that validates here cannot be rejected
/// later when the template's CLI range is evaluated.
fn parse_semver(value: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let major = parts[0].parse().ok()?;
    let minor = parts[1].parse().ok()?;
    let patch = parts[2].parse().ok()?;
    Some((major, minor, patch))
}

fn is_http_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    (lower.starts_with("http://") && lower.len() > "http://".len())
        || (lower.starts_with("https://") && lower.len() > "https://".len())
}

fn is_git_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    is_http_url(value)
        || (lower.starts_with("git://") && lower.len() > "git://".len())
        || (lower.starts_with("ssh://") && lower.len() > "ssh://".len())
        || (lower.starts_with("git@") && lower.contains(':'))
}

/// Template names become directory names under the template store, so a name
/// carrying a path separator would let a registry write outside it.
fn invalid_template_name(value: &str) -> Option<String> {
    if value == "." || value == ".." {
        return Some(format!("'{}' is not a usable template name", value));
    }
    let bad = |c: char| c == '/' || c == '\\' || c.is_whitespace() || c.is_control();
    if value.contains(bad) {
        return Some(format!(
            "'{}' is not a valid template name (path separators, whitespace and control characters are not allowed)",
            value
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_entry() -> Value {
        json!({
            "name": "escrow",
            "version": "1.0.0",
            "description": "Token escrow",
            "author": "StarForge",
            "tags": ["defi"],
            "source": { "type": "builtin", "id": "escrow" }
        })
    }

    fn registry_of(entries: Vec<Value>) -> Value {
        json!({ "version": "1", "templates": entries })
    }

    fn errors(report: &ValidationReport) -> Vec<String> {
        report.errors.iter().map(|e| e.to_string()).collect()
    }

    // ── primary flow ────────────────────────────────────────────────────────

    #[test]
    fn embedded_schema_parses() {
        assert!(registry_schema().get("$defs").is_some());
    }

    #[test]
    fn minimal_valid_registry_passes() {
        let report = validate_registry(&registry_of(vec![valid_entry()]), "test");
        assert!(report.is_valid(), "{:?}", report.errors);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }

    #[test]
    fn fully_populated_entry_passes() {
        let mut entry = valid_entry();
        let map = entry.as_object_mut().unwrap();
        map.insert("categories".into(), json!(["defi", "payments"]));
        map.insert("path".into(), json!("/tmp/escrow"));
        map.insert("downloads".into(), json!(12));
        map.insert("verified".into(), json!(true));
        map.insert("documented".into(), json!(true));
        map.insert("featured".into(), json!(false));
        map.insert("created_at".into(), json!("2025-01-01T00:00:00Z"));
        map.insert("updated_at".into(), json!("2025-06-01T00:00:00Z"));
        map.insert("cli_version_min".into(), json!("0.1.0"));
        map.insert("cli_version_max".into(), json!("1.99.99"));
        map.insert("license".into(), json!("MIT"));
        map.insert("repository".into(), json!("https://github.com/x/y"));
        map.insert("maintenance".into(), json!("active"));
        map.insert(
            "security_review".into(),
            json!({ "status": "audited", "audited_at": "2025-05-20T00:00:00Z",
                    "auditor": "StarForge", "findings": 0, "score": 97 }),
        );
        map.insert(
            "changelog".into(),
            json!([{ "version": "1.0.0", "date": "2025-01-01", "notes": "Initial release" }]),
        );

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert!(report.is_valid(), "{:?}", report.errors);
    }

    #[test]
    fn all_three_source_kinds_pass() {
        let mut git = valid_entry();
        git["name"] = json!("from-git");
        git["source"] =
            json!({ "type": "git", "url": "https://example.com/x.git", "branch": null });
        let mut local = valid_entry();
        local["name"] = json!("from-local");
        local["source"] = json!({ "type": "local", "path": "/srv/templates/x" });

        let report = validate_registry(&registry_of(vec![git, local, valid_entry()]), "test");
        assert!(report.is_valid(), "{:?}", report.errors);
    }

    // ── boundary cases ──────────────────────────────────────────────────────

    #[test]
    fn empty_registry_is_valid() {
        let report = validate_registry(&json!({ "templates": [] }), "test");
        assert!(report.is_valid(), "{:?}", report.errors);
    }

    #[test]
    fn nullable_fields_accept_null() {
        let mut entry = valid_entry();
        entry["path"] = Value::Null;
        entry["cli_version_min"] = Value::Null;
        entry["license"] = Value::Null;
        entry["security_review"] = Value::Null;
        entry["changelog"] = Value::Null;

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert!(report.is_valid(), "{:?}", report.errors);
    }

    #[test]
    fn unset_timestamps_are_allowed_but_malformed_ones_are_not() {
        let mut entry = valid_entry();
        entry["created_at"] = json!("");
        assert!(validate_registry(&registry_of(vec![entry.clone()]), "test").is_valid());

        entry["created_at"] = json!("2025-13-45");
        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(
            errors(&report),
            vec![
                "templates[0].created_at: '2025-13-45' is not an RFC 3339 timestamp (e.g. \"2025-01-01T00:00:00Z\")"
            ]
        );
    }

    #[test]
    fn unknown_fields_warn_instead_of_failing() {
        let mut entry = valid_entry();
        entry["descripton"] = json!("typo");

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert!(report.is_valid(), "{:?}", report.errors);
        assert_eq!(
            report
                .warnings
                .iter()
                .map(|w| w.field.clone())
                .collect::<Vec<_>>(),
            vec!["templates[0].descripton"]
        );
    }

    #[test]
    fn source_branch_fields_are_not_reported_as_unknown() {
        let mut entry = valid_entry();
        entry["source"] =
            json!({ "type": "git", "url": "https://example.com/x.git", "branch": "main" });

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }

    #[test]
    fn version_bounds_may_be_equal() {
        let mut entry = valid_entry();
        entry["cli_version_min"] = json!("1.0.0");
        entry["cli_version_max"] = json!("1.0.0");
        assert!(validate_registry(&registry_of(vec![entry]), "test").is_valid());
    }

    #[test]
    fn same_template_may_appear_at_different_versions() {
        let mut older = valid_entry();
        older["version"] = json!("0.9.0");
        assert!(validate_registry(&registry_of(vec![older, valid_entry()]), "test").is_valid());
    }

    // ── failure cases ───────────────────────────────────────────────────────

    #[test]
    fn missing_required_field_names_that_field() {
        let mut entry = valid_entry();
        entry.as_object_mut().unwrap().remove("source");

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(
            errors(&report),
            vec!["templates[0].source: required field is missing"]
        );
    }

    #[test]
    fn wrong_type_names_the_field_and_both_types() {
        let mut entry = valid_entry();
        entry["tags"] = json!("defi");

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(
            errors(&report),
            vec!["templates[0].tags: expected an array, found a string"]
        );
    }

    #[test]
    fn malformed_semver_is_rejected() {
        let mut entry = valid_entry();
        entry["version"] = json!("v1.2");

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(
            errors(&report),
            vec![
                "templates[0].version: 'v1.2' is not valid semver (expected major.minor.patch, e.g. \"1.2.0\")"
            ]
        );
    }

    #[test]
    fn unknown_maintenance_value_lists_the_allowed_ones() {
        let mut entry = valid_entry();
        entry["maintenance"] = json!("archived");

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(
            errors(&report),
            vec![
                "templates[0].maintenance: 'archived' is not one of: active, maintained, deprecated, unknown"
            ]
        );
    }

    #[test]
    fn unknown_source_type_is_reported_once() {
        let mut entry = valid_entry();
        entry["source"] = json!({ "type": "svn", "url": "svn://example.com" });

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(
            errors(&report),
            vec!["templates[0].source.type: 'svn' is not one of: git, local, builtin"]
        );
    }

    #[test]
    fn git_source_missing_url_is_reported_against_the_url_field() {
        let mut entry = valid_entry();
        entry["source"] = json!({ "type": "git", "branch": "main" });

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(
            errors(&report),
            vec!["templates[0].source.url: required field is missing"]
        );
    }

    #[test]
    fn git_source_rejects_a_non_remote_url() {
        let mut entry = valid_entry();
        entry["source"] = json!({ "type": "git", "url": "not-a-remote" });

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(
            errors(&report),
            vec![
                "templates[0].source.url: 'not-a-remote' is not a git remote (expected https://, http://, git://, ssh:// or git@host:path)"
            ]
        );
    }

    #[test]
    fn a_name_that_escapes_the_template_store_is_rejected() {
        let mut entry = valid_entry();
        entry["name"] = json!("../../etc/passwd");

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(
            errors(&report),
            vec![
                "templates[0].name: '../../etc/passwd' is not a valid template name (path separators, whitespace and control characters are not allowed)"
            ]
        );
    }

    #[test]
    fn empty_name_is_rejected() {
        let mut entry = valid_entry();
        entry["name"] = json!("");

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(
            errors(&report),
            vec!["templates[0].name: must not be empty"]
        );
    }

    #[test]
    fn inverted_cli_version_bounds_are_rejected() {
        let mut entry = valid_entry();
        entry["cli_version_min"] = json!("2.0.0");
        entry["cli_version_max"] = json!("1.0.0");

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(
            errors(&report),
            vec![
                "templates[0].cli_version_max: '1.0.0' is lower than cli_version_min '2.0.0'; the supported range is empty"
            ]
        );
    }

    #[test]
    fn duplicate_name_and_version_is_rejected() {
        let report = validate_registry(&registry_of(vec![valid_entry(), valid_entry()]), "test");
        assert_eq!(
            errors(&report),
            vec![
                "templates[1].name: duplicate entry: 'escrow' version 1.0.0 is already defined at templates[0]"
            ]
        );
    }

    #[test]
    fn out_of_range_audit_score_is_rejected() {
        let mut entry = valid_entry();
        entry["security_review"] = json!({ "status": "audited", "score": 140 });

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(
            errors(&report),
            vec!["templates[0].security_review.score: must be <= 100 (found 140)"]
        );
    }

    #[test]
    fn changelog_date_must_be_iso() {
        let mut entry = valid_entry();
        entry["changelog"] = json!([{ "version": "1.0.0", "date": "01/06/2025", "notes": "x" }]);

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(
            errors(&report),
            vec!["templates[0].changelog[0].date: '01/06/2025' is not a date in YYYY-MM-DD form"]
        );
    }

    #[test]
    fn several_bad_fields_are_all_reported() {
        let mut entry = valid_entry();
        entry["version"] = json!("1.0");
        entry["downloads"] = json!(-3);
        entry.as_object_mut().unwrap().remove("author");

        let report = validate_registry(&registry_of(vec![entry]), "test");
        assert_eq!(report.errors.len(), 3, "{:?}", report.errors);
        let fields: Vec<&str> = report.errors.iter().map(|e| e.field.as_str()).collect();
        assert!(fields.contains(&"templates[0].author"));
        assert!(fields.contains(&"templates[0].version"));
        assert!(fields.contains(&"templates[0].downloads"));
    }

    #[test]
    fn a_registry_without_templates_is_rejected() {
        let report = validate_registry(&json!({ "version": "1" }), "test");
        assert_eq!(
            errors(&report),
            vec!["templates: required field is missing"]
        );
    }

    #[test]
    fn report_renders_origin_and_every_field() {
        let mut entry = valid_entry();
        entry["version"] = json!("nope");
        let report = validate_registry(&registry_of(vec![entry]), "registry.json");

        let rendered = report.to_string();
        assert!(rendered
            .starts_with("registry.json does not match the template registry schema (1 problem)"));
        assert!(rendered.contains("templates[0].version"));
        assert!(report.into_result().is_err());
    }

    #[test]
    fn json_syntax_errors_report_line_and_column() {
        let err = parse_json("{\n  \"templates\": [,]\n}", "broken.json").unwrap_err();
        let message = err.to_string();
        assert!(
            message.starts_with("broken.json is not valid JSON:"),
            "{}",
            message
        );
        assert!(message.contains("line 2"), "{}", message);
    }

    // ── single-entry validation ─────────────────────────────────────────────

    #[test]
    fn a_single_entry_can_be_validated_on_its_own() {
        assert!(validate_template_entry(&valid_entry(), "entry").is_valid());

        let mut bad = valid_entry();
        bad["source"] = json!({ "type": "local" });
        let report = validate_template_entry(&bad, "entry");
        assert_eq!(
            errors(&report),
            vec!["source.path: required field is missing"]
        );
    }
}
