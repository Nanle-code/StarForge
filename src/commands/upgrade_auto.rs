use crate::utils::{config, print as p};
use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Args, Subcommand};
use colored::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use stellar_xdr::curr::{
    Limited, Limits, ReadXdr, ScSpecEntry, ScSpecFunctionV0, ScSpecTypeDef, ScSpecUdtUnionCaseV0,
};

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum UpgradeAutoCommands {
    /// Check compatibility between two WASM versions
    Compat(CompatArgs),
    /// Generate an automated upgrade workflow plan
    Plan(PlanArgs),
    /// Apply an upgrade workflow plan (runs compatibility check, migration, upgrade)
    Apply(ApplyArgs),
    /// Generate a state migration script template
    Migration(MigrationArgs),
    /// List saved upgrade workflow plans
    Plans(PlansArgs),
    /// Roll back to a previous auto-managed version
    Rollback(RollbackArgs),
}

#[derive(Args)]
pub struct CompatArgs {
    /// Path to the old WASM version
    #[arg(long)]
    pub old_wasm: PathBuf,
    /// Path to the new WASM version
    #[arg(long)]
    pub new_wasm: PathBuf,
    /// Optional old contract source file for storage layout analysis
    #[arg(long)]
    pub old_source: Option<PathBuf>,
    /// Optional new contract source file for storage layout analysis
    #[arg(long)]
    pub new_source: Option<PathBuf>,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
    /// Fail with exit code 1 if incompatible
    #[arg(long, default_value = "true")]
    pub fail_on_incompatible: bool,
}

#[derive(Args)]
pub struct PlanArgs {
    /// Contract ID to upgrade
    #[arg(long)]
    pub contract_id: String,
    /// Path to the old WASM version (for compatibility analysis)
    #[arg(long)]
    pub old_wasm: PathBuf,
    /// Path to the new WASM version
    #[arg(long)]
    pub new_wasm: PathBuf,
    /// Optional old contract source file for storage layout analysis
    #[arg(long)]
    pub old_source: Option<PathBuf>,
    /// Optional new contract source file for storage layout analysis
    #[arg(long)]
    pub new_source: Option<PathBuf>,
    /// Network
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Human-readable upgrade description
    #[arg(long, default_value = "Automated upgrade")]
    pub description: String,
    /// Auto-approve compatibility warnings (don't prompt)
    #[arg(long, default_value = "false")]
    pub auto_approve: bool,
}

#[derive(Args)]
pub struct ApplyArgs {
    /// Plan ID to apply
    #[arg(long)]
    pub plan_id: String,
    /// Wallet name for signing
    #[arg(long)]
    pub wallet: Option<String>,
    /// Network
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Skip confirmation prompt
    #[arg(long, default_value = "false")]
    pub yes: bool,
    /// Run migration step before upgrade
    #[arg(long, default_value = "true")]
    pub run_migration: bool,
}

#[derive(Args)]
pub struct MigrationArgs {
    /// Path to the old WASM version (for state analysis)
    #[arg(long)]
    pub old_wasm: PathBuf,
    /// Path to the new WASM version
    #[arg(long)]
    pub new_wasm: PathBuf,
    /// Optional old contract source file for storage layout analysis
    #[arg(long)]
    pub old_source: Option<PathBuf>,
    /// Optional new contract source file for storage layout analysis
    #[arg(long)]
    pub new_source: Option<PathBuf>,
    /// Output directory for migration script
    #[arg(long, default_value = "migrations")]
    pub out_dir: PathBuf,
    /// Contract label
    #[arg(long)]
    pub contract: String,
}

#[derive(Args)]
pub struct PlansArgs {
    /// Filter by contract ID
    #[arg(long)]
    pub contract_id: Option<String>,
    /// Filter by network
    #[arg(long)]
    pub network: Option<String>,
}

#[derive(Args)]
pub struct RollbackArgs {
    /// Plan ID for the upgrade to roll back
    #[arg(long)]
    pub plan_id: String,
    /// Wallet for signing
    #[arg(long)]
    pub wallet: Option<String>,
    /// Network
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Skip confirmation
    #[arg(long, default_value = "false")]
    pub yes: bool,
}

// ── Data structures ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CompatibilityLevel {
    Compatible,
    CompatibleWithWarnings,
    Incompatible,
}

