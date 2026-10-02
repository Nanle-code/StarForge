//! Mutation Testing Engine
//!
//! An offline, rule-based mutation-testing engine for Soroban contracts. It
//! evaluates how *effective* a test suite is by deliberately introducing small
//! faults ("mutants") into contract source and checking whether the tests
//! notice. A test suite that still passes against a mutated contract has a
//! blind spot.
//!
//! Pipeline:
//!
//!   1. [`generate_mutants`] – apply mutation operators to the source, skipping
//!      comments, string literals and the contract's own `#[cfg(test)]` module.
//!   2. [`run_mutation_testing`] – apply each mutant and ask a [`TestExecutor`]
//!      whether the suite caught it (killed) or not (survived).
//!   3. The resulting [`MutationReport`] carries the mutation score, per-operator
//!      and per-function breakdowns, weak-spot detection and targeted test
//!      improvement suggestions.
//!
//! The engine is deliberately dependency-light so it stays fast and easy to
//! unit-test; the actual test-command execution is injected through the
//! [`TestExecutor`] trait, which keeps the analysis logic pure and testable.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

// ── Operators ─────────────────────────────────────────────────────────────────

/// A mutation strategy. Each operator rewrites a specific kind of construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MutationOperator {
    /// `+` ↔ `-`, `*` ↔ `/`
    #[serde(rename = "arithmetic")]
    Arithmetic,
    /// `==` ↔ `!=`, `<` ↔ `>=`, `>` ↔ `<=`
    #[serde(rename = "comparison")]
    Comparison,
    /// `true` ↔ `false`
    #[serde(rename = "boolean")]
    BooleanLiteral,
    /// `&&` ↔ `||`
    #[serde(rename = "logical")]
    LogicalConnector,
    /// `+=` ↔ `-=`
    #[serde(rename = "assignment")]
    AssignmentOperator,
    /// Integer literal tweaks (`0` ↔ `1`, `n` → `n + 1`)
    #[serde(rename = "constant")]
    NumericConstant,
    /// Drop a unary `!`
    #[serde(rename = "negation")]
    NegationRemoval,
    /// Delete a `require_auth()` authorisation check (Soroban-specific)
    #[serde(rename = "require-auth")]
    RequireAuthRemoval,
    /// Swap storage durability: `persistent` / `instance` / `temporary`
    #[serde(rename = "storage-durability")]
    StorageDurability,
    /// `unwrap_or(x)` → `unwrap_or(Default::default())`
    #[serde(rename = "unwrap-default")]
    UnwrapDefault,
}

impl MutationOperator {
    /// Stable machine-readable slug (used for `--operators` and JSON).
    pub fn slug(self) -> &'static str {
        match self {
            MutationOperator::Arithmetic => "arithmetic",
            MutationOperator::Comparison => "comparison",
            MutationOperator::BooleanLiteral => "boolean",
            MutationOperator::LogicalConnector => "logical",
            MutationOperator::AssignmentOperator => "assignment",
            MutationOperator::NumericConstant => "constant",
            MutationOperator::NegationRemoval => "negation",
            MutationOperator::RequireAuthRemoval => "require-auth",
            MutationOperator::StorageDurability => "storage-durability",
            MutationOperator::UnwrapDefault => "unwrap-default",
        }
    }

    /// Human-readable description, shown by `starforge mutate operators`.
    pub fn description(self) -> &'static str {
        match self {
            MutationOperator::Arithmetic => "Swap arithmetic operators (+/-, */÷)",
            MutationOperator::Comparison => "Swap comparison operators (==/!=, </>=)",
            MutationOperator::BooleanLiteral => "Flip boolean literals (true/false)",
            MutationOperator::LogicalConnector => "Swap logical connectors (&&/||)",
            MutationOperator::AssignmentOperator => "Swap compound assignment (+=/-=)",
            MutationOperator::NumericConstant => "Perturb integer constants (0/1, n+1)",
            MutationOperator::NegationRemoval => "Remove a unary negation (!)",
            MutationOperator::RequireAuthRemoval => "Remove a require_auth() check",
            MutationOperator::StorageDurability => "Swap storage durability tier",
            MutationOperator::UnwrapDefault => "Replace unwrap_or fallback with default",
        }
    }

    /// Every operator, in a stable order.
    pub fn all() -> &'static [MutationOperator] {
        &[
            MutationOperator::Arithmetic,
            MutationOperator::Comparison,
            MutationOperator::BooleanLiteral,
            MutationOperator::LogicalConnector,
            MutationOperator::AssignmentOperator,
            MutationOperator::NumericConstant,
            MutationOperator::NegationRemoval,
            MutationOperator::RequireAuthRemoval,
            MutationOperator::StorageDurability,
            MutationOperator::UnwrapDefault,
        ]
    }

    /// Parse a user-supplied operator slug.
    pub fn parse(s: &str) -> Option<MutationOperator> {
        let s = s.trim().to_lowercase();
        MutationOperator::all()
            .iter()
            .copied()
            .find(|op| op.slug() == s)
    }
}

impl fmt::Display for MutationOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

// ── Mutants ───────────────────────────────────────────────────────────────────

/// A single generated mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mutant {
    /// 1-based sequential id within a run.
    pub id: usize,
    /// Source file the mutant belongs to.
    pub file: String,
    /// 1-based line number.
    pub line: usize,
    /// 0-based byte offset of the mutated token within the line.
    pub column: usize,
    pub operator: MutationOperator,
    /// The token that was replaced.
    pub original: String,
    /// What it was replaced with.
    pub replacement: String,
    /// The full original line (trimmed of trailing whitespace).
    pub original_line: String,
    /// The full mutated line.
    pub mutated_line: String,
    /// Enclosing function name, when it could be determined.
    pub function: Option<String>,
}

impl Mutant {
    /// A short human description, e.g. `comparison: '==' -> '!=' at line 42`.
    pub fn summary(&self) -> String {
        format!(
            "{}: '{}' -> '{}' at line {}",
            self.operator.slug(),
            self.original,
            self.replacement,
            self.line
        )
    }
}

