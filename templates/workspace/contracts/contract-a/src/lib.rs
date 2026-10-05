#![no_std]
use shared_types::UserConfig;
use soroban_sdk::{contract, contractimpl, Env, Symbol};

#[contract]
pub struct ContractA;

pub trait ContractATrait {
    fn do_something(env: Env, config: UserConfig) -> Symbol;
}

#[contractimpl]
impl ContractATrait for ContractA {
    fn do_something(env: Env, config: UserConfig) -> Symbol {
        soroban_sdk::symbol_short!("doneA")
    }
}
