//! Contract fuzzing and property-based testing engine.
//!
//! This module provides a self-contained, deterministic fuzzing toolkit:
//!
//! * **Typed input generation** — produce random `ScVal`-style values
//!   (`u32`, `u64`, `i128`, `bool`, `Symbol`, `Address`, `Bytes`, `String`)
//!   from a seeded RNG so runs are reproducible.
//! * **Property-based testing** — run an invariant against many generated
//!   inputs and, on failure, **shrink** the counterexample toward a minimal
//!   reproducer (the core idea behind `proptest`/`quickcheck`).
//! * **WASM validator fuzzing** — byte-mutate a contract binary and assert the
//!   validator rejects malformed input without panicking.
//! * **Mutation testing** — generate source mutants (flip operators, drop
//!   `require_auth`, etc.) and score how many a test oracle "kills".
//! * **Corpus persistence** — store interesting / crashing inputs under
//!   `~/.starforge/fuzz/` for replay.
//!
//! The engine is intentionally sync and uses only `rand`, matching the rest of
//! the codebase. The integration test-suite additionally demonstrates
//! `proptest`-driven property tests for contract logic.

use crate::utils::config;
use anyhow::{Context, Result};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Typed values
// ---------------------------------------------------------------------------

/// The Soroban-flavoured value types the generator understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FuzzType {
    U32,
    U64,
    I128,
    Bool,
    Symbol,
    Address,
    Bytes,
    String,
}

impl FuzzType {
    /// Parse a type name as it would appear in a contract signature.
    pub fn parse(s: &str) -> Option<FuzzType> {
        match s.trim().to_lowercase().as_str() {
            "u32" => Some(FuzzType::U32),
            "u64" => Some(FuzzType::U64),
            "i128" | "int" => Some(FuzzType::I128),
            "bool" => Some(FuzzType::Bool),
            "symbol" => Some(FuzzType::Symbol),
            "address" => Some(FuzzType::Address),
            "bytes" => Some(FuzzType::Bytes),
            "string" => Some(FuzzType::String),
            _ => None,
        }
    }
}

/// A concrete generated value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum FuzzValue {
    U32(u32),
    U64(u64),
    I128(i128),
    Bool(bool),
    Symbol(String),
    Address(String),
    Bytes(Vec<u8>),
    String(String),
}

impl fmt::Display for FuzzValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FuzzValue::U32(v) => write!(f, "{}u32", v),
            FuzzValue::U64(v) => write!(f, "{}u64", v),
            FuzzValue::I128(v) => write!(f, "{}i128", v),
            FuzzValue::Bool(v) => write!(f, "{}", v),
            FuzzValue::Symbol(v) => write!(f, "sym:{}", v),
            FuzzValue::Address(v) => write!(f, "{}", v),
            FuzzValue::Bytes(v) => write!(f, "bytes[{}]", v.len()),
            FuzzValue::String(v) => write!(f, "\"{}\"", v),
        }
    }
}

/// A named, typed parameter in a contract function signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgSpec {
    pub name: String,
    pub ty: FuzzType,
}

/// Generate a single random value of the requested type.
pub fn generate_value(ty: FuzzType, rng: &mut StdRng) -> FuzzValue {
    match ty {
        FuzzType::U32 => FuzzValue::U32(biased_u32(rng)),
        FuzzType::U64 => FuzzValue::U64(rng.gen()),
        FuzzType::I128 => FuzzValue::I128(rng.gen::<i64>() as i128),
        FuzzType::Bool => FuzzValue::Bool(rng.gen_bool(0.5)),
        FuzzType::Symbol => FuzzValue::Symbol(random_symbol(rng)),
        FuzzType::Address => FuzzValue::Address(random_address(rng)),
        FuzzType::Bytes => {
            let len = rng.gen_range(0..32);
            FuzzValue::Bytes((0..len).map(|_| rng.gen()).collect())
        }
        FuzzType::String => FuzzValue::String(random_string(rng)),
    }
}

/// Generate a full input tuple for a function signature.
pub fn generate_input(specs: &[ArgSpec], rng: &mut StdRng) -> Vec<FuzzValue> {
    specs.iter().map(|s| generate_value(s.ty, rng)).collect()
}

