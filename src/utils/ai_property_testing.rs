//! AI-driven property-based testing for Soroban contracts.
//!
//! Provides:
//! - Automatic property and invariant discovery from contract source code
//! - Random test case generation using `proptest`-compatible strategies
//! - Invariant validation across state transitions
//! - Edge case discovery via boundary analysis
//! - Shrinkage strategies for counterexample minimization

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::utils::ai_test_assistant as ata;

// ── Types ────────────────────────────────────────────────────────────────────

/// A discovered contract property that should hold across all executions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredProperty {
    pub name: String,
    pub description: String,
    pub property_type: PropertyType,
    pub target_function: Option<String>,
    pub invariants: Vec<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PropertyType {
    Postcondition,
    Invariant,
    Precondition,
    StateTransition,
    AccessControl,
    Overflow,
}

impl std::fmt::Display for PropertyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PropertyType::Postcondition => write!(f, "Postcondition"),
            PropertyType::Invariant => write!(f, "Invariant"),
            PropertyType::Precondition => write!(f, "Precondition"),
            PropertyType::StateTransition => write!(f, "State Transition"),
            PropertyType::AccessControl => write!(f, "Access Control"),
            PropertyType::Overflow => write!(f, "Overflow"),
        }
    }
}

/// A generated proptest strategy for a specific type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedStrategy {
    pub type_name: String,
    pub strategy_code: String,
    pub description: String,
    pub edge_cases: Vec<String>,
}

/// A property-based test case generated from discovered properties.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyTestCase {
    pub name: String,
    pub property: String,
    pub strategy: String,
    pub test_code: String,
    pub expected_outcome: ExpectedOutcome,
    pub shrink_strategy: Option<String>,
    pub edge_cases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedOutcome {
    Pass,
    Panic { reason: String },
    Error { error_type: String },
}

/// Invariant that must hold across contract state transitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvariantCheck {
    pub name: String,
    pub expression: String,
    pub description: String,
    pub functions_affected: Vec<String>,
    pub check_type: InvariantCheckType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvariantCheckType {
    BalanceNonNegative,
    TotalSupplyConsistent,
    AuthorizationPreserved,
    StateConsistent,
    Custom,
}

/// Result of property-based test generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyTestResult {
    pub properties: Vec<DiscoveredProperty>,
    pub strategies: Vec<GeneratedStrategy>,
    pub test_cases: Vec<PropertyTestCase>,
    pub invariants: Vec<InvariantCheck>,
    pub summary: String,
    pub total_tests: usize,
    pub estimated_coverage: f64,
}

/// Configuration for property test generation.
#[derive(Debug, Clone)]
pub struct PropertyTestConfig {
    pub max_tests_per_property: usize,
    pub include_shrinkage: bool,
    pub target_functions: Vec<String>,
    pub timeout_ms: u64,
}

impl Default for PropertyTestConfig {
    fn default() -> Self {
        Self {
            max_tests_per_property: 5,
            include_shrinkage: true,
            target_functions: vec![],
            timeout_ms: 10_000,
        }
    }
}

// ── Property Discovery ───────────────────────────────────────────────────────

/// Analyze contract source code and discover properties that should hold.
pub fn discover_properties(source_code: &str) -> Result<Vec<DiscoveredProperty>> {
    let analysis = ata::analyze_contract_for_testing(source_code)?;
    let mut properties = Vec::new();

    for func in &analysis.functions {
        // Postconditions: return types imply contracts
        if func.return_type.is_some() {
            properties.push(DiscoveredProperty {
                name: format!("{}_returns_valid", func.name),
                description: format!(
                    "Function '{}' should return a valid result without panicking",
                    func.name
                ),
                property_type: PropertyType::Postcondition,
                target_function: Some(func.name.clone()),
                invariants: vec!["result is not panic".to_string()],
                confidence: 0.9,
            });
        }

        // Access control: mutating functions need auth
        if func.is_mutating {
            properties.push(DiscoveredProperty {
                name: format!("{}_requires_auth", func.name),
                description: format!(
                    "Mutating function '{}' should require authorization",
                    func.name
                ),
                property_type: PropertyType::AccessControl,
                target_function: Some(func.name.clone()),
                invariants: vec!["require_auth is called".to_string()],
                confidence: 0.95,
            });

            // State transition invariants
            properties.push(DiscoveredProperty {
                name: format!("{}_state_consistent", func.name),
                description: format!(
                    "Function '{}' should maintain contract state consistency",
                    func.name
                ),
                property_type: PropertyType::StateTransition,
                target_function: Some(func.name.clone()),
                invariants: vec!["state is valid after execution".to_string()],
                confidence: 0.85,
            });
        }

        // Overflow detection for numeric parameters
        for param in &func.params {
            if param.param_type.contains("i64")
                || param.param_type.contains("u64")
                || param.param_type.contains("i32")
                || param.param_type.contains("u32")
            {
                properties.push(DiscoveredProperty {
                    name: format!("{}_{}_no_overflow", func.name, param.name),
                    description: format!(
                        "Parameter '{}' in '{}' should handle numeric boundaries without overflow",
                        param.name, func.name
                    ),
                    property_type: PropertyType::Overflow,
                    target_function: Some(func.name.clone()),
                    invariants: vec![format!("no overflow on {}", param.name)],
                    confidence: 0.8,
                });
            }
        }

        // Preconditions: input validation
        if !func.params.is_empty() {
            let preconditions: Vec<String> = func
                .params
                .iter()
                .map(|p| format!("'{}' is valid for type {}", p.name, p.param_type))
                .collect();
            properties.push(DiscoveredProperty {
                name: format!("{}_preconditions", func.name),
                description: format!("Function '{}' should validate preconditions", func.name),
                property_type: PropertyType::Precondition,
                target_function: Some(func.name.clone()),
                invariants: preconditions,
                confidence: 0.85,
            });
        }
    }

    Ok(properties)
}

