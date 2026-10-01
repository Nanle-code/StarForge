#![no_std]
use shared_types::UserConfig;
use soroban_sdk::{contract, contractimpl, Address, Env, Symbol};
use contract_a::ContractAClient;

#[contract]
pub struct ContractB;

#[contractimpl]
impl ContractB {
    pub fn do_something_else(env: Env, config: UserConfig, contract_a: Address) -> Symbol {
        let client = ContractAClient::new(&env, &contract_a);
        client.do_something(&config);
        soroban_sdk::symbol_short!("doneB")
    }
}
