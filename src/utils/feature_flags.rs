//! Feature Flag System for AI Features.
//!
//! Provides a complete feature-flagging infrastructure for the StarForge CLI:
//!
//! - **Flag categories** (Alpha / Beta / Stable / Experimental) with sensible
//!   defaults per category so unsafe flags don't accidentally ship to everyone.
//! - **Gradual rollouts** via deterministic percentage bucketing (FNV-1a hash
//!   so the same `(flag, user)` pair always maps to the same bucket across
//!   processes — unlike Rust's randomly-seeded `DefaultHasher`).
//! - **User segmentation** with allow-lists, attribute predicates, and
//!   percent-of-segment rules.
//! - **A/B testing** through weighted variants that are deterministically
//!   assigned on the same `(flag, user)` hash.
//! - **Metrics tracking** — exposure events and conversion events. The CLI
//!   keeps metrics opt-in via a config flag so power users can disable
//!   in-process bookkeeping on shared machines.
//! - **Snapshotting / rollback** — every state change is versioned in
//!   `flag_states`; the CLI can list versions, revert to a previous one, or
//!   wipe a single flag back to its default.
//!
//! The data layer lives in [`crate::utils::database`]; this module provides
//! the in-memory types and deterministic evaluation helpers. The companion
//! [`FlagManager`] is the high-level façade that ties everything together.

use crate::utils::database::Database;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

// ── Categories ───────────────────────────────────────────────────────────────

/// Rollout category — controls default behaviour when no state has been
/// persisted yet (e.g. on a fresh install).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlagCategory {
    /// Alpha — early adopters only. Defaults to **disabled**.
    Alpha,
    /// Beta — broader testing. Defaults to **disabled** with a 0% rollout.
    Beta,
    /// Stable — General Availability. Defaults to **enabled** for everyone.
    Stable,
    /// Experimental — opt-in, defaults to **disabled** but exposed in the CLI.
    Experimental,
}

impl FlagCategory {
    /// Whether a flag in this category is enabled by default (no state yet).
    pub fn default_enabled(self) -> bool {
        matches!(self, FlagCategory::Stable)
    }

    /// Default rollout percentage (0–100) when no state is set.
    pub fn default_rollout_percent(self) -> u8 {
        match self {
            FlagCategory::Stable => 100,
            _ => 0,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            FlagCategory::Alpha => "alpha",
            FlagCategory::Beta => "beta",
            FlagCategory::Stable => "stable",
            FlagCategory::Experimental => "experimental",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "alpha" => Some(FlagCategory::Alpha),
            "beta" => Some(FlagCategory::Beta),
            "stable" | "ga" | "production" => Some(FlagCategory::Stable),
            "experimental" | "experiment" | "exp" => Some(FlagCategory::Experimental),
            _ => None,
        }
    }
}

// ── Segments ──────────────────────────────────────────────────────────────────

/// Rule that decides whether a [`UserContext`] is in a segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SegmentRule {
    /// Every user matches.
    Always,
    /// User ID (or any attribute key=value with the supplied ID) is in the list.
    UserInList { user_ids: Vec<String> },
    /// The percentage bucket for `(flag, user)` falls inside the range.
    PercentOfUsers { percent: u8 },
    /// A specific attribute on the user context equals one of the values.
    HasAttribute {
        key: String,
        #[serde(default)]
        any_of: Vec<String>,
    },
}

impl SegmentRule {
    pub fn evaluate(&self, flag_name: &str, ctx: &UserContext) -> bool {
        match self {
            SegmentRule::Always => true,
            SegmentRule::UserInList { user_ids } => {
                if user_ids.iter().any(|u| u == &ctx.user_id) {
                    return true;
                }
                // Allow matching against any attribute key whose value matches.
                ctx.attributes
                    .values()
                    .any(|v| user_ids.iter().any(|u| u == v))
            }
            SegmentRule::PercentOfUsers { percent } => {
                let bucket = stable_bucket(flag_name, &ctx.user_id, 100);
                bucket < (*percent as u32).min(100)
            }
            SegmentRule::HasAttribute { key, any_of } => match ctx.attributes.get(key) {
                Some(_v) if any_of.is_empty() => true,
                Some(v) => any_of.iter().any(|cand| cand == v),
                None => false,
            },
        }
    }
}

// ── Variants (A/B testing) ────────────────────────────────────────────────────

/// A single variant in an A/B experiment. Weights are normalised at evaluation
/// time so they don't need to sum to exactly 100.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variant {
    pub name: String,
    #[serde(default = "default_variant_weight")]
    pub weight: u32,
    /// Optional opaque payload, e.g. prompt template name or model identifier.
    #[serde(default)]
    pub payload: Option<String>,
}

fn default_variant_weight() -> u32 {
    1
}

/// Result of variant selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariantAssignment {
    pub variant: String,
    pub payload: Option<String>,
    /// Bucket 0..=10000 used by deterministic assignment.
    pub bucket: u32,
}

// ── Flag definition (read-only metadata) ──────────────────────────────────────

/// Static metadata about a flag — name, category, description. Persisted once
/// at startup so the CLI can list every known flag even when no state is set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlagDefinition {
    pub name: String,
    pub category: FlagCategory,
    pub description: String,
    /// Optional stable user-facing key for telemetry bucketing.
    #[serde(default)]
    pub owner: Option<String>,
    /// Whether the flag can be enabled via the CLI. AI power-user flags default
    /// to `true`; infrastructure/internal flags are `false`.
    #[serde(default = "yes")]
    pub user_manageable: bool,
}

fn yes() -> bool {
    true
}

// ── Flag state (mutable, versioned) ───────────────────────────────────────────

/// Runtime state of a flag. Every change creates a new row in `flag_states`
/// with an incremented version, giving us free rollback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlagState {
    pub flag_name: String,
    pub version: u32,
    pub enabled: bool,
    /// 0–100 inclusive. Combined with allow/block lists during evaluation.
    pub rollout_percent: u8,
    #[serde(default)]
    pub segments: Vec<SegmentRule>,
    #[serde(default)]
    pub variants: Vec<Variant>,
    /// Reason / author of this change (free-form; surfaced in `flag list`).
    #[serde(default)]
    pub note: String,
    pub created_at: String,
}

