use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Critical,
    Major,
    Minor,
    Info,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Critical => write!(f, "critical"),
            Severity::Major => write!(f, "major"),
            Severity::Minor => write!(f, "minor"),
            Severity::Info => write!(f, "info"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Compatibility {
    FullyCompatible,
    CompatibleWithMigration,
    Incompatible,
}

impl std::fmt::Display for Compatibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Compatibility::FullyCompatible => write!(f, "fully-compatible"),
            Compatibility::CompatibleWithMigration => write!(f, "compatible-with-migration"),
            Compatibility::Incompatible => write!(f, "incompatible"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakingChange {
    pub category: String,
    pub severity: Severity,
    pub title: String,
    pub description: String,
    pub migration_guide: String,
    pub affected_items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageChange {
    pub key: String,
    pub change_type: String,
    pub scope: String,
    pub old_type: Option<String>,
    pub new_type: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkChange {
    pub old_version: Option<String>,
    pub new_version: Option<String>,
    pub deprecated_apis: Vec<String>,
    pub new_apis: Vec<String>,
    pub changed_apis: Vec<ChangedApi>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedApi {
    pub name: String,
    pub old_signature: Option<String>,
    pub new_signature: Option<String>,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationSuggestion {
    pub priority: String,
    pub title: String,
    pub description: String,
    pub effort: String,
    pub risk: String,
    pub code_snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub from_version: String,
    pub to_version: String,
    pub compatibility: Compatibility,
    pub breaking_changes: Vec<BreakingChange>,
    pub storage_changes: Vec<StorageChange>,
    pub sdk_changes: Option<SdkChange>,
    pub suggestions: Vec<MigrationSuggestion>,
    pub steps: Vec<MigrationStep>,
    pub rollback_strategy: Option<String>,
    pub estimated_effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationStep {
    pub order: usize,
    pub action: String,
    pub description: String,
    pub command: Option<String>,
    pub code_template: Option<String>,
    pub validation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisConfig {
    pub old_wasm: Option<String>,
    pub new_wasm: Option<String>,
    pub old_source: Option<String>,
    pub new_source: Option<String>,
    pub old_sdk_version: Option<String>,
    pub new_sdk_version: Option<String>,
    pub old_protocol_version: Option<u32>,
    pub new_protocol_version: Option<u32>,
    pub contract_name: Option<String>,
}

pub fn analyze_contract_compatibility(
    old_specs: &[String],
    new_specs: &[String],
    old_wasm_hash: &str,
    new_wasm_hash: &str,
    config: &AnalysisConfig,
) -> Result<MigrationPlan> {
    let mut breaking_changes = Vec::new();
    let mut storage_changes = Vec::new();
    let mut suggestions = Vec::new();

    let from_ver = config.old_sdk_version.as_deref().unwrap_or("unknown");
    let to_ver = config.new_sdk_version.as_deref().unwrap_or("unknown");

    analyze_function_changes(
        old_specs,
        new_specs,
        &mut breaking_changes,
        &mut suggestions,
    );
    analyze_type_changes(
        old_specs,
        new_specs,
        &mut breaking_changes,
        &mut suggestions,
    );
    analyze_storage_layout(
        old_specs,
        new_specs,
        &mut storage_changes,
        &mut breaking_changes,
        &mut suggestions,
    );
    analyze_sdk_upgrade(config, &mut breaking_changes, &mut suggestions);
    analyze_protocol_upgrade(config, &mut breaking_changes, &mut suggestions);

    let compatibility = determine_compatibility(&breaking_changes);
    let has_storage_migration = !storage_changes.is_empty();

    let steps = build_migration_steps(
        config,
        &breaking_changes,
        &storage_changes,
        old_wasm_hash,
        new_wasm_hash,
        has_storage_migration,
    );

    let rollback_strategy = generate_rollback_strategy(&storage_changes, config);
    let estimated_effort = estimate_effort(&breaking_changes, &storage_changes);

    Ok(MigrationPlan {
        from_version: from_ver.to_string(),
        to_version: to_ver.to_string(),
        compatibility,
        breaking_changes,
        storage_changes,
        sdk_changes: None,
        suggestions,
        steps,
        rollback_strategy,
        estimated_effort,
    })
}

fn analyze_function_changes(
    old_specs: &[String],
    new_specs: &[String],
    breaking_changes: &mut Vec<BreakingChange>,
    suggestions: &mut Vec<MigrationSuggestion>,
) {
    let old_fns = extract_functions(old_specs);
    let new_fns = extract_functions(new_specs);

    for (name, sig) in &old_fns {
        if !new_fns.contains_key(name) {
            breaking_changes.push(BreakingChange {
                category: "function_removed".into(),
                severity: Severity::Critical,
                title: format!("Function `{}` removed", name),
                description: format!("The function `{}` with signature `{}` exists in the old version but is absent in the new version.", name, sig),
                migration_guide: format!("Remove all call sites referencing `{}`. If the functionality is still needed, check release notes for its replacement.", name),
                affected_items: vec![name.clone()],
            });
        }
    }

    for (name, sig) in &new_fns {
        if !old_fns.contains_key(name) {
            suggestions.push(MigrationSuggestion {
                priority: "low".into(),
                title: format!("New function `{}` available", name),
                description: format!("The new version adds function `{}` with signature `{}`. No migration action required.", name, sig),
                effort: "none".into(),
                risk: "none".into(),
                code_snippet: None,
            });
        } else if old_fns.get(name) != Some(sig) {
            breaking_changes.push(BreakingChange {
                category: "function_signature_changed".into(),
                severity: Severity::Major,
                title: format!("Function `{}` signature changed", name),
                description: format!("Old: `{}`\nNew: `{}`", old_fns[name], sig),
                migration_guide: format!(
                    "Update all call sites for `{}` to match the new signature.",
                    name
                ),
                affected_items: vec![name.clone()],
            });
        }
    }
}

fn analyze_type_changes(
    old_specs: &[String],
    new_specs: &[String],
    breaking_changes: &mut Vec<BreakingChange>,
    _suggestions: &mut [MigrationSuggestion],
) {
    let old_types = extract_types(old_specs);
    let new_types = extract_types(new_specs);

    for (name, def) in &old_types {
        if !new_types.contains_key(name) {
            breaking_changes.push(BreakingChange {
                category: "type_removed".into(),
                severity: Severity::Critical,
                title: format!("Type `{}` removed", name),
                description: format!(
                    "The user-defined type `{}` ({}) has been removed in the new version.",
                    name, def
                ),
                migration_guide: format!(
                    "Replace all usages of `{}` with the new type or inline equivalent.",
                    name
                ),
                affected_items: vec![name.clone()],
            });
        } else if new_types.get(name) != Some(def) {
            breaking_changes.push(BreakingChange {
                category: "type_definition_changed".into(),
                severity: Severity::Major,
                title: format!("Type `{}` definition changed", name),
                description: format!("Old: `{}`\nNew: `{}`", def, new_types[name]),
                migration_guide: format!("Review all code that constructs or destructures `{}` and update to the new definition.", name),
                affected_items: vec![name.clone()],
            });
        }
    }
}

fn analyze_storage_layout(
    old_specs: &[String],
    new_specs: &[String],
    storage_changes: &mut Vec<StorageChange>,
    breaking_changes: &mut Vec<BreakingChange>,
    _suggestions: &mut [MigrationSuggestion],
) {
    let old_storage = extract_storage_keys(old_specs);
    let new_storage = extract_storage_keys(new_specs);

    for key in &old_storage {
        if !new_storage.contains(key) {
            let change = StorageChange {
                key: key.clone(),
                change_type: "removed".into(),
                scope: "instance".into(),
                old_type: None,
                new_type: None,
                description: format!(
                    "Storage key `{}` present in old version but missing in new version",
                    key
                ),
            };
            storage_changes.push(change);

            breaking_changes.push(BreakingChange {
                category: "storage_key_removed".into(),
                severity: Severity::Major,
                title: format!("Storage key `{}` removed", key),
                description: format!(
                    "The storage entry `{}` is no longer used in the new contract version.",
                    key
                ),
                migration_guide: format!(
                    "Add a migration step to remove stale key `{}` from storage.",
                    key
                ),
                affected_items: vec![key.clone()],
            });
        }
    }

    for key in &new_storage {
        if !old_storage.contains(key) {
            let change = StorageChange {
                key: key.clone(),
                change_type: "added".into(),
                scope: "instance".into(),
                old_type: None,
                new_type: None,
                description: format!("New storage key `{}` introduced in new version", key),
            };
            storage_changes.push(change);

            breaking_changes.push(BreakingChange {
                category: "storage_key_added".into(),
                severity: Severity::Minor,
                title: format!("New storage key `{}`", key),
                description: format!("The new version introduces storage key `{}` which must be initialized during migration.", key),
                migration_guide: format!("Add a migration step to populate key `{}` with appropriate default values.", key),
                affected_items: vec![key.clone()],
            });
        }
    }
}

fn analyze_sdk_upgrade(
    config: &AnalysisConfig,
    breaking_changes: &mut Vec<BreakingChange>,
    suggestions: &mut Vec<MigrationSuggestion>,
) {
    match (&config.old_sdk_version, &config.new_sdk_version) {
        (Some(old), Some(new)) if old != new => {
            let old_parts: Vec<&str> = old.split('.').collect();
            let new_parts: Vec<&str> = new.split('.').collect();

            if let (Some(old_major), Some(new_major)) = (old_parts.first(), new_parts.first()) {
                if old_major != new_major {
                    breaking_changes.push(BreakingChange {
                        category: "sdk_major_upgrade".into(),
                        severity: Severity::Critical,
                        title: format!("SDK major version upgrade: {} → {}", old, new),
                        description: format!(
                            "Major version upgrade from {} to {} may contain breaking API changes.",
                            old, new
                        ),
                        migration_guide: format!(
                            "Review the SDK changelog between {} and {}.\n\
                             Update Cargo.toml dependency: soroban-sdk = \"{}\"\n\
                             Run `cargo update` and fix any compilation errors.",
                            old, new, new
                        ),
                        affected_items: vec!["soroban-sdk".into(), "Cargo.toml".into()],
                    });
                }
            }

            suggestions.push(MigrationSuggestion {
                priority: "high".into(),
                title: format!("Update SDK dependency from {} to {}", old, new),
                description: format!("Update the soroban-sdk dependency version in Cargo.toml from {} to {} and adjust for any API changes.", old, new),
                effort: "medium".into(),
                risk: "medium".into(),
                code_snippet: Some(format!(
                    "[dependencies]\nsoroban-sdk = \"{}\"",
                    new
                )),
            });
        }
        _ => {}
    }

    if config.old_sdk_version.is_none() && config.new_sdk_version.is_some() {
        suggestions.push(MigrationSuggestion {
            priority: "info".into(),
            title: "SDK version not specified for old version".into(),
            description: "Provide the old SDK version with --old-sdk-version for more detailed SDK upgrade analysis.".into(),
            effort: "none".into(),
            risk: "none".into(),
            code_snippet: None,
        });
    }
}

fn analyze_protocol_upgrade(
    config: &AnalysisConfig,
    breaking_changes: &mut Vec<BreakingChange>,
    _suggestions: &mut [MigrationSuggestion],
) {
    match (config.old_protocol_version, config.new_protocol_version) {
        (Some(old), Some(new)) if old != new && new > old => {
            breaking_changes.push(BreakingChange {
                    category: "protocol_upgrade".into(),
                    severity: Severity::Major,
                    title: format!("Soroban protocol upgrade: v{} → v{}", old, new),
                    description: format!(
                        "Protocol version upgraded from {} to {}. This may change host function behavior, costs, and semantics.",
                        old, new
                    ),
                    migration_guide: format!(
                        "1. Review Soroban protocol v{} release notes\n\
                         2. Update any host function calls affected by the upgrade\n\
                         3. Re-run contract tests\n\
                         4. Verify gas costs are within expected ranges",
                        new
                    ),
                    affected_items: vec!["protocol".into(), "host_functions".into()],
                });
        }
        _ => {}
    }
}

fn determine_compatibility(changes: &[BreakingChange]) -> Compatibility {
    let has_critical = changes.iter().any(|c| c.severity == Severity::Critical);
    let has_major = changes.iter().any(|c| c.severity == Severity::Major);

    if has_critical {
        Compatibility::Incompatible
    } else if has_major {
        Compatibility::CompatibleWithMigration
    } else {
        Compatibility::FullyCompatible
    }
}

fn build_migration_steps(
    config: &AnalysisConfig,
    _breaking_changes: &[BreakingChange],
    storage_changes: &[StorageChange],
    old_wasm_hash: &str,
    new_wasm_hash: &str,
    has_storage_migration: bool,
) -> Vec<MigrationStep> {
    let mut steps = Vec::new();
    let mut order = 1usize;

    steps.push(MigrationStep {
        order,
        action: "backup".into(),
        description: "Create a backup of the current contract state and WASM".into(),
        command: Some("starforge backup create --contract <CONTRACT_ID>".into()),
        code_template: None,
        validation: Some("Verify backup exists: starforge backup list".into()),
    });
    order += 1;

    if has_storage_migration {
        steps.push(MigrationStep {
            order,
            action: "export_storage".into(),
            description: "Export current contract storage to a snapshot".into(),
            command: Some(
                "starforge inspect storage --contract <CONTRACT_ID> --json > snapshot.json".into(),
            ),
            code_template: None,
            validation: Some("Verify snapshot.json is valid JSON and contains entries".into()),
        });
        order += 1;

        if !storage_changes.is_empty() {
            let mut ops = Vec::new();
            for sc in storage_changes {
                match sc.change_type.as_str() {
                    "removed" => {
                        ops.push(format!(
                            "  - {{ \"op\": \"remove_field\", \"key\": \"{}\" }}",
                            sc.key
                        ));
                    }
                    "added" => {
                        ops.push(format!(
                            "  - {{ \"op\": \"add_field\", \"key\": \"{}\", \"default\": null }}",
                            sc.key
                        ));
                    }
                    _ => {}
                }
            }

            let rules_json = format!(
                r#"{{
  "from_version": "{}",
  "to_version": "{}",
  "ops": [
{}
  ],
  "required_keys": [],
  "forbidden_keys": []
}}"#,
                config.old_sdk_version.as_deref().unwrap_or("v1"),
                config.new_sdk_version.as_deref().unwrap_or("v2"),
                ops.join(",\n")
            );

            steps.push(MigrationStep {
                order,
                action: "create_rules".into(),
                description: "Create migration rules file for storage transformation".into(),
                command: Some("starforge migrate init --from-version <OLD> --to-version <NEW>".into()),
                code_template: Some(rules_json),
                validation: Some("Validate rules: starforge migrate test --sample snapshot.json --rules rules.json".into()),
            });
            order += 1;

            steps.push(MigrationStep {
                order,
                action: "test_migration".into(),
                description: "Dry-run migration to verify rules produce expected output".into(),
                command: Some(
                    "starforge migrate test --sample snapshot.json --rules rules.json".into(),
                ),
                code_template: None,
                validation: Some("Check that dry-run output matches expected schema".into()),
            });
            order += 1;

            steps.push(MigrationStep {
                order,
                action: "apply_migration".into(),
                description: "Apply storage migration to produce transformed snapshot".into(),
                command: Some("starforge migrate run --contract-id <CONTRACT_ID> --snapshot snapshot.json --rules rules.json --output migrated-snapshot.json".into()),
                code_template: None,
                validation: Some("Verify output: starforge migrate validate --snapshot migrated-snapshot.json --rules rules.json".into()),
            });
            order += 1;
        }

        steps.push(MigrationStep {
            order,
            action: "generate_migration_code".into(),
            description: "Generate on-chain migration function in the new contract".into(),
            command: Some("starforge migrate-ai generate --old-wasm <OLD> --new-wasm <NEW> --output migration.rs".into()),
            code_template: Some(generate_migration_code_stub(
                config.contract_name.as_deref().unwrap_or("Contract"),
                old_wasm_hash,
                new_wasm_hash,
                storage_changes,
            )),
            validation: Some("Verify migration code compiles with: stellar contract build".into()),
        });
        order += 1;
    }

    steps.push(MigrationStep {
        order,
        action: "compatibility_check".into(),
        description: "Run final compatibility check between old and new WASM".into(),
        command: Some("starforge upgrade-auto compat --old-wasm <OLD> --new-wasm <NEW>".into()),
        code_template: None,
        validation: Some("Ensure no incompatible issues remain".into()),
    });
    order += 1;

    steps.push(MigrationStep {
        order,
        action: "upgrade_contract".into(),
        description: "Upgrade the contract to the new WASM version".into(),
        command: Some("starforge upgrade execute --contract-id <CONTRACT_ID> --wasm <NEW_WASM> --wallet <WALLET>".into()),
        code_template: None,
        validation: Some("Verify upgrade: starforge contract info --id <CONTRACT_ID>".into()),
    });

    steps
}

fn generate_migration_code_stub(
    contract_name: &str,
    old_hash: &str,
    new_hash: &str,
    storage_changes: &[StorageChange],
) -> String {
    let mut snippet = String::new();

    snippet.push_str(&format!("// Migration function for {}\n", contract_name));
    snippet.push_str("// Generated by starforge migrate-ai\n\n");
    snippet.push_str("#[allow(unused)]\n");
    snippet.push_str("pub fn migrate(env: &soroban_sdk::Env, admin: soroban_sdk::Address) {\n");
    snippet.push_str("    admin.require_auth();\n\n");

    for sc in storage_changes {
        match sc.change_type.as_str() {
            "removed" => {
                snippet.push_str(&format!(
                    "    // Remove deprecated key\n    env.storage().instance().remove(&\"{}\");\n\n",
                    sc.key
                ));
            }
            "added" => {
                snippet.push_str(&format!(
                    "    // Initialize new storage key with default\n    if !env.storage().instance().has(&\"{}\") {{\n        env.storage().instance().set(&\"{}\", &soroban_sdk::Vec::new(env));\n    }}\n\n",
                    sc.key, sc.key
                ));
            }
            _ => {}
        }
    }

    snippet.push_str("    env.events().publish(\n");
    snippet.push_str("        (soroban_sdk::symbol_short!(\"migrated\"),),\n");
    snippet.push_str("        (\n");
    snippet.push_str(&format!(
        "            soroban_sdk::Bytes::from_slice(env, b\"{}\"),\n",
        &old_hash[..old_hash.len().min(12)]
    ));
    snippet.push_str(&format!(
        "            soroban_sdk::Bytes::from_slice(env, b\"{}\"),\n",
        &new_hash[..new_hash.len().min(12)]
    ));
    snippet.push_str("        ),\n");
    snippet.push_str("    );\n");
    snippet.push_str("}\n");

    snippet
}

fn generate_rollback_strategy(
    storage_changes: &[StorageChange],
    config: &AnalysisConfig,
) -> Option<String> {
    if storage_changes.is_empty() && config.old_sdk_version == config.new_sdk_version {
        return None;
    }

    let strategy = String::from(
        "## Rollback Strategy\n\n\
         1. **Pre-upgrade snapshot**: Before upgrading, take a full storage snapshot:\n\
         ```\n\
         starforge backup create --contract <CONTRACT_ID>\n\
         ```\n\n\
         2. **WASM rollback**: Re-deploy the previous contract WASM:\n\
         ```\n\
         starforge upgrade execute --contract-id <CONTRACT_ID> --wasm <OLD_WASM> --wallet <WALLET>\n\
         ```\n\n\
         3. **Storage rollback**: If storage was migrated, restore from backup:\n\
         ```\n\
         starforge backup restore --backup-id <BACKUP_ID>\n\
         ```\n\n\
         4. **Verify rollback**: Confirm the contract is in its previous state:\n\
         ```\n\
         starforge contract info --id <CONTRACT_ID>\n\
         ```\n\n\
         ## Data Safety\n\
         - The `starforge migrate run` command automatically creates a backup before applying.\n\
         - Use `starforge migrate rollback --migration-id <ID>` to revert storage transformations.\n\
         - Store pre-upgrade WASM files in version control for quick redeployment.\
    ");

    Some(strategy)
}

fn estimate_effort(
    breaking_changes: &[BreakingChange],
    storage_changes: &[StorageChange],
) -> String {
    let total_critical = breaking_changes
        .iter()
        .filter(|c| c.severity == Severity::Critical)
        .count();
    let total_major = breaking_changes
        .iter()
        .filter(|c| c.severity == Severity::Major)
        .count();
    let total_minor = breaking_changes
        .iter()
        .filter(|c| c.severity == Severity::Minor)
        .count();
    let storage_count = storage_changes.len();

    let estimated_hours =
        (total_critical * 4) + (total_major * 2) + total_minor + (storage_count / 2);

    if estimated_hours == 0 {
        "negligible (minutes)".to_string()
    } else if estimated_hours <= 2 {
        "small (~2 hours)".to_string()
    } else if estimated_hours <= 8 {
        format!("medium (~{} hours)", estimated_hours)
    } else {
        format!("large (~{} hours, may span multiple days)", estimated_hours)
    }
}

fn extract_functions(specs: &[String]) -> BTreeMap<String, String> {
    let mut fns = BTreeMap::new();
    for line in specs {
        if let Some(stripped) = line.strip_prefix("fn:") {
            let parts: Vec<&str> = stripped.splitn(2, "::").collect();
            if parts.len() == 2 {
                fns.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
            }
        }
    }
    fns
}

fn extract_types(specs: &[String]) -> BTreeMap<String, String> {
    let mut types = BTreeMap::new();
    for line in specs {
        if let Some(stripped) = line.strip_prefix("type:") {
            let parts: Vec<&str> = stripped.splitn(2, "::").collect();
            if parts.len() == 2 {
                types.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
            }
        }
    }
    types
}

fn extract_storage_keys(specs: &[String]) -> Vec<String> {
    let mut keys: Vec<String> = specs
        .iter()
        .filter_map(|line| line.strip_prefix("storage:"))
        .map(|s| s.trim().to_string())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

pub fn read_wasm_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("Failed to read WASM file: {}", path.display()))
}

pub fn extract_spec_entries(wasm_bytes: &[u8]) -> Result<Vec<String>> {
    let mut specs = Vec::new();

    if let Ok(spec_section) = find_custom_section(wasm_bytes, "contractspecv0") {
        let mut offset = 0;
        while offset < spec_section.len() {
            if let Ok((entry_str, consumed)) = parse_spec_entry(&spec_section[offset..]) {
                specs.push(entry_str);
                offset += consumed;
            } else {
                break;
            }
        }
    }

    if let Ok(meta_section) = find_custom_section(wasm_bytes, "contractmetav0") {
        let content = String::from_utf8_lossy(meta_section);
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("SDK_VERSION:") || trimmed.starts_with("PROTOCOL_VERSION:") {
                specs.push(format!("meta:{}", trimmed));
            }
        }
    }

    Ok(specs)
}

pub fn extract_sdk_version(specs: &[String]) -> Option<String> {
    for s in specs {
        if let Some(rest) = s.strip_prefix("meta:SDK_VERSION:") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

pub fn extract_protocol_version(specs: &[String]) -> Option<u32> {
    for s in specs {
        if let Some(rest) = s.strip_prefix("meta:PROTOCOL_VERSION:") {
            return rest.trim().parse::<u32>().ok();
        }
    }
    None
}

fn find_custom_section<'a>(wasm: &'a [u8], name: &str) -> Result<&'a [u8]> {
    if wasm.len() < 8 || &wasm[0..4] != b"\0asm" {
        anyhow::bail!("Not a valid WASM binary");
    }

    let mut offset = 8;
    while offset < wasm.len() {
        let section_id = wasm[offset];
        offset += 1;
        let section_len = read_leb128_u32(wasm, &mut offset)? as usize;
        let section_end = offset
            .checked_add(section_len)
            .filter(|end| *end <= wasm.len())
            .ok_or_else(|| anyhow::anyhow!("Malformed WASM section length"))?;

        if section_id == 0 {
            let name_len = read_leb128_u32(wasm, &mut offset)? as usize;
            if offset + name_len <= section_end {
                let section_name = std::str::from_utf8(&wasm[offset..offset + name_len])
                    .context("WASM custom section name is not UTF-8")?;
                offset += name_len;

                if section_name == name {
                    return Ok(&wasm[offset..section_end]);
                }
            }
        }

        offset = section_end;
    }

    anyhow::bail!("Custom section '{}' not found in WASM", name)
}

fn read_leb128_u32(bytes: &[u8], offset: &mut usize) -> Result<u32> {
    let mut result = 0u32;
    let mut shift = 0;

    loop {
        let byte = *bytes
            .get(*offset)
            .ok_or_else(|| anyhow::anyhow!("Unexpected end of WASM while reading LEB128"))?;
        *offset += 1;
        result |= ((byte & 0x7f) as u32) << shift;

        if byte & 0x80 == 0 {
            return Ok(result);
        }

        shift += 7;
        if shift >= 35 {
            anyhow::bail!("Invalid u32 LEB128 value in WASM");
        }
    }
}

fn parse_spec_entry(bytes: &[u8]) -> Result<(String, usize)> {
    let xdr_type = match bytes.first() {
        Some(0) => "function",
        Some(1) => "udt",
        Some(2) => "udt_error",
        _ => return Err(anyhow::anyhow!("Unknown spec entry type")),
    };

    let content = String::from_utf8_lossy(bytes);
    let line_end = content.find('\n').unwrap_or(content.len().min(80));
    let preview: String = content[..line_end]
        .chars()
        .filter(|c| c.is_ascii_graphic() || c.is_ascii_whitespace())
        .collect();

    let consumed = bytes.len().min(256);
    Ok((format!("{}:{}", xdr_type, preview.trim()), consumed))
}

pub fn extract_wasm_hash(wasm_bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(wasm_bytes);
    hash.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fully_compatible() {
        let old = vec!["fn:hello::(i32)->i32".into()];
        let new = vec!["fn:hello::(i32)->i32".into()];
        let config = AnalysisConfig {
            old_wasm: None,
            new_wasm: None,
            old_source: None,
            new_source: None,
            old_sdk_version: Some("21.0.0".into()),
            new_sdk_version: Some("21.0.0".into()),
            old_protocol_version: Some(22),
            new_protocol_version: Some(22),
            contract_name: Some("Test".into()),
        };
        let plan = analyze_contract_compatibility(&old, &new, "aaaa", "bbbb", &config).unwrap();
        assert_eq!(plan.compatibility, Compatibility::FullyCompatible);
        assert!(plan.breaking_changes.is_empty());
    }

    #[test]
    fn test_removed_function_is_critical() {
        let old = vec!["fn:hello::(i32)->i32".into(), "fn:goodbye::()->()".into()];
        let new = vec!["fn:hello::(i32)->i32".into()];
        let config = AnalysisConfig {
            old_wasm: None,
            new_wasm: None,
            old_source: None,
            new_source: None,
            old_sdk_version: Some("21.0.0".into()),
            new_sdk_version: Some("21.0.0".into()),
            old_protocol_version: None,
            new_protocol_version: None,
            contract_name: None,
        };
        let plan = analyze_contract_compatibility(&old, &new, "old", "new", &config).unwrap();
        assert_eq!(plan.compatibility, Compatibility::Incompatible);
        let critical: Vec<_> = plan
            .breaking_changes
            .iter()
            .filter(|c| c.severity == Severity::Critical)
            .collect();
        assert_eq!(critical.len(), 1);
        assert!(critical[0].title.contains("goodbye"));
    }

    #[test]
    fn test_signature_change_is_major() {
        let old = vec!["fn:add::(i32,i32)->i32".into()];
        let new = vec!["fn:add::(i64,i64)->i64".into()];
        let config = AnalysisConfig {
            old_wasm: None,
            new_wasm: None,
            old_source: None,
            new_source: None,
            old_sdk_version: Some("21.0.0".into()),
            new_sdk_version: Some("21.0.0".into()),
            old_protocol_version: None,
            new_protocol_version: None,
            contract_name: None,
        };
        let plan = analyze_contract_compatibility(&old, &new, "old", "new", &config).unwrap();
        let major: Vec<_> = plan
            .breaking_changes
            .iter()
            .filter(|c| c.severity == Severity::Major)
            .collect();
        assert_eq!(major.len(), 1);
        assert!(major[0].title.contains("add"));
    }

    #[test]
    fn test_sdk_major_upgrade_detected() {
        let old = vec!["fn:hello::()->()".into()];
        let new = vec!["fn:hello::()->()".into()];
        let config = AnalysisConfig {
            old_wasm: None,
            new_wasm: None,
            old_source: None,
            new_source: None,
            old_sdk_version: Some("20.0.0".into()),
            new_sdk_version: Some("21.0.0".into()),
            old_protocol_version: None,
            new_protocol_version: None,
            contract_name: None,
        };
        let plan = analyze_contract_compatibility(&old, &new, "old", "new", &config).unwrap();
        let sdk_changes: Vec<_> = plan
            .breaking_changes
            .iter()
            .filter(|c| c.category == "sdk_major_upgrade")
            .collect();
        assert_eq!(sdk_changes.len(), 1);
        assert!(sdk_changes[0].title.contains("20.0.0"));
    }

    #[test]
    fn test_protocol_upgrade_detected() {
        let old = vec!["fn:hello::()->()".into()];
        let new = vec!["fn:hello::()->()".into()];
        let config = AnalysisConfig {
            old_wasm: None,
            new_wasm: None,
            old_source: None,
            new_source: None,
            old_sdk_version: Some("21.0.0".into()),
            new_sdk_version: Some("21.0.0".into()),
            old_protocol_version: Some(22),
            new_protocol_version: Some(23),
            contract_name: None,
        };
        let plan = analyze_contract_compatibility(&old, &new, "old", "new", &config).unwrap();
        let proto_changes: Vec<_> = plan
            .breaking_changes
            .iter()
            .filter(|c| c.category == "protocol_upgrade")
            .collect();
        assert_eq!(proto_changes.len(), 1);
    }

    #[test]
    fn test_storage_changes_detected() {
        let old = vec!["storage:balance".into(), "storage:admin".into()];
        let new = vec!["storage:balance".into(), "storage:owner".into()];
        let config = AnalysisConfig {
            old_wasm: None,
            new_wasm: None,
            old_source: None,
            new_source: None,
            old_sdk_version: Some("21.0.0".into()),
            new_sdk_version: Some("21.0.0".into()),
            old_protocol_version: None,
            new_protocol_version: None,
            contract_name: Some("Token".into()),
        };
        let plan = analyze_contract_compatibility(&old, &new, "old", "new", &config).unwrap();
        assert!(plan
            .storage_changes
            .iter()
            .any(|s| s.key == "admin" && s.change_type == "removed"));
        assert!(plan
            .storage_changes
            .iter()
            .any(|s| s.key == "owner" && s.change_type == "added"));
    }

    #[test]
    fn test_rollback_strategy_generated() {
        let old = vec!["storage:old_key".into()];
        let new = vec!["storage:new_key".into()];
        let config = AnalysisConfig {
            old_wasm: None,
            new_wasm: None,
            old_source: None,
            new_source: None,
            old_sdk_version: Some("21.0.0".into()),
            new_sdk_version: Some("22.0.0".into()),
            old_protocol_version: None,
            new_protocol_version: None,
            contract_name: None,
        };
        let plan = analyze_contract_compatibility(&old, &new, "old", "new", &config).unwrap();
        assert!(plan.rollback_strategy.is_some());
        assert!(plan
            .rollback_strategy
            .as_ref()
            .unwrap()
            .contains("Rollback"));
    }

    #[test]
    fn test_migration_steps_when_no_changes() {
        let old = vec!["fn:hello::()->()".into()];
        let new = vec!["fn:hello::()->()".into()];
        let config = AnalysisConfig {
            old_wasm: None,
            new_wasm: None,
            old_source: None,
            new_source: None,
            old_sdk_version: Some("21.0.0".into()),
            new_sdk_version: Some("21.0.0".into()),
            old_protocol_version: None,
            new_protocol_version: None,
            contract_name: None,
        };
        let plan = analyze_contract_compatibility(&old, &new, "old", "new", &config).unwrap();
        assert!(
            plan.compatibility == Compatibility::FullyCompatible
                || plan.compatibility == Compatibility::CompatibleWithMigration
        );
        assert!(!plan.steps.is_empty());
        assert!(plan.steps.iter().any(|s| s.action == "backup"));
    }

    #[test]
    fn test_extract_functions() {
        let specs = vec!["fn:hello::(i32)->i32".into(), "fn:world::()->()".into()];
        let fns = extract_functions(&specs);
        assert_eq!(fns.len(), 2);
        assert_eq!(fns.get("hello"), Some(&"(i32)->i32".to_string()));
    }

    #[test]
    fn test_extract_types() {
        let specs = vec!["type:MyStruct::{ field: u32 }".into()];
        let types = extract_types(&specs);
        assert_eq!(types.len(), 1);
        assert_eq!(types.get("MyStruct"), Some(&"{ field: u32 }".to_string()));
    }

    #[test]
    fn test_extract_storage_keys() {
        let specs = vec![
            "storage:balance".into(),
            "storage:admin".into(),
            "storage:balance".into(),
        ];
        let keys = extract_storage_keys(&specs);
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"balance".to_string()));
        assert!(keys.contains(&"admin".to_string()));
    }
}