/// Knobs controlling mutant generation. Defaults mutate everything except the
/// contract's own test module.
#[derive(Debug, Clone)]
pub struct MutationConfig {
    /// Operators to apply. Empty means "all operators".
    pub operators: Vec<MutationOperator>,
    /// Cap on the number of mutants produced (performance guard). When the
    /// generated set is larger, mutants are sampled with a deterministic
    /// even stride so the surviving sample stays spread across the file.
    pub max_mutants: Option<usize>,
    /// Skip the contract's `#[cfg(test)]` module (mutating tests is meaningless).
    pub skip_tests: bool,
}

impl Default for MutationConfig {
    fn default() -> Self {
        MutationConfig {
            operators: Vec::new(),
            max_mutants: None,
            skip_tests: true,
        }
    }
}

impl MutationConfig {
    fn enabled(&self, op: MutationOperator) -> bool {
        self.operators.is_empty() || self.operators.contains(&op)
    }
}

// ── Generation ────────────────────────────────────────────────────────────────

/// Simple token-substitution rules keyed by operator. Each entry is
/// `(operator, needle, replacement)` and is matched against a *masked* copy of
/// the line so matches inside strings or comments are ignored.
///
/// Whitespace is included in the needles on purpose: it prevents `<` in
/// `Vec<u32>`, `-` in `-1`, or `*` in `*self` from being treated as binary
/// operators, which would generate mutants that never compile.
const TOKEN_RULES: &[(MutationOperator, &str, &str)] = &[
    // Comparison — checked before arithmetic so `==` wins over `=`.
    (MutationOperator::Comparison, " == ", " != "),
    (MutationOperator::Comparison, " != ", " == "),
    (MutationOperator::Comparison, " <= ", " < "),
    (MutationOperator::Comparison, " >= ", " > "),
    (MutationOperator::Comparison, " < ", " >= "),
    (MutationOperator::Comparison, " > ", " <= "),
    // Logical
    (MutationOperator::LogicalConnector, " && ", " || "),
    (MutationOperator::LogicalConnector, " || ", " && "),
    // Compound assignment (before bare arithmetic)
    (MutationOperator::AssignmentOperator, " += ", " -= "),
    (MutationOperator::AssignmentOperator, " -= ", " += "),
    // Arithmetic
    (MutationOperator::Arithmetic, " + ", " - "),
    (MutationOperator::Arithmetic, " - ", " + "),
    (MutationOperator::Arithmetic, " * ", " / "),
    (MutationOperator::Arithmetic, " / ", " * "),
    // Storage durability (Soroban-specific)
    (
        MutationOperator::StorageDurability,
        ".persistent()",
        ".temporary()",
    ),
    (
        MutationOperator::StorageDurability,
        ".instance()",
        ".temporary()",
    ),
    (
        MutationOperator::StorageDurability,
        ".temporary()",
        ".persistent()",
    ),
];

/// Generate every mutant the enabled operators can produce for `source`.
///
/// Mutants are deterministic and de-duplicated: the same source always yields
/// the same list in the same order, which keeps CI runs reproducible.
pub fn generate_mutants(source: &str, file: &str, cfg: &MutationConfig) -> Vec<Mutant> {
    let mut raw: Vec<Mutant> = Vec::new();
    let skip = if cfg.skip_tests {
        test_module_lines(source)
    } else {
        Vec::new()
    };

    let mut current_fn: Option<String> = None;
    let mut fn_depth: i32 = 0;
    let mut depth: i32 = 0;

    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let masked = mask_line(raw_line);

        // Track the enclosing function using brace depth on the masked line.
        if current_fn.is_none() {
            if let Some(name) = fn_name(&masked) {
                current_fn = Some(name);
                fn_depth = depth;
            }
        }
        for ch in masked.chars() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if current_fn.is_some() && depth <= fn_depth {
                        current_fn = None;
                    }
                }
                _ => {}
            }
        }

        // Never mutate inside the test module, attributes, or `use` lines.
        if skip.contains(&line_no) {
            continue;
        }
        let trimmed = raw_line.trim_start();
        if trimmed.starts_with("//")
            || trimmed.starts_with("#[")
            || trimmed.starts_with("#!")
            || trimmed.starts_with("use ")
        {
            continue;
        }

        collect_line_mutants(
            raw_line,
            &masked,
            line_no,
            file,
            current_fn.clone(),
            cfg,
            &mut raw,
        );
    }

    dedup_and_number(raw, cfg.max_mutants)
}

/// Apply all enabled operators to a single line.
fn collect_line_mutants(
    line: &str,
    masked: &str,
    line_no: usize,
    file: &str,
    function: Option<String>,
    cfg: &MutationConfig,
    out: &mut Vec<Mutant>,
) {
    let push = |out: &mut Vec<Mutant>,
                op: MutationOperator,
                col: usize,
                original: &str,
                replacement: &str,
                mutated_line: String| {
        out.push(Mutant {
            id: 0, // assigned later
            file: file.to_string(),
            line: line_no,
            column: col,
            operator: op,
            original: original.to_string(),
            replacement: replacement.to_string(),
            original_line: line.trim_end().to_string(),
            mutated_line: mutated_line.trim_end().to_string(),
            function: function.clone(),
        });
    };

    // 1. Simple token substitutions.
    for (op, needle, replacement) in TOKEN_RULES {
        if !cfg.enabled(*op) {
            continue;
        }
        for (col, _) in masked.match_indices(needle) {
            let mutated = splice(line, col, needle.len(), replacement);
            push(out, *op, col, needle, replacement, mutated);
        }
    }

    // 2. Boolean literals (word-boundary aware).
    if cfg.enabled(MutationOperator::BooleanLiteral) {
        for (word, replacement) in [("true", "false"), ("false", "true")] {
            for col in find_words(masked, word) {
                let mutated = splice(line, col, word.len(), replacement);
                push(
                    out,
                    MutationOperator::BooleanLiteral,
                    col,
                    word,
                    replacement,
                    mutated,
                );
            }
        }
    }

    // 3. Unary negation removal — `!` not preceded by an identifier char (that
    //    would be a macro call such as `vec!`) and followed by an identifier.
    if cfg.enabled(MutationOperator::NegationRemoval) {
        let bytes = masked.as_bytes();
        for (col, _) in masked.match_indices('!') {
            let prev_ok = col == 0 || !is_ident_byte(bytes[col - 1]);
            let next = bytes.get(col + 1).copied();
            let next_ok = next.map(|b| is_ident_byte(b) || b == b'(').unwrap_or(false);
            // `!=` is a comparison, not a negation.
            if next == Some(b'=') || !prev_ok || !next_ok {
                continue;
            }
            let mutated = splice(line, col, 1, "");
            push(
                out,
                MutationOperator::NegationRemoval,
                col,
                "!",
                "",
                mutated,
            );
        }
    }

    // 4. Integer constants.
    if cfg.enabled(MutationOperator::NumericConstant) {
        for (col, text) in find_int_literals(masked) {
            let replacement = match text.as_str() {
                "0" => "1".to_string(),
                "1" => "0".to_string(),
                other => match other.parse::<u64>() {
                    Ok(v) => (v.wrapping_add(1)).to_string(),
                    Err(_) => continue,
                },
            };
            let mutated = splice(line, col, text.len(), &replacement);
            push(
                out,
                MutationOperator::NumericConstant,
                col,
                &text,
                &replacement,
                mutated,
            );
        }
    }

    // 5. `unwrap_or(...)` fallback replacement.
    if cfg.enabled(MutationOperator::UnwrapDefault) {
        if let Some(col) = masked.find(".unwrap_or(") {
            if let Some(close) = matching_paren(masked, col + ".unwrap_or".len()) {
                let start = col + ".unwrap_or(".len();
                if start < close {
                    let inner = &line[start..close];
                    if inner != "Default::default()" {
                        let mutated = splice(line, start, close - start, "Default::default()");
                        push(
                            out,
                            MutationOperator::UnwrapDefault,
                            start,
                            inner,
                            "Default::default()",
                            mutated,
                        );
                    }
                }
            }
        }
    }

    // 6. `require_auth()` removal — comment the whole statement out. Only safe
    //    when the call is its own statement, which is the idiomatic form.
    if cfg.enabled(MutationOperator::RequireAuthRemoval)
        && masked.contains(".require_auth()")
        && line.trim_end().ends_with(';')
    {
        let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        let mutated = format!("{}// [mutant] {}", indent, line.trim());
        push(
            out,
            MutationOperator::RequireAuthRemoval,
            0,
            line.trim(),
            "<removed>",
            mutated,
        );
    }
}