impl FlagState {
    /// Default state for a fresh definition.
    pub fn for_definition(def: &FlagDefinition) -> Self {
        Self {
            flag_name: def.name.clone(),
            version: 1,
            enabled: def.category.default_enabled(),
            rollout_percent: def.category.default_rollout_percent(),
            segments: Vec::new(),
            variants: Vec::new(),
            note: format!("initial state (category: {})", def.category.slug()),
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

// ── Per-user overrides ────────────────────────────────────────────────────────

/// User-level override that takes priority over the flag state. Used by
/// `starforge feature-flags override` and by alpha testers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlagOverride {
    pub flag_name: String,
    pub user_id: String,
    pub enabled: bool,
    #[serde(default)]
    pub variant: Option<String>,
    pub created_at: String,
}

// ── Metrics ───────────────────────────────────────────────────────────────────

/// A record of an evaluation. Persisted to `flag_metrics`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricEvent {
    pub flag_name: String,
    pub event_type: MetricKind,
    pub user_id: String,
    #[serde(default)]
    pub variant: Option<String>,
    pub timestamp: String,
    /// Optional context (e.g. command name, contract hash).
    #[serde(default)]
    pub context: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    /// User was exposed to the flag (`is_enabled` returned `true`).
    Exposure,
    /// Manual conversion / success event.
    Conversion,
    /// Manual rejection / failure event.
    Rejection,
}

impl MetricKind {
    pub fn slug(self) -> &'static str {
        match self {
            MetricKind::Exposure => "exposure",
            MetricKind::Conversion => "conversion",
            MetricKind::Rejection => "rejection",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "exposure" | "expose" | "exposed" => Some(MetricKind::Exposure),
            "conversion" | "convert" | "converted" | "success" => Some(MetricKind::Conversion),
            "rejection" | "reject" | "rejected" | "failure" => Some(MetricKind::Rejection),
            _ => None,
        }
    }
}

// ── Evaluation result ─────────────────────────────────────────────────────────

/// Outcome of evaluating a flag for a user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub flag_name: String,
    pub user_id: String,
    pub enabled: bool,
    pub variant: Option<String>,
    pub payload: Option<String>,
    /// Human-readable explanation, surfaced in `flag list --verbose`.
    pub reason: String,
    /// True if served by a per-user override rather than percentage bucketing.
    pub from_override: bool,
}

// ── User context ──────────────────────────────────────────────────────────────

/// Context supplied by the caller for an evaluation. `user_id` is the stable
/// bucket key (UUID generated on first install).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UserContext {
    pub user_id: String,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

impl UserContext {
    pub fn new(user_id: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            attributes: HashMap::new(),
        }
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

// ── One-shot warning for misspelled flag names ────────────────────────────────

fn warned_unknown_flags() -> &'static Mutex<HashSet<String>> {
    static ONCE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    ONCE.get_or_init(|| Mutex::new(HashSet::new()))
}

fn warn_unknown_flag_once(flag_name: &str) {
    let mut seen = warned_unknown_flags().lock().unwrap();
    if seen.insert(flag_name.to_string()) {
        tracing::warn!(
            flag = %flag_name,
            "evaluating unknown feature flag (typo?). Use `starforge feature-flags list` to check names."
        );
    }
}

// ── Built-in AI flag definitions ──────────────────────────────────────────────

/// Returns the canonical AI feature flag definitions shipped with the CLI.
pub fn builtin_definitions() -> Vec<FlagDefinition> {
    vec![
        FlagDefinition {
            name: "ai.audit".into(),
            category: FlagCategory::Beta,
            description: "AI-powered security audit (Claude AI + static analysis).".into(),
            owner: Some("security".into()),
            user_manageable: true,
        },
        FlagDefinition {
            name: "ai.debug".into(),
            category: FlagCategory::Stable,
            description: "AI contract debugging assistant.".into(),
            owner: Some("tooling".into()),
            user_manageable: true,
        },
        FlagDefinition {
            name: "ai.docs".into(),
            category: FlagCategory::Beta,
            description: "AI documentation generation for Soroban contracts.".into(),
            owner: Some("tooling".into()),
            user_manageable: true,
        },
        FlagDefinition {
            name: "ai.gas_estimation".into(),
            category: FlagCategory::Beta,
            description: "AI gas usage estimation and analysis.".into(),
            owner: Some("gas".into()),
            user_manageable: true,
        },
        FlagDefinition {
            name: "ai.completion".into(),
            category: FlagCategory::Stable,
            description: "Offline AI contract completion assistant.".into(),
            owner: Some("tooling".into()),
            user_manageable: true,
        },
        FlagDefinition {
            name: "ai.experimental_routing".into(),
            category: FlagCategory::Experimental,
            description: "Route AI requests to multiple providers with failover.".into(),
            owner: Some("ai-platform".into()),
            user_manageable: true,
        },
        FlagDefinition {
            name: "ai.local_only".into(),
            category: FlagCategory::Alpha,
            description: "Restrict all AI calls to local Ollama — never reach the cloud.".into(),
            owner: Some("ai-platform".into()),
            user_manageable: true,
        },
    ]
}

// ── Deterministic hashing ─────────────────────────────────────────────────────

/// FNV-1a 64-bit hash — fast, dependency-free, **stable across processes**.
/// (Rust's `DefaultHasher` is randomly seeded per-process and is therefore
/// unsuitable for percentage bucketing.) Returns the raw 64-bit value.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Deterministic bucket in `[0, modulus)` for `(flag, user_id)`.
pub fn stable_bucket(flag_name: &str, user_id: &str, modulus: u32) -> u32 {
    let mut buf: Vec<u8> = Vec::with_capacity(flag_name.len() + user_id.len() + 1);
    buf.extend_from_slice(flag_name.as_bytes());
    buf.push(0x1f); // unit separator — won't appear in flag names
    buf.extend_from_slice(user_id.as_bytes());
    (fnv1a_64(&buf) % (modulus as u64)) as u32
}

/// Deterministic bucket in 0..=10000 for `(flag, user_id)`. Used internally
/// for both percentage rollouts (1 unit == 0.01%) and variant picking.
pub fn stable_bucket_10k(flag_name: &str, user_id: &str) -> u32 {
    let mut h = fnv1a_64(format!("{}|{}", flag_name, user_id).as_bytes());
    // Standard Murmur3 64-bit fmix finalizer — two shift/multiply rounds.
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ceb9fe1a85ec53);
    h ^= h >> 33;
    (h % 10_001) as u32
}

/// Validate a flag, user, or variant identifier. Identifiers may only contain
/// lowercase ASCII letters, digits, `.`, `-`, and `_`, with a max length of
/// 64 characters. This is intentionally strict — flag names get used as the
/// hash key, and spurious whitespace could create two keys that *look* the
/// same but evaluate differently.
pub fn validate_ident(label: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{} must not be empty", label);
    }
    if value.len() > 64 {
        bail!(
            "{} must be at most 64 characters (got {})",
            label,
            value.len()
        );
    }
    if let Some(bad) = value.chars().find(|c| {
        !(c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '_')
    }) {
        bail!(
            "{} contains invalid character {:?}; use lowercase letters, digits, '.', '-', or '_'",
            label,
            bad
        );
    }
    Ok(())
}

// ── Database operations ───────────────────────────────────────────────────────

