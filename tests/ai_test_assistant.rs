use starforge::utils::ai_test_assistant as ata;
use std::path::PathBuf;

const SAMPLE_CONTRACT: &str = r#"
#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Env, Address, Symbol};

#[contracttype]
pub enum DataKey {
    Balance(Address),
    Admin,
    TotalSupply,
}

#[contract]
pub struct Token;

#[contractimpl]
impl Token {
    pub fn initialize(env: Env, admin: Address, total_supply: i64) {
        env.storage().persistent().set(&DataKey::Admin, &admin);
        env.storage().persistent().set(&DataKey::TotalSupply, &total_supply);
        env.storage().persistent().set(&DataKey::Balance(admin.clone()), &total_supply);
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i64) {
        from.require_auth();
        assert!(amount > 0, "amount must be positive");

        let from_balance: i64 = env.storage().persistent()
            .get(&DataKey::Balance(from.clone()))
            .unwrap_or(0);
        assert!(from_balance >= amount, "insufficient balance");

        env.storage().persistent().set(
            &DataKey::Balance(from),
            &(from_balance - amount),
        );

        let to_balance: i64 = env.storage().persistent()
            .get(&DataKey::Balance(to.clone()))
            .unwrap_or(0);
        env.storage().persistent().set(
            &DataKey::Balance(to),
            &(to_balance + amount),
        );
    }

    pub fn balance(env: Env, account: Address) -> i64 {
        env.storage().persistent()
            .get(&DataKey::Balance(account))
            .unwrap_or(0)
    }

    pub fn admin(env: Env) -> Address {
        env.storage().persistent().get(&DataKey::Admin).unwrap()
    }

    pub fn total_supply(env: Env) -> i64 {
        env.storage().persistent()
            .get(&DataKey::TotalSupply)
            .unwrap_or(0)
    }
}
"#;

const SAMPLE_TESTS: &str = r#"
#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::tests::Env;

    #[test]
    fn test_initialize() {
        let env = Env::default();
        let admin = Address::random(&env);
        let contract = Token::new(&env);
        contract.initialize(&admin, &1000);
        assert_eq!(contract.admin(), admin);
        assert_eq!(contract.total_supply(), 1000);
    }

    #[test]
    fn test_transfer() {
        let env = Env::default();
        let admin = Address::random(&env);
        let recipient = Address::random(&env);
        let contract = Token::new(&env);
        contract.initialize(&admin, &1000);
        contract.transfer(&admin, &recipient, &100);
        assert_eq!(contract.balance(admin), 900);
        assert_eq!(contract.balance(recipient), 100);
    }

    #[test]
    fn test_transfer_edge_zero() {
        let env = Env::default();
        let admin = Address::random(&env);
        let recipient = Address::random(&env);
        let contract = Token::new(&env);
        contract.initialize(&admin, &1000);
        contract.transfer(&admin, &recipient, &0);
    }
}
"#;

#[test]
fn contract_analysis_finds_all_functions() {
    let analysis = ata::analyze_contract_for_testing(SAMPLE_CONTRACT).unwrap();

    assert!(analysis.total_functions >= 5);
    assert!(analysis.public_functions >= 5);
    assert!(analysis.mutating_functions >= 1);
    assert!(analysis.read_only_functions >= 2);
}

#[test]
fn contract_analysis_detects_entry_points() {
    let analysis = ata::analyze_contract_for_testing(SAMPLE_CONTRACT).unwrap();
    assert!(analysis.entry_points >= 1);
}

#[test]
fn contract_analysis_detects_storage_accesses() {
    let analysis = ata::analyze_contract_for_testing(SAMPLE_CONTRACT).unwrap();
    assert!(!analysis.storage_accesses.is_empty());
}

#[test]
fn test_priorities_rank_entry_points_critical() {
    let analysis = ata::analyze_contract_for_testing(SAMPLE_CONTRACT).unwrap();
    let priorities = ata::generate_test_priorities(&analysis);

    let init_priority = priorities.iter().find(|p| p.function_name == "initialize");
    assert!(init_priority.is_some());
    assert_eq!(init_priority.unwrap().priority, ata::TestPriority::Critical);
}

#[test]
fn test_priorities_rank_mutating_functions() {
    let analysis = ata::analyze_contract_for_testing(SAMPLE_CONTRACT).unwrap();
    let priorities = ata::generate_test_priorities(&analysis);

    let transfer_priority = priorities.iter().find(|p| p.function_name == "transfer");
    assert!(transfer_priority.is_some());
    assert!(
        transfer_priority.unwrap().priority == ata::TestPriority::High
            || transfer_priority.unwrap().priority == ata::TestPriority::Critical
    );
}

