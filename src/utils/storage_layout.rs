//! Storage Layout Introspection and Migration Hazard Analysis (#803)
//!
//! Provides automated introspection of Soroban contract storage layouts from
//! Rust source code and spec definitions, and analyzes storage diffs between
//! contract versions to detect breaking hazards (removed keys, type mutations,
//! storage tier mismatches, enum discriminant changes, uninitialized keys).

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Storage tier in Soroban (Instance, Persistent, Temporary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageTier {
    Instance,
    Persistent,
    Temporary,
}

impl std::fmt::Display for StorageTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageTier::Instance => write!(f, "instance"),
            StorageTier::Persistent => write!(f, "persistent"),
            StorageTier::Temporary => write!(f, "temporary"),
        }
    }
}

/// A single storage key definition extracted from a contract layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageKeyDefinition {
    pub name: String,
    pub storage_tier: StorageTier,
    pub key_type: String,
    pub value_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discriminant: Option<u32>,
    pub is_optional: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
}

/// A DataKey enum representation extracted from source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataKeyEnumVariant {
    pub name: String,
    pub fields: Vec<String>,
    pub discriminant: Option<u32>,
}

/// A complete introspected storage layout of a contract version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageLayout {
    pub contract_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub keys: Vec<StorageKeyDefinition>,
    pub variants: Vec<DataKeyEnumVariant>,
}

/// Severity classification of a detected migration hazard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HazardSeverity {
    Info,
    Warning,
    HighRisk,
    Breaking,
}

impl std::fmt::Display for HazardSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HazardSeverity::Info => write!(f, "INFO"),
            HazardSeverity::Warning => write!(f, "WARNING"),
            HazardSeverity::HighRisk => write!(f, "HIGH_RISK"),
            HazardSeverity::Breaking => write!(f, "BREAKING"),
        }
    }
}

/// Specific type of storage hazard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HazardKind {
    RemovedKey,
    TypeMutation,
    StorageTierMismatch,
    DiscriminantCollision,
    UninitializedNewKey,
    TTLExpirationRisk,
}

/// A single hazard detected during layout comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigrationHazard {
    pub severity: HazardSeverity,
    pub kind: HazardKind,
    pub key: String,
    pub message: String,
    pub suggested_mitigation: String,
}

/// Comprehensive migration hazard report between two contract layouts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigrationHazardReport {
    pub from_contract: String,
    pub to_contract: String,
    pub from_version: Option<String>,
    pub to_version: Option<String>,
    pub is_safe: bool,
    pub breaking_count: usize,
    pub warning_count: usize,
    pub hazards: Vec<MigrationHazard>,
    pub starter_migration_rules: Value,
}

/// Core introspection and hazard analysis engine.
pub struct StorageLayoutIntrospector;

impl StorageLayoutIntrospector {
    /// Introspect contract Rust source code to extract storage keys and layout.
    pub fn introspect_source_code(source: &str, default_name: Option<&str>) -> StorageLayout {
        let contract_name = Self::extract_contract_name(source)
            .unwrap_or_else(|| default_name.unwrap_or("Contract").to_string());

        let variants = Self::extract_datakey_variants(source);
        let mut keys = Self::extract_storage_calls(source, &variants);

        // If no explicit storage() calls found, synthesize keys from DataKey variants
        if keys.is_empty() && !variants.is_empty() {
            for (idx, var) in variants.iter().enumerate() {
                let key_type = if var.fields.is_empty() {
                    format!("DataKey::{}", var.name)
                } else {
                    format!("DataKey::{}({})", var.name, var.fields.join(", "))
                };
                let val_type = Self::guess_value_type_for_key(&var.name);
                keys.push(StorageKeyDefinition {
                    name: var.name.clone(),
                    storage_tier: StorageTier::Instance,
                    key_type,
                    value_type: val_type,
                    discriminant: var.discriminant.or(Some(idx as u32)),
                    is_optional: false,
                    doc: None,
                });
            }
        }

        StorageLayout {
            contract_name,
            version: Self::extract_version(source),
            keys,
            variants,
        }
    }

    /// Read and introspect a file (Rust source or JSON layout).
    pub fn introspect_file(path: &Path) -> Result<StorageLayout> {
        let content = fs::read_to_string(path).with_context(|| {
            format!("Failed to read contract layout file at {}", path.display())
        })?;

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(layout) = serde_json::from_str::<StorageLayout>(&content) {
                return Ok(layout);
            }
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Contract");
        Ok(Self::introspect_source_code(&content, Some(stem)))
    }