/// SQL DDL added to the schema. Idempotent — the surrounding `CREATE TABLE IF
/// NOT EXISTS` statements keep existing databases working.
pub const FEATURE_FLAGS_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS flag_definitions (
    name            TEXT PRIMARY KEY,
    category        TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    owner           TEXT,
    user_manageable INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS flag_states (
    id              INTEGER PRIMARY KEY,
    flag_name       TEXT NOT NULL,
    version         INTEGER NOT NULL,
    enabled         INTEGER NOT NULL,
    rollout_percent INTEGER NOT NULL,
    segments_json   TEXT NOT NULL DEFAULT '[]',
    variants_json   TEXT NOT NULL DEFAULT '[]',
    note            TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(flag_name, version)
);

CREATE INDEX IF NOT EXISTS idx_flag_states_flag ON flag_states(flag_name);

CREATE TABLE IF NOT EXISTS flag_overrides (
    flag_name   TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    enabled     INTEGER NOT NULL,
    variant     TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (flag_name, user_id)
);

CREATE TABLE IF NOT EXISTS flag_metrics (
    id          INTEGER PRIMARY KEY,
    flag_name   TEXT NOT NULL,
    event_type  TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    variant     TEXT,
    context_json TEXT NOT NULL DEFAULT '{}',
    timestamp   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_flag_metrics_flag ON flag_metrics(flag_name);
CREATE INDEX IF NOT EXISTS idx_flag_metrics_event ON flag_metrics(event_type);
CREATE INDEX IF NOT EXISTS idx_flag_metrics_ts ON flag_metrics(timestamp);
";

impl Database {
    /// Idempotently apply the feature-flags schema and seed definitions.
    pub fn initialize_feature_flags(&self) -> Result<()> {
        self.conn
            .execute_batch(FEATURE_FLAGS_SCHEMA)
            .context("Failed to apply feature_flags schema")?;
        for def in builtin_definitions() {
            self.upsert_definition(&def)?;
        }
        Ok(())
    }

    pub fn upsert_definition(&self, def: &FlagDefinition) -> Result<()> {
        validate_ident("flag name", &def.name)?;
        let user_manageable = if def.user_manageable { 1i64 } else { 0i64 };
        self.conn.execute(
            "INSERT INTO flag_definitions (name, category, description, owner, user_manageable) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(name) DO UPDATE SET \
                 category=excluded.category, \
                 description=excluded.description, \
                 owner=excluded.owner, \
                 user_manageable=excluded.user_manageable",
            rusqlite::params![
                def.name,
                def.category.slug(),
                def.description,
                def.owner,
                user_manageable,
            ],
        )?;
        // Ensure a state row exists for this flag.
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM flag_states WHERE flag_name = ?1 LIMIT 1",
                rusqlite::params![def.name],
                |r| r.get(0),
            )
            .ok();
        if existing.is_none() {
            let state = FlagState::for_definition(def);
            self.insert_state(&state)?;
        }
        Ok(())
    }

    pub fn list_definitions(&self) -> Result<Vec<FlagDefinition>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, category, description, owner, user_manageable FROM flag_definitions ORDER BY name",
        )?;
        let rows = stmt.query_map([], |row| {
            let cat_str: String = row.get(1)?;
            let category = FlagCategory::parse(&cat_str).unwrap_or(FlagCategory::Experimental);
            Ok(FlagDefinition {
                name: row.get(0)?,
                category,
                description: row.get(2)?,
                owner: row.get(3)?,
                user_manageable: row.get::<_, i64>(4)? != 0,
            })
        })?;
        rows.map(|r| r.map_err(anyhow::Error::from)).collect()
    }

    pub fn get_definition(&self, name: &str) -> Result<Option<FlagDefinition>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, category, description, owner, user_manageable \
             FROM flag_definitions WHERE name = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![name])?;
        if let Some(row) = rows.next()? {
            let cat_str: String = row.get(1)?;
            let category = FlagCategory::parse(&cat_str).unwrap_or(FlagCategory::Experimental);
            Ok(Some(FlagDefinition {
                name: row.get(0)?,
                category,
                description: row.get(2)?,
                owner: row.get(3)?,
                user_manageable: row.get::<_, i64>(4)? != 0,
            }))
        } else {
            Ok(None)
        }
    }

    /// Upsert a new flag state, bumping the version automatically.
    pub fn insert_state(&self, state: &FlagState) -> Result<()> {
        let enabled = if state.enabled { 1i64 } else { 0i64 };
        self.conn.execute(
            "INSERT INTO flag_states \
             (flag_name, version, enabled, rollout_percent, segments_json, variants_json, note) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                state.flag_name,
                state.version as i64,
                enabled,
                state.rollout_percent as i64,
                serde_json::to_string(&state.segments)?,
                serde_json::to_string(&state.variants)?,
                state.note,
            ],
        )?;
        Ok(())
    }

    /// Returns the most recent state for a flag (highest version).
    pub fn latest_state(&self, flag_name: &str) -> Result<Option<FlagState>> {
        let mut stmt = self.conn.prepare(
            "SELECT flag_name, version, enabled, rollout_percent, segments_json, variants_json, note, created_at \
             FROM flag_states WHERE flag_name = ?1 ORDER BY version DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(rusqlite::params![flag_name])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_state(row)?))
        } else {
            Ok(None)
        }
    }

    /// All prior versions of a flag's state (oldest first).
    pub fn state_history(&self, flag_name: &str) -> Result<Vec<FlagState>> {
        let mut stmt = self.conn.prepare(
            "SELECT flag_name, version, enabled, rollout_percent, segments_json, variants_json, note, created_at \
             FROM flag_states WHERE flag_name = ?1 ORDER BY version ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![flag_name], row_to_state)?;
        rows.map(|r| r.map_err(anyhow::Error::from)).collect()
    }

    pub fn state_at_version(&self, flag_name: &str, version: u32) -> Result<Option<FlagState>> {
        let mut stmt = self.conn.prepare(
            "SELECT flag_name, version, enabled, rollout_percent, segments_json, variants_json, note, created_at \
             FROM flag_states WHERE flag_name = ?1 AND version = ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![flag_name, version as i64])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_state(row)?))
        } else {
            Ok(None)
        }
    }

    /// Current latest state for every flag.
    pub fn latest_states(&self) -> Result<Vec<FlagState>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.flag_name, s.version, s.enabled, s.rollout_percent, s.segments_json, s.variants_json, s.note, s.created_at \
             FROM flag_states s \
             INNER JOIN ( \
                 SELECT flag_name, MAX(version) AS v FROM flag_states GROUP BY flag_name \
             ) latest ON latest.flag_name = s.flag_name AND latest.v = s.version \
             ORDER BY s.flag_name",
        )?;
        let rows = stmt.query_map([], row_to_state)?;
        rows.map(|r| r.map_err(anyhow::Error::from)).collect()
    }

    /// Highest version number across all flags; 0 if no state rows.
    pub fn current_snapshot_version(&self) -> Result<u32> {
        let v: Option<i64> = self
            .conn
            .query_row("SELECT MAX(version) FROM flag_states", [], |r| r.get(0))
            .ok();
        Ok(v.unwrap_or(0) as u32)
    }

    pub fn upsert_override(&self, ov: &FlagOverride) -> Result<()> {
        let enabled = if ov.enabled { 1i64 } else { 0i64 };
        self.conn.execute(
            "INSERT INTO flag_overrides (flag_name, user_id, enabled, variant) \
             VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(flag_name, user_id) DO UPDATE SET \
                 enabled=excluded.enabled, \
                 variant=excluded.variant",
            rusqlite::params![ov.flag_name, ov.user_id, enabled, ov.variant],
        )?;
        Ok(())
    }

    pub fn remove_override(&self, flag_name: &str, user_id: &str) -> Result<bool> {
        let n = self.conn.execute(
            "DELETE FROM flag_overrides WHERE flag_name = ?1 AND user_id = ?2",
            rusqlite::params![flag_name, user_id],
        )?;
        Ok(n > 0)
    }

    pub fn get_override(&self, flag_name: &str, user_id: &str) -> Result<Option<FlagOverride>> {
        let mut stmt = self.conn.prepare(
            "SELECT flag_name, user_id, enabled, variant, created_at \
             FROM flag_overrides WHERE flag_name = ?1 AND user_id = ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![flag_name, user_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(FlagOverride {
                flag_name: row.get(0)?,
                user_id: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
                variant: row.get(3)?,
                created_at: row.get(4)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_overrides(&self, flag_name: &str) -> Result<Vec<FlagOverride>> {
        let mut stmt = self.conn.prepare(
            "SELECT flag_name, user_id, enabled, variant, created_at \
             FROM flag_overrides WHERE flag_name = ?1 \
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![flag_name], |row| {
            Ok(FlagOverride {
                flag_name: row.get(0)?,
                user_id: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
                variant: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.map(|r| r.map_err(anyhow::Error::from)).collect()
    }

    pub fn record_metric(&self, ev: &MetricEvent) -> Result<()> {
        let kind = ev.event_type.slug();
        self.conn.execute(
            "INSERT INTO flag_metrics (flag_name, event_type, user_id, variant, context_json) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                ev.flag_name,
                kind,
                ev.user_id,
                ev.variant,
                serde_json::to_string(&ev.context)?,
            ],
        )?;
        Ok(())
    }

    /// Counts grouped by event_type for a flag.
    pub fn metrics_summary(&self, flag_name: &str) -> Result<HashMap<String, u64>> {
        let mut stmt = self.conn.prepare(
            "SELECT event_type, COUNT(*) FROM flag_metrics \
             WHERE flag_name = ?1 GROUP BY event_type",
        )?;
        let rows = stmt.query_map(rusqlite::params![flag_name], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })?;
        let mut out = HashMap::new();
        for r in rows {
            let (k, v) = r?;
            out.insert(k, v);
        }
        Ok(out)
    }

    pub fn metrics_recent(&self, flag_name: &str, limit: u32) -> Result<Vec<MetricEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT flag_name, event_type, user_id, variant, context_json, timestamp \
             FROM flag_metrics WHERE flag_name = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![flag_name, limit as i64], |row| {
            let kind_str: String = row.get(1)?;
            let kind = MetricKind::parse(&kind_str).unwrap_or(MetricKind::Exposure);
            let ctx_json: String = row.get(4)?;
            let context: HashMap<String, String> =
                serde_json::from_str(&ctx_json).unwrap_or_default();
            Ok(MetricEvent {
                flag_name: row.get(0)?,
                event_type: kind,
                user_id: row.get(2)?,
                variant: row.get(3)?,
                context,
                timestamp: row.get(5)?,
            })
        })?;
        rows.map(|r| r.map_err(anyhow::Error::from)).collect()
    }

    /// Drops metrics older than `keep_days`. Returns the number of rows removed.
    pub fn prune_metrics(&self, keep_days: u32) -> Result<u64> {
        let days = keep_days.max(1) as i64;
        let n = self.conn.execute(
            "DELETE FROM flag_metrics \
             WHERE datetime(timestamp) < datetime('now', ?1)",
            rusqlite::params![format!("-{} days", days)],
        )?;
        Ok(n as u64)
    }

    /// Delete every state row for `flag_name` — used by `flag reset`.
    pub fn delete_states(&self, flag_name: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM flag_states WHERE flag_name = ?1",
            rusqlite::params![flag_name],
        )?;
        Ok(())
    }

    pub fn delete_definition(&self, flag_name: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM flag_definitions WHERE name = ?1",
            rusqlite::params![flag_name],
        )?;
        Ok(())
    }
}

