use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

// ─── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestGenerationRequest {
    pub source_path: PathBuf,
    pub test_type: TestType,
    pub contract_name: String,
    pub contract_code: String,
    #[serde(default)]
    pub existing_tests: Option<String>,
    #[serde(default)]
    pub coverage_data: Option<CoverageInput>,
    #[serde(default)]
    pub focus_functions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TestType {
    Unit,
    Integration,
    EdgeCase,
    Security,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageInput {
    pub functions_total: u32,
    pub functions_covered: u32,
    pub lines_total: u32,
    pub lines_covered: u32,
    pub branches_total: u32,
    pub branches_covered: u32,
    pub uncovered_functions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestGenerationResponse {
    pub tests: Vec<GeneratedTest>,
    pub summary: String,
    pub estimated_coverage_improvement: f64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedTest {
    pub name: String,
    pub test_type: TestType,
    pub function_under_test: String,
    pub description: String,
    pub code: String,
    pub priority: TestPriority,
    pub edge_cases_covered: Vec<String>,
    pub security_checks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TestPriority {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestOptimizationRequest {
    pub test_code: String,
    pub contract_code: String,
    pub optimization_goals: Vec<OptimizationGoal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationGoal {
    ReduceDuplication,
    ImprovePerformance,
    IncreaseCoverage,
    BetterAssertions,
    SimplifySetup,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestOptimizationResponse {
    pub optimized_code: String,
    pub improvements: Vec<Optimization>,
    pub score_before: f64,
    pub score_after: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Optimization {
    pub category: String,
    pub description: String,
    pub impact: String,
    pub lines_changed: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageAnalysisRequest {
    pub source_code: String,
    pub test_code: String,
    pub coverage_data: CoverageInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageAnalysisResponse {
    pub current_score: f64,
    pub target_score: f64,
    pub suggestions: Vec<CoverageSuggestion>,
    pub estimated_improvement: f64,
    pub priority_areas: Vec<PriorityArea>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageSuggestion {
    pub function: String,
    pub suggestion_type: SuggestionType,
    pub description: String,
    pub estimated_lines_covered: u32,
    pub difficulty: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionType {
    AddUnitTest,
    AddEdgeCaseTest,
    AddIntegrationTest,
    AddSecurityTest,
    RefactorForTestability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityArea {
    pub name: String,
    pub current_coverage: f64,
    pub impact: String,
    pub suggestion_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestMaintenanceRequest {
    pub source_code: String,
    pub test_code: String,
    pub source_path: PathBuf,
    pub test_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestMaintenanceResponse {
    pub outdated_tests: Vec<OutdatedTest>,
    pub broken_tests: Vec<BrokenTest>,
    pub missing_tests: Vec<MissingTest>,
    pub maintenance_score: f64,
    pub recommendations: Vec<MaintenanceRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutdatedTest {
    pub test_name: String,
    pub reason: String,
    pub severity: String,
    pub suggested_fix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokenTest {
    pub test_name: String,
    pub error_message: String,
    pub line_number: Option<u32>,
    pub suggested_fix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingTest {
    pub function_name: String,
    pub test_type: String,
    pub reason: String,
    pub priority: TestPriority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceRecommendation {
    pub category: String,
    pub description: String,
    pub priority: TestPriority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockGenerationRequest {
    pub contract_code: String,
    pub mock_types: Vec<MockType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MockType {
    Address,
    Storage,
    Contract,
    Env,
    Events,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockGenerationResponse {
    pub mocks: Vec<GeneratedMock>,
    pub setup_code: String,
    pub usage_examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedMock {
    pub name: String,
    pub mock_type: MockType,
    pub code: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestDataGenerationRequest {
    pub contract_code: String,
    pub data_types: Vec<DataType>,
    pub count_per_type: u32,
    pub constraints: Vec<DataConstraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DataType {
    Address,
    Amount,
    String,
    Bytes,
    Timestamp,
    Boolean,
    Custom(String),
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataConstraint {
    pub field: String,
    pub constraint_type: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestDataGenerationResponse {
    pub test_data: Vec<TestDataItem>,
    pub setup_code: String,
    pub generators: Vec<DataGenerator>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestDataItem {
    pub name: String,
    pub data_type: DataType,
    pub value: String,
    pub description: String,
    pub valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataGenerator {
    pub name: String,
    pub return_type: String,
    pub code: String,
    pub description: String,
}

// ─── Analysis functions ────────────────────────────────────────────────────────

pub fn analyze_contract_for_testing(source: &str) -> Result<ContractAnalysis> {
    let functions = extract_functions_with_signatures(source);
    let mut analysis = ContractAnalysis {
        total_functions: functions.len() as u32,
        public_functions: 0,
        entry_points: 0,
        mutating_functions: 0,
        read_only_functions: 0,
        complex_functions: 0,
        functions: Vec::new(),
        storage_accesses: Vec::new(),
        external_calls: Vec::new(),
    };

    for func in &functions {
        if func.is_public {
            analysis.public_functions += 1;
        }
        if func.is_entry_point {
            analysis.entry_points += 1;
        }
        if func.is_mutating {
            analysis.mutating_functions += 1;
        } else {
            analysis.read_only_functions += 1;
        }
        if func.complexity_score > 5 {
            analysis.complex_functions += 1;
        }
        analysis.functions.push(func.clone());
    }

    analysis.storage_accesses = extract_storage_accesses(source);
    analysis.external_calls = extract_external_calls(source);

    Ok(analysis)
}

/// Edge cases worth covering for one function, derived from its parameters
/// and whether it mutates state.
pub fn generate_edge_case_descriptions(func: &FunctionInfo) -> Vec<String> {
    let mut cases = Vec::new();

    for param in &func.params {
        let name = &param.name;
        let ty = param.param_type.as_str();
        if ty.contains("Address") {
            cases.push(format!("Zero address passed as '{}'", name));
            cases.push(format!("Self-referencing address passed as '{}'", name));
        } else if ty.contains("u64")
            || ty.contains("i64")
            || ty.contains("u32")
            || ty.contains("i32")
        {
            cases.push(format!("Zero (0) passed as '{}'", name));
            cases.push(format!("Maximum value for the type of '{}'", name));
        } else if ty.contains("String") || ty.contains("string") {
            cases.push(format!("Empty string passed as '{}'", name));
            cases.push(format!("Maximum length string passed as '{}'", name));
        } else if ty.contains("Vec") || ty.contains("Map") {
            cases.push(format!("Empty collection passed as '{}'", name));
        }
    }

    if func.is_public || func.is_entry_point {
        cases.push(format!("Unauthorized caller invoking '{}'", func.name));
    }
    if func.is_mutating {
        cases.push(format!(
            "Repeated invocation of '{}' (idempotency)",
            func.name
        ));
    }

    cases
}

/// Security properties a generated test suite should assert for one function.
pub fn generate_security_checks(func: &FunctionInfo) -> Vec<String> {
    let mut checks = Vec::new();

    if func.is_mutating || func.is_entry_point {
        checks.push(format!(
            "Authorization: '{}' calls require_auth() before mutating state",
            func.name
        ));
    }

    for param in &func.params {
        let ty = param.param_type.as_str();
        if ty.contains("u64") || ty.contains("i64") || ty.contains("u32") || ty.contains("i32") {
            checks.push(format!(
                "Overflow: arithmetic on '{}' is checked rather than wrapping",
                param.name
            ));
        }
        if ty.contains("Address") {
            checks.push(format!(
                "Address validation: '{}' is rejected when it is not an expected participant",
                param.name
            ));
        }
    }

    if func.is_mutating {
        checks.push(format!(
            "State consistency: a failing '{}' leaves storage unchanged",
            func.name
        ));
    }

    checks
}

/// Risks in a contract that a test suite alone cannot resolve, surfaced
/// alongside generated tests.
pub fn generate_warnings(analysis: &ContractAnalysis) -> Vec<String> {
    let mut warnings = Vec::new();

    if analysis.total_functions == 0 {
        warnings.push(
            "No functions were found in the contract; nothing could be generated.".to_string(),
        );
        return warnings;
    }

    if analysis.complex_functions > 0 {
        warnings.push(format!(
            "{} function(s) are highly branched; generated tests are unlikely to cover every path.",
            analysis.complex_functions
        ));
    }
    if analysis.public_functions > 5 {
        warnings.push(format!(
            "{} public functions form a large external surface; review authorization on each.",
            analysis.public_functions
        ));
    }
    if analysis.storage_accesses.len() > 3 {
        warnings.push(format!(
            "{} storage access(es) detected; assert persisted state as well as return values.",
            analysis.storage_accesses.len()
        ));
    }
    if !analysis.external_calls.is_empty() {
        warnings.push(format!(
            "{} external call(s) detected; these need mocks to be tested deterministically.",
            analysis.external_calls.len()
        ));
    }
    if analysis.mutating_functions > 0 && analysis.entry_points == 0 {
        warnings.push(
            "State-mutating functions were found but no contract entry points; \
             confirm the contract is annotated with #[contractimpl]."
                .to_string(),
        );
    }

    warnings
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractAnalysis {
    pub total_functions: u32,
    pub public_functions: u32,
    pub entry_points: u32,
    pub mutating_functions: u32,
    pub read_only_functions: u32,
    pub complex_functions: u32,
    pub functions: Vec<FunctionInfo>,
    pub storage_accesses: Vec<String>,
    pub external_calls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub name: String,
    pub signature: String,
    pub is_public: bool,
    pub is_entry_point: bool,
    pub is_mutating: bool,
    pub params: Vec<ParamInfo>,
    pub return_type: Option<String>,
    pub complexity_score: u32,
    pub line_start: u32,
    pub line_end: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamInfo {
    pub name: String,
    pub param_type: String,
    pub is_ref: bool,
    pub is_mut: bool,
}

fn extract_functions_with_signatures(source: &str) -> Vec<FunctionInfo> {
    let mut functions = Vec::new();
    let mut in_function = false;
    let mut brace_depth = 0u32;
    let mut body_lines: Vec<&str> = Vec::new();

    for (current_line, line) in (1u32..).zip(source.lines()) {
        let trimmed = line.trim();

        if !in_function {
            if let Some(mut func) = parse_function_line(trimmed, current_line) {
                let open_braces = trimmed.matches('{').count();
                let close_braces = trimmed.matches('}').count();
                if open_braces > 0 && open_braces == close_braces {
                    if body_mutates_state(trimmed) {
                        func.is_mutating = true;
                    }
                    func.complexity_score = calculate_complexity(trimmed);
                    functions.push(func);
                } else {
                    in_function = true;
                    brace_depth = open_braces as u32;
                    brace_depth = brace_depth.saturating_sub(close_braces as u32);
                    body_lines.clear();
                    body_lines.push(trimmed);
                    functions.push(func);
                }
            }
        } else {
            body_lines.push(trimmed);
            brace_depth += trimmed.matches('{').count() as u32;
            brace_depth = brace_depth.saturating_sub(trimmed.matches('}').count() as u32);
            if brace_depth == 0 {
                if let Some(last) = functions.last_mut() {
                    last.line_end = current_line;
                    let body = body_lines.join("\n");
                    if body_mutates_state(&body) {
                        last.is_mutating = true;
                    }
                    last.complexity_score = calculate_complexity(&body);
                }
                in_function = false;
            }
        }
    }
    functions
}

/// Detects state mutation in a function body: the classic `&mut self`
/// pattern, Soroban's storage-write pattern (`env.storage()....set/remove/
/// bump/extend_ttl(...)`), or a `require_auth()` call — Soroban view
/// functions don't authorize callers, so requiring auth implies the function
/// changes state even when the write itself is elsewhere (e.g. a helper).
fn body_mutates_state(body: &str) -> bool {
    body.contains("mut self")
        || body.contains("require_auth(")
        || (body.contains(".storage()")
            && (body.contains(".set(")
                || body.contains(".remove(")
                || body.contains(".bump(")
                || body.contains(".extend_ttl(")))
}

fn parse_function_line(line: &str, line_num: u32) -> Option<FunctionInfo> {
    let is_public = line.starts_with("pub fn ") || line.starts_with("pub async fn ");
    let is_entry_point =
        line.contains("#[") && (line.contains("entry") || line.contains("constructor"));

    if !is_public && !is_entry_point {
        return None;
    }

    let func_start = line.find("fn ")? + 3;
    let rest = &line[func_start..];
    let name_end = rest.find('(')?;
    let name = rest[..name_end].trim().to_string();

    let params_start = rest.find('(')? + 1;
    let params_end = rest.find(')')?;
    let params_str = &rest[params_start..params_end];

    let params: Vec<ParamInfo> = params_str
        .split(',')
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() || p == "env" || p == "&self" || p == "&mut self" {
                return None;
            }
            let is_mut = p.contains("mut ");
            let is_ref = p.contains("&");
            let parts: Vec<&str> = p.split(':').collect();
            if parts.len() < 2 {
                return None;
            }
            let name = parts[0]
                .trim()
                .trim_start_matches("mut ")
                .trim()
                .to_string();
            let param_type = parts[1]
                .trim()
                .trim_start_matches('&')
                .trim_start_matches("mut ")
                .to_string();
            Some(ParamInfo {
                name,
                param_type,
                is_ref,
                is_mut,
            })
        })
        .collect();

    // Soroban contract functions mutate ledger state through `env`, never
    // through `&mut self`, so the signature has to carry the signal: a function
    // that returns nothing exists for its effects, and the conventional
    // state-changing verbs name the rest. `&mut self` still counts, for plain
    // Rust code.
    const MUTATING_VERBS: &[&str] = &[
        "set", "add", "remove", "delete", "update", "create", "init", "transfer", "mint", "burn",
        "approve", "deposit", "withdraw", "stake", "unstake", "claim", "vote", "execute",
        "upgrade", "write",
    ];
    let lower_name = name.to_lowercase();
    let is_mutating = line.contains("mut self")
        || !rest.contains("->")
        || MUTATING_VERBS
            .iter()
            .any(|verb| lower_name.starts_with(verb));
    let complexity = calculate_complexity(line);

    let return_type = if let Some(arrow_pos) = rest.find("->") {
        let after_arrow = &rest[arrow_pos + 2..];
        let type_end = after_arrow.find('{').unwrap_or(after_arrow.len());
        Some(after_arrow[..type_end].trim().to_string())
    } else {
        None
    };

    let is_entry_point = is_entry_point || name == "initialize" || name == "init";

    Some(FunctionInfo {
        name,
        signature: line.trim().to_string(),
        is_public,
        is_entry_point,
        is_mutating,
        params,
        return_type,
        complexity_score: complexity,
        line_start: line_num,
        line_end: line_num,
    })
}

fn calculate_complexity(line: &str) -> u32 {
    let mut score = 0u32;
    if line.contains("if ") {
        score += 1;
    }
    if line.contains("match ") {
        score += 2;
    }
    if line.contains("for ") {
        score += 1;
    }
    if line.contains("while ") {
        score += 1;
    }
    if line.contains("?.") {
        score += 1;
    }
    if line.contains("unwrap_or") {
        score += 1;
    }
    if line.contains("map_err") {
        score += 1;
    }
    score
}

fn extract_storage_accesses(source: &str) -> Vec<String> {
    let mut accesses = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        for marker in &[".set(", ".get(", ".has("] {
            if let Some(idx) = trimmed.find(marker) {
                let rest = &trimmed[idx + marker.len()..];
                if let Some(end_idx) = rest.find(')') {
                    let key = rest[..end_idx].trim().to_string();
                    if !key.is_empty() && !accesses.contains(&key) {
                        accesses.push(key);
                    }
                }
            }
        }
    }
    accesses
}

fn extract_external_calls(source: &str) -> Vec<String> {
    let mut calls = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.contains("client.") || trimmed.contains("invoke ") || trimmed.contains("deploy ")
        {
            calls.push(trimmed.to_string());
        }
    }
    calls
}

pub fn generate_test_priorities(analysis: &ContractAnalysis) -> Vec<TestPrioritySuggestion> {
    let mut suggestions = Vec::new();

    for func in &analysis.functions {
        let priority = if func.is_entry_point {
            TestPriority::Critical
        } else if func.is_mutating {
            TestPriority::High
        } else if func.complexity_score > 5 {
            TestPriority::Medium
        } else {
            TestPriority::Low
        };

        let mut test_types = Vec::new();
        test_types.push("unit".to_string());

        if func.is_entry_point {
            test_types.push("integration".to_string());
        }
        if func.is_mutating {
            test_types.push("edge_case".to_string());
            test_types.push("security".to_string());
        }
        if func.complexity_score > 3 {
            test_types.push("edge_case".to_string());
        }

        suggestions.push(TestPrioritySuggestion {
            function_name: func.name.clone(),
            priority,
            test_types,
            rationale: generate_rationale(func),
            estimated_tests_needed: estimate_tests_needed(func),
        });
    }

    suggestions
}

pub fn generate_edge_case_descriptions(func: &FunctionInfo) -> Vec<String> {
    let mut cases = Vec::new();
    for param in &func.params {
        match param.param_type.as_str() {
            t if t.contains("Address") => {
                cases.push(format!("Zero address for {}", param.name));
                cases.push(format!("Self-referencing address for {}", param.name));
                cases.push(format!("Contract address for {}", param.name));
            }
            t if t.contains("u64") || t.contains("i64") => {
                cases.push(format!("Zero value for {}", param.name));
                cases.push(format!("Maximum value for {}", param.name));
                cases.push(format!("Minimum positive value for {}", param.name));
            }
            t if t.contains("String") => {
                cases.push(format!("Empty string for {}", param.name));
                cases.push(format!("Maximum length string for {}", param.name));
                cases.push(format!("Special characters for {}", param.name));
            }
            _ => {
                cases.push(format!("Default value for {}", param.name));
            }
        }
    }
    if func.is_mutating {
        cases.push("Unauthorized caller".to_string());
        cases.push("Double spend / replay".to_string());
    }
    cases
}

pub fn generate_security_checks(func: &FunctionInfo) -> Vec<String> {
    let mut checks = Vec::new();
    if func.is_mutating {
        checks.push("Authorization required for state changes".to_string());
        checks.push("Failed auth must not mutate state".to_string());
        checks.push("Replay protection verified".to_string());
    }
    if func
        .params
        .iter()
        .any(|p| p.param_type.contains("i64") || p.param_type.contains("u64"))
    {
        checks.push("Overflow/underflow protection".to_string());
        checks.push("Negative amount handling".to_string());
    }
    checks.push("Input validation".to_string());
    checks
}

pub fn generate_warnings(analysis: &ContractAnalysis) -> Vec<String> {
    let mut warnings = Vec::new();
    if analysis.complex_functions > 3 {
        warnings.push(format!(
            "Contract has {} complex functions that may need additional test cases",
            analysis.complex_functions
        ));
    }
    if analysis.storage_accesses.len() > 5 {
        warnings
            .push("Contract has many storage accesses - ensure storage mock coverage".to_string());
    }
    if !analysis.external_calls.is_empty() {
        warnings.push("Contract makes external calls - consider integration tests".to_string());
    }
    warnings
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestPrioritySuggestion {
    pub function_name: String,
    pub priority: TestPriority,
    pub test_types: Vec<String>,
    pub rationale: String,
    pub estimated_tests_needed: u32,
}

fn generate_rationale(func: &FunctionInfo) -> String {
    if func.is_entry_point {
        format!("Entry point function '{}' requires comprehensive testing including unit, integration, and security tests", func.name)
    } else if func.is_mutating && func.complexity_score > 3 {
        format!("Complex mutating function '{}' has high risk of bugs and requires thorough edge case testing", func.name)
    } else if func.is_mutating {
        format!(
            "Mutating function '{}' should be tested for state changes and authorization",
            func.name
        )
    } else if func.complexity_score > 5 {
        format!(
            "Complex read-only function '{}' needs thorough branch coverage",
            func.name
        )
    } else {
        format!(
            "Standard function '{}' should have basic unit test coverage",
            func.name
        )
    }
}

fn estimate_tests_needed(func: &FunctionInfo) -> u32 {
    let base = 1;
    let param_bonus = func.params.len() as u32;
    let complexity_bonus = func.complexity_score;
    let mutating_bonus = if func.is_mutating { 2 } else { 0 };
    base + param_bonus + complexity_bonus + mutating_bonus
}

pub fn calculate_test_quality_score(test_code: &str, source_code: &str) -> TestQualityScore {
    let test_count = test_code.lines().filter(|l| l.contains("#[test]")).count();
    let assertion_count = test_code.lines().filter(|l| l.contains("assert")).count();
    let has_setup = test_code.contains("fn setup")
        || test_code.contains("#[ctor]")
        || test_code.contains("fn before");
    let has_edge_cases = test_code.contains("edge")
        || test_code.contains("boundary")
        || test_code.contains("zero")
        || test_code.contains("max");
    let has_security = test_code.contains("unauthorized")
        || test_code.contains("overflow")
        || test_code.contains("auth");
    let has_error_handling = test_code.contains("Err(")
        || test_code.contains("should_panic")
        || test_code.contains("unwrap_err");

    let function_count = source_code
        .lines()
        .filter(|l| l.trim().starts_with("pub fn "))
        .count();
    let test_to_func_ratio = if function_count > 0 {
        test_count as f64 / function_count as f64
    } else {
        0.0
    };

    let assertion_density = if test_count > 0 {
        assertion_count as f64 / test_count as f64
    } else {
        0.0
    };

    let mut score = 0.0;
    score += (test_to_func_ratio.min(3.0) / 3.0) * 30.0;
    score += (assertion_density.min(5.0) / 5.0) * 20.0;
    if has_setup {
        score += 10.0;
    }
    if has_edge_cases {
        score += 15.0;
    }
    if has_security {
        score += 15.0;
    }
    if has_error_handling {
        score += 10.0;
    }

    TestQualityScore {
        overall: score.min(100.0),
        test_count: test_count as u32,
        assertion_count: assertion_count as u32,
        test_to_func_ratio,
        assertion_density,
        has_setup,
        has_edge_cases,
        has_security,
        has_error_handling,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestQualityScore {
    pub overall: f64,
    pub test_count: u32,
    pub assertion_count: u32,
    pub test_to_func_ratio: f64,
    pub assertion_density: f64,
    pub has_setup: bool,
    pub has_edge_cases: bool,
    pub has_security: bool,
    pub has_error_handling: bool,
}

pub fn generate_mock_suggestions(contract_code: &str) -> Vec<MockSuggestion> {
    let mut suggestions = Vec::new();

    if contract_code.contains("Address") || contract_code.contains("address") {
        suggestions.push(MockSuggestion {
            mock_type: "address".to_string(),
            name: "MockAddress".to_string(),
            description: "Mock Stellar address for testing authorization and transfers".to_string(),
            priority: TestPriority::Critical,
        });
    }

    if contract_code.contains("storage") || contract_code.contains("Storage") {
        suggestions.push(MockSuggestion {
            mock_type: "storage".to_string(),
            name: "MockStorage".to_string(),
            description: "In-memory storage mock to simulate Soroban contract storage".to_string(),
            priority: TestPriority::High,
        });
    }

    if contract_code.contains("Env") || contract_code.contains("env") {
        suggestions.push(MockSuggestion {
            mock_type: "env".to_string(),
            name: "MockEnv".to_string(),
            description: "Mock Soroban environment for isolated contract testing".to_string(),
            priority: TestPriority::High,
        });
    }

    if contract_code.contains("client") || contract_code.contains("invoke") {
        suggestions.push(MockSuggestion {
            mock_type: "contract_client".to_string(),
            name: "MockContractClient".to_string(),
            description: "Mock contract client for testing cross-contract calls".to_string(),
            priority: TestPriority::Medium,
        });
    }

    if contract_code.contains("event") || contract_code.contains("Event") {
        suggestions.push(MockSuggestion {
            mock_type: "events".to_string(),
            name: "MockEventEmitter".to_string(),
            description: "Mock event emitter for testing contract event emission".to_string(),
            priority: TestPriority::Medium,
        });
    }

    suggestions
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockSuggestion {
    pub mock_type: String,
    pub name: String,
    pub description: String,
    pub priority: TestPriority,
}

pub fn generate_test_data_suggestions(contract_code: &str) -> Vec<TestDataSuggestion> {
    let mut suggestions = Vec::new();

    let functions = extract_functions_with_signatures(contract_code);

    for func in &functions {
        for param in &func.params {
            let suggestion = match param.param_type.as_str() {
                t if t.contains("Address") => TestDataSuggestion {
                    field: param.name.clone(),
                    data_type: "address".to_string(),
                    generators: vec![
                        "Address::random(&env)".to_string(),
                        "Address::from_string(&env, \"GA...\")".to_string(),
                    ],
                    edge_cases: vec![
                        "Zero address".to_string(),
                        "Self-referencing address".to_string(),
                        "Contract address vs account address".to_string(),
                    ],
                    description: format!(
                        "Address parameter '{}' needs test accounts and contracts",
                        param.name
                    ),
                },
                t if t.contains("u64")
                    || t.contains("i64")
                    || t.contains("u32")
                    || t.contains("i32") =>
                {
                    TestDataSuggestion {
                        field: param.name.clone(),
                        data_type: "amount".to_string(),
                        generators: vec![
                            "0".to_string(),
                            "1".to_string(),
                            "u64::MAX".to_string(),
                            "1_000_000_000".to_string(),
                        ],
                        edge_cases: vec![
                            "Zero (0)".to_string(),
                            "Maximum value (u64::MAX)".to_string(),
                            "Minimum value (1 for unsigned)".to_string(),
                            "Large amount (1_000_000_000)".to_string(),
                        ],
                        description: format!(
                            "Numeric parameter '{}' needs boundary value testing",
                            param.name
                        ),
                    }
                }
                t if t.contains("String") || t.contains("string") => TestDataSuggestion {
                    field: param.name.clone(),
                    data_type: "string".to_string(),
                    generators: vec![
                        "\"test\".into()".to_string(),
                        "\"\".into()".to_string(),
                        "\"a\".repeat(1000).into()".to_string(),
                    ],
                    edge_cases: vec![
                        "Empty string".to_string(),
                        "Maximum length string".to_string(),
                        "Special characters".to_string(),
                        "Unicode characters".to_string(),
                    ],
                    description: format!(
                        "String parameter '{}' needs length and content edge case testing",
                        param.name
                    ),
                },
                _ => TestDataSuggestion {
                    field: param.name.clone(),
                    data_type: "custom".to_string(),
                    generators: vec!["Default::default()".to_string()],
                    edge_cases: vec!["Default value".to_string()],
                    description: format!(
                        "Custom type parameter '{}' needs tailored test data",
                        param.name
                    ),
                },
            };
            suggestions.push(suggestion);
        }
    }

    suggestions
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestDataSuggestion {
    pub field: String,
    pub data_type: String,
    pub generators: Vec<String>,
    pub edge_cases: Vec<String>,
    pub description: String,
}

// ─── Prompt builders for AI models ─────────────────────────────────────────────

pub fn build_generation_prompt(request: &TestGenerationRequest) -> String {
    let test_type_desc = match request.test_type {
        TestType::Unit => "unit tests that verify individual function behavior",
        TestType::Integration => {
            "integration tests that verify contract interactions and workflows"
        }
        TestType::EdgeCase => "edge case tests that verify boundary conditions and error handling",
        TestType::Security => {
            "security tests that verify authorization and protection against exploits"
        }
        TestType::All => "comprehensive tests covering unit, integration, edge cases, and security",
    };

    let focus = if request.focus_functions.is_empty() {
        "all public functions".to_string()
    } else {
        format!("specifically: {}", request.focus_functions.join(", "))
    };

    let coverage_hint = if let Some(ref cov) = request.coverage_data {
        format!(
            "\nCurrent coverage: {:.1}% (functions: {}/{}, lines: {}/{}, branches: {}/{}). \
             Focus on uncovered functions: {}.",
            (cov.functions_covered as f64 / cov.functions_total as f64 * 100.0),
            cov.functions_covered,
            cov.functions_total,
            cov.lines_covered,
            cov.lines_total,
            cov.branches_covered,
            cov.branches_total,
            cov.uncovered_functions.join(", ")
        )
    } else {
        String::new()
    };

    let existing_tests_hint = if let Some(ref tests) = request.existing_tests {
        format!(
            "\n\nExisting tests to avoid duplication:\n```rust\n{}\n```",
            tests
        )
    } else {
        String::new()
    };

    format!(
        "Generate {} for the Soroban smart contract '{}'.\n\n\
         Contract source:\n```rust\n{}\n```\n\n\
         Target functions: {}\n{}\n{}\n\n\
         Requirements:\n\
         1. Use the soroban-sdk test harness (#![cfg(test)] mod tests)\n\
         2. Each test must have a descriptive name starting with 'test_'\n\
         3. Include setup with Env::default() and generated test addresses\n\
         4. Use assert!, assert_eq!, and assert_ne! for verification\n\
         5. Cover happy paths, error conditions, and authorization checks\n\
         6. Add doc comments explaining test purpose\n\
         7. Return valid, compilable Rust code\n\n\
         Output format: Return ONLY the Rust test code, no markdown blocks.",
        test_type_desc,
        request.contract_name,
        request.contract_code,
        focus,
        coverage_hint,
        existing_tests_hint
    )
}

pub fn build_optimization_prompt(request: &TestOptimizationRequest) -> String {
    let goals_desc: Vec<&str> = request
        .optimization_goals
        .iter()
        .map(|g| match g {
            OptimizationGoal::ReduceDuplication => "reduce code duplication",
            OptimizationGoal::ImprovePerformance => "improve test execution performance",
            OptimizationGoal::IncreaseCoverage => "increase code coverage",
            OptimizationGoal::BetterAssertions => "add more specific and meaningful assertions",
            OptimizationGoal::SimplifySetup => "simplify test setup and fixture management",
            OptimizationGoal::All => "optimize all aspects",
        })
        .collect();

    format!(
        "Optimize the following Soroban contract test suite.\n\n\
         Goals: {}\n\n\
         Contract source:\n```rust\n{}\n```\n\n\
         Current tests:\n```rust\n{}\n```\n\n\
         Requirements:\n\
         1. Maintain all existing test coverage\n\
         2. Extract common setup into shared helpers\n\
         3. Reduce assertion duplication\n\
         4. Add missing edge case coverage\n\
         5. Improve assertion specificity\n\
         6. Return optimized, compilable Rust code\n\n\
         Output format: Return ONLY the optimized Rust test code.",
        goals_desc.join(", "),
        request.contract_code,
        request.test_code
    )
}

pub fn build_coverage_improvement_prompt(request: &CoverageAnalysisRequest) -> String {
    let coverage_pct = if request.coverage_data.functions_total > 0 {
        request.coverage_data.functions_covered as f64
            / request.coverage_data.functions_total as f64
            * 100.0
    } else {
        0.0
    };

    format!(
        "Analyze test coverage and suggest improvements for a Soroban smart contract.\n\n\
         Contract source:\n```rust\n{}\n```\n\n\
         Current tests:\n```rust\n{}\n```\n\n\
         Coverage data:\n\
         - Overall: {:.1}%\n\
         - Functions: {}/{}\n\
         - Lines: {}/{}\n\
         - Branches: {}/{}\n\
         - Uncovered functions: {}\n\n\
         For each uncovered or under-tested function, provide:\n\
         1. What test scenario is missing\n\
         2. The specific test code to add\n\
         3. Expected coverage improvement\n\
         4. Difficulty level (easy/medium/hard)\n\n\
         Prioritize suggestions by impact on overall coverage.",
        request.source_code,
        request.test_code,
        coverage_pct,
        request.coverage_data.functions_covered,
        request.coverage_data.functions_total,
        request.coverage_data.lines_covered,
        request.coverage_data.lines_total,
        request.coverage_data.branches_covered,
        request.coverage_data.branches_total,
        request.coverage_data.uncovered_functions.join(", ")
    )
}

pub fn build_maintenance_prompt(request: &TestMaintenanceRequest) -> String {
    format!(
        "Analyze test maintenance issues for a Soroban smart contract.\n\n\
         Contract source ({}):\n```rust\n{}\n```\n\n\
         Test file ({}):\n```rust\n{}\n```\n\n\
         Identify:\n\
         1. Outdated tests that reference removed functions or changed signatures\n\
         2. Broken tests that will fail due to API changes\n\
         3. Missing tests for new or changed functions\n\
         4. Tests with poor quality (weak assertions, missing edge cases)\n\
         5. Tests that are redundant or duplicated\n\n\
         For each issue, provide:\n\
         - Test name or function name\n\
         - Specific problem description\n\
         - Severity (critical/high/medium/low)\n\
         - Suggested fix with code",
        request.source_path.display(),
        request.source_code,
        request.test_path.display(),
        request.test_code
    )
}

pub fn build_mock_generation_prompt(request: &MockGenerationRequest) -> String {
    let types_desc: Vec<&str> = request
        .mock_types
        .iter()
        .map(|t| match t {
            MockType::Address => "Stellar address mocks",
            MockType::Storage => "Storage mocks",
            MockType::Contract => "Contract client mocks",
            MockType::Env => "Environment mocks",
            MockType::Events => "Event emitter mocks",
            MockType::All => "all mock types",
        })
        .collect();

    format!(
        "Generate mock objects for testing a Soroban smart contract.\n\n\
         Contract source:\n```rust\n{}\n```\n\n\
         Mock types needed: {}\n\n\
         For each mock type, generate:\n\
         1. A struct definition with necessary fields\n\
         2. Implementation with common test methods\n\
         3. Builder pattern or constructor for easy setup\n\
         4. Description of what it mocks and when to use it\n\n\
         Requirements:\n\
         - Use #[cfg(test)] for test-only code\n\
         - Follow Soroban SDK conventions\n\
         - Make mocks configurable and reusable\n\
         - Include common test scenarios in usage examples",
        request.contract_code,
        types_desc.join(", ")
    )
}

pub fn build_test_data_prompt(request: &TestDataGenerationRequest) -> String {
    let types_desc: Vec<&str> = request
        .data_types
        .iter()
        .map(|t| match t {
            DataType::Address => "Stellar addresses",
            DataType::Amount => "token amounts and balances",
            DataType::String => "string inputs",
            DataType::Bytes => "byte arrays",
            DataType::Timestamp => "timestamps",
            DataType::Boolean => "boolean flags",
            DataType::Custom(name) => name,
            DataType::All => "all types",
        })
        .collect();

    format!(
        "Generate test data for a Soroban smart contract.\n\n\
         Contract source:\n```rust\n{}\n```\n\n\
         Data types needed: {}\n\
         Count per type: {}\n\
         Constraints: {}\n\n\
         For each data type, generate:\n\
         1. Valid test values (normal, boundary, stress)\n\
         2. Invalid test values (for error testing)\n\
         3. Edge cases (zero, max, min, empty)\n\
         4. A generator function for random test data\n\
         5. Comments explaining each value's purpose",
        request.contract_code,
        types_desc.join(", "),
        request.count_per_type,
        request
            .constraints
            .iter()
            .map(|c| format!("{}: {} = {:?}", c.field, c.constraint_type, c.value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

// ─── Source code parsing helpers ───────────────────────────────────────────────

pub fn read_source_file(path: &Path) -> Result<String> {
    if path.is_dir() {
        let lib_path = path.join("src/lib.rs");
        let main_path = path.join("src/main.rs");

        if lib_path.exists() {
            return fs::read_to_string(&lib_path)
                .with_context(|| format!("Failed to read {}", lib_path.display()));
        }
        if main_path.exists() {
            return fs::read_to_string(&main_path)
                .with_context(|| format!("Failed to read {}", main_path.display()));
        }

        anyhow::bail!("No src/lib.rs or src/main.rs found in {}", path.display());
    } else if path.is_file() {
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))
    } else {
        anyhow::bail!("Path does not exist: {}", path.display())
    }
}

pub fn find_test_files(project_path: &Path) -> Vec<PathBuf> {
    let mut test_files = Vec::new();

    // Check tests/ directory
    let tests_dir = project_path.join("tests");
    if tests_dir.exists() {
        if let Ok(entries) = fs::read_dir(&tests_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "rs") {
                    test_files.push(path);
                }
            }
        }
    }

    // Check src/ for #[cfg(test)] modules
    let src_dir = project_path.join("src");
    if src_dir.exists() {
        if let Ok(entries) = fs::read_dir(&src_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "rs") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if content.contains("#[cfg(test)]") {
                            test_files.push(path);
                        }
                    }
                }
            }
        }
    }

    test_files
}

pub fn detect_test_framework(test_code: &str) -> TestFramework {
    if test_code.contains("soroban_sdk") {
        TestFramework::Soroban
    } else if test_code.contains("#[test]") {
        TestFramework::Rust
    } else {
        TestFramework::Unknown
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestFramework {
    Soroban,
    Rust,
    Unknown,
}

// ─── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyze_contract_finds_public_functions() {
        let source = r#"
            #![no_std]
            use soroban_sdk::{contract, contractimpl, Env, Address};

            #[contract]
            pub struct Token;

            #[contractimpl]
            impl Token {
                pub fn initialize(env: Env, admin: Address) {
                    env.storage().persistent().set(&"admin", &admin);
                }

                pub fn transfer(env: Env, from: Address, to: Address, amount: i64) {
                    from.require_auth();
                    // transfer logic
                }

                pub fn balance(env: Env, account: Address) -> i64 {
                    env.storage().persistent().get(&account).unwrap_or(0)
                }
            }
        "#;

        let analysis = analyze_contract_for_testing(source).unwrap();
        assert!(analysis.total_functions >= 3);
        assert!(analysis.public_functions >= 3);
        assert!(analysis.mutating_functions >= 1);
        assert!(analysis.read_only_functions >= 1);
    }

    #[test]
    fn test_quality_score_calculates_correctly() {
        let test_code = r#"
            #[cfg(test)]
            mod tests {
                use super::*;

                #[test]
                fn test_initialize() {
                    let env = Env::default();
                    let admin = Address::random(&env);
                    let contract = Token::new(&env);
                    contract.initialize(&admin);
                    assert_eq!(contract.admin(), admin);
                }

                #[test]
                fn test_transfer() {
                    let env = Env::default();
                    let from = Address::random(&env);
                    let to = Address::random(&env);
                    let contract = Token::new(&env);
                    contract.transfer(&from, &to, &100);
                }

                #[test]
                fn test_transfer_edge_zero() {
                    let env = Env::default();
                    let from = Address::random(&env);
                    let to = Address::random(&env);
                    let contract = Token::new(&env);
                    contract.transfer(&from, &to, &0);
                }

                #[test]
                fn test_unauthorized_transfer() {
                    let env = Env::default();
                    let from = Address::random(&env);
                    let to = Address::random(&env);
                    let contract = Token::new(&env);
                    // Should fail without auth
                    contract.transfer(&from, &to, &100);
                }
            }
        "#;

        let source = "pub fn initialize(env: Env, admin: Address) {}\npub fn transfer(env: Env, from: Address, to: Address, amount: i64) {}";

        let score = calculate_test_quality_score(test_code, source);
        assert!(score.test_count >= 4);
        assert!(score.assertion_count >= 1);
        assert!(score.has_edge_cases);
        assert!(score.overall > 0.0);
    }

    #[test]
    fn generate_priorities_for_mutating_functions() {
        let analysis = ContractAnalysis {
            total_functions: 2,
            public_functions: 2,
            entry_points: 1,
            mutating_functions: 1,
            read_only_functions: 1,
            complex_functions: 0,
            functions: vec![
                FunctionInfo {
                    name: "initialize".to_string(),
                    signature: "pub fn initialize(env: Env, admin: Address)".to_string(),
                    is_public: true,
                    is_entry_point: true,
                    is_mutating: false,
                    params: vec![ParamInfo {
                        name: "admin".to_string(),
                        param_type: "Address".to_string(),
                        is_ref: false,
                        is_mut: false,
                    }],
                    return_type: None,
                    complexity_score: 1,
                    line_start: 1,
                    line_end: 3,
                },
                FunctionInfo {
                    name: "transfer".to_string(),
                    signature: "pub fn transfer(env: Env, from: Address, to: Address, amount: i64)"
                        .to_string(),
                    is_public: true,
                    is_entry_point: false,
                    is_mutating: true,
                    params: vec![
                        ParamInfo {
                            name: "from".to_string(),
                            param_type: "Address".to_string(),
                            is_ref: false,
                            is_mut: false,
                        },
                        ParamInfo {
                            name: "to".to_string(),
                            param_type: "Address".to_string(),
                            is_ref: false,
                            is_mut: false,
                        },
                        ParamInfo {
                            name: "amount".to_string(),
                            param_type: "i64".to_string(),
                            is_ref: false,
                            is_mut: false,
                        },
                    ],
                    return_type: None,
                    complexity_score: 2,
                    line_start: 5,
                    line_end: 8,
                },
            ],
            storage_accesses: vec![],
            external_calls: vec![],
        };

        let priorities = generate_test_priorities(&analysis);
        assert_eq!(priorities.len(), 2);

        let init_priority = priorities
            .iter()
            .find(|p| p.function_name == "initialize")
            .unwrap();
        assert_eq!(init_priority.priority, TestPriority::Critical);

        let transfer_priority = priorities
            .iter()
            .find(|p| p.function_name == "transfer")
            .unwrap();
        assert!(transfer_priority
            .test_types
            .contains(&"security".to_string()));
    }

    #[test]
    fn mock_suggestions_detect_needed_mocks() {
        let contract_code = r#"
            use soroban_sdk::{contract, contractimpl, Env, Address};

            #[contract]
            pub struct Token;

            #[contractimpl]
            impl Token {
                pub fn transfer(env: Env, from: Address, to: Address, amount: i64) {
                    from.require_auth();
                    env.storage().persistent().set(&from, &amount);
                }
            }
        "#;

        let suggestions = generate_mock_suggestions(contract_code);
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.mock_type == "address"));
        assert!(suggestions.iter().any(|s| s.mock_type == "storage"));
    }

    #[test]
    fn test_data_suggestions_for_amounts() {
        let contract_code = r#"
            pub fn transfer(env: Env, from: Address, to: Address, amount: i64) {}
            pub fn get_balance(env: Env, account: Address) -> u64 {}
        "#;

        let suggestions = generate_test_data_suggestions(contract_code);
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.data_type == "amount"));
        assert!(suggestions.iter().any(|s| s.data_type == "address"));
    }

    #[test]
    fn detect_test_framework_soroban() {
        let code = "use soroban_sdk::tests::Env;";
        assert_eq!(detect_test_framework(code), TestFramework::Soroban);
    }

    #[test]
    fn detect_test_framework_rust() {
        let code = "#[test] fn it_works() {}";
        assert_eq!(detect_test_framework(code), TestFramework::Rust);
    }

    #[test]
    fn build_generation_prompt_includes_contract() {
        let request = TestGenerationRequest {
            source_path: PathBuf::from("src/lib.rs"),
            test_type: TestType::Unit,
            contract_name: "Token".to_string(),
            contract_code: "pub fn transfer() {}".to_string(),
            existing_tests: None,
            coverage_data: None,
            focus_functions: vec![],
        };

        let prompt = build_generation_prompt(&request);
        assert!(prompt.contains("Token"));
        assert!(prompt.contains("pub fn transfer()"));
        assert!(prompt.contains("unit tests"));
    }

    #[test]
    fn build_generation_prompt_with_coverage_data() {
        let request = TestGenerationRequest {
            source_path: PathBuf::from("src/lib.rs"),
            test_type: TestType::All,
            contract_name: "Token".to_string(),
            contract_code: "pub fn transfer() {}".to_string(),
            existing_tests: None,
            coverage_data: Some(CoverageInput {
                functions_total: 10,
                functions_covered: 5,
                lines_total: 100,
                lines_covered: 50,
                branches_total: 20,
                branches_covered: 10,
                uncovered_functions: vec!["transfer".to_string(), "approve".to_string()],
            }),
            focus_functions: vec![],
        };

        let prompt = build_generation_prompt(&request);
        assert!(prompt.contains("50.0%"));
        assert!(prompt.contains("transfer, approve"));
    }

    #[test]
    fn build_optimization_prompt_includes_goals() {
        let request = TestOptimizationRequest {
            test_code: "#[test] fn test() {}".to_string(),
            contract_code: "pub fn main() {}".to_string(),
            optimization_goals: vec![
                OptimizationGoal::ReduceDuplication,
                OptimizationGoal::BetterAssertions,
            ],
        };

        let prompt = build_optimization_prompt(&request);
        assert!(prompt.contains("reduce code duplication"));
        assert!(prompt.contains("add more specific"));
    }

    #[test]
    fn build_maintenance_prompt_includes_paths() {
        let request = TestMaintenanceRequest {
            source_code: "pub fn new_func() {}".to_string(),
            test_code: "#[test] fn test_old() {}".to_string(),
            source_path: PathBuf::from("src/lib.rs"),
            test_path: PathBuf::from("tests/test.rs"),
        };

        let prompt = build_maintenance_prompt(&request);
        assert!(prompt.contains("src/lib.rs"));
        assert!(prompt.contains("tests/test.rs"));
    }
}