#[test]
fn test_priorities_include_security_tests() {
    let analysis = ata::analyze_contract_for_testing(SAMPLE_CONTRACT).unwrap();
    let priorities = ata::generate_test_priorities(&analysis);

    let transfer_priority = priorities.iter().find(|p| p.function_name == "transfer");
    assert!(transfer_priority.is_some());
    assert!(transfer_priority
        .unwrap()
        .test_types
        .contains(&"security".to_string()));
}

#[test]
fn test_quality_score_basic() {
    let score = ata::calculate_test_quality_score(SAMPLE_TESTS, SAMPLE_CONTRACT);

    assert!(score.test_count >= 3);
    assert!(score.assertion_count >= 3);
    assert!(score.overall > 0.0);
    assert!(score.has_edge_cases);
}

#[test]
fn test_quality_score_empty_tests() {
    let score = ata::calculate_test_quality_score("", SAMPLE_CONTRACT);

    assert_eq!(score.test_count, 0);
    assert_eq!(score.assertion_count, 0);
    assert_eq!(score.overall, 0.0);
    assert!(!score.has_setup);
    assert!(!score.has_edge_cases);
    assert!(!score.has_security);
}

#[test]
fn mock_suggestions_for_contract_with_address_and_storage() {
    let suggestions = ata::generate_mock_suggestions(SAMPLE_CONTRACT);

    assert!(!suggestions.is_empty());
    assert!(suggestions.iter().any(|s| s.mock_type == "address"));
    assert!(suggestions.iter().any(|s| s.mock_type == "storage"));
}

#[test]
fn test_data_suggestions_for_numeric_and_address_params() {
    let suggestions = ata::generate_test_data_suggestions(SAMPLE_CONTRACT);

    assert!(!suggestions.is_empty());
    assert!(suggestions.iter().any(|s| s.data_type == "amount"));
    assert!(suggestions.iter().any(|s| s.data_type == "address"));
}

#[test]
fn build_generation_prompt_includes_all_parts() {
    let request = ata::TestGenerationRequest {
        source_path: PathBuf::from("src/lib.rs"),
        test_type: ata::TestType::All,
        contract_name: "Token".to_string(),
        contract_code: SAMPLE_CONTRACT.to_string(),
        existing_tests: Some(SAMPLE_TESTS.to_string()),
        coverage_data: Some(ata::CoverageInput {
            functions_total: 5,
            functions_covered: 3,
            lines_total: 50,
            lines_covered: 30,
            branches_total: 10,
            branches_covered: 6,
            uncovered_functions: vec!["transfer".to_string()],
        }),
        focus_functions: vec!["transfer".to_string()],
    };

    let prompt = ata::build_generation_prompt(&request);
    assert!(prompt.contains("Token"));
    assert!(prompt.contains("transfer"));
    assert!(prompt.contains("60.0%"));
    assert!(prompt.contains("comprehensive tests"));
}

#[test]
fn build_optimization_prompt_includes_goals() {
    let request = ata::TestOptimizationRequest {
        test_code: SAMPLE_TESTS.to_string(),
        contract_code: SAMPLE_CONTRACT.to_string(),
        optimization_goals: vec![
            ata::OptimizationGoal::ReduceDuplication,
            ata::OptimizationGoal::BetterAssertions,
        ],
    };

    let prompt = ata::build_optimization_prompt(&request);
    assert!(prompt.contains("reduce code duplication"));
    assert!(prompt.contains("add more specific"));
    assert!(prompt.contains("Token"));
}

#[test]
fn build_coverage_improvement_prompt() {
    let request = ata::CoverageAnalysisRequest {
        source_code: SAMPLE_CONTRACT.to_string(),
        test_code: SAMPLE_TESTS.to_string(),
        coverage_data: ata::CoverageInput {
            functions_total: 5,
            functions_covered: 3,
            lines_total: 50,
            lines_covered: 30,
            branches_total: 10,
            branches_covered: 6,
            uncovered_functions: vec!["transfer".to_string(), "admin".to_string()],
        },
    };

    let prompt = ata::build_coverage_improvement_prompt(&request);
    assert!(prompt.contains("60.0%"));
    assert!(prompt.contains("transfer, admin"));
}