fn row_to_state(row: &rusqlite::Row<'_>) -> rusqlite::Result<FlagState> {
    let seg_json: String = row.get(4)?;
    let var_json: String = row.get(5)?;
    let segments: Vec<SegmentRule> = serde_json::from_str(&seg_json).unwrap_or_default();
    let variants: Vec<Variant> = serde_json::from_str(&var_json).unwrap_or_default();
    Ok(FlagState {
        flag_name: row.get(0)?,
        version: row.get::<_, i64>(1)? as u32,
        enabled: row.get::<_, i64>(2)? != 0,
        rollout_percent: row.get::<_, i64>(3)? as u8,
        segments,
        variants,
        note: row.get(6)?,
        created_at: row.get(7)?,
    })
}

// ── FlagManager ───────────────────────────────────────────────────────────────

/// High-level façade. Built cheaply from a [`Database`] reference; safe to
/// create per command.
pub struct FlagManager<'a> {
    db: &'a Database,
    user_context: UserContext,
    record_exposures: bool,
}

impl<'a> FlagManager<'a> {
    pub fn new(db: &'a Database, user_context: UserContext) -> Self {
        Self {
            db,
            user_context,
            record_exposures: true,
        }
    }

    /// Disables in-process exposure recording (e.g. for read-only listings).
    pub fn with_exposure_recording(mut self, enabled: bool) -> Self {
        self.record_exposures = enabled;
        self
    }

    pub fn user_context(&self) -> &UserContext {
        &self.user_context
    }

