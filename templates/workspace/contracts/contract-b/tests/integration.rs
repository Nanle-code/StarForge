#![cfg(test)]
use contract_a::{ContractA, ContractAClient};
use contract_b::{ContractB, ContractBClient};
use shared_types::UserConfig;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

#[test]
fn test_cross_contract() {
    let env = Env::default();
    
    // Register Contract A
    let contract_a_id = env.register_contract(None, ContractA);
    let client_a = ContractAClient::new(&env, &contract_a_id);
    
    // Register Contract B
    let contract_b_id = env.register_contract(None, ContractB);
    let client_b = ContractBClient::new(&env, &contract_b_id);
    
    let config = UserConfig {
        account: Address::generate(&env),
        name: String::from_str(&env, "Alice"),
    };
    
    let result = client_b.do_something_else(&config, &contract_a_id);
    assert_eq!(result, soroban_sdk::symbol_short!("doneB"));
}