    /// Compare two layouts and produce a hazard report.
    pub fn compare_layouts(
        old_layout: &StorageLayout,
        new_layout: &StorageLayout,
    ) -> MigrationHazardReport {
        let mut hazards = Vec::new();
        let old_keys: HashMap<String, &StorageKeyDefinition> = old_layout
            .keys
            .iter()
            .map(|k| (k.name.clone(), k))
            .collect();
        let new_keys: HashMap<String, &StorageKeyDefinition> = new_layout
            .keys
            .iter()
            .map(|k| (k.name.clone(), k))
            .collect();

        // 1. Check for removed keys
        for (name, old_key) in &old_keys {
            if !new_keys.contains_key(name) {
                hazards.push(MigrationHazard {
                    severity: HazardSeverity::Breaking,
                    kind: HazardKind::RemovedKey,
                    key: name.clone(),
                    message: format!(
                        "Storage key '{}' ({}) was removed in target version.",
                        name, old_key.key_type
                    ),
                    suggested_mitigation: format!(
                        "Add an explicit migration rule in migration.json to delete or archive '{}' to prevent orphan storage entries.",
                        name
                    ),
                });
            }
        }

        // 2. Check for modified keys (type mutations or tier shifts)
        for (name, new_key) in &new_keys {
            if let Some(old_key) = old_keys.get(name) {
                // Check value type mutation
                if old_key.value_type != new_key.value_type {
                    hazards.push(MigrationHazard {
                        severity: HazardSeverity::Breaking,
                        kind: HazardKind::TypeMutation,
                        key: name.clone(),
                        message: format!(
                            "Storage key '{}' value type changed from '{}' to '{}'.",
                            name, old_key.value_type, new_key.value_type
                        ),
                        suggested_mitigation: format!(
                            "Add a transformation rule in migration.json converting existing '{}' values from '{}' to '{}'.",
                            name, old_key.value_type, new_key.value_type
                        ),
                    });
                }

                // Check storage tier mutation
                if old_key.storage_tier != new_key.storage_tier {
                    hazards.push(MigrationHazard {
                        severity: HazardSeverity::Breaking,
                        kind: HazardKind::StorageTierMismatch,
                        key: name.clone(),
                        message: format!(
                            "Storage key '{}' moved from '{}' to '{}' storage tier.",
                            name, old_key.storage_tier, new_key.storage_tier
                        ),
                        suggested_mitigation: format!(
                            "Migrate values across tiers by reading from env.storage().{}() and writing to env.storage().{}() during upgrade.",
                            old_key.storage_tier, new_key.storage_tier
                        ),
                    });
                }
            } else {
                // 3. Newly added keys that might need initialization
                hazards.push(MigrationHazard {
                    severity: HazardSeverity::Warning,
                    kind: HazardKind::UninitializedNewKey,
                    key: name.clone(),
                    message: format!(
                        "New storage key '{}' ({}) added in target version.",
                        name, new_key.key_type
                    ),
                    suggested_mitigation: format!(
                        "Ensure the contract upgrade function or migration script initializes '{}' with a valid default value.",
                        name
                    ),
                });
            }
        }

        // 4. Check DataKey discriminant renumbering / collisions
        let old_variants: HashMap<&str, Option<u32>> = old_layout
            .variants
            .iter()
            .map(|v| (v.name.as_str(), v.discriminant))
            .collect();

        for new_var in &new_layout.variants {
            if let Some(&Some(old_disc)) = old_variants.get(new_var.name.as_str()) {
                if let Some(new_disc) = new_var.discriminant {
                    if old_disc != new_disc {
                        hazards.push(MigrationHazard {
                            severity: HazardSeverity::Breaking,
                            kind: HazardKind::DiscriminantCollision,
                            key: new_var.name.clone(),
                            message: format!(
                                "DataKey variant '{}' discriminant changed from {} to {}.",
                                new_var.name, old_disc, new_disc
                            ),
                            suggested_mitigation: format!(
                                "Preserve original discriminant #[repr(u32)] values: {} = {}.",
                                new_var.name, old_disc
                            ),
                        });
                    }
                }
            }
        }

        let breaking_count = hazards
            .iter()
            .filter(|h| {
                matches!(
                    h.severity,
                    HazardSeverity::Breaking | HazardSeverity::HighRisk
                )
            })
            .count();
        let warning_count = hazards
            .iter()
            .filter(|h| h.severity == HazardSeverity::Warning)
            .count();
        let is_safe = breaking_count == 0;

        let starter_migration_rules = Self::build_starter_rules(&hazards, old_layout, new_layout);

        MigrationHazardReport {
            from_contract: old_layout.contract_name.clone(),
            to_contract: new_layout.contract_name.clone(),
            from_version: old_layout.version.clone(),
            to_version: new_layout.version.clone(),
            is_safe,
            breaking_count,
            warning_count,
            hazards,
            starter_migration_rules,
        }
    }

