//! Advanced gas optimization analyzer for Soroban contracts.
//!
//! Provides deep WASM binary profiling, per-function gas estimation,
//! optimization suggestions, version comparison, and JSON/text report output.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::config;

// ── Soroban fee constants (approximations matching Stellar Core fee schedule) ──

/// Base fee per WASM instruction (CPU weight units / 1_000_000).
pub const CPU_PER_INSTRUCTION: u64 = 25;
/// Base fee per byte of WASM code section.
pub const FEE_PER_CODE_BYTE: u64 = 12;
/// Base fee per imported function.
pub const FEE_PER_IMPORT: u64 = 500;
/// Base fee per exported function.
pub const FEE_PER_EXPORT: u64 = 200;
/// Base fee per global variable.
pub const FEE_PER_GLOBAL: u64 = 150;
/// Base fee per data segment.
pub const FEE_PER_DATA_SEGMENT: u64 = 300;
/// Base fee per element segment.
pub const FEE_PER_ELEMENT_SEGMENT: u64 = 250;
/// Soroban WASM upload size limit in bytes.
pub const WASM_SIZE_LIMIT: usize = 128 * 1024;

// ── Section identifiers (WASM binary format) ─────────────────────────────────

const SECTION_TYPE: u8 = 1;
const SECTION_IMPORT: u8 = 2;
const SECTION_FUNCTION: u8 = 3;
const SECTION_TABLE: u8 = 4;
const SECTION_MEMORY: u8 = 5;
const SECTION_GLOBAL: u8 = 6;
const SECTION_EXPORT: u8 = 7;
const SECTION_START: u8 = 8;
const SECTION_ELEMENT: u8 = 9;
const SECTION_CODE: u8 = 10;
const SECTION_DATA: u8 = 11;
const SECTION_CUSTOM: u8 = 0;

// ── Core data structures ──────────────────────────────────────────────────────

/// Severity level for a gas optimization finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl std::fmt::Display for FindingSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FindingSeverity::Critical => write!(f, "critical"),
            FindingSeverity::High => write!(f, "high"),
            FindingSeverity::Medium => write!(f, "medium"),
            FindingSeverity::Low => write!(f, "low"),
            FindingSeverity::Info => write!(f, "info"),
        }
    }
}

/// A single gas optimization finding/suggestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasFinding {
    /// Machine-readable identifier, e.g. "GAS-001".
    pub id: String,
    /// Short category label, e.g. "binary-size", "debug-info".
    pub kind: String,
    pub severity: FindingSeverity,
    pub description: String,
    pub recommendation: String,
    /// Estimated gas savings if this finding is resolved (0–100%).
    pub estimated_saving_pct: f64,
    /// Estimated absolute gas units saved.
    pub estimated_gas_saving: u64,
}

/// Detailed breakdown of a parsed WASM section.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WasmSectionProfile {
    pub type_count: usize,
    pub import_count: usize,
    pub function_count: usize,
    pub table_count: usize,
    pub memory_count: usize,
    pub global_count: usize,
    pub export_count: usize,
    pub element_segment_count: usize,
    pub data_segment_count: usize,
    pub code_section_bytes: usize,
    pub custom_section_bytes: usize,
    pub has_start_function: bool,
    pub estimated_instruction_count: usize,
    pub has_name_section: bool,
    pub has_debug_section: bool,
}

/// Estimated gas cost breakdown by component.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GasCostBreakdown {
    /// Estimated cost for WASM upload/storage.
    pub upload_cost: u64,
    /// Estimated CPU execution cost.
    pub cpu_cost: u64,
    /// Import overhead cost.
    pub import_cost: u64,
    /// Export overhead cost.
    pub export_cost: u64,
    /// Global variable overhead.
    pub global_cost: u64,
    /// Data segment overhead.
    pub data_cost: u64,
    /// Total estimated gas cost.
    pub total: u64,
    /// Cost per kilobyte of WASM.
    pub cost_per_kb: f64,
}