#[test]
fn build_maintenance_prompt_includes_file_paths() {
    let request = ata::TestMaintenanceRequest {
        source_code: SAMPLE_CONTRACT.to_string(),
        test_code: SAMPLE_TESTS.to_string(),
        source_path: PathBuf::from("src/lib.rs"),
        test_path: PathBuf::from("tests/token.rs"),
    };

    let prompt = ata::build_maintenance_prompt(&request);
    assert!(prompt.contains("src/lib.rs"));
    assert!(prompt.contains("tests/token.rs"));
}

#[test]
fn build_mock_generation_prompt() {
    let request = ata::MockGenerationRequest {
        contract_code: SAMPLE_CONTRACT.to_string(),
        mock_types: vec![ata::MockType::Address, ata::MockType::Storage],
    };

    let prompt = ata::build_mock_generation_prompt(&request);
    assert!(prompt.contains("Stellar address mocks"));
    assert!(prompt.contains("Storage mocks"));
}

#[test]
fn build_test_data_prompt() {
    let request = ata::TestDataGenerationRequest {
        contract_code: SAMPLE_CONTRACT.to_string(),
        data_types: vec![ata::DataType::Address, ata::DataType::Amount],
        count_per_type: 10,
        constraints: vec![],
    };

    let prompt = ata::build_test_data_prompt(&request);
    assert!(prompt.contains("Stellar addresses"));
    assert!(prompt.contains("token amounts"));
    assert!(prompt.contains("10"));
}

#[test]
fn detect_test_framework_soroban() {
    assert_eq!(
        ata::detect_test_framework("use soroban_sdk::tests::Env;"),
        ata::TestFramework::Soroban
    );
}

#[test]
fn detect_test_framework_rust() {
    assert_eq!(
        ata::detect_test_framework("#[test] fn it_works() {}"),
        ata::TestFramework::Rust
    );
}

#[test]
fn detect_test_framework_unknown() {
    assert_eq!(
        ata::detect_test_framework("fn helper() {}"),
        ata::TestFramework::Unknown
    );
}

#[test]
fn test_generation_request_serialization_roundtrip() {
    let request = ata::TestGenerationRequest {
        source_path: PathBuf::from("src/lib.rs"),
        test_type: ata::TestType::EdgeCase,
        contract_name: "Token".to_string(),
        contract_code: "code".to_string(),
        existing_tests: None,
        coverage_data: None,
        focus_functions: vec!["transfer".to_string()],
    };

    let json = serde_json::to_string(&request).unwrap();
    let deserialized: ata::TestGenerationRequest = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.contract_name, "Token");
    assert_eq!(deserialized.test_type, ata::TestType::EdgeCase);
    assert!(deserialized
        .focus_functions
        .contains(&"transfer".to_string()));
}

#[test]
fn test_optimization_request_serialization_roundtrip() {
    let request = ata::TestOptimizationRequest {
        test_code: "#[test] fn test() {}".to_string(),
        contract_code: "pub fn main() {}".to_string(),
        optimization_goals: vec![ata::OptimizationGoal::All],
    };

    let json = serde_json::to_string(&request).unwrap();
    let deserialized: ata::TestOptimizationRequest = serde_json::from_str(&json).unwrap();

    assert_eq!(
        deserialized.optimization_goals,
        vec![ata::OptimizationGoal::All]
    );
}

#[test]
fn coverage_input_serialization_roundtrip() {
    let coverage = ata::CoverageInput {
        functions_total: 10,
        functions_covered: 7,
        lines_total: 100,
        lines_covered: 70,
        branches_total: 20,
        branches_covered: 14,
        uncovered_functions: vec!["transfer".to_string(), "approve".to_string()],
    };

    let json = serde_json::to_string(&coverage).unwrap();
    let deserialized: ata::CoverageInput = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.functions_total, 10);
    assert_eq!(deserialized.functions_covered, 7);
    assert_eq!(deserialized.uncovered_functions.len(), 2);
}

#[test]
fn empty_contract_analysis() {
    let analysis = ata::analyze_contract_for_testing("fn helper() {}").unwrap();
    assert_eq!(analysis.total_functions, 0);
    assert_eq!(analysis.public_functions, 0);
}