/// Bias integer generation toward boundary values (0, 1, MAX) which are the
/// classic edge cases that trip up contract arithmetic.
fn biased_u32(rng: &mut StdRng) -> u32 {
    match rng.gen_range(0..10) {
        0 => 0,
        1 => 1,
        2 => u32::MAX,
        3 => u32::MAX - 1,
        _ => rng.gen(),
    }
}

fn random_symbol(rng: &mut StdRng) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz_";
    let len = rng.gen_range(1..=10);
    (0..len)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

fn random_address(rng: &mut StdRng) -> String {
    const BASE32: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let prefix = if rng.gen_bool(0.5) { 'G' } else { 'C' };
    let body: String = (0..55)
        .map(|_| BASE32[rng.gen_range(0..BASE32.len())] as char)
        .collect();
    format!("{}{}", prefix, body)
}

fn random_string(rng: &mut StdRng) -> String {
    let len = rng.gen_range(0..16);
    (0..len)
        .map(|_| rng.gen_range(b' '..=b'~') as char)
        .collect()
}

// ---------------------------------------------------------------------------
// Property-based testing with shrinking
// ---------------------------------------------------------------------------

/// Outcome of evaluating a property against one input.
pub type PropertyResult = std::result::Result<(), String>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuzzConfig {
    pub iterations: usize,
    pub seed: u64,
    pub max_shrink_iters: usize,
}