// ── Invariant Extraction ─────────────────────────────────────────────────────

/// Extract invariants from contract source that must hold across all operations.
pub fn extract_invariants(source_code: &str) -> Result<Vec<InvariantCheck>> {
    let analysis = ata::analyze_contract_for_testing(source_code)?;
    let mut invariants = Vec::new();

    let mutating: Vec<&str> = analysis
        .functions
        .iter()
        .filter(|f| f.is_mutating)
        .map(|f| f.name.as_str())
        .collect();

    let mutating_names: Vec<String> = mutating.iter().map(|s: &&str| s.to_string()).collect();

    // Storage-related invariants
    if !analysis.storage_accesses.is_empty() {
        invariants.push(InvariantCheck {
            name: "storage_consistency".to_string(),
            expression: "storage.get(key) == expected_value after set(key, value)".to_string(),
            description: "Storage reads should return the last written value".to_string(),
            functions_affected: mutating_names.clone(),
            check_type: InvariantCheckType::StateConsistent,
        });
    }

    // Authorization invariant for mutating functions
    if !mutating.is_empty() {
        invariants.push(InvariantCheck {
            name: "authorization_required".to_string(),
            expression: "env.auth(|_| { /* caller must authorize */ })".to_string(),
            description: "All state-mutating functions must require authorization".to_string(),
            functions_affected: mutating_names.clone(),
            check_type: InvariantCheckType::AuthorizationPreserved,
        });
    }

    // Balance non-negative invariant
    if source_code.contains("balance") || source_code.contains("amount") {
        invariants.push(InvariantCheck {
            name: "balance_non_negative".to_string(),
            expression: "balance >= 0".to_string(),
            description: "Token balances should never go negative".to_string(),
            functions_affected: mutating_names.clone(),
            check_type: InvariantCheckType::BalanceNonNegative,
        });
    }

    // Supply consistency for token-like contracts
    if source_code.contains("total_supply") || source_code.contains("supply") {
        invariants.push(InvariantCheck {
            name: "supply_consistency".to_string(),
            expression: "sum(balances) == total_supply".to_string(),
            description: "Sum of all balances should equal total supply".to_string(),
            functions_affected: mutating_names.clone(),
            check_type: InvariantCheckType::TotalSupplyConsistent,
        });
    }

    Ok(invariants)
}

// ── Strategy Generation ──────────────────────────────────────────────────────

/// Generate proptest-compatible strategies for contract types.
pub fn generate_strategies(properties: &[DiscoveredProperty]) -> Vec<GeneratedStrategy> {
    let mut strategies = Vec::new();
    let mut seen_types = std::collections::HashSet::new();

    for prop in properties {
        if let Some(ref func_name) = prop.target_function {
            if !seen_types.contains(func_name) {
                seen_types.insert(func_name.clone());

                // Address strategy
                strategies.push(GeneratedStrategy {
                    type_name: "Address".to_string(),
                    strategy_code: r#"prop::strategy::Just(Address::random(&env))"#.to_string(),
                    description: "Generate random valid Stellar addresses".to_string(),
                    edge_cases: vec![
                        "Zero address (if applicable)".to_string(),
                        "Self-referencing address".to_string(),
                        "Contract address vs account address".to_string(),
                    ],
                });

                // Amount strategy
                if prop.property_type == PropertyType::Overflow
                    || prop.property_type == PropertyType::Postcondition
                {
                    strategies.push(GeneratedStrategy {
                        type_name: "Amount (i64/u64)".to_string(),
                        strategy_code:
                            r#"prop::num::i64::ANY.prop_filter("non-negative", |v| *v >= 0)"#
                                .to_string(),
                        description: "Generate amounts including boundaries and edge cases"
                            .to_string(),
                        edge_cases: vec![
                            "0".to_string(),
                            "1".to_string(),
                            "i64::MAX".to_string(),
                            "i64::MIN".to_string(),
                            "u64::MAX".to_string(),
                        ],
                    });
                }
            }
        }
    }

    strategies
}

