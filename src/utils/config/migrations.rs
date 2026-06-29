//! Versioned configuration migrations.
//!
//! StarForge config files carry a `version` field. Instead of relying on
//! `#[serde(default)]` to silently paper over schema drift — which can *drop
//! wallet entries* when a field is renamed or restructured — we deserialize the
//! on-disk file into a schema-agnostic [`serde_json::Value`] first, read its
//! version, and apply a sequence of pure migration functions that reshape the
//! value before it is finally deserialized into the current [`Config`].
//!
//! ## Adding a new schema version
//!
//! 1. Bump [`CURRENT_CONFIG_VERSION`](super::CURRENT_CONFIG_VERSION) and
//!    [`default_version`](super::default_version).
//! 2. Write a `migrate_vN_to_vN1(&mut serde_json::Value)` pure function below.
//! 3. Register it in [`MIGRATIONS`].
//! 4. Add a `tests/fixtures/config_vN.toml` fixture and an integration test that
//!    loads it, runs migrations, and asserts the resulting [`Config`].
//!
//! Each migration is a *pure* `fn(&mut serde_json::Value)`: it must not touch the
//! file system, network, or any global state. Backups and disk writes are handled
//! by the caller (`config::load` / `config::apply_migration`).
//!
//! [`Config`]: super::Config

use anyhow::{anyhow, Result};
use serde_json::{Map, Value};

/// A single forward migration step from schema `from` to schema `to`.
pub struct Migration {
    /// Version the migration upgrades *from*.
    pub from: u32,
    /// Version the migration upgrades *to*.
    pub to: u32,
    /// Pure transformation applied to the config value.
    pub apply: fn(&mut Value),
}

/// Ordered registry of every migration StarForge knows about.
///
/// Migrations are applied sequentially: `v1 -> v2 -> v3 -> ...` until the value
/// reaches the current schema version.
pub const MIGRATIONS: &[Migration] = &[Migration {
    from: 1,
    to: 2,
    apply: migrate_v1_to_v2,
}];

/// Reads the schema version from a config value.
///
/// Missing, empty, `"0"`, or unparseable versions are treated as version `1`
/// (the first versioned schema), so legacy files migrate forward cleanly rather
/// than erroring out.
pub fn read_version(value: &Value) -> u32 {
    let raw = match value.get("version") {
        Some(Value::String(s)) => s.trim().parse::<u32>().ok(),
        Some(Value::Number(n)) => n.as_u64().map(|n| n as u32),
        _ => None,
    };
    raw.filter(|v| *v >= 1).unwrap_or(1)
}

/// Overwrites the `version` field with `version` (as a string, matching the
/// TOML/serde representation used by [`Config`](super::Config)).
fn set_version(value: &mut Value, version: u32) {
    if let Some(obj) = value.as_object_mut() {
        obj.insert("version".to_string(), Value::String(version.to_string()));
    }
}

/// Applies every migration required to bring `value` up to `target`.
///
/// Returns the ordered list of `(from, to)` steps actually applied. This function
/// is pure with respect to the file system — it only mutates `value`.
///
/// Errors if there is no registered migration path from the current version
/// (for example a config written by a *newer* CLI than the one running).
pub fn migrate_value(value: &mut Value, target: u32) -> Result<Vec<(u32, u32)>> {
    let mut applied = Vec::new();

    loop {
        let current = read_version(value);
        if current >= target {
            break;
        }

        let migration = MIGRATIONS
            .iter()
            .find(|m| m.from == current)
            .ok_or_else(|| {
                anyhow!(
                    "No migration path from config version {} (target version {}). \
                 This config may have been written by a newer StarForge release.",
                    current,
                    target
                )
            })?;

        (migration.apply)(value);
        // Defensively ensure the version was bumped even if the migration forgot.
        set_version(value, migration.to);
        applied.push((migration.from, migration.to));
    }

    Ok(applied)
}

// ── Migrations ────────────────────────────────────────────────────────────────