impl GasCostBreakdown {
    pub fn compute(profile: &WasmSectionProfile, size_bytes: usize) -> Self {
        let upload_cost = (size_bytes as u64).saturating_mul(FEE_PER_CODE_BYTE);
        let cpu_cost =
            (profile.estimated_instruction_count as u64).saturating_mul(CPU_PER_INSTRUCTION);
        let import_cost = (profile.import_count as u64).saturating_mul(FEE_PER_IMPORT);
        let export_cost = (profile.export_count as u64).saturating_mul(FEE_PER_EXPORT);
        let global_cost = (profile.global_count as u64).saturating_mul(FEE_PER_GLOBAL);
        let data_cost = (profile.data_segment_count as u64).saturating_mul(FEE_PER_DATA_SEGMENT);
        let total = upload_cost
            .saturating_add(cpu_cost)
            .saturating_add(import_cost)
            .saturating_add(export_cost)
            .saturating_add(global_cost)
            .saturating_add(data_cost);
        let cost_per_kb = if size_bytes == 0 {
            0.0
        } else {
            total as f64 / (size_bytes as f64 / 1024.0)
        };
        Self {
            upload_cost,
            cpu_cost,
            import_cost,
            export_cost,
            global_cost,
            data_cost,
            total,
            cost_per_kb,
        }
    }
}

/// Full gas analysis report for a WASM binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasAnalysisReport {
    /// Unique report ID derived from the WASM hash.
    pub id: String,
    /// RFC3339 timestamp.
    pub generated_at: String,
    /// Contract label (user supplied or derived from path).
    pub contract_label: String,
    /// Path to analyzed WASM.
    pub wasm_path: String,
    /// SHA-256 of the WASM bytes.
    pub wasm_sha256: String,
    /// Total WASM size in bytes.
    pub size_bytes: usize,
    /// Percentage of Soroban size limit used.
    pub size_limit_pct: f64,
    /// Structural section breakdown.
    pub section_profile: WasmSectionProfile,
    /// Estimated gas cost breakdown.
    pub gas_cost: GasCostBreakdown,
    /// Overall optimization score (0–100, higher = better).
    pub optimization_score: u8,
    /// All gas findings.
    pub findings: Vec<GasFinding>,
    /// Prioritized action list.
    pub top_recommendations: Vec<String>,
}

impl GasAnalysisReport {
    pub fn critical_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == FindingSeverity::Critical)
            .count()
    }

    pub fn high_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == FindingSeverity::High)
            .count()
    }

    pub fn medium_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == FindingSeverity::Medium)
            .count()
    }

    pub fn total_estimated_gas_saving(&self) -> u64 {
        self.findings.iter().map(|f| f.estimated_gas_saving).sum()
    }
}

/// Comparison report between two WASM versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasVersionComparison {
    pub id: String,
    pub generated_at: String,
    pub baseline_path: String,
    pub candidate_path: String,
    pub baseline_sha256: String,
    pub candidate_sha256: String,
    pub baseline_size_bytes: usize,
    pub candidate_size_bytes: usize,
    pub size_delta_bytes: i64,
    pub size_delta_pct: f64,
    pub baseline_gas_cost: GasCostBreakdown,
    pub candidate_gas_cost: GasCostBreakdown,
    pub gas_delta: i64,
    pub gas_delta_pct: f64,
    pub baseline_score: u8,
    pub candidate_score: u8,
    pub score_delta: i8,
    pub baseline_instruction_count: usize,
    pub candidate_instruction_count: usize,
    pub instruction_delta: i64,
    pub instruction_delta_pct: f64,
    pub verdict: String,
    pub new_findings: Vec<GasFinding>,
    pub resolved_findings: usize,
}

// ── WASM parser ───────────────────────────────────────────────────────────────

/// Parse WASM magic bytes and version.
fn is_valid_wasm(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[..4] == b"\0asm"
}