    fn build_starter_rules(
        hazards: &[MigrationHazard],
        old_layout: &StorageLayout,
        new_layout: &StorageLayout,
    ) -> Value {
        let mut transforms = json!({});
        let mut deletions = Vec::new();
        let mut defaults = json!({});

        for hazard in hazards {
            match hazard.kind {
                HazardKind::RemovedKey => {
                    deletions.push(hazard.key.clone());
                }
                HazardKind::TypeMutation => {
                    transforms[&hazard.key] = json!({
                        "action": "cast_or_transform",
                        "target_type": new_layout.keys.iter().find(|k| k.name == hazard.key).map(|k| k.value_type.as_str()).unwrap_or("unknown")
                    });
                }
                HazardKind::UninitializedNewKey => {
                    defaults[&hazard.key] = json!("DEFAULT_VALUE");
                }
                _ => {}
            }
        }

        json!({
            "from_version": old_layout.version.as_deref().unwrap_or("1.0.0"),
            "to_version": new_layout.version.as_deref().unwrap_or("2.0.0"),
            "contract": new_layout.contract_name,
            "rules": {
                "transform_fields": transforms,
                "delete_fields": deletions,
                "initialize_fields": defaults,
            }
        })
    }

    fn extract_contract_name(source: &str) -> Option<String> {
        let re = Regex::new(r"(?m)#\[contract\]\s*(?:pub\s+)?struct\s+([A-Za-z0-9_]+)").ok()?;
        re.captures(source).map(|c| c[1].to_string())
    }

