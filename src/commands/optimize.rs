use crate::utils::{ai_telemetry, config, print as p};
use anyhow::Result;
use chrono::Utc;
use clap::{Args, Subcommand};
use colored::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum OptimizeCommands {
    /// Analyse a compiled WASM binary for performance issues
    Analyse(AnalyseArgs),
    /// Apply automatic code transformation hints to a Rust contract source file
    Transform(TransformArgs),
    /// Benchmark and compare two WASM binaries
    Bench(BenchArgs),
    /// Show the last optimization report for a contract
    Report(ReportArgs),
    /// List all stored optimization reports
    Reports(ReportsArgs),
}

#[derive(Args)]
pub struct AnalyseArgs {
    /// Path to the compiled WASM file
    #[arg(long)]
    pub wasm: PathBuf,
    /// Contract label (for report storage)
    #[arg(long)]
    pub contract: String,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
    /// Fail with exit code 1 if critical issues are found
    #[arg(long, default_value = "false")]
    pub fail_on_critical: bool,
}

#[derive(Args)]
pub struct TransformArgs {
    /// Path to the Rust contract source file to analyse
    #[arg(long)]
    pub src: PathBuf,
    /// Output file path (default: overwrite source)
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Dry-run: print suggested changes but do not write them
    #[arg(long, default_value = "false")]
    pub dry_run: bool,
    /// Skip the interactive confirmation prompt before applying changes
    #[arg(long, short = 'y', default_value = "false")]
    pub yes: bool,
    /// Also request an AI-generated optimization opinion (requires STARFORGE_AI_API_KEY)
    #[arg(long, default_value = "false")]
    pub ai: bool,
}