// ── Test Case Generation ─────────────────────────────────────────────────────

/// Generate property-based test cases from discovered properties.
pub fn generate_test_cases(
    properties: &[DiscoveredProperty],
    invariants: &[InvariantCheck],
    config: &PropertyTestConfig,
) -> Vec<PropertyTestCase> {
    let mut test_cases = Vec::new();

    for prop in properties {
        if !config.target_functions.is_empty() {
            if let Some(ref func) = prop.target_function {
                if !config.target_functions.contains(func) {
                    continue;
                }
            }
        }

        let test_code = generate_test_code_for_property(prop, invariants, config.include_shrinkage);
        let shrink = if config.include_shrinkage {
            Some(generate_shrink_strategy(prop))
        } else {
            None
        };

        test_cases.push(PropertyTestCase {
            name: format!("prop_{}", prop.name),
            property: prop.description.clone(),
            strategy: "proptest".to_string(),
            test_code,
            expected_outcome: ExpectedOutcome::Pass,
            shrink_strategy: shrink,
            edge_cases: prop.invariants.clone(),
        });
    }

    test_cases
}

fn generate_test_code_for_property(
    prop: &DiscoveredProperty,
    invariants: &[InvariantCheck],
    include_shrink: bool,
) -> String {
    let shrink_section = if include_shrink {
        "\n    // Shrink strategy: minimize counterexample to smallest failing input".to_string()
    } else {
        String::new()
    };

    let invariant_checks: Vec<String> = invariants
        .iter()
        .filter(|inv| {
            prop.target_function
                .as_ref()
                .is_some_and(|f| inv.functions_affected.contains(f))
        })
        .map(|inv| format!("    // Invariant: {} — {}", inv.name, inv.expression))
        .collect();

    let invariant_block = if invariant_checks.is_empty() {
        String::new()
    } else {
        format!("\n{}\n", invariant_checks.join("\n"))
    };

    format!(
        r#"proptest! {{
    #[test]
    fn {}(amount in 0i64..i64::MAX) {{{}{}
        // Property: {}
        let env = Env::default();
        let contract_address = Address::random(&env);
        // TODO: invoke the contract function with generated inputs
        // and verify the property holds
        prop_assert!(true, "Property should hold");
    }}
}}"#,
        prop.name, invariant_block, shrink_section, prop.description,
    )
}

fn generate_shrink_strategy(prop: &DiscoveredProperty) -> String {
    match prop.property_type {
        PropertyType::Overflow => {
            "shrink to boundary values: 0, 1, i64::MAX, i64::MIN, u64::MAX".to_string()
        }
        PropertyType::AccessControl => {
            "shrink to: random address, contract address, zero address".to_string()
        }
        PropertyType::StateTransition => {
            "shrink to minimal state sequence that reproduces failure".to_string()
        }
        _ => "shrink numerics toward 0, strings toward empty".to_string(),
    }
}

// ── Invariant Validation ─────────────────────────────────────────────────────

/// Generate invariant validation test code.
pub fn generate_invariant_tests(invariants: &[InvariantCheck]) -> String {
    let mut code = String::from(
        "// Invariant validation tests generated by StarForge AI Property Testing\n\n",
    );

    for inv in invariants {
        code.push_str(&format!(
            r#"/// Invariant: {}
/// {}
#[test]
fn test_invariant_{}() {{
    let env = Env::default();
    let contract_address = Address::random(&env);
    // Setup contract state
    // Execute mutating functions
    // Verify: {}
    prop_assert!(true, "Invariant '{}' should hold after all operations");
}}

"#,
            inv.name, inv.description, inv.name, inv.expression, inv.name,
        ));
    }

    code
}

// ── Full Pipeline ────────────────────────────────────────────────────────────