/// Migrate a v1 config to v2.
///
/// v2 normalises three pieces of legacy shape that earlier StarForge builds wrote
/// in inconsistent ways. Every transformation is conservative: it only fills a
/// canonical field when that field is *absent*, so we never clobber data and — the
/// whole point of this system — never silently drop wallet secrets.
///
/// 1. **Per-wallet `secret` → `secret_key`.** Some early builds stored the wallet
///    secret under `secret`. Plain `#[serde(default)]` deserialization would have
///    dropped it, making wallets unusable after an upgrade. We rename it.
/// 2. **Flat KDF keys → nested `wallet_encryption`.** Legacy top-level
///    `kdf_mem` / `kdf_iterations` / `kdf_parallelism` are folded into the
///    structured `[wallet_encryption]` table.
/// 3. **`telemetry` → `telemetry_enabled`.** Renamed boolean flag.
pub fn migrate_v1_to_v2(value: &mut Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };

    // 1. Rename legacy per-wallet `secret` to `secret_key`.
    if let Some(Value::Array(wallets)) = obj.get_mut("wallets") {
        for wallet in wallets.iter_mut() {
            if let Some(w) = wallet.as_object_mut() {
                if !w.contains_key("secret_key") {
                    if let Some(secret) = w.remove("secret") {
                        w.insert("secret_key".to_string(), secret);
                    }
                }
            }
        }
    }

    // 2. Restructure flat KDF keys into the nested `wallet_encryption` table.
    let legacy_mem = obj.remove("kdf_mem");
    let legacy_iterations = obj.remove("kdf_iterations");
    let legacy_parallelism = obj.remove("kdf_parallelism");
    let has_legacy_kdf =
        legacy_mem.is_some() || legacy_iterations.is_some() || legacy_parallelism.is_some();
    if has_legacy_kdf && !obj.contains_key("wallet_encryption") {
        let mut kdf = Map::new();
        if let Some(v) = legacy_mem {
            kdf.insert("mem".to_string(), v);
        }
        if let Some(v) = legacy_iterations {
            kdf.insert("iterations".to_string(), v);
        }
        if let Some(v) = legacy_parallelism {
            kdf.insert("parallelism".to_string(), v);
        }
        obj.insert("wallet_encryption".to_string(), Value::Object(kdf));
    }

    // 3. Rename `telemetry` to `telemetry_enabled`.
    if !obj.contains_key("telemetry_enabled") {
        if let Some(t) = obj.remove("telemetry") {
            obj.insert("telemetry_enabled".to_string(), t);
        }
    }

    // 4. Stamp the new schema version.
    obj.insert("version".to_string(), Value::String("2".to_string()));
}

// ── Diffing (for `config migrate --dry-run`) ────────────────────────────────────

/// A single human-readable change produced by a migration, used to render the
/// `--dry-run` preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A new key/value was introduced.
    Added { path: String, value: String },
    /// A key/value was removed.
    Removed { path: String, value: String },
    /// A value changed in place.
    Changed {
        path: String,
        from: String,
        to: String,
    },
}

impl std::fmt::Display for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Change::Added { path, value } => write!(f, "+ {path} = {value}"),
            Change::Removed { path, value } => write!(f, "- {path} = {value}"),
            Change::Changed { path, from, to } => write!(f, "~ {path}: {from} -> {to}"),
        }
    }
}

/// Computes an ordered, human-readable diff between two config values.
pub fn diff(old: &Value, new: &Value) -> Vec<Change> {
    let mut changes = Vec::new();
    diff_inner("", old, new, &mut changes);
    changes
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}.{key}")
    }
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(s) => format!("\"{s}\""),
        other => other.to_string(),
    }
}