/// Drop duplicate mutants, assign ids, and apply the `max_mutants` cap using a
/// deterministic even stride so the sample stays spread across the file.
fn dedup_and_number(raw: Vec<Mutant>, max: Option<usize>) -> Vec<Mutant> {
    let mut seen: Vec<(usize, usize, MutationOperator, String)> = Vec::new();
    let mut unique: Vec<Mutant> = Vec::new();
    for m in raw {
        let key = (m.line, m.column, m.operator, m.replacement.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        unique.push(m);
    }

    // Stable ordering: by line, then column, then operator.
    unique.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then(a.column.cmp(&b.column))
            .then(a.operator.cmp(&b.operator))
    });

    let mut selected = match max {
        Some(limit) if limit > 0 && unique.len() > limit => {
            let stride = unique.len() as f64 / limit as f64;
            let mut picked = Vec::with_capacity(limit);
            for i in 0..limit {
                let idx = ((i as f64) * stride).floor() as usize;
                picked.push(unique[idx.min(unique.len() - 1)].clone());
            }
            picked
        }
        _ => unique,
    };

    for (i, m) in selected.iter_mut().enumerate() {
        m.id = i + 1;
    }
    selected
}

/// Produce the mutated source for `mutant` by replacing its line.
pub fn apply_mutant(source: &str, mutant: &Mutant) -> String {
    let mut out: Vec<String> = Vec::new();
    for (idx, line) in source.lines().enumerate() {
        if idx + 1 == mutant.line {
            out.push(mutant.mutated_line.clone());
        } else {
            out.push(line.to_string());
        }
    }
    let mut joined = out.join("\n");
    if source.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

// ── Execution ─────────────────────────────────────────────────────────────────

/// What happened when the test suite ran against a mutant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutantOutcome {
    /// Tests failed — the mutation was detected. This is the good outcome.
    #[serde(rename = "killed")]
    Killed,
    /// Tests still passed — the mutation went unnoticed. Indicates a test gap.
    #[serde(rename = "survived")]
    Survived,
    /// Tests exceeded the timeout, usually an infinite loop. Counts as killed.
    #[serde(rename = "timeout")]
    Timeout,
    /// The mutant did not compile; it is not a valid test of the suite.
    #[serde(rename = "build-failed")]
    BuildFailed,
}

impl MutantOutcome {
    pub fn slug(self) -> &'static str {
        match self {
            MutantOutcome::Killed => "killed",
            MutantOutcome::Survived => "survived",
            MutantOutcome::Timeout => "timeout",
            MutantOutcome::BuildFailed => "build-failed",
        }
    }
}

/// Runs a test suite against mutated source. Implemented by the CLI with a real
/// subprocess runner, and by tests with a deterministic fake.
pub trait TestExecutor {
    /// Run the suite against `mutated_source` and report the outcome.
    fn run(&mut self, mutated_source: &str, mutant: &Mutant) -> Result<MutantOutcome, String>;
}

/// Outcome for one mutant, plus timing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutantResult {
    pub mutant: Mutant,
    pub outcome: MutantOutcome,
    pub duration_ms: u64,
}

// ── Report ────────────────────────────────────────────────────────────────────

/// Per-operator effectiveness breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorStat {
    pub operator: MutationOperator,
    pub total: usize,
    pub killed: usize,
    pub survived: usize,
    pub score: f64,
}

/// A function whose tests failed to catch one or more mutants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeakSpot {
    pub function: String,
    pub total: usize,
    pub survived: usize,
    pub score: f64,
    /// Lines where mutants survived, ascending.
    pub lines: Vec<usize>,
}

/// Severity of a test-improvement suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    #[serde(rename = "high")]
    High,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "low")]
    Low,
}

impl Severity {
    pub fn slug(self) -> &'static str {
        match self {
            Severity::High => "high",
            Severity::Medium => "medium",
            Severity::Low => "low",
        }
    }
}

/// A concrete, actionable recommendation for strengthening the test suite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub line: usize,
    pub function: Option<String>,
    pub operator: MutationOperator,
    pub severity: Severity,
    pub message: String,
}