/// Decode a LEB128 unsigned integer, returning (value, bytes_consumed).
fn read_uleb128(bytes: &[u8], offset: usize) -> Option<(u64, usize)> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    let mut pos = offset;
    loop {
        if pos >= bytes.len() {
            return None;
        }
        let byte = bytes[pos] as u64;
        pos += 1;
        result |= (byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    Some((result, pos - offset))
}

/// Parse WASM sections and extract structural profile.
pub fn parse_wasm_sections(bytes: &[u8]) -> WasmSectionProfile {
    let mut profile = WasmSectionProfile::default();
    if !is_valid_wasm(bytes) {
        return profile;
    }

    let mut pos = 8; // skip magic + version
    while pos < bytes.len() {
        // Section id
        let section_id = bytes[pos];
        pos += 1;
        // Section size
        let (section_size, consumed) = match read_uleb128(bytes, pos) {
            Some(v) => v,
            None => break,
        };
        pos += consumed;
        let section_end = pos + section_size as usize;
        if section_end > bytes.len() {
            break;
        }
        let section_bytes = &bytes[pos..section_end];

        match section_id {
            SECTION_TYPE => {
                if let Some((count, _)) = read_uleb128(section_bytes, 0) {
                    profile.type_count = count as usize;
                }
            }
            SECTION_IMPORT => {
                if let Some((count, _)) = read_uleb128(section_bytes, 0) {
                    profile.import_count = count as usize;
                }
            }
            SECTION_FUNCTION => {
                if let Some((count, _)) = read_uleb128(section_bytes, 0) {
                    profile.function_count = count as usize;
                }
            }
            SECTION_TABLE => {
                if let Some((count, _)) = read_uleb128(section_bytes, 0) {
                    profile.table_count = count as usize;
                }
            }
            SECTION_MEMORY => {
                if let Some((count, _)) = read_uleb128(section_bytes, 0) {
                    profile.memory_count = count as usize;
                }
            }
            SECTION_GLOBAL => {
                if let Some((count, _)) = read_uleb128(section_bytes, 0) {
                    profile.global_count = count as usize;
                }
            }
            SECTION_EXPORT => {
                if let Some((count, _)) = read_uleb128(section_bytes, 0) {
                    profile.export_count = count as usize;
                }
            }
            SECTION_START => {
                profile.has_start_function = true;
            }
            SECTION_ELEMENT => {
                if let Some((count, _)) = read_uleb128(section_bytes, 0) {
                    profile.element_segment_count = count as usize;
                }
            }
            SECTION_CODE => {
                profile.code_section_bytes = section_bytes.len();
                // Estimate instruction count: non-header bytes that are instruction opcodes
                profile.estimated_instruction_count =
                    section_bytes.iter().filter(|&&b| b <= 0xBF).count();
            }
            SECTION_DATA => {
                if let Some((count, _)) = read_uleb128(section_bytes, 0) {
                    profile.data_segment_count = count as usize;
                }
            }
            SECTION_CUSTOM => {
                profile.custom_section_bytes += section_bytes.len();
                // Check for name/debug custom sections
                if section_bytes.starts_with(b"\x04name") || section_bytes.starts_with(b"name") {
                    profile.has_name_section = true;
                }
                if section_bytes.windows(5).any(|w| w == b"debug") {
                    profile.has_debug_section = true;
                }
            }
            _ => {}
        }

        pos = section_end;
    }

    profile
}

// ── Optimization suggestion engine ───────────────────────────────────────────

/// Generate gas findings from WASM bytes and section profile.
pub fn generate_findings(bytes: &[u8], profile: &WasmSectionProfile) -> Vec<GasFinding> {
    let mut findings: Vec<GasFinding> = Vec::new();
    let size_kb = bytes.len() as f64 / 1024.0;
    let size_limit_pct = bytes.len() as f64 / WASM_SIZE_LIMIT as f64 * 100.0;

    // GAS-001: Critical size — approaching or exceeding Soroban limit
    if bytes.len() >= WASM_SIZE_LIMIT {
        findings.push(GasFinding {
            id: "GAS-001".into(),
            kind: "binary-size-critical".into(),
            severity: FindingSeverity::Critical,
            description: format!(
                "WASM is {:.1} KB — at or above the Soroban 128 KB upload limit.",
                size_kb
            ),
            recommendation:
                "Enable `opt-level = 'z'`, `lto = true`, `strip = true` in [profile.release]. \
                 Remove unused dependencies and feature flags."
                    .into(),
            estimated_saving_pct: 25.0,
            estimated_gas_saving: (bytes.len() as u64).saturating_mul(FEE_PER_CODE_BYTE) / 4,
        });
    } else if bytes.len() > 90 * 1024 {
        findings.push(GasFinding {
            id: "GAS-001".into(),
            kind: "binary-size-high".into(),
            severity: FindingSeverity::High,
            description: format!(
                "WASM is {:.1} KB ({:.0}% of 128 KB limit) — high risk of hitting the limit.",
                size_kb, size_limit_pct
            ),
            recommendation:
                "Apply `opt-level = 'z'` and `lto = true`. Use `wasm-opt -Oz` post-build.".into(),
            estimated_saving_pct: 20.0,
            estimated_gas_saving: (bytes.len() as u64).saturating_mul(FEE_PER_CODE_BYTE) / 5,
        });
    } else if bytes.len() > 64 * 1024 {
        findings.push(GasFinding {
            id: "GAS-002".into(),
            kind: "binary-size-medium".into(),
            severity: FindingSeverity::Medium,
            description: format!("WASM is {:.1} KB — moderately large.", size_kb),
            recommendation: "Use `opt-level = 's'` and `lto = true`. Strip unused symbols.".into(),
            estimated_saving_pct: 10.0,
            estimated_gas_saving: (bytes.len() as u64).saturating_mul(FEE_PER_CODE_BYTE) / 10,
        });
    }

    // GAS-003: Debug symbols
    if profile.has_debug_section || profile.has_name_section {
        let custom_kb = profile.custom_section_bytes as f64 / 1024.0;
        findings.push(GasFinding {
            id: "GAS-003".into(),
            kind: "debug-info".into(),
            severity: FindingSeverity::High,
            description: format!(
                "Debug/name sections detected ({:.1} KB of custom sections). \
                 These add cost without benefit at runtime.",
                custom_kb
            ),
            recommendation:
                "Build with `cargo build --release` and add `strip = true` under [profile.release]."
                    .into(),
            estimated_saving_pct: 15.0,
            estimated_gas_saving: (profile.custom_section_bytes as u64)
                .saturating_mul(FEE_PER_CODE_BYTE),
        });
    }

    // GAS-004: Panic strings
    let panic_count = bytes.windows(5).filter(|w| w == b"panic").count();
    if panic_count > 0 {
        findings.push(GasFinding {
            id: "GAS-004".into(),
            kind: "panic-strings".into(),
            severity: FindingSeverity::Medium,
            description: format!(
                "{} panic string(s) embedded — verbose panic messages inflate binary size.",
                panic_count
            ),
            recommendation:
                "Set `panic = \"abort\"` in Cargo.toml and replace `expect(\"long message\")` \
                 with short codes or `soroban_sdk::panic_with_error!`."
                    .into(),
            estimated_saving_pct: 8.0,
            estimated_gas_saving: (panic_count as u64) * 500,
        });
    }

    // GAS-005: println/eprintln — debug logging
    let print_count = bytes.windows(7).filter(|w| w == b"println").count()
        + bytes.windows(7).filter(|w| w == b"eprintl").count();
    if print_count > 0 {
        findings.push(GasFinding {
            id: "GAS-005".into(),
            kind: "debug-logging".into(),
            severity: FindingSeverity::Medium,
            description: format!(
                "{} debug print statement(s) detected. These are no-ops on Soroban \
                 but still bloat the binary.",
                print_count
            ),
            recommendation:
                "Remove all `println!` / `eprintln!` / `log::` calls from contract code.".into(),
            estimated_saving_pct: 5.0,
            estimated_gas_saving: (print_count as u64) * 300,
        });
    }

    // GAS-006: Excessive imports
    if profile.import_count > 20 {
        findings.push(GasFinding {
            id: "GAS-006".into(),
            kind: "excessive-imports".into(),
            severity: FindingSeverity::Medium,
            description: format!(
                "{} imported functions — each adds ~{} gas units at upload.",
                profile.import_count, FEE_PER_IMPORT
            ),
            recommendation: "Audit SDK imports; use only required host functions. \
                 Enable `default-features = false` on soroban-sdk."
                .into(),
            estimated_saving_pct: 5.0,
            estimated_gas_saving: ((profile.import_count.saturating_sub(10)) as u64)
                .saturating_mul(FEE_PER_IMPORT),
        });
    }

    // GAS-007: Excessive exports
    if profile.export_count > 30 {
        findings.push(GasFinding {
            id: "GAS-007".into(),
            kind: "excessive-exports".into(),
            severity: FindingSeverity::Low,
            description: format!(
                "{} exported functions. Each export adds overhead.",
                profile.export_count
            ),
            recommendation:
                "Only export functions called by external clients. Mark internal helpers with `pub(crate)` or remove the `#[contractimpl]` macro from test helpers."
                    .into(),
            estimated_saving_pct: 3.0,
            estimated_gas_saving: ((profile.export_count.saturating_sub(15)) as u64)
                .saturating_mul(FEE_PER_EXPORT),
        });
    }

    // GAS-008: Multiple memory sections
    if profile.memory_count > 1 {
        findings.push(GasFinding {
            id: "GAS-008".into(),
            kind: "multiple-memories".into(),
            severity: FindingSeverity::High,
            description: format!(
                "{} memory sections — Soroban only supports one linear memory.",
                profile.memory_count
            ),
            recommendation:
                "Ensure only one linear memory is declared. Multiple memories may prevent deployment."
                    .into(),
            estimated_saving_pct: 0.0,
            estimated_gas_saving: 0,
        });
    }

    // GAS-009: Start function
    if profile.has_start_function {
        findings.push(GasFinding {
            id: "GAS-009".into(),
            kind: "start-function".into(),
            severity: FindingSeverity::Medium,
            description:
                "WASM start function detected. Soroban executes this on every invocation, \
                 adding gas overhead."
                    .into(),
            recommendation:
                "Avoid the WASM start function. Initialization logic should be in an explicit \
                 init contract function called once by the deployer."
                    .into(),
            estimated_saving_pct: 5.0,
            estimated_gas_saving: 2_000,
        });
    }

    // GAS-010: Many globals
    if profile.global_count > 15 {
        findings.push(GasFinding {
            id: "GAS-010".into(),
            kind: "excessive-globals".into(),
            severity: FindingSeverity::Low,
            description: format!(
                "{} global variables declared — each costs {} gas units.",
                profile.global_count, FEE_PER_GLOBAL
            ),
            recommendation:
                "Minimize mutable globals; use contract storage for persistent state instead."
                    .into(),
            estimated_saving_pct: 2.0,
            estimated_gas_saving: ((profile.global_count.saturating_sub(5)) as u64)
                .saturating_mul(FEE_PER_GLOBAL),
        });
    }

    // GAS-011: Many data segments
    if profile.data_segment_count > 10 {
        findings.push(GasFinding {
            id: "GAS-011".into(),
            kind: "data-segments".into(),
            severity: FindingSeverity::Low,
            description: format!(
                "{} data segments — merge static data to reduce segment overhead.",
                profile.data_segment_count
            ),
            recommendation:
                "Consolidate static data into fewer segments using the `data-layout` linker option \
                 or by restructuring constants."
                    .into(),
            estimated_saving_pct: 2.0,
            estimated_gas_saving: ((profile.data_segment_count.saturating_sub(3)) as u64)
                .saturating_mul(FEE_PER_DATA_SEGMENT),
        });
    }

    // GAS-012: High instruction density
    let instr_per_byte = if !bytes.is_empty() {
        profile.estimated_instruction_count as f64 / bytes.len() as f64
    } else {
        0.0
    };
    if instr_per_byte > 0.6 && profile.estimated_instruction_count > 5_000 {
        findings.push(GasFinding {
            id: "GAS-012".into(),
            kind: "high-instruction-density".into(),
            severity: FindingSeverity::Info,
            description: format!(
                "High instruction density ({:.0} instructions, {:.2} instr/byte). \
                 Execution CPU cost will be significant.",
                profile.estimated_instruction_count, instr_per_byte
            ),
            recommendation:
                "Profile contract functions and reduce nested loops. Consider off-chain computation \
                 where possible."
                    .into(),
            estimated_saving_pct: 10.0,
            estimated_gas_saving: (profile.estimated_instruction_count as u64 / 5)
                .saturating_mul(CPU_PER_INSTRUCTION),
        });
    }

    // Sort findings: critical → high → medium → low → info
    findings.sort_by_key(|f| match f.severity {
        FindingSeverity::Critical => 0,
        FindingSeverity::High => 1,
        FindingSeverity::Medium => 2,
        FindingSeverity::Low => 3,
        FindingSeverity::Info => 4,
    });

    findings
}

// ── Scoring ───────────────────────────────────────────────────────────────────

/// Compute an overall optimization score (0–100, higher = more optimized).
pub fn compute_optimization_score(findings: &[GasFinding]) -> u8 {
    let mut penalty: i32 = 0;
    for f in findings {
        penalty += match f.severity {
            FindingSeverity::Critical => 30,
            FindingSeverity::High => 20,
            FindingSeverity::Medium => 10,
            FindingSeverity::Low => 4,
            FindingSeverity::Info => 1,
        };
    }
    (100i32 - penalty).max(0) as u8
}

/// Build a prioritized list of top recommendations from findings.
fn top_recommendations(findings: &[GasFinding]) -> Vec<String> {
    findings
        .iter()
        .filter(|f| {
            matches!(
                f.severity,
                FindingSeverity::Critical | FindingSeverity::High | FindingSeverity::Medium
            )
        })
        .take(5)
        .map(|f| format!("[{}] {}", f.id, f.recommendation.clone()))
        .collect()
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Perform a full gas analysis on a WASM file.
pub fn analyze_wasm_file(path: &Path, label: Option<&str>) -> Result<GasAnalysisReport> {
    let bytes =
        fs::read(path).with_context(|| format!("Failed to read WASM file: {}", path.display()))?;

    if !is_valid_wasm(&bytes) {
        anyhow::bail!(
            "Not a valid WASM binary (magic bytes mismatch): {}",
            path.display()
        );
    }

    let sha256 = hex::encode(Sha256::digest(&bytes));
    let contract_label = label.map(str::to_string).unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    let profile = parse_wasm_sections(&bytes);
    let gas_cost = GasCostBreakdown::compute(&profile, bytes.len());
    let findings = generate_findings(&bytes, &profile);
    let score = compute_optimization_score(&findings);
    let recs = top_recommendations(&findings);
    let size_limit_pct = bytes.len() as f64 / WASM_SIZE_LIMIT as f64 * 100.0;

    Ok(GasAnalysisReport {
        id: format!("gas-{}", &sha256[..12]),
        generated_at: Utc::now().to_rfc3339(),
        contract_label,
        wasm_path: path.display().to_string(),
        wasm_sha256: sha256,
        size_bytes: bytes.len(),
        size_limit_pct,
        section_profile: profile,
        gas_cost,
        optimization_score: score,
        findings,
        top_recommendations: recs,
    })
}

/// Compare two WASM versions and produce a diff report.
pub fn compare_versions(baseline: &Path, candidate: &Path) -> Result<GasVersionComparison> {
    let base_report = analyze_wasm_file(baseline, Some("baseline"))?;
    let cand_report = analyze_wasm_file(candidate, Some("candidate"))?;

    let size_delta = cand_report.size_bytes as i64 - base_report.size_bytes as i64;
    let size_delta_pct = if base_report.size_bytes > 0 {
        size_delta as f64 / base_report.size_bytes as f64 * 100.0
    } else {
        0.0
    };
    let gas_delta = cand_report.gas_cost.total as i64 - base_report.gas_cost.total as i64;
    let gas_delta_pct = if base_report.gas_cost.total > 0 {
        gas_delta as f64 / base_report.gas_cost.total as f64 * 100.0
    } else {
        0.0
    };
    let score_delta = cand_report.optimization_score as i8 - base_report.optimization_score as i8;
    let instr_delta = cand_report.section_profile.estimated_instruction_count as i64
        - base_report.section_profile.estimated_instruction_count as i64;
    let instr_delta_pct = if base_report.section_profile.estimated_instruction_count > 0 {
        instr_delta as f64 / base_report.section_profile.estimated_instruction_count as f64 * 100.0
    } else {
        0.0
    };

    // Identify new findings in candidate that weren't in baseline
    let base_finding_ids: std::collections::HashSet<&str> =
        base_report.findings.iter().map(|f| f.id.as_str()).collect();
    let new_findings: Vec<GasFinding> = cand_report
        .findings
        .iter()
        .filter(|f| !base_finding_ids.contains(f.id.as_str()))
        .cloned()
        .collect();

    let cand_finding_ids: std::collections::HashSet<&str> =
        cand_report.findings.iter().map(|f| f.id.as_str()).collect();
    let resolved_findings = base_report
        .findings
        .iter()
        .filter(|f| !cand_finding_ids.contains(f.id.as_str()))
        .count();

    let verdict = if gas_delta < 0 {
        format!(
            "Improved — candidate saves ~{} gas ({:.1}% reduction)",
            gas_delta.abs(),
            gas_delta_pct.abs()
        )
    } else if gas_delta > 0 {
        format!(
            "Regressed — candidate costs ~{} more gas ({:.1}% increase)",
            gas_delta, gas_delta_pct
        )
    } else {
        "No change in estimated gas cost".to_string()
    };

    let id_combined = format!(
        "cmp-{}-{}",
        &base_report.wasm_sha256[..8],
        &cand_report.wasm_sha256[..8]
    );

    Ok(GasVersionComparison {
        id: id_combined,
        generated_at: Utc::now().to_rfc3339(),
        baseline_path: baseline.display().to_string(),
        candidate_path: candidate.display().to_string(),
        baseline_sha256: base_report.wasm_sha256,
        candidate_sha256: cand_report.wasm_sha256,
        baseline_size_bytes: base_report.size_bytes,
        candidate_size_bytes: cand_report.size_bytes,
        size_delta_bytes: size_delta,
        size_delta_pct,
        baseline_gas_cost: base_report.gas_cost,
        candidate_gas_cost: cand_report.gas_cost,
        gas_delta,
        gas_delta_pct,
        baseline_score: base_report.optimization_score,
        candidate_score: cand_report.optimization_score,
        score_delta,
        baseline_instruction_count: base_report.section_profile.estimated_instruction_count,
        candidate_instruction_count: cand_report.section_profile.estimated_instruction_count,
        instruction_delta: instr_delta,
        instruction_delta_pct: instr_delta_pct,
        verdict,
        new_findings,
        resolved_findings,
    })
}

// ── Persistence ───────────────────────────────────────────────────────────────

fn gas_reports_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("gas_reports");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

/// Save a gas analysis report to disk.
pub fn save_report(report: &GasAnalysisReport) -> Result<PathBuf> {
    let dir = gas_reports_dir()?;
    let path = dir.join(format!("{}.json", report.id));
    fs::write(&path, serde_json::to_string_pretty(report)?)?;
    Ok(path)
}

/// Load a single report by ID prefix.
pub fn load_report(id: &str) -> Result<GasAnalysisReport> {
    let reports = list_reports()?;
    reports
        .into_iter()
        .find(|r| r.id == id || r.id.starts_with(id))
        .ok_or_else(|| anyhow::anyhow!("No gas report found with ID prefix '{}'", id))
}

/// List all saved gas analysis reports, newest first.
pub fn list_reports() -> Result<Vec<GasAnalysisReport>> {
    let dir = gas_reports_dir()?;
    let mut reports = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
            if let Ok(raw) = fs::read_to_string(entry.path()) {
                if let Ok(r) = serde_json::from_str::<GasAnalysisReport>(&raw) {
                    reports.push(r);
                }
            }
        }
    }
    reports.sort_by(|a, b| b.generated_at.cmp(&a.generated_at));
    Ok(reports)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_wasm() -> Vec<u8> {
        // Valid WASM header: magic + version
        vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
    }

    fn wasm_with_size(extra_kb: usize) -> Vec<u8> {
        let mut bytes = minimal_wasm();
        // Append a custom section that pads to the desired size
        let padding = vec![0x42u8; extra_kb * 1024];
        // section id = 0 (custom), then LEB128 length, then data
        bytes.push(SECTION_CUSTOM);
        let len = padding.len() as u64;
        // Encode LEB128 length
        let mut leb = Vec::new();
        let mut v = len;
        loop {
            let mut b = (v & 0x7F) as u8;
            v >>= 7;
            if v != 0 {
                b |= 0x80;
            }
            leb.push(b);
            if v == 0 {
                break;
            }
        }
        bytes.extend_from_slice(&leb);
        bytes.extend_from_slice(&padding);
        bytes
    }

    #[test]
    fn is_valid_wasm_accepts_correct_header() {
        assert!(is_valid_wasm(&minimal_wasm()));
    }

    #[test]
    fn is_valid_wasm_rejects_garbage() {
        assert!(!is_valid_wasm(b"not wasm"));
    }

    #[test]
    fn parse_wasm_sections_returns_defaults_for_minimal() {
        let profile = parse_wasm_sections(&minimal_wasm());
        assert_eq!(profile.import_count, 0);
        assert_eq!(profile.export_count, 0);
        assert!(!profile.has_start_function);
    }

    #[test]
    fn generate_findings_no_issues_for_small_wasm() {
        let bytes = minimal_wasm();
        let profile = parse_wasm_sections(&bytes);
        let findings = generate_findings(&bytes, &profile);
        let critical: Vec<_> = findings
            .iter()
            .filter(|f| f.severity == FindingSeverity::Critical)
            .collect();
        assert!(
            critical.is_empty(),
            "minimal WASM should have no critical findings"
        );
    }

    #[test]
    fn generate_findings_critical_for_oversized_wasm() {
        let bytes = wasm_with_size(130); // 130 KB > 128 KB limit
        let profile = parse_wasm_sections(&bytes);
        let findings = generate_findings(&bytes, &profile);
        assert!(
            findings
                .iter()
                .any(|f| f.severity == FindingSeverity::Critical),
            "oversized WASM should have a critical finding"
        );
    }

    #[test]
    fn compute_optimization_score_perfect_with_no_findings() {
        assert_eq!(compute_optimization_score(&[]), 100);
    }

    #[test]
    fn compute_optimization_score_decreases_with_severity() {
        let critical = vec![GasFinding {
            id: "GAS-001".into(),
            kind: "test".into(),
            severity: FindingSeverity::Critical,
            description: "test".into(),
            recommendation: "test".into(),
            estimated_saving_pct: 0.0,
            estimated_gas_saving: 0,
        }];
        let score = compute_optimization_score(&critical);
        assert_eq!(score, 70); // 100 - 30
    }

    #[test]
    fn compute_optimization_score_clamps_at_zero() {
        let many: Vec<GasFinding> = (0..10)
            .map(|i| GasFinding {
                id: format!("GAS-{:03}", i),
                kind: "test".into(),
                severity: FindingSeverity::Critical,
                description: "test".into(),
                recommendation: "fix".into(),
                estimated_saving_pct: 10.0,
                estimated_gas_saving: 100,
            })
            .collect();
        assert_eq!(compute_optimization_score(&many), 0);
    }

    #[test]
    fn gas_cost_breakdown_total_is_sum_of_parts() {
        let profile = WasmSectionProfile {
            import_count: 5,
            export_count: 3,
            global_count: 2,
            data_segment_count: 1,
            estimated_instruction_count: 100,
            ..Default::default()
        };
        let cost = GasCostBreakdown::compute(&profile, 1024);
        assert_eq!(
            cost.total,
            cost.upload_cost
                + cost.cpu_cost
                + cost.import_cost
                + cost.export_cost
                + cost.global_cost
                + cost.data_cost
        );
    }

    #[test]
    fn gas_analysis_report_counts() {
        let findings = vec![
            GasFinding {
                id: "GAS-001".into(),
                kind: "a".into(),
                severity: FindingSeverity::Critical,
                description: String::new(),
                recommendation: String::new(),
                estimated_saving_pct: 0.0,
                estimated_gas_saving: 500,
            },
            GasFinding {
                id: "GAS-002".into(),
                kind: "b".into(),
                severity: FindingSeverity::High,
                description: String::new(),
                recommendation: String::new(),
                estimated_saving_pct: 0.0,
                estimated_gas_saving: 300,
            },
        ];
        let report = GasAnalysisReport {
            id: "gas-test".into(),
            generated_at: String::new(),
            contract_label: "test".into(),
            wasm_path: String::new(),
            wasm_sha256: String::new(),
            size_bytes: 1024,
            size_limit_pct: 1.0,
            section_profile: WasmSectionProfile::default(),
            gas_cost: GasCostBreakdown::default(),
            optimization_score: 70,
            findings,
            top_recommendations: vec![],
        };
        assert_eq!(report.critical_count(), 1);
        assert_eq!(report.high_count(), 1);
        assert_eq!(report.total_estimated_gas_saving(), 800);
    }

    #[test]
    fn finding_severity_display() {
        assert_eq!(FindingSeverity::Critical.to_string(), "critical");
        assert_eq!(FindingSeverity::High.to_string(), "high");
        assert_eq!(FindingSeverity::Info.to_string(), "info");
    }
}