#[test]
fn complex_function_detection() {
    let source = r#"
        pub fn complex_func(env: Env, x: i64) -> i64 {
            if x > 0 {
                match x {
                    1 => 10,
                    2 => 20,
                    _ => 30,
                }
            } else {
                for i in 0..x {
                    let _ = i;
                }
                0
            }
        }
    "#;

    let analysis = ata::analyze_contract_for_testing(source).unwrap();
    assert_eq!(analysis.total_functions, 1);
    assert!(analysis.functions[0].complexity_score > 3);
}

#[test]
fn priority_estimates_reasonable_test_counts() {
    let analysis = ata::analyze_contract_for_testing(SAMPLE_CONTRACT).unwrap();
    let priorities = ata::generate_test_priorities(&analysis);

    for suggestion in &priorities {
        assert!(suggestion.estimated_tests_needed > 0);
        assert!(suggestion.estimated_tests_needed <= 20);
    }
}

#[test]
fn test_edge_case_descriptions_for_address_params() {
    let func = ata::FunctionInfo {
        name: "transfer".to_string(),
        signature: "pub fn transfer(env: Env, from: Address, to: Address, amount: i64)".to_string(),
        is_public: true,
        is_entry_point: false,
        is_mutating: true,
        params: vec![
            ata::ParamInfo {
                name: "from".to_string(),
                param_type: "Address".to_string(),
                is_ref: false,
                is_mut: false,
            },
            ata::ParamInfo {
                name: "to".to_string(),
                param_type: "Address".to_string(),
                is_ref: false,
                is_mut: false,
            },
            ata::ParamInfo {
                name: "amount".to_string(),
                param_type: "i64".to_string(),
                is_ref: false,
                is_mut: false,
            },
        ],
        return_type: None,
        complexity_score: 3,
        line_start: 1,
        line_end: 10,
    };

    let cases = ata::generate_edge_case_descriptions(&func);
    assert!(cases.len() >= 6);
    assert!(cases.iter().any(|c| c.contains("Zero address")));
    assert!(cases.iter().any(|c| c.contains("Maximum value")));
    assert!(cases.iter().any(|c| c.contains("Unauthorized")));
}

#[test]
fn test_security_checks_for_mutating_functions() {
    let func = ata::FunctionInfo {
        name: "transfer".to_string(),
        signature: "pub fn transfer(env: Env, from: Address, to: Address, amount: i64)".to_string(),
        is_public: true,
        is_entry_point: false,
        is_mutating: true,
        params: vec![ata::ParamInfo {
            name: "amount".to_string(),
            param_type: "i64".to_string(),
            is_ref: false,
            is_mut: false,
        }],
        return_type: None,
        complexity_score: 2,
        line_start: 1,
        line_end: 5,
    };

    let checks = ata::generate_security_checks(&func);
    assert!(checks.iter().any(|c| c.contains("Authorization")));
    assert!(checks.iter().any(|c| c.contains("Overflow")));
}

#[test]
fn warnings_generated_for_complex_contracts() {
    let complex_contract = r#"
        pub fn a(env: Env) -> bool { if true { true } else { false } }
        pub fn b(env: Env) -> bool { match 1 { 0 => false, 1 => true, _ => false } }
        pub fn c(env: Env) -> bool { for i in 0..10 { let _ = i; } true }
        pub fn d(env: Env) -> i64 { let x = env.storage().get(&"k").unwrap_or(0); x }
        pub fn e(env: Env) -> i64 { let x = env.storage().get(&"k").unwrap_or(0); x }
        pub fn f(env: Env) -> i64 { env.storage().get(&"k").unwrap_or(0) }
        pub fn g(env: Env) -> i64 { env.storage().get(&"k").unwrap_or(0) }
    "#;

    let analysis = ata::analyze_contract_for_testing(complex_contract).unwrap();
    let warnings = ata::generate_warnings(&analysis);
    assert!(!warnings.is_empty());
}

#[test]
fn all_test_type_variants_serde() {
    let types = vec![
        ata::TestType::Unit,
        ata::TestType::Integration,
        ata::TestType::EdgeCase,
        ata::TestType::Security,
        ata::TestType::All,
    ];

    for test_type in &types {
        let json = serde_json::to_string(test_type).unwrap();
        let deserialized: ata::TestType = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, test_type);
    }
}

#[test]
fn all_priority_variants_serde() {
    let priorities = vec![
        ata::TestPriority::Critical,
        ata::TestPriority::High,
        ata::TestPriority::Medium,
        ata::TestPriority::Low,
    ];

    for priority in &priorities {
        let json = serde_json::to_string(priority).unwrap();
        let deserialized: ata::TestPriority = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, priority);
    }
}