/// The full result of a mutation-testing run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationReport {
    pub file: String,
    pub total: usize,
    pub killed: usize,
    pub survived: usize,
    pub timeout: usize,
    pub build_failed: usize,
    /// Mutation score as a percentage: detected / valid mutants.
    pub score: f64,
    pub duration_ms: u64,
    pub results: Vec<MutantResult>,
    pub operator_stats: Vec<OperatorStat>,
    pub weak_spots: Vec<WeakSpot>,
    pub suggestions: Vec<Suggestion>,
}

impl MutationReport {
    /// Mutants that the suite failed to detect.
    pub fn survivors(&self) -> Vec<&MutantResult> {
        self.results
            .iter()
            .filter(|r| r.outcome == MutantOutcome::Survived)
            .collect()
    }

    /// True when the score meets `min_score` (used for CI gating).
    pub fn meets_threshold(&self, min_score: f64) -> bool {
        self.score + f64::EPSILON >= min_score
    }
}

/// Execute mutation testing end to end: generate mutants, run the suite against
/// each one via `executor`, then analyse the outcomes.
pub fn run_mutation_testing(
    source: &str,
    file: &str,
    cfg: &MutationConfig,
    executor: &mut dyn TestExecutor,
) -> Result<MutationReport, String> {
    let mutants = generate_mutants(source, file, cfg);
    let mut results = Vec::with_capacity(mutants.len());
    let mut total_ms: u64 = 0;

    for mutant in mutants {
        let mutated = apply_mutant(source, &mutant);
        let start = std::time::Instant::now();
        let outcome = executor.run(&mutated, &mutant)?;
        let duration_ms = start.elapsed().as_millis() as u64;
        total_ms += duration_ms;
        results.push(MutantResult {
            mutant,
            outcome,
            duration_ms,
        });
    }

    Ok(analyze(file, results, total_ms))
}

/// Turn raw per-mutant outcomes into a scored, annotated report.
pub fn analyze(file: &str, results: Vec<MutantResult>, duration_ms: u64) -> MutationReport {
    let total = results.len();
    let killed = count(&results, MutantOutcome::Killed);
    let survived = count(&results, MutantOutcome::Survived);
    let timeout = count(&results, MutantOutcome::Timeout);
    let build_failed = count(&results, MutantOutcome::BuildFailed);

    // Mutants that failed to compile are not a fair test of the suite, so they
    // are excluded from the denominator. Timeouts count as detected.
    let valid = killed + survived + timeout;
    let detected = killed + timeout;
    let score = if valid == 0 {
        100.0
    } else {
        (detected as f64 / valid as f64) * 100.0
    };

    MutationReport {
        file: file.to_string(),
        total,
        killed,
        survived,
        timeout,
        build_failed,
        score,
        duration_ms,
        operator_stats: operator_stats(&results),
        weak_spots: weak_spots(&results),
        suggestions: suggestions(&results),
        results,
    }
}

fn count(results: &[MutantResult], outcome: MutantOutcome) -> usize {
    results.iter().filter(|r| r.outcome == outcome).count()
}

/// Per-operator kill rates, sorted by operator for stable output.
fn operator_stats(results: &[MutantResult]) -> Vec<OperatorStat> {
    let mut by_op: BTreeMap<MutationOperator, (usize, usize, usize)> = BTreeMap::new();
    for r in results {
        let entry = by_op.entry(r.mutant.operator).or_insert((0, 0, 0));
        entry.0 += 1;
        match r.outcome {
            MutantOutcome::Killed | MutantOutcome::Timeout => entry.1 += 1,
            MutantOutcome::Survived => entry.2 += 1,
            MutantOutcome::BuildFailed => {}
        }
    }
    by_op
        .into_iter()
        .map(|(operator, (total, killed, survived))| {
            let valid = killed + survived;
            OperatorStat {
                operator,
                total,
                killed,
                survived,
                score: if valid == 0 {
                    100.0
                } else {
                    (killed as f64 / valid as f64) * 100.0
                },
            }
        })
        .collect()
}

/// Functions with surviving mutants, worst score first — these are the weak
/// tests the issue asks us to surface.
fn weak_spots(results: &[MutantResult]) -> Vec<WeakSpot> {
    let mut by_fn: BTreeMap<String, (usize, usize, Vec<usize>)> = BTreeMap::new();
    for r in results {
        if r.outcome == MutantOutcome::BuildFailed {
            continue;
        }
        let name = r
            .mutant
            .function
            .clone()
            .unwrap_or_else(|| "<top-level>".to_string());
        let entry = by_fn.entry(name).or_insert((0, 0, Vec::new()));
        entry.0 += 1;
        if r.outcome == MutantOutcome::Survived {
            entry.1 += 1;
            entry.2.push(r.mutant.line);
        }
    }

    let mut spots: Vec<WeakSpot> = by_fn
        .into_iter()
        .filter(|(_, (_, survived, _))| *survived > 0)
        .map(|(function, (total, survived, mut lines))| {
            lines.sort_unstable();
            lines.dedup();
            let killed = total - survived;
            WeakSpot {
                function,
                total,
                survived,
                score: if total == 0 {
                    100.0
                } else {
                    (killed as f64 / total as f64) * 100.0
                },
                lines,
            }
        })
        .collect();

    // Worst first, then alphabetical for determinism.
    spots.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.function.cmp(&b.function))
    });
    spots
}