#[derive(Args)]
pub struct BenchArgs {
    /// Path to the baseline WASM
    #[arg(long)]
    pub baseline: PathBuf,
    /// Path to the optimized WASM to compare against baseline
    #[arg(long)]
    pub optimized: PathBuf,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ReportArgs {
    /// Contract label
    #[arg(long)]
    pub contract: String,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ReportsArgs {
    /// Filter by contract label
    #[arg(long)]
    pub contract: Option<String>,
}

// ── Data structures ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum IssueSeverity {
    Critical,
    Warning,
    Info,
}

impl std::fmt::Display for IssueSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IssueSeverity::Critical => write!(f, "critical"),
            IssueSeverity::Warning => write!(f, "warning"),
            IssueSeverity::Info => write!(f, "info"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationIssue {
    pub id: String,
    pub kind: String,
    pub severity: IssueSeverity,
    pub description: String,
    pub recommendation: String,
    pub estimated_saving_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationReport {
    pub id: String,
    pub contract: String,
    pub wasm_hash: String,
    pub wasm_size_bytes: usize,
    pub timestamp: String,
    pub total_issues: usize,
    pub critical: usize,
    pub warnings: usize,
    pub infos: usize,
    pub overall_score: u8,
    pub issues: Vec<OptimizationIssue>,
}

impl OptimizationReport {
    pub fn has_critical(&self) -> bool {
        self.critical > 0
    }
}

/// The kind of optimization a [`TransformSuggestion`] represents (issue #497).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransformCategory {
    StoragePacking,
    FunctionInlining,
    LoopOptimization,
    ConstantFolding,
    DeadCodeElimination,
    RedundantCode,
}

impl std::fmt::Display for TransformCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TransformCategory::StoragePacking => "storage-packing",
            TransformCategory::FunctionInlining => "function-inlining",
            TransformCategory::LoopOptimization => "loop-optimization",
            TransformCategory::ConstantFolding => "constant-folding",
            TransformCategory::DeadCodeElimination => "dead-code-elimination",
            TransformCategory::RedundantCode => "redundant-code",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformSuggestion {
    pub file: String,
    pub line: usize,
    pub category: TransformCategory,
    pub original: String,
    pub suggested: String,
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BenchmarkComparison {
    pub baseline_hash: String,
    pub optimized_hash: String,
    pub baseline_size_bytes: usize,
    pub optimized_size_bytes: usize,
    pub size_delta_bytes: i64,
    pub size_reduction_pct: f64,
    pub baseline_instruction_count: usize,
    pub optimized_instruction_count: usize,
    pub instruction_delta: i64,
    pub instruction_reduction_pct: f64,
    pub timestamp: String,
}

// ── Storage helpers ───────────────────────────────────────────────────────────

fn optimize_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("optimize");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn reports_path() -> Result<PathBuf> {
    Ok(optimize_dir()?.join("reports.json"))
}

fn load_reports_store() -> Result<Vec<OptimizationReport>> {
    let path = reports_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

fn save_reports_store(reports: &[OptimizationReport]) -> Result<()> {
    fs::write(reports_path()?, serde_json::to_string_pretty(reports)?)?;
    Ok(())
}

// ── Analysis engine ───────────────────────────────────────────────────────────

fn wasm_hash_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Count approximate WASM instruction opcodes (every byte is not an instruction,
/// but this gives a rough relative comparison between binaries).
fn estimate_instruction_count(bytes: &[u8]) -> usize {
    // Count opcodes that are common Wasm instructions (0x00–0xBF range after header)
    if bytes.len() <= 8 {
        return 0;
    }
    bytes[8..].iter().filter(|&&b| b <= 0xBF).count()
}

/// Static WASM analysis — returns a list of performance issues.
pub fn analyse_wasm(bytes: &[u8]) -> Vec<OptimizationIssue> {
    let mut issues = Vec::new();
    let size_kb = bytes.len() as f64 / 1024.0;

    // Size checks
    if size_kb > 100.0 {
        issues.push(OptimizationIssue {
            id: "OPT-001".to_string(),
            kind: "binary-size".to_string(),
            severity: IssueSeverity::Critical,
            description: format!(
                "WASM binary is {:.1} KB, approaching the Soroban 128 KB limit.",
                size_kb
            ),
            recommendation:
                "Enable wasm-opt passes, use `opt-level = 'z'` in release profile, and remove unused dependencies."
                    .to_string(),
            estimated_saving_pct: Some(20.0),
        });
    } else if size_kb > 64.0 {
        issues.push(OptimizationIssue {
            id: "OPT-002".to_string(),
            kind: "binary-size".to_string(),
            severity: IssueSeverity::Warning,
            description: format!("WASM binary is {:.1} KB — moderately large.", size_kb),
            recommendation: "Consider `opt-level = 's'` and `lto = true` in release profile."
                .to_string(),
            estimated_saving_pct: Some(10.0),
        });
    }

    // Check for debug symbols (they add bloat without adding functionality)
    let has_debug_name = bytes.windows(5).any(|w| w == b".name" || w == b"debug");
    if has_debug_name {
        issues.push(OptimizationIssue {
            id: "OPT-003".to_string(),
            kind: "debug-info".to_string(),
            severity: IssueSeverity::Warning,
            description: "Debug symbols detected in WASM binary.".to_string(),
            recommendation:
                "Build with `cargo build --release` and add `strip = true` to Cargo.toml [profile.release]."
                    .to_string(),
            estimated_saving_pct: Some(15.0),
        });
    }

    // Check for data section patterns that may indicate large static strings
    let large_data_threshold = 512usize;
    let data_runs = bytes
        .windows(large_data_threshold)
        .filter(|w| w.iter().all(|&b| (0x20..=0x7E).contains(&b)))
        .count();
    if data_runs > 0 {
        issues.push(OptimizationIssue {
            id: "OPT-004".to_string(),
            kind: "large-static-strings".to_string(),
            severity: IssueSeverity::Info,
            description: format!(
                "{} large printable data block(s) detected — may be long error strings.",
                data_runs
            ),
            recommendation:
                "Use short symbolic error codes instead of long string messages in contract errors."
                    .to_string(),
            estimated_saving_pct: Some(5.0),
        });
    }

    // Check for unreachable opcode (0x00 in a context that may be wasted code)
    let unreachable_count = bytes.iter().filter(|&&b| b == 0x00).count();
    if unreachable_count > 50 {
        issues.push(OptimizationIssue {
            id: "OPT-005".to_string(),
            kind: "unreachable-code".to_string(),
            severity: IssueSeverity::Info,
            description: format!(
                "High null/unreachable byte density ({} occurrences) — dead code may be present.",
                unreachable_count
            ),
            recommendation: "Run `wasm-opt -Oz` to strip dead code.".to_string(),
            estimated_saving_pct: Some(3.0),
        });
    }

    // LTO check: if binary size is above 20 KB and no optimizations evident, suggest LTO
    if size_kb > 20.0 && !has_debug_name {
        issues.push(OptimizationIssue {
            id: "OPT-006".to_string(),
            kind: "lto-suggestion".to_string(),
            severity: IssueSeverity::Info,
            description: "Consider enabling Link-Time Optimization for further size reduction."
                .to_string(),
            recommendation:
                "Add `lto = true` and `codegen-units = 1` to [profile.release] in Cargo.toml."
                    .to_string(),
            estimated_saving_pct: Some(8.0),
        });
    }

    issues
}

/// Compute an overall optimization score (0–100, higher is better).
pub fn compute_score(issues: &[OptimizationIssue]) -> u8 {
    let mut penalty: i32 = 0;
    for issue in issues {
        penalty += match issue.severity {
            IssueSeverity::Critical => 25,
            IssueSeverity::Warning => 10,
            IssueSeverity::Info => 3,
        };
    }
    (100i32 - penalty).max(0) as u8
}

/// Perform static source-code transformation suggestions on a Rust file.
///
/// Combines line-level heuristics (redundant `.clone()`, `.unwrap()`, long
/// string literals) with the dedicated optimization-type detectors below:
/// storage packing, function inlining, loop optimization, constant folding,
/// and dead-code elimination (issue #497).
pub fn analyse_source(content: &str, file: &str) -> Vec<TransformSuggestion> {
    let mut suggestions = Vec::new();

    for (line_idx, line) in content.lines().enumerate() {
        let line_no = line_idx + 1;
        let trimmed = line.trim();

        // Suggest replacing .clone() on primitives
        if trimmed.contains(".clone()")
            && (trimmed.contains("u64")
                || trimmed.contains("i64")
                || trimmed.contains("u32")
                || trimmed.contains("bool"))
        {
            suggestions.push(TransformSuggestion {
                file: file.to_string(),
                line: line_no,
                category: TransformCategory::RedundantCode,
                original: line.to_string(),
                suggested: line.replace(".clone()", " /* .clone() not needed for Copy types */").to_string(),
                reason: "Copy types (u64, i64, u32, bool) don't need .clone() — remove it to avoid unnecessary overhead.".to_string(),
            });
        }

        // Suggest soroban_sdk::Vec instead of std::vec::Vec
        if trimmed.contains("Vec<")
            && !trimmed.starts_with("//")
            && (trimmed.contains("std::vec")
                || (trimmed.contains("Vec<") && trimmed.contains("use std")))
        {
            suggestions.push(TransformSuggestion {
                    file: file.to_string(),
                    line: line_no,
                    category: TransformCategory::RedundantCode,
                    original: line.to_string(),
                    suggested: line.replace("std::vec::Vec", "soroban_sdk::Vec").to_string(),
                    reason: "Prefer soroban_sdk::Vec over std::vec::Vec in contract code for Soroban compatibility.".to_string(),
                });
        }

        // Flag large string literals in contract code
        if trimmed.contains('"') && !trimmed.starts_with("//") {
            let string_len: usize = trimmed
                .split('"')
                .enumerate()
                .filter(|(i, _)| i % 2 == 1)
                .map(|(_, s)| s.len())
                .sum();
            if string_len > 80 {
                suggestions.push(TransformSuggestion {
                    file: file.to_string(),
                    line: line_no,
                    category: TransformCategory::RedundantCode,
                    original: line.to_string(),
                    suggested: format!(
                        "{} // TODO: replace long string with short error symbol",
                        line
                    ),
                    reason: format!(
                        "Long string literal ({} chars) increases WASM binary size. Use soroban_sdk::symbol_short!() or short codes.",
                        string_len
                    ),
                });
            }
        }

        // Suggest avoiding unwrap() in hot paths
        if trimmed.contains(".unwrap()") && !trimmed.starts_with("//") {
            suggestions.push(TransformSuggestion {
                file: file.to_string(),
                line: line_no,
                category: TransformCategory::RedundantCode,
                original: line.to_string(),
                suggested: line.replace(".unwrap()", ".expect(\"[reason]\") /* or handle error */").to_string(),
                reason: "Prefer explicit error handling over .unwrap() — panics in contracts abort the entire transaction and waste fees.".to_string(),
            });
        }
    }

    suggestions.extend(detect_constant_folding(content, file));
    suggestions.extend(detect_dead_code(content, file));
    suggestions.extend(detect_storage_packing(content, file));
    suggestions.extend(detect_function_inlining(content, file));
    suggestions.extend(detect_loop_optimization(content, file));

    suggestions
}

/// Detects arithmetic between two integer literals that could be folded to a
/// single constant at compile time (e.g. `60 * 60 * 24` → `86400`).
fn detect_constant_folding(content: &str, file: &str) -> Vec<TransformSuggestion> {
    let mut suggestions = Vec::new();
    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") || trimmed.starts_with("const") {
            continue;
        }
        for op in ['+', '*', '-'] {
            let pattern = format!(" {} ", op);
            if let Some(pos) = trimmed.find(&pattern) {
                let left = trimmed[..pos].trim();
                let right = trimmed[pos + pattern.len()..].trim();
                let left_num = left.rsplit(|c: char| !c.is_ascii_digit()).next();
                let right_num = right
                    .split(|c: char| !c.is_ascii_digit() && c != '.')
                    .next();
                if let (Some(l), Some(r)) = (left_num, right_num) {
                    if !l.is_empty()
                        && !r.is_empty()
                        && l.chars().all(|c| c.is_ascii_digit())
                        && r.chars().all(|c| c.is_ascii_digit())
                    {
                        if let (Ok(lv), Ok(rv)) = (l.parse::<i128>(), r.parse::<i128>()) {
                            let folded = match op {
                                '+' => lv.checked_add(rv),
                                '*' => lv.checked_mul(rv),
                                '-' => lv.checked_sub(rv),
                                _ => None,
                            };
                            if let Some(folded) = folded {
                                suggestions.push(TransformSuggestion {
                                    file: file.to_string(),
                                    line: line_idx + 1,
                                    category: TransformCategory::ConstantFolding,
                                    original: line.to_string(),
                                    suggested: line.replacen(
                                        &format!("{} {} {}", l, op, r),
                                        &folded.to_string(),
                                        1,
                                    ),
                                    reason: format!(
                                        "Literal expression `{} {} {}` can be folded to the constant `{}` at compile time, saving a runtime computation.",
                                        l, op, r, folded
                                    ),
                                });
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    suggestions
}

/// Detects private helper functions that are defined but never called
/// elsewhere in the same file — a conservative, single-file heuristic for
/// dead-code elimination (whole-crate reachability is out of scope here).
/// Trivial one-line functions are skipped to avoid flagging ordinary small
/// utility functions that may legitimately be part of a public API surface
/// exercised from outside this file.
fn detect_dead_code(content: &str, file: &str) -> Vec<TransformSuggestion> {
    let mut suggestions = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            continue;
        }
        let prev = if line_idx > 0 {
            lines[line_idx - 1].trim()
        } else {
            ""
        };
        if prev.starts_with('#') {
            // Skip functions annotated with #[test], #[allow(...)], etc.
            continue;
        }
        let name = trimmed
            .trim_start_matches("fn ")
            .split(|c: char| c == '(' || c == '<' || c.is_whitespace())
            .next()
            .unwrap_or("");
        if name.is_empty() {
            continue;
        }

        // Count non-empty body lines to skip trivial one-liners.
        let mut depth = trimmed.matches('{').count() as i64 - trimmed.matches('}').count() as i64;
        let mut j = line_idx + 1;
        let mut body_lines = 0usize;
        while j < lines.len() && depth > 0 {
            depth += lines[j].matches('{').count() as i64 - lines[j].matches('}').count() as i64;
            let t = lines[j].trim();
            if !t.is_empty() && t != "}" {
                body_lines += 1;
            }
            j += 1;
        }
        if body_lines <= 1 {
            continue;
        }

        let call_pattern = format!("{}(", name);
        let call_sites = content.matches(&call_pattern).count();
        // One occurrence is the definition itself; zero additional call sites
        // means the function is never invoked in this file.
        if call_sites <= 1 {
            suggestions.push(TransformSuggestion {
                file: file.to_string(),
                line: line_idx + 1,
                category: TransformCategory::DeadCodeElimination,
                original: line.to_string(),
                suggested: format!("// consider removing unused function `{}`", name),
                reason: format!(
                    "Private function `{}` has no call sites elsewhere in this file. If it isn't used from another module, removing it reduces WASM binary size.",
                    name
                ),
            });
        }
    }
    suggestions
}

/// Rough byte size for common Rust/Soroban field types, used to detect
/// suboptimal struct field ordering (storage packing / padding waste).
fn type_size_hint(ty: &str) -> Option<usize> {
    let ty = ty.trim();
    Some(match ty {
        "bool" | "u8" | "i8" => 1,
        "u16" | "i16" => 2,
        "u32" | "i32" | "f32" => 4,
        "u64" | "i64" | "f64" => 8,
        "u128" | "i128" => 16,
        "Address" | "BytesN<32>" => 32,
        _ => return None,
    })
}

/// Detects `struct` definitions whose fields are not ordered largest-to-smallest,
/// which on most targets wastes padding bytes per instance (storage packing).
fn detect_storage_packing(content: &str, file: &str) -> Vec<TransformSuggestion> {
    let mut suggestions = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if (trimmed.starts_with("pub struct ") || trimmed.starts_with("struct "))
            && trimmed.contains('{')
        {
            let struct_start = i;
            let mut fields: Vec<(String, usize)> = Vec::new();
            let mut j = i + 1;
            while j < lines.len() && !lines[j].trim().starts_with('}') {
                let field_line = lines[j].trim().trim_end_matches(',');
                if let Some(colon) = field_line.find(':') {
                    let field_name = field_line[..colon]
                        .trim_start_matches("pub ")
                        .trim()
                        .to_string();
                    let field_type = field_line[colon + 1..].trim();
                    if let Some(size) = type_size_hint(field_type) {
                        fields.push((field_name, size));
                    } else {
                        // Unknown type — bail out on this struct to avoid false positives.
                        fields.clear();
                        break;
                    }
                }
                j += 1;
            }
            if fields.len() >= 2 {
                let mut sorted = fields.clone();
                sorted.sort_by_key(|a| std::cmp::Reverse(a.1));
                if sorted != fields {
                    suggestions.push(TransformSuggestion {
                        file: file.to_string(),
                        line: struct_start + 1,
                        category: TransformCategory::StoragePacking,
                        original: lines[struct_start].to_string(),
                        suggested: format!(
                            "// reorder fields largest-to-smallest: {}",
                            sorted
                                .iter()
                                .map(|(n, _)| n.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        reason: "Struct fields are not ordered largest-to-smallest, which can waste padding bytes per instance. Reordering reduces storage/serialization size.".to_string(),
                    });
                }
            }
            i = j;
        }
        i += 1;
    }
    suggestions
}

/// Detects small, private, single-use helper functions that are good
/// candidates for `#[inline]` to remove call overhead.
fn detect_function_inlining(content: &str, file: &str) -> Vec<TransformSuggestion> {
    let mut suggestions = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("fn ") && !trimmed.starts_with("pub fn ") && trimmed.ends_with('{') {
            let prev = if i > 0 { lines[i - 1].trim() } else { "" };
            if prev.contains("#[inline") {
                i += 1;
                continue;
            }
            let mut depth = 1;
            let mut j = i + 1;
            let mut body_lines = 0usize;
            while j < lines.len() && depth > 0 {
                depth += lines[j].matches('{').count();
                depth = depth.saturating_sub(lines[j].matches('}').count());
                let t = lines[j].trim();
                if !t.is_empty() && t != "}" {
                    body_lines += 1;
                }
                j += 1;
            }
            let name = trimmed
                .trim_start_matches("fn ")
                .split(|c: char| c == '(' || c == '<' || c.is_whitespace())
                .next()
                .unwrap_or("");
            let call_count = content.matches(&format!("{}(", name)).count();
            // Body of 3 lines or fewer, called exactly once beyond its own definition.
            if body_lines <= 3 && call_count == 2 && !name.is_empty() {
                suggestions.push(TransformSuggestion {
                    file: file.to_string(),
                    line: i + 1,
                    category: TransformCategory::FunctionInlining,
                    original: lines[i].to_string(),
                    suggested: format!("#[inline]\n{}", lines[i]),
                    reason: format!(
                        "`{}` is a small (<= 3 line) private function called exactly once — adding #[inline] lets the compiler remove the call overhead.",
                        name
                    ),
                });
            }
            i = j;
            continue;
        }
        i += 1;
    }
    suggestions
}

/// Detects `Vec::new()` followed by repeated `.push(...)` inside a `for` loop
/// without a prior `with_capacity`, which causes repeated reallocation.
fn detect_loop_optimization(content: &str, file: &str) -> Vec<TransformSuggestion> {
    let mut suggestions = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("for ") && trimmed.contains(" in ") {
            // Look ahead a few lines for a `.push(` call inside this loop body.
            let window_end = (idx + 8).min(lines.len());
            let has_push = lines[idx + 1..window_end]
                .iter()
                .any(|l| l.trim_start().contains(".push("));
            if has_push {
                // Look behind a few lines for a `with_capacity` on the vec being built.
                let window_start = idx.saturating_sub(5);
                let has_capacity = lines[window_start..idx]
                    .iter()
                    .any(|l| l.contains("with_capacity"));
                if !has_capacity {
                    suggestions.push(TransformSuggestion {
                        file: file.to_string(),
                        line: idx + 1,
                        category: TransformCategory::LoopOptimization,
                        original: line.to_string(),
                        suggested: format!(
                            "// preallocate: Vec::with_capacity(<expected_len>) before this loop\n{}",
                            line
                        ),
                        reason: "A loop pushes into a Vec that wasn't preallocated with with_capacity — this causes repeated reallocation as the vector grows.".to_string(),
                    });
                }
            }
        }
    }
    suggestions
}

// ── Command handlers ──────────────────────────────────────────────────────────

pub async fn handle(cmd: OptimizeCommands) -> Result<()> {
    match cmd {
        OptimizeCommands::Analyse(args) => handle_analyse(args),
        OptimizeCommands::Transform(args) => handle_transform(args),
        OptimizeCommands::Bench(args) => handle_bench(args),
        OptimizeCommands::Report(args) => handle_report(args),
        OptimizeCommands::Reports(args) => handle_reports(args),
    }
}

fn handle_analyse(args: AnalyseArgs) -> Result<()> {
    p::header("Contract Performance Analysis");

    p::step(1, 2, "Loading and validating WASM…");
    if !args.wasm.exists() {
        anyhow::bail!(
            "WASM file not found: {}\nRun `stellar contract build` first.",
            args.wasm.display()
        );
    }
    let bytes = fs::read(&args.wasm)?;
    if bytes.len() < 4 || &bytes[..4] != b"\0asm" {
        anyhow::bail!("Not a valid WASM binary: {}", args.wasm.display());
    }
    let hash = wasm_hash_hex(&bytes);

    p::step(2, 2, "Analysing performance characteristics…");
    let issues = analyse_wasm(&bytes);
    let score = compute_score(&issues);

    let critical = issues
        .iter()
        .filter(|i| i.severity == IssueSeverity::Critical)
        .count();
    let warnings = issues
        .iter()
        .filter(|i| i.severity == IssueSeverity::Warning)
        .count();
    let infos = issues
        .iter()
        .filter(|i| i.severity == IssueSeverity::Info)
        .count();

    let report = OptimizationReport {
        id: format!("opt-{}", &hash[..12]),
        contract: args.contract.clone(),
        wasm_hash: hash.clone(),
        wasm_size_bytes: bytes.len(),
        timestamp: Utc::now().to_rfc3339(),
        total_issues: issues.len(),
        critical,
        warnings,
        infos,
        overall_score: score,
        issues: issues.clone(),
    };

    // Persist
    let mut reports = load_reports_store()?;
    reports.retain(|r| r.id != report.id);
    reports.push(report.clone());
    save_reports_store(&reports)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!();
        p::separator();
        p::kv_accent("Report ID", &report.id);
        p::kv("Contract", &args.contract);
        p::kv(
            "WASM size",
            &format!("{:.1} KB", bytes.len() as f64 / 1024.0),
        );
        p::kv("WASM hash", &hash);

        let score_str = format!("{}/100", score);
        let score_colored = if score >= 80 {
            score_str.green().to_string()
        } else if score >= 50 {
            score_str.yellow().to_string()
        } else {
            score_str.red().to_string()
        };
        p::kv_accent("Optimization score", &score_colored);
        p::kv("Issues found", &format!("{}", report.total_issues));
        p::kv("Critical", &format!("{}", critical));
        p::kv("Warnings", &format!("{}", warnings));
        p::kv("Infos", &format!("{}", infos));

        if !issues.is_empty() {
            println!();
            for issue in &issues {
                let sev = match issue.severity {
                    IssueSeverity::Critical => issue.severity.to_string().red().to_string(),
                    IssueSeverity::Warning => issue.severity.to_string().yellow().to_string(),
                    IssueSeverity::Info => issue.severity.to_string().dimmed().to_string(),
                };
                println!(
                    "  {} [{}] {}",
                    issue.id.white(),
                    sev,
                    issue.description.white()
                );
                println!("    {} {}", "→".dimmed(), issue.recommendation.dimmed());
                if let Some(saving) = issue.estimated_saving_pct {
                    println!("    {} Estimated saving: {:.0}%", "~".dimmed(), saving);
                }
                println!();
            }
        }
        p::separator();
    }

    if args.fail_on_critical && report.has_critical() {
        anyhow::bail!(
            "{} critical performance issue(s) found. Fix them before deploying.",
            critical
        );
    }

    Ok(())
}

fn handle_transform(args: TransformArgs) -> Result<()> {
    p::header("Contract Source Transformation");

    if !args.src.exists() {
        anyhow::bail!("Source file not found: {}", args.src.display());
    }

    let content = fs::read_to_string(&args.src)?;
    let file_str = args.src.to_string_lossy().to_string();
    let suggestions = analyse_source(&content, &file_str);

    if suggestions.is_empty() {
        p::separator();
        p::success("No transformation suggestions found — source looks clean.");
        p::separator();
    } else {
        p::separator();
        println!(
            "  {} suggestion(s) found in {}:",
            suggestions.len().to_string().yellow().bold(),
            args.src.display().to_string().cyan()
        );
        println!();

        let mut by_category: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for s in &suggestions {
            *by_category.entry(s.category.to_string()).or_insert(0) += 1;
        }
        println!("  {}", "By optimization type:".dimmed());
        for (cat, count) in &by_category {
            println!("    {} {}", cat.cyan(), count);
        }
        println!();

        for s in &suggestions {
            println!(
                "  Line {} [{}]: {}",
                s.line.to_string().white().bold(),
                s.category.to_string().magenta(),
                s.reason.dimmed()
            );
            println!("    {} {}", "Before:".dimmed(), s.original.trim().white());
            println!("    {} {}", "After: ".dimmed(), s.suggested.trim().cyan());
            println!();
        }
    }

    if args.ai {
        if let Some(opinion) = fetch_ai_optimization_opinion(&content)? {
            println!("  {}", "AI Optimization Opinion:".magenta().bold());
            println!("  {}", opinion.trim());
            println!();
        } else {
            p::info("AI opinion unavailable: set STARFORGE_AI_API_KEY to enable it.");
        }
    }

    if suggestions.is_empty() {
        return Ok(());
    }

    if args.dry_run {
        p::info("Dry-run mode: no files were modified.");
        return Ok(());
    }

    if !args.yes {
        let confirmed = dialoguer::Confirm::new()
            .with_prompt(format!(
                "Apply {} transformation(s) to {}? A backup will be kept.",
                suggestions.len(),
                args.out.as_ref().unwrap_or(&args.src).display()
            ))
            .default(false)
            .interact()
            .unwrap_or(false);
        if !confirmed {
            p::info("Aborted: no files were modified.");
            return Ok(());
        }
    }

    // Apply suggestions
    let out_path = args.out.as_ref().unwrap_or(&args.src);

    // Safety guarantee: always keep a pre-transform backup of the source that
    // is about to be overwritten, so changes can be reverted manually.
    if out_path == &args.src {
        let backup_path = args.src.with_extension(format!(
            "{}.bak",
            args.src
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("rs")
        ));
        fs::write(&backup_path, &content)?;
        p::info(&format!("Backup written to {}.", backup_path.display()));
    }

    let mut result = content.clone();
    for s in &suggestions {
        result = result.replacen(&s.original, &s.suggested, 1);
    }
    fs::write(out_path, &result)?;

    p::success(&format!(
        "Applied {} transformation(s) to {}.",
        suggestions.len(),
        out_path.display()
    ));
    println!(
        "  {} {} bytes → {} bytes",
        "Size:".dimmed(),
        content.len(),
        result.len()
    );
    p::info("Review the changes before committing.");
    Ok(())
}

/// Optionally queries an OpenAI-compatible LLM for a natural-language
/// optimization opinion on top of the deterministic static analysis, making
/// this genuinely "AI-driven" per issue #497 while keeping the measurable,
/// safety-guaranteed static engine as the primary source of truth. No-ops
/// (returns `Ok(None)`) when no API key is configured.
fn fetch_ai_optimization_opinion(source: &str) -> Result<Option<String>> {
    let api_key = match std::env::var("STARFORGE_AI_API_KEY") {
        Ok(key) if !key.trim().is_empty() => key,
        _ => return Ok(None),
    };
    let base_url = std::env::var("STARFORGE_AI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let model = std::env::var("STARFORGE_AI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());

    let truncated: String = source.chars().take(6000).collect();
    let body = serde_json::json!({
        "model": model,
        "temperature": 0.2,
        "messages": [
            {
                "role": "system",
                "content": "You are a Soroban smart contract performance expert. Suggest \
                    concrete gas/size optimizations (storage packing, function inlining, loop \
                    optimization, constant folding, dead-code removal) for the given contract \
                    source. Be specific and concise (under 200 words). Do not repeat generic advice."
            },
            {
                "role": "user",
                "content": format!("Contract source:\n```rust\n{}\n```", truncated)
            }
        ]
    });

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let start = std::time::Instant::now();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| -> Result<(String, Option<u64>, Option<u64>)> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(async {
                let resp = crate::utils::http_client::get_client()
                    .post(&url)
                    .bearer_auth(&api_key)
                    .json(&body)
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<serde_json::Value>()
                    .await?;
                let tokens_in = resp
                    .pointer("/usage/prompt_tokens")
                    .and_then(|v| v.as_u64());
                let tokens_out = resp
                    .pointer("/usage/completion_tokens")
                    .and_then(|v| v.as_u64());
                let content = resp
                    .pointer("/choices/0/message/content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Ok((content, tokens_in, tokens_out))
            })
        })();
        let _ = tx.send(result);
    });

    let elapsed_ms = start.elapsed().as_millis() as u64;
    match rx.recv() {
        Ok(Ok((content, tokens_in, tokens_out))) => {
            ai_telemetry::record_call(
                "openai",
                &model,
                "contract-optimize",
                tokens_in,
                tokens_out,
                elapsed_ms,
                true,
                None,
            );
            if content.trim().is_empty() {
                Ok(None)
            } else {
                Ok(Some(content))
            }
        }
        Ok(Err(e)) => {
            ai_telemetry::record_call(
                "openai",
                &model,
                "contract-optimize",
                None,
                None,
                elapsed_ms,
                false,
                Some("network"),
            );
            Err(e)
        }
        Err(e) => Err(anyhow::anyhow!(
            "AI opinion worker exited unexpectedly: {}",
            e
        )),
    }
}

fn handle_bench(args: BenchArgs) -> Result<()> {
    p::header("WASM Performance Benchmark Comparison");

    p::step(1, 2, "Loading WASM binaries…");
    if !args.baseline.exists() {
        anyhow::bail!("Baseline WASM not found: {}", args.baseline.display());
    }
    if !args.optimized.exists() {
        anyhow::bail!("Optimized WASM not found: {}", args.optimized.display());
    }

    let baseline_bytes = fs::read(&args.baseline)?;
    let optimized_bytes = fs::read(&args.optimized)?;

    if baseline_bytes.len() < 4 || &baseline_bytes[..4] != b"\0asm" {
        anyhow::bail!("Baseline is not a valid WASM binary.");
    }
    if optimized_bytes.len() < 4 || &optimized_bytes[..4] != b"\0asm" {
        anyhow::bail!("Optimized is not a valid WASM binary.");
    }

    p::step(2, 2, "Comparing metrics…");
    let baseline_hash = wasm_hash_hex(&baseline_bytes);
    let optimized_hash = wasm_hash_hex(&optimized_bytes);
    let size_delta = optimized_bytes.len() as i64 - baseline_bytes.len() as i64;
    let size_reduction_pct = if !baseline_bytes.is_empty() {
        ((baseline_bytes.len() as f64 - optimized_bytes.len() as f64) / baseline_bytes.len() as f64)
            * 100.0
    } else {
        0.0
    };

    let baseline_instr = estimate_instruction_count(&baseline_bytes);
    let optimized_instr = estimate_instruction_count(&optimized_bytes);
    let instr_delta = optimized_instr as i64 - baseline_instr as i64;
    let instr_reduction_pct = if baseline_instr > 0 {
        ((baseline_instr as f64 - optimized_instr as f64) / baseline_instr as f64) * 100.0
    } else {
        0.0
    };

    let comparison = BenchmarkComparison {
        baseline_hash: baseline_hash.clone(),
        optimized_hash: optimized_hash.clone(),
        baseline_size_bytes: baseline_bytes.len(),
        optimized_size_bytes: optimized_bytes.len(),
        size_delta_bytes: size_delta,
        size_reduction_pct,
        baseline_instruction_count: baseline_instr,
        optimized_instruction_count: optimized_instr,
        instruction_delta: instr_delta,
        instruction_reduction_pct: instr_reduction_pct,
        timestamp: Utc::now().to_rfc3339(),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&comparison)?);
        return Ok(());
    }

    p::separator();
    p::kv("Baseline hash", &format!("{}…", &baseline_hash[..16]));
    p::kv("Optimized hash", &format!("{}…", &optimized_hash[..16]));
    println!();

    let size_str = format!(
        "{} → {} bytes ({:+} bytes, {:.1}% {})",
        baseline_bytes.len(),
        optimized_bytes.len(),
        size_delta,
        size_reduction_pct.abs(),
        if size_reduction_pct >= 0.0 {
            "smaller"
        } else {
            "larger"
        }
    );
    let size_colored = if size_delta <= 0 {
        size_str.green().to_string()
    } else {
        size_str.red().to_string()
    };
    p::kv_accent("Binary size", &size_colored);

    let instr_str = format!(
        "{} → {} opcodes ({:+}, {:.1}% {})",
        baseline_instr,
        optimized_instr,
        instr_delta,
        instr_reduction_pct.abs(),
        if instr_reduction_pct >= 0.0 {
            "fewer"
        } else {
            "more"
        }
    );
    let instr_colored = if instr_delta <= 0 {
        instr_str.green().to_string()
    } else {
        instr_str.red().to_string()
    };
    p::kv("Instruction count", &instr_colored);

    println!();
    if size_delta < 0 {
        p::success("Optimized binary is smaller — good work!");
    } else if size_delta == 0 {
        p::info("Binaries are the same size.");
    } else {
        p::warn("Optimized binary is larger than baseline — review your changes.");
    }
    p::separator();
    Ok(())
}

fn handle_report(args: ReportArgs) -> Result<()> {
    p::header("Optimization Report");

    let reports = load_reports_store()?;
    let report = reports
        .iter()
        .rfind(|r| r.contract == args.contract)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No optimization report found for contract '{}'. Run `starforge optimize analyse` first.",
                args.contract
            )
        })?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }

    p::separator();
    p::kv_accent("Report ID", &report.id);
    p::kv("Contract", &report.contract);
    p::kv(
        "WASM size",
        &format!("{:.1} KB", report.wasm_size_bytes as f64 / 1024.0),
    );
    p::kv("WASM hash", &report.wasm_hash);

    let score_str = format!("{}/100", report.overall_score);
    let score_colored = if report.overall_score >= 80 {
        score_str.green().to_string()
    } else if report.overall_score >= 50 {
        score_str.yellow().to_string()
    } else {
        score_str.red().to_string()
    };
    p::kv_accent("Score", &score_colored);
    p::kv("Timestamp", &report.timestamp);
    println!();

    for issue in &report.issues {
        let sev = match issue.severity {
            IssueSeverity::Critical => issue.severity.to_string().red().to_string(),
            IssueSeverity::Warning => issue.severity.to_string().yellow().to_string(),
            IssueSeverity::Info => issue.severity.to_string().dimmed().to_string(),
        };
        println!(
            "  [{}] [{}] {}",
            issue.id.white(),
            sev,
            issue.description.white()
        );
        println!("    → {}", issue.recommendation.dimmed());
    }
    p::separator();
    Ok(())
}