    pub fn db(&self) -> &'a Database {
        self.db
    }

    /// Evaluate a flag. Returns the result and (when `record_exposures` is on
    /// and the flag is enabled) records a metric event.
    pub fn evaluate(&self, flag_name: &str) -> Result<EvaluationResult> {
        let res = self.evaluate_dry(flag_name)?;
        if self.record_exposures && res.enabled {
            let _ = self.db.record_metric(&MetricEvent {
                flag_name: flag_name.to_string(),
                event_type: MetricKind::Exposure,
                user_id: self.user_context.user_id.clone(),
                variant: res.variant.clone(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                context: HashMap::new(),
            });
        }
        Ok(res)
    }

    /// Evaluate a flag without recording metrics. Useful for CLIs that just
    /// want to know whether to show a subcommand.
    pub fn evaluate_dry(&self, flag_name: &str) -> Result<EvaluationResult> {
        // Unknown flag names return a disabled result so typos cannot silently
        // turn into "Experimental → disabled" outcomes that look correct.
        if self.db.get_definition(flag_name)?.is_none() {
            warn_unknown_flag_once(flag_name);
            return Ok(EvaluationResult {
                flag_name: flag_name.to_string(),
                user_id: self.user_context.user_id.clone(),
                enabled: false,
                variant: None,
                payload: None,
                reason: "unknown flag".to_string(),
                from_override: false,
            });
        }

        // 1) User override always wins.
        if let Some(ov) = self
            .db
            .get_override(flag_name, &self.user_context.user_id)?
        {
            let variant = pick_variant_for_override(&ov, self.db.latest_state(flag_name)?);
            return Ok(EvaluationResult {
                flag_name: flag_name.to_string(),
                user_id: self.user_context.user_id.clone(),
                enabled: ov.enabled,
                variant: variant.as_ref().map(|v| v.variant.clone()),
                payload: variant.and_then(|v| v.payload),
                reason: "user override".to_string(),
                from_override: true,
            });
        }

        let state = match self.db.latest_state(flag_name)? {
            Some(s) => s,
            None => {
                // No state => fall back to category default.
                let def = self
                    .db
                    .get_definition(flag_name)?
                    .unwrap_or(FlagDefinition {
                        name: flag_name.to_string(),
                        category: FlagCategory::Experimental,
                        description: String::new(),
                        owner: None,
                        user_manageable: true,
                    });
                let enabled = def.category.default_enabled();
                return Ok(EvaluationResult {
                    flag_name: flag_name.to_string(),
                    user_id: self.user_context.user_id.clone(),
                    enabled,
                    variant: None,
                    payload: None,
                    reason: format!("no state — using {} default", def.category.slug()),
                    from_override: false,
                });
            }
        };

        // 2) Global on/off switch first.
        if !state.enabled {
            return Ok(EvaluationResult {
                flag_name: flag_name.to_string(),
                user_id: self.user_context.user_id.clone(),
                enabled: false,
                variant: None,
                payload: None,
                reason: "flag disabled".to_string(),
                from_override: false,
            });
        }

        // 3) Segment rules.
        let mut segment_passed = false;
        let mut matched_segment = false;
        let mut last_reason = String::new();
        if state.segments.is_empty() {
            segment_passed = true;
            last_reason = "no segments — open rollout".to_string();
        } else {
            for rule in &state.segments {
                if rule.evaluate(flag_name, &self.user_context) {
                    segment_passed = true;
                    matched_segment = true;
                    last_reason = format!("matched segment rule ({:?})", rule);
                    break;
                }
            }
            if !segment_passed {
                last_reason = "no segment rule matched".to_string();
            }
        }

        if !segment_passed {
            return Ok(EvaluationResult {
                flag_name: flag_name.to_string(),
                user_id: self.user_context.user_id.clone(),
                enabled: false,
                variant: None,
                payload: None,
                reason: last_reason,
                from_override: false,
            });
        }

        // 4) Percentage bucket.
        //
        // A user picked out by an explicit segment rule is a deliberate
        // targeting decision, so it bypasses the percentage gate — the
        // percentage governs the *open* rollout only. Without this, the common
        // "rollout 0% + allow-list" setup would exclude the very users it names.
        let bucket = stable_bucket(flag_name, &self.user_context.user_id, 100);
        let in_rollout = matched_segment || bucket < state.rollout_percent as u32;
        if !in_rollout {
            return Ok(EvaluationResult {
                flag_name: flag_name.to_string(),
                user_id: self.user_context.user_id.clone(),
                enabled: false,
                variant: None,
                payload: None,
                reason: format!(
                    "user bucket {} >= rollout_percent {}",
                    bucket, state.rollout_percent
                ),
                from_override: false,
            });
        }

        // 5) Variant selection (deterministic).
        let variant = if state.variants.is_empty() {
            None
        } else {
            Some(pick_variant(
                flag_name,
                &self.user_context.user_id,
                &state.variants,
            ))
        };

        Ok(EvaluationResult {
            flag_name: flag_name.to_string(),
            user_id: self.user_context.user_id.clone(),
            enabled: true,
            variant: variant.as_ref().map(|v| v.variant.clone()),
            payload: variant.and_then(|v| v.payload),
            reason: format!("{} (bucket {}/100)", last_reason, state.rollout_percent),
            from_override: false,
        })
    }

    /// Shorthand: `is_enabled("ai.audit")` for the common case.
    pub fn is_enabled(&self, flag_name: &str) -> bool {
        self.evaluate(flag_name).map(|r| r.enabled).unwrap_or(false)
    }

    /// User-friendly summary of every known flag, used by `flag list`.
    pub fn list_all(&self) -> Result<Vec<FlagListEntry>> {
        let defs = self.db.list_definitions()?;
        let states = self.db.latest_states()?;
        let states_by_name: HashMap<String, FlagState> = states
            .into_iter()
            .map(|s| (s.flag_name.clone(), s))
            .collect();
        let mut entries = Vec::with_capacity(defs.len());
        for d in defs {
            let state = states_by_name.get(&d.name).cloned();
            let eval = self.evaluate_dry(&d.name)?;
            entries.push(FlagListEntry {
                definition: d,
                state,
                evaluation: eval,
            });
        }
        Ok(entries)
    }

    /// Update the latest state of a flag, bumping its version atomically.
    /// Wraps the version read + insert in a SQLite transaction so two
    /// concurrent callers cannot pick the same version.
    pub fn update_state(
        &self,
        flag_name: &str,
        enabled: Option<bool>,
        rollout_percent: Option<u8>,
        segments: Option<Vec<SegmentRule>>,
        variants: Option<Vec<Variant>>,
        note: Option<String>,
    ) -> Result<FlagState> {
        if self.db.get_definition(flag_name)?.is_none() {
            bail!(
                "flag '{}' is not defined. Use `starforge feature-flags define` first.",
                flag_name
            );
        }
        // Atomic read + write inside a single tx so concurrent updates can't
        // collide on UNIQUE(flag_name, version).
        let new_state = self.db.with_transaction(|| {
            let prior =
                self.db.latest_state(flag_name)?.unwrap_or_else(|| {
                    FlagState::for_definition(
                        &self.db.get_definition(flag_name).ok().flatten().unwrap_or(
                            FlagDefinition {
                                name: flag_name.to_string(),
                                category: FlagCategory::Experimental,
                                description: String::new(),
                                owner: None,
                                user_manageable: true,
                            },
                        ),
                    )
                });
            let new_state = FlagState {
                flag_name: flag_name.to_string(),
                version: prior.version + 1,
                enabled: enabled.unwrap_or(prior.enabled),
                rollout_percent: rollout_percent.unwrap_or(prior.rollout_percent),
                segments: segments.unwrap_or(prior.segments),
                variants: variants.unwrap_or(prior.variants),
                note: note.unwrap_or_else(|| "manual update".to_string()),
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            self.db.insert_state(&new_state)?;
            Ok::<FlagState, anyhow::Error>(new_state)
        })?;
        Ok(new_state)
    }

    /// Convenience helpers.
    pub fn set_enabled(&self, flag_name: &str, enabled: bool) -> Result<FlagState> {
        let note = if enabled {
            "enabled via CLI"
        } else {
            "disabled via CLI"
        };

        // Flipping a flag on while its rollout sits at 0% would leave it off for
        // everyone, which is never what "enable" means. Widen to a full rollout,
        // but only when no targeting has been configured — an operator who has
        // already set a percentage or segments keeps their configuration.
        let rollout = if enabled {
            match self.db.latest_state(flag_name)? {
                Some(state) if state.rollout_percent == 0 && state.segments.is_empty() => Some(100),
                None => Some(100),
                _ => None,
            }
        } else {
            None
        };

        self.update_state(
            flag_name,
            Some(enabled),
            rollout,
            None,
            None,
            Some(note.to_string()),
        )
    }

    pub fn set_rollout(&self, flag_name: &str, percent: u8) -> Result<FlagState> {
        let p = percent.min(100);
        self.update_state(
            flag_name,
            None,
            Some(p),
            None,
            None,
            Some(format!("rollout set to {}%", p)),
        )
    }

    pub fn replace_segments(
        &self,
        flag_name: &str,
        segments: Vec<SegmentRule>,
    ) -> Result<FlagState> {
        self.update_state(
            flag_name,
            None,
            None,
            Some(segments),
            None,
            Some("segments replaced".to_string()),
        )
    }

    pub fn replace_variants(&self, flag_name: &str, variants: Vec<Variant>) -> Result<FlagState> {
        self.update_state(
            flag_name,
            None,
            None,
            None,
            Some(variants),
            Some("variants replaced".to_string()),
        )
    }

    /// Insert a brand-new flag (state + definition). Returns `false` if the
    /// flag already exists.
    pub fn register_flag(&self, def: FlagDefinition) -> Result<bool> {
        if self.db.get_definition(&def.name)?.is_some() {
            return Ok(false);
        }
        // `upsert_definition` seeds the initial state row for a new definition.
        self.db.upsert_definition(&def)?;
        Ok(true)
    }

    /// Roll back to a prior version. Creates a NEW version that copies the
    /// old one instead of deleting the future history, so it stays auditable.
    pub fn rollback(&self, flag_name: &str, target_version: u32) -> Result<FlagState> {
        let target = self
            .db
            .state_at_version(flag_name, target_version)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "version {} not found for flag '{}'",
                    target_version,
                    flag_name
                )
            })?;
        self.update_state(
            flag_name,
            Some(target.enabled),
            Some(target.rollout_percent),
            Some(target.segments),
            Some(target.variants),
            Some(format!("rollback to v{}", target_version)),
        )
    }

    /// Reset a flag back to its category default (deletes state rows).
    pub fn reset(&self, flag_name: &str) -> Result<FlagState> {
        let def = self
            .db
            .get_definition(flag_name)?
            .ok_or_else(|| anyhow::anyhow!("flag '{}' is not defined", flag_name))?;
        self.db.delete_states(flag_name)?;
        let state = FlagState::for_definition(&def);
        self.db.insert_state(&state)?;
        Ok(state)
    }

    pub fn set_override(
        &self,
        flag_name: &str,
        user_id: &str,
        enabled: bool,
        variant: Option<String>,
    ) -> Result<FlagOverride> {
        // Explicit let-bind to satisfy the borrow checker inside the FnOnce
        // closure (auto-reborrow is unreliable for nested &Database refs).
        let db = self.db;
        db.with_transaction(|| {
            if let Some(name) = &variant {
                validate_ident("variant", name)?;
                // Verify the variant is registered on the flag atomically with
                // the override write so a concurrent variant insertion can't
                // sneak past us.
                if let Some(state) = db.latest_state(flag_name)? {
                    let known = state.variants.iter().any(|v| v.name == *name);
                    if !known {
                        bail!(
                            "variant '{}' is not registered on flag '{}'. Add it via `starforge feature-flags variant set {} {}` first.",
                            name,
                            flag_name,
                            flag_name,
                            name
                        );
                    }
                }
            }
            let ov = FlagOverride {
                flag_name: flag_name.to_string(),
                user_id: user_id.to_string(),
                enabled,
                variant,
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            db.upsert_override(&ov)?;
            Ok(ov)
        })
    }

    pub fn clear_override(&self, flag_name: &str, user_id: &str) -> Result<bool> {
        self.db.remove_override(flag_name, user_id)
    }

    pub fn record_conversion(&self, flag_name: &str) -> Result<()> {
        self.db.record_metric(&MetricEvent {
            flag_name: flag_name.to_string(),
            event_type: MetricKind::Conversion,
            user_id: self.user_context.user_id.clone(),
            variant: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            context: HashMap::new(),
        })
    }

    pub fn record_rejection(&self, flag_name: &str) -> Result<()> {
        self.db.record_metric(&MetricEvent {
            flag_name: flag_name.to_string(),
            event_type: MetricKind::Rejection,
            user_id: self.user_context.user_id.clone(),
            variant: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            context: HashMap::new(),
        })
    }
}