    fn extract_version(source: &str) -> Option<String> {
        let re = Regex::new(r#"(?i)version\s*[:=]\s*["']([^"']+)["']"#).ok()?;
        re.captures(source).map(|c| c[1].to_string())
    }

    fn extract_datakey_variants(source: &str) -> Vec<DataKeyEnumVariant> {
        let mut variants = Vec::new();
        let enum_re =
            Regex::new(r"(?s)(?:pub\s+)?enum\s+(?:DataKey|StorageKey)\s*\{([^}]+)\}").ok();
        let Some(enum_re) = enum_re else {
            return variants;
        };

        if let Some(caps) = enum_re.captures(source) {
            let body = &caps[1];
            let variant_re = Regex::new(r"([A-Za-z0-9_]+)(?:\(([^)]+)\))?(?:\s*=\s*(\d+))?").ok();
            if let Some(var_re) = variant_re {
                for line in body.lines() {
                    let trimmed = line.trim().trim_end_matches(',');
                    if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                        continue;
                    }
                    if let Some(vcaps) = var_re.captures(trimmed) {
                        let name = vcaps[1].to_string();
                        let fields = vcaps
                            .get(2)
                            .map(|m| {
                                m.as_str()
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let discriminant =
                            vcaps.get(3).and_then(|m| m.as_str().parse::<u32>().ok());
                        variants.push(DataKeyEnumVariant {
                            name,
                            fields,
                            discriminant,
                        });
                    }
                }
            }
        }
        variants
    }

    fn extract_storage_calls(
        source: &str,
        variants: &[DataKeyEnumVariant],
    ) -> Vec<StorageKeyDefinition> {
        let mut keys_map: BTreeMap<String, StorageKeyDefinition> = BTreeMap::new();

        let storage_re = Regex::new(
            r"env\.storage\(\)\.(instance|persistent|temporary)\(\)\.(?:get|set|has|set_ttl)\s*::<[^>]+>?\s*\(\s*&(?:DataKey::)?([A-Za-z0-9_]+)",
        ).ok();

        if let Some(re) = storage_re {
            for cap in re.captures_iter(source) {
                let tier_str = &cap[1];
                let key_name = &cap[2];
                let tier = match tier_str {
                    "persistent" => StorageTier::Persistent,
                    "temporary" => StorageTier::Temporary,
                    _ => StorageTier::Instance,
                };
                let val_type = Self::guess_value_type_for_key(key_name);
                let disc = variants
                    .iter()
                    .position(|v| v.name == key_name)
                    .map(|idx| idx as u32);

                keys_map.insert(
                    key_name.to_string(),
                    StorageKeyDefinition {
                        name: key_name.to_string(),
                        storage_tier: tier,
                        key_type: format!("DataKey::{}", key_name),
                        value_type: val_type,
                        discriminant: disc,
                        is_optional: false,
                        doc: None,
                    },
                );
            }
        }

        keys_map.into_values().collect()
    }

    fn guess_value_type_for_key(name: &str) -> String {
        let lower = name.to_lowercase();
        if lower.contains("admin") || lower.contains("owner") || lower.contains("holder") {
            "Address".to_string()
        } else if lower.contains("balance") || lower.contains("amount") || lower.contains("supply")
        {
            "i128".to_string()
        } else if lower.contains("count") || lower.contains("nonce") || lower.contains("seq") {
            "u32".to_string()
        } else if lower.contains("config") || lower.contains("state") {
            "Config".to_string()
        } else if lower.contains("flag") || lower.contains("paused") || lower.contains("active") {
            "bool".to_string()
        } else {
            "Val".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_V1_CONTRACT: &str = r#"
#[contract]
pub struct TokenContract;

#[derive(Clone)]
pub enum DataKey {
    Admin = 0,
    Balance(Address) = 1,
    TotalSupply = 2,
    Nonce = 3,
}

#[contractimpl]
impl TokenContract {
    pub fn init(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TotalSupply, &0i128);
    }
}
"#;

    const SAMPLE_V2_CONTRACT: &str = r#"
#[contract]
pub struct TokenContract;

#[derive(Clone)]
pub enum DataKey {
    Admin = 0,
    Balance(Address) = 1,
    TotalSupply = 2,
    Config = 3, // Renumbered and Nonce removed, Config added
}

#[contractimpl]
impl TokenContract {
    pub fn init(env: Env, admin: Address) {
        env.storage().persistent().set(&DataKey::Admin, &admin); // Tier shift: instance -> persistent
        env.storage().instance().set(&DataKey::TotalSupply, &0i128);
        env.storage().instance().set(&DataKey::Config, &Config::default());
    }
}
"#;

    #[test]
    fn test_introspect_v1_layout() {
        let layout = StorageLayoutIntrospector::introspect_source_code(SAMPLE_V1_CONTRACT, None);
        assert_eq!(layout.contract_name, "TokenContract");
        assert_eq!(layout.variants.len(), 4);
        assert!(layout.variants.iter().any(|v| v.name == "Admin"));
        assert!(layout.variants.iter().any(|v| v.name == "Balance"));
    }

    #[test]
    fn test_compare_layouts_detects_breaking_hazards() {
        let l1 = StorageLayoutIntrospector::introspect_source_code(SAMPLE_V1_CONTRACT, None);
        let l2 = StorageLayoutIntrospector::introspect_source_code(SAMPLE_V2_CONTRACT, None);

        let report = StorageLayoutIntrospector::compare_layouts(&l1, &l2);
        assert!(!report.is_safe);
        assert!(report.breaking_count >= 1);

        // Should detect removed key (Nonce)
        assert!(report
            .hazards
            .iter()
            .any(|h| h.kind == HazardKind::RemovedKey && h.key == "Nonce"));

        // Should detect tier shift (Admin from instance to persistent)
        assert!(report
            .hazards
            .iter()
            .any(|h| h.kind == HazardKind::StorageTierMismatch && h.key == "Admin"));

        // Should detect new key (Config)
        assert!(report
            .hazards
            .iter()
            .any(|h| h.kind == HazardKind::UninitializedNewKey && h.key == "Config"));

        // Generated starter migration rules should contain the changes
        let rules_str = serde_json::to_string(&report.starter_migration_rules).unwrap();
        assert!(rules_str.contains("Nonce"));
        assert!(rules_str.contains("Config"));
    }

    #[test]
    fn test_identical_layouts_are_safe() {
        let l1 = StorageLayoutIntrospector::introspect_source_code(SAMPLE_V1_CONTRACT, None);
        let report = StorageLayoutIntrospector::compare_layouts(&l1, &l1);
        assert!(report.is_safe);
        assert_eq!(report.breaking_count, 0);
        assert_eq!(report.hazards.len(), 0);
    }
}