impl Default for FuzzConfig {
    fn default() -> Self {
        Self {
            iterations: 1000,
            seed: 0xDEADBEEF,
            max_shrink_iters: 200,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuzzReport {
    pub iterations_run: usize,
    pub passed: bool,
    pub failure_message: Option<String>,
    pub counterexample: Option<Vec<FuzzValue>>,
    pub shrunk_counterexample: Option<Vec<FuzzValue>>,
    pub shrink_steps: usize,
    pub seed: u64,
}

/// Run `property` against `iterations` generated inputs. On the first failure
/// the offending input is shrunk toward a minimal reproducer.
pub fn run_property<F>(specs: &[ArgSpec], config: &FuzzConfig, property: F) -> FuzzReport
where
    F: Fn(&[FuzzValue]) -> PropertyResult,
{
    let mut rng = StdRng::seed_from_u64(config.seed);

    for i in 0..config.iterations {
        let input = generate_input(specs, &mut rng);
        if let Err(msg) = property(&input) {
            let (shrunk, steps) = shrink(&input, config.max_shrink_iters, &property);
            return FuzzReport {
                iterations_run: i + 1,
                passed: false,
                failure_message: Some(msg),
                counterexample: Some(input),
                shrunk_counterexample: Some(shrunk),
                shrink_steps: steps,
                seed: config.seed,
            };
        }
    }

    FuzzReport {
        iterations_run: config.iterations,
        passed: true,
        failure_message: None,
        counterexample: None,
        shrunk_counterexample: None,
        shrink_steps: 0,
        seed: config.seed,
    }
}

/// Greedily shrink a failing input: repeatedly try "smaller" variants of each
/// value and keep any that still fail the property.
fn shrink<F>(input: &[FuzzValue], max_iters: usize, property: &F) -> (Vec<FuzzValue>, usize)
where
    F: Fn(&[FuzzValue]) -> PropertyResult,
{
    let mut best = input.to_vec();
    let mut steps = 0;
    let mut improved = true;

    while improved && steps < max_iters {
        improved = false;
        for idx in 0..best.len() {
            for candidate_value in shrink_candidates(&best[idx]) {
                if steps >= max_iters {
                    break;
                }
                steps += 1;
                let mut candidate = best.clone();
                candidate[idx] = candidate_value;
                if property(&candidate).is_err() {
                    best = candidate;
                    improved = true;
                    break;
                }
            }
        }
    }
    (best, steps)
}

/// Produce a few "simpler" variants of a value (toward zero / empty).
fn shrink_candidates(value: &FuzzValue) -> Vec<FuzzValue> {
    match value {
        FuzzValue::U32(v) if *v > 0 => vec![FuzzValue::U32(0), FuzzValue::U32(v / 2)],
        FuzzValue::U64(v) if *v > 0 => vec![FuzzValue::U64(0), FuzzValue::U64(v / 2)],
        FuzzValue::I128(v) if *v != 0 => vec![FuzzValue::I128(0), FuzzValue::I128(v / 2)],
        FuzzValue::Bool(true) => vec![FuzzValue::Bool(false)],
        FuzzValue::Bytes(b) if !b.is_empty() => {
            vec![
                FuzzValue::Bytes(vec![]),
                FuzzValue::Bytes(b[..b.len() / 2].to_vec()),
            ]
        }
        FuzzValue::String(s) if !s.is_empty() => {
            vec![
                FuzzValue::String(String::new()),
                FuzzValue::String(s[..s.len() / 2].to_string()),
            ]
        }
        FuzzValue::Symbol(s) if s.len() > 1 => vec![FuzzValue::Symbol(s[..1].to_string())],
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// WASM validator fuzzing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmFuzzReport {
    pub iterations: usize,
    pub seed: u64,
    pub rejected: usize,
    pub accepted: usize,
    pub crashes: usize,
    /// Inputs that made the validator panic (caught), saved for replay.
    pub crash_inputs: Vec<String>,
}

/// Fuzz a contract WASM binary by applying random byte mutations and ensuring
/// the validator handles every variant gracefully (no panic). Mutants that the
/// validator panics on are recorded as crashes.
pub fn fuzz_wasm_validator(seed_bytes: &[u8], config: &FuzzConfig) -> WasmFuzzReport {
    let mut rng = StdRng::seed_from_u64(config.seed);
    let (mut rejected, mut accepted, mut crashes) = (0, 0, 0);
    let mut crash_inputs = Vec::new();

    for _ in 0..config.iterations {
        let mutated = mutate_bytes(seed_bytes, &mut rng);
        // The validator must never panic, even on garbage input.
        let result =
            std::panic::catch_unwind(|| crate::utils::mock_soroban::validate_wasm(&mutated));
        match result {
            Ok(Ok(())) => accepted += 1,
            Ok(Err(_)) => rejected += 1,
            Err(_) => {
                crashes += 1;
                if crash_inputs.len() < 16 {
                    crash_inputs.push(hex::encode(&mutated[..mutated.len().min(32)]));
                }
            }
        }
    }

    WasmFuzzReport {
        iterations: config.iterations,
        seed: config.seed,
        rejected,
        accepted,
        crashes,
        crash_inputs,
    }
}

fn mutate_bytes(input: &[u8], rng: &mut StdRng) -> Vec<u8> {
    if input.is_empty() {
        return (0..rng.gen_range(0..16)).map(|_| rng.gen()).collect();
    }
    let mut out = input.to_vec();
    let mutations = rng.gen_range(1..=8);
    for _ in 0..mutations {
        match rng.gen_range(0..4) {
            0 => {
                // flip a random byte
                let i = rng.gen_range(0..out.len());
                out[i] ^= 1 << rng.gen_range(0..8);
            }
            1 if out.len() > 1 => {
                // truncate
                let i = rng.gen_range(1..out.len());
                out.truncate(i);
            }
            2 => {
                // append junk
                out.push(rng.gen());
            }
            _ => {
                // overwrite a byte
                let i = rng.gen_range(0..out.len());
                out[i] = rng.gen();
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Mutation testing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mutant {
    pub id: usize,
    pub line: usize,
    pub category: String,
    pub description: String,
    pub original: String,
    pub mutated: String,
}

/// Generate source-level mutants for a contract. Each mutant flips one
/// operator or removes a security-relevant call on a single line.
pub fn generate_mutants(source: &str) -> Vec<Mutant> {
    // (needle, replacement, category) — applied to the first occurrence per line.
    let rules: &[(&str, &str, &str)] = &[
        (" + ", " - ", "arithmetic"),
        (" - ", " + ", "arithmetic"),
        (" * ", " / ", "arithmetic"),
        (" == ", " != ", "comparison"),
        (" != ", " == ", "comparison"),
        (" < ", " <= ", "boundary"),
        (" > ", " >= ", "boundary"),
        (" && ", " || ", "logical"),
        (" || ", " && ", "logical"),
        ("true", "false", "constant"),
        ("require_auth", "/* require_auth removed */", "security"),
    ];

    let mut mutants = Vec::new();
    let mut id = 0;
    for (lineno, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        // Skip comments to avoid generating meaningless mutants.
        if trimmed.starts_with("//") {
            continue;
        }
        for (needle, replacement, category) in rules {
            if let Some(pos) = line.find(needle) {
                let mut mutated_line = String::with_capacity(line.len());
                mutated_line.push_str(&line[..pos]);
                mutated_line.push_str(replacement);
                mutated_line.push_str(&line[pos + needle.len()..]);
                mutants.push(Mutant {
                    id,
                    line: lineno + 1,
                    category: category.to_string(),
                    description: format!(
                        "replace `{}` with `{}`",
                        needle.trim(),
                        replacement.trim()
                    ),
                    original: line.trim().to_string(),
                    mutated: mutated_line.trim().to_string(),
                });
                id += 1;
            }
        }
    }
    mutants
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationScore {
    pub total: usize,
    pub killed: usize,
    pub survived: usize,
    pub score_pct: f64,
    pub survivors: Vec<Mutant>,
}

/// Evaluate mutants against an oracle. The oracle returns `true` when the
/// mutant is "killed" (i.e. a test would catch the change). This decouples the
/// generic scoring logic from any particular test harness so it is unit-testable.
pub fn score_mutants<F>(mutants: &[Mutant], oracle: F) -> MutationScore
where
    F: Fn(&Mutant) -> bool,
{
    let mut killed = 0;
    let mut survivors = Vec::new();
    for m in mutants {
        if oracle(m) {
            killed += 1;
        } else {
            survivors.push(m.clone());
        }
    }
    let total = mutants.len();
    let score_pct = if total == 0 {
        100.0
    } else {
        (killed as f64 / total as f64) * 100.0
    };
    MutationScore {
        total,
        killed,
        survived: survivors.len(),
        score_pct,
        survivors,
    }
}

/// A heuristic oracle for offline mutation analysis: security and arithmetic
/// mutations are assumed to be caught by a well-written suite, while constant
/// and boundary flips frequently survive and signal coverage gaps.
pub fn heuristic_oracle(mutant: &Mutant) -> bool {
    matches!(
        mutant.category.as_str(),
        "security" | "arithmetic" | "logical"
    )
}

// ---------------------------------------------------------------------------
// Corpus persistence (~/.starforge/fuzz/)
// ---------------------------------------------------------------------------

fn fuzz_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("fuzz");
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    }
    Ok(dir)
}

/// Persist a fuzzing corpus (a set of inputs) under `~/.starforge/fuzz/<name>.json`.
pub fn save_corpus(name: &str, inputs: &[Vec<FuzzValue>]) -> Result<PathBuf> {
    let path = fuzz_dir()?.join(format!("{}.json", sanitize(name)));
    fs::write(&path, serde_json::to_string_pretty(inputs)?)?;
    Ok(path)
}

/// Load a previously saved corpus.
pub fn load_corpus(name: &str) -> Result<Vec<Vec<FuzzValue>>> {
    let path = fuzz_dir()?.join(format!("{}.json", sanitize(name)));
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&contents)?)
}

/// List available corpus names.
pub fn list_corpora() -> Result<Vec<String>> {
    let dir = fuzz_dir()?;
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Built-in property suite (token transfer model)
// ---------------------------------------------------------------------------

/// A minimal token model used to demonstrate property-based contract testing.
/// Real contracts would be invoked instead, but the invariants are identical.
#[derive(Debug, Clone)]
pub struct TokenModel {
    pub from_balance: u64,
    pub to_balance: u64,
}

impl TokenModel {
    /// Attempt a transfer, returning the new model or an error string. The
    /// implementation deliberately mirrors a correct contract: it rejects
    /// overflows and insufficient balances.
    pub fn transfer(&self, amount: u64) -> std::result::Result<TokenModel, String> {
        if amount > self.from_balance {
            return Err("insufficient balance".to_string());
        }
        let to_balance = self
            .to_balance
            .checked_add(amount)
            .ok_or("recipient balance overflow")?;
        Ok(TokenModel {
            from_balance: self.from_balance - amount,
            to_balance,
        })
    }

    pub fn total(&self) -> u128 {
        self.from_balance as u128 + self.to_balance as u128
    }
}

/// Run the built-in property suite: total supply is conserved across transfers
/// and balances never underflow. Returns a `FuzzReport`.
pub fn run_token_property_suite(config: &FuzzConfig) -> FuzzReport {
    let specs = vec![
        ArgSpec {
            name: "from".into(),
            ty: FuzzType::U32,
        },
        ArgSpec {
            name: "to".into(),
            ty: FuzzType::U32,
        },
        ArgSpec {
            name: "amount".into(),
            ty: FuzzType::U32,
        },
    ];

    run_property(&specs, config, |input| {
        let (from, to, amount) = match (&input[0], &input[1], &input[2]) {
            (FuzzValue::U32(a), FuzzValue::U32(b), FuzzValue::U32(c)) => {
                (*a as u64, *b as u64, *c as u64)
            }
            _ => return Err("unexpected input shape".to_string()),
        };
        let model = TokenModel {
            from_balance: from,
            to_balance: to,
        };
        let before = model.total();
        match model.transfer(amount) {
            Ok(after) => {
                if after.total() != before {
                    Err(format!(
                        "supply not conserved: {} -> {}",
                        before,
                        after.total()
                    ))
                } else {
                    Ok(())
                }
            }
            // A rejected transfer must leave supply untouched, which it does
            // because the model is immutable. Rejection is acceptable.
            Err(_) => Ok(()),
        }
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic_for_a_seed() {
        let specs = vec![ArgSpec {
            name: "x".into(),
            ty: FuzzType::U32,
        }];
        let cfg = FuzzConfig {
            seed: 7,
            ..FuzzConfig::default()
        };
        let mut a = StdRng::seed_from_u64(cfg.seed);
        let mut b = StdRng::seed_from_u64(cfg.seed);
        assert_eq!(
            generate_input(&specs, &mut a),
            generate_input(&specs, &mut b)
        );
    }

    #[test]
    fn property_passes_for_true_invariant() {
        let specs = vec![ArgSpec {
            name: "x".into(),
            ty: FuzzType::U32,
        }];
        let report = run_property(&specs, &FuzzConfig::default(), |_| Ok(()));
        assert!(report.passed);
    }

    #[test]
    fn property_finds_and_shrinks_counterexample() {
        let specs = vec![ArgSpec {
            name: "x".into(),
            ty: FuzzType::U32,
        }];
        // Invariant "x < 1000" is false for large x; shrink should drive the
        // counterexample down toward the boundary.
        let report = run_property(&specs, &FuzzConfig::default(), |input| {
            if let FuzzValue::U32(v) = input[0] {
                if v >= 1000 {
                    return Err(format!("{} >= 1000", v));
                }
            }
            Ok(())
        });
        assert!(!report.passed);
        let shrunk = report.shrunk_counterexample.unwrap();
        if let FuzzValue::U32(v) = shrunk[0] {
            let original = match report.counterexample.unwrap()[0] {
                FuzzValue::U32(o) => o,
                _ => unreachable!(),
            };
            assert!(v <= original);
            assert!(v >= 1000); // still violates
        } else {
            panic!("wrong type");
        }
    }

    #[test]
    fn token_property_suite_holds() {
        let report = run_token_property_suite(&FuzzConfig {
            iterations: 500,
            ..FuzzConfig::default()
        });
        assert!(report.passed, "{:?}", report.failure_message);
    }

    #[test]
    fn wasm_validator_never_panics_under_fuzzing() {
        let seed = b"\0asm\x01\x00\x00\x00".to_vec();
        let report = fuzz_wasm_validator(
            &seed,
            &FuzzConfig {
                iterations: 500,
                ..FuzzConfig::default()
            },
        );
        assert_eq!(
            report.crashes, 0,
            "validator panicked on {:?}",
            report.crash_inputs
        );
        assert_eq!(report.accepted + report.rejected, report.iterations);
    }

    #[test]
    fn mutants_are_generated_for_operators() {
        let src =
            "pub fn add(a: u32, b: u32) -> u32 { a + b }\nfn check() { admin.require_auth(); }";
        let mutants = generate_mutants(src);
        assert!(mutants.iter().any(|m| m.category == "arithmetic"));
        assert!(mutants.iter().any(|m| m.category == "security"));
    }

    #[test]
    fn mutation_scoring_counts_survivors() {
        let mutants = vec![
            Mutant {
                id: 0,
                line: 1,
                category: "security".into(),
                description: String::new(),
                original: String::new(),
                mutated: String::new(),
            },
            Mutant {
                id: 1,
                line: 2,
                category: "constant".into(),
                description: String::new(),
                original: String::new(),
                mutated: String::new(),
            },
        ];
        let score = score_mutants(&mutants, heuristic_oracle);
        assert_eq!(score.total, 2);
        assert_eq!(score.killed, 1);
        assert_eq!(score.survived, 1);
    }

    #[test]
    fn comments_are_not_mutated() {
        let src = "// a == b should not mutate";
        assert!(generate_mutants(src).is_empty());
    }
}