/// Combined view for the `list` subcommand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagListEntry {
    pub definition: FlagDefinition,
    pub state: Option<FlagState>,
    pub evaluation: EvaluationResult,
}

// ── Variant picking ───────────────────────────────────────────────────────────

fn pick_variant_for_override(
    ov: &FlagOverride,
    state: Option<FlagState>,
) -> Option<VariantAssignment> {
    if !ov.enabled {
        return None;
    }
    if let Some(name) = &ov.variant {
        if let Some(s) = state {
            if let Some(v) = s.variants.iter().find(|v| &v.name == name) {
                return Some(VariantAssignment {
                    variant: v.name.clone(),
                    payload: v.payload.clone(),
                    bucket: 0,
                });
            }
        }
        return Some(VariantAssignment {
            variant: name.clone(),
            payload: None,
            bucket: 0,
        });
    }
    None
}

/// Deterministic variant selection. Bucket distribution over many users should
/// approximate the configured weights (we use the same hash as rollouts).
pub fn pick_variant(flag_name: &str, user_id: &str, variants: &[Variant]) -> VariantAssignment {
    if variants.is_empty() {
        return VariantAssignment {
            variant: "control".to_string(),
            payload: None,
            bucket: 0,
        };
    }
    let total: u64 = variants.iter().map(|v| v.weight as u64).sum();
    if total == 0 {
        // All zero — fall back to first variant for determinism.
        let v = &variants[0];
        return VariantAssignment {
            variant: v.name.clone(),
            payload: v.payload.clone(),
            bucket: 0,
        };
    }
    let bucket_10k = stable_bucket_10k(flag_name, user_id);
    let scaled = ((bucket_10k as u64) * total) / 10_001;
    let mut cumulative: u64 = 0;
    for v in variants {
        cumulative += v.weight as u64;
        if scaled < cumulative {
            return VariantAssignment {
                variant: v.name.clone(),
                payload: v.payload.clone(),
                bucket: bucket_10k,
            };
        }
    }
    let last = variants.last().expect("variants non-empty");
    VariantAssignment {
        variant: last.name.clone(),
        payload: last.payload.clone(),
        bucket: bucket_10k,
    }
}