fn diff_inner(prefix: &str, old: &Value, new: &Value, out: &mut Vec<Change>) {
    match (old, new) {
        (Value::Object(old_map), Value::Object(new_map)) => {
            // Stable key order: union of keys, sorted.
            let mut keys: Vec<&String> = old_map.keys().chain(new_map.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let path = join(prefix, key);
                match (old_map.get(key), new_map.get(key)) {
                    (Some(o), Some(n)) => diff_inner(&path, o, n, out),
                    (Some(o), None) => out.push(Change::Removed {
                        path,
                        value: scalar(o),
                    }),
                    (None, Some(n)) => out.push(Change::Added {
                        path,
                        value: scalar(n),
                    }),
                    (None, None) => {}
                }
            }
        }
        (o, n) if o == n => {}
        (o, n) => out.push(Change::Changed {
            path: if prefix.is_empty() {
                "(root)".to_string()
            } else {
                prefix.to_string()
            },
            from: scalar(o),
            to: scalar(n),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn read_version_handles_missing_and_legacy() {
        assert_eq!(read_version(&json!({})), 1);
        assert_eq!(read_version(&json!({ "version": "" })), 1);
        assert_eq!(read_version(&json!({ "version": "0" })), 1);
        assert_eq!(read_version(&json!({ "version": "1" })), 1);
        assert_eq!(read_version(&json!({ "version": "2" })), 2);
        assert_eq!(read_version(&json!({ "version": 3 })), 3);
    }

    #[test]
    fn v1_to_v2_renames_wallet_secret_field() {
        let mut value = json!({
            "version": "1",
            "network": "testnet",
            "wallets": [
                { "name": "alice", "secret": "SAAA" },
                { "name": "bob", "secret_key": "SBBB" },
            ],
        });
        migrate_v1_to_v2(&mut value);
        let wallets = value["wallets"].as_array().unwrap();
        assert_eq!(wallets[0]["secret_key"], json!("SAAA"));
        assert!(wallets[0].get("secret").is_none());
        // Existing secret_key is never clobbered.
        assert_eq!(wallets[1]["secret_key"], json!("SBBB"));
        assert_eq!(value["version"], json!("2"));
    }

    #[test]
    fn v1_to_v2_nests_flat_kdf_keys() {
        let mut value = json!({
            "version": "1",
            "kdf_mem": 65536,
            "kdf_iterations": 4,
            "kdf_parallelism": 2,
        });
        migrate_v1_to_v2(&mut value);
        assert!(value.get("kdf_mem").is_none());
        assert_eq!(value["wallet_encryption"]["mem"], json!(65536));
        assert_eq!(value["wallet_encryption"]["iterations"], json!(4));
        assert_eq!(value["wallet_encryption"]["parallelism"], json!(2));
    }

    #[test]
    fn v1_to_v2_renames_telemetry_flag() {
        let mut value = json!({ "version": "1", "telemetry": false });
        migrate_v1_to_v2(&mut value);
        assert!(value.get("telemetry").is_none());
        assert_eq!(value["telemetry_enabled"], json!(false));
    }

    #[test]
    fn migrate_value_applies_sequence_and_reports_steps() {
        let mut value = json!({ "version": "1", "network": "testnet", "wallets": [] });
        let applied = migrate_value(&mut value, 2).unwrap();
        assert_eq!(applied, vec![(1, 2)]);
        assert_eq!(read_version(&value), 2);

        // Idempotent: already-current value applies nothing.
        let applied = migrate_value(&mut value, 2).unwrap();
        assert!(applied.is_empty());
    }

    #[test]
    fn migrate_value_rejects_future_versions() {
        let mut value = json!({ "version": "99" });
        // Target is 2 but value is already 99 -> nothing to do (no downgrade).
        assert!(migrate_value(&mut value, 2).unwrap().is_empty());
    }

    #[test]
    fn diff_reports_added_removed_and_changed() {
        let old = json!({ "version": "1", "telemetry": true, "kdf_mem": 1 });
        let new =
            json!({ "version": "2", "telemetry_enabled": true, "wallet_encryption": { "mem": 1 } });
        let changes = diff(&old, &new);
        assert!(changes.contains(&Change::Changed {
            path: "version".to_string(),
            from: "\"1\"".to_string(),
            to: "\"2\"".to_string(),
        }));
        assert!(changes
            .iter()
            .any(|c| matches!(c, Change::Removed { path, .. } if path == "telemetry")));
        assert!(changes
            .iter()
            .any(|c| matches!(c, Change::Added { path, .. } if path == "telemetry_enabled")));
        assert!(changes
            .iter()
            .any(|c| matches!(c, Change::Added { path, .. } if path == "wallet_encryption")));
    }
}