/// Build targeted test-improvement recommendations from surviving mutants.
fn suggestions(results: &[MutantResult]) -> Vec<Suggestion> {
    let mut out = Vec::new();
    for r in results {
        if r.outcome != MutantOutcome::Survived {
            continue;
        }
        let m = &r.mutant;
        let where_ = m
            .function
            .clone()
            .map(|f| format!("`{}`", f))
            .unwrap_or_else(|| "this file".to_string());

        let (severity, message) = match m.operator {
            MutationOperator::RequireAuthRemoval => (
                Severity::High,
                format!(
                    "Removing the authorisation check in {} did not fail any test. \
                     Add a negative test asserting an unauthorised caller is rejected.",
                    where_
                ),
            ),
            MutationOperator::StorageDurability => (
                Severity::High,
                format!(
                    "Changing the storage durability tier in {} went undetected. \
                     Assert that state survives (or expires) as the tier intends.",
                    where_
                ),
            ),
            MutationOperator::Comparison => (
                Severity::Medium,
                format!(
                    "Boundary condition at line {} in {} is untested — '{}' could be \
                     changed to '{}' unnoticed. Add cases exactly on and either side \
                     of the boundary.",
                    m.line,
                    where_,
                    m.original.trim(),
                    m.replacement.trim()
                ),
            ),
            MutationOperator::Arithmetic | MutationOperator::AssignmentOperator => (
                Severity::Medium,
                format!(
                    "The arithmetic at line {} in {} is unverified. Assert on the \
                     computed value rather than only that the call succeeds.",
                    m.line, where_
                ),
            ),
            MutationOperator::BooleanLiteral | MutationOperator::LogicalConnector => (
                Severity::Medium,
                format!(
                    "Both branches of the condition at line {} in {} are not exercised. \
                     Add a test covering the opposite outcome.",
                    m.line, where_
                ),
            ),
            MutationOperator::NegationRemoval => (
                Severity::Medium,
                format!(
                    "Dropping the negation at line {} in {} was not caught. Add a test \
                     for the inverted condition.",
                    m.line, where_
                ),
            ),
            MutationOperator::NumericConstant => (
                Severity::Low,
                format!(
                    "The constant '{}' at line {} in {} is not asserted anywhere. \
                     Check the exact expected value in a test.",
                    m.original, m.line, where_
                ),
            ),
            MutationOperator::UnwrapDefault => (
                Severity::Low,
                format!(
                    "The fallback value at line {} in {} is untested. Add a case that \
                     exercises the missing/default path.",
                    m.line, where_
                ),
            ),
        };

        out.push(Suggestion {
            line: m.line,
            function: m.function.clone(),
            operator: m.operator,
            severity,
            message,
        });
    }

    // High severity first, then by line.
    out.sort_by(|a, b| {
        severity_rank(a.severity)
            .cmp(&severity_rank(b.severity))
            .then(a.line.cmp(&b.line))
    });
    out
}