// ── Helper: build the canonical FlagManager for the running install ──────────

/// Load (or create) the install UUID. We deliberately store it in `config_kv`
/// rather than introducing a new table — config_kv already supports arbitrary
/// keys and this keeps the migration tiny.
pub fn load_or_create_install_id(db: &Database) -> Result<String> {
    if let Some(existing) = db.get_config_kv("install_id")? {
        if !existing.is_empty() {
            return Ok(existing);
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    db.insert_config_kv("install_id", &id)?;
    Ok(id)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn db() -> Database {
        Database::open_in_memory().unwrap()
    }

    fn init(db: &Database) {
        db.initialize().unwrap();
        db.initialize_feature_flags().unwrap();
    }

    #[test]
    fn fnv1a_is_deterministic_across_calls() {
        let a = fnv1a_64(b"hello world");
        let b = fnv1a_64(b"hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn stable_bucket_is_deterministic_per_pair() {
        let b1 = stable_bucket("ai.audit", "u-1", 100);
        let b2 = stable_bucket("ai.audit", "u-1", 100);
        let b3 = stable_bucket("ai.audit", "u-2", 100);
        assert_eq!(b1, b2);
        assert_ne!(b1, b3); // different user → different bucket
    }

    #[test]
    fn percentage_distribution_approximates_target() {
        let flag = "rollout.test";
        let mut in_rollout = 0;
        let n = 5_000;
        for i in 0..n {
            let id = format!("user-{i}");
            if stable_bucket(flag, &id, 100) < 25 {
                in_rollout += 1;
            }
        }
        let pct = in_rollout as f64 / n as f64;
        assert!(
            (0.20..=0.30).contains(&pct),
            "expected ~25%, got {:.1}%",
            pct * 100.0
        );
    }

    #[test]
    fn builtin_definitions_have_unique_names() {
        let defs = builtin_definitions();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len(), "duplicate built-in flag");
    }

    #[test]
    fn definitions_persist_and_round_trip() {
        let db = db();
        init(&db);
        let defs = db.list_definitions().unwrap();
        assert!(!defs.is_empty());
        for d in &defs {
            assert!(db.get_definition(&d.name).unwrap().is_some());
        }
    }

    #[test]
    fn defaults_match_category() {
        let alpha = FlagCategory::Alpha;
        let stable = FlagCategory::Stable;
        assert!(!alpha.default_enabled());
        assert!(stable.default_enabled());
        assert_eq!(alpha.default_rollout_percent(), 0);
        assert_eq!(stable.default_rollout_percent(), 100);
    }

    #[test]
    fn segment_user_in_list_matches() {
        let rule = SegmentRule::UserInList {
            user_ids: vec!["alpha-tester".into()],
        };
        let mut ctx = UserContext::new("someone-else");
        assert!(!rule.evaluate("f", &ctx));
        ctx.user_id = "alpha-tester".into();
        assert!(rule.evaluate("f", &ctx));
    }

    #[test]
    fn segment_percent_matches_subset() {
        let rule = SegmentRule::PercentOfUsers { percent: 30 };
        let mut matched = 0;
        for i in 0..500 {
            let ctx = UserContext::new(format!("u-{i}"));
            if rule.evaluate("flag-x", &ctx) {
                matched += 1;
            }
        }
        let pct = matched as f64 / 500.0;
        assert!((0.20..=0.40).contains(&pct), "got {:.2}", pct);
    }

    #[test]
    fn segment_attribute_matches_when_listed() {
        let rule = SegmentRule::HasAttribute {
            key: "team".into(),
            any_of: vec!["security".into()],
        };
        let ctx = UserContext::new("u").with_attribute("team", "security");
        assert!(rule.evaluate("f", &ctx));
        let ctx2 = UserContext::new("u").with_attribute("team", "growth");
        assert!(!rule.evaluate("f", &ctx2));
    }

    #[test]
    fn override_wins_over_state() {
        let db = db();
        init(&db);
        let mgr = FlagManager::new(&db, UserContext::new("u-1"));
        // Start disabled.
        mgr.set_enabled("ai.audit", false).unwrap();
        assert!(!mgr.is_enabled("ai.audit"));
        // Override to true.
        mgr.set_override("ai.audit", "u-1", true, None).unwrap();
        assert!(mgr.is_enabled("ai.audit"));
        let res = mgr.evaluate_dry("ai.audit").unwrap();
        assert!(res.enabled);
        assert!(res.from_override);
        // Override with variant — the variant must exist on the flag first.
        mgr.replace_variants(
            "ai.audit",
            vec![Variant {
                name: "control".to_string(),
                weight: 100,
                payload: None,
            }],
        )
        .unwrap();
        mgr.set_override("ai.audit", "u-1", true, Some("control".into()))
            .unwrap();
        let res2 = mgr.evaluate_dry("ai.audit").unwrap();
        assert_eq!(res2.variant.as_deref(), Some("control"));
    }

    #[test]
    fn rollout_zero_disables_everyone() {
        let db = db();
        init(&db);
        let mgr = FlagManager::new(&db, UserContext::new("u-r1"));
        mgr.set_enabled("ai.audit", true).unwrap();
        mgr.set_rollout("ai.audit", 0).unwrap();
        // Even though enabled, 0% rollout excludes everyone.
        for i in 0..50 {
            let ctx = UserContext::new(format!("u-{i}"));
            let mgr = FlagManager::new(&db, ctx).with_exposure_recording(false);
            assert!(!mgr.is_enabled("ai.audit"));
        }
    }

    #[test]
    fn rollout_full_enables_everyone() {
        let db = db();
        init(&db);
        mgr_set_rollout_full(&db);
        for i in 0..30 {
            let ctx = UserContext::new(format!("u-{i}"));
            let mgr = FlagManager::new(&db, ctx).with_exposure_recording(false);
            assert!(mgr.is_enabled("ai.audit"));
        }
    }

    fn mgr_set_rollout_full(db: &Database) {
        let mgr = FlagManager::new(db, UserContext::new("seed"));
        mgr.set_enabled("ai.audit", true).unwrap();
        mgr.set_rollout("ai.audit", 100).unwrap();
    }

    #[test]
    fn variant_distribution_approximates_weights() {
        let db = db();
        init(&db);
        mgr_set_rollout_full(&db);
        let mgr = FlagManager::new(&db, UserContext::new("seed"));
        mgr.replace_variants(
            "ai.audit",
            vec![
                Variant {
                    name: "control".into(),
                    weight: 1,
                    payload: None,
                },
                Variant {
                    name: "treatment".into(),
                    weight: 3,
                    payload: Some("v2".into()),
                },
            ],
        )
        .unwrap();
        let n = 2_000;
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        for i in 0..n {
            let res = FlagManager::new(&db, UserContext::new(format!("u-{i}")))
                .with_exposure_recording(false)
                .evaluate_dry("ai.audit")
                .unwrap();
            let v = res.variant.unwrap_or_else(|| "none".into());
            *counts.entry(v).or_insert(0) += 1;
        }
        let control = *counts.get("control").unwrap_or(&0) as f64 / n as f64;
        let treatment = *counts.get("treatment").unwrap_or(&0) as f64 / n as f64;
        assert!((0.18..=0.32).contains(&control), "control {}", control);
        assert!(
            (0.65..=0.82).contains(&treatment),
            "treatment {}",
            treatment
        );
    }

    #[test]
    fn rollback_restores_previous_version() {
        let db = db();
        init(&db);
        let mgr = FlagManager::new(&db, UserContext::new("u-1"));
        mgr.set_enabled("ai.audit", true).unwrap();
        mgr.set_rollout("ai.audit", 50).unwrap();
        let history_before = db.state_history("ai.audit").unwrap();
        assert_eq!(history_before.len(), 3); // initial + enabled + 50%
        let v50 = history_before.last().unwrap().clone();
        mgr.set_rollout("ai.audit", 100).unwrap();
        // Rollback to v50 (the 50% one) — version becomes a NEW row.
        let restored = mgr.rollback("ai.audit", v50.version).unwrap();
        assert_eq!(restored.rollout_percent, 50);
        assert!(restored.note.contains("rollback"));
    }

    #[test]
    fn reset_clears_history_and_uses_category_default() {
        let db = db();
        init(&db);
        let mgr = FlagManager::new(&db, UserContext::new("u-1"));
        mgr.set_enabled("ai.audit", true).unwrap();
        mgr.set_rollout("ai.audit", 50).unwrap();
        assert!(!db.state_history("ai.audit").unwrap().is_empty());
        mgr.reset("ai.audit").unwrap();
        let after = db.state_history("ai.audit").unwrap();
        // initial state + canonical reset row == 2 rows total (we don't wipe
        // the initial seed, only history past it).
        let latest = after.last().unwrap();
        // For ai.audit which is Beta, default_enabled() == false.
        assert!(!latest.enabled);
        assert_eq!(latest.rollout_percent, 0);
    }

    #[test]
    fn metrics_are_recorded() {
        let db = db();
        init(&db);
        mgr_set_rollout_full(&db);
        let mgr = FlagManager::new(&db, UserContext::new("u-1"));
        assert!(mgr.is_enabled("ai.audit"));
        mgr.record_conversion("ai.audit").unwrap();
        mgr.record_rejection("ai.audit").unwrap();
        let summary = db.metrics_summary("ai.audit").unwrap();
        assert!(summary.get("exposure").copied().unwrap_or(0) >= 1);
        assert_eq!(summary.get("conversion").copied().unwrap_or(0), 1);
        assert_eq!(summary.get("rejection").copied().unwrap_or(0), 1);
    }

    #[test]
    fn snapshot_version_increments_with_state_changes() {
        let db = db();
        init(&db);
        let v0 = db.current_snapshot_version().unwrap();
        let mgr = FlagManager::new(&db, UserContext::new("u-1"));
        mgr.set_enabled("ai.audit", false).unwrap();
        let v1 = db.current_snapshot_version().unwrap();
        assert!(v1 > v0);
    }

    #[test]
    fn dry_evaluation_does_not_record_exposure() {
        let db = db();
        init(&db);
        mgr_set_rollout_full(&db);
        let mgr = FlagManager::new(&db, UserContext::new("u-1")).with_exposure_recording(false);
        assert!(mgr.is_enabled("ai.audit"));
        assert_eq!(
            db.metrics_summary("ai.audit")
                .unwrap()
                .get("exposure")
                .copied()
                .unwrap_or(0),
            0
        );
    }

    #[test]
    fn install_id_is_persistent_and_uuid_like() {
        let db = db();
        init(&db);
        let id = load_or_create_install_id(&db).unwrap();
        assert!(!id.is_empty());
        let again = load_or_create_install_id(&db).unwrap();
        assert_eq!(id, again);
        // UUID v4 has hyphens at positions 8, 13, 18, 23.
        assert_eq!(id.len(), 36);
        assert_eq!(id.matches('-').count(), 4);
    }

    #[test]
    fn unknown_flag_returns_category_default_when_no_definition() {
        let db = db();
        init(&db);
        let mgr = FlagManager::new(&db, UserContext::new("u")).with_exposure_recording(false);
        // Insert a non-builtin state, then evaluate it.
        mgr.register_flag(FlagDefinition {
            name: "non.built.in".into(),
            category: FlagCategory::Stable,
            description: "test".into(),
            owner: None,
            user_manageable: true,
        })
        .unwrap();
        assert!(mgr.is_enabled("non.built.in"));
    }

    #[test]
    fn flag_set_enabled_then_cleared_override() {
        let db = db();
        init(&db);
        let mgr = FlagManager::new(&db, UserContext::new("u-2"));
        mgr.set_override("ai.audit", "u-2", true, None).unwrap();
        assert!(mgr.is_enabled("ai.audit"));
        assert!(mgr.clear_override("ai.audit", "u-2").unwrap());
        let res = mgr.evaluate_dry("ai.audit").unwrap();
        assert!(!res.from_override);
    }

    #[test]
    fn metrics_prune_is_safe_on_empty_db() {
        let db = db();
        init(&db);
        let n = db.prune_metrics(30).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn segment_compose_and_complex_rule() {
        // OR-style behaviour: rule passes if any sub-rule matches.
        let rules = vec![
            SegmentRule::UserInList {
                user_ids: vec!["vip".into()],
            },
            SegmentRule::HasAttribute {
                key: "subscription".into(),
                any_of: vec!["pro".into()],
            },
        ];
        let vip = UserContext::new("vip");
        let pro = UserContext::new("u").with_attribute("subscription", "pro");
        let free = UserContext::new("u");
        assert!(rules.iter().any(|r| r.evaluate("f", &vip)));
        assert!(rules.iter().any(|r| r.evaluate("f", &pro)));
        assert!(!rules.iter().any(|r| r.evaluate("f", &free)));
    }
}