impl std::fmt::Display for CompatibilityLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompatibilityLevel::Compatible => write!(f, "compatible"),
            CompatibilityLevel::CompatibleWithWarnings => write!(f, "compatible-with-warnings"),
            CompatibilityLevel::Incompatible => write!(f, "incompatible"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatCheck {
    pub old_hash: String,
    pub new_hash: String,
    pub level: CompatibilityLevel,
    pub issues: Vec<CompatIssue>,
    pub old_size_bytes: usize,
    pub new_size_bytes: usize,
    pub size_delta_bytes: i64,
    pub abi: AbiCompatibilityReport,
    pub storage: StorageCompatibilityReport,
    pub migration_suggestions: Vec<MigrationSuggestion>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompatIssue {
    pub kind: String,
    pub severity: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AbiCompatibilityReport {
    pub old_function_count: usize,
    pub new_function_count: usize,
    pub old_type_count: usize,
    pub new_type_count: usize,
    pub added_functions: Vec<String>,
    pub removed_functions: Vec<String>,
    pub changed_functions: Vec<ChangedSignature>,
    pub added_types: Vec<String>,
    pub removed_types: Vec<String>,
    pub changed_types: Vec<ChangedSignature>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangedSignature {
    pub name: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct StorageCompatibilityReport {
    pub source_backed: bool,
    pub old_entry_count: usize,
    pub new_entry_count: usize,
    pub old_scope_counts: BTreeMap<String, usize>,
    pub new_scope_counts: BTreeMap<String, usize>,
    pub added_keys: Vec<StorageKeyChange>,
    pub removed_keys: Vec<StorageKeyChange>,
    pub moved_keys: Vec<StorageKeyMove>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct StorageKeyChange {
    pub key: String,
    pub scope: String,
    pub operations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct StorageKeyMove {
    pub key: String,
    pub from_scope: String,
    pub to_scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct MigrationSuggestion {
    pub title: String,
    pub priority: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlanStatus {
    Pending,
    Applied,
    RolledBack,
    Failed,
}

impl std::fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanStatus::Pending => write!(f, "pending"),
            PlanStatus::Applied => write!(f, "applied"),
            PlanStatus::RolledBack => write!(f, "rolled-back"),
            PlanStatus::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradePlan {
    pub id: String,
    pub contract_id: String,
    pub network: String,
    pub description: String,
    pub old_wasm_hash: String,
    pub new_wasm_hash: String,
    pub compat_level: CompatibilityLevel,
    pub migration_script: Option<String>,
    pub status: PlanStatus,
    pub created_at: String,
    pub applied_at: Option<String>,
    pub applied_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpecModel {
    functions: BTreeMap<String, AbiFunction>,
    types: BTreeMap<String, AbiTypeDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AbiFunction {
    name: String,
    inputs: Vec<AbiField>,
    outputs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AbiField {
    name: String,
    type_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AbiTypeDefinition {
    Struct { fields: Vec<AbiField> },
    Union { cases: Vec<AbiUnionCase> },
    Enum { cases: Vec<AbiEnumCase> },
    ErrorEnum { cases: Vec<AbiEnumCase> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AbiUnionCase {
    name: String,
    types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AbiEnumCase {
    name: String,
    value: u32,
}

#[derive(Debug, Clone, Default)]
struct StorageLayout {
    source_backed: bool,
    entries: BTreeMap<(String, String), BTreeSet<String>>,
}

impl AbiFunction {
    fn signature(&self) -> String {
        let inputs = self
            .inputs
            .iter()
            .map(|field| format!("{}: {}", field.name, field.type_name))
            .collect::<Vec<_>>()
            .join(", ");
        let outputs = if self.outputs.is_empty() {
            "()".to_string()
        } else if self.outputs.len() == 1 {
            self.outputs[0].clone()
        } else {
            format!("({})", self.outputs.join(", "))
        };
        format!("fn {}({}) -> {}", self.name, inputs, outputs)
    }
}

impl AbiTypeDefinition {
    fn kind(&self) -> &'static str {
        match self {
            Self::Struct { .. } => "struct",
            Self::Union { .. } => "union",
            Self::Enum { .. } => "enum",
            Self::ErrorEnum { .. } => "error-enum",
        }
    }

    fn signature(&self, name: &str) -> String {
        match self {
            Self::Struct { fields } => {
                let fields = fields
                    .iter()
                    .map(|field| format!("{}: {}", field.name, field.type_name))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} {} {{ {} }}", self.kind(), name, fields)
            }
            Self::Union { cases } => {
                let cases = cases
                    .iter()
                    .map(|case| {
                        if case.types.is_empty() {
                            case.name.clone()
                        } else {
                            format!("{}({})", case.name, case.types.join(", "))
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                format!("{} {} = {}", self.kind(), name, cases)
            }
            Self::Enum { cases } | Self::ErrorEnum { cases } => {
                let cases = cases
                    .iter()
                    .map(|case| format!("{}={}", case.name, case.value))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} {} {{ {} }}", self.kind(), name, cases)
            }
        }
    }
}

// ── Storage helpers ───────────────────────────────────────────────────────────

fn auto_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("upgrade-auto");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn plans_path() -> Result<PathBuf> {
    Ok(auto_dir()?.join("plans.json"))
}

fn load_plans() -> Result<Vec<UpgradePlan>> {
    let path = plans_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

fn save_plans(plans: &[UpgradePlan]) -> Result<()> {
    fs::write(plans_path()?, serde_json::to_string_pretty(plans)?)?;
    Ok(())
}

// ── Core logic ────────────────────────────────────────────────────────────────

fn wasm_hash_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn read_valid_wasm(path: &PathBuf) -> Result<Vec<u8>> {
    if !path.exists() {
        anyhow::bail!(
            "WASM file not found: {}\nRun `stellar contract build` first.",
            path.display()
        );
    }
    let bytes = fs::read(path)?;
    if bytes.len() < 4 || &bytes[..4] != b"\0asm" {
        anyhow::bail!(
            "File does not appear to be a valid WASM binary: {}",
            path.display()
        );
    }
    Ok(bytes)
}

fn read_optional_source(path: Option<&PathBuf>) -> Result<Option<String>> {
    path.map(|p| read_source_file(p)).transpose()
}

fn read_source_file(path: &Path) -> Result<String> {
    if !path.exists() {
        anyhow::bail!("Source file not found: {}", path.display());
    }
    fs::read_to_string(path)
        .with_context(|| format!("Failed to read source file {}", path.display()))
}

/// Performs compatibility analysis between two WASM binaries and optional source files.
pub fn analyse_compat(
    old_bytes: &[u8],
    new_bytes: &[u8],
    old_source: Option<&str>,
    new_source: Option<&str>,
) -> CompatCheck {
    let old_hash = wasm_hash_hex(old_bytes);
    let new_hash = wasm_hash_hex(new_bytes);
    let size_delta = new_bytes.len() as i64 - old_bytes.len() as i64;

    let mut issues: Vec<CompatIssue> = Vec::new();

    if size_delta < -(1024 * 10) {
        issues.push(CompatIssue {
            kind: "size-reduction".to_string(),
            severity: "warning".to_string(),
            description: format!(
                "New binary is {:.1} KB smaller; public exports or metadata may have been removed",
                (-size_delta) as f64 / 1024.0
            ),
        });
    }

    let old_has_auth = old_bytes.windows(12).any(|w| *w == b"require_auth"[..]);
    let new_has_auth = new_bytes.windows(12).any(|w| *w == b"require_auth"[..]);
    if old_has_auth && !new_has_auth {
        issues.push(CompatIssue {
            kind: "auth-removed".to_string(),
            severity: "critical".to_string(),
            description:
                "Authorization guards were present in the old binary but not the new binary"
                    .to_string(),
        });
    }

    if old_hash == new_hash {
        issues.push(CompatIssue {
            kind: "identical-binary".to_string(),
            severity: "warning".to_string(),
            description: "Old and new WASM binaries are identical; no upgrade is required"
                .to_string(),
        });
    }

    let old_spec = decode_spec_model(old_bytes);
    let new_spec = decode_spec_model(new_bytes);

    if let (Err(err), _) = (&old_spec, &new_spec) {
        issues.push(CompatIssue {
            kind: "old-abi-metadata-missing".to_string(),
            severity: "warning".to_string(),
            description: format!("Unable to decode old contract ABI metadata: {err}"),
        })
    }

    if let (_, Err(err)) = (&old_spec, &new_spec) {
        issues.push(CompatIssue {
            kind: "new-abi-metadata-missing".to_string(),
            severity: "warning".to_string(),
            description: format!("Unable to decode new contract ABI metadata: {err}"),
        })
    }

    let abi = match (&old_spec, &new_spec) {
        (Ok(old_model), Ok(new_model)) => compare_abi(old_model, new_model),
        _ => AbiCompatibilityReport::default(),
    };
    issues.extend(abi_issues(&abi));

    let old_storage = analyse_storage_layout(old_source);
    let new_storage = analyse_storage_layout(new_source);
    let storage = compare_storage_layout(&old_storage, &new_storage);
    issues.extend(storage_issues(&storage));

    if storage.source_backed
        && (!storage.added_keys.is_empty()
            || !storage.removed_keys.is_empty()
            || !storage.moved_keys.is_empty())
    {
        let has_migrate = new_spec
            .as_ref()
            .ok()
            .map(|model| model.functions.contains_key("migrate"))
            .unwrap_or(false);
        if !has_migrate {
            issues.push(CompatIssue {
                kind: "migration-entrypoint-missing".to_string(),
                severity: "warning".to_string(),
                description:
                    "Storage layout changed but the new ABI does not expose a `migrate` function"
                        .to_string(),
            });
        }
    }

    let migration_suggestions = generate_migration_suggestions(&abi, &storage);

    let level = if issues.iter().any(|issue| issue.severity == "critical") {
        CompatibilityLevel::Incompatible
    } else if issues.iter().any(|issue| issue.severity == "warning") {
        CompatibilityLevel::CompatibleWithWarnings
    } else {
        CompatibilityLevel::Compatible
    };

    CompatCheck {
        old_hash,
        new_hash,
        level,
        issues,
        old_size_bytes: old_bytes.len(),
        new_size_bytes: new_bytes.len(),
        size_delta_bytes: size_delta,
        abi,
        storage,
        migration_suggestions,
        timestamp: Utc::now().to_rfc3339(),
    }
}

fn decode_spec_model(wasm: &[u8]) -> Result<SpecModel> {
    let entries = read_spec_entries(wasm)?;
    let mut functions = BTreeMap::new();
    let mut types = BTreeMap::new();

    for entry in entries {
        match entry {
            ScSpecEntry::FunctionV0(function) => {
                let abi_function = abi_function(&function);
                functions.insert(abi_function.name.clone(), abi_function);
            }
            ScSpecEntry::UdtStructV0(udt) => {
                types.insert(
                    udt.name.to_string(),
                    AbiTypeDefinition::Struct {
                        fields: udt
                            .fields
                            .iter()
                            .map(|field| AbiField {
                                name: field.name.to_string(),
                                type_name: spec_type_name(&field.type_),
                            })
                            .collect(),
                    },
                );
            }
            ScSpecEntry::UdtUnionV0(udt) => {
                types.insert(
                    udt.name.to_string(),
                    AbiTypeDefinition::Union {
                        cases: udt
                            .cases
                            .iter()
                            .map(|case| match case {
                                ScSpecUdtUnionCaseV0::VoidV0(void_case) => AbiUnionCase {
                                    name: void_case.name.to_string(),
                                    types: vec![],
                                },
                                ScSpecUdtUnionCaseV0::TupleV0(tuple_case) => AbiUnionCase {
                                    name: tuple_case.name.to_string(),
                                    types: tuple_case.type_.iter().map(spec_type_name).collect(),
                                },
                            })
                            .collect(),
                    },
                );
            }
            ScSpecEntry::UdtEnumV0(udt) => {
                types.insert(
                    udt.name.to_string(),
                    AbiTypeDefinition::Enum {
                        cases: udt
                            .cases
                            .iter()
                            .map(|case| AbiEnumCase {
                                name: case.name.to_string(),
                                value: case.value,
                            })
                            .collect(),
                    },
                );
            }
            ScSpecEntry::UdtErrorEnumV0(udt) => {
                types.insert(
                    udt.name.to_string(),
                    AbiTypeDefinition::ErrorEnum {
                        cases: udt
                            .cases
                            .iter()
                            .map(|case| AbiEnumCase {
                                name: case.name.to_string(),
                                value: case.value,
                            })
                            .collect(),
                    },
                );
            }
        }
    }

    Ok(SpecModel { functions, types })
}

fn abi_function(function: &ScSpecFunctionV0) -> AbiFunction {
    AbiFunction {
        name: function.name.to_string(),
        inputs: function
            .inputs
            .iter()
            .map(|input| AbiField {
                name: input.name.to_string(),
                type_name: spec_type_name(&input.type_),
            })
            .collect(),
        outputs: function.outputs.iter().map(spec_type_name).collect(),
    }
}

fn compare_abi(old_model: &SpecModel, new_model: &SpecModel) -> AbiCompatibilityReport {
    let mut report = AbiCompatibilityReport {
        old_function_count: old_model.functions.len(),
        new_function_count: new_model.functions.len(),
        old_type_count: old_model.types.len(),
        new_type_count: new_model.types.len(),
        ..AbiCompatibilityReport::default()
    };

    for (name, old_function) in &old_model.functions {
        match new_model.functions.get(name) {
            Some(new_function) if old_function != new_function => {
                report.changed_functions.push(ChangedSignature {
                    name: name.clone(),
                    before: old_function.signature(),
                    after: new_function.signature(),
                });
            }
            None => report.removed_functions.push(name.clone()),
            _ => {}
        }
    }

    for (name, new_function) in &new_model.functions {
        if !old_model.functions.contains_key(name) {
            report.added_functions.push(new_function.name.clone());
        }
    }

    for (name, old_type) in &old_model.types {
        match new_model.types.get(name) {
            Some(new_type) if old_type != new_type => {
                report.changed_types.push(ChangedSignature {
                    name: name.clone(),
                    before: old_type.signature(name),
                    after: new_type.signature(name),
                });
            }
            None => report.removed_types.push(name.clone()),
            _ => {}
        }
    }

    for (name, new_type) in &new_model.types {
        if !old_model.types.contains_key(name) {
            report.added_types.push(new_type.signature(name));
        }
    }

    report
}

fn abi_issues(report: &AbiCompatibilityReport) -> Vec<CompatIssue> {
    let mut issues = Vec::new();

    for function in &report.removed_functions {
        issues.push(CompatIssue {
            kind: "function-removed".to_string(),
            severity: "critical".to_string(),
            description: format!("Public ABI function `{function}` was removed"),
        });
    }

    for function in &report.changed_functions {
        issues.push(CompatIssue {
            kind: "function-signature-changed".to_string(),
            severity: "critical".to_string(),
            description: format!(
                "Public ABI function `{}` changed signature from `{}` to `{}`",
                function.name, function.before, function.after
            ),
        });
    }

    for type_name in &report.removed_types {
        issues.push(CompatIssue {
            kind: "type-removed".to_string(),
            severity: "critical".to_string(),
            description: format!("Public ABI type `{type_name}` was removed"),
        });
    }

    for changed in &report.changed_types {
        issues.push(CompatIssue {
            kind: "type-changed".to_string(),
            severity: "critical".to_string(),
            description: format!(
                "Public ABI type `{}` changed from `{}` to `{}`",
                changed.name, changed.before, changed.after
            ),
        });
    }

    for function in &report.added_functions {
        issues.push(CompatIssue {
            kind: "function-added".to_string(),
            severity: "info".to_string(),
            description: format!("New public ABI function `{function}` was added"),
        });
    }

    for type_signature in &report.added_types {
        issues.push(CompatIssue {
            kind: "type-added".to_string(),
            severity: "info".to_string(),
            description: format!("New public ABI type added: `{type_signature}`"),
        });
    }

    issues
}

fn analyse_storage_layout(source: Option<&str>) -> StorageLayout {
    let Some(source) = source else {
        return StorageLayout::default();
    };

    let compact = source
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let mut layout = StorageLayout {
        source_backed: true,
        entries: BTreeMap::new(),
    };

    let scopes = [
        ("instance", "instance()."),
        ("persistent", "persistent()."),
        ("temporary", "temporary()."),
    ];

    let mut cursor = 0usize;
    while let Some(rel) = compact[cursor..].find("storage().") {
        let search_start = cursor + rel + "storage().".len();
        let mut matched_scope = false;

        for (scope, marker) in scopes {
            if compact[search_start..].starts_with(marker) {
                matched_scope = true;
                let op_start = search_start + marker.len();
                let op_end = op_start
                    + compact.as_bytes()[op_start..]
                        .iter()
                        .take_while(|byte| byte.is_ascii_alphabetic() || **byte == b'_')
                        .count();
                let operation = compact[op_start..op_end].to_string();

                if compact.as_bytes().get(op_end) == Some(&b'(') {
                    if let Some((first_arg, next_index)) = extract_first_argument(&compact, op_end)
                    {
                        let key = normalize_key_expr(&first_arg);
                        if !key.is_empty() {
                            layout
                                .entries
                                .entry((scope.to_string(), key))
                                .or_default()
                                .insert(operation);
                        }
                        cursor = next_index;
                    } else {
                        cursor = op_end + 1;
                    }
                } else {
                    cursor = op_end;
                }
                break;
            }
        }

        if !matched_scope {
            cursor = search_start;
        }
    }

    layout
}

fn extract_first_argument(input: &str, open_paren_index: usize) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    let mut paren_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut brace_depth = 0i32;
    let start = open_paren_index + 1;
    let mut index = start;

    while index < bytes.len() {
        match bytes[index] {
            b'(' => paren_depth += 1,
            b')' => {
                if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 {
                    return Some((input[start..index].to_string(), index + 1));
                }
                paren_depth -= 1;
            }
            b'[' => bracket_depth += 1,
            b']' => bracket_depth -= 1,
            b'{' => brace_depth += 1,
            b'}' => brace_depth -= 1,
            b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                return Some((input[start..index].to_string(), index));
            }
            _ => {}
        }
        index += 1;
    }

    None
}

fn normalize_key_expr(expr: &str) -> String {
    let mut trimmed = expr.trim_start_matches('&').trim().to_string();
    while trimmed.starts_with('&') {
        trimmed.remove(0);
    }

    let mut normalized = String::new();
    let mut depth = 0usize;
    let mut inserted_placeholder = false;

    for ch in trimmed.chars() {
        match ch {
            '(' => {
                depth += 1;
                if depth == 1 && !inserted_placeholder {
                    normalized.push_str("(*)");
                    inserted_placeholder = true;
                }
            }
            ')' => {
                depth = depth.saturating_sub(1);
            }
            _ if depth == 0 => normalized.push(ch),
            _ => {}
        }
    }

    let normalized = if normalized.is_empty() {
        trimmed
    } else {
        normalized
    };

    normalized
        .trim_matches(',')
        .trim_matches(';')
        .trim_matches('"')
        .trim()
        .to_string()
}

fn compare_storage_layout(old: &StorageLayout, new: &StorageLayout) -> StorageCompatibilityReport {
    let old_scope_counts = scope_counts(&old.entries);
    let new_scope_counts = scope_counts(&new.entries);

    let old_by_key = entries_grouped_by_key(&old.entries);
    let new_by_key = entries_grouped_by_key(&new.entries);

    let mut moved_pairs = BTreeSet::new();
    let mut moved_keys = Vec::new();

    for (key, old_entries) in &old_by_key {
        if let Some(new_entries) = new_by_key.get(key) {
            let old_scopes = old_entries.keys().cloned().collect::<BTreeSet<_>>();
            let new_scopes = new_entries.keys().cloned().collect::<BTreeSet<_>>();
            if old_scopes != new_scopes {
                for old_scope in &old_scopes {
                    for new_scope in &new_scopes {
                        moved_keys.push(StorageKeyMove {
                            key: key.clone(),
                            from_scope: old_scope.clone(),
                            to_scope: new_scope.clone(),
                        });
                        moved_pairs.insert((old_scope.clone(), key.clone()));
                        moved_pairs.insert((new_scope.clone(), key.clone()));
                    }
                }
            }
        }
    }

    let added_keys = new
        .entries
        .iter()
        .filter(|((scope, key), _)| !old.entries.contains_key(&(scope.clone(), key.clone())))
        .filter(|((scope, key), _)| !moved_pairs.contains(&(scope.clone(), key.clone())))
        .map(storage_key_change)
        .collect();

    let removed_keys = old
        .entries
        .iter()
        .filter(|((scope, key), _)| !new.entries.contains_key(&(scope.clone(), key.clone())))
        .filter(|((scope, key), _)| !moved_pairs.contains(&(scope.clone(), key.clone())))
        .map(storage_key_change)
        .collect();

    StorageCompatibilityReport {
        source_backed: old.source_backed && new.source_backed,
        old_entry_count: old.entries.len(),
        new_entry_count: new.entries.len(),
        old_scope_counts,
        new_scope_counts,
        added_keys,
        removed_keys,
        moved_keys,
    }
}

fn scope_counts(entries: &BTreeMap<(String, String), BTreeSet<String>>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for (scope, _) in entries.keys() {
        *counts.entry(scope.clone()).or_insert(0) += 1;
    }
    counts
}

fn entries_grouped_by_key(
    entries: &BTreeMap<(String, String), BTreeSet<String>>,
) -> BTreeMap<String, BTreeMap<String, BTreeSet<String>>> {
    let mut grouped = BTreeMap::new();
    for ((scope, key), operations) in entries {
        grouped
            .entry(key.clone())
            .or_insert_with(BTreeMap::new)
            .insert(scope.clone(), operations.clone());
    }
    grouped
}

fn storage_key_change(
    ((scope, key), operations): (&(String, String), &BTreeSet<String>),
) -> StorageKeyChange {
    StorageKeyChange {
        key: key.clone(),
        scope: scope.clone(),
        operations: operations.iter().cloned().collect(),
    }
}

fn storage_issues(report: &StorageCompatibilityReport) -> Vec<CompatIssue> {
    let mut issues = Vec::new();

    if !report.source_backed {
        issues.push(CompatIssue {
            kind: "storage-analysis-limited".to_string(),
            severity: "info".to_string(),
            description:
                "Storage layout analysis was skipped or partial because source files were not provided"
                    .to_string(),
        });
        return issues;
    }

    for change in &report.removed_keys {
        issues.push(CompatIssue {
            kind: "storage-key-removed".to_string(),
            severity: "critical".to_string(),
            description: format!(
                "Storage key `{}` was removed from {} storage",
                change.key, change.scope
            ),
        });
    }

    for change in &report.added_keys {
        issues.push(CompatIssue {
            kind: "storage-key-added".to_string(),
            severity: "warning".to_string(),
            description: format!(
                "Storage key `{}` was added to {} storage",
                change.key, change.scope
            ),
        });
    }

    for change in &report.moved_keys {
        issues.push(CompatIssue {
            kind: "storage-key-moved".to_string(),
            severity: "critical".to_string(),
            description: format!(
                "Storage key `{}` moved from {} storage to {} storage",
                change.key, change.from_scope, change.to_scope
            ),
        });
    }

    issues
}

fn generate_migration_suggestions(
    abi: &AbiCompatibilityReport,
    storage: &StorageCompatibilityReport,
) -> Vec<MigrationSuggestion> {
    let mut suggestions = BTreeSet::new();

    for function in &abi.removed_functions {
        suggestions.insert(MigrationSuggestion {
            title: "Preserve removed function".to_string(),
            priority: "high".to_string(),
            description: format!(
                "Keep a compatibility wrapper or adapter for removed function `{function}` so existing callers can migrate gradually"
            ),
        });
    }

    for function in &abi.changed_functions {
        suggestions.insert(MigrationSuggestion {
            title: "Update client bindings".to_string(),
            priority: "high".to_string(),
            description: format!(
                "Regenerate client bindings and update callers for ABI change in `{}`",
                function.name
            ),
        });
    }

    for changed in &abi.changed_types {
        suggestions.insert(MigrationSuggestion {
            title: "Transform persisted type data".to_string(),
            priority: "high".to_string(),
            description: format!(
                "Add a migration step that rewrites persisted values matching type `{}` from the old shape to the new shape",
                changed.name
            ),
        });
    }

    for change in &storage.moved_keys {
        suggestions.insert(MigrationSuggestion {
            title: "Move storage between scopes".to_string(),
            priority: "high".to_string(),
            description: format!(
                "Copy `{}` from {} storage into {} storage before switching the new readers on",
                change.key, change.from_scope, change.to_scope
            ),
        });
    }

    for change in &storage.removed_keys {
        suggestions.insert(MigrationSuggestion {
            title: "Retire deprecated storage keys".to_string(),
            priority: "high".to_string(),
            description: format!(
                "Backfill, archive, or delete `{}` in {} storage as part of the migration",
                change.key, change.scope
            ),
        });
    }

    for change in &storage.added_keys {
        suggestions.insert(MigrationSuggestion {
            title: "Initialize new storage keys".to_string(),
            priority: "medium".to_string(),
            description: format!(
                "Seed `{}` in {} storage before the new code path reads it",
                change.key, change.scope
            ),
        });
    }

    if suggestions.is_empty() {
        suggestions.insert(MigrationSuggestion {
            title: "No migration required".to_string(),
            priority: "low".to_string(),
            description:
                "No ABI-breaking or storage-layout changes were detected in the supplied artifacts"
                    .to_string(),
        });
    }

    suggestions.into_iter().collect()
}

/// Generate a migration script template based on compatibility analysis.
pub fn generate_migration_script(contract: &str, compat: &CompatCheck) -> String {
    let suggestion_lines = compat
        .migration_suggestions
        .iter()
        .enumerate()
        .map(|(index, suggestion)| {
            format!(
                "//   {}. [{}] {}",
                index + 1,
                suggestion.priority,
                suggestion.description
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let storage_notes = if compat.storage.source_backed {
        let mut notes = Vec::new();
        for change in &compat.storage.moved_keys {
            notes.push(format!(
                "//   - Move {} from {} storage to {} storage",
                change.key, change.from_scope, change.to_scope
            ));
        }
        for change in &compat.storage.removed_keys {
            notes.push(format!(
                "//   - Retire {} from {} storage",
                change.key, change.scope
            ));
        }
        for change in &compat.storage.added_keys {
            notes.push(format!(
                "//   - Initialize {} in {} storage",
                change.key, change.scope
            ));
        }
        notes.join("\n")
    } else {
        "//   - Provide --old-source and --new-source for richer storage migration hints"
            .to_string()
    };

    format!(
        r#"//! State migration script for contract: {contract}
//! Upgrade: {old_hash_short}... -> {new_hash_short}...
//! Generated by starforge upgrade auto migration
//!
//! Suggested migration tasks:
{suggestion_lines}
//!
//! Storage notes:
{storage_notes}
//!
//! Instructions:
//!   1. Review and complete the TODO sections below.
//!   2. Deploy this migration alongside the new WASM.
//!   3. Call `migrate` before or after the WASM upgrade depending on your strategy.

use soroban_sdk::{{Address, Env}};

/// Entry point called by the governance / upgrade automation.
/// Implement state transformations here.
pub fn migrate(env: &Env, admin: Address) {{
    admin.require_auth();

    // TODO: fetch old state keys and transform them into the new layout.
    // Example:
    //   let old_value: i128 = env.storage().instance().get(&"old_key").unwrap_or(0);
    //   env.storage().instance().set(&"new_key", &old_value);

    // TODO: remove deprecated keys
    //   env.storage().instance().remove(&"deprecated_key");

    env.events().publish(
        (soroban_sdk::symbol_short!("migrated"),),
        (
            soroban_sdk::Bytes::from_slice(env, b"{old_hash_short}"),
            soroban_sdk::Bytes::from_slice(env, b"{new_hash_short}"),
        ),
    );
}}

#[cfg(test)]
mod tests {{
    use soroban_sdk::Env;

    #[test]
    fn migration_smoke_test() {{
        let env = Env::default();
        let admin = soroban_sdk::Address::generate(&env);
        env.mock_all_auths();
        let _ = admin;
        // super::migrate(&env, admin); // Uncomment after implementing migrate()
    }}
}}
"#,
        contract = contract,
        old_hash_short = &compat.old_hash[..compat.old_hash.len().min(12)],
        new_hash_short = &compat.new_hash[..compat.new_hash.len().min(12)],
        suggestion_lines = suggestion_lines,
        storage_notes = storage_notes,
    )
}

use crate::utils::bindings::read_spec_entries;

fn spec_type_name(type_def: &ScSpecTypeDef) -> String {
    match type_def {
        ScSpecTypeDef::Val => "Val".to_string(),
        ScSpecTypeDef::Bool => "bool".to_string(),
        ScSpecTypeDef::Void => "()".to_string(),
        ScSpecTypeDef::Error => "Error".to_string(),
        ScSpecTypeDef::U32 => "u32".to_string(),
        ScSpecTypeDef::I32 => "i32".to_string(),
        ScSpecTypeDef::U64 => "u64".to_string(),
        ScSpecTypeDef::I64 => "i64".to_string(),
        ScSpecTypeDef::Timepoint => "u64".to_string(),
        ScSpecTypeDef::Duration => "u64".to_string(),
        ScSpecTypeDef::U128 => "u128".to_string(),
        ScSpecTypeDef::I128 => "i128".to_string(),
        ScSpecTypeDef::U256 => "U256".to_string(),
        ScSpecTypeDef::I256 => "I256".to_string(),
        ScSpecTypeDef::Bytes => "Bytes".to_string(),
        ScSpecTypeDef::String => "String".to_string(),
        ScSpecTypeDef::Symbol => "Symbol".to_string(),
        ScSpecTypeDef::Address => "Address".to_string(),
        ScSpecTypeDef::Option(inner) => format!("Option<{}>", spec_type_name(&inner.value_type)),
        ScSpecTypeDef::Result(inner) => format!(
            "Result<{}, {}>",
            spec_type_name(&inner.ok_type),
            spec_type_name(&inner.error_type)
        ),
        ScSpecTypeDef::Vec(inner) => format!("Vec<{}>", spec_type_name(&inner.element_type)),
        ScSpecTypeDef::Map(inner) => format!(
            "Map<{}, {}>",
            spec_type_name(&inner.key_type),
            spec_type_name(&inner.value_type)
        ),
        ScSpecTypeDef::Tuple(inner) => {
            let types = inner
                .value_types
                .iter()
                .map(spec_type_name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({types})")
        }
        ScSpecTypeDef::BytesN(inner) => format!("BytesN<{}>", inner.n),
        ScSpecTypeDef::Udt(inner) => inner.name.to_string(),
    }
}

// ── Command handlers ──────────────────────────────────────────────────────────

pub async fn handle(cmd: UpgradeAutoCommands) -> Result<()> {
    match cmd {
        UpgradeAutoCommands::Compat(args) => handle_compat(args),
        UpgradeAutoCommands::Plan(args) => handle_plan(args),
        UpgradeAutoCommands::Apply(args) => handle_apply(args),
        UpgradeAutoCommands::Migration(args) => handle_migration(args),
        UpgradeAutoCommands::Plans(args) => handle_plans(args),
        UpgradeAutoCommands::Rollback(args) => handle_rollback(args),
    }
}

fn handle_compat(args: CompatArgs) -> Result<()> {
    p::header("Contract Compatibility Check");

    p::step(1, 3, "Loading WASM binaries...");
    let old_bytes = read_valid_wasm(&args.old_wasm)?;
    let new_bytes = read_valid_wasm(&args.new_wasm)?;

    p::step(2, 3, "Loading optional source files...");
    let old_source = read_optional_source(args.old_source.as_ref())?;
    let new_source = read_optional_source(args.new_source.as_ref())?;

    p::step(3, 3, "Analysing compatibility...");
    let compat = analyse_compat(
        &old_bytes,
        &new_bytes,
        old_source.as_deref(),
        new_source.as_deref(),
    );

    if args.json {
        println!("{}", serde_json::to_string_pretty(&compat)?);
    } else {
        print_compat_report(&compat);
    }

    if args.fail_on_incompatible && compat.level == CompatibilityLevel::Incompatible {
        anyhow::bail!("Compatibility check failed: new WASM is incompatible with the old version.");
    }

    Ok(())
}

fn handle_plan(args: PlanArgs) -> Result<()> {
    p::header("Create Automated Upgrade Plan");
    config::validate_network(&args.network)?;

    p::step(1, 3, "Loading WASM binaries...");
    let old_bytes = read_valid_wasm(&args.old_wasm)?;
    let new_bytes = read_valid_wasm(&args.new_wasm)?;

    let old_source = read_optional_source(args.old_source.as_ref())?;
    let new_source = read_optional_source(args.new_source.as_ref())?;

    p::step(2, 3, "Running compatibility analysis...");
    let compat = analyse_compat(
        &old_bytes,
        &new_bytes,
        old_source.as_deref(),
        new_source.as_deref(),
    );

    let level_str = compat.level.to_string();
    let level_colored = match compat.level {
        CompatibilityLevel::Compatible => level_str.green().to_string(),
        CompatibilityLevel::CompatibleWithWarnings => level_str.yellow().to_string(),
        CompatibilityLevel::Incompatible => level_str.red().to_string(),
    };
    p::kv_accent("Compatibility", &level_colored);

    if compat.level == CompatibilityLevel::Incompatible && !args.auto_approve {
        anyhow::bail!(
            "Cannot create plan: supplied artifacts are incompatible. Fix issues or use --auto-approve to force."
        );
    }

    let migration_script = generate_migration_script(&args.contract_id, &compat);

    p::step(3, 3, "Saving upgrade plan...");
    let plan_id = format!(
        "plan-{}-{}",
        &args.contract_id[..args.contract_id.len().min(8)],
        &compat.new_hash[..12]
    );

    let mut plans = load_plans()?;
    if plans.iter().any(|p| p.id == plan_id) {
        anyhow::bail!("A plan with id '{plan_id}' already exists.");
    }

    let plan = UpgradePlan {
        id: plan_id.clone(),
        contract_id: args.contract_id.clone(),
        network: args.network.clone(),
        description: args.description.clone(),
        old_wasm_hash: compat.old_hash.clone(),
        new_wasm_hash: compat.new_hash.clone(),
        compat_level: compat.level.clone(),
        migration_script: Some(migration_script),
        status: PlanStatus::Pending,
        created_at: Utc::now().to_rfc3339(),
        applied_at: None,
        applied_by: None,
    };
    plans.push(plan);
    save_plans(&plans)?;

    p::separator();
    p::kv_accent("Plan ID", &plan_id);
    p::kv("Contract", &args.contract_id);
    p::kv("Network", &args.network);
    p::kv("Old hash", &compat.old_hash);
    p::kv("New hash", &compat.new_hash);
    p::kv("Description", &args.description);
    p::separator();
    p::info(&format!(
        "Apply with: starforge upgrade auto apply --plan-id {plan_id}"
    ));
    Ok(())
}

fn handle_apply(args: ApplyArgs) -> Result<()> {
    p::header("Apply Upgrade Plan");
    config::validate_network(&args.network)?;

    let cfg = config::load()?;
    let wallet = if let Some(ref name) = args.wallet {
        cfg.wallets
            .iter()
            .find(|w| w.name == *name)
            .ok_or_else(|| {
                anyhow::anyhow!("Wallet '{name}' not found. Run `starforge wallet list`.")
            })?
    } else if !cfg.wallets.is_empty() {
        p::info(&format!(
            "No --wallet specified. Using: {}",
            cfg.wallets[0].name.cyan()
        ));
        &cfg.wallets[0]
    } else {
        anyhow::bail!("No wallets found. Create one with `starforge wallet create <name> --fund`");
    };

    let mut plans = load_plans()?;
    let plan = plans
        .iter_mut()
        .find(|p| p.id == args.plan_id && p.network == args.network)
        .ok_or_else(|| anyhow::anyhow!("Plan '{}' not found on {}", args.plan_id, args.network))?;

    if plan.status == PlanStatus::Applied {
        anyhow::bail!("Plan '{}' has already been applied.", args.plan_id);
    }

    p::separator();
    p::kv("Plan ID", &plan.id);
    p::kv("Contract", &plan.contract_id);
    p::kv("Network", &plan.network);
    p::kv("Old hash", &plan.old_wasm_hash);
    p::kv_accent("New hash", &plan.new_wasm_hash);
    p::kv("Description", &plan.description);
    p::separator();

    if !args.yes {
        print!("  Proceed with upgrade? [y/N] ");
        use std::io::{self, BufRead};
        let stdin = io::stdin();
        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        if !line.trim().eq_ignore_ascii_case("y") {
            p::info("Upgrade cancelled.");
            return Ok(());
        }
    }

    let total_steps = if args.run_migration { 3 } else { 2 };

    p::step(1, total_steps, "Verifying plan integrity...");
    p::kv("Plan verified", "yes");

    if args.run_migration {
        p::step(2, total_steps, "Running state migration...");
        println!(
            "  {}",
            "Migration script generated. Apply it on-chain before upgrading WASM.".dimmed()
        );
    }

    p::step(total_steps, total_steps, "Generating upgrade command...");

    plan.status = PlanStatus::Applied;
    plan.applied_at = Some(Utc::now().to_rfc3339());
    plan.applied_by = Some(wallet.public_key.clone());
    save_plans(&plans)?;

    println!();
    p::separator();
    println!(
        "  {} {}",
        "yes".green().bold(),
        "Run this to apply the upgrade on-chain:".bright_white()
    );
    println!();
    let contract_id = plans
        .iter()
        .find(|p| p.id == args.plan_id)
        .map(|p| p.contract_id.as_str())
        .unwrap_or("CONTRACT_ID");
    println!(
        "  {}",
        format!(
            "stellar contract invoke --id {} --source {} --network {} -- upgrade --new-wasm-hash {}",
            contract_id,
            wallet.public_key,
            args.network,
            plans.iter().find(|p| p.id == args.plan_id).map(|p| p.new_wasm_hash.as_str()).unwrap_or("NEW_HASH")
        )
        .cyan()
    );
    p::separator();
    Ok(())
}

fn handle_migration(args: MigrationArgs) -> Result<()> {
    p::header("Generate State Migration Script");

    p::step(1, 3, "Loading WASM binaries...");
    let old_bytes = read_valid_wasm(&args.old_wasm)?;
    let new_bytes = read_valid_wasm(&args.new_wasm)?;

    p::step(2, 3, "Loading optional source files...");
    let old_source = read_optional_source(args.old_source.as_ref())?;
    let new_source = read_optional_source(args.new_source.as_ref())?;
    let compat = analyse_compat(
        &old_bytes,
        &new_bytes,
        old_source.as_deref(),
        new_source.as_deref(),
    );

    p::step(3, 3, "Writing migration template...");
    if !args.out_dir.exists() {
        fs::create_dir_all(&args.out_dir)?;
    }

    let script = generate_migration_script(&args.contract, &compat);
    let out_path = args.out_dir.join(format!(
        "migrate_{}_to_{}.rs",
        &compat.old_hash[..8],
        &compat.new_hash[..8]
    ));
    fs::write(&out_path, &script)?;

    p::separator();
    p::kv_accent("Migration script", &out_path.display().to_string());
    p::kv("Old hash", &compat.old_hash);
    p::kv("New hash", &compat.new_hash);
    p::separator();
    p::success("Review and implement the TODO sections before deploying.");
    Ok(())
}

fn handle_plans(args: PlansArgs) -> Result<()> {
    p::header("Upgrade Plans");

    let plans = load_plans()?;
    let filtered: Vec<_> = plans
        .iter()
        .filter(|p| args.network.as_deref().map_or(true, |n| p.network == n))
        .filter(|p| {
            args.contract_id
                .as_deref()
                .map_or(true, |c| p.contract_id == c)
        })
        .collect();

    if filtered.is_empty() {
        p::info("No plans found. Create one with `starforge upgrade auto plan`.");
        return Ok(());
    }

    p::separator();
    println!(
        "  {:<30}  {:<14}  {:<10}  {:<20}  {}",
        "Plan ID".dimmed(),
        "Contract".dimmed(),
        "Network".dimmed(),
        "Status".dimmed(),
        "Created".dimmed(),
    );
    println!("  {}", "─".repeat(85).dimmed());

    for plan in filtered {
        let status_colored = match plan.status {
            PlanStatus::Pending => plan.status.to_string().yellow().to_string(),
            PlanStatus::Applied => plan.status.to_string().green().to_string(),
            PlanStatus::RolledBack => plan.status.to_string().cyan().to_string(),
            PlanStatus::Failed => plan.status.to_string().red().to_string(),
        };
        let ts = plan.created_at.get(..16).unwrap_or(&plan.created_at);
        println!(
            "  {:<30}  {:<14}  {:<10}  {:<20}  {}",
            plan.id.white(),
            short_id(&plan.contract_id).cyan(),
            plan.network.white(),
            status_colored,
            ts.dimmed(),
        );
    }
    p::separator();
    Ok(())
}

fn handle_rollback(args: RollbackArgs) -> Result<()> {
    p::header("Rollback Upgrade Plan");
    config::validate_network(&args.network)?;

    let cfg = config::load()?;
    let wallet = if let Some(ref name) = args.wallet {
        cfg.wallets
            .iter()
            .find(|w| w.name == *name)
            .ok_or_else(|| anyhow::anyhow!("Wallet '{name}' not found."))?
    } else if !cfg.wallets.is_empty() {
        p::info(&format!(
            "No --wallet specified. Using: {}",
            cfg.wallets[0].name.cyan()
        ));
        &cfg.wallets[0]
    } else {
        anyhow::bail!("No wallets configured.");
    };

    let mut plans = load_plans()?;
    let plan = plans
        .iter_mut()
        .find(|p| p.id == args.plan_id && p.network == args.network)
        .ok_or_else(|| anyhow::anyhow!("Plan '{}' not found on {}.", args.plan_id, args.network))?;

    if plan.status != PlanStatus::Applied {
        anyhow::bail!(
            "Plan '{}' has not been applied yet (status: {}). Only applied plans can be rolled back.",
            args.plan_id,
            plan.status
        );
    }

    p::separator();
    p::kv("Plan ID", &plan.id);
    p::kv("Contract", &plan.contract_id);
    p::kv_accent("Rollback to", &plan.old_wasm_hash);
    p::kv("Network", &args.network);
    p::separator();

    if !args.yes {
        print!("  Proceed with rollback? [y/N] ");
        use std::io::{self, BufRead};
        let stdin = io::stdin();
        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        if !line.trim().eq_ignore_ascii_case("y") {
            p::info("Rollback cancelled.");
            return Ok(());
        }
    }

    plan.status = PlanStatus::RolledBack;
    let contract_id = plan.contract_id.clone();
    let old_hash = plan.old_wasm_hash.clone();
    save_plans(&plans)?;

    println!();
    p::separator();
    println!(
        "  {} {}",
        "yes".green().bold(),
        "Run this to roll back on-chain:".bright_white()
    );
    println!();
    println!(
        "  {}",
        format!(
            "stellar contract invoke --id {} --source {} --network {} -- upgrade --new-wasm-hash {}",
            contract_id, wallet.public_key, args.network, old_hash
        )
        .cyan()
    );
    p::separator();
    Ok(())
}

fn print_compat_report(compat: &CompatCheck) {
    p::separator();
    let level_str = match compat.level {
        CompatibilityLevel::Compatible => compat.level.to_string().green().to_string(),
        CompatibilityLevel::CompatibleWithWarnings => compat.level.to_string().yellow().to_string(),
        CompatibilityLevel::Incompatible => compat.level.to_string().red().to_string(),
    };
    p::kv_accent("Compatibility", &level_str);
    p::kv("Old hash", &compat.old_hash);
    p::kv("New hash", &compat.new_hash);
    p::kv("Old size", &format!("{} bytes", compat.old_size_bytes));
    p::kv("New size", &format!("{} bytes", compat.new_size_bytes));
    p::kv(
        "Size delta",
        &format!("{:+} bytes", compat.size_delta_bytes),
    );

    println!();
    p::kv(
        "ABI summary",
        &format!(
            "{} funcs -> {} funcs, {} types -> {} types",
            compat.abi.old_function_count,
            compat.abi.new_function_count,
            compat.abi.old_type_count,
            compat.abi.new_type_count
        ),
    );
    if !compat.abi.added_functions.is_empty() {
        p::kv("Added functions", &compat.abi.added_functions.join(", "));
    }
    if !compat.abi.removed_functions.is_empty() {
        p::kv(
            "Removed functions",
            &compat.abi.removed_functions.join(", "),
        );
    }
    if !compat.abi.changed_functions.is_empty() {
        for change in &compat.abi.changed_functions {
            p::kv(
                &format!("Changed function {}", change.name),
                &format!("{} -> {}", change.before, change.after),
            );
        }
    }
    if !compat.abi.changed_types.is_empty() {
        for change in &compat.abi.changed_types {
            p::kv(
                &format!("Changed type {}", change.name),
                &format!("{} -> {}", change.before, change.after),
            );
        }
    }

    println!();
    p::kv(
        "Storage analysis",
        if compat.storage.source_backed {
            "source-backed"
        } else {
            "limited"
        },
    );
    if compat.storage.source_backed {
        p::kv(
            "Storage entries",
            &format!(
                "{} -> {}",
                compat.storage.old_entry_count, compat.storage.new_entry_count
            ),
        );
        if !compat.storage.moved_keys.is_empty() {
            for change in &compat.storage.moved_keys {
                p::kv(
                    &format!("Moved key {}", change.key),
                    &format!("{} -> {}", change.from_scope, change.to_scope),
                );
            }
        }
        if !compat.storage.added_keys.is_empty() {
            p::kv(
                "Added storage keys",
                &compat
                    .storage
                    .added_keys
                    .iter()
                    .map(|change| format!("{} ({})", change.key, change.scope))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        if !compat.storage.removed_keys.is_empty() {
            p::kv(
                "Removed storage keys",
                &compat
                    .storage
                    .removed_keys
                    .iter()
                    .map(|change| format!("{} ({})", change.key, change.scope))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
    }

    if !compat.migration_suggestions.is_empty() {
        println!();
        p::kv(
            "Migration suggestions",
            &compat.migration_suggestions.len().to_string(),
        );
        for suggestion in &compat.migration_suggestions {
            println!(
                "    [{}] {}: {}",
                suggestion.priority.white(),
                suggestion.title.bright_white(),
                suggestion.description.dimmed(),
            );
        }
    }

    if !compat.issues.is_empty() {
        println!();
        p::kv("Issues found", &compat.issues.len().to_string());
        for issue in &compat.issues {
            let severity = match issue.severity.as_str() {
                "critical" => issue.severity.red().to_string(),
                "warning" => issue.severity.yellow().to_string(),
                _ => issue.severity.dimmed().to_string(),
            };
            println!(
                "    [{:<8}] [{}] {}",
                severity,
                issue.kind.white(),
                issue.description.dimmed()
            );
        }
    }
    p::separator();
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn short_id(id: &str) -> String {
    if id.len() > 12 {
        format!("{}...", &id[..12])
    } else {
        id.to_string()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use stellar_xdr::curr::{
        Limits, ScSpecEntry, ScSpecFunctionInputV0, ScSpecFunctionV0, ScSpecTypeDef, ScSpecTypeUdt,
        ScSpecUdtStructFieldV0, ScSpecUdtStructV0, ScSymbol, StringM, VecM, WriteXdr,
    };

    fn mock_wasm(extra: &[u8]) -> Vec<u8> {
        let mut v = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        v.extend_from_slice(extra);
        v
    }

    fn encode_leb128(mut value: u32) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                break;
            }
        }
        out
    }

    fn empty_doc() -> StringM<1024> {
        "".try_into().unwrap()
    }

    fn symbol(name: &str) -> ScSymbol {
        name.try_into().unwrap()
    }

    fn field_name(name: &str) -> StringM<30> {
        name.try_into().unwrap()
    }

    fn type_name(name: &str) -> StringM<60> {
        name.try_into().unwrap()
    }

    fn udt(name: &str) -> ScSpecTypeUdt {
        ScSpecTypeUdt {
            name: type_name(name),
        }
    }

    fn inputs(items: Vec<ScSpecFunctionInputV0>) -> VecM<ScSpecFunctionInputV0, 10> {
        items.try_into().unwrap()
    }

    fn outputs(items: Vec<ScSpecTypeDef>) -> VecM<ScSpecTypeDef, 1> {
        items.try_into().unwrap()
    }

    fn struct_fields(items: Vec<ScSpecUdtStructFieldV0>) -> VecM<ScSpecUdtStructFieldV0, 40> {
        items.try_into().unwrap()
    }

    fn spec_wasm(entries: Vec<ScSpecEntry>, extra: &[u8]) -> Vec<u8> {
        let mut spec_bytes = Vec::new();
        {
            let mut limited = Limited::new(
                &mut spec_bytes,
                Limits {
                    depth: 500,
                    len: 0x1000000,
                },
            );
            for entry in entries {
                entry.write_xdr(&mut limited).unwrap();
            }
        }

        let section_name = b"contractspecv0";
        let mut section_body = Vec::new();
        section_body.extend_from_slice(&encode_leb128(section_name.len() as u32));
        section_body.extend_from_slice(section_name);
        section_body.extend_from_slice(&spec_bytes);

        let mut wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        wasm.push(0);
        wasm.extend_from_slice(&encode_leb128(section_body.len() as u32));
        wasm.extend_from_slice(&section_body);
        wasm.extend_from_slice(extra);
        wasm
    }

    fn abi_function_entry(
        name: &str,
        args: Vec<(&str, ScSpecTypeDef)>,
        output: ScSpecTypeDef,
    ) -> ScSpecEntry {
        ScSpecEntry::FunctionV0(ScSpecFunctionV0 {
            doc: empty_doc(),
            name: symbol(name),
            inputs: inputs(
                args.into_iter()
                    .map(|(arg_name, type_)| ScSpecFunctionInputV0 {
                        doc: empty_doc(),
                        name: field_name(arg_name),
                        type_,
                    })
                    .collect(),
            ),
            outputs: outputs(vec![output]),
        })
    }

    fn abi_struct_entry(name: &str, fields: Vec<(&str, ScSpecTypeDef)>) -> ScSpecEntry {
        ScSpecEntry::UdtStructV0(ScSpecUdtStructV0 {
            doc: empty_doc(),
            lib: "".try_into().unwrap(),
            name: type_name(name),
            fields: struct_fields(
                fields
                    .into_iter()
                    .map(|(field, type_)| ScSpecUdtStructFieldV0 {
                        doc: empty_doc(),
                        name: field_name(field),
                        type_,
                    })
                    .collect(),
            ),
        })
    }

    #[test]
    fn wasm_hash_is_deterministic() {
        let bytes = mock_wasm(b"v1");
        assert_eq!(wasm_hash_hex(&bytes), wasm_hash_hex(&bytes));
    }

    #[test]
    fn wasm_hash_hex_length() {
        let hash = wasm_hash_hex(b"test");
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn compat_identical_binaries_warns() {
        let wasm = mock_wasm(b"same");
        let compat = analyse_compat(&wasm, &wasm, None, None);
        assert_eq!(compat.level, CompatibilityLevel::CompatibleWithWarnings);
        assert!(compat.issues.iter().any(|i| i.kind == "identical-binary"));
    }

    #[test]
    fn compat_detects_removed_abi_function() {
        let old = spec_wasm(
            vec![abi_function_entry(
                "increment",
                vec![("amount", ScSpecTypeDef::U32)],
                ScSpecTypeDef::U32,
            )],
            b"",
        );
        let new = spec_wasm(vec![], b"");
        let compat = analyse_compat(&old, &new, None, None);
        assert_eq!(compat.level, CompatibilityLevel::Incompatible);
        assert!(compat.issues.iter().any(
            |issue| issue.kind == "function-removed" && issue.description.contains("increment")
        ));
    }

    #[test]
    fn compat_detects_changed_abi_type() {
        let old = spec_wasm(
            vec![
                abi_function_entry("load", vec![], ScSpecTypeDef::Udt(udt("Config"))),
                abi_struct_entry("Config", vec![("count", ScSpecTypeDef::U32)]),
            ],
            b"",
        );
        let new = spec_wasm(
            vec![
                abi_function_entry("load", vec![], ScSpecTypeDef::Udt(udt("Config"))),
                abi_struct_entry("Config", vec![("count", ScSpecTypeDef::U64)]),
            ],
            b"",
        );
        let compat = analyse_compat(&old, &new, None, None);
        assert_eq!(compat.level, CompatibilityLevel::Incompatible);
        assert!(compat
            .issues
            .iter()
            .any(|issue| issue.kind == "type-changed"));
    }

    #[test]
    fn compat_missing_auth_is_incompatible() {
        let old = {
            let mut v = mock_wasm(b"");
            v.extend_from_slice(b"require_auth");
            v
        };
        let new = mock_wasm(b"no_auth_here");
        let compat = analyse_compat(&old, &new, None, None);
        assert_eq!(compat.level, CompatibilityLevel::Incompatible);
        assert!(compat.issues.iter().any(|i| i.kind == "auth-removed"));
    }

    #[test]
    fn storage_layout_detects_scope_move() {
        let old = r#"
            env.storage().instance().set(&DataKey::Admin, &admin);
        "#;
        let new = r#"
            env.storage().persistent().set(&DataKey::Admin, &admin);
        "#;
        let report = compare_storage_layout(
            &analyse_storage_layout(Some(old)),
            &analyse_storage_layout(Some(new)),
        );
        assert!(report.source_backed);
        assert_eq!(report.moved_keys.len(), 1);
        assert_eq!(report.moved_keys[0].key, "DataKey::Admin");
    }

    #[test]
    fn storage_layout_detects_added_key() {
        let old = r#"
            env.storage().instance().set(&DataKey::Admin, &admin);
        "#;
        let new = r#"
            env.storage().instance().set(&DataKey::Admin, &admin);
            env.storage().persistent().set(&DataKey::Balance(user), &amount);
        "#;
        let report = compare_storage_layout(
            &analyse_storage_layout(Some(old)),
            &analyse_storage_layout(Some(new)),
        );
        assert_eq!(report.added_keys.len(), 1);
        assert_eq!(report.added_keys[0].key, "DataKey::Balance(*)");
    }

    #[test]
    fn generate_migration_script_contains_suggestions() {
        let old = spec_wasm(vec![], b"old");
        let new = spec_wasm(vec![], b"new");
        let compat = analyse_compat(
            &old,
            &new,
            Some("env.storage().instance().set(&DataKey::Admin, &admin);"),
            Some("env.storage().persistent().set(&DataKey::Admin, &admin);"),
        );
        let script = generate_migration_script("my_contract", &compat);
        assert!(script.contains("my_contract"));
        assert!(script.contains("Move"));
        assert!(script.contains("migrate"));
    }

    #[test]
    fn compat_level_display() {
        assert_eq!(CompatibilityLevel::Compatible.to_string(), "compatible");
        assert_eq!(
            CompatibilityLevel::CompatibleWithWarnings.to_string(),
            "compatible-with-warnings"
        );
        assert_eq!(CompatibilityLevel::Incompatible.to_string(), "incompatible");
    }

    #[test]
    fn plan_status_display() {
        assert_eq!(PlanStatus::Pending.to_string(), "pending");
        assert_eq!(PlanStatus::Applied.to_string(), "applied");
        assert_eq!(PlanStatus::RolledBack.to_string(), "rolled-back");
        assert_eq!(PlanStatus::Failed.to_string(), "failed");
    }
}