fn severity_rank(s: Severity) -> u8 {
    match s {
        Severity::High => 0,
        Severity::Medium => 1,
        Severity::Low => 2,
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Render a report as `markdown`, `text`, or `html`. JSON is handled by the CLI
/// via serde so the report struct stays the single source of truth.
pub fn render_report(report: &MutationReport, format: &str) -> Result<String, String> {
    match format {
        "markdown" | "md" => Ok(render_markdown(report)),
        "text" | "txt" => Ok(render_text(report)),
        "html" => Ok(render_html(report)),
        other => Err(format!(
            "Unsupported mutation report format '{}'. Use markdown, text, html, or json.",
            other
        )),
    }
}

fn render_markdown(r: &MutationReport) -> String {
    let mut s = String::new();
    s.push_str("# Mutation Testing Report\n\n");
    s.push_str(&format!("**File:** `{}`\n\n", r.file));
    s.push_str(&format!("**Mutation score:** {:.1}%\n\n", r.score));
    s.push_str("| Outcome | Count |\n|---|---|\n");
    s.push_str(&format!("| Killed | {} |\n", r.killed));
    s.push_str(&format!("| Survived | {} |\n", r.survived));
    s.push_str(&format!("| Timeout | {} |\n", r.timeout));
    s.push_str(&format!("| Build failed | {} |\n", r.build_failed));
    s.push_str(&format!("| **Total** | **{}** |\n\n", r.total));

    if !r.operator_stats.is_empty() {
        s.push_str("## By operator\n\n| Operator | Total | Killed | Survived | Score |\n|---|---|---|---|---|\n");
        for op in &r.operator_stats {
            s.push_str(&format!(
                "| {} | {} | {} | {} | {:.1}% |\n",
                op.operator.slug(),
                op.total,
                op.killed,
                op.survived,
                op.score
            ));
        }
        s.push('\n');
    }

    if !r.weak_spots.is_empty() {
        s.push_str("## Weak spots\n\n| Function | Survived | Total | Score | Lines |\n|---|---|---|---|---|\n");
        for w in &r.weak_spots {
            let lines: Vec<String> = w.lines.iter().map(|l| l.to_string()).collect();
            s.push_str(&format!(
                "| `{}` | {} | {} | {:.1}% | {} |\n",
                w.function,
                w.survived,
                w.total,
                w.score,
                lines.join(", ")
            ));
        }
        s.push('\n');
    }

    if !r.suggestions.is_empty() {
        s.push_str("## Test improvement suggestions\n\n");
        for sg in &r.suggestions {
            s.push_str(&format!(
                "- **[{}]** line {}: {}\n",
                sg.severity.slug().to_uppercase(),
                sg.line,
                sg.message
            ));
        }
        s.push('\n');
    }

    s
}

fn render_text(r: &MutationReport) -> String {
    let mut s = String::new();
    s.push_str(&format!("Mutation Testing Report - {}\n", r.file));
    s.push_str(&format!("Score: {:.1}%\n", r.score));
    s.push_str(&format!(
        "killed={} survived={} timeout={} build-failed={} total={}\n",
        r.killed, r.survived, r.timeout, r.build_failed, r.total
    ));
    if !r.weak_spots.is_empty() {
        s.push_str("\nWeak spots:\n");
        for w in &r.weak_spots {
            s.push_str(&format!(
                "  {} - {}/{} survived ({:.1}%)\n",
                w.function, w.survived, w.total, w.score
            ));
        }
    }
    if !r.suggestions.is_empty() {
        s.push_str("\nSuggestions:\n");
        for sg in &r.suggestions {
            s.push_str(&format!(
                "  [{}] line {}: {}\n",
                sg.severity.slug(),
                sg.line,
                sg.message
            ));
        }
    }
    s
}

fn render_html(r: &MutationReport) -> String {
    let colour = if r.score >= 80.0 {
        "#2e7d32"
    } else if r.score >= 60.0 {
        "#ef6c00"
    } else {
        "#c62828"
    };
    let mut rows = String::new();
    for w in &r.weak_spots {
        rows.push_str(&format!(
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{:.1}%</td></tr>",
            escape_html(&w.function),
            w.survived,
            w.total,
            w.score
        ));
    }
    let mut items = String::new();
    for sg in &r.suggestions {
        items.push_str(&format!(
            "<li><strong>[{}]</strong> line {}: {}</li>",
            sg.severity.slug(),
            sg.line,
            escape_html(&sg.message)
        ));
    }
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
         <title>Mutation Report - {file}</title>\
         <style>body{{font-family:system-ui,sans-serif;margin:2rem;}}\
         table{{border-collapse:collapse}}td,th{{border:1px solid #ddd;padding:.4rem .8rem}}\
         .score{{font-size:2rem;font-weight:700;color:{colour}}}</style></head><body>\
         <h1>Mutation Testing Report</h1><p><code>{file}</code></p>\
         <p class=\"score\">{score:.1}%</p>\
         <p>killed={killed} survived={survived} timeout={timeout} build-failed={bf} total={total}</p>\
         <h2>Weak spots</h2><table><tr><th>Function</th><th>Survived</th><th>Total</th><th>Score</th></tr>{rows}</table>\
         <h2>Suggestions</h2><ul>{items}</ul></body></html>",
        file = escape_html(&r.file),
        colour = colour,
        score = r.score,
        killed = r.killed,
        survived = r.survived,
        timeout = r.timeout,
        bf = r.build_failed,
        total = r.total,
        rows = rows,
        items = items,
    )
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Generate a GitHub Actions workflow that gates merges on the mutation score.
pub fn ci_workflow_yaml(source: &CiPath, min_score: f64, test_command: &str) -> String {
    format!(
        r#"name: StarForge Mutation Testing

on:
  pull_request:
  push:
    branches: [ master, main ]

jobs:
  mutation-testing:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Install StarForge
        run: cargo install --path .
      - name: Run mutation testing
        run: |
          starforge mutate run {source} \
            --test-command "{test_command}" \
            --min-score {min_score:.1} \
            --ci \
            --format markdown \
            --output mutation-report.md
      - name: Upload mutation report
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: mutation-report
          path: mutation-report.md
"#,
        source = source.as_str(),
        test_command = test_command,
        min_score = min_score,
    )
}

/// Minimal path newtype so this module stays free of `std::path` formatting
/// quirks across platforms (backslashes are normalised to `/` for CI YAML).
pub struct CiPath(String);

impl CiPath {
    pub fn new(s: &str) -> Self {
        CiPath(s.replace('\\', "/"))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ── Lexical helpers ───────────────────────────────────────────────────────────

/// Return a copy of `line` with string/char literal contents and any trailing
/// `//` comment replaced by spaces. Byte offsets are preserved so matches found
/// in the mask can index straight back into the original line.
fn mask_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_str = false;
    let mut in_char = false;
    let mut escaped = false;
    let mut chars = line.char_indices().peekable();

    while let Some((i, ch)) = chars.next() {
        // Trailing line comment (outside a string) — mask the rest.
        if !in_str && !in_char && ch == '/' {
            if let Some((_, '/')) = chars.peek() {
                for _ in i..line.len() {
                    out.push(' ');
                }
                return out;
            }
        }

        if escaped {
            escaped = false;
            push_blank(&mut out, ch);
            continue;
        }

        match ch {
            '\\' if in_str || in_char => {
                escaped = true;
                push_blank(&mut out, ch);
            }
            '"' if !in_char => {
                in_str = !in_str;
                push_blank(&mut out, ch);
            }
            '\'' if !in_str => {
                in_char = !in_char;
                push_blank(&mut out, ch);
            }
            _ => {
                if in_str || in_char {
                    push_blank(&mut out, ch);
                } else {
                    out.push(ch);
                }
            }
        }
    }
    out
}

/// Push `ch.len_utf8()` spaces so the mask keeps the original byte length.
fn push_blank(out: &mut String, ch: char) {
    for _ in 0..ch.len_utf8() {
        out.push(' ');
    }
}

/// Replace `len` bytes at `at` in `line` with `replacement`.
fn splice(line: &str, at: usize, len: usize, replacement: &str) -> String {
    let end = (at + len).min(line.len());
    if at > line.len() {
        return line.to_string();
    }
    format!("{}{}{}", &line[..at], replacement, &line[end..])
}

/// Byte offsets of `word` in `text`, matched on identifier boundaries.
fn find_words(text: &str, word: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    for (i, _) in text.match_indices(word) {
        let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
        let after = i + word.len();
        let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);
        if before_ok && after_ok {
            out.push(i);
        }
    }
    out
}

/// Find standalone integer literals: not part of an identifier (`u32`), not a
/// float (`1.5`), and not preceded by `.` (tuple access / method chains).
fn find_int_literals(text: &str) -> Vec<(usize, String)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let end = i;

        let prev_ok = start == 0 || (!is_ident_byte(bytes[start - 1]) && bytes[start - 1] != b'.');
        // Reject floats and numeric suffixes (`1.5`, `0u32`).
        let next_ok = end >= bytes.len() || (!is_ident_byte(bytes[end]) && bytes[end] != b'.');
        if prev_ok && next_ok {
            out.push((start, text[start..end].to_string()));
        }
    }
    out
}

/// Given the byte index of an opening `(`, return the index of its match.
fn matching_paren(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    let mut depth = 0i32;
    for (i, b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Extract a function name from a `fn name(` declaration line.
fn fn_name(line: &str) -> Option<String> {
    let pos = line.find("fn ")?;
    if pos > 0 {
        let prev = line.as_bytes()[pos - 1];
        if is_ident_byte(prev) {
            return None;
        }
    }
    let after = &line[pos + 3..];
    let name: String = after
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// 1-based line numbers belonging to a `#[cfg(test)]` module, which must not be
/// mutated (mutating the tests themselves proves nothing).
fn test_module_lines(source: &str) -> Vec<usize> {
    let lines: Vec<&str> = source.lines().collect();
    let mut skip = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].trim().starts_with("#[cfg(test)]") {
            // Walk forward to the module's opening brace, then to its match.
            let mut depth = 0i32;
            let mut started = false;
            let mut j = i;
            while j < lines.len() {
                for ch in mask_line(lines[j]).chars() {
                    match ch {
                        '{' => {
                            depth += 1;
                            started = true;
                        }
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                skip.push(j + 1);
                if started && depth <= 0 {
                    break;
                }
                j += 1;
            }
            i = j + 1;
            continue;
        }
        i += 1;
    }
    skip
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> MutationConfig {
        MutationConfig::default()
    }

    /// Executor that kills mutants whose line number is in `kill_lines`.
    struct FakeExecutor {
        kill_lines: Vec<usize>,
        calls: usize,
    }

    impl TestExecutor for FakeExecutor {
        fn run(&mut self, _src: &str, m: &Mutant) -> Result<MutantOutcome, String> {
            self.calls += 1;
            if self.kill_lines.contains(&m.line) {
                Ok(MutantOutcome::Killed)
            } else {
                Ok(MutantOutcome::Survived)
            }
        }
    }

    #[test]
    fn generates_comparison_mutants() {
        let src = "fn f(a: u32) -> bool {\n    a == 1\n}\n";
        let m = generate_mutants(src, "c.rs", &cfg());
        assert!(m
            .iter()
            .any(|x| x.operator == MutationOperator::Comparison && x.replacement.trim() == "!="));
    }

    #[test]
    fn generates_arithmetic_mutants() {
        let src = "fn f() -> u32 {\n    let x = 1 + 2;\n    x\n}\n";
        let m = generate_mutants(src, "c.rs", &cfg());
        assert!(m
            .iter()
            .any(|x| x.operator == MutationOperator::Arithmetic && x.replacement.trim() == "-"));
    }

    #[test]
    fn generates_boolean_and_logical_mutants() {
        let src = "fn f(a: bool, b: bool) -> bool {\n    a && b || true\n}\n";
        let m = generate_mutants(src, "c.rs", &cfg());
        assert!(m
            .iter()
            .any(|x| x.operator == MutationOperator::LogicalConnector));
        assert!(m
            .iter()
            .any(|x| x.operator == MutationOperator::BooleanLiteral && x.original == "true"));
    }

    #[test]
    fn generates_require_auth_removal() {
        let src = "fn f(admin: Address) {\n    admin.require_auth();\n}\n";
        let m = generate_mutants(src, "c.rs", &cfg());
        let ra: Vec<_> = m
            .iter()
            .filter(|x| x.operator == MutationOperator::RequireAuthRemoval)
            .collect();
        assert_eq!(ra.len(), 1);
        assert!(ra[0].mutated_line.trim().starts_with("// [mutant]"));
    }

    #[test]
    fn generates_storage_durability_mutants() {
        let src = "fn f(env: Env) {\n    env.storage().persistent().set(&k, &v);\n}\n";
        let m = generate_mutants(src, "c.rs", &cfg());
        assert!(m
            .iter()
            .any(|x| x.operator == MutationOperator::StorageDurability));
    }

    #[test]
    fn does_not_mutate_inside_strings() {
        let src = "fn f() {\n    let s = \"a == b && c\";\n}\n";
        let m = generate_mutants(src, "c.rs", &cfg());
        assert!(
            !m.iter().any(|x| x.operator == MutationOperator::Comparison
                || x.operator == MutationOperator::LogicalConnector),
            "string contents must not be mutated"
        );
    }

    #[test]
    fn does_not_mutate_comments() {
        let src = "fn f() {\n    // a == b\n    let x = 5;\n}\n";
        let m = generate_mutants(src, "c.rs", &cfg());
        assert!(!m.iter().any(|x| x.operator == MutationOperator::Comparison));
    }

    #[test]
    fn does_not_mutate_test_module() {
        let src = "\
fn f(a: u32) -> bool {
    a == 1
}
#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        assert!(1 == 1);
    }
}
";
        let m = generate_mutants(src, "c.rs", &cfg());
        assert!(
            m.iter().all(|x| x.line < 4),
            "no mutants may come from the test module, got {:?}",
            m.iter().map(|x| x.line).collect::<Vec<_>>()
        );
    }

    #[test]
    fn generics_are_not_treated_as_comparisons() {
        let src = "fn f() {\n    let v: Vec<u32> = Vec::new();\n}\n";
        let m = generate_mutants(src, "c.rs", &cfg());
        assert!(
            !m.iter().any(|x| x.operator == MutationOperator::Comparison),
            "Vec<u32> must not produce comparison mutants"
        );
    }

    #[test]
    fn tracks_enclosing_function() {
        let src = "fn alpha() {\n    let x = 1 + 1;\n}\nfn beta() {\n    let y = 2 + 2;\n}\n";
        let m = generate_mutants(src, "c.rs", &cfg());
        let alpha = m.iter().find(|x| x.line == 2).expect("mutant on line 2");
        let beta = m.iter().find(|x| x.line == 5).expect("mutant on line 5");
        assert_eq!(alpha.function.as_deref(), Some("alpha"));
        assert_eq!(beta.function.as_deref(), Some("beta"));
    }

    #[test]
    fn apply_mutant_replaces_only_target_line() {
        let src = "fn f() {\n    let x = 1 + 2;\n    let y = 3 + 4;\n}\n";
        let m = generate_mutants(src, "c.rs", &cfg());
        let target = m.iter().find(|x| x.line == 2).expect("line 2 mutant");
        let mutated = apply_mutant(src, target);
        assert!(mutated.contains("let y = 3 + 4;"), "other lines untouched");
        assert_eq!(mutated.lines().count(), src.lines().count());
    }

    #[test]
    fn mutants_are_deduplicated_and_numbered() {
        let src = "fn f() {\n    let x = 1 + 2;\n}\n";
        let m = generate_mutants(src, "c.rs", &cfg());
        for (i, mutant) in m.iter().enumerate() {
            assert_eq!(mutant.id, i + 1, "ids are sequential from 1");
        }
        let mut keys: Vec<_> = m
            .iter()
            .map(|x| (x.line, x.column, x.operator, x.replacement.clone()))
            .collect();
        let before = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(before, keys.len(), "no duplicate mutants");
    }

    #[test]
    fn max_mutants_caps_and_stays_deterministic() {
        let src = "fn f() {\n    let a = 1 + 2 + 3 + 4 + 5;\n    let b = 6 + 7 + 8 + 9;\n}\n";
        let mut c = cfg();
        c.max_mutants = Some(3);
        let first = generate_mutants(src, "c.rs", &c);
        let second = generate_mutants(src, "c.rs", &c);
        assert_eq!(first.len(), 3);
        assert_eq!(
            first.iter().map(|m| m.line).collect::<Vec<_>>(),
            second.iter().map(|m| m.line).collect::<Vec<_>>(),
            "sampling must be deterministic"
        );
    }

    #[test]
    fn operator_filter_restricts_generation() {
        let src = "fn f(a: u32) -> bool {\n    a == 1 && true\n}\n";
        let mut c = cfg();
        c.operators = vec![MutationOperator::BooleanLiteral];
        let m = generate_mutants(src, "c.rs", &c);
        assert!(!m.is_empty());
        assert!(m
            .iter()
            .all(|x| x.operator == MutationOperator::BooleanLiteral));
    }

    #[test]
    fn score_excludes_build_failures_and_counts_timeouts_as_killed() {
        let mk = |line: usize, outcome: MutantOutcome| MutantResult {
            mutant: Mutant {
                id: line,
                file: "c.rs".into(),
                line,
                column: 0,
                operator: MutationOperator::Arithmetic,
                original: "+".into(),
                replacement: "-".into(),
                original_line: String::new(),
                mutated_line: String::new(),
                function: Some("f".into()),
            },
            outcome,
            duration_ms: 1,
        };
        let results = vec![
            mk(1, MutantOutcome::Killed),
            mk(2, MutantOutcome::Timeout),
            mk(3, MutantOutcome::Survived),
            mk(4, MutantOutcome::BuildFailed),
        ];
        let r = analyze("c.rs", results, 10);
        // valid = 3 (killed, timeout, survived); detected = 2
        assert!((r.score - 66.666).abs() < 0.01, "score was {}", r.score);
        assert_eq!(r.build_failed, 1);
    }

    #[test]
    fn perfect_suite_scores_100() {
        let src = "fn f(a: u32) -> bool {\n    a == 1\n}\n";
        let mut ex = FakeExecutor {
            kill_lines: vec![2],
            calls: 0,
        };
        let r = run_mutation_testing(src, "c.rs", &cfg(), &mut ex).expect("run");
        assert_eq!(r.survived, 0);
        assert!((r.score - 100.0).abs() < f64::EPSILON);
        assert!(r.weak_spots.is_empty());
        assert!(r.suggestions.is_empty());
        assert!(ex.calls > 0, "executor was actually invoked");
    }

    #[test]
    fn survivors_produce_weak_spots_and_suggestions() {
        let src = "fn f(a: u32) -> bool {\n    a == 1\n}\n";
        let mut ex = FakeExecutor {
            kill_lines: vec![],
            calls: 0,
        };
        let r = run_mutation_testing(src, "c.rs", &cfg(), &mut ex).expect("run");
        assert!(r.survived > 0);
        assert!(r.score < 100.0);
        assert_eq!(r.weak_spots.len(), 1);
        assert_eq!(r.weak_spots[0].function, "f");
        assert!(!r.suggestions.is_empty());
    }

    #[test]
    fn require_auth_survivor_is_high_severity() {
        let src = "fn f(admin: Address) {\n    admin.require_auth();\n}\n";
        let mut c = cfg();
        c.operators = vec![MutationOperator::RequireAuthRemoval];
        let mut ex = FakeExecutor {
            kill_lines: vec![],
            calls: 0,
        };
        let r = run_mutation_testing(src, "c.rs", &c, &mut ex).expect("run");
        assert_eq!(r.suggestions.len(), 1);
        assert_eq!(r.suggestions[0].severity, Severity::High);
        assert!(r.suggestions[0].message.contains("unauthorised"));
    }

    #[test]
    fn threshold_gating() {
        let src = "fn f(a: u32) -> bool {\n    a == 1\n}\n";
        let mut ex = FakeExecutor {
            kill_lines: vec![2],
            calls: 0,
        };
        let r = run_mutation_testing(src, "c.rs", &cfg(), &mut ex).expect("run");
        assert!(r.meets_threshold(100.0));
        assert!(r.meets_threshold(80.0));

        let mut ex2 = FakeExecutor {
            kill_lines: vec![],
            calls: 0,
        };
        let r2 = run_mutation_testing(src, "c.rs", &cfg(), &mut ex2).expect("run");
        assert!(!r2.meets_threshold(50.0));
    }

    #[test]
    fn render_formats_round_trip() {
        let src = "fn f(a: u32) -> bool {\n    a == 1\n}\n";
        let mut ex = FakeExecutor {
            kill_lines: vec![],
            calls: 0,
        };
        let r = run_mutation_testing(src, "c.rs", &cfg(), &mut ex).expect("run");

        let md = render_report(&r, "markdown").expect("markdown");
        assert!(md.contains("# Mutation Testing Report"));
        assert!(md.contains("Weak spots"));

        let txt = render_report(&r, "text").expect("text");
        assert!(txt.contains("Mutation Testing Report"));

        let html = render_report(&r, "html").expect("html");
        assert!(html.contains("<!DOCTYPE html>"));

        assert!(render_report(&r, "bogus").is_err());
    }

    #[test]
    fn html_escapes_content() {
        assert_eq!(escape_html("a<b>&c"), "a&lt;b&gt;&amp;c");
    }

    #[test]
    fn mask_line_preserves_byte_length() {
        for line in [
            "let s = \"a == b\";",
            "let c = 'x';",
            "let n = 1; // a == b",
            "let e = \"esc\\\"aped\";",
            "let u = \"héllo == x\";",
        ] {
            assert_eq!(
                mask_line(line).len(),
                line.len(),
                "mask must preserve byte length for: {}",
                line
            );
        }
    }

    #[test]
    fn find_int_literals_skips_suffixed_and_floats() {
        let found = find_int_literals("let a = 5; let b: u32 = 1; let c = 1.5;");
        let texts: Vec<&str> = found.iter().map(|(_, t)| t.as_str()).collect();
        assert!(texts.contains(&"5"));
        // `u32` digits and float parts must not be picked up.
        assert!(!texts.contains(&"32"));
        assert!(!texts.contains(&"5.5"));
    }

    #[test]
    fn ci_workflow_contains_gate() {
        let yaml = ci_workflow_yaml(&CiPath::new("src\\lib.rs"), 75.0, "cargo test");
        assert!(yaml.contains("starforge mutate run src/lib.rs"));
        assert!(yaml.contains("--min-score 75.0"));
        assert!(yaml.contains("--ci"));
        assert!(yaml.contains("upload-artifact"));
    }

    #[test]
    fn operator_slugs_parse_round_trip() {
        for op in MutationOperator::all() {
            assert_eq!(MutationOperator::parse(op.slug()), Some(*op));
        }
        assert_eq!(MutationOperator::parse("nope"), None);
    }
}