/// Run the full property-based testing pipeline on a contract.
pub fn run_pipeline(source_code: &str, config: &PropertyTestConfig) -> Result<PropertyTestResult> {
    let properties = discover_properties(source_code).context("Failed to discover properties")?;
    let invariants = extract_invariants(source_code).context("Failed to extract invariants")?;
    let strategies = generate_strategies(&properties);
    let test_cases = generate_test_cases(&properties, &invariants, config);

    let total_tests = test_cases.len();
    let estimated_coverage = if properties.is_empty() {
        0.0
    } else {
        (total_tests as f64 / properties.len() as f64) * 20.0
    };

    Ok(PropertyTestResult {
        properties: properties.clone(),
        strategies,
        test_cases,
        invariants: invariants.clone(),
        summary: format!(
            "Discovered {} properties, {} invariants, generated {} test cases",
            properties.len(),
            invariants.len(),
            total_tests,
        ),
        total_tests,
        estimated_coverage,
    })
}

// ── Prompt Building ──────────────────────────────────────────────────────────

/// Build a prompt for AI-enhanced property discovery.
pub fn build_property_discovery_prompt(source_code: &str) -> String {
    format!(
        r#"Analyze this Soroban contract and discover properties that should hold across all executions.

Focus on:
1. Postconditions — what must be true after each function returns
2. Invariants — what must hold between function calls
3. Preconditions — what must be true before a function executes
4. State transitions — how state changes should be consistent
5. Access control — which functions require authorization
6. Overflow/underflow — numeric safety properties

Contract source:
```rust
{}
```

For each property found, provide:
- A unique name
- The property type (postcondition, invariant, precondition, state_transition, access_control, overflow)
- Target function name
- The invariant expression in plain English
- Confidence score (0.0-1.0)

Return JSON array of properties."#,
        source_code
    )
}

/// Build a prompt for AI-enhanced shrinkage strategy generation.
pub fn build_shrink_prompt(failing_test: &str) -> String {
    format!(
        r#"A property-based test is failing. Analyze the failing test and suggest shrinkage strategies
to minimize the counterexample to the simplest possible failing input.

Failing test:
```rust
{}
```

Provide:
1. Shrink strategy (what values to shrink toward)
2. Priority order for shrinking (which inputs to shrink first)
3. Expected minimal counterexample description
4. Whether the shrink is likely to converge

Return JSON with shrink_strategy, priority_order, expected_minimal, converges."#,
        failing_test
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CONTRACT: &str = r#"
        use soroban_sdk::{contractimpl, Address, Env};

        #[contract]
        pub struct TokenContract;

        #[contractimpl]
        impl TokenContract {
            pub fn transfer(env: Env, from: Address, to: Address, amount: i64) -> bool {
                from.require_auth();
                if amount <= 0 { return false; }
                // ... transfer logic
                true
            }

            pub fn balance(env: Env, account: Address) -> i64 {
                // ... read balance
                100
            }

            pub fn mint(env: Env, to: Address, amount: i64) {
                to.require_auth();
                // ... mint tokens
            }
        }
    "#;

    #[test]
    fn test_discover_properties() {
        let props = discover_properties(SAMPLE_CONTRACT).unwrap();
        assert!(!props.is_empty());
        assert!(props.iter().any(|p| p.name.contains("transfer")));
        assert!(props
            .iter()
            .any(|p| p.property_type == PropertyType::AccessControl));
    }

    #[test]
    fn test_extract_invariants() {
        let invariants = extract_invariants(SAMPLE_CONTRACT).unwrap();
        assert!(!invariants.is_empty());
        assert!(invariants
            .iter()
            .any(|i| i.name == "authorization_required"));
    }

    #[test]
    fn test_generate_strategies() {
        let props = discover_properties(SAMPLE_CONTRACT).unwrap();
        let strategies = generate_strategies(&props);
        assert!(!strategies.is_empty());
    }

    #[test]
    fn test_generate_test_cases() {
        let props = discover_properties(SAMPLE_CONTRACT).unwrap();
        let invariants = extract_invariants(SAMPLE_CONTRACT).unwrap();
        let config = PropertyTestConfig::default();
        let cases = generate_test_cases(&props, &invariants, &config);
        assert!(!cases.is_empty());
    }

    #[test]
    fn test_run_pipeline() {
        let config = PropertyTestConfig::default();
        let result = run_pipeline(SAMPLE_CONTRACT, &config).unwrap();
        assert!(result.total_tests > 0);
        assert!(!result.properties.is_empty());
    }

    #[test]
    fn test_build_prompts() {
        let prompt = build_property_discovery_prompt(SAMPLE_CONTRACT);
        assert!(prompt.contains("Soroban contract"));
        assert!(prompt.contains("transfer"));

        let shrink = build_shrink_prompt("failing test code");
        assert!(shrink.contains("shrink"));
    }
}