fn handle_reports(args: ReportsArgs) -> Result<()> {
    p::header("Optimization Reports");

    let reports = load_reports_store()?;
    let filtered: Vec<_> = reports
        .iter()
        .filter(|r| args.contract.as_deref().map_or(true, |c| r.contract == c))
        .collect();

    if filtered.is_empty() {
        p::info("No reports found. Run `starforge optimize analyse` first.");
        return Ok(());
    }

    p::separator();
    println!(
        "  {:<16}  {:<20}  {:<8}  {:<8}  {:<8}  {}",
        "ID".dimmed(),
        "Contract".dimmed(),
        "Score".dimmed(),
        "Critical".dimmed(),
        "Warnings".dimmed(),
        "Timestamp".dimmed(),
    );
    println!("  {}", "─".repeat(80).dimmed());

    for r in filtered {
        let score_str = format!("{}/100", r.overall_score);
        let score_colored = if r.overall_score >= 80 {
            score_str.green().to_string()
        } else if r.overall_score >= 50 {
            score_str.yellow().to_string()
        } else {
            score_str.red().to_string()
        };
        let critical_str = if r.critical > 0 {
            format!("{}", r.critical).red().to_string()
        } else {
            "0".green().to_string()
        };
        println!(
            "  {:<16}  {:<20}  {:<8}  {:<8}  {:<8}  {}",
            r.id.white(),
            r.contract.cyan(),
            score_colored,
            critical_str,
            format!("{}", r.warnings).yellow(),
            r.timestamp.get(..16).unwrap_or(&r.timestamp).dimmed(),
        );
    }
    p::separator();
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_wasm() -> Vec<u8> {
        vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
    }

    #[test]
    fn wasm_hash_hex_length() {
        let hash = wasm_hash_hex(b"test");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn wasm_hash_hex_is_deterministic() {
        let bytes = minimal_wasm();
        assert_eq!(wasm_hash_hex(&bytes), wasm_hash_hex(&bytes));
    }

    #[test]
    fn issue_severity_display() {
        assert_eq!(IssueSeverity::Critical.to_string(), "critical");
        assert_eq!(IssueSeverity::Warning.to_string(), "warning");
        assert_eq!(IssueSeverity::Info.to_string(), "info");
    }

    #[test]
    fn analyse_wasm_small_binary_no_critical() {
        let issues = analyse_wasm(&minimal_wasm());
        let critical_count = issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Critical)
            .count();
        assert_eq!(critical_count, 0);
    }

    #[test]
    fn analyse_wasm_large_binary_triggers_critical() {
        // Build a fake "large" WASM (>100 KB)
        let mut large = minimal_wasm();
        large.extend(vec![0x00; 110 * 1024]);
        let issues = analyse_wasm(&large);
        assert!(issues
            .iter()
            .any(|i| i.kind == "binary-size" && i.severity == IssueSeverity::Critical));
    }

    #[test]
    fn compute_score_no_issues_is_100() {
        assert_eq!(compute_score(&[]), 100);
    }

    #[test]
    fn compute_score_critical_issues_reduce_score() {
        let issues = vec![OptimizationIssue {
            id: "OPT-001".to_string(),
            kind: "binary-size".to_string(),
            severity: IssueSeverity::Critical,
            description: "Too large".to_string(),
            recommendation: "Shrink it".to_string(),
            estimated_saving_pct: Some(20.0),
        }];
        let score = compute_score(&issues);
        assert!(score < 100);
        assert_eq!(score, 75); // 100 - 25
    }

    #[test]
    fn compute_score_clamps_at_zero() {
        let issues: Vec<OptimizationIssue> = (0..10)
            .map(|i| OptimizationIssue {
                id: format!("OPT-{:03}", i),
                kind: "test".to_string(),
                severity: IssueSeverity::Critical,
                description: "issue".to_string(),
                recommendation: "fix".to_string(),
                estimated_saving_pct: None,
            })
            .collect();
        assert_eq!(compute_score(&issues), 0);
    }

    #[test]
    fn has_critical_returns_false_with_no_issues() {
        let report = OptimizationReport {
            id: "opt-test".to_string(),
            contract: "c".to_string(),
            wasm_hash: "abc".to_string(),
            wasm_size_bytes: 1024,
            timestamp: Utc::now().to_rfc3339(),
            total_issues: 0,
            critical: 0,
            warnings: 0,
            infos: 0,
            overall_score: 100,
            issues: vec![],
        };
        assert!(!report.has_critical());
    }

    #[test]
    fn analyse_source_finds_unwrap() {
        let content = "let x = some_option.unwrap();";
        let suggestions = analyse_source(content, "test.rs");
        assert!(suggestions.iter().any(|s| s.reason.contains("unwrap")));
    }

    #[test]
    fn analyse_source_clean_code_no_suggestions() {
        let content = r#"
fn add(a: u64, b: u64) -> u64 {
    a + b
}
"#;
        let suggestions = analyse_source(content, "clean.rs");
        assert!(suggestions.is_empty());
    }

    #[test]
    fn detects_constant_folding_opportunity() {
        let content = "let seconds_per_day = 60 * 60 * 24;";
        let suggestions = detect_constant_folding(content, "test.rs");
        assert!(suggestions
            .iter()
            .any(|s| s.category == TransformCategory::ConstantFolding));
    }

    #[test]
    fn detects_dead_code_unused_function() {
        let content = r#"
fn used_helper() -> u64 {
    let x = 1;
    let y = 2;
    x + y
}

fn unused_helper() -> u64 {
    let a = 10;
    let b = 20;
    a + b
}

pub fn entry_point() -> u64 {
    used_helper()
}
"#;
        let suggestions = detect_dead_code(content, "test.rs");
        assert!(suggestions
            .iter()
            .any(|s| s.reason.contains("`unused_helper`")));
        assert!(!suggestions
            .iter()
            .any(|s| s.reason.contains("`used_helper`")));
    }

    #[test]
    fn detects_storage_packing_suboptimal_order() {
        let content = r#"
pub struct Account {
    active: bool,
    balance: u128,
    nonce: u64,
}
"#;
        let suggestions = detect_storage_packing(content, "test.rs");
        assert!(suggestions
            .iter()
            .any(|s| s.category == TransformCategory::StoragePacking));
    }

    #[test]
    fn storage_packing_accepts_already_sorted_struct() {
        let content = r#"
pub struct Account {
    balance: u128,
    nonce: u64,
    active: bool,
}
"#;
        let suggestions = detect_storage_packing(content, "test.rs");
        assert!(suggestions.is_empty());
    }

    #[test]
    fn detects_loop_missing_with_capacity() {
        let content = r#"
fn build_list(n: u32) -> Vec<u32> {
    let mut out = Vec::new();
    for i in 0..n {
        out.push(i);
    }
    out
}
"#;
        let suggestions = detect_loop_optimization(content, "test.rs");
        assert!(suggestions
            .iter()
            .any(|s| s.category == TransformCategory::LoopOptimization));
    }

    #[test]
    fn transform_category_display() {
        assert_eq!(
            TransformCategory::StoragePacking.to_string(),
            "storage-packing"
        );
        assert_eq!(
            TransformCategory::DeadCodeElimination.to_string(),
            "dead-code-elimination"
        );
    }

    #[test]
    fn estimate_instruction_count_returns_zero_for_minimal_wasm() {
        let wasm = minimal_wasm();
        // 8-byte header, no instructions
        assert_eq!(estimate_instruction_count(&wasm), 0);
    }

    #[test]
    fn estimate_instruction_count_returns_more_for_larger_wasm() {
        let mut wasm = minimal_wasm();
        wasm.extend(vec![0x01, 0x02, 0x7f, 0x40]);
        let count = estimate_instruction_count(&wasm);
        assert!(count > 0);
    }
}
